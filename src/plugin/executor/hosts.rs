// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `hosts` executor plugin.
//!
//! Maps host-style domain rules to static IP responses.
//!
//! Rule sources:
//! - inline `entries`
//! - external files (`files`)
//!
//! `format` defaults to `oxidns` (`domain_rule IP [IP ...]`). The optional
//! `standard` format reads `IP hostname [alias ...]`, merging unique addresses
//! for each exact hostname. Parsing and merging happen only during creation;
//! the request path uses the same immutable index for both formats.
//! Invalid rules that match the other format report a configuration hint
//! without switching formats automatically.
//!
//! Supported matchers in `oxidns` format:
//! - exact name (`full:example.com`)
//! - suffix domain (`domain:example.com`)
//! - keyword (`keyword:cdn`)
//! - regex (`regexp:^api\\.`)
//!
//! Unprefixed rules default to `full:`.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ahash::AHashMap;
use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use async_trait::async_trait;
use regex::{Regex, RegexSet, RegexSetBuilder};
use serde::Deserialize;
use serde_yaml_ng::Value;

use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result};
use crate::infra::io::{LineClassifier, TextLocation, TextSource};
use crate::infra::observability::metrics::{
    MetricLabel, MetricSample, MetricSink, MetricSource, register_metric_source,
    unregister_metric_source,
};
use crate::plugin::executor::{ExecStep, Executor, synthetic_response};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;
use crate::proto::{A, AAAA, DNSClass, Message, Question, RData, RecordType};

const HOSTS_ANSWER_TTL: u32 = 10;

#[derive(Debug, Clone, Deserialize, Default)]
struct HostsConfig {
    /// Rule syntax shared by inline entries and external files.
    #[serde(default)]
    format: HostsFormat,
    /// Inline hosts rules.
    #[serde(default)]
    entries: Vec<String>,
    /// Paths to hosts rule files.
    #[serde(default)]
    files: Vec<String>,
    /// Whether to stop the executor chain after producing a local answer.
    #[serde(default)]
    short_circuit: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum HostsFormat {
    #[default]
    Oxidns,
    Standard,
}

#[derive(Debug, Clone)]
enum RuleMatcher {
    Full(String),
    Domain(String),
    Keyword(String),
    Regexp(String),
}

#[derive(Debug, Clone)]
struct HostsRule {
    matcher: RuleMatcher,
    answers: HostsAnswers,
}

#[derive(Debug, Clone, Default)]
struct HostsAnswers {
    ipv4: Vec<Arc<RData>>,
    ipv6: Vec<Arc<RData>>,
}

#[derive(Debug)]
struct HostsExecutor {
    tag: String,
    index: RuleIndex,
    short_circuit: bool,
    metrics: Arc<HostsMetrics>,
}

#[derive(Debug)]
struct HostsMetrics {
    tag: String,
    hit_total: AtomicU64,
    miss_total: AtomicU64,
}

impl HostsMetrics {
    fn new(tag: String) -> Self {
        Self {
            tag,
            hit_total: AtomicU64::new(0),
            miss_total: AtomicU64::new(0),
        }
    }
}

impl MetricSource for HostsMetrics {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn plugin_type(&self) -> &'static str {
        "hosts"
    }

    fn collect(&self, sink: &mut dyn MetricSink) {
        let labels = [MetricLabel::new("plugin_tag", self.tag.as_str())];
        sink.emit(MetricSample::counter(
            "hosts_hit_total",
            "Total hosts local response hits.",
            &labels,
            self.hit_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "hosts_miss_total",
            "Total hosts pass-through misses.",
            &labels,
            self.miss_total.load(Ordering::Relaxed),
        ));
    }
}

#[derive(Debug)]
struct RuleIndex {
    payloads: Vec<Arc<HostsAnswers>>,
    full_rules: AHashMap<Box<str>, usize>,
    domain_rules: DomainTrie,
    keyword_rules: Option<KeywordIndex>,
    regex_rules: Option<RegexIndex>,
}

#[derive(Debug, Default)]
struct RuleIndexBuilder {
    payloads: Vec<Arc<HostsAnswers>>,
    full_rules: AHashMap<Box<str>, usize>,
    domain_rules: DomainTrie,
    keyword_rules: KeywordIndexBuilder,
    regex_rules: RegexIndexBuilder,
}

#[derive(Debug, Default)]
struct KeywordIndexBuilder {
    patterns: Vec<String>,
    payload_slots: Vec<usize>,
    pattern_ids: AHashMap<Box<str>, usize>,
}

#[derive(Debug, Default)]
struct RegexIndexBuilder {
    patterns: Vec<String>,
    payload_slots: Vec<usize>,
    pattern_ids: AHashMap<Box<str>, usize>,
}

#[derive(Debug)]
struct KeywordIndex {
    matcher: AhoCorasick,
    payload_slots: Vec<usize>,
}

#[derive(Debug)]
struct RegexIndex {
    matcher: RegexSet,
    payload_slots: Vec<usize>,
}

#[derive(Debug, Default)]
struct DomainTrieNode {
    payload_slot: Option<usize>,
    children: AHashMap<Box<str>, u32>,
}

