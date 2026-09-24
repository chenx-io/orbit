//! JSON value -> string (shared by JSON-based extractors)

use serde_json::Value;

use crate::error::ExtractError;

/// Convert a serde_json::Value into a string
pub(crate) fn value_to_string(value: &Value) -> Result<String, ExtractError> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Null => Ok("null".into()),
        Value::Array(arr) => {
            serde_json::to_string(arr).map_err(|e| ExtractError::Failed(e.to_string()))
        }
        other => Ok(other.to_string()),
    }
}
