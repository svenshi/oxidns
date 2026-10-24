// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Upgrade installation helpers.

mod binary;
mod webui;

#[cfg(not(windows))]
pub(super) use binary::replace_binary;
#[cfg(windows)]
pub(super) use binary::replace_binary_windows;
#[cfg(all(test, not(windows)))]
pub(super) use webui::copy_dir_all;
pub(super) use webui::{find_extracted_webui, replace_webui};
