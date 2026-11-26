// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::net::Ipv4Addr;

use serde::Deserialize;

use super::{Network, NetworkIdentity, output, parse_addresses};

// DhcpNameServer holds the lease's DNS option; NameServer is the manual
// override.
const DISCOVER: &str = r#"
$ErrorActionPreference = 'Stop'
$WarningPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
$interfaces = @{}
Get-NetIPInterface -AddressFamily IPv4 | Where-Object ConnectionState -eq Connected |
    ForEach-Object { $interfaces[$_.InterfaceIndex] = $_ }
$route = Get-NetRoute -AddressFamily IPv4 -DestinationPrefix '0.0.0.0/0' -PolicyStore ActiveStore -ErrorAction SilentlyContinue |
    Where-Object { $interfaces.ContainsKey($_.InterfaceIndex) } |
    Sort-Object { [long]$_.RouteMetric + $interfaces[$_.InterfaceIndex].InterfaceMetric } |
    Select-Object -First 1
if (!$route) { '{}'; exit }
$adapter = Get-NetAdapter -InterfaceIndex $route.InterfaceIndex -IncludeHidden -ErrorAction SilentlyContinue
$dns = ''
if ($adapter -and $interfaces[$route.InterfaceIndex].Dhcp -eq 'Enabled') {
    $guid = '{' + ([guid]$adapter.InterfaceGuid).ToString() + '}'
    $lease = Get-ItemProperty -LiteralPath ('HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\' + $guid) -ErrorAction SilentlyContinue
    $dns = [string]$lease.DhcpNameServer
}
@{
    dns = $dns
    interface = $route.InterfaceIndex
    gateway = $route.NextHop
    local = @(Get-NetIPAddress -AddressFamily IPv4 | ForEach-Object { $_.IPAddress })
} | ConvertTo-Json -Compress
"#;

#[derive(Deserialize)]
struct WindowsNetwork {
    #[serde(default)]
    dns: String,
    interface: Option<u32>,
    gateway: Option<Ipv4Addr>,
    #[serde(default)]
    local: Vec<Ipv4Addr>,
}

pub(super) async fn discover() -> io::Result<Network> {
    let system_root =
        std::env::var_os("SystemRoot").ok_or_else(|| io::Error::other("SystemRoot is not set"))?;
    let powershell = std::path::PathBuf::from(system_root)
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let value = output(
        powershell,
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            DISCOVER,
        ],
    )
    .await?;
    let network: WindowsNetwork = serde_json::from_str(&value)?;
    Ok((
        parse_addresses(&network.dns),
        NetworkIdentity {
            interface: network.interface.map(|index| index.to_string()),
            gateway: network.gateway,
            local: network.local,
        },
    ))
}
