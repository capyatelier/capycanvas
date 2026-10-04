# Authored artwork, working state and capture

[Technical documentation](../README.md)

The shared editor and codec use the typed authored model in
[`authored/artwork.rs`](../../crates/layer-core/src/authored/artwork.rs).
[`Document`](../../crates/layer-core/src/lib.rs) owns that artwork and separate
working state. Renderers consume typed scene views and immutable snapshots.
[Package grammar](capy-package.md) owns wire spelling, integrity, preservation and
transport rules; this guide owns runtime state and resource lifetime.

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

Deletion, merge and effect replacement use `Document::effect_edits` to include
dependent records in the undoable edit. Removing an application removes its
saved output phases; removing or replacing the last application of a definition
releases that definition. Shared definitions and already unplaced definitions
remain authored content; undo restores the removed identities and phases.

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
and dependency bounds. Its `Editable` result qualifies only topology; editable
adoption also requires payload, wire-schema, integrity and device admission.
The projection owns no pixel data and is built at structural boundaries, never
per frame. `authored::Store` supplies the stable typed slots.

## Records and working state

[`Artwork`](../../crates/layer-core/src/authored/artwork.rs) holds the ten typed
stores, root composition, default output, photo metadata and retained ancillary
extensions. [`Store<T>`](../../crates/layer-core/src/authored/store.rs) shares its
slot inventory and immutable record owners; a changed record replaces only that
owner. Its portable ID belongs to the slot rather than a field inside the record.
Snapshots share unchanged stores and records.

The tables list runtime fields and their contracts. The package grammar specifies
which values may be omitted on wire. Required values cannot acquire defaults from
current UI preferences.

### Document and working state

| Field | Owner and default | Required assertion |
| --- | --- | --- |
| `Document.artwork` | Typed authored stores and identities. | Save/reopen preserves every retained authored record, including hidden and unplaced content. |
| `Document.owner` | Fresh runtime document owner. | Equal portable IDs from independent opens cannot admit each other's asynchronous results. |
| `Document.revision` | Monotonic runtime mutation generation. | Omit on wire; use owner and relevant generations when accepting asynchronous work. |
| `Document.scene_index` | Shared derived order, parent and source-use indexes. | Rebuild at structural publication; source-only painting and parameter edits keep the same topology index. |
| `Document.next_stroke_id` | Transient contact allocator. | Omit on wire; exhaustion fails before mutation. |
| `WorkingState.occurrence`, `target` | Optional selected occurrence and explicit `SourceTarget`; a new drawing chooses its first paint occurrence. | Delete/undo repairs or restores both atomically; portable save omits them. |
| `WorkingState.selection` | Optional current selection in composition coordinates. | Portable save omits it; selection undo, painting restrictions and parked tabs retain it. |
| `WorkingState.selection_visibility` | Transient per-occurrence overrides for saved-selection display; absent entries inherit authored visibility. | Navigation never edits `Occurrence.visible`; delete repairs overrides, undo restores them, and portable save omits them. |
| `WorkingState.inspect_mask` | Optional occurrence whose mask is inspected. | Omit from portable artwork and output pixels. |
| `WorkingState.generation` | Runtime working-state generation. | Reject stale working requests without treating navigation as an authored edit. |

`DocumentNames` supplies localized initial names. Created occurrence names are
literal authored strings, not live localization keys. A new drawing has a white
Solid Color effect named Paper at the bottom, without a mask or special layer
restrictions. Window titles, locations,
permissions, tab order, camera, tool settings and view toggles remain with their
shared session or workspace owners.

### Composition, stacks and output

