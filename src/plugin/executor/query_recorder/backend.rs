// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender as ReplySender, SyncSender, sync_channel};
use std::sync::{
    Arc, Mutex, MutexGuard, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tokio::sync::{Semaphore, broadcast};
use tracing::{error, info, warn};

use super::model::{PendingRecord, RecordDetail, ResolvedRecorderConfig, TableNames};
use super::store::{create_schema, open_writer_database, run_writer_thread, table_names};
use crate::infra::error::{DnsError, Result};

#[derive(Debug)]
pub(super) struct RecorderBackend {
    pub(super) observe_details: bool,
    pub(super) observations: crate::core::observation::ObservationStore,
    pub(super) observation_readers: Arc<Semaphore>,
    pub(super) tag: String,
    pub(super) path: PathBuf,
    pub(super) tables: TableNames,
    pub(super) queue_tx: SyncSender<WriterCommand>,
    pub(super) stop_requested: Arc<AtomicBool>,
    pub(super) writer_handle: Mutex<Option<JoinHandle<()>>>,
    pub(super) tail: Arc<Mutex<VecDeque<RecordDetail>>>,
    pub(super) memory_tail: usize,
    pub(super) broadcaster: broadcast::Sender<RecordDetail>,
    pub(super) dropped_total: Arc<AtomicU64>,
    pub(super) reader_semaphore: Arc<Semaphore>,
    pub(super) database_coordinator: Arc<DatabaseCoordinator>,
}

#[derive(Debug, Clone)]
pub(super) struct SpaceStats {
    pub(super) auto_vacuum: i64,
    pub(super) page_size: u64,
    pub(super) page_count: u64,
    pub(super) freelist_count: u64,
    pub(super) database_bytes: u64,
    pub(super) wal_bytes: u64,
}

impl SpaceStats {
    pub(super) fn total_bytes(&self) -> u64 {
        self.database_bytes.saturating_add(self.wal_bytes)
    }
}

#[derive(Debug, Clone)]
pub(super) struct SpaceReclaimResult {
    pub(super) before: SpaceStats,
    pub(super) reclaimable: SpaceStats,
    pub(super) after: SpaceStats,
    pub(super) migrated: bool,
    pub(super) peak_wal_bytes: u64,
}

impl SpaceReclaimResult {
    pub(super) fn reclaimed_bytes(&self) -> u64 {
        self.before
            .total_bytes()
            .saturating_sub(self.after.total_bytes())
    }
}

#[derive(Debug, Clone)]
pub(super) struct CleanupResult {
    pub(super) deleted_records: usize,
    pub(super) space: SpaceReclaimResult,
}

#[derive(Debug, Clone)]
pub(super) struct ClearHistoryResult {
    pub(super) cleared_records: usize,
    pub(super) space: SpaceReclaimResult,
}

pub(super) type CleanupReply = std::result::Result<CleanupResult, String>;
pub(super) type ClearHistoryReply = std::result::Result<ClearHistoryResult, String>;
#[cfg(test)]
pub(super) type FlushReply = std::result::Result<(), String>;

#[derive(Debug)]
pub(super) enum WriterCommand {
    Insert(Box<PendingRecord>),
    Cleanup {
        cutoff_ms: i64,
        reply_tx: ReplySender<CleanupReply>,
    },
    ClearHistory {
        reply_tx: ReplySender<ClearHistoryReply>,
    },
    #[cfg(test)]
    Flush {
        reply_tx: ReplySender<FlushReply>,
    },
}

#[derive(Debug)]
pub(super) struct WriterThreadContext {
    pub(super) path: PathBuf,
    pub(super) tables: TableNames,
    pub(super) stop_requested: Arc<AtomicBool>,
    pub(super) tail: Arc<Mutex<VecDeque<RecordDetail>>>,
    pub(super) memory_tail: usize,
    pub(super) broadcaster: broadcast::Sender<RecordDetail>,
    pub(super) batch_size: usize,
    pub(super) flush_interval: Duration,
    pub(super) database_coordinator: Arc<DatabaseCoordinator>,
}

#[derive(Debug, Default)]
pub(super) struct DatabaseCoordinator {
    access: RwLock<()>,
    writer: Mutex<()>,
}

impl DatabaseCoordinator {
    pub(super) fn read_access(&self) -> Result<RwLockReadGuard<'_, ()>> {
        self.access
            .read()
            .map_err(|_| DnsError::runtime("query_recorder database access lock poisoned"))
    }

    pub(super) fn write_access(&self) -> Result<RwLockWriteGuard<'_, ()>> {
        self.access
            .write()
            .map_err(|_| DnsError::runtime("query_recorder database access lock poisoned"))
    }

    pub(super) fn writer(&self) -> Result<MutexGuard<'_, ()>> {
        self.writer
            .lock()
            .map_err(|_| DnsError::runtime("query_recorder database writer lock poisoned"))
    }
}

