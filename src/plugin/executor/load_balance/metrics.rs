// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::atomic::{AtomicU64, Ordering};

use crate::infra::observability::metrics::{MetricLabel, MetricSample, MetricSink, MetricSource};

#[derive(Debug)]
pub(super) struct TargetMetrics {
    pub executor: String,
    pub selected: AtomicU64,
    pub errors: AtomicU64,
}

#[derive(Debug)]
pub(super) struct LoadBalanceMetrics {
    pub tag: String,
    pub targets: Vec<TargetMetrics>,
}

impl LoadBalanceMetrics {
    pub fn new(tag: String, targets: impl Iterator<Item = String>) -> Self {
        Self {
            tag,
            targets: targets
                .map(|executor| TargetMetrics {
                    executor,
                    selected: AtomicU64::new(0),
                    errors: AtomicU64::new(0),
                })
                .collect(),
        }
    }
}

impl MetricSource for LoadBalanceMetrics {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn plugin_type(&self) -> &'static str {
        "load_balance"
    }

    fn collect(&self, sink: &mut dyn MetricSink) {
        for target in &self.targets {
            let labels = [
                MetricLabel::new("plugin_tag", &self.tag),
                MetricLabel::new("executor", &target.executor),
            ];
            sink.emit(MetricSample::counter(
                "load_balance_selected_total",
                "Total load balance branch selections, including cancelled executions.",
                &labels,
                target.selected.load(Ordering::Relaxed),
            ));
            sink.emit(MetricSample::counter(
                "load_balance_error_total",
                "Total selected branch executions returning an error.",
                &labels,
                target.errors.load(Ordering::Relaxed),
            ));
        }
    }
}
