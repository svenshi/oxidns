// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Utility functions for connection pooling
//!
//! Provides helper functions for:
//! - TLS connection establishment
//! - QUIC connection setup
//! - DoH request construction
//! - Connection cleanup

use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
#[cfg(any(
    feature = "_tls-client",
    feature = "upstream-doq",
    feature = "upstream-doh3"
))]
use std::time::Duration;

use fast_socks5::client::Socks5Stream;
#[cfg(any(feature = "upstream-doq", feature = "upstream-doh3"))]
use quinn::crypto::rustls::QuicClientConfig;
#[cfg(any(feature = "upstream-doq", feature = "upstream-doh3"))]
use quinn::{ClientConfig, Endpoint, EndpointConfig, TokioRuntime, TransportConfig, VarInt};
#[cfg(feature = "_tls-client")]
use rustls::pki_types::ServerName;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::TcpStream;
#[cfg(any(
    feature = "_tls-client",
    feature = "upstream-doq",
    feature = "upstream-doh3"
))]
use tokio::time::timeout;
#[cfg(feature = "_tls-client")]
use tokio_rustls::TlsConnector;
#[cfg(feature = "_tls-client")]
use tokio_rustls::client::TlsStream;
use tracing::info;

use crate::core::error::{DnsError, Result};
use crate::network::proxy::Socks5Opt;
#[cfg(feature = "_tls-client")]
use crate::network::tls_config::{insecure_client_config, secure_client_config};
use crate::network::upstream::pool::{Connection, DeadlineOutcome, QueryDeadline};

/// Establish TLS connection over an existing TCP stream
///
/// Performs TLS 1.2/1.3 handshake with configurable certificate verification.
///
/// # Arguments
/// * `tcp_stream` - Established TCP connection to upgrade to TLS
/// * `skip_cert` - If true, skip certificate validation (**INSECURE** -
///   testing/debug only!)
/// * `server_name` - SNI (Server Name Indication) hostname for TLS handshake
/// * `conn_timeout` - Maximum time to wait for TLS handshake to complete
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
    skip_cert: bool,
    server_name: String,
    conn_timeout: Duration,
    alpn: Vec<Vec<u8>>,
) -> Result<TlsStream<TcpStream>> {
    let mut config = if skip_cert {
        insecure_client_config()
    } else {
        secure_client_config()
    };
    config.alpn_protocols = alpn;

    let connector = TlsConnector::from(Arc::new(config));
    let dns_name = ServerName::try_from(server_name)
        .map_err(|_| DnsError::protocol("Invalid DNS server name"))?;

    match timeout(conn_timeout, connector.connect(dns_name, tcp_stream)).await {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(DnsError::protocol(format!("TLS connection error: {}", e))),
        Err(_) => Err(DnsError::protocol("TLS handshake timeout")),
    }
}

