// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, StatusCode};
use hyper::body::Frame;
use serde_json::json;

use super::backend::RecorderBackend;
use crate::api::{ApiHandler, json_error, json_ok, streaming_response};
use crate::core::observation::QueryObservation;
use crate::infra::error::Result;
use crate::register_plugin_api;

const SNAPSHOT_LIMIT: usize = 100;

fn snapshot(backend: &RecorderBackend) -> serde_json::Value {
    let mut rows: Vec<QueryObservation> = backend.observations.snapshots(SNAPSHOT_LIMIT);
    if let Ok(tail) = backend.tail.lock() {
        rows.extend(tail.iter().rev().take(SNAPSHOT_LIMIT).map(|record| {
            if let Some(mut observation) = record.record.observation.clone() {
                observation.record_id = Some(record.record.id);
                return observation;
            }
            let question = record.record.questions_json.first();
            QueryObservation {
                version: 0,
                id: format!("legacy-{}", record.record.id),
                record_id: Some(record.record.id),
                created_at_ms: record.record.created_at_ms,
                client_ip: record.record.client_ip.clone(),
                qname: question.map(|q| q.name.clone()).unwrap_or_default(),
                qtype: question.map(|q| q.qtype.clone()).unwrap_or_default(),
                status: "not_recorded".into(),
                elapsed_ms: record.record.elapsed_ms as f64,
                rcode: record.record.rcode.clone(),
                answers: record
                    .record
                    .answers_json
                    .iter()
                    .take(64)
                    .map(|answer| answer.payload_text.clone())
                    .collect(),
                events: Vec::new(),
                truncated: false,
            }
        }));
    }
    rows.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| b.id.cmp(&a.id))
            .then_with(|| b.record_id.is_some().cmp(&a.record_id.is_some()))
    });
    rows.dedup_by_key(|row| row.id.clone());
    rows.truncate(SNAPSHOT_LIMIT);
    json!({
        "ok": true, "version": 1, "capacity": SNAPSHOT_LIMIT,
        "enabled": backend.observe_details,
        "dropped": backend.observations.dropped(),
        "recorder_dropped": backend.dropped_total.load(std::sync::atomic::Ordering::Relaxed),
        "queries": rows,
    })
}

#[derive(Debug)]
struct ObservationsHandler {
    backend: Arc<RecorderBackend>,
    stream: bool,
}

#[async_trait]
impl ApiHandler for ObservationsHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        if !self.stream {
            return json_ok(StatusCode::OK, &snapshot(&self.backend));
        }
        let Ok(permit) = self.backend.observation_readers.clone().try_acquire_owned() else {
            return json_error(
                StatusCode::TOO_MANY_REQUESTS,
                "observation_stream_limit",
                "Too many observation streams",
            );
        };
        let timer = tokio::time::interval(Duration::from_millis(1000));
        let stream = futures::stream::unfold(
            (self.backend.clone(), timer, permit),
            |(backend, mut timer, permit)| async move {
                timer.tick().await;
                if backend
                    .stop_requested
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    return None;
                }
                let data = format!("event: snapshot\ndata: {}\n\n", snapshot(&backend));
                Some((Ok(Frame::data(Bytes::from(data))), (backend, timer, permit)))
            },
        );
        let mut response = streaming_response(StatusCode::OK, stream);
        response.headers_mut().insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("text/event-stream; charset=utf-8"),
        );
        response.headers_mut().insert(
            http::header::CACHE_CONTROL,
            http::HeaderValue::from_static("no-cache"),
        );
        response
    }
}

pub(super) fn register(backend: &Arc<RecorderBackend>) -> Result<()> {
    register_plugin_api!(
        &backend.tag, |_api|
        GET "/observations" => ObservationsHandler { backend: backend.clone(), stream: false },
        GET "/observations/stream" => ObservationsHandler { backend: backend.clone(), stream: true },
    )
}
