// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Short-connection nameserver clients.

use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use super::endpoint::{NameserverConfig, NameserverProtocol};
use crate::infra::error::{DnsError, Result};
use crate::infra::network::deadline::QueryDeadline;
use crate::proto::Message;

mod doh;
mod doh3;
mod doq;
mod dot;
mod tcp;
mod udp;

#[async_trait]
pub(super) trait NameserverClient: Debug + Send + Sync {
    async fn query(&self, request: Message, deadline: QueryDeadline) -> Result<Message>;

    fn label(&self) -> &str;
}

pub(super) fn build_clients(
    nameservers: Vec<NameserverConfig>,
) -> Result<Vec<Arc<dyn NameserverClient>>> {
    nameservers
        .into_iter()
        .map(build_client)
        .collect::<Result<Vec<_>>>()
}

fn build_client(config: NameserverConfig) -> Result<Arc<dyn NameserverClient>> {
    if let Some(hint) = config.protocol.rebuild_hint() {
        return Err(DnsError::plugin(hint));
    }
    if config.socks5.is_some() && !config.protocol.supports_socks5() {
        return Err(DnsError::config(format!(
            "nameserver '{}' does not support SOCKS5 proxy",
            config.label
        )));
    }

    let client: Arc<dyn NameserverClient> = match config.protocol {
        NameserverProtocol::Udp => Arc::new(udp::UdpNameserverClient::new(config)),
        NameserverProtocol::Tcp => Arc::new(tcp::TcpNameserverClient::new(config)),
        NameserverProtocol::DoT => Arc::new(dot::DotNameserverClient::new(config)),
        NameserverProtocol::DoH => Arc::new(doh::DohNameserverClient::new(config)),
        NameserverProtocol::DoH3 => Arc::new(doh3::Doh3NameserverClient::new(config)),
        NameserverProtocol::DoQ => Arc::new(doq::DoqNameserverClient::new(config)),
    };
    Ok(client)
}

fn effective_deadline(deadline: QueryDeadline, timeout: Duration) -> QueryDeadline {
    let timeout_deadline = QueryDeadline::new(timeout);
    if timeout_deadline.expires_at_ms < deadline.expires_at_ms {
        timeout_deadline
    } else {
        deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::clock::AppClock;

    #[test]
    fn test_effective_deadline_preserves_expired_deadline() {
        AppClock::start();
        let deadline = QueryDeadline::new(Duration::ZERO);

        let effective = effective_deadline(deadline, Duration::from_secs(5));

        assert_eq!(effective, deadline);
        assert!(effective.remaining().is_none());
    }

    #[test]
    fn test_effective_deadline_uses_shorter_nameserver_timeout() {
        AppClock::start();
        let deadline = QueryDeadline::new(Duration::from_secs(5));

        let effective = effective_deadline(deadline, Duration::from_millis(500));

        assert!(effective.expires_at_ms < deadline.expires_at_ms);
    }

    #[test]
    fn test_effective_deadline_keeps_shorter_caller_deadline() {
        AppClock::start();
        let deadline = QueryDeadline::new(Duration::from_millis(500));

        let effective = effective_deadline(deadline, Duration::from_secs(5));

        assert_eq!(effective, deadline);
    }
}
