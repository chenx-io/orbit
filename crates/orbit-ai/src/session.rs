//! AI session model and file storage.
//!
//! **Sessions are not part of snapshots** (`orbit_data::PersistedData`): session content contains prompts and request data fragments,
//! and a snapshot would leak them through exported files and git projections. So they live separately under
//! `<app_data_dir>/ai/sessions/<id>.json`, read and written on demand.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AiError, AiResult};
use crate::fsx::{ensure_dir, read_optional, write_atomic};
use crate::message::{ChatMessage, Role};
use crate::mode::AiMode;
use crate::plan::PlanArtifact;

/// Context budget: total character cap for history messages (about 8k~16k tokens, estimated at 2 chars/token).
pub const DEFAULT_CONTEXT_BUDGET: usize = 32_000;

/// Session summary (for listings, avoids reading all messages into memory).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    /// Session id.
    pub id: String,
    /// Title (first 30 chars of the first user message).
    pub title: String,
    /// Owning workspace (`None` = global).
    pub workspace_id: Option<String>,
    /// Number of messages.
    pub message_count: usize,
    /// Last updated time (Unix milliseconds).
    pub updated_at: i64,
}

/// An AI session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSession {
    /// Session id.
    pub id: String,
    /// Title.
    #[serde(default)]
    pub title: String,
    /// Owning workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// Provider config id to use (`None` = default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// Model chosen for this session (`None` = the Provider default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Working mode for this session (Ask / Agent / Plan).
    ///
    /// Why it lives on the session: the mode is state describing "what this conversation is doing", and one workspace can
    /// hold a Plan session and an Agent session at the same time, which a global preference cannot express.
    #[serde(default)]
    pub mode: AiMode,
    /// Most recent plan produced (Plan mode), persisted with the session so it survives a refresh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<PlanArtifact>,
    /// Message list.
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    /// Created time (Unix milliseconds).
    #[serde(default)]
    pub created_at: i64,
    /// Updated time (Unix milliseconds).
    #[serde(default)]
    pub updated_at: i64,
}

impl AiSession {
    /// Create a new session.
    pub fn new(workspace_id: Option<String>) -> Self {
        let now = jiff::Timestamp::now().as_millisecond();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title: "New chat".to_string(),
            workspace_id,
            provider_id: None,
            model: None,
            mode: AiMode::default(),
            plan: None,
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Append a message and refresh the title/time.
    pub fn push(&mut self, message: ChatMessage) {
        if self.title == "New chat" && message.role == Role::User {
            self.title = derive_title(&message.text);
        }
        self.messages.push(message);
        self.updated_at = jiff::Timestamp::now().as_millisecond();
    }

    /// Summary.
    pub fn summary(&self) -> SessionSummary {
        SessionSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            workspace_id: self.workspace_id.clone(),
            message_count: self.messages.len(),
            updated_at: self.updated_at,
        }
    }
}

/// Derive the title from the first user message.
pub fn derive_title(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return "New chat".to_string();
    }
    let chars: Vec<char> = trimmed.chars().take(30).collect();
    let mut title: String = chars.into_iter().collect();
    if trimmed.chars().count() > 30 {
        title.push('…');
    }
    title
}

/// Session file storage.
#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    /// Create with `<app_data_dir>/ai/sessions` as root (the directory is created on demand).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Storage root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_of(&self, id: &str) -> AiResult<PathBuf> {
        // Prevent path traversal in the id
        if id.is_empty() || id.contains(['/', '\\', '.']) {
            return Err(AiError::Invalid(format!("invalid session id: {id}")));
        }
        Ok(self.root.join(format!("{id}.json")))
    }

    /// List all session summaries (most recently updated first).
    pub fn list(&self) -> AiResult<Vec<SessionSummary>> {
        ensure_dir(&self.root)?;
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Some(text) = read_optional(&path)? {
                match serde_json::from_str::<AiSession>(&text) {
                    Ok(session) => out.push(session.summary()),
                    // A single corrupt file must not fail the whole listing
                    Err(e) => tracing::warn!(
                        target: "orbit_ai",
                        "skipping corrupt session file {}: {e}",
                        path.display()
                    ),
                }
            }
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        Ok(out)
    }

    /// Load a single session.
    pub fn load(&self, id: &str) -> AiResult<AiSession> {
        let path = self.path_of(id)?;
        let text = read_optional(&path)?
            .ok_or_else(|| AiError::Invalid(format!("session not found: {id}")))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Write (atomically).
    pub fn save(&self, session: &AiSession) -> AiResult<()> {
        let path = self.path_of(&session.id)?;
        ensure_dir(&self.root)?;
        let text = serde_json::to_string_pretty(session)?;
        write_atomic(&path, text.as_bytes())
    }

    /// Delete; returns `false` if the file does not exist.
    pub fn delete(&self, id: &str) -> AiResult<bool> {
        let path = self.path_of(id)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(AiError::Io(e)),
        }
    }
}

