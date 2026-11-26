// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Discover IPv4 DNS servers advertised on the host's default network.

use std::io;
use std::net::Ipv4Addr;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use std::time::Duration;

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use tokio::process::Command;

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
const COMMAND_TIMEOUT: Duration = if cfg!(windows) {
    Duration::from_secs(5)
} else {
    Duration::from_secs(2)
};

pub(crate) const fn is_supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos", windows))
}

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct NetworkIdentity {
    // Using the route interface as well as the assigned addresses keeps pools
    // from following a stale source address when networks reuse a resolver IP.
    interface: Option<String>,
    gateway: Option<Ipv4Addr>,
    local: Vec<Ipv4Addr>,
}

#[derive(Debug, Default)]
pub(crate) struct Discovery {
    pub(crate) network: NetworkIdentity,
    pub(crate) servers: Vec<Ipv4Addr>,
}

pub(crate) async fn discover() -> io::Result<Discovery> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        let (dns, mut network) =
            tokio::time::timeout(2 * COMMAND_TIMEOUT, platform::discover()).await??;
        network.local.sort_unstable();
        network.local.dedup();
        let servers = select_servers(dns, network.gateway, &network.local);
        Ok(Discovery { network, servers })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "default_resolver is not supported on this platform",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
type Network = (Vec<Ipv4Addr>, NetworkIdentity);

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn select_servers(
    dns: Vec<Ipv4Addr>,
    gateway: Option<Ipv4Addr>,
    local: &[Ipv4Addr],
) -> Vec<Ipv4Addr> {
    let usable = |ip: &Ipv4Addr| {
        ip.octets()[0] != 0
            && !ip.is_loopback()
            && !ip.is_multicast()
            && !ip.is_broadcast()
            && !local.contains(ip)
    };
    let mut servers = Vec::new();
    for ip in dns.into_iter().filter(usable) {
        if !servers.contains(&ip) {
            servers.push(ip);
        }
    }
    if servers.is_empty() {
        servers.extend(gateway.filter(usable));
    }
    servers
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn parse_addresses(value: &str) -> Vec<Ipv4Addr> {
    value
        .split(|c: char| c.is_ascii_whitespace() || matches!(c, ',' | ';' | '{' | '}'))
        .filter_map(|part| part.parse().ok())
        .collect()
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
async fn output(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> io::Result<String> {
    let program = program.as_ref();
    let mut cmd = Command::new(program);
    cmd.args(args).env("LC_ALL", "C").kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let result = tokio::time::timeout(COMMAND_TIMEOUT, cmd.output()).await??;
    if !result.status.success() {
        return Err(io::Error::other(format!(
            "{program:?} exited with {}",
            result.status
        )));
    }
    String::from_utf8(result.stdout).map_err(io::Error::other)
}

#[cfg(target_os = "linux")]
#[path = "default_resolver/linux.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "default_resolver/macos.rs"]
mod platform;

#[cfg(windows)]
#[path = "default_resolver/windows.rs"]
mod platform;
