# Painterly GPU paint state

Paths under `artifacts/` refer to ignored local outputs, not files shipped in
this repository. See [publication notes](publication.md#publication-checks).

## Decision

Painterly brushes extend the existing incremental wgpu material pass. They are
not a second renderer and have no CPU pixel implementation. `layer-engine`
computes sequential input, dynamics, contact placement, stroke distance, and
explicit stroke start/end boundaries. `layer-render-wgpu` owns every canvas
sample, transfer, coverage update, reservoir exchange, wetness write, and edge
operation.

This milestone implements three bounded kinds of state:

| State | Storage | Lifetime | Current use |
| --- | --- | --- | --- |
| Stroke coverage | two sparse R8 256×256 pages per touched color page | keyed to layer + stroke ID | uniform accumulation |
| Canvas wetness | one sparse R8 256×256 page per touched color page | persistent, deterministic on replay | non-watercolor wet brushes only |
| Watercolor wetness | one logical sparse R8 channel, ping-ponged during updates | persistent until merge/replay | localized transport, material identity, and live edges |
| Wet brush reservoir | two 64×64 RGBA8 textures | active wet stroke; reused | spatial carried RGB + paint amount for oils/gouache |

Watercolor does not use the non-watercolor deposited-wetness field or reservoir.
Its R8 value is both membership and localized water amount, independent of
pigment alpha. Every positive region remains wet until the user merges the
result and continues on a new layer. Untouched dry media on the same raster
layer is therefore not classified as watercolor.

## One incremental batch contract

`DabBatch` now includes `stroke_id`, `stroke_start`, and `stroke_end`. This is the
only lifecycle contract needed by the renderer:

- `stroke_start` initializes a wet reservoir when needed and lazily resets
  coverage pages;
- every persistent batch processes only newly emitted contacts; wet batches
  carry the reservoir forward, while smudge and liquify advance their own
  ordered canvas feedback;
- `stroke_end` optionally applies the legacy final edge pass for non-watercolor
  brushes that request it;
- preview batches fork committed coverage into recyclable sparse scratch and
  read any wet reservoir state, but cannot mutate persistent color or material
  state.

The engine never merges batches across stroke identity or past a stroke-end
boundary. Undo, load, cancellation, and device recreation use the same full
replay path and reproduce current state deterministically.

## Material transfer

The material fragment invocation owns one destination pixel. It reads an
immutable 3×3 neighborhood of source color pages and evaluates overlapping
contacts in submission order:

```text
coverage       = tip × grain
requested      = coverage × flow × opacity × density × attack × remaining_charge
source alpha   = requested                                      (flow mode)
source alpha   = alpha increment to max(stroke coverage, requested) (uniform mode)
pickup         = source(position - motion × pull), optionally blurred
carried paint  = spatial reservoir sample (wet only)
paint color    = linear/Oklab mix(pickup, carried paint)
color output   = premultiplied source-over(destination, paint color, source alpha)
wetness output = max(old wetness, coverage × configured wetness)
```

Transparent premultiplied source pixels do not contain a meaningful straight
RGB value. Wet pickup from an empty pixel preserves carried reservoir pigment,
and partially covered canvas cannot reduce reservoir amount. Deterministic
charge depletion is the only current material-loss control. Selected pigment
loads the wet reservoir once at stroke start and is not reintroduced by later
contacts.

Smudge and Natural Blender do not use that reservoir. Their fragment path
reverse-composes the ordered contact motion into one semi-Lagrangian backtrace,
bilinearly samples the immutable canvas once, and transports straight color
without lowering existing alpha. This prevents repeated source-edge scallops,
transparent holes, and accidental selected-color marks.

The CPU precomputes remaining charge as a deterministic exponential of traveled
stroke distance measured in brush diameters. This scalar is carried in the
existing fixed-size `Dab.material` field, so charge adds no texture lookup or
new upload. A spatial reservoir preserves brush-local variation for loaded oil
and palette-knife marks. It uses the bounded 64×64 ping-pong pass after a wet
microbatch of at most three contacts. Smudge uses short deterministic
three-contact backtrace chunks with distance-normalized strength; liquify keeps
immediate one-contact destination ordering.

## Layer-wide watercolor

Watercolor is a dedicated execution class within the same material shader. The
paint layer's premultiplied RGBA stores flattened pigment while a sparse R8
channel stores water/membership. It never reads the composite or another layer
and does not allocate a brush reservoir. Drying is explicit: merge the wet
layer down and continue on a fresh layer.

Each stroke uses uniform coverage, so overlapping contacts contribute only the
increment required to reach the requested opacity. One motion-directed
backtrace samples the same layer as newly covered pixels arrive, mixes straight
color in Oklab, preserves existing alpha, then deposits selected pigment. This
gives same-layer wet interaction without repeated-dab darkening, alpha holes,
or carried silhouettes.

The built-in watercolor tip has a pinhole-free but deliberately varied
low-frequency interior and a ragged perimeter. Deterministic random rotation
and small symmetric size jitter are common contact settings available to every
brush, not watercolor-only behavior.
The tip/texture contract remains general enough for a later contact compiler to
select masks or transforms from direction, direction change, speed, pressure,
tilt, or twist without changing the raster interface.

### Instant capillary transport

`BrushTransport` adds a brush-owned, document-anchored conductance texture plus
scale, contrast, maximum distance, water load, wet-to-wet flow, and wet-to-dry
flow. Four built-in seamless fields cover long/short and broad/narrow fibers.
Watercolor defaults favor wet-to-wet mixing; an ink preset can use the same
kernel with a larger dry-flow coefficient.

Each visible stroke update first deposits pigment and nonuniform wetness. One
logical exchange runs after internal deposition microbatches and consists of
three GPU-only coarse-to-fine stages. Each stage gathers water and
premultiplied pigment from wetter neighbors. Fresh deposition recharges prior
water from the stroke's absolute uniform coverage, so a new mark creates a
strong differential without accumulating individual dabs. The shader derives a
local fiber tangent from the scalar conductance gradient on fiber shoulders and
its curvature at ridge crests, then uses a shorter normal pair to reach nearby
fibers. The update's unioned expanded dab damage is the hard work/effect bound.
Wet watercolor destinations select `wet_flow`; empty destinations select
`dry_flow`. A weaker local pigment relaxation mixes colors that have already
reached similar wetness.

The R8 field stores a persistent two-level watercolor-material floor plus water
above that floor. Excess water trends toward the floor on input events, while
the floor remains until merge so edges and material identity do not disappear.
The two physical R8 surfaces are ping-pong storage for this one logical channel.
Only stage one synchronizes them; later stages overwrite the same scissor.
There is no provenance, direction channel, second material mask, background
step, document-owned paper texture, or Gaussian blur.

## Uniform coverage and edges

Uniform accumulation stores maximum requested coverage for the current stroke.
For a pixel with old coverage `c0` and new target `c1`, the source-over alpha is:

```text
increment = (c1 - c0) / (1 - c0)
```

This reaches `c1` without repeated overlaps darkening the stroke. Coverage
pages remember their owning stroke ID and clear lazily when another stroke first
touches them; there is no canvas-wide reset.

An after-stroke edge brush reuses those same pages. At pen-up, a full-screen
triangle is scissored independently to each touched page and reads a 3×3-page
coverage neighborhood. The edge strength is derived from the local coverage
gradient and applies the configured wet/burnt edge to the copied destination
color. Brushes that opt out never encode this pass.

Watercolor does not use that pen-up operation. Its normal layer-composition
shader samples pigment plus a sparse 3×3 wetness neighborhood and derives two
bounded binary morphology bands from the unioned mask. It displays a darker,
denser immediate rim, a slightly lighter inner band, and a faint outer bleed in
every frame without mutating stored paint. Outer boundaries and interior holes
follow the same rule; overlapping wet strokes create no internal edge, pigment
alpha changes do not move the edge, and lifting the pen causes no visual change.

Each R8 wetness surface costs 64 KiB per touched 256×256 page. The ping-pong pair
costs 128 KiB per page, or 32 MiB for a fully covered 4096×4096 layer. Against
the existing watercolor page set (two RGBA8 color surfaces, two R8 coverage
surfaces, and one R8 wetness surface), the second R8 surface is a 9.1% increment.
Predicted watercolor uses recyclable private pairs and cannot mutate committed
wetness.

Pigment remains flattened RGBA. Watercolor drawn directly over dry paint on the
same raster layer may sample that underlying color inside the new wet contact;
strictly separating the optical interaction of co-located materials would
require internal pigment planes and is outside this minimal model.

## Feature-specialized GPU work

The material shader has one semantic implementation and four prepared render
pipelines selected once per batch:

| Enabled output | Attachments written |
| --- | --- |
| Color only | RGBA8 color |
| Uniform coverage | RGBA8 color + R8 coverage |
| Wetness only | RGBA8 color + R8 wetness |
| Coverage + wetness | RGBA8 color + both R8 fields |

This is deliberately a small finite pipeline set. It keeps disabled-feature
bandwidth out of the fast path without generating a pipeline for every preset.
Color and coverage use source/destination ping-pong textures to obey portable
WebGPU read/write rules. Non-watercolor deposited wetness remains a write-only
MAX attachment. Watercolor wetness ping-pongs because transport must read the
pre-update value while writing all deposition from that submitted update.

## Painter presets and benchmark proof

Preset IDs 15–24 cover Textured Flat, Dry Scumble, Pastel Block, Transparent
Glaze, Opaque Gouache, Watercolor Wash, Wet Watercolor, Loaded Oil, Palette
Knife, and Natural Blender. Every sample uses four medium/large,
pressure-varying strokes in a shared coral/indigo/gold/teal palette over
localized swatches.

The release harness runs each brush on three fresh canvases, warms the exact
pipeline outside the timing window, and aggregates all frames rather than
choosing a favorable repetition. Coverage-only, wetness-only, wet-reservoir,
and smudge-advection brushes measure each primitive independently. Combined
brushes exercise attachment composition. The legacy optional after-stroke edge
kernel is reported in pen-up timing; watercolor's live edge is included in
ordinary move-frame composition.

- `artifacts/benchmarks/painter-brushes-4k.md`
- `artifacts/benchmarks/gpu-4k-paint-state-regression.md`
- `artifacts/benchmarks/complex-brush-interactions-4k.md`
- `artifacts/brush-validation/destination/contact-sheet.png`
- `artifacts/brush-validation/watercolor-v4-capillary-relaxation/contact-sheet.png`
- `artifacts/benchmarks/watercolor-relaxation-4k.md`
- `artifacts/brush-validation/watercolor-transport-v3-relaxation/contact-sheet.png`

These measurements demonstrate the required workload on the recorded GPU; they
do not prove a globally optimal implementation or replace input-to-present
traces on every target. The acceptance gate is move and pen-up completed-work
p99 below 8.33 ms, with every wall-clock maximum and over-budget count retained
for diagnosing host/GPU scheduling noise.

The functional galleries are separate from that timing composition. The
general two-phase gallery checks isolated continuity and destination
interaction. The watercolor gallery separately checks pressure, opacity, size,
glazing, same-layer two/three-pigment interaction, interior boundaries, dynamic
curves, and lower-layer isolation. The transport matrix separately varies
long/short and broad/narrow conductance, rate, and 16–88 px distance. See the
`artifacts/brush-validation/README.md`.

## Future physical paint boundary

A later physical prototype may add timed drying, pigment load, paper
absorption, height, or bristle state. It should preserve this ownership:

- add versioned sparse material channels behind `layer-render-wgpu`;
- sample them in feature-specific GPU passes, never on the CPU;
- give predicted tails private speculative material state before allowing them
  to evolve persistent physics;
- introduce damaged-region checkpoints once state changes with time and can no
  longer be reconstructed from immutable strokes alone;
- retain the current dry, uniform, bounded smudge, ordered liquify, and bounded wet
  reservoir paths for brushes that do not request the more expensive simulation.

No time-evolving relaxation or checkpoint system is added until artist tests
identify the smallest physical behavior that materially improves painting.
