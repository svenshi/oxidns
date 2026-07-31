// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Standard Mode planning, ownership analysis, and transactional apply API.
//!
//! The compiler is owned by [`crate::config::standard_mode`]. This module is
//! the control-plane adapter around current files, versions, and application
//! lifecycle state; it never participates in DNS request execution.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use bytes::Bytes;
use http::{Method, Request, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::webui_config::{
    LoadedWebUiConfig, load_webui_config, serialize_config, webui_config_path, write_config_value,
};
use crate::api::{ApiHandler, ApiRegister, json_error, json_ok, json_response};
use crate::config::standard_mode::{
    StandardCapabilities, StandardDiagnostic, StandardDiagnosticSeverity, StandardPlan,
    compile_standard_intent, decode_standard_intent,
};
use crate::infra::control::{AppController, ControlRequestError, config_version};
use crate::infra::error::{DnsError, Result};

const STANDARD_TRANSACTION_SCHEMA: u8 = 1;
const STANDARD_TRANSACTION_MAX_BYTES: usize = 2 * 1024 * 1024;
const STANDARD_HISTORY_SCHEMA: u8 = 1;
const STANDARD_HISTORY_MAX_ENTRIES: usize = 20;
static STANDARD_APPLY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
struct StandardPlanRequest {
    intent: Value,
    base_config_version: Option<String>,
    base_standard_version: Option<String>,
    #[serde(default)]
    takeover: bool,
}

#[derive(Debug, Deserialize)]
struct StandardApplyRequest {
    intent: Value,
    base_config_version: String,
    base_standard_version: String,
    planned_config_version: String,
    #[serde(default)]
    takeover: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum StandardOwnership {
    Managed,
    Modified,
    Unmanaged,
}

#[derive(Debug, Serialize)]
struct StandardSemanticDiff {
    preserved_top_level: Vec<String>,
    generated_plugin_tags: Vec<String>,
    replaced_plugin_tags: Vec<String>,
    removed_plugin_tags: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StandardApplyBlocker {
    code: &'static str,
    path: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
struct StandardPlanResponse {
    ok: bool,
    config_version: String,
    standard_version: String,
    ownership: StandardOwnership,
    semantic_diff: StandardSemanticDiff,
    blockers: Vec<StandardApplyBlocker>,
    can_apply: bool,
    plan: StandardPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StandardTransactionStatus {
    Pending,
    Succeeded,
    Failed,
    Recovered,
}

#[derive(Debug, Serialize, Deserialize)]
struct StandardApplyJournal {
    schema: u8,
    transaction_id: String,
    status: StandardTransactionStatus,
    created_at_ms: u64,
    previous_config: String,
    candidate_config: String,
    previous_standard_present: bool,
    previous_standard: Value,
    candidate_standard: Value,
    previous_config_version: String,
    candidate_config_version: String,
    previous_standard_version: String,
    candidate_standard_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StandardTransactionRecord {
    schema: u8,
    transaction_id: String,
    status: StandardTransactionStatus,
    completed_at_ms: u64,
    previous_config_version: String,
    candidate_config_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct StandardApplyResponse {
    ok: bool,
    transaction_id: String,
    status: StandardTransactionStatus,
    target_config_version: String,
}

#[derive(Debug, Serialize)]
struct StandardTransactionStatusResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    transaction: Option<StandardTransactionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StandardHistoryEntry {
    id: String,
    created_at_ms: u64,
    transaction_id: String,
    config_version: String,
    standard_version: String,
    settings: Value,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StandardHistoryStore {
    schema: u8,
    entries: Vec<StandardHistoryEntry>,
}

#[derive(Debug, Serialize)]
struct StandardHistoryItem {
    id: String,
    created_at_ms: u64,
    transaction_id: String,
    config_version: String,
    standard_version: String,
    settings_schema: Option<u64>,
    upstream_group_count: usize,
    path_count: usize,
}

#[derive(Debug, Serialize)]
struct StandardHistoryListResponse {
    ok: bool,
    entries: Vec<StandardHistoryItem>,
}

#[derive(Debug, Deserialize)]
struct StandardHistoryRestoreRequest {
    id: String,
}

#[derive(Debug, Serialize)]
struct StandardHistoryRestoreResponse {
    ok: bool,
    entry: StandardHistoryEntry,
}

#[derive(Debug)]
struct StandardPlanHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct StandardApplyHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct StandardTransactionStatusHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct StandardHistoryListHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct StandardHistoryRestoreHandler {
    controller: Arc<AppController>,
}

#[async_trait]
impl ApiHandler for StandardPlanHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let request = match serde_json::from_slice::<StandardPlanRequest>(request.body()) {
            Ok(request) => request,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_standard_plan_request",
                    format!("request body must be JSON: {err}"),
                );
            }
        };
        match build_plan_response(self.controller.config_path(), request) {
            Ok(response) => json_ok(StatusCode::OK, &response),
            Err(StandardPlanError::InvalidIntent(message)) => {
                json_error(StatusCode::BAD_REQUEST, "invalid_standard_intent", message)
            }
            Err(StandardPlanError::BuildInfo(message)) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "standard_capabilities_unavailable",
                message,
            ),
            Err(StandardPlanError::Io(message)) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "standard_plan_io_failed",
                message,
            ),
        }
    }
}

#[async_trait]
impl ApiHandler for StandardApplyHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let request = match serde_json::from_slice::<StandardApplyRequest>(request.body()) {
            Ok(request) => request,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_standard_apply_request",
                    format!("request body must be JSON: {err}"),
                );
            }
        };

        match prepare_apply(self.controller.as_ref(), request) {
            Ok(response) => json_ok(StatusCode::ACCEPTED, &response),
            Err(StandardApplyError::Rejected(plan)) => json_response(StatusCode::CONFLICT, &plan),
            Err(StandardApplyError::Plan(StandardPlanError::InvalidIntent(message))) => {
                json_error(StatusCode::BAD_REQUEST, "invalid_standard_intent", message)
            }
            Err(StandardApplyError::Plan(StandardPlanError::BuildInfo(message))) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "standard_capabilities_unavailable",
                message,
            ),
            Err(StandardApplyError::Plan(StandardPlanError::Io(message)))
            | Err(StandardApplyError::Io(message)) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "standard_apply_io_failed",
                message,
            ),
            Err(StandardApplyError::Busy(message)) => {
                json_error(StatusCode::CONFLICT, "standard_apply_busy", message)
            }
            Err(StandardApplyError::StalePlan(message)) => {
                json_error(StatusCode::CONFLICT, "standard_plan_stale", message)
            }
            Err(StandardApplyError::Reload(message)) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "standard_reload_request_failed",
                message,
            ),
        }
    }
}

