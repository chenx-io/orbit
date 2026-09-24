//! Body-contains assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult};

/// Body-contains assertion: checks that the response body contains the given substring
pub struct BodyContainsAssertion {
    pub needle: String,
}

#[async_trait]
impl Assertion for BodyContainsAssertion {
    fn name(&self) -> &str {
        "body_contains"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let body_str = String::from_utf8_lossy(&ctx.body_bytes);
        let passed = body_str.contains(&self.needle);
        AssertionResult {
            name: "body_contains".into(),
            passed,
            message: if passed {
                format!("body contains '{}'", self.needle)
            } else {
                format!("body does not contain '{}'", self.needle)
            },
            exported_vars: std::collections::HashMap::new(),
            is_hard: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::test_util::make_ctx;

    #[tokio::test]
    async fn test_body_contains() {
        let ctx = make_ctx(200, r#"{"status":"ok"}"#);
        let a = BodyContainsAssertion {
            needle: "ok".into(),
        };
        assert!(a.evaluate(&ctx).await.passed);
    }
}
