// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ros_address_list` executor plugin.
//!
//! This executor is an observer-side effect stage designed to integrate with
//! OxiDNS sequence pipelines. It does not alter DNS decisions or response
//! content. Instead, it watches final downstream DNS answers and synchronizes
//! IPs into RouterOS address lists.
//!
//! Architecture overview:
//! - continuation pre-stage stays hot-path light.
//! - continuation post-stage extracts unique A/AAAA IPs.
//! - address-list synchronization is delegated to a single-owner background
//!   manager state machine.
//! - RouterOS API details are isolated in `MikrotikApi` adapter
//!   implementations.
//! - ownership metadata is persisted in RouterOS `comment` so cleanup can
//!   safely distinguish OxiDNS-managed entries from foreign entries.
//!
//! Behavior goals:
//! - maintain IPv4/IPv6 dynamic host entries in configured address lists.
//! - support optional always-present IP/CIDR entries via `persistent`.
//! - use RouterOS native `timeout` for dynamic expiration maintenance.
//! - preserve DNS hot-path latency (`async=true` uses non-blocking queue).
//! - provide blocking write-before-return mode (`async=false`) without
//!   affecting DNS response result.
//! - load persistent file-backed entries at startup and keep them fixed until
//!   the plugin is reloaded.

use std::net::IpAddr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::oneshot;
use tracing::warn;

use self::api::{MikrotikApi, MikrotikRsClient};
use self::config::{MikrotikConfig, parse_plugin_config, validate_comment_token};
use self::manager::{
    AddressListManager, AddressListManagerConfig, AddressListManagerHandle,
    AddressListManagerRuntime, ObserveEnqueueError,
};
use self::metrics::RosMetrics;
use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result};
use crate::infra::observability::metrics::{register_metric_source, unregister_metric_source};
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
struct MikrotikExecutor {
    /// Plugin tag from the global registry.
    tag: String,
    /// Shared observability counters.
    metrics: Arc<RosMetrics>,
    /// Fully validated immutable runtime config.
    config: MikrotikConfig,
    /// Pre-built manager consumed during `init()`.
    manager: Option<AddressListManager>,
    /// Coalescing mailbox handle exposed after the background runtime starts.
    manager_handle: Option<AddressListManagerHandle>,
    /// Runtime handle stored so `destroy()` can stop worker tasks.
    runtime: Mutex<Option<AddressListManagerRuntime>>,
    queue_logs: ErrorLogThrottle,
}

#[async_trait]
impl Plugin for MikrotikExecutor {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        // `init()` may be called more than once by the plugin framework.
        // Keep it idempotent and only build the runtime once.
        if self.manager.is_none() || self.manager_handle.is_some() {
            return Ok(());
        }

        let Some(manager) = self.manager.take() else {
            return Ok(());
        };

        register_metric_source(self.metrics.clone())?;
        let runtime = AddressListManagerRuntime::start(self.tag.clone(), manager)?;
        let manager_handle = runtime.handle();
        let mut runtime = Some(runtime);
        if let Ok(mut slot) = self.runtime.lock() {
            *slot = runtime.take();
        }
        if let Some(runtime) = runtime {
            unregister_metric_source(&self.tag);
            let _ = runtime.shutdown(false).await;
            return Err(DnsError::plugin(
                "ros_address_list runtime lock is poisoned during initialization",
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

    #[hotpath::measure]
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        self.execute_with_next(context, None).await
    }

    #[hotpath::measure]
    async fn execute_with_next(
        &self,
        context: &mut DnsContext,
        next: Option<ExecutorNext>,
    ) -> Result<ExecStep> {
        let step = continue_next!(next, context)?;
        let Some(handle) = self.manager_handle.as_ref() else {
            return Ok(step);
        };

        // This executor only reacts to successful final answers containing
        // A/AAAA data.
        let Some(addrs) = extract_observation(context, &self.config) else {
            return Ok(step);
        };
        self.metrics.observe_total.fetch_add(1, Ordering::Relaxed);

        if self.config.async_mode {
            // Async mode keeps RouterOS I/O fully off the request path.
            match handle.try_observe(addrs, None) {
                Ok(_) => {}
                Err(ObserveEnqueueError::Full) => {
                    self.metrics.dropped_total.fetch_add(1, Ordering::Relaxed);
                    if self.queue_logs.should_log("full") {
                        warn!(
                            plugin = %self.tag,
                            "ros_address_list observe queue is full, observation dropped"
                        );
                    }
                }
                Err(ObserveEnqueueError::Closed) => {
                    self.metrics.dropped_total.fetch_add(1, Ordering::Relaxed);
                    warn!(
                        plugin = %self.tag,
                        "ros_address_list manager channel closed, observation dropped"
                    );
                }
            }
            return Ok(step);
        }

        // Sync mode still preserves DNS behavior on RouterOS failures. The only
        // difference is that we wait for the manager to attempt the write.
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
                    "ros_address_list manager channel closed in sync mode, DNS response is kept unchanged"
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
                    "ros_address_list observe failed in sync mode, DNS response is kept unchanged"
                );
                Ok(step)
            }
            Ok(Err(_)) => {
                self.metrics
                    .sync_error_total
                    .fetch_add(1, Ordering::Relaxed);
                warn!(
                    plugin = %self.tag,
                    "ros_address_list manager dropped sync observe response, DNS response is kept unchanged"
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
                    "ros_address_list observe timed out in sync mode, DNS response is kept unchanged"
                );
                Ok(step)
            }
        }
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("ros_address_list")]
pub struct MikrotikFactory;

