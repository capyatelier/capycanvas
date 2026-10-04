# File format and authored graph implementation plan

[Developer guide](README.md)

Adopt the [file format foundation](../history/capy-format-foundation.md) with
ordinary layer compositions as the first supported subset of one authored graph.
Use the same semantic objects in the file and shared editor: compositions,
structured stacks, occurrences, editable sources and effect applications. Resolve
portable IDs to compact runtime handles at the boundary; derive execution plans
without constructing a second editable document.

**Status: the application uses the authored model and new package format.
M3 qualification remains pending.** The initial code assessment used
`0f6b5b708`; sequencing and replacement boundaries included the source-analysis
paths at `11954d8a1`. The milestone requirements and qualification gates below
remain the acceptance criteria, not claims of measured performance. The
[node research](../history/authored-graph-research.md) owns the broader artist
workflows. The foundation owns ZIP, checksums, compatibility and preview rules;
this plan resolves their relationship to the editor and renderer.

M1's concrete ownership and wire contracts are in [authored model](../reference/authored-model.md)
and [package grammar](../reference/capy-package.md). Their boundary fixtures live in
`crates/layer-core/tests/fixtures/capy/`. The integrated application uses typed
`Artwork`/`WorkingState`, source targets and the shared package codec. This does
not complete M3 qualification or start M4 or automatic recovery work.

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
Make one application format switch at M3, after preparing the final codec in M2.
Do not ship an intermediate format or add a compatibility reader for the
pre-release format. Refactor working paths in place and delete each superseded
path with its replacement; deprecation wrappers are not the intended end state.

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

**Source and occurrence ownership is split at the shared boundary.** This replaces
the foundation's earlier proposal to defer the split until the first sharing
feature. [`Document`](../../crates/layer-core/src/lib.rs) owns typed authored
stores and separate working state. Paint and coverage sources own pixels;
occurrences own placement and contribution properties. Render targets use
`SourceTarget` with occurrence context supplied separately. Production consumers
read typed scene views directly; reconstructing combined layer records remains
outside the accepted boundary.

The initial editable subset has at most one occurrence per editable source.
Independent Duplicate creates new authored source and occurrence IDs but shares immutable
backing until edited. Future linked uses share source identity intentionally.
An immutable resource ID, `Arc` identity, checksum or equal pixels never implies
that two sources receive the same edits. Unsupported shared uses remain preserved
and read-only until the editor implements their editing contract. The baseline
reference semantics already permit multiple uses; distinguish a valid graph
outside the editor's supported subset from malformed artwork. Support is derived
from the actual relationships, without a second required-capabilities inventory.

The first editable document has one composition, one root stack and one canvas
output. Its stacks form a non-instanced forest: each nested stack has at most one
group owner, each occurrence at most one containing stack, each effect application
at most one occurrence, and each editable source at most one use. Apply these
support checks to retained unplaced content too. Immutable definitions and binary
resources may be shared. Two group occurrences referencing the same stack are
unsupported even if every paint source has only one direct occurrence reference.
Cycles and dangling references remain invalid; valid shapes beyond this subset
use preservation mode. These are editor support limits, not restrictions on the
extensible reference grammar.

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

Built-in effects persist a stable ID, a parameter-data version and every keyed
value, including defaults. The current catalog supplies their implementation and
controls. Shader fixes do not introduce generations or retain old shaders.
Parameter meaning changes use a small explicit converter for the affected ID and
version; unknown semantics enter preservation mode. Custom effects retain their
literal labels, code and schema, and execute separately from other effects.

Parameter keys and Choice values are durable; GPU offsets and UI order are not.
Load keyed values once into the current definition's compact slot layout. Reuse
that layout during painting without making it a file contract. Accepted numeric
bounds remain separate from slider bounds. Dimensions describe units and the
reference space. See the current [package grammar](../reference/capy-package.md)
and [runtime filter contract](../reference/runtime-filters.md) for these rules.
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
  Ordinary groups pass their existing translation context to their children;
  distinguishing membership from general transform parenting does not remove
  today's inherited group offsets. No separate transform-parent network is needed.
