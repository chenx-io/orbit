//! Controller - distributed load-testing coordinator (dual mode)
//!
//! - server mode: the controller acts as a gRPC client connecting to agents (`AgentService`);
//! - client mode: the controller acts as a gRPC server (`ControllerService`) and accepts inbound agent connections.

use std::collections::HashMap;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::Stream;
use futures_util::StreamExt;
use orbit_config::TestPlan;
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};

use crate::conv::{labels_from_proto, proto_to_resource, proto_to_snapshot};
use crate::merger::MetricsMerger;
use crate::proto::agent_service_client::AgentServiceClient;
use crate::proto::controller_service_server::{
    ControllerService as ControllerTrait, ControllerServiceServer,
};
use crate::proto::{
    AgentEvent, PingRequest, PingResponse, RegisterRequest, RegisterResponse, TaskCommand,
};
use crate::registry::{AgentRegistry, RegistryEvent};
use crate::types::{
    AgentInfo, AgentMetricsSnapshot, AgentState, DistributedError, ExecuteRequestData,
    ExecuteResult, PlanPartition, ResourceSnapshot,
};

/// Runtime info for the controller gRPC server (client mode)
pub struct ControllerServerInfo {
    pub addr: SocketAddr,
    pub handle: tokio::task::JoinHandle<()>,
}

/// Shared controller state
#[derive(Clone)]
pub struct ControllerState {
    pub registry: AgentRegistry,
    pub merger: Arc<Mutex<MetricsMerger>>,
    /// client mode: pending attachment info between Register and the CommandStream being established
    pending: Arc<Mutex<HashMap<String, RegisterRequest>>>,
    /// Pending-response table for single-request execution (over the bidi channel in client mode)
    pending_executes: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<ExecuteResult>>>>,
    /// server mode: per-agent event-consumer tasks (aborted on remove/re-add so a stale stream ending cannot wrongly mark the agent offline)
    event_tasks: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// server mode: Ping loops (the previous loop is aborted on re-add)
    ping_tasks: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// Connection generation of the Ping loop: incremented on every add; a loop checks that it is still current before writing taken_over
    ping_gens: Arc<Mutex<HashMap<String, u64>>>,
    /// Runtime state of the controller gRPC server (client mode)
    pub controller_server: Arc<Mutex<Option<ControllerServerInfo>>>,
    /// task_id → agent_id → latest metrics snapshot (polled by the frontend for live metrics instead of event push)
    task_snapshots: Arc<Mutex<HashMap<String, HashMap<String, AgentMetricsSnapshot>>>>,
    /// task_id → set of agents that have already ended (finished/failed)
    task_done: Arc<Mutex<HashMap<String, HashSet<String>>>>,
}

