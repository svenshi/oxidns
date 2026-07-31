// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;

use rusqlite::Connection;
use tempfile::NamedTempFile;

use super::backend::WriterCommand;
use super::model::{
    DistributionQuery, LatencyQuery, ListQuery, PendingRecord, PluginStatsKind, PluginsStatsQuery,
    QueryRecordFilter, QueryRecordStatus, QueryRecorderConfig, TimeseriesBucket, TimeseriesQuery,
    TopQuery,
};
use super::store::{
    create_schema, load_latency_summary, load_plugin_stats, load_qtype_distribution,
    load_rcode_distribution, load_timeseries, load_top_clients, load_top_qnames,
    open_reader_database, open_writer_database, query_records, table_names,
};
use super::{QueryRecorder, QueryRecorderFactory, resolve_config, should_record};
use crate::core::context::{DnsContext, ExecutionPathEvent};
use crate::infra::clock::AppClock;
use crate::infra::error::DnsError;
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::test_utils::test_context;
use crate::plugin::{Plugin, PluginFactory};
use crate::proto::rdata::{A, CNAME};
use crate::proto::{DNSClass, Message, Name, Question, RData, Rcode, Record, RecordType};

fn recorder_config(path: &str) -> serde_yaml_ng::Value {
    serde_yaml_ng::to_value(QueryRecorderConfig {
        path: path.to_string(),
        queue_size: Some(32),
        batch_size: Some(1),
        flush_interval_ms: Some(10),
        memory_tail: Some(16),
        retention_days: Some(7),
        cleanup_interval_hours: Some(1),
        reader_concurrency: Some(2),
        max_steps: Some(512),
        context: BTreeMap::new(),
        include_marks: Vec::new(),
        exclude_marks: Vec::new(),
    })
    .unwrap()
}

fn recorder_space_config(path: &str) -> serde_yaml_ng::Value {
    serde_yaml_ng::to_value(QueryRecorderConfig {
        path: path.to_string(),
        queue_size: Some(4_096),
        batch_size: Some(256),
        flush_interval_ms: Some(10),
        memory_tail: Some(16),
        retention_days: Some(7),
        cleanup_interval_hours: Some(1),
        reader_concurrency: Some(2),
        max_steps: Some(512),
        context: BTreeMap::new(),
        include_marks: Vec::new(),
        exclude_marks: Vec::new(),
    })
    .unwrap()
}

#[test]
fn record_mark_filters_are_opt_in_and_exclusion_wins() {
    let temp = NamedTempFile::new().unwrap();
    let mut value = recorder_config(&temp.path().display().to_string());
    let mapping = value.as_mapping_mut().unwrap();
    mapping.insert(
        serde_yaml_ng::Value::String("include_marks".to_string()),
        serde_yaml_ng::to_value([41_u32]).unwrap(),
    );
    mapping.insert(
        serde_yaml_ng::Value::String("exclude_marks".to_string()),
        serde_yaml_ng::to_value([42_u32]).unwrap(),
    );
    let config = resolve_config(Some(value)).unwrap();
    let mut context = test_context();

    assert!(!should_record(&config, &context));
    context.marks_mut().insert(41);
    assert!(should_record(&config, &context));
    context.marks_mut().insert(42);
    assert!(!should_record(&config, &context));
}

fn list_query(filter: QueryRecordFilter) -> ListQuery {
    ListQuery {
        cursor: None,
        limit: 20,
        since_ms: None,
        until_ms: None,
        filter,
    }
}

fn filtered_record_ids(
    backend: std::sync::Arc<super::backend::RecorderBackend>,
    query: ListQuery,
) -> Vec<u16> {
    query_records(backend, query)
        .unwrap()
        .0
        .into_iter()
        .map(|record| record.request_id)
        .collect()
}

async fn flush_backend(backend: &std::sync::Arc<super::backend::RecorderBackend>) {
    let flush_backend = backend.clone();
    tokio::task::spawn_blocking(move || flush_backend.flush_for_test())
        .await
        .unwrap()
        .unwrap();
}

async fn seed_bulk_records(
    backend: &std::sync::Arc<super::backend::RecorderBackend>,
    count: usize,
) {
    for index in 0..count {
        backend.enqueue(pending_record(
            index as i64,
            index as u16,
            "space-reclaim.example.com.",
            RecordType::A,
            Ipv4Addr::new(192, 0, 2, 1),
            Some(Rcode::NoError),
            None,
            &[("matcher_for_space_reclaim", "matched")],
        ));
    }
    flush_backend(backend).await;
}

fn prepare_legacy_database(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE legacy_marker (id INTEGER PRIMARY KEY);",
    )
    .unwrap();
    let mode: i64 = conn
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, 0);
}

