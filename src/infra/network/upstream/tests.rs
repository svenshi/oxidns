// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;

use crate::config::types::{
    NetworkOutboundConfig, OutboundNameserverConfig, OutboundProfileConfig, OutboundProxyConfig,
    OutboundResolverConfig, OutboundResolverDetailedConfig,
};
use crate::infra::error::Result;
use crate::infra::network::outbound;
use crate::infra::network::proxy::{parse_socks5_opt, parse_socks5_opt_with_resolver};
use crate::infra::network::upstream::builder::{
    UpstreamBuilder, create_pipeline_pool, create_reuse_pool, main_pool_min_conns,
    udp_truncated_fallback_min_conns,
};
use crate::infra::network::upstream::config::{ConnectionInfo, ConnectionType, UpstreamConfig};
use crate::infra::network::upstream::pool::{
    Connection, ConnectionBuilder, ConnectionPool, QueryDeadline,
};
use crate::infra::network::upstream::resolver::{PooledUpstream, Upstream};
use crate::proto::Message;

#[derive(Debug)]
struct SlowUpstream {
    connection_info: ConnectionInfo,
}

#[async_trait]
impl Upstream for SlowUpstream {
    async fn inner_query(&self, request: Message, _deadline: QueryDeadline) -> Result<Message> {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(request)
    }

    fn connection_info(&self) -> &ConnectionInfo {
        &self.connection_info
    }
}

#[derive(Debug)]
struct NoopConnection {
    available: AtomicBool,
    using_count: AtomicU16,
    last_used: AtomicU64,
}

#[async_trait]
impl Connection for NoopConnection {
    fn close(&self) {
        self.available.store(false, Ordering::Relaxed);
    }

    async fn query(&self, request: Message, _deadline: QueryDeadline) -> Result<Message> {
        Ok(request)
    }

    fn using_count(&self) -> u16 {
        self.using_count.load(Ordering::Relaxed)
    }

