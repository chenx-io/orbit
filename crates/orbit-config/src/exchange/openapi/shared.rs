//! Shared OpenAPI 2.0 / 3.0 parsing core (used by both import / export)

use serde_json::{Map, Value};

use super::super::ir::{AuthSpec, ResponseSpec};

/// Whether it is an HTTP method (case-insensitive, including trace)
pub(crate) fn is_http_method(m: &str) -> bool {
    matches!(
        m.to_lowercase().as_str(),
        "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "trace"
    )
}

/// Extract script content from an operation-level x- extension.
pub(crate) fn script_from_extension(op: &Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(v) = op.get(*k) {
            if let Some(s) = v.as_str() {
                return s.to_string();
            }
            if let Some(arr) = v.as_array() {
                let joined: Vec<String> = arr
                    .iter()
                    .filter_map(|e| e.as_str().map(String::from))
                    .collect();
                if !joined.is_empty() {
                    return joined.join("\n");
                }
            }
        }
    }
    String::new()
}

pub(crate) const PRE_SCRIPT_KEYS: &[&str] = &[
    "x-orbit-prerequest",
    "x-prerequest",
    "x-pre-script",
    "x-codegen-prerequest",
];
pub(crate) const POST_SCRIPT_KEYS: &[&str] = &[
    "x-orbit-postrequest",
    "x-postrequest",
    "x-post-script",
    "x-codegen-postrequest",
];

/// Take the first scheme name from the security array (`security: [{ bearerAuth: [] }]`).
pub(crate) fn pick_security_scheme(sec: &Value) -> Option<String> {
    sec.as_array()?
        .iter()
        .find_map(|entry| entry.as_object()?.keys().next().cloned())
}

/// Return the name when securitySchemes / securityDefinitions has exactly one scheme.
pub(crate) fn sole_scheme_name(schemes: Option<&Map<String, Value>>) -> Option<String> {
    let m = schemes?;
    if m.len() == 1 {
        m.keys().next().cloned()
    } else {
        None
    }
}

/// OAS3 security scheme -> auth type hint
pub(crate) fn auth_from_scheme(
    scheme_name: Option<&str>,
    schemes: Option<&Map<String, Value>>,
) -> Option<AuthSpec> {
    let name = scheme_name?;
    let scheme = match schemes.and_then(|m| m.get(name)) {
        Some(s) => s,
        None => {
            // definition missing: infer common types from the reference name (covers incomplete documents)
            let lower = name.to_lowercase();
            if lower.contains("bearer") {
                return Some(AuthSpec::Bearer);
            }
            if lower.contains("basic") {
                return Some(AuthSpec::Basic);
            }
            return None;
        }
    };
    match scheme["type"].as_str().unwrap_or("") {
        "http" => match scheme["scheme"].as_str().unwrap_or("") {
            "bearer" => Some(AuthSpec::Bearer),
            "basic" => Some(AuthSpec::Basic),
            _ => None,
        },
        "apiKey" => {
            let key_name = scheme["name"].as_str().unwrap_or(name).to_string();
            let add_to = scheme["in"].as_str().unwrap_or("header").to_string();
            // some OAS3 documents express Bearer via apiKey+Authorization; a scheme name containing bearer is also treated as Bearer
            if (key_name.eq_ignore_ascii_case("authorization") && add_to == "header")
                || name.to_lowercase().contains("bearer")
            {
                Some(AuthSpec::Bearer)
            } else {
                Some(AuthSpec::ApiKey {
                    name: key_name,
                    location: add_to,
                })
            }
        }
        "oauth2" => Some(AuthSpec::OAuth2),
        _ => None,
    }
}

/// Swagger2 security scheme -> auth type hint
pub(crate) fn sw2_auth_from_scheme(
    scheme_name: Option<&str>,
    defs: Option<&Map<String, Value>>,
) -> Option<AuthSpec> {
    let name = scheme_name?;
    let scheme = match defs.and_then(|m| m.get(name)) {
        Some(s) => s,
        None => {
            let lower = name.to_lowercase();
            if lower.contains("bearer") {
                return Some(AuthSpec::Bearer);
            }
            if lower.contains("basic") {
                return Some(AuthSpec::Basic);
            }
            return None;
        }
    };
    match scheme["type"].as_str().unwrap_or("") {
        "basic" => Some(AuthSpec::Basic),
        "apiKey" => {
            let key_name = scheme["name"].as_str().unwrap_or(name).to_string();
            let add_to = scheme["in"].as_str().unwrap_or("header").to_string();
            if (key_name.eq_ignore_ascii_case("authorization") && add_to == "header")
                || name.to_lowercase().contains("bearer")
            {
                Some(AuthSpec::Bearer)
            } else {
                Some(AuthSpec::ApiKey {
                    name: key_name,
                    location: add_to,
                })
            }
        }
        "oauth2" => Some(AuthSpec::OAuth2),
        _ => None,
    }
}

