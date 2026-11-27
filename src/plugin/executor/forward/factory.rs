// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use tracing::info;

use super::concurrent::ConcurrentForwarder;
use super::config::{
    MAX_CONCURRENT_QUERIES, make_default_upstream_config, parse_forward_config,
    parse_quick_setup_param, resolve_active_concurrent, validate_upstream_addr,
};
use super::metrics::{ForwardMetrics, upstream_metric_names};
use super::selection::ResponseSelectionMode;
use super::single::SingleDnsForwarder;
use crate::config::types::PluginConfig;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::upstream::{ConnectionInfo, Upstream, UpstreamBuilder, UpstreamConfig};
use crate::plugin::{PluginFactory, UninitializedPlugin};
use crate::plugin_factory;

/// Factory for creating DNS forwarder plugins
#[derive(Debug)]
#[plugin_factory("forward")]
pub struct ForwardFactory;

impl PluginFactory for ForwardFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<UninitializedPlugin> {
        let forward_config = parse_forward_config(plugin_config)?;
        let short_circuit = forward_config.short_circuit;
        let response_selection = forward_config.response_selection;

        if forward_config.upstreams.len() == 1 {
            // Single upstream configuration
            let upstream_config = &forward_config.upstreams[0];
            info!(
                "Creating single DNS forwarder (tag: {}) with upstream: {}",
                plugin_config.tag, upstream_config.addr
            );

            let upstream = build_upstream(upstream_config.clone())?;
            let names = upstream_metric_names(&[upstream.connection_info()]);

            Ok(UninitializedPlugin::Executor(Box::new(
                SingleDnsForwarder {
                    tag: plugin_config.tag.clone(),
                    upstream,
                    short_circuit,
                    metrics: Arc::new(ForwardMetrics::new(plugin_config.tag.clone(), names)),
                },
            )))
        } else {
            let active_concurrent = resolve_active_concurrent(
                forward_config.concurrent,
                forward_config.upstreams.len(),
            );

            let mut upstreams = Vec::with_capacity(forward_config.upstreams.len());

            for upstream_config in forward_config.upstreams {
                upstreams.push(build_upstream(upstream_config)?.into());
            }

            let infos: Vec<&ConnectionInfo> = upstreams
                .iter()
                .map(|u: &Arc<dyn Upstream>| u.connection_info())
                .collect();
            let names = upstream_metric_names(&infos);

            // Multi-upstream concurrent configuration
            Ok(UninitializedPlugin::Executor(Box::new(
                ConcurrentForwarder {
                    tag: plugin_config.tag.clone(),
                    active_concurrent,
                    upstreams,
                    short_circuit,
                    response_selection,
                    metrics: Arc::new(ForwardMetrics::new(plugin_config.tag.clone(), names)),
                },
            )))
        }
    }

    fn quick_setup(&self, tag: &str, param: Option<String>) -> Result<UninitializedPlugin> {
        let (upstream_addrs, short_circuit) = parse_quick_setup_param(param)?;
        let mut upstream_configs = Vec::with_capacity(upstream_addrs.len());

        for (idx, upstream_addr) in upstream_addrs.into_iter().enumerate() {
            validate_upstream_addr(&upstream_addr).map_err(|e| {
                DnsError::plugin(format!(
                    "forward quick setup upstream[{}] '{}' is invalid: {}",
                    idx, upstream_addr, e
                ))
            })?;
            upstream_configs.push(make_default_upstream_config(upstream_addr));
        }

        if upstream_configs.len() == 1 {
            let upstream_config = upstream_configs.pop().unwrap();
            let upstream = build_upstream(upstream_config)?;
            let names = upstream_metric_names(&[upstream.connection_info()]);
            Ok(UninitializedPlugin::Executor(Box::new(
                SingleDnsForwarder {
                    tag: tag.to_string(),
                    upstream,
                    short_circuit,
                    metrics: Arc::new(ForwardMetrics::new(tag.to_string(), names)),
                },
            )))
        } else {
            let mut upstreams = Vec::with_capacity(upstream_configs.len());
            for upstream_config in upstream_configs {
                upstreams.push(build_upstream(upstream_config)?.into());
            }
            let infos: Vec<&ConnectionInfo> = upstreams
                .iter()
                .map(|u: &Arc<dyn Upstream>| u.connection_info())
                .collect();
            let names = upstream_metric_names(&infos);
            Ok(UninitializedPlugin::Executor(Box::new(
                ConcurrentForwarder {
                    tag: tag.to_string(),
                    active_concurrent: resolve_active_concurrent(
                        Some(MAX_CONCURRENT_QUERIES),
                        upstreams.len(),
                    ),
                    upstreams,
                    short_circuit,
                    response_selection: ResponseSelectionMode::default(),
                    metrics: Arc::new(ForwardMetrics::new(tag.to_string(), names)),
                },
            )))
        }
    }
}

fn build_upstream(upstream_config: UpstreamConfig) -> Result<Box<dyn Upstream>> {
    UpstreamBuilder::with_upstream_config(upstream_config)
}
