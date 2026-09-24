//! Status code assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult, Comparator};

/// Status code assertion: checks whether the response status code satisfies the comparison
pub struct StatusAssertion {
    pub expected: i32,
    pub comparator: Comparator,
}

#[async_trait]
impl Assertion for StatusAssertion {
    fn name(&self) -> &str {
        "status"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let passed = match self.comparator {
            Comparator::Equal => ctx.status_code == self.expected,
            Comparator::NotEqual => ctx.status_code != self.expected,
            _ => false,
        };
        AssertionResult {
            name: "status".into(),
            passed,
            message: if passed {
                format!("status is {}", ctx.status_code)
            } else {
                format!("expected status {}, got {}", self.expected, ctx.status_code)
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
    async fn test_status_assertion() {
        let ctx = make_ctx(200, "{}");
        let a = StatusAssertion {
            expected: 200,
            comparator: Comparator::Equal,
        };
        assert!(a.evaluate(&ctx).await.passed);
    }
}
