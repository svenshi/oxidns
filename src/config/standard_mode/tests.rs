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
    #[cfg(feature = "standard")]
    crate::config::validate_text(&generated.yaml)
        .expect("generated default configuration should pass backend analysis");
}

#[test]
fn dedicated_group_compiles_complete_native_bundle_and_deletes_without_residue() {
    use super::model::{
        StandardDedicatedGroup, StandardDedicatedListener, StandardDedicatedPathPolicy,
        StandardUpstreamStrategy,
    };

    let mut intent = StandardIntent::default();
    intent.dedicated_groups.push(StandardDedicatedGroup {
        id: "media".to_string(),
        name: "Media".to_string(),
        description: Some("Dedicated media DNS".to_string()),
        enabled: true,
        priority: 10,
        rules: vec!["domain:media.example".to_string()],
        strategy: StandardUpstreamStrategy::Consensus,
        upstreams: intent.upstream_groups[0].upstreams.clone(),
        path: StandardDedicatedPathPolicy::default(),
        listener: StandardDedicatedListener {
            enabled: true,
            address: "127.0.0.1:5539".to_string(),
            udp: true,
            tcp: true,
        },
    });

    let plan = compile_standard_intent(
        intent.clone(),
        &StandardCapabilities::for_tests(),
        None,
        None,
    );
    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.expect("dedicated group should compile");
    let tags = generated
        .tag_map
        .dedicated_groups
        .get("media")
        .expect("dedicated tag map");
    assert_eq!(tags.provider, "standard_dedicated_provider_media");
    assert_eq!(tags.matcher, "standard_dedicated_match_media");
    assert_eq!(tags.upstream_group, "standard_dedicated_forward_media");
    assert_eq!(tags.path, "standard_dedicated_path_media");
    assert_eq!(
        tags.cache.as_deref(),
        Some("standard_dedicated_cache_media")
    );
    assert_eq!(
        tags.udp_listener.as_deref(),
        Some("standard_dedicated_udp_media")
    );
    assert_eq!(
        tags.tcp_listener.as_deref(),
        Some("standard_dedicated_tcp_media")
    );
    assert!(generated.yaml.contains("standard_dedicated_provider_media"));
    assert!(generated.yaml.contains("response_selection: consensus"));
    assert!(generated.yaml.contains("listen: 127.0.0.1:5539"));

    intent.dedicated_groups.clear();
    let deleted = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    assert!(deleted.can_apply, "diagnostics: {:?}", deleted.diagnostics);
    let deleted = deleted.generated.expect("deleted intent should compile");
    assert!(deleted.tag_map.dedicated_groups.is_empty());
    assert!(!deleted.tag_map.caches.contains_key("dedicated:media"));
    assert!(!deleted.yaml.contains("standard_dedicated_"));
}

#[test]
fn dedicated_listener_collision_and_incomplete_group_are_apply_blockers() {
    use super::model::{
        StandardDedicatedGroup, StandardDedicatedListener, StandardDedicatedPathPolicy,
        StandardUpstreamStrategy,
    };

    let mut intent = StandardIntent::default();
    intent.dedicated_groups.push(StandardDedicatedGroup {
        id: "broken".to_string(),
        name: "Broken".to_string(),
        description: None,
        enabled: true,
        priority: 0,
        rules: Vec::new(),
        strategy: StandardUpstreamStrategy::Consensus,
        upstreams: vec![intent.upstream_groups[0].upstreams[0].clone()],
        path: StandardDedicatedPathPolicy::default(),
        listener: StandardDedicatedListener {
            enabled: true,
            address: intent.listen.address.clone(),
            udp: true,
            tcp: false,
        },
    });

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    let codes: std::collections::BTreeSet<_> = plan
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    for expected in [
        "dedicated_rules_required",
        "consensus_upstreams_insufficient",
        "dedicated_listener_collision",
    ] {
        assert!(codes.contains(expected), "missing diagnostic {expected}");
    }
    assert!(!plan.can_apply);
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
fn schema_v4_migrates_phase_two_placeholders_to_explicit_current_policies() {
    use super::model::StandardEcsPolicy;

    let mut value = serde_json::to_value(StandardIntent::default()).expect("serialize default");
    value["schema"] = json!(4);
    value["paths"][0]["ecs"] = json!("enabled");
    value["paths"][0]["ipSelection"] = json!("enabled");

    let (intent, migration) = decode_standard_intent(value).expect("v4 should migrate");

    assert_eq!(intent.schema, CURRENT_STANDARD_SCHEMA);
    assert!(matches!(
        intent.paths[0].ecs,
        StandardEcsPolicy::ClientSubnet {
            mask4: 24,
            mask6: 48
        }
    ));
    assert!(intent.paths[0].ip_selection.enabled);
    let codes: std::collections::BTreeSet<_> = migration
        .expect("migration metadata")
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains("schema_v4_migrated"));
    assert!(codes.contains("legacy_ecs_enabled_migrated"));
    assert!(codes.contains("legacy_ip_selection_enabled_migrated"));
}

