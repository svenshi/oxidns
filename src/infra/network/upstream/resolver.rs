// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::Debug;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::metrics::{self as network_metrics, NetworkProtocol, PoolRefreshReason};
use crate::infra::network::resolver::{NameResolver, ResolvedIp};
use crate::infra::network::upstream::builder::{
    main_pool_min_conns, pipeline_request_map_capacity, reuse_request_map_capacity,
};
use crate::infra::network::upstream::config::{ConnectionInfo, ConnectionType};
#[cfg(feature = "upstream-doh")]
use crate::infra::network::upstream::conn::{H2Connection, H2ConnectionBuilder};
#[cfg(feature = "upstream-doh3")]
use crate::infra::network::upstream::conn::{H3Connection, H3ConnectionBuilder};
#[cfg(feature = "upstream-doq")]
use crate::infra::network::upstream::conn::{QuicConnection, QuicConnectionBuilder};
use crate::infra::network::upstream::conn::{
    TcpConnection, TcpConnectionBuilder, UdpConnection, UdpConnectionBuilder,
};
use crate::infra::network::upstream::pool::pool_pipeline::PipelinePool;
use crate::infra::network::upstream::pool::pool_reuse::ReusePool;
use crate::infra::network::upstream::pool::{
    Connection, ConnectionBuilder, ConnectionPool, DeadlineOutcome, QueryDeadline,
    QueryTimeoutPolicy,
};
use crate::proto::Message;

#[async_trait]
#[allow(unused)]
pub trait Upstream: Send + Sync + Debug {
    /// **Internal API - Do not call directly!**
    ///
    /// Send a DNS query using the provided end-to-end query deadline.
    ///
    /// # For Implementors
    /// Implement this method to provide the actual DNS query logic.
    ///
    /// # For Callers
    /// **Always use `query()` or `query_with_deadline()` instead!**
    #[doc(hidden)]
    async fn inner_query(&self, request: Message, deadline: QueryDeadline) -> Result<Message>;

    /// Return the connection configuration information
    ///
    /// Provides access to all upstream connection parameters including
    /// connection type, timeout, addresses, and protocol-specific settings.
    fn connection_info(&self) -> &ConnectionInfo;

    /// Return the timeout duration for this upstream
    ///
    /// Default implementation reads from connection_info.
    /// Can be overridden if custom timeout logic is needed.
    #[inline]
    fn timeout(&self) -> Duration {
        self.connection_info().timeout
    }

    /// Return the connection type of this upstream
    ///
    /// Convenience method for accessing connection_info.connection_type.
    #[inline]
    fn connection_type(&self) -> ConnectionType {
        self.connection_info().connection_type
    }

    /// Whether `inner_query` owns deadline enforcement and timeout cleanup.
    ///
    /// Pool-backed implementations must return `true` so the pool can observe
    /// deadline expiry and apply its connection retirement/close policy.
    #[inline]
    fn handles_query_deadline(&self) -> bool {
        false
    }

    /// Send a DNS query with an existing upstream deadline.
    async fn query_with_deadline(
        &self,
        message: Message,
        deadline: QueryDeadline,
    ) -> Result<Message> {
        if deadline.remaining().is_none() {
            warn!(
                timeout_secs = self.timeout().as_secs_f64(),
                "Upstream DNS query timeout"
            );
            return Err(deadline.timeout_error());
        }
        if self.handles_query_deadline() {
            return self.inner_query(message, deadline).await;
        }
        match deadline.run(self.inner_query(message, deadline)).await {
            DeadlineOutcome::Completed(result) => result,
            DeadlineOutcome::Expired => {
                warn!(
                    timeout_secs = self.timeout().as_secs_f64(),
                    "Upstream DNS query timeout"
                );
                Err(deadline.timeout_error())
            }
        }
    }

    /// Send a DNS query with unified deadline handling
    ///
    /// This is the **recommended API** for all DNS queries.
    /// Automatically applies timeout based on `timeout()` configuration.
    ///
    /// # Performance Notes
    /// - Message is moved (not cloned) to avoid allocation overhead
    /// - Timeout error logging uses structured fields for zero-copy
    /// - Only logs on timeout, not on successful queries (hot path
    ///   optimization)
    ///
    /// # Errors
    /// - Returns `DnsError::plugin` on timeout
    /// - Returns upstream-specific errors on query failures
    async fn query(&self, message: Message) -> Result<Message> {
        let deadline = QueryDeadline::new(self.timeout());
        self.query_with_deadline(message, deadline).await
    }
}

