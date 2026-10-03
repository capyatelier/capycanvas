# File format and authored graph implementation plan

[Developer guide](README.md)

Adopt the [file format foundation](../history/capy-format-foundation.md) with
ordinary layer compositions as the first supported subset of one authored graph.
Use the same semantic objects in the file and shared editor: compositions,
structured stacks, occurrences, editable sources and effect applications. Resolve
portable IDs to compact runtime handles at the boundary; derive execution plans
without constructing a second editable document.

This selects an implementation direction, not a finished byte specification or a
performance result. The code assessment uses `0f6b5b708`. The prototype and
qualification gates below remain required. The
[node research](../history/authored-graph-research.md) owns the broader artist
workflows. The foundation owns ZIP, checksums, compatibility and preview rules;
this plan resolves their relationship to the editor and renderer.

## Scope and commitments

Keep the restricted ZIP64 package, JSON object/resource tables, 256-pixel lossless
raster encoding and optional preview of the exact saved snapshot. The first
implementation preserves existing artwork, tools, history and incremental
painting. It adds neither a node editor nor arbitrary graph editing, shared-content
commands, animation timelines, simulation or a universal channel model.

The first stable artwork schema must already distinguish a source from its uses,
stack order from other dependencies, and semantic identity from payload identity.
Later graph features extend those objects and interfaces; they do not replace the
container or make the original stacks obsolete. Unsupported future content uses
the foundation's preservation/preview path, never a lossy conversion to layers.

Changing current compositor structures is allowed where it removes repeated work
or aligns ownership. Preserve its kernels, precision, damage tracking, streaming,
fast paths and publication ordering until replacements pass equivalent tests.
Do not persist a GPU execution graph to avoid a small amount of metadata lowering.
The baseline needs typed source/occurrence ownership and a stack evaluator, not a
general node scheduler. Keep unrelated compositor optimization out of the format
cutover; require it only where the new model cannot meet the acceptance gates.

## One semantic model from file to editor

A flat wire table is an addressing convention, not a requirement to store runtime
objects in a hash map or serialize their memory layout. Table position has no
meaning. Known records decode into typed shared Rust values with the same ownership
and relationship rules used by editing. Use dedicated wire adapters for portable
spelling, default omission, references and versioning; do not maintain an editable
wire document beside an editable runtime document.

### Baseline objects

Names here describe roles; registry spelling is fixed with the schema fixtures.

| Role | Authoritative state and relationships |
| --- | --- |
| Composition | Local frame, origin, physical resolution, working color/depth and default blend domain; a reference to its result. Today the result is a stack. A nested independently framed composition can be introduced later. |
| Stack | One ordered list of occurrence references, front to back, evaluated bottom to top. Ordinary groups are structured stack uses in the enclosing composition, not newly allocated full-canvas images or independent color domains. |
| Occurrence | Identity of one layer-panel entry: name, contribution visibility, locks, placement, blend, opacity, clipping membership and optional mask use; refers to content or a structured operation. Each occurrence has at most one containing stack. Reuse creates another occurrence rather than inserting the same occurrence twice. |
| Paint source | Editable source identity, local pixel domain, immutable imported base when present, sparse painted overrides, material planes and live material settings. Refers to immutable typed resources; a source edit publishes a new revision without changing source identity. |
| Coverage source | Contours or declared initial/default coverage plus sparse painted overrides, with its own domain and resources. A mask use owns enabled/inverted/linked state and placement. Keep scalar coverage distinct from an image's alpha and from luminance extraction. |
| Effect application | Identity, definition reference, values keyed by stable parameter keys, resource bindings and typed evaluation inputs. Definition source/ABI and parameter layout are shared immutable data. A stack adjustment has a defined scoped input, not an invented independent image. |
| Saved selection and guides | Independently retained authored data. Saved-selection rows may occupy non-compositing stack entries without becoming fake paint sources. Rulers and reference markings keep their current meaning. |
| Output | Identified composition result, evaluation context, framing and delivery intent; optional saved representation. The ordinary document has one canvas output. The envelope retains the foundation's support for future empty or multiple output inventories. |

The composition boundary separates the current canvas domain from a stack's
ordering. Replacing a stack result with a more general network later does not
move color, units or framing between owners. Do not add a generic graph-container
object merely to wrap every stack. Transforms, colors and a single mask-use record
stay inline unless a feature needs independently addressable identity.
Address today's mask use by occurrence plus a stable mask-slot key; its coverage
source has separate paint-target identity. No additional wrapper node is needed.

