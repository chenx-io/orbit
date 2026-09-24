//! Response time assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult};

/// Response time assertion: checks that the response duration does not exceed the threshold
pub struct DurationAssertion {
    pub max_ms: u64,
}

#[async_trait]
impl Assertion for DurationAssertion {
    fn name(&self) -> &str {
        "duration"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let passed = ctx.duration_ms <= self.max_ms;
        AssertionResult {
            name: "duration".into(),
            passed,
            message: format!("duration {}ms (max {}ms)", ctx.duration_ms, self.max_ms),
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
    async fn test_duration_assertion() {
        let ctx = make_ctx(200, "{}");
        let a = DurationAssertion { max_ms: 500 };
        assert!(a.evaluate(&ctx).await.passed);
    }
}
