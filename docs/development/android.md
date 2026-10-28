# Android development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Android client uses Kotlin and Jetpack Compose for the editor UI. A Rust JNI
bridge connects it to `NativeHost`, and the shared Vulkan renderer presents into
a `SurfaceView`. Ink uses Vulkan shared-demand presentation when the driver has
swapchain-maintenance present fences, and buffered FIFO presentation otherwise
([Android presentation](../platforms/README.md#android-presentation)).

The app uses no Google Play services. Distribution APKs must also omit Play's
injected installer and licence checks; the [release workflow](releasing.md#android-apk)
verifies this while retaining the signing identity for updates.

The native Gradle build tracks shared Rust, filter assets and Fluent catalogs as
inputs. Catalog-only changes rebuild the embedded native UI text.

## Storage

External photos decode in `PhotoReaderService`, an isolated process with no
storage, network or permissions. `PhotoReader` binds it from the import worker
thread and passes descriptors for an input copy, the output package and
diagnostics; a disconnected or failed service fails the import.

`AppStorage` passes the platform folders to shared Rust, and
`layer_host::StorageRoots` names each store within them:

| Kind | Folder | Contents |
| --- | --- | --- |
| Settings | `SharedPreferences` `capy-canvas` | the shared settings |
| Config | `filesDir` | `export-presets` |
| Data | `filesDir` | `workspaces/` with palettes, `color-profiles/` |
| State | `noBackupFilesDir` | `sessions/` |
| Cache | `cacheDir` | `shaders/` |
| Temp | `cacheDir/temp` | package spools, parked tile spills, staged saves and imports, `clipboard/` |

The canvas worker resolves the folders before the native session starts. The
first resolution in a process empties `temp`; Android runs one process per app.
Rust temporary files have no name once open, and Kotlin staging files are deleted
when their job ends. The latest copied image keeps its name in `temp/clipboard`
so other apps can paste it until Capy Canvas starts again.

Auto Backup and device transfer copy the settings and `filesDir`, except the
workspace database's `-shm` index and `-locks` folder. Android never backs up
`noBackupFilesDir` or `cacheDir`, so sessions, caches and temporary files stay on
the device. `hasFragileUserData` lets the user keep the data when uninstalling.

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
For command-line installation, the API 37 platform package ID is
`platforms;android-37.0`.

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
`run.sh headless` install the development application ID `art.capycanvas.dev`.

`run.sh test` runs every instrumented test through
`:app:connectedDebugAndroidTest`. Gradle uninstalls the app and its test APK
after the run, so `run.sh test` builds them under an
[isolated application ID](#isolated-installs): `$CAPY_APPLICATION_ID` when set,
otherwise `tools/devices/devices.py appid`. It never installs or removes
`art.capycanvas` or `art.capycanvas.dev`.

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

Release builds use the Google Play application ID `art.capycanvas.editor`.
Debug and benchmark builds use `art.capycanvas.dev`, so a development build never
replaces the release app or reads its data. `-PcapyApplicationId=<id>` sets the ID of every
build type, and `-PcapyAppLabel=<label>` gives it its own launcher name. Its
instrumentation package is `<id>.test`. On a shared tablet use your own ID:
`tools/devices/devices.py appid` prints it, and [`devices.py run`](devices.md)
exports it as `$CAPY_APPLICATION_ID`. The commands below write
`art.capycanvas.dev` for the single-user case.

### Changing the host

- **JNI results.** `or_throw` in `native/src/android.rs` reports
  `IllegalStateException` and keeps the return sentinel; `argb_array` packs RGBA
  pixels for Kotlin. Every JNI export uses `jni_entry!`; VM loading returns
  `JNI_ERR` on failure, and the photo service writes failures to its error pipe.
- **JNI handles.** JNI `Long` handles are typed generational tokens from
  `layer_host::handles`. Apps stay in the creating render thread's table, and
  worker operations hold a task lease for the whole call. Malformed, stale,
  wrong-kind and busy handles report ordinary JNI errors.
- **File work.** Run file and conversion jobs through `inspection::on_worker`
  from an IO worker; it names the thread and gives it an 8 MiB stack. JNI file,
  image-import and clipboard tasks look up their pending request with
  `UiSession::document_request` before validating its kind. A prepared
  drawing-close job stays allocated until `documentReleasePreparedClose`, which
  the controller calls in its non-cancellable cleanup.
- **Languages.** `res/xml/locales_config.xml` advertises only the shared shipped
  languages; `native/src/launch_tests.rs` fails when they drift. Follow the
  [localization guide](../ui/localization.md) to add one.

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
adb -s "$CAPY_ANDROID_SERIAL" shell am instrument -w -e class art.capycanvas.AndroidWorkspaceSwitcherTest art.capycanvas.dev.test/androidx.test.runner.AndroidJUnitRunner
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
the test ran. [Testing](testing.md#known-failures-on-main) lists the tests that
fail on `main`.

Emulator runs check logic and layout. Physical stylus accuracy, thermal
behavior and high-refresh presentation need a real tablet.

### Where to start

Test classes live in `app/src/androidTest/java/art/capycanvas/` and are named
for their area, such as `AndroidColorPanelTest` or `AndroidVectorLayerTest`.
The broad ones:

| Class | Covers |
| --- | --- |
| `AndroidHostTest` | Panels and tabs, Properties pages, gradients, pen calibration, multi-finger gestures |
| `AndroidInteractionTest` | Drawers, drag geometry, the canvas action bar, notices, the zoom readout, effects and retouching, each with mouse, finger and stylus |
| `AndroidTitleBarTest` | The title-bar editor, compact menus, keyboard focus, Sketch drawers, the Layers/Properties drawer |
| `AndroidRasterTest` | Document, file and GPU lifecycle: saving, recovery, image objects, presentation modes, HDR, filters |
| `AndroidToolBubblesTest` | Tool bubbles with native mouse, touch and stylus; run it on both reference tablets after changing the presenter or capture path |
| `AndroidWorkspaceManagerTest`, `AndroidWorkspaceSwitcherTest`, `AndroidWorkspaceOwnershipTest` | Workspace persistence, menus, switching and window ownership |
| `AndroidLanguageTest`, `AndroidTextCompositionTest` | Language changes and native text composition |
| `AndroidFeatureParityTest`, `AndroidPortParityTest` | Features shared with the other hosts |
| `AndroidNativeHandlesTest` | JNI handle validation, without a GPU surface |

`AndroidHostTest`, `AndroidRasterTest` and `AndroidTextCompositionTest` accept
`-e presentationNarrow true` (portrait, at most 640 dp) or
`-e presentationWide true` (landscape); run layout-dependent tests in both.
These tests need other arguments:

| Test | Arguments |
| --- | --- |
| `AndroidFirstUiTest#unsupportedGpuExplainsTheDeviceInTheCanvasArea` | `-e unsupportedGpu true`, on an emulator started with `-gpu host -feature -Vulkan` |
| `AndroidToolBubblesTest#sketchEraserBubbleKeepsItsHaloChooserAndSettingsAcrossThemesAndDevices` | `-e bubbleTheme light\|dark -e bubbleInput mouse\|touch\|pen` for one variant |
| `AndroidInteractionTest`, `AndroidColorPanelTest` | `-e systemInput true` uses OS input injection where the device allows it |
| `AndroidFeatureParityTest#paperLibraryImportsRenamesAndDeletesWithoutChangingTheDrawing` | `-e theme light` or `dark` |
| `AndroidRasterTest#sharedImageObjectsKeepPosePixelsAndIdentityAcrossFilesAndRecovery` | `-e objectArchive /data/local/tmp/capy-object-ga-functional.capy`, made from a small photo by `object_fixture` ([brush benchmark](#brush-workload-benchmark)); `-e theme light` and `dark` |
| `AndroidRasterTest#hdrDisplayNegotiation` | `-e hdrFile <device path of a PQ PNG>`; `-e requireHdr true` fails instead of passing on an SDR-only display |
| `AndroidRasterTest#portablePhotoGainmapDelivery` | `-e photoDirectory /data/local/tmp/capy-portable-photo`, holding the shared `p3-grid-8bit.heic`, `p3-gray-10bit.heic` and `p3-12bit.avif` fixtures and the `web-hdr.jpg` and `web-hdr.avif` that Web's `--portable-photo` test produces |
| `AndroidRasterTest#largePhotoFilterPreviews`, `#largePhotoFilterPreviewDrawing` | `-e filterPhoto true` or `-e filterDrawing true`; they read the app's private `files/filter-memory-test.jpg` and write reports to its external-files directory |
| `AndroidRasterTest#spatialFilterWindowsKeepPaintAndHistory` | `-e spatialPhoto /data/local/tmp/capy-tier-24mp.jpg` |

`AndroidSessionRestartTest` kills its own process. Run
`#processRestartRetainsSavedAndUnsavedHistory` or
`#processRestartRetainsImageObjectsAndHistory` first with
`-e restartPhase prepare -e restartFixture UNIQUE -e theme light|dark`, which
reports `Process crashed`, then with `-e restartPhase verify -e restartFixture UNIQUE`,
which must report `OK (1 test)`. Only that named test directory survives between
the two runs; verification removes it.

`AndroidLanguageTest#genuineKeyboardCompositionDefersLanguagePublication` takes
`-e genuineIme true` and needs an installed keyboard that produces actual
preedit. Set `genuineImeKeys`, `genuineImePreedit` and `genuineImeCandidate` to
the engine's key sequence, composing text and visible candidate; for Japanese
Gboard QWERTY, use `nihonn`, `にほん` and `日本`. Missing preedit fails the
prerequisite. Record and restore the keyboard languages, layout and selected
system subtype after device setup.

### Test data

`CapyDeviceRule` sets `AppStorage.directoryForTest`, which gives a test its own
settings, workspaces, color stores and sessions under the app cache, and restores
the user's preferences afterwards. Tests share the shader cache and temporary
folder. Tests without the rule can edit the live document and settings, so save
work before running them. Never uninstall the app or clear its storage to reset a
test; use an isolated application ID instead.

Tests that produce captures write them to the app's external
`files/validation/` directory. `AndroidHostTest` and `AndroidShortcutsTest`
also save images under `Pictures/` through MediaStore.

### Writing device tests

- **Poll for conditions.** Use `CanvasHost.awaitMain`, which checks a condition
  in `runOnMainSync` until a timeout (10 s by default), and `CanvasHost.drain`,
  which waits for native publication. Pass the test's Compose rule when it owns
  the frame clock. Global idle waits can hang while a gesture is held. A cold
  shader cache on a slow tablet can outlast the default timeout; see
  [Devices](devices.md).
- **Copy snapshots.** Copy retained `CanvasHost.snapshot` JSON on the main thread
  before reading or editing it; native publication updates those objects in place.
- **Wait for the work you started.** Drawing checks wait for shared canvas/brush
  readiness and no `Native.renderingPending` work, including queued surface
  presentation. `Native.frame` can still request optional shader warmup after the
  drawing is complete. Keep separate completion checks for asynchronous UI
  operations, statistics, readbacks, file workers and recovery. History
  restoration can finish after a command publication fence; wait for the
  expected published document state before asserting Undo or Redo.
- **Settle focus and windows.** Between consecutive popup or held-contact
  journeys, wait for native window focus to settle. Popup contacts choose the
  topmost touchable window containing the screen point, send outside contacts in
  that window's coordinates, and keep the chosen window through release.
  Tooltips can coexist with the active menu.
- **Use real focus and keys.** Test focus loss with a real window. Send keyboard
  events through system dispatch so Android leaves touch mode correctly.
- **Read canvas pixels directly.** `PixelCopy` cannot read the canvas: its shared
  front-buffer image stays acquired. Debug and benchmark builds give the
  swapchain copy usage, so tests read the retained image directly or take a
  compositor screenshot. Release builds cannot be read back.
- **Create drawings with the shared actions.** `CanvasHost.createDrawing` in
  `CapyDeviceTest.kt` (with `orderDrawing`) makes a pixel-sized drawing through
  the shared [New drawing](../ui/new-drawing.md) actions and waits for its New
  request to finish. Use it when a journey only needs a canvas;
  `newDrawingDialogJourneyInBothThemes` walks the dialog itself.

### Rust GPU tests on a device

To isolate a driver shader-compiler failure, build the renderer's
`shader_compile` example with `cargo ndk -t arm64-v8a --platform 29 build
--locked --release -p layer-render-wgpu --example shader_compile`. Copy the executable
and the complete WGSL module to the reserved device through the device wrapper.
Run `shader_compile FILE ENTRY...` to compile named compute entry points without
starting the renderer or its background shader catalog. Each entry logs its
start and successful completion separately.

The shared renderer's Rust GPU tests can also be cross-built with `cargo ndk
-t arm64-v8a --platform 29 test --locked --release -p layer-render-wgpu --lib --no-run`
and run on the reserved device. Headless renderers compile deferred pipelines
as their workload needs them. Timing harnesses must prime each workload over
the full measured motion range before measuring steady motion, including the
most magnified transform pose.

## Benchmarks

The `benchmark` build type inherits `release`, is not debuggable, uses the
development application ID and is signed with the debug key. `-PcapyBenchmark`
makes it the build type of the test APK. Make performance decisions with it,
never with a debug build. Reinstall the debug APK afterwards for ordinary
development.

```bash
(cd apps/layer-android && ./gradlew :app:assembleBenchmark :app:assembleBenchmarkAndroidTest "-PcapyAbi=$CAPY_TEST_ABI" -PcapyBenchmark)
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/benchmark/app-benchmark.apk
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/androidTest/benchmark/app-benchmark-androidTest.apk
```

`-PcapyOptimize` also enables R8 minification. Use it only for the
self-instrumenting runners in `app/src/benchmark` (`BrushBenchmarkInstrumentation`
and `UiStartupInstrumentation`). R8 can remove methods that only a separate test
APK calls, so test-APK benchmarks use the unminified build.

Each benchmark skips unless its opt-in argument is set. Use the tier photo and
follow the [measurement rules](../performance/measuring.md) for gesture counts
and recording results. Renderer submissions, GPU completions, draw
callbacks and `FrameMetrics` attribute work; only the app window's actual
SurfaceFlinger presents measure display cadence. Record diagnostic options that
add work, such as renderer profiling, tracing or memory sampling, separately
from rate runs.

On the MovinkPad 11, installing, force-stopping or starting instrumentation clears
the test app's Display › Adaptive switch. The bubble benchmark's ready/go barrier
lets its `--adaptive-script` helper enable 90 Hz after private fixture setup.

- **Tool bubbles.** `AndroidBubbleBenchmarkTest#fiveBubbleMotion` drags one
  bubble through four others on the tier photo at Fit. Run
  `tools/performance/android-bubbles.py OUTPUT --photo DEVICE_PHOTO --width WIDTH --height HEIGHT`
  through `devices.py run` after installing the benchmark and test APKs under
  the worktree's application ID. `--workload hdr-drag` and `hdr-intensity` use
  the tier photo in an F16 drawing with the color bubble; the latter sweeps the
  EV arc. `--workload palette-opening` repeats palette-bead taps
  (`--palette-transitions 1`–`10`) and reports no sustained-motion rate.
  `--report-only` rebuilds a report from captured data.
- **Workspace motion.** `AndroidWorkspacePerformanceTest` runs with
  `-e workspaceBenchmark true`: `#colorPanelOverlapFrameTiming`,
  `#colorWheelFrameTiming`, `#editColorWheelFrameTiming` (the Edit Color
  dialog's wheel), `#continuousDragFrameTiming` and
  `#continuousResizeFrameTiming`. `-e workspaceTransparency 0`–`3` sets panel
  transparency (off to high) and restores it afterwards. Results appear in
  logcat under `CapyDragPerf` and `CapyResizePerf`. The gap and panel column
  benchmarks also run inside the optimized app with
  `android-viewport-presents.py --workspace-workload gap --workspace-runner self`
  ([workspace columns](../performance/measuring.md)); both entry points share
  `WorkspaceFrameTiming`. Compare optimization settings with the same runner.
- **Scrolling.** `AndroidWorkspacePerformanceTest#workspaceSwitcherScrollFrameTiming`
  with `-e switcherBenchmark true` scrolls the workspace choices and visibility
  checklist; `#groupedDrawerScrollFrameTiming` with `-e groupedToolBenchmark true`
  scrolls the Photo workspace's grouped Drawing drawer. Both take
  `-e photo <readable-tier-photo.jpg>` and write draw samples and raw
  `FrameMetrics` to the `workspace-switcher-benchmark` and
  `grouped-drawer-benchmark` external-files directories. Keep frames whose
  vsync matches a draw where the scroll offset changed, counting each vsync once.
- **Layers.** `AndroidTitleBarTest#layerSwipeFrameTiming` takes
  `-e photo <readable-tier-photo.jpg>`, `-e width`/`-e height` matching it, and
  `-e layerBenchmarkTheme light|dark` (dark by default), plus one workload:
  - `-e layerSwipeBenchmark true` swipes a row; add `-e layerSwipeDirection left`
    for the left action strip.
  - `-e layerReorderBenchmark true` drags a row preview with the mouse. Add
    `-e layerRelationshipBenchmark true` to drag an attached filter through
    clipping connectors.
  - `-e layerSelectionBenchmark true` switches between the empty layer's content
    and mask every 150 ms; `-e layerSelectionIntervalMs 400` lets each transition
    finish first.
  - `-e layerPropertiesBenchmark true` repeats the Layers/Properties footer
    transition.

  Each run writes `layer-swipe`, `layer-reorder`, `layer-selection` or
  `layer-properties` `-<run>.json` to external files with moving-window
  `FrameMetrics`. Check dropped reports before comparing rates, and exclude gaps
  between animation windows.
- **Canvas navigation and drawing.** `AndroidViewportBenchmarkTest` runs with
  `-e viewportBenchmark true`; [navigation](../performance/measuring.md) has the
  SurfaceFlinger runner.
  - `-e width 4248 -e height 2832` selects the low-tier canvas; `-e canvasSize`
    supplies both dimensions when they are omitted. `-e photo
    /data/local/tmp/FILE.jpg` places that photo; `-e openQueryPhoto true` opens it
    at its original dimensions when it is larger than New drawing accepts.
    `-e passThrough true` adds a Pass Through group with a Black & White
    adjustment above a Solid Color fill.
  - `-e motion pan|pinch|rotate` measures touch navigation;
    `-e motion smooth_zoom -e zoomDirection horizontal|vertical` measures the
    Zoom tool with stylus input; `-e motion hover -e cursor tool` or
    `tool_brush_size` moves tool cursors without ink. The default `stroke` draws,
    with `osInput`, `canvasSize`, `brushSize`, `intervalMs`, `durationMs`,
    `repeats`, `blending` and `label`. Set `radiusX`, `radiusY` in surface pixels
    for a contained tier workload.
  - `contactMs` with `pauseMs` runs repeated short contacts as a
    checkpoint/history diagnostic; it does not qualify a continuous stroke.
    `-e languageSwitches ja,zh-Hans,zh-Hant` requests a language halfway through
    each stroke. `-e navigator false` is a separate diagnostic; tier
    measurements keep the default workspace.
  - Pull `files/viewport-benchmark/` from the app's external storage and
    summarize it with `python3 tools/performance/android-viewport-report.py DIRECTORY`.
- **Artwork query workers.** `AndroidArtworkQueryBenchmarkTest#exactSamples`
  runs with `-e artworkQueryBenchmark true`; `#artworkStatistics` uses
  `-e artworkStatisticsBenchmark true -e statisticsMode preview|exact|auto`.
  `-e photo /data/local/tmp/FILE.jpg` opens the original photo at its native
  dimensions. Results and cancellation latency appear in
  `files/artwork-query-benchmark/` or `files/artwork-statistics-benchmark/`. For
  navigation interference, use `AndroidViewportBenchmarkTest` with
  `-e openQueryPhoto true`, `-e artworkQueries true -e queryCount N`, and
  `-e statisticsPreview true`, `-e statisticsExact true` or
  `-e levelsStatistics true`; choose enough requests to cover the input window.
  Use FrameTimeline without graphics tracing for rates; graphics tracing
  consumes renderer timing samples. `-e clippingPreview true` enables both
  shared clipping flags without query workers and checks a clipped frame after
  motion; `-e repeats 0` runs that check alone.
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

### Canvas action bar benchmark

`AndroidCanvasBarBenchmarkTest` runs with `-e canvasBarBenchmark true`
([canvas bar rules](../performance/measuring.md)). Each scenario logs a
`CapyBarPerf` line and writes `canvas-bar-benchmark/<label>.json` in external
files; the directory is cleared at the start of each invocation. `ui_hz` and
`ui_interval_ms` count distinct UI vsyncs until the gestures end;
`gpu_completed_hz` counts completions inside the gesture, including
thumbnail-only refreshes, not distinct poses.

| Argument | Effect |
| --- | --- |
| `-e scenarios ui,paint,photo,composed_transform,scaled,move,selection,menus,canvas_size,refine,crop,merge,dodge_burn,frequency_separation,effects,curves,spatial-effects` | Chooses scenarios |
| `-e labels <comma-separated labels>` | Limits the measured operations, keeping setup and priming |
| `durationMs`, `width`, `height`, `blending` (`perceptual` or `linear`), `transparency` (`off` to `high`) | Resize the run |
| `-e photo <readable-file>` | Uses a JPEG matching `width` and `height`; otherwise the fixture generates an image |
| `-e tierPhoto true` | Puts the reference photo beneath one empty paint layer at Fit for the `photo` journeys |
| `-e zoomOut false` | Keeps Fit zoom |
| `-e project <readable-current-format.capy>` | Opens a prepared project through the ordinary loader |
| `-e memory true`, `-e rendererProfile true` | Record renderer allocation, or open Stats for GPU phase timestamps |
| `-e composeTrace true` | Adds a trace section per composable, beside the `capy-drag`, `capy.publish.native` and `capy.publish.parse` sections |
| `-e panel navigator` | Opens the Navigator; record it separately from runs with the panel closed |

| Scenario | Measures | Options |
| --- | --- | --- |
| `photo` | Body translation, corner resizing, distortion and a warp node drag | `-e acceptedPhoto true -e labels photo-translate-drag` for retained-photo translation, with `-e materialWatercolor true` to paint a wet stroke first; `-e labels photo-retained-distort-drag` or `photo-retained-warp-drag`; `-e translationRepeats 4` keeps the first contact and three more separately; `-e transformSnapping true`; `-e finalBake true` commits a rotation and checks native backing after save/reopen; `-e saveRetainedDiagnostic true` saves `retained-diagnostic.capy` but changes the motion path |
| `composed_transform` | Resizing and distorting a translucent photo over another photo | |
| `effects`, `spatial-effects` | Exposure alone and after Levels/Vibrance; Gaussian Blur at small and large radii | `-e labels paper-scale-drag` scrubs Paper Scale; `-e effectZoom 0.5` sets the filter camera after Fit; `-e captureFilters true` captures the result in both themes after timing |
| `curves` | Dragging the Curves graph | `-e labels effect-curves-drag` |
| `refine` | Refine's Feather slider, dragged with the stylus | `-e refine grow`, `shrink` or `border`; `-e refineSpan` sets the fraction of the track; `-e refineBar off` hides the bar |
| `crop` | A crop handle over a placed photo | |
| `move` | A placed photo's selected pixels: all of it, the middle half, and the middle half with Leave Copy | |
| `merge`, `dodge_burn`, `frequency_separation` | Merge Visible and Flatten Image, New Dodge & Burn Layer, Frequency Separation; latency is `drained_ns - motion.begin_ns` | Measure latency without `rendererProfile` or memory sampling |
| `pen-response` | Pressure and tilt utility drags and live sensor previews | `-e responseRepeats 3` |

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
`straight`, `zigzag`, `stationary`, `lifts`, `pauses`, `settle`, `visual` or `pinch`); `--trace` and `--profile` add
Perfetto and simpleperf captures.
`python3 tools/performance/android-brush-report.py OUT` summarizes completed
canvas updates per second, the rate the performance targets use for brushes.
[Measuring performance](../performance/measuring.md) has the tier commands and
the rules for the workloads, `pauses`, `settle` and memory runs.

Strokes must present in the display status's `ink_present_mode`:
`SharedDemandRefresh`, or `Fifo` on drivers without shared presentation. Use the
same instrumentation source in comparison APKs, because older runners can
ignore arguments they do not recognize, and give each build and configuration
its own output directory. Comparisons across blend-space defaults must select
the same space with `--blending linear` or `--blending perceptual`.

- **Presets.** The default preset list is the dry brushes. `--presets` accepts
  every built-in preset, including wet, smudge, Liquify, and Clone Stamp,
  Healing Brush and Spot Healing Brush, which read the photo as a reference
  layer.
- **Stats.** The default workspace keeps Stats closed. `--stats` opens Stats and
  enables GPU timing; use `--trace --stats` for GPU phase diagnostics.
- **Paths.** `straight` traverses the horizontal diameter once per contact.
  `zigzag` moves back and forth horizontally at one cycle per second while
  progressing vertically. `--radius-x` and `--radius-y` set the ellipse radii in
  surface pixels.
- **Layers.** `--photo-layers` creates 1–32 photos with translucent duplicates.
  Paint is at the top by default; `--paint-layer-index` moves it to the given
  zero-based position from the top, above the opaque base photo. Compare top,
  middle and lower edits when changing composition reuse.
- **Effects.** `--workload clipped` clips the empty paint layer to the photo;
  `--workload blurred-base --effect-radius 8` also attaches Gaussian Blur (sigma
  in document pixels) to the photo.
  `--live-filter threshold --live-filter-values '{"colors":{"kind":"choice","value":1},"transparency":{"kind":"choice","value":1},"alpha_threshold":{"kind":"number","value":50}}'`
  attaches an effect to the changing paint layer using the typed effect values;
  `--live-filter-disabled` keeps the same graph with the effect hidden for the
  matched baseline.
- **Shader preparation.** `--warmup-idle-ms 0` starts photo setup once the
  selected brush is ready and measures brush preparation without waiting for the
  remaining shader catalogue. `--probe-presets 1,9,11,10,12,41` records
  representative preparation times in `-info.json`; repeat with
  `--warmup-idle-ms 30000` for idle preparation. `--size-idle-ms` waits after the
  selection before changing the size. `--wait-for-warmup` gives a matched, fully
  warm control. Clearing the benchmark app's data does not clear the driver's
  own shader caches.
- **Navigation.** `--navigation-between-strokes` and `--navigation-settle-ms`
  measure the handoff from navigation back to ink.
- **Direct arguments.** Run directly, the instrumentation also accepts
  `-e visualBackground true -e theme light|dark`, which shows Paper, hides the
  photo and cursor, and selects the theme for matching brush captures; keep the
  photo visible for performance comparisons. `-e colorBeforeStrokes true`
  changes the quick paint color before each stroke.
- **Fixtures.** `--photo` also accepts an authored `.capy` fixture under
  `/data/local/tmp`; supply `--canvas-width` and `--canvas-height` to verify its
  canvas. To ink on a crowded vector layer, generate one with
  `cargo run --release -p layer-core --example crowded_vector_fixture -- OUT.capy 4248 2832 10000`
  (add `--paint` for the raster control), push it, and pass it with
  `--canvas-width 4248 --canvas-height 2832`, `--paint-layer-name Ink` and
  `--fixture-layers 4` (5 for the control).
- **Image objects.** `cargo run --locked --release -p layer-color --example object_fixture -- PHOTO.jpg OUTPUT.capy 4 shared`
  creates a photo with four image objects above it; use `unshared` for separate
  decoded sources. Run it with `--workload objects --photo-layers 2 --image-count 4
  --image-sources shared|unshared`; paint index zero is above the object layer and
  index one below it. `objects-effects` attaches Gaussian Blur to the object
  layer. `--mode object-affine` applies committed affine edits on the render
  owner; it measures atomic-edit cost, not a host transform gesture. Object
  runs also write `-cold-setup.json` and `-prime.json`, separate from the warmed
  rate runs.

## Debugging

```bash
mkdir -p artifacts/android
adb -s "$CAPY_ANDROID_SERIAL" logcat -d -v threadtime > artifacts/android/logcat.txt
adb -s "$CAPY_ANDROID_SERIAL" exec-out screencap -p > artifacts/android/screen.png
adb -s "$CAPY_ANDROID_SERIAL" pull /sdcard/Android/data/art.capycanvas.dev/files/validation artifacts/android/
adb -s "$CAPY_ANDROID_SERIAL" shell am start -n art.capycanvas.dev/art.capycanvas.MainActivity
```

Where to look:

- Input: [`CanvasSurfaceView.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasSurfaceView.kt)
  for the canvas, [`WorkspaceInput.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceInput.kt)
  and [`WorkspaceRows.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceRows.kt)
  for chrome drags. Shared drag/drop policy and history live in `crates/layer-ui`.
- Publication: [`CanvasHost.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasHost.kt)
  runs the native session on its `capy-canvas` thread and applies model updates
  into [`ObservedModel`](../../apps/layer-android/app/src/main/java/art/capycanvas/ObservedModel.kt).
  Brush fields and tool-panel fields retain their observed objects, so a size
  change invalidates size readers without rebuilding opacity or group readers.
  Add nested observation only for objects whose consumers read their fields;
  consumers that remember a whole JSON object need its replacement identity.
  Each action publishes before the owner's next task, except moving gestures
  (`dispatch(action, moving = true)`): their moves render every frame and
  publish at most every 33 ms, always after the last move, so publication does
  not starve the canvas. `CanvasHost.actionError` holds the open error dialog's
  text or the refusal being shown; tests check it.
- Presentation: [`native/src/android.rs`](../../apps/layer-android/native/src/android.rs)
  owns the Vulkan surface and its present-mode switches. `Native.displayStatus`
  reports the present mode, frame latency, surface format and color space, and
  submitted and completed frames. A submitted frame is not proof that Android
  displayed it.
- Files: [`Documents.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Documents.kt)
  uses the Storage Access Framework. Providers control their own destination
  behavior, so atomic replacement cannot be assumed for every URI; saves finish
  the private package and checkpoint before opening the destination.
  [`Recovery.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Recovery.kt)
  owns native timing, window leases and provider access; shared Rust validates
  the window manifest, restore tickets and resource stores.
- Workspaces: `crates/layer-workspace` stores them. Android takes the
  workspace liveness lock with Bionic `flock`, because `std::fs::File::try_lock`
  is unsupported on this target.
- Tool bubbles: [`WorkspaceBubbles.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceBubbles.kt)
  retains the shared presentations and animates native layers;
  [`BubbleInput.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/BubbleInput.kt)
  captures contacts at the workspace and calls the stateless Rust hit test.
  Placement frames read only `bubble_motion`; source eligibility, values,
  options, collisions and history remain in Rust. See the
  [host contract](../ui/tool-bubbles.md#host-contract).
- Layers: `Layers.kt` draws the shared clipping rails and FX connections.
  `LayerThumbnails` owns one request queue per canvas host, shared by lists and
  bubble faces, and decodes off the UI thread. Native row hit testing feeds the
  shared `layer_drop` query, whose target drives drop feedback.
- Panel rendering: `panelSurface` in [`PanelShadow.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/PanelShadow.kt).
  Column bridges come from shared Rust: `Native.modelUpdate` publishes receiver
  IDs and the following `Native.panelVectors` call copies their packed commands.
  `PanelVectors.kt` prepares the merged native paths on the canvas owner before
  the matching UI snapshot applies; the input and UI threads never rasterize or
  copy shadow pixels.

If the canvas stops updating while menus still respond, keep the app running.
Record a Perfetto trace with the SurfaceFlinger frame timeline during the
gesture, and compare canvas submissions with the canvas `SurfaceView`'s latches
and the UI layer's. Restarting the app hides the cause. Do not add device-idle
waits or reconfiguration loops before the missing completion or consumption
signal is found.

## Distribution APK verification

The GitHub APK must open without Google Play or Google services. Play App Signing
preserves the signing identity used by existing installations; Automatic
protection adds an installer and licence check that prevents this use.

[`android_apk.py`](../../tools/build/android_apk.py) downloads the
[unprotected standalone APKs](https://developers.google.com/android-publisher/api-ref/rest/v3/generatedapks/list)
provided by Play. When protection is disabled, it can use the universal APK.
It checks the actual manifest and DEX for Play licensing and Google service code,
rejects split APKs, checks the package, version, Android 10 minimum and ARM64
renderer, and verifies the signature before publishing the download. The public
signing certificate in the script matches existing Android installations; an
upload-key signature is not a substitute. A signing-key change needs explicit
upgrade testing before changing this check.

APK verification uses Java 25 and Android Build Tools 37.0.0. Play's APK
Signature Scheme v3.2 signatures use ML-DSA; Java 17 cannot verify them. The
download step selects the runner's Java 25 through `JAVA_HOME_25_X64`. For local
verification, point `JAVA_HOME` at Java 25 and prepend `$JAVA_HOME/bin` to `PATH`.

If Play supplies only unprotected splits, turn off Automatic protection for the
release in Play Console before uploading its bundle. A per-release opt-out does
not disable protection for later releases. The workflow fails rather than
publishing a protected or incomplete APK. See Google's
[unprotected APK instructions](https://support.google.com/googleplay/android-developer/answer/10183279?hl=en).

Run the packaging regression checks without credentials or a device:

```bash
python3 -m unittest discover -s tools/build -p 'test_android_*.py'
```

For local verification, set `CAPY_APK_VERSION` to the APK's version and
`CAPY_APK_PATH` to its path, then run:

```bash
python3 tools/build/android_apk.py verify "$CAPY_APK_VERSION" "$CAPY_APK_PATH"
```

Before release, test the exact download on private Android installs with and
without Google services: first launch offline, painting, import, save/reopen,
export, restart and updating an existing installation without losing drawings.
Google service independence does not
relax the [Android GPU requirements](#prerequisites).
