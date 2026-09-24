//! Real backend HTTP API for the web frontend.
//!
//! These endpoints behave the same as the Tauri commands in `web/src-tauri/src/commands/*`,
//! but are exposed as a standalone HTTP service so the browser preview (non-Tauri) can also call the real Rust backend.
//! All requests are actually executed; there is no mock/fake data anymore.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, Sse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;

use base64::prelude::*;
use tokio_stream::StreamExt;

use crate::session::OpenSessionRequest;
use crate::{mock::MockInterface, AppState};

// ────────────────────────────────────────────────────────────
// Proxy execution (execute_request) — returns real per-phase timings
// ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ProxyRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: String,
    /// Base64 encoding of the binary / multipart request body; takes precedence over `body`
    pub body_binary: Option<String>,
    pub timeout: Option<u64>,
    /// Pre-request script (runs after variable resolution, before sending); may rewrite url/method/headers/body
    pub prereq_script: Option<String>,
    /// Post-response script (runs after receiving the response); can read the response, write variables and collect assertions
    pub postreq_script: Option<String>,
    /// **Legacy input field (deprecated)**: the "pre-interpolation actions" list from the previous two-stage refactor.
    ///
    /// They are merged in ahead of the built-in interpolation node of `pre_actions` (see `merge_pre_actions`).
    #[serde(default)]
    pub pre_resolve_actions: Option<Vec<orbit_config::RequestAction>>,
    /// Pre-request action list (scripts / read-only database queries / built-in interpolation node; the order is the execution order);
    /// takes precedence over `prereq_script` when non-empty
    #[serde(default)]
    pub pre_actions: Option<Vec<orbit_config::RequestAction>>,
    /// Post-response action list (executed in order after receiving the response); takes precedence over `postreq_script` when non-empty
    #[serde(default)]
    pub post_actions: Option<Vec<orbit_config::RequestAction>>,
    /// Script library table (reusable action templates of the current workspace): used by the engine to expand `{ type: ref }` references.
    ///
    /// The browser preview path has no workspace snapshot, so it is sent by the frontend with the request, just like the action lists.
    #[serde(default)]
    pub action_templates: Option<Vec<orbit_config::ActionTemplate>>,
    /// Snapshot of the current environment variables, read by scripts via pm.environment.get
    pub env_vars: Option<HashMap<String, String>>,
    /// Assertion config (built-in + DB/Redis), evaluated after the response; on the single-send path the frontend does not interpolate and the backend interpolates uniformly before execution
    #[serde(default)]
    pub checks: Option<Vec<orbit_config::Check>>,
    /// Un-interpolated request template (uniform interpolation for single HTTP sends).
    ///
    /// When provided, the engine takes over URL joining, header merging and body assembly, and the above
    /// `url` / `headers` / `body*` fields are ignored; `None` keeps the old semantics.
    #[serde(default)]
    pub request_template: Option<orbit_engine::request_build::RequestTemplate>,
}

#[derive(Debug, Serialize)]
pub struct TimingInfo {
    pub dns: u64,
    pub connect: u64,
    pub tls: u64,
    pub ttfb: u64,
    pub download: u64,
}

fn detect_protocol(url: &str) -> &'static str {
    if url.starts_with("ws://") || url.starts_with("wss://") {
        "websocket"
    } else if url.starts_with("tcp://") {
        "tcp"
    } else if url.starts_with("udp://") {
        "udp"
    } else if url.starts_with("sse://") {
        "sse"
    } else {
        "http"
    }
}

fn is_graphql(method: &str, body: &str) -> bool {
    method.eq_ignore_ascii_case("GRAPHQL")
        || body.trim_start().starts_with("query")
        || body.trim_start().starts_with("mutation")
        || body.trim_start().starts_with("subscription")
}

fn http_status_text(code: u16) -> String {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "",
    }
    .to_string()
}

