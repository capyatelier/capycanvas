# Huion Vulkan front-buffer experiment — 2026-09-20

Historical experiment measurements. The subsequently authorized production path
requires front buffering for all Android SDR/HDR output and removes the opt-in
flag and FIFO fallback. See [production qualification](android-front-buffer-production-2026-09-20.md).


Local experiment based on `39e9cba3537e604705755ab4ac79dc870bc96884`, in
`/tmp/capy-gpen-huion`. The user initially limited this snapshot to local benchmarking;
the production follow-up was subsequently authorized for merge. Test package: `art.capycanvas.frontbuffer`,
launcher name **Capy Canvas Front Buffer**. The ordinary app was buffered during these measurements.

## Relationship to Android's stylus guidance

This implements the front-buffer principle described by
[Android's advanced stylus guide](https://developer.android.com/develop/ui/views/touch-and-input/stylus-input/advanced-stylus-features):
update retained display pixels on input, without waiting for an application
vsync callback or rotating through buffered canvas images. The implementation
uses Vulkan `VK_KHR_shared_presentable_image`, `SHARED_DEMAND_REFRESH`, rather
than AndroidX's OpenGL renderer. The Huion actually selects the shared mode;
the benchmark asserts the mode and retained-target flag.

It is **not the complete two-layer AndroidX architecture**. The canvas itself
stays in a shared image; there is no temporary ink layer transferred to a
buffered layer on pen-up. SDR pan/zoom and other full-surface changes currently
redraw that shared image and can tear. HDR and unsupported devices use FIFO.
These are deliberate experiment limits, not a production rollout.

## Implementation

- Acquire the one shared image once, preserve initialized contents in wgpu-core,
  and use `SHARED_PRESENT_KHR` layout throughout retained rendering.
- Use explicit present fences for bounded semaphore reuse. Poll completion;
  keep accepting input while an earlier GPU update is unfinished. No per-frame
  device/queue-idle wait on the canvas owner.
- Reuse the existing brush, viewport, color, Navigator, and cursor shaders.
  Keep separate damage regions for paint, cursor, and Navigator; merge overlaps.
  Apply document damage to the Navigator too. Each region has both a scissor and
  a narrow Vulkan render area, so a tile GPU need not load/store a whole surface.
- Submit immediately from the canvas owner after input, with one queued callback
  and a bounded retry delay when the GPU is occupied. Buffered mode retains
  Choreographer and the independently measured presentation-feedback pacing.
- Gate the experiment with `-PcapyFrontBuffer`. The normal build defaults false.
  The benchmark can compare both modes in the same APK using `-e frontBuffer`.

The Huion's Vulkan loader reports swapchain-maintenance support through the
core Vulkan 1.1 feature query but reports false through its KHR alias. The
Android-specific capability probe now queries that loader feature through the
core entry point. Extension presence alone is not treated as feature support.

## Matched Android-input results

Huion KP1202, Android 16, Mali-G57 MC2, 2400×1600 at 90 Hz. Three five-second
strokes per case after warmup; G-Pen, prediction on, platform prediction off,
200 Hz stylus `MotionEvent`s through Android's input dispatcher. Canvas fitted
in the same isolated workspace; variable pressure. Fast means 4× path speed.
Each mode has the same brush implementation and buffered optimizations.

GPU timestamps bracket **viewport presentation work only**. They exclude brush
rendering, SurfaceFlinger, scanout, and panel response. They are not pen-to-photon
measurements. CPU figures are canvas-owner thread CPU per submitted update.

| Workload | FIFO GPU median / p99 | Front GPU median / p99 | Median speedup | FIFO → front updates/s |
| --- | --- | --- | --- | --- |
| 1K, 18 px | 4.462 / 5.498 ms | 0.589 / 1.538 ms | 7.58× | 90.1 → 303.8 |
| 2K, 18 px, fast | 4.405 / 5.836 ms | 0.476 / 1.164 ms | 9.25× | 89.9 → 227.8 |
| 1K, 512 px | 5.298 / 7.119 ms | 2.020 / 4.192 ms | 2.62× | 90.1 → 129.7 |

| Workload | FIFO CPU median / p99 | Front CPU median / p99 | FIFO → front CPU-core occupancy |
| --- | --- | --- | --- |
| 1K, 18 px | 2.019 / 4.489 ms | 1.464 / 2.349 ms | 20.9% → 47.1% |
| 2K, 18 px, fast | 2.152 / 4.574 ms | 1.451 / 2.744 ms | 21.8% → 38.1% |
| 1K, 512 px | 2.722 / 5.154 ms | 1.668 / 3.905 ms | 26.1% → 27.9% |

The cleaned final APK repeats 1K/18 px at **0.597 / 1.622 ms** GPU median/p99
and 1.358 / 2.295 ms owner CPU, 308.9 submissions/s, 45.2% of one CPU core.
All three strokes advance the document revision. Data: `front-final2-os-1k18`.

The extra updates include predicted preview updates between input events.
CPU time per update decreases; rendering more often increases total CPU use.
Updates/s are submissions, not distinct full-screen displayed frames.

Raw data: `artifacts/pen-latency/{control6,front6}-os-*` and
`front6-os-matrix-summary.json`. The normal 1K FIFO control was recorded with
frontbuffer5; its buffered rendering code is unchanged in frontbuffer6. The
other four matrix runs use frontbuffer6 with a mode override. Earlier 240 Hz
canvas-owner injection results are retained in `front3-matrix-summary.json`;
they bypass Android input dispatch and should not be substituted for this table.

## Why ordinary workspace drawing initially stayed slow

A five-second OS stroke in the default 2048×1536 workspace exposed work missed
by the early synthetic tests. Combining a distant Navigator with the stroke
made one ~738,000-pixel damage rectangle: 9.155 ms median / 15.124 ms p99.
Splitting rectangles reduced damage to ~74,000 pixels, but still cost 8.544 /
15.515 ms because every update resampled the entire zoomed-out Navigator.
The final algorithm also retains unchanged Navigator pixels. Those two fixes
change presentation work; they do not change the brush algorithm or materials.

The final normal-workspace replay measures **0.744 / 3.345 ms** viewport GPU
median/p99, down from 9.155 / 15.124 ms (12.3× at the median, 78% lower p99).
Median damaged area is 24,510 pixels, versus 737,672 originally. Both old and
new replays use the same 18 px brush, 2048×1536 drawing at 56% zoom, pressure 1,
200 Hz circular input through Android, with the Navigator visible. This is a
single five-second trace per version, not the three-repeat matrix above.

Final hover: **0.199 / 0.583 ms** GPU median/p99, 1,088 median damaged pixels,
197.9 submissions/s for 200 Hz input. Owner input arrival to render is
0.043 / 0.283 ms median/p99; sample to QueuePresent call is 3.541 / 7.793 ms.
Drawing sample to QueuePresent is 6.574 / 26.845 ms: CPU/input scheduling tails
remain even when GPU work is short. Polled GPU-completion callbacks are upper
bounds and must not be reported as exact completion or display latency.
Neither final trace submits another canvas frame more than one second after
input ends. Files: `hover-front-final-{stroke,hover}-front-latency.json`, their
Perfetto traces and action markers.

Hardware-composition snapshots taken **during** both final replays show DEVICE
for canvas and UI; the later idle snapshot shows CLIENT. The driver can change
composition strategy, and a snapshot is not proof for every frame.

The first frontbuffer5 cold-start trace happened before shader warmup completed:
zero damaged pixels and no viewport timestamps. It is explicitly excluded.

## Bandwidth bounds and remaining latency

A 2400×1600 RGBA8 screen is 15.36 MB. A full-screen texture copy reads and writes
at least 30.72 MB per update, excluding source shading and compositor traffic.
The [Genio 720 platform](https://genio.mediatek.com/doc/android/hw/mt8391-soc.html)
[specification](https://www.mediatek.com/hubfs/MediaTek%20Assets/Pdfs/FactSheet%20Assets/Pdf/Genio%20720%20-%20Factsheet%20(30th%20Jan).pdf)
lists a 32-bit LPDDR4X-4266 or LPDDR5(X)-6400 interface: nominal 17.064 or
25.6 GB/s. The tablet's actual DRAM configuration/rate was not established.

Those alternatives give an optimistic, uncompressed full-copy bound of
**1.80 or 1.20 ms**, before any GPU dispatch, shading, contention, or scanout.
Front-buffer damage updates remove that mandatory full-screen copy entirely;
they do not somehow execute it faster than the bandwidth bound.

The final drawing trace's median 24,510 pixels require only 196,080 bytes for
an ideal RGBA8 attachment load+store: a **0.0077–0.0115 ms** bandwidth floor.
Hover's 1,088 pixels imply 8,704 bytes, or **0.00034–0.00051 ms**. Actual traffic
also includes source reads, tile rounding, metadata, and the other display
layers. These are optimistic bounds, not measured bus traffic.

For tiny damage, render-pass/driver/shader overhead dominates the memory-only
lower bound. This experiment therefore does **not prove theoretical maximum
bandwidth or minimum physical pen-to-photon latency**. The 90 Hz panel takes
11.11 ms for a scan cycle, and Android still controls composition. SurfaceFlinger
snapshots have shown both DEVICE and CLIENT composition, so stable direct
hardware scanout must not be claimed. Shared-image frame IDs cannot identify
which input sample was visible at a given scanout position. An optical test is
needed for the remaining pen-to-photon interval.

## Validation and reproduction

Pixel-equivalence tests compare retained output to fresh full redraws across
paint, preview replacement, moving/hidden Navigator, moving/hidden cursor,
zoom and all four surface rotations. HDR uses F16/PQ; shared SDR capture uses
a composited screenshot because PixelCopy returns ERROR_SOURCE_NO_DATA for the
Huion's acquired shared image. This capture limitation is not silently skipped.

The large existing `exactSnapshotsSurviveFilesGpuReplacementAndRecovery` test
passes its save/export, GPU-loss replacement, undo/redo, and surface-recreation
stages, then fails an assumption of one recovery file (it finds two). The same
failure is reproduced in FIFO mode. Its later recovery-controller stages are
not claimed to pass. A focused shared-surface lifecycle test passes background/resume, recreation,
GPU loss, and undo/redo. Final HDR negotiation passes after explicitly draining
Compose placement and the native render queue before capture; an earlier
unsynchronized capture read the Navigator background before its image arrived.

These commands reproduce the archived experiment snapshot only. The current
production build has no `capyFrontBuffer` flag or mode override; use the production
qualification instructions above. Archived commands:

```sh
ANDROID_HOME=/home/babymastodon/Android/Sdk \
JAVA_HOME=/home/babymastodon/Applications/android-studio/jbr \
CARGO_TARGET_DIR=/home/babymastodon/code/capycanvas3/target \
./apps/layer-android/gradlew -p apps/layer-android \
  :app:assembleBenchmark :app:assembleBenchmarkAndroidTest \
  -PcapyAbi=arm64-v8a -PcapyBenchmark -PcapyFrontBuffer \
  -PcapyApplicationId=art.capycanvas.frontbuffer \
  '-PcapyAppLabel=Capy Canvas Front Buffer'
```

Install the two APKs from `app/build/outputs/apk/{benchmark,androidTest/benchmark}`.
Wait three seconds after install (the Huion launcher force-stops updated apps),
then run:

```sh
adb -s G7DL2S300241 shell am instrument -w \
  -e class art.capycanvas.AndroidViewportBenchmarkTest \
  -e viewportBenchmark true -e osInput true -e intervalMs 5 \
  -e label local-front -e repeats 3 \
  art.capycanvas.frontbuffer.test/androidx.test.runner.AndroidJUnitRunner
```

Use `-e frontBuffer false` for the control; `-e canvasSize 2048 -e speed 4` for
fast strokes; `-e brushSize 512` for large brushes. Pull the label's four JSON
files from `/sdcard/Android/data/art.capycanvas.frontbuffer/files/viewport-benchmark/`
and summarize with `python3 tools/performance/android-viewport-report.py DIRECTORY`.
The final benchmark also asserts that every measured stroke commits paint.

The slower full-screen transfer probe in the ordinary app is replaced with the
previously validated buffered4 rollback APK. On launch, its offered unsaved
recovery was discarded in accordance with the user's earlier clean-reset
instruction; no new workspace reset was performed.
