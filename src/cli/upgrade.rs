// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! CLI support for checking, downloading, and applying upgrades.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::cli::{UpgradeAction, UpgradeOptions};
use crate::config::types::NetworkConfig;
use crate::infra::clock::AppClock;
use crate::infra::error::{DnsError, Result};
#[cfg(feature = "_http-client")]
use crate::infra::network::outbound;
use crate::infra::service;
use crate::infra::upgrade::{
    self, ApplyDecision, UpgradeConfig, UpgradeContext, UpgradeDownloadProgressReporter,
};

const DEFAULT_CONFIG_FILE: &str = "config.yaml";
const DEFAULT_WEBUI_DIR: &str = "./webui";
#[cfg(target_os = "linux")]
const DEFAULT_SERVICE_CONFIG: &str = "/etc/oxidns/config.yaml";
#[cfg(target_os = "linux")]
const DEFAULT_SERVICE_WORKING_DIR: &str = "/var/lib/oxidns";

pub fn run(options: UpgradeOptions) -> Result<()> {
    AppClock::start();
    let action = options.action.unwrap_or(UpgradeAction::Apply);
    let config = config_from_options(&options)?;
    run_action(action, config)
}

fn config_from_options(options: &UpgradeOptions) -> Result<UpgradeConfig> {
    let path_defaults = CliPathDefaults::system()?;
    config_from_options_with_path_defaults(options, &path_defaults)
}

fn config_from_options_with_path_defaults(
    options: &UpgradeOptions,
    path_defaults: &CliPathDefaults,
) -> Result<UpgradeConfig> {
    let path_context = resolve_path_context(options, path_defaults);
    install_outbound_from_config(&path_context)?;
    Ok(UpgradeConfig {
        target: options.target.clone(),
        repository: options.repository.clone(),
        asset: options.asset.clone(),
        bundle: options.bundle,
        cache_dir: options.cache_dir.clone(),
        backup_dir: options.backup_dir.clone(),
        webui_dir: resolve_webui_dir(options, &path_context)?,
        skip_webui: options.skip_webui,
        no_restart: options.no_restart,
        allow_prerelease: options.allow_prerelease,
        force: options.force,
        timeout: options.timeout,
        outbound: options.outbound.clone(),
        socks5: options.socks5.clone(),
        insecure_skip_verify: options.insecure_skip_verify,
        github_token: options.github_token.clone(),
        ..UpgradeConfig::default()
    })
}

#[cfg(feature = "_http-client")]
fn install_outbound_from_config(context: &CliPathContext) -> Result<()> {
    let Some(config_path) = &context.config_path else {
        outbound::clear_global();
        return Ok(());
    };
    match read_upgrade_runtime_config(config_path) {
        Ok(config) => {
            if let Some(network) = config.network {
                outbound::install_global(&network.outbound)?;
            } else {
                outbound::clear_global();
            }
            Ok(())
        }
        Err(err) if context.config_explicit => Err(err),
        Err(_) => {
            outbound::clear_global();
            Ok(())
        }
    }
}

