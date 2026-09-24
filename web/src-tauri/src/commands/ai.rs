//! AI assistant command layer: BYOK credentials, sessions, agent turns, tool authorization, and event polling.
//!
//! Transport model: **no Tauri event push**. The project has learned this the hard way - a background thread calling `app.emit` competes with the window
//! message loop for the same webview lock, freezing the app while dragging/resizing the window (tauri#9453, see the top comment in `load.rs`).
//! So the agent's streaming events all go into `orbit_ai::EventBus`, and the frontend polls
//! `ai_drain_events` to drain them (the same pattern as load-testing's `load_progress`).
//!
//! Lifecycle of one "user turn":
//! 1. `ai_start_turn`: write the user message -> assemble context and prompt -> spawn the agent loop in the background -> immediately return `turnId`;
//! 2. during the loop, events go into the buffer and the frontend polls and renders them;
//! 3. write operations first `preview` a proposal card (not persisted), and only write after the frontend's `ai_approve_tool` resolves the oneshot;
//! 4. on completion, append this turn's new messages to the session and persist, and the frontend reloads the session after receiving `TurnFinished`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use orbit_ai::auth::{CredentialInput, CredentialStore, CredentialView};
use orbit_ai::prompt::ContextSummary;
use orbit_ai::session::{AiSession, SessionStore, SessionSummary};
use orbit_ai::{Agent, AgentLimits, AiError, Approval, Approver, EventBus, ProviderConfig};
use orbit_data::model::AiPrefs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use crate::commands::ai_host::TauriToolHost;
use crate::state::AppState;

/// AI runtime state (hangs off `AppState`, cloneable into background tasks).
#[derive(Clone)]
pub struct AiState {
    /// App data directory (run reports and the like are stored under it).
    pub app_data_dir: PathBuf,
    /// Session store.
    pub sessions: SessionStore,
    /// Credentials file path.
    pub credentials_path: PathBuf,
    /// Event buffer (drained by frontend polling).
    pub bus: EventBus,
    /// In-flight turns: turn_id -> cancellation token.
    turns: Arc<Mutex<HashMap<String, CancellationToken>>>,
    /// Tools awaiting authorization: call_id -> decision channel.
    approvals: Arc<Mutex<HashMap<String, oneshot::Sender<Approval>>>>,
}

impl AiState {
    /// Created with `<app_data_dir>/ai` as its root.
    pub fn new(app_data_dir: &std::path::Path) -> Self {
        Self {
            sessions: SessionStore::new(app_data_dir.join("ai").join("sessions")),
            credentials_path: orbit_ai::auth::default_credentials_path(app_data_dir),
            app_data_dir: app_data_dir.to_path_buf(),
            // Buffer widened to 8192: streaming text is pushed in character increments, so one long answer can produce over a thousand events;
            // the frontend drains by polling every 120ms, normally far below the cap, so this is just headroom.
            bus: EventBus::with_capacity(8192),
            turns: Arc::new(Mutex::new(HashMap::new())),
            approvals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn credentials(&self) -> Result<CredentialStore, String> {
        CredentialStore::load(&self.credentials_path).map_err(|e| e.user_message())
    }

    fn save_credentials(&self, store: &CredentialStore) -> Result<(), String> {
        store
            .save(&self.credentials_path)
            .map_err(|e| e.user_message())
    }
}

/// An Approver that waits for the frontend's authorization decision (resolved via `ai_approve_tool`).
struct UiApprover {
    approvals: Arc<Mutex<HashMap<String, oneshot::Sender<Approval>>>>,
}

#[async_trait::async_trait]
impl Approver for UiApprover {
    async fn request(
        &self,
        call: &orbit_ai::ToolCall,
        _kind: orbit_ai::ToolKind,
        cancel: &CancellationToken,
    ) -> Approval {
        let (tx, rx) = oneshot::channel::<Approval>();
        self.approvals.lock().await.insert(call.id.clone(), tx);
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Approval::deny("the user aborted this turn"),
            decided = rx => decided.unwrap_or_else(|_| Approval::deny("the approval channel was closed")),
        }
    }
}

// ─── Request/response types ───────────────────────────────────────

/// Input for starting a turn.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartTurnRequest {
    /// Session id; an empty string = new session.
    pub session_id: String,
    /// User input text.
    pub text: String,
    /// Workspace/environment/selection context assembled by the frontend.
    #[serde(default)]
    pub context: ContextSummary,
    /// Work mode (`ask` / `agent` / `plan`). When omitted, falls back in order to the session, global prefs, then Agent.
    #[serde(default)]
    pub mode: Option<String>,
    /// Override model (defaults to the model in prefs).
    #[serde(default)]
    pub model: Option<String>,
    /// Override credential id (defaults to the provider_id in prefs).
    #[serde(default)]
    pub provider_id: Option<String>,
}

