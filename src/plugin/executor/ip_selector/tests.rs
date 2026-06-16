// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ahash::AHashMap;
use async_trait::async_trait;
use serde_yaml_ng::Value;
use tokio::sync::Notify;

use super::config::{
    DnssecPolicy, IpSelectorCacheConfig, IpSelectorConfig, IpSelectorSettings, ProbeMethod,
    SelectionMode, parse_ip_selector_config, parse_ip_selector_quick_setup, settings_from_config,
};
use super::metrics::IpSelectorMetrics;
use super::probe::{
    ProbeKey, ProbeObservation, ProbeRunner, ProbeRuntime, cached_observation, probe_with_runtime,
};
use super::*;
use crate::core::context::DnsContext;
use crate::infra::clock::AppClock;
use crate::infra::error::Result;
use crate::plugin::executor::sequence::chain::ChainProgram;
use crate::plugin::test_utils::{create_plugin_for_test, plugin_config};
use crate::proto::rdata::{A, AAAA, CNAME, Edns};
use crate::proto::{DNSClass, Message, Name, Question, RData, Rcode, Record, RecordType};

#[derive(Debug)]
struct FakeProbeRunner {
    scores: AHashMap<ProbeKey, ProbeObservation>,
    calls: AtomicUsize,
    call_notify: Notify,
    delay: Duration,
}

#[async_trait]
impl ProbeRunner for FakeProbeRunner {
    async fn probe(
        &self,
        key: &ProbeKey,
        _timeout: Duration,
        _metrics: &IpSelectorMetrics,
    ) -> ProbeObservation {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.call_notify.notify_waiters();
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.scores
            .get(key)
            .copied()
            .unwrap_or_else(ProbeObservation::failure)
    }
}

impl FakeProbeRunner {
    fn new(scores: AHashMap<ProbeKey, ProbeObservation>) -> Self {
        Self {
            scores,
            calls: AtomicUsize::new(0),
            call_notify: Notify::new(),
            delay: Duration::ZERO,
        }
    }

    fn delayed(scores: AHashMap<ProbeKey, ProbeObservation>, delay: Duration) -> Self {
        Self {
            scores,
            calls: AtomicUsize::new(0),
            call_notify: Notify::new(),
            delay,
        }
    }
}

fn make_context(qtype: RecordType) -> DnsContext {
    let mut request = Message::new();
    request.add_question(Question::new(
        Name::from_ascii("example.com.").unwrap(),
        qtype,
        DNSClass::IN,
    ));
    DnsContext::new("127.0.0.1:5533".parse().unwrap(), request)
}

fn add_a(response: &mut Message, ip: [u8; 4]) {
    response.add_answer(Record::from_rdata(
        Name::from_ascii("example.com.").unwrap(),
        60,
        RData::A(A(Ipv4Addr::from(ip))),
    ));
}

fn add_aaaa(response: &mut Message, ip: Ipv6Addr) {
    response.add_answer(Record::from_rdata(
        Name::from_ascii("example.com.").unwrap(),
        60,
        RData::AAAA(AAAA(ip)),
    ));
}

fn set_a_response(context: &mut DnsContext, ips: &[[u8; 4]]) {
    let mut response = context.request().response(Rcode::NoError);
    for ip in ips {
        add_a(&mut response, *ip);
    }
    context.set_response(response);
}

fn answer_ips(context: &DnsContext) -> Vec<IpAddr> {
    context
        .response()
        .expect("response should exist")
        .answer_ips()
}

fn selector_with_runner(settings: IpSelectorSettings, runner: Arc<dyn ProbeRunner>) -> IpSelector {
    build_ip_selector("ip_selector_test".to_string(), settings, runner)
}

fn default_test_settings() -> IpSelectorSettings {
    settings_from_config(IpSelectorConfig {
        selection_mode: None,
        probe_methods: Some(vec!["tcp:443".to_string()]),
        probe_stagger: Some(0),
        probe_timeout: Some(50),
        max_wait: Some(200),
        top_n: Some(1),
        reorder_only: None,
        dnssec_policy: None,
        max_parallel_probes: Some(8),
        cache: Some(IpSelectorCacheConfig {
            enabled: Some(true),
            size: Some(16),
            ttl: Some(60),
            failure_ttl: Some(1),
        }),
    })
    .unwrap()
}

