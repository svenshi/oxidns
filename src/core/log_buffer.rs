// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Always-on in-memory log buffer + tracing-subscriber layer.
//!
//! [`LogBuffer`] keeps the most recent log entries in a bounded ring and
//! broadcasts every new entry to subscribers. The [`LogLayer`] is the
//! `tracing-subscriber` `Layer` that feeds it.
//!
//! This module is kept in `core` so logging setup (`src/app/logging.rs`)
//! does not depend on the `api` feature. The HTTP adapter in
//! [`crate::api::logs`] adds the REST + SSE endpoints around the public API
//! declared here.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use jiff::Zoned;
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::core::app_clock::AppClock;

pub const RING_CAP: usize = 1000;

static GLOBAL_LOG_BUFFER: OnceLock<Arc<LogBuffer>> = OnceLock::new();

pub fn install_global_log_buffer(buffer: Arc<LogBuffer>) {
    let _ = GLOBAL_LOG_BUFFER.set(buffer);
}

pub fn global_log_buffer() -> Option<Arc<LogBuffer>> {
    GLOBAL_LOG_BUFFER.get().cloned()
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: String,
    pub elapsed_ms: u64,
    pub level: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug)]
pub struct LogBuffer {
    ring: StdMutex<VecDeque<Arc<LogEntry>>>,
    broadcaster: broadcast::Sender<Arc<LogEntry>>,
    next_id: AtomicU64,
}

impl LogBuffer {
    pub fn new() -> Arc<Self> {
        let (sender, _) = broadcast::channel(512);
        Arc::new(Self {
            ring: StdMutex::new(VecDeque::with_capacity(RING_CAP)),
            broadcaster: sender,
            next_id: AtomicU64::new(1),
        })
    }

    pub fn push(&self, mut entry: LogEntry) {
        entry.id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let entry = Arc::new(entry);
        if let Ok(mut ring) = self.ring.lock() {
            if ring.len() >= RING_CAP {
                ring.pop_front();
            }
            ring.push_back(entry.clone());
        }
        let _ = self.broadcaster.send(entry);
    }

    /// Return the most recent `limit` entries, optionally filtered by minimum
    /// level. The filter is inclusive: `Info` returns Error/Warn/Info.
    pub fn recent_filtered(
        &self,
        limit: usize,
        min_level: Option<LevelFilter>,
    ) -> Vec<Arc<LogEntry>> {
        let Ok(ring) = self.ring.lock() else {
            return vec![];
        };
        let mut entries: Vec<Arc<LogEntry>> = ring
            .iter()
            .filter(|e| min_level.is_none_or(|f| level_passes(&e.level, f)))
            .cloned()
            .collect();
        if entries.len() > limit {
            entries.drain(..entries.len() - limit);
        }
        entries
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<LogEntry>> {
        self.broadcaster.subscribe()
    }
}

// --- Level filtering ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelFilter {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LevelFilter {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" | "warning" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warn => 1,
            Self::Info => 2,
            Self::Debug => 3,
            Self::Trace => 4,
        }
    }
}

pub fn level_passes(level_str: &str, min: LevelFilter) -> bool {
    let entry_priority = match level_str {
        "ERROR" => 0u8,
        "WARN" => 1,
        "INFO" => 2,
        "DEBUG" => 3,
        "TRACE" => 4,
        _ => 5,
    };
    entry_priority <= min.priority()
}

// --- Tracing subscriber layer ---

pub struct LogLayer {
    buffer: Arc<LogBuffer>,
}

impl LogLayer {
    pub fn new(buffer: Arc<LogBuffer>) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for LogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level = metadata.level().to_string();
        let target = metadata.target().to_string();
        let elapsed_ms = AppClock::elapsed_millis();
        let timestamp = format!("{}", Zoned::now().strftime("%Y-%m-%dT%H:%M:%S%.3f%:z"));

        let mut visitor = FieldCollector::default();
        event.record(&mut visitor);

        self.buffer.push(LogEntry {
            id: 0,
            timestamp,
            elapsed_ms,
            level,
            target,
            message: visitor.finish(),
        });
    }
}

#[derive(Default)]
struct FieldCollector {
    message: Option<String>,
    extras: Vec<String>,
}

impl FieldCollector {
    fn finish(self) -> String {
        let mut s = self.message.unwrap_or_default();
        for extra in self.extras {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&extra);
        }
        s
    }
}

impl Visit for FieldCollector {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.extras.push(format!("{}={}", field.name(), value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.extras.push(format!("{}={}", field.name(), value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.extras.push(format!("{}={}", field.name(), value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.extras.push(format!("{}={}", field.name(), value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.extras.push(format!("{}={}", field.name(), value));
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.extras.push(format!("{}={}", field.name(), value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else {
            self.extras.push(format!("{}={:?}", field.name(), value));
        }
    }
}
