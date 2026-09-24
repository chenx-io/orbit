//! Type definitions for the data-format abstraction layer

use std::collections::HashMap;
use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use base64::Engine;

/// Structured data - the intermediate representation shared by all codecs
#[derive(Debug, Clone, PartialEq)]
pub enum DataValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<DataValue>),
    Object(HashMap<String, DataValue>),
}

impl Serialize for DataValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            DataValue::Null => serializer.serialize_unit(),
            DataValue::Bool(b) => serializer.serialize_bool(*b),
            DataValue::Int(i) => serializer.serialize_i64(*i),
            DataValue::Float(f) => serializer.serialize_f64(*f),
            DataValue::String(s) => serializer.serialize_str(s),
            DataValue::Bytes(b) => {
                // Text formats (JSON/YAML, etc.) represent bytes as a base64 string (reversible),
                // while binary formats (msgpack, etc.) use the native bytes type
                if serializer.is_human_readable() {
                    serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(b))
                } else {
                    serializer.serialize_bytes(b)
                }
            }
            DataValue::Array(arr) => arr.serialize(serializer),
            DataValue::Object(map) => map.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for DataValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DataValueVisitor;

        impl<'de> Visitor<'de> for DataValueVisitor {
            type Value = DataValue;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON-compatible value")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(DataValue::Null)
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(DataValue::Null)
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
                Ok(DataValue::Bool(v))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
                Ok(DataValue::Int(v))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
                Ok(i64::try_from(v)
                    .map(DataValue::Int)
                    .unwrap_or_else(|_| DataValue::Float(v as f64)))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
                Ok(DataValue::Float(v))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
                Ok(DataValue::String(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
                Ok(DataValue::String(v))
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E> {
                Ok(DataValue::Bytes(v.to_vec()))
            }

            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                Ok(DataValue::Bytes(v))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut arr = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(item) = seq.next_element::<DataValue>()? {
                    arr.push(item);
                }
                Ok(DataValue::Array(arr))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let mut obj = HashMap::new();
                while let Some((k, v)) = map.next_entry::<String, DataValue>()? {
                    obj.insert(k, v);
                }
                Ok(DataValue::Object(obj))
            }
        }

        deserializer.deserialize_any(DataValueVisitor)
    }
}

/// Codec error
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("encode failed: {0}")]
    Encode(String),

    #[error("decode failed: {0}")]
    Decode(String),

    #[error("unsupported format: {0}")]
    Unsupported(String),
}
