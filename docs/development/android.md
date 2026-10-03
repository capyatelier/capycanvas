# Android development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Android client uses Kotlin and Jetpack Compose for the editor UI. A Rust JNI
bridge connects it to `NativeHost`, and the shared Vulkan renderer presents into
a `SurfaceView`. The canvas needs Vulkan shared-demand presentation with
swapchain-maintenance present fences; a driver without them shows a canvas
initialization error instead of a canvas.

The native Gradle build tracks shared Rust, filter assets and Fluent catalogs as
inputs. Catalog-only changes rebuild the embedded native UI text.

The render worker supplies saved settings and the ordered application resource
locales before shared launch creates the first localization context or GPU.
Language changes prepare the shared context on a separate worker, then publish
bootstrap, catalog and editor views together on the render owner. Publication
waits for active text composition and canvas or workspace contact. The Activity,
Compose context, native text fields and canvas surface retain their identities.
Recreated and resumed owners adopt the application's latest choice. Stateless
control helpers use the owning view's language tag; system fonts retain Android's
native fallback. `locales_config.xml` advertises only shared shipped languages,
and the Rust launch test rejects inventory drift. Text fields retain native
composition ranges; editor key captures yield while preedit or IME key events own
input. InputConnection fixtures check that boundary, independently of checks with
a real Japanese, Chinese or Korean input method.

JNI result handling lives in `native/src/android.rs`: `or_throw` reports
`IllegalStateException` and keeps the return sentinel; `argb_array` packs RGBA
pixels for Kotlin.
File and conversion jobs use `inspection::on_worker` for named threads with an
8 MiB stack; call it from an IO worker. Task cancellation owns a separate
`CaptureControl` and never borrows a running job.
JNI file, image-import and clipboard tasks use `UiSession::document_request`
to look up the pending request before validating its kind.
Photo profile prompts retain `ImportedDocument` and use its shared `interpret`
method on the file worker before preparing the candidate session.

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

- `AndroidLanguageTest`: all six language choices through Preferences in both
  themes, stable Activity/host/surface ownership, drawing history, and retained
  numeric text/selection across a deferred InputConnection composition.
