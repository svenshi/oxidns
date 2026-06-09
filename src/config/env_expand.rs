// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Environment variable substitution for parsed YAML configuration.
//!
//! Expansion runs **after** the YAML parser has produced a [`Value`] tree —
//! it never operates on raw YAML text. This keeps the YAML grammar a clean
//! boundary: an environment value can contain any character at all (`*`,
//! `&`, `:`, `'`, `"`, `\`, newlines, binary garbage…) without the risk of
//! breaking the surrounding document.
//!
//! Walking the tree, every `Value::String` is scanned for `${VAR}`,
//! `${VAR:-default}`, or `${env:VAR}` placeholders and replaced with the
//! looked-up environment value. Sequence items, mapping values, and string
//! mapping keys are all expanded; runtime-template placeholders such as
//! `${qname}` (see `BUILTIN_KEYS`) are preserved verbatim for later
//! per-request rendering.
//!
//! When the original scalar was *exactly* one placeholder (no surrounding
//! literal text) the expanded value is re-parsed once as a YAML scalar so
//! `timeout: ${TIMEOUT}` with `TIMEOUT=30` still recovers a `Number`. The
//! re-parse only adopts a non-string result when it is a simple scalar
//! (`Number` / `Bool` / `Null`) without newlines and structure characters;
//! everything else falls back to `Value::String(expanded)` so an env value
//! that happens to look like a YAML alias (`*foo`) or a multi-document blob
//! never silently mutates into structure.

use std::ffi::OsString;
use std::fmt::{self, Write as _};

use serde_yaml_ng::{Mapping, Value};

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
    UndefinedVariable { name: String, path: String },
    InvalidSyntax { reason: String, path: String },
}

impl fmt::Display for EnvExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndefinedVariable { name, path } => {
                write!(
                    f,
                    "undefined environment variable '{name}' at {}",
                    display_path(path)
                )
            }
            Self::InvalidSyntax { reason, path } => {
                write!(
                    f,
                    "invalid environment variable placeholder at {}: {reason}",
                    display_path(path)
                )
            }
        }
    }
}

impl std::error::Error for EnvExpandError {}

fn display_path(path: &str) -> &str {
    if path.is_empty() { "<root>" } else { path }
}

/// Expand `${VAR}`, `${VAR:-default}`, and `${env:VAR}` placeholders inside
/// every string scalar of the YAML `Value` tree (sequence items, mapping
/// values, and string mapping keys).
///
/// Executor runtime templates using built-in keys, such as `${qname}`, are
/// preserved for per-request rendering. Use the explicit `env:` prefix when an
/// environment variable name conflicts with a runtime template key.
///
/// Use `$${...}` to keep a literal `${...}` in the expanded string.
///
/// Because expansion runs after YAML parsing, the substituted value can
/// contain any character — including YAML structural characters and newlines.
/// The surrounding YAML document is parsed before the env value is ever
/// looked at, so there is no possibility of an env value corrupting the
/// document structure.
pub fn expand_env_in_value(value: &mut Value) -> Result<(), EnvExpandError> {
    expand_env_in_value_with_lookup(value, &|name| env::var_os(name))
}

pub(crate) fn expand_env_in_value_with_lookup<F>(
    value: &mut Value,
    lookup: &F,
) -> Result<(), EnvExpandError>
where
    F: Fn(&str) -> Option<OsString>,
{
    let mut path = String::new();
    expand_in_value(value, &mut path, lookup)
}

fn expand_in_value<F>(
    value: &mut Value,
    path: &mut String,
    lookup: &F,
) -> Result<(), EnvExpandError>
where
    F: Fn(&str) -> Option<OsString>,
{
    match value {
        Value::String(s) => {
            if needs_expansion(s) {
                let expanded = expand_placeholders(s, path, lookup)?;
                *value = recover_scalar(s, expanded);
            }
        }
        Value::Sequence(items) => {
            for (i, item) in items.iter_mut().enumerate() {
                let saved = path.len();
                write!(path, "[{i}]").unwrap();
                expand_in_value(item, path, lookup)?;
                path.truncate(saved);
            }
        }
        Value::Mapping(map) => expand_in_mapping(map, path, lookup)?,
        _ => {}
    }
    Ok(())
}

