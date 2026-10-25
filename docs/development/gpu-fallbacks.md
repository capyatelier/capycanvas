# GPU capability fallbacks

[Developer guide](README.md) · [Architecture](../architecture.md) · [Performance targets](../PERFORMANCE_TARGETS.md)

Capy Canvas should start and paint on every hardware GPU that meets the WebGPU
downlevel limits. Every host requests the optional rendering features its
adapter offers through `layer_render_wgpu::rendering_features`; the GTK host's
Wayland mailbox requirement is the remaining optional capability that stops
startup. This plan gives each missing feature a fallback. Capable GPUs keep their current path; the fallbacks are as cheap as the
feature allows and approximate only where an exact result costs too much.

## Rules for every fallback

- **Decide once, in shared Rust.** `PipelineDevice` reports what the device can
  do, from the features the device was created with. Layouts, shader assembly and
  pass planning read that report. Hosts request rendering features through
  `rendering_features` and add only timestamp bits their
  adapter has.
- **Capable GPUs compile today's shaders.** A fallback replaces a small shared
  WGSL helper or a pass, chosen when the shader source is assembled, rather than
  branching inside every shader.
- **One implementation per fallback.** Every caller of a capability uses the same
  helper; no per-host or per-feature copies.
- **Tested without special hardware.** Renderer tests create a device without
  the feature, which runs the fallback on any desktop GPU. No environment
  switches or test-only fields in production types.
- **Approximations are bounded.** An approximation states its error bound,
  applies only where the feature is missing, and never changes stored artwork
  formats.

## Who lacks what

