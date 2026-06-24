// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP DNS entry handling for RFC 8484 and optional JSON API requests.

use std::net::SocketAddr;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use bytes::Bytes;
use http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
use http::{HeaderValue, Method, Response, StatusCode};
use serde_json::{Value, json};
use tracing::{debug, warn};

use crate::plugin::server::{RequestHandle, RequestMeta};
use crate::proto::{
    ClientSubnet, DNSClass, EdnsOption, Message, Name, Question, RData, Rcode, Record, RecordType,
};

const CONTENT_TYPE_DNS_MESSAGE: HeaderValue = HeaderValue::from_static("application/dns-message");
const CONTENT_TYPE_DNS_JSON: HeaderValue = HeaderValue::from_static("application/dns-json");
const CONTENT_TYPE_TEXT_PLAIN: HeaderValue = HeaderValue::from_static("text/plain");

pub struct HttpDnsEntry {
    request_handle: Arc<RequestHandle>,
    json_api_enabled: bool,
}

impl HttpDnsEntry {
    pub fn new(request_handle: Arc<RequestHandle>, json_api_enabled: bool) -> Self {
        Self {
            request_handle,
            json_api_enabled,
        }
    }

    pub async fn handle(
        &self,
        method: Method,
        path: Arc<str>,
        query: Option<Arc<str>>,
        body: Bytes,
        src_addr: SocketAddr,
        server_name: Option<Arc<str>>,
    ) -> Response<Bytes> {
        match method {
            Method::GET => self.handle_get(path, query, src_addr, server_name).await,
            Method::POST => self.handle_post(path, body, src_addr, server_name).await,
            _ => not_found_response(),
        }
    }

    async fn handle_get(
        &self,
        path: Arc<str>,
        query: Option<Arc<str>>,
        src_addr: SocketAddr,
        server_name: Option<Arc<str>>,
    ) -> Response<Bytes> {
        let dns_query = match parse_rfc8484_get_query(query.as_deref()) {
            DnsQueryParam::Found(message) => message,
            DnsQueryParam::Invalid => return invalid_dns_query_response(),
            DnsQueryParam::Missing if self.json_api_enabled => {
                match parse_json_api_query(query.as_deref()) {
                    Ok(request) => {
                        let dns_result = self
                            .request_handle
                            .handle_request(
                                request.message,
                                src_addr,
                                RequestMeta {
                                    server_name,
                                    url_path: Some(path),
                                },
                            )
                            .await;
                        return match request.response_format {
                            HttpDnsResponseFormat::Json => json_api_response(dns_result.response),
                            HttpDnsResponseFormat::DnsMessage => {
                                dns_message_response(dns_result.response)
                            }
                        };
                    }
                    Err(err) => {
                        warn!("Invalid JSON API DNS query: {}", err);
                        return invalid_dns_query_response();
                    }
                }
            }
            DnsQueryParam::Missing => return invalid_dns_query_response(),
        };

        let dns_result = self
            .request_handle
            .handle_request(
                dns_query,
                src_addr,
                RequestMeta {
                    server_name,
                    url_path: Some(path),
                },
            )
            .await;
        dns_message_response(dns_result.response)
    }

    async fn handle_post(
        &self,
        path: Arc<str>,
        body: Bytes,
        src_addr: SocketAddr,
        server_name: Option<Arc<str>>,
    ) -> Response<Bytes> {
        let dns_query = match parse_rfc8484_post_body(&body, src_addr) {
            PostBodyParse::Message(message) => message,
            PostBodyParse::Response(response) => return response,
        };

        let dns_result = self
            .request_handle
            .handle_request(
                dns_query,
                src_addr,
                RequestMeta {
                    server_name,
                    url_path: Some(path),
                },
            )
            .await;
        dns_message_response(dns_result.response)
    }
}

enum DnsQueryParam {
    Found(Message),
    Missing,
    Invalid,
}

enum PostBodyParse {
    Message(Message),
    Response(Response<Bytes>),
}

enum HttpDnsResponseFormat {
    Json,
    DnsMessage,
}

