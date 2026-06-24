// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use crate::infra::error::Result;
use crate::infra::network::upstream::config::ConnectionInfo;
use crate::infra::network::upstream::conn::{TcpConnection, UdpConnection};
use crate::infra::network::upstream::pool::{Connection, ConnectionPool, QueryDeadline};
use crate::infra::network::upstream::traits::Upstream;
use crate::proto::Message;

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
