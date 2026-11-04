// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, SocketAddr};

use super::config::{
    DEFAULT_TIMEOUT, LearnDomainConfig, LearnErrorMode, LearnPhase, LearnRuleKind, QuestionMode,
    build_config, parse_provider_from_value, parse_qtypes,
};
use super::executor::LearnDomainExecutor;
use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::provider::dynamic_domain_set::DynamicDomainRuleKind;
use crate::plugin::test_utils::test_context;
use crate::proto::rdata::A;
use crate::proto::{DNSClass, Message, Name, Question, RData, Rcode, Record, RecordType};

fn make_context(qname: &str, qtype: RecordType, with_answer: bool) -> DnsContext {
    let mut request = Message::new();
    request.add_question(Question::new(
        Name::from_ascii(qname).expect("name"),
        qtype,
        DNSClass::IN,
    ));
    let mut context = DnsContext::new(SocketAddr::new("127.0.0.1".parse().unwrap(), 53), request);
    let mut response = context.request().response(Rcode::NoError);
    if with_answer {
        response.answers_mut().push(Record::from_rdata_with_class(
            Name::from_ascii(qname).expect("answer name"),
            60,
            DNSClass::IN,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
        ));
    }
    context.set_response(response);
    context
}

fn test_executor() -> LearnDomainExecutor {
    LearnDomainExecutor {
        tag: "learn".to_string(),
        config: LearnDomainConfig {
            provider_tag: "learned".to_string(),
            phase: LearnPhase::After,
            questions: QuestionMode::First,
            qtypes: parse_qtypes(None).expect("qtypes"),
            success_only: true,
            answer_required: true,
            rule_kind: LearnRuleKind::Full,
            async_mode: true,
            error_mode: LearnErrorMode::Continue,
            timeout: DEFAULT_TIMEOUT,
        },
        provider: None,
    }
}

#[test]
fn rule_kind_defaults_and_validation() {
    let config = |args: &str| PluginConfig {
        tag: "learn_main".to_string(),
        plugin_type: "learn_domain".to_string(),
        args: Some(serde_yaml_ng::from_str(args).expect("args")),
    };
    let defaults = build_config(&config("provider: learned")).expect("defaults");
    assert_eq!(defaults.rule_kind, LearnRuleKind::Full);

    for (kind, expected) in [
        ("full", LearnRuleKind::Full),
        ("domain", LearnRuleKind::Domain),
        ("registrable_domain", LearnRuleKind::RegistrableDomain),
    ] {
        let plugin = config(&format!("provider: learned\nrule_kind: {kind}"));
        assert_eq!(build_config(&plugin).expect("config").rule_kind, expected);
        assert_eq!(
            parse_provider_from_value(plugin.args).expect("provider dependency"),
            "learned"
        );
    }

    for invalid in [
        "root",
        "true",
        "[registrable_domain]",
        "{mode: registrable}",
    ] {
        let err = build_config(&config(&format!("provider: learned\nrule_kind: {invalid}")))
            .expect_err("invalid rule_kind must fail");
        assert!(err.to_string().contains("learn_domain config"));
    }
    let err = build_config(&config("provider: learned\ndomain_scope: registrable"))
        .expect_err("removed field must not silently change learning behavior");
    assert!(err.to_string().contains("unknown field `domain_scope`"));

    // This strategy belongs to the executor; provider/API input keeps its
    // existing rule syntax and must not perform implicit name extraction.
    assert!(serde_yaml_ng::from_str::<DynamicDomainRuleKind>("registrable_domain").is_err());
}

#[test]
fn rule_generation_strategies_produce_complete_matching_rules() {
    let ctx = make_context("Security.SNSSDK.COM.", RecordType::A, true);
    for (kind, expected) in [
        (LearnRuleKind::Full, "full:security.snssdk.com"),
        (LearnRuleKind::Domain, "domain:security.snssdk.com"),
        (LearnRuleKind::RegistrableDomain, "domain:snssdk.com"),
    ] {
        let mut executor = test_executor();
        executor.config.rule_kind = kind;
        assert_eq!(executor.extract_rules(&ctx).expect("rules"), vec![expected]);
    }
}

#[test]
fn registrable_rule_respects_public_private_wildcard_and_exception_boundaries() {
    let mut executor = test_executor();
    executor.config.rule_kind = LearnRuleKind::RegistrableDomain;
    for (qname, expected) in [
        ("a.b.example.com.", "domain:example.com"),
        ("example.com.", "domain:example.com"),
        ("Api.Example.Co.Uk.", "domain:example.co.uk"),
        ("api.foo.github.io.", "domain:foo.github.io"),
        ("a.foo.bar.ck.", "domain:foo.bar.ck"),
        ("a.www.ck.", "domain:www.ck"),
        ("www.xn--bcher-kva.de.", "domain:xn--bcher-kva.de"),
    ] {
        let ctx = make_context(qname, RecordType::AAAA, true);
        assert_eq!(
            executor.extract_rules(&ctx).expect("rules"),
            vec![expected],
            "{qname}"
        );
    }
}

#[test]
fn registrable_rule_skips_names_without_a_usable_site_boundary() {
    let mut executor = test_executor();
    executor.config.rule_kind = LearnRuleKind::RegistrableDomain;
    for qname in [
        ".",
        "com.",
        "co.uk.",
        "github.io.",
        "bar.ck.",
        "localhost.",
        "a.b.internal.",
        "printer.local.",
        "in-addr.arpa.",
        "4.3.2.1.in-addr.arpa.",
        "ip6.arpa.",
        "1.0.0.0.ip6.arpa.",
        r"foo\.bar.com.",
        r"foo\255.com.",
    ] {
        let ctx = make_context(qname, RecordType::A, true);
        assert!(
            executor.extract_rules(&ctx).expect("rules").is_empty(),
            "{qname}"
        );
    }
}

