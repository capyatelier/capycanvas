# Phase 1 audit: canvas action bar in shared Rust

[Photo editing research](../photo-editing-research.md) · [Phase 1 plan](../../development/canvas-action-bar-transforms.md) · source report, 2026-09-26 · baseline `5eb45a47`

Read-only implementation audit made by an agent against baseline `5eb45a47` to prepare Phase 1. It covers how the bar's state reaches each host, its contents and context validation, context predicates, anchor and placement, chrome input, the commands to add, the existing placement bars, and the shared tests to extend. The [Phase 1 plan](../../development/canvas-action-bar-transforms.md) incorporates the findings and resolves the points where the audits differ. Line numbers can drift in later commits; verify before relying on one.

---

I made no edits. Paths are relative to the repo root: `ui/` means `crates/layer-ui/src/` and `host/` means `crates/layer-host/src/`.

## Key findings

- **Published command availability is frozen for the whole polygon construction, not just during a contact.** `require_idle` fails whenever `layer_interaction.path` is non-empty (ui/session.rs:4649-4662). `refresh_commands` keeps the previous `enabled` value whenever the canvas is not idle (ui/session.rs:4737-4754). So the published Complete and Cancel are probably disabled for the entire polygon today. I inferred this from the code; it needs a test to confirm. Either way, the bar must build its items from live `UiSession::command` (ui/session.rs:1893).
- **Extending `ToolbarContext` as the spec says would churn Tool Options.** Any change to the context bumps its generation (ui/session.rs:4728-4735). GTK then rebuilds every Tool Options editor and closes the slider preview (apps/layer-linux/src/toolbar_components.rs:545-605). A "selection present" or "polygon" field would do that on every selection edit. I recommend a separate `CanvasBarContext` that embeds `ToolbarContext`.
- **A camera-only change publishes only `{camera, revision}`.** That is snapshot.rs:97-109, acknowledged by `apply_change` only when the regions are exactly CAMERA (host/lib.rs:308-316). The bar is hidden while the camera moves, so I recommend Rust compute placement in a query at settle time rather than streaming the anchor per frame. This deviates from "rides in the camera patch"; the reasons are in the publication plan below.
- **Hosts already have menu rendering.** `ContextMenu`/`ContextMenuItem` (ui/customization.rs:544-610) has nested `sections` for submenus, `selected: Option<bool>`, `enabled`, `hint` and `bindings`. Hosts render it for application menus (host/snapshot.rs:232-259), `selection_menu` (host/lib.rs:699,842) and layer menus. It derives no `PartialEq`, so fetch bar menus lazily rather than embedding them in the view.
- **Selection anchors are cheap.** GPU-produced selections come back as `SelectionShape::Pixels`, with nonzero-coverage bounds computed by the GPU (layer-render-wgpu/src/selection_readback.rs:56-60). `Selection::bounds` reads those in O(1); contours cost O(points) (layer-core/src/selection.rs:196-198, 283-313). Cache bounds per selection identity. Treat an inverted selection as the whole canvas, which falls back to the bottom edge.
- **The toggle belongs in `DockLayout`, next to `canvas_info`.** `SavedDockLayout` gives every field `#[serde(default)]` and does not deny unknown fields (ui/layout_saved.rs:4-29), so no migration is needed and older builds ignore the field. `WorkspaceState` (ui/workspace.rs:11) and `Settings` (ui/settings.rs:98) are `deny_unknown_fields`.

## 1. State and publication (current)

- **`UiState`** is defined at ui/lib.rs:1239-1291. `commands` is documented as presentation only (1273-1275). `toolbar_context_generation` is published.
- **Regions** are at ui/lib.rs:1552-1564; `ALL` is 1023.
- **`changed()`** (ui/session.rs:4698-4727) runs `update_toolbar_context` (4703). It bumps `workspace_model_revision` for every region except CAMERA, COLOR_PREVIEW and COMMAND_SEARCH (4712-4715).
- **`frame()`** calls `refresh_commands` and `changed()` on every frame (ui/session.rs:3901-3906).

Per-host transports:

