// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later
//! Provider plugin category.
//!
//! Providers expose reusable datasets to other plugins, especially matchers and
//! executors that need fast membership checks without duplicating parsing or
//! storage logic.
//!
//! Common use cases include:
//!
//! - domain-set membership for qname and CNAME decisions;
//! - IP-set membership for client IP, response IP, or routing behavior; and
//! - typed provider-specific access via downcasting when a plugin needs richer
//!   capabilities than the generic membership helpers.
//!
//! Providers are initialized once, then shared through the plugin registry.
//! Their per-request API should stay read-only and cheap.

use std::any::Any;
use std::net::IpAddr;
#[cfg(not(feature = "api"))]
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;

use crate::infra::error::{DnsError, Result as DnsResult};
#[cfg(not(feature = "api"))]
use crate::plugin;
use crate::plugin::Plugin;
use crate::proto::{Name, Question};

#[cfg(feature = "provider-adguard-rule")]
pub mod adguard_rule;
pub mod domain_set;
#[cfg(feature = "plugin-dynamic-domain")]
pub mod dynamic_domain_set;
#[cfg(feature = "provider-protobuf")]
pub mod geoip;
#[cfg(feature = "provider-protobuf")]
pub mod geosite;
pub mod ip_set;
pub(crate) mod provider_utils;
#[cfg(feature = "provider-protobuf")]
pub(crate) mod v2ray_dat;

#[async_trait]
#[allow(dead_code)]
pub trait Provider: Plugin {
    /// Type-erased view for provider-specific downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Domain membership check using an owned DNS name.
    #[inline]
    fn contains_name(&self, _name: &Name) -> bool {
        false
    }

    /// Question-level membership check for providers that need request question
    /// context.
    #[inline]
    fn contains_question(&self, _question: &Question) -> bool {
        false
    }

    /// Fast-path IP membership check for hot matcher paths.
    fn contains_ip(&self, _ip: IpAddr) -> bool {
        false
    }

    /// Reload the provider's internal data using the same startup config.
    async fn reload(&self) -> DnsResult<()> {
        Err(DnsError::plugin(format!(
            "provider '{}' does not support reload",
            self.tag()
        )))
    }

    fn runtime_status(&self) -> ProviderRuntimeStatus {
        ProviderRuntimeStatus {
            ok: true,
            plugin: self.tag().to_string(),
            supports_reload: false,
            supports_domain_matching: self.supports_domain_matching(),
            supports_ip_matching: self.supports_ip_matching(),
            last_reload_ms: None,
            last_error: None,
            rule_stats: None,
        }
    }

    #[inline]
    fn supports_ip_matching(&self) -> bool {
        false
    }

    #[inline]
    fn supports_domain_matching(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ProviderRuleStats {
    pub total_rules: Option<usize>,
    pub supported_rules: Option<usize>,
    pub skipped_rules: Option<usize>,
    pub exception_rules: Option<usize>,
    pub important_rules: Option<usize>,
    pub full_rules: Option<usize>,
    pub domain_rules: Option<usize>,
    pub keyword_rules: Option<usize>,
    pub regex_rules: Option<usize>,
    pub v4_rules: Option<usize>,
    pub v6_rules: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderRuntimeStatus {
    pub ok: bool,
    pub plugin: String,
    pub supports_reload: bool,
    pub supports_domain_matching: bool,
    pub supports_ip_matching: bool,
    pub last_reload_ms: Option<u64>,
    pub last_error: Option<String>,
    pub rule_stats: Option<ProviderRuleStats>,
}

#[cfg(feature = "api")]
mod api_routes {
    use std::sync::Arc;

    use async_trait::async_trait;
    use bytes::Bytes;
    use http::{Request, StatusCode};
    use serde::Serialize;

    use crate::api::{ApiHandler, json_error, json_ok};
    use crate::infra::error::Result as DnsResult;
    use crate::plugin::{self, PluginRegistry};
    use crate::register_plugin_api;

    #[derive(Debug, Serialize)]
    struct ProviderReloadResponse {
        ok: bool,
        action: &'static str,
        provider: String,
        status: &'static str,
    }

    #[derive(Debug)]
    struct ProviderReloadHandler {
        tag: String,
    }

    #[derive(Debug)]
    struct ProviderStatusHandler {
        tag: String,
    }

    #[async_trait]
    impl ApiHandler for ProviderReloadHandler {
        async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
            match plugin::reload_provider(&self.tag).await {
                Ok(()) => json_ok(
                    StatusCode::OK,
                    &ProviderReloadResponse {
                        ok: true,
                        action: "reload_provider",
                        provider: self.tag.clone(),
                        status: "reloaded",
                    },
                ),
                Err(err) => json_error(
                    StatusCode::BAD_REQUEST,
                    "provider_reload_failed",
                    err.to_string(),
                ),
            }
        }
    }

    #[async_trait]
    impl ApiHandler for ProviderStatusHandler {
        async fn handle(&self, _request: Request<Bytes>) -> crate::api::ApiResponse {
            let Some(runtime) = plugin::current_runtime() else {
                return json_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "provider_runtime_unavailable",
                    "plugin runtime is not available",
                );
            };
            let Some(plugin_info) = runtime.get_plugin(&self.tag) else {
                return json_error(
                    StatusCode::NOT_FOUND,
                    "provider_not_found",
                    format!("provider '{}' was not found", self.tag),
                );
            };
            if plugin_info.plugin_type != crate::plugin::PluginType::Provider {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "plugin_is_not_provider",
                    format!("plugin '{}' is not a provider", self.tag),
                );
            }
            json_ok(StatusCode::OK, &plugin_info.to_provider().runtime_status())
        }
    }

