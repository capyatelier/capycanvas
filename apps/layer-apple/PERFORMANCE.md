# Apple performance measurement

[Capy Canvas for macOS and iPadOS](README.md)

How to measure the Apple hosts. Targets are in
[Performance targets](../../docs/PERFORMANCE_TARGETS.md); Apple silicon iPads and
Macs exceed the top tier, so its targets apply. Record results only in the
[tier tables](../../docs/performance/top-tier.md).

The iPad and Mac share an optional trace recorder, the serial render owner and
the Rust GPU timer. Ordinary launches record nothing; visible Diagnostics
collects renderer timings independently. The native `CAMetalLayer` subclass
observes the drawables wgpu acquires and records actual display presentation
through Metal's `addPresentedHandler` and `presentedTime`. A display-link tick
or a completed Rust call is not a presentation. The iOS simulator exposes no
presentation callbacks, so simulator runs cannot show presentation timing.

## Recording a trace

Measure Release builds (`CAPY_CONFIGURATION=Release`). Debug traces only check
the instrumentation.

```sh
CAPY_CONFIGURATION=Release bash apps/layer-apple/scripts/build.sh macos
open -n --env CAPY_TRACE_SECONDS=30 --env CAPY_STORAGE_DIR=capy-trace \
  apps/layer-apple/DerivedData/Build/Products/Release/CapyCanvas-Mac.app
mkdir -p artifacts/performance/mac
cp ~/Library/Containers/art.capycanvas.CapyCanvas/Data/Documents/Performance/frames-*.jsonl \
  artifacts/performance/mac/
```

A Release build has the release identity, so `CAPY_STORAGE_DIR` keeps the run
away from the artist's settings and drawings.

On the iPad, install a Release build and launch it with the variables, then copy
the trace out of the app container:

```sh
xcrun devicectl device process launch --device DEVICE_ID --terminate-existing \
  --environment-variables '{"CAPY_TRACE_SECONDS":"30"}' BUNDLE_ID
xcrun devicectl device copy from --device DEVICE_ID \
  --domain-type appDataContainer --domain-identifier BUNDLE_ID \
  --source Documents/Performance --destination artifacts/performance/ipad
python3 tools/performance/apple_trace.py TRACE.jsonl --target-hz 120 \
  --output artifacts/performance/report.json
```

- `CAPY_TRACE_SECONDS` (at most 3600) starts recording when the session owner is
  created, so startup is included. Traces go to `Documents/Performance` in the
  app container unless `CAPY_TRACE_DIRECTORY` names another directory inside
  it; the sandboxed Mac app cannot write outside its container.
  A finished trace is `frames-<uuid>.jsonl`; `.partial` means export did not
  finish.
- `CAPY_TRACE_GPU=0` keeps CPU, input, memory and presentation records but turns
  off GPU timestamp markers and readback. Disabled GPU values stay null, never
  zero. Compare on/off pairs with the same build, workload, duration and display
  state before drawing conclusions about recorder overhead.
- Keep the app foregrounded and visible for the whole interval and the
  two-second callback grace period; an occluded window cannot present. Keep
  builds, profilers and UI automation idle. Match each trace to its launch by
  process and time rather than taking the newest file in the container.
- `--target-hz` sets the evaluated refresh rate (default 120). Use the display's
  actual rate: 120 on the iPad and 90 on the current Mac display.
- Traces contain timings, counts, memory, thermal state and display size; no
  coordinates, artwork, document names or identifiers. Keep them, build output
  and device-tool output in ignored `artifacts/`.

## Drawing workloads

`CAPY_WORKLOAD` runs a synthetic drawing through the same input owner, display
link, renderer, editor panels, history and recovery writer as ordinary drawing.
Each run uses a fresh private persistence root under
`Caches/CapyPerformanceSessions`, so it never touches the artist's settings,
workspaces or recovery copies. Install it under its own bundle identity and
DerivedData directory, and run one workload per device.

| Profile | Document | Paint layers | Brush and diameter | Supplied native prediction |
| --- | --- | --- | --- | --- |
| `ink` | 2048 × 2048 | 1 | G-Pen, 24 px | Off |
| `ink-predicted` | 2048 × 2048 | 1 | G-Pen, 24 px | On |
| `wet-watercolor` | 2048 × 2048 | 1 | Wet Watercolor, 320 px | On |
| `layered-4k` | 4096 × 4096 | 8 | G-Pen, 24 px | On |
| `wet-watercolor-4k` | 4096 × 4096 | 8 | Wet Watercolor, 320 px | On |