| Field | Meaning, default and units | Required assertion |
| --- | --- | --- |
| `Artwork.id` | Drawing portable identity; new unique ID. | Preserve for the drawing lifetime; opening the same ID twice does not share mutable stores. |
| `Artwork.root`, `default_output` | Typed composition and output handles. | Resolve portable references once on decode; handles never become portable identities. |
| `Composition.size`, `origin` | Positive integer frame extent and explicit origin in local pixels; origin initially zero. | Crop/grow changes the frame without discarding out-of-frame source pixels. |
| `Composition.color` | Working RGB primaries and committed sample depth; default sRGB/U8. | Preserve U8/U16/F16/F32 codes and independent imported interpretation. |
| `Composition.blend` | Blend domain, initially Linear. | Perceptual and Linear retain their evaluation rules and admission restrictions. |
| `Composition.resolution` | Optional exact positive rational physical density on both axes, with inch, centimetre or metre units. | Round-trip rational pairs and units without normalization; changing resolution alone does not invalidate pixels. |
| `Composition.result`, `Stack.entries` | Typed result stack and front-to-back occurrence order. | Preserve membership, including empty stacks; no parallel flat order or authored parent field. |
| `Output.composition`, `name` | Output source composition and literal UTF-8 name. | Preserve output identity independently of presentation name. |
| `Output.context` | Explicit elapsed time and effective effect phases. | Evaluate captured source roots at these phases, never reconstruct phases from elapsed time and the latest rate. |
| `Output.frame`, `scale` | Optional output frame and positive delivery scale, initially absent and `[1,1]`. | Output framing is independent of composition and source domains. |
| `Output.proof` | Optional proof intent with immutable profile resource. | Preserve name, profile, intent, black-point compensation, paper and black-ink simulation; temporary proof toggles are omitted. |
| `Output.sdr` | Exposure, contrast, headroom, highlight-color fraction and balance; defaults `0`, `1`, `2.3004484`, `0.3`, `0`. | All five values survive save/reopen; screen capability never changes authored intent. |
| `Artwork.metadata` | Exact opaque Exif, XMP and IPTC blocks, initially absent. | Preserve metadata unrelated to output; placing another image does not replace drawing metadata. |
| `Artwork.extensions` | Retained ancillary records and immutable resource ranges. | Apply copy-safety and reference-closure rules without losing original encoded bytes. |
| `Guides.rulers` | Stable portable ruler IDs and Straight, Parallel or Radial geometry in composition pixels, initially empty. | Round-trip and undo preserve geometry and identity; stroke constraints remain transient snapshots. |

### Occurrences and paint sources

| Field | Meaning, default and units | Required assertion |
| --- | --- | --- |
| `Occurrence.content` | Paint-source use, nested stack, effect application or saved selection. | Groups, effects and selections do not acquire fake paint sources. |
| `Occurrence.name` | Literal UTF-8 name supplied at creation. | Rename preserves identity and does not invalidate pixels. |
| `Occurrence.visible` | Contribution visibility, initially true. | Hiding contribution does not disable a source demanded by an explicit input. |
| `Occurrence.opacity` | Finite contribution factor `[0,1]`, initially 1. | Affect only this occurrence and retain pass-through interpolation. |
| `Occurrence.blend`, `clipped` | Blend operation and clipping membership, initially Normal and false. | Clipping bases follow stack position; preserve the clipped pass-through rule. |
| `Occurrence.translation` | Translation in enclosing-stack pixels, initially zero. | Preserve inherited group offsets, including mask placement. |
| `Occurrence.placement` | Local retained projective placement, optional cubic mesh and interpolation; initially identity, no mesh, Linear. | Retain analytic geometry; omit generated tessellation and GPU buffers. |
| `Occurrence.locked`, `alpha_locked` | Editing locks, initially false. | Derive ancestor locks; valid undo restores records without changing source sample identity. |
| `Occurrence.reference` | Authored reference designation, initially false. | Preserve independently of visibility and rebuild reference scopes after grouping or reorder. |
| `Occurrence.mask` | Optional `MaskUse` in the occurrence's mask slot. | Source edits differ from use enablement, linkage, inversion and placement edits. |
| `PaintSource.domain` | Explicit local pixel domain. | Canvas shrink does not shrink the source; domains and occurrence placement remain independent. |
| `PaintSource.raster` | Immutable sparse revision, initially empty, with color and material planes. | Preserve tile codes and unchanged compressed bytes; missing overrides reveal the imported base. |
| `PaintSource.original` | Optional immutable imported base, absent for new paint. | Preserve Original/Rasterized role, extent, density, profile, assumed-profile flag and samples separately from overrides. |
| `PaintSource.operations` | Accepted transient raster commands and immutable inputs. | Package preparation refuses unfinished commands; represented pending revision promises may be retained. |

