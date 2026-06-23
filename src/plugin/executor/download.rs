// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `download` executor plugin.
//!
//! Downloads one or more remote `http/https` files into local directories and
//! overwrites the target files after the new content is fully written.
//!
//! Execution semantics:
//! - each configured download runs sequentially in declaration order;
//! - a failed item logs a warning and does not stop later items;
//! - the executor always returns [`ExecStep::Next`]; and
//! - the DNS request/response itself is never mutated.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
#[cfg(feature = "api")]
use bytes::Bytes;
use futures::future::BoxFuture;
#[cfg(feature = "api")]
use http::{Request, StatusCode};
use serde::Deserialize;
#[cfg(feature = "api")]
use serde::Serialize;
use serde_yaml_ng::Value;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{info, warn};
use url::Url;

#[cfg(feature = "api")]
use crate::api::{ApiHandler, json_ok};
use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::http_client::{HttpClient, HttpClientOptions, HttpRequestOptions};
#[cfg(test)]
use crate::infra::network::proxy::{Socks5Opt, parse_optional_socks5};
use crate::infra::observability::metrics::{
    MetricLabel, MetricSample, MetricSink, MetricSource, register_metric_source,
    unregister_metric_source,
};
use crate::infra::system::deserialize_duration_option;
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;
#[cfg(feature = "api")]
use crate::register_plugin_api;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DownloadConfig {
    #[serde(default, deserialize_with = "deserialize_duration_option")]
    timeout: Option<Duration>,
    insecure_skip_verify: Option<bool>,
    outbound: Option<String>,
    socks5: Option<String>,
    startup_if_missing: Option<bool>,
    downloads: Vec<DownloadItemConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownloadItemConfig {
    url: String,
    dir: String,
    filename: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DownloadTarget {
    url: String,
    dir: PathBuf,
    filename: String,
    path: PathBuf,
}

#[derive(Debug)]
struct DownloadMetrics {
    tag: String,
    success_total: AtomicU64,
    failure_total: AtomicU64,
    timeout_total: AtomicU64,
}

impl DownloadMetrics {
    fn new(tag: String) -> Self {
        Self {
            tag,
            success_total: AtomicU64::new(0),
            failure_total: AtomicU64::new(0),
            timeout_total: AtomicU64::new(0),
        }
    }
}

impl MetricSource for DownloadMetrics {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn plugin_type(&self) -> &'static str {
        "download"
    }

    fn collect(&self, sink: &mut dyn MetricSink) {
        let labels = [MetricLabel::new("plugin_tag", self.tag.as_str())];
        sink.emit(MetricSample::counter(
            "download_success_total",
            "Total successful file downloads.",
            &labels,
            self.success_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "download_failure_total",
            "Total failed file downloads (excluding timeouts).",
            &labels,
            self.failure_total.load(Ordering::Relaxed),
        ));
        sink.emit(MetricSample::counter(
            "download_timeout_total",
            "Total file downloads that timed out.",
            &labels,
            self.timeout_total.load(Ordering::Relaxed),
        ));
    }
}

#[derive(Debug, Default, Clone)]
struct DownloadItemRunState {
    last_run_ms: Option<u64>,
    last_success_ms: Option<u64>,
    last_error: Option<String>,
    last_duration_ms: Option<u64>,
}

#[cfg_attr(feature = "api", derive(Serialize))]
#[derive(Debug, Clone)]
struct DownloadFileStatus {
    exists: bool,
    size_bytes: Option<u64>,
    modified_at_ms: Option<u64>,
}

#[cfg_attr(feature = "api", derive(Serialize))]
#[derive(Debug, Clone)]
struct DownloadItemStatus {
    index: usize,
    url: String,
    dir: String,
    filename: String,
    path: String,
    file: DownloadFileStatus,
    last_run_ms: Option<u64>,
    last_success_ms: Option<u64>,
    last_error: Option<String>,
    last_duration_ms: Option<u64>,
}

#[cfg_attr(feature = "api", derive(Serialize))]
#[derive(Debug, Clone)]
struct DownloadBatchResult {
    ok: bool,
    plugin: String,
    total: usize,
    success_count: usize,
    failure_count: usize,
    results: Vec<DownloadItemStatus>,
}

#[cfg_attr(feature = "api", derive(Serialize))]
#[derive(Debug, Clone)]
struct DownloadStatusResponse {
    ok: bool,
    plugin: String,
    timeout_ms: u128,
    insecure_skip_verify: bool,
    socks5: Option<String>,
    items: Vec<DownloadItemStatus>,
}

#[derive(Debug)]
struct DownloadRuntimeState {
    tag: String,
    client: HttpClient,
    timeout: Duration,
    downloads: Vec<DownloadTarget>,
    insecure_skip_verify: bool,
    socks5: Option<String>,
    metrics: Arc<DownloadMetrics>,
    item_states: Mutex<Vec<DownloadItemRunState>>,
    run_lock: Mutex<()>,
}

impl DownloadRuntimeState {
    fn new(
        tag: String,
        client: HttpClient,
        timeout: Duration,
        downloads: Vec<DownloadTarget>,
        insecure_skip_verify: bool,
        socks5: Option<String>,
        metrics: Arc<DownloadMetrics>,
    ) -> Self {
        let item_states = vec![DownloadItemRunState::default(); downloads.len()];
        Self {
            tag,
            client,
            timeout,
            downloads,
            insecure_skip_verify,
            socks5,
            metrics,
            item_states: Mutex::new(item_states),
            run_lock: Mutex::new(()),
        }
    }

    async fn download_one(&self, item: &DownloadTarget) -> Result<()> {
        self.client
            .download(
                HttpRequestOptions::from_url(item.url.as_str()),
                item.path.as_path(),
            )
            .await
    }

    async fn status(&self) -> DownloadStatusResponse {
        DownloadStatusResponse {
            ok: true,
            plugin: self.tag.clone(),
            timeout_ms: self.timeout.as_millis(),
            insecure_skip_verify: self.insecure_skip_verify,
            socks5: self.socks5.clone(),
            items: self.item_statuses().await,
        }
    }

    async fn run_batch(&self) -> DownloadBatchResult {
        let _guard = self.run_lock.lock().await;
        let mut success_count = 0usize;
        let mut failure_count = 0usize;

        for (index, item) in self.downloads.iter().enumerate() {
            let started_ms = AppClock::now_timestamp();
            let started_elapsed_ms = AppClock::elapsed_millis();
            let result = timeout(self.timeout, self.download_one(item)).await;
            let duration_ms = AppClock::elapsed_millis().saturating_sub(started_elapsed_ms);
            match result {
                Ok(Ok(())) => {
                    success_count += 1;
                    self.metrics.success_total.fetch_add(1, Ordering::Relaxed);
                    self.record_item_state(index, started_ms, Some(started_ms), None, duration_ms)
                        .await;
                    info!(
                        plugin = %self.tag,
                        url = %item.url,
                        target = %item.path.display(),
                        timeout_ms = self.timeout.as_millis(),
                        insecure_skip_verify = self.insecure_skip_verify,
                        socks5 = self.socks5.as_deref().unwrap_or(""),
                        "download completed"
                    );
                }
                Ok(Err(err)) => {
                    failure_count += 1;
                    self.metrics.failure_total.fetch_add(1, Ordering::Relaxed);
                    let message = err.to_string();
                    self.record_item_state(
                        index,
                        started_ms,
                        None,
                        Some(message.clone()),
                        duration_ms,
                    )
                    .await;
                    warn!(
                        plugin = %self.tag,
                        url = %item.url,
                        target = %item.path.display(),
                        error = %message,
                        "download failed; continuing with remaining items"
                    );
                }
                Err(_) => {
                    failure_count += 1;
                    self.metrics.timeout_total.fetch_add(1, Ordering::Relaxed);
                    let message =
                        format!("download timed out after {} ms", self.timeout.as_millis());
                    self.record_item_state(index, started_ms, None, Some(message), duration_ms)
                        .await;
                    warn!(
                        plugin = %self.tag,
                        url = %item.url,
                        target = %item.path.display(),
                        timeout_ms = self.timeout.as_millis(),
                        "download timed out; continuing with remaining items"
                    );
                }
            }
        }

        info!(
            plugin = %self.tag,
            successes = success_count,
            failures = failure_count,
            total = self.downloads.len(),
            "download batch finished"
        );

        DownloadBatchResult {
            ok: failure_count == 0,
            plugin: self.tag.clone(),
            total: self.downloads.len(),
            success_count,
            failure_count,
            results: self.item_statuses().await,
        }
    }

    async fn record_item_state(
        &self,
        index: usize,
        last_run_ms: u64,
        last_success_ms: Option<u64>,
        last_error: Option<String>,
        last_duration_ms: u64,
    ) {
        let mut states = self.item_states.lock().await;
        if let Some(state) = states.get_mut(index) {
            state.last_run_ms = Some(last_run_ms);
            if let Some(success_ms) = last_success_ms {
                state.last_success_ms = Some(success_ms);
            }
            state.last_error = last_error;
            state.last_duration_ms = Some(last_duration_ms);
        }
    }

    async fn item_statuses(&self) -> Vec<DownloadItemStatus> {
        let states = self.item_states.lock().await.clone();
        self.downloads
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let state = states.get(index).cloned().unwrap_or_default();
                DownloadItemStatus {
                    index,
                    url: item.url.clone(),
                    dir: item.dir.display().to_string(),
                    filename: item.filename.clone(),
                    path: item.path.display().to_string(),
                    file: file_status(&item.path),
                    last_run_ms: state.last_run_ms,
                    last_success_ms: state.last_success_ms,
                    last_error: state.last_error,
                    last_duration_ms: state.last_duration_ms,
                }
            })
            .collect()
    }
}

