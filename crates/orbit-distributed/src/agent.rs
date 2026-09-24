//! Agent - distributed load-testing execution node (dual mode)
//!
//! - server mode: the agent acts as a gRPC server (`AgentService`) that the controller connects to;
//! - client mode: the agent acts as a gRPC client connecting to the controller (`ControllerService.CommandStream` bidi).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use orbit_codec::json::JsonCodec;
use orbit_config::from_str;
use orbit_engine::Engine;
use orbit_metrics::{LocalMetricsBus, MetricsSink, MetricsSummary};
use orbit_protocol::http::HttpClient;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use crate::conv::{labels_to_proto, resource_to_proto, snapshot_to_proto};
use crate::proto::agent_service_server::{AgentService, AgentServiceServer};
use crate::proto::controller_service_client::ControllerServiceClient;
use crate::proto::{
    AgentEvent, ExecuteResponse, PingRequest, PingResponse, RegisterRequest, RegisterResponse,
    TaskCommand,
};
use crate::resource::ResourceCollector;
use crate::types::{
    AgentMetricsSnapshot, AgentState, DistributedError, ExecuteTimings, MetricsSummary as S,
    ResourceSnapshot, ScriptLogEntry, TestResultEntry,
};

/// Agent startup configuration (provided by the CLI)
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub id: String,
    pub mode: String,
    pub version: String,
    pub cpu_cores: u32,
    pub memory_mb: u64,
    pub labels: Vec<(String, String)>,
}

#[derive(Default)]
struct TaskState {
    task_id: Option<String>,
    abort: Option<Arc<AtomicBool>>,
}

/// Shared agent core: task execution + resource sampling
#[derive(Clone)]
pub struct AgentCore {
    cfg: AgentConfig,
    collector: Arc<Mutex<ResourceCollector>>,
    state: Arc<Mutex<TaskState>>,
}

impl AgentCore {
    pub fn new(cfg: AgentConfig) -> Self {
        Self {
            cfg,
            collector: Arc::new(Mutex::new(ResourceCollector::new())),
            state: Arc::new(Mutex::new(TaskState::default())),
        }
    }

    pub fn id(&self) -> &str {
        &self.cfg.id
    }

    pub fn register_request(&self, addr: &str) -> RegisterRequest {
        RegisterRequest {
            agent_id: self.cfg.id.clone(),
            mode: self.cfg.mode.clone(),
            addr: addr.to_string(),
            version: self.cfg.version.clone(),
            cpu_cores: self.cfg.cpu_cores,
            memory_mb: self.cfg.memory_mb,
            labels: labels_to_proto(&self.cfg.labels),
        }
    }

    pub fn resource_sample(&self) -> ResourceSnapshot {
        self.collector.lock().unwrap().sample()
    }

