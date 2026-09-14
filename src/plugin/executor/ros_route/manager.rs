//! Route manager for DNS-observed RouterOS route leases.
//!
//! Dynamic state is keyed only by the destination host route. DNS answers add
//! or extend leases; absence from a later answer never withdraws a route.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use ahash::{AHashMap, AHashSet};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use super::RosRouteMetrics;
use super::api::{MikrotikApi, RouterRoute};
use super::model::{
    RouteCommentCodec, RouteCommentKind, RouteCommentMeta, RouteFamily, RouteKey,
    is_validation_comment,
};
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::task as task_center;
use crate::plugin::executor::routeros::batching::join_all_bounded;
use crate::plugin::executor::routeros::completion::BatchCompletion;
use crate::plugin::executor::routeros::ip_prefix::IpPrefix;
use crate::plugin::executor::routeros::lease::{
    LeaseBook, LeaseDeadline, LeasePolicy, ROUTE_MAX_REFRESH_INTERVAL_MS,
};
use crate::plugin::executor::routeros::lifecycle::abort_and_reap;
use crate::plugin::executor::routeros::mailbox::{
    Coalesce, KeyedMailbox, PushOutcome, TryPushError,
};
use crate::plugin::executor::routeros::reconcile::{
    BackgroundReconcile, ReconcileRetry, VersionedSnapshot,
};
use crate::plugin::executor::routeros::throttle::ErrorLogThrottle;
use crate::plugin::executor::routeros::{ObservedAddr, SHUTDOWN_TIMEOUT};

const ROUTE_DEFAULT_V4: &str = "0.0.0.0/0";
const ROUTE_DEFAULT_V6: &str = "::/0";
const CONTROL_QUEUE_SIZE: usize = 2;
const SWEEP_INTERVAL_SECS: u64 = 30;
const RECONCILE_INTERVAL_SECS: u64 = 180;
const CONNECTION_GUARD_RETRY_INTERVAL_SECS: u64 = SWEEP_INTERVAL_SECS;
const CONNECTION_QUERY_BATCH_SIZE: usize = 128;
const UPSERT_PIPELINE_SIZE: usize = 16;

#[derive(Debug, Clone)]
pub(super) struct RouteManagerConfig {
    pub(super) plugin_tag: String,
    pub(super) routing_table: String,
    pub(super) gateway4: Option<String>,
    pub(super) gateway6: Option<String>,
    pub(super) persistent_ips: AHashSet<IpPrefix>,
    pub(super) comment_prefix: String,
    pub(super) distance: u8,
    pub(super) min_ttl: u32,
    pub(super) max_ttl: u32,
    pub(super) fixed_ttl: Option<u32>,
    pub(super) conntrack_guard: bool,
    pub(super) queue_capacity: usize,
}

