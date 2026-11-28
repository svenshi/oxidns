// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;
use tracing::warn;

use super::config::{MAX_CONCURRENT_QUERIES, resolve_active_concurrent};
use super::is_timeout_error;
use super::metrics::ForwardMetrics;
use super::selection::{ResponseSelectionMode, select_response};
use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::core::response::ResponseDisposition;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::default_resolver::DefaultResolver;
use crate::infra::network::upstream::{ConnectionInfo, Upstream, UpstreamBuilder};
use crate::infra::observability::metrics::{register_metric_source, unregister_metric_source};
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::{Plugin, PluginFactory, PluginInitContext, UninitializedPlugin};
use crate::plugin_factory;
use crate::proto::Message;

const PLUGIN_TYPE: &str = "dhcp_dns_servers";

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct DhcpDnsServersConfig {
    concurrent: Option<usize>,
    response_selection: ResponseSelectionMode,
    short_circuit: bool,
}

#[derive(Debug)]
#[plugin_factory("dhcp_dns_servers")]
struct DhcpDnsServersFactory;

impl PluginFactory for DhcpDnsServersFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _context: &PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        if !crate::infra::network::default_resolver::is_supported() {
            return Err(DnsError::plugin(
                "dhcp_dns_servers is not supported on this platform",
            ));
        }
        let config = plugin_config
            .args
            .clone()
            .map(serde_yaml_ng::from_value::<DhcpDnsServersConfig>)
            .transpose()
            .map_err(|e| DnsError::plugin(format!("failed to parse dhcp_dns_servers config: {e}")))?
            .unwrap_or_default();
        Ok(UninitializedPlugin::Executor(Box::new(
            DhcpDnsServersForwarder::new(plugin_config.tag.clone(), config),
        )))
    }
}

#[derive(Debug, Default)]
struct TransportGeneration {
    upstreams: Vec<Arc<dyn Upstream>>,
    invalidated: CancellationToken,
}

#[derive(Debug, Default)]
struct TransportState {
    snapshot: ArcSwapOption<TransportGeneration>,
    ready: Notify,
}

impl TransportState {
    async fn wait_ready(&self) -> Arc<TransportGeneration> {
        loop {
            let ready = self.ready.notified();
            if let Some(snapshot) = self.snapshot.load_full() {
                return snapshot;
            }
            ready.await;
        }
    }

    fn publish(&self, next: TransportGeneration) {
        if let Some(previous) = self.snapshot.swap(Some(Arc::new(next))) {
            previous.invalidated.cancel();
        }
        self.ready.notify_waiters();
    }

    async fn run(self: Arc<Self>) {
        if let Err(error) = self.watch().await {
            warn!(%error, "Default resolver network notifications stopped; clearing cache");
            self.publish(TransportGeneration::default());
        }
    }

