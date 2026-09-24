//! JSON codec implementation

use crate::traits::Codec;
use crate::types::{CodecError, DataValue};

/// JSON codec
pub struct JsonCodec;

impl Codec for JsonCodec {
    fn name(&self) -> &str {
        "json"
    }

    fn mime_types(&self) -> Vec<&str> {
        vec!["application/json", "text/json"]
    }

    fn encode(&self, value: &DataValue) -> Result<Vec<u8>, CodecError> {
        serde_json::to_vec(value).map_err(|e| CodecError::Encode(e.to_string()))
    }

    fn decode(&self, bytes: &[u8]) -> Result<DataValue, CodecError> {
        serde_json::from_slice(bytes).map_err(|e| CodecError::Decode(e.to_string()))
    }

    fn clone_codec(&self) -> Box<dyn Codec> {
        Box::new(JsonCodec)
    }
}

/// YAML codec
pub struct YamlCodec;

impl Codec for YamlCodec {
    fn name(&self) -> &str {
        "yaml"
    }

    fn mime_types(&self) -> Vec<&str> {
        vec!["application/yaml", "text/yaml", "text/x-yaml"]
    }

    fn encode(&self, value: &DataValue) -> Result<Vec<u8>, CodecError> {
        serde_yaml::to_string(value)
            .map(|s| s.into_bytes())
            .map_err(|e| CodecError::Encode(e.to_string()))
    }

    fn decode(&self, bytes: &[u8]) -> Result<DataValue, CodecError> {
        serde_yaml::from_slice(bytes).map_err(|e| CodecError::Decode(e.to_string()))
    }

    fn clone_codec(&self) -> Box<dyn Codec> {
        Box::new(YamlCodec)
    }
}

/// Raw binary pass-through codec
///
/// Performs no encoding/decoding; passes the byte sequence through verbatim.
/// Used for testing binary protocols or unknown formats.
pub struct BinaryCodec;

impl Codec for BinaryCodec {
    fn name(&self) -> &str {
        "binary"
    }

    fn mime_types(&self) -> Vec<&str> {
        vec!["application/octet-stream"]
    }

    fn encode(&self, value: &DataValue) -> Result<Vec<u8>, CodecError> {
        match value {
            DataValue::Bytes(b) => Ok(b.clone()),
            DataValue::String(s) => Ok(s.as_bytes().to_vec()),
            _ => Ok(serde_json::to_vec(value).unwrap_or_default()),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<DataValue, CodecError> {
        Ok(DataValue::Bytes(bytes.to_vec()))
    }

    fn clone_codec(&self) -> Box<dyn Codec> {
        Box::new(BinaryCodec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DataValue;
    use std::collections::HashMap;

    #[test]
    fn test_json_roundtrip() {
        let codec = JsonCodec;
        let mut obj = HashMap::new();
        obj.insert("name".to_string(), DataValue::String("orbit".to_string()));
        obj.insert(
            "version".to_string(),
            DataValue::String("0.1.0".to_string()),
        );

        let value = DataValue::Object(obj);
        let bytes = codec.encode(&value).unwrap();
        let decoded = codec.decode(&bytes).unwrap();

        match decoded {
            DataValue::Object(map) => {
                assert_eq!(
                    map.get("name").map(|v| match v {
                        DataValue::String(s) => s.clone(),
                        _ => "".to_string(),
                    }),
                    Some("orbit".to_string())
                );
            }
            _ => panic!("Expected Object"),
        }
    }

    #[test]
    fn test_json_bytes_as_base64() {
        let codec = JsonCodec;
        let value = DataValue::Bytes(vec![1, 2, 3]);
        let bytes = codec.encode(&value).unwrap();
        // In text formats Bytes is represented as a base64 string ([1,2,3] → "AQID")
        assert_eq!(String::from_utf8_lossy(&bytes), "\"AQID\"");
    }

    #[test]
    fn test_json_int_array_roundtrip_keeps_array() {
        // Regression: the untagged derive would decode [1,2,3] into Bytes; the hand-written impl must keep it an Array
        let codec = JsonCodec;
        let value = DataValue::Array(vec![
            DataValue::Int(1),
            DataValue::Int(2),
            DataValue::Int(3),
        ]);
        let bytes = codec.encode(&value).unwrap();
        let decoded = codec.decode(&bytes).unwrap();
        assert_eq!(decoded, value);
    }
}
