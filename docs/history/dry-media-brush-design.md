# Pencil, charcoal and ink brush redesign

Research and design proposal, 2026-09-13. The audit below describes the engine
before this work. The implemented follow-up and measurements are recorded in
[Contact brush engine](../development/contact-brush-engine.md).

The proposed change preserves shared Rust input processing and the sparse wgpu
renderer while replacing the contact, deposition and ending behavior of these
media. A brush needs to describe three separate things: the tool touching the
page, the stationary surface it touches, and how pigment accumulates there.

## What the code currently does

| Finding | Evidence | Consequence |
| --- | --- | --- |
| Pencil uses the grain image as its tip and has no separate paper grain. | `DefaultBrushPreset::Pencil` in [presets.rs](../../crates/layer-core/src/presets.rs). | Paper detail translates and scales with every contact. |
| Chalk combines that moving tip with canvas grain and nonzero offset jitter. There is no dedicated charcoal preset in this catalog. | `DefaultBrushPreset::Chalk` in [presets.rs](../../crates/layer-core/src/presets.rs). | Neither the tip texture nor the jittered paper is a fixed substrate. |
| Canvas grain still adds a random offset seeded by contact center. | `grain_uv` in [brush_coverage.wgsl](../../crates/layer-render-wgpu/src/brush_coverage.wgsl); callers in [advanced_brush.wgsl](../../crates/layer-render-wgpu/src/advanced_brush.wgsl). | Selecting canvas coordinates alone does not ensure stationary grain. |
| Grain multiplies each dab's coverage; ordinary dry paint uses source-over. | [advanced_brush.wgsl](../../crates/layer-render-wgpu/src/advanced_brush.wgsl). | Repeated translucent deposits can fill grain gaps even after fixing coordinates. |
| The default pressure-size mapping retains 15% diameter at zero pressure. G-Pen uses this mapping and has no explicit taper override. | `BrushMapping::pressure_size` in [layer-core](../../crates/layer-core/src/lib.rs), G-Pen preset. | An 18 px G-Pen retains a 2.7 px diameter before other limits, making a blunt ending plausible. |
| Taper has lengths and endpoint size/opacity, but its envelope is a fixed smoothstep. End taper requests a rebuild after pen-up. | `taper_factors` in [brush.rs](../../crates/layer-engine/src/brush.rs), `PenPhase::Up` in [canvas.rs](../../crates/layer-engine/src/canvas.rs). | No separate curve/tip-shape choice; changing the final taper cannot be handled solely by adding another dab. |
| Tilt can rotate and reshape contacts through existing dynamics, but there is no directional contact-density field. | `Registers` and `evaluate` in [brush.rs](../../crates/layer-engine/src/brush.rs), [Dab](../../crates/layer-render/src/lib.rs). | A rotated ellipse alone cannot produce a dark tip and softer opposite side. |
| The model contains substantially more settings than the shared controls expose. | [BrushSnapshot](../../crates/layer-core/src/lib.rs), [tool_settings.rs](../../crates/layer-ui/src/tool_settings.rs). | UI exposure and renderer capability must be tracked separately. A stored setting is not evidence of an editable or satisfactory brush. |

These findings explain plausible causes of the reported appearance. They are a
source audit, not a visual validation of replacement brushes.

## Reference products