    async fn watch(&self) -> std::io::Result<()> {
        let mut resolver = DefaultResolver::new()?;
        loop {
            let upstreams = resolver
                .refresh()
                .await?
                .iter()
                .take(MAX_CONCURRENT_QUERIES)
                .filter_map(|ip| {
                    match ConnectionInfo::with_addr(&ip.to_string())
                        .and_then(UpstreamBuilder::with_connection_info)
                    {
                        Ok(upstream) => Some(Arc::from(upstream)),
                        Err(error) => {
                            warn!(%error, "Could not build discovered DNS upstream");
                            None
                        }
                    }
                })
                .collect();
            self.publish(TransportGeneration {
                upstreams,
                invalidated: CancellationToken::new(),
            });
            resolver.changed().await?;
            // Clear and cancel the prior generation even if the same IPs
            // are reachable again after this network transition.
            if let Some(previous) = self.snapshot.swap(None) {
                previous.invalidated.cancel();
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct DefaultResolverTransports {
    state: Arc<TransportState>,
    worker: OnceLock<AbortOnDropHandle<()>>,
}

impl DefaultResolverTransports {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(TransportState::default()),
            worker: OnceLock::new(),
        }
    }

    async fn get(&self) -> Arc<TransportGeneration> {
        if let Some(snapshot) = self.state.snapshot.load_full() {
            return snapshot;
        }
        self.worker
            .get_or_init(|| AbortOnDropHandle::new(tokio::spawn(self.state.clone().run())));
        self.state.wait_ready().await
    }
}

#[derive(Debug)]
struct DhcpDnsServersForwarder {
    tag: String,
    concurrent: Option<usize>,
    response_selection: ResponseSelectionMode,
    short_circuit: bool,
    transports: Option<Arc<DefaultResolverTransports>>,
    metrics: Arc<ForwardMetrics>,
}

impl DhcpDnsServersForwarder {
    fn new(tag: String, config: DhcpDnsServersConfig) -> Self {
        Self {
            metrics: Arc::new(ForwardMetrics::for_plugin(
                tag.clone(),
                Vec::new(),
                PLUGIN_TYPE,
            )),
            tag,
            concurrent: config.concurrent,
            response_selection: config.response_selection,
            short_circuit: config.short_circuit,
            transports: None,
        }
    }
}

#[async_trait]
impl Plugin for DhcpDnsServersForwarder {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, context: &PluginInitContext<'_>) -> Result<()> {
        register_metric_source(self.metrics.clone())?;
        self.transports = Some(context.default_resolver_transports());
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        unregister_metric_source(&self.tag);
        Ok(())
    }
}

#[async_trait]
impl Executor for DhcpDnsServersForwarder {
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        let start = self.metrics.record_query_start();
        let transports = self
            .transports
            .as_ref()
            .ok_or_else(|| DnsError::plugin("dhcp_dns_servers was not initialized"))?;
        let snapshot = transports.get().await;
        let result = tokio::select! {
            biased;
            _ = snapshot.invalidated.cancelled() => Err(DnsError::plugin("default network changed during DNS query")),
            result = self.query(&snapshot.upstreams, &context.request) => result,
        };
        match result {
            Ok(message) => {
                context.set_response(message);
                self.metrics.record_success(start);
                Ok(if self.short_circuit {
                    ExecStep::Stop
                } else {
                    ExecStep::Next
                })
            }
            Err(error) => {
                self.metrics.record_error(start, is_timeout_error(&error));
                Err(error)
            }
        }
    }
}

impl DhcpDnsServersForwarder {
    async fn query(&self, upstreams: &[Arc<dyn Upstream>], request: &Message) -> Result<Message> {
        match upstreams {
            [] => Err(DnsError::plugin(format!(
                "dhcp_dns_servers plugin '{}': no usable DNS server on the default network",
                self.tag
            ))),
            [one] => one.query(request.clone()).await,
            upstreams => {
                let count = resolve_active_concurrent(
                    self.concurrent.or(Some(MAX_CONCURRENT_QUERIES)),
                    upstreams.len(),
                );
                let mut queries = JoinSet::new();
                for upstream in upstreams.iter().take(count) {
                    let upstream = upstream.clone();
                    let message = request.clone();
                    queries.spawn(async move { upstream.query(message).await });
                }
                let question = match self.response_selection {
                    ResponseSelectionMode::Fastest => None,
                    _ => request.first_question(),
                };
                let (selected, error, _) =
                    select_response(&mut queries, count, question, self.response_selection).await;
                let selected = selected.ok_or_else(|| {
                    DnsError::plugin(format!(
                        "dhcp_dns_servers plugin '{}': {}",
                        self.tag,
                        error.as_deref().unwrap_or("no upstream response")
                    ))
                })?;
                if selected.disposition == Some(ResponseDisposition::IncompleteAlias) {
                    self.metrics.record_incomplete_alias_selected();
                }
                Ok(selected.message)
            }
        }
    }
}
