// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use serde::Deserialize;

use crate::config::types::PluginConfig;
use crate::core::error::{DnsError, Result as DnsResult};

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

        Ok(Self {
            path: PathBuf::from(path),
            bootstrap_rules: raw.bootstrap_rules,
            queue_size,
            batch_size,
            flush_interval_ms,
        })
    }
}
