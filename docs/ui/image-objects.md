# Image layers and image objects

[Workspace and UI](README.md)

An image layer holds placed images instead of paint. Each image keeps its own
pixels, color interpretation, Nearest or Linear smoothing and a 64-bit affine
placement, so it can be moved, scaled, rotated and flipped any number of times
without resampling. The layer itself keeps the name, visibility, opacity, blend
mode, lock, mask and filters. Shared Rust owns every rule below; hosts present
the projections and forward native input. The record contract is in the
[authored model](../reference/authored-model.md#object-layers-and-image-objects).

## Creating image layers

Place Image, an image file dropped on the canvas or a layer row, and an image
pasted from another app become images. They join the active editable image
layer, otherwise a new image layer uses the ordinary
[insertion rules](open-and-import.md). Place and ordinary Paste open placement
handles: Apply commits one undo step, and Cancel or Undo leaves no records or
history. A second pasted image joins the same layer as the first.
Paste Into a pixel selection makes a new image layer whose stored mask is that
selection, with the pasted content centred on it; moving the image moves it behind
the fixed mask. Pixels copied inside Capy Canvas still paste as a paint layer.
See [copy and paste](clipboard.md) for copying images between drawings.

Convert to Image Layer turns a paint layer's current appearance into one image,
and Rasterize Layer turns an image layer back into paint. Both keep the layer's
mask, filters, opacity and blend so they apply once, and each is one undo step;
[documents](../internals/documents.md) describes the conversions and merges.

## Selecting and moving images

With an image layer active, Move and Scale–Rotate target its images. Selection
tools, mask editing, Crop, the ruler and an explicit Move Layer keep their own
targets. Image selection is separate from the layer-row selection and from a
pixel selection: a pixel selection never cuts or clears an image, and Delete on
selected images removes whole images.

- A click picks the frontmost visible image in an unlocked, visible image layer,
  using its placed rectangle. Clicking an image in another layer activates that
  layer. The handles of the current selection win over other images, and a click
  on empty canvas clears the image selection without changing the active layer.
- Shift-click adds or removes an image in the active layer. Shift on an image in
  another layer keeps the selection and explains that Shift adds images from the
  same layer only. A Shift drag on a handle scales proportionally.
- Touch on an image selects or moves it; touch on empty canvas keeps the usual
  pan and zoom gestures.
- A drag moves, scales or rotates the selection about its pivot, and dragging the
  pivot moves it. Every pose previews from the starting affines and is checked by
  the renderer before it is shown; a completed drag is one undo step and Escape
  cancels it. Alt (or Leave Copy) copies the images when the drag starts;
  cancelling removes the copies.
- Arrow keys nudge by one pixel, or ten with Shift. A held key is one undo step.
- Image-layer bounds are snap targets for Move. The moving layer and its groups
  are excluded.

The layer panel, canvas bar, menus and shortcuts share one target rule, so their
labels and enabled states always agree:

| Command | With images selected |
| --- | --- |
| Select All / Deselect | Selects every image in the active image layer, including hidden ones, or clears the image selection. |
| Delete, Duplicate (Ctrl/Cmd+J), Copy, Cut | Act on whole images in one undo step. Cut removes them only after the clipboard accepts them. Delete with nothing selected does nothing. |
| Transform Again | Repeats the last committed image transform of this drawing; Undo or another drawing makes it unavailable. |
| Original Size | Restores one source pixel per document pixel, keeping each image's centre, rotation and mirroring. |
| Flip and rotate | Exact reflections and quarter turns about the pivot, one undo step. |
| Smoothing | Nearest or Linear only. Distort, Warp, Perspective, Bicubic and Lanczos are paint-only. |
| Copy Pixels | Copies the image layer's appearance as pixels instead of the images. |

## Image rows

An image layer's row shows its image count and an expand control. Expanded,
`LayerState.objects` lists one indented row per image, front to back, with a
visibility toggle, a preview requested through `ThumbnailTarget::Object`, the
name and a grip. Rows use object tokens, never layer IDs. A click selects an
image and Shift, Ctrl or Cmd extends the selection; the row menu offers the same
commands plus a touch-friendly Add to Selection. Dragging a row reorders the
image within its layer, following the [drag convention](drag-and-reorder.md).
Image rows have no mask, clip, lock, alpha-lock, swipe or filter controls; those
stay on the layer row. Hidden or overlapping images are reached through these
rows.

## Painting on an image layer

Brushes, fills, clears, pixel cuts, retouch tools and Frequency Separation
never write to an image layer. They raise a notice naming the layer with three
actions: Add Mask (or Edit Mask), New Paint Layer and Rasterize Layer. A disabled
action carries its reason. Choosing an action rechecks the drawing and layer and
never replays the refused stroke. Notices carry an ordered `actions` list with
stable `NoticeActionId` tokens, and hosts answer with
`UiAction::Notice { id, accept, action }`.

## Tests

`object_editing_tests` in `layer-ui` cover picking, both command tables, gestures,
nudges, copies, cancel and undo; `notice` and `clipboard` tests cover the refusal
actions and image clips. Host journeys are GTK's
`native_image_object_rows_picking_and_transforms`,
`native_image_object_clipboard_and_paste_into`,
`native_image_layer_conversions_merges_and_alpha_selection` and
`native_image_and_pixel_targets_and_crop_keep_images`, Web's `--image-rows`,
Android's `AndroidInteractionTest#imageRowsTouchPickingMenusAndRefusalActions`, and the
Windows `exercise-image-rows.ps1` fixture.
