# Pinned dependency fixes

Vendor a crate only when a required fix cannot wait for upstream. Every vendored
crate keeps its original licences, its registry archive SHA-256 and the upstream
revision it came from. Each change is a named `.patch` described below. Remove
the vendored copy once an upstream release contains the fix.

## Portable HEIF/HEVC decoding

`rust_h265` 0.1.0 is the published MIT OR Apache-2.0 crate. Its decoder sources,
test fixtures, original licenses and registry provenance are retained. The
application calls it directly; no heif-oxide runtime crate remains. Unused
examples (including the minifb development dependency),
package lockfiles and registry cache markers are omitted.

| Crate | Upstream revision | Registry archive SHA-256 |
| --- | --- | --- |
| [rust_h265](https://github.com/roticv/rust_h265) | `e51348807a685b00343212a77e13d32692954321` | `dde60f5842f27ed06f1d84844cacd93d1a159f606365b30a5594c771e6b4eb17` |

`heif-portable.patch` exposes a bounded still-picture decoder that returns source
YUV and VUI color/chroma metadata. It admits coded dimensions and estimated
picture working memory before allocation, bounds parameter-set syntax, rejects
truncated header reads, and borrows cancellation callbacks at NAL, coding-tree
and filter boundaries. Independently coded stills cannot consume external
reference pictures or silently return an incomplete frame. The HEVC prediction,
transform and filtering algorithms are unchanged.

The application uses its shared BMFF, grid, ICC, geometry and source-storage
pipeline around this API. Its HEVC adapter borrows hvcC parameter sets and builds
Annex B only after memory admission. Grids decode one tile at a time, also on
Wasm. Memory estimates are conservative
admission checks, not a hard allocator quota. Individual in-loop filters remain
synchronous between cancellation checks.

The independent HEIC box writer and four synthetic streams from heif-oxide 0.1.0
remain as a small [test-only extract](heif-test-support/README.md), retaining
both licenses, revision/archive provenance and its picture-handler patch. They
are not a Cargo dependency and are not part of application decoding.

Original migration verification included the upstream suites (128 HEVC and 35
HEIF tests), exact
libde265 YUV comparison of a photographic still, shared source/ICC/grid/alpha
tests, Chrome execution and GTK Open/Import/Paste with an empty codec directory.
HEIC support and its limits are described in the
[portable colour guide](../docs/internals/portable-color.md).

Run the isolated vendor tests with:

```sh
cargo test --offline --release --manifest-path vendor/rust_h265/Cargo.toml --lib
cargo test --locked --offline --release -p layer-color --lib photo::avif_io::hevc_tests
```

## AV1 decoder portability

`rav1d` 1.1.0 is the published BSD-2-Clause crate from
<https://github.com/memorysafety/rav1d>, revision
`782dab2135ea64a057c097088a13eb8ed3cc3320`, registry archive SHA-256
`1932f060d5e7bd49dc9f8b272c1dc5e9ce0ffe141c28be900265d3989b36c9ed`.
The Rust library sources, manifest, license, release notes and registry provenance
are retained. Unused `.asm`/`.S` sources (7.94 MiB), their build implementation,
development CI/configuration files and package lockfiles are omitted.

`rav1d-portable.patch` removes `cc`/`nasm-rs`, defaults to the two Rust bit-depth
features, and makes assembly opt-in fail explicitly in the small build guard.
The `asm` feature name and its aliases remain only to diagnose accidental feature
unification; this snapshot cannot build assembly. The patch records source and
manifest edits; omission of the 91 upstream `.asm`/`.S` files is intentional and
is not repeated as megabytes of deletion text in the patch.
Pointer-sized C integer aliases use the matching Rust primitives.
The native `off_t` and errno values remain from libc; `wasm32-unknown-unknown`
uses an i64 offset and conventional Linux result codes without a libc dependency
or syscalls. The public error enum lets callers match target-correct EAGAIN
without hard-coding Unix errno values. The AV1 decoding algorithm is unchanged.
Both bit-depth features are enabled and default/assembly features disabled by
the application.

The patch also removes unused function-pointer equality derives and an obsolete
pending-task struct, makes three borrowed return lifetimes explicit, and builds
the overlap helper only for debug assertions or tests. This keeps release builds
warning-free without suppressing lints or changing decoding behavior.

## WebP entropy-table admission

`image-webp` is the published 0.2.4 crate, retaining its MIT/Apache licenses.
Registry archive SHA-256:
`525e9ff3e1a4be2fbea1fdf0e98686a6d98b4d8f937e1bf7402245af1909e8c3`.
Upstream: <https://github.com/image-rs/image-webp>.

The original decoder's memory limit applies to metadata but leaves entropy-table
allocations unbounded by that limit. `image-webp-memory.patch` propagates it to
lossless still, animation and compressed-alpha decoding, admits the group vector
before allocation, and accounts retained Huffman tree/table capacities. One
temporary tree is built before its capacity is known; the photo reader reserves
1 MiB for that bounded temporary and small codec state.

The photo adapter separately validates outer/inner frame dimensions and admits
encoded data, full-frame buffers, transform workspaces and source packing bands.
It assigns a quarter of the codec budget to entropy tables. This is explicit
codec admission, not an operating-system process memory limit. Regression:
`cargo test --locked --offline -p layer-color --lib photo::raster_tests`.

## wgpu platform fixes

These are the published `wgpu`, `wgpu-hal` and `wgpu-types` 30.0.1 crates,
upstream revision `40f4a34ebaf56f9a046231f54125ad046239d3f3`. The crate archives'
`.cargo_vcs_info.json` and MIT/Apache licenses are retained. Cargo's generated
cache marker, per-package lockfiles and original unnormalized manifests are
omitted. The workspace lockfile pins all other dependencies.

Registry archive SHA-256 values from the previous workspace lockfile:

| Crate | SHA-256 |
| --- | --- |
| wgpu | `527ccdf43dd5b2e8676eed9984ce00e2bbb0a1b85b70c1969dcb6cd2eb55ab9e` |
| wgpu-hal | `b6b7fb58561a792bc237628ba0792e332de418fefe145f13b5ed8201e6d52f58` |
| wgpu-types | `99dad6f1fbdbbdb4c278a6508b059d44688f5cebddf78d005a46a31340269286` |

The Wayland patch exposes `SurfaceColorSpace::PassThrough` and its capability bit,
maps them to `VK_COLOR_SPACE_PASS_THROUGH_EXT`, and rejects the new choice on
backends that cannot provide it. `Auto` behavior is unchanged. The Vulkan mapping
round-trip test includes pass-through. `wgpu-color-passthrough.patch` records every
Wayland source change relative to the published crates.

The GTK host uses this to own an explicit Wayland image description with the
piecewise sRGB curve or an equivalent ICC profile. The driver's legacy sRGB
description is ambiguous and Mutter 50.4 interprets it as gamma 2.2. The
[Vulkan specification](https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html)
defines pass-through as the way for a Wayland application to own that description.
wgpu still owns swapchain creation, acquisition, synchronization and presentation.

No other platform host enables this path. Do not replace the explicit description
with a compositor-specific gamma adjustment.

`wgpu-metal-float32.patch` corrects the Metal format capabilities independently.
The adapter advertises `FLOAT32_BLENDABLE`, but its `Rgba32Float` format omitted
blending on iPadOS; enabling adapter-specific formats therefore rejected the
SDR composition pipeline on a physical M4 iPad. The format now includes blending,
and R32/RG32/RGBA32 Float filtering/resolve follow the existing device capability
query instead of a macOS-only condition. This agrees with Apple's
[Metal feature tables](https://developer.apple.com/metal/capabilities/), including
the `supports32BitFloatFiltering` qualification for older iPad GPU families.
Devices without that capability still report it unavailable. No texture
precision, publication, rendering or presentation algorithm is changed.

`wgpu-metal-srgb.patch` records the separate existing Metal surface fix: explicit
sRGB configuration tags the CAMetalLayer with the sRGB color space, rather than
nil (which disables color matching). The Apple application currently requests
Display P3; retain this delta until alternate callers and surface tests are
qualified for removal or upstream supplies the fix.

`wgpu-android-command-memory.patch` resets completed Android Vulkan command pools
at wgpu's existing all-completed boundary. Pools containing more than 128 buffers
are destroyed there; smaller pools reuse buffers and storage for at most 16
nonempty completed cycles before destruction. Replacement buffers are allocated
on demand. These limits bound reuse cycles and buffer count, not retained driver
bytes. Empty resets allocate nothing and do not advance the lifetime.
Unbounded command-buffer or pool retention exhausts Adreno host mappings despite
available RAM. The renderer also submits cold placement previews in bounded tile
batches to limit command storage before a submission completes.

The same patch retains up to 128 Vulkan framebuffers per completed Android
encoder. Entries expire after one unused completed cycle; encoders with larger
sets release the entire set. Permanent attachment-view identities prevent
recycled Vulkan handles from matching retired attachments. The
[Vulkan object lifetime rules](https://docs.vulkan.org/spec/latest/chapters/fundamentals.html#fundamentals-objectmodel-lifetime)
permit destroying referenced objects before an unused referencing object;
object destruction must not access the referenced objects. Cache entries do not
retain textures. Framebuffer retention is independent of command-storage cleanup.
Submission ownership and completion synchronization are unchanged. Other
platforms keep their original pool and framebuffer policies.

`wgpu-android-pool-retirement.patch` transfers reclaimed, already-completed pools
to one worker with one queued and one active pool. Each pool exclusively owns its
handle and registers per-device cleanup debt before transfer. A full or stopped queue,
or failed worker creation, destroys the pool synchronously. The worker starts at
device open. Final device destruction drains the already-completed pools' CPU
cleanup before destroying device objects or releasing the instance/drop guard.
Drawing and pool reset do not wait. The 16-cycle/128-buffer policy stays
the same; the two deferred pools have no known driver byte bound.
The hardware Vulkan ownership tests cover absent/full/disconnected queues and
final-device teardown while pool destruction is still pending. Run them with:

```sh
cargo test --offline --manifest-path vendor/wgpu-hal/Cargo.toml --features vulkan \
  --lib vulkan::pool_retirement::tests \
  --config 'patch.crates-io.wgpu-types.path="vendor/wgpu-types"'
```

These tests exercise the worker on the desktop. Android compilation and device
motion, memory, idle/resume and teardown journeys still verify the Android path.

`wgpu-instance-identity.patch` makes native wgpu handles equal only when the
same instance owns them. Upstream compares only the registry identifier, and
every instance restarts identifiers, so a device recreated after GPU recovery
compared equal to the retired one. Android kept the retired device's HDR tone
guide, bound its buffer on the new device, and aborted in `create_bind_group`
with `Cannot get non-existent resource`. Ordering and hashing include the owning
instance too. WebGPU handles are unchanged. The `layer-host` test
`a_restarted_gpu_is_never_mistaken_for_the_retired_one` covers the behavior.

`wgpu-surface-submission-admission.patch` serializes native surface configuration
with queue submissions and presentation using the existing command-index lock.
It retires completed command resources before swapchain replacement and releases
the locks before mapping, device-loss and work-done callbacks run. Configuration
errors still deliver pending callbacks. A window-free core regression exercises
concurrent admission and callback resubmission on successful and failed
configuration, including ranked lock checks. Surface teardown releases its lock
before destroying owned resources; pending submissions release their guards in
reverse acquisition order. Android's `frontBufferSurfaceLifecycle` exercises
native surfaces.

Remove each patch when an upstream release supplies its equivalent fix, and
remove these snapshots when no patch remains necessary.

`wgpu-webgpu-async-pipelines.patch` exposes WebGPU-only asynchronous render and
compute pipeline creation. Both immediate and asynchronous APIs use the same
descriptor conversion; the new calls copy descriptor data before returning an
owned promise future and produce an ordinary typed wgpu pipeline only after
success. Rejections retain the pipeline label, reason and browser message.
Native backends and the existing immediate API are unchanged.

The renderer chooses this API in its browser compilation queue, retaining
required-work ordering, error scopes and readiness gates. Native compilation
keeps its existing worker/cache path. This addresses GPU-process display
stalls that merely yielding JavaScript tasks did not resolve.

`wgpu-webgpu-device-features.patch` caches the mapped, immutable `GPUDevice`
feature set when the device is created. Feature tests in material and composition
loops then read Rust bits instead of remapping all browser feature strings on
every call. The cache uses the features exposed by the actual device, not its
adapter, and is recreated with each device.

## Android front-buffer presentation

`wgpu-core` 30.0.1 is pinned to upstream revision
`40f4a34ebaf56f9a046231f54125ad046239d3f3`, registry archive SHA-256
`14c018fce9b6270aa203c2fdd56f3cce996713534bd757e4ea58c8560b121f14`.
Its licenses and registry provenance are retained. HAL surface acquisitions
explicitly report initialized retained contents;
ordinary acquired images still require initialization. One upstream comment
trailing space is also removed.

`wgpu-lock-guard.patch` names the mutex and write guards' retained lock state
`_saved` and removes the mutex guard's unused-lint expectation. The normal,
uninstrumented build allows dead code in the ranked-lock module, so that
expectation cannot be fulfilled. Retained state still drops after the native
guards and restores lock ordering.

`wgpu-surface-discard-lost-device.patch` removes the acquired texture before
checking device validity. A lost-device return releases its metadata
without retaining swapchain semaphore references through surface teardown.
The texture inner is removed under the existing resource lock, including when
other handles still reference the texture. The lock is released before the
device check and backend discard. `AndroidRasterTest#frontBufferSurfaceLifecycle`
submits a buffered frame, destroys its device and detaches its surface in one
native worker call before checking GPU recovery and exact artwork.

The Vulkan HAL adds `SharedDemandRefresh`, advertised only by Android surfaces
with shared-image color-attachment support and present fences. It acquires one
image once, retains `SHARED_PRESENT_KHR` layout, retires a bounded pool of present
semaphores using explicit fences, and limits tile attachment loads/stores to
retained render areas. Android's loader feature must be queried through the core
Vulkan 1.1 entry point; its KHR alias returns false on both tested Mali and Adreno
devices. Surface copies use the same shared layout for instrumented HDR readback.

After the initial shared-image acquisition, the HAL skips the ordinary
acquire-semaphore reuse wait: subsequent updates do not acquire the image or
signal that semaphore again. Present-slot fences still protect every recycled
presentation semaphore. The Android host bounds outstanding updates at two,
allowing CPU encoding to overlap the preceding GPU update in queue order.
The former single-update gate serialized the pipeline; widening it without
removing the obsolete acquisition wait produced raster work followed by
acquisition timeouts and no presentation.

Lost/outdated presentation results remain latched until swapchain recreation,
so the next shared acquisition reaches the host's recovery path even though it
does not call `vkAcquireNextImageKHR` again. Rejected presents still enqueue
their fence/semaphore work; teardown waits for those fences too. Successful
updates retain the same nonblocking acquisition path. Regression tests are in
`wgpu-hal/src/vulkan/swapchain/native/tests.rs`.

Run those tests from the repository root with:

```sh
cargo test --manifest-path vendor/wgpu-hal/Cargo.toml --features vulkan --lib \
  vulkan::swapchain::native::tests \
  --config 'patch.crates-io.wgpu-types.path="vendor/wgpu-types"'
```

All Android builds require shared presentation for SDR and HDR pen input.
Camera navigation uses FIFO until a new paint contact. Unsupported drivers report
a canvas initialization error; there is no driver-support fallback or build flag.
Other platform hosts retain their existing presentation modes. [Android presentation](../docs/platforms/README.md#android-presentation)
describes the host side.
