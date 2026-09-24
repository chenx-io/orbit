//! Builder for the single-send path: "un-interpolated request template" -> "final request".
//!
//! Background: variable interpolation, header merging and body assembly used to be scattered across the frontend TypeScript
//! (`web/src/lib/{resolve,request,requestBody}.ts`), which made "a pre-interpolation script writes a variable"
//! impossible (the frontend had already interpolated by the time the script ran). The build responsibility moves down into the engine here,
//! so single-shot debugging / scenario load testing / AI tool execution share one pipeline:
//!
//! ```text
//! RequestTemplate --expand_template--> un-interpolated skeleton (template form, visible to pre-interpolation scripts)
//!                  --interpolate-------->
//!                  --finalize_*------> final URL / headers / body bytes
//! ```
//!
//! Aligned item by item with the frontend's previous behavior (a difference directly changes the request the server sees):
//! - URL: percent-encoding with `encodeURIComponent` semantics ([`encode_uri_component`]);
//! - Headers: default headers -> auth headers -> user headers; on a name collision the later one overrides the earlier one;
//! - Content-Type: derived from the body mode; **not overridden** when the user configures it explicitly in the headers;
//! - Host / Content-Length: transport-derived values that only go into the request snapshot and are stripped before sending
//!   ([`is_hop_by_hop`]); the underlying client computes them.

use std::collections::HashMap;

/// Escape double quotes in a multipart Content-Disposition (consistent with the frontend)
fn escape_disp(s: &str) -> String {
    s.replace('"', "\\\"")
}

/// Variable / dynamic-value interpolation (same policy as `pipeline::interp`: missing variables are kept as-is)
fn interp(s: &str, vars: &HashMap<String, String>) -> String {
    orbit_config::interpolate(s, vars).unwrap_or_else(|_| s.to_string())
}

const HEX: [u8; 16] = *b"0123456789ABCDEF";

/// Equivalent implementation of JS `encodeURIComponent`.
///
/// Keeps `A-Za-z0-9-_.!~*'()`; everything else is UTF-8 byte-encoded as `%XX` (uppercase hex).
/// Must match JS character for character: the engine encodes URL path / query values and sends them directly,
/// and an encoding difference changes the parameter values the server parses.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            );
        if keep {
            out.push(*byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

/// Parse the host (including a non-default port) from a URL; used to auto-fill the `Host` header.
/// Consistent with the frontend `hostFromUrl`: for a non-http prefix (tcp:// etc.) take the part before the first `/`.
pub fn host_from_url(url: &str) -> String {
    if url.starts_with("http") {
        return match url::Url::parse(url) {
            Ok(parsed) => {
                let host = parsed.host_str().unwrap_or("");
                match parsed.port() {
                    Some(port) => format!("{host}:{port}"),
                    None => host.to_string(),
                }
            }
            Err(_) => String::new(),
        };
    }
    match url.find('/') {
        Some(idx) => url[..idx].to_string(),
        None => url.to_string(),
    }
}

/// Connection-level (hop-by-hop) headers: computed by the underlying HTTP client itself;
/// attaching them manually causes duplicate / conflicting headers (some legacy servers return 400), so they must be stripped before sending.
pub fn is_hop_by_hop(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "host" | "content-length" | "transfer-encoding" | "connection" | "keep-alive"
    )
}

/// Form field template (either a text value or a file read from its real path).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FormParam {
    pub key: String,
    /// Text value (empty for file fields)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Real absolute path of the file (read from disk when sending)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// File content as base64 (browser preview mode: fallback when the real path is unavailable)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    /// File MIME
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    /// Original file name (used in `Content-Disposition`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

/// Mode of a text request body (determines Content-Type derivation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextBodyMode {
    Json,
    Xml,
    /// Arbitrary text; Content-Type may take the user-specified value, defaulting to `text/plain`
    Raw,
}

