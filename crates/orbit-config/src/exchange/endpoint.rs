//! Endpoint display model (used for frontend import preview, a stable public contract)
//!
//! Derived from [`super::ir::ApiSpec`] (`From<ApiSpec>`),
//! returned to the frontend `ImportDialog` by the HTTP API / Tauri commands.

use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, serde::Serialize)]
pub struct ImportedEndpoint {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub query_params: HashMap<String, String>,
    pub body: String,
    pub content_type: String,
    pub group: String,
    pub summary: String,
    /// Name of the data model associated with the request body ($ref -> #/components/schemas/X or #/definitions/X)
    #[serde(default)]
    pub model_ref: String,
    /// Auth type: none / bearer / basic / apikey / oauth2
    #[serde(default)]
    pub auth_type: String,
    /// Request parameter name for apiKey
    #[serde(default)]
    pub auth_key_name: String,
    /// Inject location for apiKey: header / query
    #[serde(default)]
    pub auth_add_to: String,
    /// Response examples (by status code)
    #[serde(default)]
    pub responses: Vec<ImportedResponse>,
    /// Pre script (OpenAPI: operation-level x-orbit-prerequest extension; Postman: event.listen == "prerequest")
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pre_script: String,
    /// Post script (OpenAPI: operation-level x-orbit-postrequest extension; Postman: event.listen == "test")
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub post_script: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ImportedResponse {
    pub status: u16,
    pub name: String,
    pub body: String,
    /// Dereferenced raw schema (including field description, used by the frontend to render field comments)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
}

#[derive(Debug, serde::Serialize)]
pub struct ImportedSchema {
    pub name: String,
    pub schema_json: Value,
}

#[derive(Debug, serde::Serialize)]
pub struct ImportParseResult {
    pub endpoints: Vec<ImportedEndpoint>,
    pub schemas: Vec<ImportedSchema>,
}
