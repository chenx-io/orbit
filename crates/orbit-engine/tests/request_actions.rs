//! End-to-end test of the pre/post "action list": parse `examples/request-actions.yaml` ->
//! a database action writes variables -> a pre-script reads them and rewrites the request -> send.
//!
//! Uses a fake data source + an unreachable target address and stays fully offline (no real network requests).
//!
//! Pre-actions form a **single ordered list**; the built-in "interpolate" node ([`PipelineAction::Interpolate`])
//! divides the execution timing: before the node = pre-interpolation (may write variables consumed by this interpolation, rewrite the template),
//! after the node = post-interpolation (a rewrite is the final bytes, no second interpolation).

use std::collections::HashMap;
use std::sync::Arc;

use orbit_assertion::types::QueryResult;
use orbit_assertion::DataSourceProvider;
use orbit_codec::json::JsonCodec;
use orbit_config::{RequestAction, Step};
use orbit_engine::pipeline::{
    actions_to_pipeline, execute_pipeline, PipelineAction, PipelineOutcome, PipelineRuntime,
    PipelineSpec,
};
use orbit_protocol::http::HttpClient;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
struct MockProvider;

#[async_trait::async_trait]
impl DataSourceProvider for MockProvider {
    async fn query_sql(&self, _ds: &str, sql: &str) -> Result<QueryResult, String> {
        assert!(
            sql.contains("'1'"),
            "SQL should be sent after interpolating {{{{user_id}}}}, got: {sql}"
        );
        Ok(QueryResult {
            columns: vec!["id".into(), "token".into()],
            rows: vec![vec!["1".into(), "tk-abc".into()]],
            rows_affected: 0,
            elapsed_ms: 2,
        })
    }

    async fn redis_command(&self, _ds: &str, args: &[String]) -> Result<String, String> {
        Ok(format!(
            "redis:{}",
            args.first().cloned().unwrap_or_default()
        ))
    }
}

fn example_yaml() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/request-actions.yaml"
    );
    std::fs::read_to_string(path).expect("failed to read the example YAML")
}

/// Parse the example and take the first request step
fn first_step() -> Step {
    let plan = orbit_config::from_str(&example_yaml()).expect("failed to parse the example YAML");
    plan.scenarios[0].steps[0].clone()
}

/// Test runtime (HTTP + JSON codec)
fn rt_with_codec() -> PipelineRuntime {
    PipelineRuntime::new(Box::new(HttpClient::new()), Box::new(JsonCodec))
}

/// Run the pipeline once with the given pre-action list: the target is unreachable, so only the pre-stage artifacts are inspected.
///
/// The variable space always provides `ver = v2`, used by assertions like "pre-anchor script rewrites the template -> interpolation".
async fn run_pre_actions(actions: Vec<PipelineAction>) -> PipelineOutcome {
    let spec = PipelineSpec {
        protocol: "http".into(),
        target: "http://127.0.0.1:1/legacy".into(),
        operation: "POST".into(),
        pre_actions: actions,
        interpolate: true,
        ..Default::default()
    };
    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("ver".to_string(), "v2".to_string());
    let env = HashMap::new();
    let cancel = CancellationToken::new();
    execute_pipeline(&mut rt_with_codec(), spec, &mut vars, &env, &cancel, None).await
}

/// Script action constructor (test shorthand)
fn script(name: &str, code: &str) -> PipelineAction {
    PipelineAction::Script {
        name: name.into(),
        code: code.into(),
    }
}

#[test]
fn example_parses_into_single_ordered_list() {
    let step = first_step();
    let pre = step.pre_actions_normalized();
    // Single list: pre-interpolation script -> built-in interpolate node -> DB -> (disabled) DB -> script
    assert_eq!(pre.len(), 5, "the example should contain 5 pre items");
    assert!(matches!(pre[0], RequestAction::Script { .. }));
    assert!(pre[1].is_interpolate());
    assert!(matches!(pre[2], RequestAction::Db { .. }));
    assert!(matches!(pre[3], RequestAction::Db { .. }));
    assert!(matches!(pre[4], RequestAction::Script { .. }));
    // Disabled DB actions do not enter the runtime list (the built-in node is always enabled)
    assert!(!pre[3].is_enabled());
    assert_eq!(actions_to_pipeline(&pre, &[]).len(), 4);
    assert_eq!(step.post_actions_normalized().len(), 1);
}

