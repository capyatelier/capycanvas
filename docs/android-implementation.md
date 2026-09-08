# Android native host

Status: native tablet prototype implemented; emulator validation in progress.
Physical-tablet latency and stylus feel are not yet validated.

## Required acceptance

- Native Kotlin/Compose tablet UI, shared icons, brush swatches, colors and Rust
  settings/action/customization models; no HTML or second application model.
- Existing GPU-only brush engine on Vulkan, separately composited SurfaceView,
  engine and presentation off the main/UI thread, no canvas readbacks in drawing.
- Stylus pressure/history/tilt/eraser/hover/cancellation, mouse and keyboard,
  touch pan/zoom/rotation and lifecycle recovery.
- Brushes, size, opacity, colors, layers, undo/redo, menus, Zen mode, native
  adaptive settings/search/shortcuts, persistent settings and workspace.
- Shared drag targets and previews for panels, tab groups, tabs and toolbar
  tiles; dividers; live two-column customization and tool picker.
- Emulator interaction tests and visual inspection in both themes, settings,
  customization, drawing and orientation changes. Fix defects before acceptance.
- Measure input delivery, input queue age, frame rendering and actual presentation
  separately. Target 120 Hz where supported; document emulator limitations, never
  infer presentation latency from submission timing alone.
- Provide a reproducible build/emulator command and leave generated artifacts,
  SDK paths and signing material untracked.

## Host boundary

One Gradle app and one Rust cdylib live in `apps/layer-android`. Kotlin owns native
widgets, event collection, SurfaceView lifecycle and a dedicated render Looper.
That Looper exclusively calls the Rust `UiSession` and wgpu device. Low-rate
actions/snapshots use the existing Serde schema through JNI; pen history uses
numeric batches, never JSON. UI work cannot block surface acquisition or painting.
The surface is independently composited behind the transparent native controls.
The keyboard changes the settings insets, not the GPU surface size. Predictions
from Android's API 34 `MotionPredictor`, when supplied, use the engine's existing
predicted-sample flag and never become document truth. Older devices retain the
shared engine's predictor; there is no additional Kotlin prediction algorithm.

Surface loss must not destroy the session or document. All window references and
swapchains must be released on their owning render thread, after the surface is
detached. A failed GPU initialization leaves native controls and an error visible.

Build tools: Android SDK 37.0 (the repository package is `platforms/android-37.0`),
NDK r29, Gradle 9.5, Java 25 host runtime, arm64 devices and x86_64 emulator.

## Build and run

```sh
android --sdk="$HOME/Android/Sdk" sdk install platforms/android-37.0 build-tools/37.0.0 ndk/29.0.14206865 platform-tools emulator
android --sdk="$HOME/Android/Sdk" emulator create medium_tablet
bash apps/layer-android/run.sh
```

`run.sh headless` runs without an emulator window. `run.sh test` builds and runs
the native integration tests. Set `CAPY_ANDROID_SERIAL` for another connected
device and `CAPY_ANDROID_AVD` for a different existing emulator. The Gradle
wrapper validates its downloaded distribution against the pinned SHA-256.

The CLI's medium-tablet profile currently installs an Android 15 image, separate
from the SDK 37 used to compile the application. The emulator's documented
`hw.lcd.vsync=120` setting in its AVD `config.ini` enables a 120 Hz mode after a
cold restart; the default is 60 Hz. Actual app cadence and SurfaceFlinger
presentation must be measured, not inferred from that setting.

## Validation progress (not final acceptance)

- Android x86_64 and arm64 release native libraries build successfully. The arm64
  ELF load segments have 16 KB alignment. The debug APK and pinned-wrapper
  `run.sh headless` build/install/launch flow have been exercised.
- Three Rust host tests pass: GPU-unavailable UI, malformed input, shared touch routing.
- Eight emulator tests pass: visible stylus paint and pixel-checked undo/redo;
  preferences/search/theme; animated drawer and divider resizing; native context
  menus and toolbar creation/tile reordering; tab and whole-group moves; multiple
  shortcut recording, saving and activity recreation; two-finger navigation,
  coalesced history, cancellation, background/foreground surface recovery and
  display rotation; high-rate input/render/compositor measurement.
- Workspace, settings, light/dark, joined drawer, shortcut editor, custom toolbar,
  tab-group moves and actual portrait-display screenshots inspected. Android's
  letterboxed forced-activity orientation is not used as portrait validation.
  Test images survive app uninstall under `Pictures/CapyCanvasValidation` in the
  emulator. Timing JSON is under `Download/CapyCanvasValidation`.