#[allow(clippy::too_many_arguments)]
fn pending_record(
    created_at_ms: i64,
    request_id: u16,
    name: &str,
    qtype: RecordType,
    client_ip: Ipv4Addr,
    response_rcode: Option<Rcode>,
    error: Option<&str>,
    matcher_events: &[(&str, &str)],
) -> PendingRecord {
    let mut request = Message::new();
    request.set_id(request_id);
    request.add_question(Question::new(
        Name::from_ascii(name).unwrap(),
        qtype,
        DNSClass::IN,
    ));
    let response = response_rcode.map(|rcode| request.response(rcode));
    let mut ctx = DnsContext::new(SocketAddr::from((client_ip, 5300)), request.clone());
    ctx.enable_execution_path();
    for (idx, (tag, outcome)) in matcher_events.iter().enumerate() {
        ctx.push_execution_path_event(ExecutionPathEvent::new(
            "seq",
            Some(idx),
            "matcher",
            Some(*tag),
            *outcome,
        ));
    }
    PendingRecord::new(
        request,
        response,
        created_at_ms,
        1,
        ctx.execution_path.clone(),
        0,
        ctx.peer_addr(),
        error.map(ToString::to_string),
        BTreeMap::new(),
    )
}

#[test]
fn test_table_names_include_tag_hash_and_version() {
    let tables = table_names("Recorder.Main");
    assert!(tables.records.starts_with("qr_recorder_main_"));
    assert!(tables.records.ends_with("_v1_records"));
    assert!(tables.steps.ends_with("_v1_steps"));
    assert!(tables.questions.ends_with("_v1_questions"));
    assert!(tables.meta.ends_with("_v1_meta"));
}

#[test]
fn test_open_writer_database_enables_incremental_auto_vacuum_for_new_database() {
    let temp = NamedTempFile::new().unwrap();
    let tables = table_names("rec");
    let mut conn = open_writer_database(temp.path()).unwrap();

    create_schema(&mut conn, &tables).unwrap();

    let mode: i64 = conn
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, 2);
}

#[test]
fn test_open_reader_database_uses_low_memory_read_pragmas() {
    let temp = NamedTempFile::new().unwrap();
    let tables = table_names("rec");
    {
        let mut conn = open_writer_database(temp.path()).unwrap();
        create_schema(&mut conn, &tables).unwrap();
    }

    let conn = open_reader_database(temp.path()).unwrap();
    let query_only: i64 = conn
        .query_row("PRAGMA query_only", [], |row| row.get(0))
        .unwrap();
    let cache_size: i64 = conn
        .query_row("PRAGMA cache_size", [], |row| row.get(0))
        .unwrap();
    let mmap_size: i64 = conn
        .query_row("PRAGMA mmap_size", [], |row| row.get(0))
        .unwrap();
    let temp_store: i64 = conn
        .query_row("PRAGMA temp_store", [], |row| row.get(0))
        .unwrap();

    assert_eq!(query_only, 1);
    assert_eq!(cache_size, -4096);
    assert_eq!(mmap_size, 0);
    assert_eq!(temp_store, 1);
}

#[test]
fn test_record_capture_without_response_uses_empty_sections() {
    let mut ctx = test_context();
    let mut request = Message::new();
    request.set_id(7);
    request.set_recursion_desired(true);
    request.add_question(Question::new(
        Name::from_ascii("example.com.").unwrap(),
        RecordType::A,
        DNSClass::IN,
    ));

    ctx.enable_execution_path();
    ctx.push_execution_path_event(ExecutionPathEvent::new(
        "seq",
        Some(0),
        "executor",
        Some("query_recorder"),
        "entered",
    ));

    let pending = PendingRecord::new(
        request,
        ctx.response.clone(),
        100,
        10,
        ctx.execution_path.clone(),
        0,
        ctx.peer_addr(),
        Some(DnsError::plugin("boom").to_string()),
        BTreeMap::new(),
    );
    let (record, steps) = pending.take_to_record();

    assert!(!record.has_response);
    assert_eq!(record.answer_count, 0);
    assert!(record.answers_json.is_empty());
    assert!(
        record
            .error
            .as_deref()
            .is_some_and(|value| value.contains("boom"))
    );
    assert_eq!(steps.len(), 1);
}

#[test]
fn test_record_capture_with_structured_response() {
    let mut ctx = test_context();
    let mut request = Message::new();
    request.set_id(9);
    request.add_question(Question::new(
        Name::from_ascii("example.com.").unwrap(),
        RecordType::A,
        DNSClass::IN,
    ));

    let mut response = request.response(Rcode::NoError);
    response.set_authoritative(true);
    response.set_recursion_available(true);
    response.add_answer(Record::from_rdata(
        Name::from_ascii("example.com.").unwrap(),
        300,
        RData::A(A(Ipv4Addr::new(1, 1, 1, 1))),
    ));
    response.add_authority(Record::from_rdata(
        Name::from_ascii("example.com.").unwrap(),
        60,
        RData::CNAME(CNAME(Name::from_ascii("alias.example.com.").unwrap())),
    ));
    ctx.set_response(response);
    ctx.enable_execution_path();

    let pending = PendingRecord::new(
        request,
        ctx.response.clone(),
        100,
        12,
        ctx.execution_path.clone(),
        0,
        ctx.peer_addr(),
        None,
        BTreeMap::new(),
    );
    let (record, _) = pending.take_to_record();

    assert!(record.has_response);
    assert_eq!(record.answer_count, 1);
    assert_eq!(record.authority_count, 1);
    assert_eq!(record.answers_json[0].payload_kind, "A");
    assert_eq!(record.authorities_json[0].payload_kind, "CNAME");
}

