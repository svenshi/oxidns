// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! TCP DNS server plugin
//!
//! Listens for DNS queries over TCP (with optional TLS support) and processes
//! them through a configured entry plugin executor. Handles concurrent requests
//! concurrently. Connections own their I/O; the server tracks accepted requests
//! independently so disconnects do not cancel executor side effects. Shutdown
//! closes connections before draining requests and releasing their
//! dependencies.
//!
//! ## TLS Support
//!
//! The server supports optional TLS encryption. To enable TLS, provide both
//! `cert` and `key` configuration options pointing to PEM-encoded certificate
//! and private key files.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use socket2::{Socket, TcpKeepalive};
use tokio::io::AsyncWrite;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};
#[cfg(feature = "server-dot")]
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, info, warn};

use crate::config::types::PluginConfig;
use crate::infra::error::{DnsError, Result};
use crate::infra::network::listen::{self, parse_listen_addr};
#[cfg(feature = "server-dot")]
use crate::infra::network::tls_config::load_tls_config;
use crate::infra::network::transport::tcp::{TcpTransport, TcpTransportWriter};
use crate::infra::observability::metrics::{register_metric_source, unregister_metric_source};
use crate::infra::system::deserialize_duration_option;
use crate::plugin::dependency::DependencySpec;
use crate::plugin::server::{
    ConnectionGuard, DEFAULT_SERVER_IDLE_TIMEOUT, RequestHandle, RequestMeta, Server, ServerMetrics,
};
use crate::plugin::{Plugin, PluginFactory};
use crate::plugin_factory;
use crate::proto::Message;

const TCP_SOCKET_BUFFER_SIZE: usize = 64 * 1024;

/// TCP server configuration
#[derive(Deserialize)]
pub struct TcpServerConfig {
    /// Entry executor plugin tag to process incoming requests.
    ///
    /// - Must reference an existing executor plugin registered in
    ///   `PluginRegistry`.
    /// - All TCP/TLS DNS queries will be forwarded to this executor.
    entry: String,

    /// TCP listen address in `ip:port` or `:port` format.
    ///
    /// - Example: "0.0.0.0:53" (DNS over TCP), ":853" (DNS over TLS/DoT)
    /// - `:port` binds on `[::]:port` with dual-stack sockets enabled.
    /// - Must be a valid listen address or validation will fail.
    listen: String,

    /// Path to TLS certificate file (PEM format, optional).
    ///
    /// - When both `cert` and `key` are provided, TLS will be enabled (DoT on
    ///   port 853).
    /// - When either is missing, server runs in plain TCP mode.
    /// - When the binary was built without `--features server-dot`, setting
    ///   either field is a hard error so users notice they need a TLS-capable
    ///   build.
    cert: Option<String>,

    /// Path to TLS private key file (PEM format, optional).
    ///
    /// - Supports common key formats (PKCS#8/RSA/EC) via `rustls-pemfile`.
    key: Option<String>,

    /// TCP connection idle timeout in seconds.
    ///
    /// - Default: 10 seconds if omitted.
    /// - Applied as TCP keepalive interval for long-lived connections.
    #[serde(default, deserialize_with = "deserialize_duration_option")]
    idle_timeout: Option<Duration>,
}

/// TCP DNS server plugin
#[allow(unused)]
pub struct TcpServer {
    tag: String,
    listen: SocketAddr,
    request_handle: Arc<RequestHandle>,
    metrics: Arc<ServerMetrics>,
    #[cfg(feature = "server-dot")]
    tls_acceptor: Option<Arc<TlsAcceptor>>,
    idle_timeout: Option<Duration>,
    shutdown_tx: watch::Sender<bool>,
    task_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl std::fmt::Debug for TcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("TcpServer");
        d.field("tag", &self.tag).field("listen", &self.listen);
        #[cfg(feature = "server-dot")]
        d.field("has_tls", &self.tls_acceptor.is_some());
        d.field("idle_timeout", &self.idle_timeout).finish()
    }
}

