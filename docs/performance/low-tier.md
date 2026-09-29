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
| Pan: Hand tool, one or two fingers | 60 | Generated group: screen 59.4 presents/s, p99 16.7 ms; renderer 59.8 completed updates/s. Photo coverage pending | Pass Through navigation below |
| Pinch zoom | 60 | Generated group: screen 59.4 presents/s, p99 16.8 ms; renderer 59.8 completed updates/s. Photo coverage pending | Pass Through navigation below |
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
| Pointwise adjustment slider: Exposure | 60, soft | **Not met.** Screen 52.7 presents/s, p99 33.4 ms; renderer 30.8 completed updates/s | Pointwise graph comparison below |
| Pointwise chain: Levels, Vibrance, Exposure slider | 60, soft | **Not met.** Screen 57.3 presents/s, p99 33.3 ms; renderer 25.1 completed updates/s, with a 13.9–31.7 range | Pointwise graph comparison below |
| Other pointwise adjustment sliders | 60, soft | | |
| Gaussian Blur slider, small radius | 60, soft | **Not met.** Screen 41.6 presents/s, p99 33.5 ms; renderer 30.4 completed updates/s | Spatial graph comparison below |
| Gaussian Blur slider, large radius | 60, soft | **Not met.** Screen 28.7 presents/s, p99 66.6 ms; renderer 17.7 completed updates/s | Spatial graph comparison below |
| Other neighbourhood filter sliders: Unsharp Mask, Edge-Preserving Smooth | 60, soft | | |
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

## Pass Through navigation

Measured 2026-09-28 on a 4248 × 2832 generated solid-color fill with a Black &
White adjustment inside a Pass Through group. Three warmed five-second touch
gestures per motion use release Rust, default glass and thermal status 0 before
and after. These measurements do not qualify photo-backed or multilayer navigation
and have no matched old-renderer control.

| Motion | Completed updates/s | Completion gap p99 | Screen presents/s | Screen gap p99 |
| --- | --- | --- | --- | --- |
| Pan | 59.79 | 18.9 ms | 59.39 | 16.7 ms |
| Pinch | 59.79 | 20.6 ms | 59.39 | 16.8 ms |

Screen cadence meets the 57 presents/s measurement floor for this workload.
The screen-present accounting limits above apply. Renderer-owned storage is
211.7 MiB for pan and 229.6 MiB for pinch; neither is process RSS. Median last
completion after input ends is 74 ms and 29 ms respectively.

Candidate is `cbcf0aec` plus the shared Pass Through traversal port, APK SHA-256
`3cd483d10defd2ffc9cd8115dc2329173dbc5b70054f9dd4ac7d99901b539d49`.
Raw runs, traces and source provenance are under
`artifacts/display-production/pass-through-ready-tcl` and
`pass-through-ready-provenance.json`. Display-frame records identify screen
presents: the pinch trace omits the SurfaceFlinger process name, but its display
frame tokens and PID match the adjacent named pan trace.

## Pointwise filter composition

Measured 2026-09-28 on the 12 MP photo at Fit, with three alternating pairs of
warmed five-second stylus scrubs, release Rust, default glass and thermal status
0. The harness closes panel configuration, waits for shader readiness after
priming and verifies changing parameter values during half-second triangle
motion. Both builds use the same camera and test APK.

| Slider journey | Previous filter executor, completed updates/s | Display graph, completed updates/s | Speedup | Graph completion gap p99 |
| --- | --- | --- | --- | --- |
| Exposure | 1.59 | 30.84 | 19.36× | 52.6 ms |
| Levels, Vibrance, Exposure chain | 1.19 | 25.08 | 20.99× | 70.1 ms |

Neither journey meets the motion target. Screen presents include native controls
and do not independently establish canvas presentation cadence: the chain's
screen rate includes slider-only changes between completed canvas updates.
Exposure ranges from 29.86 to 31.69 completed updates/s. The chain ranges from
13.94 to 31.66, with individual completion-gap p99 values of 63.6, 285.4 and
70.1 ms. The slow repeat remains in the results; thermal status stays zero and
its viewport GPU cost remains comparable. Renderer-owned storage falls from
579.8 to 306.5 MiB; this is not process RSS.

A separate profile measures composition at 5.6 ms for Exposure and 7.9 ms for
the chain, plus viewport observations of 8.2 and 8.5 ms. Normal-run traces also
show substantial native model publication work: 419 calls consume 2.46 s in one
chain gesture and 3.16 s in the slower repeat. GPU cost alone does not explain
the journey rate; shared state publication and scheduling remain optimization
work. The profiled captures verify both themes with panel configuration closed.

