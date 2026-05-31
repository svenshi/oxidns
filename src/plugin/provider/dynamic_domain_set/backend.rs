// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arc_swap::ArcSwap;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{info, warn};

#[cfg(feature = "api")]
use super::api::{RulesListResponse, register_api};
use super::config::DynamicDomainSetConfig;
use super::rules::{DynamicDomainMutation, DynamicDomainRuleKind, canonicalize_rules};
use super::storage::{append_rule_file, read_rule_file, rewrite_rule_file};
use crate::core::app_clock::AppClock;
use crate::core::error::{DnsError, Result as DnsResult};
use crate::core::rule_matcher::DomainRuleMatcher;
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
    rules: Vec<String>,
    known: HashSet<String>,
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
    rules: Vec<String>,
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
    config: DynamicDomainSetConfig,
    /// Canonical source of truth for ordered rules and duplicate checks.
    state: Mutex<RuleState>,
    /// Lock-free read side for matcher hot paths.
    snapshot: ArcSwap<DynamicDomainSetSnapshot>,
    /// Sender becomes available after `init`; stored so API/executor calls can
    /// enqueue work without owning the worker directly.
    tx: Mutex<Option<mpsc::Sender<WorkerCommand>>>,
    /// Joined during plugin destroy to flush pending appends before shutdown.
    worker_handle: Mutex<Option<JoinHandle<()>>>,
}

