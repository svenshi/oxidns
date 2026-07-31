// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub const CURRENT_STANDARD_SCHEMA: u32 = 6;

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
    pub rule_data: StandardRuleDataSettings,
    #[serde(default)]
    pub smart_routing: StandardSmartRoutingSettings,
    #[serde(default)]
    pub dedicated_groups: Vec<StandardDedicatedGroup>,
    #[serde(default)]
    pub dynamic_learning: StandardDynamicLearningSettings,
    #[serde(default)]
    pub advanced_rules: Vec<StandardAdvancedRule>,
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
            rule_data: StandardRuleDataSettings::default(),
            smart_routing: StandardSmartRoutingSettings::default(),
            dedicated_groups: Vec::new(),
            dynamic_learning: StandardDynamicLearningSettings::default(),
            advanced_rules: Vec::new(),
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
    pub ip_selection: StandardIpSelectionSettings,
    #[serde(default)]
    pub ecs: StandardEcsPolicy,
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
            ip_selection: StandardIpSelectionSettings::default(),
            ecs: StandardEcsPolicy::Inherit,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "mode",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum StandardEcsPolicy {
    #[default]
    Inherit,
    Remove,
    PreserveClient,
    ClientSubnet {
        #[serde(default = "default_ecs_mask4")]
        mask4: u8,
        #[serde(default = "default_ecs_mask6")]
        mask6: u8,
    },
    Preset {
        address: String,
        #[serde(default = "default_ecs_mask4")]
        mask4: u8,
        #[serde(default = "default_ecs_mask6")]
        mask6: u8,
    },
}

