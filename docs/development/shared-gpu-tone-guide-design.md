# Shared GPU illumination guide design

2026-09-19. Investigation and proposed implementation scope, based on
`cb5775699a0ddcf97d13fcd66e427c9fc3b4c739`. No production implementation changed.
Continues the [GTK investigation](gtk-incremental-proof-investigation.md).

This document preserves the original design/baseline. See the subsequent
[GTK GPU guide milestone](gtk-gpu-tone-guide-milestone.md) for implemented scope
and measurements. The implementation incorporates `origin/main` at `7c426431`,
including Windows' newer HDR/local-tone host, before applying the GTK changes.
Platform entries below describe the earlier inspected revision.

**Recommendation:** implement one production guide builder in
`layer-render-wgpu`, using portable WGSL compute shaders and a shared Rust job
controller. All hosts and GPU snapshot/export paths should call it. Keep the
previous compatible guide during drawing and rebuild after idle. A full GPU
guide rebuild is the first milestone; incremental source statistics and
pyramid updates can follow measured need. Keep the current CPU algorithm as a
numerical reference, rather than a second host-selected production algorithm.

The hosts already select the necessary wgpu backends:

| Host | Current backend | Current illumination ownership / migration |
| --- | --- | --- |
| GTK | Vulkan | `local_tone_view.rs` starts a snapshot/CPU worker and sends CPU guide samples to the render thread. Replace this with shared job requests and a GPU guide handle. |
| Android | Vulkan | Native `hdr.rs` and Kotlin `HdrController` own snapshot/CPU work, cancellation and publication. Retain lifecycle integration, move algorithm and job policy into shared Rust. |
| Web | Browser WebGPU | Rust `hdr.rs` reads source bands back, reduces them on the CPU, and sends reduced samples to a Wasm worker for pyramid analysis. Replace this analysis transport with shared GPU jobs; keep event-loop yielding. |
| macOS / iPadOS | Metal | The native host already uses the shared renderer/presenter. No equivalent local-guide owner is wired in the inspected host; add the common request/status/publication integration. HDR export currently returns an explicit unsupported error. |
| Windows | D3D12 | The native host already uses the shared renderer/presenter. No equivalent local-guide owner is wired in the inspected host; add common integration. HDR export currently returns an explicit unsupported error. |

