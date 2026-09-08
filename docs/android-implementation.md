# Android native host

Status: native tablet prototype implemented and emulator-validated. The
[editor visual audit](android-ui-audit.md) records GTK/web alignment and native
adaptive settings behavior.
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

The hot input path reuses a bounded pool of numeric arrays, transferring ownership
from the input callback to the render task and returning it only after JNI has
consumed the batch. JNI copies only the used records into reusable Rust scratch
space. History and predicted records use the same route; a busy worker never
causes an in-flight buffer to be overwritten or input to be dropped. Native cursor
geometry is rebuilt into retained storage without generating unused SVG strings.

UI snapshots check the shared core revision and host presentation state before
building panel/layout models or serializing JSON. Hover and ordinary stroke
movement do not rebuild unchanged controls. Vulkan swapchain images remain owned
by the platform buffer queue and reused: there is no per-frame Compose texture,
canvas bitmap, CPU raster, or canvas-pixel round trip. The shared wgpu renderer
recycles small mapped upload chunks on native platforms for contacts, styles and cursor geometry;
camera uniforms are uploaded only when their values change. Those copies use
the existing submissions without adding a CPU wait.

Surface loss must not destroy the session or document. All window references and
swapchains must be released on their owning render thread, after the surface is
detached. A failed GPU initialization leaves native controls and an error visible.

Build tools: Android SDK 37.0 (the repository package is `platforms/android-37.0`),
NDK r29, Gradle 9.5, Java 25 host runtime, arm64 devices and x86_64 emulator.