#[async_trait]
impl ApiHandler for StandardTransactionStatusHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match transaction_status(self.controller.config_path()) {
            Ok(transaction) => json_ok(
                StatusCode::OK,
                &StandardTransactionStatusResponse {
                    ok: true,
                    transaction,
                },
            ),
            Err(message) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "standard_transaction_status_failed",
                message,
            ),
        }
    }
}

#[async_trait]
impl ApiHandler for StandardHistoryListHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match list_history(self.controller.config_path()) {
            Ok(entries) => json_ok(
                StatusCode::OK,
                &StandardHistoryListResponse { ok: true, entries },
            ),
            Err(message) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "standard_history_read_failed",
                message,
            ),
        }
    }
}

#[async_trait]
impl ApiHandler for StandardHistoryRestoreHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let request = match serde_json::from_slice::<StandardHistoryRestoreRequest>(request.body())
        {
            Ok(request) => request,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_standard_history_request",
                    format!("request body must be JSON: {err}"),
                );
            }
        };
        match history_entry(self.controller.config_path(), &request.id) {
            Ok(Some(entry)) => json_ok(
                StatusCode::OK,
                &StandardHistoryRestoreResponse { ok: true, entry },
            ),
            Ok(None) => json_error(
                StatusCode::NOT_FOUND,
                "standard_history_not_found",
                "Standard Mode history entry does not exist",
            ),
            Err(message) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "standard_history_read_failed",
                message,
            ),
        }
    }
}

