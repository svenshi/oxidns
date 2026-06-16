// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Release upgrade support shared by the CLI and executor plugin.

use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use http::header::{AUTHORIZATION, HeaderValue, USER_AGENT};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::time::timeout;
use tracing::info;

use crate::app::cli::{UpgradeAction, UpgradeOptions};
use crate::core::VERSION;
use crate::core::error::{DnsError, Result};
use crate::network::http_client::{
    DownloadProgress, HttpClient, HttpClientOptions, HttpRequestOptions,
};
use crate::network::proxy::parse_socks5_opt;
use crate::service;

const DEFAULT_REPOSITORY: &str = "svenshi/oxidns";
const DEFAULT_TARGET: &str = "latest";
const DEFAULT_CONFIG_FILE: &str = "config.yaml";
const DEFAULT_CACHE_DIR: &str = "./upgrade-cache";
const DEFAULT_BACKUP_DIR: &str = "./upgrade-backups";
const DEFAULT_WEBUI_DIR: &str = "./webui";
#[cfg(target_os = "linux")]
const DEFAULT_SERVICE_CONFIG: &str = "/etc/oxidns/config.yaml";
#[cfg(target_os = "linux")]
const DEFAULT_SERVICE_WORKING_DIR: &str = "/var/lib/oxidns";

const EXIT_RESTART_REQUIRED: i32 = 75;
const GITHUB_USER_AGENT: &str = "OxiDNS";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeBundle {
    #[default]
    Auto,
    Full,
    Minimal,
    Standard,
}

