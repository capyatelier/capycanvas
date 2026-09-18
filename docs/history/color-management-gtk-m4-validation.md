# GTK HDR editing and delivery — review candidate

2026-09-17. Implementation starts at origin/main `173f760f`, in the isolated
`color-management-m4` branch. The original checkout and its uncommitted work were
left intact. `1c0f531f` repairs the parent's GTK rasterization caller so the parent
can build. `11cc3740` adds the shared HDR architecture and GTK journey. Subsequent
qualification fixes include Chrome's stricter WGSL parsing and HDR preferences.
No remote push precedes user feature feedback.

The user deferred feature review on 2026-09-17 and requested that independent
work finish before stopping. The runnable build, tests and local commits are
ready for that later review; feature approval has not been obtained. Work stops
here without a push. Build hashes and the final source commit are recorded in
`artifacts/color-m4/review/build-manifest.json`.

This record qualifies the **GTK/Vulkan editing and mapped-SDR delivery envelope
below**, with an interoperable PQ route. It does **not** mark the entire
cross-platform phase 4 complete: physical HDR displays, mixed-display moves,
calibrated print comparisons and constrained/mobile hardware remain unqualified.
The [contract and budgets](../development/color-management-m4-design.md) were
written before accepting the numerical and HDR performance results. The actual
code audit is there; historical milestones were not treated as implementation.

## Delivered contract and workflow

- RGB 1 means 203 cd/m², fixed independently of the monitor. Canonical RGB is
  straight linear binary16, finite [-65504,65504]; alpha is binary16 [0,1].
  Processing/compositing uses Float32 with premultiplied alpha. Scalar masks and
  wetness remain UNORM16. Edited transparent pixels become transparent black;
  retained source data can preserve hidden RGB. Nearest-even half rounding occurs
  at publication; invalid/overflow edits fail whole-batch validation. No Float32
  document mode or memory-pressure precision fallback was introduced.
- New/Change Bit Depth exposes 16-bit float HDR. Exposure is linear; Curves
  defaults to a Linear HDR domain with an adjustable stops-above-white extent.
  Numeric Linear RGB/HDR entry and the actual eyedropper retain extended values.
  Histograms count full-resolution linear HDR samples in -12…+16-stop bins, with
  explicit nonpositive and above-reference-white counts. The graphical color
  wheel retains its SDR range; numeric entry supplies HDR colors.
- SDR Rendition saves exposure, contrast and highlight shoulder as one reversible
  document edit. Preview on SDR Display is transient and leaves the master clean.
  The same mapping feeds SDR presentation, thumbnails/previews, SDR exports and
  mapped ICC proof input. On an SDR fallback display the toggle has the same
  appearance as the normal view, which already uses that rendition.
- Native masters retain half bytes, retained originals, editable layers, proof
  configuration and rendition settings through save/reopen, undo and recovery.
  Floating flattened copies and source rasterization retain HDR range. Direct
  HDR-to-integer document reduction is rejected; export an SDR rendition instead.
- Input: noninterlaced 16-bit RGB/RGBA PNG, full-range RGB cICP PQ, with sRGB,
  Display P3 or BT.2020 primaries. Input normalizes to linear sRGB half samples.
  Output: 16-bit BT.2020 PQ PNG, tagged cICP [9,16,0,1], with straight alpha.
  Strict output rejects negative/out-of-gamut BT.2020 or >10,000-nit channels.
  The separately selected “map to PQ range” option deliberately clamps these
  delivery channels and reports the count. Neither modifies the master.
  SDR PNG/TIFF/JPEG retain their existing profile/depth/size controls.
- HLG, gain-map JPEG, HDR HEIF/AVIF, floating TIFF/EXR and interlaced/8-bit PQ PNG
  remain explicit unsupported paths. No gain map is reused or emitted. RAW,
  layered PSD, native CMYK documents, OCIO/ACES and full Float32 are outside scope.

## Display, jobs and recovery

