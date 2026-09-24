//! Postman Collection import / export (v2.0 / v2.1)
//!
//! Coverage (baseline schema: schema.getpostman.com/json/collection/v2.1.0/collection.json):
//! - item tree (nested folders)
//! - request: method / url (raw + query) / header / body in all modes (raw/urlencoded/formdata/file/graphql)
//! - auth (collection level + item level: basic/bearer/apikey/oauth2)
//! - event: prerequest / test scripts
//! - response examples, collection-level variables
//! - the rest (certificate/proxy/digest·ntlm, etc.) pass through via extensions as a fallback

use serde_json::{json, Map, Value};
use std::collections::HashMap;

use super::ir::{ApiSpec, AuthSpec, EndpointSpec, ResponseSpec};
use super::{ExportError, ExportFormat, ImportError, ImportFormat};
use crate::model::request::{GraphqlConfig, HttpRequestConfig, RequestSpec};

/// Postman: import (Collection -> ApiSpec) + export (ApiSpec -> Collection v2.1)
pub(crate) struct PostmanImporter;

impl ImportFormat for PostmanImporter {
    fn name(&self) -> &'static str {
        "postman"
    }

    fn parse(&self, input: &str) -> Result<ApiSpec, ImportError> {
        let coll: PostmanCollection = serde_json::from_str(input)
            .map_err(|e| ImportError::Parse(format!("Invalid Postman Collection JSON: {}", e)))?;

        let mut spec = ApiSpec::new(coll.info.name.clone());
        spec.description = pm_description(&coll.info.description);
        // collection-level variables / auth
        for v in &coll.variable {
            if !v.key.is_empty() {
                spec.variables.insert(v.key.clone(), v.value.clone());
            }
        }
        let coll_auth = coll.auth.as_ref().and_then(auth_to_spec);
        flatten_items(&coll.item, &mut spec.endpoints, &mut Vec::new(), coll_auth);
        // pass through unstructured content (certificate/proxy/protocol behavior, etc.)
        if let Some(obj) = serde_json::from_str::<Value>(input)
            .ok()
            .and_then(|v| v.as_object().cloned())
        {
            spec.raw = obj;
        }
        Ok(spec)
    }
}

impl ExportFormat for PostmanImporter {
    fn name(&self) -> &'static str {
        "postman"
    }

    fn export(&self, spec: &ApiSpec) -> Result<String, ExportError> {
        let mut item = vec![];
        // build a nested item tree by group
        for ep in &spec.endpoints {
            let node = endpoint_to_pm_item(ep);
            insert_nested(&mut item, &ep.group, 0, node);
        }
        let collection = json!({
            "info": {
                "name": spec.name,
                "description": spec.description,
                "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
            },
            "item": item,
        });
        serde_json::to_string_pretty(&collection).map_err(|e| ExportError::Serialize(e.to_string()))
    }
}

