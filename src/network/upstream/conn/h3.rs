// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later
use std::fmt::{Debug, Formatter};
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

use async_trait::async_trait;
use bytes::{BufMut, Bytes};
use futures::future::poll_fn;
use h3::client::{RequestStream, SendRequest};
use h3_quinn::{BidiStream, OpenStreams};
use http::{Request, Version};
use tokio::select;
use tokio::sync::Notify;
use tracing::{debug, trace, warn};

use super::UsingCountGuard;
use crate::core::app_clock::AppClock;
use crate::core::error::{DnsError, Result};
use crate::network::buffer_pool::wire_buffer_pool;
use crate::network::upstream::conn::doh::{
    build_dns_get_request, build_doh_request_uri, get_cap_buf_with_context_len,
};
use crate::network::upstream::dial::{connect_quic, connect_socket};
use crate::network::upstream::pool::{ConnectionBuilder, DeadlineOutcome, QueryDeadline};
use crate::network::upstream::{Connection, ConnectionInfo};
use crate::proto::Message;

enum H3RecvError {
    Transport(DnsError),
    HttpStatus(DnsError),
}

pub struct H3Connection {
    id: u16,
    sender: SendRequest<OpenStreams, Bytes>,
    using_count: AtomicU16,
    closed: AtomicBool,
    last_used: AtomicU64,
    request_uri: String,
    close_notify: Notify,
}
impl Debug for H3Connection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("H3Connection")
    }
}

