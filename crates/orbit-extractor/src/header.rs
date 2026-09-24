//! Header extractor

use crate::context::ExtractContext;
use crate::error::ExtractError;
use crate::traits::{ExtractionKind, Extractor};

/// Header extractor: extracts the value of the given header from the response headers
pub struct HeaderExtractor {
    pub name: String,
}

impl HeaderExtractor {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

impl Extractor for HeaderExtractor {
    fn kind(&self) -> ExtractionKind {
        ExtractionKind::Header
    }

    fn extract(&self, ctx: &ExtractContext) -> Result<String, ExtractError> {
        Ok(ctx.headers().get(&self.name).cloned().unwrap_or_default())
    }
}
