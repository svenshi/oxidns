// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later
use std::fmt::Debug;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

use async_trait::async_trait;
use bytes::{BufMut, Bytes};
use h2::client::{ResponseFuture, SendRequest};
use http::Version;
use tokio::select;
use tokio::sync::Notify;
use tracing::{debug, trace, warn};

use super::UsingCountGuard;
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::buffer_pool::wire_buffer_pool;
use crate::infra::network::proxy::Socks5Opt;
use crate::infra::network::upstream::conn::doh::{
    build_dns_get_request, build_doh_request_uri, get_cap_buf_with_context_len,
};
use crate::infra::network::upstream::dial::{connect_stream_with_deadline, connect_tls};
use crate::infra::network::upstream::pool::{ConnectionBuilder, DeadlineOutcome, QueryDeadline};
use crate::infra::network::upstream::{Connection, ConnectionInfo};
use crate::proto::Message;

enum H2RecvError {
    Transport(DnsError),
    HttpStatus(DnsError),
}

#[derive(Debug)]
pub struct H2Connection {
    id: u16,
    sender: SendRequest<Bytes>,
    using_count: AtomicU16,
    closed: AtomicBool,
    last_used: AtomicU64,
    request_uri: String,
    close_notify: Notify,
}

#[async_trait]
impl Connection for H2Connection {
    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        debug!(conn_id = self.id, "Closing DoH connection");
        self.close_notify.notify_waiters();
    }

    async fn query(&self, request: Message, _deadline: QueryDeadline) -> Result<Message> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DnsError::protocol("DoH connection closed"));
        }
        self.using_count.fetch_add(1, Ordering::Relaxed);
        // Guard ensures using_count is decremented even if this future is
        // cancelled by an outer timeout (cancel-safety).
        let _guard = UsingCountGuard(&self.using_count);
        if self.closed.load(Ordering::Acquire) {
            return Err(DnsError::protocol("DoH connection closed"));
        }
        self.query_inner(request).await
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

impl H2Connection {
    async fn query_inner(&self, request: Message) -> Result<Message> {
        let raw_id = request.id();
        let mut body_bytes = wire_buffer_pool().acquire();
        request.append_to_with_id(0, &mut body_bytes)?;

        let request = build_dns_get_request(
            self.request_uri.clone(),
            body_bytes.as_slice(),
            Version::HTTP_2,
        );

        let (response_future, _send_stream) = self
            .sender
            .clone()
            // DoH GET carries the DNS payload in the URI, so the request body is empty.
            // Mark the stream as finished when sending headers, otherwise some servers
            // will wait for an end-of-stream signal and never produce a response.
            .send_request(request, true)
            .map_err(|e| {
                self.close();
                DnsError::protocol(format!("H2 send_request error: {e}"))
            })?;

        match recv(response_future).await {
            Ok(bytes) => {
                let mut resp = Message::from_bytes(&bytes)?;
                resp.set_id(raw_id);
                self.last_used
                    .store(AppClock::elapsed_millis(), Ordering::Relaxed);
                trace!(conn_id = self.id, raw_id, "Received H2 response");
                Ok(resp)
            }
            Err(H2RecvError::Transport(e)) => {
                self.close();
                warn!(conn_id = self.id, raw_id, ?e, "H2 request error");
                Err(e)
            }
            Err(H2RecvError::HttpStatus(e)) => Err(e),
        }
    }
}

/// Builder
#[derive(Debug)]
pub struct H2ConnectionBuilder {
    remote_ip: Option<IpAddr>,
    port: u16,
    server_name: String,
    request_uri: String,
    insecure_skip_verify: bool,
    so_mark: Option<u32>,
    bind_to_device: Option<String>,
    socks5: Option<Socks5Opt>,
}

impl H2ConnectionBuilder {
    pub fn new(connection_info: &ConnectionInfo) -> Self {
        Self {
            remote_ip: connection_info.remote_ip,
            port: connection_info.port,
            server_name: connection_info.server_name.clone(),
            request_uri: build_doh_request_uri(connection_info),
            insecure_skip_verify: connection_info.insecure_skip_verify,
            so_mark: connection_info.so_mark,
            bind_to_device: connection_info.bind_to_device.clone(),
            socks5: connection_info.socks5.clone(),
        }
    }
}

