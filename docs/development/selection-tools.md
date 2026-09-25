# Selection tools

GTK, Web, Android, macOS and iPadOS provide Rectangle Select, Ellipse Select, Polygonal Lasso, and Select by Color.
Photo places these alongside Lasso and Auto select in its Tools toolbar. Sketch
uses a Select title-bar command and a two-panel Tools → Tool drawer, matching
the Eraser drawer. The Select command remembers the last chosen selection tool
in the workspace. Windows retains its existing selection entry points.
GTK also provides [Tonal range](tonal-selection.md) for HDR-aware luminance masks.

Preview-less tool rows match brush categories: at least 44 px on GTK/Web and
48 dp on Android.
Select, Brush, and Sculpt opener icons follow their category's remembered tool,
including while another category is active. Rust publishes the icon from shared
workspace memory; hosts update their retained header and toolbar images.
Selection modes use one connected group of icon toggles, with names and explanations
in accessible labels and tooltips instead of permanent text. The overlapping
rectangle symbols follow the familiar New/Add/Subtract/Intersect order.
Select by Color uses the wand motif with RGB sparkles.

Selection gestures, settings, completion/cancellation, document coordinates,
and one-edit history are in `crates/layer-ui/src/selection_tools.rs`. All six
selection tools share New/Add/Subtract/Intersect modes, anti-aliasing, and a
feather radius in image pixels. Rectangle and ellipse also support fixed
ratio/size and center origin. Polygonal Lasso supports 45° edge constraints
through its Tool option or Shift. Polygon vertices remain
transient until closed and are discarded on Escape, focus loss, tool changes,
or pointer cancellation. Existing selections remain intact until completion.
Geometry is independent of zoom; ellipses retain image-space edge precision.

Hold Shift before starting a selection to Add, Alt to Subtract, or Shift+Alt to
Intersect. Ctrl/Cmd temporarily chooses New. The operation is latched for the
whole gesture (including a polygon); releasing keys does not change a pending
result or the remembered tool mode. Shift/Alt pressed after the initial contact
constrain shape geometry instead. Paint selection keeps its two-mode behavior:
Alt swaps Add/Subtract and Shift chooses Add. All modes have visible controls.

Select by Color shares region classification, tolerance, visible/editing/reference
sampling, stale-result validation, expansion, and edge smoothing with Auto select.
`RegionRequest::contiguous` controls whether the GPU keeps only the seed's
component or all matching pixels. It never downloads image pixels for CPU
classification. Gap closing applies only to contiguous regions. Existing region
callers explicitly request contiguous behavior. Disabling anti-aliasing removes
the smoothing control and produces hard edges before any requested feathering.

Selection combination and feathering run once on the GPU at gesture completion,
using the existing asynchronous region queue and stale-result checks. Unfeathered
modes skip the floating-point image intermediate. Refined masks retain 8-bit
coverage; legacy four-sample masks remain readable. Painting, fills, layer masks,
and affine transforms consume the same cached mask. Pooled coverage uses alpha
to preserve its precision through color-managed rendering. No selection
classification or feathering runs per brush dab.

The defaults apply to GTK, Web, Android, macOS and iPadOS. Shared migration recognizes only untouched
included layouts and preserves working settings; customized history is retained.
The new icons use the existing SVG bank, and native tool/header/toolbar buttons
retain the application drag and reorder convention.

## Verification

- Shared UI tests cover geometry, transforms, fixed size/ratio, modifier keys,
  empty gestures, polygon editing, cancel/focus loss, undo/redo, workspace memory,
  stale color results, sources, and platform gating.
- Workspace tests cover conservative Sketch/Photo upgrades and saved settings.
- GPU tests compare disconnected color islands, Boolean selection modes,
  anti-aliasing, and Gaussian feathering against independent pixel oracles,
  including transformed masks and brush/fill coverage.
- Web: run `node apps/layer-web/device.test.mjs --selection-tools` with
  `LAYER_DEVICE_CDP` and `LAYER_WEB_URL` pointing at a forwarded tablet Chrome
  endpoint and dedicated test origin. The desktop runner supports the same flag.
- Android: run `AndroidTitleBarTest#selectionDrawerToolsModesAndRememberedIcons`
  and `AndroidRasterTest#selectionToolsRenderAndCombineOnDevice`. They cover
  native mouse/touch/stylus UI contacts and actual Vulkan selection/fill pixels.
- macOS and iPadOS: `cargo test --locked -p layer-apple --target aarch64-apple-darwin
  --lib selection -- --test-threads=1` drives both Apple policies through the
  pointer ABI and Metal (modifier latching, Grow/Shrink, Quick Mask rows, mask
  colors, thumbnails and saved layers). `EditorLaunchTests/testSelectionMasks`
  runs the native journey on each target. Command-click a layer thumbnail to load
  it (Shift adds, Option subtracts); Control-click stays the context menu.
- `native_selection_options_input` exercises mode controls on every tool,
  feather-radius entry, selection combinations, and single-step undo/redo.