struct JsonApiRequest {
    message: Message,
    response_format: HttpDnsResponseFormat,
}

fn parse_rfc8484_get_query(query: Option<&str>) -> DnsQueryParam {
    let Some(query) = query else {
        return DnsQueryParam::Missing;
    };

    for param in query.split('&') {
        if let Some(value) = param.strip_prefix("dns=") {
            return match URL_SAFE_NO_PAD.decode(value) {
                Ok(dns_bytes) => match Message::from_bytes(&dns_bytes) {
                    Ok(message) => {
                        debug!("Successfully parsed GET DNS query, ID: {}", message.id());
                        DnsQueryParam::Found(message)
                    }
                    Err(err) => {
                        warn!("Failed to parse DNS message: {}", err);
                        DnsQueryParam::Invalid
                    }
                },
                Err(err) => {
                    warn!("Failed to decode base64: {}", err);
                    DnsQueryParam::Invalid
                }
            };
        }
    }

    DnsQueryParam::Missing
}

fn parse_rfc8484_post_body(body: &Bytes, src_addr: SocketAddr) -> PostBodyParse {
    const MAX_DNS_MESSAGE_SIZE: usize = 65535;
    if body.len() > MAX_DNS_MESSAGE_SIZE {
        warn!(
            "DNS message too large: {} bytes from {}",
            body.len(),
            src_addr
        );
        return PostBodyParse::Response(
            Response::builder()
                .status(StatusCode::PAYLOAD_TOO_LARGE)
                .header(CONTENT_TYPE, CONTENT_TYPE_TEXT_PLAIN)
                .body(Bytes::from_static(b"413 Payload Too Large"))
                .expect("Failed to build error response"),
        );
    }

    match Message::from_bytes(body) {
        Ok(message) => {
            debug!(
                "Successfully parsed POST DNS query, ID: {}, size: {} bytes",
                message.id(),
                body.len()
            );
            PostBodyParse::Message(message)
        }
        Err(err) => {
            warn!("Failed to parse DNS message: {}", err);
            PostBodyParse::Response(
                Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header(CONTENT_TYPE, CONTENT_TYPE_TEXT_PLAIN)
                    .body(Bytes::from_static(b"400 Bad Request: Invalid DNS message"))
                    .expect("Failed to build error response"),
            )
        }
    }
}

fn parse_json_api_query(query: Option<&str>) -> std::result::Result<JsonApiRequest, &'static str> {
    let query = query.ok_or("missing query string")?;
    let mut name = None;
    let mut qtype = None;
    let mut cd = None;
    let mut dnssec_ok = None;
    let mut ecs = None;
    let mut response_format = HttpDnsResponseFormat::Json;

    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "name" if name.is_none() => name = Some(value.into_owned()),
            "type" if qtype.is_none() => qtype = Some(value.into_owned()),
            "cd" if cd.is_none() => cd = Some(parse_json_api_bool(&value)?),
            "do" if dnssec_ok.is_none() => dnssec_ok = Some(parse_json_api_bool(&value)?),
            "edns_client_subnet" if ecs.is_none() => {
                ecs = Some(
                    value
                        .parse::<ClientSubnet>()
                        .map_err(|_| "invalid edns_client_subnet")?,
                );
            }
            "ct" => {
                if value.eq_ignore_ascii_case("application/dns-message") {
                    response_format = HttpDnsResponseFormat::DnsMessage;
                }
            }
            "random_padding" => {}
            _ => {}
        }
    }

    let name = name.ok_or("missing name")?;
    if name.trim().is_empty() {
        return Err("empty name");
    }

    let mut message = Message::new();
    message.set_recursion_desired(true);
    message.set_checking_disabled(cd.unwrap_or(false));
    message.add_question(Question::new(
        Name::from_ascii(&name).map_err(|_| "invalid name")?,
        match qtype {
            Some(raw) if !raw.trim().is_empty() => {
                RecordType::from_token(raw.trim()).ok_or("invalid type")?
            }
            _ => RecordType::A,
        },
        DNSClass::IN,
    ));

    if dnssec_ok.unwrap_or(false) || ecs.is_some() {
        let edns = message.ensure_edns_mut();
        edns.set_dnssec_ok(dnssec_ok.unwrap_or(false));
        if let Some(ecs) = ecs {
            edns.insert(EdnsOption::Subnet(ecs));
        }
    }

    Ok(JsonApiRequest {
        message,
        response_format,
    })
}

