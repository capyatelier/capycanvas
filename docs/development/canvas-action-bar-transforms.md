# Phase 1: canvas action bar and transforms

[Developer guide](README.md) · [Photo editing research](../history/photo-editing-research.md) · [Drag convention](../ui/drag-and-reorder.md) · [Panel transparency](../ui/panel-transparency.md)

Status: **in progress.** Done and pushed: A1–A3, B1–B3, C1 (GTK), D1–D6, D8, E1, E2, F1 (Web) and finger-touch handles for every transform. D7 is done in the renderer; its Interpolation choice is next. The Web journey still has to run in Chrome on the MovinkPad 11. The current behavior is documented in [the canvas action bar guide](../ui/canvas-action-bar.md).

Two refinements made during implementation:
- **More opens a menu,** not the Tool Options drawer: the items that did not fit (mode choices as submenus), then the context's own menu and the bar toggle. Bar items are commands and choices that menus can represent, so no new drawer anchor was needed on any host.
- **Flips and quarter turns act in the layer's axes** about the centre of the transformed box. These are the document axes unless the layer itself is rotated.

The original plan follows. Written 2026-09-26 against `origin/main` at `5eb45a47`; citations re-checked at `b7a73e07`. Verify the cited lines before relying on them; later commits move code.

Phase 1 delivers two things together:
- **The canvas action bar:** a floating glass bar beside the selection, transform box or placed image, which offers the next steps and mode switches.
- **One transform session:** a single transform with switchable Free, Uniform, Distort and Warp modes.

The product specification is the "Canvas action bar (BAR)" section and the XF items of the [research record](../history/photo-editing-research.md). This document turns them into ordered, testable steps. Its evidence is in source reports [15](../history/photo-editing-research/15-phase1-bar-shared-rust.md) (shared Rust), [16](../history/photo-editing-research/16-phase1-bar-hosts.md) (hosts) and [17](../history/photo-editing-research/17-phase1-transforms.md) (transforms). Where those audits disagree, this plan records the choice.

Follow [AGENTS.md](../../AGENTS.md) and the [commit guide](../COMMIT_GUIDE.md) throughout. Do not add agent `Co-Authored-By` trailers. Put shared rules, validation and history in Rust; keep native timing, capture and widgets in hosts. Each host deletes its old placement bar in the same change that adds the new bar.

## Scope

**Delivered**
- **The bar on GTK, Web, Android, macOS, iPadOS and Windows:**
  - It is a glass surface in the panel layer, visible in Zen.
  - It replaces the six host image-placement bars.
  - A View toggle, **Show Canvas Action Bar**, is stored in the workspace layout.
- **Four bar contexts:**
  - **Placement** (import and paste);
  - **Transform** (layer content, selected pixels, masks);
  - **Polygon construction:** Complete, Remove Last Point, Cancel;
  - **Selection**, with existing commands only: Deselect, Invert, Transform, Refine ▾ (Grow, Shrink), Mask, Fill, Quick Mask, Save as Selection Layer, and More (the shared Selection Actions menu).
- **One transform session:**
  - Modes: Free, Uniform, Distort (with a Perspective option) and Warp (a mesh with node and tangent handles).
  - Session commands: Flip Horizontal, Flip Vertical, Rotate 90° left and right, Reset.
  - Skew handles, and a Skew numeric field.
  - An Interpolation choice, with bicubic sampling and supersampling when minifying.
  - Finger-touch handles for every transform.
- **Transforms survive window focus loss** (T-24). Only the contact in progress is cancelled.
- **A GTK "Selection Actions…" button** (T-20).

**Not in Phase 1**
- Crop (BAR-4), mask and Quick Mask bar modes (BAR-5), guides, samplers and clone source (BAR-6), and job progress (BAR-7).
- New selection features: Copy to Layer, Clear, Feather, Adjust and Copy. They land on the bar in Phase 2.
- Lossless Distort and Warp of placed photos (P-10).
- Multi-layer and group transforms, pivot, snapping, nudging, Transform Again and split lines.
- Moving, pinning or customizing the bar.

**Journeys at exit**, keyboard-free on every host:
- **Place or paste an image:** Free, Uniform, Flip or Rotate 90° on the bar beside the photo, then Apply.
- **Warp or distort part of an image:** Select, then Transform on the selection bar, then Warp or Distort, drag, and Apply. This opens journey 27 for paint layers and selected pixels.
- **Distort a placed photo:** Select All, then Transform, then Warp or Distort. Until P-10, the bar offers this route in the disabled reason.
- **Finish a polygon:** Complete, Remove Last Point or Cancel on the bar.
- **Act on a selection:** Deselect, Invert, Mask, Fill, Grow or Shrink, from the bar beside it.