fn expand_in_mapping<F>(
    map: &mut Mapping,
    path: &mut String,
    lookup: &F,
) -> Result<(), EnvExpandError>
where
    F: Fn(&str) -> Option<OsString>,
{
    // First pass: walk values in place. Keys are stable through this pass so
    // path strings stay valid for the recursive walk.
    for (k, v) in map.iter_mut() {
        let saved = path.len();
        push_key_segment(path, k);
        expand_in_value(v, path, lookup)?;
        path.truncate(saved);
    }

    // Second pass: rebuild the map only when at least one string key carries
    // a placeholder. Rebuilding preserves insertion order and lets us expand
    // keys with full env semantics (including type recovery for numeric keys).
    let needs_key_rebuild = map
        .iter()
        .any(|(k, _)| matches!(k, Value::String(s) if needs_expansion(s)));
    if !needs_key_rebuild {
        return Ok(());
    }

    let pairs: Vec<(Value, Value)> = std::mem::take(map).into_iter().collect();
    for (k, v) in pairs {
        let new_key = if let Value::String(s) = &k {
            if needs_expansion(s) {
                let saved = path.len();
                push_key_segment(path, &k);
                let expanded = expand_placeholders(s, path, lookup)?;
                path.truncate(saved);
                recover_scalar(s, expanded)
            } else {
                k
            }
        } else {
            k
        };
        map.insert(new_key, v);
    }
    Ok(())
}

fn push_key_segment(path: &mut String, key: &Value) {
    match key {
        Value::String(s) => write!(path, ".{s}").unwrap(),
        Value::Number(n) => write!(path, ".{n}").unwrap(),
        Value::Bool(b) => write!(path, ".{b}").unwrap(),
        Value::Null => path.push_str(".null"),
        _ => write!(path, ".{key:?}").unwrap(),
    }
}

/// Quick rejection for strings that obviously cannot hold a placeholder.
/// Saves an allocation and a char-by-char scan for the common case where the
/// scalar is plain literal text. `${` is the only prefix that can introduce
/// the regular form; `$$` is only meaningful when followed by `{` (which
/// `expand_placeholders` decides), but it cheaply falls through the same
/// substring check.
fn needs_expansion(s: &str) -> bool {
    s.contains("${") || s.contains("$$")
}

/// Substitute every `${...}` placeholder in `input`. Errors carry the YAML
/// path of the enclosing scalar (e.g. `plugins[0].args.password`) instead of
/// text line/column — by the time we reach this function the source text is
/// long gone, and the path is what the user (and the WebUI) actually wants.
fn expand_placeholders<F>(input: &str, path: &str, lookup: &F) -> Result<String, EnvExpandError>
where
    F: Fn(&str) -> Option<OsString>,
{
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            output.push(ch);
            continue;
        }
        match chars.peek().copied() {
            // `$$` escape: a following `{` lets the user write a literal
            // `${...}` (we emit a single `$` and let the `{` be consumed by
            // the next iteration as plain text); a non-`{` follower stays as
            // the literal `$$`.
            Some('$') => {
                chars.next();
                if matches!(chars.peek().copied(), Some('{')) {
                    output.push('$');
                } else {
                    output.push_str("$$");
                }
            }
            Some('{') => {
                chars.next();
                let body = read_placeholder_body(&mut chars, path)?;
                let (name, default) = split_placeholder_body(&body, path)?;
                let (lookup_name, explicit_env) = resolve_lookup_name(name, path)?;

                // Runtime-template names like `${qname}` reach this code path
                // because they share the env placeholder syntax, but they are
                // resolved per-request by the executor template engine — not
                // by env. Re-emit them verbatim so the runtime sees the
                // original placeholder. `${env:qname}` and `${qname:-default}`
                // are explicit overrides that fall through to env lookup.
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
                            path: path.to_string(),
                        });
                    }
                };
                output.push_str(&raw_value);
            }
            _ => output.push('$'),
        }
    }
    Ok(output)
}

