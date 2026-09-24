//! Streaming HTTP transport: on top of `orbit-protocol`'s hand-rolled HTTP stack (rustls ring + connection pool + phased timing)
//! it does SSE incremental parsing for the Provider adapter layer to consume.
//!
//! Why not bring in reqwest / `eventsource-stream`:
//! - the project's protocol layer is entirely hand-written (not reqwest), so reusing it avoids a second TLS stack (aws-lc-rs has build risks on Windows);
//! - SSE only needs line-by-line parsing, and a leftover buffer suffices across chunk boundaries, so no extra dependency is needed.
//!
//! Cancellation: `stream_response` itself does not support cancellation, so the connect phase wraps it in a `select!`,
//! and the read-stream phase `select!`s per frame, ensuring that clicking "stop" interrupts immediately (rather than waiting for the model to finish speaking).

use std::time::Duration;

use http_body_util::BodyExt;
use orbit_protocol::http::HttpClient;
use orbit_protocol::types::ProtocolRequest;
use tokio_util::sync::CancellationToken;

use crate::error::{AiError, AiResult};

/// Connect + first-byte timeout (streaming requests are bounded only until the response headers return).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

/// Read-stream idle timeout: no new data for this long is treated as a broken connection.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// A single SSE event (keeps only `event` / `data`, which are the only fields OpenAI and Anthropic use).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field (used by Anthropic; always `None` for OpenAI).
    pub event: Option<String>,
    /// The `data:` field (multiple `data:` lines are joined with `\n`, per the SSE spec).
    pub data: String,
}

impl SseEvent {
    /// Whether `data` is the stream-end marker (OpenAI uses `[DONE]`).
    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }
}

/// SSE incremental parse buffer: feed in arbitrarily split byte chunks, get complete events out.
///
/// Key points:
/// - a half event spanning chunks stays in the buffer for the next chunk (network fragments do not align with event boundaries);
/// - the consumed prefix is reclaimed by threshold, so memory does not grow with stream length.
#[derive(Debug, Default)]
pub struct SseBuffer {
    buf: Vec<u8>,
    /// Offset scanned so far (avoids the O(n²) moves caused by draining per line).
    offset: usize,
    /// The `event:` name accumulated for the current event.
    pending_event: Option<String>,
    /// The `data:` accumulated for the current event (multiple lines joined with `\n`).
    pending_data: String,
    /// Whether a `data:` line appeared (distinguishes "empty data" from "event only, no data").
    has_data: bool,
}

impl SseBuffer {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one chunk of network data.
    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// Take out all currently **complete** events (an incomplete part stays in the buffer).
    pub fn drain_events(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        while let Some(rel) = self.buf[self.offset..].iter().position(|&b| b == b'\n') {
            let line_end = self.offset + rel;
            let line = String::from_utf8_lossy(&self.buf[self.offset..line_end])
                .trim_end_matches('\r')
                .to_string();
            self.offset = line_end + 1;
            self.consume_line(&line, &mut out);
        }
        self.compact();
        out
    }

    /// When the stream ends normally, emit the **last** event in the buffer that is not terminated by a blank line.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if self.offset < self.buf.len() {
            let tail = String::from_utf8_lossy(&self.buf[self.offset..]).to_string();
            self.offset = self.buf.len();
            for line in tail.lines() {
                self.consume_line(line.trim_end_matches('\r'), &mut out);
            }
        }
        if self.has_data {
            self.dispatch(&mut out);
        }
        out
    }

    /// Process a single line of SSE text.
    fn consume_line(&mut self, line: &str, out: &mut Vec<SseEvent>) {
        if line.is_empty() {
            // A blank line = end of event
            if self.has_data {
                self.dispatch(out);
            } else {
                self.pending_event = None;
            }
            return;
        }
        if line.starts_with(':') {
            // Comment line (some services use it for keep-alive)
            return;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => self.pending_event = Some(value.to_string()),
            "data" => {
                if self.has_data {
                    self.pending_data.push('\n');
                }
                self.pending_data.push_str(value);
                self.has_data = true;
            }
            // id / retry are useless here, ignore them
            _ => {}
        }
    }

    /// Dispatch the currently accumulated event and reset.
    fn dispatch(&mut self, out: &mut Vec<SseEvent>) {
        out.push(SseEvent {
            event: self.pending_event.take(),
            data: std::mem::take(&mut self.pending_data),
        });
        self.has_data = false;
    }

    /// Reclaim the consumed prefix (8KB threshold, to avoid moving on every line).
    fn compact(&mut self) {
        if self.offset > 8 * 1024 {
            self.buf.drain(..self.offset);
            self.offset = 0;
        }
    }
}

