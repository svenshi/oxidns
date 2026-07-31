// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "standard")]

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket as StdUdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use oxidns::config::standard_mode::{
    StandardCapabilities, StandardIntent, compile_standard_intent, decode_standard_intent,
};
use oxidns::config::types::Config;
use oxidns::core::context::DnsContext;
use oxidns::infra::clock::AppClock;
use oxidns::infra::error::{DnsError, Result};
use oxidns::infra::network::transport::tcp::{TcpTransportReader, TcpTransportWriter};
use oxidns::infra::network::transport::udp::UdpTransport;
use oxidns::plugin::{self, PluginRegistry};
use oxidns::proto::rdata::{CNAME, TXT};
use oxidns::proto::{DNSClass, Message, Name, Question, RData, Rcode, Record, RecordType};
use serde_json::json;
use tokio::net::{TcpStream, UdpSocket};
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

fn reserve_local_addr() -> Result<SocketAddr> {
    let socket = StdUdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    Ok(socket.local_addr()?)
}

async fn start_mock_upstream(
    answer: Ipv4Addr,
) -> Result<(SocketAddr, Arc<AtomicUsize>, JoinHandle<()>)> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
    let address = socket.local_addr()?;
    let transport = UdpTransport::new(socket);
    let count = Arc::new(AtomicUsize::new(0));
    let task_count = count.clone();
    let task = tokio::spawn(async move {
        let mut buffer = [0_u8; 4096];
        loop {
            let Ok((request, peer)) = transport.read_message_from(&mut buffer).await else {
                break;
            };
            task_count.fetch_add(1, Ordering::SeqCst);
            let Some(question) = request.questions().first() else {
                continue;
            };
            let Ok(response) = request.address_response(question, 300, &[IpAddr::V4(answer)])
            else {
                continue;
            };
            if transport
                .write_message_to(&response, peer, request.max_payload())
                .await
                .is_err()
            {
                break;
            }
        }
    });
    Ok((address, count, task))
}

#[derive(Clone, Copy)]
enum MockAnswer {
    Address(Ipv4Addr),
    Nodata,
    NxDomain,
    ServFail,
    CnameOnly,
    Text,
    NoResponse,
}

async fn start_policy_upstream(
    answer: MockAnswer,
) -> Result<(SocketAddr, Arc<AtomicUsize>, JoinHandle<()>)> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
    let address = socket.local_addr()?;
    let transport = UdpTransport::new(socket);
    let count = Arc::new(AtomicUsize::new(0));
    let task_count = count.clone();
    let task = tokio::spawn(async move {
        let mut buffer = [0_u8; 4096];
        loop {
            let Ok((request, peer)) = transport.read_message_from(&mut buffer).await else {
                break;
            };
            task_count.fetch_add(1, Ordering::SeqCst);
            if matches!(answer, MockAnswer::NoResponse) {
                continue;
            }
            let response = match answer {
                MockAnswer::Address(ip) => {
                    let Some(question) = request.questions().first() else {
                        continue;
                    };
                    let Ok(response) = request.address_response(question, 60, &[IpAddr::V4(ip)])
                    else {
                        continue;
                    };
                    response
                }
                MockAnswer::Nodata => request.response(Rcode::NoError),
                MockAnswer::NxDomain => request.response(Rcode::NXDomain),
                MockAnswer::ServFail => request.response(Rcode::ServFail),
                MockAnswer::CnameOnly => {
                    let mut response = request.response(Rcode::NoError);
                    let owner = request
                        .first_question()
                        .map(|question| question.name().clone())
                        .unwrap_or_else(Name::root);
                    response.add_answer(Record::from_rdata(
                        owner,
                        60,
                        RData::CNAME(CNAME(
                            Name::from_ascii("target.smart.test.").expect("valid CNAME"),
                        )),
                    ));
                    response
                }
                MockAnswer::Text => {
                    let mut response = request.response(Rcode::NoError);
                    let owner = request
                        .first_question()
                        .map(|question| question.name().clone())
                        .unwrap_or_else(Name::root);
                    response.add_answer(Record::from_rdata(
                        owner,
                        60,
                        RData::TXT(TXT::new(Box::from([
                            8_u8, b'p', b'h', b'a', b's', b'e', b'-', b'2', b'!',
                        ]))),
                    ));
                    response
                }
                MockAnswer::NoResponse => unreachable!(),
            };
            if transport
                .write_message_to(&response, peer, request.max_payload())
                .await
                .is_err()
            {
                break;
            }
        }
    });
    Ok((address, count, task))
}

