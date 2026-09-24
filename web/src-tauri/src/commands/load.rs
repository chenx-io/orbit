//! Load-test commands: call the real Rust backend on the Tauri desktop (reusing orbit-server's LoadRunner).
//!
//! - `run_load_test`: **non-blocking**. Parses the YAML first (returning an error immediately on failure), then moves the engine run and
//!   metric polling to a background task and immediately returns `{ "status": "started" }`. Live metrics during the run are
//!   written to shared state (`AppState.load_progress`), and the final result is written when finished
//!   (`AppState.load_done`). In Tauri mode the frontend reads them by polling the `load_progress` command,
//!   equivalent to SSE in browser mode.
//! - `stop_load_test`: sets the cancel flag so the running engine exits as soon as possible.
//!
//! Note: here we deliberately **no longer push via `app.emit`**. A Tauri background thread calling emit holds the webview
//! lock until the injected JS finishes executing, and while dragging/resizing the window the main thread waiting on the same lock causes the app to hang
//! (see tauri-apps/tauri#9453), so we uniformly switched to shared state + frontend polling.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use orbit_config::{from_str, Executor};
use orbit_metrics::MetricsSink;
use orbit_server::load::{live_frontend_summary, LoadRunner};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLoadTestRequest {
    pub yaml: String,
    #[serde(default = "default_vus")]
    pub vus: u32,
    #[serde(default = "default_duration")]
    pub duration: String,
    /// Whether to capture request/response details (the record-request-details switch; off by default)
    #[serde(default)]
    pub record_details: bool,
}

fn default_vus() -> u32 {
    1
}

fn default_duration() -> String {
    "10s".to_string()
}

/// Start a load test (non-blocking).
///
/// Returns `{ "status": "started" }` instead of blocking until the test ends -- this way the frontend can, throughout the whole test,
/// stay in the "running" state and poll live metrics via the `load_progress` command until the final result arrives and triggers teardown.
#[tauri::command]
pub async fn run_load_test(
    state: State<'_, AppState>,
    request: RunLoadTestRequest,
) -> Result<Value, String> {
    // Reset the cancel flag and clone a copy for this run.
    state.load_abort.store(false, Ordering::Relaxed);
    let abort = state.load_abort.clone();

    // Reset progress and the final result (clearing leftover state from the previous run).
    *state.load_progress.write().await = None;
    *state.load_done.write().await = None;
    *state.last_load_bus.lock().await = None;
    // Record this run's epoch: background tasks of an older epoch must not overwrite this run's state.
    let epoch = state.load_epoch.fetch_add(1, Ordering::Relaxed) + 1;
    let load_epoch = state.load_epoch.clone();

    // Parse the YAML first; on invalid config return a clear error immediately (instead of getting stuck in the run).
    let mut plan = match from_str(&request.yaml) {
        Ok(p) => p,
        Err(e) => {
            return Err(format!("Failed to parse load-test config: {e}"));
        }
    };

    // Apply the vus / duration overrides coming from the frontend.
    for scenario in &mut plan.scenarios {
        match &mut scenario.executor {
            Executor::ConstantVus { vus, duration, .. } => {
                *vus = request.vus;
                *duration = request.duration.clone();
            }
            Executor::ConstantArrivalRate {
                pre_allocated_vus,
                duration,
                ..
            } => {
                *pre_allocated_vus = request.vus.max(*pre_allocated_vus);
                *duration = request.duration.clone();
            }
            Executor::RampingVus { .. } => { /* ramping-vus is controlled by stages; the HTTP override does not apply */
            }
            Executor::Sequential { .. } => { /* sequential execution is not overridden */ }
        }
    }

    let runner = Arc::new(LoadRunner::new());
    let bus = runner.bus.clone();
    // Always retain raw samples (for exporting JTL / raw JSON from the page), capped at 500k entries to bound memory;
    // beyond that the export reports truncation.
    bus.enable_raw_samples_with_cap(500_000);

    // Live metric polling: every 500ms during the run, write a snapshot to shared state for the frontend `load_progress` to poll.
    let done = Arc::new(AtomicBool::new(false));
    let done_poller = done.clone();
    let bus_poller = bus.clone();
    let load_progress = state.load_progress.clone();
    let poller_epoch = load_epoch.clone();
    let poller = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            // A newer load test has started: this old round exits to avoid overwriting the new round's state.
            if poller_epoch.load(Ordering::Relaxed) != epoch {
                break;
            }
            let snap = live_frontend_summary(&bus_poller);
            *load_progress.write().await = Some(serde_json::to_string(&snap).unwrap_or_default());
            if done_poller.load(Ordering::Relaxed) {
                break;
            }
        }
    });

    // The engine runs in the background; on finish (or cancel) it writes the final summary to shared state.
    let done_run = done.clone();
    let runner_run = runner.clone();
    let load_done = state.load_done.clone();
    let last_bus = state.last_load_bus.clone();
    let bus_retain = bus.clone();
    let run_epoch = load_epoch.clone();
    let req_vus = request.vus;
    let req_duration = request.duration.clone();
    tokio::spawn(async move {
        let result = runner_run
            .run_plan(plan, req_vus, &req_duration, Some(abort))
            .await;
        let payload = match &result {
            Ok(val) => serde_json::to_string(val).unwrap_or_default(),
            Err(e) => serde_json::to_string(&serde_json::json!({ "status": "error", "error": e }))
                .unwrap_or_default(),
        };
        // Write the final result only when no newer load test has started (to avoid overwriting the new round's state after an old round stops).
        if run_epoch.load(Ordering::Relaxed) == epoch {
            *load_done.write().await = Some(payload);
            *last_bus.lock().await = Some(bus_retain);
        }
        done_run.store(true, Ordering::Relaxed);
        poller.abort();
    });

    Ok(serde_json::json!({ "status": "started" }))
}

