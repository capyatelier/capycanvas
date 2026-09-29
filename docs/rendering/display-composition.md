# Region and scale composition

Native presentation can compose eligible paint stacks at the resolution the view
needs. Authoritative paint remains in exact document tiles. Presentation pixels
never become paint, history, export, sampling, or project backing.

The implementation is in `crates/layer-render-wgpu/src/scene/scale.rs`. The
layer order, isolated and Pass Through groups, clipping bases and adjustment-chain boundaries
are shared with exact composition in `scene/stack.rs`. The
[TCL evaluation](../development/display-composition-20260927.md) records the
measured gains, controls and remaining limits.

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
records its representation; a change invalidates all retained pages. Encoded
levels cannot supply linear transform inputs. Source identity, immutable native captures, brush damage
and retired prediction footprints invalidate the affected pages at every level.
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
source-over runs are balanced using associativity, with each layer's opacity
applied before regrouping. Power-of-two grouping boundaries and empty layer
positions preserve existing branches when painting starts; transparent operands
require no image or blend pass. Group opacity, masks, clipping and non-normal blends
remain expression boundaries. Exact and reduced composition share blend formulas
and document-depth flags, including clipped layers and extended float colors.
The document blend space invalidates composed branches and selects the shared
blend formulas. Pointwise effects decode and encode at their fused boundary;
image-boundary effects receive linear inputs and encode only their final output.
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
its unchanged branches fit the budget. Placed sources and masks use the common transform
resampler. Their most magnified axis determines source resolution; an additional
level of detail and up to four samples per axis limit placement-edge error.
Unit-scale aligned inputs use one sample per pixel.
Interior samples use the hardware linear sampler. Boundary samples account
for partially filled source texels and the smaller final output cell. Masks retain their default coverage outside their local image.
A normal placed layer over a constant backdrop keeps its source and transform
until another layer needs its pixels. If it reaches the root unchanged, the
presenter samples it directly into the surface and navigator. This avoids an
intermediate canvas image and a second resampling step. Its neighboring source
levels share the scene's validity and memory allowance. Other stacks materialize
their result and derive an adjacent output mip for trilinear presentation.
Navigator sampling subdivides footprints that span more than four coarse-source
texels along either axis.

The expression graph fills a padded, page-aligned viewport window at the selected
resolution. Retained root and branch images have viewport bounds; pointwise
temporary results reuse 256-texel working tiles at every resolution. Every value and output carries its own grid,
so a tile can compose directly into a viewport image without changing document
coordinates. Aligned native sources borrow paint or decoded tiles directly.
Decoded tiles carry leases until their consuming commands are encoded; eviction
cannot replace a borrowed tile. Other native sources use the shared paint and
mask gatherers; minified sources use retained reduced windows or streamed tiles. Composition batches
uniform records and compute dispatches, flushing before source preparation,
transforms, effects or reduction consume or replace their inputs. Admission
reserves the root, its adjacent mip,
bounded working tiles and placement gathers separately. Panning reuses valid
overlap and renders only newly required or changed pages. Shared sparse contact
plans restrict pointwise edits to touched pages, including retired prediction
pages. Spatial and global programs retain their dependency propagation. A full-document
overview serves the navigator and pixels outside the window. Covered overview
regions derive from completed detail; uncovered regions use reduced composition,
avoiding duplicate exact-source work.
The overview remains coarser than the window, including at reduced zoom.
Image-boundary effects currently retain whole-document dependency coverage at
reduced resolution.
Retained windows are admitted again when their source requirements change.
Presentation regeneration advances display damage independently of the artwork
revision used by document previews; camera motion does not publish an artwork edit.

The cache replaces the full composite, live display pyramid/detail atlas,
transform static copies and obsolete scene image intermediates for its supported
stack. It does not keep these allocations alive underneath the new display.
Shader recipes belong to the device and survive cache or level replacement.
The original scene executor is the exact-query and unsupported-stack fallback;
`live_display.rs` is explicitly an exact-composition presentation fallback.
There is no runtime benchmark switch or second legacy copy of the supported
paint compositor.

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
Mesh triangles rasterize color directly into the graph's target. Fragment
derivatives choose the source footprint, and later triangles replace earlier
ones where a mesh folds. Kept coverage and the mesh share one render pass.
Split selections use finer
inputs and four samples to limit moved/unmoved coverage error; edge texels
use their actual extent. Exact queries evaluate native source tiles for their
requested dependency window, retiring tiles from the previous window. Display
motion has no full-layer preview or post-release settling queue. Scalar-mask
transactions currently provide native coverage to the same graph.

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
committed native pixels. There is no background exact-paint queue or settling
backlog to hide from the benchmark.

## Supported contract

The region executor admits native paint layers, isolated groups, clipping
stacks, all blend modes, paper, affine placements and scalar masks. Mask
inspection remains presentation only. Effects explicitly declaring
display-resolution support execute in the reduced graph with document coordinates,
layer masks and clipping. Image passes expand dependency regions and damage by
their sampling footprints; intermediate results populate the halo needed by
later passes. Gaussian Blur prepares scaled kernels for the evaluation grid.
Native-resolution effects and native views of image-boundary effects, along
with persistent watercolor state, still use the exact presentation executor;
these dependencies have not yet migrated. Insufficient admission also retains
that executor. Advanced brushes keep their exact temporary evaluator; simple
analytic dry contacts without grain, selection, alpha lock or edge effects can
use compact tails.

An unchanged view may retain one neighboring composed output within the same
component budget. Returning to it reuses its pixels; artwork changes retire it.
Transform input allocations are reserved in admission. Source levels have independent ownership and validity, so returning through a
native view need not reread a previously reduced photo. Unretained finer content
still requires authoritative pixels. Composition currently uses bounded damage
rectangles rather than a separate pixel scheduler.

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
prediction cancellation and entry/exit through the filter fallback.
A 32-layer test edits the beginning, middle and end of the stack, checks exact
output agreement and bounds the command count while preserving untouched pages. Direct
placement presentation is compared with supersampled exact output through
rotated and nonuniform cameras. The tests
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
