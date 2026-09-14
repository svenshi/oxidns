// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `prefer_ipv4` / `prefer_ipv6` quick-setup executors.
//!
//! Behavior:
//! - For preferred qtype (A for prefer_ipv4 / AAAA for prefer_ipv6): pass query
//!   through and cache positive preferred-type answers.
//! - For non-preferred qtype:
//!   1) block immediately when cache says preferred type exists.
//!   2) otherwise run the downstream chain for the original query and a
//!      preferred-type probe concurrently, then block/pass from those outcomes.
//!      The probe can use a dedicated configured executor; without one it uses
//!      the downstream continuation for backward compatibility.
//!
//! Configuration accepts optional `probe_executor`, `cache`, and `cache_ttl`
//! fields. `probe_executor` is a plain executor tag resolved during startup;
//! normal dependency validation rejects missing targets, kind mismatches, and
//! cycles. No dependency resolution or config parsing occurs on the request
//! path.
//!
//! The selector owns a bounded TTL cache and its cleanup task. A non-preferred
//! cache miss starts at most one original task and one probe task, and every
//! decision path joins or cancels both before returning. Probe context state is
//! isolated, although external side effects completed by a probe cannot be
//! rolled back.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_yaml_ng::Value;
use tokio::task::JoinError;
use tokio_util::task::AbortOnDropHandle;

use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::cache::ttl::TtlCache;
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::task as task_center;
use crate::plugin::dependency::DependencySpec;
use crate::plugin::executor::{ExecStep, Executor, ExecutorNext};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::proto::{Rcode, RecordType};
use crate::{continue_next, register_plugin_factory};

