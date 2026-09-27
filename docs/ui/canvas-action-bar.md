# Canvas action bar

[Workspace and UI](README.md) · [Panel transparency](panel-transparency.md) · [Toolbar components](toolbar-components.md) · [Phase 1 plan](../development/canvas-action-bar-transforms.md)

The canvas action bar shows the next steps for the object being edited, beside it. It is an accelerator: every item is an ordinary command, so the menus, Tool Options and command search stay complete, and each item keeps its shared validation and one-step history.

Every host presents the bar. Bar item menus open on GTK, Web, Android, Windows, macOS and iPadOS. The mode and guide bars need no host code of their own.

## Contexts

| Context | Shown when | Items | Placement |
| --- | --- | --- | --- |
| Placement | A photo is being placed or pasted | Mode, Original Size, flips, quarter turns, Reset · Cancel, Apply | Beside the photo |
| Transform | Transform is open on paint, a mask or selected pixels | Mode (Free, Uniform, Distort, Warp), Perspective while distorting, Grid while warping, Flip H/V, Rotate 90° left/right, Reset, Interpolation · Cancel, Apply | Beside the transform box |
| Polygon | A polygon selection is under construction | Remove Last Point · Cancel, Finish | Bottom edge |
| Guide | A guide is selected with the Ruler or Move tool and guides are shown | Delete, Snap, Guides | Beside the guide's handles |
| Quick Mask | Quick Mask is on | "Quick Mask" · Invert, Fill, Clear, Refine ▾, Save as Selection Layer · Exit | Bottom edge |
| Selection Layer | A Selection Layer is being edited | "Editing *name*" · Load, Invert · Return to Artwork | Bottom edge |
| Layer mask | A layer's mask is being edited | "Editing *layer* mask" · Invert, Disable, Apply Mask · Edit Content | Bottom edge |
| Selection | A selection exists and a selection tool or Move is active, or a command such as Select All just made it | Deselect, Invert, Copy to Layer ▾, Transform, Refine ▾, Mask, Adjust ▾, Fill, Clear ▾, Crop, Quick Mask, Save as Selection Layer | Beside the selection |

- **Precedence:** a transform or placement, then a polygon under construction, then a selected guide, then a mode, then the selection.
- **Modes** (Quick Mask, Selection Layer and layer-mask editing) have no object to sit beside, so they use the bottom edge with a label, their actions and an accented exit. They show under every tool, including the painting tools these modes choose. Their exits keep every edit; there is no Apply.
  - Quick Mask's Invert, Fill and Clear act on the working mask and stay in the mode. Refine ▾ is the selection bar's menu.
  - A Selection Layer's Invert (`InvertSelectionLayer`) inverts its stored coverage and stays in the mode; Load (`LoadSelectionLayer`) makes it the selection and returns to the artwork.
  - Disable (`LayerMaskEnabled`) reads Enable while the mask is off. Apply Mask (`ApplyLayerMask`) is unavailable, with a reason, on groups, effect layers and disabled masks. Edit Content (`EditLayerContent`) returns to the layer's pixels.
  - More adds the mode's Layer menu: the Quick Mask menu, the Selection Layer's menu or the mask's menu.
- **Guide bar:** it anchors to the bounds of the guide's handles and keeps clear of them. A Move click that misses every guide deselects it, and the selected guide is highlighted under Move as under Ruler.

- **Selection bar visibility:**
  - Hidden under painting tools and while editing a mask, Quick Mask or a Selection Layer.
  - Undo and Redo that restore a selection do not bring it back.
  - A tool change ends the visibility that a selection command started.
- **Bottom-edge placement:** inverted, tonal and painted selections use the bottom edge.
- **Compact items:** flips and quarter turns show only their icons; their names are in tooltips, accessibility labels and More.
- **Accent:** Apply and Finish are drawn in the accent color.
- **Dropdowns:** a choice that is not segmented, such as Interpolation, opens its items from the bar.
- **Menu items:** an item can carry a menu. It opens from the bar as a dropdown, and in More it is a submenu.
  - **Copy to Layer ▾:** Copy Selection to New Layer (the primary command) and Cut Selection to New Layer.
  - **Refine ▾:** Grow… and Shrink….
  - **Adjust ▾:** the Filter menu's categories, which hold adjustments only. The new effect layer takes the selection as its mask and consumes it, in one undo step.
  - **Clear ▾:** Clear Selected Pixels (the primary command) and Clear Outside Selection.