fn parse_json_api_bool(raw: &str) -> std::result::Result<bool, &'static str> {
    match raw.to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err("invalid boolean"),
    }
}

fn not_found_response() -> Response<Bytes> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, CONTENT_TYPE_TEXT_PLAIN)
        .body(Bytes::from_static(b"404 Not Found"))
        .expect("Failed to build 404 response")
}

#[inline]
fn invalid_dns_query_response() -> Response<Bytes> {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(CONTENT_TYPE, CONTENT_TYPE_TEXT_PLAIN)
        .body(Bytes::from_static(b"400 Bad Request: Invalid DNS query"))
        .expect("Failed to build error response")
}

#[inline]
fn dns_message_response(dns_response: Message) -> Response<Bytes> {
    match dns_response.to_bytes() {
        Ok(response_bytes) => {
            let size = response_bytes.len();
            debug!("DNS response size: {} bytes", size);
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, CONTENT_TYPE_DNS_MESSAGE)
                .header(CONTENT_LENGTH, size);

            if let Some(ttl) = http_cache_ttl(&dns_response) {
                builder = builder.header(CACHE_CONTROL, format!("private, max-age={ttl}"));
            }

            builder
                .body(Bytes::from(response_bytes))
                .expect("Failed to build DNS response")
        }
        Err(e) => {
            warn!("Failed to serialize DNS response: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(CONTENT_TYPE, CONTENT_TYPE_TEXT_PLAIN)
                .body(Bytes::from_static(b"500 Internal Server Error"))
                .expect("Failed to build error response")
        }
    }
}

fn json_api_response(dns_response: Message) -> Response<Bytes> {
    let body = match serde_json::to_vec(&json_api_response_value(&dns_response)) {
        Ok(body) => body,
        Err(err) => {
            warn!("Failed to serialize JSON API DNS response: {}", err);
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(CONTENT_TYPE, CONTENT_TYPE_TEXT_PLAIN)
                .body(Bytes::from_static(b"500 Internal Server Error"))
                .expect("Failed to build error response");
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, CONTENT_TYPE_DNS_JSON)
        .header(CONTENT_LENGTH, body.len())
        .body(Bytes::from(body))
        .expect("Failed to build JSON API DNS response")
}

fn json_api_response_value(message: &Message) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("Status".to_string(), json!(u16::from(message.rcode())));
    out.insert("TC".to_string(), json!(message.truncated()));
    out.insert("RD".to_string(), json!(message.recursion_desired()));
    out.insert("RA".to_string(), json!(message.recursion_available()));
    out.insert("AD".to_string(), json!(message.authentic_data()));
    out.insert("CD".to_string(), json!(message.checking_disabled()));

    if !message.questions().is_empty() {
        out.insert(
            "Question".to_string(),
            Value::Array(message.questions().iter().map(json_api_question).collect()),
        );
    }
    insert_json_api_records(&mut out, "Answer", message.answers());
    insert_json_api_records(&mut out, "Authority", message.authorities());
    insert_json_api_records(&mut out, "Additional", message.additionals());

    Value::Object(out)
}

fn insert_json_api_records(
    out: &mut serde_json::Map<String, Value>,
    key: &str,
    records: &[Record],
) {
    let records = records
        .iter()
        .filter(|record| record.rr_type() != RecordType::OPT)
        .map(json_api_record)
        .collect::<Vec<_>>();
    if !records.is_empty() {
        out.insert(key.to_string(), Value::Array(records));
    }
}

fn json_api_question(question: &Question) -> Value {
    json!({
        "name": question.name().to_fqdn(),
        "type": u16::from(question.qtype()),
    })
}

