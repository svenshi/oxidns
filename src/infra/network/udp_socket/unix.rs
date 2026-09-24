// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Unix packet information. IPv4-mapped peers retain their socket address, but
//! use IPv4 send controls (required by macOS's dual-stack UDP implementation).
//!
//! Receive uses libc because socket2 0.6 does not expose MSG_CTRUNC through
//! RecvFlags. The original msg_flags and address length must remain available
//! to reject incomplete metadata. Sending can use socket2's checked header API.

use std::io;
use std::mem::{size_of, zeroed};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::AsRawFd;

use socket2::SockAddr;
use tokio::net::UdpSocket;

use super::control::{Control, decode};
use super::{UdpReplyTarget, invalid_data};

#[derive(Debug)]
pub(super) struct State;

impl State {
    pub fn new(socket: &UdpSocket) -> io::Result<Self> {
        let (level, option) = if socket.local_addr()?.is_ipv4() {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            let option = libc::IP_PKTINFO;
            #[cfg(target_os = "freebsd")]
            let option = libc::IP_RECVDSTADDR;
            (libc::IPPROTO_IP, option)
        } else {
            (libc::IPPROTO_IPV6, libc::IPV6_RECVPKTINFO)
        };
        let enabled: libc::c_int = 1;
        // SAFETY: fd is borrowed and enabled points to a correctly sized
        // integer.
        let result = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                level,
                option,
                (&enabled as *const libc::c_int).cast(),
                size_of::<libc::c_int>() as _,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self)
    }

    pub fn recv(&self, socket: &UdpSocket, buf: &mut [u8]) -> io::Result<(usize, UdpReplyTarget)> {
        let mut control = Control::new();
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        };
        // SAFETY: zero is valid for msghdr. All pointers remain live through
        // recvmsg.
        let mut message: libc::msghdr = unsafe { zeroed() };
        let (len, peer) = unsafe {
            SockAddr::try_init(|storage, addr_len| {
                message.msg_name = storage.cast();
                message.msg_namelen = *addr_len;
                message.msg_iov = &mut iov;
                message.msg_iovlen = 1;
                message.msg_control = control.bytes.as_mut_ptr().cast();
                message.msg_controllen = control.bytes.len() as _;
                let received = libc::recvmsg(socket.as_raw_fd(), &mut message, 0);
                if received == -1 {
                    return Err(io::Error::last_os_error());
                }
                *addr_len = message.msg_namelen;
                Ok(received as usize)
            })
        }?;
        if message.msg_flags & libc::MSG_TRUNC != 0 || len > buf.len() {
            return Err(invalid_data("Truncated UDP datagram"));
        }
        let (local_ip, interface_index) = packet_info(
            &control,
            message.msg_controllen as usize,
            message.msg_flags & libc::MSG_CTRUNC != 0,
        )?;
        let peer = peer
            .as_socket()
            .ok_or_else(|| invalid_data("Invalid UDP peer address"))?;
        Ok((len, UdpReplyTarget::new(peer, local_ip, interface_index)?))
    }

    pub fn send(
        &self,
        socket: &UdpSocket,
        bytes: &[u8],
        target: UdpReplyTarget,
    ) -> io::Result<usize> {
        let control = reply_control(target);
        let peer = SockAddr::from(target.peer);
        let buffers = [io::IoSlice::new(bytes)];
        let message = socket2::MsgHdr::new()
            .with_addr(&peer)
            .with_buffers(&buffers)
            .with_control(&control.bytes[..control.len]);
        socket2::SockRef::from(socket).sendmsg(&message, 0)
    }
}

fn packet_info(control: &Control, len: usize, truncated: bool) -> io::Result<(IpAddr, u32)> {
    let mut destination = None;
    for message in control.messages(len, truncated)? {
        let (level, kind, bytes) = message?;
        match (level, kind) {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            (libc::IPPROTO_IP, libc::IP_PKTINFO) => {
                // SAFETY: in_pktinfo contains only integers and IPv4 addresses.
                let info = unsafe { decode::<libc::in_pktinfo>(bytes) }?;
                destination = Some((
                    IpAddr::V4(Ipv4Addr::from(info.ipi_addr.s_addr.to_ne_bytes())),
                    info.ipi_ifindex as u32,
                ));
            }
            #[cfg(target_os = "freebsd")]
            (libc::IPPROTO_IP, libc::IP_RECVDSTADDR) => {
                // SAFETY: in_addr contains a single integer.
                let info = unsafe { decode::<libc::in_addr>(bytes) }?;
                destination = Some((IpAddr::V4(Ipv4Addr::from(info.s_addr.to_ne_bytes())), 0));
            }
            (libc::IPPROTO_IPV6, libc::IPV6_PKTINFO) => {
                // SAFETY: in6_pktinfo contains only integer fields.
                let info = unsafe { decode::<libc::in6_pktinfo>(bytes) }?;
                destination = Some((
                    IpAddr::V6(Ipv6Addr::from(info.ipi6_addr.s6_addr)),
                    info.ipi6_ifindex,
                ));
            }
            _ => {}
        }
    }
    destination.ok_or_else(|| invalid_data("Missing UDP packet destination address"))
}

