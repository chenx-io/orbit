//! ApiSpec -> OpenAPI 3.0 / Swagger 2.0 document export

use serde_json::{json, Map, Value};

use super::super::ir::{ApiSpec, AuthSpec, EndpointSpec};
use super::ExportError;
use crate::model::request::RequestSpec;

/// Extract the common origin (scheme://host[:port]) from endpoint urls and strip it to a relative path.
/// Returns (Some(origin), path) when all endpoints share the same origin; otherwise (None, url as-is).
fn split_origin(spec: &ApiSpec) -> (Option<String>, Vec<(String, String)>) {
    let first = spec.endpoints.iter().find_map(|ep| {
        let u = ep.url_of();
        if u.starts_with("http://") || u.starts_with("https://") {
            origin_of(&u).map(|o| (o, u))
        } else {
            None
        }
    });
    let mut paths = Vec::new();
    let origin = first.as_ref().map(|(o, _)| o.clone());
    for ep in &spec.endpoints {
        let u = ep.url_of();
        let path = match &origin {
            Some(o) if u.starts_with(o.as_str()) => u[o.len()..].to_string(),
            _ => u,
        };
        let path = if path.is_empty() {
            "/".to_string()
        } else {
            path
        };
        paths.push((ep.name.clone(), path));
    }
    (origin, paths)
}

fn origin_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let end = rest.find('/').unwrap_or(rest.len());
    Some(url[..url.len() - rest.len() + end].to_string())
}

fn endpoint_method(ep: &EndpointSpec) -> &str {
    match &ep.request {
        RequestSpec::Http(h) => h.method.as_str(),
        _ => "GET",
    }
}

/// Pre / post scripts -> operation-level x- extension (roundtrip: the importer reads the same-named field)
fn op_ext_scripts(op: &mut Map<String, Value>, ep: &EndpointSpec) {
    if let Some(pre) = &ep.pre_script {
        op.insert("x-orbit-prerequest".into(), json!(pre));
    }
    if let Some(post) = &ep.post_script {
        op.insert("x-orbit-postrequest".into(), json!(post));
    }
}

/// Whether model_ref points to a model within this export's collection
fn model_exists(spec: &ApiSpec, name: &str) -> bool {
    spec.models.iter().any(|m| m.name == name)
}

/// Auth -> securityScheme name (defined in the exported document)
fn auth_scheme_name(a: &AuthSpec) -> String {
    match a {
        AuthSpec::Bearer => "bearerAuth".into(),
        AuthSpec::Basic => "basicAuth".into(),
        AuthSpec::ApiKey { name, .. } => format!("apiKey_{}", name),
        AuthSpec::OAuth2 => "oauth2Auth".into(),
    }
}

fn auth_scheme_def(a: &AuthSpec) -> Value {
    match a {
        AuthSpec::Bearer => json!({ "type": "http", "scheme": "bearer" }),
        AuthSpec::Basic => json!({ "type": "http", "scheme": "basic" }),
        AuthSpec::ApiKey { name, location } => {
            json!({ "type": "apiKey", "name": name, "in": location })
        }
        AuthSpec::OAuth2 => {
            json!({ "type": "oauth2", "flows": { "clientCredentials": { "tokenUrl": "" } } })
        }
    }
}

fn auth_security(a: &AuthSpec) -> Value {
    let name = auth_scheme_name(a);
    json!([{ name: [] }])
}

