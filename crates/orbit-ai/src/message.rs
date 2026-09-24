//! Provider-agnostic message intermediate representation (IR).
//!
//! Design: the agent loop and the tool layer only know the IR; the OpenAI / Anthropic adapters each do their own two-way conversion.
//! Adding a Provider only means adding one more conversion module; the Agent stays untouched.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System prompt (Anthropic uses the top-level `system` field, OpenAI the first message).
    System,
    /// User input / tool results (Anthropic attaches `tool_result` to the user message).
    User,
    /// Model output (may carry tool calls as well).
    Assistant,
    /// Tool execution result (exists only in this IR; merged into user when converting).
    Tool,
}

/// A tool call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    /// Provider-side call id (must be echoed back verbatim when the result is fed back).
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Arguments (already parsed into a JSON object; on parse failure it degrades to `{"_raw": "<original text>"}`).
    pub arguments: Value,
}

/// One conversation message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// Role.
    pub role: Role,
    /// Text content (may be empty for Assistant with only tool_calls).
    #[serde(default)]
    pub text: String,
    /// Tool calls emitted by the model (Assistant only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Call id the tool result corresponds to (Tool only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Tool name (Tool only, for UI display).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Chain-of-thought delta (DeepSeek `reasoning_content` / Anthropic `thinking`), display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// Whether the tool succeeded (Tool only, used to color the UI card).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_ok: Option<bool>,
    /// Timestamp (Unix milliseconds), used when persisting sessions.
    #[serde(default)]
    pub at: i64,
}

impl ChatMessage {
    fn now_ms() -> i64 {
        jiff::Timestamp::now().as_millisecond()
    }

    /// User message.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            text: text.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            reasoning: None,
            tool_ok: None,
            at: Self::now_ms(),
        }
    }

    /// Assistant text message.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            text: text.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
            reasoning: None,
            tool_ok: None,
            at: Self::now_ms(),
        }
    }

    /// Assistant tool-call message.
    pub fn assistant_tools(calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            text: String::new(),
            tool_calls: calls,
            tool_call_id: None,
            tool_name: None,
            reasoning: None,
            tool_ok: None,
            at: Self::now_ms(),
        }
    }

    /// Tool result message.
    pub fn tool_result(
        call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
        ok: bool,
    ) -> Self {
        Self {
            role: Role::Tool,
            text: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
            tool_name: Some(name.into()),
            reasoning: None,
            tool_ok: Some(ok),
            at: Self::now_ms(),
        }
    }
}

/// Token usage.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Input tokens.
    #[serde(default)]
    pub input_tokens: u64,
    /// Output tokens.
    #[serde(default)]
    pub output_tokens: u64,
}

impl Usage {
    /// Accumulate the usage of another call.
    pub fn add(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
    }

    /// Total tokens.
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// Model stop reason (Provider-agnostic normalization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    /// Normal end.
    EndTurn,
    /// Needs to run tools (there are `tool_calls`).
    ToolUse,
    /// Hit max_tokens.
    MaxTokens,
    /// Other (including content filtering, etc.).
    Other,
}

/// One streaming delta from a model round.
#[derive(Debug, Clone)]
pub enum ProviderDelta {
    /// Body text delta.
    Text(String),
    /// Chain-of-thought delta.
    Reasoning(String),
    /// Tool-call argument delta (aggregated by index).
    ToolCallDelta {
        /// Index of the tool call within this response.
        index: usize,
        /// Call id when the call first appears.
        id: Option<String>,
        /// Tool name when the call first appears.
        name: Option<String>,
        /// Argument JSON fragment (must be concatenated in order before parsing).
        arguments_delta: String,
    },
    /// Usage (may arrive as a separate frame at the end of the stream).
    Usage(Usage),
}

/// Final result of one model round.
#[derive(Debug, Clone, Default)]
pub struct ProviderTurnEnd {
    /// Collapsed list of tool calls.
    pub tool_calls: Vec<ToolCall>,
    /// Stop reason.
    pub stop_reason: StopReasonRaw,
    /// Usage of this round.
    pub usage: Usage,
}

/// Raw stop-reason text (not normalized; kept for logs/display).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StopReasonRaw(pub Option<String>);

impl StopReasonRaw {
    /// Normalize into [`StopReason`].
    pub fn normalized(&self) -> StopReason {
        match self.0.as_deref() {
            Some("tool_calls") | Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") | Some("length") => StopReason::MaxTokens,
            Some("stop") | Some("end_turn") | Some("stop_sequence") => StopReason::EndTurn,
            _ => StopReason::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_openai_and_anthropic_stop_reasons() {
        assert_eq!(
            StopReasonRaw(Some("tool_calls".into())).normalized(),
            StopReason::ToolUse
        );
        assert_eq!(
            StopReasonRaw(Some("tool_use".into())).normalized(),
            StopReason::ToolUse
        );
        assert_eq!(
            StopReasonRaw(Some("length".into())).normalized(),
            StopReason::MaxTokens
        );
        assert_eq!(
            StopReasonRaw(Some("end_turn".into())).normalized(),
            StopReason::EndTurn
        );
        assert_eq!(StopReasonRaw(None).normalized(), StopReason::Other);
    }

    #[test]
    fn usage_adds_up() {
        let mut total = Usage::default();
        total.add(Usage {
            input_tokens: 10,
            output_tokens: 5,
        });
        total.add(Usage {
            input_tokens: 1,
            output_tokens: 2,
        });
        assert_eq!(total.total(), 18);
    }

    #[test]
    fn tool_result_carries_call_id_and_flag() {
        let m = ChatMessage::tool_result("call_1", "create_request", "ok", true);
        assert_eq!(m.role, Role::Tool);
        assert_eq!(m.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(m.tool_ok, Some(true));
    }
}
