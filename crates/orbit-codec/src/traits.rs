//! Codec trait - the abstract interface implemented by every data-format codec

use crate::types::{CodecError, DataValue};

/// Data codec trait
///
/// Every data format (JSON, XML, Protobuf, ...) must implement this trait.
/// Codecs are stateless - one instance can safely be shared across multiple VUs.
pub trait Codec: Send + Sync {
    /// Format name
    fn name(&self) -> &str;

    /// List of supported MIME types
    fn mime_types(&self) -> Vec<&str>;

    /// Encode a DataValue into a byte sequence
    fn encode(&self, value: &DataValue) -> Result<Vec<u8>, CodecError>;

    /// Decode a byte sequence into a DataValue
    fn decode(&self, bytes: &[u8]) -> Result<DataValue, CodecError>;

    /// Clone the codec
    fn clone_codec(&self) -> Box<dyn Codec>;
}
