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
| G3 | Partial-Zen toolbar tiles | Retired: Zen now has one mode, without alternate toolbar projections. | No remaining GTK source. |
| G4 | Collapsed-column icon tiles | [`workspace_columns.rs`](../../apps/layer-linux/src/workspace_columns.rs) registers held panel sources. Shared Rust resolves the collapsed icon as the source. | **Migrated.** The footer grip remains immediate; native strip scrolling stays available before hold. |
| G5 | Individual docked/floating panel tabs; active and inactive tabs, including toolbar tabs | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs), `install_panel_drag(DockItem::Panel)`; [`workspace_tab_drag.rs`](../../apps/layer-linux/src/workspace_tab_drag.rs) captures stable tab slots. The stable controller begins after native movement slop. | **Keep immediate pickup for all devices.** Preserve tab sliding, tear-off, grab offsets, and cancellation. |
| G6 | Panel/group title and tab-bar background, group grips, lone-panel footer strips | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs) registers headers/footers as `DockItem::Group`; the common captured controller has no hold requirement. | **Keep immediate pickup for all devices.** Do not accidentally apply G1's delay to a containing group. |
| G7 | Standalone toolbar grab handles | [`workspace_customization.rs`](../../apps/layer-linux/src/workspace_customization.rs) registers the grip as `DockItem::Panel`. | **Keep immediate pickup.** This is a handle despite carrying the same panel payload as some icon tiles. |
| G8 | Collapsed-column footer grab handles | [`workspace_columns.rs`](../../apps/layer-linux/src/workspace_columns.rs) registers `DockItem::Column`. | **Keep immediate pickup for mouse, touch, and pen.** |
| G9 | Column-drawer header backgrounds and individual drawer tabs | [`workspace_drawer.rs`](../../apps/layer-linux/src/workspace_drawer.rs) registers the header as a group and each tab as a panel through `install_panel_drag`. | **Keep immediate pickup**, including nested drawer/tab tear-off and docking. Do not confuse these tabs with G2's tool tiles. |
| G10 | Layer-list row bodies, including names, whitespace, content/mask thumbnails and child controls | [`layers.rs`](../../apps/layer-linux/src/layers.rs), `row_drag`, checks the actual device/tool for touchscreen and pen hold eligibility. | **Migrated.** Mouse remains immediate; row grips bypass the hold gate. Native pen hardware validation remains outstanding. |
| G11 | Layer-row trailing grip | The same [`row_drag`](../../apps/layer-linux/src/layers.rs) helper is installed with its direct/touch-enabled override on the grip. | **Keep immediate pickup for all devices.** Exempt the grip from the new pen row-body guard. |
| G12 | Dock/column split dividers and floating-window edge/corner resize handles | [`workspace.rs`](../../apps/layer-linux/src/workspace.rs), `register_drag` with `Divider` / `Resize`; any movement starts resizing. | **Keep.** These are direct resize controls, not reordered tiles or list rows. |
| G13 | Native application window title/header drag region | [`workspace_header.rs`](../../apps/layer-linux/src/workspace_header.rs) uses `GtkWindowHandle` for the caption background and informational items. | **Keep native immediate window movement.** Real windowed/restored movement, double-click maximize, native close and control secondary-click checks pass. |
| G14 | Manage Workspaces: all row bodies and narrow left grab handles | [`workspace_switcher_dialog.rs`](../../apps/layer-linux/src/workspace_switcher_dialog.rs), grouped native drag/click/hold recognizers | Mouse bodies drag immediately; touch/pen bodies hold first; handles start immediately for every device. Right-click and touch/pen hold open the row menu; same-contact movement closes it and drags. Mouse holds do not open menus. Real mouse/touch and keyboard checks cover scrolling, cancellation, and preview preservation. Hardware pen timing remains to be checked. Switcher order is an app preference and does not enter workspace layout history. |
| G15 | Drawing tabs in the window title bar | [`documents.rs`](../../apps/layer-linux/src/documents.rs) keeps native contact capture and movement slop; shared [`DocumentTabDrag`](../../crates/layer-ui/src/document_tabs.rs) applies the panel tabs' frozen slots, and `NativeTabSlide` draws the same sliding copies. `DocumentTabs` validates order changes and history. | **Implemented.** The held tab follows the contact within the strip while neighbors ease aside. Leaving the strip vertically by half its height returns them, and release there cancels. Mouse/touch immediate pickup, live offsets, detach/re-entry, Escape cancellation, single-step reorder undo, and ordinary click/tap selection pass `native_document_tab_input`. Pen uses the same immediate path; physical pen acceptance remains open. Customize Title Bar disables inner tab input. |

