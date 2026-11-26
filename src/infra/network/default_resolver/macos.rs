// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;

use super::{Network, NetworkIdentity, output, parse_addresses};

pub(super) async fn discover() -> io::Result<Network> {
    let route = output("/sbin/route", &["-n", "get", "-inet", "default"]).await?;
    let interface = field(&route, "interface:")
        .filter(|interface| !interface.starts_with('-'))
        .ok_or_else(|| io::Error::other("default route has no interface"))?;
    let gateway = field(&route, "gateway:").and_then(|value| value.parse().ok());
    let ipconfig_args = ["getoption", interface, "domain_name_server"];
    let (lease_dns, local) = tokio::join!(
        output("/usr/sbin/ipconfig", &ipconfig_args),
        output("/sbin/ifconfig", &[]),
    );
    let dns = lease_dns
        .map(|value| parse_addresses(&value))
        .unwrap_or_default();
    let local = local?
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next()? != "inet" {
                return None;
            }
            fields.next()?.parse().ok()
        })
        .collect();
    Ok((
        dns,
        NetworkIdentity {
            interface: Some(interface.to_owned()),
            gateway,
            local,
        },
    ))
}

fn field<'a>(route: &'a str, key: &str) -> Option<&'a str> {
    route
        .lines()
        .find_map(|line| line.trim().strip_prefix(key).map(str::trim))
}