impl UpgradeBundle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Full => "full",
            Self::Minimal => "minimal",
            Self::Standard => "standard",
        }
    }

    pub fn from_user_value(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "full" => Ok(Self::Full),
            "minimal" => Ok(Self::Minimal),
            "standard" => Ok(Self::Standard),
            other => Err(DnsError::runtime(format!(
                "invalid upgrade bundle '{other}', expected auto, full, minimal, or standard"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpgradeConfig {
    pub target: String,
    pub repository: String,
    pub asset: String,
    pub bundle: UpgradeBundle,
    pub cache_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub webui_dir: PathBuf,
    pub skip_webui: bool,
    pub no_restart: bool,
    pub allow_prerelease: bool,
    pub force: bool,
    pub cleanup_after_apply: bool,
    pub timeout: Duration,
    pub socks5: Option<String>,
    pub insecure_skip_verify: bool,
    pub github_token: Option<String>,
}

impl Default for UpgradeConfig {
    fn default() -> Self {
        Self {
            target: DEFAULT_TARGET.to_string(),
            repository: DEFAULT_REPOSITORY.to_string(),
            asset: "auto".to_string(),
            bundle: UpgradeBundle::Auto,
            cache_dir: PathBuf::from(DEFAULT_CACHE_DIR),
            backup_dir: PathBuf::from(DEFAULT_BACKUP_DIR),
            webui_dir: PathBuf::from(DEFAULT_WEBUI_DIR),
            skip_webui: false,
            no_restart: false,
            allow_prerelease: false,
            force: false,
            cleanup_after_apply: false,
            timeout: Duration::from_secs(30),
            socks5: None,
            insecure_skip_verify: false,
            github_token: None,
        }
    }
}

impl UpgradeConfig {
    pub fn from_cli(options: &UpgradeOptions) -> Result<Self> {
        let path_defaults = CliPathDefaults::system()?;
        Self::from_cli_with_path_defaults(options, &path_defaults)
    }

    fn from_cli_with_path_defaults(
        options: &UpgradeOptions,
        path_defaults: &CliPathDefaults,
    ) -> Result<Self> {
        let path_context = resolve_cli_path_context(options, path_defaults);
        Ok(Self {
            target: options.target.clone(),
            repository: options.repository.clone(),
            asset: options.asset.clone(),
            bundle: options.bundle,
            cache_dir: options.cache_dir.clone(),
            backup_dir: options.backup_dir.clone(),
            webui_dir: resolve_cli_webui_dir(options, &path_context)?,
            skip_webui: options.skip_webui,
            no_restart: options.no_restart,
            allow_prerelease: options.allow_prerelease,
            force: options.force,
            cleanup_after_apply: false,
            timeout: options.timeout,
            socks5: options.socks5.clone(),
            insecure_skip_verify: options.insecure_skip_verify,
            github_token: options.github_token.clone(),
        })
    }
}

struct CliPathDefaults {
    current_dir: PathBuf,
    service_config: Option<PathBuf>,
    service_working_dir: Option<PathBuf>,
}

impl CliPathDefaults {
    fn system() -> Result<Self> {
        let current_dir = std::env::current_dir().map_err(|err| {
            DnsError::runtime(format!("failed to resolve current directory: {err}"))
        })?;
        Ok(Self {
            current_dir,
            service_config: default_service_config_path(),
            service_working_dir: default_service_working_dir(),
        })
    }
}

struct CliPathContext {
    config_path: Option<PathBuf>,
    config_explicit: bool,
    working_dir: PathBuf,
}

fn resolve_cli_path_context(
    options: &UpgradeOptions,
    defaults: &CliPathDefaults,
) -> CliPathContext {
    let explicit_working_dir = options
        .working_dir
        .as_ref()
        .map(|path| resolve_path(&defaults.current_dir, path));
    let config_explicit = options.config.is_some();
    let config_path = if let Some(config) = &options.config {
        let base = explicit_working_dir
            .as_deref()
            .unwrap_or(defaults.current_dir.as_path());
        Some(resolve_path(base, config))
    } else {
        let cwd_config = defaults.current_dir.join(DEFAULT_CONFIG_FILE);
        if cwd_config.is_file() {
            Some(cwd_config)
        } else {
            defaults
                .service_config
                .as_ref()
                .filter(|path| path.is_file())
                .cloned()
        }
    };

    let working_dir = explicit_working_dir.unwrap_or_else(|| {
        if config_path
            .as_ref()
            .zip(defaults.service_config.as_ref())
            .is_some_and(|(config, service_config)| same_path(config, service_config))
            && let Some(service_working_dir) = defaults
                .service_working_dir
                .as_ref()
                .filter(|path| path.is_dir())
        {
            return service_working_dir.clone();
        }
        defaults.current_dir.clone()
    });

    CliPathContext {
        config_path,
        config_explicit,
        working_dir,
    }
}

fn resolve_cli_webui_dir(options: &UpgradeOptions, context: &CliPathContext) -> Result<PathBuf> {
    if let Some(webui_dir) = &options.webui_dir {
        return Ok(resolve_path(&context.working_dir, webui_dir));
    }
    if options.skip_webui {
        return Ok(resolve_path(
            &context.working_dir,
            Path::new(DEFAULT_WEBUI_DIR),
        ));
    }

    if let Some(config_path) = &context.config_path {
        match read_config_webui_root(config_path) {
            Ok(Some(root)) => return Ok(resolve_path(&context.working_dir, Path::new(&root))),
            Ok(None) => {}
            Err(err) if context.config_explicit => return Err(err),
            Err(_) => {}
        }
    }

    Ok(resolve_path(
        &context.working_dir,
        Path::new(DEFAULT_WEBUI_DIR),
    ))
}

fn read_config_webui_root(config_path: &Path) -> Result<Option<String>> {
    let string = fs::read_to_string(config_path).map_err(|err| {
        DnsError::config(format!(
            "failed to read upgrade config {}: {}",
            config_path.display(),
            err
        ))
    })?;
    let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&string).map_err(|err| {
        DnsError::config(format!(
            "failed to parse upgrade config {}: {}",
            config_path.display(),
            err
        ))
    })?;
    crate::config::env_expand::expand_env_in_value(&mut value).map_err(|err| {
        DnsError::config(format!(
            "env expansion failed in upgrade config {}: {}",
            config_path.display(),
            err
        ))
    })?;
    let config: UpgradeRuntimeConfig = serde_yaml_ng::from_value(value).map_err(|err| {
        DnsError::config(format!(
            "failed to deserialize upgrade config {}: {}",
            config_path.display(),
            err
        ))
    })?;
    let root = config
        .api
        .and_then(|api| api.http)
        .and_then(|http| match http {
            UpgradeRuntimeHttpConfig::Listen(_) => None,
            UpgradeRuntimeHttpConfig::Detailed(config) => config.webui.map(|webui| webui.root),
        });
    let Some(root) = root else {
        return Ok(None);
    };
    let root = root.trim();
    if root.is_empty() {
        return Err(DnsError::config(format!(
            "api.http.webui.root cannot be empty in {}",
            config_path.display()
        )));
    }
    Ok(Some(root.to_string()))
}

#[derive(Debug, Deserialize)]
struct UpgradeRuntimeConfig {
    api: Option<UpgradeRuntimeApiConfig>,
}

#[derive(Debug, Deserialize)]
struct UpgradeRuntimeApiConfig {
    http: Option<UpgradeRuntimeHttpConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum UpgradeRuntimeHttpConfig {
    Listen(String),
    Detailed(UpgradeRuntimeHttpDetailedConfig),
}

#[derive(Debug, Deserialize)]
struct UpgradeRuntimeHttpDetailedConfig {
    webui: Option<UpgradeRuntimeWebUiConfig>,
}

#[derive(Debug, Deserialize)]
struct UpgradeRuntimeWebUiConfig {
    root: String,
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    normalize_path(&path)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(target_os = "linux")]
fn default_service_config_path() -> Option<PathBuf> {
    Some(PathBuf::from(DEFAULT_SERVICE_CONFIG))
}

#[cfg(not(target_os = "linux"))]
fn default_service_config_path() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
fn default_service_working_dir() -> Option<PathBuf> {
    Some(PathBuf::from(DEFAULT_SERVICE_WORKING_DIR))
}

#[cfg(not(target_os = "linux"))]
fn default_service_working_dir() -> Option<PathBuf> {
    None
}

#[derive(Debug, Clone)]
pub struct UpgradeDownload {
    pub version: String,
    pub asset_name: String,
    pub archive_path: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct UpgradeCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub asset_name: String,
    pub release_url: String,
}

#[derive(Debug, Clone)]
pub struct ApplyOutcome {
    pub installed_version: String,
    pub asset_name: String,
    pub backup_path: PathBuf,
    pub binary_path: PathBuf,
    /// `Some` when the WebUI directory was installed; `None` when skipped or
    /// when the archive did not contain a `webui/` directory.
    pub webui_path: Option<PathBuf>,
    /// `Some` when an existing WebUI directory was backed up before the swap;
    /// `None` on a fresh install where there was nothing to back up.
    pub webui_backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeContext {
    Cli,
    Plugin,
}

pub fn run_cli(action: UpgradeAction, config: UpgradeConfig) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| DnsError::runtime(format!("failed to create upgrade runtime: {err}")))?;

    runtime.block_on(async move {
        match action {
            UpgradeAction::Check => {
                print_cli_plan("check", &config);
                println!("Checking GitHub release metadata...");
                let check = check(&config).await?;
                println!(
                    "Current: {}, release: {}, asset: {}, update_available: {}",
                    check.current_version,
                    check.latest_version,
                    check.asset_name,
                    check.update_available
                );
                println!("Release: {}", check.release_url);
            }
            UpgradeAction::Download => {
                print_cli_plan("download", &config);
                println!("Resolving release asset...");
                println!("Downloading archive without checking the current version...");
                let progress_reporter = UpgradeDownloadProgressReporter::new(UpgradeContext::Cli);
                let download = download(&config, move |progress| {
                    progress_reporter.report(progress);
                })
                .await?;
                println!(
                    "Downloaded {} as {}",
                    download.asset_name,
                    download.archive_path.display()
                );
                println!("SHA256: {}", download.sha256);
                println!("Archive verified successfully.");
            }
            UpgradeAction::Apply => {
                print_cli_plan("apply", &config);
                println!("Checking whether an upgrade is needed...");
                match should_apply(&config).await? {
                    ApplyDecision::Apply { check } => {
                        if config.force {
                            println!(
                                "Force enabled: applying release {} even if it is not newer than current {}.",
                                check.latest_version, check.current_version
                            );
                        } else {
                            println!(
                                "Update available: current {}, release {}, asset {}.",
                                check.current_version, check.latest_version, check.asset_name
                            );
                        }
                        println!("Downloading, verifying, and replacing the current binary...");
                        let outcome = apply_unchecked(&config, UpgradeContext::Cli).await?;
                        if !config.no_restart {
                            println!("Service restart completed.");
                        }
                        println!(
                            "Installed {} from {}",
                            outcome.installed_version, outcome.asset_name
                        );
                        println!("Binary: {}", outcome.binary_path.display());
                        println!("Backup: {}", outcome.backup_path.display());
                        match &outcome.webui_path {
                            Some(path) => println!("WebUI: {}", path.display()),
                            None => println!("WebUI: not upgraded"),
                        }
                        if let Some(path) = &outcome.webui_backup_path {
                            println!("WebUI backup: {}", path.display());
                        }
                        if prompt_cleanup_after_apply()? {
                            match cleanup_upgrade_artifacts(&config) {
                                Ok(cleaned) => {
                                    if cleaned.is_empty() {
                                        println!("No backup or cache directories to clean.");
                                    } else {
                                        for path in cleaned {
                                            println!("Cleaned: {}", path.display());
                                        }
                                    }
                                }
                                Err(err) => {
                                    println!("Cleanup failed: {err}");
                                }
                            }
                        } else {
                            println!("Cleanup skipped.");
                        }
                    }
                    ApplyDecision::Skip { check } => {
                        println!(
                            "No update available: current {}, release {}, asset {}",
                            check.current_version, check.latest_version, check.asset_name
                        );
                    }
                }
            }
        }
        Ok(())
    })
}

fn print_cli_plan(action: &str, config: &UpgradeConfig) {
    println!("OxiDNS upgrade {action}");
    println!("Repository: {}", config.repository);
    println!("Target: {}", config.target);
    println!("Asset: {}", config.asset);
    println!("Bundle: {}", config.bundle.as_str());
    println!("Cache: {}", config.cache_dir.display());
    if action == "apply" {
        println!("Backup: {}", config.backup_dir.display());
        println!("No restart: {}", config.no_restart);
        println!("Force: {}", config.force);
    }
    if action == "apply" || action == "check" {
        if config.skip_webui {
            println!("WebUI: skipped (--skip-webui)");
        } else {
            println!("WebUI: {}", config.webui_dir.display());
        }
    }
    println!("Timeout: {:?}", config.timeout);
    if let Some(socks5) = config.socks5.as_deref() {
        println!("SOCKS5: {}", socks5);
    }
    if config.insecure_skip_verify {
        println!("TLS verification: disabled");
    }
}

pub async fn check(config: &UpgradeConfig) -> Result<UpgradeCheck> {
    let release = fetch_release(config).await?;
    let asset = select_asset(config, &release)?;
    let current_version = VERSION.to_string();
    let latest_version = release.version_string();
    let update_available = is_newer_version(&latest_version, &current_version);
    Ok(UpgradeCheck {
        current_version,
        latest_version,
        update_available,
        asset_name: asset.name.clone(),
        release_url: release.html_url.unwrap_or_default(),
    })
}

#[derive(Debug, Clone)]
pub enum ApplyDecision {
    Apply { check: UpgradeCheck },
    Skip { check: UpgradeCheck },
}

#[derive(Debug, Clone)]
pub enum ApplyRunOutcome {
    Applied {
        check: UpgradeCheck,
        outcome: ApplyOutcome,
    },
    Skipped {
        check: UpgradeCheck,
    },
}

pub async fn should_apply(config: &UpgradeConfig) -> Result<ApplyDecision> {
    let check = check(config).await?;
    if config.force || check.update_available {
        Ok(ApplyDecision::Apply { check })
    } else {
        Ok(ApplyDecision::Skip { check })
    }
}

async fn download<F>(config: &UpgradeConfig, progress: F) -> Result<UpgradeDownload>
where
    F: FnMut(DownloadProgress),
{
    let release = fetch_release(config).await?;
    let asset = select_asset(config, &release)?;
    let expected = sha256_from_asset_digest(asset)?;
    let client = build_asset_http_client(config)?;
    fs::create_dir_all(&config.cache_dir).map_err(|err| {
        DnsError::runtime(format!(
            "failed to create upgrade cache directory '{}': {}",
            config.cache_dir.display(),
            err
        ))
    })?;

    let archive_path = config.cache_dir.join(&asset.name);
    timeout(
        config.timeout,
        client.download_with_progress(
            HttpRequestOptions::from_url(asset.browser_download_url.as_str())
                .with_headers(github_request_headers(config.github_token.as_deref())),
            &archive_path,
            progress,
        ),
    )
    .await
    .map_err(|_| DnsError::runtime("upgrade archive download timed out"))??;

    verify_sha256(&archive_path, &expected)?;
    Ok(UpgradeDownload {
        version: release.version_string(),
        asset_name: asset.name.clone(),
        archive_path,
        sha256: expected,
    })
}

pub async fn apply(
    config: &UpgradeConfig,
    restart_context: UpgradeContext,
) -> Result<ApplyRunOutcome> {
    let decision = should_apply(config).await?;
    apply_decision(config, restart_context, decision).await
}

pub async fn apply_decision(
    config: &UpgradeConfig,
    restart_context: UpgradeContext,
    decision: ApplyDecision,
) -> Result<ApplyRunOutcome> {
    match decision {
        ApplyDecision::Apply { check } => {
            let outcome = apply_unchecked(config, restart_context).await?;
            Ok(ApplyRunOutcome::Applied { check, outcome })
        }
        ApplyDecision::Skip { check } => Ok(ApplyRunOutcome::Skipped { check }),
    }
}

async fn apply_unchecked(
    config: &UpgradeConfig,
    restart_context: UpgradeContext,
) -> Result<ApplyOutcome> {
    print_cli_apply_step(restart_context, "Acquiring upgrade lock...");
    let lock_path = config.cache_dir.join(".upgrade.lock");
    fs::create_dir_all(&config.cache_dir)?;
    let lock_file = File::create(&lock_path).map_err(|err| {
        DnsError::runtime(format!(
            "failed to create upgrade lock '{}': {}",
            lock_path.display(),
            err
        ))
    })?;
    lock_file.try_lock_exclusive().map_err(|err| {
        DnsError::runtime(format!("another upgrade appears to be running: {err}"))
    })?;

    print_cli_apply_step(
        restart_context,
        "Downloading archive and verifying GitHub asset digest...",
    );
    let progress_reporter = UpgradeDownloadProgressReporter::new(restart_context);
    let downloaded = download(config, move |progress| {
        progress_reporter.report(progress);
    })
    .await?;
    print_cli_apply_step(
        restart_context,
        format!(
            "Archive ready: {} (sha256 {})",
            downloaded.archive_path.display(),
            downloaded.sha256
        ),
    );

    #[cfg(not(windows))]
    if !downloaded.asset_name.ends_with(".tar.gz") {
        return Err(DnsError::runtime(format!(
            "upgrade apply requires a .tar.gz asset, got '{}'",
            downloaded.asset_name
        )));
    }
    #[cfg(windows)]
    if !downloaded.asset_name.ends_with(".zip") {
        return Err(DnsError::runtime(format!(
            "upgrade apply requires a .zip asset, got '{}'",
            downloaded.asset_name
        )));
    }

    let unpack_dir = config.cache_dir.join(format!(
        ".unpack-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    ));
    if unpack_dir.exists() {
        fs::remove_dir_all(&unpack_dir)?;
    }
    fs::create_dir_all(&unpack_dir)?;
    print_cli_apply_step(
        restart_context,
        format!("Unpacking archive into {}...", unpack_dir.display()),
    );
    #[cfg(not(windows))]
    unpack_tar_gz(&downloaded.archive_path, &unpack_dir)?;
    #[cfg(windows)]
    unpack_zip(&downloaded.archive_path, &unpack_dir)?;

    #[cfg(not(windows))]
    let extracted = find_extracted_binary(&unpack_dir)?;
    #[cfg(windows)]
    let extracted = find_extracted_binary_windows(&unpack_dir)?;

    let current_exe = std::env::current_exe()
        .map_err(|err| DnsError::runtime(format!("failed to resolve current exe: {err}")))?;
    fs::create_dir_all(&config.backup_dir)?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    #[cfg(not(windows))]
    let backup_path = config.backup_dir.join(format!("oxidns-{}-{}", VERSION, ts));
    #[cfg(windows)]
    let backup_path = config
        .backup_dir
        .join(format!("oxidns-{}-{}.exe", VERSION, ts));

    print_cli_apply_step(
        restart_context,
        format!("Creating backup at {}...", backup_path.display()),
    );
    print_cli_apply_step(
        restart_context,
        format!("Replacing binary at {}...", current_exe.display()),
    );
    #[cfg(not(windows))]
    {
        fs::copy(&current_exe, &backup_path).map_err(|err| {
            DnsError::runtime(format!(
                "failed to create binary backup '{}': {}",
                backup_path.display(),
                err
            ))
        })?;
        if let Err(err) = replace_binary(&extracted, &current_exe) {
            let _ = fs::copy(&backup_path, &current_exe);
            return Err(err);
        }
    }
    // Windows: rename running exe to backup then place new binary at original path.
    // replace_binary_windows() handles backup creation and rollback atomically.
    #[cfg(windows)]
    replace_binary_windows(&extracted, &current_exe, &backup_path)?;
    print_cli_apply_step(restart_context, "Binary replacement completed.");

    let (webui_path, webui_backup_path) = if config.skip_webui {
        print_cli_apply_step(restart_context, "Skipping WebUI upgrade (--skip-webui).");
        (None, None)
    } else {
        match find_extracted_webui(&unpack_dir) {
            None => {
                print_cli_apply_step(
                    restart_context,
                    "Archive contains no webui directory; skipping WebUI upgrade.",
                );
                (None, None)
            }
            Some(src) => {
                print_cli_apply_step(
                    restart_context,
                    format!("Installing WebUI into {}...", config.webui_dir.display()),
                );
                let (path, backup) = replace_webui(
                    &src,
                    &config.webui_dir,
                    &config.backup_dir,
                    &downloaded.version,
                )?;
                print_cli_apply_step(restart_context, "WebUI upgrade completed.");
                (Some(path), backup)
            }
        }
    };

    if config.cleanup_after_apply {
        let _ = cleanup_upgrade_artifacts(config);
    }

    if !config.no_restart {
        print_cli_apply_step(restart_context, "Restarting installed service...");
        restart_after_apply(restart_context)?;
    }

    Ok(ApplyOutcome {
        installed_version: downloaded.version,
        asset_name: downloaded.asset_name,
        backup_path,
        binary_path: current_exe,
        webui_path,
        webui_backup_path,
    })
}

#[derive(Clone)]
struct UpgradeDownloadProgressReporter {
    restart_context: UpgradeContext,
    state: std::sync::Arc<std::sync::Mutex<UpgradeDownloadProgressState>>,
}

#[derive(Debug, Default)]
struct UpgradeDownloadProgressState {
    last_percent_bucket: Option<u64>,
    last_unknown_bucket: u64,
}

impl UpgradeDownloadProgressReporter {
    fn new(restart_context: UpgradeContext) -> Self {
        Self {
            restart_context,
            state: Default::default(),
        }
    }

    fn report(&self, progress: DownloadProgress) {
        match self.restart_context {
            UpgradeContext::Cli => self.report_cli(progress),
            UpgradeContext::Plugin => self.report_plugin(progress),
        }
    }

    fn report_cli(&self, progress: DownloadProgress) {
        match progress.total {
            Some(total) if total > 0 => {
                let percent = progress.downloaded.saturating_mul(100) / total;
                print!(
                    "\rDownload progress: {}% ({}/{})",
                    percent,
                    format_bytes(progress.downloaded),
                    format_bytes(total)
                );
                let _ = std::io::stdout().flush();
                if progress.downloaded >= total {
                    println!();
                }
            }
            _ => {
                print!("\rDownload progress: {}", format_bytes(progress.downloaded));
                let _ = std::io::stdout().flush();
            }
        }
    }

    fn report_plugin(&self, progress: DownloadProgress) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };

        match progress.total {
            Some(total) if total > 0 => {
                let percent = progress.downloaded.saturating_mul(100) / total;
                let bucket = (percent / 10) * 10;
                let should_log = state.last_percent_bucket != Some(bucket)
                    || progress.downloaded >= total && state.last_percent_bucket != Some(100);
                if should_log {
                    state.last_percent_bucket = Some(bucket);
                    info!(
                        downloaded = progress.downloaded,
                        total, percent, "upgrade archive download progress"
                    );
                }
            }
            _ => {
                let bucket = progress.downloaded / (1024 * 1024);
                if bucket > state.last_unknown_bucket {
                    state.last_unknown_bucket = bucket;
                    info!(
                        downloaded = progress.downloaded,
                        "upgrade archive download progress"
                    );
                }
            }
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.1} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.1} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.1} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn prompt_cleanup_after_apply() -> Result<bool> {
    loop {
        print!("Clean backup and cache directories? (Y/n): ");
        std::io::stdout()
            .flush()
            .map_err(|err| DnsError::runtime(format!("failed to flush stdout: {err}")))?;

        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|err| DnsError::runtime(format!("failed to read cleanup choice: {err}")))?;

        match input.trim().to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Please answer Y or n."),
        }
    }
}

fn cleanup_upgrade_artifacts(config: &UpgradeConfig) -> Result<Vec<PathBuf>> {
    let mut cleaned = Vec::new();
    cleanup_dir_if_exists(&config.cache_dir, &mut cleaned)?;
    if config.backup_dir != config.cache_dir {
        cleanup_dir_if_exists(&config.backup_dir, &mut cleaned)?;
    }
    Ok(cleaned)
}

fn cleanup_dir_if_exists(path: &Path, cleaned: &mut Vec<PathBuf>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    fs::remove_dir_all(path).map_err(|err| {
        DnsError::runtime(format!(
            "failed to remove upgrade directory '{}': {}",
            path.display(),
            err
        ))
    })?;
    cleaned.push(path.to_path_buf());
    Ok(())
}

fn print_cli_apply_step(restart_context: UpgradeContext, message: impl AsRef<str>) {
    match restart_context {
        UpgradeContext::Cli => println!("{}", message.as_ref()),
        UpgradeContext::Plugin => info!(message = message.as_ref(), "upgrade apply step"),
    }
}

fn restart_after_apply(restart_context: UpgradeContext) -> Result<()> {
    match restart_context {
        // CLI is a separate process; ask the platform service manager to restart
        // the running daemon.
        UpgradeContext::Cli => service::restart_installed_service(),
        // Plugin runs inside the server process: signal the main event loop to
        // do a graceful shutdown + exec_restart(), which loads the new binary
        // already on disk. Fall back to exit(75) if the controller is gone.
        UpgradeContext::Plugin => {
            crate::plugin::request_app_restart()
                .unwrap_or_else(|_| std::process::exit(EXIT_RESTART_REQUIRED));
            Ok(())
        }
    }
}

#[cfg(windows)]
fn unpack_zip(archive: &std::path::Path, out_dir: &std::path::Path) -> Result<()> {
    let file = File::open(archive).map_err(|e| {
        DnsError::runtime(format!("failed to open zip '{}': {e}", archive.display()))
    })?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| DnsError::runtime(format!("failed to read zip archive: {e}")))?;
    // Canonicalize `out_dir` once so the post-join containment check is
    // resilient to relative components and current-dir changes.
    let out_dir_canon = fs::canonicalize(out_dir).map_err(|e| {
        DnsError::runtime(format!(
            "failed to canonicalize unpack dir '{}': {e}",
            out_dir.display()
        ))
    })?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| DnsError::runtime(format!("failed to access zip entry {i}: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        // `enclosed_name()` rejects absolute paths and `..` components that
        // would escape the unpack root, mitigating zip-slip on Windows where
        // backslashes and drive letters add extra footguns. Treat any
        // rejected entry as a hard error so a malicious archive cannot
        // silently skip files and leave the install in a half-applied state.
        let Some(rel_path) = entry.enclosed_name() else {
            return Err(DnsError::runtime(format!(
                "refusing to extract zip entry with unsafe path: '{}'",
                entry.name()
            )));
        };
        let dest = out_dir_canon.join(&rel_path);
        // Defense in depth: ensure the resolved parent stays under
        // `out_dir_canon` even after the join. `enclosed_name()` already
        // enforces this, but the extra check protects against future zip
        // crate behavior changes and any host-side symlink trickery.
        let parent = dest.parent().unwrap_or(&out_dir_canon);
        fs::create_dir_all(parent).map_err(|e| {
            DnsError::runtime(format!("failed to create '{}': {e}", parent.display()))
        })?;
        if !parent.starts_with(&out_dir_canon) {
            return Err(DnsError::runtime(format!(
                "refusing to extract zip entry outside unpack dir: '{}'",
                rel_path.display()
            )));
        }
        let mut out = File::create(&dest).map_err(|e| {
            DnsError::runtime(format!("failed to create '{}': {e}", dest.display()))
        })?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| DnsError::runtime(format!("failed to extract '{}': {e}", entry.name())))?;
    }
    Ok(())
}

