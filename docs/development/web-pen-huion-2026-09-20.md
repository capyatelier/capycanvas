# Huion Web G Pen CPU and input investigation

Base: `f458d13e` (`origin/main`, 2026-09-20). Device: Huion Kamvas Pad 12 / KP1202,
Mali-G57 MC2, Android 16, 2400×1600 at 90 Hz. Chrome 143 uses desktop-site mode:
1200×680 CSS viewport, backing scale 2. Device clocks were not pinned.

The reported high CPU was the **Diagnostics panel's rolling CPU p99**, not a
saved trace. The normal 1K fixture below reproduced 6–9 ms before the changes,
not the user's >20 ms window. Do not interpret these measurements as a bound on
every document, speed, brush or pressure. Sustained 90 Hz and physical
input-to-photon latency are **not qualified**.

## What had already reached Web

The shared engine and WebGPU renderer on this base already contain the
[bounded prediction, sparse swept contacts and direct composition improvements](gpen-huion-sparse-strokes-2026-09-20.md),
as well as the unified tile executor. Web does not need a separate G Pen math
port. The native [Android presentation improvements](android-front-buffer-production-2026-09-20.md)
are host-specific: independent rendering ownership, unbuffered Android input,
retained Vulkan shared-demand presentation and partial attachment regions do not
automatically transfer to the browser.

## Costs found and changes

In a five-second fast-stroke Chrome CPU profile, repeated hit testing cost about
741 ms inclusive. The same pointer move passed through global chrome routing,
canvas routing and duplicate cursor updates. Repeated style writes invalidated
layout before `elementFromPoint`/`getBoundingClientRect` queries.

After reducing that overhead, another five-second profile attributed about
639 ms inclusive to `map_wgt_features`: every `wgpu::Device::features()` call
remapped the whole browser feature set through Wasm/JS strings. Tile, brush and
composition code asks for features frequently. The local wgpu patch now maps
the **actual device's immutable features once**, on device creation. It does not
substitute adapter capabilities or change selected rendering features.

The Web host now:

- Accepts the captured active pen's coalesced samples through passive
  `pointerrawupdate`, the earliest standardized Pointer Events delivery path.
  A matching `pointermove` supplies only later predictions. Actual raw delivery
  is tracked per contact, preserving move fallback on unsupported browsers and
  input sources. Raw updates are non-cancelable; regular events retain normal
  cancellation. Touch, mouse and workspace UI keep their existing arbitration.
- Avoids duplicate chrome/cursor processing during captured paint contacts,
  avoids unchanged DOM style writes, and measures canvas bounds once per batch.
- Uses the shared GPU cursor in the existing viewport presentation pass,
  retaining segment capacity. This removes SVG path formatting, DOM updates and
  the full-window SVG overlay. The obsolete SVG formatter and Web cursor API
  are removed; all hosts use the same segment generator. Layer overlays still
  join the same segment list. Web pen and PWA tests share GPU pixel checks.
- Models a frame against `performance.now()` after input admission. The rAF
  timestamp can predate the newest sample; it is no longer used as current time.
  Presentation prediction estimates the next display tick from recent callback
  intervals instead of assuming a 120 Hz monitor. This is an estimate, not a
  browser promise of scanout time.

