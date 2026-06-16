// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bootstrap DNS resolver for domain name resolution
//!
//! Provides efficient hostname-to-IP resolution for upstream servers.
//! Implements a lock-free caching mechanism with automatic refresh.
//!
//! # Performance Optimizations
//! - Lock-free state machine using atomic operations
//! - Cached results with TTL-based expiration
//! - Single resolver instance for multiple concurrent queries
//! - Pre-parsed DNS queries to avoid repeated allocations

use std::collections::HashMap;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, Ordering};

use rand::random;
use tokio::sync::{Notify, RwLock};
use tracing::{debug, error, info, warn};

use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::upstream::pool::{DeadlineOutcome, QueryDeadline};
use crate::infra::network::upstream::{ConnectionInfo, Upstream, UpstreamBuilder};
use crate::proto::{DNSClass, Message, MessageType, Name, Opcode, Question, Record, RecordType};

// State machine constants for atomic state transitions
const STATE_NONE: u8 = 0; // Initial state, needs query
const STATE_QUERYING: u8 = 1; // Currently performing DNS lookup
const STATE_CACHED: u8 = 2; // Valid cached result available
const STATE_FAILED: u8 = 3; // Previous query failed

/// Cached DNS resolution result
#[derive(Clone, Debug)]
struct CacheData {
    /// Resolved IP address
    ip: IpAddr,
    /// Expiration time in milliseconds since app start
    expires_at: u64,
}

/// Bootstrap DNS resolver for upstream hostname resolution
///
/// Uses a lock-free state machine to coordinate multiple concurrent
/// resolution requests efficiently. Only one query is performed at a time,
/// with other requests waiting for the result.
#[derive(Debug)]
pub(crate) struct Bootstrap {
    /// Upstream resolver for DNS queries
    upstream: Box<dyn Upstream>,

    /// Atomic state flag for lock-free fast path
    state: AtomicU8,

    /// Cached resolution data with TTL
    cache: RwLock<Option<CacheData>>,

    /// Notifier for query completion (wakes waiting tasks)
    query_done: Notify,

    /// Pre-built DNS query message (optimization)
    message: Message,

    /// Canonical query owner accepted in bootstrap answers.
    query_name: Name,

    /// Domain name being resolved (for logging only)
    domain: String,
}

impl Bootstrap {
    /// Create a new bootstrap resolver
    ///
    /// # Arguments
    /// * `bootstrap_server` - DNS server address for resolution (e.g.,
    ///   "8.8.8.8:53")
    /// * `domain` - Domain name to resolve (FQDN format)
    /// * `ip_version` - IP version preference: Some(6) for IPv6, None or
    ///   Some(4) for IPv4
    ///
    /// # Performance
    /// Pre-builds the DNS query message to avoid repeated allocations on each
    /// query
    pub fn new(bootstrap_server: &str, domain: &str, ip_version: Option<u8>) -> Result<Self> {
        // Pre-parse domain name (fail-fast strategy during initialization)
        let parsed_name = Name::from_str(domain).map_err(|e| {
            DnsError::plugin(format!(
                "invalid bootstrap target domain '{}': {}",
                domain, e
            ))
        })?;

        // Pre-build DNS query message to optimize hot path performance
        // This message template will be cloned for each actual query
        let mut message = Message::new();
        message.set_message_type(MessageType::Query);
        message.set_opcode(Opcode::Query);
        message.set_recursion_desired(true);
        // Set query type based on IP version: AAAA for IPv6, A for IPv4
        message.add_question(Question::new(
            parsed_name.clone(),
            match ip_version {
                Some(6) => RecordType::AAAA,
                _ => RecordType::A,
            },
            DNSClass::IN,
        ));

        let bootstrap_info = ConnectionInfo::with_addr(bootstrap_server).map_err(|e| {
            DnsError::plugin(format!(
                "invalid bootstrap upstream '{}': {}",
                bootstrap_server, e
            ))
        })?;
        if bootstrap_info.remote_ip.is_none() {
            return Err(DnsError::plugin(format!(
                "bootstrap upstream '{}' must use a literal IP address",
                bootstrap_server
            )));
        }

        Ok(Bootstrap {
            upstream: UpstreamBuilder::with_connection_info(bootstrap_info)?,
            state: AtomicU8::new(STATE_NONE),
            cache: RwLock::new(None),
            query_done: Notify::new(),
            message,
            query_name: parsed_name,
            domain: domain.to_string(),
        })
    }