- **Native hosts (Apple, Android, Windows):** all go through `NativeHost::take_snapshot_with` (host/snapshot.rs:77-199). The branches are:
  - camera-only (97-109);
  - search fast path (111-136);
  - incremental `Motion` (137-163);
  - layout update (164-181);
  - full snapshot with `"state"` (245) and `application_menus` (251-259).
  - Android uses `take_layout_update_bytes` and `take_model_update_bytes` (apps/layer-android/native/src/android.rs:957-975; host/model_update.rs).
  - Apple uses `capy_apple_request` 3/5/7 for snapshots and 2 for `host.query` (apps/layer-apple/native/src/lib.rs:263-330).
  - Windows uses `capy_snapshot` and `capy_query` (apps/layer-windows/native/src/host.rs:986, 1044).
  - Android JNI queries go through `Java_art_capycanvas_Native_query` (android.rs:981).
- **Web:** `state_update()` destructures `UiState` exhaustively (apps/layer-web/src/lib.rs:802-894), so a new field will not compile until it is transported. `app.js` calls it only when the model revision changes (258-285); a camera-only change fetches `app.camera()` (286-289).
- **GTK** reads `UiState` directly.

**`ToolbarComponentView`** reaches hosts only inside tile `PanelView`s (`component: state.toolbar_component(tile.control)`, ui/customization.rs:1304). No host can get Tool Options without a tile.

**`options_layout`** goes through the stateless `toolbar_ui` (ui/toolbar_transport.rs:22-29, 86-100):
- Web: `app.toolbar_ui` (apps/layer-web/src/lib.rs:928-934)
- Apple: `capy_apple_toolbar_ui` (lib.rs:145)
- Windows: `capy_toolbar_ui` (shared_controls.rs:51)
- Android: android.rs:1165
- GTK calls `tool_options_layout` directly (apps/layer-linux/src/toolbar_components.rs:193).

Because `toolbar_ui` is stateless it cannot see the work area, so bar fitting and placement need a session query.

## 2. Contents (current)

- **`refresh_tools`** (ui/session.rs:4515-4645) builds `tool_actions` for:
  - transform: TransformAspect, Apply, Cancel, plus PlacementOriginalSize while placing (4517-4529);
  - rulers (4530-4550);
  - selection modes and options, where polygon adds ConstrainAngles, CompleteSelection and CancelSelection (4556-4572).
- **`tool_options()`** (ui/toolbar_components.rs:249-357) orders completion actions first (256-278), then choices, groups, `tool_extra`, fields and the remaining actions. It clones `CommandState` from the published `self.commands`, which is the frozen copy (263-270).
- **Types:** `ToolOption` (115-135), `ToolSettingAction`/`group()` (ui/tool_settings.rs:10-50), `ToolSetItem` (ui/tools.rs:331-339).
- **Stale-edit check:** `ToolbarContext` (ui/toolbar_components.rs:50-59; `operation` is derived from `tool_actions`, 202-219). `toolbar_edit_allowed` accepts Invoke only for commands in `tool_actions` (220-248). Dispatch rejects on context mismatch (ui/session.rs:2131-2136).

**Proposal for deriving bar items**
- Add `CommandId::canvas_bar_route() -> Option<CanvasBarRoute { kinds, priority, short_label, completion }>` next to `icon()`/`label()` (ui/lib.rs:749, 1030). This is the per-command route the spec asks for.
- For Transform, Placement and Polygon, bar commands must also be in `tool_actions`; add a test so Tool Options stays complete. Selection-bar commands (Deselect and the rest) are not in `tool_actions`, which is why the bar needs its own validation.
- **Menu-valued items:**
  - Refine ▾ uses the existing `selection_resize_items(None)` (ui/selection_masks.rs:233-254), which holds Grow… and Shrink… as `SelectionAction::BeginResize`.
  - More uses `selection_menu(SelectionMenu::Selection)`, which is the Select menu (ui/selection_masks.rs:226-231; ui/application_menu.rs:93-100), plus a bar section (Hide bar, Placement ▸).
  - For Transform and Polygon, More means Tool Options. Expose it through a query returning `toolbar_component(ToolbarControl::TOOL_OPTIONS)` (ui/toolbar_components.rs:186-201).

## 3. Context predicates