- The initial composition may require editable paint to match its working format,
  as today. That restriction belongs to its type/validator, not the package or
  resource table. Keep imported originals independent. Future mixed working
  contexts require explicit conversion boundaries, not reinterpretation of bytes.
- Paper is an ordinary Solid Color effect occurrence, not viewer chrome. Empty
  stacks remain valid. Mask inspection and selection overlays never enter saved
  output previews. Session selection/active target remain separate from artwork.

## Runtime structure and performance boundaries

### Engine alternatives

All three structures below can consume the same authored types and ports. The
file must not encode the execution representation that selects among them.

| Structure | Fit and decision |
| --- | --- |
| Typed stores with direct stack traversal | Integrated baseline. Compact handles and a borrowed scene view use the common stack visitor directly. Reuse the existing exact/display evaluators and measure traversal and allocation cost. This is sufficient if it passes the baseline gates. |
| Typed authored stores plus retained indexed execution plan | Add when measurements require it or later graph workloads justify it. Lower stacks into compact operation/operand arrays and reverse dependencies; update source/parameter records separately. Replace superseded expression construction and cache traversal in the affected evaluator. No file change or second authored model. |
| Typed authored stores plus task/region execution graph | Viable later when fan-out, several outputs and expensive spatial nodes justify finer scheduling. Tasks are keyed by operation, context, region and quality, with bounded queues and lifetimes. More complex; do not build a general task framework now. The same authored file remains sufficient. |

Keeping a parallel editable layer vector and a separately mutable node model
is rejected: it duplicates edits, undo and serialization rules. Interpreting
JSON/string references on every frame is also rejected. A universal field or
simulation runtime would enlarge scope without solving today's persistence work.

### Compact handles and derived plans

Use opaque 128-bit portable IDs in the file; decode them to fixed-size IDs, and
resolve links once into typed compact handles in shared typed stores. Handles
must remain stable while referenced, or use generations to reject stale reuse.
Keep portable identity available for saving and duplication. Dense positions,
runtime allocators and generations are not file identities.

The stores hold typed payloads and share immutable large values: raster roots,
source images, effect definitions, selections and meshes. Build parent, sibling,
clipping, drawing-target and reverse-dependency indexes when their relationships
change. Do not repeat today's linear `layer(id)`/parent scans through an added
UUID map at each dab, tile or graph edge. Do not introduce a new collection or
persistent-data-structure dependency without demonstrating that existing Rust
containers and shared handles are insufficient.

The integrated renderer accepts a borrowed typed scene view with compact
source/occurrence handles. The common stack visitor supplies exact and display
ordering/scope semantics. Prototype conversion adapters are not a production
boundary; per-frame reconstruction of old layer records fails the M3 removal
and performance gates.

Current [`Graph::prepare`](../../crates/layer-render-wgpu/src/scene/scale/graph.rs)
reconstructs `Arc<Expression>` trees, hashes recursive structures, discovers cache
candidates and computes damage/cost. `scratch_images` also derives expressions.
This is existing derived work, not proof that new authored nodes would be free.
M3 may retain this algorithm if it directly consumes the new scene view and passes
the common gates; do not add another expression-building or layer-conversion pass.
A retained plan belongs to M4 unless needed to pass M3. When introducing it,
replace the superseded construction/hash traversal and update revisions and
regions without rebuilding that plan on ordinary paint frames.

Keep different change classes explicit:

| Change | Required work |
| --- | --- |
| Dabs, correction or new raster root | Update one source generation and changed regions; invalidate dependent contexts. No authored topology change, shader compile or new lowering pass. A retained execution plan, if adopted, also survives these updates. |
| Opacity, color or ordinary effect value | Update the addressed parameter/uniform and relevant dependents. Retain compatible programs and source caches. |
| Sampling radius, extent, interpolation, placement or blend contract | Recompute affected support, bounds, context or plan specialization; invalidate old and new footprints. This is more than a uniform change even without new edges. |
| Reorder, connect, group or change program | Rebuild affected topology/scopes and prepare any changed pipelines off the input path. Publish only a current, ready candidate. |
| Name, node position or active target | Update editor/view state without pixel invalidation. An artwork rename still participates in file modification/history policy. |

