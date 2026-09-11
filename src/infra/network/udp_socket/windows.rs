// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Winsock packet information. Resolve WSARecvMsg on the listener itself so
//! the function pointer belongs to the same socket service provider.

use std::io;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::windows::io::AsRawSocket;

use socket2::SockAddr;
use tokio::net::UdpSocket;
use windows::Win32::Networking::WinSock as ws;
use windows::core::PSTR;

use super::control::{Control, decode};
use super::{UdpReplyTarget, invalid_data};

#[derive(Debug)]
pub(super) struct State {
    recv_msg: ws::LPFN_WSARECVMSG,
}

fn socket_handle(socket: &UdpSocket) -> ws::SOCKET {
    ws::SOCKET(socket.as_raw_socket() as usize)
}

fn last_error() -> io::Error {
    // SAFETY: WSAGetLastError has no preconditions.
    io::Error::from_raw_os_error(unsafe { ws::WSAGetLastError() }.0)
}

fn enable(socket: &UdpSocket, level: ws::IPPROTO, option: i32) -> io::Result<()> {
    // SAFETY: the borrowed socket is live and the option is a DWORD boolean.
    if unsafe {
        ws::setsockopt(
            socket_handle(socket),
            level.0,
            option,
            Some(&1u32.to_ne_bytes()),
        )
    } == ws::SOCKET_ERROR
    {
        return Err(last_error());
    }
    Ok(())
}

fn configure_packet_info(
    is_v6: bool,
    only_v6: bool,
    mut enable: impl FnMut(ws::IPPROTO, i32) -> io::Result<()>,
    probe_ipv4: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    if (!is_v6 || !only_v6)
        && let Err(err) = enable(ws::IPPROTO_IP, ws::IP_PKTINFO)
    {
        // Winsock returns WSAEINVAL for IP_PKTINFO on a dual-stack socket
        // when IPv4 is disabled. Only WSAEAFNOSUPPORT from an AF_INET socket
        // probe establishes that IPv4 packet information is unnecessary.
        // Other option/probe errors must not weaken the reply-source guarantee.
        // https://learn.microsoft.com/en-us/windows/win32/winsock/dual-stack-sockets
        let ipv4_unavailable = is_v6
            && err.raw_os_error() == Some(ws::WSAEINVAL.0)
            && matches!(probe_ipv4(), Err(probe) if probe.raw_os_error() == Some(ws::WSAEAFNOSUPPORT.0));
        if !ipv4_unavailable {
            return Err(err);
        }
    }
    if is_v6 {
        enable(ws::IPPROTO_IPV6, ws::IPV6_PKTINFO)?;
    }
    Ok(())
}

