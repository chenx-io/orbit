//! Thresholds - test pass/fail conditions
//!
//! Threshold conditions can be configured in a test plan and are evaluated automatically once the test ends.
//!
//! # Syntax
//!
//! ```yaml
//! thresholds:
//!   - "http_req_duration: p(95) < 500"
//!   - "http_req_failed: rate < 0.01"
//!   - "http_reqs: count > 100"
//!   - "data_sent: value < 1048576"
//!   - "http_req_duration: avg < 200"
//!   - "http_req_duration: max < 2000"
//!   - "http_req_duration: min > 10"
//! ```
//!
//! # Supported metrics
//!
//! | Metric | Description | Supported aggregations |
//! |--------|------|-----------|
//! | `http_req_duration` | request duration (ms) | p(N), avg, min, max, med |
//! | `http_req_failed` | failure rate | rate, count |
//! | `http_reqs` | total requests | count |
//! | `data_sent` | bytes sent | value |
//! | `data_received` | bytes received | value |

use orbit_metrics::MetricsSummary;

/// Threshold error
#[derive(Debug, thiserror::Error)]
pub enum ThresholdError {
    #[error("failed to parse threshold expression: {0}")]
    Parse(String),
    #[error("unsupported metric: {0}")]
    UnsupportedMetric(String),
    #[error("unsupported aggregation: {0}")]
    UnsupportedAggregation(String),
}

/// Aggregation type
#[derive(Debug, Clone, PartialEq)]
pub enum Aggregation {
    /// Percentile, e.g. p(95), p(99)
    Percentile(f64),
    /// Average
    Avg,
    /// Minimum
    Min,
    /// Maximum
    Max,
    /// Median, p(50)
    Med,
    /// Count
    Count,
    /// Ratio (error rate)
    Rate,
    /// Raw value
    Value,
}

/// Comparison operator
#[derive(Debug, Clone, PartialEq)]
pub enum ThresholdOp {
    LessThan,
    GreaterThan,
    LessOrEqual,
    GreaterOrEqual,
}

impl std::fmt::Display for ThresholdOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThresholdOp::LessThan => write!(f, "<"),
            ThresholdOp::GreaterThan => write!(f, ">"),
            ThresholdOp::LessOrEqual => write!(f, "<="),
            ThresholdOp::GreaterOrEqual => write!(f, ">="),
        }
    }
}

/// Metric name
#[derive(Debug, Clone, PartialEq)]
pub enum MetricName {
    HttpReqDuration,
    HttpReqFailed,
    HttpReqs,
    DataSent,
    DataReceived,
}

impl MetricName {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "http_req_duration" => Some(Self::HttpReqDuration),
            "http_req_failed" => Some(Self::HttpReqFailed),
            "http_reqs" => Some(Self::HttpReqs),
            "data_sent" => Some(Self::DataSent),
            "data_received" => Some(Self::DataReceived),
            _ => None,
        }
    }

    /// Extract the corresponding value from MetricsSummary
    pub fn evaluate(&self, agg: &Aggregation, summary: &MetricsSummary) -> f64 {
        match self {
            MetricName::HttpReqDuration => match agg {
                Aggregation::Percentile(p) => {
                    if (*p - 50.0).abs() < 0.01 {
                        summary.p50_ms
                    } else if (*p - 90.0).abs() < 0.01 {
                        summary.p90_ms
                    } else if (*p - 95.0).abs() < 0.01 {
                        summary.p95_ms
                    } else if (*p - 99.0).abs() < 0.01 {
                        summary.p99_ms
                    } else if (*p - 99.9).abs() < 0.01 {
                        summary.p999_ms
                    } else {
                        summary.p50_ms
                    } // fallback
                }
                Aggregation::Avg => summary.mean_ms,
                Aggregation::Min => summary.min_ms,
                Aggregation::Max => summary.max_ms,
                Aggregation::Med => summary.p50_ms,
                _ => 0.0,
            },
            MetricName::HttpReqFailed => match agg {
                Aggregation::Rate => summary.error_rate,
                Aggregation::Count => summary.total_errors as f64,
                _ => 0.0,
            },
            MetricName::HttpReqs => match agg {
                Aggregation::Count | Aggregation::Value => summary.total_requests as f64,
                _ => 0.0,
            },
            MetricName::DataSent | MetricName::DataReceived => match agg {
                Aggregation::Value | Aggregation::Count => summary.total_bytes as f64,
                _ => 0.0,
            },
        }
    }
}

