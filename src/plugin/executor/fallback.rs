// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `fallback` executor plugin.
//!
//! Runs a primary executor and falls back to a secondary executor on
//! failure/timeout.
//!
//! Scheduling model:
//! - `primary` starts immediately.
//! - `secondary` starts after `threshold` milliseconds, or starts immediately
//!   in standby mode (`always_standby = true`).
//! - first successful response wins; unfinished sibling tasks are cancelled.
//!
//! Result semantics:
//! - if either branch produces a response, plugin writes it to
//!   `DnsContext.response` and returns `Next`.
//! - if both branches return no response, plugin continues the sequence without
//!   writing a response.
//! - if both branches fail, plugin returns error so the server request handler
//!   can generate a failure response.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tracing::debug;

use crate::config::types::PluginConfig;
use crate::core::context::{
    DnsContext, ExecutionPath, ExecutionPathCheckpoint, ExecutionPathEvent,
};
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::observability::metrics::{
    MetricLabel, MetricSample, MetricSink, MetricSource, register_metric_source,
    unregister_metric_source,
};
use crate::plugin::dependency::DependencySpec;
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;

#[derive(Debug, Clone, Deserialize)]
struct FallbackConfig {
    /// Executor tag used as the primary path.
    primary: String,
    /// Executor tag used as the standby path.
    secondary: String,
    /// Timeout threshold in milliseconds before primary is treated as slow.
    #[serde(default)]
    threshold: u64,
    /// Always run standby path in parallel regardless of primary latency.
    #[serde(default)]
    always_standby: bool,
    /// Whether to stop the executor chain after fallback picks a response.
    #[serde(default)]
    short_circuit: bool,
    /// Whether a slow primary may start and select the secondary branch.
    #[serde(default = "default_true")]
    fallback_on_timeout: bool,
    /// Whether an executor or transport error may select the secondary branch.
    #[serde(default = "default_true")]
    fallback_on_error: bool,
    /// Whether a completed primary without a response may select the secondary
    /// branch.
    #[serde(default = "default_true")]
    fallback_on_no_response: bool,
}

const fn default_true() -> bool {
    true
}

#[derive(Debug)]
struct FallbackExecutor {
    tag: String,
    primary_tag: String,
    secondary_tag: String,
    primary: Arc<dyn Executor>,
    secondary: Arc<dyn Executor>,
    threshold: Duration,
    always_standby: bool,
    short_circuit: bool,
    fallback_on_timeout: bool,
    fallback_on_error: bool,
    fallback_on_no_response: bool,
    metrics: Arc<FallbackMetrics>,
}

#[derive(Debug)]
struct FallbackMetrics {
    tag: String,
    primary_total: AtomicU64,
    primary_error_total: AtomicU64,
    secondary_total: AtomicU64,
}

impl FallbackMetrics {
    fn new(tag: String) -> Self {
        Self {
            tag,
            primary_total: AtomicU64::new(0),
            primary_error_total: AtomicU64::new(0),
            secondary_total: AtomicU64::new(0),
        }
    }
}

impl MetricSource for FallbackMetrics {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn plugin_type(&self) -> &'static str {
        "fallback"
    }

    fn collect(&self, sink: &mut dyn MetricSink) {
        let labels = [MetricLabel::new("plugin_tag", self.tag.as_str())];
        sink.emit(MetricSample::counter(
            "fallback_primary_total",
            "Total fallback primary executions.",
            &labels,
            self.primary_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "fallback_primary_error_total",
            "Total fallback primary executions that failed to produce a response.",
            &labels,
            self.primary_error_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "fallback_secondary_total",
            "Total fallback secondary executions.",
            &labels,
            self.secondary_total.load(Ordering::Relaxed),
        ));
    }
}

struct Outcome {
    context: Option<DnsContext>,
    execution_path: ExecutionPath,
    source: &'static str,
    error: Option<String>,
    no_response: bool,
    failure_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PrimaryState {
    Running,
    Success,
    FailedAllowed,
    FailedBlocked,
}

#[derive(Debug, Clone, Copy)]
struct FallbackSelectionDiagnostic<'a> {
    source: &'a str,
    reason: &'a str,
    started_at: std::time::Instant,
    branch_checkpoint: ExecutionPathCheckpoint,
}

