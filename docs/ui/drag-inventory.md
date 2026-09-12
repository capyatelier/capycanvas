# Drag source inventory

[Required convention](drag-and-reorder.md) · [Workspace and UI](README.md)

Source audit on 2026-09-12, starting with GTK, against main `879293f`.
This inventories the production gesture registration and pickup paths, including
reused drawer presentations. It does not claim new runtime or physical-device
validation. No interaction implementation changed in this audit.

**Change** means source code contradicts the required pickup rule. **Keep** means
the visible registration/recognition path already has the required timing;
native input still needs regression testing when surrounding code changes.
**Gap** means a presentation has no relevant drag handler. **Verify** means native
toolkit behavior is implicit and cannot be established from source alone.

## GTK first

| ID | Draggable surface / presentation | Current path and behavior | Required work |
| --- | --- | --- | --- |
| G1 | Toolbar/ribbon tile bodies: commands, tools, panel openers, parameter tiles, divider tiles; docked, floating, wrapped, vertical, and tabbed toolbars | [`workspace_customization.rs`](../../apps/layer-linux/src/workspace_customization.rs) registers `tile_root` through `install_panel_drag`. [`workspace.rs`](../../apps/layer-linux/src/workspace.rs) creates an unrestricted `GtkDragSource` for mouse/pen. Touch enters `workspace_drag_input` and starts after movement slop, without checking that a hold completed. | **Change for mouse, touch, and pen.** Gate both native DND and captured touch paths behind the same hold eligibility. Keep disabled command wrappers customizable, short-click activation, insertion hints, and cross-toolbar moves. |
| G2 | Toolbar tiles rendered inside column drawers, including nested tool drawers | [`workspace_drawer.rs`](../../apps/layer-linux/src/workspace_drawer.rs), `ToolbarBody::refresh`, creates `customization::tile_button` controls but does not register tile drag/context handlers as the main toolbar does. | **Gap**, separate from G1's timing bug. If these mirrored toolbar tiles are made reorderable, use the same hold gate and source identity; do not add an immediate fallback. Their drawer tabs already have a separate drag path. |
| G3 | Partial-Zen toolbar tiles | [`workspace_zen.rs`](../../apps/layer-linux/src/workspace_zen.rs) creates `tile_button` controls without tile drag registration. | **Gap / current presentation restriction**, not an immediate-pickup violation. The convention does not itself enable rearranging Zen projections. Any future support must require a hold. |
| G4 | Collapsed-column icon tiles | [`workspace_columns.rs`](../../apps/layer-linux/src/workspace_columns.rs) installs click/context actions on icons; only the footer is registered as a drag source. | **Gap relative to Web's draggable icons.** No timing change exists to make until icon dragging is enabled. When enabled, apply the tile rule, even though the moved object is a panel. Preserve strip scrolling. |
| G5 | Individual docked/floating panel tabs; active and inactive tabs, including toolbar tabs | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs), `install_panel_drag(DockItem::Panel)`; [`workspace_tab_drag.rs`](../../apps/layer-linux/src/workspace_tab_drag.rs) captures stable tab slots. The stable controller begins after native movement slop. | **Keep immediate pickup for all devices.** Preserve tab sliding, tear-off, grab offsets, and cancellation. |
| G6 | Panel/group title and tab-bar background, group grips, lone-panel footer strips | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs) registers headers/footers as `DockItem::Group`; the common captured controller has no hold requirement. | **Keep immediate pickup for all devices.** Do not accidentally apply G1's delay to a containing group. |
| G7 | Standalone toolbar grab handles | [`workspace_customization.rs`](../../apps/layer-linux/src/workspace_customization.rs) registers the grip as `DockItem::Panel`. | **Keep immediate pickup.** This is a handle despite carrying the same panel payload as some icon tiles. |
| G8 | Collapsed-column footer grab handles | [`workspace_columns.rs`](../../apps/layer-linux/src/workspace_columns.rs) registers `DockItem::Column`. | **Keep immediate pickup for mouse, touch, and pen.** |
| G9 | Column-drawer header backgrounds and individual drawer tabs | [`workspace_drawer.rs`](../../apps/layer-linux/src/workspace_drawer.rs) registers the header as a group and each tab as a panel through `install_panel_drag`. | **Keep immediate pickup**, including nested drawer/tab tear-off and docking. Do not confuse these tabs with G2's tool tiles. |
| G10 | Layer-list row bodies, including names, whitespace, content/mask thumbnails, and child controls; every retained Layers panel instance | [`layers.rs`](../../apps/layer-linux/src/layers.rs), `row_drag`, rejects an unheld row drag only for `InputSource::Touchscreen`. Mouse **and pen** can start native DND immediately. The grouped long-press recognizer allows touch holds to continue as a drag. | **Change pen row-body pickup to require a hold.** Keep immediate mouse dragging and existing touch hold/scroll arbitration. Preserve mask-specific menus, row child clicks, active renaming, and single document-history drops. |
| G11 | Layer-row trailing grip | The same [`row_drag`](../../apps/layer-linux/src/layers.rs) helper is installed with its direct/touch-enabled override on the grip. | **Keep immediate pickup for all devices.** Exempt the grip from the new pen row-body guard. |
| G12 | Dock/column split dividers and floating-window edge/corner resize handles | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs), `register_drag` with `Divider` / `Resize`; any movement starts resizing. | **Keep.** These are direct resize controls, not reordered tiles or list rows. |
| G13 | Native application window title/header drag region | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs) uses `AdwHeaderBar` in the native window. | **Keep native immediate window movement.** Do not intercept it with the tile hold recognizer. |

