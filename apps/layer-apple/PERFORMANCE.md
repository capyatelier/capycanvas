# Apple performance observations

The iPad and Mac use the same optional recorder, serial render owner and Rust
GPU timer. Ordinary launches leave recording and timestamp submissions disabled.
The native CAMetalLayer subclass observes the drawables acquired by wgpu and
uses Metal's `addPresentedHandler` and `presentedTime` to record actual display
presentation. A display-link tick or completed Rust call is not a presentation.
The iOS Simulator SDK does not expose drawable IDs or presentation callbacks;
simulator runs omit these events and cannot establish presentation acceptance.

This is instrumentation, not hardware performance acceptance. The workload
matrix, physical input-to-pixel evidence, calibrated instrumentation overhead
and ten-minute sustained sessions remain required on both platforms. The
currently attached Mac display advertises 90 Hz; it cannot establish 120 Hz
presentation. Keep failing workloads and unsupported measurements visible.

## Capture locally

Build with `CAPY_CONFIGURATION=Release` for performance investigations. See
[README.md](README.md) for signing and build options. Debug captures are useful
for validating instrumentation but do not close performance gates.

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
- Input enqueue/owner times measure transport queueing. The first presentation
  associated with each successfully received, nonpredicted batch is a **receipt
  proxy**. The renderer may defer that input or consume only part of its queue.
  This association does not establish that those pixels were included, nor
  physical Pencil input-to-pixel latency. Prediction remains visual-only.
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
| 2 input | enqueue ID/time, owner start/end, oldest/newest sample time, count, predicted flag, last phase, tool, accepted flag |
| 3 drawable | frame ID, acquire start/end, drawable ID, acquired flag |
| 4 presented | frame ID, actual presentation time, callback observation time, drawable ID |
| 5 memory | observation time, physical footprint bytes, resident bytes, thermal state, Mach status |
| 6 display | observation time, pixel width/height, scale multiplied by 1000, maximum refresh rate |
| 7 GPU | frame ID, GPU queue span, status (1 valid, 2 readback failure, 3 invalid timestamps) |
| 8 GPU status | observation time, support (0 uninitialized, 1 supported, 2 unavailable), requested/skipped/invalid/pending counts, poll-error flag |
| 9 state | observation time, frame ID, flags (1 canvas ready, 2 catalog loaded, 4 another frame needed, 8 shaders ready), frame-error flag |
| 10 activity | observation time, display-link awake flag |

## Fast checks

```sh
cargo test -p layer-render-wgpu --lib frame_timing::tests
python3 -m unittest discover -s tools/performance -v
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/FrameTrace.swift \
  apps/layer-apple/tests/frame-trace.swift -o /tmp/capy-frame-trace-tests
/tmp/capy-frame-trace-tests
```

The GPU test requires timestamp-capable hardware and checks frame identity,
strictly positive timestamps, bounded pending observations, completion and slot reuse. The Swift test checks
concurrent capacity, overflow, freeze and late presentation records. The Python
tests preserve active missed frames while excluding idle gaps, prevent missing
or invalid timings becoming zeros, deduplicate input receipt associations and
exclude shader startup from the ready subset. These tests use no UI automation.
