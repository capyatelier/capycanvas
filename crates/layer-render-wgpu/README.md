# layer-render-wgpu

[Package overview](../../README.md#package-layout) · [Architecture](../../docs/architecture.md)

`layer-render-wgpu` implements painting and layer composition on the GPU.
`WgpuRasterizer` consumes the frame packets defined by
[`layer-render`](../layer-render/README.md). Native clients and the browser use
this implementation through Vulkan, Metal, D3D12 or WebGPU.

## Retained rendering state

Painted content lives in sparse texture pages that are allocated as regions are
touched. New brush contacts update those pages, and the scene compositor combines
layers, groups, masks and effects into the visible image. It tracks changed regions
and caches intermediate images where effects need reusable inputs.

The viewport presenter draws that result at the current camera position, scale
and rotation. Platform clients supply the GPU device and presentation surface;
this crate supplies canvas-pixel behavior. Offscreen construction is also used by
tests and benchmarks.

Simple ink and erase brushes use coverage and blending. Brushes that interact with
existing paint use additional state and ordered GPU passes. Prediction has separate
preview storage. Shader preparation is staged, and export, thumbnails and color
sampling use explicit readback paths outside ordinary drawing and presentation.

## Where to start

| Source | Contents |
| --- | --- |
| [lib.rs](src/lib.rs) | `WgpuRasterizer`, paint resources, brush submission and the renderer implementation. |
| [scene.rs](src/scene.rs) and [scene_images.rs](src/scene_images.rs) | Layer composition, effect dependencies and cached image stages. |
| [present.rs](src/present.rs) | The shared viewport presenter. |
| [startup.rs](src/startup.rs) | Dependency ordering for staged GPU preparation. |
| [effects.rs](src/effects.rs) | Runtime filter validation and GPU preparation. |
| [export_readback.rs](src/export_readback.rs), [thumbnails.rs](src/thumbnails.rs) and [color_sample.rs](src/color_sample.rs) | Explicit image export and smaller UI readbacks. |

Start with [rendering and composition](../../docs/internals/rendering.md), then
[brushes](../../docs/internals/brushes.md) or the
[runtime filter contract](../../docs/reference/runtime-filters.md). Renderer checks
need a working GPU backend; see [testing and performance](../../docs/development/testing.md).
