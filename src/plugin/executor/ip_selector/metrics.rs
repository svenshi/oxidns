// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Metrics for the `ip_selector` executor.
//!
//! Metrics are intentionally separated from policy/probe code so counters stay
//! easy to audit. Probe completion can happen from request tasks or background
//! warmup tasks, so all counters are atomic and owned by this shared source.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use ahash::{AHashMap, AHashSet};

use super::config::ProbeMethod;
use super::policy::SelectionSource;
use super::probe::{ProbeCache, ProbeObservation};
use crate::core::metrics::{MetricLabel, MetricSample, MetricSink, MetricSource};

/// Per-method counters exposed through the OxiDNS metrics registry.
#[derive(Debug)]
struct MethodMetrics {
    method: ProbeMethod,
    method_label: String,
    success_total: AtomicU64,
    failure_total: AtomicU64,
    latency_count: AtomicU64,
    latency_sum_ms: AtomicU64,
}

impl MethodMetrics {
    fn new(method: ProbeMethod) -> Self {
        Self {
            method,
            method_label: method.to_string(),
            success_total: AtomicU64::new(0),
            failure_total: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
            latency_sum_ms: AtomicU64::new(0),
        }
    }
}

/// Metric source for one `ip_selector` instance.
#[derive(Debug)]
pub(super) struct IpSelectorMetrics {
    tag: String,
    cache: OnceLock<ProbeCache>,
    methods: Vec<MethodMetrics>,
    method_index: AHashMap<ProbeMethod, usize>,
    selected_probe_total: AtomicU64,
    selected_cache_total: AtomicU64,
    selected_fallback_total: AtomicU64,
    dropped_parallel_limit_total: AtomicU64,
    dropped_inflight_total: AtomicU64,
}

impl IpSelectorMetrics {
    pub(super) fn new(tag: String, methods: &[ProbeMethod]) -> Self {
        // Metrics are emitted only for active methods. `none` is passive and has
        // no probe attempts or latency samples of its own.
        let mut unique = Vec::new();
        let mut seen = AHashSet::new();
        for method in methods.iter().copied().filter(|method| method.is_active()) {
            if seen.insert(method) {
                unique.push(MethodMetrics::new(method));
            }
        }
        let method_index = unique
            .iter()
            .enumerate()
            .map(|(idx, item)| (item.method, idx))
            .collect();

        Self {
            tag,
            cache: OnceLock::new(),
            methods: unique,
            method_index,
            selected_probe_total: AtomicU64::new(0),
            selected_cache_total: AtomicU64::new(0),
            selected_fallback_total: AtomicU64::new(0),
            dropped_parallel_limit_total: AtomicU64::new(0),
            dropped_inflight_total: AtomicU64::new(0),
        }
    }

    pub(super) fn set_cache(&self, cache: ProbeCache) {
        let _ = self.cache.set(cache);
    }