/// Decide what YAML `Value` an expanded scalar should deserialize to. When
/// the original scalar was *exactly* one placeholder (no literal text around
/// it) we try a YAML re-parse to recover `Number` / `Bool` / `Null` typing —
/// otherwise we treat the substitution as opaque text.
///
/// This preserves the legacy ability to write `timeout: ${TIMEOUT}` and have
/// the env value land in a `u64` field, while making it impossible for an env
/// value containing YAML structure (`*foo`, `&anchor`, `--- doc`, …) to be
/// silently re-parsed as YAML semantics.
fn recover_scalar(original: &str, expanded: String) -> Value {
    if !is_single_placeholder(original) {
        return Value::String(expanded);
    }
    if expanded.contains('\n') || expanded.contains('\r') {
        return Value::String(expanded);
    }
    // The re-parse step is what lets `timeout: ${T}` with `T=30` land in a
    // numeric field, but a naive `from_str` would also honor YAML *comments*
    // and *document markers* inside the env value. `PW="true # keep-this"`
    // would parse as `Bool(true)` and silently drop the comment; `T="30 #x"`
    // would parse as `Number(30)`. Reject any expansion that contains an
    // internal whitespace character or `#` before attempting recovery so
    // the env value is opaque to YAML grammar in every position except a
    // single bare scalar literal. (Surrounding whitespace is trimmed first
    // because YAML scalar parsing already ignores it.)
    let trimmed = expanded.trim();
    if trimmed.chars().any(|c| c.is_whitespace() || c == '#') {
        return Value::String(expanded);
    }
    match serde_yaml_ng::from_str::<Value>(trimmed) {
        Ok(Value::Number(n)) => Value::Number(n),
        Ok(Value::Bool(b)) => Value::Bool(b),
        Ok(Value::Null) => Value::Null,
        // Re-parsed strings (or any failure / structure) — fall back to the
        // raw expanded text so the env value reaches serde verbatim. Notably
        // this is what lets `password: ${PW}` with `PW=*foo` keep `*foo`
        // instead of failing to re-parse as a YAML alias.
        _ => Value::String(expanded),
    }
}

fn is_single_placeholder(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.len() < 3 || !trimmed.starts_with("${") || !trimmed.ends_with('}') {
        return false;
    }
    let body = &trimmed[2..trimmed.len() - 1];
    !body.contains('}') && !body.contains("${")
}

fn read_placeholder_body<I>(
    chars: &mut std::iter::Peekable<I>,
    path: &str,
) -> Result<String, EnvExpandError>
where
    I: Iterator<Item = char>,
{
    let mut body = String::new();
    for ch in chars.by_ref() {
        if ch == '}' {
            return Ok(body);
        }
        body.push(ch);
    }
    Err(EnvExpandError::InvalidSyntax {
        reason: "unterminated environment variable placeholder".to_string(),
        path: path.to_string(),
    })
}

fn split_placeholder_body<'a>(
    body: &'a str,
    path: &str,
) -> Result<(&'a str, Option<&'a str>), EnvExpandError> {
    let (name, default) = match body.find(":-") {
        Some(index) => (&body[..index], Some(&body[index + 2..])),
        None => (body, None),
    };

    if name.is_empty() {
        return Err(EnvExpandError::InvalidSyntax {
            reason: "empty environment variable name".to_string(),
            path: path.to_string(),
        });
    }

    Ok((name, default))
}

fn resolve_lookup_name<'a>(name: &'a str, path: &str) -> Result<(&'a str, bool), EnvExpandError> {
    let Some(explicit_name) = name.strip_prefix("env:") else {
        return Ok((name, false));
    };

    if explicit_name.is_empty() {
        return Err(EnvExpandError::InvalidSyntax {
            reason: "empty explicit environment variable name".to_string(),
            path: path.to_string(),
        });
    }

    Ok((explicit_name, true))
}

fn is_runtime_template_key(name: &str) -> bool {
    BUILTIN_KEYS.contains(&name.trim())
}

#[cfg(test)]
mod tests {
    use serde_yaml_ng::Number;

    use super::*;

    fn lookup(name: &str) -> Option<OsString> {
        match name {
            "A" => Some("alpha".into()),
            "B" => Some("beta".into()),
            "EMPTY" => Some(OsString::new()),
            "STAR_PW" => Some(OsString::from("*foo")),
            "TRICKY_PW" => Some(OsString::from("p@ss\"w*0rd!'\\n")),
            "ROS_PW" => Some(OsString::from("admin*123")),
            "NUM" => Some(OsString::from("30")),
            "TRUTH" => Some(OsString::from("true")),
            "NULLISH" => Some(OsString::from("null")),
            "WITH_NEWLINE" => Some(OsString::from("line1\nline2")),
            "TRUE_WITH_COMMENT" => Some(OsString::from("true # keep-this")),
            "NUM_WITH_COMMENT" => Some(OsString::from("30 #abc")),
            "BOOL_WITH_TRAILING_TEXT" => Some(OsString::from("true keep")),
            "JUST_HASH" => Some(OsString::from("#hash")),
            "PATHY" => Some(OsString::from("/etc/oxidns")),
            "SECTION" => Some(OsString::from("settings")),
            _ => None,
        }
    }

