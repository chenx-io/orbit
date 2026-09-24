//! Test step model + protocol resolution behavior

use orbit_protocol::ProtocolKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::action::{normalize_actions, normalize_pre_actions, RequestAction};
use super::check::Check;
use super::extract::Extraction;
use super::request::{HttpRequestConfig, RequestSpec};

/// Test step - 6 types covering every API automation testing scenario
///
/// YAML format (`type` tag distinguishes the variants):
/// ```yaml
/// steps:
///   - type: request
///     name: "Login"
///     request: { method: POST, url: "http://..." }
///     checks: [{ type: status, value: 200 }]
///
///   - type: loop
///     name: "Repeat 3×"
///     count: 3
///     steps: [{ type: request, ... }]
///
///   - type: wait
///     name: "Pause"
///     duration: "2s"
///
///   - type: setvar
///     name: "Set env"
///     key: "token"
///     value: "${response.body.token}"
///
///   - type: condition
///     name: "Check status"
///     expression: "${response.status} == 200"
///     then: [{ type: request, ... }]
///     else: [{ type: request, ... }]
///
///   - type: group
///     name: "Login Flow"
///     steps: [{ type: request, ... }, { type: wait, ... }]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[allow(clippy::large_enum_variant)]
pub enum Step {
    /// HTTP/gRPC/WebSocket and other request steps
    #[serde(rename = "request")]
    Request {
        #[serde(default)]
        name: String,
        request: RequestSpec,
        /// Protocol kind (optional, built-in protocol)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        protocol: Option<ProtocolKind>,
        /// Plugin protocol id (optional, overrides the built-in protocol; e.g. `dubbo` / `myplugin-protocol`)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        protocol_id: Option<String>,
        #[serde(default)]
        checks: Vec<Check>,
        #[serde(default)]
        extract: Vec<Extraction>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pre_script: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        post_script: Option<String>,
        /// **Compatibility input field (deprecated, never written out)**: the "pre-interpolation actions" list of the previous two-stage change.
        ///
        /// It is spliced **before** the built-in interpolate node during normalization (see [`Step::pre_actions_normalized`]).
        #[serde(default, skip_serializing)]
        pre_resolve_actions: Vec<RequestAction>,
        /// **Compatibility input field (deprecated, never written out)**: the previous single-script shorthand for "pre-interpolation".
        #[serde(default, skip_serializing)]
        pre_resolve_script: Option<String>,
        /// Pre-action list (JS scripts / database queries / built-in interpolate node, executed in list order)
        ///
        /// **List order is execution order**: before the built-in [`RequestAction::Interpolate`] node = pre-interpolation
        /// (variables may be written for this interpolation and the template rewritten); after it = post-interpolation (rewrites are the final bytes).
        /// Coexists with the legacy `pre_script` field: when the list is empty, [`Step::pre_actions_normalized`]
        /// falls back to `pre_script` (guaranteeing no loss of historical config).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pre_actions: Vec<RequestAction>,
        /// Post-action list (executed in order after the response is received)
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        post_actions: Vec<RequestAction>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
        /// Whether disabled
        #[serde(default)]
        disabled: bool,
    },
    /// Loop - repeats the child steps count times
    #[serde(rename = "loop")]
    Loop {
        #[serde(default)]
        name: String,
        /// Number of iterations
        count: u32,
        /// Steps inside the loop body
        steps: Vec<Step>,
        #[serde(default)]
        disabled: bool,
    },
    /// Wait/pause - delays for the given time
    #[serde(rename = "wait")]
    Wait {
        #[serde(default)]
        name: String,
        /// Wait duration, e.g. "2s", "500ms"
        duration: String,
        #[serde(default)]
        disabled: bool,
    },
    /// Set variable - injects/updates a variable at runtime
    #[serde(rename = "setvar")]
    SetVar {
        #[serde(default)]
        name: String,
        /// Variable name
        key: String,
        /// Variable value (supports `{{var}}` / `${var}` interpolation)
        value: String,
        #[serde(default)]
        disabled: bool,
    },
    /// Conditional branch - executes the then or else branch based on an expression
    #[serde(rename = "condition")]
    Condition {
        #[serde(default)]
        name: String,
        /// Condition expression, e.g. "{{status}} == 200" or "${status} == 200"
        expression: String,
        /// Steps executed when the condition is true
        then: Vec<Step>,
        /// Steps executed when the condition is false (optional)
        #[serde(default)]
        #[serde(rename = "else")]
        else_steps: Vec<Step>,
        #[serde(default)]
        disabled: bool,
    },
    /// Group - organizes several steps into one logical unit (display/scope only, does not change the execution flow)
    #[serde(rename = "group")]
    Group {
        #[serde(default)]
        name: String,
        steps: Vec<Step>,
        #[serde(default)]
        disabled: bool,
    },
}

