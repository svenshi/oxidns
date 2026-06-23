// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;

use tempfile::NamedTempFile;

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
use super::{QueryRecorder, QueryRecorderFactory, resolve_config};
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
    })
    .unwrap()
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
    })
    .unwrap();
    assert!(resolve_config(Some(config)).is_err());
}
