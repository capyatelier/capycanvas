# Canvas action bar

[Workspace and UI](README.md) · [Panel transparency](panel-transparency.md) · [Toolbar components](toolbar-components.md) · [Phase 1 plan](../development/canvas-action-bar-transforms.md)

The canvas action bar shows the next steps for the object being edited, beside it. It is an accelerator: every item is an ordinary command, so the menus, Tool Options and command search stay complete, and each item keeps its shared validation and one-step history.

Status: shared model, GTK, Web, Android, Windows, macOS and iPadOS hosts implemented.

## Contexts

| Context | Shown when | Items | Placement |
| --- | --- | --- | --- |
| Placement | A photo is being placed or pasted | Mode, Original Size, flips, quarter turns, Reset · Cancel, Apply | Beside the photo |
| Transform | Transform is open on paint, a mask or selected pixels | Mode (Free, Uniform, Distort, Warp), Perspective while distorting, Grid while warping, Flip H/V, Rotate 90° left/right, Reset, Interpolation · Cancel, Apply | Beside the transform box |
| Polygon | A polygon selection is under construction | Remove Last Point · Cancel, Finish | Bottom edge |
| Selection | A selection exists and a selection tool or Move is active, or a command such as Select All just made it | Deselect, Invert, Transform, Mask, Fill, Quick Mask, Save as Selection Layer | Beside the selection |

- **Selection bar visibility:**
  - Hidden under painting tools and while editing a mask, Quick Mask or a Selection Layer.
  - Undo and Redo that restore a selection do not bring it back.
  - A tool change ends the visibility that a selection command started.
- **Bottom-edge placement:** inverted, tonal and painted selections use the bottom edge.
- **Compact items:** flips and quarter turns show only their icons; their names are in tooltips, accessibility labels and More.
- **Dropdowns:** a choice that is not segmented, such as Interpolation, opens its items from the bar.
- **More:** lists the items that did not fit, then the context's own menu (the full Select menu for selections), then the bar toggle.
- **Distort on photo placements:** refused with the route that works: select all, then transform the pixels.

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
  - A bar beside an object hides while a canvas contact is in progress; the input reply reports this, so no state is published at pen-down.
  - It also hides while the camera moves.
  - It returns 180 ms after input settles, at its new place. Bottom-edge bars stay put.
- **Input:** taps on the bar are chrome contacts and never paint.
- **Focus:** its controls do not take keyboard focus, and it never opens a window of its own. Losing window focus leaves an open transform intact.
- **Availability:** item availability holds its previous value while the canvas is busy, as Tool Options does. Polygon construction commands follow the path live.
- **Toggle:** **View → Show canvas action bar** is a workspace layout preference. While it is off, transforms and placements keep Cancel and Apply at the bottom edge.

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
- **GTK host:** `apps/layer-linux/src/canvas_bar.rs` is a `DockSurface` slot.
- **Web host:** `apps/layer-web/canvas-bar.js` is a glass toolbar in `#workspace`, built from the Tool Options field builders in `toolbar-components.js`. It measures its controls once per bar and moves with a transform.
- **Android host:** `apps/layer-android/app/src/main/java/art/capycanvas/CanvasBar.kt` is a glass Compose surface in the workspace, above floating groups and below drawers, built from the Tool Options `ToolOptionField` and `toolOptionSize` builders. `NativeHost::query` answers `canvas_bar_layout`, `canvas_bar_menu`, `canvas_bar_choice_menu` and `canvas_bar_reason`. The bar registers its glass region before its first visible frame, and draws in its own layer sized to the bar and its shadow, so showing, hiding and moving it re-records only that layer. Its menus, and the Tool Options choice and value menus, open without taking window focus; the value popup takes focus only while its number field is edited.
- **Apple host:** `apps/layer-apple/Shared/Editor/CanvasBar.swift` is a glass SwiftUI row inside the `WorkspacePanels` stack, above floating groups and collapsed columns and below drawers. It reuses the Tool Options `ToolOptionField` and `toolOptionSize` builders with captions, places itself through `canvas_bar_layout`, and opens More as the shared editor menu from `canvas_bar_menu`. The serial owner reads `capy_apple_canvas_bar_hidden` after each canvas pointer batch; camera changes hide a bar beside an object until the shared delay has passed.
- **Windows host:** `apps/layer-windows/CanvasActionBar.h` is a glass squircle in the workspace canvas. It places itself through the `canvas_bar_layout` native host query and opens More and dropdown choices through `canvas_bar_menu` and `canvas_bar_choice_menu`. The canvas input thread reports the first and last canvas contact, so the bar hides without a published state change.
- **Narrow windows:** when docks leave the work area narrower than the smallest bar, placement uses the window width.
- **Tests:**
  - shared: `crates/layer-ui/src/canvas_bar_tests.rs`;
  - GTK native, in `apps/layer-linux/src/canvas_bar_tests.rs`: `native_canvas_bar_input`, `native_canvas_bar_polygon_input`, `native_canvas_bar_distorts_a_pixel_selection`, `native_canvas_bar_finger_moves_a_transform`, and `native_canvas_bar_warps_a_selection` (mouse, finger and pen; run with `--tablet`);
  - Web: `node --test apps/layer-web/canvas-bar.test.mjs`, `node apps/layer-web/test.mjs --headless --canvas-bar`, and `device.test.mjs --canvas-bar` on a tablet;
  - Android: `AndroidInteractionTest#canvasActionBarJourneysAcrossDevices` with mouse, finger and stylus, and `AndroidCanvasBarBenchmarkTest` for frame timing on a 6000 × 4000 canvas (see the [Android guide](../development/android.md)).
  - Apple: `EditorLaunchTests/testCanvasActionBar` on macOS and a physical iPad covers overflow into More, modes, Interpolation, Cancel/Apply history, the toggle, Zen and placement beside a selection; the ABI test `native/src/canvas_bar_tests.rs` covers stale edits and contact hiding.
  - Windows: `apps/layer-windows/scripts/exercise-canvas-bar.ps1` with mouse, touch and pen, and the native host queries in `crates/layer-host/src/lib.rs`.