fn compiled_standard_config(intent: serde_json::Value) -> Result<Config> {
    AppClock::start();
    #[cfg(debug_assertions)]
    plugin::enable_runtime_test_serialization();
    let (intent, migration) = decode_standard_intent(intent)
        .map_err(|err| DnsError::config(format!("failed to decode Standard intent: {err}")))?;
    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, migration);
    if !plan.can_apply {
        return Err(DnsError::config(format!(
            "Standard plan failed: {:?}",
            plan.diagnostics
        )));
    }
    let generated = plan
        .generated
        .ok_or_else(|| DnsError::config("Standard plan did not generate YAML"))?;
    let config: Config = serde_yaml_ng::from_str(&generated.yaml)?;
    config.validate()?;
    Ok(config)
}

fn standard_intent(
    listen: SocketAddr,
    primary_upstream: SocketAddr,
    secondary_upstream: Option<SocketAddr>,
) -> serde_json::Value {
    let mut intent = serde_json::to_value(StandardIntent::default())
        .expect("default Standard intent should serialize");
    intent["listen"]["address"] = json!(listen.to_string());
    intent["queryLog"]["enabled"] = json!(false);
    intent["upstreamGroups"][0]["upstreams"] = json!([{
        "id": "primary",
        "name": "Primary",
        "protocol": "udp",
        "address": primary_upstream.to_string(),
        "enabled": true,
        "tlsVerify": true
    }]);
    if let Some(secondary_upstream) = secondary_upstream {
        intent["upstreamGroups"]
            .as_array_mut()
            .expect("groups array")
            .push(json!({
                "id": "secondary",
                "name": "Secondary",
                "strategy": "balanced",
                "isDefault": false,
                "upstreams": [{
                    "id": "secondary",
                    "name": "Secondary",
                    "protocol": "udp",
                    "address": secondary_upstream.to_string(),
                    "enabled": true,
                    "tlsVerify": true
                }]
            }));
        intent["paths"]
            .as_array_mut()
            .expect("paths array")
            .push(json!({
                "id": "secondary",
                "name": "Secondary path",
                "upstreamGroupId": "secondary",
                "filtering": "inherit",
                "cache": "inherit",
                "queryLog": "inherit",
                "dualStack": "inherit",
                "ipSelection": { "enabled": false },
                "ecs": { "mode": "inherit" }
            }));
        intent["devices"] = json!([{
            "id": "loopback-secondary",
            "name": "Secondary loopback client",
            "addresses": ["127.0.0.2"],
            "assignedPathId": "secondary"
        }]);
    }
    intent
}

fn smart_standard_intent(
    listen: SocketAddr,
    domestic_upstream: SocketAddr,
    remote_upstream: SocketAddr,
    unknown_mode: &str,
) -> serde_json::Value {
    let mut intent = standard_intent(listen, domestic_upstream, Some(remote_upstream));
    intent["devices"] = json!([]);
    intent["cache"]["enabled"] = json!(false);
    intent["ruleData"] = json!({
        "domesticDomains": { "sources": [{
            "type": "manual",
            "id": "domestic_domains",
            "name": "Domestic domains",
            "enabled": true,
            "rules": ["domain:domestic.smart.test"]
        }]},
        "foreignDomains": { "sources": [] },
        "domesticIps": { "sources": [{
            "type": "manual",
            "id": "domestic_ips",
            "name": "Domestic IPs",
            "enabled": true,
            "rules": ["10.0.0.0/8"]
        }]},
        "directDomains": { "sources": [] },
        "remoteDomains": { "sources": [{
            "type": "manual",
            "id": "remote_domains",
            "name": "Remote domains",
            "enabled": true,
            "rules": ["domain:remote.smart.test"]
        }]},
        "ddnsDomains": { "sources": [] }
    });
    intent["smartRouting"] = json!({
        "enabled": true,
        "domesticPathId": "default",
        "remotePathId": "secondary",
        "unknownMode": unknown_mode,
        "privacyFallbackToDomestic": false,
        "fallbackThresholdMs": 25,
        "responsePolicy": {
            "domesticIpMismatch": true,
            "cnameOnly": true,
            "nodata": true,
            "nxdomain": true,
            "servfail": true,
            "timeout": true,
            "transportFailure": true
        }
    });
    intent
}