impl ControllerState {
    pub fn new() -> Self {
        Self {
            registry: AgentRegistry::new(),
            merger: Arc::new(Mutex::new(MetricsMerger::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            pending_executes: Arc::new(Mutex::new(HashMap::new())),
            event_tasks: Arc::new(Mutex::new(HashMap::new())),
            ping_tasks: Arc::new(Mutex::new(HashMap::new())),
            ping_gens: Arc::new(Mutex::new(HashMap::new())),
            controller_server: Arc::new(Mutex::new(None)),
            task_snapshots: Arc::new(Mutex::new(HashMap::new())),
            task_done: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for ControllerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of adding a server-mode agent
pub struct AddAgentOutcome {
    pub agent_id: String,
    /// true = the agent is already held by another controller (no forced takeover)
    pub claimed: bool,
    pub agent: Option<AgentInfo>,
}

#[derive(Clone)]
pub struct Controller {
    pub state: Arc<ControllerState>,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            state: Arc::new(ControllerState::new()),
        }
    }

    // ── server mode: the controller connects to agents ─────

    /// Add a server-mode agent: connect → Register → establish the AssignTask bidirectional stream.
    pub async fn add_server_agent(
        &self,
        addr: String,
        agent_id: Option<String>,
        labels: Vec<(String, String)>,
        force: bool,
    ) -> Result<AddAgentOutcome, DistributedError> {
        // Validate the format first: garbage addresses fail immediately without any network call
        validate_agent_addr(&addr)?;
        // connect/register/stream-setup all use explicit timeouts so an unreachable address cannot hang the frontend on the OS-level TCP timeout
        let mut client = tokio::time::timeout(
            Duration::from_secs(5),
            AgentServiceClient::connect(grpc_endpoint(&addr)),
        )
        .await
        .map_err(|_| {
            DistributedError::Connection(format!(
                "timed out connecting to agent {addr} (5s); check that the address and port are correct"
            ))
        })?
        .map_err(|e| DistributedError::Connection(format!("connect agent {addr}: {e}")))?;

        // The registration id is only a handshake placeholder; the real id from the agent's Register response wins,
        // so agent_id in agent events matches the registry key (resource/heartbeat lookups then line up).
        let reg = RegisterRequest {
            agent_id: agent_id.unwrap_or_else(|| format!("agent-{}", uuid_simple())),
            mode: "server".into(),
            addr: addr.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            cpu_cores: 0,
            memory_mb: 0,
            labels: crate::conv::labels_to_proto(&labels),
        };
        let resp = tokio::time::timeout(
            Duration::from_secs(5),
            client.register(Request::new(reg.clone())),
        )
        .await
        .map_err(|_| {
            DistributedError::Connection(format!(
                "timed out registering agent {addr} (5s); that address may not be an orbit agent"
            ))
        })?
        .map_err(|e| DistributedError::Connection(format!("register failed: {e}")))?
        .into_inner();
        if !resp.accepted {
            return Err(DistributedError::Agent(resp.reason));
        }
        let id = if resp.agent_id.is_empty() {
            reg.agent_id.clone()
        } else {
            resp.agent_id.clone()
        };
        // Already held by another controller and takeover not confirmed → report it and let the frontend confirm
        if resp.claimed && !force {
            return Ok(AddAgentOutcome {
                agent_id: id,
                claimed: true,
                agent: None,
            });
        }

        let token = uuid_simple();
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<TaskCommand>(64);
        let mut assign_req = Request::new(tokio_stream::wrappers::ReceiverStream::new(cmd_rx));
        assign_req
            .metadata_mut()
            .insert("x-controller-token", token.parse().unwrap());
        let task_stream =
            tokio::time::timeout(Duration::from_secs(5), client.assign_task(assign_req))
                .await
                .map_err(|_| {
                    DistributedError::Connection(format!(
                        "timed out establishing the task stream with agent {addr} (5s)"
                    ))
                })?
                .map_err(|e| DistributedError::Connection(format!("assign_task failed: {e}")))?
                .into_inner();

        self.state.registry.register(
            id.clone(),
            "server".into(),
            addr.clone(),
            reg.version.clone(),
            reg.cpu_cores,
            reg.memory_mb,
            labels,
            cmd_tx,
        )?;
        self.state.registry.set_taken_over(&id, false);

        tracing::info!(agent_id = %id, addr = %addr, "server-mode agent connected");
        // Event handling: resource / heartbeat / task status / metrics / execution result
        // Abort the previous event task for the same id (delete → re-add) so a stale stream ending cannot wrongly mark the agent offline
        if let Some(old) = self.state.event_tasks.lock().unwrap().remove(&id) {
            old.abort();
        }
        if let Some(old) = self.state.ping_tasks.lock().unwrap().remove(&id) {
            old.abort();
        }
        // Bump the connection generation (before spawning, so the new loop does not fail its first tick check)
        let mut gens = self.state.ping_gens.lock().unwrap();
        let gen = gens.entry(id.clone()).or_insert(0);
        *gen += 1;
        let my_gen = *gen;
        drop(gens);
        let state2 = self.state.clone();
        let id2 = id.clone();
        let handle = tokio::spawn(async move {
            let mut events = task_stream;
            while let Some(ev) = events.next().await {
                match ev {
                    Ok(ev) => {
                        tracing::debug!(agent_id = %ev.agent_id, type = %ev.r#type, "agent event received");
                        handle_task_event(&state2, &ev);
                    }
                    Err(e) => {
                        tracing::warn!(agent_id = %id2, error = %e, "agent event stream error");
                        break;
                    }
                }
            }
            tracing::warn!(agent_id = %id2, "agent event stream ended");
            // Only mark offline while still registered (old tasks are aborted on re-add; this is a safety net)
            if state2.registry.get(&id2).is_some() {
                state2.registry.set_state(&id2, AgentState::Offline);
            }
        });
        self.state
            .event_tasks
            .lock()
            .unwrap()
            .insert(id.clone(), handle);

        // server mode heartbeat: the controller actively Pings (health check; resources/heartbeats still arrive over the AssignTask stream)
        {
            let registry = self.state.registry.clone();
            let addr_ping = addr.clone();
            let id_ping = id.clone();
            let token_ping = token.clone();
            let state_ping = self.state.clone();
            let ping_handle = tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(3));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tick.tick().await;
                    if let Ok(mut c) = AgentServiceClient::connect(grpc_endpoint(&addr_ping)).await
                    {
                        if let Ok(p) = c
                            .ping(Request::new(PingRequest {
                                agent_id: id_ping.clone(),
                                epoch: 0,
                                token: token_ping.clone(),
                            }))
                            .await
                        {
                            let p = p.into_inner();
                            // Verify this is still the current-generation Ping loop (so an in-flight stale response cannot wrongly set taken_over)
                            let is_current = state_ping
                                .ping_gens
                                .lock()
                                .unwrap()
                                .get(&id_ping)
                                .map(|g| *g == my_gen)
                                .unwrap_or(false);
                            if !is_current {
                                break;
                            }
                            tracing::debug!(agent_id = %id_ping, my_gen, is_current, is_owner = p.is_owner, "ping result");
                            if p.is_owner {
                                registry.set_taken_over(&id_ping, false);
                                registry.update_heartbeat(&id_ping, now_ms());
                            } else {
                                // Taken over by another controller: mark as invalid
                                registry.set_taken_over(&id_ping, true);
                            }
                        }
                    }
                }
            });
            self.state
                .ping_tasks
                .lock()
                .unwrap()
                .insert(id.clone(), ping_handle);
        }

        Ok(AddAgentOutcome {
            agent_id: id.clone(),
            claimed: false,
            agent: self.state.registry.get(&id),
        })
    }

