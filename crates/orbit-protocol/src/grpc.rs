//! gRPC protocol client implementation
//!
//! Built on tonic transport + prost. Supports:
//! - Unary calls
//! - Server streaming
//! - Client streaming
//! - Bidirectional streaming
//! - gRPC Server Reflection (service discovery, method description, dynamic proto encode/decode)
//!
//! # Usage
//!
//! ```ignore
//! let mut client = GrpcClient::new();
//! // List available services
//! let services = client.list_services().await?;
//! // Invoke a method (JSON payload auto-converted)
//! let request = ProtocolRequest {
//!     target: "http://localhost:50051".into(),
//!     operation: "/package.Service/Method".into(),
//!     payload: json_bytes,
//!     payload_format: Some("json".into()),
//!     ..Default::default()
//! };
//! let response = client.execute(request).await?;
//! ```

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};

use tower::Service;
use tower::ServiceExt;

#[cfg(feature = "grpc")]
use orbit_codec::protobuf::DynamicProto;

use crate::traits::{MessageStream, ProtocolClient};
use crate::types::{
    ProtocolError, ProtocolRequest, ProtocolResponse, ProtocolTimings, StreamMessage,
    StreamingMode, StreamingResponse,
};

/// Persistent streaming session handle: response body stream + request frame sender (reused by the session layer)
pub struct PersistentGrpcStream {
    /// Response body stream (the caller reads gRPC messages frame by frame).
    /// For client_streaming / bidirectional the response header only arrives after the server responds, so it is provided lazily via `body_rx`.
    pub body: Option<tonic::body::Body>,
    /// Deferred response body stream receiver: used when `body` is None
    pub body_rx: Option<oneshot::Receiver<tonic::body::Body>>,
    /// Request frame sender; None for server streaming (the request side is half-closed after the first message)
    pub request_tx: Option<mpsc::Sender<Bytes>>,
    /// Initial response status code
    pub status_code: u16,
}

impl PersistentGrpcStream {
    /// Await/take the response body stream: return it immediately if held directly, otherwise await the deferred channel
    pub async fn take_body(&mut self) -> Option<tonic::body::Body> {
        if let Some(b) = self.body.take() {
            return Some(b);
        }
        if let Some(rx) = self.body_rx.take() {
            return rx.await.ok();
        }
        None
    }
}

#[cfg(feature = "grpc")]
use crate::grpc_reflection::{GrpcReflectionClient, MethodDescriptorInfo, ServiceDescriptorCache};

/// gRPC call type
#[derive(Debug, Clone, Copy, PartialEq)]
enum GrpcCallType {
    Unary,
    ServerStreaming,
    ClientStreaming,
    Bidirectional,
}

impl GrpcCallType {
    fn from_streaming_mode(mode: Option<StreamingMode>) -> Self {
        match mode {
            None => GrpcCallType::Unary,
            Some(StreamingMode::ServerStreaming) => GrpcCallType::ServerStreaming,
            Some(StreamingMode::ClientStreaming) => GrpcCallType::ClientStreaming,
            Some(StreamingMode::Bidirectional) => GrpcCallType::Bidirectional,
        }
    }
}

/// gRPC protocol client
pub struct GrpcClient {
    channel: Option<tonic::transport::Channel>,
    last_target: String,
    /// gRPC reflection cache
    #[cfg(feature = "grpc")]
    service_cache: Option<ServiceDescriptorCache>,
    /// Whether loading reflection has been attempted
    #[cfg(feature = "grpc")]
    reflection_loaded: bool,
    /// Parsed descriptor pool (for dynamic JSON↔proto encode/decode)
    #[cfg(feature = "grpc")]
    descriptor_pool: Option<DynamicProto>,
}

impl GrpcClient {
    pub fn new() -> Self {
        Self {
            channel: None,
            last_target: String::new(),
            #[cfg(feature = "grpc")]
            service_cache: None,
            #[cfg(feature = "grpc")]
            reflection_loaded: false,
            #[cfg(feature = "grpc")]
            descriptor_pool: None,
        }
    }

