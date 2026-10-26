# GPU brush engine

[Technical documentation](../README.md)

## Decision

Layer has one production pixel engine: `layer-render-wgpu`. The CPU interprets
input, evaluates brush dynamics, places ordered contacts, and updates the
document. wgpu rasterizes contacts, blends paint, composites layers, and keeps
the resulting pixels on the GPU through presentation.

A hardware GPU is a painting requirement. Adapter creation rejects software
devices, and a missing or unusable GPU disables painting rather than selecting
another rasterizer. This keeps one numerical implementation as brushes gain
destination-dependent behavior.

The engine implements solid/textured dry contacts and destination-aware
smudge, spatial-reservoir wet transfer, layer-wide watercolor, uniform
accumulation, live and optional post-stroke edges, blend, and liquify contacts.
The same packet and native UI boundary select all of them.

Pencil, charcoal and the new ink family use the [swept contact
model](../internals/contact-brush-engine.md). Graphite uses normalized flow
deposition against fixed paper; ink uses its analytic contact profile. These
contacts carry the preceding pose and interpolate their footprint on the GPU.

## One semantic model, specialized GPU stages

A brush stroke is an ordered sequence of contact steps. A dry contact is the
smallest form of a step:

```text
coverage = tip(position, size, aspect, rotation, hardness)
request  = coverage × strength × color alpha
source   = premultiplied(color, 1)
canvas   = (1 − request) × canvas
         + request × ((1 − opacity) × stroke_start + opacity × source)
```

