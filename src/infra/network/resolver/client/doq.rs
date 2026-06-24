// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! DNS-over-QUIC nameserver client.

use async_trait::async_trait;

use super::super::endpoint::NameserverConfig;
#[cfg(feature = "resolver-doq")]
use super::super::query::validate_response_id;
use super::{NameserverClient, effective_deadline};
#[cfg(not(feature = "resolver-doq"))]
use crate::infra::error::DnsError;
use crate::infra::error::Result;
#[cfg(feature = "resolver-doq")]
use crate::infra::network::deadline::DeadlineOutcome;
use crate::infra::network::deadline::QueryDeadline;
#[cfg(feature = "resolver-doq")]
use crate::infra::network::dial::{
    QuicDialOptions, SocketOptions, UdpDialOptions, connect_quic, connect_udp,
};
#[cfg(feature = "resolver-doq")]
use crate::infra::network::transport::quic::QuicTransport;
use crate::proto::Message;

#[derive(Debug)]
pub(super) struct DoqNameserverClient {
    config: NameserverConfig,
}

impl DoqNameserverClient {
    pub(super) fn new(config: NameserverConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl NameserverClient for DoqNameserverClient {
    async fn query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        query_doq_config(
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

#[cfg(feature = "resolver-doq")]
async fn query_doq_config(
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
            vec![b"doq".to_vec()],
        ),
    )
    .await?;
    let transport = QuicTransport::new(quic_conn);
    let (mut reader, mut writer) = match deadline.run(transport.open_bi()).await {
        DeadlineOutcome::Completed(result) => result?,
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    let query_id = request.id();
    match deadline.run(writer.write_message(&request)).await {
        DeadlineOutcome::Completed(result) => result?,
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    }
    writer.finish()?;
    let response = match deadline.run(reader.read_message()).await {
        DeadlineOutcome::Completed(result) => result?,
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    validate_response_id(&response, query_id)?;
    transport.close(b"resolver query complete");
    Ok(response)
}

#[cfg(not(feature = "resolver-doq"))]
async fn query_doq_config(
    _config: &NameserverConfig,
    _request: Message,
    _deadline: QueryDeadline,
) -> Result<Message> {
    Err(DnsError::plugin(
        "nameserver DoQ is not compiled into this build; rebuild with --features resolver-doq",
    ))
}
