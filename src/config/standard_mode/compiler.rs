// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use serde_yaml_ng::{Mapping, Value as YamlValue};

use super::model::{
    StandardBlockResponse, StandardDiagnostic, StandardDiagnosticSeverity, StandardGeneratedConfig,
    StandardGenerationSummary, StandardIntent, StandardMigration, StandardPlan,
    StandardPolicySwitch, StandardResolutionPath, StandardRuleAction, StandardRuleCondition,
    StandardTagMap, StandardUpstream, StandardUpstreamProtocol,
};
use super::validation::{
    device_has_policy, effective_filtering_used, effective_query_log_used,
    normalize_standard_intent, safe_tag_component, standard_tag, validate_standard_intent,
};
use crate::build_info::SupportedPlugins;
use crate::infra::control::config_version;

const FILTER_SUBSCRIPTION_DIR: &str = "./data/standard-filter-subscriptions";

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
                "prefer_ipv4",
                "prefer_ipv6",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            matchers: ["qname", "client_ip", "qtype"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            providers: ["adguard_rule"].into_iter().map(str::to_string).collect(),
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
    let mut details = json!({
        "managedTopLevel": ["runtime.worker_threads", "log.level", "plugins"],
        "preservedTopLevel": ["include", "api", "network", "log.* except level"],
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
            }),
        ));
        tag_map.query_log = Some("standard_recorder".to_string());
    }

    let filtering = compile_filtering(intent, &mut plugins, &mut tag_map);

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
        let upstreams: Vec<_> = group
            .upstreams
            .iter()
            .filter(|upstream| upstream.enabled)
            .map(compiled_upstream)
            .collect();
        let tag = standard_tag("forward", &group.id);
        plugins.push(GeneratedPlugin::new(
            &tag,
            "forward",
            json!({
                "upstreams": upstreams,
                "concurrent": upstreams.len().clamp(1, 3),
                "response_selection": strategy_name(group.strategy),
            }),
        ));
        groups.insert(group.id.clone(), group);
        tag_map.upstream_groups.insert(group.id.clone(), tag);
    }

    for path in &intent.paths {
        if path_cache_enabled(path, intent) {
            let tag = standard_tag("cache", &path.id);
            plugins.push(GeneratedPlugin::new(
                &tag,
                "cache",
                json!({
                    "size": intent.cache.size,
                    "min_positive_ttl": intent.cache.min_positive_ttl,
                    "max_positive_ttl": intent.cache.max_positive_ttl,
                    "max_negative_ttl": intent.cache.max_negative_ttl,
                    "negative_ttl_without_soa": intent.cache.negative_ttl_without_soa,
                    "ecs_in_key": false,
                    "short_circuit": true,
                }),
            ));
            tag_map.caches.insert(path.id.clone(), tag);
        }
        let forward_tag = tag_map
            .upstream_groups
            .get(&path.upstream_group_id)
            .expect("validated path group should have a forward tag")
            .clone();
        let tag = standard_tag("path", &path.id);
        let sequence = build_path_sequence(
            path,
            intent,
            &forward_tag,
            &tag_map,
            PathOverrides::default(),
        );
        plugins.push(GeneratedPlugin::new(
            &tag,
            "sequence",
            JsonValue::Array(sequence),
        ));
        tag_map.paths.insert(path.id.clone(), tag);
    }

    let default_path = intent.paths.first().expect("validated path should exist");
    let default_path_tag = tag_map
        .paths
        .get(&default_path.id)
        .expect("default path should have a tag")
        .clone();

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
                prepend_exec: None,
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

    let mut main_sequence = Vec::new();
    if tag_map.system.iter().any(|tag| tag == "standard_metrics") {
        main_sequence.push(json!({ "exec": "$standard_metrics" }));
    }
    for rule in ordered_exceptions(intent) {
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
    main_sequence.push(json!({ "exec": format!("${default_path_tag}") }));
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
    if should_generate && !subscriptions.is_empty() {
        plugins.push(GeneratedPlugin::new(
            "standard_filter_download",
            "download",
            json!({
                "startup_if_missing": true,
                "downloads": subscriptions.iter().map(|subscription| json!({
                    "url": subscription.url,
                    "dir": FILTER_SUBSCRIPTION_DIR,
                    "filename": subscription_filename(&subscription.id),
                })).collect::<Vec<_>>(),
            }),
        ));
        tag_map
            .filtering
            .push("standard_filter_download".to_string());
    }

    let has_rules = should_generate
        && (!intent.filtering.block_rules.is_empty()
            || !intent.filtering.allow_rules.is_empty()
            || !subscriptions.is_empty());
    if has_rules {
        let mut rules = intent.filtering.block_rules.clone();
        rules.extend(intent.filtering.allow_rules.clone());
        plugins.push(GeneratedPlugin::new(
            "standard_ad_rules",
            "adguard_rule",
            json!({
                "files": subscriptions.iter().map(|subscription| {
                    format!("{FILTER_SUBSCRIPTION_DIR}/{}", subscription_filename(&subscription.id))
                }).collect::<Vec<_>>(),
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
        plugins.push(GeneratedPlugin::new(
            "standard_filter_cron",
            "cron",
            json!({
                "jobs": subscriptions.iter().map(|subscription| json!({
                    "name": format!("refresh_filter_{}", safe_tag_component(&subscription.id)),
                    "interval": format!("{}h", subscription.update_interval_hours.max(1)),
                    "executors": ["$standard_filter_download", "$standard_filter_reload"],
                })).collect::<Vec<_>>(),
            }),
        ));
        tag_map.filtering.push("standard_filter_cron".to_string());
    }
    has_rules
}

fn build_path_sequence(
    path: &StandardResolutionPath,
    intent: &StandardIntent,
    forward_tag: &str,
    tag_map: &StandardTagMap,
    overrides: PathOverrides,
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
    if let Some(prepend_exec) = overrides.prepend_exec {
        sequence.push(json!({ "exec": prepend_exec }));
    }
    if query_log_enabled && let Some(query_log) = &tag_map.query_log {
        sequence.push(json!({ "exec": format!("${query_log}") }));
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
    if let Some(cache_tag) = tag_map.caches.get(&path.id) {
        sequence.push(json!({ "exec": format!("${cache_tag}") }));
    }
    sequence.push(json!({
        "matches": "!has_resp",
        "exec": format!("${forward_tag}"),
    }));
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
            prepend_exec: Some("$standard_prefer_ipv4"),
            ..PathOverrides::default()
        },
        StandardRuleAction::PreferIpv6 => PathOverrides {
            prepend_exec: Some("$standard_prefer_ipv6"),
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
    if let Some(dial_address) = &upstream.dial_address {
        value.insert(
            "dial_addr".to_string(),
            JsonValue::from(dial_address.clone()),
        );
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
        super::model::StandardUpstreamStrategy::OrderedFallback => {
            unreachable!("ordered fallback is rejected before compilation")
        }
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
    }
}

fn path_cache_enabled(path: &StandardResolutionPath, intent: &StandardIntent) -> bool {
    matches!(path.cache, StandardPolicySwitch::Enabled)
        || (matches!(path.cache, StandardPolicySwitch::Inherit) && intent.cache.enabled)
}

fn subscription_filename(id: &str) -> String {
    format!("{}.txt", safe_tag_component(id))
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
    prepend_exec: Option<&'static str>,
}
