// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use serde_yaml_ng::{Mapping, Value as YamlValue};

use super::model::{
    StandardBlockResponse, StandardDedicatedTagMap, StandardDiagnostic, StandardDiagnosticSeverity,
    StandardDualStackPolicy, StandardDynamicLearningTagMap, StandardEcsPolicy,
    StandardGeneratedConfig, StandardGenerationSummary, StandardIntent, StandardMigration,
    StandardPlan, StandardPolicySwitch, StandardResolutionPath, StandardRuleAction,
    StandardRuleCondition, StandardRuleDataSource, StandardSubscriptionTagMap, StandardTagMap,
    StandardUnknownMode, StandardUpstream, StandardUpstreamProtocol, StandardUpstreamStrategy,
};
use super::validation::{
    device_has_policy, effective_filtering_used, effective_query_log_used,
    normalize_standard_intent, safe_tag_component, standard_tag, validate_standard_intent,
};
use crate::build_info::SupportedPlugins;
use crate::infra::control::config_version;

const FILTER_SUBSCRIPTION_DIR: &str = "./data/standard-filter-subscriptions";
const RULE_DATA_SUBSCRIPTION_DIR: &str = "./data/standard-rule-data";
const DYNAMIC_LEARNING_DIR: &str = "./data/standard-dynamic-learning";
pub(super) const STANDARD_QUERY_RECORD_MARK: u32 = u32::MAX - 1;
pub(super) const STANDARD_QUERY_SKIP_MARK: u32 = u32::MAX;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StandardCapabilities {
    pub(super) features: BTreeSet<String>,
    pub(super) servers: BTreeSet<String>,
    pub(super) executors: BTreeSet<String>,
    pub(super) matchers: BTreeSet<String>,
    pub(super) providers: BTreeSet<String>,
}

impl StandardCapabilities {
    pub fn from_build(
        enabled_features: impl IntoIterator<Item = impl Into<String>>,
        supported_plugins: &SupportedPlugins,
    ) -> Self {
        Self {
            features: enabled_features.into_iter().map(Into::into).collect(),
            servers: supported_plugins.servers.iter().cloned().collect(),
            executors: supported_plugins.executors.iter().cloned().collect(),
            matchers: supported_plugins.matchers.iter().cloned().collect(),
            providers: supported_plugins.providers.iter().cloned().collect(),
        }
    }

