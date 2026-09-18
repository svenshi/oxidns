// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use super::tests::sample_record_row;
use super::*;

fn step(index: usize, tag: &str) -> StepJson {
    StepJson {
        event_index: index,
        sequence_tag: "main".into(),
        node_index: Some(index),
        kind: "matcher".into(),
        tag: Some(tag.into()),
        outcome: "matched".into(),
    }
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn write(conn: &mut Connection, t: &TableNames, rows: &[PreparedRecord]) -> Vec<i64> {
    let tx = conn.transaction().unwrap();
    let ids = insert_batch(&tx, t, rows).unwrap();
    tx.commit().unwrap();
    ids
}

#[test]
fn shared_objects_survive_batches_restart_and_reclaim_last_reference() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.db");
    let t = table_names("shared");
    let mut conn = open_writer_database(&path).unwrap();
    create_schema(&mut conn, &t).unwrap();
    let mut a = sample_record_row();
    a.created_at_ms = 1;
    let mut b = a.clone();
    b.created_at_ms = 2;
    let steps = vec![step(0, "a"), step(1, "a")];
    write(
        &mut conn,
        &t,
        &[PreparedRecord::new(a, steps.clone()).unwrap()],
    );
    drop(conn);
    let mut conn = open_writer_database(&path).unwrap();
    create_schema(&mut conn, &t).unwrap();
    write(
        &mut conn,
        &t,
        &[PreparedRecord::new(b, steps.clone()).unwrap()],
    );
    assert_eq!(count(&conn, &t.traces), 1);
    assert_eq!(count(&conn, &t.question_sets), 1);
    assert_eq!(count(&conn, &t.trace_steps), 2);
    assert!(
        conn.execute(&format!("DELETE FROM {}", t.traces), [])
            .is_err()
    );
    assert_eq!(delete_batch(&mut conn, &t, Some(2)).unwrap(), 1);
    assert_eq!(count(&conn, &t.traces), 1);
    assert_eq!(count(&conn, &t.question_sets), 1);
    assert_eq!(delete_batch(&mut conn, &t, None).unwrap(), 1);
    for name in [
        &t.records,
        &t.traces,
        &t.trace_steps,
        &t.question_sets,
        &t.question_items,
    ] {
        assert_eq!(count(&conn, name), 0);
    }
    assert_eq!(count(&conn, &t.meta), 1);
}

#[test]
fn identity_and_injected_collisions_preserve_complete_content() {
    let mut conn = Connection::open_in_memory().unwrap();
    let t = table_names("collision");
    create_schema(&mut conn, &t).unwrap();
    let base = sample_record_row();
    let mut variants = vec![(base.clone(), vec![step(0, "a"), step(1, "b")])];
    variants.push((base.clone(), vec![step(0, "b"), step(1, "a")]));
    let mut other = step(0, "a");
    other.outcome = "not_matched".into();
    variants.push((base.clone(), vec![other]));
    for field in ["case", "type", "class", "duplicate", "empty"] {
        let mut row = base.clone();
        match field {
            "case" => row.questions_json[0].name = "EXAMPLE.com.".into(),
            "type" => row.questions_json[0].qtype = "AAAA".into(),
            "class" => row.questions_json[0].qclass = "CH".into(),
            "duplicate" => row.questions_json.push(row.questions_json[0].clone()),
            _ => row.questions_json.clear(),
        }
        variants.push((row, vec![]));
    }
    let mut order = base.clone();
    let mut second = order.questions_json[0].clone();
    second.name = "second.".into();
    order.questions_json.push(second);
    variants.push((order.clone(), vec![]));
    order.questions_json.reverse();
    variants.push((order, vec![]));
    let prepared: Vec<_> = variants
        .iter()
        .map(|(r, s)| {
            let mut p = PreparedRecord::new(r.clone(), s.clone()).unwrap();
            p.trace_fingerprint = [0; 32];
            p.question_fingerprint = [0; 32];
            p
        })
        .collect();
    for _ in 0..2 {
        write(&mut conn, &t, &prepared);
    }
    assert_eq!(count(&conn, &t.traces), 4);
    assert_eq!(count(&conn, &t.question_sets), 8);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {} FROM {} ORDER BY id",
            record_row_select_columns(None),
            t.records
        ))
        .unwrap();
    let stored = stmt
        .query_map([], StoredRecord::read)
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for (i, r) in stored.iter().enumerate() {
        assert_eq!(
            load_steps(&conn, &t, r.trace_id).unwrap(),
            variants[i % variants.len()].1
        );
    }
    let assembled = assemble_records(&conn, &t, stored).unwrap();
    for (i, mut r) in assembled.into_iter().enumerate() {
        r.id = 0;
        assert_eq!(r, variants[i % variants.len()].0);
    }
}

