// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! UDP nameserver client.

use async_trait::async_trait;
use tokio::net::UdpSocket;
use tracing::debug;

use super::super::endpoint::NameserverConfig;
use super::super::query::validate_response_id;
use super::tcp::query_tcp_config;
use super::{NameserverClient, effective_deadline};
use crate::infra::error::Result;
use crate::infra::network::deadline::{DeadlineOutcome, QueryDeadline};
use crate::infra::network::dial::{SocketOptions, UdpDialOptions, connect_udp};
use crate::infra::network::transport::udp::UdpTransport;
use crate::proto::Message;

const UDP_RECV_BUFFER_SIZE: usize = 8_196;

#[derive(Debug)]
pub(super) struct UdpNameserverClient {
    config: NameserverConfig,
}

impl UdpNameserverClient {
    pub(super) fn new(config: NameserverConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl NameserverClient for UdpNameserverClient {
    async fn query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        query_udp_config(
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

async fn query_udp_config(
    config: &NameserverConfig,
    request: Message,
    deadline: QueryDeadline,
) -> Result<Message> {
    let socket = connect_udp(UdpDialOptions::new(
        config.target(),
        SocketOptions::default(),
    ))?;
    let socket = UdpSocket::from_std(socket)?;
    let transport = UdpTransport::new(socket);
    let query_id = request.id();

    match deadline
        .run(transport.write_message_with_id(&request, query_id))
        .await
    {
        DeadlineOutcome::Completed(result) => result?,
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    }

    let mut buf = [0u8; UDP_RECV_BUFFER_SIZE];
    let response = match deadline.run(transport.read_message(&mut buf)).await {
        DeadlineOutcome::Completed(result) => result?,
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    validate_response_id(&response, query_id)?;
    if response.truncated() {
        debug!(
            server = %config.label,
            "Nameserver UDP response truncated, falling back to TCP"
        );
        return query_tcp_config(config, request, deadline).await;
    }
    Ok(response)
}
