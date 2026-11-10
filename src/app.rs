// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Foreground application runtime entry used by the CLI `start` command.
//!
//! This module owns the non-service startup path:
//!
//! - applies startup overrides such as working directory and log level;
//! - loads and validates configuration;
//! - builds the Tokio runtime;
//! - assembles the API hub and plugin registry; and
//! - coordinates shutdown and reload flows for the live process.
//!
//! The goal is to keep process-level concerns here so the lower-level modules
//! (`config`, `plugin`, `infra`, `api`) stay focused on their own domains.

mod banner;
pub mod bootstrap;

use std::path::PathBuf;
use std::sync::OnceLock;

use tokio::runtime;

/// Executable path captured at process startup, before any binary replacement.
///
/// On Linux, after an atomic rename-over-self upgrade `/proc/self/exe` acquires
/// a ` (deleted)` suffix that makes the path non-executable. Reading the path
/// here — before the upgrade plugin has a chance to replace the binary —
/// guarantees `restart_process()` always has a valid path to hand to
/// `execvp()`.
static ORIGINAL_EXE: OnceLock<PathBuf> = OnceLock::new();
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

use crate::app::bootstrap::AppAssembly;
use crate::config;
use crate::config::types::Config;
use crate::infra::clock::AppClock;
use crate::infra::control::{AppController, ControlCommand};
use crate::infra::error::{DnsError, Result};
use crate::infra::observability::logging;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartConfig {
    pub config: PathBuf,
    pub working_dir: Option<PathBuf>,
    pub log_level: Option<String>,
}

/// Start OxiDNS in the foreground using the provided CLI options.
pub fn run_foreground(start: StartConfig) -> Result<()> {
    match run_until_shutdown(start)? {
        ShutdownSignal::Restart => restart_process(),
        _ => Ok(()),
    }
}

#[cfg(windows)]
pub(crate) fn run_windows_service(start: StartConfig) -> Result<ShutdownSignal> {
    run_until_shutdown(start)
}

/// Apply process startup options and run until shutdown, leaving restart to the
/// caller.
fn run_until_shutdown(start: StartConfig) -> Result<ShutdownSignal> {
    // Capture the exe path before any binary replacement can invalidate it.
    ORIGINAL_EXE.get_or_init(|| std::env::current_exe().unwrap_or_default());
    AppClock::start();
    apply_working_dir_override(start.working_dir.as_ref())?;
    // Clean up any leftover staging file from an interrupted Windows upgrade.
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("upgrade-new.exe"));
    }
    banner::print_startup_banner()?;
    let config = load_startup_config(&start)?;
    run_with_tokio_runtime(start, config)
}

fn apply_working_dir_override(working_dir: Option<&std::path::PathBuf>) -> Result<()> {
    if let Some(working_dir) = working_dir {
        std::env::set_current_dir(working_dir).map_err(|err| {
            DnsError::runtime(format!(
                "Failed to switch working directory to {}: {}",
                working_dir.display(),
                err
            ))
        })?;
    }
    Ok(())
}

/// Build the Tokio runtime and block until application shutdown completes.
fn run_with_tokio_runtime(options: StartConfig, config: Config) -> Result<ShutdownSignal> {
    let worker_threads = config.runtime.effective_worker_threads();
    let mut tokio_runtime = runtime::Builder::new_multi_thread();
    tokio_runtime
        .enable_all()
        .thread_name("oxidns-worker")
        .worker_threads(worker_threads);
    let tokio_runtime = tokio_runtime
        .build()
        .map_err(|err| DnsError::runtime(format!("Failed to initialize Tokio runtime: {err}")))?;
    tokio_runtime.block_on(run_application(options, config))
}

/// Replace the current process image with a fresh copy of OxiDNS using the
/// original command-line arguments.
///
/// On Unix, `exec()` keeps the same PID so any process supervisor (systemd,
/// launchd, Docker, etc.) continues tracking the process without interruption.
/// On non-Unix platforms a new process is spawned and the current one exits.
pub(crate) fn restart_process() -> Result<()> {
    let exe = ORIGINAL_EXE
        .get()
        .cloned()
        .or_else(|| std::env::current_exe().ok())
        .ok_or_else(|| DnsError::runtime("restart: cannot determine executable path"))?;
    // std::env::args() reads the OS-level process arguments and is safe to
    // call at any point during the process lifetime.
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&exe).args(&args).exec();
        Err(DnsError::runtime(format!("exec restart failed: {err}")))
    }

    #[cfg(windows)]
    {
        // Windows services handle Restart in the SCM wrapper and never reach
        // this foreground replacement path.
        std::process::Command::new(&exe)
            .args(&args)
            .spawn()
            .map_err(|e| DnsError::runtime(format!("restart: spawn failed: {e}")))?;
        std::process::exit(0);
    }

    #[cfg(not(any(unix, windows)))]
    {
        std::process::Command::new(&exe)
            .args(&args)
            .spawn()
            .map_err(|e| DnsError::runtime(format!("restart: spawn failed: {e}")))?;
        std::process::exit(0);
    }
}

