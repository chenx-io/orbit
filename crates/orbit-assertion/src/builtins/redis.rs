//! Redis assertion: queries the cache after the request and checks the command result (polls for cache updates).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult, Comparator, RetryPolicy};

use super::compare::{compare_value, make_result};

/// Redis assertion
///
/// `args` (command name + arguments) and `expected` must be interpolated by the caller before assembly.
pub struct RedisAssertion {
    pub name: String,
    pub datasource: String,
    /// `args[0]` is the command name (e.g. GET), the rest are command arguments
    pub args: Vec<String>,
    pub comparator: Comparator,
    pub expected: String,
    pub retry: Option<RetryPolicy>,
    pub hard: bool,
    pub extract_var: Option<String>,
}

#[async_trait]
impl Assertion for RedisAssertion {
    fn name(&self) -> &str {
        "redis"
    }

    fn is_hard(&self) -> bool {
        self.hard
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let Some(provider) = ctx.datasources.clone() else {
            return make_result(
                &self.name,
                false,
                "no data source capability injected (datasources is empty), Redis assertion cannot run".into(),
                true,
            );
        };

        let (attempts, interval, deadline) = match &self.retry {
            Some(r) => (
                r.max_attempts.max(1),
                Duration::from_millis(r.interval_ms),
                r.timeout_ms.map(Duration::from_millis),
            ),
            None => (1, Duration::ZERO, None),
        };
        let started = Instant::now();
        let mut last_error: Option<String> = None;
        let mut last_actual: Option<String> = None;

        for attempt in 0..attempts {
            if attempt > 0 {
                if let Some(dl) = deadline {
                    if Instant::now().duration_since(started) >= dl {
                        break;
                    }
                }
            }
            match provider.redis_command(&self.datasource, &self.args).await {
                Ok(actual) => {
                    last_actual = Some(actual.clone());
                    let passed = compare_value(&self.comparator, Some(&actual), &self.expected);
                    if passed {
                        let elapsed = started.elapsed().as_millis();
                        let mut exported = HashMap::new();
                        if let Some(k) = &self.extract_var {
                            exported.insert(k.clone(), actual.clone());
                        }
                        return AssertionResult {
                            name: self.name.clone(),
                            passed: true,
                            message: format!(
                                "{} assertion passed: actual value {} ({} attempts / {}ms)",
                                self.name,
                                actual,
                                attempt + 1,
                                elapsed
                            ),
                            is_hard: self.hard,
                            exported_vars: exported,
                        };
                    }
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
            if attempt + 1 < attempts {
                tokio::time::sleep(interval).await;
            }
        }

        let elapsed = started.elapsed().as_millis();
        let reason = match last_error {
            Some(err) => format!("command execution failed: {err}"),
            None => match &last_actual {
                Some(a) => format!(
                    "actual value \"{a}\" does not satisfy the expectation (comparator mismatch, expected \"{}\")",
                    self.expected
                ),
                None => "command returned no result".to_string(),
            },
        };
        make_result(
            &self.name,
            false,
            format!(
                "{} assertion failed ({} attempts / {}ms): {reason}",
                self.name, attempts, elapsed
            ),
            true,
        )
    }
}
