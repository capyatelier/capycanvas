# Contact brush engine

[Editor internals](README.md) · [Brush design decisions](../history/brushes/dry-media-contact.md)

The shared CPU engine resolves ordered input into swept contacts. The GPU evaluates
those contacts against stationary paper and deposits pigment into sparse pages.
GTK, Web, Android, Apple and Windows use the same definitions and evaluator.
Brush definitions are in [contact_presets.rs](../../crates/layer-core/src/contact_presets.rs).

## Dry media

Pencil, Pointy Pencil, Shading Pencil, Charcoal, Chalk, Pastel Block and Crayon use
[analytic relief contact](../../crates/layer-render-wgpu/src/relief.wgsl).
Their edge taper comes from missing paper contacts, with a narrow transition in
pigment transfer. Contact geometry does not multiply the result by a broad opacity
feather. Ink, paint, scumble and bristles retain their own material response.

Each dry brush has three built-in poses: upright, writing at 40°, and side contact
at 60° from the surface normal. Cubic Hermite interpolation resolves the dimensions
and relief coefficients between keys; endpoint poses clamp outside their range. Tilt azimuth rotates the
contact. Pressure changes its size through the existing size mapping. Displayed
size is the long axis of the writing pose at full pressure. Other poses scale
relative to that axis. The largest pose supplies a separate reference diameter
for stroke retention and taper distances.
The keys use physical inclination after the shared projected-angle input
conversion, independently of tilt azimuth. Pointy Pencil's tip offset reaches its
full writing-pose weight at the same 40° key. Brush-size previews use that key too.

