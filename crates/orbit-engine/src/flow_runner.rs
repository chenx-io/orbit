//! Step interpreter
//!
//! Executes test steps: request / loop / wait / set-var / condition / group -> returns a list of metric samples

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use orbit_assertion::types::AssertionContext;
use orbit_codec::traits::Codec;
use orbit_config::{MessageSpec, PayloadType, RequestSpec, Step};
use orbit_metrics::MetricSample;
use orbit_protocol::guard::execute_guarded;
use orbit_protocol::traits::ProtocolClient;
use orbit_protocol::types::{
    ProtocolError, ProtocolOptions, ProtocolRequest, ProtocolResponse, WsCallOptions,
};
use tokio_util::sync::CancellationToken;

use crate::cookie_jar::CookieJar;
use crate::pipeline::{
    execute_pipeline, PipelineError, PipelineOutcome, PipelineRuntime, PipelineSpec,
};
use base64::prelude::*;

/// Request-detail body truncation cap (per direction; longer bodies are truncated and flagged truncated)
const DETAIL_BODY_CAP: usize = 32 * 1024;
/// Capture queue capacity cap (prevents memory blow-up; oldest entries are dropped when exceeded)
const CAPTURE_QUEUE_CAP: usize = 500;

/// Detail snapshot of a single request (request + response metadata and content), for "record request details" fed to the front end
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedRequest {
    /// Request method / operation name (HTTP = method, gRPC = method; empty for message sequences)
    pub method: String,
    /// Request target (URL / connection address, already variable-interpolated)
    pub target: String,
    pub operation: String,
    pub request_headers: Vec<(String, String)>,
    pub request_body: String,
    pub request_truncated: bool,
    pub status: i32,
    pub response_headers: Vec<(String, String)>,
    pub response_body: String,
    pub response_size: usize,
    pub response_truncated: bool,
    pub error: Option<String>,
}

/// body -> (string, truncated?); non-UTF-8 is decoded lossily
fn body_to_string(bytes: &[u8]) -> (String, bool) {
    if bytes.len() > DETAIL_BODY_CAP {
        (
            String::from_utf8_lossy(&bytes[..DETAIL_BODY_CAP]).into_owned(),
            true,
        )
    } else {
        (String::from_utf8_lossy(bytes).into_owned(), false)
    }
}

/// header map -> sorted key-value list (keeps output stable)
fn headers_to_vec(h: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = h.iter().map(|(k, val)| (k.clone(), val.clone())).collect();
    v.sort();
    v
}

/// PipelineError -> human-readable string
fn pipeline_error_text(e: &PipelineError) -> String {
    match e {
        PipelineError::Protocol(s) => format!("protocol: {}", s),
        PipelineError::PreScript(s) => format!("pre-script: {}", s),
        PipelineError::PostScript(s) => format!("post-script: {}", s),
        PipelineError::RequestBuild(s) => format!("request-build: {}", s),
        PipelineError::Assertion => "assertion failed".to_string(),
        PipelineError::Timeout(s) => format!("timeout: {}", s),
    }
}

/// Step interpreter - one instance per VU (Cookie Jar is isolated per instance -> each VU has its own session)
pub struct FlowRunner {
    /// Shared pipeline runtime (protocol client cache + codec cache)
    runtime: PipelineRuntime,
    /// Scenario name (the scenario dimension of metric samples)
    scenario: String,
    /// Run-level cancellation token (timeouts + aborting in-flight requests)
    cancel: CancellationToken,
    /// Runtime variables (extracted variables + initial variables)
    variables: HashMap<String, String>,
    /// Cookie Jar (session persistence): accumulates this VU's response Set-Cookie and attaches it to same-domain requests automatically
    cookie_jar: CookieJar,
    /// Whether to capture request/response details (enabled on demand for automation scenarios only; the load path keeps false for zero overhead)
    pub(crate) capture_requests: bool,
    /// Captured-detail queue (in execution order; the event path pops one per step event)
    pub(crate) captured: VecDeque<CapturedRequest>,
}

impl FlowRunner {
    pub fn new(
        protocol: Box<dyn ProtocolClient>,
        codec: Box<dyn Codec>,
        initial_vars: HashMap<String, String>,
    ) -> Self {
        Self {
            runtime: PipelineRuntime::new(protocol, codec),
            scenario: String::new(),
            cancel: CancellationToken::new(),
            variables: initial_vars,
            cookie_jar: CookieJar::new(),
            capture_requests: false,
            captured: VecDeque::new(),
        }
    }

    /// Sets the scenario name (written to metric samples' scenario field)
    pub fn with_scenario(mut self, scenario: &str) -> Self {
        self.scenario = scenario.to_string();
        self
    }

