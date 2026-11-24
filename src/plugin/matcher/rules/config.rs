// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_yaml_ng::{Number, Value};

use crate::infra::error::{DnsError, Result as DnsResult};

pub(crate) fn parse_rules_from_value(args: Option<Value>) -> DnsResult<Vec<String>> {
    let args = args.ok_or_else(|| DnsError::plugin("matcher requires args"))?;
    parse_rule_list("matcher args", args)
}

/// Split a rule-list field into raw rule tokens.
///
/// Shared by matcher `args` and non-matcher consumers (for example the
/// `forward` plugin's `bogus_answer_ips`). `field` names the offending
/// configuration field in error messages.
pub(crate) fn parse_rule_list(field: &str, value: Value) -> DnsResult<Vec<String>> {
    match value {
        Value::String(value) => Ok(split_rule_tokens(&value)),
        Value::Sequence(sequence) => {
            let mut rules = Vec::with_capacity(sequence.len());
            for (idx, item) in sequence.into_iter().enumerate() {
                match item {
                    Value::String(value) => rules.extend(split_rule_tokens(&value)),
                    other => {
                        return Err(DnsError::plugin(format!(
                            "{}[{}] must be a string, got {:?}",
                            field, idx, other
                        )));
                    }
                }
            }
            Ok(rules)
        }
        other => Err(DnsError::plugin(format!(
            "{} must be a string or string array, got {:?}",
            field, other
        ))),
    }
}

pub(crate) fn parse_enum_rules_from_value(
    field: &str,
    args: Option<Value>,
) -> DnsResult<Vec<String>> {
    let args = args.ok_or_else(|| DnsError::plugin(format!("{field} matcher requires args")))?;
    parse_enum_rule_list_value(field, args)
}

pub(crate) fn parse_quick_setup_rules(param: Option<String>) -> DnsResult<Vec<String>> {
    let raw = param.ok_or_else(|| DnsError::plugin("quick setup requires matcher parameter"))?;
    let rules = split_rule_tokens(&raw);
    if rules.is_empty() {
        return Err(DnsError::plugin(
            "quick setup requires non-empty matcher parameter",
        ));
    }
    Ok(rules)
}

pub(crate) fn validate_non_empty_rules(field: &str, rules: &[String]) -> DnsResult<()> {
    if rules.is_empty() {
        return Err(DnsError::plugin(format!(
            "{} matcher requires at least one rule",
            field
        )));
    }
    Ok(())
}

fn parse_enum_rule_list_value(field: &str, value: Value) -> DnsResult<Vec<String>> {
    match value {
        Value::String(value) => Ok(split_rule_tokens(&value)),
        Value::Number(number) => Ok(vec![parse_u16_number_rule(field, &number)?]),
        Value::Sequence(sequence) => {
            let mut rules = Vec::with_capacity(sequence.len());
            for (idx, item) in sequence.into_iter().enumerate() {
                match item {
                    Value::String(value) => rules.extend(split_rule_tokens(&value)),
                    Value::Number(number) => rules.push(parse_u16_number_rule(field, &number)?),
                    other => {
                        return Err(DnsError::plugin(format!(
                            "{} matcher args[{}] must be a string or unsigned integer, got {:?}",
                            field, idx, other
                        )));
                    }
                }
            }
            Ok(rules)
        }
        other => Err(DnsError::plugin(format!(
            "{} matcher args must be a string, unsigned integer, or list of strings/unsigned integers, got {:?}",
            field, other
        ))),
    }
}

fn parse_u16_number_rule(field: &str, number: &Number) -> DnsResult<String> {
    if let Some(value) = number.as_u64() {
        let value = u16::try_from(value).map_err(|_| {
            DnsError::plugin(format!(
                "invalid {} value {}: numeric value must be between 0 and 65535",
                field, value
            ))
        })?;
        return Ok(value.to_string());
    }
    if let Some(value) = number.as_i64() {
        return Err(DnsError::plugin(format!(
            "invalid {} value {}: numeric value must be between 0 and 65535",
            field, value
        )));
    }
    Err(DnsError::plugin(format!(
        "invalid {} value {}: numeric value must be an integer between 0 and 65535",
        field, number
    )))
}

fn split_rule_tokens(raw: &str) -> Vec<String> {
    raw.split(|character: char| character == ',' || character.is_ascii_whitespace())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_setup_rules_are_validated_and_split() {
        assert!(parse_quick_setup_rules(None).is_err());
        assert!(parse_quick_setup_rules(Some("   ".to_string())).is_err());
        assert_eq!(
            parse_quick_setup_rules(Some("a, b c".to_string())).expect("rules should parse"),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn enum_rules_accept_strings_and_numbers() {
        let value = serde_yaml_ng::from_str::<Value>("- 1\n- A,AAAA\n- ServFail\n")
            .expect("yaml should parse");
        assert_eq!(
            parse_enum_rules_from_value("qtype", Some(value)).expect("rules should parse"),
            vec!["1", "A", "AAAA", "ServFail"]
        );
    }

    #[test]
    fn enum_rules_reject_invalid_values() {
        for (raw, expected) in [
            ("-1", "between 0 and 65535"),
            ("256.0", "must be an integer"),
            ("70000", "between 0 and 65535"),
            ("true", "must be a string"),
        ] {
            let value = serde_yaml_ng::from_str::<Value>(raw).expect("yaml should parse");
            let err = parse_enum_rules_from_value("qtype", Some(value))
                .expect_err("invalid value should be rejected");
            assert!(err.to_string().contains(expected));
        }
    }
}
