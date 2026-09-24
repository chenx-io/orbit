//! Export commands (Tauri channel) - all routed through the orbit-config::exchange request-level exporter

use std::collections::HashMap;

use crate::state::AppState;
use tauri::State;

#[derive(serde::Deserialize)]
pub struct ExportRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[tauri::command]
pub fn export_request(request: ExportRequest, format: String) -> Result<String, String> {
    let ep = orbit_config::exchange::EndpointSpec::new(
        "export",
        orbit_config::RequestSpec::Http(Box::new(orbit_config::HttpRequestConfig {
            method: request.method,
            url: request.url,
            headers: request.headers,
            body: if request.body.is_empty() {
                None
            } else {
                Some(serde_yaml::Value::String(request.body))
            },
            timeout: "30s".to_string(),
            payload_format: None,
            grpc_service: None,
            grpc_use_reflection: false,
            response_format: None,
        })),
    );
    orbit_config::exchange::export_request(&format, &ep).map_err(|e| e.to_string())
}

/// Collection-level export (openapi / swagger / postman): the backend queries its own data
/// (DataService is the authoritative store); the frontend only passes the "export range" and title.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRangeRequest {
    pub format: String,
    pub title: String,
    pub collection_id: String,
    pub item_id: Option<String>,
    pub workspace_id: Option<String>,
}

#[tauri::command]
pub fn export_collection(
    state: State<'_, AppState>,
    request: ExportRangeRequest,
) -> Result<String, String> {
    let range = orbit_data::export::ExportRange {
        workspace_id: request.workspace_id,
        collection_id: request.collection_id,
        item_id: request.item_id,
    };
    let snapshot = state.data.snapshot();
    orbit_data::export::export_document(&snapshot, &request.format, &request.title, &range)
        .map_err(|e| e.to_string())
}
