// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use tokio::sync::broadcast;

use super::backend::{
    CleanupResult, ClearHistoryResult, DatabaseCoordinator, RecorderBackend, SpaceReclaimResult,
    SpaceStats, WriterCommand, WriterThreadContext,
};
#[cfg(test)]
use super::model::StepJson;
use super::model::{
    DistributionQuery, DistributionResponse, DistributionRow, LatencyHistogramBucket, LatencyQuery,
    LatencySlowRow, LatencySummary, ListCursor, ListQuery, PendingRecord, PluginStatsKind,
    PluginStatsRow, PluginsStatsQuery, QueryRecordFilter, QueryRecordStatus, RecordDetail,
    RecordRow, TableNames, TimeseriesPoint, TimeseriesQuery, TimeseriesResponse, TopBucketRow,
    TopBucketsResponse, TopQuery,
};
use super::persistence::{
    PreparedRecord, RECORD_COLUMNS, StoredRecord, assemble_records, insert_batch, load_steps,
    prepare_pending,
};
pub(super) use super::schema::create_schema;
use crate::infra::error::{DnsError, Result};

const SCHEMA_VERSION: &str = "v2";
const CLEANUP_BATCH_SIZE: usize = 1_000;
const VACUUM_BATCH_PAGES: u64 = 1_000;
const PLUGIN_STATS_SAMPLE_LIMIT: usize = 10_000;
pub(super) fn open_writer_database(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    // Tuned for the dedicated writer thread. Keep WAL and incremental vacuum
    // behavior, but avoid giving the single writer the same large read cache
    // and mmap footprint that used to be applied to every reader.
    // - auto_vacuum must be selected before WAL or schema creation for a fresh
    //   database; otherwise SQLite keeps the default NONE mode until a manual
    //   VACUUM rewrites the file.
    // - WAL + synchronous=NORMAL keeps the writer fast and readers
    //   non-blocking.
    conn.execute_batch(
        "PRAGMA auto_vacuum=INCREMENTAL;
         PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA temp_store=DEFAULT;
         PRAGMA cache_size=-4096;
         PRAGMA mmap_size=0;",
    )?;
    Ok(conn)
}

pub(super) fn open_reader_database(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    // Reader connections back WebUI list/stat/detail endpoints. They should
    // not reserve a large per-connection cache or mmap window, because several
    // dashboard requests can run at once against a large recorder database.
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA query_only=ON;
         PRAGMA temp_store=FILE;
         PRAGMA cache_size=-4096;
         PRAGMA mmap_size=0;",
    )?;
    Ok(conn)
}

pub(super) fn table_names(tag: &str) -> TableNames {
    let safe_tag = sanitize_tag(tag);
    let hash = fnv1a_hex(tag.as_bytes());
    let prefix = format!("qr_{}_{}_{}", safe_tag, hash, SCHEMA_VERSION);
    TableNames {
        records: format!("{prefix}_records"),
        traces: format!("{prefix}_traces"),
        trace_steps: format!("{prefix}_trace_steps"),
        question_sets: format!("{prefix}_question_sets"),
        question_items: format!("{prefix}_question_items"),
        meta: format!("{prefix}_meta"),
    }
}

fn record_row_select_columns(alias: Option<&str>) -> String {
    RECORD_COLUMNS
        .iter()
        .map(|column| match alias {
            Some(alias) => format!("{alias}.{column}"),
            None => (*column).to_string(),
        })
        .collect::<Vec<_>>()
        .join(",\n            ")
}

