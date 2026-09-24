//! Form-data (multipart/form-data) codec
//!
//! Supports encoding key-value pairs and file uploads as multipart/form-data.
//! Decoding parses a multipart body into key-value pairs.

use std::collections::HashMap;

use crate::traits::Codec;
use crate::types::{CodecError, DataValue};

/// Form-data codec
///
/// Encoding:
/// - DataValue::Object → multipart/form-data byte stream
/// - A value may be a String (text field) or an Object {"filename": ..., "content_type": ..., "data": ...} (file field)
///
/// Decoding:
/// - multipart/form-data byte stream → DataValue::Object
pub struct FormCodec {
    /// Boundary separator (auto-generated when None)
    boundary: String,
}

impl FormCodec {
    pub fn new() -> Self {
        Self {
            boundary: format!("----OrbitFormBoundary{}", uuid::Uuid::new_v4()),
        }
    }

    /// Get the generated boundary string (for the Content-Type header)
    pub fn boundary(&self) -> &str {
        &self.boundary
    }

    /// Encode DataValue::Object as multipart/form-data
    fn encode_object(&self, obj: &HashMap<String, DataValue>) -> Result<Vec<u8>, CodecError> {
        let mut body = Vec::new();

        for (name, value) in obj {
            body.extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());

            match value {
                DataValue::String(text) => {
                    body.extend_from_slice(
                        format!(
                            "Content-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n",
                            name, text
                        )
                        .as_bytes(),
                    );
                }
                DataValue::Bytes(data) => {
                    body.extend_from_slice(
                        format!(
                            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
                            name, name
                        )
                        .as_bytes(),
                    );
                    body.extend_from_slice(data);
                    body.extend_from_slice(b"\r\n");
                }
                DataValue::Object(file_obj) => {
                    let filename = file_obj
                        .get("filename")
                        .and_then(|v| match v {
                            DataValue::String(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .unwrap_or(name.as_str());
                    let content_type = file_obj
                        .get("content_type")
                        .and_then(|v| match v {
                            DataValue::String(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .unwrap_or("application/octet-stream");
                    let data = match file_obj.get("data") {
                        Some(DataValue::Bytes(b)) => b.clone(),
                        Some(DataValue::String(s)) => s.as_bytes().to_vec(),
                        _ => vec![],
                    };

                    body.extend_from_slice(
                        format!(
                            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                            name, filename, content_type
                        )
                        .as_bytes(),
                    );
                    body.extend_from_slice(&data);
                    body.extend_from_slice(b"\r\n");
                }
                other => {
                    // serialize other types into a JSON string
                    let json_str = serde_json::to_string(other)
                        .map_err(|e| CodecError::Encode(e.to_string()))?;
                    body.extend_from_slice(
                        format!(
                            "Content-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n",
                            name, json_str
                        )
                        .as_bytes(),
                    );
                }
            }
        }

        // closing boundary
        body.extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());

        Ok(body)
    }
}

impl Default for FormCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Codec for FormCodec {
    fn name(&self) -> &str {
        "form-data"
    }

    fn mime_types(&self) -> Vec<&str> {
        vec!["multipart/form-data"]
    }

    fn encode(&self, value: &DataValue) -> Result<Vec<u8>, CodecError> {
        match value {
            DataValue::Object(obj) => self.encode_object(obj),
            DataValue::Array(arr) => {
                // convert the array into an object with "0", "1", ... keys
                let mut map = HashMap::new();
                for (i, v) in arr.iter().enumerate() {
                    map.insert(i.to_string(), v.clone());
                }
                self.encode_object(&map)
            }
            _ => Err(CodecError::Encode(
                "FormCodec requires DataValue::Object".into(),
            )),
        }
    }

    fn decode(&self, bytes: &[u8]) -> Result<DataValue, CodecError> {
        // simple parsing of multipart/form-data
        let text = String::from_utf8_lossy(bytes);
        let boundary = self.boundary.as_str();
        let mut map = HashMap::new();

        // split by boundary
        let parts: Vec<&str> = text.split(&format!("--{}", boundary)).collect();

        // non-multipart (e.g. x-www-form-urlencoded): parse as &-separated key=value pairs,
        // and avoid a parts[1..len-1] out-of-bounds panic
        if parts.len() < 3 {
            return Ok(decode_urlencoded(&text));
        }

        for part in &parts[1..parts.len() - 1] {
            // skip the leading \r\n
            let part = part.strip_prefix("\r\n").unwrap_or(part);
            // skip the trailing \r\n
            let part = part.strip_suffix("\r\n").unwrap_or(part);

            // separate the headers and the body
            if let Some(body_start) = part.find("\r\n\r\n") {
                let headers_str = &part[..body_start];
                let body_bytes = &part[body_start + 4..];

                // parse Content-Disposition to get the name
                let name = headers_str
                    .lines()
                    .find(|l| l.starts_with("Content-Disposition:"))
                    .and_then(|l| l.split("name=\"").nth(1).and_then(|s| s.split('\"').next()))
                    .unwrap_or("unknown")
                    .to_string();

                let filename = headers_str
                    .lines()
                    .find(|l| l.starts_with("Content-Disposition:"))
                    .and_then(|l| {
                        l.split("filename=\"")
                            .nth(1)
                            .and_then(|s| s.split('\"').next())
                    });

                let body_data = body_bytes.as_bytes();

                if let Some(fname) = filename {
                    // file field
                    let content_type = headers_str
                        .lines()
                        .find(|l| l.starts_with("Content-Type:"))
                        .and_then(|l| l.split(": ").nth(1))
                        .unwrap_or("application/octet-stream");

                    let mut file_obj = HashMap::new();
                    file_obj.insert("filename".to_string(), DataValue::String(fname.to_string()));
                    file_obj.insert(
                        "content_type".to_string(),
                        DataValue::String(content_type.to_string()),
                    );
                    file_obj.insert("data".to_string(), DataValue::Bytes(body_data.to_vec()));
                    map.insert(name, DataValue::Object(file_obj));
                } else {
                    // ordinary text field
                    let text = String::from_utf8_lossy(body_data).to_string();
                    map.insert(name, DataValue::String(text));
                }
            }
        }

        Ok(DataValue::Object(map))
    }

    fn clone_codec(&self) -> Box<dyn Codec> {
        Box::new(Self {
            boundary: self.boundary.clone(),
        })
    }
}

/// Parse x-www-form-urlencoded: `a=1&b=x` → { a: "1", b: "x" } (including %XX / + decoding)
fn decode_urlencoded(text: &str) -> DataValue {
    let mut map = HashMap::new();
    for pair in text.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        map.insert(percent_decode(k), DataValue::String(percent_decode(v)));
    }
    DataValue::Object(map)
}

/// Minimal percent decoding (%XX → byte; + → space)
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                if let (Some(h), Some(l)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2))) {
                    out.push((h << 4) | l);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(x: Option<&u8>) -> Option<u8> {
    match x {
        Some(v @ b'0'..=b'9') => Some(*v - b'0'),
        Some(v @ b'a'..=b'f') => Some(*v - b'a' + 10),
        Some(v @ b'A'..=b'F') => Some(*v - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_encode_simple() {
        let codec = FormCodec::new();
        let mut map = HashMap::new();
        map.insert("name".to_string(), DataValue::String("Alice".to_string()));
        map.insert("age".to_string(), DataValue::String("30".to_string()));

        let encoded = codec.encode(&DataValue::Object(map)).unwrap();
        let text = String::from_utf8_lossy(&encoded);

        assert!(text.contains("name=\"name\""));
        assert!(text.contains("Alice"));
        assert!(text.contains("name=\"age\""));
        assert!(text.contains("30"));
        assert!(text.contains("----OrbitFormBoundary"));
        assert!(text.ends_with("--\r\n"));
    }

    #[test]
    fn test_form_encode_with_file() {
        let codec = FormCodec::new();
        let mut map = HashMap::new();
        map.insert("username".to_string(), DataValue::String("bob".to_string()));
        let mut file = HashMap::new();
        file.insert(
            "filename".to_string(),
            DataValue::String("test.txt".to_string()),
        );
        file.insert(
            "content_type".to_string(),
            DataValue::String("text/plain".to_string()),
        );
        file.insert(
            "data".to_string(),
            DataValue::Bytes(b"hello world".to_vec()),
        );
        map.insert("file".to_string(), DataValue::Object(file));

        let encoded = codec.encode(&DataValue::Object(map)).unwrap();
        let text = String::from_utf8_lossy(&encoded);

        assert!(text.contains("name=\"username\""));
        assert!(text.contains("bob"));
        assert!(text.contains("name=\"file\""));
        assert!(text.contains("filename=\"test.txt\""));
        assert!(text.contains("Content-Type: text/plain"));
        assert!(text.contains("hello world"));
    }

    #[test]
    fn test_form_roundtrip() {
        let codec = FormCodec::new();
        let mut map = HashMap::new();
        map.insert("key1".to_string(), DataValue::String("value1".to_string()));
        map.insert("key2".to_string(), DataValue::String("value2".to_string()));

        let encoded = codec.encode(&DataValue::Object(map)).unwrap();
        let decoded = codec.decode(&encoded).unwrap();

        if let DataValue::Object(obj) = decoded {
            if let DataValue::String(v) = obj.get("key1").unwrap() {
                assert_eq!(v, "value1");
            } else {
                panic!("key1 not a string");
            }
        } else {
            panic!("not an object");
        }
    }

    #[test]
    fn test_boundary_uniqueness() {
        let c1 = FormCodec::new();
        let c2 = FormCodec::new();
        assert_ne!(c1.boundary(), c2.boundary());
    }
}
