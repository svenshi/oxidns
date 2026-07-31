// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Standard Mode planning, ownership analysis, and transactional apply API.
//!
//! The compiler is owned by [`crate::config::standard_mode`]. This module is
//! the control-plane adapter around current files, versions, and application
//! lifecycle state; it never participates in DNS request execution.

use std::collections::{BTreeMap, BTreeSet};
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
    StandardCapabilities, StandardDiagnostic, StandardDiagnosticSeverity, StandardIntent,
    StandardPlan, StandardTemplateExpansion, StandardTemplateKind, StandardTemplateParameters,
    compile_standard_intent, decode_standard_intent, expand_standard_template,
    standard_intent_revision,
};
use crate::infra::control::{AppController, ControlRequestError, config_version};
use crate::infra::error::{DnsError, Result};

const STANDARD_TRANSACTION_SCHEMA: u8 = 1;
const STANDARD_TRANSACTION_MAX_BYTES: usize = 2 * 1024 * 1024;
const STANDARD_HISTORY_SCHEMA: u8 = 1;
const STANDARD_HISTORY_MAX_ENTRIES: usize = 20;
const STANDARD_ASSET_STORE_SCHEMA: u8 = 1;
const STANDARD_ASSET_STORE_MAX_ENTRIES: usize = 64;
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

#[derive(Debug, Deserialize)]
struct StandardTemplatePreviewRequest {
    base_intent: Value,
    kind: StandardTemplateKind,
    parameters: StandardTemplateParameters,
    base_config_version: Option<String>,
    base_standard_version: Option<String>,
    #[serde(default)]
    takeover: bool,
}

