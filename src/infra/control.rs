// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Always-on application controller state.
//!
//! [`AppController`] owns the live runtime state the management API surfaces
//! (uptime, shutdown / reload requests, last-reload bookkeeping, sampled
//! process metrics) and the unbounded channel the application loop drains for
//! `ControlCommand`s.
//!
//! The controller is intentionally kept here in `core` so non-HTTP callers
//! (`src/app.rs`, `src/app/bootstrap.rs`, `src/plugin/registry/runtime.rs`)
//! can hold and signal it without depending on the `api` feature. The HTTP
//! adapter in [`crate::api::control`] only wires endpoints to the public
//! methods declared here.

use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sha2::{Digest, Sha256};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use tokio::sync::mpsc;

use crate::infra::clock::AppClock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCommand {
    Shutdown,
    Reload,
    Restart,
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessMetrics {
    pub cpu_percent: f32,
    pub memory_mb: u64,
    pub system_memory_total_mb: u64,
}

#[derive(Debug)]
pub struct AppController {
    started_at_ms: u64,
    config_path: PathBuf,
    state: Mutex<ControlState>,
    command_tx: mpsc::UnboundedSender<ControlCommand>,
    sysinfo: Mutex<System>,
}

#[derive(Debug, Default)]
struct ControlState {
    shutdown_requested: bool,
    reload_pending: bool,
    reload_in_progress: bool,
    last_reload_started_ms: Option<u64>,
    last_reload_completed_ms: Option<u64>,
    last_reload_success_ms: Option<u64>,
    last_reload_error: Option<String>,
    /// SHA256 of the config the backend has actually assembled and is running.
    running_config_version: Option<String>,
    /// SHA256 of the config the most recent reload attempted to apply.
    last_reload_target_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlSnapshot {
    pub status: &'static str,
    pub uptime_ms: u64,
    pub config_path: String,
    pub shutdown_requested: bool,
    pub reload: ReloadSnapshot,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReloadSnapshot {
    pub status: &'static str,
    pub pending: bool,
    pub in_progress: bool,
    pub last_started_ms: Option<u64>,
    pub last_completed_ms: Option<u64>,
    pub last_success_ms: Option<u64>,
    pub last_error: Option<String>,
    /// Config version (SHA256) the backend is actually running right now.
    pub running_version: Option<String>,
    /// Config version (SHA256) the most recent reload attempted to apply.
    pub target_version: Option<String>,
}

#[derive(Debug)]
pub enum ControlRequestError {
    ReloadBusy,
    CommandChannelClosed,
}

impl Display for ControlRequestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReloadBusy => write!(f, "reload is already pending or in progress"),
            Self::CommandChannelClosed => write!(f, "control command channel is closed"),
        }
    }
}

