# Authored artwork, working state and capture

[Technical documentation](../README.md)

This is the target shared semantic contract for the
[file-format implementation](../development/capy-format.md). It fixes ownership
and API boundaries for the final codec and editor. The existing application still
uses `Document.layers` until the M3 cutover; this guide does not claim that cutover
or its qualification has happened. [Package grammar](capy-package.md) owns wire
spelling, integrity, preservation and transport rules.

## Objects and identity

An artwork owns typed stores for compositions, stacks, occurrences, paint
sources, coverage sources, effects, immutable effect definitions, saved
selections, guides and outputs. Every authored record has an opaque 128-bit
portable ID. A decoded ID maps once to a typed `u32` handle. Each store allocates
handles monotonically and never reuses a slot during its lifetime; exhaustion
fails admission before mutation. An occurrence handle cannot stand in for a
paint-source or coverage-source handle. Handles are meaningful only with their
store owner. Independent opened drawings with equal portable IDs remain distinct
runtime owners.

A store slot is stable across deletion and undo. Deletion removes the live value;
the inverse edit owns the removed record and its immutable roots and can restore
that same slot. A missing slot is an error, never an implicit empty source. Store
slots, indexes, allocator cursors, revisions and process addresses are not portable
identity. Reopening resolves portable references into a fresh set of handles.

Stacks alone own front-to-back order. An occurrence has at most one containing
stack. Groups refer to nested stacks; there is no second authoritative parent
field or flat layer order. Parent, sibling, clipping, ancestry and target indexes
are derived at structural publication. Ordinary painting resolves its destination
once and uses compact source handles without portable-ID or linear parent lookup
per dab.

A paint source owns editable local pixels. An occurrence owns one use of content.
Two sources can share immutable tiles or an imported image without sharing edits.
Independent Duplicate allocates new source and occurrence IDs and initially shares
immutable resources. Resource deduplication never aliases editable source identity.
A future linked duplicate has multiple occurrences referring to one source; the
baseline preserves that shape without enabling edits to it.

The `authored::GraphShape` projection validates relationship types, cardinalities
and dependency bounds. Its `Editable` result qualifies only topology; native
adoption also requires payload, wire-schema, integrity and device admission.
The projection owns no pixel data and is built at structural boundaries, never
per frame. `authored::Store` supplies the stable typed slots.

## Complete ownership mapping

The inventories below cover the current fields of
[`Document` and `Layer`](../../crates/layer-core/src/lib.rs),
[`LayerProperties` and `LayerMask`](../../crates/layer-core/src/layers.rs) and
[`EffectProgram` and `EffectInstance`](../../crates/layer-core/src/effects.rs).
“Retained” means owned by immutable authored snapshots, undo records or accepted
jobs for as long as any owner needs the value. Defaults describe new authored
values; the package grammar specifies which values may be omitted on wire.
Required values cannot acquire defaults from current UI preferences.

### Document

