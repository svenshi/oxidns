// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arc_swap::ArcSwap;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{info, warn};

#[cfg(feature = "api")]
use super::api::{RulesListResponse, register_api};
use super::config::DynamicDomainSetConfig;
use super::rules::{
    DynamicDomainMutation, DynamicDomainRuleKind, DynamicDomainRuleMetadata,
    DynamicDomainRuleOrigin, canonicalize_rules,
};
use super::storage::{
    append_rule_file, read_metadata_file, read_rule_file, rewrite_metadata_file, rewrite_rule_file,
};
use crate::core::rule_matcher::{DomainRuleKind, DomainRuleMatcher, split_domain_rule_expression};
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result as DnsResult};
use crate::infra::task::{spawn_blocking_result, spawn_isolated_build};
use crate::proto::{Name, Question};

/// Immutable state published to matchers.
///
/// The snapshot is swapped as one `Arc`, so readers always see a fully compiled
/// matcher and never observe partial file writes or partially rebuilt rule
/// structures.
#[derive(Debug, Default)]
pub(super) struct DynamicDomainSetSnapshot {
    pub(super) matcher: DomainRuleMatcher,
}

/// Ordered canonical rule list plus a set for fast duplicate suppression.
///
/// This mutex is intentionally not touched by `contains_name`; it is only used
/// by writer/API paths where preserving file order and exact rule text matters.
#[derive(Debug, Default)]
struct RuleState {
    rules: Vec<Arc<str>>,
    known: HashSet<Arc<str>>,
    metadata: BTreeMap<String, DynamicDomainRuleMetadata>,
}

type MutationReply = oneshot::Sender<DnsResult<DynamicDomainMutation>>;

/// All file, snapshot, and rule-list mutations are serialized through one
/// worker.
///
/// Append can be fire-and-forget for learned domains or request/reply for API
/// and synchronous learning. Remove, clear, and reload always wait because they
/// replace the authoritative file contents and must report completion.
#[derive(Debug)]
#[allow(dead_code)]
enum WorkerCommand {
    Append {
        rules: Vec<String>,
        origin: DynamicDomainRuleOrigin,
        wait: Option<MutationReply>,
    },
    Remove {
        rules: Vec<String>,
        wait: MutationReply,
    },
    Clear {
        wait: MutationReply,
    },
    Reload {
        wait: MutationReply,
    },
    Shutdown {
        done: oneshot::Sender<()>,
    },
}

/// Append batch item kept in memory until `batch_size` or `flush_interval_ms`.
#[derive(Debug)]
struct PendingAppend {
    rules: Vec<Arc<str>>,
    wait: Option<MutationReply>,
}

/// Shared backend for the provider instance.
///
/// It owns both the hot snapshot and the side-effect machinery. The provider
/// object itself is small and mostly delegates here so the API handlers and the
/// executor downcast path can share the same state safely.
#[derive(Debug)]
pub(super) struct DynamicDomainSetBackend {
    tag: String,
    config: Arc<DynamicDomainSetConfig>,
    /// Canonical source of truth for ordered rules and duplicate checks.
    state: Mutex<RuleState>,
    /// Lock-free read side for matcher hot paths.
    snapshot: ArcSwap<DynamicDomainSetSnapshot>,
    /// Sender becomes available after `init`; stored so API/executor calls can
    /// enqueue work without owning the worker directly.
    tx: Mutex<Option<mpsc::Sender<WorkerCommand>>>,
    /// Joined during plugin destroy to flush pending appends before shutdown.
    worker_handle: Mutex<Option<JoinHandle<()>>>,
    expired_total: AtomicU64,
    capacity_rejected_total: AtomicU64,
    queue_rejected_total: AtomicU64,
    last_success_at_ms: AtomicU64,
    last_error: Mutex<Option<String>>,
}