/// A single threshold condition
#[derive(Debug, Clone)]
pub struct ThresholdCondition {
    pub metric: MetricName,
    pub aggregation: Aggregation,
    pub op: ThresholdOp,
    pub value: f64,
    /// Whether this is a hard threshold (a failure aborts the test)
    pub abort_on_fail: bool,
}

impl ThresholdCondition {
    /// Evaluate this single threshold
    pub fn evaluate(&self, summary: &MetricsSummary) -> ThresholdResult {
        let actual = self.metric.evaluate(&self.aggregation, summary);
        let passed = match self.op {
            ThresholdOp::LessThan => actual < self.value,
            ThresholdOp::GreaterThan => actual > self.value,
            ThresholdOp::LessOrEqual => actual <= self.value,
            ThresholdOp::GreaterOrEqual => actual >= self.value,
        };

        ThresholdResult {
            condition: self.clone(),
            actual,
            passed,
        }
    }
}

impl std::fmt::Display for ThresholdCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let metric_str = match &self.metric {
            MetricName::HttpReqDuration => "http_req_duration",
            MetricName::HttpReqFailed => "http_req_failed",
            MetricName::HttpReqs => "http_reqs",
            MetricName::DataSent => "data_sent",
            MetricName::DataReceived => "data_received",
        };
        let agg_str = match &self.aggregation {
            Aggregation::Percentile(p) => format!("p({})", p),
            Aggregation::Avg => "avg".to_string(),
            Aggregation::Min => "min".to_string(),
            Aggregation::Max => "max".to_string(),
            Aggregation::Med => "med".to_string(),
            Aggregation::Count => "count".to_string(),
            Aggregation::Rate => "rate".to_string(),
            Aggregation::Value => "value".to_string(),
        };
        write!(f, "{}: {} {} {}", metric_str, agg_str, self.op, self.value)
    }
}

/// Threshold evaluation result
#[derive(Debug, Clone)]
pub struct ThresholdResult {
    pub condition: ThresholdCondition,
    pub actual: f64,
    pub passed: bool,
}

impl std::fmt::Display for ThresholdResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} — expected {} {}, actual {} — {}",
            self.condition,
            self.condition.op,
            self.condition.value,
            self.actual,
            if self.passed { "✓ PASS" } else { "✗ FAIL" }
        )
    }
}

/// Threshold set
#[derive(Debug, Clone, Default)]
pub struct ThresholdSet {
    conditions: Vec<ThresholdCondition>,
}

impl ThresholdSet {
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
    }

    pub fn add(&mut self, condition: ThresholdCondition) {
        self.conditions.push(condition);
    }

    /// Parse a threshold expression string
    ///
    /// Format: `<metric>: <agg> <op> <value>[; abort]`
    ///
    /// # Examples
    /// - `"http_req_duration: p(95) < 500"`
    /// - `"http_req_failed: rate < 0.01; abort"`
    pub fn parse_and_add(&mut self, expr: &str) -> Result<(), ThresholdError> {
        let condition = parse_threshold_expression(expr)?;
        self.conditions.push(condition);
        Ok(())
    }

    /// Evaluate all thresholds
    pub fn evaluate_all(&self, summary: &MetricsSummary) -> Vec<ThresholdResult> {
        self.conditions
            .iter()
            .map(|c| c.evaluate(summary))
            .collect()
    }

    /// Whether all thresholds passed
    pub fn all_passed(&self, results: &[ThresholdResult]) -> bool {
        results.iter().all(|r| r.passed)
    }

    /// Get the failed thresholds
    pub fn failures<'a>(&self, results: &'a [ThresholdResult]) -> Vec<&'a ThresholdResult> {
        results.iter().filter(|r| !r.passed).collect()
    }

    /// Whether any threshold has abort_on_fail and failed
    pub fn has_abort_failure(&self, results: &[ThresholdResult]) -> bool {
        results
            .iter()
            .any(|r| !r.passed && r.condition.abort_on_fail)
    }
}