#[test]
fn schema_v5_migrates_with_inactive_phase_three_defaults_and_removes_placeholders() {
    let mut value = serde_json::to_value(StandardIntent::default()).expect("serialize default");
    value["schema"] = json!(5);
    value
        .as_object_mut()
        .expect("intent object")
        .remove("dedicatedGroups");
    value
        .as_object_mut()
        .expect("intent object")
        .remove("dynamicLearning");
    value
        .as_object_mut()
        .expect("intent object")
        .remove("advancedRules");
    value["routing"]["scenarios"] = json!([]);

    let (intent, migration) = decode_standard_intent(value).expect("v5 should migrate");

    assert_eq!(intent.schema, CURRENT_STANDARD_SCHEMA);
    assert!(intent.dedicated_groups.is_empty());
    assert!(intent.dynamic_learning.profiles.is_empty());
    assert!(intent.advanced_rules.is_empty());
    let migration = migration.expect("migration metadata");
    assert_eq!(migration.from_schema, 5);
    assert_eq!(migration.to_schema, CURRENT_STANDARD_SCHEMA);
    assert!(
        migration
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "schema_v5_migrated")
    );
}

#[test]
fn schema_v5_enabled_placeholder_requires_explicit_template_rebuild() {
    let mut value = serde_json::to_value(StandardIntent::default()).expect("serialize default");
    value["schema"] = json!(5);
    value["routing"]["scenarios"] = json!([{
        "id": "privacy",
        "name": "Privacy",
        "enabled": true,
        "kind": "privacy"
    }]);

    let (_, migration) = decode_standard_intent(value).expect("v5 should decode");
    assert!(
        migration
            .expect("migration metadata")
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "legacy_scenario_requires_rebuild")
    );
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
    assert_eq!(
        generated.yaml.matches("exec: $standard_recorder").count(),
        1
    );
    assert!(generated.yaml.contains(&format!(
        "mark {}",
        super::compiler::STANDARD_QUERY_RECORD_MARK
    )));
}

#[test]
fn global_query_logging_wraps_the_complete_path_and_honors_path_opt_out() {
    let mut intent = StandardIntent::default();
    intent.paths[0].query_log = super::model::StandardPolicySwitch::Disabled;
    let mut logged_path = intent.paths[0].clone();
    logged_path.id = "logged".to_string();
    logged_path.name = "Logged path".to_string();
    logged_path.query_log = super::model::StandardPolicySwitch::Inherit;
    intent.paths.push(logged_path);

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);

    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.expect("generated config");
    assert_eq!(
        generated.yaml.matches("exec: $standard_recorder").count(),
        1
    );
    assert!(generated.yaml.contains(&format!(
        "mark {}",
        super::compiler::STANDARD_QUERY_SKIP_MARK
    )));
}

