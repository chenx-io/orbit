//! Bridge from AI tools -> execution engine (pure functions, no I/O).
//!
//! Responsibility: translate the domain model (`orbit_data::model`) into engine-executable forms --
//! the `PipelineSpec` arguments for a single request, and automation/load-test plan YAML.
//!
//! Two hard rules kept consistent with the frontend (aligned with `web/src/lib/resolve.ts` / `requestBody.ts`):
//! 1. **Pass variable placeholders through verbatim**: `{{var}}` / `{{$cat.method}}` are not substituted here,
//!    the engine interpolates them per request (`interpolate: true` or the top-level `variables` of the plan);
//! 2. Path/query parameters are **not URL-encoded when they contain a placeholder**; otherwise they are encoded per encodeURIComponent rules.

use std::collections::HashMap;

use orbit_data::model::{ApiRequest, BodyMode, HttpRequest, Scenario, ScenarioStep};
use serde_json::{json, Value};

// ─── URL / headers / body ───────────────────────────────

/// Whether it contains a variable placeholder (if so, do not encode; leave it to the engine to interpolate).
fn is_placeholder(s: &str) -> bool {
    s.contains("{{")
}

/// Equivalent to JS `encodeURIComponent` (keeps `A-Za-z0-9-_.!~*'()`).
pub fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            );
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn enc_if_needed(s: &str) -> String {
    if is_placeholder(s) {
        s.to_string()
    } else {
        encode_component(s)
    }
}

/// Assemble the final URL: `{pathParam}` substitution + appending of enabled query parameters (aligned with `buildRequestUrlRaw`).
pub fn resolve_request_url(req: &HttpRequest) -> String {
    let mut url = req.url.clone();
    for p in req
        .path_params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
    {
        if p.value.is_empty() {
            continue;
        }
        url = url.replace(&format!("{{{}}}", p.key), &enc_if_needed(&p.value));
    }
    let enabled: Vec<_> = req
        .query_params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .collect();
    if enabled.is_empty() {
        return url;
    }
    let qs = enabled
        .iter()
        .map(|p| format!("{}={}", enc_if_needed(&p.key), enc_if_needed(&p.value)))
        .collect::<Vec<_>>()
        .join("&");
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}{qs}")
}

/// Enabled request headers, plus auth headers injected per the `auth` config (aligned with the frontend send behavior).
pub fn resolve_request_headers(req: &HttpRequest) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    for h in req
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.is_empty())
    {
        out.insert(h.key.clone(), h.value.clone());
    }
    let auth = &req.auth;
    match auth.r#type.as_str() {
        "bearer" => {
            if let Some(token) = auth.token.as_deref().filter(|t| !t.is_empty()) {
                out.insert("Authorization".into(), format!("Bearer {token}"));
            }
        }
        "basic" => {
            let user = auth.username.clone().unwrap_or_default();
            let pass = auth.password.clone().unwrap_or_default();
            if !user.is_empty() || !pass.is_empty() {
                out.insert(
                    "Authorization".into(),
                    format!("Basic {}", base64_encode(&format!("{user}:{pass}"))),
                );
            }
        }
        "api-key" => {
            let key = auth.key.clone().unwrap_or_default();
            let value = auth.value.clone().unwrap_or_default();
            if !key.is_empty() && auth.add_to.as_deref() != Some("query") {
                out.insert(key, value);
            }
        }
        _ => {}
    }
    if !req.content_type.trim().is_empty() && req.body_mode != BodyMode::None {
        out.entry("Content-Type".to_string())
            .or_insert_with(|| req.content_type.clone());
    }
    out
}

/// Get the text actually sent according to the body mode (aligned with `getActiveBody`).
pub fn active_body(req: &HttpRequest) -> String {
    let mode_key = body_mode_key(req.body_mode);
    if let Some(by_mode) = &req.body_by_mode {
        if let Some(slot) = by_mode.get(mode_key) {
            return slot.clone();
        }
    }
    if req.body_mode == BodyMode::Json {
        return req.body.clone();
    }
    String::new()
}

