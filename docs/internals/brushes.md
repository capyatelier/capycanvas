# Brushes

[Technical documentation](../README.md) · [Architecture](../architecture.md)

A brush has two jobs: decide where and how to place marks along a stroke, then
evaluate those marks against the canvas. Capy Canvas keeps placement in the shared
CPU engine and pixel work in the GPU renderer. Every platform uses both parts.

The pencil, charcoal and ink presets now share a swept GPU contact model with
stationary paper, pressure thresholds, directional shading and coherent ink
coverage. See [Contact brush engine](contact-brush-engine.md) for
the implementation, preset catalog, review samples and measured costs.

## Brush definitions and strokes

[`BrushSnapshot`](../../crates/layer-core/src/lib.rs) describes the brush settings
captured for a stroke. It includes the tip, spacing, sensor mappings, color and
rendering behavior. Built-in definitions live in
[`presets.rs`](../../crates/layer-core/src/presets.rs); shared tool controls expose
the editable settings.

*Dynamics* map a changing input to a brush property. Pressure might change size or
opacity; tilt can change the shape or rotation of a mark. Input availability varies
by device, but the mapping rules are shared. A stroke stores the settings it used,
so editing a brush later does not change old strokes during replay.

## From samples to dabs

[`DabGenerator`](../../crates/layer-engine/src/brush.rs) processes the ordered path
and places contacts by distance traveled. A *dab* is one resolved brush contact:
its location, shape, color and other parameters are ready for the renderer.

Distance-based placement avoids making the brush density depend directly on how
many events a platform sends. Spacing, stabilization, taper and deterministic
variation shape the resulting sequence. The random seed is tied to the brush and
stroke so replay can reproduce its variation.

Dabs and shared style data are collected into batches. The renderer receives those
batches through the [render contract](../../crates/layer-render/src/lib.rs), rather
than receiving a separate platform callback for each mark.

## GPU execution

Ordinary ink and erase contacts use coverage and GPU blending. Textured brushes
also sample tip and grain resources. These paths can avoid the extra state needed
by brushes that interact with existing paint.

A destination-aware brush reads what is already on the layer. Smudge, wet mixing,
watercolor and deformation therefore require ordered operations and additional
GPU resources. Depending on the brush, those resources track coverage, wetness or
paint carried by the brush. Reading and writing the same image requires controlled
staging; it cannot be treated as ordinary independent source-over blending.

Brushes that mix paint (Smudge, the blenders, and the wet oil, gouache and
watercolor brushes) show a **Color mixing** choice in Tool Options, from shared
Rust on every host:

- **Oklab mixing**, the default of every built-in mixing brush, blends evenly as
  the eye sees color.
- **Linear light mixing** blends as light does.
- **Classic mixing** blends the document's encoded values, as Clip Studio Paint
  does.