Per-frame scheduling may change requested tiles, quality and cache allocation.
Keeping authored topology stable does not prohibit that bounded demand work.
Preserve balanced source-over branches, pointwise fusion, direct transform
presentation, scalar mask paths and native precision. A logical transform or
blend does not require an intermediate texture. Use lifetime-aware reuse, bounded scratch and
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

[`FramePacket`](../../crates/layer-render/src/lib.rs) borrows a typed `SceneView`,
explicit source targets, batches and dabs. The GTK
[`Frame`](../../apps/layer-linux/src/render_thread.rs) retains an
`Arc<SceneSnapshot>`, shared working visibility overrides and bounded transient
records in its mailbox. Queries retain immutable scene snapshots and explicit
scopes. Authored stores and large payloads share unchanged owners; no host rebuilds
a combined layer document for a frame or a file worker.

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

Save/export captures share authored roots once. The writer visits them through
[`ResourceInventory`](../../crates/layer-core/src/package/resources.rs) and the
record adapters, independently of ZIP assembly and without detaching resources
from a cloned editable document. Preserve that boundary when a file worker holds
a snapshot. Measure metadata snapshot cost before choosing more complex
persistent containers.

### Source identity and evaluated identity

Replace overloaded layer/mask target IDs with explicit source targets and
occurrence context. A brush resolves a drawing target once through shared tool
rules; it writes a source in its local coordinates. Captures, restoration and
history follow that source revision. Queries may request the source, a placed
occurrence, a stack prefix or an output; these are different results.

[`Sources::prepare`](../../crates/layer-render-wgpu/src/scene/scale/sources.rs)
uses `SourceTarget` and retains blend/material interpretation with its levels.
Future sharing must preserve these separate identities:

- Shared immutable decoded samples keyed by loaded owner/block identity or a
  generated tile's descriptor and compressed fingerprint, plus interpretation.
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

Keep working selection and active-target state in shared core ownership even
though the portable writer omits them. Preserve selection undo, target repair
after deletion and the distinction between session changes and modified artwork.
One shared edit transaction must still change authored and working state
atomically where necessary. Do not introduce host-specific selection histories
or serialize the entire editor just to preserve the current undo API. Durable
session/history restoration belongs to the separate
[automatic recovery plan](autorecovery.md).

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
  [`RegionSource`](../../crates/layer-render/src/lib.rs) uses explicit source and
  coverage targets or an immutable scene snapshot with `SceneScope`. Preserve raw
  versus adjusted versus displayed color, stack-prefix scope and mask coverage
  semantics.
- **Source-aware adjustment analysis:**
  [`ArtworkQuery` and `EffectInputKey`](../../crates/layer-core/src/artwork_query.rs),
  shared [analysis policy](../../crates/layer-ui/src/effect_analysis.rs) and
  [GPU preparation](../../crates/layer-render-wgpu/src/effect_analysis.rs) must use
  the same typed input scope and captured effect phases. Keep candidate acceptance,
  cancellation, leases and stale-result rejection. Replace layer-index matching
  and copied-layer query carriers; derived illumination buffers are not authored
  resources. Preserve Shadows/Highlights and Clarity through bakes and export.
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

The package reader validates bounded resources and decodes admitted tile samples
on workers through `TileBlob::from_package`. Loaded tiles use immutable owner/block
identity for cache reuse; generated tiles may use a descriptor and compressed
fingerprint computed on the compression worker. Both include color interpretation.
Decoded content digests remain explicit, lazy integrity/deduplication work, never
required for installation or cache identity on opening or input. The writer retains
original encoded resources. M4 demand loading must preserve these contracts.

