//! OpenAPI 3.0 / Swagger 2.0 import and export

mod export;
mod shared;

use serde_json::{Map, Value};
use std::collections::HashMap;

use super::ir::{ApiSpec, EndpointSpec, ModelSpec};
use super::{ExportError, ExportFormat, ImportError, ImportFormat};
use crate::model::request::{HttpRequestConfig, RequestSpec};
use shared::*;

/// OpenAPI: import (document -> ApiSpec)
pub(crate) struct OpenApiImporter;

impl ImportFormat for OpenApiImporter {
    fn name(&self) -> &'static str {
        "openapi"
    }

    fn parse(&self, input: &str) -> Result<ApiSpec, ImportError> {
        let api: Value = serde_json::from_str(input).or_else(|_| {
            serde_yaml::from_str(input).map_err(|e| {
                ImportError::Parse(format!(
                    "Invalid OpenAPI/Swagger spec (tried JSON and YAML): {}",
                    e
                ))
            })
        })?;
        let is_sw2 = api["swagger"]
            .as_str()
            .map(|s| s.starts_with("2."))
            .unwrap_or(false);
        let mut spec = if is_sw2 {
            parse_sw2(&api)?
        } else {
            parse_oas3(&api)
        };
        // L3 pass-through: the entire source document is preserved as-is (full callbacks/links/encoding/components, etc.)
        if let Some(obj) = api.as_object() {
            spec.raw = obj.clone();
        }
        Ok(spec)
    }
}

/// OpenAPI exporter ("openapi" = 3.x; "swagger" = 2.0; both instances share the implementation)
pub(crate) struct OpenApiExporter(pub(crate) &'static str);

impl ExportFormat for OpenApiExporter {
    fn name(&self) -> &'static str {
        self.0
    }

    fn export(&self, spec: &ApiSpec) -> Result<String, ExportError> {
        if self.0 == "swagger" {
            export::to_swagger2(spec)
        } else {
            export::to_openapi3(spec)
        }
    }
}

// ─── OAS3 import ─────────────────────────────────────────