#[derive(Debug, Serialize)]
struct StandardTemplatePreviewResponse {
    ok: bool,
    expansion: StandardTemplateExpansion,
    plan: StandardPlanResponse,
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
    schema: u8,
    baseline: &'static str,
    preserved_top_level: Vec<String>,
    generated_plugin_tags: Vec<String>,
    replaced_plugin_tags: Vec<String>,
    removed_plugin_tags: Vec<String>,
    objects: Vec<StandardSemanticObjectDiff>,
    affected: StandardSemanticImpact,
    summary: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StandardSemanticObjectDiff {
    category: String,
    stable_id: String,
    change: &'static str,
    changed_fields: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
struct StandardSemanticImpact {
    paths: BTreeSet<String>,
    rules: BTreeSet<String>,
    caches: BTreeSet<String>,
    listeners: BTreeSet<String>,
    upstream_groups: BTreeSet<String>,
    managed_files: BTreeSet<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    dependency_graph: Option<crate::plugin::DependencyGraphReport>,
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
    #[serde(default)]
    intent_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<crate::config::standard_mode::StandardGenerationSummary>,
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
    intent_revision: String,
    summary: Option<crate::config::standard_mode::StandardGenerationSummary>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StandardAssetEnvelope {
    asset_schema: u8,
    kind: String,
    oxidns_version: String,
    bundle: String,
    intent_schema: u32,
    intent_revision: String,
    intent: Value,
    exported_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StandardAssetImportRequest {
    asset: Value,
    base_config_version: Option<String>,
    base_standard_version: Option<String>,
    #[serde(default)]
    takeover: bool,
}

#[derive(Debug, Serialize)]
struct StandardAssetImportResponse {
    ok: bool,
    asset_schema: u8,
    source_intent_schema: u32,
    intent_revision: String,
    plan: StandardPlanResponse,
}

#[derive(Debug, Deserialize)]
struct StandardExpertCopyRequest {
    intent: Value,
}

#[derive(Debug, Deserialize)]
struct StandardExpertAnalysisRequest {
    yaml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StandardSavedTemplate {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    kind: StandardTemplateKind,
    parameters: StandardTemplateParameters,
    source_intent_schema: u32,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StandardAssetStore {
    schema: u8,
    version: String,
    templates: Vec<StandardSavedTemplate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StandardSavedTemplateWriteRequest {
    expected_version: Option<String>,
    template: StandardSavedTemplate,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StandardSavedTemplateDeleteRequest {
    expected_version: Option<String>,
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StandardSavedTemplateDuplicateRequest {
    expected_version: Option<String>,
    id: String,
    new_id: String,
    new_name: String,
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
struct StandardTemplatePreviewHandler {
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

#[derive(Debug)]
struct StandardAssetExportHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct StandardAssetImportHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct StandardExpertCopyHandler;

#[derive(Debug)]
struct StandardExpertAnalysisHandler;

#[derive(Debug)]
struct StandardSavedTemplateHandler {
    controller: Arc<AppController>,
}

#[derive(Debug)]
struct StandardSavedTemplateDuplicateHandler {
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
impl ApiHandler for StandardTemplatePreviewHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let request = match serde_json::from_slice::<StandardTemplatePreviewRequest>(request.body())
        {
            Ok(request) => request,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_standard_template_request",
                    format!("request body must be JSON: {err}"),
                );
            }
        };
        let (intent, _) = match decode_standard_intent(request.base_intent) {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_standard_intent",
                    err.to_string(),
                );
            }
        };
        let expansion = match expand_standard_template(intent, request.kind, request.parameters) {
            Ok(expansion) => expansion,
            Err(message) => {
                return json_error(StatusCode::CONFLICT, "standard_template_collision", message);
            }
        };
        let intent = serde_json::to_value(&expansion.proposed_intent)
            .expect("Standard template intent should serialize");
        match build_plan_response(
            self.controller.config_path(),
            StandardPlanRequest {
                intent,
                base_config_version: request.base_config_version,
                base_standard_version: request.base_standard_version,
                takeover: request.takeover,
            },
        ) {
            Ok(plan) => json_ok(
                StatusCode::OK,
                &StandardTemplatePreviewResponse {
                    ok: true,
                    expansion,
                    plan,
                },
            ),
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
                "standard_template_preview_failed",
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

#[async_trait]
impl ApiHandler for StandardAssetExportHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        let loaded = match load_webui_config(self.controller.config_path()) {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "standard_asset_export_failed",
                    format!("failed to read Standard state: {err:?}"),
                );
            }
        };
        let Some(settings) = loaded.config.pointer("/standard/settings").cloned() else {
            return json_error(
                StatusCode::NOT_FOUND,
                "standard_asset_not_found",
                "no applied Standard intent is available to export",
            );
        };
        let (intent, _) = match decode_standard_intent(settings) {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::CONFLICT,
                    "standard_asset_invalid_state",
                    err.to_string(),
                );
            }
        };
        let build = match crate::build_info::snapshot() {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "standard_capabilities_unavailable",
                    err.to_string(),
                );
            }
        };
        let intent_value = serde_json::to_value(&intent).expect("Standard intent should serialize");
        json_ok(
            StatusCode::OK,
            &json!({
                "ok": true,
                "asset": StandardAssetEnvelope {
                    asset_schema: 1,
                    kind: "oxidns_standard_intent".to_string(),
                    oxidns_version: build.version.to_string(),
                    bundle: build.bundle.to_string(),
                    intent_schema: intent.schema,
                    intent_revision: standard_intent_revision(&intent),
                    intent: intent_value,
                    exported_at_ms: unix_time_ms(),
                    name: None,
                    description: None,
                }
            }),
        )
    }
}

#[async_trait]
impl ApiHandler for StandardAssetImportHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        if request.body().len() > STANDARD_TRANSACTION_MAX_BYTES {
            return json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "standard_asset_too_large",
                "Standard asset exceeds the 2 MiB limit",
            );
        }
        let request = match serde_json::from_slice::<StandardAssetImportRequest>(request.body()) {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_standard_asset_request",
                    format!("request body must be JSON: {err}"),
                );
            }
        };
        let asset = match serde_json::from_value::<StandardAssetEnvelope>(request.asset) {
            Ok(value) if value.asset_schema == 1 && value.kind == "oxidns_standard_intent" => value,
            Ok(value) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "unsupported_standard_asset",
                    format!(
                        "unsupported asset schema {} or kind '{}'",
                        value.asset_schema, value.kind
                    ),
                );
            }
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_standard_asset",
                    err.to_string(),
                );
            }
        };
        let (intent, _) = match decode_standard_intent(asset.intent.clone()) {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_standard_intent",
                    err.to_string(),
                );
            }
        };
        let intent_revision = standard_intent_revision(&intent);
        match build_plan_response(
            self.controller.config_path(),
            StandardPlanRequest {
                intent: asset.intent,
                base_config_version: request.base_config_version,
                base_standard_version: request.base_standard_version,
                takeover: request.takeover,
            },
        ) {
            Ok(plan) => json_ok(
                StatusCode::OK,
                &StandardAssetImportResponse {
                    ok: true,
                    asset_schema: asset.asset_schema,
                    source_intent_schema: asset.intent_schema,
                    intent_revision,
                    plan,
                },
            ),
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
                "standard_asset_import_failed",
                message,
            ),
        }
    }
}

#[async_trait]
impl ApiHandler for StandardExpertCopyHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let request = match serde_json::from_slice::<StandardExpertCopyRequest>(request.body()) {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_standard_expert_copy_request",
                    err.to_string(),
                );
            }
        };
        let (intent, migration) = match decode_standard_intent(request.intent) {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_standard_intent",
                    err.to_string(),
                );
            }
        };
        let build = match crate::build_info::snapshot() {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "standard_capabilities_unavailable",
                    err.to_string(),
                );
            }
        };
        let capabilities = StandardCapabilities::from_build(
            build.enabled_features.iter().copied(),
            &build.supported_plugins,
        );
        let plan = compile_standard_intent(intent, &capabilities, None, migration);
        let Some(generated) = plan.generated else {
            return json_response(StatusCode::UNPROCESSABLE_ENTITY, &plan);
        };
        let validation = match crate::config::validate_text(&generated.yaml) {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "standard_expert_copy_invalid",
                    err.to_string(),
                );
            }
        };
        json_ok(
            StatusCode::OK,
            &json!({
                "ok": true,
                "detached": true,
                "ownership": "expert_unmanaged",
                "banner": "Detached Expert snapshot; future edits and applies are not owned by Standard Mode.",
                "yaml": generated.yaml,
                "configVersion": generated.config_version,
                "intentRevision": generated.explanation.intent_revision,
                "dependencyGraph": validation.dependency_graph,
                "capabilities": generated.explanation.capabilities,
            }),
        )
    }
}

