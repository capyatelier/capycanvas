# Low tier: 60 fps on 12 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: TCL TAB 11 Gen 2 (9465X) with a 1200 × 1920, 60 Hz panel. The canvas
is 4248 × 2832.

- Every row targets **60 fps** unless marked soft.
- Geometry measurements use the 12 MP photo at Fit zoom. Older UI-only rows
  name their own canvas sizes.

## Operations

| Operation | Target | Measured | Source |
| --- | --- | --- | --- |
| Pan: Hand tool, one or two fingers | 60 | | |
| Pinch zoom | 60 | | |
| Two-finger rotate | 60 | | |
| Navigator drag | 60 | | |
| Brush-cursor hover | 60 | | |
| Placed-photo translation | 60 | Screen 59.3 presents/s, p99 16.9 ms; renderer 93.8 completed updates/s | Transform damage build below, `photo-translate-drag` |
| Placed-photo corner resize | 60 | Screen 59.4 presents/s, p99 16.9 ms; renderer 86.3 completed updates/s | Transform damage build below, `photo-handle-drag-bar-hidden` |
| Pixel transform corner resize: Free | 60 | Screen 59.4 presents/s, p99 16.9 ms; renderer 77.7 completed updates/s | Transform damage build below, `photo-pixels-handle-drag` |
| Pixel transform: Uniform, Skew or Rotate | 60 | | |
| Pixel transform translation | 60 | Screen 59.4 presents/s, p99 16.9 ms; renderer 84.1 completed updates/s | Transform damage build below, `photo-pixels-translate-drag` |
| Pixel transform: Distort | 60 | **Not met.** Screen 55.8 presents/s, p99 16.9 ms; renderer 62.0 completed updates/s | Transform damage build below, `photo-pixels-distort-drag` |
| Pixel transform: Perspective | 60 | | |
| Pixel transform: Warp | 60 | **Not met.** Screen 45.9 presents/s, p99 33.4 ms; renderer 45.9 completed updates/s | Transform damage build below, `photo-pixels-warp-drag` |
| Crop corner drag | 60 | Screen 59.4 presents/s, p99 16.9 ms; renderer 113.1 completed updates/s | Geometry build below, `crop-handle-drag` |
| Pixel resize after placing the photo at 45% size | 60 | Screen 59.6 presents/s, p99 16.9 ms; renderer 115.3 completed updates/s | Geometry build below, `scaled-photo-pixels-handle-drag` |
| Selection translation, full canvas | 60 | Renderer 136–139 submissions/s; GPU interval p99 14.3–17.3 ms (6000 × 4000) | Canvas-bar `selection-handle-drag` and `selection-distort-drag`, 2026-09-27 |
| Move tool layer drag | 60 | | |
| Move selected pixels: whole image | 60 | Screen 59.1 presents/s, p99 16.8 ms; renderer 86.6 completed updates/s | Transform damage build below, `move-all-drag` |
| Move selected pixels: partial selection | 60 | Screen 59.5 presents/s, p99 16.7 ms; renderer 101.3 completed updates/s | Transform damage build below, `move-part-drag` |
| Move selected pixels: Leave Copy | 60 | Screen 59.2 presents/s, p99 16.9 ms; renderer 73.3 completed updates/s | Transform damage build below, `move-part-leave-copy-drag` |
| Marquee, Lasso or Polygon drag | 60 | | |
| Selection Brush or Quick Mask, 1024 px | 60 | | |
| Grow, Shrink or Feather drag, full canvas | 60, soft | | |
| Pointwise adjustment slider: Exposure | 60, soft | **Not met.** Screen 49.1 presents/s, p99 33.4 ms; renderer 26.5 completed updates/s | Pointwise graph comparison below |
| Pointwise chain: Levels, Vibrance, Exposure slider | 60, soft | **Not met.** Screen 53.7 presents/s, p99 33.3 ms; renderer 20.9 completed updates/s | Pointwise graph comparison below |
| Other pointwise adjustment sliders | 60, soft | | |
| Neighbourhood filter slider: Gaussian Blur, Unsharp Mask, Edge-Preserving Smooth | 60, soft | | |
| Animated or warping filter: Domain Warp, Ripple | 60, soft | | |
| Fill layer or gradient-fill edit | 60, soft | | |
| Navigation with proof or tone guide shown | 60 | | |
| Gradient drag | 60 | | |
| Figure or ruler drag | 60 | | |
| Layer opacity scrub | 60 | | |
| Layer reorder drag | 60 | | |
| Navigation with 8 visible paint layers | 60 | | |
| Drawing with 8 visible paint layers, G-Pen 1024 px | 60 | | |
| Panel, tab, column or toolbar drag and docking | 60 | | |
| Panel or column resize | 60 | | |
| Drawer open and close | 60 | | |
| Colour wheel or picker drag | 60 | | |
| Slider scrub: size, opacity, flow | 60 | | |
| Canvas action bar show, hide and move | 60 | **Not met.** UI frame p50/p95: 32.9/41.7 ms moving the bar, 11.6/21.0 ms show and hide (2048 × 1536) | Canvas-bar `ui-bar-move` and `ui-bar-show-hide`, 2026-09-27 |
| Tool Options or panel content change | 60 | **Not met.** UI frame p50/p95 25.4/35.3 ms | Canvas-bar `ui-panel-change`, 2026-09-27 |
| List scrolling: layers, brushes, filters | 60 | | |
| Menu open and close | 60 | | |

