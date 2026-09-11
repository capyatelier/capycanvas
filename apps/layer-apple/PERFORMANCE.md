# Apple performance observations

The iPad and Mac use the same optional recorder, serial render owner and Rust
GPU timer. Ordinary launches leave recording and timestamp submissions disabled.
The native CAMetalLayer subclass observes the drawables acquired by wgpu and
uses Metal's `addPresentedHandler` and `presentedTime` to record actual display
presentation. A display-link tick or completed Rust call is not a presentation.
The iOS Simulator SDK does not expose drawable IDs or presentation callbacks;
simulator runs omit these events and cannot establish presentation acceptance.

One ten-minute 4K watercolor session is recorded on each physical platform below.
Complete workload-matrix results, physical input-to-pixel evidence and calibrated
instrumentation overhead remain required on both platforms. The current Mac
display configuration advertises 90 Hz; this configuration cannot establish
120 Hz presentation. Keep failing workloads and unsupported measurements visible.

## Repeatable native drawing workloads

Set `CAPY_WORKLOAD` to run a synthetic fixture through the same serial input
owner, display link, Rust renderer, editor panels, history and recovery writer
as ordinary drawing. Use a separate benchmark bundle identifier for device
installs, and a separate DerivedData directory. The opt-in workload additionally
uses a new private persistence root under `Caches/CapyPerformanceSessions` for
every editor instance. It never reads or replaces the artist's normal settings,
workspace or recovery copies. Ordinary launches have no workload timer.

| Profile | Document | Paint layers, excluding paper | Brush / diameter | Synthetic prediction |
| --- | --- | --- | --- | --- |
| `ink` | 2048×2048 | 1 | G-Pen / 24 px | Off |
| `ink-predicted` | 2048×2048 | 1 | G-Pen / 24 px | On |
| `wet-watercolor` | 2048×2048 | 1 | Wet Watercolor / 320 px | On |
| `layered-4k` | 4096×4096 | 8 | G-Pen / 24 px | On |
| `wet-watercolor-4k` | 4096×4096 | 8 | Wet Watercolor / 320 px | On |

The 4K cases retain seven translucent full-document underpaint layers and the
active paint layer. Setup creates the document through the shared project job
and uses ordinary UI actions for layers, fills, brush selection and size. The
editor stays in its full default workspace. No brush quality settings are
reduced. Ten seconds of drawing warm up the fixture before measurement.

The versioned trajectory is in `DrawingWorkloadPlan.swift`: deterministic curves
inside the document, pressure varying from 0.25 to 1, 240 samples/second, 1.5-second
strokes and 0.1-second lift gaps. A main-run-loop producer delivers coalesced
batches at 120 callbacks/second independently of render admission. Predicted
points use the same native prediction path and remain visual-only. A delayed
producer catches up; a backlog of one second aborts the run instead of silently
dropping samples or lowering the input rate. Interval maxima expose producer
lateness, which is synthetic scheduling delay, not a physical Pencil metric.

`CAPY_WORKLOAD_SECONDS` is the measured duration after warm-up (default 600;
allowed 1–1800). The run records separate setup, warm-up, measured, end and
postlude markers. A ten-second postlude observes pen-up, deferred GPU work,
recovery and idle transitions; its end does not prove that rendering drained.
Trace recording defaults to the requested measurement plus a 140-second allowance
for setup/warm-up/postlude, but normally finishes at the postlude. An explicit
`CAPY_TRACE_SECONDS` overrides that limit, and can therefore truncate a run.

Launch the built, isolated Mac app through Launch Services to foreground it;
an occluded window cannot supply presentation evidence:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
python3 apps/layer-apple/scripts/prepare.py
python3 apps/layer-apple/scripts/project.py
xcodebuild -quiet -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-Mac -configuration Release -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath apps/layer-apple/DerivedData/PerformanceMac \
  PRODUCT_BUNDLE_IDENTIFIER=art.capycanvas.apple.mac.performance \
  CODE_SIGN_IDENTITY=- CODE_SIGNING_ALLOWED=YES build