#[derive(Debug)]
struct DomainTrie {
    nodes: Vec<DomainTrieNode>,
}

impl Default for DomainTrie {
    fn default() -> Self {
        Self {
            nodes: vec![DomainTrieNode::default()],
        }
    }
}

#[async_trait]
impl Plugin for HostsExecutor {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        register_metric_source(self.metrics.clone())
    }

    async fn destroy(&self) -> Result<()> {
        unregister_metric_source(&self.tag);
        Ok(())
    }
}

#[async_trait]
impl Executor for HostsExecutor {
    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        if context.request.questions().len() != 1 {
            self.metrics.miss_total.fetch_add(1, Ordering::Relaxed);
            return Ok(ExecStep::Next);
        }

        let Some(question) = context.request.first_question() else {
            self.metrics.miss_total.fetch_add(1, Ordering::Relaxed);
            return Ok(ExecStep::Next);
        };
        if question.qclass() != DNSClass::IN {
            self.metrics.miss_total.fetch_add(1, Ordering::Relaxed);
            return Ok(ExecStep::Next);
        }
        if question.qtype() != RecordType::A && question.qtype() != RecordType::AAAA {
            self.metrics.miss_total.fetch_add(1, Ordering::Relaxed);
            return Ok(ExecStep::Next);
        }

        let Some(answers) = self.index.match_answers(question.name().normalized()) else {
            self.metrics.miss_total.fetch_add(1, Ordering::Relaxed);
            return Ok(ExecStep::Next);
        };

        let response = build_hosts_response(&context.request, question, answers)?;
        context.set_response(response);
        self.metrics.hit_total.fetch_add(1, Ordering::Relaxed);

        if self.short_circuit {
            Ok(ExecStep::Stop)
        } else {
            Ok(ExecStep::Next)
        }
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("hosts")]
pub struct HostsFactory;

impl PluginFactory for HostsFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let cfg = parse_config(plugin_config.args.clone())?;
        let index = build_rule_index(&cfg)?;

        Ok(UninitializedPlugin::Executor(Box::new(HostsExecutor {
            tag: plugin_config.tag.clone(),
            index,
            short_circuit: cfg.short_circuit,
            metrics: Arc::new(HostsMetrics::new(plugin_config.tag.clone())),
        })))
    }
}

fn parse_config(args: Option<Value>) -> Result<HostsConfig> {
    let Some(args) = args else {
        return Ok(HostsConfig::default());
    };

    serde_yaml_ng::from_value(args)
        .map_err(|e| DnsError::plugin(format!("failed to parse hosts config: {}", e)))
}

fn build_rule_index(cfg: &HostsConfig) -> Result<RuleIndex> {
    let mut builder = RuleIndexBuilder::default();
    TextSource::new("args.entries", &cfg.entries, &cfg.files)
        .scan(
            &LineClassifier::new(&["#"]),
            |line| -> std::result::Result<(), String> {
                if cfg.format == HostsFormat::Standard {
                    let raw = line.raw();
                    let rule_text = raw.split_once('#').map_or(raw, |(left, _)| left).trim();
                    if rule_text.is_empty() {
                        return Ok(());
                    }
                    return parse_standard_hosts_line(rule_text, &mut builder).map_err(|error| {
                        hosts_rule_error(cfg.format, line.raw(), line.location(), error)
                    });
                }

                let rule_text = match line.location() {
                    TextLocation::Inline { .. } => line.raw().trim(),
                    TextLocation::File { .. } => {
                        if line.annotations().blank || line.annotations().leading_comment.is_some()
                        {
                            return Ok(());
                        }
                        let raw = line.raw();
                        raw.split_once('#').map_or(raw, |(left, _)| left).trim()
                    }
                };
                let rule = parse_hosts_line(rule_text).map_err(|error| {
                    hosts_rule_error(cfg.format, line.raw(), line.location(), error)
                })?;
                builder.add_rule(rule);
                Ok(())
            },
        )
        .map_err(|error| DnsError::plugin(error.to_string()))?;

    builder.build()
}

fn hosts_rule_error(
    format: HostsFormat,
    raw: &str,
    location: TextLocation<'_>,
    error: String,
) -> String {
    let standard_text = raw.split_once('#').map_or(raw, |(left, _)| left).trim();
    let oxidns_text = match location {
        TextLocation::Inline { .. } => raw.trim(),
        TextLocation::File { .. } => standard_text,
    };
    let (rule_text, configured, suggested) = match format {
        HostsFormat::Oxidns => (
            oxidns_text,
            "oxidns",
            parse_standard_hosts_fields(standard_text)
                .is_ok()
                .then_some("standard"),
        ),
        HostsFormat::Standard => (
            standard_text,
            "standard",
            parse_hosts_line(oxidns_text).is_ok().then_some("oxidns"),
        ),
    };
    if let Some(suggested) = suggested {
        format!(
            "invalid hosts rule '{rule_text}': format mismatch: configured format: {configured}, \
             but this rule matches {suggested}; set format: {suggested} or convert this rule to \
             {configured} format; {error}"
        )
    } else {
        format!("invalid hosts rule '{rule_text}': {error}")
    }
}

