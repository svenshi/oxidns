// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::Deserialize;

/// Sequential, deletion-only option filtering with optional request
/// restoration.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub(super) struct EdnsFilterConfig {
    pub policy: EdnsOptionsPolicy,
    pub allowlist: Vec<u16>,
    pub denylist: Vec<u16>,
    pub restore_request: bool,
}

impl Default for EdnsFilterConfig {
    fn default() -> Self {
        Self {
            policy: EdnsOptionsPolicy::Preserve,
            allowlist: Vec::new(),
            denylist: Vec::new(),
            restore_request: true,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EdnsOptionsPolicy {
    #[default]
    Preserve,
    Strip,
    StripUnknown,
}
