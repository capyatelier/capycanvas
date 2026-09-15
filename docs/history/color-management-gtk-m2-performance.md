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
