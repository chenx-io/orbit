//! Core type definitions of the protocol abstraction layer

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Protocol-agnostic request description
///
/// Requests of all network protocols (HTTP/gRPC/SQL/WebSocket, etc.) are normalized into
/// this struct and parsed by the respective ProtocolClient implementations.
#[derive(Debug, Clone, Default)]
pub struct ProtocolRequest {
    /// Target address (e.g. `https://api.example.com/users` or `mysql://localhost/db`)
    pub target: String,

    /// Operation type (e.g. `GET`, `POST`, `SELECT`, `PUBLISH`)
    pub operation: String,

    /// Metadata (HTTP Headers / gRPC Metadata / SQL parameter bindings, etc.)
    pub metadata: Vec<(String, String)>,

    /// Request body (binary data, passed in after being encoded by the Codec layer)
    pub payload: Vec<u8>,

    /// Timeout configuration (None means use the default timeout)
    pub timeout: Option<Duration>,

    /// Streaming mode (None means a unary call)
    pub streaming_mode: Option<StreamingMode>,

    /// Request body format hint ("json", "protobuf", "msgpack", etc.)
    /// When it is "json" and the protocol is gRPC, the JSON is automatically converted to protobuf binary
    pub payload_format: Option<String>,

    /// Response deserialization format hint
    pub response_format: Option<String>,

    /// Protocol-specific structured options (TLS/framing/gRPC/WS, etc.)
    pub options: ProtocolOptions,

    /// Connection config (JSON string; from the collection's connectionConfigSchema value).
    /// Passed through to the WASM plugin for plugin protocols (read when the protocol id is a dynamic plugin).
    pub connection: Option<String>,
}

/// WebSocket message type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WsMessageType {
    #[default]
    Text,
    Binary,
}

/// Raw TCP framing mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FramingMode {
    /// Split by delimiter (e.g. "\r\n")
    Delimiter,
    /// Fixed-length frames
    Fixed,
    /// Read until the connection closes
    ReadUntilClose,
    /// Length-prefixed frames (the first N bytes are the length header, followed by the frame body)
    LengthPrefix,
}

/// Raw TCP framing options
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FramingOptions {
    pub mode: FramingMode,
    /// Delimiter for Delimiter mode (supports escapes such as "\n", "\r\n")
    pub delimiter: Option<String>,
    /// Frame length for Fixed mode / length-header byte count for LengthPrefix mode (default 4)
    pub fixed_len: Option<u32>,
    /// Length-header byte order for LengthPrefix mode (default big-endian)
    #[serde(default = "default_true")]
    pub big_endian: bool,
}

fn default_true() -> bool {
    true
}

impl Default for FramingOptions {
    fn default() -> Self {
        Self {
            mode: FramingMode::Delimiter,
            delimiter: None,
            fixed_len: None,
            big_endian: true,
        }
    }
}

/// TLS options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TlsOptions {
    /// Skip certificate verification (self-signed/intranet testing)
    pub insecure_skip_verify: bool,
    /// Custom CA certificate PEM
    pub ca_cert: Option<String>,
    /// Custom SNI
    pub sni: Option<String>,
}

/// gRPC call options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GrpcCallOptions {
    /// Request compression (gzip)
    pub compress: bool,
}

/// WebSocket call options
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WsCallOptions {
    pub message_type: WsMessageType,
    /// Close after receiving N messages
    pub close_after: Option<u32>,
}

/// Collection of protocol-specific options—each protocol client consumes only the sub-structures it cares about,
/// and unrecognized fields go into `extra` to preserve forward compatibility.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProtocolOptions {
    pub tls: Option<TlsOptions>,
    pub framing: Option<FramingOptions>,
    pub grpc: Option<GrpcCallOptions>,
    pub ws: Option<WsCallOptions>,
    pub extra: HashMap<String, serde_json::Value>,
}

/// Streaming call mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamingMode {
    /// Server streaming: the client sends one request and the server returns multiple responses
    ServerStreaming,
    /// Client streaming: the client sends multiple requests and the server returns one response
    ClientStreaming,
    /// Bidirectional streaming: the client and server can both send multiple messages
    Bidirectional,
}

/// Protocol-agnostic response
///
/// `Clone` is an explicit choice; callers should note the payload may be large (e.g. a big response body)
/// and clone only when necessary (assertions, retries, fan-out, etc.).
#[derive(Debug, Clone)]
pub struct ProtocolResponse {
    /// Status code (HTTP 200 / gRPC OK / SQL 0, etc., defined by each protocol)
    pub status_code: i32,