- **More:** lists the items that did not fit, then the context's own menu (the full Select menu for selections), then the bar toggle.
- **Distort on photo placements:** refused with the route that works: select all, then transform the pixels.
- **Crop:** Crop Canvas to Selection crops the canvas to the bounds of the selection's coverage, as metadata: pixels outside stay on their layers and reappear when the canvas grows. It is disabled, with a reason, for an inverted selection.
- **Not on the bar:** Canvas Size… (Edit › Image) and Layer › New › Solid Color Fill and Gradient Fill (which also take the selection as their mask) and Revert to Original Photo have no bar item; menus and command search reach them.

## Placement

- **Measure and place:** the host measures its controls; the session places the bar and says how many items fit.
- **Near object:**
  - The bar goes below the object, clear of every handle, including the rotate handle after a vertical flip. Failing that, above it.
  - It is clamped to the free work area, clear of the HUD and floating panels.
- **Bottom edge:** used when the object is off-screen, covers more than 60% of the work area, or the work area is narrower than 600 logical pixels.
- **Stable in Zen:** placement uses the non-Zen layout, so the bar does not jump when chrome reveals.

## Behavior

- **Panel layer:** the bar is a glass surface in the panel layer: above floating panels, below drawers, the header and menus. It follows the transparency setting and stays visible in Zen. Menus opened from it stay opaque.
- **Hiding:**
  - A bar beside an object hides while a canvas contact is in progress and while the camera moves. Bottom-edge bars stay put.
  - Every bar hides while a floating panel group is dragged.
  - It returns 180 ms after input settles, at its new place.
  - The session decides: `canvas_bar_hold()` changes whenever the bar must hide and is odd while it must stay hidden. Hosts read it after input and changes, so no state is published at pen-down, and keep only the reappear timer.
- **Input:** taps on the bar are chrome contacts and never paint.
- **Focus:** its controls do not take keyboard focus, and it never opens a window of its own. Losing window focus leaves an open transform intact.
- **Availability:** item availability holds its previous value while the canvas is busy, as Tool Options does. Polygon construction commands follow the path live.
- **Toggle:** **View → Show canvas action bar** is a workspace layout preference. While it is off, transforms, placements and polygons keep their completion items at the bottom edge; mode, guide and selection bars hide.
- **Escape** cancels a gesture first. Otherwise it leaves Quick Mask and Selection Layer editing, and leaves layer-mask editing when no popup, drawer, title-bar edit or enabled Escape binding claims it.

## Transform modes

- **Free:** scale, rotate and move with the box handles. Ctrl-dragging an edge skews about the opposite edge.
- **Uniform:** Free with proportions kept.
- **Distort:** each corner moves independently and each edge moves both of its corners. **Perspective**, or Shift, mirrors a corner drag onto its neighbour so opposite sides stay symmetric.
- **Warp:** a mesh of curved patches over the content.
  - Drag a node to bend the mesh around it. Pressing a node shows its tangent handles, which shape the curves leaving it. Dragging elsewhere inside moves the whole mesh.
  - **Grid** offers 3 × 3 (the default), 4 × 4 and 5 × 5 cells. Changing it keeps the current shape.
- **Switching modes keeps the geometry:**
  - Returning to Free from a perspective quad keeps it under a bounding-box frame.
  - A parallelogram folds back into position, scale, rotation and skew exactly.
  - Warp starts from the current box or quad. Leaving Warp keeps the mesh, and Free or Distort then act on its hull.
- **Flips and quarter turns** act in the layer's axes about the centre of the transformed box, or of the mesh's hull while warping.
- **Reset** returns to Free and the geometry the transform started with.
- **Applying a distorted pixel selection:** a soft or painted selection cannot follow a perspective map as metadata, so Apply first resamples its coverage on the GPU.
  - The transform stays open, and Apply reads "Applying the transform" until the coverage returns; the result is one undo step.
  - Cancel discards the pending coverage, and any further edit to the transform supersedes it.
- **Interpolation:** Nearest neighbor, Bilinear or Bicubic, on the bar and in Tool Options.
  - Until one is chosen, Free and Uniform resample bilinearly and Distort bicubically.
  - A chosen filter stays for later transforms in the session.
  - Previews draw a moving bicubic transform bilinearly; the still preview and Apply use the chosen filter.
  - Placed photos keep their original pixels, so placements do not offer it.