fn query_for(name: &str, qtype: RecordType) -> Message {
    let mut request = Message::new();
    request.set_id(0x534D);
    request.add_question(Question::new(
        Name::from_ascii(name).expect("valid query name"),
        qtype,
        DNSClass::IN,
    ));
    request
}

fn query() -> Message {
    query_for("cache-isolation.test.", RecordType::A)
}

async fn exchange_udp(server: SocketAddr) -> Result<Message> {
    exchange_udp_query(server, query()).await
}

async fn exchange_udp_query(server: SocketAddr, request: Message) -> Result<Message> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
    socket.connect(server).await?;
    let transport = UdpTransport::new(socket);
    transport
        .write_message_with_id(&request, request.id())
        .await?;
    let mut buffer = [0_u8; 4096];
    timeout(Duration::from_secs(2), transport.read_message(&mut buffer))
        .await
        .map_err(|_| DnsError::runtime("timed out waiting for Standard UDP response"))?
}

async fn execute_standard_main(registry: Arc<PluginRegistry>, client: Ipv4Addr) -> Result<Message> {
    execute_standard_main_query(registry, client, query()).await
}

async fn execute_standard_main_query(
    registry: Arc<PluginRegistry>,
    client: Ipv4Addr,
    request: Message,
) -> Result<Message> {
    let executor = registry
        .get_plugin("standard_main_sequence")
        .ok_or_else(|| DnsError::runtime("compiled Standard main sequence is missing"))?
        .to_executor();
    let mut context = DnsContext::new(SocketAddr::from((client, 5300)), request);
    executor.execute(&mut context).await?;
    context
        .response()
        .cloned()
        .ok_or_else(|| DnsError::runtime("compiled Standard main sequence returned no response"))
}

async fn exchange_tcp(server: SocketAddr) -> Result<Message> {
    exchange_tcp_query(server, query()).await
}

async fn exchange_tcp_query(server: SocketAddr, request: Message) -> Result<Message> {
    let stream = TcpStream::connect(server).await?;
    let (reader, writer) = stream.into_split();
    let mut reader = TcpTransportReader::new(reader);
    let mut writer = TcpTransportWriter::new(writer);
    writer.write_message(&request).await?;
    timeout(Duration::from_secs(2), reader.read_message())
        .await
        .map_err(|_| DnsError::runtime("timed out waiting for Standard TCP response"))?
}

fn answer_ip(response: &Message) -> Option<IpAddr> {
    response
        .answers()
        .first()
        .and_then(|record| record.ip_addr())
}

#[tokio::test]
async fn standard_mode_udp_paths_keep_cache_entries_isolated() -> Result<()> {
    let (primary_addr, primary_count, primary_task) =
        start_mock_upstream(Ipv4Addr::new(192, 0, 2, 10)).await?;
    let (secondary_addr, secondary_count, secondary_task) =
        start_mock_upstream(Ipv4Addr::new(198, 51, 100, 20)).await?;
    let listen = reserve_local_addr()?;
    let config =
        compiled_standard_config(standard_intent(listen, primary_addr, Some(secondary_addr)))?;
    let registry = plugin::init(config).await?;

    let primary_first = exchange_udp(listen).await?;
    let secondary_first =
        execute_standard_main(registry.clone(), Ipv4Addr::new(127, 0, 0, 2)).await?;
    let primary_cached = exchange_udp(listen).await?;
    let secondary_cached =
        execute_standard_main(registry.clone(), Ipv4Addr::new(127, 0, 0, 2)).await?;

    assert_eq!(
        answer_ip(&primary_first),
        Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)))
    );
    assert_eq!(answer_ip(&primary_cached), answer_ip(&primary_first));
    assert_eq!(
        answer_ip(&secondary_first),
        Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 20)))
    );
    assert_eq!(answer_ip(&secondary_cached), answer_ip(&secondary_first));
    assert_eq!(primary_count.load(Ordering::SeqCst), 1);
    assert_eq!(secondary_count.load(Ordering::SeqCst), 1);

    registry.destroy().await;
    primary_task.abort();
    secondary_task.abort();
    Ok(())
}

