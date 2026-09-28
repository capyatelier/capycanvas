# Android development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Android client uses Kotlin and Jetpack Compose for the editor UI. A Rust JNI
bridge connects it to `NativeHost`, and the shared Vulkan renderer presents into
a `SurfaceView`. The canvas needs Vulkan shared-demand presentation with
swapchain-maintenance present fences; a driver without them shows a canvas
initialization error instead of a canvas.

## Prerequisites

Install Java 17 or newer, Node.js, Rust, Android Studio or the Android command-line tools,
and these SDK packages:

- Android SDK platform 37 and Build Tools 37.0.0.
- NDK 29.0.14206865 and Platform Tools.
- The Android Emulator and a tablet system image if testing without a device.

The exact versions are set in
[`app/build.gradle.kts`](../../apps/layer-android/app/build.gradle.kts). Install
these through Android Studio's SDK Manager. Set `ANDROID_HOME` if the SDK is not
at `$HOME/Android/Sdk`; the launcher and the Rust build use that path by default.

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

The launcher starts the configured emulator if no device is connected, builds
the debug APK for the device's ABI, installs it with `adb install -r` and opens
the app. It targets `CAPY_ANDROID_SERIAL`, default `emulator-5554`.
`run.sh headless` starts the emulator without a window. `run.sh` and
`run.sh headless` install the default application ID `art.capycanvas`.

