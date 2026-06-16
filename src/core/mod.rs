// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! DNS request execution core.
//!
//! This module owns the request-local model used by the plugin pipeline:
//!
//! - [`context`]: [`context::DnsContext`] and related request lifecycle state.
//! - [`rule_matcher`]: reusable domain and IP matching primitives used by
//!   matchers and providers.

pub mod context;
pub mod rule_matcher;
