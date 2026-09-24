// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows SCM dispatch, service lifecycle, and restart recovery.

use std::ffi::OsString;
use std::path::PathBuf;

use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod, ServiceState,
    ServiceStatus as WinServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{
    ServiceManager as WindowsServiceManager, ServiceManagerAccess,
};
use windows_service::{define_windows_service, service_dispatcher};

use super::SERVICE_LABEL;
use crate::infra::error::{DnsError, Result};

define_windows_service!(ffi_service_main, windows_service_entry);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowsServiceEvent {
    StopRequested,
    AppExited,
}

/// Try to hand control to the Windows SCM dispatcher.
///
/// Returns `true` if the process was started by SCM (service loop ran to
/// completion), `false` if running in foreground mode.  Must be called from
/// the main thread before any other work.
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

fn windows_service_entry(_args: Vec<OsString>) {
    if let Err(e) = run_windows_service() {
        eprintln!("OxiDNS service error: {e}");
    }
}

fn run_windows_service() -> Result<()> {
    use std::sync::mpsc;
    use std::time::Duration;

    let (shutdown_tx, shutdown_rx) = mpsc::channel::<WindowsServiceEvent>();
    let ctrl_tx = shutdown_tx.clone();

    let status_handle = service_control_handler::register("oxidns", move |event| match event {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = ctrl_tx.send(WindowsServiceEvent::StopRequested);
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
        let result = crate::app::run_windows_service(start_opts);
        let _ = app_tx.send(WindowsServiceEvent::AppExited);
        result
    });

    report(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        0,
    )?;

    // Block until SCM sends stop or the app exits on its own.
    let event = shutdown_rx
        .recv()
        .map_err(|err| DnsError::runtime(format!("Windows service event channel closed: {err}")))?;

    let _ = report(ServiceState::StopPending, ServiceControlAccept::empty(), 5);

    let shutdown_signal = match event {
        WindowsServiceEvent::AppExited => app_thread
            .join()
            .unwrap_or_else(|_| Err(DnsError::runtime("app thread panicked")))?,
        WindowsServiceEvent::StopRequested => {
            // An explicit SCM stop is a clean shutdown.
            let _ = report(ServiceState::Stopped, ServiceControlAccept::empty(), 0);
            std::process::exit(0);
        }
    };

    if matches!(shutdown_signal, crate::app::ShutdownSignal::Restart) {
        // Do not report a clean STOPPED state: terminate with a failure code so
        // the SCM recovery action starts a fresh service process.
        std::process::exit(1);
    }

    let _ = report(ServiceState::Stopped, ServiceControlAccept::empty(), 0);
    Ok(())
}

fn parse_windows_service_start_config() -> Result<crate::app::StartConfig> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    parse_start_config_args(&args)
}

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

/// Configure the SCM recovery action used by application-requested restarts.
///
/// The `service-manager` Windows backend installs services through `sc create`,
/// which cannot apply its `RestartPolicy`. Configure the equivalent recovery
/// action through the native Windows service API after installation, while the
/// installer is still running outside the SCM service dispatcher.
pub(super) fn configure_restart_recovery() -> Result<()> {
    let manager =
        WindowsServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(|err| DnsError::runtime(format!("Failed to open Windows SCM: {err}")))?;
    let service = manager
        .open_service(
            SERVICE_LABEL,
            ServiceAccess::CHANGE_CONFIG | ServiceAccess::START,
        )
        .map_err(|err| {
            DnsError::runtime(format!(
                "Failed to open Windows service '{SERVICE_LABEL}' for recovery configuration: {err}"
            ))
        })?;

    service
        .update_failure_actions(windows_restart_failure_actions())
        .map_err(|err| {
            DnsError::runtime(format!(
                "Failed to configure Windows service restart recovery: {err}"
            ))
        })?;
    service
        .set_failure_actions_on_non_crash_failures(true)
        .map_err(|err| {
            DnsError::runtime(format!(
                "Failed to enable Windows service restart recovery: {err}"
            ))
        })?;
    Ok(())
}

fn windows_restart_failure_actions() -> ServiceFailureActions {
    use std::time::Duration;

    ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::Never,
        reboot_msg: None,
        command: None,
        actions: Some(vec![ServiceAction {
            action_type: ServiceActionType::Restart,
            delay: Duration::from_secs(3),
        }]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_recovery_restarts_after_three_seconds() {
        use std::time::Duration;

        let recovery = windows_restart_failure_actions();
        assert_eq!(recovery.reset_period, ServiceFailureResetPeriod::Never);
        assert_eq!(
            recovery.actions,
            Some(vec![ServiceAction {
                action_type: ServiceActionType::Restart,
                delay: Duration::from_secs(3),
            }])
        );
    }

    #[test]
    fn app_exit_event_does_not_depend_on_thread_completion() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (event_tx, event_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let app_thread = std::thread::spawn(move || {
            event_tx.send(WindowsServiceEvent::AppExited).unwrap();
            resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let event = event_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event, WindowsServiceEvent::AppExited);
        assert!(!app_thread.is_finished());
        resume_tx.send(()).unwrap();
        app_thread.join().unwrap();
    }
}