/// Start result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartTurnResponse {
    /// Turn id.
    pub turn_id: String,
    /// Session id (generated by the backend when creating).
    pub session_id: String,
}

/// Test-connection input.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConnectionRequest {
    /// Credential id (used to read the real key).
    pub provider_id: String,
    /// Optional model override.
    #[serde(default)]
    pub model: Option<String>,
}

// ─── Preferences and credentials ─────────────────────────────────────────

/// Read AI preferences.
#[tauri::command]
pub async fn ai_config_get(state: State<'_, AppState>) -> Result<AiPrefs, String> {
    Ok(state.data.ai_prefs())
}

/// Overwrite AI preferences.
#[tauri::command]
pub async fn ai_config_save(state: State<'_, AppState>, prefs: AiPrefs) -> Result<(), String> {
    state
        .data
        .set_ai_prefs(prefs)
        .await
        .map_err(|e| e.to_string())
}

/// Credential list (masked; the plaintext key is never returned).
#[tauri::command]
pub fn ai_credential_list(state: State<'_, AppState>) -> Result<Vec<CredentialView>, String> {
    Ok(state.ai.credentials()?.views())
}

/// Create / update a credential; when `api_key` is omitted the existing value is kept. Returns the credential id.
#[tauri::command]
pub fn ai_credential_save(
    state: State<'_, AppState>,
    input: CredentialInput,
) -> Result<String, String> {
    let mut store = state.ai.credentials()?;
    let id = store.upsert(input).map_err(|e| e.user_message())?;
    state.ai.save_credentials(&store)?;
    Ok(id)
}

/// Delete a credential.
#[tauri::command]
pub fn ai_credential_remove(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    let mut store = state.ai.credentials()?;
    let removed = store.remove(&id);
    state.ai.save_credentials(&store)?;
    Ok(removed)
}

/// Resolve the provider kind and effective Base URL from a credential (empty falls back to the kind's default address).
fn resolve_provider(credential: &orbit_ai::auth::AiCredential) -> (orbit_ai::ProviderKind, String) {
    let kind = match credential.kind.as_str() {
        "anthropic" => orbit_ai::ProviderKind::Anthropic,
        _ => orbit_ai::ProviderKind::OpenAi,
    };
    let base = if credential.base_url.trim().is_empty() {
        kind.default_base_url().to_string()
    } else {
        credential.base_url.trim().to_string()
    };
    (kind, base)
}

