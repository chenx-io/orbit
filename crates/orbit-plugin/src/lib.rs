//! # orbit-plugin
//!
//! WASM plugin system: load protocol/codec components -> register capabilities into the global dynamic registry
//! (orbit-protocol / orbit-codec), taking part in single-shot and load testing just like built-in protocols/formats.
//!
//! Supports:
//! - WASM Protocol: new network protocols (connections managed uniformly by the host via host-transport)
//! - WASM Codec: brand-new binary data formats (pure functions, called as synchronous components)
//! - Native Protocol: dynamic-library network protocols (reuse ecosystems like sqlx, with their own connection pools)

pub mod manifest;
pub mod native;
pub mod wasm;
pub mod zip;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;
use wasmtime::Engine;

use crate::manifest::PluginManifest;
use crate::wasm::codec::{CodecPluginShared, WasmCodec};
use crate::wasm::protocol::{ProtocolPluginShared, WasmProtocolClient};

/// Plugin runtime state
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// "protocol" | "codec"
    pub kind: String,
    /// "enabled" | "error"
    pub status: String,
    pub error: Option<String>,
    /// Registered protocol id (kind=protocol)
    pub protocols: Vec<String>,
    /// Registered format name (kind=codec)
    pub codecs: Vec<String>,
    /// Connection-parameter JSON Schema (protocol plugins, maps to manifest.connectionConfigSchema)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_config_schema: Option<serde_json::Value>,
    /// Message-parameter JSON Schema (protocol plugins, maps to manifest.requestConfigSchema)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_config_schema: Option<serde_json::Value>,
    /// Plugin install directory (after zip install it is `<plugins-root>/<id>`; None = loaded temporarily, not persisted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_dir: Option<String>,
}

