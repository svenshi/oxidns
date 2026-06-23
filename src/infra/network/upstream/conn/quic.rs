// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

use async_trait::async_trait;
use tokio::select;
use tokio::sync::Notify;
use tracing::{debug, trace, warn};

use super::{UsingCountGuard, quic_idle_timeout};
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::dial::{
    DialTarget, QuicDialOptions, SocketOptions, UdpDialOptions, connect_quic, connect_udp,
};
use crate::infra::network::transport::quic::QuicTransport;
use crate::infra::network::upstream::pool::{ConnectionBuilder, QueryDeadline};
use crate::infra::network::upstream::{Connection, ConnectionInfo};
use crate::proto::Message;

pub struct QuicConnection {
    id: u16,
    transport: QuicTransport,
    using_count: AtomicU16,
    closed: AtomicBool,
    last_used: AtomicU64,
    close_notify: Notify,
}

impl Debug for QuicConnection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("QuicConnection")
    }
}

#[async_trait]
impl Connection for QuicConnection {
    /// Gracefully close the QUIC connection
    ///
    /// Sends QUIC CONNECTION_CLOSE frame to peer and notifies background tasks.
    /// This is idempotent - multiple calls are safe.
    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return; // Already closed
        }
        debug!(
            conn_id = self.id,
            "Closing QUIC connection, sending CONNECTION_CLOSE frame"
        );
        // Gracefully close the underlying QUIC connection with error code 0 (no error)
        self.transport.close(b"closing");
        self.close_notify.notify_waiters();
    }

    /// Send a DNS query over QUIC (DoQ - DNS over QUIC, RFC 9250)
    ///
    /// # Arguments
    /// * `request` - DNS query message to send
    ///
    /// # Returns
    /// - `Ok(DnsResponse)` if response received within timeout
    /// - `Err(DnsError)` if connection closed, stream open fails, or timeout
    ///   occurs
    ///
    /// # Protocol
    /// Each DNS query uses a new bidirectional QUIC stream:
    /// - 2-byte big-endian length prefix
    /// - DNS message body
    /// - Stream is closed after message sent/received
    ///
    /// This follows RFC 9250 (DNS over Dedicated QUIC Connections)
    async fn query(&self, request: Message, _deadline: QueryDeadline) -> Result<Message> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DnsError::protocol("Cannot query on closed QUIC connection"));
        }
        self.using_count.fetch_add(1, Ordering::Relaxed);
        // Guard ensures using_count is decremented even if this future is
        // cancelled by an outer timeout (cancel-safety).
        let _guard = UsingCountGuard(&self.using_count);
        if self.closed.load(Ordering::Acquire) {
            return Err(DnsError::protocol("Cannot query on closed QUIC connection"));
        }

        // Open a new bidirectional stream (reader/writer) via connection wrapper
        let (mut reader, mut writer) = match self.transport.open_bi().await {
            Ok((reader, writer)) => (reader, writer),
            Err(e) => {
                self.close();
                return Err(DnsError::protocol(format!(
                    "Failed to open QUIC bidirectional stream: {}",
                    e
                )));
            }
        };

        let raw_id = request.id();
        if let Err(e) = writer.write_message(&request).await {
            self.close();
            return Err(DnsError::protocol(format!(
                "Failed to write DNS query to QUIC stream: {}",
                e
            )));
        }
        if let Err(e) = writer.finish() {
            self.close();
            warn!(
                conn_id = self.id,
                error = ?e,
                "Failed to finish QUIC send stream (half-close)"
            );
            return Err(DnsError::protocol(format!(
                "Failed to finish QUIC send stream: {}",
                e
            )));
        }

        match reader.read_message().await {
            Ok(mut resp) => {
                resp.set_id(raw_id);
                self.last_used
                    .store(AppClock::elapsed_millis(), Ordering::Relaxed);
                trace!(
                    conn_id = self.id,
                    query_id = raw_id,
                    "Successfully received DNS response over QUIC"
                );
                Ok(resp)
            }
            Err(e) => {
                self.close();
                warn!(
                    conn_id = self.id,
                    query_id = raw_id,
                    error = ?e,
                    "Failed to read DNS response from QUIC stream"
                );
                Err(DnsError::protocol(format!(
                    "Failed to read QUIC DNS response: {}",
                    e
                )))
            }
        }
    }

    fn using_count(&self) -> u16 {
        self.using_count.load(Ordering::Relaxed)
    }

    fn available(&self) -> bool {
        !self.closed.load(Ordering::Acquire)
    }

    fn last_used(&self) -> u64 {
        self.last_used.load(Ordering::Relaxed)
    }
}