    async fn ensure_channel(&mut self, target: &str) -> Result<(), ProtocolError> {
        if self.channel.is_some() && self.last_target == target {
            return Ok(());
        }

        tracing::debug!("Creating gRPC channel to: {}", target);
        let channel = tonic::transport::Endpoint::from_shared(target.to_string())
            .map_err(|e| ProtocolError::Connect(format!("Invalid gRPC endpoint: {}", e)))?
            .connect()
            .await
            .map_err(|e| ProtocolError::Connect(format!("gRPC connect failed: {}", e)))?;

        self.channel = Some(channel);
        self.last_target = target.to_string();

        // Reset reflection state
        #[cfg(feature = "grpc")]
        {
            self.service_cache = None;
            self.reflection_loaded = false;
            self.descriptor_pool = None;
        }

        Ok(())
    }

    fn build_uri(target: &str, operation: &str) -> Result<http::Uri, ProtocolError> {
        let path = if operation.starts_with('/') {
            operation.to_string()
        } else {
            format!("/{}", operation)
        };
        format!("{}{}", target, path)
            .parse()
            .map_err(|e| ProtocolError::Protocol(format!("Invalid gRPC URI: {}", e)))
    }

    // ── Shared request build/send helpers (used by unary / streaming / persistent) ──────────

    /// Convert the JSON payload to protobuf binary according to payload_format (requires reflection descriptors);
    /// returns it unchanged when it is not JSON or descriptors are unavailable.
    async fn prepare_payload(
        &mut self,
        request: &ProtocolRequest,
    ) -> Result<Vec<u8>, ProtocolError> {
        if request.payload_format.as_deref() == Some("json") {
            self.ensure_reflection().await?;
            self.convert_json_to_proto(&request.operation, &request.payload)
        } else {
            Ok(request.payload.clone())
        }
    }

    /// Build the gRPC HTTP/2 request: always POST / `application/grpc+proto` / `te: trailers`.
    fn build_grpc_http_request(
        target: &str,
        operation: &str,
        body: tonic::body::Body,
    ) -> Result<hyper::Request<tonic::body::Body>, ProtocolError> {
        let uri = Self::build_uri(target, operation)?;
        hyper::Request::builder()
            .method(hyper::Method::POST)
            .uri(uri)
            .header("content-type", "application/grpc+proto")
            .header("te", "trailers")
            .body(body)
            .map_err(|e| ProtocolError::Send(e.to_string()))
    }

    /// Send a gRPC request and fetch the response: always `channel.ready()` + `call` + response-header metadata extraction.
    /// Returns `(HTTP status code, metadata, response body)`.
    ///
    /// tonic Channel is backed by a tower Buffer: poll_ready must be called before the call,
    /// otherwise it panics with `send_item called without first calling poll_reserve`.
    async fn send_grpc_request(
        &self,
        request: &ProtocolRequest,
        body: tonic::body::Body,
    ) -> Result<(i32, Vec<(String, String)>, tonic::body::Body), ProtocolError> {
        let tonic_req = Self::build_grpc_http_request(&request.target, &request.operation, body)?;

        let mut channel = self
            .channel
            .clone()
            .ok_or_else(|| ProtocolError::Connect("gRPC channel not established".into()))?;
        channel
            .ready()
            .await
            .map_err(|e| ProtocolError::Connect(format!("gRPC channel not ready: {e}")))?;

        let resp = channel
            .call(tonic_req)
            .await
            .map_err(|e| ProtocolError::Send(format!("gRPC call failed: {}", e)))?;

        let status_code = resp.status().as_u16() as i32;
        let mut metadata: Vec<(String, String)> = vec![];
        if let Some(grpc_status) = resp.headers().get("grpc-status") {
            metadata.push((
                "grpc-status".into(),
                grpc_status.to_str().unwrap_or("0").into(),
            ));
        }
        if let Some(grpc_msg) = resp.headers().get("grpc-message") {
            metadata.push((
                "grpc-message".into(),
                grpc_msg.to_str().unwrap_or("").into(),
            ));
        }
        Ok((status_code, metadata, resp.into_body()))
    }

    // ── Reflection API ──────────────────────────────────────────

