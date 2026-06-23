// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use fast_socks5::client::Socks5Stream;
use fast_socks5::util::target_addr::TargetAddr;
use fast_socks5::{AuthenticationMethod, Socks5Command};
use tokio::net::TcpStream;
use tracing::warn;

#[cfg(feature = "_http-client")]
use crate::infra::error::DnsError;
use crate::infra::error::Result;
use crate::infra::network::dial::{
    DialTarget, SocketOptions, TcpDialOptions, connect_tcp as dial_connect_tcp,
    try_lookup_server_name,
};

/// SOCKS5 proxy configuration with resolved socket address
///
/// This struct contains the parsed and resolved SOCKS5 proxy information,
/// ready to be used for establishing proxy connections.
///
/// # Fields
/// - `username`: Optional SOCKS5 authentication username
/// - `password`: Optional SOCKS5 authentication password
/// - `socket_addr`: Resolved proxy server socket address (IP + port)
///
/// # Note
/// The hostname in the original configuration (if any) has already been
/// resolved to an IP address when this struct is created.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Socks5Opt {
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) socket_addr: SocketAddr,
}

#[derive(Debug)]
struct Socks5Parts {
    username: Option<String>,
    password: Option<String>,
    host: String,
    port: u16,
}

fn parse_socks5_parts(socks5_str: &str) -> Option<Socks5Parts> {
    let socks5_str = socks5_str.trim();

    let (username, password, host_port) = if let Some(at_pos) = socks5_str.rfind('@') {
        let auth_part = &socks5_str[..at_pos];
        let host_part = &socks5_str[at_pos + 1..];

        if let Some(colon_pos) = auth_part.find(':') {
            let username = auth_part[..colon_pos].to_string();
            let password = auth_part[colon_pos + 1..].to_string();
            (Some(username), Some(password), host_part)
        } else {
            warn!(
                "Invalid SOCKS5 auth format (expected username:password): {}",
                socks5_str
            );
            return None;
        }
    } else {
        (None, None, socks5_str)
    };

    let (mut host, port) = match host_port.rfind(':') {
        Some(colon_pos) => {
            let host = &host_port[..colon_pos];
            let port_str = &host_port[colon_pos + 1..];

            match port_str.parse::<u16>() {
                Ok(port) => (host, port),
                Err(_) => {
                    warn!("Invalid SOCKS5 port: {}", port_str);
                    return None;
                }
            }
        }
        None => {
            warn!("Invalid SOCKS5 format (expected host:port): {}", host_port);
            return None;
        }
    };

    if host.starts_with('[') || host.ends_with(']') {
        if !(host.starts_with('[') && host.ends_with(']')) {
            warn!("Invalid SOCKS5 IPv6 bracket format: {}", host);
            return None;
        }
        host = &host[1..host.len() - 1];
    }

    if host.is_empty() {
        warn!(
            "Invalid SOCKS5 format (host cannot be empty): {}",
            socks5_str
        );
        return None;
    }

    Some(Socks5Parts {
        username,
        password,
        host: host.to_string(),
        port,
    })
}

pub(crate) fn validate_socks5_syntax(socks5_str: &str) -> bool {
    parse_socks5_parts(socks5_str).is_some()
}

/// Parse SOCKS5 proxy configuration from string
///
/// Supports two formats:
/// - "host:port" - SOCKS5 without authentication
/// - "username:password@host:port" - SOCKS5 with authentication
///
/// If host is a domain name, it will be resolved using system DNS.
///
/// # Arguments
/// * `socks5_str` - SOCKS5 proxy string in one of the supported formats
///
/// # Returns
/// - `Some(Socks5Opt)` if parsing and resolution succeed
/// - `None` if parsing fails or hostname resolution fails
///
/// # Examples
/// ```text
/// // Without auth
/// parse_socks5_opt("127.0.0.1:1080")
/// parse_socks5_opt("proxy.example.com:1080")
///
/// // With auth
/// parse_socks5_opt("user:pass@127.0.0.1:1080")
/// parse_socks5_opt("user:pass@proxy.example.com:1080")
/// ```
pub(crate) fn parse_socks5_opt_with_resolver<F>(
    socks5_str: &str,
    mut resolve_host: F,
) -> Option<Socks5Opt>
where
    F: FnMut(&str) -> Result<IpAddr>,
{
    let parts = parse_socks5_parts(socks5_str)?;

    // Resolve host to IP address
    let ip_addr = if let Ok(ip) = IpAddr::from_str(&parts.host) {
        // Already an IP address
        ip
    } else {
        // It's a hostname, resolve it
        match resolve_host(&parts.host) {
            Ok(ip) => ip,
            Err(e) => {
                warn!("Failed to resolve SOCKS5 hostname '{}': {}", parts.host, e);
                return None;
            }
        }
    };

    Some(Socks5Opt {
        username: parts.username,
        password: parts.password,
        socket_addr: SocketAddr::new(ip_addr, parts.port),
    })
}