#[derive(Debug)]
pub struct ScanReport {
    pub scanned: Vec<String>,
    pub loaded: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Plugin manager: uniformly manages wasm plugins (protocol/codec) and native dynamic-library plugins (network protocols).
pub struct PluginManager {
    engine: Engine,
    plugins: HashMap<String, PluginDescriptor>,
    protocol_handles: HashMap<String, Arc<Mutex<ProtocolPluginShared>>>,
    codec_handles: HashMap<String, Arc<Mutex<CodecPluginShared>>>,
    native_handles: HashMap<String, Arc<orbit_plugin_native::NativePluginHandle>>,
}

impl PluginManager {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            engine: wasm::protocol::new_engine()?,
            plugins: HashMap::new(),
            protocol_handles: HashMap::new(),
            codec_handles: HashMap::new(),
            native_handles: HashMap::new(),
        })
    }

    /// Load a native (dynamic-library) protocol plugin: dlopen -> info() -> register protocol capability -> build NativeProtocolClient.
    ///
    /// `id` is the plugin identifier; `path` points to the cdylib (.dll/.so/.dylib).
    pub fn load_native(&mut self, id: &str, path: &Path) -> Result<(), String> {
        // dlopen + ABI version check + function pointers
        let handle = unsafe { orbit_plugin_native::NativePluginHandle::load(path) }?;

        // info() -> capability declaration + schema
        let info_json = handle.info();
        let info: orbit_plugin_api_native::NativePluginInfo = serde_json::from_str(&info_json)
            .map_err(|e| format!("failed to parse native info: {}", e))?;
        if info.kind != "protocol" {
            return Err(format!(
                "native plugin '{}' should have type protocol, got {}",
                info.name, info.kind
            ));
        }

        let handle = Arc::new(handle);
        let mut protocol_ids = Vec::new();
        for cap in &info.capabilities {
            protocol_ids.push(cap.protocol_id.clone());
            // Build and cache the NativeProtocolClient
            let client = Arc::new(native::NativeProtocolClient {
                protocol_id: cap.protocol_id.clone(),
                display_name: cap.display_name.clone(),
                handle: handle.clone(),
            });
            // Register into the dynamic protocol registry (so the engine/frontend can get a client by protocol id)
            let c_for_factory = client.clone();
            let factory: orbit_protocol::registry::ProtocolFactory = Arc::new(move || {
                // NativeProtocolClient implements clone_client (returns Box<dyn ProtocolClient>)
                orbit_protocol::traits::ProtocolClient::clone_client(c_for_factory.as_ref())
            });
            orbit_protocol::registry::register_protocol(&cap.protocol_id, factory)
                .map_err(|e| format!("register protocol {}: {}", cap.protocol_id, e))?;
        }

        self.native_handles.insert(id.to_string(), handle);
        self.plugins.insert(
            id.to_string(),
            PluginDescriptor {
                id: id.to_string(),
                name: info.name.clone(),
                version: info.version.clone(),
                description: info.description.clone(),
                kind: "native-protocol".into(),
                status: "enabled".into(),
                error: None,
                protocols: protocol_ids,
                codecs: Vec::new(),
                connection_config_schema: info.connection_config_schema,
                request_config_schema: info.request_config_schema,
                installed_dir: Some(path.display().to_string()),
            },
        );
        Ok(())
    }

    /// Load a WASM component: detect world (protocol/codec) -> get-capabilities -> register in the dynamic table.
    /// Returns (kind, list of registered capability ids).
    ///
    /// `schemas`: JSON Schema from the manifest (connection/request), used for frontend dynamic forms; optional.
    pub async fn load_wasm(
        &mut self,
        id: &str,
        bytes: &[u8],
        schemas: Option<(&serde_json::Value, &serde_json::Value)>,
    ) -> Result<(String, Vec<String>), String> {
        let component = wasm::protocol::load_component(&self.engine, bytes)?;

        // Try the protocol world first (synchronous interface); keep the error for diagnostics
        let mut proto_err: Option<String> = None;
        match wasm::protocol::instantiate(&self.engine, &component) {
            Ok((store, instance)) => {
                let mut store = store;
                let caps = instance
                    .orbit_protocol_plugin_protocol()
                    .call_get_capabilities(&mut store)
                    .map_err(|e| format!("protocol get-capabilities: {}", e))?;
                if !caps.is_empty() {
                    let shared = Arc::new(Mutex::new(ProtocolPluginShared { store, instance }));
                    let mut ids = Vec::new();
                    for cap in &caps {
                        let pid = cap.protocol_id.clone();
                        let display = cap.display_name.clone();
                        let streaming = cap.supports_streaming;
                        let shared = shared.clone();
                        let factory: orbit_protocol::registry::ProtocolFactory =
                            Arc::new(move || {
                                Box::new(WasmProtocolClient {
                                    protocol_id: pid.clone(),
                                    display_name: display.clone(),
                                    streaming,
                                    shared: shared.clone(),
                                })
                                    as Box<dyn orbit_protocol::traits::ProtocolClient>
                            });
                        orbit_protocol::registry::register_protocol(&cap.protocol_id, factory)
                            .map_err(|e| format!("register {}: {}", cap.protocol_id, e))?;
                        ids.push(cap.protocol_id.clone());
                    }
                    self.protocol_handles.insert(id.to_string(), shared);
                    self.plugins.insert(
                        id.to_string(),
                        PluginDescriptor {
                            id: id.to_string(),
                            name: id.to_string(),
                            version: String::new(),
                            description: String::new(),
                            kind: "protocol".into(),
                            status: "enabled".into(),
                            error: None,
                            protocols: ids.clone(),
                            codecs: Vec::new(),
                            connection_config_schema: schemas
                                .map(|(c, _)| c.clone())
                                .or_else(|| Some(serde_json::json!({ "type": "object" }))),
                            request_config_schema: schemas
                                .map(|(_, r)| r.clone())
                                .or_else(|| Some(serde_json::json!({ "type": "object" }))),
                            installed_dir: None,
                        },
                    );
                    return Ok(("protocol".into(), ids));
                }
            }
            Err(e) => {
                proto_err = Some(e);
            }
        }

        // Otherwise try the codec world (synchronous)
        if let Ok((store, instance)) = wasm::codec::instantiate(&self.engine, &component) {
            let mut store = store;
            let caps = instance
                .orbit_codec_plugin_codec()
                .call_get_capabilities(&mut store)
                .map_err(|e| format!("codec get-capabilities: {}", e))?;
            if !caps.is_empty() {
                let shared = Arc::new(Mutex::new(CodecPluginShared { store, instance }));
                let mut ids = Vec::new();
                for cap in &caps {
                    let name = cap.codec_name.clone();
                    let display = cap.display_name.clone();
                    let shared = shared.clone();
                    let factory: orbit_codec::registry::CodecFactory = Arc::new(move || {
                        Box::new(WasmCodec {
                            codec_name: name.clone(),
                            display_name: display.clone(),
                            shared: shared.clone(),
                        }) as Box<dyn orbit_codec::traits::Codec>
                    });
                    orbit_codec::registry::register_codec(&cap.codec_name, factory)
                        .map_err(|e| format!("register {}: {}", cap.codec_name, e))?;
                    ids.push(cap.codec_name.clone());
                }
                self.codec_handles.insert(id.to_string(), shared);
                self.plugins.insert(
                    id.to_string(),
                    PluginDescriptor {
                        id: id.to_string(),
                        name: id.to_string(),
                        version: String::new(),
                        description: String::new(),
                        kind: "codec".into(),
                        status: "enabled".into(),
                        error: None,
                        protocols: Vec::new(),
                        codecs: ids.clone(),
                        connection_config_schema: None,
                        request_config_schema: None,
                        installed_dir: None,
                    },
                );
                return Ok(("codec".into(), ids));
            }
        }

        Err(format!(
            "plugin '{}': component implements neither the protocol-plugin nor the codec-plugin world (or declares no capabilities); protocol instantiate: {:?}",
            id,
            proto_err
        ))
    }

    /// Scan the plugin directory (<root>/<id>/manifest.json + entry wasm) and load each one.
    /// Failures are marked with error status and skipped (fail-isolated), without interrupting other plugins.
    pub async fn scan_dir(&mut self, root: &Path) -> ScanReport {
        let mut report = ScanReport {
            scanned: Vec::new(),
            loaded: Vec::new(),
            failed: Vec::new(),
        };
        let Ok(entries) = std::fs::read_dir(root) else {
            return report;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(manifest) = PluginManifest::load(&dir) else {
                continue;
            };
            if !manifest.is_valid() {
                report
                    .failed
                    .push((manifest.id.clone(), "invalid manifest".into()));
                continue;
            }
            report.scanned.push(manifest.id.clone());
            match self.load_from_dir(&manifest, &dir).await {
                Ok(_) => report.loaded.push(manifest.id.clone()),
                Err(e) => report.failed.push((manifest.id.clone(), e)),
            }
        }
        report
    }

    /// Unload a plugin: unregister dynamic protocol/format + destroy the instance
    pub fn unload(&mut self, id: &str) -> Result<(), String> {
        let desc = self
            .plugins
            .get(id)
            .ok_or_else(|| format!("plugin '{}' not loaded", id))?;
        for p in &desc.protocols {
            orbit_protocol::registry::unregister_protocol(p);
        }
        for c in &desc.codecs {
            orbit_codec::registry::unregister_codec(c);
        }
        self.protocol_handles.remove(id);
        self.codec_handles.remove(id);
        // native protocol plugin: unregister its protocol
        for pid in &desc.protocols {
            orbit_protocol::registry::unregister_protocol(pid);
        }
        self.native_handles.remove(id);
        self.plugins.remove(id);
        Ok(())
    }

    /// Load and register from the plugin directory (reused by install_zip / enable / scan_dir).
    /// Returns (kind, list of capability ids); on failure clears any leftover in-memory registrations and sets error status.
    async fn load_from_dir(
        &mut self,
        manifest: &PluginManifest,
        dir: &Path,
    ) -> Result<(String, Vec<String>), String> {
        let wasm_path = dir.join(&manifest.entry);
        let bytes =
            std::fs::read(&wasm_path).map_err(|e| format!("failed to read entry wasm: {}", e))?;
        let schemas = match (
            &manifest.connection_config_schema,
            &manifest.request_config_schema,
        ) {
            (Some(c), Some(r)) => Some((c, r)),
            (Some(c), None) => Some((c, &serde_json::json!({ "type": "object" }))),
            _ => None,
        };
        match self.load_wasm(&manifest.id, &bytes, schemas).await {
            Ok(res) => {
                if let Some(d) = self.plugins.get_mut(&manifest.id) {
                    d.installed_dir = Some(dir.display().to_string());
                }
                Ok(res)
            }
            Err(e) => {
                self.plugins.insert(
                    manifest.id.clone(),
                    PluginDescriptor {
                        id: manifest.id.clone(),
                        name: manifest.name.clone(),
                        version: manifest.version.clone(),
                        description: manifest.description.clone(),
                        kind: manifest.kind.clone(),
                        status: "error".into(),
                        error: Some(e.clone()),
                        protocols: Vec::new(),
                        codecs: Vec::new(),
                        connection_config_schema: manifest.connection_config_schema.clone(),
                        request_config_schema: manifest.request_config_schema.clone(),
                        installed_dir: Some(dir.display().to_string()),
                    },
                );
                Err(e)
            }
        }
    }

    /// Install a zip plugin package: validate + zip-slip-safe extraction to `<root>/<id>` -> load and register.
    /// Returns (kind, list of capability ids).
    pub async fn install_zip(
        &mut self,
        root: &Path,
        bytes: &[u8],
    ) -> Result<(String, Vec<String>), String> {
        let (id, manifest) = zip::install_zip(root, bytes)?;
        let dir = root.join(&id);
        self.load_from_dir(&manifest, &dir).await
    }

    /// Uninstall a plugin: unregister + destroy the instance + delete the install directory (if present).
    pub fn uninstall(&mut self, root: &Path, id: &str) -> Result<(), String> {
        self.unload(id)?;
        let dir = root.join(id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| format!("delete plugin directory '{}': {}", dir.display(), e))?;
        }
        Ok(())
    }

    /// Disable a plugin: unregister capabilities + destroy the instance; directory and descriptor are kept (status=disabled).
    pub fn disable(&mut self, id: &str) -> Result<(), String> {
        let desc = self
            .plugins
            .get(id)
            .ok_or_else(|| format!("plugin '{}' not loaded", id))?;
        if desc.status == "disabled" {
            return Ok(());
        }
        for p in &desc.protocols {
            orbit_protocol::registry::unregister_protocol(p);
        }
        for c in &desc.codecs {
            orbit_codec::registry::unregister_codec(c);
        }
        self.protocol_handles.remove(id);
        self.codec_handles.remove(id);
        if let Some(d) = self.plugins.get_mut(id) {
            d.status = "disabled".into();
            d.protocols.clear();
            d.codecs.clear();
        }
        Ok(())
    }

    /// Enable a plugin: reload and register from the install directory (directory is kept).
    pub async fn enable(&mut self, id: &str) -> Result<(), String> {
        let dir = self
            .plugins
            .get(id)
            .and_then(|d| d.installed_dir.clone())
            .ok_or_else(|| format!("plugin '{}' has no install directory, cannot enable", id))?;
        let manifest = PluginManifest::load(Path::new(&dir))
            .ok_or_else(|| format!("plugin '{}' directory has no valid manifest.json", id))?;
        let _ = self.load_from_dir(&manifest, Path::new(&dir)).await?;
        if let Some(d) = self.plugins.get_mut(id) {
            d.status = "enabled".into();
            d.error = None;
        }
        Ok(())
    }

    pub fn list(&self) -> Vec<PluginDescriptor> {
        let mut v: Vec<PluginDescriptor> = self.plugins.values().cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    pub fn get(&self, id: &str) -> Option<&PluginDescriptor> {
        self.plugins.get(id)
    }

    /// Connection/message schema map of loaded plugin protocols (protocol id -> (connection, request)).
    /// Used by `/api/protocols` to assemble dynamic form config; covers wasm protocol plugins (protocol) and native protocol plugins (native-protocol).
    pub fn protocol_schemas(&self) -> Vec<(String, serde_json::Value, serde_json::Value)> {
        let mut out = Vec::new();
        for p in self.plugins.values() {
            if p.kind != "protocol" && p.kind != "native-protocol" {
                continue;
            }
            for pid in &p.protocols {
                out.push((
                    pid.clone(),
                    p.connection_config_schema
                        .clone()
                        .unwrap_or_else(|| serde_json::json!({ "type": "object" })),
                    p.request_config_schema
                        .clone()
                        .unwrap_or_else(|| serde_json::json!({ "type": "object" })),
                ));
            }
        }
        out
    }

    /// Format names of loaded codec plugins (kind=codec)
    pub fn codec_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for p in self.plugins.values() {
            if p.kind == "codec" {
                names.extend(p.codecs.iter().cloned());
            }
        }
        names.sort();
        names.dedup();
        names
    }
}
