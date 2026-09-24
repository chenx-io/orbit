//! # orbit-mock
//!
//! Lightweight mock server. Auto-generates mock endpoints from a YAML test plan.
//!
//! ## Model
//! - Each "interface" (`MockInterface`) is routed by `method + path` and owns a set of "expectations" (`MockExpectation`).
//! - When a request arrives, it is first routed to an interface by `method + path` (exact or path template), then within that interface
//!   a matching expectation is selected by **IP condition** + multiple **param conditions (AND)** and used as the response.
//! - Param conditions support locations: `query` / `path` / `header` / `body` / `cookie`;
//!   Comparison operators: `equals` / `not_equals` / `contains` / `not_contains` / `exists` / `not_exists` / `regex` / `gt` / `gte` / `lt` / `lte`.
//! - An expectation's `ip_condition` is off by default; when enabled it only applies to requests from that IP.
//! - An expectation with no conditions (and IP condition off) acts as the "default/fallback" and always matches.

use axum::{
    body::Bytes,
    extract::{ConnectInfo, OriginalUri, State},
    response::Response,
    routing::any,
    Router,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use orbit_dynamic;

/// Param location: determines where in the request a value is read to compare against the condition value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ParamLocation {
    Query,
    Path,
    Header,
    Body,
    Cookie,
}

/// Comparison operator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    Exists,
    NotExists,
    #[serde(rename = "regex")]
    Regex,
    Gt,
    Gte,
    Lt,
    Lte,
}

/// A single param condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamCondition {
    /// Param location (query/path/header/body/cookie)
    #[serde(default = "default_location")]
    pub location: String,
    /// Param name (body supports dot paths, e.g. `user.id`)
    pub name: String,
    /// Comparison operator
    #[serde(default = "default_op")]
    pub op: String,
    /// Comparison value (ignored for exists/not_exists)
    #[serde(default)]
    pub value: String,
}

fn default_location() -> String {
    "query".into()
}
fn default_op() -> String {
    "equals".into()
}

/// IP condition (scopes when an expectation applies based on the source IP). Off by default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IpCondition {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub ip: String,
}

/// A single expectation of an interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockExpectation {
    #[serde(default = "empty_id")]
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub conditions: Vec<ParamCondition>,
    #[serde(default)]
    pub ip_condition: IpCondition,
    pub status: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub body: String,
    #[serde(default)]
    pub delay_ms: u64,
}

/// An interface (routing key = method + path, data ownership key = request_id).
/// request_id binds the mock data to a specific request (data survives URL edits);
/// server-side matching still uses method + path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockInterface {
    /// Owning request id (unique key of the frontend request; when absent in old data, falls back to method+path)
    #[serde(default)]
    pub request_id: Option<String>,
    /// Owning workspace (mock rules are isolated per workspace; old data defaults to the default workspace)
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub method: String,
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub expectations: Vec<MockExpectation>,
}

impl MockInterface {
    /// Workspace ownership (old data defaults to the default workspace)
    pub fn ws(&self) -> &str {
        self.workspace_id
            .as_deref()
            .unwrap_or(orbit_data::DEFAULT_WORKSPACE_ID)
    }
}

fn default_true() -> bool {
    true
}
fn empty_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("exp-{:x}", n)
}

/// Request match input (the per-location params already parsed from the raw request).
pub struct MatchInput {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub path_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub cookies: HashMap<String, String>,
    pub body: Vec<u8>,
    pub content_type: String,
    pub client_ip: String,
}

/// CORS headers injected into all mock responses
fn add_cors_headers(resp: Response) -> Response {
    let (mut parts, body) = resp.into_parts();
    parts
        .headers
        .insert("access-control-allow-origin", "*".parse().unwrap());
    parts.headers.insert(
        "access-control-allow-methods",
        "GET, POST, PUT, DELETE, PATCH, OPTIONS".parse().unwrap(),
    );
    parts.headers.insert(
        "access-control-allow-headers",
        "Content-Type, Authorization, X-Requested-With"
            .parse()
            .unwrap(),
    );
    Response::from_parts(parts, body)
}

