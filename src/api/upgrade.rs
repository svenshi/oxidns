// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP handlers for the upgrade check and apply endpoints.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tracing::error;

use crate::api::{ApiHandler, ApiRegister, json_error, json_ok, json_response};
use crate::infra::error::Result;
use crate::infra::upgrade::{UpgradeBundle, UpgradeConfig, UpgradeContext};

#[derive(Debug, Deserialize, Default)]
struct UpgradeApiBody {
    repository: Option<String>,
    bundle: Option<String>,
    socks5: Option<String>,
    allow_prerelease: Option<bool>,
    target: Option<String>,
    github_token: Option<String>,
}

fn build_upgrade_config(opts: UpgradeApiBody) -> std::result::Result<UpgradeConfig, String> {
    let mut config = UpgradeConfig::default();
    if let Some(repo) = opts.repository.filter(|s| !s.trim().is_empty()) {
        config.repository = repo;
    }
    if let Some(bundle_str) = opts.bundle.filter(|s| !s.trim().is_empty()) {
        config.bundle = UpgradeBundle::from_user_value(&bundle_str).map_err(|e| e.to_string())?;
    }
    config.socks5 = opts.socks5.filter(|s| !s.trim().is_empty());
    if let Some(allow_prerelease) = opts.allow_prerelease {
        config.allow_prerelease = allow_prerelease;
    }
    if let Some(target) = opts.target.filter(|s| !s.trim().is_empty()) {
        config.target = target;
    }
    config.github_token = opts.github_token.filter(|s| !s.trim().is_empty());
    Ok(config)
}

#[derive(Debug, Serialize)]
struct UpgradeCheckResponse {
    ok: bool,
    current_version: String,
    latest_version: String,
    update_available: bool,
    asset_name: String,
    release_url: String,
}

#[derive(Debug, Serialize)]
struct UpgradeApplyResponse {
    ok: bool,
    action: &'static str,
    status: &'static str,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct UpgradeBusyResponse {
    ok: bool,
    code: &'static str,
    message: &'static str,
}

struct UpgradeCheckHandler;

#[async_trait]
impl ApiHandler for UpgradeCheckHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let opts = if request.body().is_empty() {
            UpgradeApiBody::default()
        } else {
            match serde_json::from_slice::<UpgradeApiBody>(request.body()) {
                Ok(b) => b,
                Err(err) => {
                    return json_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        format!("invalid request body: {err}"),
                    );
                }
            }
        };

        let config = match build_upgrade_config(opts) {
            Ok(c) => c,
            Err(err) => {
                return json_error(StatusCode::BAD_REQUEST, "invalid_upgrade_config", err);
            }
        };

        match crate::infra::upgrade::check(&config).await {
            Ok(check) => json_ok(
                StatusCode::OK,
                &UpgradeCheckResponse {
                    ok: true,
                    current_version: check.current_version,
                    latest_version: check.latest_version,
                    update_available: check.update_available,
                    asset_name: check.asset_name,
                    release_url: check.release_url,
                },
            ),
            Err(err) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "upgrade_check_failed",
                err.to_string(),
            ),
        }
    }
}

struct UpgradeApplyHandler {
    apply_semaphore: Arc<Semaphore>,
}

#[async_trait]
impl ApiHandler for UpgradeApplyHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let opts = if request.body().is_empty() {
            UpgradeApiBody::default()
        } else {
            match serde_json::from_slice::<UpgradeApiBody>(request.body()) {
                Ok(b) => b,
                Err(err) => {
                    return json_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        format!("invalid request body: {err}"),
                    );
                }
            }
        };

        let config = match build_upgrade_config(opts) {
            Ok(c) => c,
            Err(err) => {
                return json_error(StatusCode::BAD_REQUEST, "invalid_upgrade_config", err);
            }
        };

        let permit = match self.apply_semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                return json_response(
                    StatusCode::CONFLICT,
                    &UpgradeBusyResponse {
                        ok: false,
                        code: "upgrade_in_progress",
                        message: "an upgrade is already in progress",
                    },
                );
            }
        };

        tokio::spawn(async move {
            let _permit = permit;
            match crate::infra::upgrade::apply(&config, UpgradeContext::Plugin).await {
                Ok(_) => {}
                Err(err) => {
                    error!(error = %err, "upgrade apply failed");
                }
            }
        });

        json_ok(
            StatusCode::ACCEPTED,
            &UpgradeApplyResponse {
                ok: true,
                action: "upgrade_apply",
                status: "accepted",
                message: "upgrade started; server will restart when complete",
            },
        )
    }
}

pub fn register_upgrade_routes(register: &ApiRegister) -> Result<()> {
    let apply_semaphore = Arc::new(Semaphore::new(1));
    register.register_post("/upgrade/check", Arc::new(UpgradeCheckHandler))?;
    register.register_post(
        "/upgrade/apply",
        Arc::new(UpgradeApplyHandler { apply_semaphore }),
    )?;
    Ok(())
}