/// An established SSE response stream currently being read.
pub struct SseStream {
    body: hyper::body::Incoming,
    sse: SseBuffer,
    /// Parsed events waiting to be dispatched (one network frame may contain several).
    pending: Vec<SseEvent>,
    done: bool,
}

impl SseStream {
    /// Read the next SSE event; returns `None` when the stream ends.
    ///
    /// Per-frame `select!` for cancellation + idle timeout, ensuring "stop generating" takes effect immediately.
    pub async fn next_event(&mut self, cancel: &CancellationToken) -> AiResult<Option<SseEvent>> {
        loop {
            if !self.pending.is_empty() {
                return Ok(Some(self.pending.remove(0)));
            }

            if self.done {
                return Ok(None);
            }
            let frame = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(AiError::Cancelled),
                r = tokio::time::timeout(IDLE_TIMEOUT, self.body.frame()) => r,
            };
            match frame {
                Err(_) => {
                    return Err(AiError::Transport(format!(
                        "idle timeout while reading the model stream ({}s)",
                        IDLE_TIMEOUT.as_secs()
                    )))
                }
                Ok(None) => {
                    self.done = true;
                    self.pending.extend(self.sse.finish());
                }
                Ok(Some(Err(e))) => {
                    return Err(AiError::Transport(format!(
                        "failed to read model stream: {e}"
                    )))
                }
                Ok(Some(Ok(f))) => {
                    if let Some(data) = f.data_ref() {
                        self.sse.push(data);
                    }
                    self.pending.extend(self.sse.drain_events());
                }
            }
        }
    }

    /// Read all remaining raw bytes (only for non-SSE error response bodies; on failure returns what was read so far).
    pub async fn read_raw_body(&mut self, cancel: &CancellationToken) -> Vec<u8> {
        let mut out = Vec::new();
        while !self.done {
            let frame = tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                r = tokio::time::timeout(IDLE_TIMEOUT, self.body.frame()) => r,
            };
            match frame {
                Err(_) | Ok(None) => self.done = true,
                Ok(Some(Err(_))) => self.done = true,
                Ok(Some(Ok(f))) => {
                    if let Some(data) = f.data_ref() {
                        out.extend_from_slice(data);
                    }
                }
            }
        }
        out
    }
}

/// Issue a streaming POST request, returning the status code, response headers, and the SSE stream.
pub async fn post_sse(
    url: &str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    cancel: &CancellationToken,
) -> AiResult<(i32, Vec<(String, String)>, SseStream)> {
    let mut client = HttpClient::new();
    let request = ProtocolRequest {
        target: url.to_string(),
        operation: "POST".to_string(),
        metadata: headers,
        payload: body,
        timeout: Some(CONNECT_TIMEOUT),
        ..Default::default()
    };

    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(AiError::Cancelled),
        r = tokio::time::timeout(CONNECT_TIMEOUT, client.stream_response(request)) => r,
    };

    let (_timings, status, resp_headers, body) = match result {
        Err(_) => {
            return Err(AiError::Transport(format!(
                "timeout connecting to the model service ({}s)",
                CONNECT_TIMEOUT.as_secs()
            )))
        }
        Ok(Err(e)) => return Err(AiError::Transport(e.to_string())),
        Ok(Ok(v)) => v,
    };

    let mut stream = SseStream {
        body,
        sse: SseBuffer::new(),
        pending: Vec::new(),
        done: false,
    };

    // For non-2xx the body is not SSE, so read the raw text as the error message (so users can see the server's real reason)
    if !(200..300).contains(&status) {
        let raw = stream.read_raw_body(cancel).await;
        let text = String::from_utf8_lossy(&raw).trim().to_string();
        return Err(AiError::Provider {
            status,
            body: truncate(&text, 800),
        });
    }

    Ok((status, resp_headers, stream))
}

/// Truncate overly long text (keep the head + an elision marker).
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}…(truncated)")
}

/// Issue a one-shot (non-streaming) POST request, returning the status code, response headers, and the full response body.
///
/// Used for small requests such as "test connection"; all model conversation goes through [`post_sse`].
pub async fn post_json_once(
    url: &str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    timeout: Duration,
) -> AiResult<(i32, Vec<(String, String)>, Vec<u8>)> {
    use orbit_protocol::traits::ProtocolClient;

    let mut client = HttpClient::new();
    let request = ProtocolRequest {
        target: url.to_string(),
        operation: "POST".to_string(),
        metadata: headers,
        payload: body,
        timeout: Some(timeout),
        ..Default::default()
    };
    let resp = match tokio::time::timeout(timeout, client.execute(request)).await {
        Err(_) => return Err(AiError::Transport("request timed out".to_string())),
        Ok(Err(e)) => return Err(AiError::Transport(e.to_string())),
        Ok(Ok(v)) => v,
    };
    Ok((resp.status_code, resp.metadata, resp.payload))
}

