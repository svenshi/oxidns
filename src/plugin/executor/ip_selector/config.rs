// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Configuration parsing for the `ip_selector` executor.
//!
//! This module owns the YAML and quick-setup surface. The rest of the plugin
//! consumes only validated `IpSelectorSettings`, so user-facing syntax stays
//! isolated from runtime policy/probe code.

use std::fmt::{Display, Formatter};
use std::time::Duration;

use ahash::AHashSet;
use serde::{Deserialize, Deserializer};
use serde_yaml_ng::Value;

use crate::infra::error::{DnsError, Result};

const DEFAULT_SELECTION_MODE: SelectionMode = SelectionMode::FirstSuccess;
const DEFAULT_PROBE_STAGGER_MS: u64 = 200;
const DEFAULT_PROBE_TIMEOUT_MS: u64 = 600;
const DEFAULT_MAX_WAIT_MS: u64 = 1000;
const DEFAULT_TOP_N: usize = 1;
const DEFAULT_MAX_PARALLEL_PROBES: usize = 256;
const DEFAULT_CACHE_ENABLED: bool = true;
const DEFAULT_CACHE_SIZE: usize = 4096;
const DEFAULT_CACHE_TTL_SECS: u64 = 3600;
const DEFAULT_FAILURE_TTL_SECS: u64 = 60;

/// Response selection strategy.
///
/// These modes intentionally describe response shaping only. Upstream racing is
/// a different layer and remains owned by `forward`/`fallback`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum SelectionMode {
    /// Return as soon as any active probe succeeds within `max_wait`.
    FirstSuccess,
    /// Wait up to `max_wait` and pick the lowest-latency successful probe.
    BestWithinBudget,
    /// Return the current response unchanged and warm probe cache
    /// asynchronously.
    Background,
}

impl SelectionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::FirstSuccess => "first_success",
            Self::BestWithinBudget => "best_within_budget",
            Self::Background => "background",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum DnssecPolicy {
    /// DNSSEC-sensitive responses may be reordered, but records are not
    /// removed.
    ReorderOnly,
    /// Leave DNSSEC-sensitive responses completely unchanged.
    Skip,
}

/// Active or passive scoring method for an IP candidate.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub(super) enum ProbeMethod {
    Tcp(u16),
    Ping,
    /// Passive mode: do not start new probes, only consume cached observations.
    None,
}

impl ProbeMethod {
    pub(super) fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

impl Display for ProbeMethod {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp(port) => write!(f, "tcp:{port}"),
            Self::Ping => f.write_str("ping"),
            Self::None => f.write_str("none"),
        }
    }
}

/// Internal config after deserialization, before defaults and validation.
///
/// This separate type lets quick setup and YAML config feed the same validation
/// path.
#[derive(Debug, Clone)]
pub(super) struct IpSelectorConfig {
    pub(super) selection_mode: Option<String>,
    pub(super) probe_methods: Option<Vec<String>>,
    pub(super) probe_stagger: Option<u64>,
    pub(super) probe_timeout: Option<u64>,
    pub(super) max_wait: Option<u64>,
    pub(super) top_n: Option<usize>,
    pub(super) reorder_only: Option<bool>,
    pub(super) dnssec_policy: Option<String>,
    pub(super) max_parallel_probes: Option<usize>,
    pub(super) cache: Option<IpSelectorCacheConfig>,
}

/// YAML-facing config.
///
/// Unknown fields are rejected deliberately. `ip_selector` exposes an
/// OxiDNS-native configuration surface and does not accept compatibility
/// aliases for fields or modes.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIpSelectorConfig {
    #[serde(default)]
    selection_mode: Option<String>,
    #[serde(default, deserialize_with = "deserialize_probe_methods")]
    probe_methods: Option<Vec<String>>,
    #[serde(default)]
    probe_stagger: Option<u64>,
    #[serde(default)]
    probe_timeout: Option<u64>,
    #[serde(default)]
    max_wait: Option<u64>,
    #[serde(default)]
    top_n: Option<usize>,
    #[serde(default)]
    reorder_only: Option<bool>,
    #[serde(default)]
    dnssec_policy: Option<String>,
    #[serde(default)]
    max_parallel_probes: Option<usize>,
    #[serde(default)]
    cache: Option<IpSelectorCacheConfig>,
}