- The trajectory in `DrawingWorkloadPlan.swift` draws deterministic curves at
  240 samples per second with pressure from 0.25 to 1, in 1.5 s strokes with
  0.1 s lifts, delivered in batches at 120 callbacks per second. A backlog of one
  second aborts the run rather than dropping samples.
- The 4K profiles add seven translucent full-document underpaint layers through
  ordinary UI actions. Brush quality settings are never reduced.
- Engine prediction follows the default preferences (on, 16 ms). The Mac has no
  native prediction provider, so `ink` still uses engine prediction there.
- `CAPY_WORKLOAD_SECONDS` is the measured time after a ten-second warm-up
  (default 600, range 1–1800); a ten-second postlude follows. Tracing runs for
  the workload unless `CAPY_TRACE_SECONDS` shortens it.
- `CAPY_WORKLOAD_SPACE` and `CAPY_WORKLOAD_DEPTH` choose the document color
  space and depth (default `Srgb` and `U8`, for example `ProPhoto` and `U16`).
  `CAPY_WORKLOAD_TRANSPARENCY` sets panel transparency (`off`, `low`, `medium`,
  `high`).
- `CAPY_WORKLOAD_THEME` applies `light` or `dark` through the shared settings
  action and verifies the resulting appearance before input starts. Workloads
  use fresh preferences, so staging settings under `CAPY_STORAGE_DIR` does not
  select their theme.

The `photo` profile uses an owned current-format package named by the absolute
`CAPY_WORKLOAD_DOCUMENT` path inside the private app container. Prepare the
9504 × 6336 reference photo as the bottom visible layer, one empty active paint
layer above it and hidden Paper. Verify the package's topology and source hashes
before launch. Set `CAPY_WORKLOAD_BRUSH` and `CAPY_WORKLOAD_DIAMETER` explicitly;
retain the preset's default spacing and settings. The package supplies document
color, so `CAPY_WORKLOAD_SPACE` and `CAPY_WORKLOAD_DEPTH` are rejected.

This profile has a fixed twenty-second measurement: three six-second contacts
at pressure 1 separated by one-second lifts. Before measurement, one six-second
contact primes the brush. The ordinary persistence barrier settles that contact;
Undo and a second barrier must restore the original layer revisions. Failed
restoration aborts the run. `CAPY_WORKLOAD_SECONDS` must be absent or `20`.
The trace records the prime, successful restoration and each measured contact's
Down/Up boundaries. These are synthetic native-owner inputs, not physical Pencil
samples. Match the actual package, extent, target, brush, theme and captures to
the trace before using the contact windows for qualification.

```sh
xcodebuild -quiet -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-Mac -configuration Release -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath apps/layer-apple/DerivedData/PerformanceMac \
  CAPY_APPLE_BUNDLE_ID=art.capycanvas.CapyCanvas.performance \
  CODE_SIGN_IDENTITY=- CODE_SIGNING_ALLOWED=YES build
open -n --env CAPY_WORKLOAD=ink --env CAPY_WORKLOAD_SECONDS=600 \
  apps/layer-apple/DerivedData/PerformanceMac/Build/Products/Release/CapyCanvas-Mac.app \
  --args -ApplePersistenceIgnoreState YES
mkdir -p artifacts/performance/mac-ink
cp ~/Library/Containers/art.capycanvas.CapyCanvas.performance/Data/Documents/Performance/frames-*.jsonl \
  artifacts/performance/mac-ink/
```

For the iPad, build the `CapyCanvas-iPad` scheme for `generic/platform=iOS` with a
performance bundle identity and a signing team, install it, and launch it with
`--environment-variables '{"CAPY_WORKLOAD":"ink","CAPY_WORKLOAD_SECONDS":"600"}'`.

`measurement_completed` means a complete, non-aborted interval was recorded, not
that targets passed. Review rejected input, frame errors, missing presentations,
readiness and every warning. A completed input producer does not show continuous
rendering if the window was occluded.
The analyzer requires an error-free state with both canvas and shaders ready
(mask 9); the retired catalog bit is not required. Missing readiness produces
the warning “No error-free state with canvas and shaders ready was observed.”

For an externally observed motion interval, `apple_trace.py TRACE.jsonl
--motion-windows WINDOWS.json --target-hz 90` also reports bounded canvas
presentations. The JSON array contains `begin_ns`, `end_ns` and a retained
`motion_witness` reference for each 5–10 s interval on the same native clock.
A witness label requires human review. Instrumentation eligibility does not
establish movement, filtered pixel freshness or a rate target. Readiness loss,
missing/zero/duplicate presentations and window-edge coverage remain explicit.
Canvas presentation callbacks do not measure cached native chrome.