#[tokio::test]
async fn test_query_recorder_execute_enqueues_record() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(
        serde_yaml_ng::to_value(QueryRecorderConfig {
            path: temp.path().display().to_string(),
            queue_size: Some(16),
            batch_size: Some(1),
            flush_interval_ms: Some(10),
            memory_tail: Some(8),
            retention_days: Some(7),
            cleanup_interval_hours: Some(1),
            reader_concurrency: Some(2),
            max_steps: Some(512),
            context: BTreeMap::from([
                (
                    "schema".to_string(),
                    "standard-query-diagnostic:1".to_string(),
                ),
                ("intentRevision".to_string(), "sha256:test".to_string()),
            ]),
            include_marks: Vec::new(),
            exclude_marks: Vec::new(),
        })
        .unwrap(),
    ))
    .unwrap();

    let mut plugin = QueryRecorder::new("rec".to_string(), config.clone());
    plugin.init_for_test().await.unwrap();

    let mut ctx = DnsContext::new(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)),
        Message::new(),
    );
    let step = plugin.execute_with_next(&mut ctx, None).await.unwrap();
    assert_eq!(step, ExecStep::Next);

    let backend = plugin.backend.as_ref().unwrap().clone();
    flush_backend(&backend).await;
    let records = tokio::task::spawn_blocking(move || {
        query_records(
            backend,
            ListQuery {
                cursor: None,
                limit: 10,
                since_ms: None,
                until_ms: None,
                filter: QueryRecordFilter::default(),
            },
        )
    })
    .await
    .unwrap()
    .unwrap()
    .0;
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0]
            .diagnostic_context
            .get("intentRevision")
            .map(String::as_str),
        Some("sha256:test")
    );
    assert!(!records[0].steps_truncated);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_list_cursor_only_when_more_records_exist() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(
        serde_yaml_ng::to_value(QueryRecorderConfig {
            path: temp.path().display().to_string(),
            queue_size: Some(16),
            batch_size: Some(1),
            flush_interval_ms: Some(10),
            memory_tail: Some(8),
            retention_days: Some(7),
            cleanup_interval_hours: Some(1),
            reader_concurrency: Some(2),
            max_steps: Some(512),
            context: BTreeMap::new(),
            include_marks: Vec::new(),
            exclude_marks: Vec::new(),
        })
        .unwrap(),
    ))
    .unwrap();

    let mut plugin = QueryRecorder::new("rec".to_string(), config.clone());
    plugin.init_for_test().await.unwrap();

    for request_id in 1..=3 {
        let mut request = Message::new();
        request.set_id(request_id);
        let mut ctx = DnsContext::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)), request);
        plugin.execute_with_next(&mut ctx, None).await.unwrap();
    }

    let backend = plugin.backend.as_ref().unwrap().clone();
    flush_backend(&backend).await;
    let (first_page, first_cursor) = tokio::task::spawn_blocking(move || {
        query_records(
            backend,
            ListQuery {
                cursor: None,
                limit: 2,
                since_ms: None,
                until_ms: None,
                filter: QueryRecordFilter::default(),
            },
        )
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(first_page.len(), 2);
    assert!(first_cursor.is_some());

    let cursor_record = first_page.last().unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();
    let (second_page, second_cursor) = tokio::task::spawn_blocking({
        let cursor = super::model::ListCursor {
            created_at_ms: cursor_record.created_at_ms,
            id: cursor_record.id,
        };
        move || {
            query_records(
                backend,
                ListQuery {
                    cursor: Some(cursor),
                    limit: 2,
                    since_ms: None,
                    until_ms: None,
                    filter: QueryRecordFilter::default(),
                },
            )
        }
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(second_page.len(), 1);
    assert!(second_cursor.is_none());

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_clear_history_removes_records_and_tail() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    seed_demo_records(&backend).await;
    assert!(!backend.tail.lock().unwrap().is_empty());

    let clear_backend = backend.clone();
    let clear_result = tokio::task::spawn_blocking(move || clear_backend.clear_history())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(clear_result.cleared_records, 5);
    assert!(backend.tail.lock().unwrap().is_empty());

    let records = query_records(backend, list_query(QueryRecordFilter::default()))
        .unwrap()
        .0;
    assert!(records.is_empty());

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_clear_history_does_not_wait_for_reader_permits() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    seed_demo_records(&backend).await;
    let _reader_a = backend
        .reader_semaphore
        .clone()
        .try_acquire_owned()
        .expect("first reader permit should be available");
    let _reader_b = backend
        .reader_semaphore
        .clone()
        .try_acquire_owned()
        .expect("second reader permit should be available");

    let clear_backend = backend.clone();
    let clear_result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::task::spawn_blocking(move || clear_backend.clear_history()),
    )
    .await
    .expect("clear should not wait for reader permits")
    .unwrap()
    .unwrap();

    assert_eq!(clear_result.cleared_records, 5);
    assert!(backend.tail.lock().unwrap().is_empty());

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_periodic_cleanup_reclaims_database_and_wal_space() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_space_config(
        &temp.path().display().to_string(),
    )))
    .unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    seed_bulk_records(&backend, 2_000).await;
    let cleanup_backend = backend.clone();
    let result = tokio::task::spawn_blocking(move || cleanup_backend.cleanup(i64::MAX))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(result.deleted_records, 2_000);
    assert!(!result.space.migrated);
    assert_eq!(result.space.after.auto_vacuum, 2);
    assert_eq!(result.space.after.freelist_count, 0);
    assert_eq!(result.space.after.wal_bytes, 0);
    assert!(result.space.peak_wal_bytes > 0);
    assert!(result.space.reclaimable.freelist_count > 0);
    assert!(result.space.after.page_count < result.space.reclaimable.page_count);
    assert!(result.space.after.total_bytes() < result.space.before.total_bytes());

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_manual_clear_reclaims_database_and_wal_space() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_space_config(
        &temp.path().display().to_string(),
    )))
    .unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    seed_bulk_records(&backend, 2_000).await;
    let clear_backend = backend.clone();
    let result = tokio::task::spawn_blocking(move || clear_backend.clear_history())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(result.cleared_records, 2_000);
    assert_eq!(result.space.after.freelist_count, 0);
    assert_eq!(result.space.after.wal_bytes, 0);
    assert!(result.space.peak_wal_bytes > 0);
    assert!(result.space.reclaimable.freelist_count > 0);
    assert!(result.space.after.page_count < result.space.reclaimable.page_count);
    assert!(result.space.after.total_bytes() < result.space.before.total_bytes());

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_periodic_cleanup_migrates_legacy_database() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    prepare_legacy_database(temp.path());
    let config = resolve_config(Some(recorder_space_config(
        &temp.path().display().to_string(),
    )))
    .unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    seed_bulk_records(&backend, 1_000).await;
    let cleanup_backend = backend.clone();
    let result = tokio::task::spawn_blocking(move || cleanup_backend.cleanup(i64::MAX))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(result.deleted_records, 1_000);
    assert!(result.space.migrated);
    assert_eq!(result.space.reclaimable.auto_vacuum, 0);
    assert_eq!(result.space.after.auto_vacuum, 2);
    assert_eq!(result.space.after.freelist_count, 0);
    assert_eq!(result.space.after.wal_bytes, 0);
    assert!(result.space.after.page_count < result.space.reclaimable.page_count);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_manual_clear_migrates_legacy_database() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    prepare_legacy_database(temp.path());
    let config = resolve_config(Some(recorder_space_config(
        &temp.path().display().to_string(),
    )))
    .unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    seed_bulk_records(&backend, 1_000).await;
    let clear_backend = backend.clone();
    let result = tokio::task::spawn_blocking(move || clear_backend.clear_history())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(result.cleared_records, 1_000);
    assert!(result.space.migrated);
    assert_eq!(result.space.after.auto_vacuum, 2);
    assert_eq!(result.space.after.freelist_count, 0);
    assert_eq!(result.space.after.wal_bytes, 0);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_clear_waits_for_active_database_reader() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();
    seed_demo_records(&backend).await;

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let coordinator = backend.database_coordinator.clone();
    let reader = std::thread::spawn(move || {
        let _access = coordinator.read_access().unwrap();
        ready_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    ready_rx.recv().unwrap();

    let clear_backend = backend.clone();
    let mut clear_task = tokio::task::spawn_blocking(move || clear_backend.clear_history());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut clear_task)
            .await
            .is_err()
    );

    release_tx.send(()).unwrap();
    reader.join().unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), clear_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result.cleared_records, 5);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_shared_database_clear_preserves_other_recorder() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin_a = QueryRecorder::new("rec-a".to_string(), config.clone());
    let mut plugin_b = QueryRecorder::new("rec-b".to_string(), config);
    plugin_a.init_for_test().await.unwrap();
    plugin_b.init_for_test().await.unwrap();
    let backend_a = plugin_a.backend.as_ref().unwrap().clone();
    let backend_b = plugin_b.backend.as_ref().unwrap().clone();

    backend_a.enqueue(pending_record(
        1_000,
        1,
        "a.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 1),
        Some(Rcode::NoError),
        None,
        &[],
    ));
    backend_b.enqueue(pending_record(
        2_000,
        2,
        "b.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 2),
        Some(Rcode::NoError),
        None,
        &[],
    ));
    flush_backend(&backend_a).await;
    flush_backend(&backend_b).await;

    let clear_backend = backend_a.clone();
    tokio::task::spawn_blocking(move || clear_backend.clear_history())
        .await
        .unwrap()
        .unwrap();

    assert!(
        query_records(backend_a, list_query(QueryRecordFilter::default()))
            .unwrap()
            .0
            .is_empty()
    );
    let records = query_records(backend_b, list_query(QueryRecordFilter::default()))
        .unwrap()
        .0;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].request_id, 2);

    plugin_b.destroy().await.unwrap();
    plugin_a.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_cleanup_failure_does_not_stop_writer() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();
    seed_demo_records(&backend).await;

    let blocker = Connection::open(temp.path()).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let cleanup_backend = backend.clone();
    let result = tokio::task::spawn_blocking(move || cleanup_backend.cleanup(i64::MAX))
        .await
        .unwrap();
    assert!(result.is_err());
    blocker.execute_batch("ROLLBACK;").unwrap();

    backend.enqueue(pending_record(
        10_000,
        10,
        "after-error.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 10),
        Some(Rcode::NoError),
        None,
        &[],
    ));
    flush_backend(&backend).await;
    let records = query_records(backend.clone(), list_query(QueryRecordFilter::default()))
        .unwrap()
        .0;
    assert_eq!(records.len(), 6);
    assert!(records.iter().any(|record| record.request_id == 10));

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_cleanup_is_not_skipped_when_record_queue_is_full() {
    AppClock::start();

    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(
        serde_yaml_ng::to_value(QueryRecorderConfig {
            path: temp.path().display().to_string(),
            queue_size: Some(1),
            batch_size: Some(1),
            flush_interval_ms: Some(10),
            memory_tail: Some(8),
            retention_days: Some(7),
            cleanup_interval_hours: Some(1),
            reader_concurrency: Some(2),
            max_steps: Some(512),
            context: BTreeMap::new(),
            include_marks: Vec::new(),
            exclude_marks: Vec::new(),
        })
        .unwrap(),
    ))
    .unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let coordinator = backend.database_coordinator.clone();
    let maintenance_holder = std::thread::spawn(move || {
        let _access = coordinator.write_access().unwrap();
        ready_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    ready_rx.recv().unwrap();

    let mut queue_was_full = false;
    for request_id in 1..=16 {
        let record = pending_record(
            i64::from(request_id),
            request_id,
            "queue-full.example.com.",
            RecordType::A,
            Ipv4Addr::new(192, 0, 2, 1),
            Some(Rcode::NoError),
            None,
            &[],
        );
        match backend
            .queue_tx
            .try_send(WriterCommand::Insert(Box::new(record)))
        {
            Ok(()) => std::thread::yield_now(),
            Err(std::sync::mpsc::TrySendError::Full(_)) => {
                queue_was_full = true;
                break;
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                panic!("query_recorder writer unexpectedly disconnected")
            }
        }
    }
    assert!(queue_was_full);

    let cleanup_backend = backend.clone();
    let mut cleanup_task = tokio::task::spawn_blocking(move || cleanup_backend.cleanup(i64::MAX));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut cleanup_task)
            .await
            .is_err()
    );

    release_tx.send(()).unwrap();
    maintenance_holder.join().unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), cleanup_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(result.deleted_records > 0);

    plugin.destroy().await.unwrap();
}