Composition frame, source domain, occurrence placement and output frame are
independent. Source domains are explicit and do not fall back to canvas size.
Whole-tile rebasing changes local coordinates and placement in one transaction
without losing hidden tiles. Apply Transform to Pixels publishes new source roots
and corresponding placement together.

### Coverage, mask use and saved selections

| Field | Meaning, default and units | Required assertion |
| --- | --- | --- |
| `CoverageSource.domain` | Explicit local pixel domain. | Preserve coverage outside the composition frame. |
| `CoverageSource.raster` | Sparse immutable scalar revision, initially empty. | Missing tiles retain declared initial/default coverage; coverage is not image alpha or luminance. |
| `CoverageSource.initial` | Optional retained selection geometry or scalar resource, initially absent. | Preserve contour even/odd rule, affine map, inversion and pixel coverage without rasterizing contours at save. |
| `CoverageSource.default_coverage` | Finite scalar `[0,1]`, initially 1 for Reveal All. | Freeze missing-tile and out-of-bounds coverage independently of use inversion. |
| `CoverageSource.operations` | Accepted transient coverage commands and retained inputs. | Omit commands from portable data; retain the committed scalar revision promise. |
| `MaskUse.source` | Typed coverage handle; use identity is occurrence plus mask slot. | Unlink/relink preserves coverage-source identity. |
| `MaskUse.enabled`, `linked` | Application and placement linkage, initially true. | Disable preserves source data; toggling linkage preserves displayed coverage. |
| `MaskUse.translation`, `placement` | Translation in the defined parent domain and independent projective geometry, initially zero and identity. | Preserve unlinked placement and owner pre-maps through projective/mesh placement. |
| `MaskUse.inverted` | Use inversion, initially false. | Apply at its declared stage; initial-selection inversion stays separate. |
| `SavedSelection.selection` | Authored `Selection` geometry or immutable pixel coverage. | Preserve independently of current working selection; do not composite exported color. |
| `SavedSelection.display` | Overlay color and opacity, default sRGB red `[1,0,0,1]` and `0.5`. | Preserve named selection display properties while omitting overlays from output. |

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
Rasterized images retain explicit working RGBA interpretation, matching committed
depth and profile with `profile_assumed` false. ICC bytes are
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

A resource identity comprises immutable owner and block identity. Loaded tile
cache keys use that identity; generated tiles may use their pixel descriptor and
compressed fingerprint prepared on the compression worker. Both include sample
interpretation and require no decoded hashing on opening or input. A portable
resource ID, byte offset, checksum or `Arc` address alone does not establish
identity across independent opens. Unchanged resources keep their runtime identity
across captures, independent of archive layout. Integrity checks and optional
strong deduplication remain separate from editable source identity.

### Effects

| Field | Authoritative owner and defaults | Required assertion |
| --- | --- | --- |
| `EffectApplication.definition` | Typed reference to an immutable authored definition. | Capture/undo preserves one definition owner; application and definition identities remain distinct. |
| `EffectApplication.values` | Values in validated compact ABI slots, addressed externally by stable parameter keys. | Wire decode maps keys once; control rename/reorder never retargets values. |
| `EffectApplication.domain` | Explicit local pixel domain. | Preserve independently of the composition frame. |
| `Definition.program` | Shared immutable `EffectProgram`. | Saving retains its owner rather than detaching or mutating program metadata. |
| `EffectParameter.dimension` | Semantic dimension stored with the parameter: Scalar (default), Angle, Time, SourcePixels, CompositionPixels or Normalized. | Catalog insertion, package I/O and resize use the same declaration; display units never control resizing. |
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