/// Fetch the model list available for this credential (`GET {base}/models`), for the settings UI to pick tracked models.
///
/// A few self-hosted gateways don't implement this endpoint; a readable error is returned and the user can still type the model name.
#[tauri::command]
pub async fn ai_list_models(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<String>, String> {
    let store = state.ai.credentials()?;
    let credential = store
        .get(&provider_id)
        .ok_or_else(|| format!("credential {provider_id} does not exist"))?;
    let (kind, base_url) = resolve_provider(credential);
    let cfg = orbit_ai::ProviderConfig {
        id: provider_id,
        kind,
        base_url,
        api_key: credential.api_key.clone(),
        headers: credential.headers.clone(),
        auth_style: credential.auth_style.clone(),
    };
    let (url, headers) = orbit_ai::provider::models_request(&cfg);
    let (status, _headers, body) =
        orbit_ai::transport::get_json_once(&url, headers, std::time::Duration::from_secs(20))
            .await
            .map_err(|e| e.user_message())?;
    if !orbit_ai::transport::is_success(status) {
        return Err(format!(
            "model-list endpoint returned {status}: {}",
            orbit_ai::transport::truncate(&String::from_utf8_lossy(&body), 300)
        ));
    }
    orbit_ai::provider::parse_models_response(&body).map_err(|e| e.user_message())
}

/// Test connectivity: send a minimal non-streaming request and return the status and latency.
#[tauri::command]
pub async fn ai_test_connection(
    state: State<'_, AppState>,
    request: TestConnectionRequest,
) -> Result<Value, String> {
    let store = state.ai.credentials()?;
    let credential = store
        .get(&request.provider_id)
        .ok_or_else(|| format!("credential {} does not exist", request.provider_id))?;
    let (kind, base) = resolve_provider(credential);
    let model = request
        .model
        .clone()
        .or_else(|| credential.default_model.clone())
        .unwrap_or_else(|| kind.default_model().to_string());

    // The auth header is built in **the same place** as real conversations: otherwise you get false impressions like "test connection passes but real messages 401"
    let auth = orbit_ai::provider::auth_header(kind, &credential.auth_style, &credential.api_key);
    let (url, headers, body) = match kind {
        orbit_ai::ProviderKind::OpenAi => (
            format!("{}/chat/completions", base.trim_end_matches('/')),
            vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                auth,
            ],
            serde_json::json!({
                "model": model,
                "messages": [{ "role": "user", "content": "ping" }],
                "max_tokens": 8,
            }),
        ),
        orbit_ai::ProviderKind::Anthropic => (
            format!("{}/messages", base.trim_end_matches('/')),
            vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                auth,
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ],
            serde_json::json!({
                "model": model,
                "max_tokens": 8,
                "messages": [{ "role": "user", "content": "ping" }],
            }),
        ),
    };
    // Custom headers must travel with "test connection": otherwise you get the false impression of "test passes but conversation fails"
    let headers = orbit_ai::provider::merge_custom_headers(headers, &credential.headers);

    let started = std::time::Instant::now();
    let payload = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let result = orbit_ai::transport::post_json_once(
        &url,
        headers,
        payload,
        std::time::Duration::from_secs(20),
    )
    .await;
    let elapsed = started.elapsed().as_millis() as u64;
    match result {
        Ok((status, _headers, body)) => Ok(serde_json::json!({
            "ok": (200..300).contains(&status),
            "status": status,
            "latencyMs": elapsed,
            "model": model,
            "message": orbit_ai::transport::truncate(
                &String::from_utf8_lossy(&body), 400),
        })),
        Err(e) => Ok(serde_json::json!({
            "ok": false,
            "status": 0,
            "latencyMs": elapsed,
            "model": model,
            "message": e.user_message(),
        })),
    }
}

// ─── Sessions ───────────────────────────────────────────────

/// Session list (filterable by workspace).
#[tauri::command]
pub fn ai_session_list(
    state: State<'_, AppState>,
    workspace_id: Option<String>,
) -> Result<Vec<SessionSummary>, String> {
    let mut rows = state.ai.sessions.list().map_err(|e| e.user_message())?;
    if let Some(ws) = workspace_id.filter(|w| !w.is_empty()) {
        rows.retain(|r| r.workspace_id.as_deref() == Some(ws.as_str()));
    }
    Ok(rows)
}

/// Load a single session.
#[tauri::command]
pub fn ai_session_load(state: State<'_, AppState>, id: String) -> Result<AiSession, String> {
    state.ai.sessions.load(&id).map_err(|e| e.user_message())
}

/// Save a session (rename / toggle auto-apply / manual edit).
#[tauri::command]
pub fn ai_session_save(state: State<'_, AppState>, session: AiSession) -> Result<(), String> {
    state
        .ai
        .sessions
        .save(&session)
        .map_err(|e| e.user_message())
}

