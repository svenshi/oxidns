// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Small helpers for parsing management API query strings.

use std::num::ParseIntError;

pub(crate) fn visit_query_params(
    query: Option<&str>,
    mut visit: impl FnMut(&str, &str) -> std::result::Result<(), String>,
) -> std::result::Result<(), String> {
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        visit(key.as_ref(), value.as_ref())?;
    }
    Ok(())
}

pub(crate) fn parse_usize_param(
    raw: &str,
    error: impl FnOnce(ParseIntError) -> String,
) -> std::result::Result<usize, String> {
    raw.parse::<usize>().map_err(error)
}

pub(crate) fn parse_u64_param(
    raw: &str,
    error: impl FnOnce(ParseIntError) -> String,
) -> std::result::Result<u64, String> {
    raw.parse::<u64>().map_err(error)
}

pub(crate) fn optional_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub(crate) fn optional_upper_text(raw: &str) -> Option<String> {
    optional_text(raw).map(|value| value.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visit_query_params_decodes_pairs() {
        let mut pairs = Vec::new();
        visit_query_params(Some("qname=EXAMPLE.COM.&empty=&space=a+b"), |key, value| {
            pairs.push((key.to_string(), value.to_string()));
            Ok(())
        })
        .expect("query should parse");

        assert_eq!(
            pairs,
            vec![
                ("qname".to_string(), "EXAMPLE.COM.".to_string()),
                ("empty".to_string(), String::new()),
                ("space".to_string(), "a b".to_string()),
            ]
        );
    }

    #[test]
    fn optional_text_trims_and_filters_empty() {
        assert_eq!(optional_text("  value  "), Some("value".to_string()));
        assert_eq!(optional_text("   "), None);
        assert_eq!(
            optional_upper_text(" noerror "),
            Some("NOERROR".to_string())
        );
    }
}
