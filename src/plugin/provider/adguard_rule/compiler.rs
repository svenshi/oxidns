// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use ahash::AHashSet;
use tracing::warn;

use super::config::AdGuardRuleConfig;
use super::model::{BuildStats, CompiledRule, CompiledRuleSet, DnsTypeConstraint};
use super::syntax::{
    ParsedLine, RuleDetails, RuleMeta, SkipReason, canonical_pattern_key, compile_pattern,
    fast_rule_kind, parse_line, parse_rule_details, validate_pattern, with_fast_rule,
};
use crate::core::rule_matcher::DomainRuleKind;
use crate::infra::error::{DnsError, Result as DnsResult};
use crate::infra::io::{LineClassifier, TextLocation, TextSource, TextSourceSession};

const MAX_SKIP_SAMPLES: usize = 5;
const MAX_SAMPLE_RULE_CHARS: usize = 160;

#[derive(Debug, Default, Clone, Copy)]
struct RuleSetCapacity {
    full: usize,
    regexp: usize,
    conditional: usize,
}

impl RuleSetCapacity {
    fn add(&mut self, meta: &RuleMeta<'_>) -> Result<(), String> {
        if meta.is_conditional() {
            self.conditional += 1;
            return Ok(());
        }
        match fast_rule_kind(meta.pattern)? {
            DomainRuleKind::Full => self.full += 1,
            // The suffix trie deliberately retains its original node layout
            // and insertion behavior. It has no bulk-reserve operation.
            DomainRuleKind::Domain => {}
            DomainRuleKind::Regexp => self.regexp += 1,
            DomainRuleKind::Keyword => unreachable!("adguard rules do not produce keyword rules"),
        }
        Ok(())
    }

    fn reserve(self, set: &mut CompiledRuleSet) {
        set.fast_matcher.reserve_rules(self.full, 0, self.regexp);
        set.conditional_rules.reserve(self.conditional);
    }
}

#[derive(Debug, Default)]
struct BuildCapacities {
    important_exceptions: RuleSetCapacity,
    important_blocks: RuleSetCapacity,
    exceptions: RuleSetCapacity,
    blocks: RuleSetCapacity,
}

impl BuildCapacities {
    fn target_mut(&mut self, meta: &RuleMeta<'_>) -> &mut RuleSetCapacity {
        match (meta.important, meta.is_exception) {
            (true, true) => &mut self.important_exceptions,
            (true, false) => &mut self.important_blocks,
            (false, true) => &mut self.exceptions,
            (false, false) => &mut self.blocks,
        }
    }
}

struct RuleBuckets {
    important_exceptions: CompiledRuleSet,
    important_blocks: CompiledRuleSet,
    exceptions: CompiledRuleSet,
    blocks: CompiledRuleSet,
}

impl RuleBuckets {
    fn with_capacities(capacities: BuildCapacities) -> Self {
        let mut buckets = Self {
            important_exceptions: CompiledRuleSet::default(),
            important_blocks: CompiledRuleSet::default(),
            exceptions: CompiledRuleSet::default(),
            blocks: CompiledRuleSet::default(),
        };
        capacities
            .important_exceptions
            .reserve(&mut buckets.important_exceptions);
        capacities
            .important_blocks
            .reserve(&mut buckets.important_blocks);
        capacities.exceptions.reserve(&mut buckets.exceptions);
        capacities.blocks.reserve(&mut buckets.blocks);
        buckets
    }

    fn target_mut(&mut self, meta: &RuleMeta<'_>) -> &mut CompiledRuleSet {
        match (meta.important, meta.is_exception) {
            (true, true) => &mut self.important_exceptions,
            (true, false) => &mut self.important_blocks,
            (false, true) => &mut self.exceptions,
            (false, false) => &mut self.blocks,
        }
    }

