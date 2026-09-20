# Portable photo core migration

The shared Rust core must provide photo import, export and encoded previews
without external native codec libraries or helper executables. Integrate GTK
first, then Web and Android. Apple and Windows host integration is separate
future work. Platform UI and GPU APIs are outside the codec dependency removal.
Commit and push significant milestones to `origin/main`.

## Remaining work

- Qualify larger JPEG/AVIF images and device latency alongside the remaining
  host work. Both formats now import/export and preview through shared Rust.
- Audit target dependency graphs and preserve existing PNG/PQ PNG, TIFF, SDR
  JPEG, WebP, GIF, BMP, EXR and ICC behavior.

## Availability cleanup — 2026-09-19

The always-true gain-map availability functions, GTK's unreachable AVIF fallback
choices and duplicate native/portable GTK journey are removed. JPEG and AVIF are
ordinary built-in formats. This also removes unused-variable suppressions left
from conditional codec compilation. No old `.capy` or native-codec fallback is
introduced for compatibility.

The shared color suite passes (116 tests, 14 optional tests ignored), and GTK's
real JPEG/AVIF choice, preview, flattening, save and HDR reopen journey passes
with an empty codec directory on private Wayland/Vulkan. Evidence:
`/tmp/capy-portable-gtk-cleanup-ui.log`.

The separately selected 1031×1037 AVIF grid regression passes across partial
cells, alpha and gain-map boundaries. Running the release test binary directly,
excluding compilation, takes 13.15 seconds and peaks at 42,576 KiB RSS on this
Linux host (`/tmp/capy-portable-avif-grid-runtime.log`). This small working set
does not offset the slow encode/reopen time; stage profiling and larger-photo
latency remain open work.

## Android host milestone — 2026-09-19

Android offers JPEG and transparent AVIF gain-map delivery through the same Rust
codecs and GPU capture as GTK and Web. The old export/preset rejection is removed.
MIME types, suggested filenames and filename validation share one format mapping.
Quality, explicit clipping, JPEG background, size, density and saved presets use
the existing shared export recipe. Delivery leaves the editable master unchanged.

Output comparison encodes once, then retains the HDR reconstruction and actual
SDR base for instant switching. Both comparisons use a visible fixed image area,
including for small inputs. The existing cancellation handle governs capture,
codec boundaries and publication; cancelled exports do not publish partial files.

Every APK packages original Rust dependency and toolchain notices under
`assets/licenses`, using the shared renderer and the selected Android ABI graphs.
Missing Android binding notices are retrieved at their crates' published Git
revisions with pinned checksums. The old CESU-8 crate's complete original notice
and source-header attribution are retained directly from its registry archive.
No additional source code is vendored.

Device verification uses the separate `art.capycanvas.portablephoto` application
ID, keeping the normal application and its storage intact:

- `portablePhotoGainmapDelivery` passes on Wacom MovinkPad 14 and Huion Kamvas
  Pad 12: real Open of
  8/10-bit HEIC, 12-bit P3 AVIF and JPEG/AVIF gain maps; both export choices,
  encoded preview switching, OS file type/name, save, HDR reopen, unchanged
  master, JPEG flattening and AVIF transparency. Saved AVIF presets and
  in-flight cancellation/retry pass; cancellation drained in 343 ms on Wacom
  and 1,229 ms on Huion in these runs. The final APK's larger preview images
  were visually checked on Huion.
  The test supplies only the system chooser result; app capture, codec and
  provider publication execute normally.
- Existing device regressions pass for exact v6 project save/reopen, GPU
  replacement/recovery, export presets, 16-bit wide color, placement Apply/Cancel,
  clipboard, one-step history and malformed/cancelled/stale import rejection.
  Huion repeats the placement/retention journey with HEIC, 12-bit AVIF and HDR
  JPEG inputs rather than only the existing PNG fixtures.
- The independently encoded PQ PNG journey passes, including HDR painting and
  authored SDR, strict clipping rejection, PQ PNG/EXR/SDR delivery and reopen,
  recovery and Activity recreation.
