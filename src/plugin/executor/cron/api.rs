// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Management API for manually triggering configured cron jobs.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, StatusCode};
use serde::Serialize;

use crate::api::{ApiHandler, json_error, json_ok};
use crate::infra::error::Result;
use crate::infra::task::{ManagedTaskHandle, TriggerOutcome};
use crate::register_plugin_api;

pub(super) fn register(tag: &str, jobs: Arc<HashMap<String, ManagedTaskHandle>>) -> Result<()> {
    register_plugin_api!(
        tag,
        |plugin_api|
        POST_PREFIX "/jobs/" => CronJobRunHandler {
            jobs,
            path_prefix: plugin_api.path("/jobs/")?,
        },
    )?;
    Ok(())
}

#[derive(Debug)]
struct CronJobRunHandler {
    jobs: Arc<HashMap<String, ManagedTaskHandle>>,
    path_prefix: String,
}

#[derive(Debug, Serialize)]
struct CronJobRunResponse {
    ok: bool,
    job: String,
    status: &'static str,
    trigger: &'static str,
}

#[async_trait]
impl ApiHandler for CronJobRunHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let job_name = match parse_job_run_path(request.uri().path(), &self.path_prefix) {
            Ok(job_name) => job_name,
            Err(message) => {
                return json_error(StatusCode::NOT_FOUND, "cron_job_route_not_found", message);
            }
        };

        let Some(handle) = self.jobs.get(&job_name) else {
            return json_error(
                StatusCode::NOT_FOUND,
                "cron_job_not_found",
                format!("cron job '{job_name}' does not exist"),
            );
        };

        match handle.trigger().await {
            TriggerOutcome::Started => json_ok(
                StatusCode::ACCEPTED,
                &CronJobRunResponse {
                    ok: true,
                    job: job_name,
                    status: "started",
                    trigger: "manual",
                },
            ),
            TriggerOutcome::AlreadyRunning => json_error(
                StatusCode::CONFLICT,
                "cron_job_already_running",
                format!("cron job '{job_name}' is already running"),
            ),
            TriggerOutcome::Unavailable => json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "cron_scheduler_unavailable",
                "cron scheduler is not available",
            ),
        }
    }
}

fn parse_job_run_path(path: &str, path_prefix: &str) -> std::result::Result<String, String> {
    let suffix = path
        .strip_prefix(path_prefix)
        .and_then(|suffix| suffix.strip_suffix("/run"))
        .ok_or_else(|| "cron job run route does not exist".to_string())?;
    if suffix.is_empty() || suffix.contains('/') {
        return Err("cron job name must be one encoded path segment".to_string());
    }

    let job_name = percent_decode_path_segment(suffix)?;
    if job_name.is_empty() {
        return Err("cron job name cannot be empty".to_string());
    }
    Ok(job_name)
}

fn percent_decode_path_segment(encoded: &str) -> std::result::Result<String, String> {
    fn hex_value(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let input = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] != b'%' {
            decoded.push(input[index]);
            index += 1;
            continue;
        }
        let Some(high) = input.get(index + 1).and_then(|byte| hex_value(*byte)) else {
            return Err("cron job name contains invalid percent encoding".to_string());
        };
        let Some(low) = input.get(index + 2).and_then(|byte| hex_value(*byte)) else {
            return Err("cron job name contains invalid percent encoding".to_string());
        };
        decoded.push((high << 4) | low);
        index += 3;
    }

    String::from_utf8(decoded).map_err(|_| "cron job name is not valid UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http_body_util::BodyExt;
    use tokio::sync::Notify;

    use super::*;
    use crate::infra::task::{TaskSchedule, register_scheduled};

    async fn run_handler(handle: ManagedTaskHandle) -> crate::api::ApiResponse {
        let handler = CronJobRunHandler {
            jobs: Arc::new(HashMap::from([("refresh sets/a+b".to_string(), handle)])),
            path_prefix: "/plugins/cron_main/jobs/".to_string(),
        };
        handler
            .handle(
                Request::builder()
                    .method("POST")
                    .uri("/plugins/cron_main/jobs/refresh%20sets%2Fa%2Bb/run")
                    .body(Bytes::new())
                    .unwrap(),
            )
            .await
    }

    #[tokio::test]
    async fn manual_run_handler_returns_accepted_for_started_job() {
        let handle = register_scheduled(
            "cron-api-started",
            TaskSchedule::fixed(Duration::from_secs(3600)),
            None,
            |_| async {},
        )
        .await
        .unwrap();
        let response = run_handler(handle.clone()).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("response body should collect")
            .to_bytes();
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("response should be valid json");
        assert_eq!(payload["job"], "refresh sets/a+b");
        assert_eq!(payload["trigger"], "manual");
        handle.stop().await;
    }

    #[tokio::test]
    async fn manual_run_handler_returns_conflict_for_active_job() {
        let started = Arc::new(Notify::new());
        let blocker = Arc::new(Notify::new());
        let started_task = started.clone();
        let blocker_task = blocker.clone();
        let handle = register_scheduled(
            "cron-api-busy",
            TaskSchedule::fixed(Duration::from_secs(3600)),
            None,
            move |_| {
                let started_task = started_task.clone();
                let blocker_task = blocker_task.clone();
                async move {
                    started_task.notify_one();
                    blocker_task.notified().await;
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(handle.trigger().await, TriggerOutcome::Started);
        started.notified().await;

        let response = run_handler(handle.clone()).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        blocker.notify_waiters();
        handle.stop().await;
    }

    #[tokio::test]
    async fn manual_run_handler_returns_unavailable_for_stopped_job() {
        let handle = register_scheduled(
            "cron-api-stopped",
            TaskSchedule::fixed(Duration::from_secs(3600)),
            None,
            |_| async {},
        )
        .await
        .unwrap();
        handle.stop().await;
        let response = run_handler(handle).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn manual_run_handler_returns_not_found_for_unknown_job() {
        let handler = CronJobRunHandler {
            jobs: Arc::new(HashMap::new()),
            path_prefix: "/plugins/cron_main/jobs/".to_string(),
        };
        let response = handler
            .handle(
                Request::builder()
                    .method("POST")
                    .uri("/plugins/cron_main/jobs/missing/run")
                    .body(Bytes::new())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn parse_job_run_path_rejects_unknown_suffixes() {
        let error = parse_job_run_path(
            "/plugins/cron_main/jobs/job/pause",
            "/plugins/cron_main/jobs/",
        )
        .expect_err("only the run action should be accepted");
        assert!(error.contains("does not exist"));
    }

    #[test]
    fn parse_job_run_path_preserves_literal_plus() {
        let job_name = parse_job_run_path(
            "/plugins/cron_main/jobs/a+b/run",
            "/plugins/cron_main/jobs/",
        )
        .expect("literal plus should remain part of the path segment");
        assert_eq!(job_name, "a+b");
    }
}