#[derive(Debug)]
struct DownloadExecutor {
    state: Arc<DownloadRuntimeState>,
}

#[async_trait]
impl Plugin for DownloadExecutor {
    fn tag(&self) -> &str {
        &self.state.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        register_metric_source(self.state.metrics.clone())?;
        register_download_api(self.state.clone())?;
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        unregister_metric_source(&self.state.tag);
        Ok(())
    }
}

#[async_trait]
impl Executor for DownloadExecutor {
    #[hotpath::measure]
    async fn execute(&self, _context: &mut DnsContext) -> Result<ExecStep> {
        self.state.run_batch().await;
        Ok(ExecStep::Next)
    }
}

#[derive(Debug, Clone)]
#[plugin_factory("download")]
pub struct DownloadFactory;

impl PluginFactory for DownloadFactory {
    fn prepare_startup<'a>(
        &'a self,
        plugin_config: &'a PluginConfig,
        _context: &'a crate::plugin::PluginBuildSession,
    ) -> BoxFuture<'a, Result<()>> {
        let plugin_tag = plugin_config.tag.clone();
        Box::pin(async move {
            let runtime = build_download_runtime_config(plugin_config)?;
            if !runtime.startup_if_missing {
                return Ok(());
            }

            let missing_targets = runtime
                .downloads
                .iter()
                .filter(|item| !item.path.exists())
                .collect::<Vec<_>>();

            if missing_targets.is_empty() {
                info!(
                    plugin = %plugin_tag,
                    total = runtime.downloads.len(),
                    "startup download skipped; all target files already exist"
                );
                return Ok(());
            }

            let executor = DownloadExecutor {
                state: Arc::new(DownloadRuntimeState::new(
                    plugin_tag.clone(),
                    HttpClient::new(runtime.http_options.clone()),
                    runtime.timeout,
                    runtime.downloads.clone(),
                    runtime.insecure_skip_verify,
                    runtime.raw_socks5,
                    Arc::new(DownloadMetrics::new(plugin_tag.clone())),
                )),
            };

            info!(
                plugin = %plugin_tag,
                missing = missing_targets.len(),
                total = runtime.downloads.len(),
                timeout_ms = runtime.timeout.as_millis(),
                "startup download began for missing target files"
            );

            for item in missing_targets {
                match timeout(executor.state.timeout, executor.state.download_one(item)).await {
                    Ok(Ok(())) => {
                        info!(
                            plugin = %executor.state.tag,
                            url = %item.url,
                            target = %item.path.display(),
                            "startup download completed for missing target"
                        );
                    }
                    Ok(Err(err)) => {
                        return Err(DnsError::plugin(format!(
                            "startup download failed for '{}' -> '{}': {}",
                            item.url,
                            item.path.display(),
                            err
                        )));
                    }
                    Err(_) => {
                        return Err(DnsError::plugin(format!(
                            "startup download timed out for '{}' -> '{}'",
                            item.url,
                            item.path.display()
                        )));
                    }
                }
            }

            Ok(())
        })
    }

    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let runtime = build_download_runtime_config(plugin_config)?;
        let metrics = Arc::new(DownloadMetrics::new(plugin_config.tag.clone()));

        Ok(UninitializedPlugin::Executor(Box::new(DownloadExecutor {
            state: Arc::new(DownloadRuntimeState::new(
                plugin_config.tag.clone(),
                HttpClient::new(runtime.http_options),
                runtime.timeout,
                runtime.downloads,
                runtime.insecure_skip_verify,
                runtime.raw_socks5,
                metrics,
            )),
        })))
    }

    fn quick_setup(&self, tag: &str, param: Option<String>) -> Result<UninitializedPlugin> {
        let raw = param.ok_or_else(|| {
            DnsError::plugin("download quick setup requires '<url> <dir>' arguments")
        })?;
        let (url, dir) = parse_quick_setup(raw.as_str())?;
        let downloads = resolve_download_targets(
            tag,
            vec![DownloadItemConfig {
                url,
                dir,
                filename: None,
            }],
        )?;

        Ok(UninitializedPlugin::Executor(Box::new(DownloadExecutor {
            state: Arc::new(DownloadRuntimeState::new(
                tag.to_string(),
                HttpClient::new(HttpClientOptions::from_outbound(
                    false,
                    None,
                    None,
                    |raw| DnsError::plugin(format!("invalid download socks5 proxy '{}'", raw)),
                )?),
                DEFAULT_TIMEOUT,
                downloads,
                false,
                None,
                Arc::new(DownloadMetrics::new(tag.to_string())),
            )),
        })))
    }
}