impl PluginFactory for MikrotikFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        // Plugin tag is reused inside RouterOS comment ownership metadata.
        validate_comment_token("plugin tag", plugin_config.tag.as_str())?;
        let mut config = parse_plugin_config(plugin_config.args.clone(), true)?;
        let connection = config.connection.take().ok_or_else(|| {
            DnsError::plugin("ros_address_list connection config already consumed")
        })?;
        let api = Arc::new(MikrotikRsClient::new(connection)) as Arc<dyn MikrotikApi>;

        let manager_cfg = AddressListManagerConfig {
            plugin_tag: plugin_config.tag.clone(),
            address_list4: config.address_list4.clone(),
            address_list6: config.address_list6.clone(),
            persistent_items: config.persistent_items.clone(),
            comment_prefix: config.comment_prefix.clone(),
            min_ttl: config.min_ttl,
            max_ttl: config.max_ttl,
            fixed_ttl: config.fixed_ttl,
            queue_capacity: config.queue_capacity,
        };
        let metrics = Arc::new(RosMetrics::new(plugin_config.tag.clone()));
        let manager = AddressListManager::with_metrics(api, manager_cfg, metrics.clone());

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
) -> Option<Vec<ObservedAddr>> {
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
        IpAddr::V4(_) => config.address_list4.is_some(),
        IpAddr::V6(_) => config.address_list6.is_some(),
    });
    if addrs.is_empty() {
        return None;
    }
    Some(addrs)
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use ahash::{AHashMap, AHashSet};
    use serde_yaml_ng::Value;

    use super::config::{
        DEFAULT_COMMENT_PREFIX, DEFAULT_MAX_TTL, DEFAULT_MIN_TTL, DEFAULT_QUEUE_CAPACITY,
        DEFAULT_WAIT_TIMEOUT,
    };
    use super::persistent::{load_persistent_items_from_content, parse_persistent_files};
    use super::*;
    use crate::infra::clock::AppClock;
    use crate::plugin::executor::ros_address_list::api::{MikrotikApiTimeouts, RouterListEntry};
    use crate::plugin::executor::ros_address_list::model::{
        AddressListFamily, AddressListKey, OwnedCommentKind, decode_owned_comment, encode_comment,
    };
    use crate::proto::rdata::{A, AAAA};
    use crate::proto::{DNSClass, Message, Name, Question, RData, Rcode, Record, RecordType};

    #[derive(Debug, Default)]
    struct MockApiState {
        entries: AHashMap<String, RouterListEntry>,
        next_id: u64,
        fail_next_upsert: bool,
        fail_healthcheck: bool,
        list_entries_calls: u64,
        list_entries_delay: Option<Duration>,
        convert_persistent_to_dynamic_after_list: bool,
        convert_owned_to_foreign_after_list: bool,
        upsert_v4: u64,
        upsert_v6: u64,
        update_ops: u64,
    }

    #[derive(Debug, Clone)]
    struct MockMikrotikApi {
        state: Arc<Mutex<MockApiState>>,
    }

    impl Default for MockMikrotikApi {
        fn default() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockApiState::default())),
            }
        }
    }

    impl MockMikrotikApi {
        fn storage_key(key: &AddressListKey) -> String {
            format!("{:?}:{}:{}", key.family, key.list, key.normalized_value())
        }

        fn seed_entry(&self, entry: RouterListEntry) {
            if let Ok(mut state) = self.state.lock() {
                state.entries.insert(Self::storage_key(&entry.key), entry);
            }
        }

        fn entry_count(&self) -> usize {
            self.state
                .lock()
                .map(|state| state.entries.len())
                .unwrap_or_default()
        }

        fn list_entries_calls(&self) -> u64 {
            self.state
                .lock()
                .map(|state| state.list_entries_calls)
                .unwrap_or_default()
        }
    }

    #[derive(Debug, Default)]
    struct PipelineMikrotikApi {
        active: AtomicUsize,
        max_active: AtomicUsize,
        attempts: AtomicUsize,
    }

    #[async_trait]
    impl MikrotikApi for PipelineMikrotikApi {
        async fn list_entries(
            &self,
            _list4: Option<&str>,
            _list6: Option<&str>,
        ) -> Result<Vec<RouterListEntry>> {
            Ok(Vec::new())
        }

        async fn upsert_owned_entry(
            &self,
            _key: &AddressListKey,
            _timeout: Option<&str>,
            _comment: &str,
            _comment_prefix: &str,
            _plugin_tag: &str,
            _refresh_timeout: bool,
        ) -> Result<Option<()>> {
            let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
            self.max_active.fetch_max(active, Ordering::AcqRel);
            self.attempts.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(10)).await;
            self.active.fetch_sub(1, Ordering::AcqRel);
            Ok(Some(()))
        }

        async fn delete_entry_if_matches(&self, _expected: &RouterListEntry) -> Result<bool> {
            Ok(false)
        }
    }

    #[async_trait]
    impl MikrotikApi for MockMikrotikApi {
        async fn list_entries(
            &self,
            list4: Option<&str>,
            list6: Option<&str>,
        ) -> Result<Vec<RouterListEntry>> {
            let (fail_scan, delay) = {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| DnsError::plugin("mock api lock poisoned"))?;
                state.list_entries_calls = state.list_entries_calls.saturating_add(1);
                (state.fail_healthcheck, state.list_entries_delay)
            };
            if fail_scan {
                return Err(DnsError::plugin("mock address-list scan failure"));
            }
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }

            let state = self
                .state
                .lock()
                .map_err(|_| DnsError::plugin("mock api lock poisoned"))?;
            let entries = state
                .entries
                .values()
                .filter(|entry| match entry.key.family {
                    AddressListFamily::Ipv4 => list4 == Some(entry.key.list.as_str()),
                    AddressListFamily::Ipv6 => list6 == Some(entry.key.list.as_str()),
                })
                .cloned()
                .collect::<Vec<_>>();
            drop(state);

            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::plugin("mock api lock poisoned"))?;
            if state.convert_persistent_to_dynamic_after_list {
                state.convert_persistent_to_dynamic_after_list = false;
                if let Some(entry) = state.entries.values_mut().find(|entry| {
                    decode_owned_comment("oxidns", "mk", entry.comment.as_deref())
                        .is_some_and(|meta| meta.kind == OwnedCommentKind::Persistent)
                }) {
                    entry.comment = Some(encode_comment("oxidns", "mk", OwnedCommentKind::Dynamic));
                }
            }
            if state.convert_owned_to_foreign_after_list {
                state.convert_owned_to_foreign_after_list = false;
                if let Some(entry) = state.entries.values_mut().next() {
                    entry.comment = Some("operator-owned".to_string());
                }
            }

            Ok(entries)
        }

        async fn upsert_owned_entry(
            &self,
            key: &AddressListKey,
            timeout: Option<&str>,
            comment: &str,
            comment_prefix: &str,
            plugin_tag: &str,
            refresh_timeout: bool,
        ) -> Result<Option<()>> {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::plugin("mock api lock poisoned"))?;
            if state.fail_next_upsert {
                state.fail_next_upsert = false;
                return Err(DnsError::plugin("mock upsert failure"));
            }

            let existing = state
                .entries
                .values()
                .filter(|entry| entry.key == *key)
                .cloned()
                .collect::<Vec<_>>();
            let mut owned = existing
                .iter()
                .filter(|entry| {
                    decode_owned_comment(comment_prefix, plugin_tag, entry.comment.as_deref())
                        .is_some()
                })
                .cloned()
                .collect::<Vec<_>>();
            let has_foreign = existing.len() > owned.len();
            if owned.is_empty() && has_foreign {
                return Ok(None);
            }

            if let Some(mut entry) = owned.pop() {
                let timeout_changed = entry.timeout.as_deref() != timeout;
                let comment_changed = entry.comment.as_deref() != Some(comment);
                if refresh_timeout || timeout_changed || comment_changed {
                    entry.timeout = timeout.map(str::to_string);
                    entry.comment = Some(comment.to_string());
                    state.update_ops = state.update_ops.saturating_add(1);
                    state.entries.insert(Self::storage_key(key), entry);
                }
                return Ok(Some(()));
            }

            state.next_id = state.next_id.saturating_add(1);
            let id = format!("*{}", state.next_id);
            match key.family {
                AddressListFamily::Ipv4 => state.upsert_v4 = state.upsert_v4.saturating_add(1),
                AddressListFamily::Ipv6 => state.upsert_v6 = state.upsert_v6.saturating_add(1),
            }
            state.entries.insert(
                Self::storage_key(key),
                RouterListEntry {
                    id,
                    key: key.clone(),
                    timeout: timeout.map(str::to_string),
                    comment: Some(comment.to_string()),
                },
            );
            Ok(Some(()))
        }

        async fn delete_entry_if_matches(&self, expected: &RouterListEntry) -> Result<bool> {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::plugin("mock api lock poisoned"))?;
            let key = state
                .entries
                .iter()
                .find(|(_, entry)| *entry == expected)
                .map(|(key, _)| key.clone());
            if let Some(key) = key {
                state.entries.remove(&key);
                return Ok(true);
            }
            Ok(false)
        }
    }

    fn default_cfg(tag: &str) -> AddressListManagerConfig {
        AppClock::start();
        AddressListManagerConfig {
            plugin_tag: tag.to_string(),
            address_list4: Some("oxidns_ipv4".to_string()),
            address_list6: Some("oxidns_ipv6".to_string()),
            persistent_items: AHashSet::new(),
            comment_prefix: "oxidns".to_string(),
            min_ttl: DEFAULT_MIN_TTL,
            max_ttl: DEFAULT_MAX_TTL,
            fixed_ttl: None,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
        }
    }

    fn make_context() -> DnsContext {
        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii("example.com.").unwrap(),
            RecordType::A,
            DNSClass::IN,
        ));
        DnsContext::new("127.0.0.1:5353".parse::<SocketAddr>().unwrap(), request)
    }

    fn response_with_records(records: Vec<Record>) -> Message {
        let mut resp = Message::new();
        resp.set_rcode(Rcode::NoError);
        for record in records {
            resp.answers_mut().push(record);
        }
        resp
    }

    #[test]
    fn observation_with_mismatched_response_question_is_ignored() {
        let config = MikrotikConfig {
            connection: None,
            async_mode: true,
            wait_timeout: DEFAULT_WAIT_TIMEOUT,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            address_list4: Some("oxidns_ipv4".to_string()),
            address_list6: None,
            persistent_items: AHashSet::new(),
            comment_prefix: "oxidns".to_string(),
            min_ttl: DEFAULT_MIN_TTL,
            max_ttl: DEFAULT_MAX_TTL,
            fixed_ttl: None,
            cleanup_on_shutdown: false,
        };
        let mut context = make_context();
        let mut response = response_with_records(vec![a_record(Ipv4Addr::new(192, 0, 2, 1), 60)]);
        response.add_question(Question::new(
            Name::from_ascii("other.example.").expect("name"),
            RecordType::A,
            DNSClass::IN,
        ));
        context.set_response(response);

        assert!(extract_observation(&mut context, &config).is_none());
    }

    #[test]
    fn observation_for_non_address_query_is_ignored() {
        let config = MikrotikConfig {
            connection: None,
            async_mode: true,
            wait_timeout: DEFAULT_WAIT_TIMEOUT,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            address_list4: Some("oxidns_ipv4".to_string()),
            address_list6: None,
            persistent_items: AHashSet::new(),
            comment_prefix: "oxidns".to_string(),
            min_ttl: DEFAULT_MIN_TTL,
            max_ttl: DEFAULT_MAX_TTL,
            fixed_ttl: None,
            cleanup_on_shutdown: false,
        };
        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii("example.com.").expect("name"),
            RecordType::TXT,
            DNSClass::IN,
        ));
        let mut context = DnsContext::new(
            "127.0.0.1:5353".parse::<SocketAddr>().expect("peer"),
            request,
        );
        context.set_response(response_with_records(vec![a_record(
            Ipv4Addr::new(192, 0, 2, 1),
            60,
        )]));

        assert!(extract_observation(&mut context, &config).is_none());
    }

    #[tokio::test]
    async fn dynamic_upserts_use_bounded_pipeline() {
        let api = Arc::new(PipelineMikrotikApi::default());
        let mut manager = AddressListManager::new(api.clone(), default_cfg("pipeline"));
        let addrs = (1..=17)
            .map(|last_octet| ObservedAddr {
                addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, last_octet)),
                ttl_secs: 60,
            })
            .collect();

        manager
            .observe_domain("pipeline.example.".to_string(), addrs)
            .await
            .expect("pipeline writes");

        assert_eq!(api.attempts.load(Ordering::Relaxed), 17);
        assert_eq!(api.max_active.load(Ordering::Acquire), 16);
    }

    #[tokio::test]
    async fn persistent_reconcile_uses_bounded_pipeline() {
        let api = Arc::new(PipelineMikrotikApi::default());
        let mut cfg = default_cfg("persistent-pipeline");
        cfg.persistent_items = (1..=17)
            .map(|last| {
                AddressListKey::new(
                    IpAddr::V4(Ipv4Addr::new(203, 0, 113, last)),
                    "oxidns_ipv4".to_string(),
                )
            })
            .collect();
        let mut manager = AddressListManager::new(api.clone(), cfg);

        manager.background_reconcile_for_test().await;

        assert_eq!(api.attempts.load(Ordering::Relaxed), 17);
        assert_eq!(api.max_active.load(Ordering::Acquire), 16);
    }

    #[test]
    fn observation_mailbox_keeps_distinct_addresses_from_same_domain() {
        let handle = AddressListManagerHandle::new_for_test();
        let first = vec![ObservedAddr {
            addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            ttl_secs: 60,
        }];
        let latest = vec![ObservedAddr {
            addr: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2)),
            ttl_secs: 300,
        }];

        assert!(handle.try_observe(first, None).is_ok());
        assert!(handle.try_observe(latest, None).is_ok());
        assert_eq!(handle.queued_observations(), 2);
    }

    fn a_record(ip: Ipv4Addr, ttl: u32) -> Record {
        Record::from_rdata(
            Name::from_ascii("example.com.").unwrap(),
            ttl,
            RData::A(A(ip)),
        )
    }

    fn aaaa_record(ip: Ipv6Addr, ttl: u32) -> Record {
        Record::from_rdata(
            Name::from_ascii("example.com.").unwrap(),
            ttl,
            RData::AAAA(AAAA(ip)),
        )
    }

    fn build_executor_for_test(
        tag: &str,
        async_mode: bool,
        cleanup_on_shutdown: bool,
        address_list4: Option<&str>,
        address_list6: Option<&str>,
        api: Arc<dyn MikrotikApi>,
    ) -> MikrotikExecutor {
        AppClock::start();
        let config = MikrotikConfig {
            connection: None,
            async_mode,
            wait_timeout: DEFAULT_WAIT_TIMEOUT,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            address_list4: address_list4.map(|v| v.to_string()),
            address_list6: address_list6.map(|v| v.to_string()),
            persistent_items: AHashSet::new(),
            comment_prefix: "oxidns".to_string(),
            min_ttl: DEFAULT_MIN_TTL,
            max_ttl: DEFAULT_MAX_TTL,
            fixed_ttl: None,
            cleanup_on_shutdown,
        };
        let manager_cfg = AddressListManagerConfig {
            plugin_tag: tag.to_string(),
            address_list4: config.address_list4.clone(),
            address_list6: config.address_list6.clone(),
            persistent_items: config.persistent_items.clone(),
            comment_prefix: config.comment_prefix.clone(),
            min_ttl: config.min_ttl,
            max_ttl: config.max_ttl,
            fixed_ttl: config.fixed_ttl,
            queue_capacity: config.queue_capacity,
        };
        MikrotikExecutor {
            tag: tag.to_string(),
            metrics: Arc::new(RosMetrics::new(tag.to_string())),
            config,
            manager: Some(AddressListManager::new(api, manager_cfg)),
            manager_handle: None,
            runtime: Mutex::new(None),
            queue_logs: ErrorLogThrottle::default(),
        }
    }

    async fn yield_until(description: &str, mut predicate: impl FnMut() -> bool) {
        for _ in 0..64 {
            if predicate() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("condition not met after yielding: {description}");
    }

    #[test]
    fn config_validation_requires_address_list() {
        let cfg = serde_yaml_ng::from_str::<Value>(
            r#"
address: "1.1.1.1:8728"
username: "user"
password: "pass"
"#,
        )
        .unwrap();
        let err = parse_plugin_config(Some(cfg), false).unwrap_err();
        assert!(err.to_string().contains("address_list4 or address_list6"));
    }

    #[test]
    fn config_validation_rejects_old_route_fields() {
        let cfg = serde_yaml_ng::from_str::<Value>(
            r#"
address: "1.1.1.1:8728"
username: "user"
password: "pass"
address_list4: "oxidns_ipv4"
routing_table: "oxidns_dynamic"
"#,
        )
        .unwrap();
        let err = parse_plugin_config(Some(cfg), false).unwrap_err();
        assert!(err.to_string().contains("routing_table"));
    }

    #[test]
    fn config_validation_rejects_old_persistent_route_key() {
        let cfg = serde_yaml_ng::from_str::<Value>(
            r#"
address: "1.1.1.1:8728"
username: "user"
password: "pass"
address_list4: "oxidns_ipv4"
persistent_route:
  ips:
    - "1.1.1.1"
"#,
        )
        .unwrap();
        let err = parse_plugin_config(Some(cfg), false).unwrap_err();
        assert!(err.to_string().contains("persistent_route"));
    }

    #[test]
    fn config_validation_defaults_comment_prefix() {
        let cfg = serde_yaml_ng::from_str::<Value>(
            r#"
address: "1.1.1.1:8728"
username: "user"
password: "pass"
address_list4: "oxidns_ipv4"
"#,
        )
        .unwrap();
        let parsed = parse_plugin_config(Some(cfg), false).unwrap();
        assert_eq!(DEFAULT_COMMENT_PREFIX, "oxi");
        assert_eq!(parsed.comment_prefix, "oxi");
        assert_eq!(
            parsed.connection.as_ref().expect("connection").timeouts,
            MikrotikApiTimeouts::default()
        );
    }

    #[test]
    fn config_validation_accepts_routeros_api_timeouts() {
        let cfg = serde_yaml_ng::from_str::<Value>(
            r#"
address: "1.1.1.1:8728"
username: "user"
password: "pass"
connect_timeout: 10
send_timeout: 11
receive_timeout: 60
address_list4: "oxidns_ipv4"
"#,
        )
        .unwrap();
        let parsed = parse_plugin_config(Some(cfg), false).unwrap();
        assert_eq!(
            parsed.connection.as_ref().expect("connection").timeouts,
            MikrotikApiTimeouts::from_secs(10, 11, 60)
        );
    }

    #[test]
    fn config_defaults_and_accepts_wait_and_queue_settings() {
        let defaults = serde_yaml_ng::from_str::<Value>(
            "address: 1.1.1.1:8728\nusername: user\npassword: pass\naddress_list4: policy\n",
        )
        .unwrap();
        let parsed = parse_plugin_config(Some(defaults), false).unwrap();
        assert_eq!(parsed.wait_timeout, DEFAULT_WAIT_TIMEOUT);
        assert_eq!(parsed.queue_capacity, DEFAULT_QUEUE_CAPACITY);

        let custom = serde_yaml_ng::from_str::<Value>(
            "address: 1.1.1.1:8728\nusername: user\npassword: pass\naddress_list4: policy\nwait_timeout: 1500ms\nqueue_capacity: 32\n",
        )
        .unwrap();
        let parsed = parse_plugin_config(Some(custom), false).unwrap();
        assert_eq!(parsed.wait_timeout, Duration::from_millis(1_500));
        assert_eq!(parsed.queue_capacity, 32);
    }

    #[test]
    fn config_rejects_zero_wait_and_queue_settings() {
        for invalid in ["wait_timeout: 0s", "queue_capacity: 0"] {
            let yaml = format!(
                "address: 1.1.1.1:8728\nusername: user\npassword: pass\naddress_list4: policy\n{invalid}\n"
            );
            let value = serde_yaml_ng::from_str::<Value>(&yaml).unwrap();
            assert!(parse_plugin_config(Some(value), false).is_err());
        }
    }

    #[test]
    fn config_validation_enables_verified_routeros_tls() {
        let cfg = serde_yaml_ng::from_str::<Value>(
            r#"
address: "router.example:8729"
username: "user"
password: "sensitive-credential"
tls: {}
address_list4: "oxidns_ipv4"
"#,
        )
        .unwrap();

        let parsed = parse_plugin_config(Some(cfg), false).unwrap();
        let debug = format!("{:?}", parsed.connection.expect("connection"));
        assert!(debug.contains("Secure"));
        assert!(debug.contains("router.example"));
        assert!(!debug.contains("sensitive-credential"));
    }

    #[test]
    fn config_validation_keeps_plaintext_when_tls_is_omitted() {
        let cfg = serde_yaml_ng::from_str::<Value>(
            r#"
address: "router.example:8728"
username: "user"
password: "pass"
address_list4: "oxidns_ipv4"
"#,
        )
        .unwrap();

        let parsed = parse_plugin_config(Some(cfg), false).unwrap();
        let debug = format!("{:?}", parsed.connection.expect("connection"));
        assert!(debug.contains("tls: None"));
    }

    #[test]
    fn config_validation_rejects_zero_routeros_api_timeout() {
        let cfg = serde_yaml_ng::from_str::<Value>(
            r#"
address: "1.1.1.1:8728"
username: "user"
password: "pass"
receive_timeout: 0
address_list4: "oxidns_ipv4"
"#,
        )
        .unwrap();
        let err = parse_plugin_config(Some(cfg), false).unwrap_err();
        assert!(err.to_string().contains("receive_timeout"));
    }

    #[test]
    fn config_validation_allows_zero_fixed_ttl() {
        let cfg = serde_yaml_ng::from_str::<Value>(
            r#"
address: "1.1.1.1:8728"
username: "user"
password: "pass"
address_list4: "oxidns_ipv4"
fixed_ttl: 0
"#,
        )
        .unwrap();
        let parsed = parse_plugin_config(Some(cfg), false).unwrap();
        assert_eq!(parsed.fixed_ttl, Some(0));
    }

    #[test]
    fn config_validation_ignores_persistent_item_without_family_list() {
        let cfg = serde_yaml_ng::from_str::<Value>(
            r#"
address: "1.1.1.1:8728"
username: "user"
password: "pass"
address_list4: "oxidns_ipv4"
persistent:
  ips:
    - "2001:db8::1"
"#,
        )
        .unwrap();
        let parsed = parse_plugin_config(Some(cfg), false).unwrap();
        assert!(parsed.persistent_items.is_empty());
    }

    #[test]
    fn persistent_file_content_is_loaded_and_normalized() {
        let files = parse_persistent_files(Some(vec!["persistent.txt".to_string()])).unwrap();
        let (loaded, ignored_by_family) = load_persistent_items_from_content(
            "persistent.files[0]",
            r#"
# comments are ignored
1.1.1.1
2001:db8::/64
0.0.0.0/0
"#,
            Some("oxidns_ipv4"),
            Some("oxidns_ipv6"),
        )
        .unwrap();

        assert_eq!(files, vec!["persistent.txt".to_string()]);
        assert!(loaded.contains(&AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            "oxidns_ipv4".to_string()
        )));
        assert!(
            loaded.contains(
                &AddressListKey::new_with_prefix(
                    IpAddr::V6("2001:db8::".parse().unwrap()),
                    64,
                    "oxidns_ipv6".to_string()
                )
                .unwrap()
            )
        );
        assert!(
            loaded.contains(
                &AddressListKey::new_with_prefix(
                    IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                    0,
                    "oxidns_ipv4".to_string()
                )
                .unwrap()
            )
        );
        assert_eq!(ignored_by_family, 0);
    }

    #[test]
    fn comment_codec_roundtrip() {
        let comment = encode_comment("oxidns", "mk", OwnedCommentKind::Dynamic);
        let meta = decode_owned_comment("oxidns", "mk", Some(comment.as_str())).unwrap();
        assert_eq!(meta.kind, OwnedCommentKind::Dynamic);
        assert!(!comment.contains("dm="));
    }

    #[tokio::test]
    async fn dynamic_observation_creates_address_list_entry() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut manager = AddressListManager::new(api.clone(), default_cfg("mk"));
        manager
            .observe_domain(
                "example.com".to_string(),
                vec![ObservedAddr {
                    addr: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
                    ttl_secs: 120,
                }],
            )
            .await
            .unwrap();

        let state = api.state.lock().unwrap();
        let entry = state.entries.values().next().unwrap();
        assert_eq!(entry.key.list, "oxidns_ipv4");
        assert_eq!(entry.timeout.as_deref(), Some("120s"));
    }

    #[tokio::test]
    async fn dynamic_observation_with_zero_fixed_ttl_creates_timeless_entry() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut cfg = default_cfg("mk");
        cfg.fixed_ttl = Some(0);
        let mut manager = AddressListManager::new(api.clone(), cfg);
        manager
            .observe_domain(
                "example.com".to_string(),
                vec![ObservedAddr {
                    addr: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 2)),
                    ttl_secs: 120,
                }],
            )
            .await
            .unwrap();

        let state = api.state.lock().unwrap();
        let entry = state.entries.values().next().unwrap();
        assert_eq!(entry.key.list, "oxidns_ipv4");
        assert_eq!(entry.timeout, None);
    }

    #[tokio::test]
    async fn repeated_dynamic_observation_refreshes_timeout() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut manager = AddressListManager::new(api.clone(), default_cfg("mk"));
        let observed = ObservedAddr {
            addr: IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2)),
            ttl_secs: 120,
        };
        manager
            .observe_domain("example.com".to_string(), vec![observed])
            .await
            .unwrap();
        manager
            .observe_domain(
                "example.com".to_string(),
                vec![ObservedAddr {
                    addr: observed.addr,
                    ttl_secs: 300,
                }],
            )
            .await
            .unwrap();

        let state = api.state.lock().unwrap();
        let entry = state.entries.values().next().unwrap();
        assert_eq!(entry.timeout.as_deref(), Some("300s"));
        assert!(state.update_ops >= 1);
    }

    #[tokio::test]
    async fn repeated_dynamic_observation_with_same_ttl_is_suppressed_before_refresh_window() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut manager = AddressListManager::new(api.clone(), default_cfg("mk"));
        let observed = ObservedAddr {
            addr: IpAddr::V4(Ipv4Addr::new(3, 3, 3, 3)),
            ttl_secs: 300,
        };
        manager
            .observe_domain_at_for_test("example.com".to_string(), vec![observed], 0)
            .await
            .unwrap();
        manager
            .observe_domain_at_for_test("example.com".to_string(), vec![observed], 10_000)
            .await
            .unwrap();

        let state = api.state.lock().unwrap();
        assert_eq!(state.upsert_v4, 1);
        assert_eq!(state.update_ops, 0);
    }

    #[tokio::test]
    async fn shorter_ttl_does_not_force_early_refresh() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut manager = AddressListManager::new(api.clone(), default_cfg("mk"));
        let ip = IpAddr::V4(Ipv4Addr::new(4, 4, 4, 4));
        manager
            .observe_domain_at_for_test(
                "example.com".to_string(),
                vec![ObservedAddr {
                    addr: ip,
                    ttl_secs: 300,
                }],
                0,
            )
            .await
            .unwrap();
        manager
            .observe_domain_at_for_test(
                "example.com".to_string(),
                vec![ObservedAddr {
                    addr: ip,
                    ttl_secs: 60,
                }],
                10_000,
            )
            .await
            .unwrap();

        let state = api.state.lock().unwrap();
        let entry = state.entries.values().next().unwrap();
        assert_eq!(entry.timeout.as_deref(), Some("300s"));
        assert_eq!(state.update_ops, 0);
    }

    #[tokio::test]
    async fn failed_refresh_clears_cache_and_next_observation_retries_immediately() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut manager = AddressListManager::new(api.clone(), default_cfg("mk"));
        let observed = ObservedAddr {
            addr: IpAddr::V4(Ipv4Addr::new(5, 5, 5, 5)),
            ttl_secs: 120,
        };
        manager
            .observe_domain_at_for_test("example.com".to_string(), vec![observed], 0)
            .await
            .unwrap();
        {
            let mut state = api.state.lock().unwrap();
            state.fail_next_upsert = true;
        }
        let err = manager
            .observe_domain_at_for_test("example.com".to_string(), vec![observed], 90_000)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("mock upsert failure"));
        assert_eq!(manager.dynamic_cache_len(), 0);

        manager
            .observe_domain_at_for_test("example.com".to_string(), vec![observed], 90_000)
            .await
            .unwrap();
        let state = api.state.lock().unwrap();
        assert!(state.update_ops >= 1);
    }

    #[tokio::test]
    async fn persistent_entry_is_created_without_timeout() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut cfg = default_cfg("mk");
        cfg.persistent_items.insert(
            AddressListKey::new_with_prefix(
                IpAddr::V4(Ipv4Addr::new(100, 64, 1, 0)),
                24,
                "oxidns_ipv4".to_string(),
            )
            .unwrap(),
        );
        let mut manager = AddressListManager::new(api.clone(), cfg);

        manager.reconcile().await.unwrap();

        let state = api.state.lock().unwrap();
        let entry = state.entries.values().next().unwrap();
        assert_eq!(entry.timeout, None);
        let meta = decode_owned_comment("oxidns", "mk", entry.comment.as_deref()).unwrap();
        assert_eq!(meta.kind, OwnedCommentKind::Persistent);
    }

    #[tokio::test]
    async fn unchanged_persistent_reconcile_does_not_upsert() {
        let api = Arc::new(MockMikrotikApi::default());
        let key = AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(100, 64, 1, 9)),
            "oxidns_ipv4".to_string(),
        );
        api.seed_entry(RouterListEntry {
            id: "*unchanged".to_string(),
            key: key.clone(),
            timeout: None,
            comment: Some(encode_comment("oxidns", "mk", OwnedCommentKind::Persistent)),
        });
        let mut cfg = default_cfg("mk");
        cfg.persistent_items.insert(key);
        let mut manager = AddressListManager::new(api.clone(), cfg);

        manager.reconcile().await.unwrap();

        let state = api.state.lock().unwrap();
        assert_eq!(state.upsert_v4, 0);
        assert_eq!(state.update_ops, 0);
    }

    #[tokio::test]
    async fn empty_reconcile_removes_stale_persistent_then_skips_redundant_scan() {
        let api = Arc::new(MockMikrotikApi::default());
        let key = AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(100, 64, 1, 10)),
            "oxidns_ipv4".to_string(),
        );
        api.seed_entry(RouterListEntry {
            id: "*stale-persistent".to_string(),
            key,
            timeout: None,
            comment: Some(encode_comment("oxidns", "mk", OwnedCommentKind::Persistent)),
        });
        let mut manager = AddressListManager::new(api.clone(), default_cfg("mk"));

        manager.background_reconcile_for_test().await;

        assert_eq!(api.entry_count(), 0);
        assert_eq!(api.list_entries_calls(), 1);

        manager.background_reconcile_for_test().await;
        assert_eq!(api.list_entries_calls(), 1);
    }

    #[tokio::test]
    async fn startup_reconcile_is_applied_as_soon_as_background_scan_finishes() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut cfg = default_cfg("startup-reconcile");
        cfg.persistent_items.insert(AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(100, 64, 1, 11)),
            "oxidns_ipv4".to_string(),
        ));
        let manager = AddressListManager::new(api.clone(), cfg);
        let runtime =
            AddressListManagerRuntime::start("startup-reconcile".to_string(), manager).unwrap();

        tokio::time::timeout(Duration::from_millis(500), async {
            while api.entry_count() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("startup reconcile result should be applied without waiting for a timer tick");

        runtime.shutdown(false).await.expect("shutdown");
    }

    #[tokio::test]
    async fn persistent_update_replaces_removed_entries() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut cfg = default_cfg("mk");
        cfg.persistent_items.insert(
            AddressListKey::new_with_prefix(
                IpAddr::V4(Ipv4Addr::new(100, 64, 2, 0)),
                24,
                "oxidns_ipv4".to_string(),
            )
            .unwrap(),
        );
        let mut manager = AddressListManager::new(api.clone(), cfg);
        manager.reconcile().await.unwrap();

        let mut updated = AHashSet::new();
        updated.insert(
            AddressListKey::new_with_prefix(
                IpAddr::V4(Ipv4Addr::new(100, 64, 3, 0)),
                24,
                "oxidns_ipv4".to_string(),
            )
            .unwrap(),
        );
        manager.update_persistent_items(updated).await.unwrap();

        let state = api.state.lock().unwrap();
        assert!(
            state
                .entries
                .values()
                .all(|entry| entry.key.address == IpAddr::V4(Ipv4Addr::new(100, 64, 3, 0)))
        );
    }

    #[tokio::test]
    async fn reconcile_revalidates_stale_persistent_before_delete() {
        let api = Arc::new(MockMikrotikApi::default());
        let key = AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(15, 15, 15, 15)),
            "oxidns_ipv4".to_string(),
        );
        api.seed_entry(RouterListEntry {
            id: "*401".to_string(),
            key: key.clone(),
            timeout: None,
            comment: Some(encode_comment("oxidns", "mk", OwnedCommentKind::Persistent)),
        });
        {
            let mut state = api.state.lock().unwrap();
            state.convert_persistent_to_dynamic_after_list = true;
        }

        let mut cfg = default_cfg("mk");
        cfg.persistent_items.insert(AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(15, 15, 15, 16)),
            "oxidns_ipv4".to_string(),
        ));
        let mut manager = AddressListManager::new(api.clone(), cfg);
        manager.reconcile().await.unwrap();

        let state = api.state.lock().unwrap();
        let entry = state
            .entries
            .get(&MockMikrotikApi::storage_key(&key))
            .unwrap();
        let meta = decode_owned_comment("oxidns", "mk", entry.comment.as_deref()).unwrap();
        assert_eq!(entry.id, "*401");
        assert_eq!(entry.timeout, None);
        assert_eq!(meta.kind, OwnedCommentKind::Dynamic);
    }

    #[tokio::test]
    async fn persistent_entry_wins_over_dynamic_timeout() {
        let api = Arc::new(MockMikrotikApi::default());
        let key = AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)),
            "oxidns_ipv4".to_string(),
        );
        let mut cfg = default_cfg("mk");
        cfg.persistent_items.insert(key.clone());
        let mut manager = AddressListManager::new(api.clone(), cfg);
        manager.reconcile().await.unwrap();

        manager
            .observe_domain(
                "example.com".to_string(),
                vec![ObservedAddr {
                    addr: IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)),
                    ttl_secs: 60,
                }],
            )
            .await
            .unwrap();

        let state = api.state.lock().unwrap();
        let entry = state
            .entries
            .get(&MockMikrotikApi::storage_key(&key))
            .unwrap();
        assert_eq!(entry.timeout, None);
    }

    #[tokio::test]
    async fn foreign_entry_conflict_is_left_untouched() {
        let api = Arc::new(MockMikrotikApi::default());
        let key = AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            "oxidns_ipv4".to_string(),
        );
        api.seed_entry(RouterListEntry {
            id: "*200".to_string(),
            key: key.clone(),
            timeout: Some("300s".to_string()),
            comment: Some("oxidns;pg=other;kind=dynamic;dm=foreign.example".to_string()),
        });
        let mut manager = AddressListManager::new(api.clone(), default_cfg("mk"));
        manager
            .observe_domain(
                "example.com".to_string(),
                vec![ObservedAddr {
                    addr: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
                    ttl_secs: 60,
                }],
            )
            .await
            .unwrap();

        let state = api.state.lock().unwrap();
        let entry = state
            .entries
            .get(&MockMikrotikApi::storage_key(&key))
            .unwrap();
        assert_eq!(entry.id, "*200");
        assert_eq!(entry.timeout.as_deref(), Some("300s"));
    }

    #[tokio::test]
    async fn dynamic_cache_prune_removes_expired_entries() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut manager = AddressListManager::new(api, default_cfg("mk"));
        manager
            .observe_domain_at_for_test(
                "example.com".to_string(),
                vec![ObservedAddr {
                    addr: IpAddr::V4(Ipv4Addr::new(7, 7, 7, 7)),
                    ttl_secs: 60,
                }],
                0,
            )
            .await
            .unwrap();
        assert_eq!(manager.dynamic_cache_len(), 1);

        manager
            .prune_dynamic_cache_at_for_test(61_000)
            .await
            .unwrap();
        assert_eq!(manager.dynamic_cache_len(), 0);
    }

    #[tokio::test]
    async fn dynamic_refresh_cache_has_no_record_count_limit() {
        let api = Arc::new(MockMikrotikApi::default());
        let cfg = default_cfg("cache-cap");
        let mut manager = AddressListManager::new(api, cfg);
        let addrs = (1..=3)
            .map(|last| ObservedAddr {
                addr: IpAddr::V4(Ipv4Addr::new(198, 51, 100, last)),
                ttl_secs: 300,
            })
            .collect();

        manager
            .observe_domain_at_for_test("capacity.example".to_string(), addrs, 0)
            .await
            .unwrap();

        assert_eq!(manager.dynamic_cache_len(), 3);
    }

    #[tokio::test]
    async fn reconcile_ignores_remote_dynamic_and_accepts_new_key() {
        let api = Arc::new(MockMikrotikApi::default());
        let cfg = default_cfg("over-capacity");
        let mut manager = AddressListManager::new(api.clone(), cfg);
        let first = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1));
        manager
            .observe_domain_at_for_test(
                "first.example".to_string(),
                vec![ObservedAddr {
                    addr: first,
                    ttl_secs: 300,
                }],
                0,
            )
            .await
            .unwrap();

        for last in [2, 3] {
            let key = AddressListKey::new(
                IpAddr::V4(Ipv4Addr::new(198, 51, 100, last)),
                "oxidns_ipv4".to_string(),
            );
            api.seed_entry(RouterListEntry {
                id: format!("*remote-{last}"),
                key,
                timeout: Some("300s".to_string()),
                comment: Some(encode_comment(
                    "oxidns",
                    "over-capacity",
                    OwnedCommentKind::Dynamic,
                )),
            });
        }

        manager.reconcile().await.unwrap();
        assert_eq!(manager.dynamic_cache_len(), 1);

        let rejected = AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 4)),
            "oxidns_ipv4".to_string(),
        );
        manager
            .observe_domain_at_for_test(
                "new.example".to_string(),
                vec![ObservedAddr {
                    addr: rejected.address,
                    ttl_secs: 300,
                }],
                1_000,
            )
            .await
            .unwrap();
        assert_eq!(manager.dynamic_cache_len(), 2);
        assert!(
            api.state
                .lock()
                .unwrap()
                .entries
                .contains_key(&MockMikrotikApi::storage_key(&rejected))
        );
    }

    #[tokio::test]
    async fn reconcile_ignores_manual_dynamic_deletion() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut cfg = default_cfg("manual-delete");
        cfg.fixed_ttl = Some(0);
        let mut manager = AddressListManager::new(api.clone(), cfg);
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 77));
        let key = AddressListKey::new(ip, "oxidns_ipv4".to_string());
        let observed = vec![ObservedAddr {
            addr: ip,
            ttl_secs: 300,
        }];

        manager
            .observe_domain_at_for_test("manual.example".to_string(), observed.clone(), 0)
            .await
            .unwrap();
        assert_eq!(manager.dynamic_cache_len(), 1);
        api.state
            .lock()
            .unwrap()
            .entries
            .remove(&MockMikrotikApi::storage_key(&key));

        manager.background_reconcile_for_test().await;
        assert_eq!(manager.dynamic_cache_len(), 1);

        manager
            .observe_domain_at_for_test("manual.example".to_string(), observed, 1_000)
            .await
            .unwrap();
        assert_eq!(manager.dynamic_cache_len(), 1);
        assert!(
            !api.state
                .lock()
                .unwrap()
                .entries
                .contains_key(&MockMikrotikApi::storage_key(&key))
        );
    }

    #[tokio::test]
    async fn execute_returns_next() {
        let api = Arc::new(MockMikrotikApi::default()) as Arc<dyn MikrotikApi>;
        let mut executor =
            build_executor_for_test("mk", true, false, Some("oxidns_ipv4"), None, api);
        let _ = executor.init_for_test().await;
        let mut ctx = make_context();
        let step = executor.execute(&mut ctx).await.unwrap();
        assert!(matches!(step, ExecStep::Next));
        let _ = executor.destroy().await;
    }

    #[tokio::test]
    async fn continuation_skips_unconfigured_family() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut executor = build_executor_for_test(
            "mk",
            true,
            false,
            None,
            Some("oxidns_ipv6"),
            api.clone() as Arc<dyn MikrotikApi>,
        );
        let _ = executor.init_for_test().await;
        let mut ctx = make_context();
        ctx.set_response(response_with_records(vec![
            a_record(Ipv4Addr::new(1, 1, 1, 1), 300),
            aaaa_record(Ipv6Addr::LOCALHOST, 300),
        ]));
        executor.execute_with_next(&mut ctx, None).await.unwrap();
        yield_until("ipv6 entry upsert", || {
            api.state.lock().unwrap().upsert_v6 >= 1
        })
        .await;

        {
            let state = api.state.lock().unwrap();
            assert_eq!(state.upsert_v4, 0);
            assert!(state.upsert_v6 >= 1);
        }
        let _ = executor.destroy().await;
    }

    #[tokio::test]
    async fn async_false_waits_and_keeps_dns_result_on_add_failure() {
        let api = Arc::new(MockMikrotikApi::default());
        {
            let mut state = api.state.lock().unwrap();
            state.fail_next_upsert = true;
        }
        let mut executor = build_executor_for_test(
            "mk",
            false,
            false,
            Some("oxidns_ipv4"),
            None,
            api as Arc<dyn MikrotikApi>,
        );
        let _ = executor.init_for_test().await;

        let mut ctx = make_context();
        ctx.set_response(response_with_records(vec![a_record(
            Ipv4Addr::new(10, 0, 0, 1),
            300,
        )]));
        executor.execute_with_next(&mut ctx, None).await.unwrap();
        assert!(ctx.response().is_some());
        let _ = executor.destroy().await;
    }

    #[tokio::test]
    async fn async_true_uses_background_manager() {
        let api = Arc::new(MockMikrotikApi::default());
        let mut executor = build_executor_for_test(
            "mk_async_true",
            true,
            false,
            Some("oxidns_ipv4"),
            None,
            api.clone() as Arc<dyn MikrotikApi>,
        );
        let _ = executor.init_for_test().await;
        let mut ctx = make_context();
        ctx.set_response(response_with_records(vec![a_record(
            Ipv4Addr::new(6, 6, 6, 6),
            300,
        )]));
        executor.execute_with_next(&mut ctx, None).await.unwrap();
        yield_until("background manager entry creation", || {
            api.entry_count() > 0
        })
        .await;
        assert!(api.entry_count() > 0);
        let _ = executor.destroy().await;
    }

    #[tokio::test]
    async fn startup_reconcile_failure_does_not_block_dns_execution() {
        let api = Arc::new(MockMikrotikApi::default());
        {
            let mut state = api.state.lock().unwrap();
            state.fail_healthcheck = true;
        }
        let mut executor = build_executor_for_test(
            "mk_startup",
            true,
            false,
            Some("oxidns_ipv4"),
            None,
            api.clone() as Arc<dyn MikrotikApi>,
        );
        executor.init_for_test().await.unwrap();

        let mut ctx = make_context();
        ctx.set_response(response_with_records(vec![a_record(
            Ipv4Addr::new(13, 13, 13, 13),
            300,
        )]));
        executor.execute_with_next(&mut ctx, None).await.unwrap();
        assert!(ctx.response().is_some());

        yield_until("dynamic write after startup reconcile failure", || {
            api.entry_count() > 0
        })
        .await;
        let _ = executor.destroy().await;
    }

    #[tokio::test]
    async fn startup_reconcile_scan_does_not_delay_sync_observation() {
        let api = Arc::new(MockMikrotikApi::default());
        {
            let mut state = api.state.lock().unwrap();
            state.list_entries_delay = Some(Duration::from_secs(1));
        }
        let mut executor = build_executor_for_test(
            "mk_sync_startup",
            false,
            false,
            Some("oxidns_ipv4"),
            None,
            api.clone() as Arc<dyn MikrotikApi>,
        );
        executor.init_for_test().await.unwrap();

        let mut ctx = make_context();
        ctx.set_response(response_with_records(vec![a_record(
            Ipv4Addr::new(14, 14, 14, 14),
            300,
        )]));
        tokio::time::timeout(
            Duration::from_millis(200),
            executor.execute_with_next(&mut ctx, None),
        )
        .await
        .expect("sync observation should not wait for startup reconcile scan")
        .unwrap();

        {
            let state = api.state.lock().unwrap();
            assert!(state.upsert_v4 >= 1);
        }
        let _ = executor.destroy().await;
    }

    #[tokio::test]
    async fn shutdown_cleanup_removes_only_owned_entries() {
        let api = Arc::new(MockMikrotikApi::default());
        let tag = "mk-cleanup-owned-only".to_string();
        let owned_key = AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(11, 11, 11, 11)),
            "oxidns_ipv4".to_string(),
        );
        api.seed_entry(RouterListEntry {
            id: "*301".to_string(),
            key: owned_key.clone(),
            timeout: Some("300s".to_string()),
            comment: Some(encode_comment(
                "oxidns",
                tag.as_str(),
                OwnedCommentKind::Dynamic,
            )),
        });
        api.seed_entry(RouterListEntry {
            id: "*302".to_string(),
            key: AddressListKey::new(
                IpAddr::V4(Ipv4Addr::new(12, 12, 12, 12)),
                "oxidns_ipv4".to_string(),
            ),
            timeout: Some("300s".to_string()),
            comment: Some("oxidns;pg=other;kind=dynamic;dm=foreign.example".to_string()),
        });

        let mut executor = build_executor_for_test(
            tag.as_str(),
            true,
            true,
            Some("oxidns_ipv4"),
            None,
            api.clone() as Arc<dyn MikrotikApi>,
        );
        let _ = executor.init_for_test().await;
        let _ = executor.destroy().await;

        let state = api.state.lock().unwrap();
        assert!(
            !state
                .entries
                .contains_key(&MockMikrotikApi::storage_key(&owned_key))
        );
        assert_eq!(state.entries.len(), 1);
    }

    #[tokio::test]
    async fn shutdown_cleanup_revalidates_ownership_before_delete() {
        let api = Arc::new(MockMikrotikApi::default());
        let key = AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(11, 11, 11, 13)),
            "oxidns_ipv4".to_string(),
        );
        api.seed_entry(RouterListEntry {
            id: "*ownership-race".to_string(),
            key: key.clone(),
            timeout: Some("300s".to_string()),
            comment: Some(encode_comment(
                "oxidns",
                "cleanup-race",
                OwnedCommentKind::Dynamic,
            )),
        });
        api.state
            .lock()
            .unwrap()
            .convert_owned_to_foreign_after_list = true;
        let mut manager = AddressListManager::new(api.clone(), {
            let mut cfg = default_cfg("cleanup-race");
            cfg.comment_prefix = "oxidns".to_string();
            cfg
        });

        manager.shutdown(true).await.unwrap();

        assert!(
            api.state
                .lock()
                .unwrap()
                .entries
                .contains_key(&MockMikrotikApi::storage_key(&key))
        );
    }
}