    /// Sets the run-level cancellation token (to abort in-flight requests)
    pub fn with_cancel(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// Enables request/response detail capture (used only by the sequential event path)
    pub fn with_capture_requests(mut self, on: bool) -> Self {
        self.capture_requests = on;
        self
    }

    /// Pops the oldest captured detail (one-to-one with step events; always None when capture is off)
    pub fn pop_captured(&mut self) -> Option<CapturedRequest> {
        self.captured.pop_front()
    }

    /// Captures a unified-pipeline outcome as a detail snapshot
    fn capture_outcome(&mut self, outcome: &PipelineOutcome) -> CapturedRequest {
        let (req_body, req_trunc) = body_to_string(&outcome.request.payload);
        let (resp_body, resp_trunc) = outcome
            .response
            .as_ref()
            .map(|r| body_to_string(&r.payload))
            .unwrap_or_else(|| (String::new(), false));
        CapturedRequest {
            method: outcome.request.operation.clone(),
            target: outcome.request.target.clone(),
            operation: outcome.request.operation.clone(),
            request_headers: headers_to_vec(&outcome.request.headers),
            request_body: req_body,
            request_truncated: req_trunc,
            status: outcome
                .response
                .as_ref()
                .map(|r| r.status_code as i32)
                .unwrap_or(0),
            response_headers: outcome
                .response
                .as_ref()
                .map(|r| headers_to_vec(&r.headers))
                .unwrap_or_default(),
            response_body: resp_body,
            response_size: outcome
                .response
                .as_ref()
                .map(|r| r.payload.len())
                .unwrap_or(0),
            response_truncated: resp_trunc,
            error: outcome.error.as_ref().map(pipeline_error_text),
        }
    }

    /// Pushes a captured detail (drops the oldest when over capacity)
    fn push_captured(&mut self, entry: CapturedRequest) {
        if self.captured.len() >= CAPTURE_QUEUE_CAP {
            self.captured.pop_front();
        }
        self.captured.push_back(entry);
    }

    /// Gets the current variables
    pub fn variables(&self) -> HashMap<String, String> {
        self.variables.clone()
    }

    /// Injects a variable (for datasources like a CSV Feeder)
    pub fn inject_variable(&mut self, key: String, value: String) {
        self.variables.insert(key, value);
    }

    /// Removes a variable (for cleaning up scoped variables such as loop indices)
    pub fn remove_variable(&mut self, key: &str) {
        self.variables.remove(key);
    }

    // ─── Public API ────────────────────────────────────────────────

    /// Executes a list of steps and returns all metric samples produced
    ///
    /// Iterates the given step list in order, dispatching to the matching executor by step type.
    /// - `Request` calls [`execute_request`] and produces 1 sample
    /// - `Loop` recursively runs child steps `count` times, exposing `loop.index` (0-based) to child steps during the loop
    /// - `Wait` produces no sample
    /// - `SetVar` produces no sample but modifies variables
    /// - `Condition` evaluates the expression then runs the then/else branch
    /// - `Group` recursively runs child steps
    ///
    /// `stop_on_error`: when true, an error in a single request step immediately stops later steps.
    pub async fn execute_steps(
        &mut self,
        steps: &[Step],
        vu_id: u64,
        iteration: u64,
        stop_on_error: bool,
    ) -> Result<Vec<MetricSample>, String> {
        let mut all_samples = Vec::new();
        for step in steps {
            if step.is_disabled() {
                continue;
            }
            match step {
                Step::Request { .. } => {
                    let samples = self.execute_request(step, vu_id, iteration).await?;
                    let is_err = samples.iter().any(|s| s.is_error);
                    all_samples.extend(samples);
                    if is_err && stop_on_error {
                        break;
                    }
                }
                Step::Loop {
                    count,
                    steps: child_steps,
                    ..
                } => {
                    for i in 0..*count {
                        // Expose the loop index as the variable `loop.index` (0-based) for child steps
                        let prev = self
                            .variables
                            .insert("loop.index".to_string(), i.to_string());
                        let mut samples = Box::pin(self.execute_steps(
                            child_steps,
                            vu_id,
                            iteration,
                            stop_on_error,
                        ))
                        .await?;
                        all_samples.append(&mut samples);
                        // Restore (supports nested loops)
                        match prev {
                            Some(v) => {
                                self.variables.insert("loop.index".to_string(), v);
                            }
                            None => {
                                self.variables.remove("loop.index");
                            }
                        }
                    }
                }
                Step::Wait { duration, .. } => {
                    let secs = orbit_config::parse_duration(duration).unwrap_or(1.0);
                    tokio::time::sleep(std::time::Duration::from_secs_f64(secs)).await;
                }
                Step::SetVar { key, value, .. } => {
                    let resolved = orbit_config::interpolate(value, &self.variables)
                        .unwrap_or_else(|_| value.clone());
                    self.variables.insert(key.clone(), resolved);
                }
                Step::Condition {
                    expression,
                    then: then_steps,
                    else_steps,
                    ..
                } => {
                    let branch = self.evaluate_condition(expression)?;
                    let target = if branch { then_steps } else { else_steps };
                    let mut samples =
                        Box::pin(self.execute_steps(target, vu_id, iteration, stop_on_error))
                            .await?;
                    all_samples.append(&mut samples);
                }
                Step::Group {
                    steps: child_steps, ..
                } => {
                    let mut samples =
                        Box::pin(self.execute_steps(child_steps, vu_id, iteration, stop_on_error))
                            .await?;
                    all_samples.append(&mut samples);
                }
            }
        }
        Ok(all_samples)
    }

    /// Executes a single request step (backward-compatible convenience method)
    ///
    /// Returns a single `MetricSample`. Valid only for `Step::Request`; other variants return an empty error sample.
    pub async fn execute_step(
        &mut self,
        step: &Step,
        vu_id: u64,
        iteration: u64,
    ) -> Result<MetricSample, String> {
        let mut samples = self.execute_request(step, vu_id, iteration).await?;
        samples
            .pop()
            .ok_or_else(|| format!("Step '{}' produced no samples", step.name()))
    }

    // ─── Private methods ────────────────────────────────────────────────

    /// Executes a single HTTP/gRPC/WebSocket request step
    async fn execute_request(
        &mut self,
        step: &Step,
        vu_id: u64,
        iteration: u64,
    ) -> Result<Vec<MetricSample>, String> {
        let (spec, _protocol_override) = step
            .try_as_request()
            .ok_or_else(|| format!("Step '{}' is not a Request variant", step.name()))?;

        // Long-lived connection message sequence (WebSocket/TCP/UDP messages): send/receive one by one on the connection
        if let Some(messages) = spec_messages(spec) {
            return self
                .execute_message_sequence(step, spec, messages, vu_id, iteration)
                .await;
        }

        let start = Instant::now();

        // Unified execution pipeline: interpolation -> pre-script -> encode -> send -> decode -> assert -> extract -> post-script
        let p_spec = self.pipeline_spec_from_step(step, spec)?;
        let env = self.variables.clone();
        let outcome = execute_pipeline(
            &mut self.runtime,
            p_spec,
            &mut self.variables,
            &env,
            &self.cancel,
            Some(&mut self.cookie_jar),
        )
        .await;

        tracing::debug!(
            "\n  {}\n  Status: {}\n  Error: {:?}",
            spec_display(spec),
            outcome
                .response
                .as_ref()
                .map(|r| r.status_code)
                .unwrap_or(0),
            outcome.error.as_ref().map(|e| e.category()),
        );

        // "Record request details": capture the request + response snapshot (the event path pops them one by one to the front end)
        if self.capture_requests {
            let entry = self.capture_outcome(&outcome);
            self.push_captured(entry);
        }

        Ok(vec![
            self.outcome_to_sample(step, vu_id, iteration, start, outcome)
        ])
    }

    /// Selects a protocol client (HTTP uses the default instance, others are cached by type) - via the shared pipeline runtime
    fn protocol_for(&mut self, id: &str) -> &mut dyn ProtocolClient {
        self.runtime.client_for(id)
    }

    /// Step + RequestSpec -> protocol-agnostic PipelineSpec (interpolation/scripts/encoding are handled by the pipeline)
    fn pipeline_spec_from_step(
        &self,
        step: &Step,
        spec: &RequestSpec,
    ) -> Result<PipelineSpec, String> {
        let mut p = match spec {
            RequestSpec::Http(cfg) => {
                let mut p = PipelineSpec {
                    protocol: "http".into(),
                    target: cfg.url.clone(),
                    operation: cfg.method.clone(),
                    headers: cfg.headers.clone(),
                    body_value: cfg.body.clone(),
                    request_format: cfg.payload_format.clone(),
                    response_format: cfg.response_format.clone(),
                    timeout: Some(std::time::Duration::from_secs_f64(
                        orbit_config::parse_duration(&cfg.timeout).unwrap_or(30.0),
                    )),
                    interpolate: true,
                    ..Default::default()
                };
                // Legacy gRPC side-channel: grpc_service as operation, defaulting to json message format
                if let Some(svc) = &cfg.grpc_service {
                    p.operation = svc.clone();
                    if p.request_format.is_none() {
                        p.request_format = Some("json".to_string());
                    }
                }
                p
            }
            other => spec_to_pipeline(other),
        };
        // Plugin protocol override: an explicit `protocol_id` wins over RequestSpec inference
        p.protocol = step.resolve_protocol_id();
        if let Step::Request {
            checks, extract, ..
        } = step
        {
            // Pre/post actions (including legacy single-script-field normalization): DB fetch writes variables / scripts rewrite the request, executed in list order.
            // Pre-actions are a single ordered list; the built-in interpolation node splits pre-/post-interpolation:
            // before the node they act on the un-interpolated request template (may write variables for this interpolation),
            // after the node they act on the final payload (the rewrite is the final bytes, for signing/encryption).
            // The library is passed empty: scenario / load YAML already expands script library references into concrete actions at generation time (self-contained),
            // so the engine only needs a fallback for "refs in hand-written YAML" (see `PipelineAction::UnresolvedRef`).
            p.pre_actions =
                crate::pipeline::actions_to_pipeline(&step.pre_actions_normalized(), &[]);
            p.post_actions =
                crate::pipeline::actions_to_pipeline(&step.post_actions_normalized(), &[]);
            p.checks = checks.clone();
            p.extracts = extract.clone();
        }
        Ok(p)
    }

    /// PipelineOutcome -> MetricSample (load-path projection; error samples are typed by category)
    fn outcome_to_sample(
        &self,
        step: &Step,
        vu_id: u64,
        iteration: u64,
        start: Instant,
        outcome: PipelineOutcome,
    ) -> MetricSample {
        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        let scenario = self.scenario.clone();
        let step_name = step.name().to_string();
        let ts = now_millis();

        // Protocol/IO/script/timeout errors: an error sample directly
        let Some(response) = outcome.response else {
            let err = outcome
                .error
                .unwrap_or(PipelineError::Protocol("request failed".into()));
            return MetricSample {
                scenario,
                step: step_name,
                vu_id,
                iteration,
                status: 0,
                duration_ms,
                body_size: 0,
                message_count: 0,
                is_error: true,
                error_msg: Some(format!("Request failed: {}", err.message())),
                error_type: err.category().to_string(),
                timestamp: ts,
                dns_ms: 0.0,
                tcp_ms: 0.0,
                tls_ms: 0.0,
                send_ms: 0.0,
                ttfb_ms: 0.0,
                download_ms: 0.0,
            };
        };

        let is_error = outcome.error.is_some() || response.status_code >= 500;
        let error_type: String = match &outcome.error {
            Some(e) => e.category().to_string(),
            None if response.status_code >= 500 => "server_error".to_string(),
            None => String::new(),
        };
        let error_msg: Option<String> = match &outcome.error {
            Some(e) => Some(e.message()),
            None if response.status_code >= 500 => Some(format!("HTTP {}", response.status_code)),
            None => None,
        };

        MetricSample {
            scenario,
            step: step_name,
            vu_id,
            iteration,
            status: response.status_code as i32,
            duration_ms,
            body_size: response.payload.len(),
            message_count: response.message_count,
            is_error,
            error_msg,
            error_type,
            timestamp: ts,
            dns_ms: 0.0,
            tcp_ms: 0.0,
            tls_ms: 0.0,
            send_ms: 0.0,
            ttfb_ms: 0.0,
            download_ms: 0.0,
        }
    }

    /// Request tail: decode / assert / extract / post-script / error classification -> sample (long-lived message-sequence path).
    #[allow(clippy::too_many_arguments)]
    async fn finalize_request_sample(
        &mut self,
        step: &Step,
        vu_id: u64,
        iteration: u64,
        start: Instant,
        response: ProtocolResponse,
        response_format: Option<&str>,
        run_step_post: bool,
    ) -> MetricSample {
        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

        // Decode the response body (response_format -> Content-Type -> default codec) - reusing pipeline capability
        let body_data =
            self.runtime
                .decode_body(&response.payload, &response.metadata, response_format);

        let resp_headers: HashMap<String, String> = response.metadata.iter().cloned().collect();

        // Extraction before assertions (same timing as pipeline.rs; DB assertions can reference variables extracted here)
        let extracts = match step {
            Step::Request { extract, .. } => extract.as_slice(),
            _ => &[],
        };
        if let Ok(extracted) =
            crate::pipeline::perform_extraction(extracts, &response.payload, &resp_headers)
        {
            for (k, v) in extracted {
                self.variables.insert(k, v);
            }
        }

        let ctx = AssertionContext {
            status_code: response.status_code,
            headers: resp_headers.clone(),
            body_bytes: response.payload.clone(),
            body_data,
            duration_ms: duration_ms as u64,
            body_size: response.payload.len(),
            env_vars: HashMap::new(),
            extracted_vars: self.variables.clone(),
            datasources: self.runtime.datasources.clone(),
        };

        let checks = match step {
            Step::Request { checks, .. } => checks.as_slice(),
            _ => &[],
        };
        let (has_hard_failure, results) = crate::pipeline::evaluate_checks(checks, &ctx).await;
        // DB/Redis assertion extract_var: writes into the request variable space for post-scripts and later steps
        for r in &results {
            for (k, v) in &r.exported_vars {
                self.variables.insert(k.clone(), v.clone());
            }
        }

        let post_script_error = if run_step_post {
            // Post-actions (including legacy single-script-field normalization), executed in list order; DB fetch writes into the variable space
            let actions =
                crate::pipeline::actions_to_pipeline(&step.post_actions_normalized(), &[]);
            let mut error: Option<String> = None;
            for action in &actions {
                match action {
                    crate::pipeline::PipelineAction::Script { code, .. } => {
                        if code.trim().is_empty() {
                            continue;
                        }
                        // Process-wide shared sandbox: reused across requests / FlowRunners
                        let err = orbit_js::with_sandbox(|sb| {
                            crate::pipeline::run_post_script(
                                sb,
                                code,
                                &response,
                                &resp_headers,
                                duration_ms,
                                &self.variables,
                                &HashMap::new(),
                            )
                            .err()
                        });
                        if err.is_some() {
                            error = err;
                            break;
                        }
                    }
                    crate::pipeline::PipelineAction::Db(db) => {
                        let ran =
                            crate::pipeline::run_db_action(&self.runtime, db, &self.variables)
                                .await;
                        for (k, v) in &ran.vars {
                            self.variables.insert(k.clone(), v.clone());
                        }
                    }
                    // The post-action list contains no built-in interpolation node (the anchor appears only in the pre list)
                    crate::pipeline::PipelineAction::Interpolate => {}
                    // Dangling script library reference: recorded as a step failure but does not abort the whole flow (fallback for hand-written YAML)
                    crate::pipeline::PipelineAction::UnresolvedRef { library_id, .. } => {
                        if error.is_none() {
                            error = Some(format!(
                                "referenced script library item missing: {library_id}"
                            ));
                        }
                    }
                }
            }
            error
        } else {
            None
        };

        let is_error =
            has_hard_failure || post_script_error.is_some() || (response.status_code >= 500);

        let error_type: String = if has_hard_failure {
            "assertion".to_string()
        } else if post_script_error.is_some() {
            "post_script".to_string()
        } else if response.status_code >= 500 {
            "server_error".to_string()
        } else {
            String::new()
        };

        MetricSample {
            scenario: self.scenario.clone(),
            step: step.name().to_string(),
            vu_id,
            iteration,
            status: response.status_code,
            duration_ms,
            body_size: response.payload.len(),
            message_count: response.message_count,
            is_error,
            error_msg: if has_hard_failure {
                Some("Assertion failed".into())
            } else if let Some(ref e) = post_script_error {
                Some(format!("Post-script error: {}", e))
            } else if response.status_code >= 500 {
                Some(format!("HTTP {}", response.status_code))
            } else {
                None
            },
            error_type,
            timestamp: now_millis(),
            dns_ms: response
                .timings
                .dns
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
            tcp_ms: response
                .timings
                .tcp
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
            tls_ms: response
                .timings
                .tls
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
            send_ms: response
                .timings
                .send
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
            ttfb_ms: response
                .timings
                .first_byte
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
            download_ms: response
                .timings
                .receive
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
        }
    }

    /// Executes a long-lived connection message sequence: the connection stays open, one by one pre-script -> send -> receive -> post-script decode
    async fn execute_message_sequence(
        &mut self,
        step: &Step,
        spec: &RequestSpec,
        messages: &[MessageSpec],
        vu_id: u64,
        iteration: u64,
    ) -> Result<Vec<MetricSample>, String> {
        let kind = step.resolve_protocol_id();
        // Variable interpolation: the connection address can reference variables extracted by earlier steps (e.g. a login token)
        let target = orbit_config::interpolate(&spec_target(spec), &self.variables)
            .unwrap_or_else(|_| spec_target(spec));
        let cancel = self.cancel.clone();

        self.protocol_for(&kind)
            .connect(&target)
            .await
            .map_err(|e| format!("connect {}: {}", target, e))?;

        let mut samples = Vec::new();
        for (i, msg) in messages.iter().enumerate() {
            if msg.wait_ms > 0 && i > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(msg.wait_ms)).await;
            }
            let start = Instant::now();

            // Payload decoding (text/base64/hex)
            let ptype = msg.payload_type.unwrap_or_default();
            let mut payload = msg
                .payload
                .as_deref()
                .map(|p| {
                    let resolved = orbit_config::interpolate(p, &self.variables)
                        .unwrap_or_else(|_| p.to_string());
                    decode_payload(&resolved, ptype)
                })
                .unwrap_or_default();

            // Message-level pre-script: custom encoding via pm.request.raw
            if let Some(src) = &msg.pre_script {
                self.run_payload_pre_script(src, &target, &mut payload);
            }

            let request = ProtocolRequest {
                target: target.clone(),
                operation: String::new(),
                metadata: vec![],
                payload,
                timeout: Some(std::time::Duration::from_secs(30)),
                streaming_mode: None,
                payload_format: None,
                response_format: None,
                options: seq_options(spec, msg),
                // The message-sequence path's spec is a RequestSpec (no connection-config source); plugin protocols go through the single-shot/load path
                connection: None,
            };

            // "Record request details": request-side snapshot (captured before sending, request is about to be moved)
            let mut msg_capture = if self.capture_requests {
                let (body, truncated) = body_to_string(&request.payload);
                Some(CapturedRequest {
                    method: String::new(),
                    target: target.clone(),
                    operation: format!("message #{}", i + 1),
                    request_headers: Vec::new(),
                    request_body: body,
                    request_truncated: truncated,
                    status: 0,
                    response_headers: Vec::new(),
                    response_body: String::new(),
                    response_size: 0,
                    response_truncated: false,
                    error: None,
                })
            } else {
                None
            };

            // Borrow convergence: hold &mut client only during the send
            let response = {
                let client = self.protocol_for(&kind);
                execute_guarded(client, request, &cancel).await
            };
            let response = match response {
                Ok(r) => r,
                Err(e) => {
                    let err: ProtocolError = e;
                    let text = format!("Request failed: {}", err);
                    let dur = start.elapsed().as_secs_f64() * 1000.0;
                    if let Some(entry) = msg_capture.take() {
                        let mut entry = entry;
                        entry.error = Some(text.clone());
                        self.push_captured(entry);
                    }
                    samples.push(MetricSample {
                        scenario: self.scenario.clone(),
                        step: step.name().to_string(),
                        vu_id,
                        iteration,
                        status: 0,
                        duration_ms: dur,
                        body_size: 0,
                        message_count: 0,
                        is_error: true,
                        error_msg: Some(text),
                        error_type: err.category().to_string(),
                        timestamp: now_millis(),
                        dns_ms: 0.0,
                        tcp_ms: 0.0,
                        tls_ms: 0.0,
                        send_ms: 0.0,
                        ttfb_ms: 0.0,
                        download_ms: 0.0,
                    });
                    continue;
                }
            };

            // Message-level post-script: custom decoding via pm.response.decoded
            let mut effective_payload = response.payload.clone();
            if let Some(post) = &msg.post_script {
                match self.run_decoded_post_script(post, &response) {
                    Ok(Some(decoded)) => effective_payload = decoded,
                    Ok(None) => {}
                    Err(e) => {
                        let dur = start.elapsed().as_secs_f64() * 1000.0;
                        if let Some(mut entry) = msg_capture.take() {
                            entry.error = Some(format!("Post-script error: {}", e));
                            self.push_captured(entry);
                        }
                        samples.push(MetricSample {
                            scenario: self.scenario.clone(),
                            step: step.name().to_string(),
                            vu_id,
                            iteration,
                            status: 0,
                            duration_ms: dur,
                            body_size: 0,
                            message_count: 0,
                            is_error: true,
                            error_msg: Some(format!("Post-script error: {}", e)),
                            error_type: "post_script".to_string(),
                            timestamp: now_millis(),
                            dns_ms: 0.0,
                            tcp_ms: 0.0,
                            tls_ms: 0.0,
                            send_ms: 0.0,
                            ttfb_ms: 0.0,
                            download_ms: 0.0,
                        });
                        continue;
                    }
                }
            }

            // "Record request details": response-side fill-in (based on the script-processed effective payload)
            if let Some(mut entry) = msg_capture.take() {
                entry.status = response.status_code;
                entry.response_headers = response.metadata.clone();
                let (body, trunc) = body_to_string(&effective_payload);
                entry.response_body = body;
                entry.response_size = effective_payload.len();
                entry.response_truncated = trunc;
                self.push_captured(entry);
            }

            samples.push(
                self.finalize_request_sample(
                    step,
                    vu_id,
                    iteration,
                    start,
                    ProtocolResponse {
                        payload: effective_payload,
                        ..response
                    },
                    None,
                    false,
                )
                .await,
            );
        }