- Android Rust/Kotlin builds and lint pass. The APK has only the application
  Rust library and AndroidX's platform graphics library; no photo-codec bundle.
  Android production color/storage graphs, with all features, contain no C codec,
  codec build or dynamic loader dependency. Original-notice generation for both
  arm64 and x86_64 passes, along with the 14 shared packaging tests and the
  repository license/source audit.

Local evidence: `artifacts/portable-photo/android-{wacom,huion}/files/`,
`/tmp/capy-portable-huion-{test,placement}.log`, and
`/tmp/capy-portable-android-{regressions,hdr-regression,placement,package}.log`.
These are functional device checks; larger-photo latency and peak memory remain
to be qualified separately.

## Web host milestone — 2026-09-19

The browser offers HDR JPEG and transparent HDR AVIF, using the shared Rust
codecs in its existing isolated file worker. Export uses a complete GPU snapshot,
the authored SDR rendition and the GPU illumination guide. Saved presets, MIME
and filename handling, quality, resizing, density, JPEG background choices, and
explicit clipping apply to these formats. Delivery leaves the master unchanged.

Encoded previews share one completed encoding and let the user switch between
reconstructed HDR (mapped for the SDR preview) and the actual encoded SDR base.
The small preview canvases keep CPU backing so offscreen comparisons remain
visible. Per-operation gain-map options carry the browser's explicit encoding
and decoding memory budget through both codecs. Cancellation terminates the
isolated worker, including during synchronous codec work, and cleans its OPFS job.

Verification with the production PWA on Chrome 152, private Mutter/Wayland and
NVIDIA WebGPU:

- `--portable-photo`: real Open of 8/10-bit HEIC and 12-bit/HDR AVIF; both JPEG
  and AVIF export options, encoded preview switching, file picker types, save,
  HDR reopen, unchanged source/history, JPEG flattening and AVIF coverage.
  Saved gain-map presets, in-flight cancellation/retry and OPFS cleanup pass.
- Independent browser SDR decoding differs by at most 1 premultiplied byte code
  for JPEG and 2.08 for AVIF. The comparison uses `createImageBitmap` with
  `premultiplyAlpha: 'none'`: Chrome's default Image decoding prematurely rounds
  premultiplied wide-gamut values to eight bits. Independent libavif RGBA16 and
  matrix conversion agree with the Rust preview within one visible byte code.
- `--image-placement`, supplied JPEG HDR, HEIC and 12-bit AVIF: Open, multi-file
  import, clipboard, canvas/group drops, placement Apply/Cancel, Undo/Redo,
  exact source backing after v6 save/reopen and GPU replacement, and
  malformed/stale/cancelled request rejection all pass.
- `--raster`: ordinary PNG delivery, exact project save/reopen, corruption,
  GPU replacement and recovery after reload pass after the LZ4 change.
- Shared color suite: 116 passed, 14 optional tests ignored, including explicit
  gain-map encode/decode budget admission. GTK and Wasm checks and the original
  license/source audit pass. The packaged application contains no codec helper.

Local evidence: `artifacts/portable-photo/web/report.json`, JPEG/AVIF preview
screenshots and saved exports; `/tmp/capy-portable-web-placement.log`. These
small-fixture journeys do not qualify large-photo latency or physical HDR display.

## GTK packaging milestone — 2026-09-19

The GTK staging script and Arch recipe no longer build, discover or ship a native
photo-codec bundle. Staging clears obsolete payloads from an owned generated
directory and rejects unmarked directories and symlink outputs. Final package
validation rejects the old codec directory, helper executables and codec libraries.
The pinned GTK runtime remains a platform dependency with its original notices,
corresponding source and rebuild recipe.

The C/C++ bridge, HDR helper, libheif patch and pinned build recipe now live under
`tools/validation/photo-codecs/`. Independent Rust interoperability tests use the
explicit `native-codec-reference` feature and `CAPY_PHOTO_CODEC_DIR`; executable-
relative bundle discovery is removed. `libloading` is a Linux test dependency
only, including when all production features are selected.