    fn run(yaml: &str) -> Result<Value, EnvExpandError> {
        let mut value: Value = serde_yaml_ng::from_str(yaml).expect("input must parse");
        expand_env_in_value_with_lookup(&mut value, &lookup)?;
        Ok(value)
    }

    fn as_str<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
        let mut cur = v;
        for seg in path {
            cur = cur.get(seg)?;
        }
        cur.as_str()
    }

    #[test]
    fn expands_plain_placeholder_in_string_value() {
        let v = run("key: ${A}").expect("expand");
        assert_eq!(as_str(&v, &["key"]), Some("alpha"));
    }

    #[test]
    fn expands_multiple_and_adjacent_placeholders() {
        let v = run("k: 'before ${A}/${B}:${A}${B} after'").expect("expand");
        assert_eq!(
            as_str(&v, &["k"]),
            Some("before alpha/beta:alphabeta after")
        );
    }

    #[test]
    fn supports_default_value_when_var_missing() {
        let v = run("k: ${MISSING:-fallback}").expect("expand");
        assert_eq!(as_str(&v, &["k"]), Some("fallback"));
    }

    #[test]
    fn expands_explicit_env_prefix() {
        let v = run("k: 'before ${env:A} after'").expect("expand");
        assert_eq!(as_str(&v, &["k"]), Some("before alpha after"));
    }

    #[test]
    fn explicit_env_can_target_runtime_keys() {
        let mut value: Value = serde_yaml_ng::from_str("site: ${env:qname}").unwrap();
        expand_env_in_value_with_lookup(&mut value, &|name| match name {
            "qname" => Some("from-env".into()),
            _ => lookup(name),
        })
        .expect("expand");
        assert_eq!(as_str(&value, &["site"]), Some("from-env"));
    }

    #[test]
    fn uses_default_when_env_value_is_empty() {
        let v = run("k: ${EMPTY:-fallback}").expect("expand");
        assert_eq!(as_str(&v, &["k"]), Some("fallback"));
    }

    #[test]
    fn empty_env_value_without_default_stays_empty() {
        let v = run("k: 'before${EMPTY}after'").expect("expand");
        assert_eq!(as_str(&v, &["k"]), Some("beforeafter"));
    }

    #[test]
    fn double_dollar_yields_literal_placeholder() {
        let v = run("k: $${LITERAL}").expect("expand");
        assert_eq!(as_str(&v, &["k"]), Some("${LITERAL}"));
    }

    #[test]
    fn keeps_runtime_template_placeholders_untouched() {
        let v = run("k: 'site=${qname} client=${client_ip}'").expect("expand");
        assert_eq!(
            as_str(&v, &["k"]),
            Some("site=${qname} client=${client_ip}")
        );
    }

    #[test]
    fn runtime_template_passthrough_for_every_builtin_key() {
        for key in BUILTIN_KEYS {
            let yaml = format!("k: 'value=${{{key}}}'");
            let v = run(&yaml).expect("expand");
            assert_eq!(
                as_str(&v, &["k"]),
                Some(format!("value=${{{key}}}").as_str()),
                "runtime placeholder {key} must survive"
            );
        }
    }

    #[test]
    fn runtime_template_default_form_still_uses_env() {
        let v = run("k: ${qname:-fallback}").expect("expand");
        assert_eq!(as_str(&v, &["k"]), Some("fallback"));
    }

    #[test]
    fn missing_variable_reports_path() {
        let err = run("plugins:\n  - args:\n      pw: ${MISSING}\n").expect_err("missing");
        let msg = err.to_string();
        assert!(msg.contains("MISSING"), "{msg}");
        assert!(
            msg.contains(".plugins[0].args.pw"),
            "expected YAML path in error, got {msg}"
        );
    }

    #[test]
    fn unterminated_placeholder_reports_path() {
        let err = run("k: '${A'").expect_err("unterminated");
        assert!(matches!(err, EnvExpandError::InvalidSyntax { .. }));
        let msg = err.to_string();
        assert!(msg.contains("unterminated"), "{msg}");
        assert!(msg.contains(".k"), "expected YAML path in error, got {msg}");
    }

    #[test]
    fn empty_name_reports_path() {
        let err = run("k: ${}").expect_err("empty name");
        assert!(matches!(err, EnvExpandError::InvalidSyntax { .. }));
        assert!(err.to_string().contains("empty environment variable name"));
    }

    #[test]
    fn empty_explicit_env_name_rejected() {
        let err = run("k: ${env:}").expect_err("empty explicit name");
        assert!(matches!(err, EnvExpandError::InvalidSyntax { .. }));
        assert!(
            err.to_string()
                .contains("empty explicit environment variable name")
        );
    }

    #[test]
    fn non_placeholder_dollars_pass_through() {
        let v = run("k: '$abc and a$b and trailing $'").expect("expand");
        assert_eq!(as_str(&v, &["k"]), Some("$abc and a$b and trailing $"));
    }

    #[test]
    fn double_dollars_outside_placeholder_escape_pass_through() {
        let v = run("k: cost=$$100").expect("expand");
        assert_eq!(as_str(&v, &["k"]), Some("cost=$$100"));
    }

    // ---- The real reason this module exists: env values with YAML-special
    // characters land untouched regardless of how the user wrote the YAML.

    /// Plain (unquoted) `${PW}` with `PW=*foo` used to break the YAML parser
    /// because `*foo` would have been substituted into the YAML text. Now we
    /// substitute in the parsed `Value`, so `*foo` reaches serde as a string.
    #[test]
    fn env_value_with_leading_star_is_safe_in_plain_scalar() {
        let v = run("password: ${STAR_PW}").expect("expand");
        assert_eq!(as_str(&v, &["password"]), Some("*foo"));
    }

    #[test]
    fn env_value_with_leading_star_is_safe_in_double_quoted_scalar() {
        let v = run(r#"password: "${STAR_PW}""#).expect("expand");
        assert_eq!(as_str(&v, &["password"]), Some("*foo"));
    }

    #[test]
    fn env_value_with_leading_star_is_safe_in_single_quoted_scalar() {
        let v = run("password: '${STAR_PW}'").expect("expand");
        assert_eq!(as_str(&v, &["password"]), Some("*foo"));
    }

    /// Every YAML-nasty character that used to break text substitution.
    #[test]
    fn env_value_with_arbitrary_special_chars_round_trips() {
        let v = run("password: ${TRICKY_PW}").expect("expand");
        assert_eq!(as_str(&v, &["password"]), Some("p@ss\"w*0rd!'\\n"));
    }

    /// The MikroTik connection string from the user report.
    #[test]
    fn env_value_inside_larger_literal_string_round_trips() {
        let v = run("addr: 'rest://admin:${ROS_PW}@router/'").expect("expand");
        assert_eq!(
            as_str(&v, &["addr"]),
            Some("rest://admin:admin*123@router/")
        );
    }

    /// Type recovery for numeric env values when the entire scalar is a
    /// placeholder — preserves `timeout: ${TIMEOUT}` style configs.
    #[test]
    fn whole_scalar_numeric_env_recovers_as_number() {
        let v = run("timeout: ${NUM}").expect("expand");
        assert_eq!(v.get("timeout"), Some(&Value::Number(Number::from(30u64))));
    }

    #[test]
    fn whole_scalar_bool_env_recovers_as_bool() {
        let v = run("enabled: ${TRUTH}").expect("expand");
        assert_eq!(v.get("enabled"), Some(&Value::Bool(true)));
    }

    #[test]
    fn whole_scalar_null_env_recovers_as_null() {
        let v = run("note: ${NULLISH}").expect("expand");
        assert_eq!(v.get("note"), Some(&Value::Null));
    }

    /// A numeric-looking env value embedded in a literal string must stay a
    /// string — type recovery is gated on "scalar is exactly one placeholder".
    #[test]
    fn numeric_env_inside_literal_text_stays_string() {
        let v = run("k: 'value=${NUM}'").expect("expand");
        assert_eq!(as_str(&v, &["k"]), Some("value=30"));
    }

    /// Env value with newline is opaque text — type recovery refuses to
    /// re-parse it as YAML so a multi-line secret never becomes a sequence.
    #[test]
    fn multiline_env_value_stays_string() {
        let v = run("note: ${WITH_NEWLINE}").expect("expand");
        assert_eq!(as_str(&v, &["note"]), Some("line1\nline2"));
    }

    /// Env value that happens to look like a YAML alias / anchor MUST stay
    /// string instead of triggering a re-parse failure or surprise structure.
    #[test]
    fn yaml_alias_looking_env_value_stays_string() {
        let v = run("password: ${STAR_PW}").expect("expand");
        assert_eq!(v.get("password"), Some(&Value::String("*foo".to_string())));
    }

    /// Regression for the type-recovery reviewer note: `PW=true # keep-this`
    /// must NOT recover as `Bool(true)` with the trailing comment silently
    /// dropped. YAML comment / structure semantics are excluded from type
    /// recovery — anything with internal whitespace or a `#` falls back to
    /// the verbatim string.
    #[test]
    fn env_value_with_yaml_comment_after_bool_stays_string() {
        let v = run("flag: ${TRUE_WITH_COMMENT}").expect("expand");
        assert_eq!(
            v.get("flag"),
            Some(&Value::String("true # keep-this".to_string()))
        );
    }

    #[test]
    fn env_value_with_yaml_comment_after_number_stays_string() {
        let v = run("token: ${NUM_WITH_COMMENT}").expect("expand");
        assert_eq!(v.get("token"), Some(&Value::String("30 #abc".to_string())));
    }

    /// A bare value followed by other tokens (no `#`) should also stay a
    /// string — re-parse would either fail or, worse, parse just the prefix.
    #[test]
    fn env_value_with_trailing_text_after_bool_stays_string() {
        let v = run("flag: ${BOOL_WITH_TRAILING_TEXT}").expect("expand");
        assert_eq!(v.get("flag"), Some(&Value::String("true keep".to_string())));
    }

    /// Env value that starts with `#` would re-parse as YAML null (the whole
    /// thing is a comment); must stay a string.
    #[test]
    fn env_value_that_is_only_comment_stays_string() {
        let v = run("note: ${JUST_HASH}").expect("expand");
        assert_eq!(v.get("note"), Some(&Value::String("#hash".to_string())));
    }

    /// Sequence walking.
    #[test]
    fn expands_inside_sequences() {
        let v = run("hosts:\n  - admin:${ROS_PW}@a\n  - admin:${ROS_PW}@b\n").expect("expand");
        let seq = v.get("hosts").and_then(|x| x.as_sequence()).unwrap();
        assert_eq!(seq[0].as_str(), Some("admin:admin*123@a"));
        assert_eq!(seq[1].as_str(), Some("admin:admin*123@b"));
    }

    /// Mapping keys also expand.
    #[test]
    fn expands_inside_mapping_keys() {
        let v = run("${SECTION}:\n  enabled: true\n").expect("expand");
        let map = v.as_mapping().unwrap();
        assert!(map.contains_key(Value::String("settings".to_string())));
        assert_eq!(
            map.get(Value::String("settings".to_string()))
                .and_then(|x| x.get("enabled"))
                .and_then(|x| x.as_bool()),
            Some(true)
        );
    }

    /// `include: ${PATHY}/extra.yaml` style still resolves: env value
    /// substitutes into the string scalar and reaches the include resolver as
    /// plain text.
    #[test]
    fn include_paths_expand_through_string_substitution() {
        let v = run("include:\n  - ${PATHY}/extra.yaml\n").expect("expand");
        let seq = v.get("include").and_then(|x| x.as_sequence()).unwrap();
        assert_eq!(seq[0].as_str(), Some("/etc/oxidns/extra.yaml"));
    }

    #[test]
    fn public_expand_reads_process_environment() {
        let mut v: Value = serde_yaml_ng::from_str("k: ${PATH}").unwrap();
        expand_env_in_value(&mut v).expect("PATH should expand");
        let expected = env::var_lossy("PATH").expect("PATH should exist in test environment");
        assert_eq!(as_str(&v, &["k"]), Some(expected.as_str()));
    }
}