#[cfg(windows)]
fn find_extracted_binary_windows(unpack_dir: &std::path::Path) -> Result<PathBuf> {
    let candidate = unpack_dir.join("oxidns.exe");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(DnsError::runtime(format!(
        "archive did not contain oxidns.exe at '{}'",
        candidate.display()
    )))
}

/// Windows binary replacement using the rename trick.
///
/// Windows prevents overwriting a running executable but allows renaming it.
/// This function stages the new binary first, renames the running exe to the
/// backup path, then moves the staged binary to the original path.
/// `current_exe()` returns the original path even after the rename, so
/// `exec_restart()` naturally loads the new binary on its next spawn.
#[cfg(windows)]
fn replace_binary_windows(source: &Path, target: &Path, backup_path: &Path) -> Result<()> {
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let staging = target.with_extension("upgrade-new.exe");
    fs::copy(source, &staging).map_err(|e| {
        DnsError::runtime(format!(
            "failed to stage new binary '{}': {e}",
            staging.display()
        ))
    })?;
    // Rename the running exe to backup (allowed by Windows even while running).
    if let Err(e) = fs::rename(target, backup_path) {
        let _ = fs::remove_file(&staging);
        return Err(DnsError::runtime(format!(
            "failed to move running binary to backup '{}': {e}",
            backup_path.display()
        )));
    }
    // Move staged binary to the original path.
    if let Err(e) = fs::rename(&staging, target) {
        let _ = fs::rename(backup_path, target); // attempt rollback
        let _ = fs::remove_file(&staging);
        return Err(DnsError::runtime(format!(
            "failed to place new binary at '{}': {e}",
            target.display()
        )));
    }
    Ok(())
}