/// OAS3 document export
pub(crate) fn to_openapi3(spec: &ApiSpec) -> Result<String, ExportError> {
    let (origin, names) = split_origin(spec);
    let mut paths: Map<String, Value> = Map::new();
    let mut schemes: Map<String, Value> = Map::new();

    for (i, ep) in spec.endpoints.iter().enumerate() {
        let path = names.get(i).map(|(_, p)| p.clone()).unwrap_or_default();
        let method = endpoint_method(ep).to_lowercase();
        let mut op = Map::new();
        if let Some(desc) = &ep.description {
            op.insert("summary".into(), json!(desc));
        }
        op.insert("operationId".into(), json!(ep.name));
        if !ep.group.is_empty() {
            op.insert("tags".into(), json!(ep.group));
        }

        // query params
        if !ep.query_params.is_empty() {
            let params: Vec<Value> = ep
                .query_params
                .iter()
                .map(|(k, v)| json!({ "name": k, "in": "query", "schema": { "type": "string" }, "example": v }))
                .collect();
            op.insert("parameters".into(), json!(params));
        }

        // request body: when a model is associated, output a schema $ref (so import roundtrip restores model_ref;
        // the importer prefers example over schema, so the two never coexist); otherwise output example as JSON/text.
        if let RequestSpec::Http(h) = &ep.request {
            let model_ref_ok = ep
                .model_ref
                .as_deref()
                .is_some_and(|mr| model_exists(spec, mr));
            if model_ref_ok {
                let mr = ep.model_ref.as_deref().unwrap();
                op.insert(
                    "requestBody".into(),
                    json!({
                        "content": { "application/json": { "schema": { "$ref": format!("#/components/schemas/{}", mr) } } }
                    }),
                );
            } else if let Some(body) = &h.body {
                let text = super::super::curl::body_to_text(body);
                if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                    op.insert(
                        "requestBody".into(),
                        json!({
                            "content": { "application/json": { "example": parsed } }
                        }),
                    );
                } else if !text.is_empty() {
                    op.insert(
                        "requestBody".into(),
                        json!({
                            "content": { "text/plain": { "example": text } }
                        }),
                    );
                }
            }
        }

        // pre / post scripts
        op_ext_scripts(&mut op, ep);

        // responses
        if !ep.responses.is_empty() {
            let mut responses = Map::new();
            for r in &ep.responses {
                let body = if r.body.is_empty() {
                    serde_json::from_str::<Value>(&r.body).unwrap_or(Value::Null)
                } else {
                    serde_json::from_str::<Value>(&r.body).unwrap_or(Value::String(r.body.clone()))
                };
                let mut resp = Map::new();
                resp.insert("description".into(), json!(r.name));
                if body != Value::Null {
                    resp.insert(
                        "content".into(),
                        json!({ "application/json": { "example": body } }),
                    );
                }
                responses.insert(r.status.to_string(), Value::Object(resp));
            }
            op.insert("responses".into(), Value::Object(responses));
        } else {
            op.insert(
                "responses".into(),
                json!({ "200": { "description": "ok" } }),
            );
        }

        // auth
        if let Some(a) = &ep.auth {
            op.insert("security".into(), auth_security(a));
            schemes
                .entry(auth_scheme_name(a))
                .or_insert_with(|| auth_scheme_def(a));
        }

        let item = paths.entry(path).or_insert_with(|| json!({}));
        if let Some(m) = item.as_object_mut() {
            m.insert(method, Value::Object(op));
        }
    }

    // global auth
    let mut security = None;
    if let Some(a) = &spec.auth {
        security = Some(auth_security(a));
        schemes
            .entry(auth_scheme_name(a))
            .or_insert_with(|| auth_scheme_def(a));
    }

    // data models + the full components restored from raw (securitySchemes/parameters/examples/requestBodies/headers/links/callbacks/pathItems)
    let mut components = Map::new();
    if let Some(rc) = spec.raw.get("components").and_then(|v| v.as_object()) {
        for (k, v) in rc {
            if k != "schemas" {
                components.insert(k.clone(), v.clone());
            }
        }
    }
    if !spec.models.is_empty() {
        let mut schemas = Map::new();
        for m in &spec.models {
            schemas.insert(m.name.clone(), m.schema.clone());
        }
        components.insert("schemas".into(), Value::Object(schemas));
    }
    if !schemes.is_empty() {
        components.insert("securitySchemes".into(), Value::Object(schemes));
    }

    let mut doc = Map::new();
    doc.insert("openapi".into(), json!("3.0.3"));
    doc.insert(
        "info".into(),
        json!({
            "title": spec.name,
            "version": "1.0.0",
            "description": spec.description.as_deref().unwrap_or(""),
        }),
    );
    if let Some(o) = &origin {
        doc.insert("servers".into(), json!([{ "url": o }]));
    }
    // roundtrip: 3.1 webhooks restored as-is
    if let Some(w) = spec.raw.get("webhooks") {
        doc.insert("webhooks".into(), w.clone());
    }
    if let Some(d) = spec.raw.get("jsonSchemaDialect") {
        doc.insert("jsonSchemaDialect".into(), d.clone());
    }
    doc.insert("paths".into(), Value::Object(paths));
    if !components.is_empty() {
        doc.insert("components".into(), Value::Object(components));
    }
    if let Some(s) = security {
        doc.insert("security".into(), s);
    }

    serde_json::to_string_pretty(&Value::Object(doc))
        .map_err(|e| ExportError::Serialize(e.to_string()))
}

