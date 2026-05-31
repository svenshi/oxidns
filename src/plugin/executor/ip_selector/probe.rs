// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Probe execution, probe cache, and concurrency control.
//!
//! All network side effects live here. The response policy never opens sockets,
//! and the executor entry point never manipulates cache internals directly
//! except for lifecycle cleanup. This keeps hot-path failure behavior
//! contained: probe errors and saturation become failed observations, not DNS
//! failures.

#[cfg(unix)]
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ahash::AHashMap;
use async_trait::async_trait;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::{OnceCell, Semaphore};

use super::config::{IpSelectorSettings, ProbeMethod};
use super::metrics::IpSelectorMetrics;
use super::policy::{IpScore, ScoreSource};
use crate::core::app_clock::AppClock;
use crate::core::ttl_cache::TtlCache;

pub(super) const LAST_ACCESS_TOUCH_INTERVAL_MS: u64 = 1000;
pub(super) const CLEANUP_INTERVAL_SECS: u64 = 30;
pub(super) const EXPIRED_SWEEP_BATCH: usize = 512;
const EVICTION_SAMPLE_SIZE: usize = 1024;

pub(super) type ProbeCache = TtlCache<ProbeKey, Arc<ProbeObservation>>;
type InflightProbe = Arc<OnceCell<ProbeObservation>>;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub(super) struct ProbeKey {
    pub(super) ip: IpAddr,
    pub(super) method: ProbeMethod,
}

/// Probe result stored in cache and shared by in-flight waiters.
///
/// Failures are cached too. That prevents one bad IP from being retried on
/// every cold query and keeps the plugin fail-open under repeated network
/// errors.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct ProbeObservation {
    pub(super) success: bool,
    pub(super) latency_ms: Option<u64>,
    pub(super) sampled_at_ms: u64,
}

impl ProbeObservation {
    pub(super) fn success(latency_ms: u64) -> Self {
        Self {
            success: true,
            latency_ms: Some(latency_ms),
            sampled_at_ms: AppClock::elapsed_millis(),
        }
    }

    pub(super) fn failure() -> Self {
        Self {
            success: false,
            latency_ms: None,
            sampled_at_ms: AppClock::elapsed_millis(),
        }
    }

    pub(super) fn score(self, source: ScoreSource) -> Option<IpScore> {
        self.success.then_some(IpScore {
            latency_ms: self.latency_ms.unwrap_or(u64::MAX),
            source,
        })
    }
}

/// Mutable runtime state shared across requests.
///
/// The executor itself stays small and immutable. Hot-path shared structures
/// live here so background probes, in-flight waiters, and metric collection can
/// reference the same cache and concurrency gates without copying
/// configuration.
#[derive(Debug)]
pub(super) struct ProbeRuntime {
    /// TTL-aware cache keyed by `(IP, method)`.
    pub(super) cache: ProbeCache,
    cache_enabled: bool,
    cache_size: usize,
    cache_ttl_ms: u64,
    failure_ttl_ms: u64,
    /// Abstracted for unit tests; production uses `SystemProbeRunner`.
    runner: Arc<dyn ProbeRunner>,
    /// Plugin-wide active-probe limit to cap cold-query fanout.
    semaphore: Arc<Semaphore>,
    /// Coalesces concurrent probes for the same `(IP, method)`.
    inflight: DashMap<ProbeKey, InflightProbe>,
    pub(super) metrics: Arc<IpSelectorMetrics>,
}

impl ProbeRuntime {
    pub(super) fn new(
        settings: &IpSelectorSettings,
        runner: Arc<dyn ProbeRunner>,
        metrics: Arc<IpSelectorMetrics>,
    ) -> Self {
        Self {
            cache: ProbeCache::with_capacity(settings.cache_size),
            cache_enabled: settings.cache_enabled,
            cache_size: settings.cache_size,
            cache_ttl_ms: settings.cache_ttl_ms,
            failure_ttl_ms: settings.failure_ttl_ms,
            runner,
            semaphore: Arc::new(Semaphore::new(settings.max_parallel_probes)),
            inflight: DashMap::new(),
            metrics,
        }
    }
}

/// Probe backend abstraction.
///
/// Keeping this trait narrow makes policy tests deterministic without binding
/// them to real sockets or platform ping behavior.
#[async_trait]
pub(super) trait ProbeRunner: std::fmt::Debug + Send + Sync {
    async fn probe(
        &self,
        key: &ProbeKey,
        timeout: Duration,
        metrics: &IpSelectorMetrics,
    ) -> ProbeObservation;
}

