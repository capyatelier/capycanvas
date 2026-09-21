# Huion cursor and drawing latency

The cursor lag is real. Two Android costs were outside the earlier G-Pen
renderer measurements: hover delivery waited for the UI frame, and the final
full-screen viewport used a Float16 surface even for an SDR document. Neither
cost comes from the swept-contact simplifier holding back the visible stroke.

Source base: `39e9cba3537e604705755ab4ac79dc870bc96884`. Device: Huion KP1202,
`G7DL2S300241`, Android 16, Mali-G57 MC2, 2400×1600 landscape, 90 Hz.
The app setting is **Pen & Input → Enable stroke prediction**; the user
confirmed it is enabled. “Instant feedback” is its internal engine name.

## Input delivery

The native cursor uses the latest real sample, independently of committed brush
spacing. Android delivered hover in batches at the UI vsync; the separate
canvas Looper then had its own display callback. The existing
`requestUnbufferedDispatch(MotionEvent)` call ran only on pen-down. Android's
[implementation](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/core/java/android/view/View.java)
explicitly limits that overload to touch down/move, so it does not fix hover.

The surface now requests unbuffered pointer-class delivery while hovering,
restoring the default on hover exit, focus loss, and detach. Android 29 retains
its existing behavior because the source-class API requires Android 30. Stroke
samples, pressure, prediction and shared brush geometry are unchanged.

Two original-build hover runs had median sample ages of 19.74 and 19.91 ms at
render start. The corrected runs measured 5.70–5.76 ms. UI delivery itself fell
from roughly 10–11 ms to 2.6–3.0 ms. The latest sample is thus about 14 ms fresher
before GPU work begins. All real samples still reach the engine.

## Hidden viewport GPU cost

Diagnostics' renderer GPU span stops before `ViewportPresenter::present`.
It does not measure the full-screen viewport, Android composition, or scanout.
The earlier [G-Pen results](gpen-huion-sparse-strokes-2026-09-20.md) remain valid
for their stated renderer-only interval; they are not total frame GPU costs.

The Android host unconditionally selected `Rgba16Float` on HDR-capable hardware,
including when the actual output was SDR. That path also performs shader sRGB
encoding and Float16 quantization. The corrected host keeps the negotiated native
sRGB format for SDR and switches to Float16/PQ only for HDR output. On this
Huion those formats are `Rgba8UnormSrgb` and `Rgba16Float` respectively. Document
precision and Float32 painting/composition are unchanged.

A temporary nonblocking render-pass timer isolated the viewport. The same
instrumentation was present on both compared builds and was removed afterward.
GPU observations below arrived during the marked action; they are not optical
pen-to-photon measurements or hardware utilization counters.

| Viewport workload | Float16 median / p99 | Native SDR median / p99 |
| --- | ---: | ---: |
| Hover, no painting | 19.92 / 24.87 ms | 9.47 / 12.69 ms |
| 18.8 px G-Pen drawing | 11.38 / 22.82 ms | 8.61 / 13.39 ms |

The drawing comparison produced 363 versus 441 joined displayed frames over
five seconds: about 73 versus 88 updates/s. Hover produced 483 versus 895 over
ten seconds. Device clocks were not pinned; hover/contact power policy and
the timing probes affect these observations. Do not claim the larger hover
speedup for normal painting.

## End-to-end software timestamps

The replay injects real Android stylus MotionEvents at 200 Hz. Hover runs last
ten seconds; drawing runs last five seconds at half a loop per second. The
drawing ellipse is centered at (1200,800), radii (350,250), pressure 1. Test
strokes were undone. This is a 2048×1536 SDR document, not the earlier 1K fixture.

For each canvas callback, the analysis reconstructs the latest real sample's
time from the input-age counter, joins the submitted canvas frame number to
SurfaceFlinger's `setBuffer` record, then joins its parent `commit` token to
the actual display FrameTimeline. The native submission and BLAST frame numbers
were checked against `acquireNextBufferLocked`. The older report's
`latchBuffer SurfaceView...` pattern does not match this Android 16 trace.

