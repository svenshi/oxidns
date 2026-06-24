// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, StatusCode};
use serde::{Deserialize, Serialize};

use super::backend::DynamicDomainSetBackend;
use super::rules::{DynamicDomainMutation, DynamicDomainRuleKind};
use crate::api::query::{parse_usize_param, visit_query_params};
use crate::api::{ApiHandler, json_error, json_ok};
use crate::infra::error::Result as DnsResult;
use crate::register_plugin_api;

const DEFAULT_LIST_LIMIT: usize = 500;
const MAX_LIST_LIMIT: usize = 5000;

#[derive(Debug, Clone, Serialize)]
pub(super) struct RulesListResponse {
    ok: bool,
    total: usize,
    next_cursor: Option<usize>,
    rules: Vec<String>,
}

impl RulesListResponse {
    pub(super) fn new(total: usize, next_cursor: Option<usize>, rules: Vec<String>) -> Self {
        Self {
            ok: true,
            total,
            next_cursor,
            rules,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct MutationResponse {
    ok: bool,
    added: usize,
    removed: usize,
    total: usize,
}

impl From<DynamicDomainMutation> for MutationResponse {
    fn from(value: DynamicDomainMutation) -> Self {
        Self {
            ok: true,
            added: value.added,
            removed: value.removed,
            total: value.total,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RulesMutationRequest {
    /// User-facing rules. Bare domains are interpreted using `rule_kind`.
    rules: Vec<String>,
    /// Default kind for bare domains in this request; explicit prefixes win.
    rule_kind: Option<DynamicDomainRuleKind>,
}

#[derive(Debug)]
pub(super) struct RulesListHandler {
    pub(super) backend: Arc<DynamicDomainSetBackend>,
}

#[derive(Debug)]
pub(super) struct RulesAddHandler {
    pub(super) backend: Arc<DynamicDomainSetBackend>,
}

#[derive(Debug)]
pub(super) struct RulesRemoveHandler {
    pub(super) backend: Arc<DynamicDomainSetBackend>,
}

#[derive(Debug)]
pub(super) struct RulesClearHandler {
    pub(super) backend: Arc<DynamicDomainSetBackend>,
}

#[async_trait]
impl ApiHandler for RulesListHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let query = match parse_list_query(request.uri().query()) {
            Ok(query) => query,
            Err(err) => return json_error(StatusCode::BAD_REQUEST, "invalid_query", err),
        };
        match self.backend.list_rules(query.cursor, query.limit) {
            Ok(response) => json_ok(StatusCode::OK, &response),
            Err(err) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "dynamic_domain_set_list_failed",
                err.to_string(),
            ),
        }
    }
}

#[async_trait]
impl ApiHandler for RulesAddHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let body = match parse_rules_request(request.body()) {
            Ok(body) => body,
            Err(err) => return json_error(StatusCode::BAD_REQUEST, "invalid_body", err),
        };
        // API writes are synchronous by design: the caller receives success only
        // after the file is durable and the hot snapshot has been replaced.
        match self
            .backend
            .append_rules_sync(
                body.rules,
                body.rule_kind.unwrap_or_default(),
                Duration::from_secs(5),
            )
            .await
        {
            Ok(result) => json_ok(StatusCode::OK, &MutationResponse::from(result)),
            Err(err) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "dynamic_domain_set_add_failed",
                err.to_string(),
            ),
        }
    }
}

#[async_trait]
impl ApiHandler for RulesRemoveHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let body = match parse_rules_request(request.body()) {
            Ok(body) => body,
            Err(err) => return json_error(StatusCode::BAD_REQUEST, "invalid_body", err),
        };
        // Delete accepts either explicitly prefixed rules or bare rules paired
        // with `rule_kind`, mirroring the add endpoint's input contract.
        match self
            .backend
            .remove_rules_sync(body.rules, body.rule_kind.unwrap_or_default())
            .await
        {
            Ok(result) => json_ok(StatusCode::OK, &MutationResponse::from(result)),
            Err(err) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "dynamic_domain_set_remove_failed",
                err.to_string(),
            ),
        }
    }
}

#[async_trait]
impl ApiHandler for RulesClearHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        match self.backend.clear_sync().await {
            Ok(result) => json_ok(StatusCode::OK, &MutationResponse::from(result)),
            Err(err) => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "dynamic_domain_set_clear_failed",
                err.to_string(),
            ),
        }
    }
}

pub(super) fn register_api(backend: &Arc<DynamicDomainSetBackend>) -> DnsResult<()> {
    register_plugin_api!(
        backend.tag(),
        GET "/rules" => RulesListHandler {
            backend: backend.clone(),
        },
        POST "/rules" => RulesAddHandler {
            backend: backend.clone(),
        },
        DELETE "/rules" => RulesRemoveHandler {
            backend: backend.clone(),
        },
        POST "/rules/clear" => RulesClearHandler {
            backend: backend.clone(),
        },
    )
}

#[derive(Debug, Clone, Copy)]
struct ListQuery {
    cursor: usize,
    limit: usize,
}

fn parse_list_query(query: Option<&str>) -> std::result::Result<ListQuery, String> {
    let mut cursor = 0usize;
    let mut limit = DEFAULT_LIST_LIMIT;
    visit_query_params(query, |key, value| {
        match key {
            "cursor" => {
                cursor = parse_usize_param(value, |err| {
                    format!("invalid cursor query parameter: {err}")
                })?;
            }
            "limit" => {
                let parsed = parse_usize_param(value, |err| {
                    format!("invalid limit query parameter: {err}")
                })?;
                if parsed == 0 {
                    return Err("limit must be greater than 0".to_string());
                }
                limit = parsed.min(MAX_LIST_LIMIT);
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(ListQuery { cursor, limit })
}

fn parse_rules_request(body: &Bytes) -> std::result::Result<RulesMutationRequest, String> {
    let request = serde_json::from_slice::<RulesMutationRequest>(body)
        .map_err(|err| format!("invalid json body: {err}"))?;
    if request.rules.is_empty() {
        return Err("rules must contain at least one rule".to_string());
    }
    Ok(request)
}
