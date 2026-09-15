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
