// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared IP address normalization helpers.

use std::net::{IpAddr, SocketAddr, SocketAddrV4};

/// Convert IPv4-mapped IPv6 addresses into plain IPv4 addresses.
pub(crate) fn normalize_ipv4_mapped_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => IpAddr::V4(v4),
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
    }
}

/// Convert socket addresses with IPv4-mapped IPv6 hosts into IPv4 sockets.
pub(crate) fn normalize_ipv4_mapped_socket_addr(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V4(_) => addr,
        SocketAddr::V6(v6) => match normalize_ipv4_mapped_ip(IpAddr::V6(*v6.ip())) {
            IpAddr::V4(ipv4) => SocketAddr::V4(SocketAddrV4::new(ipv4, v6.port())),
            IpAddr::V6(_) => SocketAddr::V6(v6),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV6};

    use super::*;

    #[test]
    fn normalize_ipv4_mapped_ip_converts_mapped_v6() {
        let mapped = IpAddr::V6(Ipv6Addr::from(0xFFFF_0A00_0001u128));

        assert_eq!(
            normalize_ipv4_mapped_ip(mapped),
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))
        );
    }

    #[test]
    fn normalize_ipv4_mapped_socket_addr_converts_mapped_v6() {
        let mapped = SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from(0xFFFF_0A00_0001u128),
            5353,
            0,
            0,
        ));

        assert_eq!(
            normalize_ipv4_mapped_socket_addr(mapped),
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 5353))
        );
    }
}
