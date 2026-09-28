# Image commands

[Workspace and UI](README.md)

**Edit › Image** holds the commands that change the whole image: Crop, Crop
Canvas to Selection, Canvas Size…, Image Size…, Rotate Image 90° Left and Right,
Rotate Image 180°, Flip Image Horizontally and Vertically, Trim and Reveal All.
Command search finds them all.

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
  - Paint layers and masks are resampled. Placed photos scale their placement and
    keep their original pixels. The selection, Selection Layers and guides scale
    with the image, and effect settings measured in pixels, such as a blur
    radius, scale too, within their range.
  - A size beyond the drawing's size, tile or memory limits is refused in the
    dialog, with the reason, before anything changes.
- **Rotate Image 90° Left and Right, Rotate Image 180°, Flip Image Horizontally
  and Vertically** move pixels exactly, without resampling. Quarter turns swap
  the width and height, and the horizontal and vertical resolution. Placed photos
  turn their placement, and the selection, Selection Layers and guides follow.
  The view's own rotation and flips (View menu) never change pixels.
- **Trim** shrinks the canvas to the visible pixels on it, cutting away transparent
  edges. Only visible layers count, limited by their masks; the paper covers the
  whole canvas, so hide it to trim to the artwork. Pixels outside stay hidden on
  their layers. When nothing would change, a notice says so.
- **Reveal All** grows the canvas to hold every layer's pixels, including hidden
  layers, pixels hidden by masks, and placed photos. When every pixel is already on
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
- **Content bounds:** `crates/layer-core/src/content_bounds.rs` finds pixel-tight
  bounds on the CPU. Each side of a layer decodes only its outermost column or row
  of tiles and moves inward only past transparent ones; within the canvas it also
  scans the tiles the canvas edges cross. Photos count by their placement, and
  masks limit what is visible. Results are cached by tile content, so asking again
  decodes nothing. A `ContentScope` picks what counts: the canvas (Trim), visible
  layers (Fit Content) or everything (Reveal All).
- **Commands:** `crates/layer-ui/src/image_geometry.rs` runs the turns, flips, Trim,
  Reveal All and Fit Content. A bounds scan decodes at most four tiles on the UI
  thread, then continues on a worker thread; Web, without threads, continues a few
  tiles per frame, and its frame loop keeps running while the scan is busy
  (`wants_continuous_frames`). `image_size.rs` and `canvas_size.rs` hold the dialog
  models, and hosts only present them.
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
- **Tests:**
  - shared: `crates/layer-core/src/canvas_geometry_tests.rs` (exact turns and flips,
    Image Size, resolution, tile predictions) and `content_bounds_tests.rs`;
    `crates/layer-ui/src/image_size_tests.rs` and `image_geometry_tests.rs`; the
    pixels in `crates/layer-render-wgpu/tests/canvas_geometry.rs`;
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