    fn available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }

    fn last_used(&self) -> u64 {
        self.last_used.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
struct NoopConnectionBuilder;

#[async_trait]
impl ConnectionBuilder<NoopConnection> for NoopConnectionBuilder {
    async fn create_connection(
        &self,
        _conn_id: u16,
        _deadline: QueryDeadline,
    ) -> Result<Arc<NoopConnection>> {
        Ok(Arc::new(NoopConnection {
            available: AtomicBool::new(true),
            using_count: AtomicU16::new(0),
            last_used: AtomicU64::new(crate::infra::clock::AppClock::elapsed_millis()),
        }))
    }
}

#[derive(Debug)]
struct DeadlineHandlingPool {
    handled_timeout: Arc<AtomicBool>,
}

#[async_trait]
impl ConnectionPool<NoopConnection> for DeadlineHandlingPool {
    async fn query(&self, _request: Message, deadline: QueryDeadline) -> Result<Message> {
        let Some(remaining) = deadline.remaining() else {
            self.handled_timeout.store(true, Ordering::Relaxed);
            return Err(deadline.timeout_error());
        };
        tokio::time::sleep(remaining + Duration::from_millis(20)).await;
        self.handled_timeout.store(true, Ordering::Relaxed);
        Err(deadline.timeout_error())
    }

    async fn maintain(&self) {}

    fn configured_min_size(&self) -> usize {
        0
    }
}

fn make_upstream_config(addr: &str) -> UpstreamConfig {
    UpstreamConfig {
        tag: None,
        addr: addr.to_string(),
        outbound: None,
        dial_addr: None,
        port: None,
        bootstrap: None,
        bootstrap_version: None,
        socks5: None,
        idle_timeout: None,
        max_conns: None,
        min_conns: None,
        insecure_skip_verify: None,
        timeout: None,
        enable_pipeline: None,
        enable_http3: None,
        so_mark: None,
        bind_to_device: None,
    }
}

fn outbound_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn install_test_outbound_config() {
    let config = NetworkOutboundConfig {
        default: None,
        profiles: HashMap::from([(
            "oversea".to_string(),
            OutboundProfileConfig {
                resolver: Some(OutboundResolverConfig::Nameservers(
                    OutboundResolverDetailedConfig {
                        nameservers: vec![OutboundNameserverConfig {
                            addr: "1.1.1.1:53".to_string(),
                            dial_addr: None,
                        }],
                        ip_version: Some(4),
                        timeout: Some("500ms".to_string()),
                        proxy: None,
                    },
                )),
                proxy: Some(OutboundProxyConfig::Socks5 {
                    socks5: "127.0.0.1:1080".to_string(),
                }),
            },
        )]),
    };
    outbound::install_global(&config).expect("outbound config should install");
}

fn install_test_outbound_resolver_only_config() {
    let config = NetworkOutboundConfig {
        default: None,
        profiles: HashMap::from([(
            "oversea".to_string(),
            OutboundProfileConfig {
                resolver: Some(OutboundResolverConfig::Nameservers(
                    OutboundResolverDetailedConfig {
                        nameservers: vec![OutboundNameserverConfig {
                            addr: "1.1.1.1:53".to_string(),
                            dial_addr: None,
                        }],
                        ip_version: Some(4),
                        timeout: Some("500ms".to_string()),
                        proxy: None,
                    },
                )),
                proxy: None,
            },
        )]),
    };
    outbound::install_global(&config).expect("outbound config should install");
}

#[test]
fn test_helper_scheme_tcp_pipeline_forces_pipeline() {
    let mut cfg = make_upstream_config("tcp+pipeline://1.1.1.1");
    cfg.enable_pipeline = Some(false);
    let info = ConnectionInfo::try_from(cfg).expect("helper scheme should be accepted");
    assert_eq!(info.connection_type, ConnectionType::TCP);
    assert_eq!(info.enable_pipeline, Some(true));
}

#[test]
fn test_helper_scheme_h3_forces_http3() {
    let mut cfg = make_upstream_config("h3://dns.google/dns-query");
    cfg.enable_http3 = Some(false);
    let info = ConnectionInfo::try_from(cfg).expect("helper scheme should be accepted");
    assert_eq!(info.connection_type, ConnectionType::DoH);
    assert!(info.enable_http3);
}

#[test]
fn test_connection_info_defers_domain_resolution() {
    let info = ConnectionInfo::with_addr("tls://dns.example.invalid:853")
        .expect("domain upstream should parse without DNS resolution");
    assert_eq!(info.server_name, "dns.example.invalid");
    assert!(info.remote_ip.is_none());

    let info = ConnectionInfo::try_from(make_upstream_config(
        "https://resolver.example.invalid/dns-query",
    ))
    .expect("domain upstream config should parse without DNS resolution");
    assert_eq!(info.server_name, "resolver.example.invalid");
    assert!(info.remote_ip.is_none());
}

#[test]
fn test_connection_info_uses_dial_addr_for_domain() {
    let mut cfg = make_upstream_config("tls://dns.example.invalid:853");
    cfg.dial_addr = Some(IpAddr::from_str("203.0.113.53").unwrap());

    let info = ConnectionInfo::try_from(cfg).expect("upstream config should parse");
    assert_eq!(info.server_name, "dns.example.invalid");
    assert_eq!(
        info.remote_ip,
        Some(IpAddr::from_str("203.0.113.53").unwrap())
    );
}

#[test]
fn test_connection_info_dial_addr_takes_precedence_over_bootstrap() {
    let mut cfg = make_upstream_config("tls://dns.example.invalid:853");
    cfg.dial_addr = Some(IpAddr::from_str("203.0.113.53").unwrap());
    cfg.bootstrap = Some("8.8.8.8:53".to_string());

    let info = ConnectionInfo::try_from(cfg).expect("upstream config should parse");
    assert_eq!(
        info.remote_ip,
        Some(IpAddr::from_str("203.0.113.53").unwrap())
    );
    assert!(info.bootstrap.is_none());
}

#[test]
fn test_connection_info_uses_outbound_resolver_for_domain() {
    let _guard = outbound_test_lock()
        .lock()
        .expect("outbound test lock should not be poisoned");
    install_test_outbound_resolver_only_config();

    let mut cfg = make_upstream_config("tls://dns.example.invalid:853");
    cfg.outbound = Some("oversea".to_string());
    let info = ConnectionInfo::try_from(cfg).expect("upstream config should parse");

    assert!(info.remote_ip.is_none());
    assert!(info.bootstrap.is_some());
    assert_eq!(info.bootstrap_timeout, Some(Duration::from_millis(500)));
    outbound::clear_global();
}

#[tokio::test]
async fn test_udp_upstream_with_outbound_resolver_keeps_truncated_fallback() {
    let _guard = outbound_test_lock()
        .lock()
        .expect("outbound test lock should not be poisoned");
    install_test_outbound_resolver_only_config();

    let mut cfg = make_upstream_config("udp://dns.example.invalid:53");
    cfg.outbound = Some("oversea".to_string());
    let info = ConnectionInfo::try_from(cfg).expect("upstream config should parse");
    let upstream = UpstreamBuilder::with_connection_info(info).expect("upstream should build");

    assert!(
        format!("{upstream:?}").contains("BootstrapUdpTruncatedUpstream"),
        "unexpected upstream: {upstream:?}"
    );
    outbound::clear_global();
}

#[test]
fn test_connection_info_dial_addr_takes_precedence_over_outbound_resolver() {
    let _guard = outbound_test_lock()
        .lock()
        .expect("outbound test lock should not be poisoned");
    install_test_outbound_config();

    let mut cfg = make_upstream_config("tls://dns.example.invalid:853");
    cfg.outbound = Some("oversea".to_string());
    cfg.dial_addr = Some(IpAddr::from_str("203.0.113.53").unwrap());
    let info = ConnectionInfo::try_from(cfg).expect("upstream config should parse");

    assert_eq!(
        info.remote_ip,
        Some(IpAddr::from_str("203.0.113.53").unwrap())
    );
    assert!(info.bootstrap.is_none());
    outbound::clear_global();
}

#[test]
fn test_connection_info_uses_outbound_proxy_when_local_socks5_absent() {
    let _guard = outbound_test_lock()
        .lock()
        .expect("outbound test lock should not be poisoned");
    install_test_outbound_config();

    let mut cfg = make_upstream_config("tcp://1.1.1.1:53");
    cfg.outbound = Some("oversea".to_string());
    let info = ConnectionInfo::try_from(cfg).expect("upstream config should parse");

    assert_eq!(
        info.socks5
            .as_ref()
            .expect("outbound proxy should be injected")
            .socket_addr
            .port(),
        1080
    );
    outbound::clear_global();
}

#[test]
fn test_connection_info_rejects_outbound_proxy_for_udp_upstream() {
    let _guard = outbound_test_lock()
        .lock()
        .expect("outbound test lock should not be poisoned");
    install_test_outbound_config();

    let mut cfg = make_upstream_config("8.8.8.8");
    cfg.outbound = Some("oversea".to_string());
    let err = ConnectionInfo::try_from(cfg).expect_err("UDP upstream should reject profile proxy");

    assert!(err.to_string().contains("does not support UDP"), "{err}");
    outbound::clear_global();
}

#[test]
fn test_connection_info_local_socks5_overrides_outbound_proxy() {
    let _guard = outbound_test_lock()
        .lock()
        .expect("outbound test lock should not be poisoned");
    install_test_outbound_config();

    let mut cfg = make_upstream_config("tcp://1.1.1.1:53");
    cfg.outbound = Some("oversea".to_string());
    cfg.socks5 = Some("127.0.0.1:1081".to_string());
    let info = ConnectionInfo::try_from(cfg).expect("upstream config should parse");

    assert_eq!(
        info.socks5
            .as_ref()
            .expect("local proxy should be retained")
            .socket_addr
            .port(),
        1081
    );
    outbound::clear_global();
}

#[test]
fn test_connection_info_rejects_invalid_local_socks5_with_outbound_proxy() {
    let _guard = outbound_test_lock()
        .lock()
        .expect("outbound test lock should not be poisoned");
    install_test_outbound_config();

    let mut cfg = make_upstream_config("tcp://1.1.1.1:53");
    cfg.outbound = Some("oversea".to_string());
    cfg.socks5 = Some("127.0.0.1".to_string());
    let err = ConnectionInfo::try_from(cfg).expect_err("malformed local proxy should fail");

    assert!(err.to_string().contains("invalid socks5 proxy"), "{err}");
    outbound::clear_global();
}

#[test]
fn test_connection_info_rejects_invalid_bootstrap_version() {
    let mut cfg = make_upstream_config("tls://dns.example.invalid:853");
    cfg.bootstrap = Some("8.8.8.8:53".to_string());
    cfg.bootstrap_version = Some(5);

    let err = ConnectionInfo::try_from(cfg).expect_err("invalid bootstrap_version should fail");

    assert!(
        err.to_string().contains("bootstrap_version must be 4 or 6"),
        "{err}"
    );
}

#[test]
fn test_max_conns_is_preserved() {
    let mut cfg = make_upstream_config("8.8.8.8");
    cfg.max_conns = Some(999);
    let info = ConnectionInfo::try_from(cfg).expect("upstream config should parse");
    assert_eq!(info.max_conns, Some(999));
}

#[test]
fn test_max_conns_rejects_zero() {
    let mut cfg = make_upstream_config("8.8.8.8");
    cfg.max_conns = Some(0);

    let err = ConnectionInfo::try_from(cfg).expect_err("zero max_conns should be rejected");

    assert!(
        err.to_string().contains("max_conns must be greater than 0"),
        "{err}"
    );
}

#[test]
fn test_max_conns_rejects_excessive_value() {
    let mut cfg = make_upstream_config("8.8.8.8");
    cfg.max_conns = Some(ConnectionInfo::MAX_CONFIGURED_CONNS_SIZE + 1);

    let err = ConnectionInfo::try_from(cfg).expect_err("excessive max_conns should be rejected");

    assert!(
        err.to_string().contains("max_conns must be <= 4096"),
        "{err}"
    );
}

#[test]
fn test_min_conns_is_preserved() {
    let mut cfg = make_upstream_config("8.8.8.8");
    cfg.min_conns = Some(3);

    let info = ConnectionInfo::try_from(cfg).expect("upstream config should parse");

    assert_eq!(info.min_conns, Some(3));
}

#[test]
fn test_min_conns_allows_zero() {
    let mut cfg = make_upstream_config("8.8.8.8");
    cfg.min_conns = Some(0);

    let info = ConnectionInfo::try_from(cfg).expect("zero min_conns should be accepted");

    assert_eq!(info.min_conns, Some(0));
    assert_eq!(info.min_conns_or_default(), 0);
}

#[test]
fn test_min_conns_rejects_excessive_value() {
    let mut cfg = make_upstream_config("8.8.8.8");
    cfg.max_conns = Some(ConnectionInfo::MAX_CONFIGURED_CONNS_SIZE);
    cfg.min_conns = Some(ConnectionInfo::MAX_CONFIGURED_CONNS_SIZE + 1);

    let err = ConnectionInfo::try_from(cfg).expect_err("excessive min_conns should be rejected");

    assert!(
        err.to_string().contains("min_conns must be <= 4096"),
        "{err}"
    );
}

#[test]
fn test_min_conns_rejects_value_above_configured_max_conns() {
    let mut cfg = make_upstream_config("8.8.8.8");
    cfg.max_conns = Some(2);
    cfg.min_conns = Some(3);

    let err = ConnectionInfo::try_from(cfg)
        .expect_err("min_conns above configured max_conns should be rejected");

    assert!(
        err.to_string().contains("min_conns must be <= max_conns"),
        "{err}"
    );
}

#[test]
fn test_min_conns_rejects_value_above_default_max_conns() {
    let mut cfg = make_upstream_config("8.8.8.8");
    cfg.min_conns = Some(ConnectionInfo::DEFAULT_MAX_CONNS_SIZE + 1);

    let err = ConnectionInfo::try_from(cfg)
        .expect_err("min_conns above default max_conns should be rejected");

    assert!(err.to_string().contains("effective max_conns: 64"), "{err}");
}

#[tokio::test]
async fn test_pipeline_pool_uses_configured_min_conns() {
    let mut info = ConnectionInfo::with_addr("tcp://127.0.0.1").expect("upstream should parse");
    info.max_conns = Some(4);
    info.min_conns = Some(2);

    let upstream = create_pipeline_pool::<NoopConnection>(info, Box::new(NoopConnectionBuilder));

    assert_eq!(upstream.pool.configured_min_size(), 2);
}

#[tokio::test]
async fn test_reuse_pool_uses_configured_min_conns() {
    let mut info = ConnectionInfo::with_addr("tcp://127.0.0.1").expect("upstream should parse");
    info.max_conns = Some(4);
    info.min_conns = Some(2);

    let upstream = create_reuse_pool::<NoopConnection>(info, Box::new(NoopConnectionBuilder));

    assert_eq!(upstream.pool.configured_min_size(), 2);
}

#[test]
fn test_udp_truncated_fallback_keeps_zero_min_conns() {
    let mut info = ConnectionInfo::with_addr("udp://127.0.0.1").expect("upstream should parse");
    info.min_conns = Some(2);

    assert_eq!(main_pool_min_conns(&info), 2);
    assert_eq!(udp_truncated_fallback_min_conns(), 0);
}

#[tokio::test]
async fn test_query_wraps_custom_upstream_in_deadline() {
    crate::infra::clock::AppClock::start();
    let mut connection_info =
        ConnectionInfo::with_addr("udp://127.0.0.1").expect("upstream should parse");
    connection_info.timeout = Duration::from_millis(10);
    let upstream = SlowUpstream { connection_info };

    let result = upstream.query(Message::new()).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_pooled_upstream_lets_pool_handle_deadline_expiry() {
    crate::infra::clock::AppClock::start();
    let handled_timeout = Arc::new(AtomicBool::new(false));
    let mut connection_info =
        ConnectionInfo::with_addr("tcp://127.0.0.1").expect("upstream should parse");
    connection_info.timeout = Duration::from_millis(10);
    let upstream = PooledUpstream::<NoopConnection> {
        connection_info,
        pool: Arc::new(DeadlineHandlingPool {
            handled_timeout: handled_timeout.clone(),
        }),
    };

    let result = upstream.query(Message::new()).await;

    assert!(result.is_err());
    assert!(handled_timeout.load(Ordering::Relaxed));
}

#[test]
fn test_parse_socks5_opt_ip_without_auth() {
    // Test parsing IP address without authentication
    let result = parse_socks5_opt("127.0.0.1:1080");
    assert!(result.is_some());

    let opt = result.unwrap();
    assert!(opt.username.is_none());
    assert!(opt.password.is_none());
    assert_eq!(opt.socket_addr.ip(), IpAddr::from_str("127.0.0.1").unwrap());
    assert_eq!(opt.socket_addr.port(), 1080);
}

#[test]
fn test_parse_socks5_opt_ip_with_auth() {
    // Test parsing IP address with authentication
    let result = parse_socks5_opt("myuser:mypass@192.168.1.100:8080");
    assert!(result.is_some());

    let opt = result.unwrap();
    assert_eq!(opt.username, Some("myuser".to_string()));
    assert_eq!(opt.password, Some("mypass".to_string()));
    assert_eq!(
        opt.socket_addr.ip(),
        IpAddr::from_str("192.168.1.100").unwrap()
    );
    assert_eq!(opt.socket_addr.port(), 8080);
}

#[test]
fn test_parse_socks5_opt_ipv6_without_auth() {
    // Test parsing IPv6 address without authentication
    let result = parse_socks5_opt("[::1]:1080");
    assert!(result.is_some());

    let opt = result.unwrap();
    assert!(opt.username.is_none());
    assert!(opt.password.is_none());
    assert_eq!(opt.socket_addr.ip(), IpAddr::from_str("::1").unwrap());
    assert_eq!(opt.socket_addr.port(), 1080);
}

#[test]
fn test_parse_socks5_opt_ipv6_with_auth() {
    // Test parsing IPv6 address with authentication
    let result = parse_socks5_opt("user:pass@[2001:db8::1]:8080");
    assert!(result.is_some());

    let opt = result.unwrap();
    assert_eq!(opt.username, Some("user".to_string()));
    assert_eq!(opt.password, Some("pass".to_string()));
    assert_eq!(
        opt.socket_addr.ip(),
        IpAddr::from_str("2001:db8::1").unwrap()
    );
    assert_eq!(opt.socket_addr.port(), 8080);
}

#[test]
fn test_parse_socks5_opt_ipv6_full_address() {
    // Test parsing full IPv6 address
    let result = parse_socks5_opt("[fe80::1234:5678:90ab:cdef]:9050");
    assert!(result.is_some());

    let opt = result.unwrap();
    assert_eq!(
        opt.socket_addr.ip(),
        IpAddr::from_str("fe80::1234:5678:90ab:cdef").unwrap()
    );
    assert_eq!(opt.socket_addr.port(), 9050);
}

#[test]
fn test_parse_socks5_opt_ipv6_missing_bracket() {
    // Test IPv6 without brackets - this actually succeeds for simple cases like ::1
    // because rfind(':') correctly splits "::1:1080" into "::1" and "1080"
    // However, brackets are still RECOMMENDED for clarity and standards compliance
    let result = parse_socks5_opt("::1:1080");
    assert!(result.is_some());

    let opt = result.unwrap();
    assert_eq!(opt.socket_addr.ip(), IpAddr::from_str("::1").unwrap());
    assert_eq!(opt.socket_addr.port(), 1080);
}

#[test]
fn test_parse_socks5_opt_ipv6_missing_port() {
    // Test IPv6 with brackets but no port
    let result = parse_socks5_opt("[::1]");
    assert!(result.is_none());
}

#[test]
fn test_parse_socks5_opt_ipv6_unclosed_bracket() {
    // Test IPv6 with unclosed bracket
    let result = parse_socks5_opt("[::1:1080");
    assert!(result.is_none());
}

#[test]
fn test_parse_socks5_opt_invalid_port() {
    // Test invalid port number
    let result = parse_socks5_opt("127.0.0.1:invalid");
    assert!(result.is_none());
}

#[test]
fn test_parse_socks5_opt_missing_port() {
    // Test missing port
    let result = parse_socks5_opt("127.0.0.1");
    assert!(result.is_none());
}

#[test]
fn test_parse_socks5_opt_invalid_auth_format() {
    // Test invalid auth format (missing password)
    let result = parse_socks5_opt("myuser@127.0.0.1:1080");
    assert!(result.is_none());
}

#[test]
fn test_parse_socks5_opt_password_with_colon() {
    // Test password containing colon
    let result = parse_socks5_opt("user:pass:word@127.0.0.1:1080");
    assert!(result.is_some());

    let opt = result.unwrap();
    assert_eq!(opt.username, Some("user".to_string()));
    assert_eq!(opt.password, Some("pass:word".to_string()));
    assert_eq!(opt.socket_addr.port(), 1080);
}

#[test]
fn test_parse_socks5_opt_hostname_uses_resolver() {
    let result = parse_socks5_opt_with_resolver("localhost:1080", |host| {
        assert_eq!(host, "localhost");
        Ok(IpAddr::from_str("127.0.0.1").unwrap())
    });
    assert!(result.is_some());

    let opt = result.unwrap();
    assert!(opt.username.is_none());
    assert!(opt.password.is_none());
    assert_eq!(opt.socket_addr.port(), 1080);
    assert_eq!(opt.socket_addr.ip(), IpAddr::from_str("127.0.0.1").unwrap());
}
