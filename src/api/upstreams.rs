// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Temporary upstream connectivity test API.
//!
//! These endpoints build throwaway upstream clients from request payloads, send
//! one DNS query, and return the observed result. They do not register plugins,
//! persist configuration, or participate in the DNS request path.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::api::{ApiHandler, ApiRegister, json_error, json_ok};
use crate::infra::error::Result;
use crate::infra::network::upstream::{
    ConnectionInfo, ConnectionType, UpstreamBuilder, UpstreamConfig,
};
use crate::proto::{DNSClass, Message, Name, Question, Record, RecordType};

const DEFAULT_QNAME: &str = "example.com.";
const DEFAULT_QTYPE: &str = "A";
const DEFAULT_TIMEOUT_MS: u64 = 5_000;
const MIN_TIMEOUT_MS: u64 = 1_000;
const MAX_TIMEOUT_MS: u64 = 15_000;
const MAX_GROUP_UPSTREAMS: usize = 16;

#[derive(Debug, Deserialize)]
struct UpstreamTestRequest {
    upstream: UpstreamTestInput,
    qname: Option<String>,
    qtype: Option<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct UpstreamGroupTestRequest {
    upstreams: Vec<UpstreamGroupTestInput>,
    qname: Option<String>,
    qtype: Option<String>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct UpstreamGroupTestInput {
    id: Option<String>,
    name: Option<String>,
    #[serde(flatten)]
    upstream: UpstreamTestInput,
}

#[derive(Debug, Deserialize)]
struct UpstreamTestInput {
    addr: String,
    tag: Option<String>,
    bootstrap: Option<String>,
    dial_addr: Option<std::net::IpAddr>,
    insecure_skip_verify: Option<bool>,
    enable_http3: Option<bool>,
}

#[derive(Debug, Serialize)]
struct UpstreamTestResponse {
    ok: bool,
    result: UpstreamTestResult,
}

#[derive(Debug, Serialize)]
struct UpstreamGroupTestResponse {
    ok: bool,
    results: Vec<UpstreamTestResult>,
    success_count: usize,
    failure_count: usize,
    fastest_upstream_id: Option<String>,
    fastest_latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpstreamTestResult {
    id: Option<String>,
    name: Option<String>,
    success: bool,
    latency_ms: Option<u64>,
    protocol: Option<String>,
    rcode: Option<String>,
    answers: Vec<UpstreamAnswerSummary>,
    error_code: Option<&'static str>,
    error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct UpstreamAnswerSummary {
    name: String,
    rr_type: String,
    class: String,
    ttl: u32,
    data: String,
}

#[derive(Debug)]
struct UpstreamTestHandler;

#[derive(Debug)]
struct UpstreamGroupTestHandler;

#[derive(Debug)]
struct TestQuery {
    qname: Name,
    qtype: RecordType,
    timeout_ms: u64,
}

#[async_trait]
impl ApiHandler for UpstreamTestHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let test_request = match serde_json::from_slice::<UpstreamTestRequest>(request.body()) {
            Ok(request) => request,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_upstream_test_request",
                    format!("request body must be JSON: {err}"),
                );
            }
        };
        let query = match parse_test_query(
            test_request.qname.as_deref(),
            test_request.qtype.as_deref(),
            test_request.timeout_ms,
        ) {
            Ok(query) => query,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_upstream_test_request",
                    err,
                );
            }
        };

        let result = run_upstream_test(None, None, test_request.upstream, query).await;
        json_ok(StatusCode::OK, &UpstreamTestResponse { ok: true, result })
    }
}

#[async_trait]
impl ApiHandler for UpstreamGroupTestHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let test_request = match serde_json::from_slice::<UpstreamGroupTestRequest>(request.body())
        {
            Ok(request) => request,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_upstream_test_request",
                    format!("request body must be JSON: {err}"),
                );
            }
        };
        if test_request.upstreams.len() > MAX_GROUP_UPSTREAMS {
            return json_error(
                StatusCode::BAD_REQUEST,
                "invalid_upstream_test_request",
                format!("upstreams length must be <= {MAX_GROUP_UPSTREAMS}"),
            );
        }
        let query = match parse_test_query(
            test_request.qname.as_deref(),
            test_request.qtype.as_deref(),
            test_request.timeout_ms,
        ) {
            Ok(query) => query,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_upstream_test_request",
                    err,
                );
            }
        };

        let mut join_set = JoinSet::new();
        for upstream in test_request.upstreams {
            let query = query.clone();
            join_set.spawn(async move {
                run_upstream_test(upstream.id, upstream.name, upstream.upstream, query).await
            });
        }

        let mut results = Vec::new();
        while let Some(joined) = join_set.join_next().await {
            match joined {
                Ok(result) => results.push(result),
                Err(err) => results.push(failed_result(
                    None,
                    None,
                    None,
                    "upstream_test_join_failed",
                    err.to_string(),
                )),
            }
        }
        results.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.name.cmp(&b.name)));

        let success_count = results.iter().filter(|result| result.success).count();
        let failure_count = results.len().saturating_sub(success_count);
        let fastest = results
            .iter()
            .filter(|result| result.success)
            .filter_map(|result| result.latency_ms.map(|latency| (result, latency)))
            .min_by_key(|(_, latency)| *latency);
        let fastest_upstream_id = fastest.and_then(|(result, _)| result.id.clone());
        let fastest_latency_ms = fastest.map(|(_, latency)| latency);

        json_ok(
            StatusCode::OK,
            &UpstreamGroupTestResponse {
                ok: true,
                results,
                success_count,
                failure_count,
                fastest_upstream_id,
                fastest_latency_ms,
            },
        )
    }
}

