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

Effect layers hold filters in the layer stack. An adjustment transforms the
combined image below it within its group; a clipped adjustment acts on its
clipping stack, preserving the base layer’s coverage. Multiple adjustments apply
in layer order, forming an effect chain. A generator effect instead produces
content that is composited as a layer. Masks and opacity control where and how
strongly an effect applies.

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
