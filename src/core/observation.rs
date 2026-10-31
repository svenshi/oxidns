// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded, opt-in request observations. No I/O is performed on the DNS path.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use serde::{Deserialize, Serialize};

const MAX_EVENTS: usize = 128;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObservationEvent {
    pub id: usize,
    pub parent_id: Option<usize>,
    pub kind: String,
    pub plugin: String,
    pub target: String,
    pub method: String,
    pub status: String,
    pub started_ms: f64,
    pub elapsed_ms: Option<f64>,
    pub latency_ms: Option<u64>,
    pub sampled_at_ms: Option<u64>,
    pub selected: bool,
    pub reason: Option<String>,
    pub rcode: Option<String>,
    pub answer_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryObservation {
    pub version: u8,
    pub id: String,
    #[serde(default)]
    pub record_id: Option<i64>,
    pub created_at_ms: i64,
    pub client_ip: String,
    pub qname: String,
    pub qtype: String,
    pub status: String,
    pub elapsed_ms: f64,
    pub rcode: Option<String>,
    pub answers: Vec<String>,
    pub events: Vec<ObservationEvent>,
    pub truncated: bool,
}

#[derive(Debug)]
pub struct Observation {
    started: Instant,
    created_at_ms: i64,
    data: Mutex<QueryObservation>,
}

impl Observation {
    pub fn new(created_at_ms: i64, client_ip: String, qname: String, qtype: String) -> Arc<Self> {
        Arc::new(Self {
            started: Instant::now(),
            created_at_ms,
            data: Mutex::new(QueryObservation {
                version: 1,
                id: format!(
                    "{}-{}-{:016x}",
                    created_at_ms,
                    std::process::id(),
                    NEXT_ID.fetch_add(1, Ordering::Relaxed)
                ),
                record_id: None,
                created_at_ms,
                client_ip,
                qname,
                qtype,
                status: "running".into(),
                elapsed_ms: 0.0,
                rcode: None,
                answers: Vec::new(),
                events: Vec::new(),
                truncated: false,
            }),
        })
    }

    fn created_at_ms(&self) -> i64 {
        self.created_at_ms
    }

    pub fn snapshot(&self) -> QueryObservation {
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if data.status == "running" {
            data.elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        }
        data
    }

    pub fn span(
        self: &Arc<Self>,
        parent_id: Option<usize>,
        kind: &str,
        plugin: &str,
        target: &str,
        method: &str,
    ) -> Option<ObservationSpan> {
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        if data.status != "running" {
            return None;
        }
        if data.events.len() >= MAX_EVENTS {
            data.truncated = true;
            return None;
        }
        let id = data.events.len();
        data.events.push(ObservationEvent {
            id,
            parent_id,
            kind: kind.into(),
            plugin: plugin.into(),
            target: target.into(),
            method: method.into(),
            status: "not_started".into(),
            started_ms: self.started.elapsed().as_secs_f64() * 1000.0,
            elapsed_ms: None,
            latency_ms: None,
            sampled_at_ms: None,
            selected: false,
            reason: None,
            rcode: None,
            answer_count: None,
        });
        Some(ObservationSpan {
            trace: self.clone(),
            id,
        })
    }

    pub fn update(&self, id: usize, update: impl FnOnce(&mut ObservationEvent)) {
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        if data.status == "running"
            && let Some(event) = data.events.get_mut(id)
        {
            update(event);
        }
    }

    pub fn finish(&self, status: &str, rcode: Option<String>, answers: Vec<String>) {
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        if data.status != "running" {
            return;
        }
        data.elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        let elapsed = data.elapsed_ms;
        for event in &mut data.events {
            if event.status == "running" {
                event.status = "cancelled".into();
                event.elapsed_ms = Some((elapsed - event.started_ms).max(0.0));
            }
        }
        data.status = status.into();
        data.rcode = rcode;
        data.answers = answers;
    }
}

#[derive(Debug)]
pub struct ObservationSpan {
    trace: Arc<Observation>,
    pub id: usize,
}

impl ObservationSpan {
    pub fn start(&self) {
        self.trace.update(self.id, |event| {
            event.status = "running".into();
            event.started_ms = self.trace.started.elapsed().as_secs_f64() * 1000.0;
        });
    }

    pub fn finish(&self, status: &str, reason: Option<String>) {
        self.trace.update(self.id, |event| {
            event.status = status.into();
            event.elapsed_ms = Some(
                (self.trace.started.elapsed().as_secs_f64() * 1000.0 - event.started_ms).max(0.0),
            );
            event.reason = reason;
        });
    }

    pub fn probe(&self, status: &str, latency_ms: Option<u64>, sampled_at_ms: Option<u64>) {
        self.finish(status, None);
        self.trace.update(self.id, |event| {
            event.latency_ms = latency_ms;
            event.sampled_at_ms = sampled_at_ms;
        });
    }

    pub fn select(&self, reason: &str) {
        self.trace.update(self.id, |event| {
            event.selected = true;
            event.reason = Some(reason.into());
        });
    }

    pub fn response(&self, response: &crate::proto::Message) {
        self.trace.update(self.id, |event| {
            event.rcode = Some(response.rcode().to_string());
            event.answer_count = Some(response.answers().len());
        });
    }
}

impl Drop for ObservationSpan {
    fn drop(&mut self) {
        self.trace.update(self.id, |event| {
            if event.status == "running" {
                event.status = "cancelled".into();
                event.elapsed_ms = Some(
                    (self.trace.started.elapsed().as_secs_f64() * 1000.0 - event.started_ms)
                        .max(0.0),
                );
            }
        });
    }
}

/// Only weak references are retained; cancellation cannot leak active requests.
#[derive(Debug)]
pub struct ObservationStore {
    limit: usize,
    active: Mutex<HashMap<String, Weak<Observation>>>,
    dropped: AtomicU64,
}

impl ObservationStore {
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.clamp(1, 4096),
            active: Mutex::new(HashMap::new()),
            dropped: AtomicU64::new(0),
        }
    }

    pub fn insert(&self, trace: &Arc<Observation>) -> bool {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        active.retain(|_, value| value.strong_count() > 0);
        if active.len() >= self.limit {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        active.insert(trace.snapshot().id, Arc::downgrade(trace));
        true
    }

    pub fn snapshots(&self, limit: usize) -> Vec<QueryObservation> {
        let mut traces: Vec<_> = self
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter_map(Weak::upgrade)
            .collect();
        traces.sort_by_key(|trace| trace.created_at_ms());
        traces
            .iter()
            .rev()
            .take(limit)
            .map(|trace| trace.snapshot())
            .collect()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace() -> Arc<Observation> {
        Observation::new(0, "127.0.0.1".into(), "example.com.".into(), "A".into())
    }

    #[test]
    fn distinguishes_not_started_cancelled_and_selected() {
        let trace = trace();
        let waiting = trace.span(None, "upstream", "forward", "a", "udp").unwrap();
        let cancelled = trace.span(None, "upstream", "forward", "b", "udp").unwrap();
        cancelled.start();
        drop(cancelled);
        let winner = trace.span(None, "upstream", "forward", "c", "tls").unwrap();
        winner.start();
        winner.finish("success", None);
        winner.select("fastest");
        trace.finish("success", Some("NOERROR".into()), vec![]);
        let result = trace.snapshot();
        assert_eq!(result.events[waiting.id].status, "not_started");
        assert_eq!(result.events[1].status, "cancelled");
        assert!(result.events[2].selected);
        assert!(result.events[2].elapsed_ms.is_some());
        winner.finish("error", None);
        assert_eq!(trace.snapshot(), result);
    }

    #[test]
    fn bounds_events_and_active_requests_and_releases_cancelled_queries() {
        let store = ObservationStore::new(1);
        let first = trace();
        assert!(store.insert(&first));
        assert!(!store.insert(&trace()));
        for _ in 0..MAX_EVENTS + 1 {
            first.span(None, "probe", "selector", "127.0.0.1", "tcp:443");
        }
        assert_eq!(first.snapshot().events.len(), MAX_EVENTS);
        assert!(first.snapshot().truncated);
        drop(first);
        assert!(store.snapshots(100).is_empty());
        assert!(store.insert(&trace()));
        assert_eq!(store.dropped(), 1);
    }
}
