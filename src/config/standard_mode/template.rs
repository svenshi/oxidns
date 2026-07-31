// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Side-effect-free Standard Mode scenario template expansion.

use serde::{Deserialize, Serialize};

use super::model::{
    StandardDedicatedGroup, StandardDedicatedListener, StandardDedicatedPathPolicy,
    StandardDualStackPolicy, StandardEcsPolicy, StandardIntent, StandardIpSelectionMode,
    StandardPolicySwitch, StandardUpstream, StandardUpstreamProtocol, StandardUpstreamStrategy,
};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardTemplateKind {
    LowLatency,
    PrivacyDns,
    InternalDomains,
    RegionalUpstream,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardTemplateParameters {
    pub namespace: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub domains: Vec<String>,
    pub upstreams: Vec<StandardUpstream>,
    #[serde(default)]
    pub listener_address: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardTemplateExpansion {
    pub proposed_intent: StandardIntent,
    pub objects_added: Vec<String>,
    pub objects_modified: Vec<String>,
    pub explanation_tags: Vec<String>,
}

pub fn expand_standard_template(
    mut intent: StandardIntent,
    kind: StandardTemplateKind,
    parameters: StandardTemplateParameters,
) -> std::result::Result<StandardTemplateExpansion, String> {
    let namespace = parameters.namespace.trim().to_ascii_lowercase();
    if namespace.is_empty()
        || !namespace
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("template namespace must contain only letters, digits, '-' or '_'".to_string());
    }
    if parameters.name.trim().is_empty()
        || parameters.domains.is_empty()
        || parameters.upstreams.is_empty()
    {
        return Err(
            "template requires a name, at least one domain, and at least one upstream".to_string(),
        );
    }
    if intent
        .upstream_groups
        .iter()
        .any(|item| item.id.eq_ignore_ascii_case(&namespace))
        || intent
            .paths
            .iter()
            .any(|item| item.id.eq_ignore_ascii_case(&namespace))
        || intent
            .dedicated_groups
            .iter()
            .any(|item| item.id.eq_ignore_ascii_case(&namespace))
        || intent
            .dynamic_learning
            .profiles
            .iter()
            .any(|item| item.id.eq_ignore_ascii_case(&namespace))
        || intent
            .advanced_rules
            .iter()
            .any(|item| item.id.eq_ignore_ascii_case(&namespace))
        || intent
            .routing
            .rules
            .iter()
            .any(|item| item.id.eq_ignore_ascii_case(&namespace))
        || intent
            .exceptions
            .iter()
            .any(|item| item.id.eq_ignore_ascii_case(&namespace))
        || intent
            .devices
            .iter()
            .any(|item| item.id.eq_ignore_ascii_case(&namespace))
    {
        return Err(format!(
            "template namespace '{namespace}' collides with an existing object"
        ));
    }
    if matches!(kind, StandardTemplateKind::PrivacyDns)
        && parameters.upstreams.iter().any(|upstream| {
            !matches!(
                upstream.protocol,
                StandardUpstreamProtocol::Dot
                    | StandardUpstreamProtocol::Doh
                    | StandardUpstreamProtocol::Doh3
                    | StandardUpstreamProtocol::Doq
            )
        })
    {
        return Err(
            "privacy_dns requires every upstream to use DoT, DoH, DoH3, or DoQ".to_string(),
        );
    }

    let mut path = StandardDedicatedPathPolicy::default();
    let (strategy, explanation) = match kind {
        StandardTemplateKind::LowLatency => {
            path.ip_selection.enabled = true;
            path.ip_selection.selection_mode = StandardIpSelectionMode::BestWithinBudget;
            (StandardUpstreamStrategy::Fastest, "latency_optimized")
        }
        StandardTemplateKind::PrivacyDns => {
            path.ecs = StandardEcsPolicy::Remove;
            path.cache = StandardPolicySwitch::Enabled;
            (
                StandardUpstreamStrategy::OrderedFallback,
                "encrypted_ecs_removed",
            )
        }
        StandardTemplateKind::InternalDomains => {
            path.dual_stack = StandardDualStackPolicy::Disabled;
            (
                StandardUpstreamStrategy::OrderedFallback,
                "internal_authority",
            )
        }
        StandardTemplateKind::RegionalUpstream => {
            path.ecs = StandardEcsPolicy::ClientSubnet {
                mask4: 24,
                mask6: 56,
            };
            path.cache = StandardPolicySwitch::Enabled;
            (StandardUpstreamStrategy::Balanced, "regional_ecs_isolated")
        }
    };
    let listener = parameters
        .listener_address
        .filter(|address| !address.trim().is_empty())
        .map_or_else(StandardDedicatedListener::default, |address| {
            StandardDedicatedListener {
                enabled: true,
                address,
                udp: true,
                tcp: true,
            }
        });
    intent.dedicated_groups.push(StandardDedicatedGroup {
        id: namespace.clone(),
        name: parameters.name.trim().to_string(),
        description: parameters.description,
        enabled: true,
        priority: 100,
        rules: parameters.domains,
        strategy,
        upstreams: parameters.upstreams,
        path,
        listener,
    });

    Ok(StandardTemplateExpansion {
        proposed_intent: intent,
        objects_added: vec![format!("dedicatedGroups.{namespace}")],
        objects_modified: Vec::new(),
        explanation_tags: vec![explanation.to_string(), format!("template:{namespace}")],
    })
}
