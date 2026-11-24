# Known gaps

[Performance targets](../PERFORMANCE_TARGETS.md)

- **Windows tool bubbles need hardware measurements.** The retained native
  presenter shares ordinary panel components and stops its frame clock at rest.
  Software-adapter and Surface Pro functional journeys do not qualify moving-frame
  rates, physical pen latency or display timing on any reference tier. Hardware
  measurements require access to PresentMon's ETW presentation records.

- **A tapered pen redraws its held end every frame.** The live preview shows
  the end as if the pen lifted now, so the whole end changes each frame and the
  renderer restores and redraws its area. On the TCL a 10-width end costs 7–10 %
  of completed updates at 18–30 px and 22–25 % at 100 px
  ([low tier](low-tier.md#brush-taper)). Freezing the end's far part or restoring
  less than whole pages would reduce it.

- **Very wide tapered pens shorten their end.** A 1024 px G-Pen with a 10-width
  end needs more than the 128-page held allowance, so its end shrinks to the
  contacts that fit ([tapers](../internals/brushes.md#tapers)). The early save and
  lift replay behind the TCL's lift p99 of 214 → 495 ms are gone; lift has not been
  re-measured.

- **Wet, watercolor, Smudge and blending brushes fall behind the pen at 512 px.**
  - A frame consumes input only until its material contacts reach 512 page
    jobs ([brushes](../internals/brushes.md)), so these brushes never run out
    of memory; a slower device keeps the rest of the input queued and the
    stroke trails the pen.
  - On the TCL (12 MP) they complete 1.0–2.5 updates/s, at most 1.8 GB
    resident.
  - On the MovinkPad 11 (24 MP) wet and watercolor brushes complete 1.6–2.6
    updates/s, at most 2.6 GB resident, and the blenders 1.2 (Blending Knife)
    to 31.5 (Sharpen); Smudge completes 8.5 updates/s. Natural Blender (2.3 to
    1.9) and Palette Knife (1.8 to 1.6) are slower than before the blender suite.
  - On the TCL the measured blenders complete 0.7 (Blending Knife) to 2.2
    (Blur, Bristle) updates/s.
  - On the MovinkPad Pro 14 (61 MP) wet brushes complete 7.7 (Loaded Oil) to 33
    (Wet Round) updates/s and the measured blenders 9.4 (Blending Knife) to 187
    (Blur); Smudge, Soft, Blur, Grainy, Blending Stump and Tissue meet 120.
  - On the TCL and the MovinkPad Pro 14 only Smudge, Natural Blender, Soft,
    Blur, Bristle, Grainy, the Blending Knife, Blending Stump and Tissue are
    measured.
  - Multiply Glaze at 1024 px completes 86.8 updates/s on the MovinkPad Pro 14
    and 12.1 on the MovinkPad 11, where its compute kernel is slower than the
    fragment path it replaced (14.4).
  - On the MovinkPad Pro 14, the eraser fix in the dry contact loop made Pencil
    at 16 px about 10% slower (258 -> 233 updates/s); Pencil at 2048 px is
    unchanged.
- **The simple-brush class remains unqualified.**
  - On the current contained 60 × 10 path, 1536 px G-Pen reaches only
    75.36–75.54 fresh updates/s on Mid (target 90), although its gap passes.
    The preceding larger-path result was 55.6/s with p99 31.9–33.5 ms.
    Other simple brushes need measurements at 1536 px; their older 2048 px
    results are above the
    revised guarantee. See the [mid-tier measurement](mid-tier.md#g-pen-at-the-1536-px-guarantee).
  - The current low-tier 1024 px G-Pen measured stroke meets 60/s at both full
    and partial Opacity; other simple brushes
    and stacked-photo drawing retain gaps. See the [low-tier table](low-tier.md#brushes).
  - Top 2048 px G-Pen passes on the current 60 × 10 path at 175.65–179.08/s;
    this does not qualify the preceding larger path, which reached 87.6/s with
    p99 31.6–38.8 ms. The matched base with the same
    bounded command-pool cleanup reaches 87.0/s. See the
    [top-tier table](top-tier.md#dry-media-contact-model).
- **Most complex brushes at 1024 px remain unqualified.**
  - Pastel Block at nominal Size 1024 meets the current native Top writing
    pose targets at 173.74–175.88 fresh updates/s, but reaches only
    51.93–52.17/s on Mid and 21.27–23.43/s on Low. Its longer Low side pose
    reaches 3.32–3.44/s with p99 gaps up to 483.20 ms despite fully contained support. Mid and Top side
    poses remain unmeasured on this build.
  - Earlier measurements of the other complex presets reach 76–118 updates/s
    on Top, about 6–25 on Mid and 3–19 on Low. They need current measurements.
- **Dry-media qualification remains incomplete.**
  - Size denotes the 45° writing-pose long axis. With display-resolution
    predictions, native Pencil, Pointy, Charcoal and Pastel writing profiles meet
    rate and gap on all three tiers, including Pencil at Smoothing 100%, except
    MovinkPad 14 Pencil's 17.11 ms gap against 16.7 ms. Current native Pencil
    reaches 86.92–90.00/s on Low, 123.49–124.82/s on Mid and 149.18–156.04/s on
    Top.
  - Those predictions cost MovinkPad 14 Pencil at 256 px in screen strokes:
    313.7 → 261.9/s, with the worst gap 11.3 → 17.2 ms. GPU time falls; the
    extra display gather adds frame latency.
  - TCL Web remains unqualified; see the
    [low-tier table](low-tier.md#dry-media-contact-model).
  - Passing native rows cover only their measured profiles and poses; Shading
    Pencil, Chalk and Crayon remain unmeasured. Physical input-to-present latency
    is unmeasured.
  - First dry-brush selection remains slow. After clearing app data, native
    Pencil takes 1.81 s on Low, 0.89 s on Mid and 0.78 s on Top. On the earlier
    build TCL Web Pencil took 14.80 s and the primed native probe 35.53 s, with
    driver caches retained; neither has been remeasured. Cached selection is
    quick; that does not qualify first-use latency.
- **Liquify's retained-map implementation has its own qualification.**
  Measurements of the removed color-feedback implementation do not qualify it.
  The [low](low-tier.md#retained-map-liquify),
  [mid](mid-tier.md#retained-map-liquify) and
  [top](top-tier.md#retained-map-liquify) tables measure deformation of the Photo
  layer itself: Low reaches 18.11–34.84 fresh updates/s, Mid 31.12–55.95/s,
  and Top 134.58–172.79/s. Low and Mid miss rate and gap
  limits. Top meets the rate in every mode, but only Reconstruct meets the
  fresh completion-gap limit. Empty-layer workloads only exercise geometry.
  A guarded full-width sweep covers the 61 MP canvas without exhausting memory,
  but continued drawing after Undo and map eviction takes 1649 ms in that
  broad-coverage case.
- **The retouching brushes at 512 px** (Clone Stamp, Healing and Spot Healing)
  complete 57–62 updates/s on the mid tier (target 90). Their soft edge keeps
  a wide band of pixels in the dab loop. The low and top tiers are not
  measured.
- **Some MovinkPad 11 measurements use 60 Hz.** Before any display-paced
  mid-tier row can pass, it must present at 90 Hz. The Liquify profile verifies
  the 90 Hz mode before its matrix with Adaptive refresh enabled for its
  benchmark application; it does not measure physical presentation per trial.
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
- **Vector layers: most edits meet their rates; the 5,000-path width scrub,
  navigation, Fill outside, merges and Rasterize miss.** Measured 2026-10-20 at
  `2567874a7` on `crowded_vector_fixture` documents at each tier's canvas with
  equal path density (10,000 / 20,000 / 50,000 paths), with the mid panel at
  90 Hz; cells marked *r3* were re-measured on the round 2 and 3 tree
  (`e7cdd234`) and cells marked *r4* on 2026-10-21 on the round 2–4 tree
  (`e2667d46`; the width scrub on `a4027fb6`). Tier tables: [low](low-tier.md#vector-layers),
  [mid](mid-tier.md#vector-layers), [top](top-tier.md#vector-layers). Bold
  cells miss.

  | Presented frames/s unless noted | Low (60) | Mid (90) | Top (120) |
  | --- | --- | --- | --- |
  | Pen-down after a commit, first completion p95 (33 / 22 / 17 ms) | **40.8–51.0 ms** | **32.8–35.5 ms** | 14.4–**18.0** ms |
  | Move drag start, 1 / 5,000 paths (100 ms) | 63–77 / 53–85 ms | 58–63 / 55–62 ms | 43–80 / 40–53 ms *r3* |
  | Rotate, 5,000 paths | **55.4–55.6** | 88.6–89.0 | 120 |
  | Edit Points, 1 / 100 paths | 60.0 / 59.8–60.0 *r3* | 89.4–89.6 / 89.6 *r4* | 118.6–119.0 |
  | Outline width scrub, 100 / 5,000 paths, updates/s | 60.0 / **10.4–11.2** *r3* | 89.6 / **24.7–24.9** *r4* | 120.0 / **29.6–32.1** *r4* |
  | Pan / pinch, nothing selected | **43.1–43.2 / 33.4–33.9** | **57.8–58.0 / 58.2–58.4** | **27.8–28.8 / 36.8–38.9** |
  | Pinch, 5,000 paths selected | **5.8–28.6** *r3*, one run in three with a 1.25 s gap | **56.7–57.1** | **21.1–21.4** *r3*, gaps 0.67 s |
  | Fill outside the paths, first change / complete (250 ms) | **0.60–0.64 / 3.15–3.22 s** | **0.80–0.81 / 2.68–2.73 s** | **2.10–2.12 s** |
  | Merge Down onto a vector layer (100 ms) | **310–320 ms** | **316–331 ms** | **170–178 ms** |
  | Merge Down onto paint / Rasterize, first change (100 ms) | 78–90 / **112–132 ms** | **1.21–1.34 / 1.17–1.19 s** | **1.93–2.45 / 1.42–2.36 s** |

  G-Pen inking, Move, Scale, Edit Points, the 100-path width scrub, every
  eraser, Fill inside, commit, Undo and Open meet their targets on every tier,
  as does Rotate on the top tier. The incremental history ledger brought commit,
  Fill inside and Merge Down onto a vector layer back near their earlier times,
  and pen-down after a commit on the top tier from 28–30 to 8–10 ms p50. A
  paint-layer thumbnail after filling a whole layer arrives 4.0–4.1 / 3.8 /
  6.4 s later; editing while it is pending costs no frames. The frame that
  shows a finished Rasterize takes 95–110 / 231–251 / 63–87 ms of CPU and
  157 / 222–238 / 87–136 ms of GPU; before it, no canvas frame is presented for
  about 1.0 / 1.1 / 1.3–2.3 s.

  Causes, from canvas-thread `simpleperf` profiles windowed to the slow frames
  and GPU timestamps with Stats open, with proposed fixes:
  1. **The 5,000-path width scrub re-derives every outline per step.** On the
     top tier `Vectors::replacement_batch` and `vector::tiles::derive` take
     26–28% of the canvas thread, and rebuilding `Arc<[PathItem]>` and the
     replacement map with its allocator churn about 25% more. The round 2–4
     tree (`a4027fb6`), whose presenter composes only the layers above the
     moving paths and paces the scrub to the display, presents 29.6–32.1/s on
     top (18.5–21.4 on the round 3 tree, 26.1–35.0 at `2567874a7`) and
     24.7–24.9/s on mid (17.8–18.2); low presented 10.4–11.2/s on the round 3
     tree. The 100-path scrub holds 120.0 and 89.6/s. **Fix:** derive the
     outlines on worker threads in batches and patch styles in place instead
     of rebuilding the selection's item arrays per step. Expected: 5,000 paths
     at about the 100-path rate on top; 2–4× on mid and low.
  2. **Mid-tier G-Pen painting matches `8ee87a1e5` again.** At 1536 px with
     Stats closed the round 2–4 tree paints 23.3–23.6 updates/s against
     23.1–23.6 for `8ee87a1e5` in alternating runs (21.6–22.0 on the round 3
     tree). Per-pass GPU timestamps on the round 3 tree had put the whole
     loss in the half-float conversion, which converted both levels from one
     invocation per coarse texel: 5.8–6.5 against 3.4–3.6 ms per update.
     Converting one texel per invocation again removed it. Low painting was
     unchanged on the round 3 tree (31.6–32.6 against 31.6–32.4).
  3. **A level change with a selection still renders a whole level in one
     frame on the low tier.** One pinch run in three makes frames of
     0.9–1.25 s, also on the round 3 tree: `Scene::compose` → `scale::Cache::render_graph` with 68% in
     Mali command recording and 11% in `create_target`. The per-frame budget
     covers the selection's images but not the layer at the new level.
     **Fix:** budget the layer's composition at the new level as well and keep
     the previous level scaled until it is ready. Expected: the no-selection
     pinch rate (33–34/s) without second-long frames; the same applies to the
     top tier's 0.7 s gaps.
  4. **Pen-down after a commit on low and mid** is 24–33 ms p50 against 13–23 ms
     before the history regression. The committed path is presented 157–170 /
     216–247 ms after pen-up, so the next contact starts while the commit is
     still in flight; this round did not profile it. **Fix:** submit the
     committed path's raster as separate low-priority work so the next
     contact's first update does not queue behind it. Expected: p95 under 33 /
     22 ms.
  5. **Low-tier Rotate of 5,000 paths** presents 55.4–55.6/s, GPU bound by the
     viewport pass plus the rotated moving image. **Fix:** draw it in the
     presenter's split sampling instead of compositing it each frame.
     Expected: 60/s.
  6. **Merge Down onto paint and Rasterize** show nothing until done on mid and
     top; the bake now records about three passes per window but still holds
     the canvas for 1.2–2.4 s. **Fix:** present the baked windows
     progressively. Expected: first change within a frame. **Fill outside**
     still labels the whole canvas before its first change (0.6–2.1 s); the
     same progressive presentation applies.

  Round 3 fixed Edit Points and the 100-path width scrub on Mali (repainting
  only the moving bounds) and the top tier's drag start (reused images);
  round 4 fixed the mid-tier painting regression and the top tier's slower
  5,000-path scrub. Raw summaries, tables, A/B runs and symbolized profiles are
  in `artifacts/vector-tablets-20261020/round2/`, `round3/` and `round4/`.
- **Photo zoom and several reference-tier navigation rows still miss.** The
  presentation-admission and renderer-cache fix restores top-tier default-paper
  rotate and pinch to about 119 presented frames/s. On the 61 MP photo, pan and
  rotate also reach about 119/s; pinch remains 105–106/s with Navigator closed
  and about 112/s open, below the 114/s floor.

  Factory-density/font measurements on 2026-10-25 give middle-tier default
  rotate 86.08/s and pinch 77.76/s; rotation clears the 85.5/s rate floor but
  misses the 22.222 ms gap limit. On its 24 MP photo, closed-Navigator pan,
  rotate and pinch reach 66.53, 61.10 and 66.69/s; open they reach 77.95,
  70.39 and 73.63/s. No middle-tier row meets both gates. On low, default
  rotate and pinch reach 60.02/s. Closed-Navigator photo motion reaches 56.26,
  54.52 and 48.39/s; open it reaches 60.02, 59.14 and 54.41/s. Open rotation
  clears the rate floor but one 33.361 ms gap p99 exceeds 33.333 ms. See the
  [top](top-tier.md#navigation-presentation),
  [middle](mid-tier.md#navigation-presentation) and
  [low](low-tier.md#navigation-presentation) tables. Earlier inherited-density
  comparisons remain separate diagnostics.

  The former one-frame completion gate serialized FIFO navigation. Waiting
  only for preceding artwork permits overlapping viewport frames; encoded
  backdrop uniforms, cached layer styles and retained changed-cell masks keep
  ordinary camera motion from becoming an artwork dependency. The original
  shader and quality settings are unchanged. On top, the same default-paper
  shader drops from about 19.5 to 7.0 ms GPU time after the pacing change;
  earlier 18–19 ms observations do not establish an intrinsic shader floor.
  Its GPU power/scheduling mechanism remains unestablished.

  **Remaining work:** profile photo pinch's changing display levels and the
  middle/low viewport passes. Driver presentation occupies more owner wall
  time: middle-tier default rotation input queue p95 increases from about 2 to
  10.5 ms, pinch to 15.5 ms; top-tier photo pinch increases from about 8 to
  9–10 ms. Higher presented rates do not establish lower input latency.
  Investigate driver backpressure and owner scheduling while preserving
  overlapping viewport submissions.

  Attached Gaussian 85 retains a separate roughly 16 s owner-frame stall on
  top, as in the baseline. Diagnostics locate it in the shared session-frame
  call inside frame preparation, outside native presentation; its internal
  cause remains unqualified. Low filtered paint remains about 0.3 updates/s.
  Cached navigation does not qualify pending-filter responsiveness. Evidence is
  under `artifacts/nav-fix-qualification-20261024/`; earlier viewport variants
  remain under `artifacts/vector-tablets-20261020/`.