fn load_startup_config(options: &StartConfig) -> Result<Config> {
    config::init(&options.config).map_err(|err| {
        DnsError::config(format!(
            "Configuration initialization failed for {}: {}",
            options.config.display(),
            err
        ))
    })
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ShutdownSignal {
    ApiRequest,
    Restart,
    #[cfg(unix)]
    SigInt,
    #[cfg(unix)]
    SigTerm,
    #[cfg(unix)]
    SigQuit,
    #[cfg(any(windows, not(any(unix, windows))))]
    CtrlC,
    #[cfg(windows)]
    CtrlBreak,
    #[cfg(windows)]
    CtrlClose,
    #[cfg(windows)]
    CtrlShutdown,
    #[cfg(windows)]
    CtrlLogoff,
}

impl ShutdownSignal {
    const fn as_str(self) -> &'static str {
        match self {
            ShutdownSignal::ApiRequest => "API_REQUEST",
            ShutdownSignal::Restart => "RESTART",
            #[cfg(unix)]
            ShutdownSignal::SigInt => "SIGINT",
            #[cfg(unix)]
            ShutdownSignal::SigTerm => "SIGTERM",
            #[cfg(unix)]
            ShutdownSignal::SigQuit => "SIGQUIT",
            #[cfg(any(windows, not(any(unix, windows))))]
            ShutdownSignal::CtrlC => "CTRL_C",
            #[cfg(windows)]
            ShutdownSignal::CtrlBreak => "CTRL_BREAK",
            #[cfg(windows)]
            ShutdownSignal::CtrlClose => "CTRL_CLOSE",
            #[cfg(windows)]
            ShutdownSignal::CtrlShutdown => "CTRL_SHUTDOWN",
            #[cfg(windows)]
            ShutdownSignal::CtrlLogoff => "CTRL_LOGOFF",
        }
    }
}

#[cfg(unix)]
async fn wait_for_os_shutdown_signal() -> Result<ShutdownSignal> {
    use tokio::signal::unix::{SignalKind, signal as unix_signal};

    let mut sigint = unix_signal(SignalKind::interrupt())
        .map_err(|err| DnsError::runtime(format!("Failed to listen for SIGINT: {err}")))?;
    let mut sigterm = unix_signal(SignalKind::terminate())
        .map_err(|err| DnsError::runtime(format!("Failed to listen for SIGTERM: {err}")))?;
    let mut sigquit = unix_signal(SignalKind::quit())
        .map_err(|err| DnsError::runtime(format!("Failed to listen for SIGQUIT: {err}")))?;

    tokio::select! {
        _ = sigint.recv() => Ok(ShutdownSignal::SigInt),
        _ = sigterm.recv() => Ok(ShutdownSignal::SigTerm),
        _ = sigquit.recv() => Ok(ShutdownSignal::SigQuit),
    }
}

#[cfg(windows)]
async fn wait_for_os_shutdown_signal() -> Result<ShutdownSignal> {
    use tokio::signal::windows::{
        ctrl_break, ctrl_c as windows_ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown,
    };

    let mut ctrl_c = windows_ctrl_c()
        .map_err(|err| DnsError::runtime(format!("Failed to listen for CTRL_C: {err}")))?;
    let mut ctrl_break = ctrl_break()
        .map_err(|err| DnsError::runtime(format!("Failed to listen for CTRL_BREAK: {err}")))?;
    let mut ctrl_close = ctrl_close()
        .map_err(|err| DnsError::runtime(format!("Failed to listen for CTRL_CLOSE: {err}")))?;
    let mut ctrl_shutdown = ctrl_shutdown()
        .map_err(|err| DnsError::runtime(format!("Failed to listen for CTRL_SHUTDOWN: {err}")))?;
    let mut ctrl_logoff = ctrl_logoff()
        .map_err(|err| DnsError::runtime(format!("Failed to listen for CTRL_LOGOFF: {err}")))?;

    tokio::select! {
        _ = ctrl_c.recv() => Ok(ShutdownSignal::CtrlC),
        _ = ctrl_break.recv() => Ok(ShutdownSignal::CtrlBreak),
        _ = ctrl_close.recv() => Ok(ShutdownSignal::CtrlClose),
        _ = ctrl_shutdown.recv() => Ok(ShutdownSignal::CtrlShutdown),
        _ = ctrl_logoff.recv() => Ok(ShutdownSignal::CtrlLogoff),
    }
}

