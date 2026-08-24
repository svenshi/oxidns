// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! OpenWrt/ImmortalWrt procd service integration.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use service_manager::ServiceStatus;

use crate::infra::error::{DnsError, Result};

const PROCD_PATH: &str = "/sbin/procd";
const RC_COMMON_PATH: &str = "/etc/rc.common";
const INIT_SCRIPT_PATH: &str = "/etc/init.d/oxidns";

pub(super) fn available() -> bool {
    procd_available_at(Path::new(PROCD_PATH), Path::new(RC_COMMON_PATH))
}

fn procd_available_at(procd: &Path, rc_common: &Path) -> bool {
    procd.is_file() && rc_common.is_file()
}

pub(super) fn install(program: &Path, config: &Path, working_dir: &Path) -> Result<()> {
    let script = render_init_script(program, config, working_dir)?;
    write_init_script_atomically(script.as_bytes())?;
    run_action("enable")
}

pub(super) fn status() -> Result<ServiceStatus> {
    let script = Path::new(INIT_SCRIPT_PATH);
    if !script.is_file() {
        return Ok(ServiceStatus::NotInstalled);
    }

    let output = run_init_script("running")?;
    if output.status.success() {
        Ok(ServiceStatus::Running)
    } else {
        Ok(ServiceStatus::Stopped(command_failure_reason(&output)))
    }
}

pub(super) fn run_action(action: &str) -> Result<()> {
    let output = run_init_script(action)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(DnsError::runtime(format!(
            "procd service action '{action}' failed{}",
            command_failure_reason(&output)
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default()
        )))
    }
}

pub(super) fn uninstall() -> Result<()> {
    let script = Path::new(INIT_SCRIPT_PATH);
    if !script.is_file() {
        return Ok(());
    }

    run_action("stop")?;
    run_action("disable")?;
    fs::remove_file(script).map_err(|err| {
        DnsError::runtime(format!(
            "Failed to remove procd init script {}: {err}",
            script.display()
        ))
    })
}

fn run_init_script(action: &str) -> Result<Output> {
    Command::new(INIT_SCRIPT_PATH)
        .arg(action)
        .output()
        .map_err(|err| {
            DnsError::runtime(format!("Failed to run {INIT_SCRIPT_PATH} {action}: {err}"))
        })
}

fn command_failure_reason(output: &Output) -> Option<String> {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return Some(stderr);
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stdout.is_empty()).then_some(stdout)
}

fn render_init_script(program: &Path, config: &Path, working_dir: &Path) -> Result<String> {
    let program = shell_quote_path(program)?;
    let config = shell_quote_path(config)?;
    let working_dir = shell_quote_path(working_dir)?;

    Ok(format!(
        "#!/bin/sh /etc/rc.common\n\
         \n\
         START=90\n\
         STOP=10\n\
         USE_PROCD=1\n\
         \n\
         start_service() {{\n\
         \tprocd_open_instance\n\
         \tprocd_set_param command {program} start -c {config} -d {working_dir}\n\
         \tprocd_set_param respawn 3600 3 0\n\
         \tprocd_set_param stdout 1\n\
         \tprocd_set_param stderr 1\n\
         \tprocd_close_instance\n\
         }}\n"
    ))
}

fn shell_quote_path(path: &Path) -> Result<String> {
    let value = path.to_str().ok_or_else(|| {
        DnsError::config(format!(
            "procd service paths must be valid UTF-8: {}",
            path.display()
        ))
    })?;
    Ok(format!("'{}'", value.replace('\'', "'\"'\"'")))
}

fn write_init_script_atomically(contents: &[u8]) -> Result<()> {
    let destination = Path::new(INIT_SCRIPT_PATH);
    let parent = destination.parent().ok_or_else(|| {
        DnsError::runtime(format!(
            "procd init script path has no parent: {}",
            destination.display()
        ))
    })?;
    let temporary = parent.join(format!(".oxidns.tmp.{}", std::process::id()));
    write_temporary_script(&temporary, contents)?;

    if let Err(err) = fs::rename(&temporary, destination) {
        let _ = fs::remove_file(&temporary);
        return Err(DnsError::runtime(format!(
            "Failed to install procd init script {}: {err}",
            destination.display()
        )));
    }
    Ok(())
}

fn write_temporary_script(path: &Path, contents: &[u8]) -> Result<()> {
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
        Ok::<_, std::io::Error>(())
    })();

    if let Err(err) = result {
        let _ = fs::remove_file(path);
        return Err(DnsError::runtime(format!(
            "Failed to write temporary procd init script {}: {err}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_procd_only_when_both_runtime_files_exist() {
        let temp = tempfile::tempdir().unwrap();
        let procd = temp.path().join("procd");
        let rc_common = temp.path().join("rc.common");
        fs::write(&procd, []).unwrap();
        assert!(!procd_available_at(&procd, &rc_common));
        fs::write(&rc_common, []).unwrap();
        assert!(procd_available_at(&procd, &rc_common));
    }

    #[test]
    fn init_script_quotes_paths_and_enables_respawn() {
        let script = render_init_script(
            Path::new("/usr/bin/oxidns"),
            Path::new("/etc/oxidns/user's config.yaml"),
            Path::new("/var/lib/oxidns data"),
        )
        .unwrap();

        assert!(script.starts_with("#!/bin/sh /etc/rc.common\n"));
        assert!(script.contains("USE_PROCD=1"));
        assert!(script.contains(
            "procd_set_param command '/usr/bin/oxidns' start -c '/etc/oxidns/user'\"'\"'s config.yaml' -d '/var/lib/oxidns data'"
        ));
        assert!(script.contains("procd_set_param respawn 3600 3 0"));
    }
}
