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
