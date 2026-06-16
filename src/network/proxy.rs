// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::str::FromStr;

use tracing::{info, warn};

use crate::core::error::{DnsError, Result};

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
    // Split by '@' to separate auth from host:port
    let (username, password, host_port) = if let Some(at_pos) = socks5_str.rfind('@') {
        // Format: username:password@host:port
        let auth_part = &socks5_str[..at_pos];
        let host_part = &socks5_str[at_pos + 1..];

        // Split auth by ':'
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
        // Format: host:port (no auth)
        (None, None, socks5_str)
    };

    // Parse host:port - use last colon to split
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

    // Remove IPv6 brackets if present: [::1] -> ::1
    if host.starts_with('[') && host.ends_with(']') {
        host = &host[1..host.len() - 1];
    }

    // Resolve host to IP address
    let ip_addr = if let Ok(ip) = IpAddr::from_str(host) {
        // Already an IP address
        ip
    } else {
        // It's a hostname, resolve it
        match resolve_host(host) {
            Ok(ip) => ip,
            Err(e) => {
                warn!("Failed to resolve SOCKS5 hostname '{}': {}", host, e);
                return None;
            }
        }
    };

    Some(Socks5Opt {
        username,
        password,
        socket_addr: SocketAddr::new(ip_addr, port),
    })
}

pub(crate) fn parse_socks5_opt(socks5_str: &str) -> Option<Socks5Opt> {
    parse_socks5_opt_with_resolver(socks5_str, try_lookup_server_name)
}

fn try_lookup_server_name(server_name: &str) -> Result<IpAddr> {
    match format!("{}:0", server_name).to_socket_addrs() {
        Ok(mut addrs) => match addrs.next() {
            Some(addr) => {
                let ip = addr.ip();
                info!(
                    server_name = %server_name,
                    resolved_ip = %ip,
                    ip_version = if ip.is_ipv4() { "IPv4" } else { "IPv6" },
                    "Resolved SOCKS5 hostname using system DNS"
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