GTK and Web share original Rust notice harvesting/rendering under `tools/build/`,
with each package selecting its actual target. GTK now includes both original
Rust dependency notices and the installed Rust toolchain's complete notice.
The Arch recipe declares the GTK build tools/headers and notice generator.

Verification:

- The complete release GTK package builds without a photo-codec bundle. Original
  Rust codec notices and all packaged GTK manifest hashes were checked.
- A relocated copy under a path containing spaces opens three HEIC inputs
  (8-bit rotated P3 grid, 10-bit P3, and a 1280×854 photograph), 12-bit P3 AVIF,
  HDR gain-map JPEG, transparent HDR gain-map AVIF and TIFF. All seven captured
  images were visually checked on private Mutter/Wayland and NVIDIA Vulkan.
  `/proc` sampling confirms the relocated GTK library and no removed application
  codec libraries. Evidence is under `artifacts/portable-photo/gtk-package-rust/`.
  This checks package startup/photo display; its fixed capture delay is not codec
  throughput, and the SDR display captures do not qualify physical HDR output.
- Shared photo tests: 118 passed, 27 optional tests ignored. Both separately
  selected JPEG/libultrahdr and AVIF/libavif interoperability tests pass using
  the explicit reference directory after the feature/dependency changes.
- GTK packaging tests: 3 passed; Web packaging tests: 14 passed after integrating
  current `origin/main`. Web notice generation/rendering retains every Rust photo
  and storage codec's original license. Repository license/source audit passes.
- Linux, WebAssembly and Android production `layer-color` graphs, including all
  features, contain no native codec/C build/assembler dependency or `libloading`.
  Python validation tools compile and the Arch shell recipe passes syntax checks;
  an actual Arch `makepkg` build is not qualified on this Fedora host.

## HEIC import milestone — 2026-09-19

HEIC is now an unconditional shared-core import capability. GTK no longer enables
the `heif` native feature, and neither application decoding nor file-picker
availability uses a codec bundle. The old bridge is compiled only in optional
interoperability tests; its packaging/build cleanup is recorded above.

The application uses heif-oxide 0.1.0's patched low-level HEVC adapter around
rust_h265 0.1.0. Shared BMFF parsing, sequential grid assembly, source ICC/NCLX,
orientation, print density and source storage remain in the Rust core. This path
does not call the convenience API's sRGB conversion or threaded grid decoder.
VUI color/range and chroma siting survive decoding; container color declarations
take precedence. Supported auxiliary alpha retains coded coverage values.

The vendor patch adds header/syntax bounds, expected-dimension and working-memory
admission before picture allocation, independent-still validation, and borrowed
cancellation checks between NALs, coding-tree blocks and filter stages. The
adapter preflights length-prefixed NALs, including embedded start-code rejection,
before Annex B parsing. Individual in-loop filters remain synchronous, and the
conservative working-set estimate is admission rather than a hard allocator quota.

Initial HEIC limitations remain explicit: sequences, monochrome/4:4:4 HEVC and
unsupported color/alpha representations report errors. Direct PQ/HLG sources
still require an SDR conversion. HEIC HDR auxiliary gain maps are not added by
this milestone. Verification covers 8/10-bit 4:2:0; 12-bit HEVC is not qualified.
These are the accepted initial HEIC codec limits, not claims of complete HEIF
format conformance. Larger photographs and real Android device behavior remain
part of the host/performance work above.

Verification:

- Release shared color/photo suite: 115 passed without native features; 118
  passed with the optional oracle feature. 14/27 optional tests remain ignored.
- Vendor suites: 128 HEVC tests and 35 HEIF tests passed, including independent
  decoder hashes, truncated header reads and VUI source metadata.
- A 1280×854 photograph matches libde265 exactly across all 1,639,680 YUV samples.
  The complete importer retains the primary-image disclosure. An unsupported
  external alpha fixture reports its representation error instead of dropping
  transparency. Native comparison is isolated in
  `tools/validation/heif_decode_reference.c`.
- Application cases cover all quarter turns and mirrors, P3, embedded ICC,
  10-bit normalization, odd grid edges, alpha, density, memory admission and
  cancellation/retry. Synthetic fixture provenance lives under
  `crates/layer-color/tests/fixtures/heif/`.