| Context / exclusion | Predicate |
|---|---|
| Placement | `operation.placing()` (ui/operation.rs:64-66); count = `Placement.members.len()` (operation/placement.rs:9) |
| Transform | `operation.active()` (operation.rs:61-63); handle drag = `Transaction.drag.is_some()` (operation.rs:51, private; needs an accessor) |
| Polygon | `layer_interaction.tool == Selection{Polygon} && !path.is_empty()` (ui/session.rs:2012-2013; ui/selection_tools.rs:477-503) |
| Completed selection | `doc.selection` (layer-core/src/lib.rs:1291); `LayersView.has_selection` (ui/session.rs:4949) |
| Selection tool / Move | `tool.selection_tool()` (ui/art_layers.rs:53-61); `LayerCanvasTool::Move` |
| Tonal / paint selection (bottom edge) | `tonal_active()` (ui/tonal_selection.rs:220); `selection_brush_active()` (ui/painted_selections.rs:235) |
| Painting tools (hide) | `tool.draws()` (ui/art_layers.rs:65-68) |
| Quick Mask / Selection Layer editing (exclude) | `selection_masks.quick()`/`target()` (ui/selection_masks.rs:204-210) |
| Artwork mask (exclude) | `doc.active_mask`; `ToolbarContext.mask` |
| Picker | `eyedropper.picking.previous.is_some()` (ui/color_picker_session.rs:43,69-93) or `tool.picks_color()` |
| Contact in progress | `interaction.pointer` (ui/session.rs:1156-1182); `touch.is_active()` (ui/camera.rs:176); `engine.has_active_stroke()`; `input_pending`; `painted_selections.has_contact()` |
| Camera gesture | pan = `interaction.pointer` with `!paint`, or `pan_key`; touch = `TouchGesture`; wheel and native pinch go through `scroll`/`gesture` (ui/session.rs:3551-3598; host/lib.rs:334-371) and have **no end signal**, so the host must debounce |
| Other exclusions | `state.settings_open`, `customization.is_open()`, `rendering_suspended`, `document_file.close_ready` |

- **Live availability:** `UiSession::command` (ui/session.rs:1893) and `command_flags` (1940-2120).
- **Existing disabled reasons:** `command_disabled_reason` (ui/command_catalog.rs:664-716).
- `require_document_idle` also fails while any transform is active (ui/document_files.rs:527-560), so selection items are disabled during transforms.

## 4. Anchor and placement (current)

- **Transform overlay** (ui/operation.rs:532-580): projects `camera ∘ basis ∘ pose` divided by dpi. The rotate handle sits at `reach × 2.5 × sign(scale_y)` (582-591), where `reach = 12 DIP·dpi/zoom` (ui/rulers.rs:66-71). Refactor it into `transform_handle_points() -> Vec<[f32;2]>`, shared by the overlay and the anchor (8 handles plus rotate), and add the 3.5 px marker plus a gap. That clears the handles in every orientation.
- **Layout:** `ResolvedLayout {work_area, status, groups, collapsed}` (ui/layout.rs:1365-1379); floating groups have `floating: true` (1338). With a native header the HUD strip lies inside `work_area` and is not subtracted (3461-3484), so subtract `status` explicitly.
- **`drawer_placement`** (ui/drawers.rs:778-791) uses tile measurements stored by `MeasureDrawerTiles` (ui/session.rs:2333-2340; drawers.rs:741-768). `Query::Drawer` sends host-measured heights (host/lib.rs:753, 936-962).
- **Other measurement routes:**
  - `UiAction::Measure*` (ui/lib.rs:1302-1346).
  - `ChromeFacts` fields `content_drawer`, `drawer_connection`, `expanded_panel`, `zen_button` (ui/interaction.rs:30-52).

## 5. Chrome input (current)

- **Chrome contacts:** hosts send `UiInput::Chrome{event: Contact{position, canvas}, facts}` (ui/interaction.rs:53-100).
- **Outside-contact dismissal** of drawers and columns runs only when `!facts.popup_open` (ui/session.rs:790-920).
- **Popup consumption:** a `canvas:true` contact records `(was_hidden, popup_open)` (933-935). Then `dismiss_popups = popup_open` and `handled |= popup_open` (1221-1226), which consumes the stroke.
- **Touch** reaches tools only on placement handles (1138-1147; operation.rs:510-515).
- **Blur** cancels transforms and placements (1184-1195).

