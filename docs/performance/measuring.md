# Measuring performance

[Performance targets](../PERFORMANCE_TARGETS.md)

## Rules

**What counts as a frame.** Two rates are used, depending on who paces the
frames:

- Brush strokes use **completed canvas updates per second**, as reported by
  `completed_updates_per_s` in `tools/performance/android-brush-report.py`. This
  counts GPU-completed, nonempty canvas updates inside the input window. Android
  draws ink into the front buffer, so this rate can exceed the display's refresh
  rate.
- Navigation, drags, sliders, animations and other display-paced motion use
  **presented frames per second**: SurfaceFlinger actual-present times,
  `FrameMetrics`, or the host's presentation timestamps.
  - Renderer submissions per second are recorded where no presentation data
    exists. They are marked "renderer", because a submission is not a displayed
    frame.

**When a target is met:**

1. The rate is at least the target.
   - Display-paced motion on a panel whose refresh equals the target only needs
     95% of it: 57, 85.5 and 114 fps. This matches the workspace-motion floor
     `LAYER_MOTION_MIN_HZ`.
2. The 99th-percentile interval between frames or updates is at most two frame
   budgets: 33.3, 22.2 and 16.7 ms.

**How to measure "sustained".**

- Use at least three gestures of 5–10 s each, in a release or benchmark build.
- Warm up first: pipelines compiled, and one priming gesture undone.
- Run on the reference device at thermal status 0, using its default display
  settings and the default panel glass.

**Only motion counts.** A frame where nothing moves may take longer. This includes
the frame after a drag is released and a frame waiting on a click. The exception
is a still frame that delays the start of the next motion. Start and release
latencies have their own limits under [Responsiveness](responsiveness.md).

**The target document.**

- It is the tier's photo as the bottom layer plus one empty paint layer at Fit
  zoom, with the default workspace.
- Rows that name a layer count use that many visible layers instead.
- The photos are the Android brush benchmark's 9504 × 6336 Sony JPEG and exact 3:2
  downscales of it.
- The camera is sold as 61 MP; its image is 60.2 MP.

**Brush sizes.**

- Sizes are document-pixel diameters at pressure 1. The UI allows 0.5–2048 px.
- "Guaranteed" means every diameter up to that size meets the tier rate, with the
  preset's default spacing and settings.
- Above the guaranteed size a brush may run slower. It must still draw correctly,
  keep every input sample, and never lose the GPU device.

**Soft targets.** Full-screen filters and adjustments are soft targets. A filter
may run below its target only when two things are both true:

- A calculation shows that its FLOPs or memory traffic at the tier's canvas size
  exceed what the reference hardware can deliver in one frame budget.
- No valid approximation exists.

Record that arithmetic next to the row.

Previewing at display resolution while a control moves, then committing at full
resolution, is a valid approximation. Band-limited or tiled evaluation of the
visible region is one too. A filter cannot claim the waiver while an
approximation like these would reach the target.

For scale, take a single full-resolution pass that reads and writes 8-byte
(RGBA16F) pixels. It moves 16 bytes per pixel:

| Tier | Canvas | Bytes per pass | Peak bandwidth | Passes/s at peak |
| --- | --- | --- | --- | --- |
| Low | 12 MP | 0.19 GB | 14.4 GB/s | 74 |
| Mid | 24 MP | 0.38 GB | 17.1 GB/s | 45 |
| Top | 60.2 MP | 0.96 GB | 67.2 GB/s | 70 |

- Real sustained bandwidth is lower than peak, and the renderer's RGBA32Float
  pages double the traffic.
- So per-frame full-resolution filtering cannot meet the mid or top tier target
  even for a pointwise adjustment. On the low tier, a pointwise adjustment fits
  only as a single pass.
- Filter previews must therefore work at display resolution during motion.

## How to measure

Record every result with its device, build (commit and profile), canvas, brush,
size and date. Keep raw data in ignored `artifacts/` directories, and put the
headline number with its source in the table.

