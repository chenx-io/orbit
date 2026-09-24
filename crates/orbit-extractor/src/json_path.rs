//! JSONPath extractor

use crate::context::ExtractContext;
use crate::error::ExtractError;
use crate::json::value_to_string;
use crate::traits::{ExtractionKind, Extractor};

/// JSONPath extractor (supports the a.b.c and a[0].b forms)
pub struct JsonPathExtractor {
    pub path: String,
}

impl JsonPathExtractor {
    pub fn new(path: String) -> Self {
        Self { path }
    }
}

impl Extractor for JsonPathExtractor {
    fn kind(&self) -> ExtractionKind {
        ExtractionKind::JsonPath
    }

    fn extract(&self, ctx: &ExtractContext) -> Result<String, ExtractError> {
        let value: serde_json::Value = serde_json::from_str(ctx.body())
            .map_err(|e| ExtractError::Failed(format!("invalid JSON: {}", e)))?;

        let mut current = &value;
        for segment in self
            .path
            .trim_start_matches("$.")
            .trim_start_matches('$')
            .split('.')
        {
            if segment.is_empty() {
                continue;
            }
            if let Some(idx_end) = segment.find('[') {
                let field = &segment[..idx_end];
                let idx_str = &segment[idx_end + 1..segment.len() - 1];
                if !field.is_empty() {
                    current = current.get(field).ok_or_else(|| {
                        ExtractError::Failed(format!("field '{}' not found", field))
                    })?;
                }
                if let Ok(idx) = idx_str.parse::<usize>() {
                    current = current
                        .get(idx)
                        .ok_or_else(|| ExtractError::Failed(format!("index {} not found", idx)))?;
                }
            } else {
                current = current.get(segment).ok_or_else(|| {
                    ExtractError::Failed(format!("field '{}' not found", segment))
                })?;
            }
        }

        value_to_string(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_extract_jsonpath() {
        let body = r#"{"data":{"token":"abc123","user":{"name":"test"}}}"#;
        let headers = HashMap::new();
        let ctx = ExtractContext::new(body, &headers);
        assert_eq!(
            JsonPathExtractor::new("data.token".into())
                .extract(&ctx)
                .unwrap(),
            "abc123"
        );
        assert_eq!(
            JsonPathExtractor::new("data.user.name".into())
                .extract(&ctx)
                .unwrap(),
            "test"
        );
    }
}
