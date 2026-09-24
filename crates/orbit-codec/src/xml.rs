//! XML codec - based on quick-xml
//!
//! Supports full XML parsing and generation:
//! - Nested elements (recursively parsed into DataValue::Object)
//! - Attributes (stored under `@attr_name` keys)
//! - Text content (stored under the `#text` key; text-only elements become a String directly)
//! - Repeated elements are automatically merged into an Array
//! - Namespaces (xmlns declarations)
//! - CDATA sections
//! - Self-closing tags

use std::collections::HashMap;
use std::io::BufRead;

use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::Reader;

use crate::traits::Codec;
use crate::types::{CodecError, DataValue};

pub struct XmlCodec;

impl Codec for XmlCodec {
    fn name(&self) -> &str {
        "xml"
    }

    fn mime_types(&self) -> Vec<&str> {
        vec!["application/xml", "text/xml", "application/soap+xml"]
    }

    fn encode(&self, value: &DataValue) -> Result<Vec<u8>, CodecError> {
        let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        xml.push_str(&data_to_xml(value, "root"));
        Ok(xml.into_bytes())
    }

    fn decode(&self, bytes: &[u8]) -> Result<DataValue, CodecError> {
        let mut reader = Reader::from_reader(bytes);
        reader.config_mut().trim_text(true);
        let root = parse_element(&mut reader)?;
        Ok(root)
    }

    fn clone_codec(&self) -> Box<dyn Codec> {
        Box::new(XmlCodec)
    }
}

// ── Decode: XML → DataValue ────────────────────────────────────────

/// Recursively parse XML, skipping leading declarations/comments/whitespace
fn parse_element<R: BufRead>(reader: &mut Reader<R>) -> Result<DataValue, CodecError> {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                return parse_single_element(reader, e, false);
            }
            Ok(Event::Empty(e)) => {
                return parse_single_element(reader, e, true);
            }
            Ok(Event::Decl(_)) | Ok(Event::Comment(_)) | Ok(Event::PI(_)) => continue,
            Ok(Event::Text(ref e)) => {
                let text = e
                    .unescape()
                    .map_err(|e| CodecError::Decode(format!("XML unescape: {}", e)))?;
                if !text.trim().is_empty() {
                    return Ok(DataValue::String(text.to_string()));
                }
            }
            Ok(Event::Eof) => return Ok(DataValue::Null),
            Err(e) => {
                return Err(CodecError::Decode(format!("XML parse error: {}", e)));
            }
            _ => {}
        }
    }
}

/// Parse a single element (given its Start or Empty event, already consumed)
fn parse_single_element<R: BufRead>(
    reader: &mut Reader<R>,
    start_event: quick_xml::events::BytesStart,
    is_empty: bool,
) -> Result<DataValue, CodecError> {
    let mut attributes = HashMap::new();

    for attr_result in start_event.attributes() {
        match attr_result {
            Ok(attr) => {
                let attr_key = local_name_str(attr.key);
                let attr_value = String::from_utf8_lossy(&attr.value).to_string();
                if attr_key == "xmlns" {
                    attributes.insert("@xmlns".into(), DataValue::String(attr_value));
                } else {
                    attributes.insert(format!("@{}", attr_key), DataValue::String(attr_value));
                }
            }
            Err(e) => {
                return Err(CodecError::Decode(format!("XML attribute error: {}", e)));
            }
        }
    }

    if is_empty {
        if attributes.is_empty() {
            return Ok(DataValue::String(String::new()));
        }
        return Ok(DataValue::Object(attributes));
    }

    // parse child nodes
    let mut children: Vec<(String, DataValue)> = Vec::new();
    let mut text_content = String::new();
    let mut has_child_elements = false;
    let mut buf = Vec::new();

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(child_e)) => {
                has_child_elements = true;
                let child_name = local_name_str(child_e.name());

                let mut child_attrs = HashMap::new();
                for attr in child_e.attributes().flatten() {
                    let ak = local_name_str(attr.key);
                    let av = String::from_utf8_lossy(&attr.value).to_string();
                    child_attrs.insert(format!("@{}", ak), DataValue::String(av));
                }

                let child_value = parse_element_content(reader, child_e.name())?;
                children.push((child_name, merge_attrs_and_value(child_attrs, child_value)));
            }
            Ok(Event::Empty(child_e)) => {
                has_child_elements = true;
                let child_name = local_name_str(child_e.name());

                let mut child_attrs = HashMap::new();
                for attr in child_e.attributes().flatten() {
                    let ak = local_name_str(attr.key);
                    let av = String::from_utf8_lossy(&attr.value).to_string();
                    child_attrs.insert(format!("@{}", ak), DataValue::String(av));
                }

                let val = if child_attrs.is_empty() {
                    DataValue::Null
                } else {
                    DataValue::Object(child_attrs)
                };
                children.push((child_name, val));
            }
            Ok(Event::Text(e)) => {
                if let Ok(text) = e.unescape() {
                    text_content.push_str(&text);
                }
            }
            Ok(Event::CData(e)) => {
                text_content.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(CodecError::Decode(format!("XML parse error: {}", e)));
            }
            _ => {}
        }
    }

    build_element_result(attributes, children, text_content, has_child_elements)
}

