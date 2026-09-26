# Canvas action bar

[Workspace and UI](README.md) · [Panel transparency](panel-transparency.md) · [Toolbar components](toolbar-components.md) · [Phase 1 plan](../development/canvas-action-bar-transforms.md)

The canvas action bar shows the next steps for the object being edited, beside it. It is an accelerator: every item is an ordinary command, so the menus, Tool Options and command search stay complete, and each item keeps its shared validation and one-step history.

Status: shared model and GTK host implemented. Web and Android are in progress; Apple and Windows keep their earlier placement controls until they adopt the bar.

## Contexts

| Context | Shown when | Items | Placement |
| --- | --- | --- | --- |
| Placement | A photo is being placed or pasted | Mode, Original Size, flips, quarter turns, Reset · Cancel, Apply | Beside the photo |
| Transform | Transform is open on paint, a mask or selected pixels | Mode (Free, Uniform, Distort), Perspective while distorting, Flip H/V, Rotate 90° left/right, Reset · Cancel, Apply | Beside the transform box |
| Polygon | A polygon selection is under construction | Remove Last Point · Cancel, Finish | Bottom edge |
| Selection | A selection exists and a selection tool or Move is active, or a command such as Select All just made it | Deselect, Invert, Transform, Mask, Fill, Quick Mask, Save as Selection Layer | Beside the selection |

- **Selection bar visibility:**
  - Hidden under painting tools and while editing a mask, Quick Mask or a Selection Layer.
  - Undo and Redo that restore a selection do not bring it back.
  - A tool change ends the visibility that a selection command started.
- **Bottom-edge placement:** inverted, tonal and painted selections use the bottom edge.
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
- **Switching modes keeps the geometry:**
  - Returning to Free from a perspective quad keeps it under a bounding-box frame.
  - A parallelogram folds back into position, scale, rotation and skew exactly.
- **Flips and quarter turns** act in the layer's axes about the centre of the transformed box.
- **Reset** returns to Free and the geometry the transform started with.
- **Touch:** a finger inside the box or on a handle manipulates the transform; elsewhere it navigates.

## Implementation

- **Shared model:** `crates/layer-ui/src/canvas_bar.rs` holds the context derivation, items, `CanvasBarEdit` validation, fitting, placement and the More menu. Transform geometry and modes live in `crates/layer-ui/src/operation.rs`.
- **GTK host:** `apps/layer-linux/src/canvas_bar.rs` is a `DockSurface` slot.
- **Tests:**
  - shared: `crates/layer-ui/src/canvas_bar_tests.rs`;
  - GTK native: `native_canvas_bar_input` and `native_canvas_bar_polygon_input` in `apps/layer-linux/src/canvas_bar_tests.rs`.
