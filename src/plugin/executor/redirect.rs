// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `redirect` executor plugin.
//!
//! Rewrites request qname to target qname, executes subsequent chain, then
//! restores original question and prepends a CNAME answer.
//!
//! Two-stage behavior:
//! - `execute`: match rule by original qname and replace request query name
//!   with redirect target.
//! - continuation post-stage: restore original query name in request/response
//!   and add synthetic CNAME from original -> target before upstream answers.
//!
//! This keeps downstream resolution consistent with redirected target while
//! still returning a client-facing CNAME chain.

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
use crate::plugin::executor::{ExecStep, Executor, ExecutorNext};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::proto::{CNAME, DNSClass, Name, Question, RData, Record};
use crate::{continue_next, plugin_factory};

#[derive(Debug, Clone, Deserialize, Default)]
struct RedirectConfig {
    /// Inline redirect rules.
    #[serde(default)]
    rules: Vec<String>,
    /// Paths to redirect rule files.
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Clone)]
enum RuleMatcher {
    Full(String),
    Domain(String),
    Keyword(String),
    Regexp(String),
}

#[derive(Debug, Clone)]
struct RedirectRule {
    matcher: RuleMatcher,
    target: Name,
}

#[derive(Debug)]
struct RedirectExecutor {
    tag: String,
    targets: Vec<Name>,
    index: RuleIndex,
}

#[derive(Debug, Default)]
struct RuleIndex {
    full_rules: AHashMap<Box<str>, usize>,
    domain_rules: AHashMap<Box<str>, usize>,
    keyword_matcher: Option<AhoCorasick>,
    keyword_rule_indices: Vec<usize>,
    regex_matcher: Option<RegexSet>,
    regex_rule_indices: Vec<usize>,
}

#[derive(Debug, Default)]
struct RuleIndexBuilder {
    targets: Vec<Name>,
    index: RuleIndex,
    keyword_patterns: Vec<String>,
    regex_patterns: Vec<String>,
}

#[async_trait]
impl Plugin for RedirectExecutor {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Executor for RedirectExecutor {
    fn with_next(&self) -> bool {
        true
    }

    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        self.execute_with_next(context, None).await
    }

    #[hotpath::measure]
    async fn execute_with_next(
        &self,
        context: &mut DnsContext,
        next: Option<ExecutorNext>,
    ) -> Result<ExecStep> {
        let Some((original, target)) = self.match_target(context)? else {
            return continue_next!(next, context);
        };

        set_query_name(context, &target)?;
        let step_result = continue_next!(next, context);
        self.finish_redirect(context, original, target, step_result)
    }
}

impl RedirectExecutor {
    fn finish_redirect(
        &self,
        context: &mut DnsContext,
        original: Name,
        target: Name,
        step_result: Result<ExecStep>,
    ) -> Result<ExecStep> {
        set_query_name(context, &original)?;
        let step = step_result?;
        let Some(response) = context.response_mut() else {
            return Ok(step);
        };

        for query in response.questions_mut() {
            if query.name() == &target {
                query.set_name(original.clone());
            }
        }

        let answers = response.answers_mut();
        // CNAME must be first in the answer section so glibc and other strict
        // resolvers can follow the chain (RFC 1034 §3.6.2).
        answers.insert(
            0,
            Record::from_rdata(original, 1, RData::CNAME(CNAME(target))),
        );
        Ok(step)
    }

    fn match_target(&self, context: &DnsContext) -> Result<Option<(Name, Name)>> {
        let Some(question) = context.request.first_question() else {
            return Ok(None);
        };

        if question.qclass() != DNSClass::IN {
            return Ok(None);
        }

        let Some(rule) = self
            .index
            .match_target(&self.targets, question.name().normalized())
        else {
            return Ok(None);
        };

        Ok(Some((question.name().clone(), rule.clone())))
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("redirect")]
pub struct RedirectFactory;

impl PluginFactory for RedirectFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let cfg = parse_config(plugin_config.args.clone())?;
        let (targets, index) = build_rules(&cfg)?;

        Ok(UninitializedPlugin::Executor(Box::new(RedirectExecutor {
            tag: plugin_config.tag.clone(),
            targets,
            index,
        })))
    }
}

fn parse_config(args: Option<Value>) -> Result<RedirectConfig> {
    let Some(args) = args else {
        return Ok(RedirectConfig::default());
    };

    serde_yaml_ng::from_value(args)
        .map_err(|e| DnsError::plugin(format!("failed to parse redirect config: {}", e)))
}

fn build_rules(cfg: &RedirectConfig) -> Result<(Vec<Name>, RuleIndex)> {
    let mut builder = RuleIndexBuilder::default();
    TextSource::new("args.rules", &cfg.rules, &cfg.files)
        .scan(
            &LineClassifier::new(&["#"]),
            |line| -> std::result::Result<(), String> {
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
                let rule = parse_redirect_rule(rule_text)
                    .map_err(|error| format!("invalid redirect rule '{rule_text}': {error}"))?;
                builder.add_rule(rule);
                Ok(())
            },
        )
        .map_err(|error| DnsError::plugin(error.to_string()))?;
    builder.build()
}

