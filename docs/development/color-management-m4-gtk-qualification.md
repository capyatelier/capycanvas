# GTK phase-4 completion audit and measurement protocol

Base: fetched `origin/main`, `59aff3df1aa7a75931a47aef6be2bed65accefe6`.
Feature branch: `gtk-phase4-qualification`. Runnable package review precedes merge; the user subsequently authorized merging stabilized work to `origin/main`.
The old `artifacts/color-m4/review/build-manifest.json` is historical evidence;
new evidence is under `artifacts/color-m4/qualification/`.

## Audit and scope

- The reviewed Proof dial, defaults, gestures, cancellation, undo transaction,
  preview/export navigation and HDR picker intensity semantics are retained.
  Picker hue textures are display derivatives, separate from document storage.
- Master storage remains binary16, shared processing and guide arithmetic
  Float32. The 203 cd/m² reference white, extended/negative-value edit contract,
  exact native save/undo and explicit delivery range clipping are unchanged.
  Float32 document storage is outside this work.
- The GTK null-event-surface guard was present only in a special review runtime.
  Normal `package.mjs` now builds/stages it and makes every packaged launch use
  it, including file launches. GTK sources, patch, LGPL text, recipe and hashes
  travel with the replaceable library. System libraries are not overwritten.
- The hue regression reproduces at the base with the unchanged `2/255` maximum
  error assertion. The P3 guide's eight-bit quantization precedes GTK's sRGB
  conversion and amplifies dark-channel error. A retained half-precision texture
  fixes that intermediate boundary. A gradient-interpolation-only change did
  not fix it and was discarded. The reference, sampling area and tolerance stay
  intact; failure diagnostics now identify the worst pixel.
- `LocalToneView` owns one superseding capture per window. Its key includes
  document epoch, GPU owner, color, dimensions, background and composite layer
  state. Recipe-only edits reuse the immutable guide; artwork changes invalidate
  it and debounce for 180 ms. Destroy, replacement and cancellation retire work.
  The renderer acknowledges guide publication before it is reported ready.
- Full-resolution export captures an immutable project, analyzes its complete
  rows, and applies the same saved local recipe. JPEG and AVIF use a separate
  cancellable codec process. Publication remains atomic. The qualification
  worker exercises the same `files::export::write_snapshot` implementation.
- Concurrent qualification exposed two production bottlenecks. Shared-device
  snapshots now submit at most 512 columns before readback yields the queue;
  assembled CPU bands remain included in the capture budget. Exact masked/effect
  pixels across partial column boundaries agree within the existing 2e-6 gate.
  Large AVIFs now use zero-copy grid views of the same 12-bit 4:4:4 base, alpha
  and gain planes, bounding libaom workspace per cell. No image resizing, chroma
  reduction or gain precision reduction is involved. Odd partial cells, alpha,
  metadata and gain samples have a dedicated codec regression check.
- Wayland preferred-description feedback resets headroom to 1 on change, admits
  one request, rejects obsolete generation replies and derives headroom from
  compositor peak/reference white. Neither presentation nor monitor changes
  rewrite artwork. Synthetic capability transitions are distinct from an actual
  window move between physical outputs.

## Protocol and gates

Use the predeclared workstation budgets in
[the phase-4 design](color-management-m4-design.md). Ordinary 60 MP work:
request-to-present p99 ≤20 ms, worker CPU/GPU p99 ≤2 ms, ≤1% missed 120 Hz slots,
first response ≤50 ms, cold guide readiness ≤15 s, process peak ≤2 GiB,
accounted renderer ≤2 GiB. Concurrent stress: p99 ≤33.4 ms,
≤2% missed slots, cold readiness ≤30 s and process peak ≤3 GiB. Record driver GPU allocation and codec
processes/staging separately; sampled RSS can miss short peaks. These budgets
were not expanded for local tone or codecs.

