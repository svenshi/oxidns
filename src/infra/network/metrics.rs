// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Low-cardinality metrics for resolver and bootstrap upstream cold paths.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::infra::clock::AppClock;
use crate::infra::error::Result;
use crate::infra::network::upstream::{ConnectionInfo, ConnectionType};
use crate::infra::observability::metrics::{
    MetricLabel, MetricSample, MetricSink, MetricSource, register_metric_source,
};

const NETWORK_METRICS_TAG: &str = "oxidns_builtin_network";
pub(crate) const OUTBOUND_PROFILE_LOCAL: &str = "__local";
pub(crate) const OUTBOUND_PROFILE_SYSTEM: &str = "__system";
const PROTOCOL_COUNT: usize = 6;
const REASON_COUNT: usize = 3;

static NETWORK_METRICS: OnceLock<Arc<NetworkMetrics>> = OnceLock::new();
static NETWORK_METRICS_REGISTERED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NetworkProtocol {
    Udp,
    Tcp,
    Dot,
    Doq,
    Doh2,
    Doh3,
}

impl NetworkProtocol {
    const ALL: [Self; PROTOCOL_COUNT] = [
        Self::Udp,
        Self::Tcp,
        Self::Dot,
        Self::Doq,
        Self::Doh2,
        Self::Doh3,
    ];

    #[inline]
    pub(crate) fn from_connection_info(connection_info: &ConnectionInfo) -> Self {
        match connection_info.connection_type {
            ConnectionType::UDP => Self::Udp,
            ConnectionType::TCP => Self::Tcp,
            ConnectionType::DoT => Self::Dot,
            ConnectionType::DoQ => Self::Doq,
            ConnectionType::DoH if connection_info.enable_http3 => Self::Doh3,
            ConnectionType::DoH => Self::Doh2,
        }
    }

    #[inline]
    const fn as_index(self) -> usize {
        match self {
            Self::Udp => 0,
            Self::Tcp => 1,
            Self::Dot => 2,
            Self::Doq => 3,
            Self::Doh2 => 4,
            Self::Doh3 => 5,
        }
    }

    #[inline]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Tcp => "tcp",
            Self::Dot => "dot",
            Self::Doq => "doq",
            Self::Doh2 => "doh2",
            Self::Doh3 => "doh3",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PoolRefreshReason {
    Init,
    IpChanged,
    TtlOnly,
}

impl PoolRefreshReason {
    const ALL: [Self; REASON_COUNT] = [Self::Init, Self::IpChanged, Self::TtlOnly];

    #[inline]
    const fn as_index(self) -> usize {
        match self {
            Self::Init => 0,
            Self::IpChanged => 1,
            Self::TtlOnly => 2,
        }
    }

    #[inline]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::IpChanged => "ip_changed",
            Self::TtlOnly => "ttl_only",
        }
    }
}

#[derive(Debug, Default)]
struct PoolRefreshCounters {
    total: AtomicU64,
    latency_ms_total: AtomicU64,
}

