//! Distributed load-testing type definitions (aligned with the gRPC proto, plus REST view structs for the frontend)

use orbit_config::TestPlan;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Agent state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    /// Idle
    Idle,
    /// Running (a task is executing)
    Running,
    /// Paused (no longer accepts new tasks)
    Paused,
    /// Offline (heartbeat timeout / connection lost)
    Offline,
}

impl AgentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentState::Idle => "idle",
            AgentState::Running => "running",
            AgentState::Paused => "paused",
            AgentState::Offline => "offline",
        }
    }
}

impl From<AgentState> for i32 {
    fn from(s: AgentState) -> i32 {
        match s {
            AgentState::Idle => 0,
            AgentState::Running => 1,
            AgentState::Paused => 2,
            AgentState::Offline => 3,
        }
    }
}

/// System resource snapshot (collected via sysinfo)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ResourceSnapshot {
    pub cpu_percent: f64,
    pub mem_used_mb: f64,
    pub mem_total_mb: f64,
    pub mem_percent: f64,
    pub load_avg_1m: f64,
    pub timestamp_ms: i64,
}

/// Frontend-facing agent view (registration info + live state + resources)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentInfo {
    pub id: String,
    /// "server" | "client"
    pub mode: String,
    /// Listening address in server mode; empty in client mode
    pub addr: String,
    pub version: String,
    pub cpu_cores: u32,
    pub memory_mb: u64,
    pub labels: Vec<(String, String)>,
    pub state: AgentState,
    pub degraded: bool,
    /// Whether another controller has taken over (this controller's connection is invalid)
    pub taken_over: bool,
    pub last_heartbeat_ms: i64,
    pub registered_at_ms: i64,
    /// Current task ID (while running)
    pub task_id: Option<String>,
    pub resource: Option<ResourceSnapshot>,
    /// Recent resource sample history (for trend charts, at most 60 points)
    pub resource_history: Vec<ResourceSnapshot>,
}

/// Register-agent request (controller → agent, server mode)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddAgentRequest {
    pub addr: String,
    pub agent_id: Option<String>,
    pub labels: Option<Vec<(String, String)>>,
    /// Whether to force takeover when already held by another controller
    #[serde(default)]
    pub force: bool,
}

/// Single-request execution request (frontend / REST → controller → agent)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteRequestData {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default)]
    pub request_format: Option<String>,
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub prereq_script: Option<String>,
    #[serde(default)]
    pub postreq_script: Option<String>,
    #[serde(default)]
    pub env_vars: HashMap<String, String>,
}

fn default_protocol() -> String {
    "http".into()
}

/// Single-request execution result (agent → controller → frontend)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecuteResult {
    pub status: i32,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub duration_ms: i64,
    pub error: Option<String>,
    pub timing: Option<ExecuteTimings>,
    pub pre_logs: Vec<ScriptLogEntry>,
    pub post_logs: Vec<ScriptLogEntry>,
    pub decoded: Option<String>,
    pub post_tests: Vec<TestResultEntry>,
}

/// Script log entry (aligned with orbit-js ScriptLog)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScriptLogEntry {
    pub level: String,
    pub message: String,
}

/// Script assertion result (pm.test)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TestResultEntry {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

/// Per-phase timings (milliseconds, same as local execution)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecuteTimings {
    pub dns_ms: f64,
    pub tcp_ms: f64,
    pub tls_ms: f64,
    pub ttfb_ms: f64,
    pub download_ms: f64,
    pub total_ms: f64,
}

/// Load partition - distributes a TestPlan across agents by weight
#[derive(Debug, Clone)]
pub struct PlanPartition {
    pub agent_id: String,
    pub plan: TestPlan,
    pub vu_fraction: f64,
}

/// Metrics snapshot reported by an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetricsSnapshot {
    pub agent_id: String,
    pub timestamp_ms: u64,
    pub active_vus: u32,
    pub total_requests: u64,
    pub total_errors: u64,
    /// HDR histogram encoding (base64)
    pub hdr_histogram_b64: String,
    /// Fallback summary (used when HDR is unavailable)
    pub summary: Option<MetricsSummary>,
}

/// Fallback summary statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub mean_ms: f64,
    pub rps: f64,
    pub error_rate: f64,
}

/// Distributed load-test result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedResult {
    pub total_requests: u64,
    pub total_errors: u64,
    pub error_rate: f64,
    pub rps: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    pub mean_ms: f64,
    pub agent_count: usize,
    pub agent_stats: Vec<AgentMetricsSnapshot>,
}

/// Distributed error
#[derive(Debug, thiserror::Error)]
pub enum DistributedError {
    #[error("Agent connection failed: {0}")]
    Connection(String),
    #[error("Agent error: {0}")]
    Agent(String),
    #[error("Merge error: {0}")]
    Merge(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Already registered: {0}")]
    AlreadyRegistered(String),
}