| Current field | Authoritative owner, default and units | Required assertion |
| --- | --- | --- |
| `id` | Artwork envelope portable identity; new unique ID, retained for the drawing lifetime. | Save/reopen preserves identity; opening the same ID twice does not share mutable stores. |
| `width`, `height` | Composition frame extent in integer local pixels; explicit, positive dimensions. Composition origin is explicit, initially `[0,0]`. | Crop/grow changes the frame without discarding out-of-frame source pixels. |
| `color` | Composition working RGB primaries and committed sample depth; new default sRGB/U8. | Preserve U8/U16/F16/F32 codes and independent imported interpretation. |
| `blend_space` | Composition blend domain, initially Linear. | Perceptual and Linear evaluate according to their frozen rules; admission restrictions remain separate from resource layout. |
| `resolution` | Composition physical density, absent unless authored; exact positive rational pixels per inch, centimetre or metre on each axis. | Round-trip both rational pairs and unit without normalization or rounding. |
| `proof` | Output proof intent, initially absent; its profile is an immutable resource. | Preserve name, profile, conversion intent, black-point compensation, paper and black-ink simulation flags; temporary proof toggles are omitted. |
| `metadata` | Artwork envelope photo metadata, initially no Exif, XMP or IPTC blocks. | Preserve exact opaque blocks, including metadata unrelated to the visible output. Placing another image does not replace these blocks. |
| `sdr_rendition` | Output delivery intent; exposure in stops, contrast multiplier, headroom in stops above RGB 1, highlight-color fraction and balance. Defaults are `0`, `1`, `2.3004484`, `0.3`, `0`. | All five values survive save/reopen; display capability never changes authored intent. |
| `layers` | Separate typed stores and composition root stack; no flat editable authority. | Stack order and membership reproduce composition, including empty stacks and retained unplaced objects. |
| `active_layer`, `active_mask` | Shared working state: optional selected occurrence and explicit target; new drawing chooses its first paint occurrence, empty artwork has no target. | Portable save omits both; delete/undo repairs or restores both atomically. |
| `selection` | Shared working selection, initially absent, in composition coordinates. | Portable save omits it; shared selection undo, painting restrictions and parked tabs retain it. |
| `reference_layers` | Authored `reference` designation on occurrences, initially false. | Preserve reference membership independently of visibility and rebuild reference-query scopes after grouping/reorder. |
| `rulers` | Authored guide inventory; initially empty. Geometry is in composition pixels and has stable authored identity. | Straight, Parallel and Radial geometry survives round-trip and undo; stroke constraints remain transient snapshots. |
| `revision` | Monotonic runtime mutation generation. | Omit on wire; asynchronous work cannot use a portable ID alone to accept a result. |
| `next_layer_id`, `next_stroke_id` | Typed-store allocators and transient contact allocator. | Omit on wire; reopening cannot collide with retained objects and allocator exhaustion cannot wrap. |

`DocumentNames` supplies localized initial names. Created occurrence names are
literal authored strings, not live localization keys. Window titles, locations,
permissions, tab order, camera, tool settings and view toggles remain with their
existing shared working/session or workspace owners.

### Layer and properties

| Current field | Authoritative owner, default and units | Required assertion |
| --- | --- | --- |
| `Layer.id` | Occurrence portable ID plus typed handle. Content has a distinct identity. | Rename/reorder preserves both; independent duplication changes both. |
| `name` | Occurrence name; literal UTF-8, supplied at creation. | Preserve exactly without pixel invalidation. |
| `kind` | Typed occurrence content: paint-source use, nested stack, effect or saved selection. | No fake paint source for a group, adjustment, saved selection. |
| `visible` | Occurrence contribution visibility, initially true. | Hiding contribution cannot disable a source demanded by another explicit input. |
| `opacity` | Occurrence contribution factor, initially 1, finite `[0,1]`. | Opacity affects this occurrence only and preserves pass-through interpolation. |
| `raster` | Paint-source immutable sparse revision, initially empty. Color and material planes belong to that source. | Exact tile codes and unchanged compressed bytes survive capture; a missing override reveals the imported base. |
| `source` | Paint-source optional immutable imported base; absent for new paint. | Preserve Original/Rasterized role, dimensions, physical density, profile, assumed-profile flag and samples separately from overrides. |
| `properties` | Split according to the next rows. | No aggregate old-layer replacement is necessary to edit one owner. |
| `mask` | Optional inline occurrence mask use in stable slot `mask`, referencing a coverage source. | Mask source edits differ from enabled/link/placement edits; no mask-layer ID convention remains. |
| `pending_operations` | Transient accepted commands retained by engine/publication jobs. | Omit commands from portable data; capture their committed revision promises or refuse a boundary that cannot be represented coherently. |
| `effect` | Reference to an effect application; absent for non-effect content. Application identity differs from definition identity. | Shared definitions remain shared across save and undo. |
| `selection` | Saved-selection authored record, referenced by a non-compositing occurrence. | Preserve geometry/coverage independently of the current working selection; no contribution to exported color. |
| `parent` | Derived containing-stack/group index; no authored copy. | Reorder/group uses one membership transaction; encode only stack order. |
| `offset` | Occurrence translation in enclosing stack pixels, initially zero. | Preserve inherited ordinary-group offsets, including masks and unplaced groups. |
| `placement` | Occurrence-local retained placement, initially identity projective map, no mesh, Linear interpolation. | Preserve projective matrix, cubic mesh geometry and interpolation; generated tessellation/GPU buffers are omitted. |
| `alpha_locked` | Occurrence editing lock, initially false. | Retain paint behavior without changing source sample identity. |
| `locked` | Occurrence editing lock, initially false; inherited group lock is derived. | Prevent current edits while permitting valid undo restoration. |
| `clipped` | Occurrence clipping membership, initially false. | Clipping base follows stack position; no second saved base-ID edge. |
| `blend` | Occurrence blend operation, initially Normal. | Preserve blend mode and the clipped pass-through rule. |
| `selection_mask` | Saved-selection authored display color and opacity; defaults sRGB red `[1,0,0,1]` and `0.5`. | Preserve named selection overlay preferences; overlay stays out of output pixels. |
| `extent` | Explicit source-local domain for paint and coverage; an effect/generator's local domain belongs to its application; selection geometry retains its own domain. Group frame is inherited from composition, never a new image. | Materialize the current effective local extent before removing the fallback field; shrinking canvas does not shrink sources or masks. |