#[tokio::test]
async fn standard_mode_tcp_listener_uses_the_compiled_default_path() -> Result<()> {
    let (upstream_addr, upstream_count, upstream_task) =
        start_mock_upstream(Ipv4Addr::new(203, 0, 113, 30)).await?;
    let listen = reserve_local_addr()?;
    let config = compiled_standard_config(standard_intent(listen, upstream_addr, None))?;
    let registry = plugin::init(config).await?;

    let response = exchange_tcp(listen).await?;
    assert_eq!(
        answer_ip(&response),
        Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 30)))
    );
    assert_eq!(upstream_count.load(Ordering::SeqCst), 1);

    registry.destroy().await;
    upstream_task.abort();
    Ok(())
}

#[tokio::test]
async fn standard_mode_native_local_policies_short_circuit_and_ddns_bypasses_cache() -> Result<()> {
    let (upstream_addr, upstream_count, upstream_task) =
        start_mock_upstream(Ipv4Addr::new(203, 0, 113, 40)).await?;
    let listen = reserve_local_addr()?;
    let mut intent = standard_intent(listen, upstream_addr, None);
    intent["local"] = json!({
        "hosts": {
            "entries": ["full:router.test 192.0.2.50"],
            "files": []
        },
        "redirects": { "rules": [], "files": [] },
        "records": {
            "rules": ["answer.test. 60 IN A 192.0.2.60"],
            "files": []
        },
        "responseTtl": { "enabled": false, "min": 30, "max": 86400 },
        "qtypePolicy": {
            "enabled": true,
            "qtypes": ["HTTPS", "SVCB"],
            "response": "nodata"
        },
        "ddns": {
            "enabled": true,
            "domains": ["dynamic.test"],
            "ttl": 20
        }
    });
    let config = compiled_standard_config(intent)?;
    let registry = plugin::init(config).await?;

    let hosts = exchange_udp_query(listen, query_for("router.test.", RecordType::A)).await?;
    assert_eq!(
        answer_ip(&hosts),
        Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 50)))
    );
    let record = exchange_udp_query(listen, query_for("answer.test.", RecordType::A)).await?;
    assert_eq!(
        answer_ip(&record),
        Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 60)))
    );
    let qtype = exchange_udp_query(listen, query_for("service.test.", RecordType::HTTPS)).await?;
    assert_eq!(qtype.rcode(), Rcode::NoError);
    assert!(qtype.answers().is_empty());
    assert_eq!(upstream_count.load(Ordering::SeqCst), 0);

    let ddns_first = exchange_udp_query(listen, query_for("dynamic.test.", RecordType::A)).await?;
    let ddns_second = exchange_udp_query(listen, query_for("dynamic.test.", RecordType::A)).await?;
    assert_eq!(ddns_first.answers()[0].ttl(), 20);
    assert_eq!(ddns_second.answers()[0].ttl(), 20);
    assert_eq!(upstream_count.load(Ordering::SeqCst), 2);

    let normal_first = exchange_udp(listen).await?;
    let normal_cached = exchange_udp(listen).await?;
    assert_eq!(answer_ip(&normal_first), answer_ip(&normal_cached));
    assert_eq!(upstream_count.load(Ordering::SeqCst), 3);

    registry.destroy().await;
    upstream_task.abort();
    Ok(())
}

