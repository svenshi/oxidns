// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use hotpath::criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxidns::core::rule_matcher::IpPrefixMatcher;

fn make_ip_rules() -> Vec<String> {
    let mut rules = Vec::with_capacity(3_000);

    for idx in 0..1_500u16 {
        // Leave a gap between prefixes so compilation cannot merge the corpus
        // into one interval. Each populated page exercises the bitmap path.
        let third = ((idx % 128) * 2) as u8;
        let second = (idx / 128) as u8;
        rules.push(format!("10.{second}.{third}.0/24"));
    }

    for idx in 0..1_500u16 {
        rules.push(format!("2001:db8:{:x}::/48", idx * 2));
    }

    rules
}

fn build_ip_matcher(rules: &[String]) -> IpPrefixMatcher {
    let mut matcher = IpPrefixMatcher::default();
    for rule in rules {
        matcher
            .add_rule(rule)
            .expect("benchmark ip rule should be valid");
    }
    matcher.finalize();
    matcher
}

fn bench_ip_matcher(c: &mut Criterion) {
    let rules = make_ip_rules();
    let matcher = build_ip_matcher(&rules);
    let ipv4_hit = IpAddr::V4(Ipv4Addr::new(10, 3, 8, 42));
    // Miss inside a populated page/range envelope, rather than an empty page.
    let ipv4_miss = IpAddr::V4(Ipv4Addr::new(10, 3, 9, 42));
    let ipv6_hit = IpAddr::V6(Ipv6Addr::new(0x2001, 0xDB8, 0x04D2, 0, 0, 0, 0, 1));
    let ipv6_miss = IpAddr::V6(Ipv6Addr::new(0x2001, 0xDB8, 0x04D3, 0, 0, 0, 0, 1));

    let mut group = c.benchmark_group("rule_matcher_ip");

    group.bench_function(BenchmarkId::new("build", rules.len()), |b| {
        b.iter(|| {
            let matcher = build_ip_matcher(black_box(&rules));
            black_box(matcher);
        })
    });

    for (label, ip, expected) in [
        ("bitmap_hit_ipv4", ipv4_hit, true),
        ("bitmap_miss_ipv4", ipv4_miss, false),
        ("fragmented_hit_ipv6", ipv6_hit, true),
        ("fragmented_miss_ipv6", ipv6_miss, false),
    ] {
        assert_eq!(
            matcher.contains_ip(ip),
            expected,
            "invalid fixture: {label}"
        );
        group.bench_function(BenchmarkId::new("lookup", label), |b| {
            b.iter(|| {
                let matched = matcher.contains_ip(black_box(ip));
                black_box(matched);
            })
        });
    }

    group.finish();

    // Keep the distinct small-page and tiny-IPv6 fast paths as diagnostics.
    let compact = build_ip_matcher(&["192.0.2.0/24".into(), "2001:db8::/48".into()]);
    let mut group = c.benchmark_group("rule_matcher_ip_compact");
    for (label, ip) in [
        ("inline_ipv4", IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1))),
        (
            "linear_ipv6",
            IpAddr::V6(Ipv6Addr::new(0x2001, 0xDB8, 0, 0, 0, 0, 0, 1)),
        ),
    ] {
        assert!(compact.contains_ip(ip));
        group.bench_function(label, |b| {
            b.iter(|| black_box(compact.contains_ip(black_box(ip))));
        });
    }
    group.finish();
}

criterion_group!(ip_rule_matcher, bench_ip_matcher);
// The library's #[hotpath::main] already installs the counting allocator.
criterion_main!(allocator = none; ip_rule_matcher);
