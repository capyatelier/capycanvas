# M5 retained geometry and image operations

[M5–M6 specification](photo-editing-m5-m6.md) · [Developer guide](README.md)

Status: **PROPOSED for review**. Baseline: `origin/main` at `da5b399ad`.
This document defines required behavior; it does not describe shipped support.
Code inspection established the seams below. No build, GPU measurement or host
journey is claimed. GTK, Web and Android are the first delivery; Apple and
Windows retain compiling shared consumers and explicit host follow-ups.

## Retained placement and admission

Whole paint/photo Transform without a pixel selection retains original source
and raster planes. Apply changes geometry, not those pixels. A single active
mask or selected-pixel Transform uses the existing destructive pixel transaction.
Several selected roots plus a pixel selection are refused: “Clear the pixel
selection to transform several layers”. A group root is not a single pixel target.

**New proposed `LayerPlacement` contract:** one exact outer homography `H`,
an optional retained Warp mesh (`Option<Arc<MeshMap>>`), and persisted
`Interpolation`. Keep identity/affine fast paths. This replaces affine-only
placement; it does not introduce a transform stack, expression tree, nested
document or second history system. [Live Liquify](photo-editing-m5-m6-liquify.md)
is deferred to a future effect layer/filter. Add no displacement variant,
field grid, input basis, vector plane or reserved extension slot for it.

For mesh source position `s`, the document point is `T · H · M(s)`, where `T`
includes the leaf offset and ancestor translations. Without deformation,
`M(s)=s`. A common document-space homography `A` changes every admitted leaf
by `H_new = T^-1 · A · T · H_old`. In the existing `then` convention this is
`H_old.then(T).then(A).then(T_inverse)`. Groups remain translation containers;
do not change offsets and outer maps twice.

Expose full render geometry, an optional affine editable mapping, exact outer
postcomposition, conservative destination bounds and source footprints as
distinct contracts. Never return a partial affine from `target_transform` or
invent a general mesh inverse. Geometry identity includes interpolation and
mesh topology/net.

Admit only finite, invertible, pole-free homographies across the covered domain.
Reuse `Projective::covers` conditioning, including its minimum positive weight
ratio of 1/1024; mesh admission checks the control hull as well as source bounds.
Off-canvas output is allowed. Collapse, insufficient precision or resource
overflow preserves the last valid preview. Mesh folds remain allowed with the
existing deterministic later-triangle-wins sampling rule, including all reads.

| Target | Free / Uniform / Distort | Warp |
| --- | --- | --- |
| One whole paint/photo leaf | Retained outer change | Retained mesh edit |
| Several paint/photo roots | One common document delta | Refused |
| Supported group roots | Same delta on admitted descendants/masks | Refused |
| Group containing generator effect or saved Selection Layer | Refuse entire group for every mode | Refused |
| Generator, adjustment or saved Selection Layer selected alone | Refused | Refused |
| Single affine-editable mask or selected-pixel target | Existing destructive route | Existing destructive route |

Use `Document::layer_roots` and `layer_subtrees` to normalize selection and own
each descendant once. Freeze ordered targets, original selected roots, source/
raster/geometry identities and selection identity at session start. Include
hidden descendants. Any inherited lock, unsupported member, incompatible pending
operation or invalid target refuses the entire set. Explicit paper/background
selection is refused. Never move a convenient subset.

Supported groups may contain ordinary adjustment effects. Their parameters and
document-space evaluation stay intact; move their independent masks with the
group. Preserve clipping, blend, opacity, order and group isolation. A clipped
leaf selected alone moves alone; selecting its group moves the admitted subtree.
Do not flatten a group merely to make Transform available.

## Masks and the explicit write boundary

Linked masks inherit owner geometry once. Let the mask's local pre-map be
`P(m)=mask.placement(m)+(mask.offset-owner.offset)`. After the owner sampler
finds source position `s`, sample linked coverage at `P^-1(s)`. Keep the mask
pre-map homographic with its affine fast path. Do not copy owner deformation
into the mask or independently transform linked coverage again.

