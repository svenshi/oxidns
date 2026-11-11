// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deterministic local request paths; no upstream sockets, sleeps or
//! persistence.

use std::hint::black_box;
use std::net::{Ipv4Addr, SocketAddr};

use hotpath::criterion::{Criterion, criterion_group, criterion_main};
use oxidns::config::types::Config;
use oxidns::core::context::DnsContext;
use oxidns::plugin::init as init_plugins;
use oxidns::proto::{DNSClass, Message, Name, Question, Rcode, RecordType};

fn query(name: &str) -> Vec<u8> {
    let mut query = Message::new();
    query.add_question(Question::new(
        Name::from_ascii(name).expect("fixture name should parse"),
        RecordType::A,
        DNSClass::IN,
    ));
    query.to_bytes().expect("query should encode")
}

fn context(wire: &[u8]) -> DnsContext {
    DnsContext::new(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)),
        Message::from_bytes(wire).expect("query should decode"),
    )
}

fn bench_request_pipeline(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("benchmark runtime should start");
    let mut group = c.benchmark_group("request_pipeline");
    for (label, cache, match_name, response, miss) in [
        (
            "local_answer",
            false,
            "bench.test",
            "black_hole 192.0.2.10",
            false,
        ),
        (
            "positive_cache_hit",
            true,
            "bench.test",
            "black_hole 192.0.2.10",
            false,
        ),
        (
            "negative_cache_hit",
            true,
            "bench.test",
            "reject NXDOMAIN",
            false,
        ),
        (
            "cache_miss",
            true,
            "other.test",
            "black_hole 192.0.2.10",
            true,
        ),
        (
            "matcher_miss",
            false,
            "other.test",
            "black_hole 192.0.2.10",
            true,
        ),
    ] {
        let cache_step = if cache { "      - exec: $cache\n" } else { "" };
        let yaml = format!(
            r#"
plugins:
  - tag: cache
    type: cache
    args:
      size: 128
      short_circuit: true
      cache_negative: true
      negative_ttl_without_soa: 86400
      max_negative_ttl: 86400
  - tag: bench
    type: sequence
    args:
{cache_step}      - exec: mark 1
      - matches: qname full:{match_name}
        exec: {response}
      - exec: ttl 86400
"#
        );
        let config: Config = serde_yaml_ng::from_str(&yaml).expect("config should parse");
        let registry = rt
            .block_on(init_plugins(config))
            .expect("plugins should initialize");
        let executor = registry
            .get_plugin("bench")
            .expect("sequence should exist")
            .to_executor();
        let wire = query("bench.test.");

        // Warm the exact cache key before measurement, and verify each fixture.
        let mut warm = context(&wire);
        rt.block_on(executor.execute(&mut warm))
            .expect("warmup should execute");
        assert_eq!(warm.response().is_none(), miss);
        if let Some(response) = warm.response() {
            assert_eq!(
                response.rcode(),
                if label == "negative_cache_hit" {
                    Rcode::NXDomain
                } else {
                    Rcode::NoError
                }
            );
        }
        let mut probe = context(&wire);
        rt.block_on(executor.execute(&mut probe))
            .expect("probe should execute");
        assert_eq!(probe.response().is_none(), miss);
        assert_eq!(probe.marks().contains(&1), !(cache && !miss));
        group.bench_function(label, |b| {
            b.iter(|| {
                let mut ctx = context(black_box(&wire));
                let step = rt
                    .block_on(executor.execute(&mut ctx))
                    .expect("pipeline should execute");
                black_box(step);
                if let Some(response) = ctx.response() {
                    black_box(response.to_bytes().expect("response should encode"));
                }
                black_box(ctx);
            });
        });
        rt.block_on(registry.destroy());
    }
    group.finish();
}

criterion_group!(request_pipeline, bench_request_pipeline);
// The library's #[hotpath::main] already installs the counting allocator.
criterion_main!(allocator = none; request_pipeline);
