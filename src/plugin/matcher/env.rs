// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `env` matcher plugin.
//!
//! Matches startup/runtime environment variables.

use std::fmt::Debug;

use async_trait::async_trait;
use serde_yaml_ng::Value;

use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::core::env;
use crate::core::error::{DnsError, Result as DnsResult};
use crate::plugin::matcher::Matcher;
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;

#[derive(Debug, Clone)]
#[plugin_factory("env")]
pub struct EnvFactory {}

impl PluginFactory for EnvFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> DnsResult<UninitializedPlugin> {
        let conditions = parse_env_args_from_value(plugin_config.args.clone())?;
        Ok(UninitializedPlugin::Matcher(Box::new(EnvMatcher {
            tag: plugin_config.tag.clone(),
            conditions,
        })))
    }

    fn quick_setup(&self, tag: &str, param: Option<String>) -> DnsResult<UninitializedPlugin> {
        let conditions = parse_env_quick_setup_args(param)?;
        Ok(UninitializedPlugin::Matcher(Box::new(EnvMatcher {
            tag: tag.to_string(),
            conditions,
        })))
    }
}

fn parse_env_args_from_value(args: Option<Value>) -> DnsResult<Vec<EnvCondition>> {
    let args = args.ok_or_else(|| DnsError::plugin("env matcher requires env conditions"))?;
    match args {
        Value::String(raw) => parse_env_args(split_env_inline_args(&raw)),
        Value::Sequence(seq) => {
            let mut args = Vec::with_capacity(seq.len());
            for (idx, item) in seq.into_iter().enumerate() {
                match item {
                    Value::String(raw) => args.push(raw),
                    other => {
                        return Err(DnsError::plugin(format!(
                            "env matcher args[{idx}] must be a string, got {:?}",
                            other
                        )));
                    }
                }
            }
            parse_env_args(args)
        }
        other => Err(DnsError::plugin(format!(
            "env matcher args must be a string or string array, got {:?}",
            other
        ))),
    }
}

fn parse_env_quick_setup_args(param: Option<String>) -> DnsResult<Vec<EnvCondition>> {
    let raw = param.ok_or_else(|| DnsError::plugin("quick setup requires matcher parameter"))?;
    parse_env_args(split_env_inline_args(&raw))
}

fn split_env_inline_args(raw: &str) -> Vec<String> {
    raw.split_ascii_whitespace()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_env_args(args: Vec<String>) -> DnsResult<Vec<EnvCondition>> {
    if args.is_empty() {
        return Err(DnsError::plugin("env matcher requires env conditions"));
    }

    let args = args
        .into_iter()
        .map(|arg| arg.trim().to_string())
        .collect::<Vec<_>>();
    if args.iter().any(|arg| arg.is_empty()) {
        return Err(DnsError::plugin("env condition cannot be empty"));
    }

    let mut conditions = Vec::with_capacity(args.len());
    for arg in args {
        conditions.push(parse_env_condition(&arg)?);
    }
    Ok(conditions)
}

fn parse_env_condition(raw: &str) -> DnsResult<EnvCondition> {
    let Some(index) = raw.find([':', '=']) else {
        return EnvCondition::new(raw.to_string(), None);
    };

    let key = raw[..index].trim().to_string();
    if key.is_empty() {
        return Err(DnsError::plugin("env key cannot be empty"));
    }
    let value = raw[index + 1..].trim();
    EnvCondition::new(
        key,
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        },
    )
}

#[derive(Debug)]
struct EnvCondition {
    key: String,
    value: Option<String>,
    cached_exists: bool,
    cached_value: Option<String>,
}

impl EnvCondition {
    fn new(key: String, value: Option<String>) -> DnsResult<Self> {
        if key.trim().is_empty() {
            return Err(DnsError::plugin("env key cannot be empty"));
        }
        Ok(Self {
            key,
            value,
            cached_exists: false,
            cached_value: None,
        })
    }

    fn cache(&mut self) {
        let raw = env::var_os(&self.key);
        self.cached_exists = raw.is_some();
        self.cached_value = raw.map(|v| v.to_string_lossy().into_owned());
    }

    fn is_match(&self) -> bool {
        if !self.cached_exists {
            return false;
        }

        if let Some(expect) = &self.value {
            self.cached_value.as_deref() == Some(expect.as_str())
        } else {
            true
        }
    }
}

#[derive(Debug)]
struct EnvMatcher {
    tag: String,
    conditions: Vec<EnvCondition>,
}

#[async_trait]
impl Plugin for EnvMatcher {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> DnsResult<()> {
        for condition in &mut self.conditions {
            condition.cache();
        }
        Ok(())
    }

    async fn destroy(&self) -> DnsResult<()> {
        Ok(())
    }
}