- [`AndroidInteractionTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidInteractionTest.kt):
  drawers, drag geometry, panels, the canvas action bar, notices, the zoom
  readout, effects and retouching, each with mouse, finger and stylus. It
  dispatches typed `MotionEvent`s through the native views;
  `-e systemInput true` uses OS injection where the device allows it.
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
    and front-buffer ink, rotation, surface recreation and GPU recovery,
    including capture, detach and device loss with an unpresented FIFO image.
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
  `-e canvasBarBenchmark true`. `-e scenarios ui,paint,photo,composed_transform,scaled,move,selection,menus,canvas_size,refine,crop,merge,dodge_burn,frequency_separation,effects,spatial-effects`,
  `durationMs`, `width`, `height`, `blending` (`perceptual` or `linear`) and
  `transparency` (`off` to `high`) narrow or
  resize the run. `-e photo <readable-file>` uses a JPEG matching `width` and
  `height`; otherwise the fixture generates an image. `-e zoomOut false` keeps
  Fit zoom. `-e project <readable-current-format.capy>` opens a prepared native
  project through the ordinary loader instead of creating a canvas. For paired
  retained-photo translation, use `-e acceptedPhoto true`,
  `-e labels photo-translate-drag`, and add `-e materialWatercolor true` to paint
  a sparse wet-watercolor stroke before Transform. That run verifies and saves
  the retained source and native watercolor planes alongside its motion report.
  `-e translationRepeats 4` saves the first translation and three more gestures
  in the same session, keeping cold-start and warm results separate.
  `-e labels photo-retained-distort-drag` or
  `-e labels photo-retained-warp-drag` measures retained corner or mesh-node
  motion on the same input and repeats it in the same session.
  `-e saveRetainedDiagnostic true` uses a one-way retained drag, accepts its
  geometry, and saves `retained-diagnostic.capy` and its manifest for reproduction.
  This changes the motion path; keep it separate from qualification runs.
  `-e finalBake true` accepts a translated watercolor photo, applies Transform
  to Pixels, samples job memory, and verifies native backing after save/reopen.
  For a smaller command-memory diagnostic, `AndroidRasterTest`'s
  `imagePlacementBatchHistoryAndStaleRequests` accepts
  `-e imagePlacementAffineSmoke true -e affineSmokeDrags 4` and
  `-e affineSmokeWatercolor true`; it records process maps after each drag,
  idle, and Apply without reopening between gestures.
  `-e memory true` records tracked renderer allocation; `-e rendererProfile true`
  records CPU submission and GPU observations. These rates exclude presented
  input latency. Each scenario records the initial camera and its input window in
  monotonic and boot clocks, separately from terminal completion polling,
  logs a `CapyBarPerf` line and writes
  `canvas-bar-benchmark/<label>.json` in external files; `ui_hz` and
  `ui_interval_ms` count the distinct vsyncs the UI drew until the gestures end.
  `effects` scrubs Exposure alone and after Levels/Vibrance; `spatial-effects`
  scrubs Gaussian Blur at small and large radii. Both prime the actual slider,
  wait for shader readiness and verify changing parameter values during motion.
  `-e effectZoom 0.5` sets the filter camera after Fit; omit it for the Fit control.
  `-e captureFilters true` captures the resulting filter canvas in both themes
  after timing finishes.
  `AndroidRasterTest#spatialFilterWindowsKeepPaintAndHistory` with
  `-e spatialPhoto /data/local/tmp/capy-tier-24mp.jpg` exercises Gaussian Blur
  at 50% zoom, touch navigation, stylus painting and exact undo/redo in both themes.
  `refine` drags the Refine panel's Feather slider with the stylus on a
  2048 × 1536 document and at `width` × `height`, and logs the values sent and
  previews drawn. `-e refine grow` (or `shrink`, `border`) picks another
  operation, `-e refineSpan` the fraction of the track, and `-e refineBar off`
  hides the canvas action bar. `crop` drags a crop handle over a placed photo.
  `move` drags the selected pixels of a placed `width` × `height` photo with
  Move: all of it, the middle half, and the middle half with Leave Copy.
  `merge` paints eight layers over a placed photo and times Merge Visible and
  Flatten Image; its `after_mark` has the dispatch time and the GPU completions
  that follow. `dodge_burn` and `frequency_separation` time New Dodge & Burn
  Layer and Frequency Separation (radius 8) on a placed photo the same way.
  Their completion latency is `drained_ns - motion.begin_ns`: a new layer can
  appear before its pixels finish. These scenarios add no fixed delay after
  the command. Measure latency without `rendererProfile` or PSS sampling;
  collect memory separately. Use `-e photo` for the reference photo and
  `-e zoomOut false` to retain Fit zoom.
  `photo` separates body translation, corner resizing, distortion and a warp
  node drag. Priming gestures validate their geometry, then reset the transform
  and restore the intended mode before measurement. Commands query the owner
  for the current shared `command_reason` after injected gestures. Published
  command state stays stable during contact; Android input delivery can finish before
  the renderer owner consumes the terminal sample. These waits occur outside the
  motion measurement window. The output directory is
  cleared at the beginning of each invocation, so omitted scenarios cannot
  contribute results from an earlier run.
  `-e labels <comma-separated labels>` limits the measured operations while
  preserving fixture setup and priming. `composed_transform` resizes and distorts
  a translucent photo over another photo, retaining a composed root.
  `gpu_completed_hz` counts completions inside the gesture window, including
  thumbnail-only refreshes. Raw `measurements` and `completions` permit matching
  submissions to the latest completed host input call, excluding updates with no
  newly consumed input. These counts do not establish distinct transform poses
  or display cadence; use a SurfaceFlinger trace for the latter.
  Photo drags are marked by `capy-drag` trace sections; `capy.publish.native` and `capy.publish.parse`
  time each model publication; `-e composeTrace true` adds a section per
  composable, which slows the frames it attributes.
  `-e rendererProfile true` opens the Stats panel to collect renderer GPU phase
  timestamps. Use these runs for attribution; the panel changes the workload,
  so compare motion rates with the ordinary runs separately.
  `-e panel navigator` opens the Navigator for a separate drag qualification;
  record this workload separately from runs with the panel closed. The harness
  sets Stats/Navigator visibility explicitly and asserts the overview count.
  Combining it with `rendererProfile` keeps both panels open.
- **Canvas navigation and drawing.** `AndroidViewportBenchmarkTest` runs with
  `-e viewportBenchmark true`. `-e width 4248 -e height 2832` selects the low-tier
  canvas; `-e canvasSize` supplies both dimensions when they are omitted.
  `-e photo /data/local/tmp/FILE.jpg` places that photo before drawing and navigation.
  `-e passThrough true` adds a Pass Through group with a Black & White adjustment
  above a Solid Color fill. Warmup waits for shader readiness and the timed interval ends
  with the gesture, before draining frames. Navigation warms a matching gesture
  and restores the camera before measurement. `-e motion pan|pinch` measures navigation;
  the default `stroke` draws, with `osInput`, `canvasSize`, `brushSize`,
  `intervalMs`, `durationMs`, `repeats`, `blending` and `label`.
  `-e languageSwitches ja,zh-Hans,zh-Hant` requests a language halfway through
  each stroke and records the deferred publication and first resumed completion
  separately from the moving-frame window. Photo runs move the empty paint layer above
  the photo and undo the priming stroke. Set `radiusX`, `radiusY` in surface
  pixels for a contained tier workload. `navigator false` is a separate
  diagnostic; tier measurements retain the default workspace. Pull
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
`stationary`, `lifts`, `pauses`, `settle`, `visual` or `pinch`); `--trace` and `--profile` add
Perfetto and simpleperf captures. The default preset list is the dry brushes;
the default workspace keeps Stats closed. `--stats` opens Stats and enables GPU
timing; use `--trace --stats` for GPU phase diagnostics. Report these runs
separately because Stats changes the workspace and adds measurement work.
`--presets` accepts every built-in preset, including wet, smudge, Liquify, and
Clone Stamp, Healing Brush and Spot Healing Brush, which read the photo as a
reference layer.
`--mode pauses --contact-ms 100 --pause-ms 100` resumes during refinement.
Increase only the gap to measure the same contacts after settling, or increase
contact duration to test finalization after a broad stroke. The runner checks
both durations in the observed setup; older APKs that ignore them are rejected.
`--mode settle` injects a one-second pinch immediately after a broad stroke,
then changes tools and queues a short paint contact. The first 100 ms of input
has no synchronous state query; the probe observes pending work at its first
camera sample. Use `--settle-delay-ms` for a matched control after settling.
The raw `settle_probe` records action times and camera revisions. Use
`--presentation-trace` for screen cadence; completed artwork updates do not
count camera-only frames. Brush preview PNGs load off the UI thread and share a
bounded cache across panel lifetimes.
`--radius-x` and `--radius-y` set the ellipse radii in surface pixels;
`--photo-layers` creates 1–32 photos with translucent duplicates. The runner
places paint at the top by default. `--paint-layer-index` moves it to the given
zero-based position from the top, above the opaque base photo so strokes remain
visible. Compare top, middle and lower edits when changing composition reuse.
The runner checks the observed trajectory, layer count, selected layer position, brush, prediction and requested
zoom before starting the timed window, including when reusing completed output.
`--blending linear` or `--blending perceptual` selects the document's blend space
and verifies the selected shared command; omitting it keeps the imported default.
Comparisons across blend-space defaults must explicitly select the same space.
Use the same instrumentation source in comparison APKs: older runners can ignore
arguments they do not recognize. Each build and configuration needs its own
output directory. The private benchmark restores its prediction settings
atomically, independently of which preference controls the device enables.
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
  Brush fields and tool-panel fields retain their observed objects, so a size
  change invalidates size readers without rebuilding opacity or group readers.
  Add nested observation only for objects whose consumers read their fields;
  consumers that remember a whole JSON object need its replacement identity.
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
