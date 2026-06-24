// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use async_trait::async_trait;
use rand::RngExt;
use tokio::task::JoinSet;
use tracing::{Level, debug, event_enabled, info, warn};

use super::is_timeout_error;
use super::metrics::ForwardMetrics;
use super::selection::{ResponseSelectionMode, select_response};
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::upstream::Upstream;
use crate::infra::observability::metrics::{register_metric_source, unregister_metric_source};
use crate::plugin::Plugin;
use crate::plugin::executor::{ExecStep, Executor};
use crate::proto::Message;

#[derive(Debug)]
pub(super) struct ConcurrentForwarder {
    /// Plugin identifier
    pub(super) tag: String,

    /// Fixed active upstream fanout, computed at creation time.
    pub(super) active_concurrent: usize,

    pub(super) upstreams: Vec<Arc<dyn Upstream>>,

    /// Whether to stop the executor chain after a successful upstream response.
    pub(super) short_circuit: bool,

    pub(super) response_selection: ResponseSelectionMode,

    pub(super) metrics: Arc<ForwardMetrics>,
}

#[async_trait]
impl Plugin for ConcurrentForwarder {
    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        info!("DNS ConcurrentForwarder initialized tag: {}", self.tag);
        register_metric_source(self.metrics.clone())
    }

    async fn destroy(&self) -> Result<()> {
        unregister_metric_source(&self.tag);
        Ok(())
    }
}

#[async_trait]
impl Executor for ConcurrentForwarder {
    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        let start_ms = self.metrics.record_query_start();
        let (response, last_error, timed_out) = self.query_upstreams(context.request.clone()).await;
        if let Some(response) = response {
            context.set_response(response);
            self.metrics.record_success(start_ms);
            return Ok(self.completion_step());
        }

        let err = last_error.unwrap_or_else(|| "no upstream response".to_string());
        self.metrics.record_error(start_ms, timed_out);
        warn!(
            "forward plugin '{}' failed across all concurrent upstreams: {}",
            self.tag, err
        );
        Err(DnsError::plugin(format!(
            "forward plugin '{}' failed across all concurrent upstreams: {}",
            self.tag, err
        )))
    }
}

impl ConcurrentForwarder {
    #[inline]
    fn completion_step(&self) -> ExecStep {
        if self.short_circuit {
            ExecStep::Stop
        } else {
            ExecStep::Next
        }
    }

    async fn query_upstreams(&self, request: Message) -> (Option<Message>, Option<String>, bool) {
        let total_upstreams = self.upstreams.len();
        if total_upstreams == 0 {
            return (None, Some("no upstream configured".to_string()), false);
        }

        let mut join_set = JoinSet::new();
        let start_idx = rand::rng().random_range(0..total_upstreams);

        for i in 0..self.active_concurrent {
            let selected_idx = (start_idx + i) % total_upstreams;
            let upstream = self.upstreams[selected_idx].clone();
            let message = request.clone();
            let metrics = self.metrics.clone();
            join_set.spawn(async move {
                let up_start = metrics.record_upstream_start(selected_idx);
                let result: Result<Message> = upstream.query(message).await;
                match &result {
                    Ok(_) => metrics.record_upstream_success(selected_idx, up_start),
                    Err(e) => {
                        metrics.record_upstream_error(selected_idx, up_start, is_timeout_error(e))
                    }
                }
                if event_enabled!(Level::DEBUG) {
                    debug!(
                        "DNS ConcurrentForwarder received message {}, remote_addr: {}",
                        selected_idx,
                        upstream.connection_info().raw_addr
                    );
                }
                result
            });
        }

        select_response(
            &mut join_set,
            self.active_concurrent,
            self.response_selection,
        )
        .await
    }
}
