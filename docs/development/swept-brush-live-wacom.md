# Live swept-brush experiment on Wacom — 2026-09-20

The first integrated experiment improves whole-app drawing throughput by
**2.12× for G-Pen** and **10.27× for the experimental pencil** on the 61 MP
photograph with a 2048 px brush. These are primary-app measurements, not the
earlier standalone kernel benchmark. Pencil intentionally changes deposition
semantics and rendering route; it is not an equivalent faster legacy pencil.

## Implementation

The debug instrumentation switch `wideBrushAlgorithm=swept` enables
`layer-render-wgpu/src/swept_experiment.rs` and `swept_experiment.wgsl`.
Normal launches default to the existing renderer. No brush preset or project
format changes. The switch is process-wide research state and must be set
before opening the test document, never during an active stroke. The Android
hook rejects non-profiling sessions and the test resets it in `finally`.

The experiment:

- Simplifies round-contact centerlines inside each committed or predicted
  batch with a 0.5 px center-plus-radius error bound, retaining the incoming
  segment from the previous frame. It does not merge across batch boundaries.
  Varying color, flow, pressure, tilt or non-round geometry disables merging.
- Uses a simpler round-segment distance evaluator, selects the nearest segment,
  and samples pencil paper only after selecting geometry. G-Pen has a hard
  antialiased edge; pencil has an inward 25%-radius feather and paper coverage.
- Uses the existing GPU stroke-coverage pages to apply only newly covered ink
  across frames and to fork/retire prediction. Separate pencil strokes can
  layer, but repeated passes **within one stroke do not accumulate** like the
  old Flow pencil. A future deposition model must decide that behavior.
- Retains real source tiles, source-over compositing, native storage,
  composition, display mip generation, OS input and presentation. Pencil moves
  from its existing direct/instanced route to the dry-material compute route.

This still consumes resolved contacts from `DabGenerator`; it does **not** yet
fit the original raw pen poses or introduce a persisted new brush type. It is
an integrated test of the simpler continuous evaluator and per-batch merging,
not the completed continuous-brush architecture. The qualified path is the
native Float32 normal-blend photo path, not every fallback/attachment path.

## Protocol

- Wacom `5ll21u1002931`, DTHA140, Adreno 735, Vulkan.
- Primary app `art.capycanvas`, release Rust, debug Android host, same APK for
  both algorithms. The isolated `penverify` app was stopped.
- Photo: 9504 × 6336, 60,217,344 pixels (marketed as 61 MP), original
  `sony_a7r_v_29 (1).jpg`, SHA-256
  `3aac9c9b8b34c38a5e0121f16ad1ec806e92a19e15ee5e1128f36d987e888054`.
- G-Pen preset 1 / Pencil preset 2, diameter 2048 document pixels, full pressure,
  zero injected tilt, opaque magenta color, fit view approximately 18%.
  Preset flow is retained (G-Pen 100%, Pencil 80%).
- OS-injected stylus ellipse, one revolution per second, 15 seconds per stroke,
  feedback enabled, 16 ms fallback prediction. This is path motion frequency,
  not a 1 Hz input delivery rate; injected events arrive around 200 Hz.
- Two strokes per fresh test process/document, twice per algorithm/brush:
  16 timed strokes in total. Order was G-Pen existing A, swept A; Pencil swept A,
  existing A; G-Pen swept B, existing B; Pencil existing B, swept B.
- No frames were removed as warmup. Stroke 1 includes cold page/preview costs;
  stroke 2 exposes warmer behavior. Thermal status was 0 in all recorded
  checks. GPU frequencies were not locked. Existing pencil timings varied
  significantly between runs despite the unchanged workload.

Throughput below is total host drawing callbacks divided by their observed
wall-time span across four strokes. It is not display refresh rate or measured
nib-to-photon latency. GPU figures are the app's rolling last-120 drawing
timestamps, including GPU scheduling gaps between submissions, not isolated
brush-pass times. Slow pencil runs have fewer samples, and the second stroke's
window includes the first; do not treat those windows as independent samples.
Quantiles use nearest rank, consistently in `analyze-live.mjs`.

## Results

| Brush | Existing callbacks/s | Swept callbacks/s | Throughput gain | Existing GPU median range | Swept GPU median range |
| --- | ---: | ---: | ---: | ---: | ---: |
| G-Pen | 18.28 | 38.67 | **2.12×** | 48.78–48.98 ms | 16.29–17.89 ms |
| Pencil | 2.94 | 30.21 | **10.27×** | 185.82–265.57 ms | 23.40–24.82 ms |

Every individual stroke, including cold stalls:

| Brush / algorithm / run | Stroke | Callbacks | Callbacks/s | GPU median | Callback median / p95 / p99 | Callback maximum |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| G-Pen existing A | 1 | 259 | 17.17 | 48.78 | 46.36 / 60.30 / 401.12 | 588.67 |
| G-Pen existing A | 2 | 286 | 18.95 | 48.91 | 46.65 / 59.23 / 79.87 | 117.43 |
| G-Pen swept A | 1 | 597 | 39.64 | 17.63 | 17.76 / 28.14 / 99.98 | 289.08 |
| G-Pen swept A | 2 | 583 | 38.77 | 17.89 | 19.07 / 30.31 / 50.76 | 92.18 |
| G-Pen swept B | 1 | 572 | 38.00 | 16.29 | 18.05 / 28.69 / 61.23 | 394.18 |
| G-Pen swept B | 2 | 575 | 38.28 | 17.41 | 19.37 / 33.71 / 55.94 | 94.24 |
| G-Pen existing B | 1 | 269 | 17.85 | 48.98 | 46.95 / 61.00 / 239.85 | 266.89 |
| G-Pen existing B | 2 | 289 | 19.16 | 48.80 | 45.84 / 60.87 / 89.66 | 95.88 |
| Pencil swept A | 1 | 483 | 32.13 | 23.40 | 23.88 / 31.48 / 128.67 | 274.67 |
| Pencil swept A | 2 | 444 | 29.54 | 24.35 | 24.79 / 33.61 / 125.60 | 625.77 |
| Pencil existing A | 1 | 55 | 3.63 | 185.82 | 228.76 / 470.78 / 728.64 | 728.64 |
| Pencil existing A | 2 | 49 | 3.24 | 205.61 | 288.55 / 488.10 / 570.85 | 570.85 |
| Pencil existing B | 1 | 36 | 2.38 | 265.57 | 361.94 / 729.65 / 994.91 | 994.91 |
| Pencil existing B | 2 | 39 | 2.53 | 255.21 | 330.85 / 770.72 / 785.53 | 785.53 |
| Pencil swept B | 1 | 460 | 30.58 | 24.40 | 24.41 / 32.47 / 144.09 | 544.93 |
| Pencil swept B | 2 | 431 | 28.58 | 24.82 | 24.83 / 33.76 / 130.39 | 724.36 |

All times are milliseconds. The old pencil has only 36–55 callbacks per stroke;
its p99 is effectively the maximum, not a well-qualified tail estimate. Even
the faster runs are too short to establish long-session p99 stability.

### What caused the improvement?

The instrumentation counted input contacts and output segments. Across both
G-Pen runs, **6084 contacts remained 6084 segments**. Across both pencil runs,
**6722 contacts became 6719 segments**. This live ultra-wide workload presents
too few mergeable contacts per batch for consolidation to matter. The measured
gain comes from cheaper evaluation and, for pencil, different deposition and
compute routing—not from collapsing hundreds of overlapping dabs into one.

G-Pen's approximately 2.7–3.0× lower median GPU frame interval translates into
2.12× throughput, consistent with the earlier conditional app-level estimate.
Pencil exceeds the earlier compute-kernel estimate because that benchmark did
not exercise its actual instanced fragment route or the live batching/backlog
feedback. These measurements do not isolate how much of pencil's gain comes
from each of shading, route, deposition semantics and changed live batches.

### Remaining costs and tradeoffs

- **Tail stalls remain.** Experimental pencil still had 626 and 724 ms maximum
  callbacks. In the latter, preparation consumed 436 ms, prediction 237 ms,
  and actual owner-thread CPU totaled 390 ms. In the 626 ms callback,
  preparation was 336 ms and composition 272 ms. These phase timings identify
  remaining host/resource-management or wait costs; a new profile is needed
  before attributing them to a particular allocator/driver function.
- Typical experimental owner-thread CPU medians are 16–19 ms for G-Pen and
  20–23 ms for pencil. The submission phase alone is typically 7–10 ms. A cheaper
  brush shader by itself will not turn this implementation into 120 Hz drawing.
- Pencil's retained coverage/destination state costs memory: tracked canvas
  storage after stroke 2 is **2037 MiB existing versus 2402–2413 MiB swept**,
  roughly 365–375 MiB extra. These counters exclude imported assets and driver
  overhead. G-Pen already owns coverage and shows no similar structural increase.
- The pencil looks different: softer continuous coverage with paper grain, not
  repeated Flow buildup. This is allowed for the new brush class but should not
  silently replace existing saved pencil presets.
- Millions of affected pixels still need shading and composition. This does
  not bound candidate lists for arbitrary long/self-intersecting strokes, nor
  implement raw-pose fitting, tilt-aware nibs or a final graphite model.

## Verification and reproduction

### Faster-motion stress case

At **two revolutions/s and 8 ms prediction**, the existing G-Pen failed during
the 10-second test with `StagingBelt staging buffer ... has been destroyed`.
The process log does not independently establish a GPU device-loss reason;
this is a renderer failure, not proof of a particular watchdog or driver fault.
The host kept producing callbacks after failure, so its callback count is
**not** usable as throughput. The analyzer returns null throughput for failed runs.