**Brushes (Android).**

- Reserve the tier's tablet and run device commands through
  `tools/devices/devices.py run` ([devices](../development/devices.md)). Build
  the release benchmark as the
  [Android guide](../development/android.md#brush-workload-benchmark) describes.
- Push the tier photo:
  - Top tier: the 9504 × 6336 original, at `/data/local/tmp/capy-brush-photo.jpg`.
  - Mid and low tiers: its 6000 × 4000 or 4248 × 2832 downscale, to any
    `/data/local/tmp/*.jpg`.
- Then run:

```bash
python3 tools/performance/android-brush-benchmark.py OUT --serial "$CAPY_ANDROID_SERIAL" \
  --package art.capycanvas.brushbench --photo /data/local/tmp/capy-tier-24mp.jpg \
  --presets 1,5,3 --size 2048 --prefix mid
python3 tools/performance/android-brush-report.py OUT --package art.capycanvas.brushbench
```

The runner defaults to the dry presets. Pass `--presets` to include the wet,
smudge and Liquify presets. The photo opens as a Perceptual document;
`--blending linear` measures it in linear light. The benchmark draws a 200 Hz
stylus ellipse at Fit zoom, three 10 s strokes, with the default 16 ms
prediction.

**Navigation (Android).**

- `AndroidViewportBenchmarkTest` measures presented pan and pinch rates
  (`-e viewportBenchmark true -e motion pan|pinch`).
- The brush runner's `-e mode pinch` does the same on the photo.

See [Android development](../development/android.md#benchmarks).

**Transforms, placement, selections and the canvas bar (Android).**

- Run `AndroidCanvasBarBenchmarkTest` with `-e width` and `-e height` set to the
  tier canvas.
- Its "Hz" is renderer submissions. Its JSON also records `display_hz` and the UI
  `FrameMetrics`.

**Web on a tablet.** In your own tablet Chrome tab ([devices](../development/devices.md)),
`tools/performance/web-pen.mjs` draws timed strokes over DevTools
(`LAYER_DEVICE_CDP`, `LAYER_WEB_URL`) and reports submissions per second, frame
CPU time and event-to-submission latency. `--os-input` replays a 200 Hz stylus
through Android's input dispatcher instead, using a helper built from
`tools/performance/AndroidPenMotion.java`; never pool the two kinds of run.
`tools/performance/web-refresh.mjs` reloads the tab and records startup
milestones. Both scripts list their options in their headers.

**Workspace motion (desktop and Web).** Use
`tools/performance/workspace-motion.sh` with `LAYER_MOTION_MIN_HZ` at 95% of the
tier rate.

**Brushes (Windows).** Build `cargo build --locked --release -p layer-render-wgpu
--example brush_frames`, list adapters with `brush_frames.exe --adapters`, then
run `brush_frames.exe OUT.csv dx12 240 3 [preset-ids]`. It draws a 1000 px brush
on a 9504 × 6336 canvas and reports completed generations, not displayed frames.

**Pen latency (Windows).** Against a Release build, run
`tools/performance/windows-pen-latency.ps1 -Executable <exe> -Project <.capy>
-OutputDirectory <dir>`, then `node tools/performance/windows-pen-report.mjs <dir>`.
It paces a pen circle at up to 240 Hz and records actual injection timestamps;
a delayed sample never triggers a catch-up burst. The report matches inputs to
DXGI frame statistics, so it reports software input-to-display time, not
input-to-photon. Per-window traces use `latency-<pid>-<window>` names. PresentMon
needs administrator rights.

**Desktop GPUs.**

- `layer-bench` and the renderer examples give completed-work timings, described
  in [GPU raster benchmarks](../development/gpu-raster-benchmarks.md).
- The Apple hosts record presentation, described in
  [Apple performance](../../apps/layer-apple/PERFORMANCE.md).
- Offscreen numbers exclude input delivery and presentation. They do not replace
  device measurements.
