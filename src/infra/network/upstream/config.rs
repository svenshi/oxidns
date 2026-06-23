// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, warn};
use url::Url;

use crate::infra::error::{DnsError, Result};
use crate::infra::network::outbound;
use crate::infra::network::proxy::{Socks5Opt, parse_socks5_opt};
use crate::infra::network::resolver::NameResolver;
use crate::infra::system::deserialize_duration_option;

/// Supported upstream connection types
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConnectionType {
    UDP,
    TCP,
    DoT,
    DoQ,
    DoH,
}

#[allow(unused)]
impl ConnectionType {
    /// Returns the default port for each connection type
    pub fn default_port(&self) -> u16 {
        match self {
            ConnectionType::UDP => 53,
            ConnectionType::TCP => 53,
            ConnectionType::DoT => 853,
            ConnectionType::DoQ => 853,
            ConnectionType::DoH => 443,
        }
    }

    /// Returns all supported URL schemes for this connection type
    pub fn schemes(&self) -> Vec<&str> {
        match self {
            ConnectionType::UDP => vec!["udp", ""],
            ConnectionType::TCP => vec!["tcp", "tcp+pipeline"],
            ConnectionType::DoT => vec!["tls", "tls+pipeline"],
            ConnectionType::DoQ => vec!["doq", "quic"],
            ConnectionType::DoH => vec!["doh", "https", "h3"],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProxySource {
    Local,
    Profile,
}

/// Configuration for building an upstream DNS server connection
///
/// This structure is typically deserialized from YAML/JSON configuration files
/// and contains all parameters needed to establish a connection to an upstream
/// DNS server.
///
/// # Examples
///
/// Basic UDP configuration:
/// ```yaml
/// addr: "8.8.8.8:53"
/// ```
///
/// DoH with bootstrap:
/// ```yaml
/// addr: "https://dns.google.com/dns-query"
/// bootstrap: "8.8.8.8:53"
/// timeout: 5s
/// ```
#[derive(Deserialize, Debug, Clone)]
pub struct UpstreamConfig {
    /// Optional tag for identifying this upstream in logs
    pub tag: Option<String>,

    /// DNS server address in URL format
    ///
    /// Supported formats:
    /// - `udp://8.8.8.8:53` or `8.8.8.8` - DNS over UDP
    /// - `tcp://8.8.8.8:53` - DNS over TCP
    /// - `tls://dns.google.com:853` - DNS over TLS (DoT)
    /// - `quic://dns.adguard.com:853` - DNS over QUIC (DoQ)
    /// - `https://dns.google.com/dns-query` - DNS over HTTPS (DoH)
    pub addr: String,

    /// Optional named outbound profile to supply resolver/proxy defaults.
    ///
    /// Local upstream fields keep precedence: `dial_addr` bypasses resolver
    /// injection, `bootstrap` overrides the profile resolver, and `socks5`
    /// overrides the profile proxy.
    pub outbound: Option<String>,

    /// Direct IP address to use for connection (bypasses DNS resolution)
    ///
    /// Useful when you want to connect to a specific IP but use SNI for TLS.
    /// If provided, this IP is used instead of resolving the hostname from
    /// `addr`. Mutually exclusive with `bootstrap` at runtime: when both are
    /// configured, `dial_addr` takes precedence and `bootstrap` is ignored.
    pub dial_addr: Option<IpAddr>,

    /// Override the server port (if not specified in `addr`)
    ///
    /// Defaults to protocol-specific standard ports if not provided:
    /// - UDP/TCP: 53
    /// - DoT/DoQ: 853
    /// - DoH: 443
    pub port: Option<u16>,

    /// Bootstrap DNS server for resolving the upstream hostname
    ///
    /// Recommended when `addr` contains a hostname instead of an IP address.
    /// Without bootstrap, hostname resolution is deferred to connection time
    /// and uses the operating system resolver. The bootstrap server must be
    /// specified as IP:port (e.g., "8.8.8.8:53") to avoid circular
    /// dependencies in DNS resolution; hostnames are rejected. Mutually
    /// exclusive with `dial_addr` at runtime: when both are configured,
    /// `dial_addr` takes precedence and bootstrap resolution is skipped.
    ///
    /// # Example
    /// ```yaml
    /// addr: "https://dns.google.com/dns-query"
    /// bootstrap: "8.8.8.8:53"  # Use Google's IP to resolve dns.google.com
    /// ```
    pub bootstrap: Option<String>,

    /// IP version preference for bootstrap DNS resolution
    ///
    /// - `Some(4)` or `None`: Resolve to IPv4 (A records)
    /// - `Some(6)`: Resolve to IPv6 (AAAA records)
    pub bootstrap_version: Option<u8>,

    /// SOCKS5 proxy server for upstream connections
    ///
    /// When specified, all DNS connections to the upstream server will be
    /// routed through this SOCKS5 proxy. The proxy address can be either an
    /// IP address or a hostname (which will be resolved using system DNS).
    ///
    /// Supports two formats:
    /// - **Without authentication**: `"host:port"`
    ///   - Example: `"127.0.0.1:1080"`
    ///   - Example: `"proxy.example.com:1080"`
    ///
    /// - **With authentication**: `"username:password@host:port"`
    ///   - Example: `"user:pass@127.0.0.1:1080"`
    ///   - Example: `"myuser:mypass@proxy.example.com:1080"`
    ///
    /// **Note**: If the proxy hostname fails to resolve, the upstream will
    /// not be created and an error will be logged during initialization.
    ///
    /// # IPv6 Support
    /// IPv6 addresses must be enclosed in brackets:
    /// - `"[::1]:1080"` - IPv6 without auth
    /// - `"user:pass@[2001:db8::1]:1080"` - IPv6 with auth
    pub socks5: Option<String>,

    /// Connection idle timeout in seconds
    ///
    /// Used by connection pools to recycle idle connections and bound
    /// long-lived unused sockets.
    ///
    /// The value accepts a duration string or a number. When a bare number is
    /// provided, it is interpreted as seconds.
    ///
    /// Examples:
    /// - `"5s"`
    /// - `"5"` // equivalent to `"5s"`
    #[serde(default, deserialize_with = "deserialize_duration_option")]
    pub idle_timeout: Option<Duration>,

    /// Maximum number of connections in the pool
    ///
    /// Used as the pool size upper bound to limit per-upstream resource usage.
    pub max_conns: Option<usize>,

    /// Minimum number of connections to keep warm in the pool
    ///
    /// Defaults to 0, which preserves lazy connection creation.
    pub min_conns: Option<usize>,

    /// Skip TLS certificate verification (**INSECURE**, testing only!)
    ///
    /// When `true`, disables certificate validation for TLS/QUIC/DoH
    /// connections. **Security Warning**: This makes connections vulnerable
    /// to MITM attacks. Only use for testing or with self-signed
    /// certificates you trust.
    pub insecure_skip_verify: Option<bool>,

    /// DNS query timeout duration
    ///
    /// Maximum time to wait for a DNS response before considering the query
    /// failed.
    ///
    /// The value accepts a duration string or a number. When a bare number is
    /// provided, it is interpreted as seconds.
    ///
    /// Defaults to 5 seconds if not specified.
    ///
    /// Examples:
    /// - `"5s"`
    /// - `"5"` // equivalent to `"5s"`
    #[serde(default, deserialize_with = "deserialize_duration_option")]
    pub timeout: Option<Duration>,

    /// Enable request pipelining for TCP/DoT connections
    ///
    /// When `true`, allows multiple concurrent queries over a single TCP
    /// connection. When `false`, uses connection pooling with one query per
    /// connection. Only applicable to TCP and DoT protocols.
    pub enable_pipeline: Option<bool>,

    /// Enable HTTP/3 for DoH connections
    ///
    /// When `true`, uses HTTP/3 (QUIC) instead of HTTP/2 for DoH.
    /// Requires the upstream server to support HTTP/3.
    pub enable_http3: Option<bool>,

    /// Linux SO_MARK socket option for policy routing
    ///
    /// Sets the mark on outgoing packets, which can be used with
    /// iptables/nftables for advanced routing policies.
    /// **Linux only** - ignored on other platforms.
    pub so_mark: Option<u32>,

    /// Linux SO_BINDTODEVICE - bind socket to specific network interface
    ///
    /// Forces the socket to use a specific network interface (e.g., "eth0",
    /// "wlan0"). Useful for multi-homed systems or VPN scenarios.
    /// **Linux only** - ignored on other platforms.
    pub bind_to_device: Option<String>,
}

/// Runtime connection information for upstream DNS servers
///
/// Parsed and processed configuration ready for connection establishment.
/// Created from `UpstreamConfig` via `From` trait, passed to connection
/// builders.
///
/// Thread-safe (`Clone`) for sharing across multiple connection instances.
#[derive(Debug, Clone)]
#[allow(unused)]
pub struct ConnectionInfo {
    /// Optional tag for identifying this upstream in logs
    pub tag: Option<String>,

    /// Protocol type (auto-detected from URL scheme: udp://, tcp://, tls://, quic://, https://)
    pub connection_type: ConnectionType,

    /// Original address string from configuration (for logging)
    pub raw_addr: String,

    /// Literal or explicitly configured IP address (`None` if hostname
    /// resolution is deferred to bootstrap or connection time)
    pub remote_ip: Option<IpAddr>,

    /// Server port (protocol default or explicitly configured)
    pub port: u16,

    /// SOCKS5 proxy configuration
    pub socks5: Option<Socks5Opt>,

    /// Bootstrap resolver for dynamic hostname resolution with TTL caching
    pub(crate) bootstrap: Option<Arc<NameResolver>>,

    /// Timeout to apply when the bootstrap resolver was injected from outbound.
    pub(crate) bootstrap_timeout: Option<Duration>,

    /// DoH request path (e.g., `/dns-query`), empty for non-HTTP protocols
    pub path: String,

    /// Server hostname for TLS SNI and certificate validation
    pub server_name: String,

    /// Skip TLS certificate verification (**INSECURE** - testing only)
    pub insecure_skip_verify: bool,

    /// Connection idle timeout in seconds
    pub idle_timeout: Duration,

    /// Maximum number of connections in the pool
    pub max_conns: Option<usize>,

    /// Minimum number of connections to keep warm in the pool
    pub min_conns: Option<usize>,

    /// DNS query timeout (includes I/O, handshakes, and round-trip time)
    pub timeout: Duration,

    /// Request pipelining for TCP/DoT (`None` = protocol default)
    pub enable_pipeline: Option<bool>,

    /// Use HTTP/3 (true) instead of HTTP/2 (false) for DoH
    pub enable_http3: bool,

    /// Linux SO_MARK for packet marking (policy routing)
    pub so_mark: Option<u32>,

    /// Linux SO_BINDTODEVICE - bind to specific network interface
    pub bind_to_device: Option<String>,
}

impl ConnectionInfo {
    pub(crate) const DEFAULT_CONN_IDLE_TIME: Duration = Duration::from_secs(10);
    pub(crate) const DEFAULT_MAX_CONNS_LOAD: u16 = 64;
    pub(crate) const DEFAULT_MAX_CONNS_SIZE: usize = 64;
    pub(crate) const DEFAULT_MIN_CONNS_SIZE: usize = 0;
    pub(crate) const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
    pub(crate) const MAX_CONFIGURED_CONNS_SIZE: usize = 4096;

    pub fn with_addr(addr: &str) -> Result<Self> {
        let (connection_type, host, port, path, _) = detect_connection_type(addr)?;
        let port = port.unwrap_or(connection_type.default_port());

        debug!(
            "Building ConnectionInfo: type={:?}, host={}, port={}, path={}",
            connection_type, host, port, path
        );

        let remote_ip = static_remote_ip_from_host(&host, None);

        Ok(ConnectionInfo {
            tag: None,
            remote_ip,
            port,
            socks5: None,
            connection_type,
            bootstrap: None,
            bootstrap_timeout: None,
            path,
            timeout: Self::DEFAULT_QUERY_TIMEOUT,
            server_name: host,
            insecure_skip_verify: false,
            idle_timeout: Self::DEFAULT_CONN_IDLE_TIME,
            raw_addr: addr.to_string(),
            enable_pipeline: None,
            enable_http3: false,
            so_mark: None,
            bind_to_device: None,
            max_conns: None,
            min_conns: None,
        })
    }

    pub fn validate_addr(addr: &str) -> Result<()> {
        detect_connection_type(addr).map(|_| ())
    }

    pub(crate) fn max_conns_or_default(&self) -> usize {
        self.max_conns.unwrap_or(Self::DEFAULT_MAX_CONNS_SIZE)
    }

    pub(crate) fn min_conns_or_default(&self) -> usize {
        self.min_conns.unwrap_or(Self::DEFAULT_MIN_CONNS_SIZE)
    }
}

impl TryFrom<UpstreamConfig> for ConnectionInfo {
    type Error = DnsError;

    fn try_from(upstream_config: UpstreamConfig) -> Result<Self> {
        let UpstreamConfig {
            tag,
            addr,
            outbound: outbound_ref,
            dial_addr,
            port: config_port,
            bootstrap,
            bootstrap_version,
            socks5,
            idle_timeout,
            max_conns,
            min_conns,
            insecure_skip_verify,
            timeout,
            enable_pipeline,
            enable_http3,
            so_mark,
            bind_to_device,
        } = upstream_config;
        let (connection_type, host, port, path, helper_flags) = detect_connection_type(&addr)?;
        let enable_pipeline = if helper_flags.force_pipeline {
            Some(true)
        } else {
            enable_pipeline
        };
        let enable_http3 = if helper_flags.force_http3 {
            true
        } else {
            enable_http3.unwrap_or(false)
        };
        let port = config_port
            .or(port)
            .unwrap_or(connection_type.default_port());

        if let Some(max_conns) = max_conns {
            if max_conns == 0 {
                return Err(DnsError::plugin(
                    "upstream max_conns must be greater than 0",
                ));
            }
            if max_conns > ConnectionInfo::MAX_CONFIGURED_CONNS_SIZE {
                return Err(DnsError::plugin(format!(
                    "upstream max_conns must be <= {}",
                    ConnectionInfo::MAX_CONFIGURED_CONNS_SIZE
                )));
            }
        }
        if let Some(min_conns) = min_conns {
            if min_conns > ConnectionInfo::MAX_CONFIGURED_CONNS_SIZE {
                return Err(DnsError::plugin(format!(
                    "upstream min_conns must be <= {}",
                    ConnectionInfo::MAX_CONFIGURED_CONNS_SIZE
                )));
            }

            let effective_max_conns = max_conns.unwrap_or(ConnectionInfo::DEFAULT_MAX_CONNS_SIZE);
            if min_conns > effective_max_conns {
                return Err(DnsError::plugin(format!(
                    "upstream min_conns must be <= max_conns (effective max_conns: {})",
                    effective_max_conns
                )));
            }
        }
        if !matches!(bootstrap_version, None | Some(4) | Some(6)) {
            return Err(DnsError::plugin(
                "upstream bootstrap_version must be 4 or 6",
            ));
        }

        debug!(
            "Building ConnectionInfo: type={:?}, host={}, port={}, path={}",
            connection_type, &host, port, path
        );

        let outbound_policy = match outbound_ref.as_deref().map(str::trim) {
            Some("") => {
                return Err(DnsError::plugin(
                    "upstream outbound profile cannot be empty",
                ));
            }
            Some(name) => Some(outbound::global().resolve_policy(Some(name), None)?),
            None => Some(outbound::global().resolve_policy(None, None)?),
        };

        let dial_addr_configured = dial_addr.is_some();
        let remote_ip = static_remote_ip_from_host(&host, dial_addr);

        if dial_addr_configured && bootstrap.is_some() {
            warn!(
                upstream = %addr,
                "Both dial_addr and bootstrap are configured; dial_addr takes precedence and bootstrap will be ignored"
            );
        }

        let (bootstrap, bootstrap_timeout) = if remote_ip.is_none() {
            if let Some(bootstrap_server) = bootstrap {
                (
                    Some(Arc::new(NameResolver::new(
                        vec![bootstrap_server],
                        bootstrap_version,
                    )?)),
                    None,
                )
            } else {
                outbound_policy
                    .as_ref()
                    .and_then(|policy| policy.resolver())
                    .map_or((None, None), |(resolver, timeout)| {
                        (Some(resolver), Some(timeout))
                    })
            }
        } else {
            (None, None)
        };

        let raw_socks5 = if let Some(socks5_str) = socks5.as_deref() {
            Some((
                parse_socks5_opt(socks5_str).ok_or_else(|| {
                    DnsError::plugin(format!("upstream has invalid socks5 proxy '{socks5_str}'"))
                })?,
                ProxySource::Local,
            ))
        } else {
            outbound_policy
                .as_ref()
                .and_then(|policy| policy.proxy())
                .map(|socks5| (socks5, ProxySource::Profile))
        };
        let socks5 = if let Some((socks5_opt, proxy_source)) = raw_socks5 {
            match connection_type {
                ConnectionType::TCP | ConnectionType::DoT => Some(socks5_opt),
                ConnectionType::DoH => {
                    if enable_http3 {
                        if proxy_source == ProxySource::Profile {
                            return Err(DnsError::plugin(
                                "upstream outbound profile proxy does not support DoH3",
                            ));
                        }
                        warn!("Sock5 proxy only support tcp portal");
                        None
                    } else {
                        Some(socks5_opt)
                    }
                }
                _ => {
                    if proxy_source == ProxySource::Profile {
                        return Err(DnsError::plugin(format!(
                            "upstream outbound profile proxy does not support {:?}",
                            connection_type
                        )));
                    }
                    warn!("Sock5 proxy only support tcp portal");
                    None
                }
            }
        } else {
            None
        };

        Ok(ConnectionInfo {
            tag,
            remote_ip,
            port,
            socks5,
            connection_type,
            bootstrap,
            bootstrap_timeout,
            path,
            timeout: timeout.unwrap_or(Self::DEFAULT_QUERY_TIMEOUT),
            server_name: host,
            insecure_skip_verify: insecure_skip_verify.unwrap_or(false),
            idle_timeout: idle_timeout.unwrap_or(Self::DEFAULT_CONN_IDLE_TIME),
            raw_addr: addr,
            enable_pipeline,
            enable_http3,
            so_mark,
            bind_to_device,
            max_conns,
            min_conns,
        })
    }
}

/// Determine the startup-known remote IP address.
///
/// # Arguments
/// - `host`: The hostname or IP address string
/// - `dial_addr`: Optional pre-configured IP address to use directly
///
/// # Returns
/// `Some(IpAddr)` if an IP address is explicitly configured or present
/// literally in `host`; `None` for hostnames. Hostname resolution is deferred
/// to bootstrap or connection creation so startup and config validation do not
/// depend on the local system resolver.
fn static_remote_ip_from_host(host: &str, dial_addr: Option<IpAddr>) -> Option<IpAddr> {
    // 1. Use dial_addr if provided
    if let Some(ip) = dial_addr {
        return Some(ip);
    }

    // 2. Try parsing as IP address
    if let Ok(ip) = IpAddr::from_str(host) {
        return Some(ip);
    }

    None
}

/// Detect the connection type from the config address
#[derive(Clone, Copy, Debug, Default)]
struct HelperFlags {
    force_pipeline: bool,
    force_http3: bool,
}

fn detect_connection_type(
    addr: &str,
) -> Result<(ConnectionType, String, Option<u16>, String, HelperFlags)> {
    if !addr.contains("//") {
        return detect_connection_type(&("udp://".to_owned() + addr));
    }

    let url =
        Url::parse(addr).map_err(|e| DnsError::plugin(format!("invalid upstream URL: {}", e)))?;
    let mut helper_flags = HelperFlags::default();
    let connection_type;

    let host = url
        .host_str()
        .map(|host| host.to_owned())
        .ok_or_else(|| DnsError::plugin("invalid upstream URL: no host specified"))?;

    match url.scheme() {
        "udp" => {
            connection_type = ConnectionType::UDP;
        }
        "tcp" => {
            connection_type = ConnectionType::TCP;
        }
        "tcp+pipeline" => {
            helper_flags.force_pipeline = true;
            connection_type = ConnectionType::TCP;
        }
        "tls" => {
            connection_type = ConnectionType::DoT;
        }
        "tls+pipeline" => {
            helper_flags.force_pipeline = true;
            connection_type = ConnectionType::DoT;
        }
        "quic" | "doq" => {
            connection_type = ConnectionType::DoQ;
        }
        "https" | "doh" => {
            connection_type = ConnectionType::DoH;
        }
        "h3" => {
            helper_flags.force_http3 = true;
            connection_type = ConnectionType::DoH;
        }
        other => {
            return Err(DnsError::plugin(format!(
                "invalid upstream URL scheme: {}",
                other
            )));
        }
    };

    debug!(
        "Detected upstream: scheme={}, type={:?}, host={}, port={:?}, path={}",
        url.scheme(),
        connection_type,
        host,
        url.port(),
        url.path()
    );

    Ok((
        connection_type,
        host,
        url.port(),
        url.path().to_string(),
        helper_flags,
    ))
}