pub async fn proxy_handler(
    State(state): State<AppState>,
    Json(req): Json<ProxyRequest>,
) -> Json<Value> {
    let timeout = req.timeout.unwrap_or(30_000);
    let env_vars = req.env_vars.clone().unwrap_or_default();

    let request_id = uuid::Uuid::new_v4().simple().to_string();
    crate::emit_event(
        &state,
        crate::events::OrbitEvent::RequestStarted {
            request_id: request_id.clone(),
            method: req.method.clone(),
            url: req.url.clone(),
            user: None,
        },
    );

    // Unified execution pipeline (same origin as the load-test flow_runner: pre-interpolation actions → interpolation → encoding → post-interpolation actions → send → decode → post script)
    //
    // Template path (`request_template` present): the request is not interpolated/assembled, the engine takes over building it;
    // Passthrough path: the caller has already built the request (interpolate=false), so only scripts run.
    let template = req.request_template.clone();
    let body_for_probe = template
        .as_ref()
        .and_then(|t| t.text_body())
        .unwrap_or(req.body.as_str());
    let protocol = if is_graphql(&req.method, body_for_probe) {
        "graphql"
    } else {
        detect_protocol(
            template
                .as_ref()
                .map(|t| t.url.as_str())
                .unwrap_or(&req.url),
        )
    };
    let (target, headers, body) = match &template {
        Some(t) => (t.url.clone(), HashMap::new(), Vec::new()),
        None => {
            // Binary / multipart request body: prefer the base64-decoded bytes; otherwise use the text body
            let body: Vec<u8> = match &req.body_binary {
                Some(b) => BASE64_STANDARD
                    .decode(b)
                    .ok()
                    .unwrap_or_else(|| req.body.as_bytes().to_vec()),
                None => req.body.as_bytes().to_vec(),
            };
            (req.url.clone(), req.headers.clone(), body)
        }
    };
    // Script library table: the engine uses it to expand `{ type: ref }` when mapping actions; a missing library item degrades to an error log
    let library = req.action_templates.as_deref().unwrap_or_default();
    let spec = orbit_engine::pipeline::PipelineSpec {
        protocol: protocol.to_string(),
        target,
        operation: req.method.clone(),
        headers,
        body,
        timeout: Some(std::time::Duration::from_millis(timeout)),
        pre_scripts: req
            .prereq_script
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .into_iter()
            .collect(),
        post_scripts: req
            .postreq_script
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .into_iter()
            .collect(),
        // Pre actions: a single list (including the built-in interpolation node); the previous version's "pre-interpolation actions" are merged in before the anchor
        pre_actions: orbit_engine::pipeline::actions_to_pipeline(
            &orbit_config::merge_pre_actions(
                req.pre_actions.as_deref().unwrap_or_default(),
                None,
                req.pre_resolve_actions.as_deref().unwrap_or_default(),
                None,
            ),
            library,
        ),
        post_actions: orbit_engine::pipeline::actions_to_pipeline(
            req.post_actions.as_deref().unwrap_or_default(),
            library,
        ),
        checks: req.checks.clone().unwrap_or_default(),
        // With a template the request is always un-interpolated (template convention), so the engine interpolates
        interpolate: template.is_some(),
        request_template: template,
        ..Default::default()
    };

    let mut rt = orbit_engine::pipeline::PipelineRuntime::new(
        Box::new(orbit_protocol::http::HttpClient::new()),
        Box::new(orbit_codec::json::JsonCodec),
    );
    // Inject the datasource registry: post-request DB/Redis assertions query through it (not injected when unconfigured, so the related assertions fail explicitly)
    rt.with_datasources(Some(state.data_sources.clone()));
    let mut vars: HashMap<String, String> = HashMap::new();
    let cancel = tokio_util::sync::CancellationToken::new();
    // Cookie Jar shared by single-send debugging: Set-Cookie from responses accumulates automatically and is attached to later requests to the same domain (session persistence)
    let mut cookie_jar = state.cookie_jar.lock().await;
    let outcome = orbit_engine::pipeline::execute_pipeline(
        &mut rt,
        spec,
        &mut vars,
        &env_vars,
        &cancel,
        Some(&mut cookie_jar),
    )
    .await;

    let Some(response) = &outcome.response else {
        let msg = outcome
            .error
            .as_ref()
            .map(|e| e.message())
            .unwrap_or_else(|| "request failed".into());
        crate::emit_event(
            &state,
            crate::events::OrbitEvent::RequestFailed {
                request_id,
                error: msg.clone(),
                user: None,
            },
        );
        return Json(json!({ "error": format!("request execution failed: {}", msg) }));
    };

    let status = response.status_code;
    let body_str = String::from_utf8_lossy(&response.payload).to_string();
    let duration = response.duration_ms;
    let size = body_str.len() as u64;
    let headers = response.headers.clone();
    let t = &response.timings;
    let timing = TimingInfo {
        dns: t.dns.map(|d| d.as_millis() as u64).unwrap_or(0),
        connect: t.tcp.map(|d| d.as_millis() as u64).unwrap_or(0),
        tls: t.tls.map(|d| d.as_millis() as u64).unwrap_or(0),
        ttfb: t.first_byte.map(|d| d.as_millis() as u64).unwrap_or(0),
        download: t.receive.map(|d| d.as_millis() as u64).unwrap_or(0),
    };

    crate::emit_event(
        &state,
        crate::events::OrbitEvent::RequestCompleted {
            request_id,
            status,
            duration_ms: duration,
            user: None,
        },
    );

    Json(json!({
        "status": status,
        "statusText": http_status_text(status),
        "headers": headers,
        "body": body_str,
        "duration": duration,
        "size": size,
        "timing": timing,
        // Request snapshot after pre-script rewriting (method/url/headers/body), for the frontend to merge, display and generate code from
        "request": {
            "method": outcome.request.operation,
            "url": outcome.request.target,
            "headers": outcome.request.headers,
            "body": String::from_utf8_lossy(&outcome.request.payload),
        },
        "preLogs": outcome.pre_logs,
        "postLogs": outcome.post_logs,
        "postTests": outcome.post_tests,
        // Execution results of the pre/post actions (scripts / database); the frontend renders status and written variables in action order
        "preActions": outcome.pre_action_logs,
        "postActions": outcome.post_action_logs,
        // Variables written by actions (DB query results), returned together with varsSet
        "actionVars": outcome.action_vars,
        // Assertion results (built-in + DB/Redis), rendered uniformly in the frontend Tests tab
        "assertions": outcome.tests.iter().map(|t| json!({
            "name": t.name,
            "passed": t.passed,
            "message": t.message,
            "isHard": t.is_hard,
            "exportedVars": t.exported_vars,
        })).collect::<Vec<_>>(),
        "varsSet": outcome.vars_set,
        "tempVarsSet": outcome.temp_vars_set,
    }))
}

// ────────────────────────────────────────────────────────────
// Dynamic values (generate_dynamic_value)
// ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DynamicRequest {
    pub category: String,
    pub method: String,
    pub args: Option<String>,
}