Compile first, then run serially on a private 3840×2160@120 Hz Mutter output,
scale 2, Vulkan. Compare current SDR navigation with the fresh base executable
and the fixed M2 program (`76393f9a…`), three repetitions each. Investigate
unchanged worker p95/p99 increases above `max(5%, 0.2 ms)`; presentation phase
and shared-workstation scheduling noise must remain visible in the raw results.

Run three 120-second 60 MP local-tone drags, 14,400 software updates each, then
repeat with a second live HDR document plus native save/reopen and full 60 MP
JPEG/AVIF exports. Samples cross a quantized value each tick. The initial pilot
used smaller increments, resulting in legitimate unchanged-value suppression;
its raw missed-slot result is retained and is not a failed renderer deadline.
No latency result is inferred from a heartbeat or event pumping duration.

Test-only instrumentation records the exact recipe used by each canvas frame.
Match it with the latest identical requested recipe preceding frame enqueue and
the first successful compositor feedback for that request. Report unmatched
requests, schedule lateness, p95/p99, ten-second distributions, raw cadence gaps,
worker CPU/GPU, source decoding/recomposition and memory. Assert reuse of the
guide, exact unchanged layers, one-step undo, complete saved-document equality,
full decoded export extent, finite half samples and retained above-white range.
Codec fidelity tolerances remain the existing numerical tests; a finite large
decode alone is not a worst-case codec fidelity proof.

Reproduction (set `LD_LIBRARY_PATH` to the built package's GTK library directory
and `CAPY_PHOTO_CODEC_DIR` to its photo directory for test executables):

```sh
cargo test --locked --offline --release -p layer-linux --no-run
python3 tools/performance/gtk-phase4.py CANDIDATE PARENT FIXED CURRENT_60MP_CAPY OUTPUT
cargo run --locked --offline --release -p layer-color --example local_tone_limits
python3 tools/validation/gtk_package_photo.py --binary RELOCATED/bin/capycanvas \
  --photo HDR_MASTER --photo HDR_AVIF --output EMPTY_EVIDENCE_DIRECTORY
```

## Guide and animation limits

The guide is fixed in document space, at most 768 pixels on its longest edge.
For 8192×7324 it is 768×687 (8,441,856 sample bytes); the maximum square guide
is 9,437,184 bytes. This is the final guide, not total analysis-worker memory.
Source traversal, source bands, pyramid scratch, GPU upload and snapshot
residency must also be counted. Maximum accepted dimensions are 32768 per axis.
Analysis uses coverage-weighted log luminance, a −24-stop floor and intensity
anchors at most half a stop apart. Fine detail remains in full-resolution pixel
residuals, but its illumination estimate cannot resolve every spatial feature.
Swatches and per-layer thumbnails retain their documented point-color mapping.

Animated views replace the last complete guide at most twice per second;
worker time can make updates slower. Export analyzes the captured frame, so a
moving view can differ from that frame's final export. There is no temporal
interpolation or proven flicker bound. The reproducible `local_tone_limits`
assessment moves a six-stop light over textured background at 30 fps and
compares the same current-frame pixel through old/new guides. Even ideal 2 Hz
refresh produced a maximum 0.17837 linear or 102.63 encoded-sRGB-code change at
publication; maximum stale-guide discrepancy was 0.22393 linear. These are
adversarial synthetic mapping differences, not a perceptual or optical test.
Animated local-tone preview is **not qualified as flicker-free**. Static artwork
interaction and captured-frame exports have a separate qualification scope.

## Hardware evidence boundary

The workstation exposes four NVIDIA RTX PRO 6000 Blackwell Max-Q GPUs (97,887
MiB each, driver 610.57.04, 250 W cap); GTK selects PCI `f1:00.0`. Physical
outputs include a Wacom Cintiq Pro 27 and LG TV, both configured at scale 2.
The Cintiq's pen, finger/touch and pad are enumerated by the kernel. This is
available hardware, not evidence that physical contacts were exercised.

