//! Shared request execution pipeline (RequestPipeline)
//!
//! Unifies the two paths - single-shot (HTTP Server / Tauri) and load testing (FlowRunner) - for
//! pre-actions (the built-in interpolation node splits pre-/post-interpolation) -> send -> decode -> assert -> extract -> post-actions orchestration.
//! Plugins (protocol / codec) hook in only here: protocols via [`PipelineRuntime::client_for`] (M3 dynamic registry),
//! codecs via the `orbit-codec` registry (built-in + plugins).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use orbit_assertion::builtins;
use orbit_assertion::engine::AssertionSet;
use orbit_assertion::types::{
    AssertionContext, AssertionResult, Comparator, DbTarget as AssertDbTarget, QueryResult,
    RetryPolicy,
};
use orbit_assertion::{
    extract_columns, extract_target, query_sql_retry, redis_command_retry, summarize_result,
};
use orbit_codec::registry::{build_codec, codec_for_format, codec_for_mime, CodecKind};
use orbit_codec::traits::Codec;
use orbit_config::{Check, Extraction};
use orbit_extractor::ExtractorSet;
use orbit_js::{JsSandbox, ScriptLog, TestResult};
use orbit_protocol::guard::execute_guarded;
use orbit_protocol::registry::{build_client, build_client_by_id};
use orbit_protocol::traits::ProtocolClient;
use orbit_protocol::types::{ProtocolOptions, ProtocolRequest, ProtocolResponse};
use orbit_protocol::ProtocolKind;
use tokio_util::sync::CancellationToken;

use crate::cookie_jar::{merge_cookie_header, CookieJar};

/// Pipeline runtime: protocol client cache + codec cache + script sandbox (one per single-shot / load run).
/// Protocol clients are cached by **string protocol id**: built-in (ProtocolKind) and WASM plugin dynamic protocols share one entry point.
pub struct PipelineRuntime {
    pub default_protocol: Box<dyn ProtocolClient>,
    pub protocol_cache: HashMap<String, Box<dyn ProtocolClient>>,
    pub default_codec: Box<dyn Codec>,
    pub codec_cache: HashMap<CodecKind, Box<dyn Codec>>,
    /// Datasource access capability (once injected, DB/Redis assertions work; when `None` the related assertions fail explicitly instead of passing silently)
    pub datasources: Option<Arc<dyn orbit_assertion::DataSourceProvider>>,
}

impl PipelineRuntime {
    pub fn new(default_protocol: Box<dyn ProtocolClient>, default_codec: Box<dyn Codec>) -> Self {
        Self {
            default_protocol,
            protocol_cache: HashMap::new(),
            default_codec,
            codec_cache: HashMap::new(),
            datasources: None,
        }
    }

    /// Injects the datasource capability (global datasource registry) so DB/Redis assertions can query after the request.
    pub fn with_datasources(
        &mut self,
        datasources: Option<Arc<dyn orbit_assertion::DataSourceProvider>>,
    ) -> &mut Self {
        self.datasources = datasources;
        self
    }

    fn codec_for(&mut self, kind: CodecKind) -> &mut Box<dyn Codec> {
        self.codec_cache
            .entry(kind)
            .or_insert_with(|| build_codec(kind))
    }

    /// Selects a client by string protocol id: http uses the default instance; built-ins are built and cached by enum;
    /// everything else (WASM plugin dynamic protocols) goes through the dynamic registry cache.
    pub fn client_for(&mut self, id: &str) -> &mut dyn ProtocolClient {
        if let Some(kind) = ProtocolKind::parse(id) {
            if kind == ProtocolKind::Http {
                return self.default_protocol.as_mut();
            }
            let key = id.to_ascii_lowercase();
            return self
                .protocol_cache
                .entry(key.clone())
                .or_insert_with(|| build_client(kind))
                .as_mut();
        }
        // Dynamic (plugin) protocol: fall back to the default HTTP client when build_client_by_id finds nothing
        let key = id.to_ascii_lowercase();
        self.protocol_cache
            .entry(key.clone())
            .or_insert_with(|| {
                build_client_by_id(id).unwrap_or_else(|| {
                    tracing::warn!("protocol '{}' is not registered (built-in or plugin), falling back to HTTP", id);
                    Box::new(orbit_protocol::http::HttpClient::new())
                })
            })
            .as_mut()
    }

    /// Encodes a structured request body by `payload_format`; a string body is passed through verbatim (reusable by callers)
    pub fn encode_body(&mut self, body: &serde_yaml::Value, format: Option<&str>) -> Vec<u8> {
        encode_body(self, body, format)
    }

    /// Decodes the response body: explicit response_format -> Content-Type inference -> default codec (reusable by callers)
    pub fn decode_body(
        &mut self,
        payload: &[u8],
        metadata: &[(String, String)],
        response_format: Option<&str>,
    ) -> Option<orbit_codec::DataValue> {
        decode_body(self, payload, metadata, response_format)
    }
}

/// Protocol-agnostic request execution description (built by the caller from Step / ProxyRequest)
pub struct PipelineSpec {
    /// Protocol id (HTTP / websocket / tcp / ...; M3 supports plugin protocol string ids)
    pub protocol: String,
    /// Target address (HTTP=url; TCP/UDP/WS=host:port; gRPC=server url)
    pub target: String,
    /// Operation name (HTTP=method; gRPC=service/method)
    pub operation: String,
    pub headers: HashMap<String, String>,
    /// Pre-encoded request body bytes (scripts may rewrite; when empty, body_value is encoded by request_format)
    pub body: Vec<u8>,
    /// Structured request body (for codec encoding)
    pub body_value: Option<serde_yaml::Value>,
    pub options: ProtocolOptions,
    /// Connection config (JSON string; from the collection connectionConfigSchema, read by plugin protocols)
    pub connection: Option<String>,
    /// gRPC streaming mode (None = unary; ServerStreaming / ClientStreaming / Bidirectional)
    pub streaming_mode: Option<orbit_protocol::types::StreamingMode>,
    /// Request body encoding format (request_format, may name a plugin codec)
    pub request_format: Option<String>,
    /// Response decoding format (response_format, may name a plugin codec)
    pub response_format: Option<String>,
    pub timeout: Option<Duration>,
    /// Legacy pre-scripts (still used by single-shot / distributed agent / long-lived session paths; effective when non-empty and `pre_actions` is empty)
    pub pre_scripts: Vec<String>,
    /// Legacy post-scripts
    pub post_scripts: Vec<String>,
    /// Pre-action list (JS scripts / read-only DB queries / built-in interpolation node, executed in order;
    /// takes precedence over `pre_scripts` when non-empty).
    ///
    /// **List order is execution order**; the built-in [`PipelineAction::Interpolate`] node splits execution timing:
    /// - **Before** the node: acts on the un-interpolated request template - may write variables (`pm.environment.set` /
    ///   `pm.variables.set`) for this interpolation to consume, and may rewrite the template text (the rewrite is still interpolated);
    /// - **After** the node: acts on the final payload - the rewrite is the final bytes, with no second interpolation (signing / encryption).
    ///
    /// When the list lacks the node (un-normalized caller), the anchor is inserted **first** so legacy
    /// `pre_scripts` / existing actions still run after interpolation.
    ///
    /// Mapped by [`actions_to_pipeline`] from config actions: script library references (`{ type: ref }`)
    /// are expanded to library-item content during mapping; unexpandable ones become [`PipelineAction::UnresolvedRef`].
    pub pre_actions: Vec<PipelineAction>,
    /// Post-action list (executed in order after the response; takes precedence over `post_scripts` when non-empty)
    ///
    /// Like `pre_actions`, mapped by [`actions_to_pipeline`] (script library references already expanded).
    pub post_actions: Vec<PipelineAction>,
    pub checks: Vec<Check>,
    pub extracts: Vec<Extraction>,
    /// Whether to interpolate variables/dynamic values in this pipeline (single-shot=false, front end already interpolated; load=true, regenerated each time)
    pub interpolate: bool,
    /// The single-shot path's **un-interpolated request template** (`None` = the caller already built target / headers / body).
    ///
    /// When `Some`, the pipeline takes over URL assembly (path / query parameter encoding), header merging
    /// (Content-Type / Host / Content-Length) and body assembly (including multipart file reads),
    /// and strips connection-level headers before sending (Host / Content-Length etc. are left to the underlying client).
    ///
    /// Note: the text body (json / xml / raw) still lives in [`PipelineSpec::body_value`],
    /// because pre-interpolation scripts read/write it via `pm.request.body.raw`.
    pub request_template: Option<crate::request_build::RequestTemplate>,
    /// Build only, do not send: return the final request right after pre-interpolation actions / interpolation / encoding / post-interpolation actions.
    ///
    /// Lets the distributed agent path resolve on the **control side** (the agent only sends the final payload),
    /// and is used for request previews. The returned `PipelineOutcome` has `response` and `error` both `None`.
    pub dry_run: bool,
}

impl Default for PipelineSpec {
    fn default() -> Self {
        Self {
            protocol: "http".into(),
            target: String::new(),
            operation: "GET".into(),
            headers: HashMap::new(),
            body: Vec::new(),
            body_value: None,
            options: ProtocolOptions::default(),
            connection: None,
            streaming_mode: None,
            request_format: None,
            response_format: None,
            timeout: None,
            pre_scripts: Vec::new(),
            post_scripts: Vec::new(),
            pre_actions: Vec::new(),
            post_actions: Vec::new(),
            checks: Vec::new(),
            extracts: Vec::new(),
            interpolate: false,
            request_template: None,
            dry_run: false,
        }
    }
}

/// Runtime action: mapped from the config layer [`orbit_config::RequestAction`] (enabled items only).
#[derive(Debug, Clone)]
pub enum PipelineAction {
    /// JS script action
    Script { name: String, code: String },
    /// Read-only database query action
    Db(DbActionSpec),
    /// Built-in "interpolate" node: turns the request template into the final payload (variable interpolation + URL / body assembly and encoding).
    ///
    /// Maintained by the system (not editable / deletable / disable-able); the only position marker in the pre-action list.
    Interpolate,
    /// **Unresolved script library reference**: the library item does not exist (deleted) or the caller supplied no library.
    ///
    /// The executor logs one error-level entry then **continues** (consistent with "a failed action does not abort the request"),
    /// rather than skipping silently - a silent drop would hide from the user that their orchestration no longer works.
    UnresolvedRef { library_id: String, name: String },
}