/// Body mode -> key of `bodyByMode` (consistent with the frontend bodyMode values, kebab-case).
pub fn body_mode_key(mode: BodyMode) -> &'static str {
    match mode {
        BodyMode::None => "none",
        BodyMode::Json => "json",
        BodyMode::Xml => "xml",
        BodyMode::FormData => "form-data",
        BodyMode::XWwwFormUrlencoded => "x-www-form-urlencoded",
        BodyMode::Raw => "raw",
        BodyMode::Binary => "binary",
    }
}

/// Body bytes + an extra Content-Type to add (the boundary for form-data).
pub fn resolve_request_body(req: &HttpRequest) -> Result<(Vec<u8>, Option<String>), String> {
    match req.body_mode {
        BodyMode::None => Ok((Vec::new(), None)),
        BodyMode::Json | BodyMode::Xml | BodyMode::Raw => {
            Ok((active_body(req).into_bytes(), None))
        }
        BodyMode::XWwwFormUrlencoded => {
            let body = req
                .form_params
                .iter()
                .filter(|p| p.enabled && !p.key.is_empty())
                .map(|p| format!("{}={}", enc_if_needed(&p.key), enc_if_needed(&p.value)))
                .collect::<Vec<_>>()
                .join("&");
            Ok((body.into_bytes(), None))
        }
        BodyMode::FormData => {
            let params: Vec<crate::commands::proxy::FormParamEntry> = req
                .form_params
                .iter()
                .filter(|p| p.enabled && !p.key.is_empty())
                .map(|p| crate::commands::proxy::FormParamEntry {
                    key: p.key.clone(),
                    value: Some(p.value.clone()),
                    file_path: None,
                    file_type: None,
                    filename: None,
                })
                .collect();
            if params.is_empty() {
                return Err("form-data body has no enabled fields".into());
            }
            let (bytes, ct) = crate::commands::proxy::build_multipart_from_paths(&params)?;
            Ok((bytes, Some(ct)))
        }
        BodyMode::Binary => Err(
            "AI run does not yet support a binary (file upload) body; please send manually in the interface module".to_string(),
        ),
    }
}

fn base64_encode(s: &str) -> String {
    use base64::prelude::*;
    BASE64_STANDARD.encode(s.as_bytes())
}

// ─── Plan YAML construction ─────────────────────────────────────

/// Convert an HTTP request into the `request:` node of a plan YAML.
pub fn request_node(req: &HttpRequest) -> Value {
    let mut node = json!({
        "method": if req.method.trim().is_empty() { "GET" } else { &req.method },
        "url": resolve_request_url(req),
    });
    let headers = resolve_request_headers(req);
    if !headers.is_empty() {
        node["headers"] = serde_json::to_value(headers).unwrap_or(Value::Null);
    }
    let body = active_body(req);
    if !body.is_empty() {
        node["body"] = json!(body);
    }
    if let Some(f) = &req.request_format {
        node["request_format"] = json!(f);
    }
    if let Some(f) = &req.response_format {
        node["response_format"] = json!(f);
    }
    node
}