#[tokio::test]
async fn standard_mode_rule_routes_and_filtering_form_one_runtime_pipeline() -> Result<()> {
    let (primary_addr, primary_count, primary_task) =
        start_mock_upstream(Ipv4Addr::new(192, 0, 2, 70)).await?;
    let (secondary_addr, secondary_count, secondary_task) =
        start_mock_upstream(Ipv4Addr::new(198, 51, 100, 80)).await?;
    let listen = reserve_local_addr()?;
    let mut intent = standard_intent(listen, primary_addr, Some(secondary_addr));
    intent["filtering"] = json!({
        "enabled": true,
        "subscriptions": [],
        "localFiles": [],
        "blockRules": ["||ads.test^"],
        "allowRules": [],
        "blockResponse": "nxdomain"
    });
    intent["routing"] = json!({
        "enabled": true,
        "rules": [{
            "id": "route_secondary",
            "name": "Route secondary",
            "enabled": true,
            "condition": { "type": "domain", "values": ["route.test"] },
            "action": { "type": "use_path", "pathId": "secondary" },
            "source": "manual"
        }],
        "scenarios": []
    });
    let config = compiled_standard_config(intent)?;
    let registry = plugin::init(config).await?;

    let routed = execute_standard_main_query(
        registry.clone(),
        Ipv4Addr::LOCALHOST,
        query_for("route.test.", RecordType::A),
    )
    .await?;
    let routed_cached = execute_standard_main_query(
        registry.clone(),
        Ipv4Addr::LOCALHOST,
        query_for("route.test.", RecordType::A),
    )
    .await?;
    assert_eq!(
        answer_ip(&routed),
        Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 80)))
    );
    assert_eq!(answer_ip(&routed_cached), answer_ip(&routed));
    assert_eq!(secondary_count.load(Ordering::SeqCst), 1);

    let blocked = execute_standard_main_query(
        registry.clone(),
        Ipv4Addr::LOCALHOST,
        query_for("ads.test.", RecordType::A),
    )
    .await?;
    assert_eq!(blocked.rcode(), Rcode::NXDomain);
    assert_eq!(primary_count.load(Ordering::SeqCst), 0);

    let default = execute_standard_main_query(
        registry.clone(),
        Ipv4Addr::LOCALHOST,
        query_for("ordinary.test.", RecordType::A),
    )
    .await?;
    assert_eq!(
        answer_ip(&default),
        Some(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 70)))
    );
    assert_eq!(primary_count.load(Ordering::SeqCst), 1);

    registry.destroy().await;
    primary_task.abort();
    secondary_task.abort();
    Ok(())
}

#[tokio::test]
async fn standard_mode_dedicated_group_routes_main_and_native_listener_queries() -> Result<()> {
    let (primary_addr, _primary_count, primary_task) =
        start_mock_upstream(Ipv4Addr::new(192, 0, 2, 90)).await?;
    let (dedicated_addr, dedicated_count, dedicated_task) =
        start_mock_upstream(Ipv4Addr::new(198, 51, 100, 91)).await?;
    let listen = reserve_local_addr()?;
    let dedicated_listen = reserve_local_addr()?;
    let mut intent = standard_intent(listen, primary_addr, None);
    intent["cache"]["enabled"] = json!(false);
    intent["dedicatedGroups"] = json!([{
        "id": "media",
        "name": "Media",
        "enabled": true,
        "priority": 10,
        "rules": ["domain:media.test"],
        "strategy": "balanced",
        "upstreams": [{
            "id": "media_upstream",
            "name": "Media upstream",
            "protocol": "udp",
            "address": dedicated_addr.to_string(),
            "enabled": true,
            "tlsVerify": true
        }],
        "path": { "cache": "disabled" },
        "listener": {
            "enabled": true,
            "address": dedicated_listen.to_string(),
            "udp": true,
            "tcp": true
        }
    }]);
    let registry = plugin::init(compiled_standard_config(intent)?).await?;

    let routed = exchange_udp_query(listen, query_for("video.media.test.", RecordType::A)).await?;
    assert_eq!(
        answer_ip(&routed),
        Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 91)))
    );
    let direct =
        exchange_tcp_query(dedicated_listen, query_for("anything.test.", RecordType::A)).await?;
    assert_eq!(answer_ip(&direct), answer_ip(&routed));
    assert_eq!(dedicated_count.load(Ordering::SeqCst), 2);

    registry.destroy().await;
    primary_task.abort();
    dedicated_task.abort();
    Ok(())
}