- GTK Open/Import/Paste, thumbnails, retained project save/reopen and history
  passed with an empty codec directory on private Mutter/Wayland and NVIDIA
  Vulkan. The final three-fixture journey took 9.27 seconds. Captures are under
  `artifacts/portable-photo/heif-ui/`; the rotated grid was visually checked.
- Chrome 152 executes three HEIC fixtures alongside the JPEG/ICC/storage and
  AVIF import/export checks. Its only Wasm host import initializes the bindgen
  reference table; no host codec provides pixels. Virtual-time timings are not
  device performance measurements.
- Android compilation and WebAssembly/Android production codec graphs pass
  without C codec/build/assembler dependencies. License/source auditing and Web
  packaging tests pass. The Web notice generator now retains vendored third-party
  notices as well as registry dependencies, including both original HEIC notices.

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

## Raster storage simplification — 2026-09-19

The initial Zrip migration in `5ba9d1a9` preserved Zstd archives by maintaining
local encoder fixes. That decision is superseded: `.capy` compatibility is not a
requirement, and simplicity takes precedence over compression ratio.

Raster storage now uses unmodified crates.io `lz4_flex` 0.14.0, with `std`,
`safe-encode`, `safe-decode`, and `checked-decode`; framing and its optional hash
dependency are disabled. Painted and imported tiles share one encoder. The Zrip
vendor trees, patches, decoder dependency, native Zstd oracle, old fixtures,
frame parser and separate source-compression policy are removed.

Only `.capy` version 6 is read or written. It fixes the tile codec to LZ4 blocks;
there is no migration reader or v4/v5 writer selection. Multibyte shuffling,
SHA-256 sample verification, placement, source profiles and immutable backing
remain. The descriptor sets the exact output allocation (at most 1 MiB); the
library's maximum encoded size also governs archive and worker admission.
SHA-256 still uses `sha2` 0.11 without the former ARM assembly dependency.

Before replacement, a local release comparison fed identical shuffled tile bytes
to LZ4 and both patched Zrip policies, and verified every decoded byte. These are
codec-only timings on this Linux host, excluding validation, shuffling and hashing.

| Corpus / codec | Stored bytes | Encode MB/s | Decode MB/s |
| --- | ---: | ---: | ---: |
| Photo, 8 tiles / LZ4 | 70,499 | 7,639 | 3,470 |
| Photo / Zrip capture | 65,795 | 4,001 | 4,014 |
| Photo / Zrip source | 29,176 | 2,556 | 3,368 |
| 60 MP, 928 tiles / LZ4 | 549,970,843 | 5,378 | 5,881 |
| 60 MP / Zrip capture | 546,152,132 | 3,333 | 4,837 |
| 60 MP / Zrip source | 492,886,175 | 568 | 1,466 |

The 60 MP corpus gains encode speed at a 0.7% size increase over capture and
11.6% over source compression. The small periodic photo is 2.4 times the previous
source size, an increase of 41 KiB. These fixtures are not a general ratio or
device-latency guarantee. Local comparison: `/tmp/capy-compression-choice.log`.

Verification:

- 104 core and 2 workspace tests pass, including exact integer/float samples,
  malformed blocks, file integrity, source/profile ownership, spill and history.
  The color suite also passes.
- GTK, WebAssembly and Android NDK target builds pass. Core's dependency graph
  contains neither Zrip nor a native compression dependency. License/source
  policy checks pass; the packaged Web notices include LZ4's original MIT text.
- The release GTK rasterization journey passes on private Mutter/Wayland and
  NVIDIA Vulkan: paint, placed off-canvas sources, masks, Undo/Redo and v6 reopen.
  Local log: `/tmp/capy-lz4-raster-ui.log`.
- The packaged Web raster journey passes with actual WebGPU, codec workers and
  IndexedDB: exact PNG pixels after save/reopen and GPU replacement, Undo/Redo,
  corrupt-file retention, and recovery after reload. Local log:
  `/tmp/capy-lz4-web-raster.log`. Android runtime storage remains to be exercised
  during its host integration; target compilation is not device qualification.

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
- After merging current `origin/main`, the color suite passes 102 tests with
  the legacy HEIC feature and GTK compilation still passes. The repository
  license/source check passes with rav1d's BSD-2-Clause notice retained and a
  pinned CC0-1.0 exception for its `to_method` 1.1.0 dependency.
