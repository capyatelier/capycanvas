# GPU brush engine

The expanded professional-brush feature surface and finite pipeline model are
specified in [advanced-brush-engine.md](advanced-brush-engine.md).

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

## One semantic model, specialized GPU stages

A brush stroke is an ordered sequence of contact steps. A dry contact is the
smallest form of a step:

```text
coverage = tip(position, size, aspect, rotation, hardness)
alpha    = coverage × flow × opacity × color alpha
canvas   = premultiplied source-over(canvas, color, alpha)
```

Erase uses destination-out with the same coverage. Analytic ellipses and R8 tip
textures differ only in how they produce coverage.

The current fixed-size `Dab` already carries geometry, motion, resolved color,
coverage controls, and four material values. Dry shaders consume only the
fields they need. This is an internal Rust contract rather than a stable foreign
ABI.

One semantic model does not mean one pipeline. A renderer-private
`BrushPassPlan` compiles each frozen batch style once into its raster family,
typed material operation, sparse state attachments, reservoir use, and edge
work. Allocation, committed rendering, and prediction consume that same plan.
The selected pipeline is prepared before pen-down:

- dry analytic paint or erase uses an instanced graphics pass and fixed-function
  blending;
- a single-mask dry brush uses one filtered R8 sample;
- grain and dual-tip brushes use a separate bounded textured-coverage pipeline;
- a destination-aware step uses a source/destination GPU pass over its affected
  pages.

These are stages of one wgpu engine. Ordinary source-over ink stays on the dry
fast path and pays no destination-sampling cost.

Committed and predicted contacts use one batch scheduler. Preview is a target
policy over the same schedule: it chooses private pages or reads committed pages
directly, while watercolor snapshot, deposition, transport, and direct-page
traversal stay shared. Direct and material target-layout pipelines are stored as
finite indexed tables rather than independent feature branches. The textured
direct and material shaders also compose the same `brush_coverage.wgsl`
functions for tip, grain, and dual-tip coverage.

## Current frame path

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
- optional stroke-local coverage, deposited-wetness, and watercolor-wetness
  fields, allocated only for affected regions;
- optional pigment or water channels chosen by the brush-material version.

### Brush state

- a double-buffered 64×64 RGBA8 reservoir for brush-local spatial paint;
- loaded color/material and water;
- contact orientation and the previous pose;
- optional bristle/contact data added behind a separate feature stage.

### Per-step transient state

- tip coverage;
- motion-derived displacement;
- conservative damage and the source region needed by filtering.

Smudge/blender uses one composed backtrace per destination pixel and bounded
three-contact chunk:

```text
a(x) = tip_coverage(x) × pressure × flow
source_coordinate = reverse_compose(x, ordered_contact_motion × pull × a)
dragged = bilinear_sample(canvas_old, source_coordinate)
optical_depth += -log(1 - a) × traveled_distance / reference_distance
canvas_new(x) = alpha_safe_material_mix(canvas_old(x), dragged,
                                        1 - exp(-optical_depth))
```

It does not use the wet-paint reservoir. Sampling the old canvas once after
composing contact motion avoids the repeated displaced source silhouettes that
appear when every dab is independently composited.

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

Watercolor does not use the reservoir or a drying clock. A sparse R8 wetness
channel records water/material membership independently of pigment density and
remains wet until an explicit merge.
Uniform stroke-local coverage prevents overlapping contacts from building
stripes; a motion-directed backtrace exchanges pigment only with RGBA on that
same layer, preserves alpha, and deposits the selected color. Its live
composite derives a bounded darker rim, lighter inner band, and faint outer
bleed from the wetness boundary. Overlapping wet strokes do not create internal
edges, and dry paint outside the wetness field is not classified as watercolor.

An optional `BrushTransport` uses a brush-owned conductance texture tiled in
document space. Deposition recharges prior water toward full from the stroke's
absolute uniform coverage, with tip/conductance variation, while preserving a
two-level R8 material floor. After the update's internal
microbatches, three sparse GPU stages advance water and premultiplied pigment
from wetter neighbors using incommensurate coarse-to-fine hops. A small local
relaxation continues color mixing after adjacent watercolor regions reach
similar wetness.

The scalar conductance texture is sufficient: the shader uses its gradient on
fiber shoulders and curvature at ridge crests to derive a local tangent, then
uses a shorter cross-fiber pair to enter nearby fibers. Endpoint conductance
gates each hop. The union of expanded dab damage is the hard effect bound, and
only the first stage synchronizes the color/wetness ping-pong destinations;
later stages overwrite the same scissor without redundant page copies.
Separate wet-to-wet and wet-to-dry coefficients let watercolor favor
interaction with existing watercolor material while ink can favor dry-paper
bleed. Water above the floor decays toward it on input events, while the floor
persists until merge so material identity and live edges remain stable. No
velocity, provenance, direction texture, or second logical mask is stored.