    pub fn for_tests() -> Self {
        Self {
            features: [
                "upstream-dot",
                "upstream-doh",
                "upstream-doh3",
                "upstream-doq",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            servers: ["udp_server", "tcp_server"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            executors: [
                "metrics_collector",
                "query_recorder",
                "cache",
                "download",
                "black_hole",
                "reload_provider",
                "cron",
                "forward",
                "sequence",
                "hosts",
                "redirect",
                "arbitrary",
                "ttl",
                "prefer_ipv4",
                "prefer_ipv6",
                "ecs_handler",
                "ip_selector",
                "drop_resp",
                "fallback",
                "learn_domain",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            matchers: [
                "qname",
                "client_ip",
                "qtype",
                "resp_ip",
                "rcode",
                "has_wanted_ans",
                "cname",
                "time",
                "rate_limiter",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            providers: [
                "adguard_rule",
                "domain_set",
                "dynamic_domain_set",
                "ip_set",
                "geosite",
                "geoip",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        }
    }

    pub fn feature(&self, feature: &str) -> bool {
        self.features.contains(feature)
    }

    pub fn server(&self, kind: &str) -> bool {
        self.servers.contains(kind)
    }

    pub fn executor(&self, kind: &str) -> bool {
        self.executors.contains(kind)
    }

    pub fn matcher(&self, kind: &str) -> bool {
        self.matchers.contains(kind)
    }

    pub fn provider(&self, kind: &str) -> bool {
        self.providers.contains(kind)
    }
}

pub fn compile_standard_intent(
    intent: StandardIntent,
    capabilities: &StandardCapabilities,
    base_config_yaml: Option<&str>,
    migration: Option<StandardMigration>,
) -> StandardPlan {
    let normalized_intent = normalize_standard_intent(intent);
    let mut diagnostics = validate_standard_intent(&normalized_intent, capabilities);
    let (rule_diagnostics, rule_analysis) = analyze_rule_conflicts(&normalized_intent);
    diagnostics.extend(rule_diagnostics);
    let mut details = json!({
        "managedTopLevel": ["runtime.worker_threads", "log.level", "plugins"],
        "preservedTopLevel": ["include", "api", "network", "log.* except level"],
        "ruleAnalysis": rule_analysis,
    });

    let base = match parse_base_config(base_config_yaml) {
        Ok(value) => value,
        Err(diagnostic) => {
            diagnostics.push(diagnostic);
            Mapping::new()
        }
    };

    let generated = if has_errors(&diagnostics) {
        None
    } else {
        match compile_config(&normalized_intent, capabilities, &base) {
            Ok(generated) => {
                details["pathCaches"] = serde_json::to_value(&generated.tag_map.caches)
                    .expect("tag map should serialize");
                Some(generated)
            }
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                None
            }
        }
    };
    let can_apply = generated.is_some() && !has_errors(&diagnostics);

    StandardPlan {
        normalized_intent,
        diagnostics,
        generated,
        can_apply,
        migration,
        details,
    }
}

fn analyze_rule_conflicts(intent: &StandardIntent) -> (Vec<StandardDiagnostic>, JsonValue) {
    let mut diagnostics = Vec::new();
    let mut analysis = Vec::new();
    let mut effective = BTreeMap::<String, (String, String, String)>::new();

    for rule in ordered_exceptions(intent) {
        let condition = serde_json::to_string(&rule.condition).unwrap_or_default();
        let action = serde_json::to_string(&rule.action).unwrap_or_default();
        if let Some((winner_id, winner_action, winner_category)) = effective.get(&condition) {
            let duplicate = winner_action == &action;
            diagnostics.push(StandardDiagnostic::warning(
                if duplicate {
                    "rule_duplicate"
                } else {
                    "rule_conflict_overridden"
                },
                format!("exceptions.{}", rule.id),
                format!(
                    "rule '{}' is unreachable because '{}' in {} has the same condition and higher or earlier priority",
                    rule.id, winner_id, winner_category
                ),
            ));
            analysis.push(json!({
                "id": rule.id,
                "category": "exception",
                "status": if duplicate { "duplicate" } else { "overridden" },
                "overriddenBy": winner_id,
            }));
        } else {
            effective.insert(
                condition,
                (rule.id.clone(), action, "exception".to_string()),
            );
            analysis.push(json!({
                "id": rule.id,
                "category": "exception",
                "status": "effective",
            }));
        }
    }

    if intent.routing.enabled {
        for rule in intent.routing.rules.iter().filter(|rule| rule.enabled) {
            let condition = serde_json::to_string(&rule.condition).unwrap_or_default();
            let action = serde_json::to_string(&rule.action).unwrap_or_default();
            if let Some((winner_id, winner_action, winner_category)) = effective.get(&condition) {
                let duplicate = winner_action == &action;
                diagnostics.push(StandardDiagnostic::warning(
                    if duplicate {
                        "rule_duplicate"
                    } else {
                        "rule_conflict_overridden"
                    },
                    format!("routing.rules.{}", rule.id),
                    format!(
                        "rule '{}' is unreachable because '{}' in {} has the same condition and higher or earlier priority",
                        rule.id, winner_id, winner_category
                    ),
                ));
                analysis.push(json!({
                    "id": rule.id,
                    "category": "routing",
                    "status": if duplicate { "duplicate" } else { "overridden" },
                    "overriddenBy": winner_id,
                }));
            } else {
                effective.insert(condition, (rule.id.clone(), action, "routing".to_string()));
                analysis.push(json!({
                    "id": rule.id,
                    "category": "routing",
                    "status": "effective",
                }));
            }
        }
    }

    let mut advanced: Vec<_> = intent
        .advanced_rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.enabled)
        .collect();
    advanced.sort_by_key(|(index, rule)| (rule.priority, *index));
    for (_, rule) in advanced {
        analysis.push(json!({
            "id": rule.id,
            "category": "advanced",
            "phase": rule.phase,
            "status": "effective",
            "conditions": rule.conditions,
            "action": rule.action,
            "failurePolicy": rule.failure_policy,
            "templateOrigin": rule.template_origin,
        }));
    }
    for group in intent.dedicated_groups.iter().filter(|group| group.enabled) {
        analysis.push(json!({
            "id": group.id,
            "category": "dedicated",
            "status": "effective",
            "priority": group.priority,
            "ownedResources": ["provider", "matcher", "upstream", "path", "cache", "listener"],
        }));
    }
    for profile in intent
        .dynamic_learning
        .profiles
        .iter()
        .filter(|profile| profile.enabled)
    {
        analysis.push(json!({
            "id": profile.id,
            "category": "dynamic_learning",
            "status": if profile.paused { "paused" } else { "effective" },
            "priority": profile.priority,
            "targetPathId": profile.target_path_id,
            "failurePolicy": profile.failure_policy,
            "ownedResources": ["provider", "metadata", "learner", "matcher", "route"],
        }));
    }

    (diagnostics, JsonValue::Array(analysis))
}

fn compile_config(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    base: &Mapping,
) -> Result<StandardGeneratedConfig, StandardDiagnostic> {
    let mut plugins = Vec::<GeneratedPlugin>::new();
    let mut tag_map = StandardTagMap::default();

    if capabilities.executor("metrics_collector") {
        plugins.push(GeneratedPlugin::new(
            "standard_metrics",
            "metrics_collector",
            json!({}),
        ));
        tag_map.system.push("standard_metrics".to_string());
    }

    if effective_query_log_used(intent) {
        plugins.push(GeneratedPlugin::new(
            "standard_recorder",
            "query_recorder",
            json!({
                "path": "./data/standard-query-recorder.sqlite",
                "queue_size": 8192,
                "batch_size": 256,
                "flush_interval_ms": 200,
                "memory_tail": 1024,
                "retention_days": intent.query_log.retention_days.max(1),
                "cleanup_interval_hours": 1,
                "include_marks": if intent.query_log.enabled {
                    Vec::<u32>::new()
                } else {
                    vec![STANDARD_QUERY_RECORD_MARK]
                },
                "exclude_marks": if intent.query_log.enabled {
                    vec![STANDARD_QUERY_SKIP_MARK]
                } else {
                    Vec::<u32>::new()
                },
            }),
        ));
        tag_map.query_log = Some("standard_recorder".to_string());
    }

    let filtering = compile_filtering(intent, &mut plugins, &mut tag_map);
    compile_local_plugins(intent, &mut plugins, &mut tag_map);
    compile_rule_data(intent, &mut plugins, &mut tag_map);
    let learning = compile_dynamic_learning_primitives(intent, &mut plugins, &mut tag_map);

    let needs_prefer_ipv4 = intent
        .exceptions
        .iter()
        .any(|rule| rule.enabled && matches!(rule.action, StandardRuleAction::PreferIpv4));
    let needs_prefer_ipv6 = intent
        .exceptions
        .iter()
        .any(|rule| rule.enabled && matches!(rule.action, StandardRuleAction::PreferIpv6));
    if needs_prefer_ipv4 {
        plugins.push(GeneratedPlugin::new(
            "standard_prefer_ipv4",
            "prefer_ipv4",
            json!({ "cache": true, "cache_ttl": 3600 }),
        ));
    }
    if needs_prefer_ipv6 {
        plugins.push(GeneratedPlugin::new(
            "standard_prefer_ipv6",
            "prefer_ipv6",
            json!({ "cache": true, "cache_ttl": 3600 }),
        ));
    }

    let mut groups = BTreeMap::new();
    for group in &intent.upstream_groups {
        let enabled_upstreams: Vec<_> = group
            .upstreams
            .iter()
            .filter(|upstream| upstream.enabled)
            .collect();
        let tag = standard_tag("forward", &group.id);
        if matches!(group.strategy, StandardUpstreamStrategy::OrderedFallback)
            && enabled_upstreams.len() > 1
        {
            let mut forward_tags = Vec::with_capacity(enabled_upstreams.len());
            for (index, upstream) in enabled_upstreams.iter().enumerate() {
                let member_tag = format!("{tag}_member_{index}");
                plugins.push(GeneratedPlugin::new(
                    &member_tag,
                    "forward",
                    json!({
                        "upstreams": [compiled_upstream(upstream)],
                        "concurrent": 1,
                        "response_selection": "balanced",
                    }),
                ));
                forward_tags.push(member_tag);
            }
            let mut secondary = forward_tags.last().cloned().expect("validated group");
            for index in (0..forward_tags.len() - 1).rev() {
                let fallback_tag = if index == 0 {
                    tag.clone()
                } else {
                    format!("{tag}_fallback_{index}")
                };
                plugins.push(GeneratedPlugin::new(
                    &fallback_tag,
                    "fallback",
                    json!({
                        "primary": forward_tags[index],
                        "secondary": secondary,
                        "threshold": 500,
                    }),
                ));
                secondary = fallback_tag;
            }
        } else {
            let upstreams: Vec<_> = enabled_upstreams
                .iter()
                .map(|upstream| compiled_upstream(upstream))
                .collect();
            plugins.push(GeneratedPlugin::new(
                &tag,
                "forward",
                json!({
                    "upstreams": upstreams,
                    "concurrent": upstreams.len().clamp(1, 3),
                    "response_selection": strategy_name(group.strategy),
                }),
            ));
        }
        groups.insert(group.id.clone(), group);
        tag_map.upstream_groups.insert(group.id.clone(), tag);
    }

    let advanced = compile_advanced_rules(intent, &learning.tail, &mut plugins, &mut tag_map);

    for path in &intent.paths {
        let forward_tag = tag_map
            .upstream_groups
            .get(&path.upstream_group_id)
            .expect("validated path group should have a forward tag")
            .clone();
        let tag = compile_path_bundle(
            path,
            intent,
            &forward_tag,
            &path.id,
            &mut plugins,
            &tag_map,
            PathOverrides {
                tail: learning
                    .tail
                    .iter()
                    .cloned()
                    .chain(
                        advanced
                            .response_tails
                            .get(&path.id)
                            .into_iter()
                            .flatten()
                            .cloned(),
                    )
                    .collect(),
                ..PathOverrides::default()
            },
        );
        if path_cache_enabled(path, intent) {
            tag_map
                .caches
                .insert(path.id.clone(), standard_tag("cache", &path.id));
        }
        tag_map.paths.insert(path.id.clone(), tag);
    }

    let dedicated_routes =
        compile_dedicated_groups(intent, &mut plugins, &mut tag_map, &learning.tail);

    let default_path = intent.paths.first().expect("validated path should exist");
    let default_path_tag = tag_map
        .paths
        .get(&default_path.id)
        .expect("default path should have a tag")
        .clone();

    let ddns_action_tag = if intent.local.ddns.enabled {
        let path = intent
            .local
            .ddns
            .path_id
            .as_ref()
            .and_then(|path_id| intent.paths.iter().find(|path| &path.id == path_id))
            .unwrap_or(default_path);
        let forward_tag = tag_map
            .upstream_groups
            .get(&path.upstream_group_id)
            .expect("validated DDNS path should have a forward tag");
        let tag = "standard_local_ddns_action";
        let sequence = build_path_sequence(
            path,
            intent,
            forward_tag,
            &tag_map,
            PathOverrides {
                disable_cache: true,
                response_ttl_tag: Some("standard_local_ddns_ttl".to_string()),
                ..PathOverrides::default()
            },
        );
        plugins.push(GeneratedPlugin::new(
            tag,
            "sequence",
            JsonValue::Array(sequence),
        ));
        tag_map
            .local
            .insert("ddnsAction".to_string(), tag.to_string());
        Some(tag.to_string())
    } else {
        None
    };

    let mut exception_action_tags = BTreeMap::<String, String>::new();
    for rule in intent.exceptions.iter().filter(|rule| rule.enabled) {
        let matcher = compiled_matcher(&rule.condition);
        let matcher_tag = standard_tag("exception_match", &rule.id);
        plugins.push(GeneratedPlugin::new(
            &matcher_tag,
            matcher.kind,
            matcher.args,
        ));
        tag_map.exception_rules.insert(rule.id.clone(), matcher_tag);

        if !matches!(
            rule.action,
            StandardRuleAction::UsePath { .. } | StandardRuleAction::UseDefaultPath
        ) {
            let tag = standard_tag("exception_action", &rule.id);
            let sequence =
                build_exception_sequence(rule, intent, default_path, &default_path_tag, &tag_map);
            plugins.push(GeneratedPlugin::new(
                &tag,
                "sequence",
                JsonValue::Array(sequence),
            ));
            exception_action_tags.insert(rule.id.clone(), tag);
        }
    }

    let mut device_action_tags = BTreeMap::<String, String>::new();
    for device in intent
        .devices
        .iter()
        .filter(|device| device_has_policy(device))
    {
        let matcher_tag = standard_tag("device_match", &device.id);
        plugins.push(GeneratedPlugin::new(
            &matcher_tag,
            "client_ip",
            json!(device.addresses),
        ));
        tag_map.devices.insert(device.id.clone(), matcher_tag);

        let path = device
            .assigned_path_id
            .as_ref()
            .and_then(|path_id| intent.paths.iter().find(|path| &path.id == path_id))
            .unwrap_or(default_path);
        let forward_tag = tag_map
            .upstream_groups
            .get(&path.upstream_group_id)
            .expect("validated device path should have a forward tag");
        let action_tag = standard_tag("device_action", &device.id);
        let sequence = build_path_sequence(
            path,
            intent,
            forward_tag,
            &tag_map,
            PathOverrides {
                disable_filtering: matches!(device.filtering, Some(StandardPolicySwitch::Disabled)),
                force_filtering: matches!(device.filtering, Some(StandardPolicySwitch::Enabled)),
                disable_query_log: matches!(device.query_log, Some(StandardPolicySwitch::Disabled)),
                force_query_log: matches!(device.query_log, Some(StandardPolicySwitch::Enabled)),
                ..PathOverrides::default()
            },
        );
        plugins.push(GeneratedPlugin::new(
            &action_tag,
            "sequence",
            JsonValue::Array(sequence),
        ));
        device_action_tags.insert(device.id.clone(), action_tag);
    }

    if intent.routing.enabled {
        for rule in intent.routing.rules.iter().filter(|rule| rule.enabled) {
            let matcher = compiled_matcher(&rule.condition);
            let tag = standard_tag("route_match", &rule.id);
            plugins.push(GeneratedPlugin::new(&tag, matcher.kind, matcher.args));
            tag_map.routing_rules.insert(rule.id.clone(), tag);
        }
    }
    let learned_routes = compile_dynamic_learning_routes(intent, &mut plugins, &mut tag_map);

    let smart_targets = compile_smart_routing(intent, &mut plugins, &mut tag_map);

    let mut main_sequence = Vec::new();
    if tag_map.system.iter().any(|tag| tag == "standard_metrics") {
        main_sequence.push(json!({ "exec": "$standard_metrics" }));
    }
    if tag_map.query_log.is_some() {
        main_sequence.push(json!({ "exec": "$standard_recorder" }));
    }
    for key in ["hosts", "records", "redirect"] {
        if let Some(tag) = tag_map.local.get(key) {
            main_sequence.push(json!({ "exec": format!("${tag}") }));
        }
    }
    if let (Some(matcher), Some(action)) = (
        tag_map.local.get("qtypeMatcher"),
        tag_map.local.get("qtypeAction"),
    ) {
        main_sequence.push(json!({
            "matches": format!("${matcher}"),
            "exec": format!("${action}"),
        }));
    }
    for rule in ordered_exceptions(intent).into_iter().filter(|rule| {
        matches!(
            rule.action,
            StandardRuleAction::Block
                | StandardRuleAction::Allow
                | StandardRuleAction::SkipFiltering
        )
    }) {
        let Some(match_tag) = tag_map.exception_rules.get(&rule.id) else {
            continue;
        };
        let exec_tag = match &rule.action {
            StandardRuleAction::UsePath { path_id } => tag_map.paths.get(path_id),
            StandardRuleAction::UseDefaultPath => Some(&default_path_tag),
            _ => exception_action_tags.get(&rule.id),
        };
        if let Some(exec_tag) = exec_tag {
            main_sequence.push(json!({
                "matches": format!("${match_tag}"),
                "exec": format!("${exec_tag}"),
            }));
        }
    }
    if let (Some(matcher), Some(action)) =
        (tag_map.local.get("ddnsMatcher"), ddns_action_tag.as_ref())
    {
        main_sequence.push(json!({
            "matches": format!("${matcher}"),
            "exec": format!("${action}"),
        }));
    }
    if let Some(targets) = smart_targets.as_ref()
        && let (Some(matcher), Some(action)) =
            (targets.ddns_matcher.as_ref(), targets.ddns_action.as_ref())
    {
        main_sequence.push(json!({
            "matches": format!("${matcher}"),
            "exec": format!("${action}"),
        }));
    }
    for device in intent
        .devices
        .iter()
        .filter(|device| device_has_policy(device))
    {
        if let (Some(match_tag), Some(exec_tag)) = (
            tag_map.devices.get(&device.id),
            device_action_tags.get(&device.id),
        ) {
            main_sequence.push(json!({
                "matches": format!("${match_tag}"),
                "exec": format!("${exec_tag}"),
            }));
        }
    }
    for (matcher, action) in &dedicated_routes {
        main_sequence.push(json!({
            "matches": format!("${matcher}"),
            "exec": format!("${action}"),
        }));
    }
    for rule in ordered_exceptions(intent).into_iter().filter(|rule| {
        !matches!(
            rule.action,
            StandardRuleAction::Block
                | StandardRuleAction::Allow
                | StandardRuleAction::SkipFiltering
        )
    }) {
        let Some(match_tag) = tag_map.exception_rules.get(&rule.id) else {
            continue;
        };
        let exec_tag = match &rule.action {
            StandardRuleAction::UsePath { path_id } => tag_map.paths.get(path_id),
            StandardRuleAction::UseDefaultPath => Some(&default_path_tag),
            _ => exception_action_tags.get(&rule.id),
        };
        if let Some(exec_tag) = exec_tag {
            main_sequence.push(json!({
                "matches": format!("${match_tag}"),
                "exec": format!("${exec_tag}"),
            }));
        }
    }
    if intent.routing.enabled {
        for rule in intent.routing.rules.iter().filter(|rule| rule.enabled) {
            let Some(match_tag) = tag_map.routing_rules.get(&rule.id) else {
                continue;
            };
            let target = match &rule.action {
                StandardRuleAction::UsePath { path_id } => tag_map.paths.get(path_id),
                StandardRuleAction::UseDefaultPath => Some(&default_path_tag),
                _ => None,
            };
            if let Some(target) = target {
                main_sequence.push(json!({
                    "matches": format!("${match_tag}"),
                    "exec": format!("${target}"),
                }));
            }
        }
    }
    for (matches, action) in advanced.request_routes {
        main_sequence.push(json!({ "matches": matches, "exec": format!("${action}") }));
    }
    for (matcher, action) in learned_routes {
        main_sequence.push(json!({
            "matches": format!("${matcher}"),
            "exec": format!("${action}"),
        }));
    }
    if let Some(targets) = smart_targets.as_ref() {
        for (matcher, action) in &targets.semantic_routes {
            main_sequence.push(json!({
                "matches": format!("${matcher}"),
                "exec": format!("${action}"),
            }));
        }
        main_sequence.push(json!({ "exec": format!("${}", targets.unknown_action) }));
    } else {
        main_sequence.push(json!({ "exec": format!("${default_path_tag}") }));
    }
    main_sequence.push(json!({ "exec": "accept" }));
    plugins.push(GeneratedPlugin::new(
        "standard_main_sequence",
        "sequence",
        JsonValue::Array(main_sequence),
    ));

    if intent.listen.udp {
        plugins.push(GeneratedPlugin::new(
            "standard_udp",
            "udp_server",
            json!({
                "listen": intent.listen.address,
                "entry": "standard_main_sequence",
            }),
        ));
    }
    if intent.listen.tcp {
        plugins.push(GeneratedPlugin::new(
            "standard_tcp",
            "tcp_server",
            json!({
                "listen": intent.listen.address,
                "entry": "standard_main_sequence",
            }),
        ));
    }

    let yaml = serialize_generated_config(intent, base, &plugins)?;
    let generated_tags = plugins.iter().map(|plugin| plugin.tag.clone()).collect();
    let summary = summarize(intent);
    let plugin_count = plugins.len();
    let _ = filtering;
    Ok(StandardGeneratedConfig {
        config_version: config_version(&yaml),
        yaml,
        plugin_count,
        generated_tags,
        tag_map,
        summary,
        managed_files: learning.managed_files,
    })
}

fn compile_filtering(
    intent: &StandardIntent,
    plugins: &mut Vec<GeneratedPlugin>,
    tag_map: &mut StandardTagMap,
) -> bool {
    let should_generate = effective_filtering_used(intent);
    let subscriptions: Vec<_> = intent
        .filtering
        .subscriptions
        .iter()
        .filter(|subscription| subscription.enabled)
        .collect();
    let local_files: Vec<_> = intent
        .filtering
        .local_files
        .iter()
        .filter(|file| file.enabled)
        .collect();
    if should_generate && !subscriptions.is_empty() {
        for subscription in &subscriptions {
            let component = safe_tag_component(&subscription.id);
            let download_tag = format!("standard_filter_download_{component}");
            plugins.push(GeneratedPlugin::new(
                &download_tag,
                "download",
                json!({
                    "startup_if_missing": true,
                    "fail_on_error": true,
                    "downloads": [{
                        "url": subscription.url,
                        "dir": FILTER_SUBSCRIPTION_DIR,
                        "filename": subscription_filename(&subscription.id),
                    }],
                }),
            ));
            tag_map.filtering.push(download_tag);
        }
    }

    let has_rules = should_generate
        && (!intent.filtering.block_rules.is_empty()
            || !intent.filtering.allow_rules.is_empty()
            || !subscriptions.is_empty()
            || !local_files.is_empty());
    if has_rules {
        let mut rules = intent.filtering.block_rules.clone();
        rules.extend(intent.filtering.allow_rules.clone());
        plugins.push(GeneratedPlugin::new(
            "standard_ad_rules",
            "adguard_rule",
            json!({
                "files": subscriptions.iter().map(|subscription| {
                    format!("{FILTER_SUBSCRIPTION_DIR}/{}", subscription_filename(&subscription.id))
                }).chain(local_files.iter().map(|file| file.path.clone())).collect::<Vec<_>>(),
                "rules": rules,
            }),
        ));
        tag_map.filtering.push("standard_ad_rules".to_string());
    }

    let has_block_exception = intent
        .exceptions
        .iter()
        .any(|rule| rule.enabled && matches!(rule.action, StandardRuleAction::Block));
    if has_rules || has_block_exception {
        plugins.push(GeneratedPlugin::new(
            "standard_blocked",
            "black_hole",
            json!({
                "mode": match intent.filtering.block_response {
                    StandardBlockResponse::NullIp => "null",
                    StandardBlockResponse::Nxdomain => "nxdomain",
                    StandardBlockResponse::Nodata => "nodata",
                    StandardBlockResponse::Refused => "refused",
                },
                "short_circuit": true,
            }),
        ));
        tag_map.filtering.push("standard_blocked".to_string());
    }

    if should_generate && !subscriptions.is_empty() {
        plugins.push(GeneratedPlugin::new(
            "standard_filter_reload",
            "reload_provider",
            json!(["$standard_ad_rules"]),
        ));
        tag_map.filtering.push("standard_filter_reload".to_string());
        for subscription in subscriptions {
            let component = safe_tag_component(&subscription.id);
            let download_tag = format!("standard_filter_download_{component}");
            let cron_tag = format!("standard_filter_cron_{component}");
            let job = format!("refresh_filter_{component}");
            plugins.push(GeneratedPlugin::new(
                &cron_tag,
                "cron",
                json!({
                    "jobs": [{
                        "name": job,
                        "interval": format!("{}h", subscription.update_interval_hours.max(1)),
                        "executors": [format!("${download_tag}"), "$standard_filter_reload"],
                        "stop_on_error": true,
                    }],
                }),
            ));
            tag_map.filtering.push(cron_tag.clone());
            tag_map.filter_subscriptions.insert(
                subscription.id.clone(),
                StandardSubscriptionTagMap {
                    download: download_tag,
                    cron: cron_tag,
                    job,
                },
            );
        }
    }
    has_rules
}

#[derive(Default)]
struct DynamicLearningCompilation {
    tail: Vec<JsonValue>,
    managed_files: Vec<String>,
}

#[derive(Default)]
struct AdvancedCompilation {
    request_routes: Vec<(Vec<String>, String)>,
    response_tails: BTreeMap<String, Vec<JsonValue>>,
}

fn compile_advanced_rules(
    intent: &StandardIntent,
    learning_tail: &[JsonValue],
    plugins: &mut Vec<GeneratedPlugin>,
    tag_map: &mut StandardTagMap,
) -> AdvancedCompilation {
    use super::model::{
        StandardAdvancedAction, StandardAdvancedCondition, StandardAdvancedFailurePolicy,
        StandardAdvancedFailureResponse, StandardAdvancedRulePhase,
    };

    let mut result = AdvancedCompilation::default();
    let mut ordered: Vec<_> = intent
        .advanced_rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.enabled)
        .collect();
    ordered.sort_by_key(|(index, rule)| (rule.priority, *index));

    for (_, rule) in ordered {
        let mut matches = Vec::new();
        let mut source_path = None;
        for (condition_index, condition) in rule.conditions.iter().enumerate() {
            if let StandardAdvancedCondition::SourcePath { path_id } = condition {
                source_path = Some(path_id.clone());
                continue;
            }
            let tag = standard_tag(
                "advanced_match",
                &format!("{}_{}", rule.id, condition_index),
            );
            let (kind, args, invert) = match condition {
                StandardAdvancedCondition::Domain { values } => (
                    "qname",
                    json!(
                        values
                            .iter()
                            .map(|value| format!("full:{value}"))
                            .collect::<Vec<_>>()
                    ),
                    false,
                ),
                StandardAdvancedCondition::Suffix { values } => (
                    "qname",
                    json!(
                        values
                            .iter()
                            .map(|value| format!("domain:{value}"))
                            .collect::<Vec<_>>()
                    ),
                    false,
                ),
                StandardAdvancedCondition::Keyword { values } => (
                    "qname",
                    json!(
                        values
                            .iter()
                            .map(|value| format!("keyword:{value}"))
                            .collect::<Vec<_>>()
                    ),
                    false,
                ),
                StandardAdvancedCondition::ClientCidr { values } => {
                    ("client_ip", json!(values), false)
                }
                StandardAdvancedCondition::Qtype { values } => ("qtype", json!(values), false),
                StandardAdvancedCondition::Time { timezone, periods } => (
                    "time",
                    json!({ "timezone": timezone, "periods": periods }),
                    false,
                ),
                StandardAdvancedCondition::RateLimitExceeded {
                    qps,
                    burst,
                    mask4,
                    mask6,
                } => (
                    "rate_limiter",
                    json!({ "qps": qps, "burst": burst, "mask4": mask4, "mask6": mask6 }),
                    true,
                ),
                StandardAdvancedCondition::Cname { values } => ("cname", json!(values), false),
                StandardAdvancedCondition::Rcode { values } => ("rcode", json!(values), false),
                StandardAdvancedCondition::HasWantedAnswer => ("has_wanted_ans", json!({}), false),
                StandardAdvancedCondition::ResponseIpRole { role, invert } => {
                    let provider = tag_map
                        .rule_data
                        .get(role)
                        .expect("validated response IP role");
                    ("resp_ip", json!([format!("${provider}")]), *invert)
                }
                StandardAdvancedCondition::SourcePath { .. } => unreachable!(),
            };
            plugins.push(GeneratedPlugin::new(&tag, kind, args));
            matches.push(format!("{}${tag}", if invert { "!" } else { "" }));
        }

        let action_tag = standard_tag("advanced_action", &rule.id);
        match rule.phase {
            StandardAdvancedRulePhase::Request => {
                match &rule.action {
                    StandardAdvancedAction::UsePath { path_id } => {
                        plugins.push(GeneratedPlugin::new(
                            &action_tag,
                            "sequence",
                            json!([
                                { "exec": format!("${}", standard_tag("path", path_id)) },
                                { "exec": "accept" }
                            ]),
                        ));
                    }
                    StandardAdvancedAction::Block { response } => {
                        plugins.push(GeneratedPlugin::new(
                            &action_tag,
                            "black_hole",
                            json!({ "mode": block_response_mode(*response), "short_circuit": true }),
                        ));
                    }
                }
                result.request_routes.push((matches, action_tag.clone()));
            }
            StandardAdvancedRulePhase::Response => {
                let StandardAdvancedAction::UsePath { path_id } = &rule.action else {
                    unreachable!("validated response rule action");
                };
                let target = intent
                    .paths
                    .iter()
                    .find(|path| &path.id == path_id)
                    .expect("validated advanced target path");
                let forward = tag_map
                    .upstream_groups
                    .get(&target.upstream_group_id)
                    .expect("validated advanced target upstream");
                let drop_tag = standard_tag("advanced_drop", &rule.id);
                plugins.push(GeneratedPlugin::new(
                    &drop_tag,
                    "drop_resp",
                    json!({ "reason": format!("advanced_rule_{}", rule.id) }),
                ));
                let target_tag = compile_path_bundle(
                    target,
                    intent,
                    forward,
                    &format!("advanced_target_{}", rule.id),
                    plugins,
                    tag_map,
                    PathOverrides {
                        prelude: vec![json!({ "exec": format!("${drop_tag}") })],
                        tail: learning_tail.to_vec(),
                        ..PathOverrides::default()
                    },
                );
                let secondary = standard_tag("advanced_secondary", &rule.id);
                match rule.failure_policy {
                    StandardAdvancedFailurePolicy::FailOpen => plugins.push(GeneratedPlugin::new(
                        &secondary,
                        "sequence",
                        json!([{ "exec": "accept" }]),
                    )),
                    StandardAdvancedFailurePolicy::FailClosed => {
                        let mode = match rule.failure_response {
                            StandardAdvancedFailureResponse::Servfail => "servfail",
                            StandardAdvancedFailureResponse::Refused => "refused",
                        };
                        plugins.push(GeneratedPlugin::new(
                            &secondary,
                            "black_hole",
                            json!({ "mode": mode, "short_circuit": true }),
                        ));
                    }
                }
                plugins.push(GeneratedPlugin::new(
                    &action_tag,
                    "fallback",
                    json!({
                        "primary": target_tag,
                        "secondary": secondary,
                        "threshold": 60_000,
                        "short_circuit": true,
                        "fallback_on_timeout": false,
                        "fallback_on_error": true,
                        "fallback_on_no_response": true,
                    }),
                ));
                result
                    .response_tails
                    .entry(source_path.expect("validated response source path"))
                    .or_default()
                    .push(json!({ "matches": matches, "exec": format!("${action_tag}") }));
            }
        }
        tag_map.advanced_rules.insert(rule.id.clone(), action_tag);
    }
    result
}

fn compile_dynamic_learning_primitives(
    intent: &StandardIntent,
    plugins: &mut Vec<GeneratedPlugin>,
    tag_map: &mut StandardTagMap,
) -> DynamicLearningCompilation {
    let mut result = DynamicLearningCompilation::default();
    let mut ordered: Vec<_> = intent
        .dynamic_learning
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| profile.enabled)
        .collect();
    ordered.sort_by_key(|(index, profile)| (profile.priority, *index));

    for (_, profile) in ordered {
        let component = safe_tag_component(&profile.id);
        let provider_tag = standard_tag("learn_provider", &profile.id);
        let learner_tag = standard_tag("learn_exec", &profile.id);
        let matcher_tag = standard_tag("learn_match", &profile.id);
        let qtype_tag = standard_tag("learn_qtype", &profile.id);
        let rcode_tag = standard_tag("learn_rcode", &profile.id);
        let answer_tag = standard_tag("learn_answer", &profile.id);
        let response_ip_tag = profile
            .response_ip_role
            .as_ref()
            .map(|_| standard_tag("learn_resp_ip", &profile.id));
        let rules_path = format!("{DYNAMIC_LEARNING_DIR}/{component}.txt");
        let metadata_path = format!("{DYNAMIC_LEARNING_DIR}/{component}.meta.json");

        plugins.push(GeneratedPlugin::new(
            &provider_tag,
            "dynamic_domain_set",
            json!({
                "path": rules_path,
                "metadata_path": metadata_path,
                "max_entries": profile.max_entries,
                "entry_ttl_seconds": profile.entry_ttl_seconds,
                "cleanup_interval_seconds": profile.cleanup_interval_seconds,
                "queue_size": profile.queue_size,
                "batch_size": profile.batch_size,
                "flush_interval_ms": profile.flush_interval_ms,
            }),
        ));
        plugins.push(GeneratedPlugin::new(
            &matcher_tag,
            "qname",
            json!([format!("${provider_tag}")]),
        ));
        plugins.push(GeneratedPlugin::new(
            &qtype_tag,
            "qtype",
            json!(profile.qtypes),
        ));
        plugins.push(GeneratedPlugin::new(
            &rcode_tag,
            "rcode",
            json!(profile.rcodes),
        ));
        if profile.answer_required {
            plugins.push(GeneratedPlugin::new(
                &answer_tag,
                "has_wanted_ans",
                json!({}),
            ));
        }
        if let (Some(role), Some(tag)) = (&profile.response_ip_role, response_ip_tag.as_ref()) {
            let provider = tag_map
                .rule_data
                .get(role)
                .expect("validated response IP role should have a provider");
            plugins.push(GeneratedPlugin::new(
                tag,
                "resp_ip",
                json!([format!("${provider}")]),
            ));
        }
        plugins.push(GeneratedPlugin::new(
            &learner_tag,
            "learn_domain",
            json!({
                "provider": provider_tag,
                "phase": "before",
                "questions": "first",
                "qtypes": profile.qtypes,
                "success_only": false,
                "answer_required": false,
                "rule_kind": match profile.rule_kind {
                    super::model::StandardLearningRuleKind::Full => "full",
                    super::model::StandardLearningRuleKind::Domain => "domain",
                },
                "async": matches!(profile.failure_policy, super::model::StandardLearningFailurePolicy::Continue),
                "error_mode": match profile.failure_policy {
                    super::model::StandardLearningFailurePolicy::Continue => "continue",
                    super::model::StandardLearningFailurePolicy::FailClosed => "fail",
                },
                "timeout": "1s",
                "paused": profile.paused,
            }),
        ));

        let mut matches = vec![format!("${qtype_tag}"), format!("${rcode_tag}")];
        if profile.answer_required {
            matches.push(format!("${answer_tag}"));
        }
        if let Some(tag) = response_ip_tag {
            matches.push(format!("${tag}"));
        }
        result.tail.push(json!({
            "matches": matches,
            "exec": format!("${learner_tag}"),
        }));
        result
            .managed_files
            .extend([rules_path.clone(), metadata_path.clone()]);
        tag_map.dynamic_learning.insert(
            profile.id.clone(),
            StandardDynamicLearningTagMap {
                provider: provider_tag,
                learner: learner_tag,
                matcher: matcher_tag,
                action: String::new(),
                rules_path,
                metadata_path,
            },
        );
    }
    result.managed_files.sort();
    result
}

fn compile_dynamic_learning_routes(
    intent: &StandardIntent,
    plugins: &mut Vec<GeneratedPlugin>,
    tag_map: &mut StandardTagMap,
) -> Vec<(String, String)> {
    let mut ordered: Vec<_> = intent
        .dynamic_learning
        .profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| profile.enabled)
        .collect();
    ordered.sort_by_key(|(index, profile)| (profile.priority, *index));
    ordered
        .into_iter()
        .map(|(_, profile)| {
            let target = tag_map
                .paths
                .get(&profile.target_path_id)
                .expect("validated learning target path")
                .clone();
            let action = standard_tag("learn_action", &profile.id);
            plugins.push(GeneratedPlugin::new(
                &action,
                "sequence",
                json!([{ "exec": format!("${target}") }, { "exec": "accept" }]),
            ));
            let entry = tag_map
                .dynamic_learning
                .get_mut(&profile.id)
                .expect("learning tags compiled before routes");
            entry.action = action.clone();
            (entry.matcher.clone(), action)
        })
        .collect()
}