Saved palette color tiles are an explicit immediate-pickup exception for every device.
[`palette_drag.rs`](../../apps/layer-linux/src/palette_drag.rs) groups GTK hold/drag
gestures and filters unrelated device events to retain the original contact.
Tile movement reorders after native slop; gaps and scrollbars remain available
for scrolling. Stationary touch/pen holds open native menus, dismissed when
dragging begins. A retained swatch image follows the grab point above the workspace;
neighboring colors animate into the core's preview order around a vacant slot.
Fixed grid hit cells prevent feedback oscillation. Edge scrolling is frame-paced.
Shared library moves preserve IDs and commit once per drop, with palette-local undo/redo. History
colors and the add tile are not drag sources.

GTK workspace pickup now has one captured path. Native hold timing arms tile
reordering; source classification leaves G5–G9 immediate.

The filter/layer follow-up adds a common GTK tablet scroller and touch/pen
swipe-to-delete rows without changing pickup rules. Native mouse/touch hold and
reorder regressions pass; a Wayland tablet-v2 fixture additionally verifies pen
tool-list scrolling and layer swipe/delete. Physical pen reorder qualification
remains separate. Android and Web implement the same swipe presentation and
consume shared deletion/history rules. Huion journeys cover touch/pen scrolling
and swipe reversal/deletion; Web uses a common pen panning adapter and Android
uses Compose scrolling. See the [validation record](../development/gtk-filter-drawer.md).