impl Clone for TestQuery {
    fn clone(&self) -> Self {
        Self {
            qname: self.qname.clone(),
            qtype: self.qtype,
            timeout_ms: self.timeout_ms,
        }
    }
}

fn parse_test_query(
    qname: Option<&str>,
    qtype: Option<&str>,
    timeout_ms: Option<u64>,
) -> std::result::Result<TestQuery, String> {
    let raw_qname = qname.unwrap_or(DEFAULT_QNAME).trim();
    let qname =
        Name::from_ascii(raw_qname).map_err(|err| format!("invalid qname '{raw_qname}': {err}"))?;
    let raw_qtype = qtype.unwrap_or(DEFAULT_QTYPE).trim().to_ascii_uppercase();
    let qtype = RecordType::from_str(&raw_qtype)
        .map_err(|err| format!("invalid qtype '{raw_qtype}': {err}"))?;
    let timeout_ms = timeout_ms
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS);

    Ok(TestQuery {
        qname,
        qtype,
        timeout_ms,
    })
}

async fn run_upstream_test(
    id: Option<String>,
    name: Option<String>,
    input: UpstreamTestInput,
    query: TestQuery,
) -> UpstreamTestResult {
    let config = UpstreamConfig {
        tag: input.tag,
        addr: input.addr,
        dial_addr: input.dial_addr,
        port: None,
        bootstrap: input.bootstrap,
        bootstrap_version: None,
        socks5: None,
        idle_timeout: None,
        max_conns: None,
        min_conns: None,
        insecure_skip_verify: input.insecure_skip_verify,
        timeout: Some(std::time::Duration::from_millis(query.timeout_ms)),
        enable_pipeline: None,
        enable_http3: input.enable_http3,
        so_mark: None,
        bind_to_device: None,
    };

    let connection_info = match ConnectionInfo::try_from(config) {
        Ok(info) => info,
        Err(err) => {
            let message = err.to_string();
            let code = classify_error(&message);
            return failed_result(id, name, None, code, message);
        }
    };
    let protocol = Some(protocol_label(&connection_info).to_string());
    let upstream = match UpstreamBuilder::with_connection_info(connection_info) {
        Ok(upstream) => upstream,
        Err(err) => {
            let message = err.to_string();
            let code = classify_error(&message);
            return failed_result(id, name, protocol, code, message);
        }
    };

    let request = build_request(&query);
    let started = Instant::now();
    match upstream.query(request).await {
        Ok(response) => UpstreamTestResult {
            id,
            name,
            success: true,
            latency_ms: Some(started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)),
            protocol,
            rcode: Some(format!("{:?}", response.rcode())),
            answers: response.answers().iter().map(answer_summary).collect(),
            error_code: None,
            error_message: None,
        },
        Err(err) => {
            let message = err.to_string();
            let code = classify_error(&message);
            failed_result(id, name, protocol, code, message)
        }
    }
}

fn build_request(query: &TestQuery) -> Message {
    let mut request = Message::new();
    request.add_question(Question::new(
        query.qname.clone(),
        query.qtype,
        DNSClass::IN,
    ));
    request
}

fn answer_summary(record: &Record) -> UpstreamAnswerSummary {
    UpstreamAnswerSummary {
        name: record.name().to_fqdn(),
        rr_type: record.rr_type().to_string(),
        class: record.class().to_string(),
        ttl: record.ttl(),
        data: format!("{:?}", record.data()),
    }
}

fn failed_result(
    id: Option<String>,
    name: Option<String>,
    protocol: Option<String>,
    error_code: &'static str,
    error_message: String,
) -> UpstreamTestResult {
    UpstreamTestResult {
        id,
        name,
        success: false,
        latency_ms: None,
        protocol,
        rcode: None,
        answers: Vec::new(),
        error_code: Some(error_code),
        error_message: Some(error_message),
    }
}

