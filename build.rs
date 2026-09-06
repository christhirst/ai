use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path().unwrap());
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    tonic_prost_build::configure()
        .file_descriptor_set_path(out_dir.join("table_populator_descriptor.bin"))
        .compile_protos(&["proto/table_populator.proto"], &["proto"])?;
    Ok(())
}
