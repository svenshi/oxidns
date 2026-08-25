// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cron` executor plugin.
//!
//! This executor is a background scheduler instead of a request-path action.
//! It resolves a list of executor references, then triggers them on a fixed
//! interval or standard 5-field cron expression after plugin initialization.
//!
//! Design notes:
//! - jobs run with an empty [`DnsContext`];
//! - job executors always run serially;
//! - executor `Stop`, response mutation, or errors never stop later executors;
//! - overlapping triggers are skipped rather than queued; and
//! - quick-setup executors are owned and destroyed by this plugin.

#[cfg(feature = "api")]
mod api;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{info, warn};

use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result};
use crate::infra::observability::metrics::{
    MetricLabel, MetricSample, MetricSink, MetricSource, register_metric_source,
    unregister_metric_source,
};
use crate::infra::system::{parse_simple_duration, system_timezone_name};
use crate::infra::task::{
    self as task_center, ManagedTaskHandle, TaskEvent, TaskEventHandler, TaskRunContext,
    TaskSchedule, TaskTriggerKind,
};
use crate::plugin::dependency::DependencySpec;
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::{
    Plugin, PluginFactory, PluginHolder, PluginInitContext, UninitializedPlugin,
    expand_quick_setup_dependency_specs, registered_plugin_kind,
};
use crate::plugin_factory;
use crate::proto::Message;

