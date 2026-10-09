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
- **The simple-brush class remains unqualified.**
  - On the mid tier, the 1536 px G-Pen completes 55.6 fresh updates/s (target 90)
    on a contained path, with completion-gap p99 31.9–33.5 ms (limit 22.2 ms).
    Other simple brushes need measurements at 1536 px; their older 2048 px
    results are above the
    revised guarantee. See the [mid-tier measurement](mid-tier.md#g-pen-at-the-1536-px-guarantee).
  - The low-tier 1024 px G-Pen measured stroke meets 60/s; other simple brushes
    and stacked-photo drawing retain gaps. See the [low-tier table](low-tier.md#brushes).
  - The top-tier 2048 px G-Pen reaches 87.6 fresh updates/s, with completion-gap
    p99 31.6–38.8 ms. Both targets remain open. The matched base with the same
    bounded command-pool cleanup reaches 87.0/s. See the
    [top-tier table](top-tier.md#current-g-pen-comparison).
- **Complex brushes at 1024 px miss on every tier.**
  - On the top tier all but Spray reach 76–118 updates/s, so most are close.
  - On the mid tier they reach about 6–25, and on the low tier about 3–19.
- **Liquify at 512 px** completes 28–74 updates/s on the top tier, 1.2–6.5 on
  the mid tier and 0.8–12 on the low tier.
- **The retouching brushes at 512 px** (Clone Stamp, Healing and Spot Healing)
  complete 57–62 updates/s on the mid tier (target 90). Their soft edge keeps
  a wide band of pixels in the dab loop. The low and top tiers are not
  measured.
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
- **Retained wet-photo transforms on the top tier** reach 36.3–37.0 renderer
  updates/s on the 61 MP canvas. Presentation is unmeasured; this does not meet
  the 120 fps target. See the [material-transform measurements](top-tier.md#retained-wet-photo-transforms).
  Projective and Warp motion remain unmeasured on that reference tablet.
- **Object-layer simplification has no current motion qualification on the reference tiers.**
  Imported-image placement, Move and Scale/Rotate still need warmed, repeated
  measurements on the three reference canvases. Existing object and placed-photo
  results in the tier tables describe earlier builds. Desktop browser callbacks
  and renderer timings do not establish presented-frame rates or qualify a tier.
