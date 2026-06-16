// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! OxiDNS binary entry point.
//!
//! The binary is intentionally thin: it parses CLI arguments and delegates to
//! either foreground runtime startup or operating-system service management.

use oxidns::cli;
use oxidns::infra::error::Result;

fn main() -> Result<()> {
    #[cfg(windows)]
    if oxidns::infra::service::try_dispatch_windows_service()? {
        return Ok(());
    }

    cli::run()
}
