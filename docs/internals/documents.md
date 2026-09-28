# Documents and edits

[Technical documentation](../README.md) · [Architecture](../architecture.md)

The document records enough information to reconstruct an editable drawing. It
is separate from the GPU textures used to display it and from the workspace used
to edit it. This distinction matters when implementing undo, saving a project or
recreating a renderer.

## Artwork and history

[`Document`](../../crates/layer-core/src/lib.rs) contains the layer structure and
its drawing data. Paint layers contain strokes and ordered operations; image
layers refer to source assets; groups organize child layers. Layer properties
control visibility, opacity, blending, clipping and related composition behavior.
[Layer definitions](../../crates/layer-core/src/layers.rs) also describe masks and
selection coverage.

With a filter selected, a stroke paints the first artwork layer below it in the
same group, or its clipping base; groups, paper, locked bases and selection
layers refuse with a reason (`Document::try_drawing_target`). Paper and the last
layer can be deleted; an empty stack stays valid.

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

### Merging layers

Merge Down, Merge Group, Merge Visible, Flatten Image and Stamp Visible, in the
Layer menu and the layer's menu, replace layers with one new paint layer in one
undo step ([`merge.rs`](../../crates/layer-core/src/merge.rs)). The new layer has
full opacity, Normal blending and no mask. It takes the place, name and clipping
of the layer it replaces, and references move to it. Placed photos become
ordinary document pixels.

- **Merge Down** merges the active layer into the artwork layer below it in its
  group. Both must be visible, unlocked and Normal, and the layer below must hold
  pixels: not the paper or an adjustment. A clipped layer below takes only a
  layer clipped to the same base. A clipping base instead merges its visible
  clipped layers into itself (Merge Clipped Layers), as does an adjustment
  clipped to a base. An unclipped adjustment applies to the layer below only
  (Apply Effect to Layer Below), so whatever else lies below no longer takes it.
- **Merge Group** composites a visible group, with its mask, into one layer that
  keeps the group's blend mode and opacity. Hidden layers inside are discarded. A
  Pass Through group is composited isolated into a Normal layer, as in Photoshop,
  so layers inside that blended with those below can look different.
- **Merge Visible** composites the visible layers over transparency. Hidden
  layers stay; hidden layers clipped to a merged base are released. The paper
  is not part of the composite, so a non-Normal layer can look different where
  the paper shows through it.
- **Flatten Image** does the same, then discards hidden layers and pixels
  outside the canvas. When it would discard hidden layers, it asks first through
  the canvas notice. The paper stays separate.
- **Stamp Visible** adds the visible image as a new top layer and keeps every
  layer.

Locks block every merge except Stamp Visible, and a group that holds Selection
Layers must give them up first. A merge is refused with a reason when its result
would exceed the 1 GiB publication limit or its undo step would not fit the
history budget. Merge Down, Merge Group and Merge Visible keep pixels outside the
canvas: the result's extent is the union of the merged layers' extents, on whole
tiles from the canvas origin. A merge that includes an effect layer covers the
canvas only, because effects are defined over the canvas.

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
