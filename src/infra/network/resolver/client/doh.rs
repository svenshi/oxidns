// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! DNS-over-HTTPS nameserver client.

use async_trait::async_trait;
#[cfg(feature = "resolver-doh")]
use base64::Engine;
#[cfg(feature = "resolver-doh")]
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
#[cfg(feature = "resolver-doh")]
use bytes::{BufMut, Bytes, BytesMut};
#[cfg(feature = "resolver-doh")]
use http::Version;

use super::super::endpoint::NameserverConfig;
#[cfg(feature = "resolver-doh")]
use super::super::endpoint::NameserverProtocol;
use super::{NameserverClient, effective_deadline};
use crate::infra::error::{DnsError, Result};
#[cfg(feature = "resolver-doh")]
use crate::infra::network::buffer_pool::wire_buffer_pool;
#[cfg(feature = "resolver-doh")]
use crate::infra::network::deadline::DeadlineOutcome;
use crate::infra::network::deadline::QueryDeadline;
#[cfg(feature = "resolver-doh")]
use crate::infra::network::dial::{SocketOptions, TlsDialOptions, connect_tls};
#[cfg(feature = "resolver-doh")]
use crate::infra::network::proxy::connect_tcp as proxy_connect_tcp;
use crate::proto::Message;

#[cfg(feature = "resolver-doh")]
const MAX_DNS_MESSAGE_LEN: usize = u16::MAX as usize;

#[derive(Debug)]
pub(super) struct DohNameserverClient {
    config: NameserverConfig,
}

impl DohNameserverClient {
    pub(super) fn new(config: NameserverConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl NameserverClient for DohNameserverClient {
    async fn query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        query_doh_config(
            &self.config,
            request,
            effective_deadline(deadline, self.config.timeout),
        )
        .await
    }

    fn label(&self) -> &str {
        self.config.label.as_str()
    }
}

#[cfg(feature = "resolver-doh")]
async fn query_doh_config(
    config: &NameserverConfig,
    request: Message,
    deadline: QueryDeadline,
) -> Result<Message> {
    use h2::client;

    let stream = match deadline
        .run(proxy_connect_tcp(
            config.target(),
            SocketOptions::default(),
            config.socks5.clone(),
        ))
        .await
    {
        DeadlineOutcome::Completed(result) => result?,
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    let tls_stream = connect_tls(
        stream,
        TlsDialOptions::new(
            config.target(),
            false,
            deadline
                .remaining()
                .ok_or_else(|| deadline.timeout_error())?,
            vec![b"h2".to_vec()],
        ),
    )
    .await?;
    let (mut sender, connection) = match deadline
        .run(client::Builder::new().handshake::<_, Bytes>(tls_stream))
        .await
    {
        DeadlineOutcome::Completed(Ok(value)) => value,
        DeadlineOutcome::Completed(Err(err)) => {
            return Err(DnsError::protocol(format!("H2 handshake error: {}", err)));
        }
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let raw_id = request.id();
    let mut body_bytes = wire_buffer_pool().acquire();
    request.append_to_with_id(0, &mut body_bytes)?;
    let http_request = build_doh_get_request(
        doh_request_uri(config),
        body_bytes.as_slice(),
        Version::HTTP_2,
    );
    let (response_future, _send_stream) = sender
        .send_request(http_request, true)
        .map_err(|err| DnsError::protocol(format!("H2 send_request error: {err}")))?;
    let mut response = match deadline.run(response_future).await {
        DeadlineOutcome::Completed(Ok(value)) => value,
        DeadlineOutcome::Completed(Err(err)) => {
            return Err(DnsError::protocol(format!("H2 response error: {}", err)));
        }
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    let mut response_bytes = response_buffer(&response);
    loop {
        let partial_bytes = match deadline.run(response.body_mut().data()).await {
            DeadlineOutcome::Completed(value) => value,
            DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
        };
        let Some(partial_bytes) = partial_bytes else {
            break;
        };
        response_bytes.put(
            partial_bytes
                .map_err(|err| DnsError::protocol(format!("H2 response body error: {}", err)))?,
        );
    }
    if !response.status().is_success() {
        return Err(DnsError::protocol(format!(
            "http unsuccessful code: {}",
            response.status()
        )));
    }
    let mut message = Message::from_bytes(&response_bytes)?;
    message.set_id(raw_id);
    Ok(message)
}

#[cfg(not(feature = "resolver-doh"))]
async fn query_doh_config(
    _config: &NameserverConfig,
    _request: Message,
    _deadline: QueryDeadline,
) -> Result<Message> {
    Err(DnsError::plugin(
        "nameserver DoH is not compiled into this build; rebuild with --features resolver-doh",
    ))
}

#[cfg(feature = "resolver-doh")]
pub(super) fn doh_request_uri(config: &NameserverConfig) -> String {
    let path = if config.path.is_empty() {
        "/dns-query"
    } else {
        config.path.as_str()
    };
    let mut uri = if config.port != NameserverProtocol::DoH.default_port() {
        format!("https://{}:{}{}?dns=", config.host, config.port, path)
    } else {
        format!("https://{}{}?dns=", config.host, path)
    };
    uri.reserve(512);
    uri
}

#[cfg(feature = "resolver-doh")]
pub(super) fn build_doh_get_request(
    mut uri: String,
    wire: &[u8],
    version: Version,
) -> http::Request<()> {
    uri.push_str(&BASE64_URL_SAFE_NO_PAD.encode(wire));
    http::Request::builder()
        .version(version)
        .header(http::header::CONTENT_TYPE, "application/dns-message")
        .header(http::header::ACCEPT, "application/dns-message")
        .method(http::Method::GET)
        .uri(uri)
        .body(())
        .expect("static DoH request should build")
}

#[cfg(feature = "resolver-doh")]
pub(super) fn response_buffer<T>(response: &http::Response<T>) -> BytesMut {
    let capacity = response
        .headers()
        .get(http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .map(|value| value.min(MAX_DNS_MESSAGE_LEN))
        .unwrap_or(4096);
    BytesMut::with_capacity(capacity)
}

#[cfg(all(test, feature = "resolver-doh"))]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_doh_request_uri_preserves_bracketed_ipv6_literals() {
        let config = NameserverConfig::new(
            "https://[2001:4860:4860::8888]/dns-query",
            None,
            Duration::from_secs(5),
            None,
        )
        .expect("IPv6 DoH nameserver should parse");

        let uri = doh_request_uri(&config);

        assert!(uri.starts_with("https://[2001:4860:4860::8888]/dns-query?dns="));
    }

    #[test]
    fn test_response_buffer_caps_content_length() {
        let response = http::Response::builder()
            .header(http::header::CONTENT_LENGTH, "999999999")
            .body(())
            .expect("response should build");

        let buffer = response_buffer(&response);

        assert_eq!(buffer.capacity(), MAX_DNS_MESSAGE_LEN);
    }
}