/// Swagger 2.0 document export
pub(crate) fn to_swagger2(spec: &ApiSpec) -> Result<String, ExportError> {
    let (origin, names) = split_origin(spec);
    let mut paths: Map<String, Value> = Map::new();
    let mut defs: Map<String, Value> = Map::new();

    for (i, ep) in spec.endpoints.iter().enumerate() {
        let path = names.get(i).map(|(_, p)| p.clone()).unwrap_or_default();
        let method = endpoint_method(ep).to_lowercase();
        let mut op = Map::new();
        if let Some(desc) = &ep.description {
            op.insert("summary".into(), json!(desc));
        }
        op.insert("operationId".into(), json!(ep.name));
        if !ep.group.is_empty() {
            op.insert("tags".into(), json!(ep.group));
        }

        let mut params: Vec<Value> = Vec::new();
        for (k, v) in &ep.query_params {
            params.push(json!({ "name": k, "in": "query", "type": "string", "default": v }));
        }

        // body param: when a model is associated, $ref -> definitions; otherwise generate a permissive schema from the example
        if let RequestSpec::Http(h) = &ep.request {
            let model_ref_ok = ep
                .model_ref
                .as_deref()
                .is_some_and(|mr| model_exists(spec, mr));
            if model_ref_ok {
                let mr = ep.model_ref.as_deref().unwrap();
                params.push(json!({
                    "name": "body",
                    "in": "body",
                    "schema": { "$ref": format!("#/definitions/{}", mr) }
                }));
            } else if let Some(body) = &h.body {
                let text = super::super::curl::body_to_text(body);
                if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                    let schema = value_to_schema(&parsed);
                    params.push(json!({ "name": "body", "in": "body", "schema": schema }));
                }
            }
        }
        if !params.is_empty() {
            op.insert("parameters".into(), json!(params));
        }

        // pre / post scripts
        op_ext_scripts(&mut op, ep);

        // responses
        if !ep.responses.is_empty() {
            let mut responses = Map::new();
            for r in &ep.responses {
                let body =
                    serde_json::from_str::<Value>(&r.body).unwrap_or(Value::String(r.body.clone()));
                let mut resp = Map::new();
                resp.insert("description".into(), json!(r.name));
                if body != Value::String(String::new()) {
                    resp.insert("examples".into(), json!({ "application/json": body }));
                }
                responses.insert(r.status.to_string(), Value::Object(resp));
            }
            op.insert("responses".into(), Value::Object(responses));
        } else {
            op.insert(
                "responses".into(),
                json!({ "200": { "description": "ok" } }),
            );
        }

        let item = paths.entry(path).or_insert_with(|| json!({}));
        if let Some(m) = item.as_object_mut() {
            m.insert(method, Value::Object(op));
        }
    }

    for m in &spec.models {
        defs.insert(m.name.clone(), m.schema.clone());
    }
    // roundtrip: swagger 2.0 securityDefinitions restored as-is
    let mut security_defs = Map::new();
    if let Some(sd) = spec
        .raw
        .get("securityDefinitions")
        .and_then(|v| v.as_object())
    {
        for (k, v) in sd {
            security_defs.insert(k.clone(), v.clone());
        }
    }

    // host / schemes / basePath split from the common origin
    let (host, schemes, base_path) = match &origin {
        Some(o) => {
            if let Some(rest) = o.strip_prefix("https://") {
                (rest.to_string(), "https".to_string(), String::new())
            } else if let Some(rest) = o.strip_prefix("http://") {
                (rest.to_string(), "http".to_string(), String::new())
            } else {
                (o.clone(), "http".to_string(), String::new())
            }
        }
        None => ("localhost".into(), "http".into(), String::new()),
    };

    let mut doc = Map::new();
    doc.insert("swagger".into(), json!("2.0"));
    doc.insert(
        "info".into(),
        json!({
            "title": spec.name,
            "version": "1.0.0",
            "description": spec.description.as_deref().unwrap_or(""),
        }),
    );
    doc.insert("host".into(), json!(host));
    doc.insert("schemes".into(), json!([schemes]));
    doc.insert("basePath".into(), json!(base_path));
    doc.insert("paths".into(), Value::Object(paths));
    if !defs.is_empty() {
        doc.insert("definitions".into(), Value::Object(defs));
    }
    if !security_defs.is_empty() {
        doc.insert("securityDefinitions".into(), Value::Object(security_defs));
    }

    serde_json::to_string_pretty(&Value::Object(doc))
        .map_err(|e| ExportError::Serialize(e.to_string()))
}

