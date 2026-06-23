// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! DNS-over-TLS nameserver client.

use async_trait::async_trait;

use super::super::endpoint::NameserverConfig;
#[cfg(feature = "resolver-dot")]
use super::tcp::query_framed_tcp;
use super::{NameserverClient, effective_deadline};
#[cfg(not(feature = "resolver-dot"))]
use crate::infra::error::DnsError;
use crate::infra::error::Result;
#[cfg(feature = "resolver-dot")]
use crate::infra::network::deadline::DeadlineOutcome;
use crate::infra::network::deadline::QueryDeadline;
#[cfg(feature = "resolver-dot")]
use crate::infra::network::dial::{SocketOptions, TlsDialOptions, connect_tls};
#[cfg(feature = "resolver-dot")]
use crate::infra::network::proxy::connect_tcp as proxy_connect_tcp;
#[cfg(feature = "resolver-dot")]
use crate::infra::network::transport::tcp::TcpTransport;
use crate::proto::Message;

#[derive(Debug)]
pub(super) struct DotNameserverClient {
    config: NameserverConfig,
}

impl DotNameserverClient {
    pub(super) fn new(config: NameserverConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl NameserverClient for DotNameserverClient {
    async fn query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        query_dot_config(
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

#[cfg(feature = "resolver-dot")]
async fn query_dot_config(
    config: &NameserverConfig,
    request: Message,
    deadline: QueryDeadline,
) -> Result<Message> {
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
            Vec::new(),
        ),
    )
    .await?;
    let transport = TcpTransport::new(tls_stream);
    let (reader, writer) = transport.into_split();
    query_framed_tcp(reader, writer, request, deadline).await
}

#[cfg(not(feature = "resolver-dot"))]
async fn query_dot_config(
    _config: &NameserverConfig,
    _request: Message,
    _deadline: QueryDeadline,
) -> Result<Message> {
    Err(DnsError::plugin(
        "nameserver DoT is not compiled into this build; rebuild with --features resolver-dot",
    ))
}
