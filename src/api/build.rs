// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build capability API endpoint.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, StatusCode};
use serde::Serialize;

use crate::api::{ApiHandler, ApiRegister, json_error, json_ok};
use crate::build_info::BuildInfo;
use crate::core::error::Result;

#[derive(Debug, Serialize)]
struct BuildInfoResponse {
    ok: bool,
    build: BuildInfo,
}

#[derive(Debug)]
struct BuildInfoHandler;

#[async_trait]
impl ApiHandler for BuildInfoHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match crate::build_info::snapshot() {
            Ok(build) => json_ok(StatusCode::OK, &BuildInfoResponse { ok: true, build }),
            Err(err) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "build_info_unavailable",
                err.to_string(),
            ),
        }
    }
}

pub fn register_builtin_routes(register: &ApiRegister) -> Result<()> {
    register.register_get("/build", Arc::new(BuildInfoHandler))
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;

    use super::*;
    use crate::core::VERSION;

    #[tokio::test]
    async fn build_info_handler_reports_supported_plugins() {
        let response = BuildInfoHandler.handle(Request::new(Bytes::new())).await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).expect("body should be json");
        assert_eq!(value["ok"], true);
        assert_eq!(value["build"]["version"], VERSION);
        assert!(
            value["build"]["supported_plugins"]["executors"]
                .as_array()
                .expect("executors should be an array")
                .iter()
                .any(|value| value == "sequence")
        );
    }
}