#[test]
fn test_query_recorder_query_parsers_accept_common_filters() {
    let query = super::api::parse_list_query(Some(
        "limit=50&since_ms=10&until_ms=20&qname=&qtype=aaaa&client_ip=192.0.2.1&rcode=nxdomain&status=has_response",
    ))
    .unwrap();
    assert_eq!(query.limit, 50);
    assert_eq!(query.since_ms, Some(10));
    assert_eq!(query.until_ms, Some(20));
    assert_eq!(query.filter.qname, None);
    assert_eq!(query.filter.qtype.as_deref(), Some("AAAA"));
    assert_eq!(query.filter.client_ip.as_deref(), Some("192.0.2.1"));
    assert_eq!(query.filter.rcode.as_deref(), Some("NXDOMAIN"));
    assert_eq!(query.filter.status, QueryRecordStatus::HasResponse);

    let stats =
        super::api::parse_plugins_stats_query(Some("kind=matcher&qname=example&status=all"))
            .unwrap();
    assert_eq!(stats.kind, PluginStatsKind::Matcher);
    assert_eq!(stats.filter.qname.as_deref(), Some("example"));
    assert_eq!(stats.filter.status, QueryRecordStatus::All);

    let err = super::api::parse_list_query(Some("status=bad")).unwrap_err();
    assert!(err.contains("status must be one of"));

    let with_matcher = super::api::parse_list_query(Some("matcher_tag=ads")).unwrap();
    assert_eq!(with_matcher.filter.matcher_tag.as_deref(), Some("ads"));
    let stats_with_matcher =
        super::api::parse_plugins_stats_query(Some("kind=matcher&matcher_tag=cn")).unwrap();
    assert_eq!(stats_with_matcher.filter.matcher_tag.as_deref(), Some("cn"));

    let top = super::api::parse_top_query(Some("limit=250&qname=example")).unwrap();
    assert_eq!(top.limit, 250);
    assert_eq!(top.filter.qname.as_deref(), Some("example"));

    let latency = super::api::parse_latency_query(Some("slow_limit=250")).unwrap();
    assert_eq!(latency.slow_limit, 250);
}

