// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! DNS-over-HTTP/3 nameserver client.

use async_trait::async_trait;
#[cfg(feature = "resolver-doh3")]
use futures::future::poll_fn;
#[cfg(feature = "resolver-doh3")]
use http::Version;

use super::super::endpoint::NameserverConfig;
#[cfg(feature = "resolver-doh3")]
use super::doh::{append_response_chunk, build_doh_get_request, doh_request_uri, response_buffer};
use super::{NameserverClient, effective_deadline};
use crate::infra::error::{DnsError, Result};
#[cfg(feature = "resolver-doh3")]
use crate::infra::network::buffer_pool::wire_buffer_pool;
#[cfg(feature = "resolver-doh3")]
use crate::infra::network::deadline::DeadlineOutcome;
use crate::infra::network::deadline::QueryDeadline;
#[cfg(feature = "resolver-doh3")]
use crate::infra::network::dial::{
    QuicDialOptions, SocketOptions, UdpDialOptions, connect_quic, connect_udp,
};
use crate::proto::Message;

#[derive(Debug)]
pub(super) struct Doh3NameserverClient {
    config: NameserverConfig,
}

impl Doh3NameserverClient {
    pub(super) fn new(config: NameserverConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl NameserverClient for Doh3NameserverClient {
    async fn query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        query_doh3_config(
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

#[cfg(feature = "resolver-doh3")]
async fn query_doh3_config(
    config: &NameserverConfig,
    request: Message,
    deadline: QueryDeadline,
) -> Result<Message> {
    let socket = connect_udp(UdpDialOptions::new(
        config.target(),
        SocketOptions::default(),
    ))?;
    let quic_conn = connect_quic(
        socket,
        QuicDialOptions::new(
            config.target(),
            false,
            deadline
                .remaining()
                .ok_or_else(|| deadline.timeout_error())?,
            config.timeout,
            vec![b"h3".to_vec()],
        ),
    )
    .await?;
    let h3_conn = h3_quinn::Connection::new(quic_conn);
    let (mut driver, mut send_request) = match deadline.run(h3::client::new(h3_conn)).await {
        DeadlineOutcome::Completed(Ok(value)) => value,
        DeadlineOutcome::Completed(Err(err)) => {
            return Err(DnsError::protocol(format!("h3 connection failed: {err}")));
        }
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    tokio::spawn(async move {
        let _ = poll_fn(|cx| driver.poll_close(cx)).await;
    });

    let raw_id = request.id();
    let mut body_bytes = wire_buffer_pool().acquire();
    request.append_to_with_id(0, &mut body_bytes)?;
    let http_request = build_doh_get_request(
        doh_request_uri(config),
        body_bytes.as_slice(),
        Version::HTTP_3,
    );
    let mut stream = match deadline.run(send_request.send_request(http_request)).await {
        DeadlineOutcome::Completed(Ok(value)) => value,
        DeadlineOutcome::Completed(Err(err)) => {
            return Err(DnsError::protocol(format!("H3 send_request error: {err}")));
        }
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    match deadline.run(stream.finish()).await {
        DeadlineOutcome::Completed(result) => {
            result.map_err(|err| DnsError::protocol(format!("H3 stream finish error: {err}")))?
        }
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    }
    let response = match deadline.run(stream.recv_response()).await {
        DeadlineOutcome::Completed(Ok(value)) => value,
        DeadlineOutcome::Completed(Err(err)) => {
            return Err(DnsError::protocol(format!("H3 response error: {err}")));
        }
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    let mut response_bytes = response_buffer(&response);
    loop {
        let data = match deadline.run(stream.recv_data()).await {
            DeadlineOutcome::Completed(Ok(value)) => value,
            DeadlineOutcome::Completed(Err(err)) => {
                return Err(DnsError::protocol(format!("H3 recv_data error: {err}")));
            }
            DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
        };
        let Some(partial_bytes) = data else {
            break;
        };
        append_response_chunk(&mut response_bytes, partial_bytes)?;
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

#[cfg(not(feature = "resolver-doh3"))]
async fn query_doh3_config(
    _config: &NameserverConfig,
    _request: Message,
    _deadline: QueryDeadline,
) -> Result<Message> {
    Err(DnsError::plugin(
        "nameserver DoH3 is not compiled into this build; rebuild with --features resolver-doh3",
    ))
}
