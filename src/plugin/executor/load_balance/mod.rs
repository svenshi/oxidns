// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Selects one executor subcall per invocation, with round-robin (default),
//! random, or weighted-random scheduling. Configuration uses `strategy` and a
//! non-empty `executors: [{ exec: plain_tag, weight: 1 }]` list. Dependencies
//! resolve at startup; their lifecycle remains owned by the plugin runtime.
//! Each selected executor runs on the caller's context without an outer
//! continuation. Middleware belongs inside a complete target sequence.
//! Selection adds no tasks or context clones. Errors and Stop propagate;
//! natural completion and Return resume the caller. Owned metrics register
//! during initialization and unregister during destruction.

mod config;
mod metrics;
mod selection;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use async_trait::async_trait;

use self::config::Config;
use self::metrics::LoadBalanceMetrics;
use self::selection::Selector;
use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::Result;
use crate::infra::observability::metrics::{register_metric_source, unregister_metric_source};
use crate::plugin::dependency::DependencySpec;
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::{Plugin, PluginFactory, PluginInitContext, UninitializedPlugin};
use crate::plugin_factory;

#[derive(Debug)]
struct LoadBalance {
    targets: Vec<Arc<dyn Executor>>,
    selector: Selector,
    metrics: Arc<LoadBalanceMetrics>,
}

#[async_trait]
impl Plugin for LoadBalance {
    fn tag(&self) -> &str {
        &self.metrics.tag
    }

    async fn init(&mut self, _context: &PluginInitContext<'_>) -> Result<()> {
        register_metric_source(self.metrics.clone())
    }

    async fn destroy(&self) -> Result<()> {
        unregister_metric_source(self.tag());
        Ok(())
    }
}

#[async_trait]
impl Executor for LoadBalance {
    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        let index = self.selector.select();
        let metrics = &self.metrics.targets[index];
        metrics.selected.fetch_add(1, Ordering::Relaxed);
        match self.targets[index].execute_with_next(context, None).await {
            Ok(ExecStep::Next | ExecStep::Return) => Ok(ExecStep::Next),
            Ok(ExecStep::Stop) => Ok(ExecStep::Stop),
            Err(error) => {
                metrics.errors.fetch_add(1, Ordering::Relaxed);
                Err(error)
            }
        }
    }
}

#[derive(Debug)]
#[plugin_factory("load_balance")]
pub struct LoadBalanceFactory;

impl PluginFactory for LoadBalanceFactory {
    fn get_dependency_specs(&self, plugin: &PluginConfig) -> Vec<DependencySpec> {
        // Extract structurally valid references even if other config fields
        // are invalid, so graph validation retains field-specific diagnostics.
        plugin
            .args
            .as_ref()
            .and_then(|args| args.get("executors"))
            .and_then(|targets| targets.as_sequence())
            .map(|targets| {
                targets
                    .iter()
                    .enumerate()
                    .filter_map(|(index, target)| {
                        target
                            .get("exec")
                            .and_then(|value| value.as_str())
                            .map(|tag| {
                                DependencySpec::executor(
                                    format!("args.executors[{index}].exec"),
                                    tag,
                                )
                            })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn create(
        &self,
        plugin: &PluginConfig,
        context: &PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let config = Config::parse(plugin)?;
        let selector = Selector::new(
            config.strategy,
            config.executors.iter().map(|target| target.weight),
        )?;
        let targets = config
            .executors
            .iter()
            .enumerate()
            .map(|(index, target)| {
                context.executor(&format!("args.executors[{index}].exec"), &target.exec)
            })
            .collect::<Result<Vec<_>>>()?;
        let metrics = Arc::new(LoadBalanceMetrics::new(
            plugin.tag.clone(),
            config.executors.into_iter().map(|target| target.exec),
        ));
        Ok(UninitializedPlugin::Executor(Box::new(LoadBalance {
            targets,
            selector,
            metrics,
        })))
    }
}

#[cfg(test)]
mod tests;
