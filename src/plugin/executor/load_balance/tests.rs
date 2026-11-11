// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::atomic::AtomicUsize;

use super::*;
use crate::infra::error::DnsError;
use crate::infra::observability::metrics::{
    metrics_test_guard, register_metric_source, render_prometheus_metrics,
};
use crate::plugin::executor::ExecutorNext;
use crate::proto::{Message, Rcode};

fn config(args: Option<&str>) -> PluginConfig {
    PluginConfig {
        tag: "balance_test".into(),
        plugin_type: "load_balance".into(),
        args: args.map(|args| serde_yaml_ng::from_str(args).unwrap()),
    }
}

#[test]
fn defaults_and_invalid_configs() {
    let parsed = Config::parse(&config(Some("executors: [{exec: first}]"))).unwrap();
    assert_eq!(parsed.strategy, selection::Strategy::RoundRobin);
    assert_eq!(parsed.executors[0].weight, 1);
    assert!(Config::parse(&config(None)).is_err());
    for args in [
        "{}",
        "executors: []",
        "executors: [{exec: ''}]",
        "executors: [{exec: '   '}]",
        "executors: [{exec: '$first'}]",
        "executors: [{exec: 'forward 1.1.1.1'}]",
        "executors: [{exec: first}, {exec: first}]",
        "executors: [{exec: first, weight: 0}]",
        "executors: [{exec: first, weight: -1}]",
        "executors: [{exec: first, weight: 1.5}]",
        "executors: [{exec: first, weight: 4294967296}]",
        "executors: [first]",
        "strategy: fastest\nexecutors: [{exec: first}]",
    ] {
        assert!(
            Config::parse(&config(Some(args))).is_err(),
            "accepted {args}"
        );
    }
}

#[derive(Debug)]
struct Stub {
    calls: AtomicUsize,
    step: ExecStep,
    fail: bool,
    pending: bool,
}

#[async_trait]
impl Plugin for Stub {
    fn tag(&self) -> &str {
        "stub"
    }

    async fn init(&mut self, _: &PluginInitContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        panic!("dependency lifecycle belongs to runtime")
    }
}

#[async_trait]
impl Executor for Stub {
    async fn execute(&self, _: &mut DnsContext) -> Result<ExecStep> {
        panic!("must use execute_with_next")
    }

    async fn execute_with_next(
        &self,
        context: &mut DnsContext,
        next: Option<ExecutorNext>,
    ) -> Result<ExecStep> {
        assert!(next.is_none());
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.pending {
            std::future::pending::<()>().await;
        }
        if self.fail {
            return Err(DnsError::Runtime("original branch error".into()));
        }
        assert!(context.marks().contains(&10));
        assert_eq!(context.get_attr::<String>("existing").unwrap(), "retained");
        context.marks_mut().insert(20);
        context.set_attr("branch", 370u32);
        context.request.set_id(370);
        if let Some(response) = context.response_mut() {
            response.set_rcode(Rcode::ServFail);
        }
        Ok(self.step)
    }
}

fn balancer(step: ExecStep, fail: bool, pending: bool) -> (LoadBalance, Arc<Stub>, Arc<Stub>) {
    let first = Arc::new(Stub {
        calls: AtomicUsize::new(0),
        step,
        fail,
        pending,
    });
    let second = Arc::new(Stub {
        calls: AtomicUsize::new(0),
        step: ExecStep::Next,
        fail: false,
        pending: false,
    });
    let plugin = LoadBalance {
        targets: vec![first.clone(), second.clone()],
        selector: Selector::new(selection::Strategy::RoundRobin, [1, 1].into_iter()).unwrap(),
        metrics: Arc::new(LoadBalanceMetrics::new(
            "balance_unit_metrics".into(),
            ["first".into(), "second".into()].into_iter(),
        )),
    };
    (plugin, first, second)
}

fn context() -> DnsContext {
    let mut context = DnsContext::new("127.0.0.1:12345".parse().unwrap(), Message::new());
    context.marks_mut().insert(10);
    context.set_attr("existing", "retained".to_string());
    context
}

#[tokio::test]
async fn subcall_flow_and_context_are_preserved() {
    for (child, expected) in [
        (ExecStep::Next, ExecStep::Next),
        (ExecStep::Return, ExecStep::Next),
        (ExecStep::Stop, ExecStep::Stop),
    ] {
        for response in [false, true] {
            let (plugin, first, second) = balancer(child, false, false);
            let mut context = context();
            if response {
                context.set_response(Message::new());
            }
            assert_eq!(plugin.execute(&mut context).await.unwrap(), expected);
            assert_eq!(first.calls.load(Ordering::Relaxed), 1);
            assert_eq!(second.calls.load(Ordering::Relaxed), 0);
            assert_eq!(context.request.id(), 370);
            assert!(context.marks().contains(&10));
            assert!(context.marks().contains(&20));
            assert_eq!(context.get_attr::<u32>("branch"), Some(&370));
            assert_eq!(
                context.response().map(|response| response.rcode()),
                response.then_some(Rcode::ServFail)
            );
            assert_eq!(plugin.metrics.targets[0].errors.load(Ordering::Relaxed), 0);
        }
    }
}

#[tokio::test]
async fn errors_propagate_without_trying_another_target() {
    let (plugin, first, second) = balancer(ExecStep::Next, true, false);
    let error = plugin.execute(&mut context()).await.unwrap_err();
    assert!(matches!(error, DnsError::Runtime(message) if message == "original branch error"));
    assert_eq!(first.calls.load(Ordering::Relaxed), 1);
    assert_eq!(second.calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        plugin.metrics.targets[0].selected.load(Ordering::Relaxed),
        1
    );
    assert_eq!(plugin.metrics.targets[0].errors.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn cancellation_consumes_one_selection() {
    let (plugin, first, second) = balancer(ExecStep::Next, false, true);
    let mut first_context = context();
    let mut future = Box::pin(plugin.execute(&mut first_context));
    // One poll starts the pending branch without timers or detached tasks.
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(future.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(future);
    assert_eq!(
        plugin.execute(&mut context()).await.unwrap(),
        ExecStep::Next
    );
    assert_eq!(first.calls.load(Ordering::Relaxed), 1);
    assert_eq!(second.calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        plugin.metrics.targets[0].selected.load(Ordering::Relaxed),
        1
    );
    assert_eq!(plugin.metrics.targets[0].errors.load(Ordering::Relaxed), 0);
}

#[test]
fn metrics_render_and_unregister() {
    let _guard = metrics_test_guard();
    let (plugin, _, _) = balancer(ExecStep::Next, false, false);
    plugin.metrics.targets[0]
        .selected
        .store(3, Ordering::Relaxed);
    plugin.metrics.targets[0].errors.store(1, Ordering::Relaxed);
    register_metric_source(plugin.metrics.clone()).unwrap();
    let output = render_prometheus_metrics();
    assert!(output.contains(
        "load_balance_selected_total{plugin_tag=\"balance_unit_metrics\",executor=\"first\"} 3"
    ));
    assert!(output.contains(
        "load_balance_error_total{plugin_tag=\"balance_unit_metrics\",executor=\"first\"} 1"
    ));
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(plugin.destroy())
        .unwrap();
    assert!(!render_prometheus_metrics().contains("balance_unit_metrics"));
}
