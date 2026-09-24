//! Extraction errors

/// Extraction error
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("extraction failed: {0}")]
    Failed(String),
}