The reservoir is initialized from selected pigment once at stroke start. Every
new contact exchanges with canvas color; selected pigment is not injected again
per dab. Wet contacts are submitted in ordered microbatches of at most three to
bound ping-pong/pass overhead without returning to display-frame-rate exchange.
Smudge uses deterministic chunks of at most three contacts, additionally bounded
by traveled distance and damage. Its incomplete live chunk is rendered through
the replaceable GPU preview and becomes persistent only at a stable chunk
boundary or pen-up, so results do not depend on display cadence. Liquify retains
one ordered contact per step. Contact placement and dynamics are sequential CPU
work; canvas sampling, coverage, material transfer, state writes, reservoir
exchange, and deformation are parallel GPU work. For non-watercolor wet brushes,
the sparse R8 wetness map records deposition only.

## Read/write rule

A destination-aware pass never samples and writes the same texture subresource
in place. It reads an immutable source view and writes a destination view, then
swaps them. The same rule applies to the brush reservoir. This is deterministic,
removes data races between neighboring pixels, and stays within portable WebGPU
resource rules. It also makes damaged-region checkpoints possible for undo.

When sparse storage is justified, affected regions gain a filtering halo and
neighbor material state. The public API still reports only document-space
damage; tile dimensions, halo size, ping-pong textures, and dispatch lists stay
inside `layer-render-wgpu`.

## Ordering and batching

Dry contacts may be instanced together because fixed-function blending preserves
their draw order. Wet paint uses ordered microbatches of at most three contacts.
Smudge contacts are reverse-composed in deterministic chunks of at most three;
distance-normalized influence keeps strength stable when spacing changes, while
short chunks bound the visual approximation to sequential destination feedback.
Liquify depends directly on the preceding destination and uses one contact per
source/destination swap. All pixels inside each step remain parallel. No public
API exposes contact reordering.

## Persistence and undo

All current state is a deterministic derivative of immutable strokes and can be
rebuilt from them after undo, load, or device loss. A future time-evolving
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
- Acceptance at 120 Hz remains end-to-end input-to-present p99 below 8.33 ms on
  each target device; offscreen GPU time alone is necessary but not sufficient.

## Current implementation boundary

Implemented now:

- analytic and R8-mask contact coverage;
- grain and dual-tip coverage;
- pressure/dynamics-resolved ordered contacts;
- premultiplied paint and erase;
- blend modes, smudge pickup/pull/blur, wet deposition, and Oklab mixing;
- a persistent spatial wet-brush reservoir with deterministic charge depletion;
- layer-wide same-layer watercolor mixing with stroke-uniform coverage and a
  live wetness-driven morphology edge;
- brush-owned long/short, broad/narrow conductance fields and bounded
  event-driven pigment transport with separate wet/dry rates;
- lazy sparse R8 stroke-coverage, canvas-wetness, and watercolor-wetness pages;
- push, twirl, pinch, expand, crystals, and edge deformation;
- sparse persistent GPU layer pages and sparse prediction pages;
- GPU memory/page metrics through the C ABI;
- incremental damage composition;
- explicit RGBA8 export readback;
- 15 legacy plus 10 painter scenarios at 4096×4096/32 visible layers through
  the public C ABI.

Reserved by this design, not implemented now:

- reconstruct snapshots;
- bristle simulation;
- timed drying, pigment reactions, and checkpoint-based undo for that
  time-evolving state.

The layer-wide watercolor is an efficient artistic model, not a fluid
simulation. It has three instantaneous event-driven relaxation stages, but no
background stepping, evaporation clock, persistent velocity, or pigment
separation. Watercolor material membership remains persistent until explicit
merge; the separate deposited-wetness field belongs only to other wet brushes.

## References

- [Krita Color Smudge brush engine](https://docs.krita.org/en/reference_manual/brushes/brush_engines/color_smudge_engine.html)
- [Krita Color Smudge implementation](https://github.com/KDE/krita/tree/master/plugins/paintops/colorsmudge)
- [libmypaint smudge sampling](https://github.com/mypaint/libmypaint/blob/master/mypaint-brush.c)
- [Adobe Mixer Brush model](https://helpx.adobe.com/photoshop/using/painting-mixer-brush.html)
- [Wetbrush hybrid particle/grid simulation](https://wanghmin.github.io/publication/chen-2015-wgb/)
- [Efficient Rendering of Linear Brush Strokes](https://jcgt.org/published/0007/01/01/)
- [wgpu supported native and Web backends](https://github.com/gfx-rs/wgpu)
- [wgpu storage texture access](https://docs.rs/wgpu/latest/wgpu/enum.StorageTextureAccess.html)
