// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::Debug;
use std::time::Duration;

use async_trait::async_trait;
use tracing::warn;

use crate::infra::error::Result;
use crate::infra::network::upstream::config::{ConnectionInfo, ConnectionType};
use crate::infra::network::upstream::pool::{DeadlineOutcome, QueryDeadline};
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