#[test]
fn ecs_path_policy_compiles_while_query_sampling_remains_explicitly_blocked() {
    let mut intent = StandardIntent::default();
    intent.paths[0].ecs = super::model::StandardEcsPolicy::ClientSubnet {
        mask4: 24,
        mask6: 48,
    };
    intent.query_log.sample_rate = 0.5;

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);

    assert!(!plan.can_apply);
    let codes: Vec<_> = plan
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
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
fn ordered_fallback_compiles_explicit_member_chain() {
    let mut intent = StandardIntent::default();
    intent.upstream_groups[0].strategy = super::model::StandardUpstreamStrategy::OrderedFallback;
    let mut third = intent.upstream_groups[0].upstreams[0].clone();
    third.id = "third".to_string();
    third.name = "Third".to_string();
    third.address = "9.9.9.9:53".to_string();
    intent.upstream_groups[0].upstreams.push(third);
    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let yaml = plan.generated.unwrap().yaml;
    assert!(yaml.contains("standard_forward_default_member_0"));
    assert!(yaml.contains("type: fallback"));
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
fn invalid_ttl_ranges_are_reported_with_active_path_controls() {
    use super::model::{StandardDualStackPolicy, StandardEcsPolicy};

    let mut intent = StandardIntent::default();
    intent.cache.min_positive_ttl = 600;
    intent.cache.max_positive_ttl = 60;
    intent.cache.max_negative_ttl = 30;
    intent.cache.negative_ttl_without_soa = 60;
    intent.paths[0].dual_stack = StandardDualStackPolicy::PreferIpv4;
    intent.paths[0].ip_selection.enabled = true;
    intent.paths[0].ecs = StandardEcsPolicy::ClientSubnet {
        mask4: 24,
        mask6: 48,
    };
    intent.query_log.sample_rate = 0.5;

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    let codes: std::collections::BTreeSet<_> = plan
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    for expected in [
        "cache_positive_ttl_range_invalid",
        "cache_negative_ttl_range_invalid",
        "query_log_sampling_not_available",
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

fn smart_intent(mode: super::model::StandardUnknownMode) -> StandardIntent {
    use super::model::{StandardEcsPolicy, StandardRuleDataRole, StandardRuleDataSource};

    let mut intent = StandardIntent::default();
    intent
        .upstream_groups
        .push(super::model::StandardUpstreamGroup {
            id: "remote".to_string(),
            name: "Remote".to_string(),
            description: None,
            strategy: super::model::StandardUpstreamStrategy::OrderedFallback,
            upstreams: vec![super::model::StandardUpstream {
                id: "remote_upstream".to_string(),
                name: "Remote upstream".to_string(),
                protocol: super::model::StandardUpstreamProtocol::Udp,
                address: "9.9.9.9:53".to_string(),
                enabled: true,
                bootstrap: None,
                bootstrap_version: None,
                dial_address: None,
                outbound: Some("remote-egress".to_string()),
                socks5: None,
                timeout_seconds: Some(2),
                idle_timeout_seconds: None,
                max_conns: None,
                min_conns: None,
                enable_pipeline: false,
                tls_verify: true,
                doh_path: None,
                enable_http3: false,
            }],
            is_default: false,
        });
    let mut remote_path = intent.paths[0].clone();
    remote_path.id = "remote".to_string();
    remote_path.name = "Remote".to_string();
    remote_path.upstream_group_id = "remote".to_string();
    remote_path.ecs = StandardEcsPolicy::Preset {
        address: "203.0.113.9".to_string(),
        mask4: 24,
        mask6: 48,
    };
    remote_path.ip_selection.enabled = true;
    intent.paths.push(remote_path);
    intent.rule_data.domestic_domains = StandardRuleDataRole {
        sources: vec![StandardRuleDataSource::Manual {
            id: "domestic_domains_manual".to_string(),
            name: "Domestic domains".to_string(),
            enabled: true,
            rules: vec!["domain:example.cn".to_string()],
        }],
    };
    intent.rule_data.domestic_ips = StandardRuleDataRole {
        sources: vec![StandardRuleDataSource::Manual {
            id: "domestic_ips_manual".to_string(),
            name: "Domestic IPs".to_string(),
            enabled: true,
            rules: vec!["10.0.0.0/8".to_string()],
        }],
    };
    intent.rule_data.remote_domains = StandardRuleDataRole {
        sources: vec![StandardRuleDataSource::Manual {
            id: "remote_domains_manual".to_string(),
            name: "Remote domains".to_string(),
            enabled: true,
            rules: vec!["domain:example.com".to_string()],
        }],
    };
    intent.smart_routing.enabled = true;
    intent.smart_routing.domestic_path_id = Some("default".to_string());
    intent.smart_routing.remote_path_id = Some("remote".to_string());
    intent.smart_routing.unknown_mode = mode;
    intent
}

#[test]
fn smart_routing_compiles_native_validation_fallback_and_isolated_ecs_caches() {
    let intent = smart_intent(super::model::StandardUnknownMode::CompatibilityFirst);
    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);

    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.expect("generated smart config");
    assert!(
        generated
            .yaml
            .contains("standard_smart_drop_domestic_ip_mismatch")
    );
    assert!(generated.yaml.contains("reason: domestic_ip_mismatch"));
    assert!(
        generated
            .yaml
            .contains("standard_smart_unknown_compatibility")
    );
    assert!(generated.yaml.contains("ecs_in_key: true"));
    assert!(
        generated
            .tag_map
            .caches
            .contains_key("smart:unknown_compatibility_domestic")
    );
    assert!(
        generated
            .tag_map
            .caches
            .contains_key("smart:unknown_compatibility_remote")
    );
    #[cfg(feature = "standard")]
    crate::config::validate_text(&generated.yaml)
        .expect("generated smart-routing graph should pass backend analysis");
}

#[test]
fn strict_remote_unknown_action_has_no_domestic_fallback_edge() {
    let intent = smart_intent(super::model::StandardUnknownMode::StrictRemote);
    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.unwrap();
    assert_eq!(
        generated
            .tag_map
            .smart_routing
            .get("unknownAction")
            .map(String::as_str),
        Some("standard_path_unknown_strict_remote")
    );
    assert!(
        !generated
            .generated_tags
            .iter()
            .any(|tag| tag == "standard_smart_unknown_strict_remote")
    );
}

#[test]
fn strict_remote_rejects_domestic_unknown_fallback() {
    let mut intent = smart_intent(super::model::StandardUnknownMode::StrictRemote);
    intent.smart_routing.privacy_fallback_to_domestic = true;
    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    assert!(!plan.can_apply);
    assert!(
        plan.diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "strict_remote_domestic_fallback_forbidden" })
    );
}

