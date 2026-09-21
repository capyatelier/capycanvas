# Android front-buffer production qualification

Android now requires Vulkan shared-demand presentation for both SDR and HDR.
There is no build flag or buffered fallback. A driver without shared presentation,
shared-image color-attachment usage, or swapchain-maintenance present fences
reports a clear unsupported-driver canvas error. This is an explicit product
requirement; Vulkan support alone does not imply compatibility.

## Rendering and ownership

One retained swapchain image receives the existing canvas, cursor and Navigator
shaders. The first frame, view changes, output-color changes and surface recreation
redraw the entire target. Subsequent updates redraw intersecting damage regions,
including old/new cursor bounds and document damage projected into each Navigator.
Full redraws and damaged regions share one draw loop. Distinct partial regions
use separate texture views/render passes, so tile GPUs limit
attachment load/store traffic as well as fragment shading. No extra full-screen
copy or display cache is used.

The image is acquired once and remains in `SHARED_PRESENT_KHR`. Subsequent
access dependencies use memory barriers, with no image-layout operations. Four bounded
presentation slots retire their binary semaphores with Vulkan present fences.
A single acquisition semaphore retains its one-time wait state. Resource creation
cleans up partial allocations; teardown drains GPU and pending presentation work.
Navigator placements survive document changes because they belong to the window.
Pass timestamps are attached only to the first/last damaged regions; interior
passes omit timestamp writes entirely. Normal drawing never waits for device idle. Completion polling admits only one
GPU update at a time while the owner continues accepting input; a new input
expedites a pending retry. Canvas updates do not wait for Choreographer, which
still schedules independent workspace UI animation. Startup retries are bounded
to the display interval and settled canvases stop scheduling.

This is a whole-canvas retained front buffer, not AndroidX's transient front-ink
layer with a pen-up handoff. Display reads can overlap writes, including full-view
updates. Tearing is a property of this latency choice. GPU timestamps measure
viewport rendering, not physical pen-to-photon latency; submission counts are
not visible-frame counts, and presentation fences only retire resources.

## Qualification

Base: `origin/main` at `5b9de3f3`, including the unified tile executor and latest
G-Pen release-pressure diagnostics. Devices: Huion KP1202 / Mali-G57 MC2 /
Android 16 / 2400×1600 90 Hz, and Wacom DTHA140 / Adreno 735 / Android 15 /
2880×1800 120 Hz. Both expose the required extensions and core Vulkan 1.1 feature
query; the KHR feature-query alias reports false on both.

Raw build logs, tests, captures and replay JSON live in
`artifacts/android-front-buffer-production-2026-09-20/` in the main workspace
(a copy of `artifacts/front-production/` in the production worktree). The original experiment
and its matching FIFO baseline are preserved in
`artifacts/huion-front-buffer-2026-09-20/` in the main workspace.

The device HDR test reads the actual retained swapchain image and decodes PQ
relative to 203-nit SDR white. Android PixelCopy cannot read this acquired shared
image. Readback is restricted to profiling sessions; normal release surfaces do
not request copy usage. Compositor screenshots remain additional visual evidence,
not an HDR luminance measurement.

Host qualification passed: exact retained/full redraw pixels with enabled GPU
timestamps, moving cursor, Navigator, previews and rotations; nine viewport color
and HDR tests (one licensed CMYK fixture omitted); Linux and WebAssembly app
compilation. The host color suite initially hit a native driver crash with
concurrent tests; its serial run passed, including after draw-loop unification.
Device suites run against isolated packages and preserve the normal apps' data.
Recovery assertions account for one recovery copy per modified drawing tab and
wait for history command admission after asynchronous presentation.

Khronos Android validation layer `vulkan-sdk-1.4.357.0` was loaded on the Huion
for HDR/readback, rotation, surface lifecycle and GPU-recovery testing. Replacing
same-layout image barriers with memory barriers removes
`UNASSIGNED-barrier-shared-presentable`; no subsequent image-layout operation is
needed. Remaining log categories are recorded rather than hidden:

