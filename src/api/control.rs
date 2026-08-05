// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP adapter around the always-on [`crate::infra::control`] state.
//!
//! This module only contains the HTTP handlers, request / response shapes,
//! and validation glue that the management API needs. All shared state and
//! the always-on `AppController` itself live in
//! [`crate::infra::control`].

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use bytes::Bytes;
use http::{Method, Request, StatusCode};
use serde::{Deserialize, Serialize};

use crate::api::{ApiHandler, ApiRegister, json_error, json_ok, json_response};
use crate::build_info::BuildInfo;
use crate::config;
use crate::infra::VERSION;
use crate::infra::config_transaction::{self as config_tx, BeginError};
use crate::infra::control::{
    AppController, ControlRequestError, ProcessMemoryKind, ReloadSnapshot, config_version,
};
use crate::infra::error::Result;

#[derive(Debug, Serialize)]
struct ActionAcceptedResponse {
    ok: bool,
    action: &'static str,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ConfigCheckResponse {
    ok: bool,
    source: &'static str,
    path: Option<String>,
    plugin_count: usize,
    dependency_graph: crate::plugin::DependencyGraphReport,
    version: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct ConfigFileResponse {
    ok: bool,
    path: String,
    format: &'static str,
    content: String,
    version: String,
    updated_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ConfigSaveRequest {
    format: Option<String>,
    content: String,
    base_version: Option<String>,
    validate: Option<bool>,
    reload: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ConfigSaveResponse {
    ok: bool,
    path: String,
    format: &'static str,
    version: String,
    updated_at_ms: Option<u64>,
    plugin_count: usize,
    init_order: Vec<String>,
    reload: Option<ReloadSnapshot>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct ConfigApplyRequest {
    format: Option<String>,
    content: String,
    base_version: String,
    candidate_version: String,
}

#[derive(Debug, Serialize)]
struct ConfigApplyResponse {
    ok: bool,
    status: &'static str,
    transaction_id: String,
    previous_config_version: String,
    candidate_config_version: String,
}

#[derive(Debug, Serialize)]
struct ConfigApplyStatusResponse {
    ok: bool,
    transaction: Option<config_tx::TransactionRecord>,
}

#[derive(Debug, Serialize)]
struct ConfigHistoryItem {
    id: String,
    created_at_ms: u64,
    config_version: String,
    content_bytes: usize,
}

#[derive(Debug, Serialize)]
struct ConfigHistoryResponse {
    ok: bool,
    entries: Vec<ConfigHistoryItem>,
}

#[derive(Debug, Deserialize)]
struct ConfigHistoryRestoreRequest {
    id: String,
}

#[derive(Debug, Serialize)]
struct ConfigHistoryRestoreResponse {
    ok: bool,
    id: String,
    format: &'static str,
    content: String,
    version: String,
}

#[derive(Debug, Serialize)]
struct SystemResponse {
    ok: bool,
    version: &'static str,
    build: BuildInfo,
    os: &'static str,
    arch: &'static str,
    uptime_ms: u64,
    config_path: String,
    api_enabled: bool,
    reload: ReloadSnapshot,
    process_cpu_percent: f32,
    process_memory_mb: u64,
    process_memory_kind: ProcessMemoryKind,
    system_memory_total_mb: u64,
}

#[derive(Debug, Serialize)]
struct ConfigValidationErrorResponse {
    ok: bool,
    code: &'static str,
    message: String,
    diagnostics: Vec<String>,
    diagnostic_details: Vec<ConfigDiagnostic>,
}

#[derive(Debug, Serialize)]
struct ConfigDiagnostic {
    message: String,
    severity: &'static str,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
}

#[derive(Debug)]
struct ControlHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ControlHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        json_ok(StatusCode::OK, &self.controller.snapshot())
    }
}

#[derive(Debug)]
struct ShutdownHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ShutdownHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match self.controller.request_shutdown() {
            Ok(()) => json_ok(
                StatusCode::ACCEPTED,
                &ActionAcceptedResponse {
                    ok: true,
                    action: "shutdown",
                    status: "accepted",
                },
            ),
            Err(err) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "control_command_failed",
                err.to_string(),
            ),
        }
    }
}

