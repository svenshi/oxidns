// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::net::Ipv4Addr;
use std::process::Stdio;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout};

use super::{Network, output, parse_addresses};

#[derive(Deserialize)]
struct Route {
    dev: Option<String>,
    gateway: Option<Ipv4Addr>,
    #[serde(default)]
    metric: u32,
    #[serde(default)]
    flags: Vec<String>,
}

#[derive(Deserialize)]
struct Interface {
    ifindex: u32,
    ifname: String,
    #[serde(default)]
    addr_info: Vec<Address>,
}

#[derive(Deserialize)]
struct Address {
    local: Ipv4Addr,
}

pub(super) async fn discover() -> io::Result<Network> {
    let (routes, interfaces) = tokio::try_join!(
        output("ip", &["-j", "-4", "route", "show", "default"]),
        output("ip", &["-j", "-4", "addr", "show"]),
    )?;
    let routes: Vec<Route> = serde_json::from_str(&routes)?;
    let interfaces: Vec<Interface> = serde_json::from_str(&interfaces)?;
    let Some(route) = routes
        .iter()
        .filter(|route| route.dev.is_some() && !route.flags.iter().any(|flag| flag == "linkdown"))
        .min_by_key(|route| route.metric)
    else {
        return Ok(Network::default());
    };
    let interface = interfaces
        .iter()
        .find(|iface| Some(iface.ifname.as_str()) == route.dev.as_deref())
        .ok_or_else(|| io::Error::other("default route interface disappeared"))?;

    // Read advertised DHCP options, never the effective IP4.DNS settings.
    let lease_path = format!("/run/systemd/netif/leases/{}", interface.ifindex);
    let nm_args = ["-g", "DHCP4.OPTION", "device", "show", &interface.ifname];
    let (nm, networkd) = tokio::join!(
        output("nmcli", &nm_args),
        tokio::fs::read_to_string(lease_path),
    );
    let mut dns = nm.map(|text| nm_dns(&text)).unwrap_or_default();
    if dns.is_empty()
        && let Ok(lease) = networkd
    {
        dns = networkd_dns(&lease);
    }
    let local = interfaces
        .iter()
        .flat_map(|iface| iface.addr_info.iter().map(|addr| addr.local))
        .collect();
    Ok((dns, route.gateway, local))
}

fn nm_dns(options: &str) -> Vec<Ipv4Addr> {
    // Depending on nmcli's output mode, array items are delimited by newlines
    // or pipes.
    options
        .split(['\n', '|'])
        .filter_map(|option| option.trim().split_once('='))
        .filter(|(key, _)| key.trim() == "domain_name_servers")
        .flat_map(|(_, value)| parse_addresses(value))
        .collect()
}

fn networkd_dns(lease: &str) -> Vec<Ipv4Addr> {
    lease
        .lines()
        .filter_map(|line| line.strip_prefix("DNS="))
        .flat_map(parse_addresses)
        .collect()
}

pub(crate) struct Monitor {
    _child: Child,
    events: Lines<BufReader<ChildStdout>>,
}

impl Monitor {
    pub(crate) fn new() -> io::Result<Self> {
        let mut child = super::command("ip")
            .args(["-o", "-4", "monitor", "link", "address", "route"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing ip monitor output"))?;
        Ok(Self {
            _child: child,
            events: BufReader::new(stdout).lines(),
        })
    }

    pub(crate) async fn changed(&mut self) -> io::Result<()> {
        if self.events.next_line().await?.is_some() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "ip monitor stopped",
            ))
        }
    }
}
