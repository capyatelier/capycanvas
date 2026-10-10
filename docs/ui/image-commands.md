# Image commands

[Workspace and UI](README.md)

**Image** sits between Edit and Layer. Image Size… and Canvas Size… come first,
followed by Crop, Crop Canvas to Selection, Trim and Reveal All. **Rotate and
Flip** groups the whole-image quarter turns, half turn and flips.
**Color Management** holds Assign Profile…, Convert Color Space… and Change Bit
Depth…; **Blending** holds Perceptual Blending and Linear Light Blending.
See [color management](color-management.md). Command search finds them all.

- **One step each:** every command is one undo step, and applies to locked layers
  too; locks protect content, not the document's size.
- **Unavailable, with a reason,** while a crop, transform or placement is open, in
  Quick Mask or Selection Layer editing, and while a file operation runs.
- **The view stays put:** a crop keeps the image where it was on screen; a turn,
  flip or resize keeps the point at the middle of the view there.

## Hidden pixels

Layers keep pixels outside the canvas. A crop or a smaller canvas hides them, and
a larger canvas, Reveal All or Fit Content shows them again. Delete Cropped Pixels
on the crop bar drops them instead ([crop](canvas-action-bar.md#crop)).

Moving paint reserves enough local space to paint across the canvas again. Existing
hidden pixels and mask coverage stay intact. An oversized move keeps its last valid
position; releasing commits that position as one undo step.

## Layer transforms

Move shifts a paint layer, object layer or group by whole pixels; a linked mask
follows its owner and an unlinked mask stays in place. **Transform**, with
Distort, Perspective and Warp, works on one paint layer: applying it resamples
the layer's current pixels once, including content outside the canvas and paint
wetness, together with a linked mask. Exact flips and quarter turns move samples
without resampling. A Distort or Warp only needs to be valid over the content it
moves. Groups and several layers move by whole pixels only, and object layers
transform their images instead ([object layers](image-objects.md)).

The work runs in the background with Cancel on the canvas bar. A failed or
canceled transform changes nothing; one Undo restores the layer and its mask.
**Rasterize Source…** instead turns a photo layer's photo into ordinary paint in
place.

## Commands

- **Crop** opens the Crop tool and its bar ([crop](canvas-action-bar.md#crop)).
  **Fit Content** on the crop bar sets the frame to the visible pixels, including
  those beyond the canvas.
- **Crop Canvas to Selection** crops to the bounds of the selection's coverage.
- **Canvas Size…** adds or removes canvas around the image from a 3 × 3 anchor, in
  pixels or percent.
- **Image Size…** scales the whole image. Ctrl+Alt+I opens it in the Photoshop and
  Affinity keys.
  - Width and Height are in pixels or percent. **Constrain proportions**, on by
    default, makes one follow the other.
  - **Resolution** is in pixels per inch. Changing only the resolution changes no
    pixels; it is what exports record for printing.
  - **Resample:** Automatic (Lanczos when reducing, Bicubic when enlarging),
    Bicubic, Lanczos, Bilinear or Nearest neighbor. A strong reduction averages
    every source pixel under each new pixel with any filter but Nearest neighbor.
  - Paint layers and masks are resampled. A photo layer with nothing painted on
    it keeps the photo's own color and depth, resampled; one with paint on it,
    or a CMYK photo, is resampled with its paint into ordinary paint. Image
    layers scale their images without resampling them. The selection, Selection
    Layers and guides scale
    with the image, and effect settings measured in pixels, such as a blur
    radius, scale too, within their range.
  - A size beyond the drawing's size, tile or memory limits is refused in the
    dialog, with the reason, before anything changes.
- **Rotate Image 90° Left and Right, Rotate Image 180°, Flip Image Horizontally
  and Vertically** move pixels exactly, without resampling. Quarter turns swap
  the width and height, and the horizontal and vertical resolution. A photo layer
  turns its photo exactly, keeping its own color and depth, while a worker moves
  its samples; object layers turn their images, and the selection, Selection
  Layers and guides follow.
  The view's own rotation and flips (View menu) never change pixels.
- **Trim** shrinks the canvas to the visible pixels on it, cutting away transparent
  edges. Visible layers, masks, filters and paper contribute their rendered
  coverage. Hide opaque paper to trim to the artwork. Pixels outside stay hidden on
  their layers. When nothing would change, a notice says so.
- **Reveal All** grows the canvas to hold every layer's pixels, including hidden
  layers, pixels hidden by masks, and placed images. When every pixel is already on
  the canvas, a notice says so.
- **Large drawings:** Trim, Reveal All and Fit Content find the pixels' bounds in
  the background and apply when it finishes. Changing the drawing in the meantime
  cancels them with a notice.

## Implementation

- **Geometry:** `crates/layer-core/src/canvas_geometry.rs` turns a
  `CanvasGeometry` (a crop rectangle, `CanvasGeometry::orient` for turns and flips,
  or `CanvasGeometry::resize`) into one batch with its pixel operations, checked
  against the limits first. `Edit::SetResolution` changes only the resolution.
  How the renderer resamples and drops emptied tiles is in
  [rendering](../internals/rendering.md).
- **Pixel bake:** `crates/layer-core/src/transform_pixels.rs` freezes the layer and
  plans its output extent. History admission reserves the output before work starts.
  The existing snapshot worker maps each raw plane and captures native tiles;
  `image_geometry.rs` publishes the complete replacement only while its document
  and target still match. Bounds queries and pixel bakes share this cancellable
  worker lifecycle.
- **Content bounds:** `crates/layer-core/src/content_bounds.rs` freezes the query
  and caches its result. `crates/layer-render-wgpu/src/snapshot/bounds.rs` uses
  bounded snapshot rendering and the thumbnail GPU reduction to measure actual
  positive alpha or mask coverage. Only four bounds coordinates return from the
  GPU. Transparent source padding and erased overrides do not count; visible
  queries evaluate the actual mask products and interpolation fringes. Sparse
  candidate pages avoid scanning empty gaps between distant layers. Effect
  coordinates and accumulated animation phases stay fixed for the query.
  Boundary pages run first; pages wholly inside already measured bounds need
  no further work. Virtual capture origins preserve the document's tile grid.
  Up to eight pages share a readback within the snapshot memory allowance;
  larger regions run alone. Captures reuse immutable scene pipelines,
  transfer tables and the bounds reducer on their device.
  `ContentScope` selects the canvas
  (Trim), visible layers (Fit Content), every layer (Reveal All), or the editable
  target intersected with the selection (Transform and Move). An unedited source
  whose channel format proves opacity needs no scan.
  A destructive linked paint/mask pair measures both targets before opening
  Transform, so its Warp domain preserves the companion's pixels too. Whole-layer
  retained transforms measure each paint target's coverage; selected roots and
  group descendants share one document-space frame.
- **Commands:** `crates/layer-ui/src/image_geometry.rs` runs the turns, flips, Trim,
  Reveal All and Fit Content. Bounds preparation and rendering run on a worker,
  including a dedicated Web worker. The frame loop keeps running while the query
  is pending. Target changes, cancellation and renderer replacement discard stale
  results. Move remembers motion and release while preparing; Escape cancels a
  pending Transform. Image placement and Original Size use each image's own
  frame; paint transforms use measured coverage. `image_size.rs` and
  `canvas_size.rs` hold the dialog models, and hosts only present them.
- **Hosts:** each host presents Image Size beside Canvas Size.
  - GTK: `apps/layer-linux/src/image_size.rs` and `canvas_size.rs`.
  - Web: `apps/layer-web/image-size.js` and `canvas-size.js`, modal dialogs built
    from the parts in `size-dialog.js`. A dialog opens with its title focused, so
    no field takes the keyboard until tapped; typed text reaches the draft before
    any other choice, and Apply stays disabled while the view can't apply.
  - Android: `ImageSize.kt` and `CanvasSize.kt`, panels built from `SizePanel.kt`.
    A panel sits over the undimmed canvas at the top of the work area and above the
    keyboard, takes window focus only while a number field is edited, and opens
    Resample as a windowless menu. Back cancels it.
  - Windows: `SizeDialog.cpp`, one content dialog for both, built from the
    published view. A field is rebuilt only when its numeric spec changes, and
    sends from a replaced field are ignored; typed text reaches the draft before
    any other choice, and Apply stays disabled while the view can't apply.
- **Tests:**
  - shared: `crates/layer-core/src/canvas_geometry_tests.rs` (exact turns and flips,
    Image Size, resolution, tile predictions);
    `crates/layer-ui/src/image_size_tests.rs` and `image_geometry_tests.rs`; the
    pixels in `crates/layer-render-wgpu/tests/canvas_geometry.rs` and
    `crates/layer-render-wgpu/src/content_bounds_tests.rs`;
  - GTK native, in `apps/layer-linux/src/image_tests.rs`:
    `native_image_size_down_with_constrain_then_undo`,
    `native_rotate_image_right_on_a_non_square_canvas`,
    `native_reveal_all_after_a_crop`, `native_crop_fit_content_from_the_bar` and
    `native_trim_to_the_visible_pixels`, plus the benchmark
    `native_image_size_timing_on_a_24_mp_photo`;
  - Web: `node --test apps/layer-web/size-dialog.test.mjs` for both dialogs, and
    `node apps/layer-web/test.mjs --headless --image-commands` (or
    `device.test.mjs --image-commands` on a tablet) with mouse, touch and pen: Image
    Size to 50% with Constrain proportions then Ctrl+Z, Rotate Image 90° Right on a
    non-square canvas, a crop then Reveal All, Trim, and Fit Content on the crop
    bar, plus a bounds scan that finishes over several frames;
  - Android: `AndroidInteractionTest#imageCommandsAcrossDevices`, the same journeys
    with mouse, finger and stylus.
  - Windows: `apps/layer-windows/scripts/exercise-size-dialogs.ps1`: Canvas Size
    (typed width, anchor, Relative, units, Apply with Undo and Redo, Escape) and
    Image Size (resampling choice, constrained width, Apply with Undo, Escape).
