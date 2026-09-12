# Layer context-menu audit

> Historical design or validation record. Statements about completion and remaining
> work describe the recorded checkpoint. Start with the [current technical guides](../README.md).

Reviewed 2026-09-09. Target: draw each element on a regular layer, protect its
alpha, shade on clipped layers, and mask existing texture/paint to a selection.
GTK is the review frontend; command models and behavior are shared Rust.

## Evidence and limits

Photoshop and CSP do not have one invariant layer context menu. Commands depend
on layer type, selection, the clicked thumbnail/eye, edition and version. This is
a command-family inventory across row, thumbnail and palette menus—not a claim
to have executed every item in both proprietary applications. Current official
guides establish behavior. CSP's older complete [Layer menu index](https://www.clip-studio.com/site/gd_en/csp/userguide/csp_userguide/500_menu/500_menu_layer.htm)
is only a completeness cross-check for categories, not current menu placement.
No third-party code, icons or screenshots were imported.

## Inventory and decisions

“Added” means implemented in this pass. “Existing” means available in the current
GTK review build. “Gap” is not implemented; no placeholder command implies it is.

| Action family | Industry behavior / evidence | Capy decision and status |
| --- | --- | --- |
| Create / rename / duplicate / delete | Both have basic layer management; Photoshop also exposes naming and deletion through multiple surfaces. [Adobe management](https://helpx.adobe.com/photoshop/using/layers.html) | **Existing**, plus **added** context-menu inline Rename and atomic multi-layer duplicate/delete. |
| New folder; group selection; ungroup | CSP distinguishes removing a folder from deleting its contents. [CSP folders](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_folders.htm) | New group **existing**. **Added** Group selected layers and safe Ungroup; explicitly label destructive group deletion. |
| Reorder / move into group / expand folders | CSP offers dragging and ordering commands. [CSP folders](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_folders.htm) | Single row/subtree dragging, group disclosure and existing raise/lower commands. Multi-row dragging and extra ordering submenu deferred. |
| Select multiple / select all / clear row selection | CSP has separate editing and checked-layer roles. [CSP basic operations](https://help.clip-studio.com/en-us/manual_en/180_layers/Basic_operations.htm) | Checkboxes **existing**; **added** select-all/clear and context activation that preserves an existing checked set. |
| Visibility / parents / isolation / restore | CSP's eye menu includes parent visibility and selected-layer isolation. [CSP visibility](https://help.clip-studio.com/en-us/manual_en/180_layers/Basic_operations.htm) | Eye and single-layer solo **existing**. **Added** parent reveal, show all, and isolate checked layers with restore. Clipping bases remain present during isolation. |
| Alpha lock / editing lock | Both separate transparency protection from broader locking. [Adobe locks](https://helpx.adobe.com/photoshop/using/moving-stacking-locking-layers.html), [CSP layer settings](https://help.clip-studio.com/en-us/manual_en/180_layers/Other_layer_settings.htm) | Both **existing**. Extra position-only/pixel-only lock variants deferred. |
| Reference / draft | CSP exposes reference and draft roles, including reference folders. [CSP palette](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) | Reference designation **existing**; **added** group references and permission to mark locked linework. Draft/export-exclusion role deferred. Reference-aware bucket fill remains a separate **gap**. |
| Clip / release clipping; new clipped paint | Both use lower-layer coverage for clipped shading. [Adobe clipping](https://helpx.adobe.com/photoshop/using/revealing-layers-clipping-masks.html), [CSP layer settings](https://help.clip-studio.com/en-us/manual_en/180_layers/Other_layer_settings.htm) | **Existing**. Batch organization must retain complete stacks; deleting a base cannot silently attach survivors to unrelated paint. |
| Opacity / blend / advanced blending | Photoshop separates overall opacity, fill opacity and advanced blend controls. [Adobe blending](https://helpx.adobe.com/photoshop/using/layer-opacity-blending.html) | Overall opacity and seven blends **existing** in the header. Fill opacity, Blend If, knockout and additional blend modes deferred. |
| Add mask; reveal/hide selection | Both can create masks from selections. [Adobe mask creation](https://helpx.adobe.com/photoshop/desktop/create-masks/layer-masks/add-layer-masks.html), [CSP masks](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_masks.htm) | Selection-aware Add mask **existing**. **Added** explicit reveal-selection and hide-selection choices, including replacement of an existing mask. |
| Select content/mask; link/unlink | Separate mask targets and linkage are fundamental to editing/movement. [CSP masks](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_masks.htm) | **Existing** via thumbnails/link button and menu; content menu now gives access to the complete mask submenu. |
| Disable / inspect / invert / reveal-all / hide-all | CSP documents disabling and purple masked-area inspection; Photoshop provides grayscale mask editing. [CSP masks](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_masks.htm), [Adobe mask editing](https://helpx.adobe.com/photoshop/using/editing-layer-masks.html) | **Existing**. Preserve CSP-style alpha painting: colored paint reveals; erase hides. Inspection remains target-scoped and absent from exports. |
| Copy / move a mask between owners | Photoshop supports dragging masks between layers and modifier-drag copying. [Adobe mask creation](https://helpx.adobe.com/photoshop/desktop/create-masks/layer-masks/add-layer-masks.html) | **Added** Copy mask / Paste mask / Replace with copied mask. Copy then delete covers moving without an additional gesture. Session clipboard, not OS image clipboard. |
| Apply / delete mask | Applying changes paint; deletion restores unmasked content. [CSP masks](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_masks.htm) | **Existing**, separate final menu section. Apply is restricted to enabled paint-layer masks; group mask baking is a **gap**. |
| Selection from paint/mask: replace / add / subtract / intersect | Both can derive selections from existing coverage. [Adobe selection loading](https://helpx.adobe.com/photoshop/using/load-selections-layer-mask-boundaries.html), [CSP selection from layer](https://help.clip-studio.com/en-us/manual_en/330_selection/Selection_from_layer.htm) | **Significant engine gap.** Current selections are polygon contours, not arbitrary painted alpha. Lasso, inversion, fill and selection-to-mask are existing; Copy mask addresses mask reuse without pretending to extract a pixel selection. |
| Merge down / selected / visible / new composite / flatten | Both expose destructive and retained-source compositing workflows; CSP also has transfer-to-lower. [Adobe management](https://helpx.adobe.com/photoshop/using/layers.html), [CSP merge operations](https://help.clip-studio.com/en-us/manual_en/180_layers/Basic_operations.htm) | **Significant engine gap.** Grouping is not a substitute. Requires GPU bake/snapshot history and defined handling of masks, clipping, blends and wet pigment. No fake merge entry. |
| Clear paint / delete empty or hidden | Bulk cleanup appears in both applications. [Adobe management](https://helpx.adobe.com/photoshop/using/layers.html), [CSP deletion](https://help.clip-studio.com/en-us/manual_en/180_layers/Basic_operations.htm) | **Added** Clear layer, retaining its identity/properties/mask. Bulk empty/hidden cleanup deferred; checked-layer delete supplies explicit user-controlled cleanup. |
| Color labels / thumbnail display / filtering | Photoshop exposes color assignment and panel display controls. [Adobe management](https://helpx.adobe.com/photoshop/using/layers.html), [Adobe Layers panel](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/get-started-layers/work-with-the-layers-panel.html) | Useful for large documents, but not required for the masking journey. Keep compact fixed thumbnails and meaningful names for this review. |
| Layer styles / copy-paste effects / rasterize styles | Photoshop supports layer effects and reusable styles. [Adobe effects](https://helpx.adobe.com/photoshop/using/layer-effects-styles.html) | Deferred; the engine does not yet model these effects. |
| Smart/file objects / rasterize / convert type | Both have conversion families; CSP also has specialized line/tone conversion. [Adobe Smart Objects](https://helpx.adobe.com/photoshop/using/create-smart-objects.html), [CSP menu index](https://www.clip-studio.com/site/gd_en/csp/userguide/csp_userguide/500_menu/500_menu_layer.htm) | Deferred. Imports are editable paint assets, not pretend smart objects. |
| Adjustment / solid-fill / gradient / vector / text layers | Specialized source types introduce their own commands. [Adobe adjustment/fill](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/work-with-adjustment-and-fill-layers.html), [CSP menu index](https://www.clip-studio.com/site/gd_en/csp/userguide/csp_userguide/500_menu/500_menu_layer.htm) | Deferred; regular paint plus alpha lock and attached masks is the agreed initial model. |
| Rulers / frames / animation-specific operations | CSP has type- and edition-specific layer operations. [CSP menu index](https://www.clip-studio.com/site/gd_en/csp/userguide/csp_userguide/500_menu/500_menu_layer.htm) | Outside this illustration-layer milestone. |
| Export layer assets / copy CSS | Photoshop supports layer-oriented asset workflows. [Adobe management](https://helpx.adobe.com/photoshop/using/layers.html), [Adobe CSS](https://helpx.adobe.com/photoshop/using/copy-css-shape-or-text.html) | Composite export remains existing; layer export and design/developer output are deferred. |

## Implemented menu layout

- Creation: paint, clipped paint, group.
- Organization: Rename, duplicate (checked set when applicable), Group selected,
  Ungroup for groups.
- Protection: Alpha lock, editing lock, clipping, reference designation.
- Focused submenus: Mask, Selection, Visibility; Move layer/mask.
- Destructive final section: Clear paint, delete layer/group/checked set.

Mask menus keep edit/inspect/link controls first, selection initialization next,
copy/paste and coverage changes next, then Apply/Delete. Native GTK separators,
checkmarks and disabled styling are used. Unsupported command families are not
shown. Header controls still act on the editing target; explicit bulk commands
act on checked layers. Selecting a parent and its child processes that subtree
only once.

## Safety and implementation

Organization is an atomic document edit, not a GTK implementation. Grouping
requires neighboring siblings and whole clipping stacks. Groups remain isolated
Normal groups, so grouping can change blending against an outside backdrop.
Ungroup is deliberately restricted to unlocked, unmasked, full-opacity Normal
groups with Normal direct children; otherwise removing isolation/effects could
change the artwork. Delete protects locked descendants, clipping bases and the
last paint layer. Reusing a locked source is allowed; changing it is not.

Mask copying shares immutable stroke points, allocates independent mask/stroke
identities, and preserves canvas-space positioning across translated groups.
It neither captures a thumbnail as a mask nor downloads the canvas. Clear paint
invalidates imported-image GPU state and thumbnail revisions as well as strokes
and operations; undo restores the original content.
Thumbnail requests wait until pending document edits have been submitted, so
old GPU pixels cannot be cached under a newly edited layer's revision.

Paper can be selected without becoming paintable, and exposes visibility/opacity
instead of invalid editing commands. Core insertion/movement keeps it beneath
all artwork. Selection checkmarks override editing/reference glyphs when another
layer is selected; selecting only the editing layer retains its pencil.

Rendering remains GPU-only. These commands add no per-dab branches, buffers or
readbacks. Scene rebuilding is existing document-edit work; it is not on the
steady-state drawing path. UI thumbnail readback is limited to visible rows.

## Next engine work, not hidden menu work

1. GPU bake/snapshot as an undoable document source. Start with merge-down and
   isolated-group flatten, then selected/visible variants; settle wet-paint
   drying and clipping-base semantics explicitly.
2. Immutable raster coverage in the selection model. Derive it from real paint
   or mask alpha; reuse it for selection arithmetic, painting constraints and
   mask initialization. Never substitute the original lasso for subsequently
   edited mask coverage.

These are the largest remaining gaps for the full journey. Reference-aware
Bucket Fill and project persistence for the new layer model are also still
listed in the [initial design's implementation status](layers-initial-design.md).

## Validation

Shared tests cover subtree normalization, atomic group/ungroup and undo,
translated mask positions, clipping-stack duplication/deletion, locks, reference
restoration and independent mask copies. The private-Wayland GTK review exercises
actual menu activation, inline rename, imported-image Clear/Undo, mask copy/paste,
group/ungroup, dark/light appearance and 110 additional layer rows. Generated
review PNGs stay ignored under `artifacts/ui/layers-gtk/`.

## Whole-row holds (2026-09-11)

The [2026-09-12 convention](../ui/drag-and-reorder.md) additionally requires pen
row-body dragging to wait for a hold, alongside touch. Mouse row-body dragging
and all explicit row grips remain immediate. The acceptance evidence below
predates that requirement; it does not establish pen hold gating. The convention
also supersedes mouse hold menus: only touch/pen holds open menus; mouse uses
secondary click. GTK/Web regression suites now enforce this distinction. See the
[implementation inventory](../ui/drag-inventory.md) for the remaining changes.

Web and GTK now allow holding row text, padding, thumbnails, mask/link controls,
selection/visibility controls, or the grip, then reordering with the same contact.
The menu closes when dragging starts and remains available when the hold is
released without dragging. Mask holds retain their mask-specific context.
Active name editing keeps its normal input behavior. Movement before a touch
hold completes still scrolls the list.

GTK groups the row's long-press gesture with its native drag source and claims
the contact only after the hold. Web retains a pending pointer and prevents
native touch panning only once the hold has won. Both keep the shared Rust layer
drop action and its single undo/redo step.

Validated with native Mutter input for GTK and Chrome mouse/touch input for Web:
all eight row regions, release without dragging, content/mask targeting,
cancellation, scrolling, and undo/redo passed. The existing Web long-press
workspace/grip regression suite also passed. Reproduce on a private display:

```sh
tools/performance/workspace-motion.sh gtk --layer-hold
tools/performance/workspace-motion.sh web --layer-hold
tools/performance/workspace-motion.sh web --long-press-drag
```
