//! Endpoint display model (used by the frontend import preview; a stable public contract).
//!
//! Compatibility entry point: the implementation lives in [`crate::exchange`]. Shared by the
//! Tauri commands and the HTTP API.

pub use crate::exchange::{ImportParseResult, ImportedEndpoint, ImportedResponse, ImportedSchema};

/// Parse an OpenAPI 2.0 / 3.0 document (JSON / YAML auto-detected) into an endpoint list plus
/// data models.
///
/// Compatibility entry point: forwards to the exchange openapi importer.
pub fn parse_openapi(input: &str) -> Result<ImportParseResult, String> {
    crate::exchange::import_endpoints("openapi", input).map_err(|e| e.to_string())
}