/// Request body template (un-interpolated).
///
/// Wire format (`tag = "mode"`):
/// ```json
/// { "mode": "text", "format": "json" }
/// { "mode": "urlencoded", "params": [["a", "{{v}}"]] }
/// { "mode": "multipart",  "params": [{ "key": "f", "file_path": "C:/a.png" }] }
/// { "mode": "binary",     "file_path": "C:/a.bin" }
/// { "mode": "none" }
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum BodyTemplate {
    #[default]
    None,
    /// json / xml / raw: text body template.
    ///
    /// `text` is the **initial** body: the expand stage copies it to `PipelineSpec.body_value`, which is authoritative from then on
    /// (pre-interpolation scripts read/write it via `pm.request.body.raw`, and the interpolation stage operates on it too).
    Text {
        format: TextBodyMode,
        #[serde(default)]
        text: String,
        /// User-specified Content-Type in raw mode (defaults to `text/plain`)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_type: Option<String>,
    },
    /// `application/x-www-form-urlencoded`
    Urlencoded {
        #[serde(default)]
        params: Vec<(String, String)>,
    },
    /// `multipart/form-data` (file fields are read from disk by path)
    Multipart {
        #[serde(default)]
        params: Vec<FormParam>,
    },
    /// Binary body: read from disk by path, or base64 fallback (browser preview mode)
    Binary {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base64: Option<String>,
    },
}

/// Un-interpolated request template for the single-send path.
///
/// Constructed by the host (Tauri command / HTTP API / AI tool host) from the UI configuration;
/// the engine handles interpolation, encoding, joining and final header computation.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RequestTemplate {
    /// URL template (query not appended, path not substituted)
    pub url: String,
    /// Path parameter template: after interpolation, replaces `{key}` in the URL with percent-encoding (an empty value keeps the placeholder)
    pub path_params: Vec<(String, String)>,
    /// Query parameter template: after interpolation, encoded and appended (skipped when the key is empty)
    pub query_params: Vec<(String, String)>,
    /// Automatic default header template (**excluding** Content-Type: derived from the body mode in the finalize stage)
    pub default_headers: Vec<(String, String)>,
    /// Auth header template (Bearer / Basic / apikey-in-header)
    pub auth_headers: Vec<(String, String)>,
    /// User-configured header template (highest priority; an explicitly configured Content-Type is not overridden)
    pub user_headers: Vec<(String, String)>,
    /// Enabled Cookie entries: joined into a `Cookie` header after interpolation
    pub cookies: Vec<(String, String)>,
    pub body: BodyTemplate,
}

impl RequestTemplate {
    /// Stage one: expand into the "un-interpolated request skeleton" (URL template + merged header template).
    ///
    /// **No interpolation** - pre-interpolation scripts must see the raw template (`{{var}}` kept as-is).
    /// Headers are merged as "default headers -> auth headers -> user headers"; on a name collision the later one overrides the earlier one.
    pub fn expand_template(&self) -> (String, HashMap<String, String>) {
        let mut headers: HashMap<String, String> = HashMap::new();
        // Cookie entries have the lowest priority: an explicitly configured Cookie header overrides them
        if let Some(cookie) = self.cookie_header_template() {
            headers.insert("Cookie".to_string(), cookie);
        }
        for group in [
            &self.default_headers,
            &self.auth_headers,
            &self.user_headers,
        ] {
            for (k, v) in group.iter() {
                if k.is_empty() {
                    continue;
                }
                headers.insert(k.clone(), v.clone());
            }
        }
        (self.url.clone(), headers)
    }

    /// The **initial** body of a text request body (returns `None` for non-text bodies).
    ///
    /// The expand stage copies it to `PipelineSpec.body_value` for pre-interpolation scripts and the interpolation stage to read/write.
    pub fn text_body(&self) -> Option<&str> {
        match &self.body {
            BodyTemplate::Text { text, .. } => Some(text.as_str()),
            _ => None,
        }
    }

    /// Whether the user explicitly configured Content-Type in the headers (if so, it is not overridden by the body mode).
    fn content_type_explicit(&self) -> bool {
        self.user_headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
    }

