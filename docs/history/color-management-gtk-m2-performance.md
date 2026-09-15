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

## Separate pen-up GPU stages from completion waiting

An isolated probe of `4069ef07321867d370178fc6da4e6581b60b9e90` records GPU
queries before/after native validation, each bounded encode/promote/capture
chunk, composition and command-buffer boundaries. The release palette-knife
ProPhoto U16 run has three repetitions and 12 measured pen-ups. Each uses one
command buffer, ruling out gaps between application command-buffer submissions
for these observations. The probe requests `TIMESTAMP_QUERY_INSIDE_ENCODERS`
only in its archived headless constructor; production code is unchanged.

| Instrumented span | p50 ms | p95 ms |
| --- | ---: | ---: |
| Entire recorded GPU frame | 5.121 | 6.880 |
| Native validation | 0.824 | 0.894 |
| Native encoding, summed chunks | 1.255 | 1.575 |
| Canonical promotion, summed chunks | 0.327 | 0.363 |
| Capture copies, summed chunks | 2.074 | 2.254 |
| Scene composition | 0.413 | 0.903 |
| CPU command finishing | 2.784 | 3.947 |
| CPU queue submission | 0.133 | 0.302 |
| CPU frame creation, benchmark clock | 7.832 | 10.387 |
| Completed frame, benchmark clock | 15.641 | 21.116 |
| Final completion wait | 7.819 | 12.133 |

Quantiles of component spans do not add to quantiles of complete frames.
The per-frame wait minus recorded GPU span has p50 / p95 2.794 / 5.253 ms.
That residual is **not established as CPU callback cost**: the marker span
excludes queue-inserted work before its first query, and waiting can include
polling, mapping, driver and worker activity. Capture copies are a material GPU
cost, while CPU preparation/command finishing and the unclassified residual
also remain relevant. Optimizing shaders alone is not a demonstrated solution.

