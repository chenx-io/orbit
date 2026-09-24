//! Codec registry - builds the matching `Codec` by format name / MIME type.
//!
//! The engine resolves codecs here from a step's `payload_format`/`response_format`
//! or a response's `Content-Type`, enabling multi-data-format routing.
//!
//! Dynamic registry (M3 WASM plugins): third-party plugins can register a format name (e.g. "avro"),
//! integrated identically to built-in formats via `resolve_codec`; names conflicting with built-ins are rejected.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use crate::form::FormCodec;
use crate::json::{BinaryCodec, JsonCodec, YamlCodec};
use crate::msgpack::MsgPackCodec;
use crate::protobuf::ProtobufCodec;
use crate::traits::Codec;
use crate::xml::XmlCodec;

/// Dynamic codec factory (a constructor that clones from the registry)
pub type CodecFactory = Arc<dyn Fn() -> Box<dyn Codec> + Send + Sync>;

static DYNAMIC_CODECS: LazyLock<RwLock<HashMap<String, CodecFactory>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Resolve a codec by format name: built-in enum first, dynamic (plugin) table as fallback
pub fn resolve_codec(name: &str) -> Option<Box<dyn Codec>> {
    if let Some(kind) = codec_for_format(name) {
        return Some(build_codec(kind));
    }
    DYNAMIC_CODECS
        .read()
        .ok()?
        .get(&name.to_ascii_lowercase())
        .map(|f| (f)())
}

/// Register a dynamic format (rejects conflicts with built-in names; idempotently overwrites a same-named plugin format)
pub fn register_codec(name: &str, factory: CodecFactory) -> Result<(), String> {
    let key = name.to_ascii_lowercase();
    if codec_for_format(&key).is_some() {
        return Err(format!("codec '{}' conflicts with builtin", name));
    }
    DYNAMIC_CODECS
        .write()
        .map_err(|_| "registry lock poisoned".to_string())?
        .insert(key, factory);
    Ok(())
}

/// Unregister a dynamic format
pub fn unregister_codec(name: &str) {
    if let Ok(mut g) = DYNAMIC_CODECS.write() {
        g.remove(&name.to_ascii_lowercase());
    }
}

/// List all dynamic (plugin) format names
pub fn list_codec_names() -> Vec<String> {
    DYNAMIC_CODECS
        .read()
        .map(|g| {
            let mut v: Vec<String> = g.keys().cloned().collect();
            v.sort();
            v
        })
        .unwrap_or_default()
}

/// Built-in format names (aligned with CodecKind::as_str), for catalog/dropdown display.
pub fn builtin_codec_names() -> Vec<String> {
    [
        CodecKind::Json,
        CodecKind::Yaml,
        CodecKind::MsgPack,
        CodecKind::Protobuf,
        CodecKind::Xml,
        CodecKind::Form,
        CodecKind::Binary,
    ]
    .into_iter()
    .map(|k| k.as_str().to_string())
    .collect()
}

/// Built-in data formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodecKind {
    Json,
    Yaml,
    MsgPack,
    Protobuf,
    Xml,
    Form,
    Binary,
}

impl CodecKind {
    /// Format name (matches `payload_format`/`response_format` in configuration)
    pub fn as_str(self) -> &'static str {
        match self {
            CodecKind::Json => "json",
            CodecKind::Yaml => "yaml",
            CodecKind::MsgPack => "msgpack",
            CodecKind::Protobuf => "protobuf",
            CodecKind::Xml => "xml",
            CodecKind::Form => "form",
            CodecKind::Binary => "binary",
        }
    }

    /// Parse from a format name (case-insensitive)
    pub fn parse(name: &str) -> Option<CodecKind> {
        match name.to_ascii_lowercase().as_str() {
            "json" | "application/json" => Some(CodecKind::Json),
            "yaml" | "yml" => Some(CodecKind::Yaml),
            "msgpack" | "messagepack" => Some(CodecKind::MsgPack),
            "protobuf" | "proto" => Some(CodecKind::Protobuf),
            "xml" => Some(CodecKind::Xml),
            "form" | "form-urlencoded" | "urlencoded" => Some(CodecKind::Form),
            "binary" | "bin" | "octet-stream" => Some(CodecKind::Binary),
            _ => None,
        }
    }
}