#[async_trait]
impl Plugin for FallbackExecutor {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        register_metric_source(self.metrics.clone())
    }

    async fn destroy(&self) -> Result<()> {
        unregister_metric_source(&self.tag);
        Ok(())
    }
}

#[async_trait]
impl Executor for FallbackExecutor {
    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        let started_at = AppClock::now();
        let mut join_set = JoinSet::new();
        let (primary_state_tx, primary_state_rx) = watch::channel(PrimaryState::Running);
        let branch_checkpoint = context.execution_path.checkpoint();

        let primary = self.primary.clone();
        let primary_ctx = context.copy_for_subquery();
        let primary_metrics = self.metrics.clone();
        let fallback_on_error = self.fallback_on_error;
        let fallback_on_no_response = self.fallback_on_no_response;
        join_set.spawn(async move {
            primary_metrics
                .primary_total
                .fetch_add(1, Ordering::Relaxed);
            let outcome = run_executor(primary, primary_ctx, "primary", branch_checkpoint).await;
            let state = if outcome.context.is_some() {
                PrimaryState::Success
            } else if (outcome.no_response && fallback_on_no_response)
                || (!outcome.no_response && fallback_on_error)
            {
                PrimaryState::FailedAllowed
            } else {
                PrimaryState::FailedBlocked
            };
            if matches!(
                state,
                PrimaryState::FailedAllowed | PrimaryState::FailedBlocked
            ) {
                primary_metrics
                    .primary_error_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            let _ = primary_state_tx.send(state);
            outcome
        });

        let secondary = self.secondary.clone();
        let secondary_ctx = context.copy_for_subquery();
        let delay = self.threshold;
        let always_standby = self.always_standby;
        let fallback_on_timeout = self.fallback_on_timeout;
        let mut primary_state_rx = primary_state_rx.clone();
        let secondary_metrics = self.metrics.clone();
        join_set.spawn(async move {
            if !always_standby {
                let sleeper = tokio::time::sleep(delay);
                tokio::pin!(sleeper);
                loop {
                    if fallback_on_timeout {
                        tokio::select! {
                            _ = &mut sleeper => break,
                            changed = primary_state_rx.changed() => {
                                if changed.is_err() {
                                    break;
                                }
                                match *primary_state_rx.borrow() {
                                    PrimaryState::Running => {}
                                    PrimaryState::Success | PrimaryState::FailedBlocked => {
                                        return empty_secondary_outcome();
                                    }
                                    PrimaryState::FailedAllowed => break,
                                }
                            }
                        }
                    } else {
                        if primary_state_rx.changed().await.is_err() {
                            return empty_secondary_outcome();
                        }
                        match *primary_state_rx.borrow() {
                            PrimaryState::Running => {}
                            PrimaryState::Success | PrimaryState::FailedBlocked => {
                                return empty_secondary_outcome();
                            }
                            PrimaryState::FailedAllowed => break,
                        }
                    }
                }
            }
            secondary_metrics
                .secondary_total
                .fetch_add(1, Ordering::Relaxed);
            run_executor(secondary, secondary_ctx, "secondary", branch_checkpoint).await
        });

        let mut last_err = String::new();
        let mut buffered_secondary: Option<DnsContext> = None;
        let mut failed_primary_path: Option<ExecutionPath> = None;
        let mut primary_failure_reason: Option<String> = None;
        let mut primary_completed = false;
        let mut threshold_reached = !self.always_standby || !self.fallback_on_timeout;
        let standby_timer = tokio::time::sleep(self.threshold);
        tokio::pin!(standby_timer);
        loop {
            tokio::select! {
                _ = &mut standby_timer, if self.always_standby && self.fallback_on_timeout && !threshold_reached => {
                    threshold_reached = true;
                    // In standby mode, secondary can finish early but should not win until
                    // the threshold elapses. Flush buffered response once timer fires.
                    if let Some(secondary_ctx) = buffered_secondary.take() {
                        self.apply_selected(
                            context,
                            secondary_ctx,
                            failed_primary_path.as_ref(),
                            FallbackSelectionDiagnostic {
                                source: "secondary",
                                reason: if primary_completed {
                                    primary_failure_reason.as_deref().unwrap_or("no_response")
                                } else {
                                    "timeout"
                                },
                                started_at,
                                branch_checkpoint,
                            },
                        );
                        join_set.abort_all();
                        return Ok(self.completion_step());
                    }
                }
                joined = join_set.join_next() => {
                    let Some(joined) = joined else {
                        break;
                    };
                    let outcome = match joined {
                        Ok(outcome) => outcome,
                        Err(e) => {
                            last_err = format!("fallback subtask join error: {}", e);
                            continue;
                        }
                    };

                    match outcome.source {
                        "primary" => {
                            primary_completed = true;
                            if let Some(primary_ctx) = outcome.context {
                                self.apply_selected(
                                    context,
                                    primary_ctx,
                                    None,
                                    FallbackSelectionDiagnostic {
                                        source: "primary",
                                        reason: "success",
                                        started_at,
                                        branch_checkpoint,
                                    },
                                );
                                join_set.abort_all();
                                return Ok(self.completion_step());
                            }
                            failed_primary_path = Some(outcome.execution_path);
                            primary_failure_reason = outcome.failure_reason.clone();
                            let fallback_allowed = if outcome.no_response {
                                self.fallback_on_no_response
                            } else {
                                self.fallback_on_error
                            };
                            if !fallback_allowed {
                                if context.execution_path_enabled()
                                    && let Some(primary_path) = failed_primary_path.as_ref()
                                {
                                    context
                                        .execution_path
                                        .append_from(primary_path, branch_checkpoint);
                                }
                                join_set.abort_all();
                                if outcome.no_response {
                                    return Ok(ExecStep::Next);
                                }
                                return Err(DnsError::plugin(
                                    outcome.error.unwrap_or_else(|| {
                                        "fallback primary failed and error fallback is disabled"
                                            .to_string()
                                    }),
                                ));
                            }
                            if let Some(secondary_ctx) = buffered_secondary.take() {
                                self.apply_selected(
                                    context,
                                    secondary_ctx,
                                    failed_primary_path.as_ref(),
                                    FallbackSelectionDiagnostic {
                                        source: "secondary",
                                        reason: primary_failure_reason
                                            .as_deref()
                                            .unwrap_or("no_response"),
                                        started_at,
                                        branch_checkpoint,
                                    },
                                );
                                join_set.abort_all();
                                return Ok(self.completion_step());
                            }
                        }
                        "secondary" => {
                            if let Some(secondary_ctx) = outcome.context {
                                if !self.always_standby
                                    || (self.fallback_on_timeout && threshold_reached)
                                    || primary_completed
                                {
                                    self.apply_selected(
                                        context,
                                        secondary_ctx,
                                        failed_primary_path.as_ref(),
                                        FallbackSelectionDiagnostic {
                                            source: "secondary",
                                            reason: if primary_completed {
                                                primary_failure_reason
                                                    .as_deref()
                                                    .unwrap_or("no_response")
                                            } else {
                                                "timeout"
                                            },
                                            started_at,
                                            branch_checkpoint,
                                        },
                                    );
                                    join_set.abort_all();
                                    return Ok(self.completion_step());
                                }
                                // Standby mode before threshold: keep secondary result as backup
                                // and still wait for primary to finish or timer to fire.
                                buffered_secondary = Some(secondary_ctx);
                            }
                        }
                        _ => {}
                    }

                    if let Some(err) = outcome.error.filter(|_| !outcome.no_response) {
                        if !last_err.is_empty() {
                            last_err.push_str("; ");
                        }
                        last_err.push_str(&format!("{}: {}", outcome.source, err));
                    }
                }
            }
        }

        if last_err.is_empty() {
            debug!(
                "Fallback '{}' produced no response from '{}' or '{}'; continuing",
                self.tag, self.primary_tag, self.secondary_tag
            );
            return Ok(ExecStep::Next);
        }

        Err(DnsError::plugin(last_err))
    }
}

