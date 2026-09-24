//! Protobuf codec
//!
//! Built on the official prost ecosystem (prost + prost-reflect):
//! 1. **No-descriptor mode (default)**: JSON-compatible codec (encode emits JSON bytes; decode attempts JSON parsing)
//! 2. **Descriptor mode**: true dynamic protobuf encoding/decoding
//!    - Builds a `DescriptorPool` from the `FileDescriptorProto` returned by gRPC reflection
//!    - Converts between JSON and protobuf binary (protojson semantics)

use prost::Message as _;
use prost_reflect::{DescriptorPool, DynamicMessage};
use prost_types::FileDescriptorProto;
use serde::de::DeserializeSeed;

use crate::traits::Codec;
use crate::types::{CodecError, DataValue};

/// Dynamic protobuf codec (prost-reflect).
///
/// Builds a `DescriptorPool` from a set of `FileDescriptorProto` (gRPC reflection output),
/// providing dynamic JSON ↔ protobuf binary conversion. Multiple messages/services share one pool.
#[derive(Debug, Clone)]
pub struct DynamicProto {
    pool: DescriptorPool,
}

impl DynamicProto {
    /// Build a descriptor pool from a list of serialized FileDescriptorProto bytes
    ///
    /// Deduplicates same-named files (`FileDescriptorProto.name`): later additions override earlier ones.
    /// Scenario: reflection import and local proto import may carry a same-named `accept.proto` (with different bytes);
    /// after the frontend deletes a package and re-imports, both old and new descriptors may be retained; without dedup,
    /// DescriptorPool reports "a different file named 'xxx.proto' has already been added".
    pub fn from_file_descriptors(protos: &[Vec<u8>]) -> Result<Self, CodecError> {
        let mut pool = DescriptorPool::new();
        let mut files = Vec::with_capacity(protos.len());
        // file name → index into files; same-named files are overwritten by the later import
        let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for bytes in protos {
            let fd = FileDescriptorProto::decode(bytes.as_slice()).map_err(|e| {
                CodecError::Decode(format!("failed to parse FileDescriptorProto: {e}"))
            })?;
            let name = fd.name.clone().unwrap_or_default();
            match index.get(&name) {
                Some(&i) => files[i] = fd,
                None => {
                    index.insert(name, files.len());
                    files.push(fd);
                }
            }
        }
        // File dependencies within a batch may be out of order: the pool parses all files at once
        pool.add_file_descriptor_protos(files)
            .map_err(|e| CodecError::Decode(format!("failed to build DescriptorPool: {e}")))?;
        Ok(Self { pool })
    }

    /// JSON → protobuf binary (`message` is a fully-qualified name, e.g. `pkg.Message`, with an optional leading dot)
    pub fn encode_json(&self, message: &str, json: &[u8]) -> Result<Vec<u8>, CodecError> {
        let name = message.trim_start_matches('.');
        let md = self.pool.get_message_by_name(name).ok_or_else(|| {
            CodecError::Encode(format!("message {name} not found in descriptor pool"))
        })?;
        let mut de = serde_json::Deserializer::from_slice(json);
        let msg = md
            .deserialize(&mut de)
            .map_err(|e| CodecError::Encode(format!("JSON→proto conversion failed: {e}")))?;
        de.end()
            .map_err(|e| CodecError::Encode(format!("failed to parse trailing JSON: {e}")))?;
        Ok(msg.encode_to_vec())
    }

    /// protobuf binary → JSON
    pub fn decode_to_json(
        &self,
        message: &str,
        bytes: &[u8],
    ) -> Result<serde_json::Value, CodecError> {
        let name = message.trim_start_matches('.');
        let md = self.pool.get_message_by_name(name).ok_or_else(|| {
            CodecError::Decode(format!("message {name} not found in descriptor pool"))
        })?;
        let msg = DynamicMessage::decode(md, bytes)
            .map_err(|e| CodecError::Decode(format!("proto decode failed: {e}")))?;
        serde_json::to_value(&msg)
            .map_err(|e| CodecError::Decode(format!("proto→JSON conversion failed: {e}")))
    }

