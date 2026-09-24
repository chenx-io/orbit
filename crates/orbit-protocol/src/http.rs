//! HTTP/1.1 and HTTP/2 protocol client — manual connection + hyper protocol engine
//!
//! Per-stage timing flow:
//! DNS resolution → TCP connect → TLS handshake (incl. ALPN negotiation) → HTTP request send → TTFB → response receive → total duration
//!
//! Architecture:
//! - Manual tokio::net::lookup_host (DNS)
//! - Manual TcpStream::connect (TCP)
//! - Manual tokio_rustls::connect (TLS + ALPN negotiation of HTTP/1.1 or HTTP/2)
//! - hyper HTTP/1.1 or HTTP/2 client::conn (protocol engine, based on the established TLS stream)
//!
//! HTTP/2 features:
//! - Automatically selects the h2 protocol via TLS ALPN negotiation
//! - Supports multiplexing
//! - Drives the HTTP/2 connection with hyper_util::TokioExecutor
//!
//! Connection reuse (keep-alive):
//! - Each HttpClient instance (i.e. each VU) holds an independent connection pool and reuses connections by (scheme,host,port).
//! - A new TCP/TLS connection is created and DNS/TCP/TLS stage timings are measured only when the pool has no available connection;
//!   when an existing connection is reused these stages physically no longer occur and their timing is None (displayed as 0 in the frontend).
//! - This preserves per-stage timing while eliminating at the root the high error rate caused by "a new connection per request → client ephemeral port
//!   (TIME_WAIT) exhaustion → EADDRNOTAVAIL", thereby supporting high-RPS load testing.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use async_trait::async_trait;
use futures_util::future::poll_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls::ClientConfig;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use webpki_roots::TLS_SERVER_ROOTS;

use crate::compression::{compress, decompress, parse_content_encoding, ContentEncoding};
use crate::traits::ProtocolClient;
use crate::types::{ProtocolError, ProtocolRequest, ProtocolResponse, ProtocolTimings};

/// HTTP version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpVersion {
    Http1,
    Http2,
}

/// Connection pool key: (scheme, host, port)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TargetKey {
    scheme: String,
    host: String,
    port: u16,
}

/// A pooled reusable connection (wrapped in a Mutex to satisfy Send + Sync; used sequentially within a single VU, so no contention)
struct PooledConn {
    sender: std::sync::Mutex<Option<Box<dyn HttpSender>>>,
}

/// Per-VU connection pool cap: prevents unbounded pool growth when a load test hits many different hosts.
/// When exceeded, evict any one connection from the pool (not strictly LRU; the same target can still be reused).
const MAX_POOLED_CONNECTIONS: usize = 64;

/// HTTP protocol client (manual DNS/TCP/TLS + hyper HTTP/1.1 or HTTP/2 protocol engine)
///
/// Each instance holds an independent keep-alive connection pool (isolated per VU).
pub struct HttpClient {
    tls_config: Arc<ClientConfig>,
    /// Preferred HTTP version
    preferred_version: HttpVersion,
    /// Connection pool reused by (scheme,host,port)
    pool: HashMap<TargetKey, PooledConn>,
}

impl HttpClient {
    pub fn new() -> Self {
        Self::with_version(HttpVersion::Http1)
    }

    /// Create an HTTP/2-preferred client
    pub fn new_http2() -> Self {
        Self::with_version(HttpVersion::Http2)
    }

    fn with_version(version: HttpVersion) -> Self {
        // Install the rustls crypto provider (must be called once before using TLS)
        let _ = rustls::crypto::ring::default_provider().install_default();

        let mut tls_config = ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore {
                roots: TLS_SERVER_ROOTS.to_vec(),
            })
            .with_no_client_auth();

        // Set the ALPN protocols to support HTTP/2 negotiation
        tls_config.alpn_protocols = match version {
            HttpVersion::Http2 => vec![b"h2".to_vec(), b"http/1.1".to_vec()],
            HttpVersion::Http1 => vec![b"http/1.1".to_vec()],
        };

