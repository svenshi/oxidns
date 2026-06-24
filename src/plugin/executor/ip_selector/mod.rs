// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ip_selector` executor plugin.
//!
//! Selects the best A/AAAA records from an existing DNS response by combining
//! cached probe scores with bounded active probing. It is intentionally scoped
//! to response shaping:
//! - it does not race upstream DNS responses (`forward`/`fallback` own that);
//! - it does not implement dual-stack suppression (`prefer_ipv4`/`prefer_ipv6`
//!   own that); and
//! - it does not embed domain rules (`sequence`/matchers/providers own that).
//!
//! Hot-path behavior is fail-open. Probe failures, timeouts, cache misses, or
//! concurrency limits never remove the original response.
//!
//! The intended sequence placement is before cache and before the upstream
//! executor:
//! `ip_selector -> cache -> forward/hosts/...`. Because this executor supports
//! `with_next`, it first lets the rest of the chain populate `context.response`
//! and then applies final response shaping on the return path. That keeps cache
//! entries as the full upstream RRset instead of the cut-down client response.

mod config;
mod metrics;
mod policy;
mod probe;

#[cfg(test)]
mod tests;

use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ahash::AHashMap;
use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::FuturesUnordered;

use self::config::{
    DnssecPolicy, IpSelectorSettings, SelectionMode, parse_ip_selector_config,
    parse_ip_selector_quick_setup,
};
use self::metrics::IpSelectorMetrics;
use self::policy::{
    CandidateRecord, IpScore, ScoreSource, SelectionSource, apply_response_policy,
    candidate_records, eligible_qtype, response_requires_dnssec_caution, unique_candidate_ips,
};
use self::probe::{
    CLEANUP_INTERVAL_SECS, EXPIRED_SWEEP_BATCH, ProbeKey, ProbeRunner, ProbeRuntime, ProbeWaitMode,
    SystemProbeRunner, cached_observation, collect_probe_scores, delay_for_method,
    evict_probe_cache_if_needed, probe_with_runtime,
};
use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::outbound;
use crate::infra::network::proxy::{Socks5Opt, parse_socks5_opt};
use crate::infra::observability::metrics::{register_metric_source, unregister_metric_source};
use crate::infra::task as task_center;
use crate::plugin::executor::{ExecStep, Executor, ExecutorNext};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::proto::RecordType;
use crate::{continue_next, plugin_factory};

#[derive(Debug)]
struct IpSelector {
    /// Plugin tag used by metrics and task names.
    tag: String,
    /// Immutable per-plugin settings parsed at startup.
    settings: IpSelectorSettings,
    /// Shared runtime state used by foreground and background probe paths.
    runtime: Arc<ProbeRuntime>,
    /// Periodic cache cleanup task; absent when cache is disabled.
    cleanup_task_id: Mutex<Option<u64>>,
}

#[async_trait]
impl Plugin for IpSelector {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        self.runtime.metrics.set_cache(self.runtime.cache.clone());
        register_metric_source(self.runtime.metrics.clone())?;

        if self.settings.cache_enabled {
            // TtlCache removes expired entries lazily on access. This background
            // task keeps long-lived resolver processes bounded even when old
            // keys are never queried again.
            let cache = self.runtime.cache.clone();
            let cache_size = self.settings.cache_size;
            let task_id = task_center::spawn_fixed(
                format!("ip_selector:{}:cleanup", self.tag),
                Duration::from_secs(CLEANUP_INTERVAL_SECS),
                move || {
                    let cache = cache.clone();
                    async move {
                        let now = AppClock::elapsed_millis();
                        while cache.remove_expired_batch(now, EXPIRED_SWEEP_BATCH) > 0 {}
                        evict_probe_cache_if_needed(&cache, cache_size);
                    }
                },
            );
            *self
                .cleanup_task_id
                .lock()
                .expect("cleanup_task_id poisoned") = Some(task_id);
        }
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        // Mirror init exactly: unregister metrics first so the registry stops
        // observing this instance before the cleanup task is torn down.
        unregister_metric_source(&self.tag);
        let cleanup_task_id = self
            .cleanup_task_id
            .lock()
            .expect("cleanup_task_id poisoned")
            .take();
        if let Some(task_id) = cleanup_task_id {
            task_center::stop_task(task_id).await;
        }
        Ok(())
    }
}