    /// Generate a JSON example template for a message type (derives default values from descriptor field types),
    /// used by the frontend Message editor as an editable request-body template.
    ///
    /// Scalar fields take the type default (string:"", numeric:0, bool:false),
    /// message fields expand recursively into objects, repeated expands into a single-element array, and map expands into a single-key object.
    pub fn message_template(&self, message: &str) -> Result<serde_json::Value, CodecError> {
        let name = message.trim_start_matches('.');
        let md = self.pool.get_message_by_name(name).ok_or_else(|| {
            CodecError::Encode(format!("message {name} not found in descriptor pool"))
        })?;
        let value = build_message_template(&md);
        serde_json::to_value(value)
            .map_err(|e| CodecError::Encode(format!("template serialization failed: {e}")))
    }

    /// Generate the schema for a message type (aligned with the `{ properties }` structure of an HTTP response schema),
    /// shown in the frontend "view model definition" dialog.
    ///
    /// Returns something like `{ properties: { fieldName: { type, required?, example?, properties?, items? } } }`,
    /// with nested messages expanded recursively. Scalar fields carry a type and example default; repeated/map are marked as array/map.
    pub fn message_schema(&self, message: &str) -> Result<serde_json::Value, CodecError> {
        let name = message.trim_start_matches('.');
        let md = self.pool.get_message_by_name(name).ok_or_else(|| {
            CodecError::Encode(format!("message {name} not found in descriptor pool"))
        })?;
        // `visited` records the full names of messages on the current expansion chain, to intercept recursive (self-referential) messages and avoid infinite recursion
        let mut visited = std::collections::HashSet::new();
        visited.insert(md.full_name().to_string());
        let mut properties = serde_json::Map::new();
        for field in md.fields() {
            properties.insert(
                field.json_name().to_string(),
                serde_json::Value::Object(field_schema(&field, &mut visited)),
            );
        }
        Ok(serde_json::Value::Object(serde_json::Map::from_iter([(
            "properties".to_string(),
            serde_json::Value::Object(properties),
        )])))
    }
}

/// Schema of a single field: `{ type, example?, properties? | items? | additionalProperties? }`.
/// Message fields carry nested structure per singular / repeated / map, for the frontend to generate random request bodies and display model definitions.
fn field_schema(
    field: &prost_reflect::FieldDescriptor,
    visited: &mut std::collections::HashSet<String>,
) -> serde_json::Map<String, serde_json::Value> {
    use prost_reflect::Kind;
    let mut prop = serde_json::Map::new();
    prop.insert(
        "type".to_string(),
        serde_json::Value::String(field_type_str(field)),
    );

    if field.is_list() {
        if let Kind::Message(m) = field.kind() {
            prop.insert("items".to_string(), message_fields_schema(&m, visited));
        }
    } else if field.is_map() {
        if let Kind::Message(entry) = field.kind() {
            let val_field = entry.map_entry_value_field();
            if let Kind::Message(vm) = val_field.kind() {
                prop.insert(
                    "additionalProperties".to_string(),
                    message_fields_schema(&vm, visited),
                );
            } else {
                prop.insert(
                    "additionalProperties".to_string(),
                    serde_json::Value::String(field_type_str(&val_field)),
                );
            }
        }
    } else if let Kind::Message(m) = field.kind() {
        prop.insert("properties".to_string(), message_fields_schema(&m, visited));
    }

    if let Some(ex) = field_example_json(field) {
        prop.insert("example".to_string(), ex);
    }
    prop
}

/// Recursively expand message fields into `{ fieldName: { type, properties? } }`
/// `visited` is the set of message full names on the current expansion chain; expansion stops at a message already on the chain (recursion),
/// emitting empty properties as a placeholder to prevent self-referential messages from causing infinite recursion / stack overflow.
fn message_fields_schema(
    md: &prost_reflect::MessageDescriptor,
    visited: &mut std::collections::HashSet<String>,
) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let full = md.full_name().to_string();
    if visited.contains(&full) {
        // recursive reference: emit an empty object to avoid an infinite loop
        return serde_json::Value::Object(properties);
    }
    visited.insert(full.clone());
    for field in md.fields() {
        properties.insert(
            field.json_name().to_string(),
            serde_json::Value::Object(field_schema(&field, visited)),
        );
    }
    visited.remove(&full);
    serde_json::Value::Object(properties)
}