#[test]
fn smart_timeout_and_transport_policies_compile_into_fallback_control() {
    let mut intent = smart_intent(super::model::StandardUnknownMode::CompatibilityFirst);
    intent.smart_routing.response_policy.timeout = false;
    intent.smart_routing.response_policy.transport_failure = false;

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let yaml = plan.generated.expect("generated config").yaml;
    assert!(yaml.contains("fallback_on_timeout: false"));
    assert!(yaml.contains("fallback_on_error: false"));
    assert!(yaml.contains("fallback_on_no_response: true"));
}

#[test]
fn semantic_subscription_emits_stable_lifecycle_tags_and_role_provider() {
    use super::model::{StandardRuleDataRole, StandardRuleDataSource};

    let mut intent = StandardIntent::default();
    intent.rule_data.remote_domains = StandardRuleDataRole {
        sources: vec![StandardRuleDataSource::Subscription {
            id: "remote_feed".to_string(),
            name: "Remote feed".to_string(),
            enabled: true,
            url: "https://example.com/remote.txt".to_string(),
            update_interval_hours: 12,
            max_age_hours: 36,
        }],
    };

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.expect("generated config");
    assert_eq!(
        generated
            .tag_map
            .rule_data
            .get("remote_domains")
            .map(String::as_str),
        Some("standard_rule_data_remote_domains")
    );
    let lifecycle = generated
        .tag_map
        .rule_data_sources
        .get("remote_domains:remote_feed")
        .expect("subscription lifecycle tags");
    assert_eq!(
        lifecycle.download,
        "standard_rule_data_download_remote_domains_remote_feed"
    );
    assert!(generated.yaml.contains("interval: 12h"));
    assert!(generated.yaml.contains("stop_on_error: true"));
}

#[test]
fn missing_local_and_native_rule_data_files_are_apply_blockers() {
    use super::model::{StandardRuleDataRole, StandardRuleDataSource};

    let mut intent = StandardIntent::default();
    intent.rule_data.foreign_domains = StandardRuleDataRole {
        sources: vec![
            StandardRuleDataSource::LocalFile {
                id: "missing_text".to_string(),
                name: "Missing text".to_string(),
                enabled: true,
                path: "/definitely/not/present/oxidns-domains.txt".to_string(),
            },
            StandardRuleDataSource::NativeDat {
                id: "missing_dat".to_string(),
                name: "Missing dat".to_string(),
                enabled: true,
                path: "/definitely/not/present/geosite.dat".to_string(),
                selectors: vec!["geolocation-!cn".to_string()],
            },
        ],
    };

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    let codes: std::collections::BTreeSet<_> = plan
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    assert!(!plan.can_apply);
    assert!(codes.contains("rule_data_file_missing"));
    assert!(codes.contains("rule_data_native_file_missing"));
}