The control is production `7cad88dd`, before pointwise graph admission, using APK
SHA-256 `574bac2557e30ce43444a5657de969719052975f285e3d2639e879e3eec7a977`.
Candidate is the spatial graph build based on `2de3d0ca`, APK SHA-256
`f5dda5c758ce1e79294cb2ad86b5df7572c930d6dcc068dba80d5e5ca9d3e1e1`.
Raw results and traces are under `artifacts/display-production/pointwise-workspace-paired`.
Source and harness hashes are in `spatial-hardware-provenance.json` and
`spatial-workspace-provenance.json`. Earlier pointwise runs used an expanded
panel configuration and are superseded by this closed-panel fixture.
These compare filter execution within the production branch, not against a fresh
old-main build. Other filters and high-frequency preview quality remain unqualified.

## Spatial filter composition

Measured 2026-09-28 on the 12 MP photo at Fit, with three alternating pairs of
warmed five-second stylus scrubs. Both builds use release Rust, the same test
APK, default glass, zoom 0.1769915 and thermal status 0 before and after every
run. The harness closes panel configuration, waits for shader readiness and
records changing parameter values strictly inside the slider limits. Its
half-second triangle motion avoids a slow gesture's numeric-step cadence limit.

| Gaussian radius range | Previous executor, completed updates/s | Display graph, completed updates/s | Speedup | Graph completion gap p99 |
| --- | --- | --- | --- | --- |
| 2.6–4.9 document pixels | 1.21 | 30.37 | 25.08× | 56.5 ms |
| 14.2–16.5 document pixels | 0.39 | 17.71 | 45.68× | 93.7 ms |

Neither journey meets the motion target. Screen presents include native controls
and do not independently establish canvas presentation cadence. Renderer-owned
storage falls from 894.9 to 318.0 MiB; this is not process RSS. These compare
filter execution within the production branch, not against a fresh main build.

The graph evaluates 1062 × 708 pixels instead of 4248 × 2832, and prepares its
separable kernel at that scale. This reduces the paired sample count by roughly
48–55× over these radii. Two RGBA32Float passes still read and write at least
48.1 MB, giving an optimistic copy floor of 4.8 ms at the device's calibrated
10 GB/s. This floor excludes additional texture reads, shader arithmetic,
composition and presentation; it is not a filter-throughput prediction.

A separate profiled run gives submission-aligned composition medians of
16.1 ms at small radius and 43.2 ms at large radius. Viewport GPU observations
have medians of 9.5 and 9.6 ms respectively. These costs explain the frame-budget
miss; they do not establish a hardware limit or qualify a soft-target waiver.

Control is production `2de3d0ca`, APK SHA-256
`3cd483d10defd2ffc9cd8115dc2329173dbc5b70054f9dd4ac7d99901b539d49`.
Candidate is the spatial graph build based on that commit, APK SHA-256
`f5dda5c758ce1e79294cb2ad86b5df7572c930d6dcc068dba80d5e5ca9d3e1e1`.
Raw runs and traces are under `artifacts/display-production/spatial-workspace-paired`
and `spatial-workspace-profile`. Source and harness hashes are in
`spatial-hardware-provenance.json` and `spatial-workspace-provenance.json`;
the operation model is `spatial-cost-model.json`. Light and dark captures verify
the closed-panel photo fixture. Earlier runs with panel configuration expanded
or a zero-clamped or slow cosine trajectory are diagnostic only.

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
| G-Pen (1) | Simple | 1024 px | Inside-photo path: 37.5 updates/s (36.9–37.7); gap p99 37.6 ms | **Not met** |
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

The G-Pen row uses production `7a554597`, rebased onto main's retouch changes,
measured 2026-09-28 with three five-second strokes, 240 × 140 surface-pixel radii,
16 ms prediction and thermal status 0 before and after. Renderer-owned storage
is 580.2 MiB. Raw data and the trace are under
`artifacts/display-production/spatial-rebased-gpen`. APK SHA-256 is
`1b410def35d6010cc992d81a9df85f334c8b8c696bcf6d4c6db784d69574050e`.
The preceding spatial build measured 38.0 updates/s (37.6–38.0); these are
successive runs, not alternating pairs. Neither meets the target.

At the 2048 px goal, the earlier G-Pen build completes 12.3 updates/s (12.3–12.4), with a gap p99 of 114.1 ms.