impl State {
    pub fn new(socket: &UdpSocket) -> io::Result<Self> {
        let mut recv_msg: ws::LPFN_WSARECVMSG = None;
        let mut returned = 0;
        let guid = ws::WSAID_WSARECVMSG;
        // SAFETY: input/output buffers have the exact GUID/function-pointer
        // sizes and live until the synchronous WSAIoctl call returns.
        let result = unsafe {
            ws::WSAIoctl(
                socket_handle(socket),
                ws::SIO_GET_EXTENSION_FUNCTION_POINTER,
                Some((&guid as *const windows::core::GUID).cast()),
                size_of::<windows::core::GUID>() as u32,
                Some((&mut recv_msg as *mut ws::LPFN_WSARECVMSG).cast()),
                size_of::<ws::LPFN_WSARECVMSG>() as u32,
                &mut returned,
                None,
                None,
            )
        };
        if result == ws::SOCKET_ERROR {
            return Err(last_error());
        }
        if returned as usize != size_of::<ws::LPFN_WSARECVMSG>() || recv_msg.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "WSARecvMsg is unavailable",
            ));
        }
        let is_v6 = socket.local_addr()?.is_ipv6();
        let only_v6 = is_v6 && socket2::SockRef::from(socket).only_v6()?;
        configure_packet_info(
            is_v6,
            only_v6,
            |level, option| enable(socket, level, option),
            || {
                // This temporary capability probe runs only on the documented
                // startup failure path. RAII closes a successfully opened
                // socket.
                socket2::Socket::new(
                    socket2::Domain::IPV4,
                    socket2::Type::DGRAM,
                    Some(socket2::Protocol::UDP),
                )
                .map(drop)
            },
        )?;
        Ok(Self { recv_msg })
    }

    pub fn recv(&self, socket: &UdpSocket, buf: &mut [u8]) -> io::Result<(usize, UdpReplyTarget)> {
        let mut control = Control::new();
        let mut data = ws::WSABUF {
            len: buf
                .len()
                .try_into()
                .map_err(|_| invalid_data("UDP receive buffer is too large"))?,
            buf: PSTR(buf.as_mut_ptr()),
        };
        let mut message = ws::WSAMSG {
            lpBuffers: &mut data,
            dwBufferCount: 1,
            Control: ws::WSABUF {
                len: control.bytes.len() as u32,
                buf: PSTR(control.bytes.as_mut_ptr()),
            },
            ..Default::default()
        };
        let recv_msg = self
            .recv_msg
            .expect("WSARecvMsg was validated at initialization");
        // SAFETY: recv_msg belongs to this socket's provider. All message
        // pointers refer to writable storage alive through the synchronous
        // call.
        let (len, peer) = unsafe {
            SockAddr::try_init(|storage, addr_len| {
                message.name = storage.cast();
                message.namelen = *addr_len;
                let mut received = 0;
                if recv_msg(
                    socket_handle(socket),
                    &mut message,
                    &mut received,
                    std::ptr::null_mut(),
                    None,
                ) == ws::SOCKET_ERROR
                {
                    return Err(last_error());
                }
                *addr_len = message.namelen;
                Ok(received as usize)
            })
        }?;
        if message.dwFlags & ws::MSG_TRUNC != 0 || len > buf.len() {
            return Err(invalid_data("Truncated UDP datagram"));
        }
        let (local_ip, interface_index) = packet_info(
            &control,
            message.Control.len as usize,
            message.dwFlags & ws::MSG_CTRUNC != 0,
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
        let mut control = reply_control(target);
        let peer = SockAddr::from(target.peer);
        let mut data = ws::WSABUF {
            len: bytes
                .len()
                .try_into()
                .map_err(|_| invalid_data("UDP reply is too large"))?,
            buf: PSTR(bytes.as_ptr().cast_mut()),
        };
        let message = ws::WSAMSG {
            name: peer.as_ptr().cast_mut().cast(),
            namelen: peer.len(),
            lpBuffers: &mut data,
            dwBufferCount: 1,
            Control: ws::WSABUF {
                len: control.len as u32,
                buf: PSTR(control.bytes.as_mut_ptr()),
            },
            dwFlags: 0,
        };
        let mut sent = 0;
        // SAFETY: all buffers remain live and WSASendMsg only reads their
        // contents. Null OVERLAPPED requests synchronous non-blocking I/O.
        if unsafe {
            ws::WSASendMsg(
                socket_handle(socket),
                &message,
                0,
                Some(&mut sent),
                None,
                None,
            )
        } == ws::SOCKET_ERROR
        {
            return Err(last_error());
        }
        Ok(sent as usize)
    }
}

fn packet_info(control: &Control, len: usize, truncated: bool) -> io::Result<(IpAddr, u32)> {
    let mut destination = None;
    for message in control.messages(len, truncated)? {
        let (level, kind, bytes) = message?;
        if level == ws::IPPROTO_IP.0 && kind == ws::IP_PKTINFO {
            // SAFETY: IN_PKTINFO and its address union contain only integers.
            let info = unsafe { decode::<ws::IN_PKTINFO>(bytes) }?;
            let ip = unsafe { info.ipi_addr.S_un.S_addr }.to_ne_bytes();
            destination = Some((IpAddr::V4(Ipv4Addr::from(ip)), info.ipi_ifindex));
        } else if level == ws::IPPROTO_IPV6.0 && kind == ws::IPV6_PKTINFO {
            // SAFETY: IN6_PKTINFO and its address union contain only integers.
            let info = unsafe { decode::<ws::IN6_PKTINFO>(bytes) }?;
            let ip = unsafe { info.ipi6_addr.u.Byte };
            destination = Some((IpAddr::V6(Ipv6Addr::from(ip)), info.ipi6_ifindex));
        }
    }
    destination.ok_or_else(|| invalid_data("Missing UDP packet destination address"))
}

