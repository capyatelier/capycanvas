# Drag source inventory

[Required convention](drag-and-reorder.md) · [Workspace and UI](README.md)

Source audit on 2026-09-12, updated after the GTK/Web pickup migration.
Other-platform follow-up: [short handoff](drag-pickup-handoff.md).
Hold menus are touch/pen only everywhere; mouse tile holds only arm pickup.
Preserve secondary-click and keyboard menus. GTK/Web enforce this distinction.

**Change** means source code contradicts the required pickup rule. **Keep** means
the visible registration/recognition path already has the required timing;
native input still needs regression testing when surrounding code changes.
**Gap** means a presentation has no relevant drag handler. **Verify** means native
toolkit behavior is implicit and cannot be established from source alone.

## GTK first

| ID | Draggable surface / presentation | Current path and behavior | Required work |
| --- | --- | --- | --- |
| G1 | Toolbar/ribbon tile bodies, including disabled commands and dividers | [`workspace_customization.rs`](../../apps/layer-linux/src/workspace_customization.rs) registers held tile wrappers. [`workspace.rs`](../../apps/layer-linux/src/workspace.rs) uses stable capture for mouse, touch, and pen; native long press arms pickup. | **Migrated.** Movement before hold retires pickup; clicks and shared insertion/drop actions remain. |
| G2 | Toolbar tiles in column drawers | [`workspace_drawer.rs`](../../apps/layer-linux/src/workspace_drawer.rs) registers the same held wrappers and context handlers as main toolbars. | **Migrated.** Shared drawer projections now supply tile insertion geometry. |
| G3 | Partial-Zen toolbar tiles | [`workspace_zen.rs`](../../apps/layer-linux/src/workspace_zen.rs) creates `tile_button` controls without tile drag registration. | **Gap / current presentation restriction**, not an immediate-pickup violation. The convention does not itself enable rearranging Zen projections. Any future support must require a hold. |
| G4 | Collapsed-column icon tiles | [`workspace_columns.rs`](../../apps/layer-linux/src/workspace_columns.rs) registers held panel sources. Shared Rust resolves the collapsed icon as the source. | **Migrated.** The footer grip remains immediate; native strip scrolling stays available before hold. |
| G5 | Individual docked/floating panel tabs; active and inactive tabs, including toolbar tabs | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs), `install_panel_drag(DockItem::Panel)`; [`workspace_tab_drag.rs`](../../apps/layer-linux/src/workspace_tab_drag.rs) captures stable tab slots. The stable controller begins after native movement slop. | **Keep immediate pickup for all devices.** Preserve tab sliding, tear-off, grab offsets, and cancellation. |
| G6 | Panel/group title and tab-bar background, group grips, lone-panel footer strips | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs) registers headers/footers as `DockItem::Group`; the common captured controller has no hold requirement. | **Keep immediate pickup for all devices.** Do not accidentally apply G1's delay to a containing group. |
| G7 | Standalone toolbar grab handles | [`workspace_customization.rs`](../../apps/layer-linux/src/workspace_customization.rs) registers the grip as `DockItem::Panel`. | **Keep immediate pickup.** This is a handle despite carrying the same panel payload as some icon tiles. |
| G8 | Collapsed-column footer grab handles | [`workspace_columns.rs`](../../apps/layer-linux/src/workspace_columns.rs) registers `DockItem::Column`. | **Keep immediate pickup for mouse, touch, and pen.** |
| G9 | Column-drawer header backgrounds and individual drawer tabs | [`workspace_drawer.rs`](../../apps/layer-linux/src/workspace_drawer.rs) registers the header as a group and each tab as a panel through `install_panel_drag`. | **Keep immediate pickup**, including nested drawer/tab tear-off and docking. Do not confuse these tabs with G2's tool tiles. |
| G10 | Layer-list row bodies, including names, whitespace, content/mask thumbnails and child controls | [`layers.rs`](../../apps/layer-linux/src/layers.rs), `row_drag`, checks the actual device/tool for touchscreen and pen hold eligibility. | **Migrated.** Mouse remains immediate; row grips bypass the hold gate. Native pen hardware validation remains outstanding. |
| G11 | Layer-row trailing grip | The same [`row_drag`](../../apps/layer-linux/src/layers.rs) helper is installed with its direct/touch-enabled override on the grip. | **Keep immediate pickup for all devices.** Exempt the grip from the new pen row-body guard. |
| G12 | Dock/column split dividers and floating-window edge/corner resize handles | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs), `register_drag` with `Divider` / `Resize`; any movement starts resizing. | **Keep.** These are direct resize controls, not reordered tiles or list rows. |
| G13 | Native application window title/header drag region | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs) uses `AdwHeaderBar` in the native window. | **Keep native immediate window movement.** Do not intercept it with the tile hold recognizer. |
| G14 | Manage Workspaces: all row bodies and narrow left grab handles | [`workspace_switcher_dialog.rs`](../../apps/layer-linux/src/workspace_switcher_dialog.rs), grouped native drag/click/hold recognizers | Mouse bodies drag immediately; touch/pen bodies hold first; handles start immediately for every device. Right-click and touch/pen hold open the row menu; same-contact movement closes it and drags. Mouse holds do not open menus. Real mouse/touch and keyboard checks cover scrolling, cancellation, and preview preservation. Hardware pen timing remains to be checked. Switcher order is an app preference and does not enter workspace layout history. |

GTK workspace pickup now has one captured path. Native hold timing arms tile
reordering; source classification leaves G5–G9 immediate.

