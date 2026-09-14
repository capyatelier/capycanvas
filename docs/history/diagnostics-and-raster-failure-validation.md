# Diagnostics visibility and raster failure containment

This follow-up to color milestone 1 started from `23bc780`. The fixes are in
`566eac3`; integration includes concurrent main changes through `9996f51`.
The conclusions below come from failing regressions and hardware runs, not the
milestone plan's assumptions.

## What failed

Diagnostics sampling recognized docked tabs and individual content drawers,
but excluded a whole column opened from a collapsed stack. GTK therefore kept
its initial telemetry snapshot, showing unavailable GPU timing and no CPU
samples. Android also collected no timing samples in that presentation. The
timestamp feature request and renderer telemetry forwarding were still present.
The shared visibility policy now recognizes the open stack member, stops
sampling when it closes, and reapplies sampling after renderer replacement.

The reported Domain Warp mask crash was reproduced on the same NVIDIA RTX PRO
6000 Blackwell GPU and 610.57.04 Vulkan driver, with two raster layers, an effect
mask and a large transparency brush. The failing hardware test produced exactly
`x=4294966015, y=4294967295, w=1281, h=1` against a 256×256 render target.

The rectangle/page arithmetic predates the engine refactor. A brush wholly above
the canvas clips to zero height. Inclusive page iteration nevertheless visited
row zero because it saturated `max - 1`. Intersecting that phantom page yielded
the old empty sentinel with `u32::MAX` minima. Page-local conversion and width
subtraction then turned the sentinel into the invalid unsigned scissor.

The fix replaces that representation with an opaque, canonical half-open
`PixelRect`. Private fields force construction through ordered bounds; empty
rectangles have zero coordinates and area. Page iteration is half-open, and
page-local conversion clips and translates together. The fix applies to every
consumer of sparse damage, rather than clamping only the mask's scissor.

## Failure ownership and recovery

GTK previously logged a stopped worker and continued scheduling it. A panic
could bypass the error reply, and an asynchronous error arriving after GTK went
idle had no wakeup path. Queued immutable revision producers could also disappear
without publishing a result.

The worker loop now belongs to a consuming `Worker::run`. Its outer thread
boundary contains panics from both construction and execution, retires the entire
GPU owner, drops queued producers, and posts a terminal reply plus a GTK idle
wakeup. It never resumes an encoder or renderer that unwound. The idle callback
always runs on GTK's context rather than invoking GTK inline on the worker.

Queued frames own their pending raster publications. Abandonment resolves them
with errors, so saves and restoration do not wait for nonexistent producers.
The shared editor validates a recovery candidate, removes the failed history
suffix, and retains the reachable intact undo history. Failed captures in an
undone branch cannot be restored through Redo. Pending captures are distinguished
from explicit failures; renderer replacement rechecks failures that arrived
during retirement.

GTK suspends painting and scheduling, keeps the document/session, and exposes
**Restart canvas**. Save remains available for surviving pixels. Restart restores
those immutable pixels and earlier history without marking the drawing saved.
If no intact checkpoint exists, recovery reports failure instead of inventing
pixels. An already captured save containing a failed revision still fails;
an older private recovery file is not overwritten by that failure.

## Correctness checks

- Final integration through `9996f51` passes all 490 shared tests (46 core,
  49 engine, 26 host, 368 UI, one workspace), with one separate host workload
  ignored. GTK and Web release builds and the Android build/lint pass through
  `1c83a95`. The GTK fault/restart and file journeys and both Android
  diagnostics/file tests pass again on that merged version.
- The visibility regression failed before the fix and passes for GTK, Android
  and Web policy, including closure and renderer replacement.
- The empty-damage regression failed before the fix. The new geometry test checks
  4,096 combinations of bounds around page boundaries, exact partition area,
  empty regions, and bounded local coordinates even for unrelated pages.
- The real Domain Warp/mask regression failed before the fix with the user's
  exact validation error. It now checks all four outside-canvas edges and a
  corner, unchanged pixels and page allocations outside the canvas, and resumed
  mask painting on returning inside.