Earlier geometry build (crop and scaled-photo rows): 2026-09-28, graph transform
migration based on `af177ede`,
APK SHA-256 `c872c55b455d292c4444f7c2ca42303e421fe17e1e90a4b62e1b11b9510f8554`.
Release Rust with an unminified Android benchmark harness, default glass, thermal
status 0, three warmed five-second gestures per row. Values are medians across
runs. Screen rates use SurfaceFlinger actual-present timestamps; this device
provides no separate SurfaceView timeline, so they do not independently establish
canvas presentation rates. Renderer counts exclude empty updates and terminal
polling. Setup commands wait for shared command availability outside timing.
Raw results are in `artifacts/display-production/transform-final-geometry-tcl`.

In that earlier comparison, the old renderer at `29a564eb` completes 96.3 placement translations/s,
94.6 placement resizes/s, 92.7 pixel translations/s, 91.2 pixel resizes/s,
91.7 distortions/s, 51.4 warps/s, 123.8 crops/s and 112.0 scaled-photo resizes/s.
Placement resize, crop and the pixel-transform journeys regress in renderer
throughput. Distort and Warp fall below the screen-present target; other rows
remain near 59 screen presents/s. Pixel transforms now use the shared graph,
with prefiltered immutable inputs and direct mesh color evaluation. These
measurements precede the selected-pixel Move integration from `a16bb1a4`.
Separate phase traces show 6–10 ms of transform composition plus 5.5 ms of
viewport work, rising to 12.6 ms for Warp. The matching old viewport costs
about 3.1 ms, or 9.0 ms for Warp. Both costs remain optimization work. Earlier canvas-bar handle-labelled measurements
started inside the handle and measured translation; they do not qualify resizing
or distortion.

Transform damage build: 2026-09-28, `2e04caa8` plus incremental cut
invalidation, APK SHA-256
`b614cd5942d2165ac6054497f96873c215ed6b09863659453a4b337dd00a248b`.
Three runs alternate before/after order, with the same private harness, 12 MP
photo, Fit camera, release profile and default glass. Thermal status is zero
before and after all six runs. The presentation-accounting limits above apply.
Raw results and the source patch are under
`artifacts/display-production/transform-cut-paired-*` and
`transform-cut-damage-whitebox-source.patch`.

Avoiding repeated invalidation of the stationary cut improves pixel translation
76.64 to 84.07 completed updates/s, resize 71.13 to 77.68, whole-image Move 78.60
to 86.63, and partial-selection Move 76.98 to 101.28. Leave Copy, which already
keeps the original, changes 72.18 to 73.32. Distort and Warp remain essentially
unchanged at 62.01 and 45.87. Viewport GPU medians fall from 5.60 to 5.03 ms for
translation and 5.61 to 4.96 ms for resize. Old-main transform throughput still
exceeds this build; these gains do not close the geometry regressions.