pub fn register_builtin_routes(
    register: &ApiRegister,
    controller: Arc<AppController>,
) -> Result<()> {
    register.register_route(
        Method::POST,
        "/standard/plan",
        Arc::new(StandardPlanHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_route(
        Method::POST,
        "/standard/apply",
        Arc::new(StandardApplyHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_get(
        "/standard/apply/status",
        Arc::new(StandardTransactionStatusHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_get(
        "/standard/history",
        Arc::new(StandardHistoryListHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_post(
        "/standard/history/restore",
        Arc::new(StandardHistoryRestoreHandler { controller }),
    )?;
    Ok(())
}

fn build_plan_response(
    config_path: &std::path::Path,
    request: StandardPlanRequest,
) -> std::result::Result<StandardPlanResponse, StandardPlanError> {
    let current_config = fs::read_to_string(config_path).map_err(|err| {
        StandardPlanError::Io(format!(
            "failed to read config {}: {err}",
            config_path.display()
        ))
    })?;
    let current_config_version = config_version(&current_config);
    let standard = load_webui_config(config_path)
        .map_err(|err| StandardPlanError::Io(format!("failed to read Standard state: {err:?}")))?;
    let ownership = classify_ownership(&standard, &current_config_version);
    let (intent, migration) = decode_standard_intent(request.intent)
        .map_err(|err| StandardPlanError::InvalidIntent(err.to_string()))?;
    let build = crate::build_info::snapshot()
        .map_err(|err| StandardPlanError::BuildInfo(err.to_string()))?;
    let capabilities = StandardCapabilities::from_build(
        build.enabled_features.iter().copied(),
        &build.supported_plugins,
    );
    let mut plan = compile_standard_intent(intent, &capabilities, Some(&current_config), migration);
    if let Some(generated) = plan.generated.as_ref()
        && let Err(message) = preflight_candidate(config_path, &generated.yaml)
    {
        plan.diagnostics.push(StandardDiagnostic {
            severity: StandardDiagnosticSeverity::Error,
            code: "generated_config_invalid".to_string(),
            path: "generated.yaml".to_string(),
            message,
        });
        plan.can_apply = false;
    }
    let semantic_diff = semantic_diff(&current_config, &plan);
    let mut blockers = Vec::new();
    if request
        .base_config_version
        .as_deref()
        .is_some_and(|version| version != current_config_version)
    {
        blockers.push(StandardApplyBlocker {
            code: "config_version_conflict",
            path: "base_config_version",
            message: "DNS configuration changed after it was loaded".to_string(),
        });
    }
    if request
        .base_standard_version
        .as_deref()
        .is_some_and(|version| version != standard.version)
    {
        blockers.push(StandardApplyBlocker {
            code: "standard_version_conflict",
            path: "base_standard_version",
            message: "Standard Mode state changed after it was loaded".to_string(),
        });
    }
    if ownership != StandardOwnership::Managed && !request.takeover {
        blockers.push(StandardApplyBlocker {
            code: "takeover_confirmation_required",
            path: "takeover",
            message: format!(
                "current configuration is {}; explicit takeover confirmation is required",
                match ownership {
                    StandardOwnership::Managed => "managed",
                    StandardOwnership::Modified => "modified",
                    StandardOwnership::Unmanaged => "unmanaged",
                }
            ),
        });
    }
    let can_apply = plan.can_apply && blockers.is_empty();

    Ok(StandardPlanResponse {
        ok: true,
        config_version: current_config_version,
        standard_version: standard.version,
        ownership,
        semantic_diff,
        blockers,
        can_apply,
        plan,
    })
}

fn classify_ownership(
    standard: &LoadedWebUiConfig,
    current_config_version: &str,
) -> StandardOwnership {
    let Some(standard_object) = standard.config.get("standard").and_then(Value::as_object) else {
        return StandardOwnership::Unmanaged;
    };
    let Some(settings) = standard_object.get("settings") else {
        return StandardOwnership::Unmanaged;
    };
    let Some(last_generated) = standard_object
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("lastGenerated"))
        .and_then(Value::as_object)
    else {
        return StandardOwnership::Unmanaged;
    };
    let Some(generated_config_version) =
        last_generated.get("configVersion").and_then(Value::as_str)
    else {
        return StandardOwnership::Unmanaged;
    };
    let Some(settings_revision) = last_generated
        .get("settingsRevision")
        .and_then(Value::as_str)
    else {
        return StandardOwnership::Unmanaged;
    };

    if generated_config_version == current_config_version
        && settings_revision == legacy_settings_revision(settings)
    {
        StandardOwnership::Managed
    } else {
        StandardOwnership::Modified
    }
}

fn semantic_diff(current_config: &str, plan: &StandardPlan) -> StandardSemanticDiff {
    let current_tags = plugin_tags(current_config);
    let generated_tags: BTreeSet<String> = plan
        .generated
        .as_ref()
        .map(|generated| generated.generated_tags.iter().cloned().collect())
        .unwrap_or_default();
    StandardSemanticDiff {
        preserved_top_level: vec![
            "include".to_string(),
            "api".to_string(),
            "network".to_string(),
            "log.* except level".to_string(),
        ],
        generated_plugin_tags: generated_tags.iter().cloned().collect(),
        replaced_plugin_tags: current_tags
            .intersection(&generated_tags)
            .cloned()
            .collect(),
        removed_plugin_tags: current_tags.difference(&generated_tags).cloned().collect(),
    }
}

fn plugin_tags(config: &str) -> BTreeSet<String> {
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(config)
        .ok()
        .and_then(|value| {
            value
                .get("plugins")
                .and_then(serde_yaml_ng::Value::as_sequence)
                .map(|plugins| {
                    plugins
                        .iter()
                        .filter_map(|plugin| plugin.get("tag"))
                        .filter_map(serde_yaml_ng::Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
        })
        .unwrap_or_default()
}

fn legacy_settings_revision(settings: &Value) -> String {
    let stable = stable_json(settings);
    let mut hash = 0x811C9DC5_u32;
    // Match the WebUI's legacy JavaScript FNV implementation, which iterates
    // UTF-16 code units rather than UTF-8 bytes.
    for code_unit in stable.encode_utf16() {
        hash ^= u32::from(code_unit);
        hash = hash.wrapping_mul(0x01000193);
    }
    format!("fnv1a32:{hash:08x}")
}

fn stable_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).expect("string should serialize"),
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(stable_json).collect::<Vec<_>>().join(",")
        ),
        Value::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!(
                    "{}:{}",
                    serde_json::to_string(key).expect("key should serialize"),
                    stable_json(value)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn prepare_apply(
    controller: &AppController,
    request: StandardApplyRequest,
) -> std::result::Result<StandardApplyResponse, StandardApplyError> {
    let _guard = config_mutation_guard().map_err(StandardApplyError::Io)?;
    if pending_transaction_path(controller.config_path()).exists() {
        return Err(StandardApplyError::Busy(
            "another Standard Mode transaction is pending".to_string(),
        ));
    }
    let reload = controller.reload_snapshot();
    if reload.pending || reload.in_progress {
        return Err(StandardApplyError::Busy(
            "a configuration reload is already pending or in progress".to_string(),
        ));
    }

    let plan_request = StandardPlanRequest {
        intent: request.intent,
        base_config_version: Some(request.base_config_version),
        base_standard_version: Some(request.base_standard_version),
        takeover: request.takeover,
    };
    let plan = build_plan_response(controller.config_path(), plan_request)
        .map_err(StandardApplyError::Plan)?;
    if !plan.can_apply {
        return Err(StandardApplyError::Rejected(Box::new(plan)));
    }
    let generated = plan
        .plan
        .generated
        .as_ref()
        .expect("an applicable Standard plan must contain generated configuration");
    if request.planned_config_version != generated.config_version {
        return Err(StandardApplyError::StalePlan(
            "generated configuration differs from the reviewed plan; request a new plan"
                .to_string(),
        ));
    }

    let current_config = fs::read_to_string(controller.config_path()).map_err(|err| {
        StandardApplyError::Io(format!(
            "failed to read config {}: {err}",
            controller.config_path().display()
        ))
    })?;
    let loaded_standard = load_webui_config(controller.config_path())
        .map_err(|err| StandardApplyError::Io(format!("failed to read Standard state: {err:?}")))?;
    let previous_standard_present = webui_config_path(controller.config_path()).exists();
    let transaction_id = transaction_id(&generated.config_version);
    let candidate_standard =
        candidate_standard_state(loaded_standard.config.clone(), &plan.plan, &transaction_id)
            .map_err(StandardApplyError::Io)?;
    let candidate_standard_serialized = serialize_config(&candidate_standard).map_err(|err| {
        StandardApplyError::Io(format!("failed to serialize Standard state: {err:?}"))
    })?;
    let journal = StandardApplyJournal {
        schema: STANDARD_TRANSACTION_SCHEMA,
        transaction_id: transaction_id.clone(),
        status: StandardTransactionStatus::Pending,
        created_at_ms: unix_time_ms(),
        previous_config_version: config_version(&current_config),
        candidate_config_version: generated.config_version.clone(),
        previous_standard_version: loaded_standard.version,
        candidate_standard_version: config_version(&candidate_standard_serialized),
        previous_config: current_config,
        candidate_config: generated.yaml.clone(),
        previous_standard_present,
        previous_standard: loaded_standard.config,
        candidate_standard,
    };

    write_journal(controller.config_path(), &journal).map_err(StandardApplyError::Io)?;
    if let Err(message) = atomic_replace(
        controller.config_path(),
        journal.candidate_config.as_bytes(),
        false,
    ) {
        return match rollback_journal(controller.config_path(), &journal, &message) {
            Ok(()) => Err(StandardApplyError::Io(message)),
            Err(rollback_error) => Err(StandardApplyError::Io(format!(
                "{message}; failed to restore the previous configuration: {rollback_error}"
            ))),
        };
    }

    if let Err(err) = controller.request_reload() {
        let message = err.to_string();
        rollback_journal(controller.config_path(), &journal, &message)
            .map_err(StandardApplyError::Io)?;
        return match err {
            ControlRequestError::ReloadBusy => Err(StandardApplyError::Busy(message)),
            ControlRequestError::CommandChannelClosed => Err(StandardApplyError::Reload(message)),
        };
    }

    Ok(StandardApplyResponse {
        ok: true,
        transaction_id,
        status: StandardTransactionStatus::Pending,
        target_config_version: journal.candidate_config_version,
    })
}

fn candidate_standard_state(
    mut state: Value,
    plan: &StandardPlan,
    transaction_id: &str,
) -> std::result::Result<Value, String> {
    let generated = plan
        .generated
        .as_ref()
        .ok_or_else(|| "applicable plan has no generated configuration".to_string())?;
    let settings = serde_json::to_value(&plan.normalized_intent)
        .map_err(|err| format!("failed to serialize normalized Standard intent: {err}"))?;
    let revision = legacy_settings_revision(&settings);
    let root = state
        .as_object_mut()
        .ok_or_else(|| "WebUI state must be a JSON object".to_string())?;
    root.insert("mode".to_string(), Value::String("standard".to_string()));

    let ui = root.entry("ui").or_insert_with(|| json!({}));
    let ui = ui
        .as_object_mut()
        .ok_or_else(|| "WebUI ui state must be a JSON object".to_string())?;
    ui.insert("modeSelectionDismissed".to_string(), Value::Bool(true));

    let standard = root.entry("standard").or_insert_with(|| json!({}));
    let standard = standard
        .as_object_mut()
        .ok_or_else(|| "WebUI Standard state must be a JSON object".to_string())?;
    standard.insert("settings".to_string(), settings);
    let meta = standard.entry("meta").or_insert_with(|| json!({}));
    let meta = meta
        .as_object_mut()
        .ok_or_else(|| "WebUI Standard metadata must be a JSON object".to_string())?;
    meta.insert(
        "settingsRevision".to_string(),
        Value::String(revision.clone()),
    );
    meta.insert(
        "lastGenerated".to_string(),
        json!({
            "configVersion": generated.config_version,
            "settingsRevision": revision,
            "generatedTags": generated.generated_tags,
            "tagMap": generated.tag_map,
            "summary": generated.summary,
            "generatedAtMs": unix_time_ms(),
            "transactionId": transaction_id,
        }),
    );
    Ok(state)
}

fn preflight_candidate(config_path: &Path, yaml: &str) -> std::result::Result<(), String> {
    let temp_path = adjacent_temp_path(config_path, "standard-preflight");
    atomic_replace(&temp_path, yaml.as_bytes(), true)?;
    let result = crate::config::validate_file(&temp_path)
        .map(|_| ())
        .map_err(|err| format!("generated configuration failed backend validation: {err}"));
    let _ = fs::remove_file(&temp_path);
    result
}

pub(crate) fn has_pending_transaction(config_path: &Path) -> bool {
    pending_transaction_path(config_path).exists()
}

/// Finalize Standard state only after the candidate runtime has assembled.
/// Returns `Ok(false)` for ordinary Expert Mode reloads.
pub(crate) fn finalize_pending_transaction(config_path: &Path) -> Result<bool> {
    let _guard = config_mutation_guard().map_err(DnsError::runtime)?;
    let Some(journal) = read_journal(config_path).map_err(DnsError::runtime)? else {
        return Ok(false);
    };
    write_config_value(&webui_config_path(config_path), &journal.candidate_standard)
        .map_err(|err| DnsError::runtime(format!("failed to finalize Standard state: {err:?}")))?;
    write_transaction_record(
        config_path,
        transaction_record(&journal, StandardTransactionStatus::Succeeded, None),
    )
    .map_err(DnsError::runtime)?;
    append_history_entry(config_path, &journal).map_err(DnsError::runtime)?;
    remove_pending_journal(config_path).map_err(DnsError::runtime)?;
    Ok(true)
}

/// Restore both persisted files after a candidate reload failure.
/// Returns `Ok(false)` for ordinary Expert Mode reloads.
pub(crate) fn rollback_pending_transaction(
    config_path: &Path,
    error: impl Into<String>,
) -> Result<bool> {
    let _guard = config_mutation_guard().map_err(DnsError::runtime)?;
    let Some(journal) = read_journal(config_path).map_err(DnsError::runtime)? else {
        return Ok(false);
    };
    rollback_journal(config_path, &journal, &error.into()).map_err(DnsError::runtime)?;
    Ok(true)
}

/// Recover an interrupted apply before the normal configuration load occurs.
pub(crate) fn recover_pending_transaction(config_path: &Path) -> Result<bool> {
    let _guard = config_mutation_guard().map_err(DnsError::runtime)?;
    let Some(journal) = read_journal(config_path).map_err(DnsError::runtime)? else {
        return Ok(false);
    };
    restore_previous_state(config_path, &journal).map_err(DnsError::runtime)?;
    write_transaction_record(
        config_path,
        transaction_record(
            &journal,
            StandardTransactionStatus::Recovered,
            Some("interrupted Standard Mode apply was rolled back during startup".to_string()),
        ),
    )
    .map_err(DnsError::runtime)?;
    remove_pending_journal(config_path).map_err(DnsError::runtime)?;
    Ok(true)
}

fn rollback_journal(
    config_path: &Path,
    journal: &StandardApplyJournal,
    error: &str,
) -> std::result::Result<(), String> {
    restore_previous_state(config_path, journal)?;
    remove_history_entry(config_path, &journal.transaction_id)?;
    write_transaction_record(
        config_path,
        transaction_record(
            journal,
            StandardTransactionStatus::Failed,
            Some(bounded_error(error)),
        ),
    )?;
    remove_pending_journal(config_path)
}

fn restore_previous_state(
    config_path: &Path,
    journal: &StandardApplyJournal,
) -> std::result::Result<(), String> {
    atomic_replace(config_path, journal.previous_config.as_bytes(), false)?;
    let standard_path = webui_config_path(config_path);
    if journal.previous_standard_present {
        write_config_value(&standard_path, &journal.previous_standard)
            .map_err(|err| format!("failed to restore previous Standard state: {err:?}"))?;
    } else {
        match fs::remove_file(&standard_path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(format!(
                    "failed to remove staged Standard state {}: {err}",
                    standard_path.display()
                ));
            }
        }
    }
    Ok(())
}

fn transaction_status(
    config_path: &Path,
) -> std::result::Result<Option<StandardTransactionRecord>, String> {
    let _guard = config_mutation_guard()?;
    if let Some(journal) = read_journal(config_path)? {
        return Ok(Some(transaction_record(
            &journal,
            StandardTransactionStatus::Pending,
            None,
        )));
    }
    read_transaction_record(config_path)
}

fn transaction_record(
    journal: &StandardApplyJournal,
    status: StandardTransactionStatus,
    error: Option<String>,
) -> StandardTransactionRecord {
    StandardTransactionRecord {
        schema: STANDARD_TRANSACTION_SCHEMA,
        transaction_id: journal.transaction_id.clone(),
        status,
        completed_at_ms: unix_time_ms(),
        previous_config_version: journal.previous_config_version.clone(),
        candidate_config_version: journal.candidate_config_version.clone(),
        error,
    }
}

pub(crate) fn config_mutation_guard() -> std::result::Result<MutexGuard<'static, ()>, String> {
    STANDARD_APPLY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Standard Mode transaction lock is poisoned".to_string())
}

fn transaction_id(candidate_version: &str) -> String {
    let suffix = candidate_version.get(..12).unwrap_or(candidate_version);
    format!(
        "standard-{}-{}-{suffix}",
        unix_time_ms(),
        std::process::id()
    )
}

fn pending_transaction_path(config_path: &Path) -> PathBuf {
    append_path_suffix(config_path, ".standard-transaction.json")
}

fn transaction_record_path(config_path: &Path) -> PathBuf {
    append_path_suffix(config_path, ".standard-transaction.last.json")
}

fn history_path(config_path: &Path) -> PathBuf {
    append_path_suffix(config_path, ".standard-history.json")
}

fn append_path_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn write_journal(
    config_path: &Path,
    journal: &StandardApplyJournal,
) -> std::result::Result<(), String> {
    write_bounded_json(&pending_transaction_path(config_path), journal)
}

fn write_transaction_record(
    config_path: &Path,
    record: StandardTransactionRecord,
) -> std::result::Result<(), String> {
    write_bounded_json(&transaction_record_path(config_path), &record)
}

fn append_history_entry(
    config_path: &Path,
    journal: &StandardApplyJournal,
) -> std::result::Result<(), String> {
    let settings = journal
        .candidate_standard
        .pointer("/standard/settings")
        .cloned()
        .ok_or_else(|| "candidate Standard state has no settings".to_string())?;
    let mut history = read_history(config_path)?;
    history
        .entries
        .retain(|entry| entry.transaction_id != journal.transaction_id);
    history.entries.insert(
        0,
        StandardHistoryEntry {
            id: journal.transaction_id.clone(),
            created_at_ms: unix_time_ms(),
            transaction_id: journal.transaction_id.clone(),
            config_version: journal.candidate_config_version.clone(),
            standard_version: journal.candidate_standard_version.clone(),
            settings,
        },
    );
    history.entries.truncate(STANDARD_HISTORY_MAX_ENTRIES);
    write_history(config_path, history)
}

fn remove_history_entry(
    config_path: &Path,
    transaction_id: &str,
) -> std::result::Result<(), String> {
    let mut history = read_history(config_path)?;
    let previous_len = history.entries.len();
    history
        .entries
        .retain(|entry| entry.transaction_id != transaction_id);
    if history.entries.len() != previous_len {
        write_history(config_path, history)?;
    }
    Ok(())
}

fn list_history(config_path: &Path) -> std::result::Result<Vec<StandardHistoryItem>, String> {
    let _guard = config_mutation_guard()?;
    Ok(read_history(config_path)?
        .entries
        .into_iter()
        .map(|entry| {
            let settings_schema = entry.settings.get("schema").and_then(Value::as_u64);
            let upstream_group_count = entry
                .settings
                .get("upstreamGroups")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            let path_count = entry
                .settings
                .get("paths")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            StandardHistoryItem {
                id: entry.id,
                created_at_ms: entry.created_at_ms,
                transaction_id: entry.transaction_id,
                config_version: entry.config_version,
                standard_version: entry.standard_version,
                settings_schema,
                upstream_group_count,
                path_count,
            }
        })
        .collect())
}

fn history_entry(
    config_path: &Path,
    id: &str,
) -> std::result::Result<Option<StandardHistoryEntry>, String> {
    let _guard = config_mutation_guard()?;
    Ok(read_history(config_path)?
        .entries
        .into_iter()
        .find(|entry| entry.id == id))
}

fn read_history(config_path: &Path) -> std::result::Result<StandardHistoryStore, String> {
    let history = read_bounded_json::<StandardHistoryStore>(
        &history_path(config_path),
        "Standard Mode history",
    )?
    .unwrap_or_else(|| StandardHistoryStore {
        schema: STANDARD_HISTORY_SCHEMA,
        entries: Vec::new(),
    });
    if history.schema != STANDARD_HISTORY_SCHEMA {
        return Err(format!(
            "unsupported Standard Mode history schema {}",
            history.schema
        ));
    }
    Ok(history)
}

fn write_history(
    config_path: &Path,
    mut history: StandardHistoryStore,
) -> std::result::Result<(), String> {
    history.schema = STANDARD_HISTORY_SCHEMA;
    loop {
        let mut bytes = serde_json::to_vec_pretty(&history)
            .map_err(|err| format!("failed to serialize Standard Mode history: {err}"))?;
        bytes.push(b'\n');
        if bytes.len() <= STANDARD_TRANSACTION_MAX_BYTES {
            return atomic_replace(&history_path(config_path), &bytes, true);
        }
        if history.entries.len() <= 1 {
            return Err(format!(
                "Standard Mode history entry is too large: {} bytes > {} bytes",
                bytes.len(),
                STANDARD_TRANSACTION_MAX_BYTES
            ));
        }
        history.entries.pop();
    }
}

fn write_bounded_json(path: &Path, value: &impl Serialize) -> std::result::Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| format!("failed to serialize transaction state: {err}"))?;
    bytes.push(b'\n');
    if bytes.len() > STANDARD_TRANSACTION_MAX_BYTES {
        return Err(format!(
            "Standard transaction state is too large: {} bytes > {} bytes",
            bytes.len(),
            STANDARD_TRANSACTION_MAX_BYTES
        ));
    }
    atomic_replace(path, &bytes, true)
}

fn read_journal(config_path: &Path) -> std::result::Result<Option<StandardApplyJournal>, String> {
    read_bounded_json(
        &pending_transaction_path(config_path),
        "transaction journal",
    )
}

fn read_transaction_record(
    config_path: &Path,
) -> std::result::Result<Option<StandardTransactionRecord>, String> {
    read_bounded_json(&transaction_record_path(config_path), "transaction status")
}

fn read_bounded_json<T>(path: &Path, label: &str) -> std::result::Result<Option<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("failed to read {label} {}: {err}", path.display())),
    };
    if bytes.len() > STANDARD_TRANSACTION_MAX_BYTES {
        return Err(format!(
            "{label} {} exceeds {} bytes",
            path.display(),
            STANDARD_TRANSACTION_MAX_BYTES
        ));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|err| format!("failed to parse {label} {}: {err}", path.display()))
}

fn remove_pending_journal(config_path: &Path) -> std::result::Result<(), String> {
    let path = pending_transaction_path(config_path);
    match fs::remove_file(&path) {
        Ok(()) => {
            let _ = sync_parent(&path);
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "failed to remove transaction journal {}: {err}",
            path.display()
        )),
    }
}

fn atomic_replace(path: &Path, bytes: &[u8], sensitive: bool) -> std::result::Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|err| format!("failed to create directory {}: {err}", parent.display()))?;
    let temp_path = adjacent_temp_path(path, "write");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if sensitive { 0o600 } else { 0o666 });
    }
    let mut file = options.open(&temp_path).map_err(|err| {
        format!(
            "failed to create temporary file {}: {err}",
            temp_path.display()
        )
    })?;
    if !sensitive
        && let Ok(metadata) = fs::metadata(path)
        && let Err(err) = file.set_permissions(metadata.permissions())
    {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "failed to preserve permissions for {}: {err}",
            path.display()
        ));
    }
    let write_result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    if let Err(err) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "failed to write temporary file {}: {err}",
            temp_path.display()
        ));
    }
    replace_file(&temp_path, path).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        format!("failed to replace {}: {err}", path.display())
    })?;
    sync_parent(path)
}

