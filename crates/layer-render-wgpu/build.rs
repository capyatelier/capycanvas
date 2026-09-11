use std::{env, fs, path::Path};

fn sources(path: &Path, hash: &mut u64) {
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            sources(&entry, hash);
        }
    } else {
        // Deterministic generation ID, not a security hash. Include all source
        // inputs that can change shader code, pipeline layouts, or the catalog.
        for byte in fs::read(path).unwrap() {
            *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
}

fn main() {
    let root = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&root);
    let mut hash = 0xcbf29ce484222325;
    for path in [
        "src",
        "build.rs",
        "../../Cargo.lock",
        "../layer-core/src",
        "../../assets/filters",
    ] {
        let path = root.join(path);
        if path.exists() {
            sources(&path, &mut hash);
        }
    }
    println!("cargo:rustc-env=CAPY_SHADER_GENERATION={hash:016x}");
}
