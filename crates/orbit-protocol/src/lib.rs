//! # orbit-protocol
//!
//! Extensible network protocol abstraction layer. The `ProtocolClient` trait unifies the client behavior
//! of every network protocol so the upper layers need not know the concrete protocol.
//!
//! ## Built-in protocols
//! - HTTP/1.1 + HTTP/2 (hyper + rustls)
//!
//! ## Protocol extensions
//! -  WebSocket, gRPC, TCP, UDP
//! -  SQL, Redis, MQTT (roadmap, not implemented yet)
//! - WASM plugins: arbitrary custom protocols

use serde::{Deserialize, Serialize};

/// Supported protocol types.
///
/// Used to select a protocol client per step in config, and by [`registry::build_client`] to build the matching client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProtocolKind {
    /// HTTP/1.1 + HTTP/2 (negotiates TLS/ALPN automatically based on the URL scheme)
    #[default]
    Http,
    /// Explicit HTTPS (behaves identically to `Http`; exists only for documentation clarity)
    Https,
    /// WebSocket
    WebSocket,
    /// gRPC（tonic + Server Reflection）
    Grpc,
    /// Raw TCP
    Tcp,
    /// Raw UDP
    Udp,
    /// Server-Sent Events
    Sse,
    /// GraphQL
    Graphql,
}

pub mod compression;
pub mod graphql;
#[cfg(feature = "grpc")]
pub mod grpc;
#[cfg(feature = "grpc")]
pub mod grpc_descriptor;
#[cfg(feature = "grpc")]
pub mod grpc_reflection;
pub mod guard;
pub mod http;
pub mod registry;
pub mod sse;
pub mod tcp;
pub mod traits;
pub mod types;
pub mod udp;
pub mod websocket;

pub use registry::build_client;
pub use traits::ProtocolClient;
pub use types::{
    FramingMode, FramingOptions, GrpcCallOptions, ProtocolError, ProtocolOptions, ProtocolRequest,
    ProtocolResponse, ProtocolTimings, TlsOptions, WsCallOptions, WsMessageType,
};