## Build and run

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --locked
android --sdk="$HOME/Android/Sdk" sdk install platforms/android-37.0 build-tools/37.0.0 ndk/29.0.14206865 platform-tools emulator
android --sdk="$HOME/Android/Sdk" emulator create medium_tablet
bash apps/layer-android/run.sh
```

`run.sh headless` runs without an emulator window. `run.sh test` builds and runs
the native integration tests. Set `CAPY_ANDROID_SERIAL` for another connected
device and `CAPY_ANDROID_AVD` for a different existing emulator. The Gradle
wrapper validates its downloaded distribution against the pinned SHA-256.
The SDK-management `android` CLI is optional; Android Studio's SDK/Device
Managers can install the same packages and create the tablet instead. Java,
Rust and the Android tools are external prerequisites, not vendored sources.

Settings and workspace customization persist between launches. Like the other
prototype hosts, the drawing document is currently in memory; do not use this
build for artwork that needs to survive process termination.

The CLI's medium-tablet profile currently installs an Android 15 image, separate
from the SDK 37 used to compile the application. The emulator's documented
`hw.lcd.vsync=120` setting in its AVD `config.ini` enables a 120 Hz mode after a
cold restart; the default is 60 Hz. Actual app cadence and SurfaceFlinger
presentation must be measured, not inferred from that setting.

## Validation

- Android x86_64 and arm64 release native libraries build successfully. The arm64
  ELF load segments have 16 KB alignment. The combined ARM64/x86_64 unsigned
  release-configuration APK passes `zipalign -c -P 16 4`. The debug APK and
  pinned-wrapper `run.sh headless` build/install/launch flow have been exercised.
- Four Rust host tests pass: GPU-unavailable UI, malformed input, shared touch
  routing, and unchanged-input snapshot suppression with state/error/resize updates.
- Sixteen emulator tests cover: visible stylus paint and pixel-checked undo/redo;
  preferences/search/theme; animated drawer and divider resizing; native context
  menus and toolbar creation/tile reordering; tab and whole-group moves; multiple
  shortcut recording, saving and activity recreation; two-finger navigation,
  coalesced history, cancellation, background/foreground surface recovery and
  display rotation; high-rate input/render/compositor measurement; menus, cursor
  choices and About links; palm rejection and pixel-checked erasing; Zen drawer
  dismissal and panel dragging without hiding the workspace; editor geometry;
  compact number editing and vertical-ribbon placement; full-screen settings and
  inline-detail slide animations; atomic numeric edits and settings stylus isolation;
  full-height adjacent settings panes, sidebar alignment, shared text/icon sizes
  and the filled Done button in both themes; settings slider contrast and
  release-to-apply behavior.
  All 72 shared Rust
  UI tests also pass.
- Android lint completes without errors. Remaining warnings concern pinned
  dependency updates, optional Kotlin/Compose conventions and development
  manifest/tooling choices; they are not suppressed with a blanket baseline.
- Workspace, settings, light/dark, joined drawer, shortcut editor, custom toolbar,
  tab-group moves, cursor choices, menus, About, erasing, wet brushes, Zen and
  actual portrait-display screenshots inspected. Android's
  letterboxed forced-activity orientation is not used as portrait validation.
  Test images survive app uninstall under `Pictures/CapyCanvasValidation` in the
  emulator. Timing JSON is under `Download/CapyCanvasValidation`.
- The first repeat uncovered asynchronous IME resets in the search field; native
  editing state is now retained locally until focus leaves, with accepted values
  still owned by Rust. The suite passes again after this fix.
- Refinements found during testing: honor Rust's shortcut visibility flags;
  forward keys from the focused inline shortcut editor; preserve local IME editing state;
  reuse the Vulkan instance across surface replacement; preserve canvas extent
  when the keyboard opens; use one joined drawer shadow; keep platform predictions
  out of contact routing; render Android dialogs with the shared neutral palette;
  dismiss workspace menu windows when opening Preferences.

Settings uses a full-screen top-sliding overlay with automatic persistence, not
a tablet dialog or a global header above two panes. Search/Settings are at the
sidebar's top; the main pane has its page title, filled Done button and detail
Back control. Category/detail panes adapt to list/page
navigation below 840 dp. All detail editors, choice lists, recording/conflicts
and errors stay inline; Back returns within the content pane. Rust owns detail
selection and per-edit validation/apply/save. Compose owns transitions, native
text editing and narrow-screen presentation. The still-mounted GPU canvas is
shielded from touch throughout the overlay's entry/exit. Current captures are in
`artifacts/android/settings-panes/final/`; see the [settings design](settings-implementation-plan.md#android).

These are development APKs, not store-ready releases. Before public binary
distribution, add release signing and complete the exact Maven/native/toolchain
dependency-notice package described in `THIRD_PARTY_NOTICES.md`. The APK already
retains repository license/branding notices and dependency notices supplied by
its Java artifacts; that is not a claim of complete distribution compliance.
Real tablet testing remains necessary for stylus feel, GPU-driver differences,
thermal behavior and sustained 120 Hz. Emulator timing limitations are below.

## Emulator measurements

Android 15 medium-tablet AVD, host-GPU/gfxstream Vulkan, 2560×1600 display,
120 Hz mode. The same shader engine is used on all hosts; there is no pixel
readback during drawing. One render Looper owns the session and swapchain, and
input callbacks enqueue numeric history without waiting for rendering.

September 8 allocation-cleanup comparison: 128 px brushes, warmed pipelines,
existing pigment, 601 injected stylus events spaced approximately 8 ms apart.
The baseline has one run per brush; the updated distributions combine **every
frame from four runs per brush**, not the fastest repetition. CPU callback timing
includes rendering, polling, UI publication and rescheduling (older reports ended
before publication). SurfaceFlinger fps uses its last 127 valid presentations per
run, not the entire gesture; the table gives the updated run-to-run range.

| Brush | Before callback p50 / p95 / p99 (ms) | Updated callback p50 / p95 / p99 (ms) | Publication/reschedule p95, before → updated (ms) | Updated composited fps range |
| --- | --- | --- | --- | --- |
| G-Pen | 3.03 / 11.34 / 17.89 | 3.00 / 11.41 / 18.08 | 0.305 → 0.056 | 105.7–111.4 |
| Natural Blender | 4.74 / 14.99 / 19.06 | 4.63 / 14.33 / 21.42 | 0.328 → 0.057 | 76.7–97.0 |
| Watercolor Wash | 9.56 / 20.46 / 25.00 | 8.44 / 20.46 / 28.84 | 0.375 → 0.074 | 62.0–71.5 |

The updated host allocates 3–6 input arrays across each complete test, including
warm-up, instead of one per event. It builds only 2–3 UI snapshots during the
measured stroke instead of rebuilding 126–140 before comparing serialized text.
CPU input processing p95 remains 0.015–0.016 ms. These measurements establish
less allocation/publication work, **not** a reliable improvement in frame tails:
GPU acquisition, viewport submission and virtual scheduling still dominate, and
some updated p99 values are worse. No sustained 120 fps claim is made. Physical
tablet, thermal and input-to-photon measurements remain necessary.

Maximum swapchain latency remains two; an earlier one-frame experiment did not
improve tails. Raw comparison reports are ignored local outputs under
`artifacts/android/allocation-baseline/` and `artifacts/android/allocation-after/`.

Retained storage is small relative to the canvas: eight 16-double input buffers
need 1 KiB for uncoalesced samples, growing with history; native scratch retains
the largest batch. GPU staging uses 64 KiB brush chunks and 16 KiB viewport chunks,
recycled after completion; the number of chunks depends on upload size and work
in flight. No extra canvas-sized texture is introduced. Debug measurements reserve
1 MiB for bounded flat arrays instead of allocating records on each event/frame;
release builds allocate none of that diagnostic storage.

To repeat a single workload (change the brush name/size as needed):

```sh
apps/layer-android/gradlew -p apps/layer-android :app:connectedDebugAndroidTest \
  -PcapyAbi=x86_64 \
  -Pandroid.testInstrumentationRunnerArguments.class=art.capycanvas.AndroidHostTest#measureHighRateStylusIngressAndRenderScheduling \
  '-Pandroid.testInstrumentationRunnerArguments.capyBrush=Watercolor Wash' \
  -Pandroid.testInstrumentationRunnerArguments.capyBrushSize=128
```

Debug-only bounded timing arrays capture input delivery/queue age and separate
paint, acquisition, viewport submission, queue-present, device-poll and
publication/rescheduling durations, plus the complete render callback.
Release builds do not collect them. Tests retrieve compositor timestamps
separately; no screenshot or readback is included in the timed gesture.

Primary references: [Compose external surfaces](https://github.com/androidx/androidx/blob/androidx-main/compose/foundation/foundation/src/androidMain/kotlin/androidx/compose/foundation/AndroidExternalSurface.android.kt),
[SurfaceView composition](https://source.android.com/docs/core/graphics/arch-sv-glsv),
[stylus input](https://developer.android.com/develop/ui/compose/touch-input/stylus-input/advanced-stylus-features),
[Choreographer](https://developer.android.com/ndk/reference/group/choreographer).
