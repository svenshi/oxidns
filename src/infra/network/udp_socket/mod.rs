// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Non-blocking UDP replies that preserve the query's destination address.
//!
//! Wildcard listeners capture packet information once per datagram. All socket
//! setup belongs to initialization; replies carry their own immutable routing
//! metadata and never change shared socket options. Explicit bindings need no
//! packet information. This module owns no tasks or DNS/plugin state.

use std::io;
use std::net::{IpAddr, SocketAddr};

use tokio::io::Interest;
use tokio::net::UdpSocket;

use crate::infra::network::ip::normalize_ipv4_mapped_ip;

#[cfg(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    windows
))]
mod control;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
#[path = "unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    windows
)))]
#[path = "unsupported.rs"]
mod platform;

/// The validated peer and local destination of one received datagram.
///
/// Callers carry this value unchanged from receive to reply. Keeping its fields
/// private prevents transport users from bypassing address and scope
/// validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UdpReplyTarget {
    peer: SocketAddr,
    local_ip: IpAddr,
    interface_index: u32,
}

impl UdpReplyTarget {
    pub fn peer_addr(self) -> SocketAddr {
        self.peer
    }

    fn new(peer: SocketAddr, local_ip: IpAddr, interface_index: u32) -> io::Result<Self> {
        let local_ip = normalize_ipv4_mapped_ip(local_ip);
        // Explicit global/ULA bindings have no local scope ID, but a link-local
        // peer still carries the zone needed to route the reply.
        let interface_index = if interface_index == 0 {
            match peer {
                SocketAddr::V6(addr) => addr.scope_id(),
                SocketAddr::V4(_) => 0,
            }
        } else {
            interface_index
        };
        if local_ip.is_unspecified()
            || local_ip.is_multicast()
            || local_ip == IpAddr::V4(std::net::Ipv4Addr::BROADCAST)
            || local_ip.is_ipv4() != normalize_ipv4_mapped_ip(peer.ip()).is_ipv4()
        {
            return Err(invalid_data("Invalid UDP packet destination address"));
        }
        if interface_index == 0
            && (matches!(local_ip, IpAddr::V6(ip) if ip.is_unicast_link_local())
                || matches!(peer.ip(), IpAddr::V6(ip) if ip.is_unicast_link_local()))
        {
            return Err(invalid_data(
                "Missing UDP interface index for link-local address",
            ));
        }
        Ok(Self {
            peer,
            local_ip,
            interface_index,
        })
    }

    /// Routable unicast replies must not be pinned to the arriving interface.
    fn outgoing_interface(self) -> u32 {
        if matches!(self.local_ip, IpAddr::V6(ip) if ip.is_unicast_link_local())
            || matches!(self.peer.ip(), IpAddr::V6(ip) if ip.is_unicast_link_local())
        {
            self.interface_index
        } else {
            0
        }
    }
}

#[derive(Debug)]
pub(crate) struct UdpReplySocket {
    socket: UdpSocket,
    bound: SocketAddr,
    packet_info: Option<platform::State>,
}

impl UdpReplySocket {
    pub fn new(socket: UdpSocket) -> io::Result<Self> {
        Self::initialize(socket, platform::State::new)
    }

    fn initialize(
        socket: UdpSocket,
        initialize: impl FnOnce(&UdpSocket) -> io::Result<platform::State>,
    ) -> io::Result<Self> {
        let bound = socket.local_addr()?;
        let packet_info = if normalize_ipv4_mapped_ip(bound.ip()).is_unspecified() {
            Some(initialize(&socket).map_err(|err| {
                io::Error::new(err.kind(), format!(
                    "Cannot preserve UDP reply source addresses on {bound}: {err}; bind an explicit IP address instead"
                ))
            })?)
        } else {
            None
        };
        Ok(Self {
            socket,
            bound,
            packet_info,
        })
    }

    pub async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, UdpReplyTarget)> {
        self.socket
            .async_io(Interest::READABLE, || {
                retry_interrupted(|| {
                    if let Some(state) = &self.packet_info {
                        state.recv(&self.socket, buf)
                    } else {
                        // socket2 preserves the datagram truncation flag,
                        // unlike recv_from. SAFETY: the
                        // slice stays initialized: socket2 only writes received
                        // bytes.
                        let buf = unsafe {
                            std::slice::from_raw_parts_mut(
                                buf.as_mut_ptr().cast::<std::mem::MaybeUninit<u8>>(),
                                buf.len(),
                            )
                        };
                        let (len, flags, peer) = socket2::SockRef::from(&self.socket)
                            .recv_from_vectored(&mut [socket2::MaybeUninitSlice::new(buf)])?;
                        if flags.is_truncated() {
                            return Err(invalid_data("Truncated UDP datagram"));
                        }
                        let peer = peer
                            .as_socket()
                            .ok_or_else(|| invalid_data("Invalid UDP peer address"))?;
                        let interface_index = match self.bound {
                            SocketAddr::V6(addr) => addr.scope_id(),
                            _ => 0,
                        };
                        Ok((
                            len,
                            UdpReplyTarget::new(peer, self.bound.ip(), interface_index)?,
                        ))
                    }
                })
            })
            .await
    }

    pub async fn send_to(&self, bytes: &[u8], target: UdpReplyTarget) -> io::Result<usize> {
        if let Some(state) = &self.packet_info {
            self.socket
                .async_io(Interest::WRITABLE, || {
                    retry_interrupted(|| state.send(&self.socket, bytes, target))
                })
                .await
        } else {
            self.socket.send_to(bytes, target.peer).await
        }
    }
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn retry_interrupted<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    loop {
        match operation() {
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests;