#[async_trait]
impl ApiHandler for StandardExpertAnalysisHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        if request.body().len() > STANDARD_TRANSACTION_MAX_BYTES {
            return json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "expert_config_too_large",
                "Expert configuration exceeds the 2 MiB limit",
            );
        }
        let request = match serde_json::from_slice::<StandardExpertAnalysisRequest>(request.body())
        {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_expert_analysis_request",
                    err.to_string(),
                );
            }
        };
        let validation = match crate::config::validate_text(&request.yaml) {
            Ok(value) => value,
            Err(err) => {
                return json_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "expert_config_invalid",
                    err.to_string(),
                );
            }
        };
        let system_integrations: BTreeSet<_> = validation
            .dependency_graph
            .nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.plugin_type.as_str(),
                    "ipset" | "nftset" | "mikrotik" | "ros_address_list"
                )
            })
            .map(|node| node.plugin_type.clone())
            .collect();
        let expert_only: Vec<_> = validation
            .dependency_graph
            .nodes
            .iter()
            .filter(|node| !node.tag.starts_with("standard_"))
            .map(|node| json!({ "tag": node.tag, "pluginType": node.plugin_type, "kind": node.kind }))
            .collect();
        json_ok(
            StatusCode::OK,
            &json!({
                "ok": true,
                "readOnly": true,
                "pluginCount": validation.plugin_count,
                "dependencyGraph": validation.dependency_graph,
                "nativeCapabilityFamilies": ["server", "executor", "matcher", "provider"],
                "expertOnlyObjects": expert_only,
                "systemIntegrations": system_integrations,
                "reverseConversion": {
                    "available": false,
                    "reason": "Arbitrary plugin graphs cannot be losslessly reverse-compiled into Standard intent; analysis never mutates configuration."
                }
            }),
        )
    }
}

#[async_trait]
impl ApiHandler for StandardSavedTemplateHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        match *request.method() {
            Method::GET => match read_asset_store(self.controller.config_path()) {
                Ok(store) => json_ok(StatusCode::OK, &json!({ "ok": true, "store": store })),
                Err(message) => json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "standard_asset_store_read_failed",
                    message,
                ),
            },
            Method::POST | Method::PATCH => {
                let update = request.method() == Method::PATCH;
                let payload = match serde_json::from_slice::<StandardSavedTemplateWriteRequest>(
                    request.body(),
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        return json_error(
                            StatusCode::BAD_REQUEST,
                            "invalid_saved_template_request",
                            err.to_string(),
                        );
                    }
                };
                match save_template(self.controller.config_path(), payload, update) {
                    Ok(store) => json_ok(StatusCode::OK, &json!({ "ok": true, "store": store })),
                    Err(AssetStoreError::Conflict(message)) => json_error(
                        StatusCode::CONFLICT,
                        "standard_asset_store_conflict",
                        message,
                    ),
                    Err(AssetStoreError::Invalid(message)) => {
                        json_error(StatusCode::BAD_REQUEST, "invalid_saved_template", message)
                    }
                    Err(AssetStoreError::Io(message)) => json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "standard_asset_store_write_failed",
                        message,
                    ),
                }
            }
            Method::DELETE => {
                let payload = match serde_json::from_slice::<StandardSavedTemplateDeleteRequest>(
                    request.body(),
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        return json_error(
                            StatusCode::BAD_REQUEST,
                            "invalid_saved_template_request",
                            err.to_string(),
                        );
                    }
                };
                match delete_template(self.controller.config_path(), payload) {
                    Ok(store) => json_ok(StatusCode::OK, &json!({ "ok": true, "store": store })),
                    Err(AssetStoreError::Conflict(message)) => json_error(
                        StatusCode::CONFLICT,
                        "standard_asset_store_conflict",
                        message,
                    ),
                    Err(AssetStoreError::Invalid(message)) => {
                        json_error(StatusCode::BAD_REQUEST, "invalid_saved_template", message)
                    }
                    Err(AssetStoreError::Io(message)) => json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "standard_asset_store_write_failed",
                        message,
                    ),
                }
            }
            _ => json_error(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "unsupported saved-template operation",
            ),
        }
    }
}

