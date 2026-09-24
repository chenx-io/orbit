use base64::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct FormParamEntry {
    pub key: String,
    pub value: Option<String>,
    /// Real absolute path of the file under Tauri path mode (read from disk on send, not via base64)
    pub file_path: Option<String>,
    pub file_type: Option<String>,
    pub filename: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteRequest {
    pub method: String,
    /// Target address: used directly on the passthrough path; on the template path it is only a redundant copy of `request_template.url`
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: String,
    /// base64 encoding of a binary / multipart request body; takes precedence over `body` (fallback for browser / HTTP-API paths)
    pub body_binary: Option<String>,
    /// Body mode (none/json/xml/raw/binary/form-data/x-www-form-urlencoded),
    /// used by the Tauri path mode to decide whether to assemble the body by reading files from disk
    pub body_mode: Option<String>,
    /// Tauri path mode (binary): real absolute file path, read from disk directly on send
    pub binary_file_path: Option<String>,
    /// Tauri path mode (form-data): structured fields (files carry a real path), read from disk to build multipart on send
    pub form_params: Option<Vec<FormParamEntry>>,
    pub timeout: Option<u64>,
    /// Pre-request script (runs after variable resolution and before sending); can rewrite url/method/headers/body
    pub prereq_script: Option<String>,
    /// Post-response script (runs after the response is received); can read the response, write variables, collect assertions
    pub postreq_script: Option<String>,
    /// **Legacy read-in field (deprecated)**: the "pre-interpolation actions" list from the previous two-stage refactor.
    /// It is merged before the built-in interpolation node of `pre_actions` (see `merge_pre_actions`).
    #[serde(default)]
    pub pre_resolve_actions: Option<Vec<orbit_config::RequestAction>>,
    /// Pre-request action list (a single ordered list: scripts / database queries / built-in interpolation node; order is execution order)
    #[serde(default)]
    pub pre_actions: Option<Vec<orbit_config::RequestAction>>,
    /// Post-response action list (executed in order after the response); when non-empty it takes precedence over `postreq_script`
    #[serde(default)]
    pub post_actions: Option<Vec<orbit_config::RequestAction>>,
    /// Script library table (reusable action templates of the current workspace): used by the engine to expand `{ type: ref }` references.
    ///
    /// Sent by the frontend **together with the action list**: both share the same source, avoiding reads of a stale library table not yet persisted
    /// (the user may click Run right after editing a library item, and the snapshot may not have been written back yet).
    #[serde(default)]
    pub action_templates: Option<Vec<orbit_config::ActionTemplate>>,
    /// Snapshot of the current environment variables, for scripts to read via pm.environment.get
    pub env_vars: Option<HashMap<String, String>>,
    /// Assertion configuration (built-in + DB/Redis), evaluated after the response
    #[serde(default)]
    pub checks: Option<Vec<orbit_config::Check>>,
    /// Un-interpolated request template (unified interpolation for single HTTP sends).
    ///
    /// When provided, the engine takes over URL assembly (path/query encoding), header merging
    /// (Content-Type / Host / Content-Length) and body assembly (including multipart read from disk by path),
    /// The `url` / `headers` / `body*` fields above no longer participate; when `None`, the old semantics are kept
    /// of "the caller has already built the request" (used by non-HTTP protocols such as persistent connections).
    #[serde(default)]
    pub request_template: Option<orbit_engine::request_build::RequestTemplate>,
    /// Build only, do not send: return the final request right after running pre-interpolation actions / interpolation / encoding / post-interpolation actions.
    ///
    /// Lets a distributed agent resolve on the **control side** (the agent only sends the final message).
    #[serde(default)]
    pub dry_run: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct TimingInfo {
    pub dns: u64,
    pub connect: u64,
    pub tls: u64,
    pub ttfb: u64,
    pub download: u64,
}

/// Request snapshot after pre-request script rewriting (returned to the frontend for the "Request" tab and code generation to show what was actually sent).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSnapshot {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub duration: u64,
    pub size: u64,
    pub timing: Option<TimingInfo>,
    /// Request snapshot after pre-request script rewriting (method/url/headers/body); the frontend merges it into sentRequest for display
    pub request: Option<RequestSnapshot>,
    /// Pre-request script console logs
    pub pre_logs: Vec<orbit_js::ScriptLog>,
    /// Post-response script console logs
    pub post_logs: Vec<orbit_js::ScriptLog>,
    /// Post-response script assertion results
    pub post_tests: Vec<orbit_js::TestResult>,
    /// Variables written by scripts (pre/post); the caller decides whether to persist them to the environment
    pub vars_set: HashMap<String, String>,
    /// Temporary variables written by scripts via `pm.variables.set` (lifetime of this request), not persisted to disk
    pub temp_vars_set: HashMap<String, String>,
    /// Assertion results (built-in + DB/Redis), rendered uniformly by the frontend Tests tab
    pub assertions: Vec<serde_json::Value>,
    /// Pre-request action execution results (a single list, including the built-in interpolation node entry), rendered by the frontend in execution order
    pub pre_actions: Vec<orbit_engine::pipeline::ActionLog>,
    /// Post-response action execution results
    pub post_actions: Vec<orbit_engine::pipeline::ActionLog>,
    /// Variables written by actions (DB queries); the frontend may choose to persist them to the environment
    pub action_vars: HashMap<String, String>,
}

/// Detect protocol from URL scheme
fn detect_protocol(url: &str) -> &str {
    if url.starts_with("ws://") || url.starts_with("wss://") {
        "websocket"
    } else if url.starts_with("tcp://") {
        "tcp"
    } else if url.starts_with("udp://") {
        "udp"
    } else if url.starts_with("sse://") {
        "sse"
    } else {
        "http" // default: http/https + graphql (detected by operation)
    }
}

/// Detect GraphQL from operation name containing "query" or "mutation"
fn is_graphql(method: &str, body: &str) -> bool {
    method.eq_ignore_ascii_case("GRAPHQL")
        || body.trim_start().starts_with("query")
        || body.trim_start().starts_with("mutation")
        || body.trim_start().starts_with("subscription")
}

#[tauri::command]
pub async fn execute_request(
    request: ExecuteRequest,
    // Shared Cookie Jar (session persistence): Set-Cookie responses accumulate automatically and are attached to subsequent requests to the same domain
    cookie_jar: tauri::State<'_, Arc<tokio::sync::Mutex<orbit_engine::cookie_jar::CookieJar>>>,
    // Data source registry (for DB/Redis assertion queries)
    data_sources: tauri::State<'_, Arc<orbit_datasource::DataSourceRegistry>>,
) -> Result<ProxyResponse, String> {
    let timeout = request.timeout.unwrap_or(30_000);
    let env_vars = request.env_vars.clone().unwrap_or_default();

    // Unified execution pipeline (same source as the HTTP API / load-test flow_runner).
    //
    // Two paths:
    // - **Template path** (`request_template` present, single HTTP send): the request is not yet interpolated/assembled, so the engine
    //   takes over URL assembly (path/query encoding), header merging (Content-Type/Host/Content-Length)
    //   and body assembly (including multipart read from disk by path), and runs a script segment before/after interpolation;
    // - **Passthrough path** (non-HTTP protocols such as persistent connections / legacy calls): the caller has already built the request, so only scripts run.
    let template = request.request_template.clone();
    let body_for_probe = template
        .as_ref()
        .and_then(|t| t.text_body())
        .unwrap_or(request.body.as_str());
    let protocol = if is_graphql(&request.method, body_for_probe) {
        "graphql"
    } else {
        detect_protocol(
            template
                .as_ref()
                .map(|t| t.url.as_str())
                .unwrap_or(&request.url),
        )
    };

    let (target, headers, payload) = match &template {
        Some(t) => (t.url.clone(), HashMap::new(), Vec::new()),
        None => {
            // Binary / multipart body assembly (Tauri path mode: file read from disk / multipart / base64 / text)
            let mut extra_headers = HashMap::new();
            let payload: Vec<u8> = match (
                &request.body_mode,
                &request.binary_file_path,
                &request.form_params,
            ) {
                (Some(mode), Some(path), _) if mode == "binary" && !path.is_empty() => {
                    std::fs::read(path)
                        .map_err(|e| format!("Failed to read file {}: {}", path, e))?
                }
                (Some(mode), _, Some(params)) if mode == "form-data" => {
                    let (bytes, ct) = build_multipart_from_paths(params)?;
                    extra_headers.insert("Content-Type".to_string(), ct);
                    bytes
                }
                _ => match &request.body_binary {
                    Some(b) => BASE64_STANDARD
                        .decode(b)
                        .ok()
                        .unwrap_or_else(|| request.body.as_bytes().to_vec()),
                    None => request.body.as_bytes().to_vec(),
                },
            };
            let mut headers = request.headers.clone();
            headers.extend(extra_headers);
            (request.url.clone(), headers, payload)
        }
    };

    // Script library table: the engine uses it to expand `{ type: ref }` when mapping actions; a missing library item degrades to an error log
    let library = request.action_templates.as_deref().unwrap_or_default();
    let spec = orbit_engine::pipeline::PipelineSpec {
        protocol: protocol.to_string(),
        target,
        operation: request.method.clone(),
        headers,
        body: payload,
        timeout: Some(std::time::Duration::from_millis(timeout)),
        pre_scripts: request
            .prereq_script
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .into_iter()
            .collect(),
        post_scripts: request
            .postreq_script
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .into_iter()
            .collect(),
        // Pre-request actions: a single ordered list (including the built-in interpolation node); the previous "pre-interpolation actions" are merged before the anchor.
        // When the list has no anchor the engine adds one at the **very front** (existing actions still run after interpolation).
        pre_actions: orbit_engine::pipeline::actions_to_pipeline(
            &orbit_config::merge_pre_actions(
                request.pre_actions.as_deref().unwrap_or_default(),
                None,
                request.pre_resolve_actions.as_deref().unwrap_or_default(),
                None,
            ),
            library,
        ),
        post_actions: orbit_engine::pipeline::actions_to_pipeline(
            request.post_actions.as_deref().unwrap_or_default(),
            library,
        ),
        checks: request.checks.clone().unwrap_or_default(),
        // With a template the request is necessarily un-interpolated (template invariant), so the engine interpolates it
        interpolate: template.is_some(),
        request_template: template,
        dry_run: request.dry_run.unwrap_or(false),
        ..Default::default()
    };

    let mut rt = orbit_engine::pipeline::PipelineRuntime::new(
        Box::new(orbit_protocol::http::HttpClient::new()),
        Box::new(orbit_codec::json::JsonCodec),
    );
    // Inject the data source registry: post-response DB/Redis assertions query through it
    rt.with_datasources(Some(data_sources.inner().clone()));
    let mut vars: HashMap<String, String> = HashMap::new();
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut jar = cookie_jar.lock().await;
    let outcome = orbit_engine::pipeline::execute_pipeline(
        &mut rt,
        spec,
        &mut vars,
        &env_vars,
        &cancel,
        Some(&mut jar),
    )
    .await;

    // Build only, do not send: return the final request (`status = 0` means not sent), for the agent path to resolve on the control side
    if request.dry_run.unwrap_or(false) {
        let built = outcome.request;
        return Ok(ProxyResponse {
            status: 0,
            status_text: String::new(),
            headers: HashMap::new(),
            body: String::new(),
            duration: 0,
            size: 0,
            timing: None,
            request: Some(RequestSnapshot {
                method: built.operation,
                url: built.target,
                headers: built.headers,
                body: String::from_utf8_lossy(&built.payload).to_string(),
            }),
            pre_logs: outcome.pre_logs,
            post_logs: Vec::new(),
            post_tests: Vec::new(),
            vars_set: outcome.vars_set,
            temp_vars_set: outcome.temp_vars_set,
            assertions: Vec::new(),
            pre_actions: outcome.pre_action_logs,
            post_actions: Vec::new(),
            action_vars: outcome.action_vars,
        });
    }

    let Some(response) = &outcome.response else {
        let msg = outcome
            .error
            .as_ref()
            .map(|e| e.message())
            .unwrap_or_else(|| "request failed".into());
        return Err(format!("Request execution failed: {}", msg));
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

    Ok(ProxyResponse {
        status,
        status_text: http_status_text(status),
        headers,
        body: body_str,
        duration,
        size,
        timing: Some(timing),
        request: Some(RequestSnapshot {
            method: outcome.request.operation,
            url: outcome.request.target,
            headers: outcome.request.headers,
            body: String::from_utf8_lossy(&outcome.request.payload).to_string(),
        }),
        pre_logs: outcome.pre_logs,
        post_logs: outcome.post_logs,
        post_tests: outcome.post_tests,
        vars_set: outcome.vars_set,
        temp_vars_set: outcome.temp_vars_set,
        assertions: outcome
            .tests
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "passed": t.passed,
                    "message": t.message,
                    "isHard": t.is_hard,
                    "exportedVars": t.exported_vars,
                })
            })
            .collect(),
        pre_actions: outcome.pre_action_logs,
        post_actions: outcome.post_action_logs,
        action_vars: outcome.action_vars,
    })
}