open -n --env CAPY_WORKLOAD=ink --env CAPY_WORKLOAD_SECONDS=600 \
  --env CAPY_TRACE_DIRECTORY="$PWD/artifacts/performance/mac-ink" \
  apps/layer-apple/DerivedData/PerformanceMac/Build/Products/Release/CapyCanvas-Mac.app \
  --args -ApplePersistenceIgnoreState YES
```

For an installed iPad benchmark bundle:

```sh
xcodebuild -quiet -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-iPad -configuration Release -destination 'generic/platform=iOS' \
  -derivedDataPath apps/layer-apple/DerivedData/PerformanceDevice \
  PRODUCT_BUNDLE_IDENTIFIER=art.capycanvas.apple.ipad.performance \
  DEVELOPMENT_TEAM="$CAPY_APPLE_TEAM" 'CODE_SIGN_IDENTITY=Apple Development' \
  -allowProvisioningUpdates build
xcrun devicectl device install app --device DEVICE_ID \
  apps/layer-apple/DerivedData/PerformanceDevice/Build/Products/Release-iphoneos/CapyCanvas-iPad.app
xcrun devicectl device process launch --device DEVICE_ID --terminate-existing \
  --environment-variables '{"CAPY_WORKLOAD":"ink","CAPY_WORKLOAD_SECONDS":"600"}' \
  art.capycanvas.apple.ipad.performance