With viewport timing enabled, the corrected drawing run measured input to
presentation at 32.29 ms median, 35.45 ms p95 and 60.84 ms p99. The preceding
Float16 run, already with the hover fix, measured 33.63 / 55.06 / 65.14 ms.
These short traces demonstrate remaining latency and improved pacing, not a
qualified physical-pen p99. The interval after queueing includes viewport GPU
execution; do not add the viewport timing to it a second time.

Original-build hover captures varied from 47 to 67 ms median overall. The fixed
instrumented hover run measured 34.99 ms. Input freshness improved consistently;
total display latency is more variable. A trial reducing the FIFO latency hint
to one did not establish a benefit and was discarded. FIFO and the original
two-frame hint remain. No Mailbox workaround was introduced.

The final production APK, with the viewport probe removed, was measured again:

| Workload | Joined displayed frames | Input age at render, median | Input to presentation, median / p95 / p99 |
| --- | ---: | ---: | ---: |
| Hover, 10 s | 891 | 5.30 ms | 32.14 / 35.59 / 43.90 ms |
| Drawing, 5 s | 439 | 4.72 ms | 31.83 / 43.44 / 61.45 ms |

The clean-build checks retain Android/Perfetto tracing but no extra viewport GPU
timestamp pass. These short runs do not establish a sustained physical-pen tail.

There is still approximately three refresh intervals of software input-to-display
delay in the measured drawing path. Further brush-contact simplification cannot
remove the full-screen viewport cost or presentation pipeline.

## Android game presentation investigation

