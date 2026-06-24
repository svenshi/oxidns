// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! TTL cache and singleflight refresh for resolver lookups.

use std::future::Future;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{Mutex, RwLock};
use tracing::debug;

use super::query::{ResolveQuery, ResolvedAnswer};
use crate::infra::clock::AppClock;
use crate::infra::error::Result;
use crate::infra::network::deadline::{DeadlineOutcome, QueryDeadline};
use crate::infra::network::metrics::{self as network_metrics, NetworkProfileMetrics};
use crate::proto::{Message, Name};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedIp {
    pub(crate) ip: IpAddr,
    pub(crate) expires_at_ms: u64,
}

impl ResolvedIp {
    fn is_valid(&self) -> bool {
        AppClock::elapsed_millis() < self.expires_at_ms
    }
}

#[derive(Debug)]
pub(super) struct ResolveEntry {
    pub(super) domain: String,
    query: ResolveQuery,
    metrics: Arc<NetworkProfileMetrics>,
    pub(super) cache: RwLock<Option<ResolvedIp>>,
    refresh: Mutex<()>,
    expires_at_hint: AtomicU64,
    last_accessed_at: AtomicU64,
}

impl ResolveEntry {
    pub(super) fn new(
        domain: String,
        ip_version: Option<u8>,
        metrics: Arc<NetworkProfileMetrics>,
    ) -> Result<Self> {
        let now = AppClock::elapsed_millis();
        Ok(Self {
            query: ResolveQuery::new(domain.as_str(), ip_version)?,
            domain,
            metrics,
            cache: RwLock::new(None),
            refresh: Mutex::new(()),
            expires_at_hint: AtomicU64::new(0),
            last_accessed_at: AtomicU64::new(now),
        })
    }

    pub(super) fn touch(&self) {
        self.last_accessed_at
            .store(AppClock::elapsed_millis(), Ordering::Relaxed);
    }

    pub(super) fn is_expired_hint(&self) -> bool {
        let expires_at = self.expires_at_hint.load(Ordering::Relaxed);
        expires_at == 0 || AppClock::elapsed_millis() >= expires_at
    }

    pub(super) fn last_accessed_at(&self) -> u64 {
        self.last_accessed_at.load(Ordering::Relaxed)
    }

    pub(super) async fn resolve_with<F, Fut>(
        &self,
        deadline: QueryDeadline,
        refresh: F,
    ) -> Result<ResolvedIp>
    where
        F: FnOnce(Message, Name, QueryDeadline) -> Fut,
        Fut: Future<Output = Result<ResolvedAnswer>>,
    {
        if let Some(resolved) = self.cached_ip().await {
            network_metrics::resolver_cache_hit(&self.metrics);
            return Ok(resolved);
        }

        let _guard = match deadline.run(self.refresh.lock()).await {
            DeadlineOutcome::Completed(guard) => guard,
            DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
        };

        if let Some(resolved) = self.cached_ip().await {
            network_metrics::resolver_cache_hit(&self.metrics);
            return Ok(resolved);
        }

        network_metrics::resolver_cache_miss(&self.metrics);
        debug!(
            domain = %self.domain,
            "Resolver cache miss or expired, refreshing"
        );

        let refresh_started_at_ms = AppClock::elapsed_millis();
        let answer = match refresh(
            self.query.message_template(),
            self.query.query_name(),
            deadline,
        )
        .await
        {
            Ok(answer) => {
                network_metrics::resolver_refresh(&self.metrics, refresh_started_at_ms);
                answer
            }
            Err(err) => {
                network_metrics::resolver_refresh(&self.metrics, refresh_started_at_ms);
                network_metrics::resolver_error(&self.metrics);
                return Err(err);
            }
        };
        Ok(self.store(answer).await)
    }

    async fn cached_ip(&self) -> Option<ResolvedIp> {
        let cache = self.cache.read().await;
        cache
            .as_ref()
            .and_then(|cached| cached.is_valid().then_some(*cached))
    }

    async fn store(&self, answer: ResolvedAnswer) -> ResolvedIp {
        let ttl = answer.ttl_seconds as u64 * 1000;
        let expires_at_ms = AppClock::elapsed_millis().saturating_add(ttl);
        self.expires_at_hint.store(expires_at_ms, Ordering::Relaxed);
        let resolved = ResolvedIp {
            ip: answer.ip,
            expires_at_ms,
        };
        *self.cache.write().await = Some(resolved);
        resolved
    }
}
