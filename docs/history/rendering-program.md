# Wgpu rendering program

> Historical design or validation record. Statements about completion and remaining
> work describe the recorded checkpoint. Start with the [current technical guides](../README.md).

## Scope

`layer-render-wgpu` is the only canvas-pixel renderer. It implements
`CanvasRenderer` and owns the canvas wgpu resources. The C ABI benchmark and
export path render offscreen. GTK and browser hosts use the shared viewport
presenter on the same device as the brush engine: a Vulkan Wayland swapchain for GTK and a
WebGPU surface for the browser. Future native hosts supply Metal/D3D12 surfaces.
No wgpu type enters `layer-core`, `layer-render`, `layer-engine`, or `layer-ui`.

The implemented scope is normal premultiplied-alpha paint layers, opacity and
visibility, solid and textured dry brushes, destination-aware smudge/wet/blend
and deformation brushes, uniform accumulation, spatial brush-reservoir pickup,
layer-wide watercolor with event-driven capillary transport and live edges,
optional post-stroke edges, deposited
non-watercolor canvas wetness, paint, erase, incremental composition, sparse
predicted preview storage, and explicit readback.

## Ownership

The renderer owns:

- `Instance`, hardware `Adapter`, `Device`, and `Queue`;
- persistent 256×256 `RGBA8Unorm` pages for touched paint regions;
- sparse double-buffered R8 stroke-coverage pages, sparse R8 wetness pages, and
  one logical sparse R8 watercolor-wetness channel with ping-pong storage;
- one double-buffered 64×64 RGBA8 spatial wet-brush reservoir;
- sparse predicted-preview pages and one document-sized composite texture;
- R8 tip textures and a shared filtering sampler;
- all bind-group layouts, pipelines, upload buffers, and submission ordering;
- one renderer-private `BrushPassPlan` that derives raster family, material
  operation, state attachments, reservoir use, and edge work from each frozen
  batch style.

`WgpuRasterizer::from_wgpu` accepts a platform-selected adapter, device, and
queue. A native presenter can therefore select an adapter compatible with its
surface and give the canvas the same GPU context; it does not copy canvas pixels
across APIs or devices.

The engine lends one `FramePacket` for a synchronous submission call. The
renderer copies only dabs and the aligned style table before the call returns.
Layer pixels never enter CPU memory unless an explicit export asks for its
finished byte stream.

## Resource layout

| Resource | Lifetime | Use |
| --- | --- | --- |
| Paint page | touched-region lifetime | Persistent premultiplied linear pixels |
| Stroke-coverage page pair | touched-region lifetime | Current-stroke uniform accumulation; watercolor deposition state |
| Canvas-wetness page | touched-region lifetime | Maximum deposited wetness for non-watercolor wet brushes |
| Watercolor-wetness page pair | watercolor-touched-region lifetime | Persistent material floor, local water amount, transport, and live edges |
| Wet-brush reservoir pair | renderer lifetime | Spatial loaded paint and amount for oils/gouache |
| Preview page | renderer lifetime, reused by coordinate | Replaceable predicted contacts |
| Preview-coverage page pair | renderer lifetime, recycled to current damage | Exact speculative uniform/watercolor coverage |
| Preview watercolor-wetness page pair | renderer lifetime, recycled to current damage | Private speculative water and material identity |
| Composite texture | document lifetime | Incremental visible result and presentation source |
| Tip texture | asset lifetime | Filterable R8 contact coverage |
| Dab vertex buffer | renderer lifetime, grow-only | Contiguous fixed-size contacts |
| Style uniform buffer | renderer lifetime, grow-only | Aligned batch/layer records |
| Target uniform buffer | document lifetime | Page origins/extents without duplicating contacts |
| sRGB export target | explicit export call | GPU transfer encoding before readback |

One 4K `RGBA8Unorm` layer would consume 64 MiB even when empty. The renderer
therefore allocates standalone pages only when a contact touches them. Empty
layers allocate no pixel storage. A standalone texture per page keeps addressing
and shader work simple; an atlas is warranted only if traces later show page
binding or allocation overhead.

## Prepared pipelines

- analytic paint;
- analytic erase;
- R8-mask paint;
- R8-mask erase;
- grain and dual-tip paint;
- grain and dual-tip erase;
- five destination-aware target-layout variants: color; color+coverage;
  color+wetness; color+coverage+wetness; and watercolor
  color+coverage+wetness;
- spatial brush-reservoir exchange, live watercolor composition, and optional
  post-stroke edge;
- bounded event-driven watercolor/ink capillary transport;
- inverse-mapped liquify deformation;
- background replacement;
- premultiplied layer composition;
- straight-alpha sRGB export conversion.

All are created with the device. The dry brush vertex shader expands contact
quads. Fragment shaders produce premultiplied coverage. Paint and erase use
fixed-function blending.

## Frame encoding

One `submit` call:

1. Reconciles the authoritative layer list and creates/releases textures only
   when structure changes.
