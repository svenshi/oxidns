// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP handlers for the upgrade check and apply endpoints.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Semaphore};
use tracing::{error, info};

use crate::api::{ApiHandler, ApiRegister, json_error, json_ok, json_response};
use crate::infra::error::Result;
use crate::infra::upgrade::{ApplyRunOutcome, UpgradeBundle, UpgradeConfig, UpgradeContext};

const EXIT_RESTART_REQUIRED: i32 = 75;

#[derive(Debug, Deserialize, Default)]
struct UpgradeApiBody {
    repository: Option<String>,
    bundle: Option<String>,
    outbound: Option<String>,
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
    config.outbound = opts.outbound.filter(|s| !s.trim().is_empty());
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

#[derive(Debug, Clone, Serialize)]
struct UpgradeStatusResponse {
    ok: bool,
    state: &'static str,
    started_at_ms: Option<u64>,
    completed_at_ms: Option<u64>,
    error: Option<String>,
    installed_version: Option<String>,
    restart_required: Option<bool>,
}

#[derive(Debug, Clone)]
struct UpgradeTaskStatus {
    state: UpgradeTaskState,
    started_at_ms: Option<u64>,
    completed_at_ms: Option<u64>,
    error: Option<String>,
    installed_version: Option<String>,
    restart_required: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpgradeTaskState {
    Idle,
    Running,
    Restarting,
    Completed,
    Skipped,
    Failed,
}

impl Default for UpgradeTaskStatus {
    fn default() -> Self {
        Self {
            state: UpgradeTaskState::Idle,
            started_at_ms: None,
            completed_at_ms: None,
            error: None,
            installed_version: None,
            restart_required: None,
        }
    }
}

impl UpgradeTaskStatus {
    fn running(now_ms: u64) -> Self {
        Self {
            state: UpgradeTaskState::Running,
            started_at_ms: Some(now_ms),
            ..Self::default()
        }
    }

    fn response(&self) -> UpgradeStatusResponse {
        UpgradeStatusResponse {
            ok: self.state != UpgradeTaskState::Failed,
            state: self.state.as_str(),
            started_at_ms: self.started_at_ms,
            completed_at_ms: self.completed_at_ms,
            error: self.error.clone(),
            installed_version: self.installed_version.clone(),
            restart_required: self.restart_required,
        }
    }
}

impl UpgradeTaskState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Restarting => "restarting",
            Self::Completed => "completed",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
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
    status: Arc<Mutex<UpgradeTaskStatus>>,
}

struct UpgradeStatusHandler {
    status: Arc<Mutex<UpgradeTaskStatus>>,
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

        {
            let mut status = self.status.lock().await;
            *status = UpgradeTaskStatus::running(now_ms());
        }

        let status = self.status.clone();
        tokio::spawn(async move {
            let _permit = permit;
            match crate::infra::upgrade::apply(&config, UpgradeContext::Plugin).await {
                Ok(ApplyRunOutcome::Applied { outcome, .. }) if outcome.restart_required => {
                    {
                        let mut status = status.lock().await;
                        status.state = UpgradeTaskState::Restarting;
                        status.installed_version = Some(outcome.installed_version.clone());
                        status.restart_required = Some(true);
                    }
                    info!("requesting app restart after API-triggered upgrade");
                    crate::plugin::request_app_restart()
                        .unwrap_or_else(|_| std::process::exit(EXIT_RESTART_REQUIRED));
                }
                Ok(ApplyRunOutcome::Applied { outcome, .. }) => {
                    let mut status = status.lock().await;
                    status.state = UpgradeTaskState::Completed;
                    status.completed_at_ms = Some(now_ms());
                    status.installed_version = Some(outcome.installed_version);
                    status.restart_required = Some(false);
                }
                Ok(ApplyRunOutcome::Skipped { check }) => {
                    let mut status = status.lock().await;
                    status.state = UpgradeTaskState::Skipped;
                    status.completed_at_ms = Some(now_ms());
                    status.installed_version = Some(check.current_version);
                    status.restart_required = Some(false);
                }
                Err(err) => {
                    error!(error = %err, "upgrade apply failed");
                    let mut status = status.lock().await;
                    status.state = UpgradeTaskState::Failed;
                    status.completed_at_ms = Some(now_ms());
                    status.error = Some(err.to_string());
                    status.restart_required = Some(false);
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

#[async_trait]
impl ApiHandler for UpgradeStatusHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        let status = self.status.lock().await;
        json_ok(StatusCode::OK, &status.response())
    }
}

pub fn register_upgrade_routes(register: &ApiRegister) -> Result<()> {
    let apply_semaphore = Arc::new(Semaphore::new(1));
    let status = Arc::new(Mutex::new(UpgradeTaskStatus::default()));
    register.register_post("/upgrade/check", Arc::new(UpgradeCheckHandler))?;
    register.register_get(
        "/upgrade/status",
        Arc::new(UpgradeStatusHandler {
            status: status.clone(),
        }),
    )?;
    register.register_post(
        "/upgrade/apply",
        Arc::new(UpgradeApplyHandler {
            apply_semaphore,
            status,
        }),
    )?;
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