impl From<RawIpSelectorConfig> for IpSelectorConfig {
    fn from(value: RawIpSelectorConfig) -> Self {
        Self {
            selection_mode: value.selection_mode,
            probe_methods: value.probe_methods,
            probe_stagger: value.probe_stagger,
            probe_timeout: value.probe_timeout,
            max_wait: value.max_wait,
            top_n: value.top_n,
            reorder_only: value.reorder_only,
            dnssec_policy: value.dnssec_policy,
            max_parallel_probes: value.max_parallel_probes,
            cache: value.cache,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct IpSelectorCacheConfig {
    pub(super) enabled: Option<bool>,
    pub(super) size: Option<usize>,
    pub(super) ttl: Option<u64>,
    pub(super) failure_ttl: Option<u64>,
}

/// Fully validated runtime settings.
///
/// Durations are normalized to `Duration`, TTLs are stored as milliseconds to
/// match `TtlCache`/`AppClock`, and method lists have already been
/// de-duplicated.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct IpSelectorSettings {
    pub(super) selection_mode: SelectionMode,
    pub(super) probe_methods: Vec<ProbeMethod>,
    pub(super) probe_stagger: Duration,
    pub(super) probe_timeout: Duration,
    pub(super) max_wait: Duration,
    pub(super) top_n: usize,
    pub(super) dnssec_policy: DnssecPolicy,
    pub(super) max_parallel_probes: usize,
    pub(super) cache_enabled: bool,
    pub(super) cache_size: usize,
    pub(super) cache_ttl_ms: u64,
    pub(super) failure_ttl_ms: u64,
}

pub(super) fn parse_ip_selector_config(args: Option<Value>) -> Result<IpSelectorSettings> {
    // Both full YAML config and omitted args flow through the same defaults and
    // validation to avoid quick/default behavior drifting over time.
    let config = match args {
        Some(args) => serde_yaml_ng::from_value::<RawIpSelectorConfig>(args)
            .map(IpSelectorConfig::from)
            .map_err(|e| DnsError::plugin(format!("failed to parse ip_selector config: {}", e)))?,
        None => IpSelectorConfig {
            selection_mode: None,
            probe_methods: None,
            probe_stagger: None,
            probe_timeout: None,
            max_wait: None,
            top_n: None,
            reorder_only: None,
            dnssec_policy: None,
            max_parallel_probes: None,
            cache: None,
        },
    };

    settings_from_config(config)
}

pub(super) fn parse_ip_selector_quick_setup(param: Option<String>) -> Result<IpSelectorSettings> {
    let raw = param.unwrap_or_default();
    let mut tokens: Vec<&str> = raw.split_whitespace().collect();
    let mut config = IpSelectorConfig {
        selection_mode: None,
        probe_methods: None,
        probe_stagger: None,
        probe_timeout: None,
        max_wait: None,
        top_n: None,
        reorder_only: None,
        dnssec_policy: None,
        max_parallel_probes: None,
        cache: None,
    };

    if let Some(first) = tokens.first().copied()
        && let Some(mode) = try_parse_selection_mode(first)?
    {
        // The first positional token may be a selection mode. Remaining tokens,
        // if any, are interpreted as probe methods.
        config.selection_mode = Some(mode.as_str().to_string());
        tokens.remove(0);
    }

    if !tokens.is_empty() {
        config.probe_methods = Some(tokens.into_iter().map(ToString::to_string).collect());
    }

    settings_from_config(config)
}

pub(super) fn settings_from_config(config: IpSelectorConfig) -> Result<IpSelectorSettings> {
    let selection_mode = match config.selection_mode {
        Some(raw) => parse_selection_mode(&raw)?,
        None => DEFAULT_SELECTION_MODE,
    };
    let probe_methods = match config.probe_methods {
        Some(raw) => parse_probe_methods(raw)?,
        None => vec![ProbeMethod::Tcp(443), ProbeMethod::Tcp(80)],
    };
    let dnssec_policy = match config.dnssec_policy {
        Some(raw) => parse_dnssec_policy(&raw)?,
        // `reorder_only` is accepted for compatibility with the earlier plan,
        // but the explicit `dnssec_policy` field is preferred going forward.
        None if config.reorder_only.unwrap_or(false) => DnssecPolicy::ReorderOnly,
        None => DnssecPolicy::ReorderOnly,
    };

    let cache = config.cache.unwrap_or_default();
    let cache_enabled = cache.enabled.unwrap_or(DEFAULT_CACHE_ENABLED);
    let cache_size = cache.size.unwrap_or(DEFAULT_CACHE_SIZE);
    let cache_ttl_secs = cache.ttl.unwrap_or(DEFAULT_CACHE_TTL_SECS);
    let failure_ttl_secs = cache.failure_ttl.unwrap_or(DEFAULT_FAILURE_TTL_SECS);
    let max_parallel_probes = config
        .max_parallel_probes
        .unwrap_or(DEFAULT_MAX_PARALLEL_PROBES);
    let probe_timeout = config.probe_timeout.unwrap_or(DEFAULT_PROBE_TIMEOUT_MS);
    let max_wait = config.max_wait.unwrap_or(DEFAULT_MAX_WAIT_MS);

    if probe_timeout == 0 {
        return Err(DnsError::plugin(
            "ip_selector probe_timeout must be greater than 0 milliseconds",
        ));
    }
    if max_wait == 0 {
        return Err(DnsError::plugin(
            "ip_selector max_wait must be greater than 0 milliseconds",
        ));
    }
    if max_parallel_probes == 0 {
        return Err(DnsError::plugin(
            "ip_selector max_parallel_probes must be greater than 0",
        ));
    }
    if cache_enabled && cache_size == 0 {
        return Err(DnsError::plugin(
            "ip_selector cache.size must be greater than 0",
        ));
    }
    if cache_enabled && cache_ttl_secs == 0 {
        return Err(DnsError::plugin(
            "ip_selector cache.ttl must be greater than 0 seconds",
        ));
    }
    if cache_enabled && failure_ttl_secs == 0 {
        return Err(DnsError::plugin(
            "ip_selector cache.failure_ttl must be greater than 0 seconds",
        ));
    }

    Ok(IpSelectorSettings {
        selection_mode,
        probe_methods,
        probe_stagger: Duration::from_millis(
            config.probe_stagger.unwrap_or(DEFAULT_PROBE_STAGGER_MS),
        ),
        probe_timeout: Duration::from_millis(probe_timeout),
        max_wait: Duration::from_millis(max_wait),
        top_n: config.top_n.unwrap_or(DEFAULT_TOP_N),
        dnssec_policy,
        max_parallel_probes,
        cache_enabled,
        cache_size,
        // Saturating conversion avoids accidental overflow in long TTL configs;
        // an overlarge TTL behaves as "effectively long-lived" rather than
        // wrapping to a tiny value.
        cache_ttl_ms: cache_ttl_secs.saturating_mul(1000),
        failure_ttl_ms: failure_ttl_secs.saturating_mul(1000),
    })
}

fn parse_selection_mode(raw: &str) -> Result<SelectionMode> {
    match raw.trim() {
        "first_success" => Ok(SelectionMode::FirstSuccess),
        "best_within_budget" => Ok(SelectionMode::BestWithinBudget),
        "background" => Ok(SelectionMode::Background),
        _ => Err(DnsError::plugin(format!(
            "invalid ip_selector selection_mode '{}'",
            raw
        ))),
    }
}

fn try_parse_selection_mode(raw: &str) -> Result<Option<SelectionMode>> {
    // Quick setup is positional. Returning `None` for unknown tokens lets the
    // first token be treated as a probe-method list instead of producing a mode
    // error too early.
    match raw.trim() {
        "first_success" | "best_within_budget" | "background" => {
            parse_selection_mode(raw).map(Some)
        }
        _ => Ok(None),
    }
}

fn parse_dnssec_policy(raw: &str) -> Result<DnssecPolicy> {
    match raw.trim() {
        "reorder_only" => Ok(DnssecPolicy::ReorderOnly),
        "skip" => Ok(DnssecPolicy::Skip),
        _ => Err(DnsError::plugin(format!(
            "invalid ip_selector dnssec_policy '{}'",
            raw
        ))),
    }
}

fn parse_probe_methods(raw: Vec<String>) -> Result<Vec<ProbeMethod>> {
    let tokens = split_method_tokens(raw);
    if tokens.is_empty() {
        return Err(DnsError::plugin(
            "ip_selector probe_methods must not be empty",
        ));
    }

    let mut methods = Vec::new();
    let mut seen = AHashSet::new();
    for token in tokens {
        let method = parse_probe_method(&token)?;
        // Preserve configured order while removing duplicates. Order matters
        // because `probe_stagger` uses method index as method preference.
        if seen.insert(method) {
            methods.push(method);
        }
    }

    if methods.len() > 1 && methods.contains(&ProbeMethod::None) {
        return Err(DnsError::plugin(
            "ip_selector probe method 'none' cannot be combined with other methods",
        ));
    }

    Ok(methods)
}

fn split_method_tokens(raw: Vec<String>) -> Vec<String> {
    // YAML may provide either ["tcp:443", "tcp:80"] or
    // ["tcp:443,tcp:80"]. Quick setup also feeds whitespace-separated chunks
    // through this path.
    raw.into_iter()
        .flat_map(|item| {
            item.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn parse_probe_method(raw: &str) -> Result<ProbeMethod> {
    let trimmed = raw.trim();
    if trimmed == "ping" {
        return Ok(ProbeMethod::Ping);
    }
    if trimmed == "none" {
        return Ok(ProbeMethod::None);
    }
    let Some(port) = trimmed.strip_prefix("tcp:") else {
        return Err(DnsError::plugin(format!(
            "invalid ip_selector probe method '{}'",
            raw
        )));
    };
    let port = port.parse::<u16>().map_err(|_| {
        DnsError::plugin(format!(
            "invalid ip_selector tcp probe port in method '{}'",
            raw
        ))
    })?;
    if port == 0 {
        return Err(DnsError::plugin(
            "ip_selector tcp probe port must be greater than 0",
        ));
    }
    Ok(ProbeMethod::Tcp(port))
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawProbeMethods {
    One(String),
    Many(Vec<String>),
}

fn deserialize_probe_methods<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<RawProbeMethods>::deserialize(deserializer)?;
    Ok(raw.map(|raw| match raw {
        RawProbeMethods::One(item) => vec![item],
        RawProbeMethods::Many(items) => items,
    }))
}
