# wgpu Wayland color pass-through

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

The local patch exposes `SurfaceColorSpace::PassThrough` and its capability bit,
maps them to `VK_COLOR_SPACE_PASS_THROUGH_EXT`, and rejects the new choice on
backends that cannot provide it. `Auto` behavior is unchanged. The Vulkan mapping
round-trip test includes pass-through. `wgpu-color-passthrough.patch` records every
source change relative to the published crates.

The GTK host uses this to own an explicit Wayland image description with the
piecewise sRGB curve or an equivalent ICC profile. The driver's legacy sRGB
description is ambiguous and Mutter 50.4 interprets it as gamma 2.2. The
[Vulkan specification](https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html)
defines pass-through as the way for a Wayland application to own that description.
wgpu still owns swapchain creation, acquisition, synchronization and presentation.

No other platform host enables this path. Remove the patch and these snapshots
when an upstream release supplies equivalent pass-through support. Do not replace
the explicit description with a compositor-specific gamma adjustment.