/// Delete a session.
#[tauri::command]
pub fn ai_session_delete(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    state.ai.sessions.delete(&id).map_err(|e| e.user_message())
}

// ─── External API-definition file references ─────────────────────────────────

/// Definition-file extensions allowed for reference (allowlist: read-only textual API descriptions, not arbitrary files).
const REFERENCE_EXTS: &[&str] = &[
    "json", "yaml", "yml", "har", "http", "rest", "graphql", "gql", "proto", "md", "txt",
];

/// Maximum characters injected into the prompt for a single reference.
///
/// Better to truncate than to burn the entire context budget at once: a real-world OAS is often hundreds of KB,
/// and the truncated form is still enough for the model to see paths and field conventions; the rest is left for the user to split as needed.
const MAX_REFERENCE_CHARS: usize = 24_000;

/// Reference-file read result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceFile {
    /// File name.
    pub name: String,
    /// Absolute path.
    pub path: String,
    /// File size in bytes.
    pub bytes: u64,
    /// Text content (truncated when too long).
    pub text: String,
    /// Whether truncation occurred.
    pub truncated: bool,
}

/// Read the external API-definition file selected by the user.
///
/// Security boundary: **read-only, allowlisted extensions, size-capped**. The file content is then inlined by the frontend alongside
/// [`orbit_ai::prompt::Reference`] into the prompt, rather than giving the model a "read file by path" tool -
/// the latter would hand the model read access to the entire filesystem (a single prompt injection would escalate privileges).
#[tauri::command]
pub fn ai_read_definition_file(path: String) -> Result<ReferenceFile, String> {
    read_definition_file(&path).map_err(|e| e.user_message())
}

