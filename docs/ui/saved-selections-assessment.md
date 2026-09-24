# Selection Layers and Quick Mask design

Date: 2026-09-22. Status: agreed direction, incorporated into the
[paintable-selection specification](paintable-selection-proposal.md).
The temporary Quick Mask row and independent Selection Layers with explicit
save/edit/load/replace behavior are in scope. The deeper editing operations
identified below remain follow-up proposals. See the
[selection command inventory](selection-command-inventory.md) for menu placement,
existing implementation, and delivery priorities.

## Three different jobs

| Feature | Job | Lifetime |
| --- | --- | --- |
| Quick Mask | Edit the current selection using drawing tools and grayscale coverage. | Temporary editing mode; completed coverage edits remain in the current selection. |
| Saved selection | Retain a named region for reuse, combination, or later refinement. | Document data stored in the editable project. |
| Layer mask | Continuously control the visibility of a particular layer or group. | Attached to that artwork object; affects composition. |

A saved selection does not hide artwork, clip a layer automatically, or become
active merely because its thumbnail is visible. A current selection can be used
to create a layer mask through the existing layer-mask commands.

## Quick Mask decisions to retain

- White selects, black removes, and gray specifies intermediate coverage.
  Displaying Selected instead of Protected changes the tint only. Keep this
  stable relationship even though other mask interfaces may couple display
  polarity and painting meaning.
- Entry with no selection starts from full coverage as specified, with black
  painting protecting areas. This is a product choice, not a universal Quick
  Mask convention. Entry with an explicit empty selection starts at zero.
- Preserve the original selection on an unchanged entry/exit round trip. A
  temporary editing mode must not silently create a persistent selection asset.
- Overlay opacity is independent of mask strength. Save the coverage and exclude
  the overlay from artwork/export. Saving should not unexpectedly end the mode.
- Make the active editing target and exit action visible. The user must be able
  to tell whether a brush will change artwork or selection coverage.

## Agreed Layers panel presentation

For familiarity with layer-oriented drawing workflows, entering Quick Mask
shows and selects a **Quick Mask** row at the top of the Layers panel. Give
it a normal coverage thumbnail, paintbrush target indicator, and compact Load
icon. Pin it outside artwork groups; it represents the global selection being edited.

This row is a view of the current selection and editing session, not a new
artwork layer. It does not blend, clip other layers, participate in merge/flatten,
or appear in exports. Its visibility control shows/hides the overlay without
exiting mask editing; the editing-target indicator remains visible when hidden.
Do not expose artwork opacity/blending controls or allow the temporary row to
be reordered into a group.

Exiting Quick Mask removes the row and restores artwork editing with the
resulting selection. Following the current specification, selecting an artwork
row finishes Quick Mask and selects that artwork target. Completed edits remain;
an unfinished contact is cancelled. Opening/closing the temporary row is not
another undoable layer insertion/deletion.

Provide **Save as Selection Layer…** to create a persistent snapshot. Saving
exits Quick Mask and activates the saved layer for painting. The copy is
independent of the current selection.
The Layers row supplements the existing menu/button/exit indicator, so artists
can still work when the Layers panel is closed or in a drawer.

## Further Quick Mask editing: follow-up proposals

General mask editing also benefits from restricting an edit with a lasso or
rectangle, softening mask edges, and moving or transforming mask coverage. The
initial scope exits Quick Mask when choosing a selection-construction tool and
disables artwork filters/transforms. That is a limited first implementation,
not the full set of operations an editable grayscale mask could support.

Recommended next refinements:

1. Add a temporary **edit region inside Quick Mask**. Rectangle/lasso restrict
   where subsequent painting, fills, and gradients affect the mask. This requires
   a separate transient clip: the mask being edited cannot also act as its own
   editing restriction. Display that clip as an outline over the colored mask.
   Clear it on exit. Make commands distinguish clearing the edit region from
   clearing the actual selection coverage.
2. Add **Feather Mask…** to soften coverage with a live preview and one undo step,
   sharing the scalar operation behind Modify → Feather. It never routes through
   an artwork filter layer by accident.
3. Add mask-only move/transform after the selection-target transaction and
   coordinate contracts are stable. Keep all artwork stationary while adjusting
   the mask; cancellation restores the original coverage and placement.