/// Parse a single threshold expression
fn parse_threshold_expression(expr: &str) -> Result<ThresholdCondition, ThresholdError> {
    let expr = expr.trim();

    // Check the abort marker
    let (core_expr, abort_on_fail) = if let Some(stripped) = expr.strip_suffix("; abort") {
        (stripped.trim(), true)
    } else if let Some(stripped) = expr.strip_suffix(";abort") {
        (stripped.trim(), true)
    } else {
        (expr, false)
    };

    // Split the metric and the rest on ":"
    let colon_pos = core_expr
        .find(':')
        .ok_or_else(|| ThresholdError::Parse(format!("missing ':' in '{}'", core_expr)))?;

    let metric_str = core_expr[..colon_pos].trim();
    let rest = core_expr[colon_pos + 1..].trim();

    let metric = MetricName::from_str(metric_str)
        .ok_or_else(|| ThresholdError::UnsupportedMetric(metric_str.to_string()))?;

    // Parse aggregation: p(95), avg, min, max, med, count, rate, value
    let (agg, remaining) = parse_aggregation(rest)?;

    // Parse operator: <, >, <=, >=
    let (op, value_str) = parse_operator(remaining)?;

    // Parse the threshold value
    let value = value_str
        .trim()
        .parse::<f64>()
        .map_err(|_| ThresholdError::Parse(format!("invalid threshold value: '{}'", value_str)))?;

    Ok(ThresholdCondition {
        metric,
        aggregation: agg,
        op,
        value,
        abort_on_fail,
    })
}

/// Parse an aggregation expression
fn parse_aggregation(s: &str) -> Result<(Aggregation, &str), ThresholdError> {
    let s = s.trim();

    if s.starts_with("p(") || s.starts_with("P(") {
        let close = s
            .find(')')
            .ok_or_else(|| ThresholdError::Parse("missing ')' in percentile".into()))?;
        let p_str = &s[2..close];
        let p = p_str
            .parse::<f64>()
            .map_err(|_| ThresholdError::Parse(format!("invalid percentile: {}", p_str)))?;
        Ok((Aggregation::Percentile(p), s[close + 1..].trim()))
    } else {
        // Find the first whitespace or operator
        let end = s
            .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
            .unwrap_or(s.len());
        let agg_str = &s[..end];
        let remaining = &s[end..];

        let agg = match agg_str {
            "avg" | "AVG" => Aggregation::Avg,
            "min" | "MIN" => Aggregation::Min,
            "max" | "MAX" => Aggregation::Max,
            "med" | "MED" => Aggregation::Med,
            "count" | "COUNT" => Aggregation::Count,
            "rate" | "RATE" => Aggregation::Rate,
            "value" | "VALUE" => Aggregation::Value,
            _ => return Err(ThresholdError::UnsupportedAggregation(agg_str.to_string())),
        };
        Ok((agg, remaining))
    }
}