/// Runtime description of a database action (SQL / Redis, one of the two; results are written to variables)
#[derive(Debug, Clone)]
pub struct DbActionSpec {
    pub name: String,
    pub datasource: String,
    /// Relational database read-only SQL
    pub sql: Option<String>,
    /// Redis read-only command
    pub command: Option<String>,
    pub args: Vec<String>,
    /// Single-value extraction mode (defaults to scalar)
    pub target: Option<AssertDbTarget>,
    /// Variable name to write the single value into
    pub extract_var: Option<String>,
    /// Multi-column mapping `(column name, variable name)`
    pub columns: Vec<(String, String)>,
    /// Row index for the multi-column mapping
    pub row: usize,
    pub retry: Option<RetryPolicy>,
}

/// Execution log for a single action (the front end shows status / elapsed / written variables in action order)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionLog {
    /// Phase: `pre_resolve` (before the anchor) / `interpolate` (built-in interpolation node) / `pre` (after the anchor) / `post`
    pub phase: String,
    /// Kind: `script` / `db` / `interpolate`
    pub kind: String,
    /// Action name (the caller supplies a default when the config is empty)
    pub name: String,
    /// Whether it succeeded (script action = no exception; DB action = query succeeded and the configured variable was obtained)
    pub ok: bool,
    /// Elapsed milliseconds (script action counts sandbox execution; DB action counts query + retry total time)
    pub elapsed_ms: u64,
    /// Variables written by this action
    pub vars_written: HashMap<String, String>,
    /// Result summary (DB: row/column counts; failure: reason)
    pub detail: String,
    /// Console logs from the script action
    pub logs: Vec<ScriptLog>,
}

impl ActionLog {
    #[allow(clippy::too_many_arguments)]
    fn script(
        phase: &str,
        name: String,
        ok: bool,
        elapsed_ms: u64,
        vars_written: HashMap<String, String>,
        logs: Vec<ScriptLog>,
        detail: String,
    ) -> Self {
        Self {
            phase: phase.into(),
            kind: "script".into(),
            name,
            ok,
            elapsed_ms,
            vars_written,
            detail,
            logs,
        }
    }
}

/// Config actions -> runtime actions.
///
/// First expand script library references by `library` ([`orbit_config::resolve_action_refs`]), **then** filter out disabled items.
/// The order matters: a disabled library item shows up via the expanded action, so filtering first would miss "disabling a library item disables it everywhere".
///
/// References still unresolved after expansion (dangling refs / caller supplied no library) map to
/// [`PipelineAction::UnresolvedRef`], on which the executor logs an error without aborting the request - never a silent drop.
pub fn actions_to_pipeline(
    actions: &[orbit_config::RequestAction],
    library: &[orbit_config::ActionTemplate],
) -> Vec<PipelineAction> {
    orbit_config::resolve_action_refs(actions, library)
        .iter()
        .filter(|a| a.is_enabled())
        .map(|a| match a {
            orbit_config::RequestAction::Script { name, code, .. } => PipelineAction::Script {
                name: name.clone(),
                code: code.clone(),
            },
            orbit_config::RequestAction::Db {
                name,
                datasource,
                sql,
                command,
                args,
                target,
                extract_var,
                columns,
                row,
                retry,
                ..
            } => PipelineAction::Db(DbActionSpec {
                name: name.clone(),
                datasource: datasource.clone(),
                sql: sql.clone(),
                command: command.clone(),
                args: args.clone(),
                target: target.as_ref().map(map_db_target),
                extract_var: extract_var.clone(),
                columns: columns
                    .iter()
                    .map(|c| (c.column.clone(), c.var.clone()))
                    .collect(),
                row: *row,
                retry: retry.as_ref().map(map_retry),
            }),
            orbit_config::RequestAction::Interpolate => PipelineAction::Interpolate,
            // Still a reference after expansion => missing library item: keep id / alias and let the executor report it
            orbit_config::RequestAction::Ref {
                library_id, name, ..
            } => PipelineAction::UnresolvedRef {
                library_id: library_id.clone(),
                name: name.clone().unwrap_or_default(),
            },
        })
        .collect()
}

/// Merges the explicit action list with the legacy script fields: the explicit list wins when non-empty, otherwise legacy scripts are wrapped as script actions.
pub fn effective_actions(
    actions: &[PipelineAction],
    legacy_scripts: &[String],
) -> Vec<PipelineAction> {
    if !actions.is_empty() {
        return actions.to_vec();
    }
    legacy_scripts
        .iter()
        .filter(|s| !s.trim().is_empty())
        .map(|code| PipelineAction::Script {
            name: String::new(),
            code: code.clone(),
        })
        .collect()
}

/// Position of a pre-action relative to the built-in interpolation node (decides rewrite semantics and log attribution).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageKind {
    /// **Before** the anchor (pre-interpolation): acts on the request **template**, the rewrite is still interpolated; may write variables for this interpolation to consume.
    PreResolve,
    /// **After** the anchor (post-interpolation): acts on the **final request**, the rewrite is the final bytes with no second interpolation.
    Pre,
}

impl StageKind {
    /// `ActionLog.phase` values (the front end groups execution results by phase)
    fn phase(self) -> &'static str {
        match self {
            StageKind::PreResolve => "pre_resolve",
            StageKind::Pre => "pre",
        }
    }
}

/// In-place rewrite targets for a run of pre-actions (url / method / headers / body).
struct StageRequest<'a> {
    target: &'a mut String,
    operation: &'a mut String,
    headers: &'a mut HashMap<String, String>,
    body: &'a mut Vec<u8>,
    body_value: &'a mut Option<serde_yaml::Value>,
}

/// Execution products of a run of pre-actions (the caller aggregates them into [`PipelineOutcome`] by phase).
#[derive(Default)]
struct StageRun {
    /// Action logs (in execution order)
    logs: Vec<ActionLog>,
    /// Script console logs (merged into `PipelineOutcome.pre_logs`)
    scripts: Vec<ScriptLog>,
    /// `pm.environment.set` writes (returned via varsSet; the front end persists them to the environment)
    vars_set: HashMap<String, String>,
    /// `pm.variables.set` writes (lifecycle of this request only)
    temp_vars_set: HashMap<String, String>,
}