struct DownloadRuntimeConfig {
    timeout: Duration,
    downloads: Vec<DownloadTarget>,
    insecure_skip_verify: bool,
    startup_if_missing: bool,
    raw_socks5: Option<String>,
    http_options: HttpClientOptions,
}

fn build_download_runtime_config(plugin_config: &PluginConfig) -> Result<DownloadRuntimeConfig> {
    let cfg = plugin_config
        .args
        .clone()
        .ok_or_else(|| DnsError::plugin("download requires configuration arguments"))
        .and_then(parse_download_config)?;

    let insecure_skip_verify = cfg.insecure_skip_verify.unwrap_or(false);
    let http_options = HttpClientOptions::from_outbound(
        insecure_skip_verify,
        cfg.outbound.as_deref(),
        cfg.socks5.as_deref(),
        |raw| DnsError::plugin(format!("invalid download socks5 proxy '{}'", raw)),
    )?;

    Ok(DownloadRuntimeConfig {
        timeout: cfg.timeout.unwrap_or(DEFAULT_TIMEOUT),
        downloads: resolve_download_targets(&plugin_config.tag, cfg.downloads)?,
        insecure_skip_verify,
        startup_if_missing: cfg.startup_if_missing.unwrap_or(true),
        raw_socks5: cfg.socks5,
        http_options,
    })
}