        self.protocol_for(&kind).disconnect().await.ok();
        Ok(samples)
    }

    /// Pre-script encoding hook: rewrites the payload via pm.request.raw (byte string)
    fn run_payload_pre_script(&mut self, script: &str, target: &str, payload: &mut Vec<u8>) {
        orbit_js::with_sandbox(|sandbox| {
            let Some(sandbox) = sandbox else { return };
            let mut ctx = orbit_js::RequestContext {
                url: target.to_string(),
                method: String::new(),
                headers: HashMap::new(),
                body: String::from_utf8_lossy(payload).into_owned(),
                raw: bytes_to_byte_string(payload),
            };
            let env_vars = self.variables.clone();
            let result = sandbox.run_pre_request(script, &mut ctx, Some(&env_vars), None);
            if result.success {
                *payload = byte_string_to_bytes(&ctx.raw);
            }
        });
    }

    /// Post-script decoding hook: runs the script and takes pm.response.decoded as the decode result
    fn run_decoded_post_script(
        &mut self,
        script: &str,
        response: &ProtocolResponse,
    ) -> Result<Option<Vec<u8>>, String> {
        orbit_js::with_sandbox(|sandbox| {
            let Some(sandbox) = sandbox else {
                return Ok(None);
            };
            let ctx = orbit_js::ResponseContext {
                status: response.status_code as u16,
                body: String::from_utf8_lossy(&response.payload).into_owned(),
                headers: response.metadata.iter().cloned().collect(),
                duration_ms: response.timings.total.as_millis() as u64,
                raw: bytes_to_byte_string(&response.payload),
                decoded: None,
            };
            let env_vars = self.variables.clone();
            let result = sandbox.run_post_response(script, &ctx, Some(&env_vars), None);
            if !result.success {
                return Err(result.error.unwrap_or_else(|| "post script failed".into()));
            }
            Ok(result.decoded.map(|d| d.as_bytes().to_vec()))
        })
    }

    /// Evaluates a condition expression
    ///
    /// Supported formats:
    /// - `${variable}` -> true when non-empty and not "false"/"0"
    /// - `${status} == 200` -> simple comparison
    /// - `${response.status} == 200` -> same as the simple comparison
    pub fn evaluate_condition(&self, expression: &str) -> Result<bool, String> {
        // Interpolate variables first
        let resolved = orbit_config::interpolate(expression, &self.variables)
            .unwrap_or_else(|_| expression.to_string());

        let expr = resolved.trim();

        // Empty expression -> false
        if expr.is_empty() {
            return Ok(false);
        }

        // Expressions with comparison operators
        for op in &["==", "!=", ">=", "<=", ">", "<"] {
            if let Some(pos) = expr.find(op) {
                let left = expr[..pos].trim();
                let right = expr[pos + op.len()..].trim();
                let right_trimmed = right.trim_matches('"').trim_matches('\'');

                // Try numeric comparison
                if let (Ok(l), Ok(r)) = (left.parse::<f64>(), right_trimmed.parse::<f64>()) {
                    return Ok(match *op {
                        "==" => (l - r).abs() < f64::EPSILON,
                        "!=" => (l - r).abs() >= f64::EPSILON,
                        ">=" => l >= r,
                        "<=" => l <= r,
                        ">" => l > r,
                        "<" => l < r,
                        _ => false,
                    });
                }

                // String comparison
                return Ok(match *op {
                    "==" => left == right_trimmed,
                    "!=" => left != right_trimmed,
                    ">=" => left >= right_trimmed,
                    "<=" => left <= right_trimmed,
                    ">" => left > right_trimmed,
                    "<" => left < right_trimmed,
                    _ => false,
                });
            }
        }

        // Containment check: the "contains" keyword
        if expr.contains(" contains ") {
            let parts: Vec<&str> = expr.splitn(2, " contains ").collect();
            if parts.len() == 2 {
                let left = parts[0].trim();
                let right = parts[1].trim().trim_matches('"').trim_matches('\'');
                return Ok(left.contains(right));
            }
        }

        // Simple truthiness check
        Ok(!matches!(expr, "false" | "0" | "null" | ""))
    }
}