fn json_api_record(record: &Record) -> Value {
    json!({
        "name": record.name().to_fqdn(),
        "type": u16::from(record.rr_type()),
        "TTL": record.ttl(),
        "data": json_api_record_data(record.data()),
    })
}

fn json_api_record_data(rdata: &RData) -> String {
    match rdata {
        RData::A(value) => value.0.to_string(),
        RData::AAAA(value) => value.0.to_string(),
        RData::CNAME(value) => value.0.to_fqdn(),
        RData::NS(value) => value.0.to_fqdn(),
        RData::PTR(value) => value.0.to_fqdn(),
        RData::DNAME(value) => value.0.to_fqdn(),
        RData::MD(value) => value.0.to_fqdn(),
        RData::MF(value) => value.0.to_fqdn(),
        RData::MB(value) => value.0.to_fqdn(),
        RData::MG(value) => value.0.to_fqdn(),
        RData::MR(value) => value.0.to_fqdn(),
        RData::ANAME(value) => value.0.to_fqdn(),
        RData::NSAPPTR(value) => value.0.to_fqdn(),
        RData::MX(value) => format!("{} {}", value.preference(), value.exchange().to_fqdn()),
        RData::SRV(value) => format!(
            "{} {} {} {}",
            value.priority(),
            value.weight(),
            value.port(),
            value.target().to_fqdn()
        ),
        RData::SOA(value) => format!(
            "{} {} {} {} {} {} {}",
            value.mname().to_fqdn(),
            value.rname().to_fqdn(),
            value.serial(),
            value.refresh(),
            value.retry(),
            value.expire(),
            value.minimum()
        ),
        RData::TXT(value) => value
            .txt_data()
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>()
            .join(" "),
        other => format!("{other:?}"),
    }
}

