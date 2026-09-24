//! Header assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult, Comparator};

/// Header assertion: checks whether a response header satisfies the comparison
pub struct HeaderAssertion {
    pub header_name: String,
    pub comparator: Comparator,
    pub expected: String,
}

#[async_trait]
impl Assertion for HeaderAssertion {
    fn name(&self) -> &str {
        "header"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        // HTTP header names are case-insensitive: match the stored name with ASCII case folding
        let actual = ctx
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(&self.header_name))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let passed = match &self.comparator {
            Comparator::Equal => actual == self.expected,
            Comparator::Contains => actual.contains(&self.expected),
            Comparator::Exists => !actual.is_empty(),
            _ => false,
        };
        AssertionResult {
            name: format!("header.{}", self.header_name),
            passed,
            message: format!(
                "header '{}': expected '{}', got '{}'",
                self.header_name, self.expected, actual
            ),
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
    async fn test_header_assertion() {
        let a = HeaderAssertion {
            header_name: "X-Request-Id".into(),
            comparator: Comparator::Equal,
            expected: "req-123".into(),
        };
        assert!(a.evaluate(&ctx("{}")).await.passed);
    }

    #[tokio::test]
    async fn test_header_assertion_case_insensitive() {
        // Stored header names are lowercase (hyper normalization), so uppercase in the assertion must still match
        let a = HeaderAssertion {
            header_name: "Content-Type".into(),
            comparator: Comparator::Contains,
            expected: "application/json".into(),
        };
        assert!(
            a.evaluate(&ctx("{}")).await.passed,
            "header name matching should ignore case"
        );
    }

    #[tokio::test]
    async fn test_header_assertion_missing() {
        let a = HeaderAssertion {
            header_name: "X-Nope".into(),
            comparator: Comparator::Exists,
            expected: String::new(),
        };
        assert!(!a.evaluate(&ctx("{}")).await.passed);
    }
}