// ─── Import ─────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
struct PostmanCollection {
    info: PostmanInfo,
    item: Vec<PostmanItem>,
    #[serde(default)]
    variable: Vec<PostmanVariable>,
    #[serde(default)]
    auth: Option<PostmanAuth>,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanInfo {
    name: String,
    /// Collection-level description (string or {content, type} object)
    #[serde(default)]
    description: Option<Value>,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanVariable {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: String,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanAuth {
    #[serde(rename = "type", default)]
    auth_type: Option<String>,
    /// Per-type attribute arrays (basic/bearer/apikey/oauth2/..., keyed by type name)
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// Postman auth -> auth type hint (basic/bearer/apikey/oauth2; otherwise None, pass through)
fn auth_to_spec(auth: &PostmanAuth) -> Option<AuthSpec> {
    let t = auth.auth_type.as_deref()?;
    let attrs = |name: &str| -> HashMap<String, String> {
        auth.extra
            .get(name)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| {
                        let k = a.get("key")?.as_str()?.to_string();
                        let v = a
                            .get("value")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        Some((k, v))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    match t {
        "basic" => Some(AuthSpec::Basic),
        "bearer" => Some(AuthSpec::Bearer),
        "apikey" => {
            let a = attrs("apikey");
            let name = a.get("key").cloned().unwrap_or_default();
            let location = a.get("in").cloned().unwrap_or_else(|| "header".to_string());
            Some(AuthSpec::ApiKey { name, location })
        }
        "oauth2" => Some(AuthSpec::OAuth2),
        _ => None,
    }
}

/// Postman description extraction (string or `{content, type}` object)
fn pm_description(v: &Option<Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        Some(Value::Object(o)) => o
            .get("content")
            .and_then(|c| c.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(String::from),
        _ => None,
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
enum PostmanItem {
    Folder {
        name: String,
        item: Vec<PostmanItem>,
    },
    Request(PostmanRequest),
}

#[derive(Debug, serde::Deserialize)]
struct PostmanRequest {
    name: String,
    request: PostmanRequestDetail,
    /// Item-level description (string or {content, type} object)
    #[serde(default)]
    description: Option<Value>,
    #[serde(default)]
    event: Vec<PostmanEvent>,
    #[serde(default)]
    auth: Option<PostmanAuth>,
    /// Saved response examples
    #[serde(default)]
    response: Vec<PostmanResponse>,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanResponse {
    #[serde(default)]
    code: u16,
    #[serde(default)]
    name: String,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanEvent {
    listen: String,
    script: Option<PostmanScript>,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanScript {
    exec: Option<Value>,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanRequestDetail {
    method: String,
    url: PostmanUrl,
    #[serde(default)]
    header: Vec<PostmanHeader>,
    body: Option<PostmanBody>,
    /// Request-level description (string or {content, type} object)
    #[serde(default)]
    description: Option<Value>,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanUrl {
    #[serde(default)]
    raw: Option<String>,
    #[serde(default)]
    query: Vec<PostmanQueryParam>,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanQueryParam {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    disabled: bool,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanHeader {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: String,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanBody {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    raw: Option<String>,
    /// urlencoded / formdata entries
    #[serde(default)]
    urlencoded: Vec<PostmanFormParam>,
    #[serde(default)]
    formdata: Vec<PostmanFormParam>,
    /// graphql：{ query, variables }
    #[serde(default)]
    graphql: Option<Value>,
}

#[derive(Debug, serde::Deserialize)]
struct PostmanFormParam {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: String,
    #[serde(rename = "type", default)]
    type_field: Option<String>,
    #[serde(default)]
    src: Option<String>,
}

impl PostmanFormParam {
    fn is_file(&self) -> bool {
        self.type_field.as_deref() == Some("file")
    }
}

fn postman_script(item: &PostmanRequest, listen: &str) -> Option<String> {
    for ev in &item.event {
        if ev.listen != listen {
            continue;
        }
        let exec = ev.script.as_ref().and_then(|s| s.exec.as_ref())?;
        let text = match exec {
            Value::String(s) => s.clone(),
            Value::Array(arr) => {
                let lines: Vec<&str> = arr.iter().filter_map(|l| l.as_str()).collect();
                if lines.is_empty() {
                    continue;
                }
                lines.join("\n")
            }
            _ => continue,
        };
        if !text.trim().is_empty() {
            return Some(text);
        }
    }
    None
}

fn flatten_items(
    items: &[PostmanItem],
    out: &mut Vec<EndpointSpec>,
    folder_path: &mut Vec<String>,
    coll_auth: Option<AuthSpec>,
) {
    for item in items {
        match item {
            PostmanItem::Folder { name, item } => {
                folder_path.push(name.clone());
                flatten_items(item, out, folder_path, coll_auth.clone());
                folder_path.pop();
            }
            PostmanItem::Request(req) => {
                let mut headers = HashMap::new();
                for h in &req.request.header {
                    if !h.key.is_empty() {
                        headers.insert(h.key.clone(), h.value.clone());
                    }
                }

                let mut ep = if req.request.body.as_ref().and_then(|b| b.mode.as_deref())
                    == Some("graphql")
                {
                    // graphql mode -> GraphqlConfig (multi-protocol capability of the protocol-agnostic IR)
                    let gql = req.request.body.as_ref().and_then(|b| b.graphql.as_ref());
                    let query = gql
                        .and_then(|g| g.get("query"))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    let variables = gql
                        .and_then(|g| g.get("variables"))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    EndpointSpec::new(
                        req.name.clone(),
                        RequestSpec::Graphql(GraphqlConfig {
                            url: req.request.url.raw.clone().unwrap_or_default(),
                            query,
                            variables,
                            operation_name: None,
                            headers,
                        }),
                    )
                } else {
                    // HTTP：raw body / urlencoded / formdata / file
                    let raw_body =
                        req.request
                            .body
                            .as_ref()
                            .and_then(|b| b.raw.as_ref())
                            .map(|r| {
                                if req.request.body.as_ref().and_then(|b| b.mode.as_deref())
                                    == Some("json")
                                    || r.starts_with('{')
                                    || r.starts_with('[')
                                {
                                    serde_json::from_str::<serde_yaml::Value>(r)
                                        .ok()
                                        .unwrap_or(serde_yaml::Value::String(r.clone()))
                                } else {
                                    serde_yaml::Value::String(r.clone())
                                }
                            });

                    let mode = req.request.body.as_ref().and_then(|b| b.mode.as_deref());
                    let mut content_type = match mode {
                        Some("json") => Some("application/json".to_string()),
                        Some("urlencoded") | Some("formdata") => {
                            Some("application/x-www-form-urlencoded".to_string())
                        }
                        _ => None,
                    };

                    // explicit query params (not disabled) -> display + append to url
                    let mut query_params = HashMap::new();
                    let mut url = req.request.url.raw.clone().unwrap_or_default();
                    let mut explicit_query: Vec<String> = Vec::new();
                    for q in &req.request.url.query {
                        if q.disabled || q.key.is_empty() {
                            continue;
                        }
                        query_params.insert(q.key.clone(), q.value.clone());
                        explicit_query.push(format!("{}={}", q.key, q.value));
                    }
                    if !explicit_query.is_empty() && !url.contains('?') {
                        url = format!("{}?{}", url, explicit_query.join("&"));
                    }

                    // urlencoded / formdata: text entries compose the body, file entries pass through
                    let mut form_body: Option<String> = None;
                    let mut form_files: Vec<Value> = Vec::new();
                    if let Some(b) = &req.request.body {
                        let params: &Vec<PostmanFormParam> = match mode {
                            Some("urlencoded") => &b.urlencoded,
                            Some("formdata") => &b.formdata,
                            _ => &b.urlencoded,
                        };
                        for p in params {
                            if p.key.is_empty() {
                                continue;
                            }
                            if p.is_file() {
                                form_files.push(json!({
                                    "key": p.key, "src": p.src.as_deref().unwrap_or(""),
                                }));
                            } else if let Some(fb) = &mut form_body {
                                fb.push_str(&format!("&{}={}", p.key, p.value));
                            } else {
                                form_body = Some(format!("{}={}", p.key, p.value));
                            }
                        }
                    }

                    let body = raw_body.or(form_body.map(serde_yaml::Value::String));
                    if body.is_none() && matches!(mode, Some("urlencoded") | Some("formdata")) {
                        content_type = None;
                    }

                    let mut ep = EndpointSpec::new(
                        req.name.clone(),
                        RequestSpec::Http(Box::new(HttpRequestConfig {
                            method: req.request.method.to_uppercase(),
                            url,
                            headers,
                            body,
                            timeout: "30s".to_string(),
                            payload_format: None,
                            grpc_service: None,
                            grpc_use_reflection: false,
                            response_format: None,
                        })),
                    );
                    ep.content_type = content_type;
                    ep.query_params = query_params;
                    if !form_files.is_empty() {
                        ep.extensions
                            .insert("formdata_files".into(), json!(form_files));
                    }
                    ep
                };

                ep.group = folder_path.clone();
                ep.description = pm_description(&req.description)
                    .or_else(|| pm_description(&req.request.description));
                ep.pre_script = postman_script(req, "prerequest");
                ep.post_script = postman_script(req, "test");
                // auth: item level takes precedence, otherwise collection level
                ep.auth = req
                    .auth
                    .as_ref()
                    .and_then(auth_to_spec)
                    .or_else(|| coll_auth.clone());
                // response examples
                for r in &req.response {
                    if r.code > 0 {
                        ep.responses.push(ResponseSpec {
                            status: r.code,
                            name: r.name.clone(),
                            body: r.body.clone().unwrap_or_default(),
                            schema: None,
                        });
                    }
                }
                out.push(ep);
            }
        }
    }
}

// ─── Export ─────────────────────────────────────────

fn endpoint_to_pm_item(ep: &EndpointSpec) -> Value {
    let mut request = Map::new();
    match &ep.request {
        RequestSpec::Http(h) => {
            request.insert("method".into(), json!(h.method));
            request.insert("url".into(), json!({ "raw": h.url }));
            let header: Vec<Value> = h
                .headers
                .iter()
                .map(|(k, v)| json!({ "key": k, "value": v }))
                .collect();
            if !header.is_empty() {
                request.insert("header".into(), json!(header));
            }
            // body export: urlencoded / formdata (incl. file) / raw selected by content shape
            if let Some(body) = &h.body {
                let raw = super::curl::body_to_text(body);
                let content_type = ep
                    .content_type
                    .clone()
                    .or_else(|| {
                        h.headers
                            .iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                            .map(|(_, v)| v.clone())
                    })
                    .unwrap_or_default();
                let files = ep
                    .extensions
                    .get("formdata_files")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                if content_type == "application/x-www-form-urlencoded" && !files.is_empty() {
                    // formdata: text entries (parsed from body) + file entries (from pass-through)
                    let mut formdata: Vec<Value> = urlencoded_pairs(&raw)
                        .into_iter()
                        .map(|(k, v)| json!({ "key": k, "value": v, "type": "text" }))
                        .collect();
                    for f in files {
                        let mut item = Map::new();
                        item.insert("key".into(), f.get("key").cloned().unwrap_or(Value::Null));
                        item.insert("type".into(), json!("file"));
                        if let Some(src) = f.get("src") {
                            item.insert("src".into(), src.clone());
                        }
                        formdata.push(Value::Object(item));
                    }
                    request.insert(
                        "body".into(),
                        json!({ "mode": "formdata", "formdata": formdata }),
                    );
                } else if content_type == "application/x-www-form-urlencoded" {
                    let pairs: Vec<Value> = urlencoded_pairs(&raw)
                        .into_iter()
                        .map(|(k, v)| json!({ "key": k, "value": v, "type": "text" }))
                        .collect();
                    request.insert(
                        "body".into(),
                        json!({ "mode": "urlencoded", "urlencoded": pairs }),
                    );
                } else {
                    request.insert("body".into(), json!({ "mode": "raw", "raw": raw }));
                }
            }
        }
        _ => {
            request.insert("method".into(), json!("GET"));
            request.insert("url".into(), json!({ "raw": "" }));
        }
    }
    let mut item = Map::new();
    item.insert("name".into(), json!(ep.name));
    item.insert("request".into(), Value::Object(request));
    if let Some(desc) = &ep.description {
        if !desc.is_empty() {
            item.insert("description".into(), json!(desc));
        }
    }
    let mut events: Vec<Value> = Vec::new();
    if let Some(pre) = &ep.pre_script {
        events.push(json!({
            "listen": "prerequest",
            "script": { "type": "text/javascript", "exec": pre.lines().map(String::from).collect::<Vec<_>>() }
        }));
    }
    if let Some(post) = &ep.post_script {
        events.push(json!({
            "listen": "test",
            "script": { "type": "text/javascript", "exec": post.lines().map(String::from).collect::<Vec<_>>() }
        }));
    }
    if !events.is_empty() {
        item.insert("event".into(), json!(events));
    }
    Value::Object(item)
}

/// Parse a urlencoded string (`a=1&b=2`) -> (key, value) pairs.
fn urlencoded_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let (k, v) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            if k.is_empty() {
                None
            } else {
                Some((k.to_string(), v.to_string()))
            }
        })
        .collect()
}

/// Recursively insert nested folders along the group path (postman collection tree)
fn insert_nested(items: &mut Vec<Value>, group: &[String], depth: usize, node: Value) {
    if let Some(dir) = group.get(depth) {
        let found = items
            .iter()
            .position(|it| it["name"].as_str() == Some(dir.as_str()));
        match found {
            Some(idx) => {
                let children = items[idx]["item"]
                    .as_array_mut()
                    .expect("folder item array");
                insert_nested(children, group, depth + 1, node);
            }
            None => {
                let mut f = Map::new();
                f.insert("name".into(), json!(dir));
                f.insert("item".into(), json!([]));
                items.push(Value::Object(f));
                let last = items.last_mut().unwrap();
                insert_nested(last["item"].as_array_mut().unwrap(), group, depth + 1, node);
            }
        }
    } else {
        items.push(node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postman_import() {
        let json = r#"{
            "info": {"name": "Test API"},
            "item": [
                {
                    "name": "Get Users",
                    "request": {
                        "method": "GET",
                        "url": {"raw": "https://api.example.com/users"},
                        "header": [{"key": "Accept", "value": "application/json"}],
                        "body": null
                    }
                }
            ]
        }"#;
        let spec = PostmanImporter.parse(json).unwrap();
        assert_eq!(spec.name, "Test API");
        assert_eq!(spec.endpoints.len(), 1);
        let ep = &spec.endpoints[0];
        assert_eq!(ep.name, "Get Users");
        match &ep.request {
            RequestSpec::Http(h) => assert_eq!(h.url, "https://api.example.com/users"),
            _ => panic!("expected Http"),
        }
    }

    /// Scripts must be preserved in EndpointSpec.pre_script / post_script.
    #[test]
    fn test_postman_import_preserves_scripts() {
        let json = r#"{
            "info": {"name": "Auth API", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
            "item": [
                {
                    "name": "Create Order",
                    "event": [
                        {"listen": "prerequest", "script": {"type": "text/javascript", "exec": ["pm.environment.set(\"token\", \"abc123\");"]}},
                        {"listen": "test", "script": {"type": "text/javascript", "exec": ["pm.response.to.have.status(200);"]}}
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
        let spec = PostmanImporter.parse(json).unwrap();
        let ep = &spec.endpoints[0];
        assert!(ep
            .pre_script
            .as_deref()
            .unwrap_or("")
            .contains("pm.environment.set"));
        assert!(ep
            .post_script
            .as_deref()
            .unwrap_or("")
            .contains("pm.response.to.have.status"));
    }

    /// Import -> export roundtrip: endpoints and groups are not lost.
    #[test]
    fn test_postman_roundtrip() {
        let json = r#"{
            "info": {"name": "T"},
            "item": [
                {"name": "FolderA", "item": [
                    {"name": "Inner", "request": {"method": "GET", "url": {"raw": "https://a.example.com/inner"}, "header": [], "body": null}}
                ]},
                {"name": "Plain", "request": {"method": "POST", "url": {"raw": "https://a.example.com/plain"}, "header": [], "body": {"mode": "raw", "raw": "{\"x\":1}"}}}
            ]
        }"#;
        let spec = PostmanImporter.parse(json).unwrap();
        assert_eq!(spec.endpoints.len(), 2);
        assert_eq!(spec.endpoints[0].group, vec!["FolderA".to_string()]);

        let out = PostmanImporter.export(&spec).unwrap();
        assert!(out.contains("FolderA"), "{}", out);
        assert!(out.contains("https://a.example.com/plain"), "{}", out);
    }

    /// url object: query split out + disabled skipped; urlencoded body
    #[test]
    fn test_postman_url_query_and_urlencoded() {
        let json = r#"{
            "info": {"name": "T"},
            "item": [{
                "name": "Form",
                "request": {
                    "method": "POST",
                    "url": {
                        "raw": "https://a.example.com/f",
                        "query": [
                            {"key": "page", "value": "2"},
                            {"key": "off", "value": "1", "disabled": true}
                        ]
                    },
                    "header": [],
                    "body": {"mode": "urlencoded", "urlencoded": [{"key": "a", "value": "1"}, {"key": "b", "value": "2"}]}
                }
            }]
        }"#;
        let spec = PostmanImporter.parse(json).unwrap();
        let ep = &spec.endpoints[0];
        assert_eq!(ep.query_params.get("page").unwrap(), "2");
        assert!(
            !ep.query_params.contains_key("off"),
            "disabled query should be skipped"
        );
        match &ep.request {
            RequestSpec::Http(h) => {
                assert!(h.url.contains("?page=2"), "{}", h.url);
                assert_eq!(h.body.as_ref().unwrap().as_str().unwrap(), "a=1&b=2");
            }
            _ => panic!("expected Http"),
        }
        assert_eq!(
            ep.content_type.as_deref(),
            Some("application/x-www-form-urlencoded")
        );
    }

    /// formdata: text entries go into body, file entries pass through
    #[test]
    fn test_postman_formdata_file() {
        let json = r#"{
            "info": {"name": "T"},
            "item": [{
                "name": "Up",
                "request": {
                    "method": "POST",
                    "url": {"raw": "https://a.example.com/up"},
                    "header": [],
                    "body": {"mode": "formdata", "formdata": [
                        {"key": "field", "value": "v1", "type": "text"},
                        {"key": "file", "type": "file", "src": "/tmp/x.png"}
                    ]}
                }
            }]
        }"#;
        let spec = PostmanImporter.parse(json).unwrap();
        let ep = &spec.endpoints[0];
        match &ep.request {
            RequestSpec::Http(h) => {
                assert_eq!(h.body.as_ref().unwrap().as_str().unwrap(), "field=v1")
            }
            _ => panic!("expected Http"),
        }
        let files = ep
            .extensions
            .get("formdata_files")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["key"], "file");
    }

    /// graphql mode → RequestSpec::Graphql
    #[test]
    fn test_postman_graphql() {
        let json = r#"{
            "info": {"name": "T"},
            "item": [{
                "name": "GQL",
                "request": {
                    "method": "POST",
                    "url": {"raw": "https://g.example.com/graphql"},
                    "header": [],
                    "body": {"mode": "graphql", "graphql": {"query": "{ user { id } }", "variables": "{\"id\":1}"}}
                }
            }]
        }"#;
        let spec = PostmanImporter.parse(json).unwrap();
        match &spec.endpoints[0].request {
            RequestSpec::Graphql(g) => {
                assert_eq!(g.query.as_deref(), Some("{ user { id } }"));
                assert_eq!(g.variables.as_deref(), Some(r#"{"id":1}"#));
            }
            other => panic!("expected Graphql, got {:?}", other),
        }
    }

    /// collection-level auth / variables / response examples
    #[test]
    fn test_postman_auth_variables_response() {
        let json = r#"{
            "info": {"name": "T"},
            "variable": [{"key": "base_url", "value": "https://api.example.com"}],
            "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "xxx", "type": "string"}]},
            "item": [{
                "name": "Me",
                "request": {"method": "GET", "url": {"raw": "{{base_url}}/me"}, "header": [], "body": null},
                "response": [{"code": 200, "name": "ok", "body": "{\"id\":1}"}]
            }]
        }"#;
        let spec = PostmanImporter.parse(json).unwrap();
        assert_eq!(
            spec.variables.get("base_url").unwrap(),
            "https://api.example.com"
        );
        assert_eq!(spec.endpoints[0].auth, Some(AuthSpec::Bearer));
        assert_eq!(spec.endpoints[0].responses.len(), 1);
        assert_eq!(spec.endpoints[0].responses[0].status, 200);
    }

    /// description: collection level (string / {content} object) + item level, both import and export directions
    #[test]
    fn test_postman_description_roundtrip() {
        let json = r#"{
            "info": {"name": "T", "description": "Collection description"},
            "item": [{
                "name": "Get",
                "description": {"content": "item description", "type": "text/plain"},
                "request": {
                    "method": "GET",
                    "url": {"raw": "https://a.example.com/x"},
                    "description": "request description",
                    "header": [],
                    "body": null
                }
            }]
        }"#;
        let spec = PostmanImporter.parse(json).unwrap();
        // collection level -> ApiSpec.description; item level takes precedence over request level
        assert_eq!(spec.description.as_deref(), Some("Collection description"));
        assert_eq!(
            spec.endpoints[0].description.as_deref(),
            Some("item description")
        );

        // export roundtrip: description preserved
        let out = PostmanImporter.export(&spec).unwrap();
        assert!(out.contains("Collection description"), "{}", out);
        assert!(out.contains("item description"), "{}", out);
    }

    /// Export: urlencoded body -> urlencoded mode; formdata (incl. file) -> formdata mode
    #[test]
    fn test_postman_export_body_modes() {
        // urlencoded
        let mut ep = EndpointSpec::new(
            "Form",
            RequestSpec::Http(Box::new(HttpRequestConfig {
                method: "POST".into(),
                url: "https://a.example.com/f".into(),
                headers: HashMap::new(),
                body: Some(serde_yaml::Value::String("a=1&b=2".into())),
                timeout: "30s".into(),
                payload_format: None,
                grpc_service: None,
                grpc_use_reflection: false,
                response_format: None,
            })),
        );
        ep.content_type = Some("application/x-www-form-urlencoded".into());
        let item = endpoint_to_pm_item(&ep);
        let body = &item["request"]["body"];
        assert_eq!(body["mode"], "urlencoded");
        assert_eq!(body["urlencoded"].as_array().unwrap().len(), 2);
        assert_eq!(body["urlencoded"][0]["key"], "a");

        // formdata (text + file)
        ep.extensions.insert(
            "formdata_files".into(),
            json!([{ "key": "file", "src": "/tmp/x.png" }]),
        );
        let item = endpoint_to_pm_item(&ep);
        let body = &item["request"]["body"];
        assert_eq!(body["mode"], "formdata");
        let fd = body["formdata"].as_array().unwrap();
        assert_eq!(fd.len(), 3); // 2 text + 1 file
        assert_eq!(fd[0]["type"], "text");
        assert_eq!(fd[2]["type"], "file");
        assert_eq!(fd[2]["src"], "/tmp/x.png");

        // raw fallback
        let raw_ep = EndpointSpec::new(
            "Raw",
            RequestSpec::Http(Box::new(HttpRequestConfig {
                method: "POST".into(),
                url: "https://a.example.com/r".into(),
                headers: HashMap::new(),
                body: Some(serde_yaml::Value::String("{\"x\":1}".into())),
                timeout: "30s".into(),
                payload_format: None,
                grpc_service: None,
                grpc_use_reflection: false,
                response_format: None,
            })),
        );
        let item = endpoint_to_pm_item(&raw_ep);
        assert_eq!(item["request"]["body"]["mode"], "raw");
    }
}
