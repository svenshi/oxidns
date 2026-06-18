// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! V2Ray geoip.dat-backed IP provider.

use std::any::Any;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, info};

use crate::config::types::PluginConfig;
use crate::core::rule_matcher::IpPrefixMatcher;
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result as DnsResult};
use crate::infra::task::spawn_isolated_build;
use crate::plugin::provider::Provider;
use crate::plugin::provider::v2ray::{
    Cidr, DatFileSession, GeoIp, geoip_code, normalized_selectors,
};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;

#[derive(Debug, Clone, Deserialize)]
struct GeoIpArgs {
    file: String,
    #[serde(default)]
    selectors: Vec<String>,
}

#[derive(Debug, Default)]
struct GeoIpSnapshot {
    matcher: IpPrefixMatcher,
    has_v4_rules: bool,
    has_v6_rules: bool,
}

#[derive(Debug)]
pub struct GeoIpProvider {
    tag: String,
    args: Arc<GeoIpArgs>,
    snapshot: ArcSwap<GeoIpSnapshot>,
}

impl GeoIpProvider {
    fn build_snapshot(tag: &str, args: &GeoIpArgs) -> DnsResult<GeoIpSnapshot> {
        let start_ms = AppClock::elapsed_millis();
        let requested_selectors = normalized_selectors(&args.selectors);
        let path = Path::new(&args.file);
        let mut source =
            DatFileSession::open(path).map_err(|error| geoip_file_error(tag, args, error))?;
        let mut matched_entries = 0usize;
        let mut v4_capacity = 0usize;
        let mut v6_capacity = 0usize;
        source
            .visit_geoip(|entry| {
                if !matches_selector(&entry, &requested_selectors) {
                    return Ok(());
                }
                matched_entries += 1;
                for cidr in &entry.cidr {
                    match cidr.ip.len() {
                        4 => v4_capacity += 1,
                        16 => v6_capacity += 1,
                        len => {
                            return Err(format!(
                                "invalid CIDR byte length {len} in geoip code '{}'",
                                geoip_code(&entry)
                            ));
                        }
                    }
                }
                Ok(())
            })
            .map_err(|error| geoip_file_error(tag, args, error))?;
        let mut matcher = IpPrefixMatcher::default();
        matcher.reserve_rules(v4_capacity, v6_capacity);
        source
            .visit_geoip(|entry| {
                if matches_selector(&entry, &requested_selectors) {
                    let code = geoip_code(&entry);
                    for cidr in &entry.cidr {
                        add_geoip_cidr(&mut matcher, cidr, tag, code)
                            .map_err(|error| error.to_string())?;
                    }
                }
                Ok(())
            })
            .map_err(|error| geoip_file_error(tag, args, error))?;

        if matched_entries == 0 && !requested_selectors.is_empty() {
            return Err(DnsError::plugin(format!(
                "plugin '{}' found no geoip entries in '{}' for selectors {:?}",
                tag, args.file, args.selectors
            )));
        }

        matcher.finalize_compact();
        if matcher.v4_rule_count() == 0 && matcher.v6_rule_count() == 0 {
            return Err(DnsError::plugin(format!(
                "plugin '{}' produced no IP rules from geoip dat '{}'",
                tag, args.file
            )));
        }

        let has_v4_rules = matcher.has_v4_rules();
        let has_v6_rules = matcher.has_v6_rules();
        let elapsed_ms = AppClock::elapsed_millis().saturating_sub(start_ms);
        info!(
            tag = %tag,
            file = %args.file,
            selectors = ?args.selectors,
            matched_entries,
            v4_rules = matcher.v4_rule_count(),
            v6_rules = matcher.v6_rule_count(),
            elapsed_ms,
            "geoip snapshot built"
        );
        debug!(tag = %tag, has_v4_rules, has_v6_rules, "geoip matcher compiled");

        Ok(GeoIpSnapshot {
            matcher,
            has_v4_rules,
            has_v6_rules,
        })
    }
}

