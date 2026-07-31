// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::json;
use serde_yaml_ng::Value as YamlValue;

use super::*;

#[test]
fn default_intent_compiles_deterministically_with_path_scoped_cache() {
    let intent = StandardIntent::default();
    let capabilities = StandardCapabilities::for_tests();
    let first = compile_standard_intent(intent.clone(), &capabilities, None, None);
    let second = compile_standard_intent(intent, &capabilities, None, None);

    assert!(first.can_apply, "diagnostics: {:?}", first.diagnostics);
    assert_eq!(first.generated, second.generated);
    let generated = first.generated.expect("default intent should compile");
    assert_eq!(
        generated.tag_map.caches.get("default").map(String::as_str),
        Some("standard_cache_default")
    );
    assert!(generated.yaml.contains("min_positive_ttl: 60"));
    assert!(generated.yaml.contains("max_positive_ttl: 86400"));
    assert!(generated.yaml.contains("max_negative_ttl: 300"));
    assert!(generated.yaml.contains("negative_ttl_without_soa: 300"));
    assert!(!generated.yaml.contains("min_ttl:"));
    assert!(!generated.yaml.contains("max_ttl:"));
    crate::config::validate_text(&generated.yaml)
        .expect("generated default configuration should pass backend analysis");
}

#[test]
fn schema_v2_migrates_cache_and_upstream_strategy() {
    let value = json!({
        "schema": 2,
        "listen": { "address": "127.0.0.1:5533", "udp": true, "tcp": false },
        "upstreamGroups": [{
            "id": "default",
            "name": "Default",
            "strategy": "parallel",
            "isDefault": true,
            "upstreams": [{
                "id": "upstream",
                "name": "Upstream",
                "protocol": "udp",
                "address": "127.0.0.1:5353",
                "enabled": true
            }]
        }],
        "paths": [{
            "id": "default",
            "name": "Default",
            "upstreamGroupId": "default",
            "filtering": "inherit",
            "cache": "inherit",
            "queryLog": "inherit",
            "dualStack": "inherit",
            "ipSelection": "inherit",
            "ecs": "inherit"
        }],
        "filtering": { "enabled": false },
        "cache": {
            "enabled": true,
            "size": 1024,
            "minTtl": 10,
            "maxTtl": 600,
            "negativeTtl": 30
        },
        "queryLog": { "enabled": false, "retentionDays": 7, "sampleRate": 1 },
        "routing": { "enabled": false, "rules": [], "scenarios": [] },
        "exceptions": [],
        "devices": [],
        "system": { "logLevel": "info", "threads": 2 }
    });
    let (intent, migration) = decode_standard_intent(value).expect("v2 should migrate");

    assert_eq!(intent.schema, CURRENT_STANDARD_SCHEMA);
    assert_eq!(intent.cache.min_positive_ttl, 10);
    assert_eq!(intent.cache.max_positive_ttl, 600);
    assert_eq!(intent.cache.max_negative_ttl, 30);
    assert_eq!(intent.cache.negative_ttl_without_soa, 30);
    assert_eq!(migration.expect("migration metadata").from_schema, 2);

    let plan = compile_standard_intent(
        intent,
        &StandardCapabilities::for_tests(),
        Some("api:\n  http: 127.0.0.1:9080\nnetwork: {}\nplugins: []\n"),
        None,
    );
    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let yaml = plan.generated.expect("generated config").yaml;
    assert!(yaml.contains("worker_threads: 2"));
    assert!(!yaml.contains("\n  threads:"));
    let parsed: YamlValue = serde_yaml_ng::from_str(&yaml).expect("generated YAML parses");
    assert!(parsed.get("api").is_some());
    assert!(parsed.get("network").is_some());
}

#[test]
fn schema_v1_reports_the_complete_migration_range() {
    let (intent, migration) = decode_standard_intent(json!({
        "schema": 1,
        "listen": { "address": "127.0.0.1:5533", "udp": true, "tcp": true },
        "upstreams": [{
            "id": "local",
            "name": "Local",
            "address": "127.0.0.1:5353",
            "enabled": true
        }]
    }))
    .expect("v1 should migrate");

    assert_eq!(intent.schema, CURRENT_STANDARD_SCHEMA);
    let migration = migration.expect("migration metadata");
    assert_eq!(migration.from_schema, 1);
    assert_eq!(migration.to_schema, CURRENT_STANDARD_SCHEMA);
    assert!(
        migration
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "schema_v1_migrated")
    );
}