#[derive(Debug)]
struct RestartHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for RestartHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match self.controller.request_restart() {
            Ok(()) => json_ok(
                StatusCode::ACCEPTED,
                &ActionAcceptedResponse {
                    ok: true,
                    action: "restart",
                    status: "accepted",
                },
            ),
            Err(err) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "control_command_failed",
                err.to_string(),
            ),
        }
    }
}

#[derive(Debug)]
struct ReloadHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ReloadHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match self.controller.request_reload() {
            Ok(()) => json_ok(
                StatusCode::ACCEPTED,
                &ActionAcceptedResponse {
                    ok: true,
                    action: "reload",
                    status: "accepted",
                },
            ),
            Err(ControlRequestError::ReloadBusy) => json_error(
                StatusCode::CONFLICT,
                "reload_busy",
                "reload is already pending or in progress",
            ),
            Err(err) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "control_command_failed",
                err.to_string(),
            ),
        }
    }
}

#[derive(Debug)]
struct ReloadStatusHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct SystemHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ReloadStatusHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        json_ok(StatusCode::OK, &self.controller.reload_snapshot())
    }
}

#[async_trait]
impl ApiHandler for SystemHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        let snapshot = self.controller.snapshot();
        let metrics = self.controller.sample_process_metrics();
        let build = match crate::build_info::snapshot() {
            Ok(build) => build,
            Err(err) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "build_info_unavailable",
                    err.to_string(),
                );
            }
        };
        json_ok(
            StatusCode::OK,
            &SystemResponse {
                ok: true,
                version: VERSION,
                build,
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
                uptime_ms: snapshot.uptime_ms,
                config_path: snapshot.config_path,
                api_enabled: true,
                reload: snapshot.reload,
                process_cpu_percent: metrics.cpu_percent,
                process_memory_mb: metrics.memory_mb,
                process_memory_kind: metrics.memory_kind,
                system_memory_total_mb: metrics.system_memory_total_mb,
            },
        )
    }
}

#[derive(Debug)]
struct ConfigCheckHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ConfigCheckHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match validate_config_file(self.controller.config_path()) {
            Ok(response) => json_ok(StatusCode::OK, &response),
            Err(err) => config_validation_error("config_check_failed", err, None),
        }
    }
}

#[derive(Debug)]
struct ConfigGetHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ConfigGetHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match read_config_file_response(self.controller.config_path()) {
            Ok(response) => json_ok(StatusCode::OK, &response),
            Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, "config_read_failed", err),
        }
    }
}

#[derive(Debug)]
struct ConfigSaveHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ConfigSaveHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let save_request = match serde_json::from_slice::<ConfigSaveRequest>(request.body()) {
            Ok(request) => request,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_config_save_request",
                    format!("request body must be JSON: {err}"),
                );
            }
        };

        match save_config_file(self.controller.clone(), save_request) {
            Ok(response) => json_ok(StatusCode::OK, &response),
            Err(ConfigSaveError::InvalidFormat(format)) => json_error(
                StatusCode::BAD_REQUEST,
                "unsupported_config_format",
                format!("unsupported config format: {format}"),
            ),
            Err(ConfigSaveError::VersionConflict) => json_error(
                StatusCode::CONFLICT,
                "config_conflict",
                "configuration file changed since it was loaded",
            ),
            Err(ConfigSaveError::Validation(message)) => {
                json_error(StatusCode::BAD_REQUEST, "config_validate_failed", message)
            }
            Err(ConfigSaveError::ReloadBusy) => json_error(
                StatusCode::CONFLICT,
                "reload_busy",
                "configuration was not changed because reload is already pending or in progress",
            ),
            Err(ConfigSaveError::TransactionBusy) => json_error(
                StatusCode::CONFLICT,
                "config_transaction_busy",
                "a configuration transaction is pending",
            ),
            Err(ConfigSaveError::Io(message) | ConfigSaveError::Reload(message)) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "config_save_failed",
                message,
            ),
        }
    }
}

#[derive(Debug)]
struct ConfigValidateHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ConfigValidateHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let body = match config_text_from_validate_request(request.body()) {
            Ok(body) => body,
            Err(err) => return err.into_response(),
        };

        match validate_config_text(self.controller.config_path(), &body) {
            Ok(response) => json_ok(StatusCode::OK, &response),
            Err(err) => config_validation_error("config_validate_failed", err, Some(&body)),
        }
    }
}