    /// Get the resolved IP address, using cache or triggering a new query
    ///
    /// This is the hot path - optimized for minimal overhead when cache is
    /// valid. Uses a lock-free state machine for coordination among
    /// multiple concurrent callers.
    ///
    /// # Returns
    /// - `Ok(IpAddr)` if resolution succeeds (from cache or fresh query)
    /// - `Err(DnsError)` if all resolution attempts fail after retries
    ///
    /// # Performance
    /// - Fast path: single atomic load when cache is valid
    /// - Only one concurrent query at a time (others wait for result)
    /// - Automatic retry on transient failures
    #[inline]
    pub async fn get_with_deadline(&self, deadline: QueryDeadline) -> Result<IpAddr> {
        let mut failed_count = 0;

        loop {
            if deadline.remaining().is_none() {
                return Err(deadline.timeout_error());
            }

            // Fast path: atomic load without locking (most common case)
            let state = self.state.load(Ordering::Acquire);

            match state {
                STATE_CACHED => {
                    // Hot path: check cache validity
                    let cache = self.cache.read().await;
                    if let Some(ref data) = *cache
                        && AppClock::elapsed_millis() < data.expires_at
                    {
                        // Cache hit - most common case
                        return Ok(data.ip);
                    }
                    drop(cache);

                    // Cache expired, trigger refresh
                    debug!(
                        domain = %self.domain,
                        "Bootstrap cache expired, triggering refresh"
                    );
                    if self
                        .state
                        .compare_exchange(
                            STATE_CACHED,
                            STATE_NONE,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        // Successfully transitioned to NONE, loop to trigger query
                        continue;
                    }
                    // Someone else is already refreshing, wait for result
                    self.wait_query_done(deadline).await?;
                }
                STATE_NONE => {
                    // Try to acquire query permission
                    if self
                        .state
                        .compare_exchange(
                            STATE_NONE,
                            STATE_QUERYING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        // We won the race, perform the query
                        let mut query_guard = BootstrapQueryGuard::new(self);
                        self.query(deadline).await;
                        query_guard.disarm();
                        continue;
                    }
                    // Someone else is querying, wait for result
                    self.wait_query_done(deadline).await?;
                }
                STATE_QUERYING => {
                    // Wait for query to complete
                    self.wait_query_done(deadline).await?;
                }
                STATE_FAILED => {
                    // Limit retry attempts to prevent infinite loops
                    if failed_count > 3 {
                        return Err(DnsError::protocol(format!(
                            "Bootstrap DNS resolution failed for '{}' after {} attempts",
                            self.domain, failed_count
                        )));
                    }
                    failed_count += 1;

                    // Retry by transitioning back to NONE state
                    if self
                        .state
                        .compare_exchange(
                            STATE_FAILED,
                            STATE_NONE,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        continue;
                    }
                    self.wait_query_done(deadline).await?;
                }
                _ => unreachable!("Invalid bootstrap state"),
            }
        }
    }

    async fn wait_query_done(&self, deadline: QueryDeadline) -> Result<()> {
        let notified = self.query_done.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.state.load(Ordering::Acquire) != STATE_QUERYING {
            return Ok(());
        }
        match deadline.run(notified.as_mut()).await {
            DeadlineOutcome::Completed(()) => Ok(()),
            DeadlineOutcome::Expired => Err(deadline.timeout_error()),
        }
    }