async fn fetch_release(config: &UpgradeConfig) -> Result<GitHubRelease> {
    let url = if config.target.trim() == "latest" {
        format!(
            "https://api.github.com/repos/{}/releases/latest",
            config.repository
        )
    } else {
        format!(
            "https://api.github.com/repos/{}/releases/tags/{}",
            config.repository,
            config.target.trim()
        )
    };
    let client = build_asset_http_client(config)?;
    let response = timeout(
        config.timeout,
        client.get_request(
            HttpRequestOptions::from_url(url.as_str())
                .with_headers(github_request_headers(config.github_token.as_deref())),
        ),
    )
    .await
    .map_err(|_| DnsError::runtime("GitHub release request timed out"))??;
    let release = serde_json::from_slice::<GitHubRelease>(&response.body).map_err(|err| {
        DnsError::runtime(format!("failed to parse GitHub release response: {err}"))
    })?;
    if release.prerelease && !config.allow_prerelease {
        return Err(DnsError::runtime(format!(
            "release '{}' is a prerelease; pass allow_prerelease to use it",
            release.tag_name
        )));
    }
    Ok(release)
}

fn github_request_headers(token: Option<&str>) -> Vec<(http::header::HeaderName, HeaderValue)> {
    let mut headers = vec![(USER_AGENT, HeaderValue::from_static(GITHUB_USER_AGENT))];
    if let Some(token) = token.map(str::trim).filter(|token| !token.is_empty())
        && let Ok(value) = HeaderValue::try_from(format!("Bearer {token}"))
    {
        headers.push((AUTHORIZATION, value));
    }
    headers
}

