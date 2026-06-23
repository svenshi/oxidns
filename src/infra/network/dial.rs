// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Low-level outbound connection helpers.
//!
//! Provides helper functions for:
//! - connected UDP socket setup;
//! - direct TCP connection establishment;
//! - TLS client handshakes over established TCP streams;
//! - QUIC client handshakes over connected UDP sockets.

use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
#[cfg(any(
    feature = "_tls-client",
    feature = "_dns-client-doq",
    feature = "_dns-client-doh3"
))]
use std::sync::Arc;
#[cfg(any(
    feature = "_tls-client",
    feature = "_dns-client-doq",
    feature = "_dns-client-doh3"
))]
use std::time::Duration;

#[cfg(any(feature = "_dns-client-doq", feature = "_dns-client-doh3"))]
use quinn::crypto::rustls::QuicClientConfig;
#[cfg(any(feature = "_dns-client-doq", feature = "_dns-client-doh3"))]
use quinn::{ClientConfig, Endpoint, EndpointConfig, TokioRuntime, TransportConfig, VarInt};
#[cfg(feature = "_tls-client")]
use rustls::pki_types::ServerName;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::TcpStream;
#[cfg(any(
    feature = "_tls-client",
    feature = "_dns-client-doq",
    feature = "_dns-client-doh3"
))]
use tokio::time::timeout;
#[cfg(feature = "_tls-client")]
use tokio_rustls::TlsConnector;
#[cfg(feature = "_tls-client")]
use tokio_rustls::client::TlsStream;
use tracing::info;

use crate::infra::error::{DnsError, Result};
#[cfg(feature = "_tls-client")]
use crate::infra::network::tls_config::{insecure_client_config, secure_client_config};

/// Remote endpoint information shared by UDP, TCP, TLS, and QUIC dialing.
#[derive(Clone, Debug)]
pub(crate) struct DialTarget {
    remote_ip: Option<IpAddr>,
    host: String,
    port: u16,
}

impl DialTarget {
    pub(crate) fn new(remote_ip: Option<IpAddr>, host: String, port: u16) -> Self {
        Self {
            remote_ip,
            host,
            port,
        }
    }

    pub(crate) fn from_socket_addr(socket_addr: SocketAddr) -> Self {
        Self {
            remote_ip: Some(socket_addr.ip()),
            host: socket_addr.ip().to_string(),
            port: socket_addr.port(),
        }
    }

    pub(crate) fn remote_ip(&self) -> Option<IpAddr> {
        self.remote_ip
    }

    pub(crate) fn host(&self) -> &str {
        self.host.as_str()
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    fn socket_addr(&self) -> Result<SocketAddr> {
        let ip = match self.remote_ip {
            Some(ip) => ip,
            None => try_lookup_server_name(&self.host)?,
        };
        Ok(SocketAddr::new(ip, self.port))
    }

    #[cfg(any(
        feature = "_tls-client",
        feature = "_dns-client-doq",
        feature = "_dns-client-doh3"
    ))]
    fn server_name(&self) -> &str {
        self.host.as_str()
    }
}

/// Socket-level controls common to outbound UDP and TCP dials.
#[derive(Clone, Debug, Default)]
pub(crate) struct SocketOptions {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    so_mark: Option<u32>,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    bind_to_device: Option<String>,
}

impl SocketOptions {
    pub(crate) fn new(so_mark: Option<u32>, bind_to_device: Option<String>) -> Self {
        Self {
            so_mark,
            bind_to_device,
        }
    }

    #[cfg(test)]
    pub(crate) fn so_mark(&self) -> Option<u32> {
        self.so_mark
    }