**Split source and occurrence now.** This replaces the foundation's earlier
proposal to defer the split until the first sharing feature. Current layers
combine `raster`, `source`, properties, masks and effects in
[`Layer`](../../crates/layer-core/src/lib.rs); render targets also use `LayerId`.
Moving just the file to separate sources while repeatedly reassembling `Layer`
would add work and conceal which identity a brush mutates. Make the split at the
shared model boundary and carry it through the renderer contract.

The initial editable subset has at most one occurrence per editable source.
Independent Duplicate creates new authored source and occurrence IDs but shares immutable
backing until edited. Future linked uses share source identity intentionally.
An immutable resource ID, `Arc` identity, checksum or equal pixels never implies
that two sources receive the same edits. Unsupported shared uses remain preserved
and read-only until the editor implements their editing contract. The baseline
reference semantics already permit multiple uses; distinguish a valid graph
outside the editor's supported subset from malformed artwork. Support is derived
from the actual relationships, without a second required-capabilities inventory.

A paint source is not simply RGBA tiles. Preserve Original/Rasterized image roles,
independent profile interpretation, tile-presence override rules, watercolor
wetness and edge settings, and pixels outside the canvas. Missing paint overrides
reveal the imported base; missing mask tiles use the declared initial/default
coverage. The behavior in
[`source_access.rs`](../../crates/layer-render-wgpu/src/source_access.rs),
[`raster.rs`](../../crates/layer-core/src/raster.rs) and
[`SourceImage`](../../crates/layer-core/src/color/source.rs) must survive without
baking the original and overrides together. Fills, merges, destructive transforms
and contacts remain transient commands whose committed result is raster state.

### References and interfaces

Use the foundation's reserved visible reference form for every cross-object or
resource reference. A connection endpoint adds a stable output-port key, for
example `{"object":{"ref":"source-id"},"port":"color"}`; a consumer input
is identified by its own stable key. A resource reference is not an execution
edge. Stack membership is not transform parenting. A mask use is not ownership
of another layer's source. Known types validate each relationship separately.

A stack binds its defined backdrop and clipping inputs from its ordered entries.
Persist that structured scope once, then resolve it during lowering. Do not also
save the derived explicit edges, a clipping-base ID and a second parent/order
array. A future explicit shared matte is a separate named connection: it stays
attached when entries move, while ordinary clipping follows current stack rules.

Baseline ports distinguish color results, scalar coverage and stack operations
that consume a backdrop. Their types specify bounds, coordinates, sampling and
color/alpha contracts. They need not encode arbitrary vector, text or material
values now; new typed sources and operations add those later. Absence of a port
must have a frozen type-defined default or be invalid, not silently select the
first output. Unknown ports or semantic values are unsupported content.

Effect parameter keys and Choice values are durable; GPU offsets are not. Load
keyed values once into the definition's compact slot layout, then reuse it during
painting. The embedded program's positional ABI must agree with that layout;
renaming/reordering UI controls cannot reorder shader slots. Preserve program
sharing across load, save and undo. Today's
[`EffectInstance`](../../crates/layer-core/src/effects.rs) already has parameter
keys but stores values and Choice selections positionally. Its `scaled_px` uses a
unit label; the new type must give lengths semantic dimensions/reference spaces.
Freeze evaluation semantics separately from the current shader ABI and UI schema.
The current filter ABI is not a general multi-input node ABI. New definitions can
declare additional typed ports without changing existing effect meanings or
requiring all old programs to be rewritten.

Definitions and binary/code resources refer to local slots bound through visible
references. Copy/paste remaps bindings, never IDs hidden inside shader text. Future
instances expose stable input/output and parameter keys; overrides address those
interfaces. Paths cross instance boundaries, not every grouping edge. Keep base
values literal and add future bindings as records as specified in the foundation.

Reject cycles in instantaneous evaluation and recursive definition expansion.
Do not apply this DAG rule indiscriminately to all resource, membership or
ancillary references. Bound nesting, edges and expansion work as well as object
counts. Editing validates the final transaction before publishing it.

### Current composition semantics to freeze

Reuse the contracts in [`layers.rs`](../../crates/layer-core/src/layers.rs) and
[`stack.rs`](../../crates/layer-render-wgpu/src/scene/stack.rs):

