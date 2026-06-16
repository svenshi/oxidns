// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! TLS configuration for secure DNS protocols.
//!
//! Split into two halves so a build that only needs one side does not drag in
//! the other's dependencies:
//!
//! - `client` (`_tls-client`): pre-built client `ClientConfig`s for DoT / DoH /
//!   DoQ upstreams, validated against the bundled `webpki-roots`.
//! - `server` (`_tls-server`): certificate / key loading for DoT / DoH / DoQ
//!   servers and the management API.
//!
//! The default crypto provider installation is shared by both halves and lives
//! here.

use std::sync::Once;

use rustls::crypto::ring;

#[cfg(feature = "_tls-client")]
mod client;
#[cfg(feature = "_tls-server")]
mod server;

#[cfg(feature = "_tls-client")]
pub(crate) use client::{insecure_client_config, secure_client_config};
#[cfg(feature = "_tls-server")]
pub use server::{load_server_tls_config, load_tls_config};

static DEFAULT_PROVIDER: Once = Once::new();

/// Install the ring crypto provider as the process-wide rustls default.
///
/// Idempotent: only the first call takes effect. Shared by both the client and
/// server TLS paths and by the QUIC endpoints.
pub fn install_default_provider() {
    DEFAULT_PROVIDER.call_once(|| {
        ring::default_provider()
            .install_default()
            .expect("default provider already set elsewhere");
    })
}
