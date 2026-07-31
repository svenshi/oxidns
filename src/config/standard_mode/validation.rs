// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::str::FromStr;

use super::compiler::StandardCapabilities;
use super::model::{
    CURRENT_STANDARD_SCHEMA, StandardDiagnostic, StandardDualStackPolicy, StandardEcsPolicy,
    StandardIntent, StandardPolicySwitch, StandardRuleAction, StandardRuleCondition,
    StandardRuleDataSource, StandardUnknownMode, StandardUpstreamProtocol,
    StandardUpstreamStrategy,
};

pub fn normalize_standard_intent(mut intent: StandardIntent) -> StandardIntent {
    intent.schema = CURRENT_STANDARD_SCHEMA;
    intent.listen.address = intent.listen.address.trim().to_string();

    for group in &mut intent.upstream_groups {
        group.id = normalize_id(&group.id);
        group.name = group.name.trim().to_string();
        group.description = trimmed_option(group.description.take());
        for upstream in &mut group.upstreams {
            upstream.id = normalize_id(&upstream.id);
            upstream.name = upstream.name.trim().to_string();
            upstream.address = upstream.address.trim().to_string();
            upstream.bootstrap = trimmed_option(upstream.bootstrap.take());
            upstream.dial_address = trimmed_option(upstream.dial_address.take());
            upstream.outbound = trimmed_option(upstream.outbound.take());
            upstream.socks5 = trimmed_option(upstream.socks5.take());
            upstream.doh_path = trimmed_option(upstream.doh_path.take());
            if matches!(
                upstream.protocol,
                StandardUpstreamProtocol::Doh | StandardUpstreamProtocol::Doh3
            ) && upstream.doh_path.is_none()
            {
                upstream.doh_path = Some("/dns-query".to_string());
            }
            upstream.enable_http3 = matches!(upstream.protocol, StandardUpstreamProtocol::Doh3);
        }
    }

    for path in &mut intent.paths {
        path.id = normalize_id(&path.id);
        path.name = path.name.trim().to_string();
        path.description = trimmed_option(path.description.take());
        path.upstream_group_id = normalize_id(&path.upstream_group_id);
        path.ip_selection.outbound = trimmed_option(path.ip_selection.outbound.take());
        path.ip_selection.socks5 = trimmed_option(path.ip_selection.socks5.take());
        normalize_lines(&mut path.ip_selection.probe_methods, false);
        if let StandardEcsPolicy::Preset { address, .. } = &mut path.ecs {
            *address = address.trim().to_string();
        }
    }

    for (_, role) in intent.rule_data.all_roles_mut() {
        for source in &mut role.sources {
            *source.id_mut() = normalize_id(source.id());
            *source.name_mut() = source.name().trim().to_string();
            match source {
                StandardRuleDataSource::Manual { rules, .. } => normalize_lines(rules, false),
                StandardRuleDataSource::LocalFile { path, .. } => {
                    *path = path.trim().to_string();
                }
                StandardRuleDataSource::NativeDat {
                    path, selectors, ..
                } => {
                    *path = path.trim().to_string();
                    normalize_lines(selectors, false);
                }
                StandardRuleDataSource::Subscription { url, .. } => {
                    *url = url.trim().to_string();
                }
            }
        }
    }
    intent.smart_routing.domestic_path_id = intent
        .smart_routing
        .domestic_path_id
        .take()
        .map(|path| normalize_id(&path))
        .filter(|path| !path.is_empty());
    intent.smart_routing.remote_path_id = intent
        .smart_routing
        .remote_path_id
        .take()
        .map(|path| normalize_id(&path))
        .filter(|path| !path.is_empty());

    normalize_lines(&mut intent.filtering.block_rules, false);
    normalize_lines(&mut intent.filtering.allow_rules, true);
    for subscription in &mut intent.filtering.subscriptions {
        subscription.id = normalize_id(&subscription.id);
        subscription.name = subscription.name.trim().to_string();
        subscription.url = subscription.url.trim().to_string();
    }
    for file in &mut intent.filtering.local_files {
        file.id = normalize_id(&file.id);
        file.name = file.name.trim().to_string();
        file.path = file.path.trim().to_string();
    }

    normalize_lines(&mut intent.local.hosts.entries, false);
    normalize_lines(&mut intent.local.hosts.files, false);
    normalize_lines(&mut intent.local.redirects.rules, false);
    normalize_lines(&mut intent.local.redirects.files, false);
    normalize_lines(&mut intent.local.records.rules, false);
    normalize_lines(&mut intent.local.records.files, false);
    normalize_lines(&mut intent.local.qtype_policy.qtypes, false);
    for qtype in &mut intent.local.qtype_policy.qtypes {
        qtype.make_ascii_uppercase();
    }
    normalize_lines(&mut intent.local.ddns.domains, false);
    for domain in &mut intent.local.ddns.domains {
        *domain = domain
            .trim()
            .trim_start_matches("full:")
            .trim_end_matches('.')
            .to_ascii_lowercase();
    }
    intent.local.ddns.path_id = intent
        .local
        .ddns
        .path_id
        .take()
        .map(|path_id| normalize_id(&path_id))
        .filter(|path_id| !path_id.is_empty());

    for rule in &mut intent.routing.rules {
        rule.id = normalize_id(&rule.id);
        rule.name = rule.name.trim().to_string();
        rule.note = trimmed_option(rule.note.take());
        normalize_condition(&mut rule.condition);
        normalize_action(&mut rule.action);
    }
    for scenario in &mut intent.routing.scenarios {
        scenario.id = normalize_id(&scenario.id);
        scenario.name = scenario.name.trim().to_string();
    }
    for rule in &mut intent.exceptions {
        rule.id = normalize_id(&rule.id);
        rule.name = rule.name.trim().to_string();
        rule.note = trimmed_option(rule.note.take());
        normalize_condition(&mut rule.condition);
        normalize_action(&mut rule.action);
    }
    for device in &mut intent.devices {
        device.id = normalize_id(&device.id);
        device.name = device.name.trim().to_string();
        normalize_lines(&mut device.addresses, false);
        device.assigned_path_id = device
            .assigned_path_id
            .take()
            .map(|path_id| normalize_id(&path_id))
            .filter(|path_id| !path_id.is_empty());
    }

    intent
}

