fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
    }
    tonic_prost_build::compile_protos("proto/table_populator.proto")?;
    Ok(())
}