fn compile_dedicated_groups(
    intent: &StandardIntent,
    plugins: &mut Vec<GeneratedPlugin>,
    tag_map: &mut StandardTagMap,
    learning_tail: &[JsonValue],
) -> Vec<(String, String)> {
    let mut routes = Vec::new();
    for (index, group) in intent
        .dedicated_groups
        .iter()
        .enumerate()
        .filter(|(_, group)| group.enabled)
    {
        let provider_tag = standard_tag("dedicated_provider", &group.id);
        let matcher_tag = standard_tag("dedicated_match", &group.id);
        let forward_tag = standard_tag("dedicated_forward", &group.id);
        let namespace = format!("dedicated:{}", group.id);

        plugins.push(GeneratedPlugin::new(
            &provider_tag,
            "domain_set",
            json!({ "exps": group.rules, "files": [], "sets": [] }),
        ));
        plugins.push(GeneratedPlugin::new(
            &matcher_tag,
            "qname",
            json!([format!("${provider_tag}")]),
        ));
        compile_embedded_forward(&forward_tag, group.strategy, &group.upstreams, plugins);

        let path = StandardResolutionPath {
            id: format!("dedicated_{}", group.id),
            name: group.name.clone(),
            description: group.description.clone(),
            upstream_group_id: group.id.clone(),
            filtering: group.path.filtering,
            cache: group.path.cache,
            query_log: group.path.query_log,
            dual_stack: group.path.dual_stack,
            ip_selection: group.path.ip_selection.clone(),
            ecs: group.path.ecs.clone(),
        };
        let path_tag = compile_path_bundle(
            &path,
            intent,
            &forward_tag,
            &namespace,
            plugins,
            tag_map,
            PathOverrides {
                tail: learning_tail.to_vec(),
                ..PathOverrides::default()
            },
        );
        let cache_tag =
            path_cache_enabled(&path, intent).then(|| path_bundle_tag("cache", &namespace));
        if let Some(cache_tag) = cache_tag.as_ref() {
            tag_map
                .caches
                .insert(format!("dedicated:{}", group.id), cache_tag.clone());
        }

        let entry_tag = standard_tag("dedicated_entry", &group.id);
        let mut entry = Vec::new();
        if tag_map.query_log.is_some() {
            entry.push(json!({ "exec": "$standard_recorder" }));
        }
        entry.push(json!({ "exec": format!("${path_tag}") }));
        entry.push(json!({ "exec": "accept" }));
        plugins.push(GeneratedPlugin::new(
            &entry_tag,
            "sequence",
            JsonValue::Array(entry),
        ));

        let mut udp_listener = None;
        let mut tcp_listener = None;
        if group.listener.enabled && group.listener.udp {
            let tag = standard_tag("dedicated_udp", &group.id);
            plugins.push(GeneratedPlugin::new(
                &tag,
                "udp_server",
                json!({ "listen": group.listener.address, "entry": entry_tag }),
            ));
            udp_listener = Some(tag);
        }
        if group.listener.enabled && group.listener.tcp {
            let tag = standard_tag("dedicated_tcp", &group.id);
            plugins.push(GeneratedPlugin::new(
                &tag,
                "tcp_server",
                json!({ "listen": group.listener.address, "entry": entry_tag }),
            ));
            tcp_listener = Some(tag);
        }

        tag_map.dedicated_groups.insert(
            group.id.clone(),
            StandardDedicatedTagMap {
                provider: provider_tag,
                matcher: matcher_tag.clone(),
                upstream_group: forward_tag,
                path: path_tag.clone(),
                entry: entry_tag,
                cache: cache_tag,
                udp_listener,
                tcp_listener,
            },
        );
        routes.push((group.priority, index, matcher_tag, path_tag));
    }
    routes.sort_by_key(|(priority, index, _, _)| (*priority, *index));
    routes
        .into_iter()
        .map(|(_, _, matcher, action)| (matcher, action))
        .collect()
}