| Capability | Missing on | Current behaviour |
| --- | --- | --- |
| Float32 filtering (`FLOAT32_FILTERABLE`) | Apple A9–A16 iPads (Apple's Metal feature tables grant it to Apple9 and later and only some Apple7/8 devices; an iPad mini 6 reports it missing), the iOS simulator, Adreno 6xx/7xx on Qualcomm drivers older than 512.700, Adreno 4xx/5xx, PowerVR, Samsung Xclipse | Texel-load sampling; see [Float32 filtering](#float32-filtering) |
| Float32 blending (`FLOAT32_BLENDABLE`) | Every Mali generation, including the low- and mid-tier reference tablets; Adreno 5xx | Works through the slow `portable_blend` path |
| Android shared presentation (`SharedDemandRefresh`) | Android 13 and probably 10–12: the platform loader adds `VK_EXT_swapchain_maintenance1` and `VK_EXT_surface_maintenance1` only from Android 14. A Galaxy Tab S7 (Adreno 650, Android 13) has `VK_KHR_shared_presentable_image` but neither maintenance extension. `minSdk` is 29 | FIFO ink presentation; see [Android presentation](#android-presentation) |
| Read-write Float32 storage (`TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`) | Unknown; possibly older D3D12 GPUs | `rendering_features` requests it only when both storage formats allow read-write access; otherwise dry media use separate candidates |
| Wayland mailbox presentation | Unknown | The GTK host refuses to start |
| Timestamp queries | Many mobile GPUs, Metal, Web | Already optional |
| Large textures (`max_texture_dimension_2d`) | 8192 on Web and some PowerVR | Already adaptive: the canvas size is capped |

Device data comes from Apple's
[Metal feature set tables](https://developer.apple.com/metal/Metal-Feature-Set-Tables.pdf),
the [Vulkan hardware database](https://vulkan.gpuinfo.org/) and the
[Web3D survey](https://web3dsurvey.com/webgpu). Confirm each row on a device
before relying on it.

The renderer uses no subgroups, f16, texture atomics, immediates, dual-source
blending, MSAA or binding arrays, and every host already requests
`downlevel_defaults()` limits, so bind groups and pipelines are validated
against the same ceilings on every device.

## Float32 filtering

Devices without Float32 filtering read Float32 images through texel loads; the
[rendering guide](../internals/rendering.md#optional-float32-features) describes
the rule. Remaining work:

- Measure the A-series tier on its reference hardware. The texel path costs most
  where `pyramid_sample` interpolates two levels, at the edge of a perspective or
  mesh-warped layer.
- If that tier misses its target, share texels between the four taps of an area
  sample (9 loads instead of 16) before considering approximations. Reduced
  display levels already reach the presenter as `Rgba16Float` copies, which every
  device filters; native-resolution display levels, pages and transform pyramids
  still take the texel path.

## Float32 blending

`portable_blend` renders each blended draw into a scratch texture without
blending, copies the target, then composites both in a compute pass. Its cost
falls on Mali, which is most of the low and mid tiers:

- mask painting repeats a render pass, a copy and a dispatch for every dab, into
  a scratch that is four channels wide for a one-channel mask;
- scene over-draws each take their own pass and blend the whole target, not
  their clip;
- every merge allocates a uniform buffer and a bind group.

**Fallback: accumulate in 16-bit float, merge once.** `R16Float` and
`Rgba16Float` support hardware blending on every WebGPU device. A batch draws
all its dabs, or consecutive over-draws to one target, into a cleared 16-bit
scratch with hardware blending, then one compute pass merges the scratch into
the Float32 target over the batch's damage. Over, erase and max are associative,
so one merge equals blending each dab in turn:

| Mode | Scratch blend | Merge |
| --- | --- | --- |
| Paint | Over | `scratch + target × (1 − scratch.a)` |
| Erase | Over (alpha only) | `target × (1 − scratch.a)` |
| Max | Max | `max(scratch, target)` |

- **Error bound:** the scratch rounds the batch's contribution to 11 significant
  bits (relative error at most 2⁻¹¹), below one 8-bit step. The target and
  stored artwork stay Float32. Only devices without Float32 blending use it.
- **Cost:** one pass and one merge per batch instead of one per dab; masks keep
  a one-channel scratch; merges cover the damage only and reuse one
  dynamic-offset uniform buffer.

## Android presentation

When the surface has no `SharedDemandRefresh`, the Android host presents each ink
frame through FIFO as soon as it is submitted, with a frame latency of one
(`android.rs`). Camera changes use the buffered navigation path, which holds each
frame until its GPU work completes and allows three frames in flight; moving
between the two reconfigures the swapchain, as it does on front-buffer devices.
Ink cannot tear, its update rate follows the display, and stroke prediction hides
part of the added latency.

On a Galaxy Tab S7 (Adreno 650, Android 13, 60 Hz) drawing on a 12 MP photo,
G-Pen, Pencil and Charcoal at 1024 px reach 61.4–67.1 completed updates/s with
completion-gap p99 18.6–28.0 ms, untraced. Under Perfetto, each ink frame waits
27 ms median and 44 ms p99 in the queue before SurfaceFlinger takes it, about one
frame more than shared ink. Presenting ink through the navigation path, as before,
held the same tablet to 20 updates/s. Raising `minSdk` to 34 is not needed.
Remaining work: a low-tier-class GPU on Android 13; the Tab S7 is faster than the
low tier's Mali-G52.

## Smaller fixes

- **Wayland:** fall back to FIFO when mailbox is unavailable.
- **Timestamps:** selection refinement sizes its chunks from GPU timestamps;
  without them, time `on_submitted_work_done` on the CPU instead of keeping the
  initial rate.
- **Memory:** the 512 MiB decoded-tile cache (`pipeline_device.rs`) and the
  256 MiB native colour cache (`raster/native_edit.rs`) are fixed. Size them from
  the device memory budget the display caches already use, after measuring on a
  4 GB tablet.

## Failures that remain

A device that lacks a hard requirement (a hardware adapter, the downlevel
limits) should be told which one, in plain words, in shared localized text.
Today:

- GTK, Windows, Android and Apple explain a software adapter or short limits
  with the shared localized message from `layer_host::admit_adapter` (Apple
  through its own prefix), followed by the cause; other startup failures still
  show the raw English cause under the localized heading;
- the Web page tells every iOS user to update to iPadOS 26
  (`apps/layer-web/gpu.js`), including users already on 26;
- Android titles a failure during painting "Could not initialize canvas";
- the Web reports only out-of-memory after startup, so later validation errors
  reach the console only.

On a Galaxy Tab S6 Lite (Mali-G68) a soft brush inside a selection produced
black regions that spread while smudging, then painting stopped. Publication
(`native_tiles/validity.wgsl`) now stores NaN as zero and clamps alpha and
coverage instead of refusing the edit, so painting continues and each commit
replaces bad working samples. Bad values can still spread within one stroke
before it commits: reproduce on the Mali reference tablets and fix the shader
that produces them.

## Order of work

1. **Diagnostics** from the previous section.
2. **Float32 blending accumulation.** Measure mask painting and composition on
   the low- and mid-tier tablets before and after.
3. **Wayland, timestamps and memory.**
4. **Numeric robustness** of working-page writes.
5. **A-series tier qualification** of the Float32 filtering fallback.

## Not yet verified

- Which Apple7/8 iPads report Float32 filtering; only the iPad mini 6 has been
  observed without it.
- That Android 10–12 lack shared presentation; only one Android 13 tablet has
  been tested.
- Whether any current GPU lacks read-write Float32 storage.
- The cost of each fallback on its tier's reference hardware.
- The cause of the Galaxy Tab S6 Lite failure.
