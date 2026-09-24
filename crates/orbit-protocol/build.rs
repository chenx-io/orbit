#[cfg(feature = "grpc")]
fn main() {
    // Use vendored protoc so neither CI nor local builds need a system protoc installed
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config
        .compile_protos(&["proto/reflection.proto"], &["proto/"])
        .unwrap();
}

#[cfg(not(feature = "grpc"))]
fn main() {}
