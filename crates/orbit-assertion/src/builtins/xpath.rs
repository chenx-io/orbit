//! XPath assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult, Comparator};

/// XPath assertion: validates the XML body (MVP implemented as string matching)
pub struct XPathAssertion {
    pub path: String,
    pub comparator: Comparator,
    pub expected: String,
}

#[async_trait]
impl Assertion for XPathAssertion {
    fn name(&self) -> &str {
        "xpath"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let body_str = String::from_utf8_lossy(&ctx.body_bytes);
        // MVP XPath: simple string search fallback for non-XML content
        let passed = match &self.comparator {
            Comparator::Exists => body_str.contains(&self.path),
            Comparator::Contains => body_str.contains(&self.expected),
            Comparator::Equal => body_str == self.expected,
            _ => false,
        };

        AssertionResult {
            name: "xpath".into(),
            passed,
            message: format!(
                "xpath '{}': {}",
                self.path,
                if passed { "ok" } else { "fail" }
            ),
            exported_vars: std::collections::HashMap::new(),
            is_hard: true,
        }
    }
}