## Decisions

**Resolved**

| Decision | Resolution | Source |
| --- | --- | --- |
| Name | "Canvas action bar". "Command bar" is command search. | Research BAR-0 decision 7 |
| Surface | A glass surface in the panel layer, registered for blur like panels and drawers, and visible in Zen like floating panels. Menus opened from it stay opaque. | User, 2026-09-26 |
| Scope | The bar is the canvas route for selection, transform and placement flows. Menus, Tool Options and command search stay complete. | User, 2026-09-26 |
| Toggle storage | `DockLayout.canvas_bar`, beside `canvas_info` (`crates/layer-ui/src/layout.rs:1060`). `SavedDockLayout` gives fields `#[serde(default)]` and accepts unknown fields (`crates/layer-ui/src/layout_saved.rs:9`), so no migration is needed. It is on in every built-in workspace and joins workspace history. | Report 15 |
| Movability | Not movable in Phase 1. | Research BAR-0 decision 6 |
| Tool Options | Keeps the complete form, including completion actions and mode choices. The bar reuses its row builders. | Research section 4.2 |

**Confirmed by the user on 2026-09-26**, as recommended:

| # | Decision | Resolution | Needed by |
| --- | --- | --- | --- |
| 1 | Default placement | Near Object. Use Bottom Edge when the work area is narrow (under 600 logical px), because shared Rust has no phone concept. | B2 |
| 2 | When the selection bar shows | With selection tools and Move. Also after a selection-creating command (Select All, Load Selection, Select Layer Opacity, Reselect), until the next tool change. Not after Undo or Redo. Hidden under painting tools. | E2 |
| 3 | Leaving Warp for Free or Distort | Keep the mesh and show the outer handles on its hull, as Photoshop and Procreate do. | D12 |
| 4 | Distort and Warp on placed photos | Disabled, with the reason "Select All, then Transform, to distort this photo's pixels". Later, re-base a single placement into a pixel transaction that keeps the source (report 17, option c). | D8 |
| 5 | Warp grid presets | Count cells, as Photoshop does. The default 3×3 cells equals Procreate's 4×4 points. | D12 |
| 6 | Flip and Rotate 90° axes | Document axes, about the centre of the transformed hull. | D1 |
| 7 | Reset | Return to the geometry at session start, for placements too. | D1 |
| 8 | Label | Rename "Scale / rotate" to "Transform", keeping the persisted `ScaleRotate` ID. | D1 |

## The bar: design

### Shared model

Add a new `crates/layer-ui/src/canvas_bar.rs`, included from `session.rs` like the other session modules. The core of the model:

```rust
pub enum CanvasBarKind { Placement, Transform, Polygon, Selection }
pub enum CanvasBarPlacement { NearObject, BottomEdge }
pub struct CanvasBarContext {
    pub generation: u64,
    pub toolbar: ToolbarContext,
    pub kind: CanvasBarKind,
    pub transaction: u64,
}
pub enum CanvasBarItem {
    Option { option: ToolOption, short_label: &'static str },
    Menu { menu: CanvasBarMenu, label: &'static str, icon: &'static str, enabled: bool, reason: Option<String> },
}
pub enum CanvasBarMenu { Refine, More }
pub struct CanvasBarView {
    pub context: CanvasBarContext,
    pub label: Option<String>,
    pub items: Vec<CanvasBarItem>,
    pub completion: Vec<CanvasBarItem>,
    pub placement: CanvasBarPlacement,
}
```

- **A separate context token.** `CanvasBarContext` embeds `ToolbarContext` rather than extending it.
  - Every change to `ToolbarContext` bumps its generation (`crates/layer-ui/src/session.rs:4728`). GTK then rebuilds every Tool Options editor and closes the slider preview.
  - Selection edits would do that constantly.
- **Context precedence:** Placement > Transform > Polygon > exclusions > Selection.
  - The exclusions are:
    - Quick Mask, Selection Layer editing and artwork-mask editing;
    - the colour picker;
    - an open Settings or Customize;
    - a suspended renderer.
  - The predicates are listed in report 15, section 3.
