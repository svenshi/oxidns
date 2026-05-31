// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use ahash::AHashSet;
use async_trait::async_trait;
use tracing::warn;

use super::config::{
    LearnDomainConfig, LearnErrorMode, LearnPhase, QuestionMode, build_config,
    parse_provider_from_value,
};
use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::core::error::{DnsError, Result};
use crate::plugin::dependency::DependencySpec;
use crate::plugin::executor::{ExecStep, Executor, ExecutorNext};
use crate::plugin::provider::Provider;
use crate::plugin::provider::dynamic_domain_set::{DynamicDomainSet, learned_rule_for_domain};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::proto::Rcode;
use crate::{continue_next, plugin_factory};

#[derive(Debug)]
pub(super) struct LearnDomainExecutor {
    pub(super) tag: String,
    pub(super) config: LearnDomainConfig,
    /// Stored as the provider trait object so plugin ownership stays with the
    /// registry. Each write downcasts to the concrete provider type after init
    /// has validated the dependency kind and plugin type.
    pub(super) provider: Option<Arc<dyn Provider>>,
}

#[async_trait]
impl Plugin for LearnDomainExecutor {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        // DependencySpec already enforces the tag and plugin type during graph
        // analysis. The downcast here is an extra guard and gives the executor a
        // concrete mutation API without extending the generic Provider trait.
        let provider = context.provider_of_type(
            "args.provider",
            &self.config.provider_tag,
            "dynamic_domain_set",
        )?;
        if provider
            .as_any()
            .downcast_ref::<DynamicDomainSet>()
            .is_none()
        {
            return Err(DnsError::plugin(format!(
                "learn_domain '{}' requires provider '{}' to be dynamic_domain_set",
                self.tag, self.config.provider_tag
            )));
        }
        self.provider = Some(provider);
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Executor for LearnDomainExecutor {
    fn with_next(&self) -> bool {
        true
    }

    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        self.execute_with_next(context, None).await
    }

    #[hotpath::measure]
    async fn execute_with_next(
        &self,
        context: &mut DnsContext,
        next: Option<ExecutorNext>,
    ) -> Result<ExecStep> {
        match self.config.phase {
            LearnPhase::Before => {
                // Before-phase learning sees only the request. This is useful
                // for "observe this branch" policies, but it intentionally
                // skips success/answer checks because no response exists yet.
                let learn_result = self.learn_context(context).await;
                match learn_result {
                    Ok(()) => continue_next!(next, context),
                    Err(err) => self.handle_learn_failure(err, None, next, context).await,
                }
            }
            LearnPhase::After => {
                // After-phase preserves downstream behavior first. Learning is
                // a side effect layered on the successful downstream result, so
                // downstream errors are returned unchanged.
                let next_result = continue_next!(next, context);
                match next_result {
                    Ok(step) => {
                        let learn_result = self.learn_context(context).await;
                        match learn_result {
                            Ok(()) => Ok(step),
                            Err(err) => {
                                self.handle_learn_failure(err, Some(step), None, context)
                                    .await
                            }
                        }
                    }
                    Err(err) => Err(err),
                }
            }
        }
    }
}

impl LearnDomainExecutor {
    async fn learn_context(&self, context: &DnsContext) -> Result<()> {
        let rules = self.extract_rules(context)?;
        if rules.is_empty() {
            return Ok(());
        }
        let provider = self.dynamic_provider()?;
        if self.config.async_mode {
            // Async mode is the default for request latency: enqueue only, then
            // let `dynamic_domain_set` batch and flush in the background.
            provider
                .append_rules_async(rules, self.config.rule_kind)
                .map(|_| ())
        } else {
            // Sync mode is intended for tests or strict policies that need the
            // learned rule to be visible before the current sequence continues.
            provider
                .append_rules_sync(rules, self.config.rule_kind, self.config.timeout)
                .await
                .map(|_| ())
        }
    }

