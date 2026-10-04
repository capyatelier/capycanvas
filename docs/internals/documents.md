# Documents and edits

[Technical documentation](../README.md) · [Architecture](../architecture.md)

The document records enough information to reconstruct an editable drawing. It
is separate from the GPU textures used to display it and from the workspace used
to edit it. This distinction matters when implementing undo, saving a project or
recreating a renderer.

Parking or suspending a drawing clears its source-aware analysis tasks.
Window-state adoption and renderer replacement reset those tasks without
sending commands to a retired renderer. Analysis resumes after a live renderer
is attached.

## Artwork and history

[`Document`](../../crates/layer-core/src/lib.rs) contains the layer structure and
its drawing data. Paint layers contain strokes and ordered operations; image
layers refer to source assets; groups organize child layers. Layer properties
control visibility, opacity, blending, clipping and related composition behavior.
[Layer definitions](../../crates/layer-core/src/layers.rs) also describe masks and
selection coverage.

With a filter selected, a stroke paints the first artwork layer below it in the
same group, or its clipping base. Groups, maskless generators, locked bases and
selection layers refuse with a reason (`Document::try_drawing_target`). An empty
stack stays valid.

New documents contain an empty paint layer above a white Solid Color fill named
Paper. The fill starts without a mask and follows ordinary layer rules: it can
be renamed, moved, grouped, duplicated, hidden, deleted or merged. Add a mask to
paint on it. Its thumbnail shows its color and alpha over the checkerboard.

