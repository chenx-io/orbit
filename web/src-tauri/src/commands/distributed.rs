//! Distributed load-test commands: agent list / add / pause / resume / remove / task dispatch

use serde::Deserialize;
use tauri::State;

use orbit_distributed::types::AddAgentRequest;
use orbit_distributed::types::ExecuteRequestData;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchRequest {
    pub yaml: String,
    pub task_id: Option<String>,
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

#[tauri::command]
pub async fn distributed_agents(
    state: State<'_, orbit_distributed::Controller>,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(state.agents()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn distributed_add_agent(
    state: State<'_, orbit_distributed::Controller>,
    request: AddAgentRequest,
) -> Result<serde_json::Value, String> {
    let info = state
        .add_server_agent(
            request.addr,
            request.agent_id,
            request.labels.unwrap_or_default(),
            request.force,
        )
        .await
        .map_err(|e| e.to_string())?;
    if info.claimed {
        return Ok(serde_json::json!({
            "error": "this agent is already in use by another controller",
            "claimed": true,
            "agent_id": info.agent_id,
        }));
    }
    serde_json::to_value(info.agent).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn distributed_agent_action(
    state: State<'_, orbit_distributed::Controller>,
    id: String,
    action: String,
) -> Result<(), String> {
    match action.as_str() {
        "pause" => state.pause_agent(&id).map_err(|e| e.to_string()),
        "resume" => state.resume_agent(&id).map_err(|e| e.to_string()),
        "remove" => state.remove_agent(&id).map_err(|e| e.to_string()),
        "ping" => {
            let addr = state
                .agents()
                .into_iter()
                .find(|a| a.id == id)
                .map(|a| a.addr)
                .ok_or_else(|| "agent not found".to_string())?;
            if addr.is_empty() {
                return Err("client-mode agents do not support active ping (they use the controller long connection)".into());
            }
            state.ping_agent(addr).await.map_err(|e| e.to_string())
        }
        other => Err(format!("unknown action: {other}")),
    }
}

#[tauri::command]
pub async fn distributed_run(
    state: State<'_, orbit_distributed::Controller>,
    request: DispatchRequest,
) -> Result<serde_json::Value, String> {
    let plan = orbit_config::from_str(&request.yaml).map_err(|e| e.to_string())?;
    let task_id = request
        .task_id
        .unwrap_or_else(|| format!("task-{}", uuid_simple()));
    let n = state
        .dispatch_task_to(&plan, task_id.clone(), request.agent_ids.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true, "task_id": task_id, "agents": n }))
}

#[tauri::command]
pub async fn distributed_stop(
    state: State<'_, orbit_distributed::Controller>,
    request: StopRequest,
) -> Result<serde_json::Value, String> {
    let n = state
        .stop_task(&request.task_id)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "ok": true, "stopped": n }))
}

#[tauri::command]
pub async fn distributed_result(
    state: State<'_, orbit_distributed::Controller>,
) -> Result<serde_json::Value, String> {
    let result = state.last_result();
    if result.total_requests == 0 && result.agent_count == 0 {
        return Err("no distributed load-test result yet".into());
    }
    serde_json::to_value(result).map_err(|e| e.to_string())
}

/// Live task progress (Tauri frontend polling, replacing the `distributed-event` event push):
/// Returns each agent's latest metrics snapshot and the list of finished (finished/failed) agents.
#[tauri::command]
pub async fn distributed_task_progress(
    state: State<'_, orbit_distributed::Controller>,
    task_id: String,
) -> Result<serde_json::Value, String> {
    let (snapshots, done_agents) = state.task_snapshots(&task_id);
    Ok(serde_json::json!({
        "snapshots": snapshots,
        "done_agents": done_agents,
    }))
}

#[tauri::command]
pub async fn distributed_execute(
    state: State<'_, orbit_distributed::Controller>,
    id: String,
    request: ExecuteRequestData,
) -> Result<serde_json::Value, String> {
    let result = state
        .execute_on_agent(&id, request)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn distributed_controller_status(
    state: State<'_, orbit_distributed::Controller>,
) -> Result<serde_json::Value, String> {
    match state.controller_status() {
        Some(addr) => Ok(serde_json::json!({ "running": true, "addr": addr })),
        None => Ok(serde_json::json!({ "running": false, "addr": null })),
    }
}

#[tauri::command]
pub async fn distributed_controller_start(
    state: State<'_, orbit_distributed::Controller>,
    request: ControllerStartRequest,
) -> Result<serde_json::Value, String> {
    let addr = state
        .start_controller_server(request.port)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "running": true, "addr": addr }))
}

#[tauri::command]
pub async fn distributed_controller_stop(
    state: State<'_, orbit_distributed::Controller>,
) -> Result<serde_json::Value, String> {
    state.stop_controller_server().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "running": false }))
}

fn uuid_simple() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
