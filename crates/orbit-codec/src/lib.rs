//! # orbit-codec
//!
//! Extensible data-format codec abstraction layer. The `Codec` trait unifies
//! encoding/decoding of all data formats, so upper layers need not know whether the format is JSON or Protobuf.
//!
//! ## Built-in formats
//! - JSON (serde_json)
//! - YAML (serde_yaml)
//! - Binary (pass-through)
//!
//! ## Format extensions
//! -  XML, Protobuf, Form
//! - WASM plugin: custom data formats

pub mod form;
pub mod json;
pub mod msgpack;
pub mod protobuf;
pub mod registry;
pub mod traits;
pub mod types;
pub mod xml;

pub use traits::Codec;
pub use types::{CodecError, DataValue};
