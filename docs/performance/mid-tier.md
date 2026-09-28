# Mid tier: 90 fps on 24 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: Wacom MovinkPad 11 (DTHA116) with a 1440 × 2200, 90 Hz panel. The
canvas is 6000 × 4000.

- Every row targets **90 fps** unless marked soft.
- **The panel held 60 Hz in the recorded runs.** The canvas-bar benchmark
  reported 60 Hz there, although the app requests the panel's fastest mode with
  `Surface.setFrameRate` and the default display mode is 90 Hz. Wacom's adaptive
  refresh (`setting.adaptive_refresh.enabled`) can hold 60 Hz.
- **Until it presents at 90 Hz, no display-paced row can pass.** Renderer rates
  above 90 show that the rendering has headroom.
- **Huion Kamvas Pad 12 proxy rows.** Rows marked "Huion" come from the Huion
  Kamvas Pad 12: a MediaTek MT8391 with the same Mali-G57 MC2 GPU and a 90 Hz
  panel. Use them only until the MovinkPad 11 is measured.

## Operations

| Operation | Target | Measured | Source |
| --- | --- | --- | --- |
| Pan: Hand tool, one or two fingers | 90 | Every 60 Hz vsync, 4096 px document; GPU p50 6.8 ms Linear, 7.5 ms Perceptual | [Blend space](../internals/rendering.md#blend-space), 2026-09-28 |
| Pinch zoom | 90 | Every 60 Hz vsync, 4096 px document; GPU p50 7.8 ms Linear, 8.5 ms Perceptual | [Blend space](../internals/rendering.md#blend-space), 2026-09-28 |
| Two-finger rotate | 90 | | |
| Navigator drag | 90 | | |
| Brush-cursor hover | 90 | | |
| Placed-photo drag (24 MP photo) | 90 | Renderer 216 submissions/s, GPU p50 4.8 ms Linear; 202 and 5.2 ms Perceptual | [Blend space](../internals/rendering.md#blend-space), 2026-09-28 |
| Pixel transform handle drag: Free, Uniform, Skew or Rotate | 90 | Renderer 181 submissions/s, GPU p50 5.6 ms Linear; 174 and 5.9 ms Perceptual | [Blend space](../internals/rendering.md#blend-space), 2026-09-28 |
| Pixel transform: Distort or Perspective | 90 | Renderer 190 submissions/s, GPU p50 5.6 ms Linear; 180 and 5.9 ms Perceptual | [Blend space](../internals/rendering.md#blend-space), 2026-09-28 |
| Pixel transform: Warp | 90 | | |
| Selection transform, full canvas | 90 | Renderer 217 submissions/s (handle and Distort); worst frame after release 16.4–27.1 ms | `6fcc6fba`, 2026-09-27 |
| Move tool layer drag | 90 | | |
| Marquee, Lasso or Polygon drag | 90 | | |
| Selection Brush or Quick Mask, 2048 px | 90 | | |
| Grow, Shrink or Feather drag, full canvas | 90, soft | | |
| Pointwise adjustment slider: Levels, Curves, Exposure, Hue/Saturation, Color Balance, White Balance, Black & White | 90, soft | | |
| Neighbourhood filter slider: Gaussian Blur, Unsharp Mask, Edge-Preserving Smooth | 90, soft | | |
| Animated or warping filter: Domain Warp, Ripple | 90, soft | | |
| Fill layer or gradient-fill edit | 90, soft | | |
| Navigation with proof or tone guide shown | 90 | | |
| Gradient drag | 90 | | |
| Figure or ruler drag | 90 | | |
| Layer opacity scrub | 90 | | |
| Layer reorder drag | 90 | | |
| Layer swipe right: alpha lock (24 MP photo) | 90 | **Not met.** Android 59.0–59.2 fps, interval p99 16.8 ms; Web 53.1–54.6 fps, interval p99 33.5–50.2 ms | `1d251ece`, 2026-09-27; details below |
| Navigation with 16 visible paint layers | 90 | | |
| Drawing with 16 visible paint layers, G-Pen 1024 px | 90 | | |
| Panel, tab, column or toolbar drag and docking | 90 | **Not met.** Floating panel-group drag frame p50/p95 13.4/15.5 ms | `cbfad9e5`, 2026-09-26 |
| Panel or column resize | 90 | | |
| Drawer open and close | 90 | | |
| Colour wheel or picker drag | 90 | Huion: frame CPU p50 4.6–5.1 ms, p95 under 9.6 ms (hover, 200 Hz pen) | [Colour picker](../ui/color-picker.md), 2026-09-24 |
| Slider scrub: size, opacity, flow | 90 | | |
| Canvas action bar show, hide and move | 90 | **Not met.** UI frame p50: 22.8 ms show and hide, 34.8 ms moving the bar | `cbfad9e5`, 2026-09-26 |
| Tool Options or panel content change | 90 | **Not met.** UI frame p50 21.4 ms | `cbfad9e5`, 2026-09-26 |
| List scrolling: layers, brushes, filters | 90 | | |
| Menu open and close | 90 | | |

Layer swipe measurements use the Wacom MovinkPad 11 at thermal status 0, the
6000 × 4000 reference photo beneath one empty paint layer, Fit zoom and default
panel glass. Each host ran a priming gesture and three five-second moving
gestures. Android uses the benchmark APK with release Rust and native touch
input; its frame timestamps come from `FrameMetrics`. Web uses release Rust,
Chrome 137, injected pen input through DevTools and Chrome's
`AnimationFrame::Presentation` timestamps. The display presents at 60 Hz.
Raw frame data, Chrome traces and fixture details are in
`artifacts/swipe-alpha-lock/`; Android's repeatable entry point is
`AndroidTitleBarTest#layerSwipeFrameTiming` ([layer gesture checks](../ui/drag-and-reorder.md#required-validation-when-implementing)).

## Brushes

Target: **90 completed updates/s** at the guaranteed size, on the 24 MP canvas.

Measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets. The ellipse is 520 × 299 px at 16.0% zoom.

| Brush (id) | Class | Size | Measured | Status |
| --- | --- | --- | --- | --- |
| G-Pen (1) | Simple | 2048 px | 17.2 updates/s (17.0–17.3); gap p99 104.6 ms | **Not met** |
| Rough G-Pen (28) | Simple | 2048 px | 12.2 updates/s (12.0–12.2); gap p99 152.2 ms | **Not met** |
| Calligraphy Pen (29) | Simple | 2048 px | 35.2 updates/s (35.1–35.3); gap p99 73.2 ms | **Not met** |
| Antique Pen (30) | Simple | 2048 px | 16.2 updates/s (16.0–16.3); gap p99 171.2 ms | **Not met** |
| Realistic Pen (31) | Simple | 2048 px | 13.4 updates/s (13.4–13.5); gap p99 131.6 ms | **Not met** |
| Wet Ink (32) | Simple | 2048 px | 11.0 updates/s (10.9–11.1); gap p99 168.6 ms | **Not met** |
| Pencil (2) | Simple | 2048 px | 5.3 updates/s (5.3–5.4); gap p99 225.4 ms | **Not met** |
| Pointy Pencil (25) | Simple | 2048 px | 5.5 updates/s (5.4–5.5); gap p99 222.4 ms | **Not met** |
| Shading Pencil (26) | Simple | 2048 px | 14.6 updates/s (14.5–14.9); gap p99 89.2 ms | **Not met** |
| Charcoal (27) | Simple | 2048 px | 4.4 updates/s (4.3–4.4); gap p99 295.9 ms | **Not met** |
| Chalk (6) | Simple | 2048 px | 3.8 updates/s (3.7–3.8); gap p99 324.4 ms | **Not met** |
| Eraser (3) | Simple | 2048 px | 7.1 updates/s (7.0–7.2); gap p99 174.5 ms | **Not met** |
| Airbrush (5) | Simple | 2048 px | 9.7 updates/s (9.7–9.9); gap p99 135.9 ms | **Not met** |
| Marker (7) | Complex | 1024 px | 22.7 updates/s (22.6–22.8); gap p99 84.3 ms | **Not met** |
| Blotty Ink (33) | Complex | 1024 px | 20.1 updates/s (19.9–20.2); gap p99 90.2 ms | **Not met** |
| Realistic Brushed Ink (34) | Complex | 1024 px | 19.6 updates/s (19.5–19.6); gap p99 136.3 ms | **Not met** |
| Pastel Block (17) | Complex | 1024 px | 18.2 updates/s (18.2–18.4); gap p99 107.3 ms | **Not met** |
| Paintbrush (4) | Complex | 1024 px | 23.2 updates/s (23.2–23.3); gap p99 132.7 ms | **Not met** |
| Textured Flat (15) | Complex | 1024 px | 25.5 updates/s (25.4–25.5); gap p99 107.0 ms | **Not met** |
| Dry Scumble (16) | Complex | 1024 px | 14.0 updates/s (14.0–14.3); gap p99 144.9 ms | **Not met** |
| Transparent Glaze (18) | Complex | 1024 px | 22.0 updates/s (22.0–22.1); gap p99 82.7 ms | **Not met** |
| Multiply Glaze (14) | Complex | 1024 px | 11.5 updates/s (11.4–11.5); gap p99 133.0 ms | **Not met** |
| Dual Texture (9) | Complex | 1024 px | 6.2 updates/s (6.2–6.2); gap p99 201.8 ms | **Not met** |
| Spray (8) | Complex | 1024 px | 5.9 updates/s (5.5–6.5); gap p99 266.9 ms | **Not met** |
| Opaque Gouache (19) | Very complex | 512 px | **Crashed**: native allocator out of memory (Scudo map failure) | **Not met** |
| Watercolor Wash (20) | Very complex | 512 px | **Crashed**: native allocator out of memory (Scudo map failure) | **Not met** |
| Wet Watercolor (21) | Very complex | 512 px | **Crashed**: native abort (SIGABRT) during the stroke | **Not met** |
| Loaded Oil (22) | Very complex | 512 px | **Crashed**: native allocator out of memory (Scudo map failure) | **Not met** |
| Palette Knife (23) | Very complex | 512 px | **Stopped**: canvas GPU out of memory | **Not met** |
| Wet Round (11) | Very complex | 512 px | **Crashed**: native allocator out of memory (Scudo map failure) | **Not met** |
| Natural Blender (24) | Very complex | 512 px | **Crashed**: native allocator out of memory (Scudo map failure) | **Not met** |
| Smudge (10) | Very complex | 512 px | **Stopped**: canvas GPU out of memory | **Not met** |
| Liquify Push (12) | Very complex | 512 px | 1.2 updates/s (1.2–1.3); gap p99 3288.4 ms | **Not met** |
| Liquify Twirl Clockwise (36) | Very complex | 512 px | 6.6 updates/s (6.3–7.1); gap p99 228.1 ms | **Not met** |
| Liquify Twirl Counterclockwise (13) | Very complex | 512 px | 6.4 updates/s (6.2–6.5); gap p99 236.1 ms | **Not met** |
| Liquify Pinch (37) | Very complex | 512 px | 3.8 updates/s (3.8–4.1); gap p99 429.0 ms | **Not met** |
| Liquify Expand (38) | Very complex | 512 px | 3.8 updates/s (3.7–3.9); gap p99 456.8 ms | **Not met** |
| Liquify Crystals (39) | Very complex | 512 px | 1.4 updates/s (1.4–1.4); gap p99 2078.1 ms | **Not met** |
| Clone Stamp (40) | Very complex | 512 px | 57.8 updates/s (57.6–58.4); gap p99 31.1 ms | **Not met** |
| Healing Brush (41) | Very complex | 512 px | 57.4 updates/s (57.2–57.4); gap p99 31.4 ms | **Not met** |
| Spot Healing Brush (42) | Very complex | 512 px | 62.1 updates/s (61.5–62.8); gap p99 27.9 ms | **Not met** |

The retouching brushes were measured on 2026-09-28 at the commit that made them contact brushes, copying from the photo marked as a reference layer.
