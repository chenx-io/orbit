//! k6 script importer (JS load-test script -> ApiSpec + executor hint)
//!
//! Coverage (baseline: grafana.com/docs/k6/latest):
//! - http.get/post/put/delete/patch calls (headers/body params)
//! - check() assertions (status === N) -> scenario checks
//! - options：vus/duration → ConstantVus；stages → RampingVus

use std::collections::HashMap;

use super::ir::{ApiSpec, EndpointSpec};
use super::{ImportError, ImportFormat};
use crate::model::check::{Check, CheckKind};
use crate::model::plan::{Executor, RampMode, RampingStage};
use crate::model::request::{HttpRequestConfig, RequestSpec};

/// k6: import only (scripts have no standard endpoint definition, so it produces endpoints + an executor hint)
pub(crate) struct K6Importer;

impl ImportFormat for K6Importer {
    fn name(&self) -> &'static str {
        "k6"
    }

    fn parse(&self, script: &str) -> Result<ApiSpec, ImportError> {
        let mut spec = ApiSpec::new("Imported from k6");

        // parse http.get / http.post calls (including call position, used to associate check)
        let re = regex::Regex::new(
            r#"http\.(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']\s*(?:,\s*(\{[^}]+\}))?\s*(?:,\s*(\{[^}]+\}))?\s*\)"#
        ).map_err(|e| ImportError::Parse(e.to_string()))?;

        let mut steps = Vec::new();
        for cap in re.captures_iter(script) {
            let method = cap[1].to_uppercase();
            let url = cap[2].to_string();
            let body = cap.get(3).map(|m| {
                let s = m.as_str().to_string();
                serde_json::from_str::<serde_yaml::Value>(&s)
                    .unwrap_or(serde_yaml::Value::String(s))
            });

            let mut headers = HashMap::new();
            if let Some(params) = cap.get(4) {
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(params.as_str()) {
                    if let Some(h) = obj.get("headers").and_then(|v| v.as_object()) {
                        for (k, v) in h {
                            headers.insert(k.clone(), v.as_str().unwrap_or("").to_string());
                        }
                    }
                }
            }

            let mut ep = EndpointSpec::new(
                format!("k6_{}_{}", method.to_lowercase(), sanitize_step_name(&url)),
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

            // check() assertion: look for status === N / status == N within a 400-character window after the http call
            if let Some(rel) = cap.get(0) {
                let window = &script[rel.end()..script.len().min(rel.end() + 400)];
                if let Some(status_cap) = k6_status_check(window) {
                    ep.checks.push(Check {
                        kind: CheckKind::Status { value: status_cap },
                        meta: None,
                    });
                }
            }
            steps.push(ep);
        }

        if steps.is_empty() {
            // try a simpler pattern match
            let re_simple =
                regex::Regex::new(r#"http\.(get|post|put|delete)\s*\(\s*["']([^"']+)["']"#)
                    .map_err(|e| ImportError::Parse(e.to_string()))?;

            for cap in re_simple.captures_iter(script) {
                let method = cap[1].to_uppercase();
                let url = cap[2].to_string();
                steps.push(EndpointSpec::new(
                    format!("k6_{}_{}", method.to_lowercase(), sanitize_step_name(&url)),
                    RequestSpec::Http(Box::new(HttpRequestConfig {
                        method,
                        url,
                        headers: HashMap::new(),
                        body: None,
                        timeout: "30s".to_string(),
                        payload_format: None,
                        grpc_service: None,
                        grpc_use_reflection: false,
                        response_format: None,
                    })),
                ));
            }
        }

        spec.endpoints = steps;

        // options: stages take precedence (RampingVus), otherwise vus/duration (ConstantVus)
        let stages = k6_stages(script);
        if !stages.is_empty() {
            spec.executor = Some(Executor::RampingVus {
                start_vus: 0,
                max_vus: 0,
                stages,
            });
        } else {
            let vus = regex::Regex::new(r#"vus\s*:\s*(\d+)"#)
                .ok()
                .and_then(|re| re.captures(script))
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u32>().ok())
                .unwrap_or(1);
            let duration = regex::Regex::new(r#"duration\s*:\s*["'](\d+s|\d+m)["']"#)
                .ok()
                .and_then(|re| re.captures(script))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "30s".to_string());
            spec.executor = Some(Executor::ConstantVus {
                vus,
                duration,
                ramp_up: "0s".to_string(),
            });
        }

        Ok(spec)
    }
}

/// Extract the k6 check status assertion (`status === 200` / `== 200`) within the window after the http call.
fn k6_status_check(window: &str) -> Option<i32> {
    let re = regex::Regex::new(r#"status\s*===?\s*(\d+)"#).ok()?;
    re.captures(window)?.get(1)?.as_str().parse::<i32>().ok()
}

/// Parse k6 options.stages -> list of RampingStage.
fn k6_stages(script: &str) -> Vec<RampingStage> {
    // extract stages: [ {...}, {...} ]
    let Some(start) = script.find("stages") else {
        return Vec::new();
    };
    let Some(bracket) = script[start..].find('[') else {
        return Vec::new();
    };
    let from = start + bracket;
    let Some(end) = script[from..].find(']') else {
        return Vec::new();
    };
    let block = &script[from..from + end];

    let item_re = regex::Regex::new(r#"\{[^}]*\}"#).ok();
    let dur_re = regex::Regex::new(r#"duration\s*:\s*["']([^"']+)["']"#).ok();
    let target_re = regex::Regex::new(r#"target\s*:\s*(\d+)"#).ok();
    let (Some(item_re), Some(dur_re), Some(target_re)) = (item_re, dur_re, target_re) else {
        return Vec::new();
    };

    let mut stages = Vec::new();
    for item in item_re.captures_iter(block) {
        let item = &item[0];
        let duration = dur_re
            .captures(item)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "30s".to_string());
        let target = target_re
            .captures(item)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(0);
        stages.push(RampingStage {
            target,
            duration,
            ramp: RampMode::Gradual,
            ramp_up: None,
        });
    }
    stages
}

fn sanitize_step_name(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .replace('/', "_")
        .chars()
        .take(50)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_k6_simple_import() {
        let script = r#"
import http from 'k6/http';
export default function() {
    http.get('https://api.example.com/users');
    http.post('https://api.example.com/login');
}
"#;
        let spec = K6Importer.parse(script).unwrap();
        assert_eq!(spec.endpoints.len(), 2);
        assert!(spec.endpoints[0].name.contains("users"));
        assert!(matches!(spec.executor, Some(Executor::ConstantVus { .. })));
    }

    /// check assertions -> scenario checks; stages -> RampingVus
    #[test]
    fn test_k6_checks_and_stages() {
        let script = r#"
import http from 'k6/http';
import { check } from 'k6';
export const options = {
    stages: [
        { duration: '30s', target: 10 },
        { duration: '1m', target: 50 },
    ],
};
export default function() {
    const res = http.get('https://api.example.com/users');
    check(res, { 'status is 200': (r) => r.status === 200 });
}
"#;
        let spec = K6Importer.parse(script).unwrap();
        // check extraction
        assert_eq!(spec.endpoints.len(), 1);
        assert_eq!(spec.endpoints[0].checks.len(), 1);
        match &spec.endpoints[0].checks[0].kind {
            CheckKind::Status { value } => assert_eq!(*value, 200),
            _ => panic!("expected Status check"),
        }
        // stages -> RampingVus (2 stages)
        match spec.executor {
            Some(Executor::RampingVus { stages, .. }) => {
                assert_eq!(stages.len(), 2);
                assert_eq!(stages[0].target, 10);
                assert_eq!(stages[1].duration, "1m");
            }
            other => panic!("expected RampingVus, got {:?}", other),
        }
    }
}
