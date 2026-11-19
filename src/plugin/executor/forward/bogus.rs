// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Upstream answer filtering for `bogus_answer_ips`.
//!
//! The filter runs before response classification and selection: an answer
//! with any A/AAAA address matching the configured rules is discarded whole,
//! so it never becomes a candidate, vote, or ranking entry.

use std::net::IpAddr;
use std::sync::Arc;

use crate::core::rule_matcher::IpPrefixMatcher;
use crate::infra::error::{DnsError, Result};
use crate::plugin::PluginInitContext;
use crate::plugin::matcher::rules::{
    parse_ip_rules_and_set_tags, parse_rule_list, provider_dependency_specs,
    provider_tags_from_rules,
};
use crate::plugin::provider::Provider;
use crate::proto::Message;

/// Configuration field owned by the `forward` plugin.
pub(super) const BOGUS_ANSWER_IPS_FIELD: &str = "bogus_answer_ips";

/// Compiled matcher for upstream answers that must be discarded.
///
/// Disabled by default: without configuration the filter is a no-op and the
/// hot path pays a single boolean check.
#[derive(Debug, Default)]
pub(super) struct BogusAnswerFilter {
    ip_rules: IpPrefixMatcher,
    ip_set_tags: Vec<String>,
    ip_sets: Vec<Arc<dyn Provider>>,
    enabled: bool,
}

impl BogusAnswerFilter {
    /// Build the filter from the raw `bogus_answer_ips` configuration value.
    ///
    /// Inline rules and `&file` sources are compiled here. `$ip_set` tags are
    /// recorded and resolved in [`init`](Self::init), once the provider
    /// registry is available.
    pub(super) fn from_value(value: Option<&serde_yaml_ng::Value>) -> Result<Self> {
        let Some(value) = value else {
            return Ok(Self::default());
        };

        let rules = parse_rule_list(BOGUS_ANSWER_IPS_FIELD, value.clone())?;
        let (ip_rules, ip_set_tags) = parse_ip_rules_and_set_tags(rules, BOGUS_ANSWER_IPS_FIELD)?;
        if !ip_rules.has_v4_rules() && !ip_rules.has_v6_rules() && ip_set_tags.is_empty() {
            return Err(DnsError::plugin(format!(
                "forward plugin '{}' requires at least one IP rule or ip_set tag",
                BOGUS_ANSWER_IPS_FIELD
            )));
        }

        Ok(Self {
            ip_rules,
            ip_set_tags,
            ip_sets: Vec::new(),
            enabled: true,
        })
    }

    /// Resolve `$ip_set` references and validate their matching capability.
    ///
    /// Must be called from the owning forwarder's `Plugin::init`, after the
    /// dependency graph guarantees the providers exist.
    pub(super) fn init(&mut self, context: &PluginInitContext<'_>, tag: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let mut ip_sets = Vec::with_capacity(self.ip_set_tags.len());
        for (idx, provider_tag) in self.ip_set_tags.iter().enumerate() {
            let field = format!("args.{BOGUS_ANSWER_IPS_FIELD}[{idx}]");
            ip_sets.push(context.provider(&field, provider_tag)?);
        }
        ensure_ip_capable_providers(&ip_sets, &self.ip_set_tags, tag)?;
        self.ip_sets = ip_sets;
        Ok(())
    }

    /// Return the first answer address matching the bogus rules, if any.
    ///
    /// Only the answer section is inspected. The search short-circuits on the
    /// first hit and does not allocate.
    pub(super) fn bogus_answer_ip(&self, response: &Message) -> Option<IpAddr> {
        if !self.enabled {
            return None;
        }
        response
            .answers()
            .iter()
            .filter_map(|record| record.ip_addr())
            .find(|ip| {
                self.ip_rules.contains_ip(*ip)
                    || self.ip_sets.iter().any(|set| set.contains_ip(*ip))
            })
    }

    #[cfg(test)]
    pub(super) fn is_bogus(&self, response: &Message) -> bool {
        self.bogus_answer_ip(response).is_some()
    }
}

/// Validate that every `$ip_set` provider can answer IP membership queries.
fn ensure_ip_capable_providers(
    providers: &[Arc<dyn Provider>],
    tags: &[String],
    tag: &str,
) -> Result<()> {
    for (idx, provider) in providers.iter().enumerate() {
        if !provider.supports_ip_matching() {
            let provider_tag = tags.get(idx).map(String::as_str).unwrap_or("<unknown>");
            return Err(DnsError::plugin(format!(
                "forward plugin '{}' field '{}' requires provider '{}' to support IP matching",
                tag, BOGUS_ANSWER_IPS_FIELD, provider_tag
            )));
        }
    }
    Ok(())
}