Each preset keeps its own choice with its other edited settings. The choice does
not follow the document's blending, and it changes no pipeline; see
[rendering](rendering.md#native-sdr-working-color).

Watercolor uses its own pigment and wetness behavior. It is not a general physical
fluid simulation, and not every brush uses the same state or passes. The
[painterly paint-state reference](../reference/painterly-paint-state.md) explains
those distinctions in detail.

## Feedback and replay

The engine can draw temporary predicted contacts to reduce the apparent gap
between the pen and the stroke. Prediction uses replaceable preview state, leaving
committed paint, material state and undo history unchanged. Real input replaces
that preview. Predictions are not saved into the document.

Replay uses the stored real samples and brush snapshot. Any brush change should
therefore be tested both while drawing and after undo/redo or reopening a project.

## Retouching sources

A retouching stroke copies pixels instead of laying down a color. The engine
fixes what it copies at pen-down, in the stroke's
[`Retouch`](../../crates/layer-core/src/retouch.rs): the reference layers below
the editing layer with the editing layer over them, or the editing layer alone.
With no reference below, the editing layer alone is the source. A stroke that
would copy nothing from an empty layer, that would paint a mask, or whose layer
is scaled or rotated, is refused with a notice.

The renderer keeps the source apart from the pages the stroke paints
([`retouch_sources.rs`](../../crates/layer-render-wgpu/src/retouch_sources.rs)):

- **Stroke-start pages.** Before a stroke first writes a page of its layer, the
  page is copied GPU to GPU into a pooled page, or recorded as empty. Reading the
  source therefore never sees the stroke's own dabs, and the next stroke starts
  over and sees the last. A replay starts over from the restored pages.
- **Reference composite.** Pages of the composite of the reference layers,
  captured by their own scene and cached, 96 at most, least recently used out
  first. They stay valid until a member's pixels, placement or appearance change;
  painting the editing layer keeps them. A lone untransformed layer is copied
  rather than composed.
- **Sampling.** Each pixel reads the editing layer at its opacity over the
  reference composite, with bilinear taps, so whole-pixel shifts copy exactly.

While a retouching tool is selected, 16 stroke-start pages and the source
pipelines are ready before pen-down, and reference pages around the focus points
are captured a few per still frame. While the pen is down the sources never
upload or wait for the GPU: a reference page that would need filter images, a
decode or a wait stays empty, and the stroke is reported as a miss. The engine
replays it once contact ends, as it replays an end taper, and the replay stays
one undo step.

## Clone Stamp

The Clone Stamp (`BrushExecution::Clone`) lays the source down with the brush's
tip, opacity and flow. Each document's engine keeps its
[`CloneSource`](../../crates/layer-core/src/retouch.rs): the source point, the
Aligned flag, the flips and, once an aligned stroke has started, its offset.

- **Mapping.** A stroke copies document point `p` from `flip · p + offset`. It
  takes the offset at pen-down: the kept offset when Aligned has one, otherwise
  the source point minus the stroke's first point. Aligned keeps that offset for
  later strokes and moves the source point to where the stroke left off; Reset
  Offset and setting the source start again at the source point. The stroke
  records the mapping in the editing layer's pixels, and a corrected first point
  maps it again, so replays match a direct stroke.
- **Pass.** Clone runs on the fragment path, one page at a time: it gathers the
  source for the page's dirty rectangle into the first material sample field,
  bound in the reservoir slot, then deposits. The deposit is the dry loop with
  the gathered straight color, its coverage scaling each dab; with stroke-uniform
  accumulation the dabs compose to exactly the source's coverage times the
  stroke's. Selection clipping and alpha lock apply as for any dry brush, and
  tiles, dab ranges and damage stay per page.
- **Source disc.** The session draws the disc from cursor segments and sets a new
  document's source in the middle of the view when the tool is first selected.
  Set Source (held Alt, a bound side button, or its button for one contact) makes
  the next pen or mouse contact set the source; a finger never does. The disc
  drags at once with every device, and a tap shows the
  [clone source bar](../ui/canvas-action-bar.md#contexts).

Only translation is supported: the source is not rotated or scaled.

## Healing Brush and Spot Healing Brush

Both heal when the pen lifts, in the submission of the stroke's last batch
([`heal.rs`](../../crates/layer-render-wgpu/src/heal.rs),
[`heal.wgsl`](../../crates/layer-render-wgpu/src/heal.wgsl)). The stroke's raster
is already pending then, so its capture, its one undo step, replays and late
corrections all include the healed pixels.

- **Healing Brush** (`BrushExecution::Heal`) paints the Clone Stamp's copy while
  the pen is down, with the same source, disc and options. At pen-up each page
  the stroke painted becomes `S + h` over the page as the stroke found it: `S` is
  the copy, and `h` is a membrane that matches `D = B − S` where the stroke
  leaves the image uncovered, `B` being the source composite at the destination.
  The copy keeps its texture and takes on the color and brightness around the
  stroke. Where `D` is zero, `h` is zero and the result is exactly the clone.
- **Spot Healing Brush** (`BrushExecution::SpotHeal`) needs no source point.
  While the pen is down it lays a translucent grey tint. At pen-up it scores 16
  candidate sources, 8 directions at 1.25 and 2 times the stroke's extent, by the
  squared difference of `B` against the shifted `B` over the uncovered part of a
  window around the stroke, plus a cost for a candidate window that overlaps the
  stroke or leaves the image. A workgroup reduction per page and one argmin pass
  choose the candidate on the GPU, the first winning a tie, and write indirect
  draw records, so only the chosen source is gathered, with no readback. The
  healing blend follows. Only proximity matching is available; Content-Aware and
  Create Texture are not.
- **Membrane.** `h` is solved over the stroke's pages, within a window on each
  page around the dabs plus a margin: pull-push down a pyramid whose finest
  level keeps one tile per page, each cell weighted by `1 − coverage`, then 32
  red-black relaxation sweeps at the finest level, eight at a time within 16 by
  16 blocks that alternate like a checkerboard. Levels and sweeps are fixed and
  nothing uses atomics, so replays match. Up to 4 megapixels of pages heal at
  full resolution; larger strokes heal at half or quarter resolution and the
  membrane is upsampled. In integer documents a healed pixel's color stays
  within its alpha. Healing composites over the layer as the stroke found it,
  so a transparent source, like the Clone Stamp's, never erases.

The healing pipelines compile with the retouching pipelines, before pen-down.

## Performance and mobile devices

The main reason to put pixel work on the GPU is the cost of brushes that interact
with existing paint. Simple stamps can be efficient on a CPU. Smearing, blending,
liquify and painterly models require more sampling and state updates per contact,
and larger tips multiply that work. More detailed fluid and paint simulations
would increase the cost further; the current watercolor model uses bounded,
localized transport.

The engine keeps sequential input and placement on the CPU, batches the resolved
contacts and leaves layer pixels and paint state in GPU resources. The renderer
runs pixel work in parallel while retaining the ordering that each brush requires.
Sparse pages and changed-region bounds limit the affected area. Specialized paths
avoid allocating or updating material state that a brush does not use.

These choices target both performance and energy use. On mobile hardware, memory
traffic and CPU/driver overhead contribute to power consumption alongside GPU
computation. Reducing those costs can improve efficiency even when frame rate is
unchanged, as explained in [Arm's GPU energy-efficiency guide](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/energy-efficiency-in-gpu-applications-part-1).

This is the rationale for the design, rather than a measured advantage over other
painting apps. Compare equivalent brushes and image quality on the same hardware,
and measure sustained frame time and energy per stroke. The existing
[GPU workloads](../development/gpu-raster-benchmarks.md) measure our renderer's
cost; they do not establish a CPU comparison or battery-life improvement.

## Changing a brush

Start with the preset and shared settings when changing a brush's behavior. Change
CPU dynamics for placement or sensor rules, and the renderer for pixel interactions.
Avoid implementing a separate brush rule in a platform adapter.

The [raster reference](../brush-renderer.md) documents dab fields and
correctness rules. [GPU brush stages](../reference/gpu-brush-engine.md) explains
destination interactions. The [developer testing guide](../development/testing.md)
links to replay tests, benchmark workloads and preview generation.
