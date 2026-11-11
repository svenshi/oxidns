// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::IpAddr;
use std::sync::Arc;

use ahash::AHashSet;
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use http::{Request, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};

use super::key::{CacheKey, EcsScopeDigest, normalize_domain_key};
use super::persistence::{dump_cache_to_bytes, load_cache_from_bytes};
use super::{Cache, CacheItem, CacheMap};
use crate::api::query::{optional_text, parse_usize_param, visit_query_params};
use crate::api::{ApiHandler, json_error, json_ok, simple_response};
use crate::infra::clock::AppClock;
use crate::infra::error::Result;
use crate::plugin::executor::rdata_json::{RDataPayloadMode, rdata_payload};
use crate::proto::{DNSClass, RData, Record, RecordType};
use crate::register_plugin_api;

pub(super) fn register(
    tag: &str,
    cache_map: CacheMap,
    ecs_in_key: bool,
    cache_size: usize,
) -> Result<()> {
    register_plugin_api!(
        tag,
        |plugin_api|
        GET "/entries" => CacheEntriesListHandler {
            cache_map: cache_map.clone(),
        },
        POST "/entries/delete" => CacheEntriesDeleteHandler {
            cache_map: cache_map.clone(),
        },
        DELETE_PREFIX "/entries/" => CacheEntryDeleteHandler {
            cache_map: cache_map.clone(),
            path_prefix: plugin_api.path("/entries/")?,
        },
        GET "/flush" => CacheFlushHandler {
            cache_map: cache_map.clone(),
        },
        GET "/dump" => CacheDumpHandler {
            cache_map: cache_map.clone(),
            tag: tag.to_string(),
        },
        POST "/load_dump" => CacheLoadDumpHandler {
            cache_map,
            ecs_in_key,
            cache_size,
        },
    )?;
    Ok(())
}

#[derive(Debug)]
struct CacheFlushHandler {
    cache_map: CacheMap,
}

#[derive(Debug, Serialize)]
struct CacheFlushResponse {
    ok: bool,
    cleared_entries: usize,
}

#[async_trait]
impl ApiHandler for CacheFlushHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        let cleared_entries = self.cache_map.len();
        self.cache_map.clear();
        info!("cache flushed, cleared entries {}", cleared_entries);
        json_ok(
            StatusCode::OK,
            &CacheFlushResponse {
                ok: true,
                cleared_entries,
            },
        )
    }
}

#[derive(Debug)]
struct CacheDumpHandler {
    cache_map: CacheMap,
    tag: String,
}

#[async_trait]
impl ApiHandler for CacheDumpHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match dump_cache_to_bytes(&self.cache_map) {
            Ok(bytes) => {
                let mut response = simple_response(StatusCode::OK, Bytes::from(bytes));
                response.headers_mut().insert(
                    http::header::CONTENT_TYPE,
                    http::HeaderValue::from_static("application/octet-stream"),
                );
                if let Ok(value) = http::HeaderValue::from_str(&format!(
                    "attachment; filename=\"{}.dump\"",
                    self.tag
                )) {
                    response
                        .headers_mut()
                        .insert(http::header::CONTENT_DISPOSITION, value);
                }
                response
            }
            Err(err) => {
                warn!("Failed to dump cache via API: {}", err);
                simple_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Bytes::from("failed to dump cache"),
                )
            }
        }
    }
}

#[derive(Debug)]
struct CacheLoadDumpHandler {
    cache_map: CacheMap,
    ecs_in_key: bool,
    cache_size: usize,
}

#[derive(Debug, Serialize)]
struct CacheLoadDumpResponse {
    ok: bool,
    loaded_entries: usize,
}

#[async_trait]
impl ApiHandler for CacheLoadDumpHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        match load_cache_from_bytes(&self.cache_map, request.body(), self.ecs_in_key, true) {
            Ok(loaded_entries) => {
                let stats = Cache::prune_cache_after_load(
                    &self.cache_map,
                    self.cache_size,
                    AppClock::elapsed_millis(),
                );
                if stats.total_removed() > 0 {
                    info!(
                        expired_removed = stats.expired_removed,
                        evicted = stats.evicted,
                        before = stats.before_len,
                        after = stats.after_len,
                        "cache dump load pruned entries"
                    );
                }
                json_ok(
                    StatusCode::OK,
                    &CacheLoadDumpResponse {
                        ok: true,
                        loaded_entries,
                    },
                )
            }
            Err(err) => {
                warn!("Failed to load cache dump via API: {}", err);
                json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_cache_dump",
                    "failed to load cache dump",
                )
            }
        }
    }
}

