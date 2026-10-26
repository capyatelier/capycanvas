# Testing

[Developer guide](README.md)

Different checks answer different questions. Shared tests exercise document and
editor rules without a window. Host tests exercise native controls and input.
GPU tests check pixels and rendering cost. Device tests establish whether real
pen and display behaviour matches the intended interaction. None substitutes for
another. Ordinary CI runs the window-free shared suites, the Wasm check, the Apple
project check and the license check. The manually dispatched release dry run adds
platform builds, packaging, script suites and software-adapter correctness tests
([release validation](releasing.md#release-validation)). Run the checks for every
area your change touches; hosted validation does not replace hardware journeys.

Shared `panel_bridges::tests` checks the allocated facing-edge geometry in both
opening directions. Renderer `panel_surface::tests` exercises the actual GPU
background without a window: broad constant-size gap caps, flat aligned edges,
mirrored attachments, panel/bridge seams and finite float output during entry
and exit, plus replacement through an empty scene and unrelated receivers with
target retention enabled and disabled.
Remote-anchor and single-anchor GPU cases check whole unattached corners on
both sides of a multi-panel column, as well as the adjacent gap's cap and core.
The six-pixel corner-entry case samples the whole original corner void through
the transition, mirrored in both axes at 1× and 2× with light and dark fills.
It checks gradual fill from the eligibility cutoff through full attachment.
The connected-edge case moves a real panel gap past the tile bottom and its
vertical reflection. Source-edge and bridge-core samples check continuity
through the original corner radius, including a gap just below the tile.
Renderer audit cases compare panel glass against the existing blur at odd-sized
viewports and after cached reprojection, exclude absent sidebar shadows, and
preserve normalized asymmetric facing corners.
Shared `divider_geometry_` tests cover measured-header coordinate round trips,
removing active resize sources through tab merges, late events and separate
resize/move Undo/Redo entries. Shared workspace-navigation tests hold a divider
drag across camera movement, viewport resizing, unchanged frames and a return to
the original view;
retained presentation stays ineligible until a new workspace gesture begins.
Dispatch synchronizes eligibility on success and error returns, including resize
errors that retire the gesture.
GTK `panel_tabs::tests` checks that unconnected
tab corners stay transparent without a native window.
GTK `workspace::allocation_tests` checks fixed gaps, outer edges and equal,
opposite sibling resizing across fractional divider positions and resolved layouts.
The ignored no-window GPU check
`render_thread::tests::workspace_presenter_prepares_chrome_before_first_panel_parameters`
exercises the GTK presenter factory for SDR, HDR and color replacement. It checks
that chrome prepares before the first nonempty panel/glass parameters arrive.
GTK's bridge journeys capture the GPU background and the composed
native controls; rendering a GTK widget alone omits the GPU underlay.

Renderer `panel_geometry::tests` compares native vector fill with the shared
field through both corner-entry directions, zero-level plateaus, short panes,
multiple anchors and gap partitions. The actual GPU differential tests compare
those vectors and their backdrop triangles with the GTK field at multiple
scales and both reflections. UI vector-cache tests check complete geometry
keys, stable facing receiver membership through anchor entry/exit and the packed
transport; host tests check density-only changes at an unchanged logical viewport
and content-revision subscriptions through proof edits and geometry-only motion.
Android's `columnBridgeGapFrameTiming` captures real compositor keyframes
through both tile edges. It checks every physical row along the bridge face and
the selected source color, including the split-gap case. Ordinary opening,
resizing and tab-drop journeys still run with mouse, touch and pen in both
themes. `AndroidWorkspacePresentationTest` checks that nested layout updates
retain ancestor and sibling reader scopes while changed fields invalidate their
readers, including removing and restoring JSON-null keys.
Windowless retained-Navigator tests check placement and image changes, inherited
proof samples, viewing options, empty placement lists and target replacement.
Source-pixel assertions require the final Navigator image to follow a refined
composition, then become idle without another input event.
Host snapshot-query tests preserve pending UI and host-service publications in
both transport modes, including the model baseline and prepared geometry across
a density change. Complete snapshot reads do not acknowledge streamed updates.

GTK `squircle::tests` checks corner and shadow conversion beneath bridge masks
and effect wrappers without a window. `render_thread::tests::chrome_` checks
unpublished requests, coalescing behind a held buffer, stale releases and idle
expiry with supplied timestamps. `native_squircle_corners` also downloads float
pixels for masked shadows with zero, partial and full connected-corner radii.
For scaled-display edge checks, render in physical pixels and round the cropped
viewport outward to that grid. A fractional crop origin can introduce edge
coverage absent from the displayed frame; compare suspected flashes or seams
with a real compositor recording.

Native-tile writeback tests request both four and eight storage buffers per
stage and require the same portable RGBA and gray-alpha batch sizes. The Web
`--native-writeback` journey records actual browser-device limits and encoder
bindings, paints grayscale and two-tone tiles, and checks pixels plus exact
Undo/Redo and saved-file reopening. Browser defaults can exceed lower requested
limits, so native limit checks do not replace this journey.

The Web `--heal-portability` journey checks Spot Healing's real direct and
indirect dispatches against its explicit bind-group layouts. It waits for
healing and raster publication before checking mouse/pen pixels and Undo.

`upload_capacity_tests` uses actual device limits to reject oversized dab/style
growth, counts reordered duplicates, and checks malformed batch/index refusal
before GPU buffer, style-cache or tile-range changes. Pure capacity tests cover
capped geometric growth and arithmetic overflow. The sparse Dither contact
oracle compares healthy reordered uploads with rectangular exact pixels.

`shader_limits::tests` checks per-entry workgroup storage reachability, WGSL
layout and WebGPU's per-variable padding without a GPU. The preparation tests
exercise exact-limit acceptance and padded overflow refusal before pipeline
creation. `levels_statistics_tests` covers every histogram spill bin and exact
counts across capture windows on devices requested with both 16,352 and 16,384
workgroup bytes, which share one kernel. The GTK Levels Auto and Web `--tonal-controls`
Auto journeys check rendered results and history through the shared kernel.

The renderer's `dry_material::tests` module also runs in CI without a GPU.
`every_kernel_entry_resolves_its_overrides` validates each generated dry-material
kernel and the destination and reservoir modules of every material operation.
`specialized_modules_omit_unreachable_features` requires every specialized
module to leave out the relief, bristle, retouch, blend-mode, pointwise and other
operation code it cannot reach. The first test checks pipeline constants against the overrides used by
each entry point, including called helpers. Supply only constants used by that
entry point: Safari versions can reject unused overrides even when the module
declares them. This catches the dry-brush recipe mismatch; browser compiler
failures still need checks in the actual browser. Native Metal and Chrome use
different shader compilers from Safari. The same test rejects a struct member
declared after a `bool`: Mali's compiler, in the Android driver and in the Mali
Offline Compiler, crashes on such a struct, and the app closes at the first
stroke.

Renderer `reduction_samplers_live_with_records_not_page_views` validates the
actual composed display shaders and rejects per-page sampler ownership.

Presentation changes run the vendored core's surface tests with
`--cfg wgpu_validate_locks`, including the `trace` feature. They exercise pending
artwork, overlapping viewport submissions, output ownership and error callbacks.
Renderer `submission::tests` checks that copies and clears retain their artwork
dependency, camera motion reuses style uploads and changed-cell masks, and
discarded style or backdrop uploads are encoded again. Android's
`frontBufferSurfaceLifecycle` checks buffered navigation across capture, detach,
rotation and GPU recovery. Run the reference-tier navigation and brush rows
separately: presented navigation frames do not establish fresh brush throughput.

The renderer's `first_watercolor_frame_batches_page_initialization` bounds the
passes needed for the first contact across four tiles.
`first_watercolor_contact_on_empty_layer_batches_source_reduction` bounds cold
source preparation above a photo, repeats after Undo and checks the committed
pixels against the exact renderer.
`page_zero_copies_reclear_both_formats_and_reduced_extents` checks exact zero
pixels, preview invalidation and zero render/compute passes when reusing full
and reduced paint and scalar pages.
`watercolor_transport_batches_restored_empty_backing` repeats the multitile
prediction, cancel, commit and restore fixture after restoring empty native
backing, retaining its three-pass transport bound.

The renderer's `direct_contact_paint_and_erase_match_destination_deposition`
requires hardware float32 blending and runs with `--ignored`. It compares actual
direct paint and erase with destination deposition; portable blending devices
use a different GPU route.

The renderer's `pixel_transform::program_tests` validates every generated transform
program without a window or GPU and checks that unused mapping, filtering and
display algorithms are absent. Independent GPU transform oracles qualify the
sampling math and borders. Snapshot tests compare sparse capture with full-page
capture and preserve default-one mask deviations. The cold default-one mask
capture uses a fresh device and requires the scene-composition pipelines to
remain unused; a warm native device can otherwise conceal missing preparation.
Cold effect captures run `scene::capture_readiness_tests` and
`staged_off_canvas_captures_prepare_constant_paper_and_keep_its_frame_extent`.
They require actual fused jobs to defer before encoding, preserve ordered jobs
and source ranges, then resume with the expected pixels. The Web journey
`node apps/layer-web/test.mjs --capture-readiness` covers hidden separators,
Merge Visible and Undo, channel-input prefixes, and GPU lookup preparation.
It records asynchronous pipeline creation and dispatch in the page and workers;
run both themes on hardware WebGPU. Crop followed by Reveal All also runs in
`--image-commands` to exercise constant-generator fallbacks outside the canvas.

`scene::scale::tests::source_progress` uses an 80-page image and the real minimum
64-slot decoded cache. It checks monotonic source prefixes, exact previous
presentation during preparation, distinct but value-equal image storage, and
independent final pixels. `correction_gather_` tests retain partial work across
retries and invalidate it for discarded encoding, changed input identities,
opacity or scope; repeated identical transform damage must still progress.

GTK's `native_late_shader_failure_after_startup` completes startup, then queues
a compiler failure and requires the render owner to return its recorded cause
without another startup request. Run it on the private hardware test display.

Brush preparation changes run layer-host's
`dry_brushes_never_compile_while_painting_any_pose` in a debug build: it paints
every dry preset at three sizes through pressure and tilt extremes, and the
renderer asserts that each selected dry kernel was prepared before the stroke.
GTK's `native_dry_brush_preparation` selects dry presets through the toolbar,
reports each preparation time and the slowest main-loop iteration, then paints.
Run it with `__GL_SHADER_DISK_CACHE=0` on NVIDIA to measure a cold driver.

Tilt changes also run the engine's `projected_tilt` and
`dry_pose_keys_use_physical_inclination_independent_of_azimuth` tests, Android's
`AndroidStylusTest` and `AndroidHostTest#physicalTiltControlsDryBrushCursorAndPigment`,
GTK's `native_contact_brushes`, and Web's `--contact-brushes` journey. The host
journeys check physical writing and side poses, diagonal cursor and pigment
geometry, and exact Undo/Redo where the journey supports history.

Native compiler tests require two independent pipeline recipes to enter before
either completes, keep data and optional work serial, hold cache finalization
until required work finishes, and retire both workers without blocking teardown.

Pipeline preparation changes also run the Web `--pipeline-readiness` journey.
It intercepts real GPU device calls on the page and in workers, requires zero
blocking pipeline creation during startup and cold painted and linked-mask
transforms, and holds asynchronous preparation to check ordered edits. It injects
a worker panic, checks its visible cause and unchanged artwork, then verifies
retry on a fresh worker and exact linked color/coverage Undo/Redo. Worker-client tests also
require runtime panics to reject pending requests and retire only the failed
worker, preserving the recorded cause, and require each worker to receive the
shared compiled module once, including replacements after idle retirement.
Native pixel tests do not
establish this browser contract. Web `--drawing-tabs-recovery` holds the decoder
while restoring native paint without imported images, then checks restored
pixels and Undo/Redo. This prevents optional preparation from hiding a missing
restoration dependency. The GTK
`native_photo_transform_reference_pivot_snap_and_nudge` journey checks temporary
panning, accepting a rotation by selecting Pencil, and exact Undo/Redo pixels
in both themes. `native_distort_near_collinear_corner_recovery` (GTK) and Web
`--transform-footprint` drag a Distort corner onto its neighbours' line on a dense
2048-pixel photo with Bilinear and Bicubic, then back out, Apply, paint, Undo/Redo
and save/reopen. The windowless `transforms_the_shared_page_check_admits_always_plan`
property test splits every admitted random pose with the renderer's planner.

Liquify's `native_liquify_pinch_stroke_on_a_pattern` GTK journey and Web
`--liquify-journey` cover all eight modes, tool switching, exact Undo/Redo,
zero Strength, Adjust and Reset in both themes. Run the GTK journey with
`--tablet` on the private display. Renderer Liquify tests additionally check
composed translations, integer Crystals jumps, alpha-lock transitions,
selection and tile boundaries, minification, untouched preview pixels and
continuation after restoring an older map. Regenerate the affected bundled
swatches when a preset's rendered behavior changes.
Fit-view tests also require unchanged authored color during a moving preview,
all previously touched stroke regions to remain visible, and byte-exact final
maps and color after an empty pen-up. Inspect the display cache directly:
document export readback rebuilds a full-resolution result and does not inspect
the moving Fit preview. Repeating an unchanged Fit preview must perform no
additional composition or display-mip work. Amount gestures must leave pixels and
history untouched until release, restore the prior value on cancel, and ignore
stale events after a target change.

## Checks by change type

The release validation Arch job installs the package recipe's dependencies into
a fresh Arch container, tests SDK validation, and builds the default GTK
development launcher. Its XMLB introspection dependency enables the localized
store metadata test, and colord's installed ICC profiles exercise working-profile
recognition.
It verifies the private GTK/GLib/Pango library paths and text/codec behavior,
then repeats the launcher build offline. Changes to the runtime recipe or SDK
setup run these same checks and the source regressions in the
[runtime guide](../../tools/build/gtk-runtime/README.md#build-and-validate).
GPU journeys still run through the private display on hardware.

Release checks also run the large AVIF gain-map grid test in release mode and
compare WebP decoding against checksum-pinned public-domain samples decoded by
the system libwebp. Arch checks independent LittleCMS CMYK and print-proof
references, plus CMYK, YCCK, progressive and oriented JPEG interchange against
Pillow and ImageMagick. Reference reports record tool versions and source hashes.
The independent LittleCMS oracle is built from pinned
[upstream commit 6a5a8905](https://github.com/mm2/Little-CMS/commit/6a5a89058cc0ef7e700c849d71a447e17fd07086), which restores neutral black-point
detection missing in 2.19. Its source revision is retained in `lcms-source.log`;
`LD_LIBRARY_PATH` selects this library for the reference generators.
The remaining ignored color tests require the pinned libavif 1.3.0 qualification
SDK or external HEIF references, or only emit fixtures for hardware journeys.

Run the rows that match your change, then walk the affected user journeys on each
affected host. Scope Cargo commands with `-p`; `cargo test --workspace` pulls in
GPU and platform crates that a given machine may not be able to run.

| Change | Checks |
| --- | --- |
| Shared Rust (`layer-core`, `-engine`, `-ui`, `-workspace`, `-color`, `-render`) | `cargo test --locked -p <crate>` for each changed crate, then `cargo check --locked -p layer-linux --tests` and `cargo check --locked -p layer-web --target wasm32-unknown-unknown` for the consumers. |
| Renderer, shaders or runtime filters | `cargo test --locked -p layer-render-wgpu <filter>` and `-p layer-host` on a hardware GPU. Frame-path changes need [measurements](#performance). |
| Pen prediction or stroke placement | `cargo test --locked -p layer-engine --features prediction-bench`, then `cargo build --locked -p layer-engine --release --features prediction-bench --examples` and `python3 tools/prediction/replay-bank.py --output artifacts/strokes/bank --analyze --check`. |
| GTK | `cargo test --locked -p layer-linux` (includes the confined `photo_reader` integration test) for the model tests, then each affected journey on the private display: `bash tools/performance/workspace-motion.sh gtk --native-test=<name>`, adding `--tablet` for pen journeys ([Linux](linux.md#tests)). |
| Web | `bash apps/layer-web/build.sh`, the pure tests `node --test apps/layer-web/{run,package,frame,glass,bubbles,pointer,workspace-client,canvas-bar,notice,zoom-readout,export-controls,size-dialog,text-input,localization,numeric,histogram,workspace-manager-copy,toolbar-components-copy,color-controls-copy,document-color-copy,document-recovery,workflow-stack,color-button-lifecycle,gpu-diagnostics,raster-worker-client,pipeline-wait}.test.mjs`, then the affected journeys with `bash tools/performance/workspace-motion.sh web --<journey>` (headed, hardware WebGPU) or `node apps/layer-web/test.mjs --headless --<journey>` against `run.sh`. Packaging changes: `node apps/layer-web/package.mjs && node apps/layer-web/test.mjs --package` ([Web](web.md#tests)). |
| Android | Without a device: `./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug -PcapyAbi=arm64-v8a` in `apps/layer-android`. On a reserved tablet, through `tools/devices/devices.py run`: install both APKs with `adb install -r`, then `am instrument -w -e class art.capycanvas.<Class>[#method] $CAPY_APPLICATION_ID.test/androidx.test.runner.AndroidJUnitRunner`. Read `OK` or `FAILURES!!!`, not the exit status ([Android](android.md#device-tests)). |
| Apple | Anywhere: `cargo test --locked -p layer-apple --lib -- --test-threads=1` and `python3 apps/layer-apple/tests/test_icon_assets.py`. On a Mac: the Swift fixtures, the command audit, and the XCTest journeys in the [Apple guide](apple.md#tests). The iOS simulator renders the canvas through the Float32 texel fallback; hardware filtering, Pencil and timing need a physical iPad. |
| Windows | From any OS: `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib` and `cargo clippy --locked -p layer-windows --all-targets -- -D warnings`. On Windows: `apps/layer-windows/scripts/test-without-gpu.ps1`, then the `exercise-*.ps1` fixtures for the changed area; from Linux, `tools/windows-vm/windows-vm.py check` and `fixtures <name>`. A configured [VM GPU pool](windows-vm.md#hardware-gpu-pool) also supports `gpu-check` and `fixtures --hardware-gpu <name>` for hardware renderer and document checks. HDR output, physical input and performance targets require the corresponding reference hardware ([Windows](windows.md)). |
| Dependencies or `Cargo.lock` | `cargo deny --locked check advisories licenses sources` ([publication](publication.md)). |
| Scripts under `tools/` | `python3 -m unittest discover -s tools/<dir> -p 'test_*.py'`, or `node --test` on the script's `.test.mjs`. |
| Docs only | Check every link, path, command and test name you wrote or whose target you changed. |

Android distribution changes run `python3 -m unittest discover -s tools/build -p 'test_android_*.py'`.
The release downloader verifies the actual APK's runtime dependencies, standalone
installation, device coverage and signing identity. Follow the
[APK release checks](releasing.md#android-apk) on devices with and without Google
services; an unprotected local build does not verify the APK processed by Play.

## Shared Rust

Workspace startup changes run Web's `--workspace-startup` journey. It holds
workspace storage beyond one second and checks every presented frame through fresh
desktop/mobile starts and saved Photo/Zen restarts in both themes. Shared
`workspace_controls_become_ready_with_the_restored_layout_on_every_platform`
checks that readiness and the restored arrangement publish together.

Storage-folder tests supply the override and platform folders explicitly. Their
assertions must hold when the suite sets a private `CAPY_STORAGE_DIR`.

SQLite fixtures that reopen preserved files must canonicalize their paths before
passing them to `rusqlite::Connection::open`, as the store does. Run them in a
private directory whose database paths exceed 260 characters on Windows.

A smoothed pen-up settles for 32 ms before publishing its raster boundary.
Native prediction fixtures wait for the current input to be consumed and its
Engine frame submitted before reading GPU reuse flags. Pen-down need not create
a prediction; require the first live prediction to be cold and a later prediction
to reuse tracked output. Timed pen-up settles in the final untimed drain.

Committed-stroke correction fixtures advance the existing frame helper until
the stroke and native backing publish, then capture their baseline and send the
correction. Keep smoothing enabled and preserve the two-second correction window.

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
Run `cargo test --locked -p layer-render-wgpu saved_angular_filter_render_contracts -- --nocapture`
for the 16 U8/F32 angular cases using the same fixed index and tolerances.
The full saved-artwork contract below remains required.

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
converter and unchanged-file regression. A reviewed appearance change to an
evolving procedural filter under the [package policy](../reference/capy-package.md#adding-controls-after-ga)
replaces only that filter's rows. The suite has no baseline-update mode.
A deliberate white-balance coefficient change from `.8` to `.7` must fail;
restoring `.8` must pass.

Run `cargo test --locked -p layer-render-wgpu saved_vector_render_contract` on a
hardware GPU for vector layers. It renders the fixed
[`vector-objects.capy`](../../crates/layer-core/src/package/codec/fixtures/vector-objects.capy)
at 256×192 against a linear RGBA F32
[image](../../crates/layer-render-wgpu/src/fixtures/vector-renders.rgba32f)
captured on the same GPU, within a maximum error of 0.002 and RMS of 0.0002, under
the same rules. The coverage kernel's `vector::tests` compare every fill rule,
stroke join and cap, width profile and level against an exact supersampled
reference. Interior pressure knots shape the outline between anchors, including
on straight segments; the numerical reference samples those widths independently
of the renderer's geometry subdivision.

`merging_trimmed_crossings_preserves_ink_across_raster_pages` compares native
capture and Merge Down on a drawing spanning several raster pages. Its reference
uses a separate renderer so full-resolution capture cannot warm the bake's
geometry cache. Run it on hardware and the Windows software adapter.

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
in the callers' control. NativeHost GPU fixtures fit the current viewport before
choosing an object-body gesture and project authored points through that camera.
Thumbnail waits retire held native contacts and pending engine work before
checking stroke completion, changed pixels and exact Undo restoration.
Figure and gradient tests share `abandon_layer_drag` while retaining their own
frame timestamps and commit assertions. Collapsed resize fixtures retain the
nested and outside handle coordinates, width checks and cancellation points.
Toolbar fixtures allocate tiles through `insert_tools`. Modifier fixtures compare
their rows with the shortcut page's tool contexts.
Cursor tests observe a frame before asserting that no ink was deposited. Stroke
selection tests check nonempty live batches; undo/redo compares raster identities.
The Web pointer fixture records pen-button reports separately from tip contacts
so a held barrel button exercises both input paths.
Paper library regressions retain newer texture choices when an older image read
finishes, including choosing the already selected texture. Scale edits preserve
pending texture reads. Run `a_late_library_read_cannot_replace_a_newer_paper_choice`
and `scaling_paper_preserves_the_pending_library_choice` in `layer-ui`; late
completion must preserve authored data, dirty state and history while its host
request retires.
Apple Paper bridge regressions use the existing headless app and project
fixtures to import, reload, rename, delete and add library entries. Compare exact
authored documents and source samples, checkpoint/dirty state, request retirement,
worker failure, cancelled/replaced jobs and preview ownership after editor close.
Async adoption must publish one settings-save request with the new paper identity;
cache-only reads must not save it again. Native file delivery, tile presentation
and interactions still require Mac and iPad journeys in both themes.
Paper creation tests compare initial remembered image samples and creation
settings before publication with clean history, explicit New from an uncached
identity, restored authored paper and exact earlier-stroke state. Missing,
corrupt or unavailable library material preserves the fallback and remembered
ID, reports one failure, avoids a repeated automatic Load and permits an
explicit user retry. Keep constructor/capture tests-only negatives separate
from prepared-material API adaptations.
Paper preparation also runs
`changing_paper_texture_holds_early_strokes_until_the_new_binding_is_ready` in
`layer-host` on a hardware GPU. A whole stroke sent before the changed paper is
ready must wait, commit once and be removed by one Undo. Android's
`AndroidRasterTest#paperTextureChangesRetainImmediatePencilContacts` checks that
boundary through Vulkan/JNI in both themes.
Web's `--paper-texture-readiness` holds genuine new Paper pipeline completion
while a whole Pencil contact arrives, then checks intact pixels and exact
Undo/Redo in both themes on desktop and Android Chrome.
Presentation tests use the renderer's `create_target` for their surfaces; preserve
each test's format and extent, and validate its usage flags on the test GPU.
Renderer source fixtures use `test_support::depth_source` with their original
budget and coordinate mapping. Oversized decoded-source fixtures require warm
resident tiles to be reused before misses evict them, with fixed slot counts and
GPU bytes. The native Threshold checker verifies evaluation before reduction;
optional display hierarchy residency is not a pixel oracle.
Pen fixtures override the shared event's timestamp
and pressure where their cadence differs.
Integration pen fixtures share sample construction and input draining; callers
retain pressure axes, sequence numbers, cadence and GPU-wait policy.
Completed-stroke fixtures settle `has_pending_document_edits` before reading
pixels or starting another document edit; draining input alone does not publish
delayed pen-up work. Live-preview fixtures retain their unfinished contact.
`estimated_input_tokens_reach_committed_stroke_corrections` waits for nonempty
committed pigment before correcting the stroke, then checks pixels, the dirty
checkpoint and one Undo entry.
Preview-scale equality fixtures disable position smoothing so zoom-dependent
Curve fit does not change the authored contact path being compared.
Retouch replay fixtures keep each brush's path and pen-up position. Check source
readiness and healing counts after replay settles, before collecting result pages.
Undo/Redo availability reports history entries; raster backing readiness is separate.
While healing publication is pending, check the exact busy navigation refusal and
unchanged revision and history before replaying queued contacts.
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
with independent scalar references and real source codecs. Their generator
fixtures load exact pixel bits at run time: compilers may flush subnormal shader
literals that also feed Float32 arithmetic. The unit-opacity capture regression
compares raw RGBA bits before histogram classification; copies at unit layer
opacity must preserve subnormal values without Float32 identity arithmetic.
GTK journeys are
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

Paint-layer color modes use `saved_reduced_color_layers_keep_modes_channel_counts_and_samples`
in core, `fixed_reduced_color_layers_preserve_modes_and_alpha_through_document_depth_changes`
in color, and the renderer’s `layer_color_modes` and `reduced_color` tests.
They cover original images, two-channel backing at every document depth,
colored live brush input before publication, scaled previews with in-place and
candidate writeback, exact undo and save/reopen. Native gray-alpha writeback
also covers odd partial regions and untouched packed half words. The shared UI’s
`artistic_cell_contract` checks native RGBA preservation, bounded Voronoi search,
seed stability, transformed coordinates, capture windows and portable GPU limits
for Dither Spread, Scatter and Crystallize. Their real control/history journeys
are GTK's `native_artistic_cell_filters`, Web's `--artistic-filters` and Android's
`artisticCellFiltersKeepNativePixelsControlsAndHistory`; each covers light and
dark themes. Rebuild bundled previews with the existing preview baker.

`filter_drawer_tests` checks captured layer owners and the top of the local
filter chain. Native journeys are GTK’s `native_layer_color_modes_and_add_filter`,
Web’s `--filter-drawer`, Android’s
`AndroidInteractionTest#layerColorModesAndLocalFilterMenu`, and Windows’
`exercise-layers.ps1`. Run both themes. Check Color mode in Properties and Add
Filter in Properties, the Layers footer, and the layer row context menu. Measure painting with
`android-brush-benchmark.py --color-mode grayscale` and `--color-mode two_tone`
on each reference tier.

Pointwise color adjustments use `color_adjustment_schema_tests` in `layer-core`,
the Colorize and Threshold Properties tests in `layer-ui`, and `native_effects::color`
and `scale::tests::color_effects` in `layer-render-wgpu`. The GPU references
cover physical hue membership, extended RGB, tiny alpha, tagged colors and
reduced-resolution previews independently of catalog screenshots.
Selective Color references lock original-color membership and Black scaling;
Channel Mixer references cover signed coefficients, constants and independent
Monochrome rows. Both preserve hidden page values through undo and project I/O.
Run `filter_weights_preserve_exact_original_and_signed_output_endpoints` for
bit-exact zero/full weight, signed output and blend endpoints through both
pointwise and image evaluation.

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

The Windows software-adapter runner uses one test thread for both Rust phases
so independent rendering tests do not submit to software devices concurrently.
It runs every test binary after a failure and preserves uncaptured diagnostics;
the release job also records Windows application crash events.
Timed worker-recovery fixtures inject expected unwinds with `resume_unwind` so
panic-hook backtrace formatting does not consume their recovery deadline. Keep
full panic diagnostics enabled for unexpected failures.

The CPU-only `raster_capture_waits_share_the_selected_gpu_budget` regression in
`layer-render-wgpu` checks both sides of the raster capture deadline. Run it with
and without `--features software-adapter-tests` to check feature forwarding.

`layer-render-wgpu` unit tests and `software-adapter-tests` builds accept a CPU
adapter when `LAYER_TEST_SOFTWARE_GPU=1` is set. Those runs check numbers only;
they are never performance evidence, and production hosts always require a
hardware GPU.

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

- **Blender controls.** Shared `blender_profiles_` and `blender_hidden_pickup_`
  tests cover every Blend preset on every platform, primary/default-arc agreement, secondary section order,
  conditional settings and retained overrides. GTK `native_blender_control_profiles`
  and Web `--blender-controls` exercise every preset in light and dark themes.
- **Tool bubbles.** GTK `native_tool_bubbles`, Web `--tool-bubbles`, Android
  `AndroidToolBubblesTest#nativeBubblesAcrossThemesAndDevices` and Windows
  `tool-bubbles` fixtures exercise native
  contacts in both themes. Include painting through gaps and waiting for the
  specified fade return before editing an arc; a restored model alone does not
  establish that the native presenter accepts input again. Shared
  `serialized_bubble_hit_geometry_preserves_native_contact_regions` compares
  the native serialized hit view with the reference hit test across cards,
  chooser capsules, arcs, beads, wheel, history and empty gaps.
  `restoring_workspace_cancels_bubble_contacts_before_reusing_their_ids`
  requires ring rollback and rejects inherited contact state after replacement.
  The native host's
  `outside_bubble_contact_publishes_retained_model_without_painting` requires
  an outside touch to publish the closed card without a paint or camera frame,
  including when the same contact also dismisses a drawer.
  Windows runs mouse, pen and touch variants in light and dark themes, including
  fast toolbar tear-off, cancellation, pair takeover from an arc and native
  Layers opacity, persistent menus and exact artwork Undo/Redo.
  Its separate `tool-bubbles:RapidOnly` case requires an observed fold followed
  by native reopening within 150 ms, with working retained card controls.
  The shared wheel tests require release to preserve the last sampled color,
  cancellation to restore only the original context and owner removal to
  retire the contact before layout undo reuses its ID. Snapshot tests require
  both retained Layers card pages even when neither panel is docked.
- **Layer thumbnails.** GTK `native_layer_thumbnail_squircles`, Web `--layer-hold`,
  Android `AndroidTitleBarTest#layerRelationships` and Windows `layers` fixtures
  check full squircle clipping, accent borders, preview pixels inside the border
  and mask linking in light and dark themes. Run GTK at scales 1 and 2;
  Windows `layers:Relationships` covers both themes with native pickup paths.
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
  Empty workspace backgrounds use Android
  `AndroidInteractionTest#emptyWorkspaceBackgroundsOpenMenusWithoutDragging`,
  `#workspaceBackgroundMenusPreserveControlsAndColumnDoubleTap`,
  `#contextMenusFollowNativeContactsAcrossTheWindow`,
  `#contextRequestsKeepTheirSourceAndRetireLateReplies`,
  `#headerMenusUseContactPointsAndRetireTheirOwner`,
  `#secondContactCancelsWorkspaceAndHeaderMenus`,
  Apple's mounted `workspace-native-input.swift`, and Windows `toolbars:dark`,
  `toolbars:light`, `header:options` and `header:options-light` fixtures. Check
  held release, cancellation, movement before the hold, secondary clicks and
  child-control ownership without changing the workspace.
  Check actual popup bounds against off-center contacts across both orientations
  and themes, and reject stale replies after replacement, blur or source removal.
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
  `--color-panel`: these inspect retained toolbar, header and Color tab overlap pixels after
  selection-only changes, temporary paint, mask editing, alpha and HDR rendition
  changes. Static asset captures do not exercise these updates.
  Android uses `AndroidColorPanelTest#retainedPaintIconsAndCompactControlFollowCommittedContext`
  and `#retainedPaintIconsUseMappedRenditionAndIgnorePickerHover` on a reserved tablet.
  Windows uses `exercise-compact-color.ps1 -Journey pair-dark` and `pair-light`,
  or VM fixture selections `compact-color:pair-dark compact-color:pair-light`;
  its paints come from Edit Color, which has no alpha, so translucent pairs are
  left to the shared and GTK checks.
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
brush previews with `cargo run --locked --release -p layer-bench -- --brush-previews [dir]`
and bundled filter previews with
`cargo test --locked -p layer-render-wgpu --features bake write_bundled_filter_previews -- --ignored`.
Both write lossy WebP through `layer_render_wgpu::bundled_preview_webp`.
GTK generates its embedded filter-preview lookup from that asset directory;
the other hosts package or serve the same files. Its
`every_built_in_filter_has_a_bundled_preview` test checks catalog coverage.
Blenders, Liquify, the retouching tools and the Eraser preview on greyscale scenes
that fill the swatch: a shaded stroke-shaped tube for most blenders, a halftone
for Liquify, and source and brush rings for Clone and Healing.
`cargo run --locked --release -p layer-bench -- --blend-study <dir>` paints every
blender over fixed colour scenes for visual review.

## Brush slider previews

The shared `slider_stamp` tests validate completed contact plans for every built-in
preset, consume-once buffer ownership, metadata polling and stale request handling.
Renderer `brush_stamp` compares GPU masks with canonical coverage, including dry
pressure-dependent contact and loading, texture, soft edges and completed bristle taps. GTK
`native_toolbar_strength_preview_input` and Web `--toolbar-components`
exercise retained popups, live Strength and texture updates, bookmarks and unchanged
artwork/history in both themes. Android's
`materialHardnessAndCanonicalStampsFollowNativeControls` covers the native controls
and popup. Windows' native `toolbar_stamp_returns_alpha_bytes_and_rejects_stale_context`
checks the asynchronous packet boundary.

## Known failures on main

These fail on `main` independently of your change. Don't chase them unless they
are your task, and remove an entry when you fix it.

- Metal on the M2 Pro Mac mini, reproduced on unmodified `17a4da321`:
  `dehaze_constant_area_means_keep_exact_airlight_coordinate_ties` reads
  `0.24999997` against `0.25`; `painterly_blends_high_contrast_hdr_without_a_variance_ceiling`
  reports HDR scale mismatch `0.9917051793309497` at pixel `8272`.
  Keep both assertions and their pixel bounds.
- Intel Iris Xe D3D12 renderer failures reproduced on unmodified `8f7a9c77`:
  `saved_artwork_render_contracts` reports
  `domain_warp/F32` maximum drift 0.000232696533203125 against 0.0002.
  `native_publication_pipelines_follow_canvas_and_gate_brush` and
  `idle_filter_preview_requests_refresh_completed_startup_work` exceed their
  30-second catalogue deadlines with canvas and brush readiness already true.
  Keep the existing deadlines and pixel bounds.
- On the same unmodified `8f7a9c77` Iris Xe D3D12 build, five `layer-host`
  cases exceed the existing 60-second startup wait:
  `a_contact_begun_as_a_transform_opens_is_replayed_whole_once_it_is_prepared`,
  `a_stroke_once_painting_is_reported_ready_paints_while_background_shaders_compile`,
  `canvas_bar_queries_answer_for_the_current_bar`,
  `thumbnail_work_left_for_canvas_frames_wakes_the_host`, and
  `warp_opens_over_a_large_filled_selection`.
- GTK `native_panel_value_scrub_input` on unmodified `fa32b31db`: the touch
  pass stops receiving value-drag updates after 8 px and leaves Brush size at
  120 px; the mouse pass completes.
- GTK `native_selection_pen_input`, `native_toolbar_components_narrow_input`,
  `native_workspace_motion_input`, and `native_workspace_switcher_input`
  (intermittent). `native_workspace_resize_input` presents below its rate
  threshold under the runner's `color-mgmt`.
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
- Android `AndroidInteractionTest` expectations that differ from the shared
  behaviour, on MovinkPad 11 and MovinkPad Pro 14: `#healingAcrossDevices`
  and the Web `--heal` journey look for a Heal toolbar tile, but the Photo
  toolbar holds Heal and Spot Healing in one Healing tool slot;
  `#clipboardCopyPasteAcrossDevices` reads
  the pasted image's own affine, while the paste position is in its layer
  offset; `#layerHandleLongPressDragCancelAndUndo` expects a finger or pen hold
  on a row body to start a reorder, which touch and pen leave to scrolling and
  the menu; `#dodgeBurnAndFrequencySeparationAcrossDevices` expects a radius
  above 6 at 45 % of the slider, where its square mapping gives 4.2;
  `#canvasEdgeDoubleTapUsesRecursiveDefaultWidths` expects fixed column widths
  where a double tap restores the automatic width.
- Android `AndroidInteractionTest#navigationPreferencesControlWheel` taps the
  wheel choice while Preferences slides in, and reads the option's bounds
  before its menu is laid out.
- Android `AndroidHostTest#touchNavigationHistoryCancellationAndSurfaceRecovery`
  passes recovery and continued drawing, then cannot find the portrait Settings
  label `Pen & Input` on TCL TAB 11 Gen 2.
- Android `AndroidRasterTest#hdrBlackIntensityMarkerVisible` intermittently
  keeps the last EV value when the test cancels its drag.
- Android `AndroidWorkspaceManagerTest#restoreStartingLayoutPlacesPalettesAfterColorAndProofAfterNavigator`
  cannot find its Window menu on MovinkPad 11.
- Android `AndroidRasterTest#profileLibraryKeepsExactCopiesAndPresetOwnership`
  intermittently times out waiting for profile-library buttons.
- Android `AndroidPaletteTest#palettesPersistAcrossRelaunch` and
  `AndroidWorkspaceSwitcherTest#nativeOptionsVisibilityInputsAndRestart` fail on
  MovinkPad Pro 14. `AndroidWorkspaceOwnershipTest` expects `focus_window` to be
  a workspace ID, but the view now names the owning window too.
- Android `AndroidHostTest#curvesPagesNativeContactsAndExactCoordinates` on
  unmodified `8a819ffa1` passes the light-theme calibrations, then a later save
  finds no `document` request (`NoSuchElementException` at
  `CapyDeviceTest.kt:360`) on MovinkPad 11 and Huion Kamvas Pad 12.
- Windows `exercise-layers.ps1` on Iris Xe fails "Outer editing edge did not use
  the accent color": the PrintWindow thumbnail probe reads `#A94EC1` for palette
  `#A94DC1`. The `-Relationships` variant passes that check in dark theme, then
  reads `#AA50C2` in light theme. Both failures and their sampled pixel arrays
  reproduce on unported `c5624d37`. Keep the exact color assertion.
- Windows matched Web editor captures do not pass exact image or geometry
  comparison. Native accent, caption controls, number rows and workspace controls
  differ. The Web collector also looks for the blend button as a `select` and
  measures Tool Set children beyond their panel clip. Native layout and control
  bounds match unported `c5624d37` at 744 and 960 logical pixels; keep the original
  comparison limits and report the differences.
- Windows `exercise-multiwindow.ps1`: "Pin preferences did not refresh in the
  inactive window".
- Windows `exercise-persistence.ps1`: "Missing control: Brush size slider"
  (the slider is named "Brush size").
- Windows fixtures that also fail on the unported upstream build: `exercise-pen-buttons.ps1` (Transform is
  enabled on the empty starting layer), `exercise-persistence.ps1` (the unreadable database writes a
  storage diagnostic to stderr) and `exercise-proof.ps1` (UI Automation times out
  choosing sRGB in Proof Setup after the drawing is saved and reopened).
- iPad XCTest `testCompactMenuShortcutAcrossPages` and
  `testSettingsTextSelectionShortcut`: XCTest keys don't reach UIKit key commands.
- Tablet Chrome `--workspace-manager` cannot find its new-workspace name input
  on MovinkPad 11.

## Pen response calibration

Changes to the shared response editor run pressure and tilt calibration tests,
held-contact admission tests, engine tests and localization catalog checks. Run
GTK’s `native_pressure_calibration` and `native_tilt_calibration` on the private
display with `--tablet`, in both themes. Also run the tonal Curves journey when
changing `CurveEditor`. The [tilt guide](../ui/pen-tilt.md) describes device profiles,
frame conversion and sustained motion diagnostics. Physical display rotation
requires a host with a verified native-to-surface tilt mapping.
