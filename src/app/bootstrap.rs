// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Application assembly helpers for wiring API and plugin runtime components.

use std::sync::Arc;

#[cfg(feature = "api")]
use crate::api::{self, ApiHub, clear_global_api, install_global_api};
use crate::config::types::Config;
use crate::core::app_controller::AppController;
use crate::core::error::Result;
use crate::plugin;

#[derive(Debug, Default)]
pub struct AppAssembly {
    #[cfg(feature = "api")]
    pub api_hub: Option<Arc<ApiHub>>,
}

pub async fn assemble(
    config: &Config,
    controller: Option<Arc<AppController>>,
) -> Result<AppAssembly> {
    #[cfg(feature = "api")]
    let api_hub = ApiHub::from_config(&config.api)?;
    #[cfg(feature = "api")]
    if let Some(api_hub) = &api_hub {
        install_global_api(api_hub.clone());
        api::register_builtin_routes()?;
        if let Some(controller) = &controller {
            api::register_control_routes(controller.clone())?;
        }
    } else {
        clear_global_api();
    }
    // A version/feature mismatch that does not stop the server from running:
    // DNS forwarding works fine without the management API, so warn loudly and
    // carry on instead of failing startup.
    #[cfg(not(feature = "api"))]
    if config.api.http.is_some() {
        tracing::warn!(
            "api.http is configured but this build was compiled without the `api` feature; \
             the management API (health / control / logs / config endpoints) will not start. \
             Rebuild with --features api to enable it, or remove the api block to silence this warning."
        );
    }

    if let Some(controller) = &controller {
        plugin::set_app_controller(controller.clone());
    } else {
        plugin::clear_app_controller();
    }

    let runtime = match plugin::init(config.clone()).await {
        Ok(runtime) => runtime,
        Err(err) => {
            #[cfg(feature = "api")]
            clear_global_api();
            plugin::clear_app_controller();
            return Err(err);
        }
    };

    #[cfg(feature = "api")]
    if let Some(api_hub) = &api_hub {
        api_hub.mark_plugins_initialized(runtime.plugin_count(), runtime.server_plugin_count());
        if let Err(err) = api_hub.start().await {
            plugin::destroy_runtime().await;
            clear_global_api();
            plugin::clear_app_controller();
            return Err(err);
        }
    }
    #[cfg(not(feature = "api"))]
    {
        let _ = runtime;
    }

    Ok(AppAssembly {
        #[cfg(feature = "api")]
        api_hub,
    })
}

pub async fn stop(assembly: &AppAssembly) {
    #[cfg(feature = "api")]
    {
        clear_global_api();
        if let Some(api_hub) = &assembly.api_hub {
            api_hub.stop().await;
        }
    }
    #[cfg(not(feature = "api"))]
    let _ = assembly;
    plugin::destroy_runtime().await;
    plugin::clear_app_controller();
}

#[cfg(all(test, feature = "api"))]
mod tests {
    use super::*;
    use crate::api::{
        ApiHub, ApiRegister, global_api_register, global_api_test_guard,
        set_global_api_register_for_test,
    };
    use crate::config::types::{ApiConfig, ApiHttpConfig, LogConfig, RuntimeConfig};
    use crate::core::app_clock::AppClock;

    #[tokio::test]
    async fn assemble_without_api_config_does_not_register_api() {
        let _guard = global_api_test_guard().await;
        AppClock::start();
        let stale_hub = ApiHub::from_config(&ApiConfig {
            http: Some(ApiHttpConfig::Listen("127.0.0.1:0".to_string())),
        })
        .expect("stale api config should parse")
        .expect("stale api hub should exist");
        set_global_api_register_for_test(Some(ApiRegister::new(stale_hub)));

        let assembly = assemble(
            &Config {
                include: Vec::new(),
                runtime: RuntimeConfig::default(),
                api: ApiConfig::default(),
                log: LogConfig::default(),
                plugins: Vec::new(),
            },
            None,
        )
        .await
        .expect("empty config should assemble");

        assert!(assembly.api_hub.is_none());
        assert!(global_api_register().is_none());

        stop(&assembly).await;
    }
}

#[cfg(all(test, not(feature = "api")))]
mod tests {
    use super::*;
    use crate::config::types::{ApiConfig, ApiHttpConfig, LogConfig, RuntimeConfig};
    use crate::core::app_clock::AppClock;

    /// Without the `api` feature, a config that still sets `api.http` is a
    /// version/feature mismatch that does not prevent the server from running:
    /// `assemble` should warn and succeed (no management listener), not error.
    #[tokio::test]
    async fn assemble_warns_but_succeeds_when_api_http_set_without_feature() {
        AppClock::start();
        let assembly = assemble(
            &Config {
                include: Vec::new(),
                runtime: RuntimeConfig::default(),
                api: ApiConfig {
                    http: Some(ApiHttpConfig::Listen("127.0.0.1:0".to_string())),
                },
                log: LogConfig::default(),
                plugins: Vec::new(),
            },
            None,
        )
        .await
        .expect("api.http without the api feature should warn, not fail");

        stop(&assembly).await;
    }
}