pub fn validate_standard_intent(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
) -> Vec<StandardDiagnostic> {
    let mut diagnostics = Vec::new();
    if intent.schema != CURRENT_STANDARD_SCHEMA {
        diagnostics.push(StandardDiagnostic::error(
            "schema_unsupported",
            "schema",
            format!(
                "expected Standard Mode schema {CURRENT_STANDARD_SCHEMA}, got {}",
                intent.schema
            ),
        ));
    }

    validate_listeners(intent, capabilities, &mut diagnostics);
    validate_identifiers(intent, &mut diagnostics);
    validate_upstreams(intent, capabilities, &mut diagnostics);
    validate_cache(intent, capabilities, &mut diagnostics);
    validate_filtering(intent, capabilities, &mut diagnostics);
    validate_local(intent, capabilities, &mut diagnostics);
    validate_rule_data(intent, capabilities, &mut diagnostics);
    validate_paths(intent, capabilities, &mut diagnostics);
    validate_smart_routing(intent, capabilities, &mut diagnostics);
    validate_rules(intent, capabilities, &mut diagnostics);
    validate_devices(intent, capabilities, &mut diagnostics);
    validate_system(intent, &mut diagnostics);

    if !capabilities.executor("metrics_collector") {
        diagnostics.push(StandardDiagnostic::warning(
            "optional_metrics_unavailable",
            "capabilities.executors.metrics_collector",
            "metrics_collector is unavailable; DNS behavior is unchanged but Standard Mode metrics are reduced",
        ));
    }

    diagnostics
}

fn validate_listeners(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    if !intent.listen.udp && !intent.listen.tcp {
        diagnostics.push(StandardDiagnostic::error(
            "listener_required",
            "listen",
            "at least one UDP or TCP listener must be enabled",
        ));
    }
    if intent
        .listen
        .address
        .parse::<std::net::SocketAddr>()
        .is_err()
    {
        diagnostics.push(StandardDiagnostic::error(
            "listener_address_invalid",
            "listen.address",
            "listener address must be an IP socket address such as 0.0.0.0:5335",
        ));
    }
    if intent.listen.udp && !capabilities.server("udp_server") {
        diagnostics.push(missing_plugin("listen.udp", "server", "udp_server"));
    }
    if intent.listen.tcp && !capabilities.server("tcp_server") {
        diagnostics.push(missing_plugin("listen.tcp", "server", "tcp_server"));
    }
    if !capabilities.executor("sequence") {
        diagnostics.push(missing_plugin(
            "capabilities.executors.sequence",
            "executor",
            "sequence",
        ));
    }
    if !capabilities.executor("forward") {
        diagnostics.push(missing_plugin(
            "capabilities.executors.forward",
            "executor",
            "forward",
        ));
    }
}

fn validate_identifiers(intent: &StandardIntent, diagnostics: &mut Vec<StandardDiagnostic>) {
    validate_unique_objects(
        intent
            .upstream_groups
            .iter()
            .enumerate()
            .map(|(index, group)| (index, group.id.as_str(), group.name.as_str())),
        "upstreamGroups",
        diagnostics,
    );
    validate_unique_objects(
        intent
            .paths
            .iter()
            .enumerate()
            .map(|(index, path)| (index, path.id.as_str(), path.name.as_str())),
        "paths",
        diagnostics,
    );
    validate_unique_objects(
        intent
            .filtering
            .subscriptions
            .iter()
            .enumerate()
            .map(|(index, item)| (index, item.id.as_str(), item.name.as_str())),
        "filtering.subscriptions",
        diagnostics,
    );
    validate_unique_objects(
        intent
            .filtering
            .local_files
            .iter()
            .enumerate()
            .map(|(index, item)| (index, item.id.as_str(), item.name.as_str())),
        "filtering.localFiles",
        diagnostics,
    );
    validate_unique_objects(
        intent
            .routing
            .rules
            .iter()
            .enumerate()
            .map(|(index, item)| (index, item.id.as_str(), item.name.as_str())),
        "routing.rules",
        diagnostics,
    );
    validate_unique_objects(
        intent
            .exceptions
            .iter()
            .enumerate()
            .map(|(index, item)| (index, item.id.as_str(), item.name.as_str())),
        "exceptions",
        diagnostics,
    );
    validate_unique_objects(
        intent
            .devices
            .iter()
            .enumerate()
            .map(|(index, item)| (index, item.id.as_str(), item.name.as_str())),
        "devices",
        diagnostics,
    );

    let mut tags = BTreeMap::<String, String>::new();
    for (path, tag) in generated_user_tags(intent) {
        if let Some(first_path) = tags.insert(tag.clone(), path.clone()) {
            diagnostics.push(StandardDiagnostic::error(
                "generated_tag_duplicate",
                path,
                format!("generated tag '{tag}' collides with {first_path}"),
            ));
        }
    }

    let mut filenames = BTreeMap::<String, String>::new();
    for (index, subscription) in intent.filtering.subscriptions.iter().enumerate() {
        let filename = format!("{}.txt", safe_tag_component(&subscription.id));
        let path = format!("filtering.subscriptions[{index}].id");
        if let Some(first_path) = filenames.insert(filename.clone(), path.clone()) {
            diagnostics.push(StandardDiagnostic::error(
                "subscription_filename_duplicate",
                path,
                format!("generated filename '{filename}' collides with {first_path}"),
            ));
        }
    }
}