The composition frame, source-local domain, occurrence placement and output crop
are independent. Existing paint local extent is the stored extent (or canvas)
expanded to contain the imported image. A mask's stored extent falls back to its
owner's effective extent. The cutover resolves these fallbacks into explicit
source domains. Whole-tile rebasing changes domain coordinates and placement in
one transaction without losing hidden tiles. Apply Transform to Pixels publishes
new source roots and the corresponding placement together.

### Coverage and mask use

| Current `LayerMask` field | Authoritative owner, default and units | Required assertion |
| --- | --- | --- |
| `id` | Coverage-source identity and handle; the use is addressed by occurrence plus `mask` slot. | No source/occurrence ID overloading; mask sources retain identity through unlink/relink. |
| `raster` | Coverage-source sparse immutable scalar revision, initially empty. | Missing tiles retain declared initial/default coverage; no interpretation as image alpha or luminance. |
| `enabled` | Mask use, initially true. | Disable changes application only, never source data. |
| `linked` | Mask use, initially true. | Linked source coordinates follow owner placement; toggling preserves displayed coverage. |
| `placement` | Mask-use independent projective geometry, initially identity. | Preserve mask geometry and the owner's pre-map through projective/mesh placement. |
| `extent` | Explicit coverage-source domain in local pixels; resolve old fallback at construction/cutover. | Preserve coverage outside the composition frame. |
| `offset` | Mask-use translation in its defined parent domain. | Preserve independent unlinked placement and inherited group translation. |
| `initial` | Optional retained coverage-source selection geometry or scalar resource; initially absent. | Preserve contour even/odd rule, affine map, inversion and immutable pixel coverage without rasterizing contours at save. |
| `default_coverage` | Coverage-source finite scalar `[0,1]`, initially 1 for Reveal All. | Freeze missing-tile and out-of-bounds coverage independently of use inversion. |
| `inverted` | Mask use, initially false. | Apply inversion at its defined composition stage; initial-selection inversion remains separate. |
| `pending_operations` | Transient coverage commands and their retained roots. | Omit commands; one accepted publication owns the resulting committed scalar revision. |
| `show_area` | Shared working mask-inspection state, initially false. | Omit from portable source and previews; retain only in working/session ownership. |

Saved selections retain `Selection.shape`, `affine` and `inverted`. Contours use
finite pixel coordinates and the even/odd rule. Pixel selections retain origin,
extent, sample representation and immutable coverage. An absent current selection
and an empty current selection remain different working states. Saved-selection
painting changes its authored coverage; loading it into current selection changes
working state and remains undoable without marking artwork dirty.

### Source and material resources

