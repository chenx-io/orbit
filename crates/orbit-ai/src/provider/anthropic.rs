//! Anthropic Messages Provider：`POST {base}/messages`（`stream: true`）。
//!
//! Differences from OpenAI:
//! - `system` is a top-level field and does not go into `messages`;
//! - tool calls are `tool_use` blocks inside `content`, and tool results are `tool_result` blocks
//!   inside a **user message** (and must come first among that user message's content blocks);
//! - streaming events carry a `type`: `message_start` / `content_block_start` /
//!   `content_block_delta` / `message_delta` / `message_stop`。

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::error::{AiError, AiResult};
use crate::message::{
    ChatMessage, ProviderDelta, ProviderTurnEnd, Role, StopReason, StopReasonRaw, Usage,
};
use crate::provider::{
    DeltaSink, Provider, ProviderConfig, ProviderKind, ToolCallAccumulator, TurnRequest,
};
use crate::transport::{post_sse, truncate};

/// Anthropic API version header (required for tool calls).
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic Provider。
pub struct AnthropicProvider {
    config: ProviderConfig,
}

impl AnthropicProvider {
    /// Build from the config.
    pub fn new(config: ProviderConfig) -> Self {
        Self { config }
    }

    fn headers(&self) -> Vec<(String, String)> {
        // One of two auth headers: `x-api-key` (official API key) or `Authorization: Bearer`
        // (OAuth auth token / a gateway that accepts only Bearer)-corresponding to the official SDK's apiKey / authToken
        let (auth_name, auth_value) = super::auth_header(
            super::ProviderKind::Anthropic,
            &self.config.auth_style,
            &self.config.api_key,
        );
        super::merge_custom_headers(
            vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Accept".to_string(), "text/event-stream".to_string()),
                (auth_name, auth_value),
                (
                    "anthropic-version".to_string(),
                    ANTHROPIC_VERSION.to_string(),
                ),
            ],
            &self.config.headers,
        )
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    async fn stream_turn(
        &self,
        req: TurnRequest<'_>,
        sink: DeltaSink,
        cancel: CancellationToken,
    ) -> AiResult<ProviderTurnEnd> {
        let url = format!("{}/messages", self.config.normalized_base());
        let body = serde_json::to_vec(&build_body(&req))?;
        let (_status, _headers, mut stream) = post_sse(&url, self.headers(), body, &cancel).await?;

        let mut state = AnthropicStreamState::default();
        while let Some(ev) = stream.next_event(&cancel).await? {
            if ev.data.trim().is_empty() {
                continue;
            }
            for delta in state.apply(&ev.data)? {
                let _ = sink.send(delta);
            }
        }
        Ok(state.finish())
    }
}

/// Build the request body.
pub fn build_body(req: &TurnRequest<'_>) -> Value {
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "temperature": req.temperature,
        "messages": to_messages(req.messages),
        "stream": true,
    });
    if !req.system.trim().is_empty() {
        body["system"] = json!(req.system);
    }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.iter().map(|t| t.to_anthropic()).collect());
    }
    body
}

/// IR → Anthropic `messages`。
///
/// Consecutive Tool messages are merged into the same user message (Anthropic requires a turn's tool results
/// to be gathered into one user turn, and `tool_result` to precede other content).
pub fn to_messages(messages: &[ChatMessage]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut pending_results: Vec<Value> = Vec::new();

    fn flush(out: &mut Vec<Value>, results: &mut Vec<Value>) {
        if !results.is_empty() {
            out.push(json!({ "role": "user", "content": std::mem::take(results) }));
        }
    }

    for m in messages {
        match m.role {
            Role::System => continue,
            Role::Tool => {
                pending_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.text,
                    "is_error": !m.tool_ok.unwrap_or(true),
                }));
            }
            Role::User => {
                flush(&mut out, &mut pending_results);
                out.push(json!({
                    "role": "user",
                    "content": [{ "type": "text", "text": m.text }],
                }));
            }
            Role::Assistant => {
                flush(&mut out, &mut pending_results);
                let mut blocks: Vec<Value> = Vec::new();
                if !m.text.is_empty() {
                    blocks.push(json!({ "type": "text", "text": m.text }));
                }
                for tc in &m.tool_calls {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    }));
                }
                // An empty turn (no text, no tool calls) would serialize to an empty content array, which Anthropic also rejects
                // (`text content blocks must be non-empty`). Same strategy as the OpenAI side: drop it.
                if blocks.is_empty() {
                    continue;
                }
                out.push(json!({ "role": "assistant", "content": blocks }));
            }
        }
    }
    flush(&mut out, &mut pending_results);
    out
}

