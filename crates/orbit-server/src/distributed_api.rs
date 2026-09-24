//! Distributed management API: agent list / add (controller connects to agent) / pause / resume / remove / task dispatch

use std::convert::Infallible;

use axum::extract::{Path, State};
use axum::response::sse::{Event, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_stream::StreamExt;

use orbit_distributed::types::{AddAgentRequest, ExecuteRequestData};

use crate::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchRequest {
    pub yaml: String,
    #[serde(default)]
    pub task_id: Option<String>,
    /// Agent list to dispatch to (default = dispatch to all available agents by resource weight)
    #[serde(default)]
    pub agent_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopRequest {
    pub task_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ControllerStartRequest {
    pub port: u16,
}

pub fn distributed_routes() -> Router<AppState> {
    Router::new()
        .route("/api/distributed/agents", get(list_agents))
        .route("/api/distributed/agent", post(add_agent))
        .route("/api/distributed/agent/{id}/pause", post(pause_agent))
        .route("/api/distributed/agent/{id}/resume", post(resume_agent))
        .route("/api/distributed/agent/{id}/remove", post(remove_agent))
        .route("/api/distributed/agent/{id}/ping", post(ping_agent))
        .route("/api/distributed/agent/{id}/execute", post(execute_agent))
        .route("/api/distributed/controller", get(controller_status))
        .route("/api/distributed/controller/start", post(controller_start))
        .route("/api/distributed/controller/stop", post(controller_stop))
        .route("/api/distributed/events", get(distributed_events))
        .route("/api/distributed/run", post(dispatch_run))
        .route("/api/distributed/stop", post(dispatch_stop))
        .route("/api/distributed/result", get(distributed_result))
}

async fn list_agents(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::to_value(state.distributed.agents()).unwrap_or_default())
}

async fn add_agent(
    State(state): State<AppState>,
    Json(req): Json<AddAgentRequest>,
) -> (axum::http::StatusCode, Json<Value>) {
    match state
        .distributed
        .add_server_agent(
            req.addr.clone(),
            req.agent_id.clone(),
            req.labels.unwrap_or_default(),
            req.force,
        )
        .await
    {
        Ok(outcome) => {
            if outcome.claimed {
                return (
                    axum::http::StatusCode::CONFLICT,
                    Json(json!({
                        "error": "this agent is already in use by another controller",
                        "claimed": true,
                        "agent_id": outcome.agent_id,
                    })),
                );
            }
            (
                axum::http::StatusCode::OK,
                Json(serde_json::to_value(outcome.agent).unwrap_or_default()),
            )
        }
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn pause_agent(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    match state.distributed.pause_agent(&id) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn resume_agent(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    match state.distributed.resume_agent(&id) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn remove_agent(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    match state.distributed.remove_agent(&id) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn ping_agent(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    let addr = match state.distributed.agents().into_iter().find(|a| a.id == id) {
        Some(a) if !a.addr.is_empty() => a.addr,
        Some(_) => {
            return Json(
                json!({ "ok": false, "error": "client-mode agents do not support active ping (via the controller long connection)" }),
            )
        }
        None => return Json(json!({ "ok": false, "error": "agent not found" })),
    };
    match state.distributed.ping_agent(addr).await {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn execute_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteRequestData>,
) -> (axum::http::StatusCode, Json<Value>) {
    match state.distributed.execute_on_agent(&id, req).await {
        Ok(r) => (
            axum::http::StatusCode::OK,
            Json(serde_json::to_value(r).unwrap_or_default()),
        ),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn controller_status(State(state): State<AppState>) -> Json<Value> {
    match state.distributed.controller_status() {
        Some(addr) => Json(json!({ "running": true, "addr": addr })),
        None => Json(json!({ "running": false, "addr": null })),
    }
}

async fn controller_start(
    State(state): State<AppState>,
    Json(req): Json<ControllerStartRequest>,
) -> (axum::http::StatusCode, Json<Value>) {
    match state.distributed.start_controller_server(req.port) {
        Ok(addr) => (
            axum::http::StatusCode::OK,
            Json(json!({ "ok": true, "running": true, "addr": addr })),
        ),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn controller_stop(State(state): State<AppState>) -> Json<Value> {
    match state.distributed.stop_controller_server() {
        Ok(()) => Json(json!({ "ok": true, "running": false })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn dispatch_run(
    State(state): State<AppState>,
    Json(req): Json<DispatchRequest>,
) -> (axum::http::StatusCode, Json<Value>) {
    let plan = match orbit_config::from_str(&req.yaml) {
        Ok(p) => p,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
        }
    };
    let task_id = req
        .task_id
        .unwrap_or_else(|| format!("task-{}", uuid_simple()));
    match state
        .distributed
        .dispatch_task_to(&plan, task_id.clone(), req.agent_ids.as_deref())
        .await
    {
        Ok(n) => (
            axum::http::StatusCode::OK,
            Json(json!({ "ok": true, "task_id": task_id, "agents": n })),
        ),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn dispatch_stop(
    State(state): State<AppState>,
    Json(req): Json<StopRequest>,
) -> (axum::http::StatusCode, Json<Value>) {
    match state.distributed.stop_task(&req.task_id) {
        Ok(n) => (
            axum::http::StatusCode::OK,
            Json(json!({ "ok": true, "stopped": n })),
        ),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn distributed_result(State(state): State<AppState>) -> Json<Value> {
    let result = state.distributed.last_result();
    if result.total_requests == 0 && result.agent_count == 0 {
        return Json(json!({ "error": "no distributed load test result yet" }));
    }
    Json(serde_json::to_value(result).unwrap_or_default())
}

async fn distributed_events(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.distributed.subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx).map(|item| match item {
        Ok(ev) => Ok::<_, Infallible>(
            Event::default().data(serde_json::to_string(&ev).unwrap_or_default()),
        ),
        Err(_) => Ok::<_, Infallible>(Event::default().data("{}")),
    });
    Sse::new(stream)
}

fn uuid_simple() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
