use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ProtoFile {
    pub name: String,
    pub content: String,
}

/// Parse proto source files -> package/service/rpc tree (proto import)
#[tauri::command]
pub fn grpc_import_proto(files: Vec<ProtoFile>) -> Result<serde_json::Value, String> {
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|f| (f.name.as_str(), f.content.as_str()))
        .collect();
    orbit_protocol::grpc_descriptor::parse_proto_files(&refs)
        .map(|d| serde_json::to_value(d).unwrap_or(serde_json::Value::Null))
        .map_err(|e| e.to_string())
}

/// Import the interface hierarchy via gRPC Server Reflection
#[tauri::command]
pub async fn grpc_reflection(target: String) -> Result<serde_json::Value, String> {
    orbit_protocol::grpc_descriptor::reflect_descriptor(&target)
        .await
        .map(|d| serde_json::to_value(d).unwrap_or(serde_json::Value::Null))
        .map_err(|e| e.to_string())
}

/// Generate a JSON example template for an rpc input message
#[tauri::command]
pub fn grpc_message_template(
    files: Vec<String>,
    input_type: String,
) -> Result<serde_json::Value, String> {
    let bytes = decode_files(files)?;
    orbit_protocol::grpc_descriptor::message_template(&bytes, &input_type)
        .map_err(|e| e.to_string())
}

/// Generate a schema for rpc input/output messages (aligned with the HTTP model-definition dialog)
#[tauri::command]
pub fn grpc_message_schema(
    files: Vec<String>,
    message_type: String,
) -> Result<serde_json::Value, String> {
    let bytes = decode_files(files)?;
    orbit_protocol::grpc_descriptor::message_schema(&bytes, &message_type)
        .map_err(|e| e.to_string())
}

/// Decode a base64 FileDescriptorProto into a list of byte arrays
fn decode_files(files: Vec<String>) -> Result<Vec<Vec<u8>>, String> {
    use base64::Engine;
    let mut bytes: Vec<Vec<u8>> = Vec::with_capacity(files.len());
    for b64 in &files {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("base64 decode failed: {}", e))?;
        bytes.push(decoded);
    }
    Ok(bytes)
}