Paint and photo layers retain a `LayerPlacement`: one outer homography, an
optional shared `MeshMap`, and interpolation. `Document::layer_geometry` returns
the complete map in document coordinates, including a linked mask's premap.
Pixel writers use `validate_content_write` and `affine_edit_transform`; a
nonlinear destination requires [Apply Transform to Pixels](../ui/image-commands.md#apply-a-layer-transform-to-pixels).
Read consumers use complete placed geometry. Explicit local extents stay fixed
when a capture grows its virtual canvas; editable affine paint grows through the
shared extent planner when the real canvas changes.

`retained_transform_targets` normalizes selected roots and checks every descendant
before preview. `retained_transform_edit` composes one document-space delta into
paint placement and independent masks, without moving group offsets or ordinary
adjustment coordinates. Apply creates one history edit; canceled and unchanged
transforms preserve both history directions. Mesh control roots are shared across
document and history, charged once by the existing resource accounting.

Effect layers hold filters in the layer stack. An adjustment transforms the
combined image below it within its group; a clipped adjustment acts on its
clipping stack, preserving the base layer’s coverage. Multiple adjustments apply
in layer order, forming an effect chain. A generator effect instead produces
content that is composited as a layer. Masks and opacity control where and how
strongly an effect applies.

### Groups and Pass Through

A group is isolated by default: its layers composite over transparency, and that
result goes over the layers below with the group's blend mode, opacity and mask.
**Pass Through**, a blend mode only groups can use (`LayerBlend::PassThrough`),
composites the group's layers onto the layers below it instead, as if they were
not grouped. A non-Normal layer inside blends with the layers below the group, and
an adjustment inside changes everything below it up to the nearest isolated group
([`isolated_scope`, `backdrop_layers`](../../crates/layer-core/src/layers.rs)).
The group's opacity and mask fade between what lies below and that result,
`lerp(below, result, opacity × mask)`. A clipped Pass Through group composites
isolated, as a Normal group.

New groups, from New Group and Group Selected Layers, are isolated Normal groups
unless the [Use Pass Through for new groups](../ui/settings.md) preference is on.
Ungroup keeps the image, so it needs a group at full opacity, with no mask and
not clipped, that is either Pass Through, holding any layers, or Normal, holding
only Normal layers. A referenced adjustment inside a Pass Through group keeps what
lies below the group in the reference composite.

### Blending

A document's **Blending** (`Document.blend_space`) sets how its layers combine:
- **Perceptual**, like Photoshop and Clip Studio Paint: opacity, masks, groups,
  clipping and blend modes work on the document's encoded values, so black at
  50% over white is middle gray (8-bit 128) and Soft Light follows Photoshop's
  formula. Brush dabs, brush blend modes, healing's tone match and the
  retouching filters (Gaussian Blur, Unsharp Mask, High Pass and similar) work
  on encoded values too. New 8- and 16-bit documents and photos opened as
  documents start Perceptual.
- **Linear light**, physically based: layers combine in linear document RGB.
  Float documents always blend this way, and documents saved before the setting
  existed read as Linear.

Painted pixels keep their values either way; only their combination changes
([blend space](rendering.md#blend-space)). **Edit ▸ Blending** switches it in
one undo step (`Edit::SetBlendSpace`), refused at float depth with "Float
documents blend in linear light". Converting a document to float makes it
Linear in the same step, and undo restores both. Paint color mixing follows each
brush's Color mixing choice instead, and resampling, Liquify and filters that
model light, such as Vignette and Bloom, stay linear.

### Merging layers

Merge Down, Merge Group, Merge Visible, Flatten Image and Stamp Visible, in the
Layer menu and the layer's menu, replace layers with one new paint layer in one
undo step ([`merge.rs`](../../crates/layer-core/src/merge.rs)). The new layer has
full opacity, Normal blending and no mask. It takes the place, name and clipping
of the layer it replaces, and references move to it. Placed photos become
ordinary document pixels.

- **Merge Down** merges the active layer into the artwork layer below it in its
  group. Both must be visible, unlocked and Normal, and the layer below must hold
  pixels or generated content, rather than an adjustment. A clipped layer below takes only a
  layer clipped to the same base. A clipping base instead merges its visible
  clipped layers into itself (Merge Clipped Layers), as does an adjustment
  clipped to a base. An unclipped adjustment applies to the layer below only
  (Apply Effect to Layer Below), so whatever else lies below no longer takes it.
- **Merge Group** composites a visible group, with its mask, into one layer that
  keeps the group's blend mode and opacity. Hidden layers inside are discarded. A
  Pass Through group is composited isolated into a Normal layer, as in Photoshop,
  so layers inside that blended with those below can look different.
- Every merge composites in the document's Blending, so the result looks as the
  layers did.
- **Merge Visible** composites the visible layers over transparency. Hidden
  layers stay; hidden layers clipped to a merged base are released. Visible
  fills, including Paper, participate in the composite.
- **Flatten Image** does the same, then discards hidden layers and pixels
  outside the canvas. When it would discard hidden layers, it asks first through
  the canvas notice.
- **Stamp Visible** adds the visible image as a new top layer and keeps every
  layer.

Locks block every merge except Stamp Visible, and a group that holds Selection
Layers must give them up first. A merge is refused with a reason when its result
would exceed the 1 GiB publication limit or its undo step would not fit the
history budget. Merge Down, Merge Group and Merge Visible keep pixels outside the
canvas: the result's extent is the union of the merged layers' extents, on whole
tiles from the canvas origin. A merge that includes an effect layer covers the
canvas only, because effects are defined over the canvas.

### Retouching layers

**New Dodge & Burn Layer**, in Layer › New, adds a Soft Light layer named Dodge &
Burn above the active layer and the layers clipped to it, in its group, filled
with the gray Soft Light leaves unchanged: in Perceptual documents the value
stored as 8-bit 128 or 16-bit 32768, in Linear-light ones linear 0.5
(`Document::soft_light_neutral` in
[`retouch_layers.rs`](../../crates/layer-core/src/retouch_layers.rs)). White paint
on it lightens the image below and black paint darkens it. Inserting, filling
and activating the layer are one undo step.

**Frequency Separation…**, in the Filter menu, splits the active paint layer.
Its dialog sets a Radius, the radius of the Gaussian Blur filter, and the canvas
previews that blur while the document stays as it is; Cancel leaves nothing, and
an edit of the drawing closes the dialog. Apply inserts, in one undo step, an
isolated Normal group named Frequency Separation directly above the layer, with
its opacity and clipping, holding Low, the layer blurred, and above it High,
High Pass at half strength against the same blur, in Linear Light. The layer
stays below the group, hidden, as Affinity Photo keeps its original. Both are
baked from the layer's pixels and mask over the canvas, so pixels outside the
canvas are left out. The group is isolated whatever the
[Use Pass Through for new groups](../ui/settings.md) preference says.

High and Low add up to the layer again, within two codes at 8 and 16 bits,
because they share one blur and Linear Light adds encoded values. That holds only
when layers blend perceptually, so the command is refused in Linear-light and
float documents with "Frequency Separation needs Perceptual blending. Change it
in Edit ▸ Blending." It also needs a visible, Normal paint layer outside a
locked group.

A `Stroke` stores real pen samples and a `BrushSnapshot`, which captures the brush
settings used for that stroke. Committed sample storage is shared rather than
copied whenever history changes. Later sensor corrections replace the affected
storage while existing snapshots remain stable.

`Editor` applies `Edit` values and retains the reverse operations for undo/redo.
Fills, figures, transforms and mask operations preserve their ordering and source
coverage so reconstruction does not silently depend on the current selection or
brush. The GPU renderer can therefore rebuild the visible image from the document
and its source assets when necessary.

Workspace movement has a separate history. Moving a toolbar should not occupy
the same undo stack as an ink stroke, and changing the camera should not mark the
artwork as modified.

## Editable projects

[`Project`](../../crates/layer-core/src/project.rs) packages document data with
the reachable source assets in a `.capy` file. It retains imported images, custom
brush textures and embedded filter definitions, so reopening a drawing does not
silently use a different version of a filter from the installed catalog.

The project format does not store GPU handles, preferences or the undo stack.
It stores the drawing's editable state, not the application's entire session.
Source images and textures are available without reading the rendered canvas back from GPU memory.
Exporting a PNG is a separate operation that flattens the image through an explicit
GPU readback.

The [project format reference](../reference/project-format.md) describes the
container, validation limits and replay requirements. The format is still in
development; compatibility across unreleased builds is not guaranteed.

## File operations

The session's [document-file state](../../crates/layer-ui/src/document_files.rs)
tracks the current location, unsaved changes and outstanding requests. The client
performs native file access and returns a result to the session.

A save captures a particular document checkpoint. If painting continues during
the write, completion marks only that checkpoint as saved. Later edits remain
unsaved. Undoing back to a saved state can clear the modified indicator; comparing
only a monotonically increasing revision number would get that case wrong.

Opening first validates the project and prepares its GPU dependencies. The host
must preserve the existing document if that work fails. Closing coordinates Save,
Discard and Cancel with any pending edits or write operation.

The policy is shared, but the transports differ. GTK uses native file dialogs and
local atomic replacement; Android uses document providers; Apple uses native file
services. Windows implements native project dialogs and PNG export, while the
web client uses browser file access and downloads. See the
[platform guides](../platforms/README.md) before assuming a workflow is available
or fully validated on a particular client.

Some hosts also keep recovery copies independently of manual saves. Apple’s
[private recovery implementation](../../apps/layer-apple/PERSISTENCE.md) keeps
recovered artwork unsaved until the user completes a manual save.
