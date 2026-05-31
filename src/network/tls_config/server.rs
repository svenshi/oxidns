// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Server-side TLS configuration for DoT / DoH / DoQ servers and the
//! management API.
//!
//! Loads PEM-encoded certificate chains and private keys from disk and builds
//! rustls [`ServerConfig`]s, optionally with client-certificate verification.

use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use rustls::pki_types::CertificateDer;
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use tracing::info;

use super::install_default_provider;
use crate::core::error::DnsError;

/// Load TLS certificates and private key from files
///
/// Reads PEM-encoded certificate chain and private key from the specified
/// files.
///
/// # Arguments
/// * `cert_path` - Path to the certificate file (PEM format)
/// * `key_path` - Path to the private key file (PEM format)
///
/// # Returns
/// * `Ok(TlsAcceptor)` - Configured TLS acceptor
/// * `Err(DnsError)` - Error if files cannot be read or parsed
pub fn load_tls_config(
    cert: &Option<String>,
    key: &Option<String>,
) -> Option<crate::core::error::Result<ServerConfig>> {
    match (cert, key) {
        (Some(cert), Some(key)) => {
            info!("Loading TLS configuration: cert={}, key={}", cert, key);
            Some(load_tls_config_from_path(cert, key))
        }
        (Some(_), None) => Some(Err(DnsError::plugin(" cert specified but key is missing"))),
        (None, Some(_)) => Some(Err(DnsError::plugin("key specified but cert is missing"))),
        (None, None) => None,
    }
}

/// Load server-side TLS configuration with optional client certificate
/// verification.
pub fn load_server_tls_config(
    cert: Option<&str>,
    key: Option<&str>,
    client_ca: Option<&str>,
    require_client_cert: bool,
) -> crate::core::error::Result<Option<ServerConfig>> {
    match (cert, key) {
        (Some(cert), Some(key)) => {
            let certs = load_certificates(cert)?;
            let private_key = load_private_key(key)?;
            let builder = ServerConfig::builder();
            let config = if require_client_cert {
                let ca_path = client_ca.ok_or_else(|| {
                    DnsError::plugin(
                        "api.http.ssl.require_client_cert requires api.http.ssl.client_ca",
                    )
                })?;
                let roots = Arc::new(load_root_store(ca_path)?);
                let verifier = WebPkiClientVerifier::builder(roots).build().map_err(|e| {
                    DnsError::plugin(format!(
                        "Failed to build client certificate verifier: {}",
                        e
                    ))
                })?;
                builder
                    .with_client_cert_verifier(verifier)
                    .with_single_cert(certs, private_key)
                    .map_err(|e| {
                        DnsError::plugin(format!("Failed to build TLS configuration: {}", e))
                    })?
            } else {
                builder
                    .with_no_client_auth()
                    .with_single_cert(certs, private_key)
                    .map_err(|e| {
                        DnsError::plugin(format!("Failed to build TLS configuration: {}", e))
                    })?
            };
            Ok(Some(config))
        }
        (Some(_), None) | (None, Some(_)) => Err(DnsError::plugin(
            "api.http.ssl.cert and api.http.ssl.key must be configured together",
        )),
        (None, None) => Ok(None),
    }
}

fn load_tls_config_from_path(
    cert_path: &str,
    key_path: &str,
) -> crate::core::error::Result<ServerConfig> {
    install_default_provider();
    let certs = load_certificates(cert_path)?;
    let private_key = load_private_key(key_path)?;

    // Build TLS server configuration
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, private_key)
        .map_err(|e| DnsError::plugin(format!("Failed to build TLS configuration: {}", e)))?;
    Ok(config)
}

fn load_certificates(cert_path: &str) -> crate::core::error::Result<Vec<CertificateDer<'static>>> {
    let cert_file = File::open(cert_path).map_err(|e| {
        DnsError::plugin(format!(
            "Failed to open certificate file {}: {}",
            cert_path, e
        ))
    })?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            DnsError::plugin(format!(
                "Failed to parse certificate file {}: {}",
                cert_path, e
            ))
        })?;

    if certs.is_empty() {
        return Err(DnsError::plugin(format!(
            "No certificates found in {}",
            cert_path
        )));
    }
    Ok(certs)
}

fn load_private_key(
    key_path: &str,
) -> crate::core::error::Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let key_file = File::open(key_path).map_err(|e| {
        DnsError::plugin(format!(
            "Failed to open private key file {}: {}",
            key_path, e
        ))
    })?;
    let mut key_reader = BufReader::new(key_file);
    rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| {
            DnsError::plugin(format!(
                "Failed to parse private key file {}: {}",
                key_path, e
            ))
        })?
        .ok_or_else(|| DnsError::plugin(format!("No private key found in {}", key_path)))
}

fn load_root_store(ca_path: &str) -> crate::core::error::Result<RootCertStore> {
    let certs = load_certificates(ca_path)?;
    let mut roots = RootCertStore::empty();
    let (added, ignored) = roots.add_parsable_certificates(certs);
    if added == 0 {
        return Err(DnsError::plugin(format!(
            "No CA certificates could be loaded from {} (ignored {})",
            ca_path, ignored
        )));
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_tls_config_option_validation() {
        assert!(load_tls_config(&None, &None).is_none());
        assert!(
            load_tls_config(&Some("cert.pem".into()), &None)
                .expect("should return error")
                .is_err()
        );
        assert!(
            load_tls_config(&None, &Some("key.pem".into()))
                .expect("should return error")
                .is_err()
        );
    }
}