#[tokio::test]
async fn standard_mode_response_rule_reroutes_once_to_isolated_target() -> Result<()> {
    let (primary_addr, primary_count, primary_task) =
        start_policy_upstream(MockAnswer::ServFail).await?;
    let (secondary_addr, secondary_count, secondary_task) =
        start_mock_upstream(Ipv4Addr::new(203, 0, 113, 92)).await?;
    let listen = reserve_local_addr()?;
    let mut intent = standard_intent(listen, primary_addr, Some(secondary_addr));
    intent["devices"] = json!([]);
    intent["cache"]["enabled"] = json!(false);
    intent["advancedRules"] = json!([{
        "id": "retry_servfail",
        "name": "Retry SERVFAIL",
        "enabled": true,
        "priority": 10,
        "phase": "response",
        "conditions": [
            { "type": "source_path", "pathId": "default" },
            { "type": "rcode", "values": ["SERVFAIL"] }
        ],
        "action": { "type": "use_path", "pathId": "secondary" },
        "failurePolicy": "fail_open",
        "failureResponse": "servfail"
    }]);
    let registry = plugin::init(compiled_standard_config(intent)?).await?;

    let response = exchange_udp_query(listen, query_for("retry.test.", RecordType::A)).await?;
    assert_eq!(
        answer_ip(&response),
        Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 92)))
    );
    assert_eq!(primary_count.load(Ordering::SeqCst), 1);
    assert_eq!(secondary_count.load(Ordering::SeqCst), 1);

    registry.destroy().await;
    primary_task.abort();
    secondary_task.abort();
    Ok(())
}

#[tokio::test]
async fn smart_routing_domestic_validation_has_deterministic_fallback_outcomes() -> Result<()> {
    let cases = [
        (MockAnswer::Address(Ipv4Addr::new(10, 1, 2, 3)), false),
        (MockAnswer::Address(Ipv4Addr::new(192, 0, 2, 9)), true),
        (MockAnswer::Nodata, true),
        (MockAnswer::NxDomain, true),
        (MockAnswer::ServFail, true),
        (MockAnswer::CnameOnly, true),
        (MockAnswer::NoResponse, true),
    ];

    for (index, (domestic_answer, should_fallback)) in cases.into_iter().enumerate() {
        let (domestic_addr, domestic_count, domestic_task) =
            start_policy_upstream(domestic_answer).await?;
        let remote_ip = Ipv4Addr::new(198, 51, 100, 90);
        let (remote_addr, remote_count, remote_task) =
            start_policy_upstream(MockAnswer::Address(remote_ip)).await?;
        let listen = reserve_local_addr()?;
        let config = compiled_standard_config(smart_standard_intent(
            listen,
            domestic_addr,
            remote_addr,
            "compatibility_first",
        ))?;
        let registry = plugin::init(config).await?;

        for (transport, response) in [
            (
                "udp",
                exchange_udp_query(
                    listen,
                    query_for(
                        &format!("udp-case-{index}.domestic.smart.test."),
                        RecordType::A,
                    ),
                )
                .await?,
            ),
            (
                "tcp",
                exchange_tcp_query(
                    listen,
                    query_for(
                        &format!("tcp-case-{index}.domestic.smart.test."),
                        RecordType::A,
                    ),
                )
                .await?,
            ),
        ] {
            if should_fallback {
                assert_eq!(
                    answer_ip(&response),
                    Some(IpAddr::V4(remote_ip)),
                    "unexpected {transport} fallback response"
                );
            } else {
                assert_eq!(
                    answer_ip(&response),
                    Some(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))),
                    "unexpected {transport} domestic response"
                );
            }
        }
        assert_eq!(domestic_count.load(Ordering::SeqCst), 2);
        assert_eq!(
            remote_count.load(Ordering::SeqCst),
            if should_fallback { 2 } else { 0 }
        );

        registry.destroy().await;
        domestic_task.abort();
        remote_task.abort();
    }
    Ok(())
}