    /// Perform DNS query for the domain
    ///
    /// Uses pre-built query message for efficiency.
    /// Updates cache and notifies waiting tasks on completion.
    ///
    /// # State Transitions
    /// - Success: STATE_QUERYING -> STATE_CACHED
    /// - Failure: STATE_QUERYING -> STATE_FAILED
    ///
    /// # Concurrency
    /// This method is called by only one task at a time (enforced by state
    /// machine). Other tasks wait via `query_done` notification.
    async fn query(&self, deadline: QueryDeadline) {
        // Execute DNS query using pre-built message template
        // Randomize query ID to prevent response spoofing
        let mut message = self.message.clone();
        message.set_id(random());
        match self.upstream.query_with_deadline(message, deadline).await {
            Ok(response) => {
                let answers = response.answers();

                if let Some((ip, ttl_seconds, record_type)) =
                    select_bootstrap_answer(answers, &self.query_name)
                {
                    info!(
                        domain = %self.domain,
                        ip = %ip,
                        ttl_seconds,
                        record_type = ?record_type,
                        "Bootstrap DNS resolution successful"
                    );

                    // Update cache with new IP and expiration time
                    let ttl = ttl_seconds as u64 * 1000;
                    let expires_at = AppClock::elapsed_millis().saturating_add(ttl);
                    *self.cache.write().await = Some(CacheData { ip, expires_at });

                    // Transition to CACHED state and wake all waiting tasks
                    self.state.store(STATE_CACHED, Ordering::Release);
                    self.query_done.notify_waiters();
                    return;
                }

                // No matching A/AAAA records found in response
                warn!(
                    domain = %self.domain,
                    answer_count = answers.len(),
                    "No A/AAAA records found in bootstrap DNS response"
                );
                self.state.store(STATE_FAILED, Ordering::Release);
                self.query_done.notify_waiters();
            }
            Err(e) => {
                // DNS query failed (network error, timeout, etc.)
                error!(
                    domain = %self.domain,
                    error = %e,
                    "Bootstrap DNS query failed"
                );
                self.state.store(STATE_FAILED, Ordering::Release);
                self.query_done.notify_waiters();
            }
        }
    }
}

