// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::atomic::{AtomicU64, Ordering};

use crate::infra::clock::AppClock;
use crate::infra::network::upstream::ConnectionInfo;
use crate::infra::observability::metrics::{MetricLabel, MetricSample, MetricSink, MetricSource};

/// Per-upstream forward counters.
///
/// One entry per configured upstream, index-aligned with the forwarder's
/// upstream list. The `upstream` label value is the upstream tag when set, else
/// its resolved address; both are startup-fixed and bounded by the config, so
/// this stays within the generic metrics layer's low-cardinality contract.
#[derive(Debug)]
struct UpstreamMetrics {
    name: String,
    query_total: AtomicU64,
    success_total: AtomicU64,
    error_total: AtomicU64,
    timeout_total: AtomicU64,
    latency_count: AtomicU64,
    latency_sum_ms: AtomicU64,
}

impl UpstreamMetrics {
    fn new(name: String) -> Self {
        Self {
            name,
            query_total: AtomicU64::new(0),
            success_total: AtomicU64::new(0),
            error_total: AtomicU64::new(0),
            timeout_total: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
            latency_sum_ms: AtomicU64::new(0),
        }
    }

    #[inline]
    fn record_latency(&self, start_ms: u64) {
        let elapsed = AppClock::elapsed_millis().saturating_sub(start_ms);
        self.latency_count.fetch_add(1, Ordering::Relaxed);
        self.latency_sum_ms.fetch_add(elapsed, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub(super) struct ForwardMetrics {
    pub(super) tag: String,
    pub(super) query_total: AtomicU64,
    pub(super) success_total: AtomicU64,
    pub(super) error_total: AtomicU64,
    pub(super) timeout_total: AtomicU64,
    pub(super) latency_count: AtomicU64,
    latency_sum_ms: AtomicU64,
    upstreams: Vec<UpstreamMetrics>,
}

impl ForwardMetrics {
    pub(super) fn new(tag: String, upstream_names: Vec<String>) -> Self {
        Self {
            tag,
            query_total: AtomicU64::new(0),
            success_total: AtomicU64::new(0),
            error_total: AtomicU64::new(0),
            timeout_total: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
            latency_sum_ms: AtomicU64::new(0),
            upstreams: upstream_names
                .into_iter()
                .map(UpstreamMetrics::new)
                .collect(),
        }
    }

    #[inline]
    pub(super) fn record_query_start(&self) -> u64 {
        self.query_total.fetch_add(1, Ordering::Relaxed);
        AppClock::elapsed_millis()
    }

    #[inline]
    pub(super) fn record_success(&self, start_ms: u64) {
        self.success_total.fetch_add(1, Ordering::Relaxed);
        self.record_latency(start_ms);
    }

    #[inline]
    pub(super) fn record_error(&self, start_ms: u64, timeout: bool) {
        self.error_total.fetch_add(1, Ordering::Relaxed);
        if timeout {
            self.timeout_total.fetch_add(1, Ordering::Relaxed);
        }
        self.record_latency(start_ms);
    }

    #[inline]
    fn record_latency(&self, start_ms: u64) {
        let elapsed = AppClock::elapsed_millis().saturating_sub(start_ms);
        self.latency_count.fetch_add(1, Ordering::Relaxed);
        self.latency_sum_ms.fetch_add(elapsed, Ordering::Relaxed);
    }

    #[inline]
    pub(super) fn record_upstream_start(&self, idx: usize) -> u64 {
        if let Some(up) = self.upstreams.get(idx) {
            up.query_total.fetch_add(1, Ordering::Relaxed);
        }
        AppClock::elapsed_millis()
    }

    #[inline]
    pub(super) fn record_upstream_success(&self, idx: usize, start_ms: u64) {
        if let Some(up) = self.upstreams.get(idx) {
            up.success_total.fetch_add(1, Ordering::Relaxed);
            up.record_latency(start_ms);
        }
    }

    #[inline]
    pub(super) fn record_upstream_error(&self, idx: usize, start_ms: u64, timeout: bool) {
        if let Some(up) = self.upstreams.get(idx) {
            up.error_total.fetch_add(1, Ordering::Relaxed);
            if timeout {
                up.timeout_total.fetch_add(1, Ordering::Relaxed);
            }
            up.record_latency(start_ms);
        }
    }
}

impl MetricSource for ForwardMetrics {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn plugin_type(&self) -> &'static str {
        "forward"
    }

    fn collect(&self, sink: &mut dyn MetricSink) {
        let labels = [MetricLabel::new("plugin_tag", self.tag.as_str())];
        sink.emit(MetricSample::counter(
            "forward_query_total",
            "Total forward executor queries.",
            &labels,
            self.query_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "forward_success_total",
            "Total successful forward queries.",
            &labels,
            self.success_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "forward_error_total",
            "Total failed forward queries.",
            &labels,
            self.error_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "forward_timeout_total",
            "Total forward queries that timed out.",
            &labels,
            self.timeout_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "forward_latency_count",
            "Total forward queries included in latency statistics.",
            &labels,
            self.latency_count.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "forward_latency_sum_ms",
            "Total forward query latency in milliseconds.",
            &labels,
            self.latency_sum_ms.load(Ordering::Relaxed),
        ));

        for up in &self.upstreams {
            let labels = [
                MetricLabel::new("plugin_tag", self.tag.as_str()),
                MetricLabel::new("upstream", up.name.as_str()),
            ];
            sink.emit(MetricSample::counter(
                "forward_upstream_query_total",
                "Total queries attempted against this upstream.",
                &labels,
                up.query_total.load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "forward_upstream_success_total",
                "Total successful responses from this upstream.",
                &labels,
                up.success_total.load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "forward_upstream_error_total",
                "Total failed attempts against this upstream.",
                &labels,
                up.error_total.load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "forward_upstream_timeout_total",
                "Total attempts against this upstream that timed out.",
                &labels,
                up.timeout_total.load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "forward_upstream_latency_count",
                "Total attempts against this upstream included in latency statistics.",
                &labels,
                up.latency_count.load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "forward_upstream_latency_sum_ms",
                "Total per-upstream attempt latency in milliseconds.",
                &labels,
                up.latency_sum_ms.load(Ordering::Relaxed),
            ));
        }
    }
}

/// Resolve a stable, collision-free label value for each upstream.
///
/// Uses the upstream tag when configured, otherwise its configured address. Any
/// duplicate identity is disambiguated with a `#<index>` suffix so emitted
/// time series never share an identical label set.
pub(super) fn upstream_metric_names(infos: &[&ConnectionInfo]) -> Vec<String> {
    let mut names = Vec::with_capacity(infos.len());
    for (idx, info) in infos.iter().enumerate() {
        let base = info.tag.clone().unwrap_or_else(|| info.raw_addr.clone());
        let name = if names.iter().any(|existing| existing == &base) {
            format!("{}#{}", base, idx)
        } else {
            base
        };
        names.push(name);
    }
    names
}