    fn finalize(&mut self, tag: &str) -> DnsResult<()> {
        for set in [
            &mut self.important_exceptions,
            &mut self.important_blocks,
            &mut self.exceptions,
            &mut self.blocks,
        ] {
            set.finalize().map_err(|error| {
                DnsError::plugin(format!(
                    "adguard_rule '{}' failed to finalize compiled matcher: {}",
                    tag, error
                ))
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct SkipStats {
    count: usize,
    samples: Vec<String>,
}

impl SkipStats {
    fn record(&mut self, source: TextLocation<'_>, raw: &str) {
        self.count += 1;
        if self.samples.len() >= MAX_SKIP_SAMPLES {
            return;
        }
        let mut sample_rule = raw.chars().take(MAX_SAMPLE_RULE_CHARS).collect::<String>();
        if raw.chars().count() > MAX_SAMPLE_RULE_CHARS {
            sample_rule.push('…');
        }
        self.samples.push(format!("{source}: {sample_rule}"));
    }
}

pub(super) fn build_rule_buckets(
    tag: &str,
    cfg: &AdGuardRuleConfig,
) -> DnsResult<(
    CompiledRuleSet,
    CompiledRuleSet,
    CompiledRuleSet,
    CompiledRuleSet,
    BuildStats,
)> {
    // Sources without badfilter need one planning scan and one compilation
    // scan. A non-empty global badfilter set adds an active-capacity scan so
    // disabled rules do not leave proportional allocations in the snapshot.
    let source = TextSource::new("args.rules", &cfg.rules, &cfg.files);
    let classifier = LineClassifier::new(&["!", "#"]);
    let mut session = source.open_replay().map_err(|error| {
        DnsError::plugin(format!(
            "adguard_rule '{tag}' failed to open rule sources: {error}"
        ))
    })?;
    let (badfilter_keys, capacities) = plan_build(tag, &mut session, &classifier)?;
    let mut buckets = RuleBuckets::with_capacities(capacities);
    let mut stats = BuildStats::default();
    let mut skip_stats: [SkipStats; 5] = std::array::from_fn(|_| SkipStats::default());

    session
        .scan(&classifier, |line| -> Result<(), String> {
            stats.total_rules += 1;
            let raw = line.raw();
            let parsed = parse_line(raw, line.annotations().leading_comment)?;
            let meta = match parsed {
                ParsedLine::Ignored => {
                    stats.skipped_rules += 1;
                    return Ok(());
                }
                ParsedLine::Skipped(reason) => {
                    stats.skipped_rules += 1;
                    skip_stats[reason.index()].record(line.location(), raw);
                    return Ok(());
                }
                ParsedLine::Rule(meta) => meta,
            };

            stats.supported_rules += 1;
            if meta.is_exception {
                stats.exception_rules += 1;
            }
            if meta.important {
                stats.important_rules += 1;
            }

            let needs_details =
                meta.is_conditional() || meta.badfilter || !badfilter_keys.is_empty();
            let details = needs_details
                .then(|| parse_rule_details(&meta))
                .transpose()?;

            if meta.badfilter {
                return Ok(());
            }
            if let Some(details) = details.as_ref()
                && !badfilter_keys.is_empty()
                && badfilter_keys.contains(&rule_cache_key(&meta, details))
            {
                stats.disabled_rules += 1;
                return Ok(());
            }

            let target = buckets.target_mut(&meta);
            if meta.is_conditional() {
                let details = details.expect("conditional rule details must be parsed");
                let matcher = compile_pattern(meta.pattern)?;
                target.conditional_rules.push(CompiledRule {
                    matcher,
                    dnstype: details.dnstype,
                    denyallow: details.denyallow,
                });
            } else {
                with_fast_rule(meta.pattern, |kind, value| {
                    target.fast_matcher.add_rule(kind, value, "")
                })?;
            }
            Ok(())
        })
        .map_err(|error| {
            DnsError::plugin(format!("adguard_rule '{tag}' second scan failed: {error}"))
        })?;

    buckets.finalize(tag)?;
    log_skip_summary(tag, &skip_stats);

    Ok((
        buckets.important_exceptions,
        buckets.important_blocks,
        buckets.exceptions,
        buckets.blocks,
        stats,
    ))
}

fn plan_build(
    tag: &str,
    session: &mut TextSourceSession<'_>,
    classifier: &LineClassifier<'_>,
) -> DnsResult<(AHashSet<BadfilterKey>, BuildCapacities)> {
    let mut badfilter_keys = AHashSet::new();
    let mut capacities = BuildCapacities::default();

    session
        .scan(classifier, |line| -> Result<(), String> {
            let ParsedLine::Rule(meta) =
                parse_line(line.raw(), line.annotations().leading_comment)?
            else {
                return Ok(());
            };

            // The planning pass validates the complete supported subset. It
            // never retains a matcher or emits per-line
            // diagnostics.
            validate_pattern(meta.pattern)?;
            let details = parse_rule_details(&meta)?;

            if meta.badfilter {
                badfilter_keys.insert(BadfilterKey::new(&meta, details));
            } else {
                capacities.target_mut(&meta).add(&meta)?;
            }
            Ok(())
        })
        .map_err(|error| {
            DnsError::plugin(format!(
                "adguard_rule '{tag}' planning scan failed: {error}"
            ))
        })?;

    // A badfilter is global and order-independent. Once all keys are known,
    // replay the same opened file snapshot to reserve only for active rules.
    // This avoids retaining capacity proportional to disabled rules.
    if !badfilter_keys.is_empty() {
        capacities = BuildCapacities::default();
        session
            .scan(classifier, |line| -> Result<(), String> {
                let ParsedLine::Rule(meta) =
                    parse_line(line.raw(), line.annotations().leading_comment)?
                else {
                    return Ok(());
                };
                if meta.badfilter {
                    return Ok(());
                }
                let details = parse_rule_details(&meta)?;
                if !badfilter_keys.contains(&rule_cache_key(&meta, &details)) {
                    capacities.target_mut(&meta).add(&meta)?;
                }
                Ok(())
            })
            .map_err(|error| {
                DnsError::plugin(format!(
                    "adguard_rule '{tag}' active-capacity scan failed: {error}"
                ))
            })?;
    }

    Ok((badfilter_keys, capacities))
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct BadfilterKey {
    is_exception: bool,
    important: bool,
    pattern: Box<str>,
    dnstype: Option<DnsTypeConstraint>,
    denyallow: Vec<String>,
}

impl BadfilterKey {
    fn new(meta: &RuleMeta<'_>, details: RuleDetails) -> Self {
        Self {
            is_exception: meta.is_exception,
            important: meta.important,
            pattern: canonical_pattern_key(meta.pattern).into_boxed_str(),
            dnstype: details.dnstype,
            denyallow: details.denyallow,
        }
    }
}

fn rule_cache_key(meta: &RuleMeta<'_>, details: &RuleDetails) -> BadfilterKey {
    BadfilterKey {
        is_exception: meta.is_exception,
        important: meta.important,
        pattern: canonical_pattern_key(meta.pattern).into_boxed_str(),
        dnstype: details.dnstype.clone(),
        denyallow: details.denyallow.clone(),
    }
}

fn log_skip_summary(tag: &str, stats: &[SkipStats; 5]) {
    for reason in SkipReason::ALL {
        let stats = &stats[reason.index()];
        if stats.count == 0 {
            continue;
        }
        warn!(
            tag,
            reason = reason.label(),
            skipped_rules = stats.count,
            samples = ?stats.samples,
            "adguard_rule skipped unsupported rules"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipped_rule_samples_are_bounded() {
        let mut stats = SkipStats::default();
        for index in 0..10 {
            stats.record(
                TextLocation::Inline {
                    field: "args.rules",
                    index,
                },
                "unsupported-rule",
            );
        }
        assert_eq!(stats.count, 10);
        assert_eq!(stats.samples.len(), MAX_SKIP_SAMPLES);
        assert!(stats.samples[0].contains("args.rules[0]"));
    }

    #[test]
    fn planning_reserves_only_rules_not_disabled_by_badfilter() {
        let cfg = AdGuardRuleConfig {
            rules: vec![
                "disabled.example".to_string(),
                "disabled.example$badfilter".to_string(),
                "/disabled/$".to_string(),
                "/disabled/$badfilter".to_string(),
                "active.example".to_string(),
            ],
            files: Vec::new(),
        };
        let source = TextSource::new("args.rules", &cfg.rules, &cfg.files);
        let classifier = LineClassifier::new(&["!", "#"]);
        let mut session = source.open_replay().unwrap();

        let (badfilter_keys, capacities) = plan_build("agh", &mut session, &classifier).unwrap();

        assert_eq!(badfilter_keys.len(), 2);
        assert_eq!(capacities.blocks.full, 1);
        assert_eq!(capacities.blocks.regexp, 0);
        assert_eq!(capacities.blocks.conditional, 0);
    }
}
