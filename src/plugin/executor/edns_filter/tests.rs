// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::config::EdnsOptionsPolicy;
use super::*;
use crate::core::context::ExecutionPathEvent;
use crate::plugin::executor::sequence::chain::ChainProgram;
use crate::proto::{DNSClass, EdnsLocal, EdnsOption, Name, Question, Rcode, RecordType};

fn context() -> DnsContext {
    let mut request = Message::new();
    request.set_id(42);
    request.add_question(Question::new(
        Name::from_ascii("example.com.").unwrap(),
        RecordType::A,
        DNSClass::IN,
    ));
    for code in [8, 65001] {
        request
            .ensure_edns_mut()
            .insert(EdnsOption::Local(EdnsLocal::new(code, vec![1, 2, 3])));
    }
    let mut context = DnsContext::new("127.0.0.1:5300".parse().unwrap(), request);
    context.set_attr("outer", 17u32);
    context.enable_execution_path();
    context
}

fn codes(request: &Message) -> Vec<u16> {
    request
        .edns()
        .as_ref()
        .map(|edns| edns.options().iter().map(EdnsOption::code).collect())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy)]
enum Outcome {
    Complete(ExecStep),
    Fail,
    Pending,
}

#[derive(Debug)]
struct Probe {
    calls: Arc<AtomicUsize>,
    expected_codes: Vec<u16>,
    outcome: Outcome,
}

#[async_trait]
impl Plugin for Probe {
    fn tag(&self) -> &str {
        "probe"
    }

