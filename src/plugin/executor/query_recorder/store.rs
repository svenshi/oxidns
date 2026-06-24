// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::broadcast;

use super::backend::{ClearHistoryResult, RecorderBackend, WriterCommand, WriterThreadContext};
use super::model::{
    DistributionQuery, DistributionResponse, DistributionRow, LatencyHistogramBucket, LatencyQuery,
    LatencySlowRow, LatencySummary, ListCursor, ListQuery, PendingRecord, PluginStatsKind,
    PluginStatsRow, PluginsStatsQuery, QueryRecordFilter, QueryRecordStatus, RecordDetail,
    RecordRow, StepJson, TableNames, TimeseriesPoint, TimeseriesQuery, TimeseriesResponse,
    TopBucketRow, TopBucketsResponse, TopQuery,
};
use crate::infra::error::{DnsError, Result};

const SCHEMA_VERSION: &str = "v1";
const QUESTIONS_BACKFILL_MARKER: &str = "questions_backfilled";
const CLEANUP_BATCH_SIZE: usize = 1_000;
const PLUGIN_STATS_SAMPLE_LIMIT: usize = 10_000;
const RECORD_ROW_COLUMNS: [&str; 27] = [
    "id",
    "created_at_ms",
    "elapsed_ms",
    "request_id",
    "client_ip",
    "questions_json",
    "req_rd",
    "req_cd",
    "req_ad",
    "req_opcode",
    "req_edns_json",
    "error",
    "has_response",
    "rcode",
    "resp_aa",
    "resp_tc",
    "resp_ra",
    "resp_ad",
    "resp_cd",
    "answer_count",
    "authority_count",
    "additional_count",
    "answers_json",
    "authorities_json",
    "additionals_json",
    "signature_json",
    "resp_edns_json",
];

pub(super) fn open_writer_database(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    // Tuned for the dedicated writer thread. Keep WAL and incremental vacuum
    // behavior, but avoid giving the single writer the same large read cache
    // and mmap footprint that used to be applied to every reader.
    // - auto_vacuum must be selected before WAL or schema creation for a fresh
    //   database; otherwise SQLite keeps the default NONE mode until a manual
    //   VACUUM rewrites the file.
    // - WAL + synchronous=NORMAL keeps the writer fast and readers non-blocking.
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
        steps: format!("{prefix}_steps"),
        questions: format!("{prefix}_questions"),
        meta: format!("{prefix}_meta"),
    }
}

