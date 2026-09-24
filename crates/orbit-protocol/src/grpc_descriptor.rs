//! gRPC interface descriptors: organize proto sources / reflection caches uniformly into a package → service → rpc tree
//!
//! Used by the "gRPC collection editor":
//! - `parse_proto_files`: compile .proto sources into a descriptor tree with protox (frontend proto import)
//! - `descriptor_from_cache`: deserialize the reflection cache's `FileDescriptorProto` bytes into the same tree (frontend reflection import)
//! - `message_template`: generate a JSON sample template for an rpc input message (auto-filled by the frontend Message editor)
//!
//! The resulting `GrpcDescriptor` serializes to JSON and is returned to the frontend directly by `/api/grpc/*`.

use serde::{Deserialize, Serialize};

use crate::grpc_reflection::ServiceDescriptorCache;
use crate::types::ProtocolError;

/// The complete gRPC interface hierarchy: a package level → a service level → rpc records
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcDescriptor {
    pub packages: Vec<GrpcPackage>,
    /// `FileDescriptorProto` encoded bytes (base64), sent back by the frontend to generate message templates
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descriptor_files: Vec<String>,
}

/// A single package: may contain one or more services
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcPackage {
    /// package name (an empty string when there is no package)
    pub name: String,
    /// Original proto file content for this package (source for proto import; None for reflection import)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proto: Option<String>,
    pub services: Vec<GrpcService>,
}

/// A single service: may contain one or more rpcs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcService {
    pub name: String,
    pub methods: Vec<GrpcRpc>,
}

/// A single rpc method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrpcRpc {
    /// rpc name
    pub name: String,
    /// Fully-qualified input message name (with a leading dot, e.g. .pkg.Request)
    pub input_type: String,
    /// Fully-qualified output message name
    pub output_type: String,
    /// Whether it is client streaming
    pub client_streaming: bool,
    /// Whether it is server streaming
    pub server_streaming: bool,
}

