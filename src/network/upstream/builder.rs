// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use tracing::debug;

use crate::core::error::Result;
use crate::network::upstream::config::{ConnectionInfo, ConnectionType, UpstreamConfig};
#[cfg(feature = "upstream-doh")]
use crate::network::upstream::conn::{H2Connection, H2ConnectionBuilder};
#[cfg(feature = "upstream-doh3")]
use crate::network::upstream::conn::{H3Connection, H3ConnectionBuilder};
#[cfg(feature = "upstream-doq")]
use crate::network::upstream::conn::{QuicConnection, QuicConnectionBuilder};
use crate::network::upstream::conn::{
    TcpConnection, TcpConnectionBuilder, UdpConnection, UdpConnectionBuilder,
};
use crate::network::upstream::pool::pool_pipeline::PipelinePool;
use crate::network::upstream::pool::pool_reuse::ReusePool;
use crate::network::upstream::pool::{Connection, ConnectionBuilder, QueryTimeoutPolicy};
use crate::network::upstream::resolver::{
    BootstrapUpstream, PooledUpstream, UdpTruncatedUpstream, Upstream,
};

/// Builder for creating upstream instances
pub struct UpstreamBuilder;

impl UpstreamBuilder {
    pub fn with_connection_info(connection_info: ConnectionInfo) -> Result<Box<dyn Upstream>> {
        debug!(
            "Creating upstream: type={:?}, remote={:?}, port={}",
            connection_info.connection_type, connection_info.remote_ip, connection_info.port
        );

        if connection_info.bootstrap.is_none() {
            let upstream: Box<dyn Upstream> = match connection_info.connection_type {
                ConnectionType::UDP => {
                    debug!("Creating UDP upstream for {}", connection_info.raw_addr);
                    let builder = UdpConnectionBuilder::new(
                        &connection_info,
                        pipeline_request_map_capacity(),
                    );
                    let main_pool = PipelinePool::new(
                        main_pool_min_conns(&connection_info),
                        connection_info.max_conns_or_default(),
                        ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
                        connection_info.idle_timeout,
                        Box::new(builder),
                        QueryTimeoutPolicy::Reuse,
                        connection_info.timeout,
                    );

                    let tcp_builder =
                        TcpConnectionBuilder::new(&connection_info, reuse_request_map_capacity());
                    let fallback_pool = ReusePool::new(
                        udp_truncated_fallback_min_conns(),
                        connection_info.max_conns_or_default(),
                        connection_info.idle_timeout,
                        Box::new(tcp_builder),
                        QueryTimeoutPolicy::Close,
                        connection_info.timeout,
                    );

                    Box::new(UdpTruncatedUpstream {
                        connection_info,
                        main_pool,
                        fallback_pool,
                    })
                }
                ConnectionType::TCP => {
                    debug!("Creating TCP upstream for {}", connection_info.raw_addr);
                    if connection_info.enable_pipeline.unwrap_or(false) {
                        let builder = TcpConnectionBuilder::new(
                            &connection_info,
                            pipeline_request_map_capacity(),
                        );
                        Box::new(create_pipeline_pool(connection_info, Box::new(builder)))
                    } else {
                        let builder = TcpConnectionBuilder::new(
                            &connection_info,
                            reuse_request_map_capacity(),
                        );
                        Box::new(create_reuse_pool(connection_info, Box::new(builder)))
                    }
                }
                #[cfg(feature = "upstream-dot")]
                ConnectionType::DoT => {
                    debug!("Creating DoT upstream for {}", connection_info.raw_addr);
                    if connection_info.enable_pipeline.unwrap_or(false) {
                        let builder = TcpConnectionBuilder::new(
                            &connection_info,
                            pipeline_request_map_capacity(),
                        );
                        Box::new(create_pipeline_pool(connection_info, Box::new(builder)))
                    } else {
                        let builder = TcpConnectionBuilder::new(
                            &connection_info,
                            reuse_request_map_capacity(),
                        );
                        Box::new(create_reuse_pool(connection_info, Box::new(builder)))
                    }
                }
                #[cfg(not(feature = "upstream-dot"))]
                ConnectionType::DoT => {
                    return Err(crate::core::error::DnsError::plugin(
                        "upstream DoT is not compiled into this build; \
                         rebuild with --features upstream-dot",
                    ));
                }
                #[cfg(feature = "upstream-doq")]
                ConnectionType::DoQ => {
                    debug!("Creating QUIC upstream for {}", connection_info.raw_addr);
                    let builder = QuicConnectionBuilder::new(&connection_info);
                    Box::new(create_pipeline_pool(connection_info, Box::new(builder)))
                }
                #[cfg(not(feature = "upstream-doq"))]
                ConnectionType::DoQ => {
                    return Err(crate::core::error::DnsError::plugin(
                        "upstream DoQ is not compiled into this build; \
                         rebuild with --features upstream-doq",
                    ));
                }
                #[cfg(feature = "upstream-doh")]
                ConnectionType::DoH => {
                    debug!(
                        "Creating DoH upstream for {} (HTTP/{})",
                        connection_info.raw_addr,
                        if connection_info.enable_http3 {
                            "3"
                        } else {
                            "2"
                        }
                    );
                    if connection_info.enable_http3 {
                        #[cfg(feature = "upstream-doh3")]
                        {
                            let builder = H3ConnectionBuilder::new(&connection_info);
                            Box::new(create_pipeline_pool(connection_info, Box::new(builder)))
                        }
                        #[cfg(not(feature = "upstream-doh3"))]
                        {
                            return Err(crate::core::error::DnsError::plugin(
                                "upstream DoH3 (HTTP/3) is not compiled into this build; \
                                 rebuild with --features upstream-doh3",
                            ));
                        }
                    } else {
                        let builder = H2ConnectionBuilder::new(&connection_info);
                        Box::new(create_pipeline_pool(connection_info, Box::new(builder)))
                    }
                }
                #[cfg(not(feature = "upstream-doh"))]
                ConnectionType::DoH => {
                    return Err(crate::core::error::DnsError::plugin(
                        "upstream DoH is not compiled into this build; \
                         rebuild with --features upstream-doh",
                    ));
                }
            };
            Ok(upstream)
        } else {
            // Domain-based upstream: use bootstrap or system DNS for resolution
            let upstream: Box<dyn Upstream> = match &connection_info.connection_type {
                ConnectionType::UDP => {
                    let upstream: BootstrapUpstream<UdpConnection> =
                        BootstrapUpstream::new(connection_info);
                    Box::new(upstream)
                }
                ConnectionType::TCP => {
                    let upstream: BootstrapUpstream<TcpConnection> =
                        BootstrapUpstream::new(connection_info);
                    Box::new(upstream)
                }
                #[cfg(feature = "upstream-dot")]
                ConnectionType::DoT => {
                    let upstream: BootstrapUpstream<TcpConnection> =
                        BootstrapUpstream::new(connection_info);
                    Box::new(upstream)
                }
                #[cfg(not(feature = "upstream-dot"))]
                ConnectionType::DoT => {
                    return Err(DnsError::plugin(
                        "upstream DoT is not compiled into this build; \
                         rebuild with --features upstream-dot",
                    ));
                }
                #[cfg(feature = "upstream-doq")]
                ConnectionType::DoQ => {
                    let upstream: BootstrapUpstream<QuicConnection> =
                        BootstrapUpstream::new(connection_info);
                    Box::new(upstream)
                }
                #[cfg(not(feature = "upstream-doq"))]
                ConnectionType::DoQ => {
                    return Err(DnsError::plugin(
                        "upstream DoQ is not compiled into this build; \
                         rebuild with --features upstream-doq",
                    ));
                }
                #[cfg(feature = "upstream-doh")]
                ConnectionType::DoH => {
                    if connection_info.enable_http3 {
                        #[cfg(feature = "upstream-doh3")]
                        {
                            let upstream: BootstrapUpstream<H3Connection> =
                                BootstrapUpstream::new(connection_info);
                            Box::new(upstream)
                        }
                        #[cfg(not(feature = "upstream-doh3"))]
                        {
                            return Err(DnsError::plugin(
                                "upstream DoH3 (HTTP/3) is not compiled into this build; \
                                 rebuild with --features upstream-doh3",
                            ));
                        }
                    } else {
                        let upstream: BootstrapUpstream<H2Connection> =
                            BootstrapUpstream::new(connection_info);
                        Box::new(upstream)
                    }
                }
                #[cfg(not(feature = "upstream-doh"))]
                ConnectionType::DoH => {
                    return Err(DnsError::plugin(
                        "upstream DoH is not compiled into this build; \
                         rebuild with --features upstream-doh",
                    ));
                }
            };
            Ok(upstream)
        }
    }