    /// Ping test (health check for a server-mode agent)
    pub async fn ping_agent(&self, addr: String) -> Result<(), DistributedError> {
        let mut client = AgentServiceClient::connect(grpc_endpoint(&addr))
            .await
            .map_err(|e| DistributedError::Connection(e.to_string()))?;
        client
            .ping(Request::new(PingRequest {
                agent_id: String::new(),
                epoch: 0,
                token: String::new(),
            }))
            .await
            .map_err(|e| DistributedError::Connection(e.to_string()))?;
        Ok(())
    }

    /// Execute a single request on the given agent
    pub async fn execute_on_agent(
        &self,
        agent_id: &str,
        data: ExecuteRequestData,
    ) -> Result<ExecuteResult, DistributedError> {
        let agent = self
            .state
            .registry
            .get(agent_id)
            .ok_or_else(|| DistributedError::NotFound(agent_id.to_string()))?;
        if agent.state == AgentState::Paused {
            return Err(DistributedError::InvalidInput(format!(
                "agent {agent_id} is paused and cannot execute requests"
            )));
        }
        if agent.state == AgentState::Offline {
            return Err(DistributedError::InvalidInput(format!(
                "agent {agent_id} is offline and cannot execute requests"
            )));
        }
        // server / client alike: execute over the AssignTask bidi command channel with request/response correlation
        let tx = self
            .state
            .registry
            .cmd_tx(agent_id)
            .ok_or_else(|| DistributedError::NotFound(agent_id.to_string()))?;
        let req_id = format!("exec-{}", uuid_simple());
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        self.state
            .pending_executes
            .lock()
            .unwrap()
            .insert(req_id.clone(), ack_tx);
        let cmd = TaskCommand {
            task_id: req_id.clone(),
            action: "execute".into(),
            plan_yaml: String::new(),
            vu_fraction: 0.0,
            execute: Some(crate::conv::execute_data_to_proto(&data)),
        };
        tx.send(cmd)
            .await
            .map_err(|_| DistributedError::Agent("command channel closed".into()))?;
        tokio::time::timeout(Duration::from_secs(30), ack_rx)
            .await
            .map_err(|_| DistributedError::Agent("agent execution timed out".into()))?
            .map_err(|_| DistributedError::Agent("execution result channel closed".into()))
    }