    /// Stage two a: interpolation + path parameter substitution + query parameter append -> final URL.
    ///
    /// `url_template` is the URL rewritten by pre-interpolation scripts (or `self.url` when unmodified).
    pub fn finalize_url(&self, url_template: &str, vars: &HashMap<String, String>) -> String {
        let mut url = interp(url_template, vars);
        for (key, value) in &self.path_params {
            if key.is_empty() {
                continue;
            }
            let resolved = interp(value, vars);
            if resolved.is_empty() {
                // An empty value keeps the `{key}` placeholder (consistent with the frontend applyPathParams)
                continue;
            }
            let encoded = encode_uri_component(&resolved);
            url = url.replace(&format!("{{{key}}}"), &encoded);
        }
        let pairs: Vec<(String, String)> = self
            .query_params
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .map(|(k, v)| (interp(k, vars), interp(v, vars)))
            .collect();
        if pairs.is_empty() {
            return url;
        }
        let qs = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", encode_uri_component(k), encode_uri_component(v)))
            .collect::<Vec<_>>()
            .join("&");
        let sep = if url.contains('?') { '&' } else { '?' };
        format!("{url}{sep}{qs}")
    }

    /// Stage two b: interpolation + encoding -> final body bytes, plus the Content-Type to attach.
    ///
    /// `body_value`: pre-interpolation scripts may have rewritten the text body; for structured bodies (urlencoded / multipart /
    /// binary), if a script set it to non-empty text it is treated as **the script taking over the whole body**,
    /// and those text bytes are sent directly (no further structured assembly).
    pub fn finalize_body(
        &self,
        body_value: Option<&serde_yaml::Value>,
        vars: &HashMap<String, String>,
    ) -> Result<(Vec<u8>, Option<String>), String> {
        let text = body_value.and_then(|v| v.as_str()).map(|s| interp(s, vars));
        match &self.body {
            BodyTemplate::Text {
                format,
                content_type,
                ..
            } => {
                let body = text.unwrap_or_default();
                let ct = match format {
                    TextBodyMode::Json => "application/json".to_string(),
                    TextBodyMode::Xml => "application/xml".to_string(),
                    TextBodyMode::Raw => content_type
                        .as_deref()
                        .map(|s| interp(s, vars))
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "text/plain".to_string()),
                };
                Ok((body.into_bytes(), Some(ct)))
            }
            // A script took over the body (pm.request.body.raw written under a structured / no-body mode):
            // send the text the script wrote, skip structured assembly, and leave Content-Type to the user's configuration.
            _ if text.is_some() => Ok((text.unwrap_or_default().into_bytes(), None)),
            BodyTemplate::None => Ok((Vec::new(), None)),
            BodyTemplate::Urlencoded { params } => {
                let body = params
                    .iter()
                    .filter(|(k, _)| !k.is_empty())
                    .map(|(k, v)| {
                        format!(
                            "{}={}",
                            encode_uri_component(&interp(k, vars)),
                            encode_uri_component(&interp(v, vars))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("&");
                Ok((
                    body.into_bytes(),
                    Some("application/x-www-form-urlencoded".to_string()),
                ))
            }
            BodyTemplate::Multipart { params } => {
                let (bytes, ct) = build_multipart(params, vars)?;
                Ok((bytes, Some(ct)))
            }
            BodyTemplate::Binary { file_path, base64 } => {
                let bytes = if let Some(path) = file_path.as_deref().filter(|p| !p.is_empty()) {
                    std::fs::read(path)
                        .map_err(|e| format!("failed to read file {}: {}", path, e))?
                } else if let Some(data) = base64.as_deref() {
                    decode_base64(data)
                } else {
                    Vec::new()
                };
                let ct = if bytes.is_empty() {
                    None
                } else {
                    Some("application/octet-stream".to_string())
                };
                Ok((bytes, ct))
            }
        }
    }

    /// Stage two c: fill in `Content-Type` / `Host` / `Content-Length`.
    ///
    /// - `Content-Type`: keep the user value when explicitly configured, otherwise use the value derived from the body mode;
    /// - `Host` / `Content-Length`: transport-derived values used only for request snapshot display,
    ///   and stripped before sending by [`is_hop_by_hop`].
    pub fn finalize_headers(
        &self,
        headers: &mut HashMap<String, String>,
        content_type: Option<String>,
        target: &str,
        body_len: usize,
    ) {
        if !self.content_type_explicit() {
            match content_type {
                Some(ct) => {
                    headers.insert("Content-Type".to_string(), ct);
                }
                None => {
                    headers.remove("Content-Type");
                }
            }
        }
        let host = host_from_url(target);
        if !host.is_empty() {
            headers.insert("Host".to_string(), host);
        }
        if body_len > 0 {
            headers.insert("Content-Length".to_string(), body_len.to_string());
        }
    }

    /// Enabled Cookie entries -> `Cookie` header **template** (`name=value` joined with `; `, un-interpolated).
    pub fn cookie_header_template(&self) -> Option<String> {
        let joined = self
            .cookies
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        if joined.is_empty() {
            None
        } else {
            Some(joined)
        }
    }
}

/// Base64 decode; invalid input yields empty (consistent with the frontend `base64ToBytes` fallback).
fn decode_base64(data: &str) -> Vec<u8> {
    use base64::prelude::*;
    let clean: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    BASE64_STANDARD.decode(clean).unwrap_or_default()
}

/// Assemble a multipart/form-data byte stream (text fields write their value directly, file fields are read from disk by path).
///
/// Returns `(bytes, Content-Type)`; the boundary is regenerated on every build to avoid colliding with
/// an identical byte sequence in the body.
fn build_multipart(
    params: &[FormParam],
    vars: &HashMap<String, String>,
) -> Result<(Vec<u8>, String), String> {
    let boundary = format!("----orbitFormBoundary{}", unique_suffix());
    let mut out: Vec<u8> = Vec::new();

    for p in params {
        let key = interp(&p.key, vars);
        if key.is_empty() {
            continue;
        }
        // File field: prefer reading from the real path (Tauri), fall back to base64 when the path is unavailable (browser preview)
        let file_bytes: Option<Vec<u8>> =
            match p.file_path.as_deref().filter(|path| !path.is_empty()) {
                Some(path) => Some(
                    std::fs::read(path)
                        .map_err(|e| format!("failed to read file {}: {}", path, e))?,
                ),
                None => p.base64.as_deref().map(decode_base64),
            };
        match file_bytes {
            Some(bytes) => {
                let filename = p.filename.clone().unwrap_or_else(|| {
                    p.file_path
                        .as_deref()
                        .and_then(|path| std::path::Path::new(path).file_name())
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                });
                let file_type = p
                    .file_type
                    .clone()
                    .unwrap_or_else(|| "application/octet-stream".to_string());
                out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                        escape_disp(&key),
                        escape_disp(&interp(&filename, vars))
                    )
                    .as_bytes(),
                );
                out.extend_from_slice(format!("Content-Type: {}\r\n\r\n", file_type).as_bytes());
                out.extend_from_slice(&bytes);
                out.extend_from_slice(b"\r\n");
            }
            None => {
                let value = interp(p.value.as_deref().unwrap_or_default(), vars);
                out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                        escape_disp(&key)
                    )
                    .as_bytes(),
                );
                out.extend_from_slice(value.as_bytes());
                out.extend_from_slice(b"\r\n");
            }
        }
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    Ok((out, format!("multipart/form-data; boundary={boundary}")))
}