fn compile_embedded_forward(
    tag: &str,
    strategy: StandardUpstreamStrategy,
    upstreams: &[StandardUpstream],
    plugins: &mut Vec<GeneratedPlugin>,
) {
    let enabled: Vec<_> = upstreams
        .iter()
        .filter(|upstream| upstream.enabled)
        .collect();
    if matches!(strategy, StandardUpstreamStrategy::OrderedFallback) && enabled.len() > 1 {
        let mut forward_tags = Vec::with_capacity(enabled.len());
        for (index, upstream) in enabled.iter().enumerate() {
            let member_tag = format!("{tag}_member_{index}");
            plugins.push(GeneratedPlugin::new(
                &member_tag,
                "forward",
                json!({
                    "upstreams": [compiled_upstream(upstream)],
                    "concurrent": 1,
                    "response_selection": "balanced",
                }),
            ));
            forward_tags.push(member_tag);
        }
        let mut secondary = forward_tags.last().cloned().expect("validated group");
        for index in (0..forward_tags.len() - 1).rev() {
            let fallback_tag = if index == 0 {
                tag.to_string()
            } else {
                format!("{tag}_fallback_{index}")
            };
            plugins.push(GeneratedPlugin::new(
                &fallback_tag,
                "fallback",
                json!({
                    "primary": forward_tags[index],
                    "secondary": secondary,
                    "threshold": 500,
                }),
            ));
            secondary = fallback_tag;
        }
    } else {
        let compiled: Vec<_> = enabled
            .iter()
            .map(|upstream| compiled_upstream(upstream))
            .collect();
        plugins.push(GeneratedPlugin::new(
            tag,
            "forward",
            json!({
                "upstreams": compiled,
                "concurrent": compiled.len().clamp(1, 3),
                "response_selection": strategy_name(strategy),
            }),
        ));
    }
}

