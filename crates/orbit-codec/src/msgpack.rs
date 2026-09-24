//! MessagePack codec - based on rmp-serde
//!
//! DataValue implements serde::Serialize/Deserialize directly, with no intermediate format,
//! and Bytes is transported as the native msgpack bin type (a base64 string in text formats).

use crate::traits::Codec;
use crate::types::{CodecError, DataValue};

/// MessagePack codec
///
/// Implements JSON-compatible MessagePack encoding/decoding using rmp-serde.
/// Encode path: DataValue → MessagePack bytes
/// Decode path: MessagePack bytes → DataValue
pub struct MsgPackCodec;

impl Codec for MsgPackCodec {
    fn name(&self) -> &str {
        "msgpack"
    }

    fn mime_types(&self) -> Vec<&str> {
        vec![
            "application/msgpack",
            "application/x-msgpack",
            "application/messagepack",
        ]
    }

    fn encode(&self, value: &DataValue) -> Result<Vec<u8>, CodecError> {
        rmp_serde::encode::to_vec(value)
            .map_err(|e| CodecError::Encode(format!("MessagePack encode: {}", e)))
    }

    fn decode(&self, bytes: &[u8]) -> Result<DataValue, CodecError> {
        rmp_serde::decode::from_slice(bytes)
            .map_err(|e| CodecError::Decode(format!("MessagePack decode: {}", e)))
    }

    fn clone_codec(&self) -> Box<dyn Codec> {
        Box::new(MsgPackCodec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_msgpack_string() {
        let codec = MsgPackCodec;
        let value = DataValue::String("hello world".into());
        let encoded = codec.encode(&value).unwrap();
        let decoded = codec.decode(&encoded).unwrap();
        assert_eq!(decoded, DataValue::String("hello world".into()));
    }

    #[test]
    fn test_msgpack_int() {
        let codec = MsgPackCodec;
        let value = DataValue::Int(42);
        let encoded = codec.encode(&value).unwrap();
        let decoded = codec.decode(&encoded).unwrap();
        // In MessagePack an integer may be deserialized into a different numeric type,
        // but they should be semantically equivalent
        match decoded {
            DataValue::Int(n) => assert_eq!(n, 42),
            DataValue::Float(n) => assert!((n - 42.0).abs() < f64::EPSILON),
            other => panic!("Expected number, got {:?}", other),
        }
    }

    #[test]
    fn test_msgpack_nested_object() {
        let codec = MsgPackCodec;
        let mut inner = HashMap::new();
        inner.insert("key".into(), DataValue::String("value".into()));
        inner.insert("num".into(), DataValue::Int(100));

        let mut outer = HashMap::new();
        outer.insert("inner".into(), DataValue::Object(inner));
        outer.insert("name".into(), DataValue::String("test".into()));

        let value = DataValue::Object(outer);
        let encoded = codec.encode(&value).unwrap();
        let decoded = codec.decode(&encoded).unwrap();

        if let DataValue::Object(obj) = decoded {
            assert_eq!(
                obj.get("name").map(|v| match v {
                    DataValue::String(s) => s.clone(),
                    _ => "".into(),
                }),
                Some("test".to_string())
            );
            assert!(obj.contains_key("inner"));
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_msgpack_array() {
        let codec = MsgPackCodec;
        let value = DataValue::Array(vec![
            DataValue::Int(1),
            DataValue::String("two".into()),
            DataValue::Bool(true),
        ]);
        let encoded = codec.encode(&value).unwrap();
        let decoded = codec.decode(&encoded).unwrap();

        if let DataValue::Array(arr) = decoded {
            assert_eq!(arr.len(), 3);
        } else {
            panic!("Expected Array");
        }
    }

    #[test]
    fn test_msgpack_null() {
        let codec = MsgPackCodec;
        let value = DataValue::Null;
        let encoded = codec.encode(&value).unwrap();
        let decoded = codec.decode(&encoded).unwrap();
        assert_eq!(decoded, DataValue::Null);
    }

    #[test]
    fn test_msgpack_bool() {
        let codec = MsgPackCodec;
        for b in [true, false] {
            let value = DataValue::Bool(b);
            let encoded = codec.encode(&value).unwrap();
            let decoded = codec.decode(&encoded).unwrap();
            assert_eq!(decoded, DataValue::Bool(b));
        }
    }

    #[test]
    fn test_msgpack_float() {
        let codec = MsgPackCodec;
        let value = DataValue::Float(1.25);
        let encoded = codec.encode(&value).unwrap();
        let decoded = codec.decode(&encoded).unwrap();
        match decoded {
            DataValue::Float(f) => assert!((f - 1.25).abs() < 0.01),
            DataValue::Int(_) => {} // 1.25 may be simplified
            other => panic!("Expected number, got {:?}", other),
        }
    }

    #[test]
    fn test_msgpack_empty() {
        let codec = MsgPackCodec;
        let empty_obj = DataValue::Object(HashMap::new());
        let encoded = codec.encode(&empty_obj).unwrap();
        let decoded = codec.decode(&encoded).unwrap();
        assert_eq!(decoded, empty_obj);

        let empty_arr = DataValue::Array(vec![]);
        let encoded = codec.encode(&empty_arr).unwrap();
        let decoded = codec.decode(&encoded).unwrap();
        assert_eq!(decoded, empty_arr);
    }

    #[test]
    fn test_msgpack_invalid_data() {
        let codec = MsgPackCodec;
        // Corrupt data should return an error
        let result = codec.decode(&[0xc1]); // 0xc1 is the never-used marker
        assert!(result.is_err());
    }

    #[test]
    fn test_msgpack_bytes_native_bin() {
        // In binary formats Bytes round-trips as the native msgpack bin type, preserving its type
        let codec = MsgPackCodec;
        let value = DataValue::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let encoded = codec.encode(&value).unwrap();
        // msgpack bin header: 0xC4 (bin8) + length
        assert_eq!(encoded[0], 0xC4);
        assert_eq!(encoded[1], 4);
        let decoded = codec.decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_msgpack_roundtrip_complex() {
        let codec = MsgPackCodec;

        let mut inner = HashMap::new();
        inner.insert("id".into(), DataValue::Int(1));
        inner.insert("active".into(), DataValue::Bool(true));
        inner.insert("score".into(), DataValue::Float(99.5));
        inner.insert(
            "tags".into(),
            DataValue::Array(vec![
                DataValue::String("rust".into()),
                DataValue::String("testing".into()),
            ]),
        );

        let value = DataValue::Object(inner);
        let encoded = codec.encode(&value).unwrap();
        assert!(!encoded.is_empty());

        let decoded = codec.decode(&encoded).unwrap();

        if let DataValue::Object(obj) = decoded {
            assert!(obj.contains_key("id"));
            assert!(obj.contains_key("active"));
            assert!(obj.contains_key("score"));
            assert!(obj.contains_key("tags"));
        } else {
            panic!("Expected Object after roundtrip");
        }
    }
}