The swept G-Pen completed the matching 10-second test and then two 15-second
strokes without a reported failure. The longer strokes delivered **31.55 and
32.31 callbacks/s**, GPU medians **21.97 and 21.68 ms**, and callback p99
**73.85 and 85.49 ms**. Maximum callbacks were still **789 and 403 ms**.
This is encouraging stress evidence, not long-duration device-loss qualification
or a determination/fix of the original crash's root cause. Raw stress reports
are committed alongside the 1 Hz results.

### Regression checks

The GPU test `swept_gpu_batching_and_prediction` passed on both the Linux host
and the physical Wacom. It checks actual paint,
finite pixel values, straight-stroke batch partition equivalence, visible
prediction, unchanged persistent pixels during prediction, and prediction
retirement for both ink and pencil. Run it separately because the switch is
process-wide. The two CPU merge tests also passed on Wacom. The existing
material-specialization regression passed on
the Linux Vulkan host: 48 cases and 240 full-image comparisons with zero
channel error.

**The legacy material-specialization test still fails on Wacom with the
experiment disabled:** operation 0, variant 0, preview phase 1, maximum channel
error 93. This matches the documented pre-existing non-native Multiply failure
and the older `artifacts/wacom-allocation-fix/device-baseline-material.log`.
Re-running the preserved September 19 test binary on this Wacom reproduced
the identical failure; both logs are committed with the measurement data.
The earlier compute-routing guard has not resolved that failing case. These
results must not be described as a clean Adreno regression suite or as fixing
legacy Multiply. These are focused new-path tests, not full pen-behavior
qualification.

```sh
ANDROID_HOME=/path/to/Android/Sdk apps/layer-android/gradlew -p apps/layer-android \
  :app:assembleDebug :app:assembleDebugAndroidTest -PcapyAbi=arm64-v8a
adb -s 5ll21u1002931 install -r apps/layer-android/app/build/outputs/apk/debug/app-debug.apk
adb -s 5ll21u1002931 install -r apps/layer-android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
# The source JPEG must already exist in art.capycanvas/files/photo-benchmark.jpg.
bash crates/layer-render-wgpu/examples/swept_brush/run-live.sh existing-gpen-a existing 1
bash crates/layer-render-wgpu/examples/swept_brush/run-live.sh swept-gpen-a swept 1
# Preset 2 selects pencil; repeat in reverse order for the B pair.
# Optional stress case: 2 revolutions/s, 8 ms prediction, two 15-second strokes.
bash crates/layer-render-wgpu/examples/swept_brush/run-live.sh stress-swept-gpen-long swept 1 2 8 15000 2
node crates/layer-render-wgpu/examples/swept_brush/analyze-live.mjs \
  docs/development/measurements/swept-brush/live-app/existing-gpen-a.json.gz \
  docs/development/measurements/swept-brush/live-app/swept-gpen-a.json.gz
cargo test -p layer-render-wgpu --lib swept_gpu_batching_and_prediction -- --ignored
```

Committed compressed JSON retains every frame phase, input timestamp and
rolling GPU sample; unrelated app state and process dumps were removed by
`export-live.mjs`. Original reports, screenshots, thermal reports and logs are
under local `artifacts/swept-brush/live-app/`. Both algorithms' photo strokes
were visually checked. Physical Apple, Web and GTK performance is untested.

Tested primary APK SHA-256:
`23cdfb49712d12fe78c5f0ac482dd903867596b9ace7b8a9b68c64f7a4d8f7ca`.
Test APK:
`98560b8420118a1a592fe0b36fc1d71ce47868cd670137312df81e1d4654e307`.
Wacom native test executable:
`c8c7c24799f14b53ef00d9a015ea1cac42cb28905465f41f50af46179aafcea6`.
Preserved pre-experiment executable:
`b284ed1b6cf70f94d2a48a4606ed3a9c25e709567acd9fb8bf1d256a1301e7bf`.

## Installed research build

The subsequent Wacom install uses `-PcapySweptBrush=true`, forwards the
`swept-brush-default` Cargo feature, and reports version `0.1.0-swept` under
the primary `art.capycanvas` package. This makes the optimized path active at
process startup without instrumentation or a session toggle. Ordinary builds
without this property still default to the existing renderer. Only the tested
G-Pen/Pencil contact models opt in; other contact media retain their path.
The experimental pencil appearance and limitations above still apply.

```sh
ANDROID_HOME=/path/to/Android/Sdk apps/layer-android/gradlew -p apps/layer-android \
  :app:assembleDebug :app:assembleDebugAndroidTest -PcapyAbi=arm64-v8a -PcapySweptBrush=true
```

For an installation smoke test, `wideBrushAlgorithm=installed` asserts that
the runtime already has swept rendering enabled and does not set a test
override. The test restores the build's default afterward. The instrumentation
APK is removed after verification so only the primary app remains installed.

Installed optimized-default APK SHA-256:
`c7233a16a1326b013a777b2bcbd73e020d801ad6f517febb94554a591eb520fd`.
The APK and installation smoke-test reports are retained locally under
`artifacts/swept-brush/live-app/`.