fn sanitize_tag(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len().max(1));
    for byte in tag.bytes() {
        let lower = byte.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() || lower == b'_' {
            out.push(lower as char);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

fn fnv1a_hex(input: &[u8]) -> String {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for byte in input {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01B3);
    }
    format!("{hash:016x}")
}

pub(super) fn run_writer_thread(
    context: WriterThreadContext,
    rx: Receiver<WriterCommand>,
    mut conn: Connection,
) -> Result<()> {
    let WriterThreadContext {
        path,
        tables,
        stop_requested,
        tail,
        memory_tail,
        broadcaster,
        batch_size,
        flush_interval,
        database_coordinator,
    } = context;

    let mut pending = Vec::with_capacity(batch_size);
    loop {
        match rx.recv_timeout(flush_interval) {
            Ok(WriterCommand::Insert(record)) => {
                pending.push(*record);
                if pending.len() >= batch_size {
                    flush_pending_coordinated(
                        &mut conn,
                        &tables,
                        &mut pending,
                        &tail,
                        memory_tail,
                        &broadcaster,
                        &database_coordinator,
                    )?;
                }
            }
            Ok(WriterCommand::Cleanup {
                cutoff_ms,
                reply_tx,
            }) => {
                let result = (|| {
                    let prepared = prepare_pending(&mut pending)?;
                    let _access = database_coordinator.write_access()?;
                    flush_prepared(
                        &mut conn,
                        &tables,
                        prepared,
                        &tail,
                        memory_tail,
                        &broadcaster,
                    )?;
                    run_cleanup(&mut conn, &path, &tables, cutoff_ms)
                })()
                .map_err(|err: DnsError| err.to_string());
                let _ = reply_tx.send(result);
            }
            Ok(WriterCommand::ClearHistory { reply_tx }) => {
                let result = (|| {
                    let prepared = prepare_pending(&mut pending)?;
                    let _access = database_coordinator.write_access()?;
                    flush_prepared(
                        &mut conn,
                        &tables,
                        prepared,
                        &tail,
                        memory_tail,
                        &broadcaster,
                    )?;
                    run_clear_history(&mut conn, &path, &tables, &tail)
                })()
                .map_err(|err: DnsError| err.to_string());
                let _ = reply_tx.send(result);
            }
            #[cfg(test)]
            Ok(WriterCommand::Flush { reply_tx }) => {
                let result = flush_pending_coordinated(
                    &mut conn,
                    &tables,
                    &mut pending,
                    &tail,
                    memory_tail,
                    &broadcaster,
                    &database_coordinator,
                )
                .map_err(|err| err.to_string());
                let _ = reply_tx.send(result);
            }
            Err(RecvTimeoutError::Timeout) => {
                flush_pending_coordinated(
                    &mut conn,
                    &tables,
                    &mut pending,
                    &tail,
                    memory_tail,
                    &broadcaster,
                    &database_coordinator,
                )?;
                if stop_requested.load(Ordering::Relaxed) {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                flush_pending_coordinated(
                    &mut conn,
                    &tables,
                    &mut pending,
                    &tail,
                    memory_tail,
                    &broadcaster,
                    &database_coordinator,
                )?;
                break;
            }
        }
    }

    Ok(())
}

fn flush_pending_coordinated(
    conn: &mut Connection,
    tables: &TableNames,
    pending: &mut Vec<PendingRecord>,
    tail: &Arc<Mutex<VecDeque<RecordDetail>>>,
    memory_tail: usize,
    broadcaster: &broadcast::Sender<RecordDetail>,
    database_coordinator: &DatabaseCoordinator,
) -> Result<()> {
    let prepared = prepare_pending(pending)?;
    let _access = database_coordinator.read_access()?;
    let _writer = database_coordinator.writer()?;
    flush_prepared(conn, tables, prepared, tail, memory_tail, broadcaster)
}

fn flush_prepared(
    conn: &mut Connection,
    tables: &TableNames,
    prepared: Vec<PreparedRecord>,
    tail: &Arc<Mutex<VecDeque<RecordDetail>>>,
    memory_tail: usize,
    broadcaster: &broadcast::Sender<RecordDetail>,
) -> Result<()> {
    if prepared.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    let ids = insert_batch(&tx, tables, &prepared)?;
    tx.commit()?;

    let mut tail_guard = tail
        .lock()
        .map_err(|_| "query_recorder tail buffer lock poisoned".to_string())?;
    for (prepared, id) in prepared.into_iter().zip(ids) {
        let mut detail = prepared.detail;
        detail.record.id = id;
        if tail_guard.len() >= memory_tail {
            tail_guard.pop_front();
        }
        tail_guard.push_back(detail.clone());
        let _ = broadcaster.send(detail);
    }
    Ok(())
}

#[cfg(test)]
fn insert_record(
    tx: &rusqlite::Transaction<'_>,
    tables: &TableNames,
    record: RecordRow,
    steps: Vec<StepJson>,
) -> Result<RecordDetail> {
    let mut prepared = PreparedRecord::new(record, steps)?;
    let id = insert_batch(tx, tables, std::slice::from_ref(&prepared))?[0];
    prepared.detail.record.id = id;
    Ok(prepared.detail)
}

// Record deletion and reclamation of only the affected dictionary entries
// share one transaction, so a failed batch cannot strand orphaned entries.
fn delete_batch(
    conn: &mut Connection,
    tables: &TableNames,
    cutoff_ms: Option<i64>,
) -> Result<usize> {
    let tx = conn.transaction()?;
    let predicate = if cutoff_ms.is_some() {
        "created_at_ms < ?1"
    } else {
        "1=1"
    };
    let sql = format!(
        "SELECT id,trace_id,question_set_id FROM {} WHERE {predicate} ORDER BY created_at_ms,id LIMIT {CLEANUP_BATCH_SIZE}",
        tables.records
    );
    let mut stmt = tx.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(cutoff_ms))?;
    let mut selected = Vec::new();
    while let Some(row) = rows.next()? {
        selected.push((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ));
    }
    drop(rows);
    drop(stmt);
    {
        let mut delete =
            tx.prepare_cached(&format!("DELETE FROM {} WHERE id=?1", tables.records))?;
        for (id, _, _) in &selected {
            delete.execute([id])?;
        }
    }
    for (dictionary, column, mut candidates) in [
        (
            &tables.traces,
            "trace_id",
            selected.iter().map(|r| r.1).collect::<Vec<_>>(),
        ),
        (
            &tables.question_sets,
            "question_set_id",
            selected.iter().map(|r| r.2).collect::<Vec<_>>(),
        ),
    ] {
        candidates.sort_unstable();
        candidates.dedup();
        let mut delete = tx.prepare_cached(&format!("DELETE FROM {dictionary} WHERE id=?1 AND NOT EXISTS (SELECT 1 FROM {} WHERE {column}=?1)",tables.records))?;
        for id in candidates {
            delete.execute([id])?;
        }
    }
    tx.commit()?;
    Ok(selected.len())
}

fn run_cleanup(
    conn: &mut Connection,
    path: &Path,
    tables: &TableNames,
    cutoff_ms: i64,
) -> Result<CleanupResult> {
    checkpoint_wal(conn)?;
    let before = read_space_stats(conn, path)?;
    let mut deleted_records = 0usize;
    let mut peak_wal_bytes = 0;
    loop {
        let deleted = delete_batch(conn, tables, Some(cutoff_ms))?;
        if deleted == 0 {
            break;
        }
        deleted_records = deleted_records.saturating_add(deleted);
        observe_wal_size(path, &mut peak_wal_bytes)?;
        checkpoint_wal(conn)?;
    }
    let reclaimable = read_space_stats(conn, path)?;
    let space = reclaim_database_space(conn, path, before, reclaimable, peak_wal_bytes)?;
    Ok(CleanupResult {
        deleted_records,
        space,
    })
}

fn run_clear_history(
    conn: &mut Connection,
    path: &Path,
    tables: &TableNames,
    tail: &Arc<Mutex<VecDeque<RecordDetail>>>,
) -> Result<ClearHistoryResult> {
    run_clear_history_with_checkpoint(conn, path, tables, tail, &mut checkpoint_wal)
}

fn run_clear_history_with_checkpoint<F>(
    conn: &mut Connection,
    path: &Path,
    tables: &TableNames,
    tail: &Arc<Mutex<VecDeque<RecordDetail>>>,
    checkpoint: &mut F,
) -> Result<ClearHistoryResult>
where
    F: FnMut(&Connection) -> Result<()>,
{
    // Start from an empty WAL and keep it bounded throughout the operation.
    // A single DELETE transaction for a large recorder can otherwise grow the
    // WAL close to the amount of history being removed before the final
    // checkpoint gets a chance to truncate it.
    checkpoint(conn)?;
    let before = read_space_stats(conn, path)?;
    let mut cleared_records = 0usize;
    let mut peak_wal_bytes = 0;
    loop {
        let deleted = delete_batch(conn, tables, None)?;
        if deleted == 0 {
            break;
        }
        cleared_records = cleared_records.saturating_add(deleted);
        // Deletes and dictionary reclamation commit per batch. Clear the
        // in-memory replay buffer before the next fallible checkpoint
        // so a partial clear can never advertise rows that no longer
        // exist in SQLite.
        clear_tail(tail);
        observe_wal_size(path, &mut peak_wal_bytes)?;
        checkpoint(conn)?;
    }

    clear_tail(tail);

    let reclaimable = read_space_stats(conn, path)?;
    let space =
        reclaim_database_space(conn, path, before, reclaimable, peak_wal_bytes).map_err(|err| {
            DnsError::runtime(format!(
                "query history cleared ({cleared_records} records), but space reclaim failed: {err}"
            ))
        })?;

    Ok(ClearHistoryResult {
        cleared_records,
        space,
    })
}

fn clear_tail(tail: &Arc<Mutex<VecDeque<RecordDetail>>>) {
    let mut tail_guard = tail.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    tail_guard.clear();
}

fn reclaim_database_space(
    conn: &Connection,
    path: &Path,
    before: SpaceStats,
    reclaimable: SpaceStats,
    mut peak_wal_bytes: u64,
) -> Result<SpaceReclaimResult> {
    let migrated = match reclaimable.auto_vacuum {
        0 => {
            conn.execute_batch("PRAGMA auto_vacuum=INCREMENTAL; VACUUM;")?;
            true
        }
        1 => false,
        2 => {
            run_incremental_vacuum(conn, path, &mut peak_wal_bytes)?;
            false
        }
        mode => {
            return Err(DnsError::runtime(format!(
                "query_recorder returned unsupported auto_vacuum mode {mode}"
            )));
        }
    };

    observe_wal_size(path, &mut peak_wal_bytes)?;
    checkpoint_wal(conn)?;

    let after = read_space_stats(conn, path)?;
    if migrated && after.auto_vacuum != 2 {
        return Err(DnsError::runtime(format!(
            "query_recorder legacy database migration did not enable incremental auto-vacuum (mode {})",
            after.auto_vacuum
        )));
    }
    if after.freelist_count != 0 {
        return Err(DnsError::runtime(format!(
            "query_recorder space reclaim left {} free pages",
            after.freelist_count
        )));
    }
    if reclaimable.freelist_count > 0 && after.page_count >= reclaimable.page_count {
        return Err(DnsError::runtime(format!(
            "query_recorder space reclaim made no page-count progress ({} pages, {} free)",
            after.page_count, reclaimable.freelist_count
        )));
    }

    Ok(SpaceReclaimResult {
        before,
        reclaimable,
        after,
        migrated,
        peak_wal_bytes,
    })
}

fn run_incremental_vacuum(conn: &Connection, path: &Path, peak_wal_bytes: &mut u64) -> Result<()> {
    // `PRAGMA incremental_vacuum` is a multi-step statement that yields one
    // zero-column row per reclaimed page. `Connection::execute_batch` only
    // steps a result-producing statement once. Reclaim a bounded number of
    // pages per statement and truncate the WAL between batches so a manual
    // clear cannot trade a smaller main file for an unbounded WAL peak.
    loop {
        let before = pragma_u64(conn, "PRAGMA freelist_count")?;
        if before == 0 {
            return Ok(());
        }

        let mut statement =
            conn.prepare(&format!("PRAGMA incremental_vacuum({VACUUM_BATCH_PAGES})"))?;
        let mut rows = statement.query([])?;
        while rows.next()?.is_some() {}
        drop(rows);
        drop(statement);
        observe_wal_size(path, peak_wal_bytes)?;
        checkpoint_wal(conn)?;

        let after = pragma_u64(conn, "PRAGMA freelist_count")?;
        if after >= before {
            return Err(DnsError::runtime(format!(
                "query_recorder incremental vacuum made no progress ({after} of {before} free pages remain)"
            )));
        }
    }
}

fn checkpoint_wal(conn: &Connection) -> Result<()> {
    let (busy, _log_frames, _checkpointed_frames) =
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
    if busy != 0 {
        return Err(DnsError::runtime(format!(
            "query_recorder WAL checkpoint remained busy ({busy})"
        )));
    }
    Ok(())
}

fn observe_wal_size(path: &Path, peak_wal_bytes: &mut u64) -> Result<()> {
    *peak_wal_bytes = (*peak_wal_bytes).max(file_size(&wal_path(path))?);
    Ok(())
}

fn read_space_stats(conn: &Connection, path: &Path) -> Result<SpaceStats> {
    let auto_vacuum = conn.query_row("PRAGMA auto_vacuum", [], |row| row.get::<_, i64>(0))?;
    let page_size = pragma_u64(conn, "PRAGMA page_size")?;
    let page_count = pragma_u64(conn, "PRAGMA page_count")?;
    let freelist_count = pragma_u64(conn, "PRAGMA freelist_count")?;
    Ok(SpaceStats {
        auto_vacuum,
        page_size,
        page_count,
        freelist_count,
        database_bytes: file_size(path)?,
        wal_bytes: file_size(&wal_path(path))?,
    })
}

fn pragma_u64(conn: &Connection, pragma: &str) -> Result<u64> {
    let value = conn.query_row(pragma, [], |row| row.get::<_, i64>(0))?;
    non_negative_u64(value).map_err(Into::into)
}

fn file_size(path: &Path) -> Result<u64> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(err.into()),
    }
}

