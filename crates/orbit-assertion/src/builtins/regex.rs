//! Regex assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult};

/// Regex assertion: checks whether the response body matches a regular expression
pub struct RegexAssertion {
    pub pattern: String,
}

#[async_trait]
impl Assertion for RegexAssertion {
    fn name(&self) -> &str {
        "regex"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let body_str = String::from_utf8_lossy(&ctx.body_bytes);
        let re = match regex::Regex::new(&self.pattern) {
            Ok(r) => r,
            Err(e) => {
                return AssertionResult {
                    name: "regex".into(),
                    passed: false,
                    message: format!("invalid regex: {}", e),
                    exported_vars: std::collections::HashMap::new(),
                    is_hard: true,
                }
            }
        };
        let passed = re.is_match(&body_str);
        AssertionResult {
            name: "regex".into(),
            passed,
            message: format!(
                "regex '{}': {}",
                self.pattern,
                if passed { "matched" } else { "no match" }
            ),
            exported_vars: std::collections::HashMap::new(),
            is_hard: true,
        }
    }
}