/// Anthropic streaming parse state machine.
#[derive(Debug, Default)]
pub struct AnthropicStreamState {
    acc: ToolCallAccumulator,
    stop_reason: Option<String>,
    usage: Usage,
}

impl AnthropicStreamState {
    /// Process one `data:` JSON frame.
    pub fn apply(&mut self, data: &str) -> AiResult<Vec<ProviderDelta>> {
        let value: Value = serde_json::from_str(data).map_err(|e| {
            AiError::Transport(format!(
                "failed to parse model stream: {e} (raw: {})",
                truncate(data, 200)
            ))
        })?;

        let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let mut deltas = Vec::new();

        match kind {
            "error" => {
                let msg = value
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error")
                    .to_string();
                return Err(AiError::Stream(msg));
            }
            "message_start" => {
                if let Some(u) = value
                    .get("message")
                    .and_then(|m| m.get("usage"))
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(|v| v.as_u64())
                {
                    self.usage.input_tokens = u;
                }
            }
            "message_delta" => {
                if let Some(reason) = value
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|v| v.as_str())
                {
                    self.stop_reason = Some(reason.to_string());
                }
                if let Some(o) = value
                    .get("usage")
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(|v| v.as_u64())
                {
                    self.usage.output_tokens = o;
                }
                deltas.push(ProviderDelta::Usage(self.usage));
            }
            "content_block_start" => {
                let index = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                if let Some(block) = value.get("content_block") {
                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                        let id = block.get("id").and_then(|v| v.as_str());
                        let name = block.get("name").and_then(|v| v.as_str());
                        self.acc.push(index, id, name, "");
                        deltas.push(ProviderDelta::ToolCallDelta {
                            index,
                            id: id.map(|s| s.to_string()),
                            name: name.map(|s| s.to_string()),
                            arguments_delta: String::new(),
                        });
                    }
                }
            }
            "content_block_delta" => {
                let index = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let delta = value.get("delta");
                let delta_type = delta
                    .and_then(|d| d.get("type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match delta_type {
                    "text_delta" => {
                        if let Some(t) = delta.and_then(|d| d.get("text")).and_then(|v| v.as_str())
                        {
                            deltas.push(ProviderDelta::Text(t.to_string()));
                        }
                    }
                    "thinking_delta" => {
                        if let Some(t) = delta
                            .and_then(|d| d.get("thinking"))
                            .and_then(|v| v.as_str())
                        {
                            deltas.push(ProviderDelta::Reasoning(t.to_string()));
                        }
                    }
                    "input_json_delta" => {
                        let partial = delta
                            .and_then(|d| d.get("partial_json"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        self.acc.push(index, None, None, partial);
                        deltas.push(ProviderDelta::ToolCallDelta {
                            index,
                            id: None,
                            name: None,
                            arguments_delta: partial.to_string(),
                        });
                    }
                    _ => {}
                }
            }
            // ping / content_block_stop / message_stop need no handling
            _ => {}
        }

        Ok(deltas)
    }

    /// Finish.
    pub fn finish(self) -> ProviderTurnEnd {
        let tool_calls = self.acc.finish();
        let raw = StopReasonRaw(self.stop_reason);
        let stop_reason = if !tool_calls.is_empty() && raw.normalized() != StopReason::ToolUse {
            StopReasonRaw(Some("tool_use".to_string()))
        } else {
            raw
        };
        ProviderTurnEnd {
            tool_calls,
            stop_reason,
            usage: self.usage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ToolCall;
    use crate::tools::{ToolKind, ToolSpec};

    fn spec() -> ToolSpec {
        ToolSpec {
            name: "create_scenario".into(),
            kind: ToolKind::Write,
            description: "create a new case".into(),
            parameters: json!({"type":"object","properties":{}}),
        }
    }

    #[test]
    fn body_puts_system_at_top_level_and_uses_input_schema() {
        let msgs = vec![ChatMessage::user("generate test cases")];
        let req = TurnRequest {
            model: "claude-sonnet-4-20250514",
            system: "You are a test assistant",
            messages: &msgs,
            tools: &[spec()],
            max_tokens: 4096,
            temperature: 0.3,
        };
        let body = build_body(&req);
        assert_eq!(body["system"], "You are a test assistant");
        assert_eq!(body["tools"][0]["name"], "create_scenario");
        assert!(body["tools"][0].get("input_schema").is_some());
        assert_eq!(body["messages"][0]["role"], "user");
        // system must not appear in messages
        assert!(body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["role"] != "system"));
    }

    /// Regression: an empty turn would serialize to an empty content array, which Anthropic also rejects
    /// (`text content blocks must be non-empty`)-same strategy as the OpenAI side: drop it.
    #[test]
    fn empty_assistant_turns_are_dropped_from_payload() {
        let msgs = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant(""),
            ChatMessage::assistant("normal reply"),
        ];
        let v = to_messages(&msgs);
        assert_eq!(v.len(), 2, "empty turns must be dropped: {v:?}");
        assert_eq!(v[0]["role"], "user");
        assert_eq!(v[1]["content"][0]["text"], "normal reply");

        // An empty turn wedged between tool results must not produce an empty assistant message either
        let v = to_messages(&[
            ChatMessage::assistant_tools(vec![ToolCall {
                id: "t1".into(),
                name: "run_request".into(),
                arguments: json!({"id":"r1"}),
            }]),
            ChatMessage::tool_result("t1", "run_request", "ok", true),
            ChatMessage::assistant(""),
        ]);
        assert_eq!(v.len(), 2, "{v:?}");
        assert_eq!(v[0]["role"], "assistant");
        assert_eq!(v[1]["content"][0]["type"], "tool_result");
    }

    #[test]
    fn merges_consecutive_tool_results_into_single_user_turn() {
        let msgs = vec![
            ChatMessage::user("run the request"),
            ChatMessage::assistant_tools(vec![
                ToolCall {
                    id: "t1".into(),
                    name: "run_request".into(),
                    arguments: json!({"id":"r1"}),
                },
                ToolCall {
                    id: "t2".into(),
                    name: "run_request".into(),
                    arguments: json!({"id":"r2"}),
                },
            ]),
            ChatMessage::tool_result("t1", "run_request", "200 OK", true),
            ChatMessage::tool_result("t2", "run_request", "500", false),
        ];
        let v = to_messages(&msgs);
        assert_eq!(v.len(), 3);
        assert_eq!(v[1]["role"], "assistant");
        assert_eq!(v[1]["content"][0]["type"], "tool_use");
        assert_eq!(v[1]["content"][0]["input"]["id"], "r1");
        assert_eq!(v[2]["role"], "user");
        let blocks = v[2]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "tool_result");
        assert_eq!(blocks[1]["is_error"], true);
    }