- [The integrated browser check](../../tools/validation/portable_photo.py) runs
  actual application dispatch, AV1 decoding, gain reconstruction, ICC and raster
  storage in Chrome 152 WebAssembly, with zero host imports. Exact SDR and HDR
  reference comparisons plus failed-budget/cancellation retry pass. This checks
  the shared core; browser UI and Android device integration are still pending.

## Shared AVIF export milestone — 2026-09-19

AVIF gain-map writing and encoded HDR/SDR previews now use the shared Rust core.
GTK always offers HDR AVIF for float documents, including transparent documents,
without a helper executable, native codec library, or codec bundle. The native
gain-map adapter is compiled only for independent interoperability tests; native
HEIC import and its packaging remain pending their own replacement.

The encoder retains the existing 12-bit BT.2020/sRGB-transfer base, full-resolution
RGB gain map, straight alpha, ISO tone-map metadata, and Exif print density.
rav1e 0.8.1 runs without its default/native build features for quality 1–99.
oxideav-av1 0.1.16 encodes the quality-100 base, alpha, and gain map losslessly;
independent rav1d checks require every 12-bit plane sample to match. Its AV1
sequence color description is rewritten to match the full-range container,
including full-range monochrome alpha. Both Rust and native libavif read it.

Gains are calculated against the decoded compressed SDR base, preserving the HDR
master even at low SDR quality. The authored local tone guide and rendition feed
the SDR base. Previews decode the finished file in HDR and SDR modes, retaining
transparency. One-cell and multi-cell grids trim repeated-edge padding, with
64-pixel minimum coded grid dimensions for MIAF interoperability. Exif describes
the base directly so readers with a single metadata association retain density.

Memory admission reserves the master/base planes, retained compressed packets,
container copy, and one encoder working set. Export checks cancellation at rows,
cell boundaries, codec stages, and output chunks. Individual codec calls remain
synchronous. Cells normally cap at 256 pixels; large images use 512/1024 to stay
within the 4096-item reader limit. This is conservative admission, not a hard
allocator quota; large-photo/device performance and cancellation latency still
need qualification.

Verification:

- Release color/photo suite: 107 passed with no native features; 110 passed with
  the legacy HEIC feature. The latter leaves 24 optional tests ignored.
- Nine exports cover quality 25/90/100, odd 23×17 padding, direct 32×24 coding,
  and partial 263×65 grids. Maximum HDR reconstruction error is 0.00390625 at
  intensity 4. Alpha is exact at the declared 12-bit precision.
- Independent libavif/dav1d reads all nine exports, their gain maps and print
  density. SDR RGBA16 differs by at most one normalization rounding step; HDR
  reconstruction differs by at most 0.00390625. The permanent optional oracle
  test is `rust_avif_output_interoperates_with_native_libavif`.
- The existing 1031×1037 partial-grid/alpha/gain regression passes in release
  mode (12.34 seconds on this Linux host). This is not a device throughput claim.
- `portable_gainmap_export_without_codec_bundle` passes on private Mutter/Wayland
  and NVIDIA Vulkan with an empty codec directory: actual GTK choices, encoded
  preview switching, save, HDR reopen, JPEG flattening, and unchanged history.
- Chrome 152 executes both AVIF encoders, padded grids, HDR reconstruction and
  alpha through the application API. The validation harness uses wasm-bindgen
  packaging because v_frame exports enum bindings. Its only host import initializes
  wasm-bindgen's reference table; no host codec or pixel conversion is supplied.
- Android compilation and WebAssembly/Android dependency graphs pass with no C
  codec, assembler, C build, or fuzz-harness dependency in production. Repository
  license/source checks pass. Original encoder licenses and the rav1e patent
  notice are retained; Web packaging retrieves missing proc-macro notices from
  pinned upstream revisions.
