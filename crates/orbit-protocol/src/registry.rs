//! Protocol registry - builds the matching `ProtocolClient` for each protocol type.
//!
//! This is the core of the P0 "multi-protocol engine": FlowRunner no longer holds a single hard-coded client,
//! it takes a client from here on demand per step protocol type (and caches it for reuse inside FlowRunner).
//!
//! Dynamic registry (M3 WASM plugins): third-party plugins can register a string protocol id (e.g. "dubbo"),
//! which joins through `build_client_by_id` exactly like built-in protocols; conflicts with built-in ids are rejected.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use crate::traits::ProtocolClient;
use crate::ProtocolKind;

/// Dynamic protocol factory (a client constructor that clones from the registry)
pub type ProtocolFactory = Arc<dyn Fn() -> Box<dyn ProtocolClient> + Send + Sync>;

/// Built-in protocol ids (aligned with ProtocolKind::as_str), for catalog/menu display.
pub const BUILTIN_PROTOCOL_IDS: &[&str] =
    &["http", "websocket", "grpc", "tcp", "udp", "sse", "graphql"];

static DYNAMIC_PROTOCOLS: LazyLock<RwLock<HashMap<String, ProtocolFactory>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Build a client from a string protocol id: built-in enum first, dynamic (plugin) table as fallback
pub fn build_client_by_id(id: &str) -> Option<Box<dyn ProtocolClient>> {
    if let Some(kind) = ProtocolKind::parse(id) {
        return Some(build_client(kind));
    }
    DYNAMIC_PROTOCOLS.read().ok()?.get(id).map(|f| (f)())
}

/// Register a dynamic protocol (rejects conflicts with built-in ids; idempotently overwrites a plugin protocol of the same name)
pub fn register_protocol(id: &str, factory: ProtocolFactory) -> Result<(), String> {
    if ProtocolKind::parse(id).is_some() {
        return Err(format!("protocol '{}' conflicts with builtin", id));
    }
    DYNAMIC_PROTOCOLS
        .write()
        .map_err(|_| "registry lock poisoned".to_string())?
        .insert(id.to_string(), factory);
    Ok(())
}

/// Unregister a dynamic protocol
pub fn unregister_protocol(id: &str) {
    if let Ok(mut g) = DYNAMIC_PROTOCOLS.write() {
        g.remove(id);
    }
}

/// List all dynamic (plugin) protocol ids
pub fn list_protocol_ids() -> Vec<String> {
    DYNAMIC_PROTOCOLS
        .read()
        .map(|g| {
            let mut v: Vec<String> = g.keys().cloned().collect();
            v.sort();
            v
        })
        .unwrap_or_default()
}

/// Build a client instance for the given protocol type.
///
/// # Notes
/// - Both `Http` and `Https` return `HttpClient` (it negotiates TLS/ALPN internally based on the URL scheme).
/// - `Grpc` returns `GrpcClient` when the `grpc` feature is enabled, otherwise it safely falls back to `HttpClient`.
pub fn build_client(kind: ProtocolKind) -> Box<dyn ProtocolClient> {
    match kind {
        ProtocolKind::Http | ProtocolKind::Https => Box::new(crate::http::HttpClient::new()),
        ProtocolKind::WebSocket => Box::new(crate::websocket::WebSocketClient::new()),
        ProtocolKind::Tcp => Box::new(crate::tcp::TcpClient::new()),
        ProtocolKind::Udp => Box::new(crate::udp::UdpClient::new()),
        ProtocolKind::Sse => Box::new(crate::sse::SseClient::new()),
        ProtocolKind::Graphql => Box::new(crate::graphql::GraphqlClient::new()),
        ProtocolKind::Grpc => build_grpc_client(),
    }
}

#[cfg(feature = "grpc")]
fn build_grpc_client() -> Box<dyn ProtocolClient> {
    Box::new(crate::grpc::GrpcClient::new())
}

#[cfg(not(feature = "grpc"))]
fn build_grpc_client() -> Box<dyn ProtocolClient> {
    // Safe fallback when the gRPC feature is not enabled
    Box::new(crate::http::HttpClient::new())
}