/// Pooled upstream resolver implementation
///
/// Uses connection pooling to efficiently reuse connections for multiple
/// queries. The pool type (pipeline or reuse) is determined during creation
/// based on protocol capabilities and configuration.
#[allow(unused)]
#[derive(Debug)]
pub(crate) struct PooledUpstream<C: Connection> {
    /// Connection metadata (remote address, port, etc.)
    pub(crate) connection_info: ConnectionInfo,
    /// Connection pool for load balancing and connection reuse
    pub(crate) pool: Arc<dyn ConnectionPool<C>>,
}

#[async_trait]
impl<C: Connection> Upstream for PooledUpstream<C> {
    /// Execute DNS query through the connection pool
    ///
    /// The pool handles connection selection, creation, and lifecycle
    /// management. No additional logging here as the pool layer already
    /// logs connection events.
    async fn inner_query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        self.pool.query(request, deadline).await
    }

    fn connection_info(&self) -> &ConnectionInfo {
        &self.connection_info
    }

    fn handles_query_deadline(&self) -> bool {
        true
    }
}

/// UDP upstream with automatic TCP fallback on truncation
///
/// DNS over UDP has a 512-byte size limit (or EDNS extended size).
/// When responses exceed this limit, the TC (truncated) bit is set,
/// indicating the client should retry over TCP to get the full response.
///
/// This upstream automatically handles this fallback:
/// 1. Try UDP first (fast, low overhead)
/// 2. If truncated, automatically retry over TCP
#[derive(Debug)]
pub(crate) struct UdpTruncatedUpstream {
    /// Connection configuration (includes timeout)
    pub(crate) connection_info: ConnectionInfo,
    /// Primary UDP connection pool (fast path)
    pub(crate) main_pool: Arc<dyn ConnectionPool<UdpConnection>>,
    /// Fallback TCP connection pool (used when UDP response is truncated)
    pub(crate) fallback_pool: Arc<dyn ConnectionPool<TcpConnection>>,
}

#[async_trait]
impl Upstream for UdpTruncatedUpstream {
    async fn inner_query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        // Try UDP first (most DNS queries fit in UDP packets)
        let response = self.main_pool.query(request.clone(), deadline).await?;

        // Check if response was truncated (TC bit set)
        if response.truncated() {
            // Log fallback event (only happens occasionally, minimal performance impact)
            debug!("UDP response truncated, falling back to TCP");

            // Retry over TCP to get the full response
            self.fallback_pool.query(request, deadline).await
        } else {
            // UDP response was complete, return it
            Ok(response)
        }
    }

    fn connection_info(&self) -> &ConnectionInfo {
        &self.connection_info
    }

    fn handles_query_deadline(&self) -> bool {
        true
    }
}

/// Bootstrap-resolved UDP upstream with automatic TCP fallback on truncation.
#[derive(Debug)]
pub(crate) struct BootstrapUdpTruncatedUpstream {
    connection_info: ConnectionInfo,
    main: BootstrapUpstream<UdpConnection>,
    fallback: BootstrapUpstream<TcpConnection>,
}

impl BootstrapUdpTruncatedUpstream {
    pub(crate) fn new(connection_info: ConnectionInfo) -> Self {
        let mut fallback_info = connection_info.clone();
        fallback_info.connection_type = ConnectionType::TCP;
        Self {
            connection_info: connection_info.clone(),
            main: BootstrapUpstream::udp(connection_info),
            fallback: BootstrapUpstream::tcp(fallback_info),
        }
    }
}

#[async_trait]
impl Upstream for BootstrapUdpTruncatedUpstream {
    async fn inner_query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        let response = self.main.inner_query(request.clone(), deadline).await?;
        if response.truncated() {
            debug!("Bootstrap UDP response truncated, falling back to TCP");
            self.fallback.inner_query(request, deadline).await
        } else {
            Ok(response)
        }
    }

    fn connection_info(&self) -> &ConnectionInfo {
        &self.connection_info
    }

    fn handles_query_deadline(&self) -> bool {
        true
    }
}

trait BootstrapPoolFactory<C: Connection>: Debug + Send + Sync {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<C>>;
}

#[derive(Debug)]
struct UdpBootstrapPoolFactory;