/// Compute a field's JSON type name (scalar / array / map / nested message / enum)
fn field_type_str(field: &prost_reflect::FieldDescriptor) -> String {
    use prost_reflect::Kind;
    let base = match field.kind() {
        Kind::Double | Kind::Float => "number",
        Kind::Int32
        | Kind::Int64
        | Kind::Uint32
        | Kind::Uint64
        | Kind::Sint32
        | Kind::Sint64
        | Kind::Fixed32
        | Kind::Fixed64
        | Kind::Sfixed32
        | Kind::Sfixed64 => "integer",
        Kind::Bool => "boolean",
        Kind::String => "string",
        Kind::Bytes => "bytes",
        Kind::Message(_) => "object",
        Kind::Enum(_) => "enum",
    };
    if field.is_list() {
        format!("{}[]", base)
    } else if field.is_map() {
        format!("map<string,{}>", base)
    } else {
        base.to_string()
    }
}

/// Compute a field's example default value (scalar default / empty object for a nested message / first enum value)
fn field_example_json(field: &prost_reflect::FieldDescriptor) -> Option<serde_json::Value> {
    use prost_reflect::Kind;
    if field.is_list() || field.is_map() {
        return None;
    }
    let ex = match field.kind() {
        Kind::Double | Kind::Float => serde_json::Value::Number(serde_json::Number::from(0)),
        Kind::Int32
        | Kind::Int64
        | Kind::Uint32
        | Kind::Uint64
        | Kind::Sint32
        | Kind::Sint64
        | Kind::Fixed32
        | Kind::Fixed64
        | Kind::Sfixed32
        | Kind::Sfixed64 => serde_json::Value::Number(serde_json::Number::from(0)),
        Kind::Bool => serde_json::Value::Bool(false),
        Kind::String | Kind::Bytes => serde_json::Value::String(String::new()),
        Kind::Message(_) => serde_json::Value::Object(serde_json::Map::new()),
        Kind::Enum(e) => serde_json::Value::String(
            e.values()
                .next()
                .map(|v| v.name().to_string())
                .unwrap_or_default(),
        ),
    };
    Some(ex)
}

fn build_message_template(
    md: &prost_reflect::MessageDescriptor,
) -> serde_json::Map<String, serde_json::Value> {
    let mut visited = std::collections::HashSet::new();
    build_message_template_visited(md, &mut visited)
}

/// Template building with recursion interception: `visited` records the full names of messages on the expansion chain,
/// and self-referential messages are replaced with an empty object to avoid infinite recursion / stack overflow.
fn build_message_template_visited(
    md: &prost_reflect::MessageDescriptor,
    visited: &mut std::collections::HashSet<String>,
) -> serde_json::Map<String, serde_json::Value> {
    use prost_reflect::Kind;
    let mut obj = serde_json::Map::new();
    let full = md.full_name().to_string();
    if visited.contains(&full) {
        return obj;
    }
    visited.insert(full.clone());
    for field in md.fields() {
        // skip the map_entry type (map fields are handled in the is_map branch)
        if field.is_map() {
            if let Kind::Message(entry) = field.kind() {
                let key_tpl = scalar_default(entry.map_entry_key_field().kind());
                let val_tpl = field_value_template(&entry.map_entry_value_field(), visited);
                // build a single-key map object { "<example key>": <value template> }
                let key_str = match &key_tpl {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => "key".to_string(),
                };
                let mut m = serde_json::Map::new();
                m.insert(key_str, val_tpl);
                obj.insert(field.name().to_string(), serde_json::Value::Object(m));
            }
            continue;
        }
        let tpl = field_value_template(&field, visited);
        let v = if field.is_list() {
            serde_json::Value::Array(vec![tpl])
        } else {
            tpl
        };
        obj.insert(field.name().to_string(), v);
    }
    visited.remove(&full);
    obj
}

/// Compute the template value for a single field (scalar default / message recursion / first enum value)
fn field_value_template(
    field: &prost_reflect::FieldDescriptor,
    visited: &mut std::collections::HashSet<String>,
) -> serde_json::Value {
    use prost_reflect::Kind;
    match field.kind() {
        Kind::Message(m) => serde_json::Value::Object(build_message_template_visited(&m, visited)),
        Kind::Enum(e) => e
            .values()
            .next()
            .map(|v| serde_json::Value::String(v.name().to_string()))
            .unwrap_or(serde_json::Value::String(String::new())),
        scalar => scalar_default(scalar),
    }
}

