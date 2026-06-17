// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opaque WebUI state persistence API.
//!
//! This module stores UI-only JSON state beside the active DNS YAML config.
//! The state is intentionally opaque to the backend: it is not part of the DNS
//! runtime configuration, does not trigger reload, and is never used on the DNS
//! request path.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use bytes::Bytes;
use http::{Method, Request, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::{ApiHandler, ApiRegister, json_error, json_ok};
use crate::infra::control::{AppController, config_version};
use crate::infra::error::Result;

const WEBUI_CONFIG_SCHEMA: u8 = 1;
const WEBUI_CONFIG_MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Serialize)]
struct WebUiConfigResponse {
    ok: bool,
    path: String,
    config: Value,
    version: String,
    updated_at_ms: u64,
    defaulted: bool,
    recovered: bool,
    backup_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct WebUiConfigOptionsResponse {
    ok: bool,
    persistent: bool,
    patch: bool,
    reset: bool,
    max_bytes: usize,
    schema: u8,
    path: String,
    default_config: Value,
}

#[derive(Debug, Deserialize)]
struct PutWebUiConfigRequest {
    config: Value,
    base_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PatchWebUiConfigRequest {
    patch: Value,
    base_version: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DeleteWebUiConfigRequest {
    base_version: Option<String>,
}

#[derive(Debug)]
enum WebUiConfigError {
    InvalidRequest(String),
    InvalidConfig(String),
    Conflict,
    TooLarge { actual: usize, max: usize },
    Io(String),
}

#[derive(Debug)]
struct LoadedWebUiConfig {
    path: PathBuf,
    config: Value,
    version: String,
    updated_at_ms: Option<u64>,
    defaulted: bool,
    recovered: bool,
    backup_path: Option<PathBuf>,
}

#[derive(Debug)]
struct WebUiConfigGetHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct WebUiConfigPutHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct WebUiConfigPatchHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct WebUiConfigDeleteHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct WebUiConfigOptionsHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for WebUiConfigGetHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match load_webui_config(self.controller.config_path()) {
            Ok(loaded) => json_ok(StatusCode::OK, &response_from_loaded(loaded)),
            Err(err) => error_response(err),
        }
    }
}

#[async_trait]
impl ApiHandler for WebUiConfigPutHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let save_request = match parse_json_body::<PutWebUiConfigRequest>(request.body()) {
            Ok(request) => request,
            Err(err) => return error_response(err),
        };
        match replace_webui_config(
            self.controller.config_path(),
            save_request.config,
            save_request.base_version.as_deref(),
        ) {
            Ok(loaded) => json_ok(StatusCode::OK, &response_from_loaded(loaded)),
            Err(err) => error_response(err),
        }
    }
}

#[async_trait]
impl ApiHandler for WebUiConfigPatchHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let patch_request = match parse_json_body::<PatchWebUiConfigRequest>(request.body()) {
            Ok(request) => request,
            Err(err) => return error_response(err),
        };
        match patch_webui_config(
            self.controller.config_path(),
            patch_request.patch,
            patch_request.base_version.as_deref(),
        ) {
            Ok(loaded) => json_ok(StatusCode::OK, &response_from_loaded(loaded)),
            Err(err) => error_response(err),
        }
    }
}

#[async_trait]
impl ApiHandler for WebUiConfigDeleteHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let delete_request =
            match parse_optional_json_body::<DeleteWebUiConfigRequest>(request.body()) {
                Ok(request) => request,
                Err(err) => return error_response(err),
            };
        match delete_webui_config(
            self.controller.config_path(),
            delete_request.base_version.as_deref(),
        ) {
            Ok(loaded) => json_ok(StatusCode::OK, &response_from_loaded(loaded)),
            Err(err) => error_response(err),
        }
    }
}

#[async_trait]
impl ApiHandler for WebUiConfigOptionsHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        let path = webui_config_path(self.controller.config_path());
        json_ok(
            StatusCode::OK,
            &WebUiConfigOptionsResponse {
                ok: true,
                persistent: true,
                patch: true,
                reset: true,
                max_bytes: WEBUI_CONFIG_MAX_BYTES,
                schema: WEBUI_CONFIG_SCHEMA,
                path: path.display().to_string(),
                default_config: default_webui_config(),
            },
        )
    }
}

