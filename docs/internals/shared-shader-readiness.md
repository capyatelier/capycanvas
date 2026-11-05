# Shared shader readiness

[Editor internals](README.md)

Every host uses the same demand-driven dependency tracking and input admission
policy in `layer-render-wgpu`.

Compiler failures retain the first reported cause, including a native worker's
panic payload. Dry-material labels identify the target, shader entry, operation
and contact flags, so a driver rejection can be traced to its actual recipe.
Recipes for one dry target share its two coverage-layout pipeline layouts.
Material paint and compute recipes capture full or partial stroke opacity.
Partial opacity values remain uniforms and share one prepared recipe. Gathering,
reservoir exchange, direct drawing and visibility masks retain their own keys.
Hosts keep polling compiler failures after initial startup completes, including
pipelines requested by a later transform frame.

## Order

Startup prepares paper/presentation, the actual document, then the selected
brush and physical eraser. Native document dependencies include the display
hierarchy's reduction kernels. Transform, warp, region tools and other brushes
prepare on first use. Unused filter programs retain recipes. Shared pipeline
recipes deduplicate work and prepared handles survive tool changes; a first
selection can wait for compilation, while later selections reuse those handles.
Startup cannot fail because of shaders needed only by an unused brush.
Each material operation compiles its own modules, specialized for brushes
without contact; other contacts share a module that reads each dab's flags.
Dry kernels for other blend modes and wet and watercolor page kernels are also
specialized for the brush blend mode; Smudge has one per contact and blender kind.
Non-dry operations exclude dry relief. Deposit and coverage retain their
captured contact capabilities.
Material recipes use the captured contact flags, accumulation and blend mode, with
separate entries for destination targets, gathering and reservoir exchange.
Smudge and wet brushes prepare the page kernel their commits write and
the color kernel their predictions write; wet brushes also prepare the
reservoir exchange, all compute. Watercolor prepares one kernel that
writes color, coverage and wetness for both.
Liquify prepares the selected mode's map kernel, its shared original-image
reconstruction kernel and original mip reductions. Map recipes distinguish
analytic untextured tips without a selection from the general coverage path.
The same prepared kernels handle persistent and predicted tiles.
Readiness requests only the entries used by the selected pass plan, including
its compute and display-preview eligibility; custom contacts use the same recipe
cache as presets. Changing stroke opacity across full opacity refreshes preview
dependencies; edits within reduced opacity reuse the prepared recipes.
Direct drawing and dry visibility masks specialize their advanced fragment
entries with the same captured contact flags. Vertex bounds remain dynamic.
Native paint requires tile decoding before document readiness, including drawings
without imported images: restoration and Undo consume that same decoder.

A dry brush prepares only the kernels its next stroke can select. Each batch
chooses fine-tip filtering when a dab semi-axis is under 8 px and a single-pigment
kernel when all its dabs share one color. `dry_material::Reach` bounds the tip size
and color variation from the brush's size range, poses, mappings and jitter; the
document's selection and alpha lock rule out single-pigment batches. Changing
size, opacity, selection or alpha lock prepares again before the next contact,
never during one, and debug builds assert that once a brush is ready, every
kernel its strokes select was prepared. Kernels without fine-tip filtering omit
its shader code. Once the brush is ready and speculative work is admitted, every
host prepares the other tip size in the background so a later size change is
usually ready; selecting another brush discards that queued work. This
background work does not hold startup completion. A brush with another blend
mode prepares one commit and one prediction kernel for its contact family and
blend, with no tip-size or single-pigment variants. Web's
`--tip-resize-readiness` checks that a Pencil resized from 4 to 300 px after an
idle pause needs no compilation.

One evaluator per variant serves every destination, so a brush compiles one
heavy kernel per variant. Where read-write storage exists, the evaluator reads
each texel's starting paint from its own output and stores only texels whose
bits changed, marking their cells when the reduced display tracks the page.
Commits update their page in place and display contributions start from zero;
any other prediction first stores its starting paint in its output, by copying
the page or, on the reduced display, by averaging it in a small shared pass that
also averages stroke coverage and stroke-start paint. Without read-write storage
the evaluator samples the starting paint and commits write the other half of a
ping-pong pair. Sampling a page inside the evaluator, even on a branch commits
never take, costs Adreno and Mali tablets up to a third of their painting rate.

The native compiler workers and browser task runner share `shader_admission.rs`.
Native hosts run `available_parallelism() / 4`, clamped to one through four,
required jobs at once, plus one thread for an optional pipeline compile. Only
independent required pipeline recipes compile concurrently. Generic recipes,
effect preparation, validation and optional work run alone; ordered jobs retain
queue order. Optional work waits for required jobs to finish, but required work
never waits for an in-flight optional pipeline compile: native hosts start it on
another thread, and browsers start its batch while the optional promise resolves.
Required canvas/brush dependencies and explicit package validation retain
priority and progress while input arrives. Visible optional previews wait for
200 ms without input and no continuous canvas work. Host reports, such as layout
measurements and system theme changes, are not input; a panel that remeasures
while it updates must not postpone optional preparation. Required compilation renews the
quiet interval on completion. Strokes, held gestures, queued input, pending
edits and settings keep optional work parked. Native workers sleep on their
existing condition variable; browsers use a host timer. An in-flight driver call
cannot be interrupted; admission happens between jobs.