    /// Execute a single request with full pre/post script and per-phase timing support (same unified pipeline as local execution)
    ///
    /// **Division of labour between the two script stages**: the `url` / `headers` / `body` the agent receives are already the **final request**
    /// (the control side already performed "pre-interpolation actions + interpolation + encoding/assembly", see `dry_run`), so
    /// `interpolate = false`; `prereq_script` maps to the **post-interpolation** stage (signing/encryption over the final request).
    ///
    /// The agent protocol **does not carry** action lists or request templates: pre-interpolation actions have already run on the control side,
    /// DB actions are surfaced as warnings by the control side (`requestRunner`), never "silently dropped". If the agent is ever to run
    /// action lists, `ExecuteRequestPayload` and this function must be extended together.
    pub async fn execute_request(
        &self,
        payload: &crate::proto::ExecuteRequestPayload,
    ) -> Result<AgentExecuteResult, String> {
        let env: HashMap<String, String> = payload.env_vars.clone();

        let spec = orbit_engine::pipeline::PipelineSpec {
            protocol: "http".into(),
            target: payload.url.clone(),
            operation: payload.method.clone(),
            headers: payload.headers.clone(),
            body: payload.body.as_bytes().to_vec(),
            pre_scripts: if payload.prereq_script.is_empty() {
                Vec::new()
            } else {
                vec![payload.prereq_script.clone()]
            },
            post_scripts: if payload.postreq_script.is_empty() {
                Vec::new()
            } else {
                vec![payload.postreq_script.clone()]
            },
            interpolate: false,
            ..Default::default()
        };

        let mut rt = orbit_engine::pipeline::PipelineRuntime::new(
            Box::new(HttpClient::new()),
            Box::new(JsonCodec),
        );
        let mut vars: HashMap<String, String> = HashMap::new();
        let cancel = tokio_util::sync::CancellationToken::new();
        // A distributed agent uses a one-shot execution model (no cross-request session context); the Cookie Jar stays disabled for now,
        // if agent-side session persistence is needed, the controller can push a shared jar and wire it in later.
        let outcome =
            orbit_engine::pipeline::execute_pipeline(&mut rt, spec, &mut vars, &env, &cancel, None)
                .await;

        let Some(response) = &outcome.response else {
            let msg = outcome
                .error
                .as_ref()
                .map(|e| e.message())
                .unwrap_or_else(|| "request failed".into());
            return Err(format!("request execution failed: {}", msg));
        };

        let t = &response.timings;
        Ok(AgentExecuteResult {
            status: response.status_code as i32,
            headers: response
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            body: response.payload.clone(),
            duration_ms: response.duration_ms,
            timing: ExecuteTimings {
                dns_ms: t
                    .dns
                    .map(|d| (d.as_secs_f64() * 1000.0).round())
                    .unwrap_or(0.0),
                tcp_ms: t
                    .tcp
                    .map(|d| (d.as_secs_f64() * 1000.0).round())
                    .unwrap_or(0.0),
                tls_ms: t
                    .tls
                    .map(|d| (d.as_secs_f64() * 1000.0).round())
                    .unwrap_or(0.0),
                ttfb_ms: t
                    .first_byte
                    .map(|d| (d.as_secs_f64() * 1000.0).round())
                    .unwrap_or(0.0),
                download_ms: t
                    .receive
                    .map(|d| (d.as_secs_f64() * 1000.0).round())
                    .unwrap_or(0.0),
                total_ms: (t.total.as_secs_f64() * 1000.0).round(),
            },
            pre_logs: outcome
                .pre_logs
                .iter()
                .map(|l| ScriptLogEntry {
                    level: l.level.clone(),
                    message: l.message.clone(),
                })
                .collect(),
            post_logs: outcome
                .post_logs
                .iter()
                .map(|l| ScriptLogEntry {
                    level: l.level.clone(),
                    message: l.message.clone(),
                })
                .collect(),
            decoded: outcome.post_decoded,
            post_tests: outcome
                .post_tests
                .into_iter()
                .map(|t| TestResultEntry {
                    name: t.name,
                    passed: t.passed,
                    message: t.message,
                })
                .collect(),
        })
    }

