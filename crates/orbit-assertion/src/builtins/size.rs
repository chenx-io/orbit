//! Response size assertion

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult};

/// Response size assertion: checks that the response body size does not exceed the threshold
pub struct SizeAssertion {
    pub max_bytes: usize,
}

#[async_trait]
impl Assertion for SizeAssertion {
    fn name(&self) -> &str {
        "size"
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let passed = ctx.body_size <= self.max_bytes;
        AssertionResult {
            name: "size".into(),
            passed,
            message: format!("body size {} bytes (max {})", ctx.body_size, self.max_bytes),
            exported_vars: std::collections::HashMap::new(),
            is_hard: true,
        }
    }
}