`Document::validate_integrity` checks retained payloads and resource representations;
`Document::admit` checks execution limits, and `validate` combines both. Native
`ImportedDocument` candidates retain their original backing, output inventory and
verified preview through color and renderer preparation. Unsupported execution
returns Preserved; invalid required data returns Recovered view or Failure.
Cancellation remains cancellation and cannot publish either an editor or a view.

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

`SourceTarget` selects a typed paint, coverage or saved-selection destination.
`WorkingState.occurrence` supplies the occurrence context separately. Stroke
admission resolves group locks, mask linkage, coordinates and source generation
once. The source handle selects mutable backing;
the occurrence context selects placement and editing policy. A query for an
occurrence is not a query for its raw source. Deleted or changed targets are
revalidated at a contact boundary, never silently rerouted mid-stroke.

[`Edit`](../../crates/layer-core/src/lib.rs) has one `RecordChange<T>` variant per
authored store, `Working`, `SetRaster` and an atomic `Batch`. A record change binds
a typed handle to its portable ID and an optional replacement; absence removes
the live value. `Document.apply` validates the final candidate and returns its
inverse. Structural batches rebuild topology only after every record change;
raster and parameter changes validate payloads without rebuilding topology.
`Edit.changes_image(&Document)` distinguishes pixel changes from names, locks,
reference designation and physical resolution.

Inverse edits retain typed records, source roots, parameters and affected working
state.
Deletion explicitly chooses whether to remove exclusive content with its use;
shared or intentionally unplaced content cannot disappear as a side effect.
Duplicate/paste remaps selected authored identities and visible bindings, while
sharing immutable resources. Unsupported preservation records are not an implicit
license to clone content whose copy contract is unknown.

Undo/redo restores the same handles and immutable roots, with admission in both
directions. It does not reconstruct a picture or allocate replacement identities.
[`RootInventory`](../../crates/layer-core/src/lib.rs) traverses immutable rasters,
originals, selections, LUTs, meshes, profiles, programs and ancillary package
backing, including retained command inputs. Ancillary descriptors and resident
package owners are counted once across shared resources and retained inventories.
The package byte source reports resident ownership without reading ranges; file
backing without a memory cache reports zero resident payload bytes. Current ownership accounting deduplicates shared backing held by
live stores and snapshots. History admission reserves both forward and inverse
ownership after pending producers release temporary inputs; a bake retaining old
inputs cannot hide the eventual cost of undo. Removing the last rendered use does
not release a source still held by a bake, contact-start read, history or save.

`RasterRevision` and `RasterTile` remain immutable single-publication promises.
Capture can retain pending promises; workers await them off UI/input/render
submission paths. Errors remain visible to every owner. A consumer's cancellation
releases its ownership but cannot cancel a producer needed by another consumer.
Prediction and late correction publish a later root and cannot mutate a root
already captured for save.

## Scene and query boundary

A borrowed typed scene view exposes composition, stacks, occurrences, source roots,
effects and output context to the common evaluator. An owned scene snapshot may
share that metadata and immutable roots across worker boundaries. Both forms
access the authored stores directly. A warmed source update
changes source generation and damage, with no authored topology rebuild or new
lowering pass. Structural publication atomically supplies new topology and indexes.
A worker that missed its baseline or recreated its device requests a full snapshot.

`Document.scene()` supplies `SceneView` over authored stores and the current
`SceneIndex`, bound to runtime owner and revision. `Document.snapshot()` uses the
default output context; `snapshot_with_context` supplies an explicit captured
context. `SceneSnapshot` retains artwork, index, owner, revision, context, scope
and evaluation offset, excluding working selection and mask inspection.
`SceneScope` selects All, Raw source, Members or a Prefix before an occurrence.
Member scopes preserve original placement ancestry while evaluating the selected
contributors through their scoped parents; they do not copy or mutate occurrences.

Queries address raw sources, placed occurrences, scalar coverage, stack prefixes,
effect inputs or outputs against an immutable scene revision and evaluation
context. Reference queries retain their scope and original occurrence records.
Effect comparison retains an `EffectApplication` value snapshot. Bake/merge
retains the prior evaluated scope and source roots through
destination publication, even after source occurrences leave the live stack.

