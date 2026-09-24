//! Error types for the AI layer.
//!
//! Convention: **never use `String` as an error** (matching `orbit-ui-model`'s interface discipline),
//! but when exposed outward (Tauri / CLI) it is converted to readable text via [`AiError::user_message`].

/// Errors from the AI capability layer.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// Transport-layer failure (DNS/TCP/TLS/send/receive).
    #[error("transport failure: {0}")]
    Transport(String),

    /// The provider returned a non-2xx status. `body` is the raw server error text (truncated).
    #[error("model service returned {status}: {body}")]
    Provider {
        /// HTTP status code.
        status: i32,
        /// Error text returned by the server (truncated).
        body: String,
    },

    /// In-stream error (HTTP 200 but the server reports an error inside an event, e.g. content filtering or quota).
    #[error("model stream returned an error: {0}")]
    Stream(String),

    /// No credentials configured (BYOK API key not set, or the provider does not exist).
    #[error("model credentials not configured: {0}")]
    MissingCredential(String),

    /// Authentication failure (401/403).
    #[error("authentication failed: {0}")]
    Auth(String),

    /// Model output could not be parsed / failed validation (must be fed back to the model for a retry).
    #[error("invalid model output: {0}")]
    Invalid(String),

    /// Tool execution failed.
    #[error("tool {name} failed: {message}")]
    Tool {
        /// Tool name.
        name: String,
        /// Failure reason.
        message: String,
    },

    /// Unknown tool name (model hallucination).
    #[error("unknown tool: {0}")]
    UnknownTool(String),

    /// User cancelled.
    #[error("cancelled")]
    Cancelled,

    /// Local I/O failure (credentials / session files).
    #[error("local file error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parse failure.
    #[error("JSON parse failure: {0}")]
    Json(#[from] serde_json::Error),

    /// YAML parse failure.
    #[error("YAML parse failure: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

impl AiError {
    /// User-facing message (used when a Tauri command returns `Err(String)`).
    pub fn user_message(&self) -> String {
        match self {
            AiError::Cancelled => "cancelled".to_string(),
            other => other.to_string(),
        }
    }

    /// Whether the error is retryable (transport failures are; auth/validation failures are not).
    pub fn retryable(&self) -> bool {
        matches!(self, AiError::Transport(_))
    }
}

/// Convenience `Result` alias.
pub type AiResult<T> = Result<T, AiError>;
