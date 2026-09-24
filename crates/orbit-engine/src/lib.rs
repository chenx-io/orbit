//! # orbit-engine
//!
//! Orbit core engine. Integrates Protocol + Codec + Assertion + Extractor,
//! providing VU scheduling, step execution, metrics collection and threshold evaluation.

pub mod cookie_jar;
pub mod feeder;
pub mod flow_runner;
pub mod pipeline;
pub mod request_build;
pub mod scheduler;
pub mod thresholds;

use orbit_codec::traits::Codec;
use orbit_config::TestPlan;
use orbit_metrics::{LocalMetricsBus, MetricsSink, MetricsSummary};
use orbit_protocol::traits::ProtocolClient;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use thresholds::{ThresholdResult, ThresholdSet};

/// Engine error
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("Scheduler error: {0}")]
    Scheduler(String),
    #[error("Step execution error: {0}")]
    Step(String),
    #[error("Threshold error: {0}")]
    Threshold(String),
}

/// Load-test engine run result
pub struct EngineResult {
    pub summary: MetricsSummary,
    pub threshold_results: Vec<ThresholdResult>,
    pub all_thresholds_passed: bool,
}

/// Load-test engine
pub struct Engine {
    metrics_bus: Arc<LocalMetricsBus>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            metrics_bus: Arc::new(LocalMetricsBus::new()),
        }
    }

    /// Run a test plan
    pub async fn run(
        &self,
        plan: &TestPlan,
        protocol: Arc<dyn ProtocolClient>,
        codec: Arc<dyn Codec>,
    ) -> Result<MetricsSummary, EngineError> {
        self.run_with_abort(plan, protocol, codec, None).await
    }

    /// Run a test plan (bootstraps a default HTTP + JSON runtime from the plan; the caller no longer injects a client/codec).
    pub async fn run_plan(&self, plan: &TestPlan) -> Result<MetricsSummary, EngineError> {
        self.run(
            plan,
            Arc::new(orbit_protocol::http::HttpClient::new()),
            Arc::new(orbit_codec::json::JsonCodec),
        )
        .await
    }

    /// Run a test plan and evaluate thresholds (bootstraps the default runtime)
    pub async fn run_plan_with_thresholds(
        &self,
        plan: &TestPlan,
    ) -> Result<EngineResult, EngineError> {
        self.run_with_thresholds(
            plan,
            Arc::new(orbit_protocol::http::HttpClient::new()),
            Arc::new(orbit_codec::json::JsonCodec),
        )
        .await
    }

    /// Run a test plan with cancellation support.
    ///
    /// When `abort` is `Some` and set (`store(true)`), each VU loop exits as soon as possible,
    /// so the load test ends promptly when the user clicks "stop". Returns the metrics snapshot at the moment of exit.
    pub async fn run_with_abort(
        &self,
        plan: &TestPlan,
        protocol: Arc<dyn ProtocolClient>,
        codec: Arc<dyn Codec>,
        abort: Option<Arc<AtomicBool>>,
    ) -> Result<MetricsSummary, EngineError> {
        let executor =
            scheduler::Scheduler::new(plan.clone(), protocol, codec, self.metrics_bus.clone());

        executor.run_with_abort(abort).await?;

        Ok(self.metrics_bus.as_ref().snapshot())
    }

    /// Run a test plan (with an event channel, used by SSE / Tauri to push step progress)
    pub async fn run_with_events(
        &self,
        plan: &TestPlan,
        protocol: Arc<dyn ProtocolClient>,
        codec: Arc<dyn Codec>,
        abort: Option<Arc<AtomicBool>>,
        event_tx: tokio::sync::broadcast::Sender<String>,
    ) -> Result<MetricsSummary, EngineError> {
        self.run_with_events_capture(plan, protocol, codec, abort, event_tx, false)
            .await
    }

    /// Run a test plan (with an event channel + optional request detail capture); when capture_requests is on
    /// the step event carries a detail field (request + response snapshot) for "record request detail" delivery to the frontend.
    pub async fn run_with_events_capture(
        &self,
        plan: &TestPlan,
        protocol: Arc<dyn ProtocolClient>,
        codec: Arc<dyn Codec>,
        abort: Option<Arc<AtomicBool>>,
        event_tx: tokio::sync::broadcast::Sender<String>,
        capture_requests: bool,
    ) -> Result<MetricsSummary, EngineError> {
        let executor =
            scheduler::Scheduler::new(plan.clone(), protocol, codec, self.metrics_bus.clone())
                .with_event_tx(event_tx)
                .with_capture_requests(capture_requests);

        executor.run_with_abort(abort).await?;

        Ok(self.metrics_bus.as_ref().snapshot())
    }

    /// Run a test plan (with an event channel + threshold evaluation), returning the full EngineResult at once
    pub async fn run_with_events_and_thresholds(
        &self,
        plan: &TestPlan,
        protocol: Arc<dyn ProtocolClient>,
        codec: Arc<dyn Codec>,
        abort: Option<Arc<AtomicBool>>,
        event_tx: tokio::sync::broadcast::Sender<String>,
    ) -> Result<EngineResult, EngineError> {
        self.run_with_events_and_thresholds_capture(plan, protocol, codec, abort, event_tx, false)
            .await
    }

    /// Same as `run_with_events_and_thresholds`, but request detail capture can be enabled
    pub async fn run_with_events_and_thresholds_capture(
        &self,
        plan: &TestPlan,
        protocol: Arc<dyn ProtocolClient>,
        codec: Arc<dyn Codec>,
        abort: Option<Arc<AtomicBool>>,
        event_tx: tokio::sync::broadcast::Sender<String>,
        capture_requests: bool,
    ) -> Result<EngineResult, EngineError> {
        let summary = self
            .run_with_events_capture(plan, protocol, codec, abort, event_tx, capture_requests)
            .await?;
        self.evaluate_thresholds(plan, &summary)
    }

    /// Run a test plan (with cancellation support) and evaluate thresholds
    pub async fn run_with_abort_and_thresholds(
        &self,
        plan: &TestPlan,
        protocol: Arc<dyn ProtocolClient>,
        codec: Arc<dyn Codec>,
        abort: Option<Arc<AtomicBool>>,
    ) -> Result<EngineResult, EngineError> {
        let summary = self.run_with_abort(plan, protocol, codec, abort).await?;
        self.evaluate_thresholds(plan, &summary)
    }

    /// Return the engine's internal metrics bus (an Agent can use it to extract HDR histograms for distributed reporting)
    pub fn metrics_bus(&self) -> Arc<LocalMetricsBus> {
        self.metrics_bus.clone()
    }

    /// Run a test plan and evaluate thresholds
    pub async fn run_with_thresholds(
        &self,
        plan: &TestPlan,
        protocol: Arc<dyn ProtocolClient>,
        codec: Arc<dyn Codec>,
    ) -> Result<EngineResult, EngineError> {
        let summary = self.run(plan, protocol, codec).await?;
        self.evaluate_thresholds(plan, &summary)
    }

    /// Parse and evaluate thresholds and output the full `EngineResult` (shared by all `run_*_thresholds` entry points)
    fn evaluate_thresholds(
        &self,
        plan: &TestPlan,
        summary: &MetricsSummary,
    ) -> Result<EngineResult, EngineError> {
        // Parse and evaluate thresholds
        let mut threshold_set = ThresholdSet::new();
        for t in &plan.thresholds {
            threshold_set
                .parse_and_add(t)
                .map_err(|e| EngineError::Threshold(e.to_string()))?;
        }

        let threshold_results = if threshold_set.is_empty() {
            Vec::new()
        } else {
            threshold_set.evaluate_all(summary)
        };

        let all_thresholds_passed = threshold_set.all_passed(&threshold_results);

        // Print the threshold results
        if !threshold_results.is_empty() {
            tracing::info!("Threshold results:");
            for result in &threshold_results {
                if result.passed {
                    tracing::info!("  {}", result);
                } else {
                    tracing::warn!("  {}", result);
                }
            }
            tracing::info!(
                "Thresholds: {} passed, {} failed — {}",
                threshold_results.iter().filter(|r| r.passed).count(),
                threshold_results.iter().filter(|r| !r.passed).count(),
                if all_thresholds_passed {
                    "PASS"
                } else {
                    "FAIL"
                }
            );
        }

        Ok(EngineResult {
            summary: summary.clone(),
            threshold_results,
            all_thresholds_passed,
        })
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_codec::json::JsonCodec;
    use orbit_protocol::http::HttpClient;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::time::Instant;

    /// Start a local HTTP server for testing; returns (port, join_handle)
    fn start_local_test_server() -> (u16, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let handle = tokio::spawn(async move {
            use axum::{response::Json, routing::get, Router};
            async fn handler() -> Json<serde_json::Value> {
                Json(
                    serde_json::json!({"status": "ok", "timestamp": jiff::Timestamp::now().as_second()}),
                )
            }
            let app = Router::new().route("/api/test", get(handler));
            let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
                .await
                .unwrap();
            axum::serve(listener, app).await.unwrap();
        });

        // Wait for the server to be ready
        std::thread::sleep(std::time::Duration::from_millis(100));
        (port, handle)
    }

    fn make_local_plan(
        port: u16,
        vus: u32,
        duration: &str,
        ramp_up: &str,
    ) -> orbit_config::TestPlan {
        let yaml = format!(
            r#"
name: "local test"
scenarios:
  - name: "test"
    executor:
      type: constant-vus
      vus: {}
      duration: "{}"
      ramp_up: "{}"
    steps:
      - type: request
        request:
          method: GET
          url: "http://127.0.0.1:{}/api/test"
        checks:
          - type: status
            value: 200
"#,
            vus, duration, ramp_up, port
        );
        orbit_config::from_str(&yaml).unwrap()
    }

    fn make_staged_plan(port: u16) -> orbit_config::TestPlan {
        let yaml = format!(
            r#"
name: "staged test"
scenarios:
  - name: "staged"
    executor:
      type: ramping-vus
      start_vus: 1
      max_vus: 4
      stages:
        - target: 3
          duration: "1s"
          ramp: gradual
        - target: 4
          duration: "1s"
          ramp: instant
    steps:
      - type: request
        request:
          method: GET
          url: "http://127.0.0.1:{}/api/test"
"#,
            port
        );
        orbit_config::from_str(&yaml).unwrap()
    }

    fn make_ramping_plan(port: u16) -> orbit_config::TestPlan {
        let yaml = format!(
            r#"
name: "ramping test"
scenarios:
  - name: "ramping"
    executor:
      type: ramping-vus
      start_vus: 1
      stages:
        - target: 3
          duration: "1s"
        - target: 1
          duration: "1s"
    steps:
      - type: request
        request:
          method: GET
          url: "http://127.0.0.1:{}/api/test"
"#,
            port
        );
        orbit_config::from_str(&yaml).unwrap()
    }

    fn make_jmeter_plan(port: u16) -> orbit_config::TestPlan {
        let yaml = format!(
            r#"
name: "ramping jmeter test"
scenarios:
  - name: "ramping-jmeter"
    executor:
      type: ramping-vus
      start_vus: 1
      stages:
        - target: 3
          duration: "2s"
          ramp: jmeter
          ramp_up: "1s"
    steps:
      - type: request
        request:
          method: GET
          url: "http://127.0.0.1:{}/api/test"
"#,
            port
        );
        orbit_config::from_str(&yaml).unwrap()
    }

    #[tokio::test]
    async fn test_engine_run() {
        let (port, _server) = start_local_test_server();
        let engine = Engine::new();
        let plan = make_local_plan(port, 1, "2s", "0s");
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let result = engine.run(&plan, protocol, codec).await;
        drop(_server);
        assert!(result.is_ok());
        let summary = result.unwrap();
        assert!(summary.total_requests > 0);
        assert!(summary.p50_ms > 0.0);
    }

    #[tokio::test]
    async fn test_engine_ramping_vus_staged() {
        let (port, _server) = start_local_test_server();
        let engine = Engine::new();
        let plan = make_staged_plan(port);
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let started = Instant::now();
        let result = engine.run(&plan, protocol, codec).await;
        let elapsed = started.elapsed().as_secs_f64();
        drop(_server);

        assert!(result.is_ok());
        let summary = result.unwrap();
        assert!(summary.total_requests > 0);
        assert_eq!(summary.total_errors, 0);
        // A gradual stage fills its duration and an instant stage holds its duration; total ~= 2s
        assert!(
            elapsed >= 1.8,
            "ramping-vus stage duration not filled: {elapsed}s"
        );
        assert!(elapsed < 10.0, "ramping-vus run timed out: {elapsed}s");
    }

    #[tokio::test]
    async fn test_engine_ramping_vus_reports_active_vus() {
        let (port, _server) = start_local_test_server();
        let engine = Engine::new();
        let bus = engine.metrics_bus();
        let plan = make_staged_plan(port);
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let result = engine.run(&plan, protocol, codec).await;
        drop(_server);
        assert!(result.is_ok());
        // start_vus=1 -> stage 1 ramps to 3 -> stage 2 jumps to 4; should be 4 at the end
        assert_eq!(bus.active_vus(), 4);
    }

    #[tokio::test]
    async fn test_engine_ramping_vus_stage_duration() {
        let (port, _server) = start_local_test_server();
        let engine = Engine::new();
        let plan = make_ramping_plan(port);
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let started = Instant::now();
        let result = engine.run(&plan, protocol, codec).await;
        let elapsed = started.elapsed().as_secs_f64();
        drop(_server);

        assert!(result.is_ok());
        // 1s linear ramp up to 3 + 1s linear ramp down to 1 ~= 2s; before the fix the stage duration doubled to ~3s+
        assert!(
            (1.8..2.8).contains(&elapsed),
            "ramping-vus stage duration is abnormal: {elapsed}s"
        );
    }

    #[tokio::test]
    async fn test_engine_ramping_vus_jmeter_ramp_hold() {
        let (port, _server) = start_local_test_server();
        let engine = Engine::new();
        let bus = engine.metrics_bus();
        let plan = make_jmeter_plan(port);
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let started = Instant::now();
        let result = engine.run(&plan, protocol, codec).await;
        let elapsed = started.elapsed().as_secs_f64();
        drop(_server);

        assert!(result.is_ok());
        // ramping-vus + jmeter: 1->3 within a 1s ramp_up, then hold for 1s; total stage duration 2s
        assert!(
            (1.8..3.2).contains(&elapsed),
            "ramping-vus jmeter mode stage duration is abnormal: {elapsed}s"
        );
        assert_eq!(bus.active_vus(), 3);
    }

    #[tokio::test]
    async fn test_engine_with_thresholds() {
        let (port, _server) = start_local_test_server();
        let engine = Engine::new();
        let mut plan = make_local_plan(port, 1, "2s", "0s");
        plan.thresholds = vec![
            "http_req_duration: p(95) < 5000".to_string(),
            "http_req_failed: rate < 0.5".to_string(),
            "http_reqs: count > 0".to_string(),
        ];

        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let result = engine.run_with_thresholds(&plan, protocol, codec).await;
        drop(_server);
        assert!(result.is_ok());
        let engine_result = result.unwrap();
        assert!(engine_result.all_thresholds_passed);
        assert_eq!(engine_result.threshold_results.len(), 3);
    }

    /// Verify duration accuracy: with 3s specified, the actual load-test time should be between 2.5s and 6s
    #[tokio::test]
    async fn test_duration_accuracy() {
        let (port, _server) = start_local_test_server();
        let plan = make_local_plan(port, 1, "3s", "0s");
        let engine = Engine::new();
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let start = Instant::now();
        let result = engine.run(&plan, protocol, codec).await;
        let elapsed = start.elapsed();
        drop(_server);

        assert!(result.is_ok());
        let secs = elapsed.as_secs_f64();
        assert!(
            secs >= 2.5,
            "Duration too short: {:.1}s (expected >= 2.5s)",
            secs
        );
        assert!(
            secs <= 6.0,
            "Duration too long: {:.1}s (expected <= 6.0s)",
            secs
        );
    }

    /// Verify ramp_up + duration separation
    #[tokio::test]
    async fn test_ramp_up_duration_separation() {
        let (port, _server) = start_local_test_server();
        let plan = make_local_plan(port, 1, "2s", "0s");
        let engine = Engine::new();
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let start = Instant::now();
        let result = engine.run(&plan, protocol, codec).await;
        let elapsed = start.elapsed();
        drop(_server);

        assert!(result.is_ok());
        let secs = elapsed.as_secs_f64();
        assert!(
            secs >= 1.8,
            "Total time too short: {:.1}s (expected >= 1.8s)",
            secs
        );
        assert!(
            secs <= 5.0,
            "Total time too long: {:.1}s (expected <= 5.0s)",
            secs
        );
    }

    /// Verify correct behavior with multiple VUs + ramp_up
    #[tokio::test]
    async fn test_multi_vu_with_ramp_up() {
        let (port, _server) = start_local_test_server();
        let plan = make_local_plan(port, 3, "2s", "1s");
        let engine = Engine::new();
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let start = Instant::now();
        let result = engine.run(&plan, protocol, codec).await;
        let elapsed = start.elapsed();
        drop(_server);

        assert!(result.is_ok());
        let secs = elapsed.as_secs_f64();
        // ramp_up(1s) + duration(2s) ≈ 3s
        assert!(
            secs >= 2.5,
            "Total time too short: {:.1}s (expected >= 2.5s)",
            secs
        );
        assert!(
            secs <= 6.0,
            "Total time too long: {:.1}s (expected <= 6.0s)",
            secs
        );
    }

    #[tokio::test]
    async fn test_engine_with_assertions() {
        let yaml = r#"
name: "assertion test"
scenarios:
  - name: "test"
    executor:
      type: constant-vus
      vus: 1
      duration: "2s"
    steps:
      - type: request
        request:
          method: GET
          url: "https://httpbin.org/get"
        checks:
          - type: status
            value: 200
          - type: duration_lt
            value: "10s"
          - type: jsonpath
            path: "url"
            comparator: "exists"
            expected: ""
        extract:
          - name: response_url
            from: jsonpath
            path: "url"
"#;
        let plan = orbit_config::from_str(yaml).unwrap();
        let engine = Engine::new();
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let result = engine.run(&plan, protocol, codec).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_engine_with_pre_script() {
        let yaml = r#"
name: "pre-script test"
scenarios:
  - name: "test"
    executor:
      type: constant-vus
      vus: 1
      duration: "1s"
    steps:
      - type: request
        request:
          method: GET
          url: "https://httpbin.org/get"
        pre_script: "request.url = request.url + \"?foo=bar\";"
        checks:
          - type: status
            value: 200
"#;
        let plan = orbit_config::from_str(yaml).unwrap();
        let engine = Engine::new();
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);

        let result = engine.run(&plan, protocol, codec).await;
        assert!(result.is_ok());
    }

    /// Verify the cancellation flag: after abort is set the engine should exit promptly (far below the configured duration).
    #[tokio::test]
    async fn test_engine_abort() {
        let (port, _server) = start_local_test_server();
        let plan = make_local_plan(port, 4, "30s", "0s");
        let engine = Engine::new();
        let protocol = Arc::new(HttpClient::new());
        let codec = Arc::new(JsonCodec);
        let abort = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let abort_for_run = abort.clone();

        let run_handle = tokio::spawn(async move {
            let plan = plan.clone();
            engine
                .run_with_abort(&plan, protocol, codec, Some(abort_for_run))
                .await
        });

        // Let the load test run for a while, then cancel
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        abort.store(true, std::sync::atomic::Ordering::Relaxed);

        let measure = Instant::now();
        let result = run_handle.await.unwrap();
        let settle = measure.elapsed();
        drop(_server);

        assert!(result.is_ok());
        let summary = result.unwrap();
        assert!(
            summary.total_requests > 0,
            "requests should have been captured before cancellation"
        );
        // After cancellation it should end promptly (VU loops exit at the next checkpoint), far below 30s
        assert!(
            settle.as_secs_f64() < 5.0,
            "the engine should exit promptly after cancellation, but took {:.1}s",
            settle.as_secs_f64()
        );
    }

    /// Verify the new TCP config end to end: local TCP echo server + target/payload/framing
    #[tokio::test]
    async fn test_tcp_typed_config_end_to_end() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let n = sock.read(&mut buf).await.unwrap();
            sock.write_all(&buf[..n]).await.unwrap();
            sock.shutdown().await.unwrap();
        });

        let yaml = format!(
            r#"
name: "tcp typed"
scenarios:
  - name: "s"
    executor:
      type: sequential
      iterations: 1
    steps:
      - type: request
        name: "tcp_echo"
        request:
          url: "{}"
          payload: "PING"
          framing: {{ mode: read_until_close }}
"#,
            addr
        );
        let plan = orbit_config::from_str(&yaml).unwrap();
        let engine = Engine::new();
        let summary = engine.run_plan(&plan).await.unwrap();
        server.await.unwrap();
        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.total_errors, 0);
    }

    /// Phase 1 acceptance: real msgpack request body encoding + response auto-decoded by Content-Type
    #[tokio::test]
    async fn test_msgpack_request_response_routing() {
        use axum::{routing::post, Router};

        // Local echo service: returns the request body as-is and tags it application/msgpack.
        // The listener binds before spawn, so the port is effective immediately and connections are not refused while the service is not ready.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            async fn echo(body: axum::body::Bytes) -> impl axum::response::IntoResponse {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/msgpack")],
                    body,
                )
            }
            let app = Router::new().route("/echo", post(echo));
            axum::serve(listener, app).await.unwrap();
        });

        let yaml = format!(
            r#"
name: "msgpack routing"
scenarios:
  - name: "s"
    executor:
      type: sequential
      iterations: 1
    steps:
      - type: request
        name: "msgpack_echo"
        request:
          method: POST
          url: "http://127.0.0.1:{port}/echo"
          body:
            a: 1
            b: x
          payload_format: msgpack
          response_format: msgpack
        checks:
          - type: status
            value: 200
          - type: header
            name: "content-type"
            comparator: "contains"
            expected: "application/msgpack"
"#
        );
        let plan = orbit_config::from_str(&yaml).unwrap();
        let engine = Engine::new();
        let summary = engine.run_plan(&plan).await.unwrap();
        server.abort();
        assert_eq!(summary.total_requests, 1);
        assert_eq!(
            summary.total_errors, 0,
            "msgpack encoding + Content-Type decode routing should pass"
        );
    }

    /// Local echo server: returns the request's path + header + body as-is in JSON,
    /// used to verify that dynamic values are regenerated on every request.
    fn start_echo_server() -> (u16, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let handle = tokio::spawn(async move {
            use axum::{body::to_bytes, routing::any, Router};
            async fn echo(
                uri: axum::http::Uri,
                headers: axum::http::HeaderMap,
                body: axum::body::Body,
            ) -> axum::Json<serde_json::Value> {
                let bytes = to_bytes(body, 1024 * 1024).await.unwrap_or_default();
                let body_str = String::from_utf8_lossy(&bytes).to_string();
                let mut hdrs = serde_json::Map::new();
                if let Some(v) = headers.get("x-trace") {
                    hdrs.insert(
                        "x-trace".into(),
                        serde_json::json!(v.to_str().unwrap_or("")),
                    );
                }
                axum::Json(serde_json::json!({
                    "path": uri.path_and_query().map(|p| p.to_string()).unwrap_or_default(),
                    "headers": hdrs,
                    "body": body_str,
                }))
            }
            let app = Router::new().route("/echo/{*path}", any(echo));
            let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
                .await
                .unwrap();
            axum::serve(listener, app).await.unwrap();
        });

        std::thread::sleep(std::time::Duration::from_millis(100));
        (port, handle)
    }

    #[tokio::test]
    async fn dynamic_values_regenerated_per_request() {
        // url/header/body all contain {{$string.uuid}}; two consecutive runs must generate different values
        let (port, server) = start_echo_server();
        let yaml = format!(
            r#"
name: "dynamic test"
scenarios:
  - name: "test"
    executor:
      type: constant-vus
      vus: 1
      duration: "1s"
      ramp_up: "0s"
    steps:
      - type: request
        request:
          method: POST
          url: "http://127.0.0.1:{}/echo?id={{{{$string.uuid}}}}"
          headers:
            x-trace: "{{{{$string.uuid}}}}"
          body: '{{"id":"{{{{$string.uuid}}}}"}}'
"#,
            port
        );
        let plan = orbit_config::from_str(&yaml).unwrap();

        let run_once = || async {
            let engine = Engine::new();
            let summary = engine.run_plan(&plan).await.unwrap();
            // Take the success sample count: at least 1
            assert!(
                summary.total_requests >= 1,
                "at least 1 request should be sent"
            );
        };

        // The URL cannot be recovered from the summary, so instead verify the behavior of interpolate after plan parsing:
        // Verify directly at the config layer that two interpolate calls generate different values (URL/header/body all go through interpolate)
        let vars = std::collections::HashMap::new();
        let base = format!("http://127.0.0.1:{}/echo?id=", port);
        // Use string concatenation to avoid format! escaping {{ to {
        let url_tpl = base.clone() + "{{$string.uuid}}";
        let u1 = orbit_config::interpolate(&url_tpl, &vars).unwrap();
        let u2 = orbit_config::interpolate(&url_tpl, &vars).unwrap();
        assert_ne!(
            u1, u2,
            "URL dynamic value should differ on every generation: {} vs {}",
            u1, u2
        );

        let h1 = orbit_config::interpolate("{{$string.uuid}}", &vars).unwrap();
        let h2 = orbit_config::interpolate("{{$string.uuid}}", &vars).unwrap();
        assert_ne!(h1, h2, "header dynamic value should differ each time");

        let b1 = orbit_config::interpolate(r#"{"id":"{{$string.uuid}}"}"#, &vars).unwrap();
        let b2 = orbit_config::interpolate(r#"{"id":"{{$string.uuid}}"}"#, &vars).unwrap();
        assert_ne!(b1, b2, "body dynamic value should differ each time");

        // Run the engine once to confirm a plan with dynamic values executes normally (compile path: build_http_request interpolates url/headers/body)
        run_once().await;
        server.abort();
    }
}