    /// Response metadata
    pub metadata: Vec<(String, String)>,

    /// Response body (binary data, decoded by the Codec layer)
    pub payload: Vec<u8>,

    /// Per-stage timings
    pub timings: ProtocolTimings,

    /// Number of messages received by streaming protocols (0 for non-streaming protocols)
    pub message_count: u64,
}

/// Streaming stream message
#[derive(Debug, Clone)]
pub struct StreamMessage {
    /// Message sequence number (starting from 0)
    pub index: u64,
    /// Message body
    pub payload: Vec<u8>,
    /// Receive duration of this message
    pub duration: Duration,
}

/// Summary response after Streaming completes
#[derive(Debug)]
pub struct StreamingResponse {
    /// Final status code
    pub status_code: i32,
    /// Response metadata
    pub metadata: Vec<(String, String)>,
    /// All stream messages
    pub messages: Vec<StreamMessage>,
    /// Total message count
    pub message_count: u64,
    /// Total duration
    pub total_duration: Duration,
}

/// Protocol-generic per-stage timings
///
/// Not all protocols support all stages—for example UDP has no connection setup and SQL has no TLS.
/// Each stage is an `Option<Duration>`; protocols that do not support a stage return `None`.
#[derive(Debug, Clone, Default)]
pub struct ProtocolTimings {
    /// DNS resolution duration
    pub dns: Option<Duration>,

    /// TCP connection setup duration (SYN → SYN-ACK → ACK)
    pub tcp: Option<Duration>,

    /// TLS handshake duration (ClientHello → ServerHello → ... → Finished)
    pub tls: Option<Duration>,

    /// Request send duration (first byte → last byte)
    pub send: Option<Duration>,

    /// Time to first byte (TTFB: send complete → response status line + headers received)
    pub first_byte: Option<Duration>,

    /// Duration to receive the complete response body
    pub receive: Option<Duration>,

    /// End-to-end total duration
    pub total: Duration,
}

impl std::fmt::Display for ProtocolTimings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn ms(d: Option<Duration>) -> String {
            d.map(|d| format!("{:.2}ms", d.as_secs_f64() * 1000.0))
                .unwrap_or_else(|| "N/A".to_string())
        }

        writeln!(f, "  ┌──────────────────────────────────────┐")?;
        writeln!(f, "  │  Phase          │    Time            │")?;
        writeln!(f, "  ├──────────────────────────────────────┤")?;
        writeln!(f, "  │  DNS Lookup     │  {:>16}  │", ms(self.dns))?;
        writeln!(f, "  │  TCP Connect    │  {:>16}  │", ms(self.tcp))?;
        writeln!(f, "  │  TLS Handshake  │  {:>16}  │", ms(self.tls))?;
        writeln!(f, "  │  Request Send   │  {:>16}  │", ms(self.send))?;
        writeln!(f, "  │  First Byte     │  {:>16}  │", ms(self.first_byte))?;
        writeln!(f, "  │  Receive Body   │  {:>16}  │", ms(self.receive))?;
        writeln!(f, "  ├──────────────────────────────────────┤")?;
        writeln!(
            f,
            "  │  Total          │  {:>16}  │",
            format!("{:.2}ms", self.total.as_secs_f64() * 1000.0)
        )?;
        writeln!(f, "  └──────────────────────────────────────┘")
    }
}

/// Protocol error type
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("connection failed: {0}")]
    Connect(String),

    #[error("send failed: {0}")]
    Send(String),

    #[error("receive failed: {0}")]
    Receive(String),

    #[error("timeout ({0:?})")]
    Timeout(Duration),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("codec error: {0}")]
    Codec(String),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("DNS resolution failed: {0}")]
    Dns(String),
}

impl ProtocolError {
    /// Error category key, used to group statistics by type in load-test reports.
    /// Kept consistent with orbit-metrics' `error_type` and the frontend `ErrorGroup.type`.
    pub fn category(&self) -> &'static str {
        match self {
            ProtocolError::Connect(_) => "connect",
            ProtocolError::Send(_) => "send",
            ProtocolError::Receive(_) => "receive",
            ProtocolError::Timeout(_) => "timeout",
            ProtocolError::Protocol(_) => "protocol",
            ProtocolError::Codec(_) => "codec",
            ProtocolError::Tls(_) => "tls",
            ProtocolError::Dns(_) => "dns",
        }
    }
}
