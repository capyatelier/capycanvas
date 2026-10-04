# Testing

[Developer guide](README.md)

Different checks answer different questions. Shared tests exercise document and
editor rules without a window. Host tests exercise native controls and input.
GPU tests check pixels and rendering cost. Device tests establish whether real
pen and display behaviour matches the intended interaction. None substitutes for
another. CI runs only the window-free shared suites, the Wasm check, the Apple
project check and the license check ([releasing](releasing.md#continuous-integration)),
so run the checks for every area your change touches.

## Checks by change type

Run the rows that match your change, then walk the affected user journeys on each
affected host. Scope Cargo commands with `-p`; `cargo test --workspace` pulls in
GPU and platform crates that a given machine may not be able to run.

| Change | Checks |
| --- | --- |
| Shared Rust (`layer-core`, `-engine`, `-ui`, `-workspace`, `-color`, `-render`) | `cargo test --locked -p <crate>` for each changed crate, then `cargo check --locked -p layer-linux --tests` and `cargo check --locked -p layer-web --target wasm32-unknown-unknown` for the consumers. |
| Renderer, shaders or runtime filters | `cargo test --locked -p layer-render-wgpu <filter>` and `-p layer-host` on a hardware GPU. If shader cache-key inputs changed, `python3 tools/build/test_shader_generation.py`. Frame-path changes need [measurements](#performance). |
| Pen prediction or stroke placement | `cargo test --locked -p layer-engine --features prediction-bench`, then `cargo build --locked -p layer-engine --release --features prediction-bench --examples` and `python3 tools/prediction/replay-bank.py --output artifacts/strokes/bank --analyze --check`. |
| GTK | `cargo test --locked -p layer-linux` for the model tests, then each affected journey on the private display: `bash tools/performance/workspace-motion.sh gtk --native-test=<name>`, adding `--tablet` for pen journeys ([Linux](linux.md#tests)). |
| Web | `bash apps/layer-web/build.sh`, the pure tests `node --test apps/layer-web/{run,package,frame,pointer,workspace-client,canvas-bar,notice,zoom-readout,export-controls,size-dialog,text-input,localization,numeric,histogram,workspace-manager-copy,toolbar-components-copy,color-controls-copy,document-color-copy,color-button-lifecycle,raster-worker-client}.test.mjs`, then the affected journeys with `bash tools/performance/workspace-motion.sh web --<journey>` (headed, hardware WebGPU) or `node apps/layer-web/test.mjs --headless --<journey>` against `run.sh`. Packaging changes: `node apps/layer-web/package.mjs && node apps/layer-web/test.mjs --package` ([Web](web.md#tests)). |
| Android | Without a device: `./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug -PcapyAbi=arm64-v8a` in `apps/layer-android`. On a reserved tablet, through `tools/devices/devices.py run`: install both APKs with `adb install -r`, then `am instrument -w -e class art.capycanvas.<Class>[#method] $CAPY_APPLICATION_ID.test/androidx.test.runner.AndroidJUnitRunner`. Read `OK` or `FAILURES!!!`, not the exit status ([Android](android.md#device-tests)). |
| Apple | Anywhere: `cargo test --locked -p layer-apple --lib -- --test-threads=1` and `python3 apps/layer-apple/tests/test_icon_assets.py`. On a Mac: the Swift fixtures, the command audit, and the XCTest journeys in the [Apple guide](apple.md#tests); journeys that render the canvas need a physical iPad. |
| Windows | From any OS: `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib` and `cargo clippy --locked -p layer-windows --all-targets -- -D warnings`. On Windows: `apps/layer-windows/scripts/test-without-gpu.ps1`, then the `exercise-*.ps1` fixtures for the changed area; from Linux, `tools/windows-vm/windows-vm.py check` and `fixtures <name>`. Device-removal, HDR and performance checks need a hardware GPU ([Windows](windows.md)). |
| Dependencies or `Cargo.lock` | `cargo deny --locked check licenses sources` ([publication](publication.md)). |
| Scripts under `tools/` | `python3 -m unittest discover -s tools/<dir> -p 'test_*.py'`, or `node --test` on the script's `.test.mjs`. |
| Docs only | Check every link, path, command and test name you wrote or whose target you changed. |

## Shared Rust

Private session persistence changes must retain the compile-time field and edit
classification checks and the failure-boundary tests described in
[session recovery](../internals/session-recovery.md#regression-checks). Run core
`package::session`, `package::session_transfer`, `package::session_store` and
`persistence_tests`, UI `session_recovery` and `recovery`, and affected native
restore/close fixtures. Web storage also runs
`node apps/layer-web/restart-store.test.mjs` against its private browser fixture.

Use `session::test_support` for `layer-ui`'s Recorder fixtures, action dispatch,
pointer records, selections and document-space pen input. Platform changes belong
in that test fixture module; production sessions keep the host's platform.

Artwork and filter changes run core `package::effect_records`, `package::codec`
and `package::session` tests. The fixed
[`builtin-contracts.json`](../../crates/layer-core/src/package/codec/fixtures/builtin-contracts.json)
snapshot checks semantic types, choice IDs, dimensions, hard bounds,
constraints, color domain, alpha behavior and time use. Parameter order and shader
ABI belong to renderer interface tests. The fixed
[`authored-filters.capy`](../../crates/layer-core/src/package/codec/fixtures/authored-filters.capy)
checks all built-ins alongside exact raster, watercolor, LUT and SDR values, and
every wire name it lists in the [package contract](../reference/capy-package.md#fixtures-and-acceptance-cases).
Keep resize precision, whole-count controls, choice reordering and private
selection-overlay recovery assertions. Replace superseded pre-release fixtures
without conversion readers; retain the pixel and authored-value checks.
Run `cargo test --locked -p layer-render-wgpu saved_artwork_render_contracts -- --nocapture`
on a hardware GPU for saved rendering semantics. The fixed 32×24 linear RGBA F32
[images](../../crates/layer-render-wgpu/src/fixtures/authored-renders.rgba32f)
and [index](../../crates/layer-render-wgpu/src/fixtures/authored-renders.tsv) cover
all 52 saved built-ins in unsigned and HDR compositions, every blend mode,
placement kernels with and without a mesh, watercolor material edges, authored
SDR rendition and seeded FBM. Shadows/Highlights and Clarity use the real local
tone guide. The index records each case's byte offset, length, maximum absolute
error and RMS tolerance; pointwise color filters have tighter limits than spatial
and procedural evaluation. The baseline was captured with Vulkan on an NVIDIA
RTX PRO 6000 Blackwell GPU. Qualify other hardware backends with the same inputs
and tolerances; a CPU adapter is rejected. Keep these baselines fixed. A change
beyond tolerance requires a regression fix or a new authored data version,
converter and unchanged-file regression. The suite has no baseline-update mode.
A deliberate white-balance coefficient change from `.8` to `.7` must fail;
restoring `.8` must pass.

Spatial evaluation changes also run the renderer's `scene::scale::tests::effects::gaussian`
references, including sigmas above editor bounds, source-edge mass, neutral
values, HDR, native and reduced evaluation. Preserved-package tests cover future
object/resource versions and descriptor fields separately from known corruption
and local admission limits.

Photo writer tests use `photo::test_support::assert_provider_failure` for row
cancellation. Keep each codec's admission, publication and pixel assertions.

Export Again uses `export_again_tests` in `layer-ui`, GTK's
`native_export_again_retains_recipe_and_current_pixels_per_document`, Web's
`--export-metadata` journey and Android's
`AndroidRasterTest#exportAgainRetainsDestinationRecipeAndDrawingOwnership`.
Exercise both themes at narrow and wide widths. Check current pixels with the
retained recipe, per-drawing destinations, cancellation/failure, missing-file
fallback and unchanged editable-master bytes and dirty state. Browser downloads
still require their existing confirmation; picker/handle doubles exercise
transport failures without writing outside the private test profile.

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
The Web pointer fixture records pen-button reports separately from tip contacts
so a held barrel button exercises both input paths.
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
Raster corruption fixtures retain their exact pixel descriptor. File-format
changes must read the checked-in `layer-core/src/package/codec/fixtures` files,
then edit/save/reopen them and compile their current filter implementations.
Keep fixture inputs fixed while their data versions remain supported; do not
regenerate them from the current writer. When removing a superseded pre-release
format, retain its exact value and resource assertions in a current-design fixed
fixture without adding a conversion reader. Protect stable keys,
choice IDs, accepted bounds, all explicit default-valued parameters, LUT samples,
watercolor state and SDR settings independently of editor defaults and GPU layouts.
Renderer float fixtures share readback and presenter setup. Keep pooled and
unpooled memory accounting separate, with each test's pixel and memory limits.
Partial-region fixtures keep their damage bounds; transform oracles keep source
and selection flags and tolerances. Startup fixtures retain readiness gates and
deadlines across compilation phases.
Apple stateless ABI fixtures share string ownership while retaining each entry
point's errors and independent color precision checks.

Precision adjustment checks use `artwork_statistics_tests`,
`levels_statistics_tests` and `curves_calibration_tests` in `layer-render-wgpu`,
with independent scalar references and real source codecs. GTK journeys are
`native_composite_histogram_updates_without_changing_the_drawing`,
`native_curves_histogram_preserves_numeric_focus`,
`native_levels_auto_and_calibration_atomic_history`,
`native_curves_calibration_atomic_history`,
`native_targeted_curves_rgb_contacts_and_cancel` and
`native_targeted_curves_red_contacts_and_cancel`. Run the calibration and
targeted journeys with `--tablet` for their pen contacts. Calibration defaults
to the mouse matrix. Use `LAYER_TONAL_CONTACT=pen` or `touch`,
`LAYER_TONAL_ROLE=Black`, `Gray` or `White`, and `LAYER_TONAL_THEME=Light` or `Dark`
for one calibration contact per process, so the grouped menu opens before any
tablet contact. Run at narrow and wide widths in both themes.
`native_compact_graphs_photo_review` checks the compact Properties controls on a
real portrait; `native_waveform_photo_sources_channels_and_layout` checks the
separate monitor's plotted traces, source/channel controls and dock/float/tab/hide
lifecycle. GPU Waveform tests require spatial reversal, exact histogram count
conservation, profile/HDR/tiny-alpha classification and odd-sized preview grids.
`native_targeted_curves_motion_and_latency` measures mouse contacts on the
private 120 Hz display. Run it with `LAYER_NATIVE_INPUT_TRACE=1` to record input,
shared curve adoption and presentation times. Its small desktop fixture does
not qualify the reference tablet canvases.

Pointwise color adjustments use `color_adjustment_schema_tests` in `layer-core`,
the Colorize and Threshold Properties tests in `layer-ui`, and `native_effects::color`
and `scale::tests::color_effects` in `layer-render-wgpu`. The GPU references
cover physical hue membership, extended RGB, tiny alpha, tagged colors and
reduced-resolution previews independently of catalog screenshots.
Selective Color references lock original-color membership and Black scaling;
Channel Mixer references cover signed coefficients, constants and independent
Monochrome rows. Both preserve hidden page values through undo and project I/O.

Gradient definitions use the core `gradient` tests, shared `shared_gradient`
session tests and renderer gradient fixtures. GTK's
`native_gradient_editor_modes_contacts_and_archive` exercises fill and map
controls, numeric and color editing, cancellation and exact reopen.
`native_gradient_tool_editor_shapes_and_reverse` covers each shape and Reverse,
painted pixels, Quick Mask and saved selections. Run both at narrow and wide
widths in both themes; scalar mask output must use Gray mode explicitly.

Source-aware Shadows/Highlights, Clarity and Dehaze use `effect_analysis_tests` in
`layer-ui`, `snapshot::tests::local_adjustments` and `effect_analysis_lease_tests`
in `layer-render-wgpu`. Exercise first use without a settled preview, two stacked
effects, noncontiguous group storage, clipped input, retained geometry, hidden
failed backing, animated frozen phases, cancellation and renderer replacement.
Suspension, parked document activation and renderer replacement must succeed
when the previous backend rejects analysis retention. Suspended frames must not
retire resources on that backend; active retention failures still propagate.
GTK `native_diagnostics_and_gpu_failure_recovery` and `native_document_tab_input`
exercise the corresponding recovery and parked-document journeys.
Exact consumers include merge, Frequency Separation, color conversion and Clone
reference sampling. The existing `local_tone` GPU fixtures compare fractional
area reduction, finite/subnormal input and actual consumers against independent
f64 equations. Lease tests count real guide identities and request/publication
callbacks across 100 amount edits, including snapshot retention after live
removal; reserved bytes are not a driver allocation peak. Dehaze fixtures also
check integer rank metadata, airlight ties, bounded transmission, both amount
directions and exact neutral output. Android's
`registeredDehazeMatchesIndependentTwoPixelAirlightTie` checks the registered
shader against a scalar reference with reversed source order. Run
`localAdjustmentsUpdateStackedSourcesAndPersistExactOutput` for the native
Properties, source invalidation and persistence journey.

Color Lookup uses `lut3d_tests` in `layer-core`, `layer-ui` and
`layer-render-wgpu`. These cover parser bounds, indexed payload integrity,
history ownership, stale imports, independent tetrahedral/profile references,
stacked resources, native-resolution output and buffer reuse during edits.
Run GTK `native_color_lookup_import_replace_and_persistence` at narrow and wide
widths. Web `--lookup-transport` and Android
`colorLookupImportPresetsAndResourceLifetime` exercise the preset selector and
native file picker, worker parsing, cancellation and stale imports, replacement,
save/reopen and renderer or Activity recreation. All three hosts embed imported
tables in the drawing and use the same shared import/history actions.

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
files, missing English identities in every registered language, undeclared translated
variables, missing
message/term/attribute references and formatting errors. They exercise complete
dynamic messages with Russian plural counts and literal Unicode, combining marks,
emoji, quotes, braces and long filenames. Direct Fluent formatting checks expose errors before
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
  Live paint pairs also need `native_color_pair_updates` on GTK and Web
  `--color-panel`: these inspect retained toolbar/header overlap pixels after
  selection-only changes, temporary paint, mask editing, alpha and HDR rendition
  changes. Static asset captures do not exercise these updates.
  Android uses `AndroidColorPanelTest#retainedPaintIconsAndCompactControlFollowCommittedContext`
  and `#retainedPaintIconsUseMappedRenditionAndIgnorePickerHover` on a reserved tablet.
  Windows uses `exercise-compact-color.ps1 -Journey pair-dark` and `pair-light`,
  or VM fixture selections `compact-color:pair-dark compact-color:pair-light`.
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

- `layer-render-wgpu --test merge`: seven of nine tests fail pixel comparisons
  on an unmodified `b3f6f8e51` export, with maximum differences of 56–166 U8
  codes. Stroke-only, adjustment, clipping, merge, hidden-pixel and placed-photo
  cases reproduce independently of the gradient changes; the blur/save and
  stamp-visible cases pass. Keep the existing pixel bounds when testing changes.

- Renderer `scene::scale::tests::refinement::global_filters_evict_optional_levels_before_rejecting_the_document`
  rejects the document during submission after its initial budget assertions
  pass. The same image-pixel limit failure occurs on the unmodified baseline.
- GTK `native_selection_pen_input`, `native_toolbar_components_narrow_input`,
  `native_workspace_motion_input`, and `native_workspace_switcher_input`
  (intermittent). `native_workspace_resize_input` presents below its rate
  threshold under the runner's `color-mgmt`.
- GTK `native_document_tabs_history_storage_and_close`: the reactivated camera's
  translation differs from the original by about 3e-5.
  `native_embedded_proof_replacement_preserves_local_copy_and_saves_one_profile`
  expects error text the app no longer shows, and
  `native_workspace_unavailable_close_recovery` expects a startup error where
  unavailable storage now starts in memory.
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
- Android `AndroidIconEditorTest#allToolCategoriesModesFiltersAndToolbarIconsRender`
  expects 13 brush categories where the current catalog supplies 16.
- Android `AndroidPaletteTest#palettesPersistAcrossRelaunch` and
  `AndroidWorkspaceSwitcherTest#nativeOptionsVisibilityInputsAndRestart` fail on
  MovinkPad Pro 14. `AndroidWorkspaceOwnershipTest` expects `focus_window` to be
  a workspace ID, but the view now names the owning window too.
- Windows `exercise-tab-pickup.ps1 -Device touch`: after the Layers group is torn
  off, the injected contact reaches neither XAML nor the canvas, so the drag never
  finishes. Mouse passes.
- Windows `exercise-multiwindow.ps1`: "Pin preferences did not refresh in the
  inactive window".
- Windows `exercise-clipboard.ps1`: "Ctrl+X did not write the clipboard", and
  `exercise-persistence.ps1`: "Missing control: Brush size slider" (the slider is
  named "Brush size"). `documents:RecoverGpu` on a VM: "GPU reconstruction did not
  start".
- `layer-ui` `localization::tests::preparation_chunks_preserve_all_message_values_and_attributes`
  fails in Windows checkouts with `core.autocrlf`: three multi-line English messages
  parse into differently split text elements with the same formatted text.
- Windows `exercise-selection.ps1` exits with an access violation in roughly a third
  of runs, on untouched upstream as well, after it closes the Select drawer and
  switches workspaces. The fault is in WinUI's `ScrollView::OnHideIndicatorsTimerTick`
  (Microsoft.UI.Xaml.Controls.dll), which reads released scroll-controller tracker
  references of a panel or drawer `ScrollView` that left the tree.
- Windows fixtures that also fail on the unported upstream build: `exercise-color-picker.ps1`
  ("Pen hover did not preview the paper"), `exercise-pen-buttons.ps1` (Transform is
  enabled on the empty starting layer), `exercise-tab-drag.ps1` ("Attached native
  tab preview did not cross the shared insertion threshold"), `exercise-layer-pickup.ps1`
  ("Layer tab did not begin its native drag" with touch), `exercise-column-stacks.ps1`
  ("Open column 12 did not match native panels, selected tiles and connector
  geometry"), `exercise-persistence.ps1` (the unreadable database writes a
  storage diagnostic to stderr) and `exercise-proof.ps1` (UI Automation times out
  choosing sRGB in Proof Setup after the drawing is saved and reopened).
- iPad XCTest `testCompactMenuShortcutAcrossPages` and
  `testSettingsTextSelectionShortcut`: XCTest keys don't reach UIKit key commands.
- Headless Web `--toolbar-components`, `--tonal-selection`, `--editor`, `--hdr`,
  `--proof`, `--raster`, `--selection-tools` and `--shared-workflows`.
- Tablet Chrome `--workspace-manager` cannot find its new-workspace name input
  on MovinkPad 11.
- Headless Web `--contact-brushes` with `LAYER_BRUSH_PRESETS=20,21` differs from
  the committed watercolor pixels after Redo.
- Headless Web `--zoom-readout` reaches export with an empty stroke image
  and fails its ink assertion.