Selected-pixel standby uses the same optional admission. It requests source
decoding, selection and reduction pipelines before capturing pixels; a frame
skips standby until those handles are ready. Selecting pixels on a reduced view
does not compile pipelines on the render owner or block the current canvas.

The browser admits one optional pipeline per task; required pipeline batches
remain bounded to four. Requested brush masks publish
when their generation jobs finish. Brush readiness includes their upload. Startup
completion describes current dependencies and host catalog admission, not the entire
tool catalog.
Retouch shader handles survive tool changes; unused source pixels and healing
buffers are still released.

Clone requires gathering and copying source pixels. Healing also prepares its
membrane kernels; Spot Healing additionally prepares candidate scoring, judging
and selection. Shared retouch preparation carries the selected execution so a
tool change updates readiness even when its source and focus points stay fixed.
Dry documents omit watercolor composition until a wet backing or selected
watercolor brush requires it. Native publication omits gray color variants until
the document contains a gray paint source. Presentation prepares backdrop blur
only when backdrop regions are configured.
Unresolved raster backings retain conservative watercolor preparation until their
material state is available.

Application controls such as Settings remain usable while required shaders and
raster work prepare. Shared command classification keeps these controls outside
the ordered drawing-edit queue; file commands still wait for preceding edits.

The initial backdrop frame includes only constant fills. Raster reconciliation
and pending bake dependencies follow that scene scope, so excluded paint stays
in its immutable backing until the full document and its decoder are ready.

Raster backing admits frames by pending captures and their byte budget. The
native worker creates its staging spares before processing queued captures;
spare preparation does not make an empty queue full or block a source-backed
drawing's first frame.

Readiness continues after initial startup. Shared `UiSession` wakes the host
when a UI-only brush change needs preparation. Native readiness snapshots use
the current dependencies, so a previous tool's ready flag cannot authorize the
new tool. Hosts pass every sample to `UiSession::pen`. A contact begun before
`CanvasRenderer::paint_ready` holds whole in the session's ordered edit queue,
with its drawing and camera, and is delivered once the current document and
brush are prepared. Only unfinished contacts expire after 5 s; completed contacts
wait for preparation. The queue remains bounded to 4096 samples, and a Cancel
discards its held contact. Tool and brush changes made after the contact began
queue behind it, so the stroke keeps the brush it began with. While samples are
held, document snapshots such as Save wait, and parking, suspension and renderer
replacement retire them with the queue, so they never reach another drawing.

A loaded Paper waits until earlier drawing work finishes. Its decoded pending
image keeps shared frame demand active until the choice is applied or rejected.
Waiting for disk I/O and suspended painting do not request polling. Once Paper
applies, its relief preview finishes before frame demand returns to idle.
Paper shading is part of the presenter's own pipeline. The slope pipelines
that turn a bake into its slope tile compile as optional work for a lit paper
and at preview priority, with the relief's own bake, while the Scale preview is
shown. Paper bakes advance one at a time,
most urgent first, one step per completed GPU submission, and a newer request
drops unstarted bakes of the same paper at its priority. While Scale moves, bakes of a paper that already has one wait
until release.

Frame readiness covers each batch's actual dependencies. For every distinct
batch style, `raster_dependencies_ready` requests the style's Paper bake and tip
mask, and its pipelines when the startup brush does not cover it; encoding
selects the same texture set. A frame whose dependencies are missing waits with
its input instead of failing. A texture set keeps the Paper relief it was built
with, so evicting a bake does not change the pixels of an existing set.
`ShaderDocument` memoizes the dependency-relevant document state: ordinary
raster/parameter edits do not invalidate readiness or trigger effect preparation.
Changing effect blend mode or crossing between identity and transformed reference
axes updates the shader dependencies; moving an identity reference does not.
Effect-chain inspection borrows layers and computes its variant key once per cache
search; only compiler jobs clone owned inputs.

Android and GTK no longer reload their already embedded filter package during
startup. Android also stops packaging that redundant asset copy. GTK still
accepts explicit `CAPY_FILTERS_DIR` / `CAPY_FILTERS_MODE` overrides; runtime
package installation and atomic validation remain available. Identical library
refreshes are no-ops on all three hosts. Prepared pipeline recipes remain in
process for reuse. The app does not persist opaque driver pipeline-cache data
or submit stored bytes to the driver. Native startup completes after required
dependencies and host catalog admission, without a disk-cache finalization job.
