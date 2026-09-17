# Pinned dependency fixes

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

Remove each patch when an upstream release supplies its equivalent fix, and
remove these snapshots when neither patch remains necessary.
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
