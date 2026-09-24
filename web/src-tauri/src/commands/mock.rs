use orbit_server::mock::{MockInterface, MockServer};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::state::AppState;

fn get_handle(state: &AppState) -> &Arc<RwLock<crate::state::MockServerHandle>> {
    &state.mock_server
}

/// Fill in ids for expectations missing them (keeps frontend keys stable).
fn ensure_expectation_ids(iface: &mut MockInterface) {
    for exp in &mut iface.expectations {
        if exp.id.is_empty() {
            exp.id = format!("exp-{}", Uuid::new_v4().simple());
        }
    }
}

#[tauri::command]
pub async fn start_mock_server(
    port: u16,
    workspace_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let mut handle = get_handle(&state).write().await;
    if handle.running {
        return Err("Mock server is already running".into());
    }

    // Load only enabled rules for the current workspace (Mock is isolated per workspace)
    let live: Vec<MockInterface> = handle
        .rules
        .read()
        .await
        .iter()
        .filter(|r| {
            r.enabled
                && r.ws()
                    == workspace_id
                        .as_deref()
                        .unwrap_or(orbit_data::DEFAULT_WORKSPACE_ID)
        })
        .cloned()
        .collect();
    let live_arc = Arc::new(RwLock::new(live));
    let server = MockServer::with_rules(port, live_arc.clone());

    let task = tokio::spawn(async move {
        let _ = server.start().await;
    });

    handle.running = true;
    handle.port = port;
    handle.abort_handle = Some(task);
    handle.live = live_arc;
    Ok(true)
}

#[tauri::command]
pub async fn stop_mock_server(state: State<'_, AppState>) -> Result<bool, String> {
    let mut handle = get_handle(&state).write().await;
    if !handle.running {
        return Err("Mock server is not running".into());
    }
    if let Some(task) = handle.abort_handle.take() {
        task.abort();
    }
    handle.running = false;
    handle.port = 0;
    Ok(true)
}

#[tauri::command]
pub async fn get_mock_rules(
    workspace_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<MockInterface>, String> {
    let handle = get_handle(&state).read().await;
    let rules = handle.rules.read().await.clone();
    // Passing workspace_id = only that workspace (UI); omitted = all (snapshot persistence needs the full set)
    Ok(match workspace_id {
        Some(ws) => rules.into_iter().filter(|r| r.ws() == ws).collect(),
        None => rules,
    })
}

#[tauri::command]
pub async fn save_mock_interface(
    mut interface: MockInterface,
    state: State<'_, AppState>,
) -> Result<MockInterface, String> {
    let handle = get_handle(&state).write().await;
    ensure_expectation_ids(&mut interface);
    {
        let mut rules = handle.rules.write().await;
        // upsert: prefer matching by request_id (data ownership key, globally unique) for update;
        // legacy data (no request_id) is matched by method+path within the same workspace
        let pos = rules.iter().position(|r| {
            if r.ws() != interface.ws() {
                return false;
            }
            match (&r.request_id, &interface.request_id) {
                (Some(a), Some(b)) => a == b,
                _ => r.method.eq_ignore_ascii_case(&interface.method) && r.path == interface.path,
            }
        });
        if let Some(pos) = pos {
            rules[pos] = interface.clone();
        } else {
            rules.push(interface.clone());
        }
    }
    let live: Vec<MockInterface> = handle
        .rules
        .read()
        .await
        .iter()
        .filter(|r| r.enabled)
        .cloned()
        .collect();
    *handle.live.write().await = live;
    Ok(interface)
}

#[tauri::command]
pub async fn delete_mock_interface(
    method: String,
    path: String,
    workspace_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let ws = workspace_id
        .as_deref()
        .unwrap_or(orbit_data::DEFAULT_WORKSPACE_ID);
    let handle = get_handle(&state).write().await;
    handle
        .rules
        .write()
        .await
        .retain(|r| !(r.ws() == ws && r.method.eq_ignore_ascii_case(&method) && r.path == path));
    let live: Vec<MockInterface> = handle
        .rules
        .read()
        .await
        .iter()
        .filter(|r| r.enabled)
        .cloned()
        .collect();
    *handle.live.write().await = live;
    Ok(true)
}