/// `#/components/schemas/X` → `X`
pub(crate) fn ref_name_from_ref(ref_str: &str) -> String {
    ref_str.rsplit('/').next().unwrap_or("").to_string()
}

/// Convert `{var}` in the server url to app variable syntax `{{var}}` (already-doubled braces are not converted again).
pub(crate) fn to_app_var(s: &str) -> String {
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(s.len() + 8);
    let mut i = 0;
    while i < n {
        if bytes[i] == b'{' {
            if i + 1 < n && bytes[i + 1] == b'{' {
                match s[i..].find("}}") {
                    Some(rel) => {
                        let j = i + rel + 2;
                        out.push_str(&s[i..j]);
                        i = j;
                    }
                    None => {
                        out.push_str(&s[i..]);
                        i = n;
                    }
                }
            } else if let Some(rel) = s[i..].find('}') {
                let j = i + rel + 1;
                out.push_str("{{");
                out.push_str(&s[i + 1..j - 1]);
                out.push_str("}}");
                i = j;
            } else {
                out.push('{');
                i += 1;
            }
        } else {
            let c = s[i..].chars().next().unwrap_or(' ');
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// Dereference a `$ref` from the api document.
pub(crate) fn resolve_ref<'a>(schema: &'a Value, api: &'a Value) -> &'a Value {
    if let Some(ref_str) = schema.get("$ref").and_then(|v| v.as_str()) {
        let mut current = api;
        for part in ref_str.split('/').skip(1) {
            if let Some(next) = current.get(part) {
                current = next;
            } else {
                return schema;
            }
        }
        return current;
    }
    schema
}

/// Convert a JSON value to a string (string/number/bool; otherwise None).
pub(crate) fn val_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Extract OAS3 response examples (by status code, including field description schemas).
pub(crate) fn extract_responses_oas3(responses: &Value, api: &Value) -> Vec<ResponseSpec> {
    extract_responses_impl(responses, api, false)
}

/// Extract Swagger2 response examples (by status code, including field description schemas).
pub(crate) fn extract_responses_sw2(responses: &Value, api: &Value) -> Vec<ResponseSpec> {
    extract_responses_impl(responses, api, true)
}

fn extract_responses_impl(responses: &Value, api: &Value, sw2: bool) -> Vec<ResponseSpec> {
    let mut out = Vec::new();
    if let Some(map) = responses.as_object() {
        for (status, resp) in map {
            let status_num = status.parse::<u16>().unwrap_or(0);
            if status_num == 0 {
                continue;
            }
            let name = resp["description"].as_str().unwrap_or(status).to_string();
            let (body, schema) = if sw2 {
                if let Some(ex) = resp.get("examples").and_then(|e| e.get("application/json")) {
                    (serde_json::to_string_pretty(ex).unwrap_or_default(), None)
                } else if let Some(schema) = resp.get("schema") {
                    let resolved = resolve_ref(schema, api);
                    (
                        serde_json::to_string_pretty(&generate_example_from_schema(resolved, api))
                            .unwrap_or_default(),
                        Some(resolved.clone()),
                    )
                } else {
                    (String::new(), None)
                }
            } else {
                let media = resp["content"]
                    .as_object()
                    .and_then(|c| c.get("application/json"))
                    .or_else(|| resp["content"].as_object().and_then(|c| c.values().next()));
                if let Some(media_obj) = media {
                    if let Some(ex) = media_obj.get("example") {
                        (serde_json::to_string_pretty(ex).unwrap_or_default(), None)
                    } else if let Some(schema) = media_obj.get("schema") {
                        let resolved = resolve_ref(schema, api);
                        (
                            serde_json::to_string_pretty(&generate_example_from_schema(
                                resolved, api,
                            ))
                            .unwrap_or_default(),
                            Some(resolved.clone()),
                        )
                    } else {
                        (String::new(), None)
                    }
                } else {
                    (String::new(), None)
                }
            };
            if !body.is_empty() {
                out.push(ResponseSpec {
                    status: status_num,
                    name,
                    body,
                    schema,
                });
            }
        }
    }
    out
}

/// Generate an example JSON from a schema (covering JSON Schema 2020-12 / OAS 3.1 features).
///
/// Supports: $ref dereferencing (including $defs), type arrays, const, examples/example, enum, format,
/// allOf merging, oneOf/anyOf taking the first branch, and recursive items/properties.
pub(crate) fn generate_example_from_schema(schema: &Value, api: &Value) -> Value {
    let resolved = resolve_ref(schema, api);
    let empty = Map::new();
    let schema = resolved.as_object().unwrap_or(&empty);

    // combination keywords: allOf merges properties; oneOf/anyOf takes the first non-empty branch
    if let Some(all_of) = schema.get("allOf").and_then(|v| v.as_array()) {
        let mut merged = Map::new();
        let mut required: Vec<&str> = Vec::new();
        for sub in all_of {
            if let Value::Object(sub_obj) = generate_example_from_schema(sub, api) {
                for (k, v) in sub_obj {
                    merged.insert(k, v);
                }
            }
            let sub = resolve_ref(sub, api);
            if let Some(r) = sub.get("required").and_then(|v| v.as_array()) {
                for item in r.iter().filter_map(|v| v.as_str()) {
                    required.push(item);
                }
            }
        }
        if !required.is_empty() {
            merged.retain(|k, _| required.contains(&k.as_str()));
        }
        return Value::Object(merged);
    }
    if let Some(combo) = schema.get("oneOf").or_else(|| schema.get("anyOf")) {
        if let Some(first) = combo.as_array().and_then(|a| a.first()) {
            return generate_example_from_schema(first, api);
        }
    }

    // const / examples / example / enum take precedence (3.1 features)
    if let Some(c) = schema.get("const") {
        return c.clone();
    }
    if let Some(exs) = schema.get("examples").and_then(|v| v.as_array()) {
        if let Some(first) = exs.first() {
            return first.clone();
        }
    }
    if let Some(ex) = schema.get("example") {
        return ex.clone();
    }

    // 3.1: type may be an array (e.g. ["string", "null"]) -> take the first non-null type
    let schema_type = schema
        .get("type")
        .map(|t| match t {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .find(|t| *t != "null")
                .unwrap_or("object"),
            Value::String(s) => s.as_str(),
            _ => "object",
        })
        .unwrap_or("object");

    match schema_type {
        "object" => {
            let mut obj = Map::new();
            if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
                let required: Vec<&str> = schema
                    .get("required")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                for (key, prop_schema) in props {
                    let example = generate_example_from_schema(prop_schema, api);
                    obj.insert(key.clone(), example);
                }
                if !required.is_empty() {
                    obj.retain(|k, _| required.contains(&k.as_str()));
                }
            }
            Value::Object(obj)
        }
        "array" => {
            let items = schema
                .get("items")
                .map(|s| generate_example_from_schema(s, api))
                .unwrap_or(Value::String("string".to_string()));
            Value::Array(vec![items])
        }
        "string" => {
            if let Some(enm) = schema
                .get("enum")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
            {
                enm.clone()
            } else if let Some(format) = schema.get("format").and_then(|v| v.as_str()) {
                match format {
                    "date-time" => Value::String("2024-01-01T00:00:00Z".to_string()),
                    "date" => Value::String("2024-01-01".to_string()),
                    "email" => Value::String("user@example.com".to_string()),
                    "uri" | "url" => Value::String("https://example.com".to_string()),
                    "uuid" => Value::String("00000000-0000-0000-0000-000000000000".to_string()),
                    "password" => Value::String("password".to_string()),
                    _ => Value::String("string".to_string()),
                }
            } else {
                Value::String("string".to_string())
            }
        }
        "integer" | "number" => Value::Number(serde_json::Number::from(0)),
        "boolean" => Value::Bool(false),
        _ => Value::String("".to_string()),
    }
}
