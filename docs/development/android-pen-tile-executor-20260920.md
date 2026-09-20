# Wacom: shared material executor qualification — 2026-09-20

This implements the [tile-overhead simplification plan](android-pen-tile-simplification-20260920.md).
The implementation is `2f69114d`, on top of the independently landed compositor
change `28499b2f`. The original fetched baseline was `24bd0565`, whose renderer
was unchanged from `39e9cba3`. All device timings use the primary
`art.capycanvas` application on Wacom `5ll21u1002931`.

The final matched comparison measures **5.5% more fast drawing updates** with
light probes and **14.2% less CPU time per drawing callback** with full probes.
The simpler code retains brush behavior. Undo/redo and cursor improvements are
not established, and the approximately 39 full-probe updates/s forecast was not
reached.

## What was simplified

Three material executors are replaced by one: committed paint, incremental
prediction and prediction from persistent pixels share resource selection,
initialization, evaluation and publication. The implementation uses the existing
`BrushEncodingContext`, `BrushEncodingTarget`, `BrushPassPlan` and sparse
`BrushTile` records. The change removes **127 lines overall**, including the new
modules and regression-test changes: 578 inserted, 705 deleted.

- Dry metadata uses dynamic uniform offsets. Changing a tile's position in the
  work list no longer requires constructing a new binding.
- Material bindings belong to their resource owners. Each cache retains one
  entry and replaces it when a dependent buffer or texture view changes. Coverage
  owns bindings that reference stroke state; color eviction explicitly releases
  coverage bindings that could otherwise retain retired color textures.
- Binding entries are boxed lazily. Two cache fields occupy 32 bytes per surface,
  rather than embedding large resource keys in every densely traversed page.
- Direct dry prediction uses the existing sparse contact plan. Every preview
  reader now checks current membership as well as its bounding rectangle, so an
  old pooled surface cannot override committed paint inside a clean gap.
- Independent dry tiles share a compute pass for the brush batch. Borrowed
  decoded-source slots retain their ordering boundary; independently owned pages
  and transparent inputs do not inherit the source cache's 16-slot limit.

Tile size remains 256 px. This milestone does not change brush shaders, spacing,
contact generation or ink semantics. Wet/nonlocal source dependencies,
watercolor transport and non-normal fragment blending remain explicit in the
shared executor. No new G-Pen-specific renderer, global resource cache or texture
atlas was added.

An exploratory scene-binding cache was removed. It lowered traced composition
CPU cost but did not establish a corresponding light-probe drawing advantage,
and two compact-cache runs had 550–560 ms redo presentation delays. The final
change keeps the existing bounded scene-binding lifetime. Those experiments are
retained as `after-*` and `compact-*`; they are not the final implementation.

## Workload and method

The canvas is the same 9504 × 6336 Sony photo (60,217,344 pixels), with hidden
Paper and an initially blank selected paint layer. G-Pen is 2048 px, opaque black,
pressure 1, at 17.164% zoom. The Diagnostics panel remains visible. Injected
stylus MotionEvents run at 200 Hz around physical center (1410,948), radii
(520,299), for ten seconds. Fast motion is one loop/s; slow is 0.25 loops/s.
Each workflow includes hover, stroke, undo, redo and hover again, with recorded
action timestamps.

Every capture records scheduling and actual canvas SurfaceView latches, plus
simultaneous 199 Hz simpleperf sampling. Full probes also collect application
CPU scopes, GPU phase intervals and command/allocation counters. Light probes
omit those extra scopes and phase queries. Compare like probe levels: these are
presented canvas updates, not isolated brush benchmarks or physical nib-to-photon
latency. Fast replay's contact grouping can change when the frame rate changes;
this is a fixed-input-stream test, not an identical-command-stream test.

Original-baseline/candidate/rollback runs precede the final matched comparison
with newly fetched main. Every APK and matching native symbol library is retained.
All action reports, including exploratory and slower results, are preserved in
the [measurement record](measurements/android-pen-tile-executor-20260920.json).

## Final comparison against newly fetched main

The additional main change landed during this investigation. It removes a
scratch copy for bounded displays; this Wacom workload admits the complete
display pyramid, which already used direct composition. The code path and
near-unchanged GPU composition intervals give no reason to transfer the reported
Apple bounded-display speedup to this workload. The separate `newmain-*` runs
measure that revision without this brush simplification.