fn wal_path(path: &Path) -> PathBuf {
    let mut path = path.as_os_str().to_os_string();
    path.push("-wal");
    PathBuf::from(path)
}

pub(super) fn query_records(
    backend: Arc<RecorderBackend>,
    query: ListQuery,
) -> std::result::Result<(Vec<RecordRow>, Option<String>), DnsError> {
    let conn = open_reader_database(&backend.path)?;
    let (mut clauses, mut params) = record_filter_clauses(
        "r",
        &backend.tables,
        query.since_ms,
        query.until_ms,
        &query.filter,
    )?;
    if let Some(cursor) = query.cursor {
        clauses.push("(r.created_at_ms < ? OR (r.created_at_ms = ? AND r.id < ?))".to_string());
        params.push(Value::Integer(cursor.created_at_ms));
        params.push(Value::Integer(cursor.created_at_ms));
        params.push(Value::Integer(cursor.id));
    }
    let where_sql = join_clauses(&clauses);
    params.push(Value::Integer(query.limit.saturating_add(1) as i64));

    let row_columns = record_row_select_columns(Some("r"));
    let sql = format!(
        "SELECT
            {row_columns}
         FROM {records} r
         WHERE {where_sql}
         ORDER BY r.created_at_ms DESC, r.id DESC
         LIMIT ?",
        records = backend.tables.records
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;

    let mut records = Vec::new();
    while let Some(row) = rows.next()? {
        records.push(StoredRecord::read(row)?);
    }

    let has_more = records.len() > query.limit;
    if has_more {
        records.truncate(query.limit);
    }
    let next_cursor = if has_more {
        records.last().map(|record| {
            encode_cursor(ListCursor {
                created_at_ms: record.created_at_ms,
                id: record.id,
            })
        })
    } else {
        None
    };
    drop(rows);
    drop(stmt);
    Ok((
        assemble_records(&conn, &backend.tables, records)?,
        next_cursor,
    ))
}

pub(super) fn load_record_detail(
    backend: Arc<RecorderBackend>,
    record_id: i64,
) -> std::result::Result<Option<RecordDetail>, DnsError> {
    let conn = open_reader_database(&backend.path)?;
    let row_columns = record_row_select_columns(None);
    let record_sql = format!(
        "SELECT
            {row_columns}
         FROM {records}
         WHERE id = ?1",
        records = backend.tables.records
    );

    let record = conn
        .prepare(&record_sql)?
        .query_row(params![record_id], StoredRecord::read)
        .optional()?;

    let Some(record) = record else {
        return Ok(None);
    };

    let steps = load_steps(&conn, &backend.tables, record.trace_id)?;
    let record = assemble_records(&conn, &backend.tables, vec![record])?
        .pop()
        .ok_or_else(|| DnsError::plugin("query_recorder missing record during assembly"))?;
    Ok(Some(RecordDetail { record, steps }))
}

pub(super) fn load_plugin_stats(
    backend: Arc<RecorderBackend>,
    query: PluginsStatsQuery,
) -> std::result::Result<(u64, Vec<PluginStatsRow>), DnsError> {
    let conn = open_reader_database(&backend.path)?;
    let (clauses, mut params) = record_filter_clauses(
        "r",
        &backend.tables,
        query.since_ms,
        query.until_ms,
        &query.filter,
    )?;
    let where_sql = join_clauses(&clauses);
    // Applied in the step_agg WHERE clause so SQLite can use the
    // (kind, tag, outcome, trace_id) covering index with a leading
    // kind= equality rather than a per-record nested lookup.
    let kind_where_filter = if query.kind == PluginStatsKind::All {
        String::new()
    } else {
        "AND s.kind = ?".to_string()
    };
    params.push(Value::Integer(PLUGIN_STATS_SAMPLE_LIMIT as i64));
    if query.kind != PluginStatsKind::All {
        params.push(Value::Text(query.kind.sql_value().to_string()));
    }
    // Join through each sampled record so shared paths retain per-query event
    // multiplicity, while query_hits counts each record only once.
    let sql = format!(
        "WITH sample_records AS (
            SELECT r.id, r.trace_id
            FROM {records} r
            WHERE {where_sql}
            ORDER BY r.created_at_ms DESC, r.id DESC
            LIMIT ?
         ),
         totals AS (
            SELECT COUNT(*) AS total_records FROM sample_records
         ),
         step_agg AS (
            SELECT
                s.kind,
                s.tag,
                SUM(CASE
                    WHEN s.kind = 'matcher'
                     AND s.outcome IN (
                         'matched', 'not_matched',
                         'always_true_matched', 'always_true_not_matched',
                         'always_false_matched', 'always_false_not_matched'
                     ) THEN 1
                    ELSE 0
                END) AS checked,
                SUM(CASE
                    WHEN s.kind = 'matcher'
                     AND s.outcome IN (
                         'matched', 'always_true_matched', 'always_false_matched'
                     ) THEN 1
                    ELSE 0
                END) AS matched,
                SUM(CASE
                    WHEN s.kind = 'executor' AND s.outcome = 'entered' THEN 1
                    WHEN s.kind = 'builtin' THEN 1
                    ELSE 0
                END) AS executed,
                COUNT(DISTINCT r.id) AS query_hits
            FROM sample_records r
            JOIN {steps} s ON s.trace_id = r.trace_id
            WHERE 1 = 1
            {kind_where_filter}
            GROUP BY s.kind, s.tag
         )
         SELECT
            totals.total_records,
            sa.kind,
            sa.tag,
            sa.checked,
            sa.matched,
            sa.executed,
            sa.query_hits
         FROM totals
         LEFT JOIN step_agg sa ON 1 = 1
         ORDER BY sa.kind ASC, sa.query_hits DESC, sa.tag ASC",
        steps = backend.tables.trace_steps,
        records = backend.tables.records,
        kind_where_filter = kind_where_filter
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;

    let mut total_records = 0u64;
    let mut stats = Vec::new();
    while let Some(row) = rows.next()? {
        total_records = row.get::<_, i64>(0).and_then(non_negative_u64)?;
        let Some(kind) = row.get::<_, Option<String>>(1)? else {
            continue;
        };
        let query_hits = row.get::<_, i64>(6).and_then(non_negative_u64)?;
        stats.push(PluginStatsRow {
            kind,
            tag: row.get(2)?,
            checked: row
                .get::<_, i64>(3)
                .and_then(non_negative_u64)
                .map_err(|err| DnsError::plugin(format!("invalid plugin stats checked: {err}")))?,
            matched: row.get::<_, i64>(4).and_then(non_negative_u64)?,
            executed: row.get::<_, i64>(5).and_then(non_negative_u64)?,
            query_total: query_hits,
            query_share: if total_records == 0 {
                0.0
            } else {
                query_hits as f64 / total_records as f64
            },
        });
    }
    Ok((total_records, stats))
}

pub(super) fn load_top_clients(
    backend: Arc<RecorderBackend>,
    query: TopQuery,
) -> std::result::Result<TopBucketsResponse, DnsError> {
    let conn = open_reader_database(&backend.path)?;
    let (clauses, mut params) = record_filter_clauses(
        "r",
        &backend.tables,
        query.since_ms,
        query.until_ms,
        &query.filter,
    )?;
    let where_sql = join_clauses(&clauses);
    params.push(Value::Integer(PLUGIN_STATS_SAMPLE_LIMIT as i64));
    params.push(Value::Integer(limit_to_i64(query.limit)?));

    let sql = format!(
        "WITH sample_records AS (
            SELECT r.id, r.client_ip
            FROM {records} r
            WHERE {where_sql}
            ORDER BY r.created_at_ms DESC, r.id DESC
            LIMIT ?
         ),
         totals AS (
            SELECT COUNT(*) AS sample_size FROM sample_records
         )
         SELECT totals.sample_size, sample_records.client_ip, COUNT(*) AS count
         FROM totals
         LEFT JOIN sample_records ON 1 = 1
         GROUP BY totals.sample_size, sample_records.client_ip
         ORDER BY count DESC, sample_records.client_ip ASC
         LIMIT ?",
        records = backend.tables.records,
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut sample_size = 0u64;
    let mut bucket_rows: Vec<TopBucketRow> = Vec::new();
    while let Some(row) = rows.next()? {
        sample_size = row.get::<_, i64>(0).and_then(non_negative_u64)?;
        let Some(client_ip) = row.get::<_, Option<String>>(1)? else {
            continue;
        };
        let count = row.get::<_, i64>(2).and_then(non_negative_u64)?;
        let share = bucket_share(count, sample_size);
        bucket_rows.push(TopBucketRow {
            key: client_ip,
            count,
            share,
        });
    }
    Ok(TopBucketsResponse {
        ok: true,
        sample_size,
        rows: bucket_rows,
    })
}

pub(super) fn load_top_qnames(
    backend: Arc<RecorderBackend>,
    query: TopQuery,
) -> std::result::Result<TopBucketsResponse, DnsError> {
    let conn = open_reader_database(&backend.path)?;
    let (clauses, mut params) = record_filter_clauses(
        "r",
        &backend.tables,
        query.since_ms,
        query.until_ms,
        &query.filter,
    )?;
    let where_sql = join_clauses(&clauses);
    params.push(Value::Integer(PLUGIN_STATS_SAMPLE_LIMIT as i64));
    params.push(Value::Integer(limit_to_i64(query.limit)?));

    let sql = format!(
        "WITH sample_records AS (
            SELECT r.id, r.question_set_id
            FROM {records} r
            WHERE {where_sql}
            ORDER BY r.created_at_ms DESC, r.id DESC
            LIMIT ?
         ),
         totals AS (
            SELECT COUNT(*) AS sample_size FROM sample_records
         )
         SELECT
            totals.sample_size,
            q.name_lc AS qname,
            COUNT(q.name_lc) AS count
         FROM totals
         LEFT JOIN sample_records ON 1 = 1
         LEFT JOIN {questions} q ON q.question_set_id = sample_records.question_set_id
         GROUP BY totals.sample_size, qname
         ORDER BY count DESC, qname ASC
         LIMIT ?",
        records = backend.tables.records,
        questions = backend.tables.question_items,
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut sample_size = 0u64;
    let mut bucket_rows: Vec<TopBucketRow> = Vec::new();
    while let Some(row) = rows.next()? {
        sample_size = row.get::<_, i64>(0).and_then(non_negative_u64)?;
        let Some(qname) = row.get::<_, Option<String>>(1)? else {
            continue;
        };
        let count = row.get::<_, i64>(2).and_then(non_negative_u64)?;
        let share = bucket_share(count, sample_size);
        bucket_rows.push(TopBucketRow {
            key: qname,
            count,
            share,
        });
    }
    Ok(TopBucketsResponse {
        ok: true,
        sample_size,
        rows: bucket_rows,
    })
}

pub(super) fn load_qtype_distribution(
    backend: Arc<RecorderBackend>,
    query: DistributionQuery,
) -> std::result::Result<DistributionResponse, DnsError> {
    let conn = open_reader_database(&backend.path)?;
    let (clauses, mut params) = record_filter_clauses(
        "r",
        &backend.tables,
        query.since_ms,
        query.until_ms,
        &query.filter,
    )?;
    let where_sql = join_clauses(&clauses);
    params.push(Value::Integer(PLUGIN_STATS_SAMPLE_LIMIT as i64));

    let sql = format!(
        "WITH sample_records AS (
            SELECT r.id, r.question_set_id
            FROM {records} r
            WHERE {where_sql}
            ORDER BY r.created_at_ms DESC, r.id DESC
            LIMIT ?
         ),
         totals AS (
            SELECT COUNT(*) AS sample_size FROM sample_records
         )
         SELECT
            totals.sample_size,
            q.qtype,
            COUNT(q.qtype) AS count
         FROM totals
         LEFT JOIN sample_records ON 1 = 1
         LEFT JOIN {questions} q ON q.question_set_id = sample_records.question_set_id
         GROUP BY totals.sample_size, qtype
         ORDER BY count DESC, qtype ASC",
        records = backend.tables.records,
        questions = backend.tables.question_items,
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut sample_size = 0u64;
    let mut distribution_rows: Vec<DistributionRow> = Vec::new();
    while let Some(row) = rows.next()? {
        sample_size = row.get::<_, i64>(0).and_then(non_negative_u64)?;
        let Some(qtype) = row.get::<_, Option<String>>(1)? else {
            continue;
        };
        let count = row.get::<_, i64>(2).and_then(non_negative_u64)?;
        let share = bucket_share(count, sample_size);
        distribution_rows.push(DistributionRow {
            key: qtype,
            count,
            share,
        });
    }
    Ok(DistributionResponse {
        ok: true,
        sample_size,
        rows: distribution_rows,
    })
}

pub(super) fn load_rcode_distribution(
    backend: Arc<RecorderBackend>,
    query: DistributionQuery,
) -> std::result::Result<DistributionResponse, DnsError> {
    let conn = open_reader_database(&backend.path)?;
    let (clauses, mut params) = record_filter_clauses(
        "r",
        &backend.tables,
        query.since_ms,
        query.until_ms,
        &query.filter,
    )?;
    let where_sql = join_clauses(&clauses);
    params.push(Value::Integer(PLUGIN_STATS_SAMPLE_LIMIT as i64));

    let sql = format!(
        "WITH sample_records AS (
            SELECT r.id, r.rcode, r.error, r.has_response
            FROM {records} r
            WHERE {where_sql}
            ORDER BY r.created_at_ms DESC, r.id DESC
            LIMIT ?
         ),
         totals AS (
            SELECT COUNT(*) AS sample_size FROM sample_records
         )
         SELECT
            totals.sample_size,
            CASE
                WHEN sample_records.rcode IS NOT NULL THEN sample_records.rcode
                WHEN sample_records.error IS NOT NULL THEN '_ERROR'
                WHEN sample_records.has_response = 0 THEN '_NO_RESPONSE'
                ELSE '_UNKNOWN'
            END AS bucket,
            COUNT(*) AS count
         FROM totals
         LEFT JOIN sample_records ON 1 = 1
         GROUP BY totals.sample_size, bucket
         ORDER BY count DESC, bucket ASC",
        records = backend.tables.records,
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut sample_size = 0u64;
    let mut distribution_rows: Vec<DistributionRow> = Vec::new();
    while let Some(row) = rows.next()? {
        sample_size = row.get::<_, i64>(0).and_then(non_negative_u64)?;
        let Some(bucket) = row.get::<_, Option<String>>(1)? else {
            continue;
        };
        let count = row.get::<_, i64>(2).and_then(non_negative_u64)?;
        let share = bucket_share(count, sample_size);
        distribution_rows.push(DistributionRow {
            key: bucket,
            count,
            share,
        });
    }
    Ok(DistributionResponse {
        ok: true,
        sample_size,
        rows: distribution_rows,
    })
}

pub(super) fn load_latency_summary(
    backend: Arc<RecorderBackend>,
    query: LatencyQuery,
) -> std::result::Result<LatencySummary, DnsError> {
    let conn = open_reader_database(&backend.path)?;
    let (clauses, mut params) = record_filter_clauses(
        "r",
        &backend.tables,
        query.since_ms,
        query.until_ms,
        &query.filter,
    )?;
    let where_sql = join_clauses(&clauses);
    params.push(Value::Integer(PLUGIN_STATS_SAMPLE_LIMIT as i64));
    let elapsed_sql = format!(
        "SELECT r.elapsed_ms
         FROM {records} r
         WHERE {where_sql}
         ORDER BY r.created_at_ms DESC, r.id DESC
         LIMIT ?",
        records = backend.tables.records,
    );

    let mut elapsed_values: Vec<u64> = Vec::new();
    {
        let mut stmt = conn.prepare(&elapsed_sql)?;
        let mut rows = stmt.query(params_from_iter(params.clone()))?;
        while let Some(row) = rows.next()? {
            let value = row.get::<_, i64>(0).and_then(non_negative_u64)?;
            elapsed_values.push(value);
        }
    }

    let sample_size = elapsed_values.len() as u64;
    let (avg_ms, p50_ms, p95_ms, p99_ms, max_ms) = latency_percentiles(&mut elapsed_values);
    let histogram = latency_histogram(&elapsed_values);

    let slow_limit = query.slow_limit;
    let (slow_clauses, mut slow_params) = record_filter_clauses(
        "r",
        &backend.tables,
        query.since_ms,
        query.until_ms,
        &query.filter,
    )?;
    let slow_where_sql = join_clauses(&slow_clauses);
    slow_params.push(Value::Integer(PLUGIN_STATS_SAMPLE_LIMIT as i64));
    slow_params.push(Value::Integer(limit_to_i64(slow_limit)?));
    let slow_sql = format!(
        "WITH sample_records AS (
            SELECT r.id, r.question_set_id, r.elapsed_ms
            FROM {records} r
            WHERE {where_sql}
            ORDER BY r.created_at_ms DESC, r.id DESC
            LIMIT ?
         )
         SELECT
            q.name_lc AS qname,
            COUNT(*) AS count,
            AVG(sample_records.elapsed_ms) AS avg_ms,
            MAX(sample_records.elapsed_ms) AS max_ms
         FROM sample_records
         JOIN {questions} q ON q.question_set_id = sample_records.question_set_id
         GROUP BY qname
         HAVING qname IS NOT NULL
         ORDER BY avg_ms DESC, count DESC
         LIMIT ?",
        records = backend.tables.records,
        questions = backend.tables.question_items,
        where_sql = slow_where_sql,
    );
    let mut slow_top: Vec<LatencySlowRow> = Vec::new();
    {
        let mut stmt = conn.prepare(&slow_sql)?;
        let mut rows = stmt.query(params_from_iter(slow_params))?;
        while let Some(row) = rows.next()? {
            let Some(qname) = row.get::<_, Option<String>>(0)? else {
                continue;
            };
            slow_top.push(LatencySlowRow {
                qname,
                count: row.get::<_, i64>(1).and_then(non_negative_u64)?,
                avg_ms: row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                max_ms: row.get::<_, i64>(3).and_then(non_negative_u64)?,
            });
        }
    }

    Ok(LatencySummary {
        ok: true,
        sample_size,
        avg_ms,
        p50_ms,
        p95_ms,
        p99_ms,
        max_ms,
        histogram,
        slow_top,
    })
}