    /// Current state of the controller gRPC server (client mode)
    pub fn controller_status(&self) -> Option<String> {
        self.state
            .controller_server
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.addr.to_string())
    }

    /// Start the controller gRPC server (client mode)
    pub fn start_controller_server(&self, port: u16) -> Result<String, DistributedError> {
        let mut guard = self.state.controller_server.lock().unwrap();
        if guard.is_some() {
            return Err(DistributedError::AlreadyRegistered(
                "controller already started".into(),
            ));
        }
        let addr: SocketAddr = format!("0.0.0.0:{port}")
            .parse()
            .map_err(|e| DistributedError::InvalidInput(format!("invalid port {port}: {e}")))?;
        let rt = tokio::runtime::Handle::try_current().map_err(|_| {
            DistributedError::Agent(
                "no Tokio runtime available; cannot start the controller server".into(),
            )
        })?;
        let ctrl = Arc::new(self.clone());
        let handle = rt.spawn(async move {
            if let Err(e) = ctrl.serve(addr).await {
                eprintln!("[distributed] controller server exit: {e}");
            }
        });
        let s = addr.to_string();
        guard.replace(ControllerServerInfo { addr, handle });
        Ok(s)
    }

    /// Stop the controller gRPC server
    pub fn stop_controller_server(&self) -> Result<(), DistributedError> {
        if let Some(info) = self.state.controller_server.lock().unwrap().take() {
            info.handle.abort();
        }
        // Disconnect and clear all agents (in client mode agent connections drop when the server stops)
        let event_ids: Vec<String> = self
            .state
            .event_tasks
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for id in event_ids {
            if let Some(h) = self.state.event_tasks.lock().unwrap().remove(&id) {
                h.abort();
            }
        }
        let ping_ids: Vec<String> = self
            .state
            .ping_tasks
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for id in ping_ids {
            if let Some(h) = self.state.ping_tasks.lock().unwrap().remove(&id) {
                h.abort();
            }
        }
        self.state.registry.clear();
        Ok(())
    }

    // ── client mode: the controller acts as the gRPC server ──

    pub async fn serve(self: Arc<Self>, addr: SocketAddr) -> Result<(), DistributedError> {
        let svc = ControllerGrpc {
            state: self.state.clone(),
        };
        Server::builder()
            .add_service(ControllerServiceServer::new(svc))
            .serve(addr)
            .await
            .map_err(|e| DistributedError::Connection(e.to_string()))
    }

    /// Registry event subscription (live refresh in the frontend)
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<RegistryEvent> {
        self.state.registry.subscribe()
    }

    pub fn agents(&self) -> Vec<AgentInfo> {
        self.state.registry.list()
    }

    pub fn pause_agent(&self, id: &str) -> Result<(), DistributedError> {
        self.send_action(id, "pause")?;
        self.state.registry.set_state(id, AgentState::Paused);
        Ok(())
    }

    pub fn resume_agent(&self, id: &str) -> Result<(), DistributedError> {
        self.send_action(id, "resume")?;
        self.state.registry.set_state(id, AgentState::Idle);
        Ok(())
    }

    pub fn remove_agent(&self, id: &str) -> Result<(), DistributedError> {
        if self.state.registry.get(id).is_none() {
            return Err(DistributedError::NotFound(id.to_string()));
        }
        self.send_action(id, "stop").ok();
        if let Some(h) = self.state.event_tasks.lock().unwrap().remove(id) {
            h.abort();
        }
        if let Some(h) = self.state.ping_tasks.lock().unwrap().remove(id) {
            h.abort();
        }
        self.state.registry.unregister(id);
        Ok(())
    }

    fn send_action(&self, id: &str, action: &str) -> Result<(), DistributedError> {
        let tx = self
            .state
            .registry
            .cmd_tx(id)
            .ok_or_else(|| DistributedError::NotFound(id.to_string()))?;
        let task_id = self
            .state
            .registry
            .get(id)
            .and_then(|a| a.task_id)
            .unwrap_or_default();
        let cmd = TaskCommand {
            task_id,
            action: action.to_string(),
            plan_yaml: String::new(),
            vu_fraction: 0.0,
            execute: None,
        };
        // The command channel sends asynchronously and async cannot be blocked here; send from a spawned task instead
        let tx2 = tx;
        tokio::spawn(async move {
            let _ = tx2.send(cmd).await;
        });
        Ok(())
    }

    // ── dynamic partitioning and task orchestration ────────

    /// Dynamic partitioning: static weight (CPU cores by default) × live resource correction factor
    pub fn partition_plan(&self, plan: &TestPlan) -> Vec<PlanPartition> {
        let agents = self.state.registry.dispatchable();
        let total: f64 = agents.iter().map(|(a, _)| effective_weight(a)).sum();
        if total <= 0.0 || agents.is_empty() {
            return vec![];
        }
        agents
            .into_iter()
            .map(|(a, _)| {
                let fraction = effective_weight(&a) / total;
                PlanPartition {
                    agent_id: a.id.clone(),
                    plan: scale_plan_vus(plan, fraction),
                    vu_fraction: fraction,
                }
            })
            .collect()
    }

    /// Dispatch a task: after partitioning, send TaskCommand(start) to every available agent
    pub async fn dispatch_task(
        &self,
        plan: &TestPlan,
        task_id: String,
    ) -> Result<usize, DistributedError> {
        self.dispatch_task_to(plan, task_id, None).await
    }

    /// Dispatch a task: to an explicit agent list if given (a single agent keeps the full plan), otherwise partition by resource weight.
    pub async fn dispatch_task_to(
        &self,
        plan: &TestPlan,
        task_id: String,
        agent_ids: Option<&[String]>,
    ) -> Result<usize, DistributedError> {
        let partitions = if let Some(ids) = agent_ids {
            let mut parts = Vec::new();
            for id in ids {
                let agent = self
                    .state
                    .registry
                    .get(id)
                    .ok_or_else(|| DistributedError::NotFound(id.clone()))?;
                if agent.state == AgentState::Paused {
                    return Err(DistributedError::InvalidInput(format!(
                        "agent {id} is paused and cannot be assigned tasks"
                    )));
                }
                if agent.state == AgentState::Offline {
                    return Err(DistributedError::InvalidInput(format!(
                        "agent {id} is offline and cannot be assigned tasks"
                    )));
                }
                parts.push(PlanPartition {
                    agent_id: id.clone(),
                    plan: plan.clone(),
                    vu_fraction: 1.0,
                });
            }
            parts
        } else {
            self.partition_plan(plan)
        };
        if partitions.is_empty() {
            return Err(DistributedError::Agent("no available agents".into()));
        }
        // New load-test round: reset the aggregated result and record the start time
        self.reset_result();
        // Also clear the previous round's task snapshots/done flags (keep only the current task)
        self.state.task_snapshots.lock().unwrap().clear();
        self.state.task_done.lock().unwrap().clear();
        let n = partitions.len();
        for p in partitions {
            let yaml = serde_yaml::to_string(&p.plan).map_err(|e| {
                DistributedError::InvalidInput(format!("failed to serialize partition: {e}"))
            })?;
            let tx = self
                .state
                .registry
                .cmd_tx(&p.agent_id)
                .ok_or_else(|| DistributedError::NotFound(p.agent_id.clone()))?;
            self.state
                .registry
                .set_task(&p.agent_id, Some(task_id.clone()));
            self.state
                .registry
                .set_state(&p.agent_id, AgentState::Running);
            let cmd = TaskCommand {
                task_id: task_id.clone(),
                action: "start".into(),
                plan_yaml: yaml,
                vu_fraction: p.vu_fraction,
                execute: None,
            };
            let _ = tx.send(cmd).await;
        }
        Ok(n)
    }

    /// Stop the given task: send stop to every agent running it
    pub fn stop_task(&self, task_id: &str) -> Result<usize, DistributedError> {
        let ids: Vec<String> = self
            .state
            .registry
            .list()
            .into_iter()
            .filter(|a| a.task_id.as_deref() == Some(task_id))
            .map(|a| a.id)
            .collect();
        for id in &ids {
            self.send_action(id, "stop")?;
        }
        Ok(ids.len())
    }

    /// Reset the aggregated result (called before a new load-test round starts)
    pub fn reset_result(&self) {
        self.state.merger.lock().unwrap().reset();
    }

    /// Get the current aggregated result (RPS computed from the actual elapsed time)
    pub fn last_result(&self) -> crate::types::DistributedResult {
        self.state.merger.lock().unwrap().finalize_elapsed()
    }

    /// Live task progress (polled by the Tauri frontend): the latest metrics snapshot per agent plus the list of finished agents.
    ///
    /// Replaces the `distributed-event` push: emitting from a Tauri background thread contends with the window message loop's lock,
    /// which could freeze the app while dragging/resizing windows, so the frontend now polls this API instead.
    pub fn task_snapshots(&self, task_id: &str) -> (Vec<AgentMetricsSnapshot>, Vec<String>) {
        let snaps = self.state.task_snapshots.lock().unwrap();
        let done = self.state.task_done.lock().unwrap();
        let snapshots = snaps
            .get(task_id)
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default();
        let done_agents = done
            .get(task_id)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        (snapshots, done_agents)
    }

    /// Heartbeat timeout sweep (offline detection), called periodically by the host process
    pub fn sweep_offline(&self, timeout_ms: i64) -> Vec<String> {
        self.state.registry.sweep_offline(timeout_ms)
    }

    /// Merge one finished agent snapshot (into the aggregated result)
    pub fn merge_finished_snapshot(&self, snapshot: crate::types::AgentMetricsSnapshot) {
        let _ = self.state.merger.lock().unwrap().merge_snapshot(snapshot);
    }

    pub fn finalize(&self, duration: Duration) -> crate::types::DistributedResult {
        self.state
            .merger
            .lock()
            .unwrap()
            .finalize(duration.as_secs_f64())
    }
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

