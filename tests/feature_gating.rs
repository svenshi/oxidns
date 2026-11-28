// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime behavior of the Cargo-feature gates.
//!
//! These tests assert the *observable* contract of a feature being compiled
//! out, independent of which protocol/plugin features the current build
//! happens to enable:
//!
//! - A config that uses a protocol whose feature is disabled fails to start
//!   with a clear "rebuild with --features ..." message (negative tests, gated
//!   on `#[cfg(not(feature = "..."))]`, so they run under `minimal` and any
//!   bundle that leaves the feature off).
//! - A config that uses a protocol whose feature is enabled assembles cleanly
//!   (positive tests, gated on `#[cfg(feature = "...")]`).
//!
//! The negative cases fail during `plugin::init` (or config validation for
//! unknown plugin types) before any runtime is installed, so they are safe to
//! run in parallel. Positive cases install a runtime and tear it back down via
//! `plugin::destroy_runtime`.

use oxidns::config::types::Config;
use oxidns::infra::clock::AppClock;

/// Parse + validate + initialize a config, returning the error string from
/// whichever stage rejects it. Panics if the config unexpectedly succeeds.
#[allow(dead_code)]
async fn start_error(yaml: &str) -> String {
    AppClock::start();
    #[cfg(debug_assertions)]
    oxidns::plugin::enable_runtime_test_serialization();

    let config: Config = serde_yaml_ng::from_str(yaml).expect("yaml should parse");
    // Unknown plugin types are rejected here; protocol-not-compiled errors
    // surface later during plugin initialization.
    if let Err(err) = config.validate() {
        return err.to_string();
    }
    oxidns::plugin::init(config)
        .await
        .expect_err("plugin init should fail for a not-compiled protocol")
        .to_string()
}

/// Parse + validate + initialize a config that is expected to succeed, then
/// tear the runtime back down.
#[allow(dead_code)]
async fn start_ok(yaml: &str) {
    AppClock::start();
    #[cfg(debug_assertions)]
    oxidns::plugin::enable_runtime_test_serialization();

    let config: Config = serde_yaml_ng::from_str(yaml).expect("yaml should parse");
    config.validate().expect("config should validate");
    oxidns::plugin::init(config)
        .await
        .expect("plugin init should succeed when the feature is enabled");
    oxidns::plugin::destroy_runtime().await;
}

/// A `forward` upstream + `udp_server` entry, so the forward executor is
/// actually initialized (unused plugins are skipped at runtime).
fn forward_via_udp(upstream_addr: &str) -> String {
    format!(
        r#"
plugins:
  - tag: forward_main
    type: forward
    args:
      upstreams:
        - addr: "{upstream_addr}"
  - tag: udp_main
    type: udp_server
    args:
      entry: forward_main
      listen: "127.0.0.1:0"
"#
    )
}

#[allow(dead_code)]
fn outbound_resolver(nameserver_addr: &str) -> String {
    format!(
        r#"
network:
  outbound:
    default: test
    profiles:
      test:
        resolver:
          nameservers:
            - addr: "{nameserver_addr}"
"#
    )
}

// --- Negative: protocol feature compiled out -------------------------------

#[cfg(not(feature = "resolver-dot"))]
#[tokio::test]
async fn resolver_dot_not_compiled_reports_rebuild_hint() {
    let err = start_error(&outbound_resolver("tls://1.1.1.1:853")).await;
    assert!(
        err.contains("resolver-dot"),
        "expected a resolver-dot rebuild hint, got: {err}"
    );
}

#[cfg(not(feature = "resolver-doh"))]
#[tokio::test]
async fn resolver_doh_not_compiled_reports_rebuild_hint() {
    let err = start_error(&outbound_resolver("https://1.1.1.1/dns-query")).await;
    assert!(
        err.contains("resolver-doh"),
        "expected a resolver-doh rebuild hint, got: {err}"
    );
}

#[cfg(not(feature = "resolver-doq"))]
#[tokio::test]
async fn resolver_doq_not_compiled_reports_rebuild_hint() {
    let err = start_error(&outbound_resolver("quic://1.1.1.1:853")).await;
    assert!(
        err.contains("resolver-doq"),
        "expected a resolver-doq rebuild hint, got: {err}"
    );
}

