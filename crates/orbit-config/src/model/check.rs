//! Assertion/check model

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// Assertion/check
///
/// Kind-specific parameters are carried by [`CheckKind`] (flattened to the same level as `type`),
/// while common metadata (custom name / enable switch) is kept in the nested [`CheckMeta`],
/// avoiding clashes with the top-level fields of individual checks (e.g. `Header.name`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    #[serde(flatten)]
    pub kind: CheckKind,
    /// Common check metadata (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<CheckMeta>,
}

/// Common metadata for a check.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CheckMeta {
    /// Custom check name (for result display; defaults to the type's default name)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether this check is enabled (skipped when disabled)
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

fn is_true(v: &bool) -> bool {
    *v
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CheckKind {
    Status {
        value: i32,
    },
    #[serde(rename = "body_contains")]
    BodyContains {
        value: String,
    },
    #[serde(rename = "duration_lt")]
    DurationLt {
        value: String,
    },
    #[serde(rename = "jsonpath")]
    JsonPath {
        path: String,
        #[serde(default)]
        comparator: String,
        #[serde(default)]
        expected: String,
    },
    #[serde(rename = "jmespath")]
    JmesPath {
        expression: String,
        #[serde(default)]
        comparator: String,
        #[serde(default)]
        expected: String,
    },
    #[serde(rename = "regex")]
    Regex {
        pattern: String,
    },
    #[serde(rename = "size_lt")]
    SizeLt {
        value: usize,
    },
    #[serde(rename = "xpath")]
    XPath {
        path: String,
        #[serde(default)]
        comparator: String,
        #[serde(default)]
        expected: String,
    },
    #[serde(rename = "jsonschema")]
    JsonSchema {
        schema: String,
    },
    #[serde(rename = "header")]
    Header {
        name: String,
        #[serde(default)]
        comparator: String,
        #[serde(default)]
        expected: String,
    },
    #[serde(rename = "css_selector")]
    CssSelector {
        selector: String,
        #[serde(default)]
        comparator: String,
        #[serde(default)]
        expected: String,
    },
    /// Database query check: after the request, query the database and verify the data meets the expectation.
    ///
    /// `sql` supports `${var}` variable interpolation; the query/comparison runs in the response stage and the result can be stored in a variable (`extract_var`).
    #[serde(rename = "db")]
    Db {
        /// Datasource id/name (configured in the global datasource management module)
        datasource: String,
        /// Read-only SQL (supports variable interpolation)
        sql: String,
        /// How to extract a value from the query result
        target: DbTarget,
        #[serde(default)]
        comparator: String,
        #[serde(default)]
        expected: String,
        /// Polling retry policy (waits for asynchronous persistence)
        #[serde(default)]
        retry: Option<RetryPolicy>,
        /// On check success write the actual value into this variable (for post-scripts / later steps to reference)
        #[serde(default)]
        extract_var: Option<String>,
        /// Whether a failure is a hard assertion (aborts the remaining checks of that request)
        #[serde(default = "default_true")]
        hard: bool,
    },
    /// Redis check: after the request, query the cache and verify the command result meets the expectation.
    #[serde(rename = "redis")]
    Redis {
        datasource: String,
        /// Command name (GET / HGET / EXISTS / TTL / LLEN / ...)
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        comparator: String,
        #[serde(default)]
        expected: String,
        #[serde(default)]
        retry: Option<RetryPolicy>,
        #[serde(default)]
        extract_var: Option<String>,
        #[serde(default = "default_true")]
        hard: bool,
    },
}

/// DB check extraction mode: how to extract the value to compare from the SQL query result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DbTarget {
    /// Result row count
    RowCount,
    /// First row, first column
    Scalar,
    /// Cell in the given `column` of row `row`
    Cell {
        #[serde(default)]
        row: usize,
        column: String,
    },
    /// The whole row `row` as JSON text
    Row {
        #[serde(default)]
        row: usize,
    },
    /// Value looked up by dot path in the whole row `row` as JSON
    JsonPath {
        #[serde(default)]
        row: usize,
        path: String,
    },
}

/// Polling retry policy: retry every `interval_ms`, at most `max_attempts` times,
/// with `timeout_ms` an optional overall timeout cap (0/default = unlimited).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    #[serde(default)]
    pub interval_ms: u64,
    #[serde(default = "default_attempts")]
    pub max_attempts: u32,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn default_attempts() -> u32 {
    3
}