pub struct MockServer {
    interfaces: Arc<RwLock<Vec<MockInterface>>>,
    port: u16,
}

impl MockServer {
    pub fn new(port: u16) -> Self {
        Self {
            interfaces: Arc::new(RwLock::new(Vec::new())),
            port,
        }
    }

    /// Build with an externally shared interface Arc, enabling rules to be added/removed dynamically at runtime
    pub fn with_rules(port: u16, interfaces: Arc<RwLock<Vec<MockInterface>>>) -> Self {
        Self { interfaces, port }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let interfaces = self.interfaces.clone();
        let app = Router::new()
            .route("/{*path}", any(mock_handler))
            .with_state(interfaces);

        let addr = format!("0.0.0.0:{}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
        Ok(())
    }
}

#[axum::debug_handler]
async fn mock_handler(
    original_uri: OriginalUri,
    method: axum::http::Method,
    headers: axum::http::HeaderMap,
    connect_info: ConnectInfo<SocketAddr>,
    State(interfaces): State<Arc<RwLock<Vec<MockInterface>>>>,
    body: Bytes,
) -> Response {
    // OPTIONS preflight request: return 204 + CORS directly
    if method == axum::http::Method::OPTIONS {
        let ok: axum::body::Body = "".into();
        return add_cors_headers(Response::builder().status(204).body(ok).unwrap());
    }

    // Parse the match input
    let uri = original_uri.0;
    let path = uri.path().to_string();
    let query = parse_query(uri.query().unwrap_or(""));
    let headers_map = headers_to_map(&headers);
    let cookies = parse_cookies(headers_map.get("cookie").map(|s| s.as_str()).unwrap_or(""));
    let content_type = headers_map.get("content-type").cloned().unwrap_or_default();
    let client_ip = connect_info.0.ip().to_string();

    let input = MatchInput {
        method: method.to_string(),
        path,
        query,
        path_params: HashMap::new(), // filled in by match_interface
        headers: headers_map,
        cookies,
        body: body.to_vec(),
        content_type,
        client_ip,
    };

    let ifaces = interfaces.read().await;
    match match_interface(ifaces.as_slice(), &input) {
        Some((ii, ei)) => {
            // Clone the matched expectation and release the read lock immediately (avoid holding it during the delay)
            let exp = ifaces[ii].expectations[ei].clone();
            drop(ifaces);

            if exp.delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(exp.delay_ms)).await;
            }
            let mut resp = Response::builder().status(exp.status);
            for (k, v) in &exp.headers {
                resp = resp.header(k.as_str(), v.as_str());
            }
            // Resolve dynamic values like {{$guid}} in the example body.
            let body = orbit_dynamic::resolve(&exp.body).unwrap_or_else(|_| exp.body.clone());
            let b: axum::body::Body = body.into();
            add_cors_headers(resp.body(b).unwrap())
        }
        None => {
            drop(ifaces);
            let not_found: axum::body::Body = r#"{"error":"no mock expectation matched"}"#.into();
            add_cors_headers(Response::builder().status(404).body(not_found).unwrap())
        }
    }
}

