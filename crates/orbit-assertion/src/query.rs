//! Shared logic for read-only data source queries.
//!
//! DB assertions (`builtins::db_query` / `builtins::redis`) and the pre/post-request "database actions"
//! (`orbit-engine::pipeline`) share the extraction and polling logic here instead of duplicating it.
//!
//! Safety contract: this module only calls the read-only [`DataSourceProvider`] interface (`query_sql` /
//! `redis_command`); read-only enforcement is the data source layer's (`orbit-datasource`) job.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::traits::DataSourceProvider;
use crate::types::{DbTarget, QueryResult, RetryPolicy};

/// Extract the target string from a query result by target mode (shared by assertion compare / action vars).
pub fn extract_target(target: &DbTarget, result: &QueryResult) -> Option<String> {
    match target {
        DbTarget::RowCount => Some(result.rows.len().to_string()),
        DbTarget::Scalar => result.scalar().cloned(),
        DbTarget::Cell { row, column } => result.cell(*row, column).cloned(),
        DbTarget::Row { row } => {
            let cells = result.rows.get(*row)?;
            let obj = row_to_json(&result.columns, cells);
            Some(obj.to_string())
        }
        DbTarget::JsonPath { row, path } => {
            let cells = result.rows.get(*row)?;
            let obj = row_to_json(&result.columns, cells);
            find_path(&obj, path)
        }
    }
}

/// Map several columns of row `row` into `variable name → value`.
///
/// `columns` is a `(column name, variable name)` list; missing columns or blank names are skipped (no empty vars).
pub fn extract_columns(
    result: &QueryResult,
    row: usize,
    columns: &[(String, String)],
) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(columns.len());
    for (column, var) in columns {
        if var.trim().is_empty() {
            continue;
        }
        if let Some(value) = result.cell(row, column) {
            out.push((var.clone(), value.clone()));
        }
    }
    out
}

/// Build a JSON object from one row (column name → cell); cells that are JSON text are expanded as objects.
pub fn row_to_json(columns: &[String], cells: &[String]) -> Value {
    let mut map = serde_json::Map::new();
    for (i, col) in columns.iter().enumerate() {
        let raw = cells.get(i).cloned().unwrap_or_default();
        let value = if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            v
        } else {
            Value::String(raw)
        };
        map.insert(col.clone(), value);
    }
    Value::Object(map)
}

/// Look up a dot path on a JSON value (supports `a.b`, `a[0].b`, tolerates a leading `$.`).
pub fn find_path(value: &Value, path: &str) -> Option<String> {
    let mut current = value;
    for segment in path
        .trim_start_matches("$.")
        .trim_start_matches('$')
        .split('.')
    {
        if segment.is_empty() {
            continue;
        }
        // Handle array index name[0]
        if let Some(idx_end) = segment.find('[') {
            let field = &segment[..idx_end];
            let idx_str = &segment[idx_end + 1..segment.len() - 1];
            if !field.is_empty() {
                current = current.get(field)?;
            }
            if let Ok(idx) = idx_str.parse::<usize>() {
                current = current.get(idx)?;
            }
        } else {
            current = current.get(segment)?;
        }
    }
    Some(value_to_string(current))
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => v.to_string(),
    }
}

/// Resolve a retry policy → `(attempts, interval, total timeout)`; default is one attempt, zero interval.
fn retry_params(retry: Option<&RetryPolicy>) -> (u32, Duration, Option<Duration>) {
    match retry {
        Some(r) => (
            r.max_attempts.max(1),
            Duration::from_millis(r.interval_ms),
            r.timeout_ms.map(Duration::from_millis),
        ),
        None => (1, Duration::ZERO, None),
    }
}

/// Poll a read-only SQL query until `accept` deems the result "ready" or attempts/total timeout run out.
///
/// Returns `(last result or error, actual attempts, elapsed milliseconds)`.
/// When queries succeed but `accept` stays false, return the last successful result (callers may degrade).
pub async fn query_sql_retry(
    provider: &dyn DataSourceProvider,
    datasource: &str,
    sql: &str,
    retry: Option<&RetryPolicy>,
    accept: impl Fn(&QueryResult) -> bool,
) -> (Result<QueryResult, String>, u32, u64) {
    let (attempts, interval, deadline) = retry_params(retry);
    let started = Instant::now();
    let mut last_result: Option<QueryResult> = None;
    let mut last_error: Option<String> = None;
    let mut tried = 0u32;

    for attempt in 0..attempts {
        if attempt > 0 {
            if let Some(dl) = deadline {
                if started.elapsed() >= dl {
                    break;
                }
            }
            tokio::time::sleep(interval).await;
        }
        tried = attempt + 1;
        match provider.query_sql(datasource, sql).await {
            Ok(result) => {
                if accept(&result) {
                    return (Ok(result), tried, started.elapsed().as_millis() as u64);
                }
                last_result = Some(result);
            }
            Err(e) => last_error = Some(e),
        }
    }

    let outcome = match last_result {
        Some(r) => Ok(r),
        None => Err(last_error.unwrap_or_else(|| "query returned no result".to_string())),
    };
    (outcome, tried, started.elapsed().as_millis() as u64)
}