```

Keep the benchmark window visible and leave its editor untouched. Only one
benchmark window should run on each device. Copy traces from the benchmark's
container using its bundle identifier. Trace metadata labels input as synthetic
and records the fixture specification; reports separate the measured interval
from startup and postlude. `measurement_completed` means a complete, non-aborted
interval was recorded. It does not imply performance acceptance, pixel inclusion,
physical input latency, or successful coverage of the other profiles. Review
rejected input, frame errors, missing presentations, readiness and all warnings.
The report retains the first and last presentation's distance from the measured
interval boundaries, denied display-link admissions, frames without viewport
submission and missing/zero-time completions. A completed input producer cannot
establish continuous rendering if its window becomes occluded.

## Physical ten-minute baseline: 2026-09-11

Both Release apps completed version 1 of `wet-watercolor-4k`: 4096×4096, eight
paint layers plus paper, Wet Watercolor at 320 px, synthetic pressure and
prediction. Each ran ten measured minutes after ten seconds of drawing warm-up,
followed by the ten-second postlude. The shared renderer and UI include incoming
changes through `d8a130b`. No other build or GPU test ran during measurement.
The iPad viewport was 2752×2064 physical pixels; Mac was 2400×1740 after window
layout settled. Separate benchmark bundles and private persistence roots were used.

| Measured interval | Physical iPad | Native Mac |
| --- | ---: | ---: |
| Duration, seconds | 600.000 | 600.009 |
| Display maximum, Hz | 120 | 90 |
| Nonpredicted samples delivered | 135,001 | 135,003 |
| Actual presentations | 65,407 | 49,266 |
| CPU owner service p50 / p95 / p99 / max, ms | 2.180 / 3.643 / 9.120 / 17.779 | 3.312 / 4.927 / 6.083 / 13.038 |
| CPU service over 8.33 ms | 1,092 | 28 |
| GPU queue span p50 / p95 / p99 / max, ms | 5.500 / 8.279 / 9.423 / 19.037 | 8.228 / 10.367 / 11.632 / 23.255 |
| Missing GPU samples | 204 | 221 |
| Presentation interval p50 / p95 / p99 / max, ms | 8.333 / 8.334 / 33.332 / 125.003 | 11.111 / 11.111 / 66.667 / 111.112 |
| Positive display-link target lateness p50 / p95 / p99 / max, ms | 8.338 / 8.352 / 16.670 / 28.032 | 22.297 / 33.411 / 33.419 / 66.741 |
| Display-link ticks denied admission / total | 1,132 / 66,914 | 241 / 49,882 |
| Producer interval-maximum lateness p50 / p95 / p99 / max, ms | 26.783 / 36.283 / 39.233 / 44.115 | 47.481 / 55.118 / 56.533 / 57.713 |
| Peak physical footprint, MiB | 1,798.05 | 1,910.91 |
| First-to-last measured footprint growth, MiB | +0.17 | +148.83 |
| Observed thermal states | Nominal | Nominal |

Presentation distributions retain the deliberate pen-up gaps; their high
percentiles must not all be called missed drawing frames. Every measured
presentation exceeded its display-link target by more than 1 ms. Neither target
lateness nor producer scheduling delay is physical input-to-pixel latency. GPU
queue spans include CPU submission gaps and the uncalibrated profiler; skipped
readbacks may bias their tails. They do not isolate GPU execution time.

Both measured intervals have zero rejected input batches, missing presentation
callbacks and zero-time presentations. The full traces have zero renderer errors
and recorder overflow. Each interval includes 375 admitted frames without a
viewport submission. The first/last actual presentations lie within 8 ms of
both interval boundaries on both hosts. The Mac's completed canvas was captured
after export and visibly contains the synthetic paint; no per-frame pixel oracle
or physical input latency assertion is inferred from that capture. Memory
includes document/history, ordinary recovery work and recorder storage; the Mac
growth remains to be characterized. Nominal thermal samples do not establish
the absence of clock-frequency changes.

These results leave the 8.33 ms tail budget and complete performance acceptance
open. The other four profiles still need ten-minute runs on both platforms,
along with physical input, overhead calibration and Mac 120 Hz presentation
evidence on a suitable display configuration. Artifacts stay local under
`artifacts/performance/{ipad,mac}-workload-sustained`.

A preceding twenty-second ink experiment reduced the Metal drawable count from
three to two. On iPad it increased median owner service from 1.113 to 9.482 ms,
with median drawable acquisition at 8.305 ms and median presentation intervals
at 16.667 ms. The Mac's median target lateness improved, but CPU budget
exceedances increased. The experiment was reverted on both targets; these final
runs retain three drawables. The retained scheduling change publishes native
canvas-readiness accessibility updates once per attached surface instead of
every submitted frame. The synthetic producer also leaves lift gaps asleep.
The pilot changed multiple factors, so it does not isolate this change's benefit.

## Large document replay safety

A direct Metal regression reproduced the startup device loss seen while
investigating frame scheduling: replaying seven filled 4096×4096 paint layers
in one frame attempted to create 4097 outstanding native command buffers.
The reproduction uses no native window or frame recorder. Incremental layer
fills succeeded, which explains why setup timing could hide the problem.

The shared renderer now records at most 512 render/compute passes per chunk,
then finishes and submits chunks in order after closing staging uploads.
Upload completion stays attached to the last chunk. The complete replay matches
every pixel of the incremental 4K image, with renderer telemetry off and on.
See the [renderer regression command](../../crates/layer-render-wgpu/README.md).
This fixes submission capacity; it does not close the sustained frame-time or
presentation requirements above. The published Apple scheduler is unchanged.

## Display scheduling comparison — 2026-09-11

The shared CAMetalDisplayLink experiment was **not adopted**. It supplied each
callback's drawable to the serial render owner and separated the CPU commit
deadline from the presentation target. The shorter CPU frame times did not
establish better presentation: the physical iPad repeatedly reported skipped
drawables with both requested rendering windows, including with GPU timing
disabled. The published hosts retain CADisplayLink and ordinary wgpu acquisition.

Each row below is a separate twenty-second measured `wet-watercolor-4k` run,
with ten-second warm-up and postlude. CPU times include the whole owner service;
admission-to-display times use actual nonzero Metal presentation callbacks.

| Host / requested Metal latency | GPU timer | Actual presentations | Zero-time presentations | CPU p99, ms | Admission-to-display p99, ms |
| --- | --- | ---: | ---: | ---: | ---: |
| iPad / 1 | On | 2,156 | 49 | 4.832 | 24.957 |
| iPad / 2 | On | 2,137 | 54 | 4.640 | 24.970 |
| iPad / 1 | Off | 2,154 | 57 | 4.387 | 24.916 |
| Mac / 1 | On | 1,642 | 0 | 6.371 | 33.380 |
| Mac / 2 | On | 1,655 | 1 | 5.875 | 33.376 |
| Mac / 1 | Off | 1,655 | 1 | 6.046 | 33.377 |

All six intervals completed with no renderer errors or missing presentation
callbacks. The Mac reports 90 Hz and the iPad 120 Hz. Changing the requested
latency did not change the observed target-to-deadline separation: approximately
22.222 ms on Mac and 8.333 ms on iPad. These observations do not demonstrate that
the requested latency is the actual end-to-end latency. Timer-disabled rows have
no GPU-duration samples; these short pairs do not calibrate all recorder overhead.

The experiment also exposed an unsafe explicit CATransaction commit on the
render queue: on iPad it invoked UIKit layout off the main thread and crashed
during attachment. That trial was removed before the six completed runs above.
A failed run produced no new trace; an older container file was rejected as
evidence. Collection must check file freshness and configuration against the
specific launch, not assume that the latest existing file belongs to it.

Local raw evidence remains under `artifacts/performance/{mac,ipad}-metal-link-`
`{safe1,safe2,no-gpu}`. The experiment is saved locally, not shipped in either
Apple target. Further scheduling work needs a new explanation and presentation
evidence; lower CPU measurements alone are insufficient.

After restoring CADisplayLink and integrating shared changes through `a7c048c`,
the same Release binaries were run once with GPU timing enabled and once with
it disabled. All four twenty-second intervals completed with zero rejected
input batches, missing callbacks or zero-time presentations:

| Stable host | GPU timer | Actual presentations | CPU p99, ms | Admission-to-display p99, ms |
| --- | --- | ---: | ---: | ---: |
| iPad | On | 2,190 | 9.077 | 25.965 |
| iPad | Off | 2,188 | 9.120 | 25.966 |
| Mac | On | 1,639 | 6.232 | 32.897 |
| Mac | Off | 1,655 | 5.873 | 32.928 |

The whole traces have no renderer errors or recorder overflow; startup/postlude
zero-time callbacks and omitted GPU samples remain in the reports. The iPad CPU
tail exceeds 8.33 ms with either instrumentation setting. Enabled GPU queue spans
have p99 9.389 ms on iPad and 10.363 ms on Mac and retain the overhead caveat.
This short pair does not establish a precise overhead correction or sustained
performance acceptance. The stable controls include subsequent shared layout
changes, so their comparison against the earlier Metal-link runs is not a
strictly identical-source scheduler-only experiment.
Reports are in ignored `artifacts/performance/{mac,ipad}-scheduler-control`
and corresponding `-no-gpu` directories. The Mac's completed synthetic painting
was captured and inspected; test apps were closed after collection.

## Capture locally

Build with `CAPY_CONFIGURATION=Release` for performance investigations. See
[README.md](README.md) for signing and build options. Debug captures are useful
for validating instrumentation but do not close performance gates.

Set `CAPY_TRACE_GPU=0` to retain CPU, input, memory and actual presentation
observations while disabling the GPU timestamp marker submissions and readback
polls. The default is enabled when tracing is requested; ordinary unrecorded
launches still create no frame timer. The trace header and report expose
`gpu_timing_requested`. Disabled GPU measurements remain null, with an explicit
warning; they must not be treated as zero GPU cost. This comparison isolates
the optional GPU timer, not the remaining recorder overhead. Use the same build,
workload, duration and display state for each pair.

```sh
CAPY_CONFIGURATION=Release bash apps/layer-apple/scripts/build.sh macos
open -n --env CAPY_TRACE_SECONDS=30 \
  --env CAPY_TRACE_DIRECTORY="$PWD/artifacts/performance/mac" \
  apps/layer-apple/DerivedData/Build/Products/Release/CapyCanvas-Mac.app