#[test]
fn schema_v3_migrates_with_inactive_phase_one_defaults() {
    let value = serde_json::to_value(StandardIntent::default()).expect("serialize default");
    let mut value = value.as_object().cloned().expect("intent object");
    value.insert("schema".to_string(), json!(3));
    value.remove("local");
    value
        .get_mut("filtering")
        .and_then(serde_json::Value::as_object_mut)
        .expect("filtering object")
        .remove("localFiles");

    let (intent, migration) =
        decode_standard_intent(serde_json::Value::Object(value)).expect("v3 should migrate");

    assert_eq!(intent.schema, CURRENT_STANDARD_SCHEMA);
    assert_eq!(intent.local, super::model::StandardLocalSettings::default());
    assert!(intent.filtering.local_files.is_empty());
    let migration = migration.expect("migration metadata");
    assert_eq!(migration.from_schema, 3);
    assert_eq!(migration.to_schema, CURRENT_STANDARD_SCHEMA);
}

#[test]
fn upstream_native_connection_fields_compile_without_platform_side_effects() {
    let mut intent = StandardIntent::default();
    let upstream = &mut intent.upstream_groups[0].upstreams[0];
    upstream.protocol = super::model::StandardUpstreamProtocol::Tcp;
    upstream.bootstrap = Some("1.1.1.1:53".to_string());
    upstream.bootstrap_version = Some(6);
    upstream.outbound = Some("private".to_string());
    upstream.socks5 = Some("127.0.0.1:1080".to_string());
    upstream.timeout_seconds = Some(7);
    upstream.idle_timeout_seconds = Some(30);
    upstream.max_conns = Some(32);
    upstream.min_conns = Some(2);
    upstream.enable_pipeline = true;

    let plan = compile_standard_intent(
        intent,
        &StandardCapabilities::for_tests(),
        Some(
            "network:\n  outbound:\n    profiles:\n      private:\n        resolver: system\nplugins: []\n",
        ),
        None,
    );

    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let yaml = plan.generated.expect("generated config").yaml;
    for expected in [
        "bootstrap_version: 6",
        "outbound: private",
        "socks5: 127.0.0.1:1080",
        "timeout: 7",
        "idle_timeout: 30",
        "max_conns: 32",
        "min_conns: 2",
        "enable_pipeline: true",
    ] {
        assert!(yaml.contains(expected), "missing {expected} in {yaml}");
    }
    assert!(!yaml.contains("so_mark"));
    assert!(!yaml.contains("bind_to_device"));
}

#[test]
fn subscriptions_compile_to_independent_failure_stopping_jobs() {
    let mut intent = StandardIntent::default();
    intent.filtering.enabled = true;
    intent.filtering.subscriptions = vec![
        super::model::StandardSubscription {
            id: "one".to_string(),
            name: "One".to_string(),
            url: "https://example.com/one.txt".to_string(),
            enabled: true,
            update_interval_hours: 4,
        },
        super::model::StandardSubscription {
            id: "two".to_string(),
            name: "Two".to_string(),
            url: "https://example.com/two.txt".to_string(),
            enabled: true,
            update_interval_hours: 12,
        },
    ];

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);

    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.expect("generated config");
    assert_eq!(generated.tag_map.filter_subscriptions.len(), 2);
    let one = generated
        .tag_map
        .filter_subscriptions
        .get("one")
        .expect("one tags");
    let two = generated
        .tag_map
        .filter_subscriptions
        .get("two")
        .expect("two tags");
    assert_ne!(one.download, two.download);
    assert_ne!(one.cron, two.cron);
    assert!(generated.yaml.contains("fail_on_error: true"));
    assert!(generated.yaml.contains("stop_on_error: true"));
    assert!(generated.yaml.contains("interval: 4h"));
    assert!(generated.yaml.contains("interval: 12h"));
    assert_eq!(
        generated
            .yaml
            .matches("url: https://example.com/one.txt")
            .count(),
        1
    );
    assert_eq!(
        generated
            .yaml
            .matches("url: https://example.com/two.txt")
            .count(),
        1
    );
}