| Metric | New main `28499b2f` | Final `2f69114d` |
| --- | ---: | ---: |
| Fast updates/s, light probes, 2 runs | 35.99, 34.29 | **36.89, 37.29** |
| Fast updates/s, full probes, 1 run | 32.69 | **34.59** |
| Slow updates/s, light probes, 1 run | 43.29 | **47.68** |
| CPU/drawing callback, full | 25.18 ms | **21.60 ms** |
| GPU interval, full | 20.57 ms | 19.90 ms |
| GPU composition interval, full | 10.73 ms | 10.77 ms |
| Native command-buffer allocation calls/callback | 125.98 | 100.99 |
| Fast undo to first presentation, light | 314–399 ms | 335–414 ms |
| Fast redo to first presentation, light | 403–519 ms | 428–515 ms |
| Fast hover updates/s, before and after | 79.0–82.6 | 74.6–84.2 |

The ratio of the two light-run means is **+5.5%**. The full-probe comparison is
+5.8%, with 14.2% lower CPU cost. The single slow comparison is +10.2%; it is
supporting evidence, not a repeated estimate. These small samples establish a
consistent direction across the original and newer baseline comparisons, not
a guaranteed percentage for arbitrary drawing.

Final CPU paint and prediction are 1.12 and 1.34 ms/callback, versus 2.42 and
2.93 ms on new main. Composition preparation/encoding is 5.14 versus 5.61 ms;
command finish plus submission remains 10.63 versus 12.37 ms. GPU paint and
prediction are 4.27 and 4.81 ms, versus 4.61 and 5.17 ms. Binding creation remains
about 292/callback including material input; the old incomplete counter is not
a comparable total.

Full-probe input-queue median / p95 decreases from 10.9 / 61.9 ms to
9.5 / 50.3 ms. This is queue residence, not end-to-end pen latency. Light-run
presentation p95 gaps remain approximately 42 ms, and the final runs still have
164–217 ms maximum gaps. The average exceeds 30 FPS; consistent sub-33.3 ms
delivery is not achieved. Fast full-probe redo improves in this particular pair
(475 → 420 ms), while light results overlap. Taken with the earlier regression,
there is no qualified general history-latency gain.

All final and new-main actions are fully retained, thermal status is 0 before
and after, and drawing upload-drain counters do not increase. The two full
integration traces contain 51 and 50 parser failures respectively, accounted
for by low-memory-killer diagnostic print records; their CSVs are retained.
The six corresponding light traces have zero parser errors.

The installed final APK is `integrated.apk`, SHA256
`c29efdf9e2dd469ee2ab26b21e036c7f44f16f32409e7977c874907328a84ade`.
Its native library matches runtime commit `2f69114d`; the subsequent report
commit changes documentation only.

## Isolated simplification results before the main integration

| Metric | Original baseline | Material simplification |
| --- | ---: | ---: |
| Fast presented updates/s, light probes | 35.59–36.89 (3 runs) | 37.19–37.69 (2 runs) |
| Fast presented updates/s, full probes | 33.49 (2 runs) | 35.99–36.29 (2 runs) |
| Scheduled CPU/drawing callback, full | 24.14–24.15 ms | 20.50–20.81 ms |
| Observed GPU interval, full | 19.88–19.99 ms | 19.18–19.37 ms |
| Slow presented updates/s, full | 41.98 (1 run) | 45.78 (1 run) |

Ratios of run means give **3.8% more fast updates with light probes**, **7.9%
with full probes**, and **14.5% less scheduled CPU per traced callback**. The
single slow comparison improves 9.1%; it is not a repeatability estimate.

| CPU work per fast callback | Baseline, mean of 2 runs | Simplified, mean of 2 runs |
| --- | ---: | ---: |
| Prepare | 1.98 ms | 2.18 ms |
| Committed paint | 2.43 ms | 1.12 ms |
| Prediction | 2.89 ms | 1.29 ms |
| Composition preparation/encoding | 4.87 ms | 4.69 ms |
| Command finish | 6.80 ms | 6.67 ms |
| Queue submission | 4.34 ms | 3.56 ms |

Nested binding and driver scopes must not be added to these enclosing stages.
Native `AllocateCommandBuffers` calls fall from about 124 to 99 per callback
(about 20%). Their directly measured CPU cost falls only about 0.11 ms; fewer
calls do not make all command-finalization work disappear.

The previous [complete binding census](android-pen-tile-simplification-20260920.md)
measured 593 creations / 5.319 ms per callback. The accepted candidate records
287–293 / 2.51–2.58 ms. The old ordinary `capy.allocate_binding` scope omitted raw
material-input construction; the new helper uses `PipelineDevice` and includes
it. Comparing the old incomplete 401-call counter with the new complete counter
would misstate the saving. Candidate material creation is about 77–84 calls per
callback; ordinary composition still constructs about 209.

