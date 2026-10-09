# Object layers

[Workspace and UI](README.md)

Raster layers contain editable pixels. Object layers retain source content and a
nondestructive transform. Images are the supported object content today; vectors
can use the same layer concept when supported. Groups organize ordinary layers.
There is no image-container layer or separate image selection.

## Import and placement

Place Image, dropping an image, and pasting an external image create one named
Object layer per file using the ordinary [insertion rules](open-and-import.md).
Each source keeps its original samples, color interpretation, Nearest or Linear
sampling and 64-bit affine transform. Importing while an Object layer is active
creates a sibling layer. Multiple files create selected sibling layers in the
provider's order, with shared placement handles.

Apply commits the whole batch as one undo step. Cancel or Undo removes the
provisional layers and restores the previous selection without adding history.
The placement handles stay with the imported layers until Apply or Cancel.
Placement clears a pixel selection. Paste Into instead consumes the selection
and stores it as the new layer's mask; the source moves behind that fixed mask.
See [copy and paste](clipboard.md) for destination and shortcut rules.

Convert to Object Layer preserves a Raster layer's current source appearance.
Rasterize Layer produces editable pixels. Both preserve layer properties, masks
and filters, applying them once, and are one undo step. An empty Raster layer
converts to a transparent source and remains a valid editable layer.

## Selection and transforms

Object layers use ordinary layer rows, thumbnails, selection, ordering, naming,
visibility, locks, masks, opacity, blending, filters and context menus.

- Move and Scale–Rotate transform selected Object layers without resampling the
  source. Each source retains its own layer and ancestor offsets.
- Canvas picking selects the frontmost visible, unlocked Object layer under the
  pointer. Shift-click extends or reduces the ordinary layer selection across
  layers. Existing transform handles take precedence over picking.
- Dragging moves, scales or rotates about the pivot. Each gesture is one undo
  step; Escape cancels it. Alt or Leave Copy duplicates the selected layers at
  gesture start; cancellation removes the copies.
- Arrow keys nudge one pixel, or ten with Shift. A held key is one undo step.
- Original Size restores one source pixel per document pixel while retaining
  center, rotation and mirroring. Flips and quarter turns are exact.
- All moving layers and their ancestors are excluded from snapping targets.
- Pixel selections, mask editing and mixed layer selections retain the ordinary
  target rules. Select All and Deselect act on the pixel selection.

Copy, Cut and Duplicate use ordinary selected layers. Without a pixel selection,
Copy preserves the source, transform, mask and filters and publishes a rendered
PNG for other apps. Cut removes layers only after clipboard publication succeeds.
Copy Pixels explicitly captures appearance before layer properties.

Brushes, fills, pixel cuts, retouch and Frequency Separation require Raster
content. The existing notice offers Add/Edit Mask, New Paint Layer or Rasterize
Layer and does not replay a refused stroke. Distort, Warp, Perspective, Bicubic
and Lanczos require rasterization; Object transforms use Nearest or Linear.

## Ownership and verification

An occurrence refers directly to one image object containing its immutable
source, affine and interpolation. Names and visibility belong only to the layer.
Layer selection is the only object selection. See the
[authored model](../reference/authored-model.md#object-layers-and-image-objects)
and [package contract](../reference/capy-package.md).

Shared `object_editing_tests`, `image_object_edit_tests`, conversion and clipboard
tests cover insertion, selection, transforms, masks, cancellation and undo.
Native journeys are GTK's `native_image_object_rows_picking_and_transforms` and
conversion/clipboard tests, Web's `--image-rows`, Android's
`AndroidInteractionTest#imageRowsTouchPickingMenusAndRefusalActions`, Apple's
`EditorLaunchTests/testImageObjects` and `testImageObjectsDark`, and Windows'
`exercise-image-rows.ps1`. Run UI journeys in both themes.
