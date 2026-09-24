fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/accept.proto");
    let out_dir = std::env::var("OUT_DIR")?;
    // tonic-prost-build does not expose a protoc path setting, so point PROTOC at the vendored protoc,
    // so neither CI nor local builds need a system protoc installed
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    tonic_prost_build::configure()
        .file_descriptor_set_path(std::path::PathBuf::from(&out_dir).join("accept_descriptor.bin"))
        .compile_protos(&["proto/accept.proto"], &["proto/"])?;
    Ok(())
}
