// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::UdpSocket;
use tokio::select;
use tokio::sync::{Notify, oneshot};
use tokio::time::timeout;
use tracing::{debug, error, trace, warn};

use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::dial::{DialTarget, SocketOptions, UdpDialOptions, connect_udp};
use crate::infra::network::transport::udp::UdpTransport;
use crate::infra::network::upstream::ConnectionInfo;
use crate::infra::network::upstream::conn::request_map::RequestMap;
use crate::infra::network::upstream::pool::{Connection, ConnectionBuilder, QueryDeadline};
use crate::proto::Message;

const UDP_RECV_BUFFER_SIZE: usize = 8_196;

/// Represents a single UDP connection used in DNS upstream queries.
/// Each connection manages its own socket and maintains a mapping
/// of request IDs to response channels for asynchronous query handling.
#[derive(Debug)]
pub struct UdpConnection {
    /// Unique connection ID (for debugging/tracing)
    id: u16,
    /// The underlying UDP transport bound to a local address
    transport: UdpTransport,
    /// Notifier used to signal connection closure
    close_notify: Notify,
    /// Mapping between DNS query IDs and response channels
    request_map: RequestMap,
    /// Timestamp of last activity (milliseconds)
    last_used: AtomicU64,
    /// Connection closed flag (prevents use after closure and ensures
    /// idempotent close)
    closed: AtomicBool,
}

#[cfg(test)]
const DEFAULT_REQUEST_MAP_CAPACITY: u16 = 64;

/// Retry delay for initial DNS query attempts
const RETRY_TIMEOUT: Duration = Duration::from_secs(1);

#[async_trait]
impl Connection for UdpConnection {
    /// Close this UDP connection and notify all waiting tasks
    ///
    /// UDP connections are stateless, so close mainly signals the listener task
    /// to exit. This method is idempotent - multiple calls are safe and
    /// will only execute once.
    fn close(&self) {
        // Atomically set closed flag and check previous value
        if self.closed.swap(true, Ordering::SeqCst) {
            return; // Already closed, no-op
        }
        // Cancel every pending query before waking the listener so the background
        // task can observe an empty request map and drop the socket immediately
        // instead of waiting for per-query timeouts to age out.
        let cleared = self.request_map.clear();
        debug!(
            conn_id = self.id,
            canceled_queries = cleared,
            "Closing UDP connection and signaling listener task"
        );
        self.close_notify.notify_waiters();
    }

    /// Send a DNS query and wait asynchronously for its response
    ///
    /// # Arguments
    /// * `request` - DNS query message to send
    ///
    /// # Returns
    /// - `Ok(DnsResponse)` if response received
    /// - `Err(DnsError)` if both attempts timeout or network error occurs
    ///
    /// # Retry Strategy
    /// - First attempt: 1 second timeout (quick retry on packet loss)
    /// - Second attempt: configured timeout (allows for slower network)
    ///
    /// This two-stage approach improves resilience against UDP packet loss
    /// while maintaining low latency for successful queries.
    async fn query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DnsError::protocol("UDP connection is closed"));
        }

        let raw_id = request.id();

        for attempt in 0..2 {
            let Some(remaining) = deadline.remaining() else {
                return Err(deadline.timeout_error());
            };
            let current_timeout = if attempt == 0 {
                remaining.min(RETRY_TIMEOUT)
            } else {
                remaining
            };

            let (tx, rx) = oneshot::channel();
            let mut query_guard = self.request_map.store(tx)?;
            let query_id = query_guard.query_id();
            if self.closed.load(Ordering::Acquire) {
                return Err(DnsError::protocol("UDP connection is closed"));
            }

            trace!(
                conn_id = self.id,
                attempt,
                query_id,
                timeout_ms = current_timeout.as_millis(),
                "Sending DNS query over UDP"
            );

            // Send UDP datagram via transport
            match self
                .transport
                .write_message_with_id(&request, query_id)
                .await
            {
                Ok(()) => {}
                Err(e) => {
                    error!(conn_id = self.id, err = %e, "Failed to send UDP query");
                    self.close();
                    return Err(e);
                }
            }

            // Wait for response with timeout
            match timeout(current_timeout, rx).await {
                Ok(res) => match res {
                    Ok(mut response) => {
                        query_guard.disarm();
                        response.set_id(raw_id);
                        trace!(conn_id = self.id, query_id, raw_id, "Received UDP response");
                        return Ok(response);
                    }
                    Err(_canceled) => {
                        trace!(
                            conn_id = self.id,
                            query_id, "Listener dropped channel, retrying"
                        );
                        continue;
                    }
                },
                Err(_elapsed) => {
                    trace!(
                        conn_id = self.id,
                        query_id,
                        timeout_ms = current_timeout.as_millis(),
                        "UDP response timeout"
                    );
                    continue;
                }
            }
        }

        Err(DnsError::protocol("UDP query timed out after retries"))
    }

    /// Return the number of active queries currently tracked by this
    /// connection.
    fn using_count(&self) -> u16 {
        self.request_map.size()
    }

    /// Check if the UDP connection is available for new queries
    ///
    /// Returns false if the connection has been closed (e.g., due to send
    /// failure)
    fn available(&self) -> bool {
        !self.closed.load(Ordering::Acquire)
    }

    /// Return the timestamp (in ms) of last successful activity.
    fn last_used(&self) -> u64 {
        self.last_used.load(Ordering::Relaxed)
    }
}