impl TcpServer {
    fn spawn_server_task(
        &self,
        startup_tx: Option<oneshot::Sender<std::result::Result<(), String>>>,
    ) -> Result<()> {
        let mut task_slot = self
            .task_handle
            .lock()
            .map_err(|_| DnsError::runtime("TCP server task lock poisoned"))?;

        if task_slot.is_some() {
            if let Some(startup_tx) = startup_tx {
                let _ = startup_tx.send(Ok(()));
            }
            return Ok(());
        }

        let addr = self.listen;
        let handler = self.request_handle.clone();
        #[cfg(feature = "server-dot")]
        let tls_acceptor = self.tls_acceptor.clone();
        let idle_timeout = self.idle_timeout.unwrap_or(DEFAULT_SERVER_IDLE_TIMEOUT);
        let shutdown_rx = self.shutdown_tx.subscribe();
        *task_slot = Some(tokio::spawn(run_server(
            addr,
            handler,
            #[cfg(feature = "server-dot")]
            tls_acceptor,
            idle_timeout,
            shutdown_rx,
            startup_tx,
        )));
        Ok(())
    }
}

#[async_trait]
impl Plugin for TcpServer {
    fn tag(&self) -> &str {
        self.tag.as_str()
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        register_metric_source(self.metrics.clone())?;
        let (startup_tx, startup_rx) = oneshot::channel();
        self.spawn_server_task(Some(startup_tx))?;
        match startup_rx.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(DnsError::plugin(e)),
            Err(_) => Err(DnsError::plugin(
                "TCP server startup channel closed unexpectedly",
            )),
        }
    }

    async fn destroy(&self) -> Result<()> {
        unregister_metric_source(&self.tag);
        let _ = self.shutdown_tx.send(true);
        let handle = self
            .task_handle
            .lock()
            .map_err(|_| DnsError::runtime("TCP server task lock poisoned"))?
            .take();
        if let Some(handle) = handle {
            let _ = handle.await;
        }
        Ok(())
    }
}

impl Server for TcpServer {
    fn run(&self) {
        #[cfg(feature = "server-dot")]
        let tls_mode = self.tls_acceptor.is_some();
        #[cfg(not(feature = "server-dot"))]
        let tls_mode = false;

        debug!(listen = %self.listen, tls = tls_mode, "Spawning TCP server task");
        if let Err(e) = self.spawn_server_task(None) {
            error!(plugin = %self.tag, error = %e, "Failed to spawn TCP server task");
        }
    }
}

/// Main TCP server loop
///
/// Creates a TCP stream, listens for incoming DNS queries, and spawns
/// handler tasks for each request. Uses a task tracker and cancellation token
/// to manage active connections without polling completed tasks from the
/// accept loop.
#[hotpath::measure]
async fn run_server(
    addr: SocketAddr,
    handler: Arc<RequestHandle>,
    #[cfg(feature = "server-dot")] tls_acceptor: Option<Arc<TlsAcceptor>>,
    idle_timeout: Duration,
    mut shutdown_rx: watch::Receiver<bool>,
    startup_tx: Option<oneshot::Sender<std::result::Result<(), String>>>,
) {
    let mut startup_tx = startup_tx;
    let listener = match build_tcp_listener(addr, idle_timeout) {
        Ok(s) => s,
        Err(e) => {
            if let Some(tx) = startup_tx.take() {
                let _ = tx.send(Err(format!("Failed to bind TCP socket to {}: {}", addr, e)));
            }
            error!("Failed to bind TCP socket to {}: {}", addr, e);
            return;
        }
    };

    if let Some(tx) = startup_tx.take() {
        let _ = tx.send(Ok(()));
    }
    #[cfg(feature = "server-dot")]
    let tls_mode = tls_acceptor.is_some();
    #[cfg(not(feature = "server-dot"))]
    let tls_mode = false;
    info!(
        listen = %addr,
        idle_timeout_secs = idle_timeout.as_secs(),
        tls = %tls_mode,
        "TCP server bound successfully"
    );

    let connections = TaskTracker::new();
    let requests = TaskTracker::new();
    let shutdown_token = CancellationToken::new();
    let active_connections = Arc::new(AtomicU64::new(0));

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            // Accept new connections
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, src)) => {
                        let handler = handler.clone();
                        #[cfg(feature = "server-dot")]
                        let tls_acceptor = tls_acceptor.clone();
                        let task_shutdown = shutdown_token.clone();
                        let requests = requests.clone();
                        let active_connections = active_connections.clone();

                        let active = active_connections.fetch_add(1, Ordering::Relaxed) + 1;
                        debug!("New connection from {} (active: {})", src, active);
                        connections.spawn(async move {
                            let _connection_guard =
                                ConnectionGuard::new(active_connections.clone(), src, "TCP");
                            tokio::select! {
                                _ = task_shutdown.cancelled() => {}
                                _ = async move {
                                    #[cfg(feature = "server-dot")]
                                    {
                                        // Handle TLS handshake if TLS is enabled
                                        if let Some(acceptor) = tls_acceptor {
                                            match acceptor.accept(stream).await {
                                                Ok(tls_stream) => {
                                                    let server_name = tls_stream
                                                        .get_ref()
                                                        .1
                                                        .server_name()
                                                        .map(Arc::from);
                                                    debug!("TLS handshake completed for client {}", src);
                                                    handle_dns_stream(tls_stream, src, handler, server_name, &requests)
                                                        .await;
                                                }
                                                Err(e) => {
                                                    warn!("TLS handshake failed for {}: {}", src, e);
                                                }
                                            }
                                        } else {
                                            // Plain TCP connection
                                            debug!("TCP server connected to client {}", src);
                                            handle_dns_stream(stream, src, handler, None, &requests).await;
                                        }
                                    }
                                    #[cfg(not(feature = "server-dot"))]
                                    {
                                        // Plain TCP connection only (DoT requires --features server-dot).
                                        debug!("TCP server connected to client {}", src);
                                        handle_dns_stream(stream, src, handler, None, &requests).await;
                                    }
                                } => {}
                            }
                        });
                    }
                    Err(e) => {
                        debug!(%e, listen = %addr, "Error accepting TCP connection");
                    }
                }
            }
        }
    }

    drop(listener);
    shutdown_token.cancel();
    connections.close();
    connections.wait().await;
    // Connections can no longer dispatch work. Finish accepted requests before
    // the registry destroys their executor dependencies, as for UDP servers.
    requests.close();
    requests.wait().await;
    info!(listen = %addr, "TCP server stopped");
}

