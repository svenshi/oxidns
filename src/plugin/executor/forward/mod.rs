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
use crate::proto::{EdnsCode, Message};

const MAC_OPTION_CODE: EdnsCode = EdnsCode::Unknown(65001);

fn prepare_request(mut request: Message, forward_mac: bool) -> Message {
    if !forward_mac
        && request
            .edns()
            .as_ref()
            .is_some_and(|edns| edns.option(MAC_OPTION_CODE).is_some())
        && let Some(edns) = request.edns_mut().as_mut()
    {
        edns.remove(MAC_OPTION_CODE);
    }
    request
}

fn is_timeout_error(err: &DnsError) -> bool {
    err.to_string().to_ascii_lowercase().contains("timeout")
}

#[cfg(test)]
mod tests;