- Isolated groups evaluate over transparency. Pass-through groups receive a
  backdrop `B` and produce `B + opacity * mask * (group(B) - B)` in the applicable
  blend domain. Clipped pass-through groups stay isolated Normal groups.
- Adjustments transform their scoped lower composite; clipped adjustments operate
  on the clipping stack and preserve base coverage. Hiding a direct contribution
  must not disable a source that a future explicit input still needs.
- Mask source evaluation, inversion, placement and application stage are distinct.
  A linked mask follows the owner's source mapping, including its pre-map under
  projective/mesh placement. Preserve default coverage and out-of-bounds behavior.
- Store retained placement once on the occurrence. Do not add another transform
  node with the same matrix. Existing cubic meshes remain semantic geometry;
  generated tessellation and GPU buffers are derived.
- The source-local domain, composition frame and output crop are independent.
  Preserve crop/grow, whole-tile rebasing, group translations, hidden pixels and
  Apply Transform to Pixels behavior in
  [`canvas_geometry.rs`](../../crates/layer-core/src/canvas_geometry.rs).
  Regrouping cannot silently change the coordinate domain of a target.
- The initial composition may require editable paint to match its working format,
  as today. That restriction belongs to its type/validator, not the package or
  resource table. Keep imported originals independent. Future mixed working
  contexts require explicit conversion boundaries, not reinterpretation of bytes.
- Paper is authored content with defined stack behavior, not viewer chrome. Empty
  stacks remain valid. Mask inspection and selection overlays never enter saved
  output previews. Session selection/active target remain separate from artwork.

## Runtime structure and performance boundaries

### Engine alternatives

All three structures below can consume the same authored types and ports. The
file must not encode the execution representation that selects among them.

| Structure | Fit and decision |
| --- | --- |
| Typed arena with direct demand traversal | Compact handles and typed operations can extend today's stack visitor into a demand evaluator. Simple first prototype; repeated traversal, scope discovery and scratch planning must be cached outside motion. No file change is needed if this proves sufficient. |
| Typed authored arena plus retained indexed execution plan | Preferred implementation route. Lower structured stacks into compact operation/operand arrays and reverse dependencies; update source/parameter records separately. Reuse today's kernels and retain stack fast paths. Adds derived scheduling data, not another authored graph. |
| Typed authored arena plus task/region execution graph | Viable later when fan-out, several outputs and expensive spatial nodes justify finer scheduling. Tasks are keyed by operation, context, region and quality, with bounded queues and lifetimes. More complex; do not build a general task framework now. The same authored file remains sufficient. |

Keeping today's `Vec<Layer>` authoritative and adding a separately mutable node
model is rejected: it duplicates edits, undo and serialization rules. Interpreting
JSON/string references on every frame is also rejected. A universal field or
simulation runtime would enlarge scope without solving today's persistence work.

### Compact handles and retained plans

Use opaque 128-bit portable IDs in the file; decode them to fixed-size IDs, and
resolve links once into typed compact handles in a shared object arena. Handles
must remain stable while referenced, or use generations to reject stale reuse.
Keep portable identity available for saving and duplication. Dense positions,
runtime allocators and generations are not file identities.

The arena holds typed payloads and shares immutable large values: raster roots,
source images, effect definitions, selections and meshes. Build parent, sibling,
clipping, drawing-target and reverse-dependency indexes when their relationships
change. Do not repeat today's linear `layer(id)`/parent scans through an added
UUID map at each dab, tile or graph edge. Do not introduce a new collection or
persistent-data-structure dependency without demonstrating that existing Rust
containers and shared handles are insufficient.

The first renderer cutover should accept a borrowed typed scene view with compact
source/occurrence handles. Adapt the common stack visitor to that view, so the
exact and display evaluators share ordering/scope semantics. A one-time adapter
is useful for the prototype oracle; a per-frame `graph -> Vec<Layer> -> graph`
conversion is not an acceptable production boundary. Retire superseded authority
and traversal paths at cutover.

Current [`Graph::prepare`](../../crates/layer-render-wgpu/src/scene/scale/graph.rs)
reconstructs `Arc<Expression>` trees, hashes recursive structures, discovers cache
candidates and computes damage/cost. `scratch_images` also derives expressions.
This is existing work, not proof that new authored nodes would be free. Retain
indexed topology and structural plans across paint frames. Update revisions and
regions without rebuilding the authored topology or recursively hashing it.

Keep different change classes explicit:

| Change | Required work |
| --- | --- |
| Dabs, correction or new raster root | Update one source generation and changed regions; invalidate all dependent contexts. No topology rebuild or shader compile. |
| Opacity, color or ordinary effect value | Update the addressed parameter/uniform and relevant dependents. Retain compatible programs and source caches. |
| Sampling radius, extent, interpolation, placement or blend contract | Recompute affected support, bounds, context or plan specialization; invalidate old and new footprints. This is more than a uniform change even without new edges. |
| Reorder, connect, group or change program | Rebuild affected topology/scopes and prepare any changed pipelines off the input path. Publish only a current, ready candidate. |
| Name, node position or active target | Update editor/view state without pixel invalidation. An artwork rename still participates in file modification/history policy. |

Per-frame scheduling may change requested tiles, quality and cache allocation.
Avoiding topology rebuilds does not prohibit that bounded demand work. Preserve
balanced source-over branches, pointwise fusion, direct transform presentation,
scalar mask paths and native precision. A logical transform or blend does not
require an intermediate texture. Use lifetime-aware reuse, bounded scratch and
halos; never allocate a canvas-sized image for every authored node or port.

Extend the ready/pending/error and candidate-publication paths in
[`effect_validation.rs`](../../crates/layer-render-wgpu/src/effect_validation.rs)
and the startup compiler. Synchronously requesting an unfinished
[`Deferred`](../../crates/layer-render-wgpu/src/deferred.rs) can still block or
compile immediately. Prepare a graph candidate's programs and required resources
off the input path, pair completion with its revision, and preserve the existing
accepted scene until publication. Undo or a newer edit must retire a stale
candidate without installing its pipelines/state.

### Ownership across frames and workers

[`FramePacket`](../../crates/layer-render/src/lib.rs) currently borrows layers and
dabs. [`frame_layers`](../../crates/layer-engine/src/canvas.rs) makes a metadata
copy for a temporary effect or pending bake; the GTK
[`Frame`](../../apps/layer-linux/src/render_thread.rs) copies layer/dab records
into a mailbox bounded to two frames. Raster and imported-source payloads are
shared. [`artwork::Frame`](../../crates/layer-render-wgpu/src/artwork.rs) retains
another composition snapshot for queries. Do not describe these as pixel copies,
but do not multiply them by adding a wire or graph reconstruction step.

Prefer retaining immutable render topology across submissions. Pass bounded
source-root, parameter and transient-input updates with a scene generation; the
worker keeps its derived view and validates ordering. Structural publication supplies the new
view atomically. Device recreation or a missed baseline requests a complete
snapshot. A full snapshot remains allowed at load, save/export and structural
boundaries; ordinary paint and parameter motion must not clone the complete
authored object table. A compact owned render snapshot can remain where it shares
payloads and passes the allocation and latency gates more simply than a delta
protocol. Copying bounded dab records across owners is still necessary. Choose
one transport per ownership boundary; do not maintain redundant live replicas.

Save/export snapshots may walk the object set and clone small records/shared
handles once. The writer visits them through wire adapters without mutating a
clone to detach binary payloads. In particular,
[`ResourceIndex::detach`](../../crates/layer-core/src/project_storage/resources.rs)
currently uses `Arc::make_mut` on effect instances/programs and parameter arrays;
replace that save-time transformation with direct resource enumeration. Do not
make every live edit copy a whole `Arc<Document>` or tile map just because a file
worker holds a snapshot. Measure metadata snapshot cost before choosing more
complex persistent containers.

### Source identity and evaluated identity

Replace overloaded layer/mask target IDs with explicit source targets and
occurrence context. A brush resolves a drawing target once through shared tool
rules; it writes a source in its local coordinates. Captures, restoration and
history follow that source revision. Queries may request the source, a placed
occurrence, a stack prefix or an output; these are different results.

Today [`Sources::prepare`](../../crates/layer-render-wgpu/src/scene/scale/sources.rs)
keys by `LayerId`, admits visible layer sources and folds some mask, blend and
material interpretation into their levels. Future sharing requires:

- Shared immutable decoded samples keyed by resource identity and interpretation.
- Mutable paint/material backing and publications keyed by source identity.
- Evaluated results keyed by operation/occurrence, semantic and input revisions,
  composition/color/alpha context, coordinate grid, time and quality.

