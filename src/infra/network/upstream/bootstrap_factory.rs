// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::Debug;
use std::net::IpAddr;
use std::sync::Arc;

use crate::infra::network::upstream::builder::{
    main_pool_min_conns, pipeline_request_map_capacity, reuse_request_map_capacity,
};
use crate::infra::network::upstream::config::ConnectionInfo;
#[cfg(feature = "upstream-doh")]
use crate::infra::network::upstream::conn::{H2Connection, H2ConnectionBuilder};
#[cfg(feature = "upstream-doh3")]
use crate::infra::network::upstream::conn::{H3Connection, H3ConnectionBuilder};
#[cfg(feature = "upstream-doq")]
use crate::infra::network::upstream::conn::{QuicConnection, QuicConnectionBuilder};
use crate::infra::network::upstream::conn::{
    TcpConnection, TcpConnectionBuilder, UdpConnection, UdpConnectionBuilder,
};
use crate::infra::network::upstream::pool::pipeline::PipelinePool;
use crate::infra::network::upstream::pool::reuse::ReusePool;
use crate::infra::network::upstream::pool::{Connection, ConnectionPool, QueryTimeoutPolicy};

pub(super) trait BootstrapPoolFactory<C: Connection>: Debug + Send + Sync {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<C>>;
}

#[derive(Debug)]
pub(super) struct UdpBootstrapPoolFactory;

impl BootstrapPoolFactory<UdpConnection> for UdpBootstrapPoolFactory {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<UdpConnection>> {
        let info = connection_info_with_ip(connection_info, ip);
        let builder = UdpConnectionBuilder::new(&info, pipeline_request_map_capacity());
        PipelinePool::new(
            main_pool_min_conns(&info),
            info.max_conns_or_default(),
            ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
            info.idle_timeout,
            Box::new(builder),
            QueryTimeoutPolicy::Reuse,
            info.timeout,
        )
    }
}

#[derive(Debug)]
pub(super) struct TcpBootstrapPoolFactory;

impl BootstrapPoolFactory<TcpConnection> for TcpBootstrapPoolFactory {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<TcpConnection>> {
        let info = connection_info_with_ip(connection_info, ip);
        if info.enable_pipeline.unwrap_or(false) {
            let builder = TcpConnectionBuilder::new(&info, pipeline_request_map_capacity());
            PipelinePool::new(
                main_pool_min_conns(&info),
                info.max_conns_or_default(),
                ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
                info.idle_timeout,
                Box::new(builder),
                QueryTimeoutPolicy::Retire,
                info.timeout,
            )
        } else {
            let builder = TcpConnectionBuilder::new(&info, reuse_request_map_capacity());
            ReusePool::new(
                main_pool_min_conns(&info),
                info.max_conns_or_default(),
                info.idle_timeout,
                Box::new(builder),
                QueryTimeoutPolicy::Close,
                info.timeout,
            )
        }
    }
}

#[cfg(feature = "upstream-doq")]
#[derive(Debug)]
pub(super) struct QuicBootstrapPoolFactory;

#[cfg(feature = "upstream-doq")]
impl BootstrapPoolFactory<QuicConnection> for QuicBootstrapPoolFactory {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<QuicConnection>> {
        let info = connection_info_with_ip(connection_info, ip);
        let builder = QuicConnectionBuilder::new(&info);
        PipelinePool::new(
            main_pool_min_conns(&info),
            info.max_conns_or_default(),
            ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
            info.idle_timeout,
            Box::new(builder),
            QueryTimeoutPolicy::Retire,
            info.timeout,
        )
    }
}

#[cfg(feature = "upstream-doh")]
#[derive(Debug)]
pub(super) struct H2BootstrapPoolFactory;

#[cfg(feature = "upstream-doh")]
impl BootstrapPoolFactory<H2Connection> for H2BootstrapPoolFactory {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<H2Connection>> {
        let info = connection_info_with_ip(connection_info, ip);
        let builder = H2ConnectionBuilder::new(&info);
        PipelinePool::new(
            main_pool_min_conns(&info),
            info.max_conns_or_default(),
            ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
            info.idle_timeout,
            Box::new(builder),
            QueryTimeoutPolicy::Retire,
            info.timeout,
        )
    }
}

#[cfg(feature = "upstream-doh3")]
#[derive(Debug)]
pub(super) struct H3BootstrapPoolFactory;

#[cfg(feature = "upstream-doh3")]
impl BootstrapPoolFactory<H3Connection> for H3BootstrapPoolFactory {
    fn create_pool(
        &self,
        connection_info: &ConnectionInfo,
        ip: IpAddr,
    ) -> Arc<dyn ConnectionPool<H3Connection>> {
        let info = connection_info_with_ip(connection_info, ip);
        let builder = H3ConnectionBuilder::new(&info);
        PipelinePool::new(
            main_pool_min_conns(&info),
            info.max_conns_or_default(),
            ConnectionInfo::DEFAULT_MAX_CONNS_LOAD,
            info.idle_timeout,
            Box::new(builder),
            QueryTimeoutPolicy::Retire,
            info.timeout,
        )
    }
}

fn connection_info_with_ip(connection_info: &ConnectionInfo, ip: IpAddr) -> ConnectionInfo {
    let mut info = connection_info.clone();
    info.remote_ip = Some(ip);
    info
}
