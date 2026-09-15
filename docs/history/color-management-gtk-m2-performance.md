# GTK SDR milestone 2 — final performance qualification

Status: **in progress; no acceptance claimed**. This final phase follows the
correctness checkpoint `125108bf`. See the [implementation evidence](color-management-gtk-m2-validation.md)
and [common gates](color-management-milestones.md#gates-that-apply-to-every-milestone).

## Declared reference envelope

Qualification targets the local Linux workstation: NVIDIA RTX PRO 6000
Blackwell Max-Q, Threadripper PRO 9995WX, GTK 4.22.4, Mutter 50.4 and the
120 Hz private Wayland display. Each run records actual versions, clocks/power
policy and competing GPU activity. This does not qualify constrained GPUs or
physical pen-to-photon latency. Other platform hosts remain outside the user's
authorization until GTK qualification is complete and approval is obtained.

The following budgets are declared before final acceptance runs. They are
workload limits, not permission for caches to grow to the workstation's VRAM
capacity. CPU figures include process RSS and worker buffers; GPU figures include
allocator reservations as well as live allocations. Driver totals are recorded
separately because driver-private allocations are not attributed by wgpu.
Steady means after pending work drains while documents/history remain open;
peak includes active edits, save/export, staging and worker overlap.

| Workload | CPU steady / peak | GPU reserved steady / peak | Interaction gate |
| --- | --- | --- | --- |
| Sparse 4096² drawing, 32+ layers, each SDR depth | 2 / 4 GiB | 1 / 2 GiB | CPU frame creation and completed work p99 ≤ 8.33 ms; native software input-to-present p99 ≤ 8.33 ms |
| One 24, 45 or 60 MP photo, U16 ProPhoto, retained source and adjustment history | 2 / 4 GiB | 1 / 2 GiB | Warm pan/zoom/painting ≤ 8.33 ms p99; slider preview ≤ 100 ms p95 / 200 ms p99; histogram ≤ 500 ms p95 / 1000 ms p99 |
| Three photo documents (24 + 45 + 60 MP), one save and one export overlapping active editing | 6 / 8 GiB | 3 / 4 GiB | Active warm drawing retains the 8.33 ms gate; background operations do not block GTK input |

Cold photo tiles and first use are measured separately: first visible response
to navigation/editing ≤ 100 ms p95 / 200 ms p99. Full-resolution global effects,
conversion and file operations may take longer, but must keep the GTK owner
responsive, show progress/cancellation, and remain inside peak budgets. Report
their whole-operation duration and cancellation acknowledgement; acknowledgement
after the current codec/CMM call is distinguished from immediate UI feedback.
The first visible preview must not be described as full-resolution completion.

Report missed deadlines, sustained behavior, maxima and sample counts alongside
percentiles. For unchanged work, investigate a repeatable p95/p99 increase greater
than `max(5% of baseline, 0.2 ms)`, even when the absolute gate passes. Extra
integer16 work remains subject to the absolute budgets and precision contract.
No display mip or half-float substitute may feed edits, exact queries or export.

## Benchmark coverage and comparison arms

The existing diagnostic factory at `125108bf` creates the old sRGB8 working
renderer. Its brush reports cannot qualify the native GTK Float32/integer-backed
path. The final harness replaces that factory with `new_native_headless`, retaining
the same input/engine/brush workloads. Configurations explicitly select sRGB,
Display P3, Adobe RGB or ProPhoto and integer8/integer16. Reports identify the
mode. Diagnostic export remains explicitly sRGB8 and occurs outside timing.

Fresh release builds and runs compare:

- Fixed program baseline `ebafa44`, before milestone 1's raster replacement.
- Milestone 2 starting point `e46f271`, separating milestone 1 cost from SDR work.
- Parent `125108bf`, the unchanged diagnostic working path immediately before
  replacing the factory.
- Current native sRGB8, P3 U8 and ProPhoto U16 arms, with follow-up pairs for any
  regression. The parent diagnostic arm is a reference, not a GTK performance claim.

Use the existing 25-scenario 4096² suite, eight coalesced samples per frame,
three repetitions per scenario, exact pipeline warm-up and undo. Serialize GPU
measurement runs and avoid overlapping builds. Retain exact executables, source
commits/hashes, logs, generated reports and environment records under
`artifacts/color-m2/final-performance/`. Offscreen reports are followed by native
GTK presentation/input checks and the photo/worker memory matrix above.

No latency, memory or platform gate passes merely because a benchmark completes.

## Native diagnostic factory checkpoint

The ABI now constructs `new_native_headless(document.color)` and validates both
space and integer depth before allocation. The mode is installed on the engine
document and renderer together. The benchmark accepts explicit `--space` and
`--depth`, applying them to every warm-up, measured, feedback and report canvas.
It no longer offers the superseded diagnostic sRGB8 working factory. Historical
comparison executables retain their own original code.

The initial native ABI run passed 11 of 12 tests, including all eight
space/depth configurations and invalid-mode rejection. The remaining test
asserted an old optimization: zero dry-prediction preview pages. Native integer
editing uses a private Float32 page for that 128² mark. The revised check bounds
it to one 256² page, verifies visible prediction, and checks exact whole-document
white restoration after cancellation. That focused test passes (3.90 s), making
all 12 applicable cases covered without weakening pixel restoration.

Initial exact test executable SHA-256:
`e01638512ec67515d0bb40be950109df19c724eac70e21e6418284fcf6a7b233`;
final focused executable:
`ea2988438a14acf2e39f35547fd846ea199959589d051b52f684d9f5cd8fde4a`.
The first build's test-only trait import error was corrected before execution.
Logs/captures use `native-bench-ffi-*` under `artifacts/color-m2/`. The release
native executable was built before the final test-only assertion change; its
production factory and benchmark code match this checkpoint. Its hash, the
three reference builds and exact source commits are in
`final-performance/builds.json`. Reproduction scripts and environment records
are retained there. Correctness tests overlapped CPU builds; their durations
are not used as performance evidence. The timed comparison begins only after
both builds and GPU correctness tests finish.

## First complete native comparison — failures remain

The initial native 4K run failed before its first frame. The bounded display
cache planned 274,022,656 bytes (261.33 MiB) for 4096² Float32 detail plus its
coarse image, reduction scratch and records, exceeding the 256 MiB component
cap. The cap now reserves 272 MiB for that complete allocation. This preserves
the existing 4096² workload's resolution and precision and stays inside the
previously declared 1/2 GiB aggregate GPU envelope. It is not a claim that total
GPU memory is qualified. Diagnostic ABI errors now retain the renderer's reason
on stderr instead of reporting only `RenderError`.

With that correction, all 25 scenarios execute in sRGB8, P3 U8 and ProPhoto U16.
Each arm has three repetitions; the original failed run remains retained.
The full runs expose substantial latency failures, especially palette-knife
frame creation and pen-up capture/publication. These results require fixes;
they are not accepted as the cost of higher precision.

| Arm | Worst move completed p99 ms | Worst pen-up completed p99 ms | Worst CPU move p95 / p99 ms | Scenarios exceeding 8.33 ms |
| --- | --- | --- | --- | --- |
| Fixed `ebafa44` | 3.971 | 3.386 | 2.072 / not reported by original harness | 0 |
| M2 start `e46f271` | 4.420 | 3.856 | 2.130 / 2.982 | 0 |
| Parent `125108bf` diagnostic path | 4.342 | 4.308 | 2.197 / 3.037 | 0 |
| Native sRGB8 | 16.337 | 26.579 | 10.612 / 14.318 | 13 |
| Native P3 U8 | 16.567 | 25.655 | 10.539 / 14.437 | 12 |
| Native ProPhoto U16 | 26.404 | 36.715 | 18.540 / 24.150 | 15 |

The final column counts a scenario if either move or pen-up CPU/completed p99
exceeds the absolute gate. Maxima in different columns can come from different
scenarios. Exact per-scenario rows, frame/deadline counts and capture charges are
in the corresponding generated reports and `comparison-data.json`. Fixed/M1/
parent runs took 65.36/69.62/70.85 seconds; corrected native sRGB8/P3 U8/ProPhoto
U16 runs took 93.30/92.95/96.25 seconds, including untimed setup and export.
Those whole-run durations are not interaction latency.

The selected GPU was NVIDIA index 3 in the telemetry inventory; other GPUs
remained idle. Power policy remained `amd-pstate-epp`, `powersave`,
`balance_performance`, with the GPU's 250 W limit unchanged. Record files
`environment.json`, `native-4k-environment.json` and per-arm `*-gpu.csv` retain
the full inventory, activity, clocks and temperature observations. Existing
driver allocations on that GPU are not charged as this benchmark's memory.
Per-process high-water comes from `/usr/bin/time`; renderer counters and global
driver usage remain distinct and do not by themselves pass aggregate memory.

Exact binaries and source hashes are in `builds.json` and
`native-4k-build-sources.json`; the corrected native executable is
`6d1eab2f03755b972faad3d6e1955406ed80bd585d905ab9153dce1435b5a20a`.
Commands and run boundaries are in `runs.json` and `native-4k-runs.json`.
`build-baselines.py`, `run-comparison.py` and `run-native-4k.py` reproduce these
arms. GPU measurement runs were serialized and did not overlap CPU builds or
other GPU validation runs. Follow-up phase probes are explicitly instrumented
diagnostics and will not be substituted for final uninstrumented measurements.

## Native photo and concurrent-save measurements

`raster_workloads` now constructs the native renderer and streams synthetic
source rows with real low-order U16 values. The old packed-source comparison
branch is removed; historical executables retain their original workloads.
Select `24mp`, `45mp`, `60mp`, `multiple` or `all`, with optional `--space` and
`--depth`; defaults are ProPhoto U16. The multiple case keeps 24 + 45 + 60 MP
documents simultaneously instead of two copies of the smallest photo.

These measurements use a 1024×768 offscreen view and 32+ layers. Each single
photo contributes 256 measured drawing frames during native saving, followed by
exact source/profile and committed-tile comparisons on reopening, undo/redo and
an explicit sRGB diagnostic export checksum. Native source/master data retain
U16 precision; the sRGB8 checksum is not the U16 identity test. The multiple
case alternates 768 drawing frames across three live devices/documents while
saving one snapshot. It does not yet include simultaneous profiled export,
sliders, histograms or GTK presentation.

| Case | Drawing CPU p95 / p99 ms | Completed p95 / p99 ms | Process high-water MiB | End-of-run GPU allocator reserved MiB |
| --- | --- | --- | --- | --- |
| 24 MP | 2.242 / 2.883 | 2.569 / 3.326 | 792.05 | 640 |
| 45 MP | 3.028 / 3.493 | 3.548 / 4.526 | 1078.71 | 640 |
| 60 MP | 4.183 / 4.898 | 4.599 / 5.423 | 1185.74 | 640 |
| 24 + 45 + 60 MP | 3.186 / 4.177 | 3.647 / 4.578 | 1669.68 | 1920 total |

Warm versus cold source frames are reported separately in the logs. All observed
drawing frames in these cases remained under 8.33 ms; the cold subset's worst
p99 was 7.24 ms on the 60 MP document in the multiple case. Single-photo
undo/redo completed in 30.81–57.43 ms; these are operation durations, not GTK
input blocking measurements. Source generation, device creation and the initial
submission are combined and excluded from drawing percentiles. Synthetic row
generation is not photo file-opening latency. Single-photo native save durations
were 74.39/131.34/162.68 ms; separate full export/checksum durations were
303.17/603.50/877.08 ms. Save can finish before all drawing frames do; the entire
drawing interval is not claimed to overlap disk work.

The 24 MP run used `native-photo-workloads` from
`native-4k-build-sources.json`. The final example only removes its always-empty
legacy asset map/preparation loop; `native-photo-final-workloads` runs 45 MP,
60 MP and multiple, with its hash and matching source recorded in
`native-photo-final-sources.json`. Exact checksums, allocator live/reserved
figures, RSS/high-water and commands are retained in `native-photo-*.log`,
`*-time.txt` and build records. Reproduce with `/usr/bin/time --verbose
target/release/examples/raster_workloads 60mp` after `cargo build --release -p
layer-render-wgpu --example raster_workloads --offline`.

These results fit the declared CPU and end-of-run GPU envelopes for these
specific cases. They do not establish peak allocation through every transient
or qualify the full multiple-job/photo-control memory and latency gate.

## Material-brush cache investigation

Instrumented ProPhoto U16 palette-knife runs locate the move stalls before
native commit encoding. Basic frame preparation is sub-millisecond; repeated
source initialization and material-neighborhood reads account for most of the
slow frames. Increasing only decoded-source slots reduces misses but does not
resolve the gate. Retaining more active working pages removes most move stalls,
while pen-up remains slow:

| Diagnostic working cache MiB / decoded slots | Move completed p99 ms | Pen-up completed p99 ms | Peak sampled GPU allocator reserved MiB |
| --- | --- | --- | --- |
| 256 / 16 | 25.593 | 28.629 | 1088 |
| 256 / 128 | 20.569 | 25.055 | 1344 |
| 512 / 16 | 6.766 | 26.291 | 1088 |

These are instrumented diagnostics, not acceptance timings or proposed cache
limits. Three repetitions contribute 426 move and 12 pen-up frames per policy.
The probe archives, exact build manifests, logs and `residency-summary.json`
under `final-performance/` distinguish measured frames from warm-up, undo and
export. Earlier phase probes separate native encoding from command finishing /
submission; they do not yet distinguish driver submission from encoder finish.

The production change keeps the 256 MiB working-color target. It discards
inactive blend surfaces outside this frame's destinations before evicting
completed canonical color. If the secondary surface owns current pixels, that
same texture/view becomes primary; no decoding, quantization or pixel copy is
introduced. Active color, coverage and material state remain intact. Destination
allocation also covers the whole stroke's final edge pass, so revisiting retired
scratch is safe. Transform previews retain their existing residency protection.

The uninstrumented paired run uses the same ProPhoto U16 palette-knife workload
and three repetitions per arm (426 move / 12 pen-up frames). Both binaries run
serially after CPU builds and GPU correctness checks have finished:

| Arm | Move CPU p95 / p99 ms | Move completed p95 / p99 ms | Pen-up CPU / completed p99 ms | Move / pen-up frames over 8.33 ms |
| --- | --- | --- | --- | --- |
| Before | 18.637 / 23.733 | 20.103 / 26.053 | 14.394 / 26.130 | 132 / 12 |
| Retire inactive companions first | 3.428 / 4.165 | 5.780 / 7.042 | 13.267 / 22.621 | 0 / 12 |

Maximum move time falls from 27.989 to 7.917 ms. Reported paint pages rise from
138 to 206 while reported canvas residency falls from 587 to 571 MiB: more
artwork remains available with less retained scratch. Process high-water falls
from 877,980 to 830,780 KiB; capture allocated/reserved peak is 173 / 177 MiB.
These component figures do not replace full allocator peak/steady qualification.
Pen-up still fails, and the other scenarios and native GTK presentation remain
to be measured after the remaining optimizations.

`run-companion.py`, `companion-runs.json`, the two generated reports, process
time files and GPU telemetry preserve reproduction and run boundaries. Exact
before/after executable hashes are
`6d1eab2f03755b972faad3d6e1955406ed80bd585d905ab9153dce1435b5a20a` /
`0ed1722629396254330c6be6bb911916eb77a6ea25d938dab8c2611e9eecc166`.
`companion-final-build-sources.json` and its matching patch record production
sources; later edits affect only the new test fixture and this report.

All seven existing cold-color cases pass with the production change, covering
composition, exact sampling, thumbnails, source-backed painting, filters,
transforms, prediction, terminal backing and saved history. The new pressure
case passes in both U8 and U16 with exact committed samples and undo/redo after
recreating every stroke-edge destination (28.75 s). Its initial fixture assumed
the cache target excluded current write scratch; retained failed runs document
the corrected budget and idle-redraw setup. Final test executable SHA-256:
`ae387997c82f9c4f9120bee2f213b0de5607ecf1aac45440dd02f3a8fe2ddcf5`;
logs/manifests use `native-companion-pressure-*` under `artifacts/color-m2/`.

## Batched canonical promotion

The next palette-knife probe separates command finishing from queue submission.
For 12 measured ProPhoto U16 pen-ups, it records 336 / 344 native color/scalar
inputs at p50 / p95. CPU p50 / p95 in microseconds: validation 994 / 1166,
encoding preparation 1622 / 1732, promotion preparation 994 / 1115, command
finish 4965 / 7270, queue submit 255 / 439. Capture preparation is 85 / 94.
This identifies command construction/finishing, not queue submission alone, as
the largest CPU cost. `build-penup-probe.py`, its source archive/build manifest,
`penup-probe-palette.log` and `penup-probe-summary.json` retain the instrumented
diagnostic; its timing is not substituted for acceptance measurements.

Canonical adoption now uses one compute pass per bounded batch instead of one
render pass per tile. Each dispatch loads canonical Float32 samples and stores
them into a distinct working texture, gated by the same publication-wide status.
Partial rectangles leave surrounding pixels untouched. Full-tile publication
reuses a 16-byte immutable parameter record, included in native storage metrics.
Color and scalar working targets declare storage usage; encoding, global
validation, bounded scratch reuse and asynchronous capture retain their order.
The old promotion vertex/fragment path is removed.

The format decision was checked against the pinned `wgpu-types 30.0.1`
`texture/format.rs::guaranteed_format_features`: both R32Float and Rgba32Float
support write-only storage without a new optional device feature. Compute
dispatches provide separate resource-usage scopes, consistent with the
[WebGPU synchronization model](https://www.w3.org/TR/webgpu/#programming-model-synchronization).
This changes shared renderer code for GTK qualification; no additional host is
integrated or qualified.

All three original promotion correctness cases pass (5.90 s); a new mixed
color/scalar batch checks independent partial regions across an empty entry
(3.67 s). Five native editing cases pass (54.25 s), including exact history,
save/reopen, device replacement, scalar coverage, late mixed-plane failure and
abandoned publication. Test binaries/logs use `native-compute-promotion-*`;
final test SHA-256 is
`6cacd25bcd7c3575d81972ed399f85edd16fce025cda5891afafc3ef6652a1d5`.
The subsequent accounting-only change adds the cached 16-byte uniform to metrics.

Uninstrumented ProPhoto U16 pairs use three repetitions of each scenario:

| Scenario | Before / after pen-up CPU p99 ms | Before / after pen-up completed p99 ms | Before / after move completed p99 ms |
| --- | --- | --- | --- |
| Palette knife | 14.374 / 10.325 | 23.464 / 21.761 | 7.092 / 6.881 |
| Transparent glaze | 7.169 / 6.105 | 14.210 / 12.561 | 1.979 / 1.837 |
| Large paintbrush | 8.766 / 7.512 | 14.443 / 13.970 | 2.742 / 2.810 |

Palette/glaze contribute 426 move and 12 pen-up frames per arm; large paintbrush
contributes 591 move and only three pen-ups. Pen-up deadline misses change from
12 to 12, 12 to 10, and 3 to 3 respectively. The after palette run has one move
at 8.596 ms; its p99 remains below 8.33 ms. These runs show an improvement but
do **not** pass the completed-work gate. The small pen-up sample is not a
sustained native presentation qualification.

`run-compute-promotion.py` records serial run commands, boundaries, process
high-water and GPU telemetry. No build or correctness run overlaps measurement.
Reports and `compute-promotion-runs.json` retain exact frame/CPU counts. Before
uses the saved companion executable; after SHA-256 is
`9add4aaf4ccac38ffab258f9204d93287912ca226f6cca63b927f16c058eff58`,
with matching `compute-promotion-accounted-build-sources.json` and source patch.

## Reuse full-tile parameters; reject an unhelpful counter change

The follow-up `compute-penup-probe` confirms the reduction in command finishing:
palette pen-up finish p50 / p95 is 2650 / 3944 µs, versus 4965 / 7270 µs before
compute promotion. Validation and resource preparation remain material: 1124 /
1267 µs for validation, 1722 / 1838 µs for encoding preparation, and 1132 / 1297
µs for promotion preparation. These are 12 instrumented pen-ups, not acceptance
timings. The archive, build script, exact manifest and summary retain the probe.

Clipping counters were nonzero in 54 of 348 capture-status observations across
the complete probe, including setup; the maximum observed count was 79,450.
A workgroup-counter experiment preserved all 160 numeric cases (10,485,760
pixels, zero code error), but did not improve the failing palette workload:
completed pen-up p99 was 20.981 ms before and 21.700 ms after. The shader change
was **removed**. `native-counter-source.patch`, its exact binary/manifest and
`writeback-counter-*` / `native-counter-*` logs retain this negative result.

The retained change prepares immutable full-tile encoding records with the
color/scalar pipelines. Requests select their depth, transfer curve and alpha
association without allocating a parameter buffer for every chunk. Partial
rectangles retain bounded private records. At the device's 256-byte alignment,
the cached color/scalar tables cost 4096 / 512 bytes; native storage metrics
include them. Per-batch `parameter_bytes` is zero for full tiles because the
encoder owns these shared records separately.

Five color tests pass (25.29 s), including 160 exact numeric cases, stable
repeated publication, restore/capture, failure and cancellation. Four scalar
tests pass (8.03 s), including both depths, partial packed words, mixed failure
and exact restore/capture cycles. Exact test SHA-256:
`c9a21ab61dc5de7333c41e54f5fd32157ea58b7d045b376c7d21311320c16e2e`.
Logs/manifests use `native-parameters-*` under `artifacts/color-m2/`.

The existing hardware kernel matrix also passes, with 100 measured samples
after 20 warm-up iterations per case. For unclipped ProPhoto U16, 16 full tiles
and fresh bindings, CPU preparation/submission p95 changes from 0.7288 to 0.6340
ms and completed p95 from 0.9250 to 0.8230 ms. These are matching Rust test
builds, distinct from release application timing. Exact raw results and all
other depth/curve/partial/clipped cases are in `writeback-counter-before.log`
and `writeback-parameters-benchmark.log`.

Two release pairs each contribute 426 move / 12 pen-up frames per arm:

| Pair | Before / after pen-up CPU p99 ms | Before / after completed pen-up p99 ms | Before / after completed move p99 ms |
| --- | --- | --- | --- |
| Initial | 10.719 / 9.531 | 20.612 / 20.658 | 7.077 / 6.772 |
| Repeat | 11.488 / 10.515 | 22.881 / 20.263 | 6.778 / 6.695 |

All pen-ups still miss 8.33 ms. The after repeat has one move at 8.819 ms;
other move deadline counts are zero. Native PNG outputs from the first pair
are byte-identical; exact U16 correctness is established by the numeric tests,
not by the diagnostic sRGB8 PNG. CPU creation improves in these pairs, but the
remaining completed-work gap is not qualified as GPU execution alone without
timestamps separating GPU work from host waiting/callbacks.

### Control the launch path when measuring setup

The candidate initially took 17.56 / 17.06 seconds for the entire three-repeat
process, versus 6.18 / 6.21 seconds before, despite comparable measured frames.
Existing slow-frame traces ruled out the unmeasured brush frames. Isolated
scope probes located the extra time in canvas creation. Parameter creation
itself took 0.038 ms for color and 0.032 ms for scalar; the first draw, undo,
export and teardown did not account for the difference.

Copying the **unchanged** before executable to a fresh path increased its
one-repeat process duration from about 2.7 to 5.3 seconds. A stronger control
alternated before/after/before/after at the same `/tmp` executable pathname,
preserving exact source binaries separately. Process times were 5.27, 2.42,
2.40 and 2.49 seconds: after the first launch, both versions have comparable
setup/whole-process duration. Executable context affects this startup comparison;
a driver cache is a plausible explanation, not a directly inspected fact.

Future before/after setup comparisons use a common launch path and explicitly
record cold versus reused context. Saved immutable binaries, commands and
hashes remain separate. This control does not qualify GTK startup or physical
presentation. `run-parameters-path-control.py`, `parameters-path-control-runs.json`,
`parameters-path-*`, both setup probe archives, and the trace/path-control logs
retain the investigation. Application pairs use `run-native-parameters.py` and
its repeat script; after SHA-256 is
`93230fc26db585597622d82cfa396b30cc56535f358c1bd13dee5dc7e9cfed97`,
with matching `native-parameters-build-sources.json` and source patch.
