//! WASM plugin management commands: list / scan / load / unload / protocols / codecs.
//!
//! Behaviorally consistent with the orbit-server HTTP API (`/api/plugins/*`), callable from the Tauri desktop app.

use base64::Engine;
use std::path::Path;
use tauri::State;

use crate::state::AppState;

/// Plugin descriptor serialized to the frontend
#[derive(serde::Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub kind: String,
    pub status: String,
    pub error: Option<String>,
    pub protocols: Vec<String>,
    pub codecs: Vec<String>,
}

impl From<&orbit_plugin::PluginDescriptor> for PluginInfo {
    fn from(d: &orbit_plugin::PluginDescriptor) -> Self {
        Self {
            id: d.id.clone(),
            name: d.name.clone(),
            version: d.version.clone(),
            description: d.description.clone(),
            kind: d.kind.clone(),
            status: d.status.clone(),
            error: d.error.clone(),
            protocols: d.protocols.clone(),
            codecs: d.codecs.clone(),
        }
    }
}

#[tauri::command]
pub async fn plugin_list(state: State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    let mgr = state.plugins.lock().await;
    Ok(mgr.list().iter().map(PluginInfo::from).collect())
}

#[tauri::command]
pub async fn plugin_scan(
    dir: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let mut mgr = state.plugins.lock().await;
    let report = mgr.scan_dir(Path::new(&dir)).await;
    Ok(serde_json::json!({
        "scanned": report.scanned,
        "loaded": report.loaded,
        "failed": report.failed,
    }))
}

#[tauri::command]
pub async fn plugin_load(
    id: String,
    wasm_base64: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&wasm_base64)
        .map_err(|e| format!("base64 decode: {}", e))?;
    let mut mgr = state.plugins.lock().await;
    match mgr.load_wasm(&id, &bytes, None).await {
        Ok((kind, ids)) => Ok(serde_json::json!({ "kind": kind, "capabilities": ids })),
        Err(e) => Err(e),
    }
}

/// Load a native (dynamic library) protocol plugin.
#[tauri::command]
pub async fn plugin_native_load(
    id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let mut mgr = state.plugins.lock().await;
    mgr.load_native(&id, std::path::Path::new(&path))?;
    let protocols = mgr
        .get(&id)
        .map(|d| d.protocols.clone())
        .unwrap_or_default();
    Ok(serde_json::json!({ "status": "loaded", "id": id, "protocols": protocols }))
}

#[tauri::command]
pub async fn plugin_unload(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut mgr = state.plugins.lock().await;
    mgr.uninstall(&state.plugins_root, &id)
}

/// Install a zip plugin package into `<plugins_root>/<id>` (with validation and zip-slip protection).
#[tauri::command]
pub async fn plugin_install(
    id: String,
    zip_base64: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&zip_base64)
        .map_err(|e| format!("base64 decode: {}", e))?;
    let mut mgr = state.plugins.lock().await;
    match mgr.install_zip(&state.plugins_root, &bytes).await {
        Ok((kind, ids)) => Ok(serde_json::json!({
            "status": "installed", "id": id, "kind": kind, "capabilities": ids
        })),
        Err(e) => Err(e),
    }
}

/// Enable a plugin (reload from the install dir and register).
#[tauri::command]
pub async fn plugin_enable(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut mgr = state.plugins.lock().await;
    mgr.enable(&id).await
}

/// Disable a plugin (unregister + destroy; the directory is kept).
#[tauri::command]
pub async fn plugin_disable(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut mgr = state.plugins.lock().await;
    mgr.disable(&id)
}

#[tauri::command]
pub async fn plugin_protocols() -> Result<Vec<String>, String> {
    Ok(orbit_protocol::registry::list_protocol_ids())
}

#[tauri::command]
pub async fn plugin_codecs() -> Result<Vec<String>, String> {
    Ok(orbit_codec::registry::list_codec_names())
}

/// Protocol catalog (built-in + plugin protocols, with dynamic form schemas), aligned with `/api/protocols`.
#[tauri::command]
pub async fn plugin_protocol_catalog(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut protocols: Vec<serde_json::Value> = orbit_protocol::registry::BUILTIN_PROTOCOL_IDS
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id,
                "builtin": true,
                "connectionConfigSchema": null,
                "requestConfigSchema": null,
            })
        })
        .collect();
    let mgr = state.plugins.lock().await;
    for (pid, conn_schema, req_schema) in mgr.protocol_schemas() {
        protocols.push(serde_json::json!({
            "id": pid,
            "builtin": false,
            "connectionConfigSchema": conn_schema,
            "requestConfigSchema": req_schema,
        }));
    }
    protocols.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Ok(protocols)
}

/// Codec catalog (built-in + plugin codecs), aligned with `/api/codecs`.
#[tauri::command]
pub async fn plugin_codec_catalog(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut codecs: Vec<serde_json::Value> = orbit_codec::registry::builtin_codec_names()
        .into_iter()
        .map(|name| serde_json::json!({ "name": name, "builtin": true }))
        .collect();
    let mgr = state.plugins.lock().await;
    for name in mgr.codec_names() {
        codecs.push(serde_json::json!({ "name": name, "builtin": false }));
    }
    codecs.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(codecs)
}