impl Matcher for EnvMatcher {
    #[hotpath::measure]
    fn is_match(&self, _context: &mut DnsContext) -> bool {
        self.conditions.iter().all(EnvCondition::is_match)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::test_utils::test_context;

    fn assert_condition(condition: &EnvCondition, key: &str, value: Option<&str>) {
        assert_eq!(condition.key, key);
        assert_eq!(condition.value.as_deref(), value);
        assert!(!condition.cached_exists);
        assert!(condition.cached_value.is_none());
    }

    fn cached_condition(
        key: &str,
        value: Option<&str>,
        cached_exists: bool,
        cached_value: Option<&str>,
    ) -> EnvCondition {
        EnvCondition {
            key: key.to_string(),
            value: value.map(str::to_string),
            cached_exists,
            cached_value: cached_value.map(str::to_string),
        }
    }

    #[test]
    fn test_parse_env_args_validation() {
        assert!(parse_env_args(vec![]).is_err());
        assert!(parse_env_args(vec![" ".to_string()]).is_err());
        assert!(parse_env_args(vec![":VALUE".to_string()]).is_err());
        assert!(parse_env_args(vec!["KEY".to_string()]).is_ok());
    }

    #[test]
    fn test_parse_env_args_supports_presence_forms() {
        for raw in ["KEY", "KEY:", "KEY="] {
            let conditions = parse_env_args(vec![raw.to_string()]).expect("parse should succeed");
            assert_eq!(conditions.len(), 1);
            assert_condition(&conditions[0], "KEY", None);
        }
    }

    #[test]
    fn test_parse_env_args_supports_value_forms() {
        for raw in ["KEY:VALUE", "KEY=VALUE"] {
            let conditions = parse_env_args(vec![raw.to_string()]).expect("parse should succeed");
            assert_eq!(conditions.len(), 1);
            assert_condition(&conditions[0], "KEY", Some("VALUE"));
        }
    }

    #[test]
    fn test_parse_env_args_from_value_preserves_sequence_item_delimiters() {
        let value = serde_yaml_ng::from_str::<Value>(
            r#"
- "NO_PROXY=localhost,127.0.0.1"
- "GREETING=hello world"
- "URL=https://example.com/path,a?x=1"
"#,
        )
        .expect("yaml should parse");

        let conditions = parse_env_args_from_value(Some(value)).expect("parse should succeed");

        assert_eq!(conditions.len(), 3);
        assert_condition(&conditions[0], "NO_PROXY", Some("localhost,127.0.0.1"));
        assert_condition(&conditions[1], "GREETING", Some("hello world"));
        assert_condition(
            &conditions[2],
            "URL",
            Some("https://example.com/path,a?x=1"),
        );
    }

    #[test]
    fn test_parse_env_inline_args_preserves_commas_in_values() {
        let conditions =
            parse_env_quick_setup_args(Some("NO_PROXY=localhost,127.0.0.1 FEATURE_X".to_string()))
                .expect("parse should succeed");

        assert_eq!(conditions.len(), 2);
        assert_condition(&conditions[0], "NO_PROXY", Some("localhost,127.0.0.1"));
        assert_condition(&conditions[1], "FEATURE_X", None);
    }

    #[test]
    fn test_parse_env_args_supports_multiple_mixed_conditions() {
        let conditions = parse_env_args(vec![
            "A:alpha".to_string(),
            "B=beta".to_string(),
            "C".to_string(),
            "D:".to_string(),
        ])
        .expect("parse should succeed");

        assert_eq!(conditions.len(), 4);
        assert_condition(&conditions[0], "A", Some("alpha"));
        assert_condition(&conditions[1], "B", Some("beta"));
        assert_condition(&conditions[2], "C", None);
        assert_condition(&conditions[3], "D", None);
    }

    #[test]
    fn test_parse_env_args_treats_two_bare_items_as_presence_checks() {
        let conditions = parse_env_args(vec!["KEY".to_string(), "VALUE".to_string()])
            .expect("parse should succeed");

        assert_eq!(conditions.len(), 2);
        assert_condition(&conditions[0], "KEY", None);
        assert_condition(&conditions[1], "VALUE", None);
    }

    #[test]
    fn test_parse_env_args_honors_explicit_second_condition() {
        let conditions = parse_env_args(vec!["FEATURE_X".to_string(), "PROFILE:prod".to_string()])
            .expect("parse should succeed");

        assert_eq!(conditions.len(), 2);
        assert_condition(&conditions[0], "FEATURE_X", None);
        assert_condition(&conditions[1], "PROFILE", Some("prod"));
    }

    #[test]
    fn test_env_matcher_uses_cached_state() {
        let mut ctx = test_context();
        let exists_matcher = EnvMatcher {
            tag: "env".to_string(),
            conditions: vec![cached_condition("K", None, true, Some("v"))],
        };
        assert!(exists_matcher.is_match(&mut ctx));

        let value_matcher = EnvMatcher {
            tag: "env".to_string(),
            conditions: vec![cached_condition("K", Some("v"), true, Some("v"))],
        };
        assert!(value_matcher.is_match(&mut ctx));

        let miss_matcher = EnvMatcher {
            tag: "env".to_string(),
            conditions: vec![cached_condition("K", Some("x"), true, Some("v"))],
        };
        assert!(!miss_matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_env_matcher_requires_all_cached_conditions() {
        let mut ctx = test_context();
        let matcher = EnvMatcher {
            tag: "env".to_string(),
            conditions: vec![
                cached_condition("A", Some("alpha"), true, Some("alpha")),
                cached_condition("B", None, true, Some("beta")),
            ],
        };
        assert!(matcher.is_match(&mut ctx));

        let missing_matcher = EnvMatcher {
            tag: "env".to_string(),
            conditions: vec![
                cached_condition("A", Some("alpha"), true, Some("alpha")),
                cached_condition("B", None, false, None),
            ],
        };
        assert!(!missing_matcher.is_match(&mut ctx));

        let mismatch_matcher = EnvMatcher {
            tag: "env".to_string(),
            conditions: vec![
                cached_condition("A", Some("alpha"), true, Some("alpha")),
                cached_condition("B", Some("expected"), true, Some("actual")),
            ],
        };
        assert!(!mismatch_matcher.is_match(&mut ctx));
    }
}
