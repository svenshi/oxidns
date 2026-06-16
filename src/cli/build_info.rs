// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! CLI support for printing compiled build information.

use crate::infra::error::Result;

pub fn run() -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&crate::infra::build_info::snapshot()?)?
    );
    Ok(())
}