Erase substitutes zero for `source`, retaining `(1-opacity)` of the stroke's
starting paint. Analytic ellipses and R8 tip
textures differ only in how they produce coverage. Source-over runs in the
document's blend space: on encoded values in a Perceptual document
([rendering](../internals/rendering.md#brushes-and-healing)).

The configured `flow` field carries Strength. Analytic dry media multiply
calibrated pressure by it on the CPU and store unit `Dab.flow`; other media retain their own
transfer. Dry swept contacts derive `request` from exponential optical transfer.
Opacity is retained separately in `DabStyle`;
partial-opacity pointwise strokes retain the starting RGBA of touched pages.
Contact-family specialization preserves the per-batch Opacity test. Family flags
describe the contact and cannot disable its starting-paint composite. Committed
and predicted pages use the same composite at every display resolution.
This composite permits pigment changes after a pixel reaches its alpha ceiling.
Normal paint and Erase use the direct recurrence above. Other blend modes
recover the full stroke result, apply their blend, and reapply stroke Opacity.
All arithmetic uses the deposition color space. Full-opacity Flow deposition
needs neither starting-paint copies nor coverage state.
Uniform retouching retains raw contact coverage independently of source alpha;
source transparency limits the accumulated transfer before stroke Opacity is applied.
Full-opacity beveled blocks and partial-opacity ordinary dry contacts skip
contact work at an exact final RGBA value for normal constant-color batches.
Variable colors, selections, locked alpha, bristles and coverage passes retain
their existing evaluation.

The current fixed-size `Dab` already carries geometry, motion, resolved color,
coverage controls, and four material values. Dry shaders consume only the
fields they need. This is an internal Rust contract rather than a stable foreign
ABI.

One semantic model does not mean one pipeline. A renderer-private
`BrushPassPlan` compiles each frozen batch style once into its raster family,
typed material operation, sparse state attachments, reservoir use, and edge
work. Allocation, committed rendering, and prediction consume that same plan.
Readiness keys include the blend mode, which selects the material module;
returning to a blend reuses its prepared pipelines.
The selected pipeline is prepared before pen-down:

- dry analytic paint or erase uses an instanced graphics pass and fixed-function
  blending in a Linear light document;
- a single-mask dry brush uses one filtered R8 sample;
- grain brushes use a separate bounded textured-coverage pipeline;
- a destination-aware step uses a source/destination GPU pass over its affected
  pages.

These are stages of one wgpu engine. Source-over materials use the direct path;
the new ink presets use coverage state to grow a coherent stroke.

Committed and predicted contacts use one batch scheduler. Preview is a target
policy over the same schedule: it chooses private pages or reads committed pages
directly, while watercolor deposition, transport, and direct-page
traversal stay shared. Direct and material target-layout pipelines are stored as
finite indexed tables rather than independent feature branches. The textured
direct and material shaders also compose the same `brush_coverage.wgsl`
functions for tip and grain coverage.

Watercolor transport batches resident page dispatches within each of its three
compute steps. Each step reads unchanged pigment and wetness and swaps their
active sides after all pages finish. Borrowed decoded neighborhoods are consumed
before another source preparation can evict them. Region uniforms and scratch
jobs retain their capacity across batches.

## Current frame path

### Brush color coordinates

`BrushSnapshot` primary/secondary RGB values are straight linear document
coordinates, with separate coverage alpha in [0,1]. Profiled preset/swatches must
be converted by their owner before configuring the brush. `CanvasEngine` creates
`DabGenerator` with the document RGB space; standalone callers use
`DabGenerator::new(space)`. The default standalone generator is sRGB. Resetting,
cloning, cursor evaluation, active-contact reconstruction and late sensor
correction retain the space. Integer8/integer16 use identical dab arithmetic.

Secondary-color interpolation is linear in document RGB and preserves exact
endpoints. Hue/saturation/lightness dynamics operate in transfer-encoded document
RGB, then decode back to linear RGB. This is a document-coordinate editing
policy; CSS HSL itself is defined for sRGB. There is no intermediate conversion
or gamut mapping to sRGB. For extended input, evaluate HSL in the smallest shared
channel interval containing both [0,1] and the input, then restore that interval.
This preserves negative/above-one channels during hue rotation; ordinary bounded
RGB uses the usual HSL cube. Saturation/lightness controls remain bounded within
the chosen interval. Disabled dynamics bypass transfer and HSL round trips.

The CPU evaluates these color coordinates with Float64 intermediates and emits
Float32 dabs; there is no integer or FP16 narrowing. The algebraic HSL chroma
factor `2 * min(L, 1-L)` avoids subtractive cancellation at very dark colors.
GPU coverage, material blending and native tile publication remain shared with
the rest of the renderer. The expanded GTK color/preset UI and its profile
ownership remain part of the unfinished milestone 2 integration.

### Submission

```text
native coalesced samples
        │
        ▼
pressure mapping + deterministic spacing          CPU, layer-engine
        │
        ▼
contiguous fixed-size Dabs + batch style + damage borrowed FramePacket
        │ one contiguous dab upload + one small style upload
        ▼
instanced contact quads + coverage + blend        GPU, layer-render-wgpu
        │
        ▼
persistent RGBA8 layer textures
        │ damage scissor
        ▼
background + visible-layer composition            GPU composite texture
        │
        ├──► platform surface presentation         no readback
        └──► explicit export readback              cold path only
```

Each `DabBatch` carries its stroke ID and explicit start/end flags. Each frame
contains only newly emitted contacts. Existing strokes are not
replayed during normal pen movement. Undo, redo, load, cancellation, and device
recreation explicitly request a rebuild.

Paint and predicted pixels use private 256×256 `RGBA8Unorm` pages allocated only
where contacts touch. Empty layers allocate no pixel storage. The composite is
document-sized so presentation remains one sampled texture. Damage scissors
bound raster and composition work, and page coordinates remain private to the
renderer.

## Destination-aware extension

The full brush retains three kinds of state.

### Canvas state

- dry premultiplied color;
- optional stroke-local coverage and watercolor-wetness fields, allocated only
  for affected regions;
- optional pigment or water channels chosen by the brush-material version.

### Brush state

- a double-buffered 64×64 RGBA32Float reservoir for brush-local spatial paint;
- loaded color/material and water;
- contact orientation and the previous pose;
- optional bristle/contact data added behind a separate feature stage.

### Per-step transient state

- tip coverage;
- motion-derived displacement;
- conservative damage and the source region needed by filtering.

Smudge and the blenders other than Dither compose one backtrace per destination pixel and
contact chunk, then mix toward a field chosen by the blender kind:

```text
a_i(x)        = tip_coverage_i(x) × flow_i
exposure      = Σ a_i × |motion_i| / short_axis_diameter_i
amount        = 1 - exp(-rate × exposure)
trace         = x - clamp(Σ motion_i × pull × a_i × lane_i(x), 128 px)
radius        = min(reach × diameter × sqrt(rate × exposure / amount), 112 px)
field         = blender_field(kind, x, trace, radius)   // in the mixing space
alpha_new(x)  = alpha_old(x) + max(alpha_field - alpha_old(x), 0) × amount
color_share   = amount × min(alpha_field, 1) / alpha_new(x)
color_new(x)  = mix_in_mixing_space(color_old(x), color_field, color_share)
canvas_new(x) = color_new(x) × alpha_new(x)
```

Alpha rises toward the field's alpha at the rate the colour mixes and never
falls, so consecutive chunks compose like one longer pass: two passes at half
the exposure raise alpha as far as one full pass. Paint pulled into
transparency therefore fades smoothly instead of banding at the chunk spacing.
Colour shares account for the resulting coverage. Empty pickup leaves the
destination unchanged; an empty destination takes the field's straight colour.
Colour division uses the actual positive normal alpha.

A kind that touches paper, with Texture strength above 0, compiles a paper
variant of its kernel. It reads the paper once per pixel after the contact loop
and evaluates the dry-media contact `C` with the chunk's peak pressure and the
tip's gain, engagement and recess; `C` scales the exposure and the trace before
the field is sampled ([decision record](../history/brushes/paper-aware-blenders.md)).
The variant takes eleven colour taps instead of twelve, so it issues as many
texture reads as the paper-blind kernel; the Bristle Blender's two jittered side
samples read the nearest pixel. Smudge kernels never multiply the
footprint by grain. Soft kernels compiled for a brush that sets a lift, which
only the Tissue does, lower the result by `(1 − exp(−lift · exposure)) · opacity`.

Exposure counts distance travelled, so spacing changes neither how far paint
moves nor how much it blends. The radius grows with the square root of
exposure, so one pass averages as widely as the many short passes it replaces.
Each kind compiles its own page kernel from
[`material_blend.wgsl`](../../crates/layer-render-wgpu/src/material_blend.wgsl),
selected by the module constant `BLENDER`: disc averages for Soft, Blur and Hue,
a paper-guided regression for Grainy, frequency separation for Texture-Keeping,
a clamped Catmull-Rom backtrace for the Knife, eight-sector minimum variance for
Painterly, Voronoi cells for Crystal, curl-noise advection for Marbling, a
structure-tensor streamline for Comb.
The 128 px trace and 112 px radius keep
every sample inside the page's 3×3 neighbourhood, so these kernels need no
gather, and a sample inside the page's own texture skips the neighbourhood
switch. They do not use the wet-paint reservoir, and only the Tissue's lift
lowers alpha.

Grainy's paper regression bounds its gain so every sampled pigment retains a
nonnegative weight. Its weaker Oklab chroma response reassociates chroma with
the reconstructed alpha. Texture-Keeping divides its frequency bands by their
actual coverage. Hue preserves extended RGB until native publication, and
Comb uses the document profile's luminance coefficients.
Painterly compares sectors against its first candidate, without an absolute
variance ceiling that could reject every sector on HDR paint. Each sector
accumulates its variance from each tap's deviation from the mean of the taps
before it, so a flat sector has exactly zero variance at any scale.

Dither uses [`material_dither.wgsl`](../../crates/layer-render-wgpu/src/material_dither.wgsl).
It samples seven fixed disc offsets with exact texel loads from stroke-start
paint, plus the destination's original RGBA tuple. Its radius is
`min(Spread × captured Size, 112 px)` throughout the stroke. Candidates are
snapped to the loaded texel's center before testing their footprint and selection.
Each neighbor accumulates its own exposure `Eᵢ`: destination contact dose
(coverage, flow, motion, Strength and selection) times the source's actual tip
coverage at that contact. Its weight is `Opacity × (1 − exp(−Eᵢ)) / 7`.
The original keeps all remaining weight. A candidate outside the contact cannot
gain influence, and a newly reached candidate receives only its own exposure.
Earlier valid influence survives later movement, shrinking or rotating contacts.
Source selection coverage scales each neighbor's weight, with excluded or
out-of-document weight returned to the original. A 19-comparison sorting network
orders the eight immutable colors by lightness, then RGBA, on first contact.
One integer-valued float caches the eight three-bit sample indices. The
permutation is nonzero and below 2²⁴, so its representation is exact. Later
updates use this fixed order and load only the chosen color. Zero-weight entries do not affect the cumulative distribution.
A stroke-stable 256×256 Bayer rank selects one positive-weight interval.
It never interpolates the chosen sample with the destination. Antialiased
shades have their sampled area weight rather than priority for being near an
average. This is a seven-neighbor estimate, not a neighborhood histogram.

Dither stores seven source-pair exposures and the cached palette permutation in
two 256×256 slabs of one RGBA32Float texture. The top slab's red channel holds
zero before classification, the positive permutation, or `−1` for a uniform
palette. The other seven channels hold saturated integer exposure ticks,
rounding each contact to 2⁻²⁰ units and saturating at 16. Addition stays exact
through 2²⁴ ticks, so frame partitions do not change the result. Prediction copies
the full state, and each pass preserves required state outside its dirty rectangle.

Explicit post-stroke edges add a third slab holding total destination exposure
in its red channel. Uniform pixels still accumulate this total, and the edge
pass converts its ticks to normalized coverage. Ordinary Dither needs no total
exposure: zero donor weights already select the original. State pages match by
coordinate and exact slab count, including prediction and queued brush changes.

Original pages survive native-cache pressure until correction and replay end;
their color companions remain resident during that period. Alpha lock converts
candidates to the destination alpha before ordering and replaces transparent
candidates with the original. The final donor load repeats that conversion.
Layer color-mode enforcement and explicitly requested post-stroke edges retain
their usual policies. Ordinary Dither can choose transparency and therefore
lower alpha.

Dither state pages retire with the stroke originals after correction and replay
end. Their two ping-pong surfaces require 4 MiB per touched 256×256 page, or
6 MiB with post-stroke edges. A narrow tip or large Spread can leave no eligible
neighbor among the seven fixed candidates; those pixels retain original paint.

The engine merges consecutive Dither contacts into batches of at most 64,
retaining Smudge's damage bound and charging the frame budget for equivalent
six-contact groups. Sparse page plans pass only intersecting contacts in their
original order. Fixed sources and exact exposure sums make this grouping
independent of the resulting paint, while avoiding a full-page state transfer
after every six contacts.

Analytic ellipse tips skip source footprint evaluation only where every candidate
is provably inside the solid core: the normalized destination distance plus
`(sampling radius + 1 px) / minimum tip radius` must fit inside that core.
The extra pixel bounds source-texel rounding. Other pixels and tip types use the
shared footprint evaluator.

The first positive destination dose classifies the fixed palette. If every
eligible donor equals the original after alpha lock, the permutation channel
stores `−1` as a uniform-palette marker. Such pixels skip donor accumulation;
only explicit post-stroke edges need their total destination dose. This
classification uses static bounds and selection, including donors not yet
reached by the tip. A new stroke resets it through the usual stroke owner check.

Wet paint uses the separate reservoir path. Loaded oils and palette knives keep
spatial brush-local state:

```text
reservoir_new(u) = ordered_exchange(
    reservoir_old(u),
    sample(canvas_old, previous_contact_pose(u)),
    pickup × contact(u)
)

dragged(x) = sample(canvas_old, x - smear × a(x) × motion)
source(x) = mix(dragged(x), reservoir(local(x)), dilution, paint_amount)
canvas_new(x) = deposit(canvas_old(x), source(x), density × attack × a(x))
```

Reservoir exchange and deposition weight straight colours by their pigment
coverage, including faint pickup. With no loaded paint or refill, Wet Blender
and Color Mixer raise coverage only toward sampled coverage. Repeated contact
does not turn uniformly faint paint opaque; density controls transfer rather
than inventing coverage.

Watercolor does not use the reservoir or a drying clock. A sparse wetness
channel records water/material membership independently of pigment density and
remains wet until an explicit merge.
Stroke-local accumulated coverage bounds pigment transfer; a motion-directed
backtrace exchanges pigment only with RGBA on that
same layer, preserves alpha, and deposits the selected color. Its live
composite derives a bounded darker rim, lighter inner band, and faint outer
bleed from the wetness boundary. Overlapping wet strokes do not create internal
edges, and dry paint outside the wetness field is not classified as watercolor.

An optional `BrushTransport` uses a brush-owned conductance texture tiled in
document space. Deposition recharges prior water toward full from the stroke's
absolute uniform coverage, with tip/conductance variation, while preserving the
2/255 material floor. After the update's internal
microbatches, three sparse GPU stages advance water and premultiplied pigment
from wetter neighbors using incommensurate coarse-to-fine hops. A small local
relaxation continues color mixing after adjacent watercolor regions reach
similar wetness.

The scalar conductance texture is sufficient: the shader uses its gradient on
fiber shoulders and curvature at ridge crests to derive a local tangent, then
uses a shorter cross-fiber pair to enter nearby fibers. Endpoint conductance
gates each hop. The union of expanded dab damage is the hard effect bound, and
only the first stage writes whole color/wetness ping-pong destinations, copying
texels outside its region; later stages overwrite the same region without page copies.
Separate wet-to-wet and wet-to-dry coefficients let watercolor favor
interaction with existing watercolor material while ink can favor dry-paper
bleed. Water above the floor decays toward it on input events, while the floor
persists until merge so material identity and live edges remain stable. No
velocity, provenance, direction texture, or second logical mask is stored.

The reservoir is initialized from selected pigment once at stroke start. Every
new contact exchanges with canvas color; selected pigment is not injected again
per dab. Wet contacts are submitted in ordered microbatches of at most three to
bound ping-pong/pass overhead without returning to display-frame-rate exchange.
Smudge uses deterministic chunks of at most six contacts, additionally bounded
by 0.24 diameters of the tip's long axis or 128 px of travel, and by damage.
Its incomplete live chunk and the predicted contacts are rendered through the
replaceable GPU preview in one pass up to the travel limit, and become
persistent only at a stable chunk boundary or pen-up, so results do not depend
on display cadence. Liquify composes its coordinate map in deterministic chunks
of at most eight contacts. Contact placement and dynamics are sequential CPU
work; canvas sampling, coverage, material transfer, state writes, reservoir
exchange, and deformation are parallel GPU work. Non-watercolor wet brushes keep
no canvas wetness; their state is the reservoir.

## Read/write rule

A destination-aware pass never samples and writes the same texture subresource
in place. It reads an immutable source view and writes a destination view, then
swaps them. The same rule applies to the brush reservoir. This is deterministic,
removes data races between neighboring pixels, and stays within portable WebGPU
resource rules. It also makes damaged-region checkpoints possible for undo.

Smudge, wet and watercolor brushes write their destination pages from
compute kernels through write-only storage. A page job stores the material
result inside its dirty rectangle and the unchanged source texel elsewhere,
including stroke coverage, so no page copy or coverage clear precedes it. One compute pass,
shared by a frame's consecutive batches and their watercolor transport,
dispatches every job of a batch and, after a committed wet
batch's jobs, its reservoir exchange. At stroke start the exchange begins from
the selected pigment instead of the stored reservoir. Watercolor stores wetness
only inside the dirty rectangle, except that the first job of an update on a
page also stores the pre-update wetness outside it; a later microbatch never
stores less, so no blend is needed. Tile records for all of a frame's batches
follow one another and upload before the frame's other commands.
Each dispatch is its own WebGPU usage scope, so a
later dispatch reads earlier writes in recorded order. Only a job that binds
decoded source tiles ends the pass first, because preparing it may reuse a cache
slot that an earlier job still reads.

Liquify uses separate coordinate-map and color-reconstruction passes. Its map
chunks read earlier immutable map tiles; color reconstruction reads the
session's immutable starting pixels. Each pass batches the affected tiles, and
first-touch source mip reductions share another pass. See
[Liquify](../internals/liquify.md#performance-and-memory) for source grouping,
display-sized prediction and memory ownership.

When sparse storage is justified, affected regions gain a filtering halo and
neighbor material state. The public API still reports only document-space
damage; tile dimensions, halo size, ping-pong textures, and dispatch lists stay
inside `layer-render-wgpu`.

## Ordering and batching

Dry contacts may be instanced together because fixed-function blending preserves
their draw order. Wet paint uses ordered microbatches of at most three contacts.
Smudge contacts are reverse-composed in deterministic chunks of at most six;
distance-normalized influence keeps strength stable when spacing changes, while
short chunks bound the visual approximation to sequential destination feedback.
Liquify reverse-composes up to eight dab maps before interpolating its retained
map once, with stable chunk boundaries and a replaceable unfinished preview.
All pixels inside each step remain parallel. No public API exposes contact
reordering.

The Liquify presets are Push, Twirl Counterclockwise, Twirl Clockwise, Pinch,
Expand, Crystals, Edge and Reconstruct. Pinch shrinks content toward the dab
centre, Expand bulges it, and Edge pulls toward the stroke line. Twirl modes are
named by the visible turn on the y-down canvas. Crystals scatters by whole
pixels; Reconstruct restores local deformation toward the identity map.
Momentum continues the stroke after pen-up with bounded travel and strength
decay. The GPU oracle
`liquify_modes_move_a_marker_in_their_labelled_direction` measures each
direction, and the CPU source bounds in `material_sources.rs` mirror the shader.

## Persistence and undo

Authored pixels and undo history survive load and device loss. Liquify keeps its
map with each live raster revision, but does not serialize it: a restored pixel
revision without a map starts a new session from those pixels. A future time-evolving
wet-media simulation will require versioned material checkpoints; replaying only
input points would then couple old projects to wall-clock evolution and changing
simulation code. That persistence format is deliberately deferred.

## Performance requirements

- No brush pixel is rasterized on the CPU.
- Software GPU adapters are rejected; there is no alternate raster backend.
- No layer pixel is uploaded or read back during contact.
- Pipelines, layouts, and samplers are created before pen-down.
- A frame uploads the contiguous contacts once and a small aligned style table
  once.
- Raster and composition use conservative damage scissors.
- Production submission never waits for GPU completion.
- The benchmark separately reports input-to-submit and serialized
  input-to-completed-work latency.
- Acceptance follows the [performance targets](../PERFORMANCE_TARGETS.md);
  offscreen GPU time alone is necessary but not sufficient.

## Current implementation boundary

Implemented now:

- analytic and R8-mask contact coverage;
- grain coverage;
- pressure/dynamics-resolved ordered contacts;
- premultiplied paint and erase;
- blend modes, smudge pickup/pull/blur, wet deposition, and per-brush color
  mixing in Oklab, linear light or encoded (Classic) values;
- a persistent spatial wet-brush reservoir with deterministic charge depletion;
- layer-wide same-layer watercolor mixing with stroke-uniform coverage and a
  live wetness-driven morphology edge;
- brush-owned long/short, broad/narrow conductance fields and bounded
  event-driven pigment transport with separate wet/dry rates;
- lazy sparse stroke-coverage and watercolor-wetness pages;
- push, twirl, pinch, expand, crystals, and edge deformation;
- sparse persistent GPU layer pages and sparse prediction pages;
- stroke-start page copies and a cached reference composite for retouching
  strokes, sampled without uploads or waits during contact
  ([retouching sources](../internals/brushes.md#retouching-sources)), the
  Clone Stamp pass that reads them per page ([Clone Stamp](../internals/brushes.md#clone-stamp)),
  and pen-up healing in the stroke's last submission
  ([Healing Brush and Spot Healing Brush](../internals/brushes.md#healing-brush-and-spot-healing-brush));
- GPU memory/page metrics through the C ABI;
- incremental damage composition;
- explicit RGBA8 export readback;
- 15 legacy plus 11 painter scenarios at 4096×4096/32 visible layers through
  the shared `CanvasEngine`.

Reserved by this design, not implemented now:

- reconstruct snapshots;
- bristle simulation;
- timed drying, pigment reactions, and checkpoint-based undo for that
  time-evolving state.

The layer-wide watercolor is an efficient artistic model, not a fluid
simulation. It has three instantaneous event-driven relaxation stages, but no
background stepping, evaporation clock, persistent velocity, or pigment
separation. Watercolor material membership remains persistent until explicit
merge.

## References

- [Krita Color Smudge brush engine](https://docs.krita.org/en/reference_manual/brushes/brush_engines/color_smudge_engine.html)
- [Krita Color Smudge implementation](https://github.com/KDE/krita/tree/master/plugins/paintops/colorsmudge)
- [libmypaint smudge sampling](https://github.com/mypaint/libmypaint/blob/master/mypaint-brush.c)
- [Adobe Mixer Brush model](https://helpx.adobe.com/photoshop/using/painting-mixer-brush.html)
- [Wetbrush hybrid particle/grid simulation](https://wanghmin.github.io/publication/chen-2015-wgb/)
- [Efficient Rendering of Linear Brush Strokes](https://jcgt.org/published/0007/01/01/)
- [wgpu supported native and Web backends](https://github.com/gfx-rs/wgpu)
- [wgpu storage texture access](https://docs.rs/wgpu/latest/wgpu/enum.StorageTextureAccess.html)