Use retained resource/block identity plus interpretation for loaded-byte reuse.
Scope identity to an immutable backing owner/generation; two independently opened
files containing the same declared IDs are not proof of identical bytes.
Newly encoded identical blocks may be deduplicated with a strong hash or byte
comparison; CRC-32 is only stored-byte integrity. Payload equality never establishes
editable source or occurrence identity. Preserve the same compressed LZ4 bytes
through save, including their shuffle/layout contract. Avoid a file-specific tile
object that must be copied into an otherwise identical renderer tile.

Extend the existing backing abstraction in
[`raster_storage.rs`](../../crates/layer-core/src/raster_storage.rs) to reference
immutable package ranges with a retained owner and bounded asynchronous reads.
Its current external read copies a slice into a new `Arc`, and its native disk
read seeks synchronously; neither is a promise of zero-copy access. Prefer an
owned shared byte range when possible, without requiring memory mapping on every
host. Never decompress the ZIP pack around independent LZ4 tiles.

`raster_restore_ready` polls retained typed source revisions and demanded targets;
restoration checks complete changed roots, and source upload can decode on a
cache miss. A lazy package cannot inherit those whole-document readiness rules.
For M4 lazy loading, install validated sparse indexes separately from resident
samples and prepare only demanded tiles and operation inputs on workers. M3 may
use bounded eager preparation through the same backing interface. Mark unresolved,
loaded, decoded and failed backing distinctly. Source emptiness must never be
inferred from nonresidency. Browser decode/preparation must use workers or
preprepared bounded results, not add synchronous decompression or I/O to input
callbacks.

Validate manifest/index structure before adoption; verify each block before use
or copying and validate decoded samples on use. Preserve a detected failure on
all owners, fail dependent operations and do not save substituted empty pixels.
A save copies and checks every retained required payload, including hidden work;
lazy opening does not imply a save can ignore unread source bytes. Archive handles
must survive Save As, atomic replacement, closed tabs, snapshots and undo. On
hosts that cannot retain an immutable old file, spool to private owned storage;
never leave lazy ranges pointing at a mutable destination being overwritten.

### Host transport and admission

[`ImmutableBacking` and `ByteSource`](../../crates/layer-core/src/package/backing.rs)
provide shared random access after opening. [`BackingReader` and bounded
spooling](../../crates/layer-core/src/package/transport.rs) adapt ready ranges and
non-seekable providers. [`read_import`](../../crates/layer-ui/src/import_policy.rs)
identifies package/photo input and retains native backing through integrity,
color and device admission. Its `ImportOutcome` selects an editable candidate or
a preserved/recovered/failure package outcome; `PackageView` never adopts preview
pixels as an editable document. Cancellation and stale preparation do not publish
a package view or replace the incumbent editor.

GTK, Android, Apple and Windows file workers call the shared reader/writer through
[`files.rs`](../../apps/layer-linux/src/files.rs),
[`documents.rs`](../../apps/layer-android/native/src/documents.rs),
[`project.rs`](../../apps/layer-apple/native/src/project.rs) and
[`documents.rs`](../../apps/layer-windows/native/src/documents.rs). Keep snapshots,
source capture, cancellation, recovery and durable-save acknowledgement in shared
policy. Host publication guarantees still differ. ZIP local headers contain known
sizes/CRCs; newly produced blocks/previews and compressed manifest metadata use
bounded preparation before non-seekable output. Resource packs remain STORED;
only bounded manifest JSON may use DEFLATE. ZIP64 and integer-safe offsets cross
every bridge.

The web path in
[`artwork_transfer.rs`](../../apps/layer-web/src/artwork_transfer.rs) uses shared
[`PreparedTransfer`](../../crates/layer-core/src/package/transfer.rs) metadata and
bounded transferable payloads between independent Wasm heaps. Worker package
writes stream into private browser storage before picker/download publication.
Unsupported opening returns shared presentation facts and an optional verified
PNG; the browser retains its original `Blob` as copy authority. No full archive
or second editable artwork schema is transferred back through the input owner.
Account and bound unavoidable bridge copies without adding shared memory.

