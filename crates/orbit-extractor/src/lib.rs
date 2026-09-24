//! # orbit-extractor
//!
//! Variable extractors - pull variables from HTTP responses for later requests.
//!
//! Unified as the [`Extractor`] trait + the [`ExtractorSet`] collection:
//! - JSONPath (a.b.c, a[0].b)
//! - JMESPath (a.b.c, a[0].b, length(a), etc.)
//! - Header
//! - Regex (with group capture)
//! - Cookie (extracted from the Set-Cookie header)
//!
//! To add a mode, implement [`Extractor`] and register it via [`ExtractorSet::add`];
//! callers need no changes.

mod context;
mod cookie;
mod error;
mod header;
mod jmes_path;
mod json;
mod json_path;
mod regex;
mod traits;

pub use context::ExtractContext;
pub use cookie::CookieExtractor;
pub use error::ExtractError;
pub use header::HeaderExtractor;
pub use jmes_path::JmesPathExtractor;
pub use json_path::JsonPathExtractor;
pub use regex::RegexExtractor;
pub use traits::{ExtractionKind, Extractor};

use std::collections::HashMap;

/// A collection of extractors
pub struct ExtractorSet {
    extractors: Vec<(String, Box<dyn Extractor>)>,
}

impl ExtractorSet {
    pub fn new() -> Self {
        Self {
            extractors: Vec::new(),
        }
    }

    /// Add a named extractor (accepts a concrete type or `Box<dyn Extractor>`)
    pub fn add<E: Extractor + 'static>(&mut self, name: String, extractor: E) {
        self.extractors.push((name, Box::new(extractor)));
    }

    pub fn is_empty(&self) -> bool {
        self.extractors.is_empty()
    }

    /// Run all extractors
    pub fn extract_all(
        &self,
        body: &[u8],
        headers: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>, ExtractError> {
        let body_str = String::from_utf8_lossy(body);
        let ctx = ExtractContext::new(&body_str, headers);
        let mut vars = HashMap::new();
        for (name, extractor) in &self.extractors {
            let value = extractor.extract(&ctx)?;
            vars.insert(name.clone(), value);
        }
        Ok(vars)
    }
}

impl Default for ExtractorSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CookieExtractor, HeaderExtractor, JmesPathExtractor};

    #[test]
    fn test_extractor_set_combined() {
        let mut set = ExtractorSet::new();
        set.add(
            "token".to_string(),
            JmesPathExtractor::new("auth.token".into()),
        );
        set.add(
            "req_id".to_string(),
            HeaderExtractor::new("x-request-id".into()),
        );
        set.add(
            "session".to_string(),
            CookieExtractor::new("session".into(), None),
        );

        let body = r#"{"auth":{"token":"jwt-abc-123"}}"#;
        let headers = HashMap::from([
            ("x-request-id".to_string(), "req-999".to_string()),
            (
                "set-cookie".to_string(),
                "session=sess-xyz; Path=/".to_string(),
            ),
        ]);

        let vars = set.extract_all(body.as_bytes(), &headers).unwrap();
        assert_eq!(vars.get("token").unwrap(), "jwt-abc-123");
        assert_eq!(vars.get("req_id").unwrap(), "req-999");
        assert_eq!(vars.get("session").unwrap(), "sess-xyz");
    }
}
