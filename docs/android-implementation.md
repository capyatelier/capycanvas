# Android native host

Status: implementation in progress, not yet accepted by device testing.

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
presentation still need measurement, not an inference from that setting.

## Validation progress (not final acceptance)

- Android x86_64 release native library and debug APK build successfully.
- Three Rust host tests pass: GPU-unavailable UI, malformed input, shared touch routing.
- Four emulator tests pass: real injected stylus paints visible pixels; undo/redo;
  preferences/search/theme; expanded drawer and divider resizing; high-rate input
  stream/render scheduling (the latter is a measurement, not a 120 fps assertion).
- Initial workspace, settings, light/dark and drawer screenshots inspected.
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
- Still required: wider drag/drop and customization coverage, shortcut editing,
  pressure/history/cancellation/palm/multitouch and lifecycle/rotation coverage,
  UI refinements and native touch target audit, complete performance analysis,
  arm64/release validation, and final visual acceptance. Do not treat the initial
  passing suite as proof of those remaining requirements.

Primary references: [Compose external surfaces](https://github.com/androidx/androidx/blob/androidx-main/compose/foundation/foundation/src/androidMain/kotlin/androidx/compose/foundation/AndroidExternalSurface.android.kt),
[SurfaceView composition](https://source.android.com/docs/core/graphics/arch-sv-glsv),
[stylus input](https://developer.android.com/develop/ui/compose/touch-input/stylus-input/advanced-stylus-features),
[Choreographer](https://developer.android.com/ndk/reference/group/choreographer).