    /// Establish the connection to a gRPC server (a prerequisite for Server Reflection service discovery;
    /// ordinary execute calls establish it lazily, so calling this explicitly is not required)
    #[cfg(feature = "grpc")]
    pub async fn connect(&mut self, target: &str) -> Result<(), ProtocolError> {
        self.ensure_channel(target).await
    }

    /// List all available gRPC services (via Server Reflection)
    #[cfg(feature = "grpc")]
    pub async fn list_services(&mut self) -> Result<Vec<String>, ProtocolError> {
        self.ensure_reflection().await?;
        match &self.service_cache {
            Some(cache) => Ok(cache.services.clone()),
            None => Ok(vec![]),
        }
    }

    /// Get descriptor information for a method (via Server Reflection)
    #[cfg(feature = "grpc")]
    pub async fn describe_method(
        &mut self,
        service_name: &str,
        method_name: &str,
    ) -> Result<MethodDescriptorInfo, ProtocolError> {
        self.ensure_reflection().await?;
        let key = format!("{}/{}", service_name, method_name);
        match &self.service_cache {
            Some(cache) => cache.methods.get(&key).cloned().ok_or_else(|| {
                ProtocolError::Protocol(format!("Method {} not found in reflection cache", key))
            }),
            None => Err(ProtocolError::Protocol(
                "Reflection cache not available".into(),
            )),
        }
    }

    /// Get the service descriptor cache
    #[cfg(feature = "grpc")]
    pub fn service_cache(&self) -> Option<&ServiceDescriptorCache> {
        self.service_cache.as_ref()
    }

    /// Get the FileDescriptorProto bytes of the given service
    #[cfg(feature = "grpc")]
    pub fn get_service_descriptors(&self, service_name: &str) -> Option<&Vec<Vec<u8>>> {
        self.service_cache
            .as_ref()
            .and_then(|c| c.file_descriptors.get(service_name))
    }

    #[cfg(feature = "grpc")]
    async fn ensure_reflection(&mut self) -> Result<(), ProtocolError> {
        if self.reflection_loaded {
            return Ok(());
        }

        let channel = match &self.channel {
            Some(c) => c.clone(),
            None => {
                return Err(ProtocolError::Connect(
                    "gRPC channel not established".into(),
                ))
            }
        };

        let mut reflection_client = GrpcReflectionClient::new(channel);
        match reflection_client.build_cache().await {
            Ok(cache) => {
                tracing::info!("gRPC reflection: loaded {} services", cache.services.len());
                self.service_cache = Some(cache);
            }
            Err(e) => {
                tracing::warn!("gRPC reflection not available: {}", e);
                self.service_cache = Some(ServiceDescriptorCache::default());
            }
        }

        self.reflection_loaded = true;

        // Build the descriptor pool: used for dynamic JSON↔proto encode/decode
        #[cfg(feature = "grpc")]
        {
            let all_files: Vec<Vec<u8>> = self
                .service_cache
                .as_ref()
                .map(|c| c.file_descriptors.values().flatten().cloned().collect())
                .unwrap_or_default();
            self.descriptor_pool = DynamicProto::from_file_descriptors(&all_files).ok();
        }

        Ok(())
    }

    // ── JSON payload conversion ─────────────────────────────────

    /// Convert a JSON payload to protobuf binary (dynamic encode/decode based on reflection descriptors)
    #[cfg(feature = "grpc")]
    fn convert_json_to_proto(
        &self,
        operation: &str,
        json_bytes: &[u8],
    ) -> Result<Vec<u8>, ProtocolError> {
        let pool = match &self.descriptor_pool {
            Some(p) => p,
            None => return Ok(json_bytes.to_vec()),
        };
        let (service_name, method_name) = split_operation(operation)?;
        let method_key = format!("{}/{}", service_name, method_name);
        let input_type = match &self.service_cache {
            Some(cache) => cache.methods.get(&method_key).map(|i| i.input_type.clone()),
            None => None,
        };
        let input_type = match input_type {
            Some(t) => t,
            None => return Ok(json_bytes.to_vec()),
        };
        pool.encode_json(&input_type, json_bytes)
            .map_err(|e| ProtocolError::Protocol(format!("JSON→proto encode/decode failed: {}", e)))
    }

