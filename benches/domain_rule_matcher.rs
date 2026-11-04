// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
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

    for (label, name) in [
        ("match_full", &full_hit),
        ("match_suffix", &suffix_hit),
        ("match_keyword", &keyword_hit),
        ("match_regexp", &regex_hit),
        ("miss", &miss),
    ] {
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
        let direct_hit = Name::from_ascii(&format!("host-{}.suffix-bench.example", rule_count - 1))
            .expect("direct suffix hit should parse");
        let deep_hit = Name::from_ascii(&format!(
            "api.edge.host-{}.suffix-bench.example",
            rule_count - 1
        ))
        .expect("deep suffix hit should parse");
        let miss =
            Name::from_ascii("missing.suffix-bench.example").expect("suffix miss should parse");
        let deep_ten_hit = Name::from_ascii(&format!(
            "l10.l9.l8.l7.l6.l5.l4.l3.l2.l1.host-{}.suffix-bench.example",
            rule_count - 1
        ))
        .expect("ten-level suffix hit should parse");
        let unshared_tld_miss =
            Name::from_ascii("www.unrelated.invalid").expect("unshared TLD miss should parse");
        let deep_unshared_tld_miss =
            Name::from_ascii("l10.l9.l8.l7.l6.l5.l4.l3.l2.l1.www.unrelated.invalid")
                .expect("deep unshared TLD miss should parse");

        for (label, name) in [
            ("lookup_direct_hit", &direct_hit),
            ("lookup_two_level_hit", &deep_hit),
            ("lookup_shared_suffix_miss", &miss),
            ("lookup_ten_level_hit", &deep_ten_hit),
            ("lookup_unshared_tld_miss", &unshared_tld_miss),
            ("lookup_deep_unshared_tld_miss", &deep_unshared_tld_miss),
        ] {
            scaling.bench_function(BenchmarkId::new(label, rule_count), |b| {
                b.iter(|| black_box(matcher.is_match_name(black_box(name))))
            });
        }
    }
    scaling.finish();
}

criterion_group!(domain_rule_matcher, bench_domain_matcher);
criterion_main!(domain_rule_matcher);