A paint source retains `SourceImage.kind`, `extent`, `resolution`,
`interpretation.{channels,depth,profile,profile_assumed}` and `tiles` together.
Original images keep independent Gray, GrayAlpha, RGB, RGBA or CMYK interpretation.
Rasterized images retain explicit working RGBA interpretation. ICC bytes are
immutable resources; equal profiles may share bytes without merging source IDs.
Physical resolution is optional and retains exact rational units. Future imported
per-source descriptive metadata belongs to the source, not the drawing's photo
metadata envelope.

`RasterData.tiles` retains plane, local tile coordinate, pixel descriptor and
immutable publication. Color, Wetness and WatercolorWetness are paint-source
planes; Mask is scalar coverage-source data. `RasterData.watercolor` retains
`wet_edge`, `burnt_edge` and `edge_width` as live committed material state. Its
first two values are fractions `[0,1]`, and edge width is local pixels `[1,16]`.
Absent material state stays absent. Per-contact reservoirs and coverage are
transient. Save/reopen must preserve both the present appearance and the next
stroke's wet behavior.

A resource identity comprises immutable owner and block identity; interpretation
participates in decoded-cache identity. A portable resource ID, byte offset,
checksum or `Arc` address alone does not establish identity across independent
opens. Unchanged resources keep the same runtime identity across captures,
independent of archive layout. Integrity checks and optional strong deduplication
remain separate from this ownership rule.

### Effects

| Current field | Authoritative owner and defaults | Required assertion |
| --- | --- | --- |
| `EffectInstance.program` | Shared immutable effect-definition reference. | Capture/undo preserves one definition owner; saving does not detach by mutating a cloned program. |
| `EffectInstance.values` | Effect application values addressed by stable parameter keys. | Decode once to compact ABI slots; rename/reorder of controls never retargets values. |
| `EffectProgram.abi`, `id` | Definition's execution ABI and program identity, distinct from its portable authored ID and semantic type version. | Validate ABI/slot layout before admission; unsupported definitions remain preserved. |
| `label` | Definition presentation metadata. | Preserve literal/localized label representation independently of semantic identity. |
| `constant_color` | Optional generator contract naming its color parameter. | Static pointwise fills evaluate directly from the tagged color, including its alpha; the definition and parameter survive save and undo. |
| `kind` | Definition Adjustment or Generator. | Adjustment consumes a typed scoped backdrop; generator does not invent that dependency. |
| `alpha`, `space`, `resolution` | Definition contracts; defaults Preserve, Linear, Native. | Freeze premultiplied evaluation, blend-domain conversion, support and exact/display resolution roles. |
| `wgsl`, `entry` | Immutable code resources and stable entry point; module-local bindings contain no artwork IDs. | Preserve source bytes and resource sharing; remap bindings outside shader text. |
| `passes` | Ordered immutable pass declarations; default empty pointwise path. Each owns `entry` and `sampling`. | Preserve fusion boundary, sampling bounds, previous/original-input meanings and last-pass property application. |
| `time` | Definition's time-input declaration, default false. | Evaluation uses the capture's explicit effect phase. |
| `lookups` | Immutable lookup declarations, default empty. Retain code, entry, dependency keys, output count, workgroup size and workgroups. | Derived lookup buffers are omitted; keyed dependencies retain their declared order. |
| `auxiliary` | Optional definition-local binding contract: LUT resource/color space or analysis kind. | LUT bytes are authored resources; derived illumination buffers and leases are not. |
| `pages` | Immutable page IDs and labels, default empty. | Page reorder does not alter ABI slots. |
| `parameters` | Immutable stable-key schema, including explicit ABI slot association. | Retain every field listed below and preserve shader offsets independently of UI order. |
| `constraints` | Immutable keyed constraints, default empty; ordered-number lower/upper keys and gap. | Validate complete candidate values before publication, independent of UI ordering. |

Each parameter retains `key`, `label`, optional `section`, `page`, `visible_when`
and `soft_bounds`, plus `mapping` (default Linear), `kind` and typed `default`.
Numeric parameters retain min/max/step/decimals and display unit; numeric semantic
dimension is explicit: dimensionless, seconds, or length with a named pixel
reference space. Existing pixel-scaled effect controls mean composition pixels;
a unit label such as `px` never decides resizing behavior. Visibility conditions
refer to stable parameter keys and typed values.