impl BootstrapPoolFactory<UdpConnection> for UdpBootstrapPoolFactory {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<UdpConnection>> {
        let info = connection_info_with_ip(connection_info, ip);
        let builder = UdpConnectionBuilder::new(&info, pipeline_request_map_capacity());
        PipelinePool::new(
            main_pool_min_conns(&info),
            info.max_conns_or_default(),
            ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
            info.idle_timeout,
            Box::new(builder),
            QueryTimeoutPolicy::Reuse,
            info.timeout,
        )
    }
}

#[derive(Debug)]
struct TcpBootstrapPoolFactory;

impl BootstrapPoolFactory<TcpConnection> for TcpBootstrapPoolFactory {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<TcpConnection>> {
        let info = connection_info_with_ip(connection_info, ip);
        if info.enable_pipeline.unwrap_or(false) {
            let builder = TcpConnectionBuilder::new(&info, pipeline_request_map_capacity());
            PipelinePool::new(
                main_pool_min_conns(&info),
                info.max_conns_or_default(),
                ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
                info.idle_timeout,
                Box::new(builder),
                QueryTimeoutPolicy::Retire,
                info.timeout,
            )
        } else {
            let builder = TcpConnectionBuilder::new(&info, reuse_request_map_capacity());
            ReusePool::new(
                main_pool_min_conns(&info),
                info.max_conns_or_default(),
                info.idle_timeout,
                Box::new(builder),
                QueryTimeoutPolicy::Close,
                info.timeout,
            )
        }
    }
}

#[cfg(feature = "upstream-doq")]
#[derive(Debug)]
struct QuicBootstrapPoolFactory;

#[cfg(feature = "upstream-doq")]
impl BootstrapPoolFactory<QuicConnection> for QuicBootstrapPoolFactory {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<QuicConnection>> {
        let info = connection_info_with_ip(connection_info, ip);
        let builder = QuicConnectionBuilder::new(&info);
        PipelinePool::new(
            main_pool_min_conns(&info),
            info.max_conns_or_default(),
            ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
            info.idle_timeout,
            Box::new(builder),
            QueryTimeoutPolicy::Retire,
            info.timeout,
        )
    }
}

#[cfg(feature = "upstream-doh")]
#[derive(Debug)]
struct H2BootstrapPoolFactory;

#[cfg(feature = "upstream-doh")]
impl BootstrapPoolFactory<H2Connection> for H2BootstrapPoolFactory {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<H2Connection>> {
        let info = connection_info_with_ip(connection_info, ip);
        let builder = H2ConnectionBuilder::new(&info);
        PipelinePool::new(
            main_pool_min_conns(&info),
            info.max_conns_or_default(),
            ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
            info.idle_timeout,
            Box::new(builder),
            QueryTimeoutPolicy::Retire,
            info.timeout,
        )
    }
}

#[cfg(feature = "upstream-doh3")]
#[derive(Debug)]
struct H3BootstrapPoolFactory;

#[cfg(feature = "upstream-doh3")]
impl BootstrapPoolFactory<H3Connection> for H3BootstrapPoolFactory {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<H3Connection>> {
        let info = connection_info_with_ip(connection_info, ip);
        let builder = H3ConnectionBuilder::new(&info);
        PipelinePool::new(
            main_pool_min_conns(&info),
            info.max_conns_or_default(),
            ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
            info.idle_timeout,
            Box::new(builder),
            QueryTimeoutPolicy::Retire,
            info.timeout,
        )
    }
}

fn connection_info_with_ip(connection_info: &ConnectionInfo, ip: IpAddr) -> ConnectionInfo {
    let mut info = connection_info.clone();
    info.remote_ip = Some(ip);
    info
}

