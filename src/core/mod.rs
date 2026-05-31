// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared runtime primitives used across the whole OxiDNS process.
//!
//! This module contains the small set of foundational types that most other
//! subsystems depend on:
//!
//! - [`app_clock`]: low-overhead elapsed-time tracking for logs and metrics.
//! - [`app_controller`]: shutdown/reload control state shared with the API
//!   layer.
//! - [`context`]: [`context::DnsContext`] and related state passed through the
//!   plugin pipeline during request execution.
//! - [`mod@env`]: shared process environment variable access helpers.
//! - [`error`]: common error types and result aliases.
//! - [`log_buffer`]: in-memory log ring + tracing layer shared with the API
//!   layer.
//! - [`metrics`]: shared low-overhead plugin metrics registry and renderer.
//! - [`rule_matcher`]: reusable domain and string matching helpers.
//! - [`task_center`]: shared async task orchestration helpers.
//! - [`ttl_cache`]: concurrent TTL-aware cache building block.
//!
//! Code in this module should stay generic, hot-path aware, and free from
//! plugin-specific policy decisions.

pub mod app_clock;
pub mod app_controller;
pub mod context;
pub mod env;
pub mod error;
pub mod log_buffer;
pub mod metrics;
pub mod rule_matcher;
pub mod system_utils;
pub mod task_center;
pub mod ttl_cache;

/// OxiDNS version shared by CLI and management APIs.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