    /// Start a task: parse the plan → run the engine → report metrics periodically → finish event.
    /// Events are written to `out` (an AgentEvent stream in server mode; the caller converts to AgentEvent in client mode).
    pub fn start_task(
        &self,
        task_id: String,
        plan_yaml: String,
        out: mpsc::Sender<AgentEvent>,
    ) -> Arc<AtomicBool> {
        let abort = Arc::new(AtomicBool::new(false));
        {
            let mut st = self.state.lock().unwrap();
            st.task_id = Some(task_id.clone());
            st.abort = Some(abort.clone());
        }
        let id = self.cfg.id.clone();
        let out2 = out.clone();
        let abort_in_task = abort.clone();
        tokio::spawn(async move {
            let _ = out2
                .send(task_event(
                    &id,
                    &task_id,
                    "started",
                    AgentState::Running,
                    None,
                    None,
                ))
                .await;
            let plan = match from_str(&plan_yaml) {
                Ok(p) => p,
                Err(e) => {
                    let _ = out2
                        .send(task_event(
                            &id,
                            &task_id,
                            "failed",
                            AgentState::Idle,
                            Some(format!("failed to parse plan: {e}")),
                            None,
                        ))
                        .await;
                    return;
                }
            };
            let engine = Engine::new();
            let bus = engine.metrics_bus();
            let protocol = Arc::new(HttpClient::new());
            let codec = Arc::new(JsonCodec);
            let run_fut =
                engine.run_with_abort(&plan, protocol, codec, Some(abort_in_task.clone()));
            tokio::pin!(run_fut);
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    res = &mut run_fut => {
                        match res {
                            Ok(summary) => {
                                tracing::info!(task_id = %task_id, "task finished (success)");
                                let snap = snapshot_from_bus(&id, &bus, &summary);
                                let _ = out2.send(task_event(
                                    &id, &task_id, "finished", AgentState::Idle,
                                    None, Some(snap),
                                )).await;
                            }
                            Err(e) => {
                                tracing::warn!(task_id = %task_id, error = %e, "task finished (failure)");
                                let _ = out2.send(task_event(
                                    &id, &task_id, "failed", AgentState::Idle,
                                    Some(e.to_string()), None,
                                )).await;
                            }
                        }
                        return;
                    }
                    _ = tick.tick() => {
                        let summary = bus.snapshot();
                        if summary.total_requests > 0 {
                            let snap = snapshot_from_bus(&id, &bus, &summary);
                            let _ = out2.send(task_event(
                                &id, &task_id, "progress", AgentState::Running,
                                None, Some(snap),
                            )).await;
                        }
                    }
                }
            }
        });
        abort
    }

    pub fn stop_task(&self) {
        if let Some(a) = self.state.lock().unwrap().abort.clone() {
            a.store(true, Ordering::SeqCst);
        }
    }

    /// Currently running task (None = idle)
    pub fn current_task(&self) -> Option<String> {
        self.state.lock().unwrap().task_id.clone()
    }

    pub fn clear_task(&self) {
        let mut st = self.state.lock().unwrap();
        st.task_id = None;
        st.abort = None;
    }
}

/// Single-request execution result (internal form)
pub struct AgentExecuteResult {
    pub status: i32,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub duration_ms: u64,
    pub timing: ExecuteTimings,
    pub pre_logs: Vec<ScriptLogEntry>,
    pub post_logs: Vec<ScriptLogEntry>,
    pub decoded: Option<String>,
    pub post_tests: Vec<TestResultEntry>,
}

fn to_execute_response(r: AgentExecuteResult) -> ExecuteResponse {
    let decoded = r.decoded.unwrap_or_default();
    ExecuteResponse {
        status: r.status,
        headers: r.headers.into_iter().collect(),
        body: String::from_utf8_lossy(&r.body).into_owned(),
        duration_ms: r.duration_ms as i64,
        error: String::new(),
        timing: Some(crate::proto::ExecuteTimings {
            dns_ms: r.timing.dns_ms,
            tcp_ms: r.timing.tcp_ms,
            tls_ms: r.timing.tls_ms,
            ttfb_ms: r.timing.ttfb_ms,
            download_ms: r.timing.download_ms,
            total_ms: r.timing.total_ms,
        }),
        pre_logs: r
            .pre_logs
            .into_iter()
            .map(|l| crate::proto::ScriptLogEntry {
                level: l.level,
                message: l.message,
            })
            .collect(),
        post_logs: r
            .post_logs
            .into_iter()
            .map(|l| crate::proto::ScriptLogEntry {
                level: l.level,
                message: l.message,
            })
            .collect(),
        decoded,
        post_tests: r
            .post_tests
            .into_iter()
            .map(|t| crate::proto::TestResultEntry {
                name: t.name,
                passed: t.passed,
                message: t.message,
            })
            .collect(),
    }
}

