# Region and scale composition

Native presentation can compose eligible paint stacks at the resolution the view
needs. Authoritative paint remains in exact document tiles. Presentation pixels
never become paint, history, export, sampling, or project backing.

The implementation is in `crates/layer-render-wgpu/src/scene/scale.rs`. The
layer order, isolated and Pass Through groups, clipping bases and adjustment-chain boundaries
are shared with exact composition in `scene/stack.rs`.
[Performance targets](../PERFORMANCE_TARGETS.md) defines the budgets and links
to current device measurements.

## Ownership and execution

The view's largest singular value chooses a power-of-two texel footprint no
larger than a surface pixel, capped at 16 document pixels. At the benchmark's
7.14% zoom this is an 8 × 8 footprint. Admission bounds the reduced layer images
and the expression-evaluation scratch pool plus the adjacent output mip against
the existing display component budget.

The scene owns each layer's reduced local pixels independently of the view.
A source can retain several levels within the display allowance; each records
which local pages are valid. Authoritative paint uses linear premultiplied working
color and masks use scalar coverage. In perceptual documents, identity-source
reduction encodes each input color before averaging; encoding an averaged linear
color would change the composite. Transformed sources retain linear samples for
resampling and encode their result before opacity and blending. Each source level
records its representation; a change invalidates all retained pages. A placement
transaction keeps its hinted layer's linear source and sampling resolution when
the pose passes through identity. Ending the transaction reevaluates the source
representation and composition. Encoded
levels cannot supply linear transform inputs. Source identity, immutable native captures, brush damage
and retired prediction footprints invalidate the affected pages at every level.
Brush invalidation reuses the painting tile plan, preserving untouched pages
inside a contact batch's bounding rectangle.
One current native backing is retained per source, rather than per level.
Each image carries its native extent, resident bounds and texel footprint.
Source windows cover the inverse-mapped output with page-aligned sampling
padding. A resident window grows to include nearby requests while it occupies at
most twice the requested pixel storage; admission charges the retained window.
Required source windows determine admission; optional retained overlap yields
when the combined source and composition allocation would exceed the budget.
For pointwise stacks whose identity-placed source windows exceed that allowance,
the evaluator reduces source pages into reusable working tiles on demand.
Complete source windows remain optional reuse; required coarse overview levels
retain their allocation. Cached and streamed requests share the same paint and
mask reducers. Both derive valid regions from retained finer levels before
requesting native pages, and consuming commands are encoded before a working tile
is reused.
Motion back into a retained window preserves its images. Moving beyond that allowance
replaces the window, copies valid overlap and prepares only missing pages.
Reduction between windows uses their origins and weights partial boundary texels.
Admission charges the resident dimensions, including replacement windows and
presentation levels. A partial source image cannot satisfy a whole-image query.

A changed exact page is averaged directly into its source image. Empty pages
write zero without reading a full page. Decode batches consume their inputs
before their cache slots can be reused. Coarser levels derive from valid finer
regions; gaps in a partial finer image cannot overwrite valid coarse pixels.
Required previews take priority over spare detail when several photos compete
for the admitted memory. Admission reserves composition scratch and command
capacity before retaining optional source levels, including images allocated
later in the frame. Pose changes preserve local pixels while their representation
remains valid.
Cold sources may prepare one adjacent finer level from the remaining
source allowance, deriving the required level without decoding twice. Existing
finer detail is refreshed with source damage and retained across scale boundaries;
required images take priority under pressure. A first stroke prepares only its
required level. A compact prediction cannot update
a source level finer than its own sampling grid.