fn compile_rule_data(
    intent: &StandardIntent,
    plugins: &mut Vec<GeneratedPlugin>,
    tag_map: &mut StandardTagMap,
) {
    for (role_name, role) in intent.rule_data.all_roles() {
        let enabled: Vec<_> = role
            .sources
            .iter()
            .filter(|source| source.enabled())
            .collect();
        if enabled.is_empty() {
            continue;
        }
        let is_ip = role_name == "domestic_ips";
        let role_component = safe_tag_component(role_name);
        let role_tag = format!("standard_rule_data_{role_component}");
        let mut rules = Vec::new();
        let mut files = Vec::new();
        let mut sets = Vec::new();
        let mut subscriptions = Vec::new();

        for source in enabled {
            let source_component = safe_tag_component(source.id());
            let source_key = format!("{role_name}:{}", source.id());
            match source {
                StandardRuleDataSource::Manual {
                    rules: source_rules,
                    ..
                } => rules.extend(source_rules.iter().cloned()),
                StandardRuleDataSource::LocalFile { path, .. } => files.push(path.clone()),
                StandardRuleDataSource::Subscription {
                    url,
                    update_interval_hours,
                    ..
                } => {
                    let filename = rule_data_subscription_filename(role_name, source.id());
                    let download_tag =
                        format!("standard_rule_data_download_{role_component}_{source_component}");
                    let cron_tag =
                        format!("standard_rule_data_cron_{role_component}_{source_component}");
                    let job = format!("refresh_rule_data_{role_component}_{source_component}");
                    plugins.push(GeneratedPlugin::new(
                        &download_tag,
                        "download",
                        json!({
                            "startup_if_missing": true,
                            "fail_on_error": true,
                            "downloads": [{
                                "url": url,
                                "dir": RULE_DATA_SUBSCRIPTION_DIR,
                                "filename": filename,
                            }],
                        }),
                    ));
                    files.push(format!("{RULE_DATA_SUBSCRIPTION_DIR}/{filename}"));
                    subscriptions.push((
                        source_key,
                        download_tag,
                        cron_tag,
                        job,
                        *update_interval_hours,
                    ));
                }
                StandardRuleDataSource::NativeDat {
                    path, selectors, ..
                } => {
                    let source_tag =
                        format!("standard_rule_data_native_{role_component}_{source_component}");
                    plugins.push(GeneratedPlugin::new(
                        &source_tag,
                        if is_ip { "geoip" } else { "geosite" },
                        json!({ "file": path, "selectors": selectors }),
                    ));
                    sets.push(source_tag);
                }
            }
        }

        plugins.push(GeneratedPlugin::new(
            &role_tag,
            if is_ip { "ip_set" } else { "domain_set" },
            if is_ip {
                json!({ "ips": rules, "files": files, "sets": sets })
            } else {
                json!({ "exps": rules, "files": files, "sets": sets })
            },
        ));
        tag_map
            .rule_data
            .insert(role_name.to_string(), role_tag.clone());

        if !subscriptions.is_empty() {
            let reload_tag = format!("standard_rule_data_reload_{role_component}");
            plugins.push(GeneratedPlugin::new(
                &reload_tag,
                "reload_provider",
                json!([format!("${role_tag}")]),
            ));
            for (source_key, download_tag, cron_tag, job, interval) in subscriptions {
                plugins.push(GeneratedPlugin::new(
                    &cron_tag,
                    "cron",
                    json!({
                        "jobs": [{
                            "name": job,
                            "interval": format!("{}h", interval.max(1)),
                            "executors": [format!("${download_tag}"), format!("${reload_tag}")],
                            "stop_on_error": true,
                        }],
                    }),
                ));
                tag_map.rule_data_sources.insert(
                    source_key,
                    StandardSubscriptionTagMap {
                        download: download_tag,
                        cron: cron_tag,
                        job,
                    },
                );
            }
        }
    }
}

fn compile_local_plugins(
    intent: &StandardIntent,
    plugins: &mut Vec<GeneratedPlugin>,
    tag_map: &mut StandardTagMap,
) {
    let local = &intent.local;
    if !local.hosts.entries.is_empty() || !local.hosts.files.is_empty() {
        plugins.push(GeneratedPlugin::new(
            "standard_local_hosts",
            "hosts",
            json!({
                "entries": local.hosts.entries,
                "files": local.hosts.files,
                "short_circuit": true,
            }),
        ));
        tag_map
            .local
            .insert("hosts".to_string(), "standard_local_hosts".to_string());
    }
    if !local.records.rules.is_empty() || !local.records.files.is_empty() {
        plugins.push(GeneratedPlugin::new(
            "standard_local_records",
            "arbitrary",
            json!({
                "rules": local.records.rules,
                "files": local.records.files,
                "short_circuit": true,
            }),
        ));
        tag_map
            .local
            .insert("records".to_string(), "standard_local_records".to_string());
    }
    if !local.redirects.rules.is_empty() || !local.redirects.files.is_empty() {
        plugins.push(GeneratedPlugin::new(
            "standard_local_redirect",
            "redirect",
            json!({
                "rules": local.redirects.rules,
                "files": local.redirects.files,
            }),
        ));
        tag_map.local.insert(
            "redirect".to_string(),
            "standard_local_redirect".to_string(),
        );
    }
    if local.response_ttl.enabled {
        plugins.push(GeneratedPlugin::new(
            "standard_local_response_ttl",
            "ttl",
            json!({
                "min": local.response_ttl.min,
                "max": local.response_ttl.max,
            }),
        ));
        tag_map.local.insert(
            "responseTtl".to_string(),
            "standard_local_response_ttl".to_string(),
        );
    }
    if local.qtype_policy.enabled {
        plugins.push(GeneratedPlugin::new(
            "standard_local_qtype_match",
            "qtype",
            json!(local.qtype_policy.qtypes),
        ));
        plugins.push(GeneratedPlugin::new(
            "standard_local_qtype_action",
            "black_hole",
            json!({
                "mode": block_response_mode(local.qtype_policy.response),
                "short_circuit": true,
            }),
        ));
        tag_map.local.insert(
            "qtypeMatcher".to_string(),
            "standard_local_qtype_match".to_string(),
        );
        tag_map.local.insert(
            "qtypeAction".to_string(),
            "standard_local_qtype_action".to_string(),
        );
    }
    if local.ddns.enabled {
        plugins.push(GeneratedPlugin::new(
            "standard_local_ddns_match",
            "qname",
            json!(
                local
                    .ddns
                    .domains
                    .iter()
                    .map(|domain| format!("full:{domain}"))
                    .collect::<Vec<_>>()
            ),
        ));
        plugins.push(GeneratedPlugin::new(
            "standard_local_ddns_ttl",
            "ttl",
            json!({ "fix": local.ddns.ttl }),
        ));
        tag_map.local.insert(
            "ddnsMatcher".to_string(),
            "standard_local_ddns_match".to_string(),
        );
        tag_map
            .local
            .insert("ddnsTtl".to_string(), "standard_local_ddns_ttl".to_string());
    }
}

fn block_response_mode(response: StandardBlockResponse) -> &'static str {
    match response {
        StandardBlockResponse::NullIp => "null",
        StandardBlockResponse::Nxdomain => "nxdomain",
        StandardBlockResponse::Nodata => "nodata",
        StandardBlockResponse::Refused => "refused",
    }
}