No robot or human physical pen/touch run, optical latency measurement, emitted
luminance/calibration measurement, or observed physical mixed-monitor move is
claimed. Isolated Mutter input and injected capability changes cannot substitute
for those checks. Other native hosts, Web HDR, constrained/unified-memory devices,
battery operation and long thermal equilibrium remain outside this workstation
qualification. Whole phase 4 remains open where these gates lack evidence.

## Numerical checks that prevent blanket signoff

All 18 snapshot tests pass, including masked/effect capture across partial column
boundaries; the final 512-column boundary test also passes. Existing edited HDR
JPEG/transparent AVIF reconstruction, metadata/rendition, and canonical-gain
checks pass, as does the new AVIF grid boundary/alpha test. The wider six-test
gain-map run nevertheless has **two failures**, identical with the unchanged
original worker and the new grid worker:

- `local_gainmaps_reconstruct_the_master_from_spatially_different_bases`:
  JPEG fallback red 1.0193013 versus reference 1.0, exceeding the unchanged
  0.018 absolute linear bound (0.0193013 observed).
- `unified_white_fallback_reconstructs_saturated_hdr_in_both_formats`:
  JPEG green whitening difference 0.3625228, below the unchanged >0.4 assertion.
  HDR reconstruction itself remains red and within its existing tolerance.

Both fail on the JPEG iteration before AVIF, and both small images bypass the
new AVIF grid path. The shared mapping/defaults and JPEG encoder are unchanged
by this branch. Earlier contrast-dial evidence passed before the reviewed Proof
baseline changed in `49fcae90`; historical codec passes cannot be reused as a
current complete numerical signoff. Neither assertions nor reviewed mapping
have been adjusted to hide this mismatch. Full-sized export completion and
finite decoding are qualified separately from these outstanding fidelity gates.

## Native workflow checks

The unchanged hue reference/sampling/tolerance passes at scale 1 and scale 2.
Mouse and Mutter virtual-touch HDR picker and Proof dial routes pass. Native
HDR open/edit/save/delivery, gain-map preview/flatten/reopen, explicit clipping,
failed/cancelled publication preserving the destination, display negotiation,
first-frame display hints, and GPU failure recovery pass (one test per process).
These are the eleven entries in `workflow-results.json`, plus the scale-1 hue
run. Shared non-GPU color/core/UI suites passed 655 tests.

On the connected desktop, `native_hdr_export_preview_preserves_master_and_tracks_display`
passes with actual negotiated HDR enabled. Downloaded master linear RGB is
[4.470145, 1.470159, 0.720345]; GSK compositing retains above-white values.
Injected capability loss/recovery switches presentation and preserves the master.
The compositor advertises a 10000-nit peak and 142-nit reference; these are
capabilities received in software, **not measured panel luminance**. Physical
mixed-output movement and real contact input still require review hardware use.

## Sustained interaction and full export results

Each row below is 120 seconds, 14,400 requested changes, 8192×7324 Float16
artwork with twenty live effects. Presentation uses the isolated 4K/120 Hz/2×
Wayland setup described above. RSS includes the application and codec children;
these are sampled maxima, not a proof that no shorter allocation peak occurred.

| Workload | Request→present p95 / p99 (ms) | Missed 120 Hz slots | Peak tree RSS (GiB) |
| --- | ---: | ---: | ---: |
| Local 1 | 11.084 / 14.117 | 0.639% | 1.351 |
| Local 2 | 11.293 / 14.118 | 0.389% | 1.198 |
| Local 3 | 11.732 / 14.517 | 0.632% | 1.253 |
| Original concurrent | 13.912 / 14.538 | 2.750% | 4.558 |
| 1024-column intermediate | 12.462 / 12.816 | 2.104% | 2.729 |
| Final concurrent 1 | 12.177 / 13.687 | 0.097% | 2.735 |
| Final concurrent 2 | 11.461 / 13.535 | 0.979% | 2.674 |
| Final ordinary local | 13.694 / 14.066 | 0.125% | 1.221 |

