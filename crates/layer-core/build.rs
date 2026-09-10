// Embed the same editable resources as a startup fallback. Distribution hosts
// can load an external manifest instead; this generates no filter constructors.
use std::{env, fs, path::PathBuf};
fn main() {
    let directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/filters");
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut paths: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("json" | "wgsl")
            )
        })
        .collect();
    paths.sort();
    let mut source = String::from("const FILTER_RESOURCES:&[(&str,&str)]=&[\n");
    for path in paths {
        source.push_str(&format!(
            "({:?},include_str!({:?})),\n",
            path.file_name().unwrap().to_str().unwrap(),
            path.canonicalize().unwrap().to_str().unwrap()
        ));
    }
    source.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("filter_resources.rs"),
        source,
    )
    .unwrap();
}
