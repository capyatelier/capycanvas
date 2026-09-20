# Synthetic HEIC interchange fixtures

These containers wrap lossless x265 codestreams from the published
`heif-oxide` 0.1.0 test data. The upstream MIT OR Apache-2.0 licenses and original
notices are retained under `vendor/heif-oxide/`; there are no photographic assets
in these committed fixtures. Archive and revision provenance is in
`vendor/README.md`, and fixture hashes are in `provenance.json`.

- `flat-red-8bit.heic`: 64×64, limited-range BT.601 matrix and sRGB profile.
- `p3-gray-10bit.heic`: 64×64, lossless 10-bit Y=512/Cb=Cr=512, P3/sRGB-transfer
  NCLX; imported samples retain the P3 profile and normalize to 16-bit storage.
- `p3-grid-8bit.heic`: red/blue 64×64 tiles in a 101×51 partial grid, P3 NCLX,
  then a counterclockwise quarter turn and horizontal output mirror (51×101).

The test-only upstream box writer is independent of our BMFF reader. Its local
patch adds a `pict` handler so libheif can enumerate the containers. The matching
test constructs further ICC, geometry, auxiliary-alpha, density, malformed-input
and cancellation cases. Regenerate these files using an **absolute** output path:

```sh
LAYER_HEIF_FIXTURE_OUTPUT=/absolute/path/to/fixtures \
  cargo test --release -p layer-color rust_heif_write_validation_fixtures -- --ignored
```

`tools/validation/heif_decode_reference.c` is a separate libheif 1.23.4/libde265
oracle. It writes normalized RGBA16 or raw planar YUV16 reference samples. Native
RGB conversion can choose a higher intermediate precision/chroma interpolation;
the photograph regression compares all raw YUV samples exactly before the
application's own color conversion. To run it, set `LAYER_HEIF_REFERENCES` to the
pinned libheif source tree and `LAYER_HEIF_YUV_REFERENCE` to the oracle's `yuv`
output for `examples/example.heic`, then run:

```sh
cargo test --release -p layer-color rust_heif_photograph_matches_libde265_planes \
  -- --ignored --nocapture
```
