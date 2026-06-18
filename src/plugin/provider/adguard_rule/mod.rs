// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `adguard_rule` provider plugin.
//!
//! This provider evaluates the request-side subset of AdGuard Home DNS rules.
//!
//! Scope of this implementation:
//! - supported: basic domain masks, exception rules, `important`, `badfilter`,
//!   `denyallow`, and request-side `dnstype`
//! - intentionally unsupported: `/etc/hosts` style rules, `dnsrewrite`,
//!   `$client`, `$ctag`, and unknown modifiers
//!
//! Unsupported rules are skipped with warnings so mixed upstream rule files can
//! still load, while invalid syntax inside the supported subset remains a hard
//! error.

use std::any::Any;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use async_trait::async_trait;
use tracing::info;

use self::compiler::build_rule_buckets;
use self::config::{AdGuardRuleConfig, parse_config};
use self::model::{BuildStats, CompiledRuleSet};
use crate::config::types::PluginConfig;
use crate::infra::clock::AppClock;
use crate::infra::error::Result as DnsResult;
use crate::infra::task::spawn_isolated_build;
use crate::plugin::provider::{Provider, ProviderRuleStats, ProviderRuntimeStatus};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;
use crate::proto::{Name, Question};

mod compiler;
mod config;
mod model;
mod syntax;

#[derive(Debug)]
struct AdGuardRuleSnapshot {
    important_exceptions: CompiledRuleSet,
    important_blocks: CompiledRuleSet,
    exceptions: CompiledRuleSet,
    blocks: CompiledRuleSet,
    stats: BuildStats,
}

#[derive(Debug, Default, Clone)]
struct ProviderReloadState {
    last_reload_ms: Option<u64>,
    last_error: Option<String>,
}

#[derive(Debug)]
pub struct AdGuardRule {
    tag: String,
    cfg: Arc<AdGuardRuleConfig>,
    snapshot: ArcSwap<AdGuardRuleSnapshot>,
    reload_state: Mutex<ProviderReloadState>,
}

impl AdGuardRule {
    fn contains_name_only(&self, qname: &Name) -> bool {
        let snapshot = self.snapshot.load();
        if snapshot.important_exceptions.is_match_name_only(qname) {
            return false;
        }
        if snapshot.important_blocks.is_match_name_only(qname) {
            return true;
        }
        if snapshot.exceptions.is_match_name_only(qname) {
            return false;
        }
        snapshot.blocks.is_match_name_only(qname)
    }

    fn contains_question_rule(&self, question: &Question) -> bool {
        let snapshot = self.snapshot.load();
        let qname = question.name();
        let qtype = question.qtype();

        if snapshot.important_exceptions.is_match(qname, qtype) {
            return false;
        }
        if snapshot.important_blocks.is_match(qname, qtype) {
            return true;
        }
        if snapshot.exceptions.is_match(qname, qtype) {
            return false;
        }
        snapshot.blocks.is_match(qname, qtype)
    }

