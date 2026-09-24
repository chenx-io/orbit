//! Data layer error types

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DataError {
    #[error("storage IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
    /// Snapshot version is beyond what this build understands (forward-compatibility check)
    #[error("snapshot version not compatible: file {found}, this build supports <={expected}")]
    VersionMismatch { expected: u32, found: u32 },
    #[error("snapshot corrupted: {0}")]
    Corrupt(String),
    #[error("data validation failed: {0}")]
    Validation(String),
    /// Write conflict (optimistic lock): local data is newer than the push base (local = {local}, base = {base})
    #[error("data conflict: local version ({local}) is newer than the push base ({base}), pull again before saving")]
    Conflict { local: i64, base: i64 },
    #[error("not found: {0}")]
    NotFound(String),
}