```

For iPad, build/install the Release app using the README commands and then:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
xcrun devicectl device process launch --device DEVICE_ID --terminate-existing \
  --environment-variables '{"CAPY_TRACE_SECONDS":"30"}' art.capycanvas.apple.ipad
```

Keep the app foregrounded through the interval and the two-second callback grace
period. Copy its local trace directory after export:

```sh
xcrun devicectl device copy from --device DEVICE_ID \
  --domain-type appDataContainer --domain-identifier art.capycanvas.apple.ipad \
  --source Documents/Performance --destination artifacts/performance/ipad
python3 tools/performance/apple_trace.py TRACE.jsonl \
  --output artifacts/performance/report.json
```

`CAPY_TRACE_SECONDS` must be finite, positive and no more than 3600. The default
output directory is `Documents/Performance` in the app container. The optional
`CAPY_TRACE_DIRECTORY` overrides it with a local writable directory. A finished
trace is a JSONL file; `.partial` means export did not finish. Recording starts
when the session owner is created, so startup is included. Shader/catalog/canvas
readiness and display-link activity transitions are recorded separately.

Artifacts are ignored by Git. Records contain timings, counts, memory, thermal
state and display dimensions; they contain no coordinates, artwork, document
names, account/team/device identifiers or input-device serials. Keep build,
signing, device-tool output and trace files local. Use placeholders in shared
commands and review staged source before pushing.