fn validate_upstreams(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    if intent.upstream_groups.is_empty() {
        diagnostics.push(StandardDiagnostic::error(
            "upstream_group_required",
            "upstreamGroups",
            "at least one upstream group is required",
        ));
        return;
    }
    let default_count = intent
        .upstream_groups
        .iter()
        .filter(|group| group.is_default)
        .count();
    if default_count != 1 {
        diagnostics.push(StandardDiagnostic::error(
            "default_upstream_group_invalid",
            "upstreamGroups",
            "exactly one upstream group must be marked as default",
        ));
    }

    for (group_index, group) in intent.upstream_groups.iter().enumerate() {
        let group_path = format!("upstreamGroups[{group_index}]");
        if matches!(group.strategy, StandardUpstreamStrategy::OrderedFallback)
            && !capabilities.executor("fallback")
        {
            diagnostics.push(missing_plugin(
                format!("{group_path}.strategy"),
                "executor",
                "fallback",
            ));
        }
        let enabled = group.upstreams.iter().filter(|item| item.enabled).count();
        if enabled == 0 {
            diagnostics.push(StandardDiagnostic::error(
                "enabled_upstream_required",
                format!("{group_path}.upstreams"),
                "an upstream group must contain at least one enabled upstream",
            ));
        }
        let mut upstream_ids = BTreeSet::new();
        for (upstream_index, upstream) in group.upstreams.iter().enumerate() {
            let path = format!("{group_path}.upstreams[{upstream_index}]");
            if normalize_id(&upstream.id).is_empty() {
                diagnostics.push(StandardDiagnostic::error(
                    "id_required",
                    format!("{path}.id"),
                    "upstream ID is required",
                ));
            } else if !upstream_ids.insert(normalize_id(&upstream.id)) {
                diagnostics.push(StandardDiagnostic::error(
                    "id_duplicate",
                    format!("{path}.id"),
                    "upstream IDs must be unique within their group",
                ));
            }
            if upstream.name.trim().is_empty() {
                diagnostics.push(StandardDiagnostic::error(
                    "name_required",
                    format!("{path}.name"),
                    "upstream name is required",
                ));
            }
            if upstream.enabled && upstream.address.trim().is_empty() {
                diagnostics.push(StandardDiagnostic::error(
                    "upstream_address_required",
                    format!("{path}.address"),
                    "enabled upstream address is required",
                ));
            }
            for feature in protocol_features(upstream.protocol) {
                if !capabilities.feature(feature) {
                    diagnostics.push(StandardDiagnostic::error(
                        "upstream_protocol_unavailable",
                        format!("{path}.protocol"),
                        format!("upstream protocol requires build feature '{feature}'"),
                    ));
                }
            }
            if !matches!(upstream.bootstrap_version, None | Some(4) | Some(6)) {
                diagnostics.push(StandardDiagnostic::error(
                    "upstream_bootstrap_version_invalid",
                    format!("{path}.bootstrapVersion"),
                    "bootstrapVersion must be 4 or 6",
                ));
            }
            if let Some(dial_address) = &upstream.dial_address
                && dial_address.parse::<IpAddr>().is_err()
            {
                diagnostics.push(StandardDiagnostic::error(
                    "upstream_dial_address_invalid",
                    format!("{path}.dialAddress"),
                    "dialAddress must be an IPv4 or IPv6 address",
                ));
            }
            if matches!(upstream.timeout_seconds, Some(0)) {
                diagnostics.push(StandardDiagnostic::error(
                    "upstream_timeout_invalid",
                    format!("{path}.timeoutSeconds"),
                    "timeoutSeconds must be greater than zero",
                ));
            }
            if matches!(upstream.idle_timeout_seconds, Some(0)) {
                diagnostics.push(StandardDiagnostic::error(
                    "upstream_idle_timeout_invalid",
                    format!("{path}.idleTimeoutSeconds"),
                    "idleTimeoutSeconds must be greater than zero",
                ));
            }
            if matches!(upstream.max_conns, Some(0)) {
                diagnostics.push(StandardDiagnostic::error(
                    "upstream_max_conns_invalid",
                    format!("{path}.maxConns"),
                    "maxConns must be greater than zero",
                ));
            }
            if upstream.max_conns.is_some_and(|value| value > 4096)
                || upstream.min_conns.is_some_and(|value| value > 4096)
            {
                diagnostics.push(StandardDiagnostic::error(
                    "upstream_pool_size_invalid",
                    format!("{path}.maxConns"),
                    "upstream pool sizes must not exceed 4096",
                ));
            }
            if let (Some(min), Some(max)) = (upstream.min_conns, upstream.max_conns)
                && min > max
            {
                diagnostics.push(StandardDiagnostic::error(
                    "upstream_pool_range_invalid",
                    format!("{path}.minConns"),
                    "minConns must not exceed maxConns",
                ));
            }
            if upstream.enable_pipeline
                && !matches!(
                    upstream.protocol,
                    StandardUpstreamProtocol::Auto
                        | StandardUpstreamProtocol::Tcp
                        | StandardUpstreamProtocol::Dot
                )
            {
                diagnostics.push(StandardDiagnostic::error(
                    "upstream_pipeline_protocol_invalid",
                    format!("{path}.enablePipeline"),
                    "pipelining is only available for auto, TCP, or DoT upstreams",
                ));
            }
        }
    }
}

fn validate_cache(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    if intent.cache.size < 128 {
        diagnostics.push(StandardDiagnostic::error(
            "cache_size_invalid",
            "cache.size",
            "cache size must be at least 128",
        ));
    }
    if intent.cache.min_positive_ttl > intent.cache.max_positive_ttl {
        diagnostics.push(StandardDiagnostic::error(
            "cache_positive_ttl_range_invalid",
            "cache",
            "minPositiveTtl must not exceed maxPositiveTtl",
        ));
    }
    if intent.cache.negative_ttl_without_soa > intent.cache.max_negative_ttl {
        diagnostics.push(StandardDiagnostic::error(
            "cache_negative_ttl_range_invalid",
            "cache",
            "negativeTtlWithoutSoa must not exceed maxNegativeTtl",
        ));
    }
    if effective_cache_paths(intent) > 0 && !capabilities.executor("cache") {
        diagnostics.push(missing_plugin("cache", "executor", "cache"));
    }
}

fn validate_filtering(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    let filtering_used = effective_filtering_used(intent);
    let enabled_subscriptions: Vec<_> = intent
        .filtering
        .subscriptions
        .iter()
        .enumerate()
        .filter(|(_, item)| item.enabled)
        .collect();
    let enabled_local_files: Vec<_> = intent
        .filtering
        .local_files
        .iter()
        .enumerate()
        .filter(|(_, item)| item.enabled)
        .collect();
    if filtering_used
        && intent.filtering.block_rules.is_empty()
        && enabled_subscriptions.is_empty()
        && enabled_local_files.is_empty()
        && !intent
            .exceptions
            .iter()
            .any(|rule| rule.enabled && matches!(rule.action, StandardRuleAction::Block))
    {
        diagnostics.push(StandardDiagnostic::error(
            "filter_rule_source_required",
            "filtering",
            "enabled filtering requires a block rule, local file, or subscription",
        ));
    }
    if filtering_used && !capabilities.provider("adguard_rule") {
        diagnostics.push(missing_plugin("filtering", "provider", "adguard_rule"));
    }
    if filtering_used && !capabilities.executor("black_hole") {
        diagnostics.push(missing_plugin("filtering", "executor", "black_hole"));
    }
    if !enabled_subscriptions.is_empty() {
        for kind in ["download", "reload_provider", "cron"] {
            if !capabilities.executor(kind) {
                diagnostics.push(missing_plugin("filtering.subscriptions", "executor", kind));
            }
        }
    }
    for (index, subscription) in enabled_subscriptions {
        let path = format!("filtering.subscriptions[{index}]");
        if subscription.name.trim().is_empty() {
            diagnostics.push(StandardDiagnostic::error(
                "subscription_name_required",
                format!("{path}.name"),
                "subscription name is required",
            ));
        }
        if !valid_http_url(&subscription.url) {
            diagnostics.push(StandardDiagnostic::error(
                "subscription_url_invalid",
                format!("{path}.url"),
                "subscription URL must use http or https",
            ));
        }
        if subscription.update_interval_hours == 0 {
            diagnostics.push(StandardDiagnostic::error(
                "subscription_interval_invalid",
                format!("{path}.updateIntervalHours"),
                "subscription update interval must be at least one hour",
            ));
        }
    }
    for (index, file) in enabled_local_files {
        let path = format!("filtering.localFiles[{index}]");
        if file.name.trim().is_empty() {
            diagnostics.push(StandardDiagnostic::error(
                "filter_file_name_required",
                format!("{path}.name"),
                "local filter file name is required",
            ));
        }
        if file.path.trim().is_empty() {
            diagnostics.push(StandardDiagnostic::error(
                "filter_file_path_required",
                format!("{path}.path"),
                "local filter file path is required",
            ));
        }
    }
}