pub async fn dynamic_handler(Json(req): Json<DynamicRequest>) -> Json<Value> {
    match orbit_dynamic::generate(
        &req.category,
        &req.method,
        req.args.as_deref().unwrap_or(""),
    ) {
        Ok(s) => Json(json!(s)),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

#[derive(Debug, Deserialize)]
pub struct DynamicResolveRequest {
    pub input: String,
}

pub async fn dynamic_resolve_handler(Json(req): Json<DynamicResolveRequest>) -> Json<Value> {
    match orbit_dynamic::resolve(&req.input) {
        Ok(s) => Json(json!(s)),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

// ────────────────────────────────────────────────────────────
// Import (cURL / Postman / OpenAPI…)
// ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ImportParseRequest {
    pub format: String,
    pub input: String,
}

// ─── Scenario import endpoints (return the full YAML) ────────────────────

pub async fn import_scenario_handler(Json(payload): Json<Value>) -> Json<Value> {
    let format = payload
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("curl");
    let input = payload.get("input").and_then(|v| v.as_str()).unwrap_or("");
    match orbit_config::exchange::import_scenario(format, input) {
        Ok(plan) => match serde_yaml::to_string(&plan) {
            Ok(yaml) => Json(json!({ "yaml": yaml })),
            Err(e) => Json(json!({ "error": format!("serialization failed: {}", e) })),
        },
        Err(e) => Json(json!({ "error": format!("import failed: {}", e) })),
    }
}

// ────────────────────────────────────────────────────────────
// Unified import-parse endpoint (preserves groups and models)
// ────────────────────────────────────────────────────────────

pub async fn import_parse_handler(Json(payload): Json<ImportParseRequest>) -> Json<Value> {
    match orbit_config::exchange::import_endpoints(&payload.format, &payload.input) {
        Ok(result) => Json(json!(result)),
        Err(e) => Json(json!({ "error": format!("import parsing failed: {}", e) })),
    }
}

// ────────────────────────────────────────────────────────────
// gRPC collections (proto import / reflection import / message templates)
// ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GrpcImportProtoRequest {
    /// List of `{ "name": "file.proto", "content": "syntax=..." }`, supporting multiple files importing each other
    pub files: Vec<GrpcProtoFile>,
}

#[derive(Debug, Deserialize)]
pub struct GrpcProtoFile {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct GrpcReflectionRequest {
    /// Server address, e.g. `http://localhost:50051`
    pub target: String,
}

#[derive(Debug, Deserialize)]
pub struct GrpcSchemaRequest {
    /// List of `FileDescriptorProto` encoded bytes (base64), from proto import or the reflection cache
    pub files: Vec<String>,
    /// Fully-qualified name of the input message (e.g. `.pkg.Request`)
    pub input_type: String,
}

#[derive(Debug, Deserialize)]
pub struct GrpcSchemaDefRequest {
    /// List of `FileDescriptorProto` encoded bytes (base64), from proto import or the reflection cache
    pub files: Vec<String>,
    /// Fully-qualified name of the message (e.g. `.pkg.Request`)
    pub message_type: String,
}

/// Decode a base64-encoded list of FileDescriptorProto
fn decode_grpc_files(files: &[String]) -> Result<Vec<Vec<u8>>, String> {
    let mut out: Vec<Vec<u8>> = Vec::with_capacity(files.len());
    for b64 in files {
        out.push(
            BASE64_STANDARD
                .decode(b64)
                .map_err(|e| format!("base64 decode failed: {}", e))?,
        );
    }
    Ok(out)
}

/// `POST /api/grpc/import-proto`: parse proto source files → package/service/rpc tree
pub async fn grpc_import_proto_handler(Json(payload): Json<GrpcImportProtoRequest>) -> Json<Value> {
    let files: Vec<(&str, &str)> = payload
        .files
        .iter()
        .map(|f| (f.name.as_str(), f.content.as_str()))
        .collect();

    match orbit_protocol::grpc_descriptor::parse_proto_files(&files) {
        Ok(desc) => Json(json!(desc)),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

/// `POST /api/grpc/reflection`: import the interface hierarchy via gRPC Server Reflection
pub async fn grpc_reflection_handler(Json(payload): Json<GrpcReflectionRequest>) -> Json<Value> {
    match orbit_protocol::grpc_descriptor::reflect_descriptor(&payload.target).await {
        Ok(desc) => Json(json!(desc)),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

/// `POST /api/grpc/schema`: generate a JSON example template for the rpc input message
pub async fn grpc_schema_handler(Json(payload): Json<GrpcSchemaRequest>) -> Json<Value> {
    let files = match decode_grpc_files(&payload.files) {
        Ok(f) => f,
        Err(e) => return Json(json!({ "error": e })),
    };

    match orbit_protocol::grpc_descriptor::message_template(&files, &payload.input_type) {
        Ok(tpl) => Json(json!({ "template": tpl })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

/// `POST /api/grpc/schema-def`: generate a schema for the rpc input/output message (model definition dialog)
pub async fn grpc_schema_def_handler(Json(payload): Json<GrpcSchemaDefRequest>) -> Json<Value> {
    let files = match decode_grpc_files(&payload.files) {
        Ok(f) => f,
        Err(e) => return Json(json!({ "error": e })),
    };

    match orbit_protocol::grpc_descriptor::message_schema(&files, &payload.message_type) {
        Ok(schema) => Json(schema),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

// ─── Request export (curl / powershell / xh / httpie / wget / fetch / python) ──
// The implementation lives in orbit-config::exchange (the request-level exporter); here we only bridge parameters.

#[derive(Debug, Deserialize)]
struct ExportRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

async fn export_handler(Path(format): Path<String>, Json(req): Json<ExportRequest>) -> Json<Value> {
    let ep = orbit_config::exchange::EndpointSpec::new(
        "export",
        orbit_config::RequestSpec::Http(Box::new(orbit_config::HttpRequestConfig {
            method: req.method,
            url: req.url,
            headers: req.headers,
            body: if req.body.is_empty() {
                None
            } else {
                Some(serde_yaml::Value::String(req.body))
            },
            timeout: "30s".to_string(),
            payload_format: None,
            grpc_service: None,
            grpc_use_reflection: false,
            response_format: None,
        })),
    );
    match orbit_config::exchange::export_request(&format, &ep) {
        Ok(out) => Json(json!(out)),
        Err(e) => Json(json!({ "error": format!("export failed: {}", e) })),
    }
}

// ─── Collection-level export (openapi / swagger / postman) ──
// The frontend passes "range + optional snapshot"; when snapshot is omitted the server queries its own data directly (consistent between Web and Tauri).

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportCollectionRequest {
    pub format: String,
    pub title: String,
    pub snapshot: Option<orbit_data::model::Snapshot>,
    pub collection_id: String,
    pub item_id: Option<String>,
    pub workspace_id: Option<String>,
}

async fn export_collection_handler(
    State(state): State<AppState>,
    Json(payload): Json<ExportCollectionRequest>,
) -> Json<Value> {
    let snapshot = match payload.snapshot {
        Some(s) => s,
        None => state.data.snapshot(),
    };
    let range = orbit_data::export::ExportRange {
        workspace_id: payload.workspace_id,
        collection_id: payload.collection_id,
        item_id: payload.item_id,
    };
    match orbit_data::export::export_document(&snapshot, &payload.format, &payload.title, &range) {
        Ok(out) => Json(json!({ "content": out })),
        Err(e) => Json(json!({ "error": format!("export failed: {}", e) })),
    }
}

// ─── Data channel (Web uniformly goes through the Rust data layer, consistent with Tauri command behavior) ──

/// Fetch the current snapshot JSON; on first launch (nothing stored) returns json: null, and the frontend seeds and pushes the first snapshot.
async fn data_load_handler(State(state): State<AppState>) -> Json<Value> {
    if !state.data.had_initial() {
        return Json(json!({ "json": null }));
    }
    match state.data.to_json() {
        Ok(json) => Json(json!({ "json": json })),
        Err(e) => Json(json!({ "error": format!("failed to read data: {}", e) })),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataSaveRequest {
    pub json: String,
    /// Optimistic-lock base (None = force overwrite: first launch / clearing data)
    pub base_saved_at: Option<i64>,
}

/// Push snapshot JSON: deserialize + version check + replace + persist to disk (with optimistic lock).
async fn data_save_handler(
    State(state): State<AppState>,
    Json(payload): Json<DataSaveRequest>,
) -> Json<Value> {
    let res = match payload.base_saved_at {
        Some(base) => state.data.load_from_json_checked(&payload.json, base).await,
        None => state.data.load_from_json(&payload.json).await,
    };
    match res {
        Ok(()) => {
            // After the snapshot is replaced, sync the datasource config to the runtime registry (register additions / clean up deletions)
            crate::datasource_api::sync_registry_from_data(&state).await;
            Json(json!({ "ok": true }))
        }
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

// ────────────────────────────────────────────────────────────
// Response validation (validate_response_against_model)
// ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SchemaFieldDef {
    name: String,
    #[serde(rename = "type")]
    field_type: String,
    required: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ValidationError {
    path: String,
    message: String,
}

pub async fn validate_handler(Json(payload): Json<Value>) -> Json<Value> {
    let response_body = payload
        .get("responseBody")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let model_fields = payload
        .get("modelFields")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let fields: Vec<SchemaFieldDef> = match serde_json::from_str(model_fields) {
        Ok(f) => f,
        Err(e) => {
            return Json(
                json!({ "valid": false, "errors": [{ "path": "$", "message": format!("invalid model fields JSON: {}", e) }] }),
            )
        }
    };
    let body_value: serde_json::Value = match serde_json::from_str(response_body) {
        Ok(v) => v,
        Err(e) => {
            return Json(
                json!({ "valid": false, "errors": [{ "path": "$", "message": format!("response body is not valid JSON: {}", e) }] }),
            )
        }
    };

    let mut errors: Vec<ValidationError> = Vec::new();
    for field in &fields {
        if field.required.unwrap_or(false) {
            match &body_value {
                serde_json::Value::Object(map) => {
                    if !map.contains_key(&field.name) {
                        errors.push(ValidationError {
                            path: field.name.clone(),
                            message: format!("required field '{}' is missing", field.name),
                        });
                    } else if let Some(val) = map.get(&field.name) {
                        let type_ok = match field.field_type.as_str() {
                            "string" => val.is_string(),
                            "integer" => val.is_i64() || val.is_u64(),
                            "number" => val.is_number(),
                            "boolean" => val.is_boolean(),
                            "object" => val.is_object(),
                            "array" => val.is_array(),
                            "null" => val.is_null(),
                            _ => true,
                        };
                        if !type_ok {
                            errors.push(ValidationError {
                                path: field.name.clone(),
                                message: format!(
                                    "field '{}' should be of type '{}'",
                                    field.name, field.field_type
                                ),
                            });
                        }
                    }
                }
                _ => {
                    errors.push(ValidationError {
                        path: "root".into(),
                        message: "expected a JSON object".into(),
                    });
                    break;
                }
            }
        }
    }

    Json(json!({ "valid": errors.is_empty(), "errors": errors }))
}

// ────────────────────────────────────────────────────────────
// Mock interface management (a real mock server based on the interface + expectation model)
// ────────────────────────────────────────────────────────────

/// Mock rule query params (filtered by workspace)
#[derive(Debug, Deserialize)]
pub struct MockRulesQuery {
    pub workspace_id: Option<String>,
}

pub async fn mock_rules_handler(
    State(state): State<AppState>,
    Query(q): Query<MockRulesQuery>,
) -> Json<Value> {
    let rules = state.mock_rules.read().await.clone();
    // workspace_id given = only that workspace (UI); omitted = all (snapshot persistence needs the full set)
    let filtered: Vec<MockInterface> = match q.workspace_id {
        Some(ws) => rules.into_iter().filter(|r| r.ws() == ws).collect(),
        None => rules,
    };
    Json(json!(filtered))
}

pub async fn save_mock_interface_handler(
    State(state): State<AppState>,
    Json(interface): Json<MockInterface>,
) -> Json<Value> {
    let mut rules = state.mock_rules.write().await;
    // upsert: prefer matching by request_id (globally unique) to update; legacy data falls back to method+path within the same workspace
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
    // Rebuild the running mock server's interface set in place (shares the same Arc, so a running service takes effect in real time)
    let live: Vec<MockInterface> = rules.iter().filter(|r| r.enabled).cloned().collect();
    drop(rules);
    *state.mock_live.write().await = live;
    Json(json!(interface))
}

pub async fn delete_mock_interface_handler(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Json<Value> {
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let path = req.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let ws = req
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .unwrap_or(orbit_data::DEFAULT_WORKSPACE_ID);
    {
        let mut rules = state.mock_rules.write().await;
        rules
            .retain(|r| !(r.ws() == ws && r.method.eq_ignore_ascii_case(method) && r.path == path));
    }
    let live: Vec<MockInterface> = state
        .mock_rules
        .read()
        .await
        .iter()
        .filter(|r| r.enabled)
        .cloned()
        .collect();
    *state.mock_live.write().await = live;
    Json(json!({ "ok": true }))
}

// ────────────────────────────────────────────────────────────
// Long-lived sessions (interactive debugging for non-HTTP protocols)
// ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SessionSendRequest {
    pub session_id: String,
    /// Base64-encoded message payload
    pub data: String,
    /// Per-message pre-request script (optional, overrides the session default script)
    pub pre_script: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SessionCloseRequest {
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SessionQuery {
    pub session_id: String,
}

pub async fn session_open_handler(
    State(state): State<AppState>,
    Json(req): Json<OpenSessionRequest>,
) -> (axum::http::StatusCode, Json<Value>) {
    match state.sessions.open(req).await {
        Ok(resp) => (
            axum::http::StatusCode::OK,
            Json(json!({
                "session_id": resp.session_id,
                "protocol": resp.protocol,
                "can_send": resp.can_send,
            })),
        ),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "error": e })),
        ),
    }
}

pub async fn session_send_handler(
    State(state): State<AppState>,
    Json(req): Json<SessionSendRequest>,
) -> Json<Value> {
    let data = match BASE64_STANDARD.decode(&req.data) {
        Ok(b) => b,
        Err(e) => return Json(json!({ "error": format!("base64 decode failed: {e}") })),
    };
    match state
        .sessions
        .send(&req.session_id, data, req.pre_script)
        .await
    {
        Ok(seq) => Json(json!({ "ok": true, "seq": seq })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

pub async fn session_close_handler(
    State(state): State<AppState>,
    Json(req): Json<SessionCloseRequest>,
) -> Json<Value> {
    match state.sessions.close(&req.session_id).await {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// SSE real-time event stream: pushes message/error/close events within a session one by one
pub async fn session_events_handler(
    State(state): State<AppState>,
    Query(q): Query<SessionQuery>,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>,
    (axum::http::StatusCode, String),
> {
    // Subscribe first (so no events are lost between the snapshot and the live broadcast), then fetch the history replay
    let rx = state.sessions.subscribe(&q.session_id).ok_or_else(|| {
        (
            axum::http::StatusCode::NOT_FOUND,
            "session not found".to_string(),
        )
    })?;
    let backlog: Vec<Result<Event, Infallible>> = state
        .sessions
        .events_backlog(&q.session_id)
        .into_iter()
        .map(|ev| {
            Ok::<_, Infallible>(
                Event::default().data(serde_json::to_string(&ev).unwrap_or_default()),
            )
        })
        .collect();
    let live = tokio_stream::wrappers::BroadcastStream::new(rx).map(|item| match item {
        Ok(ev) => Ok::<_, Infallible>(
            Event::default().data(serde_json::to_string(&ev).unwrap_or_default()),
        ),
        Err(_) => Ok::<_, Infallible>(Event::default().data("{}")),
    });
    Ok(Sse::new(futures_util::stream::iter(backlog).chain(live)))
}

/// Session message history (in send order)
pub async fn session_messages_handler(
    State(state): State<AppState>,
    Query(q): Query<SessionQuery>,
) -> Json<Value> {
    Json(serde_json::to_value(state.sessions.messages(&q.session_id)).unwrap_or_default())
}

/// gRPC Server Reflection service discovery ({ url } → { services })
pub async fn session_grpc_reflect_handler(
    State(_state): State<AppState>,
    Json(req): Json<GrpcReflectRequest>,
) -> Json<Value> {
    match crate::session::grpc_reflect(&req.url).await {
        Ok(services) => Json(json!({ "services": services })),
        Err(e) => Json(json!({ "error": e, "services": [] })),
    }
}

#[derive(Debug, Deserialize)]
pub struct GrpcReflectRequest {
    pub url: String,
}

/// Register all Web API routes
pub fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/api/proxy", post(proxy_handler))
        .merge(crate::datasource_api::datasource_routes())
        // Structured event stream (audit / metering subscriptions)
        .route("/api/events", get(crate::events_handler))
        // Long-lived sessions (interactive debugging for non-HTTP protocols)
        .route("/api/session/open", post(session_open_handler))
        .route("/api/session/send", post(session_send_handler))
        .route("/api/session/close", post(session_close_handler))
        .route("/api/session/events", get(session_events_handler))
        .route("/api/session/messages", get(session_messages_handler))
        .route(
            "/api/session/grpc/reflect",
            post(session_grpc_reflect_handler),
        )
        .route("/api/dynamic", post(dynamic_handler))
        .route("/api/dynamic/resolve", post(dynamic_resolve_handler))
        .route("/api/import/parse", post(import_parse_handler))
        .route("/api/import/scenario", post(import_scenario_handler))
        // gRPC collections (proto import / reflection import / message templates)
        .route("/api/grpc/import-proto", post(grpc_import_proto_handler))
        .route("/api/grpc/reflection", post(grpc_reflection_handler))
        .route("/api/grpc/schema", post(grpc_schema_handler))
        .route("/api/grpc/schema-def", post(grpc_schema_def_handler))
        .route("/api/export/{format}", post(export_handler))
        .route("/api/export/collection", post(export_collection_handler))
        .route("/api/data/load", get(data_load_handler))
        .route("/api/data/save", post(data_save_handler))
        .route("/api/validate/model", post(validate_handler))
        .route("/api/mock/rules", get(mock_rules_handler))
        .route("/api/mock/interface", post(save_mock_interface_handler))
        .route(
            "/api/mock/interface/delete",
            post(delete_mock_interface_handler),
        )
        // Performance reports
        .route("/api/report/save", post(report_save_handler))
        .route("/api/report/list", get(report_list_handler))
        .route("/api/report/{id}", get(report_load_handler))
        .route("/api/report/{id}", delete(report_delete_handler))
        // Baseline management
        .route("/api/baseline/set/{id}", post(baseline_set_handler))
        .route("/api/baseline/unset/{id}", post(baseline_unset_handler))
        .route("/api/baseline/list", get(baseline_list_handler))
        // Plugin management
        .route("/api/plugins", get(plugin_list_handler))
        .route("/api/plugins/scan", post(plugin_scan_handler))
        .route("/api/plugins/load", post(plugin_load_handler))
        .route("/api/plugins/native-load", post(plugin_native_load_handler))
        .route("/api/plugins/install", post(plugin_install_handler))
        .route("/api/plugins/{id}", delete(plugin_unload_handler))
        .route("/api/plugins/{id}/enable", post(plugin_enable_handler))
        .route("/api/plugins/{id}/disable", post(plugin_disable_handler))
        .route("/api/plugins/protocols", get(get_plugin_protocols_handler))
        .route("/api/plugins/codecs", get(get_plugin_codecs_handler))
        // Protocol/format catalog: built-in + plugins (including dynamic form schemas)
        .route("/api/protocols", get(get_protocols_handler))
        .route("/api/codecs", get(get_codecs_handler))
}

// ────────────────────────────────────────────────────────────
// Performance report & baseline API
// ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ReportSaveRequest {
    pub name: String,
    pub endpoint: String,
    pub method: String,
    pub vus: u32,
    pub duration: String,
    #[serde(default)]
    pub config: Option<String>,
    pub summary: Value,
    pub thresholds: Value,
    pub all_thresholds_passed: bool,
    /// Owning workspace (reports are isolated by workspace)
    #[serde(default)]
    pub workspace_id: Option<String>,
}

async fn report_save_handler(Json(req): Json<ReportSaveRequest>) -> Json<Value> {
    use crate::reports::{save_report, SavedReport};

    let id = format!(
        "rpt-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default();

    let report = SavedReport {
        id: id.clone(),
        name: req.name,
        workspace_id: req.workspace_id,
        endpoint: req.endpoint,
        method: req.method,
        created_at,
        vus: req.vus,
        duration: req.duration,
        config: req.config,
        summary: req.summary,
        thresholds: req.thresholds,
        all_thresholds_passed: req.all_thresholds_passed,
        is_baseline: false,
        baseline_name: None,
    };

    match save_report(&report) {
        Ok(()) => Json(json!({ "status": "ok", "id": id })),
        Err(e) => Json(json!({ "error": e })),
    }
}

/// Report list query params (filtered by workspace)
#[derive(Debug, Deserialize)]
pub struct ReportListQuery {
    pub workspace_id: Option<String>,
}

async fn report_list_handler(Query(q): Query<ReportListQuery>) -> Json<Value> {
    let reports = match q.workspace_id {
        Some(ws) => crate::reports::list_reports_in(&ws),
        None => crate::reports::list_reports(),
    };
    match reports {
        Ok(reports) => Json(json!(reports)),
        Err(e) => Json(json!({ "error": e })),
    }
}

async fn report_load_handler(Path(id): Path<String>) -> Json<Value> {
    match crate::reports::load_report(&id) {
        Ok(report) => Json(json!(report)),
        Err(e) => Json(json!({ "error": e })),
    }
}

async fn report_delete_handler(Path(id): Path<String>) -> Json<Value> {
    match crate::reports::delete_report(&id) {
        Ok(()) => Json(json!({ "status": "ok" })),
        Err(e) => Json(json!({ "error": e })),
    }
}

#[derive(Debug, Deserialize)]
struct BaselineSetRequest {
    pub baseline_name: String,
}

async fn baseline_set_handler(
    Path(id): Path<String>,
    Json(req): Json<BaselineSetRequest>,
) -> Json<Value> {
    match crate::reports::set_baseline(&id, &req.baseline_name) {
        Ok(()) => Json(json!({ "status": "ok" })),
        Err(e) => Json(json!({ "error": e })),
    }
}

async fn baseline_unset_handler(Path(id): Path<String>) -> Json<Value> {
    match crate::reports::unset_baseline(&id) {
        Ok(()) => Json(json!({ "status": "ok" })),
        Err(e) => Json(json!({ "error": e })),
    }
}

async fn baseline_list_handler(Query(q): Query<ReportListQuery>) -> Json<Value> {
    let baselines = match q.workspace_id {
        Some(ws) => crate::reports::list_baselines_in(&ws),
        None => crate::reports::list_baselines(),
    };
    match baselines {
        Ok(baselines) => Json(json!(baselines)),
        Err(e) => Json(json!({ "error": e })),
    }
}

// ────────────────────────────────────────────────────────────
// Plugin management API
// ────────────────────────────────────────────────────────────

async fn plugin_list_handler(State(state): State<AppState>) -> Json<Value> {
    let mgr = state.plugins.lock().await;
    let list: Vec<orbit_plugin::PluginDescriptor> = mgr.list();
    Json(json!(list))
}

#[derive(Debug, Deserialize)]
struct PluginScanRequest {
    dir: String,
}

async fn plugin_scan_handler(
    State(state): State<AppState>,
    Json(req): Json<PluginScanRequest>,
) -> Json<Value> {
    let mut mgr = state.plugins.lock().await;
    let report = mgr.scan_dir(std::path::Path::new(&req.dir)).await;
    Json(json!({
        "scanned": report.scanned,
        "loaded": report.loaded,
        "failed": report.failed,
    }))
}

#[derive(Debug, Deserialize)]
struct PluginLoadRequest {
    id: String,
    /// WASM component bytes (base64-encoded)
    wasm_base64: String,
}

async fn plugin_load_handler(
    State(state): State<AppState>,
    Json(req): Json<PluginLoadRequest>,
) -> Json<Value> {
    let bytes = match BASE64_STANDARD.decode(&req.wasm_base64) {
        Ok(b) => b,
        Err(e) => return Json(json!({ "error": format!("base64 decode: {}", e) })),
    };
    let mut mgr = state.plugins.lock().await;
    match mgr.load_wasm(&req.id, &bytes, None).await {
        Ok((kind, ids)) => Json(json!({ "kind": kind, "capabilities": ids })),
        Err(e) => Json(json!({ "error": e })),
    }
}

#[derive(Debug, Deserialize)]
struct PluginNativeLoadRequest {
    id: String,
    /// Dynamic library path (.dll / .so / .dylib)
    path: String,
}

/// `POST /api/plugins/native-load`: load a native (dynamic library) protocol plugin.
async fn plugin_native_load_handler(
    State(state): State<AppState>,
    Json(req): Json<PluginNativeLoadRequest>,
) -> Json<Value> {
    let mut mgr = state.plugins.lock().await;
    match mgr.load_native(&req.id, std::path::Path::new(&req.path)) {
        Ok(()) => {
            let desc = mgr.get(&req.id);
            Json(json!({
                "status": "loaded",
                "id": req.id,
                "protocols": desc.map(|d| &d.protocols).cloned().unwrap_or_default(),
            }))
        }
        Err(e) => Json(json!({ "error": e })),
    }
}

async fn plugin_unload_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let mut mgr = state.plugins.lock().await;
    match mgr.uninstall(&state.plugins_root, &id) {
        Ok(()) => Json(json!({ "status": "uninstalled", "id": id })),
        Err(e) => Json(json!({ "error": e })),
    }
}

#[derive(Debug, Deserialize)]
struct PluginInstallRequest {
    id: String,
    /// Zip package bytes (base64-encoded)
    zip_base64: String,
}

/// `POST /api/plugins/install`: install a zip plugin package into `<plugins_root>/<id>`.
async fn plugin_install_handler(
    State(state): State<AppState>,
    Json(req): Json<PluginInstallRequest>,
) -> Json<Value> {
    let bytes = match BASE64_STANDARD.decode(&req.zip_base64) {
        Ok(b) => b,
        Err(e) => return Json(json!({ "error": format!("base64 decode: {}", e) })),
    };
    let mut mgr = state.plugins.lock().await;
    match mgr.install_zip(&state.plugins_root, &bytes).await {
        Ok((kind, ids)) => {
            Json(json!({ "status": "installed", "id": req.id, "kind": kind, "capabilities": ids }))
        }
        Err(e) => Json(json!({ "error": e })),
    }
}

/// `POST /api/plugins/{id}/enable`: enable (reload from the install directory and register).
async fn plugin_enable_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let mut mgr = state.plugins.lock().await;
    match mgr.enable(&id).await {
        Ok(()) => Json(json!({ "status": "enabled", "id": id })),
        Err(e) => Json(json!({ "error": e })),
    }
}

/// `POST /api/plugins/{id}/disable`: disable (unregister + destroy, keeping the directory).
async fn plugin_disable_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let mut mgr = state.plugins.lock().await;
    match mgr.disable(&id) {
        Ok(()) => Json(json!({ "status": "disabled", "id": id })),
        Err(e) => Json(json!({ "error": e })),
    }
}

async fn get_plugin_protocols_handler(State(_state): State<AppState>) -> Json<Value> {
    let ids = orbit_protocol::registry::list_protocol_ids();
    Json(json!(ids))
}

async fn get_plugin_codecs_handler(State(_state): State<AppState>) -> Json<Value> {
    let names = orbit_codec::registry::list_codec_names();
    Json(json!(names))
}

/// `GET /api/protocols`: built-in protocols + plugin protocol catalog (including dynamic form schemas).
async fn get_protocols_handler(State(state): State<AppState>) -> Json<Value> {
    let mgr = state.plugins.lock().await;
    let mut protocols = Vec::new();
    // Built-in protocols: no plugin schema (the frontend uses dedicated forms)
    for id in orbit_protocol::registry::BUILTIN_PROTOCOL_IDS {
        protocols.push(json!({
            "id": id,
            "builtin": true,
            "connectionConfigSchema": null,
            "requestConfigSchema": null,
        }));
    }
    // Plugin protocols: connection/request schemas from the manifest
    for (pid, conn_schema, req_schema) in mgr.protocol_schemas() {
        protocols.push(json!({
            "id": pid,
            "builtin": false,
            "connectionConfigSchema": conn_schema,
            "requestConfigSchema": req_schema,
        }));
    }
    protocols.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    Json(json!(protocols))
}

/// `GET /api/codecs`: built-in formats + plugin format catalog.
async fn get_codecs_handler(State(state): State<AppState>) -> Json<Value> {
    let mgr = state.plugins.lock().await;
    let mut codecs = Vec::new();
    for name in orbit_codec::registry::builtin_codec_names() {
        codecs.push(json!({ "name": name, "builtin": true }));
    }
    for name in mgr.codec_names() {
        codecs.push(json!({ "name": name, "builtin": false }));
    }
    codecs.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Json(json!(codecs))
}

#[cfg(test)]
mod proxy_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use tokio::sync::broadcast;
    use tower::util::ServiceExt;

    async fn test_state() -> AppState {
        let (event_tx, _) = broadcast::channel::<String>(64);
        AppState {
            event_tx,
            mock_task: Arc::new(Mutex::new(None)),
            mock_rules: Arc::new(tokio::sync::RwLock::new(vec![])),
            mock_live: Arc::new(tokio::sync::RwLock::new(vec![])),
            load_abort: Arc::new(AtomicBool::new(false)),
            distributed: orbit_distributed::Controller::new(),
            sessions: crate::session::SessionManager::new(),
            plugins: Arc::new(tokio::sync::Mutex::new(
                orbit_plugin::PluginManager::new().expect("plugin manager init"),
            )),
            plugins_root: std::env::temp_dir().join("orbit-test-plugins"),
            cookie_jar: Arc::new(tokio::sync::Mutex::new(
                orbit_engine::cookie_jar::CookieJar::new(),
            )),
            data: Arc::new(
                orbit_data::DataService::open(
                    orbit_data::FileStorage::new(std::env::temp_dir().join("orbit-test-data.json")),
                    orbit_data::SNAPSHOT_VERSION,
                )
                .await
                .unwrap(),
            ),
            data_sources: Arc::new(orbit_datasource::DataSourceRegistry::new()),
            event_bus: broadcast::channel::<crate::events::OrbitEvent>(64).0,
            event_sink: None,
            mock_port: Arc::new(Mutex::new(None)),
        }
    }

    /// End-to-end: a header inserted by the pre-request script pm.request.headers.upsert
    /// must both be really sent (received by the target server) and appear in the response request snapshot (frontend display / code generation).
    #[tokio::test]
    async fn pre_script_upsert_header_in_request_snapshot() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let received2 = received.clone();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = vec![0u8; 8192];
            let mut n = 0;
            loop {
                match sock.read(&mut buf[n..]).await {
                    Ok(0) => break,
                    Ok(r) => {
                        n += r;
                        if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            for line in text.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    if k.trim().eq_ignore_ascii_case("X-Signature") {
                        received2.lock().unwrap().push(v.trim().to_string());
                    }
                }
            }
            let resp =
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}";
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.flush().await;
        });

        let app = Router::new()
            .route("/api/proxy", post(proxy_handler))
            .with_state(test_state().await);
        let body = json!({
            "method": "GET",
            "url": format!("http://{}/analytics/channels", addr),
            "headers": {},
            "body": "",
            "prereq_script": "pm.request.headers.upsert({ key: 'X-Signature', value: 'abc123' });",
            "env_vars": {}
        });
        let resp = app
            .oneshot(
                Request::post("/api/proxy")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            json["request"]["headers"]["X-Signature"], "abc123",
            "the response request snapshot should contain the header inserted by the pre-request script"
        );
        assert_eq!(
            json["request"]["url"],
            format!("http://{}/analytics/channels", addr),
            "the response request snapshot url should be the actual send address"
        );

        // The target server really received the header inserted by the script (proving the real send took effect too)
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert_eq!(
            received.lock().unwrap().len(),
            1,
            "the request received by the target server should contain X-Signature"
        );
    }

    /// Interface import path (exchange import_endpoints): the
    /// prerequest / test scripts in Postman item.event[] must be extracted into ImportedEndpoint.pre_script / post_script.
    #[test]
    fn parse_postman_direct_preserves_scripts() {
        let json = r#"{
            "info": {"name": "Auth API"},
            "item": [
                {
                    "name": "Create Order",
                    "event": [
                        {
                            "listen": "prerequest",
                            "script": {
                                "type": "text/javascript",
                                "exec": [
                                    "pm.environment.set(\"token\", \"abc123\");",
                                    "console.log(\"pre done\");"
                                ]
                            }
                        },
                        {
                            "listen": "test",
                            "script": {
                                "type": "text/javascript",
                                "exec": [
                                    "pm.test(\"status is 200\", function () {",
                                    "  pm.response.to.have.status(200);",
                                    "});"
                                ]
                            }
                        }
                    ],
                    "request": {
                        "method": "POST",
                        "header": [{"key": "Content-Type", "value": "application/json"}],
                        "body": {"mode": "raw", "raw": "{\"user\":\"admin\"}"},
                        "url": {"raw": "https://api.example.com/orders"}
                    }
                }
            ]
        }"#;

        let result = orbit_config::exchange::import_endpoints("postman", json).unwrap();
        let endpoints = result.endpoints;
        assert_eq!(endpoints.len(), 1, "should parse 1 endpoint");
        let ep = &endpoints[0];
        assert!(
            ep.pre_script.contains("pm.environment.set"),
            "pre_script lost, actual: {:?}",
            ep.pre_script
        );
        assert!(
            ep.pre_script.contains("console.log(\"pre done\")"),
            "pre_script multi-line join failed: {:?}",
            ep.pre_script
        );
        assert!(
            ep.post_script.contains("pm.response.to.have.status"),
            "post_script lost, actual: {:?}",
            ep.post_script
        );
    }
}

// ────────────────────────────────────────────────────────────
// Web data channel tests (/api/data/load · /api/data/save · direct export query)
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod data_api_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    /// Build a test AppState with a DataService (in-memory snapshot, file in a temp directory)
    async fn test_state() -> (Router, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "orbit-server-data-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let data = orbit_data::DataService::open(
            orbit_data::FileStorage::new(dir.join("orbit_data.json")),
            orbit_data::SNAPSHOT_VERSION,
        )
        .await
        .unwrap();

        let state = AppState {
            event_tx: tokio::sync::broadcast::channel(8).0,
            mock_task: Arc::new(Mutex::new(None)),
            mock_rules: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            mock_live: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            load_abort: Arc::new(AtomicBool::new(false)),
            distributed: orbit_distributed::Controller::new(),
            sessions: crate::session::SessionManager::new(),
            plugins: Arc::new(tokio::sync::Mutex::new(
                orbit_plugin::PluginManager::new().unwrap(),
            )),
            plugins_root: dir.clone(),
            cookie_jar: Arc::new(tokio::sync::Mutex::new(
                orbit_engine::cookie_jar::CookieJar::new(),
            )),
            data: Arc::new(data),
            data_sources: Arc::new(orbit_datasource::DataSourceRegistry::new()),
            event_bus: tokio::sync::broadcast::channel::<crate::events::OrbitEvent>(64).0,
            event_sink: None,
            mock_port: Arc::new(Mutex::new(None)),
        };
        (api_routes().with_state(state), dir)
    }

    async fn body_json(resp: axum::response::Response) -> Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Minimal usable snapshot (1 collection + 1 HTTP request + 1 model)
    fn sample_snapshot_json() -> String {
        serde_json::json!({
            "schemaVersion": 1,
            "savedAt": 1780000000000_i64,
            "source": "web",
            "sync": { "remoteUrl": null, "lastSyncedAt": null },
            "data": {
                "collections": [{
                    "id": "c1",
                    "name": "Demo",
                    "items": [
                        { "type": "folder", "id": "f1", "name": "Users",
                          "items": [ { "type": "request", "id": "i1", "requestId": "r1" } ] }
                    ]
                }],
                "requests": {
                    "r1": {
                        "id": "r1", "name": "Create User", "method": "POST",
                        "url": "https://api.example.com/users",
                        "headers": [], "queryParams": [], "body": "{}",
                        "bodyMode": "json", "contentType": "application/json",
                        "formParams": [], "binaryFile": null,
                        "auth": { "type": "none" }, "cookies": [],
                        "prereqScript": "pm.environment.set('t','1');",
                        "modelId": "m1"
                    }
                },
                "models": [{
                    "id": "m1", "name": "CreateUser", "description": null,
                    "fields": [ { "id": "f1", "name": "name", "type": "string",
                                  "required": true, "description": "user name" } ]
                }],
                "environments": [], "activeEnvId": null,
                "globalVariables": {}, "globalSecrets": {},
                "scenarios": [], "plugins": [], "history": [], "mockRules": [],
                "locale": "zh-CN", "theme": "dark",
                "ui": { "sidebarCollapsed": false },
                "executionTarget": { "mode": "local", "agentIds": null }
            }
        })
        .to_string()
    }

    #[tokio::test]
    async fn data_load_first_launch_returns_null() {
        let (app, dir) = test_state().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/data/load")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(
            body["json"],
            Value::Null,
            "first launch should return json: null"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn data_save_then_load_roundtrip() {
        let (app, dir) = test_state().await;
        let json = sample_snapshot_json();

        // Save (forced overwrite on first launch)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data/save")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({ "json": json }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["ok"], json!(true));

        // Load (fetch the snapshot just saved)
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/data/load")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_json(resp).await;
        let loaded: orbit_data::Snapshot =
            serde_json::from_str(body["json"].as_str().unwrap()).unwrap();
        assert_eq!(loaded.data.collections[0].name, "Demo");
        assert_eq!(loaded.data.requests.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn optimistic_lock_rejects_stale_save() {
        let (app, dir) = test_state().await;
        let json = sample_snapshot_json();

        // First save (forced)
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data/save")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({ "json": json }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Save again with a stale base → conflict
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data/save")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "json": json,
                            "baseSavedAt": 1
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_json(resp).await;
        assert!(
            body["error"].as_str().unwrap_or("").contains("conflict"),
            "a stale base should conflict, actual: {body}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn export_collection_queries_server_data() {
        let (app, dir) = test_state().await;
        let json = sample_snapshot_json();

        // Save the data first
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data/save")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({ "json": json }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Export (no snapshot passed, the server queries its own data directly)
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/export/collection")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "format": "openapi",
                            "title": "Demo",
                            "collectionId": "c1",
                            "itemId": null
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        let content = body["content"].as_str().expect("export content missing");
        assert!(
            content.contains("openapi"),
            "should generate an openapi document: {content}"
        );
        assert!(
            content.contains("#/components/schemas/CreateUser"),
            "model $ref missing: {content}"
        );
        assert!(
            content.contains("x-orbit-prerequest"),
            "pre-request script missing: {content}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
