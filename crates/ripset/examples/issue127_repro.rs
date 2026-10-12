// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Minimal reproducer for issue #127.
//!
//! Usage (Linux, as root, with nftables present):
//!
//! ```sh
//! nft delete table ip proxy 2>/dev/null || true
//! nft add table ip proxy
//! nft 'add set ip proxy proxy_set { type ipv4_addr; flags interval; }'
//!
//! cargo run -p oxidns-ripset --example issue127_repro
//! nft list set ip proxy proxy_set
//! ```
//!
//! Before the issue #127 fix, every `nftset_add` call returned
//! `Netlink error: 22` (EINVAL) because the executor sent a single
//! NFTA_SET_ELEM_KEY + NFTA_SET_ELEM_KEY_END element, which some kernels
//! reject for interval sets. After the fix, the same operation is encoded
//! as the two-element form used by `nft`, and the IPs land in the kernel.

use std::net::IpAddr;

use ripset::{IpCidr, nftset_add};

fn main() {
    let ips = ["185.45.5.35", "1.2.3.4", "8.8.8.8"];
    for ip in ips {
        let addr: IpAddr = ip.parse().expect("parse");
        let cidr = IpCidr::new(addr, 32).expect("cidr");
        match nftset_add("ip", "proxy", "proxy_set", cidr) {
            Ok(()) => println!("OK   add {cidr}"),
            Err(e) => println!("FAIL add {cidr}: {e}"),
        }
    }
}
