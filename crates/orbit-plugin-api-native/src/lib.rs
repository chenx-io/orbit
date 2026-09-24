//! Orbit dynamic-library (native) network protocol plugin SDK.
//!
//! Plugins are compiled as `cdylib` and export the following stable `extern "C"` functions (ABI-safe via abi_stable):
//! - `orbit_native_api_version`: interface version number
//! - `orbit_native_info`: plugin metadata (JSON)
//! - `orbit_native_execute`: execute one network request (input JSON, output JSON)
//!
//! Input and output cross the boundary as JSON strings (`RBox<str>`); plugins may freely use any ecosystem library internally
//! (such as `sqlx` or `tokio-postgres` with their own pools); the host does not touch networking/encryption/pooling.
//!
//! Plugin-side usage:
//! ```ignore
//! use orbit_plugin_api_native::NativePlugin;
//! struct MyPlugin;
//! impl NativePlugin for MyPlugin { /* ... */ }
//! orbit_plugin_api_native::export_plugin!(MyPlugin);
//! ```

use serde::{Deserialize, Serialize};

/// Re-export abi_stable for the `export_plugin!` macro and plugin authors (consistent with the loader)
pub use abi_stable;

/// Current ABI version
pub const API_VERSION: u32 = 1;

/// Protocol plugin capability declaration (`get_capabilities` semantics)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeCapability {
    /// Protocol id (e.g. `pg`, `redis`)
    pub protocol_id: String,
    /// Display name
    pub display_name: String,
    /// Description
    pub description: String,
}

/// Plugin metadata (returned by `orbit_native_info`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativePluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    /// Plugin type (always protocol)
    pub kind: String,
    /// Declares the supported protocols
    pub capabilities: Vec<NativeCapability>,
    /// Connection-parameter JSON Schema (drives the frontend dynamic form)
    #[serde(default)]
    pub connection_config_schema: Option<serde_json::Value>,
    /// Message-parameter JSON Schema
    #[serde(default)]
    pub request_config_schema: Option<serde_json::Value>,
}