Keep structural validity separate from current device admission. Resource bytes,
unique source backing, retained authored metadata, instance expansion, decoded
working sets and GPU contexts need distinct accounting. Existing per-layer tile
counts cannot become an accidental format-wide limit, nor can shared resources
be charged zero for additional evaluated contexts. Hidden/unplaced authored work
counts toward storage; only demanded work acquires pixel caches. Unsupported or
over-budget artwork remains preservable without creating a renderer or compiling
all its shaders.

### Recovery extension boundary

The [automatic recovery revamp](autorecovery.md) follows qualified M3. Preserve
current recovery checkpoints during the cutover and make these boundaries reusable
without implementing restart restoration, persisted undo or an incremental store
as part of the format work:

| Boundary | Contract established by the format work |
| --- | --- |
| Coherent capture | [`Editor::capture`](../../crates/layer-core/src/lib.rs) and [`CanvasEngine::capture_artwork`](../../crates/layer-engine/src/canvas.rs) produce [`ArtworkCapture` and `CaptureCheckpoint`](../../crates/layer-core/src/authored/artwork.rs) from the ordered owner and actual evaluation context. Keep working-state ownership and generation accessible so later recovery can capture selection, targets and bounded history at that same boundary. Manual saves need not clone undo history, and an artwork checkpoint must not be the sole version of session state. |
| Resource reuse | [`ResourceInventory` and `PreparedResources`](../../crates/layer-core/src/package/resources.rs), [`artwork_records`](../../crates/layer-core/src/package/artwork_records.rs) and [`PreparedTransfer`](../../crates/layer-core/src/package/transfer.rs) enumerate metadata and immutable resources independently of ZIP assembly. Unchanged resources retain identity across captures within a drawing, independent of file offsets or paths. A later private store can reuse their encoding and bytes without writing, reopening or unpacking a complete archive for each checkpoint. This does not promise incremental ZIP saves. |
| Retained roots and budgets | Reuse [`RootInventory`](../../crates/layer-core/src/lib.rs), [`Editor::retained_tiles` and `RetainedTiles`](../../crates/layer-core/src/raster_storage.rs), and [`history_budget`](../../crates/layer-core/src/history_budget.rs) for document, undo/redo, parked-tab, snapshot and accepted-job roots. A later history codec must be able to enumerate its referenced resources through that boundary. The portable writer selects authored-document roots; it never silently starts exporting history or session state. |
| Publication and lifetime | [`document_files`](../../crates/layer-ui/src/document_files.rs) binds capture/write tokens to the document/session generation and captured checkpoint; [`RecoveryState`](../../crates/layer-ui/src/recovery.rs) owns recovery publication policy. Manual-save acknowledgement remains distinct from recovery publication. Failed or stale writes cannot acknowledge newer work or release another owner's backing. Resource owners survive accepted jobs and release when their last owner goes away; future cleanup can use these lifetimes without depending on render caches. |
| Private session records | [`WorkingState`](../../crates/layer-core/src/authored/artwork.rs) and [`DocumentSessions`](../../crates/layer-ui/src/document_sessions.rs) retain editing state and tab owners. Portable object/source identity remains available for later selection, target and history references; runtime handles are resolved on restoration. Reuse shared owners for selection, targets, camera and tabs, and workspace/preference persistence for the state it already owns. New private session/history codecs extend shared capture, not the portable manifest or a duplicate artwork model. Do not persist Rust inverse-edit enums or GPU state by default. |

M2 tests capture/resource reuse and cancellation with bounded backing. M3 must
verify current recovery, Save/Undo/Redo and continued editing during publication through
the new interfaces, including stale completion and failure. Actual session/history
serialization, resource-first checkpoint publication, cross-process claims,
incremental-store selection and lifecycle flushing remain in the recovery plan.
That work rechecks the landed APIs and measures their costs before choosing a
store; the format cutover must not hardwire recovery to a whole-archive `Vec<u8>`.

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

Proceed M1 -> M2 -> M3, then independently qualified M4 work where useful. Each
milestone lands as a complete, checked change. Keep incomplete integration in the
implementation worktree; never land an app that cannot save its live model.
The integrated model and shared package codec have replaced the pre-cutover v15
native codec and separate Web field inventory. The requirements below retain the
removal and qualification gates. The complete application cutover lands after
critical correctness checks; remaining performance and host qualification must
finish before its baseline can be declared qualified.