impl AppController {
    pub fn new(config_path: PathBuf) -> (Arc<Self>, mpsc::UnboundedReceiver<ControlCommand>) {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let pid = Pid::from_u32(std::process::id());
        let refresh_kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
        let mut sys =
            System::new_with_specifics(RefreshKind::nothing().with_processes(refresh_kind));
        // Prime the CPU baseline so the first real sample isn't always 0%.
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), false, refresh_kind);
        (
            Arc::new(Self {
                started_at_ms: AppClock::elapsed_millis(),
                config_path,
                state: Mutex::new(ControlState::default()),
                command_tx,
                sysinfo: Mutex::new(sys),
            }),
            command_rx,
        )
    }

    pub fn sample_process_metrics(&self) -> ProcessMetrics {
        let pid = Pid::from_u32(std::process::id());
        let mut sys = self.sysinfo.lock().expect("sysinfo poisoned");
        sys.refresh_memory();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let cpu_count = sys.cpus().len().max(1) as f32;
        let (cpu_percent, memory_mb) = sys
            .process(pid)
            .map(|p| (p.cpu_usage() / cpu_count, p.memory() / 1_048_576))
            .unwrap_or((0.0, 0));
        ProcessMetrics {
            cpu_percent,
            memory_mb,
            system_memory_total_mb: sys.total_memory() / 1_048_576,
        }
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn snapshot(&self) -> ControlSnapshot {
        let state = self.state.lock().expect("control state poisoned");
        ControlSnapshot {
            status: if state.shutdown_requested {
                "shutdown_requested"
            } else {
                "running"
            },
            uptime_ms: AppClock::elapsed_millis().saturating_sub(self.started_at_ms),
            config_path: self.config_path.display().to_string(),
            shutdown_requested: state.shutdown_requested,
            reload: state.reload_snapshot(),
        }
    }

    pub fn reload_snapshot(&self) -> ReloadSnapshot {
        self.state
            .lock()
            .expect("control state poisoned")
            .reload_snapshot()
    }

    pub fn request_shutdown(&self) -> std::result::Result<(), ControlRequestError> {
        {
            let mut state = self.state.lock().expect("control state poisoned");
            state.shutdown_requested = true;
        }
        self.command_tx
            .send(ControlCommand::Shutdown)
            .map_err(|_| ControlRequestError::CommandChannelClosed)
    }

    pub fn request_restart(&self) -> std::result::Result<(), ControlRequestError> {
        {
            let mut state = self.state.lock().expect("control state poisoned");
            state.shutdown_requested = true;
        }
        self.command_tx
            .send(ControlCommand::Restart)
            .map_err(|_| ControlRequestError::CommandChannelClosed)
    }

    pub fn request_reload(&self) -> std::result::Result<(), ControlRequestError> {
        {
            let mut state = self.state.lock().expect("control state poisoned");
            if state.reload_pending || state.reload_in_progress {
                return Err(ControlRequestError::ReloadBusy);
            }
            state.reload_pending = true;
            state.last_reload_error = None;
        }
        self.command_tx
            .send(ControlCommand::Reload)
            .map_err(|_| ControlRequestError::CommandChannelClosed)
    }

    /// Record the config version (SHA256) the backend is actually running.
    /// Called once after the initial assembly and after every successful
    /// reload so clients can authoritatively tell "on-disk" from "running".
    pub fn set_running_config_version(&self, version: Option<String>) {
        let mut state = self.state.lock().expect("control state poisoned");
        state.running_config_version = version;
    }

    pub fn mark_reload_started(&self, target_version: Option<String>) {
        let mut state = self.state.lock().expect("control state poisoned");
        state.reload_pending = false;
        state.reload_in_progress = true;
        state.last_reload_started_ms = Some(AppClock::elapsed_millis());
        state.last_reload_error = None;
        state.last_reload_target_version = target_version;
    }

    pub fn mark_reload_succeeded(&self) {
        let now = AppClock::elapsed_millis();
        let mut state = self.state.lock().expect("control state poisoned");
        state.reload_pending = false;
        state.reload_in_progress = false;
        state.last_reload_completed_ms = Some(now);
        state.last_reload_success_ms = Some(now);
        state.last_reload_error = None;
        // The applied target is now the running config.
        if state.last_reload_target_version.is_some() {
            state.running_config_version = state.last_reload_target_version.clone();
        }
    }

    pub fn mark_reload_failed(&self, message: impl Into<String>) {
        let mut state = self.state.lock().expect("control state poisoned");
        state.reload_pending = false;
        state.reload_in_progress = false;
        state.last_reload_completed_ms = Some(AppClock::elapsed_millis());
        state.last_reload_error = Some(message.into());
    }
}

impl ControlState {
    fn reload_snapshot(&self) -> ReloadSnapshot {
        ReloadSnapshot {
            status: if self.reload_in_progress {
                "in_progress"
            } else if self.reload_pending {
                "pending"
            } else if self.last_reload_error.is_some() {
                "failed"
            } else if self.last_reload_success_ms.is_some() {
                "ok"
            } else {
                "idle"
            },
            pending: self.reload_pending,
            in_progress: self.reload_in_progress,
            last_started_ms: self.last_reload_started_ms,
            last_completed_ms: self.last_reload_completed_ms,
            last_success_ms: self.last_reload_success_ms,
            last_error: self.last_reload_error.clone(),
            running_version: self.running_config_version.clone(),
            target_version: self.last_reload_target_version.clone(),
        }
    }
}

/// SHA256 of an in-memory config string. Stable across processes, so the API
/// can compare the on-disk and running versions to tell "in-sync" from
/// "needs reload."
pub fn config_version(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// SHA256 of the on-disk config file, matching the `version` that
/// `GET /config` reports. Returns `None` if the file can't be read.
pub fn config_file_version(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|c| config_version(&c))
}

#[cfg(test)]
mod tests {
    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn reload_snapshot_tracks_state_transitions() {
        AppClock::start();
        let temp = NamedTempFile::new().expect("temp file");
        let (controller, _rx) = AppController::new(temp.path().to_path_buf());
        assert_eq!(controller.reload_snapshot().status, "idle");

        controller.request_reload().expect("request reload");
        assert_eq!(controller.reload_snapshot().status, "pending");

        controller.mark_reload_started(None);
        assert_eq!(controller.reload_snapshot().status, "in_progress");

        controller.mark_reload_failed("boom");
        let snapshot = controller.reload_snapshot();
        assert_eq!(snapshot.status, "failed");
        assert_eq!(snapshot.last_error.as_deref(), Some("boom"));

        controller.request_reload().expect("request second reload");
        controller.mark_reload_started(None);
        controller.mark_reload_succeeded();
        assert_eq!(controller.reload_snapshot().status, "ok");
    }
}