impl FallbackExecutor {
    #[inline]
    fn completion_step(&self) -> ExecStep {
        if self.short_circuit {
            ExecStep::Stop
        } else {
            ExecStep::Next
        }
    }

    fn apply_selected(
        &self,
        context: &mut DnsContext,
        mut selected: DnsContext,
        failed_primary_path: Option<&ExecutionPath>,
        diagnostic: FallbackSelectionDiagnostic<'_>,
    ) {
        if diagnostic.source == "secondary"
            && context.execution_path_enabled()
            && let Some(primary_path) = failed_primary_path
        {
            let mut merged = context.execution_path.clone();
            merged.append_from(primary_path, diagnostic.branch_checkpoint);
            merged.append_from(&selected.execution_path, diagnostic.branch_checkpoint);
            selected.execution_path = merged;
        }
        context.apply_subquery_result(selected);
        if context.execution_path_enabled() {
            context.push_execution_path_event(
                ExecutionPathEvent::new(
                    self.tag.as_str(),
                    None,
                    "fallback",
                    Some(self.tag.as_str()),
                    format!("{}_{}", diagnostic.source, diagnostic.reason),
                )
                .with_timing(
                    None,
                    Some(
                        diagnostic
                            .started_at
                            .elapsed()
                            .as_micros()
                            .min(u128::from(u64::MAX)) as u64,
                    ),
                ),
            );
        }
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("fallback")]
pub struct FallbackFactory;

impl PluginFactory for FallbackFactory {
    fn get_dependency_specs(&self, plugin_config: &PluginConfig) -> Vec<DependencySpec> {
        plugin_config
            .args
            .clone()
            .and_then(|args| serde_yaml_ng::from_value::<FallbackConfig>(args).ok())
            .map(|cfg| {
                vec![
                    DependencySpec::executor("args.primary", cfg.primary),
                    DependencySpec::executor("args.secondary", cfg.secondary),
                ]
            })
            .unwrap_or_default()
    }

