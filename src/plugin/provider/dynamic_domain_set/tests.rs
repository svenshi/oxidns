// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::Method;
use http_body_util::BodyExt;

use super::DynamicDomainSet;
use super::api::{RulesAddHandler, RulesClearHandler, RulesRemoveHandler};
use super::backend::{DynamicDomainSetBackend, DynamicDomainSetSnapshot};
use super::config::DynamicDomainSetConfig;
use super::rules::{DynamicDomainRuleKind, canonicalize_rule};
use super::storage::read_rule_file;
use crate::api::ApiHandler;
use crate::core::rule_matcher::DomainRuleMatcher;
use crate::infra::clock::AppClock;
use crate::plugin::provider::Provider;
use crate::proto::{DNSClass, Message, Name, Question, RecordType};

fn test_name(raw: &str) -> Name {
    Name::from_ascii(raw).expect("name should parse")
}

fn test_question(raw: &str) -> Question {
    Question::new(test_name(raw), RecordType::A, DNSClass::IN)
}

fn test_config(path: PathBuf) -> DynamicDomainSetConfig {
    DynamicDomainSetConfig {
        path,
        bootstrap_rules: Vec::new(),
        queue_size: 8,
        batch_size: 1,
        flush_interval_ms: 10,
    }
}

fn test_config_with_flush(
    path: PathBuf,
    batch_size: usize,
    flush_interval_ms: u64,
) -> DynamicDomainSetConfig {
    DynamicDomainSetConfig {
        path,
        bootstrap_rules: Vec::new(),
        queue_size: 8,
        batch_size,
        flush_interval_ms,
    }
}

#[test]
fn canonicalize_rule_normalizes_plain_full_domain() {
    let rule = canonicalize_rule(" WWW.Example.COM. ", DynamicDomainRuleKind::Full, "test")
        .expect("rule should canonicalize");
    assert_eq!(rule, "full:www.example.com");
}

#[test]
fn canonicalize_rule_rejects_invalid_regexp() {
    let err = canonicalize_rule("regexp:[bad", DynamicDomainRuleKind::Full, "test")
        .expect_err("invalid regexp should be rejected before staging");
    assert!(err.to_string().contains("invalid regexp expression"));
}

#[test]
fn read_file_ignores_empty_comments_and_deduplicates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("rules.txt");
    fs::write(
        &path,
        "\n# comment\nExample.COM\nfull:WWW.Example.COM.\nexample.com\n",
    )
    .expect("write rules");
    let rules = read_rule_file(&path).expect("rules should load");
    assert_eq!(rules, vec!["domain:example.com", "full:www.example.com"]);
}