## Pointwise filter composition

Measured 2026-09-28 on the 12 MP photo at Fit, with three alternating pairs of
warmed five-second stylus scrubs, release Rust, default glass and thermal status
0. The harness waits for shader readiness after priming and verifies changing
parameter values during motion. Both builds use the same camera and test APK.

| Slider journey | Previous filter executor, completed updates/s | Display graph, completed updates/s | Speedup | Graph completion gap p99 |
| --- | --- | --- | --- | --- |
| Exposure | 1.59 | 26.53 | 16.67× | 67.0 ms |
| Levels, Vibrance, Exposure chain | 0.99 | 20.90 | 21.01× | 65.9 ms |

Neither journey meets the motion target. Screen presents include native controls
and do not independently establish canvas presentation cadence. Renderer-owned
storage falls from 579.8 to 306.5 MiB; this is not process RSS.

The control is production `7cad88dd`, before pointwise graph admission, using APK
SHA-256 `574bac2557e30ce43444a5657de969719052975f285e3d2639e879e3eec7a977`.
Candidate APK SHA-256:
`dbb61014e2722cb37240f68d8b11f6d223d6d18b1c58eeed98414bfc2c9f5016`.
Raw results, immutable builds, source patch and harness provenance are under
`artifacts/display-production/effect-graph-ready-*` and `effect-graph-source.patch`.
These compare filter execution within the production branch, not against a fresh
old-main build. Other filters and high-frequency preview quality remain unqualified.

## Sources larger than the canvas

This diagnostic places the 12 MP photo at original size over a 1062 × 708 canvas
and moves it at reduced zoom. It measures source-window retention, not the
12 MP canvas target above. Three five-second gestures alternate before/after
order on the TCL at thermal status 0, using the same test APK and private app.

| Measurement | Full source images | Bounded source windows |
| --- | --- | --- |
| Completed updates/s | 139.06 | 137.71 |
| Completion gap p99 | 12.80 ms | 12.99 ms |
| Screen presents/s | 59.57 | 59.39 |
| Renderer-owned storage | 501.4 MiB | 321.4 MiB |

The 1.0% throughput difference lies within these runs' overlapping ranges;
renderer-owned storage falls 35.9%. This is not a process-memory measurement.
Windows retain nearby motion up to twice the requested pixel storage. Returning
to covered pixels preserves textures and avoids repeated source decoding.

Measured 2026-09-28 against `d99823d2`; candidate APK SHA-256
`7056d5dc4c92fcb3b2c07936ad027c901150f8844f1b42643feee6eab779ff5f`.
Source patch, immutable APKs, raw results and traces are under
`artifacts/display-production/source-window-oscillation-*`. The screen-present
accounting limits in the geometry section apply.

## Brushes

Target: **60 completed updates/s** at the guaranteed size, on the 12 MP canvas.

Except for G-Pen, measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets.

- The TCL's work area is 754 px wide, so the harness fits its 520 × 299 px
  ellipse down to 339 × 299 px, at 16.0% zoom.
- Simple brushes are measured at their guaranteed 1024 px.

