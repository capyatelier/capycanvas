# Android development

Android pen rendering requires Vulkan shared-demand presentation and swapchain-maintenance
present fences. Unsupported drivers show a canvas initialization error. Pen strokes use
the retained front buffer; whole-view navigation uses FIFO buffering to avoid scanout
artifacts. See [buffered navigation and pen handoff qualification](android-buffered-navigation-20260921.md)
and the earlier [front-buffer qualification](android-front-buffer-production-2026-09-20.md).


[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Android client uses Kotlin and Jetpack Compose for the editor UI. A Rust JNI
bridge connects it to `NativeHost`, and the shared Vulkan renderer presents into
a `SurfaceView`.

## Prerequisites

Install Java 17 or newer, Node.js, Rust, Android Studio or the Android command-line tools,
and these SDK packages:

- Android SDK platform 37 and Build Tools 37.0.0.
- NDK 29.0.14206865 and Platform Tools.
- The Android Emulator and a tablet system image if testing without a device.

The exact versions are set in
[`app/build.gradle.kts`](../../apps/layer-android/app/build.gradle.kts). Install
these through Android Studio's SDK Manager. Set `ANDROID_HOME` if the SDK is not
at `$HOME/Android/Sdk`; the launcher uses that path by default.

Prepare the Rust targets and JNI build tool:

```bash
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --locked
cargo install cargo-about --version 0.9.2 --locked
rustup component add rust-docs
```

Every APK includes the selected Rust targets' original dependency and toolchain
notices in `assets/licenses`. `LAYER_CARGO_ABOUT` can select the notice generator
executable. Photo codecs compile into the shared Rust library; no codec bundle
or helper executable is required.

For emulator use, create a tablet AVD named `medium_tablet` in Device Manager,
or select another existing name with `CAPY_ANDROID_AVD`. Enable hardware graphics.
The app's minimum Android API is 29; the compile SDK and emulator OS need not have
the same version.

## Build and run

```bash
bash apps/layer-android/run.sh
```

The launcher starts the configured emulator if necessary, builds for its ABI,
installs the debug APK and opens the app. The default serial is `emulator-5554`.
For an already connected device, select its serial:

```bash
CAPY_ANDROID_SERIAL=DEVICE_SERIAL bash apps/layer-android/run.sh
```

`DEVICE_SERIAL` is the value shown by `adb devices`. `run.sh headless` starts an
emulator without a window; `run.sh test` runs instrumented tests. The Gradle
wrapper supplies Gradle and builds the Rust library through `cargo-ndk`.

Debug APKs use the `dev-perf` Rust profile: release optimization level 3,
incremental compilation, 16 codegen units and line tables for source-level
profiling. Release and benchmark APKs use `release`. Set `CAPY_RUST_PROFILE`
or pass `-PcapyRustProfile=release` to Gradle to override the selection; the Gradle
property takes precedence. Each variant has its own JNI output directory, and
profile/ABI changes invalidate its Rust task. Direct Gradle builds default to
both ABIs; use `-PcapyAbi=arm64-v8a` or `-PcapyAbi=x86_64` for one device.
See [Rust build times](rust-build-times.md) for measured Rust rebuild times.

## How the host works

Android runs a single editor window: `MainActivity` is `singleTop` and does not
opt into multi-instance system UI, and drawing tabs hold multiple documents.
Compose renders shared tool and workspace models. Android collects `MotionEvent`
history and available predictions, while a dedicated render owner handles the
session and Vulkan work. `Choreographer` supplies frame timing. Surface recreation,
backgrounding and input cancellation need Android lifecycle handling.

[Document transport](../../apps/layer-android/app/src/main/java/art/capycanvas/Documents.kt)
uses Android's Storage Access Framework. Kotlin opens document-provider locations;
Rust processes projects using file descriptors and background work. Providers
control their own destination behavior, so local-filesystem atomic replacement
cannot be assumed for every URI.

[`NoticeBubble.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/NoticeBubble.kt)
shows the shared canvas notice (`UiState.notice`, see
[shared UI](../ui/shared-ui.md)) as a Compose surface over the canvas. It is
centred above the status strip, or above a canvas action bar along the bottom
edge, and keeps that place while a contact hides the bar. It is not a popup, so it never takes window focus or cancels a transform.
Its action button answers `accept: true`. The host hides it after 4 s, answering
`accept: false`, and at the next canvas contact. When the core refuses a
command, action, query or input, the refusal appears in the same surface as a
host-local notice without an action. Only errors that need acknowledgement open
a dialog: the core's `host_error` (file and renderer errors), shown once each
time its published value changes, and file or platform errors that Kotlin
reports through `reportActionError`. `CanvasHost.actionError`, which tests check,
is the open dialog's error or the refusal being shown. Disabled canvas action
bar items show the command's published `disabled_reason` on a tap, a touch or
pen hold, or mouse hover.

## Validation status

The [feature-parity record](../history/android-feature-parity.md) and
[Android implementation record](../history/android-implementation.md) describe
completed checkpoints and outstanding checks. Emulator UI tests do not establish
physical stylus accuracy, thermal behavior or high-refresh presentation. Use a
real tablet to validate those properties.

## Focused device tests and debugging

Run from the repository root with the SDK's `platform-tools` on `PATH`. Enable
USB debugging and authorize the development computer on the device. Select an
attached device or running emulator, then build and install without clearing data:

```bash
adb devices -l
export CAPY_ANDROID_SERIAL=DEVICE_SERIAL
CAPY_TEST_ABI=$(adb -s "$CAPY_ANDROID_SERIAL" shell getprop ro.product.cpu.abi | tr -d '\r')
(cd apps/layer-android && ./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug "-PcapyAbi=$CAPY_TEST_ABI")
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/debug/app-debug.apk
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb -s "$CAPY_ANDROID_SERIAL" shell am instrument -w -e class art.capycanvas.AndroidWorkspaceSwitcherTest art.capycanvas.test/androidx.test.runner.AndroidJUnitRunner
```

Use `ClassName#methodName` in the fully qualified test selector for one regression.
`AndroidRasterTest#portablePhotoGainmapDelivery` takes
`-e photoDirectory /data/local/tmp/capy-portable-photo`. Push the shared
`p3-grid-8bit.heic`, `p3-gray-10bit.heic` and `p3-12bit.avif` fixtures, plus
`web-hdr.jpg` and `web-hdr.avif` produced by Web's `--portable-photo` test, into
that directory. It checks the real gain-map export dialog, preview switching,
provider save, HDR reopen, unchanged master, presets and cancellation. For an
isolated installation, build with `-PcapyApplicationId=art.capycanvas.portablephoto`
and use `art.capycanvas.portablephoto.test` as the instrumentation package.

[`AndroidTitleBarTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidTitleBarTest.kt)
covers the shared title-bar editor, compact menus, native input and keyboard
focus, Sketch drawers, feedback and persistence. It isolates workspace and
recovery stores and restores preferences; see its [tablet acceptance record](title-bar-android-acceptance.md).
`AndroidInteractionTest#detachedPanelsKeepBodiesAndWiderResizeTargets` covers
mouse/finger/stylus panel and group tear-off, fixed-size clipped native allocation,
compact and scrolling release heights, squashed and usable sidebar heights,
footer anchors, widened resize targets, cancellation, and undo/redo. Lazy lists
report fixed controls and full row-count height without realizing every row.

Read the instrumentation result (`OK` or `FAILURES`); the shell exit status alone
does not establish success. Start with
[`AndroidInteractionTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidInteractionTest.kt)
for drawers and drag geometry, or
[`AndroidWorkspaceManagerTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidWorkspaceManagerTest.kt)
and [`AndroidWorkspaceSwitcherTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidWorkspaceSwitcherTest.kt)
for persistence, menus and window lifecycle. Workspace-manager tests use isolated
SQLite stores; other tests can edit live document/settings state, so save user work
before running them. Do not uninstall the app or clear its storage to reset a test.

```bash
mkdir -p artifacts/android
adb -s "$CAPY_ANDROID_SERIAL" logcat -d -v threadtime > artifacts/android/logcat.txt
adb -s "$CAPY_ANDROID_SERIAL" exec-out screencap -p > artifacts/android/screen.png
adb -s "$CAPY_ANDROID_SERIAL" pull /sdcard/Android/data/art.capycanvas/files/validation artifacts/android/
adb -s "$CAPY_ANDROID_SERIAL" shell am start -n art.capycanvas/.MainActivity
```

The validation directory contains captures from tests that produce them. Trace
input in [`WorkspaceInput.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceInput.kt)
and [`WorkspaceRows.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceRows.kt),
then host publication in [`CanvasHost.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasHost.kt).
Shared drag/drop policy and history live in `crates/layer-ui`; workspace storage
and ownership live in `crates/layer-workspace`. Android uses Bionic `flock` for
workspace liveness because `std::fs::File::try_lock` is unsupported on this
target. The file stays open for the claim and releases its lock on close;
database owner/epoch/fence validation still governs writes.
`AndroidWorkspaceOwnershipTest` verifies independent native sessions cannot
take an active workspace and can acquire it after its owner closes.

`Native.modelUpdate` sends the paths that changed since the last full model
([`model_update.rs`](../../crates/layer-host/src/model_update.rs)). Arrays of
unchanged length are diffed element by element, with a decimal index as the path
segment. An object whose keys change, such as a layout node switching variant,
is sent whole, so every object keeps a full model's key order. The header's
primary menu carries only its title, and `primaryMenu()` in `WorkspaceHeader.kt`
rebuilds its sections from `application_menus`. A single command-availability
change, such as Select All, is about 1.5 KB. On the owner thread, Rust still
builds and serializes the full 178 KB snapshot and splits it by top-level field
before diffing. That takes about 0.7 ms per publication on a desktop and 6–9 ms
on the MovinkPad 11. The follow-up is to rebuild and re-serialize only the
entries whose input revisions changed.

`CanvasHost` applies each model update on the native owner thread. Replaced
values reuse every part the previous model repeats, so unchanged objects and
array elements keep their identity. The main thread then assigns the model into
[`ObservedModel`](../../apps/layer-android/app/src/main/java/art/capycanvas/ObservedModel.kt).
Compose observes the published root, `state` and `state.document_file` one key
at a time, so a publication recomposes only the scopes that read a changed
key. Composables with unchanged arguments skip.

Camera, workspace layout and command search messages leave the update baseline
alone on both sides. Rust keeps diffing against its last full model. The owner's
model in `CanvasHost` stays equal to that baseline, and those messages patch
only the observed model. The next model update therefore carries every value
that differs from the baseline, including ones a message changed in between.

Interaction tests dispatch typed mouse/touch/stylus `MotionEvent`s through native
views; `AndroidInteractionTest` optionally accepts `-e systemInput true` where OS
injection is supported. Neither substitutes for physical pen testing. When adding
held-contact tests, use `runOnMainSync` and bounded condition polling: global idle
waits can hang during a held gesture. Test focus loss with a real window and send
keyboard events through system dispatch so Android leaves touch mode correctly.

`AndroidInteractionTest#menuBodyAndExtendedTabDropsAcrossDevices` covers
menu-bar insertion above expanded columns and collapsed stacks, body prepending,
and enlarged tab targets in first and lower groups. It uses mouse/finger/stylus
contacts, both column sides and themes, all supported payloads, cancellation and
one-step undo/redo. Body-preview captures are in `validation/layout-drops`.
Nested columns remain expanded; only top-level columns can collapse. New stacks
open whole columns by default, so compact-drawer test fixtures opt in explicitly.

The Color panel retains its native hue-ring brush by shape; its shader tracks
physical drawing size. Color changes and overlapping panel motion reuse it. The
color-field bitmap remains cached by shape, hue and size.
`AndroidColorPanelTest` checks rendering and picking across shapes and sizes.

Panel groups, collapsed columns, drawers and the canvas action bar draw through
`panelSurface` in [`PanelShadow.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/PanelShadow.kt).
Each is an offscreen Compose layer enlarged by its shadow reach
(`PanelShadowReach` × elevation). The layer draws the native elevation shadow
once, clears the panel interior so translucent fills cannot reveal it, and then
draws the content clipped to the panel outline. HWUI re-renders a layer only when
its content changes. A moved panel, or a frame drawn for some other change,
composites the cached texture instead. If HWUI drops layers after a memory trim,
it re-renders them on the next frame. In the Paint workspace the panel layers
hold about 8.5 MB of GPU memory, plus 0.6 MB while the canvas bar is shown. The
Navigator clears its overview opening
after its layer is composited (`PanelOpening`), so the live overview in the
SurfaceView still shows through. Command Search is not in a cached layer; its
shadow comes from a shadow texture that is recorded once.

Caching matters because of how this GPU driver presents frames. The MovinkPad 11
(MT8781, Mali-G57) EGL driver offers neither `EGL_EXT_buffer_age` nor an
`EGL_SWAP_BEHAVIOR_PRESERVED` config. Every process logs
`Unable to match the desired swap behavior`. As a result, HWUI redraws the whole
2200 × 1440 window on every frame. Any chrome that is not cached therefore
repeats its full draw on every frame, including the squircle clip masks, which
Skia rasterizes on the CPU and uploads as textures.

Test coverage:

- `AndroidPanelShadowTest` compares hardware-rendered interiors with and without
  shadows across fills, corner shapes, sizes and elevations. It checks that
  exterior shadows and content remain visible, and that cached layers match
  direct drawing.
- `AndroidInteractionTest#cachedPanelsMatchDirectDrawing` compares whole-window
  captures with `PanelLayers.cached` on and off. It covers light and dark, every
  transparency level, the docked layout, a floating group, a collapsed column
  with its drawer, the Navigator, Zen, and the state after `send-trim-memory`.
  It saves `dumpsys gfxinfo` beside the captures in `validation/panel-layers`.
  Samples must match within 2/255. Skia can rasterize anti-aliased path edges
  slightly differently in a layer and in the window, so a sample whose 3 × 3
  neighbourhood spans 64 levels or more may differ by up to 8/255. The Layers
  panel's more icon shows 4/255 on one edge pixel of two of its dots, at the
  same positions.

For measured overlap motion, build the release-based benchmark variant and run:

```bash
(cd apps/layer-android && ./gradlew :app:assembleBenchmark :app:assembleBenchmarkAndroidTest "-PcapyAbi=$CAPY_TEST_ABI" -PcapyBenchmark)
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/benchmark/app-benchmark.apk
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/androidTest/benchmark/app-benchmark-androidTest.apk
adb -s "$CAPY_ANDROID_SERIAL" shell am instrument -w -e workspaceBenchmark true -e workspaceTransparency 3 -e class art.capycanvas.AndroidWorkspacePerformanceTest#colorPanelOverlapFrameTiming art.capycanvas.test/androidx.test.runner.AndroidJUnitRunner
adb -s "$CAPY_ANDROID_SERIAL" logcat -d -s CapyDragPerf:I
```

This uses the real display clock and Android `FrameMetrics`, with typed native
mouse/touch input over a visible Color wheel. It asserts retained UI models and
matching placement. The benchmark variant enables measurement without making
the app debuggable. Reinstall the debug APK afterward for ordinary development.
`workspaceTransparency` accepts 0–3 (off through high); the test restores the
previous setting afterward. The same option applies to `continuousDragFrameTiming`
and `continuousResizeFrameTiming`.

`AndroidCanvasBarBenchmarkTest` measures the canvas action bar on a 6000 × 4000
canvas with the same benchmark APKs: pen strokes, a 24-megapixel photo placement
and a full-canvas selection transform, each with stylus handle drags, contact
taps that hide and return the bar, and bar show/hide alone. Run it with
`-e canvasBarBenchmark true`; `-e scenarios ui,paint,photo,selection`, `durationMs`,
`width`, `height` and `transparency` narrow or resize the run. The `ui` scenario
uses a 2048 × 1536 document to isolate UI frames: bar show/hide, bar moves, show/hide
in Zen and a plain Tool Options change. The `photo` scenario also repeats short
placement and pixel transform drags, each marked by a `capy-drag` trace section, so a
Perfetto trace shows every drag start and release. The `capy.publish.native` and
`capy.publish.parse` trace sections time each publication on the owner thread, and
`-e composeTrace true` adds a trace section for every composable, which slows the
frames it attributes. Each scenario logs one `CapyBarPerf` line and writes
`canvas-bar-benchmark/<label>.json` with the renderer frame rows, GPU completions
and UI `FrameMetrics` percentiles.
`AndroidInteractionTest#canvasActionBarJourneysAcrossDevices` covers the bar's
mouse, finger and stylus behavior and saves light and dark captures in
`validation/canvas-bar`.
`AndroidInteractionTest#canvasNoticesExplainRefusalsAcrossDevices` covers the
notice with mouse, finger and stylus. Fingers navigate the canvas, so the finger
pass makes its tool gestures with the pen and uses the finger for the notice.
It checks the Wand's reference offer and its action, the canvas rendering
afterwards, Move on a locked layer without a dialog or focus change, a repeated
refusal, dismissal by contact and by timeout, clearance above a bottom-edge bar,
and a disabled bar item's reason on tap and hold. It saves light and dark
captures in `validation/canvas-notice`.

The [61 MP Filters memory investigation](../history/filter-preview-tablet-memory-2026-09-17.md)
records the shared source-probe texture reuse fix, tablet measurements, and
remaining preview/display memory-budget work.

The [shared filter preview scheduling record](../history/filter-preview-scheduling-2026-09-17.md)
records the Rust lifecycle, 61 MP latency and drawing responsiveness measurements,
platform handoff and remaining memory work.

The focused physical-device regressions are in `AndroidRasterTest`:
`largePhotoFilterPreviews`, `largePhotoFilterPreviewDrawing` and
`largePhotoFilterPreviewLifecycle`. They are opt-in via `-e filterPhoto true` and
`-e filterDrawing true`. Place the photo at the test app's private
`files/filter-memory-test.jpg`. Use a separate app ID with
`-PcapyApplicationId=art.capycanvas.filtertest -PcapyAppLabel="Capy Filter Test"`;
install both matching APKs and target `art.capycanvas.filtertest.test` for
instrumentation. Reports are in its external-files directory. Production app
storage must not be cleared to prepare these tests.

## Brush workload benchmark

`BrushBenchmarkInstrumentation` draws with OS-injected stylus input on the
9504×6336 photo in release code. Build `:app:assembleBenchmark -PcapyAbi=arm64-v8a
-PcapyOptimize -PcapyApplicationId=art.capycanvas.brushbench`, install it, and push
the photo to `/data/local/tmp/capy-brush-photo.jpg`. Then
`python3 tools/performance/android-brush-benchmark.py OUT --serial "$CAPY_ANDROID_SERIAL" --presets 1 --size 1000`
passes `-e preset`, `-e brushSize` and `-e mode` (`constant`, `pressure`, `tilt`,
`stationary`, `lifts`, `visual` or `pinch`); `--trace` and `--profile` add
Perfetto and simpleperf captures. `python3 tools/performance/android-brush-report.py OUT`
summarizes completed canvas updates per second.