const CLEANUP_INTERVAL_SECS: u64 = 30;
const DEFAULT_CACHE_ENABLED: bool = true;
const DEFAULT_CACHE_TTL_SECS: u64 = 60 * 60;
const DEFAULT_CACHE_TTL_MS: u64 = DEFAULT_CACHE_TTL_SECS * 1000;
const PROBE_WAIT_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug)]
struct DualSelector {
    tag: String,
    preferred_type: RecordType,
    probe_executor: Option<Arc<dyn Executor>>,
    cache: TtlCache<String, Arc<CachedPreferredState>>,
    cache_enabled: bool,
    cache_ttl_ms: u64,
    cleanup_started: AtomicBool,
    cleanup_task_handle: Option<task_center::ManagedTaskHandle>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct CachedPreferredState {
    preferred_exists: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PostMode {
    Preferred,
    NonPreferredProbe,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum ExecPlan {
    Bypass,
    Stop,
    Continue { domain: String, mode: PostMode },
}

type SubqueryOutcome = (DnsContext, Result<ExecStep>);
type SubqueryHandle = AbortOnDropHandle<SubqueryOutcome>;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PreferredProbeOutcome {
    HasPreferredAnswer,
    NoPreferredAnswer,
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct DualSelectorConfig {
    /// Optional executor used exclusively for preferred-type probes.
    probe_executor: Option<String>,
    /// Enable preferred-result cache for non-preferred query short-circuiting.
    #[serde(default)]
    cache: Option<bool>,
    /// Cache TTL in seconds for preferred-result probe state.
    cache_ttl: Option<u64>,
}

#[async_trait]
impl Plugin for DualSelector {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        if !self.cache_enabled {
            return Ok(());
        }
        if self.cleanup_started.swap(true, Ordering::Relaxed) {
            return Ok(());
        }

        let cache = self.cache.clone();
        self.cleanup_task_handle = Some(
            match task_center::spawn_fixed(
                format!("dual_selector:{}:cleanup", self.tag),
                Duration::from_secs(CLEANUP_INTERVAL_SECS),
                task_center::TaskOptions::default(),
                move |_| {
                    let cache = cache.clone();
                    async move {
                        let now = AppClock::elapsed_millis();
                        while cache.remove_expired_batch(now, 256) > 0 {}
                    }
                },
            ) {
                Ok(handle) => handle,
                Err(error) => {
                    self.cleanup_started.store(false, Ordering::Relaxed);
                    return Err(error);
                }
            },
        );
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        if let Some(handle) = &self.cleanup_task_handle {
            handle.stop().await;
        }
        self.cleanup_started.store(false, Ordering::Relaxed);
        Ok(())
    }
}

#[async_trait]
impl Executor for DualSelector {
    fn with_next(&self) -> bool {
        true
    }

    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        self.execute_with_next(context, None).await
    }

    #[hotpath::measure]
    async fn execute_with_next(
        &self,
        context: &mut DnsContext,
        next: Option<ExecutorNext>,
    ) -> Result<ExecStep> {
        let plan = self.plan(context);

        match plan {
            ExecPlan::Bypass => continue_next!(next, context),
            ExecPlan::Stop => Ok(ExecStep::Stop),
            ExecPlan::Continue {
                domain,
                mode: PostMode::Preferred,
            } => {
                let step = continue_next!(next, context)?;
                let has_preferred_answer = context
                    .response()
                    .is_some_and(|response| response.has_answer_type(self.preferred_type));
                if has_preferred_answer {
                    self.cache_preferred(&domain);
                }
                Ok(step)
            }
            ExecPlan::Continue {
                domain,
                mode: PostMode::NonPreferredProbe,
            } => {
                self.execute_non_preferred_probe(context, next, &domain)
                    .await
            }
        }
    }
}

impl DualSelector {
    fn plan(&self, context: &mut DnsContext) -> ExecPlan {
        if context.request.question_count() != 1 {
            return ExecPlan::Bypass;
        }

        let Some(qtype) = context.request.first_qtype() else {
            return ExecPlan::Bypass;
        };
        if qtype != RecordType::A && qtype != RecordType::AAAA {
            return ExecPlan::Bypass;
        }

        let Some(domain) = context
            .request
            .first_question()
            .map(|question| question.name().normalized().to_string())
        else {
            return ExecPlan::Bypass;
        };

        if qtype == self.preferred_type {
            return ExecPlan::Continue {
                domain,
                mode: PostMode::Preferred,
            };
        }

        if self.cache_enabled
            && let Some(preferred_exists) = self.cache_get_preferred_state(&domain)
        {
            if preferred_exists {
                context.set_response(context.request().response(Rcode::NoError));
                return ExecPlan::Stop;
            }
            return ExecPlan::Bypass;
        }

        ExecPlan::Continue {
            domain,
            mode: PostMode::NonPreferredProbe,
        }
    }

    async fn execute_non_preferred_probe(
        &self,
        context: &mut DnsContext,
        next: Option<ExecutorNext>,
        domain: &str,
    ) -> Result<ExecStep> {
        let Some(next) = next else {
            return Ok(ExecStep::Next);
        };

        let original_ctx = context.copy_for_subquery();
        let mut preferred_ctx = context.copy_for_subquery();
        if !preferred_ctx
            .request_mut()
            .set_first_qtype(self.preferred_type)
        {
            return continue_next!(Some(next), context);
        }
        preferred_ctx.clear_response();

        let mut original_handle = spawn_subquery(next.clone(), original_ctx);
        let mut preferred_handle = match &self.probe_executor {
            Some(probe_executor) => spawn_executor(probe_executor.clone(), preferred_ctx),
            None => spawn_subquery(next, preferred_ctx),
        };

        tokio::select! {
            original_join = &mut original_handle => {
                self.finish_with_original_first(context, domain, original_join, preferred_handle).await
            }
            preferred_join = &mut preferred_handle => {
                match self.preferred_probe_outcome(preferred_join, domain) {
                    PreferredProbeOutcome::HasPreferredAnswer => {
                        abort_and_reap(original_handle).await;
                        context.set_response(context.request().response(Rcode::NoError));
                        Ok(ExecStep::Stop)
                    }
                    PreferredProbeOutcome::NoPreferredAnswer | PreferredProbeOutcome::Unknown => {
                        self.finish_with_original(context, original_handle.await)
                    }
                }
            }
        }
    }

    async fn finish_with_original_first(
        &self,
        context: &mut DnsContext,
        domain: &str,
        original_join: std::result::Result<SubqueryOutcome, JoinError>,
        mut preferred_handle: SubqueryHandle,
    ) -> Result<ExecStep> {
        match tokio::time::timeout(PROBE_WAIT_TIMEOUT, &mut preferred_handle).await {
            Ok(preferred_join) => {
                if self.preferred_probe_outcome(preferred_join, domain)
                    == PreferredProbeOutcome::HasPreferredAnswer
                {
                    context.set_response(context.request().response(Rcode::NoError));
                    return Ok(ExecStep::Stop);
                }
            }
            Err(_) => abort_and_reap(preferred_handle).await,
        }

        self.finish_with_original(context, original_join)
    }

    fn finish_with_original(
        &self,
        context: &mut DnsContext,
        original_join: std::result::Result<SubqueryOutcome, JoinError>,
    ) -> Result<ExecStep> {
        let (original_ctx, step) = original_join.map_err(join_error)?;
        context.apply_subquery_result(original_ctx);
        step
    }

    fn preferred_probe_outcome(
        &self,
        preferred_join: std::result::Result<SubqueryOutcome, JoinError>,
        domain: &str,
    ) -> PreferredProbeOutcome {
        let Ok((preferred_ctx, Ok(_))) = preferred_join else {
            return PreferredProbeOutcome::Unknown;
        };

        let Some(response) = preferred_ctx.response() else {
            return PreferredProbeOutcome::Unknown;
        };
        if response.truncated() {
            return PreferredProbeOutcome::Unknown;
        }
        if response.has_answer_type(self.preferred_type) {
            if self.cache_enabled {
                self.cache_probe_result(domain, true);
            }
            return PreferredProbeOutcome::HasPreferredAnswer;
        }

        if response.rcode() == Rcode::NoError || response.rcode() == Rcode::NXDomain {
            if self.cache_enabled {
                self.cache_probe_result(domain, false);
            }
            return PreferredProbeOutcome::NoPreferredAnswer;
        }
        PreferredProbeOutcome::Unknown
    }

    fn cache_preferred(&self, domain: &str) {
        if !self.cache_enabled {
            return;
        }
        self.cache_probe_result(domain, true);
    }

    fn cache_probe_result(&self, domain: &str, preferred_exists: bool) {
        let now = AppClock::elapsed_millis();
        let expire_at = now.saturating_add(self.cache_ttl_ms);
        self.cache.insert_or_update(
            domain.to_string(),
            Arc::new(CachedPreferredState { preferred_exists }),
            now,
            expire_at,
        );
    }

    fn cache_get_preferred_state(&self, domain: &String) -> Option<bool> {
        let now = AppClock::elapsed_millis();
        self.cache
            .get_retained_cloned(domain, now, 1000)
            .map(|entry| entry.value.preferred_exists)
    }
}

fn spawn_subquery(next: ExecutorNext, mut context: DnsContext) -> SubqueryHandle {
    AbortOnDropHandle::new(tokio::spawn(async move {
        let step = next.next(&mut context).await;
        (context, step)
    }))
}

fn spawn_executor(executor: Arc<dyn Executor>, mut context: DnsContext) -> SubqueryHandle {
    AbortOnDropHandle::new(tokio::spawn(async move {
        let step = executor.execute_with_next(&mut context, None).await;
        (context, step)
    }))
}

async fn abort_and_reap(mut handle: SubqueryHandle) {
    handle.abort();
    let _ = (&mut handle).await;
}

fn join_error(err: JoinError) -> DnsError {
    DnsError::runtime(format!("dual_selector subquery join failed: {err}"))
}

#[derive(Debug, Clone)]
pub struct DualSelectorFactory {
    record_type: RecordType,
}

register_plugin_factory!("prefer_ipv4", DualSelectorFactory::new(RecordType::A));
register_plugin_factory!("prefer_ipv6", DualSelectorFactory::new(RecordType::AAAA));

impl DualSelectorFactory {
    fn new(record_type: RecordType) -> Self {
        Self { record_type }
    }
}

#[derive(Debug)]
struct ParsedDualSelectorConfig {
    probe_executor: Option<String>,
    cache_enabled: bool,
    cache_ttl_ms: u64,
}

fn parse_dual_selector_config(args: Option<Value>) -> Result<ParsedDualSelectorConfig> {
    let cfg = match args {
        Some(args) => serde_yaml_ng::from_value::<DualSelectorConfig>(args).map_err(|e| {
            DnsError::plugin(format!("failed to parse dual_selector config: {}", e))
        })?,
        None => DualSelectorConfig::default(),
    };

    let probe_executor = normalize_probe_executor(cfg.probe_executor)?;

    let cache_enabled = cfg.cache.unwrap_or(DEFAULT_CACHE_ENABLED);
    let cache_ttl_secs = cfg.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL_SECS);
    if cache_enabled && cache_ttl_secs == 0 {
        return Err(DnsError::plugin(
            "dual_selector cache_ttl must be greater than 0 seconds",
        ));
    }
    let cache_ttl_ms = if cache_ttl_secs == 0 {
        DEFAULT_CACHE_TTL_MS
    } else {
        cache_ttl_secs.saturating_mul(1000)
    };
    Ok(ParsedDualSelectorConfig {
        probe_executor,
        cache_enabled,
        cache_ttl_ms,
    })
}

fn normalize_probe_executor(configured_probe_executor: Option<String>) -> Result<Option<String>> {
    let probe_executor = configured_probe_executor
        .as_deref()
        .map(str::trim)
        .filter(|tag| !tag.is_empty());
    if configured_probe_executor.is_some() && probe_executor.is_none() {
        return Err(DnsError::plugin(
            "dual_selector probe_executor cannot be empty",
        ));
    }
    Ok(probe_executor.map(ToOwned::to_owned))
}

impl PluginFactory for DualSelectorFactory {
    fn get_dependency_specs(&self, plugin_config: &PluginConfig) -> Vec<DependencySpec> {
        plugin_config
            .args
            .clone()
            .and_then(|args| serde_yaml_ng::from_value::<DualSelectorConfig>(args).ok())
            .and_then(|cfg| normalize_probe_executor(cfg.probe_executor).ok().flatten())
            .map(|tag| vec![DependencySpec::executor("args.probe_executor", tag)])
            .unwrap_or_default()
    }

