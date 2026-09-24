# Paintable selection specification

Date: 2026-09-23. Status: Core implementation complete on GTK, Web, and Android.
Paint selection, Quick Mask with a temporary Layers row, and persistent
Selection Layers are in scope.

The [Selection Layer design](saved-selections-assessment.md) records the agreed
save/edit/load workflow, grouping rationale, and persistence requirements.
The [selection command inventory](selection-command-inventory.md) maps commands
to menus, tool controls, and layer rows, distinguishing required integration from
recommended follow-up operations. Deeper Quick Mask editing remains identified
separately rather than implicitly enabling every image operation on masks.

Provide two ways to edit the same document selection. **Paint selection** paints
or encloses areas with Add/Subtract controls. **Quick Mask** lets ordinary
coverage-producing drawing tools edit the selection as a grayscale image.
Both preserve partial coverage, use a live overlay, and share document undo.
**Selection Layers** store named regions for reuse. Editing a saved layer changes
that stored mask; loading it produces an independent current selection.

This document defines Capy Canvas behavior. Defaults, gesture recognition, and
coverage equations below are explicit product decisions for implementation and
prototype validation.

## Paint selection

### Placement and controls

Add **Paint selection** to the Select family's existing Tools → Tool drawer,
Select menu, and customizable tool inventory. The Select opener remembers it
and displays its icon through existing workspace tool memory. Sketch needs no
new top-level tool button. Use a shared SVG icon and native controls.

The Tool panel contains:

| Control | Behavior and initial value |
| --- | --- |
| Add / Subtract | Two mutually exclusive actions; Add initially. Remember this tool's mode separately from other selection tools. |
| Size | Round-tip diameter in image pixels, initially 32 px; use the existing brush-size range, editor, and size shortcuts. |
| Hardness | 0–100%, initially 100%. Controls the solid center and soft falloff. |
| Opacity | 0–100%, initially 100%. Controls how strongly each completed stroke changes selection coverage. |
| Overlay settings | Secondary popover for overlay color and opacity. These affect display only. |
| Pressure for size | Secondary setting, initially off. When enabled, use the existing pressure curve and brush-size dynamics. |

The cursor previews the brush diameter and includes a plus or minus indicator.
Size, hardness, opacity, and mode changes do not alter an existing selection.
Keep settings separate from the painting brush and its colors. Foreground color
does not affect Paint selection.

Do not attach the geometric tools' New/Add/Subtract/Intersect row, feather,
fixed-size, or anti-aliasing controls to this tool. Edges are anti-aliased;
Hardness provides local softness. Other selection tools retain their own modes
and settings. Starting over uses the existing Deselect command, followed by Add.

Both modes are available without a keyboard. Alt/Option temporarily swaps the
chosen mode for a new stroke; a physical pen eraser forces Subtract and takes
precedence over that modifier. Resolve the effective mode at pointer-down and
keep it for the contact. Release restores the chosen mode. Native text editing
and navigation shortcuts retain their existing priority.

### Brush and enclosing gestures

A tap produces one dab. An open stroke paints its brush footprint. A stroke
that encloses an area also fills that area, using the same Add/Subtract mode and
Opacity. There is no separate lasso-mode button or confirmation step.

Use these recognition rules:

- Build the path in document coordinates from real input samples. Predicted
  samples can preview the cursor/stroke but cannot establish a committed closure.
- A path section closes when its centerline crosses an earlier section, or the
  end returns within 6 logical viewport pixels of its starting point after
  leaving that start zone. The near-start case adds a short closing segment.
- Require a nondegenerate enclosed area; stationary samples and a straight
  out-and-back gesture remain brush strokes. Do not close an arbitrary open arc
  with a long line on release.
- Fill the enclosed path sections using the existing even/odd contour rule.
  Retain any open trailing stroke. Combine enclosed interiors and the brush
  footprint before applying opacity; crossing the boundary must not double it.
- Hardness softens the outside of the painted boundary. The enclosed interior
  receives the stroke's opacity throughout; pressure on size affects the
  boundary footprint, not the interior's opacity.
- Show the filled area as soon as closure is recognized. The preview and committed
  result must use the same rules, including self-crossing paths.

For example, brushing over a narrow detail selects the brushed strip. Circling a
larger shape selects its interior as well. Circling in Subtract removes the
enclosed area from the selection. Brush strokes do not inspect image colors or
snap to subject edges.

