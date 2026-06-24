// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP Dispatcher - Routes requests based on method and path.

use std::net::SocketAddr;
use std::sync::Arc;

use ahash::AHashMap;
use bytes::Bytes;
use http::header::CONTENT_TYPE;
use http::{HeaderValue, Method, Response, StatusCode};
use tracing::{debug, warn};

use crate::plugin::server::http::entry::HttpDnsEntry;

const CONTENT_TYPE_TEXT_PLAIN: HeaderValue = HeaderValue::from_static("text/plain");

/// HTTP Dispatcher - Manages routes and handlers.
///
/// The dispatcher maintains a map of `Path -> Entry` and routes incoming HTTP
/// requests to the appropriate entry based on the request path.
pub struct HttpDispatcher {
    routes: AHashMap<Arc<str>, HttpDnsEntry>,
}

impl HttpDispatcher {
    /// Create a new HTTP dispatcher.
    pub fn new() -> Self {
        Self {
            routes: AHashMap::new(),
        }
    }

    /// Register a route entry.
    ///
    /// Associates an HTTP path with a DNS entry that will process requests
    /// matching that route.
    pub fn register_route(&mut self, path: Arc<str>, entry: HttpDnsEntry) {
        self.routes.insert(path, entry);
    }

    /// Handle an HTTP request.
    ///
    /// Dispatches the request to the appropriate entry based on path. Returns
    /// a 404 response if no matching route is found.
    #[hotpath::measure]
    pub async fn handle_request(
        &self,
        method: Method,
        path: Arc<str>,
        query: Option<Arc<str>>,
        body: Bytes,
        src_addr: SocketAddr,
        server_name: Option<Arc<str>>,
    ) -> Response<Bytes> {
        debug!("Received request: {} {} from {}", method, path, src_addr);

        if let Some(entry) = self.routes.get(&path) {
            entry
                .handle(method, path, query, body, src_addr, server_name)
                .await
        } else {
            warn!("Route not found: {} {}", method, path);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(CONTENT_TYPE, CONTENT_TYPE_TEXT_PLAIN)
                .body(Bytes::from_static(b"404 Not Found"))
                .expect("Failed to build 404 response")
        }
    }
}

impl Default for HttpDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handle_request_returns_not_found_for_unregistered_route() {
        let dispatcher = HttpDispatcher::new();

        let response = dispatcher
            .handle_request(
                Method::GET,
                Arc::from("/missing"),
                None,
                Bytes::new(),
                SocketAddr::from(([127, 0, 0, 1], 5400)),
                None,
            )
            .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.body().as_ref(), b"404 Not Found");
    }
}