/// The built-in interpolation node body: turns the request template into the **final payload** (variable interpolation + URL / body assembly and encoding).
///
/// - When `interpolate` is false, only the encoding finish-up runs (the caller already built the request, e.g. a payload a distributed agent sends directly);
/// - `content_type_out` returns the Content-Type derived from the template, so the finish-up stage can add `Content-Type` / `Host` /
///   `Content-Length` (happening after all pre-actions, so the length reflects the scripts' final rewrite);
/// - Returning `Err` = request build failure (local errors such as template file reads / base64 decoding; no request was sent).
#[allow(clippy::too_many_arguments)]
fn run_interpolate_node(
    rt: &mut PipelineRuntime,
    req: StageRequest<'_>,
    interpolate: bool,
    interp_vars: &HashMap<String, String>,
    template: Option<&crate::request_build::RequestTemplate>,
    request_format: Option<&str>,
    content_type_out: &mut Option<String>,
) -> Result<(), String> {
    let StageRequest {
        target,
        operation,
        headers,
        body,
        body_value,
    } = req;

    if interpolate {
        let url = interp(target.as_str(), interp_vars);
        let method = interp(operation.as_str(), interp_vars);
        let hdrs: HashMap<String, String> = headers
            .iter()
            .map(|(k, v)| (k.clone(), interp(v, interp_vars)))
            .collect();
        *target = url;
        *operation = method;
        *headers = hdrs;
        let text = match &*body_value {
            Some(serde_yaml::Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        if let Some(text) = text {
            *body_value = Some(serde_yaml::Value::String(interp(&text, interp_vars)));
        }
    }

    if let Some(tpl) = template {
        // URL path / query parameters and body assembly happen after interpolation
        *target = tpl.finalize_url(target.as_str(), interp_vars);
        match tpl.finalize_body(body_value.as_ref(), interp_vars) {
            Ok((bytes, content_type)) => {
                *body = bytes;
                *content_type_out = content_type;
            }
            Err(e) => return Err(e),
        }
    } else if body.is_empty() {
        if let Some(v) = &*body_value {
            *body = encode_body(rt, v, request_format);
        }
    }
    // Discard the structured body once encoding is done: actions after the anchor read the exact bytes about to be sent
    *body_value = None;
    Ok(())
}

/// Executes a **single** pre-action (rewriting the request in place); results accumulate into `run`.
///
/// - `kind` is decided by the action's position relative to the built-in interpolation node (see [`StageKind`]);
/// - Variables written by an action are visible to **later actions** (both `pm.environment.get` / `pm.variables.get`
///   and the DB action's SQL can read them) - `stage_vars` is the write snapshot shared across the whole pre-stage;
/// - Variables written by a DB action merge into `vars` (existing behavior: later steps can reference them); a script's
///   `pm.environment.set` merges into `vars` only **before** the anchor (for this interpolation to consume); after the anchor
///   it is only returned via `vars_set` for persistence;
/// - Script rewrite of the body: before the anchor it is written back to `body_value` as a **template** (still interpolated later);
///   after the anchor it lands directly in the `body` bytes (i.e. the final bytes);
/// - Replace only when the script truly changed the body text, avoiding a lossy "bytes -> string -> bytes" round trip
///   for a binary body when only headers were changed;
/// - A failed action does not abort the request (existing semantics): the error is logged and the action's rewrite is not applied.
#[allow(clippy::too_many_arguments)]
async fn run_pre_action(
    rt: &mut PipelineRuntime,
    action: &PipelineAction,
    kind: StageKind,
    req: StageRequest<'_>,
    vars: &mut HashMap<String, String>,
    env: &HashMap<String, String>,
    action_vars: &mut HashMap<String, String>,
    stage_vars: &mut HashMap<String, String>,
    run: &mut StageRun,
) {
    let StageRequest {
        target,
        operation,
        headers,
        body,
        body_value,
    } = req;

    match action {
        // The built-in anchor is executed by the caller (needs interpolation and encoding context, see [`run_interpolate_node`])
        PipelineAction::Interpolate => {}
        // Dangling / unresolved script library reference: log at error level then continue (no request abort)
        PipelineAction::UnresolvedRef { library_id, name } => {
            let label = action_name(name, "script library ref", run.logs.len());
            let detail =
                format!("referenced script library item missing or not provided: {library_id}");
            run.scripts.push(ScriptLog {
                level: "error".into(),
                message: format!("pre-action \"{label}\" {detail}"),
            });
            run.logs.push(ActionLog {
                phase: kind.phase().into(),
                kind: "ref".into(),
                name: label,
                ok: false,
                elapsed_ms: 0,
                vars_written: HashMap::new(),
                detail,
                logs: Vec::new(),
            });
        }
        PipelineAction::Db(db) => {
            let ctx_vars = overlay(merged_vars(env, vars, action_vars), stage_vars);
            let ran = run_db_action(rt, db, &ctx_vars).await;
            let name = action_name(&db.name, "database query", run.logs.len());
            run.scripts.push(ScriptLog {
                level: level_of(ran.ok).into(),
                message: format!("database action \"{}\": {}", name, ran.detail),
            });
            for (k, v) in &ran.vars {
                // Variables written by the DB action merge into the request variable space (existing behavior: later steps can reference them)
                vars.insert(k.clone(), v.clone());
                action_vars.insert(k.clone(), v.clone());
                stage_vars.insert(k.clone(), v.clone());
            }
            run.logs.push(ActionLog {
                phase: kind.phase().into(),
                kind: "db".into(),
                name,
                ok: ran.ok,
                elapsed_ms: ran.elapsed_ms,
                vars_written: ran.vars,
                detail: ran.detail,
                logs: Vec::new(),
            });
        }
        PipelineAction::Script { name, code } => {
            if code.trim().is_empty() {
                return;
            }
            // Variables visible to the script = environment | request variable space | variables already written in the pre-stage
            // (both `pm.environment.get` and `pm.variables.get` work)
            let env_plus = overlay(merged_vars(env, vars, action_vars), stage_vars);
            let seed_temp = stage_vars.clone();
            let script_body_text = match &*body_value {
                Some(v) => serde_json::to_string(v).unwrap_or_default(),
                None => String::from_utf8_lossy(body).into_owned(),
            };
            let plain_body = script_body_text.clone();
            let body_bytes = bytes_to_byte_string(&body[..]);
            let (url, method, hdrs) = (target.clone(), operation.clone(), headers.clone());
            let started = std::time::Instant::now();
            let executed = orbit_js::with_sandbox(|sandbox| {
                let sandbox = sandbox?;
                let mut req_ctx = orbit_js::RequestContext {
                    url,
                    method,
                    headers: hdrs,
                    body: script_body_text,
                    raw: body_bytes,
                };
                let result =
                    sandbox.run_pre_request(code, &mut req_ctx, Some(&env_plus), Some(&seed_temp));
                Some((result, req_ctx))
            });
            let Some((result, req_ctx)) = executed else {
                return;
            };
            let elapsed = started.elapsed().as_millis() as u64;
            run.scripts.extend(result.logs.clone());
            // Env vars written before the anchor must merge into the variable space (this interpolation consumes them)
            let merges_into_vars = matches!(kind, StageKind::PreResolve);
            for (k, v) in &result.vars_set {
                run.vars_set.insert(k.clone(), v.clone());
                stage_vars.insert(k.clone(), v.clone());
                if merges_into_vars {
                    vars.insert(k.clone(), v.clone());
                }
            }
            for (k, v) in &result.temp_vars_set {
                run.temp_vars_set.insert(k.clone(), v.clone());
                stage_vars.insert(k.clone(), v.clone());
            }

            let detail = if result.success {
                *target = req_ctx.url;
                *operation = req_ctx.method;
                *headers = req_ctx.headers;
                if !req_ctx.body.is_empty() && req_ctx.body != plain_body {
                    match kind {
                        // Before the anchor: the script writes back the template, which is still interpolated later
                        StageKind::PreResolve => {
                            *body = Vec::new();
                            *body_value = Some(serde_yaml::Value::String(req_ctx.body));
                        }
                        // After the anchor: the script's rewrite is the final bytes
                        StageKind::Pre => {
                            *body = req_ctx.body.into_bytes();
                            *body_value = None;
                        }
                    }
                }
                "applied request rewrite".to_string()
            } else {
                // Script failed: log the error at error level and keep sending the original request
                let err = result
                    .error
                    .clone()
                    .unwrap_or_else(|| "script execution failed".to_string());
                run.scripts.push(ScriptLog {
                    level: "error".into(),
                    message: format!("pre-script execution error: {}", err),
                });
                err
            };
            run.logs.push(ActionLog::script(
                kind.phase(),
                action_name(name, "script", run.logs.len()),
                result.success,
                elapsed,
                result.vars_set.clone(),
                result.logs.clone(),
                detail,
            ));
        }
    }
}

/// Overlays an extra layer of variables onto the existing variable space (the latter overrides the former).
fn overlay(
    mut base: HashMap<String, String>,
    extra: &HashMap<String, String>,
) -> HashMap<String, String> {
    for (k, v) in extra {
        base.insert(k.clone(), v.clone());
    }
    base
}

/// Request snapshot after script/hook rewrites (lets the caller show what was actually sent)
pub struct PipelineRequest {
    pub target: String,
    pub operation: String,
    pub headers: HashMap<String, String>,
    pub payload: Vec<u8>,
}

/// Pipeline execution result (response + decode + assertion/extraction + error classification)
pub struct PipelineResponse {
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub payload: Vec<u8>,
    pub decoded: Option<orbit_codec::DataValue>,
    pub duration_ms: u64,
    pub message_count: u64,
    /// Phase timings (consistent with built-in protocols; used by single-shot JSON timing)
    pub timings: orbit_protocol::types::ProtocolTimings,
}

/// Pipeline error classification (aligned with the load-sample error_type semantics)
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineError {
    Protocol(String),
    PreScript(String),
    PostScript(String),
    /// Request build failure (local errors such as template expansion / file reads / base64 decoding; no request was sent)
    RequestBuild(String),
    Assertion,
    Timeout(String),
}

impl PipelineError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::Protocol(_) => "protocol",
            Self::PreScript(_) => "pre_script",
            Self::PostScript(_) => "post_script",
            Self::RequestBuild(_) => "request_build",
            Self::Assertion => "assertion",
            Self::Timeout(_) => "timeout",
        }
    }
    pub fn message(&self) -> String {
        match self {
            Self::Protocol(m)
            | Self::PreScript(m)
            | Self::PostScript(m)
            | Self::RequestBuild(m)
            | Self::Timeout(m) => m.clone(),
            Self::Assertion => "Assertion failed".into(),
        }
    }
}

/// Pipeline summary result (the caller projects it to MetricSample or single-shot JSON)
pub struct PipelineOutcome {
    pub request: PipelineRequest,
    pub response: Option<PipelineResponse>,
    pub tests: Vec<AssertionResult>,
    pub extracted_vars: HashMap<String, String>,
    pub error: Option<PipelineError>,
    // ── Script execution returns (used by single-shot JSON's preLogs/postLogs/postTests/varsSet/tempVarsSet; ignored by load testing) ──
    /// Pre-script logs (including script errors; a failed script does not abort the request, errors are recorded here)
    pub pre_logs: Vec<ScriptLog>,
    /// Post-script logs
    pub post_logs: Vec<ScriptLog>,
    /// Post-script pm.test assertion results (postTests)
    pub post_tests: Vec<TestResult>,
    /// Decoded result written by the post-script's pm.response.decoded (used by the distributed agent's debug return)
    pub post_decoded: Option<String>,
    /// Pre+post script pm.environment.set writes (post overrides pre)
    pub vars_set: HashMap<String, String>,
    /// Pre+post script pm.variables.set temp variables (this request only)
    pub temp_vars_set: HashMap<String, String>,
    // ── Action execution returns (pre-action list: script / database query / built-in interpolation node; post-action list) ──
    /// Pre-action logs (in execution order, including the built-in interpolation node entry whose `phase = "interpolate"`)
    pub pre_action_logs: Vec<ActionLog>,
    /// Post-action logs (in execution order)
    pub post_action_logs: Vec<ActionLog>,
    /// Variables written by actions (DB queries) (for front-end display and scenario variable-space references)
    pub action_vars: HashMap<String, String>,
}