The 6-pixel closure tolerance is a prototype tuning value shared across hosts.
It is a canvas gesture rule, not the native hold/slop policy for reordering UI.

### Opacity and repeated strokes

Selection coverage is a scalar from 0 (unselected) to 1 (fully selected).
Let S be coverage before the stroke. Let B be the completed stroke's footprint,
including enclosed regions, multiplied by Opacity.

| Mode | Coverage after the stroke |
| --- | --- |
| Add | `S + (1 - S) * B` |
| Subtract | `S * (1 - B)` |

Within one contact, overlapping dabs and enclosed regions use their maximum
coverage before opacity. Lingering, crossing the same point, or receiving more
input events does not repeatedly apply opacity. Separate completed strokes
build up: two fully overlapping 50% Add strokes on zero coverage produce 75%;
two 50% Subtract strokes on full coverage leave 25%. Apply quantization only at
the committed coverage boundary and allow the corresponding byte rounding.

With no selection, Add begins from zero coverage. Subtract with no selection
does nothing. Select All followed by Subtract carves areas out of the canvas.
An unchanged result, including a zero-opacity stroke, creates no undo entry.

A present selection containing zero coverage remains an empty selection:
painting is blocked everywhere. **Deselect** removes the restriction entirely.
Never turn complete erasure into unrestricted painting automatically.

### Overlay and completion

While Paint selection is active, tint selected pixels red at an initial display
opacity of 50%, scaled by coverage. Offer alternate colors and display opacity
in Overlay settings. Keep those settings distinct from brush Opacity. Use a
display-space overlay with legible cursor contrast on light, dark, and HDR artwork.

The tint includes the current stroke and replaces marching ants during editing.
Switching to an ordinary tool restores the normal selection outline. Values
below the outline's approximately 50% threshold remain selected in proportion
to their coverage, even if no outline encloses them.

Pointer-up commits one undoable selection edit. Escape, pointer cancellation,
capture/focus loss, or an explicit tool change during a contact discards that
unfinished contact and retains earlier completed strokes. Once pointer-up has
occurred, a tool change must wait for/preserve that completion rather than
cancelling an asynchronous result. There is no Apply dialog.

## Quick Mask

### Entry and editing target

Expose a checkable **Quick Mask** command in Select and the customization
inventory, with Q as the default remappable shortcut. Publish the same checked
state to menus, toolbar buttons, and hosts. Respect shortcut conflicts and native
text-field ownership.

Entering Quick Mask finishes any already completed pending operation and
cancels an unfinished contact. Save the previous tool, artwork editing target,
and foreground/background colors for restoration. If the current tool supports
mask painting, retain it; otherwise activate the most recently used supported
brush, with a hard round brush as fallback. Preserve the original preset for exit.
The selected row carries the paintbrush target indicator. Its Load icon and
context menu provide a keyboard-free exit.

Initialize the working coverage as follows:

| Entry state | Working mask |
| --- | --- |
| Existing selection | Preserve its full coverage, including soft edges, inversion, and placement. |
| No selection | Empty working coverage. Opaque painting builds the selected region. |
| Explicit empty selection | Zero coverage; painting builds a selection from scratch. |

Entering and leaving without an edit preserves the exact original selection,
including the distinction between no selection and explicit full coverage.
Mode entry/exit does not create a document undo step.

The editing target is the global selection. It does not require a paintable or
unlocked active layer and never creates an artwork layer or a layer-mask item.
Painting into it is not clipped by its own current coverage.

### Temporary Layers row

Show and select a pinned **Quick Mask** row at the top of Layers, outside groups,
with the ordinary coverage thumbnail, paintbrush target icon, and compact Load
icon immediately to the right of the thumbnail. Avoid extra explanatory text. Its eye
controls overlay display only. Hiding the overlay leaves mask editing active and
the editing-target indicator visible. The row is a projection of the editing
session, not a durable layer insertion, and cannot be renamed or reordered.
The common Layers toolbar stays stationary when the target changes, with
unsupported artwork controls disabled. Mask settings live in Properties.
Do not expose artwork merge/export operations in the mask context menu.

Exiting removes the row without a layer-history entry. Choosing an artwork row
finishes Quick Mask and selects that target; choosing a Selection Layer finishes
Quick Mask and begins editing the saved mask. Preserve completed edits in both
cases. **Save as Selection Layer…** creates a named independent snapshot, exits
Quick Mask, and selects the new layer for painting. All these actions remain reachable
through menus and the editing-target controls when Layers is closed.