impl Step {
    /// Get the step name (common to all variants)
    pub fn name(&self) -> &str {
        match self {
            Step::Request { name, .. } => name,
            Step::Loop { name, .. } => name,
            Step::Wait { name, .. } => name,
            Step::SetVar { name, .. } => name,
            Step::Condition { name, .. } => name,
            Step::Group { name, .. } => name,
        }
    }

    /// Whether the step is disabled
    pub fn is_disabled(&self) -> bool {
        match self {
            Step::Request { disabled, .. } => *disabled,
            Step::Loop { disabled, .. } => *disabled,
            Step::Wait { disabled, .. } => *disabled,
            Step::SetVar { disabled, .. } => *disabled,
            Step::Condition { disabled, .. } => *disabled,
            Step::Group { disabled, .. } => *disabled,
        }
    }

    /// Whether the step is a request step
    pub fn is_request(&self) -> bool {
        matches!(self, Step::Request { .. })
    }

    /// Access the request config through the Request variant reference (valid only for the Request variant)
    pub fn try_as_request(&self) -> Option<(&RequestSpec, &Option<ProtocolKind>)> {
        if let Step::Request {
            request, protocol, ..
        } = self
        {
            Some((request, protocol))
        } else {
            None
        }
    }

    /// Get the request config in HTTP form (valid only when this step's request is the Http variant)
    pub fn as_http(&self) -> Option<&HttpRequestConfig> {
        match self {
            Step::Request {
                request: RequestSpec::Http(cfg),
                ..
            } => Some(cfg.as_ref()),
            _ => None,
        }
    }

    /// Resolve the protocol this step actually uses (valid only for the Request variant)
    pub fn resolve_protocol(&self) -> ProtocolKind {
        match self {
            Step::Request {
                request, protocol, ..
            } => {
                if let Some(p) = protocol {
                    return *p;
                }
                match request {
                    RequestSpec::Grpc(_) => ProtocolKind::Grpc,
                    RequestSpec::WebSocket(_) => ProtocolKind::WebSocket,
                    RequestSpec::Tcp(_) => ProtocolKind::Tcp,
                    RequestSpec::Udp(_) => ProtocolKind::Udp,
                    RequestSpec::Sse(_) => ProtocolKind::Sse,
                    RequestSpec::Graphql(_) => ProtocolKind::Graphql,
                    RequestSpec::Http(cfg) => {
                        if cfg.grpc_service.is_some() {
                            return ProtocolKind::Grpc;
                        }
                        let scheme = cfg.url.split(':').next().unwrap_or("").to_ascii_lowercase();
                        match scheme.as_str() {
                            "ws" | "wss" => ProtocolKind::WebSocket,
                            "tcp" => ProtocolKind::Tcp,
                            "udp" => ProtocolKind::Udp,
                            "grpc" => ProtocolKind::Grpc,
                            _ => ProtocolKind::Http,
                        }
                    }
                }
            }
            _ => ProtocolKind::Http,
        }
    }

    /// Resolve the protocol id this step actually uses (a string, plugin protocols supported).
    /// An explicitly given `protocol_id` wins; otherwise the built-in protocol name is used.
    pub fn resolve_protocol_id(&self) -> String {
        match self {
            Step::Request { protocol_id, .. } => {
                if let Some(pid) = protocol_id {
                    if !pid.is_empty() {
                        return pid.clone();
                    }
                }
                self.resolve_protocol().as_str().to_string()
            }
            _ => "http".to_string(),
        }
    }