Retouch brushes measured on 2026-09-28 at production `7a554597`, after integrating
main `9deeafba`'s Clone, Healing and Spot Healing optimization. Each tool uses
three five-second inside-photo strokes, 512 px diameter and 16 ms prediction.
Thermal status is zero before and after each tool. These are current-path
measurements; there is no matched old-renderer comparison.

| Tool | Completed updates/s | Update-start gap p99 | Last completion after input ends | Drawing target |
| --- | --- | --- | --- | --- |
| Clone Stamp | 63.58 (62.49–63.95) | 24.0 ms | 88 ms (84–95) | Met |
| Healing Brush | 62.71 (61.59–62.73) | 23.8 ms | 1669 ms (1616–1670) | Met |
| Spot Healing Brush | 66.78 (66.03–68.53) | 23.9 ms | 2330 ms (2322–2336) | Met |

Drawing meets the rate and gap criteria; the long Healing and Spot Healing
release delays remain unresolved. Completion rates exclude work after the input
window. Release timings include queued input, processing and GPU observation;
they do not isolate the healing solver. Every submitted frame finishes before
the drained snapshot. Renderer-owned storage is 536, 767 and 767 MiB respectively,
not process RSS. This is the same APK as the G-Pen row above. Raw results are in
`artifacts/display-production/spatial-rebased-retouch`.

The preceding production build measured 19.56, 19.35 and 29.26 updates/s for these
tools, with release tails of 148, 1661 and 3879 ms. Its results are preserved in
`artifacts/display-production/healing-finish-retouch-12mp-fit`. The new throughput
reflects integration of main's retouch changes; it does not isolate a composition
algorithm speedup.

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


## Viewport-window composition

Measured on 2026-09-28 with the same TCL 12 MP photo, G-Pen 1024 px,
240 × 140 surface-pixel ellipse, 16 ms prediction and default glass. Three
five-second gestures alternate control and candidate order; all 36 thermal
snapshots report status 0. These are completed renderer updates during input.

The control is the production spatial-filter build at `7a554597`. The candidate
is based on `25b4c2f9`, with bounded scratch at every pointwise display scale,
viewport source windows, adjacent source-detail preparation, active-paint priority
and fixed-width source sample decoding. Its APK SHA-256
is `73242ba8b3b5bdfaeea905b9725c7f6b48c6d4284b8a38ca73960e1273d88c9b`;
its source patch SHA-256 is
`c558ecafcdb87850f062a2ec0981df00c4ac10baa60153842cc0f05ce5ce2320`.
Raw pairs and the summary are under
`artifacts/display-production/source-decode-paired` and
`artifacts/display-production/source-decode-paired-summary.json`.

| Photo layers below the brush | Zoom | Control updates/s | Candidate updates/s | Speedup |
| --- | --- | --- | --- | --- |
| 8 | 50% | 1.792 | 28.247 | 15.76× |
| 8 | 100% | 1.793 | 18.714 | 10.44× |
| 1 | Fit | 37.883 | 46.567 | 1.23× |

All three miss 60 updates/s. Update-start gap p99 ranges across the candidate's
three runs are 45.7–50.6 ms, 62.4–71.6 ms and 31.7–35.4 ms respectively.
Prioritizing required paint over optional first-stroke detail removes the earlier
candidate's Fit and native painting regression. That comparison remains in
`artifacts/display-production/source-admission-paired-summary.json`.
These runs precede streamed source preparation and do not qualify its performance
or photo navigation.

## Perceptual composition

Measured on the TCL after integrating perceptual layer blending, using the same
12 MP photo, inside-photo G-Pen 1024 px trajectory, 16 ms prediction and three
warmed five-second strokes per case. The benchmark explicitly selects and checks
the document blend space before timing. Candidate is `810a125d` plus the graph
color-representation integration, APK SHA-256
`885ce8bc0de36f46bcb322d462c12b843f8dd567807b6fe927e1a5d60755ec29`.

| Photo layers | Zoom | Perceptual completed updates/s |
| --- | --- | --- |
| 1 | Fit | 46.36 |
| 8 | 50% | 26.65 |
| 8 | 100% | 17.89 |

These are current-path measurements without an alternating old-compositor control;
they do not establish a speedup or reach the long-term 60 updates/s target.
They precede perceptual dab blending and exact idle refinement. Raw reports,
traces and source provenance are under `artifacts/display-production/perceptual-graph-tcl`
and `perceptual-graph-provenance.json`.