impl ProtocolKind {
    /// Protocol id (string form, consistent with config/plugin protocol ids)
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Http | Self::Https => "http",
            Self::WebSocket => "websocket",
            Self::Grpc => "grpc",
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Sse => "sse",
            Self::Graphql => "graphql",
        }
    }

    /// Parse a protocol type from a string (case-insensitive).
    ///
    /// Supports common aliases: `ws` / `websocket`, `http` / `https`, etc.
    pub fn parse(name: &str) -> Option<ProtocolKind> {
        // Match with eq_ignore_ascii_case to avoid allocating a String via to_ascii_lowercase every time
        match name {
            s if s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https") => {
                Some(ProtocolKind::Http)
            }
            s if s.eq_ignore_ascii_case("ws") || s.eq_ignore_ascii_case("websocket") => {
                Some(ProtocolKind::WebSocket)
            }
            s if s.eq_ignore_ascii_case("grpc") => Some(ProtocolKind::Grpc),
            s if s.eq_ignore_ascii_case("tcp") => Some(ProtocolKind::Tcp),
            s if s.eq_ignore_ascii_case("udp") => Some(ProtocolKind::Udp),
            s if s.eq_ignore_ascii_case("sse") => Some(ProtocolKind::Sse),
            s if s.eq_ignore_ascii_case("graphql") => Some(ProtocolKind::Graphql),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_aliases() {
        assert_eq!(ProtocolKind::parse("http"), Some(ProtocolKind::Http));
        assert_eq!(ProtocolKind::parse("HTTPS"), Some(ProtocolKind::Http));
        assert_eq!(ProtocolKind::parse("ws"), Some(ProtocolKind::WebSocket));
        assert_eq!(
            ProtocolKind::parse("WebSocket"),
            Some(ProtocolKind::WebSocket)
        );
        assert_eq!(ProtocolKind::parse("grpc"), Some(ProtocolKind::Grpc));
        assert_eq!(ProtocolKind::parse("tcp"), Some(ProtocolKind::Tcp));
        assert_eq!(ProtocolKind::parse("sse"), Some(ProtocolKind::Sse));
        assert_eq!(ProtocolKind::parse("graphql"), Some(ProtocolKind::Graphql));
        assert_eq!(ProtocolKind::parse("nope"), None);
    }

    #[test]
    fn test_build_client_names() {
        // Verify that every protocol builds a client with the matching name
        assert_eq!(build_client(ProtocolKind::Http).name(), "http");
        assert_eq!(build_client(ProtocolKind::WebSocket).name(), "websocket");
        assert_eq!(build_client(ProtocolKind::Tcp).name(), "tcp");
        assert_eq!(build_client(ProtocolKind::Udp).name(), "udp");
        assert_eq!(build_client(ProtocolKind::Sse).name(), "sse");
        assert_eq!(build_client(ProtocolKind::Graphql).name(), "graphql");
        // Grpc is "grpc" with the grpc feature, and falls back to "http" when it is disabled
        #[cfg(feature = "grpc")]
        assert_eq!(build_client(ProtocolKind::Grpc).name(), "grpc");
    }

    #[test]
    fn test_dynamic_protocol_register_and_conflict() {
        let factory: ProtocolFactory = Arc::new(|| Box::new(crate::http::HttpClient::new()));

        // Conflict with a built-in id → rejected (M3 acceptance item)
        assert!(register_protocol("http", factory.clone()).is_err());
        assert!(register_protocol("websocket", factory.clone()).is_err());

        // Dynamic registration succeeds and the client can be built by string id
        assert!(register_protocol("dubbo", factory.clone()).is_ok());
        let client = build_client_by_id("dubbo").expect("dynamic protocol client");
        assert_eq!(client.name(), "http");
        assert_eq!(list_protocol_ids(), vec!["dubbo"]);

        // Cannot be built after unregistering
        unregister_protocol("dubbo");
        assert!(build_client_by_id("dubbo").is_none());
        assert!(list_protocol_ids().is_empty());
    }
}