impl DynamicDomainSetBackend {
    pub(super) fn new(tag: String, config: DynamicDomainSetConfig) -> Self {
        Self {
            tag,
            config: Arc::new(config),
            state: Mutex::new(RuleState::default()),
            snapshot: ArcSwap::from_pointee(DynamicDomainSetSnapshot::default()),
            tx: Mutex::new(None),
            worker_handle: Mutex::new(None),
            expired_total: AtomicU64::new(0),
            capacity_rejected_total: AtomicU64::new(0),
            queue_rejected_total: AtomicU64::new(0),
            last_success_at_ms: AtomicU64::new(0),
            last_error: Mutex::new(None),
        }
    }

    #[cfg(feature = "api")]
    pub(super) fn tag(&self) -> &str {
        &self.tag
    }

    pub(super) async fn start(self: &Arc<Self>) -> DnsResult<()> {
        // Startup is the only place that applies bootstrap rules. After this
        // point the file itself is authoritative, including external edits that
        // become visible through explicit provider reload.
        let config = self.config.clone();
        let (rules, metadata, snapshot) =
            spawn_isolated_build("dynamic_domain_set startup build", move || {
                bootstrap_file_if_needed(&config)?;
                let rules = read_rule_file(&config.path)?;
                let metadata = config
                    .metadata_path
                    .as_deref()
                    .map(read_metadata_file)
                    .transpose()?
                    .unwrap_or_default();
                let snapshot = build_snapshot(&rules)?;
                Ok((rules, metadata, snapshot))
            })
            .await?;
        self.install_compiled_rules(rules, metadata, snapshot)?;
        let (tx, rx) = mpsc::channel(self.config.queue_size);
        {
            let mut slot = self
                .tx
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set sender lock poisoned"))?;
            *slot = Some(tx);
        }
        let backend = self.clone();
        let handle = tokio::spawn(async move {
            backend.run_worker(rx).await;
        });
        {
            let mut slot = self
                .worker_handle
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set worker lock poisoned"))?;
            *slot = Some(handle);
        }
        #[cfg(feature = "api")]
        register_api(self)?;
        Ok(())
    }

    pub(super) async fn shutdown(&self) -> DnsResult<()> {
        // Ask the worker to drain pending append batches before the runtime
        // drops it. If the channel is already closed there is nothing left to
        // flush from this backend.
        let tx = self.sender()?;
        let (done_tx, done_rx) = oneshot::channel();
        if tx
            .send(WorkerCommand::Shutdown { done: done_tx })
            .await
            .is_ok()
        {
            let _ = done_rx.await;
        }
        let handle = self
            .worker_handle
            .lock()
            .map_err(|_| DnsError::runtime("dynamic_domain_set worker lock poisoned"))?
            .take();
        if let Some(handle) = handle {
            match handle.await {
                Ok(()) => {}
                Err(err) if err.is_cancelled() => {}
                Err(err) => {
                    return Err(DnsError::runtime(format!(
                        "dynamic_domain_set worker failed: {err}"
                    )));
                }
            }
        }
        Ok(())
    }

    #[inline]
    pub(super) fn contains_name(&self, name: &Name) -> bool {
        // Hot path: one atomic snapshot load plus matcher lookup. No locks, no
        // filesystem access, and no rule parsing happen per request.
        self.snapshot.load().matcher.is_match_name(name)
    }

    #[inline]
    pub(super) fn contains_question(&self, question: &Question) -> bool {
        self.contains_name(question.name())
    }

    pub(super) async fn reload(&self) -> DnsResult<()> {
        self.reload_sync().await.map(|_| ())
    }

    pub(crate) fn append_rules_async(
        &self,
        raw_rules: Vec<String>,
        default_kind: DynamicDomainRuleKind,
        origin: DynamicDomainRuleOrigin,
    ) -> DnsResult<DynamicDomainMutation> {
        let rules = canonicalize_rules(raw_rules, default_kind, "append")?;
        if rules.is_empty() {
            return Ok(DynamicDomainMutation {
                added: 0,
                removed: 0,
                total: self.current_total()?,
            });
        }
        let queued = rules.len();
        let total_hint = self.current_total()?.saturating_add(queued);
        let tx = self.sender()?;
        match tx.try_send(WorkerCommand::Append {
            rules,
            origin,
            wait: None,
        }) {
            Ok(()) => {
                // Async callers only receive an enqueue acknowledgement. The
                // worker later computes the real added/total counts after it
                // serializes this append against remove/clear/reload commands.
                Ok(DynamicDomainMutation {
                    added: queued,
                    removed: 0,
                    total: total_hint,
                })
            }
            Err(err) => {
                self.queue_rejected_total.fetch_add(1, Ordering::Relaxed);
                self.record_error(err.to_string());
                Err(DnsError::plugin(format!(
                    "dynamic_domain_set '{}' append queue failed: {}",
                    self.tag, err
                )))
            }
        }
    }

