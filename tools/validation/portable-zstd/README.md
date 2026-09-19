# Portable raster compression validation

This separate Cargo workspace compares the application Rust Zstd implementation
with native libzstd 1.5.7 through `zstd` 0.13.3. The C dependency is a validation
oracle and is absent from application builds.

From the repository root:

```sh
CARGO_TARGET_DIR=/tmp/capy-zstd-oracle-target cargo run --locked --release \
  --manifest-path tools/validation/portable-zstd/Cargo.toml -- path/to/existing.capy
```

Archive paths are optional. The oracle checks 256 mixed-entropy and length-boundary
inputs in both codec directions, 257 forced Huffman-only blocks (including full
alphabets and length-limited trees), and all four raster depths. For each supplied
v4/v5 archive, it checks every old compressed frame with both decoders, recompresses
with Rust for the native decoder, then reads, writes and reopens the project.

Tile measurements include sample validation, shuffling and SHA-256 in both Rust
and C paths, with current Rust hashing used in both to isolate compression costs.
These synthetic tile timings and sizes are diagnostics, not a drawing-latency or
device-performance guarantee. Archive fixtures supplied locally are not bundled.

To regenerate the permanent native level-1 fixtures:

```sh
CAPY_ZSTD_FIXTURES=crates/layer-core/tests/fixtures/native-zstd \
  CARGO_TARGET_DIR=/tmp/capy-zstd-oracle-target cargo run --locked --release \
  --manifest-path tools/validation/portable-zstd/Cargo.toml
```

The normal `layer-core` tests reopen those checked-in frames without compiling
native code. Vendor unit tests can be run independently:

```sh
cargo test --manifest-path vendor/zrip-core/Cargo.toml --features paranoid
```
