//! Data commands: the frontend reads/writes the backend DataService via snapshot sync + module-level commands.
//!
//! The backend DataService is the authoritative store (app_data_dir/orbit_data.json):
//! - Snapshot sync: pull on startup to hydrate the store; debounce-push the whole snapshot on change (with an optimistic-lock baseline to prevent overwrites)
//! - Module-level commands: low-value, high-frequency modules like history use fine-grained commands (avoiding redundant full-snapshot pushes)
//! - Export: the backend queries its own data directly (the frontend only passes the range)

use crate::state::AppState;
use tauri::State;

/// Fetch the current snapshot JSON; on first launch (no existing data) returns null, and the frontend pushes the first one after seeding.
#[tauri::command]
pub fn data_load_snapshot(state: State<'_, AppState>) -> Result<Option<String>, String> {
    if !state.data.had_initial() {
        return Ok(None);
    }
    state.data.to_json().map(Some).map_err(|e| e.to_string())
}

/// Push snapshot JSON (debounced save): deserialize + version check + replace memory + persist to disk.
/// `base_saved_at` is the latest save time seen by the pusher (optimistic lock); default = 0 (first time / no check).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSnapshotRequest {
    pub json: String,
    pub base_saved_at: Option<i64>,
}

#[tauri::command]
pub async fn data_save_snapshot(
    state: State<'_, AppState>,
    request: SaveSnapshotRequest,
) -> Result<(), String> {
    let base = request.base_saved_at.unwrap_or(0);
    state
        .data
        .load_from_json_checked(&request.json, base)
        .await
        .map_err(|e| e.to_string())?;
    // After the snapshot lands, sync data sources to the runtime registry (register new / clean up deleted)
    let cfgs = state.data.data_sources();
    state.data_sources.register_all(&cfgs).await;
    let registered: Vec<String> = state
        .data_sources
        .list()
        .await
        .into_iter()
        .map(|c| c.id)
        .collect();
    for id in registered {
        if !cfgs.iter().any(|c| c.id == id) {
            state.data_sources.remove(&id).await;
        }
    }
    Ok(())
}

/// Clear data (Data Management panel "Clear / Restore seed"): delete the storage file, keep an empty snapshot in memory.
#[tauri::command]
pub async fn data_clear(state: State<'_, AppState>) -> Result<(), String> {
    // Reset in-memory state to empty (until the frontend pushes the first seed); delete the storage file
    state.data.clear_storage().await.map_err(|e| e.to_string())
}

// ─── History (module-level commands rollout) ────────────────────

/// History list (for frontend hydration on startup)
#[tauri::command]
pub fn data_list_history(state: State<'_, AppState>) -> Vec<orbit_data::PersistedHistoryEntry> {
    state.data.history()
}

/// Append a history entry (fire-and-forget after sending a request)
#[tauri::command]
pub async fn data_add_history(
    state: State<'_, AppState>,
    entry: orbit_data::PersistedHistoryEntry,
) -> Result<(), String> {
    state
        .data
        .add_history(entry)
        .await
        .map_err(|e| e.to_string())
}

/// Clear history
#[tauri::command]
pub async fn data_clear_history(state: State<'_, AppState>) -> Result<(), String> {
    state.data.clear_history().await.map_err(|e| e.to_string())
}

// ─── Domain query / write commands (paving the way for a leaner frontend; complete API surface) ────────