GPU paint averages roughly 4.2 → 4.0 ms; prediction 5.07 → 4.68 ms; composition
stays near 10.5–10.6 ms. The gain is primarily CPU overhead removal, with a smaller
prediction GPU saving. There are no source-upload-drain increments during these
strokes; source admission remains 256 MiB resident / 64 MiB uploads.

Light-probe undo spans 307–332 ms before and 320–352 ms after; redo spans
426–470 ms before and 448–457 ms after. These observations do not establish an
undo/redo improvement. Hover remains approximately 80–84 updates/s. Full-probe
redo is worse: 420–427 → 497–527 ms for fast motion, and 473 → 605 ms for the
single slow comparison. It remains a measured regression under detailed tracing,
not a proven harmless instrumentation artifact. The final integration comparison
below is separate from those runs.

## Why the initial forecast was not achieved

The approximately 39 full-probe updates/s forecast assumed stable live resources
and nearly initial-only binding creation, including retained scene inputs. The
256 MiB mutable color cache retires inactive generations even while the active
stroke spans roughly 554–555 pages. Correct ownership releases the associated
bindings too. The measured creation rate therefore exceeds the initial-only
model, and the rejected scene cache is absent from the accepted implementation.
Do not present the forecast as a measured gain.

Remaining drawing cost includes approximately 10 ms of CPU command finish and
submission, approximately 4.7 ms composition CPU work, and approximately 10.5 ms
GPU composition intervals in the isolated candidate. CPU and GPU overlap;
these values cannot be summed into frame latency. Profiles still show driver
command-buffer freeing, framebuffer creation and allocator work. This evidence
does not establish external-memory-bandwidth saturation: no DRAM utilization or
shader occupancy counters were captured.

The prior model's theoretical CPU-free ceiling with unchanged observed GPU work
is approximately 1000 / 19.3 = 52 updates/s, before presentation/scheduling costs.
It is an interval-model ceiling, not the device's physical limit or a 52 FPS
promise. Further shared work should address composition command preparation and
driver work per tile; retaining bigger tiles or another cache is not justified by
this milestone.

## Correctness and scope

The full host renderer suite passed twice during development (315 passed,
31 intentionally ignored). After the final binding representation and scene-cache
removal, focused material and preview suites passed (17 and 19 tests, with one
existing preview test ignored). Cache-pressure coverage now exercises dry and
wet paint in U8/U16, retained versus evicted pixels and history. Sparse-preview
coverage moves contacts inside an unchanged bounding rectangle, checking stale
surface retirement and cancellation.

On the Wacom, the final isolated implementation passes native G-Pen, float
coverage, swept preview, watercolor microbatches, nonlocal smudge/liquify, wet
mixing, masked destination preview, constant-backdrop composition and cache
retirement: ten normal test executions, plus two forced portable-blending
executions. Shared display integration passes 23 host live-display and four mip
tests; Web compilation also passes. On the final integrated Wacom binary,
direct mip-tile composition and sparse-preview retirement pass, and direct
composition also passes with Float32 blending disabled. Existing vendored rav1d
warnings remain.

Repeated rendered-stroke screenshots are also checked at 39,600 interior samples
per fast run, all the expected ink color. This catches missing stroke interiors;
it does not claim identical boundary pixels from differently grouped live input.
The renderer pixel tests provide the stronger prediction/commit and history
checks.

This does not qualify the historical 2 Hz / 8 ms prediction device-loss case or
prove a freeze fix. Tail stalls remain, and restarting older builds previously
restored responsiveness too. No new readback-mapping, validation or device-loss
error was observed in the qualified runs.

## Reproduction and evidence

Raw evidence is under `artifacts/wacom-tile-executor-20260920/`: APKs, matching
libraries, Perfetto traces, action markers, simpleperf recordings, source
snapshots, screenshots, environment snapshots and test logs. Its README explains
each candidate family. `capture-run.py LABEL RATE [CONFIG]` records one workflow;
`report-run.py LABEL...` uses `tools/performance/android-pen-report.py` to derive
action results. `capture-integration.py FAMILY` runs the final comparison from a
fresh blank paint layer. Undo the preceding replay before subsequent captures.

Build the primary optimized benchmark APK with:

```sh
ANDROID_HOME=/home/babymastodon/Android/Sdk \
LAYER_CARGO_ABOUT=/tmp/capy-audit-tools/bin/cargo-about \
apps/layer-android/gradlew -p apps/layer-android :app:assembleBenchmark \
  -PcapyAbi=arm64-v8a -PcapyOptimize
```

Some retained exploratory/baseline traces contain systrace parser failures from
low-memory-killer diagnostic print strings. Per-run error counts are retained;
all action windows are checked for complete retention. These captures must not
silently be treated as error-free. Thermal status and cache-admission counters
are retained with the measurements.