fn build_asset_http_client(config: &UpgradeConfig) -> Result<HttpClient> {
    let socks5 =
        match config
            .socks5
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            Some(raw) => Some(parse_socks5_opt(raw).ok_or_else(|| {
                DnsError::runtime(format!("invalid upgrade socks5 proxy '{raw}'"))
            })?),
            None => None,
        };
    Ok(HttpClient::new(HttpClientOptions {
        insecure_skip_verify: config.insecure_skip_verify,
        socks5,
    }))
}

fn select_asset<'a>(
    config: &UpgradeConfig,
    release: &'a GitHubRelease,
) -> Result<&'a ReleaseAsset> {
    if config.asset.trim() != "auto" {
        return find_asset(release, config.asset.trim());
    }
    let expected = current_archive_name(config.bundle)?;
    find_asset(release, &expected)
}

fn find_asset<'a>(release: &'a GitHubRelease, name: &str) -> Result<&'a ReleaseAsset> {
    release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| {
            DnsError::runtime(format!(
                "release '{}' does not contain asset '{}'",
                release.tag_name, name
            ))
        })
}

fn current_archive_name(bundle: UpgradeBundle) -> Result<String> {
    let selected = resolve_requested_bundle(bundle, crate::build_info::PRIMARY_BUNDLE)?;
    let target = current_release_target()?;
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    archive_name_for_bundle(selected, &target, ext)
}