**What the bar must do (host side):**
- Hit-test itself before the canvas. Report presses as `Contact{canvas:false}` and never through the pointer or pen path or `ColorPickerHold`.
- Never set `popup_open`, and never take focus.
- Send `CursorLeave` on entry so the brush cursor disappears (ui/session.rs:743-747).
- Keep Enter and Escape on the shortcut path: `ApplyTransform`=Enter, `CancelTransform`=Escape (ui/shortcuts.rs:362-363; ui/session.rs:1027-1039).
- **Zen:** hovering near a screen edge reveals docks (ui/session.rs:1266-1283). Add `ChromeFacts.canvas_bar: Option<Bounds>` so hovering the bar does not reveal them, like `zen_button`.

## 6. Commands

**Existing** (ui/lib.rs):
- Selection: Deselect 626, InvertSelection 627, QuickMask 577, SaveSelectionLayer 580, FillSelection 624.
- Transform: ScaleRotate 606 (label "Scale / rotate" at 1112 and operation.rs:108), ApplyTransform 607, CancelTransform 608, TransformAspect 609, PlacementOriginalSize 610.
- Polygon: CompleteSelection 600, CancelSelection 601.
- Grow/Shrink: `SelectionAction::BeginResize` (ui/selection_masks.rs:46, 831).
- Mask: `LayerAction::MaskSelection{id, hide:false}`, which becomes AddMask with `replace:true` (ui/art_layers.rs:167, 783-790, 1271-1305). It clears the selection and switches the tool to Paint.

**New:**
- `TransformFlipHorizontal`, `TransformFlipVertical`, `TransformRotateLeft`, `TransformRotateRight`, `ResetTransform`. These are pose edits through `update_transform` (operation.rs:331-358). Reset for a placement must restore the pose at session start, not identity (placement.rs:57-65). Also give `begin_transform` a serial (it uses `transaction: 0`, operation.rs:238).
- `RemoveSelectionPoint`: factor it out of `selection_key` (selection_tools.rs:492-497).
- `MaskSelection` (short label "Mask"): acts on the active layer.
- `ShowCanvasActionBar`: a toggle.

The existing view `FlipHorizontal/FlipVertical/RotateLeft/RotateRight` commands are camera commands (ui/session.rs:2064-2067) and cannot be reused.

**Adding a `CommandId` requires:**
- ui/lib.rs: the variant, `ALL` (867, a fixed-length array), `available_on` (659), `is_toggle` (730), `icon` (749), `label` (1030); icon tests (1601-1640) and the `ui_catalog().icons` list (325+).
- An SVG in `apps/layer-web/icons` (existing `reset`, `flip-*`, `rotate-*`, `mask` and `more` can be reused).
- `tool_choice` description (ui/customization.rs:978+; exhaustive match).
- `shortcut_id` (ui/shortcuts.rs:210+), checked by ui/command_catalog_tests.rs:23; `defaults` if needed (349-409).
- `command_flags` (ui/session.rs:1940) and `invoke` (3910).
- Catalog `history`/`aliases`/`action_description` (ui/command_catalog.rs:239-363) and `command_disabled_reason` (664).
- `command_without_renderer` if it must work with the renderer suspended (ui/renderer_lifecycle.rs:111-127).
- Menus: VIEW_MENU (ui/lib.rs:237-249) for the toggle.
- `workspace_before` list for the toggle (ui/session.rs:2255-2271); `workspace_description` (workspace_description.rs:89).
- Apple `command-coverage.json` and `audit-commands.py`.

**Toggle storage:** `DockLayout.canvas_bar: CanvasBarLayout{visible, placement}`, modelled on `canvas_info` (ui/layout.rs:1060; ui/header.rs:447-456; ui/layout_saved.rs:9,36). It defaults on in every built-in workspace and joins workspace undo history.

## 7. Existing placement bars (to delete)

What drives them today:
- Web, Android, Apple and Windows show the bar while the published `placement_original_size` command is enabled (`idle && placing()`, ui/session.rs:1998).
- GTK shows it while `tool_actions` contains PlacementOriginalSize (ui/session.rs:4524).