| Brush (id) | Class | Size | Measured | Status |
| --- | --- | --- | --- | --- |
| G-Pen (1) | Simple | 1024 px | Inside-photo path: 37.8 updates/s (37.4–37.8); gap p99 39.5 ms | **Not met** |
| Rough G-Pen (28) | Simple | 1024 px | 25.6 updates/s (25.5–25.7); gap p99 69.5 ms | **Not met** |
| Calligraphy Pen (29) | Simple | 1024 px | 91.0 updates/s (90.7–91.5); gap p99 37.1 ms | **Not met** |
| Antique Pen (30) | Simple | 1024 px | 37.5 updates/s (37.1–38.0); gap p99 79.9 ms | **Not met** |
| Realistic Pen (31) | Simple | 1024 px | 28.3 updates/s (28.2–28.3); gap p99 63.4 ms | **Not met** |
| Wet Ink (32) | Simple | 1024 px | 23.0 updates/s (22.9–23.1); gap p99 73.7 ms | **Not met** |
| Pencil (2) | Simple | 1024 px | 15.0 updates/s (14.8–15.2); gap p99 99.6 ms | **Not met** |
| Pointy Pencil (25) | Simple | 1024 px | 15.2 updates/s (15.2–15.3); gap p99 96.7 ms | **Not met** |
| Shading Pencil (26) | Simple | 1024 px | 58.3 updates/s (56.2–59.8); gap p99 58.0 ms | **Not met** |
| Charcoal (27) | Simple | 1024 px | 14.2 updates/s (14.2–14.3); gap p99 114.0 ms | **Not met** |
| Chalk (6) | Simple | 1024 px | 10.4 updates/s (10.3–10.5); gap p99 131.3 ms | **Not met** |
| Eraser (3) | Simple | 1024 px | 20.8 updates/s (20.7–21.1); gap p99 72.8 ms | **Not met** |
| Airbrush (5) | Simple | 1024 px | 32.7 updates/s (32.4–33.1); gap p99 55.6 ms | **Not met** |
| Marker (7) | Complex | 1024 px | 16.5 updates/s (16.5–16.6); gap p99 100.5 ms | **Not met** |
| Blotty Ink (33) | Complex | 1024 px | 15.1 updates/s (15.0–15.1); gap p99 105.5 ms | **Not met** |
| Realistic Brushed Ink (34) | Complex | 1024 px | 14.3 updates/s (14.2–14.4); gap p99 178.0 ms | **Not met** |
| Pastel Block (17) | Complex | 1024 px | 13.4 updates/s (13.4–13.4); gap p99 136.0 ms | **Not met** |
| Paintbrush (4) | Complex | 1024 px | 16.6 updates/s (16.5–16.6); gap p99 191.7 ms | **Not met** |
| Textured Flat (15) | Complex | 1024 px | 18.7 updates/s (18.6–18.8); gap p99 169.3 ms | **Not met** |
| Dry Scumble (16) | Complex | 1024 px | 8.8 updates/s (8.7–9.0); gap p99 212.7 ms | **Not met** |
| Transparent Glaze (18) | Complex | 1024 px | 14.7 updates/s (14.6–14.8); gap p99 112.0 ms | **Not met** |
| Multiply Glaze (14) | Complex | 1024 px | 12.9 updates/s (12.8–12.9); gap p99 117.3 ms | **Not met** |
| Dual Texture (9) | Complex | 1024 px | 2.6 updates/s (2.5–2.7); gap p99 559.5 ms | **Not met** |
| Spray (8) | Complex | 1024 px | 7.4 updates/s (7.3–7.7); gap p99 217.2 ms | **Not met** |
| Opaque Gouache (19) | Very complex | 512 px | **Killed** by Android's low-memory killer at 4.2 GB resident | **Not met** |
| Watercolor Wash (20) | Very complex | 512 px | 0.3 updates/s (0.3–0.3); gap p99 3028.5 ms | **Not met** |
| Wet Watercolor (21) | Very complex | 512 px | 0.4 updates/s (0.3–0.4); gap p99 4758.1 ms | **Not met** |
| Loaded Oil (22) | Very complex | 512 px | **Killed** by Android's low-memory killer at 4.3 GB resident | **Not met** |
| Palette Knife (23) | Very complex | 512 px | **Killed** by Android's low-memory killer at 4.1 GB resident | **Not met** |
| Wet Round (11) | Very complex | 512 px | 0.4 updates/s (0.3–0.4); gap p99 3657.4 ms | **Not met** |
| Natural Blender (24) | Very complex | 512 px | 0.5 updates/s (0.5–0.5); gap p99 4221.9 ms | **Not met** |
| Smudge (10) | Very complex | 512 px | 0.5 updates/s (0.5–0.5); gap p99 4519.7 ms | **Not met** |
| Liquify Push (12) | Very complex | 512 px | 0.8 updates/s (0.8–0.8); gap p99 2360.5 ms | **Not met** |
| Liquify Twirl Clockwise (36) | Very complex | 512 px | 12.1 updates/s (11.9–12.3); gap p99 165.0 ms | **Not met** |
| Liquify Twirl Counterclockwise (13) | Very complex | 512 px | 12.3 updates/s (12.1–12.5); gap p99 166.6 ms | **Not met** |
| Liquify Pinch (37) | Very complex | 512 px | 6.9 updates/s (6.6–7.8); gap p99 242.5 ms | **Not met** |
| Liquify Expand (38) | Very complex | 512 px | 7.1 updates/s (7.0–7.2); gap p99 239.8 ms | **Not met** |
| Liquify Crystals (39) | Very complex | 512 px | 1.1 updates/s (1.1–1.1); gap p99 1725.1 ms | **Not met** |