pub(crate) fn parse_socks5_opt(socks5_str: &str) -> Option<Socks5Opt> {
    parse_socks5_opt_with_resolver(socks5_str, try_lookup_server_name)
}

#[cfg(feature = "_http-client")]
pub(crate) fn parse_optional_socks5<F>(raw: Option<&str>, invalid: F) -> Result<Option<Socks5Opt>>
where
    F: FnOnce(&str) -> DnsError,
{
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(None);
    };
    parse_socks5_opt(raw).map(Some).ok_or_else(|| invalid(raw))
}

pub(crate) async fn connect_tcp(
    target: DialTarget,
    socket_options: SocketOptions,
    socks5: Option<Socks5Opt>,
) -> Result<TcpStream> {
    match socks5 {
        Some(socks5) => connect_tcp_via_socks5(target, socket_options, socks5).await,
        None => {
            dial_connect_tcp(TcpDialOptions::new(target).with_socket_options(socket_options)).await
        }
    }
}

async fn connect_tcp_via_socks5(
    target: DialTarget,
    socket_options: SocketOptions,
    socks5: Socks5Opt,
) -> Result<TcpStream> {
    let proxy_target = DialTarget::from_socket_addr(socks5.socket_addr);
    let proxy_stream =
        dial_connect_tcp(TcpDialOptions::new(proxy_target).with_socket_options(socket_options))
            .await?;

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
    let mut socks5_stream = Socks5Stream::use_stream(proxy_stream, auth, config).await?;

    let target_addr = if let Some(remote_ip) = target.remote_ip() {
        TargetAddr::Ip(SocketAddr::new(remote_ip, target.port()))
    } else {
        TargetAddr::Domain(target.host().to_string(), target.port())
    };

    socks5_stream
        .request(Socks5Command::TCPConnect, target_addr)
        .await?;

    let stream = socks5_stream.get_socket();
    let _ = stream.set_nodelay(true);

    Ok(stream)
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    #[test]
    fn test_validate_socks5_syntax_accepts_hostname_without_dns() {
        assert!(validate_socks5_syntax("proxy.example.com:1080"));
        assert!(validate_socks5_syntax("user:pass@proxy.example.com:1080"));
    }

    #[test]
    fn test_validate_socks5_syntax_rejects_malformed_values() {
        assert!(!validate_socks5_syntax("127.0.0.1"));
        assert!(!validate_socks5_syntax("127.0.0.1:notaport"));
        assert!(!validate_socks5_syntax(":1080"));
        assert!(!validate_socks5_syntax("user@127.0.0.1:1080"));
        assert!(!validate_socks5_syntax("[::1:1080"));
    }

    #[tokio::test]
    async fn test_connect_tcp_performs_standard_socks5_handshake_without_auth() {
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

        let target = DialTarget::new(
            Some(IpAddr::from([8, 8, 8, 8])),
            "dns.google".to_string(),
            443,
        );
        let _stream = connect_tcp(
            target,
            SocketOptions::default(),
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
    async fn test_connect_tcp_performs_standard_socks5_handshake_with_password_auth() {
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

        let target = DialTarget::new(None, "dns.google".to_string(), 443);
        let _stream = connect_tcp(
            target,
            SocketOptions::default(),
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