impl GrpcDescriptor {
    /// Whether it contains no interfaces (used for the frontend hint when the import result is empty)
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

/// Compile proto source files into a descriptor tree with protox.
///
/// `files` is a list of `(file name, source content)` pairs; multiple files are allowed (mutual imports supported).
/// Compilation uses an in-memory resolver and never touches disk.
pub fn parse_proto_files(files: &[(&str, &str)]) -> Result<GrpcDescriptor, ProtocolError> {
    use protox::file::{File, FileResolver};
    use protox::Compiler;
    use std::path::Path;

    if files.is_empty() {
        return Ok(GrpcDescriptor::default());
    }

    // In-memory file resolver: returns the parsed File for a name; missing files fall through to GoogleFileResolver.
    // Contents are cloned into owned Strings so the resolver satisfies `'static` (required by Compiler).
    struct MemoryResolver {
        files: std::collections::HashMap<String, String>,
    }

    impl FileResolver for MemoryResolver {
        fn open_file(&self, name: &str) -> Result<File, protox::Error> {
            match self.files.get(name) {
                Some(source) => File::from_source(name, source),
                None => Err(protox::Error::file_not_found(name)),
            }
        }
    }

    let mut resolver = protox::file::ChainFileResolver::new();
    let map: std::collections::HashMap<String, String> = files
        .iter()
        .map(|(name, content)| (name.to_string(), content.to_string()))
        .collect();
    resolver.add(MemoryResolver { files: map });
    resolver.add(protox::file::GoogleFileResolver::new());

    let mut compiler = Compiler::with_file_resolver(resolver);
    compiler.include_imports(true).include_source_info(true);

    for (name, _) in files {
        compiler
            .open_file(Path::new(name))
            .map_err(|e| ProtocolError::Protocol(format!("proto parsing failed: {}", e)))?;
    }

    let file_set = compiler.file_descriptor_set();

    // protox 0.8 uses prost-types 0.13 internally while this crate uses 0.14; the two types are incompatible.
    // We bridge via wire bytes: encode → decode, so the reflection path shares the same traversal code.
    // Upgrade path: once protox moves to prost 0.14 this bridge can be removed and its file_descriptor_set() used directly.
    let bytes = protox::prost::Message::encode_to_vec(&file_set);
    let fds = <prost_types::FileDescriptorSet as prost::Message>::decode(bytes.as_slice())
        .map_err(|e| {
            ProtocolError::Protocol(format!("failed to parse FileDescriptorSet: {}", e))
        })?;

    // Keep a file name → source map to populate package.proto
    let source_map: std::collections::HashMap<&str, &str> =
        files.iter().map(|(n, c)| (*n, *c)).collect();

    Ok(descriptor_from_file_set(&fds, Some(&source_map)))
}

/// Import the interface hierarchy from a target server via gRPC Server Reflection.
///
/// `target` looks like `http://host:port` (tonic Endpoint format).
pub async fn reflect_descriptor(target: &str) -> Result<GrpcDescriptor, ProtocolError> {
    let endpoint = target.trim_end_matches('/');
    let channel = tonic::transport::Endpoint::from_shared(endpoint.to_string())
        .map_err(|e| ProtocolError::Connect(format!("Invalid gRPC endpoint: {}", e)))?
        .connect()
        .await
        .map_err(|e| ProtocolError::Connect(format!("gRPC connect failed: {}", e)))?;

    let mut client = crate::grpc_reflection::GrpcReflectionClient::new(channel);
    let cache = client
        .build_cache()
        .await
        .map_err(|e| ProtocolError::Protocol(format!("reflection import failed: {}", e)))?;

    Ok(descriptor_from_cache(&cache))
}

/// Build a descriptor tree from the reflection cache (reusing the output of `GrpcReflectionClient::build_cache`).
pub fn descriptor_from_cache(cache: &ServiceDescriptorCache) -> GrpcDescriptor {
    use prost::Message as _;

    let mut fds = prost_types::FileDescriptorSet::default();

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for descs in cache.file_descriptors.values() {
        for bytes in descs {
            match prost_types::FileDescriptorProto::decode(bytes.as_slice()) {
                Ok(fd) => {
                    let name = fd.name.clone().unwrap_or_default();
                    if seen.insert(name) {
                        fds.file.push(fd);
                    }
                }
                Err(e) => {
                    tracing::warn!("failed to parse reflection FileDescriptorProto: {}", e);
                }
            }
        }
    }

    descriptor_from_file_set(&fds, None)
}

/// Generate a sample JSON template for the input message of a given rpc.
///
/// `files` is `FileDescriptorProto` encoded bytes (from proto parsing or the reflection cache),
/// `input_type` is the fully-qualified input message name.
pub fn message_template(
    files: &[Vec<u8>],
    input_type: &str,
) -> Result<serde_json::Value, ProtocolError> {
    let dynamic = orbit_codec::protobuf::DynamicProto::from_file_descriptors(files)
        .map_err(|e| ProtocolError::Codec(e.to_string()))?;
    dynamic
        .message_template(input_type)
        .map_err(|e| ProtocolError::Codec(e.to_string()))
}

/// Generate a schema for a message type (matching the frontend `ResponseSchemaDialog`'s `{ properties }` structure).
///
/// `files` is `FileDescriptorProto` encoded bytes (from proto parsing or the reflection cache),
/// `message_type` is the fully-qualified message name. Returns something like:
/// `{ properties: { fieldName: { type, required, example, description, properties?, items? } } }`,
/// Nested messages are expanded recursively for the frontend "view model definition" dialog.
pub fn message_schema(
    files: &[Vec<u8>],
    message_type: &str,
) -> Result<serde_json::Value, ProtocolError> {
    let dynamic = orbit_codec::protobuf::DynamicProto::from_file_descriptors(files)
        .map_err(|e| ProtocolError::Codec(e.to_string()))?;
    dynamic
        .message_schema(message_type)
        .map_err(|e| ProtocolError::Codec(e.to_string()))
}

/// Assemble the package/service/rpc tree from a unified FileDescriptorSet (this crate's prost-types version).
fn descriptor_from_file_set(
    fds: &prost_types::FileDescriptorSet,
    source_map: Option<&std::collections::HashMap<&str, &str>>,
) -> GrpcDescriptor {
    // package name → (proto content, service name → method list)
    let mut packages: Vec<GrpcPackage> = Vec::new();
    let mut pkg_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for file in &fds.file {
        let pkg_name = file.package.clone().unwrap_or_default();
        let idx = match pkg_index.get(&pkg_name) {
            Some(&i) => i,
            None => {
                let proto = source_map.and_then(|m| {
                    file.name
                        .as_deref()
                        .and_then(|n| m.get(n))
                        .map(|s| s.to_string())
                });
                packages.push(GrpcPackage {
                    name: pkg_name.clone(),
                    proto,
                    services: Vec::new(),
                });
                let i = packages.len() - 1;
                pkg_index.insert(pkg_name.clone(), i);
                i
            }
        };

        for service in &file.service {
            let svc_name = service.name.clone().unwrap_or_default();
            let mut svc = GrpcService {
                name: svc_name,
                methods: Vec::new(),
            };
            for method in &service.method {
                svc.methods.push(GrpcRpc {
                    name: method.name.clone().unwrap_or_default(),
                    input_type: method.input_type.clone().unwrap_or_default(),
                    output_type: method.output_type.clone().unwrap_or_default(),
                    client_streaming: method.client_streaming.unwrap_or(false),
                    server_streaming: method.server_streaming.unwrap_or(false),
                });
            }
            packages[idx].services.push(svc);
        }
    }

    // Filter out empty packages with no services
    packages.retain(|p| !p.services.is_empty());

    // Serialize FileDescriptorProto (base64), returned by the frontend to generate message templates
    use base64::Engine as _;
    use prost::Message as _;
    let descriptor_files = fds
        .file
        .iter()
        .map(|fd| base64::engine::general_purpose::STANDARD.encode(fd.encode_to_vec()))
        .collect::<Vec<_>>();

    GrpcDescriptor {
        packages,
        descriptor_files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message as _;

    const DEMO_PROTO: &str = r#"
syntax = "proto3";
package hello;
message HelloRequest {
  string name = 1;
  int32 count = 2;
}
message HelloReply {
  string message = 1;
}
service Greeter {
  rpc SayHello(HelloRequest) returns (HelloReply);
}
"#;

    /// Manually build a FileDescriptorProto (with service + rpc), encoded for testing tree assembly
    fn build_demo_file_proto() -> Vec<u8> {
        use prost_types::{
            field_descriptor_proto::{Label, Type},
            DescriptorProto, FieldDescriptorProto, MethodDescriptorProto, ServiceDescriptorProto,
        };

        let req_field = FieldDescriptorProto {
            name: Some("name".into()),
            number: Some(1),
            r#type: Some(Type::String as i32),
            label: Some(Label::Optional as i32),
            ..Default::default()
        };
        let count_field = FieldDescriptorProto {
            name: Some("count".into()),
            number: Some(2),
            r#type: Some(Type::Int32 as i32),
            label: Some(Label::Optional as i32),
            ..Default::default()
        };
        let req_msg = DescriptorProto {
            name: Some("HelloRequest".into()),
            field: vec![req_field, count_field],
            ..Default::default()
        };
        let reply_msg = DescriptorProto {
            name: Some("HelloReply".into()),
            ..Default::default()
        };
        let method = MethodDescriptorProto {
            name: Some("SayHello".into()),
            input_type: Some(".hello.HelloRequest".into()),
            output_type: Some(".hello.HelloReply".into()),
            ..Default::default()
        };
        let service = ServiceDescriptorProto {
            name: Some("Greeter".into()),
            method: vec![method],
            ..Default::default()
        };
        let file = prost_types::FileDescriptorProto {
            name: Some("hello.proto".into()),
            package: Some("hello".into()),
            message_type: vec![req_msg, reply_msg],
            service: vec![service],
            ..Default::default()
        };
        file.encode_to_vec()
    }

    #[test]
    fn test_parse_proto_files_basic() {
        let desc = parse_proto_files(&[("hello.proto", DEMO_PROTO)]).unwrap();
        assert_eq!(desc.packages.len(), 1);
        let pkg = &desc.packages[0];
        assert_eq!(pkg.name, "hello");
        // proto source content should be kept with the package
        assert!(pkg
            .proto
            .as_deref()
            .unwrap_or("")
            .contains("service Greeter"));
        assert_eq!(pkg.services.len(), 1);
        let svc = &pkg.services[0];
        assert_eq!(svc.name, "Greeter");
        assert_eq!(svc.methods.len(), 1);
        let rpc = &svc.methods[0];
        assert_eq!(rpc.name, "SayHello");
        assert_eq!(rpc.input_type, ".hello.HelloRequest");
        assert_eq!(rpc.output_type, ".hello.HelloReply");
        assert!(!rpc.client_streaming);
        assert!(!rpc.server_streaming);
    }

    #[test]
    fn test_descriptor_from_cache() {
        let cache = ServiceDescriptorCache {
            services: vec!["hello.Greeter".into()],
            methods: Default::default(),
            file_descriptors: std::collections::HashMap::from([(
                "hello.Greeter".to_string(),
                vec![build_demo_file_proto()],
            )]),
        };

        let desc = descriptor_from_cache(&cache);
        assert_eq!(desc.packages.len(), 1);
        assert_eq!(desc.packages[0].name, "hello");
        assert_eq!(desc.packages[0].services[0].name, "Greeter");
        assert_eq!(desc.packages[0].services[0].methods[0].name, "SayHello");
        // The reflection path has no proto source
        assert!(desc.packages[0].proto.is_none());
    }

    #[test]
    fn test_parse_proto_empty() {
        let desc = parse_proto_files(&[]).unwrap();
        assert!(desc.is_empty());
    }

    #[test]
    fn test_message_template() {
        let files = vec![build_demo_file_proto()];
        let tpl = message_template(&files, ".hello.HelloRequest").unwrap();
        assert_eq!(tpl["name"], "");
        assert_eq!(tpl["count"], 0);
    }

    #[test]
    fn test_message_schema() {
        let files = vec![build_demo_file_proto()];
        let schema = message_schema(&files, ".hello.HelloRequest").unwrap();
        let props = schema["properties"].as_object().unwrap();
        // string field → type string
        assert_eq!(props["name"]["type"], "string");
        // int32 field → type integer + example 0
        assert_eq!(props["count"]["type"], "integer");
        assert_eq!(props["count"]["example"], 0);
    }

    /// Reflection import integration check (requires the local orbit-testservers gRPC Echo service running on port 18781).
    /// Ignored by default; to verify manually: `cargo test -p orbit-protocol -- --ignored grpc_reflect_live`
    #[tokio::test]
    #[ignore]
    async fn grpc_reflect_live() {
        let desc = crate::grpc_descriptor::reflect_descriptor("http://127.0.0.1:18781")
            .await
            .unwrap();
        assert!(!desc.packages.is_empty());
        // Find the Echo service under the orbit.accept package, which should have 4 methods
        let pkg = desc
            .packages
            .iter()
            .find(|p| p.name == "orbit.accept")
            .unwrap_or_else(|| {
                panic!(
                    "orbit.accept package not found, actual: {:?}",
                    desc.packages
                        .iter()
                        .map(|p| p.name.as_str())
                        .collect::<Vec<_>>()
                )
            });
        let svc = pkg.services.iter().find(|s| s.name == "Echo").unwrap();
        assert_eq!(svc.methods.len(), 4);
        // unary methods should be marked non-streaming, and descriptor_files should have produced base64 bytes
        let unary = svc.methods.iter().find(|m| m.name == "Unary").unwrap();
        assert!(!unary.client_streaming);
        assert!(!unary.server_streaming);
        assert!(!desc.descriptor_files.is_empty());
        // The reflection path has no proto source
        assert!(pkg.proto.is_none());
    }
}
