// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, SocketAddr};

use super::config::{
    DEFAULT_TIMEOUT, LearnDomainConfig, LearnErrorMode, LearnPhase, QuestionMode, parse_qtypes,
};
use super::executor::LearnDomainExecutor;
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
            rule_kind: DynamicDomainRuleKind::Full,
            async_mode: true,
            error_mode: LearnErrorMode::Continue,
            timeout: DEFAULT_TIMEOUT,
        },
        provider: None,
    }
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
