// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ros_route` executor plugin.
//!
//! This executor is an observer-side effect stage designed to integrate with
//! OxiDNS sequence pipelines. It does not alter DNS decisions or response
//! content. Instead, it watches final downstream DNS answers and synchronizes
//! host routes into a dedicated RouterOS routing table.
//!
//! Architecture overview:
//! - continuation execution stays hot-path light and observes final A/AAAA
//!   answers.
//! - route synchronization is delegated to a single-owner background
//!   `RouteManager` state machine.
//! - RouterOS API details are isolated in `MikrotikApi` adapter
//!   implementations.
//! - route metadata is persisted in RouterOS `comment` via `RouteCommentCodec`,
//!   allowing restart recovery without local state files.
//!
//! Behavior goals:
//! - maintain `/32` (IPv4) and `/128` (IPv6) host routes in configured table.
//! - support optional always-present CIDR routes via `persistent`.
//! - load persistent route files once during plugin initialization.
//! - preserve DNS hot-path latency (`async=true` uses non-blocking queue).
//! - provide blocking write-before-return mode (`async=false`) without
//!   affecting DNS response result.
//! - avoid long-term route pollution via TTL sweep + startup reconciliation +
//!   optional shutdown cleanup.
//! - assume routing table/rule/default routes are already provisioned by users.

use std::net::IpAddr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use ahash::AHashSet;
use async_trait::async_trait;
use tokio::sync::oneshot;
use tracing::warn;

use self::api::{MikrotikApi, MikrotikRsClient};
use self::config::{MikrotikConfig, parse_plugin_config, validate_comment_token};
use self::manager::{
    ObserveEnqueueError, RouteManager, RouteManagerConfig, RouteManagerHandle, RouteManagerRuntime,
};
use self::metrics::RosRouteMetrics;
use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result};
use crate::infra::observability::metrics::{register_metric_source, unregister_metric_source};
use crate::plugin::executor::routeros::ip_prefix::IpPrefix;
use crate::plugin::executor::routeros::throttle::ErrorLogThrottle;
use crate::plugin::executor::routeros::{ObservedAddr, SHUTDOWN_TIMEOUT, collect_observed_addrs};
use crate::plugin::executor::{ExecStep, Executor, ExecutorNext};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::proto::{Rcode, RecordType};
use crate::{continue_next, plugin_factory};

mod api;
mod config;
mod manager;
mod metrics;
mod model;
mod persistent;

#[derive(Debug)]
struct ExtractedObservation {
    addrs: Vec<ObservedAddr>,
}

#[derive(Debug)]
struct MikrotikExecutor {
    tag: String,
    metrics: Arc<RosRouteMetrics>,
    config: MikrotikConfig,
    manager: Option<RouteManager>,
    manager_handle: Option<RouteManagerHandle>,
    runtime: Mutex<Option<RouteManagerRuntime>>,
    queue_logs: ErrorLogThrottle,
}

#[async_trait]
impl Plugin for MikrotikExecutor {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        if self.manager.is_none() || self.manager_handle.is_some() {
            return Ok(());
        }

        let Some(manager) = self.manager.take() else {
            return Ok(());
        };

        register_metric_source(self.metrics.clone())?;
        let runtime = RouteManagerRuntime::start(self.tag.clone(), manager)?;
        let manager_handle = runtime.handle();
        let mut runtime = Some(runtime);
        if let Ok(mut slot) = self.runtime.lock() {
            *slot = runtime.take();
        }
        if let Some(runtime) = runtime {
            unregister_metric_source(&self.tag);
            let _ = runtime.shutdown(false).await;
            return Err(DnsError::plugin(
                "ros_route runtime lock is poisoned during initialization",
            ));
        }
        self.manager_handle = Some(manager_handle);
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        let deadline = tokio::time::Instant::now() + SHUTDOWN_TIMEOUT;
        if let Some(runtime) = self.runtime.lock().ok().and_then(|mut slot| slot.take()) {
            unregister_metric_source(&self.tag);
            return runtime
                .shutdown_until(self.config.cleanup_on_shutdown, deadline)
                .await;
        }
        Ok(())
    }
}