#[inline]
fn http_cache_ttl(response: &Message) -> Option<u32> {
    match response.rcode() {
        Rcode::NoError => response
            .min_answer_ttl()
            .filter(|ttl| *ttl > 0)
            .or_else(|| {
                if response.answers().is_empty() {
                    response.negative_ttl_from_soa().filter(|ttl| *ttl > 0)
                } else {
                    None
                }
            }),
        Rcode::NXDomain => response.negative_ttl_from_soa().filter(|ttl| *ttl > 0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use serde_json::Value;

    use super::*;
    use crate::core::context::DnsContext;
    use crate::infra::error::Result;
    use crate::plugin::Plugin;
    use crate::plugin::executor::{ExecStep, Executor};
    use crate::plugin::server::http::http_dispatcher::HttpDispatcher;
    use crate::proto::{EdnsOption, Name, Question, RData, Record, RecordType};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ObservedRequest {
        query_name: String,
        query_type: RecordType,
        query_id: u16,
        checking_disabled: bool,
        dnssec_ok: bool,
        ecs: Option<String>,
        server_name: Option<String>,
        url_path: Option<String>,
    }

    #[derive(Debug)]
    struct RecordingExecutor {
        observed: Arc<Mutex<Option<ObservedRequest>>>,
        response_code: Rcode,
        answer: Option<Record>,
    }

    #[async_trait]
    impl Plugin for RecordingExecutor {
        fn tag(&self) -> &str {
            "recording_executor"
        }

        async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
            Ok(())
        }

        async fn destroy(&self) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Executor for RecordingExecutor {
        async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
            let question = context
                .request
                .first_question()
                .expect("request should contain one query");
            let query_name = question.name().normalized().to_string();
            let dnssec_ok = context
                .request
                .edns()
                .as_ref()
                .is_some_and(|edns| edns.flags().dnssec_ok);
            let ecs = context.request.edns().as_ref().and_then(|edns| {
                edns.options().iter().find_map(|option| match option {
                    EdnsOption::Subnet(subnet) => {
                        Some(format!("{}/{}", subnet.addr(), subnet.source_prefix()))
                    }
                    _ => None,
                })
            });
            let observed = ObservedRequest {
                query_name,
                query_type: question.qtype(),
                query_id: context.request.id(),
                checking_disabled: context.request.checking_disabled(),
                dnssec_ok,
                ecs,
                server_name: context.server_name().map(str::to_string),
                url_path: context.url_path().map(str::to_string),
            };
            self.observed
                .lock()
                .expect("request observation lock should not be poisoned")
                .replace(observed);
            let mut response = context.request.response(self.response_code);
            if let Some(answer) = self.answer.clone() {
                response.add_answer(answer);
            }
            context.set_response(response);
            Ok(ExecStep::Next)
        }
    }

    fn make_request_handle(
        response_code: Rcode,
    ) -> (Arc<RequestHandle>, Arc<Mutex<Option<ObservedRequest>>>) {
        make_request_handle_with_answer(response_code, None)
    }

    fn make_request_handle_with_answer(
        response_code: Rcode,
        answer: Option<Record>,
    ) -> (Arc<RequestHandle>, Arc<Mutex<Option<ObservedRequest>>>) {
        let observed = Arc::new(Mutex::new(None));
        let executor = Arc::new(RecordingExecutor {
            observed: observed.clone(),
            response_code,
            answer,
        });
        (
            Arc::new(RequestHandle {
                entry_executor: executor,
                metrics: None,
            }),
            observed,
        )
    }

    fn make_dns_query(id: u16, qname: &str) -> Message {
        let mut request = Message::new();
        request.set_id(id);
        request.add_question(Question::new(
            Name::from_ascii(qname).expect("query name should be valid"),
            RecordType::A,
            crate::proto::DNSClass::IN,
        ));
        request
    }

    fn encode_query(message: &Message) -> String {
        URL_SAFE_NO_PAD.encode(
            message
                .to_bytes()
                .expect("DNS query should serialize successfully"),
        )
    }

    fn decode_response(response: &Response<Bytes>) -> Message {
        Message::from_bytes(response.body()).expect("HTTP body should contain DNS wire format")
    }

    #[tokio::test]
    async fn test_http_dns_entry_get_returns_bad_request_when_dns_param_is_missing() {
        let (request_handle, observed) = make_request_handle(Rcode::NoError);
        let mut dispatcher = HttpDispatcher::new();
        dispatcher.register_route(
            Arc::from("/dns-query"),
            HttpDnsEntry::new(request_handle, false),
        );

        let response = dispatcher
            .handle_request(
                Method::GET,
                Arc::from("/dns-query"),
                Some(Arc::from("foo=bar")),
                Bytes::new(),
                SocketAddr::from(([127, 0, 0, 1], 5401)),
                Some(Arc::from("dns.example.test")),
            )
            .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.body().as_ref(),
            b"400 Bad Request: Invalid DNS query"
        );
        assert!(
            observed
                .lock()
                .expect("request observation lock should not be poisoned")
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_http_dns_entry_get_processes_valid_query_and_forwards_meta() {
        let (request_handle, observed) = make_request_handle(Rcode::Refused);
        let mut dispatcher = HttpDispatcher::new();
        dispatcher.register_route(
            Arc::from("/dns-query"),
            HttpDnsEntry::new(request_handle, false),
        );
        let query = make_dns_query(31, "www.example.test.");
        let encoded_query = encode_query(&query);

        let response = dispatcher
            .handle_request(
                Method::GET,
                Arc::from("/dns-query"),
                Some(Arc::from(format!("foo=bar&dns={encoded_query}"))),
                Bytes::new(),
                SocketAddr::from(([127, 0, 0, 1], 5402)),
                Some(Arc::from("dns.example.test")),
            )
            .await;

        let dns_response = decode_response(&response);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["Content-Type"],
            "application/dns-message"
        );
        assert_eq!(
            response.headers()["Content-Length"],
            dns_response
                .to_bytes()
                .expect("DNS response should serialize")
                .len()
                .to_string()
        );
        assert!(!response.headers().contains_key("Cache-Control"));
        assert_eq!(dns_response.id(), 31);
        assert_eq!(dns_response.rcode(), Rcode::Refused);
        assert_eq!(
            observed
                .lock()
                .expect("request observation lock should not be poisoned")
                .clone(),
            Some(ObservedRequest {
                query_name: "www.example.test".to_string(),
                query_type: RecordType::A,
                query_id: 31,
                checking_disabled: false,
                dnssec_ok: false,
                ecs: None,
                server_name: Some("dns.example.test".to_string()),
                url_path: Some("/dns-query".to_string()),
            })
        );
    }

    #[tokio::test]
    async fn test_http_dns_entry_get_processes_json_api_query() {
        let answer = Record::from_rdata(
            Name::from_ascii("www.example.test.").expect("answer name should parse"),
            60,
            RData::A(crate::proto::rdata::A(std::net::Ipv4Addr::new(
                203, 0, 113, 10,
            ))),
        );
        let (request_handle, observed) =
            make_request_handle_with_answer(Rcode::NoError, Some(answer));
        let mut dispatcher = HttpDispatcher::new();
        dispatcher.register_route(
            Arc::from("/dns-query"),
            HttpDnsEntry::new(request_handle, true),
        );

        let response = dispatcher
            .handle_request(
                Method::GET,
                Arc::from("/dns-query"),
                Some(Arc::from(
                    "name=www.example.test&type=A&cd=1&do=true&edns_client_subnet=198.51.100.0/24&random_padding=abc",
                )),
                Bytes::new(),
                SocketAddr::from(([127, 0, 0, 1], 5406)),
                Some(Arc::from("dns.example.test")),
            )
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["Content-Type"], "application/dns-json");
        let json: Value =
            serde_json::from_slice(response.body()).expect("response should be valid JSON");
        assert_eq!(json["Status"], 0);
        assert_eq!(json["CD"], true);
        assert_eq!(json["Question"][0]["name"], "www.example.test.");
        assert_eq!(json["Question"][0]["type"], 1);
        assert_eq!(json["Answer"][0]["name"], "www.example.test.");
        assert_eq!(json["Answer"][0]["type"], 1);
        assert_eq!(json["Answer"][0]["TTL"], 60);
        assert_eq!(json["Answer"][0]["data"], "203.0.113.10");
        assert_eq!(
            observed
                .lock()
                .expect("request observation lock should not be poisoned")
                .clone(),
            Some(ObservedRequest {
                query_name: "www.example.test".to_string(),
                query_type: RecordType::A,
                query_id: 0,
                checking_disabled: true,
                dnssec_ok: true,
                ecs: Some("198.51.100.0/24".to_string()),
                server_name: Some("dns.example.test".to_string()),
                url_path: Some("/dns-query".to_string()),
            })
        );
    }

    #[tokio::test]
    async fn test_http_dns_entry_get_json_api_query_can_return_wire_format() {
        let (request_handle, observed) = make_request_handle(Rcode::NoError);
        let mut dispatcher = HttpDispatcher::new();
        dispatcher.register_route(
            Arc::from("/dns-query"),
            HttpDnsEntry::new(request_handle, true),
        );

        let response = dispatcher
            .handle_request(
                Method::GET,
                Arc::from("/dns-query"),
                Some(Arc::from(
                    "name=ipv6.example.test&type=28&ct=application/dns-message",
                )),
                Bytes::new(),
                SocketAddr::from(([127, 0, 0, 1], 5407)),
                None,
            )
            .await;

        let dns_response = decode_response(&response);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["Content-Type"],
            "application/dns-message"
        );
        assert_eq!(dns_response.questions()[0].qtype(), RecordType::AAAA);
        assert_eq!(
            observed
                .lock()
                .expect("request observation lock should not be poisoned")
                .clone(),
            Some(ObservedRequest {
                query_name: "ipv6.example.test".to_string(),
                query_type: RecordType::AAAA,
                query_id: 0,
                checking_disabled: false,
                dnssec_ok: false,
                ecs: None,
                server_name: None,
                url_path: Some("/dns-query".to_string()),
            })
        );
    }

    #[tokio::test]
    async fn test_http_dns_entry_get_dns_param_takes_precedence_over_json_api_query() {
        let (request_handle, observed) = make_request_handle(Rcode::NoError);
        let mut dispatcher = HttpDispatcher::new();
        dispatcher.register_route(
            Arc::from("/dns-query"),
            HttpDnsEntry::new(request_handle, true),
        );
        let encoded_query = encode_query(&make_dns_query(77, "wire.example.test."));

        let response = dispatcher
            .handle_request(
                Method::GET,
                Arc::from("/dns-query"),
                Some(Arc::from(format!(
                    "name=ignored.example.test&type=AAAA&dns={encoded_query}"
                ))),
                Bytes::new(),
                SocketAddr::from(([127, 0, 0, 1], 5408)),
                None,
            )
            .await;

        let dns_response = decode_response(&response);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["Content-Type"],
            "application/dns-message"
        );
        assert_eq!(dns_response.id(), 77);
        assert_eq!(
            observed
                .lock()
                .expect("request observation lock should not be poisoned")
                .clone(),
            Some(ObservedRequest {
                query_name: "wire.example.test".to_string(),
                query_type: RecordType::A,
                query_id: 77,
                checking_disabled: false,
                dnssec_ok: false,
                ecs: None,
                server_name: None,
                url_path: Some("/dns-query".to_string()),
            })
        );
    }

    #[tokio::test]
    async fn test_http_dns_entry_get_rejects_invalid_json_api_query() {
        let (request_handle, observed) = make_request_handle(Rcode::NoError);
        let mut dispatcher = HttpDispatcher::new();
        dispatcher.register_route(
            Arc::from("/dns-query"),
            HttpDnsEntry::new(request_handle, true),
        );

        for query in [
            "type=A",
            "name=example.test&type=70000",
            "name=example.test&do=maybe",
            "name=example.test&edns_client_subnet=198.51.100.0/129",
        ] {
            let response = dispatcher
                .handle_request(
                    Method::GET,
                    Arc::from("/dns-query"),
                    Some(Arc::from(query)),
                    Bytes::new(),
                    SocketAddr::from(([127, 0, 0, 1], 5409)),
                    None,
                )
                .await;

            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
        }
        assert!(
            observed
                .lock()
                .expect("request observation lock should not be poisoned")
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_http_dns_entry_post_returns_payload_too_large_for_oversized_body() {
        let (request_handle, observed) = make_request_handle(Rcode::NoError);
        let mut dispatcher = HttpDispatcher::new();
        dispatcher.register_route(
            Arc::from("/dns-query"),
            HttpDnsEntry::new(request_handle, false),
        );

        let response = dispatcher
            .handle_request(
                Method::POST,
                Arc::from("/dns-query"),
                None,
                Bytes::from(vec![0u8; 65536]),
                SocketAddr::from(([127, 0, 0, 1], 5403)),
                Some(Arc::from("dns.example.test")),
            )
            .await;

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(response.body().as_ref(), b"413 Payload Too Large");
        assert!(
            observed
                .lock()
                .expect("request observation lock should not be poisoned")
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_http_dns_entry_post_returns_bad_request_for_invalid_dns_body() {
        let (request_handle, observed) = make_request_handle(Rcode::NoError);
        let mut dispatcher = HttpDispatcher::new();
        dispatcher.register_route(
            Arc::from("/dns-query"),
            HttpDnsEntry::new(request_handle, false),
        );

        let response = dispatcher
            .handle_request(
                Method::POST,
                Arc::from("/dns-query"),
                None,
                Bytes::from_static(b"not-a-dns-message"),
                SocketAddr::from(([127, 0, 0, 1], 5404)),
                Some(Arc::from("dns.example.test")),
            )
            .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.body().as_ref(),
            b"400 Bad Request: Invalid DNS message"
        );
        assert!(
            observed
                .lock()
                .expect("request observation lock should not be poisoned")
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_http_dns_entry_post_processes_valid_body_and_forwards_meta() {
        let (request_handle, observed) = make_request_handle(Rcode::NXDomain);
        let mut dispatcher = HttpDispatcher::new();
        dispatcher.register_route(
            Arc::from("/dns-query"),
            HttpDnsEntry::new(request_handle, false),
        );
        let query = make_dns_query(41, "api.example.test.");
        let query_bytes = query
            .to_bytes()
            .expect("DNS query should serialize successfully");

        let response = dispatcher
            .handle_request(
                Method::POST,
                Arc::from("/dns-query"),
                None,
                Bytes::from(query_bytes),
                SocketAddr::from(([127, 0, 0, 1], 5405)),
                Some(Arc::from("dns.example.test")),
            )
            .await;

        let dns_response = decode_response(&response);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["Content-Length"],
            dns_response
                .to_bytes()
                .expect("DNS response should serialize")
                .len()
                .to_string()
        );
        assert!(!response.headers().contains_key("Cache-Control"));
        assert_eq!(dns_response.id(), 41);
        assert_eq!(dns_response.rcode(), Rcode::NXDomain);
        assert_eq!(
            observed
                .lock()
                .expect("request observation lock should not be poisoned")
                .clone(),
            Some(ObservedRequest {
                query_name: "api.example.test".to_string(),
                query_type: RecordType::A,
                query_id: 41,
                checking_disabled: false,
                dnssec_ok: false,
                ecs: None,
                server_name: Some("dns.example.test".to_string()),
                url_path: Some("/dns-query".to_string()),
            })
        );
    }

    #[test]
    fn test_http_cache_ttl_prefers_min_answer_ttl_for_positive_response() {
        let mut response = Message::new();
        response.set_message_type(crate::proto::MessageType::Response);
        response.set_rcode(Rcode::NoError);
        response.add_answer(crate::proto::Record::from_rdata(
            Name::from_ascii("example.com.").expect("name should parse"),
            120,
            crate::proto::RData::A(crate::proto::rdata::A(std::net::Ipv4Addr::new(1, 1, 1, 1))),
        ));
        response.add_answer(crate::proto::Record::from_rdata(
            Name::from_ascii("example.com.").expect("name should parse"),
            30,
            crate::proto::RData::A(crate::proto::rdata::A(std::net::Ipv4Addr::new(1, 0, 0, 1))),
        ));

        assert_eq!(http_cache_ttl(&response), Some(30));
    }

    #[test]
    fn test_http_cache_ttl_uses_soa_for_nxdomain() {
        let mut response = Message::new();
        response.set_message_type(crate::proto::MessageType::Response);
        response.set_rcode(Rcode::NXDomain);
        response.add_authority(crate::proto::Record::from_rdata(
            Name::from_ascii("example.com.").expect("name should parse"),
            180,
            crate::proto::RData::SOA(crate::proto::rdata::SOA::new(
                Name::from_ascii("ns1.example.com.").expect("mname should parse"),
                Name::from_ascii("hostmaster.example.com.").expect("rname should parse"),
                1,
                7200,
                1800,
                86400,
                60,
            )),
        ));

        assert_eq!(http_cache_ttl(&response), Some(60));
    }

    #[test]
    fn test_http_cache_ttl_uses_soa_for_nodata() {
        let mut response = Message::new();
        response.set_message_type(crate::proto::MessageType::Response);
        response.set_rcode(Rcode::NoError);
        response.add_authority(crate::proto::Record::from_rdata(
            Name::from_ascii("example.com.").expect("name should parse"),
            90,
            crate::proto::RData::SOA(crate::proto::rdata::SOA::new(
                Name::from_ascii("ns1.example.com.").expect("mname should parse"),
                Name::from_ascii("hostmaster.example.com.").expect("rname should parse"),
                1,
                7200,
                1800,
                86400,
                120,
            )),
        ));

        assert_eq!(http_cache_ttl(&response), Some(90));
    }

    #[test]
    fn test_http_cache_ttl_omits_header_when_no_safe_ttl_exists() {
        let mut response = Message::new();
        response.set_message_type(crate::proto::MessageType::Response);
        response.set_rcode(Rcode::NXDomain);

        assert_eq!(http_cache_ttl(&response), None);
    }
}
