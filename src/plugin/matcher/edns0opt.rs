// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `edns0opt` matcher plugin.
//!
//! Matches by the presence of EDNS0 option codes in the request or response
//! OPT record.
//!
//! Configuration (all fields live under `args`):
//! - `codes`: required, non-empty list of EDNS0 option codes. Both integers and
//!   decimal strings are accepted and may be mixed in one list.
//! - `require`: `any` (default) or `all`.
//! - `source`: `request` (default) or `response`.
//!
//! Quick setup only accepts codes (`edns0opt 8,12`) and always means "any code
//! present in the request". Use a named matcher plus a `$tag` reference when
//! `require: all` or `source: response` is needed.
//!
//! The matcher is read-only and does not clone messages or allocate per query.
//! Negating this matcher expresses "no OPT / no matching option".

use std::fmt::Debug;

use ahash::AHashSet;
use async_trait::async_trait;
use serde::Deserialize;
use serde_yaml_ng::Value;

use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result as DnsResult};
use crate::plugin::matcher::Matcher;
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RequireMode {
    /// Any configured code is present.
    #[default]
    Any,
    /// All configured codes are present.
    All,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum MatchSource {
    /// Inspect the request OPT record.
    #[default]
    Request,
    /// Inspect the response OPT record.
    Response,
}

#[derive(Debug, Clone, Deserialize)]
struct Edns0OptConfig {
    /// EDNS0 option codes that must be present. Raw YAML values are kept here
    /// so integers and decimal strings can be parsed uniformly below.
    codes: Vec<Value>,
    /// Whether one or all codes must be present; defaults to `any`.
    #[serde(default)]
    require: RequireMode,
    /// Which side OPT to inspect; defaults to `request`.
    #[serde(default)]
    source: MatchSource,
}

#[derive(Debug, Clone)]
#[plugin_factory("edns0opt")]
pub struct Edns0OptFactory {}

impl PluginFactory for Edns0OptFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> DnsResult<UninitializedPlugin> {
        let (codes, require, source) = parse_config(plugin_config.args.clone())?;
        build_matcher(plugin_config.tag.clone(), codes, require, source)
    }

    fn quick_setup(&self, tag: &str, param: Option<String>) -> DnsResult<UninitializedPlugin> {
        let (codes, require, source) = parse_quick_setup(param)?;
        build_matcher(tag.to_string(), codes, require, source)
    }
}

fn build_matcher(
    tag: String,
    codes: Vec<u16>,
    require: RequireMode,
    source: MatchSource,
) -> DnsResult<UninitializedPlugin> {
    Ok(UninitializedPlugin::Matcher(Box::new(Edns0OptMatcher {
        tag,
        codes: codes.into_iter().collect(),
        require,
        source,
    })))
}

fn ensure_codes_present(codes: &[u16]) -> DnsResult<()> {
    if codes.is_empty() {
        return Err(DnsError::plugin(
            "edns0opt matcher requires at least one code",
        ));
    }
    Ok(())
}

/// Parse one EDNS0 option code from either a YAML integer or a decimal string.
/// Both forms are validated against `u16`.
fn parse_code(value: &Value) -> DnsResult<u16> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|raw| u16::try_from(raw).ok())
            .ok_or_else(|| {
                DnsError::plugin(format!("EDNS0 option code out of u16 range: {}", number))
            }),
        Value::String(raw) => raw
            .parse::<u16>()
            .map_err(|e| DnsError::plugin(format!("invalid EDNS0 option code '{}': {}", raw, e))),
        other => Err(DnsError::plugin(format!(
            "invalid EDNS0 option code: {:?}",
            other
        ))),
    }
}

fn parse_codes(values: Vec<Value>) -> DnsResult<Vec<u16>> {
    values.iter().map(parse_code).collect()
}

