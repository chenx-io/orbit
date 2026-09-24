//! Load test runner (shared by orbit-server and the Tauri commands).
//!
//! Provides:
//! - `LoadRunner`: creates an independent `Engine` and its metrics bus per load run, avoiding metric accumulation across runs.
//! - `frontend_summary`: maps the engine's `MetricsSummary` to the frontend `LoadTestResult.summary`
//!   field names it expects (see `web/src/lib/bridge.ts`).
//!
//! Live metrics are polled via `live_summary()` during a run and broadcast (SSE / Tauri events).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use orbit_codec::json::JsonCodec;
use orbit_config::{from_str, Executor, TestPlan};
use orbit_engine::thresholds::ThresholdResult;
use orbit_engine::{Engine, EngineResult};
use orbit_metrics::{LocalMetricsBus, MetricsSink, MetricsSummary};
use orbit_protocol::http::HttpClient;
use serde_json::{json, Value};

/// Map the engine's `MetricsSummary` to the frontend summary field names.
pub fn frontend_summary(summary: &MetricsSummary) -> Value {
    json!({
        "total_requests": summary.total_requests,
        "total_failures": summary.total_errors,
        "total_duration_ms": summary.duration.as_millis() as u64,
        "rps": summary.rps,
        "p50_ms": summary.p50_ms,
        "p90_ms": summary.p90_ms,
        "p95_ms": summary.p95_ms,
        "p99_ms": summary.p99_ms,
        "p999_ms": summary.p999_ms,
        "min_ms": summary.min_ms,
        "max_ms": summary.max_ms,
        "mean_ms": summary.mean_ms,
        "error_rate": summary.error_rate,
        "total_bytes": summary.total_bytes,
        "error_breakdown": summary.error_breakdown.iter().map(|g| json!({
            "type": g.error_type,
            "count": g.count,
            "sample": g.sample,
        })).collect::<serde_json::Value>(),
        "timing": {
            "dns_ms": summary.avg_dns_ms,
            "tcp_ms": summary.avg_tcp_ms,
            "tls_ms": summary.avg_tls_ms,
            "send_ms": summary.avg_send_ms,
            "ttfb_ms": summary.avg_ttfb_ms,
            "download_ms": summary.avg_download_ms,
            "total_ms": summary.avg_dns_ms + summary.avg_tcp_ms + summary.avg_tls_ms
                + summary.avg_send_ms + summary.avg_ttfb_ms + summary.avg_download_ms,
        },
    })
}

/// Live metrics (including the current active VU count), used for the SSE / Tauri `load-progress` push.
pub fn live_frontend_summary(bus: &LocalMetricsBus) -> Value {
    let mut snap = frontend_summary(&bus.snapshot());
    if let Some(obj) = snap.as_object_mut() {
        obj.insert("vus".into(), json!(bus.active_vus()));
    }
    snap
}

/// Convert threshold results to frontend JSON
pub fn threshold_results_json(results: &[ThresholdResult]) -> Value {
    Value::Array(
        results
            .iter()
            .map(|r| {
                json!({
                    "label": format!("{}", r.condition),
                    "actual": r.actual,
                    "target": r.condition.value,
                    "passed": r.passed,
                    "abort_on_fail": r.condition.abort_on_fail,
                })
            })
            .collect(),
    )
}

/// Runner for one load run. Holds an independent `Engine` and metrics bus.
pub struct LoadRunner {
    engine: Engine,
    /// Metrics bus shared with the engine internals; can be polled during a run to produce live metrics.
    pub bus: Arc<LocalMetricsBus>,
}

impl LoadRunner {
    pub fn new() -> Self {
        let engine = Engine::new();
        let bus = engine.metrics_bus();
        Self { engine, bus }
    }

    /// Current metrics bus snapshot (frontend field names), used for live pushes.
    pub fn live_summary(&self) -> Value {
        live_frontend_summary(&self.bus)
    }

    /// Parse YAML, apply vus/duration overrides, run the load test, and return the result structure the frontend expects:
    /// `{ "status": "completed" | "error", "summary": {...} }` or `{ "error": "..." }`.
    pub async fn run(&self, yaml: &str, vus: u32, duration: &str) -> Result<Value, String> {
        let plan = match from_str(yaml) {
            Ok(p) => p,
            Err(e) => return Ok(json!({ "status": "error", "error": e.to_string() })),
        };
        self.run_plan(plan, vus, duration, None).await
    }