    pub(crate) async fn append_rules_sync(
        &self,
        raw_rules: Vec<String>,
        default_kind: DynamicDomainRuleKind,
        origin: DynamicDomainRuleOrigin,
        timeout_duration: Duration,
    ) -> DnsResult<DynamicDomainMutation> {
        let rules = canonicalize_rules(raw_rules, default_kind, "append")?;
        if rules.is_empty() {
            return Ok(DynamicDomainMutation {
                added: 0,
                removed: 0,
                total: self.current_total()?,
            });
        }
        // Synchronous callers use the same worker path as async learning. That
        // keeps ordering with remove/clear/reload identical while giving API
        // handlers a durable "written and snapshot-swapped" acknowledgement.
        let (reply_tx, reply_rx) = oneshot::channel();
        let tx = self.sender()?;
        let send_result = tokio::time::timeout(
            timeout_duration,
            tx.send(WorkerCommand::Append {
                rules,
                origin,
                wait: Some(reply_tx),
            }),
        )
        .await;
        match send_result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                return Err(DnsError::plugin(format!(
                    "dynamic_domain_set '{}' append queue closed: {}",
                    self.tag, err
                )));
            }
            Err(_) => {
                return Err(DnsError::plugin(format!(
                    "dynamic_domain_set '{}' append timed out enqueueing work",
                    self.tag
                )));
            }
        }
        tokio::time::timeout(timeout_duration, reply_rx)
            .await
            .map_err(|_| {
                DnsError::plugin(format!(
                    "dynamic_domain_set '{}' append timed out waiting for flush",
                    self.tag
                ))
            })?
            .map_err(|_| {
                DnsError::plugin(format!(
                    "dynamic_domain_set '{}' append worker dropped reply",
                    self.tag
                ))
            })?
    }

    #[cfg(feature = "api")]
    pub(super) async fn remove_rules_sync(
        &self,
        raw_rules: Vec<String>,
        default_kind: DynamicDomainRuleKind,
    ) -> DnsResult<DynamicDomainMutation> {
        let rules = canonicalize_rules(raw_rules, default_kind, "remove")?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender()?
            .send(WorkerCommand::Remove {
                rules,
                wait: reply_tx,
            })
            .await
            .map_err(|err| {
                DnsError::plugin(format!(
                    "dynamic_domain_set '{}' remove queue closed: {}",
                    self.tag, err
                ))
            })?;
        reply_rx.await.map_err(|_| {
            DnsError::plugin(format!(
                "dynamic_domain_set '{}' remove worker dropped reply",
                self.tag
            ))
        })?
    }

    #[cfg(feature = "api")]
    pub(super) async fn clear_sync(&self) -> DnsResult<DynamicDomainMutation> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender()?
            .send(WorkerCommand::Clear { wait: reply_tx })
            .await
            .map_err(|err| {
                DnsError::plugin(format!(
                    "dynamic_domain_set '{}' clear queue closed: {}",
                    self.tag, err
                ))
            })?;
        reply_rx.await.map_err(|_| {
            DnsError::plugin(format!(
                "dynamic_domain_set '{}' clear worker dropped reply",
                self.tag
            ))
        })?
    }

    pub(super) async fn reload_sync(&self) -> DnsResult<DynamicDomainMutation> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender()?
            .send(WorkerCommand::Reload { wait: reply_tx })
            .await
            .map_err(|err| {
                DnsError::plugin(format!(
                    "dynamic_domain_set '{}' reload queue closed: {}",
                    self.tag, err
                ))
            })?;
        reply_rx.await.map_err(|_| {
            DnsError::plugin(format!(
                "dynamic_domain_set '{}' reload worker dropped reply",
                self.tag
            ))
        })?
    }

    #[cfg(feature = "api")]
    pub(super) fn list_rules(&self, cursor: usize, limit: usize) -> DnsResult<RulesListResponse> {
        let state = self
            .state
            .lock()
            .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
        let total = state.rules.len();
        let start = cursor.min(total);
        let end = start.saturating_add(limit).min(total);
        let rules = state.rules[start..end]
            .iter()
            .map(|rule| rule.to_string())
            .collect();
        let next_cursor = (end < total).then_some(end);
        Ok(RulesListResponse::new(total, next_cursor, rules))
    }

    #[cfg(test)]
    pub(super) fn store_snapshot_for_test(&self, snapshot: DynamicDomainSetSnapshot) {
        self.snapshot.store(Arc::new(snapshot));
    }

    fn sender(&self) -> DnsResult<mpsc::Sender<WorkerCommand>> {
        self.tx
            .lock()
            .map_err(|_| DnsError::runtime("dynamic_domain_set sender lock poisoned"))?
            .clone()
            .ok_or_else(|| {
                DnsError::plugin(format!(
                    "dynamic_domain_set '{}' worker is not initialized",
                    self.tag
                ))
            })
    }

    fn current_total(&self) -> DnsResult<usize> {
        Ok(self
            .state
            .lock()
            .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?
            .rules
            .len())
    }
}

