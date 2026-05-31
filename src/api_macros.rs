// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plugin API registration macros.
//!
//! These are always compiled so plugin code can call `register_plugin_api!`
//! and `register_api_route!` unconditionally. When the `api` feature is
//! disabled the macros expand to a no-op `Ok(())`, which keeps plugin
//! source free of `#[cfg(feature = "api")]` clutter at the call sites.

#[cfg(feature = "api")]
#[macro_export]
macro_rules! register_plugin_api {
    ($tag:expr, |$plugin_api:ident| $($method:ident $path:expr => $handler:expr),+ $(,)?) => {{
        (|| -> $crate::core::error::Result<()> {
            if let Some(api_register) = $crate::api::global_api_register() {
                let $plugin_api = api_register.plugin($tag)?;
                $(
                    $crate::register_plugin_api!(@register $plugin_api, $method, $path, $handler)?;
                )+
            }
            Ok(())
        })()
    }};
    ($tag:expr, $($method:ident $path:expr => $handler:expr),+ $(,)?) => {{
        (|| -> $crate::core::error::Result<()> {
            if let Some(api_register) = $crate::api::global_api_register() {
                let plugin_api = api_register.plugin($tag)?;
                $(
                    $crate::register_plugin_api!(@register plugin_api, $method, $path, $handler)?;
                )+
            }
            Ok(())
        })()
    }};
    (@register $plugin_api:ident, GET, $path:expr, $handler:expr) => {
        $plugin_api.get($path, std::sync::Arc::new($handler))
    };
    (@register $plugin_api:ident, POST, $path:expr, $handler:expr) => {
        $plugin_api.post($path, std::sync::Arc::new($handler))
    };
    (@register $plugin_api:ident, DELETE, $path:expr, $handler:expr) => {
        $plugin_api.delete($path, std::sync::Arc::new($handler))
    };
    (@register $plugin_api:ident, GET_PREFIX, $path:expr, $handler:expr) => {
        $plugin_api.get_prefix($path, std::sync::Arc::new($handler))
    };
    (@register $plugin_api:ident, POST_PREFIX, $path:expr, $handler:expr) => {
        $plugin_api.post_prefix($path, std::sync::Arc::new($handler))
    };
    (@register $plugin_api:ident, DELETE_PREFIX, $path:expr, $handler:expr) => {
        $plugin_api.delete_prefix($path, std::sync::Arc::new($handler))
    };
}

/// No-op variant compiled when the `api` feature is disabled. Plugin code
/// can keep calling `register_plugin_api!(...)?` unchanged.
#[cfg(not(feature = "api"))]
#[macro_export]
macro_rules! register_plugin_api {
    ($($tt:tt)*) => {
        $crate::core::error::Result::<()>::Ok(())
    };
}

#[cfg(feature = "api")]
#[macro_export]
macro_rules! register_api_route {
    ($method:ident $path:expr => $handler:expr $(,)?) => {{
        (|| -> $crate::core::error::Result<()> {
            if let Some(api_register) = $crate::api::global_api_register() {
                $crate::register_api_route!(@register api_register, $method, $path, $handler)?;
            }
            Ok(())
        })()
    }};
    (@register $api_register:ident, GET, $path:expr, $handler:expr) => {
        $api_register.register_get($path, std::sync::Arc::new($handler))
    };
    (@register $api_register:ident, POST, $path:expr, $handler:expr) => {
        $api_register.register_post($path, std::sync::Arc::new($handler))
    };
    (@register $api_register:ident, DELETE, $path:expr, $handler:expr) => {
        $api_register.register_delete($path, std::sync::Arc::new($handler))
    };
    (@register $api_register:ident, GET_PREFIX, $path:expr, $handler:expr) => {
        $api_register.register_get_prefix($path, std::sync::Arc::new($handler))
    };
    (@register $api_register:ident, POST_PREFIX, $path:expr, $handler:expr) => {
        $api_register.register_post_prefix($path, std::sync::Arc::new($handler))
    };
    (@register $api_register:ident, DELETE_PREFIX, $path:expr, $handler:expr) => {
        $api_register.register_delete_prefix($path, std::sync::Arc::new($handler))
    };
}

#[cfg(not(feature = "api"))]
#[macro_export]
macro_rules! register_api_route {
    ($($tt:tt)*) => {
        $crate::core::error::Result::<()>::Ok(())
    };
}