/// Issue a one-shot GET request (for read-only endpoints such as fetching the model list).
pub async fn get_json_once(
    url: &str,
    headers: Vec<(String, String)>,
    timeout: Duration,
) -> AiResult<(i32, Vec<(String, String)>, Vec<u8>)> {
    use orbit_protocol::traits::ProtocolClient;

    let mut client = HttpClient::new();
    let request = ProtocolRequest {
        target: url.to_string(),
        operation: "GET".to_string(),
        metadata: headers,
        payload: Vec::new(),
        timeout: Some(timeout),
        ..Default::default()
    };
    let resp = match tokio::time::timeout(timeout, client.execute(request)).await {
        Err(_) => return Err(AiError::Transport("request timed out".to_string())),
        Ok(Err(e)) => return Err(AiError::Transport(e.to_string())),
        Ok(Ok(v)) => v,
    };
    Ok((resp.status_code, resp.metadata, resp.payload))
}

/// Whether the status code indicates success.
pub fn is_success(status: i32) -> bool {
    (200..300).contains(&status)
}

/// Read a header from the response header list (case-insensitive).
pub fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(buf: &mut SseBuffer) -> Vec<SseEvent> {
        buf.drain_events()
    }

    #[test]
    fn parses_single_event_from_one_chunk() {
        let mut buf = SseBuffer::new();
        buf.push(b"data: {\"a\":1}\n\n");
        assert_eq!(
            events(&mut buf),
            vec![SseEvent {
                event: None,
                data: "{\"a\":1}".into()
            }]
        );
    }

    #[test]
    fn waits_for_event_boundary_across_chunks() {
        let mut buf = SseBuffer::new();
        buf.push(b"data: {\"a\"");
        assert!(
            events(&mut buf).is_empty(),
            "a half event should not be dispatched early"
        );
        buf.push(b":1}\n\n");
        assert_eq!(events(&mut buf).len(), 1);
    }

    #[test]
    fn keeps_partial_line_in_buffer_when_no_newline() {
        let mut buf = SseBuffer::new();
        buf.push(b"data: abc");
        assert!(events(&mut buf).is_empty());
        buf.push(b"def\n\n");
        assert_eq!(events(&mut buf)[0].data, "abcdef");
    }

    #[test]
    fn joins_multiple_data_lines_with_newline() {
        let mut buf = SseBuffer::new();
        buf.push(b"data: line1\ndata: line2\n\n");
        assert_eq!(events(&mut buf)[0].data, "line1\nline2");
    }

    #[test]
    fn captures_event_name_for_anthropic() {
        let mut buf = SseBuffer::new();
        buf.push(b"event: content_block_delta\ndata: {\"i\":0}\n\n");
        let ev = &events(&mut buf)[0];
        assert_eq!(ev.event.as_deref(), Some("content_block_delta"));
        assert_eq!(ev.data, "{\"i\":0}");
    }

    #[test]
    fn ignores_comment_lines_and_retry_fields() {
        let mut buf = SseBuffer::new();
        buf.push(b": keep-alive\nretry: 1000\n\ndata: x\n\n");
        let out = events(&mut buf);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].data, "x");
    }

    #[test]
    fn parses_multiple_events_in_one_chunk() {
        let mut buf = SseBuffer::new();
        buf.push(b"data: 1\n\ndata: 2\n\ndata: 3\n\n");
        let out = events(&mut buf);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].data, "3");
    }

    #[test]
    fn treats_empty_data_as_absent_event() {
        // A block with only an event name and no data should not be dispatched (the SSE spec requires data)
        let mut buf = SseBuffer::new();
        buf.push(b"event: ping\n\n");
        assert!(events(&mut buf).is_empty());
    }

    #[test]
    fn finish_flushes_trailing_event_without_blank_line() {
        let mut buf = SseBuffer::new();
        buf.push(b"data: last");
        assert!(events(&mut buf).is_empty());
        let out = buf.finish();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].data, "last");
    }

    #[test]
    fn detects_done_marker() {
        let mut buf = SseBuffer::new();
        buf.push(b"data: [DONE]\n\n");
        assert!(events(&mut buf)[0].is_done());
    }

    #[test]
    fn crlf_line_endings_are_supported() {
        let mut buf = SseBuffer::new();
        buf.push(b"data: {\"a\":1}\r\n\r\n");
        assert_eq!(events(&mut buf)[0].data, "{\"a\":1}");
    }

    #[test]
    fn keeps_preserving_inner_spaces_after_colon() {
        let mut buf = SseBuffer::new();
        buf.push(b"data:  two spaces\n\n");
        assert_eq!(events(&mut buf)[0].data, " two spaces");
    }
}