Choice values persist stable option strings from `EffectOption.value`, retaining
optional labels. GPU numeric choice indices are derived once from the immutable
ABI option mapping. Number and Toggle retain their values; Color retains portable
color interpretation; Curve retains analytic control points; Gradient retains
positions and colors; LUT retains its optional typed resource binding. Empty LUT
and missing application values are distinct: omitted values use only the frozen
definition default, and an unknown parameter or option is unsupported content.

## Validation and supported editing

Validation distinguishes malformed data, valid unsupported content and device
admission failure. Validate every retained object, including disconnected and
unplaced content, before choosing editable mode. Visibility and output reachability
do not hide unsupported semantics.

1. Check unique IDs, reference existence, relation types, finite numbers, domains,
   resource descriptors and required parameter/port keys. Resource references do
   not become evaluation edges. Malformed references remain invalid even inside
   otherwise unsupported records whose visible reference structure is known.
2. Reject instantaneous evaluation cycles, stack containment cycles and recursive
   definition expansion. Bound object/edge counts, nesting and expansion work;
   do not apply the evaluation DAG rule to arbitrary ancillary/resource links.
3. The editable subset has one composition, one root stack and one canvas output.
   Every occurrence has at most one containing stack, every nested stack at most
   one group owner, every effect application at most one occurrence, and every
   editable paint/coverage/selection source at most one use. Immutable definitions
   and binary resources may have any admitted number of uses.
4. Count uses through reused stacks as well as direct source references. Two group
   occurrences using one nested stack are unsupported even when its paint source
   has only one direct occurrence reference. The same rule applies when one or
   both group uses are retained unplaced.
5. Empty stacks and intentionally unplaced objects are valid. Do not garbage
   collect authored records merely because the current output cannot reach them.
   Unknown non-ancillary content anywhere selects preservation mode. Unknown
   ancillary records follow the package's explicit ancillary/copy-safety rules.
6. Apply editor restrictions only after structural validation. Multiple outputs,
   composition contexts, explicit shared mattes and reusable interfaces can be
   structurally valid but unsupported for editing. Never flatten them to an
   editable subset or silently drop the unsupported records.

Source `color`, scalar `coverage`, and stack operations are different port types.
A typed endpoint includes object identity and stable port key. Color ports define
bounds, coordinates, sampling, working/profile interpretation and alpha association;
coverage ports define scalar range, domain and missing/out-of-bounds behavior.
Absent ports use a frozen type-defined default or fail validation. Unknown ports
select unsupported content. No implicit alpha-to-coverage or luma conversion is
introduced by the container.

The existing common stack evaluator owns baseline ordering. Evaluate entries
bottom-to-top. Isolated groups start transparent; pass-through groups interpolate
`B + opacity * mask * (group(B) - B)` in the applicable blend domain. A clipped
pass-through group is isolated Normal. Adjustments transform their scoped lower
composite; clipped adjustments preserve clipping-base coverage. Masks retain
source evaluation, placement, inversion and application as separate stages.
Explicit future matte connections stay attached across reorders; ordinary
clipping is derived from current stack order.

## Shared edits, targets and undo

The ordered shared editor owns authored stores and working state. Working state
contains current selection, selected occurrence, explicit drawing target and mask
inspection, plus its own mutation generation. Existing UI/session owners retain
camera, tools, tabs and preferences. One transaction can update artwork and working
state atomically. Working-only changes do not advance the artwork saved checkpoint;
selection undo remains in shared history.

A drawing target contains a typed paint/coverage/saved-selection destination and
its occurrence context. Stroke admission resolves group locks, mask linkage,
coordinates and source generation once. The source handle selects mutable backing;
the occurrence context selects placement and editing policy. A query for an
occurrence is not a query for its raw source. Deleted or changed targets are
revalidated at a contact boundary, never silently rerouted mid-stroke.