fn parse_redirect_rule(raw: &str) -> std::result::Result<RedirectRule, String> {
    let mut fields = raw.split_whitespace();
    let matcher_raw = fields
        .next()
        .ok_or_else(|| "redirect rule requires exactly 2 fields, got 0".to_string())?;
    let target_raw = fields
        .next()
        .ok_or_else(|| "redirect rule requires exactly 2 fields, got 1".to_string())?;
    if fields.next().is_some() {
        return Err("redirect rule requires exactly 2 fields, got more".to_string());
    }
    let matcher = parse_rule_matcher(matcher_raw)?;
    let target = parse_name(target_raw)?;

    Ok(RedirectRule { matcher, target })
}

fn parse_rule_matcher(raw_rule: &str) -> std::result::Result<RuleMatcher, String> {
    if let Some(v) = raw_rule.strip_prefix("full:") {
        return Ok(RuleMatcher::Full(normalize_name(v)));
    }
    if let Some(v) = raw_rule.strip_prefix("domain:") {
        return Ok(RuleMatcher::Domain(normalize_name(v)));
    }
    if let Some(v) = raw_rule.strip_prefix("keyword:") {
        return Ok(RuleMatcher::Keyword(v.to_ascii_lowercase()));
    }
    if let Some(v) = raw_rule.strip_prefix("regexp:") {
        Regex::new(v).map_err(|e| format!("invalid regexp '{}': {}", v, e))?;
        return Ok(RuleMatcher::Regexp(v.to_string()));
    }

    // redirect defaults to full match when prefix is omitted.
    Ok(RuleMatcher::Full(normalize_name(raw_rule)))
}

impl RuleIndexBuilder {
    fn add_rule(&mut self, rule: RedirectRule) {
        let rule_idx = self.targets.len();
        self.targets.push(rule.target);
        match rule.matcher {
            RuleMatcher::Full(value) => {
                self.index
                    .full_rules
                    .entry(value.into_boxed_str())
                    .or_insert(rule_idx);
            }
            RuleMatcher::Domain(value) => {
                self.index
                    .domain_rules
                    .entry(value.into_boxed_str())
                    .or_insert(rule_idx);
            }
            RuleMatcher::Keyword(value) => {
                self.keyword_patterns.push(value);
                self.index.keyword_rule_indices.push(rule_idx);
            }
            RuleMatcher::Regexp(value) => {
                self.regex_patterns.push(value);
                self.index.regex_rule_indices.push(rule_idx);
            }
        }
    }

    fn build(mut self) -> Result<(Vec<Name>, RuleIndex)> {
        if !self.keyword_patterns.is_empty() {
            self.index.keyword_matcher = Some(
                AhoCorasickBuilder::new()
                    .ascii_case_insensitive(false)
                    .build(&self.keyword_patterns)
                    .map_err(|e| {
                        DnsError::plugin(format!("failed to build redirect keyword matcher: {}", e))
                    })?,
            );
        }

        if !self.regex_patterns.is_empty() {
            self.index.regex_matcher = Some(
                RegexSetBuilder::new(&self.regex_patterns)
                    .build()
                    .map_err(|e| {
                        DnsError::plugin(format!("failed to build redirect regex matcher: {}", e))
                    })?,
            );
        }

        Ok((self.targets, self.index))
    }
}

impl RuleIndex {
    fn match_target<'a>(&self, targets: &'a [Name], domain: &str) -> Option<&'a Name> {
        let mut best: Option<usize> = None;

        if let Some(rule_idx) = self.full_rules.get(domain) {
            best = Some(*rule_idx);
        }

        let mut suffix = domain;
        loop {
            if let Some(rule_idx) = self.domain_rules.get(suffix) {
                best = Some(best.map_or(*rule_idx, |cur| cur.min(*rule_idx)));
            }
            let Some(dot) = suffix.find('.') else {
                break;
            };
            suffix = &suffix[dot + 1..];
        }

        if let Some(matcher) = &self.keyword_matcher {
            for m in matcher.find_iter(domain) {
                let rule_idx = self.keyword_rule_indices[m.pattern().as_usize()];
                best = Some(best.map_or(rule_idx, |cur| cur.min(rule_idx)));
            }
        }

        if let Some(matcher) = &self.regex_matcher {
            let matched = matcher.matches(domain);
            for pid in matched.iter() {
                let rule_idx = self.regex_rule_indices[pid];
                best = Some(best.map_or(rule_idx, |cur| cur.min(rule_idx)));
            }
        }

        best.map(|idx| &targets[idx])
    }
}

fn set_query_name(context: &mut DnsContext, name: &Name) -> Result<()> {
    let Some(question) = context.request_mut().first_question_mut() else {
        return Err(DnsError::plugin("redirect requires one question"));
    };
    question.set_name(name.clone());
    Ok(())
}