fn task_event(
    agent_id: &str,
    task_id: &str,
    type_: &str,
    state: AgentState,
    message: Option<String>,
    metrics: Option<AgentMetricsSnapshot>,
) -> AgentEvent {
    AgentEvent {
        agent_id: agent_id.to_string(),
        task_id: task_id.to_string(),
        r#type: type_.to_string(),
        message: message.unwrap_or_default(),
        state: i32::from(state),
        metrics: metrics.map(|m| snapshot_to_proto(&m)),
        resource: None,
        execute_result: None,
    }
}

fn to_execute_task_event(
    agent_id: &str,
    task_id: &str,
    res: Result<AgentExecuteResult, String>,
) -> AgentEvent {
    let (execute_result, error) = match res {
        Ok(r) => (to_execute_response(r), String::new()),
        Err(e) => (
            ExecuteResponse {
                status: 0,
                headers: HashMap::new(),
                body: String::new(),
                duration_ms: 0,
                error: String::new(),
                timing: None,
                pre_logs: Vec::new(),
                post_logs: Vec::new(),
                decoded: String::new(),
                post_tests: Vec::new(),
            },
            e,
        ),
    };
    AgentEvent {
        agent_id: agent_id.to_string(),
        task_id: task_id.to_string(),
        r#type: "execute_result".into(),
        message: String::new(),
        state: i32::from(AgentState::Idle),
        metrics: None,
        resource: None,
        execute_result: Some(ExecuteResponse {
            error,
            ..execute_result
        }),
    }
}

fn snapshot_from_bus(
    agent_id: &str,
    bus: &Arc<LocalMetricsBus>,
    summary: &MetricsSummary,
) -> AgentMetricsSnapshot {
    AgentMetricsSnapshot {
        agent_id: agent_id.to_string(),
        timestamp_ms: now_ms() as u64,
        active_vus: bus.active_vus(),
        total_requests: summary.total_requests,
        total_errors: summary.total_errors,
        hdr_histogram_b64: bus.histogram_b64().unwrap_or_default(),
        summary: Some(S {
            p50_ms: summary.p50_ms,
            p90_ms: summary.p90_ms,
            p95_ms: summary.p95_ms,
            p99_ms: summary.p99_ms,
            mean_ms: summary.mean_ms,
            rps: summary.rps,
            error_rate: summary.error_rate,
        }),
    }
}

// ─── mode A: server (the agent acts as the gRPC server) ─────

#[derive(Clone)]
pub struct AgentServer {
    core: AgentCore,
    /// Epoch of the currently attached controller: incremented on takeover by a new connection; an outdated loop stops once it notices
    epoch: Arc<std::sync::atomic::AtomicU64>,
    /// Whether an AssignTask stream from a controller is currently running
    claimed: Arc<std::sync::atomic::AtomicBool>,
    /// Token of the current owning controller
    active_token: Arc<std::sync::Mutex<Option<String>>>,
}