#[tokio::test]
async fn db_action_feeds_variables_into_pre_script_and_rewrites_request() {
    let step = first_step();
    let (http, protocol) = match &step {
        Step::Request {
            request, protocol, ..
        } => (request.clone(), protocol),
        other => panic!("expected request step, got {other:?}"),
    };
    assert!(protocol.is_none());
    let orbit_config::RequestSpec::Http(cfg) = http else {
        panic!("the example should be an HTTP request");
    };

    let mut rt = PipelineRuntime::new(Box::new(HttpClient::new()), Box::new(JsonCodec));
    let ds: Arc<dyn DataSourceProvider> = Arc::new(MockProvider);
    rt.with_datasources(Some(ds));

    let spec = PipelineSpec {
        protocol: "http".into(),
        // Port 1 always refuses connections: used only to validate the pre-stage artifacts offline
        target: "http://127.0.0.1:1/api/profile".into(),
        operation: cfg.method.clone(),
        pre_actions: actions_to_pipeline(&step.pre_actions_normalized(), &[]),
        post_actions: actions_to_pipeline(&step.post_actions_normalized(), &[]),
        ..Default::default()
    };

    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("user_id".to_string(), "1".to_string());
    let env = HashMap::new();
    let cancel = CancellationToken::new();
    let outcome = execute_pipeline(&mut rt, spec, &mut vars, &env, &cancel, None).await;

    // Action logs in execution order: pre-interpolation script -> built-in interpolate node -> DB -> script (the disabled DB does not run)
    assert_eq!(outcome.pre_action_logs.len(), 4);
    assert_eq!(outcome.pre_action_logs[0].kind, "script");
    assert_eq!(outcome.pre_action_logs[0].phase, "pre_resolve");
    assert_eq!(outcome.pre_action_logs[1].kind, "interpolate");
    assert_eq!(outcome.pre_action_logs[1].phase, "interpolate");
    assert!(outcome.pre_action_logs[1].ok);
    assert_eq!(outcome.pre_action_logs[2].kind, "db");
    assert_eq!(outcome.pre_action_logs[2].phase, "pre");
    assert!(
        outcome.pre_action_logs[2].ok,
        "{}",
        outcome.pre_action_logs[2].detail
    );
    assert_eq!(outcome.pre_action_logs[3].kind, "script");
    assert_eq!(outcome.pre_action_logs[3].phase, "pre");
    assert!(outcome.pre_action_logs[3].ok);

    // Variables written by the DB action (single value + multi-column mapping)
    assert_eq!(
        outcome.action_vars.get("dbToken").map(String::as_str),
        Some("tk-abc")
    );
    assert_eq!(
        outcome.action_vars.get("dbUserId").map(String::as_str),
        Some("1")
    );
    // The variable space has been merged in for later steps to interpolate {{var}}
    assert_eq!(vars.get("dbToken").map(String::as_str), Some("tk-abc"));

    // The pre-script reads variables and rewrites request headers (both read methods should take effect)
    assert_eq!(
        outcome.request.headers.get("X-Token").map(String::as_str),
        Some("tk-abc")
    );
    assert_eq!(
        outcome.request.headers.get("X-User-Id").map(String::as_str),
        Some("1")
    );

    // Unreachable target -> protocol error; pre-action logs are still kept for debugging
    assert!(outcome.error.is_some());
}