/// Current Unix millisecond timestamp (metric samples' timestamp field)
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ─── Helper functions ──────────────────────────────────────────────────

/// Non-HTTP protocol config -> protocol-agnostic PipelineSpec (interpolation/scripts/encoding handled by the pipeline).
/// The payload string goes into body_value, interpolated by the pipeline per the interpolate flag, then encoded.
fn spec_to_pipeline(spec: &RequestSpec) -> PipelineSpec {
    let timeout = |t: &Option<String>| {
        t.as_ref()
            .and_then(|v| orbit_config::parse_duration(v).ok())
            .map(std::time::Duration::from_secs_f64)
            .unwrap_or(std::time::Duration::from_secs(30))
    };
    match spec {
        RequestSpec::WebSocket(cfg) => PipelineSpec {
            protocol: "websocket".into(),
            target: cfg.url.clone(),
            operation: String::new(),
            body_value: cfg.message.clone().map(serde_yaml::Value::String),
            timeout: Some(timeout(&cfg.read_timeout)),
            options: ProtocolOptions {
                ws: Some(WsCallOptions {
                    message_type: cfg.message_type,
                    close_after: cfg.close_after,
                }),
                ..Default::default()
            },
            interpolate: true,
            ..Default::default()
        },
        RequestSpec::Grpc(cfg) => PipelineSpec {
            protocol: "grpc".into(),
            target: cfg.url.clone(),
            operation: cfg.service.clone(),
            headers: cfg
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            body_value: cfg.message.clone().map(serde_yaml::Value::String),
            timeout: Some(std::time::Duration::from_secs(30)),
            streaming_mode: cfg.streaming,
            request_format: Some(cfg.message_format.clone()),
            response_format: cfg.response_format.clone(),
            options: ProtocolOptions {
                grpc: Some(orbit_protocol::GrpcCallOptions { compress: false }),
                ..Default::default()
            },
            interpolate: true,
            ..Default::default()
        },
        RequestSpec::Tcp(cfg) => PipelineSpec {
            protocol: "tcp".into(),
            target: normalize_host_port(&cfg.url, "tcp://"),
            operation: String::new(),
            body_value: cfg.payload.clone().map(serde_yaml::Value::String),
            timeout: Some(timeout(&cfg.read_timeout)),
            options: ProtocolOptions {
                framing: cfg.framing.clone(),
                ..Default::default()
            },
            interpolate: true,
            ..Default::default()
        },
        RequestSpec::Udp(cfg) => PipelineSpec {
            protocol: "udp".into(),
            target: normalize_host_port(&cfg.url, "udp://"),
            operation: String::new(),
            body_value: cfg.payload.clone().map(serde_yaml::Value::String),
            timeout: Some(timeout(&cfg.read_timeout)),
            interpolate: true,
            ..Default::default()
        },
        RequestSpec::Sse(cfg) => {
            let mut options = ProtocolOptions::default();
            if let Some(n) = cfg.max_events {
                options
                    .extra
                    .insert("max_events".into(), serde_json::json!(n));
            }
            PipelineSpec {
                protocol: "sse".into(),
                target: cfg.url.clone(),
                operation: "GET".into(),
                headers: cfg
                    .headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                timeout: Some(timeout(&cfg.duration)),
                options,
                interpolate: true,
                ..Default::default()
            }
        }
        RequestSpec::Graphql(cfg) => {
            let mut metadata: HashMap<String, String> = cfg
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if let Some(v) = &cfg.variables {
                metadata.insert("graphql-variables".into(), v.clone());
            }
            if let Some(n) = &cfg.operation_name {
                metadata.insert("graphql-operation-name".into(), n.clone());
            }
            PipelineSpec {
                protocol: "graphql".into(),
                target: cfg.url.clone(),
                operation: "POST".into(),
                headers: metadata,
                body_value: cfg.query.clone().map(serde_yaml::Value::String),
                timeout: Some(std::time::Duration::from_secs(30)),
                interpolate: true,
                ..Default::default()
            }
        }
        RequestSpec::Http(_) => PipelineSpec {
            protocol: "http".into(),
            ..Default::default()
        },
    }
}

