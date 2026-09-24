# Selection command inventory

Date: 2026-09-23. Status: shared actions and GTK/Web/Android Core integration
implemented. Next/Later remain separate work.
Companion to the [paintable-selection specification](paintable-selection-proposal.md)
and [Selection Layer design](saved-selections-assessment.md).

This inventory covers creating, modifying, saving, recalling, displaying, and
using selections. It maps familiar operations to our existing UI rather than
copying every command from another editor. Repeated menu entries invoke the same
shared action, validation, and history path.

## Evidence and scope

The recurring expectations are basic selection lifecycle, additive/subtractive
construction, edge refinement, selection-only movement, selection from layer
coverage, and saved-mask recall. Official references describe these in
[selection menu documentation](https://docs.krita.org/en/reference_manual/main_menu/select_menu.html)
and a [second selection menu reference](https://docs.gimp.org/3.2/en/gimp-select-menu.html).
The latter also lists more specialized operations such as holes, paths, and
distortion; their existence does not make them initial requirements.

Named masks need separate edit and load operations, with replace/add/subtract/
intersect when loading. This is supported by the documented
[saved-mask workflow](https://helpx.adobe.com/photoshop/using/saving-selections-alpha-channel-masks.html)
and [channel context actions](https://docs.gimp.org/3.2/en/gimp-channel-dialog.html).
A layer-oriented workflow similarly provides persistent painted regions that can
be recalled without consuming the stored layer; see
[selection-layer editing and recall](https://help.clip-studio.com/en-us/manual_en/330_selection/Selection_Layers.htm).

Contextual canvas actions commonly mix operations on the boundary with operations
on artwork inside it. The documented
[selection launcher](https://help.clip-studio.com/en-us/manual_en/330_selection/Selection_Launcher.htm)
includes fill, clear, crop, transform, and copy/cut actions. Our placement and
priorities below are design recommendations, not claims that all four researched
applications use identical labels, shortcuts, mathematics, or menus.

| Mark | Meaning |
| --- | --- |
| Existing | Shared implementation exists. Reuse it, while checking host exposure and the new editing targets. |
| Core | Required command integration for the agreed Paint selection / Quick Mask / Selection Layer feature. New unless explicitly identified as an adaptation. |
| Next | Recommended next delivery for a complete everyday selection workflow; separate work from the agreed paintable-selection foundation. |
| Later | Useful specialist capability or extra UI, intentionally deferred. |

Do not ship disabled placeholders for Next/Later features. Disable a delivered
command when its current target/state makes it unavailable, with a useful reason.

## Terms and routing

- **Current selection**: the one document-wide coverage mask restricting artwork
  edits. No selection means unrestricted; an explicit empty selection blocks all
  artwork painting. Deselect and empty coverage are different states.
- **Quick Mask**: an editing mode for the current selection, with a temporary row.
- **Selection Layer**: named stored coverage, edited directly when its row is
  selected. Loading makes a working copy; it creates no live link.
- **Artwork layer mask**: coverage controlling the visibility of a particular
  artwork layer/group. It is not a Selection Layer.
- **Selected layer rows**: organizational multiselection in Layers. Label its
  commands explicitly, separate from pixel selection commands.

Every action has an explicit target: current selection, saved selection ID,
artwork ID, or artwork-mask ID. Opening a row menu captures that row's identity;
it never loads coverage or paints into a different target implicitly. Revalidate
the ID, document epoch, and applicable locks on invocation. If normal context
selection highlights a row, retain the previous artwork target for subsequent
Load Selection. Do not let a mask row become that remembered artwork target.

## 1. Select menu

Keep this the complete, discoverable home for current-selection commands.

| Group | Commands | Priority and meaning |
| --- | --- | --- |
| Lifecycle | Select All; Deselect; Invert Selection | Existing. Preserve soft coverage; inversion is `1 - coverage`. |
| Lifecycle | Reselect | Core. Restore the last deselected coverage in this document, including soft edges and placement. |
| Create | Rectangle; Ellipse; Lasso; Polygon; Auto Select; Color Select | Existing selection-tool commands. Preserve their settings. |
| Create | Paint selection | Core. Activates its own Add/Subtract controls. |
| Paint | Quick Mask | Core. Checkable mode toggle; explicit Exit Quick Mask while active. |
| From layer | Select Layer Opacity → Replace / Add / Subtract / Intersect | Core. Also available on artwork thumbnails. |
| From mask | Load Layer Mask as Selection → Replace / Add / Subtract / Intersect | Core. Also available on artwork-mask thumbnails. |
| Modify | Grow…; Shrink… | Core. Circular extrema, 1–128 image pixels, Apply/Cancel, one undo step; preserve soft values and use zero beyond the canvas. |
| Modify | Feather… | Next. Operate on the current result, not on the next gesture. |
| Modify | Border…; Smooth… | Next. Border creates a selected band; Smooth reduces small irregularities. |
| Geometry | Move Selection; Transform Selection… | Next. Affect coverage/placement only; artwork stays stationary. |
| Store | Save as Selection Layer… | Core. Named snapshot; default root placement; activates the new layer. |
| Recall | Load Selection… | Core. Named submenu with group path, Replace/Add/Subtract/Intersect, and Load Inverted Selection. Works when Layers is closed; Layers provides the coverage thumbnail. |
| Update stored | Replace Selection Layer from Current Selection… | Core. Explicit named destination submenu; never infer an overwrite from the last loaded source. Row context targets its own row. |
| Display | Show Selection Outline | Core. Same action and checked state as View. Never deselects. |

Use Grow/Shrink consistently; searchable aliases may include Expand/Contract.
Do not expose both wordings as separate operations. Feather means softening
coverage; it is distinct from smoothing the contour and from brush Hardness.
Grow, Shrink, Border, and Feather take image-pixel distances, not screen pixels.
Grow/Shrink apply after confirmation; future refinements should add live preview.
All need Apply/Cancel, one undo step, and explicit canvas-edge behavior;
do not accidentally binarize all soft coverage through the outline threshold.

In ordinary artwork mode, Invert remains unavailable when there is no current
selection, matching existing behavior. Reselect is enabled only when no selection
is active and a previous deselected snapshot exists. Store that snapshot per
document, not as a named asset; Deselect with nothing active does not replace it.
Make its relationship to selection undo/redo consistent and test it explicitly.

Save/Replace require current coverage. An explicit empty mask is valid. With no
selection, disable these actions instead of silently saving a full-canvas mask;
Select All or New Selection Layer makes that intent explicit. Quick Mask's working
coverage is available for Save even before the first stroke.

Load/Add with no current selection uses the source as the new selection.
Subtract/Intersect require a current selection; explicit empty counts as present.
Invert source affects only the incoming snapshot. All four modes preserve partial
values and use the existing Boolean rules, distinct from Paint selection's
per-stroke opacity accumulation. A no-op creates no history entry.

## 2. Canvas selection actions and optional action bar

Add a shared **Selection Actions…** menu reachable from the Select tool panel
and supported canvas context entry points. Touch/pen users must have the visible
menu entry; opening it must not require a stationary brush contact or interfere
with an active stroke. Native context triggers retain host input ownership.

Show these groups for a completed current selection, regardless of whether the
invocation point falls inside or outside it:

| Group | Items | Priority |
| --- | --- | --- |
| Selection | Deselect; Invert Selection; Quick Mask; Save as Selection Layer… | Existing/Core |
| Refine | Feather…; Grow…; Shrink…; Border…; Smooth… | Next |
| Position | Move Selection; Transform Selection… | Next |
| Artwork | Fill Selection; Transform Selected Pixels… | Existing behavior, with target-aware labels and validation |
| Artwork | Clear Selected Pixels; Clear Outside Selection | Next; new pixel operations |
| Clipboard | Copy; Copy Merged; Cut; Paste | Next for selection export/cut; existing Paste Image integration |
| New layer | Copy Selection to New Layer; Cut Selection to New Layer | Next; preserve document-space placement |
| Document | Crop Canvas to Selection… | Next; explicit document-wide command |
| Display | Show Selection Outline | Core |

Without a current selection, offer Select All, Reselect when available, Selection
Brush, Quick Mask, and Load Selection. Do not clear the selection merely because
the user opens a menu outside its boundary. During polygon construction, use
Complete Selection and Cancel Selection, not commands assuming a completed mask.

An optional floating **selection action bar** is Next UI work, not required for
the underlying commands. Suggested compact default: Deselect, Invert, Quick
Mask, Fill, More. More opens the same menu. Additional refinement/copy/transform
items become customizable when delivered. Avoid a second customization system;
reuse existing command inventory/workspace mechanisms. Provide a View toggle.

If the bar can be moved, use an explicit drag handle and the shared
[drag convention](drag-and-reorder.md). Its handle drags immediately after slop;
reorderable button bodies require a hold outside Customize Title Bar. Keep it
away from the contact and viewport edges, and do not move it during a stroke.

## 3. Tool controls

| Tool or target | Controls/actions | Priority |
| --- | --- | --- |
| Geometric/freehand selection tools | New / Add / Subtract / Intersect; anti-aliasing; incoming feather | Existing. Do not confuse incoming feather with Modify → Feather. |
| Rectangle/Ellipse | Free / Fixed Ratio / Fixed Size; dimensions; From Center | Existing; retain shape-specific visibility. |
| Polygon | Constrain Angles; Complete Selection; Cancel Selection | Existing. Escape/cancel retains the previously committed selection. |
| Auto Select/Color Select | Sampling source: editing layer / visible artwork / reference layers; relevant tolerance and region settings | Existing. Saved masks and overlays are excluded from artwork sampling. |
| Paint selection | Add / Subtract; Size; Hardness; Opacity; pressure-for-size option; overlay settings | Core. No New/Intersect row or generic feather toggle. |
| Quick Mask / Selection Layer editing | Supported brush, eraser, fill and gradient settings | Core. Painting convention and overlay settings stay in Properties; ordinary color controls provide swap/reset. |
| Mask editing indicator | Paintbrush in the active layer row; Quick Mask command checked while active | Core. Layer indicator remains visible when its overlay is hidden. |
| Select panel | Selection Actions… | Core. Keyboard-free access to the current-selection menu. |

Keep Paint selection's temporary Alt/Option mode swap and physical eraser rules
from the main specification. Do not apply thumbnail-load modifiers to canvas
gestures. Existing geometric Shift/Alt constraints need their own precedence;
adding a blanket Shift=Add rule would conflict with current shape constraints.
All persistent modes must remain selectable without a keyboard.

## 4. Temporary Quick Mask row

The row uses ordinary layer presentation, is pinned and selected while editing,
and is removed on exit. A compact Load icon ends editing.
Mode (Paint selection / Grayscale mask) and overlay color/opacity stay
in Properties; the eye controls visibility. Its context menu is short and
target-specific:

| Item | Behavior | Priority |
| --- | --- | --- |
| Exit Quick Mask | Keep completed edits, return to artwork, remove temporary row. | Core |
| Save as Selection Layer… | Create named snapshot, exit Quick Mask, activate the saved layer. | Core |
| Invert Mask | Invert current selection coverage; stay in the mode. | Core; reuse current-selection inversion |
| Select Entire Canvas | Set mask coverage to 1. | Core; adapt Select All |
| Clear Selection Coverage | Set mask coverage to 0. Does not exit or remove the selection restriction. | Core |
| Fill Mask | Paint across the document mask using the selected painting convention and paint opacity. | Core |
| Modify → Grow / Shrink | Target the working mask. | Core |
| Modify Mask → Feather / Border / Smooth | Other coverage refinements. | Next |
| Move / Transform Mask | Adjust selection coverage only. | Next |

Do not expose Rename, Duplicate Layer, Delete Layer, merge, blend mode, artwork
opacity, or group/reorder actions. Saving is the meaningful persistent-copy
action; Exit is the meaningful temporary-row removal action.

Select All/Invert from the main Select menu affect this working mask and retain
Quick Mask. Deselect explicitly exits Quick Mask and removes the restriction;
it must not be aliased to Clear Selection Coverage. Reselect is unavailable while
the Quick Mask working mask exists. Loading a saved selection ends Quick Mask
after draining completed edits, applies the requested combination, and returns
to artwork. Entering another construction tool also exits as specified.

## 5. Persistent Selection Layer row and thumbnail

Ordinary row activation edits stored coverage; the visible Load button loads it.
Its context menu and the Layer menu for this row use the same actions:

| Group | Items | Behavior / priority |
| --- | --- | --- |
| Edit | Edit Selection Layer; Return to Artwork when editing | Core. No change to the current selection. |
| Use | Load Selection; Add to Selection; Subtract from Selection; Intersect with Selection | Core. Update working selection, return to artwork, retain saved mask. |
| Use | Load Inverted Selection | Core. Invert a copy, not the stored mask. Equivalent to Invert source in Load dialog. |
| Update | Replace from Current Selection | Core. Explicitly overwrite this ID's coverage. |
| Mask | Invert Stored Mask; Select Entire Canvas in Mask; Clear Stored Mask; Fill Mask | Core. Durable edits to this saved mask; locks apply. |
| Mask | Modify → Grow / Shrink | Core. Same scalar operations, explicitly routed to the saved ID. |
| Mask | Feather / Border / Smooth; Transform Stored Mask… | Next. |
| Organize | Rename…; Duplicate; Delete; Lock Editing | Core adaptations of ordinary node actions. Rename/duplicate/delete must retain their distinct meanings. |
| Organize | Move Up / Down; Move into Group… / Move to Root; Group Selected Layers | Core integration with existing tree semantics and keyboard-accessible reorder. General creation defaults to root. |

Do not include artwork-only alpha lock, blend modes, clipping, reference-layer
sampling, Add Layer Mask, Apply Mask, or merge into artwork on these rows.
Overlay opacity must never appear as an ambiguous layer-strength slider.
Hide preview with a hidden parent group, but keep the stored region loadable.
If editing a hidden mask, retain the explicit target indicator and an accessible
way to reveal its overlay. Multiple visible previews remain display-only and
never combine the current selection automatically.

Use the ordinary duplicate/delete/group actions for multiple selected rows.
The initial mask-load/edit commands target one explicit row; do not silently
interpret row multiselection as a Boolean union. Multi-mask batch combination is
Later. Row checkboxes retain organizational multiselection semantics.

Current-selection commands remain explicitly named in Select. Invoking a
construction tool or a current-selection lifecycle operation from saved-mask
editing returns to artwork/current-selection work; it does not silently load the
saved row. Quick Mask edits the existing current selection. To refine a saved
mask as a temporary copy, Load it first. Stored-mask context actions stay on the
stored target. Save/Replace reads the current selection without changing targets.

## 6. Artwork rows, artwork-mask thumbnails, and Layers creation

These bridges prevent Selection Layers from becoming an isolated feature:

| Place | Items | Priority / scope |
| --- | --- | --- |
| Artwork row/thumbnail → Pixel Selection | Select Layer Opacity; Add Opacity to Selection; Subtract Opacity from Selection; Intersect with Layer Opacity | Core. Transparent pixels contribute 0; partial alpha gives partial selection. |
| Artwork-mask thumbnail → Pixel Selection | Load Mask as Selection; Add Mask to Selection; Subtract Mask from Selection; Intersect with Mask | Core. Read the mask's own stored coverage, even when the mask is disabled. |
| Artwork row → Layer Mask | Reveal Selection; Hide Selection; Replace Mask: Reveal Selection; Replace Mask: Hide Selection | Existing. Creates/replaces artwork visibility coverage; these are not saved-selection commands. |
| Artwork-mask context | Edit Mask / Edit Layer Content; Show Mask Area; Enable Mask; Link Mask to Layer; Copy / Paste Mask; Invert; Reveal All / Hide All; Apply / Delete Mask | Existing. Preserve these distinct visibility-mask operations. |
| Layers + / Layer → New | New Selection Layer…; Save Current Selection as Selection Layer… | Core. Blank creation enters stored editing; saving a snapshot also activates the new layer. |
| Group context → New | New Selection Layer in Group…; Save Current Selection in Group… | Core. Explicit opt-in to parent geometry/lifetime. |
| Layer row organization | Select All Layer Rows; Clear Layer Row Selection | Existing behavior with unambiguous labels. Separate from Pixel Selection submenu. |

For the initial Select Layer Opacity operation, use raw content alpha in the
layer's current document placement, before layer opacity, masks, effects,
clipping, or ancestor visibility. This is a specified product choice, not a claim
of identical behavior everywhere. Hidden artwork remains a valid source. Offer
the command for drawable layers with a well-defined content-alpha source;
group/effect-result coverage and combined selected-row opacity are Later.
Keep group menus useful without pretending a group has raw pixel alpha.

Layer-mask loading resolves its placement separately. The layer's underlying
alpha must not be multiplied into the mask merely because it is attached there.
After a thumbnail load, return to the remembered artwork target; if that is the
source artwork row itself, it stays the target. Layer-mask loading returns to
artwork content rather than leaving a visibility mask as the brush destination.

Existing Reveal/Hide Selection creation clears the current selection and enters
layer-mask editing. Preserve that behavior unless revised explicitly; populate
the reselect snapshot when it clears coverage. Loading coverage takes the other
direction and must not accidentally call this destructive replacement path.

## 7. Edit, Layer, Document, and View menus

| Home | Operation | Priority / key distinction |
| --- | --- | --- |
| Edit | Undo / Redo | Existing. Saved-mask edits dirty the project; current-selection edits follow existing transient-selection policy. |
| Edit | Fill Selection | Existing on editable artwork. In mask mode, explicit Fill Mask targets grayscale coverage over its full extent. |
| Edit | Clear Selected Pixels | Next. New selected-pixel erase operation with soft coverage. |
| Edit | Clear Outside Selection | Next. Erase through inverse coverage on the active artwork target. |
| Edit | Copy; Cut; Copy Merged; Paste | Next for selection clipboard export/cut. Paste Image exists; text fields keep native clipboard ownership. |
| Layer | Copy Selection to New Layer; Cut Selection to New Layer | Next. Single undoable operation; do not move contents to the origin. |
| Edit | Transform Selected Pixels… | Existing Scale/Rotate behavior when a selection and editable content exist. Keep separate from Transform Selection. |
| Edit | Stroke Selection… | Later. Paint an outline with width/alignment/brush settings; Border Selection changes coverage instead. |
| Document | Crop Canvas to Selection… | Next. Crop to the bounding rectangle of nonzero coverage; holes/soft edges do not erase artwork. Affects the whole document and stored masks. |
| View | Show Selection Outline | Core. Hides ants without changing coverage or editing target. |
| View | Show Selection Action Bar | Next, only when the optional bar exists. |

Outline visibility and mask-overlay visibility are independent display settings.
Hiding the outline keeps a visible selection-active indication in the Select
controls. Neither toggle disables clipping, changes mask values, or creates an
undo step. Mask editing already replaces ants with its coverage overlay; changing
the outline preference takes effect when ordinary selection display returns.

Copy/cut/exported clipboard images must exclude all selection overlays. Define
copy source explicitly: Copy reads active artwork, Copy Merged reads visible
artwork composition. Apply partial selection as coverage. Imported immutable
content can be copied; cutting must honor editability/rasterization policy.
Disable artwork clipboard/clear/transform commands in mask editing until an
explicit scalar-mask counterpart is supported; never fall through to artwork.

## 8. Shortcuts and non-keyboard access

Keep the existing remappable Ctrl/Cmd+A Select All, Ctrl/Cmd+D Deselect, and
Ctrl/Cmd+Shift+I Invert bindings. Add Ctrl/Cmd+Shift+D Reselect subject to the
existing conflict resolver. The researched applications disagree on Deselect/
Reselect defaults, so do not silently rebind existing users to another scheme.

Quick Mask uses Q; mask color swap uses X; Reset to Black/White is a visible
command with D as a proposed mask-mode default after conflict checking. Escape
cancels a contact/preview first, then exits Quick Mask while retaining edits.
Paint selection reuses existing size shortcuts and its specified Alt/Option
mode swap. Native text fields retain all editing shortcuts. Other selection
tools use Shift Add, Alt Subtract, Shift+Alt Intersect, and Ctrl/Cmd New when
held before the gesture. The operation is latched until completion; keys first
pressed during construction provide geometric constraints instead.

Apply these load gestures only to coverage thumbnails, with visible menu/button
equivalents:

| Thumbnail gesture | Action |
| --- | --- |
| Ctrl-click, or Command-click on Apple | Replace current selection from thumbnail coverage |
| Ctrl/Command + Shift-click | Add coverage |
| Ctrl/Command + Alt/Option-click | Subtract coverage |
| Ctrl/Command + Shift + Alt/Option-click | Intersect coverage |

Support saved-selection thumbnails, artwork-alpha thumbnails, and artwork-mask
thumbnails consistently. The source type changes what is read, not the load
operation. Distinguish these hit targets from row bodies, checkboxes, names,
eyes, and grab handles. Recognized drags suppress click/load on release.

All core operations need menu or native button access, accessible names, focus,
and shared checked/enabled state on GTK, Web, Android, macOS, iPadOS, and Windows.
Layer rows in retained drawers follow the same mouse/touch/pen context and
reorder rules as docked panels. Do not use a mouse hold to open a context menu.

## 9. Later operations to retain in the backlog

| Family | Operations and reason to defer |
| --- | --- |
| Extra coverage refinement | Sharpen/Threshold, Remove Holes, Remove Small Islands, Distort, rounded-corner conversion. Useful but less universal; each needs precise soft-mask semantics. |
| Image-aware refinement | Grow by Similar Color, Select Similar, edge snapping/refinement, subject/object detection. Distinct from geometric Grow; require sampling or analysis beyond a paintable mask. |
| Paths and vectors | Selection to Path/Shape; Path/Shape to Selection; raster/vector selection conversion. Requires a compatible vector editing model. |
| Deeper mask editing | A temporary lasso/rectangle edit region inside Quick Mask or a Selection Layer. Separate clipping coverage is required; initial selection tools exit mask editing. |
| Stored-mask combinations | Add/Subtract/Intersect Current Selection into a stored layer, batch combination of several stored masks, cross-document transfer/import/export. Initial explicit Replace plus load/combine/save covers the main workflow. |
| More source types | Group/effect-result alpha, opacity union from multiple selected artwork rows, selection from luminance/color channels. Source contracts and compositing need separate design. |
| Advanced pixel operations | Paste Into/Outside, floating selections, content-aware/generative fill, layer effects from selection, specialized tone creation. Reuse future image-editing features rather than introduce them as selection prerequisites. |
| Attachment | Optional link between a saved region and one artwork layer. Requires transform/lifetime rules beyond explicit grouping. |

## 10. Existing code and implementation cautions

| Current code | Finding and consequence |
| --- | --- |
| [Application menus](../../crates/layer-ui/src/application_menu.rs), [command/menu definitions](../../crates/layer-ui/src/lib.rs) | Select currently exposes All/Deselect/Invert and six selectors. Add new shared actions; keep all native projections synchronized. |
| [Selection tools](../../crates/layer-ui/src/selection_tools.rs) | Tool modes, incoming feather, constraints, polygon completion exist. They are not completed-selection Modify commands. |
| [Session](../../crates/layer-ui/src/session.rs) | `SelectionVisible`, `SelectionEditing`, and `SelectionReference` select sampling sources. None is a Show Selection Outline toggle; do not repurpose these IDs. |
| [Layer menus/actions](../../crates/layer-ui/src/art_layers.rs) | The current Selection submenu mixes pixel actions with layer-row multiselection. Split the groups and add typed source/destination actions. |
| [Layer clear](../../crates/layer-ui/src/art_layers.rs) | `LayerAction::Clear` removes the entire paint layer's content. It must never back Clear Selected Pixels or a selection Delete shortcut. |
| [Transform operations](../../crates/layer-ui/src/operation.rs) | Existing Scale/Rotate transforms artwork or layer-mask content through the current selection. Transform Selection needs a separate coverage transaction. |
| [Clipboard/menu definitions](../../crates/layer-ui/src/lib.rs) | Paste Image exists; selection Copy/Cut/Copy Merged are additional work, not just missing menu entries. |
| [Region queue](../../crates/layer-ui/src/region_tools.rs), [selection refinement](../../crates/layer-render-wgpu/src/selection_refine.wgsl) | Ordered painted edits, completed-mask refinements, and saved-target IDs require integration; existing latest-request cancellation cannot discard completed strokes. |
| [Document](../../crates/layer-core/src/lib.rs), [project storage](../../crates/layer-core/src/project.rs) | Add persistent selection node/edit/storage support; a temporary Quick Mask row must not masquerade as one. |
| [Shortcuts](../../crates/layer-ui/src/shortcuts.rs), [customization](../../crates/layer-ui/src/customization.rs) | Register discoverable/remappable actions once, preserve user bindings, and respect text focus and host-reserved chords. |

Menu invocation after pointer-up must drain completed work in order. Save,
Replace, Load, Undo, transforms, document changes, and return-to-artwork cannot
race a pending coverage readback. If the destination/source is deleted, locked,
or invalidated while a menu/dialog is open, reject safely and retain the working
selection. Separate project-dirty edits, selection-only edits, and display state.

Before calling a delivery complete, verify soft/empty/inverted selections,
none-versus-empty command availability, hidden/locked sources and destinations,
menu target stability, keyboard-free workflows, loaded-copy independence,
one-step undo, save/reopen, and retained/native host projections. Existing/Core
entries form the initial surface; prioritize Feather/Grow/Shrink, selection-only
movement, and selected-pixel clear/copy next.
