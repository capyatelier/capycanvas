# Rendering and composition

[Technical documentation](../README.md) · [Architecture](../architecture.md)

The renderer has to keep a large, layered document responsive within a limited
graphics-memory budget. Hundreds of layers may contain only small parts of an
illustration. Storing 200 full-size 4096 × 4096 RGBA8 images would require 12.5 GiB
before masks, brush state or intermediate results. Rebuilding those images for
every pen sample would also repeat almost entirely unchanged work.

Capy Canvas allocates painted regions in tiles, retains their GPU textures, and
tracks which composition results an edit invalidates. Shaders draw the marks,
apply effects and combine layers into the visible image.

## Stored pixels and the viewport

[`WgpuRasterizer`](../../crates/layer-render-wgpu/src/lib.rs) stores painted layer
content in sparse 256 × 256 texture pages. Pages are allocated as regions are
touched, rather than reserving a full-size paint texture for every empty layer.
Brushes that need coverage or wetness can allocate additional state alongside
those pages. Composite images and filter intermediates have their own storage,
so sparse paint pages do not make total memory independent of document size.
These pages are software-managed regions of the document, separate from the
small on-chip tiles used internally by some GPU architectures.

The viewport is the presentation of that document at the current camera position,
zoom and rotation. The shared
[`ViewportPresenter`](../../crates/layer-render-wgpu/src/present.rs) samples the
composed image into the platform's target. Panning the view does not, by itself,
require repainting committed raster tiles.

Viewport presentation and staging uploads return mapping failures to their host.
A device removed during buffer allocation must not unwind the render owner; the
host can reconstruct its GPU while retaining the shared document session. Uploads
continue to reuse the staging belt and never wait for GPU completion on the UI
thread.

## Native SDR working color

The headless native-document factory is currently a qualification route. GTK's
exposed factory remains sRGB8 until the complete color/photo workflows and memory/
latency gates pass. Native integer8/integer16 documents use their selected RGB
primaries, Float32 color working attachments and Float32 scalar coverage. Native
commit publication quantizes affected pages into the declared backing depth;
view transformations do not change those pages.

[`working_color.wgsl`](../../crates/layer-render-wgpu/src/working_color.wgsl) shares
unassociation, interpolation and perceptual conversion across scene composition,
effects and materials. Positive alpha is divided directly; only zero coverage
returns black. Native scene/image interpolation uses explicit Float32 texel loads.
Ordinary source-over and the existing channel blend formulas operate in **linear
document RGB**, independently of bit depth. Explicit Add/Subtract bounds remain
part of their artistic formulas. Native Oklab material mixing converts through
linear sRGB/D65 (including document-white adaptation), uses signed cube roots,
then returns to document primaries without a blanket negative-RGB clamp. Oklab
endpoints retain the selected operand. Region tolerance uses encoded document RGB
weighted by coverage, independent of display/checker colors.

Watercolor keeps its water activation threshold as a material-model parameter.
That threshold no longer rejects faint native pigment. Native transport constrains
coverage while retaining extended RGB between commits; it does not clamp RGB to
alpha. These helpers are not a claim that all brush dynamics and nonlinear tone
controls are fully qualified. Active workload limits and remaining workflow gaps
are in the [GTK milestone record](../history/color-management-gtk-m2-validation.md).

[`view_color.rs`](../../crates/layer-render-wgpu/src/view_color.rs) converts the
composition to explicitly declared sRGB, Display P3 or extended-linear sRGB view
coordinates. Surface format controls output transfer encoding. Application colors
in viewport overlays retain their sRGB definitions. Export/Navigator thumbnail
readbacks are explicitly sRGB8; exact color samples retain document RGB. Native
profiled delivery is a separate output conversion and is still being integrated.

## Incremental composition

The *compositor* combines paint and image layers, groups, masks, clipping and
blend modes. Its [scene code](../../crates/layer-render-wgpu/src/scene.rs) tracks
*damage*: regions whose previously rendered pixels are no longer valid.

A simple paint update rasterizes new dabs into the relevant layer pages and
recomposes affected regions. More complex layer structures need intermediate
results. The renderer retains those results and tracks their dependencies so,
for example, changing a clipping layer does not force unrelated source images
to be rebuilt.