fn parse_hosts_line(raw: &str) -> std::result::Result<HostsRule, String> {
    let mut fields = raw.split_whitespace();
    let Some(matcher_raw) = fields.next() else {
        return Err("hosts rule must include domain rule and at least one IP".to_string());
    };
    let matcher = parse_rule_matcher(matcher_raw)?;

    let mut ipv4 = Vec::new();
    let mut ipv6 = Vec::new();
    for token in fields {
        match token.parse::<IpAddr>() {
            Ok(IpAddr::V4(v4)) => ipv4.push(Arc::new(RData::A(A(v4)))),
            Ok(IpAddr::V6(v6)) => ipv6.push(Arc::new(RData::AAAA(AAAA(v6)))),
            Err(e) => return Err(format!("invalid hosts IP '{}': {}", token, e)),
        }
    }

    if ipv4.is_empty() && ipv6.is_empty() {
        return Err("hosts rule contains no valid IP".to_string());
    }

    Ok(HostsRule {
        matcher,
        answers: HostsAnswers { ipv4, ipv6 },
    })
}

fn parse_standard_hosts_line(
    raw: &str,
    builder: &mut RuleIndexBuilder,
) -> std::result::Result<(), String> {
    let (ip, hostnames) = parse_standard_hosts_fields(raw)?;
    let address = Arc::new(match ip {
        IpAddr::V4(ip) => RData::A(A(ip)),
        IpAddr::V6(ip) => RData::AAAA(AAAA(ip)),
    });
    for name in hostnames {
        builder.add_standard_address(normalize_name(name), &address);
    }
    Ok(())
}

fn parse_standard_hosts_fields(
    raw: &str,
) -> std::result::Result<(IpAddr, std::str::SplitWhitespace<'_>), String> {
    let mut fields = raw.split_whitespace();
    let Some(ip_raw) = fields.next() else {
        return Err("standard hosts rule must include an IP and at least one hostname".to_string());
    };
    let ip = ip_raw
        .parse::<IpAddr>()
        .map_err(|error| format!("invalid hosts IP '{ip_raw}': {error}"))?;
    if fields.clone().next().is_none() {
        return Err("standard hosts rule contains no hostname".to_string());
    }
    Ok((ip, fields))
}

fn parse_rule_matcher(raw_rule: &str) -> std::result::Result<RuleMatcher, String> {
    let raw_rule = raw_rule.trim();
    if raw_rule.is_empty() {
        return Err("empty hosts domain rule".to_string());
    }

    if let Some(v) = raw_rule.strip_prefix("full:") {
        return Ok(RuleMatcher::Full(normalize_name(v)));
    }
    if let Some(v) = raw_rule.strip_prefix("domain:") {
        return Ok(RuleMatcher::Domain(normalize_name(v)));
    }
    if let Some(v) = raw_rule.strip_prefix("keyword:") {
        return Ok(RuleMatcher::Keyword(normalize_name(v)));
    }
    if let Some(v) = raw_rule.strip_prefix("regexp:") {
        Regex::new(v).map_err(|e| format!("invalid hosts regexp '{}': {}", v, e))?;
        return Ok(RuleMatcher::Regexp(v.to_string()));
    }

    // unprefixed rules default to full match.
    Ok(RuleMatcher::Full(normalize_name(raw_rule)))
}

fn build_hosts_response(
    request: &Message,
    question: &Question,
    answers: &HostsAnswers,
) -> Result<Message> {
    match question.qtype() {
        RecordType::A if !answers.ipv4.is_empty() => {
            Ok(request.address_response_rdata(question, HOSTS_ANSWER_TTL, &answers.ipv4)?)
        }
        RecordType::AAAA if !answers.ipv6.is_empty() => {
            Ok(request.address_response_rdata(question, HOSTS_ANSWER_TTL, &answers.ipv6)?)
        }
        RecordType::A | RecordType::AAAA => Ok(synthetic_response::default_nodata_response(
            request, question,
        )),
        _ => Err(DnsError::protocol(
            "hosts synthetic response only supports A/AAAA questions",
        )),
    }
}

impl RuleIndexBuilder {
    fn add_standard_address(&mut self, name: String, address: &Arc<RData>) {
        let slot = if let Some(&slot) = self.full_rules.get(name.as_str()) {
            slot
        } else {
            let slot = self.push_answers(HostsAnswers::default());
            self.full_rules.insert(name.into_boxed_str(), slot);
            slot
        };
        let answers = Arc::make_mut(&mut self.payloads[slot]);
        let family = if matches!(address.as_ref(), RData::A(_)) {
            &mut answers.ipv4
        } else {
            &mut answers.ipv6
        };
        if !family.contains(address) {
            family.push(Arc::clone(address));
        }
    }

    fn add_rule(&mut self, rule: HostsRule) {
        match rule.matcher {
            RuleMatcher::Full(name) => self.add_full_rule(name, rule.answers),
            RuleMatcher::Domain(name) => self.add_domain_rule(name, rule.answers),
            RuleMatcher::Keyword(pattern) => {
                self.keyword_rules
                    .add(pattern, rule.answers, &mut self.payloads)
            }
            RuleMatcher::Regexp(pattern) => {
                self.regex_rules
                    .add(pattern, rule.answers, &mut self.payloads)
            }
        }
    }