fn validate_local(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    let local = &intent.local;
    if (!local.hosts.entries.is_empty() || !local.hosts.files.is_empty())
        && !capabilities.executor("hosts")
    {
        diagnostics.push(missing_plugin("local.hosts", "executor", "hosts"));
    }
    if (!local.redirects.rules.is_empty() || !local.redirects.files.is_empty())
        && !capabilities.executor("redirect")
    {
        diagnostics.push(missing_plugin("local.redirects", "executor", "redirect"));
    }
    if (!local.records.rules.is_empty() || !local.records.files.is_empty())
        && !capabilities.executor("arbitrary")
    {
        diagnostics.push(missing_plugin("local.records", "executor", "arbitrary"));
    }
    if local.response_ttl.enabled {
        if !capabilities.executor("ttl") {
            diagnostics.push(missing_plugin("local.responseTtl", "executor", "ttl"));
        }
        if local.response_ttl.min.is_none() && local.response_ttl.max.is_none() {
            diagnostics.push(StandardDiagnostic::error(
                "response_ttl_required",
                "local.responseTtl",
                "response TTL policy requires a minimum or maximum",
            ));
        }
        if let (Some(min), Some(max)) = (local.response_ttl.min, local.response_ttl.max)
            && min > max
        {
            diagnostics.push(StandardDiagnostic::error(
                "response_ttl_range_invalid",
                "local.responseTtl",
                "response TTL minimum must not exceed maximum",
            ));
        }
    }
    if local.qtype_policy.enabled {
        if local.qtype_policy.qtypes.is_empty() {
            diagnostics.push(StandardDiagnostic::error(
                "qtype_policy_values_required",
                "local.qtypePolicy.qtypes",
                "QTYPE policy requires at least one record type",
            ));
        }
        for (index, qtype) in local.qtype_policy.qtypes.iter().enumerate() {
            if crate::proto::RecordType::from_str(qtype).is_err() {
                diagnostics.push(StandardDiagnostic::error(
                    "qtype_policy_value_invalid",
                    format!("local.qtypePolicy.qtypes[{index}]"),
                    format!("'{qtype}' is not a supported DNS record type"),
                ));
            }
        }
        if !capabilities.matcher("qtype") {
            diagnostics.push(missing_plugin("local.qtypePolicy", "matcher", "qtype"));
        }
        if !capabilities.executor("black_hole") {
            diagnostics.push(missing_plugin(
                "local.qtypePolicy",
                "executor",
                "black_hole",
            ));
        }
    }
    if local.ddns.enabled {
        if local.ddns.domains.is_empty() {
            diagnostics.push(StandardDiagnostic::error(
                "ddns_domains_required",
                "local.ddns.domains",
                "DDNS policy requires at least one domain",
            ));
        }
        for (index, domain) in local.ddns.domains.iter().enumerate() {
            if crate::proto::Name::from_ascii(domain).is_err() {
                diagnostics.push(StandardDiagnostic::error(
                    "ddns_domain_invalid",
                    format!("local.ddns.domains[{index}]"),
                    format!("'{domain}' is not a valid domain name"),
                ));
            }
        }
        if local.ddns.ttl == 0 {
            diagnostics.push(StandardDiagnostic::error(
                "ddns_ttl_invalid",
                "local.ddns.ttl",
                "DDNS TTL must be greater than zero",
            ));
        }
        if let Some(path_id) = &local.ddns.path_id
            && !intent.paths.iter().any(|path| &path.id == path_id)
        {
            diagnostics.push(StandardDiagnostic::error(
                "ddns_path_missing",
                "local.ddns.pathId",
                format!("DDNS policy references missing path '{path_id}'"),
            ));
        }
        for (kind, plugin_type) in [("matcher", "qname"), ("executor", "ttl")] {
            let available = match kind {
                "matcher" => capabilities.matcher(plugin_type),
                _ => capabilities.executor(plugin_type),
            };
            if !available {
                diagnostics.push(missing_plugin("local.ddns", kind, plugin_type));
            }
        }
    }
}

