//! OpenAI-compatible Provider: `POST {base}/chat/completions` (`stream: true`).
//!
//! Compatibility scope: OpenAI, DeepSeek (including `reasoning_content`), Tongyi / Kimi,
//! vLLM, Ollama's `/v1` compatibility layer, and aggregation gateways such as One-API / New-API.

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

/// OpenAI-compatible Provider.
pub struct OpenAiProvider {
    config: ProviderConfig,
}

impl OpenAiProvider {
    /// Build from the config.
    pub fn new(config: ProviderConfig) -> Self {
        Self { config }
    }

    fn headers(&self) -> Vec<(String, String)> {
        // Defaults to `Authorization: Bearer`; switching to the `api-key` header is for gateways like Azure OpenAI
        let (auth_name, auth_value) = super::auth_header(
            super::ProviderKind::OpenAi,
            &self.config.auth_style,
            &self.config.api_key,
        );
        // Custom headers (e.g. OpenRouter's HTTP-Referer) are merged after the protocol headers; on a name clash the protocol header wins
        super::merge_custom_headers(
            vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Accept".to_string(), "text/event-stream".to_string()),
                (auth_name, auth_value),
            ],
            &self.config.headers,
        )
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAi
    }

    async fn stream_turn(
        &self,
        req: TurnRequest<'_>,
        sink: DeltaSink,
        cancel: CancellationToken,
    ) -> AiResult<ProviderTurnEnd> {
        let url = format!("{}/chat/completions", self.config.normalized_base());
        let headers = self.headers();

        let body = serde_json::to_vec(&build_body(&req, true))?;
        let mut stream = match post_sse(&url, headers.clone(), body, &cancel).await {
            Ok((_status, _headers, stream)) => stream,
            // A few compatible endpoints reject `stream_options`: drop it and retry once
            Err(AiError::Provider { status, body })
                if status == 400 && body.contains("stream_options") =>
            {
                tracing::debug!(target: "orbit_ai", "provider rejected stream_options, retrying with a downgrade");
                post_sse(
                    &url,
                    headers,
                    serde_json::to_vec(&build_body(&req, false))?,
                    &cancel,
                )
                .await?
                .2
            }
            Err(e) => return Err(e),
        };

        let mut state = OpenAiStreamState::default();
        while let Some(ev) = stream.next_event(&cancel).await? {
            if ev.is_done() {
                break;
            }
            if ev.data.trim().is_empty() {
                continue;
            }
            for delta in state.apply(&ev.data)? {
                // A frontend disconnect does not affect backend wrap-up; ignore errors with no receiver
                let _ = sink.send(delta);
            }
        }
        Ok(state.finish())
    }
}

/// Build the request body. `include_usage` controls whether token usage is requested at the end of the stream.
pub fn build_body(req: &TurnRequest<'_>, include_usage: bool) -> Value {
    let mut body = json!({
        "model": req.model,
        "messages": to_messages(req.system, req.messages),
        "stream": true,
        "temperature": req.temperature,
        "max_tokens": req.max_tokens,
    });
    if include_usage {
        body["stream_options"] = json!({ "include_usage": true });
    }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.iter().map(|t| t.to_openai()).collect());
        body["tool_choice"] = json!("auto");
    }
    body
}

/// IR -> OpenAI `messages` (`system` as the first message; `reasoning` is not sent back).
pub fn to_messages(system: &str, messages: &[ChatMessage]) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len() + 1);
    if !system.trim().is_empty() {
        out.push(json!({ "role": "system", "content": system }));
    }
    for m in messages {
        match m.role {
            Role::System => continue,
            Role::User => out.push(json!({ "role": "user", "content": m.text })),
            Role::Assistant => {
                // An assistant message with neither text nor tool calls **must be dropped**: DeepSeek / OpenAI return
                // 400（`Invalid assistant message: content or tool_calls must be set`）。
                //
                // Such "empty turns" are not rare: the model spends its whole budget on the chain of thought, content is safety-filtered, an aggregation gateway returns an empty
                // choices, the stream is cut off midway… And once it enters the conversation history it **sticks around for a long time**-every later turn
                // of that session carries it, so every turn returns 400. This is exactly the user-visible
                // "it worked before, then suddenly stopped working".
                //
                // An empty turn has no semantic value (no content, no calls), and dropping it does not hurt conversation continuity; the
                // chain of thought has its own `reasoning` field and is never sent back anyway.
                if m.text.is_empty() && m.tool_calls.is_empty() {
                    continue;
                }
                let mut item = json!({
                    "role": "assistant",
                    "content": if m.text.is_empty() { Value::Null } else { json!(m.text) },
                });
                if !m.tool_calls.is_empty() {
                    item["tool_calls"] = Value::Array(
                        m.tool_calls
                            .iter()
                            .map(|tc| {
                                json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.arguments.to_string(),
                                    }
                                })
                            })
                            .collect(),
                    );
                }
                out.push(item);
            }
            Role::Tool => out.push(json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                "content": m.text,
            })),
        }
    }
    out
}

