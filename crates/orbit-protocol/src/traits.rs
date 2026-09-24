//! Protocol client trait - the abstraction implemented by every network protocol

use async_trait::async_trait;
use futures_util::Stream;
use std::pin::Pin;

use crate::types::{
    ProtocolError, ProtocolRequest, ProtocolResponse, StreamMessage, StreamingResponse,
};

/// Streaming message stream type
pub type MessageStream = Pin<Box<dyn Stream<Item = Result<StreamMessage, ProtocolError>> + Send>>;

/// Protocol client - the trait every network protocol must implement
///
/// # Design notes
///
/// 1. **Cloneable**: each VU needs its own client instance (its own connection pool, Cookie Jar, etc.)
/// 2. **Send + Sync**: supports cross-thread concurrency
/// 3. **Connection lifecycle**: the three-stage connect/execute/disconnect; simple protocols may skip connect
/// 4. **Streaming support**: protocols such as gRPC streaming / SSE implement it via execute_streaming
///
/// # Example (HTTP)
///
/// ```ignore
/// use orbit_protocol::{ProtocolClient, ProtocolRequest};
/// use orbit_protocol::http::HttpClient;
///
/// let mut client = HttpClient::new();
/// let request = ProtocolRequest {
///     target: "https://httpbin.org/get".into(),
///     operation: "GET".into(),
///     metadata: vec![],
///     payload: vec![],
///     timeout: Some(std::time::Duration::from_secs(10)),
///     streaming_mode: None,
/// };
/// let response = client.execute(request).await?;
/// ```
#[async_trait]
pub trait ProtocolClient: Send + Sync {
    /// Protocol name (e.g. `"http"`, `"grpc"`, `"postgresql"`)
    fn name(&self) -> &str;

    /// Protocol description (optional)
    fn description(&self) -> &str {
        ""
    }

    /// Whether streaming is supported
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Establish a connection to the target (optional - some protocols have no connection concept, e.g. UDP).
    /// Long-lived sessions (WebSocket/TCP message sequences) use this interface to keep the connection alive.
    async fn connect(&mut self, _target: &str) -> Result<(), ProtocolError> {
        Ok(())
    }

    /// Send a message and receive the response on an established long-lived connection (not supported by default).
    /// Implemented by long-lived protocols such as WebSocket/TCP, used by message sequences.
    async fn send_recv(
        &mut self,
        _request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        Err(ProtocolError::Protocol(
            "send_recv not supported by this protocol client".into(),
        ))
    }

    /// Execute a request (unary mode)
    ///
    /// # Parameters
    /// - `request`: the protocol-agnostic request description
    ///
    /// # Returns
    /// - `Ok(ProtocolResponse)`: the request succeeded
    /// - `Err(ProtocolError)`: the request failed
    async fn execute(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError>;

    /// Execute a streaming request (server streaming / client streaming / bidirectional)
    ///
    /// The default implementation returns a "not supported" error. Protocols that do support streaming (gRPC, SSE) must override it.
    ///
    /// # Parameters
    /// - `request`: the initial request (with the streaming_mode marker)
    /// - `client_messages`: client stream messages (only for client_streaming / bidirectional)
    ///
    /// # Returns
    /// - An async stream in which every element is a message
    async fn execute_streaming(
        &mut self,
        request: ProtocolRequest,
        client_messages: Option<MessageStream>,
    ) -> Result<StreamingResponse, ProtocolError> {
        let _ = (request, client_messages);
        Err(ProtocolError::Protocol(
            "Streaming not supported by this protocol client".into(),
        ))
    }

    /// Close the connection (optional)
    async fn disconnect(&mut self) -> Result<(), ProtocolError> {
        Ok(())
    }

    /// Clone the client (used for VU concurrency)
    ///
    /// Each VU needs its own client instance because:
    /// - HTTP: an independent connection pool and Cookie Jar
    /// - gRPC: an independent gRPC Channel
    /// - SQL: an independent database connection
    fn clone_client(&self) -> Box<dyn ProtocolClient>;
}