impl PoolRefreshCounters {
    #[inline]
    fn record(&self, started_at_ms: u64) {
        self.total.fetch_add(1, Ordering::Relaxed);
        self.latency_ms_total
            .fetch_add(elapsed_since(started_at_ms), Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub(crate) struct NetworkProfileMetrics {
    outbound_profile: String,
    resolver_cache_hit_total: AtomicU64,
    resolver_cache_miss_total: AtomicU64,
    resolver_refresh_total: AtomicU64,
    resolver_refresh_latency_ms_total: AtomicU64,
    resolver_error_total: AtomicU64,
    upstream_pool_refresh: [[PoolRefreshCounters; REASON_COUNT]; PROTOCOL_COUNT],
}

impl NetworkProfileMetrics {
    fn new(outbound_profile: String) -> Self {
        Self {
            outbound_profile,
            resolver_cache_hit_total: AtomicU64::new(0),
            resolver_cache_miss_total: AtomicU64::new(0),
            resolver_refresh_total: AtomicU64::new(0),
            resolver_refresh_latency_ms_total: AtomicU64::new(0),
            resolver_error_total: AtomicU64::new(0),
            upstream_pool_refresh: std::array::from_fn(|_| {
                std::array::from_fn(|_| PoolRefreshCounters::default())
            }),
        }
    }

    #[inline]
    pub(crate) fn outbound_profile(&self) -> &str {
        self.outbound_profile.as_str()
    }

    #[inline]
    pub(crate) fn record_resolver_cache_hit(&self) {
        ensure_registered();
        self.resolver_cache_hit_total
            .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn record_resolver_cache_miss(&self) {
        ensure_registered();
        self.resolver_cache_miss_total
            .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn record_resolver_refresh(&self, started_at_ms: u64) {
        ensure_registered();
        self.resolver_refresh_total.fetch_add(1, Ordering::Relaxed);
        self.resolver_refresh_latency_ms_total
            .fetch_add(elapsed_since(started_at_ms), Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn record_resolver_error(&self) {
        ensure_registered();
        self.resolver_error_total.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn record_upstream_pool_refresh(
        &self,
        protocol: NetworkProtocol,
        reason: PoolRefreshReason,
        started_at_ms: u64,
    ) {
        ensure_registered();
        self.upstream_pool_refresh[protocol.as_index()][reason.as_index()].record(started_at_ms);
    }
}

#[derive(Debug, Default)]
pub(crate) struct NetworkMetrics {
    profiles: Mutex<Vec<Arc<NetworkProfileMetrics>>>,
}

impl NetworkMetrics {
    fn profile_scope(&self, outbound_profile: &str) -> Arc<NetworkProfileMetrics> {
        let outbound_profile = normalized_profile_label(outbound_profile);
        let mut profiles = self
            .profiles
            .lock()
            .expect("network metrics profiles poisoned");
        if let Some(existing) = profiles
            .iter()
            .find(|profile| profile.outbound_profile() == outbound_profile)
        {
            return existing.clone();
        }
        let metrics = Arc::new(NetworkProfileMetrics::new(outbound_profile));
        profiles.push(metrics.clone());
        metrics
    }
}

impl MetricSource for NetworkMetrics {
    fn tag(&self) -> &str {
        NETWORK_METRICS_TAG
    }

    fn plugin_type(&self) -> &'static str {
        "network"
    }

    fn collect(&self, sink: &mut dyn MetricSink) {
        let profiles = self
            .profiles
            .lock()
            .expect("network metrics profiles poisoned")
            .clone();
        for profile in profiles {
            let resolver_labels = [MetricLabel::new(
                "outbound_profile",
                profile.outbound_profile(),
            )];
            sink.emit(MetricSample::counter(
                "network_resolver_cache_hit_total",
                "Total resolver cache hits.",
                &resolver_labels,
                profile.resolver_cache_hit_total.load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "network_resolver_cache_miss_total",
                "Total resolver cache misses that triggered refresh work.",
                &resolver_labels,
                profile.resolver_cache_miss_total.load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "network_resolver_refresh_total",
                "Total resolver refresh attempts.",
                &resolver_labels,
                profile.resolver_refresh_total.load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "network_resolver_refresh_latency_ms_total",
                "Total resolver refresh latency in milliseconds.",
                &resolver_labels,
                profile
                    .resolver_refresh_latency_ms_total
                    .load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "network_resolver_error_total",
                "Total resolver refresh errors.",
                &resolver_labels,
                profile.resolver_error_total.load(Ordering::Relaxed),
            ));

            for protocol in NetworkProtocol::ALL {
                for reason in PoolRefreshReason::ALL {
                    let counters =
                        &profile.upstream_pool_refresh[protocol.as_index()][reason.as_index()];
                    let labels = [
                        MetricLabel::new("outbound_profile", profile.outbound_profile()),
                        MetricLabel::new("protocol", protocol.as_str()),
                        MetricLabel::new("reason", reason.as_str()),
                    ];
                    sink.emit(MetricSample::counter(
                        "network_upstream_pool_refresh_total",
                        "Total bootstrap upstream pool refreshes.",
                        &labels,
                        counters.total.load(Ordering::Relaxed),
                    ));
                    sink.emit(MetricSample::counter(
                        "network_upstream_pool_refresh_latency_ms_total",
                        "Total bootstrap upstream pool refresh latency in milliseconds.",
                        &labels,
                        counters.latency_ms_total.load(Ordering::Relaxed),
                    ));
                }
            }
        }
    }
}

pub(crate) fn init() -> Result<()> {
    register_metric_source(network_metrics().clone())?;
    NETWORK_METRICS_REGISTERED.store(true, Ordering::Release);
    Ok(())
}

#[inline]
pub(crate) fn profile_scope(outbound_profile: &str) -> Arc<NetworkProfileMetrics> {
    ensure_registered();
    network_metrics().profile_scope(outbound_profile)
}

#[inline]
pub(crate) fn resolver_cache_hit(profile: &NetworkProfileMetrics) {
    profile.record_resolver_cache_hit();
}

#[inline]
pub(crate) fn resolver_cache_miss(profile: &NetworkProfileMetrics) {
    profile.record_resolver_cache_miss();
}

#[inline]
pub(crate) fn resolver_refresh(profile: &NetworkProfileMetrics, started_at_ms: u64) {
    profile.record_resolver_refresh(started_at_ms);
}

#[inline]
pub(crate) fn resolver_error(profile: &NetworkProfileMetrics) {
    profile.record_resolver_error();
}

#[inline]
pub(crate) fn upstream_pool_refresh(
    profile: &NetworkProfileMetrics,
    protocol: NetworkProtocol,
    reason: PoolRefreshReason,
    started_at_ms: u64,
) {
    profile.record_upstream_pool_refresh(protocol, reason, started_at_ms);
}

#[inline]
fn network_metrics() -> &'static Arc<NetworkMetrics> {
    NETWORK_METRICS.get_or_init(|| Arc::new(NetworkMetrics::default()))
}

#[inline]
fn ensure_registered() {
    if NETWORK_METRICS_REGISTERED.load(Ordering::Acquire) {
        return;
    }
    let _ = init();
}

#[inline]
fn elapsed_since(started_at_ms: u64) -> u64 {
    AppClock::elapsed_millis().saturating_sub(started_at_ms)
}

fn normalized_profile_label(outbound_profile: &str) -> String {
    let outbound_profile = outbound_profile.trim();
    if outbound_profile.is_empty() {
        OUTBOUND_PROFILE_SYSTEM.to_string()
    } else {
        outbound_profile.to_string()
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct NetworkMetricsSnapshot {
    pub(crate) resolver_cache_hit_total: u64,
    pub(crate) resolver_cache_miss_total: u64,
    pub(crate) resolver_refresh_total: u64,
    pub(crate) resolver_error_total: u64,
    upstream_pool_refresh_total: [[u64; REASON_COUNT]; PROTOCOL_COUNT],
}

#[cfg(test)]
impl NetworkMetricsSnapshot {
    pub(crate) fn upstream_pool_refresh_total(
        &self,
        protocol: NetworkProtocol,
        reason: PoolRefreshReason,
    ) -> u64 {
        self.upstream_pool_refresh_total[protocol.as_index()][reason.as_index()]
    }
}

#[cfg(test)]
pub(crate) fn snapshot_for_profile_for_tests(outbound_profile: &str) -> NetworkMetricsSnapshot {
    let profile = profile_scope(outbound_profile);
    NetworkMetricsSnapshot {
        resolver_cache_hit_total: profile.resolver_cache_hit_total.load(Ordering::Relaxed),
        resolver_cache_miss_total: profile.resolver_cache_miss_total.load(Ordering::Relaxed),
        resolver_refresh_total: profile.resolver_refresh_total.load(Ordering::Relaxed),
        resolver_error_total: profile.resolver_error_total.load(Ordering::Relaxed),
        upstream_pool_refresh_total: std::array::from_fn(|protocol| {
            std::array::from_fn(|reason| {
                profile.upstream_pool_refresh[protocol][reason]
                    .total
                    .load(Ordering::Relaxed)
            })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::observability::metrics::{
        metrics_test_guard, render_prometheus_metrics, reset_metrics_for_tests,
    };

    #[test]
    fn network_metrics_render_prometheus_names_and_labels() {
        let _guard = metrics_test_guard();
        AppClock::start();
        reset_metrics_for_tests();
        init().expect("network metrics should register");

        let profile = profile_scope("remote");
        resolver_cache_hit(&profile);
        resolver_cache_miss(&profile);
        let started_at_ms = AppClock::elapsed_millis();
        resolver_refresh(&profile, started_at_ms);
        resolver_error(&profile);
        upstream_pool_refresh(
            &profile,
            NetworkProtocol::Udp,
            PoolRefreshReason::Init,
            started_at_ms,
        );

        let output = render_prometheus_metrics();
        assert!(output.contains("network_resolver_cache_hit_total"));
        assert!(output.contains("network_resolver_cache_miss_total"));
        assert!(output.contains("network_resolver_refresh_total"));
        assert!(output.contains("network_resolver_error_total"));
        assert!(output.contains("network_upstream_pool_refresh_total"));
        assert!(output.contains("outbound_profile=\"remote\""));
        assert!(output.contains("outbound_profile=\"remote\",protocol=\"udp\",reason=\"init\""));
    }
}