/// Domain-based upstream resolver that uses bootstrap to resolve domain names
///
/// When the upstream server is specified as a domain name (e.g.,
/// dns.google.com) instead of an IP address, we need to resolve it first. This
/// creates a chicken-and-egg problem: we need DNS to resolve the DNS server's
/// address!
///
/// This upstream solves it by using a bootstrap resolver:
/// 1. Bootstrap resolver (configured with IP) resolves the domain name
/// 2. Resolved IP is cached with TTL
/// 3. Connection pool is created/updated when IP changes
/// 4. DNS queries are forwarded through the pool
///
/// # Performance
/// - Lock-free pool swapping using ArcSwap (no blocking on IP changes)
/// - IP resolution is cached, not done on every query
/// - Automatic pool refresh when TTL expires
#[derive(Debug)]
pub(crate) struct BootstrapUpstream<C: Connection> {
    /// Upstream server domain name (for logging)
    server_name: String,
    /// Connection metadata (includes bootstrap config)
    connection_info: ConnectionInfo,
    /// Bootstrap resolver for domain name resolution
    bootstrap: Arc<NameResolver>,
    /// Lock-free connection pool with current resolved IP and TTL deadline.
    pool: ArcSwap<BootstrapPoolState<C>>,
    /// Type-safe factory for creating protocol-specific pools when IP changes.
    pool_factory: Box<dyn BootstrapPoolFactory<C>>,
    /// Serializes cold-path pool creation after bootstrap refreshes.
    pool_update_lock: Mutex<()>,
}

#[derive(Debug)]
struct BootstrapPoolState<C: Connection> {
    ip: Option<IpAddr>,
    expires_at_ms: u64,
    pool: Arc<dyn ConnectionPool<C>>,
}

impl<C: Connection> BootstrapPoolState<C> {
    fn placeholder(pool: Arc<dyn ConnectionPool<C>>) -> Self {
        Self {
            ip: None,
            expires_at_ms: 0,
            pool,
        }
    }

    fn with_pool(resolved: ResolvedIp, pool: Arc<dyn ConnectionPool<C>>) -> Self {
        Self {
            ip: Some(resolved.ip),
            expires_at_ms: resolved.expires_at_ms,
            pool,
        }
    }

    fn is_valid(&self) -> bool {
        self.ip.is_some() && AppClock::elapsed_millis() < self.expires_at_ms
    }
}

impl<C: Connection> BootstrapUpstream<C> {
    /// Create a new domain upstream with the given connection info and optional
    /// bootstrap server
    fn new(
        connection_info: ConnectionInfo,
        pool_factory: Box<dyn BootstrapPoolFactory<C>>,
    ) -> Self {
        let pool: Arc<dyn ConnectionPool<C>> = ReusePool::<C>::new(
            0,
            1,
            ConnectionInfo::DEFAULT_CONN_IDLE_TIME,
            Box::new(DummyConnectionBuilder {}),
            QueryTimeoutPolicy::Close,
            connection_info.timeout,
        );

        BootstrapUpstream {
            server_name: connection_info.server_name.clone(),
            bootstrap: connection_info.bootstrap.clone().unwrap(),
            connection_info,
            pool: ArcSwap::from_pointee(BootstrapPoolState::placeholder(pool)),
            pool_factory,
            pool_update_lock: Mutex::new(()),
        }
    }

    /// Initialize or refresh the connection pool with the resolved IP
    ///
    /// This method handles:
    /// - Initial pool creation on first query
    /// - IP change detection and pool refresh
    /// - Lock-free pool updates using ArcSwap
    ///
    /// # Performance
    /// - Fast path: cached bootstrap IP + single atomic pool load when IP
    ///   hasn't changed
    /// - Pool recreation only happens on IP change (rare)
    /// - Cold-path pool recreation is serialized to avoid duplicate pool builds
    async fn init_pool_if_needed(&self, deadline: QueryDeadline) -> Result<()> {
        let state = self.pool.load();
        if state.is_valid() {
            return Ok(());
        }
        drop(state);

        let _update_guard = match deadline.run(self.pool_update_lock.lock()).await {
            DeadlineOutcome::Completed(guard) => guard,
            DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
        };
        let state = self.pool.load();
        if state.is_valid() {
            return Ok(());
        }

        let refresh_started_at_ms = AppClock::elapsed_millis();
        let bootstrap_deadline =
            bootstrap_deadline(deadline, self.connection_info.bootstrap_timeout);
        let resolved = match self
            .bootstrap
            .resolve_with_expiry(&self.server_name, bootstrap_deadline)
            .await
        {
            Ok(value) => value,
            Err(value) => return Err(value),
        };
        let protocol = NetworkProtocol::from_connection_info(&self.connection_info);

        if let Some(current_ip) = state.ip
            && current_ip == resolved.ip
        {
            let next_state = BootstrapPoolState::with_pool(resolved, state.pool.clone());
            self.pool.swap(Arc::new(next_state));
            network_metrics::upstream_pool_refresh(
                self.bootstrap.metrics(),
                protocol,
                PoolRefreshReason::TtlOnly,
                refresh_started_at_ms,
            );
            return Ok(());
        }

        let refresh_reason = if let Some(current_ip) = state.ip {
            info!(
                server = %self.server_name,
                old_ip = %current_ip,
                new_ip = %resolved.ip,
                "Upstream IP address changed, refreshing connection pool"
            );
            PoolRefreshReason::IpChanged
        } else {
            info!(
                server = %self.server_name,
                ip = %resolved.ip,
                "Initializing connection pool for domain-based upstream"
            );
            PoolRefreshReason::Init
        };

        let new_pool = self
            .pool_factory
            .create_pool(&self.connection_info, resolved.ip);

        // Atomically swap to new pool (lock-free, readers see old or new pool
        // consistently)
        self.pool
            .swap(Arc::new(BootstrapPoolState::with_pool(resolved, new_pool)));
        network_metrics::upstream_pool_refresh(
            self.bootstrap.metrics(),
            protocol,
            refresh_reason,
            refresh_started_at_ms,
        );

        Ok(())
    }
}

