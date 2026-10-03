# Testing

[Developer guide](README.md)

Different checks answer different questions. Shared tests exercise document and
editor rules without a window. Host tests exercise native controls and input.
GPU tests check pixels and rendering cost. Device tests establish whether real
pen and display behaviour matches the intended interaction. None substitutes for
another, and there is no CI: run the checks for every area your change touches.

## Checks by change type

Run the rows that match your change, then walk the affected user journeys on each
affected host. Scope Cargo commands with `-p`; `cargo test --workspace` pulls in
GPU and platform crates that a given machine may not be able to run.

| Change | Checks |
| --- | --- |
| Shared Rust (`layer-core`, `-engine`, `-ui`, `-workspace`, `-color`, `-render`) | `cargo test --locked -p <crate>` for each changed crate, then `cargo check --locked -p layer-linux --tests` and `cargo check --locked -p layer-web --target wasm32-unknown-unknown` for the consumers. |
| Renderer, shaders or runtime filters | `cargo test --locked -p layer-render-wgpu <filter>` and `-p layer-host` on a hardware GPU. If shader cache-key inputs changed, `python3 tools/build/test_shader_generation.py`. Frame-path changes need [measurements](#performance). |
| Pen prediction or stroke placement | `cargo test --locked -p layer-engine --features prediction-bench`, then `cargo build --locked -p layer-engine --release --features prediction-bench --examples` and `python3 tools/prediction/replay-bank.py --output artifacts/strokes/bank --check`. |
| GTK | `cargo test --locked -p layer-linux` for the model tests, then each affected journey on the private display: `bash tools/performance/workspace-motion.sh gtk --native-test=<name>`, adding `--tablet` for pen journeys ([Linux](linux.md#tests)). |
| Web | `bash apps/layer-web/build.sh`, the pure tests `node --test apps/layer-web/{run,package,frame,pointer,workspace-client,canvas-bar,notice,zoom-readout,export-controls,size-dialog,text-input,localization}.test.mjs`, then the affected journeys with `bash tools/performance/workspace-motion.sh web --<journey>` (headed, hardware WebGPU) or `node apps/layer-web/test.mjs --headless --<journey>` against `run.sh`. Packaging changes: `node apps/layer-web/package.mjs && node apps/layer-web/test.mjs --package` ([Web](web.md#tests)). |
| Android | Without a device: `./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug -PcapyAbi=arm64-v8a` in `apps/layer-android`. On a reserved tablet, through `tools/devices/devices.py run`: install both APKs with `adb install -r`, then `am instrument -w -e class art.capycanvas.<Class>[#method] $CAPY_APPLICATION_ID.test/androidx.test.runner.AndroidJUnitRunner`. Read `OK` or `FAILURES!!!`, not the exit status ([Android](android.md#device-tests)). |
| Apple | Anywhere: `cargo test --locked -p layer-apple --lib -- --test-threads=1` and `python3 apps/layer-apple/tests/test_icon_assets.py`. On a Mac: the Swift fixtures, the command audit, and the XCTest journeys in the [Apple guide](apple.md#tests); journeys that render the canvas need a physical iPad. |
| Windows | From any OS: `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib` and `cargo clippy --locked -p layer-windows --all-targets -- -D warnings`. On Windows: `apps/layer-windows/scripts/test-without-gpu.ps1`, then the `exercise-*.ps1` fixtures for the changed area; from Linux, `tools/windows-vm/windows-vm.py check` and `fixtures <name>`. Device-removal, HDR and performance checks need a hardware GPU ([Windows](windows.md)). |
| Dependencies or `Cargo.lock` | `cargo deny --locked check licenses sources` ([publication](publication.md)). |
| Scripts under `tools/` | `python3 -m unittest discover -s tools/<dir> -p 'test_*.py'`, or `node --test` on the script's `.test.mjs`. |
| Docs only | Check every link, path, command and test name you wrote or whose target you changed. |

## Shared Rust

Use `session::test_support` for `layer-ui`'s Recorder fixtures, action dispatch,
pointer records, selections and document-space pen input. Platform changes belong
in that test fixture module; production sessions keep the host's platform.

Photo writer tests use `photo::test_support::assert_provider_failure` for row
cancellation. Keep each codec's admission, publication and pixel assertions.

Workspace tests replay the SQLite/browser preference fixture through IndexedDB.
The manager fixture's `expire_lease` changes both the cached claim and SQLite row.
Host test helpers keep serialization flags, update readers and GPU frame timing
in the callers' control.
Figure and gradient tests share `abandon_layer_drag` while retaining their own
frame timestamps and commit assertions. Collapsed resize fixtures retain the
nested and outside handle coordinates, width checks and cancellation points.
Toolbar fixtures allocate tiles through `insert_tools`. Modifier fixtures compare
their rows with the shortcut page's tool contexts.
Cursor tests observe a frame before asserting that no ink was deposited. Stroke
selection tests check nonempty live batches; undo/redo compares raster identities.
Presentation tests use the renderer's `create_target` for their surfaces; preserve
each test's format and extent, and validate its usage flags on the test GPU.
Renderer source fixtures use `test_support::depth_source` with their original
budget and coordinate mapping. Pen fixtures override the shared event's timestamp
and pressure where their cadence differs.
Integration pen fixtures share sample construction and input draining; callers
retain pressure axes, sequence numbers, cadence and GPU-wait policy.
Retouch replay fixtures keep each brush's path and pen-up position. Check source
readiness and healing counts after replay settles, before collecting result pages.
Mesh fixtures pass a previously constructed projective map to `MeshMap::fit`.
Raster corruption fixtures retain their exact pixel descriptor.
Renderer float fixtures share readback and presenter setup. Keep pooled and
unpooled memory accounting separate, with each test's pixel and memory limits.
Partial-region fixtures keep their damage bounds; transform oracles keep source
and selection flags and tolerances. Startup fixtures retain readiness gates and
deadlines across compilation phases.
Apple stateless ABI fixtures share string ownership while retaining each entry
point's errors and independent color precision checks.

Renderer tests create and destroy their own GPU device, so
[`.cargo/config.toml`](../../.cargo/config.toml) runs four test threads unless
`RUST_TEST_THREADS` or `--test-threads` says otherwise. Some Linux drivers limit
live Vulkan devices per process or deadlock while many threads create and destroy
devices; use `--test-threads=1` for device-heavy filters. Headless renderers keep
one wgpu instance for the process lifetime, because reloading the driver
eventually exhausts static TLS.

`layer-render-wgpu` unit tests and `software-adapter-tests` builds accept a CPU
adapter when `LAYER_TEST_SOFTWARE_GPU=1` is set. Those runs check numbers only;
they are never performance evidence, and production hosts always require a
hardware GPU.

## Localization

Run `cargo test --locked -p layer-ui localization_catalog_tests` for catalog
changes, then the shared Rust and consumer checks above when Rust changes.
These window-free tests reject malformed Fluent, duplicate identities across
files, missing English identities in shipped languages, undeclared translated
variables, missing
message/term/attribute references and formatting errors. They exercise complete
dynamic messages with zero, one and many counts and literal CJK, emoji, quotes,
braces and long filenames. Direct Fluent formatting checks expose errors before
runtime English fallback can hide them.

Live language changes use `cargo test --locked -p layer-ui language_`, followed
by the full changed-crate suites. The host journeys are
`workspace-motion.sh gtk --native-test=native_live_language_switching`,
`workspace-motion.sh web --language-switching`, Android `AndroidLanguageTest`,
and Windows `scripts/exercise-localization.ps1` in both themes. These check
preference selection, retained editor identity and drafts, generated captions,
and continued drawing ownership. Run the affected dialog and composition suites
too. Synthetic composition and InputConnection coverage do not establish genuine
CJK IME behavior.

Catalog tests do not prove host coverage or linguistic quality. For an advertised
language, walk every affected host in both themes, including search, rename with
IME, Unicode save/reopen, export, unsaved close, validation errors, workspace
history and accessibility. Check narrow windows and increased system text size.
Preserve user names and document contents across language changes. Follow the
[localization guide](../ui/localization.md) for catalog and release rules.

## UI and input

- **Both themes.** Check changed UI in light and dark themes, and brush changes in
  both live drawing and replay.
- **Drags.** Reorder gestures follow the [drag convention](../ui/drag-and-reorder.md)
  and its [validation matrix](../ui/drag-and-reorder.md#required-validation-when-implementing).
  Test mouse, touch and pen separately, and assert that movement before a hold
  does not reorder where a hold is required. The matrices run with
  `workspace-motion.sh gtk --drag-pickup` and `--column-drops`, `test.mjs
  --drag-pickup`, `--layer-hold`, `--long-press-drag` and `--drag-cursors` on
  Web, `AndroidInteractionTest`, and `exercise-layer-pickup.ps1` and
  `exercise-workspace-pickup.ps1` on Windows.
- **Parity.** Web is compared with GTK, and each native port with Web, at the
  same document, workspace, theme, viewport and scale. For GTK and Web, run
  `cargo test --locked --release -p layer-linux native_web_parity_reference -- --ignored --test-threads=1`,
  then `node apps/layer-web/test.mjs --parity` in a headed browser. The
  [visual tools](../../tools/visual/README.md) capture the other hosts; never
  rescale or crop a capture to hide a difference.
- **Icons.** `node apps/layer-web/test.mjs --icons`, `workspace-motion.sh gtk
  --icons` at scale 1 and with `LAYER_MOTION_SCALE=2`, Android `AndroidIconTest`,
  and `python3 apps/layer-apple/tests/test_icon_assets.py`.
- **Popups on GTK.** Run popup and menu checks without `--tablet`: the tablet
  proxy's synthetic serials cannot take compositor popup grabs.
- **Real pens.** Injected input checks logic, not the OS driver. Input changes
  need a real stylus on the affected host: pressure, tilt, hover, cancellation
  and fast curves, plus touch navigation and resuming after the app was in the
  background.

## Performance

[Performance targets](../PERFORMANCE_TARGETS.md) defines the tiers and targets,
and [measuring](../performance/measuring.md) the rules and commands. Keep CPU
submission, GPU completion and presentation latency separate: a short submission
time does not establish a frame rate. Offscreen GPU numbers from
`cargo run --locked --release -p layer-bench -- --scenario <name>`
([benchmark workloads](gpu-raster-benchmarks.md)) help compare revisions on one
machine but never replace a measurement on the tier's device. Regenerate bundled
brush previews with `cargo run --locked --release -p layer-bench -- --brush-previews`.

## Known failures on main

These fail on `main` independently of your change. Don't chase them unless they
are your task, and remove an entry when you fix it.

- GTK `native_selection_pen_input`, `native_toolbar_components_narrow_input`,
  `native_workspace_motion_input`, and `native_workspace_switcher_input`
  (intermittent). `native_workspace_resize_input` presents below its rate
  threshold under the runner's `color-mgmt`.
- GTK `native_toolbar_manager` and `native_panel_customization` expect the older
  customization dialogs where managed desktop menus now open the workspace manager.
- Android: 7 of 15 `AndroidTitleBarTest` cases;
  `detachedPanelsKeepBodiesAndWiderResizeTargets`;
  `AndroidInteractionTest#cachedPanelsMatchDirectDrawing` (light docked panels);
  the last assertion of `AndroidHostTest#cameraNavigationPublishesOnlyReadoutUpdates`;
  `AndroidInteractionTest#menuBodyAndExtendedTabDropsAcrossDevices` and
  `#drawerTabsKeepActiveColorsAndPadding`; `AndroidHostTest` cases
  `shortcutPageRecordsMultipleBindingsAndPersists`,
  `shortcutSearchFindsModifiedKeysAndMarksChangedBindings`,
  `settingsPanesShareTopEdgeAndUseAppScale`,
  `workspaceUsesNativeMouseAndPenCursors`,
  `workspaceMenusManageVisibilityNamesAndHistory`,
  `toolbarManagerSelectsConfirmsDeletesAndRestores`,
  `filterLayerIconsUsePackagedNames` and
  `toolbarConfigurationAndGroupCollapseUseTheSharedDefault`.
- Android `AndroidHostTest#touchNavigationHistoryCancellationAndSurfaceRecovery`
  passes recovery and continued drawing, then cannot find the portrait Settings
  label `Pen & Input` on TCL TAB 11 Gen 2.
- Android `AndroidRasterTest#hdrBlackIntensityMarkerVisible` intermittently
  keeps the last EV value when the test cancels its drag.
- Android `AndroidWorkspaceManagerTest#restoreStartingLayoutPlacesPalettesAfterColorAndProofAfterNavigator`
  cannot find its Window menu on MovinkPad 11.
- Android `AndroidRasterTest#profileLibraryKeepsExactCopiesAndPresetOwnership`
  intermittently times out waiting for profile-library buttons.
- Android `AndroidIconTest#allIconsRenderAtToolbarSizesWithThemeAndFixedPaints`
  reads below its clipped grid screenshot on MovinkPad Pro 14.
  `AndroidIconEditorTest#allToolCategoriesModesFiltersAndToolbarIconsRender`
  expects 13 brush categories where the current catalog supplies 16.
- iPad XCTest `testCompactMenuShortcutAcrossPages` and
  `testSettingsTextSelectionShortcut`: XCTest keys don't reach UIKit key commands.
- Apple Rust `apple_photo_corrections_masks_and_original_samples_remain_revisable_after_worker_reopen`
  and Windows Rust `property_wire_keeps_section_identity_and_choice_indices_with_equal_labels`
  expect controls outside the selected Properties page. Apple Rust
  `apple_current_main_drawers_paper_and_zen_use_shared_actions` compares the
  transient Properties epoch across undo/redo. All three also fail at `b886ccf6b`.
- Headless Web `--toolbar-components`, `--tonal-selection`, `--editor`, `--hdr`,
  `--proof`, `--raster`, `--selection-tools` and `--shared-workflows`, and 8 pen
  side-button cases in `pointer.test.mjs`.
- Tablet Chrome `--workspace-manager` cannot find its new-workspace name input
  on MovinkPad 11.
- Headless Web `--contact-brushes` with `LAYER_BRUSH_PRESETS=20,21` differs from
  the committed watercolor pixels after Redo.
- Headless Web `--zoom-readout` reaches export with an empty stroke image
  and fails its ink assertion.
- `cargo clippy -- -D warnings` stops in `layer-core` on lints new in Clippy 1.96.
  New code adds no warnings.