/// Extract `$ip_set` tags without opening files, for dependency declaration.
pub(super) fn dependency_specs(
    args: Option<&serde_yaml_ng::Value>,
) -> Vec<crate::plugin::dependency::DependencySpec> {
    let Some(value) = args.and_then(|args| args.get(BOGUS_ANSWER_IPS_FIELD)) else {
        return Vec::new();
    };
    let Ok(rules) = parse_rule_list(BOGUS_ANSWER_IPS_FIELD, value.clone()) else {
        return Vec::new();
    };
    provider_dependency_specs("args.bogus_answer_ips", provider_tags_from_rules(&rules))
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::sync::Arc;

    use super::*;
    use crate::proto::rdata::{A, AAAA};
    use crate::proto::{DNSClass, Message, Name, Question, RData, Record, RecordType};

    #[derive(Debug)]
    struct FakeProvider {
        supports_ip: bool,
        hits: Vec<IpAddr>,
    }

    #[async_trait::async_trait]
    impl crate::plugin::Plugin for FakeProvider {
        fn tag(&self) -> &str {
            "fake_provider"
        }
    }

    #[async_trait::async_trait]
    impl crate::plugin::provider::Provider for FakeProvider {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn contains_ip(&self, ip: IpAddr) -> bool {
            self.hits.contains(&ip)
        }

        fn supports_ip_matching(&self) -> bool {
            self.supports_ip
        }

        async fn reload(&self) -> crate::infra::error::Result<()> {
            Ok(())
        }
    }

    fn filter(raw: &str) -> BogusAnswerFilter {
        let value =
            serde_yaml_ng::from_str::<serde_yaml_ng::Value>(raw).expect("yaml should parse");
        BogusAnswerFilter::from_value(Some(&value)).expect("filter should build")
    }

    fn response_with_ips(ips: &[IpAddr]) -> Message {
        let mut response = Message::new();
        response.add_question(Question::new(
            Name::from_ascii("example.com.").unwrap(),
            RecordType::A,
            DNSClass::IN,
        ));
        for ip in ips {
            let rdata = match ip {
                IpAddr::V4(ip) => RData::A(A(*ip)),
                IpAddr::V6(ip) => RData::AAAA(AAAA(*ip)),
            };
            response.add_answer(Record::from_rdata(
                Name::from_ascii("example.com.").unwrap(),
                60,
                rdata,
            ));
        }
        response
    }

    #[test]
    fn default_filter_is_disabled_and_matches_nothing() {
        let filter = BogusAnswerFilter::default();
        assert!(!filter.enabled);
        assert!(!filter.is_bogus(&response_with_ips(&[IpAddr::from(Ipv4Addr::UNSPECIFIED)])));
    }

    #[test]
    fn string_value_splits_on_commas_and_whitespace() {
        let filter = filter("\"0.0.0.0/8, 127.0.0.0/8  ::1/128\"");
        assert!(filter.enabled);
        assert!(filter.is_bogus(&response_with_ips(&[IpAddr::from(Ipv4Addr::UNSPECIFIED)])));
        assert!(
            filter.is_bogus(&response_with_ips(&[IpAddr::from(Ipv4Addr::new(
                127, 0, 0, 5
            ))]))
        );
        assert!(filter.is_bogus(&response_with_ips(&[IpAddr::from(Ipv6Addr::LOCALHOST)])));
        assert!(
            !filter.is_bogus(&response_with_ips(&[IpAddr::from(Ipv4Addr::new(
                192, 0, 2, 1
            ))]))
        );
    }

    #[test]
    fn list_value_is_supported() {
        let filter = filter("- 0.0.0.0/8\n- ::/128\n");
        assert!(filter.is_bogus(&response_with_ips(&[IpAddr::from(Ipv4Addr::UNSPECIFIED)])));
        assert!(filter.is_bogus(&response_with_ips(&[IpAddr::from(Ipv6Addr::UNSPECIFIED)])));
    }

    #[test]
    fn only_answer_section_is_inspected() {
        let filter = filter("0.0.0.0/8");
        let mut response = Message::new();
        response.add_additional(Record::from_rdata(
            Name::from_ascii("ns.example.com.").unwrap(),
            60,
            RData::A(A(Ipv4Addr::UNSPECIFIED)),
        ));
        assert!(!filter.is_bogus(&response));
    }

    #[test]
    fn any_matching_answer_address_discards_the_response() {
        let filter = filter("0.0.0.0/8");
        assert!(filter.is_bogus(&response_with_ips(&[
            IpAddr::from(Ipv4Addr::new(192, 0, 2, 1)),
            IpAddr::from(Ipv4Addr::UNSPECIFIED),
        ])));
    }

    #[test]
    fn bogus_answer_ip_returns_the_matching_address() {
        let filter = filter("0.0.0.0/8");
        let bogus = IpAddr::from(Ipv4Addr::UNSPECIFIED);
        assert_eq!(
            filter.bogus_answer_ip(&response_with_ips(&[
                IpAddr::from(Ipv4Addr::new(192, 0, 2, 1)),
                bogus,
            ])),
            Some(bogus)
        );
        assert_eq!(
            filter.bogus_answer_ip(&response_with_ips(&[IpAddr::from(Ipv4Addr::new(
                192, 0, 2, 1
            ))])),
            None
        );
    }

    #[test]
    fn rule_file_sources_are_loaded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("bogus.txt");
        std::fs::write(&path, "# comment\n\n0.0.0.0/8\n127.0.0.0/8\n").expect("write rules");

        let raw = format!("\"&{}\"", path.display().to_string().replace('\\', "/"));
        let filter = filter(&raw);
        assert!(filter.is_bogus(&response_with_ips(&[IpAddr::from(Ipv4Addr::UNSPECIFIED)])));
    }

    #[test]
    fn provider_tags_are_recorded_for_later_resolution() {
        let filter = filter("- $reserved_ips\n- 0.0.0.0/8\n");
        assert_eq!(filter.ip_set_tags, vec!["reserved_ips".to_string()]);
        assert!(filter.ip_sets.is_empty());
    }

    #[test]
    fn invalid_rule_reports_field_name() {
        let value = serde_yaml_ng::Value::String("999.0.0.0/8".to_string());
        let err =
            BogusAnswerFilter::from_value(Some(&value)).expect_err("invalid CIDR must be rejected");
        assert!(err.to_string().contains(BOGUS_ANSWER_IPS_FIELD));
    }

    #[test]
    fn empty_rule_list_is_rejected() {
        let value = serde_yaml_ng::Value::String(" , ".to_string());
        let err = BogusAnswerFilter::from_value(Some(&value))
            .expect_err("empty rule list must be rejected");
        assert!(
            err.to_string()
                .contains("requires at least one IP rule or ip_set tag")
        );
    }

    #[test]
    fn non_string_sequence_item_is_rejected() {
        let value = serde_yaml_ng::from_str::<serde_yaml_ng::Value>("- 1").expect("yaml");
        let err =
            BogusAnswerFilter::from_value(Some(&value)).expect_err("numeric rule must be rejected");
        assert!(err.to_string().contains("bogus_answer_ips[0]"));
    }

    #[test]
    fn dependency_specs_skip_files_and_extract_tags() {
        let args = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(
            "bogus_answer_ips:\n  - $reserved_ips\n  - \"&/missing/rules.txt\"\n  - 0.0.0.0/8\n",
        )
        .expect("yaml");
        let specs = dependency_specs(Some(&args));
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].field, "args.bogus_answer_ips[0]");
        assert_eq!(specs[0].target_tag, "reserved_ips");
    }

    #[test]
    fn provider_hits_discard_the_response() {
        let mut filter = filter("- $reserved_ips\n- 0.0.0.0/8\n");
        filter.ip_sets = vec![Arc::new(FakeProvider {
            supports_ip: true,
            hits: vec![IpAddr::from(Ipv4Addr::new(192, 0, 2, 9))],
        })];

        assert!(
            filter.is_bogus(&response_with_ips(&[IpAddr::from(Ipv4Addr::new(
                192, 0, 2, 9
            ))]))
        );
        assert!(
            !filter.is_bogus(&response_with_ips(&[IpAddr::from(Ipv4Addr::new(
                192, 0, 2, 1
            ))]))
        );
    }

    #[test]
    fn non_ip_provider_is_rejected() {
        let providers = vec![Arc::new(FakeProvider {
            supports_ip: false,
            hits: Vec::new(),
        }) as Arc<dyn crate::plugin::provider::Provider>];
        let tags = vec!["plain_set".to_string()];

        let err = ensure_ip_capable_providers(&providers, &tags, "forward-test")
            .expect_err("non-IP provider must be rejected");
        assert!(
            err.to_string()
                .contains("requires provider 'plain_set' to support IP matching")
        );
    }

    #[test]
    fn dependency_specs_are_empty_without_field() {
        let args = serde_yaml_ng::from_str::<serde_yaml_ng::Value>("upstreams: []").expect("yaml");
        assert!(dependency_specs(Some(&args)).is_empty());
        assert!(dependency_specs(None).is_empty());
    }
}