impl StandardEcsPolicy {
    pub(super) const fn affects_cache_key(&self) -> bool {
        matches!(
            self,
            Self::PreserveClient | Self::ClientSubnet { .. } | Self::Preset { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardIpSelectionSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub selection_mode: StandardIpSelectionMode,
    #[serde(default = "default_probe_methods")]
    pub probe_methods: Vec<String>,
    #[serde(default = "default_probe_stagger_ms")]
    pub probe_stagger_ms: u64,
    #[serde(default = "default_probe_timeout_ms")]
    pub probe_timeout_ms: u64,
    #[serde(default = "default_probe_max_wait_ms")]
    pub max_wait_ms: u64,
    #[serde(default = "default_ip_selection_top_n")]
    pub top_n: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outbound: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socks5: Option<String>,
    #[serde(default)]
    pub dnssec_policy: StandardDnssecPolicy,
    #[serde(default = "default_ip_selection_parallel")]
    pub max_parallel_probes: usize,
    #[serde(default = "default_true")]
    pub cache_enabled: bool,
    #[serde(default = "default_ip_selection_cache_size")]
    pub cache_size: usize,
    #[serde(default = "default_ip_selection_cache_ttl")]
    pub cache_ttl_seconds: u64,
    #[serde(default = "default_ip_selection_failure_ttl")]
    pub failure_ttl_seconds: u64,
}

impl Default for StandardIpSelectionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            selection_mode: StandardIpSelectionMode::FirstSuccess,
            probe_methods: default_probe_methods(),
            probe_stagger_ms: default_probe_stagger_ms(),
            probe_timeout_ms: default_probe_timeout_ms(),
            max_wait_ms: default_probe_max_wait_ms(),
            top_n: default_ip_selection_top_n(),
            outbound: None,
            socks5: None,
            dnssec_policy: StandardDnssecPolicy::ReorderOnly,
            max_parallel_probes: default_ip_selection_parallel(),
            cache_enabled: true,
            cache_size: default_ip_selection_cache_size(),
            cache_ttl_seconds: default_ip_selection_cache_ttl(),
            failure_ttl_seconds: default_ip_selection_failure_ttl(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardIpSelectionMode {
    #[default]
    FirstSuccess,
    BestWithinBudget,
    Background,
}

impl StandardIpSelectionMode {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::FirstSuccess => "first_success",
            Self::BestWithinBudget => "best_within_budget",
            Self::Background => "background",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardDnssecPolicy {
    #[default]
    ReorderOnly,
    Skip,
}

impl StandardDnssecPolicy {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::ReorderOnly => "reorder_only",
            Self::Skip => "skip",
        }
    }
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
pub struct StandardRuleDataSettings {
    #[serde(default)]
    pub domestic_domains: StandardRuleDataRole,
    #[serde(default)]
    pub foreign_domains: StandardRuleDataRole,
    #[serde(default)]
    pub domestic_ips: StandardRuleDataRole,
    #[serde(default)]
    pub direct_domains: StandardRuleDataRole,
    #[serde(default)]
    pub remote_domains: StandardRuleDataRole,
    #[serde(default)]
    pub ddns_domains: StandardRuleDataRole,
}

impl StandardRuleDataSettings {
    pub(super) fn all_roles(&self) -> [(&'static str, &StandardRuleDataRole); 6] {
        [
            ("domestic_domains", &self.domestic_domains),
            ("foreign_domains", &self.foreign_domains),
            ("domestic_ips", &self.domestic_ips),
            ("direct_domains", &self.direct_domains),
            ("remote_domains", &self.remote_domains),
            ("ddns_domains", &self.ddns_domains),
        ]
    }

    pub(super) fn all_roles_mut(&mut self) -> [(&'static str, &mut StandardRuleDataRole); 6] {
        [
            ("domestic_domains", &mut self.domestic_domains),
            ("foreign_domains", &mut self.foreign_domains),
            ("domestic_ips", &mut self.domestic_ips),
            ("direct_domains", &mut self.direct_domains),
            ("remote_domains", &mut self.remote_domains),
            ("ddns_domains", &mut self.ddns_domains),
        ]
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardRuleDataRole {
    #[serde(default)]
    pub sources: Vec<StandardRuleDataSource>,
}

impl StandardRuleDataRole {
    pub(super) fn has_enabled_sources(&self) -> bool {
        self.sources.iter().any(StandardRuleDataSource::enabled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum StandardRuleDataSource {
    Manual {
        id: String,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        #[serde(default)]
        rules: Vec<String>,
    },
    LocalFile {
        id: String,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        path: String,
    },
    Subscription {
        id: String,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        url: String,
        #[serde(default = "default_update_interval_hours")]
        update_interval_hours: u32,
        #[serde(default = "default_rule_data_max_age_hours")]
        max_age_hours: u32,
    },
    NativeDat {
        id: String,
        name: String,
        #[serde(default = "default_true")]
        enabled: bool,
        path: String,
        #[serde(default)]
        selectors: Vec<String>,
    },
}

impl StandardRuleDataSource {
    pub(super) fn id(&self) -> &str {
        match self {
            Self::Manual { id, .. }
            | Self::LocalFile { id, .. }
            | Self::Subscription { id, .. }
            | Self::NativeDat { id, .. } => id,
        }
    }

    pub(super) fn id_mut(&mut self) -> &mut String {
        match self {
            Self::Manual { id, .. }
            | Self::LocalFile { id, .. }
            | Self::Subscription { id, .. }
            | Self::NativeDat { id, .. } => id,
        }
    }

    pub(super) fn name(&self) -> &str {
        match self {
            Self::Manual { name, .. }
            | Self::LocalFile { name, .. }
            | Self::Subscription { name, .. }
            | Self::NativeDat { name, .. } => name,
        }
    }

    pub(super) fn name_mut(&mut self) -> &mut String {
        match self {
            Self::Manual { name, .. }
            | Self::LocalFile { name, .. }
            | Self::Subscription { name, .. }
            | Self::NativeDat { name, .. } => name,
        }
    }

    pub(super) const fn enabled(&self) -> bool {
        match self {
            Self::Manual { enabled, .. }
            | Self::LocalFile { enabled, .. }
            | Self::Subscription { enabled, .. }
            | Self::NativeDat { enabled, .. } => *enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardSmartRoutingSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domestic_path_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_path_id: Option<String>,
    #[serde(default)]
    pub unknown_mode: StandardUnknownMode,
    #[serde(default)]
    pub privacy_fallback_to_domestic: bool,
    #[serde(default = "default_smart_fallback_threshold_ms")]
    pub fallback_threshold_ms: u64,
    #[serde(default)]
    pub response_policy: StandardSmartResponsePolicy,
}

impl Default for StandardSmartRoutingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            domestic_path_id: None,
            remote_path_id: None,
            unknown_mode: StandardUnknownMode::CompatibilityFirst,
            privacy_fallback_to_domestic: false,
            fallback_threshold_ms: default_smart_fallback_threshold_ms(),
            response_policy: StandardSmartResponsePolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardUnknownMode {
    #[default]
    CompatibilityFirst,
    PrivacyFirst,
    StrictRemote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardSmartResponsePolicy {
    #[serde(default = "default_true")]
    pub domestic_ip_mismatch: bool,
    #[serde(default = "default_true")]
    pub cname_only: bool,
    #[serde(default = "default_true")]
    pub nodata: bool,
    #[serde(default = "default_true")]
    pub nxdomain: bool,
    #[serde(default = "default_true")]
    pub servfail: bool,
    #[serde(default = "default_true")]
    pub timeout: bool,
    #[serde(default = "default_true")]
    pub transport_failure: bool,
}

impl Default for StandardSmartResponsePolicy {
    fn default() -> Self {
        Self {
            domestic_ip_mismatch: true,
            cname_only: true,
            nodata: true,
            nxdomain: true,
            servfail: true,
            timeout: true,
            transport_failure: true,
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
pub struct StandardDedicatedGroup {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub strategy: StandardUpstreamStrategy,
    #[serde(default)]
    pub upstreams: Vec<StandardUpstream>,
    #[serde(default)]
    pub path: StandardDedicatedPathPolicy,
    #[serde(default)]
    pub listener: StandardDedicatedListener,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardDedicatedPathPolicy {
    #[serde(default)]
    pub filtering: StandardPolicySwitch,
    #[serde(default)]
    pub cache: StandardPolicySwitch,
    #[serde(default)]
    pub query_log: StandardPolicySwitch,
    #[serde(default)]
    pub dual_stack: StandardDualStackPolicy,
    #[serde(default)]
    pub ip_selection: StandardIpSelectionSettings,
    #[serde(default)]
    pub ecs: StandardEcsPolicy,
}

impl Default for StandardDedicatedPathPolicy {
    fn default() -> Self {
        Self {
            filtering: StandardPolicySwitch::Inherit,
            cache: StandardPolicySwitch::Enabled,
            query_log: StandardPolicySwitch::Inherit,
            dual_stack: StandardDualStackPolicy::Inherit,
            ip_selection: StandardIpSelectionSettings::default(),
            ecs: StandardEcsPolicy::Inherit,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardDedicatedListener {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub address: String,
    #[serde(default = "default_true")]
    pub udp: bool,
    #[serde(default = "default_true")]
    pub tcp: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardDynamicLearningSettings {
    #[serde(default)]
    pub profiles: Vec<StandardDynamicLearningProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardDynamicLearningProfile {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub paused: bool,
    pub target_path_id: String,
    #[serde(default)]
    pub priority: u32,
    #[serde(default = "default_learning_qtypes")]
    pub qtypes: Vec<String>,
    #[serde(default = "default_learning_rcodes")]
    pub rcodes: Vec<String>,
    #[serde(default = "default_true")]
    pub answer_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_ip_role: Option<String>,
    #[serde(default)]
    pub rule_kind: StandardLearningRuleKind,
    #[serde(default = "default_learning_max_entries")]
    pub max_entries: usize,
    #[serde(default = "default_learning_entry_ttl_seconds")]
    pub entry_ttl_seconds: u64,
    #[serde(default = "default_learning_cleanup_interval_seconds")]
    pub cleanup_interval_seconds: u64,
    #[serde(default = "default_learning_queue_size")]
    pub queue_size: usize,
    #[serde(default = "default_learning_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_learning_flush_interval_ms")]
    pub flush_interval_ms: u64,
    #[serde(default)]
    pub failure_policy: StandardLearningFailurePolicy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardLearningRuleKind {
    #[default]
    Full,
    Domain,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardLearningFailurePolicy {
    #[default]
    Continue,
    FailClosed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardAdvancedRule {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: u32,
    #[serde(default)]
    pub phase: StandardAdvancedRulePhase,
    #[serde(default)]
    pub conditions: Vec<StandardAdvancedCondition>,
    pub action: StandardAdvancedAction,
    #[serde(default)]
    pub failure_policy: StandardAdvancedFailurePolicy,
    #[serde(default)]
    pub failure_response: StandardAdvancedFailureResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_origin: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardAdvancedRulePhase {
    #[default]
    Request,
    Response,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum StandardAdvancedCondition {
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
    Qtype {
        values: Vec<String>,
    },
    Time {
        timezone: String,
        periods: Vec<StandardTimePeriod>,
    },
    RateLimitExceeded {
        qps: u32,
        burst: u32,
        mask4: u8,
        mask6: u8,
    },
    SourcePath {
        path_id: String,
    },
    Cname {
        values: Vec<String>,
    },
    Rcode {
        values: Vec<String>,
    },
    HasWantedAnswer,
    ResponseIpRole {
        role: String,
        invert: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardTimePeriod {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(default)]
    pub weekdays: Vec<u8>,
    #[serde(default)]
    pub monthdays: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StandardAdvancedAction {
    UsePath {
        #[serde(rename = "pathId")]
        path_id: String,
    },
    Block {
        response: StandardBlockResponse,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardAdvancedFailurePolicy {
    #[default]
    FailOpen,
    FailClosed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardAdvancedFailureResponse {
    #[default]
    Servfail,
    Refused,
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
    pub rule_data: BTreeMap<String, String>,
    pub rule_data_sources: BTreeMap<String, StandardSubscriptionTagMap>,
    pub smart_routing: BTreeMap<String, String>,
    pub dedicated_groups: BTreeMap<String, StandardDedicatedTagMap>,
    pub dynamic_learning: BTreeMap<String, StandardDynamicLearningTagMap>,
    pub advanced_rules: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardDedicatedTagMap {
    pub provider: String,
    pub matcher: String,
    pub upstream_group: String,
    pub path: String,
    pub entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udp_listener: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_listener: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardDynamicLearningTagMap {
    pub provider: String,
    pub learner: String,
    pub matcher: String,
    pub action: String,
    pub rules_path: String,
    pub metadata_path: String,
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
    pub rule_data_source_count: usize,
    pub smart_routing_enabled: bool,
    pub dedicated_group_count: usize,
    pub dynamic_learning_profile_count: usize,
    pub advanced_rule_count: usize,
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
    #[serde(default)]
    pub managed_files: Vec<String>,
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

const fn default_rule_data_max_age_hours() -> u32 {
    72
}

const fn default_ecs_mask4() -> u8 {
    24
}

const fn default_ecs_mask6() -> u8 {
    48
}

fn default_probe_methods() -> Vec<String> {
    vec!["tcp:443".to_string(), "tcp:80".to_string()]
}

const fn default_probe_stagger_ms() -> u64 {
    200
}

const fn default_probe_timeout_ms() -> u64 {
    600
}

const fn default_probe_max_wait_ms() -> u64 {
    1000
}

const fn default_ip_selection_top_n() -> usize {
    1
}

const fn default_ip_selection_parallel() -> usize {
    256
}

const fn default_ip_selection_cache_size() -> usize {
    4096
}

const fn default_ip_selection_cache_ttl() -> u64 {
    3600
}

const fn default_ip_selection_failure_ttl() -> u64 {
    60
}

const fn default_smart_fallback_threshold_ms() -> u64 {
    500
}

fn default_learning_qtypes() -> Vec<String> {
    vec!["A".to_string(), "AAAA".to_string()]
}

fn default_learning_rcodes() -> Vec<String> {
    vec!["NOERROR".to_string()]
}

const fn default_learning_max_entries() -> usize {
    10_000
}

const fn default_learning_entry_ttl_seconds() -> u64 {
    7 * 24 * 60 * 60
}

const fn default_learning_cleanup_interval_seconds() -> u64 {
    10 * 60
}

const fn default_learning_queue_size() -> usize {
    1024
}

const fn default_learning_batch_size() -> usize {
    256
}

const fn default_learning_flush_interval_ms() -> u64 {
    200
}