#[cfg(not(feature = "_http-client"))]
fn install_outbound_from_config(_context: &CliPathContext) -> Result<()> {
    Ok(())
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

fn resolve_path_context(options: &UpgradeOptions, defaults: &CliPathDefaults) -> CliPathContext {
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

fn resolve_webui_dir(options: &UpgradeOptions, context: &CliPathContext) -> Result<PathBuf> {
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
    let config = read_upgrade_runtime_config(config_path)?;
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

fn read_upgrade_runtime_config(config_path: &Path) -> Result<UpgradeRuntimeConfig> {
    let string = std::fs::read_to_string(config_path).map_err(|err| {
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
    Ok(config)
}

#[derive(Debug, serde::Deserialize)]
struct UpgradeRuntimeConfig {
    api: Option<UpgradeRuntimeApiConfig>,
    network: Option<NetworkConfig>,
}

#[derive(Debug, serde::Deserialize)]
struct UpgradeRuntimeApiConfig {
    http: Option<UpgradeRuntimeHttpConfig>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum UpgradeRuntimeHttpConfig {
    Listen(String),
    Detailed(UpgradeRuntimeHttpDetailedConfig),
}

#[derive(Debug, serde::Deserialize)]
struct UpgradeRuntimeHttpDetailedConfig {
    webui: Option<UpgradeRuntimeWebUiConfig>,
}

#[derive(Debug, serde::Deserialize)]
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

fn run_action(action: UpgradeAction, config: UpgradeConfig) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| DnsError::runtime(format!("failed to create upgrade runtime: {err}")))?;

    runtime.block_on(async move {
        match action {
            UpgradeAction::Check => {
                print_plan("check", &config);
                println!("Checking GitHub release metadata...");
                let check = upgrade::check(&config).await?;
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
                print_plan("download", &config);
                println!("Resolving release asset...");
                println!("Downloading archive without checking the current version...");
                let progress_reporter = UpgradeDownloadProgressReporter::new(UpgradeContext::Cli);
                let download = upgrade::download(&config, move |progress| {
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
                print_plan("apply", &config);
                println!("Checking whether an upgrade is needed...");
                match upgrade::should_apply(&config).await? {
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
                        let outcome = upgrade::apply_unchecked(&config, UpgradeContext::Cli).await?;
                        if outcome.restart_required {
                            println!("Restarting installed service...");
                            service::restart_installed_service()?;
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
                            match upgrade::cleanup_upgrade_artifacts(&config) {
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

fn print_plan(action: &str, config: &UpgradeConfig) {
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
    if let Some(outbound) = config.outbound.as_deref() {
        println!("Outbound: {}", outbound);
    }
    if let Some(socks5) = config.socks5.as_deref() {
        println!("SOCKS5: {}", socks5);
    }
    if config.insecure_skip_verify {
        println!("TLS verification: disabled");
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

#[cfg(test)]
mod tests {
    use std::fs;

    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command};
    use crate::infra::upgrade::UpgradeBundle;

    #[test]
    fn config_from_options_maps_webui_fields() {
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

        let config = config_from_options(&opts).unwrap();

        assert_eq!(config.webui_dir, PathBuf::from(webui_dir_arg));
        assert!(config.skip_webui);
    }

    #[test]
    fn config_from_options_maps_github_token() {
        let cli = Cli::parse_from(["oxidns", "upgrade", "check", "--github-token", "ghp_test"]);
        let Command::Upgrade(opts) = cli.command else {
            panic!("expected upgrade command");
        };

        let config = config_from_options(&opts).unwrap();

        assert_eq!(config.github_token.as_deref(), Some("ghp_test"));
    }

    #[test]
    fn config_from_options_maps_bundle() {
        let cli = Cli::parse_from(["oxidns", "upgrade", "check", "--bundle", "minimal"]);
        let Command::Upgrade(opts) = cli.command else {
            panic!("expected upgrade command");
        };

        let config = config_from_options(&opts).unwrap();

        assert_eq!(config.bundle, UpgradeBundle::Minimal);
    }

    #[test]
    fn config_from_options_maps_no_restart_flag() {
        let cli = Cli::parse_from(["oxidns", "upgrade", "apply", "--no-restart"]);
        let Command::Upgrade(opts) = cli.command else {
            panic!("expected upgrade command");
        };

        let config = config_from_options(&opts).unwrap();

        assert!(config.no_restart);
    }

    #[test]
    fn config_from_options_resolves_webui_root_against_explicit_working_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_path = tmp.path().join("config.yaml");
        fs::write(
            &config_path,
            br#"
api:
  http:
    listen: ":9199"
    webui:
      root: ./public
"#,
        )
        .unwrap();
        let working_dir = tmp.path().join("runtime");
        fs::create_dir_all(&working_dir).unwrap();
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

        let config = config_from_options_with_path_defaults(&opts, &defaults).unwrap();

        assert_eq!(config.webui_dir, working_dir.join("public"));
    }

    #[test]
    fn config_from_options_uses_service_config_and_working_dir_when_no_local_config_exists() {
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

        let config = config_from_options_with_path_defaults(&opts, &defaults).unwrap();

        assert_eq!(config.webui_dir, service_working_dir.join("webui"));
    }
}