GTK owns timing, presentation and input. Rust retains sample validation, editing,
history, capture, mapping and delivery. One cancellable decoder/export worker
owns each request; row/band/tile processing and shared immutable backing avoid a
second full floating document. Cancellation is acknowledged before successors;
atomic file publication cannot race an accepted cancellation. Histogram and
proof preparation keep their existing bounded asynchronous paths.

The optional Wayland Windows-scRGB route requires explicit compositor support
and a floating pass-through swapchain. RGB is scaled by 203/80 for that protocol.
Asynchronous preferred-description feedback has one request in flight; a monitor
change immediately falls back to headroom 1 until a current description arrives.
Signed scRGB channels preserve wide-gamut coordinates. The compositor peak is a
capability hint, not a calibrated measurement. Unknown/unsupported capabilities
use the authored SDR mapping with an explicit footer status. Initial SDR canvases
converted to HDR retain their SDR surface until reopened; data remains HDR and
the footer reports mapped SDR. That presentation limitation is deliberate.

The tested Mutter outputs did not advertise Windows-scRGB. Thus actual native
HDR luminance and monitor migration remain outstanding even though Float32 GPU
scRGB presentation, reference-white scaling and proof mapping match CPU references.
Other hosts reject HDR document/source adoption before replacing a live SDR
document; this is an explicit limit, not silent down-conversion.

## Correctness and real application evidence

Raw artifacts live in `artifacts/color-m4/` in the original checkout. Test logs
include earlier failures as well as the final passing runs.

| Check | Result and evidence |
| --- | --- |
| Shared suites | Core 86, color 76, UI 453 pass; 7 pre-existing color tests ignored. Added HDR source-rasterization test also passes. `shared-tests.log`, `hdr-rasterization.log`. |
| Host compilation | `cargo check --offline --workspace --all-targets` passes on Linux; actual Wasm release build passes. This is not an Apple/Windows/device build qualification. |
| Canonical storage | All finite half codes including subnormals, low/zero alpha, overflow rejection and canonical cache publication pass on the actual GPU. Archive bytes/rendition metadata and one-step history round-trip exactly. |
| Editing and boundaries | Four working RGB spaces, HDR curves/exposure, fused and physical passes, native paint/undo/save/reopen/device replacement pass. Float32 operation tolerance is 2e-6 absolute + 2e-5 relative; half publication uses nearest-even. |
| Presentation/proof | CPU/GPU SDR mapping, Windows-scRGB 203/80 scaling and mapped proof agree within the declared Float32 tolerance. Display operations leave raw artwork unchanged. |
| GTK journey | FFmpeg PQ input → exposure/curves → HDR numeric paint → undo/redo → saved SDR rendition → actual eyedropper/histogram → native save/reopen → strict PQ and explicit SDR8 exports passes. `native-validation-results.json`, `journey-final/`. |
| Independent delivery | FFmpeg decodes GTK output as full-range RGB BT.2020/smpte2084. Independent Python double math checks all 196,608 exported pixels: **0 PQ16 code error and 0 SDR8 code error**. `interop-validation.json`. |
| Failure/cancel | Strict out-of-range HDR export and accepted cancellation retain an existing destination; deliberate mapped export succeeds. Repeated PQ Open cancellation retains the previous document and permits another Open. |
| Recovery | Actual GTK GPU validation failure/restart with above-white/negative paint restores exact surviving pixels/history. Mapped CMYK proof recovery also passes with `/usr/share/color/icc/krita/cmyk.icm`. |
| SDR/proof regression | GTK document modes, document profile/depth changes, proof setup/compare/history/save/reopen/RGB export, and application file launch pass in separate native processes. |
| Browser regression | Chrome 152.0.7977.64, real WebGPU on isolated Wayland: the complete existing SDR color/photo/import/edit/history/profile/export suite passes. HDR PQ/master rejection preserves the SDR drawing. `browser-sdr.log`, `browser-hdr-limits.log`. |
| UI derivatives | Changing the rendition repaints filter previews without redecoding/reprobing the source; restoring it restores preview bytes. Floating flattened output ignores the SDR recipe. |

