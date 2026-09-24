//! WASM module: bindings and host implementations for each world, split across files.
//! - protocol.rs: protocol-plugin world (host-transport networking belongs to the host)
//! - codec.rs: codec-plugin world (synchronous bindings, pure functions)

pub mod codec;
pub mod protocol;

pub use codec::WasmCodec;
pub use protocol::{ProtocolPluginShared, WasmProtocolClient};