/// Handle DNS messages over a TCP stream (works for both TLS and plain TCP)
#[hotpath::measure]
async fn handle_dns_stream<S>(
    stream: S,
    src: SocketAddr,
    handler: Arc<RequestHandle>,
    server_name: Option<Arc<str>>,
    requests: &TaskTracker,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
{
    let transport = TcpTransport::new(stream);
    let (mut reader, writer) = transport.into_split();

    let (sender, receiver) = tokio::sync::mpsc::channel::<Message>(128);

    // The writer is owned by the connection future, so EOF, write failure,
    // and cancellation all drop the receiver and both stream halves together.
    let write = write_tcp_responses(writer, receiver);
    tokio::pin!(write);

    loop {
        let req_msg = tokio::select! {
            result = &mut write => {
                if let Err(error) = result {
                    if is_peer_disconnect(&error) {
                        debug!(%src, %error, "TCP peer closed the response stream");
                    } else {
                        warn!(%src, %error, "Failed to write TCP response");
                    }
                }
                break;
            }
            result = reader.read_message() => match result {
                Ok(message) => message,
                Err(error) => {
                    debug!(%src, %error, "TCP client disconnected or read error");
                    break;
                }
            }
        };
        let handler = handler.clone();
        let sender = sender.clone();
        let server_name = server_name.clone();
        // A client's connection lifetime does not cancel executor side effects.
        // The server owns these tasks and waits for them during destruction.
        requests.spawn(async move {
            let response = handler
                .handle_request(
                    req_msg,
                    src,
                    RequestMeta {
                        server_name,
                        url_path: None,
                    },
                )
                .await;
            if sender.send(response.response).await.is_err() {
                debug!(%src, "Discarding TCP response after connection closed");
            }
        });
    }
}

fn is_peer_disconnect(error: &DnsError) -> bool {
    matches!(error, DnsError::Io(error) if matches!(error.kind(),
        std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::NotConnected
    ))
}

async fn write_tcp_responses<S>(
    mut writer: TcpTransportWriter<S>,
    mut receiver: tokio::sync::mpsc::Receiver<Message>,
) -> Result<()>
where
    S: AsyncWrite + Unpin,
{
    while let Some(response) = receiver.recv().await {
        // A failed frame may have been partially written; this stream cannot
        // safely carry another frame, even if a subsequent write succeeds.
        writer.write_message(&response).await?;
    }
    Ok(())
}

/// Build a TCP socket with reuse_address and reuse_port options when available
///
/// Creates a socket optimized for DNS server workloads with port reuse enabled.
pub fn build_tcp_listener(addr: SocketAddr, idle_timeout: Duration) -> Result<TcpListener> {
    listen::build_tcp_listener(addr, 512, |sock| configure_tcp_socket(sock, idle_timeout))
}

fn configure_tcp_socket(sock: &Socket, idle_timeout: Duration) {
    let _ = sock.set_tcp_nodelay(true);
    let keepalive = TcpKeepalive::new().with_interval(idle_timeout);
    let _ = sock.set_tcp_keepalive(&keepalive);
    #[cfg(all(
        unix,
        not(any(
            target_os = "solaris",
            target_os = "illumos",
            target_os = "cygwin",
            target_os = "wasi"
        ))
    ))]
    let _ = sock.set_reuse_port(true);
    let _ = sock.set_recv_buffer_size(TCP_SOCKET_BUFFER_SIZE);
}