        Self {
            tls_config: Arc::new(tls_config),
            preferred_version: version,
            pool: HashMap::new(),
        }
    }

    /// DNS resolution + TCP connect (the shared stage for HTTP/HTTPS), filling the dns/tcp stages of timings.
    async fn resolve_and_connect(
        host: &str,
        port: u16,
        timings: &mut ProtocolTimings,
    ) -> Result<TcpStream, ProtocolError> {
        // ─── Stage 1: DNS resolution ────────────────────
        let dns_start = Instant::now();
        let addr: SocketAddr = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| ProtocolError::Dns(e.to_string()))?
            .next()
            .ok_or_else(|| ProtocolError::Dns(format!("No address found for {}:{}", host, port)))?;
        timings.dns = Some(dns_start.elapsed());

        // ─── Stage 2: TCP connect ───────────────────────
        let tcp_start = Instant::now();
        let tcp_stream = TcpStream::connect(addr)
            .await
            .map_err(|e| ProtocolError::Connect(format!("TCP connect to {}: {}", addr, e)))?;
        tcp_stream
            .set_nodelay(true)
            .map_err(|e| ProtocolError::Connect(format!("TCP set_nodelay: {}", e)))?;
        timings.tcp = Some(tcp_start.elapsed());
        Ok(tcp_stream)
    }

    /// Establish an HTTPS connection to the target host (DNS → TCP → TLS), returning (sender, timings)
    async fn connect_https(
        &self,
        host: &str,
        port: u16,
    ) -> Result<(Box<dyn HttpSender>, ProtocolTimings), ProtocolError> {
        let mut timings = ProtocolTimings::default();
        let tcp_stream = Self::resolve_and_connect(host, port, &mut timings).await?;

        // ─── Stage 3: TLS handshake (incl. ALPN negotiation) ────
        let tls_start = Instant::now();
        let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
            .map_err(|e| ProtocolError::Tls(format!("Invalid hostname '{}': {}", host, e)))?;
        let connector = TlsConnector::from(self.tls_config.clone());
        let tls_stream = connector
            .connect(server_name, tcp_stream)
            .await
            .map_err(|e| ProtocolError::Tls(format!("TLS handshake: {}", e)))?;
        timings.tls = Some(tls_start.elapsed());

        // ─── Check the ALPN negotiation result ──────────
        let negotiated_proto = tls_stream
            .get_ref()
            .1
            .alpn_protocol()
            .map(|p| String::from_utf8_lossy(p).to_string());

        let is_h2 = negotiated_proto.as_deref() == Some("h2");
        tracing::debug!(
            "ALPN negotiated: {:?} (preferred: {:?})",
            negotiated_proto,
            self.preferred_version
        );

        // ─── Build the hyper connection ─────────────────
        let io = TokioIo::new(tls_stream);

        if is_h2 {
            // HTTP/2: needs executor + io (the executor implements Clone)
            let executor = TokioExecutor::new();
            let (send_request, connection) = hyper::client::conn::http2::handshake(executor, io)
                .await
                .map_err(|e| ProtocolError::Connect(format!("HTTP/2 handshake: {}", e)))?;

            tokio::spawn(async move {
                let _ = connection.await;
            });

            Ok((
                Box::new(Http2Sender {
                    inner: send_request,
                }),
                timings,
            ))
        } else {
            let (send_request, connection) = hyper::client::conn::http1::handshake(io)
                .await
                .map_err(|e| ProtocolError::Connect(format!("HTTP/1.1 handshake: {}", e)))?;

            tokio::spawn(async move {
                let _ = connection.await;
            });

            Ok((
                Box::new(Http1Sender {
                    inner: send_request,
                }),
                timings,
            ))
        }
    }

    /// Establish an HTTP connection to the target host (DNS → TCP, no TLS, always HTTP/1.1), returning (sender, timings)
    async fn connect_http(
        &self,
        host: &str,
        port: u16,
    ) -> Result<(Box<dyn HttpSender>, ProtocolTimings), ProtocolError> {
        let mut t = ProtocolTimings::default();
        let tcp_stream = Self::resolve_and_connect(host, port, &mut t).await?;

        // ─── Build the hyper connection (no TLS) ────────
        let io = TokioIo::new(tcp_stream);
        let (send_request, connection) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| ProtocolError::Connect(format!("HTTP handshake: {}", e)))?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok((
            Box::new(Http1Sender {
                inner: send_request,
            }),
            t,
        ))
    }

    /// Choose the connect path by scheme, returning a reusable sender and the per-stage timings of this connection
    async fn connect_target(
        &self,
        is_https: bool,
        host: &str,
        port: u16,
    ) -> Result<(Box<dyn HttpSender>, ProtocolTimings), ProtocolError> {
        if is_https {
            self.connect_https(host, port).await
        } else {
            self.connect_http(host, port).await
        }
    }

    /// Send a request and fetch the unconsumed response (the connection is not returned to the pool), shared by execute / stream_response.
    async fn send_and_get(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<
        (
            TargetKey,
            Box<dyn HttpSender>,
            hyper::Response<hyper::body::Incoming>,
            ProtocolTimings,
        ),
        ProtocolError,
    > {
        // ─── Stage 1: URL parsing ───────────────────────
        let url: url::Url = request
            .target
            .parse()
            .map_err(|e| ProtocolError::Protocol(format!("Invalid URL: {}", e)))?;

        let method: hyper::Method = request
            .operation
            .parse()
            .map_err(|e| ProtocolError::Protocol(format!("Invalid method: {}", e)))?;

        let scheme = url.scheme();
        let is_https = scheme == "https";
        let host = url.host_str().unwrap_or("localhost").to_string();
        let port = url
            .port_or_known_default()
            .unwrap_or(if is_https { 443 } else { 80 });

        let key = TargetKey {
            scheme: scheme.to_string(),
            host: host.clone(),
            port,
        };

        // ─── Stages 2-4: reuse a pooled connection or create a new one (DNS → TCP → TLS) ──
        let (mut sender, mut timings) = match self.pool.remove(&key) {
            Some(pc) => {
                // Take the connection out of the pool (not holding the lock across await)
                let taken = pc.sender.lock().unwrap().take();
                match taken {
                    Some(mut sender) => {
                        // Liveness check: reuse if the connection is still usable, otherwise reconnect (leaving DNS/TCP/TLS timings empty)
                        match poll_fn(|cx| sender.poll_ready(cx)).await {
                            Ok(()) => (sender, ProtocolTimings::default()),
                            // Connection closed/invalid: discard the old connection and create a new one
                            Err(_) => {
                                drop(sender);
                                self.connect_target(is_https, &host, port).await?
                            }
                        }
                    }
                    // Should not happen in theory (the take was empty); fall back to creating a new one
                    None => self.connect_target(is_https, &host, port).await?,
                }
            }
            // No available connection in the pool: create a new one
            None => self.connect_target(is_https, &host, port).await?,
        };

        // ─── Stage 5: build and send the request ────────
        let path_and_query =
            url.path().to_string() + &url.query().map(|q| format!("?{}", q)).unwrap_or_default();
        tracing::debug!(
            "URI origin-form: {} (from {})",
            path_and_query,
            request.target
        );

        let mut req_builder = hyper::Request::builder()
            .method(method)
            .uri(&path_and_query)
            .header("Host", format!("{}:{}", host, port));

        // Headers managed by the transport layer (Host / Content-Length / chunked encoding / connection) should not be passed through from the client,
        // otherwise they conflict with hyper's automatic settings (duplicates or inconsistent values), causing the target service to
        // return 400 Bad Request during HTTP parsing, often without reaching application-layer logs.
        const SKIP_HEADERS: [&str; 5] = [
            "host",
            "content-length",
            "transfer-encoding",
            "connection",
            "keep-alive",
        ];
        for (key, value) in &request.metadata {
            if SKIP_HEADERS.iter().any(|h| h.eq_ignore_ascii_case(key)) {
                continue;
            }
            req_builder = req_builder.header(key.as_str(), value.as_str());
        }

        // Request body compression: if Content-Encoding is explicitly configured in the request headers, compress the payload
        let request_encodings = match request
            .metadata
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("Content-Encoding"))
        {
            Some((_, v)) => parse_content_encoding(v).map_err(|e| {
                ProtocolError::Codec(format!("failed to parse request Content-Encoding: {e}"))
            })?,
            None => vec![],
        };
        let payload_to_send = if request_encodings
            .iter()
            .any(|e| *e != ContentEncoding::Identity)
        {
            compress(request.payload.clone(), &request_encodings).map_err(|e| {
                ProtocolError::Codec(format!("request body compression failed: {e}"))
            })?
        } else {
            request.payload.clone()
        };

        let body_size = payload_to_send.len();
        let body = http_body_util::Full::new(hyper::body::Bytes::from(payload_to_send));
        let req = req_builder
            .body(body)
            .map_err(|e| ProtocolError::Protocol(format!("Request build error: {}", e)))?;

        let send_start = Instant::now();
        let resp = sender
            .send_request(req)
            .await
            .map_err(|e| ProtocolError::Send(e.to_string()))?;
        let send_to_first_byte = send_start.elapsed();

        if body_size < 1024 {
            timings.send = Some(std::time::Duration::from_micros(1));
            timings.first_byte = Some(send_to_first_byte);
        } else {
            let ratio = 0.2_f64.min(body_size as f64 / 10_000_000.0);
            let send_ns = (send_to_first_byte.as_nanos() as f64 * ratio) as u128;
            timings.send = Some(std::time::Duration::from_nanos(send_ns as u64));
            timings.first_byte = Some(send_to_first_byte - timings.send.unwrap());
        }

        Ok((key, sender, resp, timings))
    }

    /// Put a usable connection back into the pool (keep-alive reuse). When the cap is exceeded, evict any one connection,
    /// preventing unbounded pool growth when a load test hits many different hosts.
    fn pool_insert(&mut self, key: TargetKey, sender: Box<dyn HttpSender>) {
        if self.pool.len() >= MAX_POOLED_CONNECTIONS {
            if let Some(evict) = self.pool.keys().next().cloned() {
                self.pool.remove(&evict);
            }
        }
        self.pool.insert(
            key,
            PooledConn {
                sender: std::sync::Mutex::new(Some(sender)),
            },
        );
    }

    /// Send a request and return the unconsumed response body stream (for long-lived connections such as SSE).
    ///
    /// The connection is not returned to the pool—a keep-alive connection cannot be reused before the response body is fully consumed.
    pub async fn stream_response(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<
        (
            ProtocolTimings,
            i32,
            Vec<(String, String)>,
            hyper::body::Incoming,
        ),
        ProtocolError,
    > {
        let total_start = Instant::now();
        let (_key, _sender, resp, mut timings) = self.send_and_get(request).await?;

        let status = resp.status().as_u16() as i32;
        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        timings.total = total_start.elapsed();

        Ok((timings, status, headers, resp.into_body()))
    }
}