The GTK [window-bar builder](window-bar.md) adds individual header-item bodies
only while editing: the full item drags immediately with movement slop for every
device, as explicitly requested for this placement-only editor on 2026-09-13.
Grips are decorative, not separate hit targets. Shared Rust uses frozen tab-group slot
thresholds and validates the final atomic move/removal. Neighbors animate;
crossing half a tile outside detaches the item with its original grab offset.
Re-entry docks it again; an outside release removes it. Escape, focus loss,
resize or an invalidated source cancels. Context actions offer
move/order/remove without dragging. Native mouse/touch checks cover hold-release,
same-contact dragging, cancellation and context menus at 1× and 2×;
physical stylus acceptance remains a hardware check, not inferred from touch.
The compact component bank has one inert, immediately draggable chip per item;
its padding, icon and label all belong to the same source. There is no click/tap
activation or persistent insertion cursor. Add Tools is itself a bank source:
dropping it opens the toolbar's separate shared picker at the
destination. Dropping a new palette source outside discards it. The inline editor
uses one preview baseline with Done/Cancel, not per-move undo entries. Done is
one ordinary workspace-history transaction. Native catalog tests cover both
mouse and touch at 1×/2×, invalidated sources, blur/Escape/outside cancellation,
inert click/hold behavior and previews excluded from saved captures.

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
| Customize Title Bar: entire item bodies and component-bank chips | [`header.js`](../../apps/layer-web/header.js) retains workspace pointer capture and feeds browser measurements to shared Rust `HeaderDrag`. | **Implemented.** Mouse/touch/pen pick up immediately after movement slop. Bank chips are drag-only; dropping Add Tools opens the existing picker. Frozen-slot previews, detach/re-entry, cancellation and one-step history are covered by [`title-bar.test.mjs`](../../apps/layer-web/title-bar.test.mjs) and the [acceptance record](../development/title-bar-web-acceptance.md). |
| Toolbar tiles, including dividers and retained drawer/Zen presentations where enabled | [`app.js`](../../apps/layer-web/app.js), `draggable`, tags held sources and uses stable workspace capture for every device. [`customization.js`](../../apps/layer-web/customization.js) supplies hold/menu ownership. | **Migrated.** HTML drag start no longer bypasses the hold. |
| Collapsed-column icon tiles | [`workspace-chrome.js`](../../apps/layer-web/workspace-chrome.js) explicitly tags icons as held sources independently of the panel payload. | **Migrated.** Shared Rust supports icon tear-off and cancellation. |
| Docked/floating/drawer tabs and title strips; toolbar, group, panel-footer and collapsed-column grips | [`app.js`](../../apps/layer-web/app.js) stable workspace pointer controller; [`workspace-chrome.js`](../../apps/layer-web/workspace-chrome.js) | **Keep immediate pickup** after movement slop. |
| Drawing tabs in the title bar | [`drawing-tabs.js`](../../apps/layer-web/drawing-tabs.js) freezes tab bounds at pickup and asks shared `DocumentTabDrag` for each slide; inert copies reuse the panel tabs' transition. | **Implemented.** Immediate pickup for every device; live offsets, detach/re-entry and outside-release cancellation pass `--drawing-tabs` in desktop Chrome and on the Huion. |
| Manage Workspaces: row bodies and narrow left grips | [`workspace-switcher.js`](../../apps/layer-web/workspace-switcher.js) delegates order and visibility to the shared manager; all rows have grips. | Mouse bodies drag immediately; touch/pen bodies require a hold; grips drag immediately on every device. Right-click or touch/pen hold opens the row menu; mouse holds never open menus. Same-contact dragging closes the menu, and release keeps it open. Automated Chrome mouse/touch/pen checks are in `workspace-switcher.test.mjs`. |
| Saved palette color tiles | [`palettes.js`](../../apps/layer-web/palettes.js) captures the tile's pointer after movement slop; shared Rust previews and commits moves. | **Implemented palette exception.** Every device drags immediately; touch/pen holds open the menu and dragging closes it. Fixed cells, a following ghost, CSS neighbor transitions, edge scrolling, cancellation and one-step Undo/Redo are covered by [`palettes.test.mjs`](../../apps/layer-web/palettes.test.mjs). |
| Layer-row bodies | [`layers.js`](../../apps/layer-web/layers.js) requires holds for touch and pen outside the grip. | **Migrated.** Mouse and all-device grips remain immediate. |
| Layer-row grips | [`layers.js`](../../apps/layer-web/layers.js), grip exemption; [`style.css`](../../apps/layer-web/style.css), `.layer-grip { touch-action:none }` | **Keep immediate pickup** for all devices. |
| Column/floating resize strips, scroll thumbs, numeric range controls, color wheel, Navigator, effect curves, canvas/tool gestures | [`app.js`](../../apps/layer-web/app.js), [`editor-panels.js`](../../apps/layer-web/editor-panels.js), [`effects.js`](../../apps/layer-web/effects.js) | Direct manipulation; no reorder delay. Browser-owned file/text DND is separate. |

## Android

