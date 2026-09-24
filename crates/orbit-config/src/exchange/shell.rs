//! Request-level export: wget / httpie / xh / powershell / fetch / python
//!
//! Input is [`EndpointSpec`] (HTTP variant); output is command/code text.

use super::curl::http_of;
use super::ir::EndpointSpec;
use super::{ExportError, ExportFormat};
use crate::model::request::HttpRequestConfig;

/// Multi-format instance exporter (each instance is bound to one format name)
pub(crate) struct ShellExporter(pub(crate) &'static str);

pub(crate) const WGET: ShellExporter = ShellExporter("wget");
pub(crate) const HTTPIE: ShellExporter = ShellExporter("httpie");
pub(crate) const XH: ShellExporter = ShellExporter("xh");
pub(crate) const POWERSHELL: ShellExporter = ShellExporter("powershell");
pub(crate) const FETCH: ShellExporter = ShellExporter("fetch");
pub(crate) const PYTHON: ShellExporter = ShellExporter("python");

impl ExportFormat for ShellExporter {
    fn name(&self) -> &'static str {
        self.0
    }

    fn export_request(&self, ep: &EndpointSpec) -> Result<String, ExportError> {
        let h = http_of(ep)?;
        let out = match self.0 {
            "wget" => export_wget(h),
            "httpie" => export_httpie(h),
            "xh" => export_xh(h),
            "powershell" => export_powershell(h),
            "fetch" => export_fetch(h),
            "python" => export_python(h),
            _ => return Err(ExportError::Unsupported(self.0.into())),
        };
        Ok(out)
    }
}

fn body_text(h: &HttpRequestConfig) -> String {
    h.body
        .as_ref()
        .map(super::curl::body_to_text)
        .unwrap_or_default()
}

fn export_wget(h: &HttpRequestConfig) -> String {
    let mut parts = vec!["wget".to_string(), "--method=POST".to_string()];
    for (k, v) in &h.headers {
        parts.push(format!("--header='{}: {}'", k, v));
    }
    let body = body_text(h);
    if !body.is_empty() {
        parts.push(format!("--body-data='{}'", body));
    }
    parts.push(format!("'{}'", h.url));
    parts.join(" \\\n  ")
}

fn export_httpie(h: &HttpRequestConfig) -> String {
    let mut parts = vec!["http".to_string(), h.method.to_lowercase()];
    for (k, v) in &h.headers {
        parts.push(format!("{}:'{}'", k, v));
    }
    let body = body_text(h);
    if !body.is_empty() {
        parts.push(format!("'{}'", body));
    }
    parts.push(format!("'{}'", h.url));
    parts.join(" \\\n  ")
}

fn export_xh(h: &HttpRequestConfig) -> String {
    let mut parts = vec!["xh".to_string(), h.method.to_lowercase()];
    for (k, v) in &h.headers {
        parts.push(format!("{}:'{}'", k, v));
    }
    let body = body_text(h);
    if !body.is_empty() {
        parts.push(format!("'{}'", body));
    }
    parts.push(format!("'{}'", h.url));
    parts.join(" \\\n  ")
}

fn export_powershell(h: &HttpRequestConfig) -> String {
    let mut parts = vec!["curl.exe".to_string()];
    parts.push(format!("-Method {}", h.method));
    parts.push(format!("-Uri '{}'", h.url));
    let body = body_text(h);
    if !body.is_empty() {
        parts.push(format!("-Body '{}'", body));
    }
    let headers: Vec<String> = h
        .headers
        .iter()
        .map(|(k, v)| format!("@{{ {} = '{}' }}", k, v))
        .collect();
    if !headers.is_empty() {
        parts.push(format!(
            "-Headers @{{\n    {}\n  }}",
            headers.join(";\n    ")
        ));
    }
    parts.join(" \\\n  ")
}

fn export_fetch(h: &HttpRequestConfig) -> String {
    let mut lines = vec![format!("fetch('{}', {{", h.url)];
    lines.push(format!("  method: '{}',", h.method));
    if !h.headers.is_empty() {
        let hs: Vec<String> = h
            .headers
            .iter()
            .map(|(k, v)| format!("    '{}': '{}'", k, v))
            .collect();
        lines.push(format!("  headers: {{\n{}\n  }},", hs.join(",\n")));
    }
    let body = body_text(h);
    if !body.is_empty() {
        lines.push(format!("  body: JSON.stringify({}),", body));
    }
    lines.push("});".to_string());
    lines.join("\n")
}

fn export_python(h: &HttpRequestConfig) -> String {
    let method_lower = h.method.to_lowercase();
    let mut lines = vec!["import requests".to_string(), String::new()];
    let hs: Vec<String> = h
        .headers
        .iter()
        .map(|(k, v)| format!("    '{}': '{}'", k, v))
        .collect();
    let body = body_text(h);
    if hs.is_empty() {
        lines.push(format!("response = requests.{}('{}')", method_lower, h.url));
    } else if !body.is_empty() {
        lines.push(format!(
            "response = requests.{}(\n    '{}',\n    headers={{\n{}\n    }},\n    json='{}'\n)",
            method_lower,
            h.url,
            hs.join(",\n"),
            body
        ));
    } else {
        lines.push(format!(
            "response = requests.{}(\n    '{}',\n    headers={{\n{}\n    }}\n)",
            method_lower,
            h.url,
            hs.join(",\n")
        ));
    }
    lines.push("print(response.status_code)".to_string());
    lines.push("print(response.text)".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn h() -> HttpRequestConfig {
        HttpRequestConfig {
            method: "POST".into(),
            url: "https://api.example.com/login".into(),
            headers: HashMap::from([("Content-Type".into(), "application/json".into())]),
            body: Some(serde_yaml::Value::String("{\"a\":1}".into())),
            timeout: "30s".into(),
            payload_format: None,
            grpc_service: None,
            grpc_use_reflection: false,
            response_format: None,
        }
    }

    fn ep() -> EndpointSpec {
        EndpointSpec::new("login", crate::RequestSpec::Http(Box::new(h())))
    }

    #[test]
    fn test_exporters() {
        for fmt in ["wget", "httpie", "xh", "powershell", "fetch", "python"] {
            let out = ShellExporter(fmt).export_request(&ep()).unwrap();
            assert!(!out.is_empty(), "{} output is empty", fmt);
        }
        assert!(ShellExporter("wget")
            .export_request(&ep())
            .unwrap()
            .contains("--body-data"));
        assert!(ShellExporter("python")
            .export_request(&ep())
            .unwrap()
            .contains("import requests"));
    }
}