#[test]
fn failed_insert_and_failed_reclamation_roll_back_whole_batch() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    let t = table_names("rollback");
    create_schema(&mut conn, &t).unwrap();
    let duplicate_index = vec![step(0, "a"), step(0, "b")];
    let tx = conn.transaction().unwrap();
    assert!(
        insert_batch(
            &tx,
            &t,
            &[PreparedRecord::new(sample_record_row(), duplicate_index).unwrap()]
        )
        .is_err()
    );
    drop(tx);
    assert_eq!(count(&conn, &t.traces), 0);
    write(
        &mut conn,
        &t,
        &[PreparedRecord::new(sample_record_row(), vec![step(0, "a")]).unwrap()],
    );
    conn.execute_batch(&format!("CREATE TRIGGER fail_reclaim BEFORE DELETE ON {} BEGIN SELECT RAISE(ABORT,'injected'); END;",t.traces)).unwrap();
    assert!(delete_batch(&mut conn, &t, None).is_err());
    assert_eq!(count(&conn, &t.records), 1);
    assert_eq!(count(&conn, &t.traces), 1);
    assert_eq!(count(&conn, &t.trace_steps), 1);
}

#[test]
fn missing_shared_references_are_errors() {
    for missing in ["trace", "questions"] {
        let mut conn = Connection::open_in_memory().unwrap();
        let t = table_names(missing);
        create_schema(&mut conn, &t).unwrap();
        write(
            &mut conn,
            &t,
            &[PreparedRecord::new(sample_record_row(), vec![]).unwrap()],
        );
        conn.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        conn.execute(
            &format!(
                "DELETE FROM {}",
                if missing == "trace" {
                    &t.traces
                } else {
                    &t.question_sets
                }
            ),
            [],
        )
        .unwrap();
        let stored = conn
            .query_row(
                &format!(
                    "SELECT {} FROM {}",
                    record_row_select_columns(None),
                    t.records
                ),
                [],
                StoredRecord::read,
            )
            .unwrap();
        assert!(assemble_records(&conn, &t, vec![stored]).is_err());
    }
}

#[test]
fn schema_revision_and_v1_are_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let mut conn = open_writer_database(&path).unwrap();
    conn.execute_batch(include_str!("fixtures/v1.sql")).unwrap();
    write_v1(&mut conn, &[(sample_record_row(), vec![step(0, "legacy")])]);
    conn.execute("INSERT INTO meta VALUES ('old-marker','untouched')", [])
        .unwrap();
    let schema: String = conn
        .query_row(
            "SELECT group_concat(sql) FROM sqlite_schema WHERE sql IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let t = table_names("legacy");
    create_schema(&mut conn, &t).unwrap();
    write(
        &mut conn,
        &t,
        &[PreparedRecord::new(sample_record_row(), vec![]).unwrap()],
    );
    run_cleanup(&mut conn, &path, &t, i64::MAX).unwrap();
    write(
        &mut conn,
        &t,
        &[PreparedRecord::new(sample_record_row(), vec![]).unwrap()],
    );
    run_clear_history(&mut conn, &path, &t, &Arc::new(Mutex::new(VecDeque::new()))).unwrap();
    drop(conn);
    let mut conn = open_writer_database(&path).unwrap();
    create_schema(&mut conn, &t).unwrap();
    let preserved:String=conn.query_row("SELECT group_concat(sql) FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT LIKE 'qr_%'",[],|r|r.get(0)).unwrap();
    assert_eq!(schema, preserved);
    assert_eq!(count(&conn, "meta"), 1);
    assert_eq!(count(&conn, "records"), 1);
    assert_eq!(count(&conn, "steps"), 1);
    assert_eq!(count(&conn, "questions"), 1);
    conn.execute(&format!("UPDATE {} SET value='99'", t.meta), [])
        .unwrap();
    assert!(create_schema(&mut conn, &t).is_err());
    assert_eq!(count(&conn, &t.meta), 1);
    conn.execute(&format!("DROP TABLE {}", t.question_items), [])
        .unwrap();
    assert!(create_schema(&mut conn, &t).is_err());
}