## Interpret the report

- CPU owner queue age measures the interval from display-link admission to
  serial-owner execution. Owner service includes the bridge and snapshot work.
  The five Rust stages are CPU preparation, drawable acquisition, viewport
  submission, present call and polling. They are not GPU execution durations.
- GPU timestamps bracket queue work across paint and viewport submissions,
  including CPU submission gaps. They measure a **GPU queue span**, not isolated
  GPU busy time. Three readback slots and a 256-result queue bound profiler work;
  full slots skip observations. The owner never waits for a readback. A bounded
  trailing poll drains the last result when the display link sleeps. Each marker
  pass performs a tiny storage write; empty passes produced zero counters on
  Metal. Counter resolution is deferred until the marker submission completes:
  resolving within that submission returned stale values on the tested Mac.
  Both marker work and extra submissions contribute profiler overhead. The older
  renderer telemetry also rejects zero counters; its empty-pass approach still
  needs migration and is not used for these Apple reports.
- Actual `presentedTime` values determine presentation intervals and lateness
  against the display link's target. Zero presentation time means skipped or
  unpresented, not zero latency. Missing callbacks are reported separately.
  Continuous cadence groups frames by display-link activity cycle, excluding
  intervals across recorded idle pauses. All intervals are also reported.
  The 8.33 ms exceedance count uses a 5% cadence tolerance; target lateness over
  1 ms is a separate descriptive count. Neither is an acceptance waiver.
  `frame_admission_to_present_ms` measures actual display time minus frame
  admission, independently of the scheduler's advertised target. It is software
  scheduling delay, not physical input-to-pixel latency. Comparing target
  lateness alone across different schedulers can conceal a changed target.
- Input enqueue/owner times measure transport queueing. The first presentation
  associated with each successfully received, nonpredicted batch is a **receipt
  proxy**. The renderer may defer that input or consume only part of its queue.
  This association does not establish that those pixels were included, nor
  physical Pencil input-to-pixel latency. Prediction remains visual-only.
  Corrections have separate batch counts, owner queue distributions and receipt
  proxies. Their sample timestamps remain the original observation times;
  correction delivery is measured from the new enqueue receipt.
- The report includes p50/p95/p99/max, missing/invalid/overflow counts, display
  capabilities, memory footprint and thermal states. Empty measurements are
null, not zero. Readiness requires canvas, shaders and bundled filter catalog.
  Startup memory growth and profiler storage are included in footprint; they
  must not be described as steady-state document growth.

The recorder reserves a capped array of fixed-size events (reported in the file
header), uses a short lock for concurrent callback appends, freezes once, and
streams JSONL on a utility queue. Memory sampling runs once per second. Timing
records and GPU timestamp submissions have overhead; run paired instrumentation
on/off investigations before drawing performance conclusions. Overflow or GPU
skips can bias distributions and must remain visible. Export keeps late callback
records for two seconds; missing completions at that boundary stay unverified.

