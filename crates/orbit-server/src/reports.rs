//! Performance report persistence: save, list, baseline management.
//!
//! Reports are stored in `~/.orbit/reports/`, one JSON file per report.
//! A baseline is implemented by setting `is_baseline: true`; each endpoint keeps only the most recent baseline.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// Report storage directory
fn reports_dir() -> PathBuf {
    dirs_next()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".orbit")
        .join("reports")
}

/// Ensure the directory exists
fn ensure_dir() -> std::io::Result<()> {
    let dir = reports_dir();
    fs::create_dir_all(&dir)
}

/// Persisted report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedReport {
    pub id: String,
    pub name: String,
    /// Owning workspace (isolated per workspace; old reports without this field -> default workspace)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// e.g. "GET /api/health"
    pub endpoint: String,
    pub method: String,
    /// ISO 8601 creation time
    pub created_at: String,
    pub vus: u32,
    pub duration: String,
    /// Human-readable load config description (e.g. the stages of ramping-vus); falls back to vus/duration when empty in old reports
    #[serde(default)]
    pub config: Option<String>,
    /// frontend_summary JSON
    pub summary: Value,
    /// threshold_results_json
    pub thresholds: Value,
    pub all_thresholds_passed: bool,
    pub is_baseline: bool,
    pub baseline_name: Option<String>,
}