    fn create(
        &self,
        plugin_config: &PluginConfig,
        init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let cfg = parse_dual_selector_config(plugin_config.args.clone())?;
        let probe_executor = cfg
            .probe_executor
            .as_deref()
            .map(|tag| init_context.executor("args.probe_executor", tag))
            .transpose()?;
        Ok(UninitializedPlugin::Executor(Box::new(DualSelector {
            tag: plugin_config.tag.clone(),
            preferred_type: self.record_type,
            probe_executor,
            cache: TtlCache::with_capacity(4096),
            cache_enabled: cfg.cache_enabled,
            cache_ttl_ms: cfg.cache_ttl_ms,
            cleanup_started: AtomicBool::new(false),
            cleanup_task_handle: None,
        })))
    }

    fn quick_setup(&self, tag: &str, _param: Option<String>) -> Result<UninitializedPlugin> {
        Ok(UninitializedPlugin::Executor(Box::new(DualSelector {
            tag: tag.to_string(),
            preferred_type: self.record_type,
            probe_executor: None,
            cache: TtlCache::with_capacity(4096),
            cache_enabled: DEFAULT_CACHE_ENABLED,
            cache_ttl_ms: DEFAULT_CACHE_TTL_MS,
            cleanup_started: AtomicBool::new(false),
            cleanup_task_handle: None,
        })))
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::plugin::executor::sequence::chain::ChainProgram;
    use crate::plugin::executor::{ExecStep, Executor};
    use crate::plugin::test_utils::plugin_config;
    use crate::proto::rdata::{A, AAAA};
    use crate::proto::{DNSClass, Message, Name, Question, RData, Record};

    fn make_context(qtype: RecordType) -> DnsContext {
        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii("example.com.").unwrap(),
            qtype,
            DNSClass::IN,
        ));
        DnsContext::new("127.0.0.1:5533".parse().unwrap(), request)
    }