## JSONL schema 1

The first line is metadata. Each remaining line is `[kind, a, b, ..., j]` with
unsigned integer fields; unused fields are zero. Times and durations use
nanoseconds in the CACurrentMediaTime monotonic clock domain. Frame IDs are
the admission timestamp. GPU timestamp differences are converted using the
queue's timestamp period, not compared as absolute CPU clock values.

| Kind | Fields in order, excluding trailing zeros |
| --- | --- |
| 0 tick | admission time, target time, admitted flag |
| 1 frame | ID, target, owner start, owner end, five CPU stage durations, latest nonpredicted receipt ID |
| 2 input | enqueue ID/time, owner start/end, oldest/newest sample time, count, kind (0 real, 1 predicted, 2 correction), original last phase, tool, accepted flag |
| 3 drawable | frame ID, acquire start/end, drawable ID, acquired flag |
| 4 presented | frame ID, actual presentation time, callback observation time, drawable ID |
| 5 memory | observation time, physical footprint bytes, resident bytes, thermal state, Mach status |
| 6 display | observation time, pixel width/height, scale multiplied by 1000, maximum refresh rate |
| 7 GPU | frame ID, GPU queue span, status (1 valid, 2 readback failure, 3 invalid timestamps) |
| 8 GPU status | observation time, support (0 uninitialized, 1 supported, 2 unavailable), requested/skipped/invalid/pending counts, poll-error flag |
| 9 state | observation time, frame ID, flags (1 canvas ready, 2 catalog loaded, 4 another frame needed, 8 shaders ready), frame-error flag |
| 10 activity | observation time, display-link awake flag |
| 11 workload | observation time, phase, profile ID, phase-dependent counters |

The analyzer also retains local scheduling experiment records: kind 12 contains
frame ID, CPU commit deadline, presentation target and drawable admission status
(0 ordinary acquisition, 1 supplied drawable accepted, 2 stale drawable rejected).
The optional sixth field of kind 6 records the requested Metal frame latency;
zero means unavailable. The published CADisplayLink hosts do not emit these
experimental fields. Owner completion includes polling and snapshot publication,
so lateness relative to the commit deadline is an upper bound, not a measured
Metal commit timestamp.

Workload phases: 0 configuration, 1 warm-up begins, 2 measurement begins,
3 measurement ends, 4 postlude ends, 5 failure, 6 producer sample. Phase 0's
remaining fields are width, height, paint-layer count, brush ID, diameter ×1000
and prediction flag. Phases 2/3/6 record cumulative nonpredicted sample and batch
counts, followed by the maximum producer lateness since its previous sample.
The metadata `workload` object includes the profile version, expected duration
and sample rate. These additions retain schema 1; older traces omit them.

## Fast checks

```sh
cargo test -p layer-render-wgpu --lib frame_timing::tests
python3 -m unittest discover -s tools/performance -v
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/FrameTrace.swift \
  apps/layer-apple/tests/frame-trace.swift -o /tmp/capy-frame-trace-tests
/tmp/capy-frame-trace-tests
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/DrawingWorkloadPlan.swift \
  apps/layer-apple/tests/drawing-workload-plan.swift -o /tmp/capy-workload-plan-tests
/tmp/capy-workload-plan-tests
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/CanvasFrameDriver.swift \
  apps/layer-apple/tests/frame-driver.swift -o /tmp/capy-frame-driver-tests
/tmp/capy-frame-driver-tests
```

The GPU test requires timestamp-capable hardware and checks frame identity,
strictly positive timestamps, bounded pending observations, completion and slot reuse. The Swift test checks
concurrent capacity, overflow, freeze and late presentation records. The Python
tests preserve active missed frames while excluding idle gaps, prevent missing
or invalid timings becoming zeros, deduplicate input receipt associations and
exclude shader startup from the ready subset. These tests use no UI automation.
Workload checks cover contact termination, lift gaps, pressure, coordinates and
invalid configuration. Report checks retain incomplete/failed measurements and
missing render observations even when the input producer completes.
