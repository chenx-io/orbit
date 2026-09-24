//! SSE (Server-Sent Events) protocol client
//!
//! Built on the shared `HttpClient` (connection pool / TLS / phased timing), consuming a streaming response body
//! and parsing the `text/event-stream` format. SSE is a one-way stream (server to client),
//! and each `execute` returns the accumulated event data.

use async_trait::async_trait;
use http_body_util::BodyExt;
use std::time::Instant;

use crate::http::HttpClient;
use crate::traits::ProtocolClient;
use crate::types::{ProtocolError, ProtocolRequest, ProtocolResponse};

/// SSE event
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub id: Option<String>,
    pub event: Option<String>,
    pub data: String,
    pub retry: Option<u64>,
}

/// SSE protocol client (reuses the HTTP transport stack)
pub struct SseClient {
    http: HttpClient,
}

impl SseClient {
    pub fn new() -> Self {
        Self {
            http: HttpClient::new(),
        }
    }
}

impl Default for SseClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProtocolClient for SseClient {
    fn name(&self) -> &str {
        "sse"
    }
    fn description(&self) -> &str {
        "Server-Sent Events client (shared HTTP transport)"
    }

    async fn execute(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        let total_start = Instant::now();

        let mut req = request;
        req.operation = "GET".to_string();
        // Maximum number of events to collect: prefer options.extra.max_events, default 50
        let max_events: usize = req
            .options
            .extra
            .get("max_events")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(50);
        let (mut timings, status, resp_headers, body) = self.http.stream_response(req).await?;

        // Collect SSE events
        let receive_start = Instant::now();
        let mut stream = body;
        let mut events = Vec::new();
        let mut buffer: Vec<u8> = Vec::new();
        // Consumed byte offset: line scanning starts at read_offset, avoiding a String allocation per line (O(n²))
        let mut read_offset = 0usize;
        let mut current_event = SseEvent {
            id: None,
            event: None,
            data: String::new(),
            retry: None,
        };
        let mut event_count = 0;
        while let Some(frame) = stream.frame().await {
            match frame {
                Ok(f) => {
                    if let Some(data) = f.data_ref() {
                        buffer.extend_from_slice(data);
                    }
                }
                Err(e) => {
                    return Err(ProtocolError::Receive(format!("SSE stream: {}", e)));
                }
            }

            // Parse SSE lines (process every line that is already complete)
            while let Some(rel) = buffer[read_offset..].iter().position(|&b| b == b'\n') {
                let line_end = read_offset + rel;
                let line = String::from_utf8_lossy(&buffer[read_offset..line_end])
                    .trim_end_matches('\r')
                    .to_string();
                read_offset = line_end + 1;

                if line.is_empty() {
                    // Empty line = end of event
                    if !current_event.data.is_empty() {
                        events.push(current_event.clone());
                        event_count += 1;
                        current_event = SseEvent {
                            id: None,
                            event: None,
                            data: String::new(),
                            retry: None,
                        };
                        if event_count >= max_events {
                            break;
                        }
                    }
                } else if let Some(value) = line.strip_prefix("id:") {
                    current_event.id = Some(value.trim().to_string());
                } else if let Some(value) = line.strip_prefix("event:") {
                    current_event.event = Some(value.trim().to_string());
                } else if let Some(value) = line.strip_prefix("data:") {
                    if !current_event.data.is_empty() {
                        current_event.data.push('\n');
                    }
                    // SSE spec: strip a single leading space after the colon; other spaces (inner/trailing) are preserved
                    let value = value.strip_prefix(' ').unwrap_or(value);
                    current_event.data.push_str(value);
                } else if let Some(value) = line.strip_prefix("retry:") {
                    current_event.retry = value.trim().parse().ok();
                }
                // Ignore comment lines (those starting with :)
            }

            // Periodically reclaim the consumed buffer to avoid unbounded growth on long streams
            if read_offset > 64 * 1024 {
                buffer.drain(..read_offset);
                read_offset = 0;
            }

            if event_count >= max_events {
                break;
            }
        }

        // Handle the last, unfinished event
        if !current_event.data.is_empty() {
            events.push(current_event);
        }

        timings.receive = Some(receive_start.elapsed());
        timings.total = total_start.elapsed();

        // Serialize events as JSON
        let payload = serde_json::to_vec(&serde_json::json!({
            "protocol": "sse",
            "event_count": events.len(),
            "events": events.iter().map(|e| serde_json::json!({
                "id": e.id,
                "event": e.event,
                "data": e.data,
                "retry": e.retry,
            })).collect::<Vec<_>>(),
        }))
        .unwrap_or_default();

        let mut metadata = resp_headers;
        metadata.push(("protocol".into(), "sse".into()));
        metadata.push(("event_count".into(), events.len().to_string()));

        Ok(ProtocolResponse {
            status_code: status,
            metadata,
            payload,
            message_count: events.len() as u64,
            timings,
        })
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
    async fn test_sse_basic() {
        let mut client = SseClient::new();
        let request = ProtocolRequest {
            target: "https://httpbin.org/get".into(),
            operation: "GET".into(),
            metadata: vec![("Accept".into(), "text/event-stream".into())],
            payload: vec![],
            timeout: Some(Duration::from_secs(10)),
            streaming_mode: None,
            payload_format: None,
            response_format: None,
            options: Default::default(),
            connection: None,
        };
        let resp = client.execute(request).await.expect("SSE request failed");
        assert!(resp.status_code == 200 || resp.status_code == 503);
        assert!(!resp.payload.is_empty());
    }
}