The shared layer traversal builds an expression tree. Normal premultiplied
source-over runs use associativity, with each layer's opacity applied before
regrouping into balanced branches across the whole run. Transparent paper adds
no operand. Power-of-two grouping boundaries and empty layer positions
preserve lower branches when painting starts; transparent operands require no
image or blend pass. Group opacity, masks, clipping and non-normal blends
remain expression boundaries. Exact and reduced composition share blend formulas
and document-depth flags, including clipped layers and extended float colors.
The document blend space invalidates composed branches and selects the shared
blend formulas. Pointwise effects decode and encode at their fused boundary;
image-boundary effects receive their declared linear or document-blending inputs.
Linear effects encode their final output for a perceptual composite; effects
already operating on encoded values retain that representation.
Paper and mask-inspection colors use the composite representation, while scalar
coverage stays unencoded. Presentation decodes the completed composite before
applying the output color transform.
Pass Through children continue the enclosing composite. Group opacity and masks
interpolate between its retained backdrop and the completed children; clipped
groups remain isolated. The same traversal serves transform previews and exact
queries, including a query that stops inside a Pass Through group.
Reusable branches retain page validity; a source
change invalidates only dependent regions. Branch images share the display
allowance with source levels, after reserving required sources, evaluation
scratch, command storage and presentation mips. Large unchanged branches get
priority. No cached expression owns original image bytes or raster history.

Evaluation visits the child needing more scratch first, reuses completed
branches, and writes changed pages into a stable root image. An edit in a
balanced normal run needs logarithmically many composition operations when
its unchanged branches fit the budget.
Placed sources and masks use the common transform
resampler. Their most magnified axis determines source resolution; an additional
level of detail and up to four samples per axis limit placement-edge error.
Unit-scale aligned inputs use one sample per pixel.
Interior samples use the hardware linear sampler. Boundary samples account
for partially filled source texels and the smaller final output cell. Masks retain their default coverage outside their local image.
A normal placed layer over a constant backdrop keeps its source and transform
until another layer needs its pixels. If it reaches the root unchanged, the
presenter samples it directly into the surface. This avoids an
intermediate canvas image and a second resampling step. Its neighboring source
levels share the scene's validity and memory allowance. Other stacks materialize
their result and derive an adjacent output mip for trilinear presentation.

The expression graph fills a padded, page-aligned viewport window at the selected
resolution. Retained root images have viewport bounds; finite-radius filters
expand their input and branch bounds by the sum of their scaled pass supports.
Source preparation, scratch allocation and admission use those same padded
bounds. Dense filter scratch reserves the expression graph's peak live images,
including held masks and intermediate passes. Pointwise
temporary results reuse 256-texel working tiles at every resolution. Every value and output carries its own grid,
so a tile can compose directly into a viewport image without changing document
coordinates. Aligned native sources borrow paint or decoded tiles directly.
Decoded tiles carry leases until their consuming commands are encoded; eviction
cannot replace a borrowed tile. The renderer owns one decoded-tile cache and
upload allowance shared by display composition, refinement and exact queries.
Replacing a scene preserves this cache; changing document color resets it.
Other native sources use the shared paint and
mask gatherers; minified sources use retained reduced windows or streamed tiles. Composition batches
uniform records and compute dispatches, flushing before source preparation,
transforms, effects or reduction consume or replace their inputs. Admission
reserves the root, its adjacent mip,
bounded working tiles and placement gathers separately. Panning reuses valid
overlap and renders only newly required or changed pages. Shared sparse contact
plans restrict pointwise edits to touched pages, including retired prediction
pages. Spatial and global programs retain their dependency propagation. A full-document
overview serves pixels outside the window. Covered overview
regions derive from completed detail; uncovered regions use reduced composition,
avoiding duplicate exact-source work.
The overview remains coarser than the window, including at reduced zoom.
Document-wide effects retain whole-document dependency coverage at reduced
resolution. Finite-radius filters preserve document-edge sampling while evaluating
the requested output window and its complete input support. Native queries and
idle refinement still use exact document pixels.
Effect render passes cover at most 512 × 512 texels, with the complete input grid
and unchanged sampling coordinates. Shorter passes let native controls share the
GPU during expensive filters; their load/store cost remains part of the preview
budget.
Retained windows are admitted again when their source requirements change.
Presentation regeneration advances display damage independently of the artwork
revision used by document previews; camera motion does not publish an artwork edit.

One display hierarchy owns the visible window, overview and adjacent levels.
Drawing and idle refinement write the same images. The separate full composite, detail atlas and duplicate display-only source cache
have been removed. Native filter evaluation and exact queries reuse the region
executor and its paint, mask, blend, placement and filter kernels. Shader recipes
belong to the device and survive cache or level replacement. Unsupported view
sizes fail admission before document or paint mutations.

