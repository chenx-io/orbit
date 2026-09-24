//! Exchange layer - a unified framework for import / export (pure functions, no IO, channel-agnostic)
//!
//! Architecture:
//! - [`ApiSpec`] protocol-agnostic IR: the sole output of importers and the sole input of exporters
//! - [`ImportFormat`]: source text -> ApiSpec (**a new format only needs to implement this one trait**)
//! - [`ExportFormat`]: ApiSpec -> document / EndpointSpec -> command code
//! - the two use cases are derived by the framework: `ApiSpec -> TestPlan` (scenarios), `ApiSpec -> ImportParseResult` (endpoints)
//!
//! This layer never touches the filesystem / environment variables / network; in the future it can be compiled to WASM for local use in the Web app;
//! Tauri commands and Web HTTP handlers are thin shells over this layer.

mod curl;
mod endpoint;
mod har;
mod ir;
mod jmeter;
mod k6;
mod openapi;
mod postman;
mod shell;

pub use endpoint::{ImportParseResult, ImportedEndpoint, ImportedResponse, ImportedSchema};
pub use ir::{ApiSpec, AuthSpec, EndpointSpec, ModelSpec, ResponseSpec};

use crate::model::plan::TestPlan;

/// Import error
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Unsupported format: {0}")]
    Unsupported(String),
}

/// Export error
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("Serialize error: {0}")]
    Serialize(String),
    #[error("Unsupported format: {0}")]
    Unsupported(String),
}

/// Import source: source text -> ApiSpec (the only trait each new format needs to implement)
pub trait ImportFormat {
    /// Format name (for registry lookup, e.g. "curl" / "postman" / "openapi" / "har" / "k6" / "jmeter")
    fn name(&self) -> &'static str;
    fn parse(&self, input: &str) -> Result<ApiSpec, ImportError>;
}

/// Export target: ApiSpec -> document (collection level) or EndpointSpec -> command/code (request level)
pub trait ExportFormat {
    fn name(&self) -> &'static str;
    /// Collection-level export (openapi / swagger / postman)
    fn export(&self, _spec: &ApiSpec) -> Result<String, ExportError> {
        Err(ExportError::Unsupported(format!(
            "{} does not support collection-level export",
            self.name()
        )))
    }
    /// Request-level export (curl / wget / fetch, etc.)
    fn export_request(&self, _ep: &EndpointSpec) -> Result<String, ExportError> {
        Err(ExportError::Unsupported(format!(
            "{} does not support request-level export",
            self.name()
        )))
    }
}

// ─── Registry (new format = implement the trait + register one line here) ─────────

const IMPORTERS: &[&dyn ImportFormat] = &[
    &curl::CurlImporter,
    &postman::PostmanImporter,
    &openapi::OpenApiImporter,
    &har::HarImporter,
    &k6::K6Importer,
    &jmeter::JmeterImporter,
];

const EXPORTERS: &[&dyn ExportFormat] = &[
    &curl::CurlImporter,                  // curl (request level)
    &postman::PostmanImporter,            // postman (collection level)
    &openapi::OpenApiExporter("openapi"), // openapi 3.x (collection level)
    &openapi::OpenApiExporter("swagger"), // swagger 2.0 (collection level)
    &shell::WGET,
    &shell::HTTPIE,
    &shell::XH,
    &shell::POWERSHELL,
    &shell::FETCH,
    &shell::PYTHON,
];

fn importer(format: &str) -> Result<&'static dyn ImportFormat, ImportError> {
    IMPORTERS
        .iter()
        .find(|f| f.name() == format)
        .copied()
        .ok_or_else(|| ImportError::Unsupported(format!("unsupported import format: {}", format)))
}

fn exporter(format: &str) -> Result<&'static dyn ExportFormat, ExportError> {
    EXPORTERS
        .iter()
        .find(|f| f.name() == format)
        .copied()
        .ok_or_else(|| ExportError::Unsupported(format!("unsupported export format: {}", format)))
}

// ─── Unified entry points ─────────────────────────────────────────

/// Endpoint import: source text -> ImportParseResult (frontend import preview)
pub fn import_endpoints(format: &str, input: &str) -> Result<ImportParseResult, ImportError> {
    let spec = importer(format)?.parse(input)?;
    Ok(spec.into())
}

/// Scenario import: source text -> TestPlan (automated scenarios, all protocols)
pub fn import_scenario(format: &str, input: &str) -> Result<TestPlan, ImportError> {
    let spec = importer(format)?.parse(input)?;
    Ok(spec.into())
}

/// Collection-level export: ApiSpec -> document (openapi / swagger / postman)
pub fn export(format: &str, spec: &ApiSpec) -> Result<String, ExportError> {
    exporter(format)?.export(spec)
}

/// Request-level export: EndpointSpec -> command/code (curl / wget / fetch ...)
pub fn export_request(format: &str, ep: &EndpointSpec) -> Result<String, ExportError> {
    exporter(format)?.export_request(ep)
}

/// List of registered import format names (for frontend options / docs)
pub fn importer_names() -> Vec<&'static str> {
    IMPORTERS.iter().map(|f| f.name()).collect()
}

/// List of registered export format names
pub fn exporter_names() -> Vec<&'static str> {
    EXPORTERS.iter().map(|f| f.name()).collect()
}