### Other GTK pointer drags and non-draggable collections

| Surface | Production source | Convention impact |
| --- | --- | --- |
| Canvas drawing, Hand/space/middle/right-button panning, touch camera gestures, selections, operation/transform/ruler/figure/gradient tools | [`input.rs`](../../apps/layer-linux/src/input.rs), shared [`interaction.rs`](../../crates/layer-ui/src/interaction.rs) and engine tool input | Direct artwork/navigation manipulation. No reorder hold should be added. |
| Navigator overview and camera outline | [`navigator.rs`](../../apps/layer-linux/src/navigator.rs), `GestureDrag` | Keep immediate navigation. |
| Color wheel/field | [`tool_panels.rs`](../../apps/layer-linux/src/tool_panels.rs), `GestureDrag` | Keep immediate color picking. |
| Effect curve points | [`effects.rs`](../../apps/layer-linux/src/effects.rs), `CurveEditor` | Keep immediate point editing. This is not list-item reordering. |
| Numeric sliders, opacity/size sliders, scrollbars | [`number_control.rs`](../../apps/layer-linux/src/number_control.rs), native `GtkScale` / `GtkScrolledWindow` | Preserve native value changes and scrolling. |
| Gradient-stop editor | [`effects.rs`](../../apps/layer-linux/src/effects.rs) | Current GTK bar selects/adds stops by click; a numeric position control edits their location. No separate bar-drag reorder source was found. |
| Brush/tool-set choices, filter browser entries, picker/configuration rows, toolbar library entries, history rows, document title | [`tool_panels.rs`](../../apps/layer-linux/src/tool_panels.rs), [`effects.rs`](../../apps/layer-linux/src/effects.rs), [`workspace_customization.rs`](../../apps/layer-linux/src/workspace_customization.rs), [`workspace_manager_dialog.rs`](../../apps/layer-linux/src/workspace_manager_dialog.rs), [`workspace_history_dialog.rs`](../../apps/layer-linux/src/workspace_history_dialog.rs) | No independent reorder source was found. Choice buttons are not automatically toolbar tiles, and library ordering is not automatically a drag feature. |

Artwork group and adjustment/filter-layer rows use the Layers list/drop model;
their placement follows the layer-row policy, not toolbar tile policy. Internal
GTK `DropTarget` receivers are destinations, not additional pickup surfaces.

## Web

| Surface | Current source behavior | Required work |
| --- | --- | --- |
| Toolbar tiles, including dividers and retained drawer/Zen presentations where enabled | [`app.js`](../../apps/layer-web/app.js), `draggable`, tags held sources and uses stable workspace capture for every device. [`customization.js`](../../apps/layer-web/customization.js) supplies hold/menu ownership. | **Migrated.** HTML drag start no longer bypasses the hold. |
| Collapsed-column icon tiles | [`workspace-chrome.js`](../../apps/layer-web/workspace-chrome.js) explicitly tags icons as held sources independently of the panel payload. | **Migrated.** Shared Rust supports icon tear-off and cancellation. |
| Docked/floating/drawer tabs and title strips; toolbar, group, panel-footer and collapsed-column grips | [`app.js`](../../apps/layer-web/app.js) stable workspace pointer controller; [`workspace-chrome.js`](../../apps/layer-web/workspace-chrome.js) | **Keep immediate pickup** after movement slop. |
| Manage Workspaces: row bodies and narrow left grips | [`workspace-switcher.js`](../../apps/layer-web/workspace-switcher.js) delegates order and visibility to the shared manager; all rows have grips. | Mouse bodies drag immediately; touch/pen bodies require a hold; grips drag immediately on every device. Right-click or touch/pen hold opens the row menu; mouse holds never open menus. Same-contact dragging closes the menu, and release keeps it open. Automated Chrome mouse/touch/pen checks are in `workspace-switcher.test.mjs`. |
| Layer-row bodies | [`layers.js`](../../apps/layer-web/layers.js) requires holds for touch and pen outside the grip. | **Migrated.** Mouse and all-device grips remain immediate. |
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

Remaining implementation work is on Android, Apple, and Windows; see the
[short handoff](drag-pickup-handoff.md). Keep hold timing out of motion benchmarks.

## Tests that need coverage changes

- GTK: [`drag_pickup_tests.rs`](../../apps/layer-linux/src/drag_pickup_tests.rs)
  exercises real Mutter mouse/touch pickup and rejection, drawers, collapsed
  icons, immediate tabs/grips, cancellation, and undo/redo. Existing workspace
  and layer hold suites retain menu/scroll coverage. Physical pen testing remains.
- Web: [`drag-pickup.test.mjs`](../../apps/layer-web/drag-pickup.test.mjs) checks
  browser-delivered mouse/touch/pen contacts, including automatic holds and early
  rejection. Existing hold suites cover menus and native touch scrolling.
  Physical stylus hardware testing remains.
- Android: extend [`AndroidInteractionTest.kt`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidInteractionTest.kt)
  beyond hold-then-drag success to assert the forbidden early pickups for each
  device and immediate handle behavior. Preserve native cancellation and history.
- Apple/Windows: add device-specific native pickup and scrolling checks to the
  existing workspace and layer workflow suites; verify real Pencil/stylus and
  mouse behavior rather than relying on generic drag actions or OS defaults.

The full required click/hold/menu/cancellation/undo matrix is in the
[convention](drag-and-reorder.md#required-validation-when-implementing).
