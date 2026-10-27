# Testing

[Developer guide](README.md)

Shared tests check document and editor rules without a window. Host tests check
native controls and input. GPU tests check pixels and rendering cost on a
hardware GPU. Device tests check real pen, touch and display behaviour. None
substitutes for another: injected input does not exercise the OS pen driver, a
software adapter or small desktop fixture does not qualify hardware rendering or
a tier's performance, and window-free tests do not establish host coverage.

Ordinary CI runs the window-free shared suites, the Wasm check, the Apple project
check and the license check. The release workflow adds platform builds,
packaging, script suites and software-adapter tests
([release validation](releasing.md#release-validation)). Neither replaces the
hardware checks and journeys for your change.

## Checks by change type

Run every row that matches your change and the area checks under
[Shared Rust](#shared-rust) and [UI and input](#ui-and-input), then walk the
affected user journeys on each affected host in light and dark themes. Scope
Cargo commands with `-p`; `cargo test --workspace` pulls in GPU and platform
crates that a given machine may not be able to run.

| Change | Checks |
| --- | --- |
| Shared Rust (`layer-core`, `-engine`, `-ui`, `-workspace`, `-color`, `-render`) | `cargo test --locked -p <crate>` for each changed crate, then `cargo check --locked -p layer-linux --tests` and `cargo check --locked -p layer-web --target wasm32-unknown-unknown` for the consumers. |
| Renderer, shaders or runtime filters | `cargo test --locked -p layer-render-wgpu <filter>` and `-p layer-host` on a hardware GPU. Frame-path changes need [measurements](#performance). |
| Pen prediction or stroke placement | `cargo test --locked -p layer-engine --features prediction-bench`, then `cargo build --locked -p layer-engine --release --features prediction-bench --examples` and `python3 tools/prediction/replay-bank.py --output artifacts/strokes/bank --analyze --check`. |
| GTK | `cargo test --locked -p layer-linux` (includes the confined `photo_reader` integration test) for the model tests, then each affected journey on the private display: `bash tools/performance/workspace-motion.sh gtk --native-test=<name>`, adding `--tablet` for pen journeys ([Linux](linux.md#tests)). |
| Web | `bash apps/layer-web/build.sh`, the pure tests `node --test apps/layer-web/{run,package,frame,glass,bubbles,pointer,workspace-client,canvas-bar,notice,zoom-readout,export-controls,size-dialog,text-input,localization,numeric,histogram,workspace-manager-copy,toolbar-components-copy,color-controls-copy,document-color-copy,document-recovery,workflow-stack,color-button-lifecycle,gpu-diagnostics,raster-worker-client,pipeline-wait}.test.mjs`, then the affected journeys with `bash tools/performance/workspace-motion.sh web --<journey>` (headed, hardware WebGPU) or `node apps/layer-web/test.mjs --headless --<journey>` against `run.sh`. Packaging changes: `node apps/layer-web/package.mjs && node apps/layer-web/test.mjs --package` ([Web](web.md#tests)). |
| Android | Without a device: `./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug -PcapyAbi=arm64-v8a` in `apps/layer-android`. On a reserved tablet, through `tools/devices/devices.py run`: install both APKs with `adb install -r`, then `am instrument -w -e class art.capycanvas.<Class>[#method] $CAPY_APPLICATION_ID.test/androidx.test.runner.AndroidJUnitRunner`. Read `OK` or `FAILURES!!!`, not the exit status ([Android](android.md#device-tests)). |
| Android distribution | `python3 -m unittest discover -s tools/build -p 'test_android_*.py'`, then the [APK release checks](releasing.md#android-apk) on devices with and without Google services. A local build does not verify the APK that Play processes. |
| Apple | Anywhere: `cargo test --locked -p layer-apple --lib -- --test-threads=1` and `python3 apps/layer-apple/tests/test_icon_assets.py`. On a Mac: the Swift fixtures, the command audit, and the XCTest journeys in the [Apple guide](apple.md#tests). The iOS simulator renders the canvas through the Float32 texel fallback; hardware filtering, Pencil and timing need a physical iPad. |
| Windows | From any OS: `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib` and `cargo clippy --locked -p layer-windows --all-targets -- -D warnings`. On Windows: `apps/layer-windows/scripts/test-without-gpu.ps1`, then the `exercise-*.ps1` fixtures for the changed area; from Linux, `tools/windows-vm/windows-vm.py check` and `fixtures <name>`. A configured [VM GPU pool](windows-vm.md#hardware-gpu-pool) also supports `gpu-check` and `fixtures --hardware-gpu <name>`. HDR output, physical input and performance targets require the corresponding reference hardware ([Windows](windows.md#test)). |
| GTK runtime recipe or SDK setup | The source regressions in the [runtime guide](../../tools/build/gtk-runtime/README.md#build-and-validate) and the release workflow's Arch job; GPU journeys still run on the private display on hardware. |
| Image codecs or color references | The affected Rust suites, the reference generators in `tools/validation/` as the Arch job in `.github/workflows/release-checks.yml` builds and runs them, and the [fuzz targets](#fuzzing-untrusted-input). The remaining ignored color tests need the pinned libavif 1.3.0 qualification SDK or external HEIF references. |
| Dependencies or `Cargo.lock` | `cargo deny --locked check advisories licenses sources` ([publication](publication.md)). |
| Scripts under `tools/` | `python3 -m unittest discover -s tools/<dir> -p 'test_*.py'`, or `node --test` on the script's `.test.mjs`. |
| Docs only | Check every link, path, command and test name you wrote or whose target you changed. |

## Shared Rust

The table names the tests for areas whose coverage is not obvious from the
module layout. Run them with the row for the crate above; GPU tests need a
hardware GPU, and Web, Android, GTK and Windows names are journeys run as their
host guide describes.

| Area | Checks |
| --- | --- |
| Session persistence | Core `package::session`, `package::session_transfer`, `package::session_store` and `persistence_tests`; UI `session_recovery` and `recovery`; affected native restore/close fixtures; `node apps/layer-web/restart-store.test.mjs`. Keep the [regression checks](../internals/session-recovery.md#regression-checks). |
| Workspace startup | `workspace_controls_become_ready_with_the_restored_layout_on_every_platform`; Web `--workspace-startup`. |
| Artwork, filters and file format | Core `package::effect_records`, `package::codec` and `package::session` with the fixed [`builtin-contracts.json`](../../crates/layer-core/src/package/codec/fixtures/builtin-contracts.json) and [`authored-filters.capy`](../../crates/layer-core/src/package/codec/fixtures/authored-filters.capy); on hardware, `saved_artwork_render_contracts` and `saved_angular_filter_render_contracts` with `-- --nocapture`. |
| Vector layers | `vector::tests`; on hardware, `saved_vector_render_contract`. |
| Pointwise color adjustments | Core `color_adjustment_schema_tests`; UI Colorize and Threshold Properties tests; renderer `native_effects::color`, `scale::tests::color_effects` and `filter_weights_preserve_exact_original_and_signed_output_endpoints`. |
| Spatial filters | `scene::scale::tests::effects::gaussian`. |
| Shadows/Highlights, Clarity, Dehaze | UI `effect_analysis_tests`; renderer `snapshot::tests::local_adjustments`, `effect_analysis_lease_tests` and `local_tone`; GTK `native_diagnostics_and_gpu_failure_recovery` and `native_document_tab_input`; Android `registeredDehazeMatchesIndependentTwoPixelAirlightTie` and `localAdjustmentsUpdateStackedSourcesAndPersistExactOutput`. |
| Color Lookup | `lut3d_tests` in `layer-core`, `layer-ui` and `layer-render-wgpu`; GTK `native_color_lookup_import_replace_and_persistence`; Web `--lookup-transport`; Android `colorLookupImportPresetsAndResourceLifetime`. |
| Levels, Curves and scopes | `artwork_statistics_tests`, `levels_statistics_tests` and `curves_calibration_tests`; the [tonal journeys](#tonal-journeys); Web `--tonal-controls`. |
| Gradients | Core `gradient`; UI `shared_gradient`; GTK `native_gradient_editor_modes_contacts_and_archive` and `native_gradient_tool_editor_shapes_and_reverse`. |
| Layer color modes and Add Filter | Core `saved_reduced_color_layers_keep_modes_channel_counts_and_samples`; color `fixed_reduced_color_layers_preserve_modes_and_alpha_through_document_depth_changes`; renderer `layer_color_modes` and `reduced_color`; UI `filter_drawer_tests`; GTK `native_layer_color_modes_and_add_filter`; Web `--filter-drawer`; Android `AndroidInteractionTest#layerColorModesAndLocalFilterMenu`; Windows `exercise-layers.ps1`; painting rates with `android-brush-benchmark.py --color-mode grayscale` and `--color-mode two_tone`. |
| Dither Spread, Scatter, Crystallize | UI `artistic_cell_contract`; GTK `native_artistic_cell_filters`; Web `--artistic-filters`; Android `artisticCellFiltersKeepNativePixelsControlsAndHistory`. |
| Export Again | UI `export_again_tests`; GTK `native_export_again_retains_recipe_and_current_pixels_per_document`; Web `--export-metadata`; Android `AndroidRasterTest#exportAgainRetainsDestinationRecipeAndDrawingOwnership`. |
| Paper | UI `a_late_library_read_cannot_replace_a_newer_paper_choice` and `scaling_paper_preserves_the_pending_library_choice`; host `changing_paper_texture_holds_early_strokes_until_the_new_binding_is_ready`; Android `AndroidRasterTest#paperTextureChangesRetainImmediatePencilContacts`; Web `--paper-texture-readiness` on desktop and Android Chrome; Apple headless app and project fixtures. |
| Liquify | Renderer Liquify tests; GTK `native_liquify_pinch_stroke_on_a_pattern` with `--tablet`; Web `--liquify-journey`. |
| Transforms and capture | Renderer `pixel_transform::program_tests`, `scene::capture_readiness_tests`, `staged_off_canvas_captures_prepare_constant_paper_and_keep_its_frame_extent`, `scene::scale::tests::source_progress` and `correction_gather_`; `transforms_the_shared_page_check_admits_always_plan`; GTK `native_photo_transform_reference_pivot_snap_and_nudge` and `native_distort_near_collinear_corner_recovery`; Web `--capture-readiness`, `--image-commands` and `--transform-footprint`. |
| Merge across raster pages | `merging_trimmed_crossings_preserves_ink_across_raster_pages` on hardware and the Windows software adapter. |
| Pipeline and shader preparation | Renderer `dry_material::tests`, `shader_limits::tests` and `reduction_samplers_live_with_records_not_page_views`; host `dry_brushes_never_compile_while_painting_any_pose` in a debug build; GTK `native_dry_brush_preparation` (with `__GL_SHADER_DISK_CACHE=0` for a cold NVIDIA driver) and `native_late_shader_failure_after_startup`; Web `--pipeline-readiness` and `--drawing-tabs-recovery`. |
| GPU limits and native writeback | `upload_capacity_tests` and `levels_statistics_tests`; Web `--native-writeback` and `--heal-portability`, because browser defaults can exceed the limits native tests request. |
| Watercolor and direct paint | `first_watercolor_frame_batches_page_initialization`, `first_watercolor_contact_on_empty_layer_batches_source_reduction`, `page_zero_copies_reclear_both_formats_and_reduced_extents` and `watercolor_transport_batches_restored_empty_backing`; `direct_contact_paint_and_erase_match_destination_deposition` with `--ignored` on hardware float32 blending. |
| Presentation and submission | The vendored wgpu-core surface tests with `RUSTFLAGS='--cfg wgpu_validate_locks'`, with and without the `trace` feature ([vendored crates](../../vendor/README.md)); renderer `submission::tests`; Android `frontBufferSurfaceLifecycle`. Measure navigation and brush rows separately. |
| Pen tilt | Engine `projected_tilt` and `dry_pose_keys_use_physical_inclination_independent_of_azimuth`; Android `AndroidStylusTest` and `AndroidHostTest#physicalTiltControlsDryBrushCursorAndPigment`; GTK `native_contact_brushes`; Web `--contact-brushes`. |
| Panels, bridges and squircles | Shared `panel_bridges::tests` and `divider_geometry_`; renderer `panel_surface::tests` and `panel_geometry::tests`; GTK `panel_tabs::tests`, `workspace::allocation_tests`, `squircle::tests`, `render_thread::tests::chrome_`, ignored `render_thread::tests::workspace_presenter_prepares_chrome_before_first_panel_parameters`, `native_squircle_corners`, `native_panel_tab_alpha_pixels` and `native_window_scene_pixels`; Android `columnBridgeGapFrameTiming` and `AndroidWorkspacePresentationTest`. |

### Tonal journeys

The GTK journeys are `native_composite_histogram_updates_without_changing_the_drawing`,
`native_curves_histogram_preserves_numeric_focus`,
`native_levels_auto_and_calibration_atomic_history`,
`native_curves_calibration_atomic_history`,
`native_targeted_curves_rgb_contacts_and_cancel`,
`native_targeted_curves_red_contacts_and_cancel`,
`native_compact_graphs_photo_review` and
`native_waveform_photo_sources_channels_and_layout`. Run the calibration and
targeted journeys with `--tablet`, at narrow and wide widths in both themes.
Calibration defaults to the mouse matrix; set `LAYER_TONAL_CONTACT=pen` or
`touch`, `LAYER_TONAL_ROLE=Black`, `Gray` or `White`, and `LAYER_TONAL_THEME=Light`
or `Dark` for one contact per process. `native_targeted_curves_motion_and_latency`
with `LAYER_NATIVE_INPUT_TRACE=1` records input, curve adoption and presentation
times on the private 120 Hz display.

### Writing shared tests

- **Test threads.** Renderer tests create and destroy their own GPU device, so
  [`.cargo/config.toml`](../../.cargo/config.toml) runs four test threads unless
  `RUST_TEST_THREADS` or `--test-threads` says otherwise. Some Linux drivers
  limit live Vulkan devices per process or deadlock when many threads create and
  destroy devices; use `--test-threads=1` for device-heavy filters. Headless
  renderers keep one wgpu instance for the process lifetime, because reloading
  the driver eventually exhausts static TLS.
- **Software adapter.** `layer-render-wgpu` unit tests and `software-adapter-tests`
  builds accept a CPU adapter when `LAYER_TEST_SOFTWARE_GPU=1` is set. Those runs
  check numbers only; production hosts always require a hardware GPU. Run
  `raster_capture_waits_share_the_selected_gpu_budget` with and without
  `--features software-adapter-tests` after changing feature forwarding. The
  Windows runner is described in the [Windows guide](windows.md#test).
- **Fixed fixtures.** File-format changes read the checked-in
  `crates/layer-core/src/package/codec/fixtures` files, then edit, save and
  reopen them. Never regenerate fixture inputs from the current writer. When a
  superseded pre-release format goes, move its value and resource assertions to
  a current-design fixed fixture without a conversion reader
  ([package contract](../reference/capy-package.md#fixtures-and-acceptance-cases)).
- **Render baselines.** The saved artwork, angular filter and vector contracts
  compare against fixed linear RGBA F32 images in
  `crates/layer-render-wgpu/src/fixtures`, captured with Vulkan on an NVIDIA RTX
  PRO 6000 Blackwell; each index row holds its case's tolerances. Other hardware
  backends qualify against the same inputs and tolerances, and a CPU adapter is
  rejected. There is no baseline-update mode: a change beyond tolerance needs a
  regression fix or a new authored data version with a converter and an
  unchanged-file regression. A reviewed appearance change to an evolving
  procedural filter under the [package policy](../reference/capy-package.md#adding-controls-after-ga)
  replaces only that filter's rows.
- **Shared helpers.** Use `session::test_support` for `layer-ui` Recorder
  fixtures, action dispatch, pointer records, selections and document-space pen
  input; platform overrides belong there, not in production sessions. Photo
  writer tests use `photo::test_support::assert_provider_failure`; presentation
  tests create surfaces with the renderer's `create_target`.
- **Settle before reading.** Completed-stroke fixtures wait for
  `has_pending_document_edits` to clear before reading pixels or starting
  another edit: draining input does not publish delayed pen-up work, and a
  smoothed pen-up settles for 32 ms. Observe a frame before asserting that no
  ink was deposited. Undo availability reports history entries, not raster
  backing readiness. Native GPU fixtures fit the current viewport and project
  authored points through that camera.
- **Previews.** Check a moving preview in the display cache: document export
  readback rebuilds a full-resolution result instead.
- **Cold devices.** Readiness and cold-capture tests use a fresh device; a warm
  native device can hide missing preparation.
- **Storage.** Storage-folder tests pass the override and platform folders
  explicitly and must hold under a private `CAPY_STORAGE_DIR`. SQLite fixtures
  canonicalize paths before `rusqlite::Connection::open`, as the store does; on
  Windows run them in a private directory whose database paths exceed 260
  characters.
- **Shaders.** Pass only the pipeline constants an entry point uses, because
  Safari rejects unused overrides, and declare no struct member after a `bool`,
  because Mali's compiler crashes on it. `dry_material::tests` checks both
  without a GPU. Native Metal, Chrome and Safari use different shader compilers,
  so check browser compiler failures in that browser. Generator fixtures load
  exact pixel bits at run time; compilers may flush subnormal shader literals.
- **Expected panics.** Timed worker-recovery fixtures inject expected unwinds
  with `resume_unwind` so panic-hook backtraces do not consume their deadline.

## Localization

Run `cargo test --locked -p layer-ui localization_catalog_tests` for catalog
changes, then the shared Rust and consumer checks above when Rust changes. These
window-free tests format every message directly, so errors surface before the
runtime English fallback can hide them.

Live language changes run `cargo test --locked -p layer-ui language_` and the
full changed-crate suites, then the host journeys in both themes:
`workspace-motion.sh gtk --native-test=native_live_language_switching`,
`workspace-motion.sh web --language-switching`, Android `AndroidLanguageTest` and
Windows `scripts/exercise-localization.ps1`. Run the affected dialog and
composition suites too. Synthetic composition and InputConnection coverage do
not establish genuine CJK IME behavior.

For an advertised language, walk every affected host in both themes, including
search, rename with IME, Unicode save/reopen, export, unsaved close, validation
errors, workspace history and accessibility. Check narrow windows and increased
system text size. Follow the [localization guide](../ui/localization.md) for
catalog and release rules.

## Fuzzing untrusted input

`fuzz/` holds `cargo fuzz` targets for the shared readers of outside files:
`import` (photos and `.capy` through the shared import policy), `workspace`
(workspace and toolbar packages), `icc`, `lut` and `palette`. It is its own
Cargo workspace with the same vendored patches. Run a target with a nightly
toolchain and AddressSanitizer, keeping corpora and artifacts outside the repository:

```sh
cd fuzz && cargo +nightly fuzz run -O import /mnt/localssd/fuzz/import -- \
  -fork=8 -ignore_crashes=1 -ignore_ooms=1 -ignore_timeouts=1 -max_total_time=1800
```

Treat an AddressSanitizer report as a memory-safety defect. A panic, timeout or
memory exhaustion is a refused input, not a vulnerability. Run the targets after
changing a decoder, a vendored codec patch or a reader of outside files.

## UI and input

- **Both themes.** Check changed UI in light and dark themes, panels and dialogs
  at narrow and wide widths, and brush changes in both live drawing and replay.
- **Each input device.** Test mouse, touch and pen separately. Input changes also
  need a real stylus on the affected host: pressure, tilt, hover, cancellation
  and fast curves, plus touch navigation and resuming from the background.
- **Drags.** Reorder gestures follow the [drag convention](../ui/drag-and-reorder.md)
  and its [validation matrix](../ui/drag-and-reorder.md#required-validation-when-implementing).
  Assert that movement before a hold does not reorder where a hold is required.
- **Popups on GTK.** Run popup and menu checks without `--tablet`: the tablet
  proxy's synthetic serials cannot take compositor popup grabs.
- **Composed pixels.** GTK's bridge journeys capture the GPU background with the
  native controls; rendering a GTK widget alone omits the GPU underlay. For
  scaled-display edge checks, render in physical pixels and round the cropped
  viewport outward to that grid, since a fractional crop origin adds edge
  coverage absent from the displayed frame. Compare suspected flashes or seams
  with a real compositor recording.
- **Parity.** Web is compared with GTK, and each native port with Web, at the
  same document, workspace, theme, viewport and scale. For GTK and Web, run
  `cargo test --locked --release -p layer-linux native_web_parity_reference -- --ignored --test-threads=1`,
  then `node apps/layer-web/test.mjs --parity` in a headed browser. The
  [visual tools](../../tools/visual/README.md) capture the other hosts; never
  rescale or crop a capture to hide a difference.

| Area | Checks |
| --- | --- |
| Drags and reorder | GTK `workspace-motion.sh gtk --drag-pickup` and `--column-drops`; Web `--drag-pickup`, `--layer-hold`, `--long-press-drag` and `--drag-cursors`; Android `AndroidInteractionTest`; Windows `exercise-layer-pickup.ps1` and `exercise-workspace-pickup.ps1`. |
| Workspace background and header menus | Android `AndroidInteractionTest#emptyWorkspaceBackgroundsOpenMenusWithoutDragging`, `#workspaceBackgroundMenusPreserveControlsAndColumnDoubleTap`, `#contextMenusFollowNativeContactsAcrossTheWindow`, `#contextRequestsKeepTheirSourceAndRetireLateReplies`, `#headerMenusUseContactPointsAndRetireTheirOwner` and `#secondContactCancelsWorkspaceAndHeaderMenus`; Apple `workspace-native-input.swift`; Windows `toolbars:dark`, `toolbars:light`, `header:options` and `header:options-light`. Check popup bounds against off-center contacts in both orientations. |
| Blender controls | Shared `blender_profiles_` and `blender_hidden_pickup_`; GTK `native_blender_control_profiles`; Web `--blender-controls`. |
| Tool bubbles | Shared `serialized_bubble_hit_geometry_preserves_native_contact_regions` and `restoring_workspace_cancels_bubble_contacts_before_reusing_their_ids`; host `outside_bubble_contact_publishes_retained_model_without_painting`; GTK `native_tool_bubbles`; Web `--tool-bubbles`; Android `AndroidToolBubblesTest#nativeBubblesAcrossThemesAndDevices`; Windows `tool-bubbles` and `tool-bubbles:RapidOnly`. Include painting through gaps and waiting for the fade return before editing an arc. |
| Layer thumbnails | GTK `native_layer_thumbnail_squircles` at scales 1 and 2; Web `--layer-hold`; Android `AndroidTitleBarTest#layerRelationships`; Windows `layers:Relationships`. |
| Icons | `node apps/layer-web/test.mjs --icons`; `workspace-motion.sh gtk --icons` at scale 1 and with `LAYER_MOTION_SCALE=2`; Android `AndroidIconTest`; `python3 apps/layer-apple/tests/test_icon_assets.py`. |
| Live paint pairs | GTK `native_color_pair_updates`; Web `--color-panel`; Android `AndroidColorPanelTest#retainedPaintIconsAndCompactControlFollowCommittedContext` and `#retainedPaintIconsUseMappedRenditionAndIgnorePickerHover`; Windows `compact-color:pair-dark` and `compact-color:pair-light`, which cover opaque paints only. Static icon captures do not exercise these updates. |
| Brush slider previews | Shared `slider_stamp`; renderer `brush_stamp`; GTK `native_toolbar_strength_preview_input`; Web `--toolbar-components`; Android `materialHardnessAndCanonicalStampsFollowNativeControls`; Windows `toolbar_stamp_returns_alpha_bytes_and_rejects_stale_context`. |
| Pen response calibration | Pressure and tilt calibration, held-contact admission, engine and localization catalog tests; GTK `native_pressure_calibration` and `native_tilt_calibration` with `--tablet`; the [tonal journeys](#tonal-journeys) when changing `CurveEditor`. The [tilt guide](../ui/pen-tilt.md) covers device profiles and rotation. |

## Performance

[Performance targets](../PERFORMANCE_TARGETS.md) defines the tiers and targets,
and [measuring](../performance/measuring.md) the rules and commands. Keep CPU
submission, GPU completion and presentation latency separate: a short submission
time does not establish a frame rate. Offscreen GPU numbers from
`cargo run --locked --release -p layer-bench -- --scenario <name>`
([benchmark workloads](gpu-raster-benchmarks.md)) compare revisions on one
machine but never replace a measurement on the tier's device.

Regenerate bundled brush previews with
`cargo run --locked --release -p layer-bench -- --brush-previews [dir]` and
bundled filter previews with
`cargo test --locked -p layer-render-wgpu --features bake write_bundled_filter_previews -- --ignored`.
Both write lossy WebP through `layer_render_wgpu::bundled_preview_webp`; GTK
embeds them and the other hosts package or serve the same files.
`every_built_in_filter_has_a_bundled_preview` checks catalog coverage. Regenerate
the affected swatches when a preset's rendered behavior changes.
`cargo run --locked --release -p layer-bench -- --blend-study <dir>` paints every
blender over fixed colour scenes for visual review.

## Known failures on main

These fail on `main` independently of your change. Don't chase them unless they
are your task, keep their assertions, deadlines and bounds, and remove an entry
when you fix it. A commit names an unmodified build that reproduces the failure.

- Metal, M2 Pro Mac mini (`17a4da321`): `dehaze_constant_area_means_keep_exact_airlight_coordinate_ties` reads `0.24999997` against `0.25`.
- Metal, M2 Pro Mac mini (`17a4da321`): `painterly_blends_high_contrast_hdr_without_a_variance_ceiling` reports an HDR scale mismatch at pixel `8272`.
- Intel Iris Xe D3D12 (`8f7a9c77`): `saved_artwork_render_contracts` drifts beyond 0.0002 on `domain_warp/F32`.
- Intel Iris Xe D3D12 (`8f7a9c77`): `native_publication_pipelines_follow_canvas_and_gate_brush` and `idle_filter_preview_requests_refresh_completed_startup_work` exceed their 30-second catalogue deadlines.
- Intel Iris Xe D3D12 (`8f7a9c77`): `layer-host` `a_contact_begun_as_a_transform_opens_is_replayed_whole_once_it_is_prepared`, `a_stroke_once_painting_is_reported_ready_paints_before_catalog_admission`, `canvas_bar_queries_answer_for_the_current_bar`, `thumbnail_work_left_for_canvas_frames_wakes_the_host` and `warp_opens_over_a_large_filled_selection` exceed the 60-second startup wait.
- GTK `native_panel_value_scrub_input` (`fa32b31db`): the touch pass stops receiving value-drag updates after 8 px.
- GTK `native_selection_pen_input`, `native_toolbar_components_narrow_input`, `native_workspace_motion_input` and `native_workspace_switcher_input`: intermittent.
- GTK `native_workspace_resize_input`: presents below its rate threshold under the runner's `color-mgmt`.
- Android `AndroidTitleBarTest`: 7 of 15 cases.
- Android `detachedPanelsKeepBodiesAndWiderResizeTargets`.
- Android `AndroidInteractionTest#menuBodyAndExtendedTabDropsAcrossDevices` and `#drawerTabsKeepActiveColorsAndPadding`.
- Android `AndroidHostTest#cameraNavigationPublishesOnlyReadoutUpdates`: the last assertion.
- Android `AndroidHostTest` `shortcutPageRecordsMultipleBindingsAndPersists`, `shortcutSearchFindsModifiedKeysAndMarksChangedBindings`, `settingsPanesShareTopEdgeAndUseAppScale`, `workspaceUsesNativeMouseAndPenCursors`, `workspaceMenusManageVisibilityNamesAndHistory`, `toolbarManagerSelectsConfirmsDeletesAndRestores`, `filterLayerIconsUsePackagedNames` and `toolbarConfigurationAndGroupCollapseUseTheSharedDefault`.
- Android `AndroidInteractionTest#healingAcrossDevices` and Web `--heal` (MovinkPad 11, MovinkPad Pro 14): look for a Heal tile, but the Photo toolbar holds Heal and Spot Healing in one Healing slot.
- Android `AndroidInteractionTest#clipboardCopyPasteAcrossDevices`: reads the pasted image's own affine, but the paste position is in its layer offset.
- Android `AndroidInteractionTest#layerHandleLongPressDragCancelAndUndo`: expects a finger or pen hold on a row body to reorder; touch and pen leave it to scrolling and the menu.
- Android `AndroidInteractionTest#dodgeBurnAndFrequencySeparationAcrossDevices`: expects a radius above 6 at 45 % of the slider, where the square mapping gives 4.2.
- Android `AndroidInteractionTest#canvasEdgeDoubleTapUsesRecursiveDefaultWidths`: expects fixed column widths where a double tap restores the automatic width.
- Android `AndroidInteractionTest#navigationPreferencesControlWheel`: taps the wheel choice while Preferences slides in.
- Android `AndroidHostTest#touchNavigationHistoryCancellationAndSurfaceRecovery` (TCL TAB 11 Gen 2): cannot find the portrait Settings label `Pen & Input`.
- Android `AndroidRasterTest#hdrBlackIntensityMarkerVisible`: intermittently keeps the last EV value when the drag is cancelled.
- Android `AndroidWorkspaceManagerTest#restoreStartingLayoutPlacesPalettesAfterColorAndProofAfterNavigator` (MovinkPad 11): cannot find its Window menu.
- Android `AndroidRasterTest#profileLibraryKeepsExactCopiesAndPresetOwnership`: intermittently times out waiting for profile-library buttons.
- Android `AndroidPaletteTest#palettesPersistAcrossRelaunch` and `AndroidWorkspaceSwitcherTest#nativeOptionsVisibilityInputsAndRestart` (MovinkPad Pro 14).
- Android `AndroidWorkspaceOwnershipTest`: expects `focus_window` to be a workspace ID, but the view also names the owning window.
- Android `AndroidHostTest#curvesPagesNativeContactsAndExactCoordinates` (`8a819ffa1`; MovinkPad 11, Huion Kamvas Pad 12): a save after the light-theme calibrations finds no `document` request.
- Windows `exercise-layers.ps1` (Iris Xe, `c5624d37`): "Outer editing edge did not use the accent color"; the PrintWindow probe reads `#A94EC1` for `#A94DC1`, and `-Relationships` reads `#AA50C2` in light theme.
- Windows matched Web editor captures (`c5624d37`): accent, caption controls, number rows and workspace controls differ; the Web collector looks for the blend button as a `select` and measures Tool Set children beyond their panel clip.
- Windows `exercise-multiwindow.ps1`: "Pin preferences did not refresh in the inactive window".
- Windows `exercise-persistence.ps1`: "Missing control: Brush size slider" (the slider is named "Brush size"), and the unreadable database writes a storage diagnostic to stderr.
- Windows `exercise-pen-buttons.ps1`: Transform is enabled on the empty starting layer.
- Windows `exercise-proof.ps1`: UI Automation times out choosing sRGB in Proof Setup after save and reopen.
- iPad XCTest `testCompactMenuShortcutAcrossPages` and `testSettingsTextSelectionShortcut`: XCTest keys don't reach UIKit key commands.
- Tablet Chrome `--workspace-manager` (MovinkPad 11): cannot find its new-workspace name input.