#[tokio::test]
async fn smart_routing_unknown_modes_select_the_expected_initial_path() -> Result<()> {
    let cases = [
        ("compatibility_first", Ipv4Addr::new(10, 0, 0, 8), 1, 0),
        ("privacy_first", Ipv4Addr::new(203, 0, 113, 88), 0, 1),
        ("strict_remote", Ipv4Addr::new(203, 0, 113, 88), 0, 1),
    ];

    for (mode, expected_ip, expected_domestic, expected_remote) in cases {
        let (domestic_addr, domestic_count, domestic_task) =
            start_policy_upstream(MockAnswer::Address(Ipv4Addr::new(10, 0, 0, 8))).await?;
        let (remote_addr, remote_count, remote_task) =
            start_policy_upstream(MockAnswer::Address(Ipv4Addr::new(203, 0, 113, 88))).await?;
        let listen = reserve_local_addr()?;
        let config = compiled_standard_config(smart_standard_intent(
            listen,
            domestic_addr,
            remote_addr,
            mode,
        ))?;
        let registry = plugin::init(config).await?;

        let udp_response = exchange_udp_query(
            listen,
            query_for(&format!("udp.unknown.{mode}.test."), RecordType::A),
        )
        .await?;
        let tcp_response = exchange_tcp_query(
            listen,
            query_for(&format!("tcp.unknown.{mode}.test."), RecordType::A),
        )
        .await?;
        assert_eq!(answer_ip(&udp_response), Some(IpAddr::V4(expected_ip)));
        assert_eq!(answer_ip(&tcp_response), Some(IpAddr::V4(expected_ip)));
        assert_eq!(domestic_count.load(Ordering::SeqCst), expected_domestic * 2);
        assert_eq!(remote_count.load(Ordering::SeqCst), expected_remote * 2);

        registry.destroy().await;
        domestic_task.abort();
        remote_task.abort();
    }
    Ok(())
}

#[tokio::test]
async fn smart_routing_non_address_answer_skips_ip_geography_validation() -> Result<()> {
    let (domestic_addr, domestic_count, domestic_task) =
        start_policy_upstream(MockAnswer::Text).await?;
    let (remote_addr, remote_count, remote_task) =
        start_policy_upstream(MockAnswer::Address(Ipv4Addr::new(198, 51, 100, 92))).await?;
    let listen = reserve_local_addr()?;
    let config = compiled_standard_config(smart_standard_intent(
        listen,
        domestic_addr,
        remote_addr,
        "compatibility_first",
    ))?;
    let registry = plugin::init(config).await?;

    for response in [
        exchange_udp_query(
            listen,
            query_for("udp.txt.domestic.smart.test.", RecordType::TXT),
        )
        .await?,
        exchange_tcp_query(
            listen,
            query_for("tcp.txt.domestic.smart.test.", RecordType::TXT),
        )
        .await?,
    ] {
        assert!(
            response
                .answers()
                .iter()
                .any(|record| record.rr_type() == RecordType::TXT)
        );
    }
    assert_eq!(domestic_count.load(Ordering::SeqCst), 2);
    assert_eq!(remote_count.load(Ordering::SeqCst), 0);

    registry.destroy().await;
    domestic_task.abort();
    remote_task.abort();
    Ok(())
}

