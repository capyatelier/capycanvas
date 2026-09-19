# Portable photo core migration

The shared Rust core must provide photo import, export and encoded previews
without external native codec libraries or helper executables. Integrate GTK
first, then Web and Android. Apple and Windows host integration is separate
future work. Platform UI and GPU APIs are outside the codec dependency removal.
Commit and push significant milestones to `origin/main`.

## Remaining work

- Replace native AVIF export and encoded previews with rav1e/ravif and the Rust
  container/gain-map path. Rust AVIF import is integrated; qualify larger images
  and device latency alongside the remaining host work.
- Replace HEIC import with heif-oxide. Its current fidelity limitations are
  accepted for the initial integration; retain explicit capability reporting.
- Finish GTK integration and remove native codec build, bundle discovery and
  packaging requirements after the remaining format replacements work.
- Connect Web and Android imports, exports and previews to the shared codecs;
  test real browser/device operation, memory admission and cancellation.
- Audit target dependency graphs and preserve existing PNG/PQ PNG, TIFF, SDR
  JPEG, WebP, GIF, BMP, EXR and ICC behavior.

## JPEG milestone — 2026-09-19

HDR JPEG writing, reading and encoded previews now use libjpeg-turbo-rs 0.8.0
(the Rust codec), the existing Rust ICC implementation and a Rust JPEG container
adapter. No `heif` feature, helper process or codec bundle is required for JPEG.
GTK always offers HDR JPEG for float documents; AVIF availability is independent.
An unavailable AVIF preset falls back to HDR PNG instead of selecting OpenEXR.

The encoder renders the authored SDR rendition into an ICC-tagged BT.2020 base
with an sRGB transfer curve, encodes and decodes that base, then generates RGB
log gains against its actual compressed samples. The gain JPEG uses direct RGB
and full sampling at quality 100. MPF offsets and lengths, ISO 21496-1 fractional
metadata and Adobe gain-map XMP describe the two images. Both metadata forms
interoperate independently with libultrahdr.

The reader validates image boundaries and metadata before codec allocation,
supports reduced grayscale/RGB maps, reconstructs in linear application RGB,
then converts to linear sRGB half-float source samples. ISO metadata takes
precedence over legacy XMP. Exif orientation and print density are retained.
Encoded previews decode the completed output and show both reconstructed HDR
and the delivered SDR base. JPEG requires explicit flattening of transparency.

Current bounds: JPEG gain maps require matrix RGB ICC profiles and an SDR base;
HDR-base/backward gain maps are rejected. Codecs buffer complete images within
the memory admission budget. Cancellation is checked at rows and codec-stage
boundaries; the Rust JPEG library's individual synchronous encode/decode calls
do not expose an interruption callback. Further cancellation qualification is
part of the remaining platform work.

### Verification

- `cargo test --offline -p layer-color --no-default-features`: 93 passed,
  7 existing optional tests ignored. Covers authored SDR changes, low/high JPEG
  quality, ISO-only and XMP-only reopening, HDR samples, reduced grayscale maps,
  orientation, print density, flattening, cancellation, memory and malformed
  input rejection, plus the existing color and photo suite.
- `cargo check --offline -p layer-color --target wasm32-unknown-unknown`: passed.
  This verifies JPEG portability, not removal of the still-existing Zstd C
  dependency or completion of browser integration.
- `cargo check --offline -p layer-linux`: passed.
- The ignored `jpeg_interoperates_both_directions_with_libultrahdr` test passed
  against the pinned reference bundle from capycanvas3. Rust output decoded in
  libultrahdr through both independent metadata paths, and native output decoded
  in Rust. The largest sample error in that test was 0.1640625 at HDR intensity 8.
- The existing edited-HDR/authored-SDR JPEG and transparent AVIF regression
  passed. Largest JPEG absolute error was 0.11951733; AVIF remained 0.004032135.
- `portable_jpeg_gainmap_export_without_codec_bundle` passed on a private
  Mutter/Wayland display and NVIDIA Vulkan GPU with `CAPY_PHOTO_CODEC_DIR` set to
  an empty directory. It exercised the actual GTK selection, encoded preview
  toggle, file chooser, save, HDR reopen and unchanged document/history. Local
  screenshot: `artifacts/color-m4/gainmap-ui/jpeg-main.png`. This is a UI and
  interchange check, not a claim about physical HDR display output.

The legacy native AVIF/HEIC path and its packaging remain until their own
replacements are verified. The native JPEG routines are retained temporarily
as an independent test oracle alongside that path; production JPEG dispatch
always uses Rust.

## Raster storage milestone — 2026-09-19