    fn add_full_rule(&mut self, name: String, answers: HostsAnswers) {
        if let Some(slot) = self.full_rules.get(name.as_str()).copied() {
            self.payloads[slot] = Arc::new(answers);
        } else {
            let slot = self.push_answers(answers);
            self.full_rules.insert(name.into_boxed_str(), slot);
        }
    }

    fn add_domain_rule(&mut self, name: String, answers: HostsAnswers) {
        if let Some(slot) = self.domain_rules.exact_payload_slot(name.as_str()) {
            self.payloads[slot] = Arc::new(answers);
        } else {
            let slot = self.push_answers(answers);
            self.domain_rules.insert(name.as_str(), slot);
        }
    }

    fn push_answers(&mut self, answers: HostsAnswers) -> usize {
        let slot = self.payloads.len();
        self.payloads.push(Arc::new(answers));
        slot
    }

    fn build(self) -> Result<RuleIndex> {
        Ok(RuleIndex {
            payloads: self.payloads,
            full_rules: self.full_rules,
            domain_rules: self.domain_rules,
            keyword_rules: self.keyword_rules.build()?,
            regex_rules: self.regex_rules.build()?,
        })
    }
}

impl KeywordIndexBuilder {
    fn add(
        &mut self,
        pattern: String,
        answers: HostsAnswers,
        payloads: &mut Vec<Arc<HostsAnswers>>,
    ) {
        if let Some(&pattern_id) = self.pattern_ids.get(pattern.as_str()) {
            let payload_slot = self.payload_slots[pattern_id];
            payloads[payload_slot] = Arc::new(answers);
        } else {
            let payload_slot = payloads.len();
            payloads.push(Arc::new(answers));
            let pattern_id = self.patterns.len();
            self.pattern_ids
                .insert(pattern.clone().into_boxed_str(), pattern_id);
            self.patterns.push(pattern);
            self.payload_slots.push(payload_slot);
        }
    }

    fn build(self) -> Result<Option<KeywordIndex>> {
        if self.patterns.is_empty() {
            return Ok(None);
        }

        let matcher = AhoCorasickBuilder::new()
            .ascii_case_insensitive(false)
            .build(&self.patterns)
            .map_err(|e| {
                DnsError::plugin(format!("failed to build hosts keyword matcher: {}", e))
            })?;

        Ok(Some(KeywordIndex {
            matcher,
            payload_slots: self.payload_slots,
        }))
    }
}

impl RegexIndexBuilder {
    fn add(
        &mut self,
        pattern: String,
        answers: HostsAnswers,
        payloads: &mut Vec<Arc<HostsAnswers>>,
    ) {
        if let Some(&pattern_id) = self.pattern_ids.get(pattern.as_str()) {
            let payload_slot = self.payload_slots[pattern_id];
            payloads[payload_slot] = Arc::new(answers);
        } else {
            let payload_slot = payloads.len();
            payloads.push(Arc::new(answers));
            let pattern_id = self.patterns.len();
            self.pattern_ids
                .insert(pattern.clone().into_boxed_str(), pattern_id);
            self.patterns.push(pattern);
            self.payload_slots.push(payload_slot);
        }
    }

    fn build(self) -> Result<Option<RegexIndex>> {
        if self.patterns.is_empty() {
            return Ok(None);
        }

        let matcher = RegexSetBuilder::new(&self.patterns)
            .build()
            .map_err(|e| DnsError::plugin(format!("failed to build hosts regex matcher: {}", e)))?;

        Ok(Some(RegexIndex {
            matcher,
            payload_slots: self.payload_slots,
        }))
    }
}

impl RuleIndex {
    fn match_answers(&self, domain: &str) -> Option<&HostsAnswers> {
        if let Some(&slot) = self.full_rules.get(domain) {
            return Some(self.payloads[slot].as_ref());
        }

        if let Some(slot) = self.domain_rules.match_payload_slot(domain) {
            return Some(self.payloads[slot].as_ref());
        }

        if let Some(slot) = self
            .regex_rules
            .as_ref()
            .and_then(|index| index.match_payload_slot(domain))
        {
            return Some(self.payloads[slot].as_ref());
        }

        self.keyword_rules
            .as_ref()
            .and_then(|index| index.match_payload_slot(domain))
            .map(|slot| self.payloads[slot].as_ref())
    }
}

impl DomainTrie {
    fn insert(&mut self, domain: &str, payload_slot: usize) {
        let mut cursor = 0u32;
        for label in domain.rsplit('.') {
            if label.is_empty() {
                continue;
            }

            let next = if let Some(next) = self.nodes[cursor as usize].children.get(label) {
                *next
            } else {
                let idx = self.nodes.len() as u32;
                self.nodes.push(DomainTrieNode::default());
                self.nodes[cursor as usize]
                    .children
                    .insert(label.to_owned().into_boxed_str(), idx);
                idx
            };
            cursor = next;
        }

        self.nodes[cursor as usize].payload_slot = Some(payload_slot);
    }