/// Parse an element's child content (from after Start to the matching End) and return the inner value
fn parse_element_content<R: BufRead>(
    reader: &mut Reader<R>,
    expected_end: QName,
) -> Result<DataValue, CodecError> {
    let mut children: Vec<(String, DataValue)> = Vec::new();
    let mut text_content = String::new();
    let mut has_child_elements = false;
    let mut buf = Vec::new();
    let exp_name = local_name_str(expected_end);

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(child_e)) => {
                has_child_elements = true;
                let child_name = local_name_str(child_e.name());

                let mut child_attrs = HashMap::new();
                for attr in child_e.attributes().flatten() {
                    let ak = local_name_str(attr.key);
                    let av = String::from_utf8_lossy(&attr.value).to_string();
                    child_attrs.insert(format!("@{}", ak), DataValue::String(av));
                }

                let child_value = parse_element_content(reader, child_e.name())?;
                children.push((child_name, merge_attrs_and_value(child_attrs, child_value)));
            }
            Ok(Event::Empty(child_e)) => {
                has_child_elements = true;
                let child_name = local_name_str(child_e.name());

                let mut child_attrs = HashMap::new();
                for attr in child_e.attributes().flatten() {
                    let ak = local_name_str(attr.key);
                    let av = String::from_utf8_lossy(&attr.value).to_string();
                    child_attrs.insert(format!("@{}", ak), DataValue::String(av));
                }

                let val = if child_attrs.is_empty() {
                    DataValue::Null
                } else {
                    DataValue::Object(child_attrs)
                };
                children.push((child_name, val));
            }
            Ok(Event::Text(e)) => {
                if let Ok(text) = e.unescape() {
                    text_content.push_str(&text);
                }
            }
            Ok(Event::CData(e)) => {
                text_content.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::End(e)) => {
                let end_name = local_name_str(e.name());
                if end_name == exp_name {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(CodecError::Decode(format!("XML parse error: {}", e)));
            }
            _ => {}
        }
    }

    build_element_result(HashMap::new(), children, text_content, has_child_elements)
}

fn merge_attrs_and_value(mut attrs: HashMap<String, DataValue>, value: DataValue) -> DataValue {
    if attrs.is_empty() {
        return value;
    }
    match value {
        DataValue::String(text) => {
            attrs.insert("#text".into(), DataValue::String(text));
            DataValue::Object(attrs)
        }
        DataValue::Object(mut obj) => {
            obj.extend(attrs);
            DataValue::Object(obj)
        }
        DataValue::Null => DataValue::Object(attrs),
        other => {
            attrs.insert("#text".into(), other);
            DataValue::Object(attrs)
        }
    }
}

fn build_element_result(
    attributes: HashMap<String, DataValue>,
    children: Vec<(String, DataValue)>,
    text_content: String,
    has_child_elements: bool,
) -> Result<DataValue, CodecError> {
    if has_child_elements {
        let mut map: HashMap<String, DataValue> = HashMap::new();

        for (k, v) in attributes {
            map.insert(k, v);
        }

        for (child_name, child_val) in children {
            match map.remove(&child_name) {
                None => {
                    map.insert(child_name, child_val);
                }
                Some(DataValue::Array(mut arr)) => {
                    arr.push(child_val);
                    map.insert(child_name, DataValue::Array(arr));
                }
                Some(existing) => {
                    map.insert(child_name, DataValue::Array(vec![existing, child_val]));
                }
            }
        }

        if !text_content.trim().is_empty() {
            map.insert("#text".into(), DataValue::String(text_content));
        }

        Ok(DataValue::Object(map))
    } else if !text_content.is_empty() && !attributes.is_empty() {
        let mut attrs = attributes;
        attrs.insert("#text".into(), DataValue::String(text_content));
        Ok(DataValue::Object(attrs))
    } else if !attributes.is_empty() {
        Ok(DataValue::Object(attributes))
    } else {
        Ok(DataValue::String(text_content))
    }
}