#[derive(Debug)]
struct ConfigApplyHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ConfigApplyHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let request = match serde_json::from_slice::<ConfigApplyRequest>(request.body()) {
            Ok(request) => request,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_config_apply_request",
                    format!("request body must be JSON: {err}"),
                );
            }
        };
        match prepare_config_apply(self.controller.clone(), request) {
            Ok(response) => json_ok(StatusCode::ACCEPTED, &response),
            Err(error) => config_apply_error(error),
        }
    }
}

#[derive(Debug)]
struct ConfigApplyStatusHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ConfigApplyStatusHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match config_tx::status(self.controller.config_path()) {
            Ok(transaction) => json_ok(
                StatusCode::OK,
                &ConfigApplyStatusResponse {
                    ok: true,
                    transaction,
                },
            ),
            Err(message) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "config_transaction_read_failed",
                message,
            ),
        }
    }
}

#[derive(Debug)]
struct ConfigHistoryHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ConfigHistoryHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match config_tx::history(self.controller.config_path()) {
            Ok(entries) => json_ok(
                StatusCode::OK,
                &ConfigHistoryResponse {
                    ok: true,
                    entries: entries
                        .into_iter()
                        .map(|entry| ConfigHistoryItem {
                            id: entry.id,
                            created_at_ms: entry.created_at_ms,
                            config_version: entry.config_version,
                            content_bytes: entry.content.len(),
                        })
                        .collect(),
                },
            ),
            Err(message) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "config_history_read_failed",
                message,
            ),
        }
    }
}

#[derive(Debug)]
struct ConfigHistoryRestoreHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for ConfigHistoryRestoreHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let request = match serde_json::from_slice::<ConfigHistoryRestoreRequest>(request.body()) {
            Ok(request) => request,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_config_history_request",
                    format!("request body must be JSON: {err}"),
                );
            }
        };
        match config_tx::history_entry(self.controller.config_path(), &request.id) {
            Ok(Some(entry)) => json_ok(
                StatusCode::OK,
                &ConfigHistoryRestoreResponse {
                    ok: true,
                    id: entry.id,
                    format: "yaml",
                    version: entry.config_version,
                    content: entry.content,
                },
            ),
            Ok(None) => json_error(
                StatusCode::NOT_FOUND,
                "config_history_not_found",
                "configuration history entry was not found",
            ),
            Err(message) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "config_history_read_failed",
                message,
            ),
        }
    }
}

fn config_validation_error(
    code: &'static str,
    message: String,
    config_text: Option<&str>,
) -> crate::api::ApiResponse {
    let diagnostic = config_text
        .map(|text| locate_config_diagnostic(text, &message))
        .unwrap_or_else(|| default_config_diagnostic(&message));
    json_response(
        StatusCode::BAD_REQUEST,
        &ConfigValidationErrorResponse {
            ok: false,
            diagnostics: vec![message.clone()],
            diagnostic_details: vec![diagnostic],
            code,
            message,
        },
    )
}

fn default_config_diagnostic(message: &str) -> ConfigDiagnostic {
    ConfigDiagnostic {
        message: message.to_string(),
        severity: "error",
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 2,
    }
}

fn locate_config_diagnostic(config_text: &str, message: &str) -> ConfigDiagnostic {
    if let Some(loc) = config::diagnostic::locate_in_config(config_text, message) {
        return ConfigDiagnostic {
            message: message.to_string(),
            severity: "error",
            line: loc.line,
            column: loc.column,
            end_line: loc.line,
            end_column: loc.end_column,
        };
    }
    default_config_diagnostic(message)
}

fn config_text_from_validate_request(
    body: &Bytes,
) -> std::result::Result<String, ConfigValidateRequestError> {
    if let Ok(request) = serde_json::from_slice::<ConfigSaveRequest>(body) {
        if request.format.as_deref().unwrap_or("yaml") != "yaml" {
            return Err(ConfigValidateRequestError::UnsupportedFormat);
        }
        return Ok(request.content);
    }

    let body = match std::str::from_utf8(body) {
        Ok(body) if !body.trim().is_empty() => body,
        Ok(_) => {
            return Err(ConfigValidateRequestError::EmptyBody);
        }
        Err(err) => {
            return Err(ConfigValidateRequestError::InvalidUtf8(err.to_string()));
        }
    };

    Ok(body.to_string())
}