#[cfg(not(feature = "resolver-doh3"))]
#[tokio::test]
async fn resolver_doh3_not_compiled_reports_rebuild_hint() {
    let err = start_error(&outbound_resolver("h3://1.1.1.1/dns-query")).await;
    assert!(
        err.contains("resolver-doh3"),
        "expected a resolver-doh3 rebuild hint, got: {err}"
    );
}

#[cfg(not(feature = "upstream-dot"))]
#[tokio::test]
async fn upstream_dot_not_compiled_reports_rebuild_hint() {
    let err = start_error(&forward_via_udp("tls://1.1.1.1:853")).await;
    assert!(
        err.contains("upstream-dot"),
        "expected an upstream-dot rebuild hint, got: {err}"
    );
}

#[cfg(not(feature = "upstream-doh"))]
#[tokio::test]
async fn upstream_doh_not_compiled_reports_rebuild_hint() {
    let err = start_error(&forward_via_udp("https://1.1.1.1/dns-query")).await;
    assert!(
        err.contains("upstream-doh"),
        "expected an upstream-doh rebuild hint, got: {err}"
    );
}

#[cfg(not(feature = "upstream-doq"))]
#[tokio::test]
async fn upstream_doq_not_compiled_reports_rebuild_hint() {
    let err = start_error(&forward_via_udp("quic://1.1.1.1:853")).await;
    assert!(
        err.contains("upstream-doq"),
        "expected an upstream-doq rebuild hint, got: {err}"
    );
}

#[cfg(not(feature = "server-dot"))]
#[tokio::test]
async fn server_dot_cert_not_compiled_reports_rebuild_hint() {
    let yaml = r#"
plugins:
  - tag: forward_main
    type: forward
    args:
      upstreams:
        - addr: "8.8.8.8:53"
  - tag: tcp_main
    type: tcp_server
    args:
      entry: forward_main
      listen: "127.0.0.1:0"
      cert: /nonexistent/cert.pem
      key: /nonexistent/key.pem
"#;
    let err = start_error(yaml).await;
    assert!(
        err.contains("server-dot"),
        "expected a server-dot rebuild hint, got: {err}"
    );
}

#[cfg(not(feature = "server-doh"))]
#[tokio::test]
async fn server_doh_type_not_compiled_is_unknown_plugin() {
    let yaml = r#"
plugins:
  - tag: forward_main
    type: forward
    args:
      upstreams:
        - addr: "8.8.8.8:53"
  - tag: doh_main
    type: http_server
    args:
      entry: forward_main
      listen: "127.0.0.1:0"
"#;
    let err = start_error(yaml).await;
    assert!(
        err.contains("Unknown plugin type"),
        "expected an unknown-plugin-type error, got: {err}"
    );
}

#[cfg(not(feature = "server-doq"))]
#[tokio::test]
async fn server_doq_type_not_compiled_is_unknown_plugin() {
    let yaml = r#"
plugins:
  - tag: forward_main
    type: forward
    args:
      upstreams:
        - addr: "8.8.8.8:53"
  - tag: doq_main
    type: quic_server
    args:
      entry: forward_main
      listen: "127.0.0.1:0"
"#;
    let err = start_error(yaml).await;
    assert!(
        err.contains("Unknown plugin type"),
        "expected an unknown-plugin-type error, got: {err}"
    );
}

#[cfg(not(feature = "plugin-arbitrary"))]
#[tokio::test]
async fn arbitrary_type_not_compiled_is_unknown_plugin() {
    let yaml = r#"
plugins:
  - tag: arbitrary_main
    type: arbitrary
    args:
      rules:
        - "example.com. 60 IN A 192.0.2.10"
"#;
    let err = start_error(yaml).await;
    assert!(
        err.contains("Unknown plugin type"),
        "expected an unknown-plugin-type error, got: {err}"
    );
}

#[cfg(not(feature = "plugin-client-ip-from-ecs"))]
#[tokio::test]
async fn client_ip_from_ecs_type_not_compiled_is_unknown_plugin() {
    let yaml = r#"
plugins:
  - tag: ecs_client
    type: client_ip_from_ecs
"#;
    let err = start_error(yaml).await;
    assert!(
        err.contains("Unknown plugin type"),
        "expected an unknown-plugin-type error, got: {err}"
    );
}