#[tokio::test]
async fn privacy_first_unknown_failure_uses_only_explicit_domestic_fallback() -> Result<()> {
    let (domestic_addr, domestic_count, domestic_task) =
        start_policy_upstream(MockAnswer::Address(Ipv4Addr::new(10, 0, 0, 9))).await?;
    let (remote_addr, remote_count, remote_task) =
        start_policy_upstream(MockAnswer::NoResponse).await?;
    let listen = reserve_local_addr()?;
    let mut intent = smart_standard_intent(listen, domestic_addr, remote_addr, "privacy_first");
    intent["smartRouting"]["privacyFallbackToDomestic"] = json!(true);
    let config = compiled_standard_config(intent)?;
    let registry = plugin::init(config).await?;

    let response = execute_standard_main_query(
        registry.clone(),
        Ipv4Addr::LOCALHOST,
        query_for("unknown.privacy-fallback.test.", RecordType::A),
    )
    .await?;
    assert_eq!(
        answer_ip(&response),
        Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9)))
    );
    assert_eq!(domestic_count.load(Ordering::SeqCst), 1);
    assert_eq!(remote_count.load(Ordering::SeqCst), 1);

    registry.destroy().await;
    domestic_task.abort();
    remote_task.abort();
    Ok(())
}

#[tokio::test]
async fn strict_remote_unknown_failure_never_executes_domestic_or_default_upstream() -> Result<()> {
    let (domestic_addr, domestic_count, domestic_task) =
        start_policy_upstream(MockAnswer::Address(Ipv4Addr::new(10, 0, 0, 10))).await?;
    let (remote_addr, remote_count, remote_task) =
        start_policy_upstream(MockAnswer::NoResponse).await?;
    let listen = reserve_local_addr()?;
    let mut intent = smart_standard_intent(listen, domestic_addr, remote_addr, "strict_remote");
    intent["upstreamGroups"][1]["upstreams"][0]["timeoutSeconds"] = json!(1);
    let config = compiled_standard_config(intent)?;
    let registry = plugin::init(config).await?;
    let executor = registry
        .get_plugin("standard_main_sequence")
        .expect("compiled Standard main sequence")
        .to_executor();
    let mut context = DnsContext::new(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)),
        query_for("unknown.strict-failure.test.", RecordType::A),
    );

    let _ = timeout(Duration::from_secs(2), executor.execute(&mut context))
        .await
        .map_err(|_| DnsError::runtime("strict-remote failure did not terminate"))?;
    assert!(context.response().is_none());
    assert_eq!(domestic_count.load(Ordering::SeqCst), 0);
    assert_eq!(remote_count.load(Ordering::SeqCst), 1);

    registry.destroy().await;
    domestic_task.abort();
    remote_task.abort();
    Ok(())
}

#[tokio::test]
async fn smart_fallback_execution_path_explains_initial_failure_and_final_branch() -> Result<()> {
    let (domestic_addr, _domestic_count, domestic_task) =
        start_policy_upstream(MockAnswer::Address(Ipv4Addr::new(192, 0, 2, 9))).await?;
    let (remote_addr, _remote_count, remote_task) =
        start_policy_upstream(MockAnswer::Address(Ipv4Addr::new(198, 51, 100, 91))).await?;
    let listen = reserve_local_addr()?;
    let config = compiled_standard_config(smart_standard_intent(
        listen,
        domestic_addr,
        remote_addr,
        "compatibility_first",
    ))?;
    let registry = plugin::init(config).await?;
    let executor = registry
        .get_plugin("standard_main_sequence")
        .expect("compiled Standard main sequence")
        .to_executor();
    let mut context = DnsContext::new(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)),
        query_for("explain.domestic.smart.test.", RecordType::A),
    );
    context.enable_execution_path();

    executor.execute(&mut context).await?;

    let events = context.execution_path_events();
    assert!(
        events
            .iter()
            .any(|event| { event.kind == "decision" && event.outcome == "domestic_ip_mismatch" })
    );
    assert!(events.iter().any(|event| {
        event.kind == "fallback" && event.outcome == "secondary_domestic_ip_mismatch"
    }));
    assert!(events.iter().any(|event| {
        event.kind == "executor"
            && event.tag.as_deref() == Some("standard_path_smart_domestic_primary")
    }));
    assert!(events.iter().any(|event| {
        event.kind == "executor"
            && event.tag.as_deref() == Some("standard_path_smart_domestic_remote_fallback")
    }));

    registry.destroy().await;
    domestic_task.abort();
    remote_task.abort();
    Ok(())
}
