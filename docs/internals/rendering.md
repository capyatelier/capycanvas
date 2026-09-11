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
require reconstructing every stroke.

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
results where possible. Some of these images are full resolution. Sparse layer
storage therefore helps with mostly empty artwork, but does not remove the memory
cost of large filters or densely painted documents.

## GPU resources and unified memory

Drawing and presenting do not read the canvas back into CPU memory. Export explicitly
requests a full image; UI thumbnails and color sampling use separate bounded
requests. These are deliberate interfaces, not an alternate path for painting.
Imported source bytes can remain in CPU memory for project persistence.

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
