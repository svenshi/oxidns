// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use serde::Deserialize;

use crate::config::types::PluginConfig;
use crate::infra::error::{DnsError, Result as DnsResult};

const DEFAULT_QUEUE_SIZE: usize = 1024;
const DEFAULT_BATCH_SIZE: usize = 256;
const DEFAULT_FLUSH_INTERVAL_MS: u64 = 200;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DynamicDomainSetArgs {
    /// Machine-managed text file owned by this provider instance.
    path: String,
    /// Initial rules written only when `path` does not exist yet.
    #[serde(default)]
    bootstrap_rules: Vec<String>,
    /// Bounded work queue from request/API code to the single writer worker.
    queue_size: Option<usize>,
    /// Number of queued append rules that triggers an immediate flush.
    batch_size: Option<usize>,
    /// Maximum time append rules may remain in memory before being flushed.
    flush_interval_ms: Option<u64>,
    /// Optional hard bound for the number of rules owned by this provider.
    max_entries: Option<usize>,
    /// Optional age limit for automatically learned entries.
    entry_ttl_seconds: Option<u64>,
    /// Bounded interval for removing expired learned entries.
    cleanup_interval_seconds: Option<u64>,
    /// Sidecar JSON storing learned/manual provenance and timestamps.
    metadata_path: Option<String>,
}

/// Validated runtime configuration.
///
/// Keeping this separate from the deserialized args lets the hot and worker
/// paths use normalized types (`PathBuf`, concrete defaults) without repeatedly
/// handling optional values.
#[derive(Debug, Clone)]
pub(super) struct DynamicDomainSetConfig {
    pub(super) path: PathBuf,
    pub(super) bootstrap_rules: Vec<String>,
    pub(super) queue_size: usize,
    pub(super) batch_size: usize,
    pub(super) flush_interval_ms: u64,
    pub(super) max_entries: Option<usize>,
    pub(super) entry_ttl_seconds: Option<u64>,
    pub(super) cleanup_interval_seconds: u64,
    pub(super) metadata_path: Option<PathBuf>,
}

impl DynamicDomainSetConfig {
    pub(super) fn from_plugin_config(plugin_config: &PluginConfig) -> DnsResult<Self> {
        let args = plugin_config
            .args
            .clone()
            .ok_or_else(|| DnsError::plugin("dynamic_domain_set requires structured args"))?;
        let raw = serde_yaml_ng::from_value::<DynamicDomainSetArgs>(args).map_err(|err| {
            DnsError::plugin(format!(
                "failed to parse dynamic_domain_set config: {}",
                err
            ))
        })?;

        // Validate bounded queue parameters at creation time. The worker relies
        // on non-zero values and should not carry defensive branches in its
        // select loop for invalid configuration.
        let path = raw.path.trim();
        if path.is_empty() {
            return Err(DnsError::plugin("dynamic_domain_set path cannot be empty"));
        }
        let queue_size = raw.queue_size.unwrap_or(DEFAULT_QUEUE_SIZE);
        let batch_size = raw.batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
        let flush_interval_ms = raw.flush_interval_ms.unwrap_or(DEFAULT_FLUSH_INTERVAL_MS);
        if queue_size == 0 {
            return Err(DnsError::plugin(
                "dynamic_domain_set queue_size must be greater than 0",
            ));
        }
        if batch_size == 0 {
            return Err(DnsError::plugin(
                "dynamic_domain_set batch_size must be greater than 0",
            ));
        }
        if flush_interval_ms == 0 {
            return Err(DnsError::plugin(
                "dynamic_domain_set flush_interval_ms must be greater than 0",
            ));
        }
        if matches!(raw.max_entries, Some(0)) {
            return Err(DnsError::plugin(
                "dynamic_domain_set max_entries must be greater than 0",
            ));
        }
        if matches!(raw.entry_ttl_seconds, Some(0)) {
            return Err(DnsError::plugin(
                "dynamic_domain_set entry_ttl_seconds must be greater than 0",
            ));
        }
        let cleanup_interval_seconds = raw.cleanup_interval_seconds.unwrap_or(600);
        if cleanup_interval_seconds == 0 {
            return Err(DnsError::plugin(
                "dynamic_domain_set cleanup_interval_seconds must be greater than 0",
            ));
        }
        let metadata_path = raw
            .metadata_path
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
            .map(PathBuf::from);
        if raw.entry_ttl_seconds.is_some() && metadata_path.is_none() {
            return Err(DnsError::plugin(
                "dynamic_domain_set entry_ttl_seconds requires metadata_path",
            ));
        }

        Ok(Self {
            path: PathBuf::from(path),
            bootstrap_rules: raw.bootstrap_rules,
            queue_size,
            batch_size,
            flush_interval_ms,
            max_entries: raw.max_entries,
            entry_ttl_seconds: raw.entry_ttl_seconds,
            cleanup_interval_seconds,
            metadata_path,
        })
    }
}