    /// Normalize the pre-action list (**the only entry point**): converge on "a single ordered list + one unique built-in interpolate node".
    ///
    /// - falls back to the legacy `pre_script` when the explicit `pre_actions` is empty;
    /// - the previous `pre_resolve_actions` / `pre_resolve_script` (read for compatibility) are spliced in before the anchor;
    /// - with no explicit anchor, one is inserted at the **front of the list** (existing actions mean post-interpolation, so behavior is unchanged);
    /// - the returned list contains **at least one** [`RequestAction::Interpolate`] node.
    ///
    /// All execution / display paths should read through this to avoid repeating compatibility checks everywhere.
    pub fn pre_actions_normalized(&self) -> Vec<RequestAction> {
        match self {
            Step::Request {
                pre_actions,
                pre_script,
                pre_resolve_actions,
                pre_resolve_script,
                ..
            } => normalize_pre_actions(
                pre_actions,
                pre_script.as_ref(),
                pre_resolve_actions,
                pre_resolve_script.as_ref(),
            ),
            _ => Vec::new(),
        }
    }

    /// Normalize the post-action list: falls back to the legacy `post_script` when the explicit `post_actions` is empty.
    pub fn post_actions_normalized(&self) -> Vec<RequestAction> {
        match self {
            Step::Request {
                post_actions,
                post_script,
                ..
            } => normalize_actions(post_actions, post_script.as_ref()),
            _ => Vec::new(),
        }
    }

    /// Legacy pre-script string (only for import/export and other paths that must keep the old format)
    pub fn legacy_pre_script(&self) -> Option<&String> {
        match self {
            Step::Request { pre_script, .. } => pre_script.as_ref(),
            _ => None,
        }
    }

    /// Legacy post-script string (only for import/export and other paths that must keep the old format)
    pub fn legacy_post_script(&self) -> Option<&String> {
        match self {
            Step::Request { post_script, .. } => post_script.as_ref(),
            _ => None,
        }
    }
}

