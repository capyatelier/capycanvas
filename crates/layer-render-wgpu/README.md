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

Large replays use ordered submission chunks. wgpu can expand one recorded
render pass into multiple native Metal command buffers when finishing an
encoder. Replaying seven filled 4096×4096 layers in one encoder exhausted
Metal's 4096-buffer limit, even without a window or instrumentation.
The internal encoder starts a new chunk after 512 render/compute passes.
It closes staging uploads before finishing and submitting each chunk in order;
upload completion callbacks remain on the final chunk. Small frames still use
one submission, and production rendering adds no CPU wait for GPU completion.

The physical-GPU regression compares every exported pixel of a full 4K replay
with incremental layer fills, with renderer telemetry disabled and enabled:

```sh
cargo test --release -p layer-render-wgpu --lib \
  multilayer_4k_fill_replay_matches_incremental_submissions \
  -- --ignored --test-threads=1
```

This checks replay correctness and submission capacity, not a frame-time budget.

## Where to start

| Source | Contents |
| --- | --- |
| [lib.rs](src/lib.rs) | `WgpuRasterizer`, paint resources, brush submission and the renderer implementation. |
| [scene.rs](src/scene.rs) and [scene_images.rs](src/scene_images.rs) | Layer composition, effect dependencies and cached image stages. |
| [present.rs](src/present.rs) | The shared viewport presenter. |
| [startup.rs](src/startup.rs) | Dependency ordering for staged GPU preparation. |
| [submission.rs](src/submission.rs) | Ordered, bounded render/compute submission chunks. |
| [effects.rs](src/effects.rs) | Runtime filter validation and GPU preparation. |
| [export_readback.rs](src/export_readback.rs), [thumbnails.rs](src/thumbnails.rs) and [color_sample.rs](src/color_sample.rs) | Explicit image export and smaller UI readbacks. |

Start with [rendering and composition](../../docs/internals/rendering.md), then
[brushes](../../docs/internals/brushes.md) or the
[runtime filter contract](../../docs/reference/runtime-filters.md). Renderer checks
need a working GPU backend; see [testing and performance](../../docs/development/testing.md).