fn record_row_select_columns(alias: Option<&str>) -> String {
    RECORD_ROW_COLUMNS
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

pub(crate) fn create_schema(conn: &mut Connection, tables: &TableNames) -> rusqlite::Result<()> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {records} (
            id INTEGER PRIMARY KEY,
            created_at_ms INTEGER NOT NULL,
            elapsed_ms INTEGER NOT NULL,
            request_id INTEGER NOT NULL,
            client_ip TEXT NOT NULL,
            questions_json TEXT NOT NULL,
            req_rd INTEGER NOT NULL,
            req_cd INTEGER NOT NULL,
            req_ad INTEGER NOT NULL,
            req_opcode TEXT NOT NULL,
            req_edns_json TEXT NULL,
            error TEXT NULL,
            has_response INTEGER NOT NULL,
            rcode TEXT NULL,
            resp_aa INTEGER NULL,
            resp_tc INTEGER NULL,
            resp_ra INTEGER NULL,
            resp_ad INTEGER NULL,
            resp_cd INTEGER NULL,
            answer_count INTEGER NOT NULL,
            authority_count INTEGER NOT NULL,
            additional_count INTEGER NOT NULL,
            answers_json TEXT NOT NULL,
            authorities_json TEXT NOT NULL,
            additionals_json TEXT NOT NULL,
            signature_json TEXT NOT NULL,
            resp_edns_json TEXT NULL
        );
        CREATE TABLE IF NOT EXISTS {steps} (
            record_id INTEGER NOT NULL,
            event_index INTEGER NOT NULL,
            sequence_tag TEXT NOT NULL,
            node_index INTEGER NULL,
            kind TEXT NOT NULL,
            tag TEXT NULL,
            outcome TEXT NOT NULL,
            PRIMARY KEY (record_id, event_index),
            FOREIGN KEY(record_id) REFERENCES {records}(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS {questions} (
            record_id INTEGER NOT NULL,
            question_index INTEGER NOT NULL,
            name_lc TEXT NOT NULL,
            qtype TEXT NOT NULL,
            qclass TEXT NOT NULL,
            PRIMARY KEY (record_id, question_index),
            FOREIGN KEY(record_id) REFERENCES {records}(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS {meta} (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS {records}_created_at_idx ON {records}(created_at_ms DESC);
        CREATE INDEX IF NOT EXISTS {records}_request_id_idx ON {records}(request_id);
        CREATE INDEX IF NOT EXISTS {records}_client_ip_idx ON {records}(client_ip);
        CREATE INDEX IF NOT EXISTS {records}_rcode_idx ON {records}(rcode);
        CREATE INDEX IF NOT EXISTS {questions}_record_id_idx ON {questions}(record_id);
        CREATE INDEX IF NOT EXISTS {questions}_name_idx ON {questions}(name_lc, record_id);
        CREATE INDEX IF NOT EXISTS {questions}_qtype_idx ON {questions}(qtype, record_id);
        CREATE INDEX IF NOT EXISTS {steps}_kind_tag_outcome_idx ON {steps}(kind, tag, outcome);
        CREATE INDEX IF NOT EXISTS {steps}_record_id_idx ON {steps}(record_id);
        -- Covering index for the matcher_tag EXISTS subquery used by /records
        -- and /stats endpoints. Without record_id in the index tail SQLite
        -- has to do a second lookup per candidate, which makes rapid
        -- matcher-click filtering pile up in the blocking pool.
        CREATE INDEX IF NOT EXISTS {steps}_matcher_lookup_idx
            ON {steps}(kind, tag, outcome, record_id);
        -- Speeds up `/stats/plugins` style JOINs which find `s.record_id = r.id
        -- AND s.kind = ?`. With only the single-column record_id index the
        -- planner reads every step for that record and filters by kind in
        -- memory; the (record_id, kind) prefix turns that into a covered
        -- range lookup.
        CREATE INDEX IF NOT EXISTS {steps}_record_kind_idx
            ON {steps}(record_id, kind);",
        records = tables.records,
        questions = tables.questions,
        meta = tables.meta,
        steps = tables.steps,
    ))?;
    backfill_questions_once(conn, tables)
}

fn backfill_questions_once(conn: &Connection, tables: &TableNames) -> rusqlite::Result<()> {
    if questions_backfill_done(conn, tables)? {
        return Ok(());
    }

    backfill_questions(conn, tables)?;
    mark_questions_backfilled(conn, tables)
}

fn questions_backfill_done(conn: &Connection, tables: &TableNames) -> rusqlite::Result<bool> {
    conn.query_row(
        &format!(
            "SELECT 1 FROM {} WHERE key = ?1 AND value = ?2",
            tables.meta
        ),
        params![QUESTIONS_BACKFILL_MARKER, "true"],
        |_| Ok(()),
    )
    .optional()
    .map(|row| row.is_some())
}

fn mark_questions_backfilled(conn: &Connection, tables: &TableNames) -> rusqlite::Result<()> {
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO {} (key, value) VALUES (?1, ?2)",
            tables.meta
        ),
        params![QUESTIONS_BACKFILL_MARKER, "true"],
    )?;
    Ok(())
}

fn backfill_questions(conn: &Connection, tables: &TableNames) -> rusqlite::Result<()> {
    conn.execute(
        &format!(
            "WITH missing_records AS (
                SELECT r.id, r.questions_json
                FROM {records} r
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM {questions} existing
                    WHERE existing.record_id = r.id
                )
             )
             INSERT OR IGNORE INTO {questions} (
                record_id,
                question_index,
                name_lc,
                qtype,
                qclass
             )
             SELECT
                missing_records.id,
                CAST(q.key AS INTEGER),
                LOWER(json_extract(q.value, '$.name')),
                UPPER(json_extract(q.value, '$.qtype')),
                json_extract(q.value, '$.qclass')
             FROM missing_records
             JOIN json_each(missing_records.questions_json) AS q
             WHERE json_extract(q.value, '$.name') IS NOT NULL
               AND json_extract(q.value, '$.qtype') IS NOT NULL
               AND json_extract(q.value, '$.qclass') IS NOT NULL",
            records = tables.records,
            questions = tables.questions,
        ),
        [],
    )?;
    Ok(())
}