### Painting and display

Quick Mask and Selection Layers use the same three Properties controls:
**Mode**, **Overlay color**, and **Overlay opacity**. There is no mask editing
popup, strip, Done, or Swap property. The overlay picker and current-color bucket
work like Paper color; the bucket copies the displayed mask painting color.
Layer visibility controls the overlay.

Default **Paint selection** mode tints selected areas: any color adds selected
coverage; transparent color and erasers remove it. **Grayscale mask** mode tints
protected areas: black protects, white selects, and gray gives partial coverage.
Transparent color and erasers remove protection, selecting those pixels.
Changing mode couples painting meaning and overlay polarity without changing
stored coverage. Mode is one application preference for every mask, including
new masks and other documents. Overlay color and opacity stay per layer. Mask
colors remain independent of artwork colors and the wheel stays unrestricted;
painting converts their display-encoded sRGB luminance to coverage. Ordinary color-panel swap/reset actions
and shortcuts remain available.

For paint target G and effective brush alpha A, blend as `S * (1 - A) + G * A`.
In Paint selection, G is 1 for paint and 0 for erasing. In Grayscale mask, G is
the chosen gray, or 1 for erasing. Brush opacity, pressure, and tip shape determine
A. Physical pen erasers follow the same convention. Ordinary mask strokes do
not use Paint selection's automatic loop fill. Swept nibs publish every real
endpoint so large G-Pen brushes refresh during sub-spacing movement.

Overlay defaults to red at 50%; color and opacity affect display only. Quick
Mask remembers these properties for the document session. Selection Layers
persist their properties; saving a Quick Mask copies them. Activating a saved
mask shows it, and switching to another layer hides the previous saved mask.
This target navigation adds no undo step. Other explicitly visible saved masks
can still composite their tints in layer order without changing the selection.

### Supported operations

| Operation | Quick Mask behavior |
| --- | --- |
| Coverage-capable drawing brushes | Apply the preset's supported tip shape, texture, size, opacity, and pressure to grayscale coverage. |
| Eraser | Remove selected coverage using the eraser’s alpha. |
| Fill | Use the existing artwork sampling sources and region classification to find an area, then paint that area into the mask. Never classify the overlay or use the mask as its own clip. |
| Fill selection command | Present as **Fill mask** and apply the selected painting convention to the entire document mask, with the current paint opacity. |
| Gradient | Paint coverage or coverage-to-transparent using the selected painting convention and existing gradient geometry. |
| Pan, zoom, rotate/flip view | Preserve the editing mode and mask values. |
| Undo/redo | Undo/redo mask edits as selection edits, one step per completed gesture. |

Brushes requiring pigment mixing, smudging, fluid transport, or deformation are
unavailable in the initial Quick Mask implementation. Publish their disabled state
and explanation from shared Rust; never silently route them to artwork or
substitute a different material behavior. The chosen ordinary brush preset is
preserved for exit.

Choosing Paint selection or another selection-construction tool exits Quick Mask
and activates that tool on the resulting selection. Explicitly choosing a layer
or layer-mask editing target also exits, then applies that target choice.
Artwork filters, destructive layer commands, and content transforms are disabled
while Quick Mask is active. They become available normally after exit.

### Exit, history, and persistence

Q, the checked command, or Exit Quick Mask returns to ordinary selection display
and restores the previous tool/target/colors. An explicit tool or target choice
takes precedence over restoration. Exiting does not add an extra undo step;
completed mask edits are already selection edits.

Escape cancels an unfinished gesture. With no gesture pending, Escape exits
Quick Mask while retaining completed edits. Undo reverses those edits; Escape
does not silently discard the entire session. Focus loss cancels the active
contact but leaves the mode visible and active.

Before changing documents, cancel the unfinished contact, drain completed edits,
and exit Quick Mask. Returning to the document uses ordinary editing mode.
Saving drains completed edits and stores the resulting selection without exiting
the visible mode. Opening/recovering that snapshot starts outside Quick Mask;
the transient mode and temporary painting UI state are not project data.
Export and color sampling use artwork without the selection overlay.

## Selection Layers

### Save, edit, and load

