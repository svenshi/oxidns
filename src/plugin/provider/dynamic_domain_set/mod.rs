// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime-writable domain rule set provider.
//!
//! `dynamic_domain_set` is intentionally separate from `domain_set`: the
//! latter stays a read-only composition provider, while this plugin owns one
//! machine-managed local rule file and exposes append/remove/clear operations.
//! Hot-path matching remains read-only through an `ArcSwap` snapshot.

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tracing::info;

use self::backend::DynamicDomainSetBackend;
use self::config::DynamicDomainSetConfig;
pub(crate) use self::rules::{DynamicDomainRuleKind, learned_rule_for_domain};
use crate::config::types::PluginConfig;
use crate::infra::error::Result as DnsResult;
use crate::plugin::provider::Provider;
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;
use crate::proto::{Name, Question};

#[cfg(feature = "api")]
mod api;
mod backend;
mod config;
mod rules;
mod storage;

#[cfg(all(test, feature = "api"))]
mod tests;

/// Provider handle registered in the plugin registry.
///
/// The backend is kept behind an `Arc` because API handlers and `learn_domain`
/// can outlive the factory call path and need the same mutation interface.
#[derive(Debug)]
pub struct DynamicDomainSet {
    tag: String,
    backend: Arc<DynamicDomainSetBackend>,
}

impl DynamicDomainSet {
    pub(crate) fn append_rules_async(
        &self,
        rules: Vec<String>,
        default_kind: DynamicDomainRuleKind,
    ) -> DnsResult<rules::DynamicDomainMutation> {
        self.backend.append_rules_async(rules, default_kind)
    }

    pub(crate) async fn append_rules_sync(
        &self,
        rules: Vec<String>,
        default_kind: DynamicDomainRuleKind,
        timeout: Duration,
    ) -> DnsResult<rules::DynamicDomainMutation> {
        self.backend
            .append_rules_sync(rules, default_kind, timeout)
            .await
    }
}

#[async_trait]
impl Plugin for DynamicDomainSet {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> DnsResult<()> {
        self.backend.start().await
    }

    async fn destroy(&self) -> DnsResult<()> {
        self.backend.shutdown().await
    }
}

#[async_trait]
impl Provider for DynamicDomainSet {
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline]
    #[hotpath::measure]
    fn contains_name(&self, name: &Name) -> bool {
        self.backend.contains_name(name)
    }

    #[inline]
    #[hotpath::measure]
    fn contains_question(&self, question: &Question) -> bool {
        self.backend.contains_question(question)
    }

    async fn reload(&self) -> DnsResult<()> {
        self.backend.reload().await
    }

    fn supports_domain_matching(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("dynamic_domain_set")]
pub struct DynamicDomainSetFactory;

impl PluginFactory for DynamicDomainSetFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> DnsResult<UninitializedPlugin> {
        let config = DynamicDomainSetConfig::from_plugin_config(plugin_config)?;
        info!(
            tag = %plugin_config.tag,
            path = %config.path.display(),
            queue_size = config.queue_size,
            batch_size = config.batch_size,
            flush_interval_ms = config.flush_interval_ms,
            "dynamic_domain_set configured"
        );
        let backend = Arc::new(DynamicDomainSetBackend::new(
            plugin_config.tag.clone(),
            config,
        ));
        Ok(UninitializedPlugin::Provider(Box::new(DynamicDomainSet {
            tag: plugin_config.tag.clone(),
            backend,
        })))
    }
}
