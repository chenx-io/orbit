//! Assertion trait

use crate::types::{AssertionContext, AssertionResult, QueryResult};
use async_trait::async_trait;

/// Assertion trait
#[async_trait]
pub trait Assertion: Send + Sync {
    fn name(&self) -> &str;
    fn is_hard(&self) -> bool {
        false
    }
    async fn evaluate(&self, ctx: &AssertionContext) -> AssertionResult;
}

/// Data source access capability (injected by the host).
///
/// DB/Redis assertions use it to run read-only queries; pure in-memory built-ins ignore it,
/// and when `AssertionContext::datasources` is `None` data assertions fail predictably,
/// never passing silently.
#[async_trait]
pub trait DataSourceProvider: std::fmt::Debug + Send + Sync {
    /// Run a read-only SQL query on the given data source and return a structured result.
    async fn query_sql(&self, datasource: &str, sql: &str) -> Result<QueryResult, String>;
    /// Run a read-only Redis command on the given data source (`args[0]` is the command name, the rest are arguments), returning a stringified result.
    async fn redis_command(&self, datasource: &str, args: &[String]) -> Result<String, String>;
}