#[derive(Debug)]
struct CacheEntriesListHandler {
    cache_map: CacheMap,
}

#[derive(Debug)]
struct CacheEntryDeleteHandler {
    cache_map: CacheMap,
    path_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntryId {
    domain: String,
    record_type: u16,
    dns_class: u16,
    do_bit: bool,
    cd_bit: bool,
    ecs_scope: Option<CacheEntryEcsId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntryEcsId {
    family: u16,
    source_prefix: u8,
    scope_prefix: u8,
    network_len: u8,
    network: [u8; 16],
}

#[derive(Debug, Clone, Serialize)]
struct CacheEntriesResponse {
    ok: bool,
    entries: Vec<CacheEntryRow>,
    next_cursor: Option<String>,
    total_entries: usize,
}

#[derive(Debug, Clone, Serialize)]
struct CacheEntryDeleteResponse {
    ok: bool,
    deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
struct CacheEntryRow {
    id: String,
    domain: String,
    record_type: String,
    dns_class: String,
    rcode: String,
    answer_count: u16,
    authority_count: u16,
    additional_count: u16,
    ttl: u32,
    remaining_ttl: u32,
    fresh: bool,
    stale: bool,
    cache_time_ms: u64,
    expire_at_ms: u64,
    last_access_ms: u64,
    cache_time_unix_ms: u64,
    expire_at_unix_ms: u64,
    last_access_unix_ms: u64,
    do_bit: bool,
    cd_bit: bool,
    answers_json: Vec<CacheRecordJson>,
    authorities_json: Vec<CacheRecordJson>,
    additionals_json: Vec<CacheRecordJson>,
    signature_json: Vec<CacheRecordJson>,
    ecs_scope: Option<CacheEntryEcsRow>,
}

#[derive(Debug, Clone, Serialize)]
struct CacheRecordJson {
    name: String,
    class: String,
    ttl: u32,
    rr_type: String,
    payload_kind: String,
    payload_text: String,
    payload: Value,
}

#[derive(Debug, Clone, Serialize)]
struct CacheEntryEcsRow {
    family: u16,
    source_prefix: u8,
    scope_prefix: u8,
    network_hex: String,
}

#[async_trait]
impl ApiHandler for CacheEntriesListHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let query = match parse_cache_entries_query(request.uri().query()) {
            Ok(query) => query,
            Err(err) => return json_error(StatusCode::BAD_REQUEST, "invalid_query", err),
        };
        let now = AppClock::elapsed_millis();
        let now_unix_ms = AppClock::now_timestamp();
        let mut entries = self
            .cache_map
            .iter_entries_cloned()
            .into_iter()
            .filter(|(_, entry)| entry.expire_at_ms > now)
            .filter(|(key, entry)| {
                cache_entry_matches_query(key, &query)
                    && answer_ip_matches(&entry.value, query.answer_ip)
            })
            .collect::<Vec<_>>();
        entries.sort_by(|(left_key, left_entry), (right_key, right_entry)| {
            right_entry
                .last_access_ms
                .cmp(&left_entry.last_access_ms)
                .then_with(|| left_key.domain.cmp(&right_key.domain))
                .then_with(|| {
                    u16::from(left_key.record_type).cmp(&u16::from(right_key.record_type))
                })
                .then_with(|| u16::from(left_key.dns_class).cmp(&u16::from(right_key.dns_class)))
        });

        let total_entries = entries.len();
        let start = query.cursor.min(total_entries);
        let end = start.saturating_add(query.limit).min(total_entries);
        let next_cursor = if end < total_entries {
            Some(end.to_string())
        } else {
            None
        };
        let rows = entries[start..end]
            .iter()
            .filter_map(|(key, entry)| cache_entry_row(key, entry, now, now_unix_ms).ok())
            .collect::<Vec<_>>();

        json_ok(
            StatusCode::OK,
            &CacheEntriesResponse {
                ok: true,
                entries: rows,
                next_cursor,
                total_entries,
            },
        )
    }
}

#[async_trait]
impl ApiHandler for CacheEntryDeleteHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let Some(raw_id) = request.uri().path().strip_prefix(self.path_prefix.as_str()) else {
            return simple_response(StatusCode::NOT_FOUND, Bytes::from("404 Not Found"));
        };
        if raw_id.is_empty() || raw_id.contains('/') {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_cache_entry_id",
                "invalid cache entry id",
            );
        }
        let key = match decode_cache_entry_id(raw_id) {
            Ok(key) => key,
            Err(err) => {
                return json_error(StatusCode::BAD_REQUEST, "invalid_cache_entry_id", err);
            }
        };
        if !self.cache_map.remove(&key) {
            return json_error(
                StatusCode::NOT_FOUND,
                "cache_entry_not_found",
                "cache entry does not exist",
            );
        }
        json_ok(
            StatusCode::OK,
            &CacheEntryDeleteResponse {
                ok: true,
                deleted: true,
            },
        )
    }
}