fn parse_config(args: Option<Value>) -> DnsResult<(Vec<u16>, RequireMode, MatchSource)> {
    let Some(args) = args else {
        return Err(DnsError::plugin(
            "edns0opt matcher requires a config mapping with codes",
        ));
    };

    let cfg: Edns0OptConfig = serde_yaml_ng::from_value(args)
        .map_err(|e| DnsError::plugin(format!("failed to parse edns0opt config: {}", e)))?;
    let codes = parse_codes(cfg.codes)?;
    ensure_codes_present(&codes)?;

    Ok((codes, cfg.require, cfg.source))
}

fn parse_quick_setup(param: Option<String>) -> DnsResult<(Vec<u16>, RequireMode, MatchSource)> {
    let Some(raw) = param else {
        return Err(DnsError::plugin(
            "edns0opt matcher requires at least one code",
        ));
    };

    let mut codes = Vec::new();

    for token in raw.split(|c: char| c == ',' || c.is_ascii_whitespace()) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        if matches!(
            token.to_ascii_lowercase().as_str(),
            "any" | "all" | "resp" | "request" | "response"
        ) {
            return Err(DnsError::plugin(format!(
                "edns0opt quick setup only accepts codes; use a named matcher with a $tag reference for require: all or source: response (got '{}')",
                token
            )));
        }

        let code = token.parse::<u16>().map_err(|e| {
            DnsError::plugin(format!("invalid EDNS0 option code '{}': {}", token, e))
        })?;
        codes.push(code);
    }

    ensure_codes_present(&codes)?;
    Ok((codes, RequireMode::Any, MatchSource::Request))
}

#[derive(Debug)]
struct Edns0OptMatcher {
    tag: String,
    codes: AHashSet<u16>,
    require: RequireMode,
    source: MatchSource,
}

#[async_trait]
impl Plugin for Edns0OptMatcher {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> DnsResult<()> {
        Ok(())
    }

    async fn destroy(&self) -> DnsResult<()> {
        Ok(())
    }
}

impl Matcher for Edns0OptMatcher {
    #[hotpath::measure]
    fn is_match(&self, context: &mut DnsContext) -> bool {
        let message = match self.source {
            MatchSource::Request => &context.request,
            MatchSource::Response => {
                let Some(response) = context.response() else {
                    return false;
                };
                response
            }
        };

        let Some(edns) = message.edns() else {
            return false;
        };

        let options = edns.options();
        match self.require {
            RequireMode::Any => options
                .iter()
                .any(|option| self.codes.contains(&option.code())),
            RequireMode::All => self
                .codes
                .iter()
                .all(|code| options.iter().any(|option| option.code() == *code)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::*;
    use crate::core::context::DnsContext;
    use crate::proto::{ClientSubnet, DNSClass, EdnsOption, Message, Name, Question, RecordType};

    fn make_request() -> Message {
        let mut request = Message::new();
        request.add_question(Question::new(
            Name::from_ascii("example.com.").unwrap(),
            RecordType::A,
            DNSClass::IN,
        ));
        request
    }

    fn subnet_option() -> EdnsOption {
        EdnsOption::Subnet(ClientSubnet::new(
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            24,
            0,
        ))
    }

    fn padding_option() -> EdnsOption {
        EdnsOption::Padding(crate::proto::EdnsPadding::new(vec![0; 4]))
    }

    fn make_context(options: &[EdnsOption]) -> DnsContext {
        let mut request = make_request();
        if !options.is_empty() {
            let edns = request.ensure_edns_mut();
            for option in options {
                edns.insert(option.clone());
            }
        }
        DnsContext::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)), request)
    }

    fn make_matcher(codes: &[u16], require: RequireMode, source: MatchSource) -> Edns0OptMatcher {
        Edns0OptMatcher {
            tag: "edns0opt".to_string(),
            codes: codes.iter().copied().collect(),
            require,
            source,
        }
    }

