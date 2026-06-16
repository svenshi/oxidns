// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP / SSE adapter over the always-on
//! [`crate::infra::observability::log_buffer::LogBuffer`].
//!
//! Two endpoints are exposed:
//!
//! - `GET /api/logs` — recent entries from the ring (REST)
//! - `GET /api/logs/stream` — new entries in real-time (SSE)

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, StatusCode};
use hyper::body::Frame;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::api::{ApiHandler, ApiRegister, json_error, json_ok, streaming_response};
use crate::infra::error::Result;
use crate::infra::observability::log_buffer::{
    LevelFilter, LogBuffer, LogEntry, global_log_buffer, level_passes,
};

const SSE_HEARTBEAT_SECS: u64 = 15;
const DEFAULT_FETCH_LIMIT: usize = 200;
const MAX_FETCH_LIMIT: usize = 1000;
const MAX_TAIL: usize = 500;

#[derive(Debug, Serialize)]
struct LogsResponse {
    ok: bool,
    total: usize,
    entries: Vec<LogEntry>,
}

#[derive(Debug)]
struct LogsHandler {
    buffer: Arc<LogBuffer>,
}

#[derive(Debug)]
struct LogsStreamHandler {
    buffer: Arc<LogBuffer>,
}

#[async_trait]
impl ApiHandler for LogsHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let (limit, min_level) = match parse_logs_params(request.uri().query()) {
            Ok(params) => params,
            Err(err) => return json_error(StatusCode::BAD_REQUEST, "invalid_query", err),
        };
        let raw = self.buffer.recent_filtered(limit, min_level);
        let total = raw.len();
        let entries: Vec<LogEntry> = raw.iter().map(|e| (**e).clone()).collect();
        json_ok(
            StatusCode::OK,
            &LogsResponse {
                ok: true,
                total,
                entries,
            },
        )
    }
}

#[async_trait]
impl ApiHandler for LogsStreamHandler {
    async fn handle(&self, request: Request<Bytes>) -> crate::api::ApiResponse {
        let (tail, min_level) = match parse_stream_params(request.uri().query()) {
            Ok(params) => params,
            Err(err) => return json_error(StatusCode::BAD_REQUEST, "invalid_query", err),
        };

        let initial = self.buffer.recent_filtered(tail, min_level);
        let pending: VecDeque<Bytes> = initial.iter().map(|e| sse_log_frame(e)).collect();
        let receiver = self.buffer.subscribe();
        let heartbeat = tokio::time::interval(Duration::from_secs(SSE_HEARTBEAT_SECS));

        let stream = futures::stream::unfold(
            SseState {
                pending,
                receiver,
                heartbeat,
                min_level,
            },
            |mut state| async move {
                if let Some(bytes) = state.pending.pop_front() {
                    return Some((Ok(Frame::data(bytes)), state));
                }

                loop {
                    tokio::select! {
                        recv = state.receiver.recv() => {
                            match recv {
                                Ok(entry) => {
                                    if state.min_level.is_none_or(|f| level_passes(&entry.level, f)) {
                                        return Some((Ok(Frame::data(sse_log_frame(&entry))), state));
                                    }
                                }
                                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                                Err(broadcast::error::RecvError::Closed) => return None,
                            }
                        }
                        _ = state.heartbeat.tick() => {
                            return Some((Ok(Frame::data(Bytes::from_static(b": heartbeat\n\n"))), state));
                        }
                    }
                }
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
        response.headers_mut().insert(
            http::header::CONNECTION,
            http::HeaderValue::from_static("keep-alive"),
        );
        response
    }
}

struct SseState {
    pending: VecDeque<Bytes>,
    receiver: broadcast::Receiver<Arc<LogEntry>>,
    heartbeat: tokio::time::Interval,
    min_level: Option<LevelFilter>,
}

fn sse_log_frame(entry: &LogEntry) -> Bytes {
    match serde_json::to_vec(entry) {
        Ok(data) => {
            let mut frame = Vec::with_capacity(data.len() + 24);
            frame.extend_from_slice(b"event: log\ndata: ");
            frame.extend_from_slice(&data);
            frame.extend_from_slice(b"\n\n");
            Bytes::from(frame)
        }
        Err(err) => Bytes::from(format!(
            "event: error\ndata: {{\"message\":\"serialize failed: {err}\"}}\n\n"
        )),
    }
}

fn parse_logs_params(
    query: Option<&str>,
) -> std::result::Result<(usize, Option<LevelFilter>), String> {
    let mut limit = DEFAULT_FETCH_LIMIT;
    let mut min_level = None;
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "limit" => {
                limit = value
                    .parse::<usize>()
                    .map_err(|err| format!("invalid limit: {err}"))?
                    .clamp(1, MAX_FETCH_LIMIT);
            }
            "level" => {
                min_level = Some(
                    LevelFilter::parse(value.as_ref())
                        .ok_or_else(|| format!("invalid level: {}", value.as_ref()))?,
                );
            }
            _ => {}
        }
    }
    Ok((limit, min_level))
}

fn parse_stream_params(
    query: Option<&str>,
) -> std::result::Result<(usize, Option<LevelFilter>), String> {
    let mut tail = 0;
    let mut min_level = None;
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "tail" => {
                tail = value
                    .parse::<usize>()
                    .map_err(|err| format!("invalid tail: {err}"))?
                    .min(MAX_TAIL);
            }
            "level" => {
                min_level = Some(
                    LevelFilter::parse(value.as_ref())
                        .ok_or_else(|| format!("invalid level: {}", value.as_ref()))?,
                );
            }
            _ => {}
        }
    }
    Ok((tail, min_level))
}

pub fn register_log_routes(register: &ApiRegister) -> Result<()> {
    let Some(buffer) = global_log_buffer() else {
        return Ok(());
    };
    register.register_get(
        "/logs",
        Arc::new(LogsHandler {
            buffer: buffer.clone(),
        }),
    )?;
    register.register_get("/logs/stream", Arc::new(LogsStreamHandler { buffer }))?;
    Ok(())
}