The viewport already renders into the acquired Vulkan swapchain image, which is
presented through the Android native window. There is no CPU readback or extra
application copy after that pass. Android's
[BufferQueue documentation](https://source.android.com/docs/core/graphics/arch-bq-gralloc)
states that buffers move by handle, not by copying their contents. A direct-copy
replacement alone would not remove the compositor or display scheduling.

Reanalysis of the final production traces splits the last interval as follows:

| Workload | QueuePresent start to SF setBuffer in commit, median | SF setBuffer to actual display, median |
| --- | ---: | ---: |
| Hover | 13.20 ms | 12.64 ms |
| Drawing | 12.43 ms | 12.65 ms |

These are stages in the same frame-number/FrameTimeline join used above, not new
captures. The SF setBuffer timestamp is neither a GPU-completion fence nor the
physical scanout time. The first interval includes asynchronous app GPU work;
the second includes compositor work and display scheduling. This split does not
prove that 25 ms is removable queue backlog. Stage medians need not sum exactly.

Google's [game frame-pacing guide](https://developer.android.com/games/sdk/frame-pacing)
uses presentation timestamps and completion fences to prevent the producer from
running too far ahead of display. Choreographer alone does not guarantee this.
Its non-pipelined mode can reduce latency when CPU and GPU work fit within one
refresh interval; the default throughput-oriented pipeline is not automatically
the lowest-latency choice. [Swappy's Vulkan integration](https://developer.android.com/games/sdk/frame-pacing/vulkan/add-functions)
wraps vkQueuePresentKHR, rather than bypassing Android's compositor.

Concrete differences from our implementation:

- `CanvasHost.wake` obtains Choreographer's expected presentation time, but it is
  passed to the engine's frame/prediction logic, not to Vulkan presentation.
  There is no actual-presentation feedback loop controlling when rendering starts.
- The attached Huion advertises `VK_GOOGLE_display_timing`. Our vendored Vulkan
  HAL already exposes `set_next_present_time` and the raw swapchain, but the
  renderer does not request `VULKAN_GOOGLE_DISPLAY_TIMING`. The extension's past
  timing data can distinguish earliest possible presentation from actual
  presentation. A desired timestamp is a scheduling constraint, not a command
  to display an unfinished buffer sooner.
- The swapchain hardcodes `pre_transform(IDENTITY)`. The Huion compositor dump
  shows the canvas with `ROT_270` and `CLIENT` composition. Google's
  [Vulkan pre-rotation guidance](https://developer.android.com/games/optimize/vulkan-prerotation)
  recommends rendering in the display's native orientation and matching the
  swapchain pre-transform to the surface. This can avoid compositor rotation
  work. Rotation is a concrete candidate here, but we have not proved it is the
  only reason this device chooses GPU composition; UI overlays and color handling
  can also matter. The change needs matching viewport, cursor and overview
  transforms, not just a different swapchain flag.
- [Unity exposes both optimized frame pacing and display rotation during rendering](https://docs.unity.com/en-us/engine/6000.6/manual/platform-specific/android/getting-started/class-player-settings).
  These are established game-engine techniques applicable to our existing renderer.

The recommended next experiments, preserving a single brush implementation, are:

1. Add frame IDs and nonblocking Vulkan presentation feedback to the existing
   native surface; measure GPU readiness, earliest/actual presentation and margin.
   Do not block the input-owning Looper on pacing waits.
2. Implement pre-rotation within the existing viewport pass and compare compositor
   mode, viewport GPU time and input-to-display latency on the Huion. Avoid adding
   another full-screen rotation pass.
3. Use that feedback to choose a render start time and bound work in flight,
   taking the newest input after any pacing delay. Evaluate a Swappy integration
   against the existing Choreographer scheduling rather than stacking two pacing
   loops. Reducing the swapchain buffer-count hint alone already failed to
   establish a benefit in the earlier trial.
4. If needed, prototype independent cursor presentation and front-buffer updates
   restricted to live ink damage. Android's
   [stylus guidance](https://developer.android.com/develop/ui/views/touch-and-input/stylus-input/advanced-stylus-features)
   recommends front buffering for small drawing updates and warns against using
   it for full-screen pan/zoom due to tearing. Reuse the current brush output;
   do not introduce another brush algorithm.

The Huion also advertises `VK_KHR_shared_presentable_image` and
`VK_ANDROID_external_memory_android_hardware_buffer`. These make native front-buffer
experiments plausible, but extension presence does not establish the required
surface modes, buffer usages or visual correctness. Front buffering is more
invasive than the game techniques above and is not a drop-in copy operation.

The capability dump and additional trace analysis are archived with the existing
latency artifacts as `huion-vkjson.json`, `latency-display-split.py`, and
`hover-final[-stroke]-display-split.json`. This investigation does not deploy a
new build or claim measured gains from these proposed changes.

## Validation and retained evidence

- Release build and Android release lint passed.
- Native HDR-capability test passed, including Float16/PQ support without
  requiring the old Float16/sRGB proof format.
- `AndroidRasterTest#hdrDisplayNegotiation` passed on the Huion (52.844 s).
  It checks HDR pixels, SDR fallback, proof switching, exact HDR pixel restoration,
  and surface recreation. Its assertions now check the negotiated format too.
- The shell input helper no longer emits an invalid HOVER_EXIT after pen-up
  without a matching hover-enter. Android 16 rejected that event and terminated
  the helper; this was not an app crash. The corrected drawing replay completed.
- Temporary timing probes and the buffer-count experiment are absent from the
  production source. The isolated validation app and its test runner were removed.

Raw traces, action markers, frame joins, comparison JSON, probe APKs, production
APK and HDR pixel reports are retained locally under
`artifacts/gpen-huion-2026-09-20/latency/`. The capture and analysis scripts are
included there. The main app was updated in place without clearing its data.
The production APK SHA-256 is
`feb129b0b7a4631c68494384e97293624ec48b4b95cfa7f2ac66e11f3d0f4b04`.


The subsequent Vulkan front-buffer experiment is covered in
[its results report](android-front-buffer-results-2026-09-20.md). The user then
authorized requiring front-buffer support for Android; see
[production qualification](android-front-buffer-production-2026-09-20.md).