#[test]
fn active_path_controls_compile_qtype_ecs_selector_and_dnssec_safe_policy() {
    use super::model::{StandardDnssecPolicy, StandardDualStackPolicy, StandardEcsPolicy};

    let mut intent = StandardIntent::default();
    let path = &mut intent.paths[0];
    path.dual_stack = StandardDualStackPolicy::Ipv4Only;
    path.ecs = StandardEcsPolicy::PreserveClient;
    path.ip_selection.enabled = true;
    path.ip_selection.dnssec_policy = StandardDnssecPolicy::Skip;

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let yaml = plan.generated.expect("generated config").yaml;
    for expected in [
        "standard_path_qtype_default",
        "standard_path_qtype_block_default",
        "type: ecs_handler",
        "forward: true",
        "ecs_in_key: true",
        "type: ip_selector",
        "dnssec_policy: skip",
    ] {
        assert!(yaml.contains(expected), "missing {expected} in {yaml}");
    }
}

#[test]
fn plan_reports_duplicate_and_overridden_rules_with_winners() {
    use super::model::{StandardExceptionRule, StandardRuleAction, StandardRuleCondition};

    let mut intent = StandardIntent::default();
    let condition = StandardRuleCondition::Suffix {
        values: vec!["example.com".to_string()],
    };
    intent.exceptions = vec![
        StandardExceptionRule {
            id: "allow_first".to_string(),
            name: "Allow first".to_string(),
            enabled: true,
            condition: condition.clone(),
            action: StandardRuleAction::Allow,
            note: None,
        },
        StandardExceptionRule {
            id: "allow_duplicate".to_string(),
            name: "Allow duplicate".to_string(),
            enabled: true,
            condition: condition.clone(),
            action: StandardRuleAction::Allow,
            note: None,
        },
        StandardExceptionRule {
            id: "route_overridden".to_string(),
            name: "Route overridden".to_string(),
            enabled: true,
            condition,
            action: StandardRuleAction::UseDefaultPath,
            note: None,
        },
    ];

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    let codes: Vec<_> = plan
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    assert!(codes.contains(&"rule_duplicate"));
    assert!(codes.contains(&"rule_conflict_overridden"));
    let analysis = plan.details["ruleAnalysis"]
        .as_array()
        .expect("rule analysis rows");
    assert!(analysis.iter().any(|row| {
        row["id"] == "route_overridden"
            && row["status"] == "overridden"
            && row["overriddenBy"] == "allow_first"
    }));
}

#[test]
fn dynamic_learning_compiles_bounded_lifecycle_response_classification_and_route() {
    use super::model::{
        StandardDynamicLearningProfile, StandardLearningFailurePolicy, StandardLearningRuleKind,
    };

    let mut intent = StandardIntent::default();
    intent
        .dynamic_learning
        .profiles
        .push(StandardDynamicLearningProfile {
            id: "video".to_string(),
            name: "Video learning".to_string(),
            enabled: true,
            paused: false,
            target_path_id: "default".to_string(),
            priority: 20,
            qtypes: vec!["A".to_string(), "AAAA".to_string()],
            rcodes: vec!["NOERROR".to_string()],
            answer_required: true,
            response_ip_role: None,
            rule_kind: StandardLearningRuleKind::Domain,
            max_entries: 4096,
            entry_ttl_seconds: 86_400,
            cleanup_interval_seconds: 300,
            queue_size: 256,
            batch_size: 32,
            flush_interval_ms: 100,
            failure_policy: StandardLearningFailurePolicy::Continue,
        });

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.expect("dynamic learning config");
    assert_eq!(
        generated.managed_files,
        vec![
            "./data/standard-dynamic-learning/video.meta.json",
            "./data/standard-dynamic-learning/video.txt",
        ]
    );
    let tags = generated
        .tag_map
        .dynamic_learning
        .get("video")
        .expect("learning tag map");
    assert_eq!(tags.provider, "standard_learn_provider_video");
    assert_eq!(tags.action, "standard_learn_action_video");
    for expected in [
        "type: dynamic_domain_set",
        "max_entries: 4096",
        "entry_ttl_seconds: 86400",
        "type: learn_domain",
        "phase: before",
        "async: true",
        "standard_learn_rcode_video",
        "standard_learn_answer_video",
    ] {
        assert!(generated.yaml.contains(expected), "missing {expected}");
    }
    #[cfg(feature = "standard")]
    crate::config::validate_text(&generated.yaml)
        .expect("generated learning graph should pass backend analysis");
}

