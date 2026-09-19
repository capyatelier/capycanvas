# Portable photo core migration

The shared Rust core must provide photo import, export and encoded previews
without external native codec libraries or helper executables. Integrate GTK
first, then Web and Android. Apple and Windows host integration is separate
future work. Platform UI and GPU APIs are outside the codec dependency removal.
Commit and push significant milestones to `origin/main`.

## Remaining work

- Replace native AVIF: rav1e/ravif encoding, Rust container and gain-map handling,
  upstream rav1d decoding. Validate actual WebAssembly decoding early. The initial
  rav1d 1.1.0 probe with assembly disabled fails on missing libc types on wasm;
  resolve portability before choosing the final decoder integration.
- Replace HEIC import with heif-oxide. Its current fidelity limitations are
  accepted for the initial integration; retain explicit capability reporting.
- Replace native Zstd in raster tile storage with a Rust implementation. Verify
  old `.capy` files, frame boundaries, integrity checks, compression and timing.
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