/// Variables written by a script **before** the anchor must be consumed by the interpolation stage of **this request**.
#[tokio::test]
async fn script_before_anchor_variables_feed_interpolation() {
    let spec = PipelineSpec {
        protocol: "http".into(),
        // Port 1 always refuses connections: used only to validate the "template -> final request" artifacts offline
        target: "http://127.0.0.1:1/api/{{token}}".into(),
        operation: "GET".into(),
        headers: HashMap::from([("X-Token".to_string(), "{{token}}".to_string())]),
        pre_actions: vec![
            script("generate token", "pm.environment.set('token', 'T-123');"),
            PipelineAction::Interpolate,
        ],
        interpolate: true,
        ..Default::default()
    };

    let mut vars: HashMap<String, String> = HashMap::new();
    let env = HashMap::new();
    let cancel = CancellationToken::new();
    let outcome =
        execute_pipeline(&mut rt_with_codec(), spec, &mut vars, &env, &cancel, None).await;

    // The action log belongs to the pre-interpolation stage
    assert_eq!(outcome.pre_action_logs.len(), 2);
    assert_eq!(outcome.pre_action_logs[0].phase, "pre_resolve");
    assert!(
        outcome.pre_action_logs[0].ok,
        "{}",
        outcome.pre_action_logs[0].detail
    );

    // The target is unreachable, but the request was built from the variables written by the pre-anchor script
    assert_eq!(outcome.request.target, "http://127.0.0.1:1/api/T-123");
    assert_eq!(
        outcome.request.headers.get("X-Token").map(String::as_str),
        Some("T-123")
    );

    // Written variables: merged into the variable space (for later steps to reference) and returned via varsSet (for the frontend to persist to the environment)
    assert_eq!(vars.get("token").map(String::as_str), Some("T-123"));
    assert_eq!(
        outcome.vars_set.get("token").map(String::as_str),
        Some("T-123")
    );

    assert!(outcome.error.is_some());
}

