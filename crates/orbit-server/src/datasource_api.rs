//! Data source HTTP API (browser preview mode; the desktop app provides equivalent capability via Tauri commands).
//!
//! Uniformly read/write data source config in the snapshot and sync it to the runtime registry; assertion execution (`/api/proxy`)
//! consumes the connection pool in the registry directly. All queries are subject to `readonly` protection by default.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::AppState;
use orbit_config::DataSourceConfig;

/// Registry routes (mounted at /api/datasources/*).
pub fn datasource_routes() -> Router<AppState> {
    Router::new()
        .route("/api/datasources", get(list_handler))
        .route("/api/datasources/upsert", post(upsert_handler))
        .route("/api/datasources/remove", post(remove_handler))
        .route("/api/datasources/test", post(test_handler))
        .route("/api/datasources/query", post(query_handler))
}

/// Sync the registry from snapshot data (called after save/load): register additions and remove deletions.
pub async fn sync_registry_from_data(state: &AppState) {
    let cfgs = state.data.data_sources();
    state.data_sources.register_all(&cfgs).await;
    let registered: Vec<String> = state
        .data_sources
        .list()
        .await
        .into_iter()
        .map(|c| c.id)
        .collect();
    for id in registered {
        if !cfgs.iter().any(|c| c.id == id) {
            state.data_sources.remove(&id).await;
        }
    }
}

async fn list_handler(State(state): State<AppState>) -> Json<Value> {
    let list: Vec<DataSourceConfig> = state
        .data_sources
        .list()
        .await
        .into_iter()
        .map(|c| c.masked())
        .collect();
    Json(json!(list))
}

async fn upsert_handler(
    State(state): State<AppState>,
    Json(cfg): Json<DataSourceConfig>,
) -> Json<Value> {
    state.data_sources.register(cfg.clone()).await;
    if let Err(e) = state.data.upsert_data_source(cfg).await {
        return Json(json!({ "error": e.to_string() }));
    }
    Json(json!({ "ok": true }))
}

#[derive(Debug, Deserialize)]
struct RemoveReq {
    id: String,
}

async fn remove_handler(State(state): State<AppState>, Json(req): Json<RemoveReq>) -> Json<Value> {
    state.data_sources.remove(&req.id).await;
    if let Err(e) = state.data.remove_data_source(&req.id).await {
        return Json(json!({ "error": e.to_string() }));
    }
    Json(json!({ "ok": true }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestReq {
    /// Test a saved data source (by id)
    id: Option<String>,
    /// Directly test an unsaved config (temporarily registered, removed after the test)
    config: Option<DataSourceConfig>,
}

async fn test_handler(State(state): State<AppState>, Json(req): Json<TestReq>) -> Json<Value> {
    let (id, existed) = match resolve_target(&state, req.id, req.config).await {
        Ok(x) => x,
        Err(e) => return Json(json!({ "error": e })),
    };
    let report = state.data_sources.test(&id).await;
    cleanup_temp(&state, &id, existed).await;
    Json(json!(report))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryReq {
    /// Saved data source id (one of id or config)
    id: Option<String>,
    /// Directly preview an unsaved config
    config: Option<DataSourceConfig>,
    /// SQL query (when the data source is a relational database)
    sql: Option<String>,
    /// Redis command (command name + args), used when the data source is Redis
    redis: Option<Vec<String>>,
}

/// Dry-run a data source / preview query results.
async fn query_handler(State(state): State<AppState>, Json(req): Json<QueryReq>) -> Json<Value> {
    let (id, existed) = match resolve_target(&state, req.id, req.config).await {
        Ok(x) => x,
        Err(e) => return Json(json!({ "error": e })),
    };
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
    cleanup_temp(&state, &id, existed).await;
    Json(result)
}

/// Resolve the target: prefer an existing id, otherwise temporarily register the config to test.
async fn resolve_target(
    state: &AppState,
    id: Option<String>,
    config: Option<DataSourceConfig>,
) -> Result<(String, bool), String> {
    if let Some(cfg) = config {
        let existed = state.data_sources.config(&cfg.id).await.is_some();
        state.data_sources.register(cfg.clone()).await;
        return Ok((cfg.id, existed));
    }
    match id {
        Some(id) if !id.is_empty() => Ok((id, true)),
        _ => Err("missing data source id or config".into()),
    }
}

/// Temporarily registered configs are removed after the operation (existing ones are left untouched).
async fn cleanup_temp(state: &AppState, id: &str, existed: bool) {
    if !existed {
        state.data_sources.remove(id).await;
    }
}
