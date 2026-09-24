//! Tauri commands: local snapshot persistence (full-database JSON).
//!
//! Shares the same snapshot structure as the browser-side localStorage (schemaVersion=1),
//! The unit of future remote sync is this JSON document. The existing SQLite (orbit_data.db) is left as is.

use crate::state::AppState;
use orbit_server::mock::MockInterface;
use std::path::PathBuf;
use tauri::Manager;
use tauri::State;

/// Snapshot file path: app_data_dir/orbit_data.json
fn snapshot_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to get data dir: {}", e))?;
    Ok(dir.join("orbit_data.json"))
}

/// Atomically write the snapshot: write .tmp first, then replace the target.
/// Note: on Windows `fs::rename` cannot overwrite an existing target, so delete the old file before renaming.
#[tauri::command]
pub fn save_snapshot(app: tauri::AppHandle, json: String) -> Result<(), String> {
    let path = snapshot_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())
        .map_err(|e| format!("failed to write snapshot temp file: {}", e))?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("failed to remove old snapshot: {}", e))?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| format!("failed to replace snapshot: {}", e))?;
    Ok(())
}

/// Read the snapshot; returns None if the file is missing; if the JSON is corrupt, rename it to .bak and treat as no snapshot.
#[tauri::command]
pub fn load_snapshot(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let path = snapshot_path(&app)?;
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("failed to read snapshot: {}", e))?;
    // Validate JSON; if corrupt, archive as .bak and treat as no snapshot (frontend falls back to seed)
    if serde_json::from_str::<serde_json::Value>(&content).is_err() {
        let bak = path.with_extension("json.bak");
        let _ = std::fs::rename(&path, &bak);
        return Ok(None);
    }
    Ok(Some(content))
}

/// Delete the local snapshot (including a leftover .tmp); used by "Clear data / Restore seed".
#[tauri::command]
pub fn clear_snapshot(app: tauri::AppHandle) -> Result<(), String> {
    let path = snapshot_path(&app)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("failed to delete snapshot: {}", e))?;
    }
    let tmp = path.with_extension("json.tmp");
    if tmp.exists() {
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(())
}

/// Export the snapshot to a user-specified path (Data Management panel "Export snapshot", used with dialog save).
#[tauri::command]
pub fn export_snapshot(path: String, json: String) -> Result<(), String> {
    if path.is_empty() {
        return Err("export path is empty".into());
    }
    if let Some(parent) = std::path::Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("failed to create dir: {}", e))?;
        }
    }
    std::fs::write(&path, json.as_bytes())
        .map_err(|e| format!("failed to write snapshot file: {}", e))?;
    Ok(())
}

/// Restore Mock rules into the in-memory MockServerHandle; if the service is running, also sync live (enabled rules only).
#[tauri::command]
pub async fn restore_mock_rules(
    rules: Vec<MockInterface>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let handle = state.mock_server.write().await;
    {
        let mut cur = handle.rules.write().await;
        *cur = rules;
    }
    if handle.running {
        let live: Vec<MockInterface> = handle
            .rules
            .read()
            .await
            .iter()
            .filter(|r| r.enabled)
            .cloned()
            .collect();
        *handle.live.write().await = live;
    }
    Ok(())
}