/// gRPC service implementation for client mode
#[derive(Clone)]
pub struct ControllerGrpc {
    state: Arc<ControllerState>,
}

#[tonic::async_trait]
impl ControllerTrait for ControllerGrpc {
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let req = request.into_inner();
        self.state
            .pending
            .lock()
            .unwrap()
            .insert(req.agent_id.clone(), req.clone());
        tracing::info!(agent_id = %req.agent_id, "agent register (client mode)");
        Ok(Response::new(RegisterResponse {
            accepted: true,
            agent_id: req.agent_id,
            reason: String::new(),
            claimed: false,
            epoch: 0,
        }))
    }

    async fn ping(&self, _request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse {
            agent_id: String::new(),
            server_time_ms: now_ms(),
            is_owner: true,
        }))
    }

    type AssignTaskStream = Pin<Box<dyn Stream<Item = Result<TaskCommand, Status>> + Send>>;

    async fn assign_task(
        &self,
        request: Request<Streaming<AgentEvent>>,
    ) -> Result<Response<Self::AssignTaskStream>, Status> {
        let mut in_stream = request.into_inner();
        let first = in_stream
            .next()
            .await
            .ok_or_else(|| Status::aborted("agent did not send a first event"))?
            .map_err(|e| Status::internal(e.to_string()))?;
        let agent_id = first.agent_id.clone();

        let reg_req = self
            .state
            .pending
            .lock()
            .unwrap()
            .remove(&agent_id)
            .ok_or_else(|| Status::not_found(format!("agent {agent_id} is not registered")))?;
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<TaskCommand>(64);
        self.state
            .registry
            .register(
                reg_req.agent_id.clone(),
                "client".into(),
                String::new(),
                reg_req.version.clone(),
                reg_req.cpu_cores,
                reg_req.memory_mb,
                labels_from_proto(&reg_req.labels),
                cmd_tx,
            )
            .map_err(|e| Status::already_exists(e.to_string()))?;

        let state2 = self.state.clone();
        handle_task_event(&state2, &first);
        let id2 = agent_id.clone();
        tokio::spawn(async move {
            while let Some(ev) = in_stream.next().await {
                match ev {
                    Ok(ev) => handle_task_event(&state2, &ev),
                    Err(_) => break,
                }
            }
            state2.registry.set_state(&id2, AgentState::Offline);
        });
        let stream = tokio_stream::wrappers::ReceiverStream::new(cmd_rx).map(Ok::<_, Status>);
        Ok(Response::new(Box::pin(stream)))
    }
}