#[test]
fn learned_routes_follow_manual_forced_routing_in_the_main_sequence() {
    use super::model::{
        StandardDynamicLearningProfile, StandardLearningFailurePolicy, StandardLearningRuleKind,
        StandardRoutingRule, StandardRuleAction, StandardRuleCondition, StandardRuleSource,
    };

    let mut intent = StandardIntent::default();
    intent.routing.enabled = true;
    intent.routing.rules.push(StandardRoutingRule {
        id: "forced".to_string(),
        name: "Forced route".to_string(),
        enabled: true,
        condition: StandardRuleCondition::Domain {
            values: vec!["forced.example".to_string()],
        },
        action: StandardRuleAction::UseDefaultPath,
        source: StandardRuleSource::Manual,
        note: None,
    });
    intent
        .dynamic_learning
        .profiles
        .push(StandardDynamicLearningProfile {
            id: "learned".to_string(),
            name: "Learned route".to_string(),
            enabled: true,
            paused: false,
            target_path_id: "default".to_string(),
            priority: 0,
            qtypes: vec!["A".to_string()],
            rcodes: vec!["NOERROR".to_string()],
            answer_required: true,
            response_ip_role: None,
            rule_kind: StandardLearningRuleKind::Full,
            max_entries: 100,
            entry_ttl_seconds: 3600,
            cleanup_interval_seconds: 60,
            queue_size: 32,
            batch_size: 8,
            flush_interval_ms: 50,
            failure_policy: StandardLearningFailurePolicy::Continue,
        });

    let generated = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None)
        .generated
        .expect("priority graph");
    let document: YamlValue = serde_yaml_ng::from_str(&generated.yaml).expect("generated YAML");
    let main = document["plugins"]
        .as_sequence()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["tag"].as_str() == Some("standard_main_sequence"))
        .expect("main sequence");
    let steps = main["args"].as_sequence().expect("main steps");
    let forced = steps
        .iter()
        .position(|step| step["matches"].as_str() == Some("$standard_route_match_forced"))
        .expect("forced route step");
    let learned = steps
        .iter()
        .position(|step| step["matches"].as_str() == Some("$standard_learn_match_learned"))
        .expect("learned route step");
    assert!(
        forced < learned,
        "manual forced routing must win before learning"
    );
}

#[test]
fn advanced_rules_compile_request_and_finite_response_reroute() {
    use super::model::{
        StandardAdvancedAction, StandardAdvancedCondition, StandardAdvancedFailurePolicy,
        StandardAdvancedFailureResponse, StandardAdvancedRule, StandardAdvancedRulePhase,
    };

    let mut intent = StandardIntent::default();
    let mut remote_group = intent.upstream_groups[0].clone();
    remote_group.id = "remote".to_string();
    remote_group.name = "Remote".to_string();
    remote_group.is_default = false;
    intent.upstream_groups.push(remote_group);
    let mut remote_path = intent.paths[0].clone();
    remote_path.id = "remote".to_string();
    remote_path.name = "Remote".to_string();
    remote_path.upstream_group_id = "remote".to_string();
    intent.paths.push(remote_path);
    intent.advanced_rules = vec![
        StandardAdvancedRule {
            id: "office_hours".to_string(),
            name: "Office hours".to_string(),
            enabled: true,
            priority: 10,
            phase: StandardAdvancedRulePhase::Request,
            conditions: vec![StandardAdvancedCondition::Qtype {
                values: vec!["AAAA".to_string()],
            }],
            action: StandardAdvancedAction::UsePath {
                path_id: "remote".to_string(),
            },
            failure_policy: StandardAdvancedFailurePolicy::FailOpen,
            failure_response: StandardAdvancedFailureResponse::Servfail,
            template_origin: None,
        },
        StandardAdvancedRule {
            id: "retry_remote".to_string(),
            name: "Retry remote".to_string(),
            enabled: true,
            priority: 20,
            phase: StandardAdvancedRulePhase::Response,
            conditions: vec![
                StandardAdvancedCondition::SourcePath {
                    path_id: "default".to_string(),
                },
                StandardAdvancedCondition::Rcode {
                    values: vec!["SERVFAIL".to_string()],
                },
            ],
            action: StandardAdvancedAction::UsePath {
                path_id: "remote".to_string(),
            },
            failure_policy: StandardAdvancedFailurePolicy::FailClosed,
            failure_response: StandardAdvancedFailureResponse::Refused,
            template_origin: None,
        },
    ];

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let generated = plan.generated.expect("advanced rules config");
    for expected in [
        "standard_advanced_action_office_hours",
        "standard_advanced_match_office_hours_0",
        "standard_path_advanced_target_retry_remote",
        "standard_advanced_drop_retry_remote",
        "fallback_on_timeout: false",
        "mode: refused",
    ] {
        assert!(generated.yaml.contains(expected), "missing {expected}");
    }
    #[cfg(feature = "standard")]
    crate::config::validate_text(&generated.yaml)
        .expect("generated advanced graph should pass backend analysis");
}