/// HTTP sender trait — abstracts the send interface for HTTP/1.1 and HTTP/2
#[async_trait::async_trait]
trait HttpSender: Send {
    /// Check whether the underlying connection can still accept new requests (used for liveness checks when reusing keep-alive)
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), hyper::Error>>;

    async fn send_request(
        &mut self,
        req: hyper::Request<http_body_util::Full<hyper::body::Bytes>>,
    ) -> Result<hyper::Response<hyper::body::Incoming>, hyper::Error>;
}

/// HTTP/1.1 sender
struct Http1Sender {
    inner: hyper::client::conn::http1::SendRequest<http_body_util::Full<hyper::body::Bytes>>,
}

#[async_trait::async_trait]
impl HttpSender for Http1Sender {
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), hyper::Error>> {
        self.inner.poll_ready(cx)
    }

    async fn send_request(
        &mut self,
        req: hyper::Request<http_body_util::Full<hyper::body::Bytes>>,
    ) -> Result<hyper::Response<hyper::body::Incoming>, hyper::Error> {
        self.inner.send_request(req).await
    }
}

/// HTTP/2 sender
struct Http2Sender {
    inner: hyper::client::conn::http2::SendRequest<http_body_util::Full<hyper::body::Bytes>>,
}

#[async_trait::async_trait]
impl HttpSender for Http2Sender {
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), hyper::Error>> {
        self.inner.poll_ready(cx)
    }

    async fn send_request(
        &mut self,
        req: hyper::Request<http_body_util::Full<hyper::body::Bytes>>,
    ) -> Result<hyper::Response<hyper::body::Incoming>, hyper::Error> {
        self.inner.send_request(req).await
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProtocolClient for HttpClient {
    fn name(&self) -> &str {
        match self.preferred_version {
            HttpVersion::Http2 => "http2",
            HttpVersion::Http1 => "http",
        }
    }

    fn description(&self) -> &str {
        match self.preferred_version {
            HttpVersion::Http2 => "HTTP/2 client (manual DNS/TCP/TLS + hyper HTTP/2 engine, 7-stage timing, keep-alive pool)",
            HttpVersion::Http1 => "HTTP/1.1 client (manual DNS/TCP/TLS + hyper engine, 7-stage timing, keep-alive pool)",
        }
    }

    async fn execute(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        let total_start = Instant::now();
        let mut current = request;

        // Automatically follow 3xx redirects (up to 5 hops)
        for _ in 0..=5 {
            let (key, mut sender, resp, mut timings) = self.send_and_get(current.clone()).await?;
            let status = resp.status().as_u16() as i32;

            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let location = resp
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                if let Some(loc) = location {
                    // Consume the redirect response body before returning the connection to the pool: dropping it unconsumed makes hyper decide
                    // the connection is not reusable and close it, so a redirect chain on the same host (e.g. 301→302→200) repeatedly
                    // rebuilds TCP/TLS connections.
                    let _ = http_body_util::BodyExt::collect(resp.into_body()).await;
                    if poll_fn(|cx| sender.poll_ready(cx)).await.is_ok() {
                        self.pool_insert(key, sender);
                    }

                    let new_url = url::Url::parse(&current.target)
                        .ok()
                        .and_then(|u| u.join(&loc).ok())
                        .map(|u| u.to_string())
                        .unwrap_or(loc);
                    // 303 See Other: subsequent requests become GET with no body
                    if status == 303 {
                        current.operation = "GET".to_string();
                        current.payload = Vec::new();
                    }
                    current.target = new_url;
                    continue;
                }
            }

            // ─── Stage 6: collect response headers ──────────
            let headers: Vec<(String, String)> = resp
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        value.to_str().unwrap_or("").to_string(),
                    )
                })
                .collect();

            // ─── Stage 7: receive response body ─────────────
            let receive_start = Instant::now();
            let body_bytes = http_body_util::BodyExt::collect(resp.into_body())
                .await
                .map_err(|e| ProtocolError::Receive(e.to_string()))?
                .to_bytes();
            timings.receive = Some(receive_start.elapsed());

            timings.total = total_start.elapsed();

            // Request succeeded and the connection is still usable → return it to the pool for reuse (keep-alive)
            if poll_fn(|cx| sender.poll_ready(cx)).await.is_ok() {
                self.pool_insert(key, sender);
            }

            // Response body decompression: automatically decode based on Content-Encoding (gzip / br / zstd / deflate)
            let response_encodings = match headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("Content-Encoding"))
            {
                Some((_, v)) => parse_content_encoding(v).map_err(|e| {
                    ProtocolError::Codec(format!("failed to parse response Content-Encoding: {e}"))
                })?,
                None => vec![],
            };
            let decoded_body = if response_encodings
                .iter()
                .any(|e| *e != ContentEncoding::Identity)
            {
                decompress(body_bytes.to_vec(), &response_encodings).map_err(|e| {
                    ProtocolError::Codec(format!("response body decompression failed: {e}"))
                })?
            } else {
                body_bytes.to_vec()
            };

            // Keep all original response headers (including Content-Encoding / Content-Length) without stripping them,
            // so the frontend can display them as needed; the body has already been decompressed to plaintext.

            return Ok(ProtocolResponse {
                status_code: status,
                metadata: headers,
                payload: decoded_body,
                message_count: 0,
                timings,
            });
        }

        Err(ProtocolError::Protocol("Too many redirects".into()))
    }

    fn clone_client(&self) -> Box<dyn ProtocolClient> {
        match self.preferred_version {
            HttpVersion::Http2 => Box::new(Self::new_http2()),
            HttpVersion::Http1 => Box::new(Self::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_http_get() {
        let mut client = HttpClient::new();
        let request = ProtocolRequest {
            target: "https://httpbin.org/get".into(),
            operation: "GET".into(),
            metadata: vec![("Accept".into(), "application/json".into())],
            payload: vec![],
            timeout: Some(Duration::from_secs(15)),
            streaming_mode: None,
            payload_format: None,
            response_format: None,
            options: Default::default(),
            connection: None,
        };

        let response = client.execute(request).await.expect("HTTP request failed");
        assert!(
            response.status_code == 200 || response.status_code == 503,
            "status was {}",
            response.status_code
        );
        assert!(!response.payload.is_empty());
        assert!(response.timings.total.as_millis() > 0);
        assert!(response.timings.dns.is_some());
        assert!(response.timings.tcp.is_some());
        assert!(response.timings.tls.is_some());
        assert!(response.timings.first_byte.is_some());
        assert!(response.timings.receive.is_some());
        println!("{}", response.timings);
    }

    #[tokio::test]
    async fn test_http_post() {
        let mut client = HttpClient::new();
        let body = serde_json::json!({"name": "orbit", "version": "0.1.0", "engine": "hyper"});
        let request = ProtocolRequest {
            target: "https://httpbin.org/post".into(),
            operation: "POST".into(),
            metadata: vec![("Content-Type".into(), "application/json".into())],
            payload: serde_json::to_vec(&body).unwrap(),
            timeout: Some(Duration::from_secs(15)),
            streaming_mode: None,
            payload_format: None,
            response_format: None,
            options: Default::default(),
            connection: None,
        };

        let response = client.execute(request).await.expect("HTTP request failed");
        assert!(
            response.status_code == 200
                || response.status_code == 503
                || response.status_code == 502,
            "status was {}",
            response.status_code
        );
        if response.status_code == 200 {
            let resp_body: serde_json::Value = serde_json::from_slice(&response.payload).unwrap();
            assert_eq!(resp_body["json"]["name"], "orbit");
            assert_eq!(resp_body["json"]["engine"], "hyper");
        }
        println!("{}", response.timings);
    }

    /// Depends on an external network (httpbin.org): unstable in CI/offline environments, ignored by default (run explicitly when needed)
    #[tokio::test]
    #[ignore = "requires external network access to httpbin.org"]
    async fn test_http_httpbin() {
        let mut client = HttpClient::new();
        let request = ProtocolRequest {
            target: "https://httpbin.org/delay/0".into(),
            operation: "GET".into(),
            metadata: vec![],
            payload: vec![],
            timeout: Some(Duration::from_secs(15)),
            streaming_mode: None,
            payload_format: None,
            response_format: None,
            options: Default::default(),
            connection: None,
        };
        let response = client.execute(request).await.expect("HTTP request failed");
        // httpbin.org can return various status codes under load
        assert!(
            response.status_code == 200
                || response.status_code == 503
                || response.status_code == 502,
            "status was {}",
            response.status_code
        );
        println!("{}", response.timings);
    }

    #[tokio::test]
    async fn test_keep_alive_reuse() {
        // Consecutive requests to the same target from the same client should reuse the connection:
        // The first establishes a connection (dns/tcp/tls have values), the second reuses it (dns/tcp/tls are None).
        let mut client = HttpClient::new_http2();
        let mk = || ProtocolRequest {
            target: "https://httpbin.org/get".into(),
            operation: "GET".into(),
            metadata: vec![],
            payload: vec![],
            timeout: Some(Duration::from_secs(15)),
            streaming_mode: None,
            payload_format: None,
            response_format: None,
            options: Default::default(),
            connection: None,
        };
        let r1 = client.execute(mk()).await.expect("first request failed");
        let dns1 = r1.timings.dns;
        let r2 = client.execute(mk()).await.expect("second request failed");
        // The second is a reused connection, so DNS/TCP/TLS timings should be absent (no physical reconnect)
        assert!(dns1.is_some(), "first request should measure DNS");
        assert!(
            r2.timings.dns.is_none(),
            "second request should reuse connection (no DNS)"
        );
        assert!(
            r2.timings.first_byte.is_some(),
            "per-request TTFB still measured on reuse"
        );
        assert!(
            r2.timings.receive.is_some(),
            "per-request download still measured on reuse"
        );
        assert!(
            r2.timings.total.as_millis() > 0,
            "per-request total still measured on reuse"
        );
        println!("reuse timings: {}", r2.timings);
    }

    #[tokio::test]
    async fn test_http2_client_creation() {
        let client = HttpClient::new_http2();
        assert_eq!(client.name(), "http2");
        assert!(client.description().contains("HTTP/2"));
    }
}