static DATABASE_COORDINATORS: OnceLock<Mutex<HashMap<PathBuf, Weak<DatabaseCoordinator>>>> =
    OnceLock::new();

fn database_coordinator(path: &Path) -> Result<Arc<DatabaseCoordinator>> {
    let path = canonical_database_path(path)?;
    let registry = DATABASE_COORDINATORS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registry = registry
        .lock()
        .map_err(|_| DnsError::runtime("query_recorder coordinator registry lock poisoned"))?;
    registry.retain(|_, coordinator| coordinator.strong_count() > 0);
    if let Some(coordinator) = registry.get(&path).and_then(Weak::upgrade) {
        return Ok(coordinator);
    }
    let coordinator = Arc::new(DatabaseCoordinator::default());
    registry.insert(path, Arc::downgrade(&coordinator));
    Ok(coordinator)
}

fn canonical_database_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(std::fs::canonicalize(path)?);
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let canonical_parent = match parent {
        Some(parent) => std::fs::canonicalize(parent)?,
        None => std::env::current_dir()?,
    };
    let file_name = path
        .file_name()
        .ok_or_else(|| DnsError::plugin("query_recorder path must include a file name"))?;
    Ok(canonical_parent.join(file_name))
}

impl RecorderBackend {
    pub(super) fn run(tag: String, config: ResolvedRecorderConfig) -> Result<Arc<Self>> {
        if let Some(parent) = config.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|err| {
                DnsError::plugin(format!(
                    "failed to create query_recorder directory '{}': {}",
                    parent.display(),
                    err
                ))
            })?;
        }

        let database_coordinator = database_coordinator(&config.path)?;
        let (conn, tables) = {
            let _access = database_coordinator.write_access()?;
            let mut conn = open_writer_database(&config.path).map_err(|err| {
                format!(
                    "failed to open database '{}': {}",
                    config.path.display(),
                    err
                )
            })?;

            let tables = table_names(&tag);
            create_schema(&mut conn, &tables)?;
            // Refresh query planner stats once at startup; this is the cheap
            // version of ANALYZE and is the recommended way to keep indexes
            // selectable across schema upgrades.
            if let Err(err) = conn.execute_batch("PRAGMA optimize;") {
                warn!("query_recorder PRAGMA optimize failed at startup: {}", err);
            }
            (conn, tables)
        };

        let (queue_tx, queue_rx) = sync_channel(config.queue_size);
        let stop_requested = Arc::new(AtomicBool::new(false));
        let tail = Arc::new(Mutex::new(VecDeque::with_capacity(
            config.memory_tail.max(1),
        )));
        let (broadcaster, _) = broadcast::channel(config.memory_tail.max(16));
        let dropped_total = Arc::new(AtomicU64::new(0));
        let reader_semaphore = Arc::new(Semaphore::new(config.reader_concurrency));

        let writer_tables = tables.clone();
        let writer_path = config.path.clone();
        let writer_stop = stop_requested.clone();
        let writer_tail = tail.clone();
        let writer_broadcaster = broadcaster.clone();
        let memory_tail = config.memory_tail.max(1);
        let batch_size = config.batch_size;
        let flush_interval = Duration::from_millis(config.flush_interval_ms);
        let writer_database_coordinator = database_coordinator.clone();
        let writer_handle = thread::Builder::new()
            .name(format!("query-recorder-{}", tag))
            .spawn(move || {
                if let Err(err) = run_writer_thread(
                    WriterThreadContext {
                        path: writer_path,
                        tables: writer_tables,
                        stop_requested: writer_stop,
                        tail: writer_tail,
                        memory_tail,
                        broadcaster: writer_broadcaster,
                        batch_size,
                        flush_interval,
                        database_coordinator: writer_database_coordinator,
                    },
                    queue_rx,
                    conn,
                ) {
                    error!("query_recorder writer stopped: {}", err);
                }
            })?;

        Ok(Arc::new(Self {
            observe_details: config.observe_details,
            observations: crate::core::observation::ObservationStore::new(config.memory_tail),
            observation_readers: Arc::new(Semaphore::new(8)),
            tag,
            path: config.path,
            tables,
            queue_tx,
            stop_requested,
            writer_handle: Mutex::new(Some(writer_handle)),
            tail,
            memory_tail,
            broadcaster,
            dropped_total,
            reader_semaphore,
            database_coordinator,
        }))
    }

    pub(super) fn enqueue(&self, pending: PendingRecord) {
        if let Err(err) = self
            .queue_tx
            .try_send(WriterCommand::Insert(Box::new(pending)))
        {
            self.dropped_total.fetch_add(1, Ordering::Relaxed);
            warn!("query_recorder dropped record: {}", err);
        }
    }

    pub(super) fn cleanup(&self, cutoff_ms: i64) -> CleanupReply {
        let started = Instant::now();
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        self.queue_tx
            .send(WriterCommand::Cleanup {
                cutoff_ms,
                reply_tx,
            })
            .map_err(|err| format!("query_recorder cleanup enqueue failed: {err}"))?;
        let result = reply_rx
            .recv()
            .map_err(|err| format!("query_recorder cleanup reply failed: {err}"))?;
        if let Ok(result) = &result {
            log_space_reclaim(
                &self.tag,
                "periodic",
                result.deleted_records,
                &result.space,
                started.elapsed(),
            );
        }
        result
    }

    pub(super) fn clear_history(&self) -> ClearHistoryReply {
        let started = Instant::now();
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        self.queue_tx
            .send(WriterCommand::ClearHistory { reply_tx })
            .map_err(|err| format!("query_recorder clear enqueue failed: {err}"))?;
        let result = reply_rx
            .recv()
            .map_err(|err| format!("query_recorder clear reply failed: {err}"))?;
        if let Ok(result) = &result {
            log_space_reclaim(
                &self.tag,
                "manual",
                result.cleared_records,
                &result.space,
                started.elapsed(),
            );
        }
        result
    }

    #[cfg(test)]
    pub(super) fn flush_for_test(&self) -> FlushReply {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        self.queue_tx
            .send(WriterCommand::Flush { reply_tx })
            .map_err(|err| format!("query_recorder flush enqueue failed: {err}"))?;
        reply_rx
            .recv()
            .map_err(|err| format!("query_recorder flush reply failed: {err}"))?
    }
}

fn log_space_reclaim(
    tag: &str,
    operation: &str,
    deleted_records: usize,
    space: &SpaceReclaimResult,
    elapsed: Duration,
) {
    info!(
        query_recorder_tag = tag,
        operation,
        deleted_records,
        migrated = space.migrated,
        auto_vacuum_before = space.reclaimable.auto_vacuum,
        auto_vacuum_after = space.after.auto_vacuum,
        page_size = space.after.page_size,
        page_count_before = space.before.page_count,
        page_count_reclaimable = space.reclaimable.page_count,
        page_count_after = space.after.page_count,
        freelist_before = space.before.freelist_count,
        freelist_reclaimable = space.reclaimable.freelist_count,
        freelist_after = space.after.freelist_count,
        database_bytes_before = space.before.database_bytes,
        database_bytes_after = space.after.database_bytes,
        wal_bytes_before = space.before.wal_bytes,
        wal_bytes_peak = space.peak_wal_bytes,
        wal_bytes_after = space.after.wal_bytes,
        reclaimed_bytes = space.reclaimed_bytes(),
        elapsed_ms = elapsed.as_millis(),
        "query_recorder space reclaim completed"
    );
}