- `native_selection_tools_input` exercises GTK mouse/touch tool selection,
  retained two-panel drawers, numeric inputs, canvas gestures, polygon keyboard
  editing, GPU masks, undo/redo, Photo toolbar entries, and light/dark captures.
- `native_selection_pen_input` exercises the six drawer choices and the four new
  tools through native Wayland tablet events. These are injected pen events,
  not physical-device testing. It is separate from numeric-entry testing because
  the test tablet proxy fails its display connection when GTK opens that editor.

Run the native tests through `tools/performance/workspace-motion.sh gtk
--native-test=native_selection_tools_input`, and the pen test with
`--native-test=native_selection_pen_input --tablet`. Review captures are in
`artifacts/selection-gtk/`.

Run `cargo test --locked -p layer-core -p layer-ui --lib` for shared behavior.
Hardware checks live in `layer_tests::selection_options`, the existing selection
and region tests, and `layer_tests::selected_brush_latency` (ignored, release,
serial). The brush probe covers no selection, legacy masks, and byte masks.
Keep machine-specific logs and screenshots in ignored `artifacts/`, not Git.

## Paintable coverage and saved masks

`selection_stroke.rs` shares the ordinary dab generator; Paint selection indexes
real centerline crossings, while grayscale mask editing uses the configured dry
brush. `painted_selections.rs` serializes completed contacts and GPU captures.
A submitted chunk stays immutable until acknowledged, including when final end
taper replaces the provisional footprint. Mode/target changes wait behind
completed contacts; unfinished contacts cancel. Retained real input and queued
contacts are bounded. Native estimated corrections/predictions do not modify
these masks; committed real samples determine coverage.

`selection_masks.rs` owns Quick Mask lifecycle, independent painting colors,
explicit saved-mask actions, reselect, and display settings. A temporary Layers
row uses the ordinary row/thumbnail projection with the reserved UI ID 0; that
ID never enters the document tree. Its preview revision tracks coverage, not
unrelated UI refreshes. Saved coverage uses independent
Selection Layers, shared history and project persistence. Loading copies coverage;
editing and replacing a stored layer are explicit operations. Returning to a
parked document and adopting a project reset temporary editing state. Filters
and destructive artwork actions are blocked during mask editing.

The GPU material and mask paths share `brush_footprint.wgsl`, contact geometry,
tip/grain/dual textures and accumulation semantics. Mask gradients and fills
write scalar coverage; connected fills classify artwork. Raw-alpha and stored
layer-mask loading share the region/refinement pipeline and preserve soft values.
Saved previews composite each visible layer’s color/opacity into a cached packed
RGBA integer texture, keeping the
presenter within the portable four-storage-buffer limit. Their grayscale 32px
thumbnails and tint are excluded from artwork sampling/export.

Quick Mask and Selection Layers share normal layer rows, coverage thumbnails,
a paintbrush target indicator, and a thumbnail-sized Load icon immediately to
the thumbnail’s right. Double-click/tap a renamable layer’s name to edit it.
The shared Layers toolbar stays stationary, disabling unsupported controls,
so selecting a mask does not move its name between double-clicks/taps.
Quick Mask has no separate editing strip or popup. Its temporary row remains
outside groups and cannot be renamed/reordered; exiting removes it.

`selection_properties.rs` publishes three controls for both mask kinds: Mode,
Overlay color, and Overlay opacity. Paint selection (default) paints selected
coverage with color, removes it with transparency/erasers, and overlays selected
areas. Grayscale mask uses black to protect, white to select, gray for partial
coverage, and transparency/erasers to select; its overlay shows protected areas.
Both blend with ordinary paint opacity. Overlay polarity derives from the mode,
with the mode stored in application settings and shared by all masks. Old layer
`painting` and `protected` fields are ignored. Colors remain unrestricted;
grayscale coverage is computed from display-encoded sRGB luminance while painting. The color
bucket copies the displayed mask painting color, independent of artwork colors.

Entry without a selection provides an empty working mask; leaving it untouched
preserves no selection and creates no history. Saved layers persist their
properties with ordinary single-step history. Saving exits Quick Mask, copies
its coverage/properties, and activates the saved layer. Activating a saved layer
shows it; leaving for another target automatically hides it without adding an
undo step or discarding redo.

Swept mask brushes flush every real input endpoint, rather than waiting for the
ordinary brush's distance simplifier. Masks have no disposable artwork tail
preview. Completed end-taper replay uses the same endpoint sampling so live and
finished geometry agree. Stamp-brush spacing is unchanged.

Select exposes Grow/Shrink directly; mask rows keep them under Modify. The
saved-selection menus stay disabled without saved layers, with no empty submenu
to open. Grow/Shrink use an integer
1–128 image-pixel distance. Apply queues one asynchronous GPU circular
maximum/minimum operation; Cancel leaves coverage untouched. Soft coverage is
preserved, values beyond the canvas are zero, and the result is one undo step.
Packed horizontal range extrema reduce circular refinement to O(radius) per
pixel, including soft masks. Pipelines compile only when their operation needs
them.
The dialog captures its target and revision; stale or locked destinations fail
without modifying artwork. Feathering an existing result, border, smooth, and
selection-only transforms remain separate work.