fn parse_name(raw: &str) -> std::result::Result<Name, String> {
    let fqdn = if raw.ends_with('.') {
        raw.to_string()
    } else {
        format!("{}.", raw)
    };
    Name::from_ascii(&fqdn).map_err(|e| format!("invalid domain '{}': {}", raw, e))
}

#[inline]
fn normalize_name(raw: &str) -> String {
    raw.trim().trim_end_matches('.').to_ascii_lowercase()
}

#[allow(dead_code)]
fn _question_name(question: &Question) -> &Name {
    question.name()
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::{Ipv4Addr, SocketAddr};

    use tempfile::NamedTempFile;

    use super::*;
    use crate::core::context::DnsContext;
    use crate::infra::error::DnsError;
    use crate::proto::rdata::A;
    use crate::proto::{Message, RData, RecordType};

    #[test]
    fn test_parse_redirect_rule_validation() {
        assert!(parse_redirect_rule("bad_rule").is_err());
        assert!(parse_redirect_rule("full:example.com target.example.com").is_ok());
    }

    #[test]
    fn inline_regexp_preserves_hash_while_file_comments_are_stripped() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# ignored").unwrap();
        writeln!(file, "full:file.test file-target.example # trailing").unwrap();
        let cfg = RedirectConfig {
            rules: vec!["regexp:^[a-z#]+\\.example$ target.example".to_string()],
            files: vec![file.path().display().to_string()],
        };
        let (targets, index) = build_rules(&cfg).expect("inline regexp should build");

        assert!(index.match_target(&targets, "abc.example").is_some());
        assert!(index.match_target(&targets, "a#b.example").is_some());
        assert_eq!(
            index.match_target(&targets, "file.test").unwrap().to_fqdn(),
            "file-target.example."
        );
    }

    fn make_context(name: &str) -> DnsContext {
        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii(name).unwrap(),
            RecordType::A,
            DNSClass::IN,
        ));
        DnsContext::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)), request)
    }

    #[tokio::test]
    async fn test_redirect_with_next_full_flow() {
        let mut builder = RuleIndexBuilder::default();
        builder.add_rule(parse_redirect_rule("full:example.com target.example.com").unwrap());
        let (targets, index) = builder.build().expect("rule index should build");
        let plugin = RedirectExecutor {
            tag: "redirect".to_string(),
            targets,
            index,
        };

        let mut ctx = make_context("example.com.");
        let mut response = Message::new();
        response.add_question(Question::new(
            Name::from_ascii("target.example.com.").unwrap(),
            RecordType::A,
            DNSClass::IN,
        ));
        response.add_answer(Record::from_rdata(
            Name::from_ascii("target.example.com.").unwrap(),
            60,
            RData::A(A::new(1, 1, 1, 1)),
        ));
        ctx.set_response(response);

        plugin
            .execute_with_next(&mut ctx, None)
            .await
            .expect("continuation execute should succeed");

        assert_eq!(
            ctx.request
                .first_question()
                .expect("question should exist")
                .name()
                .to_fqdn(),
            "example.com."
        );

        let response = ctx.response().expect("response should exist");
        assert_eq!(response.questions()[0].name().to_fqdn(), "example.com.");
        assert_eq!(response.answers().len(), 2);
        // CNAME must be first so glibc resolvers can follow the chain
        // correctly.
        assert_eq!(response.answers()[0].rr_type(), RecordType::CNAME);
        assert_eq!(response.answers()[1].rr_type(), RecordType::A);
    }

    #[test]
    fn test_finish_redirect_restores_query_name_when_next_errors() {
        let mut builder = RuleIndexBuilder::default();
        builder.add_rule(parse_redirect_rule("full:example.com target.example.com").unwrap());
        let (targets, index) = builder.build().expect("rule index should build");
        let plugin = RedirectExecutor {
            tag: "redirect".to_string(),
            targets,
            index,
        };
        let mut ctx = make_context("example.com.");
        let original = Name::from_ascii("example.com.").unwrap();
        let target = Name::from_ascii("target.example.com.").unwrap();

        set_query_name(&mut ctx, &target).expect("redirect target should be valid");
        let err = plugin
            .finish_redirect(
                &mut ctx,
                original,
                target,
                Err(DnsError::plugin("downstream failed")),
            )
            .expect_err("downstream failure should propagate");

        assert!(err.to_string().contains("downstream failed"));
        assert_eq!(
            ctx.request
                .first_question()
                .expect("question should exist")
                .name()
                .to_fqdn(),
            "example.com."
        );
    }

    #[test]
    fn first_rule_wins_across_matcher_families_and_file_sources() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "full:api.example.com file-target.example").unwrap();
        let cfg = RedirectConfig {
            rules: vec!["keyword:api inline-target.example".to_string()],
            files: vec![file.path().display().to_string()],
        };
        let (targets, index) = build_rules(&cfg).unwrap();
        let target = index
            .match_target(&targets, "api.example.com")
            .expect("rule should match");
        assert_eq!(target.to_fqdn(), "inline-target.example.");
    }
}