#[derive(Debug)]
enum ConfigValidateRequestError {
    UnsupportedFormat,
    EmptyBody,
    InvalidUtf8(String),
}

impl ConfigValidateRequestError {
    fn into_response(self) -> crate::api::ApiResponse {
        match self {
            Self::UnsupportedFormat => json_error(
                StatusCode::BAD_REQUEST,
                "unsupported_config_format",
                "request format must be yaml",
            ),
            Self::EmptyBody => json_error(
                StatusCode::BAD_REQUEST,
                "empty_config_body",
                "request body must contain YAML configuration",
            ),
            Self::InvalidUtf8(message) => json_error(
                StatusCode::BAD_REQUEST,
                "invalid_utf8_body",
                format!("request body is not valid UTF-8: {message}"),
            ),
        }
    }
}

#[derive(Debug)]
enum ConfigSaveError {
    InvalidFormat(String),
    VersionConflict,
    Validation(String),
    Io(String),
    ReloadBusy,
    TransactionBusy,
    Reload(String),
}

fn validate_config_file(path: &Path) -> std::result::Result<ConfigCheckResponse, String> {
    let summary = config::validate_file(path).map_err(|err| err.to_string())?;
    Ok(ConfigCheckResponse {
        ok: true,
        source: "file",
        path: Some(path.display().to_string()),
        plugin_count: summary.plugin_count,
        dependency_graph: summary.dependency_graph,
        version: fs::read_to_string(path)
            .map(|content| config_version(&content))
            .map_err(|err| format!("failed to read config {}: {err}", path.display()))?,
        message: "configuration is valid".to_string(),
    })
}

fn validate_config_text(
    config_path: &Path,
    text: &str,
) -> std::result::Result<ConfigCheckResponse, String> {
    let summary = config_tx::validate_candidate(config_path, text)?;
    Ok(ConfigCheckResponse {
        ok: true,
        source: "body",
        path: Some(config_path.display().to_string()),
        plugin_count: summary.plugin_count,
        dependency_graph: summary.dependency_graph,
        version: config_version(text),
        message: "configuration is valid".to_string(),
    })
}

fn read_config_file_response(path: &Path) -> std::result::Result<ConfigFileResponse, String> {
    let content = fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    let updated_at_ms = config_updated_at_ms(path);
    Ok(ConfigFileResponse {
        ok: true,
        path: path.display().to_string(),
        format: "yaml",
        version: config_version(&content),
        updated_at_ms,
        content,
    })
}

fn save_config_file(
    controller: Arc<AppController>,
    request: ConfigSaveRequest,
) -> std::result::Result<ConfigSaveResponse, ConfigSaveError> {
    let format = request.format.as_deref().unwrap_or("yaml");
    if format != "yaml" {
        return Err(ConfigSaveError::InvalidFormat(format.to_string()));
    }

    let path = controller.config_path();
    let current = fs::read_to_string(path).map_err(|err| {
        ConfigSaveError::Io(format!("failed to read config {}: {err}", path.display()))
    })?;
    if let Some(base_version) = request.base_version.as_deref()
        && base_version != config_version(&current)
    {
        return Err(ConfigSaveError::VersionConflict);
    }

    let summary = if request.validate.unwrap_or(true) {
        config_tx::validate_candidate(path, &request.content)
            .map_err(ConfigSaveError::Validation)?
    } else {
        let parsed: crate::config::types::Config = serde_yaml_ng::from_str(&request.content)
            .map_err(|err| ConfigSaveError::Validation(err.to_string()))?;
        crate::config::ConfigValidationSummary {
            plugin_count: parsed.plugins.len(),
            dependency_graph: crate::plugin::analyze_configuration(&parsed)
                .map_err(|err| ConfigSaveError::Validation(err.to_string()))?,
        }
    };

    if request.reload.unwrap_or(false) {
        let (healthy, _) = controller
            .healthy_config()
            .unwrap_or_else(|| (current.clone(), config_version(&current)));
        let base_version = request
            .base_version
            .clone()
            .unwrap_or_else(|| config_version(&current));
        let candidate_version = config_version(&request.content);
        begin_apply(
            controller.clone(),
            &healthy,
            &request.content,
            &base_version,
            &candidate_version,
        )?;
    } else {
        let _guard = config_tx::mutation_guard().map_err(ConfigSaveError::Io)?;
        if config_tx::has_pending(path) {
            return Err(ConfigSaveError::TransactionBusy);
        }
        // Re-check the version while holding the shared writer lock.
        let locked_current = fs::read_to_string(path).map_err(|err| {
            ConfigSaveError::Io(format!("failed to read config {}: {err}", path.display()))
        })?;
        if request
            .base_version
            .as_deref()
            .is_some_and(|base| base != config_version(&locked_current))
        {
            return Err(ConfigSaveError::VersionConflict);
        }
        config_tx::atomic_write_config(path, &request.content).map_err(ConfigSaveError::Io)?;
    }

    let saved = fs::read_to_string(path).map_err(|err| {
        ConfigSaveError::Io(format!(
            "failed to read saved config {}: {err}",
            path.display()
        ))
    })?;

    let reload = if request.reload.unwrap_or(false) {
        Some(controller.reload_snapshot())
    } else {
        None
    };

    Ok(ConfigSaveResponse {
        ok: true,
        path: path.display().to_string(),
        format: "yaml",
        version: config_version(&saved),
        updated_at_ms: config_updated_at_ms(path),
        plugin_count: summary.plugin_count,
        init_order: summary.dependency_graph.init_order,
        reload,
        message: "configuration saved".to_string(),
    })
}

