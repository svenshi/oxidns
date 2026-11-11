// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Filters request EDNS options within the remaining sequence continuation.
//!
//! Config: `policy`, `allowlist`, `denylist`, and `restore_request` (default
//! true). Restoration replaces the entire request on exit, including
//! cancellation; response and other context state remain untouched. Use a child
//! sequence to limit the scope to upstream resolution. Without a continuation,
//! restoring execution leaves the request unchanged; non-restoring execution
//! filters it.
//!
//! Rules are compiled at creation. No dependencies, background work, or I/O;
//! only restoring execution clones the request for a per-invocation snapshot.

mod config;
mod filter;

use async_trait::async_trait;

use self::config::EdnsFilterConfig;
use self::filter::EdnsOptionsFilter;
use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result};
use crate::plugin::executor::{ExecStep, Executor, ExecutorNext};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::proto::Message;
use crate::{continue_next, plugin_factory};

#[derive(Debug)]
struct EdnsFilter {
    tag: String,
    options: EdnsOptionsFilter,
    restore_request: bool,
}

impl EdnsFilter {
    fn new(tag: String, config: EdnsFilterConfig) -> Self {
        Self {
            tag,
            restore_request: config.restore_request,
            options: config.into(),
        }
    }
}

#[async_trait]
impl Plugin for EdnsFilter {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Executor for EdnsFilter {
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
        if self.restore_request {
            // The guard must span the await even when no option is rejected:
            // downstream stages may change any part of the request.
            let guard = RequestRestoreGuard::new(context);
            self.options.apply(&mut guard.context.request);
            continue_next!(next, guard.context)
        } else {
            self.options.apply(&mut context.request);
            continue_next!(next, context)
        }
    }
}

/// Borrow the same context, restoring only its request when this scope ends.
struct RequestRestoreGuard<'a> {
    context: &'a mut DnsContext,
    original: Option<Message>,
}

impl<'a> RequestRestoreGuard<'a> {
    fn new(context: &'a mut DnsContext) -> Self {
        let original = Some(context.request.clone());
        Self { context, original }
    }
}

impl Drop for RequestRestoreGuard<'_> {
    fn drop(&mut self) {
        if let Some(original) = self.original.take() {
            self.context.request = original;
        }
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("edns_filter")]
pub struct EdnsFilterFactory;

impl PluginFactory for EdnsFilterFactory {
    fn create(
        &self,
        config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let args = config
            .args
            .clone()
            .map(serde_yaml_ng::from_value::<EdnsFilterConfig>)
            .transpose()
            .map_err(|error| {
                DnsError::plugin(format!("failed to parse edns_filter config: {error}"))
            })?
            .unwrap_or_default();
        Ok(UninitializedPlugin::Executor(Box::new(EdnsFilter::new(
            config.tag.clone(),
            args,
        ))))
    }
}

#[cfg(test)]
mod tests;