fn reply_control(target: UdpReplyTarget) -> Control {
    let mut control = Control::new();
    match target.local_ip {
        IpAddr::V4(ip) => {
            let mut info = ws::IN_PKTINFO::default();
            info.ipi_addr.S_un.S_addr = u32::from_ne_bytes(ip.octets());
            // SAFETY: IN_PKTINFO has no padding and both fields are
            // initialized.
            unsafe { control.push(ws::IPPROTO_IP.0, ws::IP_PKTINFO, info) };
        }
        IpAddr::V6(ip) => {
            let mut info = ws::IN6_PKTINFO::default();
            info.ipi6_addr.u.Byte = ip.octets();
            info.ipi6_ifindex = target.outgoing_interface();
            // SAFETY: IN6_PKTINFO has no padding and both fields are
            // initialized.
            unsafe { control.push(ws::IPPROTO_IPV6.0, ws::IPV6_PKTINFO, info) };
        }
    }
    control
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_information_setup_respects_socket_families() {
        let v4 = (ws::IPPROTO_IP, ws::IP_PKTINFO);
        let v6 = (ws::IPPROTO_IPV6, ws::IPV6_PKTINFO);
        for (is_v6, only_v6, expected) in [
            (false, false, vec![v4]),
            (true, true, vec![v6]),
            (true, false, vec![v4, v6]),
        ] {
            let mut enabled = Vec::new();
            configure_packet_info(
                is_v6,
                only_v6,
                |level, option| {
                    enabled.push((level, option));
                    Ok(())
                },
                || panic!("Successful setup must not probe IPv4"),
            )
            .unwrap();
            assert_eq!(enabled, expected);
        }
    }

    #[test]
    fn dual_stack_setup_only_tolerates_confirmed_ipv4_absence() {
        for (is_v6, option_error, probe_error, accepted, should_probe) in [
            (true, ws::WSAEINVAL, Some(ws::WSAEAFNOSUPPORT), true, true),
            (true, ws::WSAEINVAL, None, false, true),
            (true, ws::WSAEINVAL, Some(ws::WSAEMFILE), false, true),
            (true, ws::WSAEINVAL, Some(ws::WSAEACCES), false, true),
            (
                true,
                ws::WSAENOPROTOOPT,
                Some(ws::WSAEAFNOSUPPORT),
                false,
                false,
            ),
            (
                false,
                ws::WSAEINVAL,
                Some(ws::WSAEAFNOSUPPORT),
                false,
                false,
            ),
        ] {
            let mut probed = false;
            let mut ipv6_enabled = false;
            let result = configure_packet_info(
                is_v6,
                false,
                |level, _| {
                    if level == ws::IPPROTO_IP {
                        return Err(io::Error::from_raw_os_error(option_error.0));
                    }
                    ipv6_enabled = true;
                    Ok(())
                },
                || {
                    probed = true;
                    match probe_error {
                        Some(err) => Err(io::Error::from_raw_os_error(err.0)),
                        None => Ok(()),
                    }
                },
            );
            assert_eq!(probed, should_probe);
            assert_eq!(ipv6_enabled, accepted);
            if accepted {
                result.unwrap();
            } else {
                assert_eq!(result.unwrap_err().raw_os_error(), Some(option_error.0));
            }
        }
    }

    #[test]
    fn ipv4_absence_does_not_hide_ipv6_packet_information_failure() {
        let result = configure_packet_info(
            true,
            false,
            |level, _| {
                Err(io::Error::from_raw_os_error(if level == ws::IPPROTO_IP {
                    ws::WSAEINVAL.0
                } else {
                    ws::WSAENOPROTOOPT.0
                }))
            },
            || Err(io::Error::from_raw_os_error(ws::WSAEAFNOSUPPORT.0)),
        );
        assert_eq!(
            result.unwrap_err().raw_os_error(),
            Some(ws::WSAENOPROTOOPT.0)
        );
    }

    #[test]
    fn packet_information_validates_and_round_trips_addresses() {
        assert!(packet_info(&Control::new(), 0, false).is_err());
        for (peer, local) in [
            ("[::ffff:192.0.2.2]:53", "::ffff:192.0.2.1"),
            ("[fe80::2%7]:53", "fe80::1"),
        ] {
            let target =
                UdpReplyTarget::new(peer.parse().unwrap(), local.parse().unwrap(), 7).unwrap();
            let control = reply_control(target);
            assert_eq!(
                packet_info(&control, control.len, false).unwrap(),
                (target.local_ip, target.outgoing_interface())
            );
            assert!(packet_info(&control, control.len, true).is_err());
        }
        let mut control = Control::new();
        // SAFETY: u32 has no padding or uninitialized bytes.
        unsafe { control.push(ws::IPPROTO_IPV6.0, ws::IPV6_PKTINFO, 0u32) };
        assert!(packet_info(&control, control.len, false).is_err());
    }
}