impl BootstrapUpstream<UdpConnection> {
    pub(crate) fn udp(connection_info: ConnectionInfo) -> Self {
        Self::new(connection_info, Box::new(UdpBootstrapPoolFactory))
    }
}

impl BootstrapUpstream<TcpConnection> {
    pub(crate) fn tcp(connection_info: ConnectionInfo) -> Self {
        Self::new(connection_info, Box::new(TcpBootstrapPoolFactory))
    }
}

#[cfg(feature = "upstream-doq")]
impl BootstrapUpstream<QuicConnection> {
    pub(crate) fn doq(connection_info: ConnectionInfo) -> Self {
        Self::new(connection_info, Box::new(QuicBootstrapPoolFactory))
    }
}

#[cfg(feature = "upstream-doh")]
impl BootstrapUpstream<H2Connection> {
    pub(crate) fn doh2(connection_info: ConnectionInfo) -> Self {
        Self::new(connection_info, Box::new(H2BootstrapPoolFactory))
    }
}

#[cfg(feature = "upstream-doh3")]
impl BootstrapUpstream<H3Connection> {
    pub(crate) fn doh3(connection_info: ConnectionInfo) -> Self {
        Self::new(connection_info, Box::new(H3BootstrapPoolFactory))
    }
}

fn bootstrap_deadline(deadline: QueryDeadline, timeout: Option<Duration>) -> QueryDeadline {
    let Some(timeout) = timeout else {
        return deadline;
    };
    let timeout_deadline = QueryDeadline::new(timeout);
    if timeout_deadline.expires_at_ms < deadline.expires_at_ms {
        timeout_deadline
    } else {
        deadline
    }
}

#[async_trait]
impl<C: Connection> Upstream for BootstrapUpstream<C> {
    /// Execute DNS query through bootstrap-resolved upstream
    ///
    /// # Process
    /// 1. Resolve domain name to IP (cached with TTL in bootstrap)
    /// 2. Initialize/refresh pool if IP changed
    /// 3. Forward query through the pool
    ///
    /// # Performance
    /// - Hot path: pool already initialized, just forward query
    /// - Cold path: bootstrap resolution + pool creation (first query only)
    /// - IP change: new pool creation (rare, based on DNS TTL)
    async fn inner_query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        // Ensure connection pool is initialized with current IP
        // Fast path: just checks atomic, no allocation
        // Slow path: resolves DNS + creates pool (only on first query or IP change)
        self.init_pool_if_needed(deadline).await?;

        // Get current connection pool (lock-free atomic load)
        let pool = self.pool.load();

        // Forward query through the pool
        pool.pool.query(request, deadline).await
    }

    fn connection_info(&self) -> &ConnectionInfo {
        &self.connection_info
    }

    fn handles_query_deadline(&self) -> bool {
        true
    }
}

/// Dummy connection builder for initial empty pool
///
/// This is used as a placeholder before the first DNS resolution completes.
/// Any attempt to create a connection will fail with an error.
#[derive(Debug)]
struct DummyConnectionBuilder {}