// ─── event handling ─────────────────────────────────────────

fn handle_task_event(state: &Arc<ControllerState>, ev: &AgentEvent) {
    state.registry.update_heartbeat(&ev.agent_id, now_ms());
    if let Some(r) = ev.resource.as_ref() {
        state
            .registry
            .update_resource(&ev.agent_id, proto_to_resource(r));
    }
    match ev.r#type.as_str() {
        "heartbeat" => tracing::debug!(agent_id = %ev.agent_id, "agent heartbeat"),
        "status_change" => state
            .registry
            .set_state(&ev.agent_id, state_from_i32(ev.state)),
        "started" => {
            state.registry.set_state(&ev.agent_id, AgentState::Running);
            state.registry.emit_task(
                &ev.agent_id,
                &ev.task_id,
                "started",
                AgentState::Running,
                None,
            );
        }
        "progress" => {
            let snap = ev.metrics.as_ref().map(proto_to_snapshot);
            if let Some(s) = snap.clone() {
                state
                    .task_snapshots
                    .lock()
                    .unwrap()
                    .entry(ev.task_id.clone())
                    .or_default()
                    .insert(ev.agent_id.clone(), s);
            }
            state.registry.emit_task(
                &ev.agent_id,
                &ev.task_id,
                "progress",
                AgentState::Running,
                snap,
            );
        }
        "finished" | "failed" => {
            state.registry.set_state(&ev.agent_id, AgentState::Idle);
            state.registry.set_task(&ev.agent_id, None);
            let snap = ev.metrics.as_ref().map(proto_to_snapshot);
            if let Some(s) = snap.clone() {
                state
                    .task_snapshots
                    .lock()
                    .unwrap()
                    .entry(ev.task_id.clone())
                    .or_default()
                    .insert(ev.agent_id.clone(), s.clone());
                let _ = state.merger.lock().unwrap().merge_snapshot(s);
            }
            state
                .task_done
                .lock()
                .unwrap()
                .entry(ev.task_id.clone())
                .or_default()
                .insert(ev.agent_id.clone());
            state.registry.emit_task(
                &ev.agent_id,
                &ev.task_id,
                if ev.r#type == "finished" {
                    "finished"
                } else {
                    "failed"
                },
                AgentState::Idle,
                snap,
            );
        }
        "execute_result" => {
            if let Some(result) = ev.execute_result.as_ref() {
                resolve_execute(state, &ev.task_id, result);
            }
        }
        _ => {}
    }
}