fn spec_display(spec: &RequestSpec) -> String {
    match spec {
        RequestSpec::WebSocket(c) => format!("ws {}", c.url),
        RequestSpec::Grpc(c) => format!("grpc {}", c.service),
        RequestSpec::Tcp(c) => format!("tcp {}", c.url),
        RequestSpec::Udp(c) => format!("udp {}", c.url),
        RequestSpec::Sse(c) => format!("sse {}", c.url),
        RequestSpec::Graphql(c) => format!("graphql {}", c.url),
        RequestSpec::Http(_) => String::new(),
    }
}

/// Strips the tcp:// / udp:// scheme prefix, returning "host:port"
fn normalize_host_port(input: &str, scheme: &str) -> String {
    input
        .strip_prefix(scheme)
        .map(|s| s.to_string())
        .unwrap_or_else(|| input.to_string())
}

/// Gets the long-lived message sequence (supported only by WebSocket/TCP/UDP)
fn spec_messages(spec: &RequestSpec) -> Option<&[MessageSpec]> {
    match spec {
        RequestSpec::WebSocket(c) => c.messages.as_deref(),
        RequestSpec::Tcp(c) => c.messages.as_deref(),
        RequestSpec::Udp(c) => c.messages.as_deref(),
        _ => None,
    }
}