`ArtworkQuery` and source-analysis keys identify typed effect input scope,
contributing source generations, topology, definition/parameter dependencies,
ancestor placement and captured phases. Occurrence names and control presentation
order do not invalidate pixels; authored stack order remains an evaluation
dependency. A changed upstream source or effective phase invalidates analysis. Captured phases
take priority over elapsed-time fallback. Raw source queries exclude effect phases
from their pixel dependencies.
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

[`Editor.capture(session_generation, EvaluationContext)`](../../crates/layer-core/src/lib.rs)
returns [`ArtworkCapture`](../../crates/layer-core/src/authored/artwork.rs) with
shared immutable artwork and the captured default output context. Its
[`CaptureCheckpoint`](../../crates/layer-core/src/authored/artwork.rs) binds `owner`, `document`, `session_generation`,
`artwork_generation`, `working_generation` and `edit_checkpoint`.
[`CanvasEngine::capture_artwork`](../../crates/layer-engine/src/canvas.rs) and the
host capture barrier supply the context from the matching successful source
submission. Capture filters removed effects from the supplied phase inventory;
it never reconstructs integrated phases. The caller need not clone history.
Resources are enumerable directly from the capture, independently of ZIP writing.
A future private session/history codec can capture working state and bounded
history at this same ordered boundary without extending the portable manifest.

The output context pairs source roots, effect phases and source-analysis inputs
from one committed boundary. It includes composition/output identity, framing,
working/blend interpretation and delivery intent. Paper is an ordinary Solid Color
effect occurrence; selection and mask-inspection overlays are excluded. Preview generation is optional and
uses this exact context. Admission compares both the capture checkpoint and its
complete evaluation context, including phases when no edit has occurred. Preview
failure cannot discard successfully captured source, acknowledge a newer
checkpoint or require converting source to a bitmap.

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

[`artwork_records`](../../crates/layer-core/src/package/artwork_records.rs) and
[`ResourceInventory`](../../crates/layer-core/src/package/resources.rs) select all
authored roots, including hidden and unplaced content. History/session traversal
uses the same immutable owners but selects its own roots.
[`PreparedTransfer`](../../crates/layer-core/src/package/transfer.rs) reuses those
wire adapters to expose metadata and bounded resource payload chunks without ZIP
assembly. Its transient typed-store layout preserves handle slots, including
tombstones, across worker heaps; that layout does not enter a portable package.
Optional working selection uses the shared selection wire adapter outside the
artwork manifest. Verified adoption installs immutable tile, lookup and selection
owners without decoding, hashing or color management on the UI thread. Ancillary
references can retain original encoded bytes while sharing one resource ID with
live authored content.

Current recovery writes captures through `PreparedPackage`; enumerating metadata
and unchanged resources does not require constructing or reopening an archive.
Private session restoration, persisted inverse edits, incremental storage and
recovery lifecycle changes belong to the later
[automatic-recovery work](../development/autorecovery.md).

## Regression oracles

These tests define the contracts. Their presence does not establish a passing run
or qualify host behavior and performance.

| Boundary | Existing oracle |
| --- | --- |
| Bake lifetime and composition | [`merge_tests.rs`](../../crates/layer-core/src/merge_tests.rs) and [`scene/stack.rs`](../../crates/layer-render-wgpu/src/scene/stack.rs) |
| Shared roots, selection and history admission | [`history_budget/tests.rs`](../../crates/layer-core/src/history_budget/tests.rs), [`retained_geometry_tests.rs`](../../crates/layer-core/src/retained_geometry_tests.rs) and [`raster/restore_tests.rs`](../../crates/layer-render-wgpu/src/raster/restore_tests.rs) |
| Portable codec, verified transfer and exact preview context | [`package/codec/tests.rs`](../../crates/layer-core/src/package/codec/tests.rs), [`package/transfer.rs`](../../crates/layer-core/src/package/transfer.rs) and [`authored/tests.rs`](../../crates/layer-core/src/authored/tests.rs) |
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