/// Unified execution entry: pre-actions (including the built-in interpolation node) -> send -> decode -> extract -> assert -> post-actions
///
/// Pre-actions are a **single ordered list**; the built-in interpolation node ([`PipelineAction::Interpolate`]) splits it into two segments:
/// - **Before the node**: acts on the un-interpolated request template and may write variables for this interpolation (aligned with Postman's pre-request script semantics),
///   the template rewrite is still interpolated;
/// - **After the node**: acts on the final payload; the rewrite is the final bytes with no second interpolation (where signing / encryption go).
///
/// `cookies`: optional Cookie Jar (session persistence). When passed in, accumulated same-domain cookies are attached before sending,
/// and Set-Cookie is absorbed after the response. Single-shot passes a shared jar (session kept across requests); load testing passes one jar per VU.
pub async fn execute_pipeline(
    rt: &mut PipelineRuntime,
    spec: PipelineSpec,
    vars: &mut HashMap<String, String>,
    env: &HashMap<String, String>,
    cancel: &CancellationToken,
    mut cookies: Option<&mut CookieJar>,
) -> PipelineOutcome {
    // Pre-action list (explicit actions win, legacy single-script fields as fallback) - must be taken before spec fields are moved.
    //
    // When an un-normalized caller (building spec directly / legacy `pre_scripts`) has no built-in interpolation node in the list,
    // the anchor is inserted **first**: this keeps existing "pre-scripts" running after interpolation (zero migration).
    let mut pre_actions = effective_actions(&spec.pre_actions, &spec.pre_scripts);
    if !pre_actions
        .iter()
        .any(|a| matches!(a, PipelineAction::Interpolate))
    {
        pre_actions.insert(0, PipelineAction::Interpolate);
    }
    let post_actions = effective_actions(&spec.post_actions, &spec.post_scripts);

    let mut target = spec.target;
    let mut operation = spec.operation;
    let mut headers = spec.headers;
    let mut body = spec.body;
    let mut body_value = spec.body_value;

    // 0. Single-shot path: expand the "un-interpolated request template" into a template-state skeleton (URL + headers),
    //    for scripts before the anchor to read/write; URL path/query assembly and body building are deferred to the interpolation node.
    let request_template = spec.request_template;
    if let Some(tpl) = &request_template {
        let (url, hdrs) = tpl.expand_template();
        target = url;
        headers = hdrs;
        // Text body template -> `body_value` (source of truth afterwards: scripts before the anchor may rewrite it, and interpolation acts on it)
        if body_value.is_none() {
            if let Some(text) = tpl.text_body() {
                body_value = Some(serde_yaml::Value::String(text.to_string()));
            }
        }
    }

    // Post-script returns (used by single-shot JSON)
    let mut post_logs: Vec<ScriptLog> = Vec::new();
    let mut post_tests: Vec<TestResult> = Vec::new();
    let mut post_vars: HashMap<String, String> = HashMap::new();
    let mut post_temp_vars: HashMap<String, String> = HashMap::new();

    // Pre-stage returns: action logs (in execution order, including the built-in interpolation node entry) + variables written by scripts
    let mut pre_run = StageRun::default();
    let mut post_action_logs: Vec<ActionLog> = Vec::new();
    let mut action_vars: HashMap<String, String> = HashMap::new();
    // Snapshot of variables written by actions in the pre-stage: later scripts / DB actions can read them
    let mut stage_vars: HashMap<String, String> = HashMap::new();

    // 1. Pre-stage: a **single pass** in list order.
    //
    //    At the built-in interpolation node, run "variable interpolation + URL / body assembly and encoding" (turning the template into the final payload):
    //    - Actions **before** the node act on the request template: they may write variables (`pm.environment.set` /
    //      `pm.variables.set`) for this interpolation - a hard requirement for "a script generates random data that then feeds interpolation";
    //    - Actions **after** the node act on the final payload: the rewrite is the final bytes with no second interpolation.
    //    A failed action does not abort the request (existing semantics): the error is logged and the rewrite is not applied.
    let interpolate = spec.interpolate || request_template.is_some();
    let mut template_content_type: Option<String> = None;
    let mut passed_anchor = false;
    for action in &pre_actions {
        if matches!(action, PipelineAction::Interpolate) {
            if passed_anchor {
                // Duplicate anchor (un-normalized input): execute only once
                continue;
            }
            passed_anchor = true;
            let started = std::time::Instant::now();
            // Overlay temp variables written by pre-anchor actions onto the variable space (the per-request scope of `pm.variables.set`)
            let interp_vars = overlay(merged_vars(env, vars, &action_vars), &pre_run.temp_vars_set);
            if let Err(e) = run_interpolate_node(
                rt,
                StageRequest {
                    target: &mut target,
                    operation: &mut operation,
                    headers: &mut headers,
                    body: &mut body,
                    body_value: &mut body_value,
                },
                interpolate,
                &interp_vars,
                request_template.as_ref(),
                spec.request_format.as_deref(),
                &mut template_content_type,
            ) {
                return error_outcome(
                    PipelineError::RequestBuild(e),
                    PipelineRequest {
                        target,
                        operation,
                        headers,
                        payload: body,
                    },
                    pre_run.scripts,
                    pre_run.logs,
                    action_vars,
                    pre_run.vars_set,
                    pre_run.temp_vars_set,
                );
            }
            // The anchor itself emits a log entry: the result panel renders interpolation position and elapsed time in execution order
            pre_run.logs.push(ActionLog {
                phase: "interpolate".into(),
                kind: "interpolate".into(),
                name: "variable interpolation".into(),
                ok: true,
                elapsed_ms: started.elapsed().as_millis() as u64,
                vars_written: HashMap::new(),
                detail: "expanded the request template into the final payload".into(),
                logs: Vec::new(),
            });
            continue;
        }

        let kind = if passed_anchor {
            StageKind::Pre
        } else {
            StageKind::PreResolve
        };
        run_pre_action(
            rt,
            action,
            kind,
            StageRequest {
                target: &mut target,
                operation: &mut operation,
                headers: &mut headers,
                body: &mut body,
                body_value: &mut body_value,
            },
            vars,
            env,
            &mut action_vars,
            &mut stage_vars,
            &mut pre_run,
        )
        .await;
    }

    // 2. Template path finish-up: fill in Content-Type / Host / Content-Length.
    //    Done after all pre-actions, so the snapshot length is the actually-sent size;
    //    connection-level headers are shown only in the snapshot and stripped when sending (left to the underlying client).
    if let Some(tpl) = &request_template {
        tpl.finalize_headers(&mut headers, template_content_type, &target, body.len());
    }

    // 3. Build only, do not send (distributed agent pre-resolution / request preview):
    //    the request is already the "final payload"; return directly without opening a connection or running the post-stage.
    if spec.dry_run {
        return PipelineOutcome {
            request: PipelineRequest {
                target,
                operation,
                headers,
                payload: body,
            },
            response: None,
            tests: Vec::new(),
            extracted_vars: HashMap::new(),
            error: None,
            pre_logs: pre_run.scripts,
            post_logs: Vec::new(),
            post_tests: Vec::new(),
            post_decoded: None,
            vars_set: pre_run.vars_set,
            temp_vars_set: pre_run.temp_vars_set,
            pre_action_logs: pre_run.logs,
            post_action_logs: Vec::new(),
            action_vars,
        };
    }

    // 4. Cookie Jar (session persistence): automatically attach accumulated same-domain cookies;
    //    a user-configured Cookie header wins; same names are not overwritten and the jar only fills missing ones.
    if let Some(jar) = cookies.as_deref_mut() {
        if let Some(cookie_header) = jar.header_for(&target) {
            match headers
                .iter_mut()
                .find(|(k, _)| k.eq_ignore_ascii_case("cookie"))
            {
                Some(manual) => {
                    *manual.1 = merge_cookie_header(manual.1, &cookie_header);
                }
                None => {
                    headers.insert("Cookie".into(), cookie_header);
                }
            }
        }
    }

    // 5. Send (timeout/cancellation guaranteed by the host)
    //    The template path carries connection-level headers like Host / Content-Length for the snapshot,
    //    which must be stripped when sending, or they conflict with the underlying client's auto-computed names (some servers return 400).
    let strip_hop_by_hop = request_template.is_some();
    let request = ProtocolRequest {
        target: target.clone(),
        operation: operation.clone(),
        metadata: headers
            .iter()
            .filter(|(k, _)| !strip_hop_by_hop || !crate::request_build::is_hop_by_hop(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        payload: body.clone(),
        timeout: spec.timeout,
        streaming_mode: spec.streaming_mode,
        payload_format: spec.request_format.clone(),
        response_format: spec.response_format.clone(),
        options: spec.options.clone(),
        connection: spec.connection.clone(),
    };
    let client = rt.client_for(&spec.protocol);
    let start = std::time::Instant::now();
    let response = match execute_guarded(client, request, cancel).await {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            let category = if msg.to_lowercase().contains("timeout")
                || msg.to_lowercase().contains("timed out")
            {
                PipelineError::Timeout(msg)
            } else {
                PipelineError::Protocol(msg)
            };
            return error_outcome(
                category,
                PipelineRequest {
                    target,
                    operation,
                    headers,
                    payload: body,
                },
                pre_run.scripts,
                pre_run.logs,
                action_vars,
                pre_run.vars_set,
                pre_run.temp_vars_set,
            );
        }
    };
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    // 6. Cookie Jar: absorb the response Set-Cookie (attached automatically to later same-domain requests)
    if let Some(jar) = cookies {
        let set_cookies: Vec<String> = response
            .metadata
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, v)| v.clone())
            .collect();
        if !set_cookies.is_empty() {
            jar.absorb(&target, &set_cookies);
        }
    }

    // 7. Decode the response body (response_format -> Content-Type -> default codec)
    let decoded = decode_body(
        rt,
        &response.payload,
        &response.metadata,
        spec.response_format.as_deref(),
    );

    let resp_headers: HashMap<String, String> = response.metadata.iter().cloned().collect();

    // 8. Variable extraction (before assertions: the DB assertion SQL can reference variables extracted from this response,
    //    matching the JMeter Post-Processor -> Assertion / HttpRunner extract -> validate timing)
    let mut extracted_vars = HashMap::new();
    if !spec.extracts.is_empty() {
        if let Ok(extracted) = perform_extraction(&spec.extracts, &response.payload, &resp_headers)
        {
            for (k, v) in &extracted {
                vars.insert(k.clone(), v.clone());
                extracted_vars.insert(k.clone(), v.clone());
            }
        }
    }

    // 9. Assertions (built-in + DB/Redis + plugins): extracted variables are already written to vars;
    //    the datasource capability is injected so DB/Redis assertions can check the DB after the response.
    //    Variables written by pre-scripts' pm.environment.set / pm.variables.set also merge into the assertion variable space,
    //    so DB/Redis assertion SQL can reference variables set by pre-scripts (e.g. {{user_id}}).
    let mut assertion_vars = vars.clone();
    for (k, v) in &pre_run.vars_set {
        assertion_vars.insert(k.clone(), v.clone());
    }
    for (k, v) in &pre_run.temp_vars_set {
        assertion_vars.insert(k.clone(), v.clone());
    }
    let assertion_ctx = AssertionContext {
        status_code: response.status_code,
        headers: resp_headers.clone(),
        body_bytes: response.payload.clone(),
        body_data: decoded.clone(),
        duration_ms: duration_ms as u64,
        body_size: response.payload.len(),
        env_vars: env.clone(),
        extracted_vars: assertion_vars,
        datasources: rt.datasources.clone(),
    };
    let (has_hard_failure, tests) = evaluate_checks(&spec.checks, &assertion_ctx).await;
    // DB/Redis assertion extract_var: merges the actual value into the request variable space for post-scripts and later steps
    for test in &tests {
        for (k, v) in &test.exported_vars {
            vars.insert(k.clone(), v.clone());
        }
    }

    // 10. Post-actions (read-only DB queries / JS scripts), executed in list order.
    //
    // The "environment" visible to post-actions = original env vars | request variable space (including response extraction and DB/Redis assertion
    // extract_var values). Thus on the single-shot path, post-actions can also read via `pm.environment.get('varName')`
    // the variables exported by assertions, consistent with the flow_runner scenario path (which injects self.variables as env).
    // Variables are shared among multiple post-actions: variables written by script pm.environment.set and DB actions merge into post_env.
    let mut post_env = env.clone();
    for (k, v) in &pre_run.vars_set {
        post_env.insert(k.clone(), v.clone());
    }
    for (k, v) in vars.iter() {
        post_env.insert(k.clone(), v.clone());
    }
    let mut post_error: Option<String> = None;
    let mut post_decoded: Option<String> = None;
    for action in &post_actions {
        match action {
            PipelineAction::Db(db) => {
                // post_env already contains env vars + request variable space + variables written by earlier actions
                let ctx_vars = post_env.clone();
                let ran = run_db_action(rt, db, &ctx_vars).await;
                let name = action_name(&db.name, "database query", post_action_logs.len());
                post_logs.push(ScriptLog {
                    level: level_of(ran.ok).into(),
                    message: format!("database action \"{}\": {}", name, ran.detail),
                });
                for (k, v) in &ran.vars {
                    vars.insert(k.clone(), v.clone());
                    action_vars.insert(k.clone(), v.clone());
                    // Merge into post_env / post_vars: visible to later post-actions and persisted via vars_set
                    post_env.insert(k.clone(), v.clone());
                    post_vars.insert(k.clone(), v.clone());
                }
                post_action_logs.push(ActionLog {
                    phase: "post".into(),
                    kind: "db".into(),
                    name,
                    ok: ran.ok,
                    elapsed_ms: ran.elapsed_ms,
                    vars_written: ran.vars,
                    detail: ran.detail,
                    logs: Vec::new(),
                });
            }
            // Dangling / unresolved script library reference: log at error level then continue
            PipelineAction::UnresolvedRef { library_id, name } => {
                let label = action_name(name, "script library ref", post_action_logs.len());
                let detail =
                    format!("referenced script library item missing or not provided: {library_id}");
                post_logs.push(ScriptLog {
                    level: "error".into(),
                    message: format!("post-action \"{label}\" {detail}"),
                });
                post_action_logs.push(ActionLog {
                    phase: "post".into(),
                    kind: "ref".into(),
                    name: label,
                    ok: false,
                    elapsed_ms: 0,
                    vars_written: HashMap::new(),
                    detail,
                    logs: Vec::new(),
                });
            }
            PipelineAction::Script { name, code } => {
                if code.trim().is_empty() {
                    continue;
                }
                let started = std::time::Instant::now();
                let result = orbit_js::with_sandbox(|sandbox| {
                    run_post_script(
                        sandbox,
                        code,
                        &response,
                        &resp_headers,
                        duration_ms,
                        &post_env,
                        &pre_run.temp_vars_set,
                    )
                });
                let elapsed = started.elapsed().as_millis() as u64;
                match result {
                    Ok(out) => {
                        post_logs.extend(out.logs.clone());
                        post_tests.extend(out.tests);
                        if out.decoded.is_some() {
                            post_decoded = out.decoded;
                        }
                        for (k, v) in &out.vars_set {
                            post_vars.insert(k.clone(), v.clone());
                            post_env.insert(k.clone(), v.clone());
                        }
                        for (k, v) in &out.temp_vars_set {
                            post_temp_vars.insert(k.clone(), v.clone());
                        }
                        post_action_logs.push(ActionLog::script(
                            "post",
                            action_name(name, "script", post_action_logs.len()),
                            true,
                            elapsed,
                            out.vars_set.clone(),
                            out.logs.clone(),
                            "executed".to_string(),
                        ));
                    }
                    Err(e) => {
                        post_error = Some(e.clone());
                        post_logs.push(ScriptLog {
                            level: "error".into(),
                            message: format!("post-script execution error: {}", e),
                        });
                        post_action_logs.push(ActionLog::script(
                            "post",
                            action_name(name, "script", post_action_logs.len()),
                            false,
                            elapsed,
                            HashMap::new(),
                            Vec::new(),
                            e.clone(),
                        ));
                        break;
                    }
                }
            }
            // The post-action list contains no built-in interpolation node (the anchor appears only in the pre list)
            PipelineAction::Interpolate => {}
        }
    }

    let error = if has_hard_failure {
        Some(PipelineError::Assertion)
    } else {
        post_error.map(PipelineError::PostScript)
    };

    // Merge variables written by scripts across stages (later overrides earlier) - used by single-shot JSON varsSet/tempVarsSet
    let vars_set = merged_written(&[&pre_run.vars_set, &post_vars]);
    let temp_vars_set = merged_written(&[&pre_run.temp_vars_set, &post_temp_vars]);

    PipelineOutcome {
        request: PipelineRequest {
            target,
            operation,
            headers,
            payload: body,
        },
        response: Some(PipelineResponse {
            status_code: response.status_code as u16,
            headers: resp_headers,
            payload: response.payload,
            decoded,
            duration_ms: duration_ms as u64,
            message_count: response.message_count,
            timings: response.timings,
        }),
        tests,
        extracted_vars,
        error,
        pre_logs: pre_run.scripts,
        post_logs,
        post_tests,
        post_decoded,
        vars_set,
        temp_vars_set,
        pre_action_logs: pre_run.logs,
        post_action_logs,
        action_vars,
    }
}