Relevant Procreate controls include stationary/moving grain, tilt gradation,
texture size compression and taper tip character. Texture-offset jitter varies
between strokes and applies to moving grain.
[Brush Studio settings](https://help.procreate.com/procreate/handbook/brushes/brush-studio-settings).

To explore a brush in Procreate, open an existing brush in Brush Studio, select a
property group, and test marks in its Drawing Pad. Changing settings updates the
test marks. Numerical fields also open supported pressure, tilt and roll controls.
This interaction suggests a settings editor with replayable samples and direct
access to each parameter's dynamics. [Brush Studio](https://help.procreate.com/procreate/handbook/brushes/brush-studio).

Darkly's [brush catalog](https://darkly.art/docs/reference/brushes/brushes/)
describes Hair as individual strands. At source revision
`8249402c177044b499391035dd5687ee106e698b`, its
[Hair preset](https://github.com/darkly-art/darkly/blob/8249402c177044b499391035dd5687ee106e698b/crates/darkly/brushes/hair.yaml)
combines dab-space noise with a circular mask, changes a threshold with pressure,
and rotates the noise using traveled distance and a twirl factor. That graph is
procedural and has no simulated bristle positions or velocities.

The associated
[noise implementation](https://github.com/darkly-art/darkly/blob/8249402c177044b499391035dd5687ee106e698b/crates/darkly/src/brush/nodes/noise.rs)
can cache a static noise field as a texture while evaluating coordinate transforms
at sample time. Wiring field-generation parameters selects live shader
evaluation. A changing brush appearance therefore need not require generating
and uploading a new bitmap for every contact.

Darkly's
[Rough Ink preset](https://github.com/darkly-art/darkly/blob/8249402c177044b499391035dd5687ee106e698b/crates/darkly/brushes/rough_ink.yaml)
varies a procedural silhouette's amplitude, rotation and seed per dab. That is a
useful reference effect; our proposed roughness should additionally remain stable
when contact spacing changes.

## Proposed pencil and charcoal contact

Give paper its own immutable surface description: texture or procedural field,
document-space scale, rotation and origin. Freeze that description in stroke
history. The default surface should be shared by pencil and charcoal so that both
encounter the same tooth. Brush-specific surface overrides can remain available
for intentionally stylized brushes. Changing a surface default must not silently
change existing marks on replay.

Paper coordinates must be independent of contact center, brush size, pen angle,
stroke direction and stroke seed. A canvas zoom or rotation changes the view,
not the surface. Offset variation belongs to a separate brush texture mode.
Generate any static procedural surface once, with appropriate filtering, and
sample it during drawing. A photographed luminance image is an artistic height
proxy unless an actual height map is supplied.

Resolve a separate contact footprint from pressure and orientation. Upright
pencil uses a compact tip; increasing tilt widens the side contact. An asymmetric
density profile makes the tip side darker and softens the opposite side. The
azimuth determines that profile's direction, independently of stroke travel.
Roll can rotate a chisel or worn tip where the device supplies it.

Use a pressure-dependent contact threshold against the paper field: light
contact marks the high points; stronger contact reaches more of the surface.
Apply the directional density profile to the local pressure before evaluating
that threshold. This gives pressure a spatial effect beyond multiplying opacity.
Tip sharpness, side-contact width and softness distinguish pencil from charcoal;
grade, deposit strength and texture contrast distinguish their tonal response.

Handle orientation consistently across hosts. In particular,
`to_stroke_point` currently transforms position while copying tilt and twist
unchanged. Audit and define the complete sensor coordinate contract before adding
directional shading. Convert the stylus direction into document orientation once,
including view rotation/reflection; do not apply translation or zoom magnitude to
an angle. Near upright, where azimuth becomes unstable, fade out directional
shading or retain the last valid direction. Missing sensors need a deliberate
upright/default-pose fallback.

### Deposition and accumulation

Fixing coordinates is necessary but insufficient. For a fixed grain value `g`
and a per-dab strength `f`, repeatedly painting the same pixel yields:

```text
alpha after N overlaps = 1 - (1 - f * g)^N
```

Even a small positive grain value tends toward opacity as overlap increases.
This can destroy the paper contrast without moving a single paper texel.

Provide two explicit behaviors:

- **Uniform stroke coverage:** merge overlapping contact coverage before applying
  stroke opacity. Useful for clean ink and deliberately even shading. A new
  stroke can add another coat. It does not reproduce repeated rubbing within
  one uninterrupted pencil stroke.
- **Material buildup:** integrate a bounded deposit rate over traveled distance,
  using the contact/pressure threshold so untouched valleys remain unmarked.
  Revisiting an area can darken it. A later material-capacity field can model
  saturation and filling of the tooth if simpler deposition is insufficient.

For buildup, a candidate integration is `alpha = 1 - exp(-rate * exposure)`.
Exposure must account for the swept contact and distance represented by a step;
it cannot simply be the number of dabs. Time-based dwell should be a separate,
intentional behavior for pens that pool or continuously feed ink.

Start with stationary paper, the contact threshold and normalized buildup.
Compare repeated shading passes before adding persistent material-capacity
storage. The existing uniform coverage mechanism is useful infrastructure, but
turning it on for every dry medium would remove expected rubbing behavior.

## Pointed G-Pen and rough ink

Separate the pressure floor, taper length, endpoint width and taper curve.
Keep opacity taper independently adjustable: an opaque hairline and a fading
line are different results. Support pressure-driven endings and optional
assisted endings for fast lifts or input without pressure. A tap should still
produce a dot, and cancellation should not add a finishing point.

Use a stable length reference for assisted taper, such as nominal brush size or
document pixels. The current extent changes with the evaluated diameter, which
can shrink the taper precisely as pressure falls. Capture the actual terminal
sample even when it does not land on the regular contact interval.

Prototype continuous variable-width segments for clean pens. Each segment needs
both endpoint poses; the current `Dab.motion` alone does not contain the previous
radius, orientation or density. Adapt subdivision to curvature and pose change,
including joins and subpixel endpoints. Retain the existing stamp path for
brushes that need discrete marks. Both remain inside the same renderer.

An assisted end taper needs a replaceable trailing region, since the final
stroke length is unknown during drawing. Reuse the stroke-feedback architecture
to keep a bounded tail provisional, finalize it at pen-up and publish only
affected damage. The tail must composite against the correct stroke base and
coverage, including where it crosses the committed prefix. Painting smaller
contacts on top cannot remove the existing rounded end. Avoid a full document
rebuild for an ordinary pen lift.

For Rough G-Pen, expose paper influence separately from tool-edge irregularity.
Use stationary paper to produce repeatable bites at a given document position,
and smoothly varying tool noise indexed by stroke distance for nib variation.
Both can feed one bounded shader edge operation.

Apply roughness to the continuous stroke boundary or its merged coverage, with
antialiasing and an amplitude limit that protects tiny tapers. Distorting every
circular dab independently can leave scallops, and later dabs can fill earlier
edge defects. A separate pass, if required, should process the stroke's coverage
before compositing. Filtering finished layer pixels would also affect unrelated
marks and create unwanted overlap behavior.

A built-in edge operation can expose useful artist controls without requiring a
general shader editor. Any later programmable brush stage needs declared input
coordinates, bounded output extent, deterministic seeds and persistent program
identity. It must operate within the existing selection, preview and history
contracts.

## Ink brushes: procedural first, state when needed

There are three useful implementation levels:

| Model | Behavior to target | State and cost |
| --- | --- | --- |
| Procedural contact | Strand gaps, pressure-dependent splitting, directional streaks and twirl. | Static texture/noise plus resolved pose, distance and ink amount; evaluate during rasterization. |
| Small bristle-bundle model | Bristles lag on turns, spread under load, retain splits and recover when lifted. | Persistent positions or deflections, velocities and ink loads for a bounded number of bundles. |
| Canvas transport | Ink spreads into paper or exchanges with other wet pigment after deposition. | Separate optional material passes and sparse canvas state. |

The first level is enough to prototype the visual character suggested by Darkly.
Use coherent variation across distance and stable strand identity; independent
randomness at each contact is likely to look noisy. Ink load can decline with
deposited amount or travel, producing increasingly broken strands. Expose refill
at pen-down and optional continuous feed as distinct behaviors.

Add the second level only if artist review identifies missing mechanical
response. A starting experiment could use 32–128 representative bundles with
damped deflection, pressure-driven spread and optional attraction into clumps.
Those counts are experiment parameters, not measured performance claims.
Sweep the resulting contacts between steps so fast motion does not leave gaps.

Small sequential brush-state updates can be cheap on the CPU. The existing Rust
engine already performs dynamics there. Sending a few dozen bundle parameters
to the GPU is very different from rasterizing a large dab image on the CPU.
Benchmark a CPU bundle update against GPU compute before choosing ownership.

For larger state, GPU compute can update bundles and write a contact buffer or
small footprint texture consumed by rasterization. Parallelize across bundles
and pixels while preserving the order of simulation steps. Use stable substeps
derived from recorded input time/distance, never display-frame count. Do not
assume a separate dispatch for every dab is free; batch steps where dependencies
allow it, without reordering them or reusing one final footprint for all steps.

Regenerating a tiny bitmap on the CPU is also not automatically too slow. For
scale, a 64×64 RGBA8 texture uploaded 120 times per second is about 1.88 MiB/s;
256×256 is 30 MiB/s, before upload overhead or generation cost. Multiple contacts
per frame multiply those figures. Procedural fragment evaluation avoids the
bitmap upload, but expensive noise evaluated across overlapping large contacts
can cost more than sampling a cached texture. Measure both computation and
memory traffic.

Research likewise treats shape generation and stroke deposition as separate
choices, including simulation or recorded deformation and stamping or sweeping.
[A Brush Stroke Synthesis Toolbox](https://research.adobe.com/publication/a-brush-stroke-synthesis-toolbox/).
This supports a staged design; it does not establish performance on our devices.

Canvas flow is optional and independent of brush mechanics. Brush-state modeling
can produce expressive inking without a fluid solver. It will not, by itself,
produce persistent puddles, capillary spreading or pigment mixing outside the
contact. Our current watercolor implementation already uses bounded transport,
not a full fluid solver, so compare costs against that actual implementation.

## Controls and shared UI work

The following are proposed Capy Canvas controls, grounded in the requested
behaviors and the existing model. They are not claims of Procreate parity.

| Area | Artist-facing controls | Work in this repository |
| --- | --- | --- |
| Contact | Tip size, aspect, sharpness, angle; side-contact width and directional fade. | Reuse shape dynamics; add the asymmetric contact model. |
| Paper | Surface source, feature size, tooth strength, tonal range and pressure sensitivity. | Separate surface identity from the tip; remove contact-dependent phase in the new paper mode. |
| Ending | Start/end length, endpoint width, curve, opacity fade and assistance. | Extend taper semantics and replace the ending locally. |
| Deposition | Opacity, deposit rate, even coverage or buildup. | Preserve their distinct meanings; implement spacing-stable material deposition. |
| Rough edge | Irregularity amount, feature size, paper contribution and variation along the stroke. | Add a bounded operation on continuous coverage. |
| Brush mechanics | Stiffness, spread, clumping, strand width and recovery. | Add only with the bundle model, so every visible setting has an effect. |
| Ink supply | Initial load, depletion, refill and feed; optional pickup or bleed. | Separate pigment supply from geometry and from canvas transport. |
| Input | Per-property response curves, pressure smoothing, tilt transition and orientation source. | Extend existing sensor mappings and expose them in shared settings. |

Keep fast adjustments in the tool settings panel and detailed contact/material
editing in a brush editor. Show the supported input sources beside a setting;
display an explicit fallback when a stylus lacks tilt or roll. Expose advanced
randomness by lifetime: fixed surface, per stroke, or continuous along the path.

The existing model already contains spacing/scatter, stabilization, bounds,
color dynamics, dual tips and wet-mix parameters. Preserve those capabilities
and expose them progressively. A useful editor should replay saved test gestures
through the production renderer, including pressure ramps, two opposite tilt
directions, a tight turn and a crossed stroke. Allow drawing fresh samples,
numeric edits, resetting changes and saving a variation. Avoid a preview that
only looks correct for a single canned pressure curve.

## Delivery and validation

1. Establish paired visual fixtures for the current and proposed media: fine
   lines, broad tilted shading, repeated rubbing, pressure release and tight
   turns. Include a high-contrast paper pattern to expose coordinate drift.
2. Implement the new stationary surface/contact behavior and a charcoal preset.
   Validate grain persistence and buildup before adjusting cosmetic noise.
3. Implement configurable endings and bounded tail replacement. Prototype swept
   pen coverage and compare it with dense dabs at equivalent visual quality.
4. Add Rough G-Pen using the same coverage and taper behavior.
5. Prototype procedural ink strands. Add bundle simulation only when it improves
   the reviewed strokes enough to justify its cost.
6. Expose the working controls through the shared schema and host editors.

Version the new brush semantics. `BrushSnapshot` currently validates schema
version 4 exactly; extending it requires explicit serialization and migration
work. Preserve prior appearance through legacy evaluation or stored raster
results, and distinguish upgrading a preset from changing saved artwork.

Required checks include equivalent event sequences partitioned into different
display frames; comparable strokes at different contact spacings; rotated and
mirrored views; sparse-page boundaries; upright and tilted contacts; missing
sensors; dots and rapid lifts; self-crossings; selections and alpha lock;
cancellation; prediction replacement; undo/redo; save/reopen; and device rebuild.
Predicted state must never advance committed pigment, random state, ink load or
bristle state. GPU arithmetic is not assumed bit-identical across devices;
define visual tolerances and retain raster checkpoints where exact appearance
must survive a renderer change.

Measure CPU input-to-submit time, completed GPU work, actual presentation
latency and sustained mobile cost separately. Include tiny pens and broad
shading brushes, with caching warm and cold. Use the existing
[testing guide](../development/testing.md) and
[GPU workloads](../development/gpu-raster-benchmarks.md). There are no new
performance results in this proposal.