- **Touch:** a finger inside the box or on a handle manipulates the transform; elsewhere it navigates.

## Implementation

- **Shared model:** `crates/layer-ui/src/canvas_bar.rs` holds the context derivation, items, `CanvasBarEdit` validation, fitting, placement and the More menu. Transform geometry and modes live in `crates/layer-ui/src/operation.rs`.
- **Mode items:** a plan item can show a command as a plain button with its own label (Exit, Return to Artwork, Edit Content, Disable/Enable), so the same command reads differently on different bars. The bar's context changes with the edited layer, the selected guide and the tool.
- **Menu items:** `CanvasBarItem.menu` names a `CanvasBarMenu` (`copy_to_layer`, `clear`, `refine`, `adjust`; `copy` is reserved for the clipboard commands).
  - The item's `option` stays its primary command, so a host that ignores `menu` still has a working button. Refine and Adjust have no primary command yet; their `option` is an empty, non-segmented choice whose id is the menu id, which opens the same menu.
  - `canvas_bar_choice_menu(context, id)` serves the menu with every action wrapped in `CanvasBarEdit`, and `CanvasBarMenu::icon` names its icon.
  - A bar edit is accepted when its action is on the current bar or anywhere in one of its item menus.
- **Disabled items** carry the command's `disabled_reason`, steady during a contact like `enabled`. GTK shows it as the item's tooltip; Web and Windows show it as the item's hover tooltip and reveal the same tooltip when a disabled item is tapped with any device.
- **GTK host:** `apps/layer-linux/src/canvas_bar.rs` is a `DockSurface` slot. A menu item is a `gtk::MenuButton` built like a dropdown choice, showing its menu's icon and label, whose popover `populate_canvas_bar_choice` fills; it never takes focus, and a disabled primary command disables it with its reason as the tooltip.
- **Web host:** `apps/layer-web/canvas-bar.js` is a glass toolbar in `#workspace`, built from the Tool Options field builders in `toolbar-components.js`. It measures its controls once per bar and moves with a transform. A menu item is a menu button with its icon, label and an arrow. It opens the menu from the `canvas_bar_choice_menu` export in the shared popover menu, as More does, below the bar or above it when there is no room; a second press closes it. Submenus replace the menu page. The button never takes focus, and a disabled primary command disables it with its reason. The item labels are part of the bar's schema, so a relabelled item such as Disable/Enable is rebuilt with its new label.
- **Android host:** `apps/layer-android/app/src/main/java/art/capycanvas/CanvasBar.kt` is a glass Compose surface in the workspace, above floating groups and below drawers, built from the Tool Options `ToolOptionField` and `toolOptionSize` builders. `NativeHost::query` answers `canvas_bar_layout`, `canvas_bar_menu` and `canvas_bar_choice_menu`, and `Native.canvasBarHold` reads the hold. A disabled item shows its published `disabled_reason` when tapped, held or hovered, and a hold opens nothing else. The bar registers its glass region before its first visible frame, and draws in its own layer sized to the bar and its shadow, so showing, hiding and moving it re-records only that layer. While hidden it stays composed but unplaced: it draws nothing, takes no input, registers no glass or chrome region and exposes no semantics, so returning it only places it again. Its menus, and the Tool Options choice and value menus, open without taking window focus; the value popup takes focus only while its number field is edited. A menu item is a menu button (`ToolOptionMenu`) with its icon, label and an arrow. It opens the menu from `canvas_bar_choice_menu` as a windowless menu, as More does, and a submenu opens as a page of the same menu. The button never takes focus, and a disabled primary command disables it with its reason. An action is drawn pressed only when its option is checkable, so a relabelled plain button such as Disable/Enable never looks pressed, as on GTK and Web.
- **Apple host:** `apps/layer-apple/Shared/Editor/CanvasBar.swift` is a glass SwiftUI row inside the `WorkspacePanels` stack, above floating groups and collapsed columns and below drawers. It reuses the Tool Options `ToolOptionField` and `toolOptionSize` builders with captions, places itself through `canvas_bar_layout`, and opens More as the shared editor menu from `canvas_bar_menu`. A menu item is a menu button with its icon, label and an arrow, sized as on Web, which opens `canvas_bar_choice_menu` in the same editor menu. The serial owner reads `capy_apple_canvas_bar_hold` after each canvas pointer batch and frame; `CanvasBarPresence` hides the bar at every change and returns it after `canvas_bar_reappear_ms` once the value is even. A disabled item shows its published `disabled_reason` on hover and when tapped or clicked.
- **Windows host:** `apps/layer-windows/CanvasActionBar.h` is a glass squircle in the workspace canvas. It measures its controls once per bar and places itself through the `canvas_bar_layout` native host query again only when the bar or the layout changes. More, dropdown choices and menu items open native menus from `canvas_bar_menu` and `canvas_bar_choice_menu`; a menu item is a button with its icon, label and an arrow that never takes focus. A disabled item stays out of hit testing inside a transparent host that shows the published `disabled_reason` on hover and reveals it for 4 s when tapped with any device, until the next contact. The canvas input thread reports the first and last canvas contact, so the bar hides without a published state change.
- **Narrow windows:** when docks leave the work area narrower than the smallest bar, placement uses the window width.
- **Tests:**
  - shared: `crates/layer-ui/src/canvas_bar_tests.rs`;
  - GTK native, in `apps/layer-linux/src/canvas_bar_tests.rs`: `native_canvas_bar_input`, `native_canvas_bar_polygon_input`, `native_canvas_bar_distorts_a_pixel_selection`, `native_canvas_bar_finger_moves_a_transform`, `native_canvas_bar_warps_a_selection` and `native_canvas_bar_selection_menus` (Copy to Layer, Clear ▾ › Clear Outside and Adjust ▾ › Curves with mouse, finger and pen; run both with `--tablet`), `native_delete_clears_pixels_unless_a_guide_is_selected`, `native_canvas_bar_modes` (Quick Mask → Invert → Exit, Selection Layer → Invert → Return to Artwork and layer mask → Disable → Edit Content with mouse and finger, and a notice above the bottom-edge bar) and `native_canvas_bar_guide` (select a guide with the Ruler tool, then Delete, with mouse, finger and pen; run with `--tablet`);
  - Web: `node --test apps/layer-web/canvas-bar.test.mjs` (including a mode bar's label, accented exit and a relabelled item), `node apps/layer-web/test.mjs --headless --canvas-bar` (with pen, touch and mouse: Copy to Layer, Clear ▾ › Clear Outside Selection and Adjust ▾ › Curves, through More when an item does not fit; Delete over a selection and on a focused drawing tab; Quick Mask → Invert → Exit, Selection Layer → Invert → Return to Artwork and layer mask → Disable → Edit Content; Escape in each mode; a notice above the bottom-edge bar; a guide drawn with the Ruler tool, then Delete), and `device.test.mjs --canvas-bar` on a tablet, which also opens a bar menu and the layer-mask bar with real pen, touch and mouse taps; `--notices` covers disabled-item reasons on a locked layer;
  - Android: `AndroidInteractionTest#canvasActionBarJourneysAcrossDevices` and `#canvasBarSelectionMenusAcrossDevices` (Copy to Layer, Clear ▾ › Clear Outside and Adjust ▾ › Curves on the bar and through More) with mouse, finger and stylus, `#modeBarsLeaveFromTheirExitsAcrossDevices` (the three modes from their bars, Escape, and a notice above the bottom-edge bar), `#guideBarDeletesTheSelectedGuideAcrossDevices`, `#selectionBarOverflowsIntoMoreInBothOrientations`, `#hardwareDeleteClearsSelectedPixels`, and `AndroidCanvasBarBenchmarkTest` for frame timing on a 6000 × 4000 canvas (see the [Android guide](../development/android.md)).
  - Apple: `EditorLaunchTests/testCanvasActionBar` on macOS and a physical iPad covers overflow into More, the Refine and Adjust menus, modes, Interpolation, bending a Warp edge node with the Grid choice and Reset, Cancel/Apply history, the toggle, Zen and placement beside a selection; the ABI test `native/src/canvas_bar_tests.rs` covers stale edits, published disabled reasons, the selection menus and the contact hold.
  - Windows: `apps/layer-windows/scripts/exercise-canvas-bar.ps1` with mouse, touch and pen (Refine ▾ and Copy to Layer ▾ with one-step Undo, a finger moving a transform, Warp with its Grid choice and Escape cancelling), `exercise-notice.ps1` for disabled-item reasons on a locked layer, and the native host queries in `crates/layer-host/src/lib.rs`.
