// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! CLI adapter for operating-system service management commands.

use crate::cli::{ServiceCommand, ServiceOptions};
use crate::infra::error::Result;
use crate::infra::service::ServiceInstallConfig;

pub fn run(options: ServiceOptions) -> Result<()> {
    match options.command {
        ServiceCommand::Install(install) => crate::infra::service::install(ServiceInstallConfig {
            working_dir: install.working_dir,
            config: install.config,
        }),
        ServiceCommand::Start => crate::infra::service::start(),
        ServiceCommand::Stop => crate::infra::service::stop(),
        ServiceCommand::Restart => crate::infra::service::restart_installed_service(),
        ServiceCommand::Uninstall => crate::infra::service::uninstall(),
    }
}
