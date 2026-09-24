//! Data source error types.

use thiserror::Error;

/// Data source access error.
#[derive(Debug, Error)]
pub enum DsError {
    #[error("data source not found: {0}")]
    NotFound(String),
    #[error("data source is disabled: {0}")]
    Disabled(String),
    #[error("data source type does not support this operation: {0}")]
    Unsupported(String),
    #[error("connection failed [{name}]: {detail}")]
    Connect {
        /// Data source name
        name: String,
        /// Connection failure reason
        detail: String,
    },
    #[error("{ctx}: acquiring connection / query timed out")]
    Timeout {
        /// Context description (data source name, etc.)
        ctx: String,
    },
    #[error("read-only protection rejected this operation: {0}")]
    Readonly(String),
    #[error("execution failed [{name}]: {detail}")]
    Query {
        /// Data source name
        name: String,
        /// Underlying error
        detail: String,
    },
    #[error("{0}")]
    Other(String),
}

impl DsError {
    /// Error description for the Tauri/HTTP layer.
    pub fn message(&self) -> String {
        self.to_string()
    }
}