- The first repeat uncovered asynchronous IME resets in the search field; native
  editing state is now retained locally until focus leaves, with accepted values
  still owned by Rust. The suite passes again after this fix.
- Initial 120 Hz emulator measurement (601 stylus events): frame interval median
  8.33 ms / p95 16.67 ms; CPU input p95 0.016 ms; input delivery p95 1.52 ms.
  CPU painting p95 1.07 ms; surface acquire p95 4.56 ms; viewport/present/poll
  p95 5.77 ms. These are CPU durations, **not** GPU completion or final compositor
  timestamps. Sustained 120 fps is not yet proven; investigate presentation stalls.
- Refinements found during testing: honor Rust's shortcut visibility flags;
  forward keys from the dialog's native window; preserve local IME editing state;
  reuse the Vulkan instance across surface replacement; preserve canvas extent
  when the keyboard opens; use one joined drawer shadow; keep platform predictions
  out of contact routing; render Android dialogs with the shared neutral palette.
- Still required before final handoff: remaining menu/cursor/palm/Zen interaction
  checks, release packaging check, final source review and visual acceptance.

## Emulator measurements

Android 15 medium-tablet AVD, host-GPU/gfxstream Vulkan, 2560×1600 display,
120 Hz mode. The same shader engine is used on all hosts; there is no pixel
readback during drawing. One render Looper owns the session and swapchain, and
input callbacks enqueue numeric history without waiting for rendering.

Isolated September 8 runs: 128 px brushes, warmed pipelines, existing pigment,
601 injected stylus events spaced approximately 8 ms apart. CPU values cover all
render calls; compositor values use SurfaceFlinger's last 127 valid presentations,
not the entire gesture. Virtual GPU scheduling is noisy; these are not physical
tablet or scanout/input-to-photon measurements.

| Brush | CPU frame p50 / p95 / p99 (ms) | Composited interval p50 / p95 / p99 (ms) | Composited fps |
| --- | --- | --- | --- |
| G-Pen | 3.25 / 13.74 / 19.05 | 8.37 / 17.10 / 24.75 | 100.4 |
| Watercolor Wash | 8.60 / 20.07 / 30.38 | 16.62 / 25.20 / 36.13 | 61.4 |
| Natural Blender | 4.37 / 14.62 / 27.04 | 8.36 / 20.18 / 25.36 | 95.1 |

Input delivery p95 is 1.50–1.60 ms; CPU input processing p95 is 0.015–0.017 ms.
Render-thread queue delay grows when GPU work/presentation stalls. CPU painting
submission p95 is 1.27 / 8.72 / 3.79 ms respectively. Acquisition and viewport
submission are substantial contributors; final queue-present itself is usually
below 0.4 ms p95. Reducing maximum swapchain latency from two to one did not
improve tails, so it remains two. This does **not** establish globally optimal
rendering or sustained 120 fps; watercolor especially still needs real-device
profiling. The emulator's 120 Hz capability is confirmed, but the performance
target is not met across these workloads.

To repeat a single workload (change the brush name/size as needed):

```sh
apps/layer-android/gradlew -p apps/layer-android :app:connectedDebugAndroidTest \
  -PcapyAbi=x86_64 \
  -Pandroid.testInstrumentationRunnerArguments.class=art.capycanvas.AndroidHostTest#measureHighRateStylusIngressAndRenderScheduling \
  '-Pandroid.testInstrumentationRunnerArguments.capyBrush=Watercolor Wash' \
  -Pandroid.testInstrumentationRunnerArguments.capyBrushSize=128
```

Debug-only bounded timing arrays capture input delivery/queue age and separate
paint, acquisition, viewport submission, queue-present and device-poll durations.
Release builds do not collect them. Tests retrieve compositor timestamps
separately; no screenshot or readback is included in the timed gesture.

Primary references: [Compose external surfaces](https://github.com/androidx/androidx/blob/androidx-main/compose/foundation/foundation/src/androidMain/kotlin/androidx/compose/foundation/AndroidExternalSurface.android.kt),
[SurfaceView composition](https://source.android.com/docs/core/graphics/arch-sv-glsv),
[stylus input](https://developer.android.com/develop/ui/compose/touch-input/stylus-input/advanced-stylus-features),
[Choreographer](https://developer.android.com/ndk/reference/group/choreographer).
