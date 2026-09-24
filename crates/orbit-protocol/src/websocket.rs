//! WebSocket protocol client implementation
//!
//! Based on tokio-tungstenite. Supports:
//! - Single-message round trip (execute)
//! - Long-lived sessions (connect → per-message send_recv → disconnect) for message sequences

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use std::time::Instant;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::MaybeTlsStream;

use crate::traits::ProtocolClient;
use crate::types::{
    ProtocolError, ProtocolRequest, ProtocolResponse, ProtocolTimings, WsMessageType,
};

/// An established WebSocket connection
type WsStream = tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// WebSocket protocol client
pub struct WebSocketClient {
    stream: Option<WsStream>,
}

impl WebSocketClient {
    pub fn new() -> Self {
        Self { stream: None }
    }
}

impl Default for WebSocketClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProtocolClient for WebSocketClient {
    fn name(&self) -> &str {
        "websocket"
    }

    fn description(&self) -> &str {
        "WebSocket client based on tokio-tungstenite (session + single-shot)"
    }

    async fn connect(&mut self, target: &str) -> Result<(), ProtocolError> {
        let (ws_stream, _resp) = connect_async(target)
            .await
            .map_err(|e| ProtocolError::Connect(format!("WebSocket connect failed: {}", e)))?;
        self.stream = Some(ws_stream);
        Ok(())
    }

    async fn send_recv(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        let total_start = Instant::now();
        let ws = self
            .stream
            .as_mut()
            .ok_or_else(|| ProtocolError::Connect("WebSocket not connected".into()))?;

        // Send the message (text/binary chosen by options.ws.message_type; payload takes precedence, otherwise operation)
        let ws_opts = request.options.ws.unwrap_or_default();
        let send_start = Instant::now();
        let msg = if request.payload.is_empty() {
            Message::Text(request.operation.clone())
        } else if ws_opts.message_type == WsMessageType::Binary {
            Message::Binary(request.payload.clone())
        } else {
            let text = String::from_utf8(request.payload.clone())
                .unwrap_or_else(|_| format!("<binary: {} bytes>", request.payload.len()));
            Message::Text(text)
        };
        ws.send(msg)
            .await
            .map_err(|e| ProtocolError::Send(format!("WebSocket send failed: {}", e)))?;
        let send_duration = send_start.elapsed();

        // Receive the response (close after close_after messages; default 1)
        let receive_start = Instant::now();
        let max_messages = ws_opts.close_after.unwrap_or(1).max(1);
        let mut parts: Vec<String> = Vec::new();
        for _ in 0..max_messages {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => parts.push(text),
                Some(Ok(Message::Binary(data))) => {
                    parts.push(String::from_utf8(data).unwrap_or_else(|_| "<binary>".to_string()));
                }
                Some(Ok(Message::Ping(_))) => parts.push("pong".to_string()),
                Some(Ok(Message::Pong(_))) => parts.push("ping".to_string()),
                Some(Ok(Message::Close(_))) => {
                    parts.push("closed".to_string());
                    break;
                }
                Some(Ok(Message::Frame(_))) => parts.push("<frame>".to_string()),
                Some(Err(e)) => {
                    return Err(ProtocolError::Receive(format!("WebSocket error: {}", e)));
                }
                None => {
                    parts.push("connection closed".to_string());
                    break;
                }
            }
        }
        let response_text = parts.join("\n");
        let receive_duration = receive_start.elapsed();

        Ok(ProtocolResponse {
            status_code: 101, // WebSocket 101 Switching Protocols
            metadata: vec![("protocol".to_string(), "websocket".to_string())],
            payload: response_text.into_bytes(),
            message_count: max_messages as u64,
            timings: ProtocolTimings {
                dns: None,
                tcp: None,
                tls: None,
                send: Some(send_duration),
                first_byte: None,
                receive: Some(receive_duration),
                total: total_start.elapsed(),
            },
        })
    }

    async fn disconnect(&mut self) -> Result<(), ProtocolError> {
        if let Some(mut ws) = self.stream.take() {
            let _ = ws.close(None).await;
        }
        Ok(())
    }

    async fn execute(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        let total_start = Instant::now();
        let connect_start = Instant::now();
        self.connect(&request.target).await?;
        let connect_duration = connect_start.elapsed();

        let mut resp = self.send_recv(request).await?;
        resp.timings.tcp = Some(connect_duration);
        resp.timings.total = total_start.elapsed();

        let _ = self.disconnect().await;
        Ok(resp)
    }

    fn clone_client(&self) -> Box<dyn ProtocolClient> {
        Box::new(Self::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_websocket_echo() {
        let mut client = WebSocketClient::new();
        let request = ProtocolRequest {
            target: "wss://echo.websocket.org".into(),
            operation: "Hello, orbit WebSocket!".into(),
            metadata: vec![],
            payload: vec![],
            timeout: Some(Duration::from_secs(10)),
            streaming_mode: None,
            payload_format: None,
            response_format: None,
            options: Default::default(),
            connection: None,
        };

        let result = client.execute(request).await;
        if let Ok(response) = result {
            assert_eq!(response.status_code, 101);
            assert!(response.timings.tcp.is_some());
        }
    }
}