pub(super) fn load_timeseries(
    backend: Arc<RecorderBackend>,
    query: TimeseriesQuery,
) -> std::result::Result<TimeseriesResponse, DnsError> {
    let conn = open_reader_database(&backend.path)?;
    let (clauses, mut params) = record_filter_clauses(
        "r",
        &backend.tables,
        query.since_ms,
        query.until_ms,
        &query.filter,
    )?;
    let where_sql = join_clauses(&clauses);
    params.push(Value::Integer(PLUGIN_STATS_SAMPLE_LIMIT as i64));

    let bucket_ms = query.bucket.millis();
    let sql = format!(
        "SELECT r.created_at_ms, r.elapsed_ms, r.error, r.has_response
         FROM {records} r
         WHERE {where_sql}
         ORDER BY r.created_at_ms DESC, r.id DESC
         LIMIT ?",
        records = backend.tables.records,
    );

    #[derive(Default)]
    struct Aggregator {
        total: u64,
        error_count: u64,
        no_response_count: u64,
        elapsed_sum: u64,
        elapsed_values: Vec<u64>,
    }
    let mut buckets: std::collections::BTreeMap<i64, Aggregator> =
        std::collections::BTreeMap::new();
    let mut sample_size = 0u64;

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    while let Some(row) = rows.next()? {
        let created_at_ms = row.get::<_, i64>(0)?;
        let elapsed_ms = row.get::<_, i64>(1).and_then(non_negative_u64)?;
        let error = row.get::<_, Option<String>>(2)?;
        let has_response = row.get::<_, i64>(3)? != 0;
        let bucket = bucket_floor(created_at_ms, bucket_ms);
        let aggregator = buckets.entry(bucket).or_default();
        aggregator.total = aggregator.total.saturating_add(1);
        if error.is_some() {
            aggregator.error_count = aggregator.error_count.saturating_add(1);
        }
        if error.is_none() && !has_response {
            aggregator.no_response_count = aggregator.no_response_count.saturating_add(1);
        }
        aggregator.elapsed_sum = aggregator.elapsed_sum.saturating_add(elapsed_ms);
        aggregator.elapsed_values.push(elapsed_ms);
        sample_size = sample_size.saturating_add(1);
    }

    let mut points: Vec<TimeseriesPoint> = Vec::with_capacity(buckets.len());
    for (bucket, mut aggregator) in buckets {
        let avg_ms = if aggregator.total == 0 {
            0.0
        } else {
            aggregator.elapsed_sum as f64 / aggregator.total as f64
        };
        let p95_ms = percentile_value(&mut aggregator.elapsed_values, 0.95);
        points.push(TimeseriesPoint {
            bucket_ms: bucket,
            total: aggregator.total,
            error_count: aggregator.error_count,
            no_response_count: aggregator.no_response_count,
            avg_ms,
            p95_ms,
        });
    }
    if points.len() > query.max_buckets {
        let drop = points.len() - query.max_buckets;
        points.drain(0..drop);
    }

    Ok(TimeseriesResponse {
        ok: true,
        sample_size,
        bucket_ms,
        points,
    })
}

