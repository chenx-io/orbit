//! Output export errors

/// Output error
#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("Export failed: {0}")]
    Export(String),
}

/// Result type for output exports
pub type OutputResult<T> = Result<T, OutputError>;
