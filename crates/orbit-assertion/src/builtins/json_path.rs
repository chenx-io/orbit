//! JSONPath assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult, Comparator};

/// JSONPath assertion: reads a value from the JSON body by path and compares it
pub struct JsonPathAssertion {
    pub path: String,
    pub comparator: Comparator,
    pub expected: String,
}

#[async_trait]
impl Assertion for JsonPathAssertion {
    fn name(&self) -> &str {
        "jsonpath"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let body_str = String::from_utf8_lossy(&ctx.body_bytes);
        let value: serde_json::Value = match serde_json::from_str(&body_str) {
            Ok(v) => v,
            Err(e) => {
                return AssertionResult {
                    name: "jsonpath".into(),
                    passed: false,
                    message: format!("not valid JSON: {}", e),
                    exported_vars: std::collections::HashMap::new(),
                    is_hard: true,
                }
            }
        };

        // Use a simple dot-path lookup (MVP implementation)
        let found = find_json_path(&value, &self.path);
        let passed = match &self.comparator {
            Comparator::Exists => found.is_some(),
            Comparator::Equal => found.map(|v| v == self.expected).unwrap_or(false),
            Comparator::Contains => found.map(|v| v.contains(&self.expected)).unwrap_or(false),
            _ => false,
        };

        AssertionResult {
            name: "jsonpath".into(),
            passed,
            message: format!(
                "jsonpath '{}': {}",
                self.path,
                if passed { "ok" } else { "fail" }
            ),
            exported_vars: std::collections::HashMap::new(),
            is_hard: true,
        }
    }
}

/// Simple JSON path lookup (supports the a.b.c and a[0].b forms)
fn find_json_path(value: &serde_json::Value, path: &str) -> Option<String> {
    let mut current = value;
    for segment in path
        .trim_start_matches("$.")
        .trim_start_matches('$')
        .split('.')
    {
        if segment.is_empty() {
            continue;
        }

        // Handle array index: name[0]
        if let Some(idx_end) = segment.find('[') {
            let field = &segment[..idx_end];
            let idx_str = &segment[idx_end + 1..segment.len() - 1];
            if !field.is_empty() {
                current = current.get(field)?;
            }
            if let Ok(idx) = idx_str.parse::<usize>() {
                current = current.get(idx)?;
            }
        } else {
            current = current.get(segment)?;
        }
    }
    match current {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Null => Some("null".into()),
        _ => Some(current.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::test_util::make_ctx;

    #[tokio::test]
    async fn test_jsonpath() {
        let ctx = make_ctx(200, r#"{"data":{"name":"orbit","version":"0.1.0"}}"#);
        let a = JsonPathAssertion {
            path: "data.name".into(),
            comparator: Comparator::Equal,
            expected: "orbit".into(),
        };
        assert!(a.evaluate(&ctx).await.passed);
    }

    #[tokio::test]
    async fn test_jsonpath_exists() {
        let ctx = make_ctx(200, r#"{"token":"abc123"}"#);
        let a = JsonPathAssertion {
            path: "token".into(),
            comparator: Comparator::Exists,
            expected: String::new(),
        };
        assert!(a.evaluate(&ctx).await.passed);
    }

    /// An explicit assertion failure must be hard (counted as an error), otherwise it cannot gate acceptance
    #[tokio::test]
    async fn test_jsonpath_failure_is_hard() {
        let ctx = make_ctx(200, r#"{"hello":"world"}"#);
        let a = JsonPathAssertion {
            path: "hello".into(),
            comparator: Comparator::Equal,
            expected: "WRONG".into(),
        };
        let r = a.evaluate(&ctx).await;
        assert!(!r.passed);
        assert!(r.is_hard, "assertion failure should be hard");
    }

    /// A jsonpath assertion on a non-JSON response should fail and be hard
    #[tokio::test]
    async fn test_jsonpath_on_non_json_is_hard() {
        let ctx = make_ctx(200, "hello plain text");
        let a = JsonPathAssertion {
            path: "hello".into(),
            comparator: Comparator::Exists,
            expected: String::new(),
        };
        let r = a.evaluate(&ctx).await;
        assert!(!r.passed);
        assert!(r.is_hard);
    }
}
