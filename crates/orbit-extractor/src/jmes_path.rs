//! JMESPath extractor

use crate::context::ExtractContext;
use crate::error::ExtractError;
use crate::json::value_to_string;
use crate::traits::{ExtractionKind, Extractor};

/// JMESPath extractor (a.b.c, a[0].b, length(a), filter expressions, ...)
pub struct JmesPathExtractor {
    pub expression: String,
}

impl JmesPathExtractor {
    pub fn new(expression: String) -> Self {
        Self { expression }
    }
}

impl Extractor for JmesPathExtractor {
    fn kind(&self) -> ExtractionKind {
        ExtractionKind::JmesPath
    }

    fn extract(&self, ctx: &ExtractContext) -> Result<String, ExtractError> {
        let json: serde_json::Value = serde_json::from_str(ctx.body())
            .map_err(|e| ExtractError::Failed(format!("invalid JSON: {}", e)))?;

        let expr = jmespath::compile(&self.expression)
            .map_err(|e| ExtractError::Failed(format!("invalid JMESPath expression: {}", e)))?;

        let result = expr
            .search(json)
            .map_err(|e| ExtractError::Failed(format!("JMESPath search failed: {}", e)))?;

        // result is an Rc<jmespath::Variable>, converted to serde_json::Value via serde
        let value: serde_json::Value = serde_json::to_value(result.as_ref()).map_err(|e| {
            ExtractError::Failed(format!("JMESPath result conversion failed: {}", e))
        })?;
        value_to_string(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn extract(body: &str, expression: &str) -> Result<String, ExtractError> {
        let headers = HashMap::new();
        JmesPathExtractor::new(expression.into()).extract(&ExtractContext::new(body, &headers))
    }

    #[test]
    fn test_extract_jmespath_simple() {
        let body = r#"{"data":{"token":"abc123","user":{"name":"test"}}}"#;
        assert_eq!(extract(body, "data.token").unwrap(), "abc123");
        assert_eq!(extract(body, "data.user.name").unwrap(), "test");
    }

    #[test]
    fn test_extract_jmespath_array_index() {
        let body = r#"{"items":[{"id":1},{"id":2},{"id":3}]}"#;
        assert_eq!(extract(body, "items[0].id").unwrap(), "1");
        assert_eq!(extract(body, "items[2].id").unwrap(), "3");
    }

    #[test]
    fn test_extract_jmespath_filter() {
        let body = r#"{"users":[{"name":"Alice","age":30},{"name":"Bob","age":25}]}"#;
        // Filter the user names with age > 28
        let result = extract(body, "users[?age > `28`].name").unwrap();
        assert!(result.contains("Alice"));
    }

    #[test]
    fn test_extract_jmespath_length() {
        let body = r#"{"items":[1,2,3,4,5]}"#;
        assert_eq!(extract(body, "length(items)").unwrap(), "5");
    }

    #[test]
    fn test_extract_jmespath_invalid_json() {
        assert!(extract("not json", "data.token").is_err());
    }

    #[test]
    fn test_extract_jmespath_invalid_expression() {
        assert!(extract(r#"{"key":"value"}"#, "[[invalid").is_err());
    }
}