/// Builder
#[derive(Debug)]
pub struct QuicConnectionBuilder {
    target: DialTarget,
    socket_options: SocketOptions,
    insecure_skip_verify: bool,
    timeout: std::time::Duration,
}

impl QuicConnectionBuilder {
    pub fn new(connection_info: &ConnectionInfo) -> Self {
        Self {
            target: DialTarget::new(
                connection_info.remote_ip,
                connection_info.server_name.clone(),
                connection_info.port,
            ),
            socket_options: SocketOptions::new(
                connection_info.so_mark,
                connection_info.bind_to_device.clone(),
            ),
            insecure_skip_verify: connection_info.insecure_skip_verify,
            timeout: connection_info.timeout,
        }
    }
}

#[async_trait]
impl ConnectionBuilder<QuicConnection> for QuicConnectionBuilder {
    /// Establish a new QUIC connection for DNS over QUIC (DoQ)
    ///
    /// # Returns
    /// Arc-wrapped QuicConnection with background monitoring task spawned
    ///
    /// # Protocol
    /// - Uses QUIC with TLS 1.3 (per RFC 9250)
    /// - Each DNS query uses a new bidirectional stream
    /// - Connection can be reused for multiple queries
    ///
    /// # Performance
    /// - 0-RTT support for resumed connections
    /// - Multiplexed streams avoid head-of-line blocking
    /// - Native congestion control and loss recovery
    async fn create_connection(
        &self,
        conn_id: u16,
        deadline: QueryDeadline,
    ) -> Result<Arc<QuicConnection>> {
        let socket = connect_udp(UdpDialOptions::new(
            self.target.clone(),
            self.socket_options.clone(),
        ))?;

        // Establish QUIC connection (includes TLS 1.3 handshake)
        let quic_conn = connect_quic(
            socket,
            QuicDialOptions::new(
                self.target.clone(),
                self.insecure_skip_verify,
                deadline
                    .remaining()
                    .ok_or_else(|| deadline.timeout_error())?,
                quic_idle_timeout(self.timeout),
                vec![b"doq".to_vec()],
            ),
        )
        .await?;

        debug!(
            conn_id,
            server_name = %self.target.host(),
            remote_addr = ?quic_conn.remote_address(),
            "Established QUIC connection for DoQ (DNS over QUIC)"
        );

        let quic_conn = Arc::new(QuicConnection {
            id: conn_id,
            transport: QuicTransport::new(quic_conn),
            closed: AtomicBool::new(false),
            last_used: AtomicU64::new(AppClock::elapsed_millis()),
            using_count: AtomicU16::new(0),
            close_notify: Notify::new(),
        });

        // Spawn background task to monitor connection health
        let _conn = quic_conn.clone();
        tokio::spawn(async move {
            select! {
                _ = _conn.transport.closed() => {
                    // Mark the QuicConnection as unavailable so the pool removes
                    // it on the next query or maintenance cycle instead of
                    // continuing to try open_bi() on a dead transport.
                    _conn.close();
                    debug!(
                        conn_id,
                        "QUIC connection closed by remote peer or network error"
                    );
                }
                _ = _conn.close_notify.notified() => {
                    debug!(
                        conn_id,
                        "QUIC connection closed by local request"
                    );
                }
            }
            // Ensure the underlying QUIC connection is properly closed
            let _ = _conn.transport.close(b"driver task ending");
        });

        Ok(quic_conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::network::upstream::ConnectionType;

    #[test]
    fn test_builder_new_copies_quic_connection_fields() {
        let mut connection_info = ConnectionInfo::with_addr("quic://dns.example.com")
            .expect("connection info should parse");
        connection_info.timeout = std::time::Duration::from_secs(3);
        connection_info.insecure_skip_verify = true;
        connection_info.so_mark = Some(9);
        connection_info.bind_to_device = Some("wg0".to_string());

        let builder = QuicConnectionBuilder::new(&connection_info);

        assert_eq!(connection_info.connection_type, ConnectionType::DoQ);
        assert_eq!(builder.target.port(), 853);
        assert_eq!(builder.target.host(), "dns.example.com");
        assert!(builder.insecure_skip_verify);
        assert_eq!(builder.timeout, std::time::Duration::from_secs(3));
        assert_eq!(builder.socket_options.so_mark(), Some(9));
        assert_eq!(builder.socket_options.bind_to_device(), Some("wg0"));
    }
}