fn parse_download_config(args: Value) -> Result<DownloadConfig> {
    serde_yaml_ng::from_value::<DownloadConfig>(args)
        .map_err(|e| DnsError::plugin(format!("failed to parse download config: {}", e)))
}

fn parse_quick_setup(raw: &str) -> Result<(String, String)> {
    let mut parts = raw.trim().splitn(2, char::is_whitespace);
    let url = parts
        .next()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .ok_or_else(|| DnsError::plugin("download quick setup requires a non-empty URL"))?;
    let dir = parts
        .next()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .ok_or_else(|| DnsError::plugin("download quick setup requires a non-empty directory"))?;
    Ok((url.to_string(), dir.to_string()))
}

fn resolve_download_targets(
    plugin_tag: &str,
    downloads: Vec<DownloadItemConfig>,
) -> Result<Vec<DownloadTarget>> {
    if downloads.is_empty() {
        return Err(DnsError::plugin(format!(
            "plugin '{}' download args.downloads must not be empty",
            plugin_tag
        )));
    }

    let mut targets = Vec::with_capacity(downloads.len());
    let mut seen_paths = HashSet::new();

    for (idx, item) in downloads.into_iter().enumerate() {
        let url = parse_download_url(plugin_tag, idx, item.url.as_str())?;
        let dir = item.dir.trim();
        if dir.is_empty() {
            return Err(DnsError::plugin(format!(
                "plugin '{}' field 'args.downloads[{}].dir' must not be empty",
                plugin_tag, idx
            )));
        }

        let filename = item
            .filename
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| filename_from_url(&url).unwrap_or_default());
        if filename.is_empty() {
            return Err(DnsError::plugin(format!(
                "plugin '{}' field 'args.downloads[{}]' could not derive filename from url '{}'",
                plugin_tag, idx, item.url
            )));
        }

        let dir_path = PathBuf::from(dir);
        let path = dir_path.join(&filename);
        let path_key = path.to_string_lossy().to_string();
        if !seen_paths.insert(path_key.clone()) {
            return Err(DnsError::plugin(format!(
                "plugin '{}' has duplicate download target path '{}'",
                plugin_tag, path_key
            )));
        }

        targets.push(DownloadTarget {
            url: url.to_string(),
            dir: dir_path,
            filename,
            path,
        });
    }

    Ok(targets)
}