These dependencies branch: a masked adjustment needs the original image, its
filtered result and the mask. Other layers contribute separately to the final
composite. The [README's tile example](../../README.md#rendering-engine) shows a
clipped adjustment between paint and ink. Editing its mask changes the adjustment
and subsequent composition; it does not change the stored paint, ink or background.

[Image-stage caching](../../crates/layer-render-wgpu/src/scene_images.rs) handles
operations that need reusable image inputs. A blur needs pixels outside its output
rectangle, so filter definitions describe their sampling footprint. Global effects,
layer reordering and invalidated caches can require much larger updates than a
single brush mark. Animated effects also need updates without new pen input.

The [clipping regression record](../history/filter-clipping-regression.md)
provides a concrete example of a cache invalidation bug and the work it caused.

A transform or placement drag skips both the full-resolution pages and
composition when a native document keeps a complete display pyramid and the
moving layer is an unmasked top-level layer under normal static layers.
[`render_display`](../../crates/layer-render-wgpu/src/paint_transform.rs) draws the
moving layer into the level the view samples, at most sixteen layer pixels per
texel side, and the coarser levels are reduced from it. While the Transform is
still, the layer is reduced once per transaction as the exact area mean of its
full-resolution pixels, to the level of its own pixels that matches the display
under its placement. When the selection keeps some pixels in place, those are
reduced apart from the pixels it moves. Each drag frame
[resamples](../../crates/layer-render-wgpu/src/paint_transform/resample.rs) the
copy with one bilinear sample per texel, the moved pixels through the transform
and the placement and the kept ones through the placement alone. With content
above or below, the
[layered display](../../crates/layer-render-wgpu/src/paint_transform/layers.rs)
composes the static layers once at that level, then places the moving layer
between them with its blend. A drag waits for them, the moving photo's decoded
tiles and the reduced copy. After release, the still preview of an unplaced
layer is drawn as the exact area mean; a placed layer keeps its resampled
preview. Later frames draw the preview's pages and recompose what the drag
touched a few tiles at a time, reporting pending work so hosts keep drawing.

A Warp transform is a [mesh](../../crates/layer-render-wgpu/src/paint_transform/mesh.rs)
of Bézier patches. Its pages are drawn a window of four by four pages at a
time: the mesh, tessellated within half a pixel and extended by a skirt past
its edges, is first rasterized into a texture of the source position at each
destination pixel, and the transform pass samples the original there,
averaging a pixel's footprint from its neighbors' positions where the mesh
shrinks it. Paint, masks and a selection's moved pixels draw this way in the
preview and when applied. A drag rasterizes the mesh at the display level's
texels instead, tessellated within half a texel, and resamples the reduced copy
at those positions, still or moving; the still preview then settles its pages
without drawing the exact display first. A pixel selection moved by a warp is
resampled on the GPU the same way, a window at a time. What draws meshes
compiles in the background when a warp is first shown, and until then the
preview keeps the frame before it.

A [placement drag](../../crates/layer-render-wgpu/src/placement_drag.rs) is a
frame in which only one layer's placement changed. Its layer's own pixels are
reduced once and kept between drags while they are unchanged, together with the
static layers around it. Until that copy is complete, a lone layer over the
paper is drawn from the display level as the drag began, within the canvas that
level showed. Each drag frame resamples the copy through the new placement, and
the frame's full recomposition is skipped. Frames in which nothing moves keep
the drag; once the placement has stayed still for a few frames, what the drag
drew is recomposed a few tiles at a time. When a transaction may move a layer,
the pipelines its drag draws with and the one that composes placed layers are
compiled in the background while input is quiet, and that recomposition waits
for them.

[Preparation](../../crates/layer-render-wgpu/src/preparation.rs) for a drag and
the recomposition after one, including the layer's copy and the static layers
around it, are spread over frames by the GPU time that earlier such frames took,
measured with timestamps where the device has them. A drag that starts
meanwhile waits behind at most a frame of that work.

Only drag frames and placed still previews resample, and a still display is
otherwise approximate only at the edges of partly transparent layers; pages,
Apply and commits are exact.

## Filters

Filters are stored as effect layers in the document. An adjustment processes the
combined image beneath it within its group. Clipping restricts it to a clipping
stack, and the effect layer's mask and opacity control its influence. Successive
adjustments receive the result of the earlier ones. Generator effects instead
produce new image content that participates in ordinary layer composition.

A clipped adjustment processes its clipping stack before that stack is combined
with the unrelated background. Its mask and opacity mix the original and adjusted
colors while preserving the base layer's coverage. Applying the effect to the
already flattened image would incorrectly include the background. Treating the
adjustment as another ordinary paint layer could also increase opacity where it
overlaps the original.

A runtime filter consists of a JSON definition and WGSL shader code. The definition
describes its parameters, inputs and execution requirements. Shared code validates
it, supplies the control descriptions and prepares the GPU pipeline. The same runtime handles
built-in and imported filters.

Curves and Gradient Map retain specialized Rust interpolation and lookup-table
preparation. They should not be used as examples of a filter that can be expressed
entirely by adding a JSON/WGSL pair.

The [runtime filter reference](../reference/runtime-filters.md) is the contract
for implementing a filter. The [Tent Blur example](../../examples/filters/tent-blur)
provides a small package to study.

## Shader fusion and intermediate images

A pointwise adjustment computes each output pixel from the input at that position.
Compatible chains of these adjustments are fused into a single fragment shader by
[`effects.rs`](../../crates/layer-render-wgpu/src/effects.rs). Each adjustment can
consume the previous one's result without writing it to a texture first. Ordinary
aligned masks can be sampled in the same shader, preserving each effect's mask,
opacity and blend rules. Binding limits and incompatible operations can split a
chain into separate passes.

This reduces intermediate allocations, memory traffic and pass setup. Some
composition steps can also be folded into the effect shader, and tile draws that
share an attachment can share a render pass. Fusion changes execution, not the
order or scope of the layer operations.

Filters that sample neighboring pixels need a different path. The
[image-stage implementation](../../crates/layer-render-wgpu/src/scene_images.rs)
retains reusable GPU images, tracks input changes and reuses compatible preceding
results where possible. Each image has document-coordinate bounds independent of
its texture size. Region capture expands its window by the accumulated declared
filter support, and applies the existing group, clipping, mask and effect logic
inside that window. Shaders keep document coordinates for their calculations;
current-pass and original-input sampling each carry their own texture origin.
Native one-to-one image reads use fragment positions directly, avoiding a
window-size-dependent interpolation error from reconstructed UV coordinates.

Filter previews scan four source tiles per asynchronous completion, including
the probe's corner-sampling halo. Preview rows then share a source crop expanded
by their required support. A document edit cancels an unfinished scan after its
in-flight completion; no result may combine source revisions. Global samplers
retain their full declared input. Their scheduling and allocation limits still
need qualification. Live composition also still uses full-document image stages
and a full composite. Sparse storage and cropped previews therefore do not yet
bound the total cost of large filters or densely painted documents.

The native [snapshot renderer](../../crates/layer-render-wgpu/src/snapshot.rs)
prepares document metadata independently of the live full composite. A file or
inspection worker owns an immutable project snapshot and a native Float32
renderer. Region requests restore only the translated paint, material and mask
pages needed by composition and its halos. Compressed backing remains shared;
restoration uses the same integer decoder as live editing. Initial masks use the
existing GPU crossing/coverage and affine-resampling shaders, with bounded
output rectangles and row slices of immutable packed selection coverage.

Snapshot PNG/TIFF output streams sixteen-row strips through the working-color
encoder and profiled row writers. A matching, unmodified source with default
conversion and no matte bypasses composition to preserve exact integer samples,
including hidden straight RGB. An explicit matte composites in linear document
RGB before encoding. Region captures return linear-premultiplied document values;
they contain no viewing or mask-area overlay. Cancellation and writer failures
return errors; the caller must publish its temporary file only after success.

Capture dependency plans have an explicit byte ceiling checked before restoring
pixels. This planning limit is separate from measured peak process/device memory,
codec buffers and retained compressed sources. Global samplers can exceed it and
still need their scheduled, qualified route. Snapshot capture/output is currently
a headless worker API; GTK export UI, progress/cancellation ownership and recipes
still need integration. This does not yet replace live composite residency.

## GPU resources and unified memory

Committed edits queue exact readback of changed 256² tiles for raster history
and persistence. Mapping and lossless compression run on a bounded worker; GTK
input never waits on the GPU. Move frames and presentation do not perform full
canvas readback. The exposed host export still requests a full image; the native
snapshot API streams bounded strips. Thumbnails and color sampling use separate
bounded requests. Source bytes and immutable compressed
tile backing are shared with save snapshots. See the [raster project contract](../reference/project-format.md).

On unified-memory hardware, CPU and GPU share physical RAM. Keeping separate
copies solely to move an image between processors can waste both memory and
bandwidth; older pipelines built around discrete GPU memory need to account for
this. [Apple's image-processing guidance](https://developer.apple.com/videos/play/wwdc2021/10153/)
describes those costs and the opportunities to remove redundant copies.

Here, "GPU resources" describes how the renderer accesses the pixels, not a
requirement for separate VRAM. Textures still have access rules and layouts,
and CPU/GPU coordination still matters. Capy Canvas keeps the drawing pipeline
in GPU resources through composition and presentation; imports, readbacks and
platform APIs may still need staging or copies.

## Startup

GPU preparation is staged so the platform can display its controls before the
entire brush and filter catalog is ready. Required document and brush dependencies
must be prepared before drawing uses them. This avoids moving expensive pipeline
creation into an otherwise small stroke update.

For brush-specific passes, continue with [Brushes](brushes.md). For performance
work, use the [measurement guide](../development/testing.md#performance).
