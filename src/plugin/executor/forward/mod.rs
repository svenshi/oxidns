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

fn is_timeout_error(err: &DnsError) -> bool {
    err.to_string().to_ascii_lowercase().contains("timeout")
}

#[cfg(test)]
mod tests;
