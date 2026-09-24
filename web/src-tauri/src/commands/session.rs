//! Long-connection session commands (interactive debugging for non-HTTP protocols).
//! Reuses orbit-server's SessionManager; events are pushed via `session-event`.

use base64::Engine;
use serde::Deserialize;
use tauri::State;

use orbit_server::session::{OpenSessionRequest, SessionManager};

#[derive(Debug, Deserialize)]
pub struct SessionSendRequest {
    pub session_id: String,
    /// base64-encoded message payload
    pub data: String,
    /// Per-message pre-script (optional; overrides the session default script)
    pub pre_script: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionCloseRequest {
    pub session_id: String,
}

#[tauri::command]
pub async fn session_open(
    state: State<'_, SessionManager>,
    request: OpenSessionRequest,
) -> Result<serde_json::Value, String> {
    let resp = state.open(request).await?;
    Ok(serde_json::to_value(resp).map_err(|e| e.to_string())?)
}

#[tauri::command]
pub async fn session_send(
    state: State<'_, SessionManager>,
    request: SessionSendRequest,
) -> Result<serde_json::Value, String> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(&request.data)
        .map_err(|e| format!("base64 decode failed: {e}"))?;
    let seq = state
        .send(&request.session_id, data, request.pre_script)
        .await?;
    Ok(serde_json::json!({ "ok": true, "seq": seq }))
}

#[tauri::command]
pub async fn session_close(
    state: State<'_, SessionManager>,
    request: SessionCloseRequest,
) -> Result<serde_json::Value, String> {
    state.close(&request.session_id).await?;
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub fn session_messages(
    state: State<'_, SessionManager>,
    session_id: String,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(state.messages(&session_id)).map_err(|e| e.to_string())
}

/// gRPC Server Reflection service discovery ({ url } → { services })
#[tauri::command]
pub async fn grpc_reflect(url: String) -> Result<serde_json::Value, String> {
    let services = orbit_server::session::grpc_reflect(&url).await?;
    serde_json::to_value(services).map_err(|e| e.to_string())
}