fn select_bootstrap_answer(
    answers: &[Record],
    query_name: &Name,
) -> Option<(IpAddr, u32, RecordType)> {
    let mut accepted_names = HashMap::new();
    accepted_names.insert(query_name.clone(), u32::MAX);

    loop {
        let mut changed = false;
        for answer in answers {
            let Some(target) = answer.cname_target() else {
                continue;
            };
            let Some(owner_ttl) = accepted_names.get(answer.name()).copied() else {
                continue;
            };
            let ttl = owner_ttl.min(answer.ttl());
            match accepted_names.get(target).copied() {
                Some(existing_ttl) if existing_ttl <= ttl => {}
                _ => {
                    accepted_names.insert(target.clone(), ttl);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    for answer in answers {
        if !matches!(answer.rr_type(), RecordType::A | RecordType::AAAA) {
            continue;
        }
        let Some(owner_ttl) = accepted_names.get(answer.name()).copied() else {
            continue;
        };
        let Some(ip) = answer.ip_addr() else {
            continue;
        };
        return Some((ip, owner_ttl.min(answer.ttl()), answer.rr_type()));
    }

    None
}

struct BootstrapQueryGuard<'a> {
    bootstrap: &'a Bootstrap,
    armed: bool,
}

impl<'a> BootstrapQueryGuard<'a> {
    fn new(bootstrap: &'a Bootstrap) -> Self {
        Self {
            bootstrap,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for BootstrapQueryGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.bootstrap.state.store(STATE_FAILED, Ordering::Release);
            self.bootstrap.query_done.notify_waiters();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::net::Ipv4Addr;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::sync::oneshot;

    use super::*;
    use crate::proto::RData;
    use crate::proto::rdata::{A, CNAME};

    #[derive(Debug)]
    struct BlockingUpstream {
        started: Mutex<Option<oneshot::Sender<()>>>,
        connection_info: ConnectionInfo,
    }

    #[async_trait]
    impl Upstream for BlockingUpstream {
        async fn inner_query(
            &self,
            _request: Message,
            _deadline: QueryDeadline,
        ) -> Result<Message> {
            if let Some(started) = self.started.lock().expect("started lock poisoned").take() {
                let _ = started.send(());
            }
            pending::<Result<Message>>().await
        }

        fn connection_info(&self) -> &ConnectionInfo {
            &self.connection_info
        }
    }

    #[tokio::test]
    async fn test_new_builds_ipv4_query_by_default() {
        let bootstrap = Bootstrap::new("1.1.1.1:53", "example.com.", None)
            .expect("bootstrap should be created");

        let query = bootstrap
            .message
            .first_question()
            .expect("question should be pre-built");

        assert_eq!(bootstrap.domain, "example.com.");
        assert_eq!(query.qtype(), RecordType::A);
        assert_eq!(query.name().to_fqdn(), "example.com.");
    }

    #[tokio::test]
    async fn test_new_builds_ipv6_query_when_requested() {
        let bootstrap = Bootstrap::new("8.8.8.8:53", "example.com.", Some(6))
            .expect("bootstrap should be created");

        let query = bootstrap
            .message
            .first_question()
            .expect("question should be pre-built");

        assert_eq!(query.qtype(), RecordType::AAAA);
    }

    #[tokio::test]
    async fn test_new_rejects_invalid_target_domain() {
        let result = Bootstrap::new("1.1.1.1:53", "example..com.", None);

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_new_rejects_invalid_bootstrap_server() {
        let result = Bootstrap::new("udp://127.0.0.1:notaport", "example.com.", None);

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_new_rejects_hostname_bootstrap_server() {
        let result = Bootstrap::new("udp://resolver.example.invalid:53", "example.com.", None);

        assert!(
            result
                .expect_err("hostname bootstrap server should be rejected")
                .to_string()
                .contains("must use a literal IP address")
        );
    }

    #[tokio::test]
    async fn test_canceled_bootstrap_query_releases_querying_state() {
        AppClock::start();
        let (started_tx, started_rx) = oneshot::channel();
        let bootstrap = Arc::new(Bootstrap {
            upstream: Box::new(BlockingUpstream {
                started: Mutex::new(Some(started_tx)),
                connection_info: ConnectionInfo::with_addr("udp://127.0.0.1:53")
                    .expect("connection info should parse"),
            }),
            state: AtomicU8::new(STATE_NONE),
            cache: RwLock::new(None),
            query_done: Notify::new(),
            message: Message::new(),
            query_name: Name::from_ascii("example.com.").expect("query name should parse"),
            domain: "example.com.".to_string(),
        });

        let task_bootstrap = bootstrap.clone();
        let handle = tokio::spawn(async move {
            task_bootstrap
                .get_with_deadline(QueryDeadline::new(Duration::from_secs(5)))
                .await
        });

        started_rx.await.expect("bootstrap query should start");
        handle.abort();
        assert!(
            handle
                .await
                .expect_err("bootstrap task should be cancelled")
                .is_cancelled()
        );

        assert_eq!(bootstrap.state.load(Ordering::Acquire), STATE_FAILED);
    }

    #[tokio::test]
    async fn test_wait_query_done_returns_when_state_already_changed() {
        AppClock::start();
        let bootstrap = Bootstrap {
            upstream: Box::new(BlockingUpstream {
                started: Mutex::new(None),
                connection_info: ConnectionInfo::with_addr("udp://127.0.0.1:53")
                    .expect("connection info should parse"),
            }),
            state: AtomicU8::new(STATE_CACHED),
            cache: RwLock::new(None),
            query_done: Notify::new(),
            message: Message::new(),
            query_name: Name::from_ascii("example.com.").expect("query name should parse"),
            domain: "example.com.".to_string(),
        };

        bootstrap
            .wait_query_done(QueryDeadline::new(Duration::from_millis(10)))
            .await
            .expect("changed state should not wait for a fresh notification");
    }

    #[test]
    fn test_select_bootstrap_answer_follows_cname_and_rejects_unrelated_a() {
        let query_name = Name::from_ascii("example.com.").expect("name should parse");
        let alias_name = Name::from_ascii("alias.example.net.").expect("name should parse");
        let unrelated_name = Name::from_ascii("unrelated.example.").expect("name should parse");
        let unrelated = Record::from_rdata(
            unrelated_name,
            300,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 10))),
        );
        let cname = Record::from_rdata(
            query_name.clone(),
            30,
            RData::CNAME(CNAME(alias_name.clone())),
        );
        let target =
            Record::from_rdata(alias_name, 300, RData::A(A(Ipv4Addr::new(203, 0, 113, 53))));

        let selected = select_bootstrap_answer(&[unrelated, cname, target], &query_name)
            .expect("CNAME target should be accepted");

        assert_eq!(selected.0, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 53)));
        assert_eq!(selected.1, 30);
        assert_eq!(selected.2, RecordType::A);
    }

    #[test]
    fn test_select_bootstrap_answer_rejects_unrelated_a_records() {
        let query_name = Name::from_ascii("example.com.").expect("name should parse");
        let unrelated_name = Name::from_ascii("unrelated.example.").expect("name should parse");
        let unrelated = Record::from_rdata(
            unrelated_name,
            300,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 10))),
        );

        assert!(select_bootstrap_answer(&[unrelated], &query_name).is_none());
    }
}