#[test]
fn advanced_rules_compile_every_declared_native_condition_family() {
    use super::model::{
        StandardAdvancedAction, StandardAdvancedCondition, StandardAdvancedFailurePolicy,
        StandardAdvancedFailureResponse, StandardAdvancedRule, StandardAdvancedRulePhase,
        StandardRuleDataSource, StandardTimePeriod,
    };

    let mut intent = StandardIntent::default();
    let mut remote_group = intent.upstream_groups[0].clone();
    remote_group.id = "remote".to_string();
    remote_group.name = "Remote".to_string();
    remote_group.is_default = false;
    intent.upstream_groups.push(remote_group);
    let mut remote_path = intent.paths[0].clone();
    remote_path.id = "remote".to_string();
    remote_path.name = "Remote".to_string();
    remote_path.upstream_group_id = "remote".to_string();
    intent.paths.push(remote_path);
    intent
        .rule_data
        .domestic_ips
        .sources
        .push(StandardRuleDataSource::Manual {
            id: "manual_ips".to_string(),
            name: "Manual IPs".to_string(),
            enabled: true,
            rules: vec!["192.0.2.0/24".to_string()],
        });
    intent.advanced_rules = vec![
        StandardAdvancedRule {
            id: "request_all".to_string(),
            name: "Request AND".to_string(),
            enabled: true,
            priority: 1,
            phase: StandardAdvancedRulePhase::Request,
            conditions: vec![
                StandardAdvancedCondition::Domain {
                    values: vec!["full:exact.example".to_string()],
                },
                StandardAdvancedCondition::Suffix {
                    values: vec!["example".to_string()],
                },
                StandardAdvancedCondition::Keyword {
                    values: vec!["exact".to_string()],
                },
                StandardAdvancedCondition::ClientCidr {
                    values: vec!["127.0.0.0/8".to_string()],
                },
                StandardAdvancedCondition::Qtype {
                    values: vec!["A".to_string()],
                },
                StandardAdvancedCondition::Time {
                    timezone: "UTC".to_string(),
                    periods: vec![StandardTimePeriod {
                        start: Some("00:00".to_string()),
                        end: Some("23:59".to_string()),
                        weekdays: vec![1, 2, 3, 4, 5],
                        monthdays: vec![],
                    }],
                },
                StandardAdvancedCondition::RateLimitExceeded {
                    qps: 100,
                    burst: 200,
                    mask4: 32,
                    mask6: 64,
                },
            ],
            action: StandardAdvancedAction::UsePath {
                path_id: "remote".to_string(),
            },
            failure_policy: StandardAdvancedFailurePolicy::FailOpen,
            failure_response: StandardAdvancedFailureResponse::Servfail,
            template_origin: None,
        },
        StandardAdvancedRule {
            id: "response_all".to_string(),
            name: "Response AND".to_string(),
            enabled: true,
            priority: 2,
            phase: StandardAdvancedRulePhase::Response,
            conditions: vec![
                StandardAdvancedCondition::SourcePath {
                    path_id: "default".to_string(),
                },
                StandardAdvancedCondition::Cname {
                    values: vec!["domain:alias.example".to_string()],
                },
                StandardAdvancedCondition::Rcode {
                    values: vec!["NOERROR".to_string()],
                },
                StandardAdvancedCondition::HasWantedAnswer,
                StandardAdvancedCondition::ResponseIpRole {
                    role: "domestic_ips".to_string(),
                    invert: true,
                },
            ],
            action: StandardAdvancedAction::UsePath {
                path_id: "remote".to_string(),
            },
            failure_policy: StandardAdvancedFailurePolicy::FailOpen,
            failure_response: StandardAdvancedFailureResponse::Servfail,
            template_origin: None,
        },
    ];

    let plan = compile_standard_intent(intent, &StandardCapabilities::for_tests(), None, None);
    assert!(plan.can_apply, "diagnostics: {:?}", plan.diagnostics);
    let yaml = plan.generated.expect("advanced graph").yaml;
    for expected in [
        "type: qname",
        "type: client_ip",
        "type: qtype",
        "type: time",
        "type: rate_limiter",
        "type: cname",
        "type: rcode",
        "type: has_wanted_ans",
        "type: resp_ip",
        "!$standard_advanced_match_request_all_6",
        "!$standard_advanced_match_response_all_4",
    ] {
        assert!(yaml.contains(expected), "missing {expected}");
    }
    #[cfg(feature = "standard")]
    crate::config::validate_text(&yaml).expect("all advanced matchers should initialize");
}