These backend selections are visible in
[GTK](../../apps/layer-linux/src/render_thread.rs),
[Android](../../apps/layer-android/native/src/android.rs),
[Web](../../apps/layer-web/src/lib.rs),
[Apple](../../apps/layer-apple/native/src/metal.rs), and
[Windows](../../apps/layer-windows/native/src/host.rs).
They correspond to the supported [wgpu backends](https://wgpu.rs/doc/wgpu/enum.Backend.html).
Sharing the algorithm does not by itself enable each host's complete HDR UI,
file workflows or native HDR display transport. SDR proofing of HDR content
does not require an HDR presentation surface. The current browser HDR admission
ceiling of 12 MP remains a separate measured resource policy.

**Library boundaries.**

| Library | Responsibility |
| --- | --- |
| `layer-core` | Recipe validation, dimensions, color matrices/luminance weights, analysis constants and plain metadata. CPU numerical oracle for verification. No GPU device types. |
| `layer-render-wgpu` | The WGSL kernels, pipeline cache, scratch allocation, source reduction, GPU guide resource, bounded job progression and GPU consumers. The same implementation serves live composition and snapshot capture. |
| `layer-ui` / `layer-render` | Shared request identity, compatibility, idle/publication policy, progress/status and renderer request interface. Keep GPU handles out of the generic UI contract. |
| `layer-color` | CPU profile transforms, pixel encoding, codecs and delivery orchestration inputs. Accept an explicitly prepared guide or already tone-mapped rows; stop implicitly choosing CPU analysis for production export. |
| Hosts | Native input/idle observations, waking their render owner, surface/device lifetime, application suspension and user-visible status. No host-specific illumination math. |

Do not add `layer-render-wgpu` as a dependency of `layer-color`: the renderer
already depends on `layer-color`. GPU preparation belongs above that boundary.
The CPU-only library's standalone callers need an explicit prepared-analysis
input, or orchestration by a GPU-capable caller. Tests and diagnostic examples
can call the reference implementation deliberately. Supporting a production
CPU-only runtime would be a separate fallback policy and would no longer mean
one production GPU builder.

**Shared resources and API shape.**

Introduce a GPU-owned guide carrying its document extent, guide extent,
working space, source revision, device generation and analysis version. Its
storage buffer can retain the current presenter layout: a four-u32 header and
four Float32 values per guide cell. Peak/range/status metadata can live in a
small separate buffer. A completed guide is immutable to its consumers;
pending analysis must use different storage from the displayed guide.

Conceptually, the shared API needs operations to begin analysis from an
immutable source identity, encode a bounded amount of work, observe progress,
cancel future work, obtain the ready GPU guide, and optionally download the
completed guide for CPU consumers. The presenter must bind this GPU resource
directly. Its current API only accepts `Arc<LocalToneGuide>` with a CPU `Vec`,
then allocates and uploads a new buffer. Canvas and Navigator should share the
same GPU resource on a device, as they already share uploaded viewing data.

The state machine should separate the guide currently displayed from the
desired artwork generation and the pending job. Reuse a compatible displayed
guide across ordinary edits. Retire it on incompatible document/color/extent
or device changes. Start analysis after stroke completion and a short quiet
period; defer publication if input resumes, and reject superseded candidates.
Keep the most recent usable guide on failure while reporting that refinement
failed. Device loss clears device-owned resources and requires rebuilding.

An immutable analysis input is essential even when final publication waits for
idle. A live composite can change between GPU job chunks. Either use immutable
snapshot inputs or freeze the small reduced-statistics buffer for a job and
track its source generation. Never combine samples from several artwork
revisions into a published guide. The existing shared `ToneKey` and submission
cache-validity guards provide useful building blocks.

**GPU stages.**

Preserve the current algorithm before experimenting with different appearances:

1. **Source reduction.** Consume linear-premultiplied Float32 artwork tiles
   before overlays. Compute working-space luminance, log2 with the existing
   floor, coverage-weighted area sums and the source luminance peak. Preserve
   exact guide-cell footprints, partial edges and transparent-pixel semantics.
   Use per-tile partial contributions followed by a gather/reduction, or gather
   complete guide-cell footprints. Adjacent tiles can contribute to the same
   cell, so they cannot independently overwrite its total. Existing averaged
   RGB display mips are not the same statistic.
2. **Normalize and find range.** Construct the guide's log-luminance/coverage
   plane and reduce its occupied minimum/maximum. Derive the same adaptive
   half-stop-or-finer intensity anchors. The scheduling choice is GPU-generated
   indirect dispatch metadata or a tiny asynchronous metadata readback; neither
   needs a full artwork readback. Validate limits for Float32 documents as well
   as Float16, rather than imposing a half-float-derived anchor cap.
3. **Original pyramid.** Implement the current horizontal/vertical five-tap
   coverage-normalized reduction down to 1×1, including odd dimensions and
   clamped boundaries.
4. **Remapped pyramids and detail.** For each anchor, remap log luminance, build
   its pyramid and accumulate the same weighted Laplacian coefficients.
   Stream one or a bounded batch of anchors through reusable scratch. Keeping
   all anchor pyramids resident would multiply memory by the occupied range.
5. **Reconstruction.** Expand accumulated detail from coarse to fine, preserving
   coverage-aware interpolation, then write original log luminance,
   illumination, coverage and the reserved component to the guide buffer.
6. **Consumption.** Bind the guide to the existing spatial gather and SDR mapper
   in `hdr_view.wgsl` / `hdr_mapping.wgsl`. Print proof continues with the cached
   ICC LUT. It does not need a new LUT each time illumination changes.

These are multiple dispatches with shared kernels, not necessarily six shader
files. Separate passes provide the global ordering required between pyramid
levels. Shader module/pipeline preparation should use the existing renderer
startup/cache mechanisms instead of compiling on the first pen-up.

There are two source providers, with one analysis implementation:

- Live composition can feed retained tile statistics when final artwork tiles
  are produced. Handle dense/direct composition shortcuts, effect-expanded
  damage, preview removal, offscreen changes and the mask-area tint boundary.
  This avoids recomposing the whole document after every stroke.
- Snapshot/export capture needs an operation that exposes the composed GPU
  region to analysis before readback. Today `prepare_region` couples composition
  to allocation of a readback buffer and `copy_texture_to_buffer`; split those
  stages. A first GPU prototype can process every immutable snapshot tile and
  already eliminate guide-related full-pixel readback, even before live tile
  reuse is complete.

**Portability and scheduling.**

Use Float32 storage buffers, explicit texture loads, ordinary compute workgroups
and staged reductions. Do not require subgroups, Float16 arithmetic, floating
atomics, native-only texture access or hardware filtering of Float32 textures
for the new analysis. WGSL's portable atomic types are integer types; the
vendored wgpu Float32-atomic extension is explicitly native-only and does not
cover every target. See the [WGSL atomic type specification](https://www.w3.org/TR/WGSL/#atomic-types)
and `vendor/wgpu-types/src/features.rs`. Workgroup sizes and pass batches can be
tuned through shared configuration without introducing separate algorithms.

No browser API may block waiting for GPU completion. Use the same encode/poll
job progression on native and Wasm, with host-appropriate completion delivery.
The browser GPU device stays with its existing owner; GPU guide handles should
not be serialized through the current CPU proof worker. The ICC worker can
remain, because preparing a profile LUT is a different workload.

Bound queued work, not just allocations. A CPU worker submitting GPU work does
not make that work independent of drawing: it shares the GPU queue. Cancellation
can stop future batches and suppress publication, but cannot retract submitted
commands. Submit small measured batches and yield before queuing another when
input resumes. Respect the existing Metal submission/pass chunking helper.
Mobile and browser jobs may use smaller budgets while computing the same result.

A maximum square output guide is approximately 9 MiB. Retaining old and new
guides requires approximately 18 MiB before sums, original/remapped pyramids,
detail, partial-tile statistics and staging. Explicitly plan and measure all
scratch allocations on mobile; do not assume guide size equals job memory.
Reuse allocated scratch across jobs and include multi-document jobs in the
device budget. The first full GPU implementation still has image-wide work;
it does not imply current illumination at 120 Hz during every stroke.

**Making every consumer use the same implementation.**

The live host replacements alone are insufficient. Native
`SnapshotRenderer::local_tone_guide`, snapshot previews, streaming SDR export,
gain-map base generation, Web `preview_document`, and Web output workers all
have CPU guide-building callers today. Migrate them explicitly.

For one unified **guide builder**, export can download the finished GPU guide
once and pass those samples to the existing CPU row mapper/ICC/codec pipeline.
That is at most roughly 9 MiB of guide data, rather than reading all artwork
pixels solely to analyze them. Export still reads output pixels for CPU codecs;
the migration cannot remove that separate transfer. Browser file workers need
the prepared guide in their job payload and must stop rebuilding it from their
spooled pixel rows. This is the smaller first delivery boundary.

If the intended end state also includes **one production SDR tone-mapping
implementation**, reuse the WGSL mapping functions in a bounded GPU output pass
for snapshots and previews, then read the mapped rows for CPU encoding. That
requires additional export changes: honor resize-before-map semantics, document
coordinates, matte/profile order, clipping/statistics and HDR-master bypass.
Gain-map generation must receive the actual delivered SDR base and preserve
the current post-codec base handling. It is additional scope beyond moving
guide construction, but the existing shaders are suitable shared source.

CPU profile preparation, ICC delivery and image codecs can remain shared Rust
code. Their existence does not require a second illumination algorithm. Keep
an independent CPU oracle for tests regardless of whether final per-pixel
mapping also migrates. Cross-GPU floating-point identity is not a reasonable
acceptance condition: reductions change accumulation order, and WGSL permits
implementation-dependent floating-point behavior within its numerical rules.
Use specified numerical/appearance tolerances and exact artwork preservation.
See [WGSL floating-point evaluation](https://www.w3.org/TR/WGSL/#floating-point-evaluation).

**Implementation sequence and evidence required.**

1. Define the guide/resource contract, source identity, shared retention and
   publication policy, job budgets and stage-by-stage CPU comparison fixtures.
2. Implement the shared kernels and GPU-region input path. Measure a complete
   bounded-guide rebuild on GTK, keeping the previous guide visible. Verify
   there is no guide-related artwork pixel readback and no mandatory UI wait.
3. Feed analysis from live composition tile statistics to remove duplicate
   full-document capture. Start by rebuilding the entire small pyramid after
   idle; optimize per-level invalidation only if measurements justify it.
4. Migrate native snapshots/export, Web live and file workers, and Android to
   the same builder. Add Apple/Windows owner integration and qualify their
   applicable HDR workflows separately. Remove production CPU-builder call
   sites only after all consumers are migrated.
5. If desired for the wider unified pipeline, migrate snapshot/output tone
   application to the same WGSL mapper. Keep ICC/codec work at its existing
   boundary and verify delivered pixels/gain-map reconstruction.

The algorithm is a bounded renderer subsystem with host and export migration,
not a one-shader substitution. Its most delicate work is exact source
semantics, queue scheduling, lifetime/version correctness and output parity.
The first prototype should settle performance uncertainty before estimating
the remaining optimization work in calendar time.

Validation must compare reduction, pyramid/detail stages and final guides
against the CPU reference, then compare presented and exported results. Cover
all working spaces, Float16/Float32, zero/tiny/fractional alpha, hidden RGB,
negative channels, broad HDR ranges, constant/all-transparent inputs, odd
dimensions and tile boundaries. Exercise erase/Undo, transforms/effects,
rapid new strokes, cancellation during every stage, document replacement,
device loss, background suspension and multi-document resource pressure.

Run the same numerical fixtures on Vulkan, Metal, D3D12 and actual browser
WebGPU. Include real Android and iPad hardware for memory, driver and sustained
load behavior. Record per-stage GPU times, host event-loop gaps, guide age,
input-to-current-guide and input-to-present latency, missed refresh slots and
peak combined memory. A 4K120 draw test must include actual changing HDR artwork;
the existing retained-guide Proof-dial test does not establish this workload.

The desired first outcome is stable interactive proof with asynchronous GPU
refinement after drawing. The same shaders, core color definitions and Rust
analysis implementation can serve every platform; hosts retain only their
native scheduling and lifetime responsibilities.
