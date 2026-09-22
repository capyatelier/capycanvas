# Windows feature integration and brush measurements — 2026-09-22

Upstream feature reference: `e2979a27`, including the 13 commits after `2ac130d7`. Final integration includes `a21a3f03` (Android buffered navigation and the Apple feature port/measurements), preserving every supported host’s panel capabilities.

## Changes

Windows now exposes the shared Brush, Sculpt, Tools and Filter Type panels and the Drawing Brush/Sculpt title-bar commands. The built-in Sketch workspace migration uses the same three-column Brush/Sculpt and Filters drawers as GTK, Android and Web; edited workspaces retain their layouts and working values.

The native controls project separate media categories and subtools, retain tool/size selections, and show the shared filter selection. Filters replace the selected filter in place, preserve the drawing target, and support cancellation. Paper has a color swatch and selected-color bucket action, a contrasting thumbnail glyph, and the shared deletion rules, including an empty layer stack. Drawer scrolling uses WinUI ScrollView for touch and pen.

Layer rows add touch/pen swipe-to-delete. Horizontal movement before a hold can reveal Delete; reversing or clicking outside closes it. Held row/grip reordering retains the existing host timing, capture, cancellation and shared Rust drop/history policy. Mouse row dragging remains immediate and never becomes a swipe. See the [required convention](../ui/drag-and-reorder.md).

The Windows build also includes the upstream shared trajectory predictor, cursor modes and drawing-time cursor visibility, indirect-pen hover identity, G-Pen release smoothing, integrated contact materials, compacted stamp submission, sparse constant-backdrop composition and mip reductions. These run through the existing Windows D3D12 host; renderer algorithms are shared Rust/WGSL.

## Validation

- Release WinUI build and strict Windows Clippy pass. The benchmark example has no Clippy warnings; the renderer library retains its existing 49 warnings.
- Shared unit suites: core 106, engine 101, host 29, UI 521, Windows 120, workspace 88 pass. Hardware/manual tests are separately selected rather than counted as ordinary passes.
- Native accessible-command and OS-delivered mouse/touch/pen journeys pass Brush/Sculpt/Eraser category and tool retention, light/dark drawers, filter replacement/reopen/cancel, drawing target retention, paper color, deleting Paper and the last layer, empty canvas, Undo and normal shutdown. The final native journey also verifies all ten cursor choices, painting-cursor visibility, and native/shared prediction preference dependencies.
- Native mouse, touch and pen layer regressions pass ordinary clicks, hold/release menus, immediate grips, row/child dragging, cancellation, one-step Undo/Redo, floating and drawer instances, source removal, mask/selection children, group drops, edge scrolling, minimize cancellation and rename ownership. The fixture explicitly expands today’s initial retained column and selects individual drawers for its drawer checks.
- Native prediction switching, preference persistence and mouse/touch/pen routing pass; runtime counters record 376 Windows prediction frames and 358 shared-engine prediction frames. The fixture refreshes physical drawing coordinates after Preferences closes.
- Chrome/WebGPU Brush/Sculpt and Filters journeys pass, including mouse/touch/pen selection, pen hold/drag cancellation, touch/pen swipes, scrolling and undo after an empty layer stack.
- Seven contact-material tests and three captured-Wacom release tests pass on hardware D3D12. The default hardware backend run also passes all ten. The release fixture now shares one GPU device across trace variants, preserving every assertion while avoiding repeated D3D12 device compilation.
- The D3D12 native project/color/import/export/history round trip passes. Three D3D12 cursor silhouette/native-scale/rotation tests pass on the final integrated source.
- D3D12 sparse constant-backdrop composition matches tiled Float32 composition; compacted dry stamps preserve order; mip reductions match an independent Float64 area reference.

Native screenshots were inspected against the Web reference for column widths, category order, icons, previews, selected states, theme colors and paper controls. WinUI fonts and native control furniture differ from browser controls; this is not a claim of pixel-identical application chrome. GTK and Android applications were not run on this Windows machine; their shared models/renderer and the current Web implementation supply the reference.

### Input qualification

The guarded native mouse/touch/pen journey passes category/subtool selection, filter replacement, Paper color, pen/touch swipe deletion and reversal, Escape/outside cancellation, the empty stack, and Undo. Windows required administrator access to pause `Wacom_TouchUser`; the user authorized that pause. The helper resumed successfully after the test. The expanded journey on the final integrated build subsequently passed with the helper running normally. Input is delivered through Windows synthetic pointer APIs, with foreground-process and hit-window ownership checked before injection. Physical digitizer timing/latency is not measured by synthetic input. Final native pointer captures: `artifacts/windows/new-features/5da2e7510f67414eaba3b9417381f992`.

### Existing Web history differences

All 22 optimized media produce visible strokes. The upstream Web rendering path fails its strict screenshot-history tolerance for Antique Pen, Realistic Pen, Wet Ink, Blotty Ink, Brushed Ink and Eraser on this driver. Antique Pen changes at most 6/255 in a small number of edge channels after Redo. These are retained as failures, not converted into passes. The browser fixture now waits for committed tile capture, collects failures, and supplies real underpainting for the Eraser case. The Windows changes do not alter browser renderer production code.