fn local_name_str(qname: QName) -> String {
    String::from_utf8_lossy(qname.local_name().as_ref()).to_string()
}

// ── Encode: DataValue → XML ────────────────────────────────────────

fn data_to_xml(value: &DataValue, tag: &str) -> String {
    match value {
        DataValue::Object(map) => {
            let mut attrs_str = String::new();
            let mut children_xml = String::new();
            let mut text = String::new();
            let mut xmlns_decls = String::new();

            for (k, v) in map {
                if k == "#text" {
                    if let DataValue::String(s) = v {
                        text = xml_escape(s);
                    } else {
                        text = data_to_xml_value(v);
                    }
                } else if let Some(prefix) = k.strip_prefix("@xmlns:") {
                    xmlns_decls.push_str(&format!(" xmlns:{}=\"{}\"", prefix, data_to_attr_str(v)));
                } else if k == "@xmlns" {
                    xmlns_decls.push_str(&format!(" xmlns=\"{}\"", data_to_attr_str(v)));
                } else if let Some(attr_name) = k.strip_prefix('@') {
                    attrs_str.push_str(&format!(" {}=\"{}\"", attr_name, data_to_attr_str(v)));
                } else {
                    children_xml.push_str(&data_to_xml(v, k));
                }
            }

            if children_xml.is_empty() && text.is_empty() {
                format!("<{tag}{xmlns_decls}{attrs_str} />")
            } else {
                format!("<{tag}{xmlns_decls}{attrs_str}>{text}{children_xml}</{tag}>")
            }
        }
        DataValue::Array(arr) => {
            let mut xml = String::new();
            for item in arr {
                xml.push_str(&data_to_xml(item, tag));
            }
            xml
        }
        DataValue::String(s) => format!("<{tag}>{}</{tag}>", xml_escape(s)),
        DataValue::Int(n) => format!("<{tag}>{n}</{tag}>"),
        DataValue::Float(f) => format!("<{tag}>{f}</{tag}>"),
        DataValue::Bool(b) => format!("<{tag}>{b}</{tag}>"),
        DataValue::Null => format!("<{tag} />"),
        DataValue::Bytes(b) => {
            use std::fmt::Write;
            let mut hex_str = String::new();
            for byte in b {
                write!(hex_str, "{:02x}", byte).unwrap();
            }
            format!("<{tag} encoding=\"hex\">{hex_str}</{tag}>")
        }
    }
}

fn data_to_xml_value(value: &DataValue) -> String {
    match value {
        DataValue::String(s) => xml_escape(s),
        DataValue::Int(n) => n.to_string(),
        DataValue::Float(f) => f.to_string(),
        DataValue::Bool(b) => b.to_string(),
        DataValue::Null => String::new(),
        _ => format!("{:?}", value),
    }
}

