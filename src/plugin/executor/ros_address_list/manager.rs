//! Address-list manager state machine for ros_address_list executor.
//!
//! Responsibilities:
//! - maintain desired persistent address-list entries
//! - upsert dynamic address-list entries from observed DNS answers
//! - keep ownership metadata in RouterOS comments
//! - execute idempotent create/update/delete through [`MikrotikApi`]
//!
//! Design notes:
//! - RouterOS remains the authority for dynamic expiration via native
//!   `timeout`.
//! - local state is intentionally lightweight and only suppresses redundant
//!   refresh writes; it does not attempt to mirror full remote state.
//! - persistent items are reconciled as a desired set and never enter the
//!   dynamic refresh cache.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ahash::{AHashMap, AHashSet};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use super::RosMetrics;
use super::api::{MikrotikApi, RouterListEntry};
use super::model::{
    AddressListFamily, AddressListKey, OwnedCommentKind, decode_owned_comment, encode_comment,
};
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::task as task_center;
use crate::plugin::executor::routeros::batching::join_all_bounded;
use crate::plugin::executor::routeros::completion::BatchCompletion;
use crate::plugin::executor::routeros::lease::{LeaseBook, LeaseDeadline, LeasePolicy};
use crate::plugin::executor::routeros::lifecycle::abort_and_reap;
use crate::plugin::executor::routeros::mailbox::{
    Coalesce, KeyedMailbox, PushOutcome, TryPushError,
};
use crate::plugin::executor::routeros::reconcile::{
    BackgroundReconcile, ReconcileRetry, VersionedSnapshot,
};
use crate::plugin::executor::routeros::throttle::ErrorLogThrottle;
use crate::plugin::executor::routeros::{ObservedAddr, SHUTDOWN_TIMEOUT};

/// Maximum number of distinct address-list keys waiting for manager processing.
const CONTROL_QUEUE_SIZE: usize = 2;
/// Periodic interval for persistent desired-set reconciliation.
const RECONCILE_INTERVAL_SECS: u64 = 180;
/// Periodic interval for local dynamic-cache pruning.
const DYNAMIC_CACHE_PRUNE_INTERVAL_SECS: u64 = 60;
/// Maximum number of RouterOS upserts issued concurrently by one observation.
const UPSERT_PIPELINE_SIZE: usize = 16;
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum DynamicTimeout {
    Timed(u32),
    Timeless,
}

#[derive(Debug, Clone)]
pub(super) struct AddressListManagerConfig {
    /// Plugin tag reused in RouterOS comments for ownership checks.
    pub(super) plugin_tag: String,
    /// IPv4 address-list name managed by this plugin.
    pub(super) address_list4: Option<String>,
    /// IPv6 address-list name managed by this plugin.
    pub(super) address_list6: Option<String>,
    /// Desired persistent set at startup.
    pub(super) persistent_items: AHashSet<AddressListKey>,
    /// Comment prefix used as an ownership fast-path.
    pub(super) comment_prefix: String,
    /// Minimum TTL clamp for dynamic observations.
    pub(super) min_ttl: u32,
    /// Maximum TTL clamp for dynamic observations.
    pub(super) max_ttl: u32,
    /// Optional fixed TTL override for dynamic observations.
    pub(super) fixed_ttl: Option<u32>,
    pub(super) queue_capacity: usize,
}

#[derive(Debug, Clone)]
struct AddressObservation {
    /// Absolute RouterOS timeout deadline. `None` is timeless.
    expires_at_ms: Option<u64>,
}

#[derive(Debug)]
struct AddressListSnapshot {
    entries: Vec<RouterListEntry>,
}

#[derive(Debug, Clone)]
struct ObservationCommand {
    observation: AddressObservation,
    completions: Vec<Arc<BatchCompletion>>,
}

impl Coalesce for ObservationCommand {
    fn coalesce(&mut self, mut newer: Self) {
        let keep_newer = match (
            self.observation.expires_at_ms,
            newer.observation.expires_at_ms,
        ) {
            (_, None) => true,
            (None, Some(_)) => false,
            (Some(current), Some(next)) => next >= current,
        };
        self.completions.append(&mut newer.completions);
        if keep_newer {
            self.observation = newer.observation;
        }
    }
}

#[derive(Debug, Clone)]
struct AddressObservationPolicy {
    address_list4: Option<String>,
    address_list6: Option<String>,
    lease: LeasePolicy,
}

impl AddressObservationPolicy {
    fn from_config(config: &AddressListManagerConfig) -> Self {
        Self {
            address_list4: config.address_list4.clone(),
            address_list6: config.address_list6.clone(),
            lease: LeasePolicy::new(config.min_ttl, config.max_ttl, config.fixed_ttl),
        }
    }

    fn list_for(&self, family: AddressListFamily) -> Option<&str> {
        match family {
            AddressListFamily::Ipv4 => self.address_list4.as_deref(),
            AddressListFamily::Ipv6 => self.address_list6.as_deref(),
        }
    }

    fn commands(&self, addrs: Vec<ObservedAddr>) -> Vec<(AddressListKey, AddressObservation)> {
        self.commands_at(addrs, now_millis())
    }