The accepted design defines the [analytic relief](../history/brushes/dry-media-contact.md#analytic-relief)
and [contact and loading equations](../history/brushes/dry-media-contact.md#contact-and-loading).
Elliptical relief rises away from the point; beveled sticks have a flat face and
a narrow rising rim. There is no separate pressure map or support bitmap.
Pointy Pencil offsets the relief minimum toward the stylus tip while keeping its
elliptical outline centered. Its bias follows stylus direction, including when
integration coordinates rotate with stroke motion. Pastel Block is square
upright and long and narrow on its side; its writing-pose size stays the reference.

Material pressure is `Strength*clamp(pen_pressure*pressure_gain,0,1)`, resolved
on the CPU after Strength mappings. The seven dry presets use unit gain, with
Strength defaults of 70%, 70%, 63%, 85%, 75%, 85% and 70% for Pencil, Pointy Pencil,
Shading Pencil, Charcoal, Chalk, Pastel Block and Crayon. Default material pressure
and physical pressure-driven dimensions are unchanged.
Strength sets overall material pressure; at 100%, full pressure saturates every grain it reaches.
Zero pressure or Strength deposits nothing. Increasing material pressure recruits lower paper contacts
and increases their transfer; maximum pressure need not reach every recessed area.
Texture strength scales the authored paper strength `S` from zero to twice the
preset value. Engagement width `W` is fixed at `0.05` in all built-in dry brushes
and is absent from their painting controls. Material hardness is `loading=0..1`,
displayed as 0–100%. The CPU resolves displayed hardness `h` to `H = 1 - (1 - h)^2`
before the GPU style upload. Pigment loading is `L = 1 - H + H*p*p`, where `p` is
material pressure. Displayed 50% gives effective hardness 75%; both endpoints stay
fixed. Hardness blends constant loading with material pressure squared without
changing paper contact or brush dimensions. Contact always responds to material
pressure. Pencil defaults to 70%, Pointy Pencil to 75%, and Shading Pencil to 65%.
Crayon defaults to 90%, Charcoal to 55%, Pastel Block to 60%, and Chalk to 15%. All seven dry
presets expose their material-pressure limit through their Strength defaults.
Material hardness sets light-touch loading; at full pressure loading is one at every hardness.
Texture strength sets how many deep valleys stay white at full pressure.
Tilt changes the relief itself, including
its interaction with paper, without additional per-pose paper or bite controls.
The [decision record](../history/brushes/dry-media-contact.md) explains the constants,
controls and rejected alternatives. The pose editor is not implemented.
Dry media do not use stamp spacing, size variation or rotation variation; their
contact path bypasses that math. Tip angle offsets the contact's orientation.

Paper is the nearest [Paper layer](../development/paper.md) at or below the
stroke's layer, resolved at pen-down and kept with the stroke. Its texture is
baked once into an R8 tile with one texel per document pixel; sampling depends
only on document coordinates, so moving a layer leaves the tooth on the
drawing. The paper strength is `paper × Texture strength × relief / 100 µm`.
No bitmap is generated or uploaded during a stroke; a stroke waits for its
paper's bake as it waits for shaders.

## Swept deposition

[The generator](../../crates/layer-engine/src/brush.rs) retains input vertices when
curvature, pressure, tilt or twist changes enough. Its geometric tolerance is
0.25 document pixels or 1% of the previous minor radius, whichever is larger.
Active Curve fit limits path error to 0.05 logical screen pixels, converted
through the input scale, or to 1% of the previous minor radius when that is
larger, never beyond the bound without Curve fit. Its curve subdivision samples
at the screen bound; the radius bound only decides which samples remain
vertices, so broad contacts are not evaluated once per input sample.
Straight steady runs advance at half the reference diameter, at least 4 px. Pen-up
flushes the endpoint. Segmentation follows input, and prediction clones the same
state; rendering cadence does not decide where contacts are retained.

Flow dry media start frozen at their first modeled sample. Pen-down pressure
raises greater than 0.05 update the tap pose without changing its tilt or twist.
The preview shows that imprint. Movement of three captured screen pixels
unfreezes the stroke, replaces the opening contact and first completion vertex
with that raised pose, then continues to the accepted modeled sample. Ignored
opening samples still update input dynamics; their geometry is not replayed. Mid-stroke
rest freezes the whole pose when at least 60 ms of history exists and the last
100 ms stays within three screen pixels of the current modeled sample. A preview
also freezes after 100 ms without modeled input, using the frame's current time.
Rest only controls sample acceptance; freezing does not land or flush the
committed path. Accepted samples continue from its last contact without rewinding.
A pen-up at least 100 ms after the last modeled sample ends at the held anchor
without feeding its pose. Transient stroke data retains whether its final accepted
point is pen-up; live drawing and replay share the same gap decision. The held
pose belongs to the current completion anchor. Anchors retain modeled poses,
their variation identity and the immutable settling curve for that endpoint;
contacts evaluate lazily for the lift.
Anchor steps use size-only evaluation when the quarter-minor-radius bound matters.
Samples held during mid-stroke
rest are discarded. Wet stamps, bristle contact and time-based spraying retain
their own contact behavior.

Uniform dry media hold the opening contact and prefix within one captured
screen pixel. Release resolves that tap at the strongest held pose.

Selection-mask painting flushes swept endpoints after each real sample and
resolves taps only on pen-up, using the brush's captured screen tolerance. Its
completed replay makes the same distinction. Endpoint flushing does not end a
stroke or turn pending contact into an imprint.

Flow dry media complete the missing passage at drag endpoints and selected turns.
For incident lines with full-chord exposures F and outgoing half-line exposures e,
completion adds `g*C*max(0,max(F)-sum(e))`. C is the real vertex contact and g is
one at endpoints. Turns measure angle and chord-to-travel ratio over one minor
contact radius on each side; smooth ramps span 10–30 degrees and ratios 0.3–0.6.
The generator coalesces pending positions within 5% of the minor radius, at least
0.25 document pixels. It resolves corners of that correction path with
Douglas–Peucker subdivision at 2.5% of its anchor's minor radius, at least 0.01
document pixels. Pending history is bounded to 256 vertices. Ordinary retained
segments remain intact. Modeled input advances the outgoing window; full-weight
turns resolve after a quarter radius, and other turns wait for a full radius.
Completion paths receive every retained body dab. Pen-up resolves pending turns
and flushes the last body contact. A moving smoothed stroke completes its
settling curve before finalization. Preview end-cap anchors follow the settled
pen-up endpoint, stepping by the smaller of 1.5 captured screen pixels and a
quarter of the current anchor’s minor radius. Every accepted modeled sample
updates the anchor, advancing it when committed geometry passes it. Both sampled
rest and a sample gap freeze the same lift at that anchor. A frozen pen-up
commits that lift, including its settling curve, to the held endpoint.
A later body vertex already emitted for a pose change remains painted.

Dry Flow preview shares matching real bodies with a frozen lift at anchor S.
Unmatched landing bodies are LiftOnly on the end’s owned contact; the ordinary
MainOnly continuation draws on unowned pixels. Bodies and completions keep the
lift's original order, with the end last, followed by unmatched MainOnly dabs.
The real modeled endpoint
flushes at its full radius before prediction begins, so the real span never tapers.
Each Preview batch identifies its end contact. Completion kernels compute its
geometric q≤1 ownership before the dab loop, including beveled rounded tips.
Planning omits LiftOnly bodies whose bounds miss the end contact. Pages outside
that contact omit LiftOnly and ignore preview tags when selecting their kernels.
Their original dab lists and tags stay intact; only real completions require the
completion feature on these pages. The original end stays available across page
compaction.
Other specialized kernels compose constant preview helpers. Frozen preview
adds no prediction. Every completion uses committed filtering code 2 and the normal
grip law; routing does not change that law or enter authored stroke data.
`dry_sample_gap_freezes_preview_and_ignores_the_late_up_pose` checks sampleless rest and replay.
`frozen_dry_preview_matches_canonical_stored_paint` checks exact native stored paint at pen-up.
`preview_landing_bodies_plan_only_the_pages_that_can_be_owned` checks landing culls and page ranges.
The normal engine rest tests bound slow-stroke travel and check continuous contacts.

Completion uses the same relief, loading, and optical transfer as segments.
Its contact window leaves weakly engaged grains softer than fully engaged paper.
Each planned page prefers the kernel for its own body, completion and fine-tip
features. A ready full variant serves it while that exact variant warms.
The completion helpers are absent from ordinary specialized shaders. Broad
completion variants also omit fine-contact filtering. Centered elliptical
contacts share their symmetric full-chord integral and reject the inner half of
a turn, where its incident half-lines already supply the target exposure.
The tile planner omits wholly interior turn tiles with a conservative pixel
margin. Endpoint zero planes are tangent to the contact support; completion
exposure remains nonzero on both sides of the endpoint's center. Shifted-relief,
beveled and fine-contact tiles retain their full support.
The 128-byte shared and 160-byte renderer contact records retain their sizes,
and completion adds no persistent pixel state.

Fine round completions average the analytic completion alpha over a four-point
Gauss grid in the intersection of the contact's bounding box and the pixel, weighted
by its area. One loop evaluates the center for blending and those four samples.
Filtering applies fully below a
four-pixel minor radius and blends to the point evaluator by eight pixels.
Beveled rounded contacts use point-sampled body and completion exposure at every
radius. Their specialized shaders omit the completion filter.

Repeated dry Flow preview contacts reuse their retained pages while the view, styles,
source artwork and display resources remain unchanged. Persistent input, changed
contacts or a discarded submission retire that reuse. No dry evaluation, gather
or copy dispatch runs for an unchanged preview. Pointwise painting uses one accumulator and a per-pixel end-contact ownership
flag within each preview job.
`unchanged_dry_preview_keeps_pages_and_encodes_no_gpu_work` checks retained pages and invalidation.
`completion_preview_routes_select_the_committed_filter` checks per-page contact flags.
`dry_preview_completions_use_the_ready_committed_kernel` checks preview completion readiness.

For each pixel, the dry evaluator integrates `contact*loading` over the current
segment and divides by the full geometric contact chord. Subdividing a fixed-pose
sweep therefore preserves exposure. Ellipse relief gives a quartic engagement
polynomial, whose cubic transfer has degree twelve. The evaluator clips empty and
fully engaged intervals and evaluates only the remaining polynomial moments.
A quadratic relief profile uses the shorter degree-six path. The quartic path uses
the center and three symmetric Gauss–Legendre pairs on the clipped transition
interval. Seven nodes integrate a polynomial of degree thirteen exactly; the
even/odd pair identity evaluates this degree-twelve polynomial without expanding
its square and cube. The [NIST quadrature reference](https://dlmf.nist.gov/3.5#v)
gives the exactness criterion. This is an algebraic integral, not sparse sampling
of the unclipped contact field. An ellipse interval that travels less than a
quarter of the contact radius instead applies five-point Gauss–Legendre to the
clamped transfer, skipping the four square roots that clip it. For engagement widths
below `0.05`, the CPU reduces that travel limit in proportion to the width; sharper
transitions use exact clipping over shorter sweeps. The quadrature tests hold this
within two 8-bit codes of the exact integral. Blocks split at their four inner face boundaries, yielding at most five
quadratic pieces. Flat pieces have constant contact. Neither shape uses rotated stamps.

Brush recipes specialize ellipse versus block and whether any pose has quartic
relief. Only Dry brushes may carry this contact, so the Smudge, wet, watercolor,
Liquify and Clone passes compile without it. Fully engaged intervals return their length directly. Blocks find the
full chord by intersecting four pairs of parallel planes, then split only the
portion traversed by the current segment. Segments wholly inside the flat face
use its constant contact directly.
Beveled support rejects intersections below the local float32 geometric precision
before chord normalization, so a zero-length tangency cannot become a full dose.
The CPU rotates whitened ellipse coordinates so motion follows one axis, preserving
their squared radius, and caches motion/rim constants in the GPU contact record.
Each piece computes its closest approach once and reuses it for the geometric,
empty and fully engaged interval bounds.

Fine elliptical contacts approximate the pixel footprint with two Gauss samples
across the stroke normal, clipped to its support and weighted by the pixel's
projected density, normalized to its exact trapezoid mass. Taps use four Gauss
samples in the intersection of the contact's bounding box and the pixel, weighted
by its area.
Exposure and optical alpha are averaged before recovering the equivalent dose;
Uniform contacts average exposure directly. Paper remains anchored to the output
pixel. Contacts up to four pixels in minor radius use this filtering fully,
then blend to the point evaluator at eight pixels. These fixed pixel thresholds
control antialiasing cost independently of material softness `W`. Coarse variants
omit the filter.
`fine_start_cap_preserves_the_full_pass_across_partitions` checks the body full chord across sample counts and render cadences.
`thin_filters_bound_contact_evaluations` checks the fixed body and completion sample budgets.

Quadrature nodes bind their abscissa as `f32` before sign multiplication, keeping
the generated D3D12 shader single precision.

Changing poses use a local resolved profile within the engine's retained segments.
They are an approximation to a continuously deforming contact, rather than an exact
integral of arbitrary time-varying geometry. The same approximation applies during
live drawing, prediction rollback and replay.

Dose is `6*color_alpha*exposure`. The configured `flow` field stores Strength;
resolved dry contacts carry unit `Dab.flow` and Strength-scaled material pressure
in `Dab.contact[0]`. The renderer uses unit gain for these resolved contacts,
so calibration is not applied twice. Strength does not
multiply pigment dose separately. Dry Flow lays its dose in full until the grain
holds one full-pass dose `B`; beyond that only `phi` sticks under the
[grip law](../history/brushes/dry-media-contact.md#swept-exposure-and-pigment-accumulation).
Grip follows resolved material pressure:

```text
p = contact.x = Strength * clamp(pen * pressure_gain, 0, 1)
phi = mix(0.12, 1, p^3)
room = max(0, B-D)
D <- D + mix(min(dD,room), dD, phi)
```

At Strength 100%, full pressure covers what is beneath; below 100%, full pressure
slides over part of it. Layering keeps building without a finite dose cap.
Single passages keep their exposure; self-overlap within a stroke is dwell and
follows the same buildup law as repeated strokes. Start caps follow their first
body contact; completions fill the rest of their incident pass. Raw coverage remains `A=1-exp(-D)`.
Grouped Opacity reconstructs full-opacity alpha from stored paint and retained
stroke-start paint before integrating dose, then contributes `opacity*A` over
that start. Fractional selection remains an alpha mask on each increment.
Unselected, unlocked normal batches with a single pigment integrate contacts in
stroke order before one exponential and one blend. Erasing keeps additive dose
regardless of pigment color.
Transparent paint and paint already carrying that same pigment also avoid a
per-pixel color-space round trip; arbitrary underpaint retains the normal blend.
Changing pigment colors, selections and locked alpha retain ordered deposition.
Full-opacity normal beveled blocks skip contact evaluation when the stored
pixel exactly equals their final stroke result. This comparison has no tolerance
and changes neither contact eligibility nor partial pigment transfer. Ellipse,
partial-opacity, coverage and after-stroke edge passes retain their evaluation.
Read/write destinations update only the dirty rectangle. Readonly destinations
first restore pixels outside that rectangle through a small copy kernel, then
evaluate contact only inside it. All readonly copies finish in a separate compute
pass before contact dispatches begin. Page jobs have distinct destinations and
stable original sources. Reduced display
previews preserve block averages and partial document edges. Tracked previews
compare final pixels, avoiding changes caused by intermediate restoration.
Shader preparation uses the captured contact, accumulation and pass plan before
a stroke. Dry Flow requires one full variant per pigment and output entry:
fine-tip plus completion for ellipses, completion for rounded tips. Non-Flow dry
ellipses require the fine-tip variant. Frame admission uses the same variants.
Exact variants warm at background priority during speculative idle after brush
readiness; a ready exact variant wins at dispatch, and a cold exact variant never
blocks painting.
Destination, gather, reservoir, direct drawing and dry mask recipes retain their
own dependencies.
`dry_brushes_require_one_superset_per_pigment_and_entry` checks the required set;
`dry_frame_admission_and_dispatch_use_the_ready_superset` checks cold dispatch.
`selected_dry_variants_warm_after_brush_readiness` checks visible paper lighting
precedes speculative warming.

Pointwise deposition and transported materials share their compositing helpers
but compile separate dependency sets. Each module serves one material operation
and one set of contact flags. A branch on a module constant still hands every
function it calls to the device compiler, so a module composes stub entry points
for the features it cannot reach: relief, bristles, evolving contact, retouch
sampling, the selection lookup in aggregate kernels, which only run without a
selection, and, in normal-blend kernels, the blend-mode formulas. Dry kernels
for other blend modes and wet and watercolor page kernels compile only their
brush blend mode's formula, and Smudge and Liquify page kernels, which never blend,
compile none; for brushes without contact they omit contact evaluation, and any
other contact evaluates each dab's flags. Aggregate and ordered deposition share one
contact loop, selected by the existing batch marker. They compile separate
variants so ordered compositing does not increase the fast path's register
requirements. The small-contact antialiasing variant also remains separate. Dry
kernels for other blend modes have one module per contact family and blend,
without aggregate or small-contact variants.

Inside the contact loop, erasing multiplies the paint by the contact's
transparency and every other mode deposits through the blend. The loop computes
the deposit for every contact and then selects the erased paint, rather than
branching between them on the batch's erase flag: the Adreno Vulkan driver on
the Wacom MovinkPad 14 took the deposit branch for every contact after the first
in modules that evaluate each dab's contact flags, so an eraser painted and
partial-opacity strokes lost contacts.

Selections apply after this transfer. Empty contacts remain empty through subsequent
passes with the same pose and pressure. Opacity limits one stroke; pen-up resets
the budget for the next stroke. Partial-opacity pointwise painting keeps the
starting RGBA of touched pages. In the deposition color space, Normal paint uses
`F_new=(1-q)*F_old+q*((1-O)*B+O*(color,1))`; Erase substitutes zero for the source.
The starting paint `B` preserves existing pigment while allowing later colors
to replace earlier colors at the ceiling. Prediction reads the same committed
starting paint and stores its result separately. Full-opacity Flow strokes need
no starting-paint copy or coverage state.

The compact contact payload carries pressure and three relief coefficients in the
existing contact lanes. Shared dabs remain 128 bytes; renderer records remain
160 bytes and style records 288 bytes. Relief evaluation adds no texture or
per-pixel state; the readonly pass boundary does not wait on the CPU.
Pointy's relief offset uses reserved style and contact lanes. Its integral clips
against the centered physical support separately from the shifted relief field.

Dry ellipse Flow tile bounds share the evaluator's midpoint geometry. Cached
display levels reduce only their stale regions, rounded outward to complete
averaging texels; cold, shifted and streamed inputs retain their full-page path.

Dry-flow prediction can render its pigment contribution over a transparent GPU
target, then combine it with the persistent paint. This uses the same integral,
avoiding reads of old paint during prediction. Zoomed out, prediction evaluates
one sample per display block with the block's averaged paper, which coarsens the
predicted head's grain until it is committed; committed paint keeps native grain.
Alpha lock, selections and other material responses and partial-opacity strokes
retain their existing destination path.

Updates submitted without presentation keep their source damage pending until
composition succeeds. Prediction can replace its own footprint without dropping
committed paint elsewhere on the same page.

Artwork masks preserve dry-media paper and relief. Temporary mask contacts fork
GPU pages from persistent coverage and retire on the next update; they cannot
accumulate into the committed mask. Selection and mask painting accumulate
contact coverage before applying the stroke's paint/erase Opacity ceiling.

## Other contact media

[The ink and paint evaluator](../../crates/layer-render-wgpu/src/contact.wgsl)
projects the sweep in the nib's metric, avoiding scalloped edges on angled nibs.
Ink combines contact requests as coverage union; Strength controls transfer and
Opacity limits the stroke's contribution. Boundary roughness uses stationary noise. Brushed ink uses coherent
strand gaps and declining ink supply; pooling follows the transverse profile.
The [Bristle Paintbrush](../development/bristle-paintbrush.md) retains its hair fan
and paint-loading evaluator. These responses are distinct from solid dry-media
relief; a textured paintbrush is not classified as dry pigment by its texture alone.

## Verification

The renderer's `dry_media` integration suite compares elliptical and beveled-block
sweeps with independent f64 numerical integration. It checks pressure recruitment,
empty contacts, paper stationarity, tilt continuity, segmentation, prediction and
zoom. Contact preview fixtures compare live and committed color and coverage.
`dry_press_then_draw_preserves_the_raised_start_imprint` checks alpha through unfreeze.
The `project` suite checks every contact preset through save/reopen and exact
Undo/Redo. GTK and Web journeys exercise catalog selection, sensor input and
history in both themes. Native sensor fixtures do not establish physical pen-driver
behavior.

```sh
cargo test --locked -p layer-render-wgpu --test dry_media -- --test-threads=1
cargo test --locked -p layer-render-wgpu --test project -- --test-threads=1
bash tools/performance/workspace-motion.sh gtk --native-test=native_contact_brushes
bash tools/performance/workspace-motion.sh web --contact-brushes
CAPY_CONTACT_PREVIEWS_ONLY=1 cargo run --locked --release -p layer-bench -- --brush-previews
```

Current reference-hardware results belong in the [performance tier tables](../PERFORMANCE_TARGETS.md).
