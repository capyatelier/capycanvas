use std::{env, fs, path::Path};

fn hash_bytes(bytes: &[u8], hash: &mut u64) {
    for byte in bytes {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
}

fn sources(root: &Path, path: &Path, hash: &mut u64) {
    println!("cargo:rerun-if-changed={}", path.display());
    // Include names, boundaries and lengths, so moves/deletions also invalidate.
    let relative = path.strip_prefix(root).unwrap().to_string_lossy();
    hash_bytes(relative.replace('\\', "/").as_bytes(), hash);
    hash_bytes(&[0], hash);
    if path.is_dir() {
        hash_bytes(b"directory\0", hash);
        let mut entries: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            sources(root, &entry, hash);
        }
    } else {
        let bytes = fs::read(path).unwrap();
        hash_bytes(&(bytes.len() as u64).to_le_bytes(), hash);
        hash_bytes(&bytes, hash);
    }
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest).join("../..").canonicalize().unwrap();
    let mut hash = 0xcbf29ce484222325;
    // Preserve the renderer's full previous invalidation set and include this
    // generation implementation. Hash contents at build time, never at startup.
    for path in [
        "crates/layer-render-wgpu/src",
        "crates/layer-render-wgpu/Cargo.toml",
        "crates/layer-shader-cache-key/src",
        "crates/layer-shader-cache-key/build.rs",
        "crates/layer-shader-cache-key/Cargo.toml",
        "Cargo.lock",
        "crates/layer-core/src",
        "assets/filters",
    ] {
        sources(&root, &root.join(path), &mut hash);
    }
    println!("cargo:rustc-env=CAPY_SHADER_GENERATION={hash:016x}");
}
