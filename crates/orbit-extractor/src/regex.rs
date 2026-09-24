//! Regex extractor

use crate::context::ExtractContext;
use crate::error::ExtractError;
use crate::traits::{ExtractionKind, Extractor};

/// Regex extractor: matches a regex against the body and takes the given capture group
pub struct RegexExtractor {
    pub pattern: String,
    pub group: usize,
}

impl RegexExtractor {
    pub fn new(pattern: String, group: usize) -> Self {
        Self { pattern, group }
    }
}

impl Extractor for RegexExtractor {
    fn kind(&self) -> ExtractionKind {
        ExtractionKind::Regex
    }

    fn extract(&self, ctx: &ExtractContext) -> Result<String, ExtractError> {
        let re = regex::Regex::new(&self.pattern)
            .map_err(|e| ExtractError::Failed(format!("invalid regex: {}", e)))?;
        let caps = re
            .captures(ctx.body())
            .ok_or_else(|| ExtractError::Failed("regex no match".into()))?;
        caps.get(self.group)
            .map(|m| m.as_str().to_string())
            .ok_or_else(|| ExtractError::Failed(format!("group {} not found", self.group)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_extract_regex() {
        let e = RegexExtractor::new(r"Bearer\s+(.+)".into(), 1);
        let headers = HashMap::new();
        let ctx = ExtractContext::new("Token: Bearer xyz-789-abc", &headers);
        assert_eq!(e.extract(&ctx).unwrap(), "xyz-789-abc");
    }
}