Unlinked masks retain their own document geometry and stay fixed when only the
owner transforms. Group transforms move the group mask and independent member
masks once; linked descendant masks follow their owner once. Default coverage,
initial coverage, inversion and disabled state retain their existing meaning.

**Apply Transform to Pixels** is the explicit boundary before non-affine content
writes, including the existing baked Liquify brush, and before linkage changes involving a
non-affine owner or mask. Projective is non-affine for this rule even without a
mesh. Use one shared admission/refusal path before brush, eraser, fill, gradient,
figure, cut, paste-into-content, mask application and retouch writes. No contact,
shortcut or menu action silently bakes. Ordinary affine writes keep their current
brush-footprint behavior. An independent affine unlinked mask remains editable.

Reading remains available regardless of write eligibility or lock. Current Layer
sampling, Wand/Color Select, selection-from-alpha, copy, reference Clone/Heal,
merge/stamp, thumbnails and export evaluate full placed content at document
coordinates. Destination write refusal must not disable a nonlinear reference.
An original-local read is a distinct explicit source, never an affine fallback.

## Transform session and controls

Extend the existing transaction, retained `Placement`, canvas bar and Tool
Options. Whole-layer metadata previews do not expand the destructive two-target
capture array. Starting waits asynchronously for required bounds and is
cancellable. Freeze original geometry/topology/interpolation and complete target
identities before preview. A stale target/source/selection cancels admission.

Apply validates all targets, then creates one reversible edit. Unchanged geometry
and interpolation create none. Failed Apply leaves a recoverable session. Cancel
restores exactly and creates no history; import Cancel removes only its provisional
batch. Reset restores session-start geometry, interpolation and node selection;
it does not restore identity or remove an existing warp. Blur rolls back only the
active drag and preserves the idle session. Undo/Redo restore exact retained roots.

Free/Uniform/Distort edit the outer map. Warp edits its own stored mesh beneath
that map. Entering Warp can insert an identity mesh without absorbing perspective;
node input uses the outer inverse. Reopening reads the original source domain,
mesh and interpolation. No `MeshMap::fit` on reopen, Apply, mode switch or grid
change may approximate existing geometry. Existing fresh defaults remain Linear
for Free/Uniform and Bicubic for Distort/Warp; an explicit choice survives mode
changes and saves. Nearest, Linear, Bicubic and Lanczos remain available.

Original Size applies only to photos with affine outer geometry and no nonlinear
stage. Restore unit scale while preserving position and orientation, including
reflection. Existing import-batch behavior remains only if every member qualifies.
Non-affine/projective geometry disables it with a specific reason.

**Reference and pivot:** expose one 3×3 reference chooser and draggable canvas
pivot, centered initially. X/Y are the chosen reference's absolute document
coordinates. Choosing another reference changes the readout only. Moving the
pivot changes the subsequent scale/rotation center without moving artwork.
Numeric X/Y moves the chosen reference exactly; rotation/scale use the pivot.
Handle drags retain the opposite-side anchor unless the existing center modifier
is held. Pivot/reference selection is session state, not a document edit.

**Snapping:** shared session state, off by default. Enabled translation/active
handles snap to canvas edges/center, other eligible layer bounds and existing
rulers. Exclude moved targets and descendants. Reuse `choose_ruler` and
`RulerConstraint::project`. Enter at 6 DIP and release at 10 DIP in view space;
choose the nearest candidate with stable target-ID/edge order for ties. Freeze
eligible bounds for the gesture; draw guides while captured. Numeric entry does
not snap. Retain existing Shift angle increments rather than adding angle policy.

**Nudge:** arrows move 1 document pixel; Shift+arrows move 10, independent of
view zoom/rotation. Text/numeric focus owns its arrows. Transform repeats update
one transaction. In Move, key-down through matching key-up is one continuous edit,
including repeats; blur ends it. Reuse shared key-up/repeat input, with no host
timer or time-based history grouping. Move uses the same admitted member planner.