/// Save a report
pub fn save_report(report: &SavedReport) -> Result<(), String> {
    ensure_dir().map_err(|e| format!("failed to create report directory: {}", e))?;
    let path = reports_dir().join(format!("{}.json", report.id));
    let json = serde_json::to_string_pretty(report)
        .map_err(|e| format!("failed to serialize report: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("failed to write report: {}", e))?;
    tracing::info!("Report saved: {} -> {}", report.id, path.display());
    Ok(())
}

/// List all reports
pub fn list_reports() -> Result<Vec<SavedReport>, String> {
    ensure_dir().map_err(|e| format!("failed to create report directory: {}", e))?;
    let dir = reports_dir();
    let mut reports = Vec::new();
    let entries =
        fs::read_dir(&dir).map_err(|e| format!("failed to read report directory: {}", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            match fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<SavedReport>(&content) {
                    Ok(r) => reports.push(r),
                    Err(e) => tracing::warn!("skipping corrupted report {}: {}", path.display(), e),
                },
                Err(e) => tracing::warn!("failed to read report {}: {}", path.display(), e),
            }
        }
    }
    reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(reports)
}

/// Report ownership for a workspace: old reports (without workspace_id) belong to the default workspace
fn report_ws(r: &SavedReport) -> &str {
    r.workspace_id
        .as_deref()
        .unwrap_or(orbit_data::DEFAULT_WORKSPACE_ID)
}

/// Filter reports by workspace
pub fn list_reports_in(ws_id: &str) -> Result<Vec<SavedReport>, String> {
    Ok(list_reports()?
        .into_iter()
        .filter(|r| report_ws(r) == ws_id)
        .collect())
}

/// Load a single report
pub fn load_report(id: &str) -> Result<SavedReport, String> {
    let path = reports_dir().join(format!("{}.json", id));
    let content = fs::read_to_string(&path).map_err(|_| format!("report not found: {}", id))?;
    serde_json::from_str(&content).map_err(|e| format!("failed to parse report: {}", e))
}

/// Delete a report
pub fn delete_report(id: &str) -> Result<(), String> {
    let path = reports_dir().join(format!("{}.json", id));
    fs::remove_file(&path).map_err(|_| format!("failed to delete report: {}", id))
}

/// Set a report as the baseline (replacing the old baseline for the same endpoint)
pub fn set_baseline(id: &str, baseline_name: &str) -> Result<(), String> {
    // 1. Load the target report
    let mut target = load_report(id)?;

    // 2. Clear the old baseline for the same endpoint
    let all = list_reports()?;
    for mut old in all {
        if old.endpoint == target.endpoint && old.is_baseline && old.id != id {
            old.is_baseline = false;
            old.baseline_name = None;
            save_report(&old)?;
        }
    }

    // 3. Mark the target as baseline
    target.is_baseline = true;
    target.baseline_name = Some(baseline_name.to_string());
    save_report(&target)?;

    tracing::info!(
        "Baseline set: {} ({}) for {}",
        baseline_name,
        id,
        target.endpoint
    );
    Ok(())
}

/// Unset the baseline flag
pub fn unset_baseline(id: &str) -> Result<(), String> {
    let mut target = load_report(id)?;
    target.is_baseline = false;
    target.baseline_name = None;
    save_report(&target)?;
    Ok(())
}

/// Get the current baseline for the given endpoint
pub fn get_baseline(endpoint: &str) -> Option<SavedReport> {
    match list_reports() {
        Ok(reports) => reports
            .into_iter()
            .find(|r| r.endpoint == endpoint && r.is_baseline),
        Err(_) => None,
    }
}

/// List all baselines
pub fn list_baselines() -> Result<Vec<SavedReport>, String> {
    let all = list_reports()?;
    Ok(all.into_iter().filter(|r| r.is_baseline).collect())
}

/// Filter baselines by workspace
pub fn list_baselines_in(ws_id: &str) -> Result<Vec<SavedReport>, String> {
    Ok(list_baselines()?
        .into_iter()
        .filter(|r| report_ws(r) == ws_id)
        .collect())
}

/// Data directory path (for external use)
fn dirs_next() -> Option<PathBuf> {
    // Prefer HOME / USERPROFILE
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_test_report(id: &str, endpoint: &str) -> SavedReport {
        SavedReport {
            id: id.to_string(),
            name: "Test Run".to_string(),
            workspace_id: None,
            endpoint: endpoint.to_string(),
            method: "GET".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            vus: 10,
            duration: "30s".to_string(),
            config: None,
            summary: json!({"rps": 100.0, "p95_ms": 50.0}),
            thresholds: json!([]),
            all_thresholds_passed: true,
            is_baseline: false,
            baseline_name: None,
        }
    }

    #[test]
    fn test_save_and_load() {
        let report = make_test_report("test-001", "GET /test");
        save_report(&report).unwrap();
        let loaded = load_report("test-001").unwrap();
        assert_eq!(loaded.name, "Test Run");
        // cleanup
        let _ = delete_report("test-001");
    }

    #[test]
    fn test_list_and_delete() {
        let r1 = make_test_report("test-list-1", "GET /a");
        let r2 = make_test_report("test-list-2", "GET /b");
        save_report(&r1).unwrap();
        save_report(&r2).unwrap();

        let list = list_reports().unwrap();
        assert!(list.len() >= 2);

        delete_report("test-list-1").unwrap();
        delete_report("test-list-2").unwrap();

        // Verify deleted
        assert!(load_report("test-list-1").is_err());
    }

    #[test]
    fn test_workspace_filter() {
        let mut r1 = make_test_report("test-ws-1", "GET /ws-a");
        r1.workspace_id = Some("ws-a".into());
        let mut r2 = make_test_report("test-ws-2", "GET /ws-b");
        r2.workspace_id = Some("ws-b".into());
        // Old report: no workspace_id -> belongs to default
        let r3 = make_test_report("test-ws-3", "GET /default");
        save_report(&r1).unwrap();
        save_report(&r2).unwrap();
        save_report(&r3).unwrap();

        let a = list_reports_in("ws-a").unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].id, "test-ws-1");

        // Default workspace filtering includes old reports (no workspace_id); parallel tests may have other default-owned reports, so use a contains assertion
        let def = list_reports_in(orbit_data::DEFAULT_WORKSPACE_ID).unwrap();
        assert!(
            def.iter().any(|r| r.id == "test-ws-3"),
            "old report should belong to the default workspace"
        );

        delete_report("test-ws-1").unwrap();
        delete_report("test-ws-2").unwrap();
        delete_report("test-ws-3").unwrap();
    }

    #[test]
    fn test_baseline_cycle() {
        let r1 = make_test_report("test-bl-1", "GET /bl");
        save_report(&r1).unwrap();
        set_baseline("test-bl-1", "v1.0").unwrap();

        let bl = get_baseline("GET /bl").unwrap();
        assert!(bl.is_baseline);
        assert_eq!(bl.baseline_name.as_deref(), Some("v1.0"));

        // Replace with new baseline
        let r2 = make_test_report("test-bl-2", "GET /bl");
        save_report(&r2).unwrap();
        set_baseline("test-bl-2", "v2.0").unwrap();

        let bl2 = get_baseline("GET /bl").unwrap();
        assert_eq!(bl2.id, "test-bl-2");
        assert_eq!(bl2.baseline_name.as_deref(), Some("v2.0"));

        // Old baseline is cleared
        let old = load_report("test-bl-1").unwrap();
        assert!(!old.is_baseline);

        // Unset baseline
        unset_baseline("test-bl-2").unwrap();
        assert!(get_baseline("GET /bl").is_none());

        // Cleanup
        delete_report("test-bl-1").unwrap();
        delete_report("test-bl-2").unwrap();
    }
}