    fn commands_at(
        &self,
        addrs: Vec<ObservedAddr>,
        now: u64,
    ) -> Vec<(AddressListKey, AddressObservation)> {
        let mut observations = AHashMap::<AddressListKey, AddressObservation>::new();
        for observed in addrs {
            let family = AddressListFamily::from_ip(observed.addr);
            let Some(list) = self.list_for(family) else {
                continue;
            };
            let key = AddressListKey::new(observed.addr, list.to_string());
            let deadline = self.lease.deadline(observed.ttl_secs, now);
            let observation = AddressObservation {
                expires_at_ms: deadline.unix_millis(),
            };
            observations
                .entry(key)
                .and_modify(|current| {
                    let replace = match (current.expires_at_ms, observation.expires_at_ms) {
                        (_, None) => true,
                        (None, Some(_)) => false,
                        (Some(current), Some(next)) => next >= current,
                    };
                    if replace {
                        *current = observation.clone();
                    }
                })
                .or_insert(observation);
        }
        observations.into_iter().collect()
    }
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
enum ControlKey {
    Reconcile,
    PruneDynamicCache,
}

#[derive(Debug)]
enum ControlCommand {
    Reconcile,
    PruneDynamicCache,
}

impl Coalesce for ControlCommand {
    fn coalesce(&mut self, newer: Self) {
        *self = newer;
    }
}

#[derive(Debug)]
struct ShutdownRequest {
    cleanup: bool,
    done: oneshot::Sender<Result<()>>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum ObserveEnqueueError {
    Full,
    Closed,
}

#[derive(Debug, Clone)]
pub(super) struct AddressListManagerHandle {
    observations: KeyedMailbox<AddressListKey, ObservationCommand>,
    controls: KeyedMailbox<ControlKey, ControlCommand>,
    policy: AddressObservationPolicy,
    metrics: Option<Arc<RosMetrics>>,
}

impl AddressListManagerHandle {
    fn new(config: &AddressListManagerConfig, metrics: Option<Arc<RosMetrics>>) -> Self {
        Self {
            observations: KeyedMailbox::new(config.queue_capacity),
            controls: KeyedMailbox::new(CONTROL_QUEUE_SIZE),
            policy: AddressObservationPolicy::from_config(config),
            metrics,
        }
    }

    #[cfg(test)]
    pub(super) fn new_for_test() -> Self {
        AppClock::start();
        Self::new(
            &AddressListManagerConfig {
                plugin_tag: "test".to_string(),
                address_list4: Some("test_v4".to_string()),
                address_list6: Some("test_v6".to_string()),
                persistent_items: AHashSet::new(),
                comment_prefix: "fdns".to_string(),
                min_ttl: 60,
                max_ttl: 3600,
                fixed_ttl: None,
                queue_capacity: 16_384,
            },
            None,
        )
    }

    fn refresh_pending_metric_with(&self, extra: usize) {
        if let Some(metrics) = &self.metrics {
            metrics.pending_observations.store(
                self.observations.len().saturating_add(extra) as u64,
                Ordering::Relaxed,
            );
        }
    }

