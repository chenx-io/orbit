//! JMESPath assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult, Comparator};

/// JMESPath assertion: evaluates a JMESPath expression on the JSON body and compares the result
pub struct JmesPathAssertion {
    pub expression: String,
    pub comparator: Comparator,
    pub expected: String,
}

#[async_trait]
impl Assertion for JmesPathAssertion {
    fn name(&self) -> &str {
        "jmespath"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let body_str = String::from_utf8_lossy(&ctx.body_bytes);
        let json: serde_json::Value = match serde_json::from_str(&body_str) {
            Ok(v) => v,
            Err(e) => {
                return AssertionResult {
                    name: "jmespath".into(),
                    passed: false,
                    message: format!("not valid JSON: {}", e),
                    exported_vars: std::collections::HashMap::new(),
                    is_hard: true,
                }
            }
        };

        let expr = match jmespath::compile(&self.expression) {
            Ok(e) => e,
            Err(e) => {
                return AssertionResult {
                    name: "jmespath".into(),
                    passed: false,
                    message: format!("invalid JMESPath expression: {}", e),
                    exported_vars: std::collections::HashMap::new(),
                    is_hard: true,
                }
            }
        };

        let result = match expr.search(json) {
            Ok(v) => v,
            Err(e) => {
                return AssertionResult {
                    name: "jmespath".into(),
                    passed: false,
                    message: format!("JMESPath search failed: {}", e),
                    exported_vars: std::collections::HashMap::new(),
                    is_hard: true,
                }
            }
        };

        // Rc<jmespath::Variable> → serde_json::Value → String
        let actual_str = match serde_json::to_value(result.as_ref()) {
            Ok(val) => jmes_value_to_string(&val),
            Err(e) => {
                return AssertionResult {
                    name: "jmespath".into(),
                    passed: false,
                    message: format!("result conversion failed: {}", e),
                    exported_vars: std::collections::HashMap::new(),
                    is_hard: true,
                }
            }
        };

        let passed = match &self.comparator {
            Comparator::Exists => !actual_str.is_empty() && actual_str != "null",
            Comparator::Equal => actual_str == self.expected,
            Comparator::NotEqual => actual_str != self.expected,
            Comparator::Contains => actual_str.contains(&self.expected),
            Comparator::GreaterThan => {
                if let (Ok(a), Ok(b)) = (actual_str.parse::<f64>(), self.expected.parse::<f64>()) {
                    a > b
                } else {
                    false
                }
            }
            Comparator::LessThan => {
                if let (Ok(a), Ok(b)) = (actual_str.parse::<f64>(), self.expected.parse::<f64>()) {
                    a < b
                } else {
                    false
                }
            }
            _ => false,
        };

        AssertionResult {
            name: "jmespath".into(),
            passed,
            message: format!(
                "jmespath '{}': expected '{}', got '{}' — {}",
                self.expression,
                self.expected,
                actual_str,
                if passed { "PASS" } else { "FAIL" }
            ),
            exported_vars: std::collections::HashMap::new(),
            is_hard: true,
        }
    }
}

/// Convert a jmespath result to a string
fn jmes_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(jmes_value_to_string).collect();
            format!("[{}]", items.join(", "))
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::test_util::make_ctx;

    #[tokio::test]
    async fn test_jmespath_equal() {
        let ctx = make_ctx(200, r#"{"data":{"name":"orbit","version":"0.1.0"}}"#);
        let a = JmesPathAssertion {
            expression: "data.name".into(),
            comparator: Comparator::Equal,
            expected: "orbit".into(),
        };
        assert!(a.evaluate(&ctx).await.passed);
    }

    #[tokio::test]
    async fn test_jmespath_exists() {
        let ctx = make_ctx(200, r#"{"token":"abc123"}"#);
        let a = JmesPathAssertion {
            expression: "token".into(),
            comparator: Comparator::Exists,
            expected: String::new(),
        };
        assert!(a.evaluate(&ctx).await.passed);
    }

    #[tokio::test]
    async fn test_jmespath_filter() {
        let ctx = make_ctx(
            200,
            r#"{"users":[{"name":"Alice","age":30},{"name":"Bob","age":20}]}"#,
        );
        let a = JmesPathAssertion {
            expression: "users[?age > `25`].name".into(),
            comparator: Comparator::Contains,
            expected: "Alice".into(),
        };
        let result = a.evaluate(&ctx).await;
        assert!(result.passed, "{}", result.message);
    }

    #[tokio::test]
    async fn test_jmespath_length() {
        let ctx = make_ctx(200, r#"{"items":[1,2,3,4,5]}"#);
        let a = JmesPathAssertion {
            expression: "length(items)".into(),
            comparator: Comparator::Equal,
            expected: "5".into(),
        };
        let result = a.evaluate(&ctx).await;
        assert!(result.passed, "{}", result.message);
    }

    #[tokio::test]
    async fn test_jmespath_not_equal() {
        let ctx = make_ctx(200, r#"{"status":"error"}"#);
        let a = JmesPathAssertion {
            expression: "status".into(),
            comparator: Comparator::NotEqual,
            expected: "ok".into(),
        };
        assert!(a.evaluate(&ctx).await.passed);
    }
}
