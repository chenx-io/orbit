//! curl importer + curl command exporter

use std::collections::HashMap;

use super::ir::{ApiSpec, AuthSpec, EndpointSpec};
use super::{ExportError, ExportFormat, ImportError, ImportFormat};
use crate::model::request::{HttpRequestConfig, RequestSpec};

/// curl: import (command -> ApiSpec) + export (EndpointSpec -> curl command)
pub(crate) struct CurlImporter;

impl ImportFormat for CurlImporter {
    fn name(&self) -> &'static str {
        "curl"
    }

    fn parse(&self, input: &str) -> Result<ApiSpec, ImportError> {
        let (cfg, auth) = parse_curl(input)?;
        let mut ep = EndpointSpec::new("curl_import", RequestSpec::Http(Box::new(cfg)));
        if let Some(a) = auth {
            ep.auth = Some(a.0);
            ep.extensions
                .insert("curl_auth_credential".into(), serde_json::json!(a.1));
        }
        let mut spec = ApiSpec::new("curl import");
        spec.endpoints.push(ep);
        Ok(spec)
    }
}

impl ExportFormat for CurlImporter {
    fn name(&self) -> &'static str {
        "curl"
    }

    fn export_request(&self, ep: &EndpointSpec) -> Result<String, ExportError> {
        let h = http_of(ep)?;
        Ok(export_curl_cmd(h))
    }
}

pub(crate) fn http_of(ep: &EndpointSpec) -> Result<&HttpRequestConfig, ExportError> {
    match &ep.request {
        RequestSpec::Http(h) => Ok(h),
        _ => Err(ExportError::Unsupported(
            "this export format only supports HTTP requests".into(),
        )),
    }
}

/// curl command generation (Content-Length is dropped: curl computes it automatically from the actual request body)
pub(crate) fn export_curl_cmd(h: &HttpRequestConfig) -> String {
    let mut parts = vec!["curl".to_string()];
    if h.method != "GET" {
        parts.push(format!("-X {}", h.method));
    }
    let headers: Vec<(&String, &String)> = h
        .headers
        .iter()
        .filter(|(k, _)| !k.eq_ignore_ascii_case("content-length"))
        .collect();
    for (k, v) in headers {
        parts.push(format!("-H '{}: {}'", k, v));
    }
    if let Some(body) = &h.body {
        let text = body_to_text(body);
        if !text.is_empty() {
            parts.push(format!("-d '{}'", text.replace('\'', "'\\''")));
        }
    }
    parts.push(format!("'{}'", h.url));
    parts.join(" \\\n  ")
}

