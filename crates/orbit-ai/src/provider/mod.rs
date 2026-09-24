//! Provider adapter layer: translates the unified [`TurnRequest`] into each vendor's HTTP protocol and
//! normalizes streaming deltas into [`ProviderDelta`].
//!
//! Two product lines are implemented so far:
//! - [`openai`]: OpenAI-compatible endpoints (`/v1/chat/completions`). Covers OpenAI, DeepSeek,
//!   Tongyi, Kimi, vLLM, Ollama (`/v1` compatibility layer), One-API and other gateways.
//! - [`anthropic`]: Anthropic Messages API (`/v1/messages`), `system` as a separate field +
//!   `tool_use` / `tool_result` content blocks.

pub mod anthropic;
pub mod openai;

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::error::AiResult;
use crate::message::{ChatMessage, ProviderDelta, ProviderTurnEnd, ToolCall};
use crate::tools::ToolSpec;

/// Provider type (determines the protocol shape, not the specific vendor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    /// OpenAI-compatible: `POST {base}/chat/completions`.
    OpenAi,
    /// Anthropic: `POST {base}/messages`.
    Anthropic,
}

impl ProviderKind {
    /// String identifier of the protocol shape (consistent with the credential file and the frontend).
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "openai",
            ProviderKind::Anthropic => "anthropic",
        }
    }

    /// The **vendor-standard auth header name** for this protocol (used when it is not Bearer).
    pub fn standard_auth_header(self) -> &'static str {
        match self {
            // Official Anthropic: apiKey -> x-api-key
            ProviderKind::Anthropic => "x-api-key",
            // The typical user of the `api-key` header in the OpenAI-compatible ecosystem is Azure OpenAI
            ProviderKind::OpenAi => "api-key",
        }
    }

    /// Default Base URL.
    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com/v1",
        }
    }

    /// Default model (**backend fallback** when the credential specifies none).
    ///
    /// ⚠ Models iterate quickly; writing an outdated one makes new users hit `model not found` on their very first call.
    /// Verified on 2026-09-21; when updating, change it together with the frontend's `web/src/lib/ai/providers.ts`.
    pub fn default_model(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "gpt-5.6-terra",
            ProviderKind::Anthropic => "claude-sonnet-5",
        }
    }

    /// Built-in selectable model presets (a fallback list per protocol shape).
    ///
    /// The frontend dropdown actually uses the **per-vendor**
    /// `PROVIDER_PRESETS` in `web/src/lib/ai/providers.ts` (with addresses and default models, from which `AI_MODEL_PRESETS` is derived);
    /// a generic list per protocol shape is kept here as a fallback for cases like "no credential".
    /// When updating models, `providers.ts` is the source of truth; keep this list in sync.
    pub fn preset_models(self) -> &'static [&'static str] {
        match self {
            ProviderKind::OpenAi => &[
                "gpt-6-astra",
                "gpt-5.6",
                "gpt-5.6-terra",
                "gpt-5.6-luna",
                "deepseek-v4-pro",
                "deepseek-flash",
                "qwen3.8-max",
                "qwen3.7-plus",
                "kimi-k3",
                "glm-5.3",
                "MiniMax-M3",
                "gemini-3.8-flash",
            ],
            ProviderKind::Anthropic => &[
                "claude-opus-5",
                "claude-sonnet-5",
                "claude-fable-5-1",
                "claude-haiku-4-5-20251001",
            ],
        }
    }
}

/// Runtime Provider config (the API key lives only in memory, never persisted to snapshots).
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// Config id (the identifier in the frontend credential list).
    pub id: String,
    /// Protocol type.
    pub kind: ProviderKind,
    /// Base URL (without suffixes such as `/chat/completions`).
    pub base_url: String,
    /// API Key。
    pub api_key: String,
    /// Extra custom request headers (vendor/gateway differences).
    pub headers: Vec<crate::auth::HeaderPair>,
    /// Auth header style: `apiKey` / `bearer` (empty = use the default for `kind`, see
    /// [`crate::auth::normalize_auth_style`]）。
    pub auth_style: String,
}

