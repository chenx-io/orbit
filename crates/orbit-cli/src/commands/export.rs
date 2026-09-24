//! `orbit export` — export a test plan to curl/wget/postman/openapi etc. (reuses orbit-config::exchange)

use std::collections::HashMap;
use std::path::PathBuf;

use orbit_config::exchange::{self, ApiSpec, EndpointSpec};
use orbit_config::{HttpRequestConfig, RequestSpec};

use crate::util::read_plan;

/// Interpolate with plan.variables to produce directly executable commands (placeholders replaced by real values)
fn resolve_http(
    cfg: &HttpRequestConfig,
    vars: &HashMap<String, String>,
) -> anyhow::Result<HttpRequestConfig> {
    let mut c = cfg.clone();
    c.url = orbit_config::interpolate(&c.url, vars)?;
    for v in c.headers.values_mut() {
        *v = orbit_config::interpolate(v, vars)?;
    }
    if let Some(body) = &c.body {
        let text = serde_json::to_string(body)?;
        let resolved = orbit_config::interpolate(&text, vars)?;
        c.body = Some(
            serde_json::from_str(&resolved).unwrap_or_else(|_| serde_yaml::Value::String(resolved)),
        );
    }
    Ok(c)
}

/// Collect the HTTP steps of a plan into a list of EndpointSpec
fn collect_endpoints(plan: &orbit_config::TestPlan) -> Vec<EndpointSpec> {
    let mut eps = Vec::new();
    for s in &plan.scenarios {
        for step in &s.steps {
            if let Some(cfg) = step.as_http() {
                let name = step.name();
                let resolved = resolve_http(cfg, &plan.variables).unwrap_or_else(|_| cfg.clone());
                eps.push(EndpointSpec::new(
                    name.to_string(),
                    RequestSpec::Http(Box::new(resolved)),
                ));
            }
        }
    }
    eps
}

pub fn export_command(file: PathBuf, format: String) -> anyhow::Result<()> {
    let plan = read_plan(&file)?;
    let endpoints = collect_endpoints(&plan);
    if endpoints.is_empty() {
        anyhow::bail!("the test plan has no HTTP requests to export");
    }

    // Probe format capability: request-level formats (curl/wget/fetch...) export one by one; collection-level ones (postman/openapi/swagger) build an ApiSpec
    let collection_level = match exchange::export_request(&format, &endpoints[0]) {
        Ok(_) => false,
        Err(exchange::ExportError::Unsupported(_)) => true,
        Err(e) => anyhow::bail!("export failed: {}", e),
    };

    if collection_level {
        let mut spec = ApiSpec::new(plan.name.clone());
        spec.description = Some("Exported from Orbit CLI".to_string());
        spec.endpoints = endpoints;
        let doc = exchange::export(&format, &spec)
            .map_err(|e| anyhow::anyhow!("{} export failed: {}", format, e))?;
        println!("{}", doc);
    } else {
        for ep in &endpoints {
            let cmd = exchange::export_request(&format, ep)
                .map_err(|e| anyhow::anyhow!("{} export failed: {}", format, e))?;
            println!("{}", cmd);
        }
    }
    Ok(())
}