At the 2048 px goal, the G-Pen completes 12.3 updates/s (12.3–12.4), with a gap p99 of 114.1 ms.

G-Pen was remeasured on 2026-09-28 after integrating Clone and color mixing,
with cached composition branches and the validated benchmark harness.
Release Rust APK SHA-256:
`574bac2557e30ce43444a5657de969719052975f285e3d2639e879e3eec7a977`.
The inside-photo trajectory uses 240 × 140 surface-pixel radii and three 5 s
strokes. Both builds use the same instrumentation, with observed setup checked
before timing. The old main `29a564eb` control was measured earlier with the previous
production build. The latest production measurements repeat the same workload;
they are not new alternating pairs against current main.
Thermal status is 0. Each stack has one opaque photo, translucent photo
duplicates at 35% opacity, and a separate active brush layer.

| Photo layers below the brush | Old completed updates/s | Production completed updates/s | Speedup | Production gap p99 |
| --- | --- | --- | --- | --- |
| 1 | 17.91 | 37.77 | 2.11× | 39.5 ms |
| 4 | 1.39 | 35.04 | 25.15× | 41.1 ms |
| 8 | 0.80 | 34.67 | 43.54× | 46.7 ms |

None of these cases reaches the brush target. Raw results are under
`artifacts/display-production/validated-control-stack-{1,4,8}-12mp-fit` and
`artifacts/display-production/mixing-integrated-stack-{1,4,8}-12mp-fit`.
The control APK SHA-256 is
`4106138d62c45d5d5440816f6bc6972fb98fd434a737eb6904fcff56dcf2758f`.

With 32 photos below the brush, the integrated build completes 34.44 updates/s
(34.24–34.69), gap p99 47.3 ms. The preceding production renderer at `e37176cd`
completes 8.96 updates/s on the same setup, a 3.84× improvement. This is a
separate comparison from old main. Raw data is in
`artifacts/display-production/{mixing-integrated,pre-graph}-stack-32-12mp-fit`.
Renderer-reported resident storage is 580, 649, 741 and 1030 MiB for the integrated
build's 1, 4, 8 and 32 photo runs. This includes allocations outside the bounded
display-composition component and is not process RSS.
The earlier cache build had process RSS high-water marks of 1311, 1318, 1469
and 1816 MiB respectively, versus 1323, 1326, 1319 and 1708 MiB before caching.
The latest run has not requalified process memory. Cached
branches trade retained image storage for less repeated composition.
Separate FULL-trace runs attribute 11.05 and 11.23 ms to composition at 8 and
32 photos, respectively, with about 13.8 ms of paint and prediction work in
each. Those instrumentation runs establish phase cost, not target throughput.

The standard larger trajectory partly leaves the photo at Fit. An earlier
production APK (`cff6f839c8fa90fe67df272408ac272b8fd01ba9e5fd631d9a475435dac7d735`)
completes 69.7 updates/s (68.5–69.9), with gap p99 22.9 ms on that trajectory;
it does not qualify sustained painting inside the canvas. Its raw results are
in `artifacts/display-production/final-region-12mp-fit`.
