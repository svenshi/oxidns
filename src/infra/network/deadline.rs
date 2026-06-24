// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared network operation deadlines.

use std::future::Future;
use std::time::Duration;

use crate::infra::clock::AppClock;
use crate::infra::error::DnsError;

/// Outcome of running a future under a query deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineOutcome<T> {
    Completed(T),
    Expired,
}

/// Per-query network deadline measured by the process-wide application clock.
///
/// The deadline is intentionally based on `AppClock::elapsed_millis()` so
/// related network paths can share one monotonic budget across bootstrap,
/// pool acquisition, connection expansion, handshakes, and DNS I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryDeadline {
    pub started_at_ms: u64,
    pub expires_at_ms: u64,
}

impl QueryDeadline {
    pub fn new(timeout: Duration) -> Self {
        let started_at_ms = AppClock::elapsed_millis();
        let timeout_ms = duration_millis_u64(timeout);
        Self {
            started_at_ms,
            expires_at_ms: started_at_ms.saturating_add(timeout_ms),
        }
    }

    pub fn remaining(&self) -> Option<Duration> {
        let now = AppClock::elapsed_millis();
        if now >= self.expires_at_ms {
            None
        } else {
            Some(Duration::from_millis(self.expires_at_ms - now))
        }
    }

    pub async fn run<F, T>(&self, fut: F) -> DeadlineOutcome<T>
    where
        F: Future<Output = T>,
    {
        let Some(remaining) = self.remaining() else {
            return DeadlineOutcome::Expired;
        };

        match tokio::time::timeout(remaining, fut).await {
            Ok(value) => DeadlineOutcome::Completed(value),
            Err(_) => DeadlineOutcome::Expired,
        }
    }

    pub fn timeout_error(&self) -> DnsError {
        DnsError::plugin(format!(
            "DNS query timeout after {:?}",
            Duration::from_millis(self.expires_at_ms.saturating_sub(self.started_at_ms))
        ))
    }
}

fn duration_millis_u64(duration: Duration) -> u64 {
    if duration.is_zero() {
        return 0;
    }
    duration.as_millis().try_into().unwrap_or(u64::MAX).max(1)
}