/// Context characters used by a single message (tool calls estimated by their JSON size).
///
/// This is the **single source of cost accounting**: both trimming and the "context usage" display go through it, keeping the UI number
/// consistent with the engine's actual trimming behavior.
pub fn message_cost(m: &ChatMessage) -> usize {
    m.text.chars().count() + m.tool_calls.len() * 64
}

/// Total characters used by history messages (before trimming).
pub fn history_cost(messages: &[ChatMessage]) -> usize {
    messages.iter().map(message_cost).sum()
}

/// Rough chars -> tokens estimate (about 2 chars/token, consistent with [`DEFAULT_CONTEXT_BUDGET`]).
pub fn chars_to_tokens(chars: usize) -> usize {
    chars.div_ceil(2)
}

/// Context usage snapshot (for UI display).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    /// Total messages in the session.
    pub total_messages: usize,
    /// Total characters across session messages.
    pub total_chars: usize,
    /// Messages actually sent this round (after trimming).
    pub sent_messages: usize,
    /// Characters actually sent this round (after trimming).
    pub sent_chars: usize,
    /// Character budget.
    pub budget_chars: usize,
}

impl ContextUsage {
    /// Share of the budget sent this round (0.0 ~ 1.0+).
    pub fn ratio(&self) -> f32 {
        if self.budget_chars == 0 {
            return 0.0;
        }
        self.sent_chars as f32 / self.budget_chars as f32
    }

    /// Whether trimming has kicked in (history was dropped).
    pub fn trimmed(&self) -> bool {
        self.sent_messages < self.total_messages
    }
}

/// Compute context usage: `sent_*` is the part actually sent after trimming to the budget.
pub fn context_usage(messages: &[ChatMessage], budget: usize) -> ContextUsage {
    let kept = trim_history(messages, budget);
    ContextUsage {
        total_messages: messages.len(),
        total_chars: history_cost(messages),
        sent_messages: kept.len(),
        sent_chars: history_cost(&kept),
        budget_chars: budget,
    }
}

