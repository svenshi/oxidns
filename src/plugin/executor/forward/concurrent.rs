// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use async_trait::async_trait;
use rand::RngExt;
use tokio::task::JoinSet;
use tracing::{Level, debug, event_enabled, info, warn};

use super::is_timeout_error;
use super::metrics::ForwardMetrics;
use super::selection::{ResponseSelectionMode, SelectedResponse, select_response};
use crate::core::context::DnsContext;
use crate::core::response::ResponseDisposition;
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
        let (response, last_error, timed_out) = self
            .query_upstreams(
                context.request.clone(),
                context.observation.clone(),
                context.observation_parent,
            )
            .await;
        if let Some(selected) = response {
            if selected.disposition == Some(ResponseDisposition::IncompleteAlias) {
                self.metrics.record_incomplete_alias_selected();
            }
            context.set_response(selected.message);
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

    async fn query_upstreams(
        &self,
        request: Message,
        observation: Option<Arc<crate::core::observation::Observation>>,
        parent: Option<usize>,
    ) -> (Option<SelectedResponse>, Option<String>, bool) {
        let total_upstreams = self.upstreams.len();
        if total_upstreams == 0 {
            return (None, Some("no upstream configured".to_string()), false);
        }

        let mut join_set = JoinSet::new();
        let start_idx = rand::rng().random_range(0..total_upstreams);
        let mut spans = observation.as_ref().map(|trace| {
            self.upstreams
                .iter()
                .map(|upstream| {
                    let address = &upstream.connection_info().raw_addr;
                    trace.span(
                        parent,
                        "upstream",
                        &self.tag,
                        address,
                        address
                            .split_once("://")
                            .map(|(scheme, _)| scheme)
                            .unwrap_or("udp"),
                    )
                })
                .collect::<Vec<_>>()
        });
        let event_ids = spans.as_ref().map(|spans| {
            spans
                .iter()
                .map(|s| s.as_ref().map(|s| s.id))
                .collect::<Vec<_>>()
        });

        for i in 0..self.active_concurrent {
            let selected_idx = (start_idx + i) % total_upstreams;
            let upstream = self.upstreams[selected_idx].clone();
            let message = request.clone();
            let metrics = self.metrics.clone();
            let span = spans.as_mut().and_then(|spans| spans[selected_idx].take());
            join_set.spawn(async move {
                if let Some(span) = &span {
                    span.start();
                }
                let up_start = metrics.record_upstream_start(selected_idx);
                let result: Result<Message> = upstream.query(message).await;
                match &result {
                    Ok(response) => {
                        metrics.record_upstream_success(selected_idx, up_start);
                        if let Some(span) = &span {
                            span.response(response);
                            span.finish("success", None);
                        }
                    }
                    Err(e) => {
                        metrics.record_upstream_error(selected_idx, up_start, is_timeout_error(e));
                        if let Some(span) = &span {
                            span.finish(
                                if is_timeout_error(e) {
                                    "timeout"
                                } else {
                                    "error"
                                },
                                Some(e.to_string()),
                            );
                        }
                    }
                }
                if event_enabled!(Level::DEBUG) {
                    debug!(
                        "DNS ConcurrentForwarder received message {}, remote_addr: {}",
                        selected_idx,
                        upstream.connection_info().raw_addr
                    );
                }
                (selected_idx, result)
            });
        }

        let question = match self.response_selection {
            ResponseSelectionMode::Fastest => None,
            _ => request.first_question(),
        };
        let result = select_response(
            &mut join_set,
            self.active_concurrent,
            question,
            self.response_selection,
        )
        .await;
        if let (Some(trace), Some(ids), Some(selected)) = (&observation, &event_ids, &result.0)
            && let Some(id) = ids[selected.upstream_index]
        {
            trace.update(id, |event| {
                event.selected = true;
                event.reason = Some(format!("{:?}", self.response_selection).to_lowercase());
            });
        }
        result
    }
}
