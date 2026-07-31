// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, warn};

use super::is_timeout_error;
use super::metrics::ForwardMetrics;
use crate::core::context::{DnsContext, ExecutionPathEvent};
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::upstream::Upstream;
use crate::infra::observability::metrics::{register_metric_source, unregister_metric_source};
use crate::plugin::Plugin;
use crate::plugin::executor::{ExecStep, Executor};

/// Single-upstream DNS forwarder
///
/// Forwards DNS queries to a single configured upstream server.
/// Handles timeouts and logs errors appropriately.
#[allow(unused)]
#[derive(Debug)]
pub(super) struct SingleDnsForwarder {
    /// Plugin identifier
    pub(super) tag: String,

    /// Upstream DNS resolver
    pub(super) upstream: Box<dyn Upstream>,

    /// Whether to stop the executor chain after a successful upstream response.
    pub(super) short_circuit: bool,

    pub(super) metrics: Arc<ForwardMetrics>,
}

#[async_trait]
impl Plugin for SingleDnsForwarder {
    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        info!("DNS SingleDnsForwarder initialized tag: {}", self.tag);
        register_metric_source(self.metrics.clone())
    }

    async fn destroy(&self) -> Result<()> {
        unregister_metric_source(&self.tag);
        Ok(())
    }
}

#[async_trait]
impl Executor for SingleDnsForwarder {
    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        let start_ms = self.metrics.record_query_start();
        self.metrics.record_upstream_start(0);
        let diagnostic_start = AppClock::elapsed_millis();
        let member_tag = self
            .upstream
            .connection_info()
            .tag
            .clone()
            .unwrap_or_else(|| "index:0".to_string());
        match self.upstream.query(context.request.clone()).await {
            Ok(res) => {
                context.set_response(res);
                self.metrics.record_success(start_ms);
                self.metrics.record_upstream_success(0, start_ms);
                self.record_attempt(context, member_tag, "selected", diagnostic_start);
            }
            Err(e) => {
                let timeout = is_timeout_error(&e);
                self.metrics.record_error(start_ms, timeout);
                self.metrics.record_upstream_error(0, start_ms, timeout);
                self.record_attempt(
                    context,
                    member_tag,
                    if timeout {
                        "timeout"
                    } else {
                        "transport_error"
                    },
                    diagnostic_start,
                );
                warn!(
                    "DNS query failed - source: {}, queries: {:?}, id: {}, reason: {}",
                    context.peer_addr(),
                    context.request.questions(),
                    context.request.id(),
                    e
                );
                return Err(DnsError::plugin(format!(
                    "forward plugin '{}' query failed: {}",
                    self.tag, e
                )));
            }
        }
        Ok(self.completion_step())
    }
}

impl SingleDnsForwarder {
    fn record_attempt(
        &self,
        context: &mut DnsContext,
        member_tag: String,
        outcome: &'static str,
        started_ms: u64,
    ) {
        if !context.execution_path_enabled() {
            return;
        }
        context.push_execution_path_event(
            ExecutionPathEvent::new(
                self.tag.clone(),
                None,
                "upstream",
                Some(member_tag),
                outcome,
            )
            .with_timing(
                None,
                Some(
                    AppClock::elapsed_millis()
                        .saturating_sub(started_ms)
                        .saturating_mul(1000),
                ),
            )
            .with_detail([("index", "0")]),
        );
    }

    #[inline]
    fn completion_step(&self) -> ExecStep {
        if self.short_circuit {
            ExecStep::Stop
        } else {
            ExecStep::Next
        }
    }
}
