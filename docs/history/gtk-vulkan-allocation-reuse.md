# Large Vulkan allocations in bundled GTK

[Design history](README.md)

The bundled GTK runtime carries `tools/build/gtk-runtime/vulkan-memory-pool.patch`,
which reuses large GSK Vulkan allocations for a short time. This record explains
the cause, the alternatives and the state of upstream GTK on 2026-10-28 (GTK
main `461574e0d`), for whoever revisits or upstreams the patch.

## Cause

- **SDR surfaces take a float16 colour step.** On a colour-managed Wayland
  session, Mutter describes an SDR output as sRGB primaries with a gamma 2.2
  transfer function. GDK turns that description into a CICP colour state, and
  every CICP state renders in BT.2100 PQ at float16
  (`gdk_color_state_new_for_cicp`); only sRGB descriptions map to GDK's default
  sRGB state. `gsk_gpu_node_processor_process` therefore renders each damage
  rectangle into a new float16 image and converts it into the swapchain. A PQ
  (HDR) surface renders straight into the swapchain.
- **Large images bypass suballocation.** GSK's buddy allocator suballocates
  requests up to 512 KiB from 1 MiB blocks. Larger requests call
  `vkAllocateMemory` directly and return to `vkFreeMemory` as soon as the image
  is released. Effect offscreens for shadows, blurs and opacity take the same
  path.
- **Moving geometry allocates every frame.** During window resizes and column or
  panel drags the damage size changes every frame, so each frame allocates and
  frees at least one large block. Unpatched, on an NVIDIA RTX PRO 6000 Blackwell
  Max-Q with GTK 4.24.1 at application `5b99e213d`, allocation plus free cost per
  frame was 0.9 ms for panel drags (0.8 MB blocks), 2.3 ms on average and up to
  8.5 ms for column drags, and 2.7 ms for window resizes (8 MB blocks), against
  an 8.3 ms frame at 120 Hz.
- **The driver is slow but within the API contract.** NVIDIA documents
  `vkAllocateMemory` as expensive on the CPU and recommends suballocating from
  large allocations. The per-frame allocation in GSK is the cause. Allocation
  costs on Mesa drivers were not measured.

## Shipped design

The patch changes only `gsk/gpu/gskvulkanmemory.c`:

- each memory-type allocator holds up to four freed large blocks;
- sizes round up to eight classes per power of two (at most 12.5% larger), and a
  request reuses a held block of exactly its class;
- held blocks keep their mapping and callers see their requested size;
- a held block is freed 100 ms after its return, checked on large returns and by
  a main-loop timer that stops when nothing is held; a full queue frees its
  oldest block.

`tools/build/gtk-runtime/test-vulkan-memory.py` runs the real allocator against a
mock driver. The matched results are in the tier tables under
[GTK Vulkan memory reuse](../performance/top-tier.md#gtk-vulkan-memory-reuse):
window resize at dark 2× rose from 44.7 to 110.0 presents/s, and HDR rows
improved as well, so the gain is not limited to the colour step. Dark 2× window
large allocations peaked at 112 MB instead of 64 MB and drained within about
280 ms of idle.

Known trade-offs:

- rounding also enlarges long-lived large images that never enter the queue;
- the cap counts blocks, not bytes, so large displays can briefly hold several
  window-sized blocks; only 3200 × 2000 was measured.

## Alternatives considered

| Alternative | Outcome |
| --- | --- |
| Reuse images instead of memory | Rejected. Images have fixed sizes and formats, and the damage size changes every frame, so exact-size reuse misses during resizes and drags. Reusing larger images requires every reader to stay inside the used area: edge sampling, blur reads and mipmaps. GSK creates offscreens in several node-processor paths, and the gain over the pool is image creation alone, about 0.2 ms per image. |
| Render SDR without the colour step | Not attempted. It removes a float16 pass on every SDR frame, not only during motion, but leaves effect offscreens and HDR untouched. GDK would have to treat gamma 2.2 SDR as a native rendering state, which changes colour output; that decision belongs to GTK. |
| Hold blocks longer, or evict on a miss | Rejected. A four-block queue released after 1 s of idle raised the dark 2× peak to 166 MB. Evicting on a miss hurts frames that allocate several large images. |

## Upstream GTK in October 2026

- `gskvulkanmemory.c` has not changed since 2024. No issue or merge request
  reports allocation cost or proposes reuse, and CICP surfaces still render
  through the float16 colour step.
- The maintainers' direction for offscreens is to consolidate them into atlases
  and merge render passes ([#8464](https://gitlab.gnome.org/GNOME/gtk/-/issues/8464)).
  Benjamin Otte's step plan there starts with making every offscreen consumer
  accept atlas images (!10482 and !10488, merged), then tracks atlases per format
  and usage, and finally draws offscreens into them.
- Otte wants node images cached across frames through `GskGpuCached` inside
  `gsk_gpu_node_processor_get_node_as_image()`
  ([!10026](https://gitlab.gnome.org/GNOME/gtk/-/merge_requests/10026)).
- Recent work avoids offscreens altogether: !10454, "Don't generate
  unnecessarily large offscreens", and the draft
  [!10499](https://gitlab.gnome.org/GNOME/gtk/-/merge_requests/10499).
- That plan covers small effect offscreens. The texture atlas is 1024 px with
  items up to 256 px, so window-sized colour-step images stay outside it, and
  their content changes every frame.
- Maintainers ask for root-cause analysis rather than recordings of slow motion
  ([#6328](https://gitlab.gnome.org/GNOME/gtk/-/issues/6328)). The
  [AI contribution policy](https://gitlab.gnome.org/GNOME/gtk/-/blob/main/CONTRIBUTING.md#ai-contribution-policy)
  requires disclosure and a person who can explain every change, and it
  forbids answering review with generated text.

## Revisiting

- When the pinned GTK moves past 4.24, check whether offscreens use atlases and
  whether the colour step still allocates per frame. Rerun the allocator test
  and the window and column rows of the tier tables, and shrink or drop the
  patch once GTK stops allocating large images per frame.
- To raise it upstream, start with a short issue: a sysprof capture on NVIDIA
  showing `vkAllocateMemory` and `vkFreeMemory` in frame time, a Mesa comparison,
  and the question whether allocator-level reuse is acceptable. Review may ask
  for blocks to be released from GSK's existing per-frame cleanup instead of a
  separate timer. A merge request must be written and owned by a person.

## Sources

- [NVIDIA: Vulkan dos and don'ts](https://developer.nvidia.com/blog/vulkan-dos-donts)
- [GTK #8464: offscreen render passes and the atlas plan](https://gitlab.gnome.org/GNOME/gtk/-/issues/8464)
- [GTK !10026: caching blur node images](https://gitlab.gnome.org/GNOME/gtk/-/merge_requests/10026)
- [GTK !10499: Cairo for small offscreens](https://gitlab.gnome.org/GNOME/gtk/-/merge_requests/10499)
- [GTK #6328: slow window resize](https://gitlab.gnome.org/GNOME/gtk/-/issues/6328)
- [GTK AI contribution policy](https://gitlab.gnome.org/GNOME/gtk/-/blob/main/CONTRIBUTING.md#ai-contribution-policy)
