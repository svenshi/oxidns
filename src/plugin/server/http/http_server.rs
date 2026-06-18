// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::convert::Infallible;
use std::net::SocketAddr;
use std::result::Result as StdResult;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;
use http::header::{ALT_SVC, CONTENT_LENGTH};
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use rustls::ServerConfig;
use tokio::sync::{oneshot, watch};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, info, warn};

use crate::plugin::server::http::extract_client_ip;
use crate::plugin::server::http::http_dispatcher::HttpDispatcher;
use crate::plugin::server::{ConnectionGuard, tcp};

/// Main HTTP/1.1 + HTTP/2 server loop (over TCP)
///
/// Listens for incoming DNS queries and lets Hyper drive HTTP/1.1 or HTTP/2
/// on each accepted connection. Uses a task tracker and cancellation token to
/// manage active connections without polling completed tasks from the accept
/// loop.
///
/// # Architecture
/// - Accepts TCP connections (with optional TLS handshake)
/// - Performs HTTP protocol handling through Hyper's auto protocol detector
/// - Spawns a task per connection and keeps request handling inside Hyper's
///   service model
///
/// # Parameters
/// - `addr`: Listen address
/// - `dispatcher`: HTTP request dispatcher for routing
/// - `server_config`: Optional TLS server config for HTTPS
/// - `idle_timeout`: Connection idle timeout in seconds
/// - `src_ip_header`: HTTP header name to extract real client IP
#[hotpath::measure]
#[allow(clippy::too_many_arguments)]
pub async fn run_server(
    addr: SocketAddr,
    dispatcher: Arc<HttpDispatcher>,
    server_config: Option<ServerConfig>,
    alt_svc: Option<http::HeaderValue>,
    idle_timeout: Duration,
    src_ip_header: Option<String>,
    mut shutdown_rx: watch::Receiver<bool>,
    startup_tx: Option<oneshot::Sender<Result<(), String>>>,
) {
    let mut startup_tx = startup_tx;

    let listener = match tcp::build_tcp_listener(addr, idle_timeout) {
        Ok(s) => s,
        Err(e) => {
            if let Some(tx) = startup_tx.take() {
                let _ = tx.send(Err(format!(
                    "Failed to bind HTTP socket to {}: {}",
                    addr, e
                )));
            }
            error!("Failed to bind HTTP socket to {}: {}", addr, e);
            return;
        }
    };

    if let Some(tx) = startup_tx.take() {
        let _ = tx.send(Ok(()));
    }

    info!(
        listen = %addr,
        idle_timeout_secs = idle_timeout.as_secs(),
        has_tls = %server_config.is_some(),
        alt_svc = alt_svc.as_ref().and_then(|value| value.to_str().ok()),
        "HTTP server listening"
    );

    // Wrap header name in Arc to avoid cloning Strings per request
    let src_ip_header = src_ip_header.map(Arc::from);
    let alt_svc = alt_svc.map(Arc::new);

    let tasks = TaskTracker::new();
    let shutdown_token = CancellationToken::new();
    let active_connections = Arc::new(AtomicU64::new(0));
    let tls_acceptor = if let Some(mut server_config) = server_config {
        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Some(Arc::new(TlsAcceptor::from(Arc::new(server_config))))
    } else {
        None
    };

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_ok() && *shutdown_rx.borrow() {
                    break;
                }
            }
            // Accept new connections
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, src)) => {
                        let dispatcher = dispatcher.clone();
                        let src_ip_header = src_ip_header.clone();
                        let alt_svc = alt_svc.clone();
                        let tls_acceptor = tls_acceptor.clone();
                        let task_shutdown = shutdown_token.clone();
                        let active_connections = active_connections.clone();

                        let active = active_connections.fetch_add(1, Ordering::Relaxed) + 1;
                        debug!("New connection from {} (active: {})", src, active);

                        tasks.spawn(async move {
                            let _connection_guard =
                                ConnectionGuard::new(active_connections.clone(), src, "HTTP");
                            tokio::select! {
                                _ = task_shutdown.cancelled() => {}
                                _ = async move {
                                    // Handle TLS handshake if TLS is enabled
                                    if let Some(acceptor) = tls_acceptor {
                                        match acceptor.accept(stream).await {
                                            Ok(tls_stream) => {
                                                let server_name = tls_stream
                                                    .get_ref()
                                                    .1
                                                    .server_name()
                                                    .map(Arc::<str>::from);
                                                debug!("TLS handshake completed for client {}", src);
                                                handle_http_stream(
                                                    tls_stream,
                                                    src,
                                                    dispatcher,
                                                    src_ip_header,
                                                    server_name,
                                                    alt_svc,
                                                )
                                                .await;
                                            }
                                            Err(e) => {
                                                debug!("TLS handshake failed for {}: {}", src, e);
                                            }
                                        }
                                    } else {
                                        // Plain HTTP connection
                                        debug!("HTTP server connected to client {}", src);
                                        handle_http_stream(stream, src, dispatcher, src_ip_header, None, alt_svc)
                                            .await;
                                    }
                                } => {}
                            }
                        });
                    }
                    Err(e) => {
                        debug!(%e, listen = %addr, "Error accepting HTTP connection");
                    }
                }
            }
        }
    }

    shutdown_token.cancel();
    tasks.close();
    tasks.wait().await;
    info!(listen = %addr, "HTTP server stopped");
}