fn resolve_requested_bundle(
    requested: UpgradeBundle,
    primary_bundle: &str,
) -> Result<UpgradeBundle> {
    match requested {
        UpgradeBundle::Auto => match primary_bundle {
            "full" => Ok(UpgradeBundle::Full),
            "minimal" => Ok(UpgradeBundle::Minimal),
            "standard" => Ok(UpgradeBundle::Standard),
            "custom" => Err(DnsError::runtime(
                "current build bundle is custom; pass --bundle full|minimal|standard or --asset <NAME>",
            )),
            other => Err(DnsError::runtime(format!(
                "unsupported current build bundle '{other}'; pass --bundle full|minimal|standard or --asset <NAME>"
            ))),
        },
        bundle => Ok(bundle),
    }
}

fn archive_name_for_bundle(bundle: UpgradeBundle, target: &str, ext: &str) -> Result<String> {
    match bundle {
        UpgradeBundle::Full => Ok(format!("oxidns-{target}.{ext}")),
        UpgradeBundle::Minimal | UpgradeBundle::Standard => {
            Ok(format!("oxidns-{}-{target}.{ext}", bundle.as_str()))
        }
        UpgradeBundle::Auto => Err(DnsError::runtime(
            "upgrade bundle auto must be resolved before archive naming",
        )),
    }
}

fn current_release_target() -> Result<String> {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "x86" => "i686",
        "arm" => "arm",
        other => {
            return Err(DnsError::runtime(format!(
                "unsupported upgrade architecture '{other}'"
            )));
        }
    };
    let target = match std::env::consts::OS {
        "linux" => {
            if arch == "arm" {
                "arm-unknown-linux-musleabihf".to_string()
            } else {
                format!("{arch}-unknown-linux-musl")
            }
        }
        "macos" => format!("{arch}-apple-darwin"),
        "freebsd" => format!("{arch}-unknown-freebsd"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        other => {
            return Err(DnsError::runtime(format!(
                "unsupported upgrade OS '{other}'"
            )));
        }
    };
    Ok(target)
}

fn sha256_from_asset_digest(asset: &ReleaseAsset) -> Result<String> {
    let raw = asset.digest.as_deref().ok_or_else(|| {
        DnsError::runtime(format!(
            "release asset '{}' does not include a digest",
            asset.name
        ))
    })?;
    let Some(hash) = raw.strip_prefix("sha256:") else {
        return Err(DnsError::runtime(format!(
            "release asset '{}' uses unsupported digest '{}'",
            asset.name, raw
        )));
    };
    if hash.len() != 64 || hex::decode(hash).is_err() {
        return Err(DnsError::runtime(format!(
            "release asset '{}' has invalid SHA256 digest '{}'",
            asset.name, raw
        )));
    }
    Ok(hash.to_ascii_lowercase())
}

fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let actual = sha256_file(path)?;
    if actual != expected.to_ascii_lowercase() {
        return Err(DnsError::runtime(format!(
            "SHA256 mismatch for '{}': expected {}, got {}",
            path.display(),
            expected,
            actual
        )));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path).map_err(|err| {
        DnsError::runtime(format!("failed to open '{}': {}", path.display(), err))
    })?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).map_err(|err| {
            DnsError::runtime(format!("failed to read '{}': {}", path.display(), err))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(not(windows))]
fn unpack_tar_gz(archive: &Path, out_dir: &Path) -> Result<()> {
    let file = File::open(archive).map_err(|err| {
        DnsError::runtime(format!(
            "failed to open archive '{}': {}",
            archive.display(),
            err
        ))
    })?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(out_dir).map_err(|err| {
        DnsError::runtime(format!(
            "failed to unpack archive into '{}': {}",
            out_dir.display(),
            err
        ))
    })
}

#[cfg(not(windows))]
fn find_extracted_binary(unpack_dir: &Path) -> Result<PathBuf> {
    let candidate = unpack_dir.join("oxidns");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(DnsError::runtime(format!(
        "archive did not contain oxidns binary at '{}'",
        candidate.display()
    )))
}

#[cfg(not(windows))]
fn replace_binary(source: &Path, target: &Path) -> Result<()> {
    let tmp = target.with_extension("oxidns-upgrade-new");
    fs::copy(source, &tmp).map_err(|err| {
        DnsError::runtime(format!(
            "failed to stage upgraded binary '{}': {}",
            tmp.display(),
            err
        ))
    })?;
    let permissions = fs::metadata(source)?.permissions();
    fs::set_permissions(&tmp, permissions)?;
    fs::rename(&tmp, target).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        DnsError::runtime(format!(
            "failed to replace binary '{}': {}",
            target.display(),
            err
        ))
    })
}

fn find_extracted_webui(unpack_dir: &Path) -> Option<PathBuf> {
    let candidate = unpack_dir.join("webui");
    candidate.is_dir().then_some(candidate)
}

/// Recursively copies a directory tree using std only.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Moves a directory, falling back to a recursive copy when the source and
/// destination live on different filesystems.
fn move_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => {
            copy_dir_all(from, to)?;
            fs::remove_dir_all(from)
        }
        Err(err) => Err(err),
    }
}

fn resolve_webui_install_target(target: &Path) -> PathBuf {
    if fs::symlink_metadata(target)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
        && let Ok(resolved) = fs::canonicalize(target)
    {
        return resolved;
    }
    target.to_path_buf()
}

/// Installs the unpacked `webui/` tree into `target`, keeping the served
/// directory crash-safe.
///
/// The new tree is fully staged into a sibling of `target` first, so `target`
/// keeps serving the old UI untouched until the final swap. The final swap is a
/// same-filesystem rename (staging is a sibling), so it is atomic and cannot
/// leave a half-written served directory. The only window where `target` is
/// absent is between renaming the old tree to the backup and renaming the new
/// tree in: two single-parent renames, during which the old tree is fully
/// recoverable at the backup path.
///
/// Returns `(installed_path, backup_path)`; `backup_path` is `None` on a fresh
/// install where `target` did not previously exist.
fn replace_webui(
    unpacked_webui: &Path,
    target: &Path,
    backup_dir: &Path,
    version: &str,
) -> Result<(PathBuf, Option<PathBuf>)> {
    let target = resolve_webui_install_target(target);
    let target = target.as_path();
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|err| {
        DnsError::runtime(format!(
            "failed to create WebUI parent directory '{}': {}",
            parent.display(),
            err
        ))
    })?;

    let staging = target.with_extension("webui-upgrade-new");
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|err| {
            DnsError::runtime(format!(
                "failed to clear stale WebUI staging '{}': {}",
                staging.display(),
                err
            ))
        })?;
    }
    move_dir(unpacked_webui, &staging).map_err(|err| {
        DnsError::runtime(format!(
            "failed to stage WebUI into '{}': {}",
            staging.display(),
            err
        ))
    })?;

    let backup_path = if target.exists() {
        fs::create_dir_all(backup_dir).map_err(|err| {
            DnsError::runtime(format!(
                "failed to create WebUI backup directory '{}': {}",
                backup_dir.display(),
                err
            ))
        })?;
        let path = backup_dir.join(format!(
            "webui-{}-{}",
            version,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ));
        if let Err(err) = move_dir(target, &path) {
            let _ = fs::remove_dir_all(&staging);
            return Err(DnsError::runtime(format!(
                "failed to back up existing WebUI '{}': {}",
                target.display(),
                err
            )));
        }
        Some(path)
    } else {
        None
    };

    if let Err(err) = fs::rename(&staging, target) {
        if let Some(ref backup) = backup_path {
            let _ = move_dir(backup, target);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(DnsError::runtime(format!(
            "failed to install WebUI into '{}': {}",
            target.display(),
            err
        )));
    }

    Ok((target.to_path_buf(), backup_path))
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Ok(candidate), Ok(current)) => candidate > current,
        _ => candidate != current,
    }
}