`tests/tool-bubble-motion.swift` mounts the production Mac editor on the reference
photo, primes and undoes one native drag, then supplies three six-second drags
through four neighbours, including Color and folded Layers. Every owner must move
by more than eight points; each drag is restored with workspace Undo outside timing.
Build before timing, so compilation and the GPU test suites are idle during the run:

```sh
CAPY_CONFIGURATION=Release CAPY_TEST_ASSETS_APP=/path/to/CapyCanvas-Mac.app \
  CAPY_TEST_BUILD_DIRECTORY="$PWD/artifacts/bubble-motion-build" \
  bash apps/layer-apple/scripts/test-project-files.sh \
  apps/layer-apple/tests/tool-bubble-motion.swift
CAPY_BUBBLE_MOTION_PHOTO=/path/to/sony_a7r_v_29.jpg \
  CAPY_BUBBLE_MOTION_THEME=light \
  CAPY_BUBBLE_MOTION_OUTPUT="$PWD/artifacts/bubble-motion" \
  CAPY_TRACE_DIRECTORY="$PWD/artifacts/bubble-motion" CAPY_TRACE_SECONDS=90 \
  artifacts/bubble-motion-build/Check.app/Contents/MacOS/Check
```

The fixture retains native event counts, timestamped owner poses, captures
and clock windows. Observed movement defines each window, excluding pickup
delay and 100 ms at either moving edge. Analyze its trace with `--motion-windows`
and review those witnesses. Canvas callbacks still cannot qualify the whole
native overlay's
presentation rate. Physical iPad input and timing require the device journeys;
the Mac fixture's Apple policy selection does not establish UIKit delivery.

## Reading the report

- **Canvas updates.** `workload.canvas_update_progress` joins the shared
  renderer's submission counter and consumed real-input timestamp to each
  drawable's actual presentation. It reuses the Android nonempty-update
  accounting inside complete contacts, using their source Down/Up timestamps.
  The last frame before the first contact supplies the counter baseline;
  opening the workload document may replace the renderer during setup.
  Presentation bounds queue completion from above, so `completed_per_s` and
  `input_updates_per_s_lower_bound` are conservative rate bounds. Gap statistics
  describe these observed presentation bounds, not exact GPU finish times.
  Viewport-only frames, repeated input, late completions and ambiguous records
  cannot inflate the counts. Counter resets during the measured cohort
  invalidate the measurement; incomplete contact boundaries or absent counter
  metadata are unavailable.
  Deferred canvas work can advance the counter, so this does not prove input
  pixel inclusion or physical Pencil latency. GPU timestamp probes remain off
  when `CAPY_TRACE_GPU=0`.
- **CPU.** Owner queue age runs from display-link admission to owner execution;
  owner service includes the bridge and snapshot work. The five Rust stages are
  CPU preparation, drawable acquisition, viewport submission, present call and
  polling, not GPU durations.
- **GPU.** Timestamps bracket queue work across paint and viewport submissions,
  including CPU submission gaps: a GPU queue span, not isolated GPU busy time.
  Pass-based markers sample both boundaries, so their tiny compute writes are
  included in the span. Resolves include only queries written by the selected
  marker strategy; this avoids an observed legacy Metal driver fault.
  Three readback slots bound the work and full slots skip observations; the owner
  never waits for a readback.
- **Presentation.** Actual `presentedTime` values give intervals and lateness
  against the display link's target. A zero time means skipped, not zero
  latency; missing callbacks are counted separately. Continuous cadence excludes
  intervals across recorded idle pauses. `frame_admission_to_present_ms` is
  software scheduling delay, not input-to-pixel latency.
  When `encoded_camera_revision` is true, `consumedPaint.d` records the camera
  revision immediately after the native frame, before later publications or
  startup actions. `consumedPaint.e` is zero without a valid frame result, one
  for an idle or finished native frame and two when more work remains. Join the
  frame ID to an actual drawable presentation; a valid result alone does not prove
  presentation.
  A revision identifies encoded camera state. Compare its geometry with a
  timestamped camera witness to establish movement, since identity transforms
  can also advance the revision. This does not measure SwiftUI overlay motion.
  `apple_trace.py TRACE.jsonl --camera-witness WITNESS.json` adds full-contact
  canvas counts, boundary gaps and the first proven changed-pose presentation
  upper bound. The witness object must match the trace's `process_identifier`
  and `clock` (`CACurrentMediaTime nanoseconds`). Its `poses` array contains
  `time_ns`, `drawing_identity` and the actual published shared `camera` JSON;
  retain these through a subscription to `store.camera.$value`. Its `contacts`
  array contains a unique `id`, matching `drawing_identity`, `baseline_revision`,
  `full_contact_begin_ns` and `full_contact_end_ns`. Observe baseline geometry
  before contact; keep contacts complete and separate, with no document change
  inside them. The geometry needs viewport, translation, zoom, rotation and
  flipped axes. Missing or conflicting mappings remain unknown; later model
  state cannot relabel a presented frame. These diagnostics do not establish a
  moving-frame target or physical input latency. Older traces report encoded
  camera data as unavailable; existing report metrics and CLI calls are unchanged.
  Nonbaseline geometry includes frozen poses. Successive geometry progress,
  static presentations, same-geometry revisions and full-contact progress gaps
  are reported separately. Unknown mappings break adjacency. Late geometry
  observations attribute immutable revision state but cannot establish causal
  onset or timely progress; they remain explicit and make the witness partial.