struct SmartRoutingTargets {
    semantic_routes: Vec<(String, String)>,
    unknown_action: String,
    ddns_matcher: Option<String>,
    ddns_action: Option<String>,
}

fn compile_smart_routing(
    intent: &StandardIntent,
    plugins: &mut Vec<GeneratedPlugin>,
    tag_map: &mut StandardTagMap,
) -> Option<SmartRoutingTargets> {
    let smart = &intent.smart_routing;
    if !smart.enabled {
        return None;
    }
    let domestic_path = intent
        .paths
        .iter()
        .find(|path| Some(path.id.as_str()) == smart.domestic_path_id.as_deref())
        .expect("validated domestic smart-routing path");
    let remote_path = intent
        .paths
        .iter()
        .find(|path| Some(path.id.as_str()) == smart.remote_path_id.as_deref())
        .expect("validated remote smart-routing path");
    let domestic_forward = tag_map
        .upstream_groups
        .get(&domestic_path.upstream_group_id)
        .expect("validated domestic upstream group")
        .clone();
    let remote_forward = tag_map
        .upstream_groups
        .get(&remote_path.upstream_group_id)
        .expect("validated remote upstream group")
        .clone();

    let address_qtype = "standard_smart_address_qtype";
    let rcode_noerror = "standard_smart_rcode_noerror";
    let rcode_nxdomain = "standard_smart_rcode_nxdomain";
    let rcode_servfail = "standard_smart_rcode_servfail";
    let has_wanted = "standard_smart_has_wanted_answer";
    let has_cname = "standard_smart_has_cname";
    let domestic_resp_ip = "standard_smart_domestic_response_ip";
    plugins.extend([
        GeneratedPlugin::new(address_qtype, "qtype", json!(["A", "AAAA"])),
        GeneratedPlugin::new(rcode_noerror, "rcode", json!(["NOERROR"])),
        GeneratedPlugin::new(rcode_nxdomain, "rcode", json!(["NXDOMAIN"])),
        GeneratedPlugin::new(rcode_servfail, "rcode", json!(["SERVFAIL"])),
        GeneratedPlugin::new(has_wanted, "has_wanted_ans", json!({})),
        GeneratedPlugin::new(has_cname, "cname", json!(["regexp:.*"])),
        GeneratedPlugin::new(
            domestic_resp_ip,
            "resp_ip",
            json!([format!(
                "${}",
                tag_map
                    .rule_data
                    .get("domestic_ips")
                    .expect("validated domestic IP role")
            )]),
        ),
    ]);

    let mut drop_tags = BTreeMap::new();
    for reason in [
        "domestic_ip_mismatch",
        "cname_only",
        "nodata",
        "nxdomain",
        "servfail",
    ] {
        let tag = format!("standard_smart_drop_{reason}");
        plugins.push(GeneratedPlugin::new(
            &tag,
            "drop_resp",
            json!({ "reason": reason }),
        ));
        drop_tags.insert(reason, tag);
    }

    let validation_tail = smart_validation_tail(
        smart,
        address_qtype,
        rcode_noerror,
        rcode_nxdomain,
        rcode_servfail,
        has_wanted,
        has_cname,
        domestic_resp_ip,
        &drop_tags,
    );

    let compile_variant = |path: &StandardResolutionPath,
                           forward: &str,
                           namespace: &str,
                           tail: Vec<JsonValue>,
                           plugins: &mut Vec<GeneratedPlugin>,
                           tag_map: &mut StandardTagMap| {
        let tag = compile_path_bundle(
            path,
            intent,
            forward,
            namespace,
            plugins,
            tag_map,
            PathOverrides {
                tail,
                ..PathOverrides::default()
            },
        );
        if path_cache_enabled(path, intent) {
            tag_map.caches.insert(
                format!("smart:{namespace}"),
                standard_tag("cache", namespace),
            );
        }
        tag_map
            .smart_routing
            .insert(namespace.to_string(), tag.clone());
        tag
    };

    let domestic_primary = compile_variant(
        domestic_path,
        &domestic_forward,
        "smart_domestic_primary",
        validation_tail.clone(),
        plugins,
        tag_map,
    );
    let domestic_remote = compile_variant(
        remote_path,
        &remote_forward,
        "smart_domestic_remote_fallback",
        Vec::new(),
        plugins,
        tag_map,
    );
    let domestic_action = "standard_smart_domestic_fallback".to_string();
    plugins.push(GeneratedPlugin::new(
        &domestic_action,
        "fallback",
        json!({
            "primary": domestic_primary,
            "secondary": domestic_remote,
            "threshold": smart.fallback_threshold_ms,
            "short_circuit": true,
            "fallback_on_timeout": smart.response_policy.timeout,
            "fallback_on_error": smart.response_policy.transport_failure,
            "fallback_on_no_response": true,
        }),
    ));
    tag_map
        .smart_routing
        .insert("domesticAction".to_string(), domestic_action.clone());

    let remote_action = compile_variant(
        remote_path,
        &remote_forward,
        "smart_remote",
        Vec::new(),
        plugins,
        tag_map,
    );

    let unknown_action = match smart.unknown_mode {
        StandardUnknownMode::CompatibilityFirst => {
            let primary = compile_variant(
                domestic_path,
                &domestic_forward,
                "unknown_compatibility_domestic",
                validation_tail.clone(),
                plugins,
                tag_map,
            );
            let secondary = compile_variant(
                remote_path,
                &remote_forward,
                "unknown_compatibility_remote",
                Vec::new(),
                plugins,
                tag_map,
            );
            let tag = "standard_smart_unknown_compatibility".to_string();
            plugins.push(GeneratedPlugin::new(
                &tag,
                "fallback",
                json!({
                    "primary": primary,
                    "secondary": secondary,
                    "threshold": smart.fallback_threshold_ms,
                    "short_circuit": true,
                    "fallback_on_timeout": smart.response_policy.timeout,
                    "fallback_on_error": smart.response_policy.transport_failure,
                    "fallback_on_no_response": true,
                }),
            ));
            tag
        }
        StandardUnknownMode::PrivacyFirst if smart.privacy_fallback_to_domestic => {
            let primary = compile_variant(
                remote_path,
                &remote_forward,
                "unknown_privacy_remote",
                Vec::new(),
                plugins,
                tag_map,
            );
            let secondary = compile_variant(
                domestic_path,
                &domestic_forward,
                "unknown_privacy_domestic",
                validation_tail.clone(),
                plugins,
                tag_map,
            );
            let tag = "standard_smart_unknown_privacy".to_string();
            plugins.push(GeneratedPlugin::new(
                &tag,
                "fallback",
                json!({
                    "primary": primary,
                    "secondary": secondary,
                    "threshold": smart.fallback_threshold_ms,
                    "short_circuit": true,
                    "fallback_on_timeout": smart.response_policy.timeout,
                    "fallback_on_error": smart.response_policy.transport_failure,
                    "fallback_on_no_response": true,
                }),
            ));
            tag
        }
        StandardUnknownMode::PrivacyFirst => compile_variant(
            remote_path,
            &remote_forward,
            "unknown_privacy_remote",
            Vec::new(),
            plugins,
            tag_map,
        ),
        StandardUnknownMode::StrictRemote => compile_variant(
            remote_path,
            &remote_forward,
            "unknown_strict_remote",
            Vec::new(),
            plugins,
            tag_map,
        ),
    };
    tag_map
        .smart_routing
        .insert("unknownAction".to_string(), unknown_action.clone());

    let mut semantic_routes = Vec::new();
    for (role, action) in [
        ("domestic_domains", domestic_action.as_str()),
        ("foreign_domains", remote_action.as_str()),
        ("direct_domains", domestic_action.as_str()),
        ("remote_domains", remote_action.as_str()),
    ] {
        let Some(provider_tag) = tag_map.rule_data.get(role) else {
            continue;
        };
        let matcher_tag = format!("standard_smart_match_{}", safe_tag_component(role));
        plugins.push(GeneratedPlugin::new(
            &matcher_tag,
            "qname",
            json!([format!("${provider_tag}")]),
        ));
        tag_map
            .smart_routing
            .insert(format!("matcher:{role}"), matcher_tag.clone());
        semantic_routes.push((matcher_tag, action.to_string()));
    }

    let (ddns_matcher, ddns_action) =
        if let Some(provider_tag) = tag_map.rule_data.get("ddns_domains").cloned() {
            let matcher = "standard_smart_match_ddns_domains".to_string();
            plugins.push(GeneratedPlugin::new(
                &matcher,
                "qname",
                json!([format!("${provider_tag}")]),
            ));
            let ttl_tag = "standard_smart_ddns_ttl";
            plugins.push(GeneratedPlugin::new(
                ttl_tag,
                "ttl",
                json!({ "fix": intent.local.ddns.ttl }),
            ));
            let action = compile_path_bundle(
                domestic_path,
                intent,
                &domestic_forward,
                "smart_ddns",
                plugins,
                tag_map,
                PathOverrides {
                    disable_cache: true,
                    response_ttl_tag: Some(ttl_tag.to_string()),
                    ..PathOverrides::default()
                },
            );
            (Some(matcher), Some(action))
        } else {
            (None, None)
        };

    Some(SmartRoutingTargets {
        semantic_routes,
        unknown_action,
        ddns_matcher,
        ddns_action,
    })
}

#[allow(clippy::too_many_arguments)]
fn smart_validation_tail(
    smart: &super::model::StandardSmartRoutingSettings,
    address_qtype: &str,
    rcode_noerror: &str,
    rcode_nxdomain: &str,
    rcode_servfail: &str,
    has_wanted: &str,
    has_cname: &str,
    domestic_resp_ip: &str,
    drop_tags: &BTreeMap<&str, String>,
) -> Vec<JsonValue> {
    let mut tail = Vec::new();
    let mut drop_when = |enabled: bool, matches: Vec<String>, reason: &'static str| {
        if enabled {
            tail.push(json!({
                "matches": matches,
                "exec": format!("${}", drop_tags[reason]),
            }));
        }
    };
    drop_when(
        smart.response_policy.servfail,
        vec![format!("${address_qtype}"), format!("${rcode_servfail}")],
        "servfail",
    );
    drop_when(
        smart.response_policy.nxdomain,
        vec![format!("${address_qtype}"), format!("${rcode_nxdomain}")],
        "nxdomain",
    );
    drop_when(
        smart.response_policy.cname_only,
        vec![
            format!("${address_qtype}"),
            format!("${rcode_noerror}"),
            format!("!${has_wanted}"),
            format!("${has_cname}"),
        ],
        "cname_only",
    );
    drop_when(
        smart.response_policy.nodata,
        vec![
            format!("${address_qtype}"),
            format!("${rcode_noerror}"),
            format!("!${has_wanted}"),
            format!("!${has_cname}"),
        ],
        "nodata",
    );
    drop_when(
        smart.response_policy.domestic_ip_mismatch,
        vec![
            format!("${address_qtype}"),
            format!("${rcode_noerror}"),
            format!("${has_wanted}"),
            format!("!${domestic_resp_ip}"),
        ],
        "domestic_ip_mismatch",
    );
    tail
}

