// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Application startup and shutdown for API and plugin runtime components.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures::FutureExt;

#[cfg(feature = "api")]
use crate::api::{self, ApiHub, ApiRegister, clear_global_api, install_global_api};
use crate::config::types::Config;
use crate::infra::control::AppController;
use crate::infra::error::{DnsError, Result};
use crate::plugin;

#[derive(Debug, Default)]
pub struct AppAssembly {
    #[cfg(feature = "api")]
    pub api_hub: Option<Arc<ApiHub>>,
}

/// Initialize plugins and start the management API under the managed startup
/// lifecycle.
pub async fn start_application(
    config: &Config,
    controller: Option<Arc<AppController>>,
) -> Result<AppAssembly> {
    let config = config.clone();
    plugin::global_manager().start_with(move |session| Box::pin(async move {
        crate::infra::network::metrics::init()?;
        #[cfg(feature = "api")]
        let api_hub = ApiHub::from_config(&config.api)?;

        // Keep API preparation, plugin construction, listener startup, and
        // error cleanup inside the owned lifecycle operation. Dropping the
        // observer cannot skip activation or strand global API/controller state.
        let outcome = AssertUnwindSafe(async {
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
            let runtime = session.initialize(config).await?;
            #[cfg(feature = "api")]
            if let Some(api_hub) = &api_hub {
                let register = ApiRegister::new(api_hub.clone());
                api::register_plugin_runtime_control_routes(&register, &runtime)?;
                api_hub.mark_plugins_initialized(runtime.plugin_count(), runtime.server_plugin_count());
                api_hub.start().await?;
            }
            #[cfg(not(feature = "api"))]
            let _ = runtime;
            Ok(AppAssembly {
                #[cfg(feature = "api")]
                api_hub: api_hub.clone(),
            })
        }).catch_unwind().await
            .unwrap_or_else(|_| Err(DnsError::runtime("application startup panicked")));
        if outcome.is_err() {
            // The manager destroys the unactivated candidate after this block.
            // Stop and unpublish application resources first, using the same
            // ordering as normal teardown, before releasing lifecycle ownership.
            #[cfg(feature = "api")]
            {
                if let Some(api_hub) = &api_hub {
                    api_hub.stop().await;
                }
                clear_global_api();
            }
            plugin::clear_app_controller();
        }
        outcome
    })).await
}

/// Stop the management API, destroy plugins, and clean up background tasks.
pub async fn stop_application(assembly: &AppAssembly) {
    #[cfg(feature = "api")]
    let api_hub = assembly.api_hub.clone();
    #[cfg(not(feature = "api"))]
    let _ = assembly;
    plugin::global_manager()
        .stop_with(
            move || {
                Box::pin(async move {
                    #[cfg(feature = "api")]
                    {
                        clear_global_api();
                        if let Some(api_hub) = api_hub {
                            api_hub.stop().await;
                        }
                    }
                })
            },
            || {
                Box::pin(async {
                    // Keep process-wide task cleanup inside the lifecycle
                    // permit; otherwise a concurrent new startup could have its
                    // fresh maintenance jobs cancelled by the previous
                    // runtime's stop.
                    crate::infra::task::stop_all().await;
                    plugin::clear_app_controller();
                })
            },
        )
        .await;
}

#[cfg(all(test, feature = "api"))]
mod tests {
    use super::*;
    use crate::api::{
        ApiHub, ApiRegister, global_api_register, global_api_test_guard,
        set_global_api_register_for_test,
    };
    use crate::config::types::{ApiConfig, ApiHttpConfig, LogConfig, NetworkConfig, RuntimeConfig};
    use crate::infra::clock::AppClock;

    #[tokio::test]
    async fn failed_api_start_preserves_cache_dump() {
        let _guard = global_api_test_guard().await;
        let _outbound_guard = crate::infra::network::outbound::TestGlobalGuard::clean();
        AppClock::start();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cache.dump");
        let baseline = crate::plugin::executor::cache::persistence_test_dump();
        std::fs::write(&file, &baseline).unwrap();
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config: Config = serde_yaml_ng::from_str(&format!(
            "api:\n  http: '{}'\nplugins:\n  - tag: api_failure_cache\n    type: cache\n    args:\n      size: 1\n      dump_file: '{}'",
            occupied.local_addr().unwrap(), file.display())).unwrap();
        assert!(start_application(&config, None).await.is_err());
        assert_eq!(std::fs::read(&file).unwrap(), baseline);
        assert!(plugin::current_runtime().is_none());
    }

    #[tokio::test]
    async fn start_application_without_api_config_does_not_register_api() {
        let _guard = global_api_test_guard().await;
        let _outbound_guard = crate::infra::network::outbound::TestGlobalGuard::clean();
        AppClock::start();
        let stale_hub = ApiHub::from_config(&ApiConfig {
            http: Some(ApiHttpConfig::Listen("127.0.0.1:0".to_string())),
        })
        .expect("stale api config should parse")
        .expect("stale api hub should exist");
        set_global_api_register_for_test(Some(ApiRegister::new(stale_hub)));

        let assembly = start_application(
            &Config {
                include: Vec::new(),
                runtime: RuntimeConfig::default(),
                api: ApiConfig::default(),
                log: LogConfig::default(),
                network: NetworkConfig::default(),
                plugins: Vec::new(),
            },
            None,
        )
        .await
        .expect("empty config should start");

        assert!(assembly.api_hub.is_none());
        assert!(global_api_register().is_none());

        stop_application(&assembly).await;
    }
}

#[cfg(all(test, not(feature = "api")))]
mod tests {
    use super::*;
    use crate::config::types::{ApiConfig, ApiHttpConfig, LogConfig, NetworkConfig, RuntimeConfig};
    use crate::infra::clock::AppClock;

    /// Without the `api` feature, a config that still sets `api.http` is a
    /// version/feature mismatch that does not prevent the server from running:
    /// `start_application` should warn and succeed (no management listener),
    /// not error.
    #[tokio::test]
    async fn start_application_warns_but_succeeds_when_api_http_set_without_feature() {
        let _outbound_guard = crate::infra::network::outbound::TestGlobalGuard::clean();
        AppClock::start();
        let assembly = start_application(
            &Config {
                include: Vec::new(),
                runtime: RuntimeConfig::default(),
                api: ApiConfig {
                    http: Some(ApiHttpConfig::Listen("127.0.0.1:0".to_string())),
                },
                log: LogConfig::default(),
                network: NetworkConfig::default(),
                plugins: Vec::new(),
            },
            None,
        )
        .await
        .expect("api.http without the api feature should warn, not fail");

        stop_application(&assembly).await;
    }
}