pub(super) fn run_writer_thread(
    context: WriterThreadContext,
    rx: Receiver<WriterCommand>,
    mut conn: Connection,
) -> Result<()> {
    let WriterThreadContext {
        tables,
        stop_requested,
        tail,
        memory_tail,
        broadcaster,
        batch_size,
        flush_interval,
    } = context;

    let mut pending = Vec::with_capacity(batch_size);
    loop {
        match rx.recv_timeout(flush_interval) {
            Ok(WriterCommand::Insert(record)) => {
                pending.push(*record);
                if pending.len() >= batch_size {
                    flush_pending(
                        &mut conn,
                        &tables,
                        &mut pending,
                        &tail,
                        memory_tail,
                        &broadcaster,
                    )?;
                }
            }
            Ok(WriterCommand::Cleanup { cutoff_ms }) => {
                flush_pending(
                    &mut conn,
                    &tables,
                    &mut pending,
                    &tail,
                    memory_tail,
                    &broadcaster,
                )?;
                run_cleanup(&mut conn, &tables, cutoff_ms)?;
            }
            Ok(WriterCommand::ClearHistory { reply_tx }) => {
                let result = flush_pending(
                    &mut conn,
                    &tables,
                    &mut pending,
                    &tail,
                    memory_tail,
                    &broadcaster,
                )
                .and_then(|_| run_clear_history(&mut conn, &tables, &tail))
                .map_err(|err| err.to_string());
                let _ = reply_tx.send(result);
            }
            #[cfg(test)]
            Ok(WriterCommand::Flush { reply_tx }) => {
                let result = flush_pending(
                    &mut conn,
                    &tables,
                    &mut pending,
                    &tail,
                    memory_tail,
                    &broadcaster,
                )
                .map_err(|err| err.to_string());
                let _ = reply_tx.send(result);
            }
            Err(RecvTimeoutError::Timeout) => {
                flush_pending(
                    &mut conn,
                    &tables,
                    &mut pending,
                    &tail,
                    memory_tail,
                    &broadcaster,
                )?;
                if stop_requested.load(Ordering::Relaxed) {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                flush_pending(
                    &mut conn,
                    &tables,
                    &mut pending,
                    &tail,
                    memory_tail,
                    &broadcaster,
                )?;
                break;
            }
        }
    }

    Ok(())
}

fn flush_pending(
    conn: &mut Connection,
    tables: &TableNames,
    pending: &mut Vec<PendingRecord>,
    tail: &Arc<Mutex<VecDeque<RecordDetail>>>,
    memory_tail: usize,
    broadcaster: &broadcast::Sender<RecordDetail>,
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }

    let tx = conn.transaction()?;
    let mut committed = Vec::with_capacity(pending.len());
    for pending_record in pending.drain(..) {
        let (record, steps) = pending_record.take_to_record();
        let detail = insert_record(&tx, tables, record, steps)?;
        committed.push(detail);
    }
    tx.commit()?;

    let mut tail_guard = tail
        .lock()
        .map_err(|_| "query_recorder tail buffer lock poisoned".to_string())?;
    for detail in committed {
        if tail_guard.len() >= memory_tail {
            tail_guard.pop_front();
        }
        tail_guard.push_back(detail.clone());
        let _ = broadcaster.send(detail);
    }
    Ok(())
}