impl DynamicDomainSetBackend {
    pub(super) fn new(tag: String, config: DynamicDomainSetConfig) -> Self {
        Self {
            tag,
            config,
            state: Mutex::new(RuleState::default()),
            snapshot: ArcSwap::from_pointee(DynamicDomainSetSnapshot::default()),
            tx: Mutex::new(None),
            worker_handle: Mutex::new(None),
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
        self.bootstrap_file_if_needed()?;
        let rules = read_rule_file(&self.config.path)?;
        self.install_rules(rules)?;
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
        match tx.try_send(WorkerCommand::Append { rules, wait: None }) {
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
            Err(err) => Err(DnsError::plugin(format!(
                "dynamic_domain_set '{}' append queue failed: {}",
                self.tag, err
            ))),
        }
    }

    pub(crate) async fn append_rules_sync(
        &self,
        raw_rules: Vec<String>,
        default_kind: DynamicDomainRuleKind,
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
        let rules = state.rules[start..end].to_vec();
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

    fn bootstrap_file_if_needed(&self) -> DnsResult<()> {
        if self.config.path.exists() {
            return Ok(());
        }
        if let Some(parent) = self.config.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let rules = canonicalize_rules(
            self.config.bootstrap_rules.clone(),
            DynamicDomainRuleKind::Domain,
            "bootstrap_rules",
        )?;
        // Bootstrap writes canonical rules immediately so later API rewrites do
        // not have to preserve a separate "initial rules" concept.
        rewrite_rule_file(&self.config.path, &rules)?;
        Ok(())
    }

    fn install_rules(&self, rules: Vec<String>) -> DnsResult<DynamicDomainMutation> {
        let snapshot = build_snapshot(&rules)?;
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
        }
        self.snapshot.store(Arc::new(snapshot));
        Ok(DynamicDomainMutation {
            added: 0,
            removed: 0,
            total,
        })
    }

    fn stage_new_rules(&self, rules: Vec<String>) -> DnsResult<StagedRules> {
        let mut staged = Vec::new();
        let total = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
            for rule in rules {
                // Insert into both structures while holding one lock so the
                // ordered list and duplicate set cannot drift apart.
                if state.known.insert(rule.clone()) {
                    state.rules.push(rule.clone());
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

    fn rollback_staged_rules(&self, rules: &[String]) {
        if rules.is_empty() {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            for rule in rules {
                state.known.remove(rule);
            }
            state.rules.retain(|rule| !rules.iter().any(|v| v == rule));
        }
    }

    fn flush_appends(&self, pending: &mut Vec<PendingAppend>) {
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
        let result: DnsResult<(usize, DynamicDomainSetSnapshot)> = (|| {
            let rules = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?
                .rules
                .clone();
            let total = rules.len();
            let snapshot = build_snapshot(&rules)?;
            append_rule_file(&self.config.path, &appended_rules)?;
            Ok((total, snapshot))
        })();
        match result {
            Ok((total, snapshot)) => {
                self.snapshot.store(Arc::new(snapshot));
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
                for item in pending.drain(..) {
                    if let Some(wait) = item.wait {
                        let _ = wait.send(Err(DnsError::plugin(message.clone())));
                    }
                }
            }
        }
    }

    fn remove_rules(&self, rules: Vec<String>) -> DnsResult<DynamicDomainMutation> {
        // Hold the state lock until the rewrite succeeds. Append staging also
        // uses this lock, so this prevents a concurrently learned rule from
        // being inserted into state between the delete's read and commit.
        let (removed, total, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
            let before = state.rules.len();
            let remove_set = rules.iter().map(String::as_str).collect::<HashSet<_>>();
            let current_rules = state
                .rules
                .iter()
                .filter(|rule| !remove_set.contains(rule.as_str()))
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

            // Build the replacement snapshot before touching disk. If rule
            // compilation ever fails, the file, list API, and hot matcher all
            // keep serving the previous consistent state.
            let snapshot = build_snapshot(&current_rules)?;
            // Deletes rewrite the machine-managed file so removed rules cannot
            // reappear on the next provider reload. State is committed only
            // after this durable step succeeds.
            rewrite_rule_file(&self.config.path, &current_rules)?;
            state.known = current_rules.iter().cloned().collect();
            state.rules = current_rules;
            (removed, total, snapshot)
        };
        self.snapshot.store(Arc::new(snapshot));
        Ok(DynamicDomainMutation {
            added: 0,
            removed,
            total,
        })
    }

    fn clear_rules(&self) -> DnsResult<DynamicDomainMutation> {
        let (removed, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DnsError::runtime("dynamic_domain_set state lock poisoned"))?;
            let removed = state.rules.len();
            let snapshot = build_snapshot(&[])?;
            // Clear has the same consistency requirement as remove: keep the
            // old state and snapshot visible if the file rewrite fails.
            rewrite_rule_file(&self.config.path, &[])?;
            state.rules.clear();
            state.known.clear();
            (removed, snapshot)
        };
        self.snapshot.store(Arc::new(snapshot));
        Ok(DynamicDomainMutation {
            added: 0,
            removed,
            total: 0,
        })
    }

    fn reload_from_file(&self) -> DnsResult<DynamicDomainMutation> {
        let rules = read_rule_file(&self.config.path)?;
        let total = rules.len();
        self.install_rules(rules)?;
        Ok(DynamicDomainMutation {
            added: 0,
            removed: 0,
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
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.flush_appends(&mut pending);
                }
                command = rx.recv() => {
                    let Some(command) = command else {
                        self.flush_appends(&mut pending);
                        break;
                    };
                    match command {
                        WorkerCommand::Append { rules, wait } => {
                            let flush_now = wait.is_some();
                            match self.stage_new_rules(rules) {
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
                                        self.flush_appends(&mut pending);
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
                            self.flush_appends(&mut pending);
                            let _ = wait.send(self.remove_rules(rules));
                        }
                        WorkerCommand::Clear { wait } => {
                            self.flush_appends(&mut pending);
                            let _ = wait.send(self.clear_rules());
                        }
                        WorkerCommand::Reload { wait } => {
                            self.flush_appends(&mut pending);
                            let _ = wait.send(self.reload_from_file());
                        }
                        WorkerCommand::Shutdown { done } => {
                            self.flush_appends(&mut pending);
                            let _ = done.send(());
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// Rules accepted into memory but not necessarily flushed to disk yet.
#[derive(Debug)]
struct StagedRules {
    mutation: DynamicDomainMutation,
    rules: Vec<String>,
}

pub(super) fn build_snapshot(rules: &[String]) -> DnsResult<DynamicDomainSetSnapshot> {
    let start_ms = AppClock::elapsed_millis();
    let mut matcher = DomainRuleMatcher::default();
    for (idx, rule) in rules.iter().enumerate() {
        matcher
            .add_expression(rule, &format!("dynamic_domain_set.rules[{idx}]"))
            .map_err(DnsError::plugin)?;
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