#[async_trait]
impl Executor for IpSelector {
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
        // `ip_selector` is a return-path processor when used with `with_next`.
        // Downstream executors populate the response first; only then do we sort
        // or cut address records. Without a next executor this simply processes
        // whatever response is already present in the context.
        let step = continue_next!(next, context)?;
        self.select_response_ips(context).await;
        Ok(step)
    }
}

impl IpSelector {
    async fn select_response_ips(&self, context: &mut DnsContext) {
        let Some(qtype) = eligible_qtype(context) else {
            return;
        };

        let dnssec_sensitive = response_requires_dnssec_caution(context, qtype);
        if dnssec_sensitive && self.settings.dnssec_policy == DnssecPolicy::Skip {
            // DNSSEC signatures may cover both content and ordering-sensitive
            // client expectations. In skip mode we leave the packet untouched.
            self.runtime
                .metrics
                .record_selection(SelectionSource::Fallback);
            return;
        }
        let effective_top_n = if dnssec_sensitive {
            // In reorder-only mode do not remove signed RRset members.
            0
        } else {
            self.settings.top_n
        };

        let Some(candidates) = context
            .response()
            .and_then(|response| candidate_records(response, qtype))
        else {
            return;
        };

        let mut scores = self.cached_scores(&candidates);
        if !scores.is_empty() {
            // Cache hits are enough to produce a deterministic policy decision;
            // avoid active probes on the hot path whenever possible.
            self.apply_policy(context, qtype, &scores, effective_top_n);
            return;
        }

        match self.settings.selection_mode {
            SelectionMode::Background => {
                // Background mode prioritizes response latency over immediate
                // optimization. The current query sees upstream order; later
                // queries can benefit from warmed scores.
                self.spawn_background_probes(candidates);
                self.runtime
                    .metrics
                    .record_selection(SelectionSource::Fallback);
            }
            SelectionMode::FirstSuccess => {
                scores = self
                    .probe_scores(candidates, ProbeWaitMode::FirstSuccess)
                    .await;
                self.apply_policy(context, qtype, &scores, effective_top_n);
            }
            SelectionMode::BestWithinBudget => {
                scores = self
                    .probe_scores(candidates, ProbeWaitMode::BestWithinBudget)
                    .await;
                self.apply_policy(context, qtype, &scores, effective_top_n);
            }
        }
    }

    fn cached_scores(&self, candidates: &[CandidateRecord]) -> AHashMap<IpAddr, IpScore> {
        let mut scores = AHashMap::new();
        for ip in unique_candidate_ips(candidates) {
            if let Some(score) = self.best_cached_score(ip) {
                scores.insert(ip, score);
            }
        }
        scores
    }

    fn best_cached_score(&self, ip: IpAddr) -> Option<IpScore> {
        if !self.settings.cache_enabled {
            return None;
        }
        // Multiple methods may have scores for the same IP; use the best
        // successful latency across configured methods, matching the active
        // probing path below.
        self.settings
            .probe_methods
            .iter()
            .copied()
            .filter(|method| method.is_active())
            .filter_map(|method| {
                let key = ProbeKey { ip, method };
                cached_observation(&self.runtime, &key)
                    .and_then(|observation| observation.score(ScoreSource::Cache))
            })
            .min_by_key(|score| score.latency_ms)
    }

    async fn probe_scores(
        &self,
        candidates: Vec<CandidateRecord>,
        wait_mode: ProbeWaitMode,
    ) -> AHashMap<IpAddr, IpScore> {
        let futures = FuturesUnordered::new();
        // Build one future per missing `(IP, method)` score. The futures are not
        // spawned here; dropping this collection on first-success or timeout
        // cancels outstanding foreground work.
        for ip in unique_candidate_ips(&candidates) {
            for (method_idx, method) in self
                .settings
                .probe_methods
                .iter()
                .copied()
                .filter(|method| method.is_active())
                .enumerate()
            {
                let key = ProbeKey { ip, method };
                if cached_observation(&self.runtime, &key).is_some() {
                    continue;
                }
                let runtime = self.runtime.clone();
                let timeout = self.settings.probe_timeout;
                let delay = delay_for_method(self.settings.probe_stagger, method_idx);
                futures.push(async move {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    let observation = probe_with_runtime(runtime, key.clone(), timeout).await;
                    (key, observation)
                });
            }
        }

        collect_probe_scores(futures, wait_mode, self.settings.max_wait).await
    }