2. Grows upload buffers only when packet capacity exceeds their high-water mark.
3. Uploads all new contacts once and all batch/layer styles once.
4. Clears paint textures only for an explicit rebuild.
5. Draws each non-empty batch into its persistent layer using its damage
   scissor. Stateful batches update only enabled attachments; spatial wet
   batches exchange the reservoir. Watercolor recharges nonuniform wetness from
   stroke-uniform coverage and runs three brush-configured coarse-to-fine
   capillary stages after the
   update's internal microbatches. Watercolor edges are part of live
   composition; configured
   legacy edges finalize when another brush's stroke ends.
6. Applies predicted work to replaceable GPU state. Topmost source-over paint
   draws directly after composition; one destination-aware batch reads committed
   pages directly and writes only preview damage; exact erase and multi-batch
   cases copy only the damaged GPU region they require.
7. Clears the dirty composite rectangle to the background, then draws visible
   layers back-to-front with opacity.
8. Submits one command buffer and returns without polling or waiting.

Persistent and predicted batches enter the same `encode_brush_batch` scheduler.
The target policy selects committed pages, copied preview pages, or a preview
written directly from committed state; it does not duplicate brush-family or
watercolor-lifecycle logic.

The composite texture persists because presentation surfaces do not preserve
prior contents. A future surface pass samples it into the newly acquired surface
texture; pan/zoom applies the document-to-surface transform in that pass without
rerasterizing contacts.

## Stateful brush extension

Destination-aware stages live in this crate. They read immutable source color
and optional material views and write destination views, then swap them.
Dry contacts keep the current render/blend fast path. The material stage has
four generic target layouts—color alone, color+coverage, color+wetness, and all
three—plus the watercolor state layout. An enabled feature writes only its
required attachments. `BrushPassPlan` performs feature selection once per
batch; allocation, preview, encoding, and style upload consume that same plan.
See
[../reference/gpu-brush-engine.md](../reference/gpu-brush-engine.md).

WGSL is composed only where code is genuinely shared. `brush_coverage.wgsl`
defines the analytic/mask, grain, and dual-tip coverage functions used by both
the textured direct shader and the destination-aware material shader. The
material shader remains one typed-operation program, so sharing does not create
a preset-dependent shader-permutation system.

Smudge/blender uses distance-normalized, deterministic backtrace chunks of at
most three contacts without a reservoir. The incomplete live chunk uses private
preview pages until its stable commit boundary. Loaded wet paint owns a 64×64
spatial reservoir and advances it for every contact in microbatches of at most
three. Watercolor uses the same
destination material pipeline with stroke coverage and an R8 wetness attachment
but no reservoir. Its brush-owned, document-anchored conductance texture gates
three gradient/curvature-aligned wetness/pigment stages with independent
wet-to-wet and wet-to-dry rates; the layer then uses wetness-driven live
morphology during composition. Only the first stage copies the ping-pong page; later stages
overwrite the same scissor directly.
Liquify uses ordered inverse mapping with bilinear sampling. These details
remain renderer-private.

## Synchronization

Production `submit` never calls `Device::poll`, waits on a fence, or maps a
buffer. The platform limits queued presentation frames and records
input-to-present externally.

`wait_idle` exists only for serialized completed-work benchmarks and tests.
Explicit export samples the linear composite through a GPU pass into an sRGB
target, copies that target into an aligned `MAP_READ` buffer, and waits outside
the drawing path. Host code only removes transfer-row padding and copies the
finished sRGB bytes to the caller.

## CPU boundary

CPU work ends before any canvas pixel is evaluated. It covers input
normalization, dynamics, contact placement, document/UI state, damage bounds,
resource bookkeeping, and command encoding. Brush coverage, paint, erase,
preview, layer composition, viewport sampling, presentation, reservoir
exchange, wet transfer, and state deposition are GPU-only.

## Failure policy

- No compatible hardware adapter or device creation failure prevents painting;
  there is no software rendering mode.
- Surface loss will be handled by the platform presenter and does not alter
  persistent document state.
- Device loss recreates renderer state and asks the engine for one full replay.
- Out-of-memory is fatal and returns a renderer error.
- Zero-sized native surfaces suspend presentation, not document rendering.

## Next implementation slice

1. Add platform-owned surface presentation without changing the offscreen
   renderer or shared engine.
2. Add GPU timestamp queries and input-to-present tracing in the Linux client.
3. Add RGBA imported/AI assets.
4. Test the implemented spatial reservoir, capillary fields, uniform coverage,
   live watercolor edge, and non-watercolor wetness controls with painters
   before adding time-evolving material physics.

## References

- [wgpu project and supported backends](https://github.com/gfx-rs/wgpu)
- [wgpu queue submission](https://docs.rs/wgpu/latest/wgpu/struct.Queue.html#method.submit)
- [wgpu device polling](https://docs.rs/wgpu/latest/wgpu/struct.Device.html#method.poll)
- [wgpu texture formats](https://docs.rs/wgpu/latest/wgpu/enum.TextureFormat.html)
