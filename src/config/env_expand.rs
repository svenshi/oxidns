// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Environment variable substitution for YAML configuration text.
//!
//! Expansion happens before YAML deserialization so every string position,
//! including include paths, can reference process environment variables.

use std::ffi::OsString;
use std::fmt;

use crate::core::env;

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

/// Expand `${VAR}` and `${VAR:-default}` placeholders in configuration text.
///
/// Use `$${...}` to keep a literal `${...}` in the output.
pub fn expand_env(input: &str) -> Result<String, EnvExpandError> {
    expand_env_with_lookup(input, |name| env::var_os(name))
}

fn expand_env_with_lookup<F>(input: &str, lookup: F) -> Result<String, EnvExpandError>
where
    F: Fn(&str) -> Option<OsString>,
{
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut line = 1;
    let mut col = 1;

    while let Some(ch) = chars.next() {
        let start_line = line;
        let start_col = col;
        advance_position(ch, &mut line, &mut col);

        if ch != '$' {
            output.push(ch);
            continue;
        }

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

                match lookup(name) {
                    Some(value) if !value.as_os_str().is_empty() => {
                        output.push_str(&value.to_string_lossy());
                    }
                    _ => {
                        if let Some(default) = default {
                            output.push_str(default);
                        } else {
                            return Err(EnvExpandError::UndefinedVariable {
                                name: name.to_string(),
                                line: start_line,
                                col: start_col,
                            });
                        }
                    }
                }
            }
            _ => output.push('$'),
        }
    }

    Ok(output)
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
    fn uses_default_for_empty_environment_value() {
        let expanded = expand_env_with_lookup("${EMPTY:-fallback}", lookup).expect("expand");
        assert_eq!(expanded, "fallback");
    }

    #[test]
    fn keeps_escaped_placeholder_literal() {
        let expanded = expand_env_with_lookup("$${LITERAL}", lookup).expect("expand");
        assert_eq!(expanded, "${LITERAL}");
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
}
