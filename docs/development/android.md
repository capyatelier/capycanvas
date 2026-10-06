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
Open menus project current shared copy and retain submenu paths by section and
item position in the current shared model. Refreshes resolve those paths again
and truncate removed pages; mutable checkbox commands do not identify navigation
pages. Language publication keeps the popup and its owner; document
replacement retires layer-owned menus. Recreated and resumed owners adopt the
application's latest choice. Size panel units use a full-width row, with the
checkbox below, so translated captions have room at larger system text sizes.
Numeric input starts at the visible edge of its horizontal scroll viewport; check
the glyphs against the visible field bounds as well as the paragraph bounds.
Stateless control helpers use
the owning view's language tag; system fonts retain Android's
native fallback. `locales_config.xml` advertises only shared shipped languages,
and the Rust launch test rejects inventory drift. Text fields retain native
composition ranges; editor key captures yield while preedit or IME key events own
input. InputConnection fixtures check that boundary, independently of checks with
a real Japanese, Chinese or Korean input method.

JNI result handling lives in `native/src/android.rs`: `or_throw` reports
`IllegalStateException` and keeps the return sentinel; `argb_array` packs RGBA
pixels for Kotlin.
Known numeric and color refusals retain their shared typed reason across JNI;
unexpected diagnostics stay literal. Color, profile, proof and export controls
relabel retained scalar copy without revalidating dirty input, reading profile
storage, decoding ICC data or restarting image jobs.

File and conversion jobs use `inspection::on_worker` for named threads with an
8 MiB stack; call it from an IO worker. Task cancellation owns a separate
`CaptureControl` and never borrows a running job.
JNI file, image-import and clipboard tasks use `UiSession::document_request`
to look up the pending request before validating its kind.
Photo profile prompts retain `ImportedDocument` and use its shared `interpret`
method on the file worker before preparing the candidate session.

Artwork files use shared immutable capture and the final package reader/writer.
The render owner supplies captured effect phases before file work starts. Provider
input can be non-seekable; package parsing uses private spooled backing.
`AndroidRasterTest#exactSnapshotsSurviveFilesGpuReplacementAndRecovery` reopens
a saved drawing through a real pipe and compares its pixels and resource records. Saves
finish the private package before opening the provider destination for replacement,
so encoding failure cannot truncate the previous file. Provider publication still
has the provider's durability guarantees. Shared completion checks reject stale
owners and preserve painting performed after capture. Imported native backing
stays alive until renderer admission succeeds; unsupported preparation presents
the original package, while cancellation leaves the current drawing intact.
Unsupported artwork opens
the shared package presentation with an optional preview, Copy Original File and
Export Preview Image, without adopting a partial editable document. Preview
export writes the retained PNG through a private spool to a new provider
destination; source URI, document identity and file identity checks prevent it
from replacing the original package.

Histogram and RGB Waveform are shared dockable panels in the Photo workspace.
Their selectors, status and clipping controls come from the shared views. The
render owner projects changed statistics into normalized paths and a bounded
waveform image; Compose presents those prepared values. Camera-only updates do
not rebuild plots. Hiding the scopes retires their shared query and data.
Levels and Curves embed their adjustment-input statistics and use compact fields
beside the channel selector and calibration actions. White Balance, Auto and
targeted Curves forward shared actions through the existing native contact and
loupe path. Unfinished numeric text keeps its native focus and selection through
statistics updates. Ending an edit retires its late native text callbacks.

Restart restores the selected drawing first, then hydrates the other tabs without
changing the selected canvas. The shared private session format retains tab order,
camera, working state, manual save checkpoint and bounded undo/redo history for
both saved and unsaved drawings. Common restart has no dialog. A drawing restored
after an interrupted exit shows the shared recovered suffix until its next save.
A drawing that fails to restore keeps its copy and offers Restore, Later or
Discard. Discard retires the copies that no open drawing uses, including every
copy of a session whose index cannot be read.