fn validate_rule_data(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    let mut global_ids = BTreeMap::<String, String>::new();
    for (role_name, role) in intent.rule_data.all_roles() {
        let is_ip_role = role_name == "domestic_ips";
        if role.has_enabled_sources()
            && !if is_ip_role {
                capabilities.provider("ip_set")
            } else {
                capabilities.provider("domain_set")
            }
        {
            diagnostics.push(missing_plugin(
                format!("ruleData.{role_name}"),
                "provider",
                if is_ip_role { "ip_set" } else { "domain_set" },
            ));
        }
        for (index, source) in role.sources.iter().enumerate() {
            let base = format!("ruleData.{role_name}.sources[{index}]");
            if source.id().is_empty() {
                diagnostics.push(StandardDiagnostic::error(
                    "rule_data_source_id_required",
                    format!("{base}.id"),
                    "rule-data source ID is required",
                ));
            } else if let Some(first) =
                global_ids.insert(source.id().to_string(), format!("{base}.id"))
            {
                diagnostics.push(StandardDiagnostic::error(
                    "rule_data_source_id_duplicate",
                    format!("{base}.id"),
                    format!("rule-data source ID duplicates {first}"),
                ));
            }
            if source.name().trim().is_empty() {
                diagnostics.push(StandardDiagnostic::error(
                    "rule_data_source_name_required",
                    format!("{base}.name"),
                    "rule-data source name is required",
                ));
            }
            if !source.enabled() {
                continue;
            }
            match source {
                StandardRuleDataSource::Manual { rules, .. } => {
                    if rules.is_empty() {
                        diagnostics.push(StandardDiagnostic::error(
                            "rule_data_manual_rules_required",
                            format!("{base}.rules"),
                            "enabled manual source requires at least one rule",
                        ));
                    }
                    if is_ip_role {
                        for (rule_index, rule) in rules.iter().enumerate() {
                            if !valid_client_address(rule) {
                                diagnostics.push(StandardDiagnostic::error(
                                    "rule_data_ip_invalid",
                                    format!("{base}.rules[{rule_index}]"),
                                    format!("'{rule}' is not a valid IP address or CIDR"),
                                ));
                            }
                        }
                    } else {
                        let mut matcher = crate::core::rule_matcher::DomainRuleMatcher::default();
                        for (rule_index, rule) in rules.iter().enumerate() {
                            if let Err(error) = matcher.add_expression(rule, &base) {
                                diagnostics.push(StandardDiagnostic::error(
                                    "rule_data_domain_invalid",
                                    format!("{base}.rules[{rule_index}]"),
                                    error,
                                ));
                            }
                        }
                    }
                }
                StandardRuleDataSource::LocalFile { path, .. } => {
                    if path.is_empty() {
                        diagnostics.push(StandardDiagnostic::error(
                            "rule_data_file_path_required",
                            format!("{base}.path"),
                            "enabled local-file source requires a path",
                        ));
                    } else if !std::path::Path::new(path).is_file() {
                        diagnostics.push(StandardDiagnostic::error(
                            "rule_data_file_missing",
                            format!("{base}.path"),
                            format!(
                                "local rule-data file '{path}' does not exist or is not a file"
                            ),
                        ));
                    }
                }
                StandardRuleDataSource::Subscription {
                    url,
                    update_interval_hours,
                    max_age_hours,
                    ..
                } => {
                    if !valid_http_url(url) {
                        diagnostics.push(StandardDiagnostic::error(
                            "rule_data_subscription_url_invalid",
                            format!("{base}.url"),
                            "subscription URL must use http or https",
                        ));
                    }
                    if *update_interval_hours == 0 || *max_age_hours == 0 {
                        diagnostics.push(StandardDiagnostic::error(
                            "rule_data_subscription_interval_invalid",
                            base.clone(),
                            "subscription update and maximum-age hours must be greater than zero",
                        ));
                    }
                    for plugin in ["download", "reload_provider", "cron"] {
                        if !capabilities.executor(plugin) {
                            diagnostics.push(missing_plugin(&base, "executor", plugin));
                        }
                    }
                }
                StandardRuleDataSource::NativeDat {
                    path, selectors, ..
                } => {
                    if path.is_empty() {
                        diagnostics.push(StandardDiagnostic::error(
                            "rule_data_native_path_required",
                            format!("{base}.path"),
                            "enabled native-data source requires a local dat path",
                        ));
                    } else if !std::path::Path::new(path).is_file() {
                        diagnostics.push(StandardDiagnostic::error(
                            "rule_data_native_file_missing",
                            format!("{base}.path"),
                            format!(
                                "native rule-data file '{path}' does not exist or is not a file"
                            ),
                        ));
                    }
                    if selectors.is_empty() {
                        diagnostics.push(StandardDiagnostic::warning(
                            "rule_data_native_selectors_empty",
                            format!("{base}.selectors"),
                            "empty selectors load the entire native dat file",
                        ));
                    }
                    let provider = if is_ip_role { "geoip" } else { "geosite" };
                    if !capabilities.provider(provider) {
                        diagnostics.push(missing_plugin(&base, "provider", provider));
                    }
                }
            }
        }
    }
}

fn validate_smart_routing(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    let smart = &intent.smart_routing;
    if !smart.enabled {
        return;
    }
    let path_ids: BTreeSet<_> = intent.paths.iter().map(|path| path.id.as_str()).collect();
    let domestic = smart.domestic_path_id.as_deref();
    let remote = smart.remote_path_id.as_deref();
    for (field, value) in [("domesticPathId", domestic), ("remotePathId", remote)] {
        let Some(value) = value else {
            diagnostics.push(StandardDiagnostic::error(
                "smart_routing_path_required",
                format!("smartRouting.{field}"),
                "smart routing requires explicit domestic and remote path IDs",
            ));
            continue;
        };
        if !path_ids.contains(value) {
            diagnostics.push(StandardDiagnostic::error(
                "smart_routing_path_missing",
                format!("smartRouting.{field}"),
                format!("smart routing references missing path '{value}'"),
            ));
        }
    }
    if domestic.is_some() && domestic == remote {
        diagnostics.push(StandardDiagnostic::error(
            "smart_routing_paths_not_isolated",
            "smartRouting",
            "domestic and remote paths must be different to preserve cache and upstream isolation",
        ));
    }
    if smart.fallback_threshold_ms == 0 {
        diagnostics.push(StandardDiagnostic::error(
            "smart_routing_threshold_invalid",
            "smartRouting.fallbackThresholdMs",
            "fallback threshold must be greater than zero",
        ));
    }
    if !intent.rule_data.domestic_domains.has_enabled_sources() {
        diagnostics.push(StandardDiagnostic::error(
            "domestic_domains_required",
            "ruleData.domesticDomains",
            "smart routing requires an enabled domestic_domains source",
        ));
    }
    if !intent.rule_data.domestic_ips.has_enabled_sources() {
        diagnostics.push(StandardDiagnostic::error(
            "domestic_ips_required",
            "ruleData.domesticIps",
            "smart routing requires an enabled domestic_ips source for response validation",
        ));
    }
    if matches!(smart.unknown_mode, StandardUnknownMode::StrictRemote)
        && smart.privacy_fallback_to_domestic
    {
        diagnostics.push(StandardDiagnostic::error(
            "strict_remote_domestic_fallback_forbidden",
            "smartRouting.privacyFallbackToDomestic",
            "strict-remote mode cannot fall back to a domestic path",
        ));
    }
    for (kind, plugin) in [
        ("matcher", "qname"),
        ("matcher", "qtype"),
        ("matcher", "resp_ip"),
        ("matcher", "rcode"),
        ("matcher", "has_wanted_ans"),
        ("matcher", "cname"),
        ("executor", "drop_resp"),
        ("executor", "fallback"),
    ] {
        let available = if kind == "matcher" {
            capabilities.matcher(plugin)
        } else {
            capabilities.executor(plugin)
        };
        if !available {
            diagnostics.push(missing_plugin("smartRouting", kind, plugin));
        }
    }
}