/// Build a permissive JSON schema from an example value (sw2 body params require a schema).
fn value_to_schema(v: &Value) -> Value {
    match v {
        Value::Object(_) => json!({ "type": "object", "example": v }),
        Value::Array(_) => json!({ "type": "array", "example": v }),
        Value::String(_) => json!({ "type": "string", "example": v }),
        Value::Number(_) => json!({ "type": "number", "example": v }),
        Value::Bool(_) => json!({ "type": "boolean", "example": v }),
        Value::Null => json!({ "type": "object", "example": v }),
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::ImportFormat;
    use super::*;
    use crate::exchange::openapi::OpenApiImporter;
    use crate::exchange::ModelSpec;
    use crate::model::request::HttpRequestConfig;
    use std::collections::HashMap;

    fn http_req(method: &str, url: &str) -> RequestSpec {
        RequestSpec::Http(Box::new(HttpRequestConfig {
            method: method.into(),
            url: url.into(),
            headers: HashMap::new(),
            body: Some(serde_yaml::Value::String("{\"a\":1}".into())),
            timeout: "30s".into(),
            payload_format: None,
            grpc_service: None,
            grpc_use_reflection: false,
            response_format: None,
        }))
    }

    fn sample_spec() -> ApiSpec {
        let mut ep = EndpointSpec::new(
            "createOrder",
            http_req("POST", "https://api.example.com/orders"),
        );
        ep.model_ref = Some("CreateOrderRequest".into());
        ep.pre_script = Some("pm.environment.set(\"t\", \"1\");".into());
        ep.post_script = Some("pm.test(\"ok\", () => { pm.expect(true).to.be.true; });".into());
        let mut spec = ApiSpec::new("Demo");
        spec.endpoints.push(ep);
        spec.models.push(ModelSpec {
            name: "CreateOrderRequest".into(),
            schema: json!({"type": "object", "properties": {"a": {"type": "integer"}}}),
        });
        spec
    }

    #[test]
    fn test_export_oas3_model_ref_and_scripts() {
        let out = to_openapi3(&sample_spec()).unwrap();
        assert!(
            out.contains("#/components/schemas/CreateOrderRequest"),
            "model $ref lost: {}",
            out
        );
        assert!(
            out.contains("x-orbit-prerequest"),
            "pre script lost: {}",
            out
        );
        assert!(
            out.contains("x-orbit-postrequest"),
            "post script lost: {}",
            out
        );
        // the importer prefers example over schema; both coexisting would lose model_ref on roundtrip
        assert!(
            !out.contains("\"example\""),
            "example should not be output together with a model association: {}",
            out
        );

        // roundtrip: re-import restores the model association and scripts
        let parsed = OpenApiImporter.parse(&out).unwrap();
        let ep = &parsed.endpoints[0];
        assert_eq!(ep.model_ref.as_deref(), Some("CreateOrderRequest"));
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

    #[test]
    fn test_export_sw2_model_ref_and_scripts() {
        let out = to_swagger2(&sample_spec()).unwrap();
        assert!(
            out.contains("#/definitions/CreateOrderRequest"),
            "model $ref lost: {}",
            out
        );
        assert!(
            out.contains("x-orbit-prerequest"),
            "pre script lost: {}",
            out
        );
        assert!(
            out.contains("x-orbit-postrequest"),
            "post script lost: {}",
            out
        );

        let parsed = OpenApiImporter.parse(&out).unwrap();
        let ep = &parsed.endpoints[0];
        assert_eq!(ep.model_ref.as_deref(), Some("CreateOrderRequest"));
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
}