Provide named **Selection Layer** rows with a coverage thumbnail, selection icon,
eye for overlay preview, and an explicit **Load Selection** action. Their coverage
does not render as artwork or continuously control another layer's visibility. Saving a
snapshot starts its overlay hidden; explicitly editing that row reveals it.
Visible saved layers use their own overlay color/opacity and selected/protected
setting. These settings affect display only.
There is still only one current selection restricting artwork edits.

| Action | Required behavior |
| --- | --- |
| Save as Selection Layer… | Snapshot current coverage, including completed Quick Mask edits. Ask for a name with an automatic default; activate the new layer while preserving the working selection. Default to the document root. |
| New Selection Layer… | Create an empty stored mask and enter Color / transparent painting. A group-specific creation command explicitly parents it to that group. |
| Click row / Edit Selection Layer | Edit stored coverage directly with the shared mask Properties and paintbrush row indicator. Preserve the current selection separately; it does not clip stored-mask painting. |
| Load Selection | Resolve the stored mask's placement, copy coverage to the current selection, and return to the last valid artwork editing target. Never consume or delete the saved row. |
| Add / Subtract / Intersect with Selection | Combine stored coverage with the current selection and return to artwork, using existing selection Boolean operations. |
| Replace from Current Selection | Explicitly replace the chosen saved payload, retaining its identity, name, and parent. Later current-selection edits do not follow through. |
| Rename / Duplicate / Delete | Durable undoable edits. Deleting a saved row leaves a previously loaded current selection intact. |
| Show overlay | Preview coverage only; never load, deselect, or toggle clipping. Overlay opacity is a display setting. |

Ctrl-click on Windows/Linux and Command-click on Apple thumbnails loads coverage;
the visible action provides the same operation without modifiers. Keep thumbnail
loading distinct from row selection, row multiselection, rename, and reorder.
The command inventory specifies combination modifiers and context menus.

Editing a stored layer uses Quick Mask's painting, brush, erase, fill, gradient,
and tool-availability rules, but commits to its stable saved-layer ID. It does
not insert a second temporary Quick Mask row. Leaving for artwork keeps completed
stored edits. Restore artwork tools/colors when leaving mask editing. A locked
saved layer or locked ancestor blocks changes to stored coverage, but permits
loading and preview. If the remembered artwork target has been deleted, use a
valid artwork target or require an explicit choice; never paint onto an arbitrary
mask or automatically create an artwork layer.

For example, save Hair, choose Highlights, load Hair, and paint through it.
Deselecting keeps Hair available for later. Click Hair to improve the stored
mask directly. Alternatively load Hair and refine the working copy in Quick
Mask; only **Replace from Current Selection** writes those changes back to Hair.

### Placement, lifetime, and persistence

Selection Layers are independent document nodes, allowed at the root or in
ordinary groups. They are not required to attach to a paint layer. General Save
never inherits a group just because the current artwork target belongs to it.
Explicit group creation or reparenting opts into group geometry and lifetime.

A saved mask follows its parent group's geometry, duplication, and deletion.
Moving a sibling paint layer independently leaves it unchanged. Reparenting
preserves document-space placement. Document resize/crop handles saved coverage
with the document. Loading resolves a document-space snapshot; later saved-layer
or group changes do not mutate the loaded copy or earlier strokes' captured clips.

Keep these nodes out of artwork composition, sampling, effects, and flattened
exports. Audit group visibility, clipping adjacency, merge/flatten, and automatic
paint-target fallback explicitly. Artwork merge/flatten must preserve saved
regions; if an operation removes a parent, retain their document-space geometry
when relocating them. Do not silently rasterize or discard them as image content.

Create, paint, replace, rename, duplicate, reorder, and delete are document undo
operations and mark the project modified. Load/combine changes only the current
selection and follows its existing undo/dirty policy. Save/reopen and recovery
retain stored names, IDs, parent/order, coverage, and placement in .capy. Extend
schema validation and memory limits; older projects have no saved selection nodes.
Other formats require explicit support before claiming this data is preserved.

## Shared implementation contract

Paint selection and Quick Mask edit one authoritative current selection.
Selection Layers hold independent immutable coverage snapshots. Use explicit
editing targets for artwork, layer masks, current Quick Mask, and saved selection
IDs. Display, editing target, and loaded current selection are separate state.