#[async_trait]
impl Connection for H3Connection {
    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        debug!(conn_id = self.id, "Closing H3 connection");
        self.close_notify.notify_waiters();
    }

    async fn query(&self, request: Message, _deadline: QueryDeadline) -> Result<Message> {
        if self.closed.load(Ordering::Acquire) {
            return Err(DnsError::protocol("H3 connection closed"));
        }
        self.using_count.fetch_add(1, Ordering::Relaxed);
        // Guard ensures using_count is decremented even if this future is
        // cancelled by an outer timeout (cancel-safety).
        let _guard = UsingCountGuard(&self.using_count);
        if self.closed.load(Ordering::Acquire) {
            return Err(DnsError::protocol("H3 connection closed"));
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

impl H3Connection {
    async fn query_inner(&self, request: Message) -> Result<Message> {
        let raw_id = request.id();
        let mut body_bytes = wire_buffer_pool().acquire();
        request.append_to_with_id(0, &mut body_bytes)?;

        let http_request = build_dns_get_request(
            self.request_uri.clone(),
            body_bytes.as_slice(),
            Version::HTTP_3,
        );

        self.do_request(http_request, raw_id).await
    }

    async fn do_request(&self, http_request: Request<()>, raw_id: u16) -> Result<Message> {
        let mut request_stream = self
            .sender
            .clone()
            .send_request(http_request)
            .await
            .map_err(|e| {
                self.close();
                DnsError::protocol(format!("H3 send_request error: {e}"))
            })?;

        request_stream.finish().await.map_err(|err| {
            self.close();
            DnsError::protocol(format!("H3 received a stream error: {err}"))
        })?;

        match recv(request_stream).await {
            Ok(bytes) => {
                let mut resp = Message::from_bytes(&bytes)?;
                resp.set_id(raw_id);
                self.last_used
                    .store(AppClock::elapsed_millis(), Ordering::Relaxed);
                trace!(conn_id = self.id, raw_id, "Received H3 response");
                Ok(resp)
            }
            Err(H3RecvError::Transport(e)) => {
                self.close();
                warn!(conn_id = self.id, raw_id, ?e, "H3 request error");
                Err(e)
            }
            Err(H3RecvError::HttpStatus(e)) => Err(e),
        }
    }
}

/// Builder
#[derive(Debug)]
pub struct H3ConnectionBuilder {
    remote_ip: Option<IpAddr>,
    port: u16,
    server_name: String,
    request_uri: String,
    insecure_skip_verify: bool,
    timeout: std::time::Duration,
    so_mark: Option<u32>,
    bind_to_device: Option<String>,
}

impl H3ConnectionBuilder {
    pub fn new(connection_info: &ConnectionInfo) -> Self {
        Self {
            remote_ip: connection_info.remote_ip,
            port: connection_info.port,
            server_name: connection_info.server_name.clone(),
            request_uri: build_doh_request_uri(connection_info),
            insecure_skip_verify: connection_info.insecure_skip_verify,
            timeout: connection_info.timeout,
            so_mark: connection_info.so_mark,
            bind_to_device: connection_info.bind_to_device.clone(),
        }
    }
}

#[async_trait]
impl ConnectionBuilder<H3Connection> for H3ConnectionBuilder {
    async fn create_connection(
        &self,
        conn_id: u16,
        deadline: QueryDeadline,
    ) -> Result<Arc<H3Connection>> {
        let socket = connect_socket(
            self.remote_ip,
            self.server_name.clone(),
            self.port,
            self.so_mark,
            self.bind_to_device.clone(),
        )?;

        let quic_conn = connect_quic(
            socket,
            self.insecure_skip_verify,
            self.server_name.clone(),
            deadline
                .remaining()
                .ok_or_else(|| deadline.timeout_error())?,
            self.timeout,
            vec![b"h3".to_vec()],
        )
        .await?;

        let h3_conn = h3_quinn::Connection::new(quic_conn);

        let (mut driver, send_request) = match deadline.run(h3::client::new(h3_conn)).await {
            DeadlineOutcome::Completed(Ok(value)) => value,
            DeadlineOutcome::Completed(Err(e)) => {
                return Err(DnsError::protocol(format!("h3 connection failed: {e}")));
            }
            DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
        };

        let h3_conn = Arc::new(H3Connection {
            id: conn_id,
            sender: send_request,
            closed: AtomicBool::new(false),
            last_used: AtomicU64::new(AppClock::elapsed_millis()),
            using_count: AtomicU16::new(0),
            request_uri: self.request_uri.clone(),
            close_notify: Notify::new(),
        });

        let _conn = h3_conn.clone();

        let _driver_handle = tokio::spawn(async move {
            select! {
                _ = poll_fn(|cx| driver.poll_close(cx)) => {
                    _conn.close();
                    debug!(conn_id, "H3 connection poll closed");
                }
                _ = _conn.close_notify.notified()=>{
                    debug!(conn_id, "H3 connection closed by notify");
                }
            }
            let _ = poll_fn(|cx| driver.poll_close(cx)).await;
        });

        Ok(h3_conn)
    }
}

async fn recv(
    mut request_stream: RequestStream<BidiStream<Bytes>, Bytes>,
) -> std::result::Result<Bytes, H3RecvError> {
    let mut response = request_stream.recv_response().await.map_err(|e| {
        H3RecvError::Transport(DnsError::protocol(format!("H3 response error: {}", e)))
    })?;

    let mut response_bytes = get_cap_buf_with_context_len(&mut response);

    while let Some(partial_bytes) = request_stream.recv_data().await.map_err(|e| {
        H3RecvError::Transport(DnsError::protocol(format!("h3 recv_data error: {e}")))
    })? {
        response_bytes.put(partial_bytes);
    }

    // Was it a successful request?
    if !response.status().is_success() {
        let error_string = String::from_utf8_lossy(response_bytes.as_ref());

        Err(H3RecvError::HttpStatus(DnsError::protocol(format!(
            "http unsuccessful code: {}, message: {}",
            response.status(),
            error_string
        ))))
    } else {
        Ok(response_bytes.freeze())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_new_uses_http3_request_uri_and_flags() {
        let mut connection_info = ConnectionInfo::with_addr("h3://dns.example.com/dns-query")
            .expect("connection info should parse");
        connection_info.timeout = std::time::Duration::from_secs(4);
        connection_info.insecure_skip_verify = true;
        connection_info.so_mark = Some(7);
        connection_info.bind_to_device = Some("utun1".to_string());

        let builder = H3ConnectionBuilder::new(&connection_info);

        assert_eq!(builder.port, 443);
        assert_eq!(builder.server_name, "dns.example.com");
        assert_eq!(
            builder.request_uri,
            "https://dns.example.com/dns-query?dns="
        );
        assert!(builder.insecure_skip_verify);
        assert_eq!(builder.timeout, std::time::Duration::from_secs(4));
        assert_eq!(builder.so_mark, Some(7));
        assert_eq!(builder.bind_to_device.as_deref(), Some("utun1"));
    }
}
