// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `edns0opt` matcher plugin.
//!
//! Matches by the presence of EDNS0 option codes in the request or response
//! OPT record.
//!
//! Configuration:
//! - `codes`: required, non-empty list of EDNS0 option codes.
//! - `require`: `any` (default) or `all`.
//! - `resp`: defaults to `false` (request side); `true` matches the response
//!   side OPT.
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

#[derive(Debug, Clone, Deserialize)]
struct Edns0OptConfig {
    /// EDNS0 option codes that must be present.
    codes: Vec<u16>,
    /// Whether one or all codes must be present; defaults to `any`.
    #[serde(default)]
    require: RequireMode,
    /// Match the response OPT instead of the request OPT.
    #[serde(default)]
    resp: bool,
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
        let (codes, require, resp) = parse_config(plugin_config.args.clone())?;
        build_matcher(plugin_config.tag.clone(), codes, require, resp)
    }

    fn quick_setup(&self, tag: &str, param: Option<String>) -> DnsResult<UninitializedPlugin> {
        let (codes, require, resp) = parse_quick_setup(param)?;
        build_matcher(tag.to_string(), codes, require, resp)
    }
}

fn build_matcher(
    tag: String,
    codes: Vec<u16>,
    require: RequireMode,
    resp: bool,
) -> DnsResult<UninitializedPlugin> {
    Ok(UninitializedPlugin::Matcher(Box::new(Edns0OptMatcher {
        tag,
        codes: codes.into_iter().collect(),
        require,
        resp,
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

fn parse_config(args: Option<Value>) -> DnsResult<(Vec<u16>, RequireMode, bool)> {
    let Some(args) = args else {
        return Err(DnsError::plugin(
            "edns0opt matcher requires a config mapping with codes",
        ));
    };

    let cfg: Edns0OptConfig = serde_yaml_ng::from_value(args)
        .map_err(|e| DnsError::plugin(format!("failed to parse edns0opt config: {}", e)))?;
    ensure_codes_present(&cfg.codes)?;

    Ok((cfg.codes, cfg.require, cfg.resp))
}

fn parse_quick_setup(param: Option<String>) -> DnsResult<(Vec<u16>, RequireMode, bool)> {
    let Some(raw) = param else {
        return Err(DnsError::plugin(
            "edns0opt matcher requires at least one code",
        ));
    };

    let mut require = RequireMode::default();
    let mut resp = false;
    let mut codes = Vec::new();

    for token in raw.split(|c: char| c == ',' || c.is_ascii_whitespace()) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        match token.to_ascii_lowercase().as_str() {
            "any" => require = RequireMode::Any,
            "all" => require = RequireMode::All,
            "resp" => resp = true,
            _ => {
                let code = token.parse::<u16>().map_err(|e| {
                    DnsError::plugin(format!("invalid EDNS0 option code '{}': {}", token, e))
                })?;
                codes.push(code);
            }
        }
    }

    ensure_codes_present(&codes)?;
    Ok((codes, require, resp))
}

#[derive(Debug)]
struct Edns0OptMatcher {
    tag: String,
    codes: AHashSet<u16>,
    require: RequireMode,
    resp: bool,
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
        let message = if self.resp {
            let Some(response) = context.response() else {
                return false;
            };
            response
        } else {
            &context.request
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

    fn make_matcher(codes: &[u16], require: RequireMode, resp: bool) -> Edns0OptMatcher {
        Edns0OptMatcher {
            tag: "edns0opt".to_string(),
            codes: codes.iter().copied().collect(),
            require,
            resp,
        }
    }

    #[test]
    fn test_single_code_hit_in_request() {
        let matcher = make_matcher(&[8], RequireMode::Any, false);
        let mut ctx = make_context(&[subnet_option()]);
        assert!(matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_multiple_codes_any_hit() {
        let matcher = make_matcher(&[8, 12], RequireMode::Any, false);
        let mut ctx = make_context(&[subnet_option()]);
        assert!(matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_multiple_codes_all_requires_every_code() {
        let matcher = make_matcher(&[8, 12], RequireMode::All, false);
        let mut ctx = make_context(&[subnet_option()]);
        assert!(!matcher.is_match(&mut ctx));

        let mut ctx = make_context(&[subnet_option(), padding_option()]);
        assert!(matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_no_matching_code() {
        let matcher = make_matcher(&[10], RequireMode::Any, false);
        let mut ctx = make_context(&[subnet_option()]);
        assert!(!matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_no_opt_record() {
        let matcher = make_matcher(&[8], RequireMode::Any, false);
        let mut ctx = make_context(&[]);
        assert!(!matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_opt_without_matching_option() {
        let matcher = make_matcher(&[8], RequireMode::Any, false);
        let mut ctx = make_context(&[padding_option()]);
        assert!(!matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_resp_true_matches_response_side() {
        let matcher = make_matcher(&[8], RequireMode::Any, true);
        let mut ctx = make_context(&[padding_option()]);

        let mut response = Message::new();
        response.ensure_edns_mut().insert(subnet_option());
        ctx.set_response(response);
        assert!(matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_resp_true_without_response() {
        let matcher = make_matcher(&[8], RequireMode::Any, true);
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
        let (codes, require, resp) = parse_config(Some(
            serde_yaml_ng::from_str("codes: [8, 12]\nrequire: all\nresp: true").unwrap(),
        ))
        .expect("config should parse");
        assert_eq!(codes, vec![8, 12]);
        assert_eq!(require, RequireMode::All);
        assert!(resp);
    }

    #[test]
    fn test_quick_setup_modifiers() {
        let (codes, require, resp) =
            parse_quick_setup(Some("all resp 8,12".to_string())).expect("quick setup should parse");
        assert_eq!(codes, vec![8, 12]);
        assert_eq!(require, RequireMode::All);
        assert!(resp);
        assert!(parse_quick_setup(Some("resp".to_string())).is_err());
    }
}