#[async_trait]
impl Plugin for GeoIpProvider {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> DnsResult<()> {
        self.reload().await
    }

    async fn destroy(&self) -> DnsResult<()> {
        Ok(())
    }
}

#[async_trait]
impl Provider for GeoIpProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[hotpath::measure]
    fn contains_ip(&self, ip: IpAddr) -> bool {
        let snapshot = self.snapshot.load();
        let has_family_rules = match ip {
            IpAddr::V4(_) => snapshot.has_v4_rules,
            IpAddr::V6(_) => snapshot.has_v6_rules,
        };
        if !has_family_rules {
            return false;
        }
        snapshot.matcher.contains_ip(ip)
    }

    #[hotpath::measure]
    async fn reload(&self) -> DnsResult<()> {
        let tag = self.tag.clone();
        let args = self.args.clone();
        let snapshot = spawn_isolated_build("geoip snapshot build", move || {
            Self::build_snapshot(&tag, &args)
        })
        .await?;
        self.snapshot.store(Arc::new(snapshot));
        Ok(())
    }

    fn supports_reload(&self) -> bool {
        true
    }

    fn supports_ip_matching(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("geoip")]
pub struct GeoIpFactory;

impl PluginFactory for GeoIpFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> DnsResult<UninitializedPlugin> {
        let args = plugin_config
            .args
            .clone()
            .ok_or_else(|| DnsError::plugin("geoip provider requires args"))?;
        let args = serde_yaml_ng::from_value::<GeoIpArgs>(args)
            .map_err(|e| DnsError::plugin(format!("failed to parse geoip config: {}", e)))?;

        if args.file.trim().is_empty() {
            return Err(DnsError::plugin(format!(
                "plugin '{}' geoip args.file must not be empty",
                plugin_config.tag
            )));
        }

        Ok(UninitializedPlugin::Provider(Box::new(GeoIpProvider {
            tag: plugin_config.tag.clone(),
            args: Arc::new(args),
            snapshot: ArcSwap::from_pointee(GeoIpSnapshot::default()),
        })))
    }
}

/// Feed one geoip CIDR entry into the matcher straight from its decoded bytes,
/// skipping the textual `format!` + re-parse round trip used for file rules.
fn add_geoip_cidr(
    matcher: &mut IpPrefixMatcher,
    cidr: &Cidr,
    tag: &str,
    code: &str,
) -> DnsResult<()> {
    let prefix = u8::try_from(cidr.prefix).map_err(|_| {
        DnsError::plugin(format!(
            "plugin '{}' invalid CIDR prefix {} in geoip code '{}'",
            tag, cidr.prefix, code
        ))
    })?;

    let result = match cidr.ip.len() {
        4 => {
            let mut octets = [0u8; 4];
            octets.copy_from_slice(&cidr.ip);
            matcher.add_v4_network(u32::from_be_bytes(octets), prefix)
        }
        16 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&cidr.ip);
            matcher.add_v6_network(u128::from_be_bytes(octets), prefix)
        }
        _ => {
            return Err(DnsError::plugin(format!(
                "plugin '{}' invalid CIDR bytes in geoip code '{}'",
                tag, code
            )));
        }
    };

    result.map_err(|e| {
        DnsError::plugin(format!(
            "plugin '{}' invalid CIDR in geoip code '{}': {}",
            tag, code, e
        ))
    })
}

fn matches_selector(entry: &GeoIp, requested_selectors: &[String]) -> bool {
    if requested_selectors.is_empty() {
        return true;
    }
    let code = geoip_code(entry).to_ascii_lowercase();
    requested_selectors.iter().any(|wanted| wanted == &code)
}

fn geoip_file_error(tag: &str, args: &GeoIpArgs, error: String) -> DnsError {
    DnsError::plugin(format!(
        "plugin '{}' failed to stream geoip dat file '{}': {}",
        tag, args.file, error
    ))
}
