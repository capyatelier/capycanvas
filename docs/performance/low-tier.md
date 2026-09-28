# Low tier: 60 fps on 12 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: TCL TAB 11 Gen 2 (9465X) with a 1200 × 1920, 60 Hz panel. The canvas
is 4248 × 2832.

- Every row targets **60 fps** unless marked soft.
- The TCL's only transform and UI measurements are canvas-bar benchmark runs on
  a **6000 × 4000** document, twice the tier canvas. Those numbers are
  conservative.

## Operations

| Operation | Target | Measured | Source |
| --- | --- | --- | --- |
| Pan: Hand tool, one or two fingers | 60 | | |
| Pinch zoom | 60 | | |
| Two-finger rotate | 60 | | |
| Navigator drag | 60 | | |
| Brush-cursor hover | 60 | | |
| Placed-photo drag (24 MP photo) | 60 | Renderer 141 submissions/s; GPU completion interval p50/p99 6.7/11.8 ms (6000 × 4000) | Canvas-bar `photo-handle-drag-bar-hidden`, 2026-09-27 |
| Pixel transform handle drag: Free, Uniform, Skew or Rotate | 60 | **Not met.** Renderer 24 submissions/s; owner CPU p50 30 ms; GPU interval p99 82 ms (6000 × 4000) | Canvas-bar `photo-pixels-handle-drag`, 2026-09-27 |
| Pixel transform: Distort or Perspective | 60 | Renderer 130 submissions/s; GPU interval p99 11.4 ms (6000 × 4000) | Canvas-bar `photo-pixels-distort-drag`, 2026-09-27 |
| Pixel transform: Warp | 60 | | |
| Selection transform, full canvas | 60 | Renderer 136–139 submissions/s; GPU interval p99 14.3–17.3 ms (6000 × 4000) | Canvas-bar `selection-handle-drag` and `selection-distort-drag`, 2026-09-27 |
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

## Brushes

Target: **60 completed updates/s** at the guaranteed size, on the 12 MP canvas.

Measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets.

- The TCL's work area is 754 px wide, so the harness fits its 520 × 299 px
  ellipse down to 339 × 299 px, at 16.0% zoom.
- Simple brushes are measured at their guaranteed 1024 px.

| Brush (id) | Class | Size | Measured | Status |
| --- | --- | --- | --- | --- |
| G-Pen (1) | Simple | 1024 px | 37.1 updates/s (37.1–37.4); gap p99 46.5 ms | **Not met** |
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