fn validate_paths(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    if intent.paths.is_empty() {
        diagnostics.push(StandardDiagnostic::error(
            "resolution_path_required",
            "paths",
            "at least one resolution path is required",
        ));
        return;
    }
    let group_ids: BTreeSet<_> = intent
        .upstream_groups
        .iter()
        .map(|group| normalize_id(&group.id))
        .collect();
    for (index, path) in intent.paths.iter().enumerate() {
        let base = format!("paths[{index}]");
        if !group_ids.contains(&normalize_id(&path.upstream_group_id)) {
            diagnostics.push(StandardDiagnostic::error(
                "path_upstream_group_missing",
                format!("{base}.upstreamGroupId"),
                format!(
                    "resolution path references missing upstream group '{}'",
                    path.upstream_group_id
                ),
            ));
        }
        match path.dual_stack {
            StandardDualStackPolicy::PreferIpv4 if !capabilities.executor("prefer_ipv4") => {
                diagnostics.push(missing_plugin(
                    format!("{base}.dualStack"),
                    "executor",
                    "prefer_ipv4",
                ));
            }
            StandardDualStackPolicy::PreferIpv6 if !capabilities.executor("prefer_ipv6") => {
                diagnostics.push(missing_plugin(
                    format!("{base}.dualStack"),
                    "executor",
                    "prefer_ipv6",
                ));
            }
            StandardDualStackPolicy::Ipv4Only | StandardDualStackPolicy::Ipv6Only => {
                if !capabilities.matcher("qtype") {
                    diagnostics.push(missing_plugin(
                        format!("{base}.dualStack"),
                        "matcher",
                        "qtype",
                    ));
                }
                if !capabilities.executor("black_hole") {
                    diagnostics.push(missing_plugin(
                        format!("{base}.dualStack"),
                        "executor",
                        "black_hole",
                    ));
                }
            }
            _ => {}
        }
        if path.ip_selection.enabled {
            if !capabilities.executor("ip_selector") {
                diagnostics.push(missing_plugin(
                    format!("{base}.ipSelection"),
                    "executor",
                    "ip_selector",
                ));
            }
            let selection = &path.ip_selection;
            if selection.probe_methods.is_empty()
                || selection.probe_timeout_ms == 0
                || selection.max_wait_ms == 0
                || selection.top_n == 0
                || selection.max_parallel_probes == 0
                || (selection.cache_enabled
                    && (selection.cache_size == 0
                        || selection.cache_ttl_seconds == 0
                        || selection.failure_ttl_seconds == 0))
            {
                diagnostics.push(StandardDiagnostic::error(
                    "ip_selection_limits_invalid",
                    format!("{base}.ipSelection"),
                    "enabled IP selection requires non-zero methods, budgets, limits, and cache TTLs",
                ));
            }
        }
        match &path.ecs {
            StandardEcsPolicy::Inherit => {}
            StandardEcsPolicy::ClientSubnet { mask4, mask6 }
            | StandardEcsPolicy::Preset { mask4, mask6, .. } => {
                if *mask4 > 32 || *mask6 > 128 {
                    diagnostics.push(StandardDiagnostic::error(
                        "ecs_prefix_invalid",
                        format!("{base}.ecs"),
                        "ECS IPv4/IPv6 prefixes must be within 0..=32 and 0..=128",
                    ));
                }
                if let StandardEcsPolicy::Preset { address, .. } = &path.ecs
                    && address.parse::<IpAddr>().is_err()
                {
                    diagnostics.push(StandardDiagnostic::error(
                        "ecs_preset_invalid",
                        format!("{base}.ecs.address"),
                        "ECS preset address must be an IPv4 or IPv6 address",
                    ));
                }
                if !capabilities.executor("ecs_handler") {
                    diagnostics.push(missing_plugin(
                        format!("{base}.ecs"),
                        "executor",
                        "ecs_handler",
                    ));
                }
            }
            StandardEcsPolicy::Remove | StandardEcsPolicy::PreserveClient => {
                if !capabilities.executor("ecs_handler") {
                    diagnostics.push(missing_plugin(
                        format!("{base}.ecs"),
                        "executor",
                        "ecs_handler",
                    ));
                }
            }
        }
    }
    if effective_query_log_used(intent) && !capabilities.executor("query_recorder") {
        diagnostics.push(missing_plugin("queryLog", "executor", "query_recorder"));
    }
    if (intent.query_log.sample_rate - 1.0).abs() > f64::EPSILON {
        diagnostics.push(StandardDiagnostic::error(
            "query_log_sampling_not_available",
            "queryLog.sampleRate",
            "query log sampling is not compiled in Phase 0; sampleRate must be 1",
        ));
    }
}

fn validate_rules(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    let path_ids: BTreeSet<_> = intent
        .paths
        .iter()
        .map(|path| normalize_id(&path.id))
        .collect();
    if intent
        .routing
        .scenarios
        .iter()
        .any(|scenario| scenario.enabled)
    {
        diagnostics.push(StandardDiagnostic::error(
            "scenario_not_available",
            "routing.scenarios",
            "scenario templates are not compiled in Phase 0",
        ));
    }
    if intent.routing.enabled {
        for (index, rule) in intent
            .routing
            .rules
            .iter()
            .enumerate()
            .filter(|(_, r)| r.enabled)
        {
            validate_rule_condition(
                &rule.condition,
                capabilities,
                &format!("routing.rules[{index}].condition"),
                diagnostics,
            );
            match &rule.action {
                StandardRuleAction::UsePath { path_id } => {
                    if !path_ids.contains(&normalize_id(path_id)) {
                        diagnostics.push(StandardDiagnostic::error(
                            "rule_path_missing",
                            format!("routing.rules[{index}].action.pathId"),
                            format!("routing rule references missing path '{path_id}'"),
                        ));
                    }
                }
                StandardRuleAction::UseDefaultPath => {}
                _ => diagnostics.push(StandardDiagnostic::error(
                    "routing_action_unsupported",
                    format!("routing.rules[{index}].action"),
                    "routing rules may only select a path in Phase 0",
                )),
            }
        }
    }

    for (index, rule) in intent
        .exceptions
        .iter()
        .enumerate()
        .filter(|(_, r)| r.enabled)
    {
        validate_rule_condition(
            &rule.condition,
            capabilities,
            &format!("exceptions[{index}].condition"),
            diagnostics,
        );
        match &rule.action {
            StandardRuleAction::UsePath { path_id } => {
                if !path_ids.contains(&normalize_id(path_id)) {
                    diagnostics.push(StandardDiagnostic::error(
                        "exception_path_missing",
                        format!("exceptions[{index}].action.pathId"),
                        format!("exception references missing path '{path_id}'"),
                    ));
                }
            }
            StandardRuleAction::Block if !capabilities.executor("black_hole") => {
                diagnostics.push(missing_plugin(
                    format!("exceptions[{index}].action"),
                    "executor",
                    "black_hole",
                ));
            }
            StandardRuleAction::PreferIpv4 if !capabilities.executor("prefer_ipv4") => {
                diagnostics.push(missing_plugin(
                    format!("exceptions[{index}].action"),
                    "executor",
                    "prefer_ipv4",
                ));
            }
            StandardRuleAction::PreferIpv6 if !capabilities.executor("prefer_ipv6") => {
                diagnostics.push(missing_plugin(
                    format!("exceptions[{index}].action"),
                    "executor",
                    "prefer_ipv6",
                ));
            }
            _ => {}
        }
    }
}