fn insert_record(
    tx: &rusqlite::Transaction<'_>,
    tables: &TableNames,
    record: RecordRow,
    steps: Vec<StepJson>,
) -> Result<RecordDetail> {
    let questions_json = serde_json::to_string(&record.questions_json)?;
    let req_edns_json = serialize_optional_json(&record.req_edns_json)?;
    let answers_json = serde_json::to_string(&record.answers_json)?;
    let authorities_json = serde_json::to_string(&record.authorities_json)?;
    let additionals_json = serde_json::to_string(&record.additionals_json)?;
    let signature_json = serde_json::to_string(&record.signature_json)?;
    let resp_edns_json = serialize_optional_json(&record.resp_edns_json)?;

    tx.execute(
        &format!(
            "INSERT INTO {} (
                created_at_ms,
                elapsed_ms,
                request_id,
                client_ip,
                questions_json,
                req_rd,
                req_cd,
                req_ad,
                req_opcode,
                req_edns_json,
                error,
                has_response,
                rcode,
                resp_aa,
                resp_tc,
                resp_ra,
                resp_ad,
                resp_cd,
                answer_count,
                authority_count,
                additional_count,
                answers_json,
                authorities_json,
                additionals_json,
                signature_json,
                resp_edns_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
            tables.records
        ),
        params![
            record.created_at_ms,
            as_i64(record.elapsed_ms)?,
            i64::from(record.request_id),
            record.client_ip,
            questions_json,
            bool_to_i64(record.req_rd),
            bool_to_i64(record.req_cd),
            bool_to_i64(record.req_ad),
            record.req_opcode,
            req_edns_json,
            record.error,
            bool_to_i64(record.has_response),
            record.rcode,
            record.resp_aa.map(bool_to_i64),
            record.resp_tc.map(bool_to_i64),
            record.resp_ra.map(bool_to_i64),
            record.resp_ad.map(bool_to_i64),
            record.resp_cd.map(bool_to_i64),
            i64::from(record.answer_count),
            i64::from(record.authority_count),
            i64::from(record.additional_count),
            answers_json,
            authorities_json,
            additionals_json,
            signature_json,
            resp_edns_json,
        ],
    )?;
    let record_id = tx.last_insert_rowid();

    for (question_index, question) in record.questions_json.iter().enumerate() {
        tx.execute(
            &format!(
                "INSERT OR IGNORE INTO {} (
                    record_id,
                    question_index,
                    name_lc,
                    qtype,
                    qclass
                ) VALUES (?1, ?2, ?3, ?4, ?5)",
                tables.questions
            ),
            params![
                record_id,
                question_index as i64,
                question.name.to_ascii_lowercase(),
                question.qtype.to_ascii_uppercase(),
                question.qclass.as_str(),
            ],
        )?;
    }

    for step in &steps {
        tx.execute(
            &format!(
                "INSERT INTO {} (
                    record_id,
                    event_index,
                    sequence_tag,
                    node_index,
                    kind,
                    tag,
                    outcome
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                tables.steps
            ),
            params![
                record_id,
                step.event_index as i64,
                step.sequence_tag,
                step.node_index.map(|value| value as i64),
                step.kind,
                step.tag,
                step.outcome,
            ],
        )?;
    }

    Ok(RecordDetail {
        record: RecordRow {
            id: record_id,
            ..record
        },
        steps,
    })
}

fn serialize_optional_json<T>(value: &Option<T>) -> rusqlite::Result<Option<String>>
where
    T: Serialize,
{
    value
        .as_ref()
        .map(|value| {
            serde_json::to_string(value)
                .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
        })
        .transpose()
}