    #[cfg(test)]
    pub(crate) fn bind_to_device(&self) -> Option<&str> {
        self.bind_to_device.as_deref()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct UdpDialOptions {
    target: DialTarget,
    socket: SocketOptions,
}

impl UdpDialOptions {
    pub(crate) fn new(target: DialTarget, socket: SocketOptions) -> Self {
        Self { target, socket }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TcpDialOptions {
    target: DialTarget,
    socket: SocketOptions,
}

impl TcpDialOptions {
    pub(crate) fn new(target: DialTarget) -> Self {
        Self {
            target,
            socket: SocketOptions::default(),
        }
    }

    pub(crate) fn with_socket_options(mut self, socket: SocketOptions) -> Self {
        self.socket = socket;
        self
    }
}

#[cfg(feature = "_tls-client")]
#[derive(Clone, Debug)]
pub(crate) struct TlsDialOptions {
    target: DialTarget,
    skip_cert: bool,
    handshake_timeout: Duration,
    alpn: Vec<Vec<u8>>,
}

#[cfg(feature = "_tls-client")]
#[allow(dead_code)]
impl TlsDialOptions {
    pub(crate) fn new(
        target: DialTarget,
        skip_cert: bool,
        handshake_timeout: Duration,
        alpn: Vec<Vec<u8>>,
    ) -> Self {
        Self {
            target,
            skip_cert,
            handshake_timeout,
            alpn,
        }
    }
}

#[cfg(any(feature = "_dns-client-doq", feature = "_dns-client-doh3"))]
#[derive(Clone, Debug)]
pub(crate) struct QuicDialOptions {
    target: DialTarget,
    skip_cert: bool,
    handshake_timeout: Duration,
    idle_timeout: Duration,
    alpn: Vec<Vec<u8>>,
}

#[cfg(any(feature = "_dns-client-doq", feature = "_dns-client-doh3"))]
impl QuicDialOptions {
    pub(crate) fn new(
        target: DialTarget,
        skip_cert: bool,
        handshake_timeout: Duration,
        idle_timeout: Duration,
        alpn: Vec<Vec<u8>>,
    ) -> Self {
        Self {
            target,
            skip_cert,
            handshake_timeout,
            idle_timeout,
            alpn,
        }
    }
}

/// Establish a TLS client connection over an existing TCP stream.
///
/// Performs TLS 1.2/1.3 handshake with configurable certificate verification.
///
/// # Arguments
/// * `tcp_stream` - Established TCP connection to upgrade to TLS.
/// * `options` - Target name, certificate policy, handshake timeout, and ALPN.
///
/// # Returns
/// - `Ok(TlsStream)` if TLS handshake succeeds
/// - `Err(DnsError)` if handshake fails or times out
///
/// # Security Warning
/// Setting `skip_cert` to true disables certificate validation and makes the
/// connection vulnerable to man-in-the-middle attacks. Only use this for
/// testing!
#[cfg(feature = "_tls-client")]
#[allow(dead_code)]
#[inline]
pub(crate) async fn connect_tls(
    tcp_stream: TcpStream,
    options: TlsDialOptions,
) -> Result<TlsStream<TcpStream>> {
    let mut config = if options.skip_cert {
        insecure_client_config()
    } else {
        secure_client_config()
    };
    config.alpn_protocols = options.alpn;

    let connector = TlsConnector::from(Arc::new(config));
    let dns_name = ServerName::try_from(options.target.server_name().to_string())
        .map_err(|_| DnsError::protocol("Invalid DNS server name"))?;

    match timeout(
        options.handshake_timeout,
        connector.connect(dns_name, tcp_stream),
    )
    .await
    {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(DnsError::protocol(format!("TLS connection error: {}", e))),
        Err(_) => Err(DnsError::protocol("TLS handshake timeout")),
    }
}

/// Establish a QUIC client connection over an existing connected UDP socket.
///
/// Creates a QUIC endpoint from the provided UDP socket and performs the
/// QUIC+TLS 1.3 handshake with the remote endpoint.
///
/// # Arguments
/// * `udp_socket` - Pre-configured UDP socket already connected to the remote
///   endpoint.
/// * `options` - Target name, certificate policy, handshake timeout, idle
///   timeout, and ALPN.
///
/// # Returns
/// - `Ok(quinn::Connection)` if QUIC handshake succeeds
/// - `Err(DnsError)` if handshake fails, times out, or configuration is invalid
///
/// # Protocol
/// - Uses QUIC with mandatory TLS 1.3.
/// - Supports ALPN negotiation using the caller-provided protocols.
///
/// # Security Warning
/// Setting `skip_cert` to true disables certificate validation. Only use for
/// testing!
#[cfg(any(feature = "_dns-client-doq", feature = "_dns-client-doh3"))]
pub(crate) async fn connect_quic(
    udp_socket: UdpSocket,
    options: QuicDialOptions,
) -> Result<quinn::Connection> {
    let remote_addr = udp_socket.peer_addr()?;
    let mut endpoint = Endpoint::new(
        EndpointConfig::default(),
        None,
        udp_socket,
        Arc::new(TokioRuntime),
    )?;

    let mut client_config = if options.skip_cert {
        insecure_client_config()
    } else {
        secure_client_config()
    };
    client_config.alpn_protocols = options.alpn;

    let idle_ms = options.idle_timeout.as_millis().min(u32::MAX as u128) as u32;
    let mut transport = TransportConfig::default();
    transport.max_idle_timeout(Some(VarInt::from_u32(idle_ms).into()));

    let mut client_config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(client_config)?));
    client_config.transport_config(Arc::new(transport));

    endpoint.set_default_client_config(client_config);

    match timeout(
        options.handshake_timeout,
        endpoint.connect(remote_addr, options.target.server_name())?,
    )
    .await
    {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(DnsError::protocol(format!("QUIC connection error: {}", e))),
        Err(_) => Err(DnsError::protocol("QUIC handshake timeout")),
    }
}

/// Resolve hostname to IP address using system DNS
///
/// Uses the operating system's DNS resolver (e.g., getaddrinfo on Unix/Linux).
/// This is a blocking operation that uses the system's configured DNS servers.
///
/// # Arguments
/// * `server_name` - Hostname to resolve (e.g., "dns.example.com")
///
/// # Returns
/// - `Ok(IpAddr)` with the first resolved IP address
/// - `Err(DnsError)` if resolution fails or returns no results
///
/// # Notes
/// - This is used at connection time when no literal IP, `dial_addr`, or
///   bootstrap-resolved address is available
/// - For dynamic resolution with TTL support, use Bootstrap instead
/// - Blocks the current task - consider using bootstrap for async resolution
/// - Returns the first address from the system resolver (maybe IPv4 or IPv6)
///
/// # Platform Behavior
/// - Unix/Linux: Uses getaddrinfo() respecting /etc/resolv.conf and /etc/hosts
/// - macOS: May use mDNSResponder
/// - Windows: Uses the Windows DNS Client service
pub fn try_lookup_server_name(server_name: &str) -> Result<IpAddr> {
    match format!("{}:0", server_name).to_socket_addrs() {
        Ok(mut addrs) => match addrs.next() {
            Some(addr) => {
                let ip = addr.ip();
                info!(
                    server_name = %server_name,
                    resolved_ip = %ip,
                    ip_version = if ip.is_ipv4() { "IPv4" } else { "IPv6" },
                    "Resolved hostname using system DNS (one-time, permanent cache)"
                );
                Ok(ip)
            }
            None => Err(DnsError::protocol(format!(
                "System DNS returned no addresses for '{}'",
                server_name
            ))),
        },
        Err(e) => Err(DnsError::protocol(format!(
            "System DNS resolution failed for '{}': {}",
            server_name, e
        ))),
    }
}

/// Create and configure a connected UDP socket.
///
/// Creates a non-blocking UDP socket with optional Linux-specific socket
/// options (SO_MARK, SO_BINDTODEVICE) and connects it to the target endpoint.
///
/// # Arguments
/// * `options` - Target endpoint and socket-level controls.
///
/// # Returns
/// - `Ok(UdpSocket)` connected UDP socket in non-blocking mode
/// - `Err(DnsError)` if socket creation, configuration, or connection fails
///
/// # Platform-Specific Features
/// - **Linux**: Supports SO_MARK (for netfilter/policy routing) and
///   SO_BINDTODEVICE
/// - **Other platforms**: SO_MARK and bind_to_device options are ignored
///
/// # Notes
/// - Socket is set to non-blocking mode for async I/O
/// - SO_REUSEADDR is enabled to allow rapid reconnection
/// - connect() is called to set the default destination (allows using send vs
///   send_to)
pub(crate) fn connect_udp(options: UdpDialOptions) -> Result<UdpSocket> {
    let socket_addr = options.target.socket_addr()?;
    let socket = Socket::new(
        Domain::for_address(socket_addr),
        Type::DGRAM,
        Some(Protocol::UDP),
    )?;

    configure_common_socket(&socket, &options.socket)?;
    #[cfg(all(
        unix,
        not(any(
            target_os = "solaris",
            target_os = "illumos",
            target_os = "cygwin",
            target_os = "wasi"
        ))
    ))]
    let _ = socket.set_reuse_port(true);
    let _ = socket.set_recv_buffer_size(64 * 1024);

    socket.connect(&socket_addr.into())?;

    Ok(socket.into())
}

