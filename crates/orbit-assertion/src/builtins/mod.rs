//! Built-in assertion implementations
//!
//! Each assertion lives in its own module and is re-exported into the `builtins` namespace via `pub use`,
//! keeping the public `builtins::XxxAssertion` paths unchanged.

mod body_contains;
mod compare;
mod css_selector;
mod db_query;
mod duration;
mod header;
mod jmes_path;
mod json_path;
mod json_schema;
mod redis;
mod regex;
mod size;
mod status;
mod xpath;

pub use body_contains::BodyContainsAssertion;
pub use css_selector::CssSelectorAssertion;
pub use db_query::DbQueryAssertion;
pub use duration::DurationAssertion;
pub use header::HeaderAssertion;
pub use jmes_path::JmesPathAssertion;
pub use json_path::JsonPathAssertion;
pub use json_schema::JsonSchemaAssertion;
pub use redis::RedisAssertion;
pub use regex::RegexAssertion;
pub use size::SizeAssertion;
pub use status::StatusAssertion;
pub use xpath::XPathAssertion;

/// Shared test context builders (test builds only)
#[cfg(test)]
pub(crate) mod test_util {
    use std::collections::HashMap;

    use crate::types::AssertionContext;

    /// Context without response headers
    pub fn make_ctx(status: i32, body: &str) -> AssertionContext {
        AssertionContext {
            status_code: status,
            headers: HashMap::new(),
            body_bytes: body.as_bytes().to_vec(),
            body_data: None,
            duration_ms: 100,
            body_size: body.len(),
            env_vars: HashMap::new(),
            extracted_vars: HashMap::new(),
            datasources: None,
        }
    }

    /// Context with response headers (names stored lowercase, matching hyper normalization; matching ignores case)
    pub fn ctx(body: &str) -> AssertionContext {
        AssertionContext {
            status_code: 200,
            headers: HashMap::from([
                ("content-type".into(), "application/json".into()),
                ("x-request-id".into(), "req-123".into()),
            ]),
            body_bytes: body.as_bytes().to_vec(),
            body_data: None,
            duration_ms: 100,
            body_size: body.len(),
            env_vars: HashMap::new(),
            extracted_vars: HashMap::new(),
            datasources: None,
        }
    }
}