These refinements need explicit target routing and command availability. Do not
enable every filter or transform simply because it accepts image data. Gradient
masking, a bounded fill, and softening cover useful early workflows without
exposing the entire artwork effect system.

## Agreed model: independent Selection Layers

Present persistent saved regions as **Selection Layer** rows in the existing
Layers panel.
Each has a name, coverage thumbnail, selection icon, and an explicit Load
Selection action. They are document nodes with a coverage payload, excluded
from artwork composition. Do not expose blend modes, artwork opacity, clipping
relationships, or export visibility for them.

Allow Selection Layers at the document root or inside ordinary groups. Do not
require attachment to a paint layer. A Character group can contain Hair and
Skin selection layers alongside its Ink, Color, and Shading layers. A global
Sky selection can remain at the root. Placement determines organization and
group geometry, not which artwork layers a loaded selection can affect.

Mandatory attachment makes regions spanning multiple paint layers awkward and
couples their survival to an arbitrary owner. Grouping supplies useful local
organization without introducing another kind of parent/child relationship.
An explicit link to an individual artwork layer can be a later extension, with
separate transform, duplication, deletion, and reparenting rules.

### Familiarity with channel-based selection workflows

For users accustomed to temporary mask channels and document-wide saved alpha
masks, the recommended presentation changes location more than editing meaning.

| Decision | Familiarity assessment |
| --- | --- |
| Show a temporary Quick Mask row in Layers | The temporary named mask is familiar; its location among layer rows is new. Pin it outside groups; its Quick Mask name and removal on exit identify the temporary editing mode. |
| Independent persistent Selection Layers | Closer to document-wide saved masks than mandatory attachment to a paint layer. Root-level placement is the most direct mapping. |
| Attach each saved region to an artwork layer | Adds an unfamiliar ownership rule and risks confusion with a layer visibility mask. Keep this optional future behavior. |
| Select a saved row to edit its coverage | Familiar separation between editing a stored mask and using that mask as a selection. |
| Explicit Load Selection / modifier-click | Familiar way to apply a stored region without treating its editing target as artwork. Provide Ctrl-click on Windows/Linux and Command-click on Apple thumbnails, alongside a visible load action for touch. |
| Eye controls the overlay | Familiar distinction between seeing stored mask coverage and activating a selection. |
| Saved regions move with their parent group | A deliberate extension beyond independent document-wide masks. Make grouping an explicit choice; never parent a saved region implicitly just because the current artwork layer belongs to a group. |

Default the general Save as Selection Layer command to document-root placement.
Creating inside a group through an explicit group action, or moving the saved
row into a group, opts into that group's geometry and lifetime. This keeps the
simple save/recall workflow document-wide while allowing artists to organize
character-local regions with their artwork.

| Action | Required behavior |
| --- | --- |
| Save as Selection Layer… | Create a named snapshot of the current coverage, including a completed Quick Mask result. Exit Quick Mask and activate the new saved layer. |
| New Selection Layer… | Create an empty saved mask, select it for editing, using the current global mode (initially Paint selection). Default to the root unless invoked explicitly for a group. |
| Select row | Make that saved mask the editing target. Drawing changes the stored Selection Layer directly; show its name in the editing-target indicator. |
| Load Selection | Copy the stored coverage into the current selection and restore the most recent valid artwork editing target. |
| Add / Subtract / Intersect | Combine a saved mask with the current selection using the existing selection Boolean operations, then return to artwork editing. |
| Replace from Current Selection | Explicitly replace the chosen saved layer; retain its stable identity and name. |
| Rename / Duplicate / Delete | Ordinary undoable document edits. Deleting a Selection Layer leaves an already loaded current selection intact. |
| Show/hide overlay | Preview the stored region without loading/deselecting it or affecting artwork visibility. |

Keep row editing and Load Selection distinct. Choosing a row does not replace
the current selection; loading does not leave the saved mask as the painting
target. Editing a Selection Layer uses the mask editing controls and shared
coverage engine without creating a second temporary Quick Mask row. Leaving it
for another layer retains the saved edits and automatically hides its overlay.
Activating it again shows the overlay; target navigation has no undo step.