The G1 fix needs two entry points, not just `GestureLongPress`: non-touch tile
contacts currently bypass `workspace_drag_input` and go straight to GTK DND.
`install_context` also sets `touch_only(true)`, and `show_context` preserves a
held workspace contact only when it has a touch sequence. These paths need
device-aware hold ownership for mouse and pen tiles. They must not change the
immediate G5–G9 sources. G10 has a much narrower device-classification gap.

### Other GTK pointer drags and non-draggable collections

| Surface | Production source | Convention impact |
| --- | --- | --- |
| Canvas drawing, Hand/space/middle/right-button panning, touch camera gestures, selections, operation/transform/ruler/figure/gradient tools | [`input.rs`](../../apps/layer-linux/src/input.rs), shared [`interaction.rs`](../../crates/layer-ui/src/interaction.rs) and engine tool input | Direct artwork/navigation manipulation. No reorder hold should be added. |
| Navigator overview and camera outline | [`navigator.rs`](../../apps/layer-linux/src/navigator.rs), `GestureDrag` | Keep immediate navigation. |
| Color wheel/field | [`tool_panels.rs`](../../apps/layer-linux/src/tool_panels.rs), `GestureDrag` | Keep immediate color picking. |
| Effect curve points | [`effects.rs`](../../apps/layer-linux/src/effects.rs), `CurveEditor` | Keep immediate point editing. This is not list-item reordering. |
| Numeric sliders, opacity/size sliders, scrollbars | [`number_control.rs`](../../apps/layer-linux/src/number_control.rs), native `GtkScale` / `GtkScrolledWindow` | Preserve native value changes and scrolling. |
| Gradient-stop editor | [`effects.rs`](../../apps/layer-linux/src/effects.rs) | Current GTK bar selects/adds stops by click; a numeric position control edits their location. No separate bar-drag reorder source was found. |
| Brush/tool-set choices, filter browser entries, picker/configuration rows, workspace/toolbar library entries, history rows, document title | [`tool_panels.rs`](../../apps/layer-linux/src/tool_panels.rs), [`effects.rs`](../../apps/layer-linux/src/effects.rs), [`workspace_customization.rs`](../../apps/layer-linux/src/workspace_customization.rs), [`workspace_manager_dialog.rs`](../../apps/layer-linux/src/workspace_manager_dialog.rs), [`workspace_history_dialog.rs`](../../apps/layer-linux/src/workspace_history_dialog.rs) | No independent reorder source was found. Choice buttons are not automatically toolbar tiles, and library ordering is not automatically a drag feature. |