fn resolve_execute(
    state: &Arc<ControllerState>,
    task_id: &str,
    result: &crate::proto::ExecuteResponse,
) {
    if let Some(tx) = state.pending_executes.lock().unwrap().remove(task_id) {
        let _ = tx.send(crate::conv::proto_to_execute_result(result));
    }
}

fn state_from_i32(v: i32) -> AgentState {
    match v {
        1 => AgentState::Running,
        2 => AgentState::Paused,
        3 => AgentState::Offline,
        _ => AgentState::Idle,
    }
}

// ─── dynamic weight ─────────────────────────────────────────

fn effective_weight(a: &AgentInfo) -> f64 {
    let base = a.cpu_cores.max(1) as f64;
    base * resource_factor(a.resource.as_ref())
}

fn resource_factor(r: Option<&ResourceSnapshot>) -> f64 {
    match r {
        None => 1.0,
        Some(r) => factor_for(r.cpu_percent).min(factor_for(r.mem_percent)),
    }
}

fn factor_for(usage: f64) -> f64 {
    if usage < 50.0 {
        1.0
    } else if usage <= 85.0 {
        1.0 - (usage - 50.0) * 0.5 / 35.0
    } else {
        0.3
    }
}

fn scale_plan_vus(plan: &TestPlan, fraction: f64) -> TestPlan {
    let mut scaled = plan.clone();
    for scenario in &mut scaled.scenarios {
        match &mut scenario.executor {
            orbit_config::Executor::ConstantVus { vus, .. } => {
                *vus = (*vus as f64 * fraction).max(1.0) as u32;
            }
            orbit_config::Executor::RampingVus {
                start_vus,
                max_vus: _,
                stages,
            } => {
                *start_vus = (*start_vus as f64 * fraction).max(1.0) as u32;
                for stage in stages {
                    stage.target = (stage.target as f64 * fraction).max(1.0) as u32;
                }
            }
            orbit_config::Executor::ConstantArrivalRate {
                rate,
                pre_allocated_vus,
                ..
            } => {
                *rate = (*rate as f64 * fraction).max(1.0) as u32;
                *pre_allocated_vus = (*pre_allocated_vus as f64 * fraction).max(1.0) as u32;
            }
            orbit_config::Executor::Sequential { .. } => {}
        }
    }
    scaled
}