    pub(crate) fn register_reload_api_route(
        _registry: Arc<PluginRegistry>,
        tag: &str,
    ) -> DnsResult<()> {
        register_plugin_api!(
            tag,
            GET "/status" => ProviderStatusHandler {
                tag: tag.to_string(),
            },
            POST "/reload" => ProviderReloadHandler {
                tag: tag.to_string(),
            },
        )
    }
}

#[cfg(feature = "api")]
pub(crate) use api_routes::register_reload_api_route;

#[cfg(not(feature = "api"))]
pub(crate) fn register_reload_api_route(
    _registry: Arc<plugin::PluginRegistry>,
    _tag: &str,
) -> DnsResult<()> {
    Ok(())
}

#[cfg(all(test, feature = "api"))]
mod tests {
    use std::net::{SocketAddr, TcpListener as StdTcpListener};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use http::{Method, Request as HttpRequest, StatusCode, Uri};
    use http_body_util::{BodyExt, Empty};
    use hyper_util::client::legacy::Client;
    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::rt::TokioExecutor;

    use super::*;
    use crate::api::{ApiHub, clear_global_api, global_api_test_guard, install_global_api};
    use crate::config::types::{ApiConfig, ApiHttpConfig, PluginConfig};
    use crate::infra::clock::AppClock;
    use crate::plugin::dependency::DependencyKind;
    use crate::plugin::matcher::qname::QnameFactory;
    use crate::plugin::{self, PluginFactory, PluginRegistry, UninitializedPlugin};

    fn reserve_local_addr() -> SocketAddr {
        let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let addr = listener.local_addr().expect("local addr");
        drop(listener);
        addr
    }

    #[derive(Debug)]
    struct ReloadableProvider {
        tag: String,
        reload_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Plugin for ReloadableProvider {
        fn tag(&self) -> &str {
            &self.tag
        }

        async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> DnsResult<()> {
            Ok(())
        }

        async fn destroy(&self) -> DnsResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Provider for ReloadableProvider {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn contains_name(&self, _name: &Name) -> bool {
            false
        }

        async fn reload(&self) -> DnsResult<()> {
            self.reload_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn supports_domain_matching(&self) -> bool {
            true
        }
    }

    #[derive(Debug)]
    struct ReloadableProviderFactory {
        reload_count: Arc<AtomicUsize>,
    }

    impl PluginFactory for ReloadableProviderFactory {
        fn create(
            &self,
            plugin_config: &PluginConfig,
            _init_context: &crate::plugin::PluginInitContext<'_>,
        ) -> DnsResult<UninitializedPlugin> {
            Ok(UninitializedPlugin::Provider(Box::new(
                ReloadableProvider {
                    tag: plugin_config.tag.clone(),
                    reload_count: self.reload_count.clone(),
                },
            )))
        }
    }

    #[test]
    fn provider_status_reports_default_runtime_capabilities() {
        let provider = ReloadableProvider {
            tag: "reloadable".to_string(),
            reload_count: Arc::new(AtomicUsize::new(0)),
        };

        let status = provider.runtime_status();

        assert!(status.ok);
        assert_eq!(status.plugin, "reloadable");
        assert!(!status.supports_reload);
        assert!(status.supports_domain_matching);
        assert!(!status.supports_ip_matching);
        assert!(status.rule_stats.is_none());
    }

    #[tokio::test]
    async fn provider_reload_api_calls_targeted_reload() -> DnsResult<()> {
        let _guard = global_api_test_guard().await;
        clear_global_api();
        plugin::reset_runtime_for_test().await;
        AppClock::start();
        let listen = reserve_local_addr();
        let hub = ApiHub::from_config(&ApiConfig {
            http: Some(ApiHttpConfig::Listen(listen.to_string())),
        })?
        .expect("api hub should be created");
        let reload_count = Arc::new(AtomicUsize::new(0));

        install_global_api(hub.clone());
        let mut registry = PluginRegistry::new();
        registry.register_factory("qname", DependencyKind::Matcher, Box::new(QnameFactory {}));
        registry.register_factory(
            "reloadable_provider",
            DependencyKind::Provider,
            Box::new(ReloadableProviderFactory {
                reload_count: reload_count.clone(),
            }),
        );
        let registry = Arc::new(registry);

        let configs = vec![
            PluginConfig {
                tag: "reloadable".to_string(),
                plugin_type: "reloadable_provider".to_string(),
                args: None,
            },
            PluginConfig {
                tag: "match_qname".to_string(),
                plugin_type: "qname".to_string(),
                args: Some(serde_yaml_ng::from_str("- \"$reloadable\"").unwrap()),
            },
        ];

        registry
            .clone()
            .init_plugins(configs)
            .await
            .expect("plugin init should succeed");
        plugin::set_current_runtime_for_test(registry.clone()).await;
        hub.start().await.expect("api hub should start");

        let client: Client<HttpConnector, Empty<bytes::Bytes>> =
            Client::builder(TokioExecutor::new()).build(HttpConnector::new());
        let uri: Uri = format!("http://{listen}/api/plugins/reloadable/reload")
            .parse()
            .expect("uri should parse");
        let request = HttpRequest::builder()
            .method(Method::POST)
            .uri(uri)
            .body(Empty::new())
            .expect("request should build");
        let response = client
            .request(request)
            .await
            .expect("request should succeed");
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();

        assert_eq!(status, StatusCode::OK);
        assert_eq!(reload_count.load(Ordering::Relaxed), 1);
        let payload = serde_json::from_slice::<serde_json::Value>(&body)
            .expect("response should be valid json");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["action"], "reload_provider");
        assert_eq!(payload["provider"], "reloadable");

        hub.stop().await;
        plugin::reset_runtime_for_test().await;
        clear_global_api();
        Ok(())
    }
}