/// Trim history to the character budget while keeping tool calls paired with their results.
///
/// Rule: accumulate from the newest message backwards until the budget is exceeded; then **move the start forward** to the nearest clean boundary
/// (must not begin with a Tool message or an Assistant message carrying tool_calls, or the Provider rejects it).
pub fn trim_history(messages: &[ChatMessage], budget: usize) -> Vec<ChatMessage> {
    if messages.is_empty() {
        return Vec::new();
    }
    let mut used = 0usize;
    let mut start = messages.len();
    for (idx, m) in messages.iter().enumerate().rev() {
        let len = message_cost(m);
        if used + len > budget && idx + 1 < messages.len() {
            break;
        }
        used += len;
        start = idx;
    }
    // Move forward to a safe start: must not begin with a Tool (missing assistant tool_calls),
    // nor with an Assistant carrying tool_calls (missing their results).
    while start < messages.len() {
        let m = &messages[start];
        if m.role == Role::Tool || (m.role == Role::Assistant && !m.tool_calls.is_empty()) {
            start += 1;
        } else {
            break;
        }
    }
    messages[start..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> SessionStore {
        let dir = std::env::temp_dir().join(format!("orbit-ai-sess-{}", uuid::Uuid::new_v4()));
        SessionStore::new(dir.join("sessions"))
    }

    #[test]
    fn save_load_list_delete_roundtrip() {
        let s = store();
        let mut session = AiSession::new(Some("ws-1".into()));
        session.push(ChatMessage::user("Help me generate a login request"));
        s.save(&session).unwrap();

        let loaded = s.load(&session.id).unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert!(loaded.title.starts_with("Help me generate"));

        let list = s.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, session.id);

        assert!(s.delete(&session.id).unwrap());
        assert!(!s.delete(&session.id).unwrap());
        assert!(s.list().unwrap().is_empty());
    }

    #[test]
    fn list_survives_corrupted_file() {
        let s = store();
        let mut session = AiSession::new(None);
        session.push(ChatMessage::user("hi"));
        s.save(&session).unwrap();
        ensure_dir(s.root()).unwrap();
        std::fs::write(s.root().join("broken.json"), b"{not json").unwrap();
        assert_eq!(s.list().unwrap().len(), 1);
    }

    #[test]
    fn rejects_path_traversal_ids() {
        let s = store();
        assert!(s.load("../secret").is_err());
        assert!(s.load("").is_err());
    }

    #[test]
    fn title_derived_from_first_user_message_only() {
        let mut session = AiSession::new(None);
        session.push(ChatMessage::assistant("hello"));
        assert_eq!(session.title, "New chat");
        session.push(ChatMessage::user("Generate a load test config"));
        assert_eq!(session.title, "Generate a load test config");
        session.push(ChatMessage::user("one more time"));
        assert_eq!(session.title, "Generate a load test config");
    }

    #[test]
    fn long_title_is_truncated() {
        let t = derive_title(&"x".repeat(50));
        assert_eq!(t.chars().count(), 31);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn context_usage_reports_sent_and_total_consistent_with_trim() {
        let msgs: Vec<ChatMessage> = (0..10)
            .map(|i| ChatMessage::user(format!("msg{i}{}", "x".repeat(100))))
            .collect();
        let usage = context_usage(&msgs, 350);
        // The display must match the trim result: otherwise the "usage" shown in the UI won't match what is actually sent
        assert_eq!(usage.sent_messages, trim_history(&msgs, 350).len());
        assert_eq!(usage.total_messages, 10);
        assert!(usage.trimmed());
        assert_eq!(usage.total_chars, history_cost(&msgs));
        assert!(usage.sent_chars <= usage.total_chars);
        assert!(usage.ratio() > 0.0 && usage.ratio() <= 1.0);
    }

    #[test]
    fn context_usage_within_budget_is_not_trimmed() {
        let msgs = vec![ChatMessage::user("x")];
        let usage = context_usage(&msgs, DEFAULT_CONTEXT_BUDGET);
        assert!(!usage.trimmed());
        assert_eq!(usage.sent_messages, 1);
        // "x" = 1 char -> rounds up to 1 token
        assert_eq!(chars_to_tokens(usage.sent_chars), 1);
        assert!(usage.ratio() < 0.01);
    }

    #[test]
    fn message_cost_counts_tool_call_bodies() {
        let mut m = ChatMessage::user("");
        m.tool_calls.push(crate::message::ToolCall {
            id: "c".into(),
            name: "n".into(),
            arguments: serde_json::json!({}),
        });
        assert_eq!(message_cost(&m), 64);
    }

    #[test]
    fn trim_keeps_recent_messages_under_budget() {
        let msgs: Vec<ChatMessage> = (0..10)
            .map(|i| ChatMessage::user(format!("msg{i}{}", "x".repeat(100))))
            .collect();
        let kept = trim_history(&msgs, 350);
        assert!(kept.len() < 10);
        assert!(kept.last().unwrap().text.starts_with("msg9"));
    }

    #[test]
    fn trim_never_starts_with_orphan_tool_result() {
        use crate::message::ToolCall;
        let msgs = vec![
            ChatMessage::user("a".repeat(500)),
            ChatMessage::assistant_tools(vec![ToolCall {
                id: "c1".into(),
                name: "list_requests".into(),
                arguments: serde_json::json!({}),
            }]),
            ChatMessage::tool_result("c1", "list_requests", "ok", true),
            ChatMessage::user("x"),
        ];
        let kept = trim_history(&msgs, 80);
        assert!(!kept.is_empty());
        assert_ne!(kept[0].role, Role::Tool);
        assert!(
            kept[0].tool_calls.is_empty(),
            "must not start with an assistant message carrying tool calls"
        );
    }

    #[test]
    fn trim_empty_input_is_empty() {
        assert!(trim_history(&[], 100).is_empty());
    }
}
