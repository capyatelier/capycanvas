# GTK SDR milestone 2 — final performance qualification

Status: **in progress; no acceptance claimed**. This final phase follows the
correctness checkpoint `125108bf`. See the [implementation evidence](color-management-gtk-m2-validation.md)
and [common gates](color-management-milestones.md#gates-that-apply-to-every-milestone).

## Current scope after user clarification

Finish GTK SDR color correctness and workflows first. The remaining interactive
performance requirement is **120 Hz pan, zoom and rotation of unchanged large
photographs**, including transitions that miss the current display detail cache.
Keep frame creation, completed work and native GTK presentation measurements
distinct. Existing warm navigation results alone do not qualify this requirement.
The user's latest clarification is **smooth 120 Hz navigation, with measured
input-to-presentation latency documented**. The historical strict p99 ≤ 8.33 ms
input-to-present threshold is no longer a completion gate. Real interaction
stalls still matter; the clarification does not excuse missed presentations.

The user explicitly deferred optimization of dirty-pixel/image regeneration and
the proposed resolution-aware preview pipeline. Adjustment/filter rebuilds,
full-resolution histogram regeneration and stroke regeneration timing remain
recorded below, but their previously declared latency breaches no longer block
this phase. This is a scope change, not a claim that those gates passed. Preserve
correctness, precision, bounded memory, saving, diagnostics, GPU recovery and
responsive/cancellable long operations. Other host integration still requires
approval after GTK validation.

Navigation reuses completed composition through bounded retained Float32 display
mips and native detail. The checkpoints below remove repeated zoomed-out scene
evaluation, seed native detail during existing composition and skip redundant
mip work on camera-only detail misses. The measured offscreen pointwise-adjustment
workload fits 8.333 ms at all three photo sizes. Subsequent 4K/61 MP native runs
also keep camera CPU/GPU work below 2.3 ms, including a full-resolution Gaussian
filter's completed output. The last checkpoint documents native latency and the
remaining first-interaction gap. Filters always execute at their original
resolution before display reduction. Repeated optimization of deferred
regeneration workloads is not the next step.

The budgets below are the original declaration and historical comparison basis.
Their regeneration latency thresholds remain future targets; the current
navigation requirement above supersedes the former 100/200 ms cold-view allowance.
The user's later 4K memory-policy direction also supersedes fixed byte ceilings:
use available memory when it is needed for 120 Hz interaction, preserve precision,
and let less important content yield memory first. Do not allocate the entire
admission allowance or spend memory that brings no interactive benefit. See the
4K gesture/memory-policy checkpoint at the end of this document.

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

## Expanded photo and concurrent-worker baseline

The earlier dense-photo run qualified only its measured drawing/save subset.
The expanded `raster_workloads --photo` workload uses the production snapshot
renderer for profiled ProPhoto U16 PNG output and full-resolution inspection.
It adds five retained corrections, an actual native scalar mask crossing tile
boundaries, 64 exposure previews, repeated fitted-to-4× pan/zoom, 16 Gaussian
radius changes, archive reopen and exact paint/mask undo restoration. A barrier
starts save and export together; CSVs record operation/frame overlap. These
initial end timestamps preceded renderer destruction; the corrected worker
boundary and cleanup measurements are recorded below. Editing
is paced at 120 Hz during worker overlap, with an explicit GPU completion wait.
This is offscreen work latency, not GTK input-to-present evidence.

The same captured executable, SHA-256
`bafcdc483bdbbce245f521790f67d6fd11388f62a7d1498fb8ac6caa47518390`, ran
serially at `/tmp/capy-m2-photo-expanded-path-control/raster-workloads`, with no
builds or other validation overlapping. Production renderer code is `339129bc`
plus opt-in snapshot allocation observations; the new example is retained in
`photo-expanded-before-source/`. Commands, hashes, environment, GPU/process
samples and logs are in `photo-expanded-before-*`, `photo-expanded-before-large-*`
and `photo-expanded-summary.json` under `final-performance/`. The runner's first
large-case invocation failed before launching because its executable alias had
not yet been copied; no failed measurement is included below.

| Case | Slider completed p95 / p99 ms | Warm navigation p99 ms | Cold navigation p95 / p99 ms | Three histogram times ms | Process RSS high-water MiB | Sum of GPU reservation peaks MiB |
| --- | --- | --- | --- | --- | --- | --- |
| 24 MP | 375.5 / 380.4 | 2.043 | 231.0 / 310.2 | 4749, 4428, 4782 | 940.7 | 896 |
| 45 MP | 708.2 / 711.5 | 0.638 | 415.7 / 701.3 | 7325, 9322, 8276 | 1200.3 | 1152 |
| 60 MP | 948.1 / 960.4 | 0.668 | 598.8 / 780.5 | 13295, 12815, 11797 | 1231.6 | 1152 |
| Three documents, active 60 MP | 950.7 / 955.5 | 0.558 | 601.3 / 937.4 | 12623, 12072, 12418 | 2222.9 | 2432 |

Source-cache misses define cold frames; repeatedly changing mip levels can
require another fill. There are 64 slider samples in each reported case, and
146/144/180/180 cold navigation samples versus 366/368/396/396 warm samples.
The histogram column reports three individual operations, not a qualified p99.
Gaussian preview p99 is 455/1012/1596/1554 ms (16 samples each). Slider, cold
navigation, histogram and blur clearly fail their declared response budgets.
Current code rebuilds the reduced display composite from full-resolution tiles
on adjustment changes; the bounded 16-slot decoded-source cache repeatedly
refills. This is substantial work to address, not a claim that ordinary brush
computation became that slow.

Warm painting during worker overlap has completed p99 2.727/2.614/2.867/2.691 ms
(302/263/283/283 frames). Source-missing paint p99 is
6.891/3.748/5.810/8.615 ms (82/121/165/165 frames). Individual startup stalls are
still visible: warm paint maxima 103.9/76.9/82.3/90.7 ms, with CPU time near
1 ms on the stalled thread. These establish waiting/contending work, without
yet establishing its precise cause. Snapshot construction currently warms
unused brush pipelines; isolating that work is a follow-up. Maxima and deadline
misses remain reported even where p99 passes.

Save durations are 64.6/111.8/159.1/170.7 ms; PNG export takes
8.61/13.42/18.65/18.04 seconds, including setup and sync but excluding renderer
destruction. Only 6/6/9/9 paint
frames actually overlap save; 384/384/448/448 overlap export. Navigation overlap
is separately recorded; these initial overlap counts exclude cleanup. Do not
describe the whole editing interval as save contention. All archives retain
source/profile, editable effect values and exact
native paint/mask digests; undo/redo restores the measured native roots.

The memory observations fit the declared envelopes for these operations. GPU
figures sum each live renderer's observed allocator peak and the larger of the
export/histogram worker peaks (those two workers run serially). They include
allocator reservations and staging, exclude driver-private memory, and are not
a measurement of one simultaneous instant or a proof of every transient maximum.
Process RSS high-water covers all workers and archive reopening. Whole-image
transforms, dense active edit pins, dense masks, global effects and codec pressure
remain outside this particular matrix; this is not overall memory acceptance.

## Bounded tile-row capture

The production snapshot consumer previously read 16 output rows at a time,
although `Scene::capture_region` composes complete 256-row tiles. A full-width
photo exceeding the 16 decoded-source slots repeatedly decoded and recomposed
those tiles for each small strip. Histogram and profiled output now share a
bounded band reader: up to 256 rows, at most 32 MiB of CPU Float32 pixels. The
existing dependency planner still charges GPU output, mapping and dependencies.
A typed planning-budget failure halves the request down to the former 16-row
size before any GPU work starts. Unsupported dependencies still fail explicitly;
no pixel, filter, profile or precision contract changes. Cancellation remains
checked during capture and between output rows.

A focused regression test compares exact Float32 pixel values with 16-row captures
through native masks, translated groups, watercolor and neighborhood sampling.
It also checks histogram equality, exact budget fallback, cancellation and
opt-in allocator observations. It passes (3.63 s). All 12 affected snapshot tests
pass (29.57 s), covering all eight space/depth identity modes, hidden RGB,
PNG/TIFF/JPEG, matte, dither, resampling, output previews and master preservation.
Exact GPU test executable SHA-256:
`2b7e2eba813589e7fe1760aa9bcddb24dd6293a11fe3e3b63f305f8784433206`.

The fixed-snapshot `--photo-capture` comparison omits variable-duration editing,
so both arms export and inspect the same committed pixels. The same executable
pathname, power policy and serialized measurement rules apply. This comparison
is a diagnosis of new photo consumers, not a replacement for the fixed drawing
baseline or native presentation gate.

| Case | PNG through sync, before / after seconds | Histogram before milliseconds | Histogram after milliseconds | RSS high-water before / after MiB | Aggregate GPU reservation bound before / after MiB |
| --- | --- | --- | --- | --- | --- |
| 24 MP | 7.507 / 3.855 | 6183, 5187, 4815 | 1406, 1231, 1285 | 714.5 / 728.1 | 640 / 640 |
| 60 MP | 19.963 / 7.820 | 14789, 15232, 14095 | 3168, 3147, 3164 | 1051.8 / 1042.0 | 896 / 896 |

Each export drops from 250 to 16 captures at 24 MP, and 458 to 29 at 60 MP.
Worker live allocation peaks increase from 71.4 to 115.3 MiB and from 89.6 to
149.6 MiB; worker allocator reservations remain 256 MiB. The explicit buffer
increase stays within the declared process/GPU envelopes for these cases.
Decoded PNG sample CRCs agree (`059569d1`, `1dbe9195`); every histogram repetition
matches the before-arm bin/endpoint CRC (`217b99ae`, `9cd46a99`). No identity
shortcut is eligible for these layered, masked, adjusted images.

These first paired PNG times end before GPU renderer destruction. Histogram
times likewise describe the operation on a retained worker, with setup logged
separately. The concurrency follow-up below corrects export's full worker
boundary; the stable-snapshot comparison still isolates the repeated capture
work and verifies unchanged samples.

Exact release executables:

- Before: `1bb5193859b128dbf777daf2a83da23421c43dc5facebde312cdc8c6a5f50438`.
- After: `6bef1f7da9ed7cce9fb930ff2fcde2e06d07c7bb0b0e86aeb52b86deb512f291`.

`build-photo-bands.py`, `photo-expanded-bands-builds.json`, both source snapshots,
`run-photo-expanded.py`, per-arm run/environment records, raw logs and the
analysis script retain the complete reproduction under `final-performance/`.
The final example also fixes `all` to run all four cases and writes separate
case CSVs; those dispatch/report-name changes do not alter the measured explicit
24/60 MP cases. Production snapshot code matches the captured after arm.

GTK release compilation succeeds. Native histogram refresh/document preservation
and profiled export sizing/cancel/dialog-release journeys pass separately on the
private Wayland display (8.46 s and 13.18 s). Exact GTK test executable SHA-256:
`fc4f5ef4cec1274f57f1eb7734e6d8cf3485e58f3e6cea9dc11faa6a1dabc7d9`.
Logs and reports are `photo-bands-*` under `artifacts/color-m2/` and
`final-performance/`. This is an intermediate improvement: histogram latency
still exceeds 500/1000 ms, and full-resolution adjustment rebuilds, cold view
fills, worker-startup stalls, the existing stroke tail and the remaining stress
matrix are open. No GTK milestone acceptance or other-host approval is claimed.

### Concurrent editing and corrected worker cleanup boundary

A full 24 MP after run (`photo-expanded-bands-after-full-24mp`, same after
executable) exposed a 31.6 ms paint stall just beyond export's recorded end.
The harness had timestamped the returned `Job` before Rust destroyed its local
GPU renderer. It now explicitly drops the renderer and output file before the
worker end timestamp, and records `export-cleanup` separately. The previous
through-sync durations remain retained and labeled; they are not full worker
lifetime measurements.

Both production arms were rebuilt with that same corrected harness and run
serially at the same staged pathname. Exact executable hashes:

- 16-row capture: `da55bd2e4ec2876d3abbda53b9c91675558bc0cd6add5477ceccee7f4c7eecd8`.
- Bounded bands: `92c34e81afce0d1baf195a2434f53c1f7cf567b0487932175deeaba99407002d`.

`photo-expanded-bands2-*`, `build-photo-bands-cleanup.py`, and
`photo-expanded-cleanup-analysis.json` retain the binaries, sources, measurements
and timestamp analysis. GPU cleanup takes **41.896 ms before and 39.619 ms after**.
Before, that interval overlaps an already slow 264.36 ms cold navigation frame.
After, it overlaps a **40.241 ms paint frame** (39.753 ms call-return time,
5.303 ms thread CPU). The paint call starts 0.148 ms after cleanup starts.
This strongly locates the remaining wait at worker retirement; it does not yet
identify the specific driver/wgpu lock. Both arms also retain two startup paint
stalls near the same event positions.

Comparing the identical first 192 paint events, completed p50/p95 changes from
0.850/3.141 to 0.865/3.164 ms; p99 is 26.722 → 40.241 ms, with two → three
8.33 ms misses. Maxima are 88.169 → 73.189 ms. This **does not pass** the frame
latency gate. A shorter export moves its unchanged cleanup cost into a different
interaction phase; neither the small steady-frame times nor the lower maximum
excuses the missed frames. Worker lifecycle contention must be addressed before
acceptance. The new band reader remains useful: it removes repeated capture
work with matching samples and measured bounded memory, while exposing this
separate remaining lifecycle cost.

The variable-length editing loop leaves different painted tile populations when
export finishes. Its subsequent histogram/blur/undo times must not be used as an
unchanged-work rendering regression comparison. Use the fixed-snapshot capture
arms for that comparison and the recorded matched input prefix for foreground
work. Native GTK histogram/export correctness checks above remain applicable;
no live renderer, brush math, saving, history, diagnostics or recovery code was
changed by the band reader.

### Shared capture device removes the measured lifecycle stalls

Following `de3e6311`, splitting export into explicit setup, capture/output and
cleanup intervals located large foreground waits during both device creation and
destruction. Removing eager brush/mask/transform warmup alone did not fix them:
24 MP setup fell from 356.406 to 289.207 ms, but cleanup stayed 34.731/33.746 ms
and the worst paint frame rose from 132.040 to 197.226 ms. Keep this failed
hypothesis in the evidence; lazy construction alone is not the claimed fix.

Snapshot workers now clone the canvas's adapter/device/queue and optional pipeline
cache. They build private renderers, source windows and readback buffers from the
immutable project. Their initialization skips unrelated brush warmup. GPU waits
poll briefly and then sleep on completion notifications outside wgpu's resource
lock, including bounded scene-upload chunks. No live paint buffers or mutable
scene state are shared, and no second device is created or destroyed per job.
The current wgpu implementation and the earlier readback contention experiment
motivate the nonblocking wait; the lifecycle timings do not identify a specific
internal driver lock.

GTK export, output/color/source/rasterization previews, flattened conversion and
histogram use this path. The render owner supplies the handle after initialization;
stop/failure retires it, and restart publishes the replacement device. Existing
jobs retain their device ownership until completion/cancellation, including if a
canvas closes. Jobs on a physically lost device can still fail; the existing
checked file publication and recovery checkpoint rules remain in force.

Two serialized full 24 MP comparisons use the same staged executable pathname,
corrected complete worker lifetime and identical first 128 paint events. The
larger dynamic editing loop can complete different numbers of later events, so
those differing post-edit histogram/blur populations are not a controlled
rendering regression comparison.

| Observation | Before run 1 / run 2 | Shared device run 1 / run 2 |
| --- | --- | --- |
| Export setup, ms | 356.406 / 468.344 | 8.261 / 8.407 |
| Export cleanup, ms | 34.731 / 38.558 | 0.810 / 0.717 |
| First 128 paint completed p95, ms | 3.272 / 2.668 | 3.173 / 2.572 |
| First 128 paint completed p99, ms | 44.473 / 86.492 | 4.169 / 3.212 |
| First 128 paint maximum, ms | 132.040 / 179.160 | 4.239 / 3.260 |
| First 128 paint misses over 8.33 ms | 2 / 2 | 0 / 0 |
| Export complete worker, ms | 3584 / 3563 | 3215 / 2546 |
| Process RSS high-water, MiB | 802.9 / 790.1 | 749.0 / 764.3 |
| Conservative GPU reservation bound, MiB | 896 / 896 | 640 / 640 |

On a shared device, capture allocator observations already include live canvas
allocations. The harness takes the maximum of that device's observations and adds
peaks for other canvas devices; summing canvas and capture reports would count
those same reservations twice. Driver-private memory remains reported separately.
This does not make per-request capture planning an aggregate admission policy.

Exact release executable SHA-256:

- Before (`de3e6311` production with the setup/cleanup harness):
  `b9a4690ec577125e3f8907cf825fcb24c4eafbfd8dd6719ffd2f73541db83491`.
- Lazy-only experiment:
  `c965a289168ff9790f0eeea6d0e34301e88d1296b800997e7516216f5921c809`.
- Shared-device candidate:
  `2297154cb4fdbc124215e94a5d18ea6e0741002662e1ff26cd6bb75f9745e290`.

Sources, patches, build logs, executable copies and per-run environment/raw data
are `snapshot-lazy-*`, `photo-expanded-lazy-*`, `snapshot-shared-*`,
`photo-expanded-shared-*` under `artifacts/color-m2/final-performance/`.
`run-photo-expanded.py` stages each executable at the same pathname;
`analyze-photo-shared.py` records the matched prefix and exact setup/cleanup
intersections. Repeats retain distinct output names and the same executable hashes.

All 13 snapshot GPU tests pass (23.54 s), including a new concurrent live-edit
comparison in sRGB8 and ProPhoto16, private capture pixels, completion after canvas
closure, and cancellation. The existing profile/sample/hidden-RGB, mask/effect,
resampling/dither and bounded-band tests remain passing. Exact GPU test executable:
`f323b4b2ce68d8cfe3439d20a2e7c527cad95e2770cc69ccdb0ba34e9a20dc00`.
The lazy initialization refactor separately passed all eight startup tests,
including eager/cached parity and compiler shutdown. That test executable is
`8a516c7c01c224a575c63f0b0ddbd3036379d7bde216fdfc62e0dfcd7434c247`.

The fixed-snapshot 24/60 MP arms also ran twice, serialized without builds or other
GPU tests. Exact PNG sample CRCs remain `059569d1` / `1dbe9195`; all histogram
repetitions retain `217b99ae` / `9cd46a99`. Native/source/profile/archive checks
pass in every run. The aliases are `photo-expanded-shared-capture-{before,after}`
and their `-repeat` counterparts, using the same executable hashes above.

| Fixed snapshot | Before run 1 / run 2 | Shared device run 1 / run 2 |
| --- | --- | --- |
| 24 MP export complete worker, ms | 3030 / 3890 | 3333 / 2590 |
| 60 MP export complete worker, ms | 8154 / 7979 | 7487 / 6914 |
| 24 MP RSS high-water, MiB | 733.8 / 738.6 | 688.4 / 688.5 |
| 60 MP RSS high-water, MiB | 1046.5 / 1043.6 | 1007.2 / 1008.1 |
| 24 MP conservative GPU reservation bound, MiB | 640 / 640 | 640 / 640 |
| 60 MP conservative GPU reservation bound, MiB | 896 / 896 | 640 / 640 |

The initial 24 MP export slowdown triggered the second pair; it did not repeat,
and both arms show substantial full-operation variability. Do not claim a stable
24 MP throughput gain from these few observations. Setup/cleanup reduction and
foreground latency are the repeatable result. Histogram observations across both
runs span 1214–1717 / 1222–1241 ms at 24 MP and 3194–4583 / 3223–4455 ms at 60 MP
(before / after). Sharing the device does not fix histogram's processing cost.

GTK release tests run one per process on the private 120 Hz Mutter display.
Histogram refresh and document preservation pass (7.48 s), export output sizing,
profile/depth/metadata and repeated cancellation/dialog release pass (10.01 s),
and P3-U8/ProPhoto-U16 GPU failure/restart with diagnostics, exact surviving pixels
and undo/redo pass (3.39 s). The recovery test now also verifies rejection of new
snapshot jobs on the failed owner and exact full-composite capture through the
replacement device. Its deliberately invalid render command is the recorded
failure injection, not an unexpected test failure. Exact GTK test executable:
`1b6a73a58d41f68e9606b6e0f7bae2998a5b33b581d975a5c516960a8f6971e4`.

The same GTK executable also passes complete assignment/conversion/depth history
and flattened-copy delivery (13.72 s), retained-source profile repair with baked
edits preserved (6.94 s), and off-canvas source rasterization with paint/mask/save
reopen preservation (4.97 s). These exercise previews whose document primaries
differ from the shared live canvas, without changing the displayed original.

The measured lifecycle improvement does not close the milestone. Native pen-up
presentation p99, cold large-photo navigation, full-resolution slider/blur rebuilds
and histogram latency still exceed their declared gates. Large transforms, dense
edit/scalar residency, overlapping worker admission, codec limits and the remaining
precision/display matrix still need qualification. Other platform host integration
remains unapproved; no GTK acceptance or broader platform qualification is claimed.

### Exact histogram classification without per-sample transfer evaluation

Following `33ba4165`, an isolated histogram probe separated full-resolution band
capture from CPU counting. At 24 MP, capture took 312–392 ms and counting took
910–1098 ms; at 60 MP, capture took 911–1055 ms and counting took 2254–2744 ms.
This identified CPU counting as the larger cost before changing it. The probe
uses the same immutable adjusted/masked photograph and shared-device capture as
the production fixture, with two timing reads per band. It does not substitute
display mips or sampled pixels for the full-resolution inspection.

The counter now caches the 255 decision boundaries of each distinct transfer
curve. A small Float32 exponent/mantissa table supplies a candidate bin; the
original unassociated Float64 value and exact boundaries decide the final bin.
Boundaries are found using the forward transfer function so independent rounding
of encode/decode cannot silently move an edge. sRGB/P3 share one table; Adobe RGB
and ProPhoto each have one. All three tables together occupy under 54 KiB, with
no image-sized retained cache. Opaque samples avoid division by one. Linear Y,
zero/partial alpha, out-of-range and endpoint counts retain their previous meaning.

The tests compare the new classifier with the original direct transfer evaluation
at every U16 code, neighboring Float32/Float64 bin edges, every lookup-cell extent,
and extended values. The cell-extent test establishes that one exact correction
in either direction suffices for every supported transfer curve. A separate
full-histogram oracle checks all channels and endpoints on 917,504 pixels across
all four spaces, including partial and subnormal alpha. The existing native U8/U16,
strip partition, transparency and luminance checks remain passing. Five focused
core tests pass (0.10 s).

The final algorithm's diagnostic comparison is:

| Case | Original CPU counting, ms | New CPU counting, ms | Original full histogram, ms | New full histogram, ms |
| --- | --- | --- | --- | --- |
| 24 MP, three observations | 1098, 910, 990 | 290, 242, 198 | 1489, 1222, 1319 | 735, 608, 508 |
| 60 MP, three observations | 2292, 2744, 2254 | 520, 711, 709 | 3227, 3826, 3189 | 1468, 1913, 1914 |

These are individual observations, not a well-sampled p95/p99. CPU counting is
roughly three to four times faster; capture variability and the remaining capture
cost prevent claiming the complete histogram gate. Exact PNG and histogram
checksums match the fixed-snapshot values in the preceding section, including
all bins and endpoint counts. Peak RSS stays around 692 MiB / 1007 MiB and the
conservative shared-device GPU reservation bound stays 640 MiB for both sizes.

Diagnostic executables and source captures under `final-performance/`:

- `photo-expanded-histogram-profile` (original counter):
  `ae09cad84bc0a5a99fa2b1e13e71757a57b9d474d6390c63ced55dbf121938be`.
- `photo-expanded-histogram-bins2-profile` (retained counter):
  `5ab577d9887a1e65250a56121c2f483c5209c5de1745e8c8e85d2522b8c87fd2`.

The intermediate loop-based correction remains in the artifact record as
`histogram-bins-profile`; it was slower than the bounded correction above.
All probe instrumentation is excluded from production. The build scripts retain
source bytes/hashes and restore production files with fresh modification times;
measurements use the same staged executable pathname and serialized runs.

A subsequent source probe measures **220–221 ms of decompression at 24 MP** and
**546–777 ms at 60 MP**, repeated for every histogram. Total CPU source-upload
encoding including decompression is 240–241 / 594–828 ms. These counters establish
repeated decompression as the next substantial capture cost. They do not prove a
cache budget or cache performance; that change still needs its own ownership,
peak/steady memory, cancellation and before/after evidence.

A fresh production before/after pair, without diagnostic timers, confirms the
end-to-end improvement:

| Case | Original histogram, ms | New histogram, ms | RSS high-water before / after, MiB | GPU reservation bound before / after, MiB |
| --- | --- | --- | --- | --- |
| 24 MP | 1750, 1715, 1519 | 723, 700, 695 | 691.3 / 688.6 | 640 / 640 |
| 60 MP | 3235, 3335, 3325 | 1638, 1946, 1755 | 1006.7 / 1005.4 | 640 / 640 |

All fixed-snapshot PNG sample and histogram bin/endpoint CRCs agree exactly with
the original counter. These observations still exceed the declared histogram
budget; neither the gain nor the unchanged memory bound passes that gate.
Baseline and candidate were built from the same `33ba4165` worktree with fresh
source modification times, then run serially through `run-photo-expanded.py` at
the same staged pathname. `build-histogram-final.py` records and captures the
production builds, source bytes, hashes and tests. Exact executable SHA-256:

- Original production fixture: `8b36d3243b12b296abaf76777ee6edba22c34c6d286c00070d35ece5a4c825ab`.
- New production fixture: `7a8e02961f66cbe2ee9bd01df3e4c7318d5ba9555445fa892f0fff6e6e032187`.
- Core tests: `d635c3af9ead49c308b2eb74d1ba34071d348d05741f5d7210fa10b04469cb15`.
- GPU tests: `675557aaf8d2738462971231ade5e8d29135382d0090b5a85f795c9856a940ce`.
- GTK tests: `287f4178b5cb6ddd7746b520618d108003d61bfe53929ac4c58047023a62e029`.

All 73 core tests pass (0.24 s). The hardware GPU masked/native/effect histogram
comparison, band budget fallback and cancellation test passes (1.16 s). The real
GTK histogram refresh, channel/clipping interpretation, document preservation,
pause/cancel and reopen journey passes on the private Mutter display. Build and
run records are `histogram-bins-final-*`; hardware data and exact outputs are
`photo-expanded-histogram-bins-{before,after}-*`. The remaining milestone gates
listed above remain open; this change touches CPU histogram classification only.

### Bounded exact source samples shared by canvas and workers

Following `23cc9f2a`, native source uploads reuse decoded integer samples in a
512 MiB CPU cache. The immutable compressed tile remains authoritative. Entries
use weak allocation identities, preserve exact bytes and integrity checks, and
evict expired sources before least-recently-used live entries. Decompression runs
outside the cache mutex. Concurrent misses can decode a tile twice rather than
blocking canvas work on a worker's decompression.

The canvas, snapshot workers and color-conversion candidate share one sample
cache. They retain private mutable render buffers and ICC transforms. Adoption
updates GTK's snapshot context; device loss and closure retain the existing
ownership rules. The limit charges retained sample allocation capacity, not map
metadata, decode scratch or temporarily retained evicted samples. Whole-process
measurements therefore remain necessary. No display approximation was introduced.

Fresh production builds from the same `23cc9f2a` worktree, with fresh source
modification times, ran serially at the same staged executable pathname. The
full photo fixture includes five adjustments, a native mask, slider changes,
navigation, paint during save/export, exact native history, histograms and blur.
These are offscreen frame/work durations, not native GTK presentation latency.

| Full photo fixture | Before | Cached samples |
| --- | --- | --- |
| 24 MP slider p95 / p99, ms (64 frames) | 369.95 / 370.77 | 67.21 / 68.52 |
| 60 MP slider p95 / p99, ms (64 frames) | 925.23 / 938.55 | 160.24 / 164.52 |
| 24 MP matched first 128 paint frames p99 / max, ms | 3.43 / 4.01 | 1.72 / 2.01 |
| 60 MP matched first 128 paint frames p99 / max, ms | 4.27 / 4.36 | 2.08 / 3.35 |
| 24 MP cold-navigation p95 / p99, ms | 220.51 / 308.05 | 52.31 / 60.64 |
| 60 MP cold-navigation p95 / p99, ms | 756.30 / 924.85 | 132.40 / 149.85 |
| 24 MP RSS high-water, MiB | 816.74 | 1004.54 |
| 60 MP RSS high-water, MiB | 1170.09 | 1689.16 |
| 24 MP conservative GPU reservation bound, MiB | 640 | 640 |
| 60 MP conservative GPU reservation bound, MiB | 640 | 896 |

Both matched paint prefixes have zero 8.33 ms misses. The full fixture continues
painting/navigation until export ends, so later workloads differ: 24 MP paint
counts are 128/192 and 60 MP counts are 192/320. Cold-navigation populations and
post-edit histogram pixels are consequently not identical across arms. The
fixed slider phase and matched paint prefix support their direct comparisons;
other whole-run observations describe the executed workloads. Warm navigation
p99 is 0.28–0.37 ms across these runs, but cold views still fail the current
8.33 ms navigation target. Rotation remains to be added to photo qualification.

A separate fixed-snapshot pair removes the variable paint tail. The 24/60 MP PNG
sample CRCs remain `059569d1` / `1dbe9195`; every histogram repetition remains
`217b99ae` / `9cd46a99` in both arms. Archive source/profile/sample checks and exact
undo/redo roots also pass in the full fixtures.

| Fixed snapshot | Before | Cached samples |
| --- | --- | --- |
| 24 MP histogram, three observations, ms | 530, 513, 511 | 407, 386, 374 |
| 60 MP histogram, three observations, ms | 1602, 1933, 1646 | 904, 886, 990 |
| 24 MP RSS high-water, MiB | 691.75 | 899.25 |
| 60 MP RSS high-water, MiB | 1014.34 | 1490.85 |
| 24/60 MP GPU reservation bound, MiB | 640 / 640 | 640 / 640 |

The fixed workloads show no GPU reservation increase from the CPU sample cache.
At the end of the full fixture, retained cache samples occupy 265.5 MiB at 24 MP
and the complete 512 MiB limit at 60 MP, with 233 evictions at 60 MP. These single
pairs establish useful improvement and bounded retention, not full 45 MP,
multiple-document, dense-edit or constrained-device qualification. Regeneration
latency optimization is now deferred under the user scope above.

Three cache unit tests pass, covering exact U16 samples, corrupt tiles,
concurrent readers, eviction and source lifetime. Hardware GPU source tests pass
(three tests; the separate source benchmark remains ignored), and all 13 snapshot
tests pass, including shared cache ownership with private pixels during live
edits, cancellation and capture after canvas closure. Four native GTK tests pass,
one per private compositor process: histogram refresh (7.33 s), document color
assignment/conversion/depth history (16.84 s), resized export and cancellation
(7.09 s), and GPU failure/restart with surviving samples and history (3.44 s).
Test durations are correctness evidence, not performance measurements.

Reproduction artifacts are `build-source-sample-cache.py`,
`source-sample-cache-builds.json`, the captured source trees/patches and
`photo-expanded-source-cache-{before,after}*` under `final-performance/`.
`run-photo-expanded.py` records environment, raw frames/jobs, RSS and GPU data;
`analyze-photo-expanded.py` summarizes them. The `-capture` aliases use
`LAYER_PHOTO_CAPTURE_ONLY=1`; `-large` aliases preserve the independent 60 MP
full-workflow run. `source-sample-cache-matched-paint.json` records the first
128 paint frames. Exact executable SHA-256:

- Baseline: `56e20d988e33254cd01022fa2fb4c1e3c0d3ef96394d8e42860fb65efe0aac57`.
- Cached samples: `e3eb14a42cfe3b3f761348ebf5315d2e9d16472952ebfec1a49905edf6337e0e`.
- GPU tests: `ec06f6d65b7da77dff55f9b97d6f2e6269cba9de636b35624c19d7a92b0f6e92`.
- GTK tests: `1505a97bae71b713753d4294cbd307d5a49bea9bf85ce07315471dca4722107e`.

## Unchanged-photo navigation checkpoint — retained completed pixels

This checkpoint starts from `b39ae1e1` and precedes the concurrent JPEG/CMM Rust
migration. It is partial navigation evidence, not milestone acceptance. The new
`raster_workloads --photo-navigation` fixture keeps native U16 ProPhoto source,
32 paint layers, a committed stroke and five masked/revisable adjustments. It
then changes only the camera. Each image contributes five phases, two repetitions
and 96 frames per repetition: 960 frames at 1600×1000. The phases cover fit,
50%, 100%, 200%, and continuous fit-to-200% zoom, with full rotation and panning
around a circle of radius 40% of each document axis. The first measured frame
includes changing the initial 1024×768 view to 1600×1000. Native paint/mask roots
and artwork preview revision remain unchanged.

Frame CPU, CPU through actual managed presentation submission, and completion of
**all** queue work are recorded separately. The presentation target is
RGBA16Float extended-linear sRGB; all retained display and composition pixels
remain Float32. Runs are unpaced/offscreen and do not establish native input or
Wayland presentation latency.

The old singular-value calculation was numerically unstable at exact 25%/50%
zoom. Float32 rotation rounding repeatedly crossed a mip boundary. A stable
singular-value formula and tolerance below one millionth of a surface pixel per
texel remove that oscillation; real zoom crossings still choose finer detail.
The rotation-only control removes the 24 MP fit-phase stalls but does not fix
real level transitions. Retaining already-produced complete display mip levels
addresses those transitions. Native detail is allocated and seeded while the
photo is composed, within its component allowance. Offscreen/zoomed-out edits
invalidate old native detail; pending cache keys publish only after submission.
There is no reduced-resolution filter evaluation or change to authoritative
samples, queries, history or export.

The display component ceiling is 608 MiB: the existing 272 MiB detail allowance
plus at most 336 MiB of completed display mips. Whole-device/process budgets are
unchanged. The small-budget fallback still uses a bounded view window. A full
native image is retained only if it fits the component ceiling (24 MP here).
Larger photographs retain bounded toroidal native detail and complete reduced
display levels. No unbounded whole-document composite was introduced.

### Matched baseline/current observations

Values below combine the two 96-frame repetitions per phase; maxima and deadline
misses remain visible rather than hiding the two slow frames in a 960-frame
aggregate. The current executable uses manual 64–128 MiB device allocation
blocks. Each arm runs at the same staged executable pathname. Builds and GPU
checks finished before these measurement runs.

| Photo / phase | Baseline completed p99 / max ms | Current completed p99 / max ms | Misses over 8.333 ms, baseline → current (of 192) |
| --- | --- | --- | --- |
| 24 MP fit / pan / rotate | 70.231 / 70.911 | 0.750 / 1.993 | 111 → 0 |
| 24 MP half / pan / rotate | 29.126 / 29.146 | 0.183 / 1.956 | 61 → 0 |
| 24 MP native / pan / rotate | 2.708 / 2.724 | 0.095 / 0.103 | 0 → 0 |
| 24 MP double / pan / rotate | 1.564 / 1.651 | 0.122 / 1.937 | 0 → 0 |
| 24 MP varying zoom / pan / rotate | 23.770 / 23.851 | 0.378 / 1.959 | 8 → 0 |
| 45 MP fit / pan / rotate | 97.909 / 117.051 | 0.651 / 2.445 | 6 → 0 |
| 45 MP half / pan / rotate | 43.078 / 43.499 | 0.136 / 2.075 | 89 → 0 |
| 45 MP native / pan / rotate | 3.337 / 3.714 | 3.242 / 3.419 | 0 → 0 |
| 45 MP double / pan / rotate | 1.921 / 2.686 | 1.772 / 2.089 | 0 → 0 |
| 45 MP varying zoom / pan / rotate | 78.807 / 79.877 | 8.031 / 8.209 | 18 → 0 |
| 60 MP fit / pan / rotate | 135.216 / 148.954 | 0.371 / 0.654 | 2 → 0 |
| 60 MP half / pan / rotate | 40.153 / 41.798 | 0.264 / 2.042 | 86 → 0 |
| 60 MP native / pan / rotate | 2.990 / 3.088 | 3.662 / 3.827 | 0 → 0 |
| 60 MP double / pan / rotate | 1.815 / 2.528 | 1.788 / 3.136 | 0 → 0 |
| 60 MP varying zoom / pan / rotate | 93.970 / 94.487 | 9.385 / 9.555 | 21 → 2 |

All 960 current 24 MP frames reuse completed pixels with zero scene recomposition.
The two 60 MP misses occur at step 95, scale 0.5225657, following zoomed-out
navigation: 48 uncached native tiles (3,145,728 pixels) are recomposed. CPU through
submission is 8.138/8.300 ms; full completion is 9.385/9.555 ms. These misses
remain open. The 60 MP native-phase p99 increase of 0.672 ms also exceeds the
relative regression trigger. Retained-mip copying and changed detail residency
add work on a native cache miss; the precise contribution still needs isolation.
Being under the absolute gate does not waive that investigation.

| Photo | GPU live / reserved MiB, after navigation | Process RSS high-water MiB |
| --- | --- | --- |
| 24 MP | 647.25 / 832 | 654.82 |
| 45 MP | 655.35 / 880 | 922.03 |
| 60 MP | 732.35 / 936 | 1096.23 |

These are single-document measurements; concurrent documents/jobs and full
transforms still need aggregate memory qualification. Driver-private allocations
are separate. The selected device and power conditions are recorded in each
`*-environment.json` and GPU log, using the declared reference workstation.

The initial default `MemoryHints::Performance` variant reserved 1152 MiB at
60 MP, exceeding the 1 GiB steady gate. `MemoryHints::MemoryUsage` reduced this
to 772 MiB, but repeated 45 MP navigation exposed 16–29 ms tails around source
uploads. The final manual block range retains the memory bound and removes those
tails in this run. This comparison supports the policy choice; it does not prove
the precise driver allocation mechanism. The hints are documented by
[wgpu](https://docs.rs/wgpu/30.0.1/wgpu/enum.MemoryHints.html); installed wgpu-hal
30.0.1 maps this range to 64–128 MiB device and 32–64 MiB host blocks on Vulkan.
Other backends may ignore the hints and are not qualified here.

### Correctness and reproduction

The seeded-cache GPU executable passes all 10 live-display, four display-mip,
and four managed-view tests. New checks cover 1,440 rotations per mip level,
reflections/nonuniform scale and real boundary crossings; retained pixels match
an independently rendered constrained-window path within 5e-6 Float32 channels.
A zoomed-out edit followed by native zoom matches a fresh dense reference.
Five separate native GTK processes pass managed canvas/artwork agreement,
bounded-canvas startup/painting, Assign/Convert/depth/history/copy, resized export
and cancellation, and wide-color GPU failure/recovery. Those native checks use
the same cache implementation with the preceding MemoryUsage allocation policy;
the final manual-policy GTK build remains to be tested.

Artifacts are in `artifacts/color-m2/final-performance/`. Exact baseline SHA-256:
`02bbea9257e205dffde0a7952367988f5118a4eb7e18d5699b5a72428c758590`;
current `navigation-balanced`:
`ae8df77dd71001e91e8cf3e7192dff05b8ee091efbf5728d939c856deb1d3386`.
`navigation-*-builds.json`, captured source trees/patches, raw per-frame CSV,
`photo-expanded-*-runs.json`, environment records and `/usr/bin/time` output
preserve provenance. `navigation-checkpoint-summary.json` contains per-phase
counts, p95/p99/max and missed deadlines. The 45 MP baseline uses the identical
baseline executable under the `navigation-baseline-45` alias. GPU correctness
SHA-256 is `c0a96f1b237d9a5244cf11bd1998ee5b68de46641e599f4223f962eb222fa3bf`;
GTK correctness is `fc16cfd3bbab73a4e6d5a404ff0887ee0d027a9c675dc36f0204f562e4028454`.
The first `navigation-retained-final` measurements accidentally overlapped a GPU
check and are excluded by `navigation-retained-final-run-exclusion.json`; the
`navigation-retained-isolated` runs replace them. Correctness durations from
runs overlapping compilation are not latency evidence.

Reproduce the workload with a release `raster_workloads` binary:

```sh
raster-workloads 60mp --photo-navigation --space prophoto --depth 16 --output-dir /tmp/photo-navigation
```

`run-photo-navigation.py navigation-balanced 24mp 45mp 60mp` retains the exact
captured binary, telemetry and timeout invocation used here. Final GTK
presentation, remaining 60 MP cold detail, aggregate resource qualification and
post-migration color regression checks remain open. No other platform host is
approved by this checkpoint.

## Navigation-only detail misses no longer rebuild completed mip levels

Follow-up to `1adf1afa`. A native detail miss was repeating every mip reduction
and retained-level copy, even when the artwork revision had not changed. The
renderer now distinguishes artwork damage from camera-only missing detail before
combining their tile requests. With a complete retained pyramid, camera-only
native detail copies directly from the full-resolution composition output into
the bounded detail texture. Already-current display levels are left alone.
Edits, animation, startup and constrained-cache fallback retain their existing
reduction/invalidation path. Filter evaluation, storage precision and cache
ceilings are unchanged; this does not optimize dirty-image regeneration.

The retained-mip regression test now explicitly evicts native detail, navigates
to central and partial bottom/right tiles, and compares reconstructed detail to
full composition. All completed mip levels must remain bit-identical. All ten
live-display tests pass (14.57 s). Separate native GTK processes pass managed
canvas/artwork agreement, bounded-canvas painting and wide-color GPU recovery
with the final manual allocator policy. Those correctness runs overlapped a CPU
build; their durations are not performance evidence.

The additional native fixture submits 960 requests on an absolute 120 Hz schedule
through `UiSession::gesture` and GTK's ordinary change/wake path, without waiting
for the previous render. It records exact requested/queued camera matrices and
Wayland frame IDs; the analysis joins matching requests to actual presentation
feedback. It preserves the entire document and artwork preview revision. This
is software request-to-presentation, not physical pointer/pen/touch latency. The
source has five pointwise adjustments and 32 paint layers, but no painted stroke
or adjustment mask; the offscreen fixture retains those separately.

These builds use a frozen source checkout of `1adf1afa` plus the recorded navigation
changes while another agent replaces JPEG/CMM dependencies in the shared worktree.
They deliberately retain the pre-migration color dependencies. Post-migration
color/workflow qualification is still required. An existing editor process was
left running on the measured GPU (515 MiB attributed allocation; 3% device load
at the pre-run sample); the paired follow-up captures retain background activity.
Our builds and GPU correctness tests finished before these timing runs.
Background application activity was recorded but not controlled.

### Offscreen work and native presentation are different gates

The first follow-up offscreen run has zero completed-work misses across all
2,880 frames. Per-photo worst phase p99 / maximum are 0.840 / 2.137 ms (24 MP),
5.462 / 6.923 ms (45 MP), and 6.534 / 7.628 ms (60 MP). GPU reserved memory remains
832 / 880 / 936 MiB. Process high-water is 647.34 / 918.39 / 1092.66 MiB. The
60 MP native-phase p99 is 2.804 ms, below the original 2.990 ms baseline; the
extra-mip-work change addresses the preceding checkpoint's native-phase regression.
The two 60 MP cold-zoom misses also disappear in this run.

The initial native fixture used an ordinary 1200×900 window: request-to-present
p99 was 8.226 ms, maximum 10.578 ms, four over-budget requests and one unpresented
request. The final fixture maximizes the editor and records a 1600×1000 canvas:

| Photo | Render-worker CPU p99 / max ms | GPU p99 / max ms | Request-to-present p99 / max ms | Presented requests / 960 | Rounded missed refresh slots |
| --- | --- | --- | --- | --- | --- |
| 24 MP | 0.694 / 1.252 | 0.395 / 0.704 | 12.183 / 61.906 | 952 | 8 |
| 45 MP | 2.081 / 7.190 | 2.334 / 7.510 | 13.827 / 137.946 | 937 | 23 |
| 60 MP | 2.534 / 7.020 | 2.863 / 7.572 | 13.962 / 21.406 | 956 | 6 |

No native render-worker frame exceeds 8.333 ms, but the request-to-presentation
gate is **not passed**. Normal presentation cadence is approximately 120 Hz;
cadence alone cannot waive the latency or gap observations. GTK frame-handler
p99 is 0.052 ms in the 60 MP run. Current `FrameClock::deadline` schedules work
three quarters of a refresh interval before presentation; fixed-phase synthetic
requests can wait for the next such deadline. That explains a potential source
of the persistent delay, but does not establish the cause of the 61/138 ms tails.
Those tails need isolation from compositor/background load before attributing
them to the editor. Physical-input, unlike-monitor and modal device-fallback
qualification remain outstanding; unchanged-photo navigation with costly physical
or document-wide filters is also not covered by this pointwise-adjustment fixture.

Artifacts: `navigation-native-frozen-*` records the initial native fixture;
`navigation-native-reuse-*` records maximized GTK and correctness runs;
`photo-expanded-navigation-reuse-*` records offscreen measurements. Exact
production example SHA-256:
`5d108d68f0a7ec8aeb26d0af8ad815e7f561dd51727cda18d22347af66f4cf90`;
GPU test executable:
`bf33ab013284d2dfc96caa11d4d95f7b554054f93103b64011264f3e450093f9`;
GTK test executable:
`9fb6d37111c98b0213499c470628f9c015a5c69e296ce740e3eea4ad009be848`.
The source/provenance JSON identifies the frozen base and every override.
`tools/performance/photo-navigation-report.py REPORT.json` reproduces the native
summary; the benchmark guide gives the test name, environment and harness.

A second matched pair (`navigation-balanced-control` and
`navigation-reuse-repeat`, identical respective executable hashes) confirms the
camera-miss improvement under the current background load. At 60 MP the varying-
zoom p99/max falls from 10.225/11.236 ms (three misses) to 6.710/7.245 ms (zero).
The native-phase p99 falls from 3.980 to 3.334 ms. At 45 MP those phase p99s fall
from 8.204 to 5.939 ms and 3.549 to 2.464 ms. Both 24 MP arms have zero misses;
their already-cached native-phase p99s are 0.306/0.295 ms. The earlier 24 MP warm
increase relative to the original quiet run repeats in the unchanged control,
so it is not attributable to this code change. The repeat's worst overall frame
is 2.183/6.556/7.245 ms for 24/45/60 MP, with zero misses across all 2,880 frames.
These pairs qualify the targeted work reduction; the native presentation gaps
and post-migration checks remain open.

### Three-document resource and preservation check

`navigation-resource` aliases the same production executable and runs the full
`--photo multiple` fixture: retained 24+45+60 MP ProPhoto16 documents, adjustment
history, simultaneous native save/profiled PNG export and editing, repeated full
histograms, undo/redo and Gaussian blur. It completes successfully in 54.83 s,
including exact retained native source/paint/mask comparisons and the fixture's
export/histogram consistency assertions. Regeneration timings remain deferred
observations and were not optimized or treated as passing interaction gates.

Process RSS high-water is 2,904,612 KiB (2.770 GiB), final RSS 2,765,780 KiB
(2.638 GiB). The conservative combined GPU reservation bound is 2,977,955,840
bytes (2.773 GiB), within the declared 3/4 GiB multi-document envelope. This sums
each retained document's peak and the active device's maximum including its
shared capture workers, without double-counting the shared device. The active
60 MP device peaks at 1128 MiB reserved and ends at 1064 MiB after blur; its live
allocation is 894.01 MiB. The combined gate fits, but that final device reservation
is 40 MiB over the single-document 1 GiB steady gate and requires a focused check.
The exact CPU sample cache stays at its 512 MiB ceiling; upload scratch/staging
peaks at 8 MiB. Logs, environment, GPU samples, command/executable provenance and
`/usr/bin/time` output use `photo-expanded-navigation-resource-multiple*`.

## 4K gesture failure and memory-policy correction — 2026-09-15

The user reported a stopped GPU worker while pinching/rotating a 61 MP photograph
on a 4K monitor at 200% scale in a nonmaximized window. The corrected native
fixture uses a 9504×6336 source and `LAYER_TEST_MONITOR=3840x2160@120`,
`LAYER_TEST_SCALE=2`, `LAYER_NAVIGATION_MAXIMIZE=0`. `GDK_SCALE=2` alone did not
establish Wayland output scale: that initial probe had a 1200×900 render surface
and is not evidence for the reported configuration.

With actual Mutter scale 2, the render surface is 2400×1800. The reproduced
worker error requested 660,933,728 display bytes against the fixed 637,534,208-byte
(608 MiB) component limit. Optional result polls could consume the detailed
worker error, leaving only "GPU worker stopped". The worker now retains and logs
the cause. The navigation fixture now asserts successful gestures and a live
renderer throughout; its earlier `>100 presentations` check could falsely pass
following a late worker failure. The reproduction's old test exit code is thus
not a pass: the recorded error and 817/960 camera publications show the failure.

The bounded fallback now packs visible tiles into an atlas, omits rotated-view
corners outside the sampling halo, and allows optional completed mips to yield
space. Float32 storage and full-resolution filter evaluation are unchanged.
Growing the atlas copies valid pixels on the GPU instead of recomputing them.
Replacement temporarily overlaps the old/new bounded atlas allocations; this
peak is distinct from its 608 MiB steady component limit. Thirteen fallback
GPU checks pass, including 4K/61 MP planning, arbitrary-angle Float32 pixel
comparison, edits, exact undo/recovery, and preserving pixels during growth.
The compared reference uses complete Float32 bilinear sampling: dense hardware
bilinear weights are quantized and are not a bit-precision oracle at arbitrary
rotation angles.

The first atlas native run at 3840×2160, scale 2, completes all 960 requests without
stopping the renderer. It exposes the remaining performance problem: five worker
frames exceed 8.33 ms; three regenerate 17–21 million pixels, taking 38–55 ms.
GTK frame handling stays below 0.14 ms. Fullscreen zoom can exhaust the small
cache, evict mip levels and trigger full-resolution recomposition with no artwork
change. Artifacts: `navigation-crash-61mp-scaled*`, `navigation-atlas-61mp-4k*`,
and `navigation-atlas-preserve-gpu-tests.log` under the final-performance directory.
These are diagnostic observations, not a completed 120 Hz qualification.

### Latest user direction supersedes fixed memory ceilings

The user explicitly directed use of available RAM/VRAM for interactive work,
spilling less important/inactive content down the memory hierarchy when needed,
and avoiding extra memory that does not help reach 120 Hz. Future tabs must fit
this ownership model. The original 1/2 GiB single-document and 3/4 GiB
multiple-document targets above are now historical comparison points, not hard
acceptance caps. Precision and preservation requirements are unchanged.

The implementation admits a complete Float32 display pyramid from the
Vulkan driver's current device-local heap budget minus usage, leaving most
headroom for editing, GTK and other documents. The allowance is not an allocation
target: only the actual document's completed pixels/mips are allocated. A 61 MP
Float32 pyramid needs about 1.2 GiB; available VRAM makes unchanged navigation a
sampling operation instead of repeated filter evaluation. Smaller documents use
proportionately less storage. Devices without a suitable reported allowance
retain the bounded path. This is display residency, separate from exact source,
paint, undo and archive ownership; it does not implement tabs or disk spilling.

Vulkan defines the reported budget and usage as changing estimates, not
reservations: [VkPhysicalDeviceMemoryBudgetPropertiesEXT](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceMemoryBudgetPropertiesEXT.html).
Linux admission is read-only and queries the existing Vulkan device; `ash` is
already wgpu's Rust Vulkan binding and introduces no C build dependency. Other
platform host admission remains unimplemented pending approval. Measured resource
and navigation results for this policy must follow before claiming qualification.

### Complete-display native result

The complete-display implementation passes 14 display-cache GPU tests, including
Float32 equality against the bounded path with a physical filter, edits, and zero
navigation recomposition/source misses. GPU executable SHA-256:
`fa0063ae235fdf1dc113f651c635d48b63b9ee87499992e432db723b9c1f8d3c`.
The native executable is
`373f06c00786d89c2cc5ff5b6ba3b6e1fa7427aede57cb954570f863e8b0319a`;
source bytes and hashes are retained in `navigation-complete-provenance.json`
and `navigation-complete-source/` (base commit `9f7abe15`).

Repeatable command, after building/capturing the release test executable:

```bash
LAYER_TEST_SCALE=2 LAYER_TEST_MONITOR=3840x2160@120 \
LAYER_NAVIGATION_PHOTO=61mp LAYER_NAVIGATION_COMPLETE=1 \
bash tools/performance/gtk-raster.sh "$GTK_TEST_EXECUTABLE" \
  workspace::tests::native_navigation::native_large_photo_navigation \
  artifacts/color-m2/final-performance/navigation-complete-61mp-4k
python3 tools/performance/photo-navigation-report.py \
  artifacts/color-m2/final-performance/navigation-complete-61mp-4k.json
```

Measured 3840×2160 at actual scale 2; 960 requests over eight seconds, all matched
to presentation, zero discarded/unmatched requests and zero missed refresh slots.
All camera frames retain identical composition/source-miss counters: **zero
recomposited pixels and zero source decoding**. The complete display occupies
1,226.16 MiB (1.197 GiB), versus approximately 608 MiB for the constrained atlas.
Only the actual pyramid is allocated, not the driver's available allowance.

| Metric | Constrained atlas | Complete display |
| --- | ---: | ---: |
| Worker CPU p99 / maximum | 4.218 / 54.959 ms | 0.510 / 1.443 ms |
| Worker GPU p99 / maximum | 5.824 / 56.454 ms | 1.003 / 1.941 ms |
| CPU / GPU frames over 8.333 ms | 5 / 5 | 0 / 0 |
| Presented requests / requested | 936 / 960 | 960 / 960 |
| Missed refresh slots | 24 | 0 |
| GTK frame handler maximum | 0.134 ms | 0.094 ms |

This confirms the cache/refill cause of the reported fullscreen stalls and
sustains 120 Hz presentation in this run. It does **not** establish an 8.333 ms
input-to-present bound: request-to-present p99 is 11.545 ms, maximum 13.224 ms.
The existing phase-aligned native scheduler is a separate source of delay. Do not
substitute frame cadence or worker GPU time for that latency. Native physical
input/calibrated-display and other-platform qualification remain separate.
The environment snapshot records the workstation and competing GPU activity;
these are private-compositor runs on the shared development workstation.


### Integrated resource and recovery checkpoint

The portable JPEG/CMM build plus complete display passes the full renderer suite:
249 passed, zero failed, 26 optional hardware/performance checks ignored
(`navigation-complete-all-gpu-tests.log`, 398.97 s). The current private GTK runs
pass injected GPU failure/recovery, repeated retrieval of the original error,
managed canvas/GTK artwork agreement, and native document files. The runnable
release GTK application also builds. Correctness-suite durations are not latency
measurements. Exact commands and results are in `navigation-complete-gtk-checks.json`.

The same build completes the expanded 24/45/60 MP and three-document fixtures,
including live adjustments/masks, concurrent saving/export/painting, repeated
histograms, blur, archive reopening and exact paint/mask undo/redo comparisons.
The captured CLI executable is SHA-256
`26cda9120ddf8bfd009484cea57eea3f14264dafbdb538ce5c13fe7b05d1ffea`.

| Retained workload | Process RSS high-water | Final process RSS | Conservative combined GPU reservation peak |
| --- | ---: | ---: | ---: |
| 24 MP | 904.03 MiB | 904.03 MiB | 1,079 MiB |
| 45 MP | 1,306.82 MiB | 1,256.49 MiB | 1,632 MiB |
| 60 MP | 1,514.41 MiB | 1,382.16 MiB | 1,928 MiB |
| 24 + 45 + 60 MP | 2,834.53 MiB | 2,704.84 MiB | 3,935 MiB |

GPU reservation includes allocator slack/staging and shared capture workers;
other retained devices' peaks are summed conservatively. It excludes
unreported driver-private allocations. Final active-device reservations are
951/1440/1864 MiB for 24/45/60 MP, with live allocations 691.30/1240.89/1552.89 MiB.
The three-document case retains complete displays on all three renderers: it is
a resource stress fixture, not implemented inactive-tab eviction. The CPU exact
sample cache remains at or below 512 MiB, and upload scratch/staging at 8 MiB.
Use the larger of in-process VmHWM and `/usr/bin/time` high-water measurements;
those sources differ on the 24 MP run (925724 versus 885976 KiB). Raw records,
environment and commands are `photo-expanded-navigation-complete-*` and
`navigation-complete-resource-session.log`.

These totals quantify the cost of predictable navigation under the revised
memory policy. The allowance is one quarter of reported remaining device-local
headroom; only the required display pyramid is allocated. Admission is currently
a snapshot, not a global reservation or runtime pressure manager. A complete
texture must also fit the device's maximum texture dimensions; otherwise the
bounded atlas remains in use. RAM/disk demotion of inactive documents is future
work. No claim of that hierarchy follows from a successful memory benchmark.

Dirty photo regeneration remains explicitly deferred. The resource fixture's
sliders, blur and full histograms still take tens to hundreds of milliseconds
(or seconds for histograms). Concurrent painting also records isolated frame
misses. Those observations are retained, not labeled as successful 120 Hz edit
regeneration. Unchanged navigation is the current latency target.

### Windowed presentation follow-up

The first current 2400×1800, scale-2 native windowed run preserves all artwork
and performs zero source decoding/recomposition, but presents only 935/960
requests, with 25 missed refresh slots and a 192.691 ms request-to-present maximum.
Worker CPU/GPU maxima remain 1.267/1.535 ms. Thus the crash fix and fast render
work pass, while this run does not qualify presentation cadence. The fullscreen
960/960 result above must not hide it. Artifacts are
`navigation-complete-gtk-61mp-window*`.

An isolated repeat trips the zero-recomposition assertion before writing its
report (`navigation-complete-61mp-window-repeat.log`). The fixture now writes
its report before that assertion so a future failure preserves the counters.
This unexpected work is under investigation; it is not silently waived as noise.


The diagnostic repeats preserve the evidence. One presents 960/960 requests
with zero missed slots, CPU/GPU maxima 1.564/0.812 ms. Two others present 937/933,
with 23/27 missed slots; their CPU/GPU render maxima remain below 1.71 ms.
The counter change is **only source misses, 950 → 1900**; composited pixels
remain exactly 120434688 and storage 1285724992 bytes. Inspection identifies
the cold layer-thumbnail scan on the same GTK GPU worker: it reads the 950
source tiles outside the timed canvas frame. Thus the global zero-source-work
assertion also includes this separate job. The completed display itself does
not regenerate. Batching that background scan remains a focused follow-up.
Reports are `navigation-final-61mp-window*`; executable hashes are retained in
`navigation-final-executables.json`. The final texture-dimension guard passes
all 15 display tests (`navigation-final-display-tests.log`, 24.65 s).


## Background thumbnails yield to interactive canvas work

The cold original-photo thumbnail used to scan all 950 source tiles in one
`Command::Thumbnail` call on GTK's canvas worker. Its source-cache counters
exposed the work between timed canvas frames; it explained the reproducible
~170–193 ms queue stalls despite sub-2-ms rendering. The first batching trial
removed that large pause but still competed with navigation: the windowed trial
presented 859/960 requests. A small background batch alone was insufficient.

GTK now prepares at most four original tiles per batch, checks queued canvas
work first, and waits for 50 ms without a canvas frame before doing thumbnail
work. Thus a new gesture can interrupt idle preparation between small batches,
and continuous motion receives the worker. No full-resolution preview pipeline,
extra full-image copy, filter-resolution change or precision reduction is added.
Prepared thumbnail pixels remain private until the original is complete. The
existing exact area-integration and edited-tile correction algorithm is retained;
completed thumbnails remain cached. Other hosts keep their current entry point.

Nine focused thumbnail GPU tests pass; one optional timing test is ignored
(`navigation-thumbnail-batch-gpu-tests.log`, 45.28 s). The new check covers bounded
progress, discarded batches, restart, exact repeated output, no warm source
decoding, and the existing Float64 area-integral tolerance of 0.00002.
GPU executable SHA-256:
`fa72cfe7b23db1316723011f8504267945aac418d5c7be5b934f06295ebe2765`.

The native fixture now separates a camera frame's own source misses from the
cumulative source counter, which also includes background jobs. It still asserts
zero camera source misses and zero recomposition, and measures native presentation
across all background interference. `LAYER_NAVIGATION_PHYSICAL=1` adds the actual
Gaussian blur (sigma 4) at full photo resolution. This checks navigation of a
physical filter's completed output, not its deferred regeneration speed.

Final serial 61 MP, actual scale-2 native runs, 960 requests each:

| Case | Presented / missed refresh slots | CPU maximum | GPU maximum | Request-to-present p99 |
| --- | ---: | ---: | ---: | ---: |
| 2400×1800 window, first | 957 / 3 | 1.923 ms | 2.203 ms | 9.694 ms |
| 2400×1800 window, repeat | 941 / 19 | 1.847 ms | 1.831 ms | 10.668 ms |
| 3840×2160 maximized | 933 / 27 | 1.792 ms | 2.088 ms | 10.108 ms |
| 3840×2160, physical Gaussian blur | 956 / 4 | 1.950 ms | 1.897 ms | 12.106 ms |

Every run has zero camera recomposition/source misses, no thumbnail source work
during navigation, and no timed CPU/GPU frame above 8.33 ms. The full-image
thumbnail pause is absent. These results do **not** pass a zero-missed-refresh or
8.33-ms input-to-present gate: synthetic request delivery itself has 16–33 ms
maximum lateness, and native pacing still has gaps. The rendering/storage cause
has been removed; further memory is not supported as the remedy for the remaining
presentation/scheduling behavior. Shared-workstation load and native scheduling
must be distinguished before attributing the residual gaps.

GTK executable SHA-256:
`fcce334366ea50f841f0499e8e9615561d1829f4ef95e152fbf073989257a449`.
Artifacts are `navigation-idle-thumbnail-{window1,window2,fullscreen,fullscreen-blur}*`,
`navigation-idle-thumbnail-native-runs.json`, and the build/executable records.
The unsuccessful first batching measurements are retained under
`navigation-thumbnail-{window1,window2,fullscreen,fullscreen-blur}*`.
Reproduce with the existing native navigation command above, the final executable,
`LAYER_NAVIGATION_COMPLETE=1`, the chosen `LAYER_NAVIGATION_MAXIMIZE`, and optional
`LAYER_NAVIGATION_PHYSICAL=1`. Do not overlap these timings with compilation.


Final native correctness checks pass idle photo-thumbnail publication, Clear,
exact thumbnail/artwork restoration on Undo, wide-color GPU failure/recovery,
and managed canvas/GTK artwork agreement. Results and exact commands are in
`navigation-thumbnail-qualified-functional-runs.json` (2.71/6.86/2.81 s).
The focused native thumbnail test deliberately excludes the document's monotonic
publication revision from artwork equality; its initial failure was revision 2
versus 0, with all image and layer values equal. No pixel tolerance changed.
Final correctness executable SHA-256:
`8ef55404fb8d91401e884afabb530ed5bb8f7d995e458e060c594e68eb909757`.
Its production source is unchanged from the timing executable above; only the
focused test and its revision assertion were added afterward.

The broader `native_layer_panel_review` stops earlier at its nested-row column
alignment assertion (x=6 versus x=0), identically on the pre-thumbnail-change
binary and current build. Both failed logs are retained as
`navigation-thumbnail-parent-layers*` and `navigation-idle-thumbnail-layers*`.
This unrelated existing fixture failure is not counted as a pass or changed in
this patch; the dedicated test validates actual thumbnail publication/history
without traversing that unrelated layout assertion. No drag behavior changed.

## Smooth navigation scope and latency investigation — 2026-09-15

The user selected **smooth 120 Hz navigation; document latency**, replacing the
strict input-to-present p99 ≤ 8.33 ms completion gate. Dirty-image/filter
regeneration optimization remains deferred. This checkpoint changes measurement
and documentation; it does not change production pacing or pixel processing.

The reason for the reported input-latency regression is **not established**.
The code's three-quarter-refresh scheduling lead dates to `1beaf671` on
2026-09-11, before milestone 2. It explains part of current waiting time, but is
not a newly introduced deadline. Earlier 8.226 ms and later 12–14 ms p99 reports
also differ in viewport and uncontrolled input phase; those are not a matched
before/after attribution. The known cache-refill and cold-thumbnail stalls have
separate reproduction and fixes above. Neither fix proves the cause of the
remaining first-interaction gap.

### Reporting correction

Wayland feedback for startup frames can arrive after the test clears its stats.
The earlier report counted their old presentation timestamps in navigation
cadence, including idle time before the first request. The report now restricts
cadence/discards to measured frames queued after navigation starts. It retains
every requested gesture in the denominator, reports missing requests by phase,
and separately measures the interval from the first request to the first
navigation presentation. Thus dropping/coalescing initial requests cannot make
the startup gap disappear from the evidence. Original raw reports and summaries
remain; corrected summaries have the suffix `-scoped-summary.json`.

Three deterministic report tests pass, covering late startup feedback, initial
unpresented input, measured discards/gaps, and repeated camera poses. Reproduce:
`python3 -m unittest discover -s tools/performance -p test_photo_navigation_report.py`.
The corrected four prior thumbnail runs have 0/16/23/2 missed slots, versus
3/19/27/4 previously reported. Presented request counts and latency percentiles
are unchanged; first navigation presentation takes 34.795/35.754/44.470/28.845 ms.
All unmatched requests in those runs occur in the first `fit-pan-rotate` phase.

### Bounded deadline experiment

Eight serial private-Mutter runs use the same captured executable, 61 MP
ProPhoto16, 3840×2160 at scale 2, and 960 requests over eight seconds. Each pair
starts at a nominal input offset of 0, 2, 4 or 6 ms from the then-current display
prediction. Feedback continues to update that prediction. The test-only control
compares the production-equivalent 6.25 ms lead with a 3 ms lead; production
remains unchanged. There are no overlapping builds or other test workloads.

| Initial offset | Lead before presentation | Presented / 960 | Missed slots after first navigation presentation | First navigation presentation | Request-to-present p99 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 0 ms | 6.25 ms | 956 | 2 | 29.802 ms | 13.004 ms |
| 0 ms | 3 ms | 956 | 1 | 30.283 ms | 7.701 ms |
| 2 ms | 6.25 ms | 950 | 6 | 44.641 ms | 14.351 ms |
| 2 ms | 3 ms | 956 | 1 | 28.971 ms | 4.672 ms |
| 4 ms | 6.25 ms | 951 | 5 | 45.475 ms | 13.202 ms |
| 4 ms | 3 ms | 953 | 3 | 44.653 ms | 9.539 ms |
| 6 ms | 6.25 ms | 952 | 4 | 43.491 ms | 12.087 ms |
| 6 ms | 3 ms | 950 | 5 | 45.296 ms | 16.049 ms |

All camera frames have zero recomposition and zero source misses. Across all
eight runs, CPU maximum is 1.871 ms and GPU maximum is 2.161 ms. Every request
in each later phase (`half`, `native`, `double`, continuous `zoom`) is presented:
768/768 per run. This is sustained 120 Hz evidence for those later phases, not
permission to remove first-use behavior from acceptance. Synthetic input delivery
is late by up to 18–34 ms at startup. Its cause remains unresolved; these records
do not distinguish GTK startup callbacks, driver work and other scheduling.

With the production-equivalent lead, median request-to-enqueue delay is
5.293–7.857 ms and median enqueue-to-present delay is 6.187–6.219 ms. The 3 ms
lead reduces the latter to 2.940–2.960 ms, but worsens total p99 for the 6 ms
input-offset case. It is therefore not selected as a production fix. The old
deadline also protects GTK overlay and wet-paint/transform work; a navigation
comparison alone does not qualify changing it for drawing.

Reproduce the native command above with `LAYER_NAVIGATION_PHASE_NS=0` (then
2000000/4000000/6000000) and `LAYER_PACING_LEAD_NS=6250000` (then 3000000).
Raw records, summaries and exit codes are `navigation-phase-*` and
`navigation-phase-runs.json` under `artifacts/color-m2/final-performance/`.
The captured executable is `navigation-phase-gtk-tests`, SHA-256
`53ea0656b732ce5d5d8eeed8f70e507d434f4415003647207ef2b82b7a55485f`.
Its source starts at `5717d571` plus the test-only pacing and initial-phase
controls; the committed controls retain that behavior. No production renderer
or scheduler change follows from this experiment.

The strict latency threshold is closed as a user-approved scope change, not a
passed measurement. GTK completion still requires resolving or bounding the
first-interaction interruption and closing the remaining correctness audit.
Other hosts, calibrated physical displays, unlike-monitor movement/spanning,
constrained-memory 120 Hz qualification and future inactive-tab spilling remain
outside the demonstrated platform envelope.