#[test]
fn all_phase_three_templates_expand_to_complete_reviewable_plans() {
    use super::{StandardTemplateKind, StandardTemplateParameters, expand_standard_template};

    for (index, kind) in [
        StandardTemplateKind::LowLatency,
        StandardTemplateKind::PrivacyDns,
        StandardTemplateKind::InternalDomains,
        StandardTemplateKind::RegionalUpstream,
    ]
    .into_iter()
    .enumerate()
    {
        let base = StandardIntent::default();
        let mut upstreams = base.upstream_groups[0].upstreams.clone();
        if matches!(kind, StandardTemplateKind::PrivacyDns) {
            for (upstream_index, upstream) in upstreams.iter_mut().enumerate() {
                upstream.protocol = super::model::StandardUpstreamProtocol::Doh;
                upstream.address = format!("https://dns{upstream_index}.example/dns-query");
                upstream.doh_path = Some("/dns-query".to_string());
            }
        }
        let namespace = format!("template_{index}");
        let expansion = expand_standard_template(
            base,
            kind,
            StandardTemplateParameters {
                namespace: namespace.clone(),
                name: format!("Template {index}"),
                description: None,
                domains: vec!["domain:example.com".to_string()],
                upstreams,
                listener_address: matches!(kind, StandardTemplateKind::InternalDomains)
                    .then(|| format!("127.0.0.1:{}", 5600 + index)),
            },
        )
        .expect("template expansion");
        assert_eq!(
            expansion.objects_added,
            vec![format!("dedicatedGroups.{namespace}")]
        );
        let plan = compile_standard_intent(
            expansion.proposed_intent,
            &StandardCapabilities::for_tests(),
            None,
            None,
        );
        assert!(
            plan.can_apply,
            "template diagnostics: {:?}",
            plan.diagnostics
        );
        assert!(
            plan.generated
                .expect("generated template")
                .tag_map
                .dedicated_groups
                .contains_key(&namespace)
        );
    }
}

#[test]
fn phase_three_template_namespace_never_overwrites_existing_objects() {
    use super::{StandardTemplateKind, StandardTemplateParameters, expand_standard_template};

    let intent = StandardIntent::default();
    let error = expand_standard_template(
        intent.clone(),
        StandardTemplateKind::LowLatency,
        StandardTemplateParameters {
            namespace: intent.paths[0].id.clone(),
            name: "Collision".to_string(),
            description: None,
            domains: vec!["domain:example.com".to_string()],
            upstreams: intent.upstream_groups[0].upstreams.clone(),
            listener_address: None,
        },
    )
    .expect_err("cross-category namespace collision must be rejected");
    assert!(error.contains("collides"));
}
