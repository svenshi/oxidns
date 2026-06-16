// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Protocol-specific upstream connection implementations.

use std::sync::atomic::{AtomicU16, Ordering};

#[cfg(any(feature = "upstream-doh", feature = "upstream-doh3"))]
pub(crate) mod doh;
#[cfg(feature = "upstream-doh")]
pub(crate) mod h2;
#[cfg(feature = "upstream-doh3")]
pub(crate) mod h3;
#[cfg(feature = "upstream-doq")]
pub(crate) mod quic;
pub(crate) mod request_map;
pub(crate) mod tcp;
pub(crate) mod udp;

#[cfg(feature = "upstream-doh")]
pub(crate) use h2::{H2Connection, H2ConnectionBuilder};
#[cfg(feature = "upstream-doh3")]
pub(crate) use h3::{H3Connection, H3ConnectionBuilder};
#[cfg(feature = "upstream-doq")]
pub(crate) use quic::{QuicConnection, QuicConnectionBuilder};
pub(crate) use tcp::{TcpConnection, TcpConnectionBuilder};
pub(crate) use udp::{UdpConnection, UdpConnectionBuilder};

/// RAII guard that decrements a connection's in-flight query counter on drop.
///
/// Ensures `using_count` is always decremented even when the query future is
/// cancelled by an outer timeout, preventing the pool from permanently
/// deadlocking due to a leaked counter.
#[allow(dead_code)]
pub(crate) struct UsingCountGuard<'a>(pub(crate) &'a AtomicU16);

impl Drop for UsingCountGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}