Artwork group and adjustment/filter-layer rows use the Layers list/drop model;
their placement follows the layer-row policy, not toolbar tile policy. Internal
GTK `DropTarget` receivers are destinations, not additional pickup surfaces.

## Web

| Surface | Current source behavior | Required work |
| --- | --- | --- |
| Toolbar tiles, including dividers and retained drawer/Zen presentations where drag is enabled | [`app.js`](../../apps/layer-web/app.js), `draggable`: mouse uses unrestricted HTML `draggable=true`; touch/pen capture on down and start after 8 px. [`customization.js`](../../apps/layer-web/customization.js) and [`workspace-chrome.js`](../../apps/layer-web/workspace-chrome.js) reuse it. | **Change all three devices** to require a hold. Neither native `dragstart` nor the pointer fallback may bypass the gate. The general workspace context timer currently handles touch, with an additional all-device exception only for layer rows. |
| Collapsed-column icon tiles | [`workspace-chrome.js`](../../apps/layer-web/workspace-chrome.js), `arrange`, calls `draggable(b, {kind:"panel"})`; the stable workspace controller consequently treats an icon as an immediate tab/panel drag. | **Change all three devices.** Tag the visible source as a tile separately from its panel payload. Review `.dock-tab { touch-action:none }` inherited by `.column-tab` so pre-hold contacts preserve collapsed-list scrolling. |
| Docked/floating/drawer tabs and title strips; toolbar, group, panel-footer and collapsed-column grips | [`app.js`](../../apps/layer-web/app.js) stable workspace pointer controller; [`workspace-chrome.js`](../../apps/layer-web/workspace-chrome.js) | **Keep immediate pickup** after movement slop. |
| Layer-row bodies | [`layers.js`](../../apps/layer-web/layers.js), `waitForHold`, is true only for touch away from `.layer-grip`. | **Change pen body pickup** to require a hold and preserve scrolling. Keep immediate mouse pickup. Existing hold/menu continuation can be reused. |
| Layer-row grips | [`layers.js`](../../apps/layer-web/layers.js), grip exemption; [`style.css`](../../apps/layer-web/style.css), `.layer-grip { touch-action:none }` | **Keep immediate pickup** for all devices. |
| Column/floating resize strips, scroll thumbs, numeric range controls, color wheel, Navigator, effect curves, canvas/tool gestures | [`app.js`](../../apps/layer-web/app.js), [`editor-panels.js`](../../apps/layer-web/editor-panels.js), [`effects.js`](../../apps/layer-web/effects.js) | Direct manipulation; no reorder delay. Browser-owned file/text DND is separate. |

## Android

| Surface | Current source behavior | Required work |
| --- | --- | --- |
| Toolbar tiles, including toolbar bodies reused in drawers | [`Panels.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Panels.kt), `dragSource`; [`WorkspaceInput.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceInput.kt), `workspaceGestures`, tracks a hold but starts on `touchSlop` even when `held` is false. | **Change mouse, touch, and pen** pickup eligibility. Context-menu timing already exists; it is not currently a mandatory tile gate. |
| Individual tabs, group/title bars, toolbar/group/footer/column grips, drawer tabs/headers | [`Workspace.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Workspace.kt), [`WorkspaceChrome.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceChrome.kt), common `workspaceGestures` | **Keep immediate pickup**. Split the tile gate from these sources. |
| Layer-row bodies | [`Layers.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Layers.kt), `directDrag = down.type != PointerType.Touch || ...grip area...` | **Change pen body pickup**: only mouse or an explicit grip should use the direct path. Keep pre-hold scrolling and same-contact menu continuation. |
| Layer-row trailing grip area | [`Layers.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Layers.kt), trailing 20 dp hit region | **Keep immediate pickup** for all devices; verify the visual handle and hit region agree. |
| Collapsed-column icon bodies; divider tiles | [`WorkspaceChrome.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceChrome.kt) installs context/click on icons, without `dragSource`. [`Panels.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Panels.kt) returns early for divider tiles before registering a source. | **Availability gaps**, not hold-gating violations. Any added reordering must use the tile policy. |
| Resize strips, numeric sliders, color wheel, Navigator, curves/gradient controls, canvas/tool gestures | `Workspace.kt`, [`NumberControl.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/NumberControl.kt), [`ColorPanel.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/ColorPanel.kt), [`Navigator.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Navigator.kt), [`Effects.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Effects.kt) | Direct manipulation; no reorder delay. |

