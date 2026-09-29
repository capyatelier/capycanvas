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
| Placed-photo drag (24 MP photo) | 90 | screen 59.0/s; renderer 99.4 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Pixel transform handle drag: Free, Uniform, Skew or Rotate | 90 | Free: screen 59.2/s; renderer 182.1 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Pixel transform: Distort or Perspective | 90 | Distort: screen 59.2/s; renderer 168.2 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Pixel transform: Warp | 90 | screen 59.0/s; renderer 72.5 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
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
| Clone Stamp (40) | Very complex | 512 px | 115.7 fresh updates/s; completion gap p99 15.2 ms | **Met for drawing** |
| Healing Brush (41) | Very complex | 512 px | 113.7 fresh updates/s; completion gap p99 15.2 ms | **Met for drawing** |
| Spot Healing Brush (42) | Very complex | 512 px | 119.7 fresh updates/s; completion gap p99 13.6 ms | **Met for drawing** |

Retouching rows use the integrated-compositor measurements below, copying from the photo marked as a reference layer.

## Retouching with the integrated compositor

Measured on 2026-09-29 on the reference tablet: 24 MP Perceptual photo,
512 px brushes, Fit zoom, 479.7 × 240 px trajectory, 16 ms prediction, three
five-second strokes per brush. Stats is closed. Fresh
updates count completed frames that consumed new real pen samples.

| Brush | Fresh updates/s, median | Completion gap p99, median | Moving-stroke rate |
| --- | ---: | ---: | --- |
| Clone Stamp | 115.7 | 15.2 ms | Meets 90/s |
| Healing Brush | 113.7 | 15.2 ms | Meets 90/s |
| Spot Healing Brush | 119.7 | 13.6 ms | Meets 90/s |

These measurements cover drawing. Clone settles in 470–566 ms; Healing in
1633–1830 ms and Spot Healing in 2702–2821 ms. Completion after pen-up remains an
open gate. They do not qualify presentation-paced navigation or the other
brushes. The measured build integrates `64036b643` with `13720d303`, bounded
bakes, the healing solver optimization and sparse stroke replay. Its APK SHA-256
is `be9a9ad367b9a0215ea5f57ac9d14028818d1f029fe8787324b66d3d4ac71e48`.
Raw reports are in `artifacts/integration/source-record-batch/mid`.

## Transform presentation

Measured 2026-09-29 on the 24 MP Perceptual reference photo at Fit, release Rust,
default glass and Stats closed. Each row contains three warmed five-second
gestures, with thermal status zero. Values are medians across runs. The harness
sets Navigator visibility explicitly and asserts the renderer's overview count.

| Journey | Navigator closed, completed updates/s | Navigator open, completed updates/s |
| --- | ---: | ---: |
| Placed-photo translation | 99.42 | 82.08 |
| Placed-photo resize, bar hidden | 90.87 | 79.23 |
| Placed-photo resize, bar visible | 90.64 | 78.90 |
| Pixel transform translation | 186.54 | 79.90 |
| Pixel transform resize: Free | 182.14 | 77.72 |
| Pixel transform: Distort | 168.17 | 73.10 |
| Pixel transform: Warp | 72.48 | 69.36 |

Median screen rates are 58.88–59.19 presents/s with Navigator closed and
58.87–59.20 with it open; median screen interval p99 is 16.78–16.81 ms.
These rates do not qualify the 90 Hz tier: the panel presents at 60 Hz.
SurfaceFlinger screen timestamps do not independently establish canvas scanout.

Single-input affine and perspective previews sample their retained source
directly into presentation when Navigator is closed. A visible Navigator shares
a materialized composition with the main canvas to preserve area-filtered
preview quality. Opening its panel also changes the Fit camera and work area;
open/closed rates are different workloads, not an isolated panel-cost estimate.

Closed-panel build: `e41ee80fe` plus direct transform presentation, Navigator
materialization and placement refinement scheduling, benchmark APK SHA-256
`ede35aa5b23bd8770d1207929e1b633a25b481f7987eb7201645125a11e013e3`.
Raw runs: `artifacts/latency-investigation/deferred-9-mid-closed-fixed`.
Open-Navigator rows also include fragment materialization and source-specific
main-view shader entries, APK SHA-256
`1dbac0ef1dcf9208709ba3e7df71f0a028e99d6d25e4c60356e7413a270f1208`.
Those runs are in `present-18-mid-navigator`, including its `extra` directory.
Earlier default-panel runs retained Navigator;
they are not evidence for the closed-panel workload.

## Input during Healing finalization

MovinkPad 11 navigation during large Healing/Spot Healing finalization has
input-queue p95 of 15.2–18.4 ms and a maximum of 28.8 ms. The largest settle
callback is 51.7 ms. The one-second pinch probes present at 56.5–58.6/s on the
60 Hz panel; the 90 Hz tier target remains unqualified. System available memory
stays above 1,358 MiB.

Measured on 2026-09-29, three runs per brush on the tier photo, 512 px,
Perceptual, Fit and Stats closed. Dependent painting queues until the healed
raster publishes. The [responsiveness record](responsiveness.md#healing-finalization)
contains the build, workload, tool-action limits and raw records.
