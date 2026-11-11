// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;

use serde::Deserialize;

use super::selection::Strategy;
use crate::config::types::PluginConfig;
use crate::infra::error::{DnsError, Result};

#[derive(Debug, Deserialize)]
pub(super) struct Config {
    #[serde(default)]
    pub strategy: Strategy,
    pub executors: Vec<TargetConfig>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TargetConfig {
    pub exec: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
}

fn default_weight() -> u32 {
    1
}

impl Config {
    pub fn parse(plugin: &PluginConfig) -> Result<Self> {
        let args = plugin.args.clone().ok_or_else(|| {
            DnsError::plugin(format!("load_balance '{}' requires args", plugin.tag))
        })?;
        let config: Self = serde_yaml_ng::from_value(args).map_err(|error| {
            DnsError::plugin(format!(
                "invalid load_balance '{}' config: {error}",
                plugin.tag
            ))
        })?;
        let invalid =
            |message: String| DnsError::plugin(format!("load_balance '{}': {message}", plugin.tag));
        if config.executors.is_empty() {
            return Err(invalid("args.executors must not be empty".into()));
        }
        let mut seen = HashSet::with_capacity(config.executors.len());
        for (index, target) in config.executors.iter().enumerate() {
            if target.exec.is_empty()
                || target.exec.chars().any(char::is_whitespace)
                || target.exec.starts_with('$')
            {
                return Err(invalid(format!(
                    "args.executors[{index}].exec must be a plain non-empty executor tag without '$' or whitespace"
                )));
            }
            if !seen.insert(&target.exec) {
                return Err(invalid(format!(
                    "args.executors[{index}].exec duplicates '{}'",
                    target.exec
                )));
            }
            if target.weight == 0 {
                return Err(invalid(format!(
                    "args.executors[{index}].weight must be a positive u32 integer"
                )));
            }
        }
        Ok(config)
    }
}
