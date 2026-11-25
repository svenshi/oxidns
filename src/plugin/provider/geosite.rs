// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! V2Ray geosite.dat-backed domain provider.

use std::any::Any;
use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use serde::Deserialize;
use tracing::{error, info};

use crate::config::types::PluginConfig;
use crate::core::rule_matcher::{DomainRuleKind, DomainRuleMatcher};
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result as DnsResult};
use crate::infra::task::spawn_isolated_build;
use crate::plugin::provider::Provider;
use crate::plugin::provider::v2ray::{
    DatFileSession, Domain, DomainType, GeoSite, geosite_code, geosite_domain_matches_selectors,
    matched_geosite_selectors, parse_geosite_selectors,
};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;
use crate::proto::{Name, Question};

#[derive(Debug, Clone, Deserialize)]
struct GeoSiteArgs {
    file: String,
    #[serde(default)]
    selectors: Vec<String>,
}

#[derive(Debug, Default)]
struct GeoSiteSnapshot {
    matcher: DomainRuleMatcher,
    /// Files that could not be loaded in the last build (empty = healthy).
    degraded_files: Vec<String>,
}

#[derive(Debug)]
pub struct GeoSiteProvider {
    tag: String,
    args: Arc<GeoSiteArgs>,
    snapshot: ArcSwap<GeoSiteSnapshot>,
}

impl GeoSiteProvider {
    fn build_snapshot(tag: &str, args: &GeoSiteArgs) -> DnsResult<GeoSiteSnapshot> {
        let start_ms = AppClock::elapsed_millis();
        let selectors = parse_geosite_selectors(&args.selectors).map_err(|e| {
            DnsError::plugin(format!(
                "plugin '{}' failed to parse geosite selectors: {}",
                tag, e
            ))
        })?;
        let path = Path::new(&args.file);
        let mut source = DatFileSession::open(path).map_err(DnsError::Io)?;
        let mut matched_entries = 0usize;
        let mut matched_domains = 0usize;
        let mut full = 0usize;
        let mut keyword = 0usize;
        let mut regexp = 0usize;
        source
            .visit_geosite(|entry| {
                visit_selected_geosite_domains(&entry, &selectors, |domain| {
                    match geosite_domain_kind(domain)? {
                        DomainRuleKind::Full => full += 1,
                        DomainRuleKind::Keyword => keyword += 1,
                        DomainRuleKind::Regexp => regexp += 1,
                        DomainRuleKind::Domain => {}
                    }
                    matched_domains += 1;
                    Ok(())
                })
                .map(|matched| matched_entries += usize::from(matched))
            })
            .map_err(|error| geosite_file_error(tag, args, error))?;
        let mut matcher = DomainRuleMatcher::default();
        matcher.reserve_rules(full, keyword, regexp);
        source
            .visit_geosite(|entry| {
                visit_selected_geosite_domains(&entry, &selectors, |domain| {
                    let kind = geosite_domain_kind(domain)?;
                    matcher
                        .add_rule(kind, &domain.value, "")
                        .map_err(|error| format!("geosite code '{}' {error}", geosite_code(&entry)))
                })?;
                Ok(())
            })
            .map_err(|error| geosite_file_error(tag, args, error))?;

        if matched_entries == 0 && !selectors.is_empty() {
            return Err(DnsError::plugin(format!(
                "plugin '{}' found no geosite entries in '{}' for selectors {:?}",
                tag, args.file, args.selectors
            )));
        }

        if matched_domains == 0 && !selectors.is_empty() {
            return Err(DnsError::plugin(format!(
                "plugin '{}' found no geosite rules in '{}' for selectors {:?}",
                tag, args.file, args.selectors
            )));
        }

        matcher.finalize().map_err(DnsError::plugin)?;
        let has_rules = matcher.full_rule_count()
            + matcher.trie_rule_count()
            + matcher.keyword_rule_count()
            + matcher.regexp_rule_count();
        if has_rules == 0 {
            return Err(DnsError::plugin(format!(
                "plugin '{}' produced no domain rules from geosite dat '{}'",
                tag, args.file
            )));
        }

        let elapsed_ms = AppClock::elapsed_millis().saturating_sub(start_ms);
        info!(
            tag = %tag,
            file = %args.file,
            selectors = ?args.selectors,
            matched_entries,
            matched_domains,
            full_rules = matcher.full_rule_count(),
            domain_rules = matcher.trie_rule_count(),
            keyword_rules = matcher.keyword_rule_count(),
            regex_rules = matcher.regexp_rule_count(),
            elapsed_ms,
            "geosite snapshot built"
        );

        Ok(GeoSiteSnapshot {
            matcher,
            degraded_files: Vec::new(),
        })
    }
}

#[async_trait]
impl Plugin for GeoSiteProvider {
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
impl Provider for GeoSiteProvider {
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[hotpath::measure]
    fn contains_name(&self, name: &Name) -> bool {
        self.snapshot.load().matcher.is_match_name(name)
    }

    #[hotpath::measure]
    fn contains_question(&self, question: &Question) -> bool {
        self.contains_name(question.name())
    }