    fn spawn_background_probes(&self, candidates: Vec<CandidateRecord>) {
        let mut jobs = Vec::new();
        // Snapshot jobs before spawning so the request task does all filtering
        // synchronously and the background task only performs probe execution.
        for ip in unique_candidate_ips(&candidates) {
            for (method_idx, method) in self
                .settings
                .probe_methods
                .iter()
                .copied()
                .filter(|method| method.is_active())
                .enumerate()
            {
                let key = ProbeKey { ip, method };
                if cached_observation(&self.runtime, &key).is_some() {
                    continue;
                }
                jobs.push((
                    key,
                    delay_for_method(self.settings.probe_stagger, method_idx),
                ));
            }
        }
        if jobs.is_empty() {
            return;
        }

        let runtime = self.runtime.clone();
        let timeout = self.settings.probe_timeout;
        tokio::spawn(async move {
            // Background work still goes through `probe_with_runtime`, so it is
            // bounded by the same semaphore and shares in-flight/cache entries
            // with foreground queries.
            let mut futures = FuturesUnordered::new();
            for (key, delay) in jobs {
                let runtime = runtime.clone();
                futures.push(async move {
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    let _ = probe_with_runtime(runtime, key, timeout).await;
                });
            }
            while futures.next().await.is_some() {}
        });
    }

    fn apply_policy(
        &self,
        context: &mut DnsContext,
        qtype: RecordType,
        scores: &AHashMap<IpAddr, IpScore>,
        top_n: usize,
    ) {
        let Some(response) = context.response_mut() else {
            return;
        };
        let source = apply_response_policy(response, qtype, scores, top_n);
        self.runtime.metrics.record_selection(source);
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("ip_selector")]
pub struct IpSelectorFactory;

impl PluginFactory for IpSelectorFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let settings = parse_ip_selector_config(plugin_config.args.clone())?;
        let runner = build_system_probe_runner(&settings)?;
        Ok(UninitializedPlugin::Executor(Box::new(build_ip_selector(
            plugin_config.tag.clone(),
            settings,
            runner,
        ))))
    }

    fn quick_setup(&self, tag: &str, param: Option<String>) -> Result<UninitializedPlugin> {
        let settings = parse_ip_selector_quick_setup(param)?;
        let runner = build_system_probe_runner(&settings)?;
        Ok(UninitializedPlugin::Executor(Box::new(build_ip_selector(
            tag.to_string(),
            settings,
            runner,
        ))))
    }
}

fn build_system_probe_runner(settings: &IpSelectorSettings) -> Result<Arc<SystemProbeRunner>> {
    Ok(Arc::new(SystemProbeRunner::new(resolve_probe_socks5(
        settings,
    )?)))
}

fn resolve_probe_socks5(settings: &IpSelectorSettings) -> Result<Option<Socks5Opt>> {
    let local_socks5 = match settings.socks5.as_deref() {
        Some(raw) => Some(parse_socks5_opt(raw).ok_or_else(|| {
            DnsError::plugin(format!("ip_selector has invalid socks5 proxy '{}'", raw))
        })?),
        None => None,
    };
    let policy = outbound::global().resolve_policy(settings.outbound.as_deref(), local_socks5)?;
    Ok(policy.proxy())
}

fn build_ip_selector(
    tag: String,
    settings: IpSelectorSettings,
    runner: Arc<dyn ProbeRunner>,
) -> IpSelector {
    // Build runtime state once at plugin initialization. Per-query code only
    // reads these Arcs and does not allocate semaphores, metrics, or caches.
    let metrics = Arc::new(IpSelectorMetrics::new(
        tag.clone(),
        settings.probe_methods.as_slice(),
    ));
    let runtime = Arc::new(ProbeRuntime::new(&settings, runner, metrics));
    IpSelector {
        tag,
        settings,
        runtime,
        cleanup_task_id: Mutex::new(None),
    }
}