- The GPU suite passes 126 tests and all three project round-trip integration
  tests. Eighteen separate hardware workloads remain explicitly ignored by
  the ordinary suite. The selection sharing assertion now permits an empty
  fill to skip preparation entirely while still requiring at most one shared
  preparation across fill, brush and mask consumers; pixel assertions are intact.
- The native GTK test injects a real invalid wgpu scissor into the next frame.
  It verifies prompt capture failure, suspension after idle, unchanged surviving
  checkpoint, successful project serialization, no repeated scheduling, the
  visible restart action, byte-identical export after restart, earlier Undo/Redo,
  and subsequent painting. The independent GTK file/recovery/surface journey
  also passes.
- Android device instrumentation verifies live CPU and GPU samples in an opened
  whole column and the existing exact file/renderer/recovery workflow. Test
  application IDs isolate the user's installed drawing app and files.

## Rendering performance

On the NVIDIA hardware, 25 scenarios × three repetitions covered **10,920
frames**, with zero Move or Pen-up samples over 8.33 ms. Across scenarios,
maximum CPU frame-creation p95/p99 were **2.182 / 2.909 ms**; maximum completed
Move/Pen-up p99 were **4.451 / 3.690 ms**. Every scenario's CPU p95 remains within
the larger prior `hosts-final` run plus `max(5%, 0.20 ms)`.

All seven native GTK pacing workloads passed, with 5,058 canvas frames. Relative
to the previous merged GTK run, no workload's worker frame CPU p95 increased by
0.20 ms. Some native pacing overlapped the Android baseline cross-build; the
result still passed. These are separate observations of host work, GPU completion
and presentation, not physical pen latency.

On the Wacom Android tablet, an initial benchmark lost its presentation surface
and is excluded. Two complete fixed-version runs produced 4,640 measured display
callbacks. An earlier recorded p99 comparison appeared slower, so the previous
commit was rebuilt in a separate app and measured during this session. Median
per-run frame-creation p99 was **2.617 ms before / 2.636 ms after**, a **0.019 ms**
increase within the 0.20 ms allowance. Median p95 was **1.863 / 1.926 ms**.
Maximum fixed-version frame creation was **4.316 ms**. Both versions ran the same
6000×4000 sparse painting workload with concurrent immutable recovery saves.

A further run after integrating Android/header changes through `1c83a95`
passed with 2,317 callbacks: median per-run frame-creation p95/p99 were
**1.944 / 2.381 ms**, with a **4.855 ms** maximum. This also stays within the
contemporaneous baseline allowance. The later `9996f51` integration changes
Apple title-bar workflows and shared header policy, not raster frame creation.

These results qualify these workloads and devices, not every brush or host.
Local artifacts are under `artifacts/color-m1/diagnostics-recovery*`; temporary
build/test logs use `/tmp/capy-recovery-*` and `/tmp/capy-diagnostics-baseline-*`.
The tracked report preserves the measurements; generated files are not shipped.

## Reproduction

Use `cargo test --release -p layer-core -p layer-engine -p layer-ui -p layer-host
--lib` for shared policy and recovery. Run the renderer suite with
`LAYER_GPU_INDEX=0 cargo test --release -p layer-render-wgpu -- --test-threads=1`.
The focused regressions are `clipped_empty_damage_never_visits_a_page`,
`large_transparent_effect_mask_brush_crosses_canvas_edges`, and
`diagnostics_sample_in_open_columns_and_stop_when_hidden`.

Build GTK using `cargo test --release -p layer-linux --no-run`, then run
`native_diagnostics_and_gpu_failure_recovery`, `native_document_files`, and
`native_frame_pacing` with `--ignored --test-threads=1` on a private Mutter/D-Bus
session, isolated settings/workspace/recovery directories, and the hardware GPU.
Use `target/release/gpu-bench --scenario all --repeats 3` for the offscreen
performance comparison.

For Android, use JDK 25 and `:app:assembleDebug :app:assembleDebugAndroidTest
:app:lintDebug -PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.rastertest`.
Install both test APKs, run `AndroidRasterTest`, and run
`AndroidRasterBenchmarkTest` separately. The baseline archive is commit
`23bc780`, built as `art.capycanvas.rasterbaseline`.