#[async_trait]
impl ConnectionBuilder<H2Connection> for H2ConnectionBuilder {
    async fn create_connection(
        &self,
        conn_id: u16,
        deadline: QueryDeadline,
    ) -> Result<Arc<H2Connection>> {
        let stream = connect_stream_with_deadline(
            self.remote_ip,
            self.server_name.clone(),
            self.port,
            self.so_mark,
            self.bind_to_device.clone(),
            self.socks5.clone(),
            deadline,
        )
        .await?;

        let tls_stream = connect_tls(
            stream,
            self.insecure_skip_verify,
            self.server_name.clone(),
            deadline
                .remaining()
                .ok_or_else(|| deadline.timeout_error())?,
            vec![b"h2".to_vec()],
        )
        .await?;

        let (sender, connection) = match deadline
            .run(h2::client::Builder::new().handshake(tls_stream))
            .await
        {
            DeadlineOutcome::Completed(Ok(value)) => value,
            DeadlineOutcome::Completed(Err(e)) => {
                return Err(DnsError::protocol(format!("H2 handshake error: {}", e)));
            }
            DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
        };

        let h2_conn = Arc::new(H2Connection {
            id: conn_id,
            sender,
            closed: AtomicBool::new(false),
            last_used: AtomicU64::new(AppClock::elapsed_millis()),
            using_count: AtomicU16::new(0),
            request_uri: self.request_uri.clone(),
            close_notify: Notify::new(),
        });

        let _conn = h2_conn.clone();
        tokio::spawn(async move {
            select! {
                res = connection => {
                    _conn.close();
                    match res {
                        Ok(()) => debug!(conn_id, "H2 connection closed"),
                        Err(e) => debug!(conn_id, ?e, "H2 connection error"),
                    }
                }
                _ = _conn.close_notify.notified() => {
                    debug!(conn_id, "H2 connection closed by notify");
                }
            }
        });

        Ok(h2_conn)
    }
}

async fn recv(response_future: ResponseFuture) -> std::result::Result<Bytes, H2RecvError> {
    let mut response = response_future.await.map_err(|e| {
        H2RecvError::Transport(DnsError::protocol(format!("H2 response error: {}", e)))
    })?;

    let status_code = response.status();
    let mut response_bytes = get_cap_buf_with_context_len(&mut response);
    let mut body = response.into_body();

    while let Some(partial_bytes) = body.data().await {
        let partial_bytes = partial_bytes.map_err(|e| {
            H2RecvError::Transport(DnsError::protocol(format!("H2 body error: {}", e)))
        })?;
        response_bytes.put_slice(&partial_bytes);
    }

    if !status_code.is_success() {
        let error_string = String::from_utf8_lossy(response_bytes.as_ref());
        Err(H2RecvError::HttpStatus(DnsError::protocol(format!(
            "http unsuccessful code: {}, message: {}",
            status_code, error_string
        ))))
    } else {
        Ok(response_bytes.freeze())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_new_uses_https_request_uri_and_flags() {
        let mut connection_info = ConnectionInfo::with_addr("https://dns.example.com/dns-query")
            .expect("connection info should parse");
        connection_info.insecure_skip_verify = true;
        connection_info.so_mark = Some(42);
        connection_info.bind_to_device = Some("utun9".to_string());

        let builder = H2ConnectionBuilder::new(&connection_info);

        assert_eq!(builder.port, 443);
        assert_eq!(builder.server_name, "dns.example.com");
        assert_eq!(
            builder.request_uri,
            "https://dns.example.com/dns-query?dns="
        );
        assert!(builder.insecure_skip_verify);
        assert_eq!(builder.so_mark, Some(42));
        assert_eq!(builder.bind_to_device.as_deref(), Some("utun9"));
    }
}