    fn record_outcome(&self, outcome: PushOutcome) {
        if matches!(outcome, PushOutcome::Coalesced)
            && let Some(metrics) = &self.metrics
        {
            metrics.coalesced_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn try_observe(
        &self,
        addrs: Vec<ObservedAddr>,
        wait: Option<oneshot::Sender<Result<()>>>,
    ) -> std::result::Result<PushOutcome, ObserveEnqueueError> {
        let commands = self.policy.commands(addrs);
        if commands.is_empty() {
            if let Some(waiter) = wait {
                let _ = waiter.send(Ok(()));
            }
            return Ok(PushOutcome::Inserted);
        }
        let completion = wait.map(|waiter| BatchCompletion::new(commands.len(), waiter));
        let mut outcome = PushOutcome::Coalesced;
        let mut enqueue_error = None;
        for (key, observation) in commands {
            let command = ObservationCommand {
                observation,
                completions: completion.iter().cloned().collect(),
            };
            match self.observations.try_push(key, command) {
                Ok(item_outcome @ PushOutcome::Inserted) => {
                    self.record_outcome(item_outcome);
                    outcome = PushOutcome::Inserted;
                }
                Ok(item_outcome @ PushOutcome::Coalesced) => self.record_outcome(item_outcome),
                Err(TryPushError::Full(command)) => {
                    for completion in command.completions {
                        completion.finish(&Err(DnsError::plugin(
                            "ros_address_list observation mailbox is full",
                        )));
                    }
                    enqueue_error.get_or_insert(ObserveEnqueueError::Full);
                }
                Err(TryPushError::Closed(command)) => {
                    for completion in command.completions {
                        completion.finish(&Err(DnsError::plugin(
                            "ros_address_list observation mailbox is closed",
                        )));
                    }
                    enqueue_error = Some(ObserveEnqueueError::Closed);
                }
            }
        }
        self.refresh_pending_metric_with(0);
        enqueue_error.map_or(Ok(outcome), Err)
    }

    pub(super) fn request_reconcile(&self) -> bool {
        self.controls
            .try_push(ControlKey::Reconcile, ControlCommand::Reconcile)
            .is_ok()
    }

    fn request_prune(&self) {
        let _ = self.controls.try_push(
            ControlKey::PruneDynamicCache,
            ControlCommand::PruneDynamicCache,
        );
    }

    fn close(&self) {
        self.observations.close();
        self.controls.close();
    }

    #[cfg(test)]
    pub(super) fn queued_observations(&self) -> usize {
        self.observations.len()
    }
}

#[derive(Debug)]
enum WorkerCommand {
    Observe {
        batch: Vec<(AddressListKey, ObservationCommand)>,
        from_retry: bool,
    },
    Control(ControlCommand),
    ReconcileCompleted,
}

#[derive(Debug)]
pub(super) struct AddressListManagerRuntime {
    handle: AddressListManagerHandle,
    shutdown_tx: Option<oneshot::Sender<ShutdownRequest>>,
    /// Single-owner worker task that serializes all local state transitions.
    worker_handle: Option<JoinHandle<()>>,
    /// Local-memory cache prune loop.
    prune_task_handle: Option<task_center::ManagedTaskHandle>,
    /// Periodic persistent reconcile loop.
    reconcile_task_handle: Option<task_center::ManagedTaskHandle>,
}

impl AddressListManagerRuntime {
    pub(super) fn start(tag: String, manager: AddressListManager) -> Result<Self> {
        // All mutable state lives behind one worker to avoid cross-map locking
        // or request-path synchronization in the DNS hot path.
        let has_persistent = !manager.persistent_items.is_empty();
        let handle = AddressListManagerHandle::new(&manager.cfg, manager.metrics.clone());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let worker_tag = tag.clone();
        let worker_handle_mailbox = handle.clone();
        let mut worker_handle = Some(tokio::spawn(async move {
            run_manager_worker(worker_tag, manager, worker_handle_mailbox, shutdown_rx).await;
        }));

        // Startup reconciliation is deliberately queued onto the manager worker
        // instead of awaited during plugin init. Slow RouterOS list scans must
        // not prevent the DNS service from coming up.
        handle.request_reconcile();

        // Pruning is local-memory only. It never talks to RouterOS and exists
        // solely to keep the write-suppression cache bounded.
        let prune_handle = handle.clone();
        let prune_task_handle = match task_center::spawn_fixed(
            format!("ros_address_list:{tag}:dynamic_cache_prune"),
            Duration::from_secs(DYNAMIC_CACHE_PRUNE_INTERVAL_SECS),
            task_center::TaskOptions::default(),
            move |_| {
                let prune_handle = prune_handle.clone();
                async move {
                    prune_handle.request_prune();
                }
            },
        ) {
            Ok(handle) => handle,
            Err(error) => {
                if let Some(worker) = worker_handle.take() {
                    abort_and_reap(worker);
                }
                return Err(error);
            }
        };

        // Periodic reconciliation is only a persistent desired-set safety net.
        // Dynamic entries are maintained exclusively by DNS observations and
        // RouterOS native timeout.
        let reconcile_task_handle = if has_persistent {
            let reconcile_handle = handle.clone();
            match task_center::spawn_fixed(
                format!("ros_address_list:{tag}:reconcile"),
                Duration::from_secs(RECONCILE_INTERVAL_SECS),
                task_center::TaskOptions::default(),
                move |_| {
                    let reconcile_handle = reconcile_handle.clone();
                    async move {
                        reconcile_handle.request_reconcile();
                    }
                },
            ) {
                Ok(handle) => Some(handle),
                Err(error) => {
                    prune_task_handle.stop_detached();
                    if let Some(worker) = worker_handle.take() {
                        abort_and_reap(worker);
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };

        Ok(Self {
            handle,
            shutdown_tx: Some(shutdown_tx),
            worker_handle,
            prune_task_handle: Some(prune_task_handle),
            reconcile_task_handle,
        })
    }

    #[inline]
    pub(super) fn handle(&self) -> AddressListManagerHandle {
        self.handle.clone()
    }

    pub(super) async fn shutdown(self, cleanup: bool) -> Result<()> {
        let deadline = tokio::time::Instant::now() + SHUTDOWN_TIMEOUT;
        self.shutdown_until(cleanup, deadline).await
    }

    pub(super) async fn shutdown_until(
        mut self,
        cleanup: bool,
        deadline: tokio::time::Instant,
    ) -> Result<()> {
        let tasks = [
            self.prune_task_handle.take(),
            self.reconcile_task_handle.take(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        for (index, task) in tasks.iter().enumerate() {
            if tokio::time::timeout_at(deadline, task.stop())
                .await
                .is_err()
            {
                for remaining in &tasks[index..] {
                    remaining.stop_detached();
                }
                self.handle.close();
                if let Some(worker) = self.worker_handle.take() {
                    abort_and_reap(worker);
                }
                return Err(DnsError::plugin(format!(
                    "ros_address_list shutdown exceeded {} seconds",
                    SHUTDOWN_TIMEOUT.as_secs()
                )));
            }
        }

        let (done_tx, done_rx) = oneshot::channel::<Result<()>>();
        let shutdown_requested = self.shutdown_tx.take().is_some_and(|tx| {
            tx.send(ShutdownRequest {
                cleanup,
                done: done_tx,
            })
            .is_ok()
        });
        self.handle.close();
        let result = if shutdown_requested {
            match tokio::time::timeout_at(deadline, done_rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(DnsError::plugin(
                    "ros_address_list shutdown worker closed before reporting cleanup",
                )),
                Err(_) => Err(DnsError::plugin(format!(
                    "ros_address_list shutdown exceeded {} seconds",
                    SHUTDOWN_TIMEOUT.as_secs()
                ))),
            }
        } else {
            Ok(())
        };
        if let Some(mut handle) = self.worker_handle.take()
            && tokio::time::timeout_at(deadline, &mut handle)
                .await
                .is_err()
        {
            abort_and_reap(handle);
            return Err(DnsError::plugin(format!(
                "ros_address_list shutdown exceeded {} seconds while joining worker",
                SHUTDOWN_TIMEOUT.as_secs()
            )));
        }
        result
    }
}

#[derive(Debug)]
pub(super) struct AddressListManager {
    /// RouterOS API abstraction used by the single-owner worker.
    api: Arc<dyn MikrotikApi>,
    metrics: Option<Arc<RosMetrics>>,
    /// Immutable config shared across runtime decisions.
    cfg: AddressListManagerConfig,
    /// Current desired persistent set.
    persistent_items: AHashSet<AddressListKey>,
    /// Dynamic leases and successful-write refresh suppression.
    leases: LeaseBook<AddressListKey>,
    /// Single-flight background RouterOS snapshot.
    reconcile: BackgroundReconcile<AddressListSnapshot>,
    reconcile_retry: ReconcileRetry,
    /// An empty local state still requires one successful remote scan so stale
    /// persistent rows from a previous configuration can be removed.
    startup_recovery_pending: bool,
}

impl AddressListManager {
    pub(super) fn new(api: Arc<dyn MikrotikApi>, cfg: AddressListManagerConfig) -> Self {
        Self {
            api,
            metrics: None,
            persistent_items: cfg.persistent_items.clone(),
            leases: LeaseBook::new(),
            reconcile: BackgroundReconcile::new(),
            reconcile_retry: ReconcileRetry::default(),
            startup_recovery_pending: true,
            cfg,
        }
    }

    pub(super) fn with_metrics(
        api: Arc<dyn MikrotikApi>,
        cfg: AddressListManagerConfig,
        metrics: Arc<RosMetrics>,
    ) -> Self {
        let mut manager = Self::new(api, cfg);
        manager.metrics = Some(metrics);
        manager.refresh_managed_metric();
        manager
    }

    fn refresh_managed_metric(&self) {
        if let Some(metrics) = &self.metrics {
            metrics.managed_entries.store(
                self.leases
                    .len()
                    .saturating_add(self.persistent_items.len()) as u64,
                Ordering::Relaxed,
            );
        }
    }

    async fn refresh_transport_metrics(&self) {
        let Some(metrics) = &self.metrics else {
            return;
        };
        if let Some(snapshot) = self.api.transport_snapshot().await {
            metrics
                .reconnect_total
                .store(snapshot.reconnect_total, Ordering::Relaxed);
            metrics
                .connect_attempt_total
                .store(snapshot.connect_attempt_total, Ordering::Relaxed);
            metrics
                .backoff_total
                .store(snapshot.backoff_total, Ordering::Relaxed);
            metrics
                .degraded
                .store(u64::from(snapshot.degraded), Ordering::Relaxed);
        }
    }

    #[inline]
    fn comment_for_dynamic(&self) -> String {
        encode_comment(
            self.cfg.comment_prefix.as_str(),
            self.cfg.plugin_tag.as_str(),
            OwnedCommentKind::Dynamic,
        )
    }

    #[inline]
    fn comment_for_persistent(&self) -> String {
        encode_comment(
            self.cfg.comment_prefix.as_str(),
            self.cfg.plugin_tag.as_str(),
            OwnedCommentKind::Persistent,
        )
    }

    fn should_refresh_dynamic_entry(&self, key: &AddressListKey, now_ms: u64) -> bool {
        self.leases
            .get(key)
            .is_none_or(|lease| lease.needs_sync(now_ms))
    }

    fn prune_dynamic_cache(&mut self, now_ms: u64) {
        self.leases.retain(|key, lease| {
            !lease.desired().is_expired(now_ms) && !self.persistent_items.contains(key)
        });
    }

    fn cache_dynamic_write(&mut self, key: &AddressListKey, now_ms: u64) -> bool {
        let confirmed = self.leases.confirm_synced(key, now_ms);
        self.refresh_managed_metric();
        confirmed
    }

    #[cfg(test)]
    pub(super) async fn apply_reconcile_snapshot(
        &mut self,
        existing: Vec<RouterListEntry>,
    ) -> Result<()> {
        self.apply_reconcile_snapshot_at(existing).await
    }

    async fn apply_reconcile_snapshot_at(&mut self, existing: Vec<RouterListEntry>) -> Result<()> {
        // The background task only reads RouterOS. The single state owner
        // classifies the snapshot, mutates local state, and executes the
        // resulting precise persistent diff.
        let desired_comment = self.comment_for_persistent();
        let mut owned_counts = AHashMap::<AddressListKey, usize>::new();
        for entry in &existing {
            if self.persistent_items.contains(&entry.key)
                && decode_owned_comment(
                    self.cfg.comment_prefix.as_str(),
                    self.cfg.plugin_tag.as_str(),
                    entry.comment.as_deref(),
                )
                .is_some()
            {
                *owned_counts.entry(entry.key.clone()).or_default() += 1;
            }
        }
        let correct_persistent = existing
            .iter()
            .filter(|entry| {
                self.persistent_items.contains(&entry.key)
                    && owned_counts.get(&entry.key) == Some(&1)
                    && entry.timeout.is_none()
                    && entry.comment.as_deref() == Some(desired_comment.as_str())
                    && decode_owned_comment(
                        self.cfg.comment_prefix.as_str(),
                        self.cfg.plugin_tag.as_str(),
                        entry.comment.as_deref(),
                    )
                    .is_some_and(|meta| meta.kind == OwnedCommentKind::Persistent)
            })
            .map(|entry| entry.key.clone())
            .collect::<AHashSet<_>>();
        let persistent = self
            .persistent_items
            .iter()
            .filter(|key| !correct_persistent.contains(*key))
            .collect::<Vec<_>>();
        let results = join_all_bounded(
            persistent.iter().map(|key| {
                self.api.upsert_owned_entry(
                    key,
                    None,
                    desired_comment.as_str(),
                    self.cfg.comment_prefix.as_str(),
                    self.cfg.plugin_tag.as_str(),
                    false,
                )
            }),
            UPSERT_PIPELINE_SIZE,
        )
        .await;
        let mut first_error = None;
        for (key, result) in persistent.iter().zip(results) {
            match result {
                Ok(Some(())) => {
                    if let Some(metrics) = &self.metrics {
                        metrics.write_success_total.fetch_add(1, Ordering::Relaxed);
                        metrics
                            .last_write_success_timestamp_seconds
                            .store(AppClock::now_timestamp() / 1_000, Ordering::Relaxed);
                    }
                }
                Ok(None) => {
                    warn!(
                        plugin = %self.cfg.plugin_tag,
                        list = %key.list,
                        address = %key.normalized_value(),
                        "ros_address_list persistent entry conflicts with foreign address-list entry, skipping"
                    );
                }
                Err(error) => {
                    if let Some(metrics) = &self.metrics {
                        metrics.write_error_total.fetch_add(1, Ordering::Relaxed);
                    }
                    first_error.get_or_insert(error);
                }
            }
        }
        for entry in &existing {
            let Some(meta) = decode_owned_comment(
                self.cfg.comment_prefix.as_str(),
                self.cfg.plugin_tag.as_str(),
                entry.comment.as_deref(),
            ) else {
                continue;
            };
            if meta.kind != OwnedCommentKind::Persistent {
                continue;
            }
            if self.persistent_items.contains(&entry.key) {
                continue;
            }
            if let Err(error) = self.api.delete_entry_if_matches(entry).await {
                first_error.get_or_insert(error);
            }
        }

        self.refresh_managed_metric();
        if let Some(error) = first_error {
            return Err(error);
        }
        if self.persistent_items.is_empty() {
            self.startup_recovery_pending = false;
        }
        Ok(())
    }

    fn spawn_background_reconcile(&mut self, tag: String) {
        if self.reconcile.is_running() {
            debug!(
                plugin = %tag,
                "ros_address_list reconcile already running or awaiting apply, skipping duplicate request"
            );
            return;
        }

        if self.persistent_items.is_empty() && !self.startup_recovery_pending {
            debug!(
                plugin = %tag,
                "ros_address_list reconcile already confirmed empty state, skipping remote scan"
            );
            return;
        }

        let api = self.api.clone();
        let list4 = self.cfg.address_list4.clone();
        let list6 = self.cfg.address_list6.clone();
        self.reconcile.start(0, async move {
            let entries = api.list_entries(list4.as_deref(), list6.as_deref()).await?;
            Ok(AddressListSnapshot { entries })
        });
    }

    async fn wait_for_background_reconcile(&self) {
        self.reconcile.wait().await;
    }

    #[cfg(test)]
    async fn harvest_background_reconcile(&mut self, tag: &str) {
        let Some(result) = self.reconcile.take_finished().await else {
            return;
        };
        self.apply_background_reconcile_result(tag, result).await;
    }

    async fn await_background_reconcile(&mut self, tag: &str) {
        let Some(result) = self.reconcile.take().await else {
            return;
        };
        self.apply_background_reconcile_result(tag, result).await;
    }

    async fn apply_background_reconcile_result(
        &mut self,
        tag: &str,
        result: std::result::Result<
            Result<VersionedSnapshot<AddressListSnapshot>>,
            tokio::task::JoinError,
        >,
    ) {
        match result {
            Ok(Ok(VersionedSnapshot { value, .. })) => {
                match self.apply_reconcile_snapshot_at(value.entries).await {
                    Ok(()) => {
                        self.reconcile_retry.reset();
                        if let Some(metrics) = &self.metrics {
                            metrics
                                .last_reconcile_success_timestamp_seconds
                                .store(AppClock::now_timestamp() / 1000, Ordering::Relaxed);
                        }
                        self.refresh_transport_metrics().await;
                        debug!(plugin = %tag, "ros_address_list background reconcile completed");
                    }
                    Err(error) => {
                        if let Some(metrics) = &self.metrics {
                            metrics
                                .reconcile_error_total
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        self.refresh_transport_metrics().await;
                        warn!(
                            plugin = %tag,
                            err = %error,
                            "ros_address_list background reconcile diff failed"
                        );
                        self.schedule_reconcile_retry().await;
                    }
                }
            }
            Ok(Err(error)) => {
                if let Some(metrics) = &self.metrics {
                    metrics
                        .reconcile_error_total
                        .fetch_add(1, Ordering::Relaxed);
                }
                self.refresh_transport_metrics().await;
                warn!(
                    plugin = %tag,
                    err = %error,
                    "ros_address_list background reconcile failed"
                );
                self.schedule_reconcile_retry().await;
            }
            Err(error) if error.is_cancelled() => {}
            Err(error) => {
                warn!(
                    plugin = %tag,
                    err = %error,
                    "ros_address_list background reconcile task failed"
                );
                self.schedule_reconcile_retry().await;
            }
        }
    }

    async fn schedule_reconcile_retry(&mut self) {
        self.reconcile_retry
            .schedule(self.transport_retry_delay().await);
    }

    #[cfg(test)]
    pub(super) async fn observe_domain(
        &mut self,
        _domain: String,
        addrs: Vec<ObservedAddr>,
    ) -> Result<()> {
        self.observe_at_for_test(addrs, now_millis()).await
    }

    async fn observe_address_batch(
        &mut self,
        observations: &[(AddressListKey, AddressObservation)],
    ) -> Vec<Result<()>> {
        self.observe_address_batch_at(observations, now_millis())
            .await
    }

    async fn observe_address_batch_at(
        &mut self,
        observations: &[(AddressListKey, AddressObservation)],
        now: u64,
    ) -> Vec<Result<()>> {
        self.prune_dynamic_cache(now);

        struct Prepared {
            index: usize,
            key: AddressListKey,
            timeout: DynamicTimeout,
            timeout_value: Option<String>,
            comment: String,
        }

        let mut outcomes = std::iter::repeat_with(|| None)
            .take(observations.len())
            .collect::<Vec<Option<Result<()>>>>();
        let mut prepared = Vec::new();
        for (index, (key, observation)) in observations.iter().enumerate() {
            if self.persistent_items.contains(key) {
                outcomes[index] = Some(Ok(()));
                continue;
            }
            let deadline = observation
                .expires_at_ms
                .map_or(LeaseDeadline::Timeless, LeaseDeadline::At);
            if deadline.is_expired(now) {
                outcomes[index] = Some(Ok(()));
                continue;
            }
            self.leases.observe(key.clone(), deadline, now);
            let timeout = deadline
                .remaining_secs(now)
                .map_or(DynamicTimeout::Timeless, DynamicTimeout::Timed);
            if !self.should_refresh_dynamic_entry(key, now) {
                outcomes[index] = Some(Ok(()));
                continue;
            }
            prepared.push(Prepared {
                index,
                key: key.clone(),
                timeout,
                timeout_value: match timeout {
                    DynamicTimeout::Timed(ttl) => Some(format!("{ttl}s")),
                    DynamicTimeout::Timeless => None,
                },
                comment: self.comment_for_dynamic(),
            });
        }

        let api = self.api.clone();
        let prefix = self.cfg.comment_prefix.clone();
        let plugin_tag = self.cfg.plugin_tag.clone();
        let results = join_all_bounded(
            prepared.iter().map(|item| {
                api.upsert_owned_entry(
                    &item.key,
                    item.timeout_value.as_deref(),
                    &item.comment,
                    &prefix,
                    &plugin_tag,
                    matches!(item.timeout, DynamicTimeout::Timed(_)),
                )
            }),
            UPSERT_PIPELINE_SIZE,
        )
        .await;

        for (item, result) in prepared.into_iter().zip(results) {
            outcomes[item.index] = Some(match result {
                Ok(Some(())) => {
                    if let Some(metrics) = &self.metrics {
                        metrics.write_success_total.fetch_add(1, Ordering::Relaxed);
                        metrics
                            .last_write_success_timestamp_seconds
                            .store(AppClock::now_timestamp() / 1_000, Ordering::Relaxed);
                    }
                    self.cache_dynamic_write(&item.key, now);
                    Ok(())
                }
                Ok(None) => {
                    self.leases.remove(&item.key);
                    Ok(())
                }
                Err(error) => {
                    if let Some(metrics) = &self.metrics {
                        metrics.write_error_total.fetch_add(1, Ordering::Relaxed);
                    }
                    self.leases.remove(&item.key);
                    Err(error)
                }
            });
        }

        outcomes
            .into_iter()
            .map(|outcome| outcome.unwrap_or_else(|| Ok(())))
            .collect()
    }

    #[cfg(test)]
    pub(super) async fn update_persistent_items(
        &mut self,
        items: AHashSet<AddressListKey>,
    ) -> Result<()> {
        // Persistent ownership takes precedence over any cached dynamic state.
        self.persistent_items = items;
        self.startup_recovery_pending = true;
        self.prune_dynamic_cache(now_millis());
        let entries = self
            .api
            .list_entries(
                self.cfg.address_list4.as_deref(),
                self.cfg.address_list6.as_deref(),
            )
            .await?;
        self.apply_reconcile_snapshot(entries).await
    }

    #[cfg(test)]
    pub(super) async fn reconcile(&mut self) -> Result<()> {
        self.prune_dynamic_cache(now_millis());
        if self.persistent_items.is_empty() && !self.startup_recovery_pending {
            return Ok(());
        }
        let entries = self
            .api
            .list_entries(
                self.cfg.address_list4.as_deref(),
                self.cfg.address_list6.as_deref(),
            )
            .await?;
        self.apply_reconcile_snapshot(entries).await
    }

    pub(super) async fn prune_dynamic_cache_now(&mut self) -> Result<()> {
        self.prune_dynamic_cache(now_millis());
        self.refresh_managed_metric();
        Ok(())
    }

    async fn transport_retry_delay(&self) -> Option<Duration> {
        let snapshot = self.api.transport_snapshot().await;
        if let (Some(metrics), Some(snapshot)) = (&self.metrics, snapshot) {
            metrics
                .reconnect_total
                .store(snapshot.reconnect_total, Ordering::Relaxed);
            metrics
                .connect_attempt_total
                .store(snapshot.connect_attempt_total, Ordering::Relaxed);
            metrics
                .backoff_total
                .store(snapshot.backoff_total, Ordering::Relaxed);
            metrics
                .degraded
                .store(u64::from(snapshot.degraded), Ordering::Relaxed);
            snapshot.retry_after
        } else {
            snapshot.and_then(|snapshot| snapshot.retry_after)
        }
    }

    async fn cleanup_entry_if_still_owned(&self, entry: &RouterListEntry) -> Result<()> {
        self.api.delete_entry_if_matches(entry).await?;
        Ok(())
    }

    pub(super) async fn shutdown(&mut self, cleanup: bool) -> Result<()> {
        self.reconcile.cancel().await;

        if !cleanup {
            self.leases.clear();
            return Ok(());
        }

        // Cleanup bypasses reconnect backoff but retains per-operation
        // transport timeouts.
        self.api.begin_shutdown_cleanup();
        // Cleanup only touches entries that match this plugin's comment
        // ownership.
        let entries = self
            .api
            .list_entries(
                self.cfg.address_list4.as_deref(),
                self.cfg.address_list6.as_deref(),
            )
            .await?;
        let owned = entries
            .into_iter()
            .filter(|entry| {
                decode_owned_comment(
                    self.cfg.comment_prefix.as_str(),
                    self.cfg.plugin_tag.as_str(),
                    entry.comment.as_deref(),
                )
                .is_some()
            })
            .collect::<Vec<_>>();
        let results = join_all_bounded(
            owned
                .iter()
                .map(|entry| self.cleanup_entry_if_still_owned(entry)),
            UPSERT_PIPELINE_SIZE,
        )
        .await;
        let mut first_error = None;
        let mut failures = 0u64;
        for result in results {
            if let Err(error) = result {
                failures += 1;
                first_error.get_or_insert(error);
            }
        }
        if failures > 0 {
            if let Some(metrics) = &self.metrics {
                metrics
                    .cleanup_error_total
                    .fetch_add(failures, Ordering::Relaxed);
            }
            warn!(plugin = %self.cfg.plugin_tag, failures, "ros_address_list shutdown cleanup completed with failures");
        }
        self.leases.clear();
        self.refresh_managed_metric();
        self.refresh_transport_metrics().await;
        first_error.map_or(Ok(()), Err)
    }

    #[cfg(test)]
    pub(super) fn dynamic_cache_len(&self) -> usize {
        self.leases.len()
    }

    #[cfg(test)]
    pub(super) async fn observe_domain_at_for_test(
        &mut self,
        _domain: String,
        addrs: Vec<ObservedAddr>,
        now_ms: u64,
    ) -> Result<()> {
        self.observe_at_for_test(addrs, now_ms).await
    }

    #[cfg(test)]
    async fn observe_at_for_test(&mut self, addrs: Vec<ObservedAddr>, now_ms: u64) -> Result<()> {
        let observations =
            AddressObservationPolicy::from_config(&self.cfg).commands_at(addrs, now_ms);
        self.observe_address_batch_at(&observations, now_ms)
            .await
            .into_iter()
            .find_map(std::result::Result::err)
            .map_or(Ok(()), Err)
    }

    #[cfg(test)]
    pub(super) async fn background_reconcile_for_test(&mut self) {
        let tag = self.cfg.plugin_tag.clone();
        self.spawn_background_reconcile(tag.clone());
        while self.reconcile.is_running() && !self.reconcile.is_finished() {
            tokio::task::yield_now().await;
        }
        self.harvest_background_reconcile(tag.as_str()).await;
    }

    #[cfg(test)]
    pub(super) async fn prune_dynamic_cache_at_for_test(&mut self, now_ms: u64) -> Result<()> {
        self.prune_dynamic_cache(now_ms);
        Ok(())
    }
}

async fn run_manager_worker(
    tag: String,
    mut manager: AddressListManager,
    handle: AddressListManagerHandle,
    mut shutdown_rx: oneshot::Receiver<ShutdownRequest>,
) {
    // Every state transition is serialized here. Request-path code only pushes
    // commands into the mailbox and never mutates manager state directly.
    let error_logs = ErrorLogThrottle::default();
    let mut retry_observations =
        AHashMap::<AddressListKey, (tokio::time::Instant, ObservationCommand)>::new();
    loop {
        let next_retry = retry_observations
            .values()
            .map(|(retry_at, _)| *retry_at)
            .min();
        let retry_wakeup = async {
            match next_retry {
                Some(retry_at) => tokio::time::sleep_until(retry_at).await,
                None => std::future::pending::<()>().await,
            }
        };
        let reconcile_retry_at = manager.reconcile_retry.deadline();
        let reconcile_retry_wakeup = async move {
            match reconcile_retry_at {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending::<()>().await,
            }
        };
        let command = tokio::select! {
            biased;
            shutdown = &mut shutdown_rx => {
                if let Ok(ShutdownRequest { cleanup, done }) = shutdown {
                    let _ = done.send(manager.shutdown(cleanup).await);
                }
                break;
            }
            () = manager.wait_for_background_reconcile() => {
                Some(WorkerCommand::ReconcileCompleted)
            }
            control = handle.controls.recv() => control.map(|(_, command)| WorkerCommand::Control(command)),
            () = retry_wakeup => {
                let now = tokio::time::Instant::now();
                let due_keys = retry_observations
                    .iter()
                    .filter(|(_, (retry_at, _))| *retry_at <= now)
                    .take(UPSERT_PIPELINE_SIZE)
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>();
                let due = due_keys
                    .into_iter()
                    .filter_map(|key| {
                        retry_observations
                            .remove(&key)
                            .map(|(_, mut command)| {
                                if let Some(newer) = handle.observations.take(&key) {
                                    command.coalesce(newer);
                                }
                                (key, command)
                            })
                    })
                    .collect::<Vec<_>>();
                (!due.is_empty()).then_some(WorkerCommand::Observe {
                    batch: due,
                    from_retry: true,
                })
            }
            () = reconcile_retry_wakeup => {
                manager.reconcile_retry.mark_due();
                Some(WorkerCommand::Control(ControlCommand::Reconcile))
            }
            observation = handle.observations.recv() => {
                observation.map(|first| {
                    let mut batch = vec![first];
                    while batch.len() < UPSERT_PIPELINE_SIZE {
                        let Some(next) = handle.observations.try_recv() else {
                            break;
                        };
                        batch.push(next);
                    }
                    WorkerCommand::Observe {
                        batch,
                        from_retry: false,
                    }
                })
            }
        };
        let Some(command) = command else {
            break;
        };
        match command {
            WorkerCommand::Observe {
                mut batch,
                from_retry,
            } => {
                if !from_retry
                    && let Some(retry_at) = retry_observations
                        .values()
                        .map(|(retry_at, _)| *retry_at)
                        .min()
                {
                    for (key, command) in batch.drain(..) {
                        defer_address_observation(
                            &mut retry_observations,
                            manager.cfg.queue_capacity,
                            retry_at,
                            key,
                            command,
                            handle.metrics.as_deref(),
                        );
                    }
                    handle.refresh_pending_metric_with(retry_observations.len());
                    continue;
                }
                let observations = batch
                    .iter()
                    .map(|(key, command)| (key.clone(), command.observation.clone()))
                    .collect::<Vec<_>>();
                let results = manager.observe_address_batch(&observations).await;
                let has_error = results.iter().any(|result| result.is_err());
                let retry_delay = if has_error {
                    manager.transport_retry_delay().await
                } else {
                    None
                };
                for ((key, mut command), result) in batch.drain(..).zip(results) {
                    for completion in command.completions.drain(..) {
                        completion.finish(&result);
                    }
                    if let Err(error) = &result {
                        if error_logs.should_log("observe") {
                            warn!(
                                plugin = %tag,
                                err = %error,
                                "ros_address_list observe failed in async mode"
                            );
                        }
                        if let Some(delay) = retry_delay {
                            defer_address_observation(
                                &mut retry_observations,
                                manager.cfg.queue_capacity,
                                tokio::time::Instant::now() + delay,
                                key,
                                command,
                                handle.metrics.as_deref(),
                            );
                        }
                    }
                }
            }
            WorkerCommand::Control(command) => match command {
                ControlCommand::Reconcile => {
                    manager.spawn_background_reconcile(tag.clone());
                }
                ControlCommand::PruneDynamicCache => {
                    if let Err(e) = manager.prune_dynamic_cache_now().await
                        && error_logs.should_log("prune")
                    {
                        warn!(
                            plugin = %tag,
                            err = %e,
                            "ros_address_list dynamic cache prune failed"
                        );
                    }
                }
            },
            WorkerCommand::ReconcileCompleted => {
                manager.await_background_reconcile(tag.as_str()).await;
            }
        }
        handle.refresh_pending_metric_with(retry_observations.len());
    }

    debug!(plugin = %tag, "ros_address_list manager worker exited");
}

fn defer_address_observation(
    retries: &mut AHashMap<AddressListKey, (tokio::time::Instant, ObservationCommand)>,
    capacity: usize,
    retry_at: tokio::time::Instant,
    key: AddressListKey,
    command: ObservationCommand,
    metrics: Option<&RosMetrics>,
) {
    if let Some((scheduled_at, existing)) = retries.get_mut(&key) {
        *scheduled_at = (*scheduled_at).min(retry_at);
        existing.coalesce(command);
        if let Some(metrics) = metrics {
            metrics.coalesced_total.fetch_add(1, Ordering::Relaxed);
        }
        return;
    }
    if retries.len() < capacity {
        retries.insert(key, (retry_at, command));
        return;
    }

    let error = Err(DnsError::plugin(
        "ros_address_list retry observation capacity reached",
    ));
    for completion in command.completions {
        completion.finish(&error);
    }
    if let Some(metrics) = metrics {
        metrics.dropped_total.fetch_add(1, Ordering::Relaxed);
    }
}

fn now_millis() -> u64 {
    AppClock::elapsed_millis()
}

#[cfg(test)]
mod observation_tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;

    #[derive(Debug, Default)]
    struct DuplicateApi {
        entries: Mutex<Vec<RouterListEntry>>,
    }

    #[async_trait]
    impl MikrotikApi for DuplicateApi {
        async fn list_entries(
            &self,
            _list4: Option<&str>,
            _list6: Option<&str>,
        ) -> Result<Vec<RouterListEntry>> {
            Ok(self.entries.lock().expect("entries").clone())
        }

        async fn upsert_owned_entry(
            &self,
            key: &AddressListKey,
            timeout: Option<&str>,
            comment: &str,
            comment_prefix: &str,
            plugin_tag: &str,
            _refresh_timeout: bool,
        ) -> Result<Option<()>> {
            let mut entries = self.entries.lock().expect("entries");
            let canonical = entries
                .iter()
                .find(|entry| {
                    entry.key == *key
                        && decode_owned_comment(
                            comment_prefix,
                            plugin_tag,
                            entry.comment.as_deref(),
                        )
                        .is_some()
                })
                .map(|entry| entry.id.clone());
            let Some(canonical) = canonical else {
                entries.push(RouterListEntry {
                    id: "*added".to_string(),
                    key: key.clone(),
                    timeout: timeout.map(str::to_string),
                    comment: Some(comment.to_string()),
                });
                return Ok(Some(()));
            };
            entries.retain(|entry| {
                entry.key != *key
                    || entry.id == canonical
                    || decode_owned_comment(comment_prefix, plugin_tag, entry.comment.as_deref())
                        .is_none()
            });
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == canonical)
                .expect("canonical entry");
            entry.timeout = timeout.map(str::to_string);
            entry.comment = Some(comment.to_string());
            Ok(Some(()))
        }

        async fn delete_entry_if_matches(&self, expected: &RouterListEntry) -> Result<bool> {
            let mut entries = self.entries.lock().expect("entries");
            let Some(index) = entries.iter().position(|entry| entry == expected) else {
                return Ok(false);
            };
            entries.remove(index);
            Ok(true)
        }
    }

    fn duplicate_config() -> AddressListManagerConfig {
        AppClock::start();
        AddressListManagerConfig {
            plugin_tag: "duplicate-test".to_string(),
            address_list4: Some("policy".to_string()),
            address_list6: None,
            persistent_items: AHashSet::new(),
            comment_prefix: "oxi".to_string(),
            min_ttl: 1,
            max_ttl: 3_600,
            fixed_ttl: None,
            queue_capacity: 16_384,
        }
    }

    fn list_entry(
        id: &str,
        key: &AddressListKey,
        timeout: Option<&str>,
        kind: OwnedCommentKind,
    ) -> RouterListEntry {
        RouterListEntry {
            id: id.to_string(),
            key: key.clone(),
            timeout: timeout.map(str::to_string),
            comment: Some(encode_comment("oxi", "duplicate-test", kind)),
        }
    }

    #[tokio::test]
    async fn reconcile_removes_duplicate_correct_persistent_entries() {
        let api = Arc::new(DuplicateApi::default());
        let key = AddressListKey::new(
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 21)),
            "policy".to_string(),
        );
        let first = list_entry("*first", &key, None, OwnedCommentKind::Persistent);
        let second = list_entry("*second", &key, None, OwnedCommentKind::Persistent);
        api.entries
            .lock()
            .expect("entries")
            .extend([first.clone(), second.clone()]);
        let mut config = duplicate_config();
        config.persistent_items.insert(key);
        let mut manager = AddressListManager::new(api.clone(), config);

        manager
            .apply_reconcile_snapshot_at(vec![first, second])
            .await
            .expect("reconcile");

        assert_eq!(api.entries.lock().expect("entries").len(), 1);
    }

    #[tokio::test]
    async fn mailbox_is_bounded_by_address_list_key() {
        let handle = AddressListManagerHandle::new_for_test();
        let addr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));
        for index in 0..10_000 {
            handle
                .try_observe(
                    vec![ObservedAddr {
                        addr,
                        ttl_secs: 60 + (index % 300) as u32,
                    }],
                    None,
                )
                .expect("coalesced observation");
        }

        assert_eq!(handle.observations.len(), 1);
        let (key, command) = handle.observations.recv().await.expect("observation");
        assert_eq!(key.address, addr);
        // The coalesced value keeps the longest absolute expiry.
        assert!(command.observation.expires_at_ms.is_some());
    }

    #[tokio::test]
    async fn timeless_observation_cannot_be_replaced_by_timed_observation() {
        AppClock::start();
        let mut config = AddressListManagerConfig {
            plugin_tag: "test".to_string(),
            address_list4: Some("test_v4".to_string()),
            address_list6: None,
            persistent_items: AHashSet::new(),
            comment_prefix: "fdns".to_string(),
            min_ttl: 60,
            max_ttl: 3600,
            fixed_ttl: Some(0),
            queue_capacity: 16_384,
        };
        let handle = AddressListManagerHandle::new(&config, None);
        let addr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 11));
        handle
            .try_observe(vec![ObservedAddr { addr, ttl_secs: 60 }], None)
            .expect("timeless observation");

        config.fixed_ttl = Some(300);
        let (key, observation) = AddressObservationPolicy::from_config(&config)
            .commands(vec![ObservedAddr { addr, ttl_secs: 60 }])
            .pop()
            .expect("timed command");
        handle
            .observations
            .try_push(
                key,
                ObservationCommand {
                    observation,
                    completions: Vec::new(),
                },
            )
            .expect("coalesced timed observation");

        let (_, command) = handle.observations.recv().await.expect("observation");
        assert_eq!(command.observation.expires_at_ms, None);
    }
}