Host files:
- **GTK:** apps/layer-linux/src/tool_panels.rs:14-56; workspace.rs:913, 1113-1114, 1173, 1210, 2521; photo_drop_tests.rs:232.
- **Web:** apps/layer-web/image-import.js:6-8, 42-46; style.css:4-9, 902-910; image-placement.test.mjs:24-160; image-placement-device.test.mjs:18-48.
- **Android:** ImageImport.kt:205-218 (inside `ImagePlacementControls`, 193; it uses a `Popup` and holds the `BackHandler` at 206); Documents.kt:353; AndroidRasterTest.kt:1480, 2313.
- **Apple (macOS and iPadOS):** Shared/Editor/EditorView.swift:49-51, 171-195; tests/image-import-owner.swift:165, 181; native/src/photo_tests.rs:388; command-coverage.json:42.
- **Windows:** WorkspaceView.cpp:105, 136-146, 318, 389-391, 733-735.

## 8. Tests (existing, to extend)

- **Session tests:** module at ui/session.rs:4996, with `session()` using the Recorder backend (5172) and included files (5181-5189).
  - `send`/`click` (ui/selection_tests.rs:4-17) drive `s.pen()` followed by `s.frame(1,1)`.
  - Other helpers: `invoke` (ui/session.rs:14687), `event` (14690), `chrome` (9969).
- **Polygon:** ui/selection_tests.rs:124.
- **Placement:** ui/session_source_tests.rs:2, 192.
- **Toolbar:** ui/toolbar_component_tests.rs:2, 33, 122.
- **Catalog:** ui/command_catalog_tests.rs:6, 46.
- **Transform math:** operation.rs:700-800.
- **Drawer containment:** drawers.rs:795+.
- **Publication:** host/snapshot.rs:362-973.
- **Inspecting state:** use `s.state()` directly.

## Proposed data model

```rust
// ui/canvas_bar.rs (#[path] from session.rs)
pub enum CanvasBarKind { Placement, Transform, PolygonSelection, Selection }
pub struct CanvasBarContext {                 // Copy, Eq, Serialize, Deserialize
    pub generation: u64,
    pub toolbar: ToolbarContext,
    pub kind: CanvasBarKind,
    pub transaction: u64,                     // operation serial / selection identity serial
    pub selection: bool, pub placing: bool, pub quick_mask: bool,
    pub selection_layer: Option<u64>, pub ruler: Option<u64>,
    pub polygon: bool, pub picker: bool,
}
pub enum CanvasBarItem {
    Option { option: ToolOption, short_label: &'static str, disabled_reason: Option<String> },
    Menu { menu: CanvasBarMenu, label: &'static str, icon: &'static str, enabled: bool },
}
pub enum CanvasBarMenu { Refine, More }
pub enum CanvasBarMore { SelectionActions, ToolOptions }
pub enum CanvasBarPlacementMode { NearObject, BottomEdge }
pub struct CanvasBarView {
    pub context: CanvasBarContext,
    pub label: Option<String>,               // "2 images"
    pub items: Vec<CanvasBarItem>,           // priority order, may overflow
    pub completion: Vec<CanvasBarItem>,      // trailing, never overflow
    pub more: CanvasBarMore,
    pub mode: CanvasBarPlacementMode,        // resolved (tonal/paint/polygon/inverted → BottomEdge)
    pub anchor_revision: u64,
}
// UiState { pub canvas_bar: Option<CanvasBarView>, .. }
pub struct CanvasBarMeasure { pub context: CanvasBarContext, pub items: Vec<[f32;2]>,
    pub completion: Vec<[f32;2]>, pub more: [f32;2], pub gap: f32, pub padding: [f32;2] }
pub struct CanvasBarLayout { pub bounds: Bounds, pub items: Vec<Option<Bounds>>,
    pub more: Bounds, pub completion: Vec<Bounds>, pub side: CanvasBarSide /*Below|Above|BottomEdge*/ }
pub fn canvas_bar_place(layout: &ResolvedLayout, anchor: Option<Bounds>, size: [f32;2],
    mode: CanvasBarPlacementMode) -> (Bounds, CanvasBarSide);   // pure, tested
impl UiSession { pub fn canvas_bar_layout(&self, viewport: [f32;2], m: &CanvasBarMeasure)
    -> Option<CanvasBarLayout>; pub fn canvas_bar_menu(&self, m: CanvasBarMenu) -> Result<ContextMenu,String>; }
// UiAction::CanvasBarEdit { context: CanvasBarContext, action: Box<UiAction> }
```