- **Item routes.** Add `CommandId::canvas_bar_route() -> Option<CanvasBarRoute>` beside `icon()` and `label()` in `crates/layer-ui/src/lib.rs`. A route holds the kinds, the priority, a short label and a completion flag.
  - Transform, Placement and Polygon bar commands must also be in `tool_actions`; a test enforces this so Tool Options stays complete.
- **Live availability.**
  - Build items from `UiSession::command` (`session.rs:1893`), not from the published `CommandState`. `refresh_commands` keeps the previous `enabled` value whenever the canvas is not idle (`session.rs:4737`).
  - `require_idle` fails while a polygon path exists (`session.rs:4649`), so Tool Options may show Complete and Cancel as disabled for the whole construction today. Step E1 confirms this with a test.
- **Rebuild and freeze.**
  - Rebuild the view only when a cheap key changes: kind, context, anchor revision, or the item flags.
  - Freeze it while a contact is active.
  - Publish changes through a new `regions::CANVAS_BAR`.
- **Validation.** `UiAction::CanvasBarEdit { context, action }` rejects stale edits before the deferral and placement guards, like `ToolbarEdit` (`session.rs:2131`).
- **Menus.** Use the existing `ContextMenu` model (`crates/layer-ui/src/customization.rs:544`), fetched lazily by query because it has no `PartialEq`:
  - Refine is `selection_resize_items` (`crates/layer-ui/src/selection_masks.rs:233`).
  - For selections, More is `selection_menu(SelectionMenu::Selection)` plus a bar section (Hide Bar).
  - For Transform and Polygon, More opens the complete Tool Options drawer, anchored to the bar through a new `DrawerAnchor` variant (`crates/layer-ui/src/drawers.rs:30`).

### Fitting and placement

- **Fitter.**
  - Reserve the completion items first; they never overflow.
  - Then fit the priority prefix with `tool_options_layout` (`crates/layer-ui/src/toolbar_components.rs:367`), with More placed before the completion items.
  - The bar never scrolls. `tool_options_layout` alone stops at the first field that does not fit and pins More at the end, so the bar needs this wrapper.
- **Placement.** A pure `canvas_bar_place(layout, anchor, size, placement) -> (Bounds, Side)`, tested without a host.
  - **Area:** the work area minus the HUD strip (with a native header the strip lies inside the work area), avoiding floating groups, open collapsed columns and drawers.
  - **Near Object:**
    - below the anchor, then above it;
    - clamped to the area;
    - clear of every handle point.
  - **Falls back to the bottom edge when:**
    - the anchor is off-screen;
    - the anchor covers more than about 60% of the area;
    - the context uses the bottom edge (Polygon, tonal and paint selection, inverted selections);
    - the work area is narrow (decision 1).
  - **Stable across Zen:** place against the non-Zen layout, so the bar does not jump when chrome reveals.
- **Handle clearance.** Refactor `append_transform_overlay` (`crates/layer-ui/src/operation.rs:532`) into `transform_handle_points()`, shared by the overlay and the bar.
  - The rotate handle sits 2.5 × reach beyond the box's top edge, which ends up below the box after a vertical flip (`operation.rs:581`).
  - Mesh nodes join these points in D12.
- **Anchors:**
  - **Transform and placement:** the hull of the transformed box.
  - **Selection:** bounds cached per selection identity. GPU-made `Pixels` selections carry their nonzero bounds (O(1)). Contours are measured once.
  - **Inverted selection:** treated as the whole canvas, so the bar goes to the bottom edge.
- **Publication.** The view travels in `UiState`. Hosts query the placement when:
  - the view or its anchor revision changes;
  - their measured size changes;
  - the layout changes;
  - the camera settles.

  Stale contexts return null. The queries per host:
  - GTK calls the function directly.
  - Web: `app.canvas_bar_layout` and `app.canvas_bar_menu`.
  - Native hosts: `Query::CanvasBarLayout` and `Query::CanvasBarMenu` in `NativeHost::query` (`crates/layer-host/src/lib.rs:649`).

  The camera-only snapshot patch stays as it is, because the bar is hidden while the camera moves.

### Hiding, input and focus

- **Hiding.**
  - `InputReply` gains `canvas_bar_hidden`, next to `chrome_hidden` (`crates/layer-ui/src/interaction.rs`). Rust sets it from:
    - the pointer, touch and stroke state;
    - a transform handle drag;
    - pan and pinch gestures.
  - Hosts hide the bar at once. They show it again after `CANVAS_BAR_REAPPEAR_MS`, a shared constant in `UiCatalog`, once the flag clears and no wheel or pinch event has arrived. Wheel and native pinch have no end signal, so this host-timed debounce covers them.
  - Hide the bar during workspace float drags too, so a dragged panel never crosses it.