The [Pointer Events specification](https://www.w3.org/TR/pointerevents/)
defines raw/coalesced ordering. `pointerrawupdate` can still be coalesced and
does not bypass Chrome's input dispatcher, main thread or compositor.

## Matched 18 px measurements

1024×1024, G Pen 18 px, fixed pressure 1, fitted canvas, default Paint workspace
with Diagnostics visible. Three five-second elliptical strokes per build.
The existing `AndroidPenMotion` helper delivers 200 Hz stylus samples through
Android's input dispatcher. This is OS stylus replay on the physical Huion,
**not movement of the physical pen**. Normal 16 ms engine prediction is enabled;
browser prediction is disabled for reproducibility. Camera, backing size and
workspace are identical. Each measured stroke must advance the document revision.

Ranges below are the three per-run results, not pooled percentiles:

| Measurement | Original main | Final Web changes |
| --- | ---: | ---: |
| Drawing submissions/second | 45.0–59.1 | 54.5–60.4 |
| Diagnostics CPU p99 | 6.1–8.7 ms | 3.8–4.2 ms |
| Complete `app.frame()` CPU p99 | 7.2–9.9 ms | 5.9–6.6 ms |
| Latest actual event → submission, median | 30.8–31.3 ms | 25.6–26.7 ms |
| Latest actual event → submission, p99 | 37.6–45.1 ms | 34.2–35.0 ms |

An earlier run with the feature cache and GPU cursor, before the frame-clock
change, reached 61.2–64.8 submissions/sec and CPU p99 3.4–4.4 ms. It is retained
as `cached-os`; the final repeat is `final-os`. This variability is why the
best observed rate is not a 90 Hz qualification. Keeping the cached renderer
but restoring the SVG cursor reached 48.0–54.4/sec (`cached-svg-os`).
These early variant snapshots omitted font/brush-preview assets. They are
exploratory evidence, not an isolated measurement of the GPU cursor's speedup.
The final baseline and fast Navigator comparisons use complete static assets.

With a 2048×2048 canvas, four times the normal stroke speed and **Navigator
visible**, the three-run ranges were:

| Measurement | Original main | Final Web changes |
| --- | ---: | ---: |
| Drawing submissions/second | 36.6–38.1 | 45.2–47.2 |
| Complete `app.frame()` CPU p99 | 10.2–12.4 ms | 5.5–6.8 ms |
| Latest actual event → submission, median | 31.1–31.6 ms | 24.9–25.1 ms |
| Latest actual event → submission, p99 | 41.8–46.5 ms | 33.7–34.7 ms |

These are `baseline-fast-navigator` and `final-fast-navigator`. Diagnostics
collection is disabled when its panel is hidden, so these rows report complete
Web frame CPU rather than inventing a Diagnostics percentile. The 90 Hz target
is farther away in this workload despite the reduced CPU cost.

The idle continuous rAF probe measured about 91 Hz, with median 11.0 ms and p99
11.1 ms. During drawing it misses ticks. Incoming Web events are already about
18–19 ms old at the handler in these OS replay runs; raw delivery still batches
at roughly 90 Hz. The remaining input/browser presentation path needs separate
investigation even after renderer CPU falls below the 11.1 ms budget.

Diagnostics CPU covers renderer encoding/submission, not the whole input task,
viewport presentation, Chrome GPU process or compositor. Its 120-sample window
can change between JSON collection and a screenshot. GPU Diagnostics measures
paint work, excluding viewport presentation. `app.frame()` wall time includes
viewport command submission but does not wait for the GPU. Submission counts
and event-to-submit timing do not prove visible frame counts or photon latency.

## Front-buffer investigation

[WebGPU's canvas configuration](https://gpuweb.github.io/gpuweb/#dictdef-gpucanvasconfiguration)
does not expose a retained scanout image, Vulkan present mode or a
`desynchronized` option. A current canvas texture cannot be treated as Android's
retained shared swapchain image. Calling `retain_target()` on it would be wrong.

Chrome's [desynchronized canvas path](https://developer.chrome.com/blog/desynchronized)
is available for 2D/WebGL contexts. The Huion accepted
`{alpha:false, desynchronized:true}` for both 2D and WebGL2. We tested an opaque
desynchronized 2D canvas above the WebGPU canvas and copied the rendered image
after each submission (`--front-copy`). It rendered valid ink and achieved
60.4 submissions/sec, CPU p99 4.9 ms, and event-to-submit median/p99
24.2/33.2 ms in one five-second run. This neither improves throughput convincingly
nor proves direct scanout. DOM overlays and browser composition still matter;
the copy can introduce synchronization and extra bandwidth. It is an experiment
preserved only in the local `exploratory-web-pen.mjs` artifact, **not included in
the app or the reusable benchmark**.

`navigator.ink.requestPresenter` was also available, but delegated ink is a
browser-managed provisional trail, not the full pressure/brush/composition
renderer or an Android-style retained output. Availability alone is not a
latency measurement. A WebGL presentation bridge or delegated ink would require
separate pixel, color, eraser, prediction and latency qualification.

## Reproduce and inspect

Build and serve `apps/layer-web` on a **dedicated test origin**, reverse that port
to the Huion and forward Chrome DevTools. See [the Web device guide](web.md#test-and-debug-web-on-android).
The benchmark creates documents and changes settings/workspace in that origin;
it must not target a user's drawing tab. It keeps recovery copies.

```sh
bash apps/layer-web/build.sh
python3 -m http.server 4198 --bind 127.0.0.1 --directory apps/layer-web
```

In another terminal, with `adb` on PATH:

```sh
export CAPY_ANDROID_SERIAL=G7DL2S300241
adb -s "$CAPY_ANDROID_SERIAL" reverse tcp:4198 tcp:4198
adb -s "$CAPY_ANDROID_SERIAL" forward tcp:9258 localabstract:chrome_devtools_remote
adb -s "$CAPY_ANDROID_SERIAL" shell am start -a android.intent.action.VIEW \
  -d http://127.0.0.1:4198/ -p com.android.chrome

# For --os-input, build the existing shell-only stylus helper first.
mkdir -p /tmp/capy-web-pen/classes /tmp/capy-web-pen/dex
javac -cp "$ANDROID_HOME/platforms/android-37.0/android.jar" \
  -d /tmp/capy-web-pen/classes tools/performance/AndroidPenMotion.java
"$ANDROID_HOME/build-tools/37.0.0/d8" \
  --lib "$ANDROID_HOME/platforms/android-37.0/android.jar" \
  --output /tmp/capy-web-pen/dex /tmp/capy-web-pen/classes/AndroidPenMotion.class
adb -s "$CAPY_ANDROID_SERIAL" push /tmp/capy-web-pen/dex/classes.dex /data/local/tmp/capy-web-pen.dex

LAYER_DEVICE_CDP=http://127.0.0.1:9258 LAYER_WEB_URL=http://127.0.0.1:4198/ \
  LAYER_PEN_LABEL=final LAYER_TEST_ARTIFACTS=artifacts/web-pen \
  node tools/performance/web-pen.mjs --os-input --reload
```

The OS fixture calibrates screen-to-client offset at Huion landscape coordinates
(1200,800); adapt this calibration for another device. `LAYER_PEN_SIZE` sets the
square document extent, `LAYER_PEN_SPEED` multiplies motion speed,
`LAYER_PEN_REPEATS`/`LAYER_PEN_DURATION` control repetitions and milliseconds.
`--navigator` keeps Navigator visible instead of Diagnostics. Without
`--os-input`, CDP drives pressure-varying strokes; these are a different fixture
and must not be pooled with OS replay. `--profile` and `--trace` add profiling
overhead and are for attribution, not headline throughput. Both shader startup and new-document startup
must finish before measurements.

Local evidence is under `artifacts/web-pen-huion-2026-09-20/`: original and
intermediate Wasm/site snapshots, raw per-frame JSON, Chrome CPU profiles and
traces, and PNGs. `baseline-os-complete.json` and `final-os.json` support the
normal-stroke table above; `baseline-os.json` is the earlier exploratory baseline.
The initial CDP `baseline.json` used a host wall-clock timestamp; its latency
fields are invalid because the host/device clocks differed. Later CDP runs omit
that timestamp, and OS runs use Android/Chrome's event clock.

## Regression checks

- Release Wasm build and Linux (including tests)/Android arm64 compile checks passed.
- Fifteen input tests cover coalesced/raw samples, duplicate filtering,
  predictions, fallback, tip/eraser pressure zero, pen-up, hover-up, cancellation,
  lost capture, foreign pointers and mouse/touch routing.
- Two clock tests cover 90 Hz, skipped/late callbacks, display changes and idle.
- Shared cursor tests cover retained storage, pressure, zoom, DPI, transformed
  mask contours, settings and pan using the same segments presented by every host.
- `--pen` checks actual GPU cursor pixels, canvas exit, G Pen 18 px pixels,
  one-step undo/redo, no device-feature remapping, and prediction preference/input
  behavior. The pen, PWA and parity suites share these checks. Huion Chrome and
  hardware-composited Linux Wayland passed. Headless Chrome's cursor pixel check
  failed with GPU compositing disabled; that path is not qualified.

```sh
node apps/layer-web/pointer.test.mjs
node apps/layer-web/frame.test.mjs
bash tools/performance/workspace-motion.sh web --pen
LAYER_DEVICE_CDP=http://127.0.0.1:9258 LAYER_WEB_URL=http://127.0.0.1:4198/ \
  node apps/layer-web/device.test.mjs --pen
```
