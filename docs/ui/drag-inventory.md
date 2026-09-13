# Drag source inventory

[Required convention](drag-and-reorder.md) · [Workspace and UI](README.md)

Source audit on 2026-09-12, updated after the GTK/Web/Android pickup migrations.
Other-platform follow-up: [short handoff](drag-pickup-handoff.md).
Hold menus are touch/pen only everywhere; mouse tile holds only arm pickup.
Preserve secondary-click and keyboard menus. GTK/Web/Android enforce this distinction.

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
| Toolbar tiles, including disabled commands, divider tiles, and retained drawer presentations | [`Panels.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Panels.kt) explicitly registers held sources; [`WorkspaceInput.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceInput.kt) keeps capture at the workspace root. | **Migrated.** Native stationary holds arm all devices. Early motion retires pickup and leaves scrolling available. Zen retains its existing customization restriction. |
| Individual tabs, group/title bars, toolbar/group/footer/column grips, drawer tabs/headers | [`Workspace.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Workspace.kt), [`WorkspaceChrome.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceChrome.kt), common `workspaceGestures` | **Keep immediate pickup**. Split the tile gate from these sources. |
| Layer-row bodies, including child controls and thumbnails | [`Layers.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Layers.kt) distinguishes mouse from touch/pen and recognizes native holds across the row. | **Migrated.** Mouse is immediate; touch/pen scroll before holding. Touch/pen holds retain context-menu contact; all holds suppress child clicks, and name editing keeps native ownership. |
| Layer-row trailing grip area | [`Layers.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Layers.kt), trailing 20 dp hit region | **Keep immediate pickup** for all devices; verify the visual handle and hit region agree. |
| Collapsed-column icon bodies | [`WorkspaceChrome.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceChrome.kt) explicitly registers held panel sources, independently of the panel payload. | **Added.** Icons hold before dragging, including with their drawer open. Drawer tabs and footer grips remain immediate. |
| Resize strips, numeric sliders, color wheel, Navigator, curves/gradient controls, canvas/tool gestures | `Workspace.kt`, [`NumberControl.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/NumberControl.kt), [`ColorPanel.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/ColorPanel.kt), [`Navigator.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Navigator.kt), [`Effects.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Effects.kt) | Direct manipulation; no reorder delay. |

## Apple: macOS and iPadOS

| Surface | Current source behavior | Required work |
| --- | --- | --- |
| Manage Workspaces: all row bodies and narrow left grips | Shared [`WorkspaceSwitcherRows.swift`](../../apps/layer-apple/Shared/Editor/WorkspaceSwitcherRows.swift) and AppKit/UIKit `NativeReorderInput` retain the real device and contact. | Mouse bodies and all grips are immediate; touch/pen bodies hold before dragging. AppKit mouse/tablet events and UIKit simulator workflows cover pickup, menu continuation, long-list scrolling and persisted order. The user confirmed the production iPad native vertical menu-to-drag handoff with both finger and Pencil. Direct UIKit callbacks cover native edge scrolling; the user also confirmed same-contact edge scrolling and the indicated drop in the 27-row production list with finger and Pencil, without changing the selected workspace. Mac keeps its working vertical custom row menus until a native presenter preserves the held contact. Both hosts use 56pt rows. Order is an application preference, outside workspace layout history. |
| Toolbar tiles, including drawer toolbars where gestures are enabled | [`WorkspacePanels.swift`](../../apps/layer-apple/Shared/Editor/WorkspacePanels.swift) registers held tile surfaces. [`WorkspaceReorderInteraction.swift`](../../apps/layer-apple/Shared/Bridge/WorkspaceReorderInteraction.swift) retains the native contact and menu while Rust owns movement/drop/history. | **Implemented; validation in progress.** Native AppKit mouse/tablet and UIKit touch checks cover early rejection, holds, retained menus and same-contact reorder/Undo/Redo. Disabled/divider/drawer tiles and the complete physical-input matrix still need coverage. |
| Workspace tabs, header backgrounds, toolbar/panel/group/column grips | [`WorkspacePanelHeader.swift`](../../apps/layer-apple/Shared/Editor/WorkspacePanelHeader.swift), [`WorkspacePanels.swift`](../../apps/layer-apple/Shared/Editor/WorkspacePanels.swift), [`WorkspaceDrawers.swift`](../../apps/layer-apple/Shared/Editor/WorkspaceDrawers.swift), common root drag | **Keep immediate pickup** after native movement slop. AppKit grip/focus-loss checks pass. UIKit touch grip pickup and Undo pass with shared bottom clearance for iPadOS window controls; the test verifies the window stays fixed. Physical Pencil coverage remains open. |
| Layer-row bodies | Shared [`LayerRowInteraction.swift`](../../apps/layer-apple/Shared/Editor/LayerRowInteraction.swift) and [`LayerPanel.swift`](../../apps/layer-apple/Shared/Editor/LayerPanel.swift) use the retained native input adapter. | **Implemented; validation in progress.** Mouse is immediate; touch/pen hold before dragging. iPad uses native vertical row menus; Mac retains a row-anchored popover with tested pen continuation. AppKit checks cover body pickup, covered rows, offscreen scrolling, group/locked/descendant drops, Undo/Redo and source/rename/remount/document replacement cancellation on both presets. UIKit checks cover child actions, mask/content menus, early scrolling and offscreen touch grip drops with Undo/Redo. Physical layer finger/Pencil continuation, UIKit hierarchy/interruption workflows and the wider physical matrix remain open. |
| Layer-row grip | `LayerRowInteraction.swift` classifies measured grip bounds as an immediate source. | **Keep immediate pickup** for every device. AppKit pen and iPad Simulator touch grips pass shared drop/Undo/Redo checks. Paper remains a non-draggable background anchor. |
| Collapsed-column icon bodies | [`WorkspaceDrawers.swift`](../../apps/layer-apple/Shared/Editor/WorkspaceDrawers.swift) registers clipped held tile sources, separately from open drawer tabs for the same panel. | **Implemented; validation in progress.** Native AppKit pen checks cover early rejection, held panel menus, same-contact tear-off and Undo on both Apple presets. UIKit and physical Pencil column checks remain. |
| Partial-Zen toolbar projections | `WorkspaceDrawers.swift` disables workspace gestures. | **Current presentation restriction.** Any future rearrangement support must require a hold. |
| Native window movement, resize strips, color wheel, curve/gradient controls, numeric sliders, Navigator, artwork/tool gestures | Native AppKit/UIKit input, `WorkspacePanels.swift`, [`PropertyControls.swift`](../../apps/layer-apple/Shared/Editor/PropertyControls.swift), [`NumberControl.swift`](../../apps/layer-apple/Shared/Editor/NumberControl.swift), [`NavigatorPanel.swift`](../../apps/layer-apple/Shared/Editor/NavigatorPanel.swift) | Direct manipulation; no reorder delay. |

## Windows

| Surface | Current source behavior | Required work |
| --- | --- | --- |
| Manage Workspaces: row bodies and narrow left grips | [`WorkspaceRowDrag.cpp`](../../apps/layer-windows/WorkspaceRowDrag.cpp) uses WinUI hold recognition and stable surface capture; shared manager edits persist order. | Mouse bodies and all grips drag immediately after slop. Touch/pen bodies scroll before holding, then retain the menu contact for dragging. Native injected-input checks cover clicks, menus, keyboard access, scrolling, preview preservation and cancellation. Order is an app preference, outside workspace layout history. Physical device validation remains open. |
| Toolbar tiles, including divider, disabled-command and drawer tile instances | [`PanelBody.cpp`](../../apps/layer-windows/PanelBody.cpp) registers held tile sources; [`WorkspaceGestures.cpp`](../../apps/layer-windows/WorkspaceGestures.cpp) uses native hold recognition and system slop, claiming stable capture/scrolling only after admission. | Mouse holds arm pickup; pen/touch menus preserve the contact and remain after held release. [`exercise-workspace-pickup.ps1`](../../apps/layer-windows/scripts/exercise-workspace-pickup.ps1) covers source/device identity, early rejection, menus, cancellation, floating/drawer instances and history. Physical device validation remains open. |
| Panel/drawer tabs and title strips; toolbar/group/footer/column grips | [`WorkspaceView.cpp`](../../apps/layer-windows/WorkspaceView.cpp), [`WorkspaceDrawers.cpp`](../../apps/layer-windows/WorkspaceDrawers.cpp), [`CollapsedColumns.cpp`](../../apps/layer-windows/CollapsedColumns.cpp), common `WorkspaceGestures` | **Keep immediate pickup**. |
| Partial-Zen toolbar projections | `ZenToolbars.cpp` keeps automation IDs on the actual buttons; `WorkspaceGestures` uses context-only recognition for pen/touch. | Existing movement restriction retained. Pen/touch holds open menus and suppress tile activation; mouse keeps ordinary long button presses. |
| Whole layer-row bodies, child controls and trailing grips | [`LayerRowDrag.cpp`](../../apps/layer-windows/LayerRowDrag.cpp) recognizes native device/hold/slop, transfers capture to the retained layer surface and preserves ScrollPresenter scrolling before pickup. [`LayerRow.cpp`](../../apps/layer-windows/LayerRow.cpp) gates child clicks and keeps native editing. | **Migrated.** Mouse rows and all grips are immediate; pen/touch bodies hold first. Shared Rust validates hints and final drops. Native fixtures cover docked/floating/drawer rows, menus, scrolling, cancellation and Undo/Redo with all three devices. Physical acceptance remains open. |
| Collapsed-column icon bodies | `CollapsedColumns.cpp` registers held panel sources independently of the panel/tab payload. Stable workspace capture survives icon removal during tear-off. | Native scrolling cancels pending pickup; ordinary taps toggle the drawer and held/dragged releases suppress that click. The workspace pickup fixture checks mouse/touch/pen icon gestures; physical device validation remains open. |
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

Remaining implementation work is on Apple and Windows; see the
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
- Android: [`AndroidInteractionTest.kt`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidInteractionTest.kt)
  delivers native mouse/touch/pen MotionEvents on the tablet. It covers early
  rejection, toolbar/divider/drawer/collapsed-icon holds, immediate grips/tabs,
  native row scrolling, menus, source removal, focus loss, and undo/redo.
- Windows: `exercise-layer-pickup.ps1` checks native mouse/touch/pen row bodies,
  child controls, grips, scrolling, menus and history in docked/floating/drawer
  presentations. Pen workspace-tab tear-off capture loss and physical digitizer
  validation remain open; row acceptance does not establish tab acceptance.
- Apple: finish device-specific native pickup and scrolling checks in the
  workspace/layer suites, including real Pencil/stylus and mouse behavior.

The full required click/hold/menu/cancellation/undo matrix is in the
[convention](drag-and-reorder.md#required-validation-when-implementing).
