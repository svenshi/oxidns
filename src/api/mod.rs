// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Management HTTP API hub and route registration.
//!
//! This module provides the optional control-plane HTTP server used for health
//! endpoints, lifecycle control, and plugin-specific API surfaces.
//!
//! Core responsibilities:
//!
//! - normalize and validate the configured API listen address;
//! - host a small in-process route registry keyed by method and path;
//! - provide [`ApiRegister`] so built-in components and plugins can expose
//!   endpoints without coupling to the HTTP server implementation;
//! - enforce optional basic authentication and TLS; and
//! - publish shared health state about startup, plugin initialization, and
//!   shutdown.
//!
//! The API layer is intentionally separate from the DNS request path. It shares
//! runtime state with the application, but it does not participate in query
//! matching or response generation.

mod auth;
mod build;
pub mod control;
mod cors;
mod global;
mod handler;
pub mod health;
mod hub;
pub mod logs;
#[cfg(feature = "metrics")]
mod metrics;
mod request;
mod response;
mod route;
mod server;
#[cfg(feature = "webui")]
mod static_files;
#[cfg(feature = "plugin-upgrade")]
mod upgrade;
mod webui_config;

use std::sync::Arc;

#[cfg(test)]
pub(super) use auth::is_authorized;
#[cfg(test)]
pub(crate) use global::global_api_test_guard;
#[cfg(test)]
pub(crate) use global::set_global_api_register_for_test;
pub use global::{clear_global_api, global_api_register, install_global_api};
pub use handler::{ApiBody, ApiHandler, ApiResponse};
pub use hub::{ApiHub, ApiRegister, PluginApiRegister};
#[cfg(test)]
pub(super) use request::{rewrite_request_path, strip_api_prefix};
pub use response::{json_error, json_ok, json_response, simple_response, streaming_response};
#[cfg(test)]
pub(super) use route::build_plugin_route_path;

use crate::infra::error::Result;

/// Register process-wide API routes that do not depend on application control
/// state.
///
/// Plugin-scoped routes are still registered by each plugin under
/// `/plugins/<tag>/...`; this function is only for global API surfaces such as
/// health and metrics.
pub fn register_builtin_routes() -> Result<()> {
    if let Some(register) = global_api_register() {
        health::register_builtin_routes(&register, register.health_state())?;
        #[cfg(feature = "metrics")]
        metrics::register_builtin_routes(&register)?;
        logs::register_log_routes(&register)?;
        build::register_builtin_routes(&register)?;
        #[cfg(feature = "plugin-upgrade")]
        upgrade::register_upgrade_routes(&register)?;
    }
    Ok(())
}

/// Register process-wide control routes that need the application controller.
pub fn register_control_routes(
    controller: Arc<crate::infra::control::AppController>,
) -> Result<()> {
    if let Some(register) = global_api_register() {
        control::register_builtin_routes(&register, controller.clone())?;
        webui_config::register_builtin_routes(&register, controller)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