fn deadline_is_later(candidate: LeaseDeadline, current: LeaseDeadline) -> bool {
    match (candidate, current) {
        (LeaseDeadline::Timeless, LeaseDeadline::At(_)) => true,
        (LeaseDeadline::At(candidate), LeaseDeadline::At(current)) => candidate > current,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SyncState {
    PendingCreate,
    Synced,
    Dirty,
    PendingDynamicDelete,
    PendingPersistentDelete,
}

#[derive(Debug, Clone)]
struct RouteState {
    gateway: String,
    distance: u8,
    router_id: Option<String>,
    sync_state: SyncState,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ReconcileMode {
    StartupRecovery,
    Persistent,
}

#[derive(Debug)]
struct RouteSnapshot {
    mode: ReconcileMode,
    routes: Vec<RouterRoute>,
}

#[derive(Debug, Clone)]
struct RouteObservation {
    deadline: LeaseDeadline,
    observed_at_ms: u64,
    completions: Vec<Arc<BatchCompletion>>,
}

impl Coalesce for RouteObservation {
    fn coalesce(&mut self, mut newer: Self) {
        newer.deadline = self.deadline.max(newer.deadline);
        newer.observed_at_ms = newer.observed_at_ms.max(self.observed_at_ms);
        newer.completions.append(&mut self.completions);
        *self = newer;
    }
}

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
enum ControlKey {
    Sweep,
    Reconcile,
}

#[derive(Debug)]
enum ControlCommand {
    Sweep,
    Reconcile,
}

impl Coalesce for ControlCommand {
    fn coalesce(&mut self, newer: Self) {
        *self = newer;
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum ObserveEnqueueError {
    Full,
    Closed,
}

#[derive(Debug, Clone)]
pub(super) struct RouteManagerHandle {
    observations: KeyedMailbox<RouteKey, RouteObservation>,
    controls: KeyedMailbox<ControlKey, ControlCommand>,
    routing_table: String,
    gateway4_enabled: bool,
    gateway6_enabled: bool,
    policy: LeasePolicy,
    metrics: Option<Arc<RosRouteMetrics>>,
}

impl RouteManagerHandle {
    fn new(config: &RouteManagerConfig, metrics: Option<Arc<RosRouteMetrics>>) -> Self {
        Self {
            observations: KeyedMailbox::new(config.queue_capacity),
            controls: KeyedMailbox::new(CONTROL_QUEUE_SIZE),
            routing_table: config.routing_table.clone(),
            gateway4_enabled: config.gateway4.is_some(),
            gateway6_enabled: config.gateway6.is_some(),
            policy: LeasePolicy::new(config.min_ttl, config.max_ttl, config.fixed_ttl),
            metrics,
        }
    }

    fn prepare(&self, addrs: Vec<ObservedAddr>) -> Vec<(RouteKey, LeaseDeadline, u64)> {
        let now = now_millis();
        let mut dedup = AHashMap::<RouteKey, LeaseDeadline>::new();
        for observed in addrs {
            let enabled = match observed.addr {
                IpAddr::V4(_) => self.gateway4_enabled,
                IpAddr::V6(_) => self.gateway6_enabled,
            };
            if !enabled {
                continue;
            }
            let key = RouteKey::new(observed.addr, self.routing_table.clone());
            let deadline = self.policy.deadline(observed.ttl_secs, now);
            dedup
                .entry(key)
                .and_modify(|current| *current = current.max(deadline))
                .or_insert(deadline);
        }
        dedup
            .into_iter()
            .map(|(key, deadline)| (key, deadline, now))
            .collect()
    }

    fn finish_enqueue_metric(&self, outcome: PushOutcome) {
        if matches!(outcome, PushOutcome::Coalesced)
            && let Some(metrics) = &self.metrics
        {
            metrics
                .coalesced_total
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.refresh_pending_metric();
    }

    fn refresh_pending_metric(&self) {
        self.refresh_pending_metric_with(0);
    }

    fn refresh_pending_metric_with(&self, extra: usize) {
        if let Some(metrics) = &self.metrics {
            metrics.pending_observations.store(
                self.observations.len().saturating_add(extra) as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
    }

    pub(super) fn try_observe(
        &self,
        addrs: Vec<ObservedAddr>,
        wait: Option<oneshot::Sender<Result<()>>>,
    ) -> std::result::Result<PushOutcome, ObserveEnqueueError> {
        let prepared = self.prepare(addrs);
        if prepared.is_empty() {
            if let Some(waiter) = wait {
                let _ = waiter.send(Ok(()));
            }
            return Ok(PushOutcome::Inserted);
        }
        let completion = wait.map(|waiter| BatchCompletion::new(prepared.len(), waiter));
        let mut total = PushOutcome::Coalesced;
        let mut error = None;
        for (key, deadline, observed_at_ms) in prepared {
            let command = RouteObservation {
                deadline,
                observed_at_ms,
                completions: completion.iter().cloned().collect(),
            };
            match self.observations.try_push(key, command) {
                Ok(outcome) => {
                    self.finish_enqueue_metric(outcome);
                    if matches!(outcome, PushOutcome::Inserted) {
                        total = outcome;
                    }
                }
                Err(TryPushError::Full(command)) => {
                    let result = Err(DnsError::plugin("ros_route observation mailbox is full"));
                    for completion in command.completions {
                        completion.finish(&result);
                    }
                    error.get_or_insert(ObserveEnqueueError::Full);
                }
                Err(TryPushError::Closed(command)) => {
                    let result = Err(DnsError::plugin("ros_route observation mailbox is closed"));
                    for completion in command.completions {
                        completion.finish(&result);
                    }
                    error = Some(ObserveEnqueueError::Closed);
                }
            }
        }
        error.map_or(Ok(total), Err)
    }

    pub(super) fn request_reconcile(&self) -> bool {
        self.controls
            .try_push(ControlKey::Reconcile, ControlCommand::Reconcile)
            .is_ok()
    }

    fn request_sweep(&self) {
        let _ = self
            .controls
            .try_push(ControlKey::Sweep, ControlCommand::Sweep);
    }

    fn close(&self) {
        self.observations.close();
        self.controls.close();
    }
}

#[derive(Debug)]
struct ShutdownRequest {
    cleanup: bool,
    done: oneshot::Sender<Result<()>>,
}

#[derive(Debug)]
pub(super) struct RouteManagerRuntime {
    handle: RouteManagerHandle,
    shutdown_tx: Option<oneshot::Sender<ShutdownRequest>>,
    worker_handle: Option<JoinHandle<()>>,
    sweep_task_handle: Option<task_center::ManagedTaskHandle>,
    reconcile_task_handle: Option<task_center::ManagedTaskHandle>,
}

impl RouteManagerRuntime {
    pub(super) fn start(tag: String, manager: RouteManager) -> Result<Self> {
        let has_persistent = !manager.persistent.is_empty();
        let handle = RouteManagerHandle::new(&manager.cfg, manager.metrics.clone());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let mut worker_handle = Some(tokio::spawn(run_manager_worker(
            tag.clone(),
            manager,
            handle.clone(),
            shutdown_rx,
        )));
        handle.request_reconcile();

        let sweep_handle = handle.clone();
        let sweep_task_handle = match task_center::spawn_fixed(
            format!("ros_route:{tag}:sweep"),
            Duration::from_secs(SWEEP_INTERVAL_SECS),
            task_center::TaskOptions::default(),
            move |_| {
                let sweep_handle = sweep_handle.clone();
                async move { sweep_handle.request_sweep() }
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
        let reconcile_task_handle = if has_persistent {
            let reconcile_handle = handle.clone();
            match task_center::spawn_fixed(
                format!("ros_route:{tag}:reconcile"),
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
                    sweep_task_handle.stop_detached();
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
            sweep_task_handle: Some(sweep_task_handle),
            reconcile_task_handle,
        })
    }

    pub(super) fn handle(&self) -> RouteManagerHandle {
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
            self.sweep_task_handle.take(),
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
                    "ros_route shutdown exceeded {} seconds",
                    SHUTDOWN_TIMEOUT.as_secs()
                )));
            }
        }
        let (done_tx, done_rx) = oneshot::channel();
        let requested = self.shutdown_tx.take().is_some_and(|sender| {
            sender
                .send(ShutdownRequest {
                    cleanup,
                    done: done_tx,
                })
                .is_ok()
        });
        self.handle.close();
        let result = if requested {
            match tokio::time::timeout_at(deadline, done_rx).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err(DnsError::plugin(
                    "ros_route shutdown worker closed before reporting cleanup",
                )),
                Err(_) => Err(DnsError::plugin(format!(
                    "ros_route shutdown exceeded {} seconds",
                    SHUTDOWN_TIMEOUT.as_secs()
                ))),
            }
        } else {
            Ok(())
        };
        if let Some(mut worker) = self.worker_handle.take() {
            match tokio::time::timeout_at(deadline, &mut worker).await {
                Ok(_) => {}
                Err(_) => {
                    abort_and_reap(worker);
                    return Err(DnsError::plugin(format!(
                        "ros_route shutdown exceeded {} seconds while joining worker",
                        SHUTDOWN_TIMEOUT.as_secs()
                    )));
                }
            }
        }
        result
    }
}

#[derive(Debug)]
pub(super) struct RouteManager {
    api: Arc<dyn MikrotikApi>,
    cfg: RouteManagerConfig,
    metrics: Option<Arc<RosRouteMetrics>>,
    persistent: AHashSet<RouteKey>,
    leases: LeaseBook<RouteKey>,
    routes: AHashMap<RouteKey, RouteState>,
    connection_retry_after: AHashMap<RouteKey, u64>,
    reconcile: BackgroundReconcile<RouteSnapshot>,
    reconcile_retry: ReconcileRetry,
    startup_recovery_pending: bool,
    initialized: bool,
}

impl RouteManager {
    pub(super) fn new(api: Arc<dyn MikrotikApi>, cfg: RouteManagerConfig) -> Self {
        let persistent = cfg
            .persistent_ips
            .iter()
            .map(|prefix| RouteKey {
                ip: prefix.address(),
                prefix: prefix.prefix(),
                table: cfg.routing_table.clone(),
            })
            .collect();
        Self {
            api,
            metrics: None,
            persistent,
            leases: LeaseBook::new(),
            routes: AHashMap::new(),
            connection_retry_after: AHashMap::new(),
            reconcile: BackgroundReconcile::new(),
            reconcile_retry: ReconcileRetry::default(),
            startup_recovery_pending: true,
            initialized: false,
            cfg,
        }
    }

    pub(super) fn with_metrics(
        api: Arc<dyn MikrotikApi>,
        cfg: RouteManagerConfig,
        metrics: Arc<RosRouteMetrics>,
    ) -> Self {
        let mut manager = Self::new(api, cfg);
        manager.metrics = Some(metrics);
        manager.refresh_managed_metric();
        manager
    }

    fn policy(&self) -> LeasePolicy {
        LeasePolicy::new(self.cfg.min_ttl, self.cfg.max_ttl, self.cfg.fixed_ttl)
    }

    fn gateway_for(&self, family: RouteFamily) -> Option<&str> {
        match family {
            RouteFamily::Ipv4 => self.cfg.gateway4.as_deref(),
            RouteFamily::Ipv6 => self.cfg.gateway6.as_deref(),
        }
    }

    fn refresh_managed_metric(&self) {
        if let Some(metrics) = &self.metrics {
            metrics.managed_entries.store(
                self.persistent.len().saturating_add(self.leases.len()) as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
    }

    async fn refresh_transport_metrics(&self) {
        let Some(metrics) = &self.metrics else { return };
        let Some(snapshot) = self.api.transport_snapshot().await else {
            return;
        };
        metrics.reconnect_total.store(
            snapshot.reconnect_total,
            std::sync::atomic::Ordering::Relaxed,
        );
        metrics.connect_attempt_total.store(
            snapshot.connect_attempt_total,
            std::sync::atomic::Ordering::Relaxed,
        );
        metrics
            .backoff_total
            .store(snapshot.backoff_total, std::sync::atomic::Ordering::Relaxed);
        metrics.degraded.store(
            u64::from(snapshot.degraded),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    async fn ensure_initialized(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }
        for key in self.persistent.clone() {
            let Some(gateway) = self.gateway_for(key.family()).map(str::to_string) else {
                continue;
            };
            self.routes.entry(key).or_insert(RouteState {
                gateway,
                distance: self.cfg.distance,
                router_id: None,
                sync_state: SyncState::PendingCreate,
            });
        }
        self.initialized = true;
        Ok(())
    }

    #[cfg(test)]
    async fn observe_key(&mut self, key: RouteKey, observation: &RouteObservation) -> Result<()> {
        self.observe_batch(&[(key, observation.clone())]).await
    }

    async fn observe_batch(&mut self, observations: &[(RouteKey, RouteObservation)]) -> Result<()> {
        self.ensure_initialized().await?;
        let mut keys = Vec::with_capacity(observations.len());
        for (key, observation) in observations {
            if self.stage_observation(key.clone(), observation) {
                keys.push(key.clone());
            }
        }
        self.sync_keys(keys, now_millis()).await
    }

    fn stage_observation(&mut self, key: RouteKey, observation: &RouteObservation) -> bool {
        if self.persistent.contains(&key) {
            return false;
        }
        self.leases.observe(
            key.clone(),
            observation.deadline,
            observation.observed_at_ms,
        );
        let Some(gateway) = self.gateway_for(key.family()).map(str::to_string) else {
            self.leases.remove(&key);
            return false;
        };
        let state = self.routes.entry(key.clone()).or_insert(RouteState {
            gateway: gateway.clone(),
            distance: self.cfg.distance,
            router_id: None,
            sync_state: SyncState::PendingCreate,
        });
        state.gateway = gateway;
        state.distance = self.cfg.distance;
        if matches!(
            state.sync_state,
            SyncState::PendingDynamicDelete | SyncState::PendingPersistentDelete
        ) {
            state.sync_state = if state.router_id.is_some() {
                SyncState::Dirty
            } else {
                SyncState::PendingCreate
            };
        }
        self.connection_retry_after.remove(&key);
        true
    }

    async fn sync_keys(&mut self, keys: Vec<RouteKey>, now_ms: u64) -> Result<()> {
        let mut upserts = Vec::new();
        let mut deletes = Vec::new();
        for key in keys {
            let Some(state) = self.routes.get(&key).cloned() else {
                continue;
            };
            match state.sync_state {
                SyncState::PendingDynamicDelete | SyncState::PendingPersistentDelete => {
                    deletes.push(key);
                }
                _ if self.persistent.contains(&key) => {
                    if !matches!(state.sync_state, SyncState::Synced) {
                        upserts.push((
                            key,
                            state,
                            RouteCommentCodec::encode_persistent(
                                &self.cfg.comment_prefix,
                                &self.cfg.plugin_tag,
                            ),
                        ));
                    }
                }
                _ => {
                    let Some(lease) = self.leases.get(&key).copied() else {
                        continue;
                    };
                    if lease.desired().is_expired(now_ms) {
                        if let Some(state) = self.routes.get_mut(&key) {
                            state.sync_state = SyncState::PendingDynamicDelete;
                        }
                        deletes.push(key);
                    } else if !matches!(state.sync_state, SyncState::Synced)
                        || lease.needs_sync(now_ms)
                    {
                        upserts.push((
                            key,
                            state,
                            RouteCommentCodec::encode_dynamic(
                                &self.cfg.comment_prefix,
                                &self.cfg.plugin_tag,
                                lease.desired(),
                                lease.last_observed_ms(),
                            ),
                        ));
                    }
                }
            }
        }

        let api = self.api.clone();
        let prefix = self.cfg.comment_prefix.clone();
        let tag = self.cfg.plugin_tag.clone();
        let results = join_all_bounded(
            upserts.iter().map(|(key, state, comment)| {
                api.upsert_host_route(key, &state.gateway, state.distance, comment, &prefix, &tag)
            }),
            UPSERT_PIPELINE_SIZE,
        )
        .await;
        let mut first_error = None;
        for ((key, _, _), result) in upserts.into_iter().zip(results) {
            match result {
                Ok(router_id) => {
                    if let Some(metrics) = &self.metrics {
                        metrics
                            .write_success_total
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        metrics.last_write_success_timestamp_seconds.store(
                            AppClock::now_timestamp() / 1_000,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                    }
                    if let Some(state) = self.routes.get_mut(&key) {
                        state.router_id = Some(router_id);
                        state.sync_state = SyncState::Synced;
                    }
                    if !self.persistent.contains(&key) {
                        self.leases.confirm_synced_with_max_interval(
                            &key,
                            now_ms,
                            Some(ROUTE_MAX_REFRESH_INTERVAL_MS),
                        );
                    }
                }
                Err(error) => {
                    if let Some(metrics) = &self.metrics {
                        metrics
                            .write_error_total
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    first_error.get_or_insert(error);
                }
            }
        }
        self.sync_deletes(deletes, now_ms, &mut first_error).await;
        self.refresh_managed_metric();
        first_error.map_or(Ok(()), Err)
    }

    async fn sync_deletes(
        &mut self,
        keys: Vec<RouteKey>,
        now_ms: u64,
        first_error: &mut Option<DnsError>,
    ) {
        let mut dynamic = AHashMap::<RouteFamily, Vec<(RouteKey, Vec<RouterRoute>)>>::new();
        let mut immediate = Vec::new();
        for key in keys {
            let Some(state) = self.routes.get(&key) else {
                continue;
            };
            let pending_dynamic = matches!(state.sync_state, SyncState::PendingDynamicDelete);
            if pending_dynamic
                && self.cfg.conntrack_guard
                && self
                    .connection_retry_after
                    .get(&key)
                    .is_some_and(|retry| *retry > now_ms)
            {
                continue;
            }
            match self
                .api
                .find_routes(&key, &self.cfg.comment_prefix, &self.cfg.plugin_tag)
                .await
            {
                Ok(routes) if routes.is_empty() => self.forget_deleted(&key),
                Ok(routes) if pending_dynamic => {
                    dynamic.entry(key.family()).or_default().push((key, routes));
                }
                Ok(routes) => immediate.push((key, routes)),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        for (key, routes) in immediate {
            match self.delete_routes_if_still_owned(&routes).await {
                Ok(()) => self.forget_deleted(&key),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        for (family, candidates) in dynamic {
            let active = if self.cfg.conntrack_guard {
                let mut active = AHashSet::new();
                let mut failed = false;
                for chunk in candidates.chunks(CONNECTION_QUERY_BATCH_SIZE) {
                    let destinations = chunk.iter().map(|(key, _)| key.ip).collect::<Vec<_>>();
                    match self
                        .api
                        .connection_destinations(family, &destinations)
                        .await
                    {
                        Ok(found) => active.extend(found),
                        Err(error) => {
                            failed = true;
                            first_error.get_or_insert(error);
                            break;
                        }
                    }
                }
                if failed {
                    if let Some(metrics) = &self.metrics {
                        metrics
                            .connection_check_error_total
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    for (key, _) in candidates {
                        self.defer_connection_check(&key, now_ms);
                    }
                    continue;
                }
                active
            } else {
                AHashSet::new()
            };
            for (key, routes) in candidates {
                if active.contains(&key.ip) {
                    self.defer_connection_check(&key, now_ms);
                    if let Some(metrics) = &self.metrics {
                        metrics
                            .delete_deferred_total
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    continue;
                }
                match self.delete_routes_if_still_owned(&routes).await {
                    Ok(()) => self.forget_deleted(&key),
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
        }
    }

    fn defer_connection_check(&mut self, key: &RouteKey, now_ms: u64) {
        self.connection_retry_after.insert(
            key.clone(),
            now_ms.saturating_add(CONNECTION_GUARD_RETRY_INTERVAL_SECS * 1_000),
        );
    }

    fn forget_deleted(&mut self, key: &RouteKey) {
        self.routes.remove(key);
        self.leases.remove(key);
        self.connection_retry_after.remove(key);
    }

    fn discard_unsynced_observation(&mut self, key: &RouteKey) {
        if self.leases.get(key).is_some_and(|lease| lease.has_synced()) {
            if let Some(route) = self.routes.get_mut(key) {
                route.sync_state = SyncState::Dirty;
            }
            return;
        }
        self.forget_deleted(key);
    }

    async fn sweep(&mut self) -> Result<()> {
        self.ensure_initialized().await?;
        self.harvest_reconcile().await;
        let now = now_millis();
        let expired = self.leases.expired_keys(now);
        for key in &expired {
            if let Some(state) = self.routes.get_mut(key) {
                state.sync_state = SyncState::PendingDynamicDelete;
            } else {
                self.leases.remove(key);
            }
        }
        let pending = self
            .routes
            .iter()
            .filter(|(_, state)| {
                matches!(
                    state.sync_state,
                    SyncState::PendingDynamicDelete | SyncState::PendingPersistentDelete
                )
            })
            .map(|(key, _)| key.clone())
            .collect();
        self.sync_keys(pending, now).await
    }

    async fn start_reconcile(&mut self) -> Result<()> {
        self.ensure_initialized().await?;
        if self.reconcile.is_running() {
            return Ok(());
        }
        let mode = if self.startup_recovery_pending {
            ReconcileMode::StartupRecovery
        } else {
            ReconcileMode::Persistent
        };
        if mode == ReconcileMode::Persistent && self.persistent.is_empty() {
            return Ok(());
        }
        let api = self.api.clone();
        let table = self.cfg.routing_table.clone();
        let require_ipv4 = self.cfg.gateway4.is_some();
        let require_ipv6 = self.cfg.gateway6.is_some();
        self.reconcile.start(self.leases.revision(), async move {
            let routes = api
                .list_managed_routes(&table, require_ipv4, require_ipv6)
                .await?;
            Ok(RouteSnapshot { mode, routes })
        });
        Ok(())
    }

    async fn harvest_reconcile(&mut self) {
        let Some(result) = self.reconcile.take_finished().await else {
            return;
        };
        match result {
            Ok(Ok(snapshot)) => match self.apply_reconcile_result(snapshot).await {
                Ok(()) => {
                    self.reconcile_retry.reset();
                    if let Some(metrics) = &self.metrics {
                        metrics.last_reconcile_success_timestamp_seconds.store(
                            AppClock::now_timestamp() / 1_000,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                    }
                    self.refresh_transport_metrics().await;
                }
                Err(error) => self.record_reconcile_error(error).await,
            },
            Ok(Err(error)) => self.record_reconcile_error(error).await,
            Err(error) if error.is_cancelled() => {}
            Err(error) => {
                self.record_reconcile_error(DnsError::plugin(format!(
                    "ros_route reconcile task failed: {error}"
                )))
                .await
            }
        }
    }

    async fn apply_reconcile_result(
        &mut self,
        snapshot: VersionedSnapshot<RouteSnapshot>,
    ) -> Result<()> {
        let VersionedSnapshot { generation, value } = snapshot;
        match value.mode {
            ReconcileMode::StartupRecovery => {
                self.apply_snapshot(VersionedSnapshot {
                    generation,
                    value: value.routes,
                })
                .await?;
                self.startup_recovery_pending = false;
                Ok(())
            }
            ReconcileMode::Persistent => self.apply_persistent_snapshot(value.routes).await,
        }
    }

    async fn record_reconcile_error(&mut self, error: DnsError) {
        if let Some(metrics) = &self.metrics {
            metrics
                .reconcile_error_total
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        warn!(plugin = %self.cfg.plugin_tag, err = %error, "ros_route reconcile failed");
        self.reconcile_retry
            .schedule(self.transport_retry_delay().await);
    }

    async fn apply_snapshot(
        &mut self,
        snapshot: VersionedSnapshot<Vec<RouterRoute>>,
    ) -> Result<()> {
        let now = now_millis();
        let mut owned = AHashMap::<RouteKey, Vec<(RouterRoute, RouteCommentMeta)>>::new();
        let mut first_error = None;
        for route in snapshot.value {
            if is_default_route_dst(&route.dst_address) {
                continue;
            }
            let Ok(prefix) = route.dst_address.parse::<IpPrefix>() else {
                continue;
            };
            let key = RouteKey {
                ip: prefix.address(),
                prefix: prefix.prefix(),
                table: self.cfg.routing_table.clone(),
            };
            let Some(comment) = route.comment.as_deref() else {
                continue;
            };
            if is_validation_comment(&self.cfg.comment_prefix, &self.cfg.plugin_tag, comment) {
                if let Err(error) = self.delete_route_if_still_owned(&route).await {
                    first_error.get_or_insert(error);
                }
                continue;
            }
            let meta = match RouteCommentCodec::decode(
                &self.cfg.comment_prefix,
                &self.cfg.plugin_tag,
                route.family,
                &route.dst_address,
                comment,
            ) {
                Ok(Some(meta)) => meta,
                _ => continue,
            };
            if self.gateway_for(key.family()).is_none() {
                if let Err(error) = self.delete_route_if_still_owned(&route).await {
                    first_error.get_or_insert(error);
                }
                self.forget_deleted(&key);
                continue;
            }
            owned.entry(key).or_default().push((route, meta));
        }

        let mut seen = AHashSet::new();
        for (key, mut candidates) in owned {
            seen.insert(key.clone());
            let Some(gateway) = self.gateway_for(key.family()).map(str::to_string) else {
                continue;
            };
            if self.persistent.contains(&key) {
                let expected = RouteCommentCodec::encode_persistent(
                    &self.cfg.comment_prefix,
                    &self.cfg.plugin_tag,
                );
                let canonical = candidates
                    .iter()
                    .position(|(route, meta)| {
                        meta.kind == RouteCommentKind::Persistent
                            && route.gateway.as_deref() == Some(gateway.as_str())
                            && route.distance == Some(self.cfg.distance)
                            && !route.disabled
                            && route.comment.as_deref() == Some(expected.as_str())
                    })
                    .unwrap_or_default();
                let (route, meta) = candidates.swap_remove(canonical);
                for (duplicate, _) in candidates {
                    if let Err(error) = self.delete_route_if_still_owned(&duplicate).await {
                        first_error.get_or_insert(error);
                    }
                }
                let dirty = meta.kind != RouteCommentKind::Persistent
                    || route.gateway.as_deref() != Some(gateway.as_str())
                    || route.distance != Some(self.cfg.distance)
                    || route.disabled
                    || route.comment.as_deref() != Some(expected.as_str());
                self.routes.insert(
                    key,
                    RouteState {
                        gateway,
                        distance: self.cfg.distance,
                        router_id: Some(route.id),
                        sync_state: if dirty {
                            SyncState::Dirty
                        } else {
                            SyncState::Synced
                        },
                    },
                );
                continue;
            }

            let newer = self
                .leases
                .get(&key)
                .is_some_and(|lease| lease.desired_revision() > snapshot.generation);
            let mut recovered_deadline = None;
            let mut recovered_seen = 0;
            let mut canonical_dynamic = None;
            for (index, (_, meta)) in candidates.iter().enumerate() {
                if meta.kind != RouteCommentKind::Dynamic {
                    continue;
                }
                let deadline = self
                    .policy()
                    .cap_recovered(meta.expires_at_ms, meta.last_seen_ms);
                if recovered_deadline.is_none_or(|current| deadline_is_later(deadline, current)) {
                    recovered_deadline = Some(deadline);
                    canonical_dynamic = Some(index);
                }
                recovered_seen = recovered_seen.max(meta.last_seen_ms);
            }

            if let Some(canonical) = canonical_dynamic {
                let (route, _) = candidates.swap_remove(canonical);
                for (duplicate, _) in candidates {
                    if let Err(error) = self.delete_route_if_still_owned(&duplicate).await {
                        first_error.get_or_insert(error);
                    }
                }
                let deadline = recovered_deadline.expect("dynamic candidate has a deadline");
                if deadline.is_expired(now) && !newer {
                    self.routes.insert(
                        key,
                        RouteState {
                            gateway,
                            distance: self.cfg.distance,
                            router_id: Some(route.id),
                            sync_state: SyncState::PendingDynamicDelete,
                        },
                    );
                    continue;
                }
                if !newer {
                    self.leases.recover(
                        key.clone(),
                        deadline,
                        recovered_seen,
                        snapshot.generation,
                        now,
                        Some(ROUTE_MAX_REFRESH_INTERVAL_MS),
                    );
                }
                let lease = self.leases.get(&key).copied();
                let expected = lease.map(|lease| {
                    RouteCommentCodec::encode_dynamic(
                        &self.cfg.comment_prefix,
                        &self.cfg.plugin_tag,
                        lease.desired(),
                        lease.last_observed_ms(),
                    )
                });
                let dirty = newer
                    || route.gateway.as_deref() != Some(gateway.as_str())
                    || route.distance != Some(self.cfg.distance)
                    || route.disabled
                    || expected.as_deref() != route.comment.as_deref();
                self.routes.insert(
                    key,
                    RouteState {
                        gateway,
                        distance: self.cfg.distance,
                        router_id: Some(route.id),
                        sync_state: if dirty {
                            SyncState::Dirty
                        } else {
                            SyncState::Synced
                        },
                    },
                );
                continue;
            }

            let (route, meta) = candidates.swap_remove(0);
            for (duplicate, _) in candidates {
                if let Err(error) = self.delete_route_if_still_owned(&duplicate).await {
                    first_error.get_or_insert(error);
                }
            }
            if meta.kind == RouteCommentKind::Persistent {
                if self
                    .leases
                    .get(&key)
                    .is_some_and(|lease| !lease.desired().is_expired(now))
                {
                    self.routes.insert(
                        key,
                        RouteState {
                            gateway,
                            distance: self.cfg.distance,
                            router_id: Some(route.id),
                            sync_state: SyncState::Dirty,
                        },
                    );
                    continue;
                }
                self.routes.insert(
                    key,
                    RouteState {
                        gateway,
                        distance: self.cfg.distance,
                        router_id: Some(route.id),
                        sync_state: SyncState::PendingPersistentDelete,
                    },
                );
                continue;
            }
        }

        let local_keys = self.routes.keys().cloned().collect::<Vec<_>>();
        for key in local_keys {
            if seen.contains(&key) {
                continue;
            }
            if self.persistent.contains(&key) {
                if let Some(state) = self.routes.get_mut(&key) {
                    state.router_id = None;
                    state.sync_state = SyncState::PendingCreate;
                }
                continue;
            }
            let newer = self
                .leases
                .get(&key)
                .is_some_and(|lease| lease.desired_revision() > snapshot.generation);
            if newer {
                if let Some(state) = self.routes.get_mut(&key) {
                    state.router_id = None;
                    state.sync_state = SyncState::PendingCreate;
                }
            } else {
                self.forget_deleted(&key);
            }
        }
        let keys = self.routes.keys().cloned().collect();
        if let Err(error) = self.sync_keys(keys, now).await {
            first_error.get_or_insert(error);
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn apply_persistent_snapshot(&mut self, rows: Vec<RouterRoute>) -> Result<()> {
        let now = now_millis();
        let mut desired = AHashMap::<RouteKey, Vec<(RouterRoute, RouteCommentMeta)>>::new();
        let mut stale = AHashMap::<RouteKey, Vec<RouterRoute>>::new();

        for route in rows {
            if is_default_route_dst(&route.dst_address) {
                continue;
            }
            let Ok(prefix) = route.dst_address.parse::<IpPrefix>() else {
                continue;
            };
            let key = RouteKey {
                ip: prefix.address(),
                prefix: prefix.prefix(),
                table: self.cfg.routing_table.clone(),
            };
            let Some(comment) = route.comment.as_deref() else {
                continue;
            };
            let Ok(Some(meta)) = RouteCommentCodec::decode(
                &self.cfg.comment_prefix,
                &self.cfg.plugin_tag,
                route.family,
                &route.dst_address,
                comment,
            ) else {
                continue;
            };
            if self.persistent.contains(&key) {
                desired.entry(key).or_default().push((route, meta));
            } else if meta.kind == RouteCommentKind::Persistent {
                stale.entry(key).or_default().push(route);
            }
        }

        let mut first_error = None;
        let mut sync = Vec::new();
        for key in self.persistent.clone() {
            let Some(gateway) = self.gateway_for(key.family()).map(str::to_string) else {
                continue;
            };
            let expected = RouteCommentCodec::encode_persistent(
                &self.cfg.comment_prefix,
                &self.cfg.plugin_tag,
            );
            let mut candidates = desired.remove(&key).unwrap_or_default();
            if candidates.is_empty() {
                self.routes.insert(
                    key.clone(),
                    RouteState {
                        gateway,
                        distance: self.cfg.distance,
                        router_id: None,
                        sync_state: SyncState::PendingCreate,
                    },
                );
                sync.push(key);
                continue;
            }
            let canonical = candidates
                .iter()
                .position(|(route, meta)| {
                    meta.kind == RouteCommentKind::Persistent
                        && route.gateway.as_deref() == Some(gateway.as_str())
                        && route.distance == Some(self.cfg.distance)
                        && !route.disabled
                        && route.comment.as_deref() == Some(expected.as_str())
                })
                .unwrap_or_default();
            let (route, meta) = candidates.swap_remove(canonical);
            for (duplicate, _) in candidates {
                if let Err(error) = self.delete_route_if_still_owned(&duplicate).await {
                    first_error.get_or_insert(error);
                }
            }
            let dirty = meta.kind != RouteCommentKind::Persistent
                || route.gateway.as_deref() != Some(gateway.as_str())
                || route.distance != Some(self.cfg.distance)
                || route.disabled
                || route.comment.as_deref() != Some(expected.as_str());
            self.routes.insert(
                key.clone(),
                RouteState {
                    gateway,
                    distance: self.cfg.distance,
                    router_id: Some(route.id),
                    sync_state: if dirty {
                        SyncState::Dirty
                    } else {
                        SyncState::Synced
                    },
                },
            );
            if dirty {
                sync.push(key);
            }
        }

        for (key, mut routes) in stale {
            if self
                .leases
                .get(&key)
                .is_some_and(|lease| !lease.desired().is_expired(now))
            {
                let route = routes.swap_remove(0);
                for duplicate in routes {
                    if let Err(error) = self.delete_route_if_still_owned(&duplicate).await {
                        first_error.get_or_insert(error);
                    }
                }
                if let Some(gateway) = self.gateway_for(key.family()).map(str::to_string) {
                    self.routes.insert(
                        key.clone(),
                        RouteState {
                            gateway,
                            distance: self.cfg.distance,
                            router_id: Some(route.id),
                            sync_state: SyncState::Dirty,
                        },
                    );
                    sync.push(key);
                }
            } else {
                if let Err(error) = self.delete_routes_if_still_owned(&routes).await {
                    first_error.get_or_insert(error);
                }
                self.forget_deleted(&key);
            }
        }

        if let Err(error) = self.sync_keys(sync, now).await {
            first_error.get_or_insert(error);
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn transport_retry_delay(&self) -> Option<Duration> {
        self.api
            .transport_snapshot()
            .await
            .and_then(|snapshot| snapshot.retry_after)
    }

    async fn delete_route_if_still_owned(&self, route: &RouterRoute) -> Result<bool> {
        let expected_comment = route.comment.as_deref().unwrap_or_default();
        let owned = if is_validation_comment(
            &self.cfg.comment_prefix,
            &self.cfg.plugin_tag,
            expected_comment,
        ) {
            true
        } else {
            RouteCommentCodec::decode(
                &self.cfg.comment_prefix,
                &self.cfg.plugin_tag,
                route.family,
                &route.dst_address,
                expected_comment,
            )
            .ok()
            .flatten()
            .is_some()
        };
        if !owned {
            return Ok(false);
        }
        self.api.delete_route_if_matches(route).await
    }

    async fn delete_routes_if_still_owned(&self, routes: &[RouterRoute]) -> Result<()> {
        let mut first_error = None;
        for route in routes {
            if let Err(error) = self.delete_route_if_still_owned(route).await {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn cleanup_route_if_still_owned(&self, route: &RouterRoute) -> Result<()> {
        self.delete_route_if_still_owned(route).await.map(|_| ())
    }

    async fn shutdown(&mut self, cleanup: bool) -> Result<()> {
        self.reconcile.cancel().await;
        if !cleanup {
            self.leases.clear();
            self.routes.clear();
            return Ok(());
        }
        self.api.begin_shutdown_cleanup();
        let rows = self
            .api
            .list_managed_routes(
                &self.cfg.routing_table,
                self.cfg.gateway4.is_some(),
                self.cfg.gateway6.is_some(),
            )
            .await?;
        let owned = rows
            .into_iter()
            .filter(|route| {
                route.comment.as_deref().is_some_and(|comment| {
                    is_validation_comment(&self.cfg.comment_prefix, &self.cfg.plugin_tag, comment)
                        || matches!(
                            RouteCommentCodec::decode(
                                &self.cfg.comment_prefix,
                                &self.cfg.plugin_tag,
                                route.family,
                                &route.dst_address,
                                comment,
                            ),
                            Ok(Some(_))
                        )
                })
            })
            .collect::<Vec<_>>();
        let results = join_all_bounded(
            owned
                .iter()
                .map(|route| self.cleanup_route_if_still_owned(route)),
            UPSERT_PIPELINE_SIZE,
        )
        .await;
        let failures = results.iter().filter(|result| result.is_err()).count();
        if failures > 0
            && let Some(metrics) = &self.metrics
        {
            metrics
                .cleanup_error_total
                .fetch_add(failures as u64, std::sync::atomic::Ordering::Relaxed);
        }
        self.leases.clear();
        self.routes.clear();
        results
            .into_iter()
            .find_map(std::result::Result::err)
            .map_or(Ok(()), Err)
    }
}

async fn run_manager_worker(
    tag: String,
    mut manager: RouteManager,
    handle: RouteManagerHandle,
    mut shutdown_rx: oneshot::Receiver<ShutdownRequest>,
) {
    let error_logs = ErrorLogThrottle::default();
    let mut retries = AHashMap::<RouteKey, (tokio::time::Instant, RouteObservation)>::new();
    loop {
        manager.harvest_reconcile().await;
        let next_retry = retries.values().map(|(at, _)| *at).min();
        let retry_wakeup = async {
            match next_retry {
                Some(at) => tokio::time::sleep_until(at).await,
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
        enum Event {
            Observe {
                batch: Vec<(RouteKey, RouteObservation)>,
                from_retry: bool,
            },
            Control(ControlCommand),
            ReconcileCompleted,
        }
        let event = tokio::select! {
            biased;
            shutdown = &mut shutdown_rx => {
                if let Ok(ShutdownRequest { cleanup, done }) = shutdown {
                    let _ = done.send(manager.shutdown(cleanup).await);
                }
                break;
            }
            _ = manager.reconcile.wait(), if manager.reconcile.is_running() => Some(Event::ReconcileCompleted),
            control = handle.controls.recv() => control.map(|(_, command)| Event::Control(command)),
            _ = retry_wakeup => {
                let now = tokio::time::Instant::now();
                let keys = retries
                    .iter()
                    .filter(|(_, (at, _))| *at <= now)
                    .take(UPSERT_PIPELINE_SIZE)
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>();
                let due = keys
                    .into_iter()
                    .filter_map(|key| {
                        retries.remove(&key).map(|(_, mut command)| {
                            if let Some(newer) = handle.observations.take(&key) {
                                command.coalesce(newer);
                            }
                            (key, command)
                        })
                    })
                    .collect::<Vec<_>>();
                (!due.is_empty()).then_some(Event::Observe {
                    batch: due,
                    from_retry: true,
                })
            }
            _ = reconcile_retry_wakeup => {
                manager.reconcile_retry.mark_due();
                Some(Event::Control(ControlCommand::Reconcile))
            }
            observation = handle.observations.recv() => observation.map(|first| {
                let mut batch = vec![first];
                while batch.len() < UPSERT_PIPELINE_SIZE {
                    let Some(next) = handle.observations.try_recv() else { break };
                    batch.push(next);
                }
                Event::Observe {
                    batch,
                    from_retry: false,
                }
            }),
        };
        let Some(event) = event else { break };
        match event {
            Event::ReconcileCompleted => manager.harvest_reconcile().await,
            Event::Control(ControlCommand::Reconcile) => {
                if let Err(error) = manager.start_reconcile().await {
                    manager.record_reconcile_error(error).await;
                }
            }
            Event::Control(ControlCommand::Sweep) => {
                if let Err(error) = manager.sweep().await
                    && error_logs.should_log("sweep")
                {
                    warn!(plugin = %tag, err = %error, "ros_route sweep failed");
                }
            }
            Event::Observe {
                mut batch,
                from_retry,
            } => {
                if !from_retry && let Some(retry_at) = retries.values().map(|(at, _)| *at).min() {
                    for (key, command) in batch.drain(..) {
                        defer_route_observation(
                            &mut retries,
                            manager.cfg.queue_capacity,
                            retry_at,
                            key,
                            command,
                            manager.metrics.as_deref(),
                        );
                    }
                    handle.refresh_pending_metric_with(retries.len());
                    continue;
                }
                let observations = batch
                    .iter()
                    .map(|(key, command)| (key.clone(), command.clone()))
                    .collect::<Vec<_>>();
                let result = manager.observe_batch(&observations).await;
                let retry_delay = if result.is_err() {
                    manager.transport_retry_delay().await
                } else {
                    None
                };
                for (key, mut command) in batch.drain(..) {
                    for completion in command.completions.drain(..) {
                        completion.finish(&result);
                    }
                    if let Some(delay) = retry_delay {
                        if retries.len() >= manager.cfg.queue_capacity
                            && !retries.contains_key(&key)
                        {
                            manager.discard_unsynced_observation(&key);
                        }
                        defer_route_observation(
                            &mut retries,
                            manager.cfg.queue_capacity,
                            tokio::time::Instant::now() + delay,
                            key,
                            command,
                            manager.metrics.as_deref(),
                        );
                    }
                }
                if retry_delay.is_none()
                    && let Err(error) = result
                    && error_logs.should_log("observe")
                {
                    warn!(plugin = %tag, err = %error, "ros_route observation failed");
                }
            }
        }
        handle.refresh_pending_metric_with(retries.len());
    }
    debug!(plugin = %tag, "ros_route manager worker exited");
}

fn defer_route_observation(
    retries: &mut AHashMap<RouteKey, (tokio::time::Instant, RouteObservation)>,
    capacity: usize,
    retry_at: tokio::time::Instant,
    key: RouteKey,
    command: RouteObservation,
    metrics: Option<&RosRouteMetrics>,
) {
    if let Some((scheduled_at, existing)) = retries.get_mut(&key) {
        *scheduled_at = (*scheduled_at).min(retry_at);
        existing.coalesce(command);
        if let Some(metrics) = metrics {
            metrics
                .coalesced_total
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        return;
    }
    if retries.len() < capacity {
        retries.insert(key, (retry_at, command));
        return;
    }

    let result = Err(DnsError::plugin(
        "ros_route retry observation capacity reached",
    ));
    for completion in command.completions {
        completion.finish(&result);
    }
    if let Some(metrics) = metrics {
        metrics
            .dropped_total
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(super) fn is_default_route_dst(dst: &str) -> bool {
    dst == ROUTE_DEFAULT_V4 || dst == ROUTE_DEFAULT_V6
}

fn now_millis() -> u64 {
    AppClock::now_timestamp()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::*;
    use crate::plugin::executor::ros_route::model::validation_comment;

    #[derive(Debug, Default)]
    struct MockApi {
        routes: Mutex<Vec<RouterRoute>>,
        connections: Mutex<AHashSet<IpAddr>>,
        delete_failures: AtomicUsize,
    }

    impl MockApi {
        fn routes(&self) -> Vec<RouterRoute> {
            self.routes.lock().expect("routes").clone()
        }

        fn remove_remote(&self, key: &RouteKey) {
            self.routes.lock().expect("routes").retain(|route| {
                route.dst_address != key.dst_address() || route.routing_table != key.table
            });
        }
    }

    #[async_trait]
    impl MikrotikApi for MockApi {
        async fn list_managed_routes(
            &self,
            table: &str,
            _require_ipv4: bool,
            _require_ipv6: bool,
        ) -> Result<Vec<RouterRoute>> {
            Ok(self
                .routes()
                .into_iter()
                .filter(|route| route.routing_table == table)
                .collect())
        }

        async fn find_routes(
            &self,
            key: &RouteKey,
            comment_prefix: &str,
            plugin_tag: &str,
        ) -> Result<Vec<RouterRoute>> {
            Ok(self
                .routes()
                .into_iter()
                .filter(|route| {
                    route.dst_address == key.dst_address()
                        && route.routing_table == key.table
                        && route.comment.as_deref().is_some_and(|comment| {
                            matches!(
                                RouteCommentCodec::decode(
                                    comment_prefix,
                                    plugin_tag,
                                    route.family,
                                    &route.dst_address,
                                    comment,
                                ),
                                Ok(Some(_))
                            )
                        })
                })
                .collect())
        }

        async fn upsert_host_route(
            &self,
            key: &RouteKey,
            gateway: &str,
            distance: u8,
            comment: &str,
            _comment_prefix: &str,
            _plugin_tag: &str,
        ) -> Result<String> {
            let mut routes = self.routes.lock().expect("routes");
            if let Some(route) = routes.iter_mut().find(|route| {
                route.dst_address == key.dst_address() && route.routing_table == key.table
            }) {
                route.gateway = Some(gateway.to_string());
                route.distance = Some(distance);
                route.comment = Some(comment.to_string());
                route.disabled = false;
                return Ok(route.id.clone());
            }
            let id = format!("*{}", routes.len() + 1);
            routes.push(RouterRoute {
                id: id.clone(),
                family: key.family(),
                dst_address: key.dst_address(),
                routing_table: key.table.clone(),
                gateway: Some(gateway.to_string()),
                distance: Some(distance),
                comment: Some(comment.to_string()),
                disabled: false,
            });
            Ok(id)
        }

        async fn delete_route_if_matches(&self, expected: &RouterRoute) -> Result<bool> {
            if self
                .delete_failures
                .try_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(DnsError::plugin("route delete unavailable"));
            }
            let mut routes = self.routes.lock().expect("routes");
            let Some(index) = routes.iter().position(|route| route == expected) else {
                return Ok(false);
            };
            routes.remove(index);
            Ok(true)
        }

        async fn connection_destinations(
            &self,
            _family: RouteFamily,
            destinations: &[IpAddr],
        ) -> Result<AHashSet<IpAddr>> {
            let connections = self.connections.lock().expect("connections");
            Ok(destinations
                .iter()
                .filter(|ip| connections.contains(ip))
                .copied()
                .collect())
        }
    }

    fn config(fixed_ttl: Option<u32>, conntrack_guard: bool) -> RouteManagerConfig {
        AppClock::start();
        RouteManagerConfig {
            plugin_tag: "route-test".to_string(),
            routing_table: "policy".to_string(),
            gateway4: Some("192.0.2.1".to_string()),
            gateway6: None,
            persistent_ips: AHashSet::new(),
            comment_prefix: "fdns".to_string(),
            distance: 100,
            min_ttl: 1,
            max_ttl: 3_600,
            fixed_ttl,
            conntrack_guard,
            queue_capacity: 16_384,
        }
    }

    #[test]
    fn route_comment_has_no_domain_or_mixed_ownership() {
        let comment = RouteCommentCodec::encode_dynamic(
            "fdns",
            "route-test",
            LeaseDeadline::At(400_000),
            100_000,
        );
        assert_eq!(comment, "fdns;pg=route-test;kind=D;exp=400;seen=100");
        let decoded = RouteCommentCodec::decode(
            "fdns",
            "route-test",
            RouteFamily::Ipv4,
            "203.0.113.10/32",
            &comment,
        )
        .expect("decode")
        .expect("owned");
        assert_eq!(decoded.kind, RouteCommentKind::Dynamic);
        assert_eq!(decoded.expires_at_ms, LeaseDeadline::At(400_000));
    }

    #[test]
    fn old_domain_comment_is_not_owned_by_the_new_plugin_format() {
        let decoded = RouteCommentCodec::decode(
            "fdns",
            "route-test",
            RouteFamily::Ipv4,
            "203.0.113.10/32",
            "fdns;pg=route-test;kind=dynamic;dm=example.com;exp=400;seen=100",
        )
        .expect("decode");
        assert!(decoded.is_none());
    }

    #[tokio::test]
    async fn repeated_observations_share_one_route_lease() {
        let api = Arc::new(MockApi::default());
        let mut manager = RouteManager::new(api.clone(), config(Some(300), false));
        let key = RouteKey::new("203.0.113.10".parse().expect("ip"), "policy".to_string());
        let now = now_millis();

        manager
            .observe_key(
                key.clone(),
                &RouteObservation {
                    deadline: LeaseDeadline::At(now + 300_000),
                    observed_at_ms: now,
                    completions: Vec::new(),
                },
            )
            .await
            .expect("first observation");
        manager
            .observe_key(
                key,
                &RouteObservation {
                    deadline: LeaseDeadline::At(now + 600_000),
                    observed_at_ms: now + 1,
                    completions: Vec::new(),
                },
            )
            .await
            .expect("second observation");

        assert_eq!(manager.leases.len(), 1);
        assert_eq!(api.routes().len(), 1);
        assert!(
            api.routes()[0]
                .comment
                .as_deref()
                .expect("comment")
                .contains("exp=")
        );
    }

    #[tokio::test]
    async fn reconcile_accepts_manual_dynamic_deletion_until_next_observation() {
        let api = Arc::new(MockApi::default());
        let mut manager = RouteManager::new(api.clone(), config(Some(300), false));
        let key = RouteKey::new("203.0.113.11".parse().expect("ip"), "policy".to_string());
        let now = now_millis();
        let observation = RouteObservation {
            deadline: LeaseDeadline::At(now + 300_000),
            observed_at_ms: now,
            completions: Vec::new(),
        };
        manager
            .observe_key(key.clone(), &observation)
            .await
            .expect("observation");
        let generation = manager.leases.revision();
        api.remove_remote(&key);

        manager
            .apply_snapshot(VersionedSnapshot {
                generation,
                value: Vec::new(),
            })
            .await
            .expect("reconcile");
        assert!(!manager.leases.contains_key(&key));

        manager
            .observe_key(key, &observation)
            .await
            .expect("re-observation");
        assert_eq!(api.routes().len(), 1);
    }

    #[tokio::test]
    async fn stale_snapshot_cannot_erase_a_newer_observation() {
        let api = Arc::new(MockApi::default());
        let mut manager = RouteManager::new(api.clone(), config(Some(300), false));
        let key = RouteKey::new("203.0.113.21".parse().expect("ip"), "policy".to_string());
        let now = now_millis();
        manager
            .observe_key(
                key.clone(),
                &RouteObservation {
                    deadline: LeaseDeadline::At(now + 300_000),
                    observed_at_ms: now,
                    completions: Vec::new(),
                },
            )
            .await
            .expect("initial observation");
        let scan_revision = manager.leases.revision();

        manager
            .observe_key(
                key.clone(),
                &RouteObservation {
                    deadline: LeaseDeadline::At(now + 120_000),
                    observed_at_ms: now + 1,
                    completions: Vec::new(),
                },
            )
            .await
            .expect("newer observation");
        manager
            .apply_snapshot(VersionedSnapshot {
                generation: scan_revision,
                value: Vec::new(),
            })
            .await
            .expect("stale snapshot");

        assert!(manager.leases.contains_key(&key));
        assert_eq!(api.routes().len(), 1);
    }

    #[tokio::test]
    async fn stale_persistent_snapshot_converges_to_new_dynamic_lease() {
        let api = Arc::new(MockApi::default());
        let key = RouteKey::new("203.0.113.22".parse().expect("ip"), "policy".to_string());
        let mut cfg = config(Some(300), false);
        cfg.persistent_ips
            .insert(key.dst_address().parse().expect("prefix"));
        let mut manager = RouteManager::new(api.clone(), cfg);
        manager.ensure_initialized().await.expect("initialize");
        manager
            .sync_keys(vec![key.clone()], now_millis())
            .await
            .expect("persistent sync");
        let stale = api.routes();

        manager.persistent.remove(&key);
        let now = now_millis();
        manager
            .observe_key(
                key.clone(),
                &RouteObservation {
                    deadline: LeaseDeadline::At(now + 300_000),
                    observed_at_ms: now,
                    completions: Vec::new(),
                },
            )
            .await
            .expect("dynamic observation");
        manager
            .apply_snapshot(VersionedSnapshot {
                generation: 0,
                value: stale,
            })
            .await
            .expect("stale persistent snapshot");

        let route = api.routes().pop().expect("route");
        let meta = RouteCommentCodec::decode(
            "fdns",
            "route-test",
            route.family,
            &route.dst_address,
            route.comment.as_deref().expect("comment"),
        )
        .expect("decode")
        .expect("owned");
        assert_eq!(meta.kind, RouteCommentKind::Dynamic);
        assert!(manager.leases.contains_key(&key));
    }

    #[tokio::test]
    async fn reconcile_does_not_delete_a_validation_row_after_ownership_changes() {
        let api = Arc::new(MockApi::default());
        let mut manager = RouteManager::new(api.clone(), config(None, false));
        let key = RouteKey::new("198.18.0.10".parse().expect("ip"), "policy".to_string());
        let route = RouterRoute {
            id: "*validation".to_string(),
            family: RouteFamily::Ipv4,
            dst_address: key.dst_address(),
            routing_table: key.table.clone(),
            gateway: Some("192.0.2.1".to_string()),
            distance: Some(100),
            comment: Some(format!(
                "{};nonce=1",
                validation_comment("fdns", "route-test")
            )),
            disabled: true,
        };
        api.routes.lock().expect("routes").push(route.clone());
        api.routes.lock().expect("routes")[0].comment = Some("operator-owned".to_string());

        manager
            .apply_snapshot(VersionedSnapshot {
                generation: 0,
                value: vec![route],
            })
            .await
            .expect("reconcile");

        assert_eq!(api.routes().len(), 1);
        assert_eq!(api.routes()[0].comment.as_deref(), Some("operator-owned"));
    }

    #[tokio::test]
    async fn reconcile_keeps_the_longest_dynamic_duplicate_lease() {
        let api = Arc::new(MockApi::default());
        let mut manager = RouteManager::new(api.clone(), config(None, false));
        let key = RouteKey::new("203.0.113.94".parse().expect("ip"), "policy".to_string());
        let now = now_millis();
        let route = |id: &str, deadline, seen| RouterRoute {
            id: id.to_string(),
            family: RouteFamily::Ipv4,
            dst_address: key.dst_address(),
            routing_table: key.table.clone(),
            gateway: Some("192.0.2.1".to_string()),
            distance: Some(100),
            comment: Some(RouteCommentCodec::encode_dynamic(
                "fdns",
                "route-test",
                deadline,
                seen,
            )),
            disabled: false,
        };
        let expired = route(
            "*expired",
            LeaseDeadline::At(now.saturating_sub(1_000)),
            now.saturating_sub(60_000),
        );
        let valid = route(
            "*valid",
            LeaseDeadline::At(now.saturating_add(300_000)),
            now,
        );
        api.routes
            .lock()
            .expect("routes")
            .extend([expired.clone(), valid.clone()]);

        manager
            .apply_snapshot(VersionedSnapshot {
                generation: 0,
                value: vec![expired, valid],
            })
            .await
            .expect("reconcile");

        let routes = api.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].id, "*valid");
        assert!(
            manager
                .leases
                .get(&key)
                .is_some_and(|lease| !lease.desired().is_expired(now))
        );
    }

    #[tokio::test]
    async fn periodic_persistent_reconcile_ignores_dynamic_routes() {
        let api = Arc::new(MockApi::default());
        let mut manager = RouteManager::new(api.clone(), config(None, false));
        let key = RouteKey::new("203.0.113.95".parse().expect("ip"), "policy".to_string());
        let route = RouterRoute {
            id: "*dynamic".to_string(),
            family: RouteFamily::Ipv4,
            dst_address: key.dst_address(),
            routing_table: key.table.clone(),
            gateway: Some("192.0.2.1".to_string()),
            distance: Some(100),
            comment: Some(RouteCommentCodec::encode_dynamic(
                "fdns",
                "route-test",
                LeaseDeadline::At(now_millis().saturating_add(300_000)),
                now_millis(),
            )),
            disabled: false,
        };
        api.routes.lock().expect("routes").push(route.clone());

        manager
            .apply_persistent_snapshot(vec![route])
            .await
            .expect("persistent reconcile");

        assert!(manager.leases.get(&key).is_none());
        assert_eq!(api.routes().len(), 1);
    }

    #[tokio::test]
    async fn failed_legacy_validation_cleanup_can_be_retried() {
        let api = Arc::new(MockApi::default());
        let mut manager = RouteManager::new(api.clone(), config(None, false));
        let key = RouteKey::new("198.18.0.11".parse().expect("ip"), "policy".to_string());
        let route = RouterRoute {
            id: "*validation-retry".to_string(),
            family: RouteFamily::Ipv4,
            dst_address: key.dst_address(),
            routing_table: key.table,
            gateway: Some("192.0.2.1".to_string()),
            distance: Some(100),
            comment: Some(format!(
                "{};nonce=2",
                validation_comment("fdns", "route-test")
            )),
            disabled: true,
        };
        api.routes.lock().expect("routes").push(route.clone());
        api.delete_failures.store(1, Ordering::Release);

        assert!(
            manager
                .apply_snapshot(VersionedSnapshot {
                    generation: 0,
                    value: vec![route],
                })
                .await
                .is_err()
        );
        assert_eq!(api.routes().len(), 1);

        manager
            .apply_snapshot(VersionedSnapshot {
                generation: 0,
                value: api.routes(),
            })
            .await
            .expect("retry cleanup");
        assert!(api.routes().is_empty());
    }

    #[tokio::test]
    async fn refreshed_dynamic_comment_invalidates_an_older_delete_snapshot() {
        let api = Arc::new(MockApi::default());
        let manager = RouteManager::new(api.clone(), config(Some(300), false));
        let key = RouteKey::new("203.0.113.90".parse().expect("ip"), "policy".to_string());
        let old = RouterRoute {
            id: "*refresh-race".to_string(),
            family: RouteFamily::Ipv4,
            dst_address: key.dst_address(),
            routing_table: key.table,
            gateway: Some("192.0.2.1".to_string()),
            distance: Some(100),
            comment: Some(RouteCommentCodec::encode_dynamic(
                "fdns",
                "route-test",
                LeaseDeadline::At(100_000),
                1_000,
            )),
            disabled: false,
        };
        api.routes.lock().expect("routes").push(old.clone());
        api.routes.lock().expect("routes")[0].comment = Some(RouteCommentCodec::encode_dynamic(
            "fdns",
            "route-test",
            LeaseDeadline::At(500_000),
            2_000,
        ));

        assert!(
            !manager
                .delete_route_if_still_owned(&old)
                .await
                .expect("conditional delete")
        );
        assert_eq!(api.routes().len(), 1);
    }

    #[tokio::test]
    async fn changed_route_parameters_invalidate_an_older_delete_snapshot() {
        let key = RouteKey::new("203.0.113.92".parse().expect("ip"), "policy".to_string());
        let expected = RouterRoute {
            id: "*parameter-race".to_string(),
            family: RouteFamily::Ipv4,
            dst_address: key.dst_address(),
            routing_table: key.table,
            gateway: Some("192.0.2.1".to_string()),
            distance: Some(100),
            comment: Some(RouteCommentCodec::encode_dynamic(
                "fdns",
                "route-test",
                LeaseDeadline::At(100_000),
                1_000,
            )),
            disabled: false,
        };

        for changed in [
            RouterRoute {
                gateway: Some("192.0.2.2".to_string()),
                ..expected.clone()
            },
            RouterRoute {
                distance: Some(101),
                ..expected.clone()
            },
            RouterRoute {
                disabled: true,
                ..expected.clone()
            },
        ] {
            let api = Arc::new(MockApi::default());
            api.routes.lock().expect("routes").push(changed);
            let manager = RouteManager::new(api.clone(), config(Some(300), false));

            assert!(
                !manager
                    .delete_route_if_still_owned(&expected)
                    .await
                    .expect("conditional delete")
            );
            assert_eq!(api.routes().len(), 1);
        }
    }

    #[tokio::test]
    async fn conntrack_guard_defers_all_dynamic_duplicate_deletions() {
        let api = Arc::new(MockApi::default());
        let mut manager = RouteManager::new(api.clone(), config(Some(1), true));
        let ip = "203.0.113.12".parse().expect("ip");
        let key = RouteKey::new(ip, "policy".to_string());
        let now = now_millis();
        manager
            .observe_key(
                key.clone(),
                &RouteObservation {
                    deadline: LeaseDeadline::At(now + 300_000),
                    observed_at_ms: now,
                    completions: Vec::new(),
                },
            )
            .await
            .expect("observation");
        let mut duplicate = api.routes()[0].clone();
        duplicate.id = "*dynamic-duplicate".to_string();
        api.routes.lock().expect("routes").push(duplicate);
        manager.leases.remove(&key);
        manager.leases.observe(key.clone(), LeaseDeadline::At(0), 0);
        api.connections.lock().expect("connections").insert(ip);

        manager.sweep().await.expect("guarded sweep");
        assert_eq!(api.routes().len(), 2);

        api.connections.lock().expect("connections").clear();
        manager.connection_retry_after.clear();
        manager.sweep().await.expect("retry sweep");
        assert!(api.routes().is_empty());
    }

    #[tokio::test]
    async fn removed_persistent_route_deletes_all_duplicates_without_conntrack_guard() {
        let api = Arc::new(MockApi::default());
        let ip = "203.0.113.13".parse().expect("ip");
        let key = RouteKey::new(ip, "policy".to_string());
        let mut cfg = config(None, true);
        cfg.persistent_ips
            .insert(key.dst_address().parse().expect("prefix"));
        let mut manager = RouteManager::new(api.clone(), cfg);
        manager.ensure_initialized().await.expect("initialize");
        manager
            .sync_keys(vec![key.clone()], now_millis())
            .await
            .expect("persistent upsert");
        let mut duplicate = api.routes()[0].clone();
        duplicate.id = "*persistent-duplicate".to_string();
        api.routes.lock().expect("routes").push(duplicate);
        api.connections.lock().expect("connections").insert(ip);

        manager.persistent.remove(&key);
        manager.routes.get_mut(&key).expect("route").sync_state =
            SyncState::PendingPersistentDelete;
        manager
            .sync_keys(vec![key], now_millis())
            .await
            .expect("persistent delete");

        assert!(api.routes().is_empty());
    }
}