impl Default for Step {
    fn default() -> Self {
        Step::Request {
            name: String::new(),
            request: RequestSpec::Http(Box::new(HttpRequestConfig {
                method: "GET".into(),
                url: String::new(),
                headers: HashMap::new(),
                body: None,
                timeout: "30s".into(),
                payload_format: None,
                grpc_service: None,
                grpc_use_reflection: false,
                response_format: None,
            })),
            protocol: None,
            protocol_id: None,
            checks: vec![],
            extract: vec![],
            pre_script: None,
            post_script: None,
            pre_resolve_actions: vec![],
            pre_resolve_script: None,
            pre_actions: vec![],
            post_actions: vec![],
            tags: vec![],
            disabled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_step_with_tags() {
        let yaml = r#"
name: "tagged"
scenarios:
  - name: "test"
    executor:
      type: constant-vus
      vus: 1
      duration: "1s"
    steps:
      - type: request
        request:
          method: GET
          url: "https://example.com"
        tags:
          - smoke
          - nightly
"#;
        let plan = crate::from_str(yaml).unwrap();
        if let Step::Request { tags, .. } = &plan.scenarios[0].steps[0] {
            assert_eq!(tags, &vec!["smoke".to_string(), "nightly".to_string()]);
        } else {
            panic!("Expected Request step");
        }
    }

    #[test]
    fn test_step_with_scripts() {
        let yaml = r#"
name: "scripted"
scenarios:
  - name: "test"
    executor:
      type: constant-vus
      vus: 1
      duration: "1s"
    steps:
      - type: request
        request:
          method: GET
          url: "https://example.com"
        pre_script: "request.url = request.url + \"/v2\";"
        post_script: "if (response.status !== 200) { throw new Error(\"fail\"); }"
"#;
        let plan = crate::from_str(yaml).unwrap();
        if let Step::Request {
            pre_script,
            post_script,
            ..
        } = &plan.scenarios[0].steps[0]
        {
            assert!(pre_script.is_some());
            assert!(post_script.is_some());
        } else {
            panic!("Expected Request step");
        }
    }

    #[test]
    fn test_step_with_actions() {
        let yaml = r#"
name: "actions"
scenarios:
  - name: "test"
    executor:
      type: constant-vus
      vus: 1
      duration: "1s"
    steps:
      - type: request
        request:
          method: GET
          url: "https://example.com"
        pre_actions:
          - type: db
            name: "find user"
            datasource: "user-db"
            sql: "SELECT id FROM users LIMIT 1"
            target: { type: scalar }
            extract_var: "dbId"
          - type: script
            code: "pm.request.headers.upsert({ key: 'X-Uid', value: pm.variables.get('dbId') });"
        post_actions:
          - type: script
            code: "console.log(pm.response.code);"
"#;
        let plan = crate::from_str(yaml).unwrap();
        let step = &plan.scenarios[0].steps[0];
        let pre = step.pre_actions_normalized();
        // No explicit anchor: one is inserted at the front, so all existing actions land after interpolation
        assert_eq!(pre.len(), 3);
        assert!(pre[0].is_interpolate());
        assert_eq!(pre[1].kind(), "db");
        assert_eq!(pre[2].kind(), "script");
        assert_eq!(step.post_actions_normalized().len(), 1);
    }

    #[test]
    fn test_actions_normalize_legacy_single_script() {
        // The legacy format (a single pre_script / post_script only) should normalize to a single-element action list
        let yaml = r#"
name: "legacy"
scenarios:
  - name: "test"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request: { method: GET, url: "https://example.com" }
        pre_script: "pm.request.url += '/v2';"
"#;
        let plan = crate::from_str(yaml).unwrap();
        let step = &plan.scenarios[0].steps[0];
        let pre = step.pre_actions_normalized();
        // The legacy single script means post-interpolation: it must land after the built-in anchor
        assert_eq!(pre.len(), 2);
        assert!(pre[0].is_interpolate());
        assert_eq!(pre[1].script_code(), Some("pm.request.url += '/v2';"));
        assert!(step.post_actions_normalized().is_empty());
    }

    #[test]
    fn test_resolve_protocol_explicit() {
        let yaml = r#"
name: "multi"
scenarios:
  - name: "test"
    executor:
      type: constant-vus
      vus: 1
      duration: "1s"
    steps:
      - type: request
        request:
          method: GET
          url: "https://example.com"
        protocol: grpc
      - type: request
        request:
          method: GET
          url: "wss://echo.example.com"
        protocol: websocket
"#;
        let plan = crate::from_str(yaml).unwrap();
        assert_eq!(
            plan.scenarios[0].steps[0].resolve_protocol(),
            orbit_protocol::ProtocolKind::Grpc
        );
        assert_eq!(
            plan.scenarios[0].steps[1].resolve_protocol(),
            orbit_protocol::ProtocolKind::WebSocket
        );
    }

    #[test]
    fn test_resolve_protocol_inferred() {
        let yaml = r#"
name: "multi"
scenarios:
  - name: "test"
    executor:
      type: constant-vus
      vus: 1
      duration: "1s"
    steps:
      - type: request
        request:
          method: POST
          url: "https://example.com/api"
          grpc_service: "/pkg.Svc/Method"
      - type: request
        request:
          method: GET
          url: "wss://echo.example.com/ws"
      - type: request
        request:
          method: GET
          url: "https://example.com"
"#;
        let plan = crate::from_str(yaml).unwrap();
        assert_eq!(
            plan.scenarios[0].steps[0].resolve_protocol(),
            orbit_protocol::ProtocolKind::Grpc
        );
        assert_eq!(
            plan.scenarios[0].steps[1].resolve_protocol(),
            orbit_protocol::ProtocolKind::WebSocket
        );
        assert_eq!(
            plan.scenarios[0].steps[2].resolve_protocol(),
            orbit_protocol::ProtocolKind::Http
        );
    }

    #[test]
    fn test_step_protocol_id_override() {
        let plan = crate::from_str(
            r#"
name: "t"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request: { url: "http://h:1", method: GET }
        protocol_id: "dubbo"
      - type: request
        request: { url: "wss://x/ws", message_type: text }
        protocol: websocket
"#,
        )
        .unwrap();
        let steps = &plan.scenarios[0].steps;
        // protocol_id wins over request inference (HTTP URL -> the dubbo plugin protocol)
        assert_eq!(steps[0].resolve_protocol_id(), "dubbo");
        assert_eq!(
            steps[0].resolve_protocol(),
            orbit_protocol::ProtocolKind::Http
        );
        // Without a protocol_id it falls back to the built-in protocol name
        assert_eq!(steps[1].resolve_protocol_id(), "websocket");
    }

    #[test]
    fn test_pre_actions_normalized_always_has_single_anchor() {
        // An existing step with no pre-actions: after normalization only the built-in interpolate node remains
        let yaml = r#"
name: "empty"
scenarios:
  - name: "test"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request: { method: GET, url: "https://example.com" }
"#;
        let plan = crate::from_str(yaml).unwrap();
        let pre = plan.scenarios[0].steps[0].pre_actions_normalized();
        assert_eq!(pre.len(), 1);
        assert!(pre[0].is_interpolate());
    }

    #[test]
    fn test_pre_actions_explicit_anchor_decides_stage() {
        // Single list + built-in anchor: before the anchor = pre-interpolation, after it = post-interpolation
        let yaml = r#"
name: "anchor"
scenarios:
  - name: "test"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request: { method: GET, url: "https://example.com/{{n}}" }
        pre_actions:
          - type: script
            code: "pm.environment.set('n', '1');"
          - type: interpolate
          - type: script
            code: "pm.request.headers.upsert({ key: 'X-Sign', value: 's' });"
"#;
        let plan = crate::from_str(yaml).unwrap();
        let pre = plan.scenarios[0].steps[0].pre_actions_normalized();
        assert_eq!(pre.len(), 3);
        assert_eq!(pre[0].script_code(), Some("pm.environment.set('n', '1');"));
        assert!(pre[1].is_interpolate());
        assert_eq!(
            pre[2].script_code(),
            Some("pm.request.headers.upsert({ key: 'X-Sign', value: 's' });")
        );
    }

    #[test]
    fn test_pre_actions_merges_previous_two_stage_fields() {
        // The previous two-stage form (pre_resolve_actions / pre_actions) -> merged into "before anchor + anchor + after anchor"
        let yaml = r#"
name: "compat"
scenarios:
  - name: "test"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request: { method: GET, url: "https://example.com" }
        pre_resolve_actions:
          - type: script
            code: "pm.environment.set('nonce', '1');"
        pre_actions:
          - type: script
            code: "pm.request.url += '/v2';"
"#;
        let plan = crate::from_str(yaml).unwrap();
        let pre = plan.scenarios[0].steps[0].pre_actions_normalized();
        assert_eq!(pre.len(), 3);
        assert_eq!(
            pre[0].script_code(),
            Some("pm.environment.set('nonce', '1');")
        );
        assert!(pre[1].is_interpolate());
        assert_eq!(pre[2].script_code(), Some("pm.request.url += '/v2';"));
    }

    #[test]
    fn test_pre_actions_serialization_drops_compat_fields_and_keeps_anchor() {
        // Compatibility input fields (pre_resolve_*) are read but never written; an explicit anchor is persisted with the single list
        let yaml = r#"
name: "roundtrip"
scenarios:
  - name: "test"
    executor: { type: sequential, iterations: 1 }
    steps:
      - type: request
        request: { method: GET, url: "https://example.com" }
        pre_resolve_script: "pm.environment.set('n','1');"
        pre_actions:
          - type: script
            code: "pm.request.url += '/v2';"
          - type: interpolate
"#;
        let plan = crate::from_str(yaml).unwrap();
        let step = &plan.scenarios[0].steps[0];
        let out = serde_yaml::to_string(step).unwrap();
        assert!(
            !out.contains("pre_resolve"),
            "compat fields must not be written out any more: {}",
            out
        );
        assert!(
            out.contains("interpolate"),
            "the anchor should be persisted: {}",
            out
        );
    }
}