- **Input.** The first presentation after each real input batch is a receipt
  proxy; it does not prove those samples reached the pixels. Predicted and
  correction batches are reported separately. `engine_consumed_paint_progress`
  instead joins each frame's shared-engine Down/Move timestamp to its actual
  presented callbacks. It counts distinct advancing timestamps once and retains
  missing, repeated, regressing, future and invalid-presentation counts. Older
  traces without that counter report it as unavailable. Engine consumption is
  separate from receipt; it still does not prove physical input latency or that
  every consumed sample changed the displayed pixels.
- **Other.** Reports give p50/p95/p99/max, overflow and invalid counts, display
  capability, memory footprint and thermal state. Empty measurements are null.
  Footprint includes startup growth and the recorder's own storage.

Recorded canvas readiness requires canvas and shader flags (mask 9).
`tests/persistence-owner.swift` checks that an owner frame exports the catalog
flag independently. Traces missing a required canvas flag cannot establish
readiness.

Canvas traces do not measure cached native chrome. Resizing or scrolling a
cached Core Animation layer can change its displayed geometry without a new
GPU surface write. A fresh-surface presentation count therefore cannot establish
the complete window's motion FPS or interval p99.

When complete presentation data is unavailable, record whole Core Animation
commits as **renderer submissions**, following the
[measurement rules](../../docs/performance/measuring.md). Attribute commits to
the owned process through structured process or thread records; context commits
and layout callbacks are not whole submissions. Bound intervals within each
observed gesture and retain thermal coverage, priming, native input and source
guards. Keep presentation FPS and p99 unavailable; renderer counts do not qualify
a tier. Alternative display collectors need validated nonempty frame counts and
interval scope before their rate can support a comparison.

## JSONL schema 1

The first line is metadata. Each other line is `[kind, a, b, …]` with unsigned
integer fields; trailing unused fields are zero. Host times are nanoseconds on
the `CACurrentMediaTime` clock, and frame IDs are admission timestamps.

| Kind | Fields in order |
| --- | --- |
| 0 tick | admission time, target time, admitted flag, denial reason (0 unspecified, 1 inactive, 2 owner pending) |
| 1 frame | ID, target, owner start, owner end, five CPU stage durations, latest real receipt ID |
| 2 input | enqueue ID/time, owner start/end, oldest/newest sample time, count, kind (0 real, 1 predicted, 2 correction), last phase, tool, accepted flag |
| 3 drawable | frame ID, acquire start/end, drawable ID, acquired flag |
| 4 presented | frame ID, presentation time, callback time, drawable ID |
| 5 memory | time, physical footprint, resident bytes, thermal state, Mach status |
| 6 display | time, pixel width/height, scale × 1000, maximum refresh rate |
| 7 GPU | frame ID, GPU queue span, status (1 valid, 2 readback failure, 3 invalid timestamps), raw GPU start/end ticks |
| 8 GPU status | time, support (0 uninitialized, 1 supported, 2 unavailable), requested/skipped/invalid/pending counts, poll-error flag |
| 9 state | time, frame ID, flags (1 canvas ready, 4 another frame needed, 8 shaders ready; bit 2 retired), frame-error flag |
| 10 activity | time, display-link awake flag |
| 11 workload | time, phase, profile ID, phase-dependent counters |
| 14 GPU clock | recorder time before, Metal CPU nanoseconds, Metal GPU ticks, recorder time after |
| 15 consumed paint | frame ID, latest real Down/Move timestamp consumed by the shared engine, submitted nonempty canvas-update counter; optional camera revision and native frame result when `encoded_camera_revision` is true |