    fn exact_payload_slot(&self, domain: &str) -> Option<usize> {
        let mut cursor = 0u32;
        for label in domain.rsplit('.') {
            if label.is_empty() {
                continue;
            }

            let next = self.nodes[cursor as usize].children.get(label)?;
            cursor = *next;
        }
        self.nodes[cursor as usize].payload_slot
    }

    fn match_payload_slot(&self, domain: &str) -> Option<usize> {
        let mut cursor = 0u32;
        let mut matched = self.nodes[cursor as usize].payload_slot;

        for label in domain.rsplit('.') {
            if label.is_empty() {
                continue;
            }

            let Some(next) = self.nodes[cursor as usize].children.get(label) else {
                break;
            };
            cursor = *next;
            if let Some(slot) = self.nodes[cursor as usize].payload_slot {
                matched = Some(slot);
            }
        }

        matched
    }
}

impl KeywordIndex {
    fn match_payload_slot(&self, domain: &str) -> Option<usize> {
        let mut best_pattern: Option<usize> = None;
        for matched in self.matcher.find_iter(domain) {
            let pattern_id = matched.pattern().as_usize();
            best_pattern = Some(best_pattern.map_or(pattern_id, |current| current.min(pattern_id)));
        }

        best_pattern.map(|pattern_id| self.payload_slots[pattern_id])
    }
}

impl RegexIndex {
    fn match_payload_slot(&self, domain: &str) -> Option<usize> {
        let matched = self.matcher.matches(domain);
        matched
            .iter()
            .next()
            .map(|pattern_id| self.payload_slots[pattern_id])
    }
}