fn validate_devices(
    intent: &StandardIntent,
    capabilities: &StandardCapabilities,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    let path_ids: BTreeSet<_> = intent
        .paths
        .iter()
        .map(|path| normalize_id(&path.id))
        .collect();
    let mut claimed_addresses = BTreeMap::<String, String>::new();
    for (index, device) in intent.devices.iter().enumerate() {
        let path = format!("devices[{index}]");
        if device.addresses.is_empty() {
            diagnostics.push(StandardDiagnostic::error(
                "device_address_required",
                format!("{path}.addresses"),
                "device requires at least one IP address or CIDR",
            ));
        }
        for (address_index, address) in device.addresses.iter().enumerate() {
            if !valid_client_address(address) {
                diagnostics.push(StandardDiagnostic::error(
                    "device_address_invalid",
                    format!("{path}.addresses[{address_index}]"),
                    format!("'{address}' is not a valid IP address or CIDR"),
                ));
            }
            if let Some(first_path) = claimed_addresses.insert(address.clone(), path.clone()) {
                diagnostics.push(StandardDiagnostic::error(
                    "device_address_duplicate",
                    format!("{path}.addresses[{address_index}]"),
                    format!("device address '{address}' is already owned by {first_path}"),
                ));
            }
        }
        if let Some(path_id) = &device.assigned_path_id
            && !path_ids.contains(&normalize_id(path_id))
        {
            diagnostics.push(StandardDiagnostic::error(
                "device_path_missing",
                format!("{path}.assignedPathId"),
                format!("device references missing path '{path_id}'"),
            ));
        }
        if device_has_policy(device) && !capabilities.matcher("client_ip") {
            diagnostics.push(missing_plugin(
                format!("{path}.addresses"),
                "matcher",
                "client_ip",
            ));
        }
    }
}

fn validate_system(intent: &StandardIntent, diagnostics: &mut Vec<StandardDiagnostic>) {
    if matches!(intent.system.threads, Some(0)) {
        diagnostics.push(StandardDiagnostic::error(
            "runtime_worker_threads_invalid",
            "system.threads",
            "worker thread count must be greater than zero",
        ));
    }
}

fn validate_rule_condition(
    condition: &StandardRuleCondition,
    capabilities: &StandardCapabilities,
    path: &str,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    let (values, matcher) = match condition {
        StandardRuleCondition::Domain { values }
        | StandardRuleCondition::Suffix { values }
        | StandardRuleCondition::Keyword { values } => (Some(values), Some("qname")),
        StandardRuleCondition::ClientCidr { values } => (Some(values), Some("client_ip")),
        StandardRuleCondition::Qtype { values } => (Some(values), Some("qtype")),
        StandardRuleCondition::ClientName { .. } => {
            diagnostics.push(StandardDiagnostic::error(
                "condition_client_name_not_available",
                path,
                "client-name matching requires an external device source and is outside Standard Mode",
            ));
            (None, None)
        }
        StandardRuleCondition::Subscription { .. } => {
            diagnostics.push(StandardDiagnostic::error(
                "condition_subscription_not_available",
                path,
                "subscription-backed routing rules are not compiled in Phase 0",
            ));
            (None, None)
        }
    };
    if matches!(values, Some(values) if values.is_empty()) {
        diagnostics.push(StandardDiagnostic::error(
            "condition_values_required",
            path,
            "rule condition requires at least one value",
        ));
    }
    if let Some(matcher) = matcher
        && !capabilities.matcher(matcher)
    {
        diagnostics.push(missing_plugin(path, "matcher", matcher));
    }
}

fn validate_unique_objects<'a>(
    values: impl Iterator<Item = (usize, &'a str, &'a str)>,
    base: &str,
    diagnostics: &mut Vec<StandardDiagnostic>,
) {
    let mut ids = BTreeMap::<String, usize>::new();
    let mut names = BTreeMap::<String, usize>::new();
    for (index, id, name) in values {
        let normalized_id = normalize_id(id);
        if normalized_id.is_empty() {
            diagnostics.push(StandardDiagnostic::error(
                "id_required",
                format!("{base}[{index}].id"),
                "ID is required",
            ));
        } else if let Some(first_index) = ids.insert(normalized_id.clone(), index) {
            diagnostics.push(StandardDiagnostic::error(
                "id_duplicate",
                format!("{base}[{index}].id"),
                format!("ID '{normalized_id}' duplicates {base}[{first_index}]"),
            ));
        }
        let normalized_name = name.trim().to_lowercase();
        if normalized_name.is_empty() {
            diagnostics.push(StandardDiagnostic::error(
                "name_required",
                format!("{base}[{index}].name"),
                "name is required",
            ));
        } else if let Some(first_index) = names.insert(normalized_name, index) {
            diagnostics.push(StandardDiagnostic::error(
                "name_duplicate",
                format!("{base}[{index}].name"),
                format!("name duplicates {base}[{first_index}]"),
            ));
        }
    }
}