fn file_status(path: &PathBuf) -> DownloadFileStatus {
    match std::fs::metadata(path) {
        Ok(metadata) => DownloadFileStatus {
            exists: true,
            size_bytes: Some(metadata.len()),
            modified_at_ms: metadata.modified().ok().and_then(system_time_ms),
        },
        Err(_) => DownloadFileStatus {
            exists: false,
            size_bytes: None,
            modified_at_ms: None,
        },
    }
}

fn system_time_ms(value: SystemTime) -> Option<u64> {
    value
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
}

#[cfg(feature = "api")]
#[derive(Debug)]
struct DownloadStatusHandler {
    state: Arc<DownloadRuntimeState>,
}

#[cfg(feature = "api")]
#[async_trait]
impl ApiHandler for DownloadStatusHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        json_ok(StatusCode::OK, &self.state.status().await)
    }
}

#[cfg(feature = "api")]
#[derive(Debug)]
struct DownloadRunHandler {
    state: Arc<DownloadRuntimeState>,
}

#[cfg(feature = "api")]
#[async_trait]
impl ApiHandler for DownloadRunHandler {
    async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
        json_ok(StatusCode::OK, &self.state.run_batch().await)
    }
}

fn register_download_api(state: Arc<DownloadRuntimeState>) -> Result<()> {
    register_plugin_api!(
        state.tag.as_str(),
        GET "/status" => DownloadStatusHandler {
            state: state.clone(),
        },
        POST "/run" => DownloadRunHandler {
            state,
        },
    )
}

fn parse_download_url(plugin_tag: &str, idx: usize, raw: &str) -> Result<Url> {
    let url = Url::parse(raw).map_err(|e| {
        DnsError::plugin(format!(
            "plugin '{}' field 'args.downloads[{}].url' is invalid: {}",
            plugin_tag, idx, e
        ))
    })?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        scheme => Err(DnsError::plugin(format!(
            "plugin '{}' field 'args.downloads[{}].url' uses unsupported scheme '{}'",
            plugin_tag, idx, scheme
        ))),
    }
}

