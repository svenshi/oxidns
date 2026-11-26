// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use async_trait::async_trait;
use tokio::task::JoinSet;
use tokio_util::task::AbortOnDropHandle;
use tracing::{debug, warn};

use super::config::{MAX_CONCURRENT_QUERIES, resolve_active_concurrent};
use super::is_timeout_error;
use super::metrics::ForwardMetrics;
use super::selection::{ResponseSelectionMode, select_response};
use crate::core::context::DnsContext;
use crate::core::response::ResponseDisposition;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::default_resolver::{Discovery, NetworkIdentity, discover};
use crate::infra::network::upstream::{ConnectionInfo, Upstream, UpstreamBuilder};
use crate::infra::observability::metrics::{register_metric_source, unregister_metric_source};
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::{Plugin, PluginInitContext};
use crate::proto::Message;

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
struct ResolverSet {
    network: NetworkIdentity,
    upstreams: Vec<Arc<dyn Upstream>>,
}

#[derive(Debug)]
pub(super) struct DefaultResolverForwarder {
    tag: String,
    concurrent: Option<usize>,
    response_selection: ResponseSelectionMode,
    short_circuit: bool,
    snapshot: Arc<ArcSwap<ResolverSet>>,
    refresh: Option<AbortOnDropHandle<()>>,
    metrics: Arc<ForwardMetrics>,
}

impl DefaultResolverForwarder {
    pub(super) fn new(
        tag: String,
        concurrent: Option<usize>,
        response_selection: ResponseSelectionMode,
        short_circuit: bool,
    ) -> Self {
        Self {
            metrics: Arc::new(ForwardMetrics::new(tag.clone(), Vec::new())),
            tag,
            concurrent,
            response_selection,
            short_circuit,
            snapshot: Arc::new(ArcSwap::from_pointee(ResolverSet::default())),
            refresh: None,
        }
    }
}

async fn refresh_snapshot(snapshot: &ArcSwap<ResolverSet>) {
    let observation = discover().await;
    let previous = snapshot.load();
    let Discovery {
        network,
        mut servers,
    } = match observation {
        Ok(discovery) => discovery,
        Err(error) => {
            if !previous.upstreams.is_empty() {
                warn!(%error, "Clearing previously discovered default DNS servers");
            } else {
                debug!(%error, "Default DNS discovery failed");
            }
            Discovery::default()
        }
    };
    servers.truncate(MAX_CONCURRENT_QUERIES);

    let peer_ip = |upstream: &Arc<dyn Upstream>| upstream.connection_info().remote_ip;
    let unchanged_network = previous.network == network;
    if unchanged_network
        && previous
            .upstreams
            .iter()
            .map(peer_ip)
            .eq(servers.iter().map(|ip| Some(IpAddr::V4(*ip))))
    {
        return;
    }

    let reusable = if unchanged_network {
        previous.upstreams.as_slice()
    } else {
        &[]
    };
    let mut upstreams = Vec::with_capacity(servers.len());
    for ip in servers {
        if let Some(upstream) = reusable
            .iter()
            .find(|upstream| peer_ip(upstream) == Some(ip.into()))
        {
            upstreams.push(upstream.clone());
        } else {
            // Use direct numeric endpoints, bypassing system DNS and outbound
            // profiles.
            match ConnectionInfo::with_addr(&ip.to_string())
                .and_then(UpstreamBuilder::with_connection_info)
            {
                Ok(upstream) => upstreams.push(Arc::from(upstream)),
                Err(error) => warn!(%error, "Could not build discovered DNS upstream"),
            }
        }
    }
    snapshot.store(Arc::new(ResolverSet { network, upstreams }));
}

#[async_trait]
impl Plugin for DefaultResolverForwarder {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &PluginInitContext<'_>) -> Result<()> {
        // Finish the first probe before publishing this executor to listeners.
        refresh_snapshot(&self.snapshot).await;
        register_metric_source(self.metrics.clone())?;
        let snapshot = self.snapshot.clone();
        self.refresh = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            loop {
                tokio::time::sleep(REFRESH_INTERVAL).await;
                refresh_snapshot(&snapshot).await;
            }
        })));
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        if let Some(refresh) = &self.refresh {
            refresh.abort();
        }
        unregister_metric_source(&self.tag);
        Ok(())
    }
}

#[async_trait]
impl Executor for DefaultResolverForwarder {
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        let start = self.metrics.record_query_start();
        let snapshot = self.snapshot.load_full();
        let result = self.query(&snapshot.upstreams, &context.request).await;
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

impl DefaultResolverForwarder {
    async fn query(&self, upstreams: &[Arc<dyn Upstream>], request: &Message) -> Result<Message> {
        match upstreams {
            [] => Err(DnsError::plugin(format!(
                "forward plugin '{}': no usable DNS server on the default network",
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
                        "forward plugin '{}': {}",
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