The analyzer converts GPU ticks with the paired clock samples, following Apple's
[GPU timestamp conversion](https://developer.apple.com/documentation/metal/converting-gpu-timestamps-into-cpu-time).
Workload phases: 0 configuration (width, height, paint layers, brush ID,
diameter × 1000, prediction flag), 1 warm-up, 2 measurement begins,
3 measurement ends, 4 postlude ends, 5 failure, 6 producer sample, 7 photo prime
begins, 8 photo prime Undo restored, 9 measured contact Down, 10 measured contact
Up. Phase 8 records a successful-restoration flag; phases 9 and 10 record the
synthetic contact number and source input timestamp in nanoseconds. Phases 2, 3
and 6 record cumulative real sample and batch counts and the largest producer
lateness since the previous sample.

## Brush replay

The offscreen `photo_interaction` replay measures completed canvas updates per
second without input delivery or presentation. On the Mac:

```sh
cargo build --release -p layer-render-wgpu --example photo_fixture --example photo_interaction
target/release/examples/photo_fixture artifacts/photo.capy
target/release/examples/photo_interaction artifacts/photo.capy OUTPUT.csv \
  8 18446744073709551615 180 1000 circles PRESET_ID
```

The arguments are samples per update, cache allowance in MiB, updates, brush
diameter, path and preset. On the iPad the same public `replay` function must be
called from a small wrapper app through a C ABI; no wrapper is checked in.

Replay timestamps share one sample clock across seed, contact and zoom phases.
The Eraser seed settles its pending pen-up and raster backing before its
authored roots become the Undo baseline. Seed settlement is outside the timed
frames; the replay retains its input-drain, changed-artwork and exact Undo/Redo
checks.

## Rejected scheduling changes

These were measured on both devices and made presentation worse or no better.
Do not repeat them without a new explanation:

- Driving frames from `CAMetalDisplayLink` instead of `CADisplayLink`.
- Requesting a fixed `preferredFrameRateRange` at the display's maximum rate.
- Reserving a drawable for onscreen content, lowering the admission limit or
  shrinking the drawable pool.

Shorter CPU owner service alone is not evidence of smoother drawing; judge
scheduling changes by actual presentation intervals.

## Fast checks

```sh
cargo test -p layer-render-wgpu --lib frame_timing::tests
python3 -m unittest discover -s tools/performance -v
```

The Swift recorder, workload plan and frame driver have standalone fixtures
(`tests/frame-trace.swift`, `tests/drawing-workload-plan.swift`,
`tests/frame-driver.swift`), compiled with their `Shared/Bridge` sources as shown
in the [development guide](../../docs/development/apple.md#swift-fixtures).

## HDR color bubble motion

The same `tests/tool-bubble-motion.swift` fixture supports
`CAPY_BUBBLE_MOTION_HDR=1` with `CAPY_BUBBLE_MOTION_KIND=drag` or `intensity`.
It creates a real 9504 × 6336 F16 drawing, imports the reference photo through
production image placement, commits it, adds an empty selected paint layer and
uses Fit. The 216-point HDR color bubble has its paint pair, palette and recent
colors enabled beside four retained neighbors, with default glass. Record actual
bead counts: enabled options do not imply populated recent or palette rows.

Use the Release build command above and four fresh output/trace directories:
light/dark for each workload. Keep `CAPY_TRACE_SECONDS=90` and pass the existing
`CAPY_BUBBLE_MOTION_PHOTO`, `CAPY_BUBBLE_MOTION_OUTPUT` and
`CAPY_BUBBLE_MOTION_THEME` variables. Run sequentially with every other native
window, GPU test, build and CPU comparison idle.

Each condition has one prime followed by three six-second native mouse
contacts. Drag moves the HDR contour through four neighbors and restores its
exact authored position with Undo. Intensity follows a constant-angular-speed
triangular path, preserves artwork/workspace data, and records actual published
stops, marker positions and distinct values separately from canvas presents.
Both derive their 5–10-second windows from first/last observed owner or marker
movement with 100 ms edges; no presentation/FPS-dependent trimming is allowed.
Keep unchanged-pose samples and disclose interior gaps, zero/missing callbacks,
boundary coverage and thermal observations.

Retain the binary and compiled-input hashes, actual hardware/display metadata,
`setup.json`, screenshots, timestamped witnesses, windows and complete traces.
Report CPU owner wall time separately from GPU queue spans, which include gaps
between submissions. Metal canvas callbacks do not measure native overlay
presentation, fresh intensity-marker updates, physical HDR output or input
latency, and cannot qualify an Apple reference tier. Record current diagnostics
and reference-hardware status in the
[Apple native measurements](../../docs/performance/top-tier.md#apple-native-measurements).