## Apple: macOS and iPadOS

| Surface | Current source behavior | Required work |
| --- | --- | --- |
| Toolbar tiles, including drawer toolbars where gestures are enabled | [`WorkspacePanels.swift`](../../apps/layer-apple/Shared/Editor/WorkspacePanels.swift) registers tile payloads; [`WorkspacePresentation.swift`](../../apps/layer-apple/Shared/Bridge/WorkspacePresentation.swift), `WorkspaceRootDrag`, starts every source via `DragGesture(minimumDistance: 6)`. No device or hold gate appears there. | **Change mouse, touch, and pen** tile pickup. Source category and actual input device must be retained; the current uniform SwiftUI drag is insufficient. |
| Workspace tabs, header backgrounds, toolbar/panel/group/column grips | [`WorkspacePanelHeader.swift`](../../apps/layer-apple/Shared/Editor/WorkspacePanelHeader.swift), [`WorkspacePanels.swift`](../../apps/layer-apple/Shared/Editor/WorkspacePanels.swift), [`WorkspaceDrawers.swift`](../../apps/layer-apple/Shared/Editor/WorkspaceDrawers.swift), common root drag | **Keep immediate pickup** after movement slop. |
| Layer-row bodies | [`LayerPanel.swift`](../../apps/layer-apple/Shared/Editor/LayerPanel.swift) attaches the reorder `DragGesture` only to the grip; body/name/thumbnail targets select or open context actions. | **Gap:** add immediate mouse body dragging and hold-then-drag continuation for touch/pen bodies if aligning whole-row reordering with GTK/Web/Android. A context menu alone is not a body drag path. |
| Layer-row grip | `LayerPanel.swift`, `DragGesture(minimumDistance: 6)` | **Keep immediate pickup**; do not add the body hold to it. |
| Collapsed-column icon bodies / Partial-Zen toolbar projections | `WorkspaceDrawers.swift` gives icons click/context behavior without a workspace drag source; Zen projections disable workspace gestures. | **Current non-draggable presentations**, not timing violations. Any new icon/tile reordering must hold first. |
| Native window movement, resize strips, color wheel, curve/gradient controls, numeric sliders, Navigator, artwork/tool gestures | Native AppKit/UIKit input, `WorkspacePanels.swift`, [`PropertyControls.swift`](../../apps/layer-apple/Shared/Editor/PropertyControls.swift), [`NumberControl.swift`](../../apps/layer-apple/Shared/Editor/NumberControl.swift), [`NavigatorPanel.swift`](../../apps/layer-apple/Shared/Editor/NavigatorPanel.swift) | Direct manipulation; no reorder delay. |

## Windows