**Fitting.** Reserve the completion widths first. Then call `tool_options_layout(avail − completion, row_h, Horizontal, …)` for the priority prefix (it always reserves More) and pack the result compactly.

**Placement.**
- The area is `work_area` minus `status`. Avoid floating groups, open collapsed columns and the drawer.
- Default is below the anchor, then above. Fall back to the bottom edge when:
  - the anchor is off-screen;
  - the anchor covers more than about 60% of the area;
  - the mode is BottomEdge;
  - the work area is narrow (the phone rule; there is no phone concept in shared Rust).

## Invalidation and publication plan

1. **Computing the view.** Call `update_canvas_bar()` in `changed()` right after `update_toolbar_context` (ui/session.rs:4703).
   - Derive the kind using the precedence Placement > Transform > Polygon > exclusions > Selection.
   - Selection shows when a selection tool or Move is active, or when an "armed" flag is set. Set the flag when the selection's identity changes (Arc pointer plus affine plus inverted) and clear it when the tool changes (ui/session.rs:3246).
   - While the toggle is off, show only completion contexts, with completion items only, at the bottom edge.
2. **Rebuild cost and freezing.** Rebuild only when a cheap key changes: the kind, context, anchor revision and per-item `command_flags` bits. Build items with `self.command(id)`. Freeze the view while a real contact is active (pointer, touch, stroke, pending input, paint-selection contact). Do not freeze merely because the polygon path is non-empty. This mirrors ui/session.rs:4744-4754.
3. **Region.** If the view changes, OR in a new `regions::CANVAS_BAR = 1024` (`ALL = 2047`). It counts as a model change, so there is no new fast path. That is acceptable because bar changes are rare once contacts are frozen.
4. **No suppression flag.** Rust does not publish a per-contact hidden flag. Hosts hide the bar during routed canvas contacts and camera patches, and reshow after a shared `CANVAS_BAR_REAPPEAR_MS` debounce published in `UiCatalog` (ui/lib.rs:317+). This avoids publishing at every pen-down; native timing stays in hosts as AGENTS.md requires.
5. **Placement query.** Hosts query placement when the view or `anchor_revision` changes, when their measured size changes, on LAYOUT changes, and once the camera settles. Transports:
   - GTK: direct call.
   - Web: `app.canvas_bar_layout` and `app.canvas_bar_menu`.
   - Native hosts: `Query::CanvasBarLayout` and `Query::CanvasBarMenu` in `NativeHost::query` (host/lib.rs:649).
   - Stale contexts return null.
6. **Camera patch** stays unchanged at snapshot.rs:97-109. If live tracking is wanted later, store the measured size through a `MeasureCanvasBar` action and add a computed `canvas_bar` placement to the camera-only and `Motion` packets.

## File-by-file shared-Rust changes

- **ui/lib.rs:** new CommandIds and their tables, `canvas_bar_route`, `UiState.canvas_bar`, `regions::CANVAS_BAR`, `UiAction::CanvasBarEdit`, VIEW_MENU entry, catalog constant, icon test groups.
- **ui/canvas_bar.rs (new):** types, context derivation, item builder, fitter, `canvas_bar_place`, menus, tests.
- **ui/session.rs:**
  - `changed()` hook; the CanvasBarEdit unwrap and validation next to 2131, before the deferral and placement guards;
  - `command_flags` and `invoke` for the new commands;
  - the `workspace_before` entry for the toggle;
  - `tool_actions` additions (4517-4572);
  - the armed-selection flag;
  - Zen hover exemption using `ChromeFacts.canvas_bar`.
