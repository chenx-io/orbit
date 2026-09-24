//! Parse layer - YAML entry point / duration parsing / environment merging

use crate::error::ConfigError;
use crate::model::plan::TestPlan;

/// Parse a TestPlan from a YAML string
pub fn from_str(yaml: &str) -> Result<TestPlan, ConfigError> {
    let plan: TestPlan = serde_yaml::from_str(yaml).map_err(|e| {
        ConfigError::Parse(format!(
            "YAML parse error at line {}: {}",
            e.location()
                .map(|l| l.line().to_string())
                .unwrap_or_default(),
            e
        ))
    })?;
    Ok(plan)
}

/// Parse a duration string into seconds (supports "30s", "5m", "1h")
pub fn parse_duration(s: &str) -> Result<f64, ConfigError> {
    let s = s.trim();
    if let Some(s) = s.strip_suffix("ms") {
        s.parse::<f64>()
            .map(|v| v / 1000.0)
            .map_err(|_| ConfigError::InvalidDuration(s.to_string()))
    } else if let Some(s) = s.strip_suffix('s') {
        s.parse::<f64>()
            .map_err(|_| ConfigError::InvalidDuration(s.to_string()))
    } else if let Some(s) = s.strip_suffix('m') {
        s.parse::<f64>()
            .map(|v| v * 60.0)
            .map_err(|_| ConfigError::InvalidDuration(s.to_string()))
    } else if let Some(s) = s.strip_suffix('h') {
        s.parse::<f64>()
            .map(|v| v * 3600.0)
            .map_err(|_| ConfigError::InvalidDuration(s.to_string()))
    } else {
        s.parse::<f64>()
            .map_err(|_| ConfigError::InvalidDuration(s.to_string()))
    }
}

/// Merge environment variables into a TestPlan (environment variables override plan defaults)
pub fn apply_environment(plan: &mut TestPlan, env_name: &str) -> Result<(), ConfigError> {
    let env = plan
        .environments
        .iter()
        .find(|e| e.name == env_name)
        .ok_or_else(|| ConfigError::Validation(format!("Environment '{}' not found", env_name)))?;

    for (k, v) in &env.variables {
        // Environment variable semantics: environment config overrides plan-level defaults (consistent with k6 behavior)
        plan.variables.insert(k.clone(), v.clone());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30s").unwrap(), 30.0);
        assert_eq!(parse_duration("5m").unwrap(), 300.0);
        assert_eq!(parse_duration("1h").unwrap(), 3600.0);
        assert_eq!(parse_duration("500ms").unwrap(), 0.5);
    }

    #[test]
    fn test_parse_yaml() {
        let yaml = r#"
name: "login test"
scenarios:
  - name: "login flow"
    executor:
      type: constant-vus
      vus: 10
      duration: "30s"
    steps:
      - type: request
        name: "login"
        request:
          method: POST
          url: "https://api.example.com/login"
          headers:
            Content-Type: "application/json"
          body:
            username: test
            password: test
        checks:
          - type: status
            value: 200
        extract:
          - name: token
            from: jsonpath
            path: "$.data.token"
"#;
        let plan = from_str(yaml).unwrap();
        assert_eq!(plan.name, "login test");
        assert_eq!(plan.scenarios.len(), 1);
        assert_eq!(plan.scenarios[0].steps.len(), 1);
        if let crate::Step::Request {
            checks, extract, ..
        } = &plan.scenarios[0].steps[0]
        {
            assert_eq!(checks.len(), 1);
            assert_eq!(extract.len(), 1);
        } else {
            panic!("Expected Request step");
        }
    }

    #[test]
    fn test_environment_merge() {
        let yaml = r#"
name: "test"
environments:
  - name: staging
    variables:
      base_url: "https://staging.example.com"
      api_key: "staging-key"
variables:
  api_key: "default-key"
scenarios:
  - name: "test"
    executor:
      type: constant-vus
      vus: 1
      duration: "1s"
    steps:
      - type: request
        request:
          method: GET
          url: "${base_url}/health"
"#;
        let mut plan = from_str(yaml).unwrap();
        apply_environment(&mut plan, "staging").unwrap();
        assert_eq!(
            plan.variables.get("base_url").unwrap(),
            "https://staging.example.com"
        );
        // Environment variables override plan defaults
        assert_eq!(plan.variables.get("api_key").unwrap(), "staging-key");
    }

    #[test]
    fn test_sequential_scenario_wait_request() {
        let yaml = r#"
name: "test"
scenarios:
  - name: "test"
    executor:
      type: sequential
      iterations: 1
    on_error: stop
    steps:
        - type: wait
          name: "wait"
          duration: "0.5s"
        - type: request
          name: "request"
          request:
            method: GET
            url: "https://example.com/api"
"#;
        let plan = crate::from_str(yaml).unwrap();
        assert_eq!(plan.scenarios.len(), 1);
        assert_eq!(plan.scenarios[0].steps.len(), 2);
        assert!(matches!(
            plan.scenarios[0].steps[0],
            crate::Step::Wait { .. }
        ));
        assert!(plan.scenarios[0].steps[1].is_request());
    }

    #[test]
    fn test_scenario_with_template_variable_in_url() {
        // Realistic YAML as sent by the frontend: the url contains the {{base_url}} template variable
        let yaml = r#"
name: "test"
scenarios:
  - name: "test"
    executor:
      type: sequential
      iterations: 1
    on_error: stop
    steps:
        - type: wait
          name: "wait"
          duration: "0.5s"
        - type: request
          name: "health check"
          request:
            method: GET
            url: "{{base_url}}/health"
"#;
        let plan = crate::from_str(yaml).unwrap();
        assert_eq!(plan.scenarios[0].steps.len(), 2);
    }

    #[test]
    fn test_request_step_with_headers() {
        // Verify header indentation: a header key must be indented 2 spaces deeper than headers:
        let yaml = r#"
name: "test"
scenarios:
  - name: "test"
    executor:
      type: sequential
      iterations: 1
    on_error: stop
    steps:
        - type: request
          name: "request"
          request:
            method: GET
            url: "https://example.com"
            headers:
              Accept: "application/json"
"#;
        let plan = crate::from_str(yaml).unwrap();
        assert_eq!(plan.scenarios[0].steps.len(), 1);
        if let crate::Step::Request { request, .. } = &plan.scenarios[0].steps[0] {
            if let crate::RequestSpec::Http(h) = request {
                assert_eq!(h.headers.get("Accept").unwrap(), "application/json");
            } else {
                panic!("expected Http request spec");
            }
        }
    }
}