- `VUID-StandaloneSpirv-None-10684` is the existing Naga array-layout issue
  [wgpu #7696](https://github.com/gfx-rs/wgpu/issues/7696), already filtered by the
  upstream wgpu debug callback but still printed by Android's injected layer.
- `SYNC-HAZARD-WRITE-AFTER-PRESENT` and `READ-AFTER-PRESENT` arise because this
  validator's [presentation tracker](https://github.com/KhronosGroup/Vulkan-ValidationLayers/blob/vulkan-sdk-1.4.357.0/layers/sync/sync_submit.cpp)
  treats every present as an exclusive image access until reacquisition, including
  shared swapchains. The [shared-present specification](https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html)
  explicitly permits concurrent application/display access and repeated demand
  presents without reacquisition. The app retains producer memory dependencies,
  queue completion gating and present fences for semaphore retirement. These
  messages do not constitute a clean synchronization-validation pass; their
  applicability was checked against the exact validator source and WSI rules.

The Wacom completed earlier functional checks and benchmark runs. Its final
validation run was stopped at the user's request; remaining qualification and
release deployment are Huion-only. Per-app debug-layer settings are restored by
the validation runner even when instrumentation is stopped.

## Drawing measurements

Three five-second G-Pen strokes delivered through Android at 200 Hz; stroke
prediction enabled, platform prediction disabled, every stroke required to
advance the document revision. Final runs retain a visible, updating Navigator.
Huion rows use the final release-native build (`huion-release-*`), including the
shared-image memory barriers. Wacom rows are earlier evidence, before that final
barrier change; no further Wacom work was performed after the user stopped it.
The controlled early run exposed an existing document-switch placement bug;
those earlier blank-Navigator timings must not be presented as the full UI cost.

| Device / workload | Viewport GPU median / p99 (ms) | Owner CPU median / p99 (ms) | Submissions/s |
| --- | --- | --- | --- |
| Huion 1K, 18 px | 1.071 / 2.902 | 1.442 / 2.509 | 268.9 |
| Huion 2K, 18 px, 4× speed | 0.936 / 2.011 | 1.494 / 2.588 | 213.2 |
| Huion 1K, 512 px | 3.569 / 7.213 | 1.723 / 4.376 | 105.2 |
| Wacom 1K, 18 px | 0.930 / 1.739 | 2.005 / 4.187 | 247.5 |
| Wacom 2K, 18 px, 4× speed | 0.718 / 1.231 | 2.099 / 4.432 | 171.1 |
| Wacom 1K, 512 px | 3.474 / 7.159 | 2.960 / 6.983 | 126.0 |

For the original controlled benchmark without a Navigator, the production branch
measured Huion 0.598 / 1.664 ms versus the archived FIFO 4.462 / 5.498 ms: 7.5×
lower median viewport GPU cost. The final full-UI numbers above include the
Navigator that comparison omitted. Earlier default-workspace front-buffer runs
with the Navigator measured 0.744 / 3.345 ms at 2048×1536 and 56% zoom; workload
and zoom differ, so this is supporting evidence, not an exact paired speedup.

This removes the whole-screen transfer rather than proving a DRAM limit has been
reached. A 2400×1600 RGBA8 copy reads/writes 30.72 MB. Nominal Genio 720 bandwidth
of 17.064–25.6 GB/s gives a 1.8–1.2 ms ideal bound for that copy; actual tablet
memory configuration is unverified. Small retained regions cost much less data
traffic, leaving render-pass, shader and driver overhead. The display still scans
at 90/120 Hz, and software timings do not establish physical pen-to-photon latency.
See the [original bandwidth analysis](android-front-buffer-results-2026-09-20.md).

## Huion release deployment

The normal `art.capycanvas` release app was updated in place on the Huion and
opened to a working canvas. Its unsaved-drawing recovery offer was discarded
under the user's earlier clean-reset instruction. This was an ordinary recovery
offer, not the workspace-ownership failure from the earlier deployment.
The Wacom normal app was not updated.

Signed APK SHA-256:
`b17b7ebfae417aca723b2c95517877bfa08747170550cd3549006f32601461c2`.
It is archived as `capycanvas.apk` with the evidence. The app was built with
`assembleRelease -PcapyAbi=arm64-v8a` and signed with the existing local development
certificate. The final source differs only by documentation and one whitespace
cleanup in an upstream shader comment.

A ten-second 200 Hz synthetic hover replay on the deployed normal release
(2048×1536 document, 56% zoom, 18 px cursor) measured:

| Metric | Median / p99 |
| --- | --- |
| Viewport GPU | 0.188 / 0.591 ms |
| Owner input acceptance to render callback | 0.032 / 0.206 ms |
| OS sample to `QueuePresentKHR` call | 3.157 / 8.084 ms |
| Damaged area | 1,092 / 1,296 pixels |

The replay produced 198 submissions/s and zero submissions more than one second
after hover ended. Navigator was collapsed in this normal workspace; the drawing
benchmarks above explicitly kept it visible. The trace is
`capy-hover-settled-release.perfetto-trace`, summarized by
`hover-settled-release-front-latency.json`. These remain software timings, not a
measurement of display visibility. The earlier `production-release` trace was
captured while the recovery dialog was open and is excluded from results.

## Reproduction

Build the normal app with the existing Android build instructions. For isolated
qualification, assemble `Benchmark` and `BenchmarkAndroidTest` with
`-PcapyBenchmark -PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.frontproduction`.
Run `AndroidRasterTest#frontBufferSurfaceLifecycle` and
`AndroidRasterTest#hdrDisplayNegotiation` (the latter takes `hdrFile` and optionally
`requireHdr=true`). Run `AndroidViewportBenchmarkTest` with `viewportBenchmark=true`,
`osInput=true`, `canvasSize=1024`, `brushSize=18`, `intervalMs=5`, `durationMs=5000`,
`repeats=3` and a unique `label`. Pull the matching files from the package's external
`files/viewport-benchmark` directory and summarize with
`tools/performance/android-viewport-report.py DIRECTORY`.
