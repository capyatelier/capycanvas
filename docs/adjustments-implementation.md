# Filters and programmable effects

Implemented design, 2026-09-09. Builds on
[the supplied shader research](non_destructive_filters_wgsl_shader_subsystem.md)
and the existing tiled compositor, not a second rendering engine.

## Contract

- An adjustment transforms the composite below it within its isolated group.
  A clipped adjustment instead transforms the current clipping stack before
  that stack is composited onto the backdrop. Chained clipped adjustments retain
  the base alpha; they must not pick up unrelated layers underneath.
- Opacity and mask coverage interpolate between the original and adjusted
  result. They do not source-over a second copy and increase alpha. Blend modes
  blend adjusted RGB with original RGB within that same coverage.
- Built-ins preserve alpha. A generator produces content; it uses ordinary layer
  compositing rather than adjustment replacement. The two modes share the effect
  parameter and shader execution contract.
- Shader boundaries use premultiplied linear RGBA. Artistic RGB tone controls
  explicitly convert to/from sRGB inside their shader when appropriate. Zero
  alpha remains zero. The current document is SDR; this is not an HDR promise.
- Parameters, defaults, ranges, section headings, choice labels, curve points and gradient stops
  are document/core data. Hosts render reusable control types, not per-filter UI.
  Consecutive fields in the same section share a heading; section changes add a
  subtle divider. Color Balance uses Shadows/Midtones/Highlights with short color
  pairs; Levels uses Input/Output. Small filters stay ungrouped. Ordered numeric
  constraints (such as Levels input endpoints) are enforced in Rust.

Adobe's adjustment workflow likewise separates adjustment creation, Properties,
masking and clipping to a source layer: [adjustment scope](https://www.adobe.com/learn/photoshop/web/adjustment-layer),
[creation and editing](https://www.adobe.com/learn/photoshop/web/edit-photos-adjustment-layers).
PixiEditor's [Shader node](https://pixieditor.net/docs/usage/node-graph/nodes/effects/shader/)
and [property sockets](https://pixieditor.net/docs/handbook/node-graph/property-sockets/)
support separating executable shader code from editable inputs. Capy uses WGSL,
not PixiEditor's SkSL, and does not copy its implementation.

## Execution and performance

Keep one host-owned WGSL runtime with a versioned ABI. Built-in implementations
are shader programs, not a per-pixel Rust callback or a separate CPU filter path.
The initial ten effects are pointwise, so dirty regions need no neighborhood
expansion. Future neighborhood/multipass effects must declare their footprint
and execution requirements; unsupported capabilities are rejected explicitly.

Cache pipelines by program identity; parameter edits update data, not code.
Consecutive compatible pointwise adjustments share a generated fragment
invocation without intermediate texture round-trips. Source-over before an
effect and final clipping-stack composition can be folded into that invocation.
Tile draws targeting the same attachment share a render pass.

Aligned R8 masks bind directly to the fused chain. Fourteen ordinary mask texture
bindings plus two scene inputs fit WebGPU's guaranteed sixteen sampled textures;
no native-only binding arrays are required. Split before a masked effect would
exceed those inputs. Missing mask tiles use the mask's declared coverage, not a
new image. Translated masks and isolated groups retain tile operations where
required. Each effect still applies its own mask, opacity and blend in order.
Reuse tile-sized scratch surfaces. Do not allocate a full-document image per
effect or read canvas pixels back to the CPU. Unchanged documents retain their
cached composite during navigation. Curve/gradient lookup preparation is
parameter processing and happens only when parameters change.

Use normal checked [wgpu shader creation](https://docs.rs/wgpu/30.0.1/wgpu/struct.Device.html#method.create_shader_module).
No native bytecode, unsafe shader passthrough, arbitrary bindings, native plugins,
or document-supplied pipeline caches. Cache compiled objects in memory; optional
driver [pipeline caches](https://docs.rs/wgpu/30.0.1/wgpu/struct.PipelineCache.html)
are not a portable requirement. A native graphics driver is not an RCE sandbox.
The shader editor, untrusted document execution policy, node builder, spatial
filters and temporal state remain later features, with explicit ABI evolution.

## Panels and built-ins

1. Core effect model, shared parameter schema and GPU runtime; first five:
   Curves, Levels, Brightness/Contrast, Hue/Saturation, Color Balance.
2. GTK, web and Android Filters, Properties and Stats for nerds panels. Filters
   and Properties follow Layers in the default tab group. Choosing a filter
   inserts above the editing target. Reveal Properties only when it is not
   already displayed elsewhere; restore/insert the panel through shared docking
   actions. Labeled adjustment tiles use three standard tool cells in width and
   two in height, wrapping to available panel width.
3. Exposure, Vibrance, Black & White, Gradient Map and Posterize use the same
   registry and controls as the first five. No host-specific filter algorithms.

| Filter | Controls / operation |
|---|---|
| Curves | Master and R/G/B curves; monotone cubic interpolation, parameter-time LUT |
| Levels | Input black/white, gamma, output black/white |
| Brightness / Contrast | Tone offset and contrast around mid-gray |
| Hue / Saturation | Hue, saturation and lightness |
| Color Balance | Opposing color pairs in three tonal ranges; preserve luminosity |
| Exposure | Linear-light exposure in EV, offset and gamma |
| Vibrance | Chroma-aware saturation, saturation, skin-tone protection |
| Black & White | Six hue contributions, optional tint |
| Gradient Map | Editable color/alpha stops, reverse and strength |
| Posterize | Discrete levels per RGB channel |

These are Capy's algorithms, not a claim of byte-identical Photoshop behavior.
The reusable curve/gradient controls edit Rust-owned points and interpolation.
Numeric controls retain expressions, ranges and resets.

Stats for nerds is an optional compact panel. Metrics distinguish CPU preparation,
supported GPU execution timestamps, drawing-update and dab counts, effect pass
and pipeline counts, and tracked renderer-owned memory. Unsupported metrics are unavailable, never
zero. Summaries/charts are bounded and refreshed at UI cadence, not per sample.
No synchronous GPU waits or full-image readback for telemetry. GPU timers may be
unavailable or quantized in browsers; see [WebGPU timing considerations](https://gpuweb.github.io/gpuweb/#security-timing).
Renderer timings are not display-compositor FPS or input-to-photon latency.

## Verification gates

- Identity and known-color results for every effect; alpha preservation and
  transparent input; black/white endpoints; curve and gradient interpolation.
- Clipped chains over translucent paint, masked adjustments, group isolation,
  opacity and blend modes, hidden effects, undo/redo and cache invalidation.
- Multi-tile seams and incremental brush edits match full recomposition.
- Generic shader and generator tests use the same runtime as the built-ins.
- Each host tests insertion/reveal, dynamic editing and live stats. Shared GPU
  regressions cover masks/clipping; existing host layer tests cover their controls.
- Benchmark baseline, each effect and stacked chains on small brush damage and
  full 2K/4K damage. Report median/p95/p99, cold compilation separately, and
  telemetry overhead. “Negligible” is a measured outcome, not assumed merely
  because work is on the GPU; full-canvas changes cannot have zero cost.

See [validation results](adjustments-validation.md) for measurements, captures
and remaining physical-device/performance limitations.