pub fn register_builtin_routes(
    register: &ApiRegister,
    controller: Arc<AppController>,
) -> Result<()> {
    register.register_get(
        "/webui/config",
        Arc::new(WebUiConfigGetHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_route(
        Method::PUT,
        "/webui/config",
        Arc::new(WebUiConfigPutHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_route(
        Method::PATCH,
        "/webui/config",
        Arc::new(WebUiConfigPatchHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_delete(
        "/webui/config",
        Arc::new(WebUiConfigDeleteHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_get(
        "/webui/options",
        Arc::new(WebUiConfigOptionsHandler { controller }),
    )?;
    Ok(())
}

fn default_webui_config() -> Value {
    json!({
        "schema": WEBUI_CONFIG_SCHEMA,
        "mode": "expert",
        "standard": {},
        "ui": {
            "modeSelectionDismissed": false
        }
    })
}

fn webui_config_path(config_path: &Path) -> PathBuf {
    let mut value = config_path.as_os_str().to_owned();
    value.push(".webui.json");
    PathBuf::from(value)
}

fn response_from_loaded(loaded: LoadedWebUiConfig) -> WebUiConfigResponse {
    WebUiConfigResponse {
        ok: true,
        path: loaded.path.display().to_string(),
        config: loaded.config,
        version: loaded.version,
        updated_at_ms: loaded.updated_at_ms.unwrap_or(0),
        defaulted: loaded.defaulted,
        recovered: loaded.recovered,
        backup_path: loaded.backup_path.map(|path| path.display().to_string()),
    }
}

fn load_webui_config(
    config_path: &Path,
) -> std::result::Result<LoadedWebUiConfig, WebUiConfigError> {
    load_webui_config_from_path(&webui_config_path(config_path))
}

fn load_webui_config_from_path(
    path: &Path,
) -> std::result::Result<LoadedWebUiConfig, WebUiConfigError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let config = default_webui_config();
            return Ok(loaded_default(path, config, false, None));
        }
        Err(err) => {
            return Err(WebUiConfigError::Io(format!(
                "failed to read WebUI config {}: {err}",
                path.display()
            )));
        }
    };

    match serde_json::from_str::<Value>(&raw)
        .map_err(|err| {
            WebUiConfigError::InvalidConfig(format!("WebUI config is not valid JSON: {err}"))
        })
        .and_then(validate_config_value)
    {
        Ok(config) => Ok(LoadedWebUiConfig {
            path: path.to_path_buf(),
            version: config_version(&serialize_config(&config)?),
            updated_at_ms: updated_at_ms(path),
            config,
            defaulted: false,
            recovered: false,
            backup_path: None,
        }),
        Err(_) => recover_corrupt_config(path),
    }
}

fn loaded_default(
    path: &Path,
    config: Value,
    recovered: bool,
    backup_path: Option<PathBuf>,
) -> LoadedWebUiConfig {
    let serialized = serialize_config(&config).expect("default WebUI config should serialize");
    LoadedWebUiConfig {
        path: path.to_path_buf(),
        version: config_version(&serialized),
        updated_at_ms: None,
        config,
        defaulted: true,
        recovered,
        backup_path,
    }
}

fn recover_corrupt_config(path: &Path) -> std::result::Result<LoadedWebUiConfig, WebUiConfigError> {
    let backup_path = corrupt_backup_path(path);
    fs::rename(path, &backup_path).map_err(|err| {
        WebUiConfigError::Io(format!(
            "failed to back up corrupt WebUI config {}: {err}",
            path.display()
        ))
    })?;
    Ok(loaded_default(
        path,
        default_webui_config(),
        true,
        Some(backup_path),
    ))
}

fn replace_webui_config(
    config_path: &Path,
    config: Value,
    base_version: Option<&str>,
) -> std::result::Result<LoadedWebUiConfig, WebUiConfigError> {
    let path = webui_config_path(config_path);
    assert_current_version(&path, base_version)?;
    let config = validate_config_value(config)?;
    write_config_value(&path, &config)
}

fn patch_webui_config(
    config_path: &Path,
    patch: Value,
    base_version: Option<&str>,
) -> std::result::Result<LoadedWebUiConfig, WebUiConfigError> {
    let path = webui_config_path(config_path);
    let mut loaded = load_webui_config_from_path(&path)?;
    if let Some(base_version) = base_version
        && base_version != loaded.version
    {
        return Err(WebUiConfigError::Conflict);
    }
    merge_patch(&mut loaded.config, patch);
    let config = validate_config_value(loaded.config)?;
    write_config_value(&path, &config)
}

fn delete_webui_config(
    config_path: &Path,
    base_version: Option<&str>,
) -> std::result::Result<LoadedWebUiConfig, WebUiConfigError> {
    let path = webui_config_path(config_path);
    assert_current_version(&path, base_version)?;
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(WebUiConfigError::Io(format!(
                "failed to remove WebUI config {}: {err}",
                path.display()
            )));
        }
    }
    Ok(loaded_default(&path, default_webui_config(), false, None))
}

fn assert_current_version(
    path: &Path,
    base_version: Option<&str>,
) -> std::result::Result<(), WebUiConfigError> {
    let Some(base_version) = base_version else {
        return Ok(());
    };
    let loaded = load_webui_config_from_path(path)?;
    if base_version == loaded.version {
        Ok(())
    } else {
        Err(WebUiConfigError::Conflict)
    }
}

fn write_config_value(
    path: &Path,
    config: &Value,
) -> std::result::Result<LoadedWebUiConfig, WebUiConfigError> {
    let serialized = serialize_config(config)?;
    if serialized.len() > WEBUI_CONFIG_MAX_BYTES {
        return Err(WebUiConfigError::TooLarge {
            actual: serialized.len(),
            max: WEBUI_CONFIG_MAX_BYTES,
        });
    }
    atomic_write(path, serialized.as_bytes())?;
    Ok(LoadedWebUiConfig {
        path: path.to_path_buf(),
        config: config.clone(),
        version: config_version(&serialized),
        updated_at_ms: updated_at_ms(path),
        defaulted: false,
        recovered: false,
        backup_path: None,
    })
}

fn serialize_config(config: &Value) -> std::result::Result<String, WebUiConfigError> {
    serde_json::to_string_pretty(config).map_err(|err| {
        WebUiConfigError::InvalidConfig(format!("failed to serialize WebUI config: {err}"))
    })
}

fn validate_config_value(value: Value) -> std::result::Result<Value, WebUiConfigError> {
    if value.is_object() {
        Ok(value)
    } else {
        Err(WebUiConfigError::InvalidConfig(
            "WebUI config must be a JSON object".to_string(),
        ))
    }
}

fn merge_patch(target: &mut Value, patch: Value) {
    match (target, patch) {
        (Value::Object(target), Value::Object(patch)) => {
            for (key, value) in patch {
                if value.is_null() {
                    target.remove(&key);
                } else {
                    merge_patch(target.entry(key).or_insert(Value::Null), value);
                }
            }
        }
        (target, patch) => {
            *target = patch;
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::result::Result<(), WebUiConfigError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|err| {
        WebUiConfigError::Io(format!(
            "failed to create WebUI config directory {}: {err}",
            parent.display()
        ))
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("webui-config.json");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));

    {
        let mut file = fs::File::create(&temp_path).map_err(|err| {
            WebUiConfigError::Io(format!(
                "failed to create temporary WebUI config {}: {err}",
                temp_path.display()
            ))
        })?;
        file.write_all(bytes).map_err(|err| {
            WebUiConfigError::Io(format!(
                "failed to write temporary WebUI config {}: {err}",
                temp_path.display()
            ))
        })?;
        file.write_all(b"\n").map_err(|err| {
            WebUiConfigError::Io(format!(
                "failed to finish temporary WebUI config {}: {err}",
                temp_path.display()
            ))
        })?;
        file.sync_all().map_err(|err| {
            WebUiConfigError::Io(format!(
                "failed to sync temporary WebUI config {}: {err}",
                temp_path.display()
            ))
        })?;
    }

    fs::rename(&temp_path, path).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        WebUiConfigError::Io(format!(
            "failed to replace WebUI config {}: {err}",
            path.display()
        ))
    })?;

    if let Ok(parent_dir) = fs::File::open(parent) {
        let _ = parent_dir.sync_all();
    }

    Ok(())
}

fn corrupt_backup_path(path: &Path) -> PathBuf {
    let now = unix_time_ms();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("webui-config.json");
    path.with_file_name(format!("{file_name}.corrupt.{now}"))
}

fn updated_at_ms(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn parse_json_body<T>(body: &Bytes) -> std::result::Result<T, WebUiConfigError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_slice::<T>(body).map_err(|err| {
        WebUiConfigError::InvalidRequest(format!("request body must be JSON: {err}"))
    })
}

fn parse_optional_json_body<T>(body: &Bytes) -> std::result::Result<T, WebUiConfigError>
where
    T: for<'de> Deserialize<'de> + Default,
{
    if body.is_empty() {
        Ok(T::default())
    } else {
        parse_json_body(body)
    }
}

fn error_response(err: WebUiConfigError) -> crate::api::ApiResponse {
    match err {
        WebUiConfigError::InvalidRequest(message) | WebUiConfigError::InvalidConfig(message) => {
            json_error(
                StatusCode::BAD_REQUEST,
                "invalid_webui_config_request",
                message,
            )
        }
        WebUiConfigError::Conflict => json_error(
            StatusCode::CONFLICT,
            "webui_config_conflict",
            "WebUI config changed since it was loaded",
        ),
        WebUiConfigError::TooLarge { actual, max } => json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "webui_config_too_large",
            format!("WebUI config is too large: {actual} bytes > {max} bytes"),
        ),
        WebUiConfigError::Io(message) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "webui_config_io_failed",
            message,
        ),
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use tempfile::TempDir;

    use super::*;
    use crate::infra::clock::AppClock;

    fn test_request(method: Method, path: &str, body: Bytes) -> Request<Bytes> {
        Request::builder()
            .method(method)
            .uri(path)
            .body(body)
            .expect("request should build")
    }

    async fn json_body(response: crate::api::ApiResponse) -> Value {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        serde_json::from_slice(&body).expect("body should be json")
    }

    fn controller() -> (Arc<AppController>, TempDir) {
        AppClock::start();
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "plugins: []").expect("write config");
        let (controller, _rx) = AppController::new(path);
        (controller, dir)
    }

    #[tokio::test]
    async fn webui_config_get_returns_default_without_state_file() {
        let handler = WebUiConfigGetHandler {
            controller: controller().0,
        };
        let response = handler
            .handle(test_request(Method::GET, "/webui/config", Bytes::new()))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["defaulted"], true);
        assert_eq!(body["config"]["mode"], "expert");
        assert_eq!(body["config"]["ui"]["modeSelectionDismissed"], false);
    }

    #[tokio::test]
    async fn webui_config_put_and_get_round_trip() {
        let (controller, _dir) = controller();
        let put = WebUiConfigPutHandler {
            controller: controller.clone(),
        };
        let get = WebUiConfigGetHandler { controller };
        let response = put
            .handle(test_request(
                Method::PUT,
                "/webui/config",
                Bytes::from(
                    json!({
                        "config": {
                            "schema": 1,
                            "mode": "standard",
                            "standard": { "settings": { "enabled": true } },
                            "ui": { "modeSelectionDismissed": true }
                        }
                    })
                    .to_string(),
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let saved = json_body(response).await;

        let response = get
            .handle(test_request(Method::GET, "/webui/config", Bytes::new()))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let loaded = json_body(response).await;
        assert_eq!(loaded["config"]["mode"], "standard");
        assert_ne!(
            loaded["version"].as_str().expect("version"),
            config_version(&serialize_config(&default_webui_config()).unwrap())
        );
        assert_eq!(loaded["version"], saved["version"]);
    }

    #[tokio::test]
    async fn webui_config_rejects_version_conflicts() {
        let (controller, _dir) = controller();
        let put = WebUiConfigPutHandler {
            controller: controller.clone(),
        };
        let patch = WebUiConfigPatchHandler {
            controller: controller.clone(),
        };
        let delete = WebUiConfigDeleteHandler { controller };

        for (method, path, body, handler_kind) in [
            (
                Method::PUT,
                "/webui/config",
                json!({ "config": {}, "base_version": "stale" }).to_string(),
                0_u8,
            ),
            (
                Method::PATCH,
                "/webui/config",
                json!({ "patch": {}, "base_version": "stale" }).to_string(),
                1,
            ),
            (
                Method::DELETE,
                "/webui/config",
                json!({ "base_version": "stale" }).to_string(),
                2,
            ),
        ] {
            let response = match handler_kind {
                0 => {
                    put.handle(test_request(method, path, Bytes::from(body)))
                        .await
                }
                1 => {
                    patch
                        .handle(test_request(method, path, Bytes::from(body)))
                        .await
                }
                _ => {
                    delete
                        .handle(test_request(method, path, Bytes::from(body)))
                        .await
                }
            };
            assert_eq!(response.status(), StatusCode::CONFLICT);
        }
    }

    #[tokio::test]
    async fn webui_config_patch_uses_json_merge_patch() {
        let (controller, _dir) = controller();
        replace_webui_config(
            controller.config_path(),
            json!({
                "schema": 1,
                "mode": "expert",
                "ui": { "modeSelectionDismissed": false, "density": "compact" }
            }),
            None,
        )
        .expect("seed config");
        let patch = WebUiConfigPatchHandler { controller };
        let response = patch
            .handle(test_request(
                Method::PATCH,
                "/webui/config",
                Bytes::from(
                    json!({
                        "patch": {
                            "mode": "standard",
                            "ui": {
                                "modeSelectionDismissed": true,
                                "density": null
                            }
                        }
                    })
                    .to_string(),
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["config"]["mode"], "standard");
        assert_eq!(body["config"]["ui"]["modeSelectionDismissed"], true);
        assert!(body["config"]["ui"].get("density").is_none());
    }

    #[tokio::test]
    async fn webui_config_rejects_large_payloads() {
        let (controller, _dir) = controller();
        let put = WebUiConfigPutHandler { controller };
        let response = put
            .handle(test_request(
                Method::PUT,
                "/webui/config",
                Bytes::from(
                    json!({
                        "config": {
                            "schema": 1,
                            "mode": "expert",
                            "blob": "x".repeat(WEBUI_CONFIG_MAX_BYTES)
                        }
                    })
                    .to_string(),
                ),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn webui_config_recovers_corrupt_json() {
        let (controller, _dir) = controller();
        let path = webui_config_path(controller.config_path());
        std::fs::write(&path, "{not json").expect("write corrupt config");
        let get = WebUiConfigGetHandler { controller };

        let response = get
            .handle(test_request(Method::GET, "/webui/config", Bytes::new()))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["recovered"], true);
        assert_eq!(body["defaulted"], true);
        let backup = body["backup_path"].as_str().expect("backup path");
        assert!(std::path::Path::new(backup).exists());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn webui_config_delete_resets_missing_file_to_default() {
        let delete = WebUiConfigDeleteHandler {
            controller: controller().0,
        };
        let response = delete
            .handle(test_request(Method::DELETE, "/webui/config", Bytes::new()))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["config"]["mode"], "expert");
        assert_eq!(body["defaulted"], true);
    }

    #[tokio::test]
    async fn webui_config_options_report_capabilities() {
        let handler = WebUiConfigOptionsHandler {
            controller: controller().0,
        };
        let response = handler
            .handle(test_request(Method::GET, "/webui/options", Bytes::new()))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["persistent"], true);
        assert_eq!(body["patch"], true);
        assert_eq!(body["reset"], true);
        assert_eq!(body["max_bytes"], WEBUI_CONFIG_MAX_BYTES);
        assert_eq!(body["default_config"]["mode"], "expert");
    }
}
