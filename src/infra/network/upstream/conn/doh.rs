// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(feature = "_http-client")]
use base64::Engine;
#[cfg(feature = "_http-client")]
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
#[cfg(feature = "_http-client")]
use bytes::BytesMut;
#[cfg(feature = "_http-client")]
use http::header::CONTENT_LENGTH;
#[cfg(feature = "_http-client")]
use http::{HeaderValue, Method, Request, Response, Version, header};

#[cfg(feature = "_http-client")]
use crate::infra::network::upstream::{ConnectionInfo, ConnectionType};

/// Content type header for DNS-over-HTTPS (RFC 8484 Section 6)
#[cfg(feature = "_http-client")]
#[allow(dead_code)]
const DNS_HEADER_VALUE: HeaderValue = HeaderValue::from_static("application/dns-message");

/// Upper bound for upstream DoH/H2/H3 response bodies.
///
/// A DNS message never exceeds 64 KiB on the wire, so any larger body (or a
/// spoofed Content-Length) must not drive buffering. Mirrors the resolver-side
/// limit in `resolver/client/doh.rs`.
const MAX_DNS_MESSAGE_LEN: usize = u16::MAX as usize;

/// Build a DoH GET request with base64url-encoded DNS query
///
/// Constructs an HTTP GET request following RFC 8484 Section 4.1 (GET method).
/// The DNS message is base64url-encoded (without padding) and appended to the
/// URI.
///
/// # Arguments
/// * `uri` - Base URI with "?dns=" already appended (will add base64 query)
/// * `buf` - Raw DNS message bytes (wire format)
/// * `version` - HTTP version (HTTP/2 for h2, HTTP/3 for h3)
///
/// # Returns
/// HTTP Request with empty body (query is in URI parameter)
///
/// # Example URI
/// `https://dns.example.com/dns-query?dns=AAABAAABAAAAAAAAA3d3dwdleGFtcGxlA2NvbQAAAQAB`
#[cfg(feature = "_http-client")]
#[allow(dead_code)]
#[inline]
pub fn build_dns_get_request(mut uri: String, buf: &[u8], version: Version) -> Request<()> {
    // Encode DNS message using base64url without padding (RFC 4648 Section 5)
    uri.push_str(&BASE64_URL_SAFE_NO_PAD.encode(buf));

    http::Request::builder()
        .version(version)
        .header(header::CONTENT_TYPE, DNS_HEADER_VALUE)
        .header(header::ACCEPT, DNS_HEADER_VALUE)
        .method(Method::GET)
        .uri(uri)
        .body(())
        .expect("Failed to build HTTP request (should never fail with static headers)")
}

/// Extract and pre-allocate response buffer from HTTP response
///
/// Reads the Content-Length header to optimize buffer allocation.
/// This avoids repeated reallocations when receiving the response body.
///
/// # Arguments
/// * `response` - HTTP response with headers
///
/// # Returns
/// BytesMut buffer pre-allocated to Content-Length size (or 4KB default)
///
/// # Performance
/// Pre-allocating based on Content-Length avoids:
/// - Multiple buffer reallocations during body reception
/// - Memory copies when buffer grows
/// - Potential performance hiccups from allocator
#[cfg(feature = "_http-client")]
#[allow(dead_code)]
#[inline]
pub fn get_cap_buf_with_context_len<T>(response: &mut Response<T>) -> BytesMut {
    let capacity = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .map(|v| v.min(MAX_DNS_MESSAGE_LEN)) // Never trust a remote header with pre-allocation size
        .unwrap_or(4096); // Default 4KB for typical DNS responses

    BytesMut::with_capacity(capacity)
}

/// Append one body chunk to a response buffer, rejecting bodies larger than
/// a DNS message.
///
/// Content-Length only sizes the pre-allocation; the streamed body itself can
/// still exceed any advertised length, so every chunk passes through this
/// bound.
#[cfg(feature = "_http-client")]
#[allow(dead_code)]
#[inline]
pub fn append_response_chunk(
    response: &mut BytesMut,
    chunk: &[u8],
) -> crate::infra::error::Result<()> {
    use bytes::BufMut;

    let next_len = response.len().checked_add(chunk.len()).ok_or_else(|| {
        crate::infra::error::DnsError::protocol(
            "upstream DoH response body exceeds DNS message size limit",
        )
    })?;
    if next_len > MAX_DNS_MESSAGE_LEN {
        return Err(crate::infra::error::DnsError::protocol(
            "upstream DoH response body exceeds DNS message size limit",
        ));
    }
    response.put_slice(chunk);
    Ok(())
}

