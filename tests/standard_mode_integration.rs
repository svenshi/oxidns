// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

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
use oxidns::proto::{DNSClass, Message, Name, Question, Rcode, RecordType};
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
                "ipSelection": "inherit",
                "ecs": "inherit"
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
    let stream = TcpStream::connect(server).await?;
    let (reader, writer) = stream.into_split();
    let mut reader = TcpTransportReader::new(reader);
    let mut writer = TcpTransportWriter::new(writer);
    writer.write_message(&query()).await?;
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
