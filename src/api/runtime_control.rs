// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Domain-specific runtime controls exposed through the management API.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, StatusCode};
use serde::{Deserialize, Serialize};

use crate::api::{ApiHandler, ApiRegister, json_error, json_ok};
use crate::infra::error::Result as DnsResult;
use crate::plugin::matcher::MatcherRuntimeMode;
use crate::plugin::provider::{ProviderReloadError, ProviderRuntimeControl};
use crate::plugin::runtime_control::PluginRuntimeControl;
use crate::plugin::{PluginRuntime, current_runtime};

#[derive(Debug, Serialize)]
struct MatcherStatusResponse {
    ok: bool,
    matcher: String,
    mode: MatcherRuntimeMode,
}

#[derive(Debug)]
struct MatcherStatusHandler {
    tag: String,
}

#[derive(Debug, Deserialize)]
struct MatcherModeRequest {
    mode: MatcherRuntimeMode,
}

#[derive(Debug)]
struct MatcherModeHandler {
    tag: String,
}

fn live_runtime_control(tag: &str) -> Option<PluginRuntimeControl> {
    current_runtime()?.get_plugin(tag)?.runtime_control()
}

fn runtime_control_unavailable(tag: &str, kind: &str) -> crate::api::ApiResponse {
    json_error(
        StatusCode::NOT_FOUND,
        "plugin_runtime_control_unavailable",
        format!("{} runtime control '{}' is not available", kind, tag),
    )
}

#[async_trait]
impl ApiHandler for MatcherStatusHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        let Some(PluginRuntimeControl::Matcher(control)) = live_runtime_control(&self.tag) else {
            return runtime_control_unavailable(&self.tag, "matcher");
        };
        json_ok(
            StatusCode::OK,
            &MatcherStatusResponse {
                ok: true,
                matcher: self.tag.clone(),
                mode: control.mode(),
            },
        )
    }
}

#[async_trait]
impl ApiHandler for MatcherModeHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let desired = match serde_json::from_slice::<MatcherModeRequest>(request.body()) {
            Ok(request) => request.mode,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_matcher_runtime_mode",
                    format!(
                        "request body must contain mode normal, always_false, or always_true: {err}"
                    ),
                );
            }
        };
        let Some(PluginRuntimeControl::Matcher(control)) = live_runtime_control(&self.tag) else {
            return runtime_control_unavailable(&self.tag, "matcher");
        };
        control.set_mode(desired);
        tracing::info!(
            matcher = %self.tag,
            mode = ?desired,
            "matcher runtime control updated"
        );
        json_ok(
            StatusCode::OK,
            &MatcherStatusResponse {
                ok: true,
                matcher: self.tag.clone(),
                mode: control.mode(),
            },
        )
    }
}

#[derive(Debug, Serialize)]
struct ProviderReloadResponse {
    ok: bool,
    action: &'static str,
    provider: String,
    status: &'static str,
}

#[derive(Debug)]
struct ProviderReloadHandler {
    tag: String,
}

#[derive(Debug)]
struct ProviderStatusHandler {
    tag: String,
}

#[async_trait]
impl ApiHandler for ProviderStatusHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        let Some(PluginRuntimeControl::Provider(control)) = live_runtime_control(&self.tag) else {
            return runtime_control_unavailable(&self.tag, "provider");
        };
        json_ok(StatusCode::OK, &control.status())
    }
}

#[async_trait]
impl ApiHandler for ProviderReloadHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        let Some(PluginRuntimeControl::Provider(control)) = live_runtime_control(&self.tag) else {
            return runtime_control_unavailable(&self.tag, "provider");
        };
        reload_provider_response(&self.tag, &control).await
    }
}

async fn reload_provider_response(
    tag: &str,
    control: &ProviderRuntimeControl,
) -> crate::api::ApiResponse {
    match control.reload().await {
        Ok(()) => json_ok(
            StatusCode::OK,
            &ProviderReloadResponse {
                ok: true,
                action: "reload_provider",
                provider: tag.to_string(),
                status: "reloaded",
            },
        ),
        Err(error @ ProviderReloadError::Busy { .. }) => json_error(
            StatusCode::CONFLICT,
            "provider_reload_busy",
            error.to_string(),
        ),
        Err(ProviderReloadError::Failed(error)) => json_error(
            StatusCode::BAD_REQUEST,
            "provider_reload_failed",
            error.to_string(),
        ),
    }
}

pub(crate) fn register_plugin_runtime_control_routes(
    register: &ApiRegister,
    runtime: &PluginRuntime,
) -> DnsResult<()> {
    for (tag, control) in runtime.runtime_controls() {
        let plugin = register.plugin(&tag)?;
        match control {
            PluginRuntimeControl::Matcher(_) => {
                plugin.get(
                    "/status",
                    Arc::new(MatcherStatusHandler { tag: tag.clone() }),
                )?;
                plugin.post("/mode", Arc::new(MatcherModeHandler { tag }))?;
            }
            PluginRuntimeControl::Provider(_) => {
                plugin.get(
                    "/status",
                    Arc::new(ProviderStatusHandler { tag: tag.clone() }),
                )?;
                plugin.post("/reload", Arc::new(ProviderReloadHandler { tag }))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use http_body_util::BodyExt;
    use tokio::sync::Notify;

    use super::*;
    use crate::plugin::Plugin;
    use crate::plugin::provider::Provider;

    #[derive(Debug)]
    struct BlockingProvider {
        started: Notify,
        release: Notify,
    }

    #[async_trait]
    impl Plugin for BlockingProvider {
        fn tag(&self) -> &str {
            "blocking"
        }
    }

    #[async_trait]
    impl Provider for BlockingProvider {
        fn as_any(&self) -> &dyn Any {
            self
        }

        async fn reload(&self) -> DnsResult<()> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn provider_handler_maps_concurrent_reload_to_conflict() {
        let provider = Arc::new(BlockingProvider {
            started: Notify::new(),
            release: Notify::new(),
        });
        let control = Arc::new(ProviderRuntimeControl::new(provider.clone()));
        let first_control = control.clone();
        let first = tokio::spawn(async move { first_control.reload().await });
        provider.started.notified().await;

        let response = reload_provider_response("blocking", &control).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("response body should collect")
            .to_bytes();
        let payload = serde_json::from_slice::<serde_json::Value>(&body)
            .expect("response should be valid json");
        assert_eq!(payload["code"], "provider_reload_busy");

        provider.release.notify_one();
        first
            .await
            .expect("first reload task should finish")
            .unwrap();
    }
}
