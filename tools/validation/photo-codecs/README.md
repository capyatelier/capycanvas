# Independent native photo-codec references

These optional tools compare the shared Rust photo codecs with independent
libultrahdr, libavif/dav1d and libheif/libde265 implementations. They are never
used by ordinary Cargo builds, application capabilities, GTK staging, Web or
Android photo operations. Application photo support requires no helper executable
or native codec bundle.

The pinned source manifest and checked build recipe were moved here from the
old production bundle. The bridge and helper retain their ABI so existing
reference builds can still be used explicitly. Build on Linux with C/C++,
CMake, Ninja, Meson, pkg-config, patch and NASM on x86:

```sh
python3 tools/validation/photo-codecs/photo-codecs.py --fetch
```

The default output is `target/photo-codec-reference`. Source downloads require
`--fetch`; omit it to use already verified cached archives. `--output` and
`--cache` select separate reference work/cache directories. The output retains
source archives, original licenses, local patches, this recipe and checksums.
No system libraries are installed.

Rust reference tests are opt-in and use an explicitly selected trusted directory:

```sh
CAPY_PHOTO_CODEC_DIR=/absolute/path/to/photo-codec-reference/prefix/lib \
  cargo test --release -p layer-color --features native-codec-reference \
  jpeg_interoperates_both_directions_with_libultrahdr -- --ignored --nocapture

CAPY_PHOTO_CODEC_DIR=/absolute/path/to/photo-codec-reference/prefix/lib \
  cargo test --release -p layer-color --features native-codec-reference \
  rust_avif_output_interoperates_with_native_libavif -- --ignored --nocapture
```

The `native-codec-reference` feature only enables optional test adapters. It does
not add application formats or change production dispatch. The adapters no longer
search relative to an application executable or discover installed bundles.
HEIC's raw-plane comparison is described in
`crates/layer-color/tests/fixtures/heif/README.md` and uses the separate
`tools/validation/heif_decode_reference.c` oracle.