fn uuid_simple() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn grpc_endpoint(addr: &str) -> String {
    if addr.contains("://") {
        addr.to_string()
    } else {
        format!("http://{addr}")
    }
}

/// Address format validation for server-mode agents: `host:port` or `scheme://host:port`.
/// Garbage addresses (missing or invalid port) fail before any network call is issued.
fn validate_agent_addr(addr: &str) -> Result<(), DistributedError> {
    let trimmed = addr.trim();
    if trimmed.is_empty() {
        return Err(DistributedError::InvalidInput(
            "address must not be empty".into(),
        ));
    }
    let authority = if let Some((scheme, rest)) = trimmed.split_once("://") {
        if !matches!(scheme, "http" | "https") {
            return Err(DistributedError::InvalidInput(format!(
                "unsupported scheme '{scheme}'; expected http/https"
            )));
        }
        rest
    } else {
        trimmed
    };
    let Some((host, port)) = authority.rsplit_once(':') else {
        return Err(DistributedError::InvalidInput(format!(
            "invalid address '{addr}'; expected host:port (e.g. 127.0.0.1:9091)"
        )));
    };
    if host.trim().is_empty() {
        return Err(DistributedError::InvalidInput(format!(
            "invalid address '{addr}'; missing host"
        )));
    }
    port.parse::<u16>()
        .map_err(|_| DistributedError::InvalidInput(format!("invalid port '{port}'")))?;
    Ok(())
}