fn create_tcp_socket(socket_addr: SocketAddr, options: &SocketOptions) -> Result<Socket> {
    let socket = Socket::new(
        Domain::for_address(socket_addr),
        Type::STREAM,
        Some(Protocol::TCP),
    )?;

    configure_common_socket(&socket, options)?;
    let _ = socket.set_tcp_nodelay(true);
    Ok(socket)
}

fn configure_common_socket(socket: &Socket, options: &SocketOptions) -> Result<()> {
    let _ = options;
    let _ = socket.set_nonblocking(true);
    let _ = socket.set_reuse_address(true);

    #[cfg(target_os = "linux")]
    if let Some(so_mark) = options.so_mark {
        socket.set_mark(so_mark)?;
    }

    #[cfg(target_os = "linux")]
    if let Some(device) = options.bind_to_device.as_ref() {
        socket.bind_device(Some(device.as_bytes()))?;
    }

    Ok(())
}

async fn connect_tcp_socket(socket: Socket, socket_addr: SocketAddr) -> Result<TcpStream> {
    match socket.connect(&socket_addr.into()) {
        Ok(()) => {}
        Err(e) if is_connect_in_progress(&e) => {}
        Err(e) => return Err(e.into()),
    }

    let std_stream: std::net::TcpStream = socket.into();
    let stream = TcpStream::from_std(std_stream)?;

    // Disable Nagle's algorithm. DNS-over-TCP exchanges tiny request/response
    // frames; with Nagle enabled the kernel coalesces them and interacts badly
    // with the peer's delayed ACK, adding tens of milliseconds per query.
    let _ = stream.set_nodelay(true);

    // Ensure the async connect has completed before the stream is used by higher
    // layers.
    stream.writable().await?;
    if let Some(err) = stream.take_error()? {
        return Err(err.into());
    }

    Ok(stream)
}