- **Chrome contacts.**
  - Taps on the bar are chrome contacts (`ChromeEvent::Contact { canvas: false }`). They never reach the pen, pointer or colour-picker-hold paths.
  - The bar never sets `popup_open`: an open popup makes the next canvas contact dismiss it instead of drawing (`session.rs:933, 1222`).
  - Menus opened from the bar do set it, like every explicit menu.
- **`ChromeFacts.canvas_bar` (the bar's bounds):**
  - Zen hover over the bar does not reveal the docks.
  - A tap on More toggles its drawer instead of closing it as an outside contact.
- **Focus.**
  - The bar never takes window focus. Window blur cancels transforms until step A1 changes that, and focus changes Enter and Escape.
  - Enter and Escape keep their shortcut meaning (Apply and Cancel).
  - Keyboard focus reaches the bar only through a focus command (a later step); inside, it is one Tab stop with arrow keys.
- **Glass cost.**
  - Today any change to the region list repaints every old and new glass bound (`crates/layer-render-wgpu/src/backdrop_blur.rs:510`). Hiding the bar at pen-down would repaint all panel glass in the stroke's first frame.
  - Repaint only the symmetric difference, and measure the first-frame cost.
  - Register the glass region before showing the bar, so no frame shows the fill without blur.

### Hosts

The same z-order on every host: docked groups, dividers and collapsed columns < floating groups < **bar** < drawers < header < menus and popovers. Report 16 has the full file lists.

| Host | Element and z-order | Glass and Zen | Rows, menus, focus | Delete | New tests |
| --- | --- | --- | --- | --- | --- |
| GTK | `Slot::CanvasBar` in `DockSurface` (`apps/layer-linux/src/workspace.rs:541`), allocated in the second pass, kept by `clear_docks`, raised before drawers | `dock-panel` class, so `glass::collect` finds it; exempt from Zen fading by slot. Update the two tests that assume every non-canvas child hides | Extract `OptionRows` from `Component::new` in `toolbar_components.rs`, with a name prefix. More is a `PopoverMenu` via `populate_workspace_menu` and `watch_popover`. `focus_on_click(false)` on every control | `tool_panels.rs` `PlacementActions` and its registrations | `native_canvas_bar_input`; migrate `photo_drop_tests.rs` and `photo_workflow_tests.rs`; `native_backdrop_blur_capture` with the bar |
| Web | `section.canvas-action-bar`, `role=toolbar`, in `#workspace`, z-index 900 | Add to the `glass.js` selector list; `glass.queue()` on show and hide; no `.chrome` class | Field factory extracted from `createToolbarComponent`. More via the manual popover in `customization.js`, with a flip-above. `aria-disabled`, `mousedown` `preventDefault` except on inputs | `image-import.js` controls and their CSS | `canvas-bar.test.mjs`; port `image-placement*.test.mjs` |
| Android | `CanvasBar.kt`, placed after the groups loop in `Workspace.kt`, zIndex 198, outside the `hidden` gates | `Modifier.glass`; remove from composition when hidden | Extract `ToolOptionField` and sizing. Make the choice, number and bar menus non-focusable (split `focusable` from `preserveContact` in `WorkspaceMenus.kt`), because a focusable popup blurs the window. Make `popupOpen` an owner count. Wrap in a `Surface` | The `Popup` in `ImageImport.kt`, keeping Back to cancel | `AndroidInteractionTest` bar journeys with finger, mouse and stylus; port `AndroidRasterTest` uses |
| Apple | Inside the `WorkspacePanels` `ZStack` (`apps/layer-apple/Shared/Editor/WorkspacePanels.swift`), zIndex 180. Not the root `ZStack`, where it would cover drawers and the contact menu | `glassSurface` with `palette.glassPanel` and `GlassRegistration`; remove the view when hidden; never read `chrome_hidden` | Make `ToolOptionField` internal; add `ToolOptionsRow`. Menus via the `EditorMenuButton` pattern, which sets `popup_open`, not `editorPopover`. No `FocusState`, `.focusEffectDisabled()`, no sheets or windows, because resigning key status sends blur | `PhotoPlacementControls` in `EditorView.swift` | `CanvasActionBarChecks.swift` in both launch-test targets; port `DocumentControlChecks.swift`; `native/src/canvas_bar_tests.rs` |
| Windows | `CanvasActionBar.h/.cpp`, a `Border` in the workspace `Canvas`, ZIndex 180 | Squircle path with the glass panel brush and `WorkspaceShadow`; add to `WorkspaceView::glass()`. **Fix** the early return for `chrome_hidden` (`apps/layer-windows/WorkspaceView.cpp:771`), which drops all panel glass in Zen today | Extract `ToolOptionsRow` from `ToolbarComponent`. More via a `NativeMenus.h` helper `ShowAt`. `AllowFocusOnInteraction(false)`, `IsTabStop(false)`. A transient `measure_canvas_bar` action | `placementBar` and `placeControls` in `WorkspaceView.cpp` | `exercise-canvas-bar.ps1` (mouse, touch, pen), with light and dark themes |

## Transforms: design

### Session geometry

The session holds one geometry, and switching modes keeps what the user has built. The types, in `crates/layer-core/src/affine.rs` (with a new `warp.rs`) and `crates/layer-ui/src/operation.rs`:

```rust
pub struct Projective(pub [f32; 9]);
pub struct MeshMap { pub frame: Affine, pub cells: [u16; 2], pub net: Arc<[Point]> }
pub enum TransformMap { Affine(Affine), Projective(Projective), Mesh(Arc<MeshMap>) }
pub struct ImageTransform { pub map: TransformMap, pub interpolation: Interpolation }

struct Pose { offset: Point, scale: [f32; 2], angle: f32, shear: f32 }
enum TransformMode { Free, Uniform, Distort { perspective: bool }, Warp { cells: [u16; 2] } }
enum Geometry { Pose(Pose), Quad([Point; 4]), Mesh { mesh: Arc<MeshMap>, outer: Projective } }
enum Handle { Move, Scale([f32; 2]), Rotate, Skew(usize), Corner(usize), Edge(usize), Node(u32), Tangent { node: u32, side: u8 } }
struct Drag { handle: Handle, press: Point, current: Point, start: Geometry }
```

**Pose**
- `Pose` gains `shear`, composed as translate·rotate·shear·scale about the pivot.
- `Pose::from_affine` decomposes exactly. It replaces the refusal of skewed placements (`crates/layer-ui/src/operation/placement.rs:67`).

**Converting between modes**
- Free/Uniform → Distort takes the four transformed corners exactly.
- Distort → Warp seeds the mesh from the quad's projective map.
- Leaving Warp keeps the mesh and edits an outer projective (decision 3).

**Other operations**
- Flip and Rotate 90° apply `Geometry::post(D)`, where D is a document-axis affine about the hull centre, conjugated by the layer basis. This is exact for every kind.
- Reset restores the session-start geometry.

**Modes on screen**
- Modes appear in a new segmented `ToolActionGroup::TransformMode` (`crates/layer-ui/src/tool_settings.rs:17`), on the bar and in Tool Options. Uniform is the existing `TransformAspect`.
- A mode appears only when its implementation ships. No disabled placeholders.

**Commands**
- New IDs: `TransformFlipHorizontal`, `TransformFlipVertical`, `TransformRotateLeft`, `TransformRotateRight`, `ResetTransform`, `TransformFree`, `TransformDistort`, `TransformWarp`, `TransformPerspective`.
- The existing `FlipHorizontal`, `FlipVertical`, `RotateLeft` and `RotateRight` are view commands and cannot be reused.
- Existing icons cover flip, rotate and reset.

**Handles and input**
- Hit-testing order: mesh node and tangent, corner, edge, rotate, box handle, then inside the hull.
- Every handle drags immediately with every device. Generalize `placement_touch_hit` (`crates/layer-ui/src/operation.rs:510`) to every transaction, so fingers reach handles.
- Ctrl/Cmd is tracked live, like Shift and Alt (`crates/layer-ui/src/session.rs:945`). Ctrl-drag of an edge skews, and of a corner distorts. Both are also reachable through the modes.

**Overlay**
- Draw the mesh as tessellated dashed segments. Tangents are solid stems with small filled handles.
- `CursorSegment` has no circle marker (`crates/layer-render/src/lib.rs:35`). Nodes use the filled square unless a round marker is added.

### Preview and commit

**No format step**
- `ImageTransform` and `LayerOperationKind` are never saved: every holder is `#[serde(skip)]` (`crates/layer-core/src/lib.rs:194`, `crates/layer-core/src/layers.rs:434`). Changing them needs no `.capy` or recovery format step.
- `ImageTransform` loses `Copy`, which touches about 31 sites.

**Projective**
- The `pixel_transform` uniform grows to a 3×3 inverse with a w-divide, rejecting w ≤ 0. The 48-byte size is hard-coded in four places in `pixel_transform.rs`.
- `region_jobs` (`crates/layer-render-wgpu/src/paint_transform/snapshot.rs:137`) must subdivide or skip regions across the horizon.
- Require a convex quad.

**Mesh**
- **Pass A:** a new pre-pass rasterizes the tessellated mesh into an `Rg32Float` map of source positions for each job region, with a sentinel for uncovered pixels and a skirt of at least 1 px.
- **Pass B:** the existing fullscreen composite reads the map instead of `M·world`. The colour, scalar and visibility variants stay one shader, and no pass samples and writes the same subresource.
- **Folds:** last writer wins.
- **Job footprints:** CPU triangle binning per destination page.
- **Pipeline:** the new pipeline joins on-demand shader preparation.

**Commit**
- Commit stays one `Edit::Batch` through `append_operations`, keeping the preview result when `matching_commit` matches (`crates/layer-render-wgpu/src/paint_transform.rs:680`).
- It still resamples every plane the pass moves today: colour, material and watercolour wetness, and the mask.

**Selection after commit**
- **Contours:** mapped on the CPU. Projective mapping is exact vertex by vertex; mesh mapping clips, densifies to about 1 px, then maps.
- **`Pixels` selections:** go through the GPU `selection_resample` compute (`crates/layer-render-wgpu/src/selection_clip.rs:76`) and a readback. Apply becomes two-phase (request, then commit on the result) and still makes one history edit.

### Interpolation

- **Bicubic:** Catmull-Rom, 16 taps.
  - Clamp overshoot to the range of the nearest four taps, alpha to [0, 1] and scalars to [0, 1]. `R32Float` stores do not clamp.
- **Supersampling:** Jacobian-adaptive, up to 4×4 bilinear taps, when a region is minified. The Jacobian is analytic for projective maps and comes from the UV-map derivatives for meshes.
- **Support sites:** widen all five together:
  - `crates/layer-core/src/affine.rs:29`
  - `snapshot.rs:184`
  - `pixel_transform.wgsl:35`
  - `material_sources.rs:155`
  - `scene.wgsl:54`
- **Fast path:** add one for tap blocks inside a single source view, because `original()` loops over up to 16 views per tap.
- **Defaults:** Distort and Warp default to Bicubic. Interpolation is a choice in Tool Options and on the transform bar.

### Placed photos and focus

- **Lossless operations:** Free, Uniform, Skew, Flip, Rotate 90° and Reset stay lossless through `placement`.
- **Distort and Warp:** disabled on placements and batches, with a reason (decision 4). With a selection, a photo layer already takes the destructive path, which is tested (`crates/layer-render-wgpu/src/paint_transform_tests.rs:1203`).
- **Focus loss (T-24):**
  - A new `cancel_layer_contact()` rolls back only an active drag. `UiInput::Blur` and `PenPhase::Cancel` use it (`crates/layer-ui/src/session.rs:1184`, `crates/layer-ui/src/operation.rs:501`).
  - Escape, tool switches, pen errors and `reconcile_transform` still cancel the whole session.
  - `suspend_renderer` must cancel explicitly (`crates/layer-ui/src/renderer_lifecycle.rs:158`; its test at `:524` asserts the cancel).

## Steps

Shared Rust and GTK first, then Web and Android, then Apple and Windows ([Apple porting guide](../APPLE_PORTING_GUIDE.md), [Windows porting guide](../WINDOWS_PORTING_GUIDE.md)). Each step is one commit or a small series. Each keeps every suite green and adds its tests in the same change.

**A. Foundations**

| Step | Change | Tests |
| --- | --- | --- |
| A1 | Transforms and placements survive blur and `PenPhase::Cancel`; the drag rolls back; `suspend_renderer` cancels explicitly (T-24) | Session tests for blur during a drag and between drags; the updated renderer-lifecycle test; a GTK focus-loss check |
| A2 | `transform_handle_points()`, `dragging()`, `placement_count()`, and a serial for `begin_transform` (it sends `transaction: 0`) | `operation.rs` tests; the overlay is unchanged |
| A3 | Glass repaints only the symmetric difference of region lists | A `backdrop_blur` unit test; `native_backdrop_blur_capture` timing before and after |

**B. The bar in shared Rust**, with the placement and transform contexts using existing commands

| Step | Change | Tests |
| --- | --- | --- |
| B1 | `canvas_bar.rs` model: context derivation and freeze, `regions::CANVAS_BAR`, `CanvasBarEdit`, `canvas_bar_route`, live availability, `InputReply.canvas_bar_hidden`, `ChromeFacts.canvas_bar`, bar `DrawerAnchor` | Context derivation for every state in report 11; stale edits; freeze during contacts; a test that bar commands are in `tool_actions` |
| B2 | Fitter, `canvas_bar_place`, the layout and menu queries (GTK direct, Wasm, `NativeHost::query`) | Below, above, clamped, fallback, 60% coverage, narrow area, handle clearance at every rotation and after a flip, HUD with and without a native header, floating groups and drawers avoided |
| B3 | `ShowCanvasActionBar`, `DockLayout.canvas_bar`, View menu, workspace history; completion-only at the bottom edge while off; Hide Bar in More | Serialization round trip with older layouts; toggle undo; completion items still shown while off |

**C. GTK host**

| Step | Change | Tests |
| --- | --- | --- |
| C1 | `OptionRows` extraction; the glass bar slot; menus; chrome contacts; reappear debounce; delete `PlacementActions` | `native_canvas_bar_input` with mouse, touch and pen: a tap on the bar never paints; a canvas contact never dismisses it; a transform survives a bar tap and an open More menu; the bar hides during strokes, handle drags and camera gestures and returns once; glass region count rises and falls; Zen; narrow width; light and dark at Off and High. Migrated photo drop and workflow tests |

**D. Transforms** (shared Rust with GTK checks). Each step adds its bar and Tool Options items in the same change.

| Step | Change | Tests |
| --- | --- | --- |
| D1 | Flip H/V, Rotate 90° left/right, Reset (decisions 6–8); relabel to "Transform" | Identities (flip twice, rotate four times); lossless placements; one undo step; label-based tests on Apple and in `docs/ui/icon-audit.md` updated; GTK `native_operation_tool` extended |
| D2 | Shear: `Pose.shear`, `from_affine`, Skew handles with live Ctrl, Skew field; skewed placements accepted | Decomposition round trips; handle anchors; a skewed-placement session test; the handle and setting counts in `session.rs` tests updated |
| D3 | `ToolActionGroup::TransformMode` with Free and Uniform | The segmented choice on the bar and in Tool Options |
| D4 | `TransformMap` refactor, Affine only, no behaviour change | All suites green |
| D5 | Projective in `layer-core`: map, inverse, bounds, validation, conjugation, contour mapping | CPU tests, including affine-as-projective equivalence |
| D6 | Projective in the renderer: uniform, shader, `region_jobs` | GPU oracle tests against a CPU projective oracle; seamless regions; the ignored latency tests (`pixel_transform_tests.rs:652`, `paint_transform_tests.rs:979`) within 8.33 ms p99 |
| D7 | Interpolation: bicubic, clamps, the five support sites, adaptive supersampling, fast path, Interpolation choice | Oracle tests for each filter and for minification; overshoot clamps at all four depths; latency |
| D8 | Distort mode: corner and edge handles, Perspective, conversions, overlay, contour selection mapping; disabled on placements with the reason (decision 4) | Mode round trips; convexity refusal; GTK scenario Select → Transform → Distort → Apply |
| D9 | `Pixels` selections through GPU resample and readback; two-phase Apply | Soft and inverted selections; one undo step; cancel during the readback |
| D10 | `MeshMap` in `layer-core`: tessellation and seeding | An affine seed is exact; a quad seed is within tolerance of the projective map |
| D11 | Mesh render pass: UV-map pre-pass and triangle binning | Oracle tests against the CPU tessellation; folds; latency |
| D12 | Warp mode: grid presets (decision 5), node and tangent handles, outer hull handles (decision 3), finger-touch handles for every transaction | Handle hit tests; mode switching that keeps geometry; GTK scenarios Select → Transform → Warp → Apply, with mouse, touch and pen |

**E. Polygon and selection contexts** (shared Rust and GTK)

| Step | Change | Tests |
| --- | --- | --- |
| E1 | `RemoveSelectionPoint` command; polygon bar at the bottom edge; fix the published availability of Complete and Cancel during construction if E1's test confirms it is frozen | `selection_tests.rs` polygon tests extended; keyboard-free completion |
| E2 | Selection context: `MaskSelection` command, Refine menu, More with the bar section, cached bounds that honour `inverted`, arming rules (decision 2), tonal and paint selection at the bottom edge; GTK "Selection Actions…" button (T-20) | Arming and disarming; hidden under painting tools; tonal never shows Apply; Mask makes one undo step; GTK journeys |

**F. Other hosts.** Each host ports C1, the new transform items and the E contexts in one series, deleting its placement bar.

| Step | Host | Notes |
| --- | --- | --- |
| F1 | Web | Headless Chrome journeys in `test.mjs` for pen, touch and mouse; the `set_glass` spy; `workspace-motion.sh web` for real input; `device.test.mjs` on Android Chrome |
| F2 | Android | Non-focusable menus first, so no menu opened from the bar blurs the window. Stylus, finger and mouse `MotionEvent` journeys; screenshots in light and dark |
| F3 | Apple (macOS, iPadOS) | XCUITests on macOS and a physical iPad; Pencil through the UIKit fixtures in `apps/layer-apple/tests`; `command-coverage.json` |
| F4 | Windows | Fix the Zen glass return first. UI Automation with `RowPointerDriver.cs` for mouse, touch and pen |

**G. Qualification and documentation**

| Step | Change |
| --- | --- |
| G1 | Measure pen latency with the bar shown and at pen-down hide: the Android front-buffer benchmark, `tools/performance/web-pen.mjs`, the Windows pen-latency workload (Independent Flip is unproven there), Apple `CAPY_WORKLOAD`, GTK frame pacing (`apps/layer-linux/src/tests.rs:11005`). Record the results before the bar ships on by default. |
| G2 | Write `docs/ui/canvas-action-bar.md` as the current guide. Update `docs/ui/selection-command-inventory.md` §2 and §7, `docs/ui/panel-transparency.md` (the bar is a glass surface), `docs/ui/toolbar-components.md` (shared rows), `docs/ui/icon-audit.md` and the Apple `README.md`. Mark Phase 1 done in the research record's sequencing. |

## Exit criteria

This run implements Phase 1 on GTK, Web and Android (steps A–E, F1, F2 and G). Apple and Windows follow the same plan later (F3, F4).

- **Journeys:** the exit journeys in Scope pass on all six hosts with mouse, touch and pen where the host supports them.
- **Old bars gone:** the six placement bars and their host-specific positioning are deleted.
- **Menus stay complete:** every bar item is reachable from a menu or Tool Options, and command search finds every new command.
- **One undo step:** every bar action and every transform Apply makes exactly one step, including two-phase Apply.
- **Glass:** it follows the transparency setting in light and dark. The region count returns to its previous value when the bar hides.
- **Latency:** within the 8.33 ms p99 budget in the transform latency tests. Pen latency is measured on every host with no regression beyond noise.
- **120 fps:** every bar and transform interaction holds 120 fps (8.33 ms per frame) on a reasonably large photo and selection: a 24-megapixel photo placement, and a full-canvas selection of a 6000 × 4000 paint layer. Measure it on GTK with the Wacom Cintiq Pro 27, and on the Wacom MovinkPad 14 and Huion Kamvas Pad 12 over adb. Interactions:
  - handle drags in every mode;
  - mode switches;
  - Flip and Rotate 90°;
  - Apply;
  - showing and hiding the bar.

## Risks

- **Pen latency from an overlay.** It is unmeasured on Windows Independent Flip and on the Android shared buffer. G1 measures it before the bar is on by default.
- **Glass churn.** Hiding at pen-down changes the region list. A3 limits the repaint, and G1 measures it.
- **Deep perspective.** It raises job counts toward the 65,536 cap (`crates/layer-render-wgpu/src/paint_transform/snapshot.rs:208`). Mitigations: require convex quads and supersample minified regions.
- **Wider kernels.** Combined with the 16-view loop they threaten the frame budget; D7 adds the single-view fast path and measures.
- **Two-phase Apply for `Pixels` selections.** A readback must complete before commit, and cancelling during it must leave history untouched.
- **Focus on each host:**
  - Android menus are focusable today.
  - GTK and Windows controls take focus on click.
  - Web buttons move focus on mousedown.
  - Apple popovers and sheets resign key status.

  Each host step removes these before the bar ships.
- **Label-based tests.** Relabelling `ScaleRotate` breaks them on Apple and in docs.