fn compile_path_bundle(
    path: &StandardResolutionPath,
    intent: &StandardIntent,
    forward_tag: &str,
    namespace: &str,
    plugins: &mut Vec<GeneratedPlugin>,
    tag_map: &StandardTagMap,
    mut overrides: PathOverrides,
) -> String {
    let mut prelude = Vec::new();

    match path.dual_stack {
        StandardDualStackPolicy::Ipv4Only | StandardDualStackPolicy::Ipv6Only => {
            let matcher_tag = path_bundle_tag("qtype", namespace);
            let action_tag = path_bundle_tag("qtype_block", namespace);
            plugins.push(GeneratedPlugin::new(
                &matcher_tag,
                "qtype",
                json!([
                    if matches!(path.dual_stack, StandardDualStackPolicy::Ipv4Only) {
                        "AAAA"
                    } else {
                        "A"
                    }
                ]),
            ));
            plugins.push(GeneratedPlugin::new(
                &action_tag,
                "black_hole",
                json!({ "mode": "nodata", "short_circuit": true }),
            ));
            prelude.push(json!({
                "matches": format!("${matcher_tag}"),
                "exec": format!("${action_tag}"),
            }));
        }
        StandardDualStackPolicy::PreferIpv4 | StandardDualStackPolicy::PreferIpv6 => {
            let selector_tag = path_bundle_tag("dual", namespace);
            plugins.push(GeneratedPlugin::new(
                &selector_tag,
                if matches!(path.dual_stack, StandardDualStackPolicy::PreferIpv4) {
                    "prefer_ipv4"
                } else {
                    "prefer_ipv6"
                },
                json!({ "cache": true, "cache_ttl": 3600 }),
            ));
            prelude.push(json!({ "exec": format!("${selector_tag}") }));
        }
        StandardDualStackPolicy::Inherit | StandardDualStackPolicy::Disabled => {}
    }

    if !matches!(path.ecs, StandardEcsPolicy::Inherit) {
        let ecs_tag = path_bundle_tag("ecs", namespace);
        let args = match &path.ecs {
            StandardEcsPolicy::Inherit => unreachable!(),
            StandardEcsPolicy::Remove => json!({ "forward": false, "send": false }),
            StandardEcsPolicy::PreserveClient => json!({ "forward": true, "send": false }),
            StandardEcsPolicy::ClientSubnet { mask4, mask6 } => json!({
                "forward": false,
                "send": true,
                "mask4": mask4,
                "mask6": mask6,
            }),
            StandardEcsPolicy::Preset {
                address,
                mask4,
                mask6,
            } => json!({
                "forward": false,
                "send": false,
                "preset": address,
                "mask4": mask4,
                "mask6": mask6,
            }),
        };
        plugins.push(GeneratedPlugin::new(&ecs_tag, "ecs_handler", args));
        prelude.push(json!({ "exec": format!("${ecs_tag}") }));
    }

    if path.ip_selection.enabled {
        let selection = &path.ip_selection;
        let selector_tag = path_bundle_tag("ip_selector", namespace);
        plugins.push(GeneratedPlugin::new(
            &selector_tag,
            "ip_selector",
            json!({
                "selection_mode": selection.selection_mode.as_str(),
                "outbound": selection.outbound,
                "socks5": selection.socks5,
                "probe_methods": selection.probe_methods,
                "probe_stagger": selection.probe_stagger_ms,
                "probe_timeout": selection.probe_timeout_ms,
                "max_wait": selection.max_wait_ms,
                "top_n": selection.top_n,
                "dnssec_policy": selection.dnssec_policy.as_str(),
                "max_parallel_probes": selection.max_parallel_probes,
                "cache": {
                    "enabled": selection.cache_enabled,
                    "size": selection.cache_size,
                    "ttl": selection.cache_ttl_seconds,
                    "failure_ttl": selection.failure_ttl_seconds,
                },
            }),
        ));
        prelude.push(json!({ "exec": format!("${selector_tag}") }));
    }

    if !overrides.disable_cache && path_cache_enabled(path, intent) {
        let cache_tag = path_bundle_tag("cache", namespace);
        plugins.push(GeneratedPlugin::new(
            &cache_tag,
            "cache",
            json!({
                "size": intent.cache.size,
                "min_positive_ttl": intent.cache.min_positive_ttl,
                "max_positive_ttl": intent.cache.max_positive_ttl,
                "max_negative_ttl": intent.cache.max_negative_ttl,
                "negative_ttl_without_soa": intent.cache.negative_ttl_without_soa,
                "ecs_in_key": path.ecs.affects_cache_key(),
                "short_circuit": true,
            }),
        ));
        overrides.cache_tag = Some(cache_tag);
    }
    overrides.prelude.extend(prelude);
    let tag = path_bundle_tag("path", namespace);
    let sequence = build_path_sequence(path, intent, forward_tag, tag_map, overrides);
    plugins.push(GeneratedPlugin::new(
        &tag,
        "sequence",
        JsonValue::Array(sequence),
    ));
    tag
}

fn path_bundle_tag(kind: &str, namespace: &str) -> String {
    if let Some(id) = namespace.strip_prefix("dedicated:") {
        return standard_tag(&format!("dedicated_{kind}"), id);
    }
    match kind {
        "cache" | "path" => standard_tag(kind, namespace),
        _ => format!(
            "standard_path_{}_{}",
            safe_tag_component(kind),
            safe_tag_component(namespace)
        ),
    }
}

fn build_path_sequence(
    path: &StandardResolutionPath,
    intent: &StandardIntent,
    forward_tag: &str,
    tag_map: &StandardTagMap,
    mut overrides: PathOverrides,
) -> Vec<JsonValue> {
    let filtering_enabled = !overrides.disable_filtering
        && (overrides.force_filtering
            || matches!(path.filtering, StandardPolicySwitch::Enabled)
            || (matches!(path.filtering, StandardPolicySwitch::Inherit)
                && intent.filtering.enabled));
    let query_log_enabled = !overrides.disable_query_log
        && (overrides.force_query_log
            || matches!(path.query_log, StandardPolicySwitch::Enabled)
            || (matches!(path.query_log, StandardPolicySwitch::Inherit)
                && intent.query_log.enabled));
    let mut sequence = Vec::new();
    if tag_map.query_log.is_some() {
        if intent.query_log.enabled && !query_log_enabled {
            sequence.push(json!({ "exec": format!("mark {STANDARD_QUERY_SKIP_MARK}") }));
        } else if !intent.query_log.enabled && query_log_enabled {
            sequence.push(json!({ "exec": format!("mark {STANDARD_QUERY_RECORD_MARK}") }));
        }
    }
    sequence.append(&mut overrides.prelude);
    if let Some(prepend_exec) = overrides.prepend_exec.as_ref() {
        sequence.push(json!({ "exec": prepend_exec }));
    }
    if filtering_enabled
        && tag_map
            .filtering
            .iter()
            .any(|tag| tag == "standard_ad_rules")
        && tag_map
            .filtering
            .iter()
            .any(|tag| tag == "standard_blocked")
    {
        sequence.push(json!({
            "matches": "qname $standard_ad_rules",
            "exec": "$standard_blocked",
        }));
    }
    if !overrides.disable_cache
        && let Some(cache_tag) = overrides
            .cache_tag
            .as_ref()
            .or_else(|| tag_map.caches.get(&path.id))
    {
        sequence.push(json!({ "exec": format!("${cache_tag}") }));
    }
    sequence.push(json!({
        "matches": "!has_resp",
        "exec": format!("${forward_tag}"),
    }));
    let response_ttl_tag = overrides
        .response_ttl_tag
        .as_ref()
        .or_else(|| tag_map.local.get("responseTtl"));
    if let Some(ttl_tag) = response_ttl_tag {
        sequence.push(json!({ "exec": format!("${ttl_tag}") }));
    }
    sequence.append(&mut overrides.tail);
    sequence.push(json!({ "exec": "accept" }));
    sequence
}

fn build_exception_sequence(
    rule: &super::model::StandardExceptionRule,
    intent: &StandardIntent,
    default_path: &StandardResolutionPath,
    _default_path_tag: &str,
    tag_map: &StandardTagMap,
) -> Vec<JsonValue> {
    if matches!(rule.action, StandardRuleAction::Block) {
        return vec![
            json!({ "exec": "$standard_blocked" }),
            json!({ "exec": "accept" }),
        ];
    }
    let forward_tag = tag_map
        .upstream_groups
        .get(&default_path.upstream_group_id)
        .expect("validated default path should have forward tag");
    let overrides = match rule.action {
        StandardRuleAction::Allow | StandardRuleAction::SkipFiltering => PathOverrides {
            disable_filtering: true,
            ..PathOverrides::default()
        },
        StandardRuleAction::DisableLogging => PathOverrides {
            disable_query_log: true,
            ..PathOverrides::default()
        },
        StandardRuleAction::PreferIpv4 => PathOverrides {
            prepend_exec: Some("$standard_prefer_ipv4".to_string()),
            ..PathOverrides::default()
        },
        StandardRuleAction::PreferIpv6 => PathOverrides {
            prepend_exec: Some("$standard_prefer_ipv6".to_string()),
            ..PathOverrides::default()
        },
        _ => PathOverrides::default(),
    };
    build_path_sequence(default_path, intent, forward_tag, tag_map, overrides)
}

fn compiled_matcher(condition: &StandardRuleCondition) -> CompiledMatcher {
    match condition {
        StandardRuleCondition::Domain { values } => CompiledMatcher {
            kind: "qname",
            args: json!(
                values
                    .iter()
                    .map(|value| format!("full:{value}"))
                    .collect::<Vec<_>>()
            ),
        },
        StandardRuleCondition::Suffix { values } => CompiledMatcher {
            kind: "qname",
            args: json!(
                values
                    .iter()
                    .map(|value| format!("domain:{value}"))
                    .collect::<Vec<_>>()
            ),
        },
        StandardRuleCondition::Keyword { values } => CompiledMatcher {
            kind: "qname",
            args: json!(
                values
                    .iter()
                    .map(|value| format!("keyword:{value}"))
                    .collect::<Vec<_>>()
            ),
        },
        StandardRuleCondition::ClientCidr { values } => CompiledMatcher {
            kind: "client_ip",
            args: json!(values),
        },
        StandardRuleCondition::Qtype { values } => CompiledMatcher {
            kind: "qtype",
            args: json!(values),
        },
        StandardRuleCondition::ClientName { .. } | StandardRuleCondition::Subscription { .. } => {
            unreachable!("unsupported conditions are rejected before compilation")
        }
    }
}

