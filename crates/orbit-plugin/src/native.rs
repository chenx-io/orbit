//! Dynamic-library (native) network protocol plugin bridging.
//!
//! Bridges native plugins (`orbit-plugin-api-native`) to `orbit-protocol`'s
//! `ProtocolClient` trait, so native plugins take part in single-shot and load tests like wasm plugins/built-in protocols.
//!
//! A native plugin's execute is synchronous (it may use sqlx etc. with its own pool); the bridge calls it directly;
//! networking/encryption/pooling are entirely self-managed inside the plugin; the host only does dlopen and calls.

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use orbit_plugin_native::NativePluginHandle;

/// NativeProtocolClient: bridges the orbit-protocol ProtocolClient trait -> native plugin execute.
pub struct NativeProtocolClient {
    pub protocol_id: String,
    pub display_name: String,
    pub handle: Arc<NativePluginHandle>,
}

#[async_trait]
impl orbit_protocol::traits::ProtocolClient for NativeProtocolClient {
    fn name(&self) -> &str {
        &self.protocol_id
    }
    fn description(&self) -> &str {
        &self.display_name
    }
    fn clone_client(&self) -> Box<dyn orbit_protocol::traits::ProtocolClient> {
        Box::new(Self {
            protocol_id: self.protocol_id.clone(),
            display_name: self.display_name.clone(),
            handle: self.handle.clone(),
        })
    }

    async fn execute(
        &mut self,
        request: orbit_protocol::types::ProtocolRequest,
    ) -> Result<orbit_protocol::types::ProtocolResponse, orbit_protocol::types::ProtocolError> {
        let p_err = |e: String| orbit_protocol::types::ProtocolError::Protocol(e);

        // Build the native request (JSON across the boundary)
        let native_req = orbit_plugin_api_native::NativeProtocolRequest {
            target: request.target,
            operation: request.operation,
            // payload → base64
            payload_b64: base64::engine::general_purpose::STANDARD.encode(&request.payload),
            connection: request
                .connection
                .as_deref()
                .and_then(|c| serde_json::from_str(c).ok()),
            options: Some(serde_json::to_value(&request.options).unwrap_or_default()),
            timeout_ms: request.timeout.map(|d| d.as_millis() as u64),
        };
        let req_json = serde_json::to_string(&native_req).map_err(|e| p_err(e.to_string()))?;

        // Call the native plugin (synchronous; internal sqlx uses the plugin's own global runtime)
        let resp_json = self.handle.execute(&req_json).map_err(p_err)?;
        let native_resp: orbit_plugin_api_native::NativeProtocolResponse =
            serde_json::from_str(&resp_json).map_err(|e| p_err(e.to_string()))?;

        // payload base64 → bytes
        let payload = base64::engine::general_purpose::STANDARD
            .decode(&native_resp.payload_b64)
            .map_err(|e| p_err(e.to_string()))?;

        Ok(orbit_protocol::types::ProtocolResponse {
            status_code: native_resp.status_code as i32,
            metadata: native_resp.metadata,
            payload,
            message_count: 0,
            // The native plugin's internal timings are not broken down; stages are timed uniformly by the caller (guard)
            timings: orbit_protocol::types::ProtocolTimings::default(),
        })
    }
}