Paint-transform transactions retain immutable originals and selected/unselected
input levels. The graph evaluates a transformed source only where output is
requested, reusing static branches and folding a constant backdrop into the
resampling pass. Affine, perspective and mesh previews share that source
contract. Source resolution also bounds the pixel transform’s magnification, using
the projective Jacobian or the mesh’s derivative control hull. Admission reserves
the same input resolution, including linked paint/mask companions. Whole-image
selections reserve one input pyramid; split selections reserve both moved and
kept coverage using the capture's coverage decision. Whole-image
transforms reconstruct prefiltered color from a retained mip pyramid. Each output
footprint selects detail from its local Jacobian; partial edge cells use their
actual centers. Finer transaction inputs persist across scale oscillations.
An affine or perspective transform with one input over a constant backdrop can
reach the presenter directly, including with Navigator visible. The viewport
samples that pyramid with its own rotated footprints, sharing the graph's
resampling functions. Interior pixels need one trilinear sample; pixels crossing
source or layer edges use eight samples per axis.

Navigator uses one renderer-owned retained whole-document image, at most 512
texels on either side, shared by standalone and in-surface presenters. It copies
or reduces a completed whole-document level when available. Direct roots evaluate
through the same graph and resampler at thumbnail resolution, retaining a coarser
prepared source level when the main view already uses one. Their main output
stays virtual. The main canvas's coarse coverage remains independent.
Artwork, transform poses and temporary brush tails invalidate the retained image;
camera geometry does not. Consecutive changes coalesce at 20 Hz using the frame's
native timestamp. Zero-timestamp preparation and untimed flushes preserve the
session clock. Pending pixels request a following frame. When that frame has
no new artwork change, it submits the final coarse refresh immediately, before
idle refinement, including after pen-up, Apply, Cancel and history changes. It
needs no timer polling between deadlines or new input to converge. Discarded GPU
commands invalidate the image and keep a retry pending. Document replacement,
blend-space changes and renderer replacement rebuild it before presentation.
Navigator presentation samples bilinearly, using a bounded 4 × 4 average in
linear light for footprints wider than four preview texels. It applies the
existing color and proof transforms to these coarse pixels; its camera outline,
clip, orientation and input stay current.

Partial selections and copies compose moved and
retained inputs before presentation, preserving their correlated coverage.
Switching to a materialized root invalidates the formerly virtual pixels.
Mesh triangles rasterize color directly into the graph's target. Fragment
derivatives choose the source footprint, and later triangles replace earlier
ones where a mesh folds. Kept coverage and the mesh share one render pass.
Selection contours reuse the mesh's curvature subdivisions with a requested
quarter destination-pixel tolerance and the same 64-step cap per patch axis.
Their edges split at subdivision boundaries, retaining clipped corners without
sampling every source pixel along straight edges.
Split selections use finer
inputs and four samples to limit moved/unmoved coverage error; edge texels
use their actual extent. Exact queries evaluate native source tiles for their
requested dependency window, retiring tiles from the previous window. Display
motion has no full-layer native preview. Idle display refinement evaluates exact
transform regions after the gesture stops. Scalar-mask
transactions currently provide native coverage to the same graph.
Photo placement keeps exact refinement deferred until Apply or Cancel, including
batch imports. Repeated pointer positions during a drag cannot start idle work.

Transform motion invalidates the previous and next destination bounds. The
stationary cut also changes when a transaction starts, its source changes, Leave
Copy toggles, the transform returns to identity, or native preview ownership
changes. Subsequent poses reuse the unchanged cut.

Eligible temporary dry-brush tails use compact prediction pages. They share the
existing dry material evaluator, reading averaged exact destination color and
stroke coverage. The combined layer placement and camera use a half-surface-pixel
contact evaluation density along the most magnified axis, capped at four local
pixels per prediction texel and by the source level its compositor consumes.
At Fit this gives 64 × 64 prediction pages instead of 256 × 256 pages. No private
full-size coverage fork is needed. Reduction weights partial edge texels by their
actual document area. Changing or cancelling a tail invalidates its old footprint;
committing it still executes the authoritative full-resolution brush.