fn data_to_attr_str(value: &DataValue) -> String {
    match value {
        DataValue::String(s) => xml_escape(s),
        DataValue::Int(n) => n.to_string(),
        DataValue::Float(f) => f.to_string(),
        DataValue::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xml_decode_flat() {
        let xml = r#"<?xml version="1.0"?><root><name>orbit</name><version>0.1.0</version></root>"#;
        let codec = XmlCodec;
        let value = codec.decode(xml.as_bytes()).unwrap();
        match value {
            DataValue::Object(map) => {
                assert_eq!(
                    map.get("name").map(|v| match v {
                        DataValue::String(s) => s.clone(),
                        _ => "".into(),
                    }),
                    Some("orbit".to_string())
                );
                assert_eq!(
                    map.get("version").map(|v| match v {
                        DataValue::String(s) => s.clone(),
                        _ => "".into(),
                    }),
                    Some("0.1.0".to_string())
                );
            }
            _ => panic!("Expected Object, got {:?}", value),
        }
    }

    #[test]
    fn test_xml_decode_nested() {
        let xml = r#"<root><person><name><first>John</first><last>Doe</last></name><age>30</age></person></root>"#;
        let codec = XmlCodec;
        let value = codec.decode(xml.as_bytes()).unwrap();

        if let DataValue::Object(root) = value {
            if let Some(DataValue::Object(person)) = root.get("person") {
                if let Some(DataValue::Object(name)) = person.get("name") {
                    assert_eq!(
                        name.get("first").map(|v| match v {
                            DataValue::String(s) => s.clone(),
                            _ => "".into(),
                        }),
                        Some("John".to_string())
                    );
                    assert_eq!(
                        name.get("last").map(|v| match v {
                            DataValue::String(s) => s.clone(),
                            _ => "".into(),
                        }),
                        Some("Doe".to_string())
                    );
                } else {
                    panic!("name not an object");
                }
            } else {
                panic!("person not found");
            }
        } else {
            panic!("Expected Object, got {:?}", value);
        }
    }

    #[test]
    fn test_xml_decode_with_attributes() {
        let xml = r#"<root><user id="123" role="admin">Alice</user></root>"#;
        let codec = XmlCodec;
        let value = codec.decode(xml.as_bytes()).unwrap();

        if let DataValue::Object(root) = value {
            if let Some(DataValue::Object(user)) = root.get("user") {
                assert_eq!(
                    user.get("@id").map(|v| match v {
                        DataValue::String(s) => s.clone(),
                        _ => "".into(),
                    }),
                    Some("123".to_string())
                );
                assert_eq!(
                    user.get("@role").map(|v| match v {
                        DataValue::String(s) => s.clone(),
                        _ => "".into(),
                    }),
                    Some("admin".to_string())
                );
                assert_eq!(
                    user.get("#text").map(|v| match v {
                        DataValue::String(s) => s.clone(),
                        _ => "".into(),
                    }),
                    Some("Alice".to_string())
                );
            } else {
                panic!("user not an object");
            }
        } else {
            panic!("Expected Object, got {:?}", value);
        }
    }

    #[test]
    fn test_xml_decode_repeated_elements() {
        let xml = r#"<root><items><item>a</item><item>b</item><item>c</item></items></root>"#;
        let codec = XmlCodec;
        let value = codec.decode(xml.as_bytes()).unwrap();

        if let DataValue::Object(root) = value {
            if let Some(DataValue::Object(items)) = root.get("items") {
                if let Some(DataValue::Array(arr)) = items.get("item") {
                    assert_eq!(arr.len(), 3);
                    let values: Vec<String> = arr
                        .iter()
                        .map(|v| match v {
                            DataValue::String(s) => s.clone(),
                            _ => "".into(),
                        })
                        .collect();
                    assert_eq!(values, vec!["a", "b", "c"]);
                } else {
                    panic!("item not an array");
                }
            } else {
                panic!("items not found");
            }
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_xml_decode_self_closing() {
        let xml = r#"<root><empty /></root>"#;
        let codec = XmlCodec;
        let value = codec.decode(xml.as_bytes()).unwrap();

        if let DataValue::Object(root) = value {
            assert!(root.contains_key("empty"));
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_xml_decode_cdata() {
        let xml = r#"<root><description><![CDATA[<hello> & world</hello>]]></description></root>"#;
        let codec = XmlCodec;
        let value = codec.decode(xml.as_bytes()).unwrap();

        if let DataValue::Object(root) = value {
            if let Some(DataValue::String(s)) = root.get("description") {
                assert_eq!(s, "<hello> & world</hello>");
            } else {
                panic!("description not a string");
            }
        } else {
            panic!("Expected Object");
        }
    }

    #[test]
    fn test_xml_roundtrip_simple() {
        let codec = XmlCodec;
        let mut obj = HashMap::new();
        obj.insert("status".into(), DataValue::String("ok".into()));
        obj.insert("code".into(), DataValue::Int(200));
        let value = DataValue::Object(obj);

        let bytes = codec.encode(&value).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("status"));
        assert!(text.contains("ok"));
        assert!(text.contains("code"));
    }

    #[test]
    fn test_xml_encode_with_attributes() {
        let codec = XmlCodec;
        let mut user = HashMap::new();
        user.insert("@id".into(), DataValue::String("42".into()));
        user.insert("#text".into(), DataValue::String("Bob".into()));
        let mut root = HashMap::new();
        root.insert("user".into(), DataValue::Object(user));
        let value = DataValue::Object(root);

        let bytes = codec.encode(&value).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains(r#"id="42""#));
        assert!(text.contains("Bob"));
    }

    #[test]
    fn test_xml_encode_nested() {
        let codec = XmlCodec;
        let mut name = HashMap::new();
        name.insert("first".into(), DataValue::String("Jane".into()));
        name.insert("last".into(), DataValue::String("Smith".into()));
        let mut person = HashMap::new();
        person.insert("name".into(), DataValue::Object(name));
        person.insert("age".into(), DataValue::Int(25));
        let mut root = HashMap::new();
        root.insert("person".into(), DataValue::Object(person));
        let value = DataValue::Object(root);

        let bytes = codec.encode(&value).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("<first>Jane</first>"));
        assert!(text.contains("<last>Smith</last>"));
        assert!(text.contains("<age>25</age>"));
    }
}
