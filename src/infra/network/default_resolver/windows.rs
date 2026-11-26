// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::c_void;
use std::io;
use std::net::Ipv4Addr;

use serde::Deserialize;
use tokio::sync::mpsc::{self, Receiver, Sender};
use windows::Win32::Foundation::{HANDLE, WIN32_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    CancelMibChangeNotify2, MIB_IPFORWARD_ROW2, MIB_IPINTERFACE_ROW, MIB_NOTIFICATION_TYPE,
    MIB_UNICASTIPADDRESS_ROW, NotifyIpInterfaceChange, NotifyRouteChange2,
    NotifyUnicastIpAddressChange,
};
use windows::Win32::Networking::WinSock::AF_INET;

use super::{Network, output, parse_addresses};

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
    gateway = $route.NextHop
    local = @(Get-NetIPAddress -AddressFamily IPv4 | ForEach-Object { $_.IPAddress })
} | ConvertTo-Json -Compress
"#;

#[derive(Deserialize)]
struct WindowsNetwork {
    #[serde(default)]
    dns: String,
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
        network.gateway,
        network.local,
    ))
}

unsafe extern "system" fn interface_changed(
    context: *const c_void,
    _: *const MIB_IPINTERFACE_ROW,
    _: MIB_NOTIFICATION_TYPE,
) {
    // SAFETY: The context is kept alive until deregistration completes.
    unsafe { signal(context) };
}

unsafe extern "system" fn route_changed(
    context: *const c_void,
    _: *const MIB_IPFORWARD_ROW2,
    _: MIB_NOTIFICATION_TYPE,
) {
    // SAFETY: The context is kept alive until deregistration completes.
    unsafe { signal(context) };
}

unsafe extern "system" fn address_changed(
    context: *const c_void,
    _: *const MIB_UNICASTIPADDRESS_ROW,
    _: MIB_NOTIFICATION_TYPE,
) {
    // SAFETY: The context is kept alive until deregistration completes.
    unsafe { signal(context) };
}

unsafe fn signal(context: *const c_void) {
    // SAFETY: The caller passes the stable allocation owned by Registration.
    let sender = unsafe { &*context.cast::<Sender<()>>() };
    let _ = sender.try_send(());
}

struct Registration {
    handles: [HANDLE; 3],
    sender: Option<Box<Sender<()>>>,
}

// SAFETY: Notification handles can be canceled on another thread; callbacks
// only access the allocation containing the thread-safe Sender.
unsafe impl Send for Registration {}

impl Drop for Registration {
    fn drop(&mut self) {
        let mut canceled = true;
        for handle in self.handles.iter().filter(|handle| !handle.is_invalid()) {
            // SAFETY: These are handles returned by the registration calls.
            canceled &= unsafe { CancelMibChangeNotify2(*handle) } == WIN32_ERROR(0);
        }
        if !canceled {
            // If deregistration fails, the OS may still use the callback
            // pointer.
            let _ = Box::leak(self.sender.take().expect("callback context"));
        }
    }
}

pub(crate) struct Monitor {
    _registration: Registration,
    events: Receiver<()>,
}

impl Monitor {
    pub(crate) fn new() -> io::Result<Self> {
        let (sender, events) = mpsc::channel(1);
        let mut registration = Registration {
            handles: [HANDLE::default(); 3],
            sender: Some(Box::new(sender)),
        };
        let context =
            std::ptr::from_ref(registration.sender.as_deref().expect("callback context")).cast();
        let check = |status: WIN32_ERROR| {
            if status == WIN32_ERROR(0) {
                Ok(())
            } else {
                Err(io::Error::from_raw_os_error(status.0 as i32))
            }
        };
        // SAFETY: Each call gets its own output slot and the stable context.
        unsafe {
            check(NotifyIpInterfaceChange(
                AF_INET,
                Some(interface_changed),
                Some(context),
                false,
                &mut registration.handles[0],
            ))?;
            check(NotifyRouteChange2(
                AF_INET,
                Some(route_changed),
                context,
                false,
                &mut registration.handles[1],
            ))?;
            check(NotifyUnicastIpAddressChange(
                AF_INET,
                Some(address_changed),
                Some(context),
                false,
                &mut registration.handles[2],
            ))?;
        }
        Ok(Self {
            _registration: registration,
            events,
        })
    }

    pub(crate) async fn changed(&mut self) -> io::Result<()> {
        self.events
            .recv()
            .await
            .ok_or_else(|| io::Error::other("IP Helper notification stream stopped"))
    }
}
