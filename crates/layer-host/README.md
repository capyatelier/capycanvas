# layer-host

[Package overview](../../README.md#package-layout) · [Architecture](../../docs/architecture.md)

`layer-host` shares session and renderer integration between the Android, Apple
and Windows clients. It combines [`layer-ui`](../layer-ui/README.md) with the
[`wgpu renderer`](../layer-render-wgpu/README.md), handling common input transport,
startup state and UI snapshots. GTK and web integrate the shared session directly.

## NativeHost and Renderer

`NativeHost` owns a `UiSession`, tracks canvas readiness and prepares state for the
native frontend. Pointer batches preserve the view revision from the moment their
samples were collected. UI snapshots are refreshed when relevant state changes,
with camera updates tracked separately.

`Renderer` wraps an optional `WgpuRasterizer`. This allows a host to create editor
state before attaching the GPU renderer and preparing its shaders. Pixel operations
require the attached GPU; the wrapper does not provide a CPU painting fallback.

The platform calls the host from one engine/render owner. Native callbacks queue
work for that owner. Thread creation, native widgets, surfaces, frame callbacks and
file access remain platform responsibilities.

## Where to start

- [lib.rs](src/lib.rs) defines `NativeHost`, pointer batches, action dispatch,
  startup coordination and snapshots.
- [renderer.rs](src/renderer.rs) forwards the rendering contract to the attached
  `WgpuRasterizer`.
- The [Android bridge](../../apps/layer-android/native),
  [Apple bridge](../../apps/layer-apple/native) and
  [Windows bridge](../../apps/layer-windows/native) show how hosts use this crate.

Read [platform integration](../../docs/platforms/README.md) for the shared/native
boundary. This is a Rust integration layer; the separate
[`layer-ffi`](../layer-ffi/README.md) package supplies the headless C interface.
