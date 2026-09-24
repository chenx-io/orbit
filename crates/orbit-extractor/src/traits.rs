//! Extractor trait - a unified abstraction over all extraction modes

use crate::context::ExtractContext;
use crate::error::ExtractError;

/// Extraction mode kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtractionKind {
    /// JSONPath
    JsonPath,
    /// JMESPath
    JmesPath,
    /// Response header
    Header,
    /// Regex (with group capture)
    Regex,
    /// Cookie（Set-Cookie header）
    Cookie,
}

impl ExtractionKind {
    /// Kind name
    pub fn as_str(self) -> &'static str {
        match self {
            ExtractionKind::JsonPath => "jsonpath",
            ExtractionKind::JmesPath => "jmespath",
            ExtractionKind::Header => "header",
            ExtractionKind::Regex => "regex",
            ExtractionKind::Cookie => "cookie",
        }
    }
}

/// Variable extractor: extracts a single variable value from the response body/headers
///
/// Extractors are stateless and safe to share. To add a mode, implement this trait and
/// register it via [`crate::ExtractorSet::add`].
pub trait Extractor: Send + Sync {
    /// Extraction mode kind
    fn kind(&self) -> ExtractionKind;

    /// Run the extraction
    fn extract(&self, ctx: &ExtractContext) -> Result<String, ExtractError>;
}

impl Extractor for Box<dyn Extractor> {
    fn kind(&self) -> ExtractionKind {
        (**self).kind()
    }

    fn extract(&self, ctx: &ExtractContext) -> Result<String, ExtractError> {
        (**self).extract(ctx)
    }
}