fn is_connect_in_progress(err: &std::io::Error) -> bool {
    if err.kind() == ErrorKind::WouldBlock {
        return true;
    }

    #[cfg(unix)]
    {
        err.raw_os_error() == Some(libc::EINPROGRESS)
    }

    #[cfg(not(unix))]
    {
        false
    }
}

/// Create and configure a direct TCP stream.
///
/// Creates a non-blocking TCP socket with TCP_NODELAY enabled and optional
/// Linux-specific socket options, then connects to the target endpoint.
///
/// # Arguments
/// * `options` - Target endpoint and socket-level controls.
///
/// # Returns
/// - `Ok(TcpStream)` connected TCP stream (async, non-blocking mode)
/// - `Err(DnsError)` if socket creation, configuration, or connection fails
///
/// # Socket Configuration
/// - **TCP_NODELAY**: Enabled to disable Nagle's algorithm for low-latency
///   request/response protocols.
/// - **SO_REUSEADDR**: Enabled to allow rapid reconnection
/// - **Non-blocking**: Set for async I/O compatibility
///
/// # Platform-Specific Features
/// - **Linux**: Supports SO_MARK and SO_BINDTODEVICE for advanced routing
/// - **Other platforms**: These options are silently ignored
///
/// # Performance
/// TCP_NODELAY keeps small request frames from waiting for more data before
/// being sent.
pub(crate) async fn connect_tcp(options: TcpDialOptions) -> Result<TcpStream> {
    let socket_addr = options.target.socket_addr()?;
    let socket = create_tcp_socket(socket_addr, &options.socket)?;
    connect_tcp_socket(socket, socket_addr).await
}
