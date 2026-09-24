//! Tauri commands: performance reports & baseline management (delegates to the orbit-server reports module)

use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

use orbit_metrics::{ErrorGroup, MetricsSummary};

/// Save-report request (from the frontend)
#[derive(Debug, Deserialize)]
pub struct SaveReportRequest {
    pub name: String,
    pub endpoint: String,
    pub method: String,
    pub vus: u32,
    pub duration: String,
    #[serde(default)]
    pub config: Option<String>,
    pub summary: Value,
    pub thresholds: Value,
    pub all_thresholds_passed: bool,
    /// Owning workspace (reports are isolated per workspace)
    #[serde(default)]
    pub workspace_id: Option<String>,
}

/// Save a report
#[tauri::command]
pub fn save_report(req: SaveReportRequest) -> Result<Value, String> {
    let id = format!(
        "rpt-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let created_at = jiff::Timestamp::now().to_string();

    let report = orbit_server::reports::SavedReport {
        id: id.clone(),
        name: req.name,
        workspace_id: req.workspace_id,
        endpoint: req.endpoint,
        method: req.method,
        created_at,
        vus: req.vus,
        duration: req.duration,
        config: req.config,
        summary: req.summary,
        thresholds: req.thresholds,
        all_thresholds_passed: req.all_thresholds_passed,
        is_baseline: false,
        baseline_name: None,
    };

    orbit_server::reports::save_report(&report)?;
    Ok(serde_json::json!({ "status": "ok", "id": id }))
}

/// List reports (default = all; pass workspace_id = filter by workspace)
#[tauri::command]
pub fn list_reports(workspace_id: Option<String>) -> Result<Value, String> {
    let reports = match workspace_id {
        Some(ws) => orbit_server::reports::list_reports_in(&ws)?,
        None => orbit_server::reports::list_reports()?,
    };
    serde_json::to_value(reports).map_err(|e| e.to_string())
}

/// Load a single report
#[tauri::command]
pub fn load_report(id: String) -> Result<Value, String> {
    let report = orbit_server::reports::load_report(&id)?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

/// Delete a report
#[tauri::command]
pub fn delete_report(id: String) -> Result<Value, String> {
    orbit_server::reports::delete_report(&id)?;
    Ok(serde_json::json!({ "status": "ok" }))
}

#[derive(Debug, Deserialize)]
pub struct SetBaselineRequest {
    pub baseline_name: String,
}

/// Set as baseline
#[tauri::command]
pub fn set_baseline(id: String, req: SetBaselineRequest) -> Result<Value, String> {
    orbit_server::reports::set_baseline(&id, &req.baseline_name)?;
    Ok(serde_json::json!({ "status": "ok" }))
}

/// Unset baseline
#[tauri::command]
pub fn unset_baseline(id: String) -> Result<Value, String> {
    orbit_server::reports::unset_baseline(&id)?;
    Ok(serde_json::json!({ "status": "ok" }))
}

/// List all baselines (default = all; pass workspace_id = filter by workspace)
#[tauri::command]
pub fn list_baselines(workspace_id: Option<String>) -> Result<Value, String> {
    let baselines = match workspace_id {
        Some(ws) => orbit_server::reports::list_baselines_in(&ws)?,
        None => orbit_server::reports::list_baselines()?,
    };
    serde_json::to_value(baselines).map_err(|e| e.to_string())
}

/// Rebuild MetricsSummary from a saved report's summary JSON (frontend field names) for the exporter.
fn summary_from_json(v: &Value) -> MetricsSummary {
    let f = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let u = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    let timing = v.get("timing");
    let tf = |k: &str| {
        timing
            .and_then(|t| t.get(k))
            .and_then(|x| x.as_f64())
            .unwrap_or(0.0)
    };
    let error_breakdown = v
        .get("error_breakdown")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| {
                    Some(ErrorGroup {
                        error_type: g.get("type")?.as_str()?.to_string(),
                        count: g.get("count")?.as_u64()?,
                        sample: g
                            .get("sample")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    MetricsSummary {
        total_requests: u("total_requests"),
        total_errors: u("total_failures"),
        error_rate: f("error_rate"),
        rps: f("rps"),
        duration: Duration::from_millis(u("total_duration_ms")),
        p50_ms: f("p50_ms"),
        p90_ms: f("p90_ms"),
        p95_ms: f("p95_ms"),
        p99_ms: f("p99_ms"),
        p999_ms: f("p999_ms"),
        min_ms: f("min_ms"),
        max_ms: f("max_ms"),
        mean_ms: f("mean_ms"),
        total_bytes: u("total_bytes"),
        total_messages: 0,
        avg_dns_ms: tf("dns_ms"),
        avg_tcp_ms: tf("tcp_ms"),
        avg_tls_ms: tf("tls_ms"),
        avg_send_ms: tf("send_ms"),
        avg_ttfb_ms: tf("ttfb_ms"),
        avg_download_ms: tf("download_ms"),
        timed_count: u("total_requests"),
        error_breakdown,
    }
}

/// Export a saved performance report (html/json/csv/junit).
/// Saved reports contain only the summary, not raw samples, so jtl/raw-json must be exported from the load-test result page.
#[tauri::command]
pub fn export_saved_report(id: String, format: String) -> Result<Value, String> {
    let report = orbit_server::reports::load_report(&id).map_err(|e| e.to_string())?;
    let summary = summary_from_json(&report.summary);
    let Some(format) = orbit_output::ExportFormat::parse(&format) else {
        return Err(format!("unsupported export format: {format}"));
    };
    if matches!(
        format,
        orbit_output::ExportFormat::Jtl | orbit_output::ExportFormat::RawJsonl
    ) {
        return Err("saved reports do not contain raw samples; export JTL / JSONL from the load-test result page".to_string());
    }
    let ctx = orbit_output::ExportContext::new().with_test_name(&report.name);
    let report = orbit_output::export(format, &summary, &ctx).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "content": report.content,
        "filename": report.filename,
        "truncated": false,
    }))
}