impl AgentServer {
    pub fn new(core: AgentCore) -> Self {
        Self {
            core,
            epoch: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            claimed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            active_token: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub async fn serve(self, addr: std::net::SocketAddr) -> Result<(), DistributedError> {
        tonic::transport::Server::builder()
            .add_service(AgentServiceServer::new(self))
            .serve(addr)
            .await
            .map_err(|e| DistributedError::Connection(e.to_string()))
    }
}

#[tonic::async_trait]
impl AgentService for AgentServer {
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let req = request.into_inner();
        tracing::info!(agent_id = %req.agent_id, mode = %req.mode, "agent register (server mode)");
        // Register is read-only and does not change ownership: the epoch only increments on an actual takeover (assign_task),
        // so a mere probe/confirmation does not evict the current controller.
        let epoch = self.epoch.load(std::sync::atomic::Ordering::SeqCst);
        let claimed = self.claimed.load(std::sync::atomic::Ordering::SeqCst);
        Ok(Response::new(RegisterResponse {
            accepted: true,
            agent_id: self.core.id().to_string(),
            reason: String::new(),
            claimed,
            epoch,
        }))
    }

    async fn ping(&self, request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        let req = request.into_inner();
        let is_owner = self.claimed.load(std::sync::atomic::Ordering::SeqCst)
            && self.active_token.lock().unwrap().as_deref() == Some(req.token.as_str());
        tracing::debug!(
            agent_id = %self.core.id(),
            req_token = %req.token,
            active_token = ?self.active_token.lock().unwrap().as_deref(),
            claimed = self.claimed.load(std::sync::atomic::Ordering::SeqCst),
            is_owner,
            "ping ownership"
        );
        Ok(Response::new(PingResponse {
            agent_id: self.core.id().to_string(),
            server_time_ms: now_ms(),
            is_owner,
        }))
    }

    type AssignTaskStream = ReceiverStream<Result<AgentEvent, Status>>;

    async fn assign_task(
        &self,
        request: Request<Streaming<TaskCommand>>,
    ) -> Result<Response<Self::AssignTaskStream>, Status> {
        // Single-controller ownership: refuse takeover while a task is running; otherwise take over by bumping the epoch (the old connection steps aside)
        if let Some(task_id) = self.core.current_task() {
            return Err(Status::failed_precondition(format!(
                "agent is running task {task_id}; refusing takeover by another controller"
            )));
        }
        // Takeover point: bump the epoch and claim ownership
        let my_epoch = self.epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        self.claimed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        tracing::info!(agent_id = %self.core.id(), epoch = my_epoch, "assign_task attached by a controller");
        let token = request
            .metadata()
            .get("x-controller-token")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let mut in_stream = request.into_inner();
        let (out_tx, out_rx) = mpsc::channel::<Result<AgentEvent, Status>>(128);
        let core = self.core.clone();
        let epoch_flag = self.epoch.clone();
        let claimed_flag = self.claimed.clone();
        let token_flag = self.active_token.clone();
        let id = self.core.id().to_string();
        *self.active_token.lock().unwrap() = Some(token.clone());
        tokio::spawn(async move {
            tracing::info!(agent_id = %id, "assign_task stream established; pushing resources/heartbeats");
            let mut res_tick = tokio::time::interval(Duration::from_secs(2));
            let mut hb_tick = tokio::time::interval(Duration::from_secs(3));
            res_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            hb_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut task_rx: Option<mpsc::Receiver<AgentEvent>> = None;
            let mut stream_closed = false;
            loop {
                tokio::select! {
                    cmd = in_stream.next() => {
                        match cmd {
                            Some(Ok(c)) => {
                                match c.action.as_str() {
                                    "start" => {
                                        tracing::info!(task_id = %c.task_id, "task received (server mode)");
                                        core.stop_task();
                                        let (tx, rx) = mpsc::channel(32);
                                        core.start_task(c.task_id.clone(), c.plan_yaml, tx);
                                        task_rx = Some(rx);
                                    }
                                    "stop" => core.stop_task(),
                                    "execute" => {
                                        if let Some(payload) = c.execute {
                                            tracing::info!(task_id = %c.task_id, url = %payload.url, "single-request execution received (server mode)");
                                            let res = core.execute_request(&payload).await;
                                            match &res {
                                                Ok(r) => tracing::info!(task_id = %c.task_id, status = r.status, duration_ms = r.duration_ms, "single-request execution finished (server mode)"),
                                                Err(e) => tracing::warn!(task_id = %c.task_id, error = %e, "single-request execution failed (server mode)"),
                                            }
                                            let ev = to_execute_task_event(&id, &c.task_id, res);
                                            let _ = out_tx.send(Ok(ev)).await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Some(Err(e)) => {
                                // Request stream error: log and keep pushing (the response stream is independent)
                                tracing::warn!(agent_id = %id, error = %e, "assign_task request stream error; continuing to push resources/heartbeats");
                                stream_closed = true;
                            }
                            None => {
                                // Request stream closed by the controller: still keep pushing resources/heartbeats
                                if !stream_closed {
                                    tracing::warn!(agent_id = %id, "assign_task request stream closed; continuing to push resources/heartbeats");
                                    stream_closed = true;
                                }
                            }
                        }
                    }
                    _ = res_tick.tick() => {
                        if epoch_flag.load(std::sync::atomic::Ordering::SeqCst) != my_epoch {
                            tracing::warn!(agent_id = %id, "taken over by a new controller; stopping push");
                            break;
                        }
                        let res = core.resource_sample();
                        tracing::debug!(agent_id = %id, cpu = %res.cpu_percent, "pushing resources");
                        if out_tx.send(Ok(AgentEvent {
                            agent_id: id.clone(),
                            task_id: String::new(),
                            r#type: "progress".into(),
                            message: String::new(),
                            state: i32::from(AgentState::Idle),
                            metrics: None,
                            resource: Some(resource_to_proto(&res)),
                            execute_result: None,
                        })).await.is_err() {
                            tracing::warn!(agent_id = %id, "response stream disconnected; stopping push");
                            break;
                        }
                    }
                    _ = hb_tick.tick() => {
                        if epoch_flag.load(std::sync::atomic::Ordering::SeqCst) != my_epoch {
                            tracing::warn!(agent_id = %id, "taken over by a new controller; stopping push");
                            break;
                        }
                        tracing::debug!(agent_id = %id, "pushing heartbeat");
                        if out_tx.send(Ok(AgentEvent {
                            agent_id: id.clone(),
                            task_id: String::new(),
                            r#type: "heartbeat".into(),
                            message: String::new(),
                            state: i32::from(AgentState::Idle),
                            metrics: None,
                            resource: None,
                            execute_result: None,
                        })).await.is_err() {
                            tracing::warn!(agent_id = %id, "response stream disconnected; stopping push");
                            break;
                        }
                    }
                    ev = async {
                        match &mut task_rx {
                            Some(rx) => rx.recv().await,
                            None => std::future::pending().await,
                        }
                    } => {
                        match ev {
                            Some(ev) => { let _ = out_tx.send(Ok(ev)).await; }
                            None => { task_rx = None; core.clear_task(); }
                        }
                    }
                }
            }
            // Clear the claim flag only while still the current owner (keep it true if taken over by a new controller)
            if epoch_flag.load(std::sync::atomic::Ordering::SeqCst) == my_epoch {
                claimed_flag.store(false, std::sync::atomic::Ordering::SeqCst);
                token_flag.lock().unwrap().take();
            }
            core.stop_task();
        });
        Ok(Response::new(ReceiverStream::new(out_rx)))
    }
}

// ─── mode B: client (the agent connects to the controller) ──

/// Run in client mode: register, then open a bidi task stream with the controller and report heartbeats/resources periodically.
pub async fn run_client(addr: String, core: AgentCore) -> Result<(), DistributedError> {
    let endpoint = if addr.contains("://") {
        addr.clone()
    } else {
        format!("http://{addr}")
    };
    let mut client = ControllerServiceClient::connect(endpoint)
        .await
        .map_err(|e| DistributedError::Connection(format!("connect controller {addr}: {e}")))?;

    let reg = core.register_request("");
    let resp = client
        .register(Request::new(reg))
        .await
        .map_err(|e| DistributedError::Agent(format!("register failed: {e}")))?
        .into_inner();
    if !resp.accepted {
        return Err(DistributedError::Agent(resp.reason));
    }
    tracing::info!(agent_id = %core.id(), "agent registered (client mode)");

    // Create the channel and start the uplink task first: the controller's AssignTask blocks awaiting the first AgentEvent,
    // so the stream must be established only after the uplink heartbeat/resource task has started, otherwise it deadlocks.
    let (event_tx, event_rx) = mpsc::channel::<AgentEvent>(128);
    let (task_tx, task_rx) = mpsc::channel::<AgentEvent>(64);
    let event_tx_shared = event_tx.clone();

    let uplink_core = core.clone();
    let _uplink = tokio::spawn(async move {
        let mut hb = tokio::time::interval(Duration::from_secs(3));
        let mut res_tick = tokio::time::interval(Duration::from_secs(2));
        let mut task_rx = task_rx;
        hb.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        res_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut running = false;
        loop {
            tokio::select! {
                _ = hb.tick() => {
                    let state = if running { AgentState::Running } else { AgentState::Idle };
                    if event_tx.send(AgentEvent {
                        agent_id: uplink_core.id().to_string(),
                        task_id: String::new(),
                        r#type: "heartbeat".into(),
                        message: String::new(),
                        state: i32::from(state),
                        metrics: None,
                        resource: None,
                        execute_result: None,
                    }).await.is_err() { break; }
                }
                _ = res_tick.tick() => {
                    let res = uplink_core.resource_sample();
                    let state = if running { AgentState::Running } else { AgentState::Idle };
                    if event_tx.send(AgentEvent {
                        agent_id: uplink_core.id().to_string(),
                        task_id: String::new(),
                        r#type: "resource".into(),
                        message: String::new(),
                        state: i32::from(state),
                        metrics: None,
                        resource: Some(resource_to_proto(&res)),
                        execute_result: None,
                    }).await.is_err() { break; }
                }
                ev = task_rx.recv() => {
                    match ev {
                        Some(ev) => {
                            if event_tx.send(ev.clone()).await.is_err() { break; }
                            running = matches!(ev.r#type.as_str(), "started" | "progress");
                            if ev.r#type == "finished" || ev.r#type == "failed" { uplink_core.clear_task(); }
                        }
                        None => break,
                    }
                }
            }
        }
    });

    let resp_stream = client
        .assign_task(Request::new(ReceiverStream::new(event_rx)))
        .await
        .map_err(|e| DistributedError::Agent(format!("command stream failed: {e}")))?
        .into_inner();

    // Downlink: read task commands issued by the controller
    let mut cmd_stream = resp_stream;
    while let Some(cmd) = cmd_stream.next().await {
        let cmd = cmd.map_err(|e| DistributedError::Agent(format!("command stream: {e}")))?;
        match cmd.action.as_str() {
            "start" => {
                tracing::info!(task_id = %cmd.task_id, "task received (client mode)");
                core.stop_task();
                core.start_task(cmd.task_id.clone(), cmd.plan_yaml, task_tx.clone());
            }
            "stop" => core.stop_task(),
            "execute" => {
                if let Some(payload) = cmd.execute {
                    tracing::info!(task_id = %cmd.task_id, url = %payload.url, "single-request execution received (client mode)");
                    let res = core.execute_request(&payload).await;
                    match &res {
                        Ok(r) => {
                            tracing::info!(task_id = %cmd.task_id, status = r.status, duration_ms = r.duration_ms, "single-request execution finished (client mode)")
                        }
                        Err(e) => {
                            tracing::warn!(task_id = %cmd.task_id, error = %e, "single-request execution failed (client mode)")
                        }
                    }
                    let (execute_result, error) = match res {
                        Ok(r) => (to_execute_response(r), String::new()),
                        Err(e) => (
                            ExecuteResponse {
                                status: 0,
                                headers: HashMap::new(),
                                body: String::new(),
                                duration_ms: 0,
                                error: String::new(),
                                timing: None,
                                pre_logs: Vec::new(),
                                post_logs: Vec::new(),
                                decoded: String::new(),
                                post_tests: Vec::new(),
                            },
                            e,
                        ),
                    };
                    let _ = event_tx_shared
                        .send(AgentEvent {
                            agent_id: core.id().to_string(),
                            task_id: cmd.task_id.clone(),
                            r#type: "execute_result".into(),
                            message: String::new(),
                            state: i32::from(AgentState::Idle),
                            metrics: None,
                            resource: None,
                            execute_result: Some(ExecuteResponse {
                                error,
                                ..execute_result
                            }),
                        })
                        .await;
                }
            }
            _ => {}
        }
    }
    core.stop_task();
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