/// Long-lived connection target address
fn spec_target(spec: &RequestSpec) -> String {
    match spec {
        RequestSpec::WebSocket(c) => c.url.clone(),
        RequestSpec::Tcp(c) => c.url.clone(),
        RequestSpec::Udp(c) => c.url.clone(),
        _ => String::new(),
    }
}

/// Protocol options for one message (WS message type / TCP framing)
fn seq_options(spec: &RequestSpec, msg: &MessageSpec) -> ProtocolOptions {
    match spec {
        RequestSpec::WebSocket(c) => ProtocolOptions {
            ws: Some(WsCallOptions {
                message_type: msg.message_type.unwrap_or(c.message_type),
                close_after: c.close_after,
            }),
            ..Default::default()
        },
        RequestSpec::Tcp(c) => ProtocolOptions {
            framing: c.framing.clone(),
            ..Default::default()
        },
        _ => ProtocolOptions::default(),
    }
}

/// Decodes the payload by payload_type (text/base64/hex -> bytes)
fn decode_payload(value: &str, ptype: PayloadType) -> Vec<u8> {
    match ptype {
        PayloadType::Text => value.as_bytes().to_vec(),
        PayloadType::Base64 => BASE64_STANDARD
            .decode(value)
            .ok()
            .unwrap_or_else(|| value.as_bytes().to_vec()),
        PayloadType::Hex => {
            let s: String = value.chars().filter(|c| !c.is_whitespace()).collect();
            let mut out = Vec::with_capacity(s.len() / 2);
            let mut iter = s.chars();
            while let (Some(h), Some(l)) = (iter.next(), iter.next()) {
                if let (Some(hv), Some(lv)) = (hex_val(h), hex_val(l)) {
                    out.push((hv << 4) | lv);
                }
            }
            out
        }
    }
}