#[test]
fn full_response_snapshot_roundtrips_all_sections_and_opaque_payloads() {
    let mut record = sample_record_row();
    let mut rr = record.answers_json[0].clone();
    for (kind, payload) in [
        ("TXT", serde_json::json!({"texts":["a".repeat(3000),"b"]})),
        ("RRSIG", serde_json::json!({"signature":[0,255,128]})),
        (
            "Unknown",
            serde_json::json!({"type":65400,"bytes":[0,255,1]}),
        ),
    ] {
        rr.rr_type = kind.into();
        rr.payload_kind = kind.into();
        rr.payload = payload;
        rr.ttl += 1;
        record.answers_json.push(rr.clone());
    }
    record.authorities_json = record.answers_json.clone();
    record.additionals_json = record.answers_json.clone();
    record.signature_json = record.answers_json.clone();
    record.req_edns_json.as_mut().unwrap().options[0].payload =
        serde_json::json!({"bytes":[0,255,128]});
    let compressed = super::super::payload::EncodedPayload::encode(&record).unwrap();
    assert_eq!(compressed.codec, 1);
    let raw =
        super::super::payload::EncodedPayload::encode_with_compression(&record, false).unwrap();
    assert_eq!(compressed.decode().unwrap(), raw.decode().unwrap());
    let mut conn = Connection::open_in_memory().unwrap();
    let t = table_names("response");
    create_schema(&mut conn, &t).unwrap();
    write(
        &mut conn,
        &t,
        &[PreparedRecord::new(record.clone(), vec![]).unwrap()],
    );
    let stored = conn
        .query_row(
            &format!(
                "SELECT {} FROM {}",
                record_row_select_columns(None),
                t.records
            ),
            [],
            StoredRecord::read,
        )
        .unwrap();
    record.id = 1;
    assert_eq!(
        assemble_records(&conn, &t, vec![stored]).unwrap(),
        vec![record]
    );
}

// Retained only as a benchmark/legacy-fixture writer, never called by
// production.
fn write_v1(conn: &mut Connection, details: &[(RecordRow, Vec<StepJson>)]) {
    let encoded: Vec<_> = details
        .iter()
        .map(|(r, steps)| {
            let serde_json::Value::Object(mut fields) = serde_json::to_value(r).unwrap() else {
                unreachable!()
            };
            fields.remove("id");
            let (columns, values): (Vec<_>, Vec<_>) = fields
                .into_iter()
                .map(|(k, v)| {
                    let value = if k.ends_with("_json") && !v.is_null() {
                        Value::Text(v.to_string())
                    } else {
                        match v {
                            serde_json::Value::Null => Value::Null,
                            serde_json::Value::String(s) => Value::Text(s),
                            serde_json::Value::Bool(b) => Value::Integer(i64::from(b)),
                            serde_json::Value::Number(n) => Value::Integer(n.as_i64().unwrap()),
                            _ => unreachable!(),
                        }
                    };
                    (k, value)
                })
                .unzip();
            (columns, values, steps, &r.questions_json)
        })
        .collect();
    let tx = conn.transaction().unwrap();
    for (columns, values, steps, questions) in encoded {
        tx.prepare_cached(&format!(
            "INSERT INTO records ({}) VALUES ({})",
            columns.join(","),
            vec!["?"; values.len()].join(",")
        ))
        .unwrap()
        .execute(params_from_iter(values))
        .unwrap();
        let id = tx.last_insert_rowid();
        for s in steps {
            tx.prepare_cached("INSERT INTO steps VALUES (?,?,?,?,?,?,?)")
                .unwrap()
                .execute(params![
                    id,
                    s.event_index as i64,
                    s.sequence_tag,
                    s.node_index.map(|v| v as i64),
                    s.kind,
                    s.tag,
                    s.outcome
                ])
                .unwrap();
        }
        for (i, q) in questions.iter().enumerate() {
            tx.prepare_cached("INSERT INTO questions VALUES (?,?,?,?,?)")
                .unwrap()
                .execute(params![
                    id,
                    i as i64,
                    q.name.to_ascii_lowercase(),
                    q.qtype,
                    q.qclass
                ])
                .unwrap();
        }
    }
    tx.commit().unwrap();
}

