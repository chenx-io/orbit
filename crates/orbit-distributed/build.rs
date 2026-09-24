fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile the gRPC reporting protocol between the distributed controller and agents
    // Since tonic 0.14, prost code generation has moved to tonic-prost-build
    // tonic-prost-build exposes no protoc path setting, so the vendored protoc is selected via the PROTOC env var,
    // meaning neither CI nor local builds need a system protoc installed
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    tonic_prost_build::configure().compile_protos(&["proto/controller.proto"], &["proto/"])?;
    Ok(())
}
