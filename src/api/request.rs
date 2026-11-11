// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Request body collection and `/api` path normalization.

use bytes::Bytes;
use http::{Request, StatusCode, Uri};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use tracing::warn;

/// Upper bound for collected management API request bodies.
///
/// Runs before authentication, so it must not buffer untrusted data without a
/// cap. Generous compared to the DNS-plane DoH limit because `PUT /api/config`
/// carries the full YAML configuration.
const MAX_API_BODY: usize = 4 * 1024 * 1024;

pub(crate) fn strip_api_prefix(path: &str) -> Option<String> {
    if path == "/api" {
        return Some("/".to_string());
    }
    path.strip_prefix("/api/").map(|path| format!("/{path}"))
}

pub(crate) fn rewrite_request_path(
    request: Request<Bytes>,
    path: &str,
) -> std::result::Result<Request<Bytes>, ()> {
    let query = request.uri().query().map(str::to_string);
    let (mut parts, body) = request.into_parts();
    let path_and_query = match query {
        Some(query) => format!("{path}?{query}"),
        None => path.to_string(),
    };
    parts.uri = path_and_query.parse::<Uri>().map_err(|_| ())?;
    Ok(Request::from_parts(parts, body))
}

pub(super) async fn read_hyper_request(
    request: Request<Incoming>,
) -> std::result::Result<Request<Bytes>, StatusCode> {
    let (parts, mut body) = request.into_parts();
    let mut collected = Vec::with_capacity(2048);

    while let Some(frame_result) = body.frame().await {
        let frame = frame_result.map_err(|_| StatusCode::BAD_REQUEST)?;
        if let Ok(data) = frame.into_data() {
            if collected.len() + data.len() > MAX_API_BODY {
                warn!(
                    path = parts.uri.path(),
                    collected = collected.len(),
                    chunk = data.len(),
                    "API request body exceeds limit"
                );
                return Err(StatusCode::PAYLOAD_TOO_LARGE);
            }
            collected.extend_from_slice(&data);
        }
    }

    Ok(Request::from_parts(parts, Bytes::from(collected)))
}