fn run_cleanup(conn: &mut Connection, tables: &TableNames, cutoff_ms: i64) -> rusqlite::Result<()> {
    loop {
        let deleted = conn.execute(
            &format!(
                "DELETE FROM {records}
                 WHERE id IN (
                    SELECT id FROM {records}
                    WHERE created_at_ms < ?1
                    ORDER BY created_at_ms ASC, id ASC
                    LIMIT ?2
                 )",
                records = tables.records
            ),
            params![cutoff_ms, CLEANUP_BATCH_SIZE as i64],
        )?;
        if deleted == 0 {
            break;
        }
    }
    conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE); PRAGMA incremental_vacuum;")
}

fn run_clear_history(
    conn: &mut Connection,
    tables: &TableNames,
    tail: &Arc<Mutex<VecDeque<RecordDetail>>>,
) -> Result<ClearHistoryResult> {
    let tx = conn.transaction()?;
    let cleared_records = tx.execute(&format!("DELETE FROM {}", tables.records), [])?;
    tx.commit()?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA incremental_vacuum;")?;

    let mut tail_guard = tail
        .lock()
        .map_err(|_| "query_recorder tail buffer lock poisoned".to_string())?;
    tail_guard.clear();

    Ok(ClearHistoryResult { cleared_records })
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
        records.push(read_record_row(row)?);
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
    Ok((records, next_cursor))
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
        .query_row(params![record_id], read_record_row)
        .optional()?;

    let Some(record) = record else {
        return Ok(None);
    };

    let steps = load_steps(&conn, &backend.tables, record_id)?;
    Ok(Some(RecordDetail { record, steps }))
}

