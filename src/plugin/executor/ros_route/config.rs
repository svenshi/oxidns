// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! RouterOS route plugin configuration.

use std::time::Duration;

use ahash::AHashSet;
use serde::Deserialize;
use serde_yaml_ng::Value;
use tracing::warn;

use super::api::{
    DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_RECEIVE_TIMEOUT_SECS, DEFAULT_SEND_TIMEOUT_SECS,
    MikrotikApiTimeouts,
};
use super::persistent::parse_persistent_ips;
use crate::infra::error::{DnsError, Result};
use crate::infra::system::deserialize_duration_option;
use crate::plugin::executor::routeros::transport::{RouterOsConnectionConfig, RouterOsTlsArgs};

pub(super) const DEFAULT_MIN_TTL: u32 = 60;
pub(super) const DEFAULT_MAX_TTL: u32 = 3600;
pub(super) const DEFAULT_ASYNC_MODE: bool = true;
pub(super) const DEFAULT_CLEANUP_ON_SHUTDOWN: bool = true;
pub(super) const DEFAULT_CONNTRACK_GUARD: bool = false;
pub(super) const DEFAULT_ROUTE_DISTANCE: u8 = 100;
pub(super) const DEFAULT_COMMENT_PREFIX: &str = "oxi";
pub(super) const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(8);
pub(super) const DEFAULT_QUEUE_CAPACITY: usize = 16_384;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct MikrotikConfigArgs {
    /// RouterOS API endpoint, usually `<host>:8728`.
    address: Option<String>,
    /// RouterOS login username.
    username: Option<String>,
    /// RouterOS login password.
    password: Option<String>,
    /// RouterOS API connection timeout in seconds.
    connect_timeout: Option<u64>,
    /// RouterOS API command send timeout in seconds.
    send_timeout: Option<u64>,
    /// RouterOS API response receive timeout in seconds.
    receive_timeout: Option<u64>,
    /// Optional RouterOS API-SSL configuration. Presence enables TLS.
    tls: Option<RouterOsTlsArgs>,
    /// Whether post stage waits RouterOS writes (`false`) or queues work
    /// (`true`).
    #[serde(rename = "async")]
    async_mode: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_duration_option")]
    wait_timeout: Option<Duration>,
    queue_capacity: Option<usize>,
    /// Dedicated RouterOS routing table for managed routes.
    routing_table: Option<String>,
    /// IPv4 gateway value for managed IPv4 routes.
    gateway4: Option<String>,
    /// IPv6 gateway value for managed IPv6 routes.
    gateway6: Option<String>,
    /// Prefix used in RouterOS route comments to mark OxiDNS-managed routes.
    /// Defaults to `oxi` when omitted.
    comment_prefix: Option<String>,
    /// Route distance written to RouterOS for managed routes.
    distance: Option<u8>,
    /// Optional route scopes; omitted properties remain managed by RouterOS.
    scope: Option<u8>,
    target_scope: Option<u8>,
    /// Always-present routes that should not expire with DNS TTL.
    persistent: Option<PersistentArgs>,
    /// Minimum effective TTL clamp (seconds) for observed records.
    min_ttl: Option<u32>,
    /// Maximum effective TTL clamp (seconds) for observed records.
    max_ttl: Option<u32>,
    /// Optional fixed TTL override (seconds) for dynamic observed records.
    /// `0` keeps a dynamic route until explicit cleanup or operator removal.
    fixed_ttl: Option<u32>,
    /// Whether to clean managed dynamic routes on shutdown.
    cleanup_on_shutdown: Option<bool>,
    /// Delay normal route removal while RouterOS connection tracking has a
    /// connection for the route destination.
    conntrack_guard: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct PersistentArgs {
    /// Inline always-present IPs/CIDRs. Plain IP is normalized to host route.
    pub(super) ips: Option<Vec<String>>,
    /// File list that provides always-present IPs.
    pub(super) files: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub(super) struct MikrotikConfig {
    /// Connection settings consumed when the API transport is constructed.
    pub(super) connection: Option<RouterOsConnectionConfig>,
    /// Async mode switch for post stage RouterOS writes.
    pub(super) async_mode: bool,
    pub(super) wait_timeout: Duration,
    pub(super) queue_capacity: usize,
    /// Dedicated RouterOS routing table for this plugin.
    pub(super) routing_table: String,
    /// Optional IPv4 gateway.
    pub(super) gateway4: Option<String>,
    /// Optional IPv6 gateway.
    pub(super) gateway6: Option<String>,
    /// Always-present routes in normalized CIDR format (`ip/prefix`).
    pub(super) persistent_ips: AHashSet<String>,
    /// Managed route comment prefix.
    pub(super) comment_prefix: String,
    /// Route distance written to RouterOS.
    pub(super) distance: u8,
    pub(super) scope: Option<u8>,
    pub(super) target_scope: Option<u8>,
    /// Minimum effective TTL clamp in seconds.
    pub(super) min_ttl: u32,
    /// Maximum effective TTL clamp in seconds.
    pub(super) max_ttl: u32,
    /// Optional fixed TTL override in seconds. `0` never expires by time.
    pub(super) fixed_ttl: Option<u32>,
    /// Shutdown cleanup behavior for dynamic routes.
    pub(super) cleanup_on_shutdown: bool,
    /// Delay normal route removal while a matching RouterOS connection exists.
    pub(super) conntrack_guard: bool,
}

impl MikrotikConfigArgs {
    fn into_config(self, emit_warnings: bool) -> Result<MikrotikConfig> {
        let address = required_non_empty(self.address, "address")?;
        let username = required_non_empty(self.username, "username")?;
        let password = required_non_empty(self.password, "password")?;
        let api_timeouts = MikrotikApiTimeouts::from_secs(
            timeout_secs(
                self.connect_timeout,
                "connect_timeout",
                DEFAULT_CONNECT_TIMEOUT_SECS,
            )?,
            timeout_secs(self.send_timeout, "send_timeout", DEFAULT_SEND_TIMEOUT_SECS)?,
            timeout_secs(
                self.receive_timeout,
                "receive_timeout",
                DEFAULT_RECEIVE_TIMEOUT_SECS,
            )?,
        );
        let connection = RouterOsConnectionConfig::new(
            address.clone(),
            username,
            password,
            api_timeouts,
            self.tls,
        )?;
        let routing_table = required_non_empty(self.routing_table, "routing_table")?;
        let comment_prefix = optional_non_empty(self.comment_prefix)
            .unwrap_or_else(|| DEFAULT_COMMENT_PREFIX.to_string());
        validate_comment_token("comment_prefix", &comment_prefix)?;
        let distance = self.distance.unwrap_or(DEFAULT_ROUTE_DISTANCE);
        if self.scope == Some(0) {
            return Err(DnsError::plugin(
                "ros_route 'scope' must be between 1 and 255",
            ));
        }

        let gateway4 = optional_non_empty(self.gateway4);
        let gateway6 = optional_non_empty(self.gateway6);
        if gateway4.is_none() && gateway6.is_none() {
            return Err(DnsError::plugin(
                "ros_route requires at least one of gateway4 or gateway6",
            ));
        }

        let min_ttl = self.min_ttl.unwrap_or(DEFAULT_MIN_TTL);
        let max_ttl = self.max_ttl.unwrap_or(DEFAULT_MAX_TTL);
        if min_ttl > max_ttl {
            return Err(DnsError::plugin(format!(
                "ros_route ttl range is invalid: min_ttl({min_ttl}) > max_ttl({max_ttl})"
            )));
        }
        // `0` deliberately means a dynamic route that never expires by time.
        let fixed_ttl = self.fixed_ttl;
        let wait_timeout = positive_duration(
            self.wait_timeout.unwrap_or(DEFAULT_WAIT_TIMEOUT),
            "wait_timeout",
        )?;
        let queue_capacity = positive_usize(
            self.queue_capacity.unwrap_or(DEFAULT_QUEUE_CAPACITY),
            "queue_capacity",
        )?;
        let parsed_persistent =
            parse_persistent_ips(self.persistent, gateway4.is_some(), gateway6.is_some())?;
        let ignored_by_gateway = parsed_persistent.ignored_by_gateway;
        if emit_warnings && ignored_by_gateway > 0 {
            warn!(
                ignored = ignored_by_gateway,
                "ros_route persistent ignored entries without corresponding gateway family"
            );
        }
        let ignored_default_route = parsed_persistent.ignored_default_route;
        if emit_warnings && ignored_default_route > 0 {
            warn!(
                ignored = ignored_default_route,
                "ros_route persistent ignored default-route entries (/0)"
            );
        }

        Ok(MikrotikConfig {
            connection: Some(connection),
            async_mode: self.async_mode.unwrap_or(DEFAULT_ASYNC_MODE),
            wait_timeout,
            queue_capacity,
            routing_table,
            gateway4,
            gateway6,
            persistent_ips: parsed_persistent.all_ips,
            comment_prefix,
            distance,
            scope: self.scope,
            target_scope: self.target_scope,
            min_ttl,
            max_ttl,
            fixed_ttl,
            cleanup_on_shutdown: self
                .cleanup_on_shutdown
                .unwrap_or(DEFAULT_CLEANUP_ON_SHUTDOWN),
            conntrack_guard: self.conntrack_guard.unwrap_or(DEFAULT_CONNTRACK_GUARD),
        })
    }
}

pub(super) fn parse_plugin_config(
    args: Option<Value>,
    emit_warnings: bool,
) -> Result<MikrotikConfig> {
    let Some(args) = args else {
        return Err(DnsError::plugin("ros_route plugin requires args"));
    };
    let raw = serde_yaml_ng::from_value::<MikrotikConfigArgs>(args)
        .map_err(|e| DnsError::plugin(format!("failed to parse ros_route config: {e}")))?;
    raw.into_config(emit_warnings)
}

/// Require non-empty string config fields and keep trimmed value.
fn required_non_empty(value: Option<String>, field: &str) -> Result<String> {
    let Some(value) = value else {
        return Err(DnsError::plugin(format!("ros_route '{field}' is required")));
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(DnsError::plugin(format!(
            "ros_route '{field}' cannot be empty"
        )));
    }
    Ok(trimmed.to_string())
}

/// Convert optional string to trimmed non-empty value.
fn optional_non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[inline]
fn contains_comment_delimiter(value: &str) -> bool {
    value.contains(';') || value.contains('=')
}

pub(super) fn validate_comment_token(field: &str, value: &str) -> Result<()> {
    if contains_comment_delimiter(value) {
        return Err(DnsError::plugin(format!(
            "ros_route '{field}' cannot contain ';' or '='"
        )));
    }
    Ok(())
}

fn timeout_secs(value: Option<u64>, field: &str, default_secs: u64) -> Result<u64> {
    match value {
        Some(0) => Err(DnsError::plugin(format!(
            "ros_route '{field}' must be greater than 0 seconds"
        ))),
        Some(value) => Ok(value),
        None => Ok(default_secs),
    }
}

fn positive_duration(value: Duration, field: &str) -> Result<Duration> {
    if value.is_zero() {
        return Err(DnsError::plugin(format!(
            "ros_route '{field}' must be greater than 0"
        )));
    }
    Ok(value)
}

fn positive_usize(value: usize, field: &str) -> Result<usize> {
    if value == 0 {
        return Err(DnsError::plugin(format!(
            "ros_route '{field}' must be greater than 0"
        )));
    }
    Ok(value)
}