impl UdpConnection {
    /// Construct a new UDP connection with the given parameters
    ///
    /// # Arguments
    /// * `conn_id` - Unique connection identifier for logging
    /// * `socket` - Pre-configured UDP socket connected to remote server
    fn new(conn_id: u16, socket: UdpSocket, request_map_capacity: u16) -> UdpConnection {
        Self {
            id: conn_id,
            transport: UdpTransport::new(socket),
            close_notify: Notify::new(),
            request_map: RequestMap::with_capacity(request_map_capacity),
            last_used: AtomicU64::new(AppClock::elapsed_millis()),
            closed: AtomicBool::new(false), // Initially open
        }
    }

    /// Asynchronously listen for DNS responses and deliver them to matching
    /// queries
    ///
    /// Continuously receives UDP datagrams and matches them to pending queries
    /// by ID. This task runs per connection until all requests complete or
    /// the connection closes.
    ///
    /// # Buffer Size
    /// Uses 4KB buffer which is sufficient for most DNS responses.
    /// Larger responses would typically use TCP (with TC bit set).
    async fn listen_dns_response(self: Arc<Self>) {
        let mut buf = vec![0u8; UDP_RECV_BUFFER_SIZE];
        let mut closing = false;

        debug!(
            conn_id = self.id,
            "UDP listener task started, waiting for DNS responses"
        );

        loop {
            if closing && self.request_map.is_empty() {
                debug!(conn_id = self.id, "Listener exiting (connection dropped)");
                break;
            }

            select! {
                recv = self.transport.read_message(&mut buf) => {
                    match recv {
                        Ok(msg) => {
                            let id = msg.id();
                            if let Some(sender) = self.request_map.take(id) {
                                let _ = sender.send(msg);
                                self.last_used.store(AppClock::elapsed_millis(), Ordering::Relaxed);
                                trace!(
                                    conn_id = self.id,
                                    id,
                                    "Delivered UDP response to waiting query"
                                );
                            } else {
                                trace!(
                                    conn_id = self.id,
                                    id,
                                    "No pending query or response fingerprint mismatch"
                                );
                            }
                        }
                        Err(e) => {
                            if self.closed.load(Ordering::Acquire) {
                                closing = true; // graceful shutdown path
                                continue;
                            }
                            warn!(conn_id = self.id, err = %e, "UDP listener error");
                            continue;
                        }
                    }
                }
                _ = self.close_notify.notified() => {
                    closing = true;
                }
            }
        }
    }
}

/// Builder for creating new `UdpConnection` instances.
#[derive(Debug)]
pub struct UdpConnectionBuilder {
    target: DialTarget,
    socket_options: SocketOptions,
    request_map_capacity: u16,
}

impl UdpConnectionBuilder {
    /// Initialize a new builder using upstream connection info.
    pub fn new(connection_info: &ConnectionInfo, request_map_capacity: u16) -> Self {
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
            request_map_capacity,
        }
    }
}

#[async_trait]
impl ConnectionBuilder<UdpConnection> for UdpConnectionBuilder {
    /// Create a new UDP connection, bind it locally, connect to remote server,
    /// and spawn a background listener task to handle responses
    ///
    /// # Returns
    /// Arc-wrapped UdpConnection with background listener task spawned
    ///
    /// # Performance
    /// - Non-blocking socket I/O
    /// - Single listener task handles all responses for this connection
    /// - Zero-copy where possible (direct socket buffer to DNS parser)
    async fn create_connection(
        &self,
        conn_id: u16,
        _deadline: QueryDeadline,
    ) -> Result<Arc<UdpConnection>> {
        let socket = connect_udp(UdpDialOptions::new(
            self.target.clone(),
            self.socket_options.clone(),
        ))?;

        debug!(
            conn_id,
            local_addr = ?socket.local_addr(),
            remote_addr = ?socket.peer_addr(),
            "Established UDP connection to DNS server"
        );

        let connection = UdpConnection::new(
            conn_id,
            UdpSocket::from_std(socket)?,
            self.request_map_capacity,
        );
        let arc = Arc::new(connection);

        // Spawn background task for listening responses
        tokio::spawn(UdpConnection::listen_dns_response(arc.clone()));

        Ok(arc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::network::upstream::ConnectionType;

    #[test]
    fn test_builder_new_copies_connection_info_fields() {
        let mut connection_info =
            ConnectionInfo::with_addr("udp://1.1.1.1:5300").expect("connection info should parse");
        connection_info.timeout = Duration::from_secs(7);
        connection_info.so_mark = Some(100);
        connection_info.bind_to_device = Some("en0".to_string());

        let builder = UdpConnectionBuilder::new(&connection_info, DEFAULT_REQUEST_MAP_CAPACITY);

        assert_eq!(connection_info.connection_type, ConnectionType::UDP);
        assert_eq!(builder.target.remote_ip(), connection_info.remote_ip);
        assert_eq!(builder.target.port(), 5300);
        assert_eq!(builder.request_map_capacity, DEFAULT_REQUEST_MAP_CAPACITY);
        assert_eq!(builder.target.host(), "1.1.1.1");
        assert_eq!(builder.socket_options.so_mark(), Some(100));
        assert_eq!(builder.socket_options.bind_to_device(), Some("en0"));
    }
}