Structural edits validate their complete candidate before publication. Inverse
edits retain typed records, source roots, parameters and affected working state.
Deletion explicitly chooses whether to remove exclusive content with its use;
shared or intentionally unplaced content cannot disappear as a side effect.
Duplicate/paste remaps selected authored identities and visible bindings, while
sharing immutable resources. Unsupported preservation records are not an implicit
license to clone content whose copy contract is unknown.

Undo/redo restores the same handles and immutable roots, with admission in both
directions. It does not reconstruct a picture or allocate replacement identities.
History accounting counts actual shared owners across live stores, undo/redo,
parked tabs, snapshots and accepted jobs. Removing the last rendered use does not
release a source still held by a bake, contact-start read, history or save.

`RasterRevision` and `RasterTile` remain immutable single-publication promises.
Capture can retain pending promises; workers await them off UI/input/render
submission paths. Errors remain visible to every owner. A consumer's cancellation
releases its ownership but cannot cancel a producer needed by another consumer.
Prediction and late correction publish a later root and cannot mutate a root
already captured for save.

## Scene and query boundary

A borrowed typed scene view exposes composition, stacks, occurrences, source roots,
effects and output context to the common evaluator. An owned scene snapshot may
share that metadata and immutable roots across worker boundaries. Neither form
builds `Layer` records or owns a second editable document. A warmed source update
changes source generation and damage, with no authored topology rebuild or new
lowering pass. Structural publication atomically supplies new topology and indexes.
A worker that missed its baseline or recreated its device requests a full snapshot.

Queries address raw sources, placed occurrences, scalar coverage, stack prefixes,
effect inputs or outputs against an immutable scene revision and evaluation
context. Reference queries retain a selected scope, not copied layers with changed
visibility. Effect comparison retains an effect-application value snapshot, not
`Box<Layer>`. Bake/merge retains the prior evaluated scope and source roots through
destination publication, even after source occurrences leave the live stack.

`ArtworkQuery` and source-analysis keys identify typed effect input scope,
contributing source generations, topology, definition/parameter dependencies,
ancestor placement and captured phases. Name/UI-order changes cannot invalidate
pixels; a changed upstream source or effective phase must invalidate its analysis.
An effect's own parameter changes retain reusable analysis when its input contract
is unchanged. Candidate acceptance checks document/store owner, activation,
request and relevant scene/source generations; cancelled or stale jobs cannot
install resources. Analysis leases may outlive a live scene while owned by an
accepted snapshot and must release after their final owner.

Cache identity separates decoded resource/interpretation, mutable source revision,
and evaluated occurrence/input/context. Two occurrences with different masks,
placements or backdrops cannot share evaluated pixels merely because they share
source identity. Cache keys do not hold retired authored roots indefinitely.
Exact output and sampling use exact/native paths, never display approximations.
The common evaluator retains existing kernels, fusion, damage windows, material
handling, native precision and display/exact publication order.

## Capture, phases and recovery extension

Shared capture returns immutable authored artwork, an output evaluation context,
and a token containing document/session identity, activation generation, artwork
checkpoint and working-state generation. The caller need not clone history.
Resources are enumerable directly from the capture, independently of ZIP writing.
A future private session/history codec can capture working state and bounded
history at this same ordered boundary without extending the portable manifest.

The output context pairs source roots, effect phases and source-analysis inputs
from one committed boundary. It includes composition/output identity, framing,
working/blend interpretation and delivery intent. Paper is an ordinary Solid Color effect occurrence; selection
and mask-inspection overlays are excluded. Preview generation is optional and
uses this exact context. Preview failure cannot discard successfully captured
source, acknowledge a newer checkpoint or require converting source to a bitmap.