#[async_trait]
impl ApiHandler for StandardSavedTemplateDuplicateHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let payload =
            match serde_json::from_slice::<StandardSavedTemplateDuplicateRequest>(request.body()) {
                Ok(value) => value,
                Err(err) => {
                    return json_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_saved_template_request",
                        err.to_string(),
                    );
                }
            };
        match duplicate_template(self.controller.config_path(), payload) {
            Ok(store) => json_ok(StatusCode::OK, &json!({ "ok": true, "store": store })),
            Err(AssetStoreError::Conflict(message)) => json_error(
                StatusCode::CONFLICT,
                "standard_asset_store_conflict",
                message,
            ),
            Err(AssetStoreError::Invalid(message)) => {
                json_error(StatusCode::BAD_REQUEST, "invalid_saved_template", message)
            }
            Err(AssetStoreError::Io(message)) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "standard_asset_store_write_failed",
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
    register.register_route(
        Method::POST,
        "/standard/templates/preview",
        Arc::new(StandardTemplatePreviewHandler {
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
        Arc::new(StandardHistoryRestoreHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_get(
        "/standard/assets/export",
        Arc::new(StandardAssetExportHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_post(
        "/standard/assets/import",
        Arc::new(StandardAssetImportHandler {
            controller: controller.clone(),
        }),
    )?;
    register.register_post(
        "/standard/assets/expert-copy",
        Arc::new(StandardExpertCopyHandler),
    )?;
    register.register_post(
        "/standard/assets/expert-analysis",
        Arc::new(StandardExpertAnalysisHandler),
    )?;
    for method in [Method::GET, Method::POST, Method::PATCH, Method::DELETE] {
        register.register_route(
            method,
            "/standard/assets/templates",
            Arc::new(StandardSavedTemplateHandler {
                controller: controller.clone(),
            }),
        )?;
    }
    register.register_post(
        "/standard/assets/templates/duplicate",
        Arc::new(StandardSavedTemplateDuplicateHandler { controller }),
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
    let dependency_graph = if let Some(generated) = plan.generated.as_ref() {
        match preflight_candidate(config_path, &generated.yaml) {
            Ok(summary) => Some(summary.dependency_graph),
            Err(message) => {
                plan.diagnostics.push(StandardDiagnostic {
                    severity: StandardDiagnosticSeverity::Error,
                    code: "generated_config_invalid".to_string(),
                    path: "generated.yaml".to_string(),
                    message,
                });
                plan.can_apply = false;
                None
            }
        }
    } else {
        None
    };
    let previous_files = managed_files_from_state(&standard.config);
    let candidate_files: BTreeSet<_> = plan
        .generated
        .as_ref()
        .into_iter()
        .flat_map(|generated| generated.managed_files.iter().cloned())
        .collect();
    plan.details["managedFiles"] = json!({
        "created": candidate_files.difference(&previous_files).collect::<Vec<_>>(),
        "retained": candidate_files.intersection(&previous_files).collect::<Vec<_>>(),
        "orphaned": previous_files.difference(&candidate_files).collect::<Vec<_>>(),
    });
    let previous_intent = standard
        .config
        .pointer("/standard/settings")
        .cloned()
        .and_then(|value| decode_standard_intent(value).ok().map(|(intent, _)| intent));
    let semantic_diff = semantic_diff(
        &current_config,
        previous_intent
            .as_ref()
            .filter(|_| ownership == StandardOwnership::Managed),
        &plan,
    );
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
        dependency_graph,
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

fn semantic_diff(
    current_config: &str,
    previous_intent: Option<&StandardIntent>,
    plan: &StandardPlan,
) -> StandardSemanticDiff {
    let current_tags = plugin_tags(current_config);
    let generated_tags: BTreeSet<String> = plan
        .generated
        .as_ref()
        .map(|generated| generated.generated_tags.iter().cloned().collect())
        .unwrap_or_default();
    let candidate = &plan.normalized_intent;
    let previous_objects = previous_intent.map(intent_objects).unwrap_or_default();
    let candidate_objects = intent_objects(candidate);
    let keys: BTreeSet<_> = previous_objects
        .keys()
        .chain(candidate_objects.keys())
        .cloned()
        .collect();
    let mut objects = Vec::new();
    let mut affected = StandardSemanticImpact::default();
    for (category, stable_id) in keys {
        let previous = previous_objects.get(&(category.clone(), stable_id.clone()));
        let next = candidate_objects.get(&(category.clone(), stable_id.clone()));
        let (change, changed_fields) = match (previous, next) {
            (None, Some(_)) => ("added", Vec::new()),
            (Some(_), None) => ("removed", Vec::new()),
            (Some(previous), Some(next)) if previous != next => {
                let mut fields = Vec::new();
                changed_json_fields(previous, next, "", &mut fields);
                ("modified", fields)
            }
            (Some(_), Some(_)) => ("unchanged", Vec::new()),
            (None, None) => continue,
        };
        if change != "unchanged" {
            add_semantic_impact(
                &mut affected,
                &category,
                &stable_id,
                candidate,
                previous_intent,
            );
        }
        objects.push(StandardSemanticObjectDiff {
            category,
            stable_id,
            change,
            changed_fields,
        });
    }
    let changed = objects
        .iter()
        .filter(|item| item.change != "unchanged")
        .count();
    let mut summary = vec![format!("{changed} Standard intent object(s) changed")];
    if previous_intent.is_none() {
        summary
            .push("No trusted managed Standard baseline; impact is a takeover preview".to_string());
    }
    StandardSemanticDiff {
        schema: 1,
        baseline: if previous_intent.is_some() {
            "managed"
        } else {
            "takeover"
        },
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
        objects,
        affected,
        summary,
    }
}

fn intent_objects(intent: &StandardIntent) -> BTreeMap<(String, String), Value> {
    let value = serde_json::to_value(intent).expect("Standard intent should serialize");
    let mut result = BTreeMap::new();
    for (category, pointer) in [
        ("upstream_group", "/upstreamGroups"),
        ("path", "/paths"),
        ("routing_rule", "/routing/rules"),
        ("exception", "/exceptions"),
        ("device", "/devices"),
        ("dedicated_group", "/dedicatedGroups"),
        ("dynamic_learning", "/dynamicLearning/profiles"),
        ("advanced_rule", "/advancedRules"),
        ("filter_subscription", "/filtering/subscriptions"),
    ] {
        for item in value
            .pointer(pointer)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                result.insert((category.to_string(), id.to_string()), item.clone());
            }
        }
    }
    for (category, pointer) in [
        ("listen", "/listen"),
        ("cache", "/cache"),
        ("filtering", "/filtering"),
        ("local", "/local"),
        ("rule_data", "/ruleData"),
        ("smart_routing", "/smartRouting"),
        ("query_log", "/queryLog"),
        ("system", "/system"),
    ] {
        if let Some(item) = value.pointer(pointer) {
            result.insert((category.to_string(), "$".to_string()), item.clone());
        }
    }
    result
}

fn changed_json_fields(previous: &Value, next: &Value, prefix: &str, fields: &mut Vec<String>) {
    match (previous, next) {
        (Value::Object(previous), Value::Object(next)) => {
            let keys: BTreeSet<_> = previous.keys().chain(next.keys()).cloned().collect();
            for key in keys {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                match (previous.get(&key), next.get(&key)) {
                    (Some(left), Some(right)) => changed_json_fields(left, right, &path, fields),
                    _ => fields.push(path),
                }
            }
        }
        _ if previous != next => fields.push(prefix.to_string()),
        _ => {}
    }
}

fn add_semantic_impact(
    impact: &mut StandardSemanticImpact,
    category: &str,
    stable_id: &str,
    candidate: &StandardIntent,
    previous: Option<&StandardIntent>,
) {
    match category {
        "path" => {
            impact.paths.insert(stable_id.to_string());
            impact.caches.insert(stable_id.to_string());
        }
        "upstream_group" => {
            impact.upstream_groups.insert(stable_id.to_string());
            for path in candidate
                .paths
                .iter()
                .chain(previous.into_iter().flat_map(|item| &item.paths))
                .filter(|path| path.upstream_group_id == stable_id)
            {
                impact.paths.insert(path.id.clone());
                impact.caches.insert(path.id.clone());
            }
        }
        "routing_rule" | "exception" | "advanced_rule" | "dynamic_learning" => {
            impact.rules.insert(format!("{category}:{stable_id}"));
        }
        "dedicated_group" => {
            impact.paths.insert(format!("dedicated:{stable_id}"));
            impact.caches.insert(format!("dedicated:{stable_id}"));
            impact.listeners.insert(format!("dedicated:{stable_id}"));
        }
        "listen" => {
            impact.listeners.insert("main".to_string());
        }
        "cache" => {
            impact
                .caches
                .extend(candidate.paths.iter().map(|path| path.id.clone()));
        }
        "filtering"
        | "filter_subscription"
        | "local"
        | "rule_data"
        | "smart_routing"
        | "query_log"
        | "system" => {
            impact
                .paths
                .extend(candidate.paths.iter().map(|path| path.id.clone()));
        }
        _ => {}
    }
    if category == "dynamic_learning" {
        impact.managed_files.insert(format!(
            "./data/standard-dynamic-learning/{stable_id}.rules"
        ));
        impact.managed_files.insert(format!(
            "./data/standard-dynamic-learning/{stable_id}.meta.json"
        ));
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
            "intentRevision": standard_intent_revision(&plan.normalized_intent),
            "generatedTags": generated.generated_tags,
            "tagMap": generated.tag_map,
            "summary": generated.summary,
            "explanation": generated.explanation,
            "managedFiles": generated.managed_files,
            "generatedAtMs": unix_time_ms(),
            "transactionId": transaction_id,
        }),
    );
    Ok(state)
}

fn preflight_candidate(
    config_path: &Path,
    yaml: &str,
) -> std::result::Result<crate::config::ConfigValidationSummary, String> {
    let temp_path = adjacent_temp_path(config_path, "standard-preflight");
    atomic_replace(&temp_path, yaml.as_bytes(), true)?;
    let result = crate::config::validate_file(&temp_path)
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
    if let Err(message) =
        cleanup_orphaned_managed_files(&journal.previous_standard, &journal.candidate_standard)
    {
        tracing::warn!(error = %message, "Standard Mode managed-file cleanup needs retry");
    }
    remove_pending_journal(config_path).map_err(DnsError::runtime)?;
    Ok(true)
}

fn managed_files_from_state(state: &Value) -> BTreeSet<String> {
    state
        .get("standard")
        .and_then(|standard| standard.get("meta"))
        .and_then(|meta| meta.get("lastGenerated"))
        .and_then(|generated| generated.get("managedFiles"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn validate_managed_file(path: &str) -> std::result::Result<PathBuf, String> {
    const PREFIX: &str = "./data/standard-dynamic-learning/";
    let Some(filename) = path.strip_prefix(PREFIX) else {
        return Err(format!("refusing unowned managed path '{path}'"));
    };
    if filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || filename == "."
        || filename == ".."
        || !(filename.ends_with(".txt") || filename.ends_with(".meta.json"))
    {
        return Err(format!("refusing unsafe managed filename '{path}'"));
    }
    Ok(PathBuf::from(path))
}

fn cleanup_orphaned_managed_files(
    previous_state: &Value,
    candidate_state: &Value,
) -> std::result::Result<Vec<String>, String> {
    let previous = managed_files_from_state(previous_state);
    let candidate = managed_files_from_state(candidate_state);
    let mut removed = Vec::new();
    let mut errors = Vec::new();
    for path in previous.difference(&candidate) {
        let path = match validate_managed_file(path) {
            Ok(path) => path,
            Err(message) => {
                errors.push(message);
                continue;
            }
        };
        match fs::remove_file(&path) {
            Ok(()) => removed.push(path.display().to_string()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => errors.push(format!("failed to remove {}: {err}", path.display())),
        }
    }
    if errors.is_empty() {
        Ok(removed)
    } else {
        Err(errors.join("; "))
    }
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

fn asset_store_path(config_path: &Path) -> PathBuf {
    append_path_suffix(config_path, ".standard-assets.json")
}

#[derive(Debug)]
enum AssetStoreError {
    Conflict(String),
    Invalid(String),
    Io(String),
}

fn read_asset_store(config_path: &Path) -> std::result::Result<StandardAssetStore, String> {
    let mut store = read_bounded_json::<StandardAssetStore>(
        &asset_store_path(config_path),
        "Standard Mode asset store",
    )?
    .unwrap_or_else(|| StandardAssetStore {
        schema: STANDARD_ASSET_STORE_SCHEMA,
        version: String::new(),
        templates: Vec::new(),
    });
    if store.schema != STANDARD_ASSET_STORE_SCHEMA {
        return Err(format!(
            "unsupported Standard asset-store schema {}",
            store.schema
        ));
    }
    if store.templates.len() > STANDARD_ASSET_STORE_MAX_ENTRIES {
        return Err("Standard asset store exceeds 64 templates".to_string());
    }
    let computed = asset_store_version(&store.templates)?;
    if store.version.is_empty() {
        store.version = computed;
    } else if store.version != computed {
        return Err("Standard asset store version does not match its contents".to_string());
    }
    Ok(store)
}

fn asset_store_version(templates: &[StandardSavedTemplate]) -> std::result::Result<String, String> {
    serde_json::to_string(templates)
        .map(|value| format!("sha256:{}", config_version(&value)))
        .map_err(|err| format!("failed to serialize Standard assets: {err}"))
}

fn validate_saved_template(template: &StandardSavedTemplate) -> std::result::Result<(), String> {
    if template.id.is_empty()
        || template.id.len() > 64
        || !template
            .id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("saved-template id must be 1-64 ASCII letters, digits, '-' or '_'".to_string());
    }
    if template.name.trim().is_empty() || template.name.len() > 128 {
        return Err("saved-template name must be 1-128 characters".to_string());
    }
    if template
        .description
        .as_ref()
        .is_some_and(|value| value.len() > 1024)
    {
        return Err("saved-template description exceeds 1024 characters".to_string());
    }
    expand_standard_template(
        StandardIntent::default(),
        template.kind,
        template.parameters.clone(),
    )
    .map(|_| ())
    .map_err(|message| format!("saved-template parameters are invalid: {message}"))
}

fn check_asset_store_version(
    store: &StandardAssetStore,
    expected: Option<&str>,
) -> std::result::Result<(), AssetStoreError> {
    if expected.is_some_and(|value| value != store.version) {
        return Err(AssetStoreError::Conflict(
            "Standard asset store changed after it was loaded".to_string(),
        ));
    }
    Ok(())
}

fn write_asset_store(
    config_path: &Path,
    mut store: StandardAssetStore,
) -> std::result::Result<StandardAssetStore, AssetStoreError> {
    store.schema = STANDARD_ASSET_STORE_SCHEMA;
    store
        .templates
        .sort_by(|left, right| left.id.cmp(&right.id));
    store.version = asset_store_version(&store.templates).map_err(AssetStoreError::Io)?;
    write_bounded_json(&asset_store_path(config_path), &store).map_err(AssetStoreError::Io)?;
    Ok(store)
}

fn save_template(
    config_path: &Path,
    request: StandardSavedTemplateWriteRequest,
    update: bool,
) -> std::result::Result<StandardAssetStore, AssetStoreError> {
    let _guard = config_mutation_guard().map_err(AssetStoreError::Io)?;
    let mut store = read_asset_store(config_path).map_err(AssetStoreError::Io)?;
    check_asset_store_version(&store, request.expected_version.as_deref())?;
    validate_saved_template(&request.template).map_err(AssetStoreError::Invalid)?;
    let position = store
        .templates
        .iter()
        .position(|item| item.id == request.template.id);
    if update && position.is_none() {
        return Err(AssetStoreError::Invalid(
            "saved template does not exist".to_string(),
        ));
    }
    if !update && position.is_some() {
        return Err(AssetStoreError::Conflict(
            "saved-template id already exists".to_string(),
        ));
    }
    let now = unix_time_ms();
    let mut template = request.template;
    template.updated_at_ms = now;
    if let Some(position) = position {
        template.created_at_ms = store.templates[position].created_at_ms;
        store.templates[position] = template;
    } else {
        if store.templates.len() >= STANDARD_ASSET_STORE_MAX_ENTRIES {
            return Err(AssetStoreError::Invalid(
                "saved-template limit of 64 reached".to_string(),
            ));
        }
        template.created_at_ms = now;
        store.templates.push(template);
    }
    write_asset_store(config_path, store)
}

fn delete_template(
    config_path: &Path,
    request: StandardSavedTemplateDeleteRequest,
) -> std::result::Result<StandardAssetStore, AssetStoreError> {
    let _guard = config_mutation_guard().map_err(AssetStoreError::Io)?;
    let mut store = read_asset_store(config_path).map_err(AssetStoreError::Io)?;
    check_asset_store_version(&store, request.expected_version.as_deref())?;
    let previous_len = store.templates.len();
    store.templates.retain(|item| item.id != request.id);
    if store.templates.len() == previous_len {
        return Err(AssetStoreError::Invalid(
            "saved template does not exist".to_string(),
        ));
    }
    write_asset_store(config_path, store)
}

fn duplicate_template(
    config_path: &Path,
    request: StandardSavedTemplateDuplicateRequest,
) -> std::result::Result<StandardAssetStore, AssetStoreError> {
    let _guard = config_mutation_guard().map_err(AssetStoreError::Io)?;
    let mut store = read_asset_store(config_path).map_err(AssetStoreError::Io)?;
    check_asset_store_version(&store, request.expected_version.as_deref())?;
    if store.templates.len() >= STANDARD_ASSET_STORE_MAX_ENTRIES {
        return Err(AssetStoreError::Invalid(
            "saved-template limit of 64 reached".to_string(),
        ));
    }
    if store.templates.iter().any(|item| item.id == request.new_id) {
        return Err(AssetStoreError::Conflict(
            "saved-template id already exists".to_string(),
        ));
    }
    let mut copy = store
        .templates
        .iter()
        .find(|item| item.id == request.id)
        .cloned()
        .ok_or_else(|| AssetStoreError::Invalid("saved template does not exist".to_string()))?;
    copy.id = request.new_id;
    copy.name = request.new_name;
    copy.parameters.namespace = copy.id.clone();
    copy.parameters.name = copy.name.clone();
    let now = unix_time_ms();
    copy.created_at_ms = now;
    copy.updated_at_ms = now;
    validate_saved_template(&copy).map_err(AssetStoreError::Invalid)?;
    store.templates.push(copy);
    write_asset_store(config_path, store)
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
            intent_revision: journal
                .candidate_standard
                .pointer("/standard/meta/lastGenerated/intentRevision")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_default(),
            summary: journal
                .candidate_standard
                .pointer("/standard/meta/lastGenerated/summary")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
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
                intent_revision: if entry.intent_revision.is_empty() {
                    decode_standard_intent(entry.settings.clone())
                        .ok()
                        .map(|(intent, _)| standard_intent_revision(&intent))
                        .unwrap_or_default()
                } else {
                    entry.intent_revision
                },
                summary: entry.summary,
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
        assert_eq!(body["semantic_diff"]["schema"], 1);
        assert_eq!(body["semantic_diff"]["baseline"], "takeover");
        assert!(body["semantic_diff"]["affected"]["paths"].is_array());
        assert!(body["plan"]["generated"]["yaml"].is_string());
        assert_eq!(body["plan"]["generated"]["explanation"]["schema"], 1);
        assert!(body["dependency_graph"]["nodes"].is_array());
    }

    #[tokio::test]
    async fn standard_assets_round_trip_current_and_migrate_legacy_without_writes() {
        let (controller, mut receiver, _dir) = controller_with_receiver();
        let apply = StandardApplyHandler {
            controller: controller.clone(),
        }
        .handle(
            Request::builder()
                .method(Method::POST)
                .uri("/standard/apply")
                .body(Bytes::from(apply_body(&controller).to_string()))
                .expect("apply request"),
        )
        .await;
        assert_eq!(apply.status(), StatusCode::ACCEPTED);
        assert!(receiver.try_recv().is_ok());
        assert!(finalize_pending_transaction(controller.config_path()).expect("finalize"));

        let exported = response_json(
            StandardAssetExportHandler {
                controller: controller.clone(),
            }
            .handle(
                Request::builder()
                    .method(Method::GET)
                    .uri("/standard/assets/export")
                    .body(Bytes::new())
                    .expect("export request"),
            )
            .await,
        )
        .await;
        let config_before_import =
            fs::read(controller.config_path()).expect("read config before import");
        let current_import = response_json(
            StandardAssetImportHandler {
                controller: controller.clone(),
            }
            .handle(
                Request::builder()
                    .method(Method::POST)
                    .uri("/standard/assets/import")
                    .body(Bytes::from(
                        json!({ "asset": exported["asset"], "takeover": true }).to_string(),
                    ))
                    .expect("current import request"),
            )
            .await,
        )
        .await;
        assert_eq!(current_import["ok"], true);
        assert_eq!(
            current_import["intent_revision"],
            exported["asset"]["intentRevision"]
        );
        assert_eq!(current_import["plan"]["can_apply"], true);

        let legacy_asset = json!({
            "assetSchema": 1,
            "kind": "oxidns_standard_intent",
            "oxidnsVersion": "legacy",
            "bundle": "standard",
            "intentSchema": 1,
            "intentRevision": "legacy-untrusted",
            "intent": {
                "schema": 1,
                "listen": { "address": "127.0.0.1:5533", "udp": true, "tcp": true },
                "upstreams": [{
                    "id": "local",
                    "name": "本地上游",
                    "address": "127.0.0.1:5353",
                    "enabled": true
                }]
            },
            "exportedAtMs": 1
        });
        let legacy_import = response_json(
            StandardAssetImportHandler {
                controller: controller.clone(),
            }
            .handle(
                Request::builder()
                    .method(Method::POST)
                    .uri("/standard/assets/import")
                    .body(Bytes::from(
                        json!({ "asset": legacy_asset, "takeover": true }).to_string(),
                    ))
                    .expect("legacy import request"),
            )
            .await,
        )
        .await;
        assert_eq!(legacy_import["ok"], true);
        assert_eq!(legacy_import["source_intent_schema"], 1);
        assert_eq!(
            legacy_import["plan"]["plan"]["normalizedIntent"]["schema"],
            6
        );
        assert_eq!(
            fs::read(controller.config_path()).expect("read config after import"),
            config_before_import,
            "asset import must remain a read-only Plan operation"
        );
    }

    #[test]
    fn saved_templates_are_versioned_bounded_and_duplicated_without_external_state() {
        let (controller, _dir) = controller();
        let intent = StandardIntent::default();
        let store = read_asset_store(controller.config_path()).expect("empty asset store");
        let template = StandardSavedTemplate {
            id: "local_template".to_string(),
            name: "Local template".to_string(),
            description: Some("stored beside the OxiDNS config".to_string()),
            kind: StandardTemplateKind::LowLatency,
            parameters: StandardTemplateParameters {
                namespace: "local_template".to_string(),
                name: "Local template".to_string(),
                description: None,
                domains: vec!["domain:example.com".to_string()],
                upstreams: intent.upstream_groups[0].upstreams.clone(),
                listener_address: None,
            },
            source_intent_schema: intent.schema,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let saved = save_template(
            controller.config_path(),
            StandardSavedTemplateWriteRequest {
                expected_version: Some(store.version),
                template,
            },
            false,
        )
        .expect("save template");
        assert_eq!(saved.templates.len(), 1);
        let duplicated = duplicate_template(
            controller.config_path(),
            StandardSavedTemplateDuplicateRequest {
                expected_version: Some(saved.version),
                id: "local_template".to_string(),
                new_id: "local_template_copy".to_string(),
                new_name: "Local template copy".to_string(),
            },
        )
        .expect("duplicate template");
        assert_eq!(duplicated.templates.len(), 2);
        assert_ne!(duplicated.templates[0].id, duplicated.templates[1].id);
        assert!(asset_store_path(controller.config_path()).exists());
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
                    intent_revision: String::new(),
                    summary: None,
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

    #[test]
    fn managed_file_cleanup_accepts_only_exact_standard_dynamic_paths() {
        let candidate =
            json!({ "standard": { "meta": { "lastGenerated": { "managedFiles": [] } } } });
        assert!(validate_managed_file("./data/standard-dynamic-learning/profile.txt").is_ok());
        assert!(
            validate_managed_file("./data/standard-dynamic-learning/profile.meta.json").is_ok()
        );

        for unsafe_path in [
            "./data/standard-dynamic-learning/../config.yaml",
            "./data/other/file.txt",
            "/tmp/file.txt",
        ] {
            let previous = json!({
                "standard": { "meta": { "lastGenerated": { "managedFiles": [unsafe_path] } } }
            });
            assert!(cleanup_orphaned_managed_files(&previous, &candidate).is_err());
        }
    }

    fn accepted_target_version(standard: &LoadedWebUiConfig) -> String {
        standard.config["standard"]["meta"]["lastGenerated"]["configVersion"]
            .as_str()
            .expect("generated config version")
            .to_string()
    }
}