/// Build the auth header: `bearer` -> `Authorization: Bearer <key>`,
/// otherwise use the vendor-standard header (Anthropic `x-api-key` / OpenAI-compatible `api-key`).
///
/// The Provider adapter layer and "test connection" share this one function: if each had its own copy,
/// switching the auth style could easily lead to "test connection passes but the real request returns 401" (or vice versa).
pub fn auth_header(kind: ProviderKind, auth_style: &str, api_key: &str) -> (String, String) {
    if crate::auth::normalize_auth_style(kind.as_str(), auth_style)
        == crate::auth::AUTH_STYLE_BEARER
    {
        ("Authorization".to_string(), format!("Bearer {api_key}"))
    } else {
        (kind.standard_auth_header().to_string(), api_key.to_string())
    }
}

/// Merge user custom headers into the protocol header list.
///
/// **Protocol-critical headers (auth / Content-Type / Accept) must not be overridden**: those headers determine whether the request is recognized,
/// and letting a mistyped `Authorization` override the real auth only yields hard-to-diagnose 401s.
/// Existing headers with the same name (case-insensitive) are always skipped; the rest are appended in declaration order.
pub fn merge_custom_headers(
    base: Vec<(String, String)>,
    custom: &[crate::auth::HeaderPair],
) -> Vec<(String, String)> {
    let mut out = base;
    for item in custom {
        let key = item.key.trim();
        if key.is_empty() {
            continue;
        }
        let clashing = out.iter().any(|(k, _)| k.eq_ignore_ascii_case(key));
        if !clashing {
            out.push((key.to_string(), item.value.clone()));
        }
    }
    out
}

impl ProviderConfig {
    /// Normalize the Base URL (strip a trailing `/`).
    pub fn normalized_base(&self) -> String {
        self.base_url.trim().trim_end_matches('/').to_string()
    }
}

/// Input for a single streaming turn.
pub struct TurnRequest<'a> {
    /// Model name.
    pub model: &'a str,
    /// System prompt (Anthropic passes it as the top-level `system`).
    pub system: &'a str,
    /// Conversation history (including tool calls and results).
    pub messages: &'a [ChatMessage],
    /// Available tools.
    pub tools: &'a [ToolSpec],
    /// Maximum output tokens.
    pub max_tokens: u32,
    /// Temperature.
    pub temperature: f32,
}

/// Default sampling temperature.
///
/// Deliberately **not exposed as a user setting**: mainstream chat / Agent products do not expose it, because this knob has
/// almost no upside for this product's scenarios-raising it makes tool calls (which need stable JSON) go astray more easily and assertions/scripts easier
/// to get wrong, while lowering it makes explanatory answers feel stiff. 0.2 is a safe default for Agent scenarios.
///
/// When it needs to differ by mode (e.g. Ask using 0.4 for more natural answers), change it here.
pub const DEFAULT_TEMPERATURE: f32 = 0.2;

/// Delta callback (the Agent loop uses it to push streaming text to the frontend).
pub type DeltaSink = UnboundedSender<ProviderDelta>;

/// Model Provider.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Config id.
    fn id(&self) -> &str;

    /// Protocol type.
    fn kind(&self) -> ProviderKind;

    /// Run one streaming turn: deltas are pushed through `sink`, and the turn summary is returned.
    async fn stream_turn(
        &self,
        req: TurnRequest<'_>,
        sink: DeltaSink,
        cancel: CancellationToken,
    ) -> AiResult<ProviderTurnEnd>;
}

/// Build a Provider from the config.
pub fn build_provider(config: ProviderConfig) -> Box<dyn Provider> {
    match config.kind {
        ProviderKind::OpenAi => Box::new(openai::OpenAiProvider::new(config)),
        ProviderKind::Anthropic => Box::new(anthropic::AnthropicProvider::new(config)),
    }
}