fn bucket_share(count: u64, sample_size: u64) -> f64 {
    if sample_size == 0 {
        0.0
    } else {
        count as f64 / sample_size as f64
    }
}

fn bucket_floor(created_at_ms: i64, bucket_ms: i64) -> i64 {
    if bucket_ms <= 0 {
        return created_at_ms;
    }
    let remainder = created_at_ms.rem_euclid(bucket_ms);
    created_at_ms - remainder
}

fn latency_percentiles(values: &mut [u64]) -> (f64, u64, u64, u64, u64) {
    if values.is_empty() {
        return (0.0, 0, 0, 0, 0);
    }
    values.sort_unstable();
    let avg = values.iter().copied().sum::<u64>() as f64 / values.len() as f64;
    let p50 = percentile_of_sorted(values, 0.50);
    let p95 = percentile_of_sorted(values, 0.95);
    let p99 = percentile_of_sorted(values, 0.99);
    let max = *values.last().unwrap_or(&0);
    (avg, p50, p95, p99, max)
}

fn percentile_value(values: &mut [u64], quantile: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    percentile_of_sorted(values, quantile)
}

fn percentile_of_sorted(sorted_values: &[u64], quantile: f64) -> u64 {
    if sorted_values.is_empty() {
        return 0;
    }
    let clamped = quantile.clamp(0.0, 1.0);
    let max_index = sorted_values.len() - 1;
    let rank = clamped * max_index as f64;
    let index = rank.round() as usize;
    let index = index.min(max_index);
    sorted_values[index]
}