#[test]
fn registrable_rule_deduplicates_after_extraction_and_keeps_question_order() {
    let mut executor = test_executor();
    executor.config.rule_kind = LearnRuleKind::RegistrableDomain;
    executor.config.questions = QuestionMode::All;
    let mut ctx = make_context("security.snssdk.com.", RecordType::A, true);
    for (qname, qtype) in [
        ("aweme.snssdk.com.", RecordType::AAAA),
        ("www.example.co.uk.", RecordType::A),
        ("snssdk.com.", RecordType::AAAA),
        ("ignored.example.net.", RecordType::MX),
        ("localhost.", RecordType::A),
    ] {
        ctx.request_mut().add_question(Question::new(
            Name::from_ascii(qname).unwrap(),
            qtype,
            DNSClass::IN,
        ));
    }
    assert_eq!(
        executor.extract_rules(&ctx).expect("rules"),
        vec!["domain:snssdk.com", "domain:example.co.uk"]
    );
    executor.config.questions = QuestionMode::First;
    assert_eq!(
        executor.extract_rules(&ctx).expect("rules"),
        vec!["domain:snssdk.com"]
    );
    ctx.request_mut().questions_mut().clear();
    assert!(executor.extract_rules(&ctx).expect("rules").is_empty());
}

#[test]
fn registrable_rule_preserves_response_and_qtype_guards() {
    let mut executor = test_executor();
    executor.config.rule_kind = LearnRuleKind::RegistrableDomain;
    for (qtype, has_answer, rcode) in [
        (RecordType::A, false, Rcode::NoError),
        (RecordType::MX, true, Rcode::NoError),
        (RecordType::A, true, Rcode::NXDomain),
    ] {
        let mut ctx = make_context("security.snssdk.com.", qtype, has_answer);
        ctx.response_mut().unwrap().set_rcode(rcode);
        assert!(executor.extract_rules(&ctx).expect("rules").is_empty());
    }
    let mut ctx = make_context("security.snssdk.com.", RecordType::A, false);
    ctx.take_response();
    assert!(executor.extract_rules(&ctx).expect("rules").is_empty());
    executor.config.phase = LearnPhase::Before;
    assert_eq!(
        executor.extract_rules(&ctx).expect("rules"),
        vec!["domain:snssdk.com"]
    );
}

#[tokio::test]
async fn registrable_rule_skips_are_not_learning_failures() {
    let mut executor = test_executor();
    executor.config.rule_kind = LearnRuleKind::RegistrableDomain;
    executor.config.error_mode = LearnErrorMode::Fail;
    let mut ctx = make_context("localhost.", RecordType::A, true);
    // No provider is initialized: an attempted write would fail in this mode.
    assert!(matches!(
        executor.execute(&mut ctx).await.expect("skip"),
        ExecStep::Next
    ));
}

#[test]
fn extract_rules_obeys_success_answer_and_qtype_filters() {
    let executor = test_executor();
    let ctx = make_context("Example.COM.", RecordType::A, true);
    let rules = executor.extract_rules(&ctx).expect("rules");
    assert_eq!(rules, vec!["full:example.com"]);

    let no_answer = make_context("example.net.", RecordType::A, false);
    assert!(
        executor
            .extract_rules(&no_answer)
            .expect("rules")
            .is_empty()
    );

    let unsupported = make_context("example.org.", RecordType::MX, true);
    assert!(
        executor
            .extract_rules(&unsupported)
            .expect("rules")
            .is_empty()
    );
}

#[test]
fn parse_qtypes_rejects_empty_and_unknown_values() {
    assert!(parse_qtypes(Some(vec![])).is_err());
    assert!(parse_qtypes(Some(vec!["NOPE".to_string()])).is_err());
}

#[tokio::test]
async fn learn_domain_without_provider_continues_on_default_error_mode() {
    let executor = test_executor();
    let mut ctx = test_context();
    let step = executor
        .execute_with_next(&mut ctx, None)
        .await
        .expect("default error mode should continue");
    assert!(matches!(step, ExecStep::Next));
}

#[tokio::test]
async fn learn_domain_error_modes_map_learning_failures() {
    let mut continue_executor = test_executor();
    continue_executor.config.phase = LearnPhase::Before;
    continue_executor.config.error_mode = LearnErrorMode::Continue;
    let mut ctx = make_context("example.com.", RecordType::A, true);
    let step = continue_executor
        .execute_with_next(&mut ctx, None)
        .await
        .expect("continue mode should preserve dns flow");
    assert!(matches!(step, ExecStep::Next));

    let mut stop_executor = test_executor();
    stop_executor.config.phase = LearnPhase::Before;
    stop_executor.config.error_mode = LearnErrorMode::Stop;
    let mut ctx = make_context("example.com.", RecordType::A, true);
    let step = stop_executor
        .execute_with_next(&mut ctx, None)
        .await
        .expect("stop mode should return a stop step");
    assert!(matches!(step, ExecStep::Stop));

    let mut fail_executor = test_executor();
    fail_executor.config.phase = LearnPhase::Before;
    fail_executor.config.error_mode = LearnErrorMode::Fail;
    let mut ctx = make_context("example.com.", RecordType::A, true);
    let err = fail_executor
        .execute_with_next(&mut ctx, None)
        .await
        .expect_err("fail mode should return an error");
    assert!(err.to_string().contains("provider is not initialized"));
}