#[async_trait]
impl Executor for MikrotikExecutor {
    fn with_next(&self) -> bool {
        true
    }

    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        self.execute_with_next(context, None).await
    }

    async fn execute_with_next(
        &self,
        context: &mut DnsContext,
        next: Option<ExecutorNext>,
    ) -> Result<ExecStep> {
        let step = continue_next!(next, context)?;
        let Some(handle) = self.manager_handle.as_ref() else {
            return Ok(step);
        };

        let Some(ExtractedObservation { addrs }) = extract_observation(context, &self.config)
        else {
            return Ok(step);
        };
        self.metrics.observe_total.fetch_add(1, Ordering::Relaxed);

        if self.config.async_mode {
            match handle.try_observe(addrs, None) {
                Ok(_) => {}
                Err(ObserveEnqueueError::Full) => {
                    self.metrics.dropped_total.fetch_add(1, Ordering::Relaxed);
                    if self.queue_logs.should_log("full") {
                        warn!(
                            plugin = %self.tag,
                            "ros_route observe queue is full, observation dropped"
                        );
                    }
                }
                Err(ObserveEnqueueError::Closed) => {
                    self.metrics.dropped_total.fetch_add(1, Ordering::Relaxed);
                    warn!(
                        plugin = %self.tag,
                        "ros_route manager channel closed, observation dropped"
                    );
                }
            }
            return Ok(step);
        }

        let (wait_tx, wait_rx) = oneshot::channel::<Result<()>>();
        let deadline = tokio::time::Instant::now() + self.config.wait_timeout;
        match handle.try_observe(addrs, Some(wait_tx)) {
            Ok(_) => {}
            Err(_) => {
                self.metrics
                    .sync_error_total
                    .fetch_add(1, Ordering::Relaxed);
                warn!(
                    plugin = %self.tag,
                    "ros_route manager channel closed in sync mode, DNS response is kept unchanged"
                );
                return Ok(step);
            }
        }

        let wait_outcome = tokio::time::timeout_at(deadline, wait_rx).await;
        match wait_outcome {
            Ok(Ok(Ok(()))) => Ok(step),
            Ok(Ok(Err(e))) => {
                self.metrics
                    .sync_error_total
                    .fetch_add(1, Ordering::Relaxed);
                warn!(
                    plugin = %self.tag,
                    err = %e,
                    "ros_route observe failed in sync mode, DNS response is kept unchanged"
                );
                Ok(step)
            }
            Ok(Err(_)) => {
                self.metrics
                    .sync_error_total
                    .fetch_add(1, Ordering::Relaxed);
                warn!(
                    plugin = %self.tag,
                    "ros_route manager dropped sync observe response, DNS response is kept unchanged"
                );
                Ok(step)
            }
            Err(_) => {
                self.metrics
                    .sync_timeout_total
                    .fetch_add(1, Ordering::Relaxed);
                warn!(
                    plugin = %self.tag,
                    timeout_ms = self.config.wait_timeout.as_millis(),
                    "ros_route observe timed out in sync mode, DNS response is kept unchanged"
                );
                Ok(step)
            }
        }
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("ros_route")]
pub struct MikrotikFactory;