#[tauri::command]
pub fn data_list_collections(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Vec<orbit_data::Collection> {
    state.data.collections_in(&workspace_id)
}

#[tauri::command]
pub fn data_list_requests(state: State<'_, AppState>) -> Vec<(String, orbit_data::ApiRequest)> {
    state.data.requests()
}

#[tauri::command]
pub fn data_list_models(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Vec<orbit_data::DataModel> {
    state.data.models_in(&workspace_id)
}

#[tauri::command]
pub fn data_list_environments(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Vec<orbit_data::Environment> {
    state.data.environments_in(&workspace_id)
}

#[tauri::command]
pub fn data_list_scenarios(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Vec<orbit_data::Scenario> {
    state.data.scenarios_in(&workspace_id)
}

#[tauri::command]
pub async fn data_upsert_collection(
    state: State<'_, AppState>,
    collection: orbit_data::Collection,
) -> Result<(), String> {
    state
        .data
        .upsert_collection(collection)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn data_remove_collection(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .data
        .remove_collection(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn data_upsert_request(
    state: State<'_, AppState>,
    request: orbit_data::ApiRequest,
) -> Result<(), String> {
    state
        .data
        .upsert_request(request)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn data_remove_request(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .data
        .remove_request(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn data_upsert_model(
    state: State<'_, AppState>,
    model: orbit_data::DataModel,
) -> Result<(), String> {
    state
        .data
        .upsert_model(model)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn data_remove_model(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .data
        .remove_model(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn data_upsert_environment(
    state: State<'_, AppState>,
    environment: orbit_data::Environment,
) -> Result<(), String> {
    state
        .data
        .upsert_environment(environment)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn data_remove_environment(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .data
        .remove_environment(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn data_upsert_scenario(
    state: State<'_, AppState>,
    scenario: orbit_data::Scenario,
) -> Result<(), String> {
    state
        .data
        .upsert_scenario(scenario)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn data_remove_scenario(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .data
        .remove_scenario(&id)
        .await
        .map_err(|e| e.to_string())
}

// ─── Workspace (project boundary; data authority in the backend) ──────────

/// All workspaces (selection page / management page)
#[tauri::command]
pub fn data_list_workspaces(state: State<'_, AppState>) -> Vec<orbit_data::Workspace> {
    state.data.workspaces()
}

/// Currently active workspace id (null = none selected / first launch)
#[tauri::command]
pub fn data_active_workspace_id(state: State<'_, AppState>) -> Option<String> {
    state.data.active_workspace_id()
}

/// Create a workspace (returns the creation result)
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddWorkspaceRequest {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
}

#[tauri::command]
pub async fn data_add_workspace(
    state: State<'_, AppState>,
    request: AddWorkspaceRequest,
) -> Result<orbit_data::Workspace, String> {
    state
        .data
        .add_workspace(
            &request.name,
            request.description.as_deref(),
            request.color.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameWorkspaceRequest {
    pub id: String,
    pub name: String,
}

#[tauri::command]
pub async fn data_rename_workspace(
    state: State<'_, AppState>,
    request: RenameWorkspaceRequest,
) -> Result<(), String> {
    state
        .data
        .rename_workspace(&request.id, &request.name)
        .await
        .map_err(|e| e.to_string())
}

/// Delete a workspace (the backend cascades cleanup of all its data: collections/models/environments/scenarios/history/Mock)
#[tauri::command]
pub async fn data_remove_workspace(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .data
        .remove_workspace(&id)
        .await
        .map_err(|e| e.to_string())
}

/// Set the currently active workspace
#[tauri::command]
pub async fn data_set_active_workspace(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    state
        .data
        .set_active_workspace(&id)
        .await
        .map_err(|e| e.to_string())
}

/// workspace data stats (selection page cards: collections/models/environments/scenarios/history)
#[tauri::command]
pub fn data_workspace_stats(state: State<'_, AppState>, id: String) -> orbit_data::WorkspaceStats {
    state.data.workspace_stats(&id)
}

/// Activate an environment (remembered per workspace)
#[tauri::command]
pub async fn data_set_active_env(
    state: State<'_, AppState>,
    workspace_id: String,
    id: Option<String>,
) -> Result<(), String> {
    state
        .data
        .set_active_env(&workspace_id, id)
        .await
        .map_err(|e| e.to_string())
}

/// Global variables (per workspace)
#[tauri::command]
pub async fn data_set_global_variables(
    state: State<'_, AppState>,
    workspace_id: String,
    vars: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    state
        .data
        .set_global_variables(&workspace_id, vars)
        .await
        .map_err(|e| e.to_string())
}

/// Global secrets (per workspace)
#[tauri::command]
pub async fn data_set_global_secrets(
    state: State<'_, AppState>,
    workspace_id: String,
    secrets: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    state
        .data
        .set_global_secrets(&workspace_id, secrets)
        .await
        .map_err(|e| e.to_string())
}
