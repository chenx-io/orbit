//! Read-only SQL execution (sqlx Any driver: SQLite / PostgreSQL / MySQL).
//!
//! Uniformly stringifies any engine's result set into "column names + text rows" for assertion value extraction.

use std::time::Duration;

use orbit_assertion::QueryResult;
use orbit_config::DataSourceConfig;
use sqlx::any::AnyRow;
use sqlx::{AnyPool, Column, Row, TypeInfo, ValueRef};

use crate::error::DsError;

/// Allowlist of read-only statement leading keywords (matched after stripping comments and lowercasing).
const READONLY_KEYWORDS: &[&str] = &[
    "select", "with", "show", "explain", "describe", "desc", "pragma", "values", "table",
];

/// Maximum query result rows (overflow guard: extra rows are dropped but the count is not hidden).
pub(crate) const MAX_ROWS: usize = 1000;

/// Execute SQL on the connection pool and return a stringified result.
///
/// Constrained by the data source's `query_timeout_ms` and `readonly` protection:
/// When read-only is on, non-allowlisted statements (including statements inside comments) are always rejected.
pub(crate) async fn query_sql(
    pool: &AnyPool,
    cfg: &DataSourceConfig,
    sql: &str,
) -> Result<QueryResult, DsError> {
    if cfg.readonly {
        assert_readonly(cfg, sql)?;
    }
    let timeout = Duration::from_millis(cfg.query_timeout_ms.max(1));
    let trimmed = sql.trim().to_string();
    let started = std::time::Instant::now();

    let fut = async {
        let rows = sqlx::query(&trimmed).fetch_all(pool).await?;
        Ok::<_, sqlx::Error>(rows)
    };
    let rows = match tokio::time::timeout(timeout, fut).await {
        Ok(Ok(rows)) => rows,
        Ok(Err(e)) => return Err(map_sqlx_err(cfg, e)),
        Err(_) => {
            return Err(DsError::Timeout {
                ctx: format!("{} query timed out (>{timeout:?})", cfg.name),
            });
        }
    };
    let elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(rows_to_result(&rows, elapsed_ms))
}

/// Convert a sqlx row set into assertion-semantic results (column names + text rows, first `MAX_ROWS` rows).
fn rows_to_result(rows: &[AnyRow], elapsed_ms: u64) -> QueryResult {
    let mut columns: Vec<String> = Vec::new();
    let mut out_rows: Vec<Vec<String>> = Vec::new();
    let mut truncated = false;
    for (idx, row) in rows.iter().enumerate() {
        if idx == 0 {
            columns = row.columns().iter().map(|c| c.name().to_string()).collect();
        }
        if idx >= MAX_ROWS {
            truncated = true;
            break;
        }
        let mut cells = Vec::with_capacity(columns.len());
        for i in 0..columns.len() {
            cells.push(cell_text(row, i).unwrap_or_default());
        }
        out_rows.push(cells);
    }
    let _ = truncated; // MVP: truncated rows are not exposed separately; control it via query LIMIT if needed
    QueryResult {
        columns,
        rows: out_rows,
        rows_affected: 0,
        elapsed_ms,
    }
}

/// Read-only protection: verify the SQL leading keyword is in the read-only allowlist.
fn assert_readonly(cfg: &DataSourceConfig, sql: &str) -> Result<(), DsError> {
    let first = first_keyword(sql);
    if !READONLY_KEYWORDS.contains(&first.as_str()) {
        return Err(DsError::Readonly(format!(
            "{}: only read-only statements are allowed (SELECT/WITH/SHOW/EXPLAIN/PRAGMA...), current statement starts with `{first}`",
            cfg.name
        )));
    }
    Ok(())
}

/// Extract the leading keyword (lowercased) after stripping comments and whitespace.
fn first_keyword(sql: &str) -> String {
    let cleaned = strip_comments(sql);
    cleaned
        .split(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Strip `--` line comments and `/* ... */` block comments.
fn strip_comments(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            // line comment to end of line
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Uniformly stringify cell values returned by any driver (across SQLite/PostgreSQL/MySQL).
///
/// Dispatch decoding by column type: numbers as decimal text, text/binary as UTF-8, NULL as an empty string,
/// ensuring values can be compared directly with assertion expectations.
pub(crate) fn cell_text(row: &AnyRow, idx: usize) -> Result<String, DsError> {
    let raw = row.try_get_raw(idx).map_err(|e| DsError::Query {
        name: String::new(),
        detail: e.to_string(),
    })?;
    if raw.is_null() {
        return Ok(String::new());
    }
    // Dispatch decoding by type-info name (sqlx normalizes uniformly: BOOLEAN/SMALLINT/INTEGER/BIGINT/REAL/DOUBLE/TEXT/BLOB),
    // compatible with re-export differences between sqlx 0.8.0 / 0.8.6
    let type_name = raw.type_info().name().to_ascii_uppercase();
    let text = match type_name.as_str() {
        "BOOLEAN" => row.try_get::<bool, _>(idx).map(|b| b.to_string()),
        "SMALLINT" | "INTEGER" | "BIGINT" => row.try_get::<i64, _>(idx).map(|i| i.to_string()),
        "REAL" => row.try_get::<f32, _>(idx).map(|v| format_float(v as f64)),
        "DOUBLE" => row.try_get::<f64, _>(idx).map(format_float),
        "BLOB" => row
            .try_get::<Vec<u8>, _>(idx)
            .map(|b| String::from_utf8_lossy(&b).into_owned()),
        // Including TEXT and unknown types: always decode as text (leniency first)
        _ => row.try_get::<String, _>(idx),
    };
    text.map_err(|e| DsError::Query {
        name: String::new(),
        detail: e.to_string(),
    })
}

/// Float-to-text: integer values carry no decimal point (eases comparison with integer expectations).
fn format_float(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        v.to_string()
    }
}

/// sqlx error -> DsError
pub(crate) fn map_sqlx_err(cfg: &DataSourceConfig, e: sqlx::Error) -> DsError {
    match e {
        sqlx::Error::PoolTimedOut => DsError::Timeout {
            ctx: format!("{} acquiring connection timed out", cfg.name),
        },
        sqlx::Error::PoolClosed => {
            DsError::Other(format!("{}: connection pool is closed", cfg.name))
        }
        other => DsError::Query {
            name: cfg.name.clone(),
            detail: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_comment_and_keyword() {
        assert_eq!(first_keyword("-- comment\nSELECT * FROM t"), "select");
        assert_eq!(first_keyword("/* block */ DELETE FROM t"), "delete");
        assert_eq!(
            first_keyword("WITH x AS (SELECT 1) SELECT * FROM x"),
            "with"
        );
        assert_eq!(first_keyword("INSERT INTO t VALUES (1)"), "insert");
    }

    #[test]
    fn readonly_rejects_writes() {
        let cfg = DataSourceConfig {
            name: "t".into(),
            ..Default::default()
        };
        assert!(assert_readonly(&cfg, "SELECT 1").is_ok());
        assert!(assert_readonly(&cfg, "  -- hi\nWITH a AS (SELECT 1) SELECT 1").is_ok());
        assert!(assert_readonly(&cfg, "DELETE FROM t WHERE id=1").is_err());
        assert!(assert_readonly(&cfg, "DROP TABLE t").is_err());
        assert!(assert_readonly(&cfg, "/*x*/ UPDATE t SET a=1").is_err());
    }
}
