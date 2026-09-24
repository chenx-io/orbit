//! Config error types

/// Config error
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("YAML parse error: {0}")]
    Parse(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Variable not found: {0}")]
    VariableNotFound(String),
    #[error("Invalid duration: {0}")]
    InvalidDuration(String),
}