/// The request for the model-list endpoint (URL + headers).
///
/// Both use `GET {base}/models`, differing only in the auth header; OpenAI-compatible endpoints (DeepSeek / Tongyi /
/// vLLM / One-API, etc.) generally implement it, while a few self-hosted gateways do not -> callers must tolerate 404.
pub fn models_request(config: &ProviderConfig) -> (String, Vec<(String, String)>) {
    let url = format!("{}/models", config.normalized_base());
    // The auth header is **constructed in the same place** as for real conversation: [`auth_header`] (otherwise, after switching the auth style
    // there can be hard-to-diagnose mismatches like "can list models but sending a message returns 401")
    let (auth_name, auth_value) = auth_header(config.kind, &config.auth_style, &config.api_key);
    let mut headers = vec![
        ("Accept".to_string(), "application/json".to_string()),
        (auth_name, auth_value),
    ];
    if config.kind == ProviderKind::Anthropic {
        headers.push((
            "anthropic-version".to_string(),
            anthropic::ANTHROPIC_VERSION.to_string(),
        ));
    }
    // Fetching the model list and real conversation use the same set of custom headers
    (url, merge_custom_headers(headers, &config.headers))
}

/// Parse the model-list response, returning model ids sorted by name and deduplicated.
///
/// Handles three shapes:
/// - `{"data":[{"id":"gpt-4o"},…]}` (official OpenAI / Anthropic)
/// - `{"models":[{"name":"llama3"},…]}` (Ollama's native `/api/tags` style)
/// - `["a","b"]` (some minimal gateways)
pub fn parse_models_response(body: &[u8]) -> AiResult<Vec<String>> {
    let text = String::from_utf8_lossy(body);
    let value: Value = serde_json::from_str(&text).map_err(|e| {
        crate::error::AiError::Invalid(format!(
            "model list is not valid JSON: {e} (raw: {})",
            crate::transport::truncate(&text, 200)
        ))
    })?;
    let mut out: Vec<String> = Vec::new();
    let push = |out: &mut Vec<String>, s: Option<&str>| {
        if let Some(s) = s.map(str::trim).filter(|s| !s.is_empty()) {
            out.push(s.to_string());
        }
    };
    if let Some(arr) = value.as_array() {
        for item in arr {
            match item {
                Value::String(s) => push(&mut out, Some(s)),
                Value::Object(o) => push(
                    &mut out,
                    o.get("id")
                        .or_else(|| o.get("name"))
                        .and_then(|v| v.as_str()),
                ),
                _ => {}
            }
        }
    } else {
        for key in ["data", "models"] {
            if let Some(arr) = value.get(key).and_then(|v| v.as_array()) {
                for item in arr {
                    match item {
                        Value::String(s) => push(&mut out, Some(s)),
                        Value::Object(o) => push(
                            &mut out,
                            o.get("id")
                                .or_else(|| o.get("name"))
                                .and_then(|v| v.as_str()),
                        ),
                        _ => {}
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    if out.is_empty() {
        return Err(crate::error::AiError::Invalid(
            "model list is empty (this endpoint may not implement /models; please enter the model name manually)".into(),
        ));
    }
    Ok(out)
}

/// Aggregate streaming `tool_calls` deltas into complete calls by index.
///
/// Both OpenAI and Anthropic transmit by "the first frame gives id/name, later frames append argument JSON fragments",
/// so the aggregation logic is shared (Anthropic's `content_block_start` gives id/name, and
/// `input_json_delta` gives the fragments).
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    slots: BTreeMap<usize, Slot>,
}

#[derive(Debug, Default)]
struct Slot {
    id: String,
    name: String,
    args: String,
}

impl ToolCallAccumulator {
    /// Create a new accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a delta.
    pub fn push(&mut self, index: usize, id: Option<&str>, name: Option<&str>, args: &str) {
        let slot = self.slots.entry(index).or_default();
        if let Some(id) = id {
            if !id.is_empty() {
                slot.id = id.to_string();
            }
        }
        if let Some(name) = name {
            if !name.is_empty() {
                slot.name = name.to_string();
            }
        }
        slot.args.push_str(args);
    }

    /// Whether any call has been collected.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Fold into the final tool-call list (ascending by index).
    ///
    /// When argument JSON fails to parse, degrade to `{"_raw": "<raw text>"}` so the tool layer can report a readable error
    /// instead of aborting the whole turn.
    pub fn finish(self) -> Vec<ToolCall> {
        self.slots
            .into_iter()
            .filter(|(_, s)| !s.name.is_empty())
            .map(|(index, s)| {
                let args_text = if s.args.trim().is_empty() {
                    "{}"
                } else {
                    s.args.as_str()
                };
                let arguments = serde_json::from_str::<Value>(args_text)
                    .unwrap_or_else(|_| serde_json::json!({ "_raw": args_text, "_invalid": true }));
                ToolCall {
                    id: if s.id.is_empty() {
                        format!("call_{index}")
                    } else {
                        s.id
                    },
                    name: s.name,
                    arguments,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(key: &str, value: &str) -> crate::auth::HeaderPair {
        crate::auth::HeaderPair {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn custom_headers_are_appended() {
        let base = vec![("Authorization".to_string(), "Bearer k".to_string())];
        let merged = merge_custom_headers(
            base,
            &[
                pair("HTTP-Referer", "https://orbit.dev"),
                pair("X-Title", "Orbit"),
            ],
        );
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[1].0, "HTTP-Referer");
        assert_eq!(merged[2].1, "Orbit");
    }

    #[test]
    fn custom_headers_never_clobber_protocol_headers() {
        // A mistyped Authorization would override the real auth, yielding only a hard-to-diagnose 401
        let base = vec![
            ("Authorization".to_string(), "Bearer real".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        let merged = merge_custom_headers(
            base,
            &[
                pair("authorization", "Bearer fake"),
                pair("CONTENT-TYPE", "text/plain"),
            ],
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].1, "Bearer real");
        assert_eq!(merged[1].1, "application/json");
    }

    #[test]
    fn blank_custom_header_keys_are_ignored() {
        let merged = merge_custom_headers(vec![], &[pair("   ", "x"), pair("X-Ok", "1")]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].0, "X-Ok");
    }

    #[test]
    fn accumulates_tool_call_arguments_split_across_frames() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(0, Some("call_1"), Some("create_request"), "{\"name\":");
        acc.push(0, None, None, "\"login\"}");
        let calls = acc.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "create_request");
        assert_eq!(calls[0].arguments["name"], "login");
    }

    #[test]
    fn accumulates_parallel_tool_calls_by_index_order() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(1, Some("b"), Some("run_request"), "{}");
        acc.push(0, Some("a"), Some("list_requests"), "{}");
        let calls = acc.finish();
        assert_eq!(calls[0].name, "list_requests");
        assert_eq!(calls[1].name, "run_request");
    }

    #[test]
    fn malformed_arguments_fall_back_to_raw_marker() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(0, Some("c"), Some("x"), "{not json");
        let calls = acc.finish();
        assert_eq!(calls[0].arguments["_invalid"], true);
        assert_eq!(calls[0].arguments["_raw"], "{not json");
    }

    #[test]
    fn empty_arguments_become_empty_object() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(0, Some("c"), Some("x"), "");
        assert_eq!(acc.finish()[0].arguments, serde_json::json!({}));
    }

    #[test]
    fn slots_without_name_are_dropped() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(0, Some("c"), None, "{}");
        assert!(acc.finish().is_empty());
    }

    fn cfg(kind: ProviderKind) -> ProviderConfig {
        ProviderConfig {
            id: "x".into(),
            kind,
            base_url: "https://api.example.com/v1".into(),
            api_key: "sk-1".into(),
            headers: vec![],
            // Empty = use the protocol default (OpenAI -> Bearer, Anthropic -> x-api-key)
            auth_style: String::new(),
        }
    }

    #[test]
    fn models_request_uses_provider_specific_auth() {
        let (url, headers) = models_request(&cfg(ProviderKind::OpenAi));
        assert_eq!(url, "https://api.example.com/v1/models");
        assert!(headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer sk-1"));

        let (url, headers) = models_request(&cfg(ProviderKind::Anthropic));
        assert_eq!(url, "https://api.example.com/v1/models");
        assert!(headers.iter().any(|(k, _)| k == "x-api-key"));
        assert!(headers.iter().any(|(k, _)| k == "anthropic-version"));
        assert!(!headers.iter().any(|(k, _)| k == "Authorization"));
    }

    #[test]
    fn auth_header_switches_between_standard_and_bearer() {
        // Anthropic: when bearer is chosen explicitly (OAuth auth token / a proxy that accepts only Bearer), switch to Authorization
        let (name, value) = auth_header(ProviderKind::Anthropic, "bearer", "sk-ant-oat");
        assert_eq!(name, "Authorization");
        assert_eq!(value, "Bearer sk-ant-oat");
        // The default (apiKey) is still the official x-api-key
        let (name, value) = auth_header(ProviderKind::Anthropic, "", "sk-ant-api");
        assert_eq!(name, "x-api-key");
        assert_eq!(value, "sk-ant-api");
        // OpenAI: choosing apiKey explicitly -> `api-key` header (Azure style); default is Bearer
        let (name, value) = auth_header(ProviderKind::OpenAi, "apiKey", "az-key");
        assert_eq!(name, "api-key");
        assert_eq!(value, "az-key");
        let (name, value) = auth_header(ProviderKind::OpenAi, "", "sk-1");
        assert_eq!(name, "Authorization");
        assert_eq!(value, "Bearer sk-1");
        // Invalid values fall back to the protocol default
        assert_eq!(
            crate::auth::normalize_auth_style("anthropic", "whatever"),
            crate::auth::AUTH_STYLE_API_KEY
        );
        assert_eq!(
            crate::auth::normalize_auth_style("openai", "whatever"),
            crate::auth::AUTH_STYLE_BEARER
        );
    }

    #[test]
    fn models_request_honors_bearer_style_for_anthropic() {
        // Fetching the model list must use the same auth header as real conversation, otherwise "can list models but sending a message returns 401"
        let mut c = cfg(ProviderKind::Anthropic);
        c.auth_style = crate::auth::AUTH_STYLE_BEARER.to_string();
        let (_, headers) = models_request(&c);
        assert!(headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer sk-1"));
        assert!(!headers.iter().any(|(k, _)| k == "x-api-key"));
    }

    #[test]
    fn parses_openai_and_anthropic_model_lists() {
        let body = br#"{"object":"list","data":[{"id":"gpt-4o"},{"id":"gpt-4o-mini"}]}"#;
        assert_eq!(
            parse_models_response(body).unwrap(),
            vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]
        );
        let body = br#"{"data":[{"id":"claude-a"}],"has_more":false}"#;
        assert_eq!(
            parse_models_response(body).unwrap(),
            vec!["claude-a".to_string()]
        );
    }

    #[test]
    fn parses_alternative_model_list_shapes() {
        // Ollama native style
        assert_eq!(
            parse_models_response(br#"{"models":[{"name":"llama3"}]}"#).unwrap(),
            vec!["llama3".to_string()]
        );
        // Minimal array
        assert_eq!(
            parse_models_response(br#"["b","a","a"]"#).unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn reports_actionable_error_for_unimplemented_endpoint() {
        let err = parse_models_response(br#"{"error":"not found"}"#).unwrap_err();
        match err {
            crate::error::AiError::Invalid(msg) => {
                assert!(msg.contains("enter the model name manually"))
            }
            other => panic!("unexpected {other}"),
        }
        assert!(parse_models_response(b"<html>404</html>").is_err());
    }

    #[test]
    fn normalizes_base_url_trailing_slash() {
        let cfg = ProviderConfig {
            id: "x".into(),
            kind: ProviderKind::OpenAi,
            base_url: " https://api.deepseek.com/v1/ ".into(),
            api_key: "k".into(),
            headers: vec![],
            auth_style: String::new(),
        };
        assert_eq!(cfg.normalized_base(), "https://api.deepseek.com/v1");
    }
}
