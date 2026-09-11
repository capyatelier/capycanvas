# layer-render

[Package overview](../../README.md#package-layout) · [Architecture](../../docs/architecture.md)

`layer-render` defines what the engine sends to a renderer and what it can request
back. It depends on [`layer-core`](../layer-core/README.md), but contains no `wgpu`
or windowing types. The GPU implementation lives in
[`layer-render-wgpu`](../layer-render-wgpu/README.md).

## The rendering contract

A `Dab` describes one resolved brush contact. Pressure mappings, spacing and random
variation have already been evaluated by the engine. `DabStyle` and `DabBatch`
group contacts that share rendering properties.

A `FramePacket` borrows the prepared contacts and document changes for one
submission. It includes the affected document regions, allowing the renderer to
retain previous results and update only what changed. The `CanvasRenderer` trait
accepts this work without exposing the renderer's textures, queues or caches to
the engine.

Separate methods handle source-image uploads, previews, color sampling, selections
and explicit export. Drawing and presentation do not require a CPU copy of the
canvas. `ReadbackImage` represents an explicitly requested image, rather than a
surface that the host redraws every frame.

## Where to start

| Source | Contents |
| --- | --- |
| [lib.rs](src/lib.rs) | `CanvasRenderer`, frame data, brush records and image requests/results. |
| [outline.rs](src/outline.rs) | Brush-tip outlines derived from source masks. |
| [telemetry.rs](src/telemetry.rs) | Renderer timing and resource measurements. |
| [png_export.rs](src/png_export.rs) | PNG encoding behind the optional `png` feature. |

Contract changes can affect the engine, GPU renderer and native host wrappers.
The [rendering guide](../../docs/internals/rendering.md) explains how the concrete
renderer uses these types, and the [brush raster reference](../../docs/brush-renderer.md)
describes the dab fields and batching rules.