#[derive(Debug)]
pub(super) struct SystemProbeRunner;

#[async_trait]
impl ProbeRunner for SystemProbeRunner {
    async fn probe(
        &self,
        key: &ProbeKey,
        timeout: Duration,
        _metrics: &IpSelectorMetrics,
    ) -> ProbeObservation {
        match key.method {
            ProbeMethod::Tcp(port) => probe_tcp(key.ip, port, timeout).await,
            ProbeMethod::Ping => probe_ping(key.ip, timeout).await,
            ProbeMethod::None => ProbeObservation::failure(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum ProbeWaitMode {
    /// Stop as soon as the first successful score is available.
    FirstSuccess,
    /// Keep collecting successful scores until all probes finish or `max_wait`.
    BestWithinBudget,
}

/// Collect active probe results under the response budget.
///
/// This function returns only successful scores. If every probe fails or the
/// budget expires before success, the caller's response policy will fall back
/// to the original upstream ordering.
pub(super) async fn collect_probe_scores<F>(
    mut futures: FuturesUnordered<F>,
    wait_mode: ProbeWaitMode,
    max_wait: Duration,
) -> AHashMap<IpAddr, IpScore>
where
    F: std::future::Future<Output = (ProbeKey, ProbeObservation)>,
{
    let mut scores = AHashMap::new();
    if futures.is_empty() {
        return scores;
    }

    let deadline = tokio::time::sleep(max_wait);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            maybe_result = futures.next() => {
                let Some((key, observation)) = maybe_result else {
                    break;
                };
                if let Some(score) = observation.score(ScoreSource::Probe) {
                    update_best_score(&mut scores, key.ip, score);
                    if wait_mode == ProbeWaitMode::FirstSuccess {
                        break;
                    }
                }
            }
            _ = &mut deadline => {
                break;
            }
        }
    }
    scores
}

/// Keep the lowest-latency score per IP.
fn update_best_score(scores: &mut AHashMap<IpAddr, IpScore>, ip: IpAddr, score: IpScore) {
    match scores.get(&ip).copied() {
        Some(existing) if existing.latency_ms <= score.latency_ms => {}
        _ => {
            scores.insert(ip, score);
        }
    }
}

pub(super) async fn probe_with_runtime(
    runtime: Arc<ProbeRuntime>,
    key: ProbeKey,
    timeout: Duration,
) -> ProbeObservation {
    if let Some(cached) = cached_observation(&runtime, &key) {
        return cached;
    }

    // OnceCell lets the first request own the real probe while concurrent
    // requests await the same result. The DashMap entry is removed by the owner
    // after completion, but the shared cell remains alive for waiters.
    let (cell, owner) = match runtime.inflight.entry(key.clone()) {
        Entry::Occupied(entry) => {
            runtime.metrics.record_dropped_inflight();
            (entry.get().clone(), false)
        }
        Entry::Vacant(entry) => {
            let cell = Arc::new(OnceCell::new());
            entry.insert(cell.clone());
            (cell, true)
        }
    };

    let observation = *cell
        .get_or_init(|| async {
            // Use try-acquire to preserve the resolver latency budget. If the
            // plugin is saturated we record a bounded failure and keep the
            // original DNS response instead of queueing unbounded work.
            let Ok(_permit) = runtime.semaphore.clone().try_acquire_owned() else {
                runtime.metrics.record_dropped_parallel_limit();
                let observation = ProbeObservation::failure();
                runtime.metrics.record_probe(key.method, observation);
                store_probe_observation(&runtime, key.clone(), observation);
                return observation;
            };

            let observation = runtime
                .runner
                .probe(&key, timeout, runtime.metrics.as_ref())
                .await;
            runtime.metrics.record_probe(key.method, observation);
            store_probe_observation(&runtime, key.clone(), observation);
            observation
        })
        .await;

    if owner {
        runtime.inflight.remove(&key);
    }

    observation
}

pub(super) fn cached_observation(
    runtime: &ProbeRuntime,
    key: &ProbeKey,
) -> Option<ProbeObservation> {
    if !runtime.cache_enabled {
        return None;
    }
    // Reads also refresh last-access metadata, but only once per configured
    // touch interval to keep cache hits cheap under heavy query volume.
    runtime
        .cache
        .get_retained_cloned(
            key,
            AppClock::elapsed_millis(),
            LAST_ACCESS_TOUCH_INTERVAL_MS,
        )
        .map(|entry| *entry.value)
}

fn store_probe_observation(runtime: &ProbeRuntime, key: ProbeKey, observation: ProbeObservation) {
    if !runtime.cache_enabled {
        return;
    }
    let now = AppClock::elapsed_millis();
    // Failed probes deliberately use a shorter TTL. They suppress retry storms
    // for unhealthy IPs while allowing recovery to be noticed quickly.
    let ttl_ms = if observation.success {
        runtime.cache_ttl_ms
    } else {
        runtime.failure_ttl_ms
    };
    runtime
        .cache
        .insert_or_update(key, Arc::new(observation), now, now.saturating_add(ttl_ms));
    evict_probe_cache_if_needed(&runtime.cache, runtime.cache_size);
}

pub(super) fn evict_probe_cache_if_needed(cache: &ProbeCache, cache_size: usize) {
    let current_size = cache.len();
    if current_size <= cache_size {
        return;
    }
    // TtlCache does not require strict LRU for correctness. Sampling keeps the
    // eviction cost bounded while still biasing removal toward cold entries.
    let mut sample = cache.sample_last_access(current_size.min(EVICTION_SAMPLE_SIZE));
    sample.sort_unstable_by_key(|(_, last_access)| *last_access);
    let remove_count = current_size.saturating_sub(cache_size);
    for (key, _) in sample.into_iter().take(remove_count) {
        cache.remove(&key);
    }
}

async fn probe_tcp(ip: IpAddr, port: u16, timeout: Duration) -> ProbeObservation {
    let start = Instant::now();
    let addr = SocketAddr::new(ip, port);
    match tokio::time::timeout(timeout, TcpStream::connect(addr)).await {
        Ok(Ok(_stream)) => {
            let latency_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
            ProbeObservation::success(latency_ms)
        }
        Ok(Err(_)) | Err(_) => ProbeObservation::failure(),
    }
}

async fn probe_ping(ip: IpAddr, timeout: Duration) -> ProbeObservation {
    let start = Instant::now();
    if run_ping(ip, timeout).await {
        let latency_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        ProbeObservation::success(latency_ms)
    } else {
        ProbeObservation::failure()
    }
}

#[cfg(unix)]
async fn run_ping(ip: IpAddr, timeout: Duration) -> bool {
    let ip_arg = ip.to_string();
    if ip.is_ipv6() {
        // Different Unix platforms expose IPv6 ping as either `ping6` or
        // `ping -6`. Try both and treat missing binaries as ordinary failure.
        match run_ping_command(
            "ping6",
            vec!["-c".into(), "1".into(), ip_arg.clone()],
            timeout,
        )
        .await
        {
            Ok(success) => return success,
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(_) => return false,
        }
        return run_ping_command(
            "ping",
            vec!["-6".into(), "-c".into(), "1".into(), ip_arg],
            timeout,
        )
        .await
        .unwrap_or(false);
    }

    run_ping_command("ping", vec!["-c".into(), "1".into(), ip_arg], timeout)
        .await
        .unwrap_or(false)
}

#[cfg(windows)]
async fn run_ping(ip: IpAddr, timeout: Duration) -> bool {
    run_ping_command(
        "ping",
        vec!["-n".into(), "1".into(), ip.to_string()],
        timeout,
    )
    .await
    .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
async fn run_ping(_ip: IpAddr, _timeout: Duration) -> bool {
    // Ping is best-effort; unsupported platforms simply fall back to other
    // methods or to the original DNS answer.
    false
}

async fn run_ping_command(
    program: &str,
    args: Vec<String>,
    timeout: Duration,
) -> std::io::Result<bool> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    // The command output is intentionally ignored. We only need success/failure
    // plus wall-clock elapsed time measured by the caller.
    match tokio::time::timeout(timeout, command.status()).await {
        Ok(Ok(status)) => Ok(status.success()),
        Ok(Err(err)) => Err(err),
        Err(_) => Ok(false),
    }
}

pub(super) fn delay_for_method(stagger: Duration, method_idx: usize) -> Duration {
    // Later methods start slightly after earlier methods so `first_success`
    // gives preferred methods a small head start without blocking fallback
    // methods for the whole timeout.
    stagger
        .checked_mul(method_idx as u32)
        .unwrap_or(Duration::MAX)
}