/// Implementation of [`ai_read_definition_file`] (split out to ease testing).
pub fn read_definition_file(path: &str) -> orbit_ai::AiResult<ReferenceFile> {
    use orbit_ai::AiError;

    let p = std::path::Path::new(path);
    if !p.is_file() {
        return Err(AiError::Invalid(format!(
            "file does not exist or is not a regular file: {path}"
        )));
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !REFERENCE_EXTS.contains(&ext.as_str()) {
        return Err(AiError::Invalid(format!(
            "unsupported file type .{ext} (supported: {})",
            REFERENCE_EXTS.join(" / ")
        )));
    }
    let meta = std::fs::metadata(p).map_err(AiError::Io)?;
    let bytes = meta.len();
    // Hard cap: refuse to read anything over 4MB (to avoid exhausting memory); character-level truncation is handled separately
    const MAX_BYTES: u64 = 4 * 1024 * 1024;
    if bytes > MAX_BYTES {
        return Err(AiError::Invalid(format!(
            "file too large ({:.1} MB, limit 4 MB): split it or save a smaller definition file",
            bytes as f64 / 1_048_576.0
        )));
    }
    let raw = std::fs::read(p).map_err(AiError::Io)?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let truncated = text.chars().count() > MAX_REFERENCE_CHARS;
    let text = if truncated {
        text.chars().take(MAX_REFERENCE_CHARS).collect::<String>()
            + "\n...(file too long, truncated)"
    } else {
        text
    };
    Ok(ReferenceFile {
        name: p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("definition")
            .to_string(),
        path: path.to_string(),
        bytes,
        text,
        truncated,
    })
}

// ─── Turns and authorization ─────────────────────────────────────────

/// Start a turn (non-blocking; returns `turnId` immediately).
#[tauri::command]
pub async fn ai_start_turn(
    state: State<'_, AppState>,
    request: StartTurnRequest,
) -> Result<StartTurnResponse, String> {
    let workspace_id = state
        .data
        .active_workspace_id()
        .unwrap_or_else(|| orbit_data::model::DEFAULT_WORKSPACE_ID.to_string());
    let prefs = state.data.ai_prefs();

    // Session: an empty id = create new
    let mut session = if request.session_id.trim().is_empty() {
        AiSession::new(Some(workspace_id.clone()))
    } else {
        state
            .ai
            .sessions
            .load(&request.session_id)
            .map_err(|e| e.user_message())?
    };
    if session.title.is_empty() || session.title == "New chat" {
        session.title = orbit_ai::session::derive_title(&request.text);
    }
    session.push(orbit_ai::ChatMessage::user(request.text.clone()));
    state
        .ai
        .sessions
        .save(&session)
        .map_err(|e| e.user_message())?;

    // Provider and credentials
    let provider_id = request
        .provider_id
        .clone()
        .or(prefs.provider_id.clone())
        .ok_or_else(|| {
            "no model credential configured; enter an API Key in AI settings first".to_string()
        })?;
    let credential = state
        .ai
        .credentials()?
        .get(&provider_id)
        .cloned()
        .ok_or_else(|| {
            format!("credential {provider_id} does not exist; reconfigure it in AI settings")
        })?;
    let (kind, credential_base) = resolve_provider(&credential);
    let model = request
        .model
        .clone()
        .or_else(|| session.model.clone())
        .unwrap_or_else(|| {
            if prefs.model.trim().is_empty() {
                credential
                    .default_model
                    .clone()
                    .unwrap_or_else(|| kind.default_model().to_string())
            } else {
                prefs.model.clone()
            }
        });
    session.model = Some(model.clone());
    session.provider_id = Some(provider_id.clone());
    // Priority: Base URL in global prefs > the credential's own > the kind's default address
    let base_url = if prefs.base_url.trim().is_empty() {
        credential_base
    } else {
        prefs.base_url.trim().to_string()
    };

    let provider = orbit_ai::build_provider(ProviderConfig {
        id: provider_id,
        kind,
        base_url,
        api_key: credential.api_key.clone(),
        headers: credential.headers.clone(),
        auth_style: credential.auth_style.clone(),
    });

    // Work mode: request > session (what this session used last) > global prefs (new-session default) > Agent
    let is_new_session = request.session_id.trim().is_empty();
    let mode = request
        .mode
        .as_deref()
        .map(orbit_ai::AiMode::from_tag)
        .unwrap_or_else(|| {
            if is_new_session {
                orbit_ai::AiMode::from_tag(&prefs.mode)
            } else {
                session.mode
            }
        });
    session.mode = mode;

    // Tools / prompt / host
    let tools = orbit_ai::tools::catalog::all_tools();
    let language = orbit_ai::Language::from_tag(&prefs.language);
    let system = orbit_ai::prompt::system_prompt(language, &request.context, mode);
    let host = Arc::new(TauriToolHost {
        data: state.data.clone(),
        data_sources: state.data_sources.clone(),
        cookie_jar: state.cookie_jar.clone(),
        workspace_id: workspace_id.clone(),
        app_data_dir: state.ai.app_data_dir.clone(),
        bus: state.ai.bus.clone(),
        sessions: state.ai.sessions.clone(),
        session_id: session.id.clone(),
    });

    let turn_id = uuid::Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    state
        .ai
        .turns
        .lock()
        .await
        .insert(turn_id.clone(), cancel.clone());

    let agent_req = orbit_ai::AgentRunRequest {
        turn_id: turn_id.clone(),
        model: model.clone(),
        system,
        messages: session.messages.clone(),
        tools,
        mode,
        // Temperature is not user-configurable: agent scenarios need stable tool calls, so a low temperature is fixed
        temperature: orbit_ai::DEFAULT_TEMPERATURE,
        max_tokens: prefs.max_tokens,
        context_budget: orbit_ai::session::DEFAULT_CONTEXT_BUDGET,
    };

    let agent = Agent::new(
        provider,
        host,
        Arc::new(UiApprover {
            approvals: state.ai.approvals.clone(),
        }),
        state.ai.bus.sink(None),
        AgentLimits {
            max_rounds: prefs.max_rounds.clamp(1, 20) as usize,
            max_tool_output_chars: 8_000,
        },
    );

    let ai = state.ai.clone();
    let session_id = session.id.clone();
    let task_turn_id = turn_id.clone();
    let task_session_id = session_id.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = agent.run(agent_req, cancel).await;
        // Re-read from disk before appending, to avoid concurrent turns overwriting each other's messages
        let mut updated = ai
            .sessions
            .load(&task_session_id)
            .unwrap_or_else(|_| session.clone());
        for message in outcome.messages {
            updated.messages.push(message);
        }
        updated.updated_at = jiff::Timestamp::now().as_millisecond();
        // Mode is remembered with the session: reopening this session continues with the same working style.
        // Do not overwrite `plan` here: it is written by the host during tool calls and may have just been written.
        updated.mode = mode;
        if let Err(e) = ai.sessions.save(&updated) {
            tracing::warn!(target: "orbit_ai", "failed to save session: {}", e.user_message());
        }
        ai.turns.lock().await.remove(&task_turn_id);
        ai.approvals.lock().await.clear();
    });

    Ok(StartTurnResponse {
        turn_id,
        session_id,
    })
}

