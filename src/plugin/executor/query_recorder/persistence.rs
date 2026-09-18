// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Prepared writes, shared dictionaries, and SQL-to-API assembly for v2.

use std::collections::HashMap;

use rusqlite::{CachedStatement, Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

use super::model::{PendingRecord, QuestionJson, RecordDetail, RecordRow, StepJson, TableNames};
use super::payload::EncodedPayload;
use crate::infra::error::{DnsError, Result};

#[derive(Debug)]
pub(super) struct StoredRecord {
    pub(super) id: i64,
    pub(super) created_at_ms: i64,
    pub(super) elapsed_ms: u64,
    pub(super) request_id: u16,
    pub(super) client_ip: String,
    pub(super) req_rd: bool,
    pub(super) req_cd: bool,
    pub(super) req_ad: bool,
    pub(super) req_opcode: String,
    pub(super) error: Option<String>,
    pub(super) has_response: bool,
    pub(super) rcode: Option<String>,
    pub(super) resp_aa: Option<bool>,
    pub(super) resp_tc: Option<bool>,
    pub(super) resp_ra: Option<bool>,
    pub(super) resp_ad: Option<bool>,
    pub(super) resp_cd: Option<bool>,
    pub(super) answer_count: u32,
    pub(super) authority_count: u32,
    pub(super) additional_count: u32,
    pub(super) trace_id: i64,
    pub(super) question_set_id: i64,
    pub(super) payload: EncodedPayload,
}
pub(super) const RECORD_COLUMNS: &[&str] = &[
    "id",
    "created_at_ms",
    "elapsed_ms",
    "request_id",
    "client_ip",
    "req_rd",
    "req_cd",
    "req_ad",
    "req_opcode",
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
    "trace_id",
    "question_set_id",
    "payload_version",
    "payload_codec",
    "payload_raw_len",
    "payload",
];

impl StoredRecord {
    pub(super) fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            created_at_ms: row.get(1)?,
            elapsed_ms: unsigned(row.get::<_, i64>(2)?, 2)?,
            request_id: row.get(3)?,
            client_ip: row.get(4)?,
            req_rd: row.get(5)?,
            req_cd: row.get(6)?,
            req_ad: row.get(7)?,
            req_opcode: row.get(8)?,
            error: row.get(9)?,
            has_response: row.get(10)?,
            rcode: row.get(11)?,
            resp_aa: row.get(12)?,
            resp_tc: row.get(13)?,
            resp_ra: row.get(14)?,
            resp_ad: row.get(15)?,
            resp_cd: row.get(16)?,
            answer_count: row.get(17)?,
            authority_count: row.get(18)?,
            additional_count: row.get(19)?,
            trace_id: row.get(20)?,
            question_set_id: row.get(21)?,
            payload: EncodedPayload {
                version: row.get(22)?,
                codec: row.get(23)?,
                raw_len: row.get(24)?,
                data: row.get(25)?,
            },
        })
    }

    fn into_api(self, questions: Vec<QuestionJson>) -> Result<RecordRow> {
        let snapshot = self.payload.decode()?;
        Ok(RecordRow {
            id: self.id,
            created_at_ms: self.created_at_ms,
            elapsed_ms: self.elapsed_ms,
            request_id: self.request_id,
            client_ip: self.client_ip,
            req_rd: self.req_rd,
            req_cd: self.req_cd,
            req_ad: self.req_ad,
            req_opcode: self.req_opcode,
            error: self.error,
            has_response: self.has_response,
            rcode: self.rcode,
            resp_aa: self.resp_aa,
            resp_tc: self.resp_tc,
            resp_ra: self.resp_ra,
            resp_ad: self.resp_ad,
            resp_cd: self.resp_cd,
            answer_count: self.answer_count,
            authority_count: self.authority_count,
            additional_count: self.additional_count,
            questions_json: questions,
            req_edns_json: snapshot.req_edns_json,
            answers_json: snapshot.answers_json,
            authorities_json: snapshot.authorities_json,
            additionals_json: snapshot.additionals_json,
            signature_json: snapshot.signature_json,
            resp_edns_json: snapshot.resp_edns_json,
        })
    }
}

#[derive(Debug)]
pub(super) struct PreparedRecord {
    pub(super) detail: RecordDetail,
    pub(super) questions_json: String,
    pub(super) question_fingerprint: [u8; 32],
    pub(super) trace_fingerprint: [u8; 32],
    pub(super) payload: EncodedPayload,
}

impl PreparedRecord {
    pub(super) fn new(record: RecordRow, steps: Vec<StepJson>) -> Result<Self> {
        Self::with_compression(record, steps, true)
    }