Visual review found that the final effect pass could overwrite neighboring
partial tiles. The scissor now uses the effect's actual document rectangle;
512×384, 513×385 and 385×513 regressions pass for U8/U16/F16. Updated GTK captures
show the complete canvas. Actual-browser testing caught ambiguous WGSL bit-shift
parentheses accepted by the native compiler; fixing them restored browser startup.
The first headless Chrome runs also emitted a Dawn instance error; only the
subsequent strict, headed Wayland runs establish browser workflow success.

An initial desktop review launch with GTK's automatic renderer crashed in
`gdk_surface_handle_event`. The Vulkan desktop launch and native file-launch
regression pass; `review/launch.sh` selects Vulkan. The automatic-renderer crash
is retained in `review/startup-coredump.txt` and is not claimed resolved upstream.

## Performance and memory

Fedora 44, kernel 7.1.10-200.fc44.x86_64, GTK 4.22.4/libadwaita 1.9.3,
NVIDIA RTX PRO 6000 Blackwell Max-Q (97,887 MiB, driver 610.57.04, PCI f1:00.0),
250 W power limit. Actual Vulkan rendering, isolated 3840×2160@120 Hz output,
scale 2. Native navigation uses 32 paint layers, five full-resolution adjustments
and 960 software pose requests over eight seconds. No compilation/second local
GPU test overlaps the serial timing sweep. This is a shared workstation, not an
exclusive machine; unrelated GPU residency is present and excluded from the
per-process memory accounting. Software input→compositor feedback is measured,
not physical input-to-photon or physical pen latency.