Do not share evaluated pixels merely because the source ID matches. Two uses can
have different transforms, masks, backdrops or working spaces. Conversely, do not
upload or capture the same source separately for every occurrence. Visibility
filters contributions; demanded dependency closure determines evaluation. A
hidden matte still supplies all consumers. Decode/cache keys must not keep
retired document/history roots alive; preserve the intent of
[`Metadata`](../../crates/layer-render-wgpu/src/scene/metadata.rs).

Demand travels backward separately for each input, including filter halos and
transform sampling footprints. Damage travels forward to every consumer and
unions old/new footprints. Current stack-wide radius sums and scope searches in
[`windows.rs`](../../crates/layer-render-wgpu/src/scene/windows.rs) are safe
baseline specializations, not a general fan-out algorithm. Cache eviction and
multiple viewers share accounted budgets; a new output cannot silently acquire
another full display-cache allowance.

### Transactions and other consumers

[`Editor`](../../crates/layer-core/src/lib.rs) stores inverse edits; batch rollback
and some history admission clone document metadata. Preserve the one-edit atomic
contract while addressing objects and source revisions by stable identity.
Structural changes validate all affected references, support inverse edits and
publish with the matching renderer generation. Undo restores source and parameter
roots, not a reconstructed picture. Restore both directions' memory admission and
shared-ownership accounting in
[`history_budget.rs`](../../crates/layer-core/src/history_budget.rs).

Deletion is a typed edit, not garbage collection from rendered outputs. Removing
an occurrence cannot delete shared content still used elsewhere; removing its
exclusive content can be part of the same explicit transaction. Intentionally
retained unplaced content stays in the object table. Duplicate/paste remaps the
selected authored identities and bindings, while sharing immutable resources.
Retain roots needed by undo, queued jobs and snapshots until their owners release
them. Read-only preservation of unknown records is not permission to clone them.

Pending publications remain immutable promises shared by document, history and
save. [`reconcile_rasters`](../../crates/layer-render-wgpu/src/raster.rs) and
[`NativeCapture`](../../crates/layer-render-wgpu/src/raster/deferred.rs) must still
publish once, preserve errors, reserve pending bytes and prioritize presentation.
Cancellation of one consumer cannot abandon a source another consumer owns.
Saved checkpoints identify the captured edit, not whichever scene is current
when asynchronous encoding completes. Prediction and the late-correction window
remain transient; a correction cannot mutate an older saved snapshot.

These consumers must adopt the same scene/source contracts at the baseline
cutover, rather than flatten arbitrary graphs into temporary layer documents:

- **Merge, flatten, stamp and Apply Transform:**
  [`merge.rs`](../../crates/layer-core/src/merge.rs) retains bake members after
  removing their visible entries. Keep a snapshot of the old evaluated scope and
  its source roots until the destination publication finishes. Derived render
  membership must not be their lifetime owner.
- **Retouch and smudge:**
  [`retouch_sources.rs`](../../crates/layer-render-wgpu/src/retouch_sources.rs)
  freezes stroke-start pages and reference composites, with bounded prefetch and
  deferred replay on misses. A read of a prior source generation is not an
  instantaneous self-cycle. Keep the selected occurrence's coordinates and
  explicit read generation; defer structural changes at the contact boundary.
- **Selection, color sampling and statistics:**
  [`RegionSource`](../../crates/layer-render/src/lib.rs) includes raw layer,
  coverage and copied-layer queries. Replace copied layer lists with typed query
  targets and an immutable scene revision. Preserve raw versus adjusted versus
  displayed color, stack-prefix scope and mask coverage semantics.
- **Thumbnails, Navigator, filter previews and export:**
  [`thumbnails.rs`](../../crates/layer-render-wgpu/src/thumbnails.rs),
  [`scale/navigator.rs`](../../crates/layer-render-wgpu/src/scene/scale/navigator.rs)
  and [`SnapshotRenderer`](../../crates/layer-render-wgpu/src/snapshot.rs) must use
  explicit targets/contexts and shared backing. Exact export and sampled queries
  cannot use a coarse display cache as authoritative pixels. Optional previews
  yield to painting and do not each retain an unbounded scene copy.
- **Color/profile and canvas operations:**
  [`color_edit.rs`](../../crates/layer-core/src/color_edit.rs) and
  [`color_transition.rs`](../../crates/layer-engine/src/color_transition.rs)
  publish backing, interpretation, history and prepared GPU state together.
  Preserve that transaction. Future shared sources need an explicit make-unique
  or all-uses edit policy before enabling operations that currently rewrite one
  layer's domain or precision.