/// Stop the running load test: sets the cancel flag; the engine exits as soon as possible at the next loop checkpoint.
#[tauri::command]
pub fn stop_load_test(state: State<'_, AppState>) {
    state.load_abort.store(true, Ordering::Relaxed);
}

/// Read the current load-test progress (Tauri frontend polling, replacing the `load-progress` / `load-done` events).
///
/// Returns:
/// - `snapshot`: the latest live metric snapshot (JSON string), `null` when there is no data yet;
/// - `done`: the final result (JSON string), non-`null` after the test ends;
/// - `running`: whether it is still running (an empty `done` means it is still running).
#[tauri::command]
pub async fn load_progress(state: State<'_, AppState>) -> Result<Value, String> {
    let snapshot = state.load_progress.read().await.clone();
    let done = state.load_done.read().await.clone();
    Ok(serde_json::json!({
        "snapshot": snapshot,
        "done": done,
        "running": done.is_none(),
    }))
}

/// Pull automation scenario step progress (polling drain, replacing the `load-progress` event).
/// Each call returns the step events accumulated since the last call and clears the buffer.
#[tauri::command]
pub async fn scenario_progress(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let mut queue = state.scenario_progress.lock().await;
    Ok(queue.drain(..).collect())
}

/// Export the latest load-test report (the page's "Export report"): generate the content by format and return the filename.
/// Summary formats (html/json/csv/junit) come from the metric summary; raw-sample formats (jtl/raw-json)
/// come from the samples collected during the test (which may be truncated by the cap, indicated by the truncated field).
#[tauri::command]
pub async fn export_load_report(
    state: State<'_, AppState>,
    format: String,
    name: Option<String>,
) -> Result<Value, String> {
    let guard = state.last_load_bus.lock().await;
    let Some(bus) = guard.as_ref() else {
        return Err("No load-test data yet; please run a load test first".to_string());
    };
    let summary = bus.snapshot();
    let label = name.unwrap_or_else(|| "Load Test".to_string());
    let Some(format) = orbit_output::ExportFormat::parse(&format) else {
        return Err(format!("Unsupported export format: {format}"));
    };
    let samples = bus.raw_samples();
    let ctx = orbit_output::ExportContext::new()
        .with_test_name(&label)
        .with_samples(&samples);
    let report = orbit_output::export(format, &summary, &ctx).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "content": report.content,
        "filename": report.filename,
        "truncated": bus.raw_truncated(),
    }))
}

/// Run an automation scenario (Tauri desktop)
/// Receives the scenario YAML and runs it through LoadRunner; step progress is written to a shared buffer that the frontend `scenario_progress`
/// polls (avoiding the background emit deadlock).
#[tauri::command]
pub async fn run_scenario(
    state: State<'_, AppState>,
    request: RunLoadTestRequest,
) -> Result<Value, String> {
    let runner = LoadRunner::new();

    // Create an event broadcast channel (for step-progress collection)
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel::<String>(64);
    let scenario_progress = state.scenario_progress.clone();

    // Background task: listen on the broadcast channel -> write into the shared buffer
    tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(msg) => {
                    scenario_progress.lock().await.push_back(msg);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    });

    // Parse the YAML and run
    tracing::debug!(yaml = %request.yaml, "Received scenario YAML");
    let plan = match from_str(&request.yaml) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(yaml = %request.yaml, "YAML parse error: {}", e);
            return Err(format!("YAML parse error: {}", e));
        }
    };
    runner
        .run_plan_with_events_opts(
            plan,
            request.vus,
            &request.duration,
            None,
            event_tx,
            request.record_details,
        )
        .await
}
