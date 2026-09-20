# Pinned dependency fixes

## Portable HEIF/HEVC decoding

`heif-oxide` 0.1.0 and `rust_h265` 0.1.0 are the published MIT OR Apache-2.0
crates. Their sources, test fixtures, original licenses and registry provenance
are retained. Unused examples (including the minifb development dependency),
package lockfiles and registry cache markers are omitted.

| Crate | Upstream revision | Registry archive SHA-256 |
| --- | --- | --- |
| [heif-oxide](https://github.com/dan335/heif-oxide) | `86d722e46da3292cc5d777aaa99198fdc516f0c5` | `e12acb6edcb3bb9227dc6a06dd375a221eafe397ae2de72a876d79313831e365` |
| [rust_h265](https://github.com/roticv/rust_h265) | `e51348807a685b00343212a77e13d32692954321` | `dde60f5842f27ed06f1d84844cacd93d1a159f606365b30a5594c771e6b4eb17` |

`heif-portable.patch` exposes a bounded still-picture decoder that returns source
YUV and VUI color/chroma metadata. It admits coded dimensions and estimated
picture working memory before allocation, bounds parameter-set syntax, rejects
truncated header reads, and borrows cancellation callbacks at NAL, coding-tree
and filter boundaries. Independently coded stills cannot consume external
reference pictures or silently return an incomplete frame. The HEVC prediction,
transform and filtering algorithms are unchanged. The test-only container writer
adds the picture handler required by independent libheif enumeration.

The application uses its shared BMFF, grid, ICC, geometry and source-storage
pipeline around this API. It does not call the convenience `decode_bytes` path,
which converts source color to sRGB and uses scoped threads for grids. Application
grids decode one tile at a time, also on Wasm. Memory estimates are conservative
admission checks, not a hard allocator quota. Individual in-loop filters remain
synchronous between cancellation checks.

Verification includes the upstream suites (128 HEVC and 35 HEIF tests), exact
libde265 YUV comparison of a photographic still, shared source/ICC/grid/alpha
tests, Chrome execution and GTK Open/Import/Paste with an empty codec directory.
Initial HEIC variant limits and outstanding host work are recorded in the
[migration plan](../docs/development/portable-photo-core.md).

Run the isolated vendor tests with:

```sh
cargo test --offline --release --manifest-path vendor/rust_h265/Cargo.toml --lib
cargo test --offline --release --manifest-path vendor/heif-oxide/Cargo.toml \
  --config 'patch.crates-io.rust_h265.path="vendor/rust_h265"' --lib
```

## AV1 decoder portability

`rav1d` 1.1.0 is the published BSD-2-Clause crate from
<https://github.com/memorysafety/rav1d>, revision
`782dab2135ea64a057c097088a13eb8ed3cc3320`, registry archive SHA-256
`1932f060d5e7bd49dc9f8b272c1dc5e9ce0ffe141c28be900265d3989b36c9ed`.
The library sources, manifest, build script, license, release notes and registry
provenance are retained; development CI/configuration files and package lockfiles
are omitted.

`rav1d-portable.patch` makes `cc`/`nasm-rs` optional dependencies of the existing
`asm` feature. Pointer-sized C integer aliases use the matching Rust primitives.
The native `off_t` and errno values remain from libc; `wasm32-unknown-unknown`
uses an i64 offset and conventional Linux result codes without a libc dependency
or syscalls. The public error enum lets callers match target-correct EAGAIN
without hard-coding Unix errno values. The AV1 decoding algorithm is unchanged.
Both bit-depth features are enabled and default/assembly features disabled by
the application and the
[portability check](../tools/validation/portable-av1/README.md).

That independent check proves exact lossless 8/10/12-bit plane decoding in native
Rust and Chrome WebAssembly, with no WebAssembly host imports, and checks the
Android target. It uses one decoder thread and a frame-size limit. Application
container/color/memory/cancellation integration is tracked separately in the
[migration plan](../docs/development/portable-photo-core.md).

## Portable Zstd raster storage

`zrip-core` 0.10.1 and `zrip-encode` 0.8.7 are the published MIT-licensed
crates from <https://github.com/paddor/zrip>, revision
`c8aa18a056a1a4895c788d0c950782d2dd6b82d2`. Their registry archive SHA-256s are:

| Crate | SHA-256 |
| --- | --- |
| zrip-core | `a91bc58032e884eb76396eaa2d6607db874dcf95e6752fb15401da494ab72ba7` |
| zrip-encode | `a8eb51ef0ac2bda5ff1381eccf970be2bf7cc431e44f0e7a4b8cdccf685fd6f7` |

The leaf archives omit a license file; the MIT license here is copied from the
same upstream release's `zrip` 0.8.8 archive. Package lockfiles, cache markers,
and original unnormalized manifests are omitted. The application pins the
unmodified registry decoder 0.8.7 and enables `paranoid` throughout: these
codec paths forbid unsafe code and use bounds-checked scratch storage.

`zrip-raster.patch` preserves compression of raster byte planes. Histogram and
quarter-block shortcuts incorrectly classify periodic ramps as incompressible;
they are disabled while ordinary match search and raw-block fallback remain.
Blocks with no/few matches may still use Huffman literals, zero-sequence blocks
omit the mode byte, and a rejected block restores its previous Huffman table.
The Huffman encoder supports the full byte alphabet using FSE-compressed weight
descriptions, limits deep trees to Zstd's 11 bits, and caches descriptions only
when representable. FSE final-symbol initialization uses the reference half-word
bias. These changes retain standard Zstd frames and the existing archive format.

The [independent C oracle](../tools/validation/portable-zstd/README.md) checks both
directions, forced Huffman blocks, raster fixtures, and existing archives. C Zstd
exists only in that separate validation workspace. Remove this patch when an
upstream release provides the equivalent fixes.

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

`wgpu-android-command-memory.patch` frees completed Android Vulkan command
buffers at wgpu's existing all-completed boundary and allocates replacements
on demand. Large photo preparation followed
by save/reopen and editing exhausted host mappings in Adreno command recording on
the Wacom DTHA140: one failing process reached 64,039 mappings despite available
RAM. Resetting pools alone and periodic buffer reclamation still failed during
the long workload; freeing completed buffers on every reset passed.
The renderer also submits cold placement previews in batches of 64 tiles;
resetting pools alone did not bound command storage while recording a new preview.
The pool itself now retains reusable storage with the ordinary reset flags.
Profiling the initial `RELEASE_RESOURCES` policy showed repeated driver
allocation/reset costs: full 61 MP drawing took roughly 14–16 ms per host
callback despite only 1–2 ms of GPU work. Retaining pool storage while still
freeing every completed buffer reduced median callbacks to about 4.6 ms and
reached 8.33 ms presentation intervals. Submission ownership and completion
synchronization remain unchanged. Other targets retain their existing policy. See the
[host qualification](../docs/development/image-placement-web-android-progress.md).

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
stalls that merely yielding JavaScript tasks did not resolve. The
[tablet startup record](../docs/history/web-startup-tablet-2026-09-17.md)
records the evidence and physical-device regressions.
## HEIF/AVIF source color preservation

`libheif-source-profile.patch` applies to upstream libheif **1.23.4**. With
`output_image_nclx_profile_passthrough` enabled, the RGB conversion pipeline can
return pixels without their source NCLX primaries/transfer. The patch restores
an actually present source profile after successful passthrough conversion,
including profiles signalled only by the compressed bitstream. It does not
invent a profile for an untagged source or change sample conversion.

The pinned archive, dynamic libde265 backend and bridge are built by
[`tools/build/photo-codecs.py`](../tools/build/photo-codecs.py). No codec source
is downloaded during a Cargo build. GTK packaging verifies the source/recipe,
patch and library checksums and includes corresponding sources and licenses.
The patch is supplied under libheif's LGPL-3.0-or-later terms.

Reference: [libheif 1.23.4 decoding options](https://github.com/strukturag/libheif/blob/v1.23.4/libheif/api/libheif/heif_decoding.h).

AVIF now uses unmodified **libavif 1.4.2** with **dav1d 1.5.3**, both BSD-2-Clause,
through bridge ABI 2. The shared source archive/notice packaging includes both.
libavif supplies still/sequence metadata; Rust applies clean aperture, rotation
and mirroring while packing original samples into tiles. The HEIF profile patch
continues to apply only to the libheif/HEIC route. The independent AVIF oracle uses
system libavif 1.3.0 with AOM; source-constructed lossless fixtures additionally
compare against known samples. Reference:
[libavif 1.4.2 API](https://github.com/AOMediaCodec/libavif/blob/v1.4.2/include/avif/avif.h).
