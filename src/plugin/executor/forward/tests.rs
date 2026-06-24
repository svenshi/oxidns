// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use async_trait::async_trait;

use super::concurrent::ConcurrentForwarder;
use super::config::{
    MAX_CONCURRENT_QUERIES, parse_forward_config, parse_quick_setup_param,
    resolve_active_concurrent, validate_upstream_addr,
};
use super::factory::ForwardFactory;
use super::metrics::ForwardMetrics;
use super::selection::ResponseSelectionMode;
use super::single::SingleDnsForwarder;
use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::upstream::{ConnectionInfo, QueryDeadline, Upstream};
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::{PluginFactory, UninitializedPlugin};
use crate::proto::{A, Message, Name, Question, RData, Rcode, Record, RecordType};

#[derive(Debug)]
struct MockUpstream {
    connection_info: ConnectionInfo,
    response_code: Option<Rcode>,
    answer: bool,
    fail_message: Option<String>,
    delay: Duration,
}

impl MockUpstream {
    fn ok() -> Self {
        Self::response(Rcode::NoError, Duration::ZERO)
    }

    fn ok_with_answer(delay: Duration) -> Self {
        let mut upstream = Self::response(Rcode::NoError, delay);
        upstream.answer = true;
        upstream
    }

    fn response(response_code: Rcode, delay: Duration) -> Self {
        Self {
            connection_info: ConnectionInfo::with_addr("1.1.1.1")
                .expect("mock upstream addr must be valid"),
            response_code: Some(response_code),
            answer: false,
            fail_message: None,
            delay,
        }
    }

    fn fail(msg: &str, delay: Duration) -> Self {
        Self {
            connection_info: ConnectionInfo::with_addr("1.1.1.1")
                .expect("mock upstream addr must be valid"),
            response_code: None,
            answer: false,
            fail_message: Some(msg.to_string()),
            delay,
        }
    }
}

#[async_trait]
impl Upstream for MockUpstream {
    async fn inner_query(&self, request: Message, _deadline: QueryDeadline) -> Result<Message> {
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        if let Some(err) = self.fail_message.as_ref() {
            return Err(DnsError::plugin(err.clone()));
        }
        let response_code = self.response_code.unwrap_or(Rcode::NoError);
        let mut response = request.response(response_code);
        if self.answer {
            response.add_answer(Record::from_rdata(
                Name::from_ascii("example.com.").unwrap(),
                60,
                RData::A(A("192.0.2.1".parse().unwrap())),
            ));
        }
        Ok(response)
    }

    fn connection_info(&self) -> &ConnectionInfo {
        &self.connection_info
    }
}

fn make_context() -> DnsContext {
    AppClock::start();
    let mut request = Message::new();
    request.add_question(Question::new(
        Name::from_ascii("example.com.").unwrap(),
        RecordType::A,
        crate::proto::DNSClass::IN,
    ));
    DnsContext::new("127.0.0.1:5533".parse().unwrap(), request)
}

fn make_plugin_config(args: &str) -> PluginConfig {
    PluginConfig {
        tag: "forward-test".to_string(),
        plugin_type: "forward".to_string(),
        args: Some(serde_yaml_ng::from_str(args).unwrap()),
    }
}

fn test_metrics() -> Arc<ForwardMetrics> {
    Arc::new(ForwardMetrics::new(
        "forward-test".to_string(),
        vec!["u0".to_string(), "u1".to_string()],
    ))
}

#[tokio::test]
async fn concurrent_returns_error_when_all_upstreams_fail() {
    let metrics = test_metrics();
    let forwarder = ConcurrentForwarder {
        tag: "forward-test".to_string(),
        active_concurrent: 2,
        upstreams: vec![
            Arc::new(MockUpstream::fail("u1 fail", Duration::ZERO)),
            Arc::new(MockUpstream::fail("u2 fail", Duration::ZERO)),
        ],
        short_circuit: false,
        response_selection: ResponseSelectionMode::default(),
        metrics: metrics.clone(),
    };

    let mut context = make_context();
    let err = forwarder.execute(&mut context).await.unwrap_err();

    assert!(
        err.to_string()
            .contains("failed across all concurrent upstreams")
    );
    assert!(context.response().is_none());
    assert_eq!(metrics.query_total.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.error_total.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.latency_count.load(Ordering::Relaxed), 1);
}