Current effect playback uses a stateful
[`EffectClock`](../../crates/layer-core/src/effects.rs). Do not equate its session
elapsed time with a future seekable timeline. Keep its baseline behavior defined;
preview capture must freeze the effective evaluation state as well as source
roots. Specify opening phase, effective phase after rate edits and the saved
output context in baseline fixtures; elapsed time alone cannot reproduce an
integrated clock. Later timeline types add exact time, instance time mapping and
seeking contracts without putting frame numbers into source identity.

## Storage and host blockers

### Lazy backing requires more than archive offsets

The current reader constructs `TileBlob::from_compressed`, which decodes and
checks decoded hashes. The writer deduplicates by that hash, and the renderer's
[`DecodedTiles`](../../crates/layer-render-wgpu/src/scene/sources.rs) uses it for
raster cache identity. Simply replacing the file digest with a CRC would either
force eager decoding to reconstruct the old key or make collisions unsafe.

Use retained resource/block identity plus interpretation for loaded-byte reuse.
Scope identity to an immutable backing owner/generation; two independently opened
files containing the same declared IDs are not proof of identical bytes.
Newly encoded identical blocks may be deduplicated with a strong hash or byte
comparison; CRC-32 is only stored-byte integrity. No decoded content hash is
required to install a resource handle. Preserve the same compressed LZ4 bytes
through save, including their shuffle/layout contract. Avoid a file-specific tile
object that must be copied into an otherwise identical renderer tile.

Extend the existing backing abstraction in
[`raster_storage.rs`](../../crates/layer-core/src/raster_storage.rs) to reference
immutable package ranges with a retained owner and bounded asynchronous reads.
Its current external read copies a slice into a new `Arc`, and its native disk
read seeks synchronously; neither is a promise of zero-copy access. Prefer an
owned shared byte range when possible, without requiring memory mapping on every
host. Never decompress the ZIP pack around independent LZ4 tiles.

`raster_restore_ready` currently polls source tiles across the layer list;
restoration checks complete changed roots, and source upload can decode on a
cache miss. A lazy package cannot inherit those whole-document readiness rules.
Install validated sparse indexes separately from resident samples; prepare only
demanded tiles and operation inputs on workers. Mark unresolved, loaded, decoded
and failed backing distinctly. Source emptiness must never be inferred from
nonresidency. Browser decode/preparation must use workers or preprepared bounded
results, not add synchronous decompression or I/O to input callbacks.

Validate manifest/index structure before adoption; verify each block before use
or copying and validate decoded samples on use. Preserve a detected failure on
all owners, fail dependent operations and do not save substituted empty pixels.
A save copies and checks every retained required payload, including hidden work;
lazy opening does not imply a save can ignore unread source bytes. Archive handles
must survive Save As, atomic replacement, closed tabs, snapshots and undo. On
hosts that cannot retain an immutable old file, spool to private owned storage;
never leave lazy ranges pointing at a mutable destination being overwritten.

### Host transport and admission

[`Project::read`](../../crates/layer-core/src/project.rs) currently takes `Read`;
the ZIP directory needs random access, and lazy resources must retain access
after that call returns. Introduce one shared random-access package/byte-source
boundary and keep a stream-to-private-storage adapter for pipes/providers.
[`read_import`](../../crates/layer-ui/src/import_policy.rs) identifies the master
format before decoding; update that sniffing path too. Unknown native content
needs a typed preserved/preview document result: today's `Result<Project, String>`
with a mandatory editable `Document` cannot represent it faithfully.

GTK, Android, Apple and Windows file workers call the shared reader/writer through
[`files.rs`](../../apps/layer-linux/src/files.rs),
[`documents.rs`](../../apps/layer-android/native/src/documents.rs),
[`project.rs`](../../apps/layer-apple/native/src/project.rs) and
[`documents.rs`](../../apps/layer-windows/native/src/documents.rs). Keep snapshots,
source capture, cancellation, recovery and durable-save acknowledgement in shared
policy. Host publication guarantees still differ. ZIP STORED local headers require
known sizes/CRCs; newly produced blocks/previews need bounded spooling before
non-seekable output. ZIP64 and integer-safe offsets must cross every bridge.

