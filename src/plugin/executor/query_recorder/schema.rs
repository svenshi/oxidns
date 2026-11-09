// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recorder-local schema revisions. No v1 data is inspected or modified.

use rusqlite::{Connection, OptionalExtension};

use super::model::TableNames;
use crate::infra::error::{DnsError, Result};

pub(super) fn create_schema(conn: &mut Connection, tables: &TableNames) -> Result<()> {
    let tx = conn.transaction()?;
    let names = [
        &tables.records,
        &tables.traces,
        &tables.trace_steps,
        &tables.question_sets,
        &tables.question_items,
        &tables.meta,
    ];
    let mut existing = 0;
    for name in names {
        existing += usize::from(
            tx.query_row(
                "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                [name],
                |_| Ok(()),
            )
            .optional()?
            .is_some(),
        );
    }
    if existing != 0 {
        if existing != names.len() {
            return Err(DnsError::plugin("query_recorder incomplete v2 schema"));
        }
        let revision: Option<String> = tx
            .query_row(
                &format!(
                    "SELECT value FROM {} WHERE key = 'schema_revision'",
                    tables.meta
                ),
                [],
                |row| row.get(0),
            )
            .optional()?;
        if revision.as_deref() != Some("1") {
            return Err(DnsError::plugin(format!(
                "query_recorder unsupported schema revision: {revision:?}"
            )));
        }
        ensure_minute_stats(&tx, tables)?;
        tx.commit()?;
        return Ok(());
    }
    tx.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {records} (
            id INTEGER PRIMARY KEY,
            created_at_ms INTEGER NOT NULL,
            elapsed_ms INTEGER NOT NULL,
            request_id INTEGER NOT NULL,
            client_ip TEXT NOT NULL,
            trace_id INTEGER NOT NULL REFERENCES {traces}(id),
            question_set_id INTEGER NOT NULL REFERENCES {question_sets}(id),
            req_rd INTEGER NOT NULL,
            req_cd INTEGER NOT NULL,
            req_ad INTEGER NOT NULL,
            req_opcode TEXT NOT NULL,
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
            payload_version INTEGER NOT NULL,
            payload_codec INTEGER NOT NULL,
            payload_raw_len INTEGER NOT NULL,
            payload BLOB NOT NULL
        );
        CREATE TABLE {traces} (id INTEGER PRIMARY KEY, fingerprint BLOB NOT NULL);
        CREATE INDEX {traces}_fingerprint_idx ON {traces}(fingerprint);
        CREATE TABLE {trace_steps} (
            trace_id INTEGER NOT NULL REFERENCES {traces}(id) ON DELETE CASCADE,
            event_index INTEGER NOT NULL,
            sequence_tag TEXT NOT NULL,
            node_index INTEGER,
            kind TEXT NOT NULL,
            tag TEXT,
            outcome TEXT NOT NULL,
            PRIMARY KEY(trace_id, event_index)
        ) WITHOUT ROWID;
        CREATE INDEX {trace_steps}_matcher_idx ON {trace_steps}(kind, tag, outcome, trace_id);
        CREATE TABLE {question_sets} (id INTEGER PRIMARY KEY, fingerprint BLOB NOT NULL, questions_json TEXT NOT NULL);
        CREATE INDEX {question_sets}_fingerprint_idx ON {question_sets}(fingerprint);
        CREATE TABLE {question_items} (
            question_set_id INTEGER NOT NULL REFERENCES {question_sets}(id) ON DELETE CASCADE,
            question_index INTEGER NOT NULL,
            name_lc TEXT NOT NULL,
            qtype TEXT NOT NULL,
            PRIMARY KEY(question_set_id, question_index)
        ) WITHOUT ROWID;
        CREATE INDEX {question_items}_qtype_idx ON {question_items}(qtype, question_set_id);
        CREATE INDEX {records}_created_at_idx ON {records}(created_at_ms DESC, id DESC);
        CREATE INDEX {records}_trace_idx ON {records}(trace_id);
        CREATE INDEX {records}_question_set_idx ON {records}(question_set_id);
        CREATE INDEX {records}_rcode_idx ON {records}(rcode COLLATE NOCASE);
        CREATE TABLE {meta} (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        INSERT INTO {meta} VALUES ('schema_revision', '1');",
        records = tables.records, traces = tables.traces, trace_steps = tables.trace_steps,
        question_sets = tables.question_sets, question_items = tables.question_items, meta = tables.meta,
    ))?;
    ensure_minute_stats(&tx, tables)?;
    tx.commit()?;
    Ok(())
}

fn ensure_minute_stats(tx: &rusqlite::Transaction<'_>, tables: &TableNames) -> Result<()> {
    tx.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {minute_stats} (
            minute_ms INTEGER PRIMARY KEY,
            total INTEGER NOT NULL,
            blocked INTEGER NOT NULL,
            elapsed_sum INTEGER NOT NULL,
            sample_count INTEGER NOT NULL
        );",
        minute_stats = tables.minute_stats,
    ))?;

    let count: i64 = tx
        .query_row(
            &format!("SELECT COUNT(*) FROM {}", tables.minute_stats),
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if count == 0 {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let since_ms = now_ms.saturating_sub(8 * 24 * 60 * 60 * 1000);
        let _ = tx.execute(
            &format!(
                "INSERT OR IGNORE INTO {minute_stats} (minute_ms, total, blocked, elapsed_sum, sample_count)
                 SELECT
                     (created_at_ms / 60000) * 60000 AS minute_ms,
                     COUNT(*) AS total,
                     SUM(CASE WHEN error IS NOT NULL OR UPPER(COALESCE(rcode, '')) IN ('NXDOMAIN', 'REFUSED', 'NON-EXISTENT DOMAIN') THEN 1 ELSE 0 END) AS blocked,
                     SUM(elapsed_ms) AS elapsed_sum,
                     COUNT(*) AS sample_count
                 FROM {records}
                 WHERE created_at_ms >= ?1
                 GROUP BY minute_ms",
                minute_stats = tables.minute_stats,
                records = tables.records,
            ),
            [since_ms],
        );
    }
    Ok(())
}
