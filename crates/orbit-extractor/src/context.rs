//! Extraction context

use std::collections::HashMap;

/// Extraction context: carries the response data needed for one extraction (body + headers)
#[derive(Debug, Clone)]
pub struct ExtractContext<'a> {
    body: &'a str,
    headers: &'a HashMap<String, String>,
}

impl<'a> ExtractContext<'a> {
    /// Build a context (body is the lossy UTF-8 converted response body text)
    pub fn new(body: &'a str, headers: &'a HashMap<String, String>) -> Self {
        Self { body, headers }
    }

    /// Response body text
    pub fn body(&self) -> &'a str {
        self.body
    }

    /// Response headers
    pub fn headers(&self) -> &'a HashMap<String, String> {
        self.headers
    }
}