fn classify_error(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if lower.contains("not compiled") || lower.contains("rebuild with --features upstream-") {
        "protocol_unsupported"
    } else if lower.contains("timeout") {
        "timeout"
    } else if lower.contains("invalid upstream") || lower.contains("invalid q") {
        "invalid_request"
    } else {
        "query_failed"
    }
}

fn protocol_label(info: &ConnectionInfo) -> &'static str {
    match info.connection_type {
        ConnectionType::UDP => "udp",
        ConnectionType::TCP => "tcp",
        ConnectionType::DoT => "dot",
        ConnectionType::DoQ => "doq",
        ConnectionType::DoH => {
            if info.enable_http3 {
                "doh3"
            } else {
                "doh"
            }
        }
    }
}

pub fn register_builtin_routes(register: &ApiRegister) -> Result<()> {
    register.register_post("/upstreams/test", Arc::new(UpstreamTestHandler))?;
    register.register_post("/upstreams/test-group", Arc::new(UpstreamGroupTestHandler))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use bytes::Bytes;
    use http::Request;
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tokio::net::UdpSocket;

    use super::*;
    use crate::infra::clock::AppClock;
    use crate::proto::rdata::A;
    use crate::proto::{RData, Rcode};

    async fn json_body(response: crate::api::ApiResponse) -> Value {
        let body = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&body).expect("response should be json")
    }

    fn request(body: Value) -> Request<Bytes> {
        Request::builder()
            .method("POST")
            .uri("/upstreams/test")
            .body(Bytes::from(body.to_string()))
            .unwrap()
    }

    async fn start_udp_mock() -> SocketAddr {
        AppClock::start();
        let socket = UdpSocket::bind("127.0.0.1:0").await.expect("bind udp");
        let addr = socket.local_addr().expect("local addr");
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            let Ok((len, peer)) = socket.recv_from(&mut buf).await else {
                return;
            };
            let Ok(request) = Message::from_bytes(&buf[..len]) else {
                return;
            };
            let mut response = request.response(Rcode::NoError);
            response.add_answer(Record::from_rdata(
                Name::from_ascii("example.com.").unwrap(),
                60,
                RData::A(A(std::net::Ipv4Addr::new(93, 184, 216, 34))),
            ));
            let Ok(bytes) = response.to_bytes() else {
                return;
            };
            let _ = socket.send_to(&bytes, peer).await;
        });
        addr
    }

    #[tokio::test]
    async fn upstream_test_rejects_invalid_json() {
        let handler = UpstreamTestHandler;
        let response = handler.handle(Request::new(Bytes::from("{not json"))).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(body["code"], "invalid_upstream_test_request");
    }

    #[tokio::test]
    async fn upstream_test_rejects_invalid_qtype() {
        let handler = UpstreamTestHandler;
        let response = handler
            .handle(request(json!({
                "upstream": { "addr": "127.0.0.1:53" },
                "qtype": "NOPE"
            })))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn upstream_test_reports_unsupported_protocol_as_result() {
        let handler = UpstreamTestHandler;
        let response = handler
            .handle(request(json!({
                "upstream": { "addr": "https://dns.example/dns-query" }
            })))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        if cfg!(feature = "upstream-doh") {
            assert_eq!(body["result"]["success"], false);
        } else {
            assert_eq!(body["result"]["success"], false);
            assert_eq!(body["result"]["error_code"], "protocol_unsupported");
        }
    }

    #[tokio::test]
    async fn upstream_test_queries_udp_mock() {
        let addr = start_udp_mock().await;
        let handler = UpstreamTestHandler;
        let response = handler
            .handle(request(json!({
                "upstream": { "addr": format!("udp://{addr}") },
                "qname": "example.com.",
                "qtype": "A",
                "timeout_ms": 1000
            })))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["result"]["success"], true);
        assert_eq!(body["result"]["protocol"], "udp");
        assert_eq!(body["result"]["rcode"], "NoError");
        assert_eq!(body["result"]["answers"][0]["rr_type"], "A");
    }

    #[tokio::test]
    async fn upstream_test_group_returns_fastest_success() {
        let addr = start_udp_mock().await;
        let handler = UpstreamGroupTestHandler;
        let response = handler
            .handle(
                Request::builder()
                    .method("POST")
                    .uri("/upstreams/test-group")
                    .body(Bytes::from(
                        json!({
                            "upstreams": [
                                { "id": "ok", "name": "OK", "addr": format!("udp://{addr}") },
                                { "id": "bad", "name": "Bad", "addr": "udp://127.0.0.1:9" }
                            ],
                            "timeout_ms": 1000
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["success_count"], 1);
        assert_eq!(body["failure_count"], 1);
        assert_eq!(body["fastest_upstream_id"], "ok");
    }
}
