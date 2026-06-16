// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Upstream DNS resolver infrastructure.
//!
//! This module builds outbound resolvers used by forwarding-style executors.
//! It turns upstream configuration into protocol-specific clients with shared
//! pooling, bootstrap resolution, timeout handling, and fallback behavior.

mod bootstrap;
mod builder;
mod config;
mod conn;
mod dial;
mod pool;
mod resolver;

pub use builder::UpstreamBuilder;
pub use config::{ConnectionInfo, ConnectionType, UpstreamConfig};
#[cfg(feature = "_http-client")]
pub(crate) use dial::connect_tcp_stream;
#[cfg(any(
    feature = "upstream-doh",
    feature = "upstream-doh3",
    feature = "upstream-doq"
))]
pub(crate) use pool::Connection;
pub use pool::QueryDeadline;
pub use resolver::Upstream;

#[cfg(test)]
mod tests;