| Surface | Current source behavior | Required work |
| --- | --- | --- |
| Customize Title Bar: entire item bodies, grips, overflow rows and component-bank chips | [`HeaderInput.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/HeaderInput.kt) keeps native capture at the workspace root and uses shared Rust `HeaderDrag`; [`WorkspaceHeader.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceHeader.kt) retains item identity through compaction. | **Implemented.** Every device drags immediately after native slop; bank chips are drag-only. See [`AndroidTitleBarTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidTitleBarTest.kt) and the [tablet acceptance record](../development/title-bar-android-acceptance.md). |
| Toolbar tiles, including disabled commands, divider tiles, and retained drawer presentations | [`Panels.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Panels.kt) explicitly registers held sources; [`WorkspaceInput.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceInput.kt) keeps capture at the workspace root. | **Migrated.** Native stationary holds arm all devices. Early motion retires pickup and leaves scrolling available. Zen retains its existing customization restriction. |
| Individual tabs, group/title bars, toolbar/group/footer/column grips, drawer tabs/headers | [`Workspace.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Workspace.kt), [`WorkspaceChrome.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceChrome.kt), common `workspaceGestures` | **Keep immediate pickup**. Split the tile gate from these sources. |
| Drawing tabs in the title bar | [`DrawingTabs.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/DrawingTabs.kt) freezes strip geometry at pickup; the JNI `slide` query returns shared `DocumentTabDrag` offsets, animated with the panel tabs' easing. | **Implemented.** Immediate pickup for every device. The order publishes before the slide retires, in one frame. `AndroidRasterTest#drawingTabsNativePointerAndCloseUi` checks live positions, detach/re-entry and outside cancellation with mouse, stylus and finger on the Huion. |
| Layer-row bodies, including child controls and thumbnails | [`Layers.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Layers.kt) distinguishes mouse from touch/pen and recognizes native holds across the row. | **Migrated.** Mouse is immediate; touch/pen scroll before holding. Touch/pen holds retain context-menu contact; all holds suppress child clicks, and name editing keeps native ownership. |
| Layer-row trailing grip area | [`Layers.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Layers.kt), trailing 20 dp hit region | **Keep immediate pickup** for all devices; verify the visual handle and hit region agree. |
| Saved palette color tiles | [`Palettes.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Palettes.kt) claims the contact after touch slop with its tool type and draws the lifted tile in the workspace overlay. | **Implemented palette exception.** Mouse, finger and stylus drag immediately; stationary touch/stylus holds open the shared menu and dragging closes it. [`AndroidPaletteTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidPaletteTest.kt) covers reordering, cancellation, menus and one-step Undo/Redo on a tablet with injected events; physical stylus acceptance remains open. |
| Collapsed-column icon bodies | [`WorkspaceChrome.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceChrome.kt) explicitly registers held panel sources, independently of the panel payload. | **Added.** Icons hold before dragging, including with their drawer open. Drawer tabs and footer grips remain immediate. |
| Resize strips, numeric sliders, color wheel, Navigator, curves/gradient controls, canvas/tool gestures | `Workspace.kt`, [`NumberControl.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/NumberControl.kt), [`ColorPanel.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/ColorPanel.kt), [`Navigator.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Navigator.kt), [`Effects.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/Effects.kt) | Direct manipulation; no reorder delay. |

## Apple: macOS and iPadOS

Drawing tabs added on 2026-09-20 use [`DrawingTabs.swift`](../../apps/layer-apple/Shared/Editor/DrawingTabs.swift)
with the retained native reorder adapter. Horizontal tab bodies and selector
grips pick up immediately after movement slop and slide live through the shared
`DocumentTabDrag` preview, as on GTK, Web and Android; vertical selector row bodies
require a touch/pen hold and remain immediate for mouse. Shared Rust validates
drop positions and maintains separate tab-order undo/redo. Native Mac and
physical-iPad UI tests cover row reorder, one-step undo/redo, selection and close.
The Swift owner fixture additionally exercises all three device identities,
same-contact menu dismissal and cancellation on the actual tab target model.
These automated device identities do not qualify physical Pencil sensors.

UIKit's common native reorder adapter cancels only for its owning window scene.
A two-scene fixture reproduces the former application-wide cancellation and
passes pending/held/dragging contacts, retained menus, late releases, resumed
drops and reparenting with supplied lifecycle notifications. This scopes the
adapter's cancellation across all rows/tiles/grips below; physical scene
interruption remains separate. See the
[qualification](../development/apple-handoff.md#ipad-scene-scoped-drag-cancellation).

| Surface | Current source behavior | Required work |
| --- | --- | --- |
| Customize Title Bar: whole items, overflow rows and component-bank chips | [`EditorHeader.swift`](../../apps/layer-apple/Shared/Editor/EditorHeader.swift) and [`HeaderPresentation.swift`](../../apps/layer-apple/Shared/Bridge/HeaderPresentation.swift) use the shared native header protocol with AppKit/UIKit contact capture. | Immediate pickup after native slop for every device; bank clicks/holds are inert. Native Mac and iPad customization workflows pass. The cleanup removes the duplicate item-level context gesture, leaving the native root as the single contact/menu owner. AppKit mouse/pen checks cover all sizes, secondary click, held item menus, same-contact dragging, detach/re-entry, hidden-item overflow, cancellation and one-step history on both presets. Physical Pencil and iPad keyboard coverage remain open. |
| Manage Workspaces: all row bodies and narrow left grips | Shared [`WorkspaceSwitcherRows.swift`](../../apps/layer-apple/Shared/Editor/WorkspaceSwitcherRows.swift) and AppKit/UIKit `NativeReorderInput` retain the real device and contact. | Mouse bodies and all grips are immediate; touch/pen bodies hold before dragging. AppKit mouse/tablet events and UIKit simulator workflows cover pickup, menu continuation, long-list scrolling and persisted order. The user confirmed the production iPad native vertical menu-to-drag handoff with both finger and Pencil. Direct UIKit callbacks cover native edge scrolling; the user also confirmed same-contact edge scrolling and the indicated drop in the 27-row production list with finger and Pencil, without changing the selected workspace. Mac keeps its working vertical custom row menus until a native presenter preserves the held contact. Both hosts use 56pt rows. Order is an application preference, outside workspace layout history. |
| Toolbar tiles, including drawer toolbars where gestures are enabled | [`WorkspacePanels.swift`](../../apps/layer-apple/Shared/Editor/WorkspacePanels.swift) registers held tile surfaces. [`WorkspaceReorderInteraction.swift`](../../apps/layer-apple/Shared/Bridge/WorkspaceReorderInteraction.swift) retains the native contact and menu while Rust owns movement/drop/history. | **Implemented; validation in progress.** Native AppKit mouse/tablet and UIKit touch checks cover early rejection, holds, retained menus and same-contact reorder/Undo/Redo. The expanded AppKit fixture also passes enabled tools, disabled commands and dividers in docked, floating and drawer presentations on both Apple policies. Physical-input coverage and those expanded UIKit cases remain open; AppKit tablet events are supplied, not physical pen input. |
| Tonal range tracks and handles in Tool Options and the Tool settings panel | [`RangeControl.swift`](../../apps/layer-apple/Shared/Editor/RangeControl.swift) captures the track immediately; Tool Options keeps it inside a control surface. | **Implemented.** Direct manipulation with no reorder delay for every device; a press moves the nearest endpoint, Escape or a system cancel restores it, and contacts never reach workspace drags. The Mac journey passes a handle drag and typed endpoints; physical Pencil acceptance remains open. |
| Toolbar components: slider caps, Tool Options More button and empty options space | [`ToolbarComponents.swift`](../../apps/layer-apple/Shared/Editor/ToolbarComponents.swift) registers caps and More as held tile sources, empty options space as a menu-only source, and tracks/fields as control surfaces that never start a reorder. | **Implemented.** Tracks and fields respond immediately; caps and More hold before reordering the whole component; empty options space opens the display menu on touch/pen hold or secondary click. Native Mac and physical-iPad UI journeys pass quick-drag rejection and hold-then-drag cap reordering; bridge checks cover compact-edge docking with one-step Undo/Redo. Physical Pencil acceptance remains open. |
| Workspace tabs, header backgrounds, toolbar/panel/group/column grips | [`WorkspacePanelHeader.swift`](../../apps/layer-apple/Shared/Editor/WorkspacePanelHeader.swift), [`WorkspacePanels.swift`](../../apps/layer-apple/Shared/Editor/WorkspacePanels.swift), [`WorkspaceDrawers.swift`](../../apps/layer-apple/Shared/Editor/WorkspaceDrawers.swift), common root drag | **Keep immediate pickup** after native movement slop. Both hosts pass drawer-tab reorder, tear-off, redock and whole-group tear-off. AppKit mouse/pen checks cover immediate tabs, exact Undo/Redo and grip focus-loss cancellation with increasing and repeated injected event counters. UIKit touch grip pickup and Undo pass with shared bottom clearance for iPadOS window controls; the test verifies the window stays fixed. Physical Pencil coverage remains open. |
| Covered panel groups and retained closing drawers | `WorkspacePanels.swift` and `WorkspaceDrawers.swift` disable hit testing together with source geometry; `WorkspaceDrag` keeps the existing native source registration path. | The unreachable alternate context-menu path and its platform handlers are removed. Focused drawer geometry, reorder/reopen and closing-source retirement pass; no input is admitted by these noninteractive views. |
| Saved palette color tiles and chooser rows | [`PaletteInteraction.swift`](../../apps/layer-apple/Shared/Editor/PaletteInteraction.swift) registers tiles as immediate `.swatch` sources and chooser rows as menu-only `.row` sources on the retained native adapter; [`PalettePanel.swift`](../../apps/layer-apple/Shared/Editor/PalettePanel.swift) draws fixed cells and the lifted workspace overlay. | **Implemented palette exception.** Every device drags after native slop; touch/Pencil holds open the shared swatch menu and dragging closes it; mouse holds open nothing. `testPalettes` covers a Mac mouse reorder, one-step ⌘Z and the secondary-click menu; iPad touch and physical Pencil acceptance remain open. |
| Layer-row bodies | Shared [`LayerRowInteraction.swift`](../../apps/layer-apple/Shared/Editor/LayerRowInteraction.swift) and [`LayerPanel.swift`](../../apps/layer-apple/Shared/Editor/LayerPanel.swift) use the retained native input adapter. | **Implemented; validation in progress.** Mouse is immediate; touch/pen hold before dragging. Both Apple hosts use the shared vertical action list and anchored SwiftUI editor popup; UIKit holds retain the original contact without a native menu/drag-session handoff. AppKit checks cover body pickup, covered rows, offscreen scrolling, group/locked/descendant drops, Undo/Redo and source/rename/remount/document replacement cancellation on both presets. UIKit checks cover child actions, mask/content menus, early scrolling and offscreen touch grip drops with Undo/Redo. Physical finger/Pencil downward continuation passed in the earlier native adapter; its reported upward failure prompted replacement. The new upward UIKit UI workflow passes with Undo/Redo; direct touch/pen/mouse callback checks cover both edge-scroll directions. On 2026-09-16 the user confirms upward Pencil reordering in a drawing with at least three layers: both the grab handle and held row body move the bottom layer upward, and each Undo restores the original order. UIKit hierarchy/interruption workflows and the wider physical matrix remain open. |
| Layer-row grip | `LayerRowInteraction.swift` classifies measured grip bounds as an immediate source. | **Keep immediate pickup** for every device. AppKit pen and iPad Simulator touch grips pass shared drop/Undo/Redo checks. On 2026-09-16 the user also confirms physical Pencil upward grip dragging and correct Undo. Paper remains a non-draggable background anchor. |
| Collapsed-column icon bodies | [`WorkspaceDrawers.swift`](../../apps/layer-apple/Shared/Editor/WorkspaceDrawers.swift) registers clipped held tile sources, separately from open drawer tabs for the same panel. | **Implemented; validation in progress.** Native AppKit mouse/tablet checks cover held panel menus, same-contact tear-off, new stack members, trailing group targets and exact Undo/Redo on both Apple presets. Earlier pen checks cover early rejection. UIKit held-icon and physical Pencil checks remain. |
| Stacked-column member grips and open-member resize edges | `WorkspaceDrawers.swift` and `WorkspacePanels.swift` reuse the native workspace input root and shared ordinary dock views. | Grips remain immediate and expose shared stack preferences. AppKit mouse/tablet checks cover stacking, member switching, fixed closed width, open-member resizing, focus cancellation and one-step history on both presets. Native Mac/iPad workflows pass in both themes, including stack pickup, opening, width resize and Undo/Redo, preferences and auto-hide. Physical Pencil and the full retained/scrolling matrix remain open. |
| Native window movement, resize strips, color wheel, curve/gradient controls, numeric sliders, Navigator, artwork/tool gestures | Native AppKit/UIKit input, `WorkspacePanels.swift`, [`PropertyControls.swift`](../../apps/layer-apple/Shared/Editor/PropertyControls.swift), [`NumberControl.swift`](../../apps/layer-apple/Shared/Editor/NumberControl.swift), [`NavigatorPanel.swift`](../../apps/layer-apple/Shared/Editor/NavigatorPanel.swift) | Direct manipulation; no reorder delay. |

## Windows

| Surface | Current source behavior | Required work |
| --- | --- | --- |
| Drawing tabs and compact drawing selector | [`DrawingTabs.h`](../../apps/layer-windows/DrawingTabs.h) freezes tab bounds and the strip at pickup and asks shared `DocumentTabDrag` (`capy_document_tab_slide`) for each move; retained copies ease aside with the panel tabs' 120 ms curve. Selector rows reuse `WorkspaceRowDrag`. Shared `DocumentTabs` revalidates the slide and owns order history. | **Implemented.** Tab bodies and row grips drag immediately after native slop. The held tab follows the contact; leaving the strip vertically by half its height returns the neighbors, and release there cancels. Selector bodies use immediate mouse pickup and touch/pen hold arbitration, preserving scrolling and menus. Escape, capture loss and canceled contacts cancel; order history is separate from artwork history. `exercise-hdr.ps1` checks live offsets, detach, outside-release cancellation, reorder and history with synthetic mouse, touch and pen, plus selector grips, keyboard selection and close cancellation. Physical-device acceptance remains open. |
| Saved palette color tiles | [`PalettesView.cpp`](../../apps/layer-windows/PalettesView.cpp) captures the tile's pointer after system drag slop; shared Rust previews and commits moves. | **Implemented palette exception.** Every device drags immediately; touch/pen holds open the shared menu, and dragging closes it. Fixed cells, a following popup ghost, 140 ms Composition neighbor animations, edge scrolling, outside-release and capture-loss cancellation, and one-step Ctrl+Z are covered by [`exercise-palettes.ps1`](../../apps/layer-windows/scripts/exercise-palettes.ps1) with synthetic mouse, touch and pen. Physical-device acceptance remains open. |
| Proof SDR dial | [`ProofDial.h`](../../apps/layer-windows/ProofDial.h) owns native capture/focus; shared Rust supplies geometry and recipe transactions. | Direct manipulation without reorder hold. Escape, focus/capture loss and canceled contacts restore the recipe; a completed drag is one artwork history step. Synthetic mouse/touch/pen cancellation and one-step undo/redo passed in both F16/F32 native journeys. Physical-device acceptance remains open. |
| Customize Title Bar: item bodies and component-bank chips | [`HeaderInput.cpp`](../../apps/layer-windows/HeaderInput.cpp) owns native slop, hold recognition and stable capture; `HeaderView.cpp` projects shared geometry and overflow. | Immediate placement after slop for all devices; bank chips are drag-only. Native mouse/pen/touch journeys cover hold menus, same-contact dragging, cancellation and workspace history. See [acceptance scope](../development/title-bar-windows-acceptance.md). Physical input acceptance remains open. |
| Manage Workspaces: row bodies and narrow left grips | [`WorkspaceRowDrag.cpp`](../../apps/layer-windows/WorkspaceRowDrag.cpp) uses WinUI hold recognition and stable surface capture; shared manager edits persist order. | Mouse bodies and all grips drag immediately after slop. Touch/pen bodies scroll before holding, then retain the menu contact for dragging. Native injected-input checks cover clicks, menus, keyboard access, scrolling, preview preservation and cancellation. Order is an app preference, outside workspace layout history. Physical device validation remains open. |
| Toolbar tiles, including divider, disabled-command, drawer and attached-column tile instances | [`PanelBody.cpp`](../../apps/layer-windows/PanelBody.cpp) registers held tile sources; [`WorkspaceGestures.cpp`](../../apps/layer-windows/WorkspaceGestures.cpp) uses native hold recognition and system slop, claiming stable capture/scrolling only after admission. | Mouse holds arm pickup; pen/touch menus preserve the contact and remain after held release. [`exercise-workspace-pickup.ps1`](../../apps/layer-windows/scripts/exercise-workspace-pickup.ps1) covers source/device identity, early rejection, menus, cancellation, floating/drawer instances and history. Physical device validation remains open. |
| Panel/drawer tabs and title strips; toolbar/group/footer/column grips | [`WorkspaceView.cpp`](../../apps/layer-windows/WorkspaceView.cpp), [`WorkspaceDrawers.cpp`](../../apps/layer-windows/WorkspaceDrawers.cpp), [`CollapsedColumns.cpp`](../../apps/layer-windows/CollapsedColumns.cpp), common `WorkspaceGestures` | **Keep immediate pickup**. |
| Toolbar components: brush slider caps, Tool Options More buttons and empty options space | [`ToolbarComponents.cpp`](../../apps/layer-windows/ToolbarComponents.cpp) registers only slider caps and More buttons as held tile sources and empty options space as a context-only source; slider tracks and option fields capture immediately. | Held caps and More buttons move the whole component; quick drags do not. Pen/touch holds on empty options space open the display menu. [`exercise-toolbar-components.ps1`](../../apps/layer-windows/scripts/exercise-toolbar-components.ps1) covers mouse/touch/pen slider drags, preview lifetime, bookmarks, quick cap drags and Tool Options edits. Physical device validation remains open. |
| Whole layer-row bodies, child controls and trailing grips | [`LayerRowDrag.cpp`](../../apps/layer-windows/LayerRowDrag.cpp) recognizes native device/hold/slop, transfers capture to the retained layer surface and preserves ScrollPresenter scrolling before pickup. [`LayerRow.cpp`](../../apps/layer-windows/LayerRow.cpp) gates child clicks and keeps native editing. | **Migrated.** Mouse rows and all grips are immediate; pen/touch bodies hold first. Pen/touch horizontal movement before holding reveals swipe-to-delete; reversal, cancellation and outside presses close it. The September 22 [Windows feature journey](../development/windows-features-20260922.md) qualifies native synthetic swipe deletion, reversal, an empty stack and Undo. Shared Rust validates hints and final drops. Native fixtures cover docked/floating rows and both column presentations, menus, scrolling, cancellation and Undo/Redo with all three devices. Physical acceptance remains open. |
| Tonal range tracks and handles in Tool Options and the Tool settings panel | [`RangeControl.cpp`](../../apps/layer-windows/RangeControl.cpp) captures the track immediately; Tool Options keeps it inside a control surface. | **Implemented.** Direct manipulation with no reorder delay for every device; a press moves the nearest endpoint through the shared resolver over a domain frozen for the contact, and Escape, capture loss or a canceled contact restores it. `exercise-selection.ps1` drags both presentations with the synthetic pointer driver; physical pen acceptance remains open. |
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
  presentations. The fixture expands the initial retained stack and explicitly selects individual panel drawers; the native row controller is also used in retained stacks. The user
  confirmed physical pen tab tear-off follows contact through release; injected
  pen/touch tab capture loss remains unresolved. Row acceptance does not
  establish the complete physical digitizer matrix.
- Apple: finish device-specific native pickup and scrolling checks in the
  workspace/layer suites, including real Pencil/stylus and mouse behavior.

The full required click/hold/menu/cancellation/undo matrix is in the
[convention](drag-and-reorder.md#required-validation-when-implementing).