#[derive(Debug, Clone)]
struct CacheEntriesQuery {
    limit: usize,
    cursor: usize,
    qname: Option<String>,
    answer_ip: Option<IpAddr>,
}

fn parse_cache_entries_query(
    query: Option<&str>,
) -> std::result::Result<CacheEntriesQuery, String> {
    let mut limit = 100usize;
    let mut cursor = 0usize;
    let mut qname = None;
    let mut answer_ip = None;
    visit_query_params(query, |key, value| {
        match key {
            "limit" => {
                limit =
                    parse_usize_param(value, |_| "limit must be a positive integer".to_string())?
                        .clamp(1, 500);
            }
            "cursor" => {
                cursor = parse_usize_param(value, |_| {
                    "cursor must be a non-negative integer".to_string()
                })?;
            }
            "answer_ip" => {
                answer_ip = parse_answer_ip(value)?;
            }
            "qname" => {
                qname = optional_text(value).map(|value| normalize_domain_key(value.as_str()));
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(CacheEntriesQuery {
        limit,
        cursor,
        qname,
        answer_ip,
    })
}

fn cache_entry_matches_query(key: &CacheKey, query: &CacheEntriesQuery) -> bool {
    query
        .qname
        .as_deref()
        .is_none_or(|qname| key.domain.to_ascii_lowercase().contains(qname))
}

fn parse_answer_ip(raw: &str) -> std::result::Result<Option<IpAddr>, String> {
    optional_text(raw)
        .map(|value| {
            value
                .parse()
                .map_err(|_| "answer_ip must be a complete IPv4 or IPv6 address".to_string())
        })
        .transpose()
}

fn answer_ip_matches(item: &CacheItem, ip: Option<IpAddr>) -> bool {
    ip.is_none_or(|ip| {
        item.resp
            .answers()
            .iter()
            .any(|record| match record.data() {
                RData::A(addr) => ip == IpAddr::V4(addr.0),
                RData::AAAA(addr) => ip == IpAddr::V6(addr.0),
                _ => false,
            })
    })
}

#[derive(Debug)]
struct CacheEntriesDeleteHandler {
    cache_map: CacheMap,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteRequest {
    ids: Option<Vec<String>>,
    filter: Option<DeleteFilter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteFilter {
    qname: Option<String>,
    answer_ip: Option<String>,
}

enum DeleteSelection {
    Ids(AHashSet<CacheKey>),
    Filter(CacheEntriesQuery),
}

fn parse_delete_request(body: &[u8]) -> std::result::Result<DeleteSelection, String> {
    let request: DeleteRequest =
        serde_json::from_slice(body).map_err(|_| "invalid delete request".to_string())?;
    match (request.ids, request.filter) {
        (Some(ids), None) if !ids.is_empty() => {
            let keys = ids
                .iter()
                .map(|id| decode_cache_entry_id(id))
                .collect::<std::result::Result<AHashSet<_>, _>>()?;
            Ok(DeleteSelection::Ids(keys))
        }
        (None, Some(filter)) => {
            let qname = filter
                .qname
                .as_deref()
                .and_then(optional_text)
                .map(|value| normalize_domain_key(&value))
                .filter(|value| !value.is_empty());
            let answer_ip = parse_answer_ip(filter.answer_ip.as_deref().unwrap_or_default())?;
            if qname.is_none() && answer_ip.is_none() {
                return Err("a non-empty filter is required".into());
            }
            Ok(DeleteSelection::Filter(CacheEntriesQuery {
                limit: 100,
                cursor: 0,
                qname,
                answer_ip,
            }))
        }
        _ => Err("provide either non-empty ids or a filter".into()),
    }
}

fn delete_filtered_entry(
    cache: &CacheMap,
    key: &CacheKey,
    snapshot: &crate::infra::cache::ttl::TtlCacheEntry<Arc<CacheItem>>,
) -> bool {
    cache.remove_if(key, |current| {
        Arc::ptr_eq(&current.value, &snapshot.value)
            && current.cache_time_ms == snapshot.cache_time_ms
            && current.expire_at_ms == snapshot.expire_at_ms
    })
}

#[async_trait]
impl ApiHandler for CacheEntriesDeleteHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let selection = match parse_delete_request(request.body()) {
            Ok(selection) => selection,
            Err(err) => return json_error(StatusCode::BAD_REQUEST, "invalid_delete_request", err),
        };
        let deleted_entries = match selection {
            DeleteSelection::Ids(keys) => {
                keys.iter().filter(|key| self.cache_map.remove(key)).count()
            }
            DeleteSelection::Filter(query) => {
                let now = AppClock::elapsed_millis();
                self.cache_map
                    .iter_entries_cloned()
                    .into_iter()
                    .filter(|(key, entry)| {
                        entry.expire_at_ms > now
                            && cache_entry_matches_query(key, &query)
                            && answer_ip_matches(&entry.value, query.answer_ip)
                    })
                    .filter(|(key, entry)| delete_filtered_entry(&self.cache_map, key, entry))
                    .count()
            }
        };
        json_ok(
            StatusCode::OK,
            &serde_json::json!({"ok": true, "deleted_entries": deleted_entries}),
        )
    }
}

fn cache_entry_row(
    key: &CacheKey,
    entry: &crate::infra::cache::ttl::TtlCacheEntry<Arc<CacheItem>>,
    now: u64,
    now_unix_ms: u64,
) -> std::result::Result<CacheEntryRow, String> {
    let item = entry.value.as_ref();
    let fresh = now < item.fresh_until_ms;
    let stale = !fresh && now < entry.expire_at_ms;
    Ok(CacheEntryRow {
        id: encode_cache_entry_id(key)?,
        domain: key.domain.clone(),
        record_type: key.record_type.to_string(),
        dns_class: key.dns_class.to_string(),
        rcode: item.resp.rcode().to_string(),
        answer_count: item.resp.answer_count(),
        authority_count: item.resp.authority_count(),
        additional_count: item.resp.additionals().len() as u16,
        ttl: item.ttl,
        remaining_ttl: entry.expire_at_ms.saturating_sub(now).saturating_div(1000) as u32,
        fresh,
        stale,
        cache_time_ms: entry.cache_time_ms,
        expire_at_ms: entry.expire_at_ms,
        last_access_ms: entry.last_access_ms,
        cache_time_unix_ms: elapsed_to_unix_ms(entry.cache_time_ms, now, now_unix_ms),
        expire_at_unix_ms: elapsed_to_unix_ms(entry.expire_at_ms, now, now_unix_ms),
        last_access_unix_ms: elapsed_to_unix_ms(entry.last_access_ms, now, now_unix_ms),
        do_bit: key.do_bit,
        cd_bit: key.cd_bit,
        answers_json: item.resp.answers().iter().map(cache_record_json).collect(),
        authorities_json: item
            .resp
            .authorities()
            .iter()
            .map(cache_record_json)
            .collect(),
        additionals_json: item
            .resp
            .additionals()
            .iter()
            .map(cache_record_json)
            .collect(),
        signature_json: item
            .resp
            .signature()
            .iter()
            .map(cache_record_json)
            .collect(),
        ecs_scope: key.ecs_scope.as_ref().map(|ecs| CacheEntryEcsRow {
            family: ecs.family,
            source_prefix: ecs.source_prefix,
            scope_prefix: ecs.scope_prefix,
            network_hex: ecs.network[..usize::from(ecs.network_len)]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        }),
    })
}

fn elapsed_to_unix_ms(elapsed_ms: u64, now_ms: u64, now_unix_ms: u64) -> u64 {
    if elapsed_ms <= now_ms {
        now_unix_ms.saturating_sub(now_ms - elapsed_ms)
    } else {
        now_unix_ms.saturating_add(elapsed_ms - now_ms)
    }
}

fn cache_record_json(record: &Record) -> CacheRecordJson {
    let (payload_kind, payload_text, payload) =
        rdata_payload(record.data(), RDataPayloadMode::Cache);
    CacheRecordJson {
        name: record.name().to_fqdn(),
        class: dns_class_name(record.class()),
        ttl: record.ttl(),
        rr_type: record_type_name(record.rr_type()),
        payload_kind,
        payload_text,
        payload,
    }
}

fn dns_class_name(class: DNSClass) -> String {
    match class {
        DNSClass::Unknown(value) => format!("CLASS{value}"),
        DNSClass::OPT(value) => format!("OPT({value})"),
        _ => class.to_string(),
    }
}

fn record_type_name(record_type: RecordType) -> String {
    match record_type {
        RecordType::Unknown(value) => format!("TYPE{value}"),
        _ => record_type.to_string(),
    }
}

fn encode_cache_entry_id(key: &CacheKey) -> std::result::Result<String, String> {
    let id = CacheEntryId {
        domain: key.domain.clone(),
        record_type: u16::from(key.record_type),
        dns_class: u16::from(key.dns_class),
        do_bit: key.do_bit,
        cd_bit: key.cd_bit,
        ecs_scope: key.ecs_scope.as_ref().map(|ecs| CacheEntryEcsId {
            family: ecs.family,
            source_prefix: ecs.source_prefix,
            scope_prefix: ecs.scope_prefix,
            network_len: ecs.network_len,
            network: ecs.network,
        }),
    };
    serde_json::to_vec(&id)
        .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
        .map_err(|err| format!("failed to encode cache entry id: {err}"))
}

fn decode_cache_entry_id(raw: &str) -> std::result::Result<CacheKey, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| "cache entry id is not valid base64url".to_string())?;
    let id: CacheEntryId = serde_json::from_slice(&bytes)
        .map_err(|_| "cache entry id is not valid json".to_string())?;
    if id.domain.trim().is_empty() {
        return Err("cache entry id domain is empty".to_string());
    }
    Ok(CacheKey {
        domain: id.domain,
        record_type: id.record_type.into(),
        dns_class: id.dns_class.into(),
        do_bit: id.do_bit,
        cd_bit: id.cd_bit,
        ecs_scope: id.ecs_scope.map(|ecs| EcsScopeDigest {
            family: ecs.family,
            source_prefix: ecs.source_prefix,
            scope_prefix: ecs.scope_prefix,
            network_len: ecs.network_len,
            network: ecs.network,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cache_key(domain: &str) -> CacheKey {
        CacheKey {
            domain: domain.to_string(),
            record_type: RecordType::A,
            dns_class: DNSClass::IN,
            do_bit: false,
            cd_bit: false,
            ecs_scope: None,
        }
    }

    #[test]
    fn parse_cache_entries_query_accepts_qname_filter() {
        let query = parse_cache_entries_query(Some("limit=20&cursor=5&qname=%20EXAMPLE.COM.%20"))
            .expect("query should parse");

        assert_eq!(query.limit, 20);
        assert_eq!(query.cursor, 5);
        assert_eq!(query.qname.as_deref(), Some("example.com"));
    }

    #[test]
    fn parse_cache_entries_query_ignores_empty_qname_filter() {
        let query = parse_cache_entries_query(Some("qname=%20%20")).expect("query should parse");

        assert_eq!(query.qname, None);
    }

    #[test]
    fn cache_entry_matches_query_filters_qname_case_insensitively() {
        let query = CacheEntriesQuery {
            limit: 100,
            cursor: 0,
            qname: Some("example.com".to_string()),
            answer_ip: None,
        };

        assert!(cache_entry_matches_query(
            &test_cache_key("www.Example.COM"),
            &query
        ));
        assert!(!cache_entry_matches_query(
            &test_cache_key("www.example.net"),
            &query
        ));
    }
    fn seed(cache: &CacheMap, key: CacheKey, fresh: bool, expired: bool) {
        use crate::proto::{Message, Name};
        let now = AppClock::elapsed_millis();
        let mut response = Message::new();
        response.add_answer(Record::from_rdata(
            Name::from_ascii("alias.example.").unwrap(),
            60,
            RData::CNAME(crate::proto::rdata::CNAME(
                Name::from_ascii("target.example.").unwrap(),
            )),
        ));
        for ip in ["192.0.2.1", "192.0.2.2"] {
            response.add_answer(Record::from_rdata(
                Name::from_ascii("target.example.").unwrap(),
                60,
                RData::A(crate::proto::rdata::A(ip.parse().unwrap())),
            ));
        }
        response.add_answer(Record::from_rdata(
            Name::from_ascii("target.example.").unwrap(),
            60,
            RData::AAAA(crate::proto::rdata::AAAA("2001:db8::1".parse().unwrap())),
        ));
        response.add_additional(Record::from_rdata(
            Name::from_ascii("ns.example.").unwrap(),
            60,
            RData::A(crate::proto::rdata::A("198.51.100.1".parse().unwrap())),
        ));
        cache.insert_or_update(
            key,
            Arc::new(CacheItem::new(
                response,
                60,
                if fresh { now + 60_000 } else { 0 },
            )),
            now,
            if expired { 0 } else { now + 120_000 },
        );
    }

    async fn json(response: crate::api::ApiResponse) -> Value {
        use http_body_util::BodyExt;
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }

    #[tokio::test]
    async fn address_filters_apply_before_pagination_and_keep_stale() {
        let cache = CacheMap::with_capacity(4);
        seed(&cache, test_cache_key("alias.example"), true, false);
        seed(&cache, test_cache_key("stale.example"), false, false);
        seed(&cache, test_cache_key("expired.example"), true, true);
        let handler = CacheEntriesListHandler { cache_map: cache };
        for ip in ["192.0.2.1", "192.0.2.2", "2001:0db8:0:0:0:0:0:1"] {
            let response = handler
                .handle(
                    Request::builder()
                        .uri(format!("/?limit=1&answer_ip={ip}"))
                        .body(Bytes::new())
                        .unwrap(),
                )
                .await;
            let value = json(response).await;
            assert_eq!(value["total_entries"], 2);
            assert_eq!(value["entries"].as_array().unwrap().len(), 1);
            assert_eq!(value["next_cursor"], "1");
        }
        let value = json(
            handler
                .handle(
                    Request::builder()
                        .uri("/?answer_ip=192.0.2.1&qname=ALIAS.EXAMPLE.")
                        .body(Bytes::new())
                        .unwrap(),
                )
                .await,
        )
        .await;
        assert_eq!(value["total_entries"], 1);
        assert_eq!(value["entries"][0]["domain"], "alias.example");
        let value = json(
            handler
                .handle(
                    Request::builder()
                        .uri("/?answer_ip=198.51.100.1")
                        .body(Bytes::new())
                        .unwrap(),
                )
                .await,
        )
        .await;
        assert_eq!(value["total_entries"], 0);
        assert!(parse_cache_entries_query(Some("answer_ip=192.0.2")).is_err());
    }

    #[tokio::test]
    async fn bulk_delete_validates_before_mutation_and_preserves_key_variants() {
        let cache = CacheMap::with_capacity(4);
        let key = test_cache_key("alias.example");
        let mut variant = key.clone();
        variant.do_bit = true;
        variant.cd_bit = true;
        variant.ecs_scope = Some(EcsScopeDigest {
            family: 1,
            source_prefix: 24,
            scope_prefix: 0,
            network_len: 3,
            network: [0; 16],
        });
        seed(&cache, key.clone(), true, false);
        seed(&cache, variant.clone(), true, false);
        let handler = CacheEntriesDeleteHandler {
            cache_map: cache.clone(),
        };
        let id = encode_cache_entry_id(&key).unwrap();
        for body in [
            serde_json::json!({"ids":[id,"!"]}),
            serde_json::json!({"ids":[]}),
            serde_json::json!({"filter":{}}),
            serde_json::json!({"filter":{"answer_ip":"bad"}}),
            serde_json::json!({"ids":[id],"filter":{"qname":"example"}}),
        ] {
            let response = handler
                .handle(Request::new(Bytes::from(body.to_string())))
                .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(cache.len(), 2);
        }
        let missing = encode_cache_entry_id(&test_cache_key("missing.example")).unwrap();
        let value = json(
            handler
                .handle(Request::new(Bytes::from(
                    serde_json::json!({"ids":[id,id,missing]}).to_string(),
                )))
                .await,
        )
        .await;
        assert_eq!(value["deleted_entries"], 1);
        assert_eq!(cache.len(), 1);
        let value = json(
            handler
                .handle(Request::new(Bytes::from_static(
                    br#"{"filter":{"answer_ip":"192.0.2.1","qname":"example"}}"#,
                )))
                .await,
        )
        .await;
        assert_eq!(value["deleted_entries"], 1);
        assert!(cache.is_empty());
    }

    #[test]
    fn filtered_delete_does_not_remove_replacement() {
        let cache = CacheMap::with_capacity(1);
        let key = test_cache_key("alias.example");
        seed(&cache, key.clone(), true, false);
        let (_, snapshot) = cache.iter_entries_cloned().pop().unwrap();
        seed(&cache, key.clone(), true, false);
        assert!(!delete_filtered_entry(&cache, &key, &snapshot));
        assert_eq!(cache.len(), 1);
    }
}