    #[test]
    fn tool_result_precedes_text_when_user_message_follows() {
        let msgs = vec![
            ChatMessage::assistant_tools(vec![ToolCall {
                id: "t1".into(),
                name: "x".into(),
                arguments: json!({}),
            }]),
            ChatMessage::tool_result("t1", "x", "ok", true),
            ChatMessage::user("continue"),
        ];
        let v = to_messages(&msgs);
        // The tool_result user turn must precede "continue"
        assert_eq!(v[1]["content"][0]["type"], "tool_result");
        assert_eq!(v[2]["content"][0]["type"], "text");
        assert_eq!(v[2]["content"][0]["text"], "continue");
    }

    #[test]
    fn assembles_tool_use_from_block_start_and_json_deltas() {
        let mut st = AnthropicStreamState::default();
        st.apply(r#"{"type":"message_start","message":{"usage":{"input_tokens":20}}}"#)
            .unwrap();
        st.apply(
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"create_scenario","input":{}}}"#,
        )
        .unwrap();
        st.apply(r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"name\":"}}"#).unwrap();
        st.apply(r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"checkout flow\"}"}}"#).unwrap();
        st.apply(r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42}}"#).unwrap();
        let end = st.finish();
        assert_eq!(end.tool_calls.len(), 1);
        assert_eq!(end.tool_calls[0].name, "create_scenario");
        assert_eq!(end.tool_calls[0].arguments["name"], "checkout flow");
        assert_eq!(end.stop_reason.normalized(), StopReason::ToolUse);
        assert_eq!(end.usage.input_tokens, 20);
        assert_eq!(end.usage.output_tokens, 42);
    }

    #[test]
    fn parses_text_and_thinking_deltas() {
        let mut st = AnthropicStreamState::default();
        let d1 = st
            .apply(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#)
            .unwrap();
        assert!(matches!(&d1[0], ProviderDelta::Text(t) if t == "ok"));
        let d2 = st
            .apply(r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#)
            .unwrap();
        assert!(matches!(&d2[0], ProviderDelta::Reasoning(t) if t == "hmm"));
    }

    #[test]
    fn surfaces_event_level_error() {
        let mut st = AnthropicStreamState::default();
        let err = st
            .apply(r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#)
            .unwrap_err();
        match err {
            AiError::Stream(msg) => assert_eq!(msg, "Overloaded"),
            other => panic!("expected stream error, got {other}"),
        }
    }

    #[test]
    fn ping_and_stop_events_are_noop() {
        let mut st = AnthropicStreamState::default();
        assert!(st.apply(r#"{"type":"ping"}"#).unwrap().is_empty());
        assert!(st.apply(r#"{"type":"message_stop"}"#).unwrap().is_empty());
    }
}