| Existing code | Integration |
| --- | --- |
| [Selection data](../../crates/layer-core/src/layers.rs) | Reuse contour/pixel coverage, inversion, affine placement, immutable snapshots, and 8-bit results. Normalize into document coordinates for a new painted result without modifying old snapshots. |
| [Selection tools](../../crates/layer-ui/src/selection_tools.rs), [session](../../crates/layer-ui/src/session.rs) | Add Paint selection identity, separate Add/Subtract settings, Quick Mask state, shared commands, control visibility, gesture lifecycle, and target restoration. Update geometric-tool guards explicitly. |
| [Stroke engine](../../crates/layer-engine/src/canvas.rs) | Reuse pen samples, pressure, stabilization, and dab placement. Route to explicit current/saved selection targets; neither uses the current selection as its clip. Current-selection edits bypass artwork locks; saved edits validate their own and ancestor locks. Commit to the captured target instead of layer raster edits. |
| [Mask renderer](../../crates/layer-render-wgpu/src/layer_masks.rs), [brush shader](../../crates/layer-render-wgpu/src/brush.wgsl) | Reuse coverage geometry and GPU page management. Add per-contact footprint accumulation for Paint selection and grayscale painting for Quick Mask. Existing layer-mask setup forces white, so it cannot provide grayscale behavior unchanged. |
| [Selection refinement](../../crates/layer-render-wgpu/src/selection_refine.wgsl), [region tools](../../crates/layer-ui/src/region_tools.rs) | Reuse coverage transport and validation; preserve geometric selection Boolean modes. The new brush blending equations are separate. Replace latest-request cancellation with ordered completion for successive paint strokes. |
| [Presentation](../../crates/layer-render-wgpu/src/present.rs), [display shader](../../crates/layer-render-wgpu/src/present.wgsl) | Add continuous-coverage tint and transient stroke display. Keep the overlay out of document raster, sampling, and export. |
| [Workspace working state](../../crates/layer-ui/src/workspace.rs) | Persist tool settings, display preferences, and last selection-tool identity with defaults for old workspaces. Do not serialize a pending contact or Quick Mask editing session into a workspace. |
| [Document edits](../../crates/layer-core/src/lib.rs), [project storage](../../crates/layer-core/src/project.rs) | Add typed persistent selection nodes, durable coverage edits, validation, and project/recovery serialization. Distinguish project changes from composited-image changes. |
| [Layer actions](../../crates/layer-ui/src/art_layers.rs), [shared UI state](../../crates/layer-ui/src/lib.rs) | Project the temporary row separately; add typed saved rows, load/edit/preview actions, target-aware controls, grouping, and native host parity. |

Keep the pre-gesture selection immutable and render tentative changes into a
transient GPU target. Cancellation restores the original without an undo entry.
Each completed stroke, filled loop, fill, or gradient publishes exactly one
coverage edit: `Edit::SetSelection` for the current selection, or a durable
saved-node edit for a Selection Layer. A brush stroke plus its enclosed interior
is one edit.
Old artwork strokes retain their captured selection snapshots.

Successive strokes and dependent actions must observe completion in order.
Rapid pen-up/down cannot discard a stroke or blend against an outdated selection.
Fill, transform, undo, save, tool changes, and Quick Mask transitions after
pointer-up must wait for the appropriate committed result. In-flight failures
or cancellation invalidate late replies without overwriting a newer document.
Do not block native input/rendering while awaiting GPU readback.

Preserve partial values through anti-aliasing, overlay rendering, save/reopen,
and renderer recreation. Do not threshold low-opacity strokes through the
existing geometric refinement path. Current-selection edits remain undoable
without marking the project unsaved, following the existing document policy.
Stored Selection Layer changes mark the project modified, even though artwork
pixels do not change.

Rasterize only touched regions during a stroke and update display at frame
cadence. Do not perform full-image classification, feathering, or readback per
dab. Current packed masks cost 64 MiB at 8192² before scratch buffers and history.
An unchanged snapshot can share an Arc; successive changed masks do not share
their unchanged pixels automatically. Profile a long editing session on tablets.
If packed per-stroke snapshots exceed supported latency or memory budgets,
bounded storage or immutable shared tiles are prerequisite implementation work.

## Delivery and acceptance

Implementation milestones, in delivery order:

1. Shared document foundation: typed saved selection nodes, independent working
   copies, placement, lock validation, project persistence, and bounded undo
   accounting. Implemented; core/engine/UI regression suites pass. Existing
   selection types were moved out of layer ownership code, not duplicated.
