//! # orbit-assertion
//!
//! A powerful assertion engine with composable assertion capabilities:
//! - Built-in assertions: status code, body, duration, JSONPath, regex, XPath, CSS, JSON Schema
//! - JS expression assertions
//! - WASM plugin assertions

pub mod builtins;
pub mod engine;
pub mod query;
pub mod traits;
pub mod types;

pub use engine::AssertionSet;
pub use query::{
    extract_columns, extract_target, query_sql_retry, redis_command_retry, summarize_result,
};
pub use traits::{Assertion, DataSourceProvider};
pub use types::{AssertionContext, AssertionResult, DbTarget, QueryResult, RetryPolicy};