    pub(super) fn record_probe(&self, method: ProbeMethod, observation: ProbeObservation) {
        let Some(idx) = self.method_index.get(&method).copied() else {
            return;
        };
        let metrics = &self.methods[idx];
        if observation.success {
            metrics.success_total.fetch_add(1, Ordering::Relaxed);
            if let Some(latency_ms) = observation.latency_ms {
                metrics.latency_count.fetch_add(1, Ordering::Relaxed);
                metrics
                    .latency_sum_ms
                    .fetch_add(latency_ms, Ordering::Relaxed);
            }
        } else {
            metrics.failure_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn record_selection(&self, source: SelectionSource) {
        match source {
            SelectionSource::Probe => {
                self.selected_probe_total.fetch_add(1, Ordering::Relaxed);
            }
            SelectionSource::Cache => {
                self.selected_cache_total.fetch_add(1, Ordering::Relaxed);
            }
            SelectionSource::Fallback => {
                self.selected_fallback_total.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub(super) fn record_dropped_parallel_limit(&self) {
        self.dropped_parallel_limit_total
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_dropped_inflight(&self) {
        self.dropped_inflight_total.fetch_add(1, Ordering::Relaxed);
    }
}

impl MetricSource for IpSelectorMetrics {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn plugin_type(&self) -> &'static str {
        "ip_selector"
    }

    fn collect(&self, sink: &mut dyn MetricSink) {
        for method in &self.methods {
            let success_labels = [
                MetricLabel::new("plugin_tag", self.tag.as_str()),
                MetricLabel::new("method", method.method_label.as_str()),
                MetricLabel::new("result", "success"),
            ];
            sink.emit(MetricSample::counter(
                "ip_selector_probe_total",
                "Total ip_selector probes by method and result.",
                &success_labels,
                method.success_total.load(Ordering::Relaxed),
            ));

            let failure_labels = [
                MetricLabel::new("plugin_tag", self.tag.as_str()),
                MetricLabel::new("method", method.method_label.as_str()),
                MetricLabel::new("result", "failure"),
            ];
            sink.emit(MetricSample::counter(
                "ip_selector_probe_total",
                "Total ip_selector probes by method and result.",
                &failure_labels,
                method.failure_total.load(Ordering::Relaxed),
            ));

            let method_labels = [
                MetricLabel::new("plugin_tag", self.tag.as_str()),
                MetricLabel::new("method", method.method_label.as_str()),
            ];
            sink.emit(MetricSample::counter(
                "ip_selector_probe_latency_count",
                "Total successful ip_selector probe latency samples.",
                &method_labels,
                method.latency_count.load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "ip_selector_probe_latency_sum_ms",
                "Sum of successful ip_selector probe latencies in milliseconds.",
                &method_labels,
                method.latency_sum_ms.load(Ordering::Relaxed),
            ));
        }

        let probe_selected_labels = [
            MetricLabel::new("plugin_tag", self.tag.as_str()),
            MetricLabel::new("source", "probe"),
        ];
        sink.emit(MetricSample::counter(
            "ip_selector_selected_total",
            "Total ip_selector response selections by score source.",
            &probe_selected_labels,
            self.selected_probe_total.load(Ordering::Relaxed),
        ));

        let cache_selected_labels = [
            MetricLabel::new("plugin_tag", self.tag.as_str()),
            MetricLabel::new("source", "cache"),
        ];
        sink.emit(MetricSample::counter(
            "ip_selector_selected_total",
            "Total ip_selector response selections by score source.",
            &cache_selected_labels,
            self.selected_cache_total.load(Ordering::Relaxed),
        ));

        let fallback_selected_labels = [
            MetricLabel::new("plugin_tag", self.tag.as_str()),
            MetricLabel::new("source", "fallback"),
        ];
        sink.emit(MetricSample::counter(
            "ip_selector_selected_total",
            "Total ip_selector response selections by score source.",
            &fallback_selected_labels,
            self.selected_fallback_total.load(Ordering::Relaxed),
        ));

        let cache_labels = [MetricLabel::new("plugin_tag", self.tag.as_str())];
        sink.emit(MetricSample::gauge(
            "ip_selector_cache_entries",
            "Current number of ip_selector probe cache entries.",
            &cache_labels,
            self.cache
                .get()
                .map(|cache| cache.len() as u64)
                .unwrap_or(0),
        ));

        let parallel_labels = [
            MetricLabel::new("plugin_tag", self.tag.as_str()),
            MetricLabel::new("reason", "parallel_limit"),
        ];
        sink.emit(MetricSample::counter(
            "ip_selector_dropped_probe_total",
            "Total ip_selector probe attempts dropped or coalesced before active probing.",
            &parallel_labels,
            self.dropped_parallel_limit_total.load(Ordering::Relaxed),
        ));

        let inflight_labels = [
            MetricLabel::new("plugin_tag", self.tag.as_str()),
            MetricLabel::new("reason", "inflight"),
        ];
        sink.emit(MetricSample::counter(
            "ip_selector_dropped_probe_total",
            "Total ip_selector probe attempts dropped or coalesced before active probing.",
            &inflight_labels,
            self.dropped_inflight_total.load(Ordering::Relaxed),
        ));
    }
}
