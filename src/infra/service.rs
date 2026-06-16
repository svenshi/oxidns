// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Operating-system service management infrastructure.
//!
//! This module wraps the `service-manager` crate to install, start, stop,
//! restart, and uninstall OxiDNS as a system service. It keeps
//! platform-specific service manager details outside the normal foreground
//! application runner.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use service_manager::{
    RestartPolicy, ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx,
    ServiceStatus, ServiceStatusCtx, ServiceStopCtx, ServiceUninstallCtx, native_service_manager,
};
#[cfg(windows)]
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState,
        ServiceStatus as WinServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

use crate::infra::error::{DnsError, Result};

#[cfg(windows)]
define_windows_service!(ffi_service_main, windows_service_entry);

/// Try to hand control to the Windows SCM dispatcher.
///
/// Returns `true` if the process was started by SCM (service loop ran to
/// completion), `false` if running in foreground mode.  Must be called from
/// the main thread before any other work.
#[cfg(windows)]
pub fn try_dispatch_windows_service() -> Result<bool> {
    match service_dispatcher::start("oxidns", ffi_service_main) {
        Ok(()) => Ok(true),
        // ERROR_FAILED_SERVICE_CONTROLLER_CONNECT (1063): not started by SCM.
        Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1063) => Ok(false),
        Err(e) => Err(DnsError::runtime(format!(
            "Windows service dispatcher error: {e}"
        ))),
    }
}

#[cfg(windows)]
fn windows_service_entry(_args: Vec<OsString>) {
    if let Err(e) = run_windows_service() {
        eprintln!("OxiDNS service error: {e}");
    }
}

#[cfg(windows)]
fn run_windows_service() -> Result<()> {
    use std::sync::mpsc;
    use std::time::Duration;

    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
    let ctrl_tx = shutdown_tx.clone();

    let status_handle = service_control_handler::register("oxidns", move |event| match event {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = ctrl_tx.send(());
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })
    .map_err(|e| DnsError::runtime(format!("Failed to register service control handler: {e}")))?;

    let report = |state: ServiceState, accepted: ServiceControlAccept, hint_secs: u64| {
        status_handle
            .set_service_status(WinServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: state,
                controls_accepted: accepted,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::from_secs(hint_secs),
                process_id: None,
            })
            .map_err(|e| DnsError::runtime(format!("SetServiceStatus failed: {e}")))
    };

    report(
        ServiceState::StartPending,
        ServiceControlAccept::empty(),
        30,
    )?;

    let start_opts = parse_windows_service_start_config()?;

    let app_tx = shutdown_tx;
    let app_thread = std::thread::spawn(move || {
        let result = crate::app::run(start_opts);
        let _ = app_tx.send(());
        result
    });

    report(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        0,
    )?;

    // Block until SCM sends stop or the app exits on its own.
    let _ = shutdown_rx.recv();

    let _ = report(ServiceState::StopPending, ServiceControlAccept::empty(), 5);

    let app_result = if app_thread.is_finished() {
        app_thread
            .join()
            .unwrap_or_else(|_| Err(DnsError::runtime("app thread panicked")))
    } else {
        // App is still running after receiving stop — exit so SCM marks us stopped.
        let _ = report(ServiceState::Stopped, ServiceControlAccept::empty(), 0);
        std::process::exit(0);
    };

    let _ = report(ServiceState::Stopped, ServiceControlAccept::empty(), 0);
    app_result
}

#[cfg(windows)]
fn parse_windows_service_start_config() -> Result<crate::app::StartConfig> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    parse_start_config_args(&args)
}

#[cfg(windows)]
fn parse_start_config_args(args: &[OsString]) -> Result<crate::app::StartConfig> {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return Err(DnsError::runtime(
            "Windows service: binary path must use the 'start' subcommand",
        ));
    };
    if command != "start" {
        return Err(DnsError::runtime(
            "Windows service: binary path must use the 'start' subcommand",
        ));
    }

    let mut config = PathBuf::from("config.yaml");
    let mut working_dir = None;
    let mut log_level = None;
    let mut index = 1;
    while index < args.len() {
        let Some(flag) = args[index].to_str() else {
            return Err(DnsError::runtime(
                "Windows service: failed to parse service command-line flag",
            ));
        };
        match flag {
            "-c" | "--config" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(DnsError::runtime(
                        "Windows service: missing value for config flag",
                    ));
                };
                config = PathBuf::from(value);
            }
            "-d" | "--working-dir" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(DnsError::runtime(
                        "Windows service: missing value for working-dir flag",
                    ));
                };
                working_dir = Some(PathBuf::from(value));
            }
            "-l" | "--log-level" => {
                index += 1;
                let Some(value) = args.get(index).and_then(|value| value.to_str()) else {
                    return Err(DnsError::runtime(
                        "Windows service: missing or invalid value for log-level flag",
                    ));
                };
                log_level = Some(value.to_string());
            }
            other => {
                return Err(DnsError::runtime(format!(
                    "Windows service: unsupported start flag '{other}'"
                )));
            }
        }
        index += 1;
    }

    Ok(crate::app::StartConfig {
        config,
        working_dir,
        log_level,
    })
}