## 1000 px brush / 61 MP-class measurement

Hardware: Intel Core i7-1255U, Intel Iris Xe integrated GPU, driver 32.0.101.6737, 16 GB RAM, Windows 11 Home build 26200, AC power. Release build, explicit hardware D3D12.

The reproducible [brush_frames example](../../crates/layer-render-wgpu/examples/brush_frames.rs) uses a 9504 × 6336 canvas (60,217,344 pixels, the repository’s 61 MP camera workload), a 1000 px brush, a 1600 × 1000 fitted viewport, and an opaque white canvas background. It feeds two timestamped 240 Hz samples into each generation, with a 16 ms shared prediction horizon. Each stroke follows the same elliptical path at full pressure and zero tilt. Eraser receives underpainting first.

For every preset, shader/material preparation and a 64-generation warmup finish before timing. Three repetitions contain 240 generations each, including pen-down and pen-up. Each measured generation includes input processing, brush rendering, composition, managed FP16 offscreen presentation and waiting for the GPU to finish. Initialization, PNG readback, file writing and history checks are outside the timer. The rate is total measured generations divided by total completed time. It is not monitor refresh rate, swapchain FPS or physical input latency. It is a blank drawing workload, not a multilayer photograph/filter-stack benchmark.

Every repetition checks exact native raster Undo/Redo using stored tile digests. The first completed stroke is captured for comparison with the same Vulkan workload. Measurements were taken before the final `a21a3f03` integration; its renderer change renames the target-retention API, while this benchmark keeps default nonretained presentation. Brush and compositor algorithms are unchanged.

| Brush | Completed generations/s | Median generation (ms) | P95 generation (ms) |
| --- | ---: | ---: | ---: |
| Pencil | 82.08 | 10.67 | 18.39 |
| Pointy Pencil | 83.93 | 10.89 | 16.18 |
| Shading Pencil | 116.76 | 7.93 | 12.17 |
| Charcoal | 95.45 | 10.03 | 13.56 |
| G-Pen | 82.82 | 10.30 | 18.29 |
| Rough G-Pen | 75.56 | 11.72 | 20.16 |
| Calligraphy Pen | 125.91 | 7.49 | 10.26 |
| Antique Pen | 82.23 | 11.28 | 17.63 |
| Realistic Pen | 66.61 | 13.81 | 22.67 |
| Wet Ink | 63.09 | 14.65 | 22.17 |
| Blotty Ink | 50.46 | 18.28 | 27.71 |
| Brushed Ink | 53.19 | 17.02 | 28.33 |
| Eraser | 74.36 | 12.40 | 17.83 |
| Paintbrush | 51.93 | 17.61 | 28.66 |
| Airbrush | 87.71 | 10.46 | 14.21 |
| Chalk | 76.75 | 12.23 | 16.49 |
| Marker | 44.65 | 20.27 | 32.58 |
| Dual Texture | 56.93 | 16.37 | 23.98 |
| Textured Flat | 49.70 | 18.36 | 30.25 |
| Dry Scumble | 51.64 | 17.97 | 27.27 |
| Pastel Block | 48.82 | 18.64 | 29.24 |
| Transparent Glaze | 49.62 | 18.32 | 30.58 |

All 15,840 timed D3D12 generations and 5,280 Vulkan comparison generations completed. All 88 measured strokes passed exact native Undo/Redo. Across all 22 final 1600 × 1000 captures, D3D12 and Vulkan differ by at most 1/255 per RGB channel; no channel exceeds the 3/255 visual tolerance. The largest whole-image mean absolute channel difference is 0.001643/255 (Pastel Block). This comparison uses the same shared renderer with different hardware backends, not GTK/Android application screenshots.

[Machine-readable measurements, repeat rates and comparison metrics](windows-brush-results-20260922.json).

## Reproduction

```powershell
./apps/layer-windows/scripts/build.ps1 -Configuration Release -SkipRestore
pwsh -NoProfile -Sta -File apps/layer-windows/scripts/exercise-new-features.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe -InputMode Automation
# Pointer mode requires the owned review to have foreground input.
pwsh -NoProfile -Sta -File apps/layer-windows/scripts/exercise-new-features.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe -InputMode Pointer
cargo build --locked --release -p layer-render-wgpu --example brush_frames
target/release/examples/brush_frames.exe --adapters
$env:CAPY_BRUSH_CAPTURES = 'artifacts/windows/brush-captures'
target/release/examples/brush_frames.exe artifacts/windows/brush-frames.csv dx12 240 3
```

Evidence directory: `artifacts/windows/september`. Native command captures: `artifacts/windows/new-features/2e8eb88093f045c5b804d0ae8119e546`. Browser drawer captures: `artifacts/brush-sculpt-web` and `artifacts/filters-web`. Raw captures and per-frame CSV files remain local test artifacts.
