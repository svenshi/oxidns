// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later
//! High-performance IP/CIDR set provider.
//!
//! Design goals:
//! - Constant-time-ish membership checks on hot path.
//! - Unified IPv4/IPv6 semantics.
//! - Local matcher plus stable referenced providers for composed sets.
//! - Precise parse errors for file-based rules.

use std::any::Any;
use std::fmt::Debug;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, info};

use crate::config::types::PluginConfig;
use crate::core::rule_matcher::{IpPrefixMatcher, IpRuleFamily};
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result as DnsResult};
use crate::infra::io::{LineClassifier, TextLine, TextLocation, TextSource};
use crate::infra::task::spawn_isolated_build;
use crate::plugin::dependency::DependencySpec;
use crate::plugin::provider::{Provider, ProviderRuleStats, ProviderRuntimeStatus};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;

#[derive(Debug, Clone, Deserialize, Default)]
struct IpSetArgs {
    /// Inline ip/cidr rules.
    #[serde(default)]
    ips: Vec<String>,
    /// Referenced ip_set plugin tags.
    #[serde(default)]
    sets: Vec<String>,
    /// Text files containing one ip/cidr rule per line.
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Default)]
struct IpSetSnapshot {
    matcher: IpPrefixMatcher,
    /// Family-level fast guards to avoid useless scans.
    has_v4_rules: bool,
    has_v6_rules: bool,
    stats: IpSetRuleStats,
}

#[derive(Debug, Default, Clone, Copy)]
struct IpSetRuleStats {
    v4_rules: usize,
    v6_rules: usize,
}

impl IpSetRuleStats {
    fn total_rules(self) -> usize {
        self.v4_rules + self.v6_rules
    }
}

#[derive(Debug, Default, Clone)]
struct ProviderReloadState {
    last_reload_ms: Option<u64>,
    last_error: Option<String>,
}

#[derive(Debug)]
pub struct IpSet {
    tag: String,
    args: Arc<IpSetArgs>,
    referenced_sets: Vec<Arc<dyn Provider>>,
    snapshot: ArcSwap<IpSetSnapshot>,
    reload_state: Mutex<ProviderReloadState>,
}

impl IpSet {
    fn build_local_snapshot(tag: &str, args: &IpSetArgs) -> DnsResult<IpSetSnapshot> {
        let start_ms = AppClock::elapsed_millis();
        let (local_rules, v4_capacity, v6_capacity) = count_ip_rules(args)?;
        let mut matcher = IpPrefixMatcher::default();
        matcher.reserve_rules(v4_capacity, v6_capacity);
        scan_ip_rules(args, |raw, source| {
            add_ip_rule_from_source(&mut matcher, raw, source)
        })?;
        matcher.finalize_compact();

        let has_v4_rules = matcher.has_v4_rules();
        let has_v6_rules = matcher.has_v6_rules();
        let total_v4_rules = matcher.v4_rule_count();
        let total_v6_rules = matcher.v6_rule_count();
        let elapsed_ms = AppClock::elapsed_millis().saturating_sub(start_ms);
        info!(
            tag = %tag,
            local_rules,
            referenced_sets = args.sets.len(),
            v4_rules = total_v4_rules,
            v6_rules = total_v6_rules,
            has_v4_rules,
            has_v6_rules,
            elapsed_ms,
            "ip_set snapshot built"
        );

        Ok(IpSetSnapshot {
            matcher,
            has_v4_rules,
            has_v6_rules,
            stats: IpSetRuleStats {
                v4_rules: total_v4_rules,
                v6_rules: total_v6_rules,
            },
        })
    }

    fn update_reload_state(&self, result: &DnsResult<()>) {
        let mut state = self
            .reload_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.last_reload_ms = Some(AppClock::now_timestamp());
        state.last_error = result.as_ref().err().map(ToString::to_string);
    }
}

#[async_trait]
impl Plugin for IpSet {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, context: &crate::plugin::PluginInitContext<'_>) -> DnsResult<()> {
        let mut providers = Vec::with_capacity(self.args.sets.len());
        for (set_idx, set_tag) in self.args.sets.iter().enumerate() {
            let field = format!("args.sets[{}]", set_idx);
            debug!(
                tag = %self.tag,
                referenced_set = %set_tag,
                "resolving referenced ip provider"
            );
            let provider = context.provider(&field, set_tag.as_str())?;
            if !provider.supports_ip_matching() {
                return Err(DnsError::plugin(format!(
                    "plugin '{}' field '{}' expects provider '{}' to support IP matching",
                    self.tag, field, set_tag
                )));
            }
            providers.push(provider);
        }
        self.referenced_sets = providers;
        self.reload().await
    }

    async fn destroy(&self) -> DnsResult<()> {
        Ok(())
    }
}