/// A script **after** the anchor receives the **final message** (interpolation + encoding already done); a rewrite is the final bytes, with no second interpolation.
///
/// It also locks in the fix for an existing defect: a structured request body used to be wrapped
/// by `serde_json::to_string` into a quoted JSON string (`"{\"a\":1}"`) and then sent as the raw body, so scripts did not see the final content either.
#[tokio::test]
async fn script_after_anchor_sees_final_request_and_rewrite_is_final_bytes() {
    let spec = PipelineSpec {
        protocol: "http".into(),
        target: "http://127.0.0.1:1/sign".into(),
        operation: "POST".into(),
        headers: HashMap::from([("X-Sign".to_string(), "{{sign}}".to_string())]),
        // Structured body: interpolated -> encoded -> then handed to the post-anchor script
        body_value: Some(serde_yaml::Value::String(r#"{"name":"{{user}}"}"#.into())),
        pre_actions: vec![
            PipelineAction::Interpolate,
            script(
                "sign",
                r#"
console.log("seen:" + pm.request.body.raw);
pm.request.body.raw = '{"name":"{{user}}","sig":"sig-1"}';
pm.request.headers.upsert({ key: "X-Sign", value: "sig-1" });
"#,
            ),
        ],
        interpolate: true,
        ..Default::default()
    };

    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("user".to_string(), "alice".to_string());
    let env = HashMap::new();
    let cancel = CancellationToken::new();
    let outcome =
        execute_pipeline(&mut rt_with_codec(), spec, &mut vars, &env, &cancel, None).await;

    assert_eq!(outcome.pre_action_logs.len(), 2);
    assert_eq!(outcome.pre_action_logs[0].phase, "interpolate");
    assert_eq!(outcome.pre_action_logs[1].phase, "pre");
    assert!(
        outcome.pre_action_logs[1].ok,
        "{}",
        outcome.pre_action_logs[1].detail
    );

    // (1) The script sees the final request body: already interpolated (alice) and without an extra JSON outer quote
    assert!(
        outcome
            .pre_logs
            .iter()
            .any(|l| l.message == r#"seen:{"name":"alice"}"#),
        "the script should see the final request body, actual logs: {:?}",
        outcome
            .pre_logs
            .iter()
            .map(|l| &l.message)
            .collect::<Vec<_>>()
    );

    // (2) A script rewrite is the final bytes: the leftover {{user}} in it is not interpolated again
    assert_eq!(
        String::from_utf8(outcome.request.payload.clone()).unwrap(),
        r#"{"name":"{{user}}","sig":"sig-1"}"#
    );
    assert_eq!(
        outcome.request.headers.get("X-Sign").map(String::as_str),
        Some("sig-1")
    );
}

/// **Position determines timing**: the same script placed before / after the built-in anchor has completely different semantics.
#[tokio::test]
async fn anchor_position_decides_script_semantics() {
    let code = r#"
pm.request.url = "http://127.0.0.1:1/{{ver}}/x";
pm.request.body.raw = '{"v":"{{ver}}"}';
"#;

    // (1) Before the anchor -> the **template** is rewritten and still interpolated afterwards
    let before = run_pre_actions(vec![
        script("rewrite template", code),
        PipelineAction::Interpolate,
    ])
    .await;
    assert_eq!(before.request.target, "http://127.0.0.1:1/v2/x");
    assert_eq!(
        String::from_utf8(before.request.payload.clone()).unwrap(),
        r#"{"v":"v2"}"#
    );
    assert_eq!(before.pre_action_logs[0].phase, "pre_resolve");
    assert_eq!(before.pre_action_logs[1].phase, "interpolate");

    // (2) After the anchor -> the rewrite is the **final bytes**, {{ver}} is not interpolated again
    let after = run_pre_actions(vec![
        PipelineAction::Interpolate,
        script("rewrite final message", code),
    ])
    .await;
    assert_eq!(after.request.target, "http://127.0.0.1:1/{{ver}}/x");
    assert_eq!(
        String::from_utf8(after.request.payload.clone()).unwrap(),
        r#"{"v":"{{ver}}"}"#
    );
    assert_eq!(after.pre_action_logs[0].phase, "interpolate");
    assert_eq!(after.pre_action_logs[1].phase, "pre");
}

/// Zero migration for existing data: when the list has no built-in anchor, one is prepended at the **front**, and actions still run after interpolation.
#[tokio::test]
async fn missing_anchor_is_prepended_so_actions_run_after_interpolation() {
    // The `{{ver}}` written back by the script is not interpolated by this request -> proof that it runs after the anchor
    let outcome = run_pre_actions(vec![script(
        "legacy pre-script",
        r#"pm.request.url = "http://127.0.0.1:1/{{ver}}/legacy";"#,
    )])
    .await;

    assert_eq!(outcome.pre_action_logs.len(), 2);
    assert_eq!(outcome.pre_action_logs[0].kind, "interpolate");
    assert_eq!(outcome.pre_action_logs[1].phase, "pre");
    assert_eq!(outcome.request.target, "http://127.0.0.1:1/{{ver}}/legacy");
}

/// Legacy `pre_scripts` (semantics = post-interpolation) also land after the anchor on a non-normalized spec.
#[tokio::test]
async fn legacy_pre_scripts_stay_after_the_anchor() {
    let spec = PipelineSpec {
        protocol: "http".into(),
        target: "http://127.0.0.1:1/legacy".into(),
        operation: "GET".into(),
        pre_scripts: vec![r#"pm.request.url = "http://127.0.0.1:1/{{ver}}/raw";"#.into()],
        interpolate: true,
        ..Default::default()
    };
    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("ver".to_string(), "v2".to_string());
    let env = HashMap::new();
    let cancel = CancellationToken::new();
    let outcome =
        execute_pipeline(&mut rt_with_codec(), spec, &mut vars, &env, &cancel, None).await;

    assert_eq!(outcome.pre_action_logs[0].kind, "interpolate");
    assert_eq!(outcome.pre_action_logs[1].kind, "script");
    assert_eq!(outcome.pre_action_logs[1].phase, "pre");
    assert_eq!(outcome.request.target, "http://127.0.0.1:1/{{ver}}/raw");
}

/// Legacy behavior: with no pre-actions configured, the request matches the passed spec field by field (zero migration).
#[tokio::test]
async fn no_pre_actions_leaves_request_untouched() {
    let spec = PipelineSpec {
        protocol: "http".into(),
        target: "http://127.0.0.1:1/api/{{token}}".into(),
        operation: "PUT".into(),
        headers: HashMap::from([("X-Raw".to_string(), "{{token}}".to_string())]),
        body: b"raw-bytes".to_vec(),
        // Consistent with historical behavior: on the single-send path the caller builds the request, and the pipeline does not interpolate
        interpolate: false,
        ..Default::default()
    };

    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("token".to_string(), "T-1".to_string());
    let env = HashMap::new();
    let cancel = CancellationToken::new();
    let outcome =
        execute_pipeline(&mut rt_with_codec(), spec, &mut vars, &env, &cancel, None).await;

    assert!(outcome.error.is_some());
    assert_eq!(outcome.request.target, "http://127.0.0.1:1/api/{{token}}");
    assert_eq!(outcome.request.operation, "PUT");
    assert_eq!(
        outcome.request.headers.get("X-Raw").map(String::as_str),
        Some("{{token}}")
    );
    assert_eq!(outcome.request.payload, b"raw-bytes");
    // Only the built-in anchor entry (un-interpolated, raw byte body -> the anchor is a no-op)
    assert_eq!(outcome.pre_action_logs.len(), 1);
    assert_eq!(outcome.pre_action_logs[0].kind, "interpolate");
    assert!(outcome.pre_action_logs[0].ok);
}

/// Template path end to end: a pre-anchor script generates a URL variable -> interpolation -> path/query encoding and joining
/// -> a post-anchor script signs the final bytes.
#[tokio::test]
async fn request_template_end_to_end_with_anchor() {
    use orbit_engine::request_build::{BodyTemplate, RequestTemplate, TextBodyMode};

    let template = RequestTemplate {
        url: "http://127.0.0.1:1/{{ver}}/items/{id}".into(),
        path_params: vec![("id".into(), "{{itemId}}".into())],
        query_params: vec![("q".into(), "{{kw}}".into())],
        default_headers: vec![("Accept".into(), "*/*".into())],
        auth_headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
        user_headers: vec![],
        cookies: vec![("sid".into(), "{{sid}}".into())],
        // The body comes with the template (copied to body_value when the engine expands it, rewritable by pre-anchor scripts)
        body: BodyTemplate::Text {
            format: TextBodyMode::Json,
            text: r#"{"n":"{{name}}"}"#.into(),
            content_type: None,
        },
    };

    let spec = PipelineSpec {
        protocol: "http".into(),
        operation: "POST".into(),
        request_template: Some(template),
        pre_actions: vec![
            script(
                "generate version",
                "pm.environment.set('ver', 'v' + pm.variables.get('n'));",
            ),
            PipelineAction::Interpolate,
            script(
                "sign final bytes",
                r#"
const raw = pm.request.body.raw;
pm.request.headers.upsert({ key: "X-Sig", value: "len:" + raw.length });
"#,
            ),
        ],
        ..Default::default()
    };

    let mut vars: HashMap<String, String> = HashMap::new();
    for (k, v) in [
        ("n", "9"),
        ("itemId", "a b"),
        ("kw", "c d"),
        ("token", "tk"),
        ("sid", "s1"),
        ("name", "alice"),
    ] {
        vars.insert(k.to_string(), v.to_string());
    }
    let env = HashMap::new();
    let cancel = CancellationToken::new();
    let outcome =
        execute_pipeline(&mut rt_with_codec(), spec, &mut vars, &env, &cancel, None).await;

    // URL: the `ver` written by the pre-anchor script participates in interpolation; path / query values are encoded as with encodeURIComponent
    assert_eq!(
        outcome.request.target,
        "http://127.0.0.1:1/v9/items/a%20b?q=c%20d"
    );

    let h = &outcome.request.headers;
    assert_eq!(h.get("Accept").unwrap(), "*/*");
    assert_eq!(h.get("Authorization").unwrap(), "Bearer tk");
    assert_eq!(h.get("Cookie").unwrap(), "sid=s1");
    assert_eq!(h.get("Content-Type").unwrap(), "application/json");
    assert_eq!(h.get("Host").unwrap(), "127.0.0.1:1");
    // The body is interpolated: `{"n":"alice"}` is 13 bytes, and the script sees the final bytes
    assert_eq!(
        String::from_utf8(outcome.request.payload.clone()).unwrap(),
        r#"{"n":"alice"}"#
    );
    assert_eq!(h.get("Content-Length").unwrap(), "13");
    assert_eq!(h.get("X-Sig").unwrap(), "len:13");

    // Variables written by the pre-anchor script can be persisted (the frontend writes them back to the environment)
    assert_eq!(vars.get("ver").map(String::as_str), Some("v9"));
    assert_eq!(outcome.vars_set.get("ver").map(String::as_str), Some("v9"));
    assert!(outcome.error.is_some());
}

/// Build only, do not send (`dry_run`): returns the final request and **does not open a connection**.
///
/// The distributed agent path relies on it: the controller performs "pre-anchor actions + interpolation + assembly + post-anchor actions",
/// and the agent only sends the final message. The target is unreachable, so if a request were actually sent `error` would not be None.
#[tokio::test]
async fn dry_run_builds_request_without_sending() {
    use orbit_engine::request_build::{BodyTemplate, RequestTemplate, TextBodyMode};

    let spec = PipelineSpec {
        protocol: "http".into(),
        operation: "POST".into(),
        request_template: Some(RequestTemplate {
            url: "http://127.0.0.1:1/api/{{path}}".into(),
            default_headers: vec![("Accept".into(), "*/*".into())],
            body: BodyTemplate::Text {
                format: TextBodyMode::Json,
                text: r#"{"v":"{{v}}"}"#.into(),
                content_type: None,
            },
            ..Default::default()
        }),
        pre_actions: vec![
            script(
                "generate path variable",
                "pm.environment.set('path', 'v' + pm.variables.get('n'));",
            ),
            PipelineAction::Interpolate,
        ],
        dry_run: true,
        ..Default::default()
    };

    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("n".to_string(), "2".to_string());
    vars.insert("v".to_string(), "x".to_string());
    let env = HashMap::new();
    let cancel = CancellationToken::new();
    let outcome =
        execute_pipeline(&mut rt_with_codec(), spec, &mut vars, &env, &cancel, None).await;

    // Not sent: neither a response nor an error
    assert!(outcome.response.is_none());
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    // But the request is fully built (variables generated by the pre-anchor script participate in interpolation)
    assert_eq!(outcome.request.target, "http://127.0.0.1:1/api/v2");
    assert_eq!(
        String::from_utf8(outcome.request.payload.clone()).unwrap(),
        r#"{"v":"x"}"#
    );
    assert_eq!(
        outcome
            .request
            .headers
            .get("Content-Type")
            .map(String::as_str),
        Some("application/json")
    );
    assert_eq!(outcome.vars_set.get("path").map(String::as_str), Some("v2"));
}

// ─── script library reference (`RequestAction::Ref`) ────────────────────

/// References are **expanded to the library item's current content** before execution, keeping their position - position still decides pre-/post-interpolation.
#[tokio::test]
async fn library_ref_expands_to_library_content_at_its_position() {
    use orbit_config::ActionTemplate;

    let library = vec![
        ActionTemplate::new(
            "tpl-nonce",
            "generate nonce",
            RequestAction::from_script("pm.environment.set('nonce', 'N1');"),
        ),
        ActionTemplate::new(
            "tpl-sign",
            "compute signature",
            RequestAction::from_script(
                "pm.request.headers.upsert({ key: 'X-Sign', value: pm.variables.get('nonce') });",
            ),
        ),
    ];
    let actions = vec![
        RequestAction::from_ref("tpl-nonce"),
        RequestAction::Interpolate,
        RequestAction::from_ref("tpl-sign"),
    ];
    let outcome = run_pre_actions(actions_to_pipeline(&actions, &library)).await;

    assert_eq!(
        outcome.pre_action_logs.len(),
        3,
        "{:?}",
        outcome.pre_action_logs
    );
    // Before the anchor: the library item script runs in the pre-interpolation stage, and the variables it writes are consumed by this interpolation
    assert_eq!(outcome.pre_action_logs[0].name, "generate nonce");
    assert_eq!(outcome.pre_action_logs[0].phase, "pre_resolve");
    assert!(outcome.pre_action_logs[0].ok);
    assert_eq!(outcome.pre_action_logs[1].kind, "interpolate");
    // After the anchor: the library item script runs on the final message
    assert_eq!(outcome.pre_action_logs[2].name, "compute signature");
    assert_eq!(outcome.pre_action_logs[2].phase, "pre");
    assert!(outcome.pre_action_logs[2].ok);
    // The library item content really takes effect: the signature header takes the variable written by the pre-anchor library item script
    assert_eq!(
        outcome.request.headers.get("X-Sign").map(String::as_str),
        Some("N1")
    );
}

/// A disabled library item => the reference is not executed (disabling the library item disables it globally).
///
/// This locks in the "resolve references first, then filter by enabled" order: filtering first and resolving after would miss this case.
#[tokio::test]
async fn disabled_library_item_skips_the_reference() {
    use orbit_config::ActionTemplate;

    let mut tpl = ActionTemplate::new(
        "tpl-idle",
        "disabled library item",
        RequestAction::from_script("pm.environment.set('x', '1');"),
    );
    tpl.action.set_enabled(false);

    let actions = vec![
        RequestAction::Interpolate,
        RequestAction::from_ref("tpl-idle"),
    ];
    let outcome = run_pre_actions(actions_to_pipeline(&actions, &[tpl])).await;

    assert_eq!(
        outcome.pre_action_logs.len(),
        1,
        "{:?}",
        outcome.pre_action_logs
    );
    assert_eq!(outcome.pre_action_logs[0].kind, "interpolate");
    assert!(outcome.vars_set.is_empty());
}

/// Dangling reference (library item deleted / library table not provided): log one **error** entry, **do not abort the request**, and do not silently drop it.
#[tokio::test]
async fn dangling_ref_logs_error_without_aborting_request() {
    let actions = vec![
        RequestAction::Interpolate,
        RequestAction::from_ref("tpl-gone"),
        RequestAction::from_script("pm.environment.set('after', '1');"),
    ];
    let outcome = run_pre_actions(actions_to_pipeline(&actions, &[])).await;

    assert_eq!(
        outcome.pre_action_logs.len(),
        3,
        "{:?}",
        outcome.pre_action_logs
    );
    let broken = &outcome.pre_action_logs[1];
    assert_eq!(broken.kind, "ref");
    assert!(!broken.ok);
    assert!(
        broken.detail.contains("tpl-gone"),
        "the error message should name the library item id: {}",
        broken.detail
    );
    assert!(
        outcome.pre_logs.iter().any(|l| l.level == "error"),
        "there should be an error-level console log: {:?}",
        outcome.pre_logs
    );
    // Subsequent actions run as usual (the request was not aborted)
    assert!(outcome.pre_action_logs[2].ok);
    assert_eq!(outcome.vars_set.get("after").map(String::as_str), Some("1"));
}

/// Regression: **environment variables** must be visible to the interpolation node.
///
/// `base_url` is conventionally kept in the active environment rather than in the request variable space, so a URL
/// like `{{base_url}}/users/{id}` used to stay un-interpolated: the raw `{{base_url}}` then reached the protocol
/// client as a relative URL and the send failed with "Invalid URL: relative URL without a base".
#[tokio::test]
async fn env_variable_in_url_template_is_interpolated() {
    use orbit_engine::request_build::RequestTemplate;

    let template = RequestTemplate {
        url: "{{base_url}}/users/{id}".into(),
        path_params: vec![("id".into(), "{{userId}}".into())],
        ..Default::default()
    };

    let spec = PipelineSpec {
        protocol: "http".into(),
        operation: "GET".into(),
        request_template: Some(template),
        ..Default::default()
    };

    // `base_url` comes from the environment; `userId` from the request variable space
    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("userId".to_string(), "42".to_string());
    let mut env: HashMap<String, String> = HashMap::new();
    env.insert("base_url".to_string(), "http://127.0.0.1:1".to_string());
    let cancel = CancellationToken::new();
    let outcome =
        execute_pipeline(&mut rt_with_codec(), spec, &mut vars, &env, &cancel, None).await;

    // The environment variable is interpolated, so the URL becomes absolute; the target is unreachable, which is
    // irrelevant here - the assertions are on the request that was built.
    assert_eq!(outcome.request.target, "http://127.0.0.1:1/users/42");
}