fn load_steps(
    conn: &Connection,
    tables: &TableNames,
    record_id: i64,
) -> std::result::Result<Vec<StepJson>, DnsError> {
    let sql = format!(
        "SELECT event_index, sequence_tag, node_index, kind, tag, outcome
         FROM {steps}
         WHERE record_id = ?1
         ORDER BY event_index ASC",
        steps = tables.steps
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![record_id])?;

    let mut steps = Vec::new();
    while let Some(row) = rows.next()? {
        steps.push(StepJson {
            event_index: row.get::<_, i64>(0).and_then(non_negative_usize)?,
            sequence_tag: row.get(1)?,
            node_index: row
                .get::<_, Option<i64>>(2)?
                .map(|value| {
                    usize::try_from(value).map_err(|_| DnsError::plugin("negative step node_index"))
                })
                .transpose()?,
            kind: row.get(3)?,
            tag: row.get(4)?,
            outcome: row.get(5)?,
        });
    }
    Ok(steps)
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
    // (kind, tag, outcome, record_id) covering index with a leading
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
    // Restructured from a cross-join (sample_count × sample_records × steps)
    // to an IN-subquery so SQLite aggregates steps directly without producing
    // a 10k-row intermediate for every API call.
    let sql = format!(
        "WITH sample_records AS (
            SELECT r.id
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
                     AND s.outcome IN ('matched', 'not_matched') THEN 1
                    ELSE 0
                END) AS checked,
                SUM(CASE
                    WHEN s.kind = 'matcher' AND s.outcome = 'matched' THEN 1
                    ELSE 0
                END) AS matched,
                SUM(CASE
                    WHEN s.kind = 'executor' AND s.outcome = 'entered' THEN 1
                    WHEN s.kind = 'builtin' THEN 1
                    ELSE 0
                END) AS executed,
                COUNT(DISTINCT s.record_id) AS query_hits
            FROM {steps} s
            WHERE s.record_id IN (SELECT id FROM sample_records)
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
        steps = backend.tables.steps,
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
            SELECT r.id
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
         LEFT JOIN {questions} q ON q.record_id = sample_records.id
         GROUP BY totals.sample_size, qname
         ORDER BY count DESC, qname ASC
         LIMIT ?",
        records = backend.tables.records,
        questions = backend.tables.questions,
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
            SELECT r.id
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
         LEFT JOIN {questions} q ON q.record_id = sample_records.id
         GROUP BY totals.sample_size, qtype
         ORDER BY count DESC, qtype ASC",
        records = backend.tables.records,
        questions = backend.tables.questions,
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
            SELECT r.id, r.elapsed_ms
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
         JOIN {questions} q ON q.record_id = sample_records.id
         GROUP BY qname
         HAVING qname IS NOT NULL
         ORDER BY avg_ms DESC, count DESC
         LIMIT ?",
        records = backend.tables.records,
        questions = backend.tables.questions,
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
        // IMPORTANT: keep this as an UNCORRELATED `IN (SELECT ...)` subquery.
        //
        // An EXISTS form that references `{alias}.id` becomes a correlated
        // subquery, forcing SQLite to re-run the lookup for every candidate
        // record. With LIMIT 100 plus a low-selectivity matcher (say, hits
        // 0.5% of records), the planner can end up scanning hundreds of
        // thousands of rows even with the covering steps index — making the
        // /records endpoint feel unresponsive when a matcher row is clicked.
        //
        // This IN form is uncorrelated: SQLite materializes the matched
        // record_id set once (a tight index range scan over the
        // `(kind, tag, outcome, record_id)` index), then the outer plan
        // walks records desc by created_at and does O(1) membership tests.
        clauses.push(format!(
            "{alias}.id IN (
                SELECT s.record_id
                FROM {steps} s
                WHERE s.kind = 'matcher'
                  AND s.outcome = 'matched'
                  AND s.tag = ?
            )",
            steps = tables.steps,
        ));
        params.push(Value::Text(matcher_tag.to_string()));
    }
    if let Some(qname) = filter.qname.as_deref() {
        clauses.push(format!(
            "{alias}.id IN (
                SELECT q.record_id
                FROM {questions} q
                WHERE q.name_lc LIKE ? ESCAPE '\\'
            )",
            questions = tables.questions,
        ));
        params.push(Value::Text(like_pattern(&qname.to_ascii_lowercase())));
    }
    if let Some(qtype) = filter.qtype.as_deref() {
        clauses.push(format!(
            "{alias}.id IN (
                SELECT q.record_id
                FROM {questions} q
                WHERE q.qtype = ?
            )",
            questions = tables.questions,
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
        clauses.push(format!(
            "{alias}.rcode IS NOT NULL AND UPPER({alias}.rcode) = UPPER(?)"
        ));
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

fn read_record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordRow> {
    Ok(RecordRow {
        id: row.get(0)?,
        created_at_ms: row.get::<_, i64>(1)?,
        elapsed_ms: row.get::<_, i64>(2).and_then(non_negative_u64)?,
        request_id: row.get::<_, i64>(3).and_then(non_negative_u16)?,
        client_ip: row.get(4)?,
        questions_json: parse_json_column(row.get(5)?)?,
        req_rd: read_bool(row, 6)?,
        req_cd: read_bool(row, 7)?,
        req_ad: read_bool(row, 8)?,
        req_opcode: row.get(9)?,
        req_edns_json: parse_optional_json_column(row.get(10)?)?,
        error: row.get(11)?,
        has_response: read_bool(row, 12)?,
        rcode: row.get(13)?,
        resp_aa: read_optional_bool(row, 14)?,
        resp_tc: read_optional_bool(row, 15)?,
        resp_ra: read_optional_bool(row, 16)?,
        resp_ad: read_optional_bool(row, 17)?,
        resp_cd: read_optional_bool(row, 18)?,
        answer_count: row.get::<_, i64>(19).and_then(non_negative_u32)?,
        authority_count: row.get::<_, i64>(20).and_then(non_negative_u32)?,
        additional_count: row.get::<_, i64>(21).and_then(non_negative_u32)?,
        answers_json: parse_json_column(row.get(22)?)?,
        authorities_json: parse_json_column(row.get(23)?)?,
        additionals_json: parse_json_column(row.get(24)?)?,
        signature_json: parse_json_column(row.get(25)?)?,
        resp_edns_json: parse_optional_json_column(row.get(26)?)?,
    })
}

fn parse_json_column<T>(raw: String) -> rusqlite::Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_str(raw.as_str()).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn parse_optional_json_column<T>(raw: Option<String>) -> rusqlite::Result<Option<T>>
where
    T: DeserializeOwned,
{
    raw.map(parse_json_column).transpose()
}

fn read_bool(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<bool> {
    Ok(row.get::<_, i64>(index)? != 0)
}

fn read_optional_bool(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<bool>> {
    Ok(row.get::<_, Option<i64>>(index)?.map(|value| value != 0))
}

fn as_i64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))
}

fn limit_to_i64(value: usize) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, i64::MAX))
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn non_negative_u64(value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}

