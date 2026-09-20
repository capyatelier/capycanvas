# Independent HEIC test support

This is a test-only extract of the published `heif-oxide` 0.1.0 crate, not an
application dependency or a full crate snapshot. It retains the independent
ISO box writer and the four synthetic lossless x265 streams used by
`crates/layer-color/src/photo/avif_io/hevc_tests.rs`.

Upstream: <https://github.com/dan335/heif-oxide>, revision
`86d722e46da3292cc5d777aaa99198fdc516f0c5`.
Registry archive SHA-256:
`e12acb6edcb3bb9227dc6a06dd375a221eafe397ae2de72a876d79313831e365`.
The MIT and Apache-2.0 licenses and registry revision record are retained.

`test_builder.rs` is upstream `src/test_builder.rs` with only the existing
`pict` handler addition recorded in `test-builder.patch`, so independent libheif
can enumerate the constructed files. The `.h265` files are unchanged upstream
testdata. No codec library or native encoder is needed to run the ordinary tests.

The production HEIC path uses `rust_h265` directly. No heif-oxide runtime source
or Cargo dependency remains. Keep this extract attributed when moving or
modifying it; its placement in a test directory does not make it first-party code.