fn parse_oas3(api: &Value) -> ApiSpec {
    let title = api["info"]["title"].as_str().unwrap_or("OpenAPI Import");
    // {var} in the server url -> app variable {{var}}
    let server_prefix = api["servers"]
        .as_array()
        .and_then(|s| s.first())
        .and_then(|s| s["url"].as_str())
        .map(to_app_var)
        .unwrap_or_default();
    let security_schemes = api["components"]["securitySchemes"].as_object();
    // fallback when global security is missing: if securitySchemes has exactly one scheme -> apply it automatically
    let global_security = api
        .get("security")
        .and_then(pick_security_scheme)
        .or_else(|| sole_scheme_name(security_schemes))
        .and_then(|n| auth_from_scheme(Some(&n), security_schemes));

    let mut spec = ApiSpec::new(title);
    spec.description = api["info"]["description"]
        .as_str()
        .map(String::from)
        .filter(|s| !s.is_empty());
    spec.auth = global_security;

    if let Some(paths) = api["paths"].as_object() {
        for (path, methods) in paths {
            for (method, op) in methods.as_object().unwrap_or(&Map::new()) {
                if !is_http_method(method) {
                    continue;
                }
                let name = op["summary"]
                    .as_str()
                    .or(op["operationId"].as_str())
                    .unwrap_or("")
                    .to_string();
                let name = if name.is_empty() {
                    format!("{} {}", method.to_uppercase(), path)
                } else {
                    name
                };
                let full_url = if server_prefix.is_empty() {
                    path.clone()
                } else {
                    format!("{}{}", server_prefix.trim_end_matches('/'), path)
                };
                let tags: Vec<String> = op["tags"]
                    .as_array()
                    .map(|t| {
                        t.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                // auth: operation-level security overrides global; security: [] means no auth
                let auth = if op.get("security").is_some() {
                    pick_security_scheme(&op["security"])
                        .and_then(|n| auth_from_scheme(Some(&n), security_schemes))
                } else {
                    None
                };

                // request body (example -> examples[0] -> schema generation)
                let mut content_type = None;
                let mut body_str = String::new();
                let mut model_ref = None;
                if let Some(rb) = op.get("requestBody") {
                    let content = rb.get("content").and_then(|c| c.as_object());
                    let media_entry = content.and_then(|c| {
                        if let Some(j) = c.get("application/json") {
                            Some(("application/json", j))
                        } else {
                            c.iter().next().map(|(k, v)| (k.as_str(), v))
                        }
                    });
                    if let Some((media, media_obj)) = media_entry {
                        content_type = Some(media.to_string());
                        if let Some(ex) = media_obj.get("example") {
                            body_str = serde_json::to_string_pretty(ex).unwrap_or_default();
                        } else if let Some(exs) =
                            media_obj.get("examples").and_then(|e| e.as_object())
                        {
                            // 3.1: examples is a named map; take the first inline value
                            if let Some((_, ex_obj)) = exs.iter().next() {
                                if let Some(v) = ex_obj.get("value") {
                                    body_str = serde_json::to_string_pretty(v).unwrap_or_default();
                                }
                            }
                        }
                        if body_str.is_empty() {
                            if let Some(schema) = media_obj.get("schema") {
                                if let Some(ref_str) = schema.get("$ref").and_then(|v| v.as_str()) {
                                    model_ref = Some(ref_name_from_ref(ref_str));
                                }
                                let resolved = resolve_ref(schema, api);
                                body_str = serde_json::to_string_pretty(
                                    &generate_example_from_schema(resolved, api),
                                )
                                .unwrap_or_default();
                            }
                        }
                    }
                }

                // header / cookie params (OAS3: required header; cookie merged into the Cookie header)
                let mut headers = HashMap::new();
                let mut cookie_pairs: Vec<(String, String)> = Vec::new();
                let all_params: [Option<&Vec<Value>>; 2] = [
                    methods.get("parameters").and_then(|p| p.as_array()),
                    op.get("parameters").and_then(|p| p.as_array()),
                ];
                for params in all_params.iter().flatten() {
                    for param in *params {
                        match param["in"].as_str() {
                            Some("header") => {
                                if param["required"].as_bool().unwrap_or(false) {
                                    let pname = param["name"].as_str().unwrap_or("");
                                    let val = param["schema"]["example"]
                                        .as_str()
                                        .or_else(|| param["example"].as_str())
                                        .or_else(|| param["schema"]["default"].as_str());
                                    if !pname.is_empty() {
                                        if let Some(v) = val {
                                            headers.insert(pname.to_string(), v.to_string());
                                        }
                                    }
                                }
                            }
                            Some("cookie") => {
                                let pname = param["name"].as_str().unwrap_or("");
                                if !pname.is_empty() {
                                    let val = param["schema"]["example"]
                                        .as_str()
                                        .or_else(|| param["schema"]["default"].as_str())
                                        .unwrap_or("");
                                    cookie_pairs.push((pname.to_string(), val.to_string()));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if !cookie_pairs.is_empty() {
                    let joined = cookie_pairs
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect::<Vec<_>>()
                        .join("; ");
                    headers.entry("Cookie".to_string()).or_insert(joined);
                }

                let mut ep = EndpointSpec::new(
                    name,
                    RequestSpec::Http(Box::new(HttpRequestConfig {
                        method: method.to_uppercase(),
                        url: full_url,
                        headers,
                        body: if body_str.is_empty() {
                            None
                        } else {
                            Some(serde_yaml::Value::String(body_str.clone()))
                        },
                        timeout: "30s".to_string(),
                        payload_format: None,
                        grpc_service: None,
                        grpc_use_reflection: false,
                        response_format: None,
                    })),
                );
                ep.group = tags;
                ep.description = op["summary"]
                    .as_str()
                    .map(String::from)
                    .filter(|s| !s.is_empty());
                ep.auth = auth;
                ep.content_type = content_type;
                ep.model_ref = model_ref;
                ep.responses = extract_responses_oas3(&op["responses"], api);
                ep.pre_script = opt_script(op, PRE_SCRIPT_KEYS);
                ep.post_script = opt_script(op, POST_SCRIPT_KEYS);
                if op
                    .get("deprecated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    ep.extensions.insert("deprecated".into(), Value::Bool(true));
                }
                if let Some(callbacks) = op.get("callbacks") {
                    ep.extensions.insert("callbacks".into(), callbacks.clone());
                }
                if let Some(links) = op.get("links") {
                    ep.extensions.insert("links".into(), links.clone());
                }
                spec.endpoints.push(ep);
            }
        }
    }

    if let Some(comp) = api["components"]["schemas"].as_object() {
        for (name, schema) in comp {
            spec.models.push(ModelSpec {
                name: name.clone(),
                schema: schema.clone(),
            });
        }
    }
    // 3.1 webhooks (callback endpoints with no fixed method/url -> preserved via pass-through)
    if let Some(w) = api.get("webhooks") {
        spec.extensions.insert("webhooks".into(), w.clone());
    }
    spec
}

// ─── Swagger2 import ───────────────────────────────────────

fn parse_sw2(api: &Value) -> Result<ApiSpec, ImportError> {
    let title = api["info"]["title"].as_str().unwrap_or("Swagger Import");
    let base = api["basePath"].as_str().unwrap_or("");
    let host = api["host"].as_str().unwrap_or("");
    let base_url = if host.is_empty() {
        String::new()
    } else if host.contains('{') || host.contains('}') {
        // host is a variable placeholder: no scheme prefix is prepended; the variable is preserved for the frontend to resolve
        format!("{}{}", host, base)
    } else {
        let scheme = api["schemes"]
            .as_array()
            .and_then(|s| s.first())
            .and_then(|s| s.as_str())
            .unwrap_or("http");
        format!("{}://{}{}", scheme, host, base)
    };
    let global_consumes = api["consumes"].as_array();
    let security_defs = api["securityDefinitions"].as_object();
    let global_security = api
        .get("security")
        .and_then(pick_security_scheme)
        .or_else(|| sole_scheme_name(security_defs))
        .and_then(|n| sw2_auth_from_scheme(Some(&n), security_defs));

    let mut spec = ApiSpec::new(title);
    spec.description = api["info"]["description"]
        .as_str()
        .map(String::from)
        .filter(|s| !s.is_empty());
    spec.auth = global_security;

    if let Some(paths) = api["paths"].as_object() {
        for (path, methods) in paths {
            let path_params_arr = methods
                .get("parameters")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();
            for (method, op) in methods.as_object().unwrap_or(&Map::new()) {
                if !is_http_method(method) {
                    continue;
                }
                let name = op["summary"]
                    .as_str()
                    .or(op["operationId"].as_str())
                    .unwrap_or("")
                    .to_string();
                let name = if name.is_empty() {
                    format!("{} {}", method.to_uppercase(), path)
                } else {
                    name
                };
                let tags: Vec<String> = op["tags"]
                    .as_array()
                    .map(|t| {
                        t.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let consumes = op["consumes"].as_array().or(global_consumes);
                let ct = consumes
                    .and_then(|c| c.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/json");
                let op_params_arr = op["parameters"].as_array().cloned().unwrap_or_default();

                let auth = if op.get("security").is_some() {
                    pick_security_scheme(&op["security"])
                        .and_then(|n| sw2_auth_from_scheme(Some(&n), security_defs))
                } else {
                    None
                };

                let mut headers = HashMap::new();
                let mut query_params = HashMap::new();
                let mut body_str = String::new();
                let mut content_type = None;
                let mut model_ref = None;
                let mut form_params: Vec<(String, String)> = Vec::new();

                for param in path_params_arr.iter().chain(op_params_arr.iter()) {
                    match param.get("in").and_then(|v| v.as_str()) {
                        Some("header") => {
                            if let Some(n) = param.get("name").and_then(|v| v.as_str()) {
                                let val = param
                                    .get("default")
                                    .and_then(val_str)
                                    .or_else(|| param.get("x-example").and_then(val_str))
                                    .unwrap_or_default();
                                headers.insert(n.to_string(), val);
                            }
                        }
                        Some("query") => {
                            if let Some(n) = param.get("name").and_then(|v| v.as_str()) {
                                let val = param
                                    .get("default")
                                    .and_then(val_str)
                                    .or_else(|| param.get("x-example").and_then(val_str))
                                    .unwrap_or_default();
                                query_params.insert(n.to_string(), val);
                            }
                        }
                        Some("body") => {
                            if let Some(schema) = param.get("schema") {
                                if let Some(ref_str) = schema.get("$ref").and_then(|v| v.as_str()) {
                                    model_ref = Some(ref_name_from_ref(ref_str));
                                }
                                let resolved = resolve_ref(schema, api);
                                body_str = serde_json::to_string_pretty(
                                    &generate_example_from_schema(resolved, api),
                                )
                                .unwrap_or_default();
                            }
                        }
                        Some("formData") => {
                            if let Some(n) = param.get("name").and_then(|v| v.as_str()) {
                                let val = param
                                    .get("default")
                                    .and_then(val_str)
                                    .or_else(|| param.get("x-example").and_then(val_str))
                                    .unwrap_or_default();
                                form_params.push((n.to_string(), val));
                            }
                        }
                        _ => {}
                    }
                }

                if !form_params.is_empty() && body_str.is_empty() {
                    content_type = Some("application/x-www-form-urlencoded".into());
                    body_str = form_params
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect::<Vec<_>>()
                        .join("&");
                } else if !body_str.is_empty() {
                    content_type = Some(ct.to_string());
                }

                let url = if base_url.is_empty() {
                    path.clone()
                } else {
                    format!("{}{}", base_url.trim_end_matches('/'), path)
                };
                let mut ep = EndpointSpec::new(
                    name,
                    RequestSpec::Http(Box::new(HttpRequestConfig {
                        method: method.to_uppercase(),
                        url,
                        headers,
                        body: if body_str.is_empty() {
                            None
                        } else {
                            Some(serde_yaml::Value::String(body_str.clone()))
                        },
                        timeout: "30s".to_string(),
                        payload_format: None,
                        grpc_service: None,
                        grpc_use_reflection: false,
                        response_format: None,
                    })),
                );
                ep.group = tags;
                ep.description = op["summary"]
                    .as_str()
                    .map(String::from)
                    .filter(|s| !s.is_empty());
                ep.auth = auth;
                ep.content_type = content_type;
                ep.model_ref = model_ref;
                ep.query_params = query_params;
                ep.responses = extract_responses_sw2(&op["responses"], api);
                ep.pre_script = opt_script(op, PRE_SCRIPT_KEYS);
                ep.post_script = opt_script(op, POST_SCRIPT_KEYS);
                spec.endpoints.push(ep);
            }
        }
    }

    if spec.endpoints.is_empty() {
        return Err(ImportError::Parse(
            "No paths found in Swagger spec".to_string(),
        ));
    }
    if let Some(defs) = api["definitions"].as_object() {
        for (name, schema) in defs {
            spec.models.push(ModelSpec {
                name: name.clone(),
                schema: schema.clone(),
            });
        }
    }
    Ok(spec)
}

fn opt_script(op: &Value, keys: &[&str]) -> Option<String> {
    let s = script_from_extension(op, keys);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_import() {
        let spec = r#"{
            "openapi": "3.0.0",
            "info": {"title": "Test API", "version": "1.0"},
            "servers": [{"url": "https://api.example.com/v1"}],
            "paths": {
                "/users": {"get": {"summary": "List users"}, "post": {"summary": "Create user"}},
                "/users/{id}": {"get": {"summary": "Get user"}}
            }
        }"#;
        let parsed = OpenApiImporter.parse(spec).unwrap();
        assert_eq!(parsed.name, "Test API");
        assert_eq!(parsed.endpoints.len(), 3);
        assert_eq!(
            parsed.endpoints[0].url_of(),
            "https://api.example.com/v1/users"
        );
    }

    #[test]
    fn test_openapi_scenario_import_preserves_scripts() {
        let spec = r#"{
            "openapi": "3.0.3",
            "info": {"title": "Demo", "version": "1.0.0"},
            "paths": {
                "/orders": {
                    "post": {
                        "summary": "Create order",
                        "x-orbit-prerequest": "pm.environment.set(\"t\", \"1\");",
                        "x-orbit-postrequest": "pm.test(\"ok\", () => { pm.expect(true).to.be.true; });",
                        "responses": {"200": {"description": "ok"}}
                    }
                }
            }
        }"#;
        let parsed = OpenApiImporter.parse(spec).unwrap();
        let ep = &parsed.endpoints[0];
        assert!(ep
            .pre_script
            .as_deref()
            .unwrap_or("")
            .contains("pm.environment.set"));
        assert!(ep
            .post_script
            .as_deref()
            .unwrap_or("")
            .contains("pm.expect(true)"));
    }

    /// server variable {base_url} -> {{base_url}}
    #[test]
    fn test_oas3_roundtrip_base_url_variable() {
        let doc = r#"
openapi: 3.0.3
info: {title: Test, version: 1.0.0}
servers:
  - url: '{base_url}'
paths:
  /analytics/channels:
    get:
      summary: Channels
      responses:
        '200': {description: ok}
"#;
        let parsed = OpenApiImporter.parse(doc).unwrap();
        assert_eq!(
            parsed.endpoints[0].url_of(),
            "{{base_url}}/analytics/channels"
        );
    }

    /// Swagger2: when host is the {{base_url}} variable, no scheme prefix is prepended.
    #[test]
    fn test_sw2_variable_host_no_scheme_prefix() {
        let doc = r#"
swagger: '2.0'
info: {title: T, version: 1.0.0}
host: '{{base_url}}'
basePath: ''
schemes: []
paths:
  /pets:
    get:
      responses:
        '200': {description: ok}
"#;
        let parsed = OpenApiImporter.parse(doc).unwrap();
        assert_eq!(parsed.endpoints[0].url_of(), "{{base_url}}/pets");
    }

    #[test]
    fn test_export_openapi3() {
        let doc = r#"
openapi: 3.0.3
info: {title: T, version: 1.0.0}
paths:
  /pets:
    get:
      summary: List
      responses:
        '200': {description: ok}
"#;
        let parsed = OpenApiImporter.parse(doc).unwrap();
        let out = OpenApiExporter("openapi").export(&parsed).unwrap();
        assert!(out.contains("\"openapi\": \"3.0.3\""), "{}", out);
        assert!(out.contains("/pets"), "{}", out);
    }

    /// 3.1: webhooks pass-through + trace method + empty paths (a webhooks-only document is valid)
    #[test]
    fn test_oas31_webhooks_trace() {
        let doc = r#"
openapi: 3.1.0
info: {title: T, version: 1.0.0}
paths:
  /debug:
    trace:
      summary: Trace route
      responses:
        '200': {description: ok}
webhooks:
  newPet:
    post:
      requestBody:
        content:
          application/json:
            schema: {$ref: '#/components/schemas/Pet'}
      responses:
        '200': {description: ok}
components:
  schemas:
    Pet:
      type: object
      properties:
        name: {type: string}
"#;
        let parsed = OpenApiImporter.parse(doc).unwrap();
        // the trace method is recognized as an endpoint
        assert_eq!(parsed.endpoints.len(), 1);
        assert_eq!(parsed.endpoints[0].name, "Trace route");
        // webhooks are passed through to extensions
        assert!(parsed.extensions.contains_key("webhooks"));
        // models
        assert_eq!(parsed.models.len(), 1);
        assert_eq!(parsed.models[0].name, "Pet");
        // raw preserves the whole document (no roundtrip loss)
        assert!(parsed.raw.contains_key("webhooks"));

        // export roundtrip: webhooks + components.schemas restored
        let out = OpenApiExporter("openapi").export(&parsed).unwrap();
        assert!(out.contains("newPet"), "webhooks lost in export: {}", out);
        assert!(
            !out.contains("securitySchemes"),
            "should not be generated without a security scheme: {}",
            out
        );
    }

    /// Export roundtrip: components (securitySchemes, etc.) + webhooks restored as-is
    #[test]
    fn test_export_roundtrip_components() {
        let doc = r#"
openapi: 3.0.3
info: {title: T, version: 1.0.0}
components:
  securitySchemes:
    bearerAuth: {type: http, scheme: bearer}
  parameters:
    TraceId:
      name: X-Trace-Id
      in: header
      schema: {type: string}
security:
  - bearerAuth: []
paths:
  /pets:
    get:
      summary: List
      responses:
        '200': {description: ok}
"#;
        let parsed = OpenApiImporter.parse(doc).unwrap();
        let out = OpenApiExporter("openapi").export(&parsed).unwrap();
        // securitySchemes / parameters (the non-schemas parts of components) restored on roundtrip
        assert!(out.contains("bearerAuth"), "securitySchemes lost: {}", out);
        assert!(
            out.contains("X-Trace-Id"),
            "components.parameters lost: {}",
            out
        );
        assert!(
            out.contains("\"security\""),
            "global security lost: {}",
            out
        );
    }

    /// 3.1 schema features: type arrays / const / examples / $defs generate examples
    #[test]
    fn test_oas31_schema_features() {
        let doc = r#"
openapi: 3.1.0
info: {title: T, version: 1.0.0}
paths:
  /x:
    post:
      summary: X
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                nullable: {type: ['string', 'null']}
                fixed: {const: 'hello'}
                choice: {examples: ['a', 'b']}
                refed: {$ref: '#/components/schemas/Thing'}
              required: [nullable, fixed, choice, refed]
      responses:
        '200': {description: ok}
components:
  schemas:
    Thing:
      $defs:
        Inner:
          type: object
          properties:
            v: {type: integer}
      type: object
      properties:
        inner: {$ref: '#/components/schemas/Thing/$defs/Inner'}
      required: [inner]
"#;
        let parsed = OpenApiImporter.parse(doc).unwrap();
        let ep = &parsed.endpoints[0];
        let body = match &ep.request {
            RequestSpec::Http(h) => h.body.as_ref().unwrap().as_str().unwrap().to_string(),
            _ => panic!("expected Http"),
        };
        let v: Value = serde_json::from_str(&body).unwrap();
        // type arrays take the non-null type; const takes its value directly; examples takes the first; $ref recurses into $defs
        assert_eq!(v["nullable"], "string");
        assert_eq!(v["fixed"], "hello");
        assert_eq!(v["choice"], "a");
        assert_eq!(v["refed"]["inner"]["v"], 0);
    }

    /// 3.0: cookie params merged into the Cookie header; callbacks pass through
    #[test]
    fn test_oas3_cookie_and_callbacks() {
        let doc = r#"
openapi: 3.0.3
info: {title: T, version: 1.0.0}
paths:
  /sub:
    post:
      summary: Subscribe
      parameters:
        - {name: session, in: cookie, required: true, schema: {type: string, default: abc}}
        - {name: X-Token, in: header, required: true, schema: {type: string, example: t1}}
      callbacks:
        myCallback:
          '{$request.query.queryUrl}':
            post:
              responses:
                '200': {description: ok}
      responses:
        '200': {description: ok}
"#;
        let parsed = OpenApiImporter.parse(doc).unwrap();
        let ep = &parsed.endpoints[0];
        match &ep.request {
            RequestSpec::Http(h) => {
                assert_eq!(h.headers.get("Cookie").unwrap(), "session=abc");
                assert_eq!(h.headers.get("X-Token").unwrap(), "t1");
            }
            _ => panic!("expected Http"),
        }
        assert!(ep.extensions.contains_key("callbacks"));
    }
}