/// Fallback outcome for protocol/timeout errors (keeps executed pre-action logs and written variables for diagnosis).
///
/// A network failure does **not** affect `pm.environment.set` / `pm.variables.set` results: the scripts already ran,
/// and the front end still needs `vars_set` to persist them to the environment.
#[allow(clippy::too_many_arguments)]
fn error_outcome(
    err: PipelineError,
    request: PipelineRequest,
    pre_logs: Vec<ScriptLog>,
    pre_action_logs: Vec<ActionLog>,
    action_vars: HashMap<String, String>,
    vars_set: HashMap<String, String>,
    temp_vars_set: HashMap<String, String>,
) -> PipelineOutcome {
    PipelineOutcome {
        request,
        response: None,
        tests: Vec::new(),
        extracted_vars: HashMap::new(),
        error: Some(err),
        pre_logs,
        post_logs: Vec::new(),
        post_tests: Vec::new(),
        post_decoded: None,
        vars_set,
        temp_vars_set,
        pre_action_logs,
        post_action_logs: Vec::new(),
        action_vars,
    }
}

/// Sequentially merges variables written by multiple script stages (later overrides earlier).
fn merged_written(stages: &[&HashMap<String, String>]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for stage in stages {
        for (k, v) in *stage {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// Merges the variable spaces: `base | vars | extra` (the latter overrides the former).
fn merged_vars(
    base: &HashMap<String, String>,
    vars: &HashMap<String, String>,
    extra: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut out = base.clone();
    for (k, v) in vars {
        out.insert(k.clone(), v.clone());
    }
    for (k, v) in extra {
        out.insert(k.clone(), v.clone());
    }
    out
}

/// Default action name: generated from "type + index" when unnamed, keeping logs readable.
fn action_name(name: &str, kind: &str, index: usize) -> String {
    if name.trim().is_empty() {
        format!("{} #{}", kind, index + 1)
    } else {
        name.trim().to_string()
    }
}

fn level_of(ok: bool) -> &'static str {
    if ok {
        "log"
    } else {
        "error"
    }
}

/// Value preview: truncate and flatten newlines to avoid leaking large result sets or plaintext passwords in logs.
fn preview_value(s: &str) -> String {
    const MAX: usize = 80;
    let flat = s.replace(['\n', '\r'], " ");
    if flat.chars().count() <= MAX {
        flat
    } else {
        let truncated: String = flat.chars().take(MAX).collect();
        format!("{truncated}…")
    }
}

/// Execution result of a single database action.
pub(crate) struct DbActionRun {
    pub(crate) ok: bool,
    pub(crate) elapsed_ms: u64,
    pub(crate) vars: HashMap<String, String>,
    pub(crate) detail: String,
}

impl DbActionRun {
    fn fail(detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            elapsed_ms: 0,
            vars: HashMap::new(),
            detail: detail.into(),
        }
    }
}

/// Executes a read-only database action: SQL or Redis command -> extract value -> write to variable.
///
/// Failure does not abort the request (the caller decides the error semantics); only read-only datasource APIs are called.
pub(crate) async fn run_db_action(
    rt: &PipelineRuntime,
    db: &DbActionSpec,
    vars: &HashMap<String, String>,
) -> DbActionRun {
    let Some(provider) = rt.datasources.clone() else {
        return DbActionRun::fail(
            "datasource capability not injected (datasources is empty); database action cannot run",
        );
    };

    let datasource = interp_vars(&db.datasource, vars);
    let has_sql = db
        .sql
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_cmd = db
        .command
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !has_sql && !has_cmd {
        return DbActionRun::fail("no SQL or Redis command configured");
    }
    if datasource.trim().is_empty() {
        return DbActionRun::fail("no datasource selected");
    }
    // Whether variable writing is configured (when not configured it is only a probe, not a failure)
    let needs_var = db
        .extract_var
        .as_deref()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
        || !db.columns.is_empty();

    // ── Redis read-only command ──
    if has_cmd {
        let command = interp_vars(db.command.as_deref().unwrap_or_default(), vars);
        let mut args: Vec<String> = Vec::with_capacity(db.args.len() + 1);
        args.push(command.clone());
        for a in &db.args {
            args.push(interp_vars(a, vars));
        }
        let (result, attempts, elapsed) = redis_command_retry(
            provider.as_ref(),
            &datasource,
            &args,
            db.retry.as_ref(),
            |v| !v.is_empty(),
        )
        .await;
        return match result {
            Ok(value) => {
                let mut written = HashMap::new();
                if let Some(var) = db.extract_var.as_deref().filter(|v| !v.trim().is_empty()) {
                    written.insert(var.to_string(), value.clone());
                }
                let ok = !needs_var || !written.is_empty();
                DbActionRun {
                    ok,
                    elapsed_ms: elapsed,
                    vars: written,
                    detail: format!(
                        "Redis {} -> {} ({} attempts / {}ms)",
                        command,
                        preview_value(&value),
                        attempts,
                        elapsed
                    ),
                }
            }
            Err(e) => DbActionRun {
                ok: false,
                elapsed_ms: elapsed,
                vars: HashMap::new(),
                detail: format!(
                    "Redis command failed ({} attempts / {}ms): {}",
                    attempts, elapsed, e
                ),
            },
        };
    }

    // ── Relational DB read-only SQL ──
    let sql = interp_vars(db.sql.as_deref().unwrap_or_default(), vars);
    let target = db.target.clone().unwrap_or(AssertDbTarget::Scalar);
    let columns = db.columns.clone();
    let row = db.row;
    let (result, attempts, elapsed) = query_sql_retry(
        provider.as_ref(),
        &datasource,
        &sql,
        db.retry.as_ref(),
        |r: &QueryResult| {
            let target_ok = extract_target(&target, r).is_some();
            let cols_ok = columns.iter().all(|(c, _)| r.cell(row, c).is_some());
            target_ok && cols_ok
        },
    )
    .await;

    match result {
        Ok(qr) => {
            let mut written = HashMap::new();
            if let Some(var) = db.extract_var.as_deref().filter(|v| !v.trim().is_empty()) {
                if let Some(v) = extract_target(&target, &qr) {
                    written.insert(var.to_string(), v);
                }
            }
            for (var, val) in extract_columns(&qr, row, &columns) {
                written.insert(var, val);
            }
            let ok = !needs_var || !written.is_empty();
            let detail = if ok {
                format!(
                    "{} ({} attempts / {}ms)",
                    summarize_result(&qr),
                    attempts,
                    elapsed
                )
            } else {
                format!(
                    "query succeeded but the configured variable was not obtained ({} attempts / {}ms)",
                    attempts, elapsed
                )
            };
            DbActionRun {
                ok,
                elapsed_ms: elapsed,
                vars: written,
                detail,
            }
        }
        Err(e) => DbActionRun {
            ok: false,
            elapsed_ms: elapsed,
            vars: HashMap::new(),
            detail: format!(
                "query failed ({} attempts / {}ms): {}",
                attempts, elapsed, e
            ),
        },
    }
}

/// Interpolates action parameters (`{{var}}` / `${var}`, normalized then handled uniformly; missing variables are kept verbatim).
fn interp_vars(template: &str, vars: &HashMap<String, String>) -> String {
    let norm = template.replace("{{", "${").replace("}}", "}");
    replace_tokens(&norm, vars)
}

/// Variable/dynamic-value interpolation ({{$...}} regenerated each time; ${var} substituted)
fn interp(s: &str, vars: &HashMap<String, String>) -> String {
    orbit_config::interpolate(s, vars).unwrap_or_else(|_| s.to_string())
}

/// Encodes a structured request body by `payload_format`; a string body is passed through verbatim
fn encode_body(
    rt: &mut PipelineRuntime,
    body: &serde_yaml::Value,
    format: Option<&str>,
) -> Vec<u8> {
    match body {
        serde_yaml::Value::String(s) => s.as_bytes().to_vec(),
        other => {
            let default = serde_json::to_vec(other).unwrap_or_default();
            match format {
                Some(f) if !f.eq_ignore_ascii_case("json") => {
                    if let Some(kind) = codec_for_format(f) {
                        let json: serde_json::Value =
                            serde_json::to_value(other).unwrap_or(serde_json::Value::Null);
                        let data = serde_json::from_value::<orbit_codec::DataValue>(json)
                            .unwrap_or(orbit_codec::DataValue::Null);
                        rt.codec_for(kind).encode(&data).unwrap_or(default)
                    } else {
                        default
                    }
                }
                _ => default,
            }
        }
    }
}

/// Decodes the response body: explicit response_format -> Content-Type inference -> default codec
fn decode_body(
    rt: &mut PipelineRuntime,
    payload: &[u8],
    metadata: &[(String, String)],
    response_format: Option<&str>,
) -> Option<orbit_codec::DataValue> {
    if payload.is_empty() {
        return None;
    }
    if let Some(fmt) = response_format {
        if let Some(kind) = codec_for_format(fmt) {
            if let Ok(v) = rt.codec_for(kind).decode(payload) {
                return Some(v);
            }
        }
    }
    let mime = metadata
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.split(';').next().unwrap_or(v).trim());
    if let Some(mime) = mime {
        if let Some(kind) = codec_for_mime(mime) {
            if let Ok(v) = rt.codec_for(kind).decode(payload) {
                return Some(v);
            }
        }
    }
    rt.default_codec.decode(payload).ok()
}