The original concurrent run failed the 3 GiB and 2% gates. AVIF cells brought
memory under budget; the intermediate 1024-column GPU capture still missed
2.104% of slots. The final 512-column captures pass in both repetitions without
changing those budgets. First concurrent response was 105.642 ms / 89.594 ms;
this is reported separately from matched-request percentiles and unmatched
requests remain in the raw report. It is not an input-to-photon measurement.
All local-tone runs retain the same guide, unchanged artwork layers and one-step
undo, with zero source-cache misses or extra artwork recomposition during recipe
interaction. The renderer owns 1,426,526,132 bytes (1.329 GiB); actual driver GPU
residency is higher and reported separately.

Both final concurrent runs save/reopen the exact native document, keep a second
HDR window live, export and fully decode JPEG then AVIF while the first window
continues receiving control changes. Each decode visits all 59,998,208 finite
half-float pixels and retains above-white samples. JPEG publication takes
21.12–21.28 s and AVIF 34.19–34.75 s; complete save/reopen/export/decode work takes
73.81–74.59 s. JPEG is 9,045,503 bytes, AVIF 25,653,895 bytes. The JPEG SHA-256 is
`44c0a625d7e6fd13ef71e5c464fb68f9cee7028e2cfe91c0e5eaae15e1489f81`, identical
for the original, intermediate and both final capture implementations. AVIF grid
boundaries change compressed bytes; its small boundary/alpha/reference test
passes. This does not override the two existing numerical failures above.

The stress artwork contains negative/out-of-delivery-gamut samples. These runs
explicitly exercise the allowed clipping route (30,523,194 delivery channels),
never clipping or rewriting the master. Largest decoded RGB is 55 for JPEG and
225.125 for AVIF; these maxima alone are not fidelity measurements. `/tmp`
staging can exceed 2 GiB alongside process RSS; on this host it is tmpfs and
therefore adds physical memory pressure. Driver allocation peaks at 2645 MiB in
concurrent runs, separate from renderer accounting. Do not transfer the discrete
workstation result to a unified-memory device by ignoring staging or driver RAM.
Instrumentation retains every request/frame and grows throughout each run;
report serialization also contributes to process memory. The initial serial
sweep recorded 65–73°C and 70.82–125.95 W on the selected GPU, with no reported
throttling. These minutes of activity do not establish thermal equilibrium.

The final ordinary run also passes all declared gates: first response 2.207 ms,
worker CPU/GPU p99 0.681/0.553 ms, and peak sampled process tree 1.221 GiB.
The three earlier ordinary runs had p99 14.117–14.517 ms and first response
14.357–44.719 ms. All cold-guide readiness observations are retained separately.

## SDR comparison

The nine serial parent/fixed/candidate runs each exercise 960 changes on the same
60 MP ProPhoto U16 navigation fixture, with no concurrent compilation or benchmark.
Request-to-present p99 ranges are parent 9.887–14.060 ms, fixed M2
9.236–14.286 ms, and candidate 9.777–13.072 ms. Worker p95/p99 comparisons do not
exceed the predeclared max(5%, 0.2 ms) investigation trigger. The fixed M2 binary,
fresh `59aff3df` parent, and candidate hashes are recorded in `timing/runs.json`.
First SDR response varies substantially (parent 44.466–134.419 ms; candidate
29.336–79.751 ms), so the warm percentiles do not establish a universal first-input
latency claim. Report these coalesced first updates rather than assigning zero
latency to requests that never received matching presentation feedback.

The final implementation's extra SDR run has p95/p99 9.781/9.939 ms, first
response 40.892 ms, worker CPU/GPU p99 0.684/0.523 ms, and four missed slots
out of 960. This remains inside the observed baseline envelope. Final-run GPU
samples cover 71–75°C and 82.46–128.11 W without reported throttling.

## Review delivery

Implementation commit: `5d564bd2`. The final normal package is built using
`CAPY_GTK_BUILD_DIR=artifacts/color-m4/gtk-runtime CARGO_NET_OFFLINE=true node apps/layer-linux/package.mjs`;
the cache override supplies the already fetched pinned archive and extracted
build dependencies, not a different runtime recipe. The exact package is copied
to `artifacts/color-m4/qualification/review/package`, with an isolated-settings
`review/launch.sh` and the reviewed HDR Proof document. `build-manifest.json`
records source, executable, library, fixture and evidence hashes. Binary artifacts
stay in the local ignored artifact tree; the committed report records results.