/// Factory for creating TCP server plugin instances
#[derive(Debug)]
#[plugin_factory("tcp_server")]
pub struct TcpServerFactory {}

#[async_trait]
impl PluginFactory for TcpServerFactory {
    /// Get dependencies (the entry executor plugin)
    fn get_dependency_specs(&self, plugin_config: &PluginConfig) -> Vec<DependencySpec> {
        if let Some(args) = &plugin_config.args
            && let Ok(config) = serde_yaml_ng::from_value::<TcpServerConfig>(args.clone())
        {
            return vec![DependencySpec::executor("args.entry", config.entry)];
        }
        vec![]
    }

    fn create(
        &self,
        plugin_config: &PluginConfig,
        init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> Result<crate::plugin::UninitializedPlugin> {
        let tcp_config = serde_yaml_ng::from_value::<TcpServerConfig>(
            plugin_config
                .args
                .clone()
                .ok_or_else(|| DnsError::plugin("TCP Server requires configuration arguments"))?,
        )
        .map_err(|e| DnsError::plugin(format!("Failed to parse TCP Server config: {}", e)))?;
        let listen = parse_listen_addr(&tcp_config.listen).map_err(|e| {
            DnsError::plugin(format!(
                "Invalid TCP listen address '{}': {}",
                tcp_config.listen, e
            ))
        })?;

        // Resolve and type-check the entry executor using contextual
        // diagnostics.
        let entry_executor = init_context.executor("args.entry", &tcp_config.entry)?;

        // Load TLS configuration if cert and key are provided
        #[cfg(feature = "server-dot")]
        let tls_acceptor = match load_tls_config(&tcp_config.cert, &tcp_config.key) {
            None => None,
            Some(res) => {
                let mut config = res?;
                config.alpn_protocols = vec![b"dot".to_vec()];
                Some(Arc::new(TlsAcceptor::from(Arc::new(config))))
            }
        };
        #[cfg(not(feature = "server-dot"))]
        if tcp_config.cert.is_some() || tcp_config.key.is_some() {
            return Err(DnsError::plugin(
                "DoT is not compiled into this build; rebuild with --features server-dot \
                 (or remove `cert`/`key` from the tcp_server config to use plain TCP)",
            ));
        }

        #[cfg(feature = "server-dot")]
        let protocol = if tls_acceptor.is_some() { "dot" } else { "tcp" };
        #[cfg(not(feature = "server-dot"))]
        let protocol = "tcp";
        let metrics = Arc::new(ServerMetrics::new(plugin_config.tag.clone(), protocol));

        Ok(crate::plugin::UninitializedPlugin::Server(Box::new(
            TcpServer {
                tag: plugin_config.tag.clone(),
                listen,
                request_handle: Arc::new(RequestHandle {
                    entry_executor,
                    metrics: Some(metrics.clone()),
                }),
                metrics,
                #[cfg(feature = "server-dot")]
                tls_acceptor,
                idle_timeout: tcp_config.idle_timeout,
                shutdown_tx: watch::channel(false).0,
                task_handle: Mutex::new(None),
            },
        )))
    }
}

#[cfg(test)]
mod tests;