    fn create(
        &self,
        plugin_config: &PluginConfig,
        init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let cfg: FallbackConfig = serde_yaml_ng::from_value(
            plugin_config
                .args
                .clone()
                .ok_or_else(|| DnsError::plugin("fallback requires args"))?,
        )
        .map_err(|e| DnsError::plugin(format!("failed to parse fallback config: {}", e)))?;

        let primary = init_context.executor("args.primary", cfg.primary.as_str())?;
        let secondary = init_context.executor("args.secondary", cfg.secondary.as_str())?;

        Ok(UninitializedPlugin::Executor(Box::new(FallbackExecutor {
            tag: plugin_config.tag.clone(),
            primary_tag: cfg.primary.clone(),
            secondary_tag: cfg.secondary.clone(),
            primary,
            secondary,
            threshold: Duration::from_millis(if cfg.threshold == 0 {
                500
            } else {
                cfg.threshold
            }),
            always_standby: cfg.always_standby,
            short_circuit: cfg.short_circuit,
            fallback_on_timeout: cfg.fallback_on_timeout,
            fallback_on_error: cfg.fallback_on_error,
            fallback_on_no_response: cfg.fallback_on_no_response,
            metrics: Arc::new(FallbackMetrics::new(plugin_config.tag.clone())),
        })))
    }
}

fn empty_secondary_outcome() -> Outcome {
    Outcome {
        context: None,
        execution_path: ExecutionPath::default(),
        source: "secondary",
        error: None,
        no_response: false,
        failure_reason: None,
    }
}

async fn run_executor(
    executor: Arc<dyn Executor>,
    mut context: DnsContext,
    source: &'static str,
    branch_checkpoint: ExecutionPathCheckpoint,
) -> Outcome {
    if context.execution_path_enabled() {
        context.push_execution_path_event(ExecutionPathEvent::new(
            executor.tag(),
            None,
            "executor",
            Some(executor.tag()),
            "entered",
        ));
    }
    match executor.execute_with_next(&mut context, None).await {
        Ok(step) => {
            let has_response = context.response().is_some();
            let execution_path = context.execution_path.clone();
            let failure_reason = if has_response {
                None
            } else {
                execution_path
                    .events_from_checkpoint(branch_checkpoint)
                    .iter()
                    .rev()
                    .find(|event| event.kind == "decision")
                    .map(|event| event.outcome.clone())
                    .or_else(|| Some("no_response".to_string()))
            };
            Outcome {
                context: if has_response { Some(context) } else { None },
                execution_path,
                source,
                no_response: !has_response,
                failure_reason,
                error: if has_response {
                    None
                } else {
                    Some(format!("executor returned {:?} without response", step))
                },
            }
        }
        Err(e) => {
            let execution_path = context.execution_path.clone();
            Outcome {
                context: None,
                execution_path,
                source,
                error: Some(e.to_string()),
                no_response: false,
                failure_reason: Some("transport_failure".to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::plugin::test_utils::{plugin_config, test_context};

    #[test]
    fn test_fallback_factory_requires_args() {
        let factory = FallbackFactory;
        let cfg = plugin_config("fb", "fallback", None);
        assert!(crate::plugin::test_utils::create_plugin_for_test(&factory, &cfg).is_err());
    }

    #[derive(Debug)]
    struct StubExecutor {
        tag: String,
        should_fail: bool,
        produce_response: bool,
        refused_with_next: bool,
    }

    #[async_trait]
    impl Plugin for StubExecutor {
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
    impl Executor for StubExecutor {
        fn with_next(&self) -> bool {
            self.refused_with_next
        }

        async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
            if self.should_fail {
                return Err(DnsError::plugin("stub failed"));
            }
            if self.produce_response {
                context.set_response(crate::proto::Message::new());
            }
            Ok(ExecStep::Next)
        }

        async fn execute_with_next(
            &self,
            context: &mut DnsContext,
            next: Option<crate::plugin::executor::ExecutorNext>,
        ) -> Result<ExecStep> {
            if self.refused_with_next {
                let _ = next;
                context.set_response(context.request.response(crate::proto::Rcode::Refused));
                return Ok(ExecStep::Next);
            }
            self.execute(context).await
        }
    }

    #[tokio::test]
    async fn test_run_executor_reports_success_and_errors() {
        let success = run_executor(
            Arc::new(StubExecutor {
                tag: "ok".to_string(),
                should_fail: false,
                produce_response: true,
                refused_with_next: false,
            }),
            test_context(),
            "primary",
            ExecutionPath::default().checkpoint(),
        )
        .await;
        assert!(success.context.is_some());
        assert!(success.error.is_none());

        let no_response = run_executor(
            Arc::new(StubExecutor {
                tag: "noresp".to_string(),
                should_fail: false,
                produce_response: false,
                refused_with_next: false,
            }),
            test_context(),
            "secondary",
            ExecutionPath::default().checkpoint(),
        )
        .await;
        assert!(no_response.context.is_none());
        assert!(
            no_response
                .error
                .as_deref()
                .is_some_and(|e| e.contains("without response"))
        );

        let failed = run_executor(
            Arc::new(StubExecutor {
                tag: "err".to_string(),
                should_fail: true,
                produce_response: false,
                refused_with_next: false,
            }),
            test_context(),
            "primary",
            ExecutionPath::default().checkpoint(),
        )
        .await;
        assert!(failed.context.is_none());
        assert!(
            failed
                .error
                .as_deref()
                .is_some_and(|e| e.contains("stub failed"))
        );
    }

    #[tokio::test]
    async fn test_run_executor_supports_with_next_executor() {
        let outcome = run_executor(
            Arc::new(StubExecutor {
                tag: "with_next".to_string(),
                should_fail: false,
                produce_response: false,
                refused_with_next: true,
            }),
            test_context(),
            "primary",
            ExecutionPath::default().checkpoint(),
        )
        .await;

        let context = outcome
            .context
            .expect("with-next executor should produce a response");
        assert_eq!(
            context
                .response()
                .expect("with-next executor should set response")
                .rcode(),
            crate::proto::Rcode::Refused
        );
        assert!(outcome.error.is_none());
    }

    #[test]
    fn test_fallback_config_accepts_short_circuit() {
        let cfg: FallbackConfig = serde_yaml_ng::from_str(
            r#"
primary: "fast"
secondary: "slow"
short_circuit: true
"#,
        )
        .expect("fallback config should parse");

        assert!(cfg.short_circuit);
        assert!(cfg.fallback_on_timeout);
        assert!(cfg.fallback_on_error);
        assert!(cfg.fallback_on_no_response);
    }

    #[tokio::test]
    async fn test_fallback_execute_stops_when_short_circuit_enabled() {
        let metrics = Arc::new(FallbackMetrics::new("fb".to_string()));
        let executor = FallbackExecutor {
            tag: "fb".to_string(),
            primary_tag: "primary".to_string(),
            secondary_tag: "secondary".to_string(),
            primary: Arc::new(StubExecutor {
                tag: "primary".to_string(),
                should_fail: false,
                produce_response: true,
                refused_with_next: false,
            }),
            secondary: Arc::new(StubExecutor {
                tag: "secondary".to_string(),
                should_fail: false,
                produce_response: false,
                refused_with_next: false,
            }),
            threshold: Duration::from_secs(60),
            always_standby: false,
            short_circuit: true,
            fallback_on_timeout: true,
            fallback_on_error: true,
            fallback_on_no_response: true,
            metrics: metrics.clone(),
        };

        let mut context = test_context();
        let step = executor.execute(&mut context).await.unwrap();

        assert!(matches!(step, ExecStep::Stop));
        assert!(context.response().is_some());
        assert_eq!(metrics.primary_total.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.primary_error_total.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.secondary_total.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_fallback_metrics_record_primary_error_and_secondary() {
        let metrics = Arc::new(FallbackMetrics::new("fb".to_string()));
        let executor = FallbackExecutor {
            tag: "fb".to_string(),
            primary_tag: "primary".to_string(),
            secondary_tag: "secondary".to_string(),
            primary: Arc::new(StubExecutor {
                tag: "primary".to_string(),
                should_fail: true,
                produce_response: false,
                refused_with_next: false,
            }),
            secondary: Arc::new(StubExecutor {
                tag: "secondary".to_string(),
                should_fail: false,
                produce_response: true,
                refused_with_next: false,
            }),
            threshold: Duration::ZERO,
            always_standby: false,
            short_circuit: false,
            fallback_on_timeout: true,
            fallback_on_error: true,
            fallback_on_no_response: true,
            metrics: metrics.clone(),
        };

        let mut context = test_context();
        let step = executor.execute(&mut context).await.unwrap();

        assert!(matches!(step, ExecStep::Next));
        assert!(context.response().is_some());
        assert_eq!(metrics.primary_total.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.primary_error_total.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.secondary_total.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_fallback_continues_when_branches_return_no_response() {
        let metrics = Arc::new(FallbackMetrics::new("fb".to_string()));
        let executor = FallbackExecutor {
            tag: "fb".to_string(),
            primary_tag: "primary".to_string(),
            secondary_tag: "secondary".to_string(),
            primary: Arc::new(StubExecutor {
                tag: "primary".to_string(),
                should_fail: false,
                produce_response: false,
                refused_with_next: false,
            }),
            secondary: Arc::new(StubExecutor {
                tag: "secondary".to_string(),
                should_fail: false,
                produce_response: false,
                refused_with_next: false,
            }),
            threshold: Duration::ZERO,
            always_standby: false,
            short_circuit: true,
            fallback_on_timeout: true,
            fallback_on_error: true,
            fallback_on_no_response: true,
            metrics: metrics.clone(),
        };

        let mut context = test_context();
        let step = executor.execute(&mut context).await.unwrap();

        assert!(matches!(step, ExecStep::Next));
        assert!(context.response().is_none());
        assert_eq!(metrics.primary_total.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.primary_error_total.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.secondary_total.load(Ordering::Relaxed), 1);
    }

    #[derive(Debug)]
    struct TraceExecutor {
        tag: &'static str,
        response: bool,
        decision: Option<&'static str>,
        delay: Duration,
        fail: bool,
    }

    #[async_trait]
    impl Plugin for TraceExecutor {
        fn tag(&self) -> &str {
            self.tag
        }

        async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
            Ok(())
        }

        async fn destroy(&self) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Executor for TraceExecutor {
        async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            if self.fail {
                return Err(DnsError::plugin("trace transport failure"));
            }
            if let Some(decision) = self.decision {
                context.push_execution_path_event(ExecutionPathEvent::new(
                    self.tag,
                    None,
                    "decision",
                    Some(self.tag),
                    decision,
                ));
            }
            if self.response {
                context.set_response(context.request().response(crate::proto::Rcode::NoError));
            }
            Ok(ExecStep::Next)
        }
    }

    #[tokio::test]
    async fn fallback_preserves_failed_primary_decision_and_records_selected_reason() {
        let executor = FallbackExecutor {
            tag: "smart_fallback".to_string(),
            primary_tag: "domestic".to_string(),
            secondary_tag: "remote".to_string(),
            primary: Arc::new(TraceExecutor {
                tag: "domestic",
                response: false,
                decision: Some("nodata"),
                delay: Duration::ZERO,
                fail: false,
            }),
            secondary: Arc::new(TraceExecutor {
                tag: "remote",
                response: true,
                decision: None,
                delay: Duration::ZERO,
                fail: false,
            }),
            threshold: Duration::from_secs(60),
            always_standby: false,
            short_circuit: true,
            fallback_on_timeout: true,
            fallback_on_error: true,
            fallback_on_no_response: true,
            metrics: Arc::new(FallbackMetrics::new("smart_fallback".to_string())),
        };
        let mut context = test_context();
        context.enable_execution_path();

        let step = executor.execute(&mut context).await.unwrap();

        assert_eq!(step, ExecStep::Stop);
        let outcomes: Vec<_> = context
            .execution_path_events()
            .iter()
            .map(|event| event.outcome.as_str())
            .collect();
        assert!(outcomes.contains(&"nodata"));
        assert!(outcomes.contains(&"secondary_nodata"));
    }

    #[tokio::test]
    async fn fallback_records_threshold_timeout_when_secondary_wins() {
        let executor = FallbackExecutor {
            tag: "smart_fallback".to_string(),
            primary_tag: "domestic".to_string(),
            secondary_tag: "remote".to_string(),
            primary: Arc::new(TraceExecutor {
                tag: "domestic",
                response: true,
                decision: None,
                delay: Duration::from_millis(100),
                fail: false,
            }),
            secondary: Arc::new(TraceExecutor {
                tag: "remote",
                response: true,
                decision: None,
                delay: Duration::ZERO,
                fail: false,
            }),
            threshold: Duration::from_millis(5),
            always_standby: false,
            short_circuit: true,
            fallback_on_timeout: true,
            fallback_on_error: true,
            fallback_on_no_response: true,
            metrics: Arc::new(FallbackMetrics::new("smart_fallback".to_string())),
        };
        let mut context = test_context();
        context.enable_execution_path();

        assert_eq!(
            executor.execute(&mut context).await.unwrap(),
            ExecStep::Stop
        );
        assert!(
            context
                .execution_path_events()
                .iter()
                .any(|event| event.outcome == "secondary_timeout" && event.duration_us.is_some())
        );
    }

    #[tokio::test]
    async fn fallback_records_transport_failure_before_secondary_selection() {
        let executor = FallbackExecutor {
            tag: "smart_fallback".to_string(),
            primary_tag: "domestic".to_string(),
            secondary_tag: "remote".to_string(),
            primary: Arc::new(TraceExecutor {
                tag: "domestic",
                response: false,
                decision: None,
                delay: Duration::ZERO,
                fail: true,
            }),
            secondary: Arc::new(TraceExecutor {
                tag: "remote",
                response: true,
                decision: None,
                delay: Duration::ZERO,
                fail: false,
            }),
            threshold: Duration::from_secs(60),
            always_standby: false,
            short_circuit: true,
            fallback_on_timeout: true,
            fallback_on_error: true,
            fallback_on_no_response: true,
            metrics: Arc::new(FallbackMetrics::new("smart_fallback".to_string())),
        };
        let mut context = test_context();
        context.enable_execution_path();

        assert_eq!(
            executor.execute(&mut context).await.unwrap(),
            ExecStep::Stop
        );
        assert!(
            context
                .execution_path_events()
                .iter()
                .any(|event| event.outcome == "secondary_transport_failure")
        );
    }

    #[tokio::test]
    async fn standby_response_cannot_win_before_threshold() {
        let executor = FallbackExecutor {
            tag: "standby".to_string(),
            primary_tag: "primary".to_string(),
            secondary_tag: "secondary".to_string(),
            primary: Arc::new(TraceExecutor {
                tag: "primary",
                response: true,
                decision: None,
                delay: Duration::from_millis(10),
                fail: false,
            }),
            secondary: Arc::new(TraceExecutor {
                tag: "secondary",
                response: true,
                decision: None,
                delay: Duration::ZERO,
                fail: false,
            }),
            threshold: Duration::from_millis(100),
            always_standby: true,
            short_circuit: false,
            fallback_on_timeout: true,
            fallback_on_error: true,
            fallback_on_no_response: true,
            metrics: Arc::new(FallbackMetrics::new("standby".to_string())),
        };
        let mut context = test_context();
        context.enable_execution_path();

        assert_eq!(
            executor.execute(&mut context).await.unwrap(),
            ExecStep::Next
        );
        assert!(
            context
                .execution_path_events()
                .iter()
                .any(|event| event.outcome == "primary_success")
        );
        assert!(
            !context
                .execution_path_events()
                .iter()
                .any(|event| event.outcome.starts_with("secondary_"))
        );
    }

    #[tokio::test]
    async fn disabled_timeout_fallback_waits_for_primary_success() {
        let metrics = Arc::new(FallbackMetrics::new("no_timeout".to_string()));
        let executor = FallbackExecutor {
            tag: "no_timeout".to_string(),
            primary_tag: "primary".to_string(),
            secondary_tag: "secondary".to_string(),
            primary: Arc::new(TraceExecutor {
                tag: "primary",
                response: true,
                decision: None,
                delay: Duration::from_millis(10),
                fail: false,
            }),
            secondary: Arc::new(TraceExecutor {
                tag: "secondary",
                response: true,
                decision: None,
                delay: Duration::ZERO,
                fail: false,
            }),
            threshold: Duration::from_millis(1),
            always_standby: false,
            short_circuit: false,
            fallback_on_timeout: false,
            fallback_on_error: true,
            fallback_on_no_response: true,
            metrics: metrics.clone(),
        };
        let mut context = test_context();
        context.enable_execution_path();

        assert_eq!(
            executor.execute(&mut context).await.unwrap(),
            ExecStep::Next
        );
        assert!(
            context
                .execution_path_events()
                .iter()
                .any(|event| event.outcome == "primary_success")
        );
        assert_eq!(metrics.secondary_total.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn disabled_error_fallback_returns_primary_error_without_secondary() {
        let metrics = Arc::new(FallbackMetrics::new("no_error".to_string()));
        let executor = FallbackExecutor {
            tag: "no_error".to_string(),
            primary_tag: "primary".to_string(),
            secondary_tag: "secondary".to_string(),
            primary: Arc::new(TraceExecutor {
                tag: "primary",
                response: false,
                decision: None,
                delay: Duration::ZERO,
                fail: true,
            }),
            secondary: Arc::new(TraceExecutor {
                tag: "secondary",
                response: true,
                decision: None,
                delay: Duration::ZERO,
                fail: false,
            }),
            threshold: Duration::ZERO,
            always_standby: false,
            short_circuit: false,
            fallback_on_timeout: true,
            fallback_on_error: false,
            fallback_on_no_response: true,
            metrics: metrics.clone(),
        };
        let mut context = test_context();

        let error = executor
            .execute(&mut context)
            .await
            .expect_err("primary error should be returned");
        assert!(error.to_string().contains("trace transport failure"));
        assert_eq!(metrics.secondary_total.load(Ordering::Relaxed), 0);
    }
}