The fresh parent test executable is based on `173f760f` plus `1c0f531f`, SHA-256
`83c97f3fcc8853311162059c99538d4c2879a1e3b809edda440bb0c80e1b415f`.
The fixed M2 program executable and accepted limits remain in
[the M2 performance record](color-management-gtk-m2-performance.md#final-gtk-navigation-qualification--2026-09-15),
SHA-256 `76393f9a4de7065ad0c0868c505238a1d01fd555e62a86df6756d5acbde1dd64`.
This phase's preserved timing executable is `performance/gtk-tests`, SHA-256
`885874b7735aad1e1a6efd37799f01525b3d5be27e04c44118b827267cd16900`.
Later qualification changes affect WGSL parentheses, HDR display-only signed
mapping, preferences and tests, not the measured SDR-surface hot paths.

Times are milliseconds. Three serial 60 MP SDR repetitions retain noise/outliers:

| Arm | Request→present p99, repeats 1/2/3 | Missed slots 1/2/3 | Whole-process peak RSS MiB |
| --- | --- | --- | ---: |
| Fresh parent | 7.633 / 7.202 / 8.207 | 1 / 0 / 0 | 1153–1159 |
| Fixed M2 program | 8.559 / 8.104 / 7.864 | 0 / 0 / 0 | 1444–1451 |
| Current SDR | 7.458 / 6.997 / 7.565 | 0 / 0 / 0 | 1163–1204 |

Unchanged SDR worker p95/p99 deltas remain under max(5%, 0.2 ms); the fresh
parent and fixed program comparisons both remain in `performance/*.summary.json`.
No favorable single run replaces either baseline.

| HDR workload | First response | Request→present p95 / p99 | Missed slots | Worker CPU / GPU p99 | Peak RSS MiB | Accounted renderer steady MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 24 MP | 32.785 | 7.505 / 7.640 | 0 | 0.423 / 0.361 | 716 | 621 |
| 45 MP | 25.871 | 8.745 / 9.030 | 0 | 0.417 / 0.289 | 886 | 1050 |
| 60 MP | 25.953 | 9.191 / 9.273 | 0 | 0.419 / 0.341 | 1059 | 1353 |
| 60 MP mapped CMYK proof | 11.318 | 10.965 / 11.223 | 0 | 0.471 / 0.400 | 1066 | 1358 |
| 60 MP, 30 effects + concurrent save/export | 26.247 | 9.442 / 9.599 | 7 | 2.883 / 1.118 | 1545 | 1352 |

The stress case adds 24 exposure adjustments and a physical Gaussian blur to
the ordinary five. Atomic save finishes in 136 ms; save, reopen and 480 MB SDR
TIFF delivery finish in 6.84 s, while navigation remains live. The 202 MB half-float
master reopens successfully. Ordinary HDR cold canvas readiness is 1.39/1.97/2.40 s
for 24/45/60 MP; stress readiness is 3.36 s. Timed navigation then uses warm
composition. Each run asserts unchanged artwork and zero camera-only source
decoding/recomposition. This does not establish a dirty 60 MP rebuild at 120 Hz.

Twelve 800 ms, size-720 palette-knife contacts on 4096² ProPhoto artwork with 32
paint layers retain history while overlapping exact backing. HDR brush RGB is
[8,-0.125,2], with alpha 1.

| Drawing | CPU p95 / p99 | GPU p95 / p99 | Max pen-up→commit / observed backing | Peak RSS MiB |
| --- | ---: | ---: | ---: | ---: |
| Parent SDR | 0.888 / 1.021 | 0.951 / 1.132 | 0.169 / 16.461 | 432 |
| Current SDR | 0.911 / 1.043 | 0.956 / 1.140 | 0.179 / 16.459 | 437 |
| HDR | 0.878 / 1.167 | 0.938 / 1.286 | 0.146 / 26.704 | 434 |
| HDR mapped proof | 0.909 / 1.144 | 0.977 / 1.258 | 0.150 / 26.775 | 426 |

All measured workloads meet the predeclared workstation budgets. Backing is
observed on 2 ms event-loop samples, not measured as disk latency. A separate
60 MP memory run observes 1,786 MiB maximum per-process NVML graphics residency;
concurrent long-chain save/export reaches 2,042 MiB, including the capture worker
and driver allocation. 0.5-second sampling can miss shorter peaks. Retain
this separately from Rust-owned steady allocations and `/usr/bin/time -v` RSS.
The serial sweep records 63–69°C on the used GPU, at most 125.26 W, with no clock
event/throttling flags. This short workstation sweep does not qualify long
thermal equilibrium, battery operation or unified-memory/mobile pressure.

## Reproduction and review

Build with `CARGO_TARGET_DIR=… cargo test --offline --release -p layer-linux
-p layer-render-wgpu --no-run`, and `cargo build --offline --release -p layer-linux`.
Use one native test filter per `tools/performance/gtk-raster.sh` process: Rust's
test runner creates a different thread for each test, even with one test thread,
and GTK may only initialize on one thread. A combined-filter attempt is retained
as a harness failure, not an application regression.

- `tools/validation/hdr_reference.py generate DIR` creates independent PQ input.
  Set `LAYER_HDR_INPUT` and `LAYER_HDR_OUTPUT`, run
  `native_hdr_open_edit_rendition_save_and_deliver`, then run the script's `verify`
  operation on that output directory.
- `tools/performance/hdr-gtk.py CANDIDATE PARENT FIXED OUTPUT` records the serial
  sweep, exact environments, binary hashes, raw feedback and summaries.
  `hdr-memory.py BINARY FILTER PREFIX` samples per-process GPU/RSS separately.
- `tools/performance/workspace-motion.sh web --hdr-limits` and `--shared-workflows`
  run real browser tests. Stage the PQ/native HDR fixtures as documented in
  `apps/layer-web/hdr-limits.test.mjs`; the SDR suite uses its existing ProPhoto16
  fixture. Browser output is software-driven, not touch/pen-device qualification.
- `artifacts/color-m4/review/launch.sh` opens the runnable review master with
  isolated app settings/workspaces/recovery. Its README gives feature steps.
  The source now lives in `~/code/capycanvas3` on branch `capycanvas3`, after a
  user-requested relocation on 2026-09-17. That checkout was fast-forwarded to
  `d6b295ae`, preserving its existing local documentation edits. Further work
  uses that checkout; `/tmp/capycanvas-hdr-m4` was only the original build path.

User feature feedback and physical/cross-platform qualification remain explicit
review items. Missing measurements are not passes, and no push to origin/master
has occurred as part of this qualification.
