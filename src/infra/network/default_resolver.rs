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

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
type Network = (Vec<Ipv4Addr>, Option<Ipv4Addr>, Vec<Ipv4Addr>);

async fn discover() -> io::Result<Vec<Ipv4Addr>> {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        let (dns, gateway, local) =
            tokio::time::timeout(2 * COMMAND_TIMEOUT, platform::discover()).await??;
        Ok(select_servers(dns, gateway, &local))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "default_resolver is not supported",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
use platform::Monitor;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
struct Monitor;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
impl Monitor {
    pub(crate) fn new() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "default_resolver is not supported",
        ))
    }

    pub(crate) async fn changed(&mut self) -> io::Result<()> {
        std::future::pending().await
    }
}

/// The shared list of system-discovered DNS addresses, independent of
/// forwarding.
pub(crate) struct DefaultResolver {
    addresses: Vec<Ipv4Addr>,
    monitor: Monitor,
}

impl DefaultResolver {
    pub(crate) fn new() -> io::Result<Self> {
        Ok(Self {
            addresses: Vec::new(),
            monitor: Monitor::new()?,
        })
    }

    /// Cache the first complete observation. A newer event supersedes any read
    /// that was still in progress when it arrived.
    pub(crate) async fn refresh(&mut self) -> io::Result<&[Ipv4Addr]> {
        loop {
            tokio::select! {
                biased;
                event = self.monitor.changed() => event?,
                result = discover() => {
                    self.addresses = result.unwrap_or_else(|error| {
                        tracing::debug!(%error, "Default DNS discovery failed");
                        Vec::new()
                    });
                    return Ok(&self.addresses);
                }
            }
        }
    }

    pub(crate) async fn changed(&mut self) -> io::Result<()> {
        self.monitor.changed().await?;
        self.addresses.clear();
        Ok(())
    }
}

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
fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program.as_ref());
    cmd.env("LC_ALL", "C").kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
async fn output(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> io::Result<String> {
    let program = program.as_ref();
    let mut cmd = command(program);
    cmd.args(args);
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