#[async_trait]
impl<C: Connection> ConnectionBuilder<C> for DummyConnectionBuilder {
    async fn create_connection(&self, _conn_id: u16, _deadline: QueryDeadline) -> Result<Arc<C>> {
        Err(DnsError::protocol(
            "DummyConnectionBuilder cannot create connections (pool not yet initialized)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::net::UdpSocket;

    use super::*;
    use crate::infra::clock::AppClock;
    use crate::infra::network::metrics::{self as network_metrics, OUTBOUND_PROFILE_LOCAL};
    use crate::infra::network::upstream::UpstreamConfig;
    use crate::proto::rdata::A;
    use crate::proto::{MessageType, RData, Rcode, Record};

    async fn spawn_bootstrap_server(answers: Vec<(Ipv4Addr, u32)>) -> (String, Arc<AtomicUsize>) {
        let socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bootstrap socket should bind");
        let addr = socket
            .local_addr()
            .expect("bootstrap socket should have addr");
        let answers = Arc::new(Mutex::new(VecDeque::from(answers)));
        let count = Arc::new(AtomicUsize::new(0));
        let server_count = count.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                let Ok((len, peer)) = socket.recv_from(&mut buf).await else {
                    break;
                };
                let Ok(request) = Message::from_bytes(&buf[..len]) else {
                    continue;
                };
                let Some(question) = request.first_question().cloned() else {
                    continue;
                };
                let answer = answers
                    .lock()
                    .expect("answers lock should not be poisoned")
                    .pop_front()
                    .unwrap_or((Ipv4Addr::new(203, 0, 113, 53), 60));
                server_count.fetch_add(1, Ordering::Relaxed);

                let mut response = Message::new();
                response.set_id(request.id());
                response.set_message_type(MessageType::Response);
                response.set_recursion_desired(request.recursion_desired());
                response.set_recursion_available(true);
                response.set_rcode(Rcode::NoError);
                response.add_question(question.clone());
                response.add_answer(Record::from_rdata(
                    question.name().clone(),
                    answer.1,
                    RData::A(A(answer.0)),
                ));
                let wire = response
                    .to_bytes()
                    .expect("bootstrap response should encode");
                let _ = socket.send_to(&wire, peer).await;
            }
        });

        (addr.to_string(), count)
    }

    fn bootstrap_connection_info(bootstrap: String) -> ConnectionInfo {
        let config = UpstreamConfig {
            tag: None,
            addr: "udp://dns.example.invalid:53".to_string(),
            outbound: None,
            dial_addr: None,
            port: None,
            bootstrap: Some(bootstrap),
            bootstrap_version: Some(4),
            socks5: None,
            idle_timeout: None,
            max_conns: None,
            min_conns: None,
            insecure_skip_verify: None,
            timeout: None,
            enable_pipeline: None,
            enable_http3: None,
            so_mark: None,
            bind_to_device: None,
        };
        ConnectionInfo::try_from(config).expect("bootstrap upstream config should parse")
    }

    fn force_pool_expired(upstream: &BootstrapUpstream<UdpConnection>) {
        let state = upstream.pool.load();
        upstream.pool.swap(Arc::new(BootstrapPoolState {
            ip: state.ip,
            expires_at_ms: 0,
            pool: state.pool.clone(),
        }));
    }

    #[tokio::test]
    async fn bootstrap_udp_truncated_upstream_constructs_typed_main_and_fallback() {
        AppClock::start();
        let upstream =
            BootstrapUdpTruncatedUpstream::new(bootstrap_connection_info("127.0.0.1:53".into()));

        assert_eq!(
            upstream.main.connection_info.connection_type,
            ConnectionType::UDP
        );
        assert_eq!(
            upstream.fallback.connection_info.connection_type,
            ConnectionType::TCP
        );
    }

