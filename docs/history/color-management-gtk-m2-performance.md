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
