//! `orbit mock` — start a mock server from a test plan

use std::path::PathBuf;
use std::sync::Arc;

use orbit_server::mock::{MockExpectation, MockInterface};
use tokio::sync::RwLock;

use crate::util::read_plan;

pub async fn mock_command(file: PathBuf, port: u16) -> anyhow::Result<()> {
    let plan = read_plan(&file)?;
    let vars = &plan.variables;

    let mut interfaces = Vec::new();
    for s in &plan.scenarios {
        for step in &s.steps {
            let Some(req) = step.as_http() else {
                continue;
            };
            // Parse the URL after variable interpolation; mock routes match on the request path (excluding host/query)
            let url = orbit_config::interpolate(&req.url, vars).map_err(|e| {
                anyhow::anyhow!("step {}: URL interpolation failed: {}", step.name(), e)
            })?;
            let parsed = url::Url::parse(&url).map_err(|e| {
                anyhow::anyhow!("step {}: invalid URL '{}': {}", step.name(), url, e)
            })?;
            let path = if parsed.path().is_empty() {
                "/".to_string()
            } else {
                parsed.path().to_string()
            };
            let method = req.method.to_uppercase();

            // Interpolate the request body (the mock response is the JSON serialization directly; defaults to {"status":"ok"} when there is no body)
            let body = match &req.body {
                Some(b) => {
                    let text = serde_json::to_string(b).unwrap_or_default();
                    let resolved =
                        orbit_config::interpolate(&text, vars).unwrap_or_else(|_| text.clone());
                    serde_json::from_str(&resolved)
                        .unwrap_or_else(|_| serde_json::json!({ "raw": resolved }))
                }
                None => serde_json::json!({ "status": "ok" }),
            };

            eprintln!("  📋 {} {} → 200", req.method, path);
            interfaces.push(MockInterface {
                request_id: None,
                workspace_id: None,
                method,
                path,
                enabled: true,
                expectations: vec![MockExpectation {
                    id: format!("cli-{}", interfaces.len()),
                    name: format!("{} {}", req.method, req.url),
                    enabled: true,
                    conditions: Vec::new(),
                    ip_condition: Default::default(),
                    status: 200,
                    headers: vec![("Content-Type".into(), "application/json".into())]
                        .into_iter()
                        .collect(),
                    body: serde_json::to_string(&body).unwrap_or_default(),
                    delay_ms: 0,
                }],
            });
        }
    }

    let server =
        orbit_server::mock::MockServer::with_rules(port, Arc::new(RwLock::new(interfaces)));
    eprintln!("🎭 Mock server starting on port {}...", port);
    server.start().await?;
    Ok(())
}