For example: save a region as Hair, then choose an artwork layer and continue
drawing. Select the Hair row to improve the stored mask. Choose Load Selection
to paint artwork through the revised region. Alternatively load Hair and refine
the current selection in Quick Mask: that edits only the working copy until
Replace from Current Selection explicitly updates Hair.

Only one current selection limits artwork edits. Multiple saved entries are
independent recipes that can be loaded or combined; they are not simultaneously
active layer masks. Previewing a thumbnail does not enable clipping or render
the stored region into the artwork.

At the root, saved regions use document coordinates. In a group, their geometry
follows that group: moving or transforming Character also moves its Hair region.
Moving a sibling paint layer independently does not move the region. Group
duplication/deletion includes its Selection Layers. Reparenting preserves visible
document-space placement. Document-wide geometry changes transform/crop them
with the document. Loading always resolves their current placement into a
document-space selection snapshot; later saved-layer edits do not mutate that
snapshot or the selection captured by earlier artwork strokes.

## Code and persistence consequences

The current [Document](../../crates/layer-core/src/lib.rs) has one current
selection and no persistent Selection Layer kind. The current
[selection representation](../../crates/layer-core/src/layers.rs) already retains
immutable pixel/contour coverage, affine placement, and inversion. Use that
representation as a persistent selection node's payload; it is not a paint
raster or a mask that controls an artwork layer's visibility.

Extend the document tree and shared row model with a typed selection node,
stable identity, name, coverage, parent group, and placement. Extend editing
targets to distinguish artwork, layer masks, the current Quick Mask, and stored
selection coverage. Thumbnail caches and overlay visibility are UI state, not
authoritative coverage. The temporary Quick Mask row uses a separate session
identity and does not allocate a durable document layer on every toggle.

Creating, painting, replacing, renaming, duplicating, reordering, and deleting
Selection Layers must mark the project modified and participate in document undo. Loading
uses the existing current-selection edit policy, which does not itself mark
artwork unsaved. Editing stored coverage changes document data without changing
the composited artwork. Update the engine's change classification accordingly.

Extend [project serialization and validation](../../crates/layer-core/src/project.rs)
for selection nodes, unique IDs, valid parenting, names, and aggregate coverage
size. Preserve them in .capy and recovery snapshots; flattened image exports
contain only artwork. Imported formats must not claim selection-asset support
without explicit import/export implementation.

Loading, saving, and duplication can initially share immutable coverage through
Arc. Later edits publish a new snapshot. This avoids immediate full copies in
memory, but does not guarantee sharing in the file format or between changed
snapshots. Many large saved masks add to the existing selection history/readback
budget; profile both project size and peak memory before committing to storage.

This layer-oriented workflow adds tree/host work beyond a separate saved-mask
list. Audit group operations, merge/flatten, automatic paint-target fallback,
export, transforms, and row controls so selection nodes cannot be treated as
paint layers accidentally. Share coverage rendering and history with Quick Mask;
do not implement another brush engine for persistent selection nodes.

## Acceptance for the agreed scope

- Quick Mask shows one selected temporary row, including in retained drawers;
  exit removes it without a layer-history entry or a leaked target.
- Save/load soft and transformed coverage without changing artwork pixels;
  one undo step for each saved-mask edit and each load/combine.
- Painting a Selection Layer changes that stored mask. Editing a loaded working
  selection leaves its source unchanged; replacing/deleting the saved source
  does not invalidate the working copy.
- Save/reopen and recovery preserve names, IDs, order, coverage, and inversion;
  older projects open without selection nodes; exports omit saved masks.
- Keep row editing, Load Selection, context menus, name editing, and preview
  distinct on mouse, touch, and pen. Reordering follows the existing
  [list-row and handle convention](drag-and-reorder.md).
- Test queued Quick Mask completion before Save/Replace/Load, document
  switching, undo back to a saved checkpoint, document-geometry changes, and
  aggregate memory/file-size limits across many masks.
- Group move/transform/duplicate/delete, independent sibling movement, reparenting
  with placement preservation, hidden groups, merge/flatten, and fallback when
  the last artwork target is deleted must handle selection nodes explicitly.
