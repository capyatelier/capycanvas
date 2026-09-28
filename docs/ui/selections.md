# Selections

[Workspace and UI](README.md) · [Canvas action bar](canvas-action-bar.md) ·
[Selection tools](selection-tools.md)

## Terms

- **Current selection:** the one document-wide coverage mask that restricts
  artwork edits. Coverage is a scalar from 0 to 1, and soft values survive every
  operation. No selection means unrestricted; an explicit empty selection blocks
  all artwork painting. Deselect and empty coverage are different states.
- **Quick Mask:** a mode that edits the current selection with drawing tools,
  shown as a temporary Layers row.
- **Selection Layer:** named stored coverage in the layer tree. Clicking its row
  edits it; loading it copies its coverage into the current selection, with no
  live link.
- **Layer mask:** coverage that controls one artwork layer's visibility. It is
  not a Selection Layer.

Every action has an explicit target: the current selection, a saved selection,
an artwork layer or an artwork mask. A row menu captures its row's identity and
never loads coverage or paints into another target implicitly.

## Select menu

The Select menu is the complete home for current-selection commands, in this
order: Select All, Deselect, Reselect, Invert Selection; Quick Mask, New
Selection Layer…, Save as Selection Layer…; Copy and Cut Selection to New Layer,
Clear Selected Pixels, Clear Outside Selection; Grow… and Shrink…; selection
from layer opacity or from a layer mask (Replace, Add, Subtract or Intersect);
Load Selection and Replace Selection Layer from Current Selection submenus; and
Show Selection Outline. [`application_menu.rs`](../../crates/layer-ui/src/application_menu.rs)
holds the order.

- Reselect restores the last deselected coverage of the document and is enabled
  only while nothing is selected.
- Grow and Shrink take 1–128 image pixels, apply after confirmation as one undo
  step, preserve soft values and treat pixels beyond the canvas as unselected.
- Save and Replace need current coverage; with no selection they are disabled
  rather than saving a full-canvas mask. Load and Add with no selection use the
  source as the new selection. Subtract and Intersect need a selection.
- A command that would change nothing adds no history entry.

## Moving selected pixels

With a selection, a Move drag moves the selected pixels of the active layer or
mask, as Photoshop's Move tool and Clip Studio Paint's Move Layer tool do. Photo
starts with Move, so dragging over a selection there moves its pixels. Without a
selection, Move moves the whole layer by changing its offset, which resamples
nothing.

- **Pen and mouse** start the drag at the press, anywhere on the canvas. A
  **finger** starts it on the selected area, the selection's bounds (outside
  them for an inverted selection); elsewhere a finger navigates. Neither waits
  for a hold.
- The pixels move by whole layer pixels, so their values are exact. A linked
  mask moves with its layer. Shift keeps the drag horizontal or vertical.
- Releasing applies the move as one undo step, and the selection moves with the
  pixels. Move stays the tool, and the selection bar keeps its context.
- **Leave Copy** (`MoveLeaveCopy`), in Move's Tool Options and on the selection
  bar while Move is active, keeps the original in place and places a copy. Alt
  held as the drag starts does the opposite of the toggle.
- Cancelling the contact, or losing window focus, leaves the pixels where they
  were. A press that moves no whole pixel changes nothing.
- Move refuses with a notice on a locked layer and on the paper. With a
  selection it also refuses on a group or effect layer ("Choose a paint layer or
  a mask to move selected pixels") and on a layer with no pixels.
- Moving pixels of a placed photo paints them over its original, which Revert to
  Original Photo brings back.

## Paint selection

Paint selection is a tool in the Select family. Its Tool panel has Add and
Subtract (Add first), Size (32 px), Hardness (100%), Opacity (100%), overlay
settings and Pressure for size (off). Its settings are separate from the
painting brush, and foreground color does not affect it. Alt swaps the mode for
one stroke, and a physical pen eraser forces Subtract. The mode is fixed at
pointer-down.

- A tap makes one dab and an open stroke paints its footprint. A stroke that
  crosses itself, or ends within 6 logical pixels of its start after leaving it,
  also fills the enclosed area. There is no separate lasso mode or Apply step.
- Only real input samples can close a loop; predicted samples only preview.
- Within one contact, overlapping dabs and enclosed areas take their maximum
  coverage B before opacity. With coverage S before the stroke, Add gives
  `S + (1 - S) * B` and Subtract gives `S * (1 - B)`, so repeated 50% strokes
  build up.
- Selected pixels are tinted red at 50% by default, scaled by coverage, while
  the tool is active. Other tools show the outline.
- Pointer-up commits one selection edit. Escape, cancellation and focus loss
  discard only the unfinished contact.

## Quick Mask

Quick Mask (Q) is a checkable command in Select. Entering keeps the current
coverage exactly, or starts from empty coverage when there is no selection;
leaving without an edit restores the exact original state. Entering and leaving
add no undo steps. It keeps a tool that can paint masks, otherwise it chooses
the last supported brush, and exit restores the previous tool, target and colors.

- A pinned **Quick Mask** row at the top of Layers shows the coverage thumbnail
  and a Load icon. Its eye hides only the overlay. The row cannot be renamed or
  moved, and exiting removes it.
- Quick Mask and Selection Layers share three Properties controls: Mode,
  Overlay color and Overlay opacity. In **Paint selection** mode (the default)
  any color adds coverage and transparency or erasers remove it. In
  **Grayscale mask** mode black protects, white selects and gray is partial.
  Mode is one application preference; overlay settings belong to each mask.
- Brushes, the eraser, Fill, Fill mask and Gradient edit coverage. Brushes that
  mix pigment, smudge, move fluid or deform are disabled with a reason, and
  artwork filters and destructive layer commands are unavailable until exit.
- Escape cancels an unfinished gesture, otherwise it exits Quick Mask and keeps
  completed edits. Choosing an artwork row, a Selection Layer or a selection
  tool also exits, as does Save as Selection Layer…. Switching documents
  exits; saving the project does not.

## Selection Layers

Selection Layers are document nodes at the root or in groups. They never render
as artwork, and they are excluded from sampling, effects and exports.

- **Save as Selection Layer…** snapshots the current coverage under a name,
  at the document root, and selects the new layer. **New Selection Layer…**
  starts an empty one.
- Clicking a row edits the stored coverage with the Quick Mask painting rules,
  without changing the current selection. A locked layer or group blocks edits
  but still allows loading and preview.
- **Load Selection** copies the coverage into the current selection and returns
  to the last artwork target; Add, Subtract and Intersect combine it instead.
  Ctrl-click (Command-click on Apple) on the thumbnail loads, with Shift for
  Add, Alt for Subtract and Shift+Alt for Intersect.
- **Replace from Current Selection** is the only way to write a loaded and
  refined copy back into a stored layer.
- Creating, painting, replacing, renaming, moving and deleting are document undo
  steps and mark the project modified. Loading changes only the current
  selection. Projects and recovery keep names, IDs, parents and coverage.
