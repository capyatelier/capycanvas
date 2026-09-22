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
rectangle symbols follow the New/Add/Subtract/Intersect order used by GIMP and
Photoshop. Select by Color uses the wand motif with RGB sparkles.

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