fn prepare_config_apply(
    controller: Arc<AppController>,
    request: ConfigApplyRequest,
) -> std::result::Result<ConfigApplyResponse, BeginError> {
    if request.format.as_deref().unwrap_or("yaml") != "yaml" {
        return Err(BeginError::Invalid(
            "request format must be yaml".to_string(),
        ));
    }
    let reload = controller.reload_snapshot();
    if reload.pending || reload.in_progress {
        return Err(BeginError::Busy);
    }
    let current = fs::read_to_string(controller.config_path()).map_err(|err| {
        BeginError::Io(format!(
            "failed to read config {}: {err}",
            controller.config_path().display()
        ))
    })?;
    let (healthy, _) = controller
        .healthy_config()
        .unwrap_or_else(|| (current, request.base_version.clone()));
    let record = config_tx::begin(
        controller.config_path(),
        &healthy,
        &request.content,
        &request.base_version,
        &request.candidate_version,
    )?;
    if let Err(error) = controller.request_reload() {
        let message = error.to_string();
        config_tx::rollback(controller.config_path(), &message)
            .map_err(|rollback| BeginError::Io(rollback.to_string()))?;
        return Err(match error {
            ControlRequestError::ReloadBusy => BeginError::Busy,
            ControlRequestError::CommandChannelClosed => BeginError::Io(message),
        });
    }
    Ok(ConfigApplyResponse {
        ok: true,
        status: "pending",
        transaction_id: record.transaction_id,
        previous_config_version: record.previous_config_version,
        candidate_config_version: record.candidate_config_version,
    })
}

fn begin_apply(
    controller: Arc<AppController>,
    healthy: &str,
    content: &str,
    base_version: &str,
    candidate_version: &str,
) -> std::result::Result<(), ConfigSaveError> {
    let reload = controller.reload_snapshot();
    if reload.pending || reload.in_progress {
        return Err(ConfigSaveError::ReloadBusy);
    }
    config_tx::begin(
        controller.config_path(),
        healthy,
        content,
        base_version,
        candidate_version,
    )
    .map_err(|error| match error {
        BeginError::Busy => ConfigSaveError::TransactionBusy,
        BeginError::VersionConflict => ConfigSaveError::VersionConflict,
        BeginError::CandidateVersionConflict => ConfigSaveError::Validation(
            "candidate version does not match configuration content".to_string(),
        ),
        BeginError::TooLarge { actual, max } => ConfigSaveError::Validation(format!(
            "configuration is too large: {actual} bytes > {max} bytes"
        )),
        BeginError::Invalid(message) => ConfigSaveError::Validation(message),
        BeginError::Io(message) => ConfigSaveError::Io(message),
    })?;
    match controller.request_reload() {
        Ok(()) => Ok(()),
        Err(error) => {
            let message = error.to_string();
            config_tx::rollback(controller.config_path(), &message)
                .map_err(|rollback| ConfigSaveError::Io(rollback.to_string()))?;
            match error {
                ControlRequestError::ReloadBusy => Err(ConfigSaveError::ReloadBusy),
                ControlRequestError::CommandChannelClosed => Err(ConfigSaveError::Reload(message)),
            }
        }
    }
}