**Transform Again:** add a shared menu command, unbound by default. Store only
the last successfully applied retained outer document delta `A` in this document
session. Reapply it to current admitted targets as one undo entry. Clear on document
replacement; Cancel, no-op and Undo/Redo do not replace it. Destructive pixel
transforms, mesh edits and Liquify are not recorded or repeated. They leave the
previous record intact. A mixed outer-plus-mesh edit is not repeatable. Admission
uses the same target/mode restrictions; no record means disabled with a reason.

## Warp topology and point editing

Replace uniform-only cell indexing with ordered normalized source breakpoints
on each axis, derived cell counts and the existing cubic control net. Retain
1–32 cells per axis and grid presets. Endpoints are exactly 0 and 1; values are
finite and strictly increasing. **New proposed minimum interval:** 1/65536.
Validate spacing before editing/loading; derive patch-local parameters from the
containing source interval, never destination screen spacing.

A vertical/horizontal split subdivides every patch of that column/row using exact
cubic de Casteljau subdivision. Insert the entire source line and corresponding
controls; no T-junctions or patch graph. Cross performs both splits atomically.
Evaluate intermediate arithmetic in f64 and store the existing finite point type.
The map before/after must agree within the existing independent tessellation
oracle's tolerance before moving a node. Never refit a sampled mesh.

Vertical, Horizontal and Cross arm one on-canvas insertion. Hover previews;
tap inserts; Escape cancels insertion and returns to Warp. Locate the source
parameter using the mesh's deterministic hit/UV evaluation after outer inversion;
fold overlaps use the same winning triangle as rendering. No hit, an existing
breakpoint or insufficient interval leaves geometry unchanged. At 32 cells the
affected axis is disabled; Cross requires room on both axes. Refining a compatible
grid is exact. Coarsening or replacing an edited grid requires explicit Reset Grid
within the session and must expose that shape change; it never silently refits.

Tap selects one node. Select Points permits tap toggles for pen/touch; Shift
toggles for mouse. Dragging a selected node moves the selected set. Apply the
delta once to the union of dependent controls, including shared controls. One
active node's tangents remain individually draggable. Selection, split and node
edits stay within the same Transform transaction; Reset restores the start set.

## P-8 bounds and incremental scheduling

Replace the production CPU pixel scan with shared GPU actual-alpha reductions;
extend `ContentBoundsRequest`, `ContentBoundsCache` and existing preparation/
continuation queues. Reuse thumbnail reduction internals, not an independent
bounds shader or generic job manager. Read back the small bounds record only.
Do not decode tiles, iterate pixels, wait for GPU work or copy a full selection
on the UI/input owner. Web yields bounded continuations on its event loop.

| Bounds purpose | Required coverage |
| --- | --- |
| Whole-content transform | Actual local color/source after overrides; alpha > 0, including off-canvas content; ignore layer visibility, opacity and mask |
| Active mask | Actual scalar coverage after default/initial/inversion and overrides over its finite editing extent |
| Selected pixels | Actual target coverage multiplied by selection coverage, including inversion; empty intersection refuses |
| Visible snap/anchor object | Full placed material appearance with actual mask products, not intersection of bounding rectangles |
| Group | Union each included member's appropriate bounds once; exclude unrelated/background content |

Local source bounds do not evaluate appearance in source space. Destination
material halos enter render footprints and actual visible/image bounds.
Wetness-only tiles do not count merely because they exist. Proved opaque-source
metadata may shortcut a scan; source existence cannot prove opacity. Reuse this
service for Trim, Reveal All and Fit Content and delete their superseded CPU scan.
During motion use frozen source hulls and conservative full-map damage; never
rescan every drag frame or label a conservative rectangle pixel-tight.

Keys include document epoch, query serial, renderer generation, target IDs,
source/raster revisions, full geometry/interpolation, color/material definition,
mask/selection state and captured animation time where relevant. Apply/Cancel,
target changes and renderer replacement reject stale results. Cancel stops new
work and drains accepted callbacks without publishing. Exact multi-chunk reads
freeze one immutable `artwork::Frame`/time; animation cannot restart them forever.

## Paint extents, material and explicit baking

