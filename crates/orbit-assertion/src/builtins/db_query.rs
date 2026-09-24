//! DB query assertion: queries the database after the request and checks the result (polls for async writes).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::query::extract_target;
use crate::traits::Assertion;
use crate::types::{AssertionContext, AssertionResult, Comparator, DbTarget, RetryPolicy};

use super::compare::{compare_value, make_result};

/// DB query assertion
///
/// `sql` and `expected` must be interpolated by the caller before assembly; without `retry` it queries once.
pub struct DbQueryAssertion {
    pub name: String,
    pub datasource: String,
    pub sql: String,
    pub target: DbTarget,
    pub comparator: Comparator,
    pub expected: String,
    pub retry: Option<RetryPolicy>,
    pub hard: bool,
    pub extract_var: Option<String>,
}

#[async_trait]
impl Assertion for DbQueryAssertion {
    fn name(&self) -> &str {
        "db"
    }

    fn is_hard(&self) -> bool {
        self.hard
    }

    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult {
        let Some(provider) = ctx.datasources.clone() else {
            return make_result(
                &self.name,
                false,
                "no data source capability injected (datasources is empty), DB assertion cannot run".into(),
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
            // Check the total timeout before each attempt (the first attempt never fails on timeout)
            if attempt > 0 {
                if let Some(dl) = deadline {
                    if Instant::now().duration_since(started) >= dl {
                        break;
                    }
                }
            }
            match provider.query_sql(&self.datasource, &self.sql).await {
                Ok(result) => {
                    let actual = extract_target(&self.target, &result);
                    last_actual = actual.clone();
                    let passed = compare_value(&self.comparator, actual.as_deref(), &self.expected);
                    if passed {
                        let elapsed = started.elapsed().as_millis();
                        let mut exported = HashMap::new();
                        if let Some(k) = &self.extract_var {
                            if let Some(v) = &actual {
                                exported.insert(k.clone(), v.clone());
                            }
                        }
                        return AssertionResult {
                            name: self.name.clone(),
                            passed: true,
                            message: format!(
                                "{} assertion passed: actual value {} ({} attempts / {}ms)",
                                self.name,
                                actual.unwrap_or_default(),
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
            Some(err) => format!("query failed: {err}"),
            None => match &last_actual {
                Some(a) => format!(
                    "actual value \"{a}\" does not satisfy the expectation (comparator mismatch, expected \"{}\")",
                    self.expected
                ),
                None => "query returned no comparable target result (too few rows/columns)".to_string(),
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;

    use crate::builtins::test_util::make_ctx;
    use crate::traits::DataSourceProvider;
    use crate::types::{AssertionContext, DbTarget, QueryResult, RetryPolicy};

    use super::*;

    /// In-memory fake: `fail_before` makes the first N calls return an empty result (simulating async write delay).
    #[derive(Debug)]
    struct MockProvider {
        call: AtomicUsize,
        fail_before: usize,
    }

    #[async_trait]
    impl DataSourceProvider for MockProvider {
        async fn query_sql(&self, _ds: &str, _sql: &str) -> Result<QueryResult, String> {
            let n = self.call.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_before {
                Ok(QueryResult {
                    columns: vec!["status".into()],
                    rows: vec![],
                    ..Default::default()
                })
            } else {
                Ok(QueryResult {
                    columns: vec!["status".into()],
                    rows: vec![vec!["PAID".into()]],
                    ..Default::default()
                })
            }
        }
        async fn redis_command(&self, _ds: &str, _args: &[String]) -> Result<String, String> {
            Ok("1".into())
        }
    }

    fn ctx_with(provider: MockProvider) -> AssertionContext {
        let mut ctx = make_ctx(200, "{}");
        ctx.datasources = Some(Arc::new(provider));
        ctx
    }

    fn assertion(target: DbTarget, expected: &str, retry: Option<RetryPolicy>) -> DbQueryAssertion {
        DbQueryAssertion {
            name: "order status".into(),
            datasource: "test".into(),
            sql: "SELECT status FROM orders".into(),
            target,
            comparator: Comparator::Equal,
            expected: expected.into(),
            retry,
            hard: true,
            extract_var: None,
        }
    }

    #[tokio::test]
    async fn scalar_equal_passes() {
        let a = assertion(DbTarget::Scalar, "PAID", None);
        assert!(
            a.evaluate(&ctx_with(MockProvider {
                call: AtomicUsize::new(0),
                fail_before: 0
            }))
            .await
            .passed
        );
    }

    #[tokio::test]
    async fn missing_row_is_fail_hard() {
        let a = assertion(DbTarget::Scalar, "PAID", None);
        let r = a
            .evaluate(&ctx_with(MockProvider {
                call: AtomicUsize::new(0),
                fail_before: 99,
            }))
            .await;
        assert!(!r.passed);
        assert!(r.is_hard);
    }

    #[tokio::test]
    async fn retry_eventually_passes() {
        let a = assertion(
            DbTarget::Scalar,
            "PAID",
            Some(RetryPolicy {
                interval_ms: 1,
                max_attempts: 5,
                timeout_ms: None,
            }),
        );
        let r = a
            .evaluate(&ctx_with(MockProvider {
                call: AtomicUsize::new(0),
                fail_before: 2,
            }))
            .await;
        assert!(r.passed, "expected to pass after retry: {}", r.message);
    }

    #[tokio::test]
    async fn retry_exhausted_fails() {
        let a = assertion(
            DbTarget::Scalar,
            "PAID",
            Some(RetryPolicy {
                interval_ms: 1,
                max_attempts: 3,
                timeout_ms: None,
            }),
        );
        let r = a
            .evaluate(&ctx_with(MockProvider {
                call: AtomicUsize::new(0),
                fail_before: 99,
            }))
            .await;
        assert!(!r.passed);
        assert!(r.message.contains("3 attempts"), "{}", r.message);
    }

    #[tokio::test]
    async fn no_provider_is_hard_fail() {
        let a = assertion(DbTarget::Scalar, "PAID", None);
        let r = a.evaluate(&make_ctx(200, "{}")).await;
        assert!(!r.passed);
        assert!(r.is_hard);
    }

    #[test]
    fn extract_json_path_on_row() {
        let result = QueryResult {
            columns: vec!["id".into(), "meta".into()],
            rows: vec![vec!["42".into(), r#"{"status":"PAID","items":3}"#.into()]],
            ..Default::default()
        };
        assert_eq!(
            extract_target(
                &DbTarget::JsonPath {
                    row: 0,
                    path: "meta.status".into()
                },
                &result
            ),
            Some("PAID".into())
        );
        assert_eq!(
            extract_target(&DbTarget::RowCount, &result),
            Some("1".into())
        );
        assert_eq!(
            extract_target(
                &DbTarget::Cell {
                    row: 0,
                    column: "id".into()
                },
                &result
            ),
            Some("42".into())
        );
    }
}