2. Shared painted coverage and GTK Paint selection: implemented. Shared real-pen
   sampling, enclosed-area filling, ordered asynchronous captures, unchanged-edit
   suppression, and coverage-scaled overlays have core/GPU regression coverage.
   GTK mouse and injected Wayland pen journeys pass; light/dark controls reviewed.
   Overlay preferences remain part of the shared mask interface below.
3. GTK Quick Mask and Selection Layers: ordinary temporary row, painting conventions,
   saved-mask actions, display preferences, and editing indicator. Implemented.
   Native GTK light/dark journeys check coverage, exit, save/edit/load, and tint
   persistence. Grayscale thumbnails and multiple visible saved-mask previews
   are GPU-rendered; previews never affect artwork or the current selection.
4. Web: shared controls, independent mask colors, typed menus and asynchronous
   saved-mask thumbnails. Implemented; the seven-tool and mask workflow suite
   passes on Huion Chrome with injected mouse/touch/pen input.
5. Android: native projection with 48dp controls, following the reviewed GTK
   hierarchy and spacing. Implemented; Huion Vulkan/stylus checks cover painted
   coverage, saved-mask load/edit, history, and export isolation. Physical pen
   feel remains a manual check; automated device contacts are injected.

Build the selection editing target, ordered transactions, and overlay first.
Then deliver Paint selection, Quick Mask with its temporary row, and persistent
Selection Layers with the agreed save/edit/load workflow. All three are required
to complete this feature. Use the command inventory's **Core** entries for their
necessary menu integration. Its **Next** and **Later** entries are recommendations,
not implicit release requirements. Automatic subject detection and additional
material simulators remain separate work.

GTK, Web, and Android currently share the documented Select drawer. macOS,
iPadOS, and Windows need explicit native projection through their existing entry
points. Shared Rust owns behavior, validation, command availability, settings,
and history; hosts own native input capture, widgets, focus, and accessibility.
Preserve customized workspaces and the existing included-layout migration policy.

Tool tiles in toolbars and retained drawers follow the application-wide
[drag and reorder convention](drag-and-reorder.md), including the Customize
Title Bar exception. Canvas selection strokes begin immediately without a hold.

Required acceptance cases:

- Tap, open stroke, closed loop, near-start closure, self-crossing contours,
  open tails, subtractive loops, and cancelled gestures on mouse, touch, and pen.
- Same input replay at different event batch sizes; pressure on/off; camera zoom,
  rotation, and flips; geometry remains in document coordinates.
- A single 50% stroke, overlapping dabs within it, two separate 50% strokes,
  low-opacity values without ants, and full subtraction to an explicit empty mask.
- Independent Paint selection settings, mode modifiers, physical eraser priority,
  live closure feedback, overlay visibility, and keyboard-free mode controls.
- Quick Mask entry from none/empty/soft/transformed selections; unchanged round
  trip; black/white/gray painting, erase, swap, fill, and gradient.
- One temporary Quick Mask row; eye hides only overlay; Save creates an independent
  named snapshot and retains Quick Mask; exit removes only the temporary row.
- Select saved row to edit versus Load to use; visible touch actions and modifier
  clicks; last artwork target restoration; no accidental overwrite of a loaded
  source; explicit Replace; deletion retains the working copy.
- Durable saved-mask undo/redo and project dirty state; soft coverage, names,
  IDs, parents, and placement survive project save/reopen and recovery.
- Root default, explicit grouping, group move/duplicate/delete, reparenting,
  locked ancestors, independent sibling movement, merge/flatten preservation,
  and exclusion of saved masks from artwork rendering and sampling.
- Entry from supported/unsupported tools; mode/target/color restoration;
  document switch, focus loss, save/reopen, and explicit exit.
- Rapid successive strokes and immediate fill/paint/save/undo after pointer-up;
  one undo step per gesture; redo; late GPU results and renderer recreation.
- Locked/transformed artwork layers and active layer masks; no self-clipping;
  unchanged earlier artwork replay; exclusion of tint from exports and sampling.
- Large-canvas sustained latency and undo memory on desktop and tablet. Use
  independent coverage oracles, native-input tests on every host, and a physical
  pen usability pass for loop closure and pressure defaults.

The closure threshold and default size/pressure settings should receive a focused
prototype usability check. Any adjustment must update this shared specification
and its acceptance cases rather than introduce host-specific behavior.