`run.sh test` runs every instrumented test through
`:app:connectedDebugAndroidTest`. Gradle uninstalls the app and its test APK
after the run, so `run.sh test` builds them under an
[isolated application ID](#isolated-installs): `$CAPY_APPLICATION_ID` when set,
otherwise `tools/devices/devices.py appid`. It never installs or removes
`art.capycanvas`.

The Gradle wrapper supplies Gradle and builds the Rust library through
`cargo-ndk`. Direct Gradle builds default to both ABIs; use
`-PcapyAbi=arm64-v8a` or `-PcapyAbi=x86_64` for one device.

Debug APKs use the `dev-perf` Rust profile: release optimization level 3,
incremental compilation, 16 codegen units and line tables for source-level
profiling. Release and benchmark APKs use `release`. Set `CAPY_RUST_PROFILE`
or pass `-PcapyRustProfile=release` to Gradle to override the selection; the Gradle
property takes precedence. Each variant has its own JNI output directory, and
profile/ABI changes invalidate its Rust task.

### Isolated installs

`-PcapyApplicationId=<id>` builds the app under a separate package, and
`-PcapyAppLabel=<label>` gives it its own launcher name. Its instrumentation
package is `<id>.test`. On a shared tablet use your own ID:
`tools/devices/devices.py appid` prints it, and [`devices.py run`](devices.md)
exports it as `$CAPY_APPLICATION_ID`. The commands below write `art.capycanvas`
for the single-user case.

## Device tests

Without a device, build both APKs and run lint:

```bash
(cd apps/layer-android && ./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug -PcapyAbi=arm64-v8a)
```

On a device, run from the repository root with the SDK's `platform-tools` on
`PATH`, USB debugging enabled and the computer authorized. On a shared tablet,
reserve it and run each device command through `tools/devices/devices.py run`
([Devices](devices.md)). Select the device, then build and install without
clearing data:

```bash
adb devices -l
export CAPY_ANDROID_SERIAL=DEVICE_SERIAL
CAPY_TEST_ABI=$(adb -s "$CAPY_ANDROID_SERIAL" shell getprop ro.product.cpu.abi | tr -d '\r')
(cd apps/layer-android && ./gradlew :app:assembleDebug :app:assembleDebugAndroidTest "-PcapyAbi=$CAPY_TEST_ABI")
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/debug/app-debug.apk
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb -s "$CAPY_ANDROID_SERIAL" shell am instrument -w -e class art.capycanvas.AndroidWorkspaceSwitcherTest art.capycanvas.test/androidx.test.runner.AndroidJUnitRunner
```

`DEVICE_SERIAL` is the value shown by `adb devices`. Use
`ClassName#methodName` in the fully qualified selector for one test, and pass
test arguments with `-e name value`. Gradle can run the same selection with
`:app:connectedDebugAndroidTest -Pandroid.testInstrumentationRunnerArguments.class=...`
and `ANDROID_SERIAL` set, but it uninstalls the APKs afterwards, so combine it
with an isolated application ID on a tablet.

Read the instrumentation result: `OK (N tests)` or `FAILURES!!!`. `am instrument`
can exit 0 when tests fail, so the exit status proves nothing. Opt-in tests skip
through JUnit assumptions when their `-e` argument is missing, so confirm that
the test ran.
[Testing](testing.md#known-failures-on-main) lists the tests that fail on `main`.

Emulator runs check logic and layout. Physical stylus accuracy, thermal
behavior and high-refresh presentation need a real tablet.

### Where to start

- [`AndroidInteractionTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidInteractionTest.kt):
  drawers, drag geometry, panels, the canvas action bar, notices, the zoom
  readout and effects, each with mouse, finger and stylus. It dispatches typed
  `MotionEvent`s through the native views; `-e systemInput true` uses OS
  injection where the device allows it.
- [`AndroidWorkspaceManagerTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidWorkspaceManagerTest.kt)
  and [`AndroidWorkspaceSwitcherTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidWorkspaceSwitcherTest.kt):
  persistence, menus and window lifecycle. `AndroidWorkspaceOwnershipTest`
  checks that a second native session cannot take an active workspace.
- [`AndroidTitleBarTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidTitleBarTest.kt):
  the title-bar editor, compact menus, keyboard focus, Sketch drawers and
  persistence.
- `AndroidRasterTest`: document, file and GPU lifecycle.
  - `#navigationBuffersAndPenReturnsToFrontBuffer` and
    `#frontBufferSurfaceLifecycle` cover the switch between buffered navigation
    and front-buffer ink, rotation, surface recreation and GPU recovery.
  - `#hdrDisplayNegotiation` takes `-e hdrFile <device path of a PQ PNG>`;
    `-e requireHdr true` fails instead of passing on an SDR-only display.
  - `#portablePhotoGainmapDelivery` takes
    `-e photoDirectory /data/local/tmp/capy-portable-photo`. Push the shared
    `p3-grid-8bit.heic`, `p3-gray-10bit.heic` and `p3-12bit.avif` fixtures, plus
    `web-hdr.jpg` and `web-hdr.avif` produced by Web's `--portable-photo` test,
    into that directory.
  - `#largePhotoFilterPreviews` and `#largePhotoFilterPreviewLifecycle` opt in
    with `-e filterPhoto true`, `#largePhotoFilterPreviewDrawing` with
    `-e filterDrawing true`. They read the app's private
    `files/filter-memory-test.jpg` and write reports to its external-files
    directory.

### Test data

`CapyDeviceRule` gives a test its own workspace, recovery and color stores
under the app cache, and restores the user's preferences afterwards. Tests
without it can edit the live document and settings, so save work before running
them. Never uninstall the app or clear its storage to reset a test; use an
isolated application ID instead.

Tests that produce captures write them to the app's external
`files/validation/` directory. `AndroidHostTest` and `AndroidShortcutsTest`
also save images under `Pictures/` through MediaStore.

### Writing device tests

- Poll with `CanvasHost.awaitMain`, which checks a condition in `runOnMainSync`
  until a timeout, and `CanvasHost.drain`, which waits for native publication.
  Global idle waits can hang while a gesture is held.
- Between consecutive popup or held-contact journeys, wait for native window
  focus to settle.
- Test focus loss with a real window. Send keyboard events through system
  dispatch so Android leaves touch mode correctly.
- `PixelCopy` cannot read the canvas: its shared front-buffer image stays
  acquired. Debug and benchmark builds give the swapchain copy usage, so tests
  read the retained image directly or take a compositor screenshot. Release
  builds cannot be read back.
- `awaitMain` defaults to a 10 s timeout. A cold shader cache on a slow tablet
  can take longer to start; see [Devices](devices.md).

## Benchmarks

The `benchmark` build type inherits `release`, is not debuggable, and is signed
with the debug key. `-PcapyBenchmark` makes it the build type of the test APK.
Make performance decisions with it, never with a debug build. Reinstall the
debug APK afterwards for ordinary development.

```bash
(cd apps/layer-android && ./gradlew :app:assembleBenchmark :app:assembleBenchmarkAndroidTest "-PcapyAbi=$CAPY_TEST_ABI" -PcapyBenchmark)
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/benchmark/app-benchmark.apk
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/androidTest/benchmark/app-benchmark-androidTest.apk
```

`-PcapyOptimize` also enables R8 minification. Use it only for the
self-instrumenting runners in `app/src/benchmark` (`BrushBenchmarkInstrumentation`
and `UiStartupInstrumentation`). R8 can remove methods that only a separate test
APK calls, so test-APK benchmarks use the unminified build.

- **Workspace motion.** `AndroidWorkspacePerformanceTest` runs with
  `-e workspaceBenchmark true`. `#colorPanelOverlapFrameTiming`,
  `#continuousDragFrameTiming` and `#continuousResizeFrameTiming` use the real
  display clock and `FrameMetrics`. `-e workspaceTransparency 0`–`3` sets panel
  transparency (off to high) and restores it afterwards. Results appear in
  logcat under `CapyDragPerf` and `CapyResizePerf`.
- **Canvas action bar.** `AndroidCanvasBarBenchmarkTest` runs with
  `-e canvasBarBenchmark true`. `-e scenarios ui,paint,photo,scaled,selection,menus,canvas_size,refine,crop`,
  `durationMs`, `width`, `height` and `transparency` (`off` to `high`) narrow or
  resize the run. Each scenario logs a `CapyBarPerf` line and writes
  `canvas-bar-benchmark/<label>.json` in external files; `ui_hz` and
  `ui_interval_ms` count the distinct vsyncs the UI drew until the gestures end.
  `refine` drags the Refine panel's Feather slider with the stylus on a
  2048 × 1536 document and at `width` × `height`, and logs the values sent and
  previews drawn. `-e refine grow` (or `shrink`, `border`) picks another
  operation, `-e refineSpan` the fraction of the track, and `-e refineBar off`
  hides the canvas action bar. `crop` drags a crop handle over a placed photo.
  Photo drags are marked by `capy-drag` trace sections; `capy.publish.native` and `capy.publish.parse`
  time each model publication; `-e composeTrace true` adds a section per
  composable, which slows the frames it attributes.
- **Canvas navigation and drawing.** `AndroidViewportBenchmarkTest` runs with
  `-e viewportBenchmark true`. `-e motion pan|pinch` measures navigation;
  the default `stroke` draws, with `osInput`, `canvasSize`, `brushSize`,
  `intervalMs`, `durationMs`, `repeats` and `label`. Pull
  `files/viewport-benchmark/` from the app's external storage and summarize it
  with `python3 tools/performance/android-viewport-report.py DIRECTORY`.
- **UI startup.** With an `-PcapyOptimize` build under an isolated ID, run
  `adb shell am instrument -w -e uiStartupAudit true -e auditLabel LABEL <id>/art.capycanvas.UiStartupInstrumentation`.
  It records first draw, Settings responses, UI frame intervals and publication
  costs in external `files/ui-startup-audit/`.
- **Pen workflow traces.** `tools/performance/AndroidPenMotion.java` injects a
  200 Hz stylus ellipse from the shell and prints `CLOCK_BOOTTIME` action
  markers. Compile it with `javac` against `platforms/android-37.0/android.jar`
  and `d8 --min-api 29`, push the dex, and run
  `CLASSPATH=<dex> app_process / AndroidPenMotion CX CY RX RY workflow [turns/s] [seconds]`
  while Perfetto records. `tools/performance/android-pen-report.py TRACE MARKERS --processor TRACE_PROCESSOR`
  reports CPU scopes, GPU observations and canvas latches per action.

### Brush workload benchmark

`BrushBenchmarkInstrumentation` draws with OS-injected stylus input on a photo in
release code. Build `:app:assembleBenchmark -PcapyAbi=arm64-v8a
-PcapyOptimize -PcapyApplicationId=art.capycanvas.brushbench`, install it, and push
the 9504×6336 photo to `/data/local/tmp/capy-brush-photo.jpg`. `--photo` selects
another JPEG under `/data/local/tmp`, such as a
[performance tier](../PERFORMANCE_TARGETS.md) canvas; the canvas takes the photo's
size. Then
`python3 tools/performance/android-brush-benchmark.py OUT --serial "$CAPY_ANDROID_SERIAL" --presets 1 --size 1000`
passes `-e preset`, `-e brushSize` and `-e mode` (`constant`, `pressure`, `tilt`,
`stationary`, `lifts`, `visual` or `pinch`); `--trace` and `--profile` add
Perfetto and simpleperf captures. The default preset list is the dry brushes;
`--presets` accepts every built-in preset, including wet, smudge and Liquify.
`python3 tools/performance/android-brush-report.py OUT` summarizes completed
canvas updates per second, the rate the performance targets use for brushes.
Run directly, the instrumentation also accepts `-e navigationBetweenStrokes true`
and `-e navigationSettleMs` to measure the handoff from navigation back to ink.
`-e colorBeforeStrokes true` changes the quick paint color before each stroke
and checks that the prepared brush remains ready.
[Measuring performance](../performance/measuring.md) has the tier rules.

## Debugging

```bash
mkdir -p artifacts/android
adb -s "$CAPY_ANDROID_SERIAL" logcat -d -v threadtime > artifacts/android/logcat.txt
adb -s "$CAPY_ANDROID_SERIAL" exec-out screencap -p > artifacts/android/screen.png
adb -s "$CAPY_ANDROID_SERIAL" pull /sdcard/Android/data/art.capycanvas/files/validation artifacts/android/
adb -s "$CAPY_ANDROID_SERIAL" shell am start -n art.capycanvas/.MainActivity
```

Where to look:

- Input: [`CanvasSurfaceView.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasSurfaceView.kt)
  for the canvas, [`WorkspaceInput.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceInput.kt)
  and [`WorkspaceRows.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceRows.kt)
  for chrome drags. Shared drag/drop policy and history live in `crates/layer-ui`.
- Publication: [`CanvasHost.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasHost.kt)
  runs the native session on its `capy-canvas` thread and applies model updates
  into [`ObservedModel`](../../apps/layer-android/app/src/main/java/art/capycanvas/ObservedModel.kt).
  `CanvasHost.actionError` holds the open error dialog's text or the refusal
  being shown; tests check it.
- Presentation: [`native/src/android.rs`](../../apps/layer-android/native/src/android.rs)
  owns the Vulkan surface and its present-mode switches. `Native.displayStatus`
  reports the present mode, frame latency, surface format and color space, and
  submitted and completed frames. A submitted frame is not proof that Android
  displayed it.
- Files: [`Documents.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Documents.kt)
  uses the Storage Access Framework. Providers control their own destination
  behavior, so atomic replacement cannot be assumed for every URI.
- Workspaces: `crates/layer-workspace` stores them. Android takes the
  workspace liveness lock with Bionic `flock`, because `std::fs::File::try_lock`
  is unsupported on this target.
- Panel rendering: `panelSurface` in [`PanelShadow.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/PanelShadow.kt);
  `AndroidPanelShadowTest` compares panels with and without shadows.

If the canvas stops updating while menus still respond, keep the app running.
Record a Perfetto trace with the SurfaceFlinger frame timeline during the
gesture, and compare canvas submissions with the canvas `SurfaceView`'s latches
and the UI layer's. Restarting the app hides the cause. Do not add device-idle
waits or reconfiguration loops before the missing completion or consumption
signal is found.