    pub(super) fn with_compression(
        record: RecordRow,
        steps: Vec<StepJson>,
        compress: bool,
    ) -> Result<Self> {
        let questions_json = serde_json::to_string(&record.questions_json)?;
        let question_identity: Vec<_> = record
            .questions_json
            .iter()
            .map(|q| (q.name.as_str(), q.qtype.as_str(), q.qclass.as_str()))
            .collect();
        let question_fingerprint = Sha256::digest(serde_json::to_vec(&question_identity)?).into();
        // Explicit identity fields: future request-specific timings and values
        // must not silently become part of the shared path identity.
        let identity: Vec<_> = steps
            .iter()
            .map(|s| {
                (
                    s.event_index,
                    s.sequence_tag.as_str(),
                    s.node_index,
                    s.kind.as_str(),
                    s.tag.as_deref(),
                    s.outcome.as_str(),
                )
            })
            .collect();
        let trace_fingerprint = Sha256::digest(serde_json::to_vec(&identity)?).into();
        let payload = EncodedPayload::encode_with_compression(&record, compress)?;
        Ok(Self {
            detail: RecordDetail { record, steps },
            questions_json,
            question_fingerprint,
            trace_fingerprint,
            payload,
        })
    }
}

pub(super) fn prepare_pending(pending: &mut Vec<PendingRecord>) -> Result<Vec<PreparedRecord>> {
    pending
        .drain(..)
        .map(|p| {
            let (record, steps) = p.take_to_record();
            PreparedRecord::new(record, steps)
        })
        .collect()
}

#[derive(Default)]
struct BatchCache<'a> {
    traces: HashMap<[u8; 32], Vec<(&'a [StepJson], i64)>>,
    questions: HashMap<[u8; 32], Vec<(&'a str, i64)>>,
}

struct Inserts<'a> {
    record: CachedStatement<'a>,
    trace: CachedStatement<'a>,
    step: CachedStatement<'a>,
    question_set: CachedStatement<'a>,
    question_item: CachedStatement<'a>,
    find_trace: CachedStatement<'a>,
    find_questions: CachedStatement<'a>,
}

impl<'a> Inserts<'a> {
    fn new(conn: &'a Connection, t: &TableNames) -> Result<Self> {
        Ok(Self {
            record: conn.prepare_cached(&format!("INSERT INTO {} (created_at_ms, elapsed_ms, request_id, client_ip, req_rd, req_cd, req_ad, req_opcode, error, has_response, rcode, resp_aa, resp_tc, resp_ra, resp_ad, resp_cd, answer_count, authority_count, additional_count, trace_id, question_set_id, payload_version, payload_codec, payload_raw_len, payload) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)", t.records))?,
            trace: conn.prepare_cached(&format!("INSERT INTO {} (fingerprint) VALUES (?1)", t.traces))?,
            step: conn.prepare_cached(&format!("INSERT INTO {} (trace_id,event_index,sequence_tag,node_index,kind,tag,outcome) VALUES (?,?,?,?,?,?,?)", t.trace_steps))?,
            question_set: conn.prepare_cached(&format!("INSERT INTO {} (fingerprint,questions_json) VALUES (?,?)", t.question_sets))?,
            question_item: conn.prepare_cached(&format!("INSERT INTO {} (question_set_id,question_index,name_lc,qtype) VALUES (?,?,?,?)", t.question_items))?,
            find_trace: conn.prepare_cached(&format!("SELECT id FROM {} WHERE fingerprint=?1", t.traces))?,
            find_questions: conn.prepare_cached(&format!("SELECT id,questions_json FROM {} WHERE fingerprint=?1", t.question_sets))?,
        })
    }
}

