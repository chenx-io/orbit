//! Plugin manifest.json reading and validation.
//!
//! Directory layout: `<plugins-root>/<plugin-id>/manifest.json + <entry>.wasm [+ assets/]`

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// "protocol" | "codec"
    #[serde(rename = "type")]
    pub kind: String,
    /// Entry wasm file name (relative to the plugin directory)
    pub entry: String,
    /// Capabilities declared by the plugin (protocol id / format name)
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    /// Protocol plugins: JSON Schema for the connection-parameter area of new collections (connectionConfigSchema)
    #[serde(default, rename = "connectionConfigSchema")]
    pub connection_config_schema: Option<Value>,
    /// Protocol plugins: JSON Schema for the parameter area of message nodes (requestConfigSchema)
    #[serde(default, rename = "requestConfigSchema")]
    pub request_config_schema: Option<Value>,
    /// Runtime engine version constraint (e.g. { "orbit": ">=0.9.0" })
    #[serde(default)]
    pub engines: Option<Value>,
    /// Commercial/paid flag (display and marketplace catalog only; license self-check is done by the plugin itself, the open-source library does not gate)
    #[serde(default)]
    pub commercial: Option<bool>,
    /// Pricing display info (display only, e.g. `{"price": 199, "currency": "CNY", "period": "year"}`)
    #[serde(default)]
    pub pricing: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub protocols: Vec<String>,
    #[serde(default)]
    pub codecs: Vec<String>,
    #[serde(default)]
    pub mime_types: Vec<String>,
}

impl PluginManifest {
    /// Read manifest.json from the plugin directory (returns None if absent)
    pub fn load(dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Basic validation: id allowlist + valid kind/entry
    pub fn is_valid(&self) -> bool {
        let id_ok = !self.id.is_empty()
            && self.id.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-' || c == '_'
            });
        id_ok
            && matches!(
                self.kind.as_str(),
                "protocol" | "codec" | "assertion" | "extractor" | "output"
            )
            && !self.entry.is_empty()
            && self.entry.ends_with(".wasm")
            && (self.kind != "protocol" || self.connection_config_schema.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &std::path::Path, manifest: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    }

    fn tmp() -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("orbit-plugin-test-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn test_load_and_validate_valid_protocol() {
        let dir = tmp();
        write(
            &dir,
            r#"{
                "id": "com.example.dubbo-client",
                "name": "Dubbo",
                "version": "1.2.0",
                "type": "protocol",
                "entry": "dubbo.wasm",
                "capabilities": { "protocols": ["dubbo"] },
                "connectionConfigSchema": { "type": "object", "properties": { "registry": { "type": "string" } } },
                "requestConfigSchema": { "type": "object", "properties": { "method": { "type": "string" } } },
                "engines": { "orbit": ">=0.9.0" },
                "author": "Team A",
                "license": "MIT"
            }"#,
        );
        let m = PluginManifest::load(&dir).unwrap();
        assert!(m.is_valid());
        assert_eq!(m.id, "com.example.dubbo-client");
        assert_eq!(m.kind, "protocol");
        assert!(m.connection_config_schema.is_some());
        assert!(m.request_config_schema.is_some());
        assert!(m.engines.is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_protocol_requires_connection_schema() {
        // protocol plugin without connectionConfigSchema -> validation fails
        let dir = tmp();
        write(
            &dir,
            r#"{
                "id": "com.example.bad",
                "name": "Bad",
                "version": "1.0.0",
                "type": "protocol",
                "entry": "bad.wasm"
            }"#,
        );
        let m = PluginManifest::load(&dir).unwrap();
        assert!(!m.is_valid());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_codec_need_no_connection_schema() {
        let dir = tmp();
        write(
            &dir,
            r#"{
                "id": "com.example.avro-codec",
                "name": "Avro",
                "version": "1.0.0",
                "type": "codec",
                "entry": "avro.wasm",
                "capabilities": { "codecs": ["avro"] }
            }"#,
        );
        let m = PluginManifest::load(&dir).unwrap();
        assert!(m.is_valid());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_commercial_and_pricing_fields_roundtrip() {
        let dir = tmp();
        write(
            &dir,
            r#"{
                "id": "com.example.premium",
                "name": "Premium Codec",
                "version": "1.0.0",
                "type": "codec",
                "entry": "premium.wasm",
                "capabilities": { "codecs": ["premium"] },
                "commercial": true,
                "pricing": { "price": 199, "currency": "CNY", "period": "year" }
            }"#,
        );
        let m = PluginManifest::load(&dir).unwrap();
        assert_eq!(m.commercial, Some(true));
        assert_eq!(m.pricing.as_ref().unwrap()["price"], 199);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_manifest_without_commercial_fields_is_backward_compatible() {
        let dir = tmp();
        write(
            &dir,
            r#"{
                "id": "com.example.free",
                "name": "Free Codec",
                "version": "1.0.0",
                "type": "codec",
                "entry": "free.wasm",
                "capabilities": { "codecs": ["free"] }
            }"#,
        );
        let m = PluginManifest::load(&dir).unwrap();
        assert_eq!(m.commercial, None);
        assert_eq!(m.pricing, None);
        assert!(m.is_valid());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_id_whitelist_rejects_bad_ids() {
        for bad in ["Bad/Id", "has space", "UPPER", "", "../evil"] {
            let dir = tmp();
            write(
                &dir,
                &format!(
                    r#"{{ "id": "{}", "name": "x", "version": "1.0.0", "type": "codec", "entry": "x.wasm" }}"#,
                    bad
                ),
            );
            let m = PluginManifest::load(&dir).unwrap();
            assert!(!m.is_valid(), "id should be rejected: {}", bad);
            std::fs::remove_dir_all(&dir).ok();
        }
    }
}