#[tokio::test]
async fn dynamic_domain_set_appends_and_matches() {
    AppClock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("learned.txt");
    let backend = Arc::new(DynamicDomainSetBackend::new(
        "learned".to_string(),
        test_config(path.clone()),
    ));
    backend.start().await.expect("backend should start");
    backend
        .append_rules_sync(
            vec!["Example.COM.".to_string()],
            DynamicDomainRuleKind::Full,
            Duration::from_secs(2),
        )
        .await
        .expect("append should succeed");

    assert!(backend.contains_name(&test_name("example.com.")));
    assert_eq!(
        fs::read_to_string(&path).expect("file should exist"),
        "full:example.com\n"
    );

    backend.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn dynamic_domain_set_append_preserves_unterminated_tail_rule() {
    AppClock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("learned.txt");
    fs::write(&path, "full:one.example").expect("write unterminated rule");
    let backend = Arc::new(DynamicDomainSetBackend::new(
        "learned".to_string(),
        test_config(path.clone()),
    ));
    backend.start().await.expect("backend should start");

    backend
        .append_rules_sync(
            vec!["Two.Example.".to_string()],
            DynamicDomainRuleKind::Full,
            Duration::from_secs(1),
        )
        .await
        .expect("append should succeed");

    assert_eq!(
        fs::read_to_string(&path).expect("file should exist"),
        "full:one.example\nfull:two.example\n"
    );
    assert_eq!(
        read_rule_file(&path).expect("reload parser should see separate rules"),
        vec![
            "full:one.example".to_string(),
            "full:two.example".to_string()
        ]
    );
    backend.reload_sync().await.expect("reload");
    assert!(backend.contains_name(&test_name("one.example.")));
    assert!(backend.contains_name(&test_name("two.example.")));

    backend.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn dynamic_domain_set_invalid_regexp_append_does_not_poison_file() {
    AppClock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("learned.txt");
    fs::write(&path, "full:stable.example\n").expect("write initial");
    let backend = Arc::new(DynamicDomainSetBackend::new(
        "learned".to_string(),
        test_config(path.clone()),
    ));
    backend.start().await.expect("backend should start");

    backend
        .append_rules_sync(
            vec!["regexp:[bad".to_string()],
            DynamicDomainRuleKind::Full,
            Duration::from_secs(1),
        )
        .await
        .expect_err("invalid regexp should fail before file append");

    assert_eq!(
        fs::read_to_string(&path).expect("file should stay readable"),
        "full:stable.example\n"
    );
    let listed = serde_json::to_value(backend.list_rules(0, 10).expect("list rules"))
        .expect("serialize list");
    assert_eq!(listed["rules"], serde_json::json!(["full:stable.example"]));
    assert!(backend.contains_name(&test_name("stable.example.")));
    backend.reload_sync().await.expect("reload remains healthy");

    backend.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn dynamic_domain_set_async_append_is_ordered_before_clear() {
    AppClock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("learned.txt");
    let backend = Arc::new(DynamicDomainSetBackend::new(
        "learned".to_string(),
        test_config_with_flush(path.clone(), 64, 60_000),
    ));
    backend.start().await.expect("backend should start");

    backend
        .append_rules_async(
            vec!["Queued.Example.".to_string()],
            DynamicDomainRuleKind::Full,
        )
        .expect("async append should enqueue");
    backend.clear_sync().await.expect("clear");

    let listed = serde_json::to_value(backend.list_rules(0, 10).expect("list rules"))
        .expect("serialize list");
    assert_eq!(listed["rules"], serde_json::json!([]));
    assert_eq!(fs::read_to_string(&path).expect("file should exist"), "");
    assert!(!backend.contains_name(&test_name("queued.example.")));

    backend.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn dynamic_domain_set_sync_append_flushes_without_batch_or_tick() {
    AppClock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("learned.txt");
    let backend = Arc::new(DynamicDomainSetBackend::new(
        "learned".to_string(),
        test_config_with_flush(path.clone(), 64, 60_000),
    ));
    backend.start().await.expect("backend should start");

    // Give the worker time to consume Tokio interval's immediate first tick.
    // Without an explicit flush for waited appends, this request would then sit
    // in the pending batch until the 60s interval and exceed the caller timeout.
    tokio::time::sleep(Duration::from_millis(25)).await;
    backend
        .append_rules_sync(
            vec!["Sync.Example.".to_string()],
            DynamicDomainRuleKind::Full,
            Duration::from_millis(250),
        )
        .await
        .expect("sync append should flush immediately");

    assert!(backend.contains_name(&test_name("sync.example.")));
    assert_eq!(
        fs::read_to_string(&path).expect("file should exist"),
        "full:sync.example\n"
    );

    backend.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn dynamic_domain_set_remove_clear_and_reload() {
    AppClock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("learned.txt");
    fs::write(&path, "full:one.example\n").expect("write initial");
    let backend = Arc::new(DynamicDomainSetBackend::new(
        "learned".to_string(),
        test_config(path.clone()),
    ));
    backend.start().await.expect("backend should start");
    assert!(backend.contains_name(&test_name("one.example.")));

    backend
        .remove_rules_sync(
            vec!["full:one.example".to_string()],
            DynamicDomainRuleKind::Full,
        )
        .await
        .expect("remove");
    assert!(!backend.contains_name(&test_name("one.example.")));

    fs::write(&path, "full:two.example\n").expect("external edit");
    backend.reload_sync().await.expect("reload");
    assert!(backend.contains_name(&test_name("two.example.")));

    backend.clear_sync().await.expect("clear");
    assert!(!backend.contains_name(&test_name("two.example.")));
    assert_eq!(fs::read_to_string(&path).expect("file"), "");
    backend.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn dynamic_domain_set_remove_failure_keeps_state_and_snapshot() {
    AppClock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("learned.txt");
    fs::write(&path, "full:one.example\nfull:two.example\n").expect("write initial");
    let backend = Arc::new(DynamicDomainSetBackend::new(
        "learned".to_string(),
        test_config(path.clone()),
    ));
    backend.start().await.expect("backend should start");

    fs::remove_file(&path).expect("remove file");
    fs::create_dir(&path).expect("replace rule file with directory");
    let _err = backend
        .remove_rules_sync(
            vec!["full:one.example".to_string()],
            DynamicDomainRuleKind::Full,
        )
        .await
        .expect_err("remove should fail when rewrite cannot replace directory");

    let listed = serde_json::to_value(backend.list_rules(0, 10).expect("list rules"))
        .expect("serialize list");
    assert_eq!(
        listed["rules"],
        serde_json::json!(["full:one.example", "full:two.example"])
    );
    assert!(backend.contains_name(&test_name("one.example.")));
    assert!(backend.contains_name(&test_name("two.example.")));

    backend.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn dynamic_domain_set_clear_failure_keeps_state_and_snapshot() {
    AppClock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("learned.txt");
    fs::write(&path, "full:one.example\n").expect("write initial");
    let backend = Arc::new(DynamicDomainSetBackend::new(
        "learned".to_string(),
        test_config(path.clone()),
    ));
    backend.start().await.expect("backend should start");

    fs::remove_file(&path).expect("remove file");
    fs::create_dir(&path).expect("replace rule file with directory");
    let _err = backend
        .clear_sync()
        .await
        .expect_err("clear should fail when rewrite cannot replace directory");

    let listed = serde_json::to_value(backend.list_rules(0, 10).expect("list rules"))
        .expect("serialize list");
    assert_eq!(listed["rules"], serde_json::json!(["full:one.example"]));
    assert!(backend.contains_name(&test_name("one.example.")));

    backend.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn dynamic_domain_set_rule_api_adds_removes_and_clears() {
    AppClock::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("learned.txt");
    let backend = Arc::new(DynamicDomainSetBackend::new(
        "learned".to_string(),
        test_config(path.clone()),
    ));
    backend.start().await.expect("backend should start");

    let add = RulesAddHandler {
        backend: backend.clone(),
    };
    let response = add
        .handle(
            http::Request::builder()
                .method(Method::POST)
                .uri("/rules")
                .body(Bytes::from_static(
                    br#"{"rules":["Api.Example."],"rule_kind":"full"}"#,
                ))
                .expect("request"),
        )
        .await;
    assert_eq!(response.status(), http::StatusCode::OK);
    let _ = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    assert!(backend.contains_name(&test_name("api.example.")));

    let remove = RulesRemoveHandler {
        backend: backend.clone(),
    };
    let response = remove
        .handle(
            http::Request::builder()
                .method(Method::DELETE)
                .uri("/rules")
                .body(Bytes::from_static(br#"{"rules":["full:api.example"]}"#))
                .expect("request"),
        )
        .await;
    assert_eq!(response.status(), http::StatusCode::OK);
    assert!(!backend.contains_name(&test_name("api.example.")));

    backend
        .append_rules_sync(
            vec!["clear.example".to_string()],
            DynamicDomainRuleKind::Full,
            Duration::from_secs(2),
        )
        .await
        .expect("append before clear");
    let clear = RulesClearHandler {
        backend: backend.clone(),
    };
    let response = clear
        .handle(
            http::Request::builder()
                .method(Method::POST)
                .uri("/rules/clear")
                .body(Bytes::new())
                .expect("request"),
        )
        .await;
    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(fs::read_to_string(&path).expect("file"), "");
    assert!(!backend.contains_name(&test_name("clear.example.")));

    backend.shutdown().await.expect("shutdown");
}

#[test]
fn contains_question_uses_name_matching() {
    let mut matcher = DomainRuleMatcher::default();
    matcher
        .add_expression("domain:example.com", "test")
        .expect("rule");
    matcher.finalize().expect("finalize");
    let backend = Arc::new(DynamicDomainSetBackend::new(
        "learned".to_string(),
        test_config(PathBuf::from("unused")),
    ));
    backend.store_snapshot_for_test(DynamicDomainSetSnapshot { matcher });
    let provider = DynamicDomainSet {
        tag: "learned".to_string(),
        backend,
    };
    assert!(provider.contains_question(&test_question("www.example.com.")));

    let request = Message::new();
    let _ctx = crate::core::context::DnsContext::new(
        SocketAddr::new("127.0.0.1".parse().unwrap(), 53),
        request,
    );
}