    fn dynamic_provider(&self) -> Result<&DynamicDomainSet> {
        let provider = self.provider.as_ref().ok_or_else(|| {
            DnsError::plugin(format!(
                "learn_domain '{}' provider is not initialized",
                self.tag
            ))
        })?;
        provider
            .as_any()
            .downcast_ref::<DynamicDomainSet>()
            .ok_or_else(|| {
                DnsError::plugin(format!(
                    "learn_domain '{}' provider '{}' is not dynamic_domain_set",
                    self.tag, self.config.provider_tag
                ))
            })
    }

    pub(super) fn extract_rules(&self, context: &DnsContext) -> Result<Vec<String>> {
        if self.config.phase == LearnPhase::After {
            let Some(response) = context.response() else {
                return Ok(Vec::new());
            };
            // The default after-phase behavior learns only domains that
            // resolved successfully and produced usable answers. This avoids
            // persisting typo, NXDOMAIN, and upstream-failure traffic.
            if self.config.success_only && response.rcode() != Rcode::NoError {
                return Ok(Vec::new());
            }
            if self.config.answer_required && response.answers().is_empty() {
                return Ok(Vec::new());
            }
        }

        let questions = context.request().questions();
        let selected = match self.config.questions {
            QuestionMode::First => questions.iter().take(1).collect::<Vec<_>>(),
            QuestionMode::All => questions.iter().collect::<Vec<_>>(),
        };
        let mut rules = Vec::new();
        let mut seen = AHashSet::new();
        for question in selected {
            if !self.config.qtypes.contains(&u16::from(question.qtype())) {
                continue;
            }
            // Only the request qname is learned in v1. CNAME target learning is
            // intentionally left out because it broadens policy side effects and
            // needs separate operator controls.
            let rule =
                learned_rule_for_domain(question.name().normalized(), self.config.rule_kind)?;
            if seen.insert(rule.clone()) {
                rules.push(rule);
            }
        }
        Ok(rules)
    }

    async fn handle_learn_failure(
        &self,
        err: DnsError,
        success_step: Option<ExecStep>,
        next: Option<ExecutorNext>,
        context: &mut DnsContext,
    ) -> Result<ExecStep> {
        match self.config.error_mode {
            LearnErrorMode::Continue => {
                warn!(
                    plugin = %self.tag,
                    provider = %self.config.provider_tag,
                    error = %err,
                    "learn_domain failed; continuing"
                );
                // Preserve a successful downstream step in after-phase; in
                // before-phase there is no downstream result yet, so continue
                // into `next` as if the side effect had been skipped.
                if let Some(step) = success_step {
                    Ok(step)
                } else {
                    continue_next!(next, context)
                }
            }
            LearnErrorMode::Stop => {
                warn!(
                    plugin = %self.tag,
                    provider = %self.config.provider_tag,
                    error = %err,
                    "learn_domain failed; stopping"
                );
                Ok(ExecStep::Stop)
            }
            LearnErrorMode::Fail => Err(DnsError::plugin(format!(
                "learn_domain '{}' failed: {}",
                self.tag, err
            ))),
        }
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("learn_domain")]
pub struct LearnDomainFactory;

impl PluginFactory for LearnDomainFactory {
    fn get_dependency_specs(&self, plugin_config: &PluginConfig) -> Vec<DependencySpec> {
        // Declare the concrete provider type in the dependency graph. This
        // catches accidental references to read-only `domain_set` at startup
        // before any DNS request reaches the executor.
        parse_provider_from_value(plugin_config.args.clone())
            .map(|provider| {
                vec![DependencySpec::provider_type(
                    "args.provider",
                    provider,
                    "dynamic_domain_set",
                )]
            })
            .unwrap_or_default()
    }

    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let config = build_config(plugin_config)?;
        Ok(UninitializedPlugin::Executor(Box::new(
            LearnDomainExecutor {
                tag: plugin_config.tag.clone(),
                config,
                provider: None,
            },
        )))
    }
}
