//! JSON Schema assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult};

/// JSON Schema assertion: checks whether the response body conforms to a JSON Schema
pub struct JsonSchemaAssertion {
    pub schema_json: String,
}

#[async_trait]
impl Assertion for JsonSchemaAssertion {
    fn name(&self) -> &str {
        "jsonschema"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let body_str = String::from_utf8_lossy(&ctx.body_bytes);
        let instance: serde_json::Value = match serde_json::from_str(&body_str) {
            Ok(v) => v,
            Err(e) => {
                return AssertionResult {
                    name: "jsonschema".into(),
                    passed: false,
                    message: format!("Invalid JSON: {}", e),
                    exported_vars: std::collections::HashMap::new(),
                    is_hard: true,
                }
            }
        };
        let schema: serde_json::Value = match serde_json::from_str(&self.schema_json) {
            Ok(v) => v,
            Err(e) => {
                return AssertionResult {
                    name: "jsonschema".into(),
                    passed: false,
                    message: format!("Invalid schema: {}", e),
                    exported_vars: std::collections::HashMap::new(),
                    is_hard: true,
                }
            }
        };
        let compiled = match jsonschema::JSONSchema::compile(&schema) {
            Ok(v) => v,
            Err(e) => {
                return AssertionResult {
                    name: "jsonschema".into(),
                    passed: false,
                    message: format!("Schema compile error: {}", e),
                    exported_vars: std::collections::HashMap::new(),
                    is_hard: true,
                }
            }
        };
        let passed = compiled.is_valid(&instance);
        AssertionResult {
            name: "jsonschema".into(),
            passed,
            message: if passed {
                "JSON Schema validation passed".into()
            } else {
                "JSON Schema validation failed".into()
            },
            exported_vars: std::collections::HashMap::new(),
            is_hard: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::test_util::ctx;

    #[tokio::test]
    async fn test_json_schema_valid() {
        let a = JsonSchemaAssertion {
            schema_json:
                r#"{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}"#
                    .into(),
        };
        assert!(a.evaluate(&ctx(r#"{"name":"orbit"}"#)).await.passed);
    }

    #[tokio::test]
    async fn test_json_schema_invalid() {
        let a = JsonSchemaAssertion {
            schema_json:
                r#"{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}"#
                    .into(),
        };
        assert!(!a.evaluate(&ctx(r#"{"version":"0.1.0"}"#)).await.passed);
    }
}