`Recovery.kt` owns native timing, window leases and provider access. Shared Rust
validates the window manifest, restore attempt tickets and incremental resource
stores. Membership is durable before a new drawing's first checkpoint. A two-second
poll coalesces changes and writes only changed session stamps; background and Quit
wait for a durable checkpoint. Polling stops in the background; returning to the
canvas marks the session active again without blocking input. Explicit tab close prepares the native transaction
and retirement intent, removes membership durably, then commits the prepared close.
Resource collection respects restored readers. Save preserves this private history
and updates the manual save checkpoint rather than retiring the session.
Activity teardown completes accepted restores and settles pending edits on the
native owner without a presentation surface before its final checkpoint. Storage
leases and the native editor stay alive until that work completes.
Drawing replacement and tab switching await an accepted checkpoint before changing
membership. Each checkpoint assigns storage owners from its captured tab list.
`AndroidRasterTest#acceptedRecoveryCheckpointDrainsBeforeAdoption` holds native
owner work while checking that an accepted checkpoint drains before replacement.

Persisted provider permissions permit reopening saved destinations after process
restart. Restore observes the saved destination on an I/O worker without opening
a permission dialog. Missing, unreadable or changed originals retain their private
copy and require Save, Discard or Cancel when closing. A shared fingerprint check
compares the destination immediately before replacement; an externally changed
or unreadable destination refuses direct Save.
Save As remains available. Save must durably checkpoint the current drawing
before opening a provider destination for replacement. A failed checkpoint leaves
the original untouched. Provider replacement retains the provider's own durability
guarantees.