fn generated_user_tags(intent: &StandardIntent) -> Vec<(String, String)> {
    let mut tags = Vec::new();
    for (index, group) in intent.upstream_groups.iter().enumerate() {
        tags.push((
            format!("upstreamGroups[{index}].id"),
            standard_tag("forward", &group.id),
        ));
    }
    for (index, path) in intent.paths.iter().enumerate() {
        tags.push((format!("paths[{index}].id"), standard_tag("path", &path.id)));
        tags.push((
            format!("paths[{index}].id"),
            standard_tag("cache", &path.id),
        ));
    }
    for (index, rule) in intent.routing.rules.iter().enumerate() {
        tags.push((
            format!("routing.rules[{index}].id"),
            standard_tag("route_match", &rule.id),
        ));
    }
    for (index, rule) in intent.exceptions.iter().enumerate() {
        tags.push((
            format!("exceptions[{index}].id"),
            standard_tag("exception_match", &rule.id),
        ));
        tags.push((
            format!("exceptions[{index}].id"),
            standard_tag("exception_action", &rule.id),
        ));
    }
    for (index, device) in intent.devices.iter().enumerate() {
        tags.push((
            format!("devices[{index}].id"),
            standard_tag("device_match", &device.id),
        ));
        tags.push((
            format!("devices[{index}].id"),
            standard_tag("device_action", &device.id),
        ));
    }
    tags
}

pub(super) fn standard_tag(prefix: &str, id: &str) -> String {
    format!("standard_{prefix}_{}", safe_tag_component(id))
}

pub(super) fn safe_tag_component(value: &str) -> String {
    let value = normalize_id(value);
    if value.is_empty() {
        "default".to_string()
    } else {
        value
    }
}

fn normalize_id(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut previous_underscore = false;
    for character in value.trim().chars().flat_map(char::to_lowercase) {
        let character = if character.is_ascii_alphanumeric() || character == '-' {
            character
        } else {
            '_'
        };
        if character == '_' {
            if previous_underscore || normalized.is_empty() {
                continue;
            }
            previous_underscore = true;
        } else {
            previous_underscore = false;
        }
        normalized.push(character);
    }
    while normalized.ends_with('_') {
        normalized.pop();
    }
    normalized
}

fn trimmed_option(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_lines(lines: &mut Vec<String>, adguard_allow: bool) {
    let mut seen = BTreeSet::new();
    lines.retain_mut(|line| {
        *line = line.trim().to_string();
        if adguard_allow && !line.is_empty() && !line.starts_with("@@") {
            *line = format!("@@{line}");
        }
        !line.is_empty() && seen.insert(line.clone())
    });
}

fn normalize_condition(condition: &mut StandardRuleCondition) {
    match condition {
        StandardRuleCondition::Domain { values }
        | StandardRuleCondition::Keyword { values }
        | StandardRuleCondition::ClientCidr { values }
        | StandardRuleCondition::ClientName { values } => normalize_lines(values, false),
        StandardRuleCondition::Suffix { values } => {
            normalize_lines(values, false);
            for value in values {
                *value = value.trim_start_matches('.').to_string();
            }
        }
        StandardRuleCondition::Qtype { values } => {
            normalize_lines(values, false);
            for value in values {
                value.make_ascii_uppercase();
            }
        }
        StandardRuleCondition::Subscription { subscription_id } => {
            *subscription_id = normalize_id(subscription_id);
        }
    }
}

fn normalize_action(action: &mut StandardRuleAction) {
    if let StandardRuleAction::UsePath { path_id } = action {
        *path_id = normalize_id(path_id);
    }
}

fn protocol_features(protocol: StandardUpstreamProtocol) -> &'static [&'static str] {
    match protocol {
        StandardUpstreamProtocol::Auto
        | StandardUpstreamProtocol::Udp
        | StandardUpstreamProtocol::Tcp => &[],
        StandardUpstreamProtocol::Dot => &["upstream-dot"],
        StandardUpstreamProtocol::Doh => &["upstream-doh"],
        StandardUpstreamProtocol::Doh3 => &["upstream-doh", "upstream-doh3"],
        StandardUpstreamProtocol::Doq => &["upstream-doq"],
    }
}

fn effective_cache_paths(intent: &StandardIntent) -> usize {
    intent
        .paths
        .iter()
        .filter(|path| {
            matches!(path.cache, StandardPolicySwitch::Enabled)
                || (matches!(path.cache, StandardPolicySwitch::Inherit) && intent.cache.enabled)
        })
        .count()
}

pub(super) fn effective_filtering_used(intent: &StandardIntent) -> bool {
    intent.paths.iter().any(|path| {
        matches!(path.filtering, StandardPolicySwitch::Enabled)
            || (matches!(path.filtering, StandardPolicySwitch::Inherit) && intent.filtering.enabled)
    }) || intent
        .devices
        .iter()
        .any(|device| matches!(device.filtering, Some(StandardPolicySwitch::Enabled)))
}

pub(super) fn effective_query_log_used(intent: &StandardIntent) -> bool {
    intent.paths.iter().any(|path| {
        matches!(path.query_log, StandardPolicySwitch::Enabled)
            || (matches!(path.query_log, StandardPolicySwitch::Inherit) && intent.query_log.enabled)
    }) || intent
        .devices
        .iter()
        .any(|device| matches!(device.query_log, Some(StandardPolicySwitch::Enabled)))
}

pub(super) fn device_has_policy(device: &super::model::StandardDeviceProfile) -> bool {
    device.assigned_path_id.is_some()
        || matches!(
            device.filtering,
            Some(StandardPolicySwitch::Enabled | StandardPolicySwitch::Disabled)
        )
        || matches!(
            device.query_log,
            Some(StandardPolicySwitch::Enabled | StandardPolicySwitch::Disabled)
        )
}

fn missing_plugin(path: impl Into<String>, category: &str, kind: &str) -> StandardDiagnostic {
    StandardDiagnostic::error(
        "required_capability_missing",
        path,
        format!("required {category} plugin '{kind}' is not available in this build"),
    )
}

fn valid_http_url(value: &str) -> bool {
    let value = value.trim();
    (value.starts_with("http://") || value.starts_with("https://"))
        && !value.chars().any(char::is_whitespace)
        && value
            .split_once("://")
            .is_some_and(|(_, rest)| !rest.is_empty())
}

fn valid_client_address(value: &str) -> bool {
    let value = value.trim();
    if let Some((address, prefix)) = value.split_once('/') {
        if prefix.contains('/') {
            return false;
        }
        let Ok(address) = address.parse::<IpAddr>() else {
            return false;
        };
        let Ok(prefix) = prefix.parse::<u8>() else {
            return false;
        };
        return prefix <= if address.is_ipv4() { 32 } else { 128 };
    }
    value.parse::<IpAddr>().is_ok()
}