    #[cfg(not(feature = "grpc"))]
    fn convert_json_to_proto(
        &self,
        _operation: &str,
        json_bytes: &[u8],
    ) -> Result<Vec<u8>, ProtocolError> {
        Ok(json_bytes.to_vec())
    }

    /// Convert a protobuf binary response to JSON (dynamic encode/decode based on reflection descriptors)
    #[cfg(feature = "grpc")]
    fn convert_proto_to_json(
        &self,
        operation: &str,
        proto_bytes: &[u8],
    ) -> Option<serde_json::Value> {
        let pool = self.descriptor_pool.as_ref()?;
        let (service_name, method_name) = split_operation(operation).ok()?;
        let method_key = format!("{}/{}", service_name, method_name);
        let cache = self.service_cache.as_ref()?;
        let info = cache.methods.get(&method_key)?;
        pool.decode_to_json(&info.output_type, proto_bytes).ok()
    }

    #[cfg(not(feature = "grpc"))]
    fn convert_proto_to_json(
        &self,
        _operation: &str,
        _proto_bytes: &[u8],
    ) -> Option<serde_json::Value> {
        None
    }

    // ── gRPC call implementation ────────────────────────────────

    async fn execute_unary(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        let total_start = Instant::now();

        let connect_start = Instant::now();
        self.ensure_channel(&request.target).await?;
        let connect_duration = connect_start.elapsed();

        let payload = self.prepare_payload(&request).await?;

        let send_start = Instant::now();
        let (status, mut metadata, body) = self
            .send_grpc_request(
                &request,
                tonic::body::Body::new(http_body_util::Full::new(encode_grpc_frame(payload))),
            )
            .await?;
        let send_duration = send_start.elapsed();

        let receive_start = Instant::now();
        let collected = http_body_util::BodyExt::collect(body)
            .await
            .map_err(|e| ProtocolError::Receive(e.to_string()))?;
        let receive_duration = receive_start.elapsed();

        // The gRPC status code/error message usually lives in the trailers (after the response body)—reading only the headers misses the error
        // status, so a failed call is misjudged as successful (e.g. a non-OK gRPC status with an HTTP-layer 200).
        if let Some(trailers) = collected.trailers() {
            if let Some(v) = trailers.get("grpc-status") {
                metadata.push(("grpc-status".into(), v.to_str().unwrap_or("0").into()));
            }
            if let Some(v) = trailers.get("grpc-message") {
                metadata.push(("grpc-message".into(), v.to_str().unwrap_or("").into()));
            }
        }

        // Decode the gRPC frame: 1-byte compression flag + 4-byte big-endian length + payload (gzip responses supported)
        let raw_payload = decode_grpc_payload(&collected.to_bytes())?;

        // Attempt proto → JSON conversion
        let final_payload = if request.response_format.as_deref() == Some("json") {
            self.ensure_reflection().await?;
            if let Some(json_val) = self.convert_proto_to_json(&request.operation, &raw_payload) {
                serde_json::to_vec(&json_val).unwrap_or(raw_payload)
            } else {
                raw_payload
            }
        } else {
            raw_payload
        };

        Ok(ProtocolResponse {
            status_code: status,
            metadata,
            payload: final_payload,
            message_count: 0,
            timings: ProtocolTimings {
                dns: None,
                tcp: Some(connect_duration),
                tls: None,
                send: Some(send_duration),
                first_byte: None,
                receive: Some(receive_duration),
                total: total_start.elapsed(),
            },
        })
    }