fn compiled_upstream(upstream: &StandardUpstream) -> JsonValue {
    let mut value = serde_json::Map::new();
    value.insert("tag".to_string(), JsonValue::from(upstream.id.clone()));
    value.insert(
        "addr".to_string(),
        JsonValue::from(upstream_address(upstream)),
    );
    if let Some(bootstrap) = &upstream.bootstrap {
        value.insert("bootstrap".to_string(), JsonValue::from(bootstrap.clone()));
    }
    if let Some(bootstrap_version) = upstream.bootstrap_version {
        value.insert(
            "bootstrap_version".to_string(),
            JsonValue::from(bootstrap_version),
        );
    }
    if let Some(dial_address) = &upstream.dial_address {
        value.insert(
            "dial_addr".to_string(),
            JsonValue::from(dial_address.clone()),
        );
    }
    if let Some(outbound) = &upstream.outbound {
        value.insert("outbound".to_string(), JsonValue::from(outbound.clone()));
    }
    if let Some(socks5) = &upstream.socks5 {
        value.insert("socks5".to_string(), JsonValue::from(socks5.clone()));
    }
    if let Some(timeout_seconds) = upstream.timeout_seconds {
        value.insert("timeout".to_string(), JsonValue::from(timeout_seconds));
    }
    if let Some(idle_timeout_seconds) = upstream.idle_timeout_seconds {
        value.insert(
            "idle_timeout".to_string(),
            JsonValue::from(idle_timeout_seconds),
        );
    }
    if let Some(max_conns) = upstream.max_conns {
        value.insert("max_conns".to_string(), JsonValue::from(max_conns));
    }
    if let Some(min_conns) = upstream.min_conns {
        value.insert("min_conns".to_string(), JsonValue::from(min_conns));
    }
    if upstream.enable_pipeline {
        value.insert("enable_pipeline".to_string(), JsonValue::from(true));
    }
    if !upstream.tls_verify {
        value.insert("insecure_skip_verify".to_string(), JsonValue::from(true));
    }
    if matches!(upstream.protocol, StandardUpstreamProtocol::Doh3) || upstream.enable_http3 {
        value.insert("enable_http3".to_string(), JsonValue::from(true));
    }
    JsonValue::Object(value)
}

fn upstream_address(upstream: &StandardUpstream) -> String {
    let address = upstream.address.trim();
    match upstream.protocol {
        StandardUpstreamProtocol::Auto => address.to_string(),
        StandardUpstreamProtocol::Udp => with_scheme(address, "udp://"),
        StandardUpstreamProtocol::Tcp => with_scheme(address, "tcp://"),
        StandardUpstreamProtocol::Dot => with_scheme(address, "tls://"),
        StandardUpstreamProtocol::Doq => with_scheme(address, "quic://"),
        StandardUpstreamProtocol::Doh | StandardUpstreamProtocol::Doh3 => {
            let base = with_scheme(address, "https://");
            let authority_end = base["https://".len()..]
                .find('/')
                .map(|index| index + "https://".len());
            if authority_end.is_some() {
                base
            } else {
                format!(
                    "{base}{}",
                    upstream.doh_path.as_deref().unwrap_or("/dns-query")
                )
            }
        }
    }
}

fn with_scheme(address: &str, scheme: &str) -> String {
    if address.contains("://") {
        address.to_string()
    } else {
        format!("{scheme}{address}")
    }
}

fn strategy_name(strategy: super::model::StandardUpstreamStrategy) -> &'static str {
    match strategy {
        super::model::StandardUpstreamStrategy::Fastest => "fastest",
        super::model::StandardUpstreamStrategy::Balanced => "balanced",
        super::model::StandardUpstreamStrategy::PreferPositive => "prefer_positive",
        super::model::StandardUpstreamStrategy::Consensus => "consensus",
        super::model::StandardUpstreamStrategy::OrderedFallback => "balanced",
    }
}

fn serialize_generated_config(
    intent: &StandardIntent,
    base: &Mapping,
    plugins: &[GeneratedPlugin],
) -> Result<String, StandardDiagnostic> {
    let mut root = Mapping::new();
    for key in ["include", "api", "network"] {
        let key_value = YamlValue::from(key);
        if let Some(value) = base.get(&key_value) {
            root.insert(key_value, value.clone());
        }
    }

    let mut runtime = mapping_from_base(base, "runtime");
    runtime.remove(YamlValue::from("threads"));
    if let Some(threads) = intent.system.threads {
        runtime.insert(
            YamlValue::from("worker_threads"),
            YamlValue::from(u64::try_from(threads).unwrap_or(u64::MAX)),
        );
    } else {
        runtime.remove(YamlValue::from("worker_threads"));
    }
    if !runtime.is_empty() {
        root.insert(YamlValue::from("runtime"), YamlValue::Mapping(runtime));
    }

    let mut log = mapping_from_base(base, "log");
    log.insert(
        YamlValue::from("level"),
        YamlValue::from(intent.system.log_level.as_str()),
    );
    root.insert(YamlValue::from("log"), YamlValue::Mapping(log));

    let plugins = serde_yaml_ng::to_value(plugins).map_err(|err| {
        StandardDiagnostic::error(
            "generated_config_serialize_failed",
            "plugins",
            err.to_string(),
        )
    })?;
    root.insert(YamlValue::from("plugins"), plugins);

    let body = serde_yaml_ng::to_string(&YamlValue::Mapping(root)).map_err(|err| {
        StandardDiagnostic::error(
            "generated_config_serialize_failed",
            "config",
            err.to_string(),
        )
    })?;
    Ok(format!("# oxidns-webui.mode: standard\n{body}"))
}

fn parse_base_config(value: Option<&str>) -> Result<Mapping, StandardDiagnostic> {
    let Some(value) = value else {
        return Ok(Mapping::new());
    };
    let parsed: YamlValue = serde_yaml_ng::from_str(value).map_err(|err| {
        StandardDiagnostic::error("base_config_invalid", "baseConfig", err.to_string())
    })?;
    parsed.as_mapping().cloned().ok_or_else(|| {
        StandardDiagnostic::error(
            "base_config_invalid",
            "baseConfig",
            "base configuration root must be a YAML mapping",
        )
    })
}

fn mapping_from_base(base: &Mapping, key: &str) -> Mapping {
    base.get(YamlValue::from(key))
        .and_then(YamlValue::as_mapping)
        .cloned()
        .unwrap_or_default()
}

fn summarize(intent: &StandardIntent) -> StandardGenerationSummary {
    StandardGenerationSummary {
        upstream_group_count: intent.upstream_groups.len(),
        path_count: intent.paths.len(),
        enabled_upstream_count: intent
            .upstream_groups
            .iter()
            .map(|group| group.upstreams.iter().filter(|item| item.enabled).count())
            .sum(),
        filtering_enabled: intent.filtering.enabled,
        cache_enabled: intent.cache.enabled,
        query_log_enabled: intent.query_log.enabled,
        routing_rule_count: intent
            .routing
            .rules
            .iter()
            .filter(|rule| rule.enabled)
            .count(),
        exception_rule_count: intent.exceptions.iter().filter(|rule| rule.enabled).count(),
        device_count: intent.devices.len(),
        local_policy_count: [
            !intent.local.hosts.entries.is_empty() || !intent.local.hosts.files.is_empty(),
            !intent.local.redirects.rules.is_empty() || !intent.local.redirects.files.is_empty(),
            !intent.local.records.rules.is_empty() || !intent.local.records.files.is_empty(),
            intent.local.response_ttl.enabled,
            intent.local.qtype_policy.enabled,
            intent.local.ddns.enabled,
        ]
        .into_iter()
        .filter(|enabled| *enabled)
        .count(),
        rule_data_source_count: intent
            .rule_data
            .all_roles()
            .into_iter()
            .map(|(_, role)| {
                role.sources
                    .iter()
                    .filter(|source| source.enabled())
                    .count()
            })
            .sum(),
        smart_routing_enabled: intent.smart_routing.enabled,
        dedicated_group_count: intent
            .dedicated_groups
            .iter()
            .filter(|group| group.enabled)
            .count(),
        dynamic_learning_profile_count: intent
            .dynamic_learning
            .profiles
            .iter()
            .filter(|profile| profile.enabled)
            .count(),
        advanced_rule_count: intent
            .advanced_rules
            .iter()
            .filter(|rule| rule.enabled)
            .count(),
    }
}

fn path_cache_enabled(path: &StandardResolutionPath, intent: &StandardIntent) -> bool {
    matches!(path.cache, StandardPolicySwitch::Enabled)
        || (matches!(path.cache, StandardPolicySwitch::Inherit) && intent.cache.enabled)
}

fn subscription_filename(id: &str) -> String {
    format!("{}.txt", safe_tag_component(id))
}

fn rule_data_subscription_filename(role: &str, id: &str) -> String {
    format!(
        "{}_{}.txt",
        safe_tag_component(role),
        safe_tag_component(id)
    )
}

fn ordered_exceptions(intent: &StandardIntent) -> Vec<&super::model::StandardExceptionRule> {
    let mut rules: Vec<_> = intent
        .exceptions
        .iter()
        .filter(|rule| rule.enabled)
        .collect();
    rules.sort_by_key(|rule| match rule.action {
        StandardRuleAction::Block => 0,
        StandardRuleAction::Allow => 1,
        StandardRuleAction::SkipFiltering => 2,
        StandardRuleAction::UsePath { .. } | StandardRuleAction::UseDefaultPath => 3,
        StandardRuleAction::PreferIpv4 | StandardRuleAction::PreferIpv6 => 4,
        StandardRuleAction::DisableLogging => 5,
    });
    rules
}

fn has_errors(diagnostics: &[StandardDiagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == StandardDiagnosticSeverity::Error)
}

#[derive(Debug, Serialize)]
struct GeneratedPlugin {
    tag: String,
    #[serde(rename = "type")]
    kind: String,
    args: JsonValue,
}

impl GeneratedPlugin {
    fn new(tag: &str, kind: &str, args: JsonValue) -> Self {
        Self {
            tag: tag.to_string(),
            kind: kind.to_string(),
            args,
        }
    }
}

struct CompiledMatcher {
    kind: &'static str,
    args: JsonValue,
}

#[derive(Default)]
struct PathOverrides {
    disable_filtering: bool,
    force_filtering: bool,
    disable_query_log: bool,
    force_query_log: bool,
    prepend_exec: Option<String>,
    disable_cache: bool,
    response_ttl_tag: Option<String>,
    cache_tag: Option<String>,
    prelude: Vec<JsonValue>,
    tail: Vec<JsonValue>,
}
