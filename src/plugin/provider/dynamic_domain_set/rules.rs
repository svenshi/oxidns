// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::borrow::Cow;
use std::collections::HashSet;

use regex::RegexBuilder;
use serde::{Deserialize, Serialize};

use crate::core::error::{DnsError, Result as DnsResult};

/// Rule kind used when the caller supplies a bare domain instead of an
/// explicit `full:` or `domain:` expression.
///
/// Learned rules default to `Full` so the automatic path does not unexpectedly
/// widen a single successful query into a whole suffix rule. File bootstrap and
/// manual file loading use `Domain`, matching the long-standing `domain_set`
/// convention for bare domains.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DynamicDomainRuleKind {
    #[default]
    Full,
    Domain,
}

impl DynamicDomainRuleKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Domain => "domain",
        }
    }
}

/// Summary returned to synchronous callers and API responses after a mutation.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct DynamicDomainMutation {
    pub(crate) added: usize,
    pub(crate) removed: usize,
    pub(crate) total: usize,
}

pub(crate) fn learned_rule_for_domain(
    domain: &str,
    kind: DynamicDomainRuleKind,
) -> DnsResult<String> {
    // Centralize learned rule formatting so executor-side behavior and API-side
    // canonicalization stay aligned when new rule kinds are added later.
    let normalized = normalize_plain_domain(domain, "learned domain")?;
    Ok(format!("{}:{}", kind.prefix(), normalized))
}

pub(super) fn canonicalize_rules(
    raw_rules: Vec<String>,
    default_kind: DynamicDomainRuleKind,
    source: &str,
) -> DnsResult<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (idx, raw) in raw_rules.into_iter().enumerate() {
        let rule = canonicalize_rule(&raw, default_kind, &format!("{source}[{idx}]"))?;
        // Collapse duplicates within one request/file while preserving the
        // first occurrence order for list output and rewritten files.
        if seen.insert(rule.clone()) {
            out.push(rule);
        }
    }
    Ok(out)
}

pub(super) fn canonicalize_rule(
    raw: &str,
    default_kind: DynamicDomainRuleKind,
    source: &str,
) -> DnsResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DnsError::plugin(format!(
            "dynamic_domain_set {source} cannot be empty"
        )));
    }
    let has_explicit_prefix = has_domain_rule_prefix(trimmed);
    if !has_explicit_prefix {
        let normalized = normalize_plain_domain(trimmed, source)?;
        return Ok(format!("{}:{}", default_kind.prefix(), normalized));
    }

    // Store every non-regex domain expression in one canonical spelling. That
    // makes duplicate suppression independent of case and trailing dots.
    let (kind, value) = split_dynamic_rule_expression(trimmed);
    match kind {
        DynamicRuleExpressionKind::Full => {
            let normalized = normalize_plain_domain(value, source)?;
            Ok(format!("full:{normalized}"))
        }
        DynamicRuleExpressionKind::Domain => {
            let normalized = normalize_plain_domain(value, source)?;
            Ok(format!("domain:{normalized}"))
        }
        DynamicRuleExpressionKind::Keyword => {
            let normalized = normalize_plain_domain(value, source)?;
            Ok(format!("keyword:{normalized}"))
        }
        DynamicRuleExpressionKind::Regexp => {
            let value = value.trim();
            if value.is_empty() {
                return Err(DnsError::plugin(format!(
                    "dynamic_domain_set {source} has empty regexp expression"
                )));
            }
            RegexBuilder::new(value)
                .case_insensitive(true)
                .build()
                .map_err(|err| {
                    DnsError::plugin(format!(
                        "dynamic_domain_set {source} has invalid regexp expression '{value}': {err}"
                    ))
                })?;
            Ok(format!("regexp:{value}"))
        }
    }
}

fn has_domain_rule_prefix(raw: &str) -> bool {
    raw.starts_with("full:")
        || raw.starts_with("domain:")
        || raw.starts_with("keyword:")
        || raw.starts_with("regexp:")
}

#[derive(Debug, Clone, Copy)]
enum DynamicRuleExpressionKind {
    Full,
    Domain,
    Keyword,
    Regexp,
}

fn split_dynamic_rule_expression(raw: &str) -> (DynamicRuleExpressionKind, &str) {
    if let Some(value) = raw.strip_prefix("full:") {
        (DynamicRuleExpressionKind::Full, value)
    } else if let Some(value) = raw.strip_prefix("domain:") {
        (DynamicRuleExpressionKind::Domain, value)
    } else if let Some(value) = raw.strip_prefix("keyword:") {
        (DynamicRuleExpressionKind::Keyword, value)
    } else if let Some(value) = raw.strip_prefix("regexp:") {
        (DynamicRuleExpressionKind::Regexp, value)
    } else {
        (DynamicRuleExpressionKind::Domain, raw)
    }
}

fn normalize_plain_domain(raw: &str, source: &str) -> DnsResult<String> {
    let normalized = normalize_domain_cow(raw);
    if normalized.is_empty() {
        return Err(DnsError::plugin(format!(
            "dynamic_domain_set {source} has empty domain expression"
        )));
    }
    Ok(normalized.into_owned())
}

fn normalize_domain_cow(domain: &str) -> Cow<'_, str> {
    // Keep the common already-normalized path allocation-free. Learned qnames
    // arrive normalized most of the time, while manual/API input may need
    // trimming, lowercasing, and trailing-dot removal.
    let bytes = domain.as_bytes();
    let mut start = 0;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }

    let mut end = bytes.len();
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    while end > start && bytes[end - 1] == b'.' {
        end -= 1;
    }
    if start == end {
        return Cow::Borrowed("");
    }

    let slice = &domain[start..end];
    if slice.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(slice.to_ascii_lowercase())
    } else {
        Cow::Borrowed(slice)
    }
}