impl PluginFactory for MikrotikFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        validate_comment_token("plugin tag", plugin_config.tag.as_str())?;
        let mut config = parse_plugin_config(plugin_config.args.clone(), true)?;
        let connection = config
            .connection
            .take()
            .ok_or_else(|| DnsError::plugin("ros_route connection config already consumed"))?;
        let api = Arc::new(MikrotikRsClient::new(connection)) as Arc<dyn MikrotikApi>;

        let manager_cfg = RouteManagerConfig {
            plugin_tag: plugin_config.tag.clone(),
            routing_table: config.routing_table.clone(),
            gateway4: config.gateway4.clone(),
            gateway6: config.gateway6.clone(),
            persistent_ips: config
                .persistent_ips
                .iter()
                .map(|raw| raw.parse::<IpPrefix>())
                .collect::<std::result::Result<AHashSet<_>, _>>()?,
            comment_prefix: config.comment_prefix.clone(),
            distance: config.distance,
            min_ttl: config.min_ttl,
            max_ttl: config.max_ttl,
            fixed_ttl: config.fixed_ttl,
            conntrack_guard: config.conntrack_guard,
            queue_capacity: config.queue_capacity,
        };
        let metrics = Arc::new(RosRouteMetrics::new(plugin_config.tag.clone()));
        let manager = RouteManager::with_metrics(api, manager_cfg, metrics.clone());

        Ok(UninitializedPlugin::Executor(Box::new(MikrotikExecutor {
            tag: plugin_config.tag.clone(),
            metrics,
            config,
            manager: Some(manager),
            manager_handle: None,
            runtime: Mutex::new(None),
            queue_logs: ErrorLogThrottle::default(),
        })))
    }
}