    async fn execute_grpc_streaming(
        &mut self,
        request: ProtocolRequest,
        client_messages: Option<MessageStream>,
        _call_type: GrpcCallType,
    ) -> Result<StreamingResponse, ProtocolError> {
        let total_start = Instant::now();

        self.ensure_channel(&request.target).await?;

        let payload = self.prepare_payload(&request).await?;

        // Assemble the request body: the initial payload as the first message (if any), followed by the client-stream messages (one gRPC frame each).
        // This is how client/bidirectional streaming actually sends messages instead of discarding them after the response ends.
        let mut frames: Vec<Bytes> = Vec::new();
        if !payload.is_empty() {
            frames.push(encode_grpc_frame(payload));
        }
        if let Some(mut client_stream) = client_messages {
            while let Some(msg) = client_stream.next().await {
                let msg = msg.map_err(|e| {
                    ProtocolError::Send(format!("client stream message failed: {e}"))
                })?;
                frames.push(encode_grpc_frame(msg.payload));
            }
        }

        let (status_code, mut metadata, body) = self
            .send_grpc_request(
                &request,
                tonic::body::Body::new(http_body_util::StreamBody::new(
                    futures_util::stream::iter(
                        frames
                            .into_iter()
                            .map(|b| Ok::<_, std::io::Error>(hyper::body::Frame::data(b))),
                    ),
                )),
            )
            .await?;

        let mut messages = Vec::new();
        let mut body_stream = body;
        let mut buffer: Vec<u8> = Vec::new();
        // Consumed byte offset: frame parsing is based on read_offset, avoiding a drain shift per message (O(n²))
        let mut read_offset = 0usize;
        // Message arrival times are all relative to the "stream start" (the original implementation recorded parse time, ≈0, which is meaningless)
        let stream_started = Instant::now();
        let mut index: u64 = 0;

        use http_body_util::BodyExt;
        while let Some(frame_result) = body_stream.frame().await {
            match frame_result {
                Ok(frame) => {
                    if let Some(data) = frame.data_ref() {
                        buffer.extend_from_slice(data);
                    }
                    // The gRPC status code/error message may live in the trailers (after the response body)
                    if let Some(trailers) = frame.trailers_ref() {
                        if let Some(v) = trailers.get("grpc-status") {
                            metadata.push(("grpc-status".into(), v.to_str().unwrap_or("0").into()));
                        }
                        if let Some(v) = trailers.get("grpc-message") {
                            metadata.push(("grpc-message".into(), v.to_str().unwrap_or("").into()));
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("gRPC stream frame error: {}", e);
                    break;
                }
            }

            // Parse complete frames starting at read_offset (compression flag + length + payload)
            while buffer.len() - read_offset >= 5 {
                let compressed = buffer[read_offset];
                let msg_len = u32::from_be_bytes([
                    buffer[read_offset + 1],
                    buffer[read_offset + 2],
                    buffer[read_offset + 3],
                    buffer[read_offset + 4],
                ]) as usize;

                if buffer.len() - read_offset < 5 + msg_len {
                    break;
                }

                let payload = buffer[read_offset + 5..read_offset + 5 + msg_len].to_vec();
                read_offset += 5 + msg_len;

                // Decode according to the compression flag (0=plaintext, 1=gzip)
                let payload = decompress_grpc_payload(compressed, payload)?;

                messages.push(StreamMessage {
                    index,
                    payload,
                    // Message arrival time (since stream start), not parse time
                    duration: stream_started.elapsed(),
                });
                index += 1;
            }

            // Periodically reclaim the consumed buffer to avoid unbounded growth on long streams
            if read_offset > 64 * 1024 {
                buffer.drain(..read_offset);
                read_offset = 0;
            }
        }

        Ok(StreamingResponse {
            status_code,
            metadata,
            messages,
            message_count: index,
            total_duration: total_start.elapsed(),
        })
    }

    /// Open a persistent streaming call (server / client / bidirectional streaming).
    /// The connection stays alive after the call: the response body stream yields frames and `request_tx` can keep sending gRPC frames.
    ///
    /// - server_streaming: the request side is half-closed after the first message (`request_tx` set to None)
    pub async fn open_persistent_stream(
        &mut self,
        request: ProtocolRequest,
        mode: StreamingMode,
    ) -> Result<PersistentGrpcStream, ProtocolError> {
        let half_close_after_initial = matches!(mode, StreamingMode::ServerStreaming);
        self.open_stream_impl(request, half_close_after_initial, false)
            .await
    }

    /// Open an interactively sendable persistent stream (client_streaming / bidirectional).
    ///
    /// Key difference from `open_persistent_stream`: for client_streaming the server waits for the client
    /// to half-close the request stream before returning the response header, so `channel.call()` must not block connection setup—here
    /// `call` is placed in a background task, the initial message is sent and control returns immediately, and the response body stream is provided lazily via `body_rx`.
    /// For bidirectional the server returns the response stream immediately, also taking the deferred path, with consistent behavior.
    pub async fn open_sendable_stream(
        &mut self,
        request: ProtocolRequest,
        mode: StreamingMode,
    ) -> Result<PersistentGrpcStream, ProtocolError> {
        let _ = mode;
        self.open_stream_impl(request, false, true).await
    }

    /// Shared implementation for opening a persistent streaming call.
    ///
    /// - `half_close_after_initial`: for server_streaming, close the request side after the first message
    /// - `deferred_response`: for client_streaming/bidi, wait for the response header in the background and provide the body lazily via oneshot
    async fn open_stream_impl(
        &mut self,
        request: ProtocolRequest,
        half_close_after_initial: bool,
        deferred_response: bool,
    ) -> Result<PersistentGrpcStream, ProtocolError> {
        self.ensure_channel(&request.target).await?;

        let payload = self.prepare_payload(&request).await?;

        let (request_tx, request_rx) = mpsc::channel::<Bytes>(16);
        if !payload.is_empty() {
            request_tx
                .send(encode_grpc_frame(payload))
                .await
                .map_err(|e| ProtocolError::Send(format!("gRPC initial message failed: {e}")))?;
        }

        let body_stream = futures_util::stream::unfold(request_rx, |mut rx| async move {
            rx.recv()
                .await
                .map(|frame| (Ok::<_, std::io::Error>(hyper::body::Frame::data(frame)), rx))
        });

        let tonic_req = Self::build_grpc_http_request(
            &request.target,
            &request.operation,
            tonic::body::Body::new(http_body_util::StreamBody::new(body_stream)),
        )?;

        let mut channel = self
            .channel
            .clone()
            .ok_or_else(|| ProtocolError::Connect("gRPC channel not established".into()))?;
        channel
            .ready()
            .await
            .map_err(|e| ProtocolError::Connect(format!("gRPC channel not ready: {e}")))?;

        let call_fut = channel.call(tonic_req);

        if deferred_response {
            // client_streaming returns the response header only after the client half-closes; bidi returns immediately.
            // Wait for the response header in the background and return the connection handle first, so ready is not blocked.
            let (body_tx, body_rx) = oneshot::channel();
            tokio::spawn(async move {
                match call_fut.await {
                    Ok(resp) => {
                        let _ = body_tx.send(resp.into_body());
                    }
                    Err(e) => {
                        // Notify the session layer that the response stream has ended/failed
                        let _ = e;
                        drop(body_tx);
                    }
                }
            });

            Ok(PersistentGrpcStream {
                body: None,
                body_rx: Some(body_rx),
                request_tx: Some(request_tx),
                status_code: 200,
            })
        } else {
            let resp = call_fut
                .await
                .map_err(|e| ProtocolError::Send(format!("gRPC streaming call failed: {e}")))?;

            let sender = if half_close_after_initial {
                drop(request_tx);
                None
            } else {
                Some(request_tx)
            };
            let status_code = resp.status().as_u16();
            Ok(PersistentGrpcStream {
                body: Some(resp.into_body()),
                body_rx: None,
                request_tx: sender,
                status_code,
            })
        }
    }

    /// Single-message JSON → protobuf (for interactive session sending)
    pub fn encode_json_message(
        &self,
        operation: &str,
        json_bytes: &[u8],
    ) -> Result<Vec<u8>, ProtocolError> {
        self.convert_json_to_proto(operation, json_bytes)
    }

    /// Single-message protobuf → JSON (for interactive session receiving)
    pub fn decode_proto_message(&self, operation: &str, proto_bytes: &[u8]) -> Option<Vec<u8>> {
        self.convert_proto_to_json(operation, proto_bytes)
            .map(|v| serde_json::to_vec(&v).unwrap_or_else(|_| proto_bytes.to_vec()))
    }

    /// Decode snapshot for the background streaming-consumer thread: clone the descriptor pool + service cache,
    /// so response frames can complete proto → JSON decoding in a separate task without contending with the send loop for `&mut self`.
    #[cfg(feature = "grpc")]
    pub fn decode_snapshot(&self) -> Option<(DynamicProto, ServiceDescriptorCache)> {
        Some((self.descriptor_pool.clone()?, self.service_cache.clone()?))
    }
}

/// Decode a gRPC binary frame into JSON bytes using the decode snapshot (called by the background streaming-consumer thread).
#[cfg(feature = "grpc")]
pub fn decode_message_snapshot(
    snapshot: &(DynamicProto, ServiceDescriptorCache),
    operation: &str,
    proto_bytes: &[u8],
) -> Option<Vec<u8>> {
    let (pool, cache) = snapshot;
    let (service_name, method_name) = split_operation(operation).ok()?;
    let method_key = format!("{}/{}", service_name, method_name);
    let info = cache.methods.get(&method_key)?;
    pool.decode_to_json(&info.output_type, proto_bytes)
        .ok()
        .map(|v| serde_json::to_vec(&v).unwrap_or_else(|_| proto_bytes.to_vec()))
}

/// Encode a single gRPC message frame: 1-byte compression flag (0=uncompressed) + 4-byte big-endian length + payload
pub fn encode_grpc_frame(payload: Vec<u8>) -> Bytes {
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(0);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(&payload);
    Bytes::from(buf)
}

/// Decompress the message payload according to the gRPC compression flag (0=plaintext, 1=gzip, anything else is an error).
///
/// This client currently sends only plaintext, but the server may return, negotiated via `grpc-accept-encoding`,
/// gzip-compressed responses; supported defensively to avoid parsing compressed binary as proto.
fn decompress_grpc_payload(flag: u8, payload: Vec<u8>) -> Result<Vec<u8>, ProtocolError> {
    match flag {
        0 => Ok(payload),
        1 => {
            let mut decoder = flate2::read::GzDecoder::new(&payload[..]);
            let mut out = Vec::with_capacity(payload.len());
            std::io::Read::read_to_end(&mut decoder, &mut out).map_err(|e| {
                ProtocolError::Codec(format!("gRPC gzip decompression failed: {e}"))
            })?;
            Ok(out)
        }
        other => Err(ProtocolError::Protocol(format!(
            "unsupported gRPC compression flag: {}",
            other
        ))),
    }
}

/// Decode the payload of a single gRPC frame: skip the 5-byte frame header (compression flag + big-endian length),
/// then decompress according to the compression flag. When the input is shorter than one complete frame it degenerates to returning the bytes after the frame header
/// (for backward compatibility).
pub fn decode_grpc_payload(frame: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    if frame.len() < 5 {
        return Ok(Vec::new());
    }
    let compressed = frame[0];
    let msg_len = u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]) as usize;
    if frame.len() < 5 + msg_len {
        return Ok(frame[5..].to_vec());
    }
    decompress_grpc_payload(compressed, frame[5..5 + msg_len].to_vec())
}

