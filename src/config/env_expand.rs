// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Environment variable substitution for YAML configuration text.
//!
//! Expansion happens before YAML deserialization so every string position,
//! including include paths, can reference process environment variables.

use std::ffi::OsString;
use std::fmt;

use crate::core::env;

/// The stable set of per-request context keys used by executor template
/// plugins. Config-time env expansion preserves `${key}` for any name in this
/// list so they reach the runtime renderer intact.
pub(crate) const BUILTIN_KEYS: &[&str] = &[
    "qname",
    "qtype",
    "qtype_name",
    "qclass",
    "qclass_name",
    "client_ip",
    "client_port",
    "server_name",
    "url_path",
    "marks",
    "has_resp",
    "rcode",
    "rcode_name",
    "resp_ip",
    "cron_plugin_tag",
    "cron_job_name",
    "cron_trigger_kind",
    "cron_scheduled_at_unix_ms",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvExpandError {
    UndefinedVariable {
        name: String,
        line: usize,
        col: usize,
    },
    InvalidSyntax {
        reason: String,
        line: usize,
        col: usize,
    },
}

impl fmt::Display for EnvExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndefinedVariable { name, line, col } => write!(
                f,
                "undefined environment variable '{name}' at line {line}, col {col}"
            ),
            Self::InvalidSyntax { reason, line, col } => {
                write!(
                    f,
                    "invalid environment variable placeholder at line {line}, col {col}: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for EnvExpandError {}

/// Expand `${VAR}`, `${VAR:-default}`, and `${env:VAR}` placeholders in
/// configuration text.
///
/// Executor runtime templates using built-in keys, such as `${qname}`, are
/// preserved for per-request rendering. Use the explicit `env:` prefix when an
/// environment variable name conflicts with a runtime template key.
///
/// Use `$${...}` to keep a literal `${...}` in the output.
///
/// Substitution is YAML scalar aware. When a placeholder appears inside a
/// `'single'` or `"double"` quoted YAML scalar, the substituted value is
/// escaped per YAML 1.2 quoting rules so env values containing YAML-special
/// characters (`*`, `&`, `:`, `'`, `"`, `\`, newlines, …) cannot break the
/// surrounding YAML structure. Plain (unquoted) scalar substitution stays
/// verbatim — values whose leading char triggers YAML semantics (anchor
/// alias `*`, block sequence `- `, etc.) must be wrapped in quotes.
pub fn expand_env(input: &str) -> Result<String, EnvExpandError> {
    expand_env_with_lookup(input, |name| env::var_os(name))
}

/// YAML scalar context for `${VAR}` placeholders. Substituted values are
/// escaped to match the enclosing scalar style so env values containing
/// YAML-special characters (`*`, `&`, `\`, `"`, `'`, newlines, …) do not
/// corrupt the surrounding YAML when the placeholder is expanded.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScalarContext {
    /// Outside any quoted scalar. Substitution is verbatim (legacy behavior);
    /// the caller is responsible for choosing a value whose characters parse
    /// correctly as a YAML plain scalar.
    Plain,
    /// Inside a single-quoted scalar `'...'`. Substituted `'` is doubled.
    SingleQuoted,
    /// Inside a double-quoted scalar `"..."`. `\` and `"` are escaped and
    /// control characters are rewritten as YAML 1.2 hex escapes.
    DoubleQuoted,
}

fn expand_env_with_lookup<F>(input: &str, lookup: F) -> Result<String, EnvExpandError>
where
    F: Fn(&str) -> Option<OsString>,
{
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut line = 1;
    let mut col = 1;
    let mut context = ScalarContext::Plain;
    // True when the previous char inside a double-quoted scalar was a
    // backslash, so the current char (including `"` or `\`) is part of an
    // escape sequence and should not toggle quote state.
    let mut double_quoted_escape = false;

    while let Some(ch) = chars.next() {
        let start_line = line;
        let start_col = col;
        advance_position(ch, &mut line, &mut col);

        // Track YAML scalar style before deciding how to substitute `${...}`.
        // The substituted value is escaped to match the enclosing context so
        // env values with YAML-special characters do not break the surrounding
        // structure when the placeholder is expanded.
        match context {
            ScalarContext::Plain => match ch {
                '\'' => {
                    context = ScalarContext::SingleQuoted;
                    output.push(ch);
                    continue;
                }
                '"' => {
                    context = ScalarContext::DoubleQuoted;
                    output.push(ch);
                    continue;
                }
                '$' => {} // fall through to $-handling below
                _ => {
                    output.push(ch);
                    continue;
                }
            },
            ScalarContext::SingleQuoted => match ch {
                '\'' => {
                    // YAML 1.2 single-quoted escape: `''` represents a literal
                    // `'`. Preserve both quotes and stay in single-quoted
                    // context so we don't treat the second quote as a close.
                    if chars.peek().copied() == Some('\'') {
                        chars.next();
                        advance_position('\'', &mut line, &mut col);
                        output.push('\'');
                        output.push('\'');
                        continue;
                    }
                    context = ScalarContext::Plain;
                    output.push(ch);
                    continue;
                }
                '$' => {}
                _ => {
                    output.push(ch);
                    continue;
                }
            },
            ScalarContext::DoubleQuoted => {
                if double_quoted_escape {
                    double_quoted_escape = false;
                    output.push(ch);
                    continue;
                }
                match ch {
                    '\\' => {
                        double_quoted_escape = true;
                        output.push(ch);
                        continue;
                    }
                    '"' => {
                        context = ScalarContext::Plain;
                        output.push(ch);
                        continue;
                    }
                    '$' => {}
                    _ => {
                        output.push(ch);
                        continue;
                    }
                }
            }
        }

        // ch == '$'. Decide between `$$` literal, `${...}` placeholder, or a
        // bare `$` followed by something that is not an opener.
        match chars.peek().copied() {
            Some('$') => {
                chars.next();
                advance_position('$', &mut line, &mut col);
                if matches!(chars.peek().copied(), Some('{')) {
                    output.push('$');
                } else {
                    output.push_str("$$");
                }
            }
            Some('{') => {
                chars.next();
                advance_position('{', &mut line, &mut col);
                let body =
                    read_placeholder_body(&mut chars, &mut line, &mut col, start_line, start_col)?;
                let (name, default) = split_placeholder_body(&body, start_line, start_col)?;
                let (lookup_name, explicit_env) = resolve_lookup_name(name, start_line, start_col)?;

                if !explicit_env && default.is_none() && is_runtime_template_key(name) {
                    output.push_str("${");
                    output.push_str(&body);
                    output.push('}');
                    continue;
                }

                let raw_value: String = match (lookup(lookup_name), default) {
                    (Some(value), Some(default)) if value.as_os_str().is_empty() => {
                        default.to_string()
                    }
                    (Some(value), _) => value.to_string_lossy().into_owned(),
                    (None, Some(default)) => default.to_string(),
                    (None, None) => {
                        return Err(EnvExpandError::UndefinedVariable {
                            name: lookup_name.to_string(),
                            line: start_line,
                            col: start_col,
                        });
                    }
                };

                escape_substitution(&raw_value, context, &mut output);
            }
            _ => output.push('$'),
        }
    }

    Ok(output)
}

/// Append `value` to `output`, escaping it for the YAML scalar context the
/// placeholder appears in so any character (including newlines, quotes, and
/// backslashes) round-trips through YAML parsing.
fn escape_substitution(value: &str, context: ScalarContext, output: &mut String) {
    match context {
        ScalarContext::Plain => {
            // Verbatim: the user accepted plain-scalar quoting responsibility
            // by leaving the placeholder unquoted. Special characters at the
            // start (e.g. a leading `*`) still need the user to wrap the
            // scalar in quotes; we cannot rewrite the surrounding YAML here.
            output.push_str(value);
        }
        ScalarContext::SingleQuoted => {
            // YAML single-quoted scalars only escape `'` as `''`. Every other
            // char (including newlines) is literal.
            for ch in value.chars() {
                if ch == '\'' {
                    output.push_str("''");
                } else {
                    output.push(ch);
                }
            }
        }
        ScalarContext::DoubleQuoted => {
            for ch in value.chars() {
                match ch {
                    '\\' => output.push_str(r"\\"),
                    '"' => output.push_str("\\\""),
                    '\n' => output.push_str(r"\n"),
                    '\r' => output.push_str(r"\r"),
                    '\t' => output.push_str(r"\t"),
                    c if (c as u32) < 0x20 || c as u32 == 0x7F => {
                        // YAML 1.2 8-bit hex escape covers all C0 controls
                        // and DEL; anything wider stays literal because
                        // double-quoted YAML accepts arbitrary Unicode.
                        let _ =
                            std::fmt::Write::write_fmt(output, format_args!("\\x{:02x}", c as u32));
                    }
                    c => output.push(c),
                }
            }
        }
    }
}

fn read_placeholder_body<I>(
    chars: &mut std::iter::Peekable<I>,
    line: &mut usize,
    col: &mut usize,
    start_line: usize,
    start_col: usize,
) -> Result<String, EnvExpandError>
where
    I: Iterator<Item = char>,
{
    let mut body = String::new();

    for ch in chars.by_ref() {
        advance_position(ch, line, col);
        if ch == '}' {
            return Ok(body);
        }
        body.push(ch);
    }

    Err(EnvExpandError::InvalidSyntax {
        reason: "unterminated environment variable placeholder".to_string(),
        line: start_line,
        col: start_col,
    })
}

fn split_placeholder_body(
    body: &str,
    line: usize,
    col: usize,
) -> Result<(&str, Option<&str>), EnvExpandError> {
    let (name, default) = match body.find(":-") {
        Some(index) => (&body[..index], Some(&body[index + 2..])),
        None => (body, None),
    };

    if name.is_empty() {
        return Err(EnvExpandError::InvalidSyntax {
            reason: "empty environment variable name".to_string(),
            line,
            col,
        });
    }

    Ok((name, default))
}

fn resolve_lookup_name(
    name: &str,
    line: usize,
    col: usize,
) -> Result<(&str, bool), EnvExpandError> {
    let Some(explicit_name) = name.strip_prefix("env:") else {
        return Ok((name, false));
    };

    if explicit_name.is_empty() {
        return Err(EnvExpandError::InvalidSyntax {
            reason: "empty explicit environment variable name".to_string(),
            line,
            col,
        });
    }

    Ok((explicit_name, true))
}

fn is_runtime_template_key(name: &str) -> bool {
    BUILTIN_KEYS.contains(&name.trim())
}

fn advance_position(ch: char, line: &mut usize, col: &mut usize) {
    if ch == '\n' {
        *line += 1;
        *col = 1;
    } else {
        *col += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(name: &str) -> Option<OsString> {
        match name {
            "A" => Some("alpha".into()),
            "B" => Some("beta".into()),
            "EMPTY" => Some(OsString::new()),
            _ => None,
        }
    }

    #[test]
    fn expands_regular_placeholders() {
        let expanded = expand_env_with_lookup("before ${A} after", lookup).expect("expand");
        assert_eq!(expanded, "before alpha after");
    }

    #[test]
    fn expands_multiple_and_adjacent_placeholders() {
        let expanded = expand_env_with_lookup("${A}/${B}:${A}${B}", lookup).expect("expand");
        assert_eq!(expanded, "alpha/beta:alphabeta");
    }

    #[test]
    fn supports_default_value() {
        let expanded =
            expand_env_with_lookup("value=${MISSING:-fallback}", lookup).expect("expand");
        assert_eq!(expanded, "value=fallback");
    }

    #[test]
    fn expands_explicit_env_placeholders() {
        let expanded = expand_env_with_lookup("before ${env:A} after", lookup).expect("expand");
        assert_eq!(expanded, "before alpha after");
    }

    #[test]
    fn explicit_env_placeholders_can_access_runtime_key_names() {
        let expanded = expand_env_with_lookup("site=${env:qname}", |name| match name {
            "qname" => Some("from-env".into()),
            _ => lookup(name),
        })
        .expect("expand");
        assert_eq!(expanded, "site=from-env");
    }

    #[test]
    fn explicit_env_placeholders_support_default_value() {
        let expanded =
            expand_env_with_lookup("site=${env:qname:-fallback}", lookup).expect("expand");
        assert_eq!(expanded, "site=fallback");
    }

    #[test]
    fn uses_default_for_empty_environment_value() {
        let expanded = expand_env_with_lookup("${EMPTY:-fallback}", lookup).expect("expand");
        assert_eq!(expanded, "fallback");
    }

    #[test]
    fn expands_empty_environment_value_without_default() {
        let expanded = expand_env_with_lookup("before:${EMPTY}:after", lookup).expect("expand");
        assert_eq!(expanded, "before::after");
    }

    #[test]
    fn keeps_escaped_placeholder_literal() {
        let expanded = expand_env_with_lookup("$${LITERAL}", lookup).expect("expand");
        assert_eq!(expanded, "${LITERAL}");
    }

    #[test]
    fn keeps_runtime_template_placeholders_literal() {
        let expanded =
            expand_env_with_lookup("site=${qname} client=${client_ip}", lookup).expect("expand");
        assert_eq!(expanded, "site=${qname} client=${client_ip}");
    }

    #[test]
    fn keeps_all_runtime_template_placeholders_literal() {
        for key in BUILTIN_KEYS {
            let raw = format!("value=${{{key}}}");
            let expanded = expand_env_with_lookup(&raw, lookup).expect("expand");
            assert_eq!(expanded, raw, "runtime placeholder {key} should survive");
        }
    }

    #[test]
    fn keeps_runtime_template_placeholders_literal_even_when_env_exists() {
        let expanded = expand_env_with_lookup("site=${qname}", |name| match name {
            "qname" => Some("from-env".into()),
            _ => lookup(name),
        })
        .expect("expand");
        assert_eq!(expanded, "site=${qname}");
    }

    #[test]
    fn runtime_template_names_with_defaults_still_expand_as_env_vars() {
        let expanded = expand_env_with_lookup("site=${qname:-fallback}", lookup).expect("expand");
        assert_eq!(expanded, "site=fallback");
    }

    #[test]
    fn rejects_undefined_variable_without_default() {
        let err = expand_env_with_lookup("line one\n${MISSING}", lookup)
            .expect_err("missing variable should fail");
        let msg = err.to_string();
        assert!(msg.contains("MISSING"));
        assert!(msg.contains("line 2"));
    }

    #[test]
    fn rejects_unterminated_placeholder() {
        let err = expand_env_with_lookup("${A", lookup).expect_err("syntax should fail");
        assert!(matches!(err, EnvExpandError::InvalidSyntax { .. }));
        assert!(err.to_string().contains("unterminated"));
    }

    #[test]
    fn rejects_empty_variable_name() {
        let err = expand_env_with_lookup("${}", lookup).expect_err("syntax should fail");
        assert!(matches!(err, EnvExpandError::InvalidSyntax { .. }));
        assert!(err.to_string().contains("empty environment variable name"));
    }

    #[test]
    fn rejects_empty_explicit_env_name() {
        let err = expand_env_with_lookup("${env:}", lookup).expect_err("syntax should fail");
        assert!(matches!(err, EnvExpandError::InvalidSyntax { .. }));
        assert!(
            err.to_string()
                .contains("empty explicit environment variable name")
        );
    }

    #[test]
    fn preserves_non_placeholder_dollars() {
        let expanded =
            expand_env_with_lookup("$abc and a$b and trailing $", lookup).expect("expand");
        assert_eq!(expanded, "$abc and a$b and trailing $");
    }

    #[test]
    fn preserves_double_dollars_outside_placeholder_escape() {
        let expanded = expand_env_with_lookup("cost=$$100", lookup).expect("expand");
        assert_eq!(expanded, "cost=$$100");
    }

    #[test]
    fn public_expand_reads_process_environment() {
        let expected = env::var_lossy("PATH").expect("PATH should exist in test environment");
        let expanded = expand_env("${PATH}").expect("PATH should expand");
        assert_eq!(expanded, expected);
    }

    /// `${PW}` inside a single-quoted YAML scalar must escape literal `'`
    /// inside the env value so the surrounding scalar stays terminated. Values
    /// containing every other YAML-special character (`*`, `&`, `:`, `#`, …)
    /// stay literal because single-quoted YAML treats them as plain text.
    #[test]
    fn substitutes_inside_single_quoted_scalar_escaping_single_quotes() {
        let yaml = "key: '${PW}'";
        let lookup = |name: &str| match name {
            "PW" => Some(OsString::from("can't*stop & go")),
            _ => None,
        };
        let expanded = expand_env_with_lookup(yaml, lookup).expect("expand");
        assert_eq!(expanded, "key: 'can''t*stop & go'");
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&expanded).expect("expanded YAML must parse");
        assert_eq!(
            value.get("key").and_then(|v| v.as_str()),
            Some("can't*stop & go")
        );
    }

    /// `${PW}` inside a double-quoted YAML scalar must escape `\`, `"`, and
    /// control chars per YAML 1.2 double-quoted style so the env value
    /// survives parsing intact.
    #[test]
    fn substitutes_inside_double_quoted_scalar_escaping_special_chars() {
        let yaml = "key: \"${PW}\"";
        let lookup = |name: &str| match name {
            "PW" => Some(OsString::from("a\"b\\c\nd\te")),
            _ => None,
        };
        let expanded = expand_env_with_lookup(yaml, lookup).expect("expand");
        assert_eq!(expanded, r#"key: "a\"b\\c\nd\te""#);
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&expanded).expect("expanded YAML must parse");
        assert_eq!(
            value.get("key").and_then(|v| v.as_str()),
            Some("a\"b\\c\nd\te")
        );
    }

    /// Quoted-scalar substitution must work for the specific YAML alias trap
    /// users hit: `*` at the start of an env value would alias-parse if
    /// substituted into a plain scalar, but quoting must make it safe.
    #[test]
    fn quoted_scalars_accept_yaml_alias_leading_chars() {
        let lookup = |name: &str| match name {
            "PW" => Some(OsString::from("*foo")),
            _ => None,
        };
        for yaml in ["key: \"${PW}\"", "key: '${PW}'"] {
            let expanded = expand_env_with_lookup(yaml, lookup).expect("expand");
            let value: serde_yaml_ng::Value =
                serde_yaml_ng::from_str(&expanded).expect("expanded YAML must parse");
            assert_eq!(
                value.get("key").and_then(|v| v.as_str()),
                Some("*foo"),
                "{yaml} must round-trip the env value verbatim"
            );
        }
    }

    /// Plain-scalar substitution stays verbatim (legacy behavior). A `${PW}`
    /// with a leading `*` still fails YAML parsing because we cannot rewrite
    /// the unquoted scalar; documented behavior is that the user must quote.
    #[test]
    fn plain_scalar_substitution_is_verbatim() {
        let lookup = |name: &str| match name {
            "A" => Some(OsString::from("alpha")),
            _ => None,
        };
        let expanded = expand_env_with_lookup("key: ${A}", lookup).expect("expand");
        assert_eq!(expanded, "key: alpha");
    }

    /// `'` inside an enclosing double-quoted scalar must be treated as a
    /// literal — not as the start of a single-quoted scalar — so a subsequent
    /// `${VAR}` is still seen as double-quoted.
    #[test]
    fn nested_quote_chars_dont_flip_context() {
        let lookup = |name: &str| match name {
            "PW" => Some(OsString::from("X\"Y")),
            _ => None,
        };
        let expanded = expand_env_with_lookup("key: \"o'clock ${PW}\"", lookup).expect("expand");
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&expanded).expect("expanded YAML must parse");
        assert_eq!(
            value.get("key").and_then(|v| v.as_str()),
            Some("o'clock X\"Y")
        );
    }

    /// A `\"` escape inside a double-quoted scalar must NOT terminate the
    /// scalar — the next char remains in DoubleQuoted context.
    #[test]
    fn double_quoted_backslash_escape_keeps_context() {
        let lookup = |name: &str| match name {
            "PW" => Some(OsString::from("*foo")),
            _ => None,
        };
        // The literal YAML escape `\"` represents `"` inside the value; the
        // ${PW} placeholder remains inside the same double-quoted scalar.
        let expanded = expand_env_with_lookup(r#"key: "pre\"mid ${PW}""#, lookup).expect("expand");
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&expanded).expect("expanded YAML must parse");
        assert_eq!(
            value.get("key").and_then(|v| v.as_str()),
            Some("pre\"mid *foo")
        );
    }

    /// `''` inside a single-quoted scalar must NOT terminate the scalar.
    #[test]
    fn single_quoted_double_apostrophe_keeps_context() {
        let lookup = |name: &str| match name {
            "PW" => Some(OsString::from("*foo")),
            _ => None,
        };
        let expanded = expand_env_with_lookup("key: 'it''s ${PW}'", lookup).expect("expand");
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&expanded).expect("expanded YAML must parse");
        assert_eq!(value.get("key").and_then(|v| v.as_str()), Some("it's *foo"));
    }
}