/// Default value template for scalar types
fn scalar_default(kind: prost_reflect::Kind) -> serde_json::Value {
    use prost_reflect::Kind;
    match kind {
        Kind::Double
        | Kind::Float
        | Kind::Int32
        | Kind::Int64
        | Kind::Uint32
        | Kind::Uint64
        | Kind::Sint32
        | Kind::Sint64
        | Kind::Fixed32
        | Kind::Fixed64
        | Kind::Sfixed32
        | Kind::Sfixed64 => serde_json::Value::Number(serde_json::Number::from(0)),
        Kind::Bool => serde_json::Value::Bool(false),
        Kind::String => serde_json::Value::String(String::new()),
        Kind::Bytes => serde_json::Value::String(String::new()),
        Kind::Message(m) => serde_json::Value::Object(build_message_template(&m)),
        Kind::Enum(e) => e
            .values()
            .next()
            .map(|v| serde_json::Value::String(v.name().to_string()))
            .unwrap_or(serde_json::Value::String(String::new())),
    }
}

/// Protobuf codec
///
/// Uses JSON-compatible mode by default; once a `DynamicProto` (descriptor pool) is configured,
/// it performs true protobuf binary encoding/decoding.
pub struct ProtobufCodec {
    /// Optional dynamic descriptor pool (obtained from gRPC reflection)
    proto: Option<DynamicProto>,
    /// Fully-qualified name of the message type (e.g. "pkg.Service.RequestMessage")
    message_name: Option<String>,
}

impl ProtobufCodec {
    /// Create a codec without a descriptor (JSON-compatible mode)
    pub fn new() -> Self {
        Self {
            proto: None,
            message_name: None,
        }
    }

    /// Create a codec using a dynamic descriptor pool (true protobuf encoding/decoding)
    pub fn with_dynamic(proto: DynamicProto, message_name: &str) -> Self {
        Self {
            proto: Some(proto),
            message_name: Some(message_name.to_string()),
        }
    }

    /// Create a codec from a single serialized FileDescriptorProto
    pub fn with_descriptor(
        descriptor_bytes: Vec<u8>,
        message_name: &str,
    ) -> Result<Self, CodecError> {
        let proto = DynamicProto::from_file_descriptors(&[descriptor_bytes])?;
        Ok(Self::with_dynamic(proto, message_name))
    }

    /// Whether a descriptor is configured
    pub fn has_descriptor(&self) -> bool {
        self.proto.is_some()
    }
}

impl Default for ProtobufCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Codec for ProtobufCodec {
    fn name(&self) -> &str {
        "protobuf"
    }

    fn mime_types(&self) -> Vec<&str> {
        vec![
            "application/protobuf",
            "application/x-protobuf",
            "application/grpc+proto",
        ]
    }

    fn encode(&self, value: &DataValue) -> Result<Vec<u8>, CodecError> {
        if let (Some(proto), Some(name)) = (&self.proto, &self.message_name) {
            let json = match value {
                DataValue::Bytes(b) => return Ok(b.clone()),
                DataValue::String(s) => s.as_bytes().to_vec(),
                other => {
                    serde_json::to_vec(other).map_err(|e| CodecError::Encode(e.to_string()))?
                }
            };
            return proto.encode_json(name, &json);
        }
        encode_json_compat(value)
    }

    fn decode(&self, bytes: &[u8]) -> Result<DataValue, CodecError> {
        if let (Some(proto), Some(name)) = (&self.proto, &self.message_name) {
            let json = proto.decode_to_json(name, bytes)?;
            return serde_json::from_value(json).map_err(|e| CodecError::Decode(e.to_string()));
        }
        decode_json_compat(bytes)
    }

    fn clone_codec(&self) -> Box<dyn Codec> {
        Box::new(Self {
            proto: self.proto.clone(),
            message_name: self.message_name.clone(),
        })
    }
}

// ── JSON-compatible mode ───────────────────────────────────────────

