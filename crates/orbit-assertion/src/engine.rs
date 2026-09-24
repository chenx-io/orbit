//! Assertion set engine

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult};

/// A set of assertions
pub struct AssertionSet {
    assertions: Vec<Box<dyn Assertion>>,
}

impl AssertionSet {
    pub fn new() -> Self {
        Self {
            assertions: Vec::new(),
        }
    }

    pub fn add(&mut self, assertion: Box<dyn Assertion>) {
        self.assertions.push(assertion);
    }

    pub fn is_empty(&self) -> bool {
        self.assertions.is_empty()
    }

    /// Evaluate all assertions
    pub async fn evaluate_all(&self, ctx: &AssertionContext) -> Vec<AssertionResult> {
        let mut results = Vec::new();
        for assertion in &self.assertions {
            results.push(assertion.evaluate(ctx).await);
        }
        results
    }

    /// Whether any hard assertion failed
    pub fn has_hard_failure(&self, results: &[AssertionResult]) -> bool {
        results.iter().any(|r| r.is_hard && !r.passed)
    }

    /// Compute the pass rate
    pub fn pass_rate(&self, results: &[AssertionResult]) -> f64 {
        if results.is_empty() {
            return 1.0;
        }
        let passed = results.iter().filter(|r| r.passed).count();
        passed as f64 / results.len() as f64
    }
}

impl Default for AssertionSet {
    fn default() -> Self {
        Self::new()
    }
}