Layer menus group creation, organization, settings, and selection operations.
Overlay settings live in Properties. Loading is also available through menus
and Ctrl-thumbnail (Shift add, Alt subtract, Shift+Alt intersect). Web preflights
mask thumbnail pipelines asynchronously; an idle Layers drawer wakes compilation
and retries pending previews without blocking canvas input. Mask painting colors use bounded SDR values in HDR documents.

Additional reproducible checks:

- `cargo test --locked -p layer-ui painted_selection_checks` covers ordered
  captures, final replay/backpressure, independent colors, saved masks, locks,
  adoption, and artwork restrictions.
- `cargo test --locked -p layer-render-wgpu selection_paint -- --test-threads=1`
  covers scalar blending, coherent brush sweeps, gradients, source coverage,
  previews, thumbnails and export isolation (requires a GPU).
- Web `--selection-tools` also exercises Paint selection Add/Subtract controls,
  Quick Mask rows/properties, independent colors, compact Load icons, mouse/touch inline rename,
  Grow/Shrink dialogs, saved-mask edit/load and reselect. It also checks G-Pen
  coverage during sub-spacing moves, the color bucket, Quick Mask save/activation,
  and automatic hiding.
  Its light/dark screenshot assertions inspect the painted canvas area. Tested
  on Huion Kamvas Pad 12 / Chrome 143 / ARM Valhall with CDP mouse/touch/pen
  input. Injected input verifies the device rendering and host paths, not the
  physical pen sensor. Keep the tablet awake before starting the harness; it
  holds a screen wake lock during this suite and preserves recovery prompts
  using Keep for Later.
- Android `AndroidRasterTest#paintableSelectionsOnDevice` tests Paint selection
  GPU history, native stylus Quick Mask contacts, normal thumbnails, double-tap
  rename, Grow, independent colors, compact Load icons and artwork export isolation.
  Light/dark captures are written to the app’s external files directory. Run alongside
  `AndroidRasterTest#selectionToolsRenderAndCombineOnDevice` and
  `AndroidTitleBarTest#selectionDrawerToolsModesAndRememberedIcons`. Huion tests
  use the actual Vulkan device and Android input dispatcher with injected stylus
  events; they do not establish physical pressure/tilt feel.
- Run `native_quick_mask_input` through the native GTK harness above. It checks
  coverage and tint pixels, saves/edits/loads a mask, and captures both themes.
- `cargo test --locked -p layer-render-wgpu --release selection_paint_latency
  -- --ignored --nocapture --test-threads=1` measures a 4096² mask. On NVIDIA
  RTX PRO 6000 Blackwell Max-Q/Vulkan 610.57.04, eight real G-Pen samples per
  update measured 0.073/0.209 ms sample-and-submit p95 and 0.177/0.628 ms
  GPU-complete p95 for 64/800px brushes. Final capture took 11.5/5.2 ms.
  Interactive hosts compile pipelines asynchronously. These are workstation
  measurements, not tablet latency claims.
- `cargo test --locked -p layer-render-wgpu selection_resize_latency --lib --
  --ignored --nocapture --test-threads=1` measures a 2048² soft mask with no
  binary early-outs. On the workstation above, GPU completion plus capture took
  80 ms on first use and 28–30 ms for 32/128-pixel Grow/Shrink. These measure a
  completed operation, not brush latency; tablet timings depend on its GPU.

Layers and Color drawers close on an outside contact like other tool drawers;
a canvas contact that dismisses them does not paint.
Photo's secondary panel strip opens individual panels by default. Default
upgrades preserve customized workspace layouts.

The color wheel mirrors the foreground circle with a compact transparency
circle. Black and white shortcuts trail below and left of transparency. They
replace the selected foreground/background paint; from transparency they select
an independent paint, preserving both remembered colors. Wheel edits continue
on that independent paint until a remembered slot is selected. The extra paint
and its SDR/HDR picker coordinates survive workspace saves.

Regression checks for these controls include `device.test.mjs --color-panel`
and `--selection-tools` against Huion Chrome, and Android's
`neutralShortcutsPreserveRememberedColorsWithTouchPenAndMouse`,
`compactGeometryAndRastersInBothThemes`, and `paintableSelectionsOnDevice`.
The native GTK color-panel, HDR-picker, and Quick Mask input tests cover the
same shared model. Color-panel fixtures retain recoverable drawings and use
isolated workspaces. Compact hosts fit the full footer and leave the HDR
readout clear of both shortcuts. Web caches the fitted geometry across color
changes.

The transparency circle matches the secondary color's diameter, with its top
aligned to the primary color at the opposite edge. Equally sized, smaller black and white
circles overlap along the wheel's curve with the same edge clearance as
transparency. White stays above the opposite footer's bottom edge, allowing
about one pixel of clearance variation at compact widths. HDR
places its exposure readout below the circles so the compact overlap stays clear.