    /// Run an already-parsed test plan (called by the Tauri command after parsing, supports cancellation).
    pub async fn run_plan(
        &self,
        mut plan: TestPlan,
        vus: u32,
        duration: &str,
        abort: Option<Arc<AtomicBool>>,
    ) -> Result<Value, String> {
        // Apply vus / duration overrides from the frontend (consistent with the legacy run_handler behavior).
        for scenario in &mut plan.scenarios {
            match &mut scenario.executor {
                Executor::ConstantVus {
                    vus: v,
                    duration: d,
                    ..
                } => {
                    *v = vus;
                    *d = duration.to_string();
                }
                Executor::ConstantArrivalRate {
                    pre_allocated_vus,
                    duration: d,
                    ..
                } => {
                    // Pre-allocated VU count is at least the requested value to avoid throttling the arrival rate
                    *pre_allocated_vus = vus.max(*pre_allocated_vus);
                    *d = duration.to_string();
                }
                Executor::RampingVus { .. } => {
                    // ramping-vus controls VUs and duration via start_vus/stages, so HTTP overrides do not apply
                }
                Executor::Sequential { .. } => { /* sequential execution, no override */ }
            }
        }

        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        match self
            .engine
            .run_with_abort_and_thresholds(&plan, protocol, codec, abort)
            .await
        {
            Ok(result) => Ok(build_result_json(&result)),
            Err(e) => Ok(json!({ "status": "error", "error": e.to_string() })),
        }
    }

    pub async fn run_plan_with_events(
        &self,
        mut plan: TestPlan,
        vus: u32,
        _duration: &str,
        abort: Option<Arc<AtomicBool>>,
        event_tx: tokio::sync::broadcast::Sender<String>,
    ) -> Result<Value, String> {
        for scenario in &mut plan.scenarios {
            match &mut scenario.executor {
                Executor::ConstantVus {
                    vus: v,
                    duration: d,
                    ..
                } => {
                    *v = vus;
                    *d = _duration.to_string();
                }
                Executor::ConstantArrivalRate {
                    pre_allocated_vus,
                    duration: d,
                    ..
                } => {
                    *pre_allocated_vus = vus.max(*pre_allocated_vus);
                    *d = _duration.to_string();
                }
                _ => {}
            }
        }
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);
        match self
            .engine
            .run_with_events_and_thresholds(&plan, protocol, codec, abort, event_tx)
            .await
        {
            Ok(result) => Ok(build_result_json(&result)),
            Err(e) => Ok(json!({ "status": "error", "error": e.to_string() })),
        }
    }

    /// Same as `run_plan_with_events`, but supports capturing request/response details (automation scenario "record request details")
    pub async fn run_plan_with_events_opts(
        &self,
        mut plan: TestPlan,
        vus: u32,
        _duration: &str,
        abort: Option<Arc<AtomicBool>>,
        event_tx: tokio::sync::broadcast::Sender<String>,
        capture_requests: bool,
    ) -> Result<Value, String> {
        for scenario in &mut plan.scenarios {
            match &mut scenario.executor {
                Executor::ConstantVus {
                    vus: v,
                    duration: d,
                    ..
                } => {
                    *v = vus;
                    *d = _duration.to_string();
                }
                Executor::ConstantArrivalRate {
                    pre_allocated_vus,
                    duration: d,
                    ..
                } => {
                    *pre_allocated_vus = vus.max(*pre_allocated_vus);
                    *d = _duration.to_string();
                }
                _ => {}
            }
        }
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);
        match self
            .engine
            .run_with_events_and_thresholds_capture(
                &plan,
                protocol,
                codec,
                abort,
                event_tx,
                capture_requests,
            )
            .await
        {
            Ok(result) => Ok(build_result_json(&result)),
            Err(e) => Ok(json!({ "status": "error", "error": e.to_string() })),
        }
    }
}

/// Pack `EngineResult` into the JSON structure the frontend expects
fn build_result_json(result: &EngineResult) -> Value {
    let mut out = json!({
        "status": "completed",
        "summary": frontend_summary(&result.summary),
        "thresholds": threshold_results_json(&result.threshold_results),
        "all_thresholds_passed": result.all_thresholds_passed,
    });
    // Additional fields such as p90 / p999 / error_rate, directly usable by the frontend PercentilePoint
    if let Some(obj) = out["summary"].as_object_mut() {
        // These fields are already in the frontend summary (because we extended frontend_summary)
        let _ = obj; // keep
    }
    out
}

impl Default for LoadRunner {
    fn default() -> Self {
        Self::new()
    }
}