/// Establish QUIC connection for DoQ (DNS over QUIC, RFC 9250)
///
/// Creates a QUIC endpoint from the provided UDP socket and performs the
/// QUIC+TLS 1.3 handshake with the remote DNS server.
///
/// # Arguments
/// * `udp_socket` - Pre-configured UDP socket (already connected to remote)
/// * `skip_cert` - If true, skip certificate validation (**INSECURE** - testing
///   only!)
/// * `server_name` - SNI (Server Name Indication) hostname for TLS 1.3
///   handshake
/// * `conn_timeout` - Maximum time to wait for QUIC handshake to complete
///
/// # Returns
/// - `Ok(quinn::Connection)` if QUIC handshake succeeds
/// - `Err(DnsError)` if handshake fails, times out, or configuration is invalid
///
/// # Protocol
/// - Uses QUIC with mandatory TLS 1.3 (per RFC 9250)
/// - Supports 0-RTT for resumed connections
/// - Includes ALPN negotiation for "doq" protocol
///
/// # Security Warning
/// Setting `skip_cert` to true disables certificate validation. Only use for
/// testing!
#[cfg(any(feature = "upstream-doq", feature = "upstream-doh3"))]
pub(crate) async fn connect_quic(
    udp_socket: UdpSocket,
    skip_cert: bool,
    server_name: String,
    handshake_timeout: Duration,
    idle_timeout: Duration,
    alpn: Vec<Vec<u8>>,
) -> Result<quinn::Connection> {
    let remote_addr = udp_socket.peer_addr()?;
    let mut endpoint = Endpoint::new(
        EndpointConfig::default(),
        None,
        udp_socket,
        Arc::new(TokioRuntime),
    )?;

    let mut client_config = if skip_cert {
        insecure_client_config()
    } else {
        secure_client_config()
    };
    client_config.alpn_protocols = alpn;

    // Set QUIC idle timeout to 3× the configured query timeout. Without this,
    // zombie connections (server stops responding but never sends
    // CONNECTION_CLOSE) are never detected by the QUIC layer, and
    // send_request() / open_bi() block forever. With idle timeout, the QUIC
    // stack closes the connection and the H3/DoQ driver task calls
    // conn.close(), letting the pool replace it.
    let idle_ms = (idle_timeout.as_millis() * 3).min(u32::MAX as u128) as u32;
    let mut transport = TransportConfig::default();
    transport.max_idle_timeout(Some(VarInt::from_u32(idle_ms).into()));

    let mut client_config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(client_config)?));
    client_config.transport_config(Arc::new(transport));

    endpoint.set_default_client_config(client_config);

    match timeout(
        handshake_timeout,
        endpoint.connect(remote_addr, &server_name)?,
    )
    .await
    {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(DnsError::protocol(format!("QUIC connection error: {}", e))),
        Err(_) => Err(DnsError::protocol("QUIC handshake timeout")),
    }
}

