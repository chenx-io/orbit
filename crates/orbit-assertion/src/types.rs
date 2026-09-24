//! Assertion engine type definitions

use orbit_codec::DataValue;
use std::collections::HashMap;
use std::sync::Arc;

use crate::traits::DataSourceProvider;

/// Assertion evaluation context
#[derive(Debug, Clone)]
pub struct AssertionContext {
    pub status_code: i32,
    pub headers: HashMap<String, String>,
    pub body_bytes: Vec<u8>,
    pub body_data: Option<DataValue>,
    pub duration_ms: u64,
    pub body_size: usize,
    pub env_vars: HashMap<String, String>,
    pub extracted_vars: HashMap<String, String>,
    /// External data source access (used by DB/Redis assertions). Injected by the host (engine / single-shot agent);
    /// when `None`, data assertions return a predictable hard failure (no data source); built-ins ignore it.
    pub datasources: Option<Arc<dyn DataSourceProvider>>,
}

/// Assertion result
#[derive(Debug)]
pub struct AssertionResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub is_hard: bool,
    /// Variables written by the assertion (DB/Redis `extract_var`): on pass the actual value is carried out
    /// and merged back into the request variable space by the engine for post-scripts and later steps. Empty for built-ins.
    pub exported_vars: HashMap<String, String>,
}

/// Comparator
#[derive(Debug, Clone)]
pub enum Comparator {
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    Contains,
    NotContains,
    Matches,
    Exists,
    InRange(f64, f64),
}

impl Comparator {
    /// All **official names** (order is documentation order; `NAMES[0]` is the default semantics).
    ///
    /// This table exists so AI prompts are written from it and anti-drift tests verify against it
    /// (see `orbit_ai::syntax`) - avoiding the case "prompts teach `eq` but the impl only knows `equal`".
    pub const NAMES: &'static [&'static str] = &[
        "equal",
        "not_equal",
        "contains",
        "not_contains",
        "exists",
        "matches",
        "regex",
        "gt",
        "lt",
    ];

    /// Name → comparator; **unknown names return `None`** (callers decide their own fallback).
    ///
    /// Deliberately not "unknown means Equal": a typo like `contain` (missing the s) would **silently** change semantics.
    /// The engine still falls back to `Equal` for historical behavior, but AI/docs code can use `None` to flag the error.
    /// An empty string means the default `Equal` (`comparator` is usually omitted).
    pub fn from_name(name: &str) -> Option<Comparator> {
        match name {
            "" | "equal" => Some(Comparator::Equal),
            "not_equal" => Some(Comparator::NotEqual),
            "contains" => Some(Comparator::Contains),
            "not_contains" => Some(Comparator::NotContains),
            "exists" => Some(Comparator::Exists),
            "matches" | "regex" => Some(Comparator::Matches),
            "gt" => Some(Comparator::GreaterThan),
            "lt" => Some(Comparator::LessThan),
            _ => None,
        }
    }
}

/// SQL query result (assertion-side view: column names + rows, cell values uniformly stringified).
///
/// Filled by the host (data source connection manager); DB assertions extract the comparison target from it.
#[derive(Debug, Clone, Default)]
pub struct QueryResult {
    /// Result set column names (order matches the cells in `rows`)
    pub columns: Vec<String>,
    /// Result rows: each row is stringified cell values aligned with `columns`
    pub rows: Vec<Vec<String>>,
    /// Affected row count (used by non-query statements; 0 for queries)
    pub rows_affected: u64,
    /// Execution time (milliseconds)
    pub elapsed_ms: u64,
}

/// Get a cell by row/column coordinates (out of range returns None)
impl QueryResult {
    pub fn cell(&self, row: usize, column: &str) -> Option<&String> {
        let idx = self.columns.iter().position(|c| c == column)?;
        self.rows.get(row)?.get(idx)
    }

    pub fn scalar(&self) -> Option<&String> {
        self.rows.first()?.first()
    }
}

/// DB assertion target mode: how to extract the value to compare from the SQL result.
#[derive(Debug, Clone)]
pub enum DbTarget {
    /// Number of result rows
    RowCount,
    /// First row, first column (single-value query)
    Scalar,
    /// Cell at column `column` of row `row`
    Cell { row: usize, column: String },
    /// Whole row `row` as JSON text (column name → value)
    Row { row: usize },
    /// Whole row `row` as JSON, then take `path` (dot path) from it
    JsonPath { row: usize, path: String },
}

/// Polling retry policy: retry every `interval_ms`, up to `max_attempts` times,
/// with an optional `timeout_ms` capping the total time (to wait for async data to land).
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub interval_ms: u64,
    pub max_attempts: u32,
    pub timeout_ms: Option<u64>,
}