/// Generate a process-unique boundary suffix (timestamp nanoseconds + auto-increment counter, avoiding duplicates within a run).
fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", nanos, SEQ.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn encode_uri_component_matches_js() {
        // JS: encodeURIComponent("a b&c=d") === "a%20b%26c%3Dd"
        assert_eq!(encode_uri_component("a b&c=d"), "a%20b%26c%3Dd");
        // Unreserved character set: A-Za-z0-9-_.!~*'()
        assert_eq!(encode_uri_component("AZaz09-_.!~*'()"), "AZaz09-_.!~*'()");
        // Non-ASCII is encoded byte by byte as UTF-8
        assert_eq!(encode_uri_component("a b"), "a%20b");
        // `+` and `#` must be encoded (servers treat `+` as a space)
        assert_eq!(encode_uri_component("a+b#c"), "a%2Bb%23c");
    }

    #[test]
    fn host_from_url_matches_frontend() {
        assert_eq!(
            host_from_url("https://api.example.com/v1"),
            "api.example.com"
        );
        assert_eq!(host_from_url("http://127.0.0.1:8080/x"), "127.0.0.1:8080");
        // Default ports are omitted (consistent with JS `URL.host`)
        assert_eq!(
            host_from_url("https://api.example.com:443/v1"),
            "api.example.com"
        );
        // Non-http prefix: take the part before the first `/`. The first `/` of `tcp://host` is at `//`,
        // so the result is "tcp:" - character for character identical to the frontend `hostFromUrl`.
        // (The Host header for non-HTTP protocols is meaningless anyway; here we only require unchanged behavior.)
        assert_eq!(host_from_url("tcp://10.0.0.1:9000"), "tcp:");
        assert_eq!(host_from_url("tcp://10.0.0.1:9000/x"), "tcp:");
    }

    fn http_template() -> RequestTemplate {
        RequestTemplate {
            url: "https://api.example.com/{{ver}}/users/{id}".into(),
            path_params: vec![("id".into(), "{{userId}}".into())],
            query_params: vec![
                ("q".into(), "{{keyword}}".into()),
                ("".into(), "skipped".into()),
            ],
            default_headers: vec![
                ("Accept".into(), "*/*".into()),
                ("User-Agent".into(), "Orbit/1.0".into()),
            ],
            auth_headers: vec![("Authorization".into(), "Bearer {{token}}".into())],
            user_headers: vec![("X-Trace".into(), "{{trace}}".into())],
            cookies: vec![("sid".into(), "{{sid}}".into())],
            body: BodyTemplate::Text {
                format: TextBodyMode::Json,
                text: String::new(),
                content_type: None,
            },
        }
    }

    #[test]
    fn expand_template_keeps_placeholders_and_merges_headers() {
        let t = http_template();
        let (url, headers) = t.expand_template();
        // Un-interpolated: the template is kept as-is, so only pre-interpolation scripts see `{{...}}`
        assert_eq!(url, "https://api.example.com/{{ver}}/users/{id}");
        assert_eq!(headers.get("Authorization").unwrap(), "Bearer {{token}}");
        assert_eq!(headers.get("X-Trace").unwrap(), "{{trace}}");
        assert_eq!(headers.get("Accept").unwrap(), "*/*");
        // Default headers do not contain Content-Type (derived from the body mode in the finalize stage)
        assert!(!headers.contains_key("Content-Type"));
    }

    #[test]
    fn user_header_overrides_auth_and_default() {
        let mut t = http_template();
        t.user_headers.push(("X-Trace".into(), "user-wins".into()));
        t.auth_headers.push(("Accept".into(), "auth-loses".into()));
        let (_, headers) = t.expand_template();
        assert_eq!(headers.get("X-Trace").unwrap(), "user-wins");
        // user headers > auth headers > default headers
        assert_eq!(headers.get("Accept").unwrap(), "auth-loses");
    }

    #[test]
    fn finalize_url_interpolates_encodes_and_appends_query() {
        let t = http_template();
        let v = vars(&[("ver", "v2"), ("userId", "a b/c"), ("keyword", "a b&c")]);
        let url = t.finalize_url(&t.url, &v);
        assert_eq!(
            url,
            "https://api.example.com/v2/users/a%20b%2Fc?q=a%20b%26c"
        );
    }

    #[test]
    fn finalize_url_keeps_placeholder_when_path_value_empty() {
        let t = http_template();
        let v = vars(&[("ver", "v1"), ("userId", ""), ("keyword", "k")]);
        let url = t.finalize_url(&t.url, &v);
        assert!(
            url.contains("/users/{id}"),
            "an empty value should keep the placeholder: {url}"
        );
    }

    #[test]
    fn finalize_url_appends_with_amp_when_url_already_has_query() {
        let t = RequestTemplate {
            url: "https://x.dev/a?fixed=1".into(),
            query_params: vec![("q".into(), "v".into())],
            ..Default::default()
        };
        assert_eq!(
            t.finalize_url(&t.url, &HashMap::new()),
            "https://x.dev/a?fixed=1&q=v"
        );
    }

    #[test]
    fn finalize_body_text_modes_derive_content_type() {
        let v = vars(&[]);
        let json = RequestTemplate {
            body: BodyTemplate::Text {
                format: TextBodyMode::Json,
                text: String::new(),
                content_type: None,
            },
            ..Default::default()
        };
        let (bytes, ct) = json
            .finalize_body(
                Some(&serde_yaml::Value::String("{\"a\":\"{{x}}\"}".into())),
                &vars(&[("x", "1")]),
            )
            .unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "{\"a\":\"1\"}");
        assert_eq!(ct.as_deref(), Some("application/json"));

        let raw = RequestTemplate {
            body: BodyTemplate::Text {
                format: TextBodyMode::Raw,
                text: String::new(),
                content_type: None,
            },
            ..Default::default()
        };
        let (_, ct) = raw
            .finalize_body(Some(&serde_yaml::Value::String("hi".into())), &v)
            .unwrap();
        assert_eq!(ct.as_deref(), Some("text/plain"));

        let raw_custom = RequestTemplate {
            body: BodyTemplate::Text {
                format: TextBodyMode::Raw,
                text: "a,b".into(),
                content_type: Some("text/csv".into()),
            },
            ..Default::default()
        };
        let (_, ct) = raw_custom
            .finalize_body(Some(&serde_yaml::Value::String("a,b".into())), &v)
            .unwrap();
        assert_eq!(ct.as_deref(), Some("text/csv"));
    }

    #[test]
    fn text_body_exposes_only_text_templates() {
        let t = RequestTemplate {
            body: BodyTemplate::Text {
                format: TextBodyMode::Json,
                text: "{\"a\":\"{{x}}\"}".into(),
                content_type: None,
            },
            ..Default::default()
        };
        // The initial body stays un-interpolated for pre-interpolation scripts and the interpolation stage to read/write
        assert_eq!(t.text_body(), Some("{\"a\":\"{{x}}\"}"));
        assert_eq!(RequestTemplate::default().text_body(), None);
    }

    #[test]
    fn finalize_body_urlencoded_encodes_pairs() {
        let t = RequestTemplate {
            body: BodyTemplate::Urlencoded {
                params: vec![
                    ("a".into(), "1 2".into()),
                    ("".into(), "skipped".into()),
                    ("b".into(), "c d".into()),
                ],
            },
            ..Default::default()
        };
        let (bytes, ct) = t.finalize_body(None, &HashMap::new()).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "a=1%202&b=c%20d");
        assert_eq!(ct.as_deref(), Some("application/x-www-form-urlencoded"));
    }

    #[test]
    fn finalize_body_multipart_builds_boundary_and_fields() {
        let t = RequestTemplate {
            body: BodyTemplate::Multipart {
                params: vec![
                    FormParam {
                        key: "name".into(),
                        value: Some("{{who}}".into()),
                        ..Default::default()
                    },
                    FormParam {
                        key: "".into(),
                        value: Some("ignored".into()),
                        ..Default::default()
                    },
                ],
            },
            ..Default::default()
        };
        let (bytes, ct) = t.finalize_body(None, &vars(&[("who", "alice")])).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("name=\"name\"\r\n\r\nalice\r\n"), "{text}");
        assert!(!text.contains("ignored"));
        assert!(text.ends_with("--\r\n"));
        let ct = ct.unwrap();
        assert!(ct.starts_with("multipart/form-data; boundary=----orbitFormBoundary"));
    }

    #[test]
    fn finalize_body_multipart_accepts_base64_file_fallback() {
        // In browser preview mode the file content is delivered as base64 when the real path is unavailable
        let t = RequestTemplate {
            body: BodyTemplate::Multipart {
                params: vec![FormParam {
                    key: "avatar".into(),
                    base64: Some("aGk=".into()),
                    filename: Some("a.txt".into()),
                    file_type: Some("text/plain".into()),
                    ..Default::default()
                }],
            },
            ..Default::default()
        };
        let (bytes, _) = t.finalize_body(None, &HashMap::new()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(
            text.contains(
                "name=\"avatar\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhi\r\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn finalize_body_binary_decodes_base64_fallback() {
        let t = RequestTemplate {
            body: BodyTemplate::Binary {
                file_path: None,
                base64: Some("aGk=".into()),
            },
            ..Default::default()
        };
        let (bytes, ct) = t.finalize_body(None, &HashMap::new()).unwrap();
        assert_eq!(bytes, b"hi");
        assert_eq!(ct.as_deref(), Some("application/octet-stream"));
    }

    #[test]
    fn finalize_body_script_override_wins_over_structured_body() {
        // A pre-interpolation script rewrote pm.request.body.raw: structured body assembly yields and the bytes the script wrote are sent directly
        let t = RequestTemplate {
            body: BodyTemplate::Urlencoded {
                params: vec![("a".into(), "1".into())],
            },
            ..Default::default()
        };
        let (bytes, ct) = t
            .finalize_body(
                Some(&serde_yaml::Value::String("scripted={{v}}".into())),
                &vars(&[("v", "9")]),
            )
            .unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "scripted=9");
        // Content-Type is left to the user's configuration (no guessing)
        assert!(ct.is_none());
    }

    #[test]
    fn finalize_headers_sets_content_type_host_and_length() {
        let t = http_template();
        let mut headers = t.expand_template().1;
        t.finalize_headers(
            &mut headers,
            Some("application/json".into()),
            "https://api.example.com/v2/users/1?q=a",
            11,
        );
        assert_eq!(headers.get("Content-Type").unwrap(), "application/json");
        assert_eq!(headers.get("Host").unwrap(), "api.example.com");
        assert_eq!(headers.get("Content-Length").unwrap(), "11");
    }

    #[test]
    fn finalize_headers_respects_user_content_type() {
        let mut t = http_template();
        t.user_headers
            .push(("content-type".into(), "application/xml".into()));
        let mut headers = t.expand_template().1;
        t.finalize_headers(
            &mut headers,
            Some("application/json".into()),
            "https://api.example.com/x",
            3,
        );
        assert_eq!(headers.get("content-type").unwrap(), "application/xml");
        assert!(!headers.contains_key("Content-Type"));
    }

    #[test]
    fn hop_by_hop_headers_are_identified_case_insensitively() {
        for key in [
            "Host",
            "CONTENT-LENGTH",
            "Transfer-Encoding",
            "Connection",
            "Keep-Alive",
        ] {
            assert!(is_hop_by_hop(key), "{key} should be a hop-by-hop header");
        }
        assert!(!is_hop_by_hop("Content-Type"));
        assert!(!is_hop_by_hop("Cookie"));
    }

    #[test]
    fn cookies_are_merged_as_lowest_priority_cookie_header() {
        let t = http_template();
        let (_, headers) = t.expand_template();
        // Template form: placeholders are kept as-is and resolved by the interpolation stage
        assert_eq!(headers.get("Cookie").unwrap(), "sid={{sid}}");
        assert!(RequestTemplate::default()
            .cookie_header_template()
            .is_none());

        // An explicitly configured Cookie header takes priority
        let mut custom = http_template();
        custom
            .user_headers
            .push(("Cookie".into(), "manual=1".into()));
        let (_, headers) = custom.expand_template();
        assert_eq!(headers.get("Cookie").unwrap(), "manual=1");
    }

    #[test]
    fn template_roundtrips_through_serde() {
        let t = http_template();
        let json = serde_json::to_string(&t).unwrap();
        let back: RequestTemplate = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
}