/// Scenario step -> YAML node; unsupported steps return `Err` (to avoid false negatives from silent skips).
pub fn step_node(
    step: &ScenarioStep,
    requests: &HashMap<String, ApiRequest>,
) -> Result<Option<Value>, String> {
    let (base_id, base_name, disabled) = match step {
        ScenarioStep::Request(s) => (&s.base.id, &s.base.name, s.base.disabled),
        ScenarioStep::Loop(s) => (&s.base.id, &s.base.name, s.base.disabled),
        ScenarioStep::Condition(s) => (&s.base.id, &s.base.name, s.base.disabled),
        ScenarioStep::Wait(s) => (&s.base.id, &s.base.name, s.base.disabled),
        ScenarioStep::Group(s) => (&s.base.id, &s.base.name, s.base.disabled),
        ScenarioStep::Setvar(s) => (&s.base.id, &s.base.name, s.base.disabled),
    };
    let _ = base_id; // id is only used for UI references; the plan YAML does not need it
    if disabled.unwrap_or(false) {
        return Ok(None);
    }
    let mut node = json!({ "name": base_name });

    match step {
        ScenarioStep::Request(s) => {
            let Some(request_id) = s.request_id.as_deref().filter(|r| !r.is_empty()) else {
                return Err(format!(
                    "Step \"{}\" is not bound to an endpoint",
                    base_name
                ));
            };
            let Some(req) = requests.get(request_id) else {
                return Err(format!(
                    "The endpoint {request_id} referenced by step \"{base_name}\" does not exist (deleted?)"
                ));
            };
            let ApiRequest::Http(http) = req else {
                return Err(format!(
                    "Step \"{base_name}\" uses a non-HTTP protocol ({}); the AI direct run only supports HTTP for now, please run this case in the Automation module.",
                    req.protocol()
                ));
            };
            node["type"] = json!("request");
            node["request"] = request_node(http);
            if let Some(script) = http
                .prereq_script
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                node["pre_script"] = json!(script);
            }
            if let Some(script) = http
                .postreq_script
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                node["post_script"] = json!(script);
            }
            // Pre/post action lists: isomorphic to the output of the frontend `yaml.ts`.
            // Pre is a **single ordered list** (including the built-in interpolation node `{type: interpolate}`):
            // The previous "pre-interpolation actions" field is merged before the anchor; when the action list is non-empty the engine prefers it
            // and ignores the same-named legacy single-script field (`normalize_actions`).
            let pre_actions = orbit_config::merge_pre_actions(
                &http.pre_actions,
                None,
                &http.pre_resolve_actions,
                None,
            );
            for (field, actions) in [
                ("pre_actions", &pre_actions),
                ("post_actions", &http.post_actions),
            ] {
                let enabled: Vec<&orbit_config::RequestAction> =
                    actions.iter().filter(|a| a.is_enabled()).collect();
                if !enabled.is_empty() {
                    node[field] = serde_json::to_value(enabled).unwrap_or(Value::Null);
                }
            }
            if let (Some(var), Some(path)) = (
                s.extract_var.as_deref().filter(|v| !v.is_empty()),
                s.extract_path.as_deref().filter(|p| !p.is_empty()),
            ) {
                node["extract"] = json!([extract_node(var, s.extract_type.as_deref(), path)?]);
            }
        }
        ScenarioStep::Loop(s) => {
            let children = collect_children(&s.children, requests)?;
            node["type"] = json!("loop");
            node["count"] = json!(s.count.max(1));
            node["steps"] = Value::Array(children);
        }
        ScenarioStep::Condition(s) => {
            node["type"] = json!("condition");
            node["expression"] = json!(if s.expr.trim().is_empty() {
                "true"
            } else {
                &s.expr
            });
            node["then"] = Value::Array(collect_children(&s.children, requests)?);
            let else_children = collect_children(&s.else_children, requests)?;
            if !else_children.is_empty() {
                node["else"] = Value::Array(else_children);
            }
        }
        ScenarioStep::Wait(s) => {
            node["type"] = json!("wait");
            node["duration"] = json!(format_duration_ms(s.ms));
        }
        ScenarioStep::Group(s) => {
            node["type"] = json!("group");
            node["steps"] = Value::Array(collect_children(&s.children, requests)?);
        }
        ScenarioStep::Setvar(s) => {
            node["type"] = json!("setvar");
            node["key"] = json!(s.var_key.clone().unwrap_or_else(|| "var".into()));
            node["value"] = json!(s.var_value.clone().unwrap_or_default());
        }
    }
    Ok(Some(node))
}