/// Poll a read-only Redis command until `accept` deems the value "ready" or attempts/total timeout run out.
pub async fn redis_command_retry(
    provider: &dyn DataSourceProvider,
    datasource: &str,
    args: &[String],
    retry: Option<&RetryPolicy>,
    accept: impl Fn(&str) -> bool,
) -> (Result<String, String>, u32, u64) {
    let (attempts, interval, deadline) = retry_params(retry);
    let started = Instant::now();
    let mut last_value: Option<String> = None;
    let mut last_error: Option<String> = None;
    let mut tried = 0u32;

    for attempt in 0..attempts {
        if attempt > 0 {
            if let Some(dl) = deadline {
                if started.elapsed() >= dl {
                    break;
                }
            }
            tokio::time::sleep(interval).await;
        }
        tried = attempt + 1;
        match provider.redis_command(datasource, args).await {
            Ok(value) => {
                if accept(&value) {
                    return (Ok(value), tried, started.elapsed().as_millis() as u64);
                }
                last_value = Some(value);
            }
            Err(e) => last_error = Some(e),
        }
    }

    let outcome = match last_value {
        Some(v) => Ok(v),
        None => Err(last_error.unwrap_or_else(|| "command returned no result".to_string())),
    };
    (outcome, tried, started.elapsed().as_millis() as u64)
}

/// Result summary (for logs/display): column names + row count, never the full table data.
pub fn summarize_result(result: &QueryResult) -> String {
    let cols = if result.columns.is_empty() {
        "-".to_string()
    } else {
        result.columns.join(", ")
    };
    format!("{} rows · cols[{}]", result.rows.len(), cols)
}

/// Turn a "variable name → value" list into a map (later entries win).
pub fn vars_to_map(vars: Vec<(String, String)>) -> HashMap<String, String> {
    vars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result() -> QueryResult {
        QueryResult {
            columns: vec!["id".into(), "mobile".into(), "meta".into()],
            rows: vec![
                vec![
                    "42".into(),
                    "13800000000".into(),
                    r#"{"status":"PAID"}"#.into(),
                ],
                vec![
                    "43".into(),
                    "13900000000".into(),
                    r#"{"status":"NEW"}"#.into(),
                ],
            ],
            rows_affected: 0,
            elapsed_ms: 3,
        }
    }

    #[test]
    fn extract_target_variants() {
        let r = result();
        assert_eq!(extract_target(&DbTarget::RowCount, &r), Some("2".into()));
        assert_eq!(extract_target(&DbTarget::Scalar, &r), Some("42".into()));
        assert_eq!(
            extract_target(
                &DbTarget::Cell {
                    row: 1,
                    column: "mobile".into()
                },
                &r
            ),
            Some("13900000000".into())
        );
        assert_eq!(
            extract_target(
                &DbTarget::JsonPath {
                    row: 0,
                    path: "meta.status".into()
                },
                &r
            ),
            Some("PAID".into())
        );
        // Out of range / missing column → None
        assert_eq!(
            extract_target(
                &DbTarget::Cell {
                    row: 9,
                    column: "id".into()
                },
                &r
            ),
            None
        );
        assert_eq!(
            extract_target(
                &DbTarget::Cell {
                    row: 0,
                    column: "nope".into()
                },
                &r
            ),
            None
        );
    }

    #[test]
    fn extract_columns_maps_and_skips() {
        let r = result();
        let cols = vec![
            ("id".to_string(), "dbId".to_string()),
            ("mobile".to_string(), "mobile".to_string()),
            ("missing".to_string(), "gone".to_string()),
            ("mobile".to_string(), "".to_string()),
        ];
        let out = extract_columns(&r, 1, &cols);
        assert_eq!(out.len(), 2);
        let map = vars_to_map(out);
        assert_eq!(map.get("dbId").unwrap(), "43");
        assert_eq!(map.get("mobile").unwrap(), "13900000000");
        assert!(!map.contains_key("gone"));
    }

    #[test]
    fn summarize_is_safe() {
        let s = summarize_result(&result());
        assert!(s.contains("2 rows"));
        assert!(s.contains("id"));
        assert!(
            !s.contains("13800000000"),
            "summary should not contain data values"
        );
    }
}