fn hex_val(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        _ => None,
    }
}

/// Bytes -> byte string (each char 0-255), for QuickJS scripts to read/write pm.request.raw / pm.response.raw
fn bytes_to_byte_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Byte string -> bytes
fn byte_string_to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u8).collect()
}

// ═══════════════════════════════════════════════════════════
// Unit tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_config::Step;
    use std::collections::HashMap;

    fn make_req_step(name: &str, url: &str) -> Step {
        Step::Request {
            name: name.to_string(),
            request: RequestSpec::Http(Box::new(orbit_config::HttpRequestConfig {
                method: "GET".to_string(),
                url: url.to_string(),
                headers: HashMap::new(),
                body: None,
                timeout: "30s".to_string(),
                payload_format: None,
                grpc_service: None,
                grpc_use_reflection: false,
                response_format: None,
            })),
            protocol: None,
            protocol_id: None,
            checks: vec![],
            extract: vec![],
            pre_script: None,
            post_script: None,
            pre_resolve_actions: vec![],
            pre_resolve_script: None,
            pre_actions: vec![],
            post_actions: vec![],
            tags: vec![],
            disabled: false,
        }
    }

    #[test]
    fn step_name_all_variants() {
        assert_eq!(make_req_step("req", "http://localhost").name(), "req");
        assert_eq!(
            Step::Loop {
                name: "lp".into(),
                count: 3,
                steps: vec![],
                disabled: false
            }
            .name(),
            "lp"
        );
        assert_eq!(
            Step::Wait {
                name: "w".into(),
                duration: "1s".into(),
                disabled: false
            }
            .name(),
            "w"
        );
        assert_eq!(
            Step::SetVar {
                name: "sv".into(),
                key: "k".into(),
                value: "v".into(),
                disabled: false
            }
            .name(),
            "sv"
        );
        assert_eq!(
            Step::Condition {
                name: "c".into(),
                expression: "1==1".into(),
                then: vec![],
                else_steps: vec![],
                disabled: false
            }
            .name(),
            "c"
        );
        assert_eq!(
            Step::Group {
                name: "g".into(),
                steps: vec![],
                disabled: false
            }
            .name(),
            "g"
        );
    }

    #[test]
    fn step_is_disabled() {
        let s = Step::Wait {
            name: "w".into(),
            duration: "1s".into(),
            disabled: true,
        };
        assert!(s.is_disabled());
        let s2 = Step::Wait {
            name: "w".into(),
            duration: "1s".into(),
            disabled: false,
        };
        assert!(!s2.is_disabled());
    }

    #[test]
    fn step_is_request_type() {
        assert!(make_req_step("r", "http://localhost").is_request());
        assert!(!Step::Wait {
            name: "w".into(),
            duration: "1s".into(),
            disabled: false
        }
        .is_request());
    }

    #[test]
    fn try_as_request_returns_config() {
        let step = make_req_step("r", "http://api.example.com/users");
        let (cfg, _) = step.try_as_request().unwrap();
        if let RequestSpec::Http(h) = cfg {
            assert_eq!(h.url, "http://api.example.com/users");
            assert_eq!(h.method, "GET");
        } else {
            panic!("expected Http request spec");
        }
    }

    #[test]
    fn try_as_request_returns_none_for_non_request() {
        let step = Step::Wait {
            name: "w".into(),
            duration: "1s".into(),
            disabled: false,
        };
        assert!(step.try_as_request().is_none());
    }

    #[test]
    fn test_encode_body_msgpack_routing() {
        let mut runner = make_dummy_runner();
        let body: serde_yaml::Value = serde_yaml::from_str("a: 1\nb: x").unwrap();
        let bytes = runner.runtime.encode_body(&body, Some("msgpack"));
        assert!(!bytes.is_empty());

        let codec = orbit_codec::registry::build_codec(orbit_codec::registry::CodecKind::MsgPack);
        let decoded = codec.decode(&bytes).unwrap();
        match decoded {
            orbit_codec::DataValue::Object(m) => {
                assert_eq!(m.get("a"), Some(&orbit_codec::DataValue::Int(1)));
                assert_eq!(
                    m.get("b"),
                    Some(&orbit_codec::DataValue::String("x".into()))
                );
            }
            other => panic!("expected Object, got {:?}", other),
        }
    }

    #[test]
    fn test_decode_body_content_type_routing() {
        let mut runner = make_dummy_runner();
        // msgpack-encoded response + Content-Type hint -> should decode as msgpack
        let codec = orbit_codec::registry::build_codec(orbit_codec::registry::CodecKind::MsgPack);
        let payload = codec
            .encode(&orbit_codec::DataValue::String("hello".into()))
            .unwrap();
        let metadata = vec![(
            "content-type".to_string(),
            "application/msgpack".to_string(),
        )];
        let decoded = runner
            .runtime
            .decode_body(&payload, &metadata, None)
            .expect("decode via content-type");
        assert_eq!(decoded, orbit_codec::DataValue::String("hello".into()));

        // Explicit response_format wins over Content-Type
        let decoded = runner
            .runtime
            .decode_body(&payload, &metadata, Some("binary"))
            .expect("decode via response_format");
        assert!(matches!(decoded, orbit_codec::DataValue::Bytes(_)));
    }

    /// Builds a test-only FlowRunner (dummy protocol client, produces no real requests)
    fn make_dummy_runner() -> FlowRunner {
        FlowRunner {
            runtime: PipelineRuntime::new(Box::new(DummyProtocolClient), Box::new(DummyCodec)),
            scenario: String::new(),
            cancel: CancellationToken::new(),
            variables: HashMap::new(),
            cookie_jar: CookieJar::new(),
            capture_requests: false,
            captured: VecDeque::new(),
        }
    }

    #[tokio::test]
    async fn loop_index_exposed_to_setvar_with_arithmetic() {
        let mut runner = make_dummy_runner();
        let steps = vec![Step::Loop {
            name: "lp".into(),
            count: 3,
            steps: vec![Step::SetVar {
                name: "sv".into(),
                key: "offset".into(),
                value: "${=loop.index * 10}".into(),
                disabled: false,
            }],
            disabled: false,
        }];
        let _ = runner.execute_steps(&steps, 0, 0, false).await;
        // Final round loop.index == 2 -> offset == 20
        assert_eq!(runner.variables().get("offset"), Some(&"20".to_string()));
        // loop.index is cleaned up after the loop, not polluting later steps
        assert_eq!(runner.variables().get("loop.index"), None);
    }

    #[tokio::test]
    async fn loop_index_usable_in_condition() {
        let mut runner = make_dummy_runner();
        let steps = vec![Step::Loop {
            name: "lp".into(),
            count: 2,
            steps: vec![Step::Condition {
                name: "c".into(),
                expression: "${loop.index} == 0".into(),
                then: vec![Step::SetVar {
                    name: "s".into(),
                    key: "hit".into(),
                    value: "yes".into(),
                    disabled: false,
                }],
                else_steps: vec![],
                disabled: false,
            }],
            disabled: false,
        }];
        let _ = runner.execute_steps(&steps, 0, 0, false).await;
        // The then branch is hit only when loop.index == 0
        assert_eq!(runner.variables().get("hit"), Some(&"yes".to_string()));
    }

    /// Verifies FlowRunner's condition-expression evaluation (pure logic, no protocol client needed)
    #[test]
    fn evaluate_condition_empty() {
        assert!(!eval_cond(""));
    }

    #[test]
    fn evaluate_condition_literal_true() {
        assert!(eval_cond("true"));
        assert!(!eval_cond("false"));
        assert!(!eval_cond("0"));
    }

    #[test]
    fn evaluate_condition_numeric_eq() {
        let mut vars = HashMap::new();
        vars.insert("status".into(), "200".into());
        assert!(eval_cond_with_vars("${status} == 200", &vars));
        assert!(!eval_cond_with_vars("${status} == 404", &vars));
    }

    #[test]
    fn evaluate_condition_numeric_comparisons() {
        let mut vars = HashMap::new();
        vars.insert("count".into(), "5".into());
        assert!(eval_cond_with_vars("${count} >= 3", &vars));
        assert!(eval_cond_with_vars("${count} <= 10", &vars));
        assert!(eval_cond_with_vars("${count} != 0", &vars));
    }

    #[test]
    fn evaluate_condition_contains() {
        let mut vars = HashMap::new();
        vars.insert("body".into(), r#"{"status":"ok"}"#.into());
        assert!(eval_cond_with_vars(r#"${body} contains "ok""#, &vars));
        assert!(!eval_cond_with_vars(r#"${body} contains "error""#, &vars));
    }

    #[test]
    fn evaluate_condition_string_comparison() {
        let mut vars = HashMap::new();
        vars.insert("method".into(), "POST".into());
        assert!(eval_cond_with_vars(r#"${method} == "POST""#, &vars));
        assert!(!eval_cond_with_vars(r#"${method} == "GET""#, &vars));
    }

    #[test]
    fn evaluate_condition_falsy_values() {
        assert!(!eval_cond_with_vars("0", &HashMap::new()));
        assert!(!eval_cond_with_vars("false", &HashMap::new()));
        assert!(!eval_cond_with_vars("null", &HashMap::new()));
    }

    // ─── helpers ──────────────────────────────────────────

    fn eval_cond(expr: &str) -> bool {
        eval_cond_with_vars(expr, &HashMap::new())
    }

    fn eval_cond_with_vars(expr: &str, vars: &HashMap<String, String>) -> bool {
        // Use a minimal runner (no protocol client needed for condition evaluation)
        // We can't easily create a dummy ProtocolClient without a mock impl,
        // but evaluate_condition doesn't use the protocol client at all.
        // The issue is FlowRunner requires Box<dyn ProtocolClient>.
        // Workaround: test evaluate_condition directly via the private method.
        // Since Rust allows testing private functions in the same crate:
        // evaluate_condition uses neither protocol client nor codec; constructing the runtime directly is enough.
        let runner = FlowRunner {
            runtime: PipelineRuntime::new(Box::new(DummyProtocolClient), Box::new(DummyCodec)),
            scenario: String::new(),
            cancel: CancellationToken::new(),
            variables: vars.clone(),
            cookie_jar: CookieJar::new(),
            capture_requests: false,
            captured: VecDeque::new(),
        };
        runner.evaluate_condition(expr).unwrap_or(false)
    }

    /// Empty protocol client stub, used only for testing condition-expression evaluation
    #[derive(Clone)]
    struct DummyProtocolClient;
    #[async_trait::async_trait]
    impl orbit_protocol::traits::ProtocolClient for DummyProtocolClient {
        fn clone_client(&self) -> Box<dyn orbit_protocol::traits::ProtocolClient> {
            Box::new(self.clone())
        }
        fn name(&self) -> &str {
            "dummy"
        }
        async fn execute(
            &mut self,
            _req: orbit_protocol::types::ProtocolRequest,
        ) -> Result<orbit_protocol::types::ProtocolResponse, orbit_protocol::ProtocolError>
        {
            Err(orbit_protocol::ProtocolError::Connect("dummy".into()))
        }
    }

    /// Empty codec stub
    #[derive(Clone)]
    struct DummyCodec;
    impl orbit_codec::traits::Codec for DummyCodec {
        fn clone_codec(&self) -> Box<dyn orbit_codec::traits::Codec> {
            Box::new(self.clone())
        }
        fn name(&self) -> &str {
            "dummy"
        }
        fn mime_types(&self) -> Vec<&str> {
            vec![]
        }
        fn encode(
            &self,
            _value: &orbit_codec::DataValue,
        ) -> Result<Vec<u8>, orbit_codec::CodecError> {
            Err(orbit_codec::CodecError::Encode("dummy".into()))
        }
        fn decode(&self, _bytes: &[u8]) -> Result<orbit_codec::DataValue, orbit_codec::CodecError> {
            Err(orbit_codec::CodecError::Decode("dummy".into()))
        }
    }
}