/// Parse an operator
fn parse_operator(s: &str) -> Result<(ThresholdOp, &str), ThresholdError> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("<=") {
        Ok((ThresholdOp::LessOrEqual, rest.trim()))
    } else if let Some(rest) = s.strip_prefix(">=") {
        Ok((ThresholdOp::GreaterOrEqual, rest.trim()))
    } else if let Some(rest) = s.strip_prefix('<') {
        Ok((ThresholdOp::LessThan, rest.trim()))
    } else if let Some(rest) = s.strip_prefix('>') {
        Ok((ThresholdOp::GreaterThan, rest.trim()))
    } else {
        Err(ThresholdError::Parse(format!(
            "expected operator (<, >, <=, >=) in '{}'",
            s
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_summary() -> MetricsSummary {
        MetricsSummary {
            total_requests: 1000,
            total_errors: 10,
            error_rate: 0.01,
            rps: 100.0,
            duration: std::time::Duration::from_secs(10),
            p50_ms: 45.0,
            p90_ms: 98.0,
            p95_ms: 127.0,
            p99_ms: 210.0,
            p999_ms: 350.0,
            min_ms: 12.0,
            max_ms: 450.0,
            mean_ms: 52.3,
            total_bytes: 1024000,
            total_messages: 0,
            avg_dns_ms: 0.0,
            avg_tcp_ms: 0.0,
            avg_tls_ms: 0.0,
            avg_send_ms: 0.0,
            avg_ttfb_ms: 0.0,
            avg_download_ms: 0.0,
            timed_count: 0,
            error_breakdown: vec![],
        }
    }

    #[test]
    fn test_parse_threshold_p95() {
        let cond = parse_threshold_expression("http_req_duration: p(95) < 500").unwrap();
        assert_eq!(cond.metric, MetricName::HttpReqDuration);
        assert_eq!(cond.aggregation, Aggregation::Percentile(95.0));
        assert_eq!(cond.op, ThresholdOp::LessThan);
        assert_eq!(cond.value, 500.0);
        assert!(!cond.abort_on_fail);
    }

    #[test]
    fn test_parse_threshold_rate_abort() {
        let cond = parse_threshold_expression("http_req_failed: rate < 0.01; abort").unwrap();
        assert_eq!(cond.metric, MetricName::HttpReqFailed);
        assert_eq!(cond.aggregation, Aggregation::Rate);
        assert_eq!(cond.op, ThresholdOp::LessThan);
        assert!(cond.abort_on_fail);
    }

    #[test]
    fn test_parse_threshold_count() {
        let cond = parse_threshold_expression("http_reqs: count > 100").unwrap();
        assert_eq!(cond.metric, MetricName::HttpReqs);
        assert_eq!(cond.aggregation, Aggregation::Count);
        assert_eq!(cond.op, ThresholdOp::GreaterThan);
        assert_eq!(cond.value, 100.0);
    }

    #[test]
    fn test_parse_threshold_avg() {
        let cond = parse_threshold_expression("http_req_duration: avg <= 200").unwrap();
        assert_eq!(cond.aggregation, Aggregation::Avg);
        assert_eq!(cond.op, ThresholdOp::LessOrEqual);
    }

    #[test]
    fn test_evaluate_p95_pass() {
        let cond = parse_threshold_expression("http_req_duration: p(95) < 500").unwrap();
        let result = cond.evaluate(&make_summary());
        assert!(result.passed);
        assert_eq!(result.actual, 127.0);
    }

    #[test]
    fn test_evaluate_p95_fail() {
        let cond = parse_threshold_expression("http_req_duration: p(95) < 100").unwrap();
        let result = cond.evaluate(&make_summary());
        assert!(!result.passed);
    }

    #[test]
    fn test_evaluate_rate_pass() {
        let cond = parse_threshold_expression("http_req_failed: rate < 0.05").unwrap();
        let result = cond.evaluate(&make_summary());
        assert!(result.passed);
    }

    #[test]
    fn test_evaluate_rate_fail() {
        let cond = parse_threshold_expression("http_req_failed: rate < 0.005").unwrap();
        let result = cond.evaluate(&make_summary());
        assert!(!result.passed);
    }

    #[test]
    fn test_evaluate_count() {
        let cond = parse_threshold_expression("http_reqs: count > 500").unwrap();
        let result = cond.evaluate(&make_summary());
        assert!(result.passed);
    }

    #[test]
    fn test_evaluate_max() {
        let cond = parse_threshold_expression("http_req_duration: max < 2000").unwrap();
        let result = cond.evaluate(&make_summary());
        assert!(result.passed);
    }

    #[test]
    fn test_threshold_set() {
        let mut set = ThresholdSet::new();
        set.parse_and_add("http_req_duration: p(95) < 500").unwrap();
        set.parse_and_add("http_req_failed: rate < 0.05").unwrap();
        set.parse_and_add("http_reqs: count > 100").unwrap();

        let results = set.evaluate_all(&make_summary());
        assert!(set.all_passed(&results));
        assert!(set.failures(&results).is_empty());
    }

    #[test]
    fn test_threshold_set_with_failure() {
        let mut set = ThresholdSet::new();
        set.parse_and_add("http_req_duration: p(95) < 500").unwrap();
        set.parse_and_add("http_req_duration: p(95) < 100").unwrap(); // will fail

        let results = set.evaluate_all(&make_summary());
        assert!(!set.all_passed(&results));
        assert_eq!(set.failures(&results).len(), 1);
    }

    #[test]
    fn test_parse_invalid_metric() {
        let result = parse_threshold_expression("unknown_metric: avg < 100");
        assert!(result.is_err());
    }

    #[test]
    fn test_threshold_display() {
        let cond = parse_threshold_expression("http_req_duration: p(95) < 500").unwrap();
        let display = format!("{}", cond);
        assert_eq!(display, "http_req_duration: p(95) < 500");
    }

    #[test]
    fn test_result_display() {
        let cond = parse_threshold_expression("http_req_duration: p(95) < 500").unwrap();
        let result = cond.evaluate(&make_summary());
        let display = format!("{}", result);
        assert!(display.contains("PASS"));
    }
}