fn parse_version(raw: &str) -> std::result::Result<Version, semver::Error> {
    Version::parse(raw.trim_start_matches('v'))
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    prerelease: bool,
    html_url: Option<String>,
    assets: Vec<ReleaseAsset>,
}

impl GitHubRelease {
    fn version_string(&self) -> String {
        self.tag_name.trim_start_matches('v').to_string()
    }
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_asset_sha256_digest() {
        let asset = ReleaseAsset {
            name: "oxidns.tar.gz".to_string(),
            browser_download_url: "https://example.com/oxidns.tar.gz".to_string(),
            digest: Some(
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            ),
        };
        let parsed = sha256_from_asset_digest(&asset).unwrap();
        assert_eq!(
            parsed,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn version_compare_handles_v_prefix() {
        assert!(is_newer_version("v0.4.2", "0.4.1"));
        assert!(!is_newer_version("v0.4.1", "0.4.1"));
    }

    #[test]
    fn github_request_headers_include_authorization_when_token_is_set() {
        let headers = github_request_headers(Some(" ghp_test "));
        assert!(headers.iter().any(|(name, value)| {
            *name == AUTHORIZATION && value.to_str().unwrap() == "Bearer ghp_test"
        }));
    }

    #[test]
    fn github_request_headers_skip_authorization_when_token_is_empty() {
        let headers = github_request_headers(Some("   "));
        assert!(!headers.iter().any(|(name, _)| *name == AUTHORIZATION));
    }

    #[test]
    fn archive_name_for_full_bundle_uses_legacy_name() {
        let name =
            archive_name_for_bundle(UpgradeBundle::Full, "x86_64-unknown-linux-musl", "tar.gz")
                .unwrap();

        assert_eq!(name, "oxidns-x86_64-unknown-linux-musl.tar.gz");
    }

    #[test]
    fn archive_name_for_slim_bundles_uses_prefixed_name() {
        let minimal = archive_name_for_bundle(
            UpgradeBundle::Minimal,
            "x86_64-unknown-linux-musl",
            "tar.gz",
        )
        .unwrap();
        let standard = archive_name_for_bundle(
            UpgradeBundle::Standard,
            "aarch64-unknown-linux-musl",
            "tar.gz",
        )
        .unwrap();

        assert_eq!(minimal, "oxidns-minimal-x86_64-unknown-linux-musl.tar.gz");
        assert_eq!(
            standard,
            "oxidns-standard-aarch64-unknown-linux-musl.tar.gz"
        );
    }

    #[test]
    fn auto_bundle_resolves_from_primary_bundle() {
        assert_eq!(
            resolve_requested_bundle(UpgradeBundle::Auto, "standard").unwrap(),
            UpgradeBundle::Standard
        );
        assert_eq!(
            resolve_requested_bundle(UpgradeBundle::Auto, "minimal").unwrap(),
            UpgradeBundle::Minimal
        );
        assert_eq!(
            resolve_requested_bundle(UpgradeBundle::Auto, "full").unwrap(),
            UpgradeBundle::Full
        );
    }

    #[test]
    fn auto_bundle_rejects_custom_builds() {
        let err = resolve_requested_bundle(UpgradeBundle::Auto, "custom").unwrap_err();

        assert!(err.to_string().contains("current build bundle is custom"));
        assert!(err.to_string().contains("--asset"));
    }

    #[test]
    fn explicit_asset_overrides_bundle_selection() {
        let release = GitHubRelease {
            tag_name: "v1.2.3".to_string(),
            prerelease: false,
            html_url: None,
            assets: vec![
                ReleaseAsset {
                    name: "oxidns-standard-x86_64-unknown-linux-musl.tar.gz".to_string(),
                    browser_download_url: "https://example.com/standard.tar.gz".to_string(),
                    digest: None,
                },
                ReleaseAsset {
                    name: "custom.tar.gz".to_string(),
                    browser_download_url: "https://example.com/custom.tar.gz".to_string(),
                    digest: None,
                },
            ],
        };
        let config = UpgradeConfig {
            asset: "custom.tar.gz".to_string(),
            bundle: UpgradeBundle::Standard,
            ..UpgradeConfig::default()
        };

        let asset = select_asset(&config, &release).unwrap();

        assert_eq!(asset.name, "custom.tar.gz");
    }

    #[test]
    fn config_default_has_webui_defaults() {
        let config = UpgradeConfig::default();
        assert_eq!(config.webui_dir, PathBuf::from("./webui"));
        assert!(!config.skip_webui);
        assert!(!config.no_restart);
        assert_eq!(config.bundle, UpgradeBundle::Auto);
    }

    #[test]
    fn from_cli_maps_webui_fields() {
        use clap::Parser;

        use crate::app::cli::{Cli, Command};

        #[cfg(windows)]
        let webui_dir_arg = r"C:\tmp\oxidns-webui";
        #[cfg(not(windows))]
        let webui_dir_arg = "/tmp/oxidns-webui";

        let cli = Cli::parse_from([
            "oxidns",
            "upgrade",
            "apply",
            "--webui-dir",
            webui_dir_arg,
            "--skip-webui",
        ]);
        let Command::Upgrade(opts) = cli.command else {
            panic!("expected upgrade command");
        };
        let config = UpgradeConfig::from_cli(&opts).unwrap();
        assert_eq!(config.webui_dir, PathBuf::from(webui_dir_arg));
        assert!(config.skip_webui);
    }

    #[test]
    fn from_cli_maps_github_token() {
        use clap::Parser;

        use crate::app::cli::{Cli, Command};

        let cli = Cli::parse_from(["oxidns", "upgrade", "check", "--github-token", "ghp_test"]);
        let Command::Upgrade(opts) = cli.command else {
            panic!("expected upgrade command");
        };
        let config = UpgradeConfig::from_cli(&opts).unwrap();
        assert_eq!(config.github_token.as_deref(), Some("ghp_test"));
    }

    #[test]
    fn from_cli_maps_bundle() {
        use clap::Parser;

        use crate::app::cli::{Cli, Command};

        let cli = Cli::parse_from(["oxidns", "upgrade", "check", "--bundle", "minimal"]);
        let Command::Upgrade(opts) = cli.command else {
            panic!("expected upgrade command");
        };
        let config = UpgradeConfig::from_cli(&opts).unwrap();
        assert_eq!(config.bundle, UpgradeBundle::Minimal);
    }

    #[test]
    fn from_cli_maps_no_restart_flag() {
        use clap::Parser;

        use crate::app::cli::{Cli, Command};

        let cli = Cli::parse_from(["oxidns", "upgrade", "apply", "--no-restart"]);
        let Command::Upgrade(opts) = cli.command else {
            panic!("expected upgrade command");
        };
        let config = UpgradeConfig::from_cli(&opts).unwrap();
        assert!(config.no_restart);
    }

    #[test]
    fn from_cli_resolves_webui_root_against_explicit_working_dir() {
        use clap::Parser;

        use crate::app::cli::{Cli, Command};

        let tmp = tempfile::TempDir::new().unwrap();
        let config_path = tmp.path().join("config.yaml");
        fs::write(
            &config_path,
            r#"
api:
  http:
    listen: ":9199"
    webui:
      root: ./webui
"#,
        )
        .unwrap();
        let working_dir = tmp.path().join("work");
        let cli = Cli::parse_from([
            "oxidns",
            "upgrade",
            "-c",
            config_path.to_str().unwrap(),
            "-d",
            working_dir.to_str().unwrap(),
        ]);
        let Command::Upgrade(opts) = cli.command else {
            panic!("expected upgrade command");
        };
        let defaults = CliPathDefaults {
            current_dir: tmp.path().to_path_buf(),
            service_config: None,
            service_working_dir: None,
        };

        let config = UpgradeConfig::from_cli_with_path_defaults(&opts, &defaults).unwrap();

        assert_eq!(config.webui_dir, working_dir.join("webui"));
    }

    #[test]
    fn from_cli_uses_service_config_and_working_dir_when_no_local_config_exists() {
        use clap::Parser;

        use crate::app::cli::{Cli, Command};

        let tmp = tempfile::TempDir::new().unwrap();
        let current_dir = tmp.path().join("home");
        let service_working_dir = tmp.path().join("var/lib/oxidns");
        fs::create_dir_all(&current_dir).unwrap();
        fs::create_dir_all(&service_working_dir).unwrap();
        let service_config = tmp.path().join("etc/oxidns/config.yaml");
        fs::create_dir_all(service_config.parent().unwrap()).unwrap();
        fs::write(
            &service_config,
            br#"
api:
  http:
    listen: ":9199"
    webui:
      root: ./webui
"#,
        )
        .unwrap();
        let cli = Cli::parse_from(["oxidns", "upgrade"]);
        let Command::Upgrade(opts) = cli.command else {
            panic!("expected upgrade command");
        };
        let defaults = CliPathDefaults {
            current_dir,
            service_config: Some(service_config),
            service_working_dir: Some(service_working_dir.clone()),
        };

        let config = UpgradeConfig::from_cli_with_path_defaults(&opts, &defaults).unwrap();

        assert_eq!(config.webui_dir, service_working_dir.join("webui"));
    }

    #[cfg(not(windows))]
    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    #[cfg(not(windows))]
    fn copy_dir_all_copies_nested_tree() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        write_file(&src.join("index.html"), b"index");
        write_file(&src.join("_next/static/a.js"), b"chunk");
        fs::create_dir_all(src.join("empty")).unwrap();

        copy_dir_all(&src, &dst).unwrap();

        assert_eq!(fs::read(dst.join("index.html")).unwrap(), b"index");
        assert_eq!(fs::read(dst.join("_next/static/a.js")).unwrap(), b"chunk");
        assert!(dst.join("empty").is_dir());
    }

    #[test]
    #[cfg(not(windows))]
    fn find_extracted_webui_detects_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(find_extracted_webui(tmp.path()).is_none());
        write_file(&tmp.path().join("webui").join("index.html"), b"x");
        assert_eq!(
            find_extracted_webui(tmp.path()),
            Some(tmp.path().join("webui"))
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn replace_webui_fresh_install_no_backup() {
        let tmp = tempfile::TempDir::new().unwrap();
        let unpacked = tmp.path().join(".unpack/webui");
        write_file(&unpacked.join("index.html"), b"new");
        let target = tmp.path().join("nested/served/webui");
        let backup_dir = tmp.path().join("backups");

        let (installed, backup) = replace_webui(&unpacked, &target, &backup_dir, "0.6.0").unwrap();

        assert_eq!(installed, target);
        assert!(backup.is_none());
        assert_eq!(fs::read(target.join("index.html")).unwrap(), b"new");
    }

    #[test]
    #[cfg(not(windows))]
    fn replace_webui_backs_up_and_swaps() {
        let tmp = tempfile::TempDir::new().unwrap();
        let unpacked = tmp.path().join(".unpack/webui");
        write_file(&unpacked.join("index.html"), b"new-content");
        let target = tmp.path().join("webui");
        write_file(&target.join("marker.txt"), b"old-marker");
        let backup_dir = tmp.path().join("backups");

        let (installed, backup) = replace_webui(&unpacked, &target, &backup_dir, "0.6.0").unwrap();

        assert_eq!(installed, target);
        assert_eq!(fs::read(target.join("index.html")).unwrap(), b"new-content");
        assert!(!target.join("marker.txt").exists());
        let backup = backup.expect("existing webui must be backed up");
        assert!(backup.starts_with(&backup_dir));
        assert_eq!(fs::read(backup.join("marker.txt")).unwrap(), b"old-marker");
    }

    #[test]
    #[cfg(unix)]
    fn replace_webui_updates_symlink_target_without_replacing_link() {
        let tmp = tempfile::TempDir::new().unwrap();
        let unpacked = tmp.path().join(".unpack/webui");
        write_file(&unpacked.join("index.html"), b"new-content");
        let real_target = tmp.path().join("usr/share/oxidns/webui");
        write_file(&real_target.join("marker.txt"), b"old-marker");
        let link_parent = tmp.path().join("var/lib/oxidns");
        fs::create_dir_all(&link_parent).unwrap();
        let link_target = link_parent.join("webui");
        std::os::unix::fs::symlink(&real_target, &link_target).unwrap();
        let backup_dir = tmp.path().join("backups");

        let (installed, backup) =
            replace_webui(&unpacked, &link_target, &backup_dir, "0.6.0").unwrap();

        assert_eq!(installed, fs::canonicalize(&real_target).unwrap());
        assert!(
            fs::symlink_metadata(&link_target)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read(link_target.join("index.html")).unwrap(),
            b"new-content"
        );
        assert!(!link_target.join("marker.txt").exists());
        let backup = backup.expect("existing symlink target must be backed up");
        assert_eq!(fs::read(backup.join("marker.txt")).unwrap(), b"old-marker");
    }
}