fn extract_observation(
    context: &mut DnsContext,
    config: &MikrotikConfig,
) -> Option<ExtractedObservation> {
    let question = context.request.first_question()?;
    match question.qtype() {
        RecordType::A | RecordType::AAAA => {}
        _ => return None,
    }

    let response = context.response()?;
    if response.rcode() != Rcode::NoError {
        return None;
    }
    let addrs = collect_observed_addrs(&context.request, response, |ip| match ip {
        IpAddr::V4(_) => config.gateway4.is_some(),
        IpAddr::V6(_) => config.gateway6.is_some(),
    });
    (!addrs.is_empty()).then_some(ExtractedObservation { addrs })
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::time::Duration;

    use serde_yaml_ng::Value;

    use super::config::{DEFAULT_QUEUE_CAPACITY, DEFAULT_WAIT_TIMEOUT};
    use super::*;
    use crate::proto::rdata::{A, AAAA, CNAME, SOA};
    use crate::proto::{DNSClass, Message, Name, Question, RData, Record};

    fn observation_config() -> MikrotikConfig {
        let args = serde_yaml_ng::from_str::<Value>(
            r#"
address: "127.0.0.1:8728"
username: "api"
password: "secret"
routing_table: "policy"
gateway4: "192.0.2.1"
gateway6: "2001:db8::1"
"#,
        )
        .expect("yaml");
        parse_plugin_config(Some(args), false).expect("config")
    }

    fn context_with_rcode(qtype: RecordType, rcode: Rcode) -> DnsContext {
        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii("example.com.").expect("domain"),
            qtype,
            DNSClass::IN,
        ));
        let response = request.response(rcode);
        let mut context = DnsContext::new(
            "127.0.0.1:5353".parse::<SocketAddr>().expect("client"),
            request,
        );
        context.set_response(response);
        context
    }

    fn context_with_nodata(qtype: RecordType) -> DnsContext {
        context_with_rcode(qtype, Rcode::NoError)
    }

    #[test]
    fn fixed_ttl_zero_is_accepted() {
        let args = serde_yaml_ng::from_str::<Value>(
            r#"
address: "127.0.0.1:8728"
username: "api"
password: "secret"
routing_table: "policy"
gateway4: "192.0.2.1"
fixed_ttl: 0
"#,
        )
        .expect("yaml");
        let parsed = parse_plugin_config(Some(args), false).expect("config");
        assert_eq!(parsed.fixed_ttl, Some(0));
    }

    #[test]
    fn config_defaults_and_accepts_wait_and_queue_settings() {
        let defaults = observation_config();
        assert_eq!(defaults.wait_timeout, DEFAULT_WAIT_TIMEOUT);
        assert_eq!(defaults.queue_capacity, DEFAULT_QUEUE_CAPACITY);

        let args = serde_yaml_ng::from_str::<Value>(
            "address: 127.0.0.1:8728\nusername: api\npassword: secret\nrouting_table: policy\ngateway4: 192.0.2.1\nwait_timeout: 1500ms\nqueue_capacity: 32\n",
        )
        .expect("yaml");
        let parsed = parse_plugin_config(Some(args), false).expect("config");
        assert_eq!(parsed.wait_timeout, Duration::from_millis(1_500));
        assert_eq!(parsed.queue_capacity, 32);
    }

    #[test]
    fn config_rejects_zero_wait_and_queue_settings() {
        for invalid in ["wait_timeout: 0s", "queue_capacity: 0"] {
            let yaml = format!(
                "address: 127.0.0.1:8728\nusername: api\npassword: secret\nrouting_table: policy\ngateway4: 192.0.2.1\n{invalid}\n"
            );
            let value = serde_yaml_ng::from_str::<Value>(&yaml).expect("yaml");
            assert!(parse_plugin_config(Some(value), false).is_err());
        }
    }

    #[test]
    fn config_rejects_old_persistent_route_key() {
        let args = serde_yaml_ng::from_str::<Value>(
            r#"
address: "127.0.0.1:8728"
username: "api"
password: "secret"
routing_table: "policy"
gateway4: "192.0.2.1"
persistent_route:
  ips:
    - "192.0.2.10"
"#,
        )
        .expect("yaml");
        let error = parse_plugin_config(Some(args), false).expect_err("old key");
        assert!(error.to_string().contains("persistent_route"));
    }

    #[test]
    fn config_keeps_plaintext_when_tls_is_omitted() {
        let args = serde_yaml_ng::from_str::<Value>(
            r#"
address: "router.example:8728"
username: "api"
password: "secret"
routing_table: "policy"
gateway4: "192.0.2.1"
"#,
        )
        .expect("yaml");
        let parsed = parse_plugin_config(Some(args), false).expect("config");
        let debug = format!("{:?}", parsed.connection.expect("connection"));
        assert!(debug.contains("tls: None"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn conntrack_guard_defaults_to_disabled_and_can_be_enabled() {
        let base = r#"
address: "127.0.0.1:8728"
username: "api"
password: "secret"
routing_table: "policy"
gateway4: "192.0.2.1"
"#;
        let default_args = serde_yaml_ng::from_str::<Value>(base).expect("yaml");
        assert!(
            !parse_plugin_config(Some(default_args), false)
                .expect("default config")
                .conntrack_guard
        );

        let enabled_args =
            serde_yaml_ng::from_str::<Value>(&format!("{base}conntrack_guard: true\n"))
                .expect("yaml");
        assert!(
            parse_plugin_config(Some(enabled_args), false)
                .expect("enabled config")
                .conntrack_guard
        );
    }

    #[test]
    fn config_requires_a_gateway() {
        let args = serde_yaml_ng::from_str::<Value>(
            r#"
address: "127.0.0.1:8728"
username: "api"
password: "secret"
routing_table: "policy"
"#,
        )
        .expect("yaml");
        assert!(parse_plugin_config(Some(args), false).is_err());
    }

    #[test]
    fn config_defaults_comment_prefix_to_oxi() {
        let args = serde_yaml_ng::from_str::<Value>(
            r#"
address: "127.0.0.1:8728"
username: "api"
password: "secret"
routing_table: "policy"
gateway4: "192.0.2.1"
"#,
        )
        .expect("yaml");
        let parsed = parse_plugin_config(Some(args), false).expect("route config");
        assert_eq!(parsed.comment_prefix, "oxi");
    }

    #[test]
    fn observation_ignores_non_address_queries_and_nodata() {
        let config = observation_config();
        let mut txt_context = context_with_nodata(RecordType::TXT);
        assert!(extract_observation(&mut txt_context, &config).is_none());
        let mut any_context = context_with_nodata(RecordType::ANY);
        assert!(extract_observation(&mut any_context, &config).is_none());

        let mut a_context = context_with_nodata(RecordType::A);
        assert!(extract_observation(&mut a_context, &config).is_none());
    }

    #[test]
    fn nodata_for_disabled_query_family_is_ignored() {
        let mut config = observation_config();
        config.gateway4 = None;
        let mut context = context_with_nodata(RecordType::A);

        assert!(extract_observation(&mut context, &config).is_none());
    }

    #[test]
    fn observation_collects_all_answer_addresses_without_cname_ttl_cap() {
        let config = observation_config();
        let mut context = context_with_rcode(RecordType::A, Rcode::NoError);
        let response = context.response_mut().expect("response");
        response.add_answer(Record::from_rdata(
            Name::from_ascii("example.com.").expect("owner"),
            30,
            RData::CNAME(CNAME(
                Name::from_ascii("edge.example.com.").expect("target"),
            )),
        ));
        response.add_answer(Record::from_rdata(
            Name::from_ascii("edge.example.com.").expect("owner"),
            300,
            RData::A(A(Ipv4Addr::new(203, 0, 113, 27))),
        ));
        response.add_answer(Record::from_rdata(
            Name::from_ascii("unrelated.example.com.").expect("owner"),
            600,
            RData::A(A(Ipv4Addr::new(203, 0, 113, 28))),
        ));
        response.add_answer(Record::from_rdata(
            Name::from_ascii("unrelated.example.com.").expect("owner"),
            120,
            RData::AAAA(AAAA(Ipv6Addr::new(0x2001, 0xDB8, 0, 0, 0, 0, 0, 27))),
        ));

        let observation = extract_observation(&mut context, &config).expect("CNAME observation");

        assert_eq!(observation.addrs.len(), 3);
        assert!(observation.addrs.contains(&ObservedAddr {
            addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 27)),
            ttl_secs: 300,
        }));
        assert!(observation.addrs.contains(&ObservedAddr {
            addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 28)),
            ttl_secs: 600,
        }));
        assert!(observation.addrs.contains(&ObservedAddr {
            addr: IpAddr::V6(Ipv6Addr::new(0x2001, 0xDB8, 0, 0, 0, 0, 0, 27)),
            ttl_secs: 120,
        }));
    }

    #[test]
    fn nxdomain_does_not_withdraw_existing_leases() {
        let config = observation_config();
        let mut context = context_with_rcode(RecordType::AAAA, Rcode::NXDomain);

        assert!(extract_observation(&mut context, &config).is_none());
    }

    #[test]
    fn nxdomain_with_mismatched_question_is_ignored() {
        let config = observation_config();
        let mut context = context_with_rcode(RecordType::A, Rcode::NXDomain);
        let response = context.response_mut().expect("response");
        response.questions_mut().clear();
        response.add_question(Question::new(
            Name::from_ascii("other.example.").expect("other domain"),
            RecordType::A,
            DNSClass::IN,
        ));

        assert!(extract_observation(&mut context, &config).is_none());
    }

    #[test]
    fn negative_soa_ttl_does_not_create_a_withdrawal_observation() {
        let config = observation_config();
        let mut context = context_with_rcode(RecordType::A, Rcode::NXDomain);
        context
            .response_mut()
            .expect("response")
            .add_authority(Record::from_rdata(
                Name::from_ascii("example.com.").expect("zone"),
                120,
                RData::SOA(SOA::new(
                    Name::from_ascii("ns.example.com.").expect("mname"),
                    Name::from_ascii("hostmaster.example.com.").expect("rname"),
                    1,
                    3600,
                    600,
                    86400,
                    30,
                )),
            ));

        assert!(extract_observation(&mut context, &config).is_none());
    }
}
