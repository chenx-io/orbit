//! Data source commands (desktop): registry + snapshot persistence + test connection + trial query.
//!
//! Kept isomorphic with the HTTP endpoints in `crates/orbit-server/src/datasource_api.rs`,
//! ensuring the browser preview mode and desktop have consistent capabilities.

use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::State;

use crate::state::AppState;
use orbit_config::DataSourceConfig;
use orbit_datasource::DataSourceRegistry;

/// List all data sources (passwords masked).
#[tauri::command]
pub async fn ds_list(state: State<'_, AppState>) -> Result<Value, String> {
    let list: Vec<DataSourceConfig> = state
        .data_sources
        .list()
        .await
        .into_iter()
        .map(|c| c.masked())
        .collect();
    serde_json::to_value(list).map_err(|e| format!("serialization failed: {e}"))
}

/// Add/update a data source: register in the runtime registry and persist to the snapshot.
#[tauri::command]
pub async fn ds_upsert(state: State<'_, AppState>, cfg: DataSourceConfig) -> Result<(), String> {
    state.data_sources.register(cfg.clone()).await;
    state
        .data
        .upsert_data_source(cfg)
        .await
        .map_err(|e| e.to_string())
}

/// Remove a data source and release its connections.
#[tauri::command]
pub async fn ds_remove(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.data_sources.remove(&id).await;
    state
        .data
        .remove_data_source(&id)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestReq {
    pub id: Option<String>,
    pub config: Option<DataSourceConfig>,
}

/// Test connection: returns elapsed time, server info, or the failure reason.
#[tauri::command]
pub async fn ds_test(state: State<'_, AppState>, req: TestReq) -> Result<Value, String> {
    let (id, existed) = resolve_target(state.data_sources.clone(), req.id, req.config).await;
    let report = state.data_sources.test(&id).await;
    cleanup_temp(state.data_sources.clone(), &id, existed).await;
    serde_json::to_value(report).map_err(|e| format!("serialization failed: {e}"))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryReq {
    pub id: Option<String>,
    pub config: Option<DataSourceConfig>,
    pub sql: Option<String>,
    pub redis: Option<Vec<String>>,
}

/// Trial run: execute a read-only SQL / Redis command and return the result (for the "Assertions" tab preview).
#[tauri::command]
pub async fn ds_preview_query(state: State<'_, AppState>, req: QueryReq) -> Result<Value, String> {
    let (id, existed) = resolve_target(state.data_sources.clone(), req.id, req.config).await;
    let result = if let Some(sql) = &req.sql {
        match state.data_sources.query_sql(&id, sql).await {
            Ok(r) => json!({
                "columns": r.columns,
                "rows": r.rows,
                "rowsAffected": r.rows_affected,
                "elapsedMs": r.elapsed_ms,
            }),
            Err(e) => json!({ "error": e.message() }),
        }
    } else if let Some(args) = &req.redis {
        match state.data_sources.redis_cmd(&id, args).await {
            Ok(v) => json!({ "value": v }),
            Err(e) => json!({ "error": e.message() }),
        }
    } else {
        json!({ "error": "missing sql or redis command" })
    };
    cleanup_temp(state.data_sources.clone(), &id, existed).await;
    Ok(result)
}

/// Resolve the target: prefer an existing id; otherwise temporarily register the config to test/preview.
async fn resolve_target(
    registry: Arc<DataSourceRegistry>,
    id: Option<String>,
    config: Option<DataSourceConfig>,
) -> (String, bool) {
    if let Some(cfg) = config {
        let existed = registry.config(&cfg.id).await.is_some();
        registry.register(cfg.clone()).await;
        return (cfg.id, existed);
    }
    (id.unwrap_or_default(), true)
}

/// The temporarily registered config is removed after the operation.
async fn cleanup_temp(registry: Arc<DataSourceRegistry>, id: &str, existed: bool) {
    if !existed {
        registry.remove(id).await;
    }
}
