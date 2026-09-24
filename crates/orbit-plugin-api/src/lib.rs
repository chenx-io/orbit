//! # orbit-plugin-api
//!
//! Plugin SDK - for third-party developers writing Orbit WASM plugins.
//!
//! Plugin types:
//! - Protocol Plugin: custom network protocols
//! - Codec Plugin: custom data formats
//! - Assertion Plugin: custom assertions
//! - Extractor Plugin: custom extractors
//! - Output Plugin: custom output export

/// Plugin info
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin name
    pub name: String,
    /// Version number
    pub version: String,
    /// Plugin type
    pub plugin_type: PluginType,
    /// Description
    pub description: String,
    /// Author
    pub author: String,
}

/// Plugin type
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginType {
    Protocol,
    Codec,
    Assertion,
    Extractor,
    Output,
}

impl PluginType {
    /// String aligned with the manifest `type` field
    pub fn as_str(&self) -> &'static str {
        match self {
            PluginType::Protocol => "protocol",
            PluginType::Codec => "codec",
            PluginType::Assertion => "assertion",
            PluginType::Extractor => "extractor",
            PluginType::Output => "output",
        }
    }

    /// Parse from the manifest `type` string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "protocol" => Some(PluginType::Protocol),
            "codec" => Some(PluginType::Codec),
            "assertion" => Some(PluginType::Assertion),
            "extractor" => Some(PluginType::Extractor),
            "output" => Some(PluginType::Output),
            _ => None,
        }
    }
}

/// A single capability declaration.
///
/// The plugin's `capabilities()` returns the list of capabilities it provides; the host writes them to the global registry:
/// - `kind == Protocol`: `name` is the protocol id (e.g. `"dubbo"`), written to the orbit-protocol registry
/// - `kind == Codec`: `name` is the format name (e.g. `"avro"`), written to the orbit-codec registry
/// - Phase 2 (Assertion/Extractor/Output): `name` is the capability identifier, written to the corresponding registry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginCapability {
    pub kind: PluginType,
    /// Protocol id / format name / capability identifier
    pub name: String,
    /// Extra metadata (e.g. protocol display-name, mime-types, display name)
    #[serde(default)]
    pub meta: serde_json::Value,
}

impl PluginCapability {
    pub fn new(kind: PluginType, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            meta: serde_json::Value::Null,
        }
    }
}

/// Plugin registry
pub trait Plugin: Send + Sync {
    fn info(&self) -> PluginInfo;

    /// The capability list the plugin provides. Empty by default.
    fn capabilities(&self) -> Vec<PluginCapability> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_type_roundtrip() {
        for (t, s) in [
            (PluginType::Protocol, "protocol"),
            (PluginType::Codec, "codec"),
            (PluginType::Assertion, "assertion"),
            (PluginType::Extractor, "extractor"),
            (PluginType::Output, "output"),
        ] {
            assert_eq!(t.as_str(), s);
            assert_eq!(PluginType::from_str(s), Some(t));
        }
        assert_eq!(PluginType::from_str("unknown"), None);
    }

    #[test]
    fn capability_default_meta_is_null() {
        let c = PluginCapability::new(PluginType::Protocol, "dubbo");
        assert_eq!(c.kind, PluginType::Protocol);
        assert_eq!(c.name, "dubbo");
        assert_eq!(c.meta, serde_json::Value::Null);
    }

    struct Stub;

    impl Plugin for Stub {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: "stub".into(),
                version: "1.0.0".into(),
                plugin_type: PluginType::Codec,
                description: String::new(),
                author: String::new(),
            }
        }
        fn capabilities(&self) -> Vec<PluginCapability> {
            vec![PluginCapability::new(PluginType::Codec, "tsv")]
        }
    }

    #[test]
    fn plugin_capabilities_override_default() {
        let p = Stub;
        assert_eq!(p.capabilities().len(), 1);
        assert_eq!(p.capabilities()[0].name, "tsv");
    }
}
