// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use ahash::AHashSet;
use serde::Deserialize;
use serde_yaml_ng::Value;

use crate::config::types::PluginConfig;
use crate::infra::error::{DnsError, Result};
use crate::infra::system::parse_simple_duration;
use crate::plugin::provider::dynamic_domain_set::DynamicDomainRuleKind;
use crate::proto::RecordType;

pub(super) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);

/// Raw YAML arguments for `learn_domain`.
///
/// The executor is deliberately narrow: it only learns request qnames and only
/// writes into `dynamic_domain_set`, keeping persistence concerns out of the
/// sequence engine and out of static providers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LearnDomainArgs {
    /// Target `dynamic_domain_set` provider tag.
    provider: String,
    /// Whether to learn before or after the downstream executor chain.
    phase: Option<LearnPhase>,
    /// Whether multi-question messages learn only the first or every question.
    questions: Option<QuestionMode>,
    /// DNS qtypes eligible for learning; defaults to A/AAAA.
    qtypes: Option<Vec<String>>,
    /// After-phase guard: require NOERROR.
    success_only: Option<bool>,
    /// After-phase guard: require at least one answer record.
    answer_required: Option<bool>,
    /// Rule kind generated for learned qnames.
    rule_kind: Option<DynamicDomainRuleKind>,
    /// Fire-and-forget enqueue by default; sync mode waits for provider flush.
    #[serde(rename = "async")]
    async_mode: Option<bool>,
    /// How DNS flow should react when learning fails.
    error_mode: Option<LearnErrorMode>,
    /// Sync-mode wait budget.
    timeout: Option<String>,
}

/// Learning point relative to the downstream executor chain.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum LearnPhase {
    /// Learn from the request before `next` runs. Response guards are ignored.
    Before,
    /// Run `next` first, then learn only if response guards pass.
    #[default]
    After,
}

/// Controls how many questions from a DNS message are considered.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum QuestionMode {
    /// The normal DNS case; avoids surprising duplicate work on malformed or
    /// unusual multi-question messages.
    #[default]
    First,
    /// Useful for explicit multi-question tests or specialized clients.
    All,
}

/// Failure policy for provider enqueue/write errors.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum LearnErrorMode {
    /// Keep DNS resolution successful even if the side effect fails.
    #[default]
    Continue,
    /// Stop the current sequence branch without turning the DNS flow into an
    /// error.
    Stop,
    /// Bubble a plugin error to the caller.
    Fail,
}

/// Fully resolved runtime configuration.
#[derive(Debug, Clone)]
pub(super) struct LearnDomainConfig {
    pub(super) provider_tag: String,
    pub(super) phase: LearnPhase,
    pub(super) questions: QuestionMode,
    /// Parsed qtypes as wire values for cheap membership checks on the hot
    /// path.
    pub(super) qtypes: AHashSet<u16>,
    pub(super) success_only: bool,
    pub(super) answer_required: bool,
    pub(super) rule_kind: DynamicDomainRuleKind,
    pub(super) async_mode: bool,
    pub(super) error_mode: LearnErrorMode,
    pub(super) timeout: Duration,
}

pub(super) fn parse_provider_from_value(args: Option<Value>) -> Result<String> {
    let args = args.ok_or_else(|| DnsError::plugin("learn_domain requires structured args"))?;
    let raw = serde_yaml_ng::from_value::<LearnDomainArgs>(args)
        .map_err(|err| DnsError::plugin(format!("failed to parse learn_domain config: {err}")))?;
    let provider = raw.provider.trim();
    if provider.is_empty() {
        return Err(DnsError::plugin("learn_domain provider cannot be empty"));
    }
    Ok(provider.to_string())
}

pub(super) fn build_config(plugin_config: &PluginConfig) -> Result<LearnDomainConfig> {
    let args = plugin_config
        .args
        .clone()
        .ok_or_else(|| DnsError::plugin("learn_domain requires structured args"))?;
    let raw = serde_yaml_ng::from_value::<LearnDomainArgs>(args)
        .map_err(|err| DnsError::plugin(format!("failed to parse learn_domain config: {err}")))?;
    let provider_tag = raw.provider.trim();
    if provider_tag.is_empty() {
        return Err(DnsError::plugin("learn_domain provider cannot be empty"));
    }
    let qtypes = parse_qtypes(raw.qtypes)?;
    let timeout = parse_timeout(raw.timeout.as_deref())?;
    Ok(LearnDomainConfig {
        provider_tag: provider_tag.to_string(),
        phase: raw.phase.unwrap_or_default(),
        questions: raw.questions.unwrap_or_default(),
        qtypes,
        success_only: raw.success_only.unwrap_or(true),
        answer_required: raw.answer_required.unwrap_or(true),
        rule_kind: raw.rule_kind.unwrap_or_default(),
        async_mode: raw.async_mode.unwrap_or(true),
        error_mode: raw.error_mode.unwrap_or_default(),
        timeout,
    })
}

pub(super) fn parse_qtypes(raw: Option<Vec<String>>) -> Result<AHashSet<u16>> {
    let raw = raw.unwrap_or_else(|| vec!["A".to_string(), "AAAA".to_string()]);
    if raw.is_empty() {
        return Err(DnsError::plugin(
            "learn_domain qtypes must contain at least one qtype",
        ));
    }
    let mut out = AHashSet::with_capacity(raw.len());
    for (idx, value) in raw.iter().enumerate() {
        let token = value.trim();
        if token.is_empty() {
            return Err(DnsError::plugin(format!(
                "learn_domain qtypes[{idx}] cannot be empty"
            )));
        }
        let qtype = RecordType::from_token(token)
            .map(u16::from)
            .ok_or_else(|| {
                DnsError::plugin(format!(
                    "learn_domain qtypes[{idx}] has unsupported qtype '{}'",
                    token
                ))
            })?;
        out.insert(qtype);
    }
    // Returning a set makes repeated qtype entries harmless and keeps per-query
    // filtering to a single hash lookup.
    Ok(out)
}

fn parse_timeout(raw: Option<&str>) -> Result<Duration> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(DEFAULT_TIMEOUT);
    };
    parse_simple_duration(raw).map_err(|err| {
        DnsError::plugin(format!(
            "learn_domain timeout is invalid '{}': {}",
            raw, err
        ))
    })
}
