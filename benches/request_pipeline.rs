// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deterministic request paths. Network load/concurrency lives in benchmarks/.

use std::hint::black_box;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use hotpath::criterion::{Criterion, criterion_group, criterion_main};
use oxidns::config::types::Config;
use oxidns::core::context::DnsContext;
use oxidns::plugin::executor::Executor;
use oxidns::plugin::server::{RequestHandle, RequestMeta};
use oxidns::plugin::{PluginRegistry, init as init_plugins};
use oxidns::proto::{DNSClass, Message, Name, Question, Rcode, RecordType};
use tokio::runtime::Runtime;

const SOURCE: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 5300);

fn query(name: &str, ecs: bool) -> Vec<u8> {
    let mut query = Message::new();
    query.add_question(Question::new(
        Name::from_ascii(name).expect("fixture name should parse"),
        RecordType::A,
        DNSClass::IN,
    ));
    if ecs {
        use oxidns::proto::{ClientSubnet, EdnsOption};
        let edns = query.ensure_edns_mut();
        edns.set_dnssec_ok(true);
        edns.insert(EdnsOption::Subnet(ClientSubnet::new(
            "192.0.2.0".parse().unwrap(),
            24,
            0,
        )));
    }
    query.to_bytes().expect("query should encode")
}

fn context(wire: &[u8]) -> DnsContext {
    DnsContext::new(
        SOURCE,
        Message::from_bytes(wire).expect("query should decode"),
    )
}

/// Startup and teardown belong outside the measured per-iteration routine.
struct Fixture<'a> {
    rt: &'a Runtime,
    registry: Arc<PluginRegistry>,
    executor: Arc<dyn Executor>,
}

impl<'a> Fixture<'a> {
    fn new(rt: &'a Runtime, yaml: &str) -> Self {
        let config: Config = serde_yaml_ng::from_str(yaml).expect("config should parse");
        let registry = rt
            .block_on(init_plugins(config))
            .expect("plugins should initialize");
        let executor = registry
            .get_plugin("bench")
            .expect("sequence should exist")
            .to_executor();
        Self {
            rt,
            registry,
            executor,
        }
    }

    fn execute(&self, wire: &[u8]) -> DnsContext {
        let mut ctx = context(wire);
        black_box(
            self.rt
                .block_on(self.executor.execute(&mut ctx))
                .expect("pipeline should execute"),
        );
        if let Some(response) = ctx.response() {
            black_box(response.to_bytes().expect("response should encode"));
        }
        ctx
    }
}

impl Drop for Fixture<'_> {
    fn drop(&mut self) {
        self.rt.block_on(self.registry.destroy());
    }
}

fn config(cache: bool, match_name: &str, response: &str) -> String {
    let cache_step = if cache { "      - exec: $cache\n" } else { "" };
    format!(
        r#"
plugins:
  - tag: cache
    type: cache
    args:
      size: 128
      short_circuit: true
      cache_negative: true
      negative_ttl_without_soa: 0
      max_negative_ttl: 86400
      ecs_in_key: true
  - tag: bench
    type: sequence
    args:
{cache_step}      - exec: mark 1
      - matches: qname full:{match_name}
        exec: {response}
      - exec: ttl 86400
"#
    )
}