fn adjacent_temp_path(path: &Path, purpose: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("oxidns-config");
    parent.join(format!(
        ".{name}.{purpose}.{}.{}.tmp",
        std::process::id(),
        unix_time_ms()
    ))
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows::core::PCWSTR;

    let from = from
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let to = to
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(std::io::Error::other)
}

fn sync_parent(path: &Path) -> std::result::Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if let Ok(directory) = fs::File::open(parent) {
        directory
            .sync_all()
            .map_err(|err| format!("failed to sync directory {}: {err}", parent.display()))?;
    }
    Ok(())
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn bounded_error(error: &str) -> String {
    const MAX_ERROR_CHARS: usize = 2048;
    error.chars().take(MAX_ERROR_CHARS).collect()
}

#[derive(Debug)]
enum StandardPlanError {
    InvalidIntent(String),
    BuildInfo(String),
    Io(String),
}

#[derive(Debug)]
enum StandardApplyError {
    Plan(StandardPlanError),
    Rejected(Box<StandardPlanResponse>),
    Busy(String),
    StalePlan(String),
    Reload(String),
    Io(String),
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use serde_json::json;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    use super::*;
    use crate::config::standard_mode::StandardIntent;
    use crate::infra::clock::AppClock;

    fn controller() -> (Arc<AppController>, TempDir) {
        AppClock::start();
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            "log:\n  level: info\nplugins:\n  - tag: existing\n    type: debug_print\n",
        )
        .expect("write config");
        let (controller, _rx) = AppController::new(path);
        (controller, dir)
    }

    fn controller_with_receiver() -> (
        Arc<AppController>,
        mpsc::UnboundedReceiver<crate::infra::control::ControlCommand>,
        TempDir,
    ) {
        AppClock::start();
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("config.yaml");
        fs::write(
            &path,
            "log:\n  level: info\nplugins:\n  - tag: existing\n    type: debug_print\n",
        )
        .expect("write config");
        let (controller, receiver) = AppController::new(path);
        (controller, receiver, dir)
    }

    fn apply_body(controller: &AppController) -> Value {
        let loaded = load_webui_config(controller.config_path()).expect("load Standard state");
        let plan = build_plan_response(
            controller.config_path(),
            StandardPlanRequest {
                intent: serde_json::to_value(StandardIntent::default()).expect("serialize intent"),
                base_config_version: None,
                base_standard_version: None,
                takeover: true,
            },
        )
        .expect("build plan");
        json!({
            "intent": StandardIntent::default(),
            "base_config_version": plan.config_version,
            "base_standard_version": loaded.version,
            "planned_config_version": plan.plan.generated.expect("generated config").config_version,
            "takeover": true,
        })
    }

    async fn response_json(response: crate::api::ApiResponse) -> Value {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("response body")
            .to_bytes();
        serde_json::from_slice(&body).expect("JSON response")
    }

    #[test]
    fn legacy_settings_revision_matches_javascript_for_unicode() {
        assert_eq!(
            legacy_settings_revision(&json!({ "name": "默认上游" })),
            "fnv1a32:f7a009c8"
        );
    }

    #[tokio::test]
    async fn plan_requires_takeover_for_unmanaged_configuration() {
        let (controller, _dir) = controller();
        let handler = StandardPlanHandler { controller };
        let request = Request::builder()
            .method(Method::POST)
            .uri("/standard/plan")
            .body(Bytes::from(
                json!({ "intent": StandardIntent::default() }).to_string(),
            ))
            .expect("request");

        let response = handler.handle(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ownership"], "unmanaged");
        assert_eq!(body["can_apply"], false);
        assert_eq!(body["plan"]["canApply"], true);
        assert_eq!(
            body["blockers"][0]["code"],
            "takeover_confirmation_required"
        );
    }

    #[tokio::test]
    async fn plan_with_takeover_returns_semantic_diff_and_generated_config() {
        let (controller, _dir) = controller();
        let handler = StandardPlanHandler { controller };
        let request = Request::builder()
            .method(Method::POST)
            .uri("/standard/plan")
            .body(Bytes::from(
                json!({
                    "intent": StandardIntent::default(),
                    "takeover": true,
                })
                .to_string(),
            ))
            .expect("request");

        let response = handler.handle(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["can_apply"], true);
        assert_eq!(body["semantic_diff"]["removed_plugin_tags"][0], "existing");
        assert!(body["plan"]["generated"]["yaml"].is_string());
    }

    #[tokio::test]
    async fn apply_stages_dns_and_finalizes_standard_state_after_runtime_success() {
        let (controller, mut receiver, _dir) = controller_with_receiver();
        let previous = fs::read_to_string(controller.config_path()).expect("read old config");
        let handler = StandardApplyHandler {
            controller: controller.clone(),
        };
        let response = handler
            .handle(
                Request::builder()
                    .method(Method::POST)
                    .uri("/standard/apply")
                    .body(Bytes::from(apply_body(&controller).to_string()))
                    .expect("request"),
            )
            .await;

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            receiver.try_recv().expect("reload command"),
            crate::infra::control::ControlCommand::Reload
        );
        assert_ne!(
            fs::read_to_string(controller.config_path()).expect("read candidate"),
            previous
        );
        assert!(has_pending_transaction(controller.config_path()));
        assert!(!webui_config_path(controller.config_path()).exists());

        assert!(finalize_pending_transaction(controller.config_path()).expect("finalize"));
        assert!(!has_pending_transaction(controller.config_path()));
        let standard = load_webui_config(controller.config_path()).expect("load finalized state");
        assert_eq!(standard.config["mode"], "standard");
        assert_eq!(
            standard.config["standard"]["settings"]["schema"],
            crate::config::standard_mode::CURRENT_STANDARD_SCHEMA
        );
        assert_eq!(
            classify_ownership(
                &standard,
                &config_version(
                    &fs::read_to_string(controller.config_path()).expect("read final config")
                ),
            ),
            StandardOwnership::Managed
        );
        assert_eq!(
            transaction_status(controller.config_path())
                .expect("transaction status")
                .expect("last transaction")
                .status,
            StandardTransactionStatus::Succeeded
        );
        let history = list_history(controller.config_path()).expect("list history");
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].config_version,
            accepted_target_version(&standard)
        );
        let restored = history_entry(controller.config_path(), &history[0].id)
            .expect("read history entry")
            .expect("history entry");
        assert_eq!(
            restored.settings["schema"],
            crate::config::standard_mode::CURRENT_STANDARD_SCHEMA
        );

        let list_response = StandardHistoryListHandler {
            controller: controller.clone(),
        }
        .handle(
            Request::builder()
                .method(Method::GET)
                .uri("/standard/history")
                .body(Bytes::new())
                .expect("request"),
        )
        .await;
        let list_body = response_json(list_response).await;
        assert!(list_body["entries"][0].get("settings").is_none());

        let restore_response = StandardHistoryRestoreHandler {
            controller: controller.clone(),
        }
        .handle(
            Request::builder()
                .method(Method::POST)
                .uri("/standard/history/restore")
                .body(Bytes::from(json!({ "id": history[0].id }).to_string()))
                .expect("request"),
        )
        .await;
        assert_eq!(
            response_json(restore_response).await["entry"]["settings"]["schema"],
            crate::config::standard_mode::CURRENT_STANDARD_SCHEMA
        );
    }

    #[tokio::test]
    async fn failed_runtime_apply_restores_both_files() {
        let (controller, mut receiver, _dir) = controller_with_receiver();
        let previous = fs::read_to_string(controller.config_path()).expect("read old config");
        let handler = StandardApplyHandler {
            controller: controller.clone(),
        };
        let response = handler
            .handle(
                Request::builder()
                    .method(Method::POST)
                    .uri("/standard/apply")
                    .body(Bytes::from(apply_body(&controller).to_string()))
                    .expect("request"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(receiver.try_recv().is_ok());

        assert!(
            rollback_pending_transaction(controller.config_path(), "injected assembly failure")
                .expect("rollback")
        );
        assert_eq!(
            fs::read_to_string(controller.config_path()).expect("read restored config"),
            previous
        );
        assert!(!webui_config_path(controller.config_path()).exists());
        assert!(!has_pending_transaction(controller.config_path()));
        let record = transaction_status(controller.config_path())
            .expect("transaction status")
            .expect("last transaction");
        assert_eq!(record.status, StandardTransactionStatus::Failed);
        assert_eq!(record.error.as_deref(), Some("injected assembly failure"));
        assert!(
            list_history(controller.config_path())
                .expect("list history")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn startup_recovery_rolls_back_interrupted_apply() {
        let (controller, mut receiver, _dir) = controller_with_receiver();
        let previous = fs::read_to_string(controller.config_path()).expect("read old config");
        let handler = StandardApplyHandler {
            controller: controller.clone(),
        };
        let response = handler
            .handle(
                Request::builder()
                    .method(Method::POST)
                    .uri("/standard/apply")
                    .body(Bytes::from(apply_body(&controller).to_string()))
                    .expect("request"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(receiver.try_recv().is_ok());

        assert!(recover_pending_transaction(controller.config_path()).expect("recover"));
        assert_eq!(
            fs::read_to_string(controller.config_path()).expect("read recovered config"),
            previous
        );
        assert_eq!(
            transaction_status(controller.config_path())
                .expect("transaction status")
                .expect("last transaction")
                .status,
            StandardTransactionStatus::Recovered
        );
    }

    #[tokio::test]
    async fn stale_planned_version_is_rejected_without_writes() {
        let (controller, _receiver, _dir) = controller_with_receiver();
        let previous = fs::read_to_string(controller.config_path()).expect("read old config");
        let mut body = apply_body(&controller);
        body["planned_config_version"] = Value::String("stale".to_string());
        let handler = StandardApplyHandler {
            controller: controller.clone(),
        };
        let response = handler
            .handle(
                Request::builder()
                    .method(Method::POST)
                    .uri("/standard/apply")
                    .body(Bytes::from(body.to_string()))
                    .expect("request"),
            )
            .await;

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            fs::read_to_string(controller.config_path()).expect("read config"),
            previous
        );
        assert!(!has_pending_transaction(controller.config_path()));
    }

    #[tokio::test]
    async fn concurrent_apply_is_rejected_while_first_transaction_is_pending() {
        let (controller, mut receiver, _dir) = controller_with_receiver();
        let body = apply_body(&controller);
        let handler = StandardApplyHandler {
            controller: controller.clone(),
        };
        let first = handler
            .handle(
                Request::builder()
                    .method(Method::POST)
                    .uri("/standard/apply")
                    .body(Bytes::from(body.to_string()))
                    .expect("request"),
            )
            .await;
        assert_eq!(first.status(), StatusCode::ACCEPTED);
        assert!(receiver.try_recv().is_ok());

        let second = handler
            .handle(
                Request::builder()
                    .method(Method::POST)
                    .uri("/standard/apply")
                    .body(Bytes::from(body.to_string()))
                    .expect("request"),
            )
            .await;
        assert_eq!(second.status(), StatusCode::CONFLICT);
        let second = response_json(second).await;
        assert_eq!(second["code"], "standard_apply_busy");
        rollback_pending_transaction(controller.config_path(), "test cleanup")
            .expect("rollback pending test transaction");
    }

    #[tokio::test]
    async fn closed_reload_channel_restores_files_and_records_failure() {
        let (controller, _dir) = controller();
        let previous = fs::read_to_string(controller.config_path()).expect("old config");
        let handler = StandardApplyHandler {
            controller: controller.clone(),
        };
        let response = handler
            .handle(
                Request::builder()
                    .method(Method::POST)
                    .uri("/standard/apply")
                    .body(Bytes::from(apply_body(&controller).to_string()))
                    .expect("request"),
            )
            .await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            fs::read_to_string(controller.config_path()).expect("restored config"),
            previous
        );
        assert!(!webui_config_path(controller.config_path()).exists());
        assert!(!has_pending_transaction(controller.config_path()));
        assert_eq!(
            transaction_status(controller.config_path())
                .expect("status")
                .expect("failed record")
                .status,
            StandardTransactionStatus::Failed
        );
    }

    #[tokio::test]
    async fn transaction_status_handler_reports_pending_transaction() {
        let (controller, mut receiver, _dir) = controller_with_receiver();
        let apply = StandardApplyHandler {
            controller: controller.clone(),
        };
        let response = apply
            .handle(
                Request::builder()
                    .method(Method::POST)
                    .uri("/standard/apply")
                    .body(Bytes::from(apply_body(&controller).to_string()))
                    .expect("request"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(receiver.try_recv().is_ok());

        let status = StandardTransactionStatusHandler {
            controller: controller.clone(),
        }
        .handle(
            Request::builder()
                .method(Method::GET)
                .uri("/standard/apply/status")
                .body(Bytes::new())
                .expect("request"),
        )
        .await;
        assert_eq!(status.status(), StatusCode::OK);
        assert_eq!(
            response_json(status).await["transaction"]["status"],
            "pending"
        );
        rollback_pending_transaction(controller.config_path(), "test cleanup")
            .expect("rollback pending test transaction");
    }

    #[test]
    fn managed_configuration_becomes_modified_after_expert_edit() {
        let (controller, _receiver, _dir) = controller_with_receiver();
        let request: StandardApplyRequest =
            serde_json::from_value(apply_body(&controller)).expect("apply request");
        let accepted = prepare_apply(&controller, request).expect("prepare apply");
        assert!(!accepted.transaction_id.is_empty());
        finalize_pending_transaction(controller.config_path()).expect("finalize");

        let mut content = fs::read_to_string(controller.config_path()).expect("read config");
        content.push_str("\n# edited in Expert Mode\n");
        fs::write(controller.config_path(), content).expect("edit config");
        let response = build_plan_response(
            controller.config_path(),
            StandardPlanRequest {
                intent: serde_json::to_value(StandardIntent::default()).expect("intent"),
                base_config_version: None,
                base_standard_version: None,
                takeover: false,
            },
        )
        .expect("plan");
        assert_eq!(response.ownership, StandardOwnership::Modified);
        assert_eq!(response.blockers[0].code, "takeover_confirmation_required");
    }

    #[test]
    fn corrupt_pending_journal_blocks_startup_without_touching_config() {
        let (controller, _dir) = controller();
        let previous = fs::read_to_string(controller.config_path()).expect("read config");
        fs::write(
            pending_transaction_path(controller.config_path()),
            b"{not-json",
        )
        .expect("write corrupt journal");

        assert!(recover_pending_transaction(controller.config_path()).is_err());
        assert_eq!(
            fs::read_to_string(controller.config_path()).expect("read config after recovery"),
            previous
        );
        assert!(pending_transaction_path(controller.config_path()).exists());
    }

    #[test]
    fn history_is_bounded_and_latest_entry_can_be_restored() {
        let (controller, _dir) = controller();
        let mut history = StandardHistoryStore {
            schema: STANDARD_HISTORY_SCHEMA,
            entries: Vec::new(),
        };
        for index in 0..(STANDARD_HISTORY_MAX_ENTRIES + 5) {
            history.entries.insert(
                0,
                StandardHistoryEntry {
                    id: format!("history-{index}"),
                    created_at_ms: index as u64,
                    transaction_id: format!("transaction-{index}"),
                    config_version: format!("config-{index}"),
                    standard_version: format!("standard-{index}"),
                    settings: json!({ "schema": 4, "upstreamGroups": [], "paths": [] }),
                },
            );
            history.entries.truncate(STANDARD_HISTORY_MAX_ENTRIES);
        }
        write_history(controller.config_path(), history).expect("write history");

        let entries = list_history(controller.config_path()).expect("list history");
        assert_eq!(entries.len(), STANDARD_HISTORY_MAX_ENTRIES);
        assert_eq!(entries[0].id, "history-24");
        let restored = history_entry(controller.config_path(), "history-24")
            .expect("read history")
            .expect("history entry");
        assert_eq!(restored.settings["schema"], 4);
    }

    fn accepted_target_version(standard: &LoadedWebUiConfig) -> String {
        standard.config["standard"]["meta"]["lastGenerated"]["configVersion"]
            .as_str()
            .expect("generated config version")
            .to_string()
    }
}
