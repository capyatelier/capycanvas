# Selection tools

GTK, Web, and Android provide Rectangle Select, Ellipse Select, Polygonal Lasso, and Select by Color.
Photo places these alongside Lasso and Auto select in its Tools toolbar. Sketch
uses a Select title-bar command and a two-panel Tools → Tool drawer, matching
the Eraser drawer. The Select command remembers the last chosen selection tool
in the workspace. Other hosts retain their existing selection entry points.

Preview-less tool rows match brush categories: at least 44 px on GTK/Web and
48 dp on Android.
Select, Brush, and Sculpt opener icons follow their category's remembered tool,
including while another category is active. Rust publishes the icon from shared
workspace memory; hosts update their retained header and toolbar images.
Selection modes use one compact row of icon toggles, with names and explanations
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

The defaults apply to GTK, Web, and Android. Shared migration recognizes only untouched
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

`selection_stroke.rs` shares the ordinary dab generator; Selection Brush indexes
real centerline crossings, while grayscale mask editing uses the configured dry
brush. `painted_selections.rs` serializes completed contacts and GPU captures.
A submitted chunk stays immutable until acknowledged, including when final end
taper replaces the provisional footprint. Mode/target changes wait behind
completed contacts; unfinished contacts cancel. Retained real input and queued
contacts are bounded. Native estimated corrections/predictions do not modify
these masks; committed real samples determine coverage.

`selection_masks.rs` owns Quick Mask lifecycle, independent grayscale colors,
explicit saved-mask actions, reselect, and display settings. A temporary Layers
row is host presentation, without a document ID. Saved coverage uses independent
Selection Layers, shared history and project persistence. Loading copies coverage;
editing and replacing a stored layer are explicit operations. Returning to a
parked document and adopting a project reset temporary editing state. Filters
and destructive artwork actions are blocked during mask editing.

The GPU material and mask paths share `brush_footprint.wgsl`, contact geometry,
tip/grain/dual textures and accumulation semantics. Mask gradients and fills
write scalar coverage; connected fills classify artwork. Raw-alpha and stored
layer-mask loading share the region/refinement pipeline and preserve soft values.
Saved previews use a cached GPU union and a packed integer texture, keeping the
presenter within the portable four-storage-buffer limit. Their grayscale 32px
thumbnails and tint are excluded from artwork sampling/export.

GTK and Web present 44px actions; Android uses 48dp. All three have a pinned
Quick Mask row, a persistent editing strip,
explicit Load buttons and Ctrl-thumbnail loading (Shift add, Alt subtract,
Shift+Alt intersect). Shared Select/Layer/View menus expose lifecycle, coverage
sources, saved destinations and display actions without keyboard modifiers.
The temporary row uses a compact actions menu so its title stays on one line.
Properties shows mask information instead of artwork opacity/blending controls.
Web preflights saved-mask thumbnail pipelines asynchronously; pending previews
retry without blocking canvas input. Mask color dialogs use bounded SDR values
even when the artwork document is HDR.

Additional reproducible checks:

- `cargo test --locked -p layer-ui painted_selection_checks` covers ordered
  captures, final replay/backpressure, independent colors, saved masks, locks,
  adoption, and artwork restrictions.
- `cargo test --locked -p layer-render-wgpu selection_paint -- --test-threads=1`
  covers scalar blending, coherent brush sweeps, gradients, source coverage,
  previews, thumbnails and export isolation (requires a GPU).
- Web `--selection-tools` also exercises Selection Brush Add/Subtract controls,
  Quick Mask, independent grayscale colors, saved-mask edit/load and reselect.
  Its light/dark screenshot assertions inspect the painted canvas area. Tested
  on Huion Kamvas Pad 12 / Chrome 143 / ARM Valhall with CDP mouse/touch/pen
  input. Injected input verifies the device rendering and host paths, not the
  physical pen sensor. Keep the tablet awake before starting the harness; it
  holds a screen wake lock during this suite and preserves recovery prompts
  using Keep for Later.
- Android `AndroidRasterTest#paintableSelectionsOnDevice` tests Selection Brush
  GPU history, native stylus Quick Mask contacts, mask menus, independent colors,
  the saved-layer Load button and artwork export isolation. Light/dark captures
  are written to the app’s external files directory. Run alongside
  `AndroidRasterTest#selectionToolsRenderAndCombineOnDevice` and
  `AndroidTitleBarTest#selectionDrawerToolsModesAndRememberedIcons`. Huion tests
  use the actual Vulkan device and Android input dispatcher with injected stylus
  events; they do not establish physical pressure/tilt feel.
- Run `native_quick_mask_input` through the native GTK harness above. It checks
  coverage and tint pixels, saves/edits/loads a mask, and captures both themes.
- `cargo test --locked -p layer-render-wgpu --release selection_paint_latency
  -- --ignored --nocapture --test-threads=1` measures a 4096² mask. On NVIDIA
  RTX PRO 6000 Blackwell Max-Q/Vulkan 610.57.04, the shared-footprint path measured
  0.027 ms submit p95, 0.090 ms GPU-complete p95 per eight contacts, and 32.482 ms
  final asynchronous capture. First-use pipeline/init completion was 74.695 ms;
  interactive hosts compile pipelines asynchronously. These are workstation
  measurements, not tablet latency claims.