The web path in
[`raster_project.rs`](../../apps/layer-web/src/raster_project.rs) detaches a
`Document`, transfers compressed blocks between independent Wasm heaps, rebuilds
a `Project` on the worker and writes a complete `Vec<u8>`. Its yields bound chunks
of copying, not total retained file bytes. Transferable JS buffers do not remove
copies into/out of Wasm. Replace this duplicate field inventory with the same
shared snapshot/resource visitor and opaque resource handles/range requests.
Account and bound unavoidable bridge copies; do not send unchanged large payloads
back through the input owner for every save. A worker-owned package/Blob or private
store is a possible backing implementation, not a requirement to add shared memory.

Keep structural validity separate from current device admission. Resource bytes,
unique source backing, retained authored metadata, instance expansion, decoded
working sets and GPU contexts need distinct accounting. Existing per-layer tile
counts cannot become an accidental format-wide limit, nor can shared resources
be charged zero for additional evaluated contexts. Hidden/unplaced authored work
counts toward storage; only demanded work acquires pixel caches. Unsupported or
over-budget artwork remains preservable without creating a renderer or compiling
all its shaders.

## Extension cases that constrain the baseline

| Future case | Representation and evidence required before enabling it |
| --- | --- |
| One painted matte drives two effects while its direct row is hidden | Both inputs reference the coverage source endpoint. Demand ignores contribution visibility; changing the source invalidates both consumers. No duplicate paint source or implicit luma conversion. |
| One logo is placed twice with different masks and transforms | Two occurrences reference one content source; edits to the source affect both, occurrence edits affect one. Raw samples may be reused; placed results use distinct contexts. Ordinary Duplicate remains independent. |
| A reusable effect group exposes two controls and two outputs | A definition owns stable interface keys and a subgraph; instances bind those keys. Renaming controls does not retarget bindings. Group encapsulation preserves connections, unlike creating an isolated compositing group. |
| A vector/text source feeds raster effects | Retain editable geometry/text/font resources upstream. The new port type and rasterization operation define sampling. No mandatory early conversion of all content to RGBA and no container change. |
| Two outputs use different crops, scales or working contexts | Outputs reference composition results and their contexts. Different working domains use separate composition contexts or explicit conversions; an output crop does not redefine source color. Share only equivalent evaluations and schedule requested regions under common admission. |
| Held cels and two time-offset instances | Cels reference sources; exposures/time maps are new typed records. Properties are addressed by object/parameter IDs and instance paths. Existing paint sources and stacks retain their meaning. |
| A procedural source or delayed simulation is added | Declare typed inputs, bounds, time/state and resource dependencies. Instantaneous cycles stay invalid. A future solver/delayed-state type owns its contract; do not weaken baseline DAG validation. |

Shared mattes, parameterized groups and two contexts are prototype stress tests,
not a requirement to expose those features in the first product. A prototype
that only serializes ordinary layers cannot establish that the identity and
context boundaries work.

## Milestones and acceptance

Each milestone lands as a complete, checked change; prototypes may be temporary.
The old pre-release codec need not be retained as a compatibility reader. The
long-term compatibility commitment starts with the qualified new baseline.

1. **Specify and test the semantic boundary.** Freeze role ownership, port types,
   stack order/scope, source/occurrence duplication, mask/placement behavior,
   evaluation defaults and version rules in fixtures. Produce the extension
   examples above. Test remapping, retained disconnected content, cycles and
   unknown ports/types. Reuse existing model and GPU oracles; do not create a
   second reference renderer that shares the implementation's mistakes.
2. **Prototype and choose the runtime view.** Compare direct typed traversal and
   a retained indexed plan on equivalent stacks, plus shared matte, contextual
   instances and multiple outputs. Count topology construction, handle lookups,
   metadata allocations, backing copies, decodes, GPU passes and retained bytes.
   Select the simpler passing structure; keep the file independent of that choice.
3. **Cut over the baseline authored model.** Change core edits/history, source
   targets, frame contracts and every consumer together in a buildable milestone.
   Adapt shared stack evaluation and reuse current kernels/fast paths. Remove the
   superseded layer authority. Qualify painting, correction, undo, bakes, queries,
   color changes, recreation and worker failures before freezing persistence.
4. **Implement the package and host boundaries.** Serialize the typed model through
   thin wire adapters, retain compressed resources and implement the preserved
   preview result. Replace detach/rebuild worker transports. Complete source-only
   save, exact-snapshot preview, recovery, non-seekable I/O and ZIP64 tests. Lazy
   residency may be a separate milestone with the same schema; use an explicitly
   bounded eager mode until demand/readiness/lifetime gates pass, rather than
   advertise lazy behavior supplied only by archive offsets.