- **ui/operation.rs and operation/placement.rs:** `dragging()`, `placement_count()`, `transform_handle_points()` (refactoring the overlay), pose commands, initial pose, `begin_transform` serial.
- **ui/selection_tools.rs:** `pop_polygon_point()`.
- **ui/selection_masks.rs:** selection-bounds cache; the Refine menu reuses `selection_resize_items`.
- **ui/art_layers.rs:** `MaskSelection` command maps to `LayerAction::MaskSelection{active, hide:false}`.
- **ui/layout.rs and layout_saved.rs:** `DockLayout.canvas_bar` with `CanvasBarLayout`.
- **ui/customization.rs:** a placement-choice customization action; `tool_choice` descriptions.
- **ui/workspace_description.rs** (toggle description), **ui/command_catalog.rs**, **ui/shortcuts.rs**, **ui/interaction.rs** (`ChromeFacts.canvas_bar`, `#[serde(default)]`).
- **ui/toolbar_components.rs:** optional; leave `ToolbarContext` unchanged.
- **host/lib.rs:** the two queries.
- **host/snapshot.rs:** only tests; `canvas_bar` travels inside `"state"`.
- **apps/layer-web/src/lib.rs:** `state_update` field and wasm functions (a host file, but it is the Rust transport).
- **layer-ffi:** no changes. It is the low-level canvas C ABI and does not use `UiState` (crates/layer-ffi/Cargo.toml).

## Risks

1. **Glass (the user's decision).**
   - Glass regions re-register one frame late on every host (docs/ui/panel-transparency.md:40-66). The first frame may show the translucent fill without blur, so register the region before making the bar visible.
   - Hiding during camera motion and contacts means the glass never tracks a moving surface.
   - The fill must be exactly `palette.glass.panel`, or GTK's node walk will not detect it.
   - The research doc's "Surface: opaque panel fill" line (BAR-0) and the transparency doc need updating.
2. **Polygon freeze.** The published-availability freeze covers the whole polygon, not just contacts (see key findings). Hosts must use the bar's live states.
3. **Churn from extending `ToolbarContext`** (see key findings), if the spec's literal wording is followed.
4. **Android's current bar** is a `Popup` (ImageImport.kt:209). The replacement must be in-tree and keep `BackHandler`.
5. **Paint-selection capture** defers bar actions (ui/painted_selections.rs:296-321). Stale validation happens before the deferral.
6. **Undo arming the selection bar.** Undo that restores a selection would arm it under painting tools. Decide whether that is acceptable.
7. **Label change.** Relabelling ScaleRotate breaks label-based tests: apps/layer-apple/Shared/Tests/SelectionTransformChecks.swift and docs/ui/icon-audit.md.
8. **Pen latency.** Rust publishes nothing per contact, but host-side hide must not trigger layout passes on Android or Windows. Measure before enabling by default.

## Commit-sized steps

1. **Shared bar core.** Placement and Transform contexts using existing commands only. Includes `CanvasBarView`/`CanvasBarContext`/items, `regions::CANVAS_BAR`, `CanvasBarEdit`, the live-availability freeze, `canvas_bar_place` plus the fitter plus the session query, the transform handle-points refactor, and the Web and NativeHost transports. Tests: context derivation, stale edits, freeze, below/flipped/clamped/fallback/handle clearance/rotated views, HUD with and without a native header.
2. **Hosts, one commit each.** The glass bar component on each of the six hosts, porting the Placement context and deleting the old bar and its tests in the same change (COMMIT_GUIDE "replace obsolete paths").
3. **Toggle.** Show Canvas Action Bar: `DockLayout.canvas_bar`, View menu, workspace history, completion-only mode while off, More section (Hide bar, Placement ▸), `ChromeFacts.canvas_bar`.
4. **Transform session commands.** Flip H/V, Rotate 90° left/right, Reset (placement-aware), and the ScaleRotate relabel. They appear in Tool Options and on the bar.
5. **Polygon context.** Add `RemoveSelectionPoint`; Complete, Remove Last Point and Cancel at the bottom edge; More opens Tool Options.
6. **Selection context.**
   - Add the `MaskSelection` command, the Refine menu, and More as Selection Actions plus the bar section.
   - Anchor on cached bounds that honor `inverted`.
   - Tonal and paint selection go to the bottom edge; hide under painting tools; add the armed-by-command rule.
   - Host menu rendering, including the GTK Selection Actions button (T-20).
7. **Docs.** A new docs/ui bar guide; docs/ui/selection-command-inventory.md §2 and §7; the panel-transparency doc; the research record's BAR-0 surface line.