fn normalize_name(raw: &str) -> String {
    raw.trim().trim_end_matches('.').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::{Ipv4Addr, SocketAddr};

    use tempfile::NamedTempFile;

    use super::*;
    use crate::proto::{DNSClass, Name, Question, Rcode};

    #[test]
    fn test_parse_hosts_line_validation() {
        assert!(parse_hosts_line("").is_err());
        assert!(parse_hosts_line("full:example.com").is_err());
        assert!(parse_hosts_line("full:example.com 1.1.1.1").is_ok());
    }

    #[test]
    fn test_hosts_format_defaults_and_validation() {
        assert_eq!(parse_config(None).unwrap().format, HostsFormat::Oxidns);
        for (yaml, expected) in [
            ("entries: []", HostsFormat::Oxidns),
            ("format: oxidns", HostsFormat::Oxidns),
            ("format: standard", HostsFormat::Standard),
        ] {
            let args = serde_yaml_ng::from_str(yaml).unwrap();
            assert_eq!(parse_config(Some(args)).unwrap().format, expected);
        }
        for format in ["unknown", "mosdns"] {
            let args = serde_yaml_ng::from_str(&format!("format: {format}")).unwrap();
            let error = parse_config(Some(args)).unwrap_err().to_string();
            assert!(error.contains("unknown variant"), "{error}");
        }
    }

    #[tokio::test]
    async fn test_hosts_omitted_and_explicit_oxidns_format_preserve_overrides() {
        for format in ["", "format: oxidns\n"] {
            let args = serde_yaml_ng::from_str(&format!(
                "{format}entries:\n  - 'example.test 192.0.2.1'\n  - 'example.test 192.0.2.2'\n"
            ))
            .unwrap();
            let plugin = build_plugin(parse_config(Some(args)).unwrap());
            let mut context = make_context("example.test.", RecordType::A);
            plugin.execute(&mut context).await.unwrap();
            assert_eq!(
                context.response().unwrap().answer_ips(),
                vec!["192.0.2.2".parse::<IpAddr>().unwrap()]
            );
        }
    }

    #[tokio::test]
    async fn test_standard_hosts_merges_aliases_and_families_across_sources() {
        let mut first_file = NamedTempFile::new().unwrap();
        first_file
            .write_all(
                b"\r\n # comment\r\n\t192.0.2.2\tEXAMPLE.test\tAlias.TEST.\t# trailing\r\n2001:db8::2 example.test alias.test\r\n",
            )
            .unwrap();
        let mut second_file = NamedTempFile::new().unwrap();
        writeln!(second_file, "192.0.2.1 example.test").unwrap();
        writeln!(second_file, "2001:0db8::1 example.test").unwrap();
        writeln!(second_file, "192.0.2.3 example.test").unwrap();
        let plugin = build_plugin(HostsConfig {
            format: HostsFormat::Standard,
            entries: vec![
                String::new(),
                "  # inline comment".into(),
                "192.0.2.1 Example.TEST. alias.test # trailing".into(),
                "2001:db8::1 example.test".into(),
                "192.0.2.1 example.test example.test".into(),
                "192.0.2.9 domain:literal.test".into(),
            ],
            files: vec![
                first_file.path().display().to_string(),
                second_file.path().display().to_string(),
            ],
            ..HostsConfig::default()
        });

        for (name, qtype, expected) in [
            (
                "EXAMPLE.test.",
                RecordType::A,
                vec!["192.0.2.1", "192.0.2.2", "192.0.2.3"],
            ),
            (
                "example.test.",
                RecordType::AAAA,
                vec!["2001:db8::1", "2001:db8::2"],
            ),
            ("alias.test.", RecordType::A, vec!["192.0.2.1", "192.0.2.2"]),
            ("alias.test.", RecordType::AAAA, vec!["2001:db8::2"]),
        ] {
            let mut context = make_context(name, qtype);
            plugin.execute(&mut context).await.unwrap();
            let response = context.response().unwrap();
            assert_eq!(
                response.answer_ips(),
                expected
                    .into_iter()
                    .map(|ip| ip.parse::<IpAddr>().unwrap())
                    .collect::<Vec<_>>()
            );
            assert!(
                response
                    .answers()
                    .iter()
                    .all(|rr| rr.ttl() == HOSTS_ANSWER_TTL)
            );
        }
        assert!(plugin.index.match_answers("www.example.test").is_none());
        assert!(plugin.index.match_answers("literal.test").is_none());
        assert!(plugin.index.match_answers("domain:literal.test").is_some());
    }

    #[test]
    fn test_standard_hosts_errors_include_source_location() {
        for rule in ["not-an-ip example.test", "192.0.2.1 # missing hostname"] {
            let cfg = HostsConfig {
                format: HostsFormat::Standard,
                entries: vec!["# comment".into(), rule.into()],
                ..HostsConfig::default()
            };
            let error = build_rule_index(&cfg).unwrap_err().to_string();
            assert!(error.contains("args.entries[1]"), "{error}");
            assert!(error.contains("invalid hosts rule"), "{error}");

            let mut file = NamedTempFile::new().unwrap();
            writeln!(file, "# comment\n{rule}").unwrap();
            let cfg = HostsConfig {
                format: HostsFormat::Standard,
                files: vec![file.path().display().to_string()],
                ..HostsConfig::default()
            };
            let error = build_rule_index(&cfg).unwrap_err().to_string();
            assert!(
                error.contains(&file.path().display().to_string()),
                "{error}"
            );
            assert!(error.contains("line 2"), "{error}");
        }
    }

    #[test]
    fn test_hosts_format_mismatch_suggests_other_format_with_location() {
        for (format, rule, configured, suggested) in [
            (
                HostsFormat::Oxidns,
                "192.0.2.1 example.test alias.test # standard comment",
                "oxidns",
                "standard",
            ),
            (
                HostsFormat::Oxidns,
                "2001:db8::1 example.test alias.test",
                "oxidns",
                "standard",
            ),
            (
                HostsFormat::Standard,
                "example.test 192.0.2.1 2001:db8::1",
                "standard",
                "oxidns",
            ),
            (
                HostsFormat::Standard,
                "domain:example.test 192.0.2.1",
                "standard",
                "oxidns",
            ),
        ] {
            let cfg = HostsConfig {
                format,
                entries: vec![rule.into()],
                ..HostsConfig::default()
            };
            let error = build_rule_index(&cfg).unwrap_err().to_string();
            assert!(error.contains("args.entries[0]"), "{error}");
            assert!(error.contains("format mismatch"), "{error}");
            assert!(
                error.contains(&format!("configured format: {configured}")),
                "{error}"
            );
            assert!(
                error.contains(&format!("set format: {suggested}")),
                "{error}"
            );

            let mut file = NamedTempFile::new().unwrap();
            let valid_rule = match format {
                HostsFormat::Oxidns => "valid.test 192.0.2.2",
                HostsFormat::Standard => "192.0.2.2 valid.test",
            };
            writeln!(file, "{valid_rule}\n{rule} # file comment").unwrap();
            let cfg = HostsConfig {
                format,
                files: vec![file.path().display().to_string()],
                ..HostsConfig::default()
            };
            let error = build_rule_index(&cfg).unwrap_err().to_string();
            assert!(
                error.contains(&file.path().display().to_string()),
                "{error}"
            );
            assert!(error.contains("line 2"), "{error}");
            assert!(
                error.contains(&format!("set format: {suggested}")),
                "{error}"
            );
        }
    }

    #[test]
    fn test_hosts_format_hint_preserves_inline_regexp_hash() {
        let cfg = HostsConfig {
            format: HostsFormat::Standard,
            entries: vec!["regexp:^[a-z#]+\\.example$ 192.0.2.1".into()],
            ..HostsConfig::default()
        };
        let error = build_rule_index(&cfg).unwrap_err().to_string();
        assert!(error.contains("set format: oxidns"), "{error}");
    }

    #[test]
    fn test_hosts_format_hints_do_not_guess_for_invalid_or_ambiguous_rules() {
        for format in [HostsFormat::Oxidns, HostsFormat::Standard] {
            for rule in [
                "not-an-ip example.test",
                "192.0.2.1",
                "example.test 192.0.2.1 invalid-ip",
            ] {
                let cfg = HostsConfig {
                    format,
                    entries: vec![rule.into()],
                    ..HostsConfig::default()
                };
                let error = build_rule_index(&cfg).unwrap_err().to_string();
                assert!(error.contains("invalid hosts rule"), "{error}");
                assert!(!error.contains("format mismatch"), "{error}");
                assert!(!error.contains("set format:"), "{error}");
            }

            let cfg = HostsConfig {
                format,
                entries: vec!["192.0.2.1 192.0.2.2".into()],
                ..HostsConfig::default()
            };
            let index = build_rule_index(&cfg).unwrap();
            let (matched, unmatched) = match format {
                HostsFormat::Oxidns => ("192.0.2.1", "192.0.2.2"),
                HostsFormat::Standard => ("192.0.2.2", "192.0.2.1"),
            };
            assert!(index.match_answers(matched).is_some());
            assert!(index.match_answers(unmatched).is_none());
        }
    }

    #[test]
    fn inline_regexp_preserves_hash_while_file_comments_are_stripped() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# ignored").unwrap();
        writeln!(file, "full:file.example 192.0.2.2 # trailing").unwrap();
        let cfg = HostsConfig {
            entries: vec!["regexp:^[a-z#]+\\.example$ 192.0.2.1".to_string()],
            files: vec![file.path().display().to_string()],
            short_circuit: false,
            ..HostsConfig::default()
        };
        let index = build_rule_index(&cfg).expect("inline regexp should build");

        assert!(index.match_answers("abc.example").is_some());
        assert!(index.match_answers("a#b.example").is_some());
        assert!(index.match_answers("file.example").is_some());
    }

    fn make_context(name: &str, qtype: RecordType) -> DnsContext {
        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii(name).unwrap(),
            qtype,
            DNSClass::IN,
        ));
        DnsContext::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)), request)
    }

    fn build_plugin(cfg: HostsConfig) -> HostsExecutor {
        HostsExecutor {
            tag: "hosts".to_string(),
            index: build_rule_index(&cfg).expect("rules should parse"),
            short_circuit: cfg.short_circuit,
            metrics: Arc::new(HostsMetrics::new("hosts".to_string())),
        }
    }

    #[tokio::test]
    async fn test_hosts_execute_defaults_unprefixed_rule_to_full_match() {
        let plugin = build_plugin(HostsConfig {
            entries: vec!["example.com 1.1.1.1".to_string()],
            files: vec![],
            short_circuit: false,
            ..HostsConfig::default()
        });

        let mut exact = make_context("example.com.", RecordType::A);
        plugin
            .execute(&mut exact)
            .await
            .expect("execute should work");
        assert_eq!(
            exact
                .response()
                .expect("response should exist")
                .answer_ips(),
            vec![IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))]
        );
        assert_eq!(plugin.metrics.hit_total.load(Ordering::Relaxed), 1);

        let mut subdomain = make_context("www.example.com.", RecordType::A);
        plugin
            .execute(&mut subdomain)
            .await
            .expect("execute should work");
        assert!(subdomain.response().is_none());
        assert_eq!(plugin.metrics.miss_total.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_hosts_execute_prefers_longest_domain_suffix() {
        let plugin = build_plugin(HostsConfig {
            entries: vec![
                "domain:example.com 192.0.2.10".to_string(),
                "domain:svc.example.com 192.0.2.11".to_string(),
            ],
            files: vec![],
            short_circuit: false,
            ..HostsConfig::default()
        });

        let mut ctx = make_context("api.svc.example.com.", RecordType::A);
        plugin.execute(&mut ctx).await.expect("execute should work");

        assert_eq!(
            ctx.response().expect("response should exist").answer_ips(),
            vec![IpAddr::V4(Ipv4Addr::new(192, 0, 2, 11))]
        );
    }

    #[tokio::test]
    async fn test_hosts_execute_respects_family_priority() {
        let plugin = build_plugin(HostsConfig {
            entries: vec![
                "keyword:test 192.0.2.1".to_string(),
                "regexp:^svc\\.example\\.com$ 192.0.2.2".to_string(),
                "domain:example.com 192.0.2.3".to_string(),
                "full:api.example.com 192.0.2.4".to_string(),
                "regexp:^api-only\\.test$ 192.0.2.5".to_string(),
            ],
            files: vec![],
            short_circuit: false,
            ..HostsConfig::default()
        });

        let mut full_hit = make_context("api.example.com.", RecordType::A);
        plugin
            .execute(&mut full_hit)
            .await
            .expect("execute should work");
        assert_eq!(
            full_hit
                .response()
                .expect("response should exist")
                .answer_ips(),
            vec![IpAddr::V4(Ipv4Addr::new(192, 0, 2, 4))]
        );

        let mut domain_hit = make_context("svc.example.com.", RecordType::A);
        plugin
            .execute(&mut domain_hit)
            .await
            .expect("execute should work");
        assert_eq!(
            domain_hit
                .response()
                .expect("response should exist")
                .answer_ips(),
            vec![IpAddr::V4(Ipv4Addr::new(192, 0, 2, 3))]
        );

        let mut regex_hit = make_context("api-only.test.", RecordType::A);
        plugin
            .execute(&mut regex_hit)
            .await
            .expect("execute should work");
        assert_eq!(
            regex_hit
                .response()
                .expect("response should exist")
                .answer_ips(),
            vec![IpAddr::V4(Ipv4Addr::new(192, 0, 2, 5))]
        );
    }

    #[tokio::test]
    async fn test_hosts_execute_replaces_duplicate_rules_in_load_order() {
        let plugin = build_plugin(HostsConfig {
            entries: vec![
                "full:example.com 192.0.2.10".to_string(),
                "full:example.com 192.0.2.11".to_string(),
                "keyword:test 192.0.2.20".to_string(),
                "keyword:test 192.0.2.21".to_string(),
            ],
            files: vec![],
            short_circuit: false,
            ..HostsConfig::default()
        });

        let mut full_ctx = make_context("example.com.", RecordType::A);
        plugin
            .execute(&mut full_ctx)
            .await
            .expect("execute should work");
        assert_eq!(
            full_ctx
                .response()
                .expect("response should exist")
                .answer_ips(),
            vec![IpAddr::V4(Ipv4Addr::new(192, 0, 2, 11))]
        );

        let mut keyword_ctx = make_context("cache.test.", RecordType::A);
        plugin
            .execute(&mut keyword_ctx)
            .await
            .expect("execute should work");
        assert_eq!(
            keyword_ctx
                .response()
                .expect("response should exist")
                .answer_ips(),
            vec![IpAddr::V4(Ipv4Addr::new(192, 0, 2, 21))]
        );
    }

    #[tokio::test]
    async fn test_hosts_execute_uses_ttl_10_for_positive_answers() {
        let plugin = build_plugin(HostsConfig {
            entries: vec!["full:example.com 1.1.1.1".to_string()],
            files: vec![],
            short_circuit: false,
            ..HostsConfig::default()
        });

        let mut ctx = make_context("example.com.", RecordType::A);
        plugin.execute(&mut ctx).await.expect("execute should work");

        let response = ctx.response().expect("response should exist");
        assert_eq!(response.answers().len(), 1);
        assert_eq!(response.answers()[0].ttl(), HOSTS_ANSWER_TTL);
    }

    #[tokio::test]
    async fn test_hosts_execute_returns_nodata_with_fake_soa_for_family_mismatch() {
        let plugin = build_plugin(HostsConfig {
            entries: vec!["full:example.com 1.1.1.1".to_string()],
            files: vec![],
            short_circuit: false,
            ..HostsConfig::default()
        });

        let mut ctx = make_context("example.com.", RecordType::AAAA);
        let step = plugin.execute(&mut ctx).await.expect("execute should work");

        assert!(matches!(step, ExecStep::Next));
        let response = ctx.response().expect("response should exist");
        assert_eq!(response.rcode(), Rcode::NoError);
        assert!(response.answers().is_empty());
        assert_eq!(response.authorities().len(), 1);
        assert_eq!(response.authorities()[0].rr_type(), RecordType::SOA);
        assert_eq!(
            response.authorities()[0].ttl(),
            synthetic_response::DEFAULT_FAKE_SOA_TTL
        );
    }

    #[tokio::test]
    async fn test_hosts_execute_stops_on_empty_local_answer_when_short_circuit_enabled() {
        let plugin = build_plugin(HostsConfig {
            entries: vec!["full:example.com 1.1.1.1".to_string()],
            files: vec![],
            short_circuit: true,
            ..HostsConfig::default()
        });

        let mut ctx = make_context("example.com.", RecordType::AAAA);
        let step = plugin.execute(&mut ctx).await.expect("execute should work");

        assert!(matches!(step, ExecStep::Stop));
        assert!(ctx.response().is_some());
    }

    #[tokio::test]
    async fn test_hosts_execute_ignores_multi_question_requests() {
        let plugin = build_plugin(HostsConfig {
            entries: vec!["full:example.com 1.1.1.1".to_string()],
            files: vec![],
            short_circuit: false,
            ..HostsConfig::default()
        });

        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii("example.com.").unwrap(),
            RecordType::A,
            DNSClass::IN,
        ));
        request.add_question(Question::new(
            Name::from_ascii("example.net.").unwrap(),
            RecordType::A,
            DNSClass::IN,
        ));
        let mut ctx = DnsContext::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)), request);

        let step = plugin.execute(&mut ctx).await.expect("execute should work");
        assert!(matches!(step, ExecStep::Next));
        assert!(ctx.response().is_none());
    }

    #[test]
    fn test_build_rule_index_loads_files_after_entries() {
        let mut file = NamedTempFile::new().expect("temp file should be created");
        writeln!(file, "full:example.com 192.0.2.11").expect("temp file should be writable");

        let cfg = HostsConfig {
            entries: vec!["full:example.com 192.0.2.10".to_string()],
            files: vec![file.path().to_string_lossy().to_string()],
            short_circuit: false,
            ..HostsConfig::default()
        };
        let index = build_rule_index(&cfg).expect("rule index should build");
        let answers = index
            .match_answers("example.com")
            .expect("example.com should match");
        let response = build_hosts_response(
            &Message::new(),
            &Question::new(
                Name::from_ascii("example.com.").unwrap(),
                RecordType::A,
                DNSClass::IN,
            ),
            answers,
        )
        .expect("response should build");

        assert_eq!(
            response.answer_ips(),
            vec![IpAddr::V4(Ipv4Addr::new(192, 0, 2, 11))]
        );
    }
}
