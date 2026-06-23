// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! TTL cache and singleflight refresh for resolver lookups.

use std::future::Future;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{Mutex, RwLock};
use tracing::debug;

use super::query::{ResolveQuery, ResolvedAnswer};
use crate::infra::clock::AppClock;
use crate::infra::error::Result;
use crate::infra::network::deadline::{DeadlineOutcome, QueryDeadline};
use crate::proto::{Message, Name};

#[derive(Clone, Debug)]
pub(super) struct CachedIp {
    pub(super) ip: IpAddr,
    pub(super) expires_at: u64,
}

impl CachedIp {
    fn is_valid(&self) -> bool {
        AppClock::elapsed_millis() < self.expires_at
    }
}

#[derive(Debug)]
pub(super) struct ResolveEntry {
    pub(super) domain: String,
    query: ResolveQuery,
    pub(super) cache: RwLock<Option<CachedIp>>,
    refresh: Mutex<()>,
    expires_at_hint: AtomicU64,
    last_accessed_at: AtomicU64,
}

impl ResolveEntry {
    pub(super) fn new(domain: String, ip_version: Option<u8>) -> Result<Self> {
        let now = AppClock::elapsed_millis();
        Ok(Self {
            query: ResolveQuery::new(domain.as_str(), ip_version)?,
            domain,
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
    ) -> Result<IpAddr>
    where
        F: FnOnce(Message, Name, QueryDeadline) -> Fut,
        Fut: Future<Output = Result<ResolvedAnswer>>,
    {
        if let Some(ip) = self.cached_ip().await {
            return Ok(ip);
        }

        let _guard = match deadline.run(self.refresh.lock()).await {
            DeadlineOutcome::Completed(guard) => guard,
            DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
        };

        if let Some(ip) = self.cached_ip().await {
            return Ok(ip);
        }

        debug!(
            domain = %self.domain,
            "Resolver cache miss or expired, refreshing"
        );

        let answer = refresh(
            self.query.message_template(),
            self.query.query_name(),
            deadline,
        )
        .await?;
        self.store(answer).await;
        Ok(answer.ip)
    }

    async fn cached_ip(&self) -> Option<IpAddr> {
        let cache = self.cache.read().await;
        cache
            .as_ref()
            .and_then(|cached| cached.is_valid().then_some(cached.ip))
    }

    async fn store(&self, answer: ResolvedAnswer) {
        let ttl = answer.ttl_seconds as u64 * 1000;
        let expires_at = AppClock::elapsed_millis().saturating_add(ttl);
        self.expires_at_hint.store(expires_at, Ordering::Relaxed);
        *self.cache.write().await = Some(CachedIp {
            ip: answer.ip,
            expires_at,
        });
    }
}
