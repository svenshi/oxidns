// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! DNS forwarding plugin.
//!
//! Forwards DNS queries to configured upstream resolvers.

mod concurrent;
mod config;
mod factory;
mod metrics;
mod selection;
mod single;

pub use config::ForwardConfig;
pub use factory::ForwardFactory;
pub use selection::ResponseSelectionMode;

use crate::infra::error::DnsError;
use crate::infra::network::upstream::ConnectionInfo;

fn is_timeout_error(err: &DnsError) -> bool {
    err.to_string().to_ascii_lowercase().contains("timeout")
}

/// Display identity for an upstream in logs: `tag(addr)` when a tag is
/// configured, otherwise just the configured address (`raw_addr` mirrors the
/// `addr` field in the YAML config).
fn upstream_log_name(info: &ConnectionInfo) -> String {
    match info.tag.as_deref() {
        Some(tag) => format!("{tag}({})", info.raw_addr),
        None => info.raw_addr.clone(),
    }
}

#[cfg(test)]
mod tests;