fn bootstrap_file_if_needed(config: &DynamicDomainSetConfig) -> DnsResult<()> {
    if config.path.exists() {
        return Ok(());
    }
    if let Some(parent) = config.path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let rules = canonicalize_rules(
        config.bootstrap_rules.clone(),
        DynamicDomainRuleKind::Domain,
        "bootstrap_rules",
    )?;
    // Bootstrap writes canonical rules immediately so later API rewrites do
    // not have to preserve a separate "initial rules" concept.
    rewrite_rule_file(&config.path, &rules)?;
    Ok(())
}

impl DynamicDomainSetBackend {
    fn install_compiled_rules(
        &self,
        rules: Vec<Arc<str>>,
        mut metadata: BTreeMap<String, DynamicDomainRuleMetadata>,
        snapshot: DynamicDomainSetSnapshot,
    ) -> DnsResult<DynamicDomainMutation> {
        let now = AppClock::now_timestamp();
        let known = rules
            .iter()
            .map(|rule| rule.as_ref())
            .collect::<HashSet<_>>();
        metadata.retain(|rule, _| known.contains(rule.as_str()));
        for rule in &rules {
            metadata
                .entry(rule.to_string())
                .or_insert(DynamicDomainRuleMetadata {
                    origin: if self.config.metadata_path.is_some() {
                        DynamicDomainRuleOrigin::Learned
                    } else {
                        DynamicDomainRuleOrigin::Manual
                    },
                    created_at_ms: now,
                });
        }
        let total = rules.len();
        {
            // State and snapshot are updated in this order so API list output
            // and hot-path matching converge on the same rule set immediately
            // after the snapshot swap.
            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
            state.known = rules.iter().cloned().collect();
            state.rules = rules;
            state.metadata = metadata;
        }
        self.snapshot.store(Arc::new(snapshot));
        Ok(DynamicDomainMutation {
            added: 0,
            removed: 0,
            total,
        })
    }