/// Abort a turn.
#[tauri::command]
pub async fn ai_abort_turn(state: State<'_, AppState>, turn_id: String) -> Result<bool, String> {
    let turns = state.ai.turns.lock().await;
    match turns.get(&turn_id) {
        Some(token) => {
            token.cancel();
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Decide on a pending tool call (currently only execution-type tools reach here).
#[tauri::command]
pub async fn ai_approve_tool(
    state: State<'_, AppState>,
    call_id: String,
    allow: bool,
    note: Option<String>,
) -> Result<bool, String> {
    let sender = state.ai.approvals.lock().await.remove(&call_id);
    let Some(tx) = sender else {
        return Ok(false);
    };
    let approval = if allow {
        Approval::allow()
    } else {
        Approval::deny(note.unwrap_or_else(|| "the user rejected this operation".into()))
    };
    Ok(tx.send(approval).is_ok())
}

/// Drain and clear the AI event buffer (frontend polling).
#[tauri::command]
pub async fn ai_drain_events(state: State<'_, AppState>) -> Result<Vec<orbit_ai::AiEvent>, String> {
    Ok(state.ai.bus.drain())
}

/// Abort: expose the underlying error type so callers can present a unified message (keeping the `String` error consistent with other commands).
#[allow(dead_code)]
fn to_message(e: &AiError) -> String {
    e.user_message()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temp file and return its path (reclaimed by the system when the test ends).
    fn temp_file(name: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("orbit-ai-ref-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn reads_supported_definition_file() {
        let path = temp_file("petstore.yaml", "openapi: 3.0.0\n");
        let file = read_definition_file(path.to_str().unwrap()).unwrap();
        assert_eq!(file.name, "petstore.yaml");
        assert_eq!(file.text, "openapi: 3.0.0\n");
        assert!(!file.truncated);
        assert_eq!(
            file.bytes, 15,
            "byte count is shown as the file size on chips"
        );
    }

    #[test]
    fn rejects_unknown_extension_with_actionable_message() {
        // The allowlist is half the security boundary: it must not become an "arbitrary file read" tool
        let path = temp_file("secrets.env", "TOKEN=abc");
        let err = read_definition_file(path.to_str().unwrap()).unwrap_err();
        let msg = err.user_message();
        assert!(msg.contains("unsupported file type"), "{msg}");
        assert!(
            msg.contains("json"),
            "must tell the user which types are supported: {msg}"
        );
    }

    #[test]
    fn rejects_missing_path_and_directory() {
        assert!(read_definition_file("").is_err());
        let path = temp_file("x.json", "{}");
        let dir = path.parent().unwrap();
        assert!(read_definition_file(dir.to_str().unwrap()).is_err());
    }

    #[test]
    fn truncates_oversized_content_before_injecting() {
        let big = "a".repeat(MAX_REFERENCE_CHARS + 500);
        let path = temp_file("big.json", &big);
        let file = read_definition_file(path.to_str().unwrap()).unwrap();
        assert!(file.truncated);
        assert!(file.text.chars().count() < big.chars().count());
        assert!(file.text.ends_with("truncated)"));
    }
}