#[async_trait]
impl Provider for IpSet {
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline]
    #[hotpath::measure]
    fn contains_ip(&self, ip: IpAddr) -> bool {
        let snapshot = self.snapshot.load();
        let has_family_rules = match ip {
            IpAddr::V4(_) => snapshot.has_v4_rules,
            IpAddr::V6(_) => snapshot.has_v6_rules,
        };
        if has_family_rules && snapshot.matcher.contains_ip(ip) {
            return true;
        }

        self.referenced_sets.iter().any(|set| set.contains_ip(ip))
    }

    #[hotpath::measure]
    async fn reload(&self) -> DnsResult<()> {
        let tag = self.tag.clone();
        let args = self.args.clone();
        let result = spawn_isolated_build("ip_set snapshot build", move || {
            Self::build_local_snapshot(&tag, &args)
        })
        .await
        .map(|snapshot| {
            self.snapshot.store(Arc::new(snapshot));
        });
        self.update_reload_state(&result);
        result
    }

    fn supports_reload(&self) -> bool {
        true
    }

    fn runtime_status(&self) -> ProviderRuntimeStatus {
        let reload_state = self
            .reload_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let stats = self.snapshot.load().stats;
        ProviderRuntimeStatus {
            ok: true,
            plugin: self.tag.clone(),
            supports_reload: true,
            supports_domain_matching: false,
            supports_ip_matching: true,
            last_reload_ms: reload_state.last_reload_ms,
            last_error: reload_state.last_error,
            rule_stats: Some(ProviderRuleStats {
                total_rules: Some(stats.total_rules()),
                v4_rules: Some(stats.v4_rules),
                v6_rules: Some(stats.v6_rules),
                ..ProviderRuleStats::default()
            }),
        }
    }

    fn supports_ip_matching(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("ip_set")]
pub struct IpSetFactory {}

