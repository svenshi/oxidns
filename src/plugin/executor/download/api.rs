// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Manual downloads use the live configuration and share the executor's run lock.

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, StatusCode};
use serde::Deserialize;
use serde_json::json;

use super::DownloadExecutor;
use crate::api::{ApiHandler, ApiResponse, json_error, json_ok};
use crate::infra::error::Result;
use crate::register_plugin_api;

pub(super) fn register(executor: &DownloadExecutor) -> Result<()> {
    register_plugin_api!(
        &executor.tag,
        GET "/downloads" => DownloadListHandler(executor.clone()),
        POST "/download" => DownloadRunHandler(executor.clone()),
    )
}

#[derive(Debug)]
struct DownloadListHandler(DownloadExecutor);

#[async_trait]
impl ApiHandler for DownloadListHandler {
    async fn handle(&self, _request: Request<Bytes>) -> ApiResponse {
        let downloads: Vec<_> = self
            .0
            .downloads
            .iter()
            .enumerate()
            .map(|(index, item)| json!({ "index": index, "url": item.url, "path": item.path }))
            .collect();
        json_ok(
            StatusCode::OK,
            &json!({
                "ok": true,
                "running": self.0.run_lock.try_lock().is_err(),
                "downloads": downloads,
            }),
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadRequest {
    /// A zero-based index in the live download list; omission downloads all items.
    index: Option<usize>,
}

#[derive(Debug)]
struct DownloadRunHandler(DownloadExecutor);

#[async_trait]
impl ApiHandler for DownloadRunHandler {
    async fn handle(&self, request: Request<Bytes>) -> ApiResponse {
        let object = match serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(
            request.body(),
        ) {
            Ok(object) => object,
            Err(_) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "expected a JSON object",
                );
            }
        };
        let params =
            match serde_json::from_value::<DownloadRequest>(serde_json::Value::Object(object)) {
                Ok(params) => params,
                Err(_) => {
                    return json_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        "expected an object with an optional non-negative integer index",
                    );
                }
            };
        let downloads = match params.index {
            Some(index) => match self.0.downloads.get(index) {
                Some(item) => std::slice::from_ref(item),
                None => {
                    return json_error(
                        StatusCode::NOT_FOUND,
                        "download_not_found",
                        "download index is out of range",
                    );
                }
            },
            None => &self.0.downloads,
        };
        let Ok(_run) = self.0.run_lock.try_lock() else {
            return json_error(
                StatusCode::CONFLICT,
                "download_busy",
                "a download is already running",
            );
        };
        let (succeeded, failed) = self.0.download_batch(downloads).await;
        json_ok(
            StatusCode::OK,
            &json!({
                "ok": failed == 0,
                "total": downloads.len(),
                "succeeded": succeeded,
                "failed": failed,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use http_body_util::BodyExt;
    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;
    use tokio::time::timeout;

    use super::*;
    use crate::infra::network::http_client::{HttpClient, HttpClientOptions};
    use crate::plugin::executor::download::{DownloadMetrics, DownloadTarget};

    fn executor(dir: &Path, urls: &[String]) -> DownloadExecutor {
        DownloadExecutor {
            tag: "download_api_test".into(),
            client: HttpClient::new(HttpClientOptions::new(false, None)),
            timeout: Duration::from_secs(2),
            downloads: urls
                .iter()
                .enumerate()
                .map(|(index, url)| DownloadTarget {
                    url: url.clone(),
                    dir: dir.into(),
                    filename: format!("{index}.txt"),
                    path: dir.join(format!("{index}.txt")),
                })
                .collect(),
            insecure_skip_verify: false,
            socks5: None,
            metrics: Arc::new(DownloadMetrics::new("download_api_test".into())),
            run_lock: Arc::new(Mutex::new(())),
        }
    }

    async fn body(response: ApiResponse) -> Value {
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }

    async fn run(executor: &DownloadExecutor, payload: &str) -> ApiResponse {
        timeout(
            Duration::from_secs(5),
            DownloadRunHandler(executor.clone()).handle(
                Request::builder()
                    .method("POST")
                    .body(Bytes::copy_from_slice(payload.as_bytes()))
                    .unwrap(),
            ),
        )
        .await
        .unwrap()
    }

    async fn mock_server(statuses: Vec<u16>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            timeout(Duration::from_secs(5), async move {
                let mut paths = Vec::new();
                for status in statuses {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut request = Vec::new();
                    while !request.ends_with(b"\r\n\r\n") {
                        request.push(stream.read_u8().await.unwrap());
                    }
                    paths.push(String::from_utf8(request).unwrap().split_whitespace().nth(1).unwrap().into());
                    stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: 3\r\nConnection: close\r\n\r\nnew").as_bytes()).await.unwrap();
                }
                paths
            }).await.unwrap()
        });
        (url, task)
    }

    #[tokio::test]
    async fn manual_download_selects_only_requested_item_and_updates_metrics() {
        let dir = tempfile::tempdir().unwrap();
        let (url, server) = mock_server(vec![200]).await;
        let executor = executor(
            dir.path(),
            &[format!("{url}/first"), format!("{url}/second")],
        );
        std::fs::write(dir.path().join("1.txt"), "old").unwrap();
        let response = run(&executor, r#"{"index":1}"#).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            body(response).await,
            json!({"ok":true,"total":1,"succeeded":1,"failed":0})
        );
        assert_eq!(server.await.unwrap(), vec!["/second"]);
        assert!(!dir.path().join("0.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("1.txt")).unwrap(),
            "new"
        );
        assert_eq!(executor.metrics.success_total.load(Ordering::Relaxed), 1);
        assert!(executor.run_lock.try_lock().is_ok());
    }

    #[tokio::test]
    async fn manual_download_all_continues_after_failure_and_preserves_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let (url, server) = mock_server(vec![500, 200]).await;
        let executor = executor(
            dir.path(),
            &[format!("{url}/first"), format!("{url}/second")],
        );
        std::fs::write(dir.path().join("0.txt"), "old").unwrap();
        let response = run(&executor, "{}").await;
        assert_eq!(
            body(response).await,
            json!({"ok":false,"total":2,"succeeded":1,"failed":1})
        );
        assert_eq!(server.await.unwrap(), vec!["/first", "/second"]);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("0.txt")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("1.txt")).unwrap(),
            "new"
        );
        assert_eq!(executor.metrics.failure_total.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn manual_download_rejects_invalid_requests_and_overlapping_runs() {
        let dir = tempfile::tempdir().unwrap();
        let executor = executor(dir.path(), &["http://127.0.0.1:9/file".into()]);
        for payload in [
            "",
            "null",
            "[]",
            r#"{"index":-1}"#,
            r#"{"index":0.5}"#,
            r#"{"index":"0"}"#,
            r#"{"url":"http://example.com"}"#,
        ] {
            assert_eq!(
                run(&executor, payload).await.status(),
                StatusCode::BAD_REQUEST,
                "{payload}"
            );
        }
        assert_eq!(
            run(&executor, r#"{"index":1}"#).await.status(),
            StatusCode::NOT_FOUND
        );
        let guard = executor.run_lock.lock().await;
        assert_eq!(run(&executor, "{}").await.status(), StatusCode::CONFLICT);
        let response = DownloadListHandler(executor.clone())
            .handle(Request::new(Bytes::new()))
            .await;
        let list = body(response).await;
        assert_eq!(list["running"], true);
        assert_eq!(list["downloads"][0]["index"], 0);
        assert_eq!(list["downloads"][0]["url"], executor.downloads[0].url);
        drop(guard);
        assert_eq!(executor.metrics.failure_total.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn manual_download_timeout_is_reported_and_releases_lock() {
        let dir = tempfile::tempdir().unwrap();
        // A live listener with no response makes timeout deterministic.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut executor = executor(
            dir.path(),
            &[format!("http://{}/slow", listener.local_addr().unwrap())],
        );
        executor.timeout = Duration::from_millis(20);
        let response = run(&executor, "{}").await;
        assert_eq!(
            body(response).await,
            json!({"ok":false,"total":1,"succeeded":0,"failed":1})
        );
        assert_eq!(executor.metrics.timeout_total.load(Ordering::Relaxed), 1);
        assert!(executor.run_lock.try_lock().is_ok());
    }
}