#[test]
fn native_local_policies_compile_with_ddns_cache_bypass_and_ttl() {
    let mut intent = StandardIntent::default();
    intent.local.hosts.entries = vec!["full:router.test 192.0.2.1".to_string()];
    intent.local.redirects.rules = vec!["full:old.test target.test".to_string()];
    intent.local.records.rules = vec!["answer.test. 60 IN A 192.0.2.2".to_string()];
    intent.local.response_ttl.enabled = true;
    intent.local.response_ttl.min = Some(30);
    intent.local.response_ttl.max = Some(600);
    intent.local.qtype_policy.enabled = true;
    intent.local.qtype_policy.qtypes = vec!["HTTPS".to_string(), "SVCB".to_string()];
    intent.local.qtype_policy.response = super::model::StandardBlockResponse::Nodata;
    intent.local.ddns.enabled = true;
    intent.local.ddns.domains = vec!["dynamic.test".to_string()];
    intent.local.ddns.ttl = 20;

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);

    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.expect("generated config");
    for tag in [
        "standard_local_hosts",
        "standard_local_redirect",
        "standard_local_records",
        "standard_local_response_ttl",
        "standard_local_qtype_match",
        "standard_local_qtype_action",
        "standard_local_ddns_match",
        "standard_local_ddns_ttl",
        "standard_local_ddns_action",
    ] {
        assert!(generated.generated_tags.iter().any(|value| value == tag));
    }
    let parsed: YamlValue = serde_yaml_ng::from_str(&generated.yaml).expect("generated YAML");
    let plugins = parsed["plugins"].as_sequence().expect("plugins");
    let ddns = plugins
        .iter()
        .find(|plugin| plugin["tag"] == "standard_local_ddns_action")
        .expect("DDNS action");
    let ddns_steps = ddns["args"].as_sequence().expect("DDNS sequence");
    assert!(
        ddns_steps
            .iter()
            .all(|step| step["exec"] != "$standard_cache_default"),
        "DDNS action must bypass the normal path cache"
    );
    assert!(
        ddns_steps
            .iter()
            .any(|step| { step["exec"] == "$standard_local_ddns_ttl" })
    );
}

#[test]
fn invalid_path_reference_is_an_error_instead_of_default_fallback() {
    let mut intent = StandardIntent::default();
    intent.paths[0].upstream_group_id = "missing".to_string();
    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);

    assert!(!plan.can_apply);
    assert!(plan.generated.is_none());
    assert!(
        plan.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "path_upstream_group_missing")
    );
}

#[test]
fn path_local_feature_enablement_generates_required_plugins() {
    let mut intent = StandardIntent::default();
    intent.filtering.enabled = false;
    intent.query_log.enabled = false;
    intent.filtering.block_rules = vec!["||ads.example^".to_string()];
    intent.paths[0].filtering = super::model::StandardPolicySwitch::Enabled;
    intent.paths[0].query_log = super::model::StandardPolicySwitch::Enabled;

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);

    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.expect("generated config");
    assert!(
        generated
            .generated_tags
            .iter()
            .any(|tag| tag == "standard_ad_rules")
    );
    assert!(
        generated
            .generated_tags
            .iter()
            .any(|tag| tag == "standard_recorder")
    );
}

#[test]
fn unsupported_active_phase_two_fields_fail_planning() {
    let mut intent = StandardIntent::default();
    intent.paths[0].ecs = super::model::StandardPolicySwitch::Enabled;
    intent.query_log.sample_rate = 0.5;

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);

    assert!(!plan.can_apply);
    assert!(plan.generated.is_none());
    let codes: Vec<_> = plan
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    assert!(codes.contains(&"ecs_not_available"));
    assert!(codes.contains(&"query_log_sampling_not_available"));
}