    #[hotpath::measure]
    fn build_snapshot(tag: &str, cfg: &AdGuardRuleConfig) -> DnsResult<AdGuardRuleSnapshot> {
        let (important_exceptions, important_blocks, exceptions, blocks, stats) =
            build_rule_buckets(tag, cfg)?;

        info!(
            tag,
            total_rules = stats.total_rules,
            supported_rules = stats.supported_rules,
            skipped_rules = stats.skipped_rules,
            exception_rules = stats.exception_rules,
            important_rules = stats.important_rules,
            disabled_rules = stats.disabled_rules,
            "adguard_rule snapshot built"
        );

        Ok(AdGuardRuleSnapshot {
            important_exceptions,
            important_blocks,
            exceptions,
            blocks,
            stats,
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
impl Plugin for AdGuardRule {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> DnsResult<()> {
        self.reload().await
    }

    async fn destroy(&self) -> DnsResult<()> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("adguard_rule")]
pub struct AdGuardRuleFactory;

#[async_trait]
impl Provider for AdGuardRule {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn supports_domain_matching(&self) -> bool {
        // This provider participates through runtime `contains_name` and
        // `contains_question` evaluation. That keeps exception precedence and
        // request-scoped modifiers intact when another provider composes it.
        true
    }

    #[hotpath::measure]
    fn contains_name(&self, name: &Name) -> bool {
        self.contains_name_only(name)
    }

    #[hotpath::measure]
    fn contains_question(&self, question: &Question) -> bool {
        self.contains_question_rule(question)
    }

    #[hotpath::measure]
    async fn reload(&self) -> DnsResult<()> {
        let tag = self.tag.clone();
        let cfg = self.cfg.clone();
        let result = spawn_isolated_build("adguard_rule snapshot build", move || {
            Self::build_snapshot(&tag, &cfg)
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
            supports_domain_matching: true,
            supports_ip_matching: false,
            last_reload_ms: reload_state.last_reload_ms,
            last_error: reload_state.last_error,
            rule_stats: Some(ProviderRuleStats {
                total_rules: Some(stats.total_rules),
                supported_rules: Some(stats.supported_rules),
                skipped_rules: Some(stats.skipped_rules),
                exception_rules: Some(stats.exception_rules),
                important_rules: Some(stats.important_rules),
                ..ProviderRuleStats::default()
            }),
        }
    }
}

impl PluginFactory for AdGuardRuleFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> DnsResult<UninitializedPlugin> {
        let cfg = parse_config(plugin_config.args.clone())?;

        Ok(UninitializedPlugin::Provider(Box::new(AdGuardRule {
            tag: plugin_config.tag.clone(),
            cfg: Arc::new(cfg),
            snapshot: ArcSwap::from_pointee(AdGuardRuleSnapshot {
                important_exceptions: CompiledRuleSet::default(),
                important_blocks: CompiledRuleSet::default(),
                exceptions: CompiledRuleSet::default(),
                blocks: CompiledRuleSet::default(),
                stats: BuildStats::default(),
            }),
            reload_state: Mutex::new(ProviderReloadState::default()),
        })))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufWriter, Write};
    use std::net::{Ipv4Addr, SocketAddr};

    use tempfile::tempdir;

    use super::*;
    use crate::core::context::DnsContext;
    use crate::plugin::provider::adguard_rule::syntax::{
        ParsedLine, SkipReason, compile_pattern, parse_line, parse_rule_details,
    };
    use crate::proto::{DNSClass, Message, Name, Question, RecordType};

    fn make_context(name: &str, qtype: RecordType) -> DnsContext {
        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii(name).unwrap(),
            qtype,
            DNSClass::IN,
        ));
        DnsContext::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)), request)
    }

    fn compile_test_rule(raw: &str) -> model::CompiledRule {
        let ParsedLine::Rule(meta) = parse_line(raw, None).expect("rule should parse") else {
            panic!("rule should be supported");
        };
        let details = parse_rule_details(&meta).expect("modifiers should parse");
        model::CompiledRule {
            matcher: compile_pattern(meta.pattern).expect("pattern should compile"),
            dnstype: details.dnstype,
            denyallow: details.denyallow,
        }
    }

    fn make_provider(cfg: config::AdGuardRuleConfig) -> AdGuardRule {
        let (important_exceptions, important_blocks, exceptions, blocks, stats) =
            build_rule_buckets("agh", &cfg).expect("rules should build");
        AdGuardRule {
            tag: "agh".to_string(),
            cfg: Arc::new(cfg),
            snapshot: ArcSwap::from_pointee(AdGuardRuleSnapshot {
                important_exceptions,
                important_blocks,
                exceptions,
                blocks,
                stats,
            }),
            reload_state: Mutex::new(ProviderReloadState::default()),
        }
    }

    #[test]
    fn plain_domain_rule_matches_exact_only() {
        let compiled = compile_test_rule("example.org");

        assert!(compiled.is_match("example.org", RecordType::A));
        assert!(!compiled.is_match("www.example.org", RecordType::A));
    }

    #[test]
    fn domain_anchor_rule_matches_subdomains() {
        let compiled = compile_test_rule("||example.org^");

        assert!(compiled.is_match("example.org", RecordType::A));
        assert!(compiled.is_match("www.example.org", RecordType::A));
        assert!(!compiled.is_match("testexample.org", RecordType::A));
    }

    #[test]
    fn regex_rule_is_case_insensitive() {
        let compiled = compile_test_rule("/EXAMPLE\\.(org|net)/");

        assert!(compiled.is_match("example.org", RecordType::A));
        assert!(compiled.is_match("example.net", RecordType::A));
    }

    #[test]
    fn unsupported_modifier_skips_rule() {
        let parsed = parse_line("||example.org^$dnsrewrite=1.2.3.4", None).unwrap();
        assert!(matches!(
            parsed,
            ParsedLine::Skipped(SkipReason::UnsupportedModifier)
        ));
    }

    #[test]
    fn invalid_supported_regex_is_error() {
        let err = compile_pattern("/(/").expect_err("invalid regex should fail");
        assert!(err.contains("invalid regex"));
    }

    #[test]
    fn empty_badfilter_regex_is_rejected_during_planning() {
        for rule in ["//$badfilter", "/   /$badfilter"] {
            let cfg = config::AdGuardRuleConfig {
                rules: vec![rule.to_string()],
                files: Vec::new(),
            };

            let error = build_rule_buckets("agh", &cfg).unwrap_err().to_string();

            assert!(error.contains("args.rules[0]"), "{error}");
            assert!(error.contains("empty regex rule"), "{error}");
        }
    }

    #[test]
    fn denyallow_excludes_domains() {
        let compiled = compile_test_rule("||example.org^$denyallow=sub.example.org");

        assert!(compiled.is_match("example.org", RecordType::A));
        assert!(!compiled.is_match("sub.example.org", RecordType::A));
    }

    #[test]
    fn dnstype_uses_request_type() {
        let compiled = compile_test_rule("||example.org^$dnstype=AAAA");

        assert!(compiled.is_match("example.org", RecordType::AAAA));
        assert!(!compiled.is_match("example.org", RecordType::A));
    }

    #[test]
    fn badfilter_disables_matching_rule() {
        let cfg = config::AdGuardRuleConfig {
            rules: vec![
                "||example.org^$important".to_string(),
                "||example.org^$important,badfilter".to_string(),
            ],
            files: Vec::new(),
        };

        let (_, important_blocks, _, blocks, _) =
            build_rule_buckets("agh", &cfg).expect("rules should build");
        assert!(important_blocks.is_empty());
        assert!(blocks.is_empty());
    }

    #[test]
    fn badfilter_order_does_not_change_result() {
        for rules in [
            ["||example.org^", "||example.org^$badfilter"],
            ["||example.org^$badfilter", "||example.org^"],
        ] {
            let cfg = config::AdGuardRuleConfig {
                rules: rules.into_iter().map(str::to_string).collect(),
                files: Vec::new(),
            };
            let (_, _, _, blocks, stats) =
                build_rule_buckets("agh", &cfg).expect("rules should build");
            assert!(blocks.is_empty());
            assert_eq!(stats.disabled_rules, 1);
        }
    }

    #[test]
    fn badfilter_applies_across_files() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("target.txt");
        let badfilter = dir.path().join("badfilter.txt");
        std::fs::write(&target, "||example.org^\n").unwrap();
        std::fs::write(&badfilter, "||example.org^$badfilter\n").unwrap();
        let cfg = config::AdGuardRuleConfig {
            rules: Vec::new(),
            files: vec![
                target.to_string_lossy().into_owned(),
                badfilter.to_string_lossy().into_owned(),
            ],
        };

        let (_, _, _, blocks, stats) = build_rule_buckets("agh", &cfg).unwrap();

        assert!(blocks.is_empty());
        assert_eq!(stats.disabled_rules, 1);
    }

    #[test]
    fn url_path_rule_is_skipped_but_delimited_regex_is_supported() {
        assert!(matches!(
            parse_line(
                "@@/js/adview_*.adsbygoogle.js$~third-party,domain=example.org",
                None,
            )
            .unwrap(),
            ParsedLine::Skipped(SkipReason::Path)
        ));

        let ParsedLine::Rule(meta) = parse_line("/EXAMPLE\\.ORG/$dnstype=A", None).unwrap() else {
            panic!("delimited regex should be supported");
        };
        assert_eq!(meta.pattern, "/EXAMPLE\\.ORG/");
        assert_eq!(meta.dnstype, Some("A"));
    }

    #[test]
    fn cosmetic_and_global_network_rules_are_skipped() {
        assert!(matches!(
            parse_line("##global-cosmetic", Some("#")).unwrap(),
            ParsedLine::Skipped(SkipReason::Cosmetic)
        ));
        assert!(matches!(
            parse_line("# ordinary comment", Some("#")).unwrap(),
            ParsedLine::Ignored
        ));
        assert!(matches!(
            parse_line("example.org#@?#banner:contains(/ad|sponsor/)", None).unwrap(),
            ParsedLine::Skipped(SkipReason::Cosmetic)
        ));
        assert!(matches!(
            parse_line("$script,third-party,domain=example.org", None).unwrap(),
            ParsedLine::Skipped(SkipReason::UnsupportedModifier)
        ));
        assert!(matches!(
            parse_line("||example.org/assets/ad.js", None).unwrap(),
            ParsedLine::Skipped(SkipReason::Path)
        ));
    }

    #[test]
    fn rejects_supported_modifiers_without_a_rule_pattern() {
        for rule in [
            "$important",
            "$badfilter",
            "$dnstype=A",
            "$denyallow=example.org",
            "$script,important",
        ] {
            let error = parse_line(rule, None).unwrap_err();
            assert_eq!(error, "empty rule pattern", "rule: {rule}");
        }

        assert!(matches!(
            parse_line("$script,third-party", None).unwrap(),
            ParsedLine::Skipped(SkipReason::UnsupportedModifier)
        ));
    }

    #[test]
    fn large_file_builds_with_streaming_visitor() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("large.txt");
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = BufWriter::new(file);
        for index in 0..20_000usize {
            writeln!(writer, "||host-{index}.example.org^").unwrap();
        }
        writer.flush().unwrap();
        let cfg = config::AdGuardRuleConfig {
            rules: Vec::new(),
            files: vec![path.to_string_lossy().into_owned()],
        };

        let (_, _, _, blocks, stats) = build_rule_buckets("agh", &cfg).unwrap();

        assert_eq!(stats.total_rules, 20_000);
        assert_eq!(blocks.fast_matcher.trie_rule_count(), 20_000);
    }

    #[test]
    fn supported_syntax_error_reports_file_line() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("invalid.txt");
        std::fs::write(&path, "! comment\n/(/\n").unwrap();
        let cfg = config::AdGuardRuleConfig {
            rules: Vec::new(),
            files: vec![path.to_string_lossy().into_owned()],
        };

        let error = build_rule_buckets("agh", &cfg).unwrap_err().to_string();

        assert!(error.contains("line 2"), "{error}");
        assert!(error.contains("invalid regex"), "{error}");
    }

    #[test]
    fn modifier_error_reports_file_line_during_planning_scan() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("invalid-modifier.txt");
        std::fs::write(&path, "! comment\n||example.org^$dnstype=NOT_A_TYPE\n").unwrap();
        let cfg = config::AdGuardRuleConfig {
            rules: Vec::new(),
            files: vec![path.to_string_lossy().into_owned()],
        };

        let error = build_rule_buckets("agh", &cfg).unwrap_err().to_string();

        assert!(error.contains("line 2"), "{error}");
        assert!(error.contains("invalid dnstype"), "{error}");
    }

    #[test]
    fn four_priority_buckets_remain_distinct() {
        let cfg = config::AdGuardRuleConfig {
            rules: vec![
                "@@||ie.example^$important".to_string(),
                "||ib.example^$important".to_string(),
                "@@||e.example^".to_string(),
                "||b.example^".to_string(),
            ],
            files: Vec::new(),
        };
        let (important_exceptions, important_blocks, exceptions, blocks, _) =
            build_rule_buckets("agh", &cfg).unwrap();

        assert!(important_exceptions.is_match_name_only(&Name::from_ascii("ie.example").unwrap()));
        assert!(important_blocks.is_match_name_only(&Name::from_ascii("ib.example").unwrap()));
        assert!(exceptions.is_match_name_only(&Name::from_ascii("e.example").unwrap()));
        assert!(blocks.is_match_name_only(&Name::from_ascii("b.example").unwrap()));
    }

    #[tokio::test]
    async fn provider_returns_true_only_for_effective_block() {
        let cfg = config::AdGuardRuleConfig {
            rules: vec![
                "||example.org^".to_string(),
                "@@||safe.example.org^".to_string(),
                "||ads.example.org^$important".to_string(),
            ],
            files: Vec::new(),
        };
        let provider = make_provider(cfg);

        let ads = make_context("ads.example.org.", RecordType::A);
        assert!(
            provider.contains_question(
                ads.request()
                    .first_question()
                    .expect("question should exist")
            )
        );

        let safe = make_context("safe.example.org.", RecordType::A);
        assert!(
            !provider.contains_question(
                safe.request()
                    .first_question()
                    .expect("question should exist")
            )
        );
    }

    #[tokio::test]
    async fn contains_name_ignores_dnstype_rules() {
        let cfg = config::AdGuardRuleConfig {
            rules: vec![
                "||always.example.org^".to_string(),
                "||type-only.example.org^$dnstype=AAAA".to_string(),
                "@@||safe.example.org^".to_string(),
            ],
            files: Vec::new(),
        };
        let provider = make_provider(cfg);

        assert!(provider.contains_name(&Name::from_ascii("always.example.org.").unwrap()));
        assert!(!provider.contains_name(&Name::from_ascii("type-only.example.org.").unwrap()));
        assert!(!provider.contains_name(&Name::from_ascii("safe.example.org.").unwrap()));
    }

    #[tokio::test]
    async fn failed_reload_keeps_current_snapshot() {
        let mut provider = make_provider(config::AdGuardRuleConfig {
            rules: vec!["||existing.example.org^".to_string()],
            files: Vec::new(),
        });
        let existing = Name::from_ascii("existing.example.org.").unwrap();
        assert!(provider.contains_name(&existing));

        let dir = tempdir().unwrap();
        provider.cfg = Arc::new(config::AdGuardRuleConfig {
            rules: Vec::new(),
            files: vec![
                dir.path()
                    .join("missing-rules.txt")
                    .to_string_lossy()
                    .into_owned(),
            ],
        });

        assert!(provider.reload().await.is_err());
        assert!(provider.contains_name(&existing));
    }

    #[test]
    fn provider_status_reports_adguard_stats() {
        let cfg = config::AdGuardRuleConfig {
            rules: vec![
                "||example.org^".to_string(),
                "@@||safe.example.org^".to_string(),
                "||ads.example.org^$important".to_string(),
            ],
            files: Vec::new(),
        };
        let provider = make_provider(cfg);

        let status = provider.runtime_status();

        assert!(status.ok);
        assert!(status.supports_reload);
        let stats = status.rule_stats.expect("adguard stats should exist");
        assert_eq!(stats.total_rules, Some(3));
        assert_eq!(stats.supported_rules, Some(3));
        assert_eq!(stats.exception_rules, Some(1));
        assert_eq!(stats.important_rules, Some(1));
    }
}
