// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::Deserialize;

use super::selection::ResponseSelectionMode;
use crate::config::types::PluginConfig;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::upstream::{ConnectionInfo, UpstreamConfig};

pub(super) const MAX_CONCURRENT_QUERIES: usize = 32;

/// Forward plugin configuration
#[derive(Deserialize)]
#[allow(unused)]
pub struct ForwardConfig {
    /// Number of upstreams to query concurrently in multi-upstream mode.
    ///
    /// Defaults to `1`, and clamped to `1..=32` and the upstream count.
    pub concurrent: Option<usize>,

    /// Concurrent upstream response selection mode.
    ///
    /// Defaults to `balanced`.
    #[serde(default)]
    pub response_selection: ResponseSelectionMode,

    /// List of upstream DNS servers
    pub upstreams: Vec<UpstreamConfig>,

    /// Whether to stop the executor chain after a successful upstream response.
    #[serde(default)]
    pub short_circuit: bool,
}

pub(super) fn parse_forward_config(plugin_config: &PluginConfig) -> Result<ForwardConfig> {
    let cfg = plugin_config.args.clone().ok_or_else(|| {
        DnsError::plugin("forward plugin requires 'concurrent' and 'upstreams' configuration")
    })?;
    let cfg = serde_yaml_ng::from_value::<ForwardConfig>(cfg)
        .map_err(|e| DnsError::plugin(format!("failed to parse forward plugin config: {}", e)))?;
    validate_forward_config(&cfg)?;
    Ok(cfg)
}

fn validate_forward_config(cfg: &ForwardConfig) -> Result<()> {
    if cfg.upstreams.is_empty() {
        return Err(DnsError::plugin(
            "forward plugin requires at least one upstream",
        ));
    }

    for (idx, upstream) in cfg.upstreams.iter().enumerate() {
        validate_upstream_addr(&upstream.addr).map_err(|e| {
            DnsError::plugin(format!(
                "forward plugin upstream[{}] addr '{}' is invalid: {}",
                idx, upstream.addr, e
            ))
        })?;
    }

    Ok(())
}

pub(super) fn validate_upstream_addr(addr: &str) -> std::result::Result<(), String> {
    ConnectionInfo::validate_addr(addr).map_err(|e| e.to_string())
}

#[inline]
pub(super) fn resolve_active_concurrent(
    concurrent: Option<usize>,
    total_upstreams: usize,
) -> usize {
    let upper = total_upstreams.clamp(1, MAX_CONCURRENT_QUERIES);
    concurrent.unwrap_or(1).clamp(1, upper)
}

pub(super) fn parse_quick_setup_param(param: Option<String>) -> Result<(Vec<String>, bool)> {
    let param = param.ok_or_else(|| {
        DnsError::plugin("forward quick setup requires non-empty upstream address parameter")
    })?;
    let (param, short_circuit) = strip_short_circuit_suffix(&param)?;
    let upstream_addrs: Vec<String> = param
        .split_whitespace()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    if upstream_addrs.is_empty() {
        return Err(DnsError::plugin(
            "forward quick setup requires non-empty upstream address parameter",
        ));
    }
    Ok((upstream_addrs, short_circuit))
}

fn strip_short_circuit_suffix(raw: &str) -> Result<(String, bool)> {
    let mut tokens: Vec<&str> = raw.split_whitespace().collect();
    let mut short_circuit = false;

    while let Some(last) = tokens.last().copied() {
        let Some(value) = parse_short_circuit_token(last)? else {
            break;
        };
        short_circuit = value;
        tokens.pop();
    }

    Ok((tokens.join(" "), short_circuit))
}

fn parse_short_circuit_token(token: &str) -> Result<Option<bool>> {
    if token == "short_circuit" {
        return Ok(Some(true));
    }

    let Some(value) = token.strip_prefix("short_circuit=") else {
        return Ok(None);
    };

    match value {
        "true" => Ok(Some(true)),
        "false" => Ok(Some(false)),
        _ => Err(DnsError::plugin(format!(
            "invalid short_circuit value '{}', expected true or false",
            value
        ))),
    }
}

#[inline]
pub(super) fn make_default_upstream_config(addr: String) -> UpstreamConfig {
    UpstreamConfig {
        tag: None,
        addr,
        outbound: None,
        dial_addr: None,
        port: None,
        bootstrap: None,
        bootstrap_version: None,
        socks5: None,
        idle_timeout: None,
        max_conns: None,
        min_conns: None,
        insecure_skip_verify: None,
        timeout: None,
        enable_pipeline: None,
        enable_http3: None,
        so_mark: None,
        bind_to_device: None,
    }
}