    async fn init(&mut self, _: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Executor for Probe {
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(codes(&context.request), self.expected_codes);
        assert_eq!(context.get_attr::<u32>("outer"), Some(&17));
        context.request.set_id(99);
        context.request.questions_mut()[0].set_name(Name::from_ascii("changed.example.").unwrap());
        context
            .request
            .ensure_edns_mut()
            .insert(EdnsOption::Local(EdnsLocal::new(65002, vec![4])));
        context.set_response(context.request.response(Rcode::NoError));
        context.marks_mut().insert(7);
        context.set_attr("probe", true);
        context.push_execution_path_event(ExecutionPathEvent::new(
            "probe",
            None,
            "executor",
            Some("probe"),
            "entered",
        ));
        match self.outcome {
            Outcome::Complete(step) => Ok(step),
            Outcome::Fail => Err(DnsError::Runtime("probe failure".into())),
            Outcome::Pending => futures::future::pending().await,
        }
    }
}

fn continuation(outcome: Outcome, expected_codes: Vec<u16>) -> (ExecutorNext, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let program = ChainProgram::single_with_next_executor_for_test(Arc::new(Probe {
        calls: calls.clone(),
        expected_codes,
        outcome,
    }));
    (ExecutorNext::from_program_for_test(program, 0), calls)
}

fn assert_other_state_preserved(context: &DnsContext) {
    assert_eq!(context.response().unwrap().id(), 99);
    assert_eq!(
        context.response().unwrap().questions()[0].name().to_fqdn(),
        "changed.example."
    );
    assert!(context.marks().contains(&7));
    assert_eq!(context.get_attr::<u32>("outer"), Some(&17));
    assert_eq!(context.get_attr::<bool>("probe"), Some(&true));
    assert!(
        context.execution_path_events().iter().any(|event| {
            event.sequence_tag == "probe" && event.tag.as_deref() == Some("probe")
        })
    );
}

#[tokio::test]
async fn restores_only_request_and_propagates_every_result() {
    for restore_request in [true, false] {
        for outcome in [
            Outcome::Complete(ExecStep::Next),
            Outcome::Complete(ExecStep::Return),
            Outcome::Complete(ExecStep::Stop),
            Outcome::Fail,
        ] {
            let plugin = EdnsFilter::new(
                "filter".into(),
                EdnsFilterConfig {
                    denylist: vec![65001],
                    restore_request,
                    ..Default::default()
                },
            );
            let (next, calls) = continuation(outcome, vec![8]);
            let mut context = context();
            let original = context.request.clone();
            let result = plugin.execute_with_next(&mut context, Some(next)).await;
            match (outcome, result) {
                (Outcome::Complete(expected), Ok(actual)) => assert_eq!(actual, expected),
                (Outcome::Fail, Err(DnsError::Runtime(message))) => {
                    assert_eq!(message, "probe failure")
                }
                (_, other) => panic!("unexpected result: {other:?}"),
            }
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            if restore_request {
                assert_eq!(context.request, original);
            } else {
                assert_eq!(context.request.id(), 99);
                assert_eq!(codes(&context.request), [8, 65002]);
                assert_eq!(
                    context.request.questions()[0].name().to_fqdn(),
                    "changed.example."
                );
            }
            assert_other_state_preserved(&context);
        }
    }
}

#[tokio::test]
async fn restores_downstream_edits_even_without_option_deletions() {
    let plugin = EdnsFilter::new("filter".into(), EdnsFilterConfig::default());
    let (next, calls) = continuation(Outcome::Complete(ExecStep::Next), vec![8, 65001]);
    let mut context = context();
    let original = context.request.clone();
    plugin
        .execute_with_next(&mut context, Some(next))
        .await
        .unwrap();
    assert_eq!(context.request, original);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_other_state_preserved(&context);
}

#[tokio::test]
async fn dropping_started_execution_restores_request_without_rolling_back_other_state() {
    for restore_request in [true, false] {
        let plugin = EdnsFilter::new(
            "filter".into(),
            EdnsFilterConfig {
                denylist: vec![65001],
                restore_request,
                ..Default::default()
            },
        );
        let (next, calls) = continuation(Outcome::Pending, vec![8]);
        let mut context = context();
        let original = context.request.clone();
        let mut execution = Box::pin(plugin.execute_with_next(&mut context, Some(next)));
        assert!(futures::poll!(execution.as_mut()).is_pending());
        drop(execution);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        if restore_request {
            assert_eq!(context.request, original);
        } else {
            assert_eq!(codes(&context.request), [8, 65002]);
        }
        assert_other_state_preserved(&context);
    }
}

#[tokio::test]
async fn no_continuation_obeys_restore_setting() {
    for restore_request in [true, false] {
        let plugin = EdnsFilter::new(
            "filter".into(),
            EdnsFilterConfig {
                policy: EdnsOptionsPolicy::Strip,
                restore_request,
                ..Default::default()
            },
        );
        let mut context = context();
        let original = context.request.clone();
        assert_eq!(plugin.execute(&mut context).await.unwrap(), ExecStep::Next);
        if restore_request {
            assert_eq!(context.request, original);
        } else {
            assert!(codes(&context.request).is_empty());
            assert!(context.request.edns().is_some());
        }
    }
}

#[test]
fn nested_scopes_restore_in_reverse_order() {
    let mut context = context();
    let original = context.request.clone();
    {
        let outer = RequestRestoreGuard::new(&mut context);
        outer.context.request.set_id(43);
        {
            let inner = RequestRestoreGuard::new(outer.context);
            inner.context.request.set_id(44);
        }
        assert_eq!(outer.context.request.id(), 43);
    }
    assert_eq!(context.request, original);
}

#[test]
fn parses_defaults_policies_lists_and_restore_switch() {
    for yaml in ["{}", "{policy: preserve}"] {
        let config: EdnsFilterConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(config.policy, EdnsOptionsPolicy::Preserve);
        assert!(config.allowlist.is_empty());
        assert!(config.denylist.is_empty());
        assert!(config.restore_request);
    }
    for (policy, expected) in [
        ("strip", EdnsOptionsPolicy::Strip),
        ("strip_unknown", EdnsOptionsPolicy::StripUnknown),
    ] {
        let config: EdnsFilterConfig = serde_yaml_ng::from_str(&format!(
            "policy: {policy}\nallowlist: [0, 65535]\ndenylist: [8, 8]\nrestore_request: false"
        ))
        .unwrap();
        assert_eq!(config.policy, expected);
        assert_eq!(config.allowlist, [0, 65535]);
        assert_eq!(config.denylist, [8, 8]);
        assert!(!config.restore_request);
    }
    for yaml in [
        "policy: invalid",
        "allowlist: [-1]",
        "denylist: [65536]",
        "allowlist: [1.5]",
        "denylist: [text]",
        "allowlist: ['8']",
        "restore_request: 'false'",
    ] {
        assert!(
            serde_yaml_ng::from_str::<EdnsFilterConfig>(yaml).is_err(),
            "accepted {yaml}"
        );
    }
}