| Surface | Current source behavior | Required work |
| --- | --- | --- |
| Toolbar tiles, including divider and drawer tile instances | [`PanelBody.cpp`](../../apps/layer-windows/PanelBody.cpp) registers `tile_drag`; [`WorkspaceGestures.cpp`](../../apps/layer-windows/WorkspaceGestures.cpp) starts after 6 px mouse/pen or 12 px touch slop, with no hold gate. It also claims scrolling at pointer down. | **Change all three devices** to require a hold and defer scroll interception where relevant. Existing context handling clears the pending pointer/action, so it also needs same-contact continuation for held tiles. |
| Panel/drawer tabs and title strips; toolbar/group/footer/column grips | [`WorkspaceView.cpp`](../../apps/layer-windows/WorkspaceView.cpp), [`WorkspaceDrawers.cpp`](../../apps/layer-windows/WorkspaceDrawers.cpp), [`CollapsedColumns.cpp`](../../apps/layer-windows/CollapsedColumns.cpp), common `WorkspaceGestures` | **Keep immediate pickup**. |
| Layer name/content/mask and grip sources | [`LayerRow.cpp`](../../apps/layer-windows/LayerRow.cpp), `dragSource`, uses `CanDrag(true)` / `DragStarting` on these four child surfaces, with no explicit device/hold distinction. | **Verify native timing**, then encode the row-versus-grip rule explicitly. Source alone cannot prove that WinUI's default touch/pen pickup matches either required path. Whole-row whitespace/control continuation is also a **gap** compared with GTK/Web/Android. |
| Collapsed-column icon bodies | `CollapsedColumns.cpp` registers an empty drag action plus a context target. | **Not draggable**; do not confuse its context registration with immediate reordering. |
| Native window title movement, resize handles, sliders, scrollbars, color/curve/gradient controls, Navigator, canvas/tool input | [`WorkspaceGestures.cpp`](../../apps/layer-windows/WorkspaceGestures.cpp), [`UiControls.h`](../../apps/layer-windows/UiControls.h), [`ColorView.cpp`](../../apps/layer-windows/ColorView.cpp), [`CurveView.cpp`](../../apps/layer-windows/CurveView.cpp), [`GradientView.cpp`](../../apps/layer-windows/GradientView.cpp), [`NavigatorView.cpp`](../../apps/layer-windows/NavigatorView.cpp), [`CanvasWindow.cpp`](../../apps/layer-windows/CanvasWindow.cpp) | Direct manipulation; no reorder delay. |

Across the non-GTK hosts, brush/filter choice grids, library/manager entries,
configuration rows, and history lists were also checked for independent reorder
registration. No additional reorder family was found. Their click actions,
native scrolling, or external file/text drop receivers do not make them drag
sources under this convention.

## Shared boundary and implementation order

[`DockItem`](../../crates/layer-ui/src/layout.rs) identifies the moved item, not
the visual source or device. [`UiSession::drag_workspace`](../../crates/layer-ui/src/session.rs)
accepts item, phase, position, viewport, and tab geometry, with no hold/device
input. [`workspace_update.rs`](../../crates/layer-ui/src/workspace_update.rs)
publishes motion/layout/content revisions after the gesture is admitted. Layer
drops likewise receive validated edit actions after native pickup. These contracts
must remain the movement/drop/history path; changing a shared distance threshold
cannot implement this convention.

1. GTK: fix G1's native DND and touch gate together, then G10's pen classification.
   Preserve direct handles/tabs and explicitly track G2–G4 as availability work.
2. Apply the same tile and pen-row policy to Web and Android. Web additionally
   needs a visual-source distinction for collapsed-column icon tiles.
3. Split Apple's uniform root drag by source/device and address row-body pickup;
   make Windows tile gating explicit and validate native row/grip DND timing.
4. Preserve the already-correct direct-manipulation and handle paths on every
   host. Keep the intentional hold delay out of steady-motion performance probes.

## Tests that need coverage changes

- GTK: extend [`native_long_press_drag_input`](../../apps/layer-linux/src/tests.rs)
  and [`native_layer_hold_input`](../../apps/layer-linux/src/layer_hold_tests.rs)
  with mouse/touch/pen **rejection before hold**, plus direct pen-grip acceptance.
  Existing success after a hold does not prove that a hold is required. Check
  both GTK tile entry points, editable layer names, disabled tile wrappers, and
  drawer/column registration. Keep immediate tab/group/column motion benchmarks.
- Web: extend [`long-press-drag.test.mjs`](../../apps/layer-web/long-press-drag.test.mjs)
  and layer/interaction tests to reject immediate tile/pen-row movement and
  preserve collapsed-strip scrolling. Its pen cases explicitly synthesize a
  context-menu event; that is not proof of automatic pen hold recognition.
- Android: extend [`AndroidInteractionTest.kt`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidInteractionTest.kt)
  beyond hold-then-drag success to assert the forbidden early pickups for each
  device and immediate handle behavior. Preserve native cancellation and history.
- Apple/Windows: add device-specific native pickup and scrolling checks to the
  existing workspace and layer workflow suites; verify real Pencil/stylus and
  mouse behavior rather than relying on generic drag actions or OS defaults.

The full required click/hold/menu/cancellation/undo matrix is in the
[convention](drag-and-reorder.md#required-validation-when-implementing).