/// Close multiple connections synchronously
///
/// Iterates through the connection vector and calls `close()` on each.
/// This is a convenience function for bulk connection cleanup.
///
/// # Arguments
/// * `conns` - Vector of Arc-wrapped connections to close
///
/// # Notes
/// - close() is idempotent, so calling this multiple times is safe
/// - close() is synchronous, so this function doesn't need to be async
/// - Connections are not removed from the vector, just marked as closed
#[inline]
pub fn close_conns<C: Connection>(conns: &Vec<Arc<C>>) {
    for conn in conns {
        conn.close();
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

/// Create and configure a UDP socket for DNS communication
///
/// Creates a non-blocking UDP socket with optional Linux-specific socket
/// options (SO_MARK, SO_BINDTODEVICE) and connects it to the remote DNS server.
///
/// # Arguments
/// * `remote_ip` - Remote server IP address (if None, resolves server_name)
/// * `server_name` - Hostname to resolve if remote_ip is None
/// * `port` - Remote server port
/// * `so_mark` - Linux SO_MARK socket option (for policy routing)
/// * `bind_to_device` - Linux SO_BINDTODEVICE option (bind to specific
///   interface)
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
#[allow(unused)]
pub fn connect_socket(
    remote_ip: Option<IpAddr>,
    server_name: String,
    port: u16,
    so_mark: Option<u32>,
    bind_to_device: Option<String>,
) -> Result<UdpSocket> {
    // Resolve remote address if not provided
    let socket_addr = if let Some(remote_ip) = remote_ip {
        SocketAddr::new(remote_ip, port)
    } else {
        let addr = try_lookup_server_name(&server_name)?;
        SocketAddr::new(addr, port)
    };

    // Create UDP socket with appropriate address family
    let socket = Socket::new(
        Domain::for_address(socket_addr),
        Type::DGRAM,
        Some(Protocol::UDP),
    )?;

    // Configure socket for async I/O
    let _ = socket.set_nonblocking(true);
    let _ = socket.set_reuse_address(true);
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

    // Linux-specific socket options for advanced routing
    #[cfg(target_os = "linux")]
    if let Some(so_mark) = so_mark {
        socket.set_mark(so_mark)?;
    }

    #[cfg(target_os = "linux")]
    if let Some(device) = bind_to_device {
        socket.bind_device(Some(device.as_bytes()))?;
    }

    // Connect socket to set default destination (allows using send() instead of
    // send_to())
    socket.connect(&socket_addr.into())?;

    Ok(socket.into())
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
    // with the peer's delayed ACK, adding tens of milliseconds per query. This
    // is applied here so both direct and SOCKS5-proxy TCP paths are covered.
    let _ = stream.set_nodelay(true);

    // Ensure the async connect has completed before the stream is used by SOCKS/TLS
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

/// Create and configure a TCP stream for DNS communication
///
/// Creates a non-blocking TCP socket with TCP_NODELAY enabled and optional
/// Linux-specific socket options, then connects to the remote DNS server.
/// Supports SOCKS5 proxy with bind_device applied to the proxy connection.
///
/// # Arguments
/// * `remote_ip` - Remote server IP address (if None, resolves server_name)
/// * `server_name` - Hostname to resolve if remote_ip is None
/// * `port` - Remote server port
/// * `so_mark` - Linux SO_MARK socket option (for policy routing)
/// * `bind_to_device` - Linux SO_BINDTODEVICE option (bind to specific
///   interface)
/// * `socks5_opt` - Optional SOCKS5 proxy configuration
///
/// # Returns
/// - `Ok(TcpStream)` connected TCP stream (async, non-blocking mode)
/// - `Err(DnsError)` if socket creation, configuration, or connection fails
///
/// # Socket Configuration
/// - **TCP_NODELAY**: Enabled to disable Nagle's algorithm for low-latency DNS
///   queries
/// - **SO_REUSEADDR**: Enabled to allow rapid reconnection
/// - **Non-blocking**: Set for async I/O compatibility
///
/// # Platform-Specific Features
/// - **Linux**: Supports SO_MARK and SO_BINDTODEVICE for advanced routing
/// - **Other platforms**: These options are silently ignored
///
/// # SOCKS5 Support
/// When `socks5_opt` is provided:
/// - Creates connection to SOCKS5 proxy server
/// - Applies bind_device to the proxy connection (Linux only)
/// - Establishes SOCKS5 tunnel to the target server
/// - Supports username/password authentication
///
/// # Performance
/// TCP_NODELAY is critical for DNS-over-TCP performance, as it ensures
/// small DNS queries are sent immediately without waiting for more data
#[allow(unused)]
pub async fn connect_stream(
    remote_ip: Option<IpAddr>,
    server_name: String,
    port: u16,
    so_mark: Option<u32>,
    bind_to_device: Option<String>,
    socks5_opt: Option<Socks5Opt>,
) -> Result<TcpStream> {
    // If SOCKS5 proxy is configured, use it
    if let Some(socks5) = socks5_opt {
        // Create socket to SOCKS5 proxy server
        let socket = Socket::new(
            Domain::for_address(socks5.socket_addr),
            Type::STREAM,
            Some(Protocol::TCP),
        )?;

        // Configure socket for low-latency async I/O
        let _ = socket.set_nonblocking(true);
        let _ = socket.set_tcp_nodelay(true);
        let _ = socket.set_reuse_address(true);

        // Apply Linux-specific socket options to proxy connection
        #[cfg(target_os = "linux")]
        if let Some(so_mark) = so_mark {
            socket.set_mark(so_mark)?;
        }

        #[cfg(target_os = "linux")]
        if let Some(ref device) = bind_to_device {
            socket.bind_device(Some(device.as_bytes()))?;
        }

        let proxy_stream = connect_tcp_socket(socket, socks5.socket_addr).await?;

        // Establish SOCKS5 connection through proxy
        use fast_socks5::util::target_addr::TargetAddr;
        use fast_socks5::{AuthenticationMethod, Socks5Command};

        // Create authentication method
        let auth = if let (Some(username), Some(password)) =
            (socks5.username.as_ref(), socks5.password.as_ref())
        {
            Some(AuthenticationMethod::Password {
                username: username.clone(),
                password: password.clone(),
            })
        } else {
            None
        };

        // Standard SOCKS5 servers still require method negotiation for "no auth".
        let config = fast_socks5::client::Config::default();

        // Create SOCKS5 stream
        let mut socks5_stream = Socks5Stream::use_stream(proxy_stream, auth, config).await?;

        // Prepare target address
        let target_addr = if let Some(remote_ip) = remote_ip {
            TargetAddr::Ip(SocketAddr::new(remote_ip, port))
        } else {
            TargetAddr::Domain(server_name, port)
        };

        // Connect to target through SOCKS5
        socks5_stream
            .request(Socks5Command::TCPConnect, target_addr)
            .await?;

        // Get the underlying TcpStream
        let stream = socks5_stream.get_socket();

        // Enable TCP_NODELAY on the established SOCKS5 tunnel
        let _ = stream.set_nodelay(true);

        Ok(stream)
    } else {
        // Direct connection (no SOCKS5 proxy)
        let socket_addr = if let Some(remote_ip) = remote_ip {
            SocketAddr::new(remote_ip, port)
        } else {
            let addr = try_lookup_server_name(&server_name)?;
            SocketAddr::new(addr, port)
        };

        // Create TCP socket with appropriate address family
        let socket = Socket::new(
            Domain::for_address(socket_addr),
            Type::STREAM,
            Some(Protocol::TCP),
        )?;

        // Configure socket for low-latency async I/O
        let _ = socket.set_nonblocking(true);
        let _ = socket.set_reuse_address(true);

        // Linux-specific socket options for advanced routing
        #[cfg(target_os = "linux")]
        if let Some(so_mark) = so_mark {
            socket.set_mark(so_mark)?;
        }

        #[cfg(target_os = "linux")]
        if let Some(ref device) = bind_to_device {
            socket.bind_device(Some(device.as_bytes()))?;
        }

        connect_tcp_socket(socket, socket_addr).await
    }
}

#[cfg(feature = "_http-client")]
pub(crate) async fn connect_tcp_stream(
    remote_ip: Option<IpAddr>,
    server_name: String,
    port: u16,
    socks5_opt: Option<Socks5Opt>,
) -> Result<TcpStream> {
    connect_stream(remote_ip, server_name, port, None, None, socks5_opt).await
}

pub async fn connect_stream_with_deadline(
    remote_ip: Option<IpAddr>,
    server_name: String,
    port: u16,
    so_mark: Option<u32>,
    bind_to_device: Option<String>,
    socks5_opt: Option<Socks5Opt>,
    deadline: QueryDeadline,
) -> Result<TcpStream> {
    match deadline
        .run(connect_stream(
            remote_ip,
            server_name,
            port,
            so_mark,
            bind_to_device,
            socks5_opt,
        ))
        .await
    {
        DeadlineOutcome::Completed(result) => result,
        DeadlineOutcome::Expired => Err(deadline.timeout_error()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    use async_trait::async_trait;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::network::upstream::QueryDeadline;
    use crate::proto::Message;

    #[derive(Debug)]
    struct MockConnection {
        closed: AtomicBool,
        close_calls: AtomicUsize,
    }

    impl MockConnection {
        fn new() -> Self {
            Self {
                closed: AtomicBool::new(false),
                close_calls: AtomicUsize::new(0),
            }
        }

        fn close_calls(&self) -> usize {
            self.close_calls.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl Connection for MockConnection {
        fn close(&self) {
            self.close_calls.fetch_add(1, Ordering::Relaxed);
            self.closed.store(true, Ordering::Relaxed);
        }

        async fn query(&self, request: Message, _deadline: QueryDeadline) -> Result<Message> {
            Ok(request)
        }

        fn using_count(&self) -> u16 {
            0
        }

        fn available(&self) -> bool {
            !self.closed.load(Ordering::Relaxed)
        }

        fn last_used(&self) -> u64 {
            AtomicU64::new(0).load(Ordering::Relaxed)
        }
    }

    #[test]
    fn test_close_conns_closes_every_connection_once() {
        let first = Arc::new(MockConnection::new());
        let second = Arc::new(MockConnection::new());
        let conns = vec![first.clone(), second.clone()];

        close_conns(&conns);

        assert_eq!(first.close_calls(), 1);
        assert_eq!(second.close_calls(), 1);
    }

    #[tokio::test]
    async fn test_connect_stream_performs_standard_socks5_handshake_without_auth() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let proxy_addr = listener.local_addr().expect("listener should have addr");

        let proxy = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("proxy should accept");

            let mut greeting = [0u8; 3];
            stream
                .read_exact(&mut greeting)
                .await
                .expect("proxy should read greeting");
            assert_eq!(greeting, [0x05, 0x01, 0x00]);

            stream
                .write_all(&[0x05, 0x00])
                .await
                .expect("proxy should accept no-auth");

            let mut request_header = [0u8; 4];
            stream
                .read_exact(&mut request_header)
                .await
                .expect("proxy should read request header");
            assert_eq!(request_header, [0x05, 0x01, 0x00, 0x01]);

            let mut request_target = [0u8; 6];
            stream
                .read_exact(&mut request_target)
                .await
                .expect("proxy should read target");
            assert_eq!(request_target, [8, 8, 8, 8, 0x01, 0xBB]);

            stream
                .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x1F, 0x90])
                .await
                .expect("proxy should send success reply");
        });

        let _stream = connect_stream(
            Some(IpAddr::from([8, 8, 8, 8])),
            "dns.google".to_string(),
            443,
            None,
            None,
            Some(Socks5Opt {
                username: None,
                password: None,
                socket_addr: proxy_addr,
            }),
        )
        .await
        .expect("SOCKS5 tunnel should be established");

        proxy.await.expect("proxy task should complete");
    }

    #[tokio::test]
    async fn test_connect_stream_performs_standard_socks5_handshake_with_password_auth() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let proxy_addr = listener.local_addr().expect("listener should have addr");
        let username = "demo-user".to_string();
        let password = "demo-pass".to_string();

        let proxy_username = username.clone();
        let proxy_password = password.clone();
        let proxy = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("proxy should accept");

            let mut greeting = [0u8; 4];
            stream
                .read_exact(&mut greeting)
                .await
                .expect("proxy should read greeting");
            assert_eq!(greeting, [0x05, 0x02, 0x00, 0x02]);

            stream
                .write_all(&[0x05, 0x02])
                .await
                .expect("proxy should request password auth");

            let mut auth_header = [0u8; 2];
            stream
                .read_exact(&mut auth_header)
                .await
                .expect("proxy should read auth header");
            assert_eq!(auth_header, [0x01, proxy_username.len() as u8]);

            let mut auth_username = vec![0u8; proxy_username.len()];
            stream
                .read_exact(&mut auth_username)
                .await
                .expect("proxy should read username");
            assert_eq!(auth_username, proxy_username.as_bytes());

            let mut pass_len = [0u8; 1];
            stream
                .read_exact(&mut pass_len)
                .await
                .expect("proxy should read password length");
            assert_eq!(pass_len, [proxy_password.len() as u8]);

            let mut auth_password = vec![0u8; proxy_password.len()];
            stream
                .read_exact(&mut auth_password)
                .await
                .expect("proxy should read password");
            assert_eq!(auth_password, proxy_password.as_bytes());

            stream
                .write_all(&[0x01, 0x00])
                .await
                .expect("proxy should accept credentials");

            let mut request_header = [0u8; 4];
            stream
                .read_exact(&mut request_header)
                .await
                .expect("proxy should read request header");
            assert_eq!(request_header, [0x05, 0x01, 0x00, 0x03]);

            let mut domain_len = [0u8; 1];
            stream
                .read_exact(&mut domain_len)
                .await
                .expect("proxy should read domain length");
            assert_eq!(domain_len, [10]);

            let mut domain = vec![0u8; domain_len[0] as usize];
            stream
                .read_exact(&mut domain)
                .await
                .expect("proxy should read domain");
            assert_eq!(domain, b"dns.google");

            let mut port = [0u8; 2];
            stream
                .read_exact(&mut port)
                .await
                .expect("proxy should read port");
            assert_eq!(port, [0x01, 0xBB]);

            stream
                .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x1F, 0x90])
                .await
                .expect("proxy should send success reply");
        });

        let _stream = connect_stream(
            None,
            "dns.google".to_string(),
            443,
            None,
            None,
            Some(Socks5Opt {
                username: Some(username),
                password: Some(password),
                socket_addr: proxy_addr,
            }),
        )
        .await
        .expect("SOCKS5 tunnel should be established");

        proxy.await.expect("proxy task should complete");
    }
}