/// serde_yaml::Value -> text (strings as-is, others to JSON)
pub(crate) fn body_to_text(body: &serde_yaml::Value) -> String {
    match body {
        serde_yaml::Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

// ─── curl parser (moved from importer/curl.rs + enhancements: -F/-u/-b/-k/-L/-m) ────────────

/// Parse result: HTTP config + optional auth hint (AuthSpec, raw credentials)
fn parse_curl(cmd: &str) -> Result<(HttpRequestConfig, Option<(AuthSpec, String)>), ImportError> {
    let cmd = cmd.trim();

    let mut method = "GET".to_string();
    let mut url = String::new();
    let mut headers = HashMap::new();
    let mut body: Option<serde_yaml::Value> = None;
    let mut form_fields: Vec<(String, String)> = Vec::new();
    let mut auth: Option<(AuthSpec, String)> = None;
    let mut timeout = "30s".to_string();

    let tokens = shell_words_split(cmd);

    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        match token.as_str() {
            "-X" | "--request" => {
                i += 1;
                if i < tokens.len() {
                    method = tokens[i].to_uppercase();
                }
            }
            "-H" | "--header" => {
                i += 1;
                if i < tokens.len() {
                    let header = &tokens[i];
                    if let Some(pos) = header.find(':') {
                        let key = header[..pos].trim().to_string();
                        let value = header[pos + 1..].trim().to_string();
                        headers.insert(key, value);
                    }
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => {
                i += 1;
                if i < tokens.len() {
                    let raw = &tokens[i];
                    if raw.starts_with('{') || raw.starts_with('[') {
                        body = serde_json::from_str::<serde_yaml::Value>(raw)
                            .ok()
                            .or(Some(serde_yaml::Value::String(raw.clone())));
                    } else {
                        body = Some(serde_yaml::Value::String(raw.clone()));
                    }
                    if method == "GET" {
                        method = "POST".to_string();
                    }
                }
            }
            "-F" | "--form" => {
                i += 1;
                if i < tokens.len() {
                    let f = &tokens[i];
                    if let Some(eq) = f.find('=') {
                        form_fields.push((f[..eq].to_string(), f[eq + 1..].to_string()));
                    }
                }
            }
            "-u" | "--user" => {
                i += 1;
                if i < tokens.len() {
                    auth = Some((AuthSpec::Basic, tokens[i].clone()));
                }
            }
            "-b" | "--cookie" => {
                i += 1;
                if i < tokens.len() {
                    headers
                        .entry("Cookie".to_string())
                        .or_insert_with(|| tokens[i].clone());
                }
            }
            "-k" | "--insecure" => {
                // skip TLS verification: pass through (allowed by default in the execution layer)
            }
            "-L" | "--location" => {
                // follow redirects: pass through (followed by default in the execution layer)
            }
            "-m" | "--max-time" => {
                i += 1;
                if i < tokens.len() {
                    if let Ok(secs) = tokens[i].parse::<f64>() {
                        timeout = format!("{}s", secs);
                    }
                }
            }
            other if other.starts_with("http://") || other.starts_with("https://") => {
                url = other.to_string();
            }
            other if other.starts_with('-') => {
                // skip unknown flags
            }
            other if url.is_empty() && !other.starts_with("curl") => {
                url = other.to_string();
            }
            _ => {}
        }
        i += 1;
    }

    if url.is_empty() {
        return Err(ImportError::Parse(
            "No URL found in curl command".to_string(),
        ));
    }

    // -F form fields -> urlencoded body (multipart file uploads currently pass through to extensions)
    if !form_fields.is_empty() && body.is_none() {
        body = Some(serde_yaml::Value::String(
            form_fields
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&"),
        ));
    }

    let cfg = HttpRequestConfig {
        method,
        url,
        headers,
        body,
        timeout,
        payload_format: None,
        grpc_service: None,
        grpc_use_reflection: false,
        response_format: None,
    };
    Ok((cfg, auth))
}

fn shell_words_split(cmd: &str) -> Vec<String> {
    // simplified shell tokenization
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '\\' if !in_single => {
                i += 1;
                if i < chars.len() {
                    current.push(chars[i]);
                }
            }
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(c),
        }
        i += 1;
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_curl_get() {
        let curl = r#"curl -X GET "https://api.example.com/users" -H "Accept: application/json""#;
        let spec = CurlImporter.parse(curl).unwrap();
        let ep = &spec.endpoints[0];
        match &ep.request {
            RequestSpec::Http(h) => {
                assert_eq!(h.method, "GET");
                assert_eq!(h.url, "https://api.example.com/users");
                assert_eq!(h.headers.get("Accept").unwrap(), "application/json");
            }
            _ => panic!("expected Http"),
        }
    }

    #[test]
    fn test_parse_curl_post() {
        let curl = r#"curl -X POST https://api.example.com/login -H "Content-Type: application/json" -d '{"username":"test","password":"test"}'"#;
        let spec = CurlImporter.parse(curl).unwrap();
        match &spec.endpoints[0].request {
            RequestSpec::Http(h) => {
                assert_eq!(h.method, "POST");
                assert!(h.body.is_some());
            }
            _ => panic!("expected Http"),
        }
    }

    #[test]
    fn test_export_curl_omits_content_length() {
        let cfg = HttpRequestConfig {
            method: "POST".into(),
            url: "https://x.example.com/api".into(),
            headers: HashMap::from([
                ("Content-Length".into(), "10".into()),
                ("Accept".into(), "application/json".into()),
            ]),
            body: Some(serde_yaml::Value::String("{\"a\":1}".into())),
            timeout: "30s".into(),
            payload_format: None,
            grpc_service: None,
            grpc_use_reflection: false,
            response_format: None,
        };
        let out = export_curl_cmd(&cfg);
        assert!(!out.contains("Content-Length"), "{}", out);
        assert!(out.contains("-H 'Accept: application/json'"), "{}", out);
    }

    /// -u basic auth hint + -b cookie + -F form + -m timeout
    #[test]
    fn test_parse_curl_advanced_flags() {
        let curl = r#"curl -X POST 'https://api.example.com/up' \
  -u 'user:pass' \
  -b 'session=abc' \
  -F 'field=v1' \
  -F 'file=@/tmp/x.png' \
  -m 15"#;
        let spec = CurlImporter.parse(curl).unwrap();
        let ep = &spec.endpoints[0];
        assert_eq!(ep.auth, Some(AuthSpec::Basic));
        assert_eq!(
            ep.extensions
                .get("curl_auth_credential")
                .unwrap()
                .as_str()
                .unwrap(),
            "user:pass"
        );
        match &ep.request {
            RequestSpec::Http(h) => {
                assert_eq!(h.headers.get("Cookie").unwrap(), "session=abc");
                assert_eq!(h.timeout, "15s");
                // -F fields composed into an urlencoded body
                assert_eq!(
                    h.body.as_ref().unwrap().as_str().unwrap(),
                    "field=v1&file=@/tmp/x.png"
                );
            }
            _ => panic!("expected Http"),
        }
    }
}