fn encode_json_compat(value: &DataValue) -> Result<Vec<u8>, CodecError> {
    match value {
        DataValue::Bytes(b) => Ok(b.clone()),
        DataValue::String(s) => Ok(s.as_bytes().to_vec()),
        _ => serde_json::to_vec(value).map_err(|e| CodecError::Encode(e.to_string())),
    }
}

fn decode_json_compat(bytes: &[u8]) -> Result<DataValue, CodecError> {
    // first, attempt JSON parsing
    match serde_json::from_slice::<DataValue>(bytes) {
        Ok(v) => Ok(v),
        Err(_) => {
            // JSON parsing failed; attempt to treat it as a UTF-8 string
            match std::str::from_utf8(bytes) {
                Ok(s)
                    if !s.is_empty()
                        && s.bytes()
                            .all(|b| b.is_ascii_graphic() || b.is_ascii_whitespace()) =>
                {
                    Ok(DataValue::String(s.to_string()))
                }
                _ => {
                    // return the raw bytes
                    Ok(DataValue::Bytes(bytes.to_vec()))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_protobuf_json_compat_encode_object() {
        let codec = ProtobufCodec::new();
        let mut map = HashMap::new();
        map.insert("name".into(), DataValue::String("test".into()));
        map.insert("value".into(), DataValue::Int(42));
        let value = DataValue::Object(map);

        let encoded = codec.encode(&value).unwrap();
        // should be valid JSON
        let decoded: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded["name"], "test");
        assert_eq!(decoded["value"], 42);
    }

    #[test]
    fn test_protobuf_json_compat_encode_string() {
        let codec = ProtobufCodec::new();
        let value = DataValue::String("hello protobuf".into());
        let encoded = codec.encode(&value).unwrap();
        assert_eq!(String::from_utf8_lossy(&encoded), "hello protobuf");
    }

    #[test]
    fn test_protobuf_json_compat_encode_bytes() {
        let codec = ProtobufCodec::new();
        let data = vec![8, 1, 18, 5, 104, 101, 108, 108, 111];
        let value = DataValue::Bytes(data.clone());
        let encoded = codec.encode(&value).unwrap();
        assert_eq!(encoded, data);
    }

    #[test]
    fn test_protobuf_json_compat_decode_json() {
        let codec = ProtobufCodec::new();
        let json_bytes = br#"{"status":"ok","code":200}"#;
        let decoded = codec.decode(json_bytes).unwrap();

        if let DataValue::Object(map) = decoded {
            assert_eq!(
                map.get("status").map(|v| match v {
                    DataValue::String(s) => s.clone(),
                    _ => "".into(),
                }),
                Some("ok".to_string())
            );
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_protobuf_json_compat_decode_binary() {
        let codec = ProtobufCodec::new();
        let binary_data = vec![0x00, 0x01, 0xFF, 0xFE];
        let decoded = codec.decode(&binary_data).unwrap();
        // not JSON and not pure ASCII text → return Bytes
        assert!(matches!(decoded, DataValue::Bytes(_)));
    }

    #[test]
    fn test_protobuf_json_compat_roundtrip() {
        let codec = ProtobufCodec::new();
        let mut map = HashMap::new();
        map.insert("id".into(), DataValue::Int(1));
        map.insert("name".into(), DataValue::String("orbit".into()));
        let value = DataValue::Object(map);

        let encoded = codec.encode(&value).unwrap();
        let decoded = codec.decode(&encoded).unwrap();

        if let DataValue::Object(obj) = decoded {
            assert!(obj.contains_key("id"));
            assert!(obj.contains_key("name"));
        } else {
            panic!("Expected Object after roundtrip");
        }
    }

    #[test]
    fn test_protobuf_codec_clone() {
        let codec = ProtobufCodec::new();
        let cloned = codec.clone_codec();
        assert_eq!(cloned.name(), "protobuf");
        assert_eq!(cloned.mime_types(), codec.mime_types());
    }

    /// Same-name file dedup: add two same-named demo.proto files (with different fields) in order,
    /// the pool should build successfully and adopt the later version (field id takes effect, the old field name does not exist).
    #[test]
    fn test_from_file_descriptors_dedup_same_name() {
        use prost_types::{
            field_descriptor_proto::{Label, Type},
            DescriptorProto, FieldDescriptorProto,
        };

        // file 1: demo.proto, message Greeting { string name = 1; }
        let f1 = FieldDescriptorProto {
            name: Some("name".into()),
            number: Some(1),
            r#type: Some(Type::String as i32),
            label: Some(Label::Optional as i32),
            ..Default::default()
        };
        let m1 = DescriptorProto {
            name: Some("Greeting".into()),
            field: vec![f1],
            ..Default::default()
        };
        let file1 = FileDescriptorProto {
            name: Some("demo.proto".into()),
            package: Some("demo".into()),
            message_type: vec![m1],
            ..Default::default()
        };

        // file 2: same-named demo.proto, message Greeting { int32 id = 1; } (different fields)
        let f2 = FieldDescriptorProto {
            name: Some("id".into()),
            number: Some(1),
            r#type: Some(Type::Int32 as i32),
            label: Some(Label::Optional as i32),
            ..Default::default()
        };
        let m2 = DescriptorProto {
            name: Some("Greeting".into()),
            field: vec![f2],
            ..Default::default()
        };
        let file2 = FileDescriptorProto {
            name: Some("demo.proto".into()),
            package: Some("demo".into()),
            message_type: vec![m2],
            ..Default::default()
        };

        let proto =
            DynamicProto::from_file_descriptors(&[file1.encode_to_vec(), file2.encode_to_vec()])
                .unwrap();
        let schema = proto.message_schema("demo.Greeting").unwrap();
        let props = schema["properties"].as_object().unwrap();
        assert_eq!(props["id"]["type"], "integer");
        assert!(!props.contains_key("name"));
    }

    /// Real dynamic codec roundtrip: programmatically build a demo.proto descriptor
    #[test]
    fn test_protobuf_dynamic_roundtrip() {
        use prost_types::{
            field_descriptor_proto::{Label, Type},
            DescriptorProto, FieldDescriptorProto,
        };

        let field = FieldDescriptorProto {
            name: Some("name".into()),
            number: Some(1),
            r#type: Some(Type::String as i32),
            label: Some(Label::Optional as i32),
            ..Default::default()
        };
        let msg = DescriptorProto {
            name: Some("Greeting".into()),
            field: vec![field],
            ..Default::default()
        };
        let file = FileDescriptorProto {
            name: Some("demo.proto".into()),
            package: Some("demo".into()),
            message_type: vec![msg],
            ..Default::default()
        };

        let proto = DynamicProto::from_file_descriptors(&[file.encode_to_vec()]).unwrap();
        let codec = ProtobufCodec::with_dynamic(proto, "demo.Greeting");
        assert!(codec.has_descriptor());

        let bytes = codec
            .encode(&DataValue::Object(
                [("name".to_string(), DataValue::String("hello".into()))]
                    .into_iter()
                    .collect(),
            ))
            .unwrap();
        let decoded = codec.decode(&bytes).unwrap();
        match decoded {
            DataValue::Object(m) => {
                assert_eq!(m.get("name"), Some(&DataValue::String("hello".into())));
            }
            other => panic!("expected Object, got {:?}", other),
        }
    }

    /// Recursive (self-referential) message: message_template and message_schema should terminate via cycle detection, without stack overflow.
    #[test]
    fn test_dynamic_recursive_message_no_stack_overflow() {
        use prost_types::{
            field_descriptor_proto::{Label, Type},
            DescriptorProto, FieldDescriptorProto,
        };
        // message Node { int32 value = 1; Node next = 2; }
        let value_field = FieldDescriptorProto {
            name: Some("value".into()),
            number: Some(1),
            r#type: Some(Type::Int32 as i32),
            label: Some(Label::Optional as i32),
            ..Default::default()
        };
        let next_field = FieldDescriptorProto {
            name: Some("next".into()),
            number: Some(2),
            r#type: Some(Type::Message as i32),
            r#type_name: Some(".demo.Node".into()),
            label: Some(Label::Optional as i32),
            ..Default::default()
        };
        let node = DescriptorProto {
            name: Some("Node".into()),
            field: vec![value_field, next_field],
            ..Default::default()
        };
        let file = FileDescriptorProto {
            name: Some("recursive.proto".into()),
            package: Some("demo".into()),
            message_type: vec![node],
            ..Default::default()
        };

        let proto = DynamicProto::from_file_descriptors(&[file.encode_to_vec()]).unwrap();

        // message_template: recursive references should be an empty-object placeholder, and the whole thing should return successfully
        let tpl = proto.message_template("demo.Node").unwrap();
        let tpl_obj = tpl.as_object().unwrap();
        assert!(tpl_obj.contains_key("value"));
        // next is a recursive reference → empty-object placeholder (rather than infinite expansion)
        assert!(tpl_obj.get("next").and_then(|v| v.as_object()).is_some());

        // message_schema: should likewise return successfully, with an empty-object placeholder for next's properties
        let schema = proto.message_schema("demo.Node").unwrap();
        let props = schema["properties"].as_object().unwrap();
        assert_eq!(props["value"]["type"], "integer");
        let next_props = props["next"]["properties"].as_object().unwrap();
        assert!(next_props.is_empty());
    }

    /// repeated recursion + map recursion: the schema should be finite and valid, and recursion into nested fields should not crash.
    #[test]
    fn test_dynamic_schema_repeated_and_map_recursive() {
        use prost_types::{
            field_descriptor_proto::{Label, Type},
            DescriptorProto, FieldDescriptorProto,
        };
        // message Item { string key = 1; }
        let key_field = FieldDescriptorProto {
            name: Some("key".into()),
            number: Some(1),
            r#type: Some(Type::String as i32),
            label: Some(Label::Optional as i32),
            ..Default::default()
        };
        let item = DescriptorProto {
            name: Some("Item".into()),
            field: vec![key_field],
            ..Default::default()
        };
        // message Node { int32 value=1; repeated Node children=2; map<string,Item> items=3; }
        let value_field = FieldDescriptorProto {
            name: Some("value".into()),
            number: Some(1),
            r#type: Some(Type::Int32 as i32),
            label: Some(Label::Optional as i32),
            ..Default::default()
        };
        let children_field = FieldDescriptorProto {
            name: Some("children".into()),
            number: Some(2),
            r#type: Some(Type::Message as i32),
            r#type_name: Some(".demo.Node".into()),
            label: Some(Label::Repeated as i32),
            ..Default::default()
        };
        // map<string,Item> → entry message
        let entry = DescriptorProto {
            name: Some("ItemsEntry".into()),
            options: Some(prost_types::MessageOptions {
                map_entry: Some(true),
                ..Default::default()
            }),
            field: vec![
                FieldDescriptorProto {
                    name: Some("key".into()),
                    number: Some(1),
                    r#type: Some(Type::String as i32),
                    label: Some(Label::Optional as i32),
                    ..Default::default()
                },
                FieldDescriptorProto {
                    name: Some("value".into()),
                    number: Some(2),
                    r#type: Some(Type::Message as i32),
                    r#type_name: Some(".demo.Item".into()),
                    label: Some(Label::Optional as i32),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let items_field = FieldDescriptorProto {
            name: Some("items".into()),
            number: Some(3),
            r#type: Some(Type::Message as i32),
            r#type_name: Some(".demo.Node.ItemsEntry".into()),
            label: Some(Label::Repeated as i32),
            ..Default::default()
        };
        let node = DescriptorProto {
            name: Some("Node".into()),
            field: vec![value_field, children_field, items_field],
            nested_type: vec![entry],
            ..Default::default()
        };
        let file = FileDescriptorProto {
            name: Some("tree.proto".into()),
            package: Some("demo".into()),
            message_type: vec![item, node],
            ..Default::default()
        };

        let proto = DynamicProto::from_file_descriptors(&[file.encode_to_vec()]).unwrap();
        let schema = proto.message_schema("demo.Node").unwrap();
        let props = schema["properties"].as_object().unwrap();

        // ordinary field
        assert_eq!(props["value"]["type"], "integer");
        // repeated recursion → items is an empty-object placeholder (children is Node itself, avoiding infinite expansion)
        assert_eq!(props["children"]["type"], "object[]");
        let child_items = props["children"]["items"].as_object().unwrap();
        assert!(child_items.is_empty());
        // map → additionalProperties is the field map of Item (the key field)
        assert!(props["items"]["type"].as_str().unwrap().starts_with("map<"));
        let add_props = props["items"]["additionalProperties"].as_object().unwrap();
        assert_eq!(add_props["key"]["type"], "string");
    }
}