pub(super) fn insert_batch(
    tx: &Transaction<'_>,
    tables: &TableNames,
    prepared: &[PreparedRecord],
) -> Result<Vec<i64>> {
    let mut sql = Inserts::new(tx, tables)?;
    let mut cache = BatchCache::default();
    let mut ids = Vec::with_capacity(prepared.len());
    for p in prepared {
        let trace_id = match cache.traces.get(&p.trace_fingerprint).and_then(|items| {
            items
                .iter()
                .find(|(steps, _)| *steps == p.detail.steps.as_slice())
        }) {
            Some((_, id)) => *id,
            None => {
                let mut candidates = sql.find_trace.query([p.trace_fingerprint.as_slice()])?;
                let mut found = None;
                while let Some(row) = candidates.next()? {
                    let id = row.get(0)?;
                    if load_steps(tx, tables, id)? == p.detail.steps {
                        found = Some(id);
                        break;
                    }
                }
                drop(candidates);
                let id = if let Some(id) = found {
                    id
                } else {
                    sql.trace.execute([p.trace_fingerprint.as_slice()])?;
                    let id = tx.last_insert_rowid();
                    for s in &p.detail.steps {
                        sql.step.execute(params![
                            id,
                            s.event_index as i64,
                            s.sequence_tag,
                            s.node_index.map(|v| v as i64),
                            s.kind,
                            s.tag,
                            s.outcome
                        ])?;
                    }
                    id
                };
                cache
                    .traces
                    .entry(p.trace_fingerprint)
                    .or_default()
                    .push((&p.detail.steps, id));
                id
            }
        };
        let question_set_id = match cache
            .questions
            .get(&p.question_fingerprint)
            .and_then(|items| items.iter().find(|(json, _)| *json == p.questions_json))
        {
            Some((_, id)) => *id,
            None => {
                let mut candidates = sql
                    .find_questions
                    .query([p.question_fingerprint.as_slice()])?;
                let mut found = None;
                while let Some(row) = candidates.next()? {
                    if row.get::<_, String>(1)? == p.questions_json {
                        found = Some(row.get(0)?);
                        break;
                    }
                }
                drop(candidates);
                let id = if let Some(id) = found {
                    id
                } else {
                    sql.question_set
                        .execute(params![p.question_fingerprint.as_slice(), p.questions_json])?;
                    let id = tx.last_insert_rowid();
                    for (index, q) in p.detail.record.questions_json.iter().enumerate() {
                        sql.question_item.execute(params![
                            id,
                            index as i64,
                            q.name.to_ascii_lowercase(),
                            q.qtype.to_ascii_uppercase()
                        ])?;
                    }
                    id
                };
                cache
                    .questions
                    .entry(p.question_fingerprint)
                    .or_default()
                    .push((&p.questions_json, id));
                id
            }
        };
        let r = &p.detail.record;
        sql.record.execute(params![
            r.created_at_ms,
            i64::try_from(r.elapsed_ms)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, i64::MAX))?,
            r.request_id,
            r.client_ip,
            r.req_rd,
            r.req_cd,
            r.req_ad,
            r.req_opcode,
            r.error,
            r.has_response,
            r.rcode,
            r.resp_aa,
            r.resp_tc,
            r.resp_ra,
            r.resp_ad,
            r.resp_cd,
            r.answer_count,
            r.authority_count,
            r.additional_count,
            trace_id,
            question_set_id,
            p.payload.version,
            p.payload.codec,
            p.payload.raw_len,
            p.payload.data,
        ])?;
        ids.push(tx.last_insert_rowid());
    }
    Ok(ids)
}

pub(super) fn load_steps(
    conn: &Connection,
    tables: &TableNames,
    trace_id: i64,
) -> Result<Vec<StepJson>> {
    let exists = conn
        .query_row(
            &format!("SELECT 1 FROM {} WHERE id=?1", tables.traces),
            [trace_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(DnsError::plugin("query_recorder missing shared trace"));
    }
    let mut stmt = conn.prepare_cached(&format!("SELECT event_index,sequence_tag,node_index,kind,tag,outcome FROM {} WHERE trace_id=?1 ORDER BY event_index",tables.trace_steps))?;
    let rows = stmt.query_map([trace_id], |r| {
        Ok(StepJson {
            event_index: unsigned(r.get::<_, i64>(0)?, 0)?,
            sequence_tag: r.get(1)?,
            node_index: r
                .get::<_, Option<i64>>(2)?
                .map(|v| unsigned(v, 2))
                .transpose()?,
            kind: r.get(3)?,
            tag: r.get(4)?,
            outcome: r.get(5)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub(super) fn assemble_records(
    conn: &Connection,
    tables: &TableNames,
    stored: Vec<StoredRecord>,
) -> Result<Vec<RecordRow>> {
    if stored.is_empty() {
        return Ok(Vec::new());
    }
    let mut question_ids: Vec<_> = stored.iter().map(|r| r.question_set_id).collect();
    question_ids.sort_unstable();
    question_ids.dedup();
    let mut trace_ids: Vec<_> = stored.iter().map(|r| r.trace_id).collect();
    trace_ids.sort_unstable();
    trace_ids.dedup();
    let mut questions = HashMap::new();
    // Chunk even when an internal caller bypasses the API's 500-row limit.
    for chunk in question_ids.chunks(500) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let mut stmt = conn.prepare(&format!(
            "SELECT id,questions_json FROM {} WHERE id IN ({placeholders})",
            tables.question_sets
        ))?;
        let mut rows = stmt.query(rusqlite::params_from_iter(chunk))?;
        while let Some(row) = rows.next()? {
            questions.insert(
                row.get::<_, i64>(0)?,
                serde_json::from_str::<Vec<QuestionJson>>(&row.get::<_, String>(1)?)?,
            );
        }
    }
    for chunk in trace_ids.chunks(500) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let count: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM {} WHERE id IN ({placeholders})",
                tables.traces
            ),
            rusqlite::params_from_iter(chunk),
            |r| r.get(0),
        )?;
        if count != chunk.len() as i64 {
            return Err(DnsError::plugin("query_recorder missing shared trace"));
        }
    }
    stored
        .into_iter()
        .map(|r| {
            let qs = questions
                .get(&r.question_set_id)
                .ok_or_else(|| DnsError::plugin("query_recorder missing shared questions"))?
                .clone();
            r.into_api(qs)
        })
        .collect()
}

fn unsigned<T: TryFrom<i64>>(value: i64, column: usize) -> rusqlite::Result<T> {
    T::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(column, value))
}