#[cfg(not(any(unix, windows)))]
async fn wait_for_os_shutdown_signal() -> Result<ShutdownSignal> {
    tokio::signal::ctrl_c()
        .await
        .map_err(|err| DnsError::runtime(format!("Failed to listen for Ctrl+C: {err}")))?;
    Ok(ShutdownSignal::CtrlC)
}

/// Start application resources, process control commands, and shut down
/// gracefully.
#[hotpath::main]
async fn run_application(options: StartConfig, config: Config) -> Result<ShutdownSignal> {
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<Result<ShutdownSignal>>();
    tokio::spawn(async move {
        let _ = shutdown_tx.send(wait_for_os_shutdown_signal().await);
    });

    let (app_controller, mut control_rx) = AppController::new(options.config.clone());

    let worker_threads = config.runtime.effective_worker_threads();
    let options = options.clone();

    let mut log_config = config.log.clone();
    let configured_level = log_config.level.clone();
    if let Some(level) = options.log_level.clone() {
        log_config.level = level;
    }

    let effective_log_level = log_config.level.clone();
    let _log_guard = logging::start_logging(log_config);
    info!(
        config = %options.config.display(),
        plugins = config.plugins.len(),
        "Configuration loaded"
    );
    info!(
        tokio_worker_threads = worker_threads,
        "Tokio runtime configured"
    );
    if let Some(level) = options.log_level {
        info!(
            config_level = %configured_level,
            cli_level = %level,
            "Log level overridden by CLI option"
        );
    }
    info!(log_level = %effective_log_level, "OxiDNS server initializing");

    let mut current_config = config;
    let mut assembly =
        match bootstrap::start_application(&current_config, Some(app_controller.clone())).await {
            Ok(assembly) => {
                app_controller.set_running_config_version(
                    crate::infra::control::config_file_version(app_controller.config_path()),
                );
                info!("OxiDNS server started successfully");
                assembly
            }
            Err(err) => {
                error!("Plugin initialization failed: {}", err);
                return Err(err);
            }
        };

    let shutdown_signal = run_control_loop(
        &mut control_rx,
        shutdown_rx,
        &mut assembly,
        &mut current_config,
        app_controller.clone(),
    )
    .await?;
    info!(
        signal = shutdown_signal.as_str(),
        "Destroying plugins for shutdown"
    );
    bootstrap::stop_application(&assembly).await;
    info!(
        signal = shutdown_signal.as_str(),
        "Graceful shutdown complete"
    );
    Ok(shutdown_signal)
}

/// Handle reload commands until an OS signal, shutdown request, or restart
/// request.
async fn run_control_loop(
    control_rx: &mut mpsc::UnboundedReceiver<ControlCommand>,
    mut shutdown_rx: oneshot::Receiver<Result<ShutdownSignal>>,
    assembly: &mut AppAssembly,
    current_config: &mut Config,
    controller: std::sync::Arc<AppController>,
) -> Result<ShutdownSignal> {
    loop {
        tokio::select! {
            shutdown_signal = &mut shutdown_rx => {
                let shutdown_signal = shutdown_signal
                    .map_err(|_| DnsError::runtime("Shutdown signal task exited unexpectedly"))??;
                info!(
                    signal = shutdown_signal.as_str(),
                    "Received shutdown signal, initiating graceful shutdown"
                );
                return Ok(shutdown_signal);
            }
            command = control_rx.recv() => {
                match command {
                    Some(ControlCommand::Shutdown) => {
                        info!("Received shutdown request from management API");
                        return Ok(ShutdownSignal::ApiRequest);
                    }
                    Some(ControlCommand::Restart) => {
                        info!("Received restart request from management API");
                        return Ok(ShutdownSignal::Restart);
                    }
                    Some(ControlCommand::Reload) => {
                        reload_application(assembly, current_config, controller.clone()).await?;
                    }
                    None => return Err(DnsError::runtime("Control command channel closed unexpectedly")),
                }
            }
        }
    }
}