#[cfg(not(feature = "plugin-response"))]
#[tokio::test]
async fn response_type_not_compiled_is_unknown_plugin() {
    let yaml = r#"
plugins:
  - tag: response_main
    type: response
    args:
      answers:
        - "example.com. 60 IN A 192.0.2.10"
"#;
    let err = start_error(yaml).await;
    assert!(
        err.contains("Unknown plugin type"),
        "expected an unknown-plugin-type error, got: {err}"
    );
}

// --- Positive: protocol feature compiled in --------------------------------

#[cfg(feature = "upstream-dot")]
#[tokio::test]
async fn upstream_dot_builds_when_compiled() {
    // Direct-IP upstreams build their pool lazily, so init succeeds offline.
    start_ok(&forward_via_udp("tls://1.1.1.1:853")).await;
}

#[cfg(feature = "upstream-doh")]
#[tokio::test]
async fn upstream_doh_builds_when_compiled() {
    start_ok(&forward_via_udp("https://1.1.1.1/dns-query")).await;
}

#[cfg(feature = "plugin-arbitrary")]
#[tokio::test]
async fn arbitrary_builds_when_compiled() {
    let yaml = r#"
plugins:
  - tag: arbitrary_main
    type: arbitrary
    args:
      rules:
        - "example.com. 60 IN A 192.0.2.10"
  - tag: entry
    type: sequence
    args:
      - exec: "$arbitrary_main"
  - tag: udp_main
    type: udp_server
    args:
      entry: entry
      listen: "127.0.0.1:0"
"#;
    start_ok(yaml).await;
}

#[cfg(feature = "plugin-client-ip-from-ecs")]
#[tokio::test]
async fn client_ip_from_ecs_builds_when_compiled() {
    let yaml = r#"
plugins:
  - tag: ecs_client
    type: client_ip_from_ecs
  - tag: entry
    type: sequence
    args:
      - exec: "$ecs_client"
  - tag: udp_main
    type: udp_server
    args:
      entry: entry
      listen: "127.0.0.1:0"
"#;
    start_ok(yaml).await;
}

#[cfg(feature = "plugin-response")]
#[tokio::test]
async fn response_builds_when_compiled() {
    let yaml = r#"
plugins:
  - tag: response_main
    type: response
    args:
      answers:
        - "{qname} 60 {qclass} A 192.0.2.10"
  - tag: entry
    type: sequence
    args:
      - exec: "$response_main"
  - tag: udp_main
    type: udp_server
    args:
      entry: entry
      listen: "127.0.0.1:0"
"#;
    start_ok(yaml).await;
}

const DHCP_DNS_SERVER_CONFIG: &str = r#"
plugins:
  - tag: forward_local
    type: dhcp_dns_servers
  - tag: udp_main
    type: udp_server
    args:
      entry: forward_local
      listen: "127.0.0.1:0"
"#;

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[tokio::test]
async fn dhcp_dns_servers_registers_as_executor_with_optional_args() {
    start_ok(DHCP_DNS_SERVER_CONFIG).await;
    let configured = DHCP_DNS_SERVER_CONFIG.replace(
        "    type: dhcp_dns_servers",
        "    type: dhcp_dns_servers\n    args:\n      concurrent: 1\n      response_selection: prefer_positive\n      short_circuit: true",
    );
    start_ok(&configured).await;
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[tokio::test]
async fn dhcp_dns_servers_rejects_static_upstreams() {
    let configured = DHCP_DNS_SERVER_CONFIG.replace(
        "    type: dhcp_dns_servers",
        "    type: dhcp_dns_servers\n    args:\n      upstreams: []",
    );
    let err = start_error(&configured).await;
    assert!(
        err.contains("unknown field") && err.contains("upstreams"),
        "{err}"
    );
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
#[tokio::test]
async fn dhcp_dns_servers_reports_unsupported_platform() {
    let err = start_error(DHCP_DNS_SERVER_CONFIG).await;
    assert!(err.contains("not supported on this platform"), "{err}");
}

#[tokio::test]
async fn forward_still_requires_static_upstreams() {
    let configured = DHCP_DNS_SERVER_CONFIG.replace(
        "    type: dhcp_dns_servers",
        "    type: forward\n    args:\n      default_resolver: true",
    );
    let err = start_error(&configured).await;
    assert!(err.contains("upstreams"), "{err}");
}