Raster tiles use Rust Zstd (`zrip-core` 0.10.1, encode/decode 0.8.7) with the
bounds-checked `paranoid` feature throughout. The
[vendor patch](../../vendor/README.md#portable-zstd-raster-storage) fixes periodic
byte-plane compression, full-alphabet Huffman weights, and block table state.
Interactive capture uses a short-stride fast match search and raw literals;
imported source tiles use level 1 with entropy coding.

The `.capy` format, multibyte sample shuffle and SHA-256 tile identities remain
unchanged. Decode admits at most one tile, requires exactly one complete frame,
rejects dictionaries, malformed sizes and trailing/concatenated data, and keeps
the existing digest verification. Spare decode capacity is released before cache
admission so the cache's existing capacity accounting remains valid.

SHA-256 uses `sha2` 0.11, removing the former ARM `sha2-asm` build dependency.
Workspace content IDs retain their lowercase hexadecimal representation. Core
dependency graphs for WebAssembly and Android contain no `cc`, `zstd-sys`, or
`sha2-asm`. Native GTK/GPU APIs and the pending AVIF/HEIC codec bridge are outside
this storage milestone.

Verification:

- Core: 104 tests passed; workspace: 2 passed. Permanent C-generated fixtures
  cover U8, U16, F16 and F32 samples, both encoding policies and unchanged IDs.
- Color: 93 tests passed, 7 existing optional tests ignored.
- Vendored core: 59 tests passed, including full-alphabet Huffman tables.
- `layer-color` checks passed for `wasm32-unknown-unknown` and
  `aarch64-linux-android`.
- The [separate native oracle](../../tools/validation/portable-zstd/README.md)
  passed 256 mixed-entropy/boundary cases, 257 forced Huffman blocks, and all 936
  old frames from the local photo and 60 MP project fixtures. Both projects were
  read, written and reopened. C Zstd is confined to that validation workspace.
- The release GTK `portable_jpeg_gainmap_export_without_codec_bundle` journey
  passed on private Mutter/Wayland with NVIDIA Vulkan after the storage change:
  actual export selection, encoded SDR/HDR preview, save and HDR reopen, with
  an empty codec directory and unchanged document/history.

On this Linux host, synthetic tile encode/validation/shuffle/hash throughput was
about 392–2090 MB/s with Rust and 409–2356 MB/s with C using the same Rust hash.
U16 was the largest throughput difference (roughly 1.45–1.52 times the C time).
Source compressed sizes were within 2% of C for U8/F16/F32; U16 source grew 25%.
Interactive F16/F32 sizes improved; interactive U16 grew 45%. These measurements
describe this corpus and host, not browser/device drawing latency. No archive
migration or lossy sample conversion is involved.

## AV1 portability qualification — 2026-09-19

The [rav1d portability patch](../../vendor/README.md#av1-decoder-portability)
removes the initial WebAssembly compile failure from libc ABI aliases and makes
assembly build tools optional. With assembly disabled, the independent
[portable AV1 check](../../tools/validation/portable-av1/README.md) decodes
lossless AOM-generated 8/10/12-bit frames in native Rust and Chrome 152
WebAssembly. Every plane sample matches the known formula; the Wasm module has
zero host imports. Android target compilation also passes. These tests use a
single decoder thread and small bounded frames.

This qualification workspace is separate from application builds. AVIF
container parsing, alpha, geometry, NCLX/ICC, gain maps, encoding, large-image
memory admission and host integration still require implementation and tests.

## Shared AVIF import milestone — 2026-09-19

Application AVIF imports now use Rust BMFF parsing and the patched, assembly-free
rav1d decoder. AVIF import capability is always present, including when GTK has
no codec bundle. HEIC and AVIF writing still use the old adapters pending their
own replacements.

The reader retains 8/10/12-bit samples, straight alpha, ICC/NCLX and bitstream
color descriptions, crop/rotation/mirror geometry and Exif print density. Grid
tiles are joined in YUV before chroma interpolation, avoiding internal seams.
Sequences select the actual first color/alpha samples and their track metadata,
even with a different primary poster; grid tiles are excluded from collection
disclosure. Unsupported timeline edits and display scaling are rejected.

Preferred `tmap` alternatives reconstruct SDR-base HDR gain maps in linear
application RGB, using the same validated per-channel metadata and ICC matrix
transforms as JPEG. Reduced grayscale/RGB maps, distinct alternate primaries,
and alpha are supported. Reconstructed source samples use linear sRGB F16;
out-of-gamut negative RGB is retained. Unsupported gain-map versions leave the
SDR primary available; malformed fractions and inconsistent geometry fail.
HDR-base/backward maps and standalone PQ/HLG AVIF remain explicitly unsupported.

Container records, extents and references are bounded before allocation. AV1
sequence dimensions are checked before decoder allocation, including subsequent
sequence headers. Admission reserves encoded bytes, joined planes, source bands
and conservative single-thread decoder working/reference storage. This is an
admission estimate, not a replacement allocator enforcing a hard decoder quota.
Cancellation is checked during parsing, row conversion and codec-stage boundaries;
an individual synchronous rav1d decode does not yet expose mid-call cancellation.
Large-photo and physical-device latency qualification remain outstanding.

Verification:

- Color suite without native features: 98 passed, 10 optional tests ignored.
  With the legacy HEIC feature: 101 passed, 27 optional tests ignored, including
  missing-bundle capability behavior.
- Permanent synthetic fixtures independently verify exact high-bit-depth P3
  samples, alpha, geometry and density; three native gain-map references agree
  within 0.00049 in nonnegative linear RGB. Metadata corruption, truncated input,
  inflated counts, dimension limits, low budgets and cancellation/retry pass.
  Fixture generation and hashes are in
  [the fixture record](../../crates/layer-color/tests/fixtures/avif/README.md).
- The 22 external lossless still fixtures, photographic grids and three sequence
  references pass. The six existing AVIF regression tests pass through the new
  dispatch, including embedded ICC, bitstream-only P3, rotated alpha, unsupported
  geometry and PQ rejection.
- The photographic gain-map fixture matches the previous native import within
  0.00098 per linear RGB channel. The existing JPEG/transparent-AVIF edited HDR
  and authored SDR roundtrip test passes with native AVIF writing and Rust import.
- GTK, WebAssembly and Android target checks pass. WebAssembly/Android color
  dependency graphs contain no `cc`, `nasm-rs`, `zstd-sys`, `sha2-asm`, `dav1d-sys`
  or `libaom-sys`.
- [The integrated browser check](../../tools/validation/portable_photo.py) runs
  actual application dispatch, AV1 decoding, gain reconstruction, ICC and raster
  storage in Chrome 152 WebAssembly, with zero host imports. Exact SDR and HDR
  reference comparisons plus failed-budget/cancellation retry pass. This checks
  the shared core; browser UI and Android device integration are still pending.