    #[hotpath::measure]
    async fn reload(&self) -> DnsResult<()> {
        let tag = self.tag.clone();
        let args = self.args.clone();
        let build = spawn_isolated_build("geosite snapshot build", move || {
            Self::build_snapshot(&tag, &args)
        })
        .await;
        match build {
            Ok(snapshot) => {
                self.snapshot.store(Arc::new(snapshot));
                Ok(())
            }
            // The dat file is this provider's only rule source. An I/O failure
            // degrades it to an empty snapshot and a later reload can restore
            // the rules. Content/configuration and infrastructure failures stay
            // fatal and leave the published snapshot untouched.
            Err(DnsError::Io(source)) => {
                error!(
                    tag = %self.tag,
                    file = %self.args.file,
                    error = %source,
                    "geosite dat file unavailable, provider degraded (matching disabled)"
                );
                self.snapshot.store(Arc::new(GeoSiteSnapshot {
                    degraded_files: vec![self.args.file.clone()],
                    ..GeoSiteSnapshot::default()
                }));
                Ok(())
            }
            Err(other) => Err(other),
        }
    }

    fn degraded_files(&self) -> Vec<String> {
        self.snapshot.load().degraded_files.clone()
    }

    fn supports_domain_matching(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("geosite")]
pub struct GeoSiteFactory;

impl PluginFactory for GeoSiteFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> DnsResult<UninitializedPlugin> {
        let args = plugin_config
            .args
            .clone()
            .ok_or_else(|| DnsError::plugin("geosite provider requires args"))?;
        let args = serde_yaml_ng::from_value::<GeoSiteArgs>(args)
            .map_err(|e| DnsError::plugin(format!("failed to parse geosite config: {}", e)))?;

        if args.file.trim().is_empty() {
            return Err(DnsError::plugin(format!(
                "plugin '{}' geosite args.file must not be empty",
                plugin_config.tag
            )));
        }

        Ok(UninitializedPlugin::Provider(Box::new(GeoSiteProvider {
            tag: plugin_config.tag.clone(),
            args: Arc::new(args),
            snapshot: ArcSwap::from_pointee(GeoSiteSnapshot::default()),
        })))
    }
}

fn visit_selected_geosite_domains<F>(
    entry: &GeoSite,
    selectors: &[crate::plugin::provider::v2ray::GeoSiteSelector],
    mut on_domain: F,
) -> Result<bool, String>
where
    F: FnMut(&Domain) -> Result<(), String>,
{
    let matched_selectors = matched_geosite_selectors(entry, selectors);
    if !selectors.is_empty() && matched_selectors.is_empty() {
        return Ok(false);
    }
    for domain in &entry.domain {
        if selectors.is_empty() || geosite_domain_matches_selectors(domain, &matched_selectors) {
            on_domain(domain)?;
        }
    }
    Ok(true)
}

fn geosite_domain_kind(domain: &Domain) -> Result<DomainRuleKind, String> {
    match DomainType::try_from(domain.r#type).map_err(|_| {
        format!(
            "unsupported domain type '{}' for '{}'",
            domain.r#type, domain.value
        )
    })? {
        DomainType::Plain => Ok(DomainRuleKind::Keyword),
        DomainType::Regex => Ok(DomainRuleKind::Regexp),
        DomainType::RootDomain => Ok(DomainRuleKind::Domain),
        DomainType::Full => Ok(DomainRuleKind::Full),
    }
}

fn geosite_file_error(tag: &str, args: &GeoSiteArgs, error: String) -> DnsError {
    DnsError::plugin(format!(
        "plugin '{}' failed to stream geosite dat file '{}': {}",
        tag, args.file, error
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::*;
    use crate::infra::clock::AppClock;

    fn test_dat_path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/testdata/rules")
            .join(name)
    }

    fn provider_with_file(file: String, selectors: Vec<String>) -> GeoSiteProvider {
        GeoSiteProvider {
            tag: "geosite-test".to_string(),
            args: Arc::new(GeoSiteArgs { file, selectors }),
            snapshot: ArcSwap::from_pointee(GeoSiteSnapshot::default()),
        }
    }

    #[tokio::test]
    async fn missing_dat_file_degrades_then_reload_restores() {
        AppClock::start();
        let dir = tempdir().unwrap();
        let file = dir.path().join("geosite.dat");
        let provider = provider_with_file(file.display().to_string(), Vec::new());
        let name = Name::from_ascii("265.com.").unwrap();

        provider.reload().await.unwrap();
        assert_eq!(provider.degraded_files(), vec![file.display().to_string()]);
        assert!(!provider.contains_name(&name));

        std::fs::copy(test_dat_path("geosite.dat"), &file).unwrap();
        provider.reload().await.unwrap();
        assert!(provider.degraded_files().is_empty());
        assert!(provider.contains_name(&name));

        // A later reload re-evaluates the file: removing it again degrades
        // the provider and drops its rules.
        std::fs::remove_file(&file).unwrap();
        provider.reload().await.unwrap();
        assert_eq!(provider.degraded_files(), vec![file.display().to_string()]);
        assert!(!provider.contains_name(&name));
    }

    #[tokio::test]
    async fn invalid_selector_fails_reload_and_keeps_last_good_snapshot() {
        AppClock::start();
        let dat = test_dat_path("geosite.dat").display().to_string();
        let mut provider = provider_with_file(dat.clone(), vec!["cn".to_string()]);
        let name = Name::from_ascii("265.com.").unwrap();

        provider.reload().await.unwrap();
        assert!(provider.contains_name(&name));

        provider.args = Arc::new(GeoSiteArgs {
            file: dat,
            selectors: vec!["not-found-code".to_string()],
        });
        let error = provider.reload().await.unwrap_err();
        assert!(error.to_string().contains("found no geosite entries"));
        assert!(provider.degraded_files().is_empty());
        assert!(provider.contains_name(&name));
    }
}
