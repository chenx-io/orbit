//! Shared "actual vs expected" comparison helpers for DB/Redis assertions.
//!
//! Semantics match the existing built-ins: `Exists` only checks presence; `Equal/Contains/Gt/Lt` etc.
//! are unsatisfied when the actual value is missing (never silently passing); `Matches` treats the expected value as a regex.

use regex::Regex;

use crate::types::Comparator;

/// Compare the actual value (which may be missing) against the expected value using the comparator.
///
/// `actual` of `None` means the target does not exist (e.g. empty result / missing column),
/// and every comparator except `Exists` returns false.
pub fn compare_value(comparator: &Comparator, actual: Option<&str>, expected: &str) -> bool {
    match comparator {
        Comparator::Exists => actual.is_some(),
        Comparator::NotEqual => actual.map(|a| a != expected).unwrap_or(false),
        Comparator::Contains => actual.map(|a| a.contains(expected)).unwrap_or(false),
        Comparator::NotContains => actual.map(|a| !a.contains(expected)).unwrap_or(false),
        Comparator::Matches => actual
            .map(|a| {
                Regex::new(expected)
                    .map(|re| re.is_match(a))
                    .unwrap_or(false)
            })
            .unwrap_or(false),
        Comparator::Equal => actual == Some(expected),
        Comparator::GreaterThan => num_cmp(actual, expected, |a, e| a > e),
        Comparator::LessThan => num_cmp(actual, expected, |a, e| a < e),
        Comparator::InRange(lo, hi) => actual
            .and_then(|a| a.parse::<f64>().ok())
            .map(|v| v >= *lo && v <= *hi)
            .unwrap_or(false),
    }
}

/// Numeric comparison: if both sides parse as floats compare numerically, otherwise unsatisfied.
fn num_cmp(actual: Option<&str>, expected: &str, pred: fn(f64, f64) -> bool) -> bool {
    let Some(a) = actual.and_then(|s| s.parse::<f64>().ok()) else {
        return false;
    };
    let Ok(e) = expected.parse::<f64>() else {
        return false;
    };
    pred(a, e)
}

/// Build an assertion result with empty exported variables.
pub fn make_result(
    name: &str,
    passed: bool,
    message: String,
    hard: bool,
) -> crate::types::AssertionResult {
    crate::types::AssertionResult {
        name: name.to_string(),
        passed,
        message,
        is_hard: hard,
        exported_vars: std::collections::HashMap::new(),
    }
}
