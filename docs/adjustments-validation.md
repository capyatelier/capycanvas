# Filter validation

2026-09-09. All ten filters use the same WGSL runtime on GTK, web and Android.
Shared UI labels are **Filters** and **Properties**.

This is the original ten-filter cross-platform baseline. The expanded catalog
and current incremental/animation measurements are in
[forty-filter validation](filter-library-validation.md).

## Measured rendering cost

Release build, Vulkan on NVIDIA RTX PRO 6000 Blackwell Max-Q. Painted RGBA8 canvas,
25 warm-up updates + 120 measured updates. Full case recomposites every pixel;
incremental case deposits a 60px round dab with 64×64 damage on the same resident
4096×4096 document. Each update waits for completion **only in the benchmark**.
Live rendering never waits this way. GPU timestamps bracket drawing commands,
not presentation; completion includes CPU preparation, submission and GPU wait.

| Case | Full 4K GPU median / p95 / p99, ms | Full CPU + completion median / p95 / p99, ms | Incremental GPU median / p95 / p99, ms |
|---|---|---|---|
| Baseline | 0.084 / 0.085 / 0.086 | 0.243 / 0.407 / 0.460 | 0.010 / 0.010 / 0.010 |
| Curves | 0.212 / 0.214 / 0.216 | 0.883 / 1.137 / 1.491 | 0.013 / 0.013 / 0.013 |
| Levels | 0.161 / 0.163 / 0.163 | 0.919 / 1.043 / 1.165 | 0.013 / 0.013 / 0.013 |
| Brightness / Contrast | 0.134 / 0.136 / 0.136 | 0.872 / 1.007 / 1.052 | 0.013 / 0.013 / 0.013 |
| Hue / Saturation | 0.176 / 0.178 / 0.180 | 0.857 / 1.004 / 1.013 | 0.013 / 0.013 / 0.013 |
| Color Balance | 0.177 / 0.179 / 0.181 | 0.930 / 1.044 / 1.084 | 0.013 / 0.013 / 0.013 |
| Exposure | 0.108 / 0.108 / 0.109 | 0.888 / 1.164 / 2.431 | 0.013 / 0.013 / 0.014 |
| Vibrance | 0.176 / 0.178 / 0.178 | 0.934 / 1.433 / 1.829 | 0.013 / 0.013 / 0.014 |
| Black & White | 0.165 / 0.166 / 0.167 | 0.896 / 1.077 / 1.546 | 0.013 / 0.013 / 0.014 |
| Gradient Map | 0.158 / 0.159 / 0.160 | 1.060 / 1.377 / 1.422 | 0.013 / 0.014 / 0.014 |
| Posterize | 0.148 / 0.149 / 0.149 | 0.876 / 1.197 / 1.218 | 0.013 / 0.013 / 0.013 |
| Ten fused | 1.189 / 1.212 / 1.218 | 2.198 / 2.506 / 2.626 | 0.020 / 0.020 / 0.020 |
| Ten masked | 1.286 / 1.316 / 1.329 | 3.771 / 5.801 / 6.441 | 0.021 / 0.022 / 0.022 |
| Ten clipped | 1.213 / 1.231 / 1.241 | 3.531 / 4.595 / 4.702 | 0.020 / 0.021 / 0.021 |

The masked case uses actual nonuniform R8 masks with alternating inversion, not
constant full coverage. All three ten-filter cases above share one effect render
pass for their tile draws. Incremental CPU + completion medians are .132ms
baseline, .140–.144ms individually, .150ms ten fused, .246ms ten masked and .125ms
ten clipped. The masked chain's incremental p99 is .576ms.

Full-update CPU medians are .582–.793ms individually versus .116ms baseline.
These filters have a small incremental drawing cost on this workstation, but
full 4K changes are not free. All measured warm full-update p99 values fit the
8.33ms rendering budget here; this is not a universal 120Hz guarantee.

With Stats hidden, ten-fused completion is 2.636 / 3.163 / 3.268ms full and
.106 / .138 / .200ms incremental. Differences from the enabled results
include measurement noise and system scheduling. Stats uses three reusable
asynchronous query/readback slots, skipping samples rather than blocking when
busy; there is no canvas readback for telemetry.

Cold shader compilation is excluded above and takes tens of milliseconds on
this workstation. GTK compiles on its GPU worker, not its main event loop.
Parameters reuse pipelines. First insertion or a new chain can still cause a
one-time pause, especially in a browser. Mobile integrated GPUs, translated
masks, complex groups and the end compositor need their own measurements.

Raw run: `artifacts/benchmarks/adjustments.csv` (generated, ignored). The benchmark
also covers 128×128 and 2048×2048 canvases:

```sh
cargo test --release -p layer-render-wgpu adjustment_latency -- --ignored --nocapture --test-threads=1
```

## Correctness and interaction checks

- All ten shaders parsed and validated with Naga, individually and fused.
- Neutral defaults where applicable; known-color transforms, linear exposure,
  curve/gradient interpolation, preserved alpha and transparent input.
- Masked and chained clipped adjustments; hiding and opacity-zero restoration;
  parameter edits reuse pipelines; translated masks affect their new location.
- Seventeen masked filters split at the portable texture limit without losing
  coverage. Isolated groups and incremental changes across tile seams match full
  recomposition.
- A custom generator and adjustment share the runtime, with translucent alpha
  and continuity at 255/256 pixel boundaries.
- Shared UI tests on all three platform profiles cover insertion/reveal,
  moving/hiding/restoring Properties, curve reset, gradient stop alpha,
  undo/redo and section headings.
- GTK tests exercise all ten insertion buttons, generated controls, two-column
  108×72 tiles, gradient insertion and live CPU/GPU stats. The existing Layers
  interaction regression also passes on the private Wayland display.
- Chrome/WebGPU and Android Compose/JNI/Vulkan emulator tests exercise all ten
  filters, generated controls, curve/gradient editing, section headings and live
  stats. Web's existing Layers interaction regression passes too.
- Core/engine/UI suites: 175 passed. GPU suite: 38 passed, two opt-in benchmarks
  ignored; all six filter regressions pass, including the portable-limit test.
- Static packaging tests: 12 passed, including portable SVG outlines, the new module's fingerprints and
  service-worker version propagation. The static PWA builds successfully and
  passes the real Chrome/WebGPU ten-filter interaction test.
  libm's complete original notice is checksum-pinned because automatic detection
  misidentifies its combined license document. No licensing check is bypassed.

## Review images

Generated images are intentionally ignored by Git:

- GTK: `artifacts/ui/adjustments-gtk/` — all ten, grid, live stats.
- Web: `artifacts/ui/adjustments-web/` — all ten, grid, dark/light stats.
- Android: `artifacts/ui/adjustments-android/1789017808707/` — all ten, grid,
  dark/light stats, captured on the tablet emulator.

Color Balance shows Shadows/Midtones/Highlights; Levels uses Input/Output.
GTK/web keep compact panel spacing. Android scrolls when the property list
exceeds panel height. An emulator test does not prove physical mobile 120Hz.
