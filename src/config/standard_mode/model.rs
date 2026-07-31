// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub const CURRENT_STANDARD_SCHEMA: u32 = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardIntent {
    pub schema: u32,
    #[serde(default)]
    pub listen: StandardListenSettings,
    #[serde(default)]
    pub upstream_groups: Vec<StandardUpstreamGroup>,
    #[serde(default)]
    pub paths: Vec<StandardResolutionPath>,
    #[serde(default)]
    pub filtering: StandardFilteringSettings,
    #[serde(default)]
    pub local: StandardLocalSettings,
    #[serde(default)]
    pub cache: StandardCacheSettings,
    #[serde(default)]
    pub query_log: StandardQueryLogSettings,
    #[serde(default)]
    pub routing: StandardRoutingSettings,
    #[serde(default)]
    pub exceptions: Vec<StandardExceptionRule>,
    #[serde(default)]
    pub devices: Vec<StandardDeviceProfile>,
    #[serde(default)]
    pub system: StandardSystemSettings,
}

impl Default for StandardIntent {
    fn default() -> Self {
        Self {
            schema: CURRENT_STANDARD_SCHEMA,
            listen: StandardListenSettings::default(),
            upstream_groups: vec![StandardUpstreamGroup::default()],
            paths: vec![StandardResolutionPath::default()],
            filtering: StandardFilteringSettings::default(),
            local: StandardLocalSettings::default(),
            cache: StandardCacheSettings::default(),
            query_log: StandardQueryLogSettings::default(),
            routing: StandardRoutingSettings::default(),
            exceptions: Vec::new(),
            devices: Vec::new(),
            system: StandardSystemSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardListenSettings {
    #[serde(default = "default_listen_address")]
    pub address: String,
    #[serde(default = "default_true")]
    pub udp: bool,
    #[serde(default = "default_true")]
    pub tcp: bool,
}

impl Default for StandardListenSettings {
    fn default() -> Self {
        Self {
            address: default_listen_address(),
            udp: true,
            tcp: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardUpstreamGroup {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub strategy: StandardUpstreamStrategy,
    #[serde(default)]
    pub upstreams: Vec<StandardUpstream>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_default: bool,
}

impl Default for StandardUpstreamGroup {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "Default upstream group".to_string(),
            description: None,
            strategy: StandardUpstreamStrategy::Balanced,
            upstreams: vec![
                StandardUpstream::new("alidns", "AliDNS", "223.5.5.5:53"),
                StandardUpstream::new("cloudflare", "Cloudflare", "1.1.1.1:53"),
            ],
            is_default: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardUpstreamStrategy {
    Fastest,
    #[default]
    Balanced,
    PreferPositive,
    Consensus,
    OrderedFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardUpstream {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub protocol: StandardUpstreamProtocol,
    pub address: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap_version: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dial_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outbound: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks5: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_conns: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_conns: Option<usize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enable_pipeline: bool,
    #[serde(default = "default_true")]
    pub tls_verify: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doh_path: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enable_http3: bool,
}

impl StandardUpstream {
    fn new(id: &str, name: &str, address: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            protocol: StandardUpstreamProtocol::Auto,
            address: address.to_string(),
            enabled: true,
            bootstrap: None,
            bootstrap_version: None,
            dial_address: None,
            outbound: None,
            socks5: None,
            timeout_seconds: None,
            idle_timeout_seconds: None,
            max_conns: None,
            min_conns: None,
            enable_pipeline: false,
            tls_verify: true,
            doh_path: None,
            enable_http3: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardUpstreamProtocol {
    #[default]
    Auto,
    Udp,
    Tcp,
    Dot,
    Doh,
    Doh3,
    Doq,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardResolutionPath {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub upstream_group_id: String,
    #[serde(default)]
    pub filtering: StandardPolicySwitch,
    #[serde(default)]
    pub cache: StandardPolicySwitch,
    #[serde(default)]
    pub query_log: StandardPolicySwitch,
    #[serde(default)]
    pub dual_stack: StandardDualStackPolicy,
    #[serde(default)]
    pub ip_selection: StandardPolicySwitch,
    #[serde(default)]
    pub ecs: StandardPolicySwitch,
}

impl Default for StandardResolutionPath {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "Default path".to_string(),
            description: None,
            upstream_group_id: "default".to_string(),
            filtering: StandardPolicySwitch::Inherit,
            cache: StandardPolicySwitch::Inherit,
            query_log: StandardPolicySwitch::Inherit,
            dual_stack: StandardDualStackPolicy::Inherit,
            ip_selection: StandardPolicySwitch::Inherit,
            ecs: StandardPolicySwitch::Inherit,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardPolicySwitch {
    #[default]
    Inherit,
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardDualStackPolicy {
    #[default]
    Inherit,
    Disabled,
    PreferIpv4,
    PreferIpv6,
    Ipv4Only,
    Ipv6Only,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardCacheSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cache_size")]
    pub size: usize,
    #[serde(default = "default_min_positive_ttl")]
    pub min_positive_ttl: u32,
    #[serde(default = "default_max_positive_ttl")]
    pub max_positive_ttl: u32,
    #[serde(default = "default_max_negative_ttl")]
    pub max_negative_ttl: u32,
    #[serde(default = "default_negative_ttl_without_soa")]
    pub negative_ttl_without_soa: u32,
}

impl Default for StandardCacheSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            size: default_cache_size(),
            min_positive_ttl: default_min_positive_ttl(),
            max_positive_ttl: default_max_positive_ttl(),
            max_negative_ttl: default_max_negative_ttl(),
            negative_ttl_without_soa: default_negative_ttl_without_soa(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardQueryLogSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: f64,
}

impl Default for StandardQueryLogSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: default_retention_days(),
            sample_rate: default_sample_rate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardFilteringSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub subscriptions: Vec<StandardSubscription>,
    #[serde(default)]
    pub local_files: Vec<StandardFilterFile>,
    #[serde(default)]
    pub block_rules: Vec<String>,
    #[serde(default)]
    pub allow_rules: Vec<String>,
    #[serde(default)]
    pub block_response: StandardBlockResponse,
}

impl Default for StandardFilteringSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            subscriptions: Vec::new(),
            local_files: Vec::new(),
            block_rules: Vec::new(),
            allow_rules: Vec::new(),
            block_response: StandardBlockResponse::NullIp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardSubscription {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_update_interval_hours")]
    pub update_interval_hours: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardFilterFile {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardBlockResponse {
    #[default]
    NullIp,
    Nxdomain,
    Nodata,
    Refused,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardLocalSettings {
    #[serde(default)]
    pub hosts: StandardLocalHosts,
    #[serde(default)]
    pub redirects: StandardLocalRedirects,
    #[serde(default)]
    pub records: StandardLocalRecords,
    #[serde(default)]
    pub response_ttl: StandardResponseTtl,
    #[serde(default)]
    pub qtype_policy: StandardQtypePolicy,
    #[serde(default)]
    pub ddns: StandardDdnsPolicy,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardLocalHosts {
    #[serde(default)]
    pub entries: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardLocalRedirects {
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardLocalRecords {
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardResponseTtl {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

impl Default for StandardResponseTtl {
    fn default() -> Self {
        Self {
            enabled: false,
            min: Some(30),
            max: Some(86_400),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardQtypePolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub qtypes: Vec<String>,
    #[serde(default)]
    pub response: StandardBlockResponse,
}

impl Default for StandardQtypePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            qtypes: Vec::new(),
            response: StandardBlockResponse::Nodata,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardDdnsPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_id: Option<String>,
    #[serde(default = "default_ddns_ttl")]
    pub ttl: u32,
}

impl Default for StandardDdnsPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            domains: Vec::new(),
            path_id: None,
            ttl: default_ddns_ttl(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardRoutingSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<StandardRoutingRule>,
    #[serde(default)]
    pub scenarios: Vec<StandardScenario>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardRoutingRule {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub condition: StandardRuleCondition,
    pub action: StandardRuleAction,
    #[serde(default)]
    pub source: StandardRuleSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardExceptionRule {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub condition: StandardRuleCondition,
    pub action: StandardRuleAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StandardRuleCondition {
    Domain {
        values: Vec<String>,
    },
    Suffix {
        values: Vec<String>,
    },
    Keyword {
        values: Vec<String>,
    },
    ClientCidr {
        values: Vec<String>,
    },
    ClientName {
        values: Vec<String>,
    },
    Qtype {
        values: Vec<String>,
    },
    Subscription {
        #[serde(rename = "subscriptionId")]
        subscription_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StandardRuleAction {
    UsePath {
        #[serde(rename = "pathId")]
        path_id: String,
    },
    UseDefaultPath,
    Block,
    Allow,
    SkipFiltering,
    PreferIpv4,
    PreferIpv6,
    DisableLogging,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardRuleSource {
    #[default]
    Manual,
    Scenario,
    Subscription,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardScenario {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub kind: StandardScenarioKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardScenarioKind {
    Privacy,
    Gaming,
    ChildProtection,
    DomesticOptimization,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardDeviceProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_path_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filtering: Option<StandardPolicySwitch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_log: Option<StandardPolicySwitch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardSystemSettings {
    #[serde(default)]
    pub log_level: StandardLogLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threads: Option<usize>,
}

impl Default for StandardSystemSettings {
    fn default() -> Self {
        Self {
            log_level: StandardLogLevel::Info,
            threads: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardLogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl StandardLogLevel {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardDiagnosticSeverity {
    Error,
    Warning,
    Suggestion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardDiagnostic {
    pub severity: StandardDiagnosticSeverity,
    pub code: String,
    pub path: String,
    pub message: String,
}

impl StandardDiagnostic {
    pub(super) fn error(
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: StandardDiagnosticSeverity::Error,
            code: code.into(),
            path: path.into(),
            message: message.into(),
        }
    }

    pub(super) fn warning(
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: StandardDiagnosticSeverity::Warning,
            code: code.into(),
            path: path.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardMigration {
    pub from_schema: u32,
    pub to_schema: u32,
    pub diagnostics: Vec<StandardDiagnostic>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardTagMap {
    pub system: Vec<String>,
    pub caches: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_log: Option<String>,
    pub filtering: Vec<String>,
    pub filter_subscriptions: BTreeMap<String, StandardSubscriptionTagMap>,
    pub local: BTreeMap<String, String>,
    pub upstream_groups: BTreeMap<String, String>,
    pub paths: BTreeMap<String, String>,
    pub routing_rules: BTreeMap<String, String>,
    pub exception_rules: BTreeMap<String, String>,
    pub devices: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardSubscriptionTagMap {
    pub download: String,
    pub cron: String,
    pub job: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardGenerationSummary {
    pub upstream_group_count: usize,
    pub path_count: usize,
    pub enabled_upstream_count: usize,
    pub filtering_enabled: bool,
    pub cache_enabled: bool,
    pub query_log_enabled: bool,
    pub routing_rule_count: usize,
    pub exception_rule_count: usize,
    pub device_count: usize,
    pub local_policy_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardGeneratedConfig {
    pub yaml: String,
    pub config_version: String,
    pub plugin_count: usize,
    pub generated_tags: Vec<String>,
    pub tag_map: StandardTagMap,
    pub summary: StandardGenerationSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardPlan {
    pub normalized_intent: StandardIntent,
    pub diagnostics: Vec<StandardDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated: Option<StandardGeneratedConfig>,
    pub can_apply: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration: Option<StandardMigration>,
    #[serde(default)]
    pub details: JsonValue,
}

const fn default_true() -> bool {
    true
}

const fn default_ddns_ttl() -> u32 {
    30
}

fn default_listen_address() -> String {
    "0.0.0.0:5335".to_string()
}

const fn default_cache_size() -> usize {
    8192
}

const fn default_min_positive_ttl() -> u32 {
    60
}

const fn default_max_positive_ttl() -> u32 {
    86_400
}

const fn default_max_negative_ttl() -> u32 {
    300
}

const fn default_negative_ttl_without_soa() -> u32 {
    300
}

const fn default_retention_days() -> u32 {
    7
}

const fn default_sample_rate() -> f64 {
    1.0
}

const fn default_update_interval_hours() -> u32 {
    24
}
