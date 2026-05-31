// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared QUIC endpoint construction.
//!
//! Both the DoQ server (`server-doq`) and the HTTP/3 leg of the DoH server
//! (`server-doh3`) need to bind a QUIC endpoint from a rustls `ServerConfig`.
//! Keeping the builder here — instead of inside the DoQ-only `quic` module —
//! lets a DoH3-only build (`--features server-doh3` without `server-doq`)
//! compile without pulling in the full DoQ server plugin.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use quinn::{Endpoint, EndpointConfig, IdleTimeout, TransportConfig};
use rustls::ServerConfig;

use crate::core::error::Result;
use crate::plugin::server::udp;

/// Bind a QUIC [`Endpoint`] on `addr` using the provided rustls server config.
///
/// The endpoint reuses OxiDNS's tuned UDP socket builder and applies the
/// configured idle timeout to the QUIC transport.
pub fn build_quic_endpoint(
    addr: SocketAddr,
    server_config: ServerConfig,
    timeout: Duration,
) -> Result<Endpoint> {
    let socket = udp::build_udp_socket(addr)?;

    let quic_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(Arc::new(server_config))?;
    let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_crypto));

    let mut config = TransportConfig::default();
    let timeout = IdleTimeout::try_from(timeout)?;
    config.max_idle_timeout(Some(timeout));
    server_config.transport = Arc::new(config);

    Ok(Endpoint::new(
        EndpointConfig::default(),
        Some(server_config),
        socket,
        Arc::new(quinn::TokioRuntime),
    )?)
}