#[test]
fn every_supported_upstream_strategy_emits_runtime_response_selection() {
    use super::model::StandardUpstreamStrategy;

    for (strategy, expected) in [
        (StandardUpstreamStrategy::Fastest, "fastest"),
        (StandardUpstreamStrategy::Balanced, "balanced"),
        (StandardUpstreamStrategy::PreferPositive, "prefer_positive"),
        (StandardUpstreamStrategy::Consensus, "consensus"),
    ] {
        let mut intent = StandardIntent::default();
        intent.upstream_groups[0].strategy = strategy;
        let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
        assert!(plan.can_apply, "{expected}: {:?}", plan.diagnostics);
        assert!(
            plan.generated
                .expect("generated config")
                .yaml
                .contains(&format!("response_selection: {expected}"))
        );
    }
}

#[test]
fn ordered_fallback_is_explicitly_blocked() {
    let mut intent = StandardIntent::default();
    intent.upstream_groups[0].strategy = super::model::StandardUpstreamStrategy::OrderedFallback;

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);

    assert!(!plan.can_apply);
    assert!(
        plan.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ordered_fallback_not_available")
    );
}

#[test]
fn multiple_paths_receive_distinct_cache_plugins() {
    let mut intent = StandardIntent::default();
    let mut secondary = intent.paths[0].clone();
    secondary.id = "secondary".to_string();
    secondary.name = "Secondary".to_string();
    intent.paths.push(secondary);

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);

    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.expect("generated config");
    assert_eq!(generated.tag_map.caches.len(), 2);
    assert_eq!(
        generated.tag_map.caches.get("default").map(String::as_str),
        Some("standard_cache_default")
    );
    assert_eq!(
        generated
            .tag_map
            .caches
            .get("secondary")
            .map(String::as_str),
        Some("standard_cache_secondary")
    );
    assert!(generated.yaml.contains("$standard_cache_default"));
    assert!(generated.yaml.contains("$standard_cache_secondary"));
}

#[test]
fn invalid_ttl_ranges_and_all_inert_phase_two_controls_are_reported() {
    use super::model::{
        StandardDualStackPolicy, StandardPolicySwitch, StandardScenario, StandardScenarioKind,
    };

    let mut intent = StandardIntent::default();
    intent.cache.min_positive_ttl = 600;
    intent.cache.max_positive_ttl = 60;
    intent.cache.max_negative_ttl = 30;
    intent.cache.negative_ttl_without_soa = 60;
    intent.paths[0].dual_stack = StandardDualStackPolicy::PreferIpv4;
    intent.paths[0].ip_selection = StandardPolicySwitch::Enabled;
    intent.paths[0].ecs = StandardPolicySwitch::Enabled;
    intent.query_log.sample_rate = 0.5;
    intent.routing.scenarios.push(StandardScenario {
        id: "privacy".to_string(),
        name: "Privacy".to_string(),
        enabled: true,
        kind: StandardScenarioKind::Privacy,
    });

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    let codes: std::collections::BTreeSet<_> = plan
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    for expected in [
        "cache_positive_ttl_range_invalid",
        "cache_negative_ttl_range_invalid",
        "dual_stack_not_available",
        "ip_selection_not_available",
        "ecs_not_available",
        "query_log_sampling_not_available",
        "scenario_not_available",
    ] {
        assert!(codes.contains(expected), "missing diagnostic {expected}");
    }
}

#[test]
fn required_capabilities_block_generation_while_metrics_only_warns() {
    let unsupported = StandardCapabilities::from_build(
        std::iter::empty::<String>(),
        &crate::build_info::SupportedPlugins::default(),
    );
    let blocked = compile_standard_intent(StandardIntent::default(), &unsupported, None, None);
    assert!(!blocked.can_apply);
    assert!(blocked.generated.is_none());
    assert!(blocked.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == StandardDiagnosticSeverity::Error
            && diagnostic.code == "required_capability_missing"
    }));

    let mut without_metrics = StandardCapabilities::for_tests();
    without_metrics.executors.remove("metrics_collector");
    let warning = compile_standard_intent(StandardIntent::default(), &without_metrics, None, None);
    assert!(warning.can_apply, "diagnostics: {:?}", warning.diagnostics);
    assert!(warning.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == StandardDiagnosticSeverity::Warning
            && diagnostic.code == "optional_metrics_unavailable"
    }));
}