const ATTR_PLUGIN_TAG: &str = "cron.plugin_tag";
const ATTR_JOB_NAME: &str = "cron.job_name";
const ATTR_SCHEDULED_AT_UNIX_MS: &str = "cron.scheduled_at_unix_ms";
const ATTR_TRIGGER_KIND: &str = "cron.trigger_kind";
const CRON_PLUGIN_TYPE: &str = "cron";
const MIN_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Deserialize)]
struct CronConfig {
    timezone: Option<String>,
    jobs: Vec<JobConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct JobConfig {
    name: String,
    schedule: Option<String>,
    interval: Option<String>,
    executors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExecutorRef {
    PluginTag(String),
    QuickSetup {
        plugin_type: String,
        param: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct PreparedJob {
    name: String,
    schedule: TaskSchedule,
    scheduled_trigger_kind: &'static str,
    executors: Vec<Arc<dyn Executor>>,
}

#[derive(Debug)]
struct CronMetrics {
    tag: String,
    run_total: AtomicU64,
    skipped_total: AtomicU64,
    executor_error_total: AtomicU64,
}

impl CronMetrics {
    fn new(tag: String) -> Self {
        Self {
            tag,
            run_total: AtomicU64::new(0),
            skipped_total: AtomicU64::new(0),
            executor_error_total: AtomicU64::new(0),
        }
    }
}

impl MetricSource for CronMetrics {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn plugin_type(&self) -> &'static str {
        "cron"
    }

    fn collect(&self, sink: &mut dyn MetricSink) {
        let labels = [MetricLabel::new("plugin_tag", self.tag.as_str())];
        sink.emit(MetricSample::counter(
            "cron_job_run_total",
            "Total cron job runs that were started.",
            &labels,
            self.run_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "cron_job_skipped_total",
            "Total cron job triggers skipped because the previous run was still active.",
            &labels,
            self.skipped_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "cron_executor_error_total",
            "Total executor failures across cron job runs.",
            &labels,
            self.executor_error_total.load(Ordering::Relaxed),
        ));
    }
}

#[derive(Debug)]
struct CronExecutor {
    tag: String,
    config: CronConfig,
    quick_setup_executors: Vec<Arc<dyn Executor>>,
    task_handles: Vec<ManagedTaskHandle>,
    metrics: Arc<CronMetrics>,
}

#[async_trait]
impl Plugin for CronExecutor {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, context: &PluginInitContext<'_>) -> Result<()> {
        let mut prepared_jobs = Vec::with_capacity(self.config.jobs.len());
        let jobs = self.config.jobs.clone();
        for (job_index, job) in jobs.iter().enumerate() {
            match self.prepare_job(context, job, job_index).await {
                Ok(prepared) => prepared_jobs.push(prepared),
                Err(error) => {
                    rollback_quick_setup_executors(&mut self.quick_setup_executors).await;
                    return Err(error);
                }
            }
        }

        if let Err(error) = register_metric_source(self.metrics.clone()) {
            rollback_quick_setup_executors(&mut self.quick_setup_executors).await;
            return Err(error);
        }

        let mut task_handles = Vec::with_capacity(prepared_jobs.len());
        #[cfg(feature = "api")]
        let mut api_jobs = std::collections::HashMap::with_capacity(prepared_jobs.len());
        for job in prepared_jobs {
            let task_name = format!("cron:{}:{}", self.tag, job.name);
            let run_plugin_tag = self.tag.clone();
            let run_job_name = job.name.clone();
            let run_executors = job.executors.clone();
            let run_metrics = self.metrics.clone();
            let scheduled_trigger_kind = job.scheduled_trigger_kind;

            let event_plugin_tag = self.tag.clone();
            let event_job_name = job.name.clone();
            let event_metrics = self.metrics.clone();
            let on_event: TaskEventHandler = Arc::new(move |event| match event {
                TaskEvent::Skipped { trigger } => {
                    event_metrics.skipped_total.fetch_add(1, Ordering::Relaxed);
                    let trigger_kind = match trigger {
                        TaskTriggerKind::Scheduled => scheduled_trigger_kind,
                        TaskTriggerKind::Manual => "manual",
                    };
                    warn!(
                        plugin = %event_plugin_tag,
                        job = %event_job_name,
                        trigger = %trigger_kind,
                        "cron job trigger skipped because the previous run is still active"
                    );
                }
            });

            let handle = match task_center::register_scheduled(
                task_name,
                job.schedule,
                Some(on_event),
                move |context: TaskRunContext| {
                    let plugin_tag = run_plugin_tag.clone();
                    let job_name = run_job_name.clone();
                    let executors = run_executors.clone();
                    let metrics = run_metrics.clone();
                    async move {
                        let trigger_kind = match context.trigger {
                            TaskTriggerKind::Scheduled => scheduled_trigger_kind,
                            TaskTriggerKind::Manual => "manual",
                        };
                        run_job(
                            plugin_tag,
                            job_name,
                            trigger_kind.to_string(),
                            context.scheduled_at_unix_ms,
                            executors,
                            metrics,
                        )
                        .await;
                    }
                },
            )
            .await
            {
                Ok(handle) => handle,
                Err(error) => {
                    stop_registered_tasks(&task_handles).await;
                    unregister_metric_source(&self.tag);
                    rollback_quick_setup_executors(&mut self.quick_setup_executors).await;
                    return Err(DnsError::plugin(format!(
                        "failed to register cron job '{}': {}",
                        job.name, error
                    )));
                }
            };

            #[cfg(feature = "api")]
            api_jobs.insert(job.name, handle.clone());
            task_handles.push(handle);
        }

        #[cfg(feature = "api")]
        if let Err(error) = api::register(&self.tag, Arc::new(api_jobs)) {
            stop_registered_tasks(&task_handles).await;
            unregister_metric_source(&self.tag);
            rollback_quick_setup_executors(&mut self.quick_setup_executors).await;
            return Err(error);
        }

        self.task_handles = task_handles;
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        unregister_metric_source(&self.tag);
        stop_registered_tasks(&self.task_handles).await;

        let mut first_err = None;
        for executor in &self.quick_setup_executors {
            if let Err(err) = executor.destroy().await
                && first_err.is_none()
            {
                first_err = Some(err);
            }
        }

        if let Some(err) = first_err {
            Err(err)
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl Executor for CronExecutor {
    #[hotpath::measure]
    async fn execute(&self, _context: &mut DnsContext) -> Result<ExecStep> {
        Err(DnsError::plugin(
            "cron can only run as a background scheduled executor",
        ))
    }
}

impl CronExecutor {
    async fn prepare_job(
        &mut self,
        context: &PluginInitContext<'_>,
        job: &JobConfig,
        job_index: usize,
    ) -> Result<PreparedJob> {
        let timezone_name = resolve_timezone_name(self.config.timezone.as_deref());
        let trigger = parse_job_trigger_with_timezone(job, &timezone_name)?;
        let mut executors = Vec::with_capacity(job.executors.len());
        for (exec_index, raw) in job.executors.iter().enumerate() {
            let field = format!("args.jobs[{}].executors[{}]", job_index, exec_index);
            executors.push(
                self.resolve_executor_ref(context, raw, job_index, exec_index, &field)
                    .await?,
            );
        }

        Ok(PreparedJob {
            name: job.name.trim().to_string(),
            schedule: trigger.0,
            scheduled_trigger_kind: trigger.1,
            executors,
        })
    }

    async fn resolve_executor_ref(
        &mut self,
        context: &PluginInitContext<'_>,
        raw: &str,
        job_index: usize,
        exec_index: usize,
        field: &str,
    ) -> Result<Arc<dyn Executor>> {
        match parse_executor_ref(raw)? {
            ExecutorRef::PluginTag(tag) => {
                if tag == self.tag {
                    return Err(DnsError::plugin(format!(
                        "plugin '{}' field '{}' references itself",
                        self.tag, field
                    )));
                }
                let plugin = context.plugin(field, &tag)?;
                if plugin.plugin_type != crate::plugin::PluginType::Executor {
                    return Err(DnsError::plugin(format!(
                        "plugin '{}' field '{}' expects executor plugin, but '{}' is {} (type '{}')",
                        self.tag,
                        field,
                        tag,
                        plugin_type_kind_name(plugin.plugin_type),
                        plugin.plugin_name
                    )));
                }
                if plugin.plugin_name == CRON_PLUGIN_TYPE {
                    return Err(DnsError::plugin(format!(
                        "plugin '{}' field '{}' cannot reference cron executor '{}'",
                        self.tag, field, tag
                    )));
                }
                Ok(plugin.to_executor())
            }
            ExecutorRef::QuickSetup { plugin_type, param } => {
                if plugin_type == CRON_PLUGIN_TYPE {
                    return Err(DnsError::plugin(format!(
                        "plugin '{}' field '{}' cannot reference cron executor type '{}'",
                        self.tag, field, plugin_type
                    )));
                }

                let quick_tag = format!("qs.cron.{}.{}.{}", self.tag, job_index, exec_index);
                let executor = match context
                    .init_quick_setup(&plugin_type, &quick_tag, param)
                    .await?
                {
                    PluginHolder::Executor(executor) => executor,
                    _ => {
                        return Err(DnsError::plugin(format!(
                            "quick setup plugin '{}' is not an executor",
                            plugin_type
                        )));
                    }
                };
                self.quick_setup_executors.push(executor.clone());
                Ok(executor)
            }
        }
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("cron")]
pub struct CronFactory;

impl PluginFactory for CronFactory {
    fn get_dependency_specs(&self, plugin_config: &PluginConfig) -> Vec<DependencySpec> {
        let Some(args) = plugin_config.args.clone() else {
            return vec![];
        };
        let Ok(config) = serde_yaml_ng::from_value::<CronConfig>(args) else {
            return vec![];
        };

        let mut deps = Vec::new();
        for (job_index, job) in config.jobs.into_iter().enumerate() {
            for (exec_index, raw) in job.executors.into_iter().enumerate() {
                let field = format!("args.jobs[{}].executors[{}]", job_index, exec_index);
                match parse_executor_ref(&raw) {
                    Ok(ExecutorRef::PluginTag(tag)) => {
                        deps.push(DependencySpec::executor(field, tag));
                    }
                    Ok(ExecutorRef::QuickSetup { plugin_type, param }) => {
                        deps.extend(expand_quick_setup_dependency_specs(
                            &field,
                            &plugin_type,
                            param.as_deref(),
                        ));
                    }
                    Err(_) => {}
                }
            }
        }
        deps
    }

    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let args = plugin_config
            .args
            .clone()
            .ok_or_else(|| DnsError::plugin("cron requires configuration arguments"))?;
        let config = serde_yaml_ng::from_value::<CronConfig>(args)
            .map_err(|e| DnsError::plugin(format!("failed to parse cron config: {}", e)))?;
        validate_config(plugin_config, &config)?;

        Ok(UninitializedPlugin::Executor(Box::new(CronExecutor {
            tag: plugin_config.tag.clone(),
            metrics: Arc::new(CronMetrics::new(plugin_config.tag.clone())),
            config,
            quick_setup_executors: Vec::new(),
            task_handles: Vec::new(),
        })))
    }
}

fn validate_config(plugin_config: &PluginConfig, config: &CronConfig) -> Result<()> {
    if config.jobs.is_empty() {
        return Err(DnsError::plugin("cron requires at least one job"));
    }

    let timezone_name = resolve_timezone_name(config.timezone.as_deref());
    let mut seen_names = std::collections::HashSet::new();
    for (job_index, job) in config.jobs.iter().enumerate() {
        let name = job.name.trim();
        if name.is_empty() {
            return Err(DnsError::plugin(format!(
                "plugin '{}' args.jobs[{}].name cannot be empty",
                plugin_config.tag, job_index
            )));
        }
        if !seen_names.insert(name.to_string()) {
            return Err(DnsError::plugin(format!(
                "plugin '{}' has duplicate cron job name '{}'",
                plugin_config.tag, name
            )));
        }
        if job.executors.is_empty() {
            return Err(DnsError::plugin(format!(
                "plugin '{}' args.jobs[{}].executors cannot be empty",
                plugin_config.tag, job_index
            )));
        }
        parse_job_trigger_with_timezone(job, &timezone_name)?;

        for (exec_index, raw) in job.executors.iter().enumerate() {
            let field = format!("args.jobs[{}].executors[{}]", job_index, exec_index);
            match parse_executor_ref(raw)? {
                ExecutorRef::PluginTag(tag) if tag == plugin_config.tag => {
                    return Err(DnsError::plugin(format!(
                        "plugin '{}' field '{}' references itself",
                        plugin_config.tag, field
                    )));
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn parse_job_trigger_with_timezone(
    job: &JobConfig,
    timezone_name: &str,
) -> Result<(TaskSchedule, &'static str)> {
    let has_schedule = job.schedule.as_ref().is_some_and(|v| !v.trim().is_empty());
    let has_interval = job.interval.as_ref().is_some_and(|v| !v.trim().is_empty());

    match (has_schedule, has_interval) {
        (true, true) => {
            return Err(DnsError::plugin(format!(
                "cron job '{}' must configure exactly one of schedule or interval",
                job.name
            )));
        }
        (false, false) => {
            return Err(DnsError::plugin(format!(
                "cron job '{}' must configure exactly one of schedule or interval",
                job.name
            )));
        }
        _ => {}
    }

    if let Some(schedule) = job.schedule.as_deref().map(str::trim)
        && !schedule.is_empty()
    {
        let field_count = schedule.split_whitespace().count();
        if field_count != 5 {
            return Err(DnsError::plugin(format!(
                "cron job '{}' schedule must use standard 5-field cron format; second-level cron is not supported",
                job.name
            )));
        }

        let task_schedule = TaskSchedule::cron(schedule, timezone_name).map_err(|e| {
            DnsError::plugin(format!(
                "failed to parse cron schedule for job '{}': {}",
                job.name, e
            ))
        })?;
        return Ok((task_schedule, "schedule"));
    }

    let interval = parse_interval(
        job.name.as_str(),
        job.interval.as_deref().unwrap_or_default(),
    )?;
    Ok((TaskSchedule::fixed(interval), "interval"))
}

fn parse_executor_ref(raw: &str) -> Result<ExecutorRef> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(DnsError::plugin("executor reference cannot be empty"));
    }
    if let Some(tag) = raw.strip_prefix('$') {
        let tag = tag.trim();
        if tag.is_empty() {
            return Err(DnsError::plugin(format!(
                "invalid executor reference '{}'",
                raw
            )));
        }
        return Ok(ExecutorRef::PluginTag(tag.to_string()));
    }

    let mut split = raw.splitn(2, char::is_whitespace);
    let first = split
        .next()
        .ok_or_else(|| DnsError::plugin(format!("invalid executor reference '{}'", raw)))?;
    let param = split
        .next()
        .map(str::trim)
        .filter(|param| !param.is_empty());

    if matches!(
        registered_plugin_kind(first),
        Some(crate::plugin::dependency::DependencyKind::Executor)
    ) {
        return Ok(ExecutorRef::QuickSetup {
            plugin_type: first.to_string(),
            param: param.map(ToOwned::to_owned),
        });
    }

    Ok(ExecutorRef::PluginTag(raw.to_string()))
}

fn parse_interval(job_name: &str, raw: &str) -> Result<Duration> {
    let raw = raw.trim();
    let duration = parse_simple_duration(raw).map_err(|err| {
        let detail = if raw.chars().all(|c| c.is_ascii_digit()) {
            format!(
                "cron job '{}' interval must include a unit and second-level tasks are not supported",
                job_name
            )
        } else if raw.is_empty() {
            format!("cron job '{}' interval cannot be empty", job_name)
        } else {
            format!("invalid interval '{}' for cron job '{}': {}", raw, job_name, err)
        };
        DnsError::plugin(detail)
    })?;

    if duration < MIN_INTERVAL {
        return Err(DnsError::plugin(format!(
            "cron job '{}' interval must be at least 1 minute; second-level tasks are not supported",
            job_name
        )));
    }

    Ok(duration)
}

fn resolve_timezone_name(configured: Option<&str>) -> String {
    configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let timezone = system_timezone_name();
            if timezone == "UTC" {
                warn!("system timezone name is unavailable; falling back to UTC for cron jobs");
            }
            timezone
        })
}

fn plugin_type_kind_name(plugin_type: crate::plugin::PluginType) -> &'static str {
    match plugin_type {
        crate::plugin::PluginType::Server => "server",
        crate::plugin::PluginType::Executor => "executor",
        crate::plugin::PluginType::Matcher => "matcher",
        crate::plugin::PluginType::Provider => "provider",
    }
}

async fn stop_registered_tasks(handles: &[ManagedTaskHandle]) {
    for handle in handles {
        handle.stop().await;
    }
}

async fn rollback_quick_setup_executors(executors: &mut Vec<Arc<dyn Executor>>) {
    for executor in std::mem::take(executors) {
        if let Err(error) = executor.destroy().await {
            warn!(
                executor = %executor.tag(),
                error = %error,
                "failed to roll back cron quick-setup executor"
            );
        }
    }
}

async fn run_job(
    plugin_tag: String,
    job_name: String,
    trigger_kind: String,
    scheduled_at_ms: i64,
    executors: Vec<Arc<dyn Executor>>,
    metrics: Arc<CronMetrics>,
) {
    metrics.run_total.fetch_add(1, Ordering::Relaxed);
    info!(
        plugin = %plugin_tag,
        job = %job_name,
        trigger = %trigger_kind,
        executor_count = executors.len(),
        "cron job started"
    );

    let mut context = DnsContext::new(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)), Message::new());
    context.set_attr(ATTR_PLUGIN_TAG, plugin_tag.clone());
    context.set_attr(ATTR_JOB_NAME, job_name.clone());
    context.set_attr(ATTR_SCHEDULED_AT_UNIX_MS, scheduled_at_ms);
    context.set_attr(ATTR_TRIGGER_KIND, trigger_kind.clone());

    for executor in executors {
        match executor.execute(&mut context).await {
            Ok(ExecStep::Next) | Ok(ExecStep::Stop) | Ok(ExecStep::Return) => {}
            Err(err) => {
                metrics.executor_error_total.fetch_add(1, Ordering::Relaxed);
                warn!(
                    plugin = %plugin_tag,
                    job = %job_name,
                    executor = %executor.tag(),
                    error = %err,
                    "cron job executor failed"
                );
            }
        }
    }

    info!(
        plugin = %plugin_tag,
        job = %job_name,
        trigger = %trigger_kind,
        "cron job finished"
    );
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use async_trait::async_trait;
    use serde_yaml_ng::Value;
    use tokio::sync::Notify;

    use super::*;
    use crate::plugin::dependency::DependencySpec;
    use crate::plugin::executor::ExecStep;
    use crate::plugin::test_utils::plugin_config;
    use crate::register_plugin_factory;

    #[derive(Debug, Clone, Copy)]
    enum StubBehavior {
        Next,
        Stop,
        Error(&'static str),
    }

    #[derive(Debug)]
    struct StubExecutor {
        tag: String,
        behavior: StubBehavior,
        log: Arc<StdMutex<Vec<String>>>,
        destroyed: Arc<AtomicBool>,
        started: Arc<AtomicUsize>,
    }

    impl StubExecutor {
        fn new(tag: &str, behavior: StubBehavior, log: Arc<StdMutex<Vec<String>>>) -> Self {
            Self {
                tag: tag.to_string(),
                behavior,
                log,
                destroyed: Arc::new(AtomicBool::new(false)),
                started: Arc::new(AtomicUsize::new(0)),
            }
        }
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
            self.destroyed.store(true, Ordering::Relaxed);
            self.log
                .lock()
                .unwrap()
                .push(format!("destroy:{}", self.tag));
            Ok(())
        }
    }

    #[async_trait]
    impl Executor for StubExecutor {
        async fn execute(&self, _context: &mut DnsContext) -> Result<ExecStep> {
            self.started.fetch_add(1, Ordering::Relaxed);
            self.log.lock().unwrap().push(self.tag.clone());
            match self.behavior {
                StubBehavior::Next => Ok(ExecStep::Next),
                StubBehavior::Stop => Ok(ExecStep::Stop),
                StubBehavior::Error(msg) => Err(DnsError::plugin(msg)),
            }
        }
    }

    #[derive(Debug, Clone)]
    struct TestQuickSetupDependencyExecutorFactory;

    register_plugin_factory!(
        "test_cron_quick_dep_exec",
        TestQuickSetupDependencyExecutorFactory {}
    );

    impl PluginFactory for TestQuickSetupDependencyExecutorFactory {
        fn get_quick_setup_dependency_specs(&self, param: Option<&str>) -> Vec<DependencySpec> {
            let Some(tag) = param.map(str::trim).filter(|tag| !tag.is_empty()) else {
                return vec![];
            };
            vec![DependencySpec::provider(
                "provider_tags[0]",
                tag.to_string(),
            )]
        }

        fn create(
            &self,
            plugin_config: &PluginConfig,
            _init_context: &crate::plugin::PluginInitContext<'_>,
        ) -> Result<UninitializedPlugin> {
            Ok(UninitializedPlugin::Executor(Box::new(StubExecutor::new(
                &plugin_config.tag,
                StubBehavior::Next,
                Arc::new(StdMutex::new(Vec::new())),
            ))))
        }

        fn quick_setup(&self, tag: &str, _param: Option<String>) -> Result<UninitializedPlugin> {
            Ok(UninitializedPlugin::Executor(Box::new(StubExecutor::new(
                tag,
                StubBehavior::Next,
                Arc::new(StdMutex::new(Vec::new())),
            ))))
        }
    }

    #[test]
    fn test_parse_executor_ref_supports_tag_and_quick_setup() {
        assert_eq!(
            parse_executor_ref("$abc").unwrap(),
            ExecutorRef::PluginTag("abc".to_string())
        );
        assert_eq!(
            parse_executor_ref("plain_tag").unwrap(),
            ExecutorRef::PluginTag("plain_tag".to_string())
        );
        assert_eq!(
            parse_executor_ref("debug_print hello").unwrap(),
            ExecutorRef::QuickSetup {
                plugin_type: "debug_print".to_string(),
                param: Some("hello".to_string())
            }
        );
    }

    #[test]
    fn test_factory_dependency_specs_expand_quick_setup_dependencies() {
        let cfg = plugin_config(
            "cron",
            "cron",
            Some(
                serde_yaml_ng::from_str(
                    r#"
jobs:
  - name: job
    schedule: "0 * * * *"
    executors:
      - test_cron_quick_dep_exec dep_provider
"#,
                )
                .expect("cron args should parse"),
            ),
        );

        let deps = CronFactory.get_dependency_specs(&cfg);
        assert_eq!(
            deps,
            vec![DependencySpec::provider(
                "args.jobs[0].executors[0] -> quick_setup(test_cron_quick_dep_exec).provider_tags[0]",
                "dep_provider",
            )]
        );
    }

    #[test]
    fn test_parse_job_trigger_rejects_second_level_cron() {
        let job = JobConfig {
            name: "bad".to_string(),
            schedule: Some("0 0 * * * *".to_string()),
            interval: None,
            executors: vec!["$a".to_string()],
        };
        let err = parse_job_trigger_with_timezone(&job, "UTC")
            .expect_err("6-field cron should be rejected");
        assert!(err.to_string().contains("second-level cron"));
    }

    #[test]
    fn test_parse_job_trigger_accepts_explicit_timezone() {
        let job = JobConfig {
            name: "ok".to_string(),
            schedule: Some("0 */6 * * *".to_string()),
            interval: None,
            executors: vec!["$a".to_string()],
        };
        let trigger =
            parse_job_trigger_with_timezone(&job, "Asia/Shanghai").expect("timezone should work");
        match trigger.0 {
            TaskSchedule::Cron { timezone, .. } => assert_eq!(timezone, "Asia/Shanghai"),
            TaskSchedule::FixedInterval { .. } => panic!("expected cron trigger"),
        }
        assert_eq!(trigger.1, "schedule");
    }

    #[test]
    fn test_parse_interval_requires_minute_granularity() {
        assert!(parse_interval("ok", "5m").is_ok());
        assert!(parse_interval("ok", "1h").is_ok());
        let err = parse_interval("bad", "30s").expect_err("30s should be rejected");
        assert!(err.to_string().contains("at least 1 minute"));
    }

    #[tokio::test]
    async fn test_run_job_continues_after_stop_and_error() {
        let log = Arc::new(StdMutex::new(Vec::new()));
        let executors: Vec<Arc<dyn Executor>> = vec![
            Arc::new(StubExecutor::new("first", StubBehavior::Stop, log.clone())),
            Arc::new(StubExecutor::new(
                "second",
                StubBehavior::Error("boom"),
                log.clone(),
            )),
            Arc::new(StubExecutor::new("third", StubBehavior::Next, log.clone())),
        ];

        run_job(
            "cron".to_string(),
            "job".to_string(),
            "interval".to_string(),
            123,
            executors,
            Arc::new(CronMetrics::new("cron".to_string())),
        )
        .await;

        assert_eq!(
            log.lock().unwrap().clone(),
            vec![
                "first".to_string(),
                "second".to_string(),
                "third".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn test_destroy_stops_tasks_before_quick_setup_executors() {
        let log = Arc::new(StdMutex::new(Vec::new()));
        let started = Arc::new(Notify::new());
        let started_task = started.clone();
        let task_log = log.clone();
        let handle = task_center::register_scheduled(
            "cron-destroy-order",
            TaskSchedule::fixed(Duration::from_secs(60)),
            None,
            move |_| {
                let started_task = started_task.clone();
                let task_log = task_log.clone();
                async move {
                    struct DropLog(Arc<StdMutex<Vec<String>>>);

                    impl Drop for DropLog {
                        fn drop(&mut self) {
                            self.0.lock().unwrap().push("task-stopped".to_string());
                        }
                    }

                    let _drop_log = DropLog(task_log);
                    started_task.notify_one();
                    std::future::pending::<()>().await;
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(handle.trigger().await, task_center::TriggerOutcome::Started);
        started.notified().await;

        let quick = Arc::new(StubExecutor::new("quick", StubBehavior::Next, log.clone()));
        let destroyed = quick.destroyed.clone();
        let cron = CronExecutor {
            tag: "cron".to_string(),
            config: CronConfig {
                timezone: None,
                jobs: vec![JobConfig {
                    name: "job".to_string(),
                    schedule: Some("0 * * * *".to_string()),
                    interval: None,
                    executors: vec!["$x".to_string()],
                }],
            },
            quick_setup_executors: vec![quick],
            task_handles: vec![handle],
            metrics: Arc::new(CronMetrics::new("cron".to_string())),
        };

        cron.destroy().await.unwrap();
        assert!(destroyed.load(Ordering::Relaxed));
        assert_eq!(
            log.lock().unwrap().as_slice(),
            ["task-stopped", "destroy:quick"]
        );
    }

    #[test]
    fn test_factory_create_rejects_invalid_args() {
        let factory = CronFactory;
        let cfg = plugin_config("cron", "cron", Some(Value::String("bad".into())));
        assert!(crate::plugin::test_utils::create_plugin_for_test(&factory, &cfg).is_err());
    }

    #[test]
    fn test_resolve_timezone_name_prefers_configured_value() {
        assert_eq!(resolve_timezone_name(Some("UTC")), "UTC");
        assert_eq!(
            resolve_timezone_name(Some("  Asia/Shanghai  ")),
            "Asia/Shanghai"
        );
    }
}
