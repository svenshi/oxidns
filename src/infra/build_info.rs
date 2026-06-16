// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build-time feature and plugin capability reporting.

use serde::Serialize;

use crate::infra::VERSION;
use crate::infra::error::Result;
use crate::plugin::dependency::DependencyKind;
use crate::plugin::registry::try_global_catalog;

#[cfg(feature = "full")]
pub const PRIMARY_BUNDLE: &str = "full";
#[cfg(all(not(feature = "full"), feature = "standard"))]
pub const PRIMARY_BUNDLE: &str = "standard";
#[cfg(all(not(feature = "full"), not(feature = "standard"), feature = "minimal"))]
pub const PRIMARY_BUNDLE: &str = "minimal";
#[cfg(all(
    not(feature = "full"),
    not(feature = "standard"),
    not(feature = "minimal")
))]
pub const PRIMARY_BUNDLE: &str = "custom";

#[cfg(feature = "full")]
pub const CLI_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (full)");
#[cfg(all(not(feature = "full"), feature = "standard"))]
pub const CLI_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (standard)");
#[cfg(all(not(feature = "full"), not(feature = "standard"), feature = "minimal"))]
pub const CLI_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (minimal)");
#[cfg(all(
    not(feature = "full"),
    not(feature = "standard"),
    not(feature = "minimal")
))]
pub const CLI_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (custom)");

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BuildInfo {
    pub version: &'static str,
    pub bundle: &'static str,
    pub enabled_bundles: Vec<&'static str>,
    pub enabled_features: Vec<&'static str>,
    pub supported_plugins: SupportedPlugins,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct SupportedPlugins {
    pub servers: Vec<String>,
    pub executors: Vec<String>,
    pub matchers: Vec<String>,
    pub providers: Vec<String>,
}

pub fn snapshot() -> Result<BuildInfo> {
    Ok(BuildInfo {
        version: VERSION,
        bundle: PRIMARY_BUNDLE,
        enabled_bundles: enabled_bundles(),
        enabled_features: enabled_public_features(),
        supported_plugins: supported_plugins()?,
    })
}

fn supported_plugins() -> Result<SupportedPlugins> {
    let mut plugins = SupportedPlugins::default();
    for (plugin_type, kind) in try_global_catalog()?.plugin_types() {
        match kind {
            DependencyKind::Server => plugins.servers.push(plugin_type.to_string()),
            DependencyKind::Executor => plugins.executors.push(plugin_type.to_string()),
            DependencyKind::Matcher => plugins.matchers.push(plugin_type.to_string()),
            DependencyKind::Provider => plugins.providers.push(plugin_type.to_string()),
            DependencyKind::Any | DependencyKind::Unknown => {}
        }
    }
    Ok(plugins)
}

fn enabled_bundles() -> Vec<&'static str> {
    let mut bundles = Vec::new();
    push_feature(&mut bundles, cfg!(feature = "minimal"), "minimal");
    push_feature(&mut bundles, cfg!(feature = "standard"), "standard");
    push_feature(&mut bundles, cfg!(feature = "full"), "full");
    bundles
}

fn enabled_public_features() -> Vec<&'static str> {
    let mut features = Vec::new();
    push_feature(&mut features, cfg!(feature = "minimal"), "minimal");
    push_feature(&mut features, cfg!(feature = "standard"), "standard");
    push_feature(&mut features, cfg!(feature = "full"), "full");
    push_feature(&mut features, cfg!(feature = "api"), "api");
    push_feature(&mut features, cfg!(feature = "webui"), "webui");
    push_feature(&mut features, cfg!(feature = "metrics"), "metrics");
    push_feature(&mut features, cfg!(feature = "server-dot"), "server-dot");
    push_feature(&mut features, cfg!(feature = "server-doh"), "server-doh");
    push_feature(&mut features, cfg!(feature = "server-doq"), "server-doq");
    push_feature(&mut features, cfg!(feature = "server-doh3"), "server-doh3");
    push_feature(
        &mut features,
        cfg!(feature = "upstream-dot"),
        "upstream-dot",
    );
    push_feature(
        &mut features,
        cfg!(feature = "upstream-doh"),
        "upstream-doh",
    );
    push_feature(
        &mut features,
        cfg!(feature = "upstream-doq"),
        "upstream-doq",
    );
    push_feature(
        &mut features,
        cfg!(feature = "upstream-doh3"),
        "upstream-doh3",
    );
    push_feature(
        &mut features,
        cfg!(feature = "plugin-mikrotik"),
        "plugin-mikrotik",
    );
    push_feature(
        &mut features,
        cfg!(feature = "plugin-query-recorder"),
        "plugin-query-recorder",
    );
    push_feature(
        &mut features,
        cfg!(feature = "plugin-ipset"),
        "plugin-ipset",
    );
    push_feature(&mut features, cfg!(feature = "plugin-cron"), "plugin-cron");
    push_feature(
        &mut features,
        cfg!(feature = "plugin-script"),
        "plugin-script",
    );
    push_feature(
        &mut features,
        cfg!(feature = "plugin-arbitrary"),
        "plugin-arbitrary",
    );
    push_feature(
        &mut features,
        cfg!(feature = "plugin-upgrade"),
        "plugin-upgrade",
    );
    push_feature(
        &mut features,
        cfg!(feature = "plugin-download"),
        "plugin-download",
    );
    push_feature(
        &mut features,
        cfg!(feature = "plugin-http-request"),
        "plugin-http-request",
    );
    push_feature(
        &mut features,
        cfg!(feature = "plugin-reverse-lookup"),
        "plugin-reverse-lookup",
    );
    push_feature(
        &mut features,
        cfg!(feature = "provider-protobuf"),
        "provider-protobuf",
    );
    push_feature(
        &mut features,
        cfg!(feature = "provider-adguard-rule"),
        "provider-adguard-rule",
    );
    push_feature(&mut features, cfg!(feature = "hotpath"), "hotpath");
    push_feature(
        &mut features,
        cfg!(feature = "hotpath-alloc"),
        "hotpath-alloc",
    );
    features
}

fn push_feature(features: &mut Vec<&'static str>, enabled: bool, feature: &'static str) {
    if enabled {
        features.push(feature);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reports_primary_bundle_and_plugins() {
        let info = snapshot().expect("build info should be available");

        assert_eq!(info.version, VERSION);
        assert_eq!(info.bundle, PRIMARY_BUNDLE);
        assert!(
            info.supported_plugins
                .executors
                .contains(&"sequence".into())
        );
        assert!(info.supported_plugins.matchers.contains(&"qname".into()));
    }
}
