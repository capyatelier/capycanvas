# Known gaps

[Performance targets](../PERFORMANCE_TARGETS.md)

- **Wet, smudge and blending brushes run out of memory on every tier at 512 px.**
  - On the TCL (12 MP), Android's low-memory killer stops Opaque Gouache, Loaded
    Oil and Palette Knife at about 4.2 GB resident. The rest complete only
    0.3–0.5 updates/s.
  - On the MovinkPad 11 (24 MP), every one of them fails:
    - Opaque Gouache, Watercolor Wash, Wet Watercolor, Loaded Oil, Wet Round and
      Natural Blender abort in the native allocator (a Scudo map failure).
    - Smudge and Palette Knife stop with a canvas GPU out-of-memory error.
  - On the MovinkPad Pro 14 (61 MP), only Smudge completes, at 20 updates/s.
    - The others crash, mostly inside the Adreno Vulkan driver (a SIGSEGV at
      address 0x1c).
    - Opaque Gouache aborts in the native allocator.
    - Smudge also crashed in the driver in one of two runs.
  - On the TCL the process runs out of RAM.
  - On the MovinkPads the likely cause is exhausting the process's memory
    mappings instead. This is not yet verified.
    - Successful Smudge and Liquify Push strokes on the MovinkPad Pro 14 reach
      41,000–51,000 mappings. The G-Pen reaches about 11,000.
    - Android's default `vm.max_map_count` is 65,530.
- **Simple brushes miss their guaranteed size on every tier.**
  - Only the Calligraphy Pen meets it, and only on the top tier.
  - At 2048 px the G-Pen completes 88 updates/s on the top tier (target 120)
    and 17 on the mid tier (target 90).
  - On the low tier it completes 37 at 1024 px and 12 at the 2048 px goal
    (target 60).
- **Complex brushes at 1024 px miss on every tier.**
  - On the top tier all but Spray reach 76–118 updates/s, so most are close.
  - On the mid tier they reach about 6–25, and on the low tier about 3–19.
- **Liquify at 512 px** completes 28–74 updates/s on the top tier, 1.2–6.5 on
  the mid tier and 0.8–12 on the low tier.
- **The MovinkPad 11 presents at 60 Hz.** Before any display-paced mid-tier row
  can pass, it must present at 90 Hz.
- **Navigation on the tier canvas is untested on two tiers.** The low and mid
  tiers have no pan, zoom or rotate measurements on their canvases. The top tier
  has only pinch data at 61 MP.
- **Operations with no timing harness:**
  - Layer opacity scrub, reorder and blend changes.
  - Gradient and curve-point drags.
  - List scrolling.
  - Android canvas rotation.
  - Magic Wand and fill latency.
- **Transforms on the top tier** have been measured only on GTK, not on the
  MovinkPad Pro 14.
