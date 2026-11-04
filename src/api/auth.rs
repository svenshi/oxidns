// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Authentication helpers for the management API.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use http::HeaderMap;

use crate::config::types::ApiAuthConfig;

pub(crate) fn is_authorized(headers: &HeaderMap, auth: Option<&ApiAuthConfig>) -> bool {
    let Some(auth) = auth else {
        return true;
    };
    match auth {
        ApiAuthConfig::Basic { username, password } => {
            let Some(value) = headers.get(http::header::AUTHORIZATION) else {
                return false;
            };
            let Ok(value) = value.to_str() else {
                return false;
            };
            let Some((scheme, encoded)) = value.split_once(char::is_whitespace) else {
                return false;
            };
            if !scheme.eq_ignore_ascii_case("basic") {
                return false;
            }
            let Ok(decoded) = STANDARD.decode(encoded.trim()) else {
                return false;
            };
            let mut expected = Vec::with_capacity(username.len() + password.len() + 1);
            expected.extend_from_slice(username.as_bytes());
            expected.push(b':');
            expected.extend_from_slice(password.as_bytes());
            constant_time_eq(&decoded, &expected)
        }
    }
}

/// Byte-wise comparison without short-circuiting.
///
/// The combined user:pass blob is sensitive to prefix-matching timing
/// signals, so every byte is XOR-accumulated before the equality check.
/// Length differences return immediately; the length itself is not treated
/// as secret.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0_u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;

    fn basic_config() -> ApiAuthConfig {
        ApiAuthConfig::Basic {
            username: "admin".to_string(),
            password: "secret".to_string(),
        }
    }

    fn headers_with(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_str(value).unwrap(),
        );
        headers
    }

    #[test]
    fn accepts_correct_credentials_and_scheme_any_case() {
        let encoded = STANDARD.encode("admin:secret");
        assert!(is_authorized(
            &headers_with(&format!("Basic {encoded}")),
            Some(&basic_config())
        ));
        assert!(is_authorized(
            &headers_with(&format!("basic {encoded}")),
            Some(&basic_config())
        ));
    }

    #[test]
    fn rejects_wrong_password_missing_or_malformed_header() {
        let encoded = STANDARD.encode("admin:wrong");
        assert!(!is_authorized(
            &headers_with(&format!("Basic {encoded}")),
            Some(&basic_config())
        ));
        assert!(!is_authorized(&HeaderMap::new(), Some(&basic_config())));
        assert!(!is_authorized(
            &headers_with("Bearer anything"),
            Some(&basic_config())
        ));
        assert!(!is_authorized(
            &headers_with("Basic !!!not-base64!!!"),
            Some(&basic_config())
        ));
    }

    #[test]
    fn no_auth_config_always_authorizes() {
        assert!(is_authorized(&HeaderMap::new(), None));
    }

    #[test]
    fn constant_time_eq_behaves_like_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"", b""));
    }
}