/// Restart application resources with the new config, restoring the old config
/// on failure.
async fn reload_application(
    assembly: &mut AppAssembly,
    current_config: &mut Config,
    controller: std::sync::Arc<AppController>,
) -> Result<()> {
    controller.mark_reload_started(crate::infra::control::config_file_version(
        controller.config_path(),
    ));

    let candidate_config = match load_and_validate_reload_config(controller.config_path()) {
        Ok(config) => config,
        Err(err) => {
            controller.mark_reload_failed(err.to_string());
            return Ok(());
        }
    };

    let previous_config = current_config.clone();
    info!(
        config = %controller.config_path().display(),
        "Reloading configuration from management API"
    );

    bootstrap::stop_application(assembly).await;

    match bootstrap::start_application(&candidate_config, Some(controller.clone())).await {
        Ok(new_assembly) => {
            *assembly = new_assembly;
            *current_config = candidate_config;
            controller.mark_reload_succeeded();
            info!("Configuration reload completed successfully");
            Ok(())
        }
        Err(reload_err) => {
            error!("Configuration reload failed: {}", reload_err);
            match bootstrap::start_application(&previous_config, Some(controller.clone())).await {
                Ok(restored_assembly) => {
                    *assembly = restored_assembly;
                    controller.mark_reload_failed(format!(
                        "reload failed and previous configuration was restored: {}",
                        reload_err
                    ));
                    Ok(())
                }
                Err(rollback_err) => {
                    controller.mark_reload_failed(format!(
                        "reload failed: {}; rollback failed: {}",
                        reload_err, rollback_err
                    ));
                    Err(DnsError::runtime(format!(
                        "reload failed: {}; rollback failed: {}",
                        reload_err, rollback_err
                    )))
                }
            }
        }
    }
}

fn load_and_validate_reload_config(path: &std::path::Path) -> Result<Config> {
    let path = path.to_path_buf();
    let config = config::init(&path).map_err(|err| {
        DnsError::config(format!(
            "Configuration initialization failed for {}: {}",
            path.display(),
            err
        ))
    })?;
    crate::plugin::validate_configuration(&config)?;
    Ok(config)
}

#[cfg(all(test, feature = "api"))]
mod cache_reload_tests {
    use super::*;

    #[tokio::test]
    async fn failed_reload_restores_historical_cache() {
        let _guard = crate::api::global_api_test_guard().await;
        let _outbound_guard = crate::infra::network::outbound::TestGlobalGuard::clean();
        crate::infra::clock::AppClock::start();
        let dir = tempfile::tempdir().unwrap();
        let dump = dir.path().join("cache.dump");
        std::fs::write(
            &dump,
            crate::plugin::executor::cache::persistence_test_dump(),
        )
        .unwrap();
        let yaml = format!(
            "plugins:\n  - tag: reload_cache\n    type: cache\n    args:\n      ecs_in_key: true\n      dump_file: '{}'\n",
            dump.display()
        );
        let mut config: Config = serde_yaml_ng::from_str(&yaml).unwrap();
        let path = dir.path().join("config.yaml");
        let (controller, _rx) = AppController::new(path.clone());
        let mut assembly = bootstrap::start_application(&config, Some(controller.clone()))
            .await
            .unwrap();
        let bad = yaml.replace("ecs_in_key: true", "ecs_in_key: false\n      size: 1")
            + "  - tag: zz_invalid\n    type: forward\n    args:\n      upstreams: [{addr: 'invalid://bad'}]\n";
        std::fs::write(&path, bad).unwrap();
        reload_application(&mut assembly, &mut config, controller.clone())
            .await
            .unwrap();
        bootstrap::stop_application(&assembly).await;
        // Restoring the old config must retain all four ECS variants.
        let restored = std::fs::read(&dump).unwrap();
        assert_eq!(u64::from_le_bytes(restored[..8].try_into().unwrap()), 4);
        // A successful reload still commits persistence normally.
        std::fs::write(&path, yaml).unwrap();
        assembly = bootstrap::start_application(&config, Some(controller.clone()))
            .await
            .unwrap();
        reload_application(&mut assembly, &mut config, controller)
            .await
            .unwrap();
        bootstrap::stop_application(&assembly).await;
        assert_eq!(
            u64::from_le_bytes(std::fs::read(&dump).unwrap()[..8].try_into().unwrap()),
            4
        );
    }
}