Exact queries replay a compact tail through the existing exact tile executor
using retained preview contacts. Replay preserves the existing full-size preview
semantics, including its view-dependent block evaluation. This happens on demand,
once for that tail, replacing the compact scratch pages. The already-composed display stays valid;
the next live tail returns to compact scratch. Save/undo history contains only
committed native pixels. Idle display refinement composes those authoritative
pixels without becoming a publication gate or adding work to undo history.
Its unfinished GPU batch still precedes newly submitted painting on the queue.

Once the view and artwork stop changing, the renderer refines up to four native pages
per idle submission through the same region executor used by exact queries.
It reduces the exact composite into the retained display window and overview,
then updates the adjacent presentation mip. When a native hierarchy is resident,
composition writes directly into it, batching pages that share a prepared source
window into one command sequence. Only one refinement batch may remain in
flight. Fresh artwork can prepare and submit behind that batch without waiting
for its completion. Required raster work keeps submission backpressure, and
each held batch owns its completion token. Dependent painting still waits for
Healing publication.
Intermediate idle batches do not require a present; completion and new
artwork, navigation or overlays do. A skipped revision forces the next retained
presentation to redraw the complete view. Native identity composition already
contains exact pixels. Moving transform previews defer refinement until release.
Each cache tracks exact validity alongside ordinary display validity; edits
invalidate both through the same dependency regions. The first stroke on an empty
layer changes the expression tree but retains untouched output pages when the
layer metadata is unchanged and sparse damage bounds the edit. Changes to layer
settings still invalidate the complete affected graph. Navigation retains exact
overlap. Directly presented placements switch to a materialized display only
when that output is complete. Refinement advances presentation damage without
changing the artwork revision, and pending work remains visible to the shared
frame scheduler. Unchanged global filter dependencies are reused across pages.
When the device's display allowance covers the working cache and full-document
residency, the same hierarchy retains every level down to native resolution.
Existing full-size level images are reused; bounded windows are copied into
their resident levels and released. The requested region remains independent of
the allocated image bounds, so drawing evaluates only missing or dirty regions.
Refinement writes each exact native page once and repairs its ancestors in place.
There is no separate settled pyramid or handoff after the final page.
Subsequent pan, rotation and zoom select these resident levels. An edit
invalidates affected pages across the hierarchy; finer detail is repaired before
reuse. Devices without that allowance retain bounded visible windows and their
overview. Admission checks current device headroom plus the bytes already owned
by the hierarchy being retained. The first artwork change after idle refinement
rechecks this allowance and releases optional residency when it no longer fits.
Continuous changes reuse that admission until refinement resumes. Snapshot workers do
not allocate optional display levels.
Exact filter image pixels and retained display pixels share one composition
allowance; the filter budget reserves the full bounded display cache before
admitting native dependencies, plus the actual resident hierarchy. Optional residency is released when an effect's
required images fit only without it; unsupported dependencies are rejected before
any artwork changes.

Display outputs and their scene dependencies share submission validity. Discarding
GPU commands invalidates both, so the next frame rebuilds them instead of reusing
pixels that were never written. View and resource admission precede artwork changes.

## Supported contract

The region executor admits native paint layers, isolated groups, clipping
stacks, all blend modes, paper, affine placements and scalar masks. Mask
inspection remains presentation only. Effects explicitly declaring
display-resolution support execute in the reduced graph with document coordinates,
layer masks and clipping. Image passes expand dependency regions and damage by
their sampling footprints; intermediate results populate the halo needed by
later passes. Gaussian Blur prepares scaled kernels for the evaluation grid.
Native-resolution effects and native views of image-boundary effects evaluate
native regions into the same display cache. Native evaluation also handles
source requests that exceed the reduced graph's admission limits. Region windows
include the complete filter halo; document-wide dependencies retain their whole
input. Watercolor resolves its native pigment state before source reduction.
Both evaluation modes share presentation geometry, exact validity and mip updates. Advanced brushes keep their exact temporary evaluator; simple
analytic dry contacts without grain, selection, alpha lock or edge effects can
use compact tails.