impl PluginFactory for IpSetFactory {
    fn get_dependency_specs(&self, plugin_config: &PluginConfig) -> Vec<DependencySpec> {
        plugin_config
            .args
            .clone()
            .and_then(|args| serde_yaml_ng::from_value::<IpSetArgs>(args).ok())
            .map(|args| {
                args.sets
                    .into_iter()
                    .enumerate()
                    .map(|(idx, tag)| DependencySpec::provider(format!("args.sets[{}]", idx), tag))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> DnsResult<UninitializedPlugin> {
        let args = plugin_config
            .args
            .clone()
            .map(serde_yaml_ng::from_value::<IpSetArgs>)
            .transpose()
            .map_err(|e| DnsError::plugin(format!("failed to parse ip_set config: {}", e)))?
            .unwrap_or_default();
        info!(
            tag = %plugin_config.tag,
            ips = args.ips.len(),
            files = args.files.len(),
            sets = args.sets.len(),
            "ip_set configured"
        );

        Ok(UninitializedPlugin::Provider(Box::new(IpSet {
            tag: plugin_config.tag.clone(),
            args: Arc::new(args),
            referenced_sets: Vec::new(),
            snapshot: ArcSwap::from_pointee(IpSetSnapshot::default()),
            reload_state: Mutex::new(ProviderReloadState::default()),
        })))
    }
}

fn scan_ip_rules<F>(args: &IpSetArgs, mut on_rule: F) -> DnsResult<()>
where
    F: FnMut(&str, TextLocation<'_>) -> DnsResult<()>,
{
    TextSource::new("args.ips", &args.ips, &args.files)
        .scan(&LineClassifier::new(&["#"]), |line: TextLine<'_>| {
            if line.annotations().blank || line.annotations().leading_comment.is_some() {
                return Ok(());
            }
            let rule = normalize_ip_rule_line(line.raw());
            if rule.is_empty() {
                return Ok(());
            }
            on_rule(rule, line.location())
        })
        .map_err(|error| DnsError::plugin(format!("failed to load IP rules: {error}")))
}

fn count_ip_rules(args: &IpSetArgs) -> DnsResult<(usize, usize, usize)> {
    let mut total = 0usize;
    let mut v4 = 0usize;
    let mut v6 = 0usize;
    scan_ip_rules(args, |raw, source| {
        total += 1;
        match IpPrefixMatcher::classify_rule(raw).map_err(|error| {
            DnsError::plugin(format!("invalid ip/cidr '{raw}' in {source}: {error}"))
        })? {
            IpRuleFamily::V4 => v4 += 1,
            IpRuleFamily::V6 => v6 += 1,
        }
        Ok(())
    })?;
    Ok((total, v4, v6))
}

fn add_ip_rule_from_source(
    matcher: &mut IpPrefixMatcher,
    rule: &str,
    source: TextLocation<'_>,
) -> DnsResult<()> {
    matcher
        .add_rule(rule)
        .map_err(|error| DnsError::plugin(format!("invalid ip/cidr '{rule}' in {source}: {error}")))
}

#[cfg(test)]
fn add_ip_rule(matcher: &mut IpPrefixMatcher, rule: &str, source: &str) -> DnsResult<()> {
    let rule = rule.trim();
    if rule.is_empty() {
        return Ok(());
    }
    matcher.add_rule(rule).map_err(|e| {
        DnsError::plugin(format!("invalid ip/cidr '{}' in {}: {}", rule, source, e))
    })?;
    Ok(())
}

fn normalize_ip_rule_line(line: &str) -> &str {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return "";
    }
    line.split_once('#')
        .map(|(rule, _)| rule)
        .unwrap_or(line)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_rules_text(
        matcher: &mut IpPrefixMatcher,
        source_name: &str,
        content: &str,
    ) -> DnsResult<()> {
        for (line_no, raw) in content.lines().enumerate() {
            let rule = normalize_ip_rule_line(raw);
            if rule.is_empty() {
                continue;
            }
            let source = format!("file '{}', line {}", source_name, line_no + 1);
            add_ip_rule(matcher, rule, &source)?;
        }
        Ok(())
    }

    #[test]
    fn test_ipv4_and_ipv6_match() {
        let mut m = IpPrefixMatcher::default();
        add_ip_rule(&mut m, "192.168.1.0/24", "test").unwrap();
        add_ip_rule(&mut m, "2001:db8::/32", "test").unwrap();
        m.finalize();

        assert!(m.contains_ip("192.168.1.7".parse().unwrap()));
        assert!(!m.contains_ip("192.168.2.1".parse().unwrap()));
        assert!(m.contains_ip("2001:db8:1::1".parse().unwrap()));
        assert!(!m.contains_ip("2001:db9::1".parse().unwrap()));
    }

    #[test]
    fn test_single_ip_default_prefix() {
        let mut m = IpPrefixMatcher::default();
        add_ip_rule(&mut m, "1.1.1.1", "test").unwrap();
        add_ip_rule(&mut m, "2001:db8::1", "test").unwrap();
        m.finalize();

        assert!(m.contains_ip("1.1.1.1".parse().unwrap()));
        assert!(!m.contains_ip("1.1.1.2".parse().unwrap()));
        assert!(m.contains_ip("2001:db8::1".parse().unwrap()));
        assert!(!m.contains_ip("2001:db8::2".parse().unwrap()));
    }

    #[test]
    fn test_cidr_host_bits_are_masked() {
        let mut m = IpPrefixMatcher::default();
        add_ip_rule(&mut m, "10.10.10.7/24", "test").unwrap();
        m.finalize();
        assert!(m.contains_ip("10.10.10.200".parse().unwrap()));
        assert!(!m.contains_ip("10.10.11.1".parse().unwrap()));
    }

    #[test]
    fn test_file_line_error_has_line_number() {
        let mut m = IpPrefixMatcher::default();
        let err = load_rules_text(&mut m, "inline-ip-test", "1.1.1.1\n2001::1/200\n").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("line 2"),
            "error should include line number: {msg}"
        );
    }

    #[test]
    fn test_parse_error_includes_input() {
        let mut m = IpPrefixMatcher::default();
        let err = add_ip_rule(&mut m, "1.1.1.1/abc", "test").unwrap_err();
        assert!(err.to_string().contains("1.1.1.1/abc"));
    }

    #[test]
    fn test_inline_comment_in_file() {
        let mut m = IpPrefixMatcher::default();
        load_rules_text(
            &mut m,
            "inline-ip-comment-test",
            "1.1.1.1 # test\n# ignore\n\n",
        )
        .unwrap();
        m.finalize();
        assert!(m.contains_ip("1.1.1.1".parse().unwrap()));
    }
}