/// Build DoH request URI template from connection info
///
/// Constructs the full HTTPS URI for DoH requests, handling non-standard ports.
/// The returned URI ends with "?dns=" ready for base64url-encoded query to be
/// appended.
///
/// # Arguments
/// * `connection_info` - Connection configuration with server name, port, and
///   path
///
/// # Returns
/// String containing "https://server:port/path?dns=" (port omitted if 443)
///
/// # Examples
/// - Standard port: `https://dns.example.com/dns-query?dns=`
/// - Custom port: `https://dns.example.com:8443/dns-query?dns=`
///
/// # Performance
/// Pre-reserves 512 bytes to accommodate the base64-encoded DNS query without
/// reallocation
#[cfg(feature = "_http-client")]
#[allow(dead_code)]
pub fn build_doh_request_uri(connection_info: &ConnectionInfo) -> String {
    let mut uri = if connection_info.port != ConnectionType::DoH.default_port() {
        // Include port in URI for non-standard ports
        format!(
            "https://{}:{}{}?dns=",
            connection_info.server_name, connection_info.port, connection_info.path
        )
    } else {
        // Omit port 443 (standard HTTPS port) from URI
        format!(
            "https://{}{}?dns=",
            connection_info.server_name, connection_info.path
        )
    };

    // Pre-allocate space for base64url-encoded DNS query (~600 bytes for
    // typical query)
    uri.reserve(512);
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_dns_get_request_sets_uri_method_and_headers() {
        let request = build_dns_get_request(
            "https://dns.example.test/dns-query?dns=".to_string(),
            &[0, 1, 2, 3],
            Version::HTTP_2,
        );

        assert_eq!(request.method(), Method::GET);
        assert_eq!(request.version(), Version::HTTP_2);
        assert_eq!(
            request.uri().to_string(),
            "https://dns.example.test/dns-query?dns=AAECAw"
        );
        assert_eq!(request.headers()[header::CONTENT_TYPE], DNS_HEADER_VALUE);
    }

    #[test]
    fn test_get_cap_buf_with_context_len_uses_content_length_header() {
        let mut response = Response::builder()
            .header(CONTENT_LENGTH, "128")
            .body(())
            .expect("response should build");

        let buf = get_cap_buf_with_context_len(&mut response);

        assert_eq!(buf.capacity(), 128);
    }

    #[test]
    fn test_get_cap_buf_with_context_len_uses_default_capacity_without_header() {
        let mut response = Response::builder().body(()).expect("response should build");

        let buf = get_cap_buf_with_context_len(&mut response);

        assert_eq!(buf.capacity(), 4096);
    }

    #[test]
    fn test_get_cap_buf_with_context_len_clamps_huge_content_length() {
        let mut response = Response::builder()
            .header(CONTENT_LENGTH, "99999999999")
            .body(())
            .expect("response should build");

        let buf = get_cap_buf_with_context_len(&mut response);

        assert_eq!(buf.capacity(), u16::MAX as usize);
    }

    #[test]
    fn test_append_response_chunk_rejects_oversize_body() {
        let full = vec![0_u8; u16::MAX as usize];
        let mut buf = BytesMut::with_capacity(u16::MAX as usize);
        assert!(append_response_chunk(&mut buf, &full).is_ok());
        assert_eq!(buf.len(), u16::MAX as usize);
        assert!(append_response_chunk(&mut buf, &[0_u8; 1][..]).is_err());

        let oversize = vec![0_u8; u16::MAX as usize + 1];
        let mut fresh = BytesMut::with_capacity(16);
        assert!(append_response_chunk(&mut fresh, &oversize).is_err());
    }

    #[test]
    fn test_build_doh_request_uri_omits_default_https_port() {
        let mut connection_info = ConnectionInfo::with_addr("https://dns.example.test/dns-query")
            .expect("connection info should parse");
        connection_info.port = 443;

        let uri = build_doh_request_uri(&connection_info);

        assert_eq!(uri, "https://dns.example.test/dns-query?dns=");
    }

    #[test]
    fn test_build_doh_request_uri_includes_custom_port() {
        let mut connection_info = ConnectionInfo::with_addr("https://dns.example.test/dns-query")
            .expect("connection info should parse");
        connection_info.port = 8443;

        let uri = build_doh_request_uri(&connection_info);

        assert_eq!(uri, "https://dns.example.test:8443/dns-query?dns=");
    }
}