Each time-dependent effect carries an effective captured phase in seconds,
addressed by effect identity. Opening seeds its playback clock at the saved phase
and a fresh local elapsed-time origin. Animated playback integrates future elapsed
time at its current rate. A rate edit first advances at the previous rate to the
edit boundary, then installs the new rate. Thus two seconds at rate 1 followed by
three seconds at rate 2 has phase 8, not `elapsed * current_rate = 10`. Nonanimated
effects evaluate their authored time value multiplied by rate. Enabling animation
continues the prior static phase at its boundary; disabling uses the authored
static phase, preserving existing `EffectClock` behavior. Recreating a renderer or
capturing an export never re-derives an integrated phase from total elapsed time.
This is a playback capture contract, not a seekable timeline or animation system.

Manual save reserves its captured checkpoint and acknowledges it only after the
host confirms durable publication. A later live edit remains modified; Undo back
to the acknowledged checkpoint can become clean. Recovery publication uses a
separate token and never changes the manual-save checkpoint or destination.
A failed, cancelled or stale completion cannot acknowledge newer work, retire
another generation's resources or resurrect a closed drawing. Save As and atomic
replacement retain immutable backing ownership for old snapshots and history.

The portable resource visitor selects all authored roots, including hidden and
unplaced content. History/session traversal uses the same resource ownership
boundary but selects its own roots. Current recovery can write captures through
the package writer; future recovery must not require constructing, reopening or
unpacking a complete archive to enumerate metadata and unchanged resources.
Private session restoration, persisted inverse edits, incremental storage and
recovery lifecycle changes belong to the later
[automatic-recovery work](../development/autorecovery.md).

## Regression oracles

These existing tests define behavior to preserve; they do not establish that the
new implementation passes it.

| Boundary | Existing oracle |
| --- | --- |
| Bake lifetime and composition | [`merge_tests.rs`](../../crates/layer-core/src/merge_tests.rs) and [`scene/stack.rs`](../../crates/layer-render-wgpu/src/scene/stack.rs) |
| Shared roots, selection and history admission | [`history_budget/tests.rs`](../../crates/layer-core/src/history_budget/tests.rs) and [`raster/restore_tests.rs`](../../crates/layer-render-wgpu/src/raster/restore_tests.rs) |
| Groups, masks, native precision and transforms | [`scene/scale/tests.rs`](../../crates/layer-render-wgpu/src/scene/scale/tests.rs), [`effect_tests.rs`](../../crates/layer-render-wgpu/src/scene/scale/effect_tests.rs), [`transform_tests.rs`](../../crates/layer-render-wgpu/src/scene/scale/transform_tests.rs) and [`placement_material_tests.rs`](../../crates/layer-render-wgpu/src/placement_material_tests.rs) |
| Captured phases and source-aware analysis | [`artwork_sample_tests.rs`](../../crates/layer-render-wgpu/src/artwork_sample_tests.rs), [`effect_analysis_lease_tests.rs`](../../crates/layer-render-wgpu/src/effect_analysis_lease_tests.rs) and [`snapshot/tests/local_adjustments.rs`](../../crates/layer-render-wgpu/src/snapshot/tests/local_adjustments.rs) |
| Committed capture and late correction | [`canvas.rs`](../../crates/layer-engine/src/canvas.rs), [`raster/native_tests.rs`](../../crates/layer-render-wgpu/src/raster/native_tests.rs) and [`snapshot/tests.rs`](../../crates/layer-render-wgpu/src/snapshot/tests.rs) |
| Save acknowledgement, recovery and parked owners | [`document_files.rs`](../../crates/layer-ui/src/document_files.rs), [`recovery.rs`](../../crates/layer-ui/src/recovery.rs) and [`document_sessions.rs`](../../crates/layer-ui/src/document_sessions.rs) |

Schema fixtures must cover direct sharing, indirect sharing through reused groups,
retained unplaced shared content, independent sources sharing immutable bytes,
unknown disconnected non-ancillary records, typed-port mismatch, cycles, renamed
and reordered keyed effects, two output contexts and captured phase after a rate
change. Integration tests additionally assert source-only save, selection omission
with working undo retained, resource lifetime after delete/bake/parking, stale
completion rejection and preview failure after valid source capture. Correctness
fixtures do not qualify frame time, allocation, memory or host transport bounds;
those require the plan's matched hardware measurements and host journeys.