/// Match a request against the interface list, returning the matched (interface index, expectation index).
pub fn match_interface(interfaces: &[MockInterface], input: &MatchInput) -> Option<(usize, usize)> {
    let req_method = input.method.to_uppercase();
    for (ii, iface) in interfaces.iter().enumerate() {
        if !iface.enabled {
            continue;
        }
        if iface.method.to_uppercase() != req_method {
            continue;
        }
        // Path mismatch: keep trying the next interface (cannot early-return with `?`,
        // otherwise a non-template rule earlier in the list would block a later template rule from matching)
        let path_params = match match_path(&iface.path, &input.path) {
            Some(pp) => pp,
            None => continue,
        };
        // Merge the template-extracted path params into input (effective only within this interface)
        let mut local_query = input.query.clone();
        let mut local_path_params = path_params;
        // Already-extracted path params take precedence
        for (k, v) in &input.path_params {
            local_path_params.entry(k.clone()).or_insert(v.clone());
        }

        // Find the first matching expectation within this interface
        for (ei, exp) in iface.expectations.iter().enumerate() {
            if !exp.enabled {
                continue;
            }
            // IP condition
            if exp.ip_condition.enabled
                && !exp.ip_condition.ip.is_empty()
                && exp.ip_condition.ip != input.client_ip
            {
                continue;
            }
            // Multiple param conditions (AND)
            let mut all_ok = true;
            for c in &exp.conditions {
                let actual = extract_value(
                    &c.location,
                    &c.name,
                    &input.query,
                    &local_path_params,
                    &input.headers,
                    &input.cookies,
                    &input.body,
                    &input.content_type,
                );
                if !eval_condition(c, actual) {
                    all_ok = false;
                    break;
                }
            }
            if all_ok {
                // Matched
                let _ = &mut local_query;
                return Some((ii, ei));
            }
        }
        // Interface matched but no expectation hit: keep trying the next interface
        // (there may be multiple records for the same method+path - different request ids sharing the same url,
        //   so later fallback interfaces also get a chance to match)
        continue;
    }
    None
}

/// Path matching: supports exact and `{var}` templates (multi-segment).
/// On match returns the extracted path params; otherwise None.
fn match_path(pattern: &str, actual: &str) -> Option<HashMap<String, String>> {
    let p = pattern.trim_matches('/');
    let a = actual.trim_matches('/');
    if p == a {
        return Some(HashMap::new());
    }
    if !p.contains('{') {
        return None;
    }
    let pseg: Vec<&str> = p.split('/').collect();
    let aseg: Vec<&str> = a.split('/').collect();
    if pseg.len() != aseg.len() {
        return None;
    }
    let mut params = HashMap::new();
    for (ps, as_) in pseg.iter().zip(aseg.iter()) {
        if let Some(name) = ps.strip_prefix('{') {
            let name = name.strip_suffix('}').unwrap_or(name);
            if name.is_empty() {
                return None;
            }
            params.insert(name.to_string(), (*as_).to_string());
        } else if ps != as_ {
            return None;
        }
    }
    Some(params)
}

/// Extract the value for a param name from the given location.
#[allow(clippy::too_many_arguments)]
fn extract_value(
    location: &str,
    name: &str,
    query: &HashMap<String, String>,
    path_params: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    cookies: &HashMap<String, String>,
    body: &[u8],
    content_type: &str,
) -> Option<String> {
    match location.to_lowercase().as_str() {
        "query" => query.get(name).cloned(),
        "path" => path_params.get(name).cloned(),
        "header" => headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone()),
        "cookie" => cookies.get(name).cloned(),
        "body" => extract_body_value(content_type, body, name),
        _ => None,
    }
}

/// Read a value from the body by field name (dot paths supported).
fn extract_body_value(content_type: &str, body: &[u8], name: &str) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    let body_str = String::from_utf8_lossy(body);
    if content_type.to_lowercase().contains("json")
        || body_str.trim_start().starts_with('{')
        || body_str.trim_start().starts_with('[')
    {
        let v: serde_json::Value = serde_json::from_str(&body_str).ok()?;
        let mut cur = &v;
        for part in name.split('.') {
            cur = cur.get(part)?;
        }
        return match cur {
            serde_json::Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        };
    }
    if content_type
        .to_lowercase()
        .contains("x-www-form-urlencoded")
    {
        let params = parse_query(&body_str);
        return params.get(name).cloned();
    }
    None
}

/// Numeric-first, string-fallback comparison (used for gt/gte/lt/lte):
/// If both sides parse as numbers, compare numerically; otherwise compare lexicographically.
fn compare_values(
    a: &str,
    b: &str,
    num_cmp: impl Fn(f64, f64) -> bool,
    str_cmp: impl Fn(&str, &str) -> bool,
) -> bool {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => num_cmp(x, y),
        _ => str_cmp(a, b),
    }
}