    fn make_selector(preferred_type: RecordType) -> DualSelector {
        DualSelector {
            tag: "dual_selector_test".to_string(),
            preferred_type,
            probe_executor: None,
            cache: TtlCache::with_capacity(1024),
            cache_enabled: true,
            cache_ttl_ms: DEFAULT_CACHE_TTL_MS,
            cleanup_started: AtomicBool::new(false),
            cleanup_task_handle: None,
        }
    }

    fn make_selector_with_probe(
        preferred_type: RecordType,
        probe_executor: Arc<dyn Executor>,
    ) -> DualSelector {
        let mut selector = make_selector(preferred_type);
        selector.probe_executor = Some(probe_executor);
        selector
    }

    #[test]
    fn dependency_specs_normalize_probe_executor_tag() {
        let factory = DualSelectorFactory::new(RecordType::A);
        let args = serde_yaml_ng::from_str("probe_executor: ' probe_sequence '")
            .expect("probe executor config should parse");
        let config = plugin_config("selector", "prefer_ipv4", Some(args));

        assert_eq!(
            factory.get_dependency_specs(&config),
            vec![DependencySpec::executor(
                "args.probe_executor",
                "probe_sequence"
            )]
        );
    }

    fn set_answer(context: &mut DnsContext, qtype: RecordType) {
        let qname = context
            .request
            .first_question()
            .expect("question must exist")
            .name()
            .clone();
        let mut response = context.request.response(Rcode::NoError);
        match qtype {
            RecordType::A => response.answers_mut().push(Record::from_rdata(
                qname,
                60,
                RData::A(A(Ipv4Addr::new(1, 2, 3, 4))),
            )),
            RecordType::AAAA => response.answers_mut().push(Record::from_rdata(
                qname,
                60,
                RData::AAAA(AAAA(Ipv6Addr::LOCALHOST)),
            )),
            _ => {}
        }
        context.set_response(response);
    }

    fn has_answer_of_type(context: &DnsContext, qtype: RecordType) -> bool {
        context.response().is_some_and(|response| {
            response
                .answers()
                .iter()
                .any(|answer| answer.rr_type() == qtype)
        })
    }

    #[derive(Debug)]
    struct StubNextExecutor {
        answer_a: bool,
        answer_aaaa: bool,
        delay_a: Duration,
        delay_aaaa: Duration,
        error_a: Option<&'static str>,
        error_aaaa: Option<&'static str>,
        mark_a: Option<u32>,
        mark_aaaa: Option<u32>,
        calls: Arc<AtomicUsize>,
    }

