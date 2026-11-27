// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared test utilities for plugin unit tests.

#![cfg(test)]
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use serde_yaml_ng::Value;

use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::Result;
use crate::plugin::{
    PluginCreateContext, PluginFactory, PluginInitContext, PluginRegistry, UninitializedPlugin,
};
use crate::proto::Message;

pub(crate) fn test_registry() -> Arc<PluginRegistry> {
    Arc::new(PluginRegistry::new())
}

pub(crate) fn plugin_config(
    tag: impl Into<String>,
    plugin_type: impl Into<String>,
    args: Option<Value>,
) -> PluginConfig {
    PluginConfig {
        tag: tag.into(),
        plugin_type: plugin_type.into(),
        args,
    }
}

pub(crate) fn create_plugin_for_test(
    factory: &dyn PluginFactory,
    plugin_config: &PluginConfig,
) -> Result<UninitializedPlugin> {
    let create_context = PluginCreateContext::default();
    let init_context =
        PluginInitContext::new(test_registry(), plugin_config.tag.clone(), &create_context);
    factory.create(plugin_config, &init_context)
}

pub(crate) fn test_context() -> DnsContext {
    DnsContext::new(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 5353)),
        Message::new(),
    )
}

/// Read the first question name back from the encoded wire message.
///
/// `Name::to_fqdn` returns the case-folded presentation form, so assertions
/// about the echoed case must inspect the bytes the client receives.
pub(crate) fn encoded_question_fqdn(message: &Message) -> String {
    let bytes = message.to_bytes().expect("message should encode");
    assert_eq!(
        u16::from_be_bytes([bytes[4], bytes[5]]),
        1,
        "test helper expects exactly one question"
    );

    let mut fqdn = String::new();
    let mut offset = 12;
    loop {
        let len = bytes[offset] as usize;
        if len == 0 {
            break;
        }
        if !fqdn.is_empty() {
            fqdn.push('.');
        }
        fqdn.push_str(
            std::str::from_utf8(&bytes[offset + 1..offset + 1 + len])
                .expect("question label should be ASCII"),
        );
        offset += 1 + len;
    }
    fqdn.push('.');
    fqdn
}