impl Default for GrpcClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProtocolClient for GrpcClient {
    fn name(&self) -> &str {
        "grpc"
    }

    fn description(&self) -> &str {
        "gRPC client (tonic + prost, supports unary/server/client/bidirectional streaming, reflection)"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn execute(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        let response_format = request.response_format.clone();
        let operation = request.operation.clone();
        match GrpcCallType::from_streaming_mode(request.streaming_mode) {
            GrpcCallType::Unary => self.execute_unary(request).await,
            call_type => {
                let streaming_resp = self
                    .execute_grpc_streaming(request, None, call_type)
                    .await?;

                let combined_payload: Vec<u8> = streaming_resp
                    .messages
                    .iter()
                    .flat_map(|m| {
                        let mut v = if response_format.as_deref() == Some("json") {
                            self.convert_proto_to_json(&operation, &m.payload)
                                .and_then(|j| serde_json::to_vec(&j).ok())
                                .unwrap_or_else(|| m.payload.clone())
                        } else {
                            m.payload.clone()
                        };
                        v.push(b'\n');
                        v
                    })
                    .collect();

                Ok(ProtocolResponse {
                    status_code: streaming_resp.status_code,
                    metadata: streaming_resp.metadata,
                    payload: combined_payload,
                    message_count: streaming_resp.message_count,
                    timings: ProtocolTimings {
                        dns: None,
                        tcp: None,
                        tls: None,
                        send: None,
                        first_byte: None,
                        receive: None,
                        total: streaming_resp.total_duration,
                    },
                })
            }
        }
    }

    async fn execute_streaming(
        &mut self,
        request: ProtocolRequest,
        client_messages: Option<MessageStream>,
    ) -> Result<StreamingResponse, ProtocolError> {
        let call_type = GrpcCallType::from_streaming_mode(request.streaming_mode);
        self.execute_grpc_streaming(request, client_messages, call_type)
            .await
    }

    fn clone_client(&self) -> Box<dyn ProtocolClient> {
        Box::new(Self::new())
    }
}

// ── JSON ↔ Protobuf conversion helper functions ────────────────

/// Parse operation into (service_name, method_name)
/// operation format: /package.Service/Method
#[cfg(feature = "grpc")]
fn split_operation(operation: &str) -> Result<(&str, &str), ProtocolError> {
    let parts: Vec<&str> = operation.trim_start_matches('/').split('/').collect();
    if parts.len() != 2 {
        return Err(ProtocolError::Protocol(format!(
            "Invalid gRPC operation format: {}",
            operation
        )));
    }
    Ok((parts[0], parts[1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ProtocolRequest, StreamingMode};

    #[test]
    fn test_grpc_client_creation() {
        let client = GrpcClient::new();
        assert_eq!(client.name(), "grpc");
        assert!(client.supports_streaming());
    }

    #[test]
    fn test_call_type_mapping() {
        assert_eq!(GrpcCallType::from_streaming_mode(None), GrpcCallType::Unary);
        assert_eq!(
            GrpcCallType::from_streaming_mode(Some(StreamingMode::ServerStreaming)),
            GrpcCallType::ServerStreaming
        );
        assert_eq!(
            GrpcCallType::from_streaming_mode(Some(StreamingMode::ClientStreaming)),
            GrpcCallType::ClientStreaming
        );
        assert_eq!(
            GrpcCallType::from_streaming_mode(Some(StreamingMode::Bidirectional)),
            GrpcCallType::Bidirectional
        );
    }

    #[tokio::test]
    async fn test_grpc_channel_creation() {
        let mut client = GrpcClient::new();
        let request = ProtocolRequest {
            target: "http://localhost:59999".into(),
            operation: "/test.Service/Method".into(),
            ..Default::default()
        };
        let result = client.execute(request).await;
        assert!(result.is_err());
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn test_split_operation() {
        assert_eq!(
            split_operation("/pkg.Svc/Method").unwrap(),
            ("pkg.Svc", "Method")
        );
        assert!(split_operation("invalid").is_err());
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn test_json_proto_dynamic_roundtrip() {
        use prost::Message as _;
        use prost_types::{
            field_descriptor_proto::{Label, Type},
            DescriptorProto, FieldDescriptorProto, FileDescriptorProto,
        };

        let field = FieldDescriptorProto {
            name: Some("name".into()),
            number: Some(1),
            r#type: Some(Type::String as i32),
            label: Some(Label::Optional as i32),
            ..Default::default()
        };
        let msg = DescriptorProto {
            name: Some("Greeting".into()),
            field: vec![field],
            ..Default::default()
        };
        let file = FileDescriptorProto {
            name: Some("demo.proto".into()),
            package: Some("demo".into()),
            message_type: vec![msg],
            ..Default::default()
        };

        let pool = DynamicProto::from_file_descriptors(&[file.encode_to_vec()]).unwrap();
        let proto_bytes = pool
            .encode_json("demo.Greeting", br#"{"name":"hello"}"#)
            .unwrap();
        let out = pool.decode_to_json("demo.Greeting", &proto_bytes).unwrap();
        assert_eq!(out["name"], "hello");
    }
}