#[test]
#[ignore = "manual disk/throughput comparison; run with --ignored --nocapture"]
fn benchmark_storage_variants() {
    use std::time::Instant;
    const N: usize = 2000;
    println!(
        "scenario,variant,records,db_bytes,bytes_per_record,write_records_s,page_100_us,stats_us"
    );
    for scenario in [
        "repeated",
        "no_steps",
        "unique_questions",
        "unique_questions_no_steps",
        "empty_response",
        "unique_paths",
        "large_response",
        "entropy",
    ] {
        let details: Vec<_> = (0..N)
            .map(|i| {
                let mut r = sample_record_row();
                r.created_at_ms += i as i64;
                r.request_id = i as u16;
                let mut steps: Vec<_> = (0..12).map(|n| step(n, &format!("plugin_{n}"))).collect();
                match scenario {
                    "no_steps" => steps.clear(),
                    "unique_questions" => r.questions_json[0].name = format!("q{i}.example.com."),
                    "unique_questions_no_steps" => {
                        r.questions_json[0].name = format!("q{i}.example.com.");
                        steps.clear();
                    }
                    "empty_response" => {
                        steps.clear();
                        r.req_edns_json = None;
                        r.resp_edns_json = None;
                        r.answers_json.clear();
                        r.answer_count = 0;
                        r.has_response = false;
                        r.rcode = None;
                        r.resp_aa = None;
                        r.resp_tc = None;
                        r.resp_ra = None;
                        r.resp_ad = None;
                        r.resp_cd = None;
                    }
                    "unique_paths" => {
                        for s in &mut steps {
                            s.sequence_tag = format!("unique_{i}");
                        }
                    }
                    "large_response" => {
                        r.answers_json = vec![r.answers_json[0].clone(); 32];
                        r.answer_count = 32;
                    }
                    "entropy" => {
                        let mut state = i as u64 + 1;
                        let text: String = (0..8192)
                            .map(|_| {
                                state ^= state << 13;
                                state ^= state >> 7;
                                state ^= state << 17;
                                char::from(b'!' + (state % 90) as u8)
                            })
                            .collect();
                        r.answers_json[0].payload = serde_json::json!({"opaque":text});
                    }
                    _ => {}
                }
                (r, steps)
            })
            .collect();
        for variant in ["v1", "v2_raw", "v2_zlib"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("bench.db");
            let mut conn = open_writer_database(&path).unwrap();
            let t = table_names("bench");
            if variant == "v1" {
                conn.execute_batch(include_str!("fixtures/v1.sql")).unwrap();
            } else {
                create_schema(&mut conn, &t).unwrap();
            }
            let start = Instant::now();
            for batch in details.chunks(128) {
                if variant == "v1" {
                    write_v1(&mut conn, batch);
                } else {
                    let prepared: Vec<_> = batch
                        .iter()
                        .map(|(r, s)| {
                            PreparedRecord::with_compression(
                                r.clone(),
                                s.clone(),
                                variant != "v2_raw",
                            )
                            .unwrap()
                        })
                        .collect();
                    write(&mut conn, &t, &prepared);
                }
            }
            let throughput = N as f64 / start.elapsed().as_secs_f64();
            checkpoint_wal(&conn).unwrap();
            let bytes = std::fs::metadata(&path).unwrap().len();
            let start = Instant::now();
            for _ in 0..20 {
                if variant == "v1" {
                    let mut stmt=conn.prepare("SELECT questions_json,req_edns_json,answers_json,authorities_json,additionals_json,signature_json,resp_edns_json FROM records ORDER BY created_at_ms DESC,id DESC LIMIT 100").unwrap();
                    let mut rows = stmt.query([]).unwrap();
                    while let Some(row) = rows.next().unwrap() {
                        for i in 0..7 {
                            if let Some(json) = row.get::<_, Option<String>>(i).unwrap() {
                                std::hint::black_box(
                                    serde_json::from_str::<serde_json::Value>(&json).unwrap(),
                                );
                            }
                        }
                    }
                } else {
                    let mut stmt = conn
                        .prepare(&format!(
                            "SELECT {} FROM {} ORDER BY created_at_ms DESC,id DESC LIMIT 100",
                            record_row_select_columns(None),
                            t.records
                        ))
                        .unwrap();
                    let rows = stmt
                        .query_map([], StoredRecord::read)
                        .unwrap()
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .unwrap();
                    std::hint::black_box(assemble_records(&conn, &t, rows).unwrap());
                }
            }
            let page_us = start.elapsed().as_micros() / 20;
            let (records, steps, reference) = if variant == "v1" {
                ("records", "steps", "r.id")
            } else {
                (t.records.as_str(), t.trace_steps.as_str(), "r.trace_id")
            };
            let step_ref = if variant == "v1" {
                "record_id"
            } else {
                "trace_id"
            };
            let sql = format!(
                "WITH sample AS (SELECT * FROM {records} ORDER BY created_at_ms DESC,id DESC LIMIT 10000) SELECT s.tag,COUNT(*),COUNT(DISTINCT r.id) FROM sample r JOIN {steps} s ON s.{step_ref}={reference} GROUP BY s.tag"
            );
            let start = Instant::now();
            for _ in 0..10 {
                let mut stmt = conn.prepare(&sql).unwrap();
                let mut rows = stmt.query([]).unwrap();
                while rows.next().unwrap().is_some() {}
            }
            let stats_us = start.elapsed().as_micros() / 10;
            println!(
                "{scenario},{variant},{N},{bytes},{:.1},{throughput:.0},{page_us},{stats_us}",
                bytes as f64 / N as f64
            );
            if let Ok(mut stmt) =
                conn.prepare("SELECT name,SUM(pgsize) FROM dbstat GROUP BY name ORDER BY name")
            {
                let sizes = stmt
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                println!("objects,{scenario},{variant},{sizes:?}");
            }
            let plan=conn.prepare(&format!("EXPLAIN QUERY PLAN SELECT id FROM {records} ORDER BY created_at_ms DESC,id DESC LIMIT 100")).unwrap().query_map([],|r|r.get::<_,String>(3)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
            println!("plan,{scenario},{variant},{plan:?}");
        }
    }
}

#[test]
fn query_plans_use_the_retained_indexes() {
    let mut conn = Connection::open_in_memory().unwrap();
    let t = table_names("plans");
    create_schema(&mut conn, &t).unwrap();
    for (sql, index) in [
        (
            format!(
                "SELECT id FROM {} ORDER BY created_at_ms DESC,id DESC LIMIT 100",
                t.records
            ),
            format!("{}_created_at_idx", t.records),
        ),
        (
            format!(
                "SELECT id FROM {} WHERE rcode='noerror' COLLATE NOCASE",
                t.records
            ),
            format!("{}_rcode_idx", t.records),
        ),
        (
            format!(
                "SELECT trace_id FROM {} WHERE kind='matcher' AND tag='test' AND outcome='matched'",
                t.trace_steps
            ),
            format!("{}_matcher_idx", t.trace_steps),
        ),
        (
            format!(
                "SELECT question_set_id FROM {} WHERE qtype='AAAA'",
                t.question_items
            ),
            format!("{}_qtype_idx", t.question_items),
        ),
        (
            format!("SELECT id FROM {} WHERE trace_id=1", t.records),
            format!("{}_trace_idx", t.records),
        ),
        (
            format!("SELECT id FROM {} WHERE question_set_id=1", t.records),
            format!("{}_question_set_idx", t.records),
        ),
    ] {
        let plan = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        assert!(plan.contains(&index), "{sql}: {plan}");
    }
}

#[test]
fn schema_initialization_failure_is_atomic() {
    let mut conn = Connection::open_in_memory().unwrap();
    let t = table_names("atomic");
    conn.execute_batch(&format!("CREATE VIEW {} AS SELECT 1", t.question_sets))
        .unwrap();
    assert!(create_schema(&mut conn, &t).is_err());
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
}
