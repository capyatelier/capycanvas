# Advanced brush engine

## Product surface

The target Layer engine supports professional 2D raster brushes through one immutable
`BrushSnapshot`. The settings use familiar painting-app concepts, but the
runtime compiles them into a small finite set of GPU execution classes. A preset
never supplies arbitrary shader source.

Implementation status: the expanded schema and GPU paths for solid, single-mask,
grain, dual-tip, alpha-threshold, per-contact edge coverage, stroke-uniform
accumulation, and post-stroke edges are present. Sparse dry/prediction storage,
source/destination page companions, underlying-pixel pickup, blur, pull, wet
deposition, Oklab mixing, blend modes, a persistent spatial brush reservoir,
layer-wide watercolor with live edges, a sparse non-watercolor canvas-wetness
field, and push/twirl/pinch/expand/crystal/edge liquify are implemented.
Reconstruct snapshots, bristle simulation, time-based drying, fluid transport,
and pigment physics remain future extensions. The 15 legacy plus 10
painter-brush benchmarks and labeled outputs are complete.

The feature surface is based on the current
[Procreate Brush Studio](https://help.procreate.com/procreate/handbook/brushes/brush-studio-settings),
[Procreate Dual Brush](https://help.procreate.com/procreate/handbook/brushes/dual-brush),
and Clip Studio Paint's official
[brush customization](https://help.clip-studio.com/en-us/manual_en/240_brushes/Customizing_brush_tools.htm)
and [blending](https://help.clip-studio.com/en-us/manual_en/240_brushes/Blending_tools.htm)
documentation. Names below are Layer's stable concepts, not file-format
compatibility promises.

| Layer concept | Familiar controls covered | Owner |
| --- | --- | --- |
| Path | spacing, spacing jitter, lateral/linear jitter, falloff, continuous spray | CPU contact compiler |
| Stabilization | streamline, pressure smoothing, moving-average stabilization, motion filtering/expression | CPU input filter |
| Taper | independent start/end size and opacity envelopes | CPU contact compiler |
| Shape | analytic or image tip, aspect, angle, direction/azimuth/barrel rotation, size/rotation jitter, random flips, count/count jitter | CPU placement + GPU coverage |
| Grain | moving or canvas-anchored texture, scale, depth, rotation, offset jitter | GPU coverage |
| Dynamics | pressure, speed, direction, tilt, twist, distance, time, and deterministic random mappings | CPU register program |
| Color dynamics | per-stamp/per-stroke hue, saturation, lightness, secondary-color mix | CPU resolved color |
| Rendering | source-over glaze, uniform accumulation, blend mode, alpha threshold, wet/dark edge | GPU raster |
| Dual tip | independently transformed secondary shape/grain and coverage combine mode | GPU coverage |
| Smudge/blend | pickup, pull/color stretch, blur, deposit/amount, opacity/density | destination GPU pass |
| Wet mix | spatial reservoir, charge/depletion, dilution, attack, pull, blur, wetness variation; layer-wide watercolor interaction | destination GPU pass + optional GPU reservoir |
| Deform | push, twirl, pinch, expand, crystals, edge, reconstruct | destination GPU pass |
| Bounds | minimum/maximum size and opacity plus preview metadata | shared UI/preset model |

3D material channels and lighting are not 2D brush behavior. They belong to a
future 3D document renderer rather than this raster canvas engine.

## Common contact representation

Input samples are stabilized and mapped before raster work. The contact
compiler emits ordered fixed-size `Dab` records containing:

- center, elliptical radii, rotation, and motion since the previous contact;
- resolved linear color, flow, hardness, and deterministic variation;
- conservative affected bounds.

All brush families consume these steps. A dry pen uses only geometry, color,
flow, and hardness. Smudge and wet paint additionally use motion and material
parameters. Liquify uses the same geometry, motion, coverage, ordering, and
damage calculation but changes the destination operation.

The document stores source samples and an immutable brush snapshot, not compiled
GPU records. Replay recompiles the same deterministic contacts.

## GPU execution classes

There are four engine-owned classes, selected once per batch:

1. **Solid dry** — analytic coverage and fixed-function paint/erase blending.
   This preserves the minimal G-Pen fast path.
2. **Textured dry** — one coverage shader with explicit feature flags for shape,
   grain, and dual-tip sampling. Disabled sources perform no texture sample.
3. **Material transfer** — a destination-aware GPU raster pass for blend,
   smudge, blur, and wet paint using immutable source and distinct destination
   views.
4. **Deformation** — a destination-aware GPU raster pass that evaluates inverse
   displacement for liquify-style modes.

Pipeline keys contain only execution class, paint/erase operation, coverage
source, and required target format. User settings remain data. This avoids a
shader permutation per preset while keeping destination reads and unused texture
samples out of ordinary ink.

The destination operation uses a fragment pass rather than compute because the
work product is already a rectangular color attachment and raster scissors map
directly to damage. Within that pass, one invocation owns one destination pixel
and evaluates the ordered contacts in its batch. Wet paint uses microbatches of
at most three contacts. Smudge reverse-composes deterministic chunks of at most
three contacts, also bounded by travel and damage; liquify uses one contact per
ping-pong step. Source/destination layer textures are ping-ponged; the renderer
synchronizes only regions changed since the inactive texture was last current.

## Material transfer model

Layer uses artist-facing controls rather than exposing simulation constants:

```text
coverage  = shape * grain * pressure-resolved flow
pickup    = sample(source, position - motion * pull * coverage)
carried   = spatial_reservoir(contact_local_position)
source    = perceptual_mix(pickup, carried, dilution)
deposit   = attack * remaining_charge * coverage
result    = transfer(destination, source, deposit, density, paint_amount)
```

A double-buffered 64×64 RGBA8 reservoir stores carried color and paint amount
for wet paint only. It is initialized from loaded color once at stroke start
and exchanges paint with the immutable active layer for every contact in a
bounded wet microbatch. Spatial mode preserves brush-local color for loaded oil
and palette knives. The contact compiler applies deterministic charge depletion
by traveled distance. Selected color is not reinjected after initialization.

Smudge/blender instead reverse-composes contact motion into one
semi-Lagrangian source coordinate per short deterministic chunk and samples the
immutable canvas once. Smudge strength integrates physical travel rather than
raw contact count. An incomplete live chunk uses the same private preview path
as predicted input and commits only at a deterministic boundary. Transport
preserves existing alpha, so pulling from an empty area cannot cut holes into
opaque paint. This separate semantic path still shares contact coverage, sparse
pages, damage, ping-pong storage, undo, and scheduling.

Transparent premultiplied canvas pixels have no straight color. Pickup from
them therefore preserves loaded/carried pigment instead of interpreting empty
canvas as black. Lower-alpha canvas can replenish color but cannot drain brush
material; charge depletion already owns that loss.

Standard mixing uses linear premultiplied RGB. Perceptual mixing uses Oklab,
matching the user expectation behind Clip Studio's Standard/Perceptual choice
without exposing pigment-model coefficients. A later true pigment mode may use
measured Kubelka–Munk data, but it is not silently approximated by RGB controls.

Stroke-uniform accumulation stores maximum requested coverage for the current
stroke in sparse double-buffered R8 pages. Each contact contributes only the
alpha increment needed to reach that coverage, so overlaps within one stroke do
not darken like flow accumulation. Coverage is keyed by stroke ID and cleared
lazily on first page use by the next stroke.

Watercolor uses that coverage and a separate logical sparse R8 wetness channel,
but no reservoir or drying clock. Marked regions remain wet until an explicit
merge/replay workflow dries them. Newly covered pixels perform one
motion-directed backtrace into immutable same-layer RGBA, mix color in Oklab,
preserve alpha, and then deposit pigment. Lower layers and the composite are
never sampled. A pinhole-free, low-frequency varied, ragged-perimeter tip plus
common random rotation and size jitter controls pigment and water load.

`BrushTransport` optionally adds a document-anchored tiled conductance texture,
scale/contrast, maximum 0–96 px radius, water load, and separate wet/dry flow
rates. Three coarse-to-fine GPU stages run after each submitted update, not
after every internal microbatch. They advance the R8 water field and
premultiplied pigment together along a tangent derived from the scalar field's
gradient and curvature. A watercolor preset normally favors wet-to-wet flow;
an ink preset can favor wet-to-dry bleed without adding another engine.

For non-watercolor brushes, a bounded optional post-stroke edge pass reads that
coverage with a 3×3-page neighborhood and darkens the wet boundary at pen-up.
It is never encoded for brushes that opt out. Their separate sparse R8 wetness
field records maximum deposited wetness but does not yet diffuse, dry, or drive
later interaction.

Watercolor instead applies edge behavior in the normal live layer-composition
pass. Two small binary morphology radii over the unioned wetness mask produce a
denser/darker immediate rim, a slightly lighter inner band, and a faint outer
bleed at both external and hole boundaries. Overlap-alpha and pigment-color
transitions stay inside the mask and therefore do not become edges. This
display transform does not mutate stored paint, and pen-up does not change the
result. Merging the layer down and creating a new watercolor layer is the
explicit drying workflow.

The 2026
[Dripping Thin Films](https://research.adobe.com/publication/dripping-thin-films-for-real-time-digital-painting/)
work is promising for an optional watercolor relaxation stage because it offers
artist-facing drip length, thickness, and frequency controls. It does not
replace the interactive contact-transfer path and is not enabled for ordinary
wet brushes.

## Deformation model

Liquify is a brush operation over the same coverage kernel. For every damaged
destination pixel, the shader walks contacts newest-to-oldest and computes a
source coordinate using Push, Twirl, Pinch, Expand, Crystals, or Edge. It then
bilinearly samples the immutable source texture once at the composed coordinate,
including across sparse-page boundaries.

This is the inverse-mapping form of Procreate's documented
[Liquify modes](https://help.procreate.com/procreate/handbook/adjustments/adjustments-liquify).
Reconstruct blends toward a stroke-start snapshot using the same coverage.
Deformation therefore shares input, dynamics, shape, damage, ping-pong storage,
undo, and scheduling with brushes rather than becoming a second canvas engine.

## Stroke finalization

Start taper is known during live input. End taper requires the final stroke
length. The live preview uses pressure and the known start envelope; the current
pen-up implementation performs a deterministic document replay with the exact
two-sided envelope. Damage-only stroke replacement is a later latency
improvement, not current behavior.

Predicted samples remain replaceable GPU-only state for every execution class.
The common single-batch destination path samples committed pages directly into
private preview damage. Exact erase and uncommon multi-batch prediction copy
only the required damaged GPU region. Prediction never advances committed
state.

## Memory model

Dense per-layer 4K textures do not scale: one `RGBA8Unorm` image is 64 MiB.
Paint and optional material channels therefore use private 256×256 GPU pages
allocated only for touched regions. Empty layers consume metadata only. The
current implementation uses standalone page textures; an atlas is not required
unless target traces show binding or allocation overhead.

The document-sized composite is one unavoidable 64 MiB cache at 4K. Predicted
state and source/destination companions are allocated lazily and only for
affected pages. Uniform coverage uses two R8 pages per touched page; deposited
wetness uses one R8 page for non-watercolor wet brushes. Watercolor uses two
physical R8 surfaces for one logical wetness channel so an update can compare
old and newly deposited water. The pair is 128 KiB per touched page, or 32 MiB
for full 4K coverage; the second surface adds 9.1% to the existing watercolor
page set. The double-buffered spatial reservoir is a fixed 32 KiB and is not
allocated per layer. Normal sparse use allocates only touched regions. Page
coordinates, storage slots, and halos never enter
`layer-core`, native bindings, or project semantics.

Renderer metrics report allocated byte totals by purpose. Benchmarks must cover
32-layer and 128-layer documents with sparse marks so layer count cannot hide a
linear full-canvas allocation.

## Performance acceptance matrix

Every benchmark reports submit and completed-work p50/p95/p99, allocated GPU
bytes, touched pages, contacts, and affected pixels. The common suite uses a
4096×4096 canvas with 32 visible layers; exotic operations use the same canvas
and a representative painted destination.

| Family | Representative settings | Required completed p99 |
| --- | --- | ---: |
| Solid ink/eraser | 4–512 px, pressure size/flow | < 8.33 ms |
| Shape + grain | 16–512 px, moving and anchored grain | < 8.33 ms |
| Scatter/spray | 4–128 px particles in a 64–1024 px envelope | < 8.33 ms |
| Dual textured | 16–512 px, two masks + grain | < 8.33 ms |
| Glaze/blend modes | 16–512 px | < 8.33 ms |
| Smudge/blend | 32–512 px, pickup and blur | < 8.33 ms |
| Wet paint | 32–700 px, reservoir + pickup + perceptual mix | < 8.33 ms |
| Layer-wide watercolor | 32–700 px, coverage + same-layer advection + R8 wetness live edge | < 8.33 ms |
| Stroke-uniform paint | 32–700 px, sparse R8 coverage | < 8.33 ms |
| Post-stroke edge | 32–700 px, coverage morphology at pen-up | < 8.33 ms |
| Liquify | 64–1024 px push/twirl/pinch | < 8.33 ms |

Brushes larger than the reasonable matrix remain bounded and responsive, but a
full-canvas material/deformation operation is reported as a stress case rather
than made to pass by changing its visual semantics.

The complex interaction scenarios each run three fresh-canvas repetitions at
4K and contribute all frames to their distribution. A separate controlled
gallery crosses isolated color wells or deforms a fine grid, and includes a
labeled contact sheet for smudge, wet mixing, blender, push, and twirl review.

## Research basis

- [Ciallo GPU brush strokes, SIGGRAPH 2024](https://research.adobe.com/publication/ciallo-gpu-accelerated-rendering-of-vector-brush-strokes/)
- [Efficient Rendering of Linear Brush Strokes](https://jcgt.org/published/0007/01/01/)
- [Wetbrush GPU bristle simulation](https://wanghmin.github.io/publication/chen-2015-wgb/)
- [Industrial-Strength Virtual Bristle Brush](https://research.adobe.com/publication/industrial-strength-painting-with-a-virtual-bristle-brush/)
- [Real-Time Oil Painting on Mobile Hardware](https://diglib.eg.org/items/d37a6d8c-1ea9-47f0-a39c-49bddbd67e5f)
- [RealPigment compositing](https://research.adobe.com/publication/realpigment-paint-compositing-by-example/)