#[tokio::test]
async fn test_query_recorder_query_records_support_common_filters() {
    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    backend.enqueue(pending_record(
        1_000,
        1,
        "www.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 1),
        Some(Rcode::NoError),
        None,
        &[("ads", "matched")],
    ));
    backend.enqueue(pending_record(
        2_000,
        2,
        "ads.test.",
        RecordType::AAAA,
        Ipv4Addr::new(192, 0, 2, 2),
        Some(Rcode::NXDomain),
        None,
        &[("ads", "matched")],
    ));
    backend.enqueue(pending_record(
        3_000,
        3,
        "boom.example.net.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 3),
        None,
        Some("boom"),
        &[("ads", "not_matched")],
    ));
    backend.enqueue(pending_record(
        4_000,
        4,
        "empty.test.",
        RecordType::HTTPS,
        Ipv4Addr::new(192, 0, 2, 4),
        None,
        None,
        &[],
    ));
    flush_backend(&backend).await;

    assert_eq!(
        filtered_record_ids(
            backend.clone(),
            list_query(QueryRecordFilter {
                qname: Some("WWW.EXAMPLE".to_string()),
                ..QueryRecordFilter::default()
            }),
        ),
        vec![1]
    );
    assert_eq!(
        filtered_record_ids(
            backend.clone(),
            list_query(QueryRecordFilter {
                qtype: Some("AAAA".to_string()),
                ..QueryRecordFilter::default()
            }),
        ),
        vec![2]
    );
    assert_eq!(
        filtered_record_ids(
            backend.clone(),
            list_query(QueryRecordFilter {
                client_ip: Some("0.2.3".to_string()),
                ..QueryRecordFilter::default()
            }),
        ),
        vec![3]
    );
    let all_records = query_records(backend.clone(), list_query(QueryRecordFilter::default()))
        .unwrap()
        .0;
    let nxdomain_record = all_records
        .iter()
        .find(|record| record.request_id == 2)
        .expect("record 2 should exist");
    assert_eq!(
        nxdomain_record.rcode.as_deref(),
        Some("Non-Existent Domain")
    );
    assert_eq!(
        filtered_record_ids(
            backend.clone(),
            list_query(QueryRecordFilter {
                rcode: Some("Non-Existent Domain".to_string()),
                ..QueryRecordFilter::default()
            }),
        ),
        vec![2]
    );
    assert_eq!(
        filtered_record_ids(
            backend.clone(),
            list_query(QueryRecordFilter {
                status: QueryRecordStatus::Error,
                ..QueryRecordFilter::default()
            }),
        ),
        vec![3]
    );
    assert_eq!(
        filtered_record_ids(
            backend.clone(),
            list_query(QueryRecordFilter {
                status: QueryRecordStatus::HasResponse,
                ..QueryRecordFilter::default()
            }),
        ),
        vec![2, 1]
    );
    assert_eq!(
        filtered_record_ids(
            backend.clone(),
            list_query(QueryRecordFilter {
                status: QueryRecordStatus::NoResponse,
                ..QueryRecordFilter::default()
            }),
        ),
        vec![4]
    );
    assert_eq!(
        filtered_record_ids(
            backend.clone(),
            ListQuery {
                cursor: None,
                limit: 20,
                since_ms: Some(1_500),
                until_ms: Some(3_500),
                filter: QueryRecordFilter::default(),
            },
        ),
        vec![3, 2]
    );
    assert_eq!(
        filtered_record_ids(
            backend.clone(),
            list_query(QueryRecordFilter {
                matcher_tag: Some("ads".to_string()),
                ..QueryRecordFilter::default()
            }),
        ),
        vec![2, 1]
    );
    assert!(
        filtered_record_ids(
            backend.clone(),
            list_query(QueryRecordFilter {
                matcher_tag: Some("nope".to_string()),
                ..QueryRecordFilter::default()
            }),
        )
        .is_empty()
    );

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_matcher_stats_use_record_filters() {
    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    backend.enqueue(pending_record(
        1_000,
        1,
        "www.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 1),
        Some(Rcode::NoError),
        None,
        &[("ads", "matched"), ("cn", "not_matched")],
    ));
    backend.enqueue(pending_record(
        2_000,
        2,
        "ads.test.",
        RecordType::AAAA,
        Ipv4Addr::new(192, 0, 2, 2),
        Some(Rcode::NoError),
        None,
        &[("ads", "matched")],
    ));
    backend.enqueue(pending_record(
        3_000,
        3,
        "boom.example.net.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 3),
        None,
        Some("boom"),
        &[("ads", "not_matched")],
    ));
    flush_backend(&backend).await;

    let (query_total, stats) = load_plugin_stats(
        backend,
        PluginsStatsQuery {
            since_ms: None,
            until_ms: None,
            kind: PluginStatsKind::Matcher,
            filter: QueryRecordFilter {
                qname: Some("example".to_string()),
                ..QueryRecordFilter::default()
            },
        },
    )
    .unwrap();

    assert_eq!(query_total, 2);
    let ads = stats
        .iter()
        .find(|row| row.tag.as_deref() == Some("ads"))
        .unwrap();
    assert_eq!(ads.checked, 2);
    assert_eq!(ads.matched, 1);
    assert_eq!(ads.query_total, 2);
    assert_eq!(ads.query_share, 1.0);

    let cn = stats
        .iter()
        .find(|row| row.tag.as_deref() == Some("cn"))
        .unwrap();
    assert_eq!(cn.checked, 1);
    assert_eq!(cn.matched, 0);
    assert_eq!(cn.query_total, 1);
    assert_eq!(cn.query_share, 0.5);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_tracks_fixed_values_and_effective_match_results() {
    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    backend.enqueue(pending_record(
        1_000,
        1,
        "hit.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 1),
        Some(Rcode::NoError),
        None,
        &[("controlled", "always_true_matched")],
    ));
    backend.enqueue(pending_record(
        2_000,
        2,
        "miss.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 2),
        Some(Rcode::NoError),
        None,
        &[("controlled", "always_true_not_matched")],
    ));
    backend.enqueue(pending_record(
        3_000,
        3,
        "false-negated.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 3),
        Some(Rcode::NoError),
        None,
        &[("controlled", "always_false_matched")],
    ));
    backend.enqueue(pending_record(
        4_000,
        4,
        "false-positive.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 4),
        Some(Rcode::NoError),
        None,
        &[("controlled", "always_false_not_matched")],
    ));
    flush_backend(&backend).await;

    assert_eq!(
        filtered_record_ids(
            backend.clone(),
            list_query(QueryRecordFilter {
                matcher_tag: Some("controlled".to_string()),
                ..QueryRecordFilter::default()
            }),
        ),
        vec![3, 1]
    );

    let (_, stats) = load_plugin_stats(
        backend,
        PluginsStatsQuery {
            since_ms: None,
            until_ms: None,
            kind: PluginStatsKind::Matcher,
            filter: QueryRecordFilter::default(),
        },
    )
    .unwrap();
    let controlled = stats
        .iter()
        .find(|row| row.tag.as_deref() == Some("controlled"))
        .unwrap();
    assert_eq!(controlled.checked, 4);
    assert_eq!(controlled.matched, 2);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_query_recorder_plugin_stats_preserve_total_without_steps() {
    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    backend.enqueue(pending_record(
        1_000,
        1,
        "www.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 1),
        Some(Rcode::NoError),
        None,
        &[],
    ));
    backend.enqueue(pending_record(
        2_000,
        2,
        "ads.example.com.",
        RecordType::AAAA,
        Ipv4Addr::new(192, 0, 2, 2),
        Some(Rcode::NoError),
        None,
        &[],
    ));
    flush_backend(&backend).await;

    let (query_total, stats) = load_plugin_stats(
        backend,
        PluginsStatsQuery {
            since_ms: None,
            until_ms: None,
            kind: PluginStatsKind::Matcher,
            filter: QueryRecordFilter {
                qname: Some("example".to_string()),
                ..QueryRecordFilter::default()
            },
        },
    )
    .unwrap();

    assert_eq!(query_total, 2);
    assert!(stats.is_empty());

    plugin.destroy().await.unwrap();
}

async fn seed_demo_records(backend: &std::sync::Arc<super::backend::RecorderBackend>) {
    backend.enqueue(pending_record(
        1_000,
        1,
        "www.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 1),
        Some(Rcode::NoError),
        None,
        &[],
    ));
    backend.enqueue(pending_record(
        2_000,
        2,
        "ads.example.com.",
        RecordType::AAAA,
        Ipv4Addr::new(192, 0, 2, 1),
        Some(Rcode::NXDomain),
        None,
        &[],
    ));
    backend.enqueue(pending_record(
        3_000,
        3,
        "www.example.com.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 2),
        Some(Rcode::NoError),
        None,
        &[],
    ));
    backend.enqueue(pending_record(
        4_000,
        4,
        "boom.example.net.",
        RecordType::A,
        Ipv4Addr::new(192, 0, 2, 3),
        None,
        Some("boom"),
        &[],
    ));
    backend.enqueue(pending_record(
        5_000,
        5,
        "empty.test.",
        RecordType::HTTPS,
        Ipv4Addr::new(192, 0, 2, 4),
        None,
        None,
        &[],
    ));
    flush_backend(backend).await;
}

#[tokio::test]
async fn test_load_top_clients_ranks_by_count() {
    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();
    seed_demo_records(&backend).await;

    let response = load_top_clients(
        backend,
        TopQuery {
            since_ms: None,
            until_ms: None,
            filter: QueryRecordFilter::default(),
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(response.sample_size, 5);
    assert_eq!(response.rows[0].key, "192.0.2.1");
    assert_eq!(response.rows[0].count, 2);
    assert!((response.rows[0].share - 0.4).abs() < 1.0e-9);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_load_top_clients_allows_limit_above_200() {
    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(
        serde_yaml_ng::to_value(QueryRecorderConfig {
            path: temp.path().display().to_string(),
            queue_size: Some(512),
            batch_size: Some(64),
            flush_interval_ms: Some(10),
            memory_tail: Some(16),
            retention_days: Some(7),
            cleanup_interval_hours: Some(1),
            reader_concurrency: Some(2),
            max_steps: Some(512),
            context: BTreeMap::new(),
            include_marks: Vec::new(),
            exclude_marks: Vec::new(),
        })
        .unwrap(),
    ))
    .unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();

    for index in 0..250u16 {
        let octet = (index + 1) as u8;
        backend.enqueue(pending_record(
            1_000 + i64::from(index),
            index + 1,
            &format!("host-{index}.example."),
            RecordType::A,
            Ipv4Addr::new(10, 0, 0, octet),
            Some(Rcode::NoError),
            None,
            &[],
        ));
    }
    flush_backend(&backend).await;
    assert_eq!(backend.dropped_total.load(Ordering::Relaxed), 0);

    let response = load_top_clients(
        backend,
        TopQuery {
            since_ms: None,
            until_ms: None,
            filter: QueryRecordFilter::default(),
            limit: 250,
        },
    )
    .unwrap();
    assert_eq!(response.sample_size, 250);
    assert_eq!(response.rows.len(), 250);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_load_top_qnames_unwinds_questions() {
    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();
    seed_demo_records(&backend).await;

    let response = load_top_qnames(
        backend,
        TopQuery {
            since_ms: None,
            until_ms: None,
            filter: QueryRecordFilter::default(),
            limit: 10,
        },
    )
    .unwrap();
    let top = response
        .rows
        .iter()
        .find(|row| row.key == "www.example.com.")
        .expect("www.example.com. should be present");
    assert_eq!(top.count, 2);
    assert_eq!(response.sample_size, 5);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_qtype_and_rcode_distribution_counts() {
    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();
    seed_demo_records(&backend).await;

    let qtype = load_qtype_distribution(
        backend.clone(),
        DistributionQuery {
            since_ms: None,
            until_ms: None,
            filter: QueryRecordFilter::default(),
        },
    )
    .unwrap();
    let a_count = qtype.rows.iter().find(|row| row.key == "A").unwrap().count;
    let aaaa_count = qtype
        .rows
        .iter()
        .find(|row| row.key == "AAAA")
        .unwrap()
        .count;
    let https_count = qtype
        .rows
        .iter()
        .find(|row| row.key == "HTTPS")
        .unwrap()
        .count;
    assert_eq!(a_count, 3);
    assert_eq!(aaaa_count, 1);
    assert_eq!(https_count, 1);

    let rcode = load_rcode_distribution(
        backend,
        DistributionQuery {
            since_ms: None,
            until_ms: None,
            filter: QueryRecordFilter::default(),
        },
    )
    .unwrap();
    let error_bucket = rcode
        .rows
        .iter()
        .find(|row| row.key == "_ERROR")
        .expect("_ERROR bucket expected for failed records");
    assert_eq!(error_bucket.count, 1);
    let no_response_bucket = rcode
        .rows
        .iter()
        .find(|row| row.key == "_NO_RESPONSE")
        .expect("_NO_RESPONSE bucket expected for missing response");
    assert_eq!(no_response_bucket.count, 1);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_latency_summary_returns_percentiles_and_histogram() {
    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();
    seed_demo_records(&backend).await;

    let summary = load_latency_summary(
        backend,
        LatencyQuery {
            since_ms: None,
            until_ms: None,
            filter: QueryRecordFilter::default(),
            slow_limit: 5,
        },
    )
    .unwrap();
    assert_eq!(summary.sample_size, 5);
    assert!(summary.histogram.iter().any(|bucket| bucket.count > 0));
    assert!(summary.histogram.last().unwrap().lt_ms.is_none());
    let histogram_total: u64 = summary.histogram.iter().map(|bucket| bucket.count).sum();
    assert_eq!(histogram_total, summary.sample_size);

    plugin.destroy().await.unwrap();
}

#[tokio::test]
async fn test_timeseries_buckets_records_by_minute() {
    let temp = NamedTempFile::new().unwrap();
    let config = resolve_config(Some(recorder_config(&temp.path().display().to_string()))).unwrap();
    let mut plugin = QueryRecorder::new("rec".to_string(), config);
    plugin.init_for_test().await.unwrap();
    let backend = plugin.backend.as_ref().unwrap().clone();
    let minute_ms: i64 = 60_000;
    backend.enqueue(pending_record(
        100,
        10,
        "a.example.",
        RecordType::A,
        Ipv4Addr::new(10, 0, 0, 1),
        Some(Rcode::NoError),
        None,
        &[],
    ));
    backend.enqueue(pending_record(
        200,
        11,
        "b.example.",
        RecordType::A,
        Ipv4Addr::new(10, 0, 0, 1),
        None,
        Some("boom"),
        &[],
    ));
    backend.enqueue(pending_record(
        minute_ms + 500,
        12,
        "c.example.",
        RecordType::A,
        Ipv4Addr::new(10, 0, 0, 2),
        Some(Rcode::NoError),
        None,
        &[],
    ));
    flush_backend(&backend).await;

    let response = load_timeseries(
        backend,
        TimeseriesQuery {
            since_ms: None,
            until_ms: None,
            filter: QueryRecordFilter::default(),
            bucket: TimeseriesBucket::Minute,
            max_buckets: 60,
        },
    )
    .unwrap();
    assert_eq!(response.bucket_ms, minute_ms);
    assert_eq!(response.sample_size, 3);
    assert_eq!(response.points.len(), 2);
    let first = &response.points[0];
    assert_eq!(first.bucket_ms, 0);
    assert_eq!(first.total, 2);
    assert_eq!(first.error_count, 1);
    let second = &response.points[1];
    assert_eq!(second.bucket_ms, minute_ms);
    assert_eq!(second.total, 1);

    plugin.destroy().await.unwrap();
}

#[test]
fn test_factory_rejects_quick_setup() {
    let factory = QueryRecorderFactory;
    let err = match factory.quick_setup("rec", None) {
        Ok(_) => panic!("quick setup should be rejected"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("quick setup"));
}

#[test]
fn test_resolve_config_rejects_zero_limits() {
    let config = serde_yaml_ng::to_value(QueryRecorderConfig {
        path: "test.db".to_string(),
        queue_size: Some(0),
        batch_size: Some(1),
        flush_interval_ms: Some(1),
        memory_tail: Some(1),
        retention_days: Some(1),
        cleanup_interval_hours: Some(1),
        reader_concurrency: Some(2),
        max_steps: Some(512),
        context: BTreeMap::new(),
        include_marks: Vec::new(),
        exclude_marks: Vec::new(),
    })
    .unwrap();
    assert!(resolve_config(Some(config)).is_err());
}