const SERVICE_LABEL: &str = "oxidns";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceInstallConfig {
    pub working_dir: PathBuf,
    pub config: PathBuf,
}

pub fn status() -> Result<ServiceStatus> {
    let service_manage = service_manager()?;
    let status = service_manage.status(ServiceStatusCtx {
        label: service_label()?,
    })?;
    Ok(status)
}

pub fn restart_installed_service() -> Result<()> {
    stop()?;
    start()
}

pub fn install(options: ServiceInstallConfig) -> Result<()> {
    let working_dir = normalize_working_dir(&options.working_dir)?;
    let config_path = normalize_config_path(&options.config, &working_dir)?;
    let program = std::env::current_exe()
        .map_err(|err| DnsError::runtime(format!("Failed to resolve current executable: {err}")))?;

    let mut manager = native_service_manager().map_err(|err| {
        DnsError::runtime(format!("Failed to detect native service manager: {err}"))
    })?;
    manager
        .set_level(ServiceLevel::System)
        .map_err(|err| DnsError::runtime(format!("Failed to set service level: {err}")))?;

    let ctx = ServiceInstallCtx {
        label: service_label()?,
        program,
        args: vec![
            OsString::from("start"),
            OsString::from("-c"),
            config_path.into_os_string(),
            OsString::from("-d"),
            working_dir.clone().into_os_string(),
        ],
        contents: None,
        username: None,
        // Keep `-d` as the single source of truth for runtime-relative paths.
        // This lets OxiDNS report path problems after startup instead of the
        // service manager failing an earlier chdir with less context.
        working_directory: None,
        environment: None,
        autostart: true,
        restart_policy: RestartPolicy::OnFailure {
            delay_secs: Some(3),
            max_retries: None,
            reset_after_secs: None,
        },
    };
    manager
        .install(ctx)
        .map_err(|err| DnsError::runtime(format!("Failed to install service: {err}")))?;
    Ok(())
}

pub fn start() -> Result<()> {
    let manager = service_manager()?;
    manager
        .start(ServiceStartCtx {
            label: service_label()?,
        })
        .map_err(|err| DnsError::runtime(format!("Failed to start service: {err}")))?;
    Ok(())
}

pub fn stop() -> Result<()> {
    let manager = service_manager()?;
    manager
        .stop(ServiceStopCtx {
            label: service_label()?,
        })
        .map_err(|err| DnsError::runtime(format!("Failed to stop service: {err}")))?;
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let manager = service_manager()?;
    manager
        .uninstall(ServiceUninstallCtx {
            label: service_label()?,
        })
        .map_err(|err| DnsError::runtime(format!("Failed to uninstall service: {err}")))?;
    Ok(())
}

fn service_manager() -> Result<Box<dyn ServiceManager>> {
    let mut manager = native_service_manager().map_err(|err| {
        DnsError::runtime(format!("Failed to detect native service manager: {err}"))
    })?;
    manager
        .set_level(ServiceLevel::System)
        .map_err(|err| DnsError::runtime(format!("Failed to set service level: {err}")))?;
    Ok(manager)
}

fn service_label() -> Result<ServiceLabel> {
    SERVICE_LABEL
        .parse()
        .map_err(|err| DnsError::runtime(format!("Invalid service label '{SERVICE_LABEL}': {err}")))
}

fn normalize_working_dir(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(DnsError::config(format!(
            "service install working directory must be absolute: {}",
            path.display()
        )));
    }
    std::fs::create_dir_all(path).map_err(|err| {
        DnsError::runtime(format!(
            "Failed to create working directory {}: {}",
            path.display(),
            err
        ))
    })?;
    path.canonicalize().map_err(|err| {
        DnsError::runtime(format!(
            "Failed to canonicalize working directory {}: {}",
            path.display(),
            err
        ))
    })
}

fn normalize_config_path(path: &Path, working_dir: &Path) -> Result<PathBuf> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    };
    candidate.canonicalize().map_err(|err| {
        DnsError::runtime(format!(
            "Failed to canonicalize config path {}: {}",
            candidate.display(),
            err
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_working_dir_rejects_relative_paths() {
        let err = normalize_working_dir(Path::new("relative/path")).expect_err("should fail");
        assert!(err.to_string().contains("must be absolute"));
    }

    #[test]
    fn packaged_systemd_unit_uses_cli_working_dir_only() {
        let unit = include_str!("../../packaging/oxidns.service");
        assert!(
            !unit
                .lines()
                .any(|line| line.starts_with("WorkingDirectory="))
        );
        assert!(unit.contains(
            "ExecStart=/usr/bin/oxidns start -c /etc/oxidns/config.yaml -d /var/lib/oxidns"
        ));
    }
}