For source-less paint with affine placement, inverse-map the full canvas and
union it with existing local content and linked editable masks before admission.
Reuse/extract `canvas_geometry`'s tile-aligned rebase plan without changing canvas
size. Shift tile keys and mask initial coverage; conjugate maps/offsets; preserve
tile identities with no resampling. Reserve the resulting extent before accepting
preview. Reject any dimension above 32768 while retaining the last valid state.
Aggressive downscale can exceed that limit even when numeric scale is valid.
Retained photos keep their fixed original-coordinate domain; non-affine writes
are refused and do not request a fictitious inverse paint extent.

For material-bearing paint, map raw pigment, Wetness and WatercolorWetness through
full geometry into document-space working planes, then evaluate existing material
appearance once there. Masks/opacity/blend follow. Missing scalar pages use their
existing zero defaults; unavailable backed pages require preparation. Bake uses
the same mapped raw planes, style coordinates and halo; it does not store material
appearance in Color and evaluate wetness again.

A senior bounded renderer spike must prove source halos, destination neighbors,
style scale, working-set admission, cache identity and preview/exact/bake parity
for affine, homographic and Warp geometry before enabling wet
paint Transform. Failure reopens design review; it does not authorize a second
material renderer, silent flattening or an unannounced paint restriction.

Apply Transform to Pixels operates on one admitted whole paint/photo leaf with
nonidentity placement. Freeze immutable inputs and use existing pixel-plane
resampling plus `BakeSteps` scheduling/publication. Cover all retained content,
including outside the canvas, at stored interpolation. Rebase output extent/
origin, clear source/deformation and set identity placement with established
offset translation. Preserve identity/name/parent/order, flags, clipping/blend,
references and material style. Preserve scalar material planes. Resample linked
masks through matching geometry as editable masks; unlinked masks keep world
coverage. Do not fold mask/opacity/blend into Color.

Validate destination, capture and history budgets before live mutation. Complete
publication is one atomic undo entry; failure/loss/cancellation preserves prior
state and immutable inputs. Undo restores exact source/raster/map/mask identities.
Reuse retained-input work from Merge/Frequency Separation where suitable, but
their composited Color bake alone cannot preserve these planes. No export/import
round trip, CPU canvas copy or new publication/history root.

## Image commands and source changes

| Command | Required retained-geometry policy |
| --- | --- |
| Canvas Size; Crop without Delete Cropped Pixels | Preserve local data/topology; change canvas window/root placement exactly |
| Rotate Image; Flip Image | Compose common document affine after outer maps; independent/group masks once, linked masks follow once; preserve topology |
| Reveal All; Trim; Crop Fit Content | Async actual placed coverage, then non-destructive canvas-window change |
| Image Size; Crop with Delete Cropped Pixels | If any affected non-affine retained content exists, refuse whole command before mutation; explicitly Apply Transform to Pixels first; keep existing affine policy |
| Rasterize Original Photo | Convert original extent/grid to document color/depth; preserve geometry; do not apply placement |
| Source Profile Repair | Replace immutable source interpretation, preserve sample tiles/extent and geometry |
| Revert to Original Photo | Clear base raster edits/material; preserve original source, outer placement, Warp and masks |
| Assign/Convert Profile; Change Depth | Preserve outer placement, mesh control values and interpolation while converting the existing color/material planes according to their contracts |

Source repair preserves retained geometry. With actual base edits, retain the
existing corrected-sibling workflow: copy outer placement, mesh and interpolation
to the corrected sibling, never old-meaning pigment/wetness; leave the edited
original intact. Preview and Apply share this candidate policy and guard source,
raster, geometry and target identities. One undo restores all prior roots.
No command gains displacement-plane handling in this effort.

Individual layer/group Transform leaves document samplers fixed. Accepted image
commands map samplers with `CanvasGeometry::to_canvas` in the same undo batch as
rulers/selection; non-destructive crop keeps outside points. Image Size/Delete
refusal changes no canvas, effect parameter, guide, mask, selection or sampler.

## Integration seams and release gates

