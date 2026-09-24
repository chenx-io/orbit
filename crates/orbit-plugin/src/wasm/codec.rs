//! codec-plugin world: WIT bindings (synchronous, pure functions) + host-log implementation.
//! The Codec trait is a synchronous interface, so codec plugins use synchronous component calls (no host-transport).

use std::sync::Arc;

use tokio::sync::Mutex;
use wasmtime::component::{bindgen, Component, Linker};
use wasmtime::{Engine, Store};

bindgen!({
    path: "../orbit-plugin-api/wit/codec",
    world: "codec-plugin",
});

/// Host state for a codec plugin instance (pure functions, stateless)
#[derive(Default)]
pub struct CodecHostState;

impl wasmtime::component::HasData for CodecHostState {
    type Data<'a> = &'a mut CodecHostState;
}

/// host-log: tracing output (codec plugin world)
impl orbit::codec_plugin::host_log::Host for CodecHostState {
    fn log(&mut self, level: String, message: String) {
        match level.as_str() {
            "error" => tracing::error!("[wasm-codec] {}", message),
            "warn" => tracing::warn!("[wasm-codec] {}", message),
            "debug" => tracing::debug!("[wasm-codec] {}", message),
            _ => tracing::info!("[wasm-codec] {}", message),
        }
    }
}

/// Instantiate the codec plugin (synchronous)
pub fn instantiate(
    engine: &Engine,
    component: &Component,
) -> Result<(Store<CodecHostState>, CodecPlugin), String> {
    let mut store = Store::new(engine, CodecHostState);
    let mut linker = Linker::<CodecHostState>::new(engine);
    CodecPlugin::add_to_linker::<CodecHostState, CodecHostState>(&mut linker, |s| s)
        .map_err(|e| e.to_string())?;
    let instance = CodecPlugin::instantiate(&mut store, component, &linker)
        .map_err(|e| format!("instantiate codec plugin: {}", e))?;
    Ok((store, instance))
}

/// Shared codec plugin instance (encode/decode run serially)
pub struct CodecPluginShared {
    pub store: Store<CodecHostState>,
    pub instance: CodecPlugin,
}

/// WasmCodec: bridges the orbit-codec Codec trait -> wasm plugin (synchronous component calls)
pub struct WasmCodec {
    pub codec_name: String,
    pub display_name: String,
    pub shared: Arc<Mutex<CodecPluginShared>>,
}

impl orbit_codec::traits::Codec for WasmCodec {
    fn name(&self) -> &str {
        &self.codec_name
    }
    fn mime_types(&self) -> Vec<&str> {
        vec![]
    }
    fn clone_codec(&self) -> Box<dyn orbit_codec::Codec> {
        Box::new(Self {
            codec_name: self.codec_name.clone(),
            display_name: self.display_name.clone(),
            shared: self.shared.clone(),
        })
    }

    fn encode(
        &self,
        value: &orbit_codec::types::DataValue,
    ) -> Result<Vec<u8>, orbit_codec::CodecError> {
        let mut guard = self
            .shared
            .try_lock()
            .map_err(|_| orbit_codec::CodecError::Encode("codec busy".into()))?;
        let value_json = serde_json::to_string(value)
            .map_err(|e| orbit_codec::CodecError::Encode(e.to_string()))?;
        let req = crate::wasm::codec::exports::orbit::codec_plugin::codec::EncodeRequest {
            value: value_json,
            options: None,
        };
        let CodecPluginShared { store, instance } = &mut *guard;
        let result = instance
            .orbit_codec_plugin_codec()
            .call_encode(store, &req)
            .map_err(|e| orbit_codec::CodecError::Encode(format!("wasm encode: {}", e)))?;
        let out =
            result.map_err(|e| orbit_codec::CodecError::Encode(format!("plugin error: {}", e)))?;
        Ok(out.bytes)
    }

    fn decode(
        &self,
        bytes: &[u8],
    ) -> Result<orbit_codec::types::DataValue, orbit_codec::CodecError> {
        let mut guard = self
            .shared
            .try_lock()
            .map_err(|_| orbit_codec::CodecError::Decode("codec busy".into()))?;
        let req = crate::wasm::codec::exports::orbit::codec_plugin::codec::DecodeRequest {
            bytes: bytes.to_vec(),
            options: None,
        };
        let CodecPluginShared { store, instance } = &mut *guard;
        let result = instance
            .orbit_codec_plugin_codec()
            .call_decode(store, &req)
            .map_err(|e| orbit_codec::CodecError::Decode(format!("wasm decode: {}", e)))?;
        let out =
            result.map_err(|e| orbit_codec::CodecError::Decode(format!("plugin error: {}", e)))?;
        serde_json::from_str(&out.value)
            .map_err(|e| orbit_codec::CodecError::Decode(format!("invalid DataValue JSON: {}", e)))
    }
}