fn best_within_budget_settings() -> IpSelectorSettings {
    let mut settings = default_test_settings();
    settings.selection_mode = SelectionMode::BestWithinBudget;
    settings
}

fn score_key(ip: [u8; 4], latency_ms: u64) -> (ProbeKey, ProbeObservation) {
    (
        ProbeKey {
            ip: IpAddr::from(ip),
            method: ProbeMethod::Tcp(443),
        },
        ProbeObservation {
            success: true,
            latency_ms: Some(latency_ms),
            sampled_at_ms: 0,
        },
    )
}

async fn wait_for_call_count(runner: &FakeProbeRunner, expected: usize) {
    tokio::time::timeout(Duration::from_secs(1), async {
        if runner.calls.load(Ordering::SeqCst) >= expected {
            return;
        }
        loop {
            runner.call_notify.notified().await;
            if runner.calls.load(Ordering::SeqCst) >= expected {
                return;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("probe runner did not reach {expected} calls"));
}

async fn wait_for_cached_observation(runtime: &Arc<ProbeRuntime>, key: &ProbeKey) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while cached_observation(runtime, key).is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cached observation should appear");
}

#[test]
fn parse_config_accepts_native_names() {
    let args = serde_yaml_ng::from_str::<Value>(
        r#"
selection_mode: first_success
probe_methods: "tcp:443,tcp:80"
cache:
  enabled: true
"#,
    )
    .unwrap();

    let settings = parse_ip_selector_config(Some(args)).unwrap();

    assert_eq!(settings.selection_mode, SelectionMode::FirstSuccess);
    assert_eq!(
        settings.probe_methods,
        vec![ProbeMethod::Tcp(443), ProbeMethod::Tcp(80)]
    );
}

#[test]
fn parse_config_rejects_unknown_fields() {
    let args = serde_yaml_ng::from_str::<Value>("probe_methodz: tcp:443").unwrap();

    let err = parse_ip_selector_config(Some(args)).unwrap_err();

    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn parse_config_rejects_unknown_cache_fields() {
    let args = serde_yaml_ng::from_str::<Value>(
        r#"
cache:
  enabled: true
  failure_ttls: 1
"#,
    )
    .unwrap();

    let err = parse_ip_selector_config(Some(args)).unwrap_err();

    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn parse_config_rejects_unknown_selection_mode() {
    let args = serde_yaml_ng::from_str::<Value>("selection_mode: fastest").unwrap();

    let err = parse_ip_selector_config(Some(args)).unwrap_err();

    assert!(
        err.to_string()
            .contains("invalid ip_selector selection_mode")
    );
}

#[test]
fn quick_setup_parses_mode_and_methods() {
    let settings =
        parse_ip_selector_quick_setup(Some("best_within_budget tcp:443,tcp:80,ping".to_string()))
            .unwrap();

    assert_eq!(settings.selection_mode, SelectionMode::BestWithinBudget);
    assert_eq!(
        settings.probe_methods,
        vec![
            ProbeMethod::Tcp(443),
            ProbeMethod::Tcp(80),
            ProbeMethod::Ping
        ]
    );
}

#[test]
fn factory_create_accepts_default_config() {
    let factory = IpSelectorFactory;
    let cfg = plugin_config("ip_select", "ip_selector", None);

    let plugin = create_plugin_for_test(&factory, &cfg).unwrap();

    assert!(matches!(plugin, UninitializedPlugin::Executor(_)));
}

#[tokio::test]
async fn non_a_aaaa_query_passes_through() {
    AppClock::start();
    let selector = selector_with_runner(
        default_test_settings(),
        Arc::new(FakeProbeRunner::new(AHashMap::new())),
    );
    let mut ctx = make_context(RecordType::TXT);
    ctx.set_response(ctx.request().response(Rcode::NoError));

    selector.select_response_ips(&mut ctx).await;

    assert!(ctx.response().is_some());
}

#[tokio::test]
async fn best_within_budget_selects_fastest_probe_candidate() {
    AppClock::start();
    let mut scores = AHashMap::new();
    let (slow_key, slow_score) = score_key([1, 1, 1, 1], 50);
    let (fast_key, fast_score) = score_key([2, 2, 2, 2], 10);
    scores.insert(slow_key, slow_score);
    scores.insert(fast_key, fast_score);
    let selector = selector_with_runner(
        best_within_budget_settings(),
        Arc::new(FakeProbeRunner::new(scores)),
    );
    let mut ctx = make_context(RecordType::A);
    set_a_response(&mut ctx, &[[1, 1, 1, 1], [2, 2, 2, 2]]);

    selector.select_response_ips(&mut ctx).await;

    assert_eq!(answer_ips(&ctx), vec![IpAddr::from([2, 2, 2, 2])]);
}

#[tokio::test]
async fn top_n_zero_reorders_without_deleting() {
    AppClock::start();
    let mut settings = best_within_budget_settings();
    settings.top_n = 0;
    let mut scores = AHashMap::new();
    let (slow_key, slow_score) = score_key([1, 1, 1, 1], 50);
    let (fast_key, fast_score) = score_key([2, 2, 2, 2], 10);
    scores.insert(slow_key, slow_score);
    scores.insert(fast_key, fast_score);
    let selector = selector_with_runner(settings, Arc::new(FakeProbeRunner::new(scores)));
    let mut ctx = make_context(RecordType::A);
    set_a_response(&mut ctx, &[[1, 1, 1, 1], [2, 2, 2, 2]]);

    selector.select_response_ips(&mut ctx).await;

    assert_eq!(
        answer_ips(&ctx),
        vec![IpAddr::from([2, 2, 2, 2]), IpAddr::from([1, 1, 1, 1])]
    );
}

#[tokio::test]
async fn keeps_cname_and_non_address_records() {
    AppClock::start();
    let mut scores = AHashMap::new();
    let (slow_key, slow_score) = score_key([1, 1, 1, 1], 50);
    let (fast_key, fast_score) = score_key([2, 2, 2, 2], 10);
    scores.insert(slow_key, slow_score);
    scores.insert(fast_key, fast_score);
    let selector = selector_with_runner(
        best_within_budget_settings(),
        Arc::new(FakeProbeRunner::new(scores)),
    );
    let mut ctx = make_context(RecordType::A);
    let mut response = ctx.request().response(Rcode::NoError);
    response.add_answer(Record::from_rdata(
        Name::from_ascii("example.com.").unwrap(),
        60,
        RData::CNAME(CNAME(Name::from_ascii("edge.example.com.").unwrap())),
    ));
    add_a(&mut response, [1, 1, 1, 1]);
    add_a(&mut response, [2, 2, 2, 2]);
    ctx.set_response(response);

    selector.select_response_ips(&mut ctx).await;

    let response = ctx.response().unwrap();
    assert_eq!(response.answers()[0].rr_type(), RecordType::CNAME);
    assert_eq!(response.answer_ips(), vec![IpAddr::from([2, 2, 2, 2])]);
}

#[tokio::test]
async fn cache_hit_avoids_probe_and_selects_cached_score() {
    AppClock::start();
    let runner = Arc::new(FakeProbeRunner::new(AHashMap::new()));
    let selector = selector_with_runner(default_test_settings(), runner.clone());
    let now = AppClock::elapsed_millis();
    let key = ProbeKey {
        ip: IpAddr::from([2, 2, 2, 2]),
        method: ProbeMethod::Tcp(443),
    };
    selector.runtime.cache.insert_or_update(
        key,
        Arc::new(ProbeObservation {
            success: true,
            latency_ms: Some(1),
            sampled_at_ms: now,
        }),
        now,
        now + 60_000,
    );
    let mut ctx = make_context(RecordType::A);
    set_a_response(&mut ctx, &[[1, 1, 1, 1], [2, 2, 2, 2]]);

    selector.select_response_ips(&mut ctx).await;

    assert_eq!(answer_ips(&ctx), vec![IpAddr::from([2, 2, 2, 2])]);
    assert_eq!(runner.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failure_without_scores_keeps_original_response() {
    AppClock::start();
    let selector = selector_with_runner(
        default_test_settings(),
        Arc::new(FakeProbeRunner::new(AHashMap::new())),
    );
    let mut ctx = make_context(RecordType::A);
    set_a_response(&mut ctx, &[[1, 1, 1, 1], [2, 2, 2, 2]]);

    selector.select_response_ips(&mut ctx).await;

    assert_eq!(
        answer_ips(&ctx),
        vec![IpAddr::from([1, 1, 1, 1]), IpAddr::from([2, 2, 2, 2])]
    );
}

#[tokio::test]
async fn dnssec_reorder_only_prevents_deletion() {
    AppClock::start();
    let mut scores = AHashMap::new();
    let (slow_key, slow_score) = score_key([1, 1, 1, 1], 50);
    let (fast_key, fast_score) = score_key([2, 2, 2, 2], 10);
    scores.insert(slow_key, slow_score);
    scores.insert(fast_key, fast_score);
    let selector = selector_with_runner(
        best_within_budget_settings(),
        Arc::new(FakeProbeRunner::new(scores)),
    );
    let mut ctx = make_context(RecordType::A);
    let mut request = ctx.request().clone();
    let mut edns = Edns::new();
    edns.set_dnssec_ok(true);
    request.set_edns(edns);
    ctx.replace_request(request);
    let mut response = ctx.request().response(Rcode::NoError);
    add_a(&mut response, [1, 1, 1, 1]);
    add_a(&mut response, [2, 2, 2, 2]);
    ctx.set_response(response);

    selector.select_response_ips(&mut ctx).await;

    assert_eq!(
        answer_ips(&ctx),
        vec![IpAddr::from([2, 2, 2, 2]), IpAddr::from([1, 1, 1, 1])]
    );
}

#[tokio::test]
async fn dnssec_skip_keeps_original() {
    AppClock::start();
    let mut settings = default_test_settings();
    settings.dnssec_policy = DnssecPolicy::Skip;
    let selector = selector_with_runner(settings, Arc::new(FakeProbeRunner::new(AHashMap::new())));
    let mut ctx = make_context(RecordType::A);
    let mut request = ctx.request().clone();
    let mut edns = Edns::new();
    edns.set_dnssec_ok(true);
    request.set_edns(edns);
    ctx.replace_request(request);
    set_a_response(&mut ctx, &[[1, 1, 1, 1], [2, 2, 2, 2]]);

    selector.select_response_ips(&mut ctx).await;

    assert_eq!(
        answer_ips(&ctx),
        vec![IpAddr::from([1, 1, 1, 1]), IpAddr::from([2, 2, 2, 2])]
    );
}

#[tokio::test]
async fn background_mode_returns_original_and_warms_cache() {
    AppClock::start();
    let mut settings = default_test_settings();
    settings.selection_mode = SelectionMode::Background;
    let mut scores = AHashMap::new();
    let (fast_key, fast_score) = score_key([2, 2, 2, 2], 10);
    scores.insert(fast_key.clone(), fast_score);
    let selector = selector_with_runner(settings, Arc::new(FakeProbeRunner::new(scores)));
    let mut ctx = make_context(RecordType::A);
    set_a_response(&mut ctx, &[[1, 1, 1, 1], [2, 2, 2, 2]]);

    selector.select_response_ips(&mut ctx).await;

    assert_eq!(
        answer_ips(&ctx),
        vec![IpAddr::from([1, 1, 1, 1]), IpAddr::from([2, 2, 2, 2])]
    );
    wait_for_cached_observation(&selector.runtime, &fast_key).await;
    assert!(cached_observation(&selector.runtime, &fast_key).is_some());
}

#[tokio::test]
async fn inflight_probes_are_coalesced() {
    AppClock::start();
    let settings = default_test_settings();
    let key = ProbeKey {
        ip: IpAddr::from([2, 2, 2, 2]),
        method: ProbeMethod::Tcp(443),
    };
    let mut scores = AHashMap::new();
    scores.insert(
        key.clone(),
        ProbeObservation {
            success: true,
            latency_ms: Some(5),
            sampled_at_ms: 0,
        },
    );
    let runner = Arc::new(FakeProbeRunner::delayed(scores, Duration::from_millis(20)));
    let selector = selector_with_runner(settings, runner.clone());

    let first = probe_with_runtime(
        selector.runtime.clone(),
        key.clone(),
        Duration::from_millis(100),
    );
    let second = probe_with_runtime(selector.runtime.clone(), key, Duration::from_millis(100));
    let _ = tokio::join!(first, second);

    assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancelled_owner_probe_removes_inflight_entry() {
    AppClock::start();
    let mut settings = default_test_settings();
    settings.cache_enabled = false;
    let key = ProbeKey {
        ip: IpAddr::from([2, 2, 2, 2]),
        method: ProbeMethod::Tcp(443),
    };
    let mut scores = AHashMap::new();
    scores.insert(
        key.clone(),
        ProbeObservation {
            success: true,
            latency_ms: Some(5),
            sampled_at_ms: 0,
        },
    );
    let runner = Arc::new(FakeProbeRunner::delayed(scores, Duration::from_millis(100)));
    let selector = selector_with_runner(settings, runner.clone());

    let first = tokio::spawn(probe_with_runtime(
        selector.runtime.clone(),
        key.clone(),
        Duration::from_millis(500),
    ));
    wait_for_call_count(runner.as_ref(), 1).await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());

    let _ = probe_with_runtime(
        selector.runtime.clone(),
        key.clone(),
        Duration::from_millis(500),
    )
    .await;
    let calls_after_second_probe = runner.calls.load(Ordering::SeqCst);

    let _ = probe_with_runtime(selector.runtime.clone(), key, Duration::from_millis(500)).await;

    assert_eq!(
        runner.calls.load(Ordering::SeqCst),
        calls_after_second_probe + 1
    );
}

#[tokio::test]
async fn handles_aaaa_records() {
    AppClock::start();
    let mut settings = best_within_budget_settings();
    settings.top_n = 0;
    let fast_ip = Ipv6Addr::LOCALHOST;
    let slow_ip = Ipv6Addr::UNSPECIFIED;
    let mut scores = AHashMap::new();
    scores.insert(
        ProbeKey {
            ip: IpAddr::V6(slow_ip),
            method: ProbeMethod::Tcp(443),
        },
        ProbeObservation {
            success: true,
            latency_ms: Some(50),
            sampled_at_ms: 0,
        },
    );
    scores.insert(
        ProbeKey {
            ip: IpAddr::V6(fast_ip),
            method: ProbeMethod::Tcp(443),
        },
        ProbeObservation {
            success: true,
            latency_ms: Some(10),
            sampled_at_ms: 0,
        },
    );
    let selector = selector_with_runner(settings, Arc::new(FakeProbeRunner::new(scores)));
    let mut ctx = make_context(RecordType::AAAA);
    let mut response = ctx.request().response(Rcode::NoError);
    add_aaaa(&mut response, slow_ip);
    add_aaaa(&mut response, fast_ip);
    ctx.set_response(response);

    selector.select_response_ips(&mut ctx).await;

    assert_eq!(
        answer_ips(&ctx),
        vec![IpAddr::V6(fast_ip), IpAddr::V6(slow_ip)]
    );
}

#[derive(Debug)]
struct SetResponseExecutor;

#[async_trait]
impl Plugin for SetResponseExecutor {
    fn tag(&self) -> &str {
        "set_response"
    }
}

#[async_trait]
impl Executor for SetResponseExecutor {
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        set_a_response(context, &[[1, 1, 1, 1], [2, 2, 2, 2]]);
        Ok(ExecStep::Next)
    }
}

#[tokio::test]
async fn with_next_processes_downstream_response() {
    AppClock::start();
    let mut scores = AHashMap::new();
    let (fast_key, fast_score) = score_key([2, 2, 2, 2], 10);
    scores.insert(fast_key, fast_score);
    let selector = selector_with_runner(
        default_test_settings(),
        Arc::new(FakeProbeRunner::new(scores)),
    );
    let program = ChainProgram::single_with_next_executor_for_test(Arc::new(SetResponseExecutor));
    let next = ExecutorNext::from_program_for_test(program, 0);
    let mut ctx = make_context(RecordType::A);

    selector
        .execute_with_next(&mut ctx, Some(next))
        .await
        .unwrap();

    assert_eq!(answer_ips(&ctx), vec![IpAddr::from([2, 2, 2, 2])]);
}
