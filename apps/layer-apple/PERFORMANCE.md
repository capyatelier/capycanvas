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

## Reading the report

- **CPU.** Owner queue age runs from display-link admission to owner execution;
  owner service includes the bridge and snapshot work. The five Rust stages are
  CPU preparation, drawable acquisition, viewport submission, present call and
  polling, not GPU durations.
- **GPU.** Timestamps bracket queue work across paint and viewport submissions,
  including CPU submission gaps: a GPU queue span, not isolated GPU busy time.
  Three readback slots bound the work and full slots skip observations; the owner
  never waits for a readback.
- **Presentation.** Actual `presentedTime` values give intervals and lateness
  against the display link's target. A zero time means skipped, not zero
  latency; missing callbacks are counted separately. Continuous cadence excludes
  intervals across recorded idle pauses. `frame_admission_to_present_ms` is
  software scheduling delay, not input-to-pixel latency.
- **Input.** The first presentation after each real input batch is a receipt
  proxy; it does not prove those samples reached the pixels. Predicted and
  correction batches are reported separately.
- **Other.** Reports give p50/p95/p99/max, overflow and invalid counts, display
  capability, memory footprint and thermal state. Empty measurements are null.
  Footprint includes startup growth and the recorder's own storage.

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
| 9 state | time, frame ID, flags (1 canvas ready, 2 catalog loaded, 4 another frame needed, 8 shaders ready), frame-error flag |
| 10 activity | time, display-link awake flag |
| 11 workload | time, phase, profile ID, phase-dependent counters |
| 14 GPU clock | recorder time before, Metal CPU nanoseconds, Metal GPU ticks, recorder time after |

The analyzer converts GPU ticks with the paired clock samples, following Apple's
[GPU timestamp conversion](https://developer.apple.com/documentation/metal/converting-gpu-timestamps-into-cpu-time).
Workload phases: 0 configuration (width, height, paint layers, brush ID,
diameter × 1000, prediction flag), 1 warm-up, 2 measurement begins,
3 measurement ends, 4 postlude ends, 5 failure, 6 producer sample. Phases 2, 3
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