/// One network request (`orbit_native_execute` input)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeProtocolRequest {
    /// Target address (e.g. `host:port`; may be empty for url direct-connect mode)
    #[serde(default)]
    pub target: String,
    /// Operation identifier (optional)
    #[serde(default)]
    pub operation: String,
    /// Request payload (base64-encoded; binary-safe)
    #[serde(default)]
    pub payload_b64: String,
    /// Connection config JSON (rendered from connectionConfigSchema)
    #[serde(default)]
    pub connection: Option<serde_json::Value>,
    /// Message-parameter JSON (rendered from requestConfigSchema)
    #[serde(default)]
    pub options: Option<serde_json::Value>,
    /// Timeout in milliseconds
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Execution result (returned by `orbit_native_execute`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeProtocolResponse {
    pub status_code: u16,
    /// Response payload (base64-encoded)
    pub payload_b64: String,
    /// Response metadata (e.g. content-type)
    #[serde(default)]
    pub metadata: Vec<(String, String)>,
    /// Timing statistics
    #[serde(default)]
    pub timings: serde_json::Value,
}

/// Dynamic-library protocol plugin trait. Plugins implement this trait and export it via `export_plugin!`.
pub trait NativePlugin: Send + Sync {
    /// Plugin metadata + capability declaration
    fn info(&self) -> NativePluginInfo;
    /// Execute one network request (plugins manage their own connection pools internally)
    fn execute(&self, req: NativeProtocolRequest) -> Result<NativeProtocolResponse, String>;
}

// ─── Serialization helpers ───────────────────────────

/// Parse the request JSON into a `NativeProtocolRequest`
pub fn parse_request(json: &str) -> Result<NativeProtocolRequest, String> {
    serde_json::from_str(json).map_err(|e| format!("invalid request JSON: {}", e))
}

/// Encode the execution result as JSON
pub fn encode_response(resp: &NativeProtocolResponse) -> Result<String, String> {
    serde_json::to_string(resp).map_err(|e| format!("failed to encode result: {}", e))
}

/// Export the plugin: generates stable `extern "C"` entry functions.
///
/// Passes JSON across the ABI via abi_stable's stable type (`RString`).
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty) => {
        pub mod chenx_native_export {
            use super::*;
            use $crate::NativePlugin;
            use $crate::abi_stable::std_types::RString;

            /// Plugin instance (lazily initialized; plugins are usually stateless or hold a connection pool)
            static PLUGIN: std::sync::OnceLock<$plugin_type> = std::sync::OnceLock::new();

            fn instance() -> &'static $plugin_type {
                PLUGIN.get_or_init(|| <$plugin_type>::default())
            }

            #[no_mangle]
            pub extern "C" fn chenx_native_api_version() -> u32 {
                $crate::API_VERSION
            }

            #[no_mangle]
            pub extern "C" fn chenx_native_info() -> RString {
                let info = instance().info();
                RString::from(
                    serde_json::to_string(&info).unwrap_or_else(|_| "{}".into()),
                )
            }

            #[no_mangle]
            pub extern "C" fn chenx_native_execute(req_json: RString) -> RString {
                let result = $crate::parse_request(&req_json)
                    .and_then(|req| instance().execute(req))
                    .and_then(|resp| $crate::encode_response(&resp));
                match result {
                    Ok(json) => RString::from(json),
                    Err(e) => RString::from(
                        serde_json::json!({ "error": e }).to_string(),
                    ),
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_ok() {
        let req = parse_request(
            r#"{"target":"127.0.0.1:5432","operation":"query","payload_b64":"aGk=","timeout_ms":1000}"#,
        )
        .unwrap();
        assert_eq!(req.target, "127.0.0.1:5432");
        assert_eq!(req.operation, "query");
        assert_eq!(req.payload_b64, "aGk=");
        assert_eq!(req.timeout_ms, Some(1000));
        assert!(req.connection.is_none());
        assert!(req.options.is_none());
    }

    #[test]
    fn parse_request_defaults_missing_fields() {
        // Missing fields should have a serde default
        let req = parse_request(r#"{}"#).unwrap();
        assert_eq!(req.target, "");
        assert_eq!(req.payload_b64, "");
        assert_eq!(req.timeout_ms, None);
    }

    #[test]
    fn parse_request_rejects_invalid_json() {
        assert!(parse_request(r#"{not-json"#).is_err());
        assert!(parse_request(r#""#).is_err());
    }

    #[test]
    fn encode_response_roundtrip() {
        let resp = NativeProtocolResponse {
            status_code: 200,
            payload_b64: "aGVsbG8=".into(),
            metadata: vec![("content-type".into(), "text/plain".into())],
            timings: serde_json::json!({ "total_ms": 1.5 }),
        };
        let json = encode_response(&resp).unwrap();
        let parsed: NativeProtocolResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status_code, 200);
        assert_eq!(parsed.payload_b64, "aGVsbG8=");
        assert_eq!(parsed.metadata.len(), 1);
        assert_eq!(parsed.timings["total_ms"], 1.5);
    }

    #[test]
    fn plugin_info_serde() {
        let info = NativePluginInfo {
            name: "demo".into(),
            version: "1.0.0".into(),
            description: "demo plugin".into(),
            kind: "protocol".into(),
            capabilities: vec![NativeCapability {
                protocol_id: "pg".into(),
                display_name: "PostgreSQL".into(),
                description: "PG query".into(),
            }],
            connection_config_schema: Some(serde_json::json!({ "type": "object" })),
            request_config_schema: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: NativePluginInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, "protocol");
        assert_eq!(back.capabilities[0].protocol_id, "pg");
        assert!(back.connection_config_schema.is_some());
        assert!(back.request_config_schema.is_none());
    }
}
