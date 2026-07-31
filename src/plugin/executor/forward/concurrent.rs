// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use async_trait::async_trait;
use rand::RngExt;
use tokio::task::JoinSet;
use tracing::{Level, debug, event_enabled, info, warn};

use super::is_timeout_error;
use super::metrics::ForwardMetrics;
use super::selection::{ResponseSelectionMode, SelectedResponse, UpstreamAttempt, select_response};
use crate::core::context::{DnsContext, ExecutionPathEvent};
use crate::core::response::ResponseDisposition;
use crate::infra::clock::AppClock;
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
        let (response, last_error, timed_out, mut attempts) =
            self.query_upstreams(context.request.clone()).await;
        let selected_index = response.as_ref().map(|selected| selected.upstream_index);
        for attempt in &mut attempts {
            if attempt.outcome == "response" {
                attempt.outcome = if Some(attempt.index) == selected_index {
                    "selected"
                } else {
                    "response_not_selected"
                };
            }
            self.record_attempt(context, attempt);
        }
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
    ) -> (
        Option<SelectedResponse>,
        Option<String>,
        bool,
        Vec<AttemptTrace>,
    ) {
        let total_upstreams = self.upstreams.len();
        if total_upstreams == 0 {
            return (
                None,
                Some("no upstream configured".to_string()),
                false,
                Vec::new(),
            );
        }

        let mut join_set = JoinSet::new();
        let (trace_tx, mut trace_rx) = tokio::sync::mpsc::unbounded_channel();
        let start_idx = rand::rng().random_range(0..total_upstreams);
        let mut attempted = Vec::with_capacity(self.active_concurrent);

        for i in 0..self.active_concurrent {
            let selected_idx = (start_idx + i) % total_upstreams;
            attempted.push(selected_idx);
            let upstream = self.upstreams[selected_idx].clone();
            let member_tag = upstream
                .connection_info()
                .tag
                .clone()
                .unwrap_or_else(|| format!("index:{selected_idx}"));
            let message = request.clone();
            let metrics = self.metrics.clone();
            let trace_tx = trace_tx.clone();
            join_set.spawn(async move {
                let up_start = metrics.record_upstream_start(selected_idx);
                let diagnostic_start = AppClock::elapsed_millis();
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
                let outcome = match &result {
                    Ok(_) => "response",
                    Err(error) if is_timeout_error(error) => "timeout",
                    Err(_) => "transport_error",
                };
                let _ = trace_tx.send(AttemptTrace {
                    index: selected_idx,
                    member_tag,
                    outcome,
                    duration_us: AppClock::elapsed_millis()
                        .saturating_sub(diagnostic_start)
                        .saturating_mul(1000),
                });
                UpstreamAttempt {
                    upstream_index: selected_idx,
                    result,
                }
            });
        }
        drop(trace_tx);

        let question = match self.response_selection {
            ResponseSelectionMode::Fastest => None,
            _ => request.first_question(),
        };
        let selected = select_response(
            &mut join_set,
            self.active_concurrent,
            question,
            self.response_selection,
        )
        .await;
        let mut traces = Vec::with_capacity(attempted.len());
        while let Ok(trace) = trace_rx.try_recv() {
            traces.push(trace);
        }
        for index in attempted {
            if traces.iter().all(|trace| trace.index != index) {
                let member_tag = self.upstreams[index]
                    .connection_info()
                    .tag
                    .clone()
                    .unwrap_or_else(|| format!("index:{index}"));
                traces.push(AttemptTrace {
                    index,
                    member_tag,
                    outcome: "cancelled_after_selection",
                    duration_us: 0,
                });
            }
        }
        (selected.0, selected.1, selected.2, traces)
    }

    fn record_attempt(&self, context: &mut DnsContext, attempt: &AttemptTrace) {
        if !context.execution_path_enabled() {
            return;
        }
        context.push_execution_path_event(
            ExecutionPathEvent::new(
                self.tag.clone(),
                None,
                "upstream",
                Some(attempt.member_tag.clone()),
                attempt.outcome,
            )
            .with_timing(None, Some(attempt.duration_us))
            .with_detail([
                ("index", attempt.index.to_string()),
                (
                    "selection",
                    format!("{:?}", self.response_selection).to_ascii_lowercase(),
                ),
            ]),
        );
    }
}

#[derive(Debug)]
struct AttemptTrace {
    index: usize,
    member_tag: String,
    outcome: &'static str,
    duration_us: u64,
}