## Storage

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
the test ran.
[Testing](testing.md#known-failures-on-main) lists the tests that fail on `main`.

Emulator runs check logic and layout. Physical stylus accuracy, thermal
behavior and high-refresh presentation need a real tablet.

### Where to start

`AndroidSessionRestartTest#processRestartRetainsSavedAndUnsavedHistory` is a
paired process-crash fixture. Run first with `-e restartPhase prepare
-e restartFixture UNIQUE -e theme light|dark`, then with `-e restartPhase verify
-e restartFixture UNIQUE`. Preparation deliberately kills its isolated test
process and reports `Process crashed`; verification must report `OK (1 test)`.
It preserves only that named test directory between invocations and removes it
after verification. It checks saved and dirty tabs, camera, recovered labels,
manual save checkpoints and undo/redo after an actual process death.

- `AndroidHostTest#nativeGradientStopContactsAndCompactControlsRetainDefinition`
  checks native stop contacts, precise positions, interpolation, color and
  history, with archive reopening and Activity recreation.
  `gradientToolPanelAndToolbarUseNativeGeometryContacts` exercises the tool
  panel, toolbar popup and canvas shapes. Run both in narrow and wide layouts;
  each covers light and dark themes.
- `AndroidLanguageTest`: every shipped language through Preferences in both
  themes, retained popup routes, typed numeric refusals, raw color and export
  drafts, profile metadata and prepared comparisons, saved Unicode drawings,
  and stable Activity/host/surface ownership and drawing history.
  InputConnection cases cover deferred composition and selection.
  `#genuineKeyboardCompositionDefersLanguagePublication -e genuineIme true`
  requires an installed keyboard to produce actual preedit before requesting a
  language change in both themes, then commits it through that keyboard. Set
  `genuineImeKeys`, `genuineImePreedit` and `genuineImeCandidate` to the engine's
  key sequence, composing text and visible candidate. For Japanese Gboard
  QWERTY, use `nihonn`, `にほん` and `日本`. Missing preedit fails the prerequisite
  and does not count as IME evidence. Record and restore keyboard languages,
  layout and the selected system subtype after device setup.
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
  persistence. `#toolVariantCornersAndContextMenusShareRememberedChoices` checks
  toolbar and title-bar variant corners, mouse context menus, touch and pen holds,
  full drawer sibling choices, retained openers and workspace switching in both
  themes. `#commandGroupsProjectTheirOwnChoicesIconsAndSelectionScope` checks
  inactive Paint and Sketch group corners, remembered media icons in headers,
  pinned preset leaves, selection family scope and Sketch drawer columns in both
  themes. Native drag capture respects panel stacking: a drawer blocks covered
  resize handles while its own controls remain interactive.
- `AndroidRasterTest`: document, file and GPU lifecycle.
  - `#encloseFillNativeContactsControlsAndHistory` uses Android stylus events
    with Reference source in both themes. It chooses the remembered Enclose
    and Fill subtool in the Lasso fill category and switches the Source control.
    It checks the edge controls, fills
    two enclosed transparent areas, leaves open and partly enclosed areas
    intact, and verifies cancellation and exact undo/redo.
  - `#sharedImageObjectsKeepPosePixelsAndIdentityAcrossFilesAndRecovery` takes
    `-e objectArchive /data/local/tmp/capy-object-ga-functional.capy`, made by
    `object_fixture` from a small photo. Run with `-e theme light` and `dark`.
    It verifies shared paint/object image identity, exact affine undo/redo,
    save/open, private history recovery and pixels after replacing the GPU.
  - `#drawingTabsRestoreMultipleInactiveDrawingsWithoutPrompt` restores order,
    active tab, camera, saved checkpoints and independent undo/redo history.
    `#failedInactiveSessionRetriesWithoutLosingNewDrawing` preserves a failed
    checkpoint while another drawing is edited, then retries and restarts.
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
  - `#attachedFilterPreviewsReferenceAndRecoveryKeepOwnerInput` checks Filters
    drawer previews for a clipped owner and its attached Motion Blur in both
    themes, exact Histogram Reference statistics, hidden-owner animation idle,
    and captured-session undo/redo. It generates private translucent fixtures
    and writes screenshots and `attached-filter-owner-journey.json` to the
    app's external-files directory.
  - `#largePhotoFilterPreviews` and `#largePhotoFilterPreviewLifecycle` opt in
    with `-e filterPhoto true`, `#largePhotoFilterPreviewDrawing` with
    `-e filterDrawing true`. They read the app's private
    `files/filter-memory-test.jpg` and write reports to its external-files
    directory.
- `AndroidHostTest#curvesPagesNativeContactsAndExactCoordinates` covers shared
  Properties pages, native graph contacts, keyboard editing and precise HDR
  fields. `AndroidTextCompositionTest#curveCoordinatesKeepNativeCompositionAndUnchangedPrecision`
  checks composition ownership and unchanged numeric commits through the native
  InputConnection. `AndroidRasterTest#imagePlacementBatchHistoryAndStaleRequests`
  includes Position anchor, pivot, held nudges and Transform Again.
- `AndroidHostTest#pointwiseColorPagesUseNativeControlsAndRetainHiddenValues`
  checks Hue range pages, Colorize, Brightness to Opacity, Threshold and Photo Filter controls and
  slider history in both themes. Run with and without `-e presentationNarrow true`.
  `AndroidRasterTest#pointwiseColorEffectsPersistAllParametersAndOriginalSource`
  covers all Hue parameters, tagged filter colors, source identity and Activity
  recreation across integer and floating document profiles.
  `AndroidRasterTest#illustrationConversionsKeepPaintMaskHistoryAndNativeValues`
  exercises Brightness to Opacity and Threshold with native U16 ProPhoto paint
  in both blending spaces. Stylus contacts cross tile seams and source edges,
  paint separated marks and a mask, and erase with Undo/Redo. It checks partial
  alpha, unchanged filter input and native paint, and exact save/reopen results.

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

- Poll with `CanvasHost.awaitMain`, which checks a condition in `runOnMainSync`
  until a timeout, and `CanvasHost.drain`, which waits for native publication.
  Pass the test's Compose rule when it owns the frame clock. Global idle waits
  can hang while a gesture is held.
- Drawing checks wait for shared canvas/brush readiness and no
  `Native.renderingPending` work, including queued surface presentation.
  `Native.frame` can still request optional shader warmup after the drawing is
  complete. Keep separate completion checks for asynchronous UI operations,
  statistics, readbacks, file workers and recovery.
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

`AndroidViewportBenchmarkTest` accepts `motion=hover` and `cursor=tool` or
`cursor=tool_brush_size` to measure moving tool cursors without depositing ink.
Use the tier photo, release or benchmark build, and three five-second gestures
under the [measurement rules](../performance/measuring.md).

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

- **Workspace motion.** `AndroidWorkspacePerformanceTest` runs with
  `-e workspaceBenchmark true`. `#colorPanelOverlapFrameTiming`,
  `#colorWheelFrameTiming`, `#editColorWheelFrameTiming` (the Edit Color
  dialog's wheel), `#continuousDragFrameTiming` and
  `#continuousResizeFrameTiming` use the real display clock and `FrameMetrics`.
  `-e workspaceTransparency 0`–`3` sets panel transparency (off to high) and
  restores it afterwards. Results appear in logcat under `CapyDragPerf` and
  `CapyResizePerf`.
- **Workspace switcher scrolling.**
  `AndroidWorkspacePerformanceTest#workspaceSwitcherScrollFrameTiming` runs with
  `-e switcherBenchmark true -e photo <readable-tier-photo.jpg>`. It scrolls the
  workspace choices and visibility checklist on the 12 MP photo with an empty
  paint layer, Fit zoom and a long workspace list.
- **Layer reorder.** `AndroidTitleBarTest#layerSwipeFrameTiming` runs with
  `-e layerReorderBenchmark true -e photo <readable-tier-photo.jpg>` and
  `-e width`/`-e height` matching that photo. It moves the native row preview
  with mouse contacts after immediate pickup, with one priming gesture and
  three five-second gestures. The `layer-reorder-<run>.json` external files
  retain moving-window `FrameMetrics` timestamps, camera and layer state.
  Add `-e layerRelationshipBenchmark true` to drag an attached filter through
  owner-chain gaps with clipping connectors and attachment feedback visible.
- **Layer thumbnail selection.** `AndroidTitleBarTest#layerSwipeFrameTiming`
  also accepts `-e layerSelectionBenchmark true`, with the same photo and
  dimensions. It switches between the empty paint layer's content and mask
  every 150 ms, including the resulting tool and canvas action bar changes.
  The theme defaults to dark; `-e layerBenchmarkTheme light` changes it.
  `-e layerSelectionIntervalMs 400` allows each transition to finish before
  the next selection for separate settled-transition diagnostics.
  After one second of priming, three five-second runs record native
  `FrameMetrics` in `layer-selection-<run>.json`. The records include observed
  selection-change timestamps and retain frames within their 200 ms animation
  windows, alongside all received reports, dropped-report counts and named
  timing components. Vsync timestamps measure window-frame cadence, not actual
  screen-presentation times. Check dropped reports before comparing rates and
  exclude gaps between separate animation windows.
- **Grouped tool drawer scrolling.**
  `AndroidWorkspacePerformanceTest#groupedDrawerScrollFrameTiming` runs with
  `-e groupedToolBenchmark true -e photo <readable-tier-photo.jpg>`. It uses the
  default Photo workspace and scrolls the grouped Drawing drawer on the 12 MP
  photo with an empty paint layer and Fit zoom. It does not measure drawer
  opening, sibling switching or tile dragging.
  Both scrolling fixtures use native contacts, one warm-up and three six-second
  gestures per surface. Their `workspace-switcher-benchmark` and
  `grouped-drawer-benchmark` external-file directories contain draw samples where
  the scroll offset changes and raw `FrameMetrics`. The draw timestamps and frame
  vsyncs use Android's monotonic clock; match them to retain moving frames and
  count each vsync once. A 200 ms callback drain retains late frame metrics
  without extending the recorded motion window. Draw callbacks alone do not
  establish presentation performance.
- **Canvas action bar.** `AndroidCanvasBarBenchmarkTest` runs with
  `-e canvasBarBenchmark true`. `-e scenarios ui,paint,photo,composed_transform,scaled,move,selection,menus,canvas_size,refine,crop,merge,dodge_burn,frequency_separation,effects,curves,spatial-effects`,
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
  `-e transformSnapping true` enables snapping for the translation measurement.
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
  `-e imagePlacementCanvasWidth 9504 -e imagePlacementCanvasHeight 6336`
  opens `-e imagePlacementPhotos photo.jpg` from this test app's private files
  directory as the initial document and verifies its extent, then imports another
  occurrence. Each smoke drag selects a live image contact clear of the action
  bar and records its bounds in `validation/pixel-bake/input-probes/`. `-e affineSmokeIdleMs 120000`
  waits after Apply, records memory before and after idle, then verifies a resumed
  drag and Apply. Each stage also records PSS, RSS, system memory and GPU
  allocations in `validation/pixel-bake/maps/*-memory.json`; motion records are
  in `validation/pixel-bake/motion-affine.json`. This measures large accepted
  resources and their retention; renderer submission chunks still bound commands.
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
  `curves` with `-e labels effect-curves-drag` drags the native Curves graph over
  the photo and verifies changing points. `-e translationRepeats 4` retains the
  first contact and three subsequent contacts separately.
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
  node drag. `-e tierPhoto true` uses the reference photo beneath one empty paint
  layer at Fit zoom for its retained Transform journeys. Priming gestures
  validate their geometry, then reset the transform
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
  `contactMs` with `pauseMs` runs repeated short contacts as a checkpoint/history
  diagnostic. Pauses remain in the reported interval; this does not qualify a
  continuous stroke target. `session_samples` records process-wide kernel write
  counters, disk footprint and newest checkpoint age on a separate IO sampler.
  `-e languageSwitches ja,zh-Hans,zh-Hant` requests a language halfway through
  each stroke and records the deferred publication and first resumed completion
  separately from the moving-frame window. Photo runs move the empty paint layer above
  the photo and undo the priming stroke. Set `radiusX`, `radiusY` in surface
  pixels for a contained tier workload. `navigator false` is a separate
  diagnostic; tier measurements retain the default workspace. Pull
  `files/viewport-benchmark/` from the app's external storage and summarize it
  with `python3 tools/performance/android-viewport-report.py DIRECTORY`.
- **Artwork query workers.** `AndroidArtworkQueryBenchmarkTest#exactSamples`
  runs with `-e artworkQueryBenchmark true`; `#artworkStatistics` uses
  `-e artworkStatisticsBenchmark true -e statisticsMode preview|exact|auto`.
  `-e photo /data/local/tmp/FILE.jpg` opens the original photo at its native
  dimensions. The Auto statistics fixture inserts a Levels adjustment and
  queries its shared EffectChannels source; it does not measure UI adoption.
  Results and cancellation latency appear in `files/artwork-query-benchmark/`
  or `files/artwork-statistics-benchmark/`. For navigation interference, use
  `AndroidViewportBenchmarkTest` with `-e openQueryPhoto true`,
  `-e artworkQueries true -e queryCount N`, and `-e statisticsPreview true`,
  `-e statisticsExact true` or `-e levelsStatistics true`. Levels controls also
  use `levelsStatistics true` with `artworkQueries false` to retain the same
  fixture. Query timestamps and owner capture costs are separate from the input
  window; choose enough requests to cover that window and report later drain
  separately. Actual presentation requires the owned SurfaceView's timestamps,
  not renderer submission counts. Use FrameTimeline without graphics tracing
  for rate collection; graphics tracing consumes renderer timing samples.
  `-e clippingPreview true` enables both shared clipping flags without query
  workers, records the applied state and photo frame, and checks a known
  clipped solid-color frame after motion. Use `-e repeats 0` for that
  presentation check alone; it does not provide motion qualification.
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
`Capy GPU native capture ns` measures GPU validation, native encoding and
canonical promotion after paint and before prediction. It excludes CPU
preparation and backing-worker readback. A scope that cannot fit the current
command encoder or obtain a timer slot records its renderer frame ID in
`Capy GPU native capture omitted`; treat those frames as missing phase data.
`--presets` accepts every built-in preset, including wet, smudge, Liquify, and
Clone Stamp, Healing Brush and Spot Healing Brush, which read the photo as a
reference layer.
`--workload clipped` clips the empty paint layer to the photo.
`--live-filter threshold --live-filter-values '{"colors":{"kind":"choice","value":1},"transparency":{"kind":"choice","value":1},"alpha_threshold":{"kind":"number","value":50}}'`
attaches the selected effect to the changing paint layer. The values object uses
the existing typed effect values. `--live-filter-disabled` retains the same
graph with that effect hidden for the matched baseline. Setup checks the owner,
attachment, enabled state and requested values before motion. These options
require the ordinary workload and a brush gesture. Completion observations cover
the submitted canvas work, including synchronous filter evaluation; separately
attribute derived revisions before using them for asynchronous filters.
The report samples latest-input age every millisecond during active contact and
records input-to-GPU-completion latency. These are completion proxies, not scanout
measurements. Use at least 20 separate contacts for response and settling
percentiles, for example `--duration 1000 --repeats 20`; report these separately
from sustained five-second strokes.
`--workload blurred-base --effect-radius 8` also attaches Gaussian Blur to the
photo, then paints its clipped layer. The radius argument is the Gaussian sigma
in document pixels; sigma 8 has 24 px sampling support along each axis in the
two separable blur passes. These fixtures require one photo and the paint layer
at index zero. Their reports retain the paint, base and effect handles, actual
sigma and initial layer state; setup validation checks row order, attachment,
selected paint and parameters before collecting motion. Ordinary photo-plus-paint
remains the default. The opaque tier photo measures ownership and cache cost;
shared GPU fixtures cover expanded fractional alpha.
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
The Python runner exposes these as `--navigation-between-strokes` and
`--navigation-settle-ms`. `--photo` also accepts an authored `.capy` fixture under
`/data/local/tmp`; supply `--canvas-width` and `--canvas-height` to verify its
active canvas before collecting input. Its Photo row and paint-layer setup must
match the requested workload.
`cargo run --locked --release -p layer-color --example object_fixture -- PHOTO.jpg
OUTPUT.capy 4 shared` creates a photo with four image objects above it. Use
`unshared` for separate decoded sources. Run it with `--workload objects
--photo-layers 2 --image-count 4 --image-sources shared|unshared`; paint index zero
is above the object layer and index one is below it. `objects-effects` attaches
Gaussian Blur to the object layer. `--mode object-affine` applies committed
shared affine edits on the render owner and records call times separately from
pen input. The completion observer records the document revision actually
composed for each object frame. The report counts each new pose once, only when
its GPU completion falls inside the motion interval, and records edit-to-GPU
completion separately from JNI adoption. These observations do not establish
scanout or physical input-to-present latency. This measures atomic-edit cost;
it does not qualify a host transform
gesture or physical input latency. The moving-layer boundary uses the renderer's
ordinary interactive sampling and resumes exact refinement on release. Memory
diagnostics retain process PSS alongside GPU allocation and system memory fields.
Object runs also retain `-cold-setup.json` and `-prime.json`: package adoption,
pending composition at the first stroke, and the priming motion before exact
settling. These startup diagnostics remain separate from the warmed rate runs.
`-e colorBeforeStrokes true` changes the quick paint color before each stroke
and checks that the prepared brush remains ready.
[Measuring performance](../performance/measuring.md) has the tier rules.

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
- Layers: `Layers.kt` draws shared clipping rails and FX connections over measured
  content thumbnails, using the relationship palette role for clipping and native
  text ink for the canonical FX glyph. Connector geometry cancels the row face
  translation while drop hit testing keeps the moved thumbnail bounds. Saved
  Selection uses the ordinary button helper with a 16 dp icon in a 30 dp slot.
  Group badges, contextual attachment, inherited visibility and optional
  right-swipe actions come from shared state.
  Native row and thumbnail hit testing supplies the contact surface to the
  `layer_drop` query; its normalized target and position drive feedback before
  the shared action commits a drop. `AndroidTitleBarTest#layerRelationships`
  covers the focused light/dark journey. Add `-e layerRelationshipBenchmark true`
  to `#layerSwipeFrameTiming` to measure moving relationship indicators over the
  24 MP photo.
- Panel rendering: `panelSurface` in [`PanelShadow.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/PanelShadow.kt);
  `AndroidPanelShadowTest` compares panels with and without shadows.

If the canvas stops updating while menus still respond, keep the app running.
Record a Perfetto trace with the SurfaceFlinger frame timeline during the
gesture, and compare canvas submissions with the canvas `SurfaceView`'s latches
and the UI layer's. Restarting the app hides the cause. Do not add device-idle
waits or reconfiguration loops before the missing completion or consumption
signal is found.
