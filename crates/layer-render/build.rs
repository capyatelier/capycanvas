#[path = "src/panel_profile.rs"]
mod panel_profile;

fn main() {
    println!("cargo:rerun-if-changed=src/panel_profile.rs");
    let path=std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("panel-profiles.bin");
    let bytes=panel_profile::generate().into_iter().flatten().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
    std::fs::write(path,bytes).unwrap();
}
