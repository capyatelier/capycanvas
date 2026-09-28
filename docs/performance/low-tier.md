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
| Placed-photo translation | 60 | Screen 59.3 presents/s, p99 16.9 ms; renderer 94.4 completed updates/s | Geometry build below, `photo-translate-drag` |
| Placed-photo corner resize | 60 | Screen 59.4 presents/s, p99 16.9 ms; renderer 84.1 completed updates/s | Geometry build below, `photo-handle-drag-bar-hidden` |
| Pixel transform corner resize: Free | 60 | Screen 59.4 presents/s, p99 16.7 ms; renderer 91.0 completed updates/s | Geometry build below, `photo-pixels-handle-drag` |
| Pixel transform: Uniform, Skew or Rotate | 60 | | |
| Pixel transform translation | 60 | Screen 59.4 presents/s, p99 16.7 ms; renderer 93.0 completed updates/s | Geometry build below, `photo-pixels-translate-drag` |
| Pixel transform: Distort | 60 | Screen 59.5 presents/s, p99 16.8 ms; renderer 91.1 completed updates/s | Geometry build below, `photo-pixels-distort-drag` |
| Pixel transform: Perspective | 60 | | |
| Pixel transform: Warp | 60 | **Not met.** Screen 52.4 presents/s, p99 33.4 ms; renderer 51.8 completed updates/s | Geometry build below, `photo-pixels-warp-drag` |
| Crop corner drag | 60 | Screen 59.4 presents/s, p99 16.9 ms; renderer 112.8 completed updates/s | Geometry build below, `crop-handle-drag` |
| Pixel resize after placing the photo at 45% size | 60 | Screen 59.5 presents/s, p99 16.9 ms; renderer 112.2 completed updates/s | Geometry build below, `scaled-photo-pixels-handle-drag` |
| Selection translation, full canvas | 60 | Renderer 136–139 submissions/s; GPU interval p99 14.3–17.3 ms (6000 × 4000) | Canvas-bar `selection-handle-drag` and `selection-distort-drag`, 2026-09-27 |
| Move tool layer drag | 60 | | |
| Marquee, Lasso or Polygon drag | 60 | | |
| Selection Brush or Quick Mask, 1024 px | 60 | | |
| Grow, Shrink or Feather drag, full canvas | 60, soft | | |
| Pointwise adjustment slider: Levels, Curves, Exposure, Hue/Saturation, Color Balance, White Balance, Black & White | 60, soft | | |
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

Geometry build: 2026-09-28, cached composition worktree based on `e6b361dd`,
APK SHA-256 `3158915c6ea69e3d027a7ca3f979410460e8cbfa55b131428e1e77dacca38cd4`.
Release Rust with an unminified Android benchmark harness, default glass, thermal
status 0, three warmed five-second gestures per row. Values are medians across
runs. Screen rates use SurfaceFlinger actual-present timestamps; this device
provides no separate SurfaceView timeline, so they do not independently establish
canvas presentation rates. Renderer counts exclude empty updates and terminal
polling. Setup commands wait for shared command availability outside timing.
Raw results are in `artifacts/display-production/stable-graph-geometry-ready-tcl`.

The paired old renderer at `29a564eb` completes 96.3 placement translations/s,
94.6 placement resizes/s, 92.7 pixel translations/s, 91.2 pixel resizes/s,
91.7 distortions/s, 51.4 warps/s, 123.8 crops/s and 112.0 scaled-photo resizes/s.
Placement resize and crop have regressed in renderer throughput while maintaining
about 59 screen presents/s. Pixel transforms still use the existing executor;
warp does not meet the target. Earlier canvas-bar handle-labelled measurements
started inside the handle and measured translation; they do not qualify resizing
or distortion.

## Brushes

Target: **60 completed updates/s** at the guaranteed size, on the 12 MP canvas.

Except for G-Pen, measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets.

- The TCL's work area is 754 px wide, so the harness fits its 520 × 299 px
  ellipse down to 339 × 299 px, at 16.0% zoom.
- Simple brushes are measured at their guaranteed 1024 px.

| Brush (id) | Class | Size | Measured | Status |
| --- | --- | --- | --- | --- |
| G-Pen (1) | Simple | 1024 px | Inside-photo path: 37.7 updates/s (37.5–37.8); gap p99 36.8 ms | **Not met** |
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

G-Pen was remeasured on 2026-09-28 with cached composition branches and the
validated benchmark harness. Optimized release APK SHA-256:
`da7d5d73452abe36472b446880468ab607686975138a4fe55b7b476445055b22`.
The inside-photo trajectory uses 240 × 140 surface-pixel radii and three 5 s
strokes. Both builds use the same instrumentation, with observed setup checked
before timing. The old main `29a564eb` control was measured in the same session
with the previous production build; the cache build follows those paired runs.
Thermal status is 0. Each stack has one opaque photo, translucent photo
duplicates at 35% opacity, and a separate active brush layer.

| Photo layers below the brush | Old completed updates/s | Production completed updates/s | Speedup | Production gap p99 |
| --- | --- | --- | --- | --- |
| 1 | 17.91 | 37.67 | 2.10× | 36.8 ms |
| 4 | 1.39 | 34.47 | 24.75× | 47.7 ms |
| 8 | 0.80 | 34.59 | 43.44× | 46.7 ms |

None of these cases reaches the brush target. Raw results are under
`artifacts/display-production/validated-control-stack-{1,4,8}-12mp-fit` and
`artifacts/display-production/stable-graph-stack-{1,4,8}-12mp-fit`.
The control APK SHA-256 is
`4106138d62c45d5d5440816f6bc6972fb98fd434a737eb6904fcff56dcf2758f`.

With 32 photos below the brush, the cache build completes 34.25 updates/s
(34.07–34.41), gap p99 48.3 ms. The preceding production renderer at `e37176cd`
completes 8.96 updates/s on the same setup, a 3.82× cache improvement. This is a
separate comparison from old main. Raw data is in
`artifacts/display-production/{stable-graph,pre-graph}-stack-32-12mp-fit`.
Renderer-reported resident storage is 580, 649, 741 and 1030 MiB for the cache
build's 1, 4, 8 and 32 photo runs. This includes allocations outside the bounded
display-composition component and is not process RSS.
Process RSS high-water marks are 1311, 1318, 1469 and 1816 MiB respectively;
the previous production build records 1323, 1326, 1319 and 1708 MiB. Cached
branches trade retained image storage for less repeated composition.
Separate FULL-trace runs attribute 11.05 and 11.23 ms to composition at 8 and
32 photos, respectively, with about 13.8 ms of paint and prediction work in
each. Those instrumentation runs establish phase cost, not target throughput.

The standard larger trajectory partly leaves the photo at Fit. An earlier
production APK (`cff6f839c8fa90fe67df272408ac272b8fd01ba9e5fd631d9a475435dac7d735`)
completes 69.7 updates/s (68.5–69.9), with gap p99 22.9 ms on that trajectory;
it does not qualify sustained painting inside the canvas. Its raw results are
in `artifacts/display-production/final-region-12mp-fit`.