### M1: Resolve contracts and test the risky boundaries

Produce concrete wire fixtures and the smallest renderer/transport prototypes
needed to choose a passing implementation. Close these decisions before M2:

| Contract | Required result |
| --- | --- |
| Field ownership | Account for every pre-cutover field of `Document`, `Layer`, `LayerProperties`, `LayerMask` and effect definitions to an authored owner, derived data or shared working state. Include paper, reference designations, rulers, selections, image/profile/photo metadata, proof/SDR intent, placements and material state. Record defaults, units, lifetime and a round-trip or intentional-omission assertion. No field silently disappears. |
| Graph semantics | Specify the editable-subset validator, typed ports, scoped backdrop/clipping, group coordinates, mask slots, duplication/deletion and unplaced retention. Include indirect sharing through a reused group as an unsupported-content fixture. |
| Wire grammar | Fix record/version names, reference and ID spelling, integer representation, frozen defaults, resource descriptors, pack index, checksum coverage, ZIP64/header rules and the bounded preview convention. Include malformed, unsupported and ancillary examples, not only valid JSON. |
| Core and renderer API | Choose typed occurrence/source handles, lookup indexes, undo ownership, handle lifetime through deletion/redo, scene/query views and publication generations. Use the existing stack evaluator first; investigate retained plans only where counters show a need. |
| File/host API | Define editable/preserved/recovered-view outcomes and their permitted operations; immutable byte-range ownership; read readiness, cancellation and failure; snapshot capture and save acknowledgement. A preserved package view cannot become an editable `Document`. |
| Output capture | Define opening phase, phase after rate changes and the saved output context for current time-dependent effects. Pair source roots, effect phases and analysis inputs in one capture; do not infer integrated phase from elapsed time alone. |
| Recovery extension | Define the shared capture, resource traversal and checkpoint boundaries in [recovery extension](#recovery-extension-boundary). Retain identity and ownership needed by later private session/history codecs without adding them to the portable manifest. |

Use existing test oracles for exact/display pixels, pending publications and undo.
Exercise shared matte, reusable-group interfaces and two evaluation contexts in
focused prototypes, without adding their editing UI or a second full renderer.
Measure handle resolution, traversal, metadata allocation, backing copies,
decoding, GPU passes and retained bytes. Exercise owned ranges and bounded
transport on native and Web before selecting a cross-host API. Remove rejected
prototypes; retain useful assertions in the established test harnesses.

### M2: Build the final codec and backing infrastructure

Implement the chosen typed records, thin wire adapters, shared snapshot/resource
visitor, package reader/writer and immutable backing interface. Test round trips,
compressed-byte reuse, remapping, unsupported-content preservation, corruption,
non-seekable streams and ZIP64 independently of changing the live editor. Use the
same records that M3 will adopt; do not create a throwaway runtime model.

During M2 the application retained the pre-cutover model/codec. M3 now integrates
the replacement; qualification remains pending. There is no intermediate default
format, dual writing or migration chain. A
separately landable codec is a complete tested library boundary. If preparation
cannot stand independently, keep it with M3 rather than land broken integration.
Temporary test conversion from the old model is allowed; it is removed at M3.
Reuse existing encoding/validation helpers and refactor shared primitives in
place. Delete superseded helpers as their last caller moves. Any live-path
refactor in M2 must preserve current save/recovery and pass its own affected checks.

### M3: Switch the application once and remove superseded paths

Integrate the authored model, shared edits/history and working state, frame/query
contracts, source targets, all renderer consumers, new codec and host transports
as one coherent cutover. Wire import sniffing, editable/preserved opening,
source-only save, optional exact-snapshot preview, Save As, recovery checkpoints,
tab parking and export on GTK, Web, Android, Apple and Windows. Reuse shared
publication policy and native services. The replacement requirements below are
part of completion, not a later cleanup task.

Bounded eager opening is acceptable through the final resource interface; all
I/O, sample decoding and pixel copying still stay off input/UI threads. The
baseline can retain a compact owned render snapshot and the existing derived
composition algorithm if they pass the common gates. No old-layer reconstruction
or graph synchronization bridge may remain in production.

Before landing, run the correctness and host journeys below and measure affected
frame paths against the common performance gates. Keep existing failures explicit.
The integrated replacement removes superseded readers, writers and editable
model paths in the same change. Keep current architecture, document, format and
host guides accurate while qualification is pending. Freeze the baseline and
record M3 completion only after those gates pass; maintain lossless support for
that qualified baseline thereafter.
The [automatic recovery plan](autorecovery.md) can resume after this milestone;
its session-restoration features are not part of this cutover.

### M4: Qualify optional runtime improvements without another format change

Implement lazy residency and retained execution plans as separate measured changes
when justified. Lazy loading replaces mandatory whole-document readiness with
per-input demand, worker preparation and bounded caches using the same codec and
resource handles. A retained plan replaces repeated expression construction and
recursive cache traversal in the affected evaluator. Remove replaced queues,
traversals and eager-only assumptions in the same change; do not keep a second
reader or renderer as a permanent fallback. Preserve existing exact/display
roles and their shared semantics.

Apply the additional M4 gates only to the feature being enabled, together with
all common gates for affected paths. M4 is not a prerequisite for freezing the
baseline or resuming automatic recovery. General graph editing and a task/region
scheduler remain later feature work. Once this plan's implementation scope is
complete, keep lasting contracts in the current guides and remove the plan;
do not retain it merely to catalogue unimplemented future possibilities.

### Required replacement and removal

| Superseded boundary | Required replacement and deletion |
| --- | --- |
| `Document.layers`, combined `Layer` ownership and overloaded paint/mask `LayerId` targets | At M3, use typed authored sources/occurrences, shared working state and explicit targets. Remove the old editable authority, duplicate parent/order storage, old ID-routing branches and production adapters that rebuild `Layer` records. Adapt callers to the new API rather than preserving a legacy facade. |
| Native `project_storage` codec and direct `Document` serde | At M3, replace v15 read/write, its manifest/index records and detach/rebuild flows with the final package codec and visitor. Move useful profile, selection, metadata, material and validation helpers into their new owners once. Remove obsolete readers, writers and version branches; no old-format migration layer. |
| Web `raster_project` metadata/rasters/blobs/originals inventory | At M3, remove the duplicate artwork schema and pack/unpack reconstruction. Use shared snapshot/resource enumeration and bounded worker transport. Keep only host buffer/storage execution in Web; do not route an unchanged archive through every heap. |
| Layer-only frame, merge, preview and query carriers | At M3, replace copied-layer and boxed-layer carriers with typed scene snapshots, targets and contexts. Refactor `FramePacket`, `RegionSource`, `ArtworkQuery`, `EffectInputKey`, bake retention and analysis requests together. Remove obsolete variants, matching logic and helper traversals after moving all callers. Immutable snapshots remain legitimate owners. |
| Resource detachment and decoded-hash-only cache identity | By M3, enumerate resources without mutating a cloned document and key loaded backing by immutable owner/block identity plus interpretation. Remove mandatory decoded hashing from resource installation. Retain strong hashing where still needed for independent deduplication or integrity uses. |
| Host-specific project reconstruction and policy | At M3, replace reconstruction with the shared open/capture/publication contracts. Delete superseded bridge entry points, messages and duplicated admission/checkpoint logic across all clients. Preserve required picker, permission, lifecycle and durable-storage adapters. |
| Compositor traversal, source access and caches | Adapt the common stack visitor and source access in place at M3. Keep useful GPU kernels, fusion, exact/display paths, damage tracking and capture machinery. If M4 replaces an execution algorithm, delete that algorithm's old construction/cache path instead of layering both. |
| Fixtures, dependencies and temporary integration code | Port semantic assertions to the new model/format, remove obsolete codec-only fixtures and prototype adapters, and remove imports, exports, dependencies and feature flags left unused by the cutover. No production test switches, dual-format switch or deferred cleanup stubs. |

For each milestone, identify replaced symbols and their remaining callers before
editing. At review, search production code, tests, host bridges and build inputs
for those symbols and account for every remaining reference. Reuse helpers and
test harnesses; pure refactors should not add lines. Report added/removed lines
and explain necessary growth from new authored contracts, codec or preservation
behavior separately. An unexplained parallel path or leftover pre-reset
compatibility wrapper fails the milestone even if the new path passes tests.
Remove code because its responsibility moved, not to discard required behavior
or independent regression coverage.

### Required correctness evidence

Run [the checks for the implementation](testing.md). Existing oracles include
[`merge_tests.rs`](../../crates/layer-core/src/merge_tests.rs),
[`history_budget/tests.rs`](../../crates/layer-core/src/history_budget/tests.rs),
[`raster/restore_tests.rs`](../../crates/layer-render-wgpu/src/raster/restore_tests.rs),
[`scale/tests.rs`](../../crates/layer-render-wgpu/src/scene/scale/tests.rs),
[`scale/effect_tests.rs`](../../crates/layer-render-wgpu/src/scene/scale/effect_tests.rs)
and [`scale/transform_tests.rs`](../../crates/layer-render-wgpu/src/scene/scale/transform_tests.rs),
plus [analysis lease tests](../../crates/layer-render-wgpu/src/effect_analysis_lease_tests.rs)
and [local adjustment snapshots](../../crates/layer-render-wgpu/src/snapshot/tests/local_adjustments.rs).
They cover behavior to preserve; their existence does not qualify the new model.

Add regression cases for both the saved semantic data and rendered pixels:
unchanged exact raster bytes; original/override/material ownership; linked and
independent masks; pass-through and clipping; out-of-canvas transforms; metadata
reorder/rename; source sharing versus independent duplication; undo and late
correction during saves; old sources retained by bakes; cold reads and late
corruption; stale async completions; cache keys releasing old roots; and save/reopen
of all current effects including LUT resources and HDR. Test unknown non-ancillary
content even when disconnected, and preview failure after successful source
capture. Neither unknown content nor absent preview may silently destroy source.

M1 prototypes cover future sharing and multiple contexts; M3 tests must classify
those shapes as unsupported when outside its editable subset. M3 also covers
selection undo without portable session fields, source-analysis inputs/phases and
recovery resource lifetime. M4 lazy-loading tests add partial residency, missing
and corrupt blocks discovered after adoption, eviction/reload and demand through
each input. Later optimization is not a reason to postpone baseline correctness.

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
simultaneous canvas/preview/export, current source-aware adjustments, and memory
pressure. Measure cold edits/open separately from warmed motion. Shared versus
duplicated sources and multiple output contexts belong to M1's focused prototypes
and later feature qualification until those shapes are editable in production.

Use these common rejection gates in addition to the tier targets at M3 and for
every M2 or M4 change affecting a live path:

- No new full-document reconstruction, source readback/recompression, authored
  topology rebuild, extra lowering pass or shader compilation during ordinary
  warmed painting. Measure retained baseline derived-plan work. No pixel work
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
  reuse, random access and metadata snapshot latency. Save must not build another
  complete archive in each host heap. Measure first cold paint after opening,
  current recovery writes and save/undo while a captured snapshot remains owned.

M4 adds the gates for its chosen optimization. Lazy opening must avoid scanning
or decoding every payload, read only demanded regions under bounded queues, and
handle missing backing without treating it as empty. Retained execution plans
must avoid construction and recursive topology hashing on ordinary paint frames.
Both must preserve common latency, memory and correctness gates, and include
evidence that their superseded production paths were removed. A qualified bounded
eager M3 implementation need not meet the lazy-opening gate.

Record measured results in the tier tables with their artifacts as required by
the repository. M1 and M2 have landed. M3's authored model and package cutover
is implemented; qualification against the correctness, host and performance
gates above remains pending.
