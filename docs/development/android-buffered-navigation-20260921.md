# Android buffered navigation and front-buffer ink

## Presentation contract

Camera changes (pinch, pan, rotation, fit) use FIFO with
`desired_maximum_frame_latency = 3` (4 on some MediaTek Android 14 devices;
see below). Navigation stays buffered after settling; an accepted new paint
contact returns to `SharedDemandRefresh`, latency 1.
Simultaneous camera motion takes precedence. Idle views stop rendering.

Paint admission belongs to shared Rust `NativeHost`; its contact sequence is
available before the engine consumes queued input. The Android Rust surface
owner switches **before submitting brush GPU work**. Switching afterward makes
the swapchain drain wait for that update and caused 40–60 ms pen-down stalls.
Kotlin UI scheduling and shared brush/cache/mip algorithms are unchanged.

`ViewportPresenter::set_target_retention` resets damage history on every
reconfiguration, including resize and recovery. Buffered images redraw fully;
a replacement shared image also needs a full first draw. Android still requires
shared-presentation support for painting; FIFO is not a driver fallback.

## MediaTek Android 14 BufferQueue workaround

The XP-Pen MagicNotePad (MT8781, Mali-G57 MC2, Android 14 `UP1A.231005.007`)
crashed with SIGSEGV on the first FIFO present after any shared present.
Rotation, zoom and pan all trigger it. Its `libgui` differs from AOSP: for an
EGL-connected producer whose `mMaxDequeuedBufferCount` is exactly 3,
`queueBuffer` throttles on the fence from two queues back. Any other queue
leaves that older fence null, and connect/disconnect do not reset it. Shared
presentation dequeues one buffer, so the next 3-buffer FIFO queue waits on
null (`Fence::waitForever`, fault address `0xc`). Recreating the swapchain or
`wgpu::Surface` does not help, because the window keeps the same producer.

On 2026-09-27, disassembly found this change only in the XP-Pen's `libgui`.
The Wacom RosePlus (MT8781, Android 14, February 2026 build), TCL 9465X
(MT8786, Android 15) and Huion KP1202 (MT8391, Android 16) keep the AOSP
single-fence throttle. Forcing three dequeued buffers after shared
presentation crashed only the XP-Pen.

On Android 14+, a FIFO swapchain's dequeue budget is
`latency + 1 - (minimum latency - 1)`. On MediaTek Android 14 devices
(`ro.soc.manufacturer`), the host adds one frame of navigation latency when
that budget would be 3. The XP-Pen (minimum latency 2) therefore navigates at
latency 4. The RosePlus minimum of 4 already avoids the count. Other devices
keep latency 3.

## Wacom qualification, 2026-09-21

MovinkPad DTHA140, Adreno 735, Android 15, 2880×1800 at 120 Hz. The SDR photo
is 9504×6336, fit near 17.96%; two injected fingers at 200 Hz zoom through roughly
12–27% once per second. Current shared and pre-optimization `16bb86c1` shared
presentation both showed physical-screen RGB artifacts; current FIFO was clean.
This predates optimization `65a323b1`. Captures did not establish the precise
driver/scanout mechanism. The user confirmed the deployed fix works well.

| Measurement | Result |
| --- | --- |
| FIFO depth hints 1 / 2 / 3 | 95.88 / 96.24 / 119.05 displayed FPS |
| Initial / integrated-main automatic policy, 30-second replay | 118.51 / 119.28 displayed FPS |
| 60-second buffered replay | 118.84 displayed FPS |
| 2000px G-Pen, three 10-second strokes | 105.38 initially; 105.76 after integration; initial shared control 104.85 |
| Ten immediate navigation → 18px pen starts | Switch 2.36–5.25 ms; input-owner → first submission 6.72–11.04 ms |

FPS uses canvas SurfaceFlinger `actualPresentTime` samples, not submitted frames
or panel refresh. A 27.706-second trace independently measured 118.64 latches/sec;
its largest gap was 49.54 ms. More buffer capacity improves overlap but can add
navigation queue latency; the physical replay was clean and responsive.
Choreographer pacing was slower and was removed. Completion callbacks do not
measure pen-to-photon latency. Other GPUs and HDR navigation need qualification.

The integrated-main recheck on September 22 also recorded one 18.35 updates/sec
run with lower tracked storage and much longer composition time. The same APK
then reached 105.12, beside 105.25 for the prior build; three final runs averaged
105.76. This suggests allocation/cache-path variability, but the slow run's
admission log was unavailable, so the cause is not confirmed. `displayStatus`
now includes `display_memory_limits` (display/source/upload ceilings) for future
comparisons. No cache admission or memory limits were relaxed.

## Regression checks and reproduction

- `layer-render-wgpu` retained-scene pixel test: shared → buffered → new shared
  images match the full redraw reference without changing the scene.
- `AndroidRasterTest#navigationBuffersAndPenReturnsToFrontBuffer`: the first ink
  frame is shared; repeated navigation preserves document pixels and revision.
  Without the MediaTek Android 14 workaround, the XP-Pen crashes at its first
  zoom.
- `AndroidRasterTest#frontBufferSurfaceLifecycle`: rotation, surface recreation,
  GPU recovery, undo/redo and artwork hashes.

Use an isolated benchmark package following [Android development](android.md).
Build `:app:assembleBenchmark -PcapyAbi=arm64-v8a -PcapyOptimize
-PcapyApplicationId=art.capycanvas.pinchprobe`. Supply the benchmark photo at
`/data/local/tmp/capy-brush-photo.jpg`, or its lossless project at the test app's
external `files/photo.capy`. Run:

```sh
adb -s "$CAPY_ANDROID_SERIAL" shell am instrument -w \
  -e brushBenchmark true -e mode pinch -e label pinch -e durationMs 30000 \
  art.capycanvas.pinchprobe/art.capycanvas.BrushBenchmarkInstrumentation
```

For stroke handoffs use the brush runner with `navigationBetweenStrokes=true`
and `navigationSettleMs=0`. Reports stay in external `files/brush-benchmark`.
Measure the native canvas SurfaceView's actual-present timestamps during motion;
screenshots are limited to before/after. Remove the isolated app after testing.
Raw investigation artifacts and rejected experiments are not source deliverables.