fn config_apply_error(error: BeginError) -> crate::api::ApiResponse {
    match error {
        BeginError::Busy => json_error(
            StatusCode::CONFLICT,
            "config_apply_busy",
            "a configuration transaction or reload is already active",
        ),
        BeginError::VersionConflict => json_error(
            StatusCode::CONFLICT,
            "config_conflict",
            "configuration file changed since it was loaded",
        ),
        BeginError::CandidateVersionConflict => json_error(
            StatusCode::CONFLICT,
            "candidate_version_conflict",
            "candidate version does not match configuration content",
        ),
        BeginError::TooLarge { actual, max } => json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "config_too_large",
            format!("configuration is too large: {actual} bytes > {max} bytes"),
        ),
        BeginError::Invalid(message) => {
            json_error(StatusCode::BAD_REQUEST, "config_validate_failed", message)
        }
        BeginError::Io(message) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "config_apply_failed",
            message,
        ),
    }
}

fn config_updated_at_ms(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

pub fn register_builtin_routes(
    register: &ApiRegister,
    controller: Arc<AppController>,
) -> Result<()> {
    register.register_get(
        "/control",
        Arc::new(ControlHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_post(
        "/shutdown",
        Arc::new(ShutdownHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_post(
        "/restart",
        Arc::new(RestartHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_post(
        "/reload",
        Arc::new(ReloadHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_get(
        "/reload/status",
        Arc::new(ReloadStatusHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_get(
        "/system",
        Arc::new(SystemHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_get(
        "/config/check",
        Arc::new(ConfigCheckHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_get(
        "/config",
        Arc::new(ConfigGetHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_route(
        Method::PUT,
        "/config",
        Arc::new(ConfigSaveHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_post(
        "/config/validate",
        Arc::new(ConfigValidateHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_post(
        "/config/apply",
        Arc::new(ConfigApplyHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_get(
        "/config/apply/status",
        Arc::new(ConfigApplyStatusHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_get(
        "/config/history",
        Arc::new(ConfigHistoryHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_post(
        "/config/history/restore",
        Arc::new(ConfigHistoryRestoreHandler { controller }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use http::Method;
    use http_body_util::BodyExt;
    use tempfile::TempDir;
    use tokio::sync::mpsc::error::TryRecvError;

    use super::*;
    use crate::api::ApiHandler;
    use crate::infra::clock::AppClock;
    use crate::infra::control::ControlCommand;

    struct TempConfig {
        _dir: TempDir,
        path: PathBuf,
    }

    impl TempConfig {
        fn new() -> Self {
            let dir = TempDir::new().expect("temp directory");
            let path = dir.path().join("config.yaml");
            Self { _dir: dir, path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    fn valid_config_yaml() -> &'static str {
        r#"
plugins:
  - tag: debug_main
    type: debug_print
"#
    }

    fn test_request(method: Method, path: &str, body: Bytes) -> Request<Bytes> {
        Request::builder()
            .method(method)
            .uri(path)
            .body(body)
            .expect("request should build")
    }

    #[tokio::test]
    async fn control_handlers_enqueue_shutdown_and_reload() {
        AppClock::start();
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, mut rx) = AppController::new(temp.path().to_path_buf());

        let shutdown = ShutdownHandler {
            controller: controller.clone(),
        };
        let reload = ReloadHandler {
            controller: controller.clone(),
        };

        let response = shutdown
            .handle(test_request(Method::POST, "/shutdown", Bytes::new()))
            .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            rx.try_recv().expect("shutdown command"),
            ControlCommand::Shutdown
        );

        let response = reload
            .handle(test_request(Method::POST, "/reload", Bytes::new()))
            .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            rx.try_recv().expect("reload command"),
            ControlCommand::Reload
        );
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn reload_handler_rejects_parallel_reload_requests() {
        AppClock::start();
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, _rx) = AppController::new(temp.path().to_path_buf());
        controller.request_reload().expect("first reload accepted");

        let handler = ReloadHandler { controller };
        let response = handler
            .handle(test_request(Method::POST, "/reload", Bytes::new()))
            .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn system_handler_reports_build_capabilities() {
        AppClock::start();
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, _rx) = AppController::new(temp.path().to_path_buf());
        let handler = SystemHandler { controller };

        let response = handler
            .handle(test_request(Method::GET, "/system", Bytes::new()))
            .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).expect("body should be json");
        assert_eq!(value["version"], VERSION);
        assert!(matches!(
            value["process_memory_kind"].as_str(),
            Some("rss" | "private_working_set" | "private_commit" | "working_set")
        ));
        assert_eq!(value["build"]["version"], VERSION);
        assert_eq!(value["build"]["bundle"], crate::build_info::PRIMARY_BUNDLE);
        assert!(
            value["build"]["supported_plugins"]["executors"]
                .as_array()
                .expect("executors should be an array")
                .iter()
                .any(|value| value == "sequence")
        );
    }

    #[tokio::test]
    async fn config_handlers_validate_current_file_and_request_body() {
        AppClock::start();
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, _rx) = AppController::new(temp.path().to_path_buf());

        let check = ConfigCheckHandler {
            controller: controller.clone(),
        };
        let validate = ConfigValidateHandler { controller };

        let response = check
            .handle(test_request(Method::GET, "/config/check", Bytes::new()))
            .await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = validate
            .handle(test_request(
                Method::POST,
                "/config/validate",
                Bytes::from(valid_config_yaml().as_bytes().to_vec()),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = validate
            .handle(test_request(
                Method::POST,
                "/config/validate",
                Bytes::from(
                    serde_json::json!({
                        "format": "yaml",
                        "content": valid_config_yaml()
                    })
                    .to_string(),
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK);

        let response = validate
            .handle(test_request(
                Method::POST,
                "/config/validate",
                Bytes::from_static(b"plugins: ["),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn config_validate_error_includes_diagnostic_location() {
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, _rx) = AppController::new(temp.path().to_path_buf());
        let validate = ConfigValidateHandler { controller };
        let response = validate
            .handle(test_request(
                Method::POST,
                "/config/validate",
                Bytes::from_static(
                    b"
plugins:
  - tag: bad
    type: missing_plugin
",
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).expect("body should be json");
        assert_eq!(
            value["diagnostic_details"][0]["message"],
            "Plugin error: Unknown plugin type: missing_plugin"
        );
        assert_eq!(value["diagnostic_details"][0]["line"], 4);
        assert_eq!(value["diagnostic_details"][0]["column"], 11);
    }

    #[tokio::test]
    async fn config_validate_reports_invalid_plugin_tag_at_its_value() {
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, _rx) = AppController::new(temp.path().to_path_buf());
        let validate = ConfigValidateHandler { controller };
        let response = validate
            .handle(test_request(
                Method::POST,
                "/config/validate",
                Bytes::from_static(
                    b"
plugins:
  - tag: cache..cn
    type: cache
",
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).expect("body should be json");
        assert!(
            value["diagnostic_details"][0]["message"]
                .as_str()
                .expect("diagnostic message")
                .contains("Invalid plugin tag 'cache..cn'")
        );
        assert_eq!(value["diagnostic_details"][0]["line"], 3);
        assert_eq!(value["diagnostic_details"][0]["column"], 10);
        assert_eq!(value["diagnostic_details"][0]["end_column"], 19);
    }

    #[tokio::test]
    async fn config_get_and_save_handlers_round_trip_file_content() {
        AppClock::start();
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, mut rx) = AppController::new(temp.path().to_path_buf());

        let get = ConfigGetHandler {
            controller: controller.clone(),
        };
        let save = ConfigSaveHandler {
            controller: controller.clone(),
        };

        let response = get
            .handle(test_request(Method::GET, "/config", Bytes::new()))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let loaded: serde_json::Value = serde_json::from_slice(&body).expect("json response");
        let version = loaded["version"].as_str().expect("version");
        assert_eq!(loaded["content"], valid_config_yaml());

        let next_config = r#"
plugins:
  - tag: saved_debug
    type: debug_print
"#;
        let response = save
            .handle(test_request(
                Method::PUT,
                "/config",
                Bytes::from(
                    serde_json::json!({
                        "format": "yaml",
                        "content": next_config,
                        "base_version": version,
                        "validate": true,
                        "reload": true
                    })
                    .to_string(),
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(std::fs::read_to_string(temp.path()).unwrap(), next_config);
        assert_eq!(
            rx.try_recv().expect("reload command"),
            ControlCommand::Reload
        );
    }

    #[tokio::test]
    async fn config_save_rejects_invalid_yaml_and_version_conflicts() {
        AppClock::start();
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, _rx) = AppController::new(temp.path().to_path_buf());
        let save = ConfigSaveHandler { controller };

        let response = save
            .handle(test_request(
                Method::PUT,
                "/config",
                Bytes::from(
                    serde_json::json!({
                        "format": "yaml",
                        "content": "plugins: [",
                        "validate": true
                    })
                    .to_string(),
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = save
            .handle(test_request(
                Method::PUT,
                "/config",
                Bytes::from(
                    serde_json::json!({
                        "format": "yaml",
                        "content": valid_config_yaml(),
                        "base_version": "stale"
                    })
                    .to_string(),
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn config_save_reload_busy_does_not_change_disk() {
        AppClock::start();
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, _rx) = AppController::new(temp.path().to_path_buf());
        controller.request_reload().expect("seed pending reload");
        let save = ConfigSaveHandler { controller };

        let next_config = r#"
plugins:
  - tag: saved_before_busy_reload
    type: debug_print
"#;
        let response = save
            .handle(test_request(
                Method::PUT,
                "/config",
                Bytes::from(
                    serde_json::json!({
                        "format": "yaml",
                        "content": next_config,
                        "reload": true
                    })
                    .to_string(),
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            std::fs::read_to_string(temp.path()).unwrap(),
            valid_config_yaml()
        );
    }

    #[tokio::test]
    async fn config_apply_is_atomic_and_enqueues_reload() {
        AppClock::start();
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, mut rx) = AppController::new(temp.path().to_path_buf());
        controller.set_healthy_config(valid_config_yaml().to_string());
        let apply = ConfigApplyHandler {
            controller: controller.clone(),
        };
        let candidate = "plugins: []\n";
        let response = apply
            .handle(test_request(
                Method::POST,
                "/config/apply",
                Bytes::from(
                    serde_json::json!({
                        "format": "yaml",
                        "content": candidate,
                        "base_version": config_version(valid_config_yaml()),
                        "candidate_version": config_version(candidate)
                    })
                    .to_string(),
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            rx.try_recv().expect("reload command"),
            ControlCommand::Reload
        );
        assert_eq!(std::fs::read_to_string(temp.path()).unwrap(), candidate);
        let status = config_tx::status(temp.path()).unwrap().expect("status");
        assert_eq!(status.status, config_tx::TransactionStatus::Pending);
        config_tx::rollback(temp.path(), "test cleanup").unwrap();
        assert_eq!(
            std::fs::read_to_string(temp.path()).unwrap(),
            valid_config_yaml()
        );
    }

    #[tokio::test]
    async fn config_apply_rejects_stale_and_forged_versions_without_writing() {
        AppClock::start();
        let temp = TempConfig::new();
        std::fs::write(temp.path(), valid_config_yaml()).expect("write config");
        let (controller, _rx) = AppController::new(temp.path().to_path_buf());
        controller.set_healthy_config(valid_config_yaml().to_string());
        let apply = ConfigApplyHandler { controller };
        let candidate = "plugins: []\n";
        for (base_version, candidate_version) in [
            ("stale".to_string(), config_version(candidate)),
            (config_version(valid_config_yaml()), "forged".to_string()),
        ] {
            let response = apply
                .handle(test_request(
                    Method::POST,
                    "/config/apply",
                    Bytes::from(
                        serde_json::json!({
                            "format": "yaml",
                            "content": candidate,
                            "base_version": base_version,
                            "candidate_version": candidate_version
                        })
                        .to_string(),
                    ),
                ))
                .await;
            assert_eq!(response.status(), StatusCode::CONFLICT);
            assert_eq!(
                std::fs::read_to_string(temp.path()).unwrap(),
                valid_config_yaml()
            );
        }
    }
}