The package is relocated to a path containing spaces and tested using normal
automatic GSK renderer selection, both native HDR documents and the actual new
60 MP AVIF. Validation checks mapped library paths, not just shipped file names.
A separate desktop package capture exercises startup with the Cintiq devices
attached. See `package-final-auto/report.json` and `desktop-package/report.json`.

The user authorized merging stabilized work to `origin/main` after requesting
feature-branch delivery. The package and evidence are made available before
that merge. This change closes the startup packaging and hue regression and
adds bounded, measured static local-tone/concurrent export behavior. It does
not turn the unresolved numerical, animation or hardware gates into passes.

## Integration with concurrently updated main

Before the authorized main merge, `origin/main` advanced from `59aff3df` to
`537464b7` (iPad window-control spacing and separate Float32/OpenEXR work).
The first fast-forward push was rejected; no remote history was overwritten.
Integration commit `b8ff85a3` merges that work cleanly in an isolated checkout.
The GTK diff relative to the new main adds no Float32 storage implementation.
References to half masters and the measurements above describe this task's
Float16 workload; the combined application also contains the independently
published Float32 feature, which this report does not qualify.

The integrated source is rebuilt and checked separately under
`artifacts/color-m4/qualification/integration/`. Its 91 non-ignored shared color
tests pass. The gain-map suite reproduces the same four passes and two unchanged
JPEG failures. Original evidence and its runnable package remain preserved;
the integration package and manifest identify the exact combined revision.

All nineteen integrated snapshot checks and all eleven integrated native
workflow checks pass, including the unchanged hue assertion at 2×. The rebuilt
normal package opens the reviewed native document and the full exported 60 MP
AVIF after relocation to a path containing spaces. Actual-desktop HDR texture
preservation and injected capability transitions pass again. The following
additional measurements use the combined binary, with compilation and other GPU
checks completed before timing; they exercise Float16, not Float32 storage.

Integrated concurrent run (120 s, 14,400 requests): p95/p99 12.239/12.535 ms,
0.215% missed slots, sampled tree RSS 2.653 GiB. All declared concurrent gates
pass. Native save/reopen plus both full exports and complete decoding finishes
in 89.933 s during the interaction window. JPEG publication is 26.896 s, AVIF
41.909 s; these combined-revision timings are slower than the original branch's
21.12–21.28/34.19–34.75 s and are not presented as a throughput improvement.
The decoded counts, extents, clipping counts and compressed bytes are unchanged.

Integrated ordinary local interaction also passes all gates: p95/p99
13.742/14.058 ms, 0.146% missed slots, first response
2.392 ms, worker CPU/GPU p99 0.701/0.621 ms, and peak tree
RSS 1.246 GiB. The integrated SDR comparison records p95/p99
12.863/13.157 ms, worker CPU/GPU p99 0.663/0.555 ms, first response
110.482 ms and 31 missed slots. Its worker percentiles remain inside
the original parent/fixed-baseline investigation envelope. These are additional
integration observations; the three-way paired baselines above use the original
base, not the concurrently published Float32 commit.

Latest runnable build: `artifacts/color-m4/qualification/integration/review/launch.sh`.
Its source/build/evidence identities are in `integration/build-manifest.json`.
The original branch's package and manifest remain available separately.

A targeted integrated SDR repeat records p95/p99 13.032/13.164 ms, worker
CPU/GPU p99 0.689/0.538 ms, four missed slots and first response 64.824 ms.
The preceding 31-slot/110.482 ms first-use outlier is retained. Worker percentile
regression gates are satisfied, but these runs do not establish a ≤50 ms SDR
first-response guarantee; that limitation also appears in the original parent
and fixed-baseline observations. No bad run is removed from the evidence.