/// Evaluate a single param condition.
fn eval_condition(c: &ParamCondition, actual: Option<String>) -> bool {
    match c.op.as_str() {
        "equals" => actual.as_deref() == Some(c.value.as_str()),
        "not_equals" => actual.as_deref() != Some(c.value.as_str()),
        "contains" => actual.is_some_and(|a| a.contains(&c.value)),
        "not_contains" => actual.is_none_or(|a| !a.contains(&c.value)),
        "exists" => actual.is_some(),
        "not_exists" => actual.is_none(),
        "regex" => match Regex::new(&c.value) {
            Ok(re) => actual.is_some_and(|a| re.is_match(&a)),
            Err(_) => false,
        },
        "gt" => actual.is_some_and(|a| compare_values(&a, &c.value, |x, y| x > y, |x, y| x > y)),
        "gte" => actual.is_some_and(|a| compare_values(&a, &c.value, |x, y| x >= y, |x, y| x >= y)),
        "lt" => actual.is_some_and(|a| compare_values(&a, &c.value, |x, y| x < y, |x, y| x < y)),
        "lte" => actual.is_some_and(|a| compare_values(&a, &c.value, |x, y| x <= y, |x, y| x <= y)),
        _ => false,
    }
}

/// Convert a HeaderMap into a HashMap (preserving original key names; ignore case as needed when matching).
fn headers_to_map(headers: &axum::http::HeaderMap) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for (k, v) in headers.iter() {
        if let Ok(s) = v.to_str() {
            m.insert(k.as_str().to_string(), s.to_string());
        }
    }
    m
}

/// Parse a query / form-urlencoded string (minimal percent-decode).
fn parse_query(q: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    if q.is_empty() {
        return m;
    }
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        m.insert(percent_decode(k), percent_decode(v));
    }
    m
}

/// Parse the Cookie header.
fn parse_cookies(header_val: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for part in header_val.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            m.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    m
}