    #[tokio::test]
    async fn bootstrap_upstream_pool_refresh_metrics_record_reasons() {
        AppClock::start();
        let before = network_metrics::snapshot_for_profile_for_tests(OUTBOUND_PROFILE_LOCAL);
        let (bootstrap, _count) = spawn_bootstrap_server(vec![
            (Ipv4Addr::new(203, 0, 113, 1), 60),
            (Ipv4Addr::new(203, 0, 113, 1), 60),
            (Ipv4Addr::new(203, 0, 113, 2), 60),
        ])
        .await;
        let upstream =
            BootstrapUpstream::<UdpConnection>::udp(bootstrap_connection_info(bootstrap));

        upstream
            .init_pool_if_needed(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("initial pool init should resolve bootstrap");
        force_pool_expired(&upstream);
        upstream.bootstrap.clear_entries_for_test();
        upstream
            .init_pool_if_needed(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("same IP refresh should update TTL");
        force_pool_expired(&upstream);
        upstream.bootstrap.clear_entries_for_test();
        upstream
            .init_pool_if_needed(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("changed IP refresh should rebuild pool");

        let after = network_metrics::snapshot_for_profile_for_tests(OUTBOUND_PROFILE_LOCAL);
        assert!(
            after.upstream_pool_refresh_total(NetworkProtocol::Udp, PoolRefreshReason::Init)
                > before.upstream_pool_refresh_total(NetworkProtocol::Udp, PoolRefreshReason::Init),
            "expected init pool refresh metric to increase: before={before:?}, after={after:?}"
        );
        assert!(
            after.upstream_pool_refresh_total(NetworkProtocol::Udp, PoolRefreshReason::TtlOnly)
                > before
                    .upstream_pool_refresh_total(NetworkProtocol::Udp, PoolRefreshReason::TtlOnly),
            "expected ttl_only pool refresh metric to increase: before={before:?}, after={after:?}"
        );
        assert!(
            after.upstream_pool_refresh_total(NetworkProtocol::Udp, PoolRefreshReason::IpChanged)
                > before.upstream_pool_refresh_total(
                    NetworkProtocol::Udp,
                    PoolRefreshReason::IpChanged
                ),
            "expected ip_changed pool refresh metric to increase: before={before:?}, after={after:?}"
        );
    }

    #[tokio::test]
    async fn bootstrap_upstream_valid_pool_skips_resolver_cache() {
        AppClock::start();
        let (bootstrap, count) =
            spawn_bootstrap_server(vec![(Ipv4Addr::new(203, 0, 113, 1), 60)]).await;
        let upstream =
            BootstrapUpstream::<UdpConnection>::udp(bootstrap_connection_info(bootstrap));

        upstream
            .init_pool_if_needed(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("initial pool init should resolve bootstrap");
        upstream.bootstrap.clear_entries_for_test();
        upstream
            .init_pool_if_needed(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("valid pool state should not need resolver");

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn bootstrap_upstream_expired_same_ip_refreshes_ttl_without_rebuilding_pool() {
        AppClock::start();
        let (bootstrap, count) = spawn_bootstrap_server(vec![
            (Ipv4Addr::new(203, 0, 113, 1), 60),
            (Ipv4Addr::new(203, 0, 113, 1), 60),
        ])
        .await;
        let upstream =
            BootstrapUpstream::<UdpConnection>::udp(bootstrap_connection_info(bootstrap));

        upstream
            .init_pool_if_needed(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("initial pool init should resolve bootstrap");
        let first_pool = upstream.pool.load().pool.clone();
        force_pool_expired(&upstream);
        upstream.bootstrap.clear_entries_for_test();
        upstream
            .init_pool_if_needed(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("expired pool state should refresh bootstrap");
        let second_pool = upstream.pool.load().pool.clone();

        assert_eq!(count.load(Ordering::Relaxed), 2);
        assert!(Arc::ptr_eq(&first_pool, &second_pool));
    }

    #[tokio::test]
    async fn bootstrap_upstream_expired_changed_ip_rebuilds_pool() {
        AppClock::start();
        let (bootstrap, count) = spawn_bootstrap_server(vec![
            (Ipv4Addr::new(203, 0, 113, 1), 60),
            (Ipv4Addr::new(203, 0, 113, 2), 60),
        ])
        .await;
        let upstream =
            BootstrapUpstream::<UdpConnection>::udp(bootstrap_connection_info(bootstrap));

        upstream
            .init_pool_if_needed(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("initial pool init should resolve bootstrap");
        let first_pool = upstream.pool.load().pool.clone();
        force_pool_expired(&upstream);
        upstream.bootstrap.clear_entries_for_test();
        upstream
            .init_pool_if_needed(QueryDeadline::new(Duration::from_secs(1)))
            .await
            .expect("changed IP should rebuild bootstrap pool");
        let second_pool = upstream.pool.load().pool.clone();

        assert_eq!(count.load(Ordering::Relaxed), 2);
        assert!(!Arc::ptr_eq(&first_pool, &second_pool));
        assert_eq!(
            upstream.pool.load().ip,
            Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2)))
        );
    }
}
