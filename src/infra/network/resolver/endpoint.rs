// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolver nameserver endpoint modeling and parsing.

use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;

use url::Url;

use crate::infra::error::{DnsError, Result};
use crate::infra::network::dial::DialTarget;
use crate::infra::network::proxy::Socks5Opt;

const DEFAULT_NAMESERVER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NameserverProtocol {
    Udp,
    Tcp,
    DoT,
    DoH,
    DoH3,
    DoQ,
}

impl NameserverProtocol {
    pub(super) fn default_port(self) -> u16 {
        match self {
            Self::Udp | Self::Tcp => 53,
            Self::DoT | Self::DoQ => 853,
            Self::DoH | Self::DoH3 => 443,
        }
    }

    pub(super) fn supports_socks5(self) -> bool {
        matches!(self, Self::Tcp | Self::DoT | Self::DoH)
    }

    pub(super) fn rebuild_hint(self) -> Option<&'static str> {
        match self {
            Self::DoT if !cfg!(feature = "resolver-dot") => Some(
                "nameserver DoT is not compiled into this build; rebuild with --features resolver-dot",
            ),
            Self::DoH if !cfg!(feature = "resolver-doh") => Some(
                "nameserver DoH is not compiled into this build; rebuild with --features resolver-doh",
            ),
            Self::DoH3 if !cfg!(feature = "resolver-doh3") => Some(
                "nameserver DoH3 is not compiled into this build; rebuild with --features resolver-doh3",
            ),
            Self::DoQ if !cfg!(feature = "resolver-doq") => Some(
                "nameserver DoQ is not compiled into this build; rebuild with --features resolver-doq",
            ),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NameserverConfig {
    pub(super) label: String,
    pub(super) protocol: NameserverProtocol,
    pub(super) host: String,
    pub(super) remote_ip: Option<IpAddr>,
    pub(super) port: u16,
    #[cfg_attr(not(feature = "resolver-doh"), allow(dead_code))]
    pub(super) path: String,
    pub(super) timeout: Duration,
    pub(super) socks5: Option<Socks5Opt>,
}

impl NameserverConfig {
    pub(crate) fn new(
        addr: impl Into<String>,
        dial_addr: Option<IpAddr>,
        timeout: Duration,
        socks5: Option<Socks5Opt>,
    ) -> Result<Self> {
        let addr = addr.into();
        let endpoint = parse_nameserver_endpoint(addr.as_str())?;
        if socks5.is_some() && !endpoint.protocol.supports_socks5() {
            return Err(DnsError::config(format!(
                "nameserver '{}' does not support SOCKS5 proxy",
                addr
            )));
        }
        let remote_ip = dial_addr.or_else(|| endpoint.host.parse::<IpAddr>().ok());
        Ok(Self {
            label: addr,
            protocol: endpoint.protocol,
            host: endpoint.host,
            remote_ip,
            port: endpoint.port,
            path: endpoint.path,
            timeout,
            socks5,
        })
    }

    pub(super) fn legacy_bootstrap(server: &str) -> Result<Self> {
        let server = server.trim();
        let socket_addr = SocketAddr::from_str(server).map_err(|err| {
            if server.contains("://") {
                DnsError::plugin(format!("invalid bootstrap upstream '{}': {}", server, err))
            } else {
                DnsError::plugin(format!(
                    "bootstrap upstream '{}' must use a literal IP address",
                    server
                ))
            }
        })?;
        Self::new(
            format!("udp://{socket_addr}"),
            Some(socket_addr.ip()),
            DEFAULT_NAMESERVER_TIMEOUT,
            None,
        )
    }

    pub(super) fn target(&self) -> DialTarget {
        DialTarget::new(self.remote_ip, self.host.clone(), self.port)
    }
}

#[derive(Debug)]
struct ParsedNameserverEndpoint {
    protocol: NameserverProtocol,
    host: String,
    port: u16,
    path: String,
}

fn parse_nameserver_endpoint(addr: &str) -> Result<ParsedNameserverEndpoint> {
    let raw = addr.trim();
    if raw.is_empty() {
        return Err(DnsError::config("nameserver addr cannot be empty"));
    }
    let normalized;
    let candidate = if raw.contains("//") {
        raw
    } else {
        normalized = format!("udp://{raw}");
        normalized.as_str()
    };
    let url = Url::parse(candidate)
        .map_err(|err| DnsError::config(format!("invalid nameserver addr '{}': {}", raw, err)))?;
    let host = match url
        .host()
        .ok_or_else(|| DnsError::config(format!("invalid nameserver addr '{}': no host", raw)))?
    {
        url::Host::Domain(domain) => domain.to_string(),
        url::Host::Ipv4(ip) => ip.to_string(),
        url::Host::Ipv6(ip) => ip.to_string(),
    };
    let protocol = match url.scheme() {
        "udp" => NameserverProtocol::Udp,
        "tcp" | "tcp+pipeline" => NameserverProtocol::Tcp,
        "tls" | "tls+pipeline" => NameserverProtocol::DoT,
        "https" | "doh" => NameserverProtocol::DoH,
        "h3" => NameserverProtocol::DoH3,
        "quic" | "doq" => NameserverProtocol::DoQ,
        other => {
            return Err(DnsError::config(format!(
                "invalid nameserver URL scheme: {}",
                other
            )));
        }
    };
    let port = url.port().unwrap_or_else(|| protocol.default_port());
    let path = match protocol {
        NameserverProtocol::DoH | NameserverProtocol::DoH3 => url.path().to_string(),
        _ => String::new(),
    };
    Ok(ParsedNameserverEndpoint {
        protocol,
        host,
        port,
        path,
    })
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    #[test]
    fn test_legacy_bootstrap_rejects_invalid_server() {
        let result = NameserverConfig::legacy_bootstrap("udp://127.0.0.1:notaport");

        assert!(result.is_err());
    }

    #[test]
    fn test_legacy_bootstrap_rejects_hostname_server() {
        let result = NameserverConfig::legacy_bootstrap("resolver.example.invalid:53");

        assert!(
            result
                .expect_err("hostname bootstrap server should be rejected")
                .to_string()
                .contains("must use a literal IP address")
        );
    }

    #[test]
    fn test_parser_defaults_to_udp() {
        let endpoint =
            parse_nameserver_endpoint("8.8.8.8:53").expect("endpoint should parse as UDP");

        assert_eq!(endpoint.protocol, NameserverProtocol::Udp);
        assert_eq!(endpoint.host, "8.8.8.8");
        assert_eq!(endpoint.port, 53);
    }

    #[test]
    fn test_parser_applies_protocol_default_ports() {
        let cases = [
            ("tcp://dns.example", NameserverProtocol::Tcp, 53),
            ("tls://dns.example", NameserverProtocol::DoT, 853),
            (
                "https://dns.example/dns-query",
                NameserverProtocol::DoH,
                443,
            ),
            ("h3://dns.example/dns-query", NameserverProtocol::DoH3, 443),
            ("doq://dns.example", NameserverProtocol::DoQ, 853),
        ];

        for (addr, protocol, port) in cases {
            let endpoint = parse_nameserver_endpoint(addr).expect("endpoint should parse");
            assert_eq!(endpoint.protocol, protocol);
            assert_eq!(endpoint.port, port);
        }
    }

    #[test]
    fn test_parser_preserves_doh_path() {
        let endpoint =
            parse_nameserver_endpoint("https://dns.example/custom").expect("endpoint should parse");

        assert_eq!(endpoint.path, "/custom");
    }

    #[test]
    fn test_parser_rejects_invalid_scheme() {
        let result = parse_nameserver_endpoint("ftp://dns.example");

        assert!(result.is_err());
    }

    #[test]
    fn test_nameserver_config_uses_dial_addr_for_domain_endpoint() {
        let config = NameserverConfig::new(
            "tls://dns.example:853",
            Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 53))),
            Duration::from_secs(1),
            None,
        )
        .expect("config should build");

        assert_eq!(config.host, "dns.example");
        assert_eq!(
            config.remote_ip,
            Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 53)))
        );
    }

    #[test]
    fn test_nameserver_config_stores_ipv6_literal_without_brackets() {
        let config = NameserverConfig::new(
            "https://[2001:4860:4860::8888]/dns-query",
            None,
            Duration::from_secs(1),
            None,
        )
        .expect("config should build");

        assert_eq!(config.host, "2001:4860:4860::8888");
        assert_eq!(
            config.remote_ip,
            Some(IpAddr::V6(
                "2001:4860:4860::8888".parse().expect("IPv6 should parse")
            ))
        );
    }
}