fn reply_control(target: UdpReplyTarget) -> Control {
    let mut control = Control::new();
    // SAFETY: these native packet information types consist of initialized
    // integer/address fields with no padding on the supported Unix platforms.
    unsafe {
        match target.local_ip {
            IpAddr::V4(ip) => {
                let addr = libc::in_addr {
                    s_addr: u32::from_ne_bytes(ip.octets()),
                };
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                control.push(
                    libc::IPPROTO_IP,
                    libc::IP_PKTINFO,
                    libc::in_pktinfo {
                        ipi_ifindex: 0,
                        ipi_spec_dst: addr,
                        ipi_addr: libc::in_addr { s_addr: 0 },
                    },
                );
                #[cfg(target_os = "freebsd")]
                control.push(libc::IPPROTO_IP, libc::IP_SENDSRCADDR, addr);
            }
            IpAddr::V6(ip) => control.push(
                libc::IPPROTO_IPV6,
                libc::IPV6_PKTINFO,
                libc::in6_pktinfo {
                    ipi6_addr: libc::in6_addr {
                        s6_addr: ip.octets(),
                    },
                    ipi6_ifindex: target.outgoing_interface(),
                },
            ),
        }
    }
    control
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_controls_use_the_header_destination_as_the_reply_source() {
        let ip = Ipv4Addr::new(192, 0, 2, 1);
        let addr = libc::in_addr {
            s_addr: u32::from_ne_bytes(ip.octets()),
        };
        let mut received = Control::new();
        // SAFETY: all fields are initialized and these native types have no
        // padding.
        unsafe {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            received.push(
                libc::IPPROTO_IP,
                libc::IP_PKTINFO,
                libc::in_pktinfo {
                    ipi_ifindex: 7,
                    ipi_spec_dst: libc::in_addr {
                        s_addr: u32::from_ne_bytes([198, 51, 100, 1]),
                    },
                    ipi_addr: addr,
                },
            );
            #[cfg(target_os = "freebsd")]
            received.push(libc::IPPROTO_IP, libc::IP_RECVDSTADDR, addr);
        }
        assert_eq!(
            packet_info(&received, received.len, false).unwrap().0,
            IpAddr::V4(ip)
        );

        let target =
            UdpReplyTarget::new("198.51.100.10:1234".parse().unwrap(), ip.into(), 7).unwrap();
        let sent = reply_control(target);
        let (level, kind, bytes) = sent
            .messages(sent.len, false)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(level, libc::IPPROTO_IP);
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            assert_eq!(kind, libc::IP_PKTINFO);
            let info = unsafe { decode::<libc::in_pktinfo>(bytes) }.unwrap();
            assert_eq!(info.ipi_spec_dst.s_addr, addr.s_addr);
            assert_eq!(info.ipi_ifindex, 0);
        }
        #[cfg(target_os = "freebsd")]
        {
            assert_eq!(kind, libc::IP_SENDSRCADDR);
            assert_eq!(
                unsafe { decode::<libc::in_addr>(bytes) }.unwrap().s_addr,
                addr.s_addr
            );
        }
    }

    #[test]
    fn ipv6_controls_preserve_link_local_scope() {
        let target = UdpReplyTarget::new(
            "[fe80::2%7]:1234".parse().unwrap(),
            "fe80::1".parse().unwrap(),
            7,
        )
        .unwrap();
        let control = reply_control(target);
        assert_eq!(
            packet_info(&control, control.len, false).unwrap(),
            (target.local_ip, 7)
        );
    }

    #[test]
    fn packet_information_requires_a_complete_destination() {
        let mut control = Control::new();
        assert!(packet_info(&control, 0, false).is_err());
        // SAFETY: u32 has no padding or uninitialized bytes.
        unsafe { control.push(libc::IPPROTO_IPV6, libc::IPV6_PKTINFO, 0u32) };
        assert!(packet_info(&control, control.len, false).is_err());
        let mut control = Control::new();
        let ip: Ipv6Addr = "::ffff:192.0.2.1".parse().unwrap();
        // SAFETY: in6_pktinfo has no padding and both fields are initialized.
        unsafe {
            control.push(
                libc::IPPROTO_IPV6,
                libc::IPV6_PKTINFO,
                libc::in6_pktinfo {
                    ipi6_addr: libc::in6_addr {
                        s6_addr: ip.octets(),
                    },
                    ipi6_ifindex: 7,
                },
            );
        }
        assert_eq!(
            packet_info(&control, control.len, false).unwrap(),
            (ip.into(), 7)
        );
        assert!(packet_info(&control, control.len, true).is_err());
        let target =
            UdpReplyTarget::new("[::ffff:192.0.2.2]:1234".parse().unwrap(), ip.into(), 7).unwrap();
        let control = reply_control(target);
        let (level, _, _) = control
            .messages(control.len, false)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(level, libc::IPPROTO_IP);
        assert!(target.peer.is_ipv6());
    }
}
