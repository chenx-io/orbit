//! Tauri commands: persist automation scenario run reports (app_data_dir/scenario_reports/<id>.json).
//!
//! Kept isolated from load-test reports (orbit_server::reports, ~/.orbit/reports):
//! scenario reports are aggregated by the frontend; this only does file CRUD + capacity trimming (keeps the newest MAX_KEEP entries).

use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use tauri::Manager;

/// Number of newest reports to keep (oldest evicted in ascending startedAt order once exceeded)
const MAX_KEEP: usize = 200;

fn reports_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to get data dir: {}", e))?;
    Ok(dir.join("scenario_reports"))
}

#[derive(Debug, Deserialize)]
pub struct SaveScenarioReportRequest {
    /// Full report object (serialized frontend ScenarioRunRecord)
    pub report: Value,
    #[serde(default)]
    pub workspace_id: Option<String>,
}

/// Save a run report
#[tauri::command]
pub fn save_scenario_report(
    app: tauri::AppHandle,
    req: SaveScenarioReportRequest,
) -> Result<Value, String> {
    let id = req
        .report
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "report is missing the id field".to_string())?
        .to_string();
    let dir = reports_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create reports dir: {}", e))?;

    let mut report = req.report;
    if report.get("workspaceId").is_none() {
        if let Some(ws) = req.workspace_id {
            report["workspaceId"] = Value::String(ws);
        }
    }
    let body = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.json", id));
    let tmp = dir.join(format!("{}.json.tmp", id));
    std::fs::write(&tmp, body.as_bytes()).map_err(|e| format!("failed to write report: {}", e))?;
    // On Windows, rename cannot overwrite an existing file, so delete first
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("failed to replace old report: {}", e))?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| format!("failed to save report: {}", e))?;

    trim_reports(&dir)?;
    Ok(serde_json::json!({ "status": "ok", "id": id }))
}

/// List reports (newest startedAt first); empty workspace_id = all
#[tauri::command]
pub fn list_scenario_reports(
    app: tauri::AppHandle,
    workspace_id: Option<String>,
) -> Result<Value, String> {
    let dir = reports_dir(&app)?;
    if !dir.exists() {
        return Ok(Value::Array(vec![]));
    }
    let mut items: Vec<Value> = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let value: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(ws) = &workspace_id {
            let own = value.get("workspaceId").and_then(|v| v.as_str());
            if own != Some(ws.as_str()) {
                continue;
            }
        }
        items.push(value);
    }
    items.sort_by(|a, b| {
        let ta = a.get("startedAt").and_then(|v| v.as_i64()).unwrap_or(0);
        let tb = b.get("startedAt").and_then(|v| v.as_i64()).unwrap_or(0);
        tb.cmp(&ta)
    });
    // The list returns only summary fields to avoid reading all details at once
    let summaries: Vec<Value> = items
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get("id"),
                "workspaceId": r.get("workspaceId"),
                "targetType": r.get("targetType"),
                "targetId": r.get("targetId"),
                "targetName": r.get("targetName"),
                "runMode": r.get("runMode"),
                "startedAt": r.get("startedAt"),
                "durationMs": r.get("durationMs"),
                "envName": r.get("envName"),
                "status": r.get("status"),
                "totalPass": r.get("totalPass"),
                "totalFail": r.get("totalFail"),
                "totalSkip": r.get("totalSkip"),
                "caseCount": r.get("cases").and_then(|c| c.as_array()).map(|a| a.len()).unwrap_or(0),
            })
        })
        .collect();
    Ok(Value::Array(summaries))
}

/// Load a single report's details
#[tauri::command]
pub fn load_scenario_report(app: tauri::AppHandle, id: String) -> Result<Value, String> {
    let path = reports_dir(&app)?.join(format!("{}.json", id));
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("failed to read report: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("failed to parse report: {}", e))
}

/// Delete a single report
#[tauri::command]
pub fn delete_scenario_report(app: tauri::AppHandle, id: String) -> Result<Value, String> {
    let path = reports_dir(&app)?.join(format!("{}.json", id));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("failed to delete report: {}", e))?;
    }
    Ok(serde_json::json!({ "status": "ok" }))
}

/// Clear all reports
#[tauri::command]
pub fn clear_scenario_reports(app: tauri::AppHandle) -> Result<Value, String> {
    let dir = reports_dir(&app)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("failed to clear reports: {}", e))?;
    }
    Ok(serde_json::json!({ "status": "ok" }))
}

/// Capacity trim: keep the newest MAX_KEEP entries (oldest evicted in ascending startedAt order)
fn trim_reports(dir: &PathBuf) -> Result<(), String> {
    let mut entries: Vec<(i64, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let started = std::fs::read_to_string(&path)
            .ok()
            .and_then(|c| serde_json::from_str::<Value>(&c).ok())
            .and_then(|v| v.get("startedAt").and_then(|s| s.as_i64()))
            .unwrap_or_else(|| {
                entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0)
            });
        entries.push((started, path));
    }
    if entries.len() <= MAX_KEEP {
        return Ok(());
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let remove_count = entries.len() - MAX_KEEP;
    for (_, path) in entries.into_iter().take(remove_count) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}
