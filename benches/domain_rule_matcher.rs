// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::hint::black_box;

use hotpath::criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxidns::core::rule_matcher::DomainRuleMatcher;
use oxidns::proto::Name;

fn make_domain_rules() -> Vec<String> {
    let mut rules = Vec::with_capacity(4_000);

    for idx in 0..1_000usize {
        rules.push(format!("full:edge-{idx}.bench.example"));
        rules.push(format!("domain:zone-{idx}.bench.example"));
        rules.push(format!("keyword:tenant-{idx}"));
    }

    for idx in 0..1_000usize {
        rules.push(format!(r"regexp:^svc{idx}-[a-z0-9-]+\.bench\.example$"));
    }

    rules
}

fn build_domain_matcher(rules: &[String]) -> DomainRuleMatcher {
    let mut matcher = DomainRuleMatcher::default();
    for rule in rules {
        matcher
            .add_expression(rule, "benchmark")
            .expect("benchmark domain rule should be valid");
    }
    matcher
        .finalize()
        .expect("benchmark domain matcher should finalize");
    matcher
}

fn make_suffix_rules(count: usize) -> Vec<String> {
    (0..count)
        .map(|idx| format!("domain:host-{idx}.suffix-bench.example"))
        .collect()
}

fn bench_domain_matcher(c: &mut Criterion) {
    let rules = make_domain_rules();
    let matcher = build_domain_matcher(&rules);
    let full_hit = Name::from_ascii("edge-777.bench.example.").expect("name should parse");
    let suffix_hit = Name::from_ascii("api.zone-777.bench.example.").expect("name should parse");
    let keyword_hit =
        Name::from_ascii("tenant-777-gateway.prod.example.").expect("name should parse");
    let regex_hit = Name::from_ascii("svc777-alpha.bench.example.").expect("name should parse");
    let miss = Name::from_ascii("miss.case.example.").expect("name should parse");

    let mut group = c.benchmark_group("rule_matcher_domain");

    group.bench_function(BenchmarkId::new("build", rules.len()), |b| {
        b.iter(|| {
            let matcher = build_domain_matcher(black_box(&rules));
            black_box(matcher);
        })
    });

    for (label, name, expected) in [
        ("match_full", &full_hit, true),
        ("match_suffix", &suffix_hit, true),
        ("match_keyword", &keyword_hit, true),
        ("match_regexp", &regex_hit, true),
        ("miss", &miss, false),
    ] {
        assert_eq!(
            matcher.is_match_name(name),
            expected,
            "invalid fixture: {label}"
        );
        group.bench_function(BenchmarkId::new("lookup", label), |b| {
            b.iter(|| {
                let matched = matcher.is_match_name(black_box(name));
                black_box(matched);
            })
        });
    }

    group.finish();

    let mut scaling = c.benchmark_group("rule_matcher_domain_suffix_scaling");
    scaling.sample_size(10);
    for rule_count in [1_000usize, 100_000usize] {
        let suffix_rules = make_suffix_rules(rule_count);
        scaling.bench_function(BenchmarkId::new("build", rule_count), |b| {
            b.iter(|| {
                let matcher = build_domain_matcher(black_box(&suffix_rules));
                black_box(matcher);
            })
        });

        let matcher = build_domain_matcher(&suffix_rules);
        let deep_hit = Name::from_ascii(&format!(
            "api.edge.host-{}.suffix-bench.example",
            rule_count - 1
        ))
        .expect("deep suffix hit should parse");
        let miss =
            Name::from_ascii("missing.suffix-bench.example").expect("suffix miss should parse");
        let unshared_tld_miss =
            Name::from_ascii("www.unrelated.invalid").expect("unshared TLD miss should parse");
        // Extra labels to the left of the matched suffix do not deepen the
        // reversed trie walk. Retain one hit and the two distinct miss paths.
        for (label, name, expected) in [
            ("lookup_two_level_hit", &deep_hit, true),
            ("lookup_shared_suffix_miss", &miss, false),
            ("lookup_unshared_tld_miss", &unshared_tld_miss, false),
        ] {
            assert_eq!(
                matcher.is_match_name(name),
                expected,
                "invalid fixture: {label}"
            );
            scaling.bench_function(BenchmarkId::new(label, rule_count), |b| {
                b.iter(|| black_box(matcher.is_match_name(black_box(name))))
            });
        }
    }
    scaling.finish();
}

criterion_group!(domain_rule_matcher, bench_domain_matcher);
// The library's #[hotpath::main] already installs the counting allocator.
criterion_main!(allocator = none; domain_rule_matcher);