const LATENCY_BUCKET_EDGES_MS: [u64; 6] = [10, 20, 50, 100, 300, 1000];

fn latency_histogram(values: &[u64]) -> Vec<LatencyHistogramBucket> {
    let mut counts = vec![0u64; LATENCY_BUCKET_EDGES_MS.len() + 1];
    for value in values {
        let mut placed = false;
        for (index, edge) in LATENCY_BUCKET_EDGES_MS.iter().enumerate() {
            if *value < *edge {
                counts[index] = counts[index].saturating_add(1);
                placed = true;
                break;
            }
        }
        if !placed {
            *counts.last_mut().expect("at least one bucket") =
                counts.last().copied().unwrap_or(0).saturating_add(1);
        }
    }
    let mut histogram = Vec::with_capacity(counts.len());
    for (index, count) in counts.into_iter().enumerate() {
        let lt_ms = LATENCY_BUCKET_EDGES_MS.get(index).copied();
        histogram.push(LatencyHistogramBucket { lt_ms, count });
    }
    histogram
}

fn record_filter_clauses(
    alias: &str,
    tables: &TableNames,
    since_ms: Option<u64>,
    until_ms: Option<u64>,
    filter: &QueryRecordFilter,
) -> std::result::Result<(Vec<String>, Vec<Value>), DnsError> {
    let mut clauses = Vec::new();
    let mut params = Vec::new();

    if let Some(since_ms) = since_ms {
        clauses.push(format!("{alias}.created_at_ms >= ?"));
        params.push(Value::Integer(as_i64(since_ms)?));
    }
    if let Some(until_ms) = until_ms {
        clauses.push(format!("{alias}.created_at_ms <= ?"));
        params.push(Value::Integer(as_i64(until_ms)?));
    }
    if let Some(matcher_tag) = filter.matcher_tag.as_deref() {
        // Keep dictionary membership uncorrelated; rare matches must not
        // re-evaluate an event lookup for every candidate record.
        clauses.push(format!(
            "{alias}.trace_id IN (
                SELECT s.trace_id FROM {steps} s
                WHERE s.kind = 'matcher'
                  AND s.outcome IN ('matched', 'always_true_matched', 'always_false_matched')
                  AND s.tag = ?
            )",
            steps = tables.trace_steps,
        ));
        params.push(Value::Text(matcher_tag.to_string()));
    }
    if let Some(qname) = filter.qname.as_deref() {
        clauses.push(format!(
            "{alias}.question_set_id IN (
                SELECT q.question_set_id
                FROM {questions} q
                WHERE q.name_lc LIKE ? ESCAPE '\\'
            )",
            questions = tables.question_items,
        ));
        params.push(Value::Text(like_pattern(&qname.to_ascii_lowercase())));
    }
    if let Some(qtype) = filter.qtype.as_deref() {
        clauses.push(format!(
            "{alias}.question_set_id IN (
                SELECT q.question_set_id
                FROM {questions} q
                WHERE q.qtype = ?
            )",
            questions = tables.question_items,
        ));
        params.push(Value::Text(qtype.to_ascii_uppercase()));
    }
    if let Some(client_ip) = filter.client_ip.as_deref() {
        clauses.push(format!(
            "LOWER({alias}.client_ip) LIKE LOWER(?) ESCAPE '\\'"
        ));
        params.push(Value::Text(like_pattern(client_ip)));
    }
    if let Some(rcode) = filter.rcode.as_deref() {
        clauses.push(format!("{alias}.rcode = ? COLLATE NOCASE"));
        params.push(Value::Text(rcode.to_string()));
    }
    match filter.status {
        QueryRecordStatus::All => {}
        QueryRecordStatus::Error => clauses.push(format!("{alias}.error IS NOT NULL")),
        QueryRecordStatus::HasResponse => clauses.push(format!("{alias}.has_response = 1")),
        QueryRecordStatus::NoResponse => clauses.push(format!(
            "{alias}.error IS NULL AND {alias}.has_response = 0"
        )),
    }

    Ok((clauses, params))
}

