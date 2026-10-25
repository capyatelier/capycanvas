# Brushes

[Technical documentation](../README.md) · [Architecture](../architecture.md)

A brush has two jobs: decide where and how to place marks along a stroke, then
evaluate those marks against the canvas. Capy Canvas keeps placement in the shared
CPU engine and pixel work in the GPU renderer. Every platform uses both parts.

Solid dry media use analytic relief and stationary paper to decide where pigment
contacts the canvas. Ink and textured paint share swept placement with their own
material response. See [Contact brush engine](contact-brush-engine.md) for the
implementation and [design decisions](../history/brushes/dry-media-contact.md). The
[Bristle Paintbrush](../development/bristle-paintbrush.md) sweeps a fan of hairs
through the same contact path.

## Brush definitions and strokes

[`BrushSnapshot`](../../crates/layer-core/src/lib.rs) describes the brush settings
captured for a stroke. It includes the tip, spacing, sensor mappings, color and
rendering behavior. Built-in definitions live in
[`presets.rs`](../../crates/layer-core/src/presets.rs); shared tool controls expose
the editable settings.
Preset constructors share liquify settings, painter mappings and paper grain. Grain brushes read the drawing's [Paper](../development/paper.md); a preset sets only how strongly it catches the tooth.

Strength is the primary application control: pigment transfer for drawing,
removal for erasing, mixing for smudging and deformation for Liquify. Painting
settings retain the internal `flow` ID; Liquify retains `strength`. Opacity caps
one stroke's contribution over its starting paint, rather than reducing pigment
already present. A new stroke can build further. Liquify does not expose Opacity.
Zero Strength emits no painting contacts while retaining the brush-size cursor.
Pressure mappings scale the selected Strength instead of replacing it.
Analytic dry media use that result to scale calibrated material pressure, with
unit pigment feed before optical transfer. Strength changes contact recruitment
and conditional loading, rather than independently fading the pigment dose.
Shared [family settings profiles](../ui/toolbar-components.md#contextual-options)
place frequently used controls first and group the remaining controls.

*Dynamics* map a changing input to a brush property. Pressure might change size or
opacity; tilt can change the shape or rotation of a mark. Input availability varies
by device, but the mapping rules are shared. A stroke stores the settings it used,
so editing a brush later does not change old strokes during replay.

## From samples to dabs

The shared [input contract](input.md#platform-records) admits numerical samples
before cursor, tool and recording consumers.

The engine caps a contact at 131,072 retained real points. Reaching that limit
cancels the contact, restores its prior committed pixels and raises a shared
notice. It leaves rendering, earlier history and subsequent strokes available.

[`DabGenerator`](../../crates/layer-engine/src/brush.rs) processes the ordered path
and places contacts by distance traveled. A *dab* is one resolved brush contact:
its location, shape, color and other parameters are ready for the renderer.

Distance-based placement avoids making the brush density depend directly on how
many events a platform sends. Spacing, stabilization, taper and deterministic
variation shape stamped brushes. Swept dry media use neither spacing nor size
or rotation variation. The random seed is tied to the brush and
stroke so replay can reproduce its variation.

Dabs and shared style data are collected into batches. The renderer receives those
batches through the [render contract](../../crates/layer-render/src/lib.rs), rather
than receiving a separate platform callback for each mark.
Pointwise paint packs each tile's intersecting dabs in stroke order. Revisiting a
tile does not make it evaluate intervening contacts elsewhere on the canvas.

## Stroke smoothing

Smoothing is one control from 0 to 100, initially 6, without a section heading.
Slider travel is logarithmic, with 30 at 75% of the track.
Zero bypasses position smoothing. Each brush keeps its own choices. Selection
and coverage-mask painting use unsmoothed positions.

The shared engine uses the Curve fit model described in the
[curve design record](../history/brushes/curve-fit-work-in-progress.md).
It integrates a positive polynomial kernel over every delivered position;
continuous evaluation between samples keeps the path smooth at low input rates.
Input-interval calibration keeps strength comparable across sample rates.
The selected smoothing strength applies at every pen speed. The captured view
scale keeps curve evaluation tolerance in surface pixels.

Committed geometry depends on real samples, regardless of whether stroke preview
is enabled. Preview continues that same curve with reduced prediction lead at
high strengths, and ends with a contact at the curve's endpoint, as it does
without smoothing, so widely spaced brushes such as Liquify and Smudge follow
the pen every frame. On pen-up, the curve completes through its held endpoint;
an enabled preview lands over 32 ms before publication. With preview off, or
strength zero, there is no additional preview settlement wait. Live drawing and
replay use the same completion path, including Smudge.

The [input policy](input.md#prediction-and-corrections) keeps smoothing preview
available for mouse and screenless tablets without adding input prediction lead.

## GPU execution

Ordinary ink and erase contacts use coverage and GPU blending. Textured brushes
also sample tip and grain resources. These paths can avoid the extra state needed
by brushes that interact with existing paint.

A destination-aware brush reads what is already on the layer. Smudge, wet mixing,
watercolor and deformation therefore require ordered operations and additional
GPU resources. Depending on the brush, those resources track coverage, wetness or
paint carried by the brush. Reading and writing the same image requires controlled
staging; it cannot be treated as ordinary independent source-over blending.

How a dab lays over paint follows the document's Blending. In a Perceptual
document soft edges, opacity and flow build up on the document's encoded values,
as in Photoshop and Clip Studio Paint, so black at 50% over white paint is middle
gray; in a Linear light document they build up in linear light. Brush blend
modes use the layer formulas in the same space, and erasing is the same in both.
See [rendering](rendering.md#brushes-and-healing).

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

The Blend tool holds the blenders. Each moves or mixes paint already on the
layer and lays no color of its own. They come in three sets:

- **Blend**, for dry media and everyday softening: Smudge, Soft Blender, Blur,
  the Blending Stump, which pushes pigment into the paper's tooth and deepens
  the tone without dragging it, the Tissue, which spreads it softly over the
  peaks and lifts some away, the Grainy Blender, which puts the paper tooth
  back, the Texture-Keeping Blender, which keeps fine brush texture over the
  blended color, and the Hue Blender, which blends hue and saturation but keeps
  every pixel's lightness.
- **Wet paint**, which moves paint as a brush or knife does: the Natural
  Blender, the Wet Blender, which carries what each part of the tip picked up
  and runs dry, the Color Mixer, which carries one color averaged over the tip,
  as Clip Studio Paint's Blend mode does, the Blending Knife, which scrapes
  paint along in unmixed streaks, and the Bristle Blender.
- **Effects**: Painterly, Comb (along the strands already painted), Scatter,
  Sharpen, Marbling, Crystal and Dither.

Dither chooses whole color-and-opacity samples from the paint present when the
stroke starts. Soft edges, Strength and Opacity change the density of replaced
pixels. Spread sets a sampling radius from the captured brush size; pressure
changes contact and strength without changing that radius during the stroke.
Each source gains influence only while the same brush contact covers both it
and the destination. Pickup follows the actual tip, including its soft edges,
pressure and rotation. A later smaller contact keeps earlier valid influence;
a larger contact adds newly reached paint in proportion to its own exposure.
Transparency participates alongside paint, so blending a solid stroke into
empty space produces painted and empty pixels. Antialiased shades already in
the drawing remain valid samples with their local area weight; Dither cannot
distinguish them from deliberately painted intermediate colors. Alpha lock
keeps each destination pixel's opacity and chooses existing source hues.

Cell size chooses whole squares from 1 to 50 document pixels per side. Its
slider follows Smoothing's logarithmic curve and starts at 4 px. Larger cells
share one source-color choice. A source cell that crosses a color boundary
contributes its native colors through stable sampling, without averaging them.
The grid stays fixed while painting, zooming and crossing rendering pages.
Only contacts covering a cell's selected pixels add influence, so larger cells
leave an untouched rim and need a tip wide enough to cover them. Spread below
roughly half a cell produces no transfer. Selection boundaries clip squares;
alpha lock keeps each native destination pixel's opacity. Unchanged cells keep
their original detail.

Dither shows Spread above Cell size in Tool Settings and on the left side of
its bubble, with Cell size on the right. Spread follows Smoothing's logarithmic
curve over its percentage range.

Each set lists its blenders from the most to the least broadly useful, and
Smudge is the Blend tool's default. Sets are tool groups of the Blend tool.
Sculpting presets show their tool's icon in Tools, on toolbar buttons and as
the cursor; only the sets carry their own icons.

### Blenders and the paper

Blenders that imitate a tool touching paper (Smudge, Natural Blender, Soft
Blender, Bristle Blender, Blending Knife, Grainy Blender, Blending Stump and
Tissue) blend only the paper their tip reaches. A light touch keeps the tooth;
pressure pushes pigment into it. Texture strength sets how much they feel the
paper, and 0% makes them ignore it; Soft Blender starts at 0%. Each tip meets the
paper in its own way, with constants in the preset rather than in Tool Options;
the [decision record](../history/brushes/paper-aware-blenders.md) gives them.
The effect blenders and the wet blenders ignore the paper.

Tool Options shows a **Blender** group with Spread and the one setting each
kind uses: Streaks, Sharpness, Texture kept, Blend lightness, Grit, Stretch,
Swirl, Edge keeping, Detail size, Cell size or Swirl size. Wet brushes add
Average pickup, Persistence and Pressure refill to the Mixing group. The Mixing
Brush in Paint › Oil lays its color under firm pressure and only blends under a
light touch.

Watercolor uses its own pigment and wetness behavior. It is not a general physical
fluid simulation, and not every brush uses the same state or passes. The
[painterly paint-state reference](../reference/painterly-paint-state.md) explains
those distinctions in detail.

## Feedback and replay

The engine can draw temporary predicted contacts to reduce the apparent gap
between the pen and the stroke. Prediction uses replaceable preview state, leaving
committed paint, material state and undo history unchanged. Real input replaces
that preview. Predictions are not saved into the document. When the view is
zoomed out so far that a 4×4 or 2×2 block of document pixels covers at most one
surface pixel, dry preview paint is evaluated once per block; committed paint is
always evaluated per pixel.
Partial-opacity analytic prediction averages both current paint and immutable
stroke-start paint over the same display footprint, then writes its full RGBA
result. It cannot use the full-opacity pigment-contribution shortcut. Paper
texture predictions also evaluate per block, against the block's averaged
paper, so the predicted head's grain looks coarser until real input commits it.

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
  reference composite, with bilinear taps; a whole-pixel shift reads one texel
  of each, so it copies exactly. [`retouch_sample.wgsl`](../../crates/layer-render-wgpu/src/retouch_sample.wgsl)
  holds this sampling for every pass that reads a source.

While a retouching tool is selected, 16 stroke-start pages and the source
pipelines are ready before pen-down, and reference pages around the focus points
are captured a few per still frame. While the pen is down the sources never
upload or wait for the GPU: a reference page that would need filter images, a
decode or a wait stays empty, and the stroke is reported as a miss. The engine
replays it once contact ends, as it replays an end taper, and the replay stays
one undo step. Completed-stroke replay restores only its target raster on
renderers that track raster damage; untouched composition pages remain valid.
Object layers among the references are read through their canonical pixels,
whatever the view's zoom. When a replay finds such a page still being evaluated,
the renderer keeps the pages it needs, captures them on later frames, and reports
the stroke again once they are cached; until then the stroke stays correctable
past the usual correction window (`retouch_waiting`).

## Clone Stamp

The Clone Stamp (`BrushExecution::Clone`) lays the source down with the brush's
tip, opacity and flow. Each document's engine keeps its
[`CloneSource`](../../crates/layer-core/src/retouch.rs): the source point, the
Aligned flag, the flips and, once an aligned stroke has started, its offset.

- **Tip.** The retouching presets use the solid nib of the
  [contact brush engine](contact-brush-engine.md) with a linear edge: the
  footprint sweeps between contacts, so a stroke is one continuous stamp whose
  coverage falls linearly from its hard core to its rim, as an analytic stamp's
  does, without the scallops of stamps a spacing apart. Contacts follow the
  swept generator, a few per update, and falling pressure is limited as for
  the pens, so lifting without moving keeps the stroke's width to its end.
- **Mapping.** A stroke copies document point `p` from `flip · p + offset`. It
  takes the offset at pen-down: the kept offset when Aligned has one, otherwise
  the source point minus the stroke's first point. Aligned keeps that offset for
  later strokes and moves the source point to where the stroke left off; Reset
  Offset and setting the source start again at the source point. The stroke
  records the mapping in the editing layer's pixels, and a corrected first point
  maps it again, so replays match a direct stroke.
- **Pass.** Clone runs the dry page kernels, with the pixels a page needs from
  the source read in the same pass: each page binds the 2x2 blocks of target and
  reference pages its dirty rectangle maps to, in the eight neighborhood slots
  around it, and its dry record carries the mapping. The deposit is the dry loop
  with the source's straight color, its coverage scaling each dab. With
  stroke-uniform accumulation over Normal paint, the dabs compose to exactly the
  source's coverage times the stroke's, so the loop only raises the stroke
  coverage and the source is read once, for pixels whose coverage rose. Other
  blend modes read the source first and lay it down dab by dab. Selection
  clipping and alpha lock apply as for any dry brush, and tiles, dab ranges and
  damage stay per page. A page whose source needs a capture or a decode is
  encoded after the pages before it, so reading it cannot evict what they read.
  While the pen is down, a lone reference layer's pages are read where the
  renderer holds them rather than copied into the reference cache.
- **Source disc.** The session draws the disc from cursor segments and sets a new
  document's source in the middle of the view when the tool is first selected.
  Set Source (held Alt, a bound side button, or its button for one contact) makes
  the next pen or mouse contact set the source; a finger never does. The disc
  drags at once with every device, and a tap shows the
  [clone source bar](../ui/canvas-action-bar.md#contexts).

Only translation is supported: the source is not rotated or scaled.

## Healing Brush and Spot Healing Brush

Both heal after the pen lifts
([`heal.rs`](../../crates/layer-render-wgpu/src/heal.rs),
[`heal.wgsl`](../../crates/layer-render-wgpu/src/heal.wgsl)). A live stroke advances
through bounded GPU batches: source search/gather, seeding, pyramid levels,
relaxation, application and native raster validation/conversion. The next batch
waits for the previous one's completion without blocking the input owner.
Camera changes postpone batch admission and present the existing composition.
Tools remain selectable; dependent painting waits in input order, retaining each
contact's settings and camera. GTK forwards presentation changes independently
of its artwork mailbox.

The raster revision stays pending until all batches finish. Capture, one undo
step and late corrections therefore include the healed pixels. Replays use the
same kernels and ordering. Abandoning the job resolves its pending revisions as
failures so history and save cannot wait forever.

The two-second sensor-correction window begins once the completed stroke's own
raster revision has published its backing. A corrected replay starts a new
window when its replacement revision publishes. Other GPU work never restarts
that window; the next stroke or a history step closes it.

- **Healing Brush** (`BrushExecution::Heal`) paints the Clone Stamp's copy while
  the pen is down, with the same source, disc and options. At pen-up each page
  the stroke painted becomes `S + h` over the page as the stroke found it: `S` is
  the copy, and `h` is a membrane that matches `D = B − S` where the stroke
  leaves the image uncovered, `B` being the source composite at the destination.
  The copy keeps its texture and takes on the color and brightness around the
  stroke. `D` and `h` are in the document's Blending, so a Perceptual document
  matches tone on encoded values, as Photoshop does. Where `D` is zero, `h` is
  zero and the result is exactly the clone.
- **Spot Healing Brush** (`BrushExecution::SpotHeal`) needs no source point.
  While the pen is down it lays a translucent grey tint. At pen-up it scores 16
  candidate sources, 8 directions at 1.25 and 2 times the stroke's extent, by the
  squared difference of `B` against the shifted `B` over the uncovered part of a
  window around the stroke, plus a cost for a candidate window that overlaps the
  stroke or leaves the image. `B` is gathered once per page; each candidate's
  shifted `B` is read straight from its pages while it is scored, 32 by 32
  pixels to a workgroup. Candidates are judged in order on the GPU, and one that
  is strictly better than the best so far is laid down over each page's window
  through dispatch-indirect records, so the first best wins, nothing is read
  back, and the pages a candidate reads are still cached when its source is
  laid down. The healing blend follows. Only proximity matching is available;
  Content-Aware and Create Texture are not.
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
Spot Healing's chosen-source pipeline declares and binds an explicit empty
group between its parameters and candidate source on every backend.

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

Smudge, wet and watercolor evaluate one page job per page their contact batch
touches, each writing its whole page. Consecutive batches and watercolor transport
share one compute pass, so a large brush or a long frame adds no page copies or
passes per page or batch; only preparing decoded source tiles, a page's
starting paint, a pen-up edge or healing ends it. Dry, clone and healing
batches keep one pass each: sharing a pass across them added about 0.8 ms of
GPU time per update on the MovinkPad 11. A frame's commands and bindings stay allocated until the GPU finishes it.
A frame therefore consumes input only until its committed contacts reach 512
page jobs; Dither charges its larger batches as equivalent six-contact groups.
A slower device keeps the rest of the input queued
for the next frame instead of building one frame from its whole backlog. Watercolor
transport runs once per frame, so a device that falls behind now runs it as often
as one that keeps up. A replay that repaints a whole stroke, after a late sensor
correction, an end taper or a renderer rebuild, is submitted in windows of at most
512 page jobs that never split one watercolor update; the layer is captured once,
after the last window, so the result matches a single-frame replay.

Liquify composes eight-dab chunks into retained displacement and footprint maps,
then reconstructs changed color from the original source. At Fit zoom, a cached
display-resolution preview shows unresolved map changes while full-resolution
color waits for pen-up. Crystals keeps native-resolution integer sampling.
Map jobs share a compute pass, as do color jobs and original mip reductions.
The [Liquify guide](liquify.md) describes session
ownership, source bounds, sampling and the portable Mali path.

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