    /// Build an upstream instance from configuration
    pub fn with_upstream_config(upstream_config: UpstreamConfig) -> Result<Box<dyn Upstream>> {
        let connection_info = ConnectionInfo::try_from(upstream_config)?;
        debug!("create upstream, connection info: {:?}", connection_info);
        Self::with_connection_info(connection_info)
    }
}

#[inline]
pub(crate) const fn pipeline_request_map_capacity() -> u16 {
    ConnectionInfo::DEFAULT_MAX_CONNS_LOAD
}

#[inline]
pub(crate) const fn reuse_request_map_capacity() -> u16 {
    1
}

#[inline]
pub(crate) fn main_pool_min_conns(connection_info: &ConnectionInfo) -> usize {
    connection_info.min_conns_or_default()
}

#[inline]
pub(crate) const fn udp_truncated_fallback_min_conns() -> usize {
    0
}

pub(crate) fn create_pipeline_pool<C: Connection>(
    connection_info: ConnectionInfo,
    builder: Box<dyn ConnectionBuilder<C>>,
) -> PooledUpstream<C> {
    let timeout = connection_info.timeout;
    let min_size = main_pool_min_conns(&connection_info);
    PooledUpstream::<C> {
        pool: PipelinePool::new(
            min_size,
            connection_info.max_conns_or_default(),
            ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
            connection_info.idle_timeout,
            builder,
            QueryTimeoutPolicy::Retire,
            timeout,
        ),
        connection_info,
    }
}

pub(crate) fn create_reuse_pool<C: Connection>(
    connection_info: ConnectionInfo,
    builder: Box<dyn ConnectionBuilder<C>>,
) -> PooledUpstream<C> {
    let timeout = connection_info.timeout;
    let min_size = main_pool_min_conns(&connection_info);
    PooledUpstream::<C> {
        pool: ReusePool::new(
            min_size,
            connection_info.max_conns_or_default(),
            connection_info.idle_timeout,
            builder,
            QueryTimeoutPolicy::Close,
            timeout,
        ),
        connection_info,
    }
}