fn bench_request_pipeline(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut group = c.benchmark_group("request_pipeline");
    for (label, cache, match_name, response, miss, ecs) in [
        (
            "local_answer",
            false,
            "bench.test",
            "black_hole 192.0.2.10",
            false,
            false,
        ),
        (
            "positive_cache_hit",
            true,
            "bench.test",
            "black_hole 192.0.2.10",
            false,
            false,
        ),
        // black_hole supplies SOA; reject NXDOMAIN would exercise fallback TTL.
        (
            "negative_cache_hit_soa",
            true,
            "bench.test",
            "black_hole nxdomain",
            false,
            false,
        ),
        (
            "positive_cache_hit_ecs",
            true,
            "bench.test",
            "black_hole 192.0.2.10",
            false,
            true,
        ),
        (
            "positive_cache_hit_multi_rr",
            true,
            "bench.test",
            "black_hole 192.0.2.10 192.0.2.11 192.0.2.12 192.0.2.13 192.0.2.14 192.0.2.15 192.0.2.16 192.0.2.17",
            false,
            false,
        ),
        (
            "cache_miss_no_response",
            true,
            "other.test",
            "black_hole 192.0.2.10",
            true,
            false,
        ),
        (
            "matcher_miss",
            false,
            "other.test",
            "black_hole 192.0.2.10",
            true,
            false,
        ),
    ] {
        let fixture = Fixture::new(&rt, &config(cache, match_name, response));
        let wire = query("bench.test.", ecs);
        let warm = fixture.execute(&wire);
        assert_eq!(warm.response().is_none(), miss);
        if label == "negative_cache_hit_soa" {
            let response = warm.response().unwrap();
            assert_eq!(response.rcode(), Rcode::NXDomain);
            assert!(
                response
                    .authorities()
                    .iter()
                    .any(|rr| rr.rr_type() == RecordType::SOA)
            );
        }
        let probe = fixture.execute(&wire);
        assert_eq!(probe.response().is_none(), miss);
        assert_eq!(probe.marks().contains(&1), !(cache && !miss));
        if label == "positive_cache_hit_multi_rr" {
            assert_eq!(probe.response().unwrap().answers().len(), 8);
        }
        group.bench_function(label, |b| {
            b.iter(|| {
                black_box(fixture.execute(black_box(&wire)));
            })
        });
    }

    // A fresh bounded cache per iteration guarantees admission, not a hit after
    // the first iteration or time-dependent eviction from a growing key stream.
    let yaml = config(true, "bench.test", "black_hole 192.0.2.10");
    let wire = query("bench.test.", false);
    {
        let fixture = Fixture::new(&rt, &yaml);
        let first = fixture.execute(&wire);
        assert!(first.marks().contains(&1));
        assert!(first.response().is_some());
        let second = fixture.execute(&wire);
        assert!(
            !second.marks().contains(&1),
            "admission must produce a cache hit"
        );
    }
    group.bench_function("cache_miss_insert", |b| {
        b.iter_batched_ref(
            || Fixture::new(&rt, &yaml),
            |fixture| {
                black_box(fixture.execute(black_box(&wire)));
            },
            hotpath::criterion::__criterion::BatchSize::PerIteration,
        );
    });
    group.finish();

    let fixture = Fixture::new(&rt, &config(false, "bench.test", "black_hole 192.0.2.10"));
    let handler = RequestHandle::new(fixture.executor.clone());
    let mut group = c.benchmark_group("request_handle");
    for (label, ecs) in [("local_answer", false), ("local_answer_edns", true)] {
        let wire = query("bench.test.", ecs);
        let run = || {
            let request = Message::from_bytes(black_box(&wire)).unwrap();
            let result =
                rt.block_on(handler.handle_request(request, SOURCE, RequestMeta::default()));
            black_box(result.response.to_bytes().unwrap());
            result
        };
        let probe = run();
        assert_eq!(probe.response.answers().len(), 1);
        assert!(probe.response.recursion_available());
        assert_eq!(probe.response.edns().is_some(), ecs);
        group.bench_function(label, |b| {
            b.iter(|| {
                black_box(run());
            })
        });
    }
    group.finish();

    let fixture = Fixture::new(
        &rt,
        r#"
plugins:
  - tag: domains
    type: domain_set
    args:
      exps: [domain:bench.test]
  - tag: addresses
    type: ip_set
    args:
      ips: [192.0.2.0/24]
  - tag: bench
    type: sequence
    args:
      - matches: qname $domains
        exec: black_hole 192.0.2.10
      - matches: resp_ip $addresses
        exec: ttl 60
"#,
    );
    let wire = query("api.bench.test.", false);
    let probe = fixture.execute(&wire);
    assert_eq!(probe.response().unwrap().answers()[0].ttl(), 60);
    c.bench_function("request_pipeline/provider_chain", |b| {
        b.iter(|| {
            black_box(fixture.execute(black_box(&wire)));
        });
    });
}

criterion_group!(request_pipeline, bench_request_pipeline);
// The library's #[hotpath::main] already installs the counting allocator.
criterion_main!(allocator = none; request_pipeline);