An unchanged view may retain one neighboring composed output within the same
component budget. Returning to it reuses its pixels; artwork changes retire it.
Transform input allocations are reserved in admission. Source levels have independent ownership and validity, so returning through a
native view need not reread a previously reduced photo. Unretained finer content
still requires authoritative pixels. Composition currently uses bounded damage
rectangles rather than a separate pixel scheduler.

Materialized projective transforms use the same render-pass layout as Warp,
with one immutable source binding. Direct and materialized transforms share
the sampling functions. Main-view shader entries specialize composed, placed
and mapped sources before compilation; the host selects the entry using the
existing presentation kind. This changes neither filtering nor pixel formats.

## Why display composition is approximate

Reduction and source-over do not generally commute, even with normal blending.
For premultiplied foreground `F`, foreground alpha `a`, background `B` and area
average `R`, the difference between reduced-layer composition and reduction of
exact composition is:

```
[R(F) + (1 - R(a)) R(B)] - R(F + (1 - a) B)
    = R(a B) - R(a) R(B)
```

This covariance vanishes for constant alpha or constant background within the
footprint. Correlated high-frequency alpha and background can produce visible
errors; normal blending alone does not guarantee equality. Compact prediction
adds contact-sampling error. Exact document output avoids both approximations.
The photographic and synthetic tests quantify particular fixtures, not a global
quality guarantee for arbitrary artwork or HDR values.

At 8 × reduction in each axis there are 64 times fewer composition pixels, but
changed exact paint must still be read, brush work still runs at document
resolution, and CPU submission, capture, source decoding and presentation remain.
The end-to-end measurements, rather than the pixel ratio, determine the benefit.

The design follows the general separation of responsive reduced feedback from
exact painting described by [Krita's Instant Preview documentation](https://docs.krita.org/en/reference_manual/instant_preview.html).
The [Vulkan render-pass sample](https://docs.vulkan.org/samples/latest/samples/performance/render_passes/README.html)
explains why attachment reads/writes and render-pass organization matter on tile
GPUs. Neither source predicts this renderer's measured speedup.

## Validation

Run on a physical GPU:

```sh
cargo test -p layer-render-wgpu --lib --offline -- --test-threads=1
cargo test -p layer-render-wgpu --test project --offline -- --test-threads=1
```

The scale tests compare against exact composition and exact output, exercise odd
edges, changing prediction footprints, opacity, ordering, source removal,
resolution changes, window overlap, active pixel transforms, cancellation,
zoom transitions, release, exact commit and linked mask/group/clipping semantics,
bounded exact queries, affine source and mask placement, sparse
source derivation across different window origins, bounded source storage,
prediction cancellation and transitions between native and reduced evaluation.
A 32-layer test edits the beginning, middle and end of the stack, checks exact
output agreement and bounds the command count while preserving untouched pages. Direct
placement presentation is compared with supersampled exact output through
rotated and nonuniform cameras. Direct transformed roots are compared with both
supersampled native output and intermediate-image presentation, including smooth
ramps, fine color patterns, opacity and both blend spaces.
Navigator tests check coarse artwork, geometry, color and alpha, coalescing,
final convergence, document replacement and abandoned submissions. Perceptual
encoding and final surface averaging do not commute; those comparisons retain the existing
transformed-source encoding rule. The tests
assert that superseded presentation allocations are absent. Project tests cover
all brush presets, save/reopen and exact undo/redo; the source-backed test also
compares the same committed stroke drawn at 12.5% and 100%.

An optional photographic oracle consumes a local 2048 × 1536 RGBA8 sRGB crop:

```sh
LAYER_DISPLAY_PHOTO_RGBA=/absolute/path/crop.rgba \
  cargo test -p layer-render-wgpu --lib --offline photographic_preview \
  -- --ignored --nocapture --test-threads=1
```

This evaluates 50, 460, 1000 and 2000 px G-Pen contacts, comparing both compact
prediction and committed display against area-reduced exact composition, and
requiring exact output equality in each case. The photo is deliberately not
included in the repository.