/// Handle HTTP requests over a stream (works for both TLS and plain HTTP)
///
/// This function:
/// 1. Lets Hyper detect HTTP/1.1 or HTTP/2 for the connection
/// 2. Extracts real client IP from HTTP headers if configured
/// 3. Reads request body with a bounded buffer
/// 4. Dispatches to the configured DoH handler
/// 5. Returns HTTP response
///
/// # Type Parameters
/// - `S`: Stream type implementing AsyncRead + AsyncWrite (e.g., TcpStream,
///   TlsStream)
#[hotpath::measure]
async fn handle_http_stream<S>(
    stream: S,
    src: SocketAddr,
    dispatcher: Arc<HttpDispatcher>,
    src_ip_header: Option<Arc<str>>,
    tls_server_name: Option<Arc<str>>,
    alt_svc: Option<Arc<http::HeaderValue>>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
{
    let service = service_fn(move |request: Request<Incoming>| {
        let dispatcher = dispatcher.clone();
        let src_ip_header = src_ip_header.clone();
        let tls_server_name = tls_server_name.clone();
        let alt_svc = alt_svc.clone();
        async move {
            handle_hyper_request(
                request,
                src,
                dispatcher,
                src_ip_header,
                tls_server_name,
                alt_svc,
            )
            .await
        }
    });

    let io = TokioIo::new(stream);
    let builder = AutoBuilder::new(TokioExecutor::new());
    if let Err(err) = builder.serve_connection(io, service).await {
        debug!("HTTP connection error from {}: {}", src, err);
    }
}

const MAX_HTTP_BODY: usize = 64 * 1024;
const INITIAL_HTTP_BODY_CAPACITY: usize = 2048;

#[inline]
async fn handle_hyper_request(
    request: Request<Incoming>,
    src: SocketAddr,
    dispatcher: Arc<HttpDispatcher>,
    src_ip_header: Option<Arc<str>>,
    tls_server_name: Option<Arc<str>>,
    alt_svc: Option<Arc<http::HeaderValue>>,
) -> StdResult<Response<Full<Bytes>>, Infallible> {
    let (parts, body) = request.into_parts();
    let method = parts.method;
    let uri = parts.uri;
    let path = Arc::from(uri.path());
    let query = uri.query().map(Arc::from);
    let client_addr = extract_client_ip(&parts.headers, &src_ip_header, src);

    debug!(
        "Received {} {} from {} (real: {})",
        method, path, src, client_addr
    );

    let body = match read_hyper_body(body, src).await {
        Ok(body) => body,
        Err(status) => return Ok(error_response(status, src, alt_svc.as_deref())),
    };

    let response = dispatcher
        .handle_request(method, path, query, body, client_addr, tls_server_name)
        .await;

    let (mut parts, response_bytes) = response.into_parts();
    insert_alt_svc_header(&mut parts.headers, alt_svc.as_deref());
    Ok(Response::from_parts(parts, Full::new(response_bytes)))
}

#[inline]
async fn read_hyper_body(
    mut body: Incoming,
    src: SocketAddr,
) -> StdResult<Bytes, http::StatusCode> {
    let mut collected = Vec::with_capacity(INITIAL_HTTP_BODY_CAPACITY);

    while let Some(frame_result) = body.frame().await {
        let frame = match frame_result {
            Ok(frame) => frame,
            Err(e) => {
                warn!("Failed to read request body chunk from {}: {}", src, e);
                return Err(http::StatusCode::BAD_REQUEST);
            }
        };

        let Ok(data) = frame.into_data() else {
            continue;
        };

        if collected.len() + data.len() > MAX_HTTP_BODY {
            warn!(
                "HTTP request body too large from {}: {}+{} bytes",
                src,
                collected.len(),
                data.len()
            );
            return Err(http::StatusCode::PAYLOAD_TOO_LARGE);
        }

        collected.extend_from_slice(&data);
    }

    Ok(Bytes::from(collected))
}

#[inline]
fn error_response(
    status: http::StatusCode,
    src: SocketAddr,
    alt_svc: Option<&http::HeaderValue>,
) -> Response<Full<Bytes>> {
    let mut response = match Response::builder()
        .status(status)
        .header(CONTENT_LENGTH, 0)
        .body(Full::new(Bytes::new()))
    {
        Ok(resp) => resp,
        Err(e) => {
            warn!("Failed to build HTTP error response for {}: {}", src, e);
            Response::new(Full::new(Bytes::new()))
        }
    };
    insert_alt_svc_header(response.headers_mut(), alt_svc);
    response
}

#[inline]
fn insert_alt_svc_header(headers: &mut http::HeaderMap, alt_svc: Option<&http::HeaderValue>) {
    if let Some(alt_svc) = alt_svc {
        headers.insert(ALT_SVC, alt_svc.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use bytes::Bytes;
    use http::{Request, StatusCode};
    use http_body_util::{BodyExt, Full};
    use hyper::client::conn::{http1, http2};
    use tokio::io::duplex;

    use super::*;
    use crate::core::context::DnsContext;
    use crate::infra::error::Result;
    use crate::plugin::Plugin;
    use crate::plugin::executor::{ExecStep, Executor};
    use crate::plugin::server::RequestHandle;
    use crate::plugin::server::http::http_dispatcher::DnsPostHandler;
    use crate::proto::{Message, Name, Question, Rcode, RecordType};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ObservedRequest {
        src_addr: SocketAddr,
        server_name: Option<String>,
        url_path: Option<String>,
    }

    #[derive(Debug)]
    struct CaptureAndRespondExecutor {
        observed: Arc<Mutex<Option<ObservedRequest>>>,
    }

    #[async_trait]
    impl Plugin for CaptureAndRespondExecutor {
        fn tag(&self) -> &str {
            "capture_and_respond"
        }

        async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
            Ok(())
        }

        async fn destroy(&self) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Executor for CaptureAndRespondExecutor {
        async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
            self.observed
                .lock()
                .expect("capture lock should not be poisoned")
                .replace(ObservedRequest {
                    src_addr: context.peer_addr(),
                    server_name: context.server_name().map(str::to_string),
                    url_path: context.url_path().map(str::to_string),
                });
            context.set_response(context.request.response(Rcode::NoError));
            Ok(ExecStep::Stop)
        }
    }

    fn make_request_handle(observed: Arc<Mutex<Option<ObservedRequest>>>) -> Arc<RequestHandle> {
        Arc::new(RequestHandle {
            entry_executor: Arc::new(CaptureAndRespondExecutor { observed }),
            metrics: None,
        })
    }

    fn make_dns_query(id: u16) -> Message {
        let mut message = Message::new();
        message.set_id(id);
        message.add_question(Question::new(
            Name::from_ascii("example.com.").expect("query name should be valid"),
            RecordType::A,
            crate::proto::DNSClass::IN,
        ));
        message
    }

    fn make_dispatcher(observed: Arc<Mutex<Option<ObservedRequest>>>) -> Arc<HttpDispatcher> {
        let request_handle = make_request_handle(observed);
        let mut dispatcher = HttpDispatcher::new();
        dispatcher.register_route(
            http::Method::POST,
            Arc::from("/dns-query"),
            Box::new(DnsPostHandler::new(request_handle)),
        );
        Arc::new(dispatcher)
    }

    async fn assert_dns_response(
        response: Response<Incoming>,
        expected_id: u16,
    ) -> http::HeaderMap {
        let headers = response.headers().clone();
        let response_bytes = response
            .into_body()
            .collect()
            .await
            .expect("response body should be readable")
            .to_bytes();

        let dns_response = Message::from_bytes(&response_bytes)
            .expect("response bytes should decode as DNS message");

        assert_eq!(dns_response.id(), expected_id);
        assert_eq!(dns_response.rcode(), Rcode::NoError);
        headers
    }

    #[tokio::test]
    async fn test_handle_http_stream_processes_http1_post_request_and_forwards_meta() {
        let observed = Arc::new(Mutex::new(None));
        let dispatcher = make_dispatcher(observed.clone());

        let (client, server) = duplex(16 * 1024);
        let server_task = tokio::spawn(handle_http_stream(
            server,
            SocketAddr::from(([127, 0, 0, 1], 443)),
            dispatcher,
            Some(Arc::from("x-real-ip")),
            Some(Arc::from("resolver.example")),
            Some(Arc::new(http::HeaderValue::from_static(
                "h3=\":443\"; ma=86400",
            ))),
        ));

        let (mut sender, connection) = http1::handshake(TokioIo::new(client))
            .await
            .expect("client handshake should succeed");
        let client_task = tokio::spawn(async move {
            let _ = connection.await;
        });

        let dns_query = make_dns_query(55);
        let dns_bytes = dns_query
            .to_bytes()
            .expect("dns query should serialize successfully");

        let request = Request::builder()
            .method("POST")
            .uri("/dns-query")
            .header("x-real-ip", "198.51.100.77")
            .body(Full::new(Bytes::from(dns_bytes)))
            .expect("http request should build");

        let response = sender
            .send_request(request)
            .await
            .expect("response future should resolve");

        let headers = assert_dns_response(response, 55).await;
        assert_eq!(headers["alt-svc"], "h3=\":443\"; ma=86400");
        assert_eq!(
            observed
                .lock()
                .expect("capture lock should not be poisoned")
                .clone(),
            Some(ObservedRequest {
                src_addr: SocketAddr::from(([198, 51, 100, 77], 443)),
                server_name: Some("resolver.example".to_string()),
                url_path: Some("/dns-query".to_string()),
            })
        );

        client_task.abort();
        server_task.abort();
    }

    #[tokio::test]
    async fn test_handle_http_stream_processes_http2_post_request_and_forwards_meta() {
        let observed = Arc::new(Mutex::new(None));
        let dispatcher = make_dispatcher(observed.clone());

        let (client, server) = duplex(16 * 1024);
        let server_task = tokio::spawn(handle_http_stream(
            server,
            SocketAddr::from(([127, 0, 0, 1], 443)),
            dispatcher,
            Some(Arc::from("x-real-ip")),
            Some(Arc::from("resolver.example")),
            Some(Arc::new(http::HeaderValue::from_static(
                "h3=\":443\"; ma=86400",
            ))),
        ));

        let (mut sender, connection) = http2::Builder::new(TokioExecutor::new())
            .handshake(TokioIo::new(client))
            .await
            .expect("client handshake should succeed");
        let client_task = tokio::spawn(async move {
            let _ = connection.await;
        });

        let dns_query = make_dns_query(56);
        let dns_bytes = dns_query
            .to_bytes()
            .expect("dns query should serialize successfully");

        let request = Request::builder()
            .method("POST")
            .uri("/dns-query")
            .header("x-real-ip", "198.51.100.78")
            .body(Full::new(Bytes::from(dns_bytes)))
            .expect("http request should build");

        let response = sender
            .send_request(request)
            .await
            .expect("response future should resolve");

        let headers = assert_dns_response(response, 56).await;
        assert_eq!(headers["alt-svc"], "h3=\":443\"; ma=86400");
        assert_eq!(
            observed
                .lock()
                .expect("capture lock should not be poisoned")
                .clone(),
            Some(ObservedRequest {
                src_addr: SocketAddr::from(([198, 51, 100, 78], 443)),
                server_name: Some("resolver.example".to_string()),
                url_path: Some("/dns-query".to_string()),
            })
        );

        client_task.abort();
        server_task.abort();
    }

    #[tokio::test]
    async fn test_handle_http_stream_rejects_oversized_http_body() {
        let observed = Arc::new(Mutex::new(None));
        let dispatcher = make_dispatcher(observed);

        let (client, server) = duplex(128 * 1024);
        let server_task = tokio::spawn(handle_http_stream(
            server,
            SocketAddr::from(([127, 0, 0, 1], 443)),
            dispatcher,
            None,
            None,
            None,
        ));

        let (mut sender, connection) = http1::handshake(TokioIo::new(client))
            .await
            .expect("client handshake should succeed");
        let client_task = tokio::spawn(async move {
            let _ = connection.await;
        });

        let request = Request::builder()
            .method("POST")
            .uri("/dns-query")
            .body(Full::new(Bytes::from(vec![0_u8; MAX_HTTP_BODY + 1])))
            .expect("http request should build");

        let response = sender
            .send_request(request)
            .await
            .expect("response future should resolve");

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            response
                .into_body()
                .collect()
                .await
                .expect("response body should be readable")
                .to_bytes()
                .len(),
            0
        );

        client_task.abort();
        server_task.abort();
    }
}