fn filename_from_url(url: &Url) -> Option<String> {
    url.path_segments()
        .and_then(Iterator::last)
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
fn parse_socks5(raw: Option<&str>) -> Result<Option<Socks5Opt>> {
    parse_optional_socks5(raw, |raw| {
        DnsError::plugin(format!("invalid download socks5 proxy '{}'", raw))
    })
}

#[cfg(test)]
mod tests {
    use serde_yaml_ng::Value;

    use super::*;
    use crate::plugin::executor::ExecStep;
    use crate::plugin::test_utils::{plugin_config, test_context};

    #[test]
    fn test_parse_quick_setup_requires_url_and_dir() {
        assert!(parse_quick_setup("").is_err());
        assert!(parse_quick_setup("https://example.com/file.txt").is_err());
        let (url, dir) = parse_quick_setup("https://example.com/file.txt /tmp/downloads").unwrap();
        assert_eq!(url, "https://example.com/file.txt");
        assert_eq!(dir, "/tmp/downloads");
    }

    #[test]
    fn test_resolve_targets_rejects_invalid_values() {
        let err = resolve_download_targets("dl", Vec::new()).unwrap_err();
        assert!(err.to_string().contains("must not be empty"));

        let err = resolve_download_targets(
            "dl",
            vec![DownloadItemConfig {
                url: "ftp://example.com/file.txt".to_string(),
                dir: "/tmp".to_string(),
                filename: None,
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("unsupported scheme"));

        let err = resolve_download_targets(
            "dl",
            vec![DownloadItemConfig {
                url: "https://example.com/".to_string(),
                dir: "/tmp".to_string(),
                filename: None,
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("could not derive filename"));
    }

    #[test]
    fn test_resolve_targets_rejects_duplicate_paths() {
        let err = resolve_download_targets(
            "dl",
            vec![
                DownloadItemConfig {
                    url: "https://example.com/a.txt".to_string(),
                    dir: "/tmp".to_string(),
                    filename: Some("same.txt".to_string()),
                },
                DownloadItemConfig {
                    url: "https://example.com/b.txt".to_string(),
                    dir: "/tmp".to_string(),
                    filename: Some("same.txt".to_string()),
                },
            ],
        )
        .unwrap_err();
        assert!(err.to_string().contains("duplicate download target path"));
    }

    #[test]
    fn test_download_factory_create_rejects_invalid_config() {
        let factory = DownloadFactory;
        let cfg = plugin_config("download", "download", Some(Value::String("bad".into())));
        assert!(crate::plugin::test_utils::create_plugin_for_test(&factory, &cfg).is_err());
    }

    #[test]
    fn test_parse_socks5_accepts_and_rejects_values() {
        let parsed = parse_socks5(Some("127.0.0.1:1080")).expect("valid socks5 should parse");
        assert!(parsed.is_some());

        let err = parse_socks5(Some("bad")).expect_err("invalid socks5 should fail");
        assert!(err.to_string().contains("invalid download socks5 proxy"));
    }

    #[tokio::test]
    async fn test_download_executor_returns_next_for_empty_runtime_errors() {
        AppClock::start();

        let plugin = DownloadExecutor {
            state: Arc::new(DownloadRuntimeState::new(
                "download".to_string(),
                HttpClient::new(HttpClientOptions::new(false, None)),
                Duration::from_millis(10),
                vec![DownloadTarget {
                    url: "http://127.0.0.1:9/missing.txt".to_string(),
                    dir: PathBuf::from("/tmp"),
                    filename: "missing.txt".to_string(),
                    path: PathBuf::from("/tmp/missing.txt"),
                }],
                false,
                None,
                Arc::new(DownloadMetrics::new("download".to_string())),
            )),
        };
        let mut ctx = test_context();
        let step = plugin
            .execute(&mut ctx)
            .await
            .expect("execute should not fail");
        assert!(matches!(step, ExecStep::Next));
    }

    #[tokio::test]
    async fn download_status_reports_file_metadata() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("rules.txt");
        std::fs::write(&path, "||example.org^\n").expect("file should be written");
        let state = DownloadRuntimeState::new(
            "standard_filter_download".to_string(),
            HttpClient::new(HttpClientOptions::new(false, None)),
            Duration::from_secs(1),
            vec![DownloadTarget {
                url: "https://example.com/rules.txt".to_string(),
                dir: temp.path().to_path_buf(),
                filename: "rules.txt".to_string(),
                path,
            }],
            false,
            None,
            Arc::new(DownloadMetrics::new("standard_filter_download".to_string())),
        );

        let status = state.status().await;

        assert!(status.ok);
        assert_eq!(status.items.len(), 1);
        assert!(status.items[0].file.exists);
        assert_eq!(status.items[0].file.size_bytes, Some(15));
        assert!(status.items[0].file.modified_at_ms.is_some());
    }
}