fn collect_children(
    steps: &[ScenarioStep],
    requests: &HashMap<String, ApiRequest>,
) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for s in steps {
        if let Some(node) = step_node(s, requests)? {
            out.push(node);
        }
    }
    Ok(out)
}

/// Build the engine's extraction node.
///
/// Note: the engine's `Extraction` is `{ name: <variable name>, #[flatten] source }`, while
/// the `header` / `cookie` sources also use the `name` field to hold the header name, which conflicts --
/// this source cannot be expressed under the current model, so we fail explicitly instead of producing a broken plan.
fn extract_node(var: &str, kind: Option<&str>, path: &str) -> Result<Value, String> {
    match kind.unwrap_or("jsonpath") {
        "jmespath" => Ok(json!({ "name": var, "from": "jmespath", "expression": path })),
        "regex" => Ok(json!({ "name": var, "from": "regex", "pattern": path, "group": 1 })),
        "header" | "cookie" => Err(format!(
            "The variable extraction source of the step is {kind:?}, which the plan model does not yet support (extraction name and variable name share a field); \
             please extract via pm.response.headers in the endpoint's post-response script."
        )),
        _ => Ok(json!({ "name": var, "from": "jsonpath", "path": path })),
    }
}

/// Milliseconds -> engine duration text (`1s` / `500ms`).
pub fn format_duration_ms(ms: u64) -> String {
    if ms >= 1000 && ms % 1000 == 0 {
        format!("{}s", ms / 1000)
    } else if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

/// Build a sequential-execution plan YAML from a case (isomorphic to the frontend `buildScenarioYaml`).
pub fn build_scenario_plan_yaml(
    scenario: &Scenario,
    requests: &HashMap<String, ApiRequest>,
    extra_vars: &HashMap<String, String>,
) -> Result<String, String> {
    let mut steps = Vec::new();
    for s in &scenario.steps {
        if let Some(node) = step_node(s, requests)? {
            steps.push(node);
        }
    }
    if steps.is_empty() {
        return Err("this case has no executable steps".into());
    }
    let mut plan = json!({
        "name": scenario.name,
        "scenarios": [{
            "name": scenario.name,
            "executor": { "type": "sequential", "iterations": scenario.iterations.unwrap_or(1).max(1) },
            "on_error": match scenario.on_error.as_deref() { Some("continue") => "continue", _ => "stop" },
            "steps": steps,
        }]
    });
    if !extra_vars.is_empty() {
        plan["variables"] = serde_json::to_value(extra_vars).unwrap_or(Value::Null);
    }
    serde_yaml::to_string(&plan).map_err(|e| format!("Failed to generate case plan: {e}"))
}

/// Build a load-test plan YAML from a single HTTP request (isomorphic to the frontend `buildLoadYaml`, constant concurrency).
pub fn build_load_plan_yaml(
    request: &HttpRequest,
    vus: u32,
    duration_secs: u64,
    ramp_up_secs: u64,
) -> Result<String, String> {
    let duration = format_duration_ms(duration_secs.saturating_mul(1000));
    let ramp_up = format_duration_ms(ramp_up_secs.saturating_mul(1000));
    let plan = json!({
        "name": "AI Load Test",
        "scenarios": [{
            "name": if request.name.is_empty() { "request" } else { &request.name },
            "executor": {
                "type": "constant-vus",
                "vus": vus,
                "duration": duration,
                "ramp_up": ramp_up,
            },
            "steps": [{ "type": "request", "request": request_node(request) }],
        }]
    });
    serde_yaml::to_string(&plan).map_err(|e| format!("Failed to generate load-test plan: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_data::model::{AuthConfig, KeyValue, ScenarioStep, StepBase, StepRequest, StepWait};

    fn kv(key: &str, value: &str) -> KeyValue {
        KeyValue {
            id: format!("kv-{key}"),
            key: key.into(),
            value: value.into(),
            enabled: true,
            ..Default::default()
        }
    }

    fn http_req() -> HttpRequest {
        HttpRequest {
            id: "r1".into(),
            name: "Login".into(),
            method: "POST".into(),
            url: "https://api.example.com/users/{id}".into(),
            headers: vec![kv("X-Trace", "abc")],
            query_params: vec![kv("page", "1")],
            path_params: vec![kv("id", "42")],
            body: "{\"u\":\"{{user}}\"}".into(),
            body_mode: BodyMode::Json,
            content_type: "application/json".into(),
            auth: AuthConfig {
                r#type: "bearer".into(),
                token: Some("{{token}}".into()),
                ..Default::default()
            },
            prereq_script: Some("pm.environment.set('a','1')".into()),
            postreq_script: Some("pm.test('ok',()=>{})".into()),
            ..Default::default()
        }
    }

    #[test]
    fn encodes_query_and_path_params_but_keeps_placeholders() {
        let mut req = http_req();
        req.path_params = vec![kv("id", "a b"), kv("keep", "{{x}}")];
        req.url = "https://api.example.com/users/{id}?keep={keep}".into();
        req.query_params = vec![kv("q", "hello world")];
        let url = resolve_request_url(&req);
        assert!(url.contains("/users/a%20b"), "{url}");
        assert!(
            url.contains("keep={{x}}"),
            "placeholder should not be encoded: {url}"
        );
        assert!(url.contains("q=hello%20world"), "{url}");
    }

    #[test]
    fn injects_bearer_auth_header_without_resolving_placeholder() {
        let headers = resolve_request_headers(&http_req());
        assert_eq!(headers.get("Authorization").unwrap(), "Bearer {{token}}");
        assert_eq!(headers.get("X-Trace").unwrap(), "abc");
        assert_eq!(headers.get("Content-Type").unwrap(), "application/json");
    }

    #[test]
    fn basic_auth_is_base64_encoded() {
        let mut req = http_req();
        req.auth = AuthConfig {
            r#type: "basic".into(),
            username: Some("u".into()),
            password: Some("p".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_request_headers(&req).get("Authorization").unwrap(),
            "Basic dTpw"
        );
    }

    #[test]
    fn body_uses_mode_slot_then_falls_back_to_body() {
        let mut req = http_req();
        assert_eq!(active_body(&req), "{\"u\":\"{{user}}\"}");
        req.body_by_mode = Some(HashMap::from([("json".to_string(), "edited".to_string())]));
        assert_eq!(active_body(&req), "edited");
        req.body_mode = BodyMode::Raw;
        assert_eq!(
            active_body(&req),
            "",
            "raw mode with no slot should not fall back to body"
        );
    }

    #[test]
    fn form_urlencoded_body_is_built_from_form_params() {
        let mut req = http_req();
        req.body_mode = BodyMode::XWwwFormUrlencoded;
        req.form_params = vec![kv("a", "1"), kv("b", "x y")];
        let (body, ct) = resolve_request_body(&req).unwrap();
        assert_eq!(String::from_utf8_lossy(&body), "a=1&b=x%20y");
        assert!(ct.is_none());
    }

    #[test]
    fn binary_body_reports_unsupported() {
        let mut req = http_req();
        req.body_mode = BodyMode::Binary;
        assert!(resolve_request_body(&req).is_err());
    }

    #[test]
    fn scenario_yaml_parses_with_engine() {
        let reqs = HashMap::from([("r1".to_string(), ApiRequest::Http(http_req()))]);
        let scenario = Scenario {
            id: "s1".into(),
            name: "Login flow".into(),
            workspace_id: "ws-not-used".into(),
            description: None,
            steps: vec![
                ScenarioStep::Request(StepRequest {
                    base: StepBase {
                        id: "st1".into(),
                        name: "Login".into(),
                        disabled: None,
                    },
                    request_id: Some("r1".into()),
                    extract_path: Some("$.data.token".into()),
                    extract_var: Some("token".into()),
                    extract_type: Some("jsonpath".into()),
                }),
                ScenarioStep::Wait(StepWait {
                    base: StepBase {
                        id: "st2".into(),
                        name: "Wait".into(),
                        disabled: None,
                    },
                    ms: 1500,
                }),
            ],
            folder_id: None,
            priority: None,
            env_id: None,
            data_set_id: None,
            use_data_set: None,
            iterations: Some(2),
            on_error: Some("continue".into()),
            record_request_details: None,
        };
        let yaml = build_scenario_plan_yaml(
            &scenario,
            &reqs,
            &HashMap::from([("user".to_string(), "admin".to_string())]),
        )
        .unwrap();
        let plan = orbit_config::from_str(&yaml)
            .expect("engine must be able to parse the AI-generated case plan");
        assert_eq!(plan.scenarios.len(), 1);
        assert_eq!(
            plan.variables.get("user").map(String::as_str),
            Some("admin")
        );
    }

    #[test]
    fn scenario_with_non_http_step_is_rejected_with_guidance() {
        let reqs = HashMap::from([(
            "w1".to_string(),
            ApiRequest::Ws(orbit_data::model::WsRequest {
                id: "w1".into(),
                name: "ws".into(),
                protocol: "websocket".into(),
                url: "ws://x".into(),
                headers: vec![],
                messages: vec![],
                close_after: None,
                prereq_script: None,
                postreq_script: None,
                pre_resolve_actions: vec![],
                pre_actions: vec![],
                post_actions: vec![],
            }),
        )]);
        let scenario = Scenario {
            id: "s1".into(),
            name: "ws flow".into(),
            workspace_id: "ws".into(),
            description: None,
            steps: vec![ScenarioStep::Request(StepRequest {
                base: StepBase {
                    id: "st1".into(),
                    name: "Handshake".into(),
                    disabled: None,
                },
                request_id: Some("w1".into()),
                extract_path: None,
                extract_var: None,
                extract_type: None,
            })],
            folder_id: None,
            priority: None,
            env_id: None,
            data_set_id: None,
            use_data_set: None,
            iterations: None,
            on_error: None,
            record_request_details: None,
        };
        let err = build_scenario_plan_yaml(&scenario, &reqs, &HashMap::new()).unwrap_err();
        assert!(err.contains("Automation module"), "{err}");
    }

    #[test]
    fn disabled_steps_are_skipped() {
        let reqs = HashMap::from([("r1".to_string(), ApiRequest::Http(http_req()))]);
        let scenario = Scenario {
            id: "s1".into(),
            name: "x".into(),
            workspace_id: "ws".into(),
            description: None,
            steps: vec![ScenarioStep::Request(StepRequest {
                base: StepBase {
                    id: "st1".into(),
                    name: "Skip".into(),
                    disabled: Some(true),
                },
                request_id: Some("r1".into()),
                extract_path: None,
                extract_var: None,
                extract_type: None,
            })],
            folder_id: None,
            priority: None,
            env_id: None,
            data_set_id: None,
            use_data_set: None,
            iterations: None,
            on_error: None,
            record_request_details: None,
        };
        assert!(build_scenario_plan_yaml(&scenario, &reqs, &HashMap::new()).is_err());
    }

    #[test]
    fn load_yaml_parses_with_engine_and_keeps_placeholders() {
        let yaml = build_load_plan_yaml(&http_req(), 20, 30, 5).unwrap();
        assert!(
            yaml.contains("{{token}}"),
            "placeholder must be preserved verbatim: {yaml}"
        );
        let plan =
            orbit_config::from_str(&yaml).expect("engine must be able to parse the load-test plan");
        assert_eq!(plan.scenarios.len(), 1);
    }

    #[test]
    fn duration_formatting_matches_engine_expectations() {
        assert_eq!(format_duration_ms(500), "500ms");
        assert_eq!(format_duration_ms(1000), "1s");
        assert_eq!(format_duration_ms(1500), "1.5s");
    }
}