    #[test]
    fn test_single_code_hit_in_request() {
        let matcher = make_matcher(&[8], RequireMode::Any, MatchSource::Request);
        let mut ctx = make_context(&[subnet_option()]);
        assert!(matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_multiple_codes_any_hit() {
        let matcher = make_matcher(&[8, 12], RequireMode::Any, MatchSource::Request);
        let mut ctx = make_context(&[subnet_option()]);
        assert!(matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_multiple_codes_all_requires_every_code() {
        let matcher = make_matcher(&[8, 12], RequireMode::All, MatchSource::Request);
        let mut ctx = make_context(&[subnet_option()]);
        assert!(!matcher.is_match(&mut ctx));

        let mut ctx = make_context(&[subnet_option(), padding_option()]);
        assert!(matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_no_matching_code() {
        let matcher = make_matcher(&[10], RequireMode::Any, MatchSource::Request);
        let mut ctx = make_context(&[subnet_option()]);
        assert!(!matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_no_opt_record() {
        let matcher = make_matcher(&[8], RequireMode::Any, MatchSource::Request);
        let mut ctx = make_context(&[]);
        assert!(!matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_opt_without_matching_option() {
        let matcher = make_matcher(&[8], RequireMode::Any, MatchSource::Request);
        let mut ctx = make_context(&[padding_option()]);
        assert!(!matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_source_response_matches_response_side() {
        let matcher = make_matcher(&[8], RequireMode::Any, MatchSource::Response);
        let mut ctx = make_context(&[padding_option()]);

        let mut response = Message::new();
        response.ensure_edns_mut().insert(subnet_option());
        ctx.set_response(response);
        assert!(matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_source_response_without_response() {
        let matcher = make_matcher(&[8], RequireMode::Any, MatchSource::Response);
        let mut ctx = make_context(&[subnet_option()]);
        assert!(!matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_empty_codes_config_error() {
        assert!(parse_config(Some(serde_yaml_ng::from_str("codes: []").unwrap())).is_err());
        assert!(parse_config(Some(serde_yaml_ng::from_str("require: all").unwrap())).is_err());
        assert!(parse_config(None).is_err());
    }

    #[test]
    fn test_parse_config_fields() {
        let (codes, require, source) = parse_config(Some(
            serde_yaml_ng::from_str("codes: [8, 12]\nrequire: all\nsource: response").unwrap(),
        ))
        .expect("config should parse");
        assert_eq!(codes, vec![8, 12]);
        assert_eq!(require, RequireMode::All);
        assert_eq!(source, MatchSource::Response);
    }

    #[test]
    fn test_parse_config_accepts_mixed_code_forms() {
        let (codes, require, source) = parse_config(Some(
            serde_yaml_ng::from_str("codes: [8, \"12\", 65001, \"1\"]").unwrap(),
        ))
        .expect("mixed integer and string codes should parse");
        assert_eq!(codes, vec![8, 12, 65001, 1]);
        assert_eq!(require, RequireMode::Any);
        assert_eq!(source, MatchSource::Request);

        assert!(
            parse_config(Some(serde_yaml_ng::from_str("codes: [\"65536\"]").unwrap())).is_err()
        );
        assert!(parse_config(Some(serde_yaml_ng::from_str("codes: [\"ecs\"]").unwrap())).is_err());
        assert!(parse_config(Some(serde_yaml_ng::from_str("codes: [65536]").unwrap())).is_err());
    }

    #[test]
    fn test_quick_setup_only_accepts_codes() {
        let (codes, require, source) =
            parse_quick_setup(Some("8,12".to_string())).expect("quick setup should parse");
        assert_eq!(codes, vec![8, 12]);
        assert_eq!(require, RequireMode::Any);
        assert_eq!(source, MatchSource::Request);

        for invalid in ["resp", "all 8", "any 8", "response 8", "8 resp"] {
            assert!(
                parse_quick_setup(Some(invalid.to_string())).is_err(),
                "quick setup should reject '{}'",
                invalid
            );
        }
        assert!(parse_quick_setup(None).is_err());
    }
}
