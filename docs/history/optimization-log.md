# Rendering optimization log

> Historical design or validation record. Statements about completion and remaining
> work describe the recorded checkpoint. Start with the [current technical guides](../README.md).

Paths under `artifacts/` refer to ignored local outputs, not files shipped in
this repository. See [publication notes](../development/publication.md#publication-checks).

This is the running list of measured canvas-renderer changes and remaining
performance questions. It covers the only raster path, `layer-render-wgpu`.

## Measurement rules

- Keep the 15 legacy and 10 painter 4096×4096/32-visible-layer inputs fixed.
- Report non-blocking submit and serialized completed-work p50/p95/p99.
- Prime the scenario's real pipeline before recording.
- Use fresh canvases for repeated runs and aggregate every measured frame; do
  not select the fastest repetition.
- Keep initialization, structural texture allocation, undo warm-up, and export
  outside the timing window.
- Do not change brush spacing, coverage, or layer semantics solely to improve a
  number. A preset correction must pass the same blank/destination visual suite.
- Validate raster output visually after shader or blend changes.

## Completed

### Reused native input, cursor and GPU upload storage

Android now checks the shared UI revision plus host presentation state **before**
building a snapshot. Its input arrays have exclusive ownership while queued and
return to a bounded pool after consumption; JNI retains its numeric scratch
buffer and reads only the used portion. Debug timing uses fixed arrays and
includes publication/rescheduling rather than ending at the render call.
Four repeated 128 px workloads per brush used 3–6 input arrays, with 2–3 actual
snapshots instead of 126–140 serialization attempts per stroke. Publication/
rescheduling p95 fell from 0.305–0.375 ms to 0.056–0.074 ms. Frame tails did not
consistently improve; see [Android measurements](android-implementation.md#emulator-measurements).

GTK and Android refill native cursor geometry without formatting unused SVG
paths or allocating intermediate contours. The shared presenter retains scaled
vertices and skips unchanged camera uploads. Web still uses the same cursor
geometry and its SVG presentation.

One private upload helper recycles wgpu `StagingBelt` chunks on native platforms,
finishing/remapping after the existing submission completes. Web retains direct
`Queue::write_buffer`: wgpu's mapped-slice implementation there creates a Wasm
temporary and copies back into JavaScript memory, defeating this optimization.
This is upload transport only, not another brush/raster path. No pixel readback,
extra submission, CPU wait, or canvas-sized allocation is introduced.
See [wgpu queue behavior](https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html#method.write_buffer)
and [web mapped slices](https://docs.rs/wgpu/30.0.1/src/wgpu/backend/webgpu.rs.html).

The isolated native comparison runs all 25 fixed 4K/32-layer workloads three
times. All 25 final PNGs are byte-identical. Move completed-work p99 stays below
4.4 ms and both versions pass the harness's 8.33 ms percentile gate; individual
timings vary and this is not a blanket speedup claim. Natural Blender p95/p99
changed from 2.743/3.364 ms to 2.432/3.154 ms; Watercolor Wash changed from
3.244/4.072 ms to 3.304/4.355 ms. Full reports are ignored local files:
`artifacts/benchmarks/android-upload-before-isolated.md` and
`artifacts/benchmarks/android-upload-after.md`. These exclude native presentation
and do not establish Android's 120 Hz target. Swapchain image-view caching is
unchanged; no measured hotspot justified adding that lifetime machinery.

### Worker-owned full-window Wayland canvas

The canvas now presents directly through an app-owned Vulkan Wayland swapchain.
One worker owns brush raster, composition, viewport/cursor rendering and WSI;
GTK handles native events and controls, passing at most two small paint packets
in flight. Pen records remain queued when the worker is busy. The old image
export/import and inset-offload implementations were removed. The viewport
shader applies rounded corners without hiding any canvas area.

An independent 120 Hz timer avoids GTK's idle-scene frame-clock fallback to
60 Hz. GPU cursor rendering avoids repainting GTK for every pointer position.
Mailbox is required: FIFO stalled the tested NVIDIA/Mutter child when the GTK
parent was idle. No alternative raster or presentation path is retained.
Full results, limitations and native-input tests are in
`artifacts/benchmarks/gtk-wayland.md`. The superseded
`artifacts/benchmarks/gtk-offload.md` remains measurement
history only; it did not remove the multi-frame stalls.

### Full-window canvas independent of native chrome

GTK and DOM panels now overlay the viewport instead of shrinking it. Dock
actions and Zen fades update only native UI; they do not wake the canvas or
move its camera. Explicit Fit Canvas uses a separate shared work-area rectangle.
The current full-window subsurface path is described above. The larger viewport increases
display-image memory, but the isolated native test remains below 8.333 ms on
all 1,080 measured move frames; see the
`artifacts/benchmarks/gtk-vulkan.md`.
This is not a scanout/input-to-photon guarantee. Brush preview PNGs are generated
offline with the same GPU presets, not rendered on demand by the drawing path.

### GPU-resident contact raster and blending

Resolved contacts upload once and rasterize as instanced quads. Coverage and
paint/erase blending never touch CPU canvas memory.

### Persistent layer and composite textures

Paint layers and the composed result survive across frames. Normal movement
draws only new contacts and recomposes only conservative damage. No layer pixel
is uploaded or read back during contact.

### Prepared pipelines and grow-only uploads

Pipelines, layouts, and samplers are created with the device. Contact and style
buffers retain their high-water capacity. Each frame performs one contiguous
contact upload and one aligned style upload.

### Real pipeline warm-up

The harness draws and undoes one scenario stroke before measurement. This keeps
lazy driver compilation and device clock ramp outside the recorded distribution
without changing measured content.

### Single raster engine

All canvas-pixel behavior has one semantic and numerical implementation in
`layer-render-wgpu`.

### GPU export color conversion

Explicit export renders the linear premultiplied composite into a
straight-alpha sRGB attachment before readback. The host only removes transfer
row padding and copies the finished bytes; it does not run a canvas-wide color
conversion loop.

### Damage-only predictive branches

Topmost source-over prediction draws directly into the freshly recomposed GPU
composite and allocates no preview page. The common single destination-aware
batch samples committed sparse pages and writes only its private preview damage,
removing both committed-to-preview and preview ping-pong copies. Exact erase
copies only its damaged GPU rectangle. Composition splits page scissors at the
preview boundary, so unchanged committed pixels are never copied or hidden.

### Stateful material targets without disabled-feature writes

The destination material pass owns color transfer, stroke coverage, and
wetness deposition. Four generic target-layout variants cover color;
color+coverage; color+wetness; and all three, with a fifth all-state variant for
watercolor wetness blending. They are built and selected as one finite table.
This avoids a second brush engine while ensuring a wetness-only brush does not
allocate/write coverage and a coverage-only brush does not allocate/write
wetness. Sparse R8 pages allocate on first touched page. The reports
independently time coverage-only, wetness-only, wet-reservoir, and
smudge-advection paths plus their combinations.

Stroke lifecycle remains part of `DabBatch`. Material dispatch is a typed
four-word operation record (first contact, count, operation, erase flag) inside
the existing 256-byte aligned style record; it does not add another uniform
upload.

### GPU brush reservoir and charge

One double-buffered 64×64 RGBA8 reservoir carries paint color and amount for wet
paint only. Spatial mode preserves brush-local variation for oil and knife
marks. It loads selected pigment once at stroke start; later contacts do not
reinject it. Transparent or lower-alpha canvas cannot drain reservoir amount
because the CPU already computes deterministic charge depletion from stroke
distance. Wet contacts use microbatches of at most three, so the bounded
reservoir pass is amortized without advancing merely once per display frame.

Smudge/blender is intentionally reservoir-free. It reverse-composes ordered
contact motion, samples the old canvas once with cross-page bilinear filtering,
and transports straight color while preserving existing alpha. Physical-GPU
tests and controlled blank/destination galleries cover no invented selected
color, no transparent cuts, and coherent carried color.

### Optional stroke-final edge isolation

Uniform coverage pages double as the post-stroke edge mask. The edge shader is
encoded only at the explicit stroke-end boundary and reads a 3×3 sparse-page
neighborhood for seam-free gradients. Its cost is reported separately as
pen-up p99; ordinary move frames and brushes that opt out never run it. The
shader already writes every output pixel and passes through source color outside
the edge band, so the initial implementation's full RGBA source-to-destination
page copy was redundant and has been removed. Watercolor does not select this
path; its morphology edge is visible during movement.

### Continuous painter texture and smudge pickup

Painter presets use an analytic contact silhouette plus canvas-stable
isotropic grain. This removes the redundant detailed-tip texture sample and
prevents identical local texture coordinates from restarting at every dab.
At that stage, pure smudge used one semi-Lagrangian backtrace across each
ordered contact step and one filtered source sample. The former per-dab
displaced-source composition reproduced the edge of a color well as repeated
crescents. The single composed coordinate removes those silhouettes, while bilinear filtering
across sparse page boundaries removes stair-steps in both smudge and liquify.

That revision's corrected three-repeat 4K report kept complex-brush move p99
below 6.4 ms. Blank- and destination-canvas galleries accompany the timing
report so continuity is checked independently of speed.

### Deterministic Natural Blender chunks

Profiling a later Natural Blender workload found that the shader already knew
how to compose multiple contacts, but the engine forced every smudge contact
through a separate full-page copy, neighborhood bind group, render pass, and
source/destination swap. A representative frame contained about 39 contacts,
making command setup and page traffic dominate both CPU submit and completed
GPU latency.

Smudge now commits deterministic chunks of at most three contacts, with
additional bounds of 0.14 brush diameters of travel and four squared brush
diameters of conservative damage. The unfinished chunk is drawn immediately as
replaceable GPU preview work, then committed at a stroke-derived boundary or
pen-up. Live output, stored-stroke replay, and rebuild therefore use the same
boundaries regardless of display cadence. Influence accumulates as optical
depth per traveled brush diameter, so changing contact spacing does not change
smudge strength merely by changing contact count.

The filtered source lookup retains one manual-bilinear center sample but uses
nearest cardinal taps for low-frequency blur. Sparse-atlas sampling falls from
20 texture loads to eight per output pixel. The three-contact bound was selected
after destination-canvas inspection: two contacts missed the 120 Hz p95 gate,
while four made long-range transport visibly harsher.

The live tail also exposed that destination-preview pages were retained for
every coordinate visited during a stroke. Preview color pages now recycle around
the current damage and all preview state is released when the tail commits or is
cancelled. In the 4K Natural Blender run, completed-stroke preview pages fall
from 106 to zero and resident canvas storage from 172.0 MiB to 145.5 MiB.

On the tracked three-repeat 4K painter matrix, Natural Blender measures 2.758 ms
p50, 5.842 ms p95, and 8.176 ms p99 completed work; non-blocking submit p95 is
1.912 ms. Two of 438 move frames exceeded 8.33 ms, while the p99 and 6.438 ms
pen-up p99 remain inside the 120 Hz budget. The engine tests verify immediate
incomplete-chunk preview, identical chunk boundaries across frame cadences,
exact contact replay at pen-up, and correct active-stroke rebuild.

### Layer-wide watercolor

Watercolor uses flattened layer RGBA for pigment and one logical sparse R8
wetness channel, independent of pigment alpha and persistent until explicit
merge. The channel is physically ping-ponged during an update so every transport
stage reads an immutable source while writing its complete destination.
Sparse current-stroke R8 coverage converts overlapping contacts to a
source-over alpha increment, while one motion-directed same-layer backtrace
mixes straight color in Oklab without advecting alpha. The ragged tip now has a
pinhole-free, low-frequency varied interior so pigment and water deposition are
not perfectly uniform.

Visual validation caught two cadence artifacts. Advecting alpha created
microbatch stripes, so watercolor now preserves layer alpha during pigment
exchange. Weighting color transfer by each radial contact exposed circular
bands, so the material pass now reverse-composes a continuous motion backtrace
and samples the same layer once. A third high-resolution check found faint
density bands where antialiased fringe became solid: paint load is now applied
to old/new coverage targets before source-over conversion, making the result
independent of contact count. A later overlap test showed that alpha morphology
still darkened internal stroke intersections. Live composition now computes
near/deep binary morphology from the unioned wetness, so only the
combined wet boundary receives the dark rim, lighter inner band, and faint
outside bleed. It does not mutate paint or add a pen-up pass.

The current capillary transport is event-driven rather than a fluid simulation.
Four brush-owned, document-anchored conductance textures provide long/short and
broad/narrow connected paper fibers. Three GPU-only coarse-to-fine stages
advance a capillary activation front from wetter neighbors. Each stage derives
a local ridge tangent from the scalar field's gradient on fiber shoulders and
its Hessian at ridge crests, samples in both tangent directions plus a short
normal pair, and uses an endpoint conductance score.
Wetness therefore supplies both the material mask and local flow gradient; no
stored direction field or dab-list rescan is needed. The destination's current
wetness selects `wet_flow` or `dry_flow`, so newly reached paper participates in
the wet interaction on the next stage. Watercolor and ink share this kernel.
Maximum artist-visible effect distance remains configurable through 96 px,
while each stage clamps its individual gather to 32 px.

The first implementation ran exchange after every internal three-dab microbatch.
One visible frame cannot observe those intermediate states. The replacement
unions all microbatch damage by touched sparse page, deposits once, then runs
one three-stage transport sequence for the visible update. Only stage one copies
the source color/wetness pair into its companion; stages two and three overwrite
the same scissor, because stage one already synchronized pixels outside it.
Center-page loads bypass general neighborhood routing.

Visual review exposed four defects in the discarded exchange. Gaussian-like
averaging suppressed the visible front, a hard path minimum canceled curved
fibers at any low texel, parallel ridges produced a ladder, and large direct hops
left fixed-distance rails. The replacement uses connected multiscale warped
Voronoi webs, a bounded soft endpoint score, and incommensurate 51/31/18 percent
coarse-to-fine steps. Deposition recharges prior water toward full using the
stroke's absolute uniform coverage and writes the resulting target through MAX
blending. It therefore creates a strong local differential without counting
overlapping dabs. Excess water relaxes toward a persistent two-R8-level material
floor, so the front slows away from the dab without erasing watercolor
membership or edges.

High-rate testing also exposed source hollowing: replacing local pigment with a
distant sample split fresh marks into rails. Front propagation now admits the
strongest arriving pigment while preserving the greater local alpha, and a
weaker bounded relaxation mixes already-wet neighbors. The model is
intentionally artistic rather than mass-conserving; dry destinations gain a
stain without draining a hole from the source.

The final dedicated six-repeat report records Watercolor Wash at 8.195 ms move
p99 and 5.436 ms pen-up p99, and Wet Watercolor at 8.100 ms move p99 and
3.227 ms pen-up p99. Both pass the 8.33 ms completed-work gate. The full
`artifacts/benchmarks/watercolor-relaxation-4k.md`
retains maxima and over-budget counts rather than hiding host/GPU scheduling
outliers.

The default near-edge radii remain 10 px for Watercolor Wash and 8 px for Wet
Watercolor. Each second R8 wetness surface adds 64 KiB per touched page: 9.1%
over the previous watercolor per-page set. Prediction forks the pair into
private recyclable pages. The standard and amplified-edge galleries cover edge
semantics; the transport matrix covers four conductance fields, three flow
levels, 16–88 px distances, wet mixing, and dry bleed.

Spatial wet brushes also do not reduce reservoir amount when they encounter
their own partially transparent trail. This makes the palette knife retain its
loaded paint while existing charge depletion still controls material loss;
spatial paint remains one reservoir texture load.

## Next measurements

### Native GTK Vulkan brush and viewport — implemented

The native frame benchmark localized the severe wet-brush slowdown to the
GTK GLES backend's engine submission, not a CPU raster path or viewport copy.
Natural Blender took 268.853 ms median inside engine submission in release mode.
GTK now uses the same hardware Vulkan device/queue for the unchanged shared
brush engine, composition and viewport. Its image-export integration has since
been replaced by the worker-owned swapchain described above. These historical
measurements isolated the backend change; they are not the current presentation
path or proof of compositor/input throughput.

Across three runs, Natural Blender completed-frame median/p95/p99 became
0.527/0.724/0.876 ms. Wet Round p99 is 0.676 ms and Watercolor Wash p99 is
2.753 ms. See `artifacts/benchmarks/gtk-vulkan.md`,
including the G-Pen scheduling outlier and dependency validation diagnostics.
This fixes a measured native backend bottleneck without changing brush spacing,
wet transport, image quality, or the shared shader paths.

### Native input-to-present

Linux GPU presentation and host-path completion timings are implemented. Add
GPU timestamps and compositor presentation feedback to separate device work
from host scheduling. Completion does not measure compositor or scanout latency.

### Large tip minification

The built-in masks are small and filter well at current sizes. Add GPU-generated
mip chains when imported high-resolution tips are implemented, then compare
cache traffic and image quality across desktop and mobile GPUs.

### Sparse layer memory — implemented

Paint and predicted pixels use private 256×256 GPU pages. A 4K document with
128 empty paint layers allocates zero layer-pixel bytes instead of 8 GiB. The
document-sized composite remains 64 MiB. Metrics expose page counts and bytes;
tiling does not enter shared document or UI APIs.

### Destination-aware stages — implemented

Smudge, wet mix, non-normal blend, and liquify use lazy per-page
source/destination companions. A fragment invocation owns one destination pixel
and evaluates ordered contacts; a 3×3 page neighborhood makes pull, blur, and
bilinear deformation continuous across page boundaries. Current exact 4K
results are generated in
`artifacts/benchmarks/complex-brush-interactions-4k.md`.

### Destination submit setup

The destination path creates neighborhood bind groups for touched pages. Submit
p95 remains below the 8.33 ms frame budget in the current matrix. Caching their
alternating page-state combinations remains unwarranted until native traces
identify command setup rather than fragment work as the limiting stage.

## Deliberately excluded

- a public tile API;
- multiple GPU brush engines;
- a software adapter or host-memory raster fallback;
- a monolithic shader that pays for disabled wet stages;
- brush-spacing changes made only to improve benchmark numbers;
- readback or CPU canvas mirrors in the live path.