/// Evaluates the assertion list (built-in + DB/Redis + plugins), returning (whether there is a hard failure, assertion results).
///
/// DB/Redis assertion SQL / expected values are interpolated here (reusing request-context variables),
/// and use the variables extracted at the request's "assertion moment" (extraction runs before assertions).
pub async fn evaluate_checks(
    checks: &[Check],
    ctx: &AssertionContext,
) -> (bool, Vec<AssertionResult>) {
    let mut assertion_set = AssertionSet::new();
    // One-to-one with results (disabled checks are skipped and not counted)
    let mut custom_names: Vec<String> = Vec::new();
    for check in checks {
        let meta = check.meta.as_ref();
        if !meta.map(|m| m.enabled).unwrap_or(true) {
            continue;
        }
        let custom = meta.and_then(|m| m.name.clone()).unwrap_or_default();
        match &check.kind {
            orbit_config::CheckKind::Status { value } => {
                assertion_set.add(Box::new(builtins::StatusAssertion {
                    expected: *value,
                    comparator: Comparator::Equal,
                }));
            }
            orbit_config::CheckKind::BodyContains { value } => {
                assertion_set.add(Box::new(builtins::BodyContainsAssertion {
                    needle: value.clone(),
                }));
            }
            orbit_config::CheckKind::DurationLt { value } => {
                let max_ms = orbit_config::parse_duration(value).unwrap_or(500.0) * 1000.0;
                assertion_set.add(Box::new(builtins::DurationAssertion {
                    max_ms: max_ms as u64,
                }));
            }
            orbit_config::CheckKind::JsonPath {
                path,
                comparator,
                expected,
            } => {
                assertion_set.add(Box::new(builtins::JsonPathAssertion {
                    path: path.clone(),
                    comparator: parse_comparator(comparator),
                    expected: expected.clone(),
                }));
            }
            orbit_config::CheckKind::JmesPath {
                expression,
                comparator,
                expected,
            } => {
                assertion_set.add(Box::new(builtins::JmesPathAssertion {
                    expression: expression.clone(),
                    comparator: parse_comparator(comparator),
                    expected: expected.clone(),
                }));
            }
            orbit_config::CheckKind::Regex { pattern } => {
                assertion_set.add(Box::new(builtins::RegexAssertion {
                    pattern: pattern.clone(),
                }));
            }
            orbit_config::CheckKind::SizeLt { value } => {
                assertion_set.add(Box::new(builtins::SizeAssertion { max_bytes: *value }));
            }
            orbit_config::CheckKind::XPath {
                path,
                comparator,
                expected,
            } => {
                assertion_set.add(Box::new(builtins::XPathAssertion {
                    path: path.clone(),
                    comparator: parse_comparator(comparator),
                    expected: expected.clone(),
                }));
            }
            orbit_config::CheckKind::JsonSchema { schema } => {
                assertion_set.add(Box::new(builtins::JsonSchemaAssertion {
                    schema_json: schema.clone(),
                }));
            }
            orbit_config::CheckKind::Header {
                name,
                comparator,
                expected,
            } => {
                assertion_set.add(Box::new(builtins::HeaderAssertion {
                    header_name: name.clone(),
                    comparator: parse_comparator(comparator),
                    expected: expected.clone(),
                }));
            }
            orbit_config::CheckKind::CssSelector {
                selector,
                comparator,
                expected,
            } => {
                assertion_set.add(Box::new(builtins::CssSelectorAssertion {
                    selector: selector.clone(),
                    comparator: parse_comparator(comparator),
                    expected: expected.clone(),
                }));
            }
            orbit_config::CheckKind::Db {
                datasource,
                sql,
                target,
                comparator,
                expected,
                retry,
                extract_var,
                hard,
            } => {
                // Pre-interpolation: SQL / datasource name / expected value support ${var} / ${env:var} / {{var}} references
                assertion_set.add(Box::new(builtins::DbQueryAssertion {
                    name: String::new(),
                    datasource: interp_value(datasource, ctx),
                    sql: interp_value(sql, ctx),
                    target: map_db_target(target),
                    comparator: parse_comparator(comparator),
                    expected: interp_value(expected, ctx),
                    retry: retry.clone().map(|r| map_retry(&r)),
                    hard: *hard,
                    extract_var: extract_var.clone(),
                }));
            }
            orbit_config::CheckKind::Redis {
                datasource,
                command,
                args,
                comparator,
                expected,
                retry,
                extract_var,
                hard,
            } => {
                let mut cmd_args: Vec<String> = Vec::with_capacity(args.len() + 1);
                cmd_args.push(interp_value(command, ctx));
                for a in args {
                    cmd_args.push(interp_value(a, ctx));
                }
                assertion_set.add(Box::new(builtins::RedisAssertion {
                    name: String::new(),
                    datasource: interp_value(datasource, ctx),
                    args: cmd_args,
                    comparator: parse_comparator(comparator),
                    expected: interp_value(expected, ctx),
                    retry: retry.clone().map(|r| map_retry(&r)),
                    hard: *hard,
                    extract_var: extract_var.clone(),
                }));
            }
        }
        custom_names.push(custom);
    }
    let mut results = assertion_set.evaluate_all(ctx).await;
    // User-defined assertion names override the type default (results and custom_names are one-to-one)
    for (result, name) in results.iter_mut().zip(custom_names) {
        if !name.is_empty() {
            result.name = name;
        }
    }
    let has_failure = assertion_set.has_hard_failure(&results);
    (has_failure, results)
}