/// Build a codec from a format
pub fn build_codec(kind: CodecKind) -> Box<dyn Codec> {
    match kind {
        CodecKind::Json => Box::new(JsonCodec),
        CodecKind::Yaml => Box::new(YamlCodec),
        CodecKind::MsgPack => Box::new(MsgPackCodec),
        CodecKind::Protobuf => Box::new(ProtobufCodec::new()),
        CodecKind::Xml => Box::new(XmlCodec),
        CodecKind::Form => Box::new(FormCodec::new()),
        CodecKind::Binary => Box::new(BinaryCodec),
    }
}

/// Infer a format from a MIME type (only recognizes common built-in MIME types)
pub fn codec_for_mime(mime: &str) -> Option<CodecKind> {
    let mime = mime.to_ascii_lowercase();
    match mime.as_str() {
        "application/json" | "text/json" | "application/json; charset=utf-8" => {
            Some(CodecKind::Json)
        }
        "application/yaml" | "text/yaml" | "text/x-yaml" => Some(CodecKind::Yaml),
        "application/msgpack" | "application/x-msgpack" | "application/messagepack" => {
            Some(CodecKind::MsgPack)
        }
        "application/protobuf" | "application/x-protobuf" | "application/grpc+proto" => {
            Some(CodecKind::Protobuf)
        }
        "application/xml" | "text/xml" => Some(CodecKind::Xml),
        "application/x-www-form-urlencoded" => Some(CodecKind::Form),
        "application/octet-stream" => Some(CodecKind::Binary),
        // text/* and unknown types are not structurally decoded (keep the default JSON attempt / raw bytes)
        _ => None,
    }
}

/// Resolve by format name (`payload_format`/`response_format` configuration)
pub fn codec_for_format(format: &str) -> Option<CodecKind> {
    CodecKind::parse(format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DataValue;

    #[test]
    fn test_parse_formats() {
        assert_eq!(CodecKind::parse("json"), Some(CodecKind::Json));
        assert_eq!(CodecKind::parse("MSGPACK"), Some(CodecKind::MsgPack));
        assert_eq!(CodecKind::parse("proto"), Some(CodecKind::Protobuf));
        assert_eq!(CodecKind::parse("nope"), None);
    }

    #[test]
    fn test_mime_lookup() {
        assert_eq!(codec_for_mime("application/json"), Some(CodecKind::Json));
        assert_eq!(codec_for_mime("application/xml; charset=utf-8"), None); // with parameters it is not recognized; the caller must split them off first
        assert_eq!(
            codec_for_mime("application/grpc+proto"),
            Some(CodecKind::Protobuf)
        );
        assert_eq!(codec_for_mime("text/plain"), None);
    }

    #[test]
    fn test_build_codec_roundtrip() {
        for kind in [CodecKind::Json, CodecKind::MsgPack, CodecKind::Yaml] {
            let codec = build_codec(kind);
            let value = DataValue::String("hello".into());
            let bytes = codec.encode(&value).unwrap();
            let decoded = codec.decode(&bytes).unwrap();
            assert_eq!(decoded, value, "roundtrip failed for {}", kind.as_str());
        }
    }

    #[test]
    fn test_dynamic_codec_register_and_conflict() {
        let factory: CodecFactory = Arc::new(|| Box::new(crate::json::JsonCodec));

        // Conflicts with a built-in name → rejected (M3 acceptance item)
        assert!(register_codec("json", factory.clone()).is_err());
        assert!(register_codec("MSGPACK", factory.clone()).is_err());

        // Dynamic registration succeeds (case-insensitive key) and can be resolved by name
        assert!(register_codec("Avro", factory.clone()).is_ok());
        let codec = resolve_codec("avro").expect("dynamic codec");
        let value = DataValue::String("hi".into());
        let bytes = codec.encode(&value).unwrap();
        assert_eq!(codec.decode(&bytes).unwrap(), value);
        assert_eq!(list_codec_names(), vec!["avro"]);

        // Not resolvable after unregistering
        unregister_codec("avro");
        assert!(resolve_codec("avro").is_none());
        assert!(list_codec_names().is_empty());
    }
}