/// Minimal percent-decode (handles + and %XX).
fn percent_decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8 as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: empty expectation headers (the old module-level default_headers was test-only and moved into the test module)
    fn default_headers() -> HashMap<String, String> {
        HashMap::new()
    }

    fn iface_with_expectations(exps: Vec<MockExpectation>) -> MockInterface {
        MockInterface {
            request_id: None,
            workspace_id: None,
            method: "GET".into(),
            path: "/api/users".into(),
            enabled: true,
            expectations: exps,
        }
    }

    fn exp(
        name: &str,
        conditions: Vec<ParamCondition>,
        ip: Option<&str>,
        status: u16,
        body: &str,
    ) -> MockExpectation {
        MockExpectation {
            id: format!("exp-{}", name),
            name: name.into(),
            enabled: true,
            conditions,
            ip_condition: match ip {
                Some(ip) => IpCondition {
                    enabled: true,
                    ip: ip.into(),
                },
                None => IpCondition::default(),
            },
            status,
            headers: default_headers(),
            body: body.into(),
            delay_ms: 0,
        }
    }

    fn input(
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        headers: &[(&str, &str)],
        cookies: &[(&str, &str)],
        body: &str,
        ip: &str,
    ) -> MatchInput {
        let q: HashMap<String, String> = query
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let h: HashMap<String, String> = headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let c: HashMap<String, String> = cookies
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        MatchInput {
            method: method.into(),
            path: path.into(),
            query: q,
            path_params: HashMap::new(),
            headers: h,
            cookies: c,
            body: body.as_bytes().to_vec(),
            content_type: "application/json".into(),
            client_ip: ip.into(),
        }
    }

    #[test]
    fn test_no_expectation_matches() {
        let ifaces = vec![iface_with_expectations(vec![exp(
            "default",
            vec![],
            None,
            200,
            "ok",
        )])];
        let inp = input("GET", "/api/users", &[], &[], &[], "", "127.0.0.1");
        let (_, ei) = match_interface(&ifaces, &inp).unwrap();
        assert_eq!(ei, 0);
    }

    #[test]
    fn test_query_condition_and() {
        let ifaces = vec![iface_with_expectations(vec![
            exp(
                "admin",
                vec![
                    ParamCondition {
                        location: "query".into(),
                        name: "role".into(),
                        op: "equals".into(),
                        value: "admin".into(),
                    },
                    ParamCondition {
                        location: "header".into(),
                        name: "x-token".into(),
                        op: "exists".into(),
                        value: "".into(),
                    },
                ],
                None,
                200,
                "admin-body",
            ),
            exp("fallback", vec![], None, 200, "fb"),
        ])];
        // Matches admin
        let inp = input(
            "GET",
            "/api/users",
            &[("role", "admin")],
            &[("x-token", "abc")],
            &[],
            "",
            "127.0.0.1",
        );
        let (_, ei) = match_interface(&ifaces, &inp).unwrap();
        assert_eq!(ei, 0);
        // Missing header, matches fallback
        let inp2 = input(
            "GET",
            "/api/users",
            &[("role", "admin")],
            &[],
            &[],
            "",
            "127.0.0.1",
        );
        let (_, ei2) = match_interface(&ifaces, &inp2).unwrap();
        assert_eq!(ei2, 1);
    }

    #[test]
    fn test_numeric_compare_ops() {
        let ifaces = vec![iface_with_expectations(vec![
            exp(
                "gte100",
                vec![ParamCondition {
                    location: "query".into(),
                    name: "amount".into(),
                    op: "gte".into(),
                    value: "100".into(),
                }],
                None,
                200,
                "ge-100",
            ),
            exp(
                "lt100",
                vec![ParamCondition {
                    location: "query".into(),
                    name: "amount".into(),
                    op: "lt".into(),
                    value: "100".into(),
                }],
                None,
                200,
                "lt-100",
            ),
            exp("fallback", vec![], None, 200, "fb"),
        ])];
        // amount=150 -> matches gte
        let inp = input(
            "GET",
            "/api/users",
            &[("amount", "150")],
            &[],
            &[],
            "",
            "127.0.0.1",
        );
        let (_, ei) = match_interface(&ifaces, &inp).unwrap();
        assert_eq!(ei, 0);
        assert_eq!(ifaces[0].expectations[ei].body, "ge-100");
        // amount=50 -> matches lt
        let inp2 = input(
            "GET",
            "/api/users",
            &[("amount", "50")],
            &[],
            &[],
            "",
            "127.0.0.1",
        );
        let (_, ei2) = match_interface(&ifaces, &inp2).unwrap();
        assert_eq!(ei2, 1);
        // amount=100 -> matches gte (boundary value)
        let inp3 = input(
            "GET",
            "/api/users",
            &[("amount", "100")],
            &[],
            &[],
            "",
            "127.0.0.1",
        );
        let (_, ei3) = match_interface(&ifaces, &inp3).unwrap();
        assert_eq!(ei3, 0);
        // Missing param -> fallback
        let inp4 = input("GET", "/api/users", &[], &[], &[], "", "127.0.0.1");
        let (_, ei4) = match_interface(&ifaces, &inp4).unwrap();
        assert_eq!(ei4, 2);
        // String fallback (non-numeric compares lexicographically; note "v2" > "v1" and "v10" < "v9")
        let c = |op: &str, value: &str| ParamCondition {
            location: "query".into(),
            name: "v".into(),
            op: op.into(),
            value: value.into(),
        };
        assert!(eval_condition(&c("gt", "v1"), Some("v2".into())));
        assert!(!eval_condition(&c("gt", "v2"), Some("v1".into())));
        assert!(eval_condition(&c("lte", "v1"), Some("v1".into())));
    }

    #[test]
    fn test_body_condition_dot_path() {
        let ifaces = vec![iface_with_expectations(vec![exp(
            "byid",
            vec![ParamCondition {
                location: "body".into(),
                name: "user.id".into(),
                op: "equals".into(),
                value: "42".into(),
            }],
            None,
            201,
            "created",
        )])];
        let inp = input(
            "GET",
            "/api/users",
            &[],
            &[("content-type", "application/json")],
            &[],
            r#"{"user":{"id":42,"name":"x"}}"#,
            "127.0.0.1",
        );
        let (_, ei) = match_interface(&ifaces, &inp).unwrap();
        assert_eq!(ei, 0);
    }

    #[test]
    fn test_cookie_condition() {
        let ifaces = vec![iface_with_expectations(vec![exp(
            "session",
            vec![ParamCondition {
                location: "cookie".into(),
                name: "sid".into(),
                op: "equals".into(),
                value: "xyz".into(),
            }],
            None,
            200,
            "ok",
        )])];
        let inp = input(
            "GET",
            "/api/users",
            &[],
            &[],
            &[("sid", "xyz")],
            "",
            "127.0.0.1",
        );
        assert!(match_interface(&ifaces, &inp).is_some());
    }

    #[test]
    fn test_ip_condition_filters() {
        let ifaces = vec![iface_with_expectations(vec![
            exp("internal", vec![], Some("10.0.0.5"), 200, "internal"),
            exp("public", vec![], None, 200, "public"),
        ])];
        // From 10.0.0.5 -> matches internal
        let inp = input("GET", "/api/users", &[], &[], &[], "", "10.0.0.5");
        let (_, ei) = match_interface(&ifaces, &inp).unwrap();
        assert_eq!(ei, 0);
        // From another IP -> skips internal, matches public
        let inp2 = input("GET", "/api/users", &[], &[], &[], "", "192.168.1.2");
        let (_, ei2) = match_interface(&ifaces, &inp2).unwrap();
        assert_eq!(ei2, 1);
    }

    #[test]
    fn test_path_template_and_param() {
        let ifaces = vec![MockInterface {
            request_id: None,
            workspace_id: None,
            method: "GET".into(),
            path: "/api/users/{id}".into(),
            enabled: true,
            expectations: vec![exp(
                "one",
                vec![ParamCondition {
                    location: "path".into(),
                    name: "id".into(),
                    op: "equals".into(),
                    value: "7".into(),
                }],
                None,
                200,
                "user7",
            )],
        }];
        let inp = input("GET", "/api/users/7", &[], &[], &[], "", "127.0.0.1");
        let (_, ei) = match_interface(&ifaces, &inp).unwrap();
        assert_eq!(ei, 0);
        // Path does not match the template -> None
        let inp2 = input("GET", "/api/accounts/7", &[], &[], &[], "", "127.0.0.1");
        assert!(match_interface(&ifaces, &inp2).is_none());
    }

    #[test]
    fn test_path_condition_after_nonmatching_rule() {
        // Regression: a path-mismatching rule earlier in the list (e.g. the old template-less /health)
        // must not block a later template rule (/health/{id} + path condition id=2) from matching.
        let first = MockInterface {
            request_id: None,
            workspace_id: None,
            method: "GET".into(),
            path: "/health".into(),
            enabled: true,
            expectations: vec![exp("plain", vec![], None, 200, "plain")],
        };
        let second = MockInterface {
            request_id: Some("req-1".into()),
            workspace_id: None,
            method: "GET".into(),
            path: "/health/{id}".into(),
            enabled: true,
            expectations: vec![exp(
                "byid",
                vec![ParamCondition {
                    location: "path".into(),
                    name: "id".into(),
                    op: "equals".into(),
                    value: "2".into(),
                }],
                None,
                200,
                "hit-id-2",
            )],
        };
        let ifaces = vec![first, second];
        // Request /health/2: the first rule does not match, so it should fall through to the second and match the path condition
        let inp = input("GET", "/health/2", &[], &[], &[], "", "127.0.0.1");
        let (ii, ei) = match_interface(&ifaces, &inp).unwrap();
        assert_eq!(ii, 1);
        assert_eq!(ei, 0);
        assert_eq!(ifaces[1].expectations[ei].body, "hit-id-2");
        // Request /health/3: path condition unmet -> no expectation hit -> None
        let inp3 = input("GET", "/health/3", &[], &[], &[], "", "127.0.0.1");
        assert!(match_interface(&ifaces, &inp3).is_none());
    }
}