    fn stage_new_rules(
        &self,
        rules: Vec<String>,
        origin: DynamicDomainRuleOrigin,
    ) -> DnsResult<StagedRules> {
        let mut staged = Vec::new();
        let total = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
            let new_count = rules
                .iter()
                .filter(|rule| !state.known.contains(rule.as_str()))
                .count();
            if self
                .config
                .max_entries
                .is_some_and(|limit| state.rules.len().saturating_add(new_count) > limit)
            {
                self.capacity_rejected_total
                    .fetch_add(new_count as u64, Ordering::Relaxed);
                let message = format!(
                    "dynamic_domain_set '{}' capacity {} would be exceeded",
                    self.tag,
                    self.config.max_entries.unwrap_or_default()
                );
                self.record_error(message.clone());
                return Err(DnsError::plugin(message));
            }
            let now = AppClock::now_timestamp();
            for rule in rules {
                let rule: Arc<str> = Arc::from(rule);
                // Insert into both structures while holding one lock so the
                // ordered list and duplicate set cannot drift apart.
                if state.known.insert(rule.clone()) {
                    state.rules.push(rule.clone());
                    state.metadata.insert(
                        rule.to_string(),
                        DynamicDomainRuleMetadata {
                            origin,
                            created_at_ms: now,
                        },
                    );
                    staged.push(rule);
                }
            }
            state.rules.len()
        };
        Ok(StagedRules {
            mutation: DynamicDomainMutation {
                added: staged.len(),
                removed: 0,
                total,
            },
            rules: staged,
        })
    }

    fn rollback_staged_rules(&self, rules: &[Arc<str>]) {
        if rules.is_empty() {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            for rule in rules {
                state.known.remove(rule.as_ref());
                state.metadata.remove(rule.as_ref());
            }
            state
                .rules
                .retain(|rule| !rules.iter().any(|value| value.as_ref() == rule.as_ref()));
        }
    }

    async fn flush_appends(&self, pending: &mut Vec<PendingAppend>) {
        if pending.is_empty() {
            return;
        }
        // Compile the replacement snapshot before touching the managed file.
        // Regex syntax errors and other matcher validation failures must not
        // poison the file with a rule that would later break reload/startup.
        let appended_rules = pending
            .iter()
            .flat_map(|item| item.rules.iter().cloned())
            .collect::<Vec<_>>();
        let (rules, metadata) = match self.state.lock() {
            Ok(state) => (state.rules.clone(), state.metadata.clone()),
            Err(_) => {
                let error = DnsError::runtime("dynamic_domain_set state lock poisoned");
                self.rollback_staged_rules(&appended_rules);
                let message = error.to_string();
                for item in pending.drain(..) {
                    if let Some(wait) = item.wait {
                        let _ = wait.send(Err(DnsError::plugin(message.clone())));
                    }
                }
                return;
            }
        };
        let total = rules.len();
        let path = self.config.path.clone();
        let metadata_path = self.config.metadata_path.clone();
        let appended_for_task = appended_rules.clone();
        let result = spawn_blocking_result("dynamic_domain_set append build", move || {
            let snapshot = build_snapshot(&rules)?;
            if let Some(metadata_path) = metadata_path.as_deref() {
                rewrite_metadata_file(metadata_path, &metadata)?;
            }
            append_rule_file(&path, &appended_for_task)?;
            Ok(snapshot)
        })
        .await
        .map(|snapshot| (total, snapshot));
        match result {
            Ok((total, snapshot)) => {
                self.snapshot.store(Arc::new(snapshot));
                self.record_success();
                info!(
                    plugin = %self.tag,
                    added = appended_rules.len(),
                    total,
                    "dynamic_domain_set appended rules"
                );
                for item in pending.drain(..) {
                    if let Some(wait) = item.wait {
                        let _ = wait.send(Ok(DynamicDomainMutation {
                            added: item.rules.len(),
                            removed: 0,
                            total,
                        }));
                    }
                }
            }
            Err(err) => {
                warn!(
                    plugin = %self.tag,
                    added = appended_rules.len(),
                    error = %err,
                    "dynamic_domain_set append flush failed"
                );
                // Flush failure means the file and snapshot were not advanced.
                // Remove staged rules so later retries can enqueue them again.
                self.rollback_staged_rules(&appended_rules);
                let message = err.to_string();
                self.record_error(message.clone());
                for item in pending.drain(..) {
                    if let Some(wait) = item.wait {
                        let _ = wait.send(Err(DnsError::plugin(message.clone())));
                    }
                }
            }
        }
    }

    async fn remove_rules(&self, rules: Vec<String>) -> DnsResult<DynamicDomainMutation> {
        let (current_rules, mut metadata, before) = {
            let state = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
            let before = state.rules.len();
            let remove_set = rules.iter().map(String::as_str).collect::<HashSet<_>>();
            let current_rules = state
                .rules
                .iter()
                .filter(|rule| !remove_set.contains(rule.as_ref()))
                .cloned()
                .collect::<Vec<_>>();
            let removed = before.saturating_sub(current_rules.len());
            let total = current_rules.len();
            if removed == 0 {
                return Ok(DynamicDomainMutation {
                    added: 0,
                    removed,
                    total,
                });
            }
            (current_rules, state.metadata.clone(), before)
        };
        let removed = before.saturating_sub(current_rules.len());
        let total = current_rules.len();
        let retained = current_rules
            .iter()
            .map(|rule| rule.as_ref())
            .collect::<HashSet<_>>();
        metadata.retain(|rule, _| retained.contains(rule.as_str()));
        let committed_rules = current_rules.clone();
        let committed_metadata = metadata.clone();
        let path = self.config.path.clone();
        let metadata_path = self.config.metadata_path.clone();
        let snapshot = spawn_isolated_build("dynamic_domain_set remove build", move || {
            let snapshot = build_snapshot(&current_rules)?;
            rewrite_rule_file(&path, &current_rules)?;
            if let Some(metadata_path) = metadata_path.as_deref() {
                rewrite_metadata_file(metadata_path, &metadata)?;
            }
            Ok(snapshot)
        })
        .await?;
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
            state.known = committed_rules.iter().cloned().collect();
            state.rules = committed_rules;
            state.metadata = committed_metadata;
        }
        self.snapshot.store(Arc::new(snapshot));
        self.record_success();
        Ok(DynamicDomainMutation {
            added: 0,
            removed,
            total,
        })
    }

    async fn clear_rules(&self) -> DnsResult<DynamicDomainMutation> {
        let removed = self
            .state
            .lock()
            .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?
            .rules
            .len();
        let path = self.config.path.clone();
        let metadata_path = self.config.metadata_path.clone();
        let snapshot = spawn_isolated_build("dynamic_domain_set clear build", move || {
            let snapshot = build_snapshot::<Arc<str>>(&[])?;
            rewrite_rule_file::<Arc<str>>(&path, &[])?;
            if let Some(metadata_path) = metadata_path.as_deref() {
                rewrite_metadata_file(metadata_path, &BTreeMap::new())?;
            }
            Ok(snapshot)
        })
        .await?;
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
            state.rules.clear();
            state.known.clear();
            state.metadata.clear();
        }
        self.snapshot.store(Arc::new(snapshot));
        self.record_success();
        Ok(DynamicDomainMutation {
            added: 0,
            removed,
            total: 0,
        })
    }

    async fn reload_from_file(&self) -> DnsResult<DynamicDomainMutation> {
        let config = self.config.clone();
        let (rules, metadata, snapshot) =
            spawn_isolated_build("dynamic_domain_set reload build", move || {
                let rules = read_rule_file(&config.path)?;
                let metadata = config
                    .metadata_path
                    .as_deref()
                    .map(read_metadata_file)
                    .transpose()?
                    .unwrap_or_default();
                let snapshot = build_snapshot(&rules)?;
                Ok((rules, metadata, snapshot))
            })
            .await?;
        let total = rules.len();
        self.install_compiled_rules(rules, metadata, snapshot)?;
        Ok(DynamicDomainMutation {
            added: 0,
            removed: 0,
            total,
        })
    }

    async fn cleanup_expired_rules(&self) -> DnsResult<DynamicDomainMutation> {
        let Some(ttl_seconds) = self.config.entry_ttl_seconds else {
            return Ok(DynamicDomainMutation {
                added: 0,
                removed: 0,
                total: self.current_total()?,
            });
        };
        let now = AppClock::now_timestamp();
        let ttl_ms = ttl_seconds.saturating_mul(1000);
        let (retained, mut metadata, removed) = {
            let state = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
            let before = state.rules.len();
            let retained = state
                .rules
                .iter()
                .filter(|rule| {
                    state.metadata.get(rule.as_ref()).is_none_or(|metadata| {
                        metadata.origin == DynamicDomainRuleOrigin::Manual
                            || now.saturating_sub(metadata.created_at_ms) < ttl_ms
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            let removed = before.saturating_sub(retained.len());
            if removed == 0 {
                return Ok(DynamicDomainMutation {
                    added: 0,
                    removed: 0,
                    total: before,
                });
            }
            (retained, state.metadata.clone(), removed)
        };
        let retained_keys = retained
            .iter()
            .map(|rule| rule.as_ref())
            .collect::<HashSet<_>>();
        metadata.retain(|rule, _| retained_keys.contains(rule.as_str()));
        let committed_rules = retained.clone();
        let committed_metadata = metadata.clone();
        let total = retained.len();
        let path = self.config.path.clone();
        let metadata_path = self.config.metadata_path.clone();
        let snapshot = spawn_isolated_build("dynamic_domain_set cleanup build", move || {
            let snapshot = build_snapshot(&retained)?;
            rewrite_rule_file(&path, &retained)?;
            if let Some(metadata_path) = metadata_path.as_deref() {
                rewrite_metadata_file(metadata_path, &metadata)?;
            }
            Ok(snapshot)
        })
        .await?;
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
            state.known = committed_rules.iter().cloned().collect();
            state.rules = committed_rules;
            state.metadata = committed_metadata;
        }
        self.snapshot.store(Arc::new(snapshot));
        self.expired_total
            .fetch_add(removed as u64, Ordering::Relaxed);
        self.record_success();
        Ok(DynamicDomainMutation {
            added: 0,
            removed,
            total,
        })
    }

    async fn run_worker(self: Arc<Self>, mut rx: mpsc::Receiver<WorkerCommand>) {
        // The worker is the only task allowed to touch the rule file. This
        // keeps ordering simple: every mutating API call either waits behind
        // earlier appends or observes their flushed state before it runs.
        let mut pending = Vec::new();
        let mut interval =
            tokio::time::interval(Duration::from_millis(self.config.flush_interval_ms.max(1)));
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(
            self.config.cleanup_interval_seconds.max(1),
        ));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.flush_appends(&mut pending).await;
                }
                _ = cleanup_interval.tick() => {
                    self.flush_appends(&mut pending).await;
                    if let Err(err) = self.cleanup_expired_rules().await {
                        self.record_error(err.to_string());
                        warn!(plugin = %self.tag, error = %err, "dynamic_domain_set cleanup failed");
                    }
                }
                command = rx.recv() => {
                    let Some(command) = command else {
                        self.flush_appends(&mut pending).await;
                        break;
                    };
                    match command {
                        WorkerCommand::Append { rules, origin, wait } => {
                            let flush_now = wait.is_some();
                            match self.stage_new_rules(rules, origin) {
                                Ok(staged) if staged.rules.is_empty() => {
                                    if let Some(wait) = wait {
                                        let _ = wait.send(Ok(staged.mutation));
                                    }
                                }
                                Ok(staged) => {
                                    pending.push(PendingAppend {
                                        rules: staged.rules,
                                        wait,
                                    });
                                    let pending_count: usize = pending.iter().map(|item| item.rules.len()).sum();
                                    if flush_now || pending_count >= self.config.batch_size {
                                        self.flush_appends(&mut pending).await;
                                    }
                                }
                                Err(err) => {
                                    warn!(
                                        plugin = %self.tag,
                                        error = %err,
                                        "dynamic_domain_set append staging failed"
                                    );
                                    if let Some(wait) = wait {
                                        let _ = wait.send(Err(err));
                                    }
                                }
                            }
                        }
                        WorkerCommand::Remove { rules, wait } => {
                            // Full-file mutations must see all earlier appends
                            // first, otherwise a pending learned rule could be
                            // appended after a delete/clear/reload reordered it.
                            self.flush_appends(&mut pending).await;
                            let _ = wait.send(self.remove_rules(rules).await);
                        }
                        WorkerCommand::Clear { wait } => {
                            self.flush_appends(&mut pending).await;
                            let _ = wait.send(self.clear_rules().await);
                        }
                        WorkerCommand::Reload { wait } => {
                            self.flush_appends(&mut pending).await;
                            let _ = wait.send(self.reload_from_file().await);
                        }
                        WorkerCommand::Shutdown { done } => {
                            self.flush_appends(&mut pending).await;
                            let _ = done.send(());
                            break;
                        }
                    }
                }
            }
        }
    }

    pub(super) fn status(&self) -> DnsResult<DynamicDomainSetStatus> {
        let state = self
            .state
            .lock()
            .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
        let learned = state
            .metadata
            .values()
            .filter(|metadata| metadata.origin == DynamicDomainRuleOrigin::Learned)
            .count();
        let manual = state.rules.len().saturating_sub(learned);
        Ok(DynamicDomainSetStatus {
            ok: true,
            total: state.rules.len(),
            learned,
            manual,
            max_entries: self.config.max_entries,
            entry_ttl_seconds: self.config.entry_ttl_seconds,
            expired_total: self.expired_total.load(Ordering::Relaxed),
            capacity_rejected_total: self.capacity_rejected_total.load(Ordering::Relaxed),
            queue_rejected_total: self.queue_rejected_total.load(Ordering::Relaxed),
            last_success_at_ms: match self.last_success_at_ms.load(Ordering::Relaxed) {
                0 => None,
                value => Some(value),
            },
            last_error: self.last_error.lock().ok().and_then(|value| value.clone()),
        })
    }

    fn record_success(&self) {
        self.last_success_at_ms
            .store(AppClock::now_timestamp(), Ordering::Relaxed);
        if let Ok(mut error) = self.last_error.lock() {
            *error = None;
        }
    }

    fn record_error(&self, message: String) {
        if let Ok(mut error) = self.last_error.lock() {
            *error = Some(message);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DynamicDomainSetStatus {
    ok: bool,
    total: usize,
    learned: usize,
    manual: usize,
    max_entries: Option<usize>,
    entry_ttl_seconds: Option<u64>,
    expired_total: u64,
    capacity_rejected_total: u64,
    queue_rejected_total: u64,
    last_success_at_ms: Option<u64>,
    last_error: Option<String>,
}

/// Rules accepted into memory but not necessarily flushed to disk yet.
#[derive(Debug)]
struct StagedRules {
    mutation: DynamicDomainMutation,
    rules: Vec<Arc<str>>,
}

pub(super) fn build_snapshot<T: AsRef<str>>(rules: &[T]) -> DnsResult<DynamicDomainSetSnapshot> {
    let start_ms = AppClock::elapsed_millis();
    let mut matcher = DomainRuleMatcher::default();
    let mut full = 0usize;
    let mut keyword = 0usize;
    let mut regexp = 0usize;
    for rule in rules {
        match split_domain_rule_expression(rule.as_ref()).0 {
            DomainRuleKind::Full => full += 1,
            DomainRuleKind::Keyword => keyword += 1,
            DomainRuleKind::Regexp => regexp += 1,
            DomainRuleKind::Domain => {}
        }
    }
    matcher.reserve_rules(full, keyword, regexp);
    for (idx, rule) in rules.iter().enumerate() {
        let (kind, value) = split_domain_rule_expression(rule.as_ref());
        matcher.add_rule(kind, value, "").map_err(|error| {
            DnsError::plugin(format!("invalid dynamic_domain_set.rules[{idx}]: {error}"))
        })?;
    }
    matcher.finalize().map_err(DnsError::plugin)?;
    let elapsed_ms = AppClock::elapsed_millis().saturating_sub(start_ms);
    info!(
        rules = rules.len(),
        full_rules = matcher.full_rule_count(),
        domain_rules = matcher.trie_rule_count(),
        keyword_rules = matcher.keyword_rule_count(),
        regex_rules = matcher.regexp_rule_count(),
        elapsed_ms,
        "dynamic_domain_set snapshot built"
    );
    Ok(DynamicDomainSetSnapshot { matcher })
}
