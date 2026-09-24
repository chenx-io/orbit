//! Type definitions related to script execution

use std::collections::HashMap;

/// Request context (readable/modifiable by pre-request scripts)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RequestContext {
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub body: String,
    /// Raw payload (byte string: each character 0-255), used for custom long-connection protocol encoding
    pub raw: String,
}

/// Response context (read-only by post-response scripts)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResponseContext {
    pub status: u16,
    pub body: String,
    pub headers: HashMap<String, String>,
    pub duration_ms: u64,
    /// Raw response (byte string)
    pub raw: String,
    /// Response after script decoding (read back after post_script writes pm.response.decoded)
    pub decoded: Option<String>,
}

/// Script console log entry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptLog {
    pub level: String,
    pub message: String,
}

/// Post-response assertion result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

/// Script execution result: success flag, error, console logs, variables written by the script, post-response assertions.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ScriptOutcome {
    pub success: bool,
    pub error: Option<String>,
    pub logs: Vec<ScriptLog>,
    /// Variable writes by the script outside of `pm.environment.set` / `pm.secret` are merged back into the environment (persisted).
    pub vars_set: HashMap<String, String>,
    /// "Temporary variables" written by the script via `pm.variables.set`, valid only for this request's lifetime, not persisted and not merged into the environment.
    pub temp_vars_set: HashMap<String, String>,
    pub tests: Vec<TestResult>,
    /// The decode result written by the post-response script via `pm.response.decoded`
    pub decoded: Option<String>,
}

/// Environment variable snapshot
pub type EnvVars = HashMap<String, String>;