fn non_negative_u32(value: i64) -> rusqlite::Result<u32> {
    u32::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}

fn non_negative_u16(value: i64) -> rusqlite::Result<u16> {
    u16::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}

fn non_negative_usize(value: i64) -> rusqlite::Result<usize> {
    usize::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, params};
    use serde_json::json;

    use super::super::model::{EdnsJson, EdnsOptionJson, QuestionJson, RecordJson};
    use super::*;

    #[test]
    fn test_read_record_row_matches_insert_and_select_column_order() {
        let mut conn = Connection::open_in_memory().unwrap();
        let tables = TableNames {
            records: "records".to_string(),
            steps: "steps".to_string(),
            questions: "questions".to_string(),
            meta: "meta".to_string(),
        };
        create_schema(&mut conn, &tables).unwrap();

        let expected = sample_record_row();
        let tx = conn.transaction().unwrap();
        let detail = insert_record(&tx, &tables, expected.clone(), Vec::new()).unwrap();
        tx.commit().unwrap();

        let question_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM questions WHERE record_id = ?1",
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
            .query_row(&sql, params![detail.record.id], read_record_row)
            .unwrap();

        let expected = RecordRow {
            id: detail.record.id,
            ..expected
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_create_schema_backfills_missing_question_index_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        let tables = TableNames {
            records: "records".to_string(),
            steps: "steps".to_string(),
            questions: "questions".to_string(),
            meta: "meta".to_string(),
        };
        create_schema(&mut conn, &tables).unwrap();

        let tx = conn.transaction().unwrap();
        let detail = insert_record(&tx, &tables, sample_record_row(), Vec::new()).unwrap();
        tx.commit().unwrap();
        conn.execute("DELETE FROM questions", []).unwrap();
        conn.execute(
            "DELETE FROM meta WHERE key = ?1",
            params![QUESTIONS_BACKFILL_MARKER],
        )
        .unwrap();

        create_schema(&mut conn, &tables).unwrap();

        let question: (String, String) = conn
            .query_row(
                "SELECT name_lc, qtype FROM questions WHERE record_id = ?1",
                params![detail.record.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(question, ("example.com.".to_string(), "A".to_string()));
    }

    #[test]
    fn test_create_schema_skips_question_backfill_after_marker() {
        let mut conn = Connection::open_in_memory().unwrap();
        let tables = TableNames {
            records: "records".to_string(),
            steps: "steps".to_string(),
            questions: "questions".to_string(),
            meta: "meta".to_string(),
        };
        create_schema(&mut conn, &tables).unwrap();

        let tx = conn.transaction().unwrap();
        insert_record(&tx, &tables, sample_record_row(), Vec::new()).unwrap();
        tx.commit().unwrap();
        conn.execute("DELETE FROM questions", []).unwrap();

        create_schema(&mut conn, &tables).unwrap();

        let question_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM questions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(question_count, 0);
    }

    fn sample_record_row() -> RecordRow {
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