| Existing seam | Required replacement/check |
| --- | --- |
| `crates/layer-core/src/layers.rs`, `projective.rs`, `warp.rs` | Full geometry, explicit affine projection, linked-mask evaluation, finite limits and breakpoints |
| `canvas_geometry.rs`, `content_bounds.rs`, `merge.rs` | Exact outer/window composition, affine rebase, GPU bounds lifecycle, full retained bake bounds |
| `crates/layer-ui/src/operation.rs`, `operation/placement.rs`, `image_geometry.rs` | One transaction/member plan, full geometry snapshot, bounds preparation and shared refusal |
| `crates/layer-ui/src/source_edit.rs`, `clipboard.rs`, `color_picker_session.rs` | Source policy and placed reads; no partial affine inverse |
| `crates/layer-engine/src/canvas.rs`, `bake_steps.rs`; `crates/layer-render/src/lib.rs` | Central write admission, immutable inputs, bounded jobs and atomic publication |
| `crates/layer-render-wgpu/src/scene/placement.rs`, `paint_transform/mesh.rs`, `scene/scale.rs` | Reuse UV sampler; material order; full-map cache keys/LOD/damage/source footprints |
| `region_sources.rs`, `retouch_sources.rs`, `snapshot.rs`, `thumbnails.rs` | Placed exact reads, mask products, folds and off-canvas coverage |
| `project.rs`, `project_storage.rs`, `history_budget.rs`, `raster.rs` | Validate/account every geometry/resource root and restore them atomically |

One integration owner revises the current project format for retained maps and
M6 metadata/resources. Update `docs/reference/project-format.md`, transports and
schema fixtures together. The new reader accepts only the new current format;
delete superseded old-reader fixtures, add no migrations. Charge shared mesh and
resource roots once in project/history budgets; reject malformed net/breakpoints,
wrong-owner geometry and corrupt final tiles atomically.

Extend existing no-window Recorder/core and GPU oracle fixtures. Existing seams
include `canvas_bar_tests::flipping_a_placement_stays_lossless_and_applies_as_one_step`,
`a_failed_apply_leaves_the_transform_ready_to_apply_again`,
`placement_tests::retained_placement_samples_full_source_across_tiles_without_creating_raster`,
and `transform_oracle_tests::native_mesh_transforms_match_the_cpu_tessellation_including_folds`.
Replace the photo Distort/Warp refusal fixture with retained success/re-edit;
delete silent mesh fitting and affine-only consumer fallbacks as replacements land.

Required independent regressions cover transparent source padding/erased edges,
actual disjoint mask products, inverted selection, stale bounds, negative local
origins and four-corner painting after affine scale/rotation/shear; nested roots,
hidden/locked members and whole-group unsupported-member refusal; linked/unlinked
mask registration; perspective pole/collapse rejection and folds; de Casteljau
split invariance/caps/shared-control motion; source repair/sibling/Revert;
all color depths; mapped wet-edge preview/exact/bake parity; pending publication,
renderer loss, immediate undo and corrupt last-tile rejection. Assert raw roots,
history and geometry as well as pixels. Save/reopen preserves net and interpolation.

Every affected read path must agree across incremental/full composition, Current
Layer/Visible sampling, reference retouch, selection-from-alpha, merge, thumbnail
and export. Display LOD may approximate sampling only; exact capture must not use
overview pixels. Check existing tests and commands through [Testing](testing.md).
Walk complete GTK/Web/Android journeys in light/dark with mouse/touch/pen where
applicable, then compile Apple/Windows consumers and retain their host follow-ups.
Measure moving frames on each tier's reference hardware under [measurement
rules](../performance/measuring.md); update the [target tables](../PERFORMANCE_TARGETS.md).
Unmeasured/unmet tiers and unwalked hosts remain open, even if affine tests pass.

Review must accept the explicit nonlinear-write/bake boundary, single-leaf Warp,
generator/Selection Layer group refusal, one-mesh limit, restricted Again,
extent-overflow refusal and source/Revert semantics. Material parity and
worst-case projective/mesh working sets are critical gates;
resolve them before assigning dependent implementation work.
