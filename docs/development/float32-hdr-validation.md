# Float32 HDR implementation and validation

Implementation: `537464b7`, with shutdown correction `d21ea159` and concurrent
GTK work integrated at `62ae6640`. Development started from `origin/main` at
`59aff3df`. Final release builds and native measurements below use the integrated
`62ae6640` production code. Float32 is a document storage precision,
using the existing linear RGBA32Float working renderer and 203 cd/m² reference
white. The implemented interactive host is GTK/Linux. The shared core and explicit
C creation API support Float32; the other hosts keep their existing HDR rejection.
A successful cross-compile does not qualify those hosts for HDR editing.

Subsequent Windows implementation and functional qualification are recorded in
[Windows feature parity progress](windows-feature-parity-progress.md#windows-hdr-implementation-and-verification-2026-09-19).
The host-status statements below describe this earlier GTK qualification; physical
HDR and Windows performance acceptance remain unqualified.

## Data contract

- Editable color tiles use little-endian IEEE binary32 straight RGBA. Placed
  originals retain their own interpretation. Masks and both wetness planes remain
  unsigned integer16, as with Float16 HDR.
- RGB may be signed and above reference white, up to finite binary32 range.
  Alpha is finite linear coverage in `[0, 1]`. NaN, infinity and invalid alpha
  are rejected at ingestion and before committing GPU edits. Unassociation
  overflow also fails before publishing the affected edit.
- Native archives, undo/redo and recovery preserve the committed sample bytes,
  including subnormals, signed zero and hidden RGB in imported zero-alpha samples.
  The archive's reversible byte shuffle now covers 32-bit samples. A complete
  color tile has 1 MiB of uncompressed samples, and pending history charges that
  actual precision against its existing budget.
- GPU editing uses premultiplied Float32 arithmetic. An edited zero-alpha pixel
  is canonical transparent black. Premultiplication, unassociation and other
  arithmetic can round; GPU arithmetic is not a bit-preserving transport for
  subnormals. Native storage preservation does not promise exact arithmetic on
  every GPU or safe intermediate arithmetic for every finite input. WGSL permits
  flushing subnormal arithmetic to zero ([floating-point evaluation rules](https://www.w3.org/TR/WGSL/#floating-point-evaluation)).
- Float16 promotion preserves its finite samples exactly; it does not recover
  precision. Explicit demotion rounds to the requested storage and rejects
  unsupported ranges atomically. Cancellation does not replace the original.
- Authored colors retain exact linear RGB when transfer encoding would lose
  information. This is necessary for typed values, sampled colors and workspace
  serialization, even though the ordinary picker still displays encoded RGB.
- Color validation, intensity, histogram bins and bundled exposure/HDR-curves
  controls use the selected precision. Existing Float16 storage validation stays
  in place. There is no alternate tone mapper or floating mask format.

## OpenEXR interchange

The bounded codec accepts single-part, flat, increasing scanline RGB or RGBA with
full-resolution FLOAT channels and matching data/display windows. Supported
lossless compression is NONE, RLE, ZIP1 and ZIP16. Output uses FLOAT RGBA and ZIP1.
HALF/UINT channels, extra channels, deep, multipart, tiled and other compression
modes return errors. Arbitrary primaries and adopted-neutral rendering are also
rejected, rather than relabeled as a supported space.

Recognized chromaticities are sRGB/Rec.709, Display P3, Adobe RGB and ProPhoto.
Missing chromaticities assume Rec.709. Missing `whiteLuminance` means the
application's 203 cd/m² reference; an explicit positive white luminance is
normalized to that reference. Output writes chromaticities and 203 explicitly.
Pixel density is carried through `xDensity` and `pixelAspectRatio`; floating input
densities are converted to bounded rational metadata.

OpenEXR RGB is premultiplied, following its [alpha convention](https://openexr.com/en/latest/TechnicalIntroduction.html).
Import unassociates into native straight RGB with finite-range validation. Nonzero
emission at zero alpha cannot be represented by this layer model and is rejected.
Export writes the premultiplied rendered composite. FLOAT channel storage is
lossless, but alpha association, reference-white normalization, effects and
resampling can round; EXR is not an exact archive of hidden or layered samples.
Use `.capy` for that contract.

The decoder preflights attribute lengths before codec allocation, caps headers at
1 MiB, and checks the source and codec budgets. Processing retains one source
256-row band plus bounded codec/row scratch; export streams scanlines. Both
paths propagate errors/cancellation. GTK publishes successful output atomically.

“Further editing” chooses EXR for floating documents. PQ PNG and the saved SDR
rendition remain deliberate delivery formats. EXR keeps document primaries and
alpha, with no tone mapping, dither or implicit half-float conversion. The existing
SDR rendition retains its 16-stop range control; its automatic range fit reports
an error beyond that range. Float32 storage does not expand PQ delivery
luminance or change the saved SDR appearance algorithm.

## Build and automated checks

Broad shared-code and renderer suites were run for the Float32 implementation
milestone. The final integrated build received the focused regressions listed
below, plus repeated native performance workloads.

- `cargo build --locked --release -p layer-linux`: runnable GTK executable at
  `target/release/layer-linux`.
- `cargo test --locked -p layer-core -p layer-color -p layer-engine -p layer-ui`:
  core 96, color 88, engine 63, and UI 480 passing tests; seven opt-in codec
  tests skipped. The subsequently added embedded-effect range/history test also
  passed, giving 481 distinct passing UI tests.
- Hardware renderer suite: 296 passed, 30 opt-in benchmarks/profile tests
  skipped. The added Float32 EXR/PQ/SDR delivery regression passed separately.
  This includes native publication, exact archive/history/restoration across
  all four depths and RGB spaces, low-alpha effects, mask storage, cancellation,
  invalid late tiles, source caches, proof/view and ordinary brush regressions.
- C API: 13 passed, including explicit Float16/Float32 construction and ABI
  validation. Renderer integration suites: four contact and six project tests
  passed, including every contact preset through archive/history operations.
- GTK release unit tests: 20 passed, 174 native/opt-in tests skipped after
  integration. The two explicit native Float32 tests passed: create/open/edit/save/EXR export through
  real widgets, and injected GPU failure followed by restoration and continued
  editing. These functional runs are separate from the timing observations.
- Workspace tests: one passed. Host tests: 27 passed and one opt-in test skipped.
  `snapshot::tests::workspace_updates_retain_models_and_match_authoritative_layout_and_history`
  fails identically on clean `59aff3df` (row y=400 versus expected y=48). It was
  excluded from the remaining host run; no drag behavior was changed.
- `cargo check --locked -p layer-web --target wasm32-unknown-unknown` passed.
  Linux-target checks of `layer-android`, `layer-apple`, and `layer-windows` passed;
  these are shared/native library build checks, not target-device qualification.
- After integrating concurrent GTK work, four focused hardware renderer tests
  passed: Float32 shared snapshot boundaries/EXR, masked-effect column boundaries,
  band budget/cancellation, and Float32 EXR/PQ/SDR delivery. All three EXR codec
  tests passed again. The three native GTK checks (Float32 create/open/edit/export,
  GPU recovery, and closing during Float16/Float32 analysis) passed in separate
  processes on the rebuilt executable.
- `git diff --check` passed.

Raw native logs, timing JSON and memory samples are in the ignored
`artifacts/float32/` directory. The test binary is built before timing and
performance workloads run serially without other GPU tests or compilation.
Final release executable SHA-256:
`3c58d4fc0d8d719deb5a4de012e33a25bcb51c403226b27be08fac20374ae58b`.
Final GTK test executable SHA-256:
`9a01af74cd8f22f8e97eb5d3e48c1d8baa9ee5246be1411bb1983577710c0bd7`.
The final native checks and repeated performance workloads are retained under
`artifacts/float32/merged/`. Earlier navigation observations used test SHA-256
`5c786f027dadf85304184b7b2a24c2991f5315495763b0ef2cd31620028847c4`;
the first successful 100-stroke shutdown-fix run used
`1eb1c20db388ab06f1fa0eb7d6e22f6415b61be6048f5f3aed5aebd197bc5e7f`.

## Shutdown regression found during qualification

The initial 100-stroke test completed its assertions and measurements, then the
process crashed during driver teardown. The core dump showed the local SDR
analysis worker issuing a native texture copy while the main thread ran NVIDIA
exit handlers. This was an asynchronous worker lifetime issue, not a failed
sample-preservation assertion.

Analysis cancellation now follows the canvas's existing unrealize/shutdown hook,
with resume on map. An application hold keeps the process alive until the GPU
snapshot worker and its owned resources finish. The native harness, which drives
the main context without `Application::run`, explicitly drains that worker before
returning. A regression closes Float16 and Float32 windows while an actual GPU
analysis job is pending, checks cancellation, and waits for completion. That test
passed. The original failed run and backtrace are retained in
`artifacts/float32/painting-before-shutdown-fix/`.

## Isolated native measurements

Single runs on the final integrated build and environment below, at 120 Hz with
no concurrent builds or other GPU tests. Fresh processes reuse persistent shader
caches; startup timings are not cold-machine measurements. Navigation sends 960
synthetic requests; the test requires zero source decoding or recomposition once the complete display cache is ready.
All five runs passed that assertion. Percentiles use requests matched to actual
presentation feedback; coalesced/unmatched requests remain visible in the counts.
These observations do not establish a hard 120 Hz latency guarantee.

| Workload | p95 ms | p99 ms | Max ms | Matched requests | Peak RSS MiB | Sampled GPU peak MiB |
|---|---:|---:|---:|---:|---:|---:|
| Float32 24 MP, 5 adjustments | 13.59 | 13.75 | 18.16 | 959/960 | 1072 | 876 |
| Float32 45 MP, 5 adjustments | 7.65 | 7.91 | 16.51 | 959/960 | 1365 | 1365 |
| Float32 60 MP, 5 adjustments | 10.93 | 11.65 | 24.03 | 959/960 | 1679 | 1661 |
| Float16 60 MP control | 9.69 | 10.04 | 22.76 | 958/960 | 1134 | 1661 |
| Float32 60 MP, 29 adjustments, concurrent save/export | 10.39 | 10.80 | 27.12 | 943/960 | 2645 | 1661 |

Float32 startup-to-ready was 1.59/2.06/2.97 seconds for 24/45/60 MP, and 2.65
seconds for the long chain. In the 60 MP control comparison, renderer-accounted
resident GPU allocations were 1360.69 MiB for Float16 and 1361.19 MiB for Float32;
the large working/display planes already use Float32. Process RSS grew by about
546 MiB in this fixture. The timings vary with scheduling and presentation phase;
a single control run does not show a general speed advantage for either depth.

Concurrent native save completed in 212 ms; save, reopen and EXR export completed
in 17.70 seconds. The native archive was 489,911,172 bytes and EXR was 692,947,857
bytes. Peak process RSS was about 2.58 GiB. Export remained a separate immutable
snapshot while camera input continued.

The final 4096×4096 Float32 painting run used 32 paint layers and 100 Palette
Knife strokes (720 px, 800 ms per contact). Every pen-up produced one commit;
all 100 revisions reached host backing, and the process exited successfully.
Pen-up to the first presented frame containing that commit was **9.95 ms p95,
10.61 ms p99**. This includes later presented frames when the terminal frame was
coalesced; using only directly matched terminal frames would retain just 67/100
samples and understate latency. Background host backing completed in **57.53 ms
p95, 59.49 ms p99**. Peak RSS was 941 MiB and sampled GPU residency 985 MiB.
The run lasted 83.10 seconds and required no synchronous backing wait between
strokes. These are software-event measurements, not physical pen latency.

### Reproduction

Build the release GTK test executable with
`cargo test --locked --release -p layer-linux --no-run` and substitute the printed
executable path below. `hdr-memory.py` requires NVIDIA NVML through `nvidia-smi`;
the underlying `gtk-raster.sh` can run functional tests without that sampler.

```sh
LAYER_NAVIGATION_HDR=32 LAYER_NAVIGATION_PHOTO=60mp LAYER_NAVIGATION_COMPLETE=1 \
  python3 tools/performance/hdr-memory.py TEST_BINARY native_large_photo_navigation artifacts/float32/merged/navigation-60mp
LAYER_NAVIGATION_HDR=32 LAYER_NAVIGATION_PHOTO=60mp LAYER_NAVIGATION_COMPLETE=1 \
  LAYER_NAVIGATION_LONG_CHAIN=1 LAYER_NAVIGATION_CONCURRENT=1 \
  python3 tools/performance/hdr-memory.py TEST_BINARY native_large_photo_navigation artifacts/float32/merged/navigation-60mp-concurrent
LAYER_DRAWING_HDR=32 LAYER_PENUP_STROKES=100 \
  python3 tools/performance/hdr-memory.py TEST_BINARY native_penup_and_following_strokes artifacts/float32/merged/painting
```

Use `24mp` and `45mp` for the other sizes, and `LAYER_NAVIGATION_HDR=1` for the
Float16 control. `tools/performance/photo-navigation-report.py` summarizes the
navigation JSON without dropping unmatched requests or maximum stalls. Sampled
RSS/NVML data is in each `.memory.json`; raw logs and `/usr/bin/time` observations
are adjacent. `artifacts/float32/checks/` retains the automated test/build logs.

## Qualification limits

The native test environment uses NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation
Edition, Vulkan driver 610.57.04, and a private Mutter Wayland 1600×1000 120 Hz
virtual monitor, GTK 4.22.4 and Rust 1.96.0. Synthetic GTK requests measure
request-to-presentation latency, not physical pen/touch latency or physical HDR display accuracy. The virtual
monitor reports SDR headroom. Float32 filtering and blending remain required by
the native renderer; an unavailable capability returns an error without narrowing
storage.

The large-photo workload contains one populated source layer, 31 empty paint
layers, a background and five adjustments. The long-chain variant adds 24
adjustments. This is not a measurement of 32 densely painted full-frame layers.
NVML/RSS observations are supplemental samples and may miss brief allocation
peaks. Renderer accounting and OS/driver residency describe different quantities.

Browser/device runtime qualification, physical HDR display verification, mobile,
Apple and Windows HDR product paths, and physical pen/touch latency remain release
limitations. RAW, PSD, native CMYK, OCIO/ACES and deep EXR remain outside this work.