/// Streaming-response parse state machine (decoupled from the network, for easy unit testing).
#[derive(Debug, Default)]
pub struct OpenAiStreamState {
    acc: ToolCallAccumulator,
    finish_reason: Option<String>,
    usage: Usage,
}

impl OpenAiStreamState {
    /// Process one `data:` JSON frame, producing zero or more deltas.
    pub fn apply(&mut self, data: &str) -> AiResult<Vec<ProviderDelta>> {
        let value: Value = serde_json::from_str(data).map_err(|e| {
            AiError::Transport(format!(
                "failed to parse model stream: {e} (raw: {})",
                truncate(data, 200)
            ))
        })?;

        if let Some(err) = value.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| err.to_string());
            return Err(AiError::Stream(msg));
        }

        let mut deltas = Vec::new();

        if let Some(u) = value.get("usage").filter(|u| !u.is_null()) {
            let usage = Usage {
                input_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                output_tokens: u
                    .get("completion_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            };
            if usage.total() > 0 {
                self.usage = usage;
                deltas.push(ProviderDelta::Usage(usage));
            }
        }

        let Some(choice) = value.get("choices").and_then(|c| c.get(0)) else {
            // Usage frames / heartbeat frames have no choices
            return Ok(deltas);
        };

        if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            self.finish_reason = Some(reason.to_string());
        }

        let Some(delta) = choice.get("delta") else {
            return Ok(deltas);
        };

        if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
            if !text.is_empty() {
                deltas.push(ProviderDelta::Text(text.to_string()));
            }
        }
        // Chain-of-thought fields of DeepSeek / some gateways
        for key in ["reasoning_content", "reasoning"] {
            if let Some(text) = delta.get(key).and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    deltas.push(ProviderDelta::Reasoning(text.to_string()));
                }
            }
        }

        if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
            for call in calls {
                let index = call.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let id = call.get("id").and_then(|v| v.as_str());
                let function = call.get("function");
                let name = function
                    .and_then(|f| f.get("name"))
                    .and_then(|v| v.as_str());
                let args = function
                    .and_then(|f| f.get("arguments"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.acc.push(index, id, name, args);
                deltas.push(ProviderDelta::ToolCallDelta {
                    index,
                    id: id.map(|s| s.to_string()),
                    name: name.map(|s| s.to_string()),
                    arguments_delta: args.to_string(),
                });
            }
        }

        Ok(deltas)
    }

    /// Finish: fold tool calls and normalize the stop reason.
    pub fn finish(self) -> ProviderTurnEnd {
        let tool_calls = self.acc.finish();
        let raw = StopReasonRaw(self.finish_reason);
        let stop_reason = if !tool_calls.is_empty() && raw.normalized() != StopReason::ToolUse {
            // Some gateways still return "stop" even with tool calls
            StopReasonRaw(Some("tool_calls".to_string()))
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
    use crate::message::{ChatMessage, ToolCall};
    use crate::tools::{ToolKind, ToolSpec};

    fn spec() -> ToolSpec {
        ToolSpec {
            name: "create_request".into(),
            kind: ToolKind::Write,
            description: "create a new request".into(),
            parameters: json!({"type":"object","properties":{"name":{"type":"string"}}}),
        }
    }

    #[test]
    fn body_includes_tools_and_system_message() {
        let msgs = vec![ChatMessage::user("help me create a login endpoint")];
        let req = TurnRequest {
            model: "gpt-4o-mini",
            system: "You are an assistant",
            messages: &msgs,
            tools: &[spec()],
            max_tokens: 4096,
            temperature: 0.2,
        };
        let body = build_body(&req, true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "You are an assistant");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["tools"][0]["function"]["name"], "create_request");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn body_omits_tools_when_empty() {
        let msgs = vec![ChatMessage::user("hi")];
        let req = TurnRequest {
            model: "m",
            system: "",
            messages: &msgs,
            tools: &[],
            max_tokens: 100,
            temperature: 0.0,
        };
        let body = build_body(&req, false);
        assert!(body.get("tools").is_none());
        assert!(body.get("stream_options").is_none());
        // An empty system produces no system message
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn assistant_with_tool_calls_is_serialized_with_null_content() {
        let msgs = vec![
            ChatMessage::user("create endpoint"),
            ChatMessage::assistant_tools(vec![ToolCall {
                id: "call_1".into(),
                name: "create_request".into(),
                arguments: json!({"name":"login"}),
            }]),
            ChatMessage::tool_result("call_1", "create_request", "ok", true),
        ];
        let v = to_messages("", &msgs);
        assert_eq!(v[1]["role"], "assistant");
        assert_eq!(v[1]["content"], Value::Null);
        assert_eq!(v[1]["tool_calls"][0]["function"]["name"], "create_request");
        assert_eq!(
            v[1]["tool_calls"][0]["function"]["arguments"],
            "{\"name\":\"login\"}"
        );
        assert_eq!(v[2]["role"], "tool");
        assert_eq!(v[2]["tool_call_id"], "call_1");
    }

    /// Regression (real incident): DeepSeek 400 `Invalid assistant message: content or tool_calls must be set`.
    ///
    /// When an "empty turn" (no text, no tool calls) slipped into the history, the old code serialized it as
    /// `{"role":"assistant","content":null}` and sent it, so **every later turn of that session** was rejected.
    #[test]
    fn empty_assistant_turns_are_dropped_from_payload() {
        // Exactly the shape of the real damaged session (model deepseek-flash): a pure reasoning turn
        // - text empty, tool_calls empty, but reasoning with 16k characters.
        let mut reasoning_only = ChatMessage::assistant("");
        reasoning_only.reasoning = Some("x".repeat(16_872));

        let msgs = vec![
            ChatMessage::user("hello"),
            reasoning_only,
            ChatMessage::assistant("normal reply"),
        ];
        let v = to_messages("", &msgs);
        assert_eq!(v.len(), 2, "empty turns must be dropped: {v:?}");
        assert_eq!(v[0]["role"], "user");
        assert_eq!(v[1]["content"], "normal reply");

        // When the whole history is only empty turns, no message may remain (otherwise even system cannot save it)
        let mut only = ChatMessage::assistant("");
        only.reasoning = Some("thinking".into());
        assert!(to_messages("", &[only]).is_empty());

        // An assistant with tool calls (content=null) is a **valid shape** and must be preserved as-is
        let v = to_messages(
            "",
            &[ChatMessage::assistant_tools(vec![ToolCall {
                id: "c1".into(),
                name: "list_requests".into(),
                arguments: json!({}),
            }])],
        );
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0]["content"], Value::Null);
        assert_eq!(v[0]["tool_calls"][0]["id"], "c1");
    }

    #[test]
    fn parses_text_delta() {
        let mut st = OpenAiStreamState::default();
        let deltas = st
            .apply(r#"{"choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#)
            .unwrap();
        assert!(matches!(&deltas[0], ProviderDelta::Text(t) if t == "hello"));
    }

    #[test]
    fn parses_reasoning_delta() {
        let mut st = OpenAiStreamState::default();
        let deltas = st
            .apply(r#"{"choices":[{"delta":{"reasoning_content":"let me think"}}]}"#)
            .unwrap();
        assert!(matches!(&deltas[0], ProviderDelta::Reasoning(t) if t == "let me think"));
    }

    #[test]
    fn assembles_tool_call_across_frames_and_normalizes_stop_reason() {
        let mut st = OpenAiStreamState::default();
        st.apply(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_9","function":{"name":"run_request","arguments":"{\"id\":"}}]}}]}"#).unwrap();
        st.apply(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"r1\"}"}}]}}]}"#).unwrap();
        st.apply(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#)
            .unwrap();
        let end = st.finish();
        assert_eq!(end.tool_calls.len(), 1);
        assert_eq!(end.tool_calls[0].arguments["id"], "r1");
        assert_eq!(end.stop_reason.normalized(), StopReason::ToolUse);
    }

    #[test]
    fn captures_usage_frame_without_choices() {
        let mut st = OpenAiStreamState::default();
        let deltas = st
            .apply(r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#)
            .unwrap();
        assert!(matches!(deltas[0], ProviderDelta::Usage(u) if u.total() == 15));
        assert_eq!(st.finish().usage.total(), 15);
    }

    #[test]
    fn surfaces_inline_error_object() {
        let mut st = OpenAiStreamState::default();
        let err = st
            .apply(r#"{"error":{"message":"rate limited","type":"rate_limit"}}"#)
            .unwrap_err();
        match err {
            AiError::Stream(msg) => assert!(msg.contains("rate limited")),
            other => panic!("expected stream error, got {other}"),
        }
    }

    #[test]
    fn heartbeat_frame_without_choices_is_ignored() {
        let mut st = OpenAiStreamState::default();
        assert!(st.apply(r#"{"choices":[]}"#).unwrap().is_empty());
    }
}
