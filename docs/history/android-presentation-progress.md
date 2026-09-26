# Android canvas presentation investigation — 2026-09-20

## Status: qualified mitigation, original defect not conclusively fixed

The primary Wacom app now uses FIFO instead of preferring Mailbox. This removes
the replacement-only presentation path observed during the original freeze and
uses one fewer screen buffer on this device. It does not change brush math,
document rendering, prediction, HDR encoding, or other platforms' presentation.

The candidate remained responsive during sustained, OS-injected navigation,
including repeated **2% to 1600%** zoom and rapid rotation. However, a freshly
restarted **old** build also passed the aggressive replay. The experiment does
not isolate the policy change from restarting the previously stuck surface.
Do not describe this milestone as a proven repair of all freezes or GPU crashes,
or as an FPS improvement. The condition that originally stopped buffer
consumption remains unidentified.

Base: freshly fetched `origin/main`, `efe76d57ca199a4425d02cff0b5cb54d642a92a5`.
Publication is rebased onto `dc9e99ba` (concurrent Zen visibility preferences);
the paired trace measurements above that rebase remain on `efe76d57` plus the
presentation candidate. No presentation-source conflict was introduced.
Device: Wacom DTHA140 `5ll21u1002931`, Android 15, Adreno 735 Vulkan, 2880 × 1800,
120 Hz. All device interaction here uses the primary `art.capycanvas` package,
release Rust and optimized/R8 release Kotlin, not a second benchmark app.
The recovered photograph is `sony_a7r_v_29 (1)`, 9504 × 6336 (60,217,344 pixels;
the user's approximately 61 MP canvas), with its existing layers. No app data
was cleared. Installation replaces the package in place.

## What the original frozen trace establishes

The retained 4.616 seconds of the user's 30-second trace contained 534 Vulkan
presentation calls (maximum 0.669 ms), **zero canvas buffer latches**, and 497
UI-layer latches. Canvas acquisition was also fast (maximum 0.180 ms).
The canvas queued-buffer counter stayed at one. Four float16 screen buffers
were imported, each 40,856 KiB. This is not an unbounded queue of 61 MP frames.

Separate SurfaceFlinger latency records showed buffers becoming ready in about
5–6 ms but being displayed progressively later. Immediately before the first
replacement, injected navigation produced examples of approximately 21 ms,
204 ms and 3.41 seconds desired-to-actual presentation delay. These records
and the trace localize the failure after submission, before canvas latching;
fast API returns alone are not proof of GPU completion for every buffer.
Menus and settings continued to work because their layer kept advancing.

Android's [Vulkan swapchain implementation](https://android.googlesource.com/platform/frameworks/native/+/refs/heads/main/vulkan/libvulkan/swapchain.cpp)
selects swap interval zero for Mailbox and one for FIFO. Its asynchronous
buffer-replacement route can keep accepting new frames without increasing the
queue length. The [Android 15 BLAST implementation](https://android.googlesource.com/platform/frameworks/native/+/refs/tags/android-security-15.0.0_r10/libs/gui/BLASTBufferQueue.cpp)
has separate frame-available, replacement, release and transaction callbacks.
These are architectural references, not proof of a particular vendor bug or
that an identified BLAST callback was the original blocker.

## Change and critical-path complexity

- Select guaranteed-supported FIFO; retain the existing two-frame latency hint
  and Choreographer callback coalescing. No added frame scheduler or watchdog.
- Expose configured mode, latency hint and submitted-frame count in the existing
  display-status query. Submission is deliberately not labelled visibility.
- Add allocation-free Android trace counters for canvas submissions and rendered
  camera zoom. Compare them with SurfaceFlinger canvas latches, not Compose FPS.
- Keep the existing GPU-ready startup signal unchanged. It is not a general
  screen-presentation acknowledgement.

The Wacom surface dump contained **three** float16 buffers under FIFO versus
**four** under Mailbox: approximately **39.9 MiB less** screen-buffer storage.
FIFO also introduces ordinary producer backpressure: present calls can take
several milliseconds. The comparison below is not evidence of higher throughput
or lower physical pen-to-photon latency. No unbounded GPU queue, surface reset
loop, readback, timer thread or vendor HAL change was introduced.

## Reproduction and results

`tools/performance/AndroidCanvasMotion.java` is a shell-only input injector, not
packaged with the app. It sends actual two-finger Android MotionEvents into the
already-running primary canvas. `limits` alternates two 100× zoom-out pinches
and two 100× zoom-in pinches, lifting/recontacting between pinches, at 120 motion
ticks/s with 16 ticks per pinch: approximately 1.875 full-range cycles/s. Each
pinch also rotates up to approximately 75 degrees. This reaches both camera
clamps, unlike merely oscillating around an already-magnified starting view.

The initial FIFO trial used 25 seconds of gentle motion, followed by 90 seconds
of 2 Hz/100× oscillation and 60 seconds of the stronger recontacting sequence.
The final instrumented candidate ran another 60 seconds with Diagnostics and
Layers visible. Its 20-second trace verified rendered zoom **2%–1600%**.

| Capture | Active submission window | Present calls / canvas latches | Latch gap p99 / maximum |
| --- | ---: | ---: | ---: |
| Original frozen main | 4.509 s | 534 / 0 | 4508.95 / 4508.95 ms (no latches) |
| Restarted old main, limits replay | 19.960 s | 1792 / 1704 | 25.159 / 33.679 ms |
| Restarted old main, Diagnostics visible | 11.784 s | 1112 / 1064 | 25.151 / 25.489 ms |
| FIFO, late in 90-second rapid oscillation | 14.753 s | 1243 / 1242 | 24.704 / 25.276 ms |
| Final FIFO, Diagnostics visible, limits replay | 19.951 s | 1478 / 1477 | 25.403 / 41.143 ms |

The last row is approximately 74 canvas latches/s. A healthy fresh-start Mailbox
control also presented well; these data must not be repackaged as a speedup
ratio against the originally frozen process. Latching is not physical display
scanout or an image-content equivalence check.

One earlier FIFO limits capture contained a **1.50-second internal gap**, followed
by shorter 301/209 ms gaps. It fails the strict progress check. Its presentation
calls were short and no comparable long canvas callback began in that interval;
this was not the original pattern of hundreds of presents with no latches.
Its cause was not isolated, and it is not omitted from qualification. The final
controlled repeat above did not show that gap. Other device interaction was not
prevented during these exploratory runs.

`android-presentation-report.py` rejects a replacement-frame false positive:
continuous successful presents with no canvas latches fail. It compares within
the first-to-last submission window, retaining internal gaps and both window
edges; idle time after a gesture ends is not labelled a freeze. It reports the
actual trace retention, rejects trace errors, and accepts an explicit canvas
TID when an exploratory ring buffer lost process/thread metadata. It cannot
detect a stopped input producer from presentation events alone. The final
capture retained process identity and has no reported trace errors.

Local raw evidence: `artifacts/wacom-zoom-freeze-20260920/`, including original,
FIFO, fresh-start control and final traces, screenshots, SurfaceFlinger dumps,
APK hashes and logs. Compact, committed results accompany this document in
`measurements/android-presentation-20260920.json`. Record a trace only while
continuous gestures are actually running; inspect screenshots and document
state as well as counters.

Example reproduction (set `CAPY_ANDROID_SDK` to the installed SDK):

```sh
mkdir -p artifacts/presentation/classes artifacts/presentation/dex
javac -cp "$CAPY_ANDROID_SDK/platforms/android-37.0/android.jar" \
  -d artifacts/presentation/classes tools/performance/AndroidCanvasMotion.java
"$CAPY_ANDROID_SDK/build-tools/37.0.0/d8" \
  --lib "$CAPY_ANDROID_SDK/platforms/android-37.0/android.jar" \
  --output artifacts/presentation/dex artifacts/presentation/classes/AndroidCanvasMotion.class
adb -s 5ll21u1002931 push artifacts/presentation/dex/classes.dex /data/local/tmp/capy-canvas-motion.dex
adb -s 5ll21u1002931 shell \
  'CLASSPATH=/data/local/tmp/capy-canvas-motion.dex app_process / AndroidCanvasMotion 60 1400 900 limits'
```

While that command is running, in another terminal:

```sh
adb -s 5ll21u1002931 shell perfetto --txt -c - \
  -o /data/misc/perfetto-traces/capy-presentation.perfetto-trace \
  < tools/performance/android-presentation.pbtxt
adb -s 5ll21u1002931 pull /data/misc/perfetto-traces/capy-presentation.perfetto-trace artifacts/presentation/
python3 tools/performance/android-presentation-report.py \
  artifacts/presentation/capy-presentation.perfetto-trace \
  --processor /path/to/trace_processor --check
```

Run only over an unobstructed test canvas, with coordinates adjusted for the
device. Injection is synthetic touch, not physical pen input or pen prediction.

## Regression scope and remaining work

- Release APK build and lint succeed; Android host color-capability test passes.
- Three shared camera tests pass, including two-finger zoom/rotate/pan lifecycle.
- Five report tests cover false progress, continuous latching, trailing freeze,
  trace loss and zoom units.
- No brush code changed. Brush-specific device-loss stress, undo latency,
  hardware pen latency and all HDR transitions were not requalified here.
- Apple, Web, GTK and Windows are unchanged and untested in this milestone.
  This is an Android presentation mitigation, not a claimed shared speedup.

If the final app stalls again, retain it running and capture submission, zoom,
acquisition, canvas latches and UI-layer latches together. A permanently stuck
surface cannot be called fixed merely because restarting it or rebuilding the
app temporarily restores progress. Do not add automatic device-idle waits or
reconfigure loops without isolating the missing completion/consumption signal.