#[test]
fn validate_rejects_empty_upstreams() {
    let factory = ForwardFactory;
    let cfg = make_plugin_config("upstreams: []");
    let err = match crate::plugin::test_utils::create_plugin_for_test(&factory, &cfg) {
        Ok(_) => panic!("expected create to fail for empty upstreams"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("at least one upstream"));
}

#[test]
fn validate_rejects_invalid_upstream_addr() {
    let factory = ForwardFactory;
    let cfg = make_plugin_config(
        r#"
upstreams:
  - addr: "udp://"
"#,
    );
    let err = match crate::plugin::test_utils::create_plugin_for_test(&factory, &cfg) {
        Ok(_) => panic!("expected create to fail for invalid upstream addr"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("is invalid"));
}

#[test]
fn validate_accepts_domain_upstream_addr_without_resolution() {
    validate_upstream_addr("tls://dns.example.invalid:853")
        .expect("domain upstream validation should only parse address syntax");
}

#[test]
fn quick_setup_rejects_invalid_upstream_addr() {
    let factory = ForwardFactory;
    let result = factory.quick_setup("forward-test", Some("udp://".to_string()));
    let err = match result {
        Ok(_) => panic!("expected quick_setup to fail for invalid upstream addr"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("is invalid"));
}

#[test]
fn parse_forward_config_accepts_short_circuit() {
    let cfg = parse_forward_config(&make_plugin_config(
        r#"
short_circuit: true
upstreams:
  - addr: "udp://1.1.1.1:53"
"#,
    ))
    .expect("forward config should parse");

    assert!(cfg.short_circuit);
}

#[test]
fn parse_forward_config_defaults_response_selection_to_balanced() {
    let cfg = parse_forward_config(&make_plugin_config(
        r#"
upstreams:
  - addr: "udp://1.1.1.1:53"
"#,
    ))
    .expect("forward config should parse");

    assert_eq!(cfg.response_selection, ResponseSelectionMode::Balanced);
}

#[test]
fn parse_forward_config_accepts_response_selection() {
    let cfg = parse_forward_config(&make_plugin_config(
        r#"
response_selection: prefer_positive
upstreams:
  - addr: "udp://1.1.1.1:53"
"#,
    ))
    .expect("forward config should parse");

    assert_eq!(
        cfg.response_selection,
        ResponseSelectionMode::PreferPositive
    );
}

#[test]
fn quick_setup_supports_short_circuit_flag() {
    let (upstreams, short_circuit) =
        parse_quick_setup_param(Some("1.1.1.1 8.8.8.8 short_circuit=true".to_string()))
            .expect("quick setup should parse");

    assert_eq!(
        upstreams,
        vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()]
    );
    assert!(short_circuit);
}

#[tokio::test]
async fn quick_setup_accepts_multiple_upstreams() {
    let factory = ForwardFactory;
    let result = factory.quick_setup("forward-test", Some("1.1.1.1 8.8.8.8".to_string()));
    match result {
        Ok(UninitializedPlugin::Executor(_)) => {}
        Ok(_) => panic!("expected quick setup forward to return an executor plugin"),
        Err(err) => panic!("expected quick setup with multi upstreams to succeed, got {err}"),
    }
}

#[test]
fn active_concurrent_defaults_to_one() {
    assert_eq!(resolve_active_concurrent(None, 8), 1);
}

#[test]
fn active_concurrent_caps_at_upstream_count() {
    assert_eq!(resolve_active_concurrent(Some(10), 4), 4);
}

#[test]
fn active_concurrent_caps_at_maximum() {
    assert_eq!(
        resolve_active_concurrent(Some(100), 64),
        MAX_CONCURRENT_QUERIES
    );
}

#[tokio::test]
async fn concurrent_success_sets_response() {
    let forwarder = ConcurrentForwarder {
        tag: "forward-test".to_string(),
        active_concurrent: 1,
        upstreams: vec![Arc::new(MockUpstream::ok())],
        short_circuit: false,
        response_selection: ResponseSelectionMode::default(),
        metrics: test_metrics(),
    };

    let mut context = make_context();
    let step = forwarder.execute(&mut context).await.unwrap();
    assert!(matches!(step, ExecStep::Next));
    assert!(context.response().is_some());
}

#[tokio::test]
async fn single_success_stops_when_short_circuit_enabled() {
    let metrics = test_metrics();
    let forwarder = SingleDnsForwarder {
        tag: "forward-test".to_string(),
        upstream: Box::new(MockUpstream::ok()),
        short_circuit: true,
        metrics: metrics.clone(),
    };

    let mut context = make_context();
    let step = forwarder.execute(&mut context).await.unwrap();
    assert!(matches!(step, ExecStep::Stop));
    assert!(context.response().is_some());
    assert_eq!(metrics.query_total.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.success_total.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.latency_count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn single_metrics_record_error_and_timeout() {
    let metrics = test_metrics();
    let forwarder = SingleDnsForwarder {
        tag: "forward-test".to_string(),
        upstream: Box::new(MockUpstream::fail(
            "DNS query timeout after 1s",
            Duration::ZERO,
        )),
        short_circuit: false,
        metrics: metrics.clone(),
    };

    let mut context = make_context();
    let err = forwarder.execute(&mut context).await.unwrap_err();

    assert!(err.to_string().contains("query failed"));
    assert_eq!(metrics.query_total.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.error_total.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.timeout_total.load(Ordering::Relaxed), 1);
    assert_eq!(metrics.latency_count.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn concurrent_success_stops_when_short_circuit_enabled() {
    let forwarder = ConcurrentForwarder {
        tag: "forward-test".to_string(),
        active_concurrent: 1,
        upstreams: vec![Arc::new(MockUpstream::ok())],
        short_circuit: true,
        response_selection: ResponseSelectionMode::default(),
        metrics: test_metrics(),
    };

    let mut context = make_context();
    let step = forwarder.execute(&mut context).await.unwrap();
    assert!(matches!(step, ExecStep::Stop));
    assert!(context.response().is_some());
}

#[tokio::test(start_paused = true)]
async fn concurrent_prefers_noerror_over_early_servfail() {
    let forwarder = ConcurrentForwarder {
        tag: "forward-test".to_string(),
        active_concurrent: 2,
        upstreams: vec![
            Arc::new(MockUpstream::response(Rcode::ServFail, Duration::ZERO)),
            Arc::new(MockUpstream::response(
                Rcode::NoError,
                Duration::from_millis(20),
            )),
        ],
        short_circuit: false,
        response_selection: ResponseSelectionMode::default(),
        metrics: test_metrics(),
    };

    let mut context = make_context();
    let step = forwarder.execute(&mut context).await.unwrap();
    assert!(matches!(step, ExecStep::Next));
    assert_eq!(
        context.response().expect("response must exist").rcode(),
        Rcode::NoError
    );
}

#[tokio::test(start_paused = true)]
async fn concurrent_returns_last_non_preferred_rcode_when_no_preferred_response() {
    let forwarder = ConcurrentForwarder {
        tag: "forward-test".to_string(),
        active_concurrent: 2,
        upstreams: vec![
            Arc::new(MockUpstream::response(Rcode::ServFail, Duration::ZERO)),
            Arc::new(MockUpstream::response(
                Rcode::Refused,
                Duration::from_millis(20),
            )),
        ],
        short_circuit: false,
        response_selection: ResponseSelectionMode::default(),
        metrics: test_metrics(),
    };

    let mut context = make_context();
    let step = forwarder.execute(&mut context).await.unwrap();
    assert!(matches!(step, ExecStep::Next));
    assert_eq!(
        context.response().expect("response must exist").rcode(),
        Rcode::Refused
    );
}

#[tokio::test(start_paused = true)]
async fn fastest_selection_returns_early_nxdomain() {
    let forwarder = ConcurrentForwarder {
        tag: "forward-test".to_string(),
        active_concurrent: 2,
        upstreams: vec![
            Arc::new(MockUpstream::response(Rcode::NXDomain, Duration::ZERO)),
            Arc::new(MockUpstream::ok_with_answer(Duration::from_millis(20))),
        ],
        short_circuit: false,
        response_selection: ResponseSelectionMode::Fastest,
        metrics: test_metrics(),
    };

    let mut context = make_context();
    let step = forwarder.execute(&mut context).await.unwrap();
    assert!(matches!(step, ExecStep::Next));
    assert_eq!(
        context.response().expect("response must exist").rcode(),
        Rcode::NXDomain
    );
}

#[tokio::test(start_paused = true)]
async fn balanced_selection_waits_briefly_for_positive_after_negative() {
    let forwarder = ConcurrentForwarder {
        tag: "forward-test".to_string(),
        active_concurrent: 2,
        upstreams: vec![
            Arc::new(MockUpstream::response(Rcode::NXDomain, Duration::ZERO)),
            Arc::new(MockUpstream::ok_with_answer(Duration::from_millis(20))),
        ],
        short_circuit: false,
        response_selection: ResponseSelectionMode::Balanced,
        metrics: test_metrics(),
    };

    let mut context = make_context();
    let step = forwarder.execute(&mut context).await.unwrap();
    let response = context.response().expect("response must exist");
    assert!(matches!(step, ExecStep::Next));
    assert_eq!(response.rcode(), Rcode::NoError);
    assert_eq!(response.answers().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn prefer_positive_waits_for_late_positive() {
    let forwarder = ConcurrentForwarder {
        tag: "forward-test".to_string(),
        active_concurrent: 2,
        upstreams: vec![
            Arc::new(MockUpstream::response(Rcode::NXDomain, Duration::ZERO)),
            Arc::new(MockUpstream::ok_with_answer(Duration::from_millis(200))),
        ],
        short_circuit: false,
        response_selection: ResponseSelectionMode::PreferPositive,
        metrics: test_metrics(),
    };

    let mut context = make_context();
    let step = forwarder.execute(&mut context).await.unwrap();
    let response = context.response().expect("response must exist");
    assert!(matches!(step, ExecStep::Next));
    assert_eq!(response.rcode(), Rcode::NoError);
    assert_eq!(response.answers().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn consensus_selection_returns_after_two_negative_votes() {
    let forwarder = ConcurrentForwarder {
        tag: "forward-test".to_string(),
        active_concurrent: 3,
        upstreams: vec![
            Arc::new(MockUpstream::response(Rcode::NXDomain, Duration::ZERO)),
            Arc::new(MockUpstream::response(
                Rcode::NXDomain,
                Duration::from_millis(20),
            )),
            Arc::new(MockUpstream::ok_with_answer(Duration::from_millis(200))),
        ],
        short_circuit: false,
        response_selection: ResponseSelectionMode::Consensus,
        metrics: Arc::new(ForwardMetrics::new(
            "forward-test".to_string(),
            vec!["u0".to_string(), "u1".to_string(), "u2".to_string()],
        )),
    };

    let mut context = make_context();
    let step = forwarder.execute(&mut context).await.unwrap();
    assert!(matches!(step, ExecStep::Next));
    assert_eq!(
        context.response().expect("response must exist").rcode(),
        Rcode::NXDomain
    );
}

#[tokio::test(start_paused = true)]
async fn consensus_selection_waits_when_negative_votes_disagree() {
    let forwarder = ConcurrentForwarder {
        tag: "forward-test".to_string(),
        active_concurrent: 3,
        upstreams: vec![
            Arc::new(MockUpstream::response(Rcode::NXDomain, Duration::ZERO)),
            Arc::new(MockUpstream::response(
                Rcode::NoError,
                Duration::from_millis(20),
            )),
            Arc::new(MockUpstream::ok_with_answer(Duration::from_millis(200))),
        ],
        short_circuit: false,
        response_selection: ResponseSelectionMode::Consensus,
        metrics: Arc::new(ForwardMetrics::new(
            "forward-test".to_string(),
            vec!["u0".to_string(), "u1".to_string(), "u2".to_string()],
        )),
    };

    let mut context = make_context();
    let step = forwarder.execute(&mut context).await.unwrap();
    let response = context.response().expect("response must exist");
    assert!(matches!(step, ExecStep::Next));
    assert_eq!(response.rcode(), Rcode::NoError);
    assert_eq!(response.answers().len(), 1);
}