fn join_clauses(clauses: &[String]) -> String {
    if clauses.is_empty() {
        "1 = 1".to_string()
    } else {
        clauses.join(" AND ")
    }
}

fn like_pattern(raw: &str) -> String {
    let mut pattern = String::with_capacity(raw.len() + 2);
    pattern.push('%');
    for ch in raw.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

fn encode_cursor(cursor: ListCursor) -> String {
    format!("{}:{}", cursor.created_at_ms, cursor.id)
}

impl PluginStatsKind {
    fn sql_value(self) -> &'static str {
        match self {
            Self::Matcher => "matcher",
            Self::Executor => "executor",
            Self::Builtin => "builtin",
            Self::All => "all",
        }
    }
}

fn as_i64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))
}

fn limit_to_i64(value: usize) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))
}

fn non_negative_u64(value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use rusqlite::{Connection, params};
    use serde_json::json;

    use super::super::model::{EdnsJson, EdnsOptionJson, QuestionJson, RecordJson};
    use super::*;

    #[test]
    fn test_read_record_row_matches_insert_and_select_column_order() {
        let mut conn = Connection::open_in_memory().unwrap();
        let tables = TableNames {
            records: "records".to_string(),
            traces: "traces".to_string(),
            trace_steps: "trace_steps".to_string(),
            question_sets: "question_sets".to_string(),
            question_items: "question_items".to_string(),
            meta: "meta".to_string(),
        };
        create_schema(&mut conn, &tables).unwrap();

        let expected = sample_record_row();
        let tx = conn.transaction().unwrap();
        let detail = insert_record(&tx, &tables, expected.clone(), Vec::new()).unwrap();
        tx.commit().unwrap();

        let question_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM question_items WHERE question_set_id = (SELECT question_set_id FROM records WHERE id=?1)",
                params![detail.record.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(question_count, 1);

        let row_columns = record_row_select_columns(None);
        let sql = format!(
            "SELECT
                {row_columns}
             FROM {}
             WHERE id = ?1",
            tables.records
        );
        let actual = conn
            .query_row(&sql, params![detail.record.id], StoredRecord::read)
            .unwrap();

        let actual = assemble_records(&conn, &tables, vec![actual])
            .unwrap()
            .pop()
            .unwrap();
        let expected = RecordRow {
            id: detail.record.id,
            ..expected
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_clear_history_clears_tail_before_partial_batch_checkpoint_failure() {
        let mut conn = Connection::open_in_memory().unwrap();
        let tables = TableNames {
            records: "records".to_string(),
            traces: "traces".to_string(),
            trace_steps: "trace_steps".to_string(),
            question_sets: "question_sets".to_string(),
            question_items: "question_items".to_string(),
            meta: "meta".to_string(),
        };
        create_schema(&mut conn, &tables).unwrap();

        let tx = conn.transaction().unwrap();
        let mut first_detail = None;
        for request_id in 0..=CLEANUP_BATCH_SIZE {
            let mut record = sample_record_row();
            record.request_id = request_id as u16;
            let detail = insert_record(&tx, &tables, record, Vec::new()).unwrap();
            if first_detail.is_none() {
                first_detail = Some(detail);
            }
        }
        tx.commit().unwrap();

        let tail = Arc::new(Mutex::new(VecDeque::from([first_detail.unwrap()])));
        let checkpoint_calls = Cell::new(0);
        let mut checkpoint = |_: &Connection| {
            let calls = checkpoint_calls.get() + 1;
            checkpoint_calls.set(calls);
            if calls == 2 {
                Err(DnsError::runtime("injected checkpoint failure"))
            } else {
                Ok(())
            }
        };

        let result = run_clear_history_with_checkpoint(
            &mut conn,
            Path::new("/nonexistent/query-recorder-review.sqlite"),
            &tables,
            &tail,
            &mut checkpoint,
        );

        assert!(result.is_err());
        assert!(tail.lock().unwrap().is_empty());
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 1);
    }

    pub(super) fn sample_record_row() -> RecordRow {
        let question = QuestionJson {
            name: "example.com.".to_string(),
            qtype: "A".to_string(),
            qclass: "IN".to_string(),
        };
        let answer = RecordJson {
            name: "example.com.".to_string(),
            class: "IN".to_string(),
            ttl: 300,
            rr_type: "A".to_string(),
            payload_kind: "A".to_string(),
            payload_text: "192.0.2.10".to_string(),
            payload: json!({ "ip": "192.0.2.10" }),
        };
        let edns = EdnsJson {
            udp_payload_size: 1232,
            ext_rcode: 0,
            version: 0,
            dnssec_ok: true,
            z: 0,
            options: vec![EdnsOptionJson {
                code: 8,
                name: "Subnet".to_string(),
                payload_kind: "Subnet".to_string(),
                payload: json!({
                    "addr": "192.0.2.0",
                    "source_prefix": 24,
                    "scope_prefix": 0,
                }),
            }],
        };

        RecordRow {
            id: 0,
            created_at_ms: 1_700_000_000_123,
            elapsed_ms: 37,
            request_id: 42,
            client_ip: "127.0.0.1".to_string(),
            questions_json: vec![question],
            req_rd: true,
            req_cd: false,
            req_ad: true,
            req_opcode: "Query".to_string(),
            req_edns_json: Some(edns.clone()),
            error: None,
            has_response: true,
            rcode: Some("NoError".to_string()),
            resp_aa: Some(false),
            resp_tc: Some(false),
            resp_ra: Some(true),
            resp_ad: Some(false),
            resp_cd: Some(false),
            answer_count: 1,
            authority_count: 0,
            additional_count: 0,
            answers_json: vec![answer],
            authorities_json: Vec::new(),
            additionals_json: Vec::new(),
            signature_json: Vec::new(),
            resp_edns_json: Some(edns),
        }
    }
}

#[cfg(test)]
#[path = "v2_tests.rs"]
mod v2_tests;