Timestamp stages are diagnostic observations, not isolated kernel timings or
native presentation acceptance. The pinned wgpu 30.0.1 Vulkan implementation
writes encoder timestamps at `BOTTOM_OF_PIPE`; wgpu documents that command
reordering can affect their position relative to surrounding work. The probe
checks nonzero/monotonic queries, includes query overhead in its span, and does
not add blocking waits. See [wgpu's timestamp contract](https://docs.rs/wgpu/30.0.1/wgpu/struct.CommandEncoder.html#method.write_timestamp).

Reproduce using `build-penup-gpu-timeline.py`, then serially run
`run-penup-gpu-timeline.py` on the GPU and
`analyze-penup-gpu-timeline.py`, all under
`artifacts/color-m2/final-performance/`. The builder retains its complete source
archive and changed-file hashes. Exact executable SHA-256 is
`830f6e8287c1827b8925c6610772decb153a3bff8352deead12c558cbf925549`.
`penup-gpu-timeline-*` retains hardware/power information, run commands, raw
logs, report, source manifest and per-frame summary. Analysis joins asynchronous
query callbacks to measured submission IDs, including one callback delivered
after its measured-window end; all 12 measured submissions have valid queries.
No compilation or other GPU test overlaps this measurement. The 8.33 ms
completed-work gate remains open.

## Reject packed color capture; reuse publication texture views

A packed native RGBA buffer prototype replaced integer output textures with
one/two little-endian storage words per U8/U16 pixel. It passed 13 native
color/scalar/promotion tests (33.60 s), including 160 cases / 10,485,760 pixels
with zero code error; nine native runtime/capture tests (61.86 s), including
save/reopen/undo/device replacement and late failure/cancellation; and the
Float32 scene-to-native low-alpha identity test (1.53 s). Exact test SHA-256:
`38c07f17c5d04e6350c0a9c349f5f23e79f8082bf7c8aa89b680c17cd2a63569`.

Two fixed-launch-path release palette-knife ProPhoto U16 comparisons did not
show a repeatable gain. Completed pen-up p99 changed 20.000 → 22.810 ms in the
first pair and 19.685 → 19.549 ms in the repeat; CPU pen-up p99 changed
10.003 → 12.499 ms and 9.942 → 9.595 ms. All 12 pen-ups per arm failed the
8.33 ms budget. Diagnostic PNGs were byte-identical, separately from the exact
native tests. The entire packed-buffer candidate was **removed**, including its
fixture/API changes. `native-packed-source.patch`, exact binary
`f518d79d3d3ae58862b21eec1f3ef78901c4b1fa8e6ff3b500ad705c9a35fdfb`,
matching manifest, tests, reports, `run-native-packed.py` and run/environment
records retain this negative result. It does not establish that texture copies
are intrinsically faster; this replacement failed to improve the target workload.

The retained change shares full default texture views while recording one native
publication. Validation, color/scalar encoding and canonical promotion use the
same views for each working or fixed scratch texture. All primitive preflight
checks remain in their common preparation implementation. Standalone primitive
calls own a temporary view set; the native owner shares one across every bounded
chunk. Only default views constructed from validated textures enter this set.
It is dropped when publication recording returns, before the next input frame,
and cannot retain cold artwork between frames. It allocates no additional pixel
storage; the maximum view count is the admitted input count plus the fixed
scratch textures.

Thirteen native primitive tests pass (36.12 s), again with all 160 exact numeric
cases, and all five native runtime tests pass (50.16 s), including multichunk
scratch reuse, abandoned/failed publications, exact save/undo/reopen and device
replacement. Exact test SHA-256:
`ce94663a12500396d8560ce50a5b82e2c03f45843496ad683d0807b368e5b824`.
Logs and manifests use `native-views-*` under `artifacts/color-m2/`.

Each before/after release arm below has three repetitions, 426 move frames and
12 pen-up frames. Runs alternate before/after/before/after at the same launch
pathname, retaining the immutable source executables separately.

| Pair | Before / after pen-up CPU p99 ms | Before / after completed pen-up p99 ms | Before / after completed move p99 ms |
| --- | --- | --- | --- |
| Initial | 9.703 / 8.673 | 21.124 / 16.794 | 7.586 / 6.877 |
| Repeat | 10.046 / 8.056 | 19.634 / 16.516 | 6.938 / 7.156 |

All pen-ups still fail 8.33 ms. Move misses are 0 → 1 and 1 → 0; the after
initial maximum is 8.662 ms, and the after repeat maximum is 8.221 ms. Canvas
storage remains 571 MiB and peak capture allocated/reserved remains 177 MiB
in these reports. The small move differences do not establish a movement-path
improvement. The repeatable pen-up reduction supports retaining view reuse;
it does not qualify the complete latency/resource matrix.

`run-native-views.py`, `native-views-runs.json`, reports, images and hardware/power
records under `final-performance/` reproduce this comparison. No build or GPU
test overlaps measurement. Before is the saved parameter-reuse executable;
after SHA-256 is
`926711fd357375cbefe227d21430bec459f964daad287a8778582256b7322527`,
with `native-views-build-sources.json` and `native-views-source.patch` recording
its exact parent and source changes.

## Larger staging retention does not resolve the pen-up delay

An isolated allocation probe of `5a5d0978` counts staging reuse, misses and
allocated bytes around each submitted frame. The same exact executable selects
64, 128 or 256 MiB of pool retention once at process startup; no production
setting or limit changes. Two rounds each run those three capacities in order
at a common executable pathname, three repetitions per arm. The ProPhoto U16
palette workload contributes 12 pen-ups per arm, 24 per capacity.

At 64 MiB, each measured pen-up allocates 32.5–52 MiB of fresh staging; each
12-pen-up arm has 132 misses totaling 508.5 MiB plus 456 bytes. At 128 and
256 MiB, large allocation misses disappear: only 48 eight-byte status-buffer
misses remain per arm (384 bytes total), due to the separate 16-status-buffer
retention ceiling. This confirms the allocation hypothesis but **not** its
proposed latency benefit.

| Pool capacity | Round 1 / 2 pen-up CPU p99 ms | Round 1 / 2 completed pen-up p99 ms | Peak capture allocated/reserved MiB |
| --- | --- | --- | ---: |
| 64 MiB | 8.357 / 8.251 | 16.823 / 16.549 | 177.0 |
| 128 MiB | 7.946 / 9.340 | 17.206 / 18.760 | 223.5 |
| 256 MiB | 8.053 / 10.921 | 16.925 / 19.571 | 287.5 |

All pen-ups miss 8.33 ms. Retaining more staging does not yield a repeatable
improvement and increases reserved memory, so production remains at 64 MiB.
These instrumented results do not identify the remaining completion-wait
residual or justify treating it as allocation time. Query spans, owner CPU
work and host-backed publication remain separate measurements.

Exact probe SHA-256 is recorded in `native-pool-probe-build-sources.json`;
`build-native-pool-probe.py` retains the parent archive and instrumentation,
`run-native-pool-probe.py` records explicit capacity environment overrides,
hardware/power state and serial run boundaries, and
`analyze-native-pool-probe.py` joins the measured allocation/timing records.
All files, per-frame samples, raw logs and reports use `native-pool-probe-*`
under `artifacts/color-m2/final-performance/`. No CPU build or other GPU test
runs during measurement. This experiment changes no production source.

## Shared validation bindings did not improve publication latency

A prototype reused each native encoder's full bind group for a separate
validation entry point, preparing all bounded batches before promotion. All
13 primitive tests and five native-owner tests passed, including zero code
error across 160 cases / 10,485,760 pixels. Test executable SHA-256:
`bafae06244bee63649a590ba6769d3c133a9b623516838edf63b96473c9c4bba`.

The initial three-repeat ProPhoto U16 palette pair improved CPU pen-up p99
8.438 → 7.423 ms and completed pen-up 17.587 → 15.831 ms, but the repeat
changed 8.796 → 8.274 ms and 17.745 → 17.649 ms. A larger nine-repeat pair
(1,278 moves / 36 pen-ups per arm) regressed CPU pen-up 9.276 → 10.606 ms
and completed pen-up 17.287 → 19.329 ms. All 36 pen-ups failed 8.33 ms in
both arms. Smaller glaze, large-paint and sRGB8/P3 U8 comparisons likewise
failed to establish a repeatable benefit. The complete candidate was removed.

Pinned wgpu-core 30.0.1 `command/compute.rs::flush_bindings` merges the resources
of each active bind group into every dispatch's usage scope. Sharing a larger
layout therefore also tracks candidate output resources during validation.
This is a concrete implementation property, not a measured attribution of the
regression. The separate read-only validation layout remains in production.
A follow-up batching prototype was set aside before runtime qualification to
investigate presentation/backing scheduling independently.

`native-shared-validation-*`, `run-native-shared-validation.py` and
`run-native-shared-validation-cases.py` under `final-performance/` retain exact
source changes, deleted-file identities, environment, commands and raw reports.
Release candidate SHA-256:
`3eb6ae5d3066860f06bf831d891083a352270ed647649b66ee65a8c4b102ac0a`.
Every performance arm runs serially at the same staged executable pathname;
no build or GPU correctness test overlaps measurement.

## Presentation/backing separation: reject the extra-copy prototype

At the user's request, investigate taking recovery/undo backing off canvas frame
creation rather than only reducing the cost of the existing pen-up burst. CPU
compression was already asynchronous. Native validation, encoding, promotion
and copies into CPU-readable buffers still preceded canvas composition in the
same command stream.

The prototype retained immutable GPU-only buffers before reusing native scratch.
A backing worker copied each buffer into CPU-readable storage, mapped and
compressed it. GTK held a presentation-priority scope from canvas encoding
through surface presentation submission. The worker checked that scope before
submitting each bounded transfer, with at most one <=16 MiB transfer ahead of a
new frame. Both GPU-only and mapping allocations shared the existing 64 MiB pool;
active transfer bytes were added to diagnostics and admission reserved room for
one transfer inside the 512 MiB pending-storage ceiling. No color/coverage math
or precision changed.

The new ownership test completed three canvas submissions while host transfer
was deliberately held back. It reused all native scratch slots and the status
buffer, then verified exact earlier U8/U16 color/mask snapshots after a later
edit and a later failed publication. All six native-owner tests passed
(60.46 s), including save/undo/reopen/replacement. Test SHA-256:
`c7bd3ee8fd4961fdb7c0245ac465cace5422e192d54fefd4165302ce5e767efc`.
This proves separation and snapshot correctness, not a presentation latency win.

Three-repeat alternating palette ProPhoto U16 pairs initially improved completed
pen-up p99 17.468 → 15.792 ms and 18.676 → 16.577 ms, while completed move p99
regressed 6.798 → 8.500 ms and 7.022 → 8.289 ms. Canvas storage remained 571 MiB;
capture allocated/reserved peak rose from 177 to 209.5 / 207.5 MiB.

A second candidate combined this separation with the nonblocking worker wait
described below. Sixteen capture/native-owner/recovery tests passed (72.22 s),
with one explicit benchmark ignored. Its GTK release test executable compiled;
no native presentation improvement is claimed from that build. Larger serial
nine-repeat comparisons against the wait fix alone contributed 1,278 moves and
36 pen-ups per arm:

| Pair | Before / after pen-up CPU p99 ms | Before / after completed pen-up p99 ms | Before / after completed move p99 ms | Before / after move misses |
| --- | --- | --- | --- | --- |
| Initial | 8.474 / 8.694 | 16.771 / 16.952 | 6.924 / 7.483 | 2 / 9 |
| Repeat | 9.600 / 8.979 | 18.049 / 17.070 | 6.836 / 7.419 | 2 / 9 |

All 36 pen-ups fail 8.33 ms in each arm. Combined capture allocated/reserved
peak reaches 215 MiB. The extra-copy design does not yield a repeatable pen-up
improvement and increases move deadline misses, so **the entire deferred-copy
prototype and its GTK scopes are removed**. Moving work to a thread did not
remove competition on the shared GPU queue. Neither queue competition nor the
resource-lock hypothesis alone is established as the whole remaining delay.
Further separation needs a better snapshot ownership/scheduling strategy.

Raw `native-deferred-capture-*`, `native-deferred-poll-*` and
`native-deferred-poll-nine-*` reports, runners, patches, manifests, hardware/power
samples and test logs retain both implementations. Initial release SHA-256:
`f2e5a66e6af8450c12baae720560fb2a1771c00fc69c346ad23796b9c605f913`.
Combined release SHA-256:
`5e4817976a4bf2fc419ad7def44791e43d0c8d675557f78d3c9574e04624b27c`.
Combined test SHA-256:
`ee127bf0ee387dfa7d02c1c7322cdbc1ce50facf708d21dd72babcee889aa1f5`.
No CPU build or GPU correctness test overlaps the performance runs. The original
benchmark still waits for its exact canvas submission; deferred background GPU
work is a changed boundary in the prototype, not evidence that all backing work
finished or that native presentation met its deadline.

## Release resource locks while awaiting capture mapping

The retained change replaces the backing worker's blocking `Device::poll(Wait)`
with nonblocking polls and sleeps on its mapping notification outside wgpu. It
first checks for an already delivered result, then polls once and waits up to
1 ms on the channel. The existing overall mapping timeout and ticket-failure
handling remain. There is no busy spin and no periodic work after captures drain.

Pinned wgpu-core 30.0.1 holds its snatchable-resource read lock across the GPU
wait in `device/resource.rs::poll_and_return_closures` / `maintain`.
`deferred_resource_destruction`, also reachable during queue submission, acquires
the write lock to retire views/bind groups. Releasing that lock during a worker
wait removes a concrete contention opportunity. It does not prove that the lock
accounts for every measured completion-wait residual. The public
[Device::poll contract](https://docs.rs/wgpu/30.0.1/wgpu/struct.Device.html#method.poll)
and [buffer mapping contract](https://docs.rs/wgpu/30.0.1/wgpu/struct.Buffer.html#method.map_async)
also require explicit progress for headless mapping; simply waiting on the
channel without polling would deadlock an otherwise idle owner.

Fifteen capture/native/recovery tests pass (70.62 s), with one hardware benchmark
ignored. They include device loss, abandoned/invalid captures, exact native
samples, mixed chunks, undo/save/reopen, masks and subsequent painting. Exact
test SHA-256:
`e4a60f7a839d8af55733647adf35a6c9fc971d4d4c639f919cfccfc7f3b48d66`.

| Three-repeat pair | Before / after pen-up CPU p99 ms | Before / after completed pen-up p99 ms | Before / after completed move p99 ms |
| --- | --- | --- | --- |
| Initial | 9.240 / 8.283 | 17.705 / 16.663 | 6.832 / 7.380 |
| Repeat | 9.893 / 9.697 | 18.641 / 18.071 | 7.429 / 7.050 |

Each arm has 426 moves and 12 pen-ups. Pen-up reductions are modest; move
changes do not repeat in the same direction. All pen-ups still fail 8.33 ms;
move misses are 0 / 0 in the initial pair and 1 / 1 in the repeat. Canvas storage
remains 571 MiB and capture allocated/reserved peak remains 177 MiB. These
measurements support a small synchronization fix, not completion of GTK latency
qualification or a claim of measured battery savings.

`run-native-capture-poll.py`, `native-capture-poll-*` reports, environment and
source manifest reproduce this control. Exact release SHA-256:
`2eb19745d4add952226e6b4c2571725767dee5d40657651c1092a172a029879f`.
The same staged executable pathname is used for every arm, with immutable
source binaries retained separately. No build/test overlaps measurement.

## Batch validation and canonical promotion by texture

Validation now binds up to sixteen read-only working textures per dispatch,
selected within the device's sampled-texture limit. Color and scalar rules use
separate groups; every group contributes to the publication-wide status before
any promotion. The last group repeats only read-only padding views, and never
dispatches those padding slots. CPU ownership/layout preflight remains global.

Canonical promotion groups up to four adjacent requests with the same format
and region, bounded by the device's sampled/storage-texture limits. Separate
exact-size layouts cover the final group without writable aliases or dummy
pixel allocations. All requests are preflighted before commands are recorded;
empty and differing regions retain their independent behavior. The shader still
copies Float32 samples exactly and suppresses all writes on invalid status.
Both changes reuse existing scratch and parameter storage.

Six native-owner tests pass (56.36 s) for validation alone, including a new
bad-last-pixel scan across all seventeen color and seventeen scalar texture
slots. With both changes, five promotion tests pass (9.12 s; one separate
benchmark ignored) and six native-owner tests pass (53.38 s). New promotion
coverage checks two/three/four-tile dispatches plus a trailing tile, both working
formats, full/partial rectangles and invalid-status preservation. Existing
save/reopen/undo/device replacement and late failure/abandonment checks pass.
Exact test executables:

- Validation: `ce121e64bc6ebedebcfe97e761c94699392175133c093d6c16de4e31821b4fc5`.
- Combined: `c174d80e3ffb168e38c186ef5810cc02bb16c3ed7398854d12ddee9ff7c23519`.

The benchmark now retains each already collected frame measurement in a sibling
`*.frames.csv`, written after measurement. Records include mode, repetition,
stroke, frame, pen-up flag, both time boundaries and capture reservation. The
initial validation-only nine-repeat comparison had mixed p99 results; the
individual records below expose medians and matching-stroke behavior as well.

Fresh baseline/validation/combined executables use the same CSV instrumentation
and source directory. Six serial nine-repeat arms run baseline → validation →
combined → baseline → combined → validation at one staged executable pathname.
Each arm contributes 1,278 moves and 36 pen-ups. No build or GPU test overlaps.

| Arm | Pen-up CPU p50 / p95 / p99 ms | Completed pen-up p50 / p95 / p99 ms | Completed move p99 ms | Move misses |
| --- | --- | --- | ---: | ---: |
| Baseline | 5.630 / 8.515 / 8.873 | 12.327 / 16.630 / 16.854 | 6.957 | 5 |
| Validation | 5.218 / 7.200 / 10.173 | 10.873 / 14.725 / 18.178 | 7.175 | 2 |
| Combined | 4.622 / 6.632 / 7.800 | 10.478 / 14.089 / 15.908 | 7.193 | 4 |
| Baseline repeat | 5.941 / 8.057 / 10.117 | 12.528 / 16.759 / 18.661 | 6.870 | 3 |
| Combined repeat | 4.689 / 7.297 / 8.444 | 10.498 / 14.762 / 16.201 | 7.154 | 4 |
| Validation repeat | 5.386 / 7.091 / 8.189 | 11.065 / 14.950 / 15.793 | 7.343 | 2 |

Combined completed pen-up medians improve for all four individual strokes in
both rounds. The largest third stroke changes 16.062 → 13.980 ms and
16.245 → 14.350 ms. Combined move p99 increases 0.236 / 0.284 ms, below the
declared `max(5%, 0.2 ms)` investigation threshold for these completed-frame
baselines; CPU move p99 does not increase consistently. Canvas residency stays
571 MiB and capture allocated/reserved peak stays 177 MiB. Every pen-up still
misses 8.33 ms. This is a retained batching improvement, not full qualification.

`build-native-dispatch-controls.py`, `run-native-dispatch-controls.py` and
`analyze-native-dispatch-controls.py` under `final-performance/` retain/reproduce
the parent archive, exact changed-file contents, individual samples, per-stroke
summaries, commands and hardware/power records. Exact release SHA-256:

- Baseline: `3ca5c356483bb64b572959baf5b1f4376d95b65b7a64130c8db793b5a1cc529f`.
- Validation: `fd87ffeb9436f49dca6467a1d38c0d9e539543bbe36803e2f55e5c6a228f189c`.
- Combined: `d7e73cf7fb6e46288bf4929ebd3e458c58e96a84ab9b374d3ad20e8c35c8f51b`.

A first control-build attempt preserved old source mtimes with `copy2`, causing
Cargo to reuse one executable for all variants. Hash verification caught this;
that run was stopped and retained under `native-dispatch-invalid-stale-build/`
with an explicit invalid marker. None of its timings supports the comparison
above. The corrected builder updates source mtimes, records actual compilation,
and both builder and runner require distinct executable hashes.

## Batch native color and scalar encoding

The native encoders now group two adjacent compatible tiles per dispatch,
within sampled/storage texture and buffer limits. Exact-size layouts cover the
one-tile tail without writable aliases. Color transfer, quantization, alpha,
clipping and scalar packed-edge arithmetic are unchanged. Complete preflight
still precedes recording, and partial regions keep their independent settings.

Sixteen native primitive tests pass (43.04 s; four separate benchmarks ignored),
including all 160 numerical cases / 10,485,760 pixels with zero native-code
error. New tests compare grouped slots and a trailing tile with independent
encodes and CPU references across U8/U16, sRGB/ProPhoto, alpha modes, full/odd
partial regions and packed scalar edges. Six native-owner tests pass (58.44 s),
including save/undo/reopen, multi-chunk scratch reuse, abandonment, invalid
inputs and device replacement. Exact test SHA-256:
`18f69f923fe3c91d303374720f945cb7e7a900cfe663f0bbdeaf3a2b4a5b51d6`.

Four serial nine-repeat palette-knife / ProPhoto U16 arms use the same staged
executable pathname, with no build or GPU test overlapping measurements. Each
arm has 1,278 moves and 36 pen-ups; frame records and the analyzer validate counts.

| Arm | Pen-up CPU p50 / p95 / p99 ms | Completed pen-up p50 / p95 / p99 ms | CPU move p99 ms | Completed move p99 ms | Move misses |
| --- | --- | --- | ---: | ---: | ---: |
| Before | 4.428 / 6.547 / 7.734 | 10.302 / 14.110 / 15.743 | 4.058 | 6.592 | 1 |
| After | 4.030 / 6.027 / 6.212 | 9.437 / 12.921 / 13.074 | 4.342 | 6.823 | 2 |
| Before repeat | 4.910 / 6.907 / 7.010 | 10.582 / 14.245 / 14.628 | 4.259 | 6.946 | 3 |
| After repeat | 4.370 / 6.146 / 7.247 | 9.479 / 13.043 / 14.176 | 4.461 | 6.904 | 3 |

Completed pen-up medians improve for every individual stroke in both rounds;
the largest third stroke changes 13.997 → 12.797 ms and 14.125 → 12.934 ms.
Completed pen-up tails also improve in both rounds. CPU move p99 increases
0.284 / 0.202 ms; the initial round crosses the declared investigation threshold
(0.203 ms), while the repeat is just below its 0.213 ms threshold. Completed
move tails remain below 8.33 ms and do not increase consistently. This small CPU
move concern remains open for the full matrix; these measurements alone do not
qualify unchanged frame creation. Pen-up misses decrease from 36 to 29 / 33,
but the absolute latency gate still fails. The batching change is retained for
its repeated completed pen-up improvement and exact numerical behavior.

`run-native-encoding-dispatch.py`, `analyze-native-encoding-dispatch.py` and
`native-encoding-dispatch-*` under `final-performance/` retain the exact source
patch (including the new test file), source hashes, immutable executables,
individual samples, hardware/power records and commands. Exact release SHA-256:

- Before: `d7e73cf7fb6e46288bf4929ebd3e458c58e96a84ab9b374d3ad20e8c35c8f51b`.
- After: `13fe9db473334f44441ec98ecc970c8bfb659a40e5dcc37669a8792889a15c67`.

## Encode into immutable native outputs; defer readback until after presentation

Native publications now own their encoded integer texture/scalar-buffer outputs
and status. Quantization writes those outputs directly; fixed Float32 canonical
scratch and promotion retain the exact working samples. This removes the capture
copy before scratch reuse, without the extra GPU snapshot copy of the earlier
rejected prototype. A bounded worker transfers at most 16 MiB at a time after
GTK's render/present scope releases. Only a transfer already submitted before a
new scope can precede that frame. Mapping waits remain nonblocking wgpu polls.

The outputs and readback buffers share one 64 MiB retention pool, matched by
format/size/usage. Outputs return only after their readback completes. Every
publication has its own 8-byte validation status. Admission reserves a transfer
inside the 512 MiB pending-storage ceiling and retains the 256 MiB individual
publication limit. CPU compression scratch is separately charged. Fixed native
scratch drops by 10 MiB for U16; the benchmark's narrower canvas column excludes
that scratch, which remains included in renderer telemetry.

Eighteen capture/undo/save/recovery tests pass (79.13 s; one separate hardware
benchmark ignored). New tests hold backing while three publications reuse the
working/canonical surfaces, including a later invalid status; earlier exact U16
versions remain independent. Another test starts save and undo while backing is
held, then verifies the saved newer version and restored older version. Existing
abandonment, late invalid color/mask, multi-chunk, device replacement and exact
U8/U16 save/reopen tests pass. Exact test SHA-256:
`55e5e75a7570dd5a571eb7d9539f2b43ddc2f788f662bbd16ab26433cc8fe63e`.

Four serial nine-repeat offscreen arms each contain 1,278 moves and 36 pen-ups.
They use the same staged executable pathname, with no build/test overlapping.

| Arm | Pen-up CPU p50 / p95 / p99 ms | Completed pen-up p50 / p95 / p99 ms | Completed move p99 ms | Move / pen-up misses |
| --- | --- | --- | ---: | --- |
| Before | 4.085 / 6.568 / 6.842 | 9.396 / 13.357 / 13.846 | 6.784 | 3 / 29 |
| After | 4.504 / 6.910 / 8.189 | 7.867 / 12.683 / 13.350 | 7.107 | 3 / 11 |
| Before repeat | 4.228 / 6.119 / 6.518 | 9.389 / 13.374 / 13.653 | 7.038 | 1 / 31 |
| After repeat | 4.553 / 6.649 / 7.441 | 7.989 / 11.422 / 12.586 | 7.168 | 1 / 11 |

All four per-stroke completed medians improve in both rounds. CPU pen-up cost
increases, reflecting the changed ownership/allocation work; the larger strokes
still exceed the absolute gate. CPU move p99 changes 4.341 → 4.540 ms and
4.358 → 4.521 ms; those and completed move changes stay within the declared
relative investigation thresholds. Capture allocated/reserved peak changes
177.0 → 203.5 MiB; canvas residency remains 571 MiB. Final exported PNG bytes are
identical across all four arms (SHA-256
`84984da09047d3aabe6a38a0b5c9622043c6302172307d58c19aa1c957af2722`).

`run-native-direct-capture.py`, `analyze-native-direct-capture.py` and
`native-direct-capture-*` retain raw frames, full source patch, hashes,
hardware/power records and commands. Exact after release SHA-256:
`a5adc15fe72264cd100dff0f9f2ca67959448b0bc25855e1c34b64a987ce396a`.
Before release SHA-256:
`13fe9db473334f44441ec98ecc970c8bfb659a40e5dcc37669a8792889a15c67`.

A GTK benchmark now pairs each actual pen-up event with its publication frame
and child-surface presentation feedback, while observing host-backed completion
separately. It uses 32 paint layers, 4096² ProPhoto U16, a 720 px palette knife,
a warm/undone first contact, and 24 measured 800 ms contacts per arm. Later
contacts start as soon as their predecessor is admitted. Each corrected arm
passes; all 24 pen-up frames have presentation feedback. No build or GPU test
overlaps measurement. The compositor describes a P3 SDR output; this is protocol
and renderer validation, not calibrated physical-monitor qualification.

| GTK arm | Pen-up GPU median ms | Pen-up event-to-present p50 / p95 / p99 ms | Move queued-to-present p99 ms | Observed host backing p95 ms |
| --- | ---: | --- | ---: | ---: |

| before | 1.596 | 10.904 / 12.570 / 13.538 | 6.319 | 22.811 |
| after | 1.269 | 11.147 / 11.849 / 13.386 | 6.315 | 22.901 |
| before-repeat | 1.604 | 10.845 / 12.383 / 13.436 | 6.224 | 23.415 |
| after-repeat | 1.289 | 10.912 / 13.035 / 13.399 | 8.481 | 23.958 |
| before-confirm | 1.570 | 10.687 / 11.538 / 12.037 | 6.330 | 21.905 |
| after-confirm | 1.294 | 10.820 / 11.594 / 12.753 | 6.330 | 22.529 |

GPU pen-up medians decrease in all three pairs. Event-to-present latency does
not improve consistently; it remains outside the declared 8.33 ms goal.
Event-to-queue medians are about 5.4 ms. Current GTK `FrameClock::deadline` reserves
three quarters of a refresh for rendering/composition, and `Workspace::wake`
waits for its timer; this scheduling delay is separate from native readback.
The worker's shorter GPU work therefore does not automatically produce an earlier
compositor presentation. Host-backed observations are sampled at roughly 2 ms,
so they bound completion rather than measuring exact worker service time.

The after-repeat arm contains one long movement frame (ID 1567): 16.459 ms total
worker elapsed, 9.626 ms thread CPU, with 16.287 ms elapsed in composition. GPU
timestamps span 16.373 ms and include queue gaps. The preceding backing had been
observed complete 635 ms earlier; the next pen-up is 153 ms later. This is not a
pen-up/capture frame. Following frame IDs 1569–1672 show a roughly 2.4 ms
presentation-phase shift, producing 106 move latency misses in that arm versus
0–3 elsewhere. A further serial control pair does not repeat the long frame or
phase shift. The exact trigger remains unproven and stays open for full-matrix
qualification; it is not discarded from the evidence. The one-second display
phase sampling in `wayland.rs` is a candidate for the sustained timing effect,
not an established explanation for the original long frame.

`rebuild-native-direct-gtk.py` restores the explicit parent production sources,
then builds before/after with identical benchmark instrumentation and fresh file
mtimes. `run-native-direct-gtk.py` runs the paired controls (optional `-confirm`);
`analyze-native-direct-gtk.py` retains denominators, missed/discarded feedback,
per-stroke timings and raw records. Initial fixture attempts are separately
marked invalid: the first bypassed the ID allocator and the next preflight
caught paint layers below Paper. The corrected fixture allocates IDs normally,
inserts above Paper and validates before opening; its logs have no recovery
checkpoint failure. Invalid fixture timings provide no acceptance evidence.

Exact corrected GTK release test SHA-256:

- Before: `cc0b868ce713dc790a106f01181c51792c00d18cc51a635f7ab0c226dd1a2167`.
- After: `958338ca66932fcfbb94545f001ca8a66bfd30debc120d759b7e6f1440f04dd2`.

The ownership/scheduling change is retained for exact version isolation and
repeatable offscreen pen-up/GPU improvement. It does not qualify the complete
latency goal, unchanged-movement tails, combined photo/window memory pressure,
physical monitor behavior or other platform hosts. GTK pacing and the larger
native publication costs remain separate open work.


A further device-loss test passes (10.69 s): destroy the GPU after the native
canvas submission completes while readback is deliberately held, then verify
all pending tiles fail and the last host-backed checkpoint restores on a new
device. Final test executable SHA-256:
`9a649c0e31e752a7da216a4ff42f31229b664db82e07d817697e21f360def28e`.
The source changes after the measured renderer build are comments/formatting and
this additional test; they do not change its execution path.

GTK diagnostics/recovery passes, as does the separate wide-color recovery test
(5.97 s), using the corrected after executable above. Their deliberate invalid
scissor submissions produce the expected renderer failures and recovery UI. An
initial attempt to run both GTK tests in one process passed the first but could
not initialize GTK on libtest's second thread; the wide-color test was rerun
successfully in its own process. This is a test-runner restriction, not a
renderer recovery failure. Logs are `native-direct-gtk-recovery*` and
`native-direct-gtk-wide-recovery*`; capture tests/builds use
`native-direct-capture-*` and `native-direct-final-*`.

## Quantize validated working pixels in place

Native publication can now encode immutable integer outputs and write their
canonical Float32 working values in the same compute invocation. The complete
publication is still validated first. In-place shaders check the shared failure
flag before any write; every invocation owns one color pixel or one scalar
packed word. The transfer/quantization code is shared with the separate-candidate
encoder. There is no change to alpha, clipping, working precision or the native
storage contract.

The pinned wgpu 30.0.1 [storage access documentation](https://docs.rs/wgpu/latest/wgpu/enum.StorageTextureAccess.html#variant.ReadWrite)
and local `vendor/wgpu-types/src/texture.rs` require
`TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` for read/write storage textures.
The optional feature is requested only when both RGBA32Float and R32Float report
storage read/write support. GTK and the headless native reference opt in; other
host integrations are untouched. A device without the extension retains the
same exact separate-candidate path. On a supporting device, NativeEdit allocates
no canonical scratch and records no promotion pass, removing 20 MiB of fixed
pixel storage. The copy path remains necessary for capability fallback and
private-candidate primitive users; it is not a compatibility renderer.

Twenty primitive tests pass (65.33 s; four separate benchmarks ignored), including
both paths through all 160 color cases / 10,485,760 pixels each with zero
native-code error. Coverage tests include every native code, rounding-neighbor
values, odd packed edges, partial/empty regions and independent dispatch slots.
Ten runtime tests pass (71.10 s): both paths agree for mixed planes, invalid late
color/mask inputs preserve all working pixels, delayed captures preserve earlier
versions, and save/undo/reopen/device replacement/loss continue to pass. Exact
test SHA-256:
`8389626b88a59a187dd1eaf899f8d88d51bde66efe8d7a55027e6c149f79c98d`.

Four serial nine-repeat offscreen palette-knife/ProPhoto U16 arms use one staged
executable pathname. Each arm has 1,278 moves and 36 pen-ups; no build or GPU test
overlaps measurement.

| Arm | Pen-up CPU p50 / p95 / p99 ms | Completed pen-up p50 / p95 / p99 ms | Completed move p99 ms | Move / pen-up misses |
| --- | --- | --- | ---: | --- |
| Before | 4.448 / 6.721 / 8.036 | 7.877 / 11.755 / 13.574 | 7.113 | 1 / 13 |
| After | 3.670 / 5.784 / 8.460 | 6.778 / 10.342 / 13.168 | 7.282 | 0 / 9 |
| Before repeat | 4.590 / 7.297 / 7.667 | 7.968 / 12.294 / 12.871 | 7.170 | 2 / 11 |
| After repeat | 3.886 / 5.743 / 6.089 | 6.987 / 10.188 / 10.711 | 6.961 | 3 / 10 |

Every individual stroke's completed median improves in both rounds. The largest
third stroke changes 11.346 → 9.929 ms and 12.015 → 10.084 ms. CPU pen-up p99 is
mixed in the initial pair, so median gains are not a claim that every tail passed.
Move p99 changes remain inside the declared relative investigation thresholds.
The absolute pen-up gate still fails. Capture allocated/reserved peak stays
203.5 MiB and the narrower canvas metric stays 571 MiB; the removed 20 MiB of
native scratch is accounted separately by renderer telemetry. All four exported
PNG files have identical bytes, SHA-256
`84984da09047d3aabe6a38a0b5c9622043c6302172307d58c19aa1c957af2722`.

`run-native-inplace.py`, `analyze-native-inplace.py`, `native-inplace-source/` and
`native-inplace-*` retain source contents/patch, exact executables, raw frames,
hardware/power records, tests and commands. Exact release SHA-256:

- Before: `a5adc15fe72264cd100dff0f9f2ca67959448b0bc25855e1c34b64a987ce396a`.
- After: `2ad4572050fe2793f7e41c9a18262fed9b0154219538924e351cedc49faf97bf`.


The release GTK portable paint/managed-sampling workflow passes in all four
working spaces (18.89 s) with in-place capability enabled. Exact GTK test SHA-256:
`8920efa768cc574896ff998ada55864c0961686825aa116c3f95ef50f8570f3c`.
`native-inplace-gtk-*` records the build, source manifest and private-compositor
workflow. This is correctness evidence; GTK input-to-present qualification still
requires further scheduling work and is not implied by the offscreen gain.

## Full native drawing matrix after deferred backing and in-place publication

The complete 25-scenario suite was rerun at `5f37e447` in sRGB U8, Display P3 U8
and ProPhoto U16, paired with an experimental eight-tile encoder. Every arm uses
three fresh-device repetitions per scenario: 10,920 measured frames, comprising
10,686 moves and 234 pen-ups. Six serial arms therefore contribute 65,520 frames.
The same staged executable pathname is used throughout, with no overlapping
build or GPU test. The retained implementation remains the two-tile encoder at
`5f37e447`; the larger-batch experiment described below was removed.

| Retained mode | Worst scenario move CPU / completed p99 ms | Worst scenario pen-up CPU / completed p99 ms | Move / pen-up deadline misses | Scenarios failing completed p99 | Process peak RSS MiB |
| --- | --- | --- | --- | ---: | ---: |
| sRGB U8 | 8.454 / 8.833 | 5.390 / 9.588 | 17 / 5 | 3 | 801.59 |
| Display P3 U8 | 7.496 / 7.753 | 5.332 / 9.608 | 15 / 3 | 1 | 803.77 |
| ProPhoto U16 | 8.401 / 8.613 | 5.886 / 10.499 | 22 / 3 | 2 | 841.86 |

These are the largest *per-scenario* p99 values, not pooled suite percentiles.
All modes still fail the absolute latency gate. Palette-knife pen-up fails in
all three; sRGB also has flat-marker movement and watercolor-wash pen-up
failures, and ProPhoto has a soft-airbrush movement failure. Individual long
frames remain in the distributions even when a scenario's p99 passes. In
particular, both implementations show slow first measured watercolor frames
after warm-up/undo; these must be investigated separately from ordinary warm
movement. Three repetitions give only 3–36 pen-ups for an individual scenario;
its p99 can be the maximum and cannot substitute for sustained tail evidence.

Compared with the first native matrix in this report, failing scenario counts
fall from 13 / 12 / 15 to 3 / 1 / 2. That is progress against the initial native
path, not acceptance against the fixed program baseline or the declared GTK
input-to-present budget. The earlier fresh fixed/M1/parent measurements remain
in `comparison-data.json`. This run measures frame creation and offscreen
completion; it does not measure GTK presentation or qualify combined photo,
export, multiple-window and driver allocation peaks.

Artifacts are `run-native-inplace-matrix.py`,
`analyze-native-inplace-matrix.py`, `native-inplace-matrix-{environment,runs,summary}.json`
and the per-arm reports, raw frames, process high-water and GPU samples. The
retained executable SHA-256 is
`2ad4572050fe2793f7e41c9a18262fed9b0154219538924e351cedc49faf97bf`.
All 25 final exported PNGs match byte-for-byte between the two implementations
in every mode (75 comparisons); their hashes are retained in the summary.
These sRGB diagnostic exports supplement the native-code tests, rather than
establishing U16 interchange precision by themselves.

### Rejected eight-tile encoding experiment

The candidate requested only available, bounded binding capacity: at most 16
storage textures and nine storage buffers per shader stage. Supporting devices
encoded up to eight independent tiles per dispatch; other devices were limited
by their advertised capacities. Per-tile extent, pixel storage and quantization
math were unchanged. Color/scalar tests exercised all eight slots plus a
one-tile tail, and the late-invalid whole-publication test passed. The candidate
also passed all 160 color cases with zero native-code error. Exact test SHA-256:
`b6156eae764c963ef3bdf5609958177c8479e1d1b784bdb38faf3dab49448c13`.

Four separate nine-repeat palette-knife arms suggested a useful improvement:
completed pen-up medians changed 6.885 → 6.237 ms and 7.050 → 6.162 ms. Every
individual stroke's median improved. Movement tail changes were mixed, however,
and the complete matrix exposed repeatable CPU regressions in other brushes.
For example, the first large-paintbrush pen-up CPU medians in sRGB and P3 changed
0.820 → 3.263 ms and 0.896 → 2.690 ms. The second chalk pen-up changed
1.139 → 2.858 ms and 1.068 → 3.336 ms. These are repeated, workload-specific
changes, not just the occasional 18–21 ms outlier. ProPhoto repeats the same
patterns. Their exact internal cause has not been established; increased binding
capacity cannot be treated as a free optimization on this driver.

The candidate was rejected and all six changed production/test files restored
exactly to `5f37e447`. No GTK device-limit expansion remains. Its immutable
executable is
`74059e01d395865c51a3c655e3026c7737b260a708e3d06763c1c758b7146a96`;
`native-inplace-eight-source/`, `native-inplace-eight-rejected.patch`,
`native-inplace-eight-restoration.json` and the `native-inplace-eight-*` runs
preserve the experiment. The retained two-tile code and its earlier GTK workflow
validation are unchanged; no new GTK correctness claim is made for the rejected
candidate.

A separate analysis of the first four moves after each palette-knife pen-up did
not identify readback as the dominant movement tail: those groups' p99 values
were below the other moves in each of the four nine-repeat arms. GTK already
holds presentation priority through canvas submission and surface publication.
The core/headless entry point has no equivalent outer scope; adding one without
checking restoration/backing waits could introduce a dependency cycle. No such
speculative scheduling change was made. The outstanding GTK timer delay, larger
native publication cost and first-use CPU stalls remain distinct investigation
targets; the exact save/undo/recovery ownership changes are retained.

## Prompt GTK stroke completion with display-phase restoration

GTK now expedites its existing canvas timer for pen-up and cancellation. It
still coalesces input into one frame source. After that frame, continuing work
returns to the compositor's normal display phase even if the expedited time
was inside the clock's ordinary jitter tolerance. Movement keeps its existing
pacing, and idle/unmapped/failed canvases release the timer handle. The handle
replaces the separate `ticking` boolean; no second timer or per-sample rendering
path was added.

Rearming introduces a real readiness edge case. The [Linux timerfd manual](https://man7.org/linux/man-pages/man2/timerfd_create.2.html)
defines expiration counts relative to the latest rearm/read, and a nonblocking
read can return EAGAIN when no expiration remains. If input rearms after GLib
polls the descriptor, that old readiness must not remove the frame source. The
callback now retains the source on EAGAIN/EINTR. A kernel-timer test first polls
an expired timer, rearms it, checks the cleared readiness, and verifies the next
expiration still works.

The GTK correctness fixture passes (4.00 s) on a P3 U16 drawing. Repeated
completion-wake requests produce one committed stroke; a second stroke visibly
changes the canvas before cancellation restores the exact prior pixels and
raster revision. One undo/redo restores the expected roots, and idle processing
stops. This checks actual GTK input dispatch, worker rendering and readback,
rather than just timer state.

### Measurement correction and full presentation accounting

The pen-up fixture previously used a one-millisecond sleep/poll loop while
waiting for admission. It now waits on `MainContext::iteration(true)`, as it
already did during movement. Both comparison executables were rebuilt with the
same corrected benchmark function. This also changes the phase at which later
contacts begin, so results are compared within these new paired arms, not against
the earlier polling fixture's latency numbers.

Mailbox presentation can replace a pen-up frame with a later frame containing
the same completed stroke. The analysis retains direct presented/discarded
feedback and measures the first successfully presented frame at or after the
commit frame ID. This fixture never undoes a measured contact, so later frames
include it. Every one of the 24 contacts in each arm must have such feedback;
there is no percentile calculated only from the faster directly presented subset.
Movement numbers below remain enqueue-to-present, not physical input latency.

Four serial 24-contact arms use the same staged executable pathname, private
120 Hz Wayland display, 4096² ProPhoto U16 document, 32 paint layers and 720 px
palette knife. No build or GPU test overlaps measurement.

| Arm | Pen-up enqueue delay p50 / p99 ms | First-visible pen-up p50 / p95 / p99 ms | Pen-ups over 8.33 ms / contacts | Move enqueue-to-present p99 ms | Move worker CPU p99 ms |
| --- | --- | --- | --- | ---: | ---: |
| Before | 6.670 / 7.894 | 12.572 / 13.759 / 14.067 | 23 / 24 | 6.353 | 0.986 |
| After | 0.152 / 0.322 | 6.264 / 9.852 / 10.463 | 7 / 24 | 6.357 | 1.017 |
| Before repeat | 6.636 / 7.727 | 12.518 / 13.456 / 13.619 | 22 / 24 | 6.310 | 0.964 |
| After repeat | 0.161 / 0.339 | 6.149 / 9.715 / 9.956 | 6 / 24 | 6.375 | 0.978 |

Before arms contain 2,305 movement worker frames and no discarded presentation
feedback. After arms contain 2,309 / 2,306 movement worker frames, with 2,297 /
2,295 directly presented; 11 / 10 of the 24 original pen-up frames are replaced.
All contacts remain in the first-visible statistics. Directly presented movement
frames over 8.33 ms are 0 / 2 / 0 / 2 in table order. Movement p95/p99 changes
remain inside the declared investigation thresholds. Process peak RSS is
612.66 / 611.70 / 624.67 / 610.23 MiB respectively; these are the test processes,
not a combined multi-window/export memory qualification.

Observed host-backing medians improve from about 22.6 to 16.4 ms. As before,
these are first observations at the fixture's event-loop sampling cadence, not
exact compression service times. The renderer's numerical work is unchanged.
The retained scheduling change removes a measured input-owner wait and improves
pen-up latency, but its roughly 10 ms p99 still fails the currently declared
8.33 ms input-to-present gate. This is not a complete GTK latency qualification.

An earlier candidate allowed an expedited timer to retain a phase error inside
the normal jitter tolerance. Its repeat move p99 changed 6.290 → 6.699 ms, crossing
the relative threshold. The final candidate explicitly restores phase, with the
paired results above. Initial polling-harness and intermediate event-loop runs
remain separately named `native-terminal-wake-*` and `native-terminal-eventloop-*`;
they are not substituted for the final `native-terminal-realign-*` evidence.

Exact final measurement test executables:

- Before: `e87bb33e23bb74545f5baa7b16c46c07ae07afb726b90c6764cfe4c65cdb898f`.
- After: `3dc867ec1f4dfc632519c8e429b76ce9ee8b0ad66ef977bfdaca432a04b4e9c0`.

`rebuild-native-terminal-eventloop.py` records the baseline rebuild with common
instrumentation and fresh source mtimes. `run-native-terminal-realign-gtk.py`,
`analyze-native-terminal-realign-gtk.py`, the source snapshot/patch, build records,
raw presentation JSON, per-arm process/GPU logs and environment records retain
the final comparison. The preceding offscreen matrix remains applicable to the
unchanged shared renderer, including its outstanding large-commit and first-frame
stalls. Photo/global-effect/concurrent-worker and monitor/platform qualification
remain open.

The final integration follow-up passes navigation/color sampling (3.68 s), wide-
color diagnostics/device-failure recovery, and twelve successive window
create/destroy cycles (5.06 s). These are correctness checks, not latency trials;
the recovery check overlapped a test-only CPU rebuild. The lifecycle check now
verifies that each renderer thread is joined and its frame timer released. The
current unrealize path intentionally retains the shared document session for
reattachment, so the former assertion that the whole `GpuCanvas` option vanished
was obsolete.

The navigation fixture also needed to use the existing readiness-aware stroke
helper after selecting its brush. Its original white-sample assertion failed
identically on the unchanged baseline. Once painting was admitted correctly, its
fixed 250 ms idle assumption proved too short in a cold run: a bounded state wait
settled after another 956.9 ms; the subsequent warm run needed no extra wait.
Color sample values were already correct before that idle wait. The corrected
fixture keeps the actual idle assertion and failure-state diagnostics, without
claiming that functional timeout as a color-sampling latency budget. Initial
assertion-failing runs also ended with exit 139 during teardown; their logs are
retained, including the matching baseline failure. Corrected runs close normally.

Only test fixtures/inspection changed after the final measured production code.
Recovery executable SHA-256:
`9f6a2b8cbca5d3d9a07533aad8d6322914482a0aa0abad6e029828fb6f5c4d42`.
Final navigation/lifecycle executable SHA-256:
`e79e0750fecff48d909cd4c917b043d5e9a40f7461f63beec009e9c0e9615ac0`.
Their exact build/source manifests and logs use `native-terminal-final-*`,
`native-terminal-idle-*` and `native-terminal-integrated-*` prefixes.
