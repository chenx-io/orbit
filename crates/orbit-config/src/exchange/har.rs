//! HAR importer (HTTP Archive traffic log -> ApiSpec)
//!
//! Coverage (baseline: w3c.github.io/web-performance/specs/HAR/Overview.html):
//! - request：method/url/queryString/cookies/headers/postData(mimeType/text)
//! - response: status/headers/content (pass through to extensions)

use std::collections::HashMap;

use super::ir::{ApiSpec, EndpointSpec, ResponseSpec};
use super::{ImportError, ImportFormat};
use crate::model::request::{HttpRequestConfig, RequestSpec};

/// HAR: import only (restores endpoints from the traffic log)
pub(crate) struct HarImporter;

impl ImportFormat for HarImporter {
    fn name(&self) -> &'static str {
        "har"
    }

    fn parse(&self, input: &str) -> Result<ApiSpec, ImportError> {
        let har: serde_json::Value = serde_json::from_str(input)
            .map_err(|e| ImportError::Parse(format!("Invalid HAR JSON: {}", e)))?;

        let entries = har["log"]["entries"]
            .as_array()
            .ok_or_else(|| ImportError::Parse("No entries in HAR".into()))?;

        let mut spec = ApiSpec::new("HAR Import");
        // pass through unstructured content (log metadata/timings/cache/response details)
        if let Some(obj) = har.get("log").and_then(|v| v.as_object().cloned()) {
            spec.raw = obj;
        }
        for entry in entries {
            let request = &entry["request"];
            let method = request["method"].as_str().unwrap_or("GET").to_uppercase();
            let url = request["url"].as_str().unwrap_or("").to_string();

            let mut headers = HashMap::new();
            for h in request["headers"].as_array().unwrap_or(&vec![]) {
                let name = h["name"].as_str().unwrap_or("");
                let value = h["value"].as_str().unwrap_or("");
                if !name.is_empty() {
                    headers.insert(name.to_string(), value.to_string());
                }
            }

            // cookies -> Cookie header (HAR standard field)
            let cookies: Vec<String> = request["cookies"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|c| {
                    let name = c.get("name")?.as_str()?;
                    let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
                    Some(format!("{}={}", name, value))
                })
                .collect();
            if !cookies.is_empty() {
                headers
                    .entry("Cookie".to_string())
                    .or_insert(cookies.join("; "));
            }

            // queryString -> display params
            let mut query_params = HashMap::new();
            for q in request["queryString"].as_array().unwrap_or(&vec![]) {
                if let Some(name) = q.get("name").and_then(|v| v.as_str()) {
                    let value = q.get("value").and_then(|v| v.as_str()).unwrap_or("");
                    if !name.is_empty() {
                        query_params.insert(name.to_string(), value.to_string());
                    }
                }
            }

            // postData：mimeType + text
            let body = request["postData"]["text"].as_str().map(|t| {
                serde_json::from_str::<serde_yaml::Value>(t)
                    .unwrap_or(serde_yaml::Value::String(t.to_string()))
            });
            let content_type = request["postData"]["mimeType"]
                .as_str()
                .map(String::from)
                .filter(|s| !s.is_empty());

            let mut ep = EndpointSpec::new(
                format!("{} {}", method, url),
                RequestSpec::Http(Box::new(HttpRequestConfig {
                    method,
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
            ep.query_params = query_params;
            ep.content_type = content_type;

            // response examples (status code + response body)
            let status = entry["response"]["status"].as_u64().unwrap_or(0) as u16;
            if status > 0 {
                let resp_body = entry["response"]["content"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                if !resp_body.is_empty() {
                    ep.responses.push(ResponseSpec {
                        status,
                        name: entry["response"]["statusText"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                        body: resp_body,
                        schema: None,
                    });
                }
            }
            spec.endpoints.push(ep);
        }

        if spec.endpoints.is_empty() {
            return Err(ImportError::Parse("No entries found in HAR".into()));
        }
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_har_import() {
        let har = r#"{"log":{"entries":[{"request":{
            "method":"POST","url":"https://api.example.com/users?page=2",
            "headers":[{"name":"Accept","value":"application/json"}],
            "cookies":[{"name":"session","value":"abc"}],
            "queryString":[{"name":"page","value":"2"}],
            "postData":{"mimeType":"application/json","text":"{\"a\":1}"}
        },"response":{"status":200,"statusText":"OK","content":{"mimeType":"application/json","text":"{\"id\":1}"}}}]}}"#;
        let spec = HarImporter.parse(har).unwrap();
        assert_eq!(spec.endpoints.len(), 1);
        let ep = &spec.endpoints[0];
        match &ep.request {
            RequestSpec::Http(h) => {
                assert_eq!(h.url, "https://api.example.com/users?page=2");
                assert_eq!(h.headers.get("Cookie").unwrap(), "session=abc");
                assert!(h.body.is_some());
            }
            _ => panic!("expected Http"),
        }
        assert_eq!(ep.query_params.get("page").unwrap(), "2");
        assert_eq!(ep.content_type.as_deref(), Some("application/json"));
        // response examples
        assert_eq!(ep.responses.len(), 1);
        assert_eq!(ep.responses[0].status, 200);
        assert!(ep.responses[0].body.contains("\"id\""));
    }
}