/// Interpolates against the request context (${var} / ${env:var} / {{var}}, normalized then handled uniformly).
///
/// Missing variables are left as-is and later execution errors naturally, avoiding silently swallowing variable names.
fn interp_value(template: &str, ctx: &AssertionContext) -> String {
    let mut vars = ctx.env_vars.clone();
    for (k, v) in &ctx.extracted_vars {
        vars.insert(k.clone(), v.clone());
    }
    // The front-end single-shot path uses {{name}}, load-test YAML uses ${name}: normalized uniformly.
    // Note: `{{name}}` -> `${name}` requires `{{`->`${` and `}}`->`}` (not deleted to "",
    // otherwise the closing brace is swallowed giving `${name`, leaving an unclosed `$` in the SQL).
    let norm = template.replace("{{", "${").replace("}}", "}");
    replace_tokens(&norm, &vars)
}

/// Lenient interpolation: only replaces resolvable `${...}` / `${env:...}`; unresolvable ones are kept verbatim.
fn replace_tokens(template: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            // Unclosed: keep the remaining text from `${` onward. Do not re-push the whole rest,
            // otherwise the prefix would be duplicated (which once left an extra unquoted `$` in SQL).
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        // Exclude the dynamic-value form ${$fake...} (left to request-side dynamic-value handling)
        if name.starts_with('$') {
            out.push_str(&rest[start..start + 2 + end + 1]);
        } else if let Some(value) = lookup_var(name, vars) {
            out.push_str(&value);
        } else {
            out.push_str(&rest[start..start + 2 + end + 1]);
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn lookup_var(name: &str, vars: &HashMap<String, String>) -> Option<String> {
    if let Some(rest) = name.strip_prefix("env:") {
        return std::env::var(rest).ok();
    }
    vars.get(name).cloned().or_else(|| std::env::var(name).ok())
}

/// Config-model DbTarget -> assertion-layer extraction mode.
fn map_db_target(target: &orbit_config::DbTarget) -> AssertDbTarget {
    match target {
        orbit_config::DbTarget::RowCount => AssertDbTarget::RowCount,
        orbit_config::DbTarget::Scalar => AssertDbTarget::Scalar,
        orbit_config::DbTarget::Cell { row, column } => AssertDbTarget::Cell {
            row: *row,
            column: column.clone(),
        },
        orbit_config::DbTarget::Row { row } => AssertDbTarget::Row { row: *row },
        orbit_config::DbTarget::JsonPath { row, path } => AssertDbTarget::JsonPath {
            row: *row,
            path: path.clone(),
        },
    }
}

/// Config-model RetryPolicy -> assertion-layer retry policy.
fn map_retry(r: &orbit_config::RetryPolicy) -> RetryPolicy {
    RetryPolicy {
        interval_ms: r.interval_ms,
        max_attempts: r.max_attempts.max(1),
        timeout_ms: r.timeout_ms,
    }
}

/// Performs variable extraction and returns the result (the caller decides whether to write to vars)
pub fn perform_extraction(
    extracts: &[Extraction],
    payload: &[u8],
    headers: &HashMap<String, String>,
) -> Result<HashMap<String, String>, String> {
    let mut extractor_set = ExtractorSet::new();
    for ext in extracts {
        match &ext.source {
            orbit_config::ExtractionSource::JsonPath { path } => {
                extractor_set.add(
                    ext.name.clone(),
                    orbit_extractor::JsonPathExtractor::new(path.clone()),
                );
            }
            orbit_config::ExtractionSource::JmesPath { expression } => {
                extractor_set.add(
                    ext.name.clone(),
                    orbit_extractor::JmesPathExtractor::new(expression.clone()),
                );
            }
            orbit_config::ExtractionSource::Header { name } => {
                extractor_set.add(
                    ext.name.clone(),
                    orbit_extractor::HeaderExtractor::new(name.clone()),
                );
            }
            orbit_config::ExtractionSource::Regex { pattern, group } => {
                extractor_set.add(
                    ext.name.clone(),
                    orbit_extractor::RegexExtractor::new(pattern.clone(), *group),
                );
            }
            orbit_config::ExtractionSource::Cookie { name, attr } => {
                extractor_set.add(
                    ext.name.clone(),
                    orbit_extractor::CookieExtractor::new(name.clone(), attr.clone()),
                );
            }
        }
    }
    if extractor_set.is_empty() {
        return Ok(HashMap::new());
    }
    // Extraction failure is not treated as request failure (existing semantics: no match means ignore)
    Ok(extractor_set
        .extract_all(payload, headers)
        .unwrap_or_default())
}

/// Runs the post-script (read-only pm.response + pm.test assertions + pm.environment.set/variables.set write-back).
/// On success returns the script outcome (logs/assertions/variables); on failure returns an error message.
///
/// `sandbox` is a reused sandbox (lazily created by the pipeline runtime); `None` means the sandbox is unavailable (creation failed).
pub fn run_post_script(
    sandbox: Option<&JsSandbox>,
    script: &str,
    response: &ProtocolResponse,
    headers: &HashMap<String, String>,
    duration_ms: f64,
    env: &HashMap<String, String>,
    temp_vars: &HashMap<String, String>,
) -> Result<orbit_js::ScriptOutcome, String> {
    if script.trim().is_empty() {
        return Ok(orbit_js::ScriptOutcome::default());
    }
    let Some(sandbox) = sandbox else {
        return Err("script sandbox unavailable".to_string());
    };
    let resp_ctx = orbit_js::ResponseContext {
        status: response.status_code as u16,
        body: String::from_utf8_lossy(&response.payload).to_string(),
        headers: headers.clone(),
        duration_ms: duration_ms as u64,
        raw: bytes_to_byte_string(&response.payload),
        decoded: None,
    };
    let result = sandbox.run_post_response(script, &resp_ctx, Some(env), Some(temp_vars));
    if result.success {
        Ok(result)
    } else {
        Err(result.error.unwrap_or_else(|| "post script failed".into()))
    }
}

/// Bytes -> byte string (each char 0-255), for QuickJS scripts to read/write pm.request.raw / pm.response.raw
pub fn bytes_to_byte_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Byte string -> bytes
pub fn byte_string_to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u8).collect()
}

/// Name -> comparator. The name table lives in [`Comparator::from_name`] (single source; AI docs and tests validate against it);
/// this keeps the historical "unknown name degrades to Equal" behavior so existing configs don't error on typos.
fn parse_comparator(s: &str) -> Comparator {
    Comparator::from_name(s).unwrap_or(Comparator::Equal)
}

#[cfg(test)]
mod interp_value_tests {
    use super::*;

    fn ctx_with(vars: &[(&str, &str)]) -> AssertionContext {
        let mut extracted_vars = HashMap::new();
        for (k, v) in vars {
            extracted_vars.insert((*k).to_string(), (*v).to_string());
        }
        AssertionContext {
            status_code: 200,
            headers: HashMap::new(),
            body_bytes: Vec::new(),
            body_data: None,
            duration_ms: 0,
            body_size: 0,
            env_vars: HashMap::new(),
            extracted_vars,
            datasources: None,
        }
    }

    /// Reproduces a production issue: the DB assertion SQL uses `{{user_id}}`, the variable is written by a pre-script.
    #[test]
    fn double_brace_placeholder_is_interpolated() {
        let ctx = ctx_with(&[("user_id", "152371")]);
        let sql = "SELECT user_id, nickname from accountsinfo where user_id = '{{user_id}}';";
        assert_eq!(
            interp_value(sql, &ctx),
            "SELECT user_id, nickname from accountsinfo where user_id = '152371';"
        );
    }

    /// `${var}` (YAML / load-test style) works as well.
    #[test]
    fn dollar_brace_placeholder_is_interpolated() {
        let ctx = ctx_with(&[("orderId", "42")]);
        assert_eq!(
            interp_value("SELECT status FROM orders WHERE id = '${orderId}'", &ctx),
            "SELECT status FROM orders WHERE id = '42'"
        );
    }

    /// A missing variable must not corrupt SQL: no swallowing of the closing brace, no duplicated prefix producing a stray `$`.
    #[test]
    fn missing_placeholder_does_not_corrupt_sql() {
        let ctx = ctx_with(&[]);
        let out = interp_value("WHERE id = '{{missing}}'", &ctx);
        assert_eq!(out, "WHERE id = '${missing}'");
        assert!(!out.contains("WHERE id = 'WHERE"));
    }

    /// An unclosed `${` keeps the remaining text once, without duplicating the prefix (historical bug).
    #[test]
    fn unclosed_token_keeps_remainder_once() {
        let mut vars = HashMap::new();
        vars.insert("x".to_string(), "1".to_string());
        assert_eq!(replace_tokens("a=${x", &vars), "a=${x");
        assert_eq!(replace_tokens("a=${x} b", &vars), "a=1 b");
        assert_eq!(replace_tokens("pre${oops", &vars), "pre${oops");
    }