    impl StubNextExecutor {
        fn new(answer_a: bool, answer_aaaa: bool) -> Self {
            Self {
                answer_a,
                answer_aaaa,
                delay_a: Duration::ZERO,
                delay_aaaa: Duration::ZERO,
                error_a: None,
                error_aaaa: None,
                mark_a: None,
                mark_aaaa: None,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn calls(&self) -> Arc<AtomicUsize> {
            self.calls.clone()
        }
    }

    #[async_trait]
    impl Plugin for StubNextExecutor {
        fn tag(&self) -> &str {
            "stub_next"
        }

        async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
            Ok(())
        }

        async fn destroy(&self) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Executor for StubNextExecutor {
        async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match context.request().first_qtype() {
                Some(RecordType::A) => {
                    if !self.delay_a.is_zero() {
                        tokio::time::sleep(self.delay_a).await;
                    }
                    if let Some(err) = self.error_a {
                        return Err(DnsError::plugin(err));
                    }
                    if let Some(mark) = self.mark_a {
                        context.marks_mut().insert(mark);
                    }
                    if self.answer_a {
                        set_answer(context, RecordType::A);
                    } else {
                        context.set_response(context.request().response(Rcode::NoError));
                    }
                }
                Some(RecordType::AAAA) => {
                    if !self.delay_aaaa.is_zero() {
                        tokio::time::sleep(self.delay_aaaa).await;
                    }
                    if let Some(err) = self.error_aaaa {
                        return Err(DnsError::plugin(err));
                    }
                    if let Some(mark) = self.mark_aaaa {
                        context.marks_mut().insert(mark);
                    }
                    if self.answer_aaaa {
                        set_answer(context, RecordType::AAAA);
                    } else {
                        context.set_response(context.request().response(Rcode::NoError));
                    }
                }
                _ => {}
            }
            Ok(ExecStep::Next)
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum ProbeResponse {
        Answer,
        NoData,
        NxDomain,
        ServFail,
        Truncated,
        NoResponse,
        Error,
    }

    #[derive(Debug)]
    struct DedicatedProbeExecutor {
        response: ProbeResponse,
        mark: Option<u32>,
        calls: Arc<AtomicUsize>,
    }

    impl DedicatedProbeExecutor {
        fn new(response: ProbeResponse) -> Self {
            Self {
                response,
                mark: None,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn calls(&self) -> Arc<AtomicUsize> {
            self.calls.clone()
        }
    }

    #[async_trait]
    impl Plugin for DedicatedProbeExecutor {
        fn tag(&self) -> &str {
            "dedicated_probe"
        }
    }

    #[async_trait]
    impl Executor for DedicatedProbeExecutor {
        async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(mark) = self.mark {
                context.marks_mut().insert(mark);
            }
            match self.response {
                ProbeResponse::Answer => {
                    let qtype = context
                        .request()
                        .first_qtype()
                        .expect("probe request should have qtype");
                    set_answer(context, qtype);
                }
                ProbeResponse::NoData => {
                    context.set_response(context.request().response(Rcode::NoError));
                }
                ProbeResponse::NxDomain => {
                    context.set_response(context.request().response(Rcode::NXDomain));
                }
                ProbeResponse::ServFail => {
                    context.set_response(context.request().response(Rcode::ServFail));
                }
                ProbeResponse::Truncated => {
                    let mut response = context.request().response(Rcode::NoError);
                    response.set_truncated(true);
                    context.set_response(response);
                }
                ProbeResponse::NoResponse => {}
                ProbeResponse::Error => return Err(DnsError::plugin("probe failed")),
            }
            Ok(ExecStep::Next)
        }
    }

    struct CancellationGuard {
        cancelled: Arc<AtomicBool>,
        completed: bool,
    }

    impl CancellationGuard {
        fn new(cancelled: Arc<AtomicBool>) -> Self {
            Self {
                cancelled,
                completed: false,
            }
        }

        fn complete(&mut self) {
            self.completed = true;
        }
    }

    impl Drop for CancellationGuard {
        fn drop(&mut self) {
            if !self.completed {
                self.cancelled.store(true, Ordering::SeqCst);
            }
        }
    }

    #[derive(Debug)]
    struct OriginalCancellationExecutor {
        original_started: Arc<AtomicBool>,
        original_cancelled: Arc<AtomicBool>,
    }

    #[async_trait]
    impl Plugin for OriginalCancellationExecutor {
        fn tag(&self) -> &str {
            "original_cancellation"
        }
    }

    #[async_trait]
    impl Executor for OriginalCancellationExecutor {
        async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
            if context.request().first_qtype() == Some(RecordType::AAAA) {
                self.original_started.store(true, Ordering::SeqCst);
                let mut guard = CancellationGuard::new(self.original_cancelled.clone());
                tokio::time::sleep(Duration::from_secs(60)).await;
                guard.complete();
                set_answer(context, RecordType::AAAA);
            } else {
                while !self.original_started.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
                set_answer(context, RecordType::A);
            }
            Ok(ExecStep::Next)
        }
    }

    #[derive(Debug)]
    struct HangingProbeExecutor {
        started: Arc<AtomicBool>,
        cancelled: Arc<AtomicBool>,
    }

    #[async_trait]
    impl Plugin for HangingProbeExecutor {
        fn tag(&self) -> &str {
            "hanging_probe"
        }
    }

    #[async_trait]
    impl Executor for HangingProbeExecutor {
        async fn execute(&self, _context: &mut DnsContext) -> Result<ExecStep> {
            let mut guard = CancellationGuard::new(self.cancelled.clone());
            self.started.store(true, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_secs(60)).await;
            guard.complete();
            Ok(ExecStep::Next)
        }
    }

    #[derive(Debug)]
    struct WaitForProbeOriginalExecutor {
        probe_started: Arc<AtomicBool>,
    }

    #[async_trait]
    impl Plugin for WaitForProbeOriginalExecutor {
        fn tag(&self) -> &str {
            "wait_for_probe_original"
        }
    }

    #[async_trait]
    impl Executor for WaitForProbeOriginalExecutor {
        async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
            while !self.probe_started.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
            set_answer(context, RecordType::AAAA);
            Ok(ExecStep::Next)
        }
    }

    fn make_next(executor: Arc<dyn Executor>) -> ExecutorNext {
        let program = ChainProgram::single_with_next_executor_for_test(executor);
        ExecutorNext::from_program_for_test(program, 0)
    }

    async fn run_selector(
        selector: &DualSelector,
        context: &mut DnsContext,
        next: Option<ExecutorNext>,
    ) -> Result<ExecStep> {
        selector.execute_with_next(context, next).await
    }

    #[tokio::test]
    async fn cache_hit_blocks_non_preferred_immediately() {
        AppClock::start();
        let selector = make_selector(RecordType::A);
        selector.cache_preferred("example.com");

        let mut context = make_context(RecordType::AAAA);
        let step = run_selector(&selector, &mut context, None).await.unwrap();

        assert!(matches!(step, ExecStep::Stop));
        assert!(!has_answer_of_type(&context, RecordType::AAAA));
    }

    #[tokio::test]
    async fn preferred_post_warms_cache_for_next_non_preferred_request() {
        AppClock::start();
        let selector = make_selector(RecordType::A);
        let mut preferred_context = make_context(RecordType::A);
        let next = make_next(Arc::new(StubNextExecutor::new(true, true)));
        run_selector(&selector, &mut preferred_context, Some(next))
            .await
            .unwrap();

        let mut non_preferred_context = make_context(RecordType::AAAA);
        let step2 = run_selector(&selector, &mut non_preferred_context, None)
            .await
            .unwrap();
        assert!(matches!(step2, ExecStep::Stop));
        assert!(!has_answer_of_type(
            &non_preferred_context,
            RecordType::AAAA
        ));
    }

    #[tokio::test]
    async fn dedicated_probe_suppresses_without_leaking_probe_marks() {
        AppClock::start();
        let mut probe = DedicatedProbeExecutor::new(ProbeResponse::Answer);
        probe.mark = Some(20);
        let probe_calls = probe.calls();
        let selector = make_selector_with_probe(RecordType::A, Arc::new(probe));
        let mut context = make_context(RecordType::AAAA);
        context.marks_mut().insert(1);
        let mut original = StubNextExecutor::new(false, true);
        original.delay_aaaa = Duration::from_millis(100);
        original.mark_aaaa = Some(10);

        let step = run_selector(&selector, &mut context, Some(make_next(Arc::new(original))))
            .await
            .unwrap();

        assert_eq!(step, ExecStep::Stop);
        assert_eq!(context.request().first_qtype(), Some(RecordType::AAAA));
        let response = context.response().expect("selector should suppress AAAA");
        assert_eq!(response.rcode(), Rcode::NoError);
        assert!(response.answers().is_empty());
        assert!(context.marks().contains(&1));
        assert!(!context.marks().contains(&10));
        assert!(!context.marks().contains(&20));
        assert_eq!(probe_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dedicated_probe_does_not_repeat_outer_continuation() {
        AppClock::start();
        let probe = DedicatedProbeExecutor::new(ProbeResponse::NoData);
        let probe_calls = probe.calls();
        let selector = make_selector_with_probe(RecordType::A, Arc::new(probe));
        let mut context = make_context(RecordType::AAAA);
        let original = StubNextExecutor::new(false, true);
        let original_calls = original.calls();

        let step = run_selector(&selector, &mut context, Some(make_next(Arc::new(original))))
            .await
            .unwrap();

        assert_eq!(step, ExecStep::Next);
        assert!(has_answer_of_type(&context, RecordType::AAAA));
        assert_eq!(original_calls.load(Ordering::SeqCst), 1);
        assert_eq!(probe_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn preferred_query_does_not_run_dedicated_probe() {
        AppClock::start();
        let probe = DedicatedProbeExecutor::new(ProbeResponse::Answer);
        let probe_calls = probe.calls();
        let selector = make_selector_with_probe(RecordType::A, Arc::new(probe));
        let mut context = make_context(RecordType::A);
        let original = StubNextExecutor::new(true, false);
        let original_calls = original.calls();

        let step = run_selector(&selector, &mut context, Some(make_next(Arc::new(original))))
            .await
            .unwrap();

        assert_eq!(step, ExecStep::Next);
        assert!(has_answer_of_type(&context, RecordType::A));
        assert_eq!(original_calls.load(Ordering::SeqCst), 1);
        assert_eq!(probe_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn dedicated_probe_unknown_adopts_original_context_and_marks() {
        AppClock::start();
        let mut probe = DedicatedProbeExecutor::new(ProbeResponse::ServFail);
        probe.mark = Some(20);
        let selector = make_selector_with_probe(RecordType::A, Arc::new(probe));
        let mut context = make_context(RecordType::AAAA);
        context.marks_mut().insert(1);
        let mut original = StubNextExecutor::new(false, true);
        original.mark_aaaa = Some(10);

        let step = run_selector(&selector, &mut context, Some(make_next(Arc::new(original))))
            .await
            .unwrap();

        assert_eq!(step, ExecStep::Next);
        assert!(has_answer_of_type(&context, RecordType::AAAA));
        assert!(context.marks().contains(&1));
        assert!(context.marks().contains(&10));
        assert!(!context.marks().contains(&20));
    }

    #[tokio::test]
    async fn dedicated_probe_definitive_absence_is_cached() {
        for response in [ProbeResponse::NoData, ProbeResponse::NxDomain] {
            AppClock::start();
            let probe = DedicatedProbeExecutor::new(response);
            let probe_calls = probe.calls();
            let selector = make_selector_with_probe(RecordType::A, Arc::new(probe));

            for _ in 0..2 {
                let mut context = make_context(RecordType::AAAA);
                let original = StubNextExecutor::new(false, true);
                let step =
                    run_selector(&selector, &mut context, Some(make_next(Arc::new(original))))
                        .await
                        .unwrap();
                assert_eq!(step, ExecStep::Next);
                assert!(has_answer_of_type(&context, RecordType::AAAA));
            }

            assert_eq!(probe_calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn dedicated_probe_unknown_outcomes_are_not_cached() {
        for response in [
            ProbeResponse::ServFail,
            ProbeResponse::Truncated,
            ProbeResponse::NoResponse,
            ProbeResponse::Error,
        ] {
            AppClock::start();
            let probe = DedicatedProbeExecutor::new(response);
            let probe_calls = probe.calls();
            let selector = make_selector_with_probe(RecordType::A, Arc::new(probe));

            for _ in 0..2 {
                let mut context = make_context(RecordType::AAAA);
                let original = StubNextExecutor::new(false, true);
                run_selector(&selector, &mut context, Some(make_next(Arc::new(original))))
                    .await
                    .unwrap();
                assert!(has_answer_of_type(&context, RecordType::AAAA));
            }

            assert_eq!(probe_calls.load(Ordering::SeqCst), 2);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn preferred_winner_cancels_and_reaps_original_task() {
        AppClock::start();
        let original_started = Arc::new(AtomicBool::new(false));
        let original_cancelled = Arc::new(AtomicBool::new(false));
        let executor = OriginalCancellationExecutor {
            original_started,
            original_cancelled: original_cancelled.clone(),
        };
        let selector = make_selector(RecordType::A);
        let mut context = make_context(RecordType::AAAA);

        let step = run_selector(&selector, &mut context, Some(make_next(Arc::new(executor))))
            .await
            .unwrap();

        assert_eq!(step, ExecStep::Stop);
        assert!(original_cancelled.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn probe_timeout_cancels_and_reaps_probe_task() {
        AppClock::start();
        let probe_started = Arc::new(AtomicBool::new(false));
        let probe_cancelled = Arc::new(AtomicBool::new(false));
        let probe = HangingProbeExecutor {
            started: probe_started.clone(),
            cancelled: probe_cancelled.clone(),
        };
        let selector = make_selector_with_probe(RecordType::A, Arc::new(probe));
        let original = WaitForProbeOriginalExecutor { probe_started };
        let mut context = make_context(RecordType::AAAA);

        let step = run_selector(&selector, &mut context, Some(make_next(Arc::new(original))))
            .await
            .unwrap();

        assert_eq!(step, ExecStep::Next);
        assert!(has_answer_of_type(&context, RecordType::AAAA));
        assert!(probe_cancelled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn parent_cancellation_aborts_original_and_probe_tasks() {
        AppClock::start();
        let original_started = Arc::new(AtomicBool::new(false));
        let original_cancelled = Arc::new(AtomicBool::new(false));
        let original = HangingProbeExecutor {
            started: original_started.clone(),
            cancelled: original_cancelled.clone(),
        };
        let probe_started = Arc::new(AtomicBool::new(false));
        let probe_cancelled = Arc::new(AtomicBool::new(false));
        let probe = HangingProbeExecutor {
            started: probe_started.clone(),
            cancelled: probe_cancelled.clone(),
        };
        let selector = make_selector_with_probe(RecordType::A, Arc::new(probe));

        let parent = tokio::spawn(async move {
            let mut context = make_context(RecordType::AAAA);
            run_selector(&selector, &mut context, Some(make_next(Arc::new(original)))).await
        });

        tokio::time::timeout(Duration::from_secs(1), async {
            while !original_started.load(Ordering::SeqCst) || !probe_started.load(Ordering::SeqCst)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("both subqueries should start");

        parent.abort();
        let parent_result = parent.await;
        assert!(parent_result.is_err_and(|err| err.is_cancelled()));

        tokio::time::timeout(Duration::from_secs(1), async {
            while !original_cancelled.load(Ordering::SeqCst)
                || !probe_cancelled.load(Ordering::SeqCst)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("both subqueries should be cancelled with their parent");
    }

    #[tokio::test]
    async fn dedicated_probe_supports_prefer_ipv6_symmetrically() {
        AppClock::start();
        let probe = DedicatedProbeExecutor::new(ProbeResponse::Answer);
        let selector = make_selector_with_probe(RecordType::AAAA, Arc::new(probe));
        let mut context = make_context(RecordType::A);
        let original = StubNextExecutor::new(true, false);

        let step = run_selector(&selector, &mut context, Some(make_next(Arc::new(original))))
            .await
            .unwrap();

        assert_eq!(step, ExecStep::Stop);
        assert_eq!(context.request().first_qtype(), Some(RecordType::A));
        assert!(
            context
                .response()
                .is_some_and(|response| response.answers().is_empty())
        );
    }

    #[tokio::test]
    async fn non_preferred_concurrent_probe_blocks_when_preferred_exists() {
        AppClock::start();
        let selector = make_selector(RecordType::A);
        let mut context = make_context(RecordType::AAAA);
        let next = make_next(Arc::new(StubNextExecutor::new(true, true)));

        run_selector(&selector, &mut context, Some(next))
            .await
            .unwrap();
        assert!(!has_answer_of_type(&context, RecordType::AAAA));

        let mut second = make_context(RecordType::AAAA);
        let step2 = run_selector(&selector, &mut second, None).await.unwrap();
        assert!(matches!(step2, ExecStep::Stop));
        assert!(!has_answer_of_type(&second, RecordType::AAAA));
    }

    #[tokio::test]
    async fn non_preferred_without_preferred_answer_is_cached_to_skip_next_probe() {
        AppClock::start();
        let selector = make_selector(RecordType::A);
        let mut first = make_context(RecordType::AAAA);
        let executor = StubNextExecutor::new(false, true);
        let calls = executor.calls();
        run_selector(&selector, &mut first, Some(make_next(Arc::new(executor))))
            .await
            .unwrap();
        assert!(has_answer_of_type(&first, RecordType::AAAA));
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let mut second = make_context(RecordType::AAAA);
        let executor = StubNextExecutor::new(false, true);
        let calls = executor.calls();
        let step2 = run_selector(&selector, &mut second, Some(make_next(Arc::new(executor))))
            .await
            .unwrap();
        assert!(matches!(step2, ExecStep::Next));
        assert!(has_answer_of_type(&second, RecordType::AAAA));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cache_disabled_always_probes_non_preferred() {
        let mut selector = make_selector(RecordType::A);
        selector.cache_enabled = false;

        let mut first = make_context(RecordType::AAAA);
        let executor = StubNextExecutor::new(false, true);
        let first_calls = executor.calls();
        run_selector(&selector, &mut first, Some(make_next(Arc::new(executor))))
            .await
            .unwrap();
        assert_eq!(first_calls.load(Ordering::SeqCst), 2);

        let mut second = make_context(RecordType::AAAA);
        let executor = StubNextExecutor::new(false, true);
        let second_calls = executor.calls();
        let step2 = run_selector(&selector, &mut second, Some(make_next(Arc::new(executor))))
            .await
            .unwrap();
        assert!(matches!(step2, ExecStep::Next));
        assert_eq!(second_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn non_preferred_returns_original_error_when_probe_not_blocking() {
        AppClock::start();
        let selector = make_selector(RecordType::A);
        let mut context = make_context(RecordType::AAAA);
        let mut executor = StubNextExecutor::new(false, false);
        executor.error_aaaa = Some("forward original query failed");

        let err = run_selector(&selector, &mut context, Some(make_next(Arc::new(executor))))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("forward original query failed"));
    }

    #[tokio::test]
    async fn preferred_probe_error_does_not_block_or_warm_cache() {
        AppClock::start();
        let selector = make_selector(RecordType::A);
        let mut context = make_context(RecordType::AAAA);
        let mut executor = StubNextExecutor::new(true, true);
        executor.error_a = Some("probe failed");

        run_selector(&selector, &mut context, Some(make_next(Arc::new(executor))))
            .await
            .unwrap();
        assert!(has_answer_of_type(&context, RecordType::AAAA));

        let mut second = make_context(RecordType::AAAA);
        let executor = StubNextExecutor::new(true, true);
        let calls = executor.calls();
        let step2 = run_selector(&selector, &mut second, Some(make_next(Arc::new(executor))))
            .await
            .unwrap();
        assert!(matches!(step2, ExecStep::Stop));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn preferred_probe_timeout_does_not_block_or_warm_cache() {
        AppClock::start();
        let selector = make_selector(RecordType::A);
        let mut context = make_context(RecordType::AAAA);
        let mut executor = StubNextExecutor::new(true, true);
        executor.delay_a = PROBE_WAIT_TIMEOUT + Duration::from_millis(100);

        run_selector(&selector, &mut context, Some(make_next(Arc::new(executor))))
            .await
            .unwrap();
        assert!(has_answer_of_type(&context, RecordType::AAAA));

        let mut second = make_context(RecordType::AAAA);
        let executor = StubNextExecutor::new(false, true);
        let calls = executor.calls();
        let step2 = run_selector(&selector, &mut second, Some(make_next(Arc::new(executor))))
            .await
            .unwrap();
        assert!(matches!(step2, ExecStep::Next));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