5. **Qualify and freeze.** Maintain baseline fixtures and lossless readers for
   supported type versions thereafter. Remove temporary adapters and update the
   current architecture, document and project-format guides. Add general graph
   editing/execution only through later feature gates; existing stack files stay
   natively editable.

### Required correctness evidence

Run [the checks for the implementation](testing.md). Existing oracles include
[`merge_tests.rs`](../../crates/layer-core/src/merge_tests.rs),
[`history_budget/tests.rs`](../../crates/layer-core/src/history_budget/tests.rs),
[`raster/restore_tests.rs`](../../crates/layer-render-wgpu/src/raster/restore_tests.rs),
[`scale/tests.rs`](../../crates/layer-render-wgpu/src/scene/scale/tests.rs),
[`scale/effect_tests.rs`](../../crates/layer-render-wgpu/src/scene/scale/effect_tests.rs)
and [`scale/transform_tests.rs`](../../crates/layer-render-wgpu/src/scene/scale/transform_tests.rs).
They cover behavior to preserve; their existence does not qualify the new model.

Add regression cases for both the saved semantic data and rendered pixels:
unchanged exact raster bytes; original/override/material ownership; linked and
independent masks; pass-through and clipping; out-of-canvas transforms; metadata
reorder/rename; source sharing versus independent duplication; undo and late
correction during saves; old sources retained by bakes; cold/lazy reads and late
corruption; stale async completions; cache keys releasing old roots; and save/reopen
of all current effects including LUT resources and HDR. Test unknown non-ancillary
content even when disconnected, and preview failure after successful source
capture. Neither unknown content nor absent preview may silently destroy source.

Walk save/open/save-as, recovery, export, continued painting during save,
preview-only opening, tab parking and renderer recreation on GTK, Web, Android,
Apple and Windows; UI journeys need both themes. Include actual pipe/provider
and browser large-resource paths, not only seekable desktop files. A missing host
implementation remains an explicit incomplete milestone.

### Performance gates

Apply [performance targets](../PERFORMANCE_TARGETS.md),
[measurement rules](../performance/measuring.md) and
[responsiveness](../performance/responsiveness.md). Reserve reference hardware and
compare matched release builds/workloads; desktop model timings and GPU tests
cannot qualify tablet tiers. A pre-existing miss remains a miss.

Compare current and candidate ordinary stacks at 2, 8, 16 and 32 visible layers,
with top/middle/bottom painting; clipping, pass-through and masks; pointwise and
spatial effects; Fit/100%/magnified views; transforms, pan, parameter scrubs, pen-up,
undo and resumed input during refinement. Add a metadata-heavy admitted document,
shared versus duplicated sources, simultaneous output/preview/export, and memory
pressure. Measure cold edits/open separately from warmed motion.

Use these implementation rejection gates in addition to the tier targets:

- No new full-document reconstruction, source readback/recompression, topology
  rebuild or shader compilation during ordinary warmed painting. No pixel work
  on unrelated branches. No new whole-authored-document copy during ordinary
  motion; measure any retained compact render-snapshot copies explicitly.
- Equivalent stacks retain native bytes and established pixel tolerances, fusion,
  branch reuse, bounded windows and direct presentation paths. Extra source,
  transform or mask records do not imply extra GPU passes.
- Reject repeatable throughput loss or CPU/GPU p95 growth above 5%, or p99 response
  latency growth above 1 ms, on equivalent workloads. Noise that cannot resolve
  the bound is inconclusive. Existing passing tier rows must remain passing.
- Retain existing component ceilings. Initially bound additional peak accounted
  renderer memory to `max(16 MiB, 5% of baseline)` for equivalent documents; also
  account CPU metadata, decoded/staged samples, save/bridge buffers and retained
  package handles separately. No monotonic growth through edit/undo/output cycles.
- Compare file size, save/open latency, peak RAM, unchanged-save compressed-byte
  reuse, random access and metadata snapshot latency. Verify that lazy open does
  not scan/decode every payload and that save does not build another complete
  archive in each host heap. Measure first cold paint after opening as well.

Record measured results in the tier tables with their artifacts as required by
the repository. This plan changes no runtime path; no build, host journey or new
performance measurement establishes the proposed design yet.