    /// Redis assertions share `interp_value` with DB assertions: command / args / expected value must all interpolate correctly.
    /// This case runs the full `evaluate_checks` path, capturing the command args actually sent to the datasource.
    #[tokio::test]
    async fn redis_args_are_interpolated_end_to_end() {
        #[derive(Debug, Default)]
        struct MockProvider {
            captured: std::sync::Mutex<Vec<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl orbit_assertion::DataSourceProvider for MockProvider {
            async fn query_sql(
                &self,
                _ds: &str,
                _sql: &str,
            ) -> Result<orbit_assertion::types::QueryResult, String> {
                Ok(orbit_assertion::types::QueryResult::default())
            }

            async fn redis_command(&self, _ds: &str, args: &[String]) -> Result<String, String> {
                self.captured.lock().unwrap().push(args.to_vec());
                Ok("1".to_string())
            }
        }

        let provider = Arc::new(MockProvider::default());
        let mut ctx = ctx_with(&[("orderId", "42")]);
        let ds: Arc<dyn orbit_assertion::DataSourceProvider> = provider.clone();
        ctx.datasources = Some(ds);

        let check = Check {
            kind: orbit_config::CheckKind::Redis {
                datasource: "cache-redis".into(),
                command: "TTL".into(),
                args: vec!["order:{{orderId}}".into()],
                comparator: "equal".into(),
                expected: "1".into(),
                retry: None,
                extract_var: None,
                hard: true,
            },
            meta: None,
        };

        let (has_failure, results) = evaluate_checks(&[check], &ctx).await;
        assert!(!has_failure, "sample should pass: {results:?}");
        assert_eq!(results.len(), 1);
        assert_eq!(
            provider.captured.lock().unwrap()[0],
            vec!["TTL".to_string(), "order:42".to_string()],
            "Redis command args should have {{orderId}} interpolated"
        );
    }
}

#[cfg(test)]
mod action_tests {
    use super::*;
    use orbit_assertion::types::QueryResult;
    use orbit_codec::json::JsonCodec;
    use orbit_protocol::http::HttpClient;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Fake datasource: SQL returns empty first (simulating not-ready), then the target row; records the SQL received.
    #[derive(Debug, Default)]
    struct MockProvider {
        calls: AtomicUsize,
        sqls: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl orbit_assertion::DataSourceProvider for MockProvider {
        async fn query_sql(&self, _ds: &str, sql: &str) -> Result<QueryResult, String> {
            self.sqls.lock().unwrap().push(sql.to_string());
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Ok(QueryResult::default())
            } else {
                Ok(QueryResult {
                    columns: vec!["id".into(), "token".into(), "meta".into()],
                    rows: vec![vec!["42".into(), "T-9".into(), r#"{"tier":"gold"}"#.into()]],
                    rows_affected: 0,
                    elapsed_ms: 1,
                })
            }
        }
        async fn redis_command(&self, _ds: &str, _args: &[String]) -> Result<String, String> {
            Ok("redis-value".into())
        }
    }

    fn runtime(provider: Arc<MockProvider>) -> PipelineRuntime {
        let mut rt = PipelineRuntime::new(Box::new(HttpClient::new()), Box::new(JsonCodec));
        let ds: Arc<dyn orbit_assertion::DataSourceProvider> = provider;
        rt.with_datasources(Some(ds));
        rt
    }

    fn script(code: &str) -> PipelineAction {
        PipelineAction::Script {
            name: String::new(),
            code: code.to_string(),
        }
    }

    #[test]
    fn effective_actions_prefers_explicit_then_legacy() {
        let explicit = vec![script("void 0;")];
        assert_eq!(
            effective_actions(&explicit, &["legacy;".to_string()]).len(),
            1,
            "legacy scripts ignored when explicit actions are non-empty"
        );
        let merged = effective_actions(&[], &["a;".to_string(), "b;".to_string()]);
        assert_eq!(
            merged.len(),
            2,
            "legacy script fields are each wrapped as a script action"
        );
        assert!(effective_actions(&[], &["  ".to_string()]).is_empty());
    }

    #[test]
    fn config_actions_map_filters_disabled() {
        let actions = vec![
            orbit_config::RequestAction::Script {
                name: "s1".into(),
                enabled: true,
                language: None,
                code: "void 0;".into(),
            },
            orbit_config::RequestAction::Script {
                name: "disabled".into(),
                enabled: false,
                language: None,
                code: "boom;".into(),
            },
            orbit_config::RequestAction::Db {
                name: "q".into(),
                enabled: true,
                datasource: "db".into(),
                sql: Some("SELECT 1".into()),
                command: None,
                args: vec![],
                target: Some(orbit_config::DbTarget::Scalar),
                extract_var: Some("v".into()),
                columns: vec![],
                row: 0,
                retry: None,
            },
        ];
        let mapped = actions_to_pipeline(&actions, &[]);
        assert_eq!(mapped.len(), 2);
        assert!(matches!(mapped[1], PipelineAction::Db(_)));
    }

    #[tokio::test]
    async fn db_action_writes_scalar_and_columns_with_retry() {
        let provider = Arc::new(MockProvider::default());
        let rt = runtime(provider.clone());
        let spec = DbActionSpec {
            name: "lookup user".into(),
            datasource: "user-db".into(),
            sql: Some("SELECT id, token, meta FROM users WHERE id = '{{user_id}}'".into()),
            command: None,
            args: vec![],
            target: Some(AssertDbTarget::Scalar),
            extract_var: Some("dbId".into()),
            columns: vec![
                ("token".into(), "dbToken".into()),
                ("meta".into(), "dbMeta".into()),
            ],
            row: 0,
            retry: Some(RetryPolicy {
                interval_ms: 1,
                max_attempts: 5,
                timeout_ms: None,
            }),
        };
        let mut vars = HashMap::new();
        vars.insert("user_id".to_string(), "42".to_string());

        let ran = run_db_action(&rt, &spec, &vars).await;
        assert!(ran.ok, "should succeed after retry: {}", ran.detail);
        assert_eq!(ran.vars.get("dbId").unwrap(), "42");
        assert_eq!(ran.vars.get("dbToken").unwrap(), "T-9");
        assert_eq!(ran.vars.get("dbMeta").unwrap(), r#"{"tier":"gold"}"#);
        // The {{user_id}} in the SQL must be interpolated before it is sent
        let sqls = provider.sqls.lock().unwrap();
        assert!(
            sqls[1].contains("'42'"),
            "SQL not interpolated: {}",
            sqls[1]
        );
        assert!(
            !ran.detail.contains("T-9"),
            "summary should not leak data content"
        );
    }

    #[tokio::test]
    async fn db_action_without_datasource_fails_clearly() {
        let mut rt = PipelineRuntime::new(Box::new(HttpClient::new()), Box::new(JsonCodec));
        rt.with_datasources(None);
        let spec = DbActionSpec {
            name: String::new(),
            datasource: "db".into(),
            sql: Some("SELECT 1".into()),
            command: None,
            args: vec![],
            target: Some(AssertDbTarget::Scalar),
            extract_var: Some("v".into()),
            columns: vec![],
            row: 0,
            retry: None,
        };
        let ran = run_db_action(&rt, &spec, &HashMap::new()).await;
        assert!(!ran.ok);
        assert!(ran.detail.contains("datasource capability not injected"));
    }

    #[tokio::test]
    async fn db_action_redis_writes_variable() {
        let provider = Arc::new(MockProvider::default());
        let rt = runtime(provider);
        let spec = DbActionSpec {
            name: String::new(),
            datasource: "cache".into(),
            sql: None,
            command: Some("GET".into()),
            args: vec!["order:{{orderId}}".into()],
            target: None,
            extract_var: Some("cacheVal".into()),
            columns: vec![],
            row: 0,
            retry: None,
        };
        let mut vars = HashMap::new();
        vars.insert("orderId".to_string(), "7".to_string());
        let ran = run_db_action(&rt, &spec, &vars).await;
        assert!(ran.ok, "{}", ran.detail);
        assert_eq!(ran.vars.get("cacheVal").unwrap(), "redis-value");
    }

    /// End to end: DB action fetches data -> variable -> pre-script reads it and rewrites the request header -> send (target unreachable; only the rewrite result is checked).
    #[tokio::test]
    async fn pipeline_db_action_then_script_rewrites_request() {
        let provider = Arc::new(MockProvider::default());
        let mut rt = runtime(provider);
        let spec = PipelineSpec {
            protocol: "http".into(),
            // Port 1 always refuses connections: used for offline verification of pre-stage products (no real network request)
            target: "http://127.0.0.1:1/api".into(),
            operation: "GET".into(),
            pre_actions: vec![
                // Built-in interpolation node: subsequent actions act on the final payload (DB fetch + script rewrites headers)
                PipelineAction::Interpolate,
                PipelineAction::Db(DbActionSpec {
                    name: "fetch token".into(),
                    datasource: "user-db".into(),
                    sql: Some("SELECT token FROM users LIMIT 1".into()),
                    command: None,
                    args: vec![],
                    target: Some(AssertDbTarget::Cell {
                        row: 0,
                        column: "token".into(),
                    }),
                    extract_var: Some("dbToken".into()),
                    columns: vec![],
                    row: 0,
                    // mock returns empty first (simulating not-ready); relies on retry to get the data
                    retry: Some(RetryPolicy {
                        interval_ms: 1,
                        max_attempts: 3,
                        timeout_ms: None,
                    }),
                }),
                script(
                    "pm.request.headers.upsert({ key: 'X-Token', value: pm.variables.get('dbToken') });",
                ),
            ],
            ..Default::default()
        };

        let mut vars = HashMap::new();
        let env = HashMap::new();
        let cancel = CancellationToken::new();
        let outcome = execute_pipeline(&mut rt, spec, &mut vars, &env, &cancel, None).await;

        assert_eq!(outcome.pre_action_logs.len(), 3);
        assert_eq!(outcome.pre_action_logs[0].kind, "interpolate");
        assert!(outcome.pre_action_logs[1].ok);
        assert_eq!(outcome.pre_action_logs[1].kind, "db");
        assert_eq!(outcome.pre_action_logs[1].phase, "pre");
        assert!(outcome.pre_action_logs[2].ok);
        // The variable written by the DB action is read by the pre-script and used to rewrite the header
        assert_eq!(
            outcome.request.headers.get("X-Token").map(String::as_str),
            Some("T-9")
        );
        assert_eq!(
            outcome.action_vars.get("dbToken").map(String::as_str),
            Some("T-9")
        );
        // The variable space (for later steps to reference) also contains it
        assert_eq!(vars.get("dbToken").map(String::as_str), Some("T-9"));
        // Target unreachable -> protocol error; pre-action logs are still kept
        assert!(outcome.error.is_some());
    }
}