fn http_status_text(code: u16) -> String {
    match code {
        200 => "OK".into(),
        201 => "Created".into(),
        204 => "No Content".into(),
        301 => "Moved Permanently".into(),
        302 => "Found".into(),
        304 => "Not Modified".into(),
        400 => "Bad Request".into(),
        401 => "Unauthorized".into(),
        403 => "Forbidden".into(),
        404 => "Not Found".into(),
        405 => "Method Not Allowed".into(),
        409 => "Conflict".into(),
        422 => "Unprocessable Entity".into(),
        429 => "Too Many Requests".into(),
        500 => "Internal Server Error".into(),
        502 => "Bad Gateway".into(),
        503 => "Service Unavailable".into(),
        _ => String::new(),
    }
}

/// Escape double quotes in a multipart Content-Disposition
fn escape_disp(s: &str) -> String {
    s.replace('"', "\\\"")
}

/// Read files by their real paths and assemble a multipart/form-data byte stream (returns the bytes and the Content-Type).
/// Text fields write their value directly; file fields are read from disk by `file_path` and carry the original filename and MIME.
///
/// Also reused by the AI tool host (`form-data` body, text fields only).
pub(crate) fn build_multipart_from_paths(
    params: &[FormParamEntry],
) -> Result<(Vec<u8>, String), String> {
    let boundary = format!("----orbitFormBoundary{}", uuid::Uuid::new_v4().simple());
    let mut out: Vec<u8> = Vec::new();
    let crlf = b"\r\n";

    for p in params {
        if p.key.is_empty() {
            continue;
        }
        if let Some(path) = &p.file_path {
            let bytes =
                std::fs::read(path).map_err(|e| format!("Failed to read file {}: {}", path, e))?;
            let fname = p.filename.clone().unwrap_or_else(|| {
                std::path::Path::new(path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
            let ctype = p
                .file_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_string());

            out.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            out.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                    escape_disp(&p.key),
                    escape_disp(&fname)
                )
                .as_bytes(),
            );
            out.extend_from_slice(format!("Content-Type: {}\r\n\r\n", ctype).as_bytes());
            out.extend_from_slice(&bytes);
            out.extend_from_slice(crlf);
        } else {
            let val = p.value.clone().unwrap_or_default();
            out.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            out.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                    escape_disp(&p.key)
                )
                .as_bytes(),
            );
            out.extend_from_slice(val.as_bytes());
            out.extend_from_slice(crlf);
        }
    }
    out.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    let ct = format!("multipart/form-data; boundary={}", boundary);
    Ok((out, ct))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wire format: the **template-state** request sent by the frontend (snake_case, body template carrying a `mode` tag) must parse.
    ///
    /// This is the outward contract of the Tauri command -- if a field name does not match, serde silently ignores it (falling back to the default),
    /// which shows up as "the script/template did not take effect but the pipeline looks fine".
    #[test]
    fn execute_request_parses_template_payload() {
        let json = r#"{
            "method": "POST",
            "url": "https://api.example.com/{{ver}}/items/{id}",
            "headers": {},
            "body": "",
            "timeout": 30000,
            "pre_resolve_actions": [{ "type": "script", "code": "pm.environment.set('ver','v1');" }],
            "pre_actions": [{ "type": "script", "code": "pm.request.headers.upsert({key:'X-S',value:'1'});" }],
            "env_vars": { "token": "t" },
            "request_template": {
                "url": "https://api.example.com/{{ver}}/items/{id}",
                "path_params": [["id", "{{itemId}}"]],
                "query_params": [["q", "{{kw}}"]],
                "default_headers": [["Accept", "*/*"]],
                "auth_headers": [["Authorization", "Bearer {{token}}"]],
                "user_headers": [],
                "cookies": [["sid", "{{sid}}"]],
                "body": { "mode": "text", "format": "json", "text": "{\"n\":\"{{name}}\"}" }
            }
        }"#;
        let req: ExecuteRequest =
            serde_json::from_str(json).expect("template-state request should parse");
        let tpl = req
            .request_template
            .as_ref()
            .expect("template should exist");
        assert_eq!(
            tpl.path_params,
            vec![("id".to_string(), "{{itemId}}".to_string())]
        );
        assert_eq!(tpl.query_params.len(), 1);
        assert_eq!(tpl.cookies.len(), 1);
        // The body template is kept verbatim (interpolation is done by the engine during the interpolation stage)
        assert_eq!(tpl.text_body(), Some(r#"{"n":"{{name}}"}"#));
        // The previous "pre-interpolation actions" field still parses (legacy read-in, merged into the single list)
        assert_eq!(req.pre_resolve_actions.as_ref().unwrap().len(), 1);
        assert_eq!(req.pre_actions.as_ref().unwrap().len(), 1);
        assert!(req.post_actions.is_none());
        // After merging: legacy actions -> built-in anchor -> original actions of the single list
        let merged = orbit_config::merge_pre_actions(
            req.pre_actions.as_deref().unwrap_or_default(),
            None,
            req.pre_resolve_actions.as_deref().unwrap_or_default(),
            None,
        );
        assert_eq!(merged.len(), 3);
        assert!(merged[1].is_interpolate());
    }

    /// Script library wire format: the inner action of a library item must be in the `type`-tagged form.
    ///
    /// In the frontend store a library item action is in the `kind`-tagged form (`{ kind: "script", code }`); before sending it must
    /// go through `toWireAction` -- skipping this step makes the whole `execute_request` fail to deserialize
    /// (`missing field 'type'`), which shows up as "as long as there is any library item, no request can be sent".
    #[test]
    fn execute_request_parses_library_payload() {
        let json = r#"{
            "method": "GET",
            "url": "http://127.0.0.1:1/x",
            "headers": {},
            "body": "",
            "body_binary": null,
            "pre_actions": [
                { "type": "ref", "library_id": "tpl-sign", "enabled": true },
                { "type": "interpolate" }
            ],
            "action_templates": [
                {
                    "id": "tpl-sign",
                    "name": "Compute signature",
                    "description": "HMAC written to X-Sign",
                    "sortIndex": 0,
                    "workspaceId": "ws-default",
                    "action": { "type": "script", "enabled": true, "code": "sign();" }
                }
            ]
        }"#;
        let req: ExecuteRequest =
            serde_json::from_str(json).expect("script library wire format should parse");
        let templates = req.action_templates.expect("library table should parse");
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].name, "Compute signature");
        assert!(
            templates[0].is_valid(),
            "script library item content should be valid"
        );
        // Unknown fields (the frontend workspaceId badge) must not affect parsing, and ref items stay in ref form for the engine to expand
        assert_eq!(templates[0].sort_index, 0);
        let actions = req.pre_actions.expect("pre-request actions should parse");
        assert!(actions[0].is_ref(), "a ref item must be in ref form");
        assert!(actions[1].is_interpolate());
    }

    /// Existing wire format (no template, only the old single script) must keep parsing (zero migration).
    #[test]
    fn execute_request_parses_legacy_payload_without_template() {
        let json = r#"{
            "method": "GET",
            "url": "http://127.0.0.1:1/x",
            "headers": { "Accept": "*/*" },
            "body": "",
            "body_binary": null,
            "prereq_script": "void 0;"
        }"#;
        let req: ExecuteRequest =
            serde_json::from_str(json).expect("existing request should parse");
        assert!(req.request_template.is_none());
        assert!(req.pre_resolve_actions.is_none());
        assert_eq!(req.prereq_script.as_deref(), Some("void 0;"));
    }
}
