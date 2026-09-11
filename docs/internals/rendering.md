# Rendering and composition

[Technical documentation](../README.md) · [Architecture](../architecture.md)

The renderer turns document changes into pixels and combines the layers into the
visible drawing. It retains GPU resources between frames so a new stroke usually
updates a small part of the existing image.

## Stored pixels and the viewport

[`WgpuRasterizer`](../../crates/layer-render-wgpu/src/lib.rs) stores painted layer
content in sparse 256 × 256 texture pages. Pages are allocated as regions are
touched, rather than reserving a full-size paint texture for every empty layer.
Brushes that need coverage or wetness can allocate additional state alongside
those pages. Composite images and filter intermediates have their own storage,
so sparse paint pages do not make total memory independent of document size.

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

## CPU access and startup

Drawing and presenting do not read the canvas back into CPU memory. Export explicitly
requests a full image; UI thumbnails and color sampling use separate bounded
requests. These are deliberate interfaces, not an alternate path for painting.
Imported source bytes can remain in CPU memory for project persistence.

GPU preparation is staged so the platform can display its controls before the
entire brush and filter catalog is ready. Required document and brush dependencies
must be prepared before drawing uses them. This avoids moving expensive pipeline
creation into an otherwise small stroke update.

For brush-specific passes, continue with [Brushes](brushes.md). For performance
work, use the [measurement guide](../development/testing.md#performance).
