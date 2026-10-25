# Top tier: 120 fps on 61 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: Wacom MovinkPad Pro 14 (DTHA140) with a 1800 × 2880, 120 Hz OLED. The
canvas is 9504 × 6336. Every row targets **120 fps** unless marked soft.

## Navigation presentation

Measured 2026-10-24 on the reference Wacom MovinkPad Pro 14, comparing
`2d7c4d5d7` with the presentation-admission and renderer-cache fix. Both APKs
use optimized Rust release code and the same benchmark host; the final APK is
`55febd3740001e0ca1f89c1761e1995ade72ea5bb8b8aa96da497680c67b623d`.
Each row has three independent eight-second moving contacts, warm required
shaders, actual 120 Hz display mode and thermal status 0. Rates are medians of
SurfaceFlinger canvas presentations, independently matched to its latency ring.
The photo is the tier's 9504 × 6336 reference at Fit. The default-paper rows use
the startup camera, Sketch workspace and Coldpress Naturalwhite at strength 100;
they supplement the tier-photo measurements. Navigator conditions remain separate.

| Workload and motion | Baseline fps | Fixed fps | Gain | Worst contact gap p99, ms | 120 Hz rate and gap target |
| --- | --- | --- | --- | --- | --- |
| Default 2400 × 1800 paper, rotate | 35.79 | 119.25 | +233.2% | 8.337 | Met |
| Default 2400 × 1800 paper, pinch | 38.79 | 118.99 | +206.8% | 8.544 | Met |
| 61 MP, Navigator closed, pan | 32.50 | 118.74 | +265.4% | 8.338 | Met |
| 61 MP, Navigator closed, rotate | 30.28 | 118.87 | +292.5% | 8.501 | Met |
| 61 MP, Navigator closed, pinch | 38.87 | 105.84 | +172.3% | 16.668 | Not met |
| 61 MP, Navigator open, pan | 36.78 | 119.00 | +223.6% | 8.336 | Met |
| 61 MP, Navigator open, rotate | 33.32 | 119.12 | +257.5% | 8.541 | Met |
| 61 MP, Navigator open, pinch | 35.40 | 111.74 | +215.7% | 16.668 | Not met |

The original presentation shader, paper, filtering and quantization remain
unchanged. Seven of eight matched resting captures are byte-identical; photo
pan with Navigator closed differs by one code value in 44,074 pixels, with a
measured camera drift below 0.003 screen pixels. These captures do not establish
pixel identity at every moving pose.

Presentation occupies more time on the canvas owner. Default rotate input-queue
p95 is 8.38–8.61 → 6.65–8.12 ms, default pinch is 5.52–8.10 → 8.31–8.74 ms,
and closed-Navigator photo pinch is 7.71–8.29 → 9.92–10.11 ms. Throughput
improves substantially; a general input-latency improvement is not established.
Photo pinch still misses the 114 presentations/s floor in both Navigator states.
Per-contact rates, GPU times, camera/paper state, captures, traces and APK/source
provenance are under `artifacts/nav-fix-qualification-20261024/`.

### Brush and pending-filter controls

The same build comparison uses G-Pen 2048 px on the 61 MP reference photo at
Fit, Navigator closed, pressure 1 and physical inclination 40°, with nominal
200 Hz input. Fresh input-attributed GPU completions, rather than presented
frames, are the brush metric. The three baseline contacts are
114.82 / 118.74 / 119.49 updates/s; fixed contacts are
119.21 / 123.78 / 118.52 updates/s. Completion-gap p99 improves from
17.27 / 20.55 / 16.28 to 15.30 / 15.09 / 16.11 ms. All final gaps meet the
limit, but two contacts remain below the full 120 updates/s requirement.
Latest-input age p99 is variable, with no established improvement.

An interruption probe begins a nominal one-second pinch while attached Gaussian
Blur 85 has verified pending composition. Actual gesture wall time is
15.72–15.94 s in the baseline and 15.97–16.15 s with the fix; some displayed
gaps are about 15.9 s. Native frame records locate the long owner call before
surface acquisition in the shared session-frame call inside frame preparation, rather than in the
presentation call. Its internal cause remains unqualified. This probe is a
response failure in both builds and does not establish sustained filter FPS.

## Operations

### Checked upload setup failure

The 2026-10-25 comparison of `e038e3361` with checked upload growth at
`c1b215667` does not reach motion on the reference 61 MP photo. The baseline
waits 600 seconds and a bounded candidate observation waits 90 seconds without
finishing composition. Canvas, brush and shader readiness are true, four GPU
frames are submitted/completed, and the photo remains invisible. Heavy source
cache churn supports a preparation-progress problem; the exact active branch
was not recorded. No renderer crash or upload-guard regression is established.

These are optimized release/R8 builds on the reference tablet with factory
display settings. No top-tier brush rate is available for this comparison.
Exact APK/source proof, state, counters and screenshots are retained under
`/mnt/localssd/capycanvas9-stability-android-artifacts/ad5-reference/`. Real-device
composition progress must pass before those motion rows can be measured.

Required-only shader startup and first-use tool preparation are unmeasured on
this reference device. Earlier idle-catalog measurements describe their named
builds, not the current startup policy. See [startup preparation](responsiveness.md#demand-driven-startup)
for the Apple diagnostic and its limits.

The M3 cutover is committed at `b3f6f8e51`. The earlier M3 G-Pen comparison uses
frozen baseline BUILD29 and M3 BUILD32 binaries based on `192601dac`; their
forward and reverse results are [below](#current-m3-g-pen-comparison).
Both miss the tier target, and order-dependent CPU and response bounds remain
unresolved. BUILD20 canvas comparisons use `4a2cf6aa0`; earlier operation,
effect and transform rows apply only to their named binaries and retain their
presentation and scope limits.

Overall M3 performance qualification remains pending. Matched release offscreen
runs on the desktop RTX PRO 6000 compare frozen `192601dac` with the authored
candidate. Navigation CPU p95 improves in every comparison, with unchanged
camera/work/cache counters and zero source misses or recomposition. Completed
p95 exceeds 5% in 7/15 cold and 8/15 warmed observations; warmed native-scale
excesses repeat in all three pairs (+0.024–0.178 ms). All p99 shifts stay below
+1 ms. These completion spans include host queue polling, not GPU timestamps.

The 100-repeat sixteen-layer bottom-paint ABBA comparison retains exact PNGs,
frame/work counters and 110,624,776 bytes of capture backing. CPU-submit bounds
pass both temporal pairs. The reverse pair exceeds completed-motion p95 by
7.09% (+0.041 ms), loses 6.23% throughput and adds 1.025 ms to pen-up p99;
the forward pair passes. These excesses remain unresolved and do not qualify
reference-tablet rates or physical input-to-present response. Exact observations
are under `artifacts/format/m3-uninstrumented-34/` and
`artifacts/format/m3-final192-ordinary-fixture/measurements/`. Earlier passing
`4a2cf6aa0` diagnostics remain in their own artifacts and do not override these
current-source results.

The 2026-10-08 navigation rows use three 5 s gestures on the 61 MP photo,
actual SurfaceFlinger presentations, and thermal status 0. The benchmark
opened the 8-bit sRGB JPEG directly, whereas low/mid tiers place their photos
in new documents; its native tab reports 9504 × 6336 with one empty paint
layer above it. The display is `Rgba8UnormSrgb` in `Srgb`. The brush/canvas and matching gesture were warmed;
optional full-catalog `shaders_ready` was false. Source: `067f5cbb7` plus the
uncommitted navigation candidate (`navigation.rs` SHA-256
`2c0e9c5a7eee70c3e881ba9810a286cbe5a49ca1059d23a18a3892c8d1354d79`);
optimized APK SHA-256 `e74cd6b9b8f0dc15e1beb2f5e6ff6348bae20025ebc6e1f6adbd9dca7682d462`.

| Operation | Target | Measured | Source |
| --- | --- | --- | --- |
| Startup and first-use shader preparation | [Latency limits](responsiveness.md) | Unmeasured with required-only startup on this reference device | [Apple diagnostic](responsiveness.md#demand-driven-startup); no tier qualification |
| Tool cursor hover, with and without brush size | 120 | Renderer 119.3–119.6 submissions/s; submission interval p99 15.0–15.6 ms. Presentation unqualified. | [Tool cursors](#tool-cursors) |
| Pan: Hand tool, one or two fingers | 120 | **Met for two-finger pan.** 118.87–119.50 presented fps, worst p99 8.336 ms, 61 MP photo, Navigator open | [Navigation presentation](#navigation-presentation), 2026-10-24 |
| Pinch zoom | 120 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: **Not met.** 111.73–112.09 presented fps, p99 up to 16.668 ms, 61 MP photo, Navigator open; default paper 118.74–119.00 fps | [Navigation presentation](#navigation-presentation), 2026-10-24 |
| Mouse-wheel pan and Ctrl-wheel zoom, including held navigation buttons | 120 | Unmeasured on the reference tablet | [Wheel input contract](../ui/shared-ui.md); desktop correctness checks do not qualify this tier |
| Two-finger rotate | 120 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: **Met.** 118.74–119.37 presented fps, worst p99 8.541 ms, 61 MP photo, Navigator open; default paper 119.00–119.37 fps | [Navigation presentation](#navigation-presentation), 2026-10-24 |
| Rotation snapping: two fingers and Rotate View | 120 | Unmeasured on the reference tablet | [Shared behavior](../ui/shared-ui.md#drawing-and-navigation); [GTK diagnostic](#gtk-rotation-snapping-diagnostic) |
| Smooth Zoom tool, left/right | 120 | **Misses:** 35.75–42.09 presented fps, p99 50.00–58.34 ms, 61 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized 1.0.12 APK `ef20cbb3`, navigation `cbcee39a`, base `ee4805dfe`; `artifacts/navigation-controls/zoom-audit-20261009/post-rebase/top/` |
| Smooth Zoom tool, up/down | 120 | **Misses:** 34.56–43.73 presented fps, p99 50.00–58.34 ms, 61 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized 1.0.12 APK `ef20cbb3`, navigation `cbcee39a`, base `ee4805dfe`; `artifacts/navigation-controls/zoom-audit-20261009/post-rebase/top/` |
| Painting with Filters previews pending (61 MP) | 120 | One frame-gap outlier; repeat passes. Completed-update rate unqualified. Curves thumbnail opening 6.782 → 0.943–1.015 s | [Filters previews](#filters-previews), 2026-10-06 |
| Footer zoom and rotation sliders | 120 | Unmeasured on the reference tablet with slider snapping enabled | [Shared slider behavior](../ui/shared-ui.md#drawing-and-navigation) |
| Navigator drag | 120 | **Web diagnostic:** 60.50–64.92 Chrome surface presents/s; p99 25.00–33.33 ms on the 61 MP photo. Target unqualified; native unmeasured | [Web Navigator layout](#web-navigator-layout), 2026-10-19 |
| Pan with Navigator open, checkerboard shader | 120 | Diagnostic: 34.92–39.12 renderer submissions/s; submission interval p99 37.11–39.52 ms. Presented FPS unmeasured; target unqualified | 2026-10-17, three warmed 5 s gestures, 61 MP tier photo, thermal 0; release APK `e38cf984`, base `5cee14be0` plus shared white/gray checker colors; `artifacts/checker-android/top/` |
| Brush-cursor hover | 120 | | |
| Placed-photo drag (24 MP photo) | 120 | | |
| Retained photo translation with snapping (61 MP) | 120 | **Not met.** 59.29–59.87 completed updates/s; matched snapping-off run 59.46–59.71/s | [Transform snapping](#transform-snapping), 2026-10-02 |
| Retained dry-photo Transform body drag (61 MP) | 120 | **Not met.** 93.68–95.48 renderer-completed updates/s; completion gap p99 20.70–21.58 ms; canvas presentation unmeasured | [Layer reorder and retained photo translation](#layer-reorder-and-retained-photo-translation), 2026-10-04 |
| Imported dry-photo translation, sparse neighbor, snapping off (61 MP) | 120 | **Not met.** BUILD20 M3 64.252–65.299 completed updates/s; completion gap p99 24.150–24.576 ms; presentation unmeasured | [BUILD20 selected transforms](#build20-selected-imported-photo-transforms) |
| Imported dry-photo Distort, sparse neighbor, snapping off (61 MP) | 120 | **Not met.** BUILD20 M3 60.489–61.278 completed updates/s; completion gap p99 26.172–27.025 ms; baseline setup fails | [BUILD20 selected transforms](#build20-selected-imported-photo-transforms) |
| Imported dry-photo Warp, sparse neighbor, snapping off (61 MP) | 120 | **Not met.** BUILD20 M3 18.961–19.375 completed updates/s; completion gap p99 88.404–91.979 ms; presentation unmeasured | [BUILD20 selected transforms](#build20-selected-imported-photo-transforms) |
| Retained wet-photo Transform body drag (61 MP) | 120 | **Not met.** 36.3–37.0 renderer updates/s; presentation unmeasured | [Material transforms](#retained-wet-photo-transforms), 2026-10-02 |
| Retained wet-photo Distort corner drag (61 MP) | 120 | **Not met.** 29.37 completed updates/s, warm median; presentation unmeasured | [Retained Distort and Warp](#retained-distort-and-warp), 2026-10-02 |
| Retained wet-photo Warp node drag (61 MP) | 120 | **Not met.** 15.97 completed updates/s, warm median; presentation unmeasured | [Retained Distort and Warp](#retained-distort-and-warp), 2026-10-02 |
| Imported dry-photo Warp, three visible layers, snapping on (61 MP) | 120 | **Not met.** 19.15–19.47 renderer submissions/s; completed-update interval p99 87.88–90.37 ms; presentation unmeasured | [Folded warp source planning](#folded-warp-source-planning), 2026-10-04 |
| Pixel transform handle drag: Free, Uniform, Skew or Rotate | 120 | | |
| Pixel transform: Distort or Perspective | 120 | | |
| Pixel transform: Warp | 120 | | |
| Selection transform, full canvas | 120 | | |
| Move tool layer drag | 120 | | |
| Object Layer placement, Move and Scale/Rotate | 120 | Current Object-layer simplification unmeasured on the reference tablet | [Qualification gap](known-gaps.md) |
| Vector layer: G-Pen 24 px on 50,000 paths | 120 | **Met.** 239.4–244.8 completed updates/s, gap p99 12.8 ms | [Vector layers](#vector-layers), 2026-10-21 |
| Vector paths: Move drag, 1 / 100 / 5,000 paths | 120 | **Met.** 119.8–120.0 presented/s, p99 ≤8.5 ms, start 40–80 ms | [Vector layers](#vector-layers), 2026-10-20 |
| Vector paths: Scale/Rotate, eraser, Outline width scrub | 120 | **Met** except the 5,000-path width scrub (29.6–32.1 updates/s); Edit Points **met** | [Vector layers](#vector-layers), 2026-10-21 |
| Navigation over a crowded vector layer | 120 | **Not met.** Pan 27.7–28.8, pinch 36.8–38.9 (21.1–21.4 with 5,000 paths selected), rotate 25.6–26.1 presented/s | [Vector layers](#vector-layers), 2026-10-20 |
| Marquee, Lasso or Polygon drag | 120 | Met on a small document: in-stroke interval p50/p99 4.2/6.9 ms with the canvas bar shown, p99 8.8 ms with it off (2048 × 1536) | `ba9483a8`, 2026-09-27 |
| Enclose and Fill: loop drag and navigation during completion | 120 | Unmeasured on the reference tablet; 61 MP completion exceeds the current 128 MiB component-buffer binding limit | Shared lasso overlay; GPU region discovery starts on release. Desktop checks do not qualify this tier |
| Selection Brush or Quick Mask, 2048 px | 120 | | |
| Grow, Shrink or Feather drag, full canvas | 120, soft | **Not met.** Feather: 14.9 updates/s on 6000 × 4000; 72.5 updates/s on 2048 × 1536 | Canvas-bar `refine-feather-drag`, 2026-09-27 |
| Paper Scale drag with relief preview | 120 | Unmeasured on the reference tablet | Canvas-bar `paper-scale-drag`; [Paper rendering](../development/paper.md#rendering-and-performance) |
| Pointwise adjustment slider: Levels, Curves, Exposure, Hue/Saturation, Color Balance, White Balance, Black & White | 120, soft | **Not met for Hue.** Master 102.54–109.43, Range 106.57–111.08 presents/s | [Photo color adjustments](#photo-color-adjustments), 2026-10-03 |
| Colorize Saturation and Photo Filter Density | 120, soft | **Not met.** 112.58–115.04 and 110.26–114.09 presents/s | [Photo color adjustments](#photo-color-adjustments), 2026-10-03 |
| Threshold slider, exact native resolution | 120, soft | **Not met.** 4.75–4.79 presents/s, interval p99 241.67 ms | [Photo color adjustments](#photo-color-adjustments), 2026-10-03 |
| Selective Color, neutral and red-family corrections | 120, soft | **Not met.** 110.35–113.31 and 113.89–114.09 presents/s | [Selective Color and Channel Mixer](#selective-color-and-channel-mixer), 2026-10-03 |
| Channel Mixer, coefficient and Constant | 120, soft | **Not met.** 112.20–115.25 and 109.06–112.53 presents/s | [Selective Color and Channel Mixer](#selective-color-and-channel-mixer), 2026-10-03 |
| Color Lookup Intensity, native 65³ table | 120, soft | Current packed LUT path unmeasured; previous path **not met** at 2.72–2.99 presents/s; interval p99 341.68–366.68 ms | [Color Lookup](#color-lookup), 2026-10-03 |
| Navigation with Color Lookup, native 65³ table | 120 | Current packed LUT path unmeasured; previous path **not met** at 41.34–51.29 presents/s; interval p99 25.00–33.33 ms | [Color Lookup](#color-lookup), 2026-10-03 |
| Shadows and Highlights sliders, native | 120, soft | **Not met.** Shadows 2.99–3.99, Highlights 1.20–1.40 presents/s | [Local guide adjustments](#local-guide-adjustments), 2026-10-03 |
| Clarity slider, native | 120, soft | **Not met.** 1.76–1.99 presents/s; interval p99 533.35–741.69 ms | [Local guide adjustments](#local-guide-adjustments), 2026-10-03 |
| Navigation with Shadows/Highlights or Clarity | 120 | **Not met.** 40.14–44.71 presents/s; interval p99 25.00–33.33 ms | [Local guide adjustments](#local-guide-adjustments), 2026-10-03 |
| Android targeted Curves and compact precision controls | 120, soft | Current port unmeasured on the reference tablet | [Android precision and Dehaze diagnostics](#android-precision-and-dehaze-diagnostics) |
| Dehaze Amount | 120, soft | Unmeasured on the reference tablet; non-reference workload misses below | [Android precision and Dehaze diagnostics](#android-precision-and-dehaze-diagnostics) |
| Curves point drag (61 MP) | 120, soft | **Not met.** 72.07–74.61 completed canvas updates/s; native UI 76.51–78.53 frames/s | [Curves editing](#curves-editing), 2026-10-02 |
| Gaussian Blur Radius, soft range | 120, soft | **Not met.** 34.15–35.44 presents/s; interval p99 66.67–75.00 ms | [Gaussian Blur and Unsharp Mask](#gaussian-blur-and-unsharp-mask), 2026-10-03 |
| Unsharp Mask Amount, native Radius 21 or 85 | 120, soft | **Not met.** No presents during three five-second contacts at either radius | [Gaussian Blur and Unsharp Mask](#gaussian-blur-and-unsharp-mask), 2026-10-03 |
| Navigation with Gaussian Blur, Radius 85 | 120 | **Not met.** 40.55–41.09 presents/s; interval p99 33.33 ms | [Gaussian Blur and Unsharp Mask](#gaussian-blur-and-unsharp-mask), 2026-10-03 |
| Saved spatial lengths beyond editor bounds, including Gaussian sigma >85 | 120, soft | Unmeasured | Full authored lengths need moving-frame qualification; earlier capped-value measurements do not apply |
| Edge-Preserving Smooth slider | 120, soft | | |
| Animated or warping filter: Domain Warp, Ripple | 120, soft | **Unqualified.** 9504 × 6336 on the 2376 × 1584 display grid of a non-reference Mali-G57 MC2 tablet: Ripple 58.8 ms, Domain Warp 74.4 ms, from 7.5–8.5 s. Fit between 25% and 50% still evaluates at document resolution; reference tablet unmeasured | [Animated filters diagnostic](mid-tier.md#animated-filters-diagnostic), 2026-10-18; [Windows Domain Warp comparison](#windows-domain-warp-comparison) |
| Fill layer or gradient-fill edit | 120, soft | Solid Color revision unmeasured on this reference device | [Low-tier measurements](low-tier.md#solid-color-fills) do not qualify this tier |
| Paper Texture visibility, Light and Shows through paint drag | 120 | Unmeasured on the reference tablet | [Paper lighting diagnostic](mid-tier.md#paper-lighting-diagnostic) does not qualify this tier |
| Navigation with proof or tone guide shown | 120 | | |
| Navigation with exact artwork sampling (61 MP) | 120 | **Not met.** 48.43–49.89 canvas presents/s; interval p99 25.00 ms | [Exact artwork samples](#exact-artwork-samples), 2026-10-02 |
| Web Histogram pan, statistics pending / Exact settled (61 MP) | 120 | **Not met.** Chrome surface 111.27–112.40 / 111.96–113.88 presents/s; p99 gaps 16.67–25.00 / 16.67 ms | [Web scopes](#web-scopes), 2026-10-03 |
| Web Waveform pan, statistics pending / Exact settled (61 MP) | 120 | **Not met.** Chrome surface 109.83–110.71 / 108.74–111.59 presents/s; p99 gaps 25.00 / 16.67 ms | [Web scopes](#web-scopes), 2026-10-03 |
| Web Levels black slider (61 MP) | 120 | **Not met.** 44.57 / 49.01 / 41.45 Chrome surface presents/s; p99 gaps 41.67–91.67 ms | [Web scopes](#web-scopes), 2026-10-03 |
| Web Curves knot / targeted adjustment (61 MP) | 120 | **Not met.** 40.57 / 38.81 / 35.63 and 49.58 / 48.09 / 48.14 Chrome surface presents/s | [Web scopes](#web-scopes), 2026-10-03 |
| Navigation with Histogram Preview (61 MP) | 120 | **Not met.** 35.78–36.90 canvas presents/s; interval p99 41.67–50.00 ms | [Waveform statistics](#waveform-statistics), 2026-10-03 |
| Navigation with Histogram Exact (61 MP) | 120 | **Not met.** 33.56–35.81 canvas presents/s; interval p99 41.67 ms | [Waveform statistics](#waveform-statistics), 2026-10-03 |
| Navigation with Waveform Preview (61 MP) | 120 | **Not met.** 35.29–36.53 canvas presents/s; interval p99 41.67–50.00 ms | [Waveform statistics](#waveform-statistics), 2026-10-03 |
| Navigation with Waveform Exact (61 MP) | 120 | **Not met.** 33.47–35.58 canvas presents/s; interval p99 41.67 ms | [Waveform statistics](#waveform-statistics), 2026-10-03 |
| Navigation with clipping preview (61 MP) | 120 | **Not met.** 36.01–38.50 canvas presents/s; interval p99 33.33–41.67 ms | [Clipping preview](#clipping-preview), 2026-10-03 |
| Navigation with pending Auto statistics (61 MP) | 120 | **Not met.** 39.40–40.04 canvas presents/s; interval p99 33.33 ms | [Auto statistics worker](#auto-statistics-worker), 2026-10-03 |
| Gradient drag | 120 | | |
| Figure or ruler drag | 120 | | |
| Layer opacity scrub | 120 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: Solid Color revision unmeasured on this reference device | [Low-tier measurements](low-tier.md#solid-color-fills) do not qualify this tier |
| Layer reorder drag | 120 | **Met.** Native UI 117.20–119.41 fps; moving-frame gap p99 8.38–16.67 ms | [Layer reorder and retained photo translation](#layer-reorder-and-retained-photo-translation), 2026-10-04 |
| Layer row swipe | 120 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: **Unqualified for full reveal.** Android partial left fade: 116.57–120.00 moving window frames/s, p99 ≤16.667 ms; GTK desktop: 119.20–120.07 presents/s, p99 ≤8.952 ms | [Layer handle fade](#layer-handle-fade), 2026-10-21–22 |
| Workspace-manager row swipe and reorder | 120 | Unmeasured on reference hardware; functional input checks do not qualify moving-frame timing | [Workspace manager](../ui/default-workspaces.md#workspace-manager) |
| Layer thumbnail selection animation | 120 | **Touch gesture revision unmeasured (2026-10-25).** Earlier: **Not met.** Repeated content/mask selection 77.08–79.86 window frames/s; worst moving-frame gap p99 33.33 ms | [Layer thumbnail selection](#layer-thumbnail-selection), 2026-10-05 |
| Attached filter drag inside a clipping run | 120 | **Not met.** 93.24–102.71 native UI fps; moving-frame gap p99 25.00–33.33 ms | [Filter attachment feedback](#filter-attachment-feedback), 2026-10-04 |
| Navigation with 32 visible paint layers | 120 | | |
| Drawing with 32 visible paint layers, G-Pen 1024 px | 120 | | |
| Panel, tab, column or toolbar drag and docking | 120 | **Not met.** Toolbar or component drag 103–119 fps. GTK desktop group probe fails its 114 Hz floor; current short tab probe gives 110.79–120.02 presents/s, with one touch trial below that floor. Other continuous-tab revisions remain unqualified | `dc27e04d`, 2026-09-23; [GTK panel-tab input diagnostic](#gtk-panel-tab-input-diagnostic), [tab-hover diagnostic](#gtk-tab-hover-diagnostic) and [panel-tab compositing](#panel-tab-compositing), 2026-10-24–25 |
| Android palette pill opening at the work-area edge | 120 | **FPS unqualified.** Instant clamp; two actual presents per opening, all neighbors stationary in the diagnostic | [Native palette opening](#native-palette-opening), 2026-10-23 |
| Android tool bubble drag through four neighbours | 120 | **Unqualified after the bubble release fixes.** Previous parity build: 115.00–116.55 actual app-window presents/s; p99 ≤16.667447 ms, default glass, 61 MP photo at Fit | [Native tool bubbles](#native-tool-bubbles), 2026-10-24 |
| Web tool bubble drag through four neighbours | 120 | Unmeasured after the bubble release fixes; desktop browser journeys do not qualify the reference tablet | [Web checks](../development/web.md#tests), 2026-10-24 |
| Panel or column resize | 120 | **Web diagnostic:** Navigator column 30.00–30.64 Chrome surface presents/s; p99 50.00–58.34 ms. Tier qualification remains unset | [Web Navigator layout](#web-navigator-layout), 2026-10-19; [GTK Color resize](#gtk-color-panel-resize), 2026-10-10 |
| Pen pressure utility and curve drag | 120 | **Unqualified.** Current GTK desktop panel 119.20–119.40 presents/s, p99 up to 9.207 ms; curve 120.00, p99 up to 8.470 ms; reference tablet unmeasured | [GTK pen pressure utility](#gtk-pen-pressure-utility), 2026-10-10 UTC |
| Windows tool bubble drag through four neighbours | 120 | Unmeasured on reference hardware; Surface Pro / Adreno X1-85 functional checks do not qualify timing (PresentMon ETW access denied) | [Windows tool bubbles](../development/windows.md#tool-bubbles) |
| Windows HDR color bubble drag and intensity scrub | 120 | Unmeasured on reference hardware; native input and pixel checks do not qualify presentation timing (PresentMon ETW access denied) | [Windows tool bubbles](../development/windows.md#tool-bubbles) |
| Apple tool bubble drag through four neighbours | 120 | **Unqualified.** M4 iPad input and presentation timing remain unmeasured; the M2 Pro Mac canvas diagnostic cannot qualify the whole native overlay | [Apple native measurements](#apple-native-measurements), 2026-10-24 |
| HDR color bubble drag and intensity scrub | 120 | **Not met.** Android drag 111.38–119.30 actual app-window presents/s; intensity 54.29–55.42/s, p99 up to 41.668 ms, 61 MP F16 photo at Fit; Apple: unqualified Mac canvas drag 89.398–89.938/s; intensity presentation, native overlay and reference timing unmeasured | [Web and Android HDR color bubbles](#web-and-android-hdr-color-bubbles); [Apple HDR diagnostic](#apple-hdr-color-bubble-diagnostic), 2026-10-24 |
| Drawer open and close | 120 | **Unqualified.** GTK Properties at scales 1/2 misses desktop criteria: pooled 73.10–119.86 presents/s, p99 up to 20.198 ms; current Apple measurements pending; reference tablet unmeasured | [GTK Sketch Layers diagnostic](#gtk-sketch-layers-diagnostic), 2026-10-10; [Apple native measurements](#apple-native-measurements) |
| Zen fade, expanded configuration and dialog page transitions | 120 | Unmeasured on the reference tablet, including Windows header replacement during live theme changes; controlled-clock and input checks do not qualify presentation timing | [Shared chrome](../ui/shared-ui.md#window-chrome-and-zen-mode); [numeric controls](../ui/numeric-controls.md) |
| Grouped tool menus, drawer switching and tile drag | 120 | Not measured on reference hardware | [Tool variations](../ui/panel-customization.md#tool-variations); desktop functional checks do not qualify this tier |
| Colour wheel or picker drag | 120 | **Unqualified.** Current GTK 2× workstation picker diagnostic: docked SDR 75.00, HDR 62.96 canvas presents/s; reference tablet unmeasured | [GTK Color diagnostic](#gtk-color-panel-resize), 2026-10-04; earlier [swatch diagnostic](#gtk-selected-swatch-diagnostic) |
| Slider and value scrub: size, opacity, flow | 120 | **Unqualified.** GTK presentations and Web callbacks measured on the workstation; reference tablet unmeasured | [GTK](#gtk-panel-slider-diagnostic) and [Web](#web-numeric-panel-diagnostic), 2026-10-10 |
| Canvas action bar show, hide and move | 120 | **Not met.** UI frame p50/p95: 63.2/90.0 ms moving the bar, 23.0/34.6 ms show and hide (2048 × 1536) | Canvas-bar `ui-bar-move` and `ui-bar-show-hide`, 2026-09-27 |
| Tool Options or panel content change | 120 | **Not met.** UI frame p50/p95 25.1/30.6 ms | Canvas-bar `ui-panel-change`, 2026-09-27 |
| List scrolling: layers, brushes, filters | 120 | **Unqualified.** GTK with 1,000 layers misses desktop criteria: touch at scale 1 gives 97.24–109.99 presents/s, p99 up to 25.016 ms; reference tablet unmeasured | [GTK layer input diagnostic](#gtk-layer-input-diagnostic), 2026-10-10 |
| Menu open and close | 120 | Application menu heading switching unmeasured on reference hardware. Selection-bar diagnostic: menu open adds no canvas frames; UI frame p50/p95 11.5/26.6 ms | Canvas-bar `selection-bar-menu-open`, 2026-09-27; [menu interaction](../ui/window-bar.md) needs presented-frame qualification |

## Web and Android HDR color bubbles

Measured 2026-10-24 on the HDR bubble port published as `712b4a18a`. Android uses
an optimized release-Rust benchmark APK, the tier photo in an F16 drawing with
an empty paint layer at Fit, default Low glass, a 216 dp HDR color bubble and
four neighboring bubbles. Each condition has a priming contact followed by
three six-second contacts. Intensity follows a constant-speed triangular path.
SurfaceFlinger actual-present timestamps cover the active app-window motion;
release and settling frames are excluded.

On Wacom MovinkPad Pro 14, the photo is 9504 × 6336, the display runs at 120 Hz
and thermal status is 0. The rates below count window presentations, not fresh
intensity values: the intensity runs publish 180–181 full models per contact
and visit 166–180 distinct values. Android's moving bubble action uses the
existing 33 ms publication interval. A window presentation does not establish
that its intensity marker reflects a new input.

| Motion | Actual app-window presents/s, three trials | Worst presentation gap p99 |
| --- | --- | --- |
| Drag through four neighbors | 118.272 / 111.376 / 119.305 | 16.668 ms |
| Intensity | 54.732 / 54.292 / 55.423 | 41.668 ms |

The second drag contact misses the 114 fps floor; all intensity contacts miss
both the throughput and gap criteria. These measurements do not qualify the
120 fps target. The [mid-tier result](mid-tier.md#hdr-color-bubbles) records its
separate display-setting limits.

The Web diagnostic uses a hardware-accelerated workstation browser at 120 Hz
with a 2048 × 1536 F16 drawing. In light/dark themes, three six-second mouse
contacts per condition produce 110.63–120.05 changing DOM positions/s while
dragging and 65.32–77.42 changing intensity values/s. These are observations at
animation callbacks, not actual presentations. Browser presentation, reference
tier rates and physical HDR output remain unqualified.

Exact source and binary hashes, raw trials, native logs and review captures are
under `artifacts/hdr-color-bubble-ports/`. The Android benchmark was built at
`bcfc0a978`; subsequent rebases leave its bubble and Android production paths
unchanged. Its APK SHA-256 is
`6a63c3c919fa8ec4e717938626fa439e5bc4443a2ee821a2c7c1ee94e8f02e9d`.
The Web Wasm SHA-256 is
`e78a56053dd39f14e02744c773cb69a7e6b1bd065753d1af03479609d6ce9bf2`.

## GTK HDR color bubble

Measured 2026-10-24 on `ae97b0732` plus the equal-distance HDR bubble outline,
using a release build, NVIDIA RTX PRO 6000 Blackwell Max-Q, Vulkan, the private
GTK runtime and a 1600 × 1000 Mutter display at 120 Hz. The drawing is 640 × 480
F16, with the paint pair, recent colors and palette enabled. The compositor
advertises SDR at 80 nits. This does not qualify physical HDR or a reference tier.

`native_hdr_color_bubble_motion` warms each condition, then records three
six-second gestures with 4 ms native input. Intensity uses a constant angular
speed so two-decimal rounding does not create pauses at sinusoidal extrema.
GTK presentation timestamps exclude release and settling frames.

| Input / motion | Presented frames/s, three trials | Worst presentation gap p99 | GTK paint p95 range |
| --- | --- | --- | --- |
| Mouse drag | 119.18 / 119.18 / 120.00 | 8.548 ms | 3.934–4.295 ms |
| Touch drag | 116.21 / 119.02 / 119.67 | 16.697 ms | 3.965–4.253 ms |
| Mouse intensity | 69.48 / 69.95 / 67.98 | 16.898 ms | 6.359–6.862 ms |
| Touch intensity | 112.45 / 112.95 / 108.53 | 16.827 ms | 5.313–6.926 ms |

Intensity still misses the desktop 120 Hz criteria. No builds or tests from
this task ran concurrently with the measurement. The private GTK runtime
changed since the previous diagnostic, so the two runs are not a matched
performance comparison. No tier target is marked met.

Source, runtime and binary hashes and raw trials are in
`artifacts/hdr-color-bubble-uniform-border/`. The test binary SHA-256 is
`71a43f75118c762a29890d0ed02029c53af9258cf3b7edcdac7e3aeda8fa5dbf`.

## Native palette opening

The 2026-10-23 corrected palette journey passed light/dark touch on MovinkPad Pro
14. The reopened pill bottom is 1775.5 px inside the 1800 px workspace, with both
buttons visible; Next advances once and Previous restores the original name.

Using the prescribed 9504 × 6336 photo at Fit, default glass, release-Rust benchmark
APK `27d0bd47`, 120 Hz display and thermal status 0, three short opening diagnostics
recorded production-active windows of 36.552 / 45.177 / 38.554 ms. Each contains
two actual SurfaceFlinger app-window presents, two wheel poses and one stationary
pose per neighbor. The wheel clamps in one step. The active flag also covers
request/settling bookkeeping, so this is not sustained animation and no moving
FPS or p99 qualification is claimed. The existing five-bubble drag result remains
separate. See the [protocol and full build hashes](mid-tier.md#native-palette-opening).
Evidence is in `artifacts/bubbles/android-palette-growth/top-short/`, with native
light/dark bounds and screenshots in the adjacent `top-corrected-evidence/`.

## Native tool bubbles

The 2026-10-24 bubble release fixes have no new measurement on the reference
MovinkPad Pro 14. The earlier qualifying result below does not qualify the
current build. The [mid-tier measurement](mid-tier.md#native-tool-bubbles) uses
the updated scheduler on the MovinkPad 11.

The previous Android visual parity build measured 2026-10-23 on MovinkPad Pro 14,
with the prescribed 9504 × 6336 photo at Fit, default Low glass, 120 Hz display
and thermal status 0 throughout. Benchmark APK SHA-256
`c9ad05fde41bde88d63b5e0a972334af9a2d9dfa7f6cc90dad4f5b4b747329eb`
uses release Rust and nondebuggable, unminified Android code; this is the existing
white-box benchmark, not an R8 release. Test APK SHA-256 is
`ab325b4472515ca0690ccd7ff6f1f6a35d03e7f692eb8db6c3ec6f1b24e6937d`.

`AndroidBubbleBenchmarkTest#fiveBubbleMotion` warms a six-second native touch
contact, then performs three six-second drags through four settled neighbors.
Every neighbor visibly moves. Canonical position resets and settling are outside
timed motion. The collector polls actual app-window SurfaceFlinger timestamps;
warmup and 100 ms at each contact edge are excluded. No tablet setting changed.

| Contact | Actual presents/s | p99 interval, ms |
| --- | --- | --- |
| 1 | 116.547929 | 16.667240 |
| 2 | 114.996250 | 16.667292 |
| 3 | 115.314155 | 16.667447 |

All contacts meet the 114 presents/s floor and 16.7 ms p99 limit. JUnit passed
with clean ActivityScenario teardown, `OK (1 test)` in 37.341 s. FrameMetrics and
renderer timings remain attribution proxies. Current source/binary hashes are
in `artifacts/bubble-parity-review/performance/source.json`; raw measurements
are under `/mnt/localssd/capycanvas2-bubble-parity-review/performance/android-top/`.

The earlier APK `5c2e4751` measured 116.720 / 115.858 / 115.341 presents/s with
p99 16.667188 / 16.667239 / 16.667292 ms, also meeting both limits. That earlier
evidence remains in `artifacts/bubbles/android-top-transport-presents/`.
Android 16 wraps SurfaceFlinger layer names in `RequestedLayerState`; the
collector normalizes those names before sampling the exact app-window layer.

## Layer thumbnail selection

Measured 2026-10-05 on the Wacom MovinkPad Pro 14 at thermal status 0 before
and after motion, using benchmark APKs with release Rust based on `8f73b600d`
plus the Android drawing changes. The 9504 × 6336 reference photo sits beneath
an empty paint layer with a mask, at Fit zoom and default panel glass.
`AndroidTitleBarTest#layerSwipeFrameTiming` with `layerSelectionBenchmark=true`
retargets between content and mask every 150 ms. Compose drives the 200 ms
outline animation in an isolated graphics layer using cached squircle contours.
The workload also changes
tools and the canvas action bar; it does not isolate outline drawing cost.

After one second of priming, each variant ran three five-second gestures without
screen recording, with the same test APK, dark theme and 120 Hz display.

| Drawing implementation | Window frames/s, three runs | Per-run median draw recording |
| --- | --- | --- |
| Original thumbnail drawing | 77.36 / 78.33 / 77.17 | 0.550–0.759 ms |
| Isolated outline drawing | 77.91 / 79.53 / 76.10 | 0.312–0.389 ms |
| Retained outer shape and masked hole | 75.98 / 77.80 / 77.18 | 0.011 ms |

`FrameMetrics` intervals within observed selection animation windows had p99
33.33 ms in all comparison runs, with zero dropped reports. These are unique window-vsync
timestamps, not SurfaceFlinger actual-present times. The 120 fps target is not
met. Isolating the outline reduces draw recording while preserving the original
geometry and antialiasing. The masked variant reduces recording further without
improving cadence; it needs two additional offscreen surfaces and changes edge
coverage, so the isolated outline is retained. Raw frames, timing components,
APK hashes and the rate calculation are under
`artifacts/android-border-performance/`.

The post-rebase repeat at `2a55ce947`, with identical app and test APK hashes,
recorded 77.08, 79.86 and 78.65 window frames/s, with p99 gaps of 33.33, 25.00
and 33.33 ms. Dropped reports remained zero and thermal status remained 0.

## GTK Sketch Layers diagnostic

Measured 2026-10-10 on the `e9c1c5cdb` working tree with the shared drawer
port, release Rust, GTK 4.22.5, libadwaita 1.9 and NVIDIA RTX PRO 6000 Blackwell
Max-Q/Vulkan 615.71.09. The private Mutter display requests 120 Hz and
1100 × 1000 logical pixels; scale 2 uses 2200 × 2000 physical pixels.
The drawing is empty, 2048 × 1536. These desktop diagnostics do not qualify
the reference tablet or its 61 MP photographic workload.
The frozen tester SHA-256 is
`44cc15f421cfa870a63eda5a21587acb7195d5e3930a9799d61ae14dba22a2a9`;
the review app is
`ec2b8f8c1f45aeec74671ba0952d6c46b5b1f38ea88beff5de062103d9e6cab2`.
Source manifests and raw results are under `artifacts/layer-drawer-ports/gtk/`.

The native journeys verify header and custom toolbar openers in all three
workspaces, footer alignment, six-row sizing, immediate additions and deletion,
1,000-row scroll restoration, overflow, text-size and viewport changes,
filter insertion, Group properties, All layers, native Undo and cancellation.
Both themes pass at scales 1 and 2. Touch, mouse and injected pen paths pass;
these do not establish physical pen behavior. The synchronized transition test
compares the container height and child slide on native painted frames,
including reversal and disabled animation.

Only unique completed GdkFrameClock presentation timestamps where native
geometry changes count. Idle intervals and gaps between gestures are excluded;
interval p99 uses nearest rank. Each scrolling or swipe case has three primed
six-second gestures per theme. Properties uses four five-second toggle windows
per theme, including scroll restoration. Build checks run outside motion windows.
These are current-only diagnostics; no matched baseline was measured for this
frozen build.

| 1,000-layer scrolling | Presents/s, three runs | Maximum interval p99 |
| --- | --- | --- |
| Light / scale 1 | 95.57–99.83 | 74.878 ms |
| Dark / scale 1 | 113.46–116.48 | 16.814 ms |
| Light / scale 2 | 89.49–101.55 | 24.990 ms |
| Dark / scale 2 | 92.16–95.85 | 25.021 ms |

The list realizes 205–206 widgets; pending thumbnails are zero at all sampled
endpoints. The issued request counter is cumulative and does not measure
pending work or texture memory. Visited thumbnails remain cached, so bounded
widget count does not establish constant total memory use. Native painted-row
checks preserve the visible layer ID and pixel offset after returning from
Properties at both scales. Scrolling misses the desktop rate or interval limits.

Properties transitions are remeasured on the `996cc49f3` working tree with
GTK panel surfaces moved from reusable content to their owning containers,
in the same desktop setup.
The tester SHA-256 is
`c81218a51013d57652ada32b981130de1d63dd2405ba4d66fae2c2c8a886bbaf`;
the app is
`9cbe1b296b1901f56e7dcd20a44a949d2fb80828d5e034d0401233ff6dcc4194`.
Source hashes, native surface checks and presentation arrays are under
`artifacts/panel-surface-ownership/`. The 476 native surface cases cover
17 reusable panel types across docked, tabbed, floating, header, toolbar,
column and configuration placements, in both themes with glass Off/High.
Content has no panel background or shadow, normal previews add no duplicate
fill, and owning containers retain their surfaces. Expanded preview and
configuration columns have opaque fills with one combined shadow. Standalone
monitors receive one inset; Properties' nested histogram receives none.
Properties behavior and synchronized slide/resize checks pass at both scales.

| Properties transition | Scale | Pooled presents/s | Interval p99 |
| --- | --- | --- | --- |
| Light / open | 1 | 115.96 | 16.420 ms |
| Light / close | 1 | 119.86 | 8.645 ms |
| Dark / open | 1 | 113.98 | 16.725 ms |
| Dark / close | 1 | 117.89 | 16.631 ms |
| Light / open | 2 | 100.59 | 18.297 ms |
| Light / close | 2 | 74.57 | 16.818 ms |
| Dark / open | 2 | 99.20 | 20.198 ms |
| Dark / close | 2 | 73.10 | 16.796 ms |

These current-only transition measurements do not establish a speedup from
the surface change. Raw timestamps are in `properties{1,2}/properties-animation.json`;
pooled rates and interval p99 are in `properties-metrics.json`.

Properties also misses the desktop criteria. The Lock/Duplicate/Delete strip
meets those desktop criteria in every sampled gesture; reference-tablet
qualification remains open.

| Layer swipe | Presents/s, three runs | Maximum interval p99 |
| --- | --- | --- |
| Light | 119.96–119.98 | 8.749 ms |
| Dark | 119.96–120.00 | 9.097 ms |

The raw presentation arrays are in `autoheight{1,2}/large-list-scroll.json`,
`native_sketch_layer_properties_drawer/properties-animation.json` and
`swipe-motion/swipe-animation.json`; the summary retains source and binary hashes.

## GTK layer input diagnostic

Measured 2026-10-10 on the `b5b9bfc48` working tree with GTK layer contact
capture, hidden displaced-row handles and device-aware scrollbars. The release
build uses NVIDIA RTX PRO 6000 Blackwell Max-Q/Vulkan 615.71.09 and an empty
2048 × 1536 drawing. Native input runs on a private Mutter display at 120 Hz.
These workstation results do not qualify a reference tablet or its tier canvas.
Source diffs, executable hashes and raw arrays are in `artifacts/gtk-layer-input/`.

The matched swipe runs use system GTK 4.22.5/libadwaita 1.9.4, a 1600 × 1000
viewport and native touch input at requested 4 ms intervals. Each theme has
three six-second motion windows. Rates pool unique completed presentations only
while row geometry changes, excluding gaps between contacts; interval p99 uses
nearest rank. Builds are idle during measurement. The runner's default 60 ms
input cadence produces an input-limited diagnostic and is excluded here.

| Layer swipe | Baseline presents/s | Changed presents/s | Baseline / changed maximum interval p99 |
| --- | --- | --- | --- |
| Light | 119.94–119.97 | 119.99–120.03 | 8.696 / 8.694 ms |
| Dark | 119.64–119.96 | 119.60–119.99 | 8.769 / 8.644 ms |

The bundled-runtime 1,000-layer touch scroll uses a 1100 × 1000 viewport at
scale 1 and the same 4 ms input cadence, with three six-second gestures per
theme. Only unique completed presentations where the scroll adjustment changes
count. This is a current-only diagnostic without a matched scrolling baseline.
Both rate and p99 miss the 120 Hz desktop criteria.

| Touch scrolling | Presents/s, three runs | Maximum interval p99 |
| --- | --- | --- |
| Light | 101.91 / 108.41 / 109.93 | 24.915 ms |
| Dark | 97.24 / 109.99 / 106.96 | 25.016 ms |

Functional journeys use the bundled GTK 4.22.4 runtime. Both themes cover docked,
floating and drawer layers, mouse scrollbar dragging, first touch at an overlapping
scrollbar/handle, native reorder Undo/Redo and cancellation, displaced-row pickup
refusal, pen hover, pen handle capture and touch/pen body scrolling. Pen events
are injected; physical pen pickup and reference-tablet motion remain unverified.

## Layer handle fade

GTK measured 2026-10-21 at `3ac660dcc` plus the handle fade, using a release
build, system GTK 4.22.5, Mutter 50.5 and NVIDIA RTX PRO 6000 Blackwell Max-Q
Workstation Edition/Vulkan 615.71.09. The private monitor is 1600 × 1000 at
120 Hz; the drawing is the default 1333 × 1000 sheet. Native touch events use
4 ms intervals. These desktop results do not qualify a reference tablet.

Each theme has three motion windows of 5.7–6.5 seconds. The first open/close pair
per theme primes the animation and is excluded. Rates pool unique completed
presentations while row geometry changes, preserving each contact's intervals
without joining gaps between contacts. Interval p99 uses nearest rank.

| Theme | Presents/s, three runs | Maximum interval p99 |
| --- | --- | --- |
| Light | 120.07 / 119.96 / 119.62 | 8.952 ms |
| Dark | 119.95 / 119.20 / 119.98 | 8.708 ms |

Raw timestamps are in `artifacts/gtk-grip-fade/motion/swipe-animation.json`;
`motion-summary.json` retains excluded priming contacts and source/binary hashes.
The partial-opacity, reversal, cancellation and pickup journeys pass in both
themes. Physical pen input remains unverified.

Android measured 2026-10-22 at the same source revision plus the cross-platform
fade, on Wacom MovinkPad Pro 14 with the release-profile benchmark build and the
9504 × 6336 reference photo. Each case primes for one second, then measures three
five-second gestures. The existing benchmark oscillates 24–64 dp from rest;
these measurements cover partial opacity changes rather than full reveal and
return. Left-swipe runs have thermal status 0 before and after; the right-swipe
run has no pre-run thermal reading.

| Direction / theme | Moving window frames/s, three runs | Maximum vsync interval p99 |
| --- | --- | --- |
| Left / light | 119.80 / 120.00 / 117.38 | 16.667 ms |
| Left / dark | 116.57 / 120.00 / 120.00 | 16.667 ms |
| Right / dark | 119.80 / 117.58 / 118.59 | 16.667 ms |

FrameMetrics samples count only changed row geometry. Total-duration p95 is
19.2–19.4 ms; that pipelined frame duration differs from the vsync cadence.
All nine runs report zero dropped metric callbacks. Raw arrays, thermal readings,
captures and `frame-summary.json` are in `artifacts/gtk-grip-fade/android/`.
There is no matched baseline or SurfaceFlinger scanout capture. Full-swipe
qualification, current Web frame timing, native Apple validation and Windows
hardware performance remain unverified.

## GTK panel slider diagnostic

Measured 2026-10-10 at `778c02e48`, with bottom-aligned GTK values and shared
whole-pixel value scrubbing above 1000 px, in a release build with Adwaita Sans
11 on NVIDIA RTX PRO 6000 Blackwell
Max-Q/Vulkan 615.71.09. The private Mutter display is 1600 × 1000 at a requested
120 Hz, with a 2048 × 1536 empty drawing. `native_panel_slider_motion` uses
native mouse input at requested 4 ms intervals, warms each field, and records
three six-second scrubs each of brush size, opacity and flow in both themes,
for both slider and value handles. Size value runs start at 120 px and 1200 px.
Each scrub traverses sixteen legs: 60% of the track, or 240 logical pixels for
size values and 120 for opacity/flow values.
This requests enough distinct values to exercise 120 Hz presentation.
Only GTK presentations whose numeric value changed are counted. Whole-value
slider steps can leave some presentations unchanged at the sampled value.

| Theme / handle / field | Moving presentations/s | Interval p99 |
| --- | --- | --- |
| Light / Slider / Size | 118.53–118.85 | 9.368–16.656 ms |
| Light / Slider / Opacity | 113.98–115.22 | 16.745–16.822 ms |
| Light / Slider / Flow | 114.23–115.72 | 16.708–16.788 ms |
| Light / Value / Size (120 px) | 119.84–120.00 | 8.561–8.596 ms |
| Light / Value / Size (1200 px) | 119.34–119.67 | 8.532–8.605 ms |
| Light / Value / Opacity | 120.00–120.01 | 8.559–8.580 ms |
| Light / Value / Flow | 119.52–119.84 | 8.591–8.668 ms |
| Dark / Slider / Size | 119.02–119.51 | 8.565–8.829 ms |
| Dark / Slider / Opacity | 114.07–115.72 | 16.699–16.798 ms |
| Dark / Slider / Flow | 114.40–115.06 | 16.702–16.795 ms |
| Dark / Value / Size (120 px) | 119.84–120.01 | 8.524–8.555 ms |
| Dark / Value / Size (1200 px) | 118.69–119.34 | 8.619–16.573 ms |
| Dark / Value / Opacity | 119.67–119.84 | 8.613–8.664 ms |
| Dark / Value / Flow | 119.84–119.84 | 8.587–8.812 ms |

These are workstation diagnostics with a small drawing. They do not qualify
any reference-tablet target.
Raw records, the measured source diff and executable hash are in
`artifacts/platform-number-controls/gtk/`.

The Web port's four five-second Hue slider diagnostics, at 640 and 1100 pixels
in both themes on the same GPU, recorded host callback CPU p95 of 0.9–1.0 ms
and maxima of 1.2–1.4 ms. The drawing is 256 × 256. These callback measurements
have no compositor presentation feedback and do not establish frame rate or
input latency. Records are in `artifacts/panel-sliders/port-web-rebase-native/hue-motion.json`.
Android functional checks use the available Huion tablet; the low, mid and top
reference devices were reserved by other sessions. Apple native hardware and
Windows hardware presentation were not measured.

## Web numeric panel diagnostic

Measured 2026-10-10 on the same NVIDIA workstation with Chrome 154.0.8037.57,
the optimized `dev-perf` Wasm build at `778c02e48`, and actual Adwaita Sans
glyph fonts. The private Mutter display is
1600 × 1000 at a requested 120 Hz; the drawing is 2048 × 1536.
After a priming gesture, each theme runs three six-second native mouse scrubs
from 1200 px through sixteen 240-pixel legs at requested 4 ms input intervals.
Only animation callbacks where the numeric value changed are counted.

| Theme | Changed-value callbacks/s | Callback interval p99 |
| --- | --- | --- |
| Light | 118.82–119.64 | 8.5–11.3 ms |
| Dark | 119.10–119.52 | 8.4–8.5 ms |

These are browser callback diagnostics, without compositor presentation
feedback. They do not qualify a reference-tablet target. Raw callbacks, input
geometry and actual font reports are in
`artifacts/platform-number-controls/web/final-rebase/numeric/`.
Android numeric input and geometry pass on the MovinkPad 11, but sustained
numeric presentation is unmeasured. Apple numeric UI builds and hardware
presentation are unavailable on this Linux host; Windows VM checks use WARP
and do not measure hardware presentation.

## GTK selected swatch diagnostic

Measured 2026-10-03 with the selected-swatch change based on `b112ce6ff`, in a
release build on NVIDIA RTX PRO 6000 Blackwell Max-Q/Vulkan. The private Mutter
display is 3200 × 2000 at 120 Hz and 2× scale. The existing
`native_color_picker_preview_pacing` fixture uses its 2048 × 1536 painted document,
three 6.6-second moving gestures per condition and 16 ms input intervals.

| Document / Color panel | Canvas presents/s | Present interval p95 |
| --- | --- | --- |
| SDR / closed | 83.74–87.68 | 20.02–20.41 ms |
| SDR / docked | 72.33–75.96 | 27.98–28.55 ms |
| HDR / closed | 84.95–86.17 | 20.24–20.32 ms |
| HDR / docked | 67.06–70.35 | 29.33–36.64 ms |

Docked wheel snapshot p95 is 0.38–0.48 ms and refresh p95 is 0.16–0.20 ms.
The complete GTK paint phase reaches 11.48–19.68 ms p95. These are workstation
results with p95 intervals, a small document and no paired GTK baseline; they
neither qualify the 61 MP tablet target nor establish a before/after comparison.
HDR here means a float document, not a physical HDR display. Binary identity,
raw records and summaries are under `artifacts/color-overlap/pacing/`.

## GTK color panel resize

Remeasured 2026-10-10 on the `996cc49f3` working tree with GTK panel surfaces
owned by workspace containers, using the frozen tester and app identified in
[GTK Sketch Layers diagnostic](#gtk-sketch-layers-diagnostic). Hardware and
runtime versions are unchanged. The private Mutter display and maximized
window are 1600 × 1000 logical/physical pixels at 120 Hz and scale 1. The
fixture uses a 2048 × 1536 SDR drawing and default Low transparency.
Each mouse/touch condition has three primed 6.6-second contacts. Only completed
presentations with changed panel geometry count; interval p99 uses nearest rank.
No builds or other GPU journeys run during motion.

| Theme / input | Color column presents/s | Maximum interval p99 | Floating Color presents/s | Maximum interval p99 |
| --- | --- | --- | --- | --- |
| Light / mouse | 60.14–60.68 | 18.741 ms | 118.29–118.81 | 16.654 ms |
| Light / touch | 61.10–65.40 | 24.914 ms | 118.36–118.96 | 16.629 ms |
| Dark / mouse | 59.62–60.15 | 24.939 ms | 118.66–118.96 | 16.645 ms |
| Dark / touch | 61.12–62.24 | 18.948 ms | 118.36–118.96 | 16.662 ms |

Both theme journeys pass. All 24 contacts have zero field rebuilds and zero
model refreshes during resize. Floating Color meets the desktop rate and gap
criteria in these runs; the column misses both. This smaller workstation
workload does not qualify the reference tablet. Current raw results are in
`artifacts/panel-surface-ownership/color-resize-{light,dark}/gtk.json`, summarized
in `color-resize-metrics.json`.

Matched Light runs with the preserved `996cc49f3` production sources give
58.98–59.65 presents/s for mouse column resize and 58.68–59.51 for touch,
versus 60.14–60.68 and 61.10–65.40 with the surface change. Baseline floating
resize gives 114.92–118.21 versus 118.29–118.96. The column misses desktop
criteria in both builds. Baseline raw results are in
`artifacts/panel-surface-ownership/baseline-color-light/gtk.json`;
the combined summary preserves all three repetitions per condition. The
baseline tester SHA-256 is
`22464b59c8aae43b76003b5d35ed29472ed0616e39d011946ad356a055895dd6`.

Earlier measurements on 2026-10-04 used a release build based on `48efaf593`, on AMD Ryzen
Threadripper PRO 9995WX and NVIDIA RTX PRO 6000 Blackwell Max-Q/Vulkan
615.71.09. The private Mutter display is 3200 × 2000 at 120 Hz. The
`native_color_wheel_resize_input` and `native_hdr_color_wheel_resize_input`
fixtures use 2048 × 1536 SDR and F16 drawings, three
6.6-second contacts per mouse/touch condition, 4 ms input intervals, and GTK's
completed presentation feedback for frames with a changed panel width.

| Document / scale / theme | Color column presents/s | Moving interval p99 | Floating Color presents/s | Moving interval p99 |
| --- | --- | --- | --- | --- |
| SDR / 1× / Light | 27.20–32.24 | 91.62–158.04 ms | 78.86–84.26 | 49.82–50.12 ms |
| SDR / 1× / Dark | 22.72–38.78 | 100.00–164.62 ms | 62.19–86.02 | 49.87–66.81 ms |
| SDR / 2× / Light | 19.52–27.82 | 108.34–133.34 ms | 48.58–58.50 | 66.63–83.33 ms |
| SDR / 2× / Dark | 20.59–25.13 | 125.00–174.98 ms | 39.63–55.88 | 66.60–100.17 ms |
| HDR / 1× / Light | 24.64–44.61 | 58.16–175.21 ms | 57.33–93.91 | 33.29–83.23 ms |
| HDR / 1× / Dark | 27.91–38.16 | 66.81–141.39 ms | 47.00–100.58 | 25.02–83.33 ms |
| HDR / 2× / Light | 12.88–28.67 | 74.88–241.48 ms | 43.79–58.64 | 50.07–91.74 ms |
| HDR / 2× / Dark | 18.64–27.28 | 100.04–183.43 ms | 45.00–65.21 | 33.31–108.53 ms |

After the first repetition, maximum per-contact wheel snapshot p95 is 1.89 ms;
over all 96 contacts it is 3.90 ms. Every contact performs zero wheel raster jobs
during motion; HDR also retains
the same intensity texture. Only the hue ring, field and HDR ramp scale.
Text, buttons and markers follow current geometry on every frame. Control pixels
outside the color surfaces match within one byte before and after release at the
same width. Release finishes the current hue, shape, rendition and physical
raster size on workers. Native input, cancellation, retained controls and
undo/redo pass at both scales and themes. These rates remain below 120 Hz;
the small workstation fixtures do not qualify any reference tablet tier.

The former 472 px managed ring takes 46.39–46.86 ms in a warmed release CPU
probe. Shared adaptive hue stops and an antialiased ring texture reduce that to
7.30–7.64 ms; the SDR field adds 3.09–3.12 ms, and HDR base evaluation alone
adds 8.52–8.83 ms before mapping. Computation alone therefore cannot fit a
120 Hz frame. Active panel resize scales the retained color surfaces every frame
and prepares the final raster on release, with no redraw-rate cap.

A 2× control run with Color hidden presents other column resizes at
22.8–27.8/s. Main-thread stack samples during motion predominantly land in
native GTK renderer/driver calls, including GPU image retirement. Reusing
panel corner-mask slices and tinting solid masks do not improve that diagnostic
and are excluded from the change. The remaining whole-UI stalls are not color
raster computation; lowering the wheel redraw rate has no supporting evidence.
Earlier CPU probes and stack samples are under `artifacts/color-resize/`.
Current control-pixel captures, timing records, release executable and source
identities are under `artifacts/color-resize-controls/`, indexed in `evidence.json`.

The Web resize diagnostic measured 2026-10-03 passes three five-second mouse/touch
contacts per theme and placement with zero synchronous field raster calls. On the same
workstation, its changed-width animation callbacks measure 65.16–77.58/s for
the Color column and 49.50–82.55/s for floating Color at 1×, and 36.26–67.69/s
and 43.33–68.56/s at explicit 2× browser DPI. The 1× browser viewport is
1600 × 1000; the 2× viewport is 1440 × 1000. Maximum callback-gap p99 is
127.2 ms at 1× and 280.5 ms at 2×. These are browser callback diagnostics,
not screen presents or a matched scale comparison, and qualify no tier.
Reports are `artifacts/color-resize/web-final-{1,2}/web.json`.

The affected picker path also passes `native_color_picker_preview_pacing` with the
same production sources at 2× scale, with one 6.6-second moving gesture per condition
and 16 ms input intervals. This is a functional and timing diagnostic, not the
three-contact qualification required for a target result.

| Document / Color panel | Canvas presents/s | Present interval p95 | GTK paint p95 |
| --- | --- | --- | --- |
| SDR / closed | 84.36 | 20.35 ms | 0.10 ms |
| SDR / docked | 75.00 | 24.97 ms | 9.42 ms |
| HDR / closed | 83.22 | 20.34 ms | 0.07 ms |
| HDR / docked | 62.96 | 39.96 ms | 23.87 ms |

Docked wheel snapshot p95 is 0.36–0.40 ms and refresh p95 is 0.12–0.14 ms.
Worker field evaluation p95 is 2.86 ms for SDR and 21.69 ms for HDR. Physical
HDR display behavior remains unverified. Records are in
`artifacts/color-resize-controls/picker-pacing.log`.

## Photo color adjustments

Measured on 2026-10-03 on the reference tablet, using the original Sony photo,
Fit zoom, Navigator, default display settings and glass, and a private benchmark
application. Three warmed five-second contacts per control count the private
SurfaceView's actual presents inside the input window. Thermal status is zero.

| Motion | Actual presents/s, three contacts | Moving interval p99 |
| --- | --- | --- |
| Master Hue | 103.192 / 102.537 / 109.426 | 16.667–25.001 ms |
| Green Range Hue | 109.615 / 111.079 / 106.571 | 16.667–25.001 ms |
| Colorize Saturation | 112.576 / 113.980 / 115.038 | 16.667 ms |
| Photo Filter Density | 114.093 / 110.255 / 112.373 | 16.667–25.001 ms |
| Threshold | 4.773 / 4.745 / 4.790 | 241.674 ms |

No row meets the sustained target. Integer Density and Saturation use 40% slider
travel with a 0.5-second triangle period, providing over 120 value transitions/s.
Earlier narrow-waveform runs quantize to too few changes and are diagnostic only.
Before these changes, matched original-photo Exposure and Master Hue controls
presented at 111.15–113.35 and 105.60–111.28 Hz. Static Hand navigation presented
at 41.73–52.36 Hz without an adjustment and 40.29–42.94 Hz with unchanged Invert,
Desaturate, Threshold or Photo Filter. Those navigation runs precede the final
native scheduling changes and remain baseline evidence.

Native scans now alternate direction to reuse the warm end of the bounded
source cache. Resident output batches at most 16 tiles, final pointwise effects
write directly into the admitted destination, and independent decodes precede
compatible consumers without crossing a source or mask dependency. Shared mask
bindings retain the existing bounded cache. A redundant scan for the next missing
page is also removed. With closely matched 753–760 decoded slots, Threshold
improved from 1.20–1.40 Hz before batching to 3.57–3.60 Hz after batching and
4.75–4.79 Hz after decode ordering. Owner CPU medians fell from 547–556 ms to
180–191 ms and then 134 ms. Final effect passes fell from 246 to 60. Colorize
avoids calculating the input hue and saturation it replaces.

A separate final trace retains 751 decoded slots and records 751 hits plus 199
misses per native update. Over five seconds, command finish takes 1.588 s under
publication and 0.469 s under composition; composition submit takes 0.693 s and
bounded waits 0.476 s. These nested scopes overlap and must not be summed.
Source planning takes about 2.2 ms/update. The final trace reports 36 ftrace setup
notices and no error-severity parser statistics. The remaining cost includes repeated decode
under the source admission limit and driver command work; this does not establish
a hardware ceiling.

Same-kernel calibration on this tablet uses exact 3:2 derivatives of the tier
photo, with the same U8 assumed-sRGB source and RGBA32Float decoded textures.
At 6000 × 4000, Threshold presents at 28.56–29.33 Hz and ordinary owner CPU
medians are 15–16 ms. At 4248 × 2832 it presents at 51.09–53.12 Hz. Both retain
all source tiles (384 and 204) and their diagnostic traces show no source misses
or upload drains. The 61 MP workload exceeds the retained source allowance.
These calibrations do not qualify the low or mid reference hardware.

One logical full-resolution RGBA32Float read and write at 61 MP moves
1,926,955,008 bytes, requiring 231.23 decimal GB/s at 120 updates/s before padding
and other passes. This is workload arithmetic, not measured physical bandwidth.
Threshold fails the reduced-graph edge/high-frequency quality bounds and retains
exact native evaluation. Invert, Desaturate and Photo Filter pass the existing
reduced-graph qualification and use display-resolution previews.

Final native source allocation is 753 slots / 792,668,672 bytes, with five resident
hierarchy levels / 1,214,406,400 bytes. These are allocator boundaries, not
continuous total process, driver or VRAM peaks. Early 355-slot and 733-slot runs
have different admissions and are not strict paired comparisons. The 8 ms value
observer includes drag slop; changed-preview and physical display latency remain
unverified. Low and mid reference tiers and continuous memory peaks are unmeasured.

Final native/Hue benchmark APK SHA-256:
`21d787596f616ea5593961d6e3879b28597fa4880ebef5ab8983c3475e04b474`.
Density and Saturation use `665cd1246e2a4ae5bc39edd63df34945e1a936379217dcc6cee6de47c3774e3e`;
only native scheduling changed afterward, with matching shader and manifest
hashes. Full source identities, corrected input records, calibration provenance
and measurements are in
`artifacts/photo-editing-color/p21-performance/p21-final-evidence.json`.

## Selective Color and Channel Mixer

Measured on 2026-10-03 on the reference tablet, with the original 9504 × 6336
Sony photo, Fit, Navigator, default display settings and glass, and a private
release-Rust benchmark application. Three warmed five-second native slider
contacts per control use 40% travel and a 0.5-second triangle period. Actual
SurfaceView presents inside each contact are counted; all controls traverse
enough numeric steps to change faster than the target rate.

| Motion | Actual presents/s, three contacts | Moving interval p99 |
| --- | --- | --- |
| Selective Color, Neutrals Cyan | 110.814 / 110.349 / 113.314 | 16.667–25.001 ms |
| Selective Color, Reds Cyan | 113.889 / 113.982 / 114.089 | 16.667 ms |
| Channel Mixer, Red Green coefficient | 112.983 / 115.249 / 112.204 | 16.667 ms |
| Channel Mixer, Red Constant | 109.058 / 112.526 / 109.257 | 16.667 ms |

None meets the sustained 120/s target. The initial Selective Color shader scanned
36 controls per pixel and evaluated hue even when only neutral corrections were
active. Preparing nine correction vectors and two section flags once per edit
removes those scans and skips inactive calculations. Neutrals Cyan improves from
65.33–73.73 presents/s to 110.35–113.31; the unchanged Mixer coefficient previously
presented at 111.24–113.28. Independent numerical and photographic comparisons
remain unchanged. Both effects pass reduced-graph quality checks and retain
native evaluation for exact export.

Separate traced profiles put Neutrals Cyan composition at 10.768 ms median /
10.908 ms p95 before preparation and 5.953 / 6.377 ms afterward. Source admissions
are close (769 versus 760 slots), both retain five hierarchy levels, and the
motion windows record 12 versus 16 source misses with no upload drains. Owner
CPU medians are 7.305 and 7.803 ms. The two live parameter buffers grow from
1,280 to 1,664 bytes. Tracing reduces presentation rates and GPU scopes include
scheduling gaps; these profiles diagnose work, not the sustained rate. The
remaining miss is not evidence of a hardware ceiling.

The benchmark awaits completion of Properties scrolling on Compose's UI frame
clock before locating the visible track. Earlier fire-and-return scrolling and
fixed-delay setup did not establish scroll completion and sometimes produced no
value change. Those failed preparations and screenshot-assisted diagnostic passes are
excluded from the table. The final twelve contacts pass without screenshots or
diagnostic probes. Physical pen input and changed-preview latency remain
unverified, as do low/mid reference tiers and continuous process/driver peaks.

Benchmark APK SHA-256:
`5219219a6e6ebec0fd20e30b32f3ac3eb1404c8b2afbd193b44ee053d7275746`.
Full input records, source identities, profiles and setup diagnostics are in
`artifacts/photo-editing-color/p22-performance/p22-final-evidence.json`.

## Color Lookup

Measured on 2026-10-03 on the reference tablet with the original 9504 × 6336
sRGB U8 photo, an owned nonidentity 65³ LUT, sRGB table interpretation, Fit zoom
0.173295, Navigator and the private release benchmark. Native resolution remains
required: admitted sharp tables fail the reduced-rendering quality bounds. Each
motion has three warmed five-second contacts; only the owned SurfaceView's
actual presents inside the input window count. Thermal status stays zero.

| Motion | Actual presents/s, three contacts | Moving interval p99 |
| --- | --- | --- |
| Intensity, sampler/window slider APK `6b37` | 2.990 / 2.773 / 2.717 | 341.678–366.678 ms |
| Intensity, Android import controls build | 3.194 / 2.946 / 2.965 | 333.344–400.013 ms |
| Intensity, preceding shared-resource build | 3.194 / 2.935 / 2.996 | 325.011–333.344 ms |
| Hand navigation, Intensity 100% | 51.291 / 43.695 / 41.338 | 25.002–33.334 ms |

The matched preceding build presents Intensity at 0.399 / 0.200 / 0.200 Hz.
Auxiliary-resource stages now stop shader fusion without forcing image-stage
preparation. Separate traces show total command passes falling from 3,368 to
285 per native update and expensive renderer-owner CPU from 2,508 ms to a
170 ms median. These are total passes, distinct from final effect passes.
The preceding trace observes 759 bounded-source allocations; the corrected trace
observes 734, with 950 source visits and 216 misses per update. The comparable
Threshold trace observes 751 allocations and records 268 total passes with
199 misses per update. The
17-pass difference matches the additional source misses.

Remaining traced work includes 1,330 ms of command finalization, 27 ms of queue
submission and 2,633 ms of bounded waits across the five-second window. Nested
scopes overlap. GPU composition elapsed has a 329.73 ms median and includes
scheduling and CPU submission gaps. Ordinary owner CPU medians are 118–150 ms.
The native full-resolution work, admission-limited decoding and driver costs
remain measured misses; they do not establish a hardware ceiling.

Allocator boundaries retain one immutable LUT allocation of 4,394,112 bytes
across Intensity changes and navigation. Ordinary Intensity observes 763 source
allocations and navigation 749; each observes seven hierarchy allocations totaling
1,290,511,168 bytes. These counts do not prove distinct mip levels. Continuous
process/driver peaks and tablet upload counts are unverified; independent GPU
tests verify one upload across repeated parameter changes. An eight-millisecond
parameter observer measures 41–58 ms to its first changed value, including drag
slop; changed-preview and physical presentation latency remain unverified. A
failed distinct-value-count assertion is retained as diagnostic: sparse sampled
values are not emitted transitions. Final contacts verify numeric range coverage
with 40% travel and a 0.5-second triangle period.

Separate workstation release measurements of the same production 96-byte-header
parser take 26.5–29.1 ms for this 9,886,586-byte LUT. Dropping temporary sample
storage before payload adoption reduces incremental requested Rust allocator
peak from 12,083,776 to 8,788,256 bytes, 27.3%, excluding the retained input.
Owned-payload hydration takes 3.4–3.9 ms without new heap requests; copying and
hydrating takes 3.6–4.4 ms with 4,394,112 additional bytes. These are CPU
diagnostics, not tablet import or OS-memory measurements.

Measured APK SHA-256:
`7d88637bcb8e112228b89f7888b820e9e789ef56bc2eb793d513bbf16008ba35`.
The final source adds a 16-byte empty-resource accounting correction and import
completion wake/error handling. These do not change the measured loaded-project
slider or navigation path. Raw records, workload/source hashes,
CPU memory scope and the failed observer run are retained in
`artifacts/photo-editing-color/p23-performance/p23-final-evidence.json`.

The Android import-controls measurement uses APK SHA-256
`f1af2d5a5cf934d98d6f56f4269485dcc57bc76ae0b8ca3d39baf0c40d2d7180`
and the same native-resolution 65³ resource, digest
`7c4f0421c8d2169516f0ce532022e02e61c11363f890c08fd50e0b1c50244ca1`.
Three five-second Intensity contacts retain thermal status zero and approximately
2.247 GB of reported renderer allocations. The target remains unmet. Raw motion
records and build/resource hashes are in
`artifacts/photo-editing-color/p27-android/performance/final-lut-61mp-intensity-summary.json`.

The sampler/window slider APK, SHA-256
`6b37f85eb43371bc7fd2b797c9bd9ca03952d1c9db63e2793bb1af48a3f95335`,
repeats the same imported 65³ Intensity workload at 2.990 / 2.773 / 2.717 Hz.
The preceding import-controls and original P23 measurements remain separate.
The final native resource, digest and layer resolution are unchanged; native
presets remain a distinct 17³ workload. Records are in
`artifacts/photo-editing-color/p27-android/performance/final-2048-lut-61mp-intensity-summary.json`.

Native controls pass import, cancel, malformed-file refusal, resource replacement,
stale-adoption refusal, undo/redo and save/reopen checks in both themes and both
orientations. The default display is 2880 × 1800 physical pixels at 280 dpi
(approximately 1646 × 1029 dp, reversed in portrait). A separate compact display
uses 1000 × 1600 pixels at the same density (571 × 914 dp). The native picker
request is exercised with intercepted results rather than manual system-picker
navigation. The final APK `37f7` repeats all four default-display routes. Preset completion
takes 491–707 ms and valid import takes 399–490 ms on that build; these boundaries include controls and an exact PNG export
checkpoint, not input-to-photon latency. The built-in preset resources are 17³,
distinct from the imported 65³ motion workload. The valid picker-import timing
uses a small 2³ inverse fixture. Process PSS at replacement
boundaries changes by approximately −10, −27, −66 and −13 MiB across the four
final default-display routes. The compact-display routes remain separately
qualified by the preceding import-controls build. Continuous process/driver peaks and resource-leak absence
remain unverified. Evidence is retained in
`artifacts/photo-editing-color/p27-android/lut-native-evidence.json`,
`lut-refresh-timing.json` and `lut-narrow-evidence.json` in the same directory;
final default-display timings are in
`rebased-halo-fixed/p24-native-lut-timing.json`.

## Gaussian Blur and Unsharp Mask

Measured on 2026-10-03 on the reference tablet with the original 9504 × 6336
photo, Fit zoom 0.17329544, Navigator and default glass. Thermal status is zero.
Each slider starts after actual raster completion and uses three requested
five-second native contacts. Rates count the owned SurfaceView's presents inside
the input window; post-motion completion is separate. Radius denotes Gaussian
sigma, with a soft range of 0–21 and native text entry up to 85.

| Motion and measured build | Actual presents/s, three contacts | Moving interval p99 |
| --- | --- | --- |
| Gaussian Radius, observed sigma 9.7–13.1, slider APK `6b37` | 34.299 / 35.438 / 34.150 | 66.669–75.002 ms |
| Unsharp Amount, native sigma 21, slider APK `6b37` | 0 / 0 / 0 | No moving presents |
| Unsharp Amount, native sigma 85, slider APK `6b37` | 0 / 0 / 0 | No moving presents |
| Unsharp navigation, native sigma 85, APK `6b37` | 42.674 / 41.295 / 42.545 | 25.001–33.334 ms |
| Gaussian navigation, sigma 85, final APK `37f7` | 41.091 / 40.549 / 40.684 | 33.334 ms |

At native sigma 21, slider APK `6b37` drains in 7.51–7.68 seconds after motion,
versus 9.26–9.55 seconds for the matched pre-change Amount workload. Maximum
renderer-owner CPU falls from approximately 3.9 seconds to 2.66–2.72 seconds.
This improves completion cost but does not meet the frame-rate target. At native
sigma 85, the drain is 25.85–26.36 seconds, maximum owner CPU is 3.46–3.64
seconds and maximum callback duration is 15.01–15.23 seconds. First GPU
completion occurs 10.03–10.26 seconds after motion ends. Reported retained
allocation boundaries are approximately 2.214 GB at sigma 21 and 2.244 GB at
sigma 85. Continuous process and driver peaks remain unverified.

Gaussian uses a display grid with texel side 4 at this Fit zoom, making the
observed slider sigma approximately 2.425–3.275 on that grid. The preceding
linear slider reaches sigma 14.3–16.6 for the same physical gesture; the current
shared power mapping reaches 9.7–13.1. This is not a matched-sigma comparison.
The successful Gaussian phase is preserved despite its combined fixture's later
next-document snapshot failure; corrected independent fixtures pass.

Final APK `37f7` completes native text entry to sigma 85 and full raster readiness
in **79.802 seconds**, within the existing 120-second deadline. The earlier
`6b37` build fails that deadline with repeated internal composition work, despite
completed surface submissions. The corrected idle gate and batched refinement
remove that failure. Reported allocation boundaries change from 2.313 to
2.404 GB. Subsequent navigation drains in 33–49 ms after motion, but its moving
rate still misses 120 Hz. Cold completion includes pending source/filter work;
it is not an isolated kernel time or input-to-photon latency.
A separate single five-second interruption diagnostic starts with composition
still pending and records 265 presents, 52.631 Hz. DOWN injection acknowledgment
takes 32.397 ms and the first GPU completion occurs at 208.862 ms; frame/input
identity does not establish input-to-display latency. Refinement then drains for
81.612 seconds after motion. This confirms interruption progresses, but is an
unqualified responsiveness and latency miss, not a three-contact target pass.

Supplementary 6000 × 4000 measurements on this same tablet use Fit zoom 0.2745
and Gaussian texel side 2. With APK `6b37`, Gaussian Radius presents at
16.951 / 16.941 / 17.174 Hz, interval p99 233.341–241.674 ms. Native sigma 85
Unsharp Amount has one present in each recorded five-second prefix, 0.2 Hz.
The original input operations last 6.72–9.89 seconds because release injection
acknowledgment blocks; their whole-operation rates are 0.136 / 0.202 / 0.149 Hz.
Both windows remain recorded. Retained allocations are approximately 1.261 GB
for Gaussian and 2.222 GB for Unsharp. Typed sigma 85 navigation reaches
40.546–42.161 Hz for Gaussian and 38.509–41.752 Hz for Unsharp. Cold text entry
through readiness takes 62.835 and 4.558 seconds respectively. This is
supplementary top-tier evidence, not a mid-tier hardware measurement.

Native Gaussian and Unsharp controls pass all eight combinations of theme and
orientation on final APK `37f7`: typed Radius 85, exact PNG, changed pixels,
undo/redo and save/reopen state and pixels. Surface recovery and all four
Color Lookup import/preset routes also pass on that build.

The implementation prepares 129 paired records with a 256-lane reduction,
accumulates grouped half-scaled FMA pairs and preserves degenerate-kernel
identity. Strict finite RGB, coverage and real PNG tests pass. Native sigma 85
requires radius 255, 128 paired taps and 257 sample requests per blur axis:
514 across two passes, plus one original sample for Unsharp. These are nominal
shader requests, not physical memory transfers or hardware limits.
Budget-admitted 2048-pixel windows reduce the native sigma 21 planner from 70 to
20 windows. In the minimum 256 MiB image-budget model, sigma 85 uses 1024:
its 2048 image bound is 451,805,952 bytes, while the 1024 bound is
200,540,928 bytes. Actual tablet image admission can be larger depending on
composition and retained storage; this model is not a measured universal window
choice. Display refinement
prepares shared halos for at most four output pages per chunk. A real-source GPU
regression completes within bounded page visits at both 96 and 256 MiB, respects
actual image storage limits and matches an independent full-render pixel oracle.
It measures 42.75–44.99 million filter-pass pixels against a conservative
98.64 million old per-page halo bound. These remaining target misses do not
establish a hardware ceiling.

Slider APK SHA-256:
`6b37f85eb43371bc7fd2b797c9bd9ca03952d1c9db63e2793bb1af48a3f95335`.
Final cold/navigation APK SHA-256:
`37f7fc2dbb002d7da09e701659f3638456b7a508428898a3c27743b5a6f6944a`.
Raw measurements, exact source hashes and retained failures are under
`artifacts/photo-editing-color/p27-android/`; offscreen completion, budget and
pixel evidence is in `artifacts/photo-editing-color/p27-b11/display-refinement/`.

## Local guide adjustments

Measured on 2026-10-03 on the reference tablet using the original 9504 × 6336
photo, Fit zoom 0.17329544, Navigator, default glass and the private release
benchmark. Each motion has three warmed five-second contacts. Rates count the
owned SurfaceView's actual presents inside input windows; thermal status is zero.
Current amount, export and successful navigation runs have thermal status zero.
Both effects retain native evaluation: reduced evaluation with the same frozen
guide exceeds the photographic error bounds in 53 of 168 GPU comparisons.

| Motion | Actual presents/s, three contacts | Moving interval p99 |
| --- | --- | --- |
| Shadows | 3.990 / 3.390 / 2.990 | 300.01–341.68 ms |
| Highlights | 1.399 / 1.398 / 1.198 | 750.02–900.03 ms |
| Clarity | 1.991 / 1.950 / 1.762 | 533.35–741.69 ms |
| Hand navigation, Shadows/Highlights | 44.527 / 41.932 / 42.311 | 33.33 ms |
| Hand navigation, Clarity | 44.706 / 40.140 / 40.494 | 25.00–33.33 ms |

Surface configuration previously raced background analysis submissions, causing
“Failed to wait for GPU to come idle before reconfiguring the Surface.” The
configure path now excludes new submissions until reconfiguration finishes and
delivers callbacks after releasing its locks. All six navigation contacts above
complete without that failure. Pixel checks also pass across suspension, Activity
recreation, rotation and GPU replacement. Native callback p99 is 11.95–13.46 ms;
one initial Clarity callback reaches 736.84 ms. The target remains unmet.

The path without an admitted full composition pyramid initially uses 950 effect
passes per update and about 1.52 seconds of renderer-owner CPU. Horizontal
working strips reduce this to 75, but per-tile reductions still produce about
2,504 total command passes. Batching those reductions brings a matched Clarity
trace to about 754 total passes and 457–462 ms of owner CPU, from 1.31–1.33
seconds. Source allowance is closely matched: 445.6 versus 444.6 MB, 425 versus
424 observed source allocations, and approximately 528 misses and 422 hits per
950-tile update. Independent exact-pixel tests reduce 72 cached tiles from 152
total passes to 24, including sparse updates, masks and shifted windows.

The preceding batching checkpoint’s five-second diagnostic observes six composition updates. Nested spans
include 3.860 seconds of composition, 2.210 seconds of bounded waits, 1.443
seconds of command finalization and 15.4 ms of queue submission. These durations
are not additive. The preceding strip checkpoint spends 1.340 seconds submitting
commands in its corresponding contact. Ordinary Highlights contacts with similar
admission reduce late owner CPU from 1.157–1.198 seconds to 338–342 ms. Remaining
native work, repeated decoding under bounded admission and driver costs are
measured misses; these observations do not establish a hardware ceiling. Even a
single RGBA32Float native read/write moves 1.927 GB, or 28.7 ms at the tablet's
67.2 GB/s theoretical peak; actual paths do more work. This arithmetic does not
excuse avoidable CPU or scheduling costs.

The finite-guide repair preserves coverage and computes the weighted mean without
nonfinite intermediates. Current guide publication and settling take 1.146 seconds
for Shadows, 1.223 for Highlights and 1.153 for Clarity, compared with the preceding
1.192 / 1.250 / 1.156 seconds. Current native amount contacts supersede the preceding
Shadows 3.16–4.00, Highlights 1.40–1.59 and Clarity 1.98–2.20 presents/s; sequential
order and admission differences do not establish a controlled rate regression. The 768 × 512 guide occupies one observed
6,291,520-byte allocation. Independent GPU and shared-state observers verify
reuse across 100 own-parameter edits, snapshot leases and release on error or
cancellation. An in-flight 42,119,200-byte reservation releases 5.460 ms after
cancellation in the workstation diagnostic; tablet build/upload counts are not
observed directly.

A real stacked 61 MP export uses Shadows 63 and Clarity 28. Guide-cold capture
takes 1.423 seconds and its PNG worker 29.788 seconds; warm capture takes 18.561
ms and its worker 17.807 seconds. Both current original-resolution U8 sRGB PNGs are
140,471,461 bytes with identical SHA-256
`c96c3d46fc68cf8a423bd9a76f14a9a7cab234548c5fa4e966dcf6cfa824ec89`.
The preceding shader’s workers take 27.860 / 16.883 seconds; its output hash is
retained separately rather than required to match the repaired consumer. Worker time includes exact rendering,
readback, conversion and encoding. A source-opacity change rebuilds both guides
and settles the live renderer in 15.812 seconds, compared with 15.346 seconds
before the finite-guide repair.

These rows do not meet 120 Hz. An eight-millisecond parameter observer includes
drag slop and does not establish changed-preview or physical pen latency.
Source allocation counts are boundary observations, not a decoded-slot census;
continuous process/driver peaks and low/mid-tier qualification remain unverified.
Amount, guide and export APK SHA-256 is
`d10069f5ee46015685dd9ed687d4f64f48eee95489a557fce3b52a175fdc5e85`.
Navigation and recovery use the surface-fix APK
`07f9dbd57f9113fdcad29d59a02cfa5026419499a59fd0b70fecf197861cdfeb`;
records are in
`artifacts/photo-editing-color/waveform-performance/guide-confirmation/configure-fixed-navigation-summary.json`
and `artifacts/photo-editing-color/surface-configure-android/hardware-final.local.md`.
Finite-guide confirmation records and source hashes are under
`artifacts/photo-editing-color/waveform-performance/guide-confirmation/current-evidence.json`.
The preceding APK is
`6a47d7166150ea05e0533a19dd431f36e5a4ad4d5f93c3b83c7ba151b9ad4e66`;
its batching diagnostics and independent lease observers remain under
`artifacts/photo-editing-color/p25-performance/final-evidence.json`.

## Exact artwork samples

Measured on 2026-10-02 on the reference tablet, using the original 9504 × 6336
tier photo, a nondebuggable release-Rust benchmark build and a private application
ID. The first 101-pixel circular Visible sample completes in 49.51 ms. Ten
repetitions per width give worker medians of 11.88, 12.20, 11.00, 10.53 and
10.76 ms for widths 1, 5, 15, 51 and 101. These exclude owner-side snapshot
capture and UI publication; they do not establish tap-to-visible latency.
LayerContent and retained-source reads also succeed after the live document is
replaced. Cancellation 2.51 ms after launching a ready-source request returns
29.76 ms later. Separate regressions cover cancellation during unpublished
raster roots and tiles, without cancelling their producer.

Three warmed five-second pan runs per condition use a 2880 × 1800 surface, Fit
zoom 0.16534, the default Navigator, the photo and an empty paint layer.

| Moving-window measurement | No queries | Concurrent 101-pixel queries |
| --- | ---: | ---: |
| Renderer submissions/s | 38.31–42.79 | 48.65–49.90 |
| Completed canvas updates/s | 38.11–42.59 | 48.25–49.70 |
| Completion-gap p99 | 32.42–33.59 ms | 24.15–25.80 ms |
| Query completion median | — | 19.29–19.66 ms |
| Query completion p99 | — | 27.20–29.13 ms |

The query streams cover the entire motion window; 244–248 requests start during
each contact. This comparison observes no sampling-related slowdown. Run order
and clock scaling prevent attributing the higher rate to sampling.

Three further warmed five-second contacts on the final integrated build measure
actual presents from the private canvas SurfaceView's SurfaceFlinger latency
records, deduplicated and restricted to the input window. Canvas presentation is
48.43–49.89 Hz with 25.00 ms interval p99. Completed updates are 48.63–50.09/s;
query medians are 19.22–19.52 ms and p99 is 26.45–27.60 ms. Both presentation
targets are missed. Low and mid tiers remain unmeasured.

A separate graphics trace shows stable source misses, upload drains and composite
pixel counters during pan: navigation reuses prepared artwork. The viewport still
draws 5.184 million pixels per moving frame and takes 9.72–11.77 ms on the GPU;
glass backdrop work occurs in roughly half those frames. The diagnostic window
is inferred from input counters and tracing adds overhead. These observations
identify the remaining viewport workload, not an absolute hardware limit.

Allocator boundaries around isolated sampling change from 2173.96 to 2174.15 MiB,
with 2236.89 MiB reserved. After document replacement and the retained query,
allocated memory falls to 9.14 MiB; after cancellation, reservation falls to
56 MiB. The concurrent-query boundary is 2392.55 MiB allocated. The benchmark
pre-captures 600 shared snapshots in 70–245 ms outside each motion window;
interactive picking retains one active request and one newest point. These are
boundary observations, not peaks or total process memory. PSS was not captured;
post-run system MemAvailable is 6,820,124 kB and thermal status is zero. These
overlapping measures must not be summed and do not qualify the memory target.

Final benchmark APK SHA-256:
`c23f0cb49f060da9ad19fd04649e66461a9e8ec99c1f209180234f1271c15b2c`.
Raw records are under
`artifacts/photo-editing-color/p15-performance/`, including
`moving-comparison.json`, `final-sample-summary.json`,
`final-navigation-summary.json` and `baseline2/diagnostic-audit.csv`.

## Web scopes

Chrome on the reference Pro 14 was measured on 2026-10-03 with the 9504 × 6336
photo, the normal Photo workspace and three native five-second pan contacts per
condition. The frozen final-layout WASM digest starts `eecc98acc73f`; complete
source identities and records are in
`artifacts/photo-editing-color/p19-web-pro14/final-layout/`.

| Histogram condition | Chrome surface presents/s | Present gap p99 | Artwork canvas acquisitions/s |
| --- | --- | --- | --- |
| Analysis pending | 111.64 / 112.40 / 111.27 | 16.67–25.00 ms | 113.76–114.95 |
| Exact settled | 111.96 / 113.22 / 113.88 | 16.67 ms | 112.68–114.39 |

The Chrome surface includes DOM scope updates; artwork canvas acquisition is a
separate submission proxy, not display presentation. Every contact moved the
camera and preserved paint. Input event age p99 was 9.1–9.5 ms pending and
8.9–9.0 ms settled. There is no input-to-photon identity. Surface clock alignment
has 14–39 ms uncertainty. Thermal status was 0 afterward; initial thermal status
was not captured. Earlier owned test tabs remained open in the background; their
memory footprint was not measured. These results do not establish a matched improvement over
native or earlier combined pan, zoom and rotation workloads, or meet 120 Hz.

Pending show-to-Exact worker completion took 14.06–14.89 seconds; completion after
release took 8.57–9.44 seconds. Preview requests took 2.79–3.19 seconds and Exact
requests 10.58–11.01 seconds. Each transferred 168,012,079 bytes and explicitly
returned `retire=true`, so the 256 MiB idle-arena policy did not reuse these large
workers. Actual histogram pixels were nonempty. Hiding during a pending query
terminated its worker without a late response. Separate cold/warm pilots have
different workspace geometry and draw implementations and establish no tier gain.

Waveform used the rebased WASM (`e3d5f8b0005`) with 254px scope panels on the same
61 MP source, with three five-second contacts per condition. Pending Chrome
surface rates were 110.71 / 109.83 / 109.83 presents/s; Exact-settled rates were
109.65 / 108.74 / 111.59. Present gap p99 was 25.00 ms pending and 16.67 ms settled,
with input event age p99 9.0–9.6 ms and clock alignment uncertainty 14.8–20.8 ms.
Both thermal checks reported status 0. All contacts moved the camera, preserved
paint and retained nonempty 424 × 280 Waveform plots.

Pending pointer release to observed Exact status took 9.50 / 9.88 / 10.08 seconds;
this is query readiness, not input-to-display latency. A separate admitted Preview
request transferred 168,012,079 bytes and was terminated 37.4 ms after hiding,
without publishing a late completion. Only the owned measurement tab was active.
These results do not meet 120 Hz or establish a matched gain; complete source and
evidence are in `artifacts/photo-editing-color/p19-web-pro14/rebased-final/`.

The bounded 512² fixture transferred 1,042 bytes per request, returned
`retire=false`, reused one worker for Preview and Exact (259.1 and 58.2 ms), and
expired 5.000 seconds after the last response. These are different query modes,
not a matched startup speedup. A separate CPU trace measured synchronous sends
at 1.3–2.1 ms; sampled main-thread packing totaled about 210 ms and worker
unpacking about 130–135 ms per query. Tile digest validation during region decode
accounted for about 597/926 ms of Preview/Exact self samples. Sampling estimates
and overlapping inclusive costs do not identify GPU or asynchronous device wait
time. Pilot, retirement and CPU evidence is retained in the adjacent
`after-reuse/` directory.

Precision controls used a separate frozen copy of the `e3d5f8b0005` WASM plus the
numeric-coordinate CSS fix, the same 61 MP photo, and three native five-second
contacts per row. The Photo Properties dock was active; both artwork scope
panels were hidden, with viewport 2880 × 1590 and work area 2257 × 1422.

| Precision contact | Chrome surface presents/s | Present gap p99 | Post-contact completion |
| --- | --- | --- | --- |
| Levels black slider | 44.57 / 49.01 / 41.45 | 41.67–91.67 ms | 10.8–17.4 ms |
| Curves interior knot | 40.57 / 38.81 / 35.63 | 50.00–91.67 ms | 11.7–17.5 ms |
| Targeted Curves adjustment | 49.58 / 48.09 / 48.14 | 58.34 ms | 13.12–14.54 s |

Each contact committed real parameter changes; final Undo restored the initial
curve parameters. Thermal status was 0 before and after, input event age p99 was
9.5–11.0 ms, and clock alignment uncertainty was 14.0–25.4 ms. No page errors
were recorded. These Chrome surface rates do not meet 120 Hz and do not establish
artwork or input-to-photon rates. Levels insertion to Exact worker response took
12.42 s (Preview 4.64 s, Exact 6.91 s); both requests transferred 168,012,079
bytes and retired their workers. Targeted five-pixel EffectInput requests took
0.684–0.741 s, transferred the same bytes, and returned `retire=false`. Their
13–14 s post-contact wait was the re-enabled Exact tonal histogram, not the
pixel-sample latency. One bounded held-touch observation reached a finite
preview in 1.415 s including the stationary hold, updated after movement, and
cancelled without changing parameters. This pilot does not qualify Auto completion
on the large photo; correction behavior is qualified by the separate bounded UI
fixture. Complete source identities, successful
rows and rejected fixture attempts remain in
`artifacts/photo-editing-color/p19-web-pro14/precision-final/`.

## Artwork statistics

The initial Histogram checkpoints were measured on 2026-10-02–03 on the reference
tablet, with the original 9504 × 6336 photo and private, nondebuggable ARM64
benchmark using release Rust. Current query/cadence controls appear in
[Waveform statistics](#waveform-statistics) below. Visible
Preview counts 65,536 deterministic original-grid positions; Exact counts all
60,217,344 pixels. Counts and channel totals pass in every reported request.
First requests start after photo import and source preparation, so they do not
measure cold import or tap-to-visible UI latency.

Warm Preview improves from 17.19 s to 1.636 s through sparse composition, fewer
idle classifier groups and workgroup counters. Exact improves from 45.50 s
(45.07 s first request) to 10.464 s (7.903 s first request). Classifier changes
preserve exact boundary correction and move rare wide luminance work to a
separate kernel. An intermediate GPU trace measures counting falling from
34.55 s to 2.00 s. With that classifier and 950 prepared windows, warm worker
time is 11.36 s: preparation, restoration and submission outside the capture
callback take 5.21 s, while completion waits take 5.33 s. The captured GPU span
is 2.53 s and excludes preceding upload commands; it must not be added to
worker elapsed time.

Preparing 1024-pixel source windows reduces Exact to 5.59 s, but submitting all
16 native tiles together lowers navigation to 19.96–22.75 presents/s, with
75–100 ms interval p99 and 89 ms cancellation return. The accepted per-tile implementation
retains that source preparation and submits and waits per 256-pixel Exact tile.
Exact cancellation returns in 15.19 ms after cancellation at 100 ms; Preview
returns in 7.25 ms. This restores short submission boundaries at the cost of
longer query completion.

Nine final five-second OS two-finger pan contacts use a 2880 × 1800 surface,
Fit zoom 0.16534, default Navigator, the photo and an empty paint layer.

| Moving-window measurement | No queries | Histogram Preview | Histogram Exact |
| --- | ---: | ---: | ---: |
| Actual canvas presents/s | 36.72–41.63 | 38.94–41.24 | 36.61–39.30 |
| Presented interval p99 | 33.33–41.67 ms | 33.33–41.67 ms | 33.33–41.67 ms |
| Completed updates/s | 36.92–41.63 | 39.14–41.44 | 36.81–39.30 |
| Completion-gap p99 | 32.73–35.08 ms | 35.99–37.20 ms | 35.99–36.31 ms |

Four sequential immutable requests cover each complete moving window. Preview
streams last 8.14–8.32 s; the Exact request overlapping motion lasts
12.28–12.64 s. Post-motion drain, 3.12–3.31 s for Preview and 36.20–38.95 s for
the four Exact requests, is excluded from cadence. Actual presents come from
the owned SurfaceView's SurfaceFlinger latency records, deduplicated and
restricted to each input window. All nine GPU timing assertions pass. The large
Exact interference regression is absent in these contacts; Preview has slightly
larger completion tails. Sequential run order and uncontrolled clock scaling
prevent claiming an average-rate improvement. Neither condition meets the
presentation target, and these results do not establish a hardware ceiling.

Separate tracked requests on the final algorithm observe shared-device peaks
of 2267.17 MiB allocated/2332.89 MiB reserved for Preview and
2252.03 MiB allocated/2316.89 MiB reserved for Exact, at 70 capture boundaries
per request. They include the live canvas and exclude CPU sources and
private driver memory; continuous transient peaks and motion PSS remain
unverified. Thermal status is zero. Low/mid tiers, effects, animated sources
and full Histogram UI publication are not qualified by this static Visible
transport workload. The tracked APK hash differs from the ordinary timing
APK and is recorded in `p17-performance/final-allocator-summary.json`.

Final benchmark APK SHA-256:
`53985e0cf0bce0d0ed4f986e1d72b0b5fd8e3a815f4c4c0eb54ed2dbfb73f8b2`.
Records are under `artifacts/photo-editing-color/p16-performance/`, including
`baseline-summary.json`, `fallback-diagnostic-summary.json`,
`shared-window-navigation-summary.json`, `tile-submit-summary.json`,
`tile-submit-navigation-summary.json`, `common-capture-navigation-summary.json`
and paired APK/source hashes.

## Waveform statistics

Measured on 2026-10-03 using the original 9504 × 6336 photo and an owned
same-size uniform white U8 sRGB JPEG on the reference tablet. Both use the same
immutable Visible query, exact original-grid counts, Fit and Navigator. Waveform
adds four 256 × 256 count planes. Shared GPU tests verify spatial counts directly;
Android asserts histogram channel totals while timing the complete worker,
including the Waveform readback. Full planes are omitted from UI serialization.

| Warm worker completion, two requests after the first | Histogram only | With Waveform |
| --- | ---: | ---: |
| Photo Preview | 1.635–1.734 s | 1.676–1.793 s |
| Photo Exact | 10.507–10.525 s | 11.546–11.766 s |
| Uniform Preview | 1.634–1.638 s | 1.644–1.653 s |
| Uniform Exact | 8.303–8.367 s | 9.809–9.853 s |

The first request is recorded separately and is not a hardware-cache cold test.
The initial uniform Exact implementation adds about 32% wall time. Aggregating
repeated bin/column keys within each invocation reduces warm overhead to about
18%; the photo Exact overhead is about 10–12%. This removes repeated atomic
writes without extra buffers or changing counts. Sequential order, source
admission and clock scaling prevent a controlled GPU-speedup claim.

| Three five-second navigation contacts | Actual presents/s | Presented interval p99 |
| --- | ---: | ---: |
| No queries | 34.94–36.33 | 41.67 ms |
| Histogram Preview | 35.78–36.90 | 41.67–50.00 ms |
| Waveform Preview | 35.29–36.53 | 41.67–50.00 ms |
| Histogram Exact | 33.56–35.81 | 41.67 ms |
| Waveform Exact | 33.47–35.58 | 41.67 ms |

Four Preview requests or one Exact request cover each complete moving window.
Actual presents come from the owned SurfaceView's deduplicated SurfaceFlinger
latency timestamps restricted to injected motion. Post-motion drain is excluded:
2.85–3.41 s for Preview, 7.02–7.28 s for Histogram Exact and 7.97–8.25 s for
Waveform Exact. These current Histogram controls supersede the earlier cadence
rows above. Every condition misses 120 Hz; no hardware ceiling is established.

Waveform cancellation returns in 4.67–5.44 ms for Preview and 23.02–26.03 ms
for Exact after a five-millisecond worker delay. Three reopened requests have
stable allocated-byte boundaries after each result releases. Preview cancellation
briefly retains about 540 KB of small capture resources; the 250 ms settled
boundary returns to the preceding value. The requested Waveform budget includes
10 MiB for shards, summary and readback staging. These observations include the
live canvas and do not measure continuous allocator, process or driver peaks.
Process PSS and system MemAvailable are recorded separately. Android has no
Histogram/Waveform UI port: this is query interference with Navigator visible,
not qualification of panel-open motion or UI hide/reopen resource lifetime.

The initial APK SHA-256 is
`7dabed7684d03ffef5eeb43a19b2d0eac7b27626d92e23cfc9da35e87cc87aa8`;
the current optimized APK is
`d10069f5ee46015685dd9ed687d4f64f48eee95489a557fce3b52a175fdc5e85`.
Test APK, source/fixture hashes, thermal observations, individual requests and
navigation windows are under
`artifacts/photo-editing-color/waveform-performance/current-evidence.json`
and `initial-latency-evidence.json`. Low/mid tiers, changed-preview latency and
reference-hardware panel interactions remain unverified.

## Clipping preview

Three five-second contacts on the same 61 MP photo, with both clipping flags
enabled and no query workers, present at 36.01–38.50/s. Presented interval p99
is 33.33–41.67 ms; completion-gap p99 is 34.58–35.47 ms. Matched Histogram
controls above use the same photo and Navigator. All three presentation
assertions pass, but the 120 fps target is not met. The full shared snapshot
confirms both flags. The filtered photo capture at Fit does not contain
identifiable clipping-marker pixels; a separate known clipped solid-color
capture checks the native marker presentation outside the timing workload,
with 882 black and 877 white marker pixels in a 64 × 64 center sample.
This does not qualify the full Histogram UI or different source/view workloads.
Raw frames, flags and timing are under
`artifacts/photo-editing-color/p16-performance/final-clipping-motion2/` and
`final-clipping-navigation-summary.json`; the separate marker frame is in
`final-clipping-marker/`.

## Auto statistics worker

On the same reference tablet and original 61 MP photo, the real Levels
EffectChannels query improves from 23.56/25.29 s first/warm to 15.35/15.11 s.
It prepares bounded 1024-pixel source windows, submits four 256-pixel min/max
tiles per boundary and one counting tile per boundary, uses 16 counting shards
instead of 64, and folds once after counting. All channel totals pass.
Cancellation at 100 ms returns in 10.12 ms.

Before the last change, Android GPU timestamps show warm min/max taking
7.85 s with 0.83 s of captured GPU work, and counting taking 14.46 s with
3.81 s of GPU work. CPU submission and completion waits take 7.03/13.34 s
across 950 tiles per pass. Source preparation is small and the final fold takes
2 ms. These intervals overlap GPU execution; they do not sum to independent
costs or establish a hardware ceiling.

Three matched five-second navigation controls use the same default Levels
fixture and present at 36.68–39.08/s, with interval p99 33.33–41.67 ms.
Three contacts with one pending Auto query present at 39.40–40.04/s, with
33.33 ms interval p99 and 32.24–32.79 ms completion-gap p99. The requests last
16.49–16.91 s, covering each moving window; post-motion drain is excluded.
Separate tracked Auto observations find 2257.34 MiB allocated and 2316.89 MiB
reserved over 140 capture boundaries, including the live canvas. They exclude
CPU and driver-private memory and do not measure a continuous peak.
All six presentation assertions pass. Sequential run order and clock scaling
prevent claiming a rate improvement, and neither workload meets 120 fps.
This measures the shared statistics worker, not complete Android Auto UI
publication or parameter adoption. Records and APK hashes are under
`artifacts/photo-editing-color/p17-performance/`, including
`batch4-shards16-summary.json`, `batch4-shards16-navigation-summary.json` and
`diagnostic-summary.json`. Temporary profiling code is removed.

## GTK targeted Curves diagnostic

A private 1100 × 800, 120 Hz headless Mutter display on NVIDIA RTX PRO 6000
Blackwell Max-Q/Vulkan measures a 256 × 256 opaque sRGB U8 colored gradient.
After warm-up, three five-second native mouse gestures per theme use requested
8 ms input intervals. Changed-preview presentations run at 117.57–118.40/s,
with moving interval p99 8.519–16.677 ms. Injected Down to first observed curve
adoption takes 28.434–39.184 ms; first changed-preview presentation takes
37.372–48.234 ms. The requested 1 ms GLib observer includes routing and polling
uncertainty, so this is not pure GPU request latency. The small canvas and
non-reference hardware do not qualify a tier. Raw records:
`artifacts/photo-editing-color/p17-p18-gtk/targeted-timing-1100.json`;
host details: `artifacts/photo-editing-color/batch-host-inventory.json`.

## Transform snapping

Measured on 2026-10-02 at `3e0684b08` plus the transform controls, on the reference
tablet at thermal status zero. Both runs use the same nondebuggable release-Rust
benchmark APK as the final Curves run, the exact tier photo, default Photo
workspace, Fit zoom 0.203125 and closed Stats. A small real paint stroke supplies
a neighboring snap target; photo, paint and paper are the three visible layers.
Each run has one warm-up and three five-second body drags.

| Warm measurement | Snapping on | Snapping off |
| --- | ---: | ---: |
| Renderer submissions/s | 59.69–60.47 | 59.86–60.11 |
| Completed canvas updates/s | 59.29–59.87 | 59.46–59.71 |
| Renderer owner CPU, median | 9.60–10.01 ms | 9.71–10.06 ms |
| Renderer callback, p99 | 27.33–28.30 ms | 27.46–27.69 ms |

The comparison detects no significant snapping-specific overhead. Target bounds
are prepared asynchronously and frozen for each contact; motion reuses them.
The existing transform workload remains below 120 Hz. GPU completions do not
measure screen presents, and the mostly stationary native UI supplies too few
FrameMetrics samples to qualify canvas presentation. Callback p99 is not an
update-gap measurement.

Allocator samples before and after every warm contact remain at 2,482,850,752
allocated bytes with snapping and 2,490,216,384 without, with the same
2,553,843,712-byte reservation. These are boundary samples, not peak memory.
Records: `artifacts/testing/material/p11-p14-benchmark/rebased-10/snap-comparison-summary.json`.
Low and mid tiers remain unmeasured.

## Curves editing

The final integrated build at `3e0684b08` plus this change uses the same photo,
camera, workspace and three warm five-second contacts described below. At thermal
status zero it reaches 72.47–74.81 renderer submissions/s and 72.07–74.61 completed
canvas updates/s. Renderer owner CPU medians are 4.12–4.18 ms, with p99
7.34–7.56 ms. Native UI rates are 76.51–78.53 frames/s with 25.00 ms interval p99;
neither presentation criterion is met. Allocator boundary samples stay at
2,265,137,024 allocated bytes and 2,328,776,704 reserved bytes, not an in-motion
peak. APK SHA-256:
`69b731b96ecb25a634c35b07794ef30431d8e95504f7ad88ddb3b20b3f86d47a`.
Records: `artifacts/testing/material/p11-p14-benchmark/rebased-10/curves`.
This confirmation includes the integrated live-language implementation and is separate
from the comparison that isolates the renderer fix below.

Measured on 2026-10-02 at `2e574cb20` plus the Properties and pointwise renderer
changes, on the reference tablet at thermal status zero. The exact tier photo
uses Perceptual blending, the default Photo workspace, Fit zoom 0.203125 and
closed Stats. Both nondebuggable benchmark builds use release Rust with R8
disabled for instrumentation. The same test drags the existing middle point of
a three-point RGB curve for one warm-up and three five-second repeats.

| Warm measurement | Tiled baseline | Bounded whole-window evaluation |
| --- | ---: | ---: |
| Renderer submissions/s | 10.38–10.57 | 78.40–78.63 |
| Completed canvas updates/s | 10.18–10.37 | 78.00–78.23 |
| Renderer owner CPU, median | 66.32–67.31 ms | 4.29–4.36 ms |
| Native UI frames/s | 68.19–74.82 | 84.02–86.37 |

The updated run's completion-gap p99 is 24.55–25.17 ms. Native UI interval p99
is 16.67 ms, but its rate is below the 114 fps presentation floor. Canvas updates
are GPU completions, not screen presents. The 120 Hz target remains unmet.

Pointwise effects now reuse the existing whole-window graph evaluator when its
sources and scratch fit the existing 608 MiB display allowance. Larger graphs
and native-resolution evaluation retain tiled scratch. Independent pointwise
regions share a render pass; spatial filters retain bounded passes. This removes
the measured driver overhead without changing effect math or source resolution.

A separate Stats trace reduces effect passes from 140 to a median two per
update and command finishing from 40.21 to 0.85 ms. The changed preview contains
3.764 MP: main GPU composition takes 6.89 ms median and viewport work 2.69 ms.
Nested GPU intervals overlap and must not be summed. The warm trace has no
upload drains or restores and only three source misses across 357 updates.
Native shared effect actions take 0.36 ms median; publication takes 2.35 ms plus
0.30 ms parsing. Qualification renderer CPU stays around 4.3 ms while native UI
GPU time is 12.44–12.87 ms and presentation queue waits reach 8.08–8.50 ms p95.
These costs are consistent with the remaining GPU and native UI workload; the
audit found no further major repeated source processing or command amplification.
They do not prove an absolute hardware limit or qualify a soft-target waiver.
The Stats trace changes the work area and adds timing overhead, so it is not
pooled with the rate runs. Android FrameMetrics CPU phases report elapsed time,
not actual UI-thread CPU time.

Allocator samples before and after each warm contact remain at 2,273,446,784
allocated bytes and 2,345,553,920 reserved bytes; the baseline allocates
2,274,506,304 bytes with the same reservation. These are boundary samples, not
an in-motion peak or total process memory qualification.

Candidate APK SHA-256:
`5aecd79c39fd61bd0c3c900a410bd06408b32e3b3a78583f804bec5a18a37b46`.
Baseline APK SHA-256:
`d40c3dd859d448532db2fd2dc776570c36f794adfa1d58dd09cdbf4cef27eb1d`.
Raw records are under `artifacts/testing/material/p11-p14-benchmark/`:
`matched-baseline-08/qualification`, `optimized-08/qualification` and
`optimized-08/trace-summary.json`. Low and mid tiers remain unmeasured.

## Exact content bounds

Measured on 2026-10-01 on Wacom MovinkPad Pro 14 (DTHA140), Android 15,
thermal status 0, with a release build and a 9504 × 6336 synthetic RGBA source
with a ten-pixel transparent border. These are worker query completion times,
without the UI result cache. All four queries per scope return the exact expected
rectangle. They do not measure command publication, first motion or presentation.

| Query | First run | Three repeated runs | Completion within 100 ms |
| --- | ---: | ---: | --- |
| Transform target | 134.9 ms | 94.6–103.5 ms | Not consistently |
| Visible content, used by Fit Content | 214.7 ms | 120.2–142.6 ms | No |

Boundary-first reduction and tile-aligned snapshot origins reduce target queries
from 804.1 ms first / 657.4–703.2 ms repeated, and visible queries from 4895.4 ms
first / 4564.9–6190.1 ms repeated. Subsequent readback batching, exact copy
footprints and shared immutable pipelines and transfer tables remove repeated
work. Visible scans use 122 pages and 16 readbacks instead of 186 of each on this
fixture. Combining page submissions regressed target completion in adjacent A/B
runs and is not used. Comparisons are against preceding GPU implementations,
not the former CPU scan.

Cached results avoid another query; opaque unedited RGB sources use their proved
extent. The remaining completion times are not evidence of a hardware limit.
Low and mid tiers remain unmeasured, and these timings qualify no moving-frame
target. Android debug-build command diagnostics are also not release performance
qualification.

The nondebuggable Android benchmark build (release Rust, R8 disabled for the
instrumentation ABI) reopens a cancelled linked Transform from its cached paint
and mask bounds. Mouse/touch/pen dispatch reaches shared state in
38.4/41.5/41.5 ms and the composed Apply button in 63.5/69.0/67.1 ms. These three
command-readiness observations are below 100 ms; the test polls every 16 ms and
does not measure first-motion presentation or sustained frame rate. A shared
regression verifies that reopening submits no new bounds query.

Base `271918681` plus the bounds change; release test executable SHA-256
`c1222ba05dfcd5058e70b89f1de53c205a5536ee17147f354a8ca002d97b9985`.
Records: `artifacts/testing/profiling/bounds-final-release-timing.log` and
`bounds-alternating-phases.log`, plus
`artifacts/testing/android-benchmark-publication-split.log` and
`android-benchmark-cache-apk.sha256` in the photo-editing worktree.

## Retained wet-photo transforms

Measured on 2026-10-02 at `eb9b8bab1` plus the retained-material changes, on
the reference tablet at thermal status zero. The exact 9504 × 6336 tier photo
has an accepted Wet Watercolor stroke, Linear blending, Fit zoom 0.16534 and
Navigator open. The nondebuggable benchmark uses release Rust. Four five-second
translations include one cold motion and three warm repeats.

The warm repeats submit 36.34–36.95 renderer updates/s, with callback medians
22.66–24.26 ms. Median submission duration is 6.315–6.472 ms, with p95
16.20–17.01 ms. Transform dispatch after the new stroke reaches shared placement
state after 87.889 ms; pending-to-idle takes 201.339 ms. This single shared-state
observation is below 100 ms but does not qualify press-to-first-motion latency.
These are renderer and shared-state timings, not presented frame rates;
the 120 fps target remains unmet.

A separate trace before the float-document precision guard observes median
main GPU time of 21.68 ms (p95 26.36 ms),
command finishing of 4.88 ms and queue submission of 0.65 ms. Before the box
reduction change, main GPU time was 29.23 ms (p95 36.91 ms). Across 141 moving
callbacks, 3,594 Color and 3,377 watercolor-wetness page mappings are all unique
within their frames. The preceding 32-page cache repeats 24.6% of its mappings
and takes 37.63 ms median main GPU time. The 64-page cache and omission of unused
color neighbors remove that duplicate work. Admission reserves the larger
bounded cache before allocating it.

Repeated-dispatch probes before the box reduction estimate 8.81 ms for raw-plane mapping,
1.51 ms for watercolor, 4.88 ms for display-area sampling, 8.25 ms for native
reduction and 0.23 ms for composition. These differences include scheduling and
cache effects; they are cost estimates, not physical lower bounds. The reduction
averages approximately 1.72 million native pixels into 108,000 output pixels per
frame. Replacing its complete linear-color blocks with hardware box averages
reduces measured main GPU time to 21.68 ms, p95 26.36 ms.
Partial edges, nonlinear color conversion and float-document material differences
retain the general reduction.

The audited motion maps each required raw page once, retains the photo's source
levels, evaluates material only around wet content, and filters 3.76 million
display pixels. Raw mapping and display sampling alone account for approximately
13.7 ms on this tablet, against a 6 ms GPU budget. After removing the demonstrated
duplicate work and slow reduction, the remaining gap is consistent with the
measured sampling workload on this hardware. This assessment does not establish
an absolute hardware limit or qualify the 120 fps target.

Bulk destruction of completed Android command pools keeps process mappings
between 8,377 and 8,443 through the final four motions, from 8,409 beforehand,
and returns to 8,399 after idle. The old pool-retention policy exceeds 27,000
mappings during brush warm-up and is stopped
by the private-process guard. This establishes the observed stability of these
workloads, not every brush or an absolute memory maximum.

The separate 61 MP bake run, before the float-document precision guard,
completes Apply Transform to Pixels in 18.793 s. The guard changes display
reduction, leaving this raw bake path unchanged.
The result keeps 950 Color and ten watercolor-wetness pages plus material style,
removes the original source, sets identity placement, and reopens with exact
native raster backing. Across 141 off-thread samples, observed peak PSS is
828.0 MB, tracked GPU allocation/reservation peaks are 2.758/2.837 GB, and
system available memory stays at least 7.904 GB. These overlapping measures
must not be added; sampling does not establish absolute peaks. Pending shared
state is observed after 129.9 ms, not a presented-response qualification.

Clean final benchmark APK SHA-256:
`dd64a08b6f2b0a209a18b77fc4192bbebcdae493129df18deb055e5cfcfd0ca0`.
Box-reduction trace and large-bake APK SHA-256:
`d00c75cb7b2d7ea98773e1e18b732640aa9eecf3a548de55c73a46f7ab2273da`.
Preceding cache-counter APK SHA-256:
`c85304da262916f9983fb4d13643dde16b3058595b4aebd295708596351533a6`.
Records: `artifacts/testing/material/{android-box-hdr-final,android-box-reduction}` and the preceding
`android-lru64-probe`, `android-placement-calibration`,
`android-watercolor-calibration`, `android-area-calibration`,
`android-reduce-calibration` and `android-compose-calibration` directories
in the photo-editing worktree. Low and mid reference tiers remain unmeasured.

## Folded warp source planning

Measured on 2026-10-04 on the reference Wacom MovinkPad Pro 14, using the
original 9504 × 6336 Sony photo, Fit, default workspace and glass, and a private
nondebuggable APK with release Rust. The canvas-bar fixture imports the photo,
adds its empty paint layer and a painted snapping neighbor: three visible
layers, with snapping enabled. Each build runs a priming Warp gesture, then
three six-second grid-point drags. Thermal status is zero before and after
each measured run.

| Build | Renderer submissions/s, three gestures | Completed-update interval p99 | Render-owner CPU p99 |
| --- | --- | --- | --- |
| `e90ae51a1`, aggregate source rectangle | 19.805 / 19.480 / 19.148 | 89.365 / 88.206 / 89.658 ms | 70.583 / 71.245 / 72.317 ms |
| Same base, per-triangle source pages | 19.809 / 19.494 / 19.303 | 89.215 / 88.612 / 88.637 ms | 72.112 / 70.143 / 71.128 ms |
| Rebased onto `00b2d6e73`, per-triangle source pages | 19.473 / 19.147 / 19.320 | 90.369 / 90.278 / 87.881 ms | 71.340 / 72.481 / 71.559 ms |

The median submission rate changes by +0.07%; this comparison shows no
responsiveness regression in the measured workload. The rebased build's median
is 19.320 submissions/s, within the matched builds' observed range. Submission and completion
rates do not establish presented frames or input-to-photon latency, and the
120 fps target remains unmet. Low and mid reference tiers are unmeasured for
this change. Raw input windows, reports, APK hashes and thermal observations
are in `artifacts/warp/performance-summary.json` and its `baseline/` and
`final/` and `rebased/` report directories.

## Retained Distort and Warp

Measured on 2026-10-02 on the Wacom MovinkPad Pro 14, using the private
Android benchmark app with release Rust, a nondebuggable release-derived app,
and R8 disabled for the separate instrumentation APK. Distort uses
`7040f1be6` plus the retained-geometry changes; Warp uses `516a613cb` plus those
changes and the footprint-cache correction. Both load the 9504 × 6336 native
project normally, import the generated opaque photo, and paint a 400 px wet
watercolor stroke. The saved U8/sRGB, Linear-blending input has fourteen Color
and fourteen watercolor-wetness pages. Fit zoom is 0.16534 at a 2880 × 1800
viewport. Each gesture moves a corner or node for five seconds; the table
separates the first gesture from the following three.

| Motion | First submitted / completed updates/s | Warm submitted / completed median | Warm completed range | Warm callback median range | Warm callback p99 range |
| --- | --- | --- | --- | --- | --- |
| Distort | 29.96 / 29.56 | 29.77 / 29.37 | 29.37–29.55 | 30.35–31.29 ms | 37.41–40.87 ms |
| Warp | 16.78 / 16.38 | 16.37 / 15.97 | 15.96–16.18 | 55.88–56.47 ms | 74.39–75.48 ms |

These are submitted drawing updates and GPU-completed updates during motion,
not presented frames. Neither motion qualifies the 120 fps target.
The four Warp postgesture samples observe 8,473–8,500 process mappings,
2.679–2.687 GB tracked allocation, 2.762–2.770 GB reservation, and
0.836–0.952 GB PSS. These measures overlap and finite samples do not establish
absolute memory peaks.

The initial near-unit Distort incorrectly requested full-resolution source
sampling and reached only 0.40 updates/s. Corrected source-level admission and
batched projective sampling reach the Distort result above. Warp additionally
needed tighter conservative stretch and geometry accounting, a sparse mapped
material region, canonical mesh reuse, source-plan reuse, batching, and reuse of
immutable source footprints. Its measured composition falls from 5.16 s to
8.81 ms; prepare falls to 0.56 ms. The final short trace has 3.764 million
display pixels, approximately 93 command passes per update, six cold source
misses, and no upload drains or raster restores during motion.

Separate Stats attribution runs observe Distort main GPU median 30.8 ms and
Warp approximately 40.4 ms for readings added between the before/after snapshots.
The latter includes end updates; it is not a motion-only presentation measure.
Warp command finishing and queue submission take median 27.7 and 20.1 ms in
the ordinary short trace. The material cache uses absolute page coordinates;
Color and wetness share mapped UV positions. A single UV target introduces
render-to-compute dependencies between mapped pages. The remaining GPU and
driver costs are consistent with that audited mesh sampling workload after the
identified repeated CPU work is removed. This does not establish an absolute
hardware limit or exclude future improvements.

Distort APK SHA-256:
`840bf66b4058daec17dda621ceffa07deef0188820eaa647fce66c8e5d5735f0`.
Warp APK SHA-256:
`878768b91230680d4c8b9bc627c8851d3fcef77e648f5b3c08645b9627393c05`.
Raw records and source manifests remain in the photo-editing worktree under
`artifacts/testing/material/p03-android-batch` and
`artifacts/testing/material/p03-android-warp-footprint`. Low and mid reference
tiers remain unmeasured.

## BUILD20 selected imported-photo transforms

The existing fixture opens the 9504 × 6336 JPEG directly, adds a sparse painted
80 px neighbor and verifies snapping off: `acceptedPhoto=true`,
`transformSnapping=true`, `snappingEnabled=false`. It has three visible
occurrences, Navigator, SDR and Fit zoom 0.175284. This is a dry retained photo
workload, distinct from the separate wet-material measurements. All six available
baseline/candidate contact pairs match logical setup; configured memory budgets
differ. Translation endpoints change equally in both builds. Distort and Warp
return nearly to their initial bounds; intermediate fresh rendered poses are not
recorded.

| Motion / contact | Completed updates/s, baseline → M3 | Completion gap p99, ms, baseline → M3 | Owner CPU p95, ms, baseline → M3 |
| --- | --- | --- | --- |
| Imported-photo translation / 1 | 64.712 → 65.299 | 24.566 → 24.576 | 18.519 → 18.270 |
| Imported-photo translation / 2 | 64.476 → 64.252 | 24.789 → 24.150 | 18.605 → 18.700 |
| Imported-photo translation / 3 | 64.111 → 64.307 | 24.993 → 24.342 | 18.824 → 18.401 |
| Imported-photo Distort / 1 | — → 60.937 | — → 27.025 | — → 19.509 |
| Imported-photo Distort / 2 | — → 60.489 | — → 26.172 | — → 19.981 |
| Imported-photo Distort / 3 | — → 61.278 | — → 26.365 | — → 19.432 |
| Imported-photo Warp / 1 | 19.369 → 18.965 | 91.848 → 91.979 | 67.903 → 70.532 |
| Imported-photo Warp / 2 | 19.370 → 18.961 | 88.424 → 89.652 | 63.088 → 70.811 |
| Imported-photo Warp / 3 | 19.380 → 19.375 | 87.856 → 88.404 | 62.545 → 62.712 |

Baseline Distort fails **“Snapping has no painted target”** before any timed
report; its three M3 contacts have no matched baseline. The direct import avoids
the earlier current83 New Document 8192 limit but does not resolve or replace
those six original transform setup failures. Translation and Warp complete below
120/s in both builds, and M3 Distort below 120/s. No matched throughput loss
exceeds 5%, but Warp contact 2 increases owner CPU p95 by 12.24%; the common CPU
gate is not cleared. Translation contact 2 has submit-to-complete p99
32.024 → 57.978 ms and Warp contact 2 76.597 → 97.074 ms. These adverse latency
samples remain explicit, without treating them as input-response measurements.

Tracked after-contact residency is 2317.114 → 2336.114 MiB for translation and
2331.114–2331.362 → 2351.362 MiB for Warp. The +19/+20.000–20.248 MiB boundary
differences are below 5% of baseline residency, with different admission limits
and no allocation attribution. M3 Distort retains 2337.362 MiB with no comparable
baseline. These boundary measurements do not qualify common peak or lifetime
limits.

These are renderer completion diagnostics. Navigator and retained refreshes
count; actual fresh canvas presentation times and pose identities are absent.
Scheduled presentation deadlines and physical panel refresh do not establish
presentation rates. Submit-to-complete latency includes scheduling and polling;
it cannot establish the common input-response p99 bound. GPU execution samples,
continuous memory peaks and process/driver inventories were not collected.

The exact baseline and candidate are the 4a/BUILD20 APKs identified in the
[G-Pen comparison](#build20-g-pen-comparison). Each contact requests five seconds;
all scenario thermal status samples are zero before and after. Raw reports,
verified instrumentation outcomes, each parameter value, per-contact CPU/input
quantiles and configured admission limits are retained under
`artifacts/format/m3-final-android-performance-20/`, with
`canvas-analysis.local.md`, `canvas-analysis.json` and `canvas-gestures.tsv`.

## Current M3 G-Pen comparison

Measured on 2026-10-04 on the reference Wacom MovinkPad Pro 14, with the clean
`192601dac` BUILD29 baseline and M3 BUILD32 based on the same revision. Both
benchmark APKs use release Rust. These frozen measurements precede the
committed `b3f6f8e51` cutover and do not qualify later runtime changes. The
61 MP photo has one empty drawing layer
above it, Solid Color fill hidden, Linear blending, Navigator open and Stats
closed. G-Pen is 2048 px at Fit zoom 16.5341%, with pressure 1, 16 ms prediction
and a 520 × 299 px ellipse. A priming stroke is undone before each series of
three ten-second strokes with 200 Hz OS-injected stylus input.

Forward runs measure baseline then candidate; the reverse series measures the
same candidate then baseline APKs after an idle interval. Values below follow
stroke order within each series. Fresh updates consume new input and complete
inside the contact. Response ends at GPU completion, not presentation or
physical pen response.

| Series / build | Fresh updates/s | Completion-gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | --- | --- | --- | --- |
| Forward baseline | 102.036 / 100.770 / 102.750 | 30.412 / 30.046 / 28.093 | 62.387 / 61.744 / 63.357 | 12.356 / 12.757 / 11.897 |
| Forward candidate | 96.539 / 96.878 / 95.880 | 29.908 / 31.045 / 28.989 | 61.667 / 63.197 / 76.441 | 13.486 / 13.278 / 13.357 |
| Reverse baseline | 102.188 / 102.930 / 102.616 | 28.832 / 27.915 / 28.826 | 61.651 / 57.428 / 62.433 | 12.692 / 11.935 / 12.104 |
| Reverse candidate | 102.529 / 102.864 / 102.127 | 28.601 / 29.960 / 28.288 | 59.411 / 54.162 / 61.414 | 12.658 / 12.756 / 12.115 |

**Neither build meets the target.** All strokes are below 120 fresh updates/s,
its 95% floor of 114/s and the 16.7 ms gap limit. Indexed forward throughput
changes are −5.39 / −3.86 / −6.69%; callback CPU p95 grows 10.09 / 4.08 / 12.53%.
The reverse changes are +0.33 / −0.06 / −0.48% and −0.25 / +6.25 / +0.52%.
Reverse stroke 2 still exceeds the CPU comparison bound: callback p95 grows
0.769 ms, owner CPU p95 grows 6.87%, callback p99 grows 1.146 ms and the
completion-gap p99 grows 2.045 ms. Forward response p99 grows 1.453 and
13.084 ms in strokes 2 and 3; reverse response p99 improves in all three.
These grouped series do not resolve an overall comparison pass.

Visible tabs, workspace, settings, camera and fixture match. The forward
candidate starts warmer; the reverse candidate starts cooler than its later
baseline. Thermal status is zero at every endpoint, but clocks are unmeasured.
Source admission also changes with the run: baseline/candidate retain 750/764
decoded tiles forward and 762/748 reverse. Candidate accounted residency
changes by +14 MiB forward and −14 MiB reverse. These settled boundaries do
not measure continuous process or driver peaks.

In reverse stroke 2, paint p95 improves by 0.261 ms while queue-present p95
grows 1.177 ms. Each build's eleven largest callback records include ten in
the first second and one at the contact end; the ongoing second half also has
a callback p95 increase. No onset or tail sample is excluded. GPU execution
and the internal renderer phases are unmeasured, so this does not assign a
cause or qualify first-contact response. Other brush and canvas rows retain
their stated build scope; overall M3 qualification remains pending.

Exact APK/source identities and all forward/reverse records are retained in
`artifacts/format/m3-{baseline192-android-build-29,candidate192-android-build-32}/`
and `artifacts/format/m3-top-brush-35/`, including `order-comparison.json` and
`reverse-stroke1-tail-analysis.json`.

## BUILD20 G-Pen comparison

Measured on 2026-10-04 UTC with the clean `4a2cf6aa0` baseline and the M3
candidate based on the same revision. Both are benchmark APKs with release Rust.
The tier photo has one drawing layer above it, Paper hidden, Linear blending,
Navigator open, Stats closed and default glass. Pressure is 1 and prediction is
enabled. A priming stroke is undone before three warmed ten-second strokes with
200 Hz OS-injected stylus input. Both before/after thermal samples are zero.
The matched Fit zoom is 16.5341%, with screen semiaxes 520 × 299 px; the
nominal full brush tip remains at least 55.49 px inside the photo.

Rates count completed nonempty updates consuming new paint input inside the
contact, excluding refinement-only completions. Response is the latest consumed
input event to GPU completion; its p99 differs from the intercompletion gap.
Neither metric establishes physical pen latency or screen presentation.

| Build / stroke | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline 1 | 103.286 | 26.469 | 50.421 | 12.355 |
| Baseline 2 | 101.434 | 27.100 | 63.602 | 12.429 |
| Baseline 3 | 102.848 | 26.019 | 54.128 | 12.615 |
| Candidate 1 | 101.912 | 25.336 | 55.502 | 12.217 |
| Candidate 2 | 101.817 | 25.925 | 55.184 | 12.922 |
| Candidate 3 | 104.616 | 24.615 | 54.406 | 11.888 |

Both builds miss 120 fresh updates/s and the 16.7 ms gap limit. Candidate
throughput changes by −1.33 / +0.38 / +1.72%, and owner CPU p95 changes by
−1.12 / +3.97 / −5.76%, within the 5% growth bound. Response p99 changes by
+5.081 / −8.418 / +0.278 ms. The first stroke exceeds +1 ms and the second
improves; these samples do not resolve a common response pass or a repeatable
regression.

Accounted renderer residency is 2720.017 MiB candidate versus 2729.017 MiB
baseline. Source admission retains 771 versus 780 decoded slots. The logical
fixture matches, while runtime memory budgets and admitted cache sizes differ.

Measured resident boundaries stay within the additional max(16 MiB, 5%)
comparison ceiling. Allocator snapshots are taken after settling, not inside
motion; they do not establish continuous renderer/process/driver peaks or
edit/undo/output lifetime. GPU execution p95 is unmeasured. Logical workload
settings match; dynamic admission budgets are retained separately. One paired
batch does not establish the repeatability of response outliers or complete M3
qualification. Other brushes and earlier effect/transform rows retain their
stated scope.

Baseline APK SHA-256:
`2dd557379b1cac6ac51ce813f8a168a46cd9a460249846efe08a8e79b5801af2`.
Candidate APK SHA-256:
`83aa369e1b7cc2d9eb16eaf4c2dc1f0f40f574ddeaa0a2965317feb75c2ca01e`.
Exact app/test APK and source provenance is under
`artifacts/format/m3-{baseline,candidate}4a-android-build-20/`; raw strokes and
per-repetition analysis are under `artifacts/format/m3-final-android-performance-20/`.

## Layer color modes

Measured on 2026-10-04 at `de1159228`, release benchmark APK SHA-256
`fc0de7ee32ebbb9947803e461ca2bb436e07915019c2f82543cafcce8e5ea645`.
App sources match the implementation at `c92420837`.
The tier photo sits below one empty paint layer at Fit, with Perceptual blending,
pressure 1, 16 ms prediction, default display settings and panel glass, Stats
closed and thermal status 0 before and after each workload. Each mode uses one
priming gesture undone, followed by three five-second 200 Hz stylus ellipses.
The ellipse radii are 310 × 150 surface pixels at 16.53% Fit.

| Brush | Color mode | Canvas updates/s, median (range) | Fresh input updates/s, median (range) | Fresh completion-gap p99, range | Target |
| --- | --- | --- | --- | --- | --- |
| G-Pen 2048 px | Full color | 99.49 (99.39–102.36) | 99.49 (99.39–102.36) | 25.51–29.35 ms | **Not met** |
| G-Pen 2048 px | Grayscale | 99.87 (98.23–100.25) | 99.87 (98.23–100.25) | 19.50–20.50 ms | **Not met** |
| G-Pen 2048 px | Two-tone | 97.42 (96.69–97.63) | 97.42 (96.69–97.63) | 20.65–22.10 ms | **Not met** |
| Paintbrush 1024 px | Full color | 90.58 (90.07–90.78) | 90.58 (90.07–90.78) | 23.52–24.55 ms | **Not met** |
| Paintbrush 1024 px | Grayscale | 88.24 (88.02–90.18) | 88.24 (88.02–90.18) | 23.86–25.76 ms | **Not met** |
| Paintbrush 1024 px | Two-tone | 117.78 (117.59–118.27) | 117.59 (117.18–118.27) | 21.34–26.06 ms | **Not met** |
| Watercolor Wash 512 px | Full color | 3.99 (3.79–3.99) | 3.99 (3.79–3.99) | 575.35–609.82 ms | **Not met** |
| Watercolor Wash 512 px | Grayscale | 3.59 (3.40–4.19) | 3.59 (3.40–4.19) | 440.00–640.91 ms | **Not met** |
| Watercolor Wash 512 px | Two-tone | 3.99 (3.99–4.19) | 3.99 (3.99–4.19) | 528.27–600.27 ms | **Not met** |

These workloads miss the tier rate. Other brushes, layer-mode conversion latency,
and physical input-to-present latency remain unqualified. Raw runs, screenshots
and environment records are under `artifacts/layer-modes/top-rebased/`;
the binary and source snapshot are under `artifacts/layer-modes/rebased-build/`.

## Brushes

### Dry-media contact model

Measured on 2026-10-17 UTC with dry kernels specialized to normal blending and
zoomed-out dry relief previews evaluated at display resolution, applied to the
control `c674ee521`. The builds alternate for two passes each with the contained
45° profile described below; the Smoothing 100% row sets Curve fit to its
maximum, which lengthens the predicted tail. Optimized APK SHA-256
`1c8474b097ad06cff2f84aff7eee75be6025f09a739187344eddfc823c097a1f`, control
`4159a2cb7a267538ee3a0ac46d5bc8830686e04e70c07e69629e7b12a755f3ed`. Thermal
status was zero in every record. Ranges cover all six contacts; gaps are the
worst per-contact p99 between fresh completions.

| Brush and Size | Before completed updates/s | After completed updates/s | After fresh input updates/s | After worst fresh p99 gap | After input-to-GPU p99 | 120/s, 16.7 ms fresh profile |
| --- | --- | --- | --- | --- | --- | --- |
| Pencil 2048 px | 76.19–77.22 | 156.00–161.53 | 149.18–156.04 | 17.11 ms | 67.46–88.97 ms | **Not met** |
| Pointy Pencil 2048 px | 88.21–89.49 | 168.66–177.39 | 155.52–163.41 | 16.59 ms | 60.47–106.47 ms | Meets |
| Charcoal 2048 px | 73.35–74.50 | 158.98–161.91 | 152.55–155.85 | 16.19 ms | 63.69–102.37 ms | Meets |
| Pastel Block 1024 px | 192.57–195.62 | 231.67–262.38 | 178.24–191.31 | 14.44 ms | 36.40–47.24 ms | Meets |
| Pencil 2048 px, Smoothing 100% | 66.51–68.03 | 161.11–165.14 | 154.37–160.14 | 16.35 ms | 56.59–105.64 ms | Meets |

Pencil misses only the gap, by 0.41 ms; an earlier round of this change
measured 15.2 ms. First selection after clearing app data (`--probe-presets
2,25,6`, three runs) takes Pencil 0.99 → 0.78 s, Pointy Pencil 0.92 → 0.64 s and
Chalk 0.43 → 0.41 s; Chalk's background compile order varies between runs. Raw
runs are under `artifacts/dry-preview-speed/movinkpad14/`. The earlier
2026-10-17 rows below measure the previous build.

Measured on 2026-10-17 UTC with the one-kernel dry evaluator, two fine-tip
nodes from 4 px minor radius, Curve fit vertices bounded by 1% of the contact
radius and five-point Gauss–Legendre for short segments, applied to the control
`1300ba9f2`. The builds alternate for
two passes each with the profile described below, and ranges cover all six
contacts. Optimized APK SHA-256
`c8db6e1f2a909dea4f5d2b1266066d5c59bc6eb99c1ec9785adb4c24685c56c9`, control
`3a7340bdd1fdf0fc9d78f50c5cca810bd5c9226d9d066e6edb1abe5263439b4d`. Thermal
status was zero at every install. Gaps are the worst per-contact p99 between
fresh completions.

| Brush and Size | Before completed updates/s | After completed updates/s | After fresh input updates/s | After worst fresh p99 gap | After input-to-GPU p99 | 120/s, 16.7 ms fresh profile |
| --- | --- | --- | --- | --- | --- | --- |
| Pencil 2048 px | 61.55–62.89 | 72.51–74.56 | 72.51–74.56 | 23.61 ms | 131.81–161.27 ms | **Not met** |
| Pointy Pencil 2048 px | 83.16–84.20 | 95.86–97.40 | 95.86–97.40 | 18.55 ms | 104.18–131.03 ms | **Not met** |
| Charcoal 2048 px | 59.76–60.90 | 70.10–71.43 | 70.10–71.43 | 24.98 ms | 140.82–172.66 ms | **Not met** |
| Pastel Block 1024 px | 216.60–220.28 | 201.89–203.42 | 174.54–178.28 | 13.74 ms | 58.16–78.32 ms | Meets measured writing pose |

Pastel Block's fresh rate falls from 179.80–184.81 to 174.54–178.28/s; its
rounded tip uses neither the fewer fine-tip nodes nor the short-segment
quadrature, so it keeps only the one-kernel cost. Selecting a brush after
clearing app data (`--probe-presets 2,25,6`, three runs) takes Pencil 4.51 →
1.05 s, Pointy Pencil 4.00 → 0.93 s and Chalk 1.94 → 0.46 s; a 36 px size change
then takes 1.95 s → 41 ms. Raw runs are under
`artifacts/dry-kernel-merge/movinkpad14/`, beside the summary script and the
benchmark wrapper that passes tilt and opacity. The 2026-10-13 rows below
measure the earlier build.

Measured on 2026-10-13 (UTC), with the final dry defaults, unified contact loop
and compile-time aggregate specialization based on `cbc74b304`.
Frozen source SHA-256 `04adb5a414d5fee9148980b56487e9d211e1dcb7b497185ee8faab955f287eda`;
optimized APK `66f517b7c867a83caee591780226ff36591bf246bb3b3fd6ba04ccd7239d6020`,
JNI `7de0e1d01ae832bcb4bd91b0ed7157052f24d3668439edbdc299c50d54c7e49b`.
The production portions of the three renderer files restored for aggregate
specialization match production; later differences are test assertions and trailing
WGSL whitespace. The matched dynamic-aggregate
comparison uses APK `a144b7dc328d9861481901cb650977670bf9f02c7737ca5689298ed06165b36a`;
it has the same contact equations, current defaults and Curve fit input code.

Both use the tier photo at Fit, one empty Full color paint layer above it,
Navigator open, Stats closed and 16 ms prediction. Each warmed profile has three
six-second 200 Hz stylus contacts at physical pressure one, 45° tilt and a
contained 60 × 10 surface-pixel ellipse. All before/after thermal records are zero.
Size is the full-pressure long axis at the writing pose. Strength, Opacity and
Texture strength are 100%; Material hardness is Pencil 90%, Pointy 95%, Charcoal
55% and Pastel 50%. Smoothing retains its default 6%. Pressure balance is absent.
The calibrated Pencil and Pointy pressure gains are 0.7.

| Brush and Size | Dynamic aggregate fresh updates/s | Current completed canvas updates/s | Current fresh input updates/s | Current worst fresh p99 gap | Current input-to-GPU p99 | 120/s, 16.7 ms fresh profile |
| --- | --- | --- | --- | --- | --- | --- |
| Pencil 2048 px | 39.61–40.28 | 61.57–61.93 | 61.57–61.93 | 23.49 ms | 141.75–163.47 ms | **Not met** |
| Pointy Pencil 2048 px | 55.07–56.37 | 82.02–82.89 | 82.02–82.89 | 24.23 ms | 126.67–159.32 ms | **Not met** |
| Charcoal 2048 px | 38.75–39.11 | 60.21–60.88 | 60.21–60.88 | 27.45 ms | 164.98–168.93 ms | **Not met** |
| Pastel Block 1024 px | 151.21–151.57 | 207.02–209.83 | 173.74–175.88 | 13.36 ms | 73.31–86.63 ms | Meets measured writing pose |

Fresh input throughput counts the first GPU completion consuming a new input;
completed canvas throughput also includes prediction/refinement completions.
Input-to-GPU observations include completion polling and do not measure physical
scanout. Compile-time aggregate specialization improves these matched profiles,
but only Pastel meets the fresh profile; the three Simple brushes miss rate and gap.
Earlier `611578d3` and `fcb19ecf` results use different defaults and remain
historical comparisons, not qualification of this build.

A separate same-APK Pencil 2048 px diagnostic changes only native Smoothing
to 0% before three measured contacts. Every post-contact state verifies zero;
the default 6% setup snapshot remains preserved. It completes 83.51–86.23 fresh
updates/s with worst fresh gap 18.41 ms and input-to-GPU p99
118.68–131.76 ms. This isolates a substantial Curve fit cost but
still misses the nominal rate and gap; the qualification rows keep 6%.

Brief visual-only replays on the dynamic-aggregate comparison build cover
Shading Pencil Size 24.615 px, Chalk 19.870 px and Crayon 12.308 px at 45°,
Strength 100% and actual physical pressure 0.1–1.0. Each has one two-second
contact and a retained screenshot; these do not qualify nominal tier sizes or
sustained frame rates. Other tilts and partial Opacity remain unmeasured.
Raw current profiles, matched controls, diagnostic captures and source/binary
proof are under `artifacts/dry-defaults-final/midtop/`.

### Paper

Measured on 2026-10-14 (UTC) on movinkpad14, against a matched control built
at `7d6dd662e`, the commit before Paper (APK
`e5668201831fe4585f291c52ae69af76a951b5e4f69716c095ff767ac6a09955`). The Paper
build is the commit that adds this section (APK
`22eca42a44bef031f90cd9f9cbbdc9962d6f71ae3f160e3cb4c0100a1aa64371`, JNI
`307ee52df5adb4c14e8c6fbc53c173ef80718a682cd88c365d306ac7c3650b68`).

Each run imports the tier photo, so strokes use the default Drawing paper, and
paints into one empty layer above it at Fit with Stats closed and 16 ms
prediction. Each warmed profile has three six-second 200 Hz contacts at
pressure one along a 60 × 10 surface-pixel ellipse with the preset's own tilt
and settings; this is not the 45° writing pose above. Five control runs and two
Paper runs alternate; thermal status is zero before and after every run.

| Brush and Size | Control completed updates/s | Paper completed updates/s | Paper / control | Control gap p99 | Paper gap p99 |
| --- | --- | --- | --- | --- | --- |
| Pencil 2048 px | 93.46–94.65 | 96.01–96.38 | 1.022 | 16.67 ms | 15.99 ms |
| Charcoal 2048 px | 90.18–91.97 | 90.23–91.01 | 0.992 | 17.79 ms | 18.02 ms |
| Pastel Block 1024 px | 294.90–300.02 | 291.61–301.87 | 1.001 | 11.21 ms | 11.06 ms |
| Paintbrush 1024 px | 126.82–127.86 | 127.21–128.51 | 1.005 | 15.35 ms | 15.48 ms |

Each value is the median of a run's three contacts; ranges span runs, ratios
compare run means and gaps are mean run p99s. Pastel Block and Paintbrush have
four control runs. Control runs differ by up to 2.0%, so no row regresses.

Dry-media cost follows tooth contrast rather than the Paper code path. Drawing
paper with 20% more contrast measured Pencil 0.983 and Charcoal 0.976 of the
same control, and with 1.9 times the contrast 0.94 for both; caching the
paper binding per surface changed neither. Drawing paper therefore keeps the
earlier paper's contrast. The mid tier was not measured because its tablet was
behind a secure lock screen, and tablet bake times are not measured. Raw runs
are under `artifacts/paper-perf/top/`.

### Retained-photo brush measurements

Target: **120 completed updates/s** at the guaranteed size, on the 61 MP canvas.

Except for G-Pen, Watercolor Wash and the current dry-media rows, measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets. The ellipse is 520 × 299 px at 16.5% zoom.

Multiply Glaze and the very complex material rows were measured on 2026-10-19 with the same profile, using a release benchmark build of `66c1142ee`, which runs every material brush as compute kernels; their gap is the completion-gap p99.

Smudge, Natural Blender, Soft, Blur, Bristle, Grainy, the Blending Knife, Blending Stump and Tissue were measured on 2026-10-20 at `68012f20a`, after [paper-aware blending](../history/brushes/paper-aware-blenders.md), with the 9504 × 6336 photo: six runs from two candidate sets interleaved with a control of the previous `main` (three runs for the new presets). Every run of the rows marked met completed at least 160 updates/s with a gap p99 of at most 12.7 ms. The control completed Smudge 143.4, Natural Blender 85.0, Soft 142.0, Blur 178.3, Bristle 80.6, Grainy 122.9 and Blending Knife 10.0 updates/s. The other blenders (presets 49–55 and 57–62) are unmeasured on this tier, and the wet rows predate the blender suite. Liquify uses the separate Photo-layer profile below.

Marker, Paintbrush, Textured Flat, Dry Scumble and Transparent Glaze were measured on 2026-10-21 after Uniform contacts stopped counting the pose they share twice ([paint state](../reference/painterly-paint-state.md)): six strokes from two candidate sets interleaved with a control of the previous `main` (`c12ec63d2`), which completed Marker 120.88, Paintbrush 113.30, Textured Flat 128.77, Dry Scumble 59.58 and Transparent Glaze 59.87 updates/s. The control's strokes sped up after a lap or two, once the doubled coats brought the ellipse to full coverage and its pixels skipped their contacts. Each pass now lays one coat, so a pixel the ellipse crosses again costs as much as on its first pass.

**The 2026-09-22 G-Pen record used a different stroke.** It recorded 105.76
updates/s for a 2000 px G-Pen.
That run's wider work area set Fit zoom to 18.0%, so each stroke crossed about 8%
less canvas at the same screen speed.

A separate 2026-09-27 release run changed the quick color before each of three
5 s, 2048 px G-Pen strokes on the same 61 MP photo. Brush readiness stayed true.
The strokes completed 92.9–95.3 updates/s, with update-start gap p99 of
28.0–31.7 ms. This does not meet the 120 updates/s target; its shorter strokes
are kept separate from the 10 s comparison table below.

| Brush (id) | Class | Size | Measured | Status |
| --- | --- | --- | --- | --- |
| G-Pen (1) | Simple | 2048 px | 119.21 (118.52–123.78) fresh input-attributed updates/s; completion-gap p99 15.09–16.11 ms | **Not met**: two rates below 120/s; [current controls](#brush-and-pending-filter-controls) |
| G-Pen (1), brush controls | Simple | 2048 px | 175.65–179.08 fresh updates/s; fresh gap p99 ≤13.23 ms, contained 60 × 10 path | Meets measured profile; [earlier controls](#dry-media-contact-model) |
| Rough G-Pen (28) | Simple | 2048 px | 61.8 updates/s (60.6–62.7); gap p99 33.6 ms | **Not met** |
| Calligraphy Pen (29) | Simple | 2048 px | 189.3 updates/s (188.6–190.5); gap p99 10.6 ms | Met |
| Antique Pen (30) | Simple | 2048 px | 82.0 updates/s (81.7–82.2); gap p99 26.4 ms | **Not met** |
| Realistic Pen (31) | Simple | 2048 px | 68.5 updates/s (68.0–68.8); gap p99 31.3 ms | **Not met** |
| Wet Ink (32) | Simple | 2048 px | 54.6 updates/s (54.4–54.8); gap p99 36.1 ms | **Not met** |
| Pencil (2) | Simple | 2048 px | 61.57–61.93 fresh updates/s; fresh gap p99 ≤23.49 ms | **Not met**; [dry media](#dry-media-contact-model) |
| Pointy Pencil (25) | Simple | 2048 px | 82.02–82.89 fresh updates/s; fresh gap p99 ≤24.23 ms | **Not met**; [dry media](#dry-media-contact-model) |
| Shading Pencil (26) | Simple | 2048 px | Current contact model unmeasured | Unqualified; [dry media](#dry-media-contact-model) |
| Charcoal (27) | Simple | 2048 px | 60.21–60.88 fresh updates/s; fresh gap p99 ≤27.45 ms | **Not met**; [dry media](#dry-media-contact-model) |
| Chalk (6) | Simple | 2048 px | Current contact model unmeasured | Unqualified; [dry media](#dry-media-contact-model) |
| Crayon (43) | Simple | 2048 px | Current contact model unmeasured | Unqualified; [dry media](#dry-media-contact-model) |
| Eraser (3) | Simple | 2048 px | 49.7 updates/s (49.6–49.9); gap p99 46.0 ms | **Not met** |
| Airbrush (5) | Simple | 2048 px | 59.7 updates/s (59.5–60.6); gap p99 30.9 ms | **Not met** |
| Marker (7) | Complex | 1024 px | 117.67 updates/s (116.85–121.35); gap p99 16.1 ms | **Not met** |
| Blotty Ink (33) | Complex | 1024 px | 117.8 updates/s (117.1–118.4); gap p99 17.0 ms | **Not met** |
| Realistic Brushed Ink (34) | Complex | 1024 px | 112.5 updates/s (111.8–112.7); gap p99 18.3 ms | **Not met** |
| Pastel Block (17) | Complex | 1024 px | 173.74–175.88 fresh updates/s; fresh gap p99 ≤13.36 ms | Meets measured writing pose; [dry media](#dry-media-contact-model) |
| Paintbrush (4) | Complex | 1024 px | 72.08 updates/s (71.15–72.66); completion-gap p99 26.1 ms | **Not met**; [layer color modes](#layer-color-modes) |
| Textured Flat (15) | Complex | 1024 px | 80.99 updates/s (79.66–82.13); gap p99 22.0 ms | **Not met** |
| Dry Scumble (16) | Complex | 1024 px | 51.74 updates/s (51.44–52.22); gap p99 31.6 ms | **Not met** |
| Transparent Glaze (18) | Complex | 1024 px | 63.44 updates/s (62.82–63.80); gap p99 23.6 ms | **Not met** |
| Multiply Glaze (14) | Complex | 1024 px | 86.8 updates/s (86.0–87.1); input 86.8/s; completion-gap p99 19.1 ms | **Not met** |
| Dual Texture (9) | Complex | 1024 px | 80.0 updates/s (79.5–80.6); gap p99 21.3 ms | **Not met** |
| Spray (8) | Complex | 1024 px | 33.8 updates/s (32.7–34.3); gap p99 113.2 ms | **Not met** |
| Opaque Gouache (19) | Very complex | 512 px | 10.4 updates/s (10.3–10.7); input 10.4/s; completion-gap p99 115.4 ms | **Not met** |
| Watercolor Wash (20) | Very complex | 512 px | 23.8 updates/s (23.7–24.0); input 23.8/s; completion-gap p99 65.8 ms | **Not met**; [layer color modes](#layer-color-modes) |
| Wet Watercolor (21) | Very complex | 512 px | 24.5 updates/s (24.0–24.7); input 24.5/s; completion-gap p99 63.4 ms | **Not met** |
| Loaded Oil (22) | Very complex | 512 px | 7.7 updates/s (7.6–8.0); input 7.7/s; completion-gap p99 157.3 ms | **Not met** |
| Palette Knife (23) | Very complex | 512 px | 11.1 updates/s (11.0–11.2); input 11.1/s; completion-gap p99 110.1 ms | **Not met** |
| Wet Round (11) | Very complex | 512 px | 33.1 updates/s (32.8–33.6); input 33.1/s; completion-gap p99 54.1 ms | **Not met** |
| Natural Blender (24) | Very complex | 512 px | 108.8 updates/s (106.1–109.5); input 108.8/s; completion-gap p99 15.4 ms | **Not met** |
| Smudge (10) | Very complex | 512 px | 177.0 updates/s (176.0–180.7); input 165.5/s; completion-gap p99 10.4 ms | **Met** |
| Soft Blender (45) | Very complex | 512 px | 176.6 updates/s (173.5–179.0); input 166.2/s; completion-gap p99 10.4 ms | **Met** |
| Blur (46) | Very complex | 512 px | 187.3 updates/s (185.7–191.5); input 173.4/s; completion-gap p99 9.7 ms | **Met** |
| Bristle Blender (47) | Very complex | 512 px | 95.1 updates/s (94.3–95.9); input 95.1/s; completion-gap p99 17.8 ms | **Not met** |
| Grainy Blender (48) | Very complex | 512 px | 163.1 updates/s (160.1–164.4); input 156.3/s; completion-gap p99 11.7 ms | **Met** |
| Blending Knife (56) | Very complex | 512 px | 9.4 updates/s (9.4–9.7); input 9.4/s; completion-gap p99 132.7 ms | **Not met** |
| Blending Stump (65) | Very complex | 512 px | 177.7 updates/s (176.4–178.1); input 167.7/s; completion-gap p99 10.4 ms | **Met** |
| Tissue (66) | Very complex | 512 px | 171.0 updates/s (170.8–174.1); input 162.3/s; completion-gap p99 10.7 ms | **Met** |
| Liquify Push (12) | Very complex | 512 px | 172.79 fresh updates/s (153.97–173.94); fresh completion-gap p99 ≤29.53 ms | **Gap not met** |
| Liquify Twirl Clockwise (36) | Very complex | 512 px | 134.58 fresh updates/s (132.49–148.14); fresh completion-gap p99 ≤30.61 ms | **Gap not met** |
| Liquify Twirl Counterclockwise (13) | Very complex | 512 px | 143.98 fresh updates/s (132.92–154.05); fresh completion-gap p99 ≤31.46 ms | **Gap not met** |
| Liquify Pinch (37) | Very complex | 512 px | 162.04 fresh updates/s (151.53–168.59); fresh completion-gap p99 ≤25.05 ms | **Gap not met** |
| Liquify Expand (38) | Very complex | 512 px | 162.66 fresh updates/s (154.26–164.86); fresh completion-gap p99 ≤22.41 ms | **Gap not met** |
| Liquify Crystals (39) | Very complex | 512 px | 154.61 fresh updates/s (152.92–154.97); fresh completion-gap p99 ≤22.53 ms | **Gap not met** |
| Liquify Edge (63) | Very complex | 512 px | 155.55 fresh updates/s (146.84–159.02); fresh completion-gap p99 ≤28.42 ms | **Gap not met** |
| Liquify Reconstruct (64) | Very complex | 512 px | 172.73 fresh updates/s (171.20–179.56); fresh completion-gap p99 ≤16.54 ms | **Completion limits met** |

### Blenders on populated photographic paint

Measured on 2026-10-21 on the Wacom MovinkPad Pro 14, with the blender correctness
changes on `e16049138`: `material_blend.wgsl` SHA-256 starts `d4aab206a8b2`,
`material_transport.wgsl` starts `0477a69071a1`, and the optimized benchmark APK
starts `a3025b3a7a43`. Rust 1.99.1 uses the release profile and R8 optimization.
These measurements select the populated `Photo` paint layer itself at 100%
opacity, with hidden Paper, rather than an empty paint layer above it. They do
not establish the ordinary empty-layer guarantee or a comparison with older rows.

The 9504 × 6336 sRGB8 Sony photo uses Perceptual blending, Fit zoom 0.23828125,
the default Sketch workspace and glass, Stats closed, and no tracing. Each
512 px preset keeps its default settings, including Grainy's paper strength 1,
with 16 ms prediction and a contained 310 × 150 px ellipse at pressure 1 and
tilt 0. Pipelines are warm and a priming stroke is undone before three 10 s
OS-injected 200 Hz strokes. Thermal status is 0 before and after each preset.

| Preset | Completed updates/s, median (range) | Fresh input updates/s, median (range) | Completion gap p99 range | Fresh input gap p99 range | 120/s and 16.7 ms limits |
| --- | --- | --- | --- | --- | --- |
| Grainy Blender (48) | 198.090 (197.052–198.955) | 174.880 (173.316–177.770) | 10.524–11.224 ms | 13.231–13.533 ms | **Met for this workload** |
| Color Mixer (58) | 110.450 (110.319–110.884) | 109.751 (109.320–109.985) | 17.884–19.599 ms | 17.884–20.292 ms | **Not met** |
| Blending Knife (56) | 27.557 (27.274–27.868) | 27.557 (27.274–27.868) | 47.740–62.078 ms | 47.740–62.078 ms | **Not met** |

Raw inputs, completion records, setup state, screenshots, source hashes and the
measured APK are retained in `artifacts/blender-audit-current/tablets/top/` and
its parent. These are GPU-completed canvas updates, not scanout measurements.
The photographic paint is opaque; transparent-edge and HDR correctness are
separate renderer fixtures.

### Retained-map Liquify

Measured on 2026-10-20 on the Wacom MovinkPad Pro 14, Adreno 735, with the
retained-map implementation based on `b7fb3654a`. Benchmark APK SHA-256:
`7a5a27c61b93890c744413a0f31a4d6c54c074377884af3ac8e7c3bb2f115c05`.
The eight rows above use the 9504 × 6336 photo, Size 512, full pressure,
16 ms prediction and three five-second contacts after shader warm-up.
`--paint-layer-name Photo` selects the photo itself. Reconstruct first seeds
its map with a five-second Push stroke; each measured contact is undone before
the next. Every mode changes visible canvas pixels.

The 300 × 180 screen-pixel semiaxes run at Fit zoom 0.23828125 on the
2880 × 1800 surface. Sketch is collapsed and Stats is closed; thermal status
is zero. A post-run display dump verifies the active 120 Hz mode; hardware
refresh was not captured per trial. Values are median completed updates consuming
fresh input per second with the three-run range, and the worst run's fresh completion-gap p99.
Completed nonempty median rates span 138.08–191.97/s across modes, while
fresh-input medians span 134.58–172.79/s. They do not measure physical scanout or
pen-to-display latency. Every mode reaches the median rate of 120/s;
Reconstruct also meets the 16.7 ms gap limit. The other seven modes miss that
limit. The previous empty-layer measurements do not qualify deformation of
photo pixels.

The diagnostics and broad-coverage check below use APK
`933d671b62210326655307c6a85e7601c51ba5f670e073757c7a047e6aee79fe`,
with the same Push map arithmetic, twelve-source chunks and allocation rules.
A separate Stats-enabled Push diagnostic measures median GPU time of 3.19 ms:
map updates 0.95 ms, reconstruction 1.82 ms, main composition 0.239 ms and
display mips 0.066 ms. CPU callback running time is 5.84 ms, including 1.13 ms
in queue submission. Nested phases must not be summed. GPU deformation alone
does not explain the remaining completion gaps on this device.

Guarded broad-coverage Push completes all thirteen full-width stripes with a
1024 MiB available-memory floor and 512 MiB additional pre-stripe allowance.
Tracked GPU allocation grows from 2640 to 3655 MiB, peaking at 4059 MiB before
history pruning. MemAvailable stays above 2348 MiB. The geometric support upper
bound reaches all 950 tile cells; it does not count unique deformed pixels.
There is no GPU loss or out-of-memory failure. Undo takes 447 ms and continued
moving Push 1649 ms after the historical map is evicted. Undoing coverage and
trimming redo reduces tracked GPU allocation to 2472 MiB. One coverage stripe
takes 1368 ms to settle after pen-up; the others take 439–786 ms, including
momentum and publication.

## Windows Surface diagnostic

Measured on 2026-10-06 on a Surface Laptop 5, i7-1255U, Intel Iris Xe D3D12,
driver 32.0.101.6737, 2256 × 1504 at 60 Hz and 150% scale. AC stayed online,
battery 76%, Balanced power plan. Runs were isolated from builds and other GPU
work. This is a Windows regression comparison, not reference-tier qualification.

Baseline source is `c5624d37`; the candidate is `8b1bfca6` plus the Windows scope
and menu port. Native executable SHA-256 prefixes are `2CD17495CB35` and
`396601C0A6D0`; brush executable prefixes are `ED3EA3416EEE` and `98B66128E6CE`.
Full hashes, hardware records, raw CSVs, traces and per-run p50/p95/p99 values
are under `artifacts/windows/port-audit/performance-matched/` and the adjacent
`performance-hardware.json`.

The brush example uses 1000 px brushes on an empty 9504 × 6336 document, a
1600 × 1000 managed offscreen presentation, and a priming stroke followed by
Undo. Each invocation has three 240-frame repetitions. The table counts the
239 moving frames per repetition; pen-up is separate in the CSV. Rates are
completed offscreen generations/s, not displayed fps. Every run passed exact
native Undo/Redo.

| Workload | Baseline median (range), /s | Candidate median (range), /s |
| --- | ---: | ---: |
| G-Pen | 83.38 (81.23–86.21) | 83.21 (80.40–84.08) |
| Eraser, candidate then baseline | 87.87 (82.16–92.25) | 87.97 (87.67–90.04) |
| Airbrush, baseline then candidate | 90.73 (81.73–97.10) | 87.37 (81.74–95.06) |
| Airbrush, candidate then baseline | 90.92 (80.15–92.05) | 87.32 (84.28–90.91) |
| Airbrush, same executable path, ABBA, six repetitions per build | 85.68 (80.88–98.42) | 85.88 (79.23–97.85) |

The first two Airbrush comparisons showed a 3.7–4.0% lower candidate median.
The benchmark executables have identical code and runtime data; only nine bytes
of PE/debug timestamps and PDB age differ. A same-path baseline/candidate/
candidate/baseline check did not reproduce that gap. Median per-repetition
completion p95 was 16.64 → 16.31 ms and p99 was 18.70 → 17.71 ms across six
repetitions per build. The live process loaded the bundled DXC, SHA-256 prefix
`9A5100511E12`. No Airbrush slowdown is established; the earlier observations
remain in `brush/` and `brush-reverse-airbrush/`, with the controlled check in
`airbrush-controlled/` and executable comparison in `pe-comparison.json`.

Pen runs use the same plain 8192 × 6336 drawing, G-Pen 18 px, and three
10-second OS-injected strokes per app in alternating baseline/candidate order.
The harness waits for the requested drawing, enabled canvas and three seconds
of settling; it does not prime a stroke. Every run consumed 2401/2401 samples,
with zero unmatched inputs. Project SHA-256 prefix is `5CDB97C6E4CB`.
Values below are medians of the three per-run percentiles, with full run ranges,
in milliseconds.

| Metric | Baseline p95 | Candidate p95 | Baseline p99 | Candidate p99 |
| --- | ---: | ---: | ---: | ---: |
| Injection to frame return | 5.26 (5.05–7.65) | 5.08 (4.74–5.38) | 61.64 (52.95–81.84) | 64.88 (56.90–74.53) |
| Injection to presentation observation bound | 10.87 (10.64–11.51) | 10.83 (10.74–10.88) | 67.88 (66.86–93.56) | 69.87 (63.49–79.38) |
| Host frame span | 3.65 (3.57–4.10) | 3.54 (3.42–3.56) | 4.72 (4.53–5.43) | 4.43 (4.26–4.55) |

Large p99 tails remain on both builds. Immediate presentation makes the DXGI
refresh timestamp inapplicable, so exact input-to-display latency remains null;
the observation metric is an upper bound including intervening host work.
PresentMon was unavailable without capture privileges and was skipped. These
runs exclude physical digitizer and panel response and use neither the reference
photo nor the reference tablet. They do not qualify a tier rate or latency target.

### Windows image port comparison, 2026-10-08

On the same Surface Laptop 5, D3D12 driver and display configuration,
`9f9342d9` is compared with `8f7a9c77` plus the Windows image and notice port.
AC remained online with the Balanced plan. No build or other owned GPU workload
ran during these measurements. Existing user Chrome processes remained open:
endpoint samples showed zero CPU/GPU activity, but they accumulated 0.766 CPU
seconds during the focused Airbrush comparison and an extension renderer started.
Thermal status was unavailable. These are diagnostic comparisons, not reference
hardware or tier-target qualification.

The brush workload and counting rules are the same as above. An ABBA sequence
has six repetitions per preset and build. Every repetition passed exact Undo/Redo.

| Workload | Baseline median (range), /s | Candidate median (range), /s | Median run completion p99, before -> after (ms) |
| --- | ---: | ---: | ---: |
| G-Pen | 89.73 (85.61-93.99) | 89.30 (86.56-96.00) | 20.53 -> 19.91 |
| Eraser | 89.22 (79.76-96.67) | 89.61 (87.43-96.11) | 17.11 -> 17.98 |
| Airbrush | 93.06 (89.61-97.02) | 88.95 (84.16-93.78) | 16.37 -> 17.85 |

The mixed-preset Airbrush median falls 4.42%. A separate Airbrush-only ABBA
comparison, using the same executable path with verified source/copy hashes,
also falls: 94.65 (86.86-98.91) -> 89.19 (85.44-98.19) completed generations/s,
a 5.77% reduction. Median run completion p99 is 16.46 -> 17.82 ms; all twelve
repetitions pass exact Undo/Redo. The slowdown remains unresolved. These results
do not separate upstream changes after `9f9342d9` from the Windows port.

A separate comparison isolates the local patch at the same `8f7a9c77` revision.
A clean Release build uses unchanged source and ordinary Release settings. The
same executable path and workload run in patched/clean/clean/patched order,
with six repetitions per build; all twelve pass exact Undo/Redo.

| Same-revision Airbrush | Clean main | Patched main |
| --- | ---: | ---: |
| Completed generations/s, median (range) | 90.75 (81.96-101.96) | 92.28 (86.64-99.17) |
| Median run completion p99, ms (range) | 17.36 (14.80-19.52) | 16.56 (15.27-19.31) |

This sequence does not reproduce an added slowdown from the local patch. It does
not establish statistical equivalence or explain the earlier revision-to-revision
gap. AC/Balanced and all eleven user Chrome PIDs remain unchanged; Chrome uses
0.766 CPU seconds over this window, with zero sampled GPU activity at its ends.
Intermittent activity remains unexcluded. Clean executable SHA-256 prefix is
`3DFE498906F5`; its source manifest and build proof are in
`artifacts/windows/port-20261007/performance/clean8f-release/`, and this comparison
is in `airbrush-bccb/` below the raw evidence directory named below.

Pen captures use the same plain 8192 x 6336 project and 18 px G-Pen, with an
enabled brush and three seconds of settling, without priming or waiting for the
full optional shader catalogue. Order is ABBABAAB, four fresh profiles per build.
All eight runs consume 2401/2401 samples with zero unmatched inputs. Nominal
240 Hz pacing achieves about 212-216 Hz. Values below are medians and ranges of
the four per-run percentiles in milliseconds.

| Metric | Baseline p95 | Candidate p95 | Baseline p99 | Candidate p99 |
| --- | ---: | ---: | ---: | ---: |
| Injection to frame return | 5.88 (5.77-5.91) | 5.62 (5.48-6.61) | 72.27 (57.59-98.23) | 58.79 (54.24-62.34) |
| Injection to presentation observation bound | 10.90 (10.81-11.07) | 11.06 (11.03-11.39) | 80.05 (69.53-106.94) | 67.21 (62.41-72.15) |
| Host frame span | 3.73 (3.68-3.79) | 3.74 (3.66-3.88) | 4.86 (4.75-5.04) | 4.69 (4.59-4.85) |

Two setup attempts failed before injection when rebuilt brush controls were
queried before becoming available. Five accepted captures use the original
lookup; the final three use the existing control-readiness wait and reacquire
the size control to verify its committed value. Input packets, pacing, settling
and reporting are unchanged. Both cohorts and failures remain in the evidence.
Immediate presentation provides an observation upper bound; exact display
latency is unavailable. PresentMon lacked capture privileges. These runs exclude
physical digitizer and panel response.

The initial measurements for this image port had two task-owned hidden review
apps still running; their isolation claim is unsupported. Those processes were identified
from their original launch logs and stopped before this comparison. Full hashes,
raw CSVs, activity records, percentile ranges and setup provenance are in
`artifacts/windows/port-20261007/performance/candidate/quiet-comparison-20261008T135944Z/`.
Native candidate executable SHA-256 prefix is `6CB0E28F0FEA`, DLL `04468C30D911`,
and brush executable `3F5189244C13`. The preserved pen project is
`FF686BDF3CB3`.

### Windows controls comparison, 2026-10-09

The same Surface Laptop 5, i7-1255U and Intel Iris Xe D3D12 driver
`32.0.101.6737` compare preserved `8f7a9c77` binaries with `0f2ab217` plus the
Windows controls and shared filesystem changes. The display is 2256 × 1504 at
60 Hz; AC and Balanced remain unchanged. No other owned app, build or GPU
workload ran. Eleven existing user Chrome processes remained open, accumulating
0.750 CPU seconds during brush ABBA and 0.422 during pen ABBA. Endpoint GPU
samples were zero; intermittent background activity and thermal effects remain
unexcluded. These are Surface regression diagnostics, not reference-tier results.

Brush ABBA uses the same executable path, 9504 × 6336 canvas, 1000 px brushes,
1600 × 1000 offscreen surface and three 240-frame repetitions per preset per
block. Only the 239 moving frames per repetition count. All four blocks and
their exact Undo/Redo checks passed, giving six repetitions per build and preset.

| Workload | Baseline completed generations/s, median (range) | Current completed generations/s, median (range) | Median run completion p99, before → after (ms) |
| --- | ---: | ---: | ---: |
| G-Pen | 90.87 (86.01–96.61) | 87.91 (74.03–93.28) | 19.94 → 20.73 |
| Eraser | 88.20 (84.31–89.98) | 87.77 (80.23–91.40) | 18.00 → 18.14 |
| Airbrush | 92.45 (86.46–96.46) | 89.86 (83.49–93.33) | 17.19 → 17.57 |

Median rates fall 3.26%, 0.49% and 2.80%, respectively. This comparison spans
upstream revisions and cannot attribute the decreases to the local port.
The same-revision comparison below records the remaining decreases.
Completed generations are not displayed frames.

Pen ABBA uses the unchanged plain 8192 × 6336 project, 18 px G-Pen, original
enabled-brush readiness and three-second settling. All four fresh-profile runs
consume 2401/2401 inputs with zero unmatched samples. Nominal 240 Hz injection
achieves 215.72–216.54 Hz. The surface remains 1418 × 988, density 1.5,
`Rgba16Float`, Immediate and maximum frame latency 1. Each entry below is the
median and range of the two per-run percentiles, in milliseconds.

| Metric | Baseline p95 | Current p95 | Baseline p99 | Current p99 |
| --- | ---: | ---: | ---: | ---: |
| Injection to frame return | 6.49 (5.75–7.22) | 6.00 (5.90–6.10) | 99.69 (67.20–132.17) | 60.72 (60.03–61.41) |
| Injection to presentation observation bound | 10.93 (10.74–11.12) | 11.15 (10.93–11.36) | 103.29 (78.72–127.87) | 66.22 (65.02–67.42) |
| Newest input to presentation observation bound | 10.66 (10.65–10.67) | 10.81 (10.71–10.92) | 12.87 (12.72–13.02) | 13.40 (13.32–13.48) |
| Host frame span | 3.83 (3.75–3.91) | 3.84 (3.77–3.91) | 4.92 (4.68–5.17) | 4.68 (4.65–4.71) |

Newest-input observation ranges increase without overlap in this small sample;
the same-revision native comparison below provides a further check. All original tails remain.
Immediate provides an observation bound, not a true presentation timestamp or
physical digitizer/scanout/panel latency. PresentMon capture privileges were
unavailable. The ordinary paint timing path records CSV evidence without composed
screenshots; it does not replace the separate functional painting journeys.

Supplementary Hand, Zoom and Rotate view measurements subsequently completed
on the same current binary and plain project, in a maximized 2256 × 1432 surface
at density 1.5. Each mode uses one nominal one-second priming contact, a view
reset, then three nominal six-second contacts with 200 × 120 px radii. Actual
measured contacts last 6.738–6.896 seconds at 208.80–213.70 input Hz. Each mode
consumes 4564/4564 injected inputs with zero unmatched samples. UI tracing stays
disabled; selected tool, clean title and disabled Undo/Redo are checked before
and after every contact. All 24 composed before/after images were reviewed.

| Navigation mode | Inferred renderer submissions/s | Contact rate range | Submission interval p50 / p95 / p99 / max (ms) |
| --- | ---: | ---: | ---: |
| Hand | 209.73 | 209.08–210.15 | 4.66 / 5.36 / 6.09 / 29.11 |
| Zoom | 199.12 | 198.72–199.68 | 4.67 / 6.87 / 14.17 / 29.22 |
| Rotate view | 206.97 | 206.16–207.97 | 4.65 / 5.77 / 8.57 / 29.13 |

These rates count distinct submitted present IDs in frames consuming net-distinct
navigation input, keeping intervening gaps within each contact. They are not
presented frame rates. Exact camera translation, work-area geometry and rotation
angle are unavailable to this trace-disabled measurement; the native rounded
readout and composed captures supply only before/after observations. There is
no paired navigation baseline or target-met claim. The maximized navigation
viewport does not change the ordinary pen ABBA viewport above.

Earlier ownership/setup failures remain preserved. The successful Hand capture
initially encountered a coordinator JSON-reader error on an empty native UIA ID;
parsing it as a hashtable qualified the saved evidence without repeating Hand,
then the two unrun modes completed. No timing or report assertion changed.
Fresh Object preflight on the corrected numeric-control build passed at
9504 × 6336, including saved artwork, camera and all 256 initial pixel samples.
The first Move contact then failed the opaque-artwork movement assertion.
Scale, Rotate and placement remain unrun; no Object rate is qualified. The
failure and original setup attempts remain in the evidence for follow-up.

Raw CSVs, timing reports, hardware/activity records, source and binary identities
are under `artifacts/windows/port-20261007/performance/` in
`batch2-brush-abba-0f2ab217`, `batch2-pen-abba-0f2ab217`,
`navigation-0f2ab217-owned-canvas`, `object-motion-0f2ab217` and
`object-motion-0f2ab217-canvas-size`. Current brush SHA-256 prefix is
`D34F09D9006C`; native executable is `54D289294671`, DLL `2E82C9F64824`.
The retained baseline brush is `3F5189244C13` and app `6CB0E28F0FEA`.
The project remains `FF686BDF3CB3`. No reference-tier, physical-input or
true-presented-frame target is qualified by these observations.

#### Same-revision control

A subsequent BCCB comparison uses `0f2ab217` for both the exported control C
and port candidate B, including the numeric-control fix in the native candidate.
The same Surface, power settings, workloads, pacing and assertions apply.
All four brush blocks passed, including exact Undo/Redo, with six repetitions
per build and preset. All four pen blocks consumed 2401/2401 inputs with zero
unmatched samples; actual injection was 212.47–216.14 Hz. The pen surface stayed
1418 × 988 at density 1.5. No build or other owned GPU workload ran concurrently.
User Chrome remained open; endpoint samples cannot exclude intermittent activity
or thermal effects.

| Workload | Control generations/s, median (range) | Port generations/s, median (range) | Median completion p99, control → port (ms) |
| --- | ---: | ---: | ---: |
| G-Pen | 87.86 (81.28–96.00) | 87.35 (84.32–93.33) | 20.12 → 20.66 |
| Eraser | 85.96 (82.58–96.72) | 86.93 (80.72–98.60) | 17.80 → 18.77 |
| Airbrush | 93.33 (87.31–98.93) | 89.81 (85.32–96.39) | 16.32 → 17.75 |

G-Pen and Airbrush median throughput decrease 0.58% and 3.77%; Eraser increases
1.14%. Completion p99 increases for all three. These observations do not
establish no regression; the decreases remain follow-up work. No runs or tails
were discarded.

The pen values below are medians and ranges of the two per-run percentiles,
in milliseconds, using the unchanged plain drawing and 18 px G-Pen.

| Metric | Control p95 | Port p95 | Control p99 | Port p99 |
| --- | ---: | ---: | ---: | ---: |
| Injection to frame return | 5.80 (5.64–5.96) | 6.71 (6.20–7.21) | 61.06 (60.02–62.10) | 77.15 (56.92–97.39) |
| Injection to presentation observation bound | 10.98 (10.95–11.01) | 11.39 (11.24–11.54) | 74.81 (74.52–75.09) | 91.30 (61.74–120.87) |
| Newest input to presentation observation bound | 10.79 (10.73–10.84) | 10.85 (10.81–10.89) | 13.66 (13.51–13.81) | 13.80 (13.54–14.06) |
| Host frame span | 3.78 (3.72–3.84) | 3.95 (3.90–4.00) | 4.82 (4.71–4.93) | 4.97 (4.73–5.21) |

Frame-return p95 increases 0.91 ms and its ranges do not overlap in this small
sample. The first port run also has longer tails; injection-to-frame maxima
are 161.54–161.90 ms for control and 155.36–208.06 ms for port. These differences
remain follow-up work, with no causal or no-regression claim. Immediate mode
provides an observation bound, excluding physical digitizer and panel latency;
these measurements do not qualify reference-tier or presented-frame targets.

Raw records and all per-run statistics are under
`artifacts/windows/port-20261007/performance/same-head-brush-bccb-0f2ab217/`
and `same-head-pen-bccb-0f2ab217/`. Brush SHA-256 prefixes are `37A8CA3A57DC`
(control) and `D34F09D9006C` (port). Native prefixes are `519A8B45E2FB` and
`F6AF9FE46867`, with DLLs `3FC038404891` and `2E82C9F64824`. The plain pen
project remains `FF686BDF3CB3`. The fresh Object failure is retained separately
under `object-motion-0f2ab217-numeric-feedback/`; it supplies no timing result.

### Windows historical diagnostic, 2026-10-10

On the same Surface Laptop 5, i7-1255U and Intel Iris Xe D3D12 driver
32.0.101.6737, source `3a5da4436` was built in Release with no local production
changes. These headless engine measurements precede the Windows menu and shared
column sizing corrections. No owned compiler or other Capy Canvas workload ran
during capture.
The user's Chrome session remained open, so intermittent background activity is
not excluded.

The existing `brush_frames` workload uses a synthetic 9504 × 6336 drawing,
1000 px brush and offscreen composition. Three 480-frame contacts per brush
retain 479 moving frames each, lasting 5.57–5.88 seconds. Pen-up frames are
excluded; initial moving frames and all tails are retained. Exact native
Undo/Redo passed for every contact.

| Brush | Completed updates/s, median (range) | Completion p99 range (ms) |
| --- | ---: | ---: |
| G-Pen | 85.41 (81.95–86.02) | 21.74–23.95 |
| Eraser | 82.30 (81.49–83.65) | 19.47–19.89 |
| Airbrush | 84.40 (83.35–85.03) | 18.03–18.68 |

These offscreen results qualify neither displayed frame rates nor the reference
tier. They do not resolve the earlier Windows comparison's latency and
throughput differences. Raw contacts, moving-frame summary and build identity
are in `artifacts/windows/port-20261010/performance/baseline-brush/`.
The benchmark SHA-256 prefix is `F7223887BE07`; DXC is `9A5100511E12`.

### Windows brush comparison, 2026-10-17

The same Surface Laptop 5 / Iris Xe D3D12 driver 32.0.101.7085 ran the
original Release `brush_frames` workload: 1000 px brushes, synthetic
9504 × 6336 Paint/Paper canvas, 1600 × 1000 offscreen composition, 64 warmup
frames, two samples and three 240-frame repetitions. Only the 239 moving
frames per repetition count; pen-up, startup, history/readback and CSV writing
are outside the frame timer. Exact Undo/Redo passes for all 24 current presets.

The control is archived `3a5da4436ed4`; the candidate is source manifest
`3E4391CFB134`, based on `7c71b731c391` with the Windows port. They span upstream
brush changes, including grain calibration, so the comparison does not isolate
a local regression. Values below are medians of the three per-repetition rates
or nearest-rank p99 values. Completed time includes CPU-observed GPU completion,
not a GPU timestamp or presented frame. No initial moving frames or tails are
removed. This synthetic canvas and Intel GPU do not qualify the reference tier.

| Preset | Control to candidate completed updates/s | Control to candidate completion p99 (ms) |
| --- | ---: | ---: |
| Pencil | 75.05 to 94.44 | 17.511 to 14.693 |
| Pointy Pencil | 85.73 to 105.80 | 16.755 to 12.210 |
| Shading Pencil | 111.11 to 156.26 | 13.238 to 9.486 |
| Charcoal | 86.73 to 98.62 | 16.760 to 13.098 |
| Crayon | unmeasured to 94.40 | unmeasured to 14.492 |
| G-Pen | 93.17 to 94.09 | 19.187 to 15.055 |
| Rough G-Pen | 67.13 to 65.01 | 28.327 to 22.324 |
| Calligraphy Pen | 115.49 to 109.61 | 12.434 to 12.382 |
| Antique Pen | 86.51 to 76.13 | 21.177 to 17.302 |
| Realistic Pen | 77.14 to 81.49 | 22.634 to 15.550 |
| Wet Ink | 69.22 to 63.43 | 22.798 to 19.840 |
| Blotty Ink | 56.48 to 54.76 | 29.510 to 28.630 |
| Brushed Ink | 49.90 to 51.83 | 37.071 to 28.716 |
| Eraser | 89.60 to 86.69 | 16.751 to 16.384 |
| Paintbrush | 49.62 to 48.92 | 41.654 to 28.601 |
| Bristle Paintbrush | 92.96 to 87.08 | 17.731 to 16.037 |
| Airbrush | 94.68 to 86.66 | 16.750 to 14.768 |
| Chalk | 75.89 to 107.94 | 19.895 to 12.581 |
| Marker | 52.71 to 53.05 | 39.686 to 28.360 |
| Dual Texture | 64.50 to 50.72 | 21.255 to 27.814 |
| Textured Flat | 57.45 to 51.43 | 33.900 to 25.708 |
| Dry Scumble | 50.29 to 49.75 | 33.610 to 28.896 |
| Pastel Block | 49.16 to 107.12 | 37.947 to 13.311 |
| Transparent Glaze | 48.38 to 36.35 | 37.836 to 36.432 |

Of the 23 comparable presets, ten have higher median throughput and thirteen
have lower throughput; the median change is -1.40%. Crayon has no matched
control. These results establish neither uniform improvement nor no regression.
All per-repetition values and maxima remain in the raw evidence.

Raw CSVs are `artifacts/windows/port-20261010/`
`b3a-dx23-before-18a8efb39433/before23.csv` and
`dx24-after-and-native6-prepared-421734aec379/after24.csv`; the latter directory
also holds `retained-performance-comparison.json`. CSV SHA-256 prefixes are
`1B56F75D1E8F` and `B8991D073C60`. The current run has 17,280 rows and 24 exact
Undo/Redo checks; control has 16,560 rows and 23 presets.

#### Same-revision brush control

The same laptop and driver also ran a matched control based on `73a298f2c6fa`
and candidate source `67E7D45A45B4`, with the same Release toolchain, build
options and workload above. Both retain the existing untimed settle repair.
The control ran first, followed by the candidate, with three repetitions of
Dual Texture and Transparent Glaze. Each run retains 1,440 rows and passes
exact Undo/Redo. The browser already running on the machine remained open;
these measurements do not establish an otherwise idle machine.

| Preset | Control to candidate completed updates/s | Completion p99 (ms) | CPU submit p99 (ms) |
| --- | ---: | ---: | ---: |
| Dual Texture | 43.72 to 49.45 | 33.446 to 28.701 | 11.024 to 9.864 |
| Transparent Glaze | 37.17 to 40.51 | 45.688 to 38.409 | 14.766 to 15.667 |

Completed moving rates increase by 13.10% and 8.97%, respectively. Completion
p99 improves for both; Glaze CPU submit p99 worsens. Candidate maximum moving
completion times remain 151.667 ms and 142.986 ms. No moving samples or spikes
are removed. This two-preset comparison does not establish all-brush,
native-input, physical-pen or reference-tier performance.

Evidence is in `artifacts/windows/port-20261010/`
`brush-stock73a-comparable-prepared-20261017/`: the raw runs, immutable build
identities, moving summary and analysis. Benchmark SHA-256 prefixes are
`B43D59C3AF94` (control) and `D118C028944A` (candidate); analysis is
`47B8FA668E72`.

### Windows snapshot routing comparison

On the Surface Laptop 5, i7-1255U and Intel Iris Xe D3D12 driver
32.0.101.7085, the retained control `n3e` is compared with snapshot-routing
build `nrte`, source manifest `5248F242EE22`. Both use Balanced power, a
2256 × 1504 display at scale 1.5, and the same plain 8192 × 6336 project
`FF686BDF3CB3`. These observations do not qualify the reference tablet,
physical pen latency or displayed frame rates.

The original 18 px G-Pen workload injects a ten-second contact after settling,
without a priming stroke, at a nominal 240 Hz. Each run consumes all 2401
inputs with none unmatched; the canvas is 1418 × 988 at density 1.5. All moving
frames and latency tails are retained. Input-to-frame-return values are ms:

| Run | Control p99 | Snapshot-routing p99 | Control maximum | Snapshot-routing maximum |
| --- | ---: | ---: | ---: | ---: |
| 1 | 76.239 | 99.267 | 184.024 | 207.852 |
| 2 | 55.245 | 99.003 | 162.403 | 206.615 |
| 3 | 57.305 | 23.405 | 165.275 | 130.245 |

Pen latency is mixed: two tails increase and one decreases. The candidate's
first host frame takes 72.8–114.6 ms, with additional stalls near contact end.
The measurements do not identify the internal cause or establish that Windows
pen stutter is fixed.

Hand, Zoom and Rotate view use their original maximized-window workload: one
priming contact and three six-second contacts, Reset view between contacts.
Each mode consumes all 4564 inputs with none unmatched. Rates count distinct
renderer submissions during motion; interval p99 retains gaps within contacts.

| Navigation | Control submissions/s | Snapshot-routing submissions/s | Control interval p99 (ms) | Snapshot-routing interval p99 (ms) |
| --- | ---: | ---: | ---: | ---: |
| Hand | 203.112 | 204.986 | 6.999 | 6.843 |
| Zoom | 173.006 | 196.341 | 37.830 | 14.102 |
| Rotate view | 166.847 | 200.039 | 38.550 | 9.917 |

Navigation interval p99 decreases in these captures. UI tracing is absent
during motion; no owned build or competing Capy Canvas workload runs
concurrently. Background activity and thermal effects are not excluded. All 24
navigation captures were reviewed; the pen workload emits no screenshots.
Original guard failures and the Pen 1 geometry supplement remain with the raw
records; no motion workload was repeated for postprocessing.

Raw before/after records are under `artifacts/windows/port-20261010/performance/`
in `dynamic-pen-navigation-9e5c042d1c364f97b79893188fc2f706/` and
`dynamic-pen-navigation-798df53f75f345169e3769253953a717/`. The lossless comparison
is `nrte-four-outer-00aeed8a89d5/direct-all6-original3e-comparison.json`.
Control executable/DLL prefixes are `F7ACFCD0471E`/`1D9E74825AC2`; candidate
prefixes are `9E72CB289257`/`B44C8ED6F692`. Later test-fixture and documentation
changes are not new measurements. Presenter-readiness changes are unmeasured
by these runs.

## Retained-material G-Pen comparison

This earlier comparison is measured on 2026-10-02 at `eb9b8bab1` with the retained-material changes:
the tier photo beneath one paint layer, Perceptual blending, 2048 px G-Pen,
Fit, 16 ms prediction, default workspace with Stats closed, warm-up and three
five-second strokes. Thermal status is zero. Both nondebuggable ARM64 benchmark
builds use release Rust, R8 optimization and the same completed-pool cleanup.

| Build | Fresh updates/s, median (range) | Completion-gap p99, range |
| --- | ---: | ---: |
| `eb9b8bab1` plus bounded Android command-pool cleanup | 86.98 (85.70–89.07) | 31.43–33.17 ms |
| Same base and cleanup, plus retained-material changes | 87.62 (85.98–87.88) | 31.59–38.83 ms |

Neither meets 120/s or the 16.7 ms gap target. The matched comparison isolates
no throughput regression from the retained-material changes. The older 150.2/s
record used a different shared-renderer revision and pool policy; this comparison
does not attribute that difference to hardware or one change.

Candidate APK SHA-256:
`356fdd9d9855cf2e9cd0717d6b59a1343a84b77119b944334186e0bcd3d584d5`.
Baseline APK SHA-256:
`2f45e728edd62a59f6c648cd3e6f08f6d5604770c4df0e3a2669555bf7e61aa5`.
Raw records: `artifacts/testing/material/android-pool-destroy/{gpen-top-fit,gpen-head-baseline}`.
These strokes do not qualify resumed-contact latency; see
[the resumed-contact audit](responsiveness.md#resumed-contacts-before-the-batch-tradeoff).

## Watercolor prediction precision

Measured on 2026-10-02 at `3e0684b08` plus the photo editing changes, on the
reference tablet. The matched nondebuggable ARM64 benchmark builds use release
Rust, R8 disabled, the exact tier photo under one empty paint layer, Perceptual
blending, default Paint workspace, Fit zoom 0.16534 and closed Stats. Preset 20
uses its default settings at 512 px, 16 ms prediction and a 520 × 299 px stylus
ellipse. Each build ran one warm-up and three five-second strokes; thermal
status stayed zero. The candidate also fixes prediction damage outside the
canvas and supplies provisional watercolor style to composition.

| Warm measurement | Before preview fixes | With native wetness precision |
| --- | ---: | ---: |
| Completed updates/s consuming fresh input | 1.196–1.198 | 1.197–1.397 |
| Input completion-gap p99 | 1154.74–1202.27 ms | 960.82–1279.16 ms |
| Owner thread CPU median | 734.39–848.12 ms | 700.57–816.47 ms |

Completed canvas updates and fresh-input updates coincide in these runs. Both
builds finish without the crash in the older table; neither meets the target.
The comparison shows no throughput regression from canonicalizing predicted
wetness. Candidate allocator boundary samples are 2.90–2.91 GB allocated and
3.25–3.31 GB reserved after the warm strokes; these are not peak measurements.

A separate warmed, five-second trace with Stats closed locates the large CPU
cost in command finalization and submission: 29 finish spans total 2927 ms and
28 queue submissions total 1244 ms. Median preparation, paint encoding,
prediction and composition spans are 4.41, 21.28, 1.28 and 9.22 ms. Nested spans
and different sample counts must not be added as a frame budget. Command-pass
counters grow by 11,077 for 1,502 new dabs; source misses grow by 16, with no
upload drain or restore batch. This is the existing material command workload,
not evidence of a hardware ceiling or a performance waiver.
Committed watercolor uses three-dab batches, with immutable per-tile inputs and
three transport stages per material update. Commands consume new contacts;
they do not replay the whole stroke. Reducing those dependent passes requires
changing the painting execution path while preserving its material results.

Candidate APK SHA-256:
`1058ce2101aa60ef80ed71f3b14df9045efd45d1d2282a6b53d966ac1994b5f9`.
Baseline APK SHA-256:
`69b731b96ecb25a634c35b07794ef30431d8e95504f7ad88ddb3b20b3f86d47a`.
Records: `artifacts/testing/material/p11-p14-benchmark/wc-comparison-summary.json`
and `wc-current-11-trace/trace-summary.json` in the same directory. Low and mid
tiers and preset 21 were not remeasured.

## Retouching with the integrated compositor

Measured on 2026-09-29 on the reference tablet: 61 MP Perceptual photo,
512 px brushes, Fit zoom, 520 × 240 px trajectory, 16 ms prediction, three
five-second strokes per brush. Stats is closed and thermal status remains zero.
Fresh updates count completed frames that consumed new real pen samples.

| Brush | Fresh updates/s, median | Completion gap p99, median | Moving-stroke rate |
| --- | ---: | ---: | --- |
| Clone Stamp | 189.6 | 11.8 ms | Meets 120/s |
| Healing Brush | 191.4 | 10.9 ms | Meets 120/s |
| Spot Healing Brush | 193.3 | 12.1 ms | Meets 120/s |

Clone settles in 389–824 ms; Healing in 1,525–1,885 ms and Spot Healing in
3,033–3,437 ms. Cooperative Healing permits independent navigation while
finalization continues; these strokes do not measure that interruption or
physical scanout. All nine runs complete with at least 2,646 MiB system RAM
available. Process PSS is not sampled during rate qualification.

The optimized release build allows foreground submission during two-page idle
refinement and keeps backpressure for required raster work; APK SHA-256
`649f12d58147542a5a8925d7727d11907ceeafb68a7e68b135b3ef9539ca1d7f`.
Raw reports are `artifacts/latency-investigation/qualified-25-top-retouch`.
These successful runs do not establish that the earlier rare Adreno fault is
fixed, or qualify the other brushes and presentation-paced navigation.

## Shared layout regression comparison

GTK release checks on 2026-09-30 use the NVIDIA RTX PRO 6000 Blackwell Max-Q,
Vulkan 615.71.09 and a private 120 Hz display. Each mouse/touch scenario runs
three 5.5-second gestures, with a further alternating repeat. The before build
is `0d5b6dda`; the candidate shares default-chrome layout calls through an inline
helper. These input-paced desktop checks do not qualify the reference tablet or
61 MP canvas. Reports: `artifacts/simplification-cleanup/performance/m27/`.

| Check | Before | After |
| --- | --- | --- |
| Floating group presentation, mouse median | 99.491 fps | 99.492 fps |
| Floating group presentation, touch median | 99.645 fps | 99.643 fps |
| Largest dispatch p95 in alternating repeat | 0.0730 ms | 0.0740 ms |
| Largest placement p95 in alternating repeat | 0.00966 ms | 0.00982 ms |

## Localization measurement coverage

The reference tablet was reserved by another owner during the 2026-10-01 PDT
localization comparison. No commands were issued to it and no localization
brush, numeric, header or toolbar row is qualified on this tier. Low/mid
results and nonreference functional checks cannot fill this gap.

## Android precision and Dehaze diagnostics

On 2026-10-04, the non-reference 90 Hz Huion KP1202 ran the original
9504 × 6336 photo in Photo, with its default display settings and physical
density. The benchmark APK uses optimized release Rust and nondebuggable,
unminified Kotlin. These measurements do not qualify the reference tablet's
120 Hz target. Rates count SurfaceFlinger canvas presents during three
five-second contacts, excluding setup, query waits and subsequent raster drain.

| Motion | Canvas presents/s | Result scope |
| --- | --- | --- |
| Targeted Curves | 24.42 / 24.42 / 24.62 | Production tree `9b81f098`, app `d694fc92`, JNI `3ecaecb4` |
| Dehaze Amount | 0.40 / 0.40 / 0.40 | Rebased tree `98bbf3ec` on `cf6db4f9e`, app `7e62db2d`, JNI `6806e582` |
| Levels Input Black | 19.42 / 19.62 / 18.21 | Same rebased app, test-only native-contact fixture `037aa79c`, test APK `30348574` |

Levels Auto adopts its exact result in 23.524 s; Undo restores Gamma 2. The
98.8 px native prime changes only Input Black, and measured contacts vary it
through approximately 0.005–0.482. The earlier 24.7 px prime did not change a
control. Platform touch slop is 16 px, but slop alone is not established as that
failure's cause; these different gesture ranges are not a before/after comparison.

Retaining unchanged pre-effect statistics removes a second full-source scan
when targeted adjustment ends: Exact returns 58 / 71 / 91 ms after retiring the
contact, compared with a previous 27-second rescan. Up-to-Exact remains
7.82–7.87 s including raster drain and retirement scheduling; it is not the
query latency. Source edits still invalidate the retained result.

Dehaze retains its 6.29 MB guide during Amount changes. A separate trace records
four internal GPU waits occupying 2.456 s of 3.043 s composition wall time during
the contact. Command finishing occupies 1.061 s across overlapping scopes;
these spans are not additive CPU measurements. The ordinary final-submit
completion of 16–19 ms excludes earlier internal work.

Single-contact diagnostics on the same photo's 12/24 MP derivatives produce
4.2 / 2.2 presents/s. They retain 258 / 513 MB native hierarchies; the 61 MP case
uses a bounded 16 MiB strip without a resident hierarchy. Non-idle GPU timestamp
history is approximately 210 / 420 / 1,250–1,420 ms, including scheduling gaps
and warm-up/drain samples. The changed admission path prevents a linear
per-pixel scaling conclusion. Coarse display-resolution Dehaze fails 6 of 84
quality comparisons; a tested two-mip-finer policy reaches native resolution
at this Fit scale. No hardware ceiling or soft-target waiver is established.

Thermal status is 0 at the recorded boundaries. PSS after contacts is
1.76–1.77 GB for Targeted and 1.73–1.77 GB for Dehaze; renderer allocation
boundaries are about 1.02 GB and 790–796 MB. These are not continuous peaks.
Raw APK identities, native contacts, retention timings and full traces are in
`artifacts/photo-editing-color/p20-p26-android-performance/`;
`performance/dehaze-final-calibration-attribution.json` separates each path.

## Unified gradients and precision controls

On 2026-10-04, the reference MovinkPad Pro 14 ran the original 9504 × 6336 photo at Fit in
Photo, default density 280, with thermal status 0 at the recorded boundaries.
The private nondebuggable benchmark uses optimized release Rust. Its base is
`0af58a32d` plus the unified-gradient changes: app SHA256 `5921585b`, test
`c37654eb`, JNI `06c1da32`, source diff `d27cea30`. Each row primes, undoes and
settles before three five-second contacts. Rates count SurfaceFlinger canvas
presents strictly inside those contacts.

| Motion | Canvas presents/s | p99 present gaps, ms | After-Up drain, s |
| --- | --- | --- | --- |
| Gradient tool geometry | 118.90 / 119.10 / 119.10 | 8.33 / 8.33 / 8.33 | 4.81 / 4.75 / 4.72 |
| Gradient Map stop, before texture reuse | 27.42 / 26.62 / 27.42 | 75 / 83 / 75 | 3.11 / 3.29 / 3.27 |
| Gradient Map stop, with texture reuse | 27.42 / 28.62 / 30.22 | 92 / 83 / 75 | 3.06 / 3.20 / 3.11 |
| Gradient Fill stop, before texture reuse | 9.81 / 9.01 / 8.41 | 183 / 217 / 158 | 7.06 / 7.10 / 7.07 |
| Gradient Fill stop, with texture reuse | 16.01 / 15.61 / 16.21 | 142 / 142 / 142 | 7.18 / 7.01 / 7.29 |
| Levels Input Black | 28.22 / 32.03 / 30.83 | 75 / 67 / 67 | 3.28 / 3.24 / 3.21 |
| Targeted Curves | 49.04 / 48.44 / 48.24 | 242 / 242 / 242 | 1.57 / 1.61 / 1.56 |
| Dehaze Amount | 2.20 / 2.20 / 2.40 | 483 / 542 / 475 | .70 / .66 / .90 |

Tool geometry meets the display-paced motion target for its interactive line
preview. Each release adopts one document edit; the subsequent raster work and
drain are separate and do not establish 120 Hz full-resolution painting. Opening
Tool Settings also presents Navigator in the default Photo column; the benchmark
declares and records that visible panel. The other rows miss the 120 Hz target.
Native stop definitions change hundreds of times per contact.

Levels Auto adopts its exact result in 14.885 s, and Undo restores Gamma 2.
Targeted adjustment restores retained Exact statistics 13.397 ms after contact
retirement; Up-to-Exact is 4.14 s including raster drain and retirement scheduling.
These are different latencies. Renderer allocations at the measured boundaries
are 2.28–2.73 GB; they are not continuous peaks. The resident native source
hierarchy differs from the earlier Huion workload, so these rates are not a
matched device speedup.

Separate one-contact profiles show display-sized main gradient composition,
about 3.76 million changed mip pixels per update, and stationary source upload
hit/miss counters. The ordinary contacts before reuse record approximately
39 ms renderer-owner CPU per submitted callback for Fill, versus 5 ms for Map.
Fill's two cached branch images each occupy 60,678,144 bytes.
Parameter edits now recycle invalidated branch storage
with empty validity instead of allocating replacement textures.

The matched reuse build uses app `9bdec068`, test `ad71997f` and JNI `dc50cfba`,
with the same benchmark fixture, photo, layout and physical density. Fill's
owner CPU median falls to 6.52–6.93 ms, and its rate rises to 15.61–16.21 frames/s.
The 142 ms p99 gaps still miss the motion bound; seven-second post-contact drain
remains a separate latency.
Map records 27.42–30.22 frames/s with 4.83–5.26 ms owner CPU; the small rate
change does not establish a repeatable gain, and its target remains unmet.
The frozen build manifest is
`artifacts/photo-editing-color/p30-android/keyboard-branch-reuse/numeric-phase/PERFORMANCE.local.json`;
the matched contacts are in `performance/pro14-p30-branch-reuse-fill/` under the
performance artifact directory below.

The Fill profile also records 2.284 s of command finishing on a separate worker
inside its five-second contact. The visible Histogram and windowed capture
pattern suggest live statistics, but the trace lacks thread names, so that
attribution is inferred. Overlapping wall-time scopes are not additive CPU cost;
final-submit completion omits earlier internal composition waits and is not total
GPU request latency. Neither the remaining misses nor these profiles establish
a hardware ceiling or a soft-target waiver.

APK and source identities are in
`artifacts/photo-editing-color/p30-android/final-rebase/PERFORMANCE.local.json`.
Contact records and traces are under
`artifacts/photo-editing-color/p20-p26-android-performance/performance/`;
`pro14-gradient-profile-attribution.json` separates the profiled paths.

Separate desktop diagnostics use 256 × 256 documents and do not qualify a tablet
tier. GTK stop drags present 49.45–49.46 frames/s with 50 Hz injected input;
tool geometry presents 96.12–98.52 frames/s. In Web, coalescing gradient previews
to animation frames reduces projection calls over matched five-second contacts
from 1,202 to 298 for mouse, 1,040 to 516 for touch, and 1,248 to 144 for pen.
Median projection cost remains 0.8–0.9 ms. These Web counts measure CPU preview
work, not presented frames. Raw records are in
`artifacts/photo-editing-color/p28-gtk/final-0af58/` and
`artifacts/photo-editing-color/p28-web/final-matrix/`.

## Tool cursors

The Wacom MovinkPad Pro 14 runs three five-second hover gestures on the
9504 × 6336 Sony photo with one empty paint layer at Fit zoom, the default
workspace and Navigator, thermal status 0, and 120 Hz input. The benchmark build
is based on `0516be628` with shared tool cursors, measured 2026-10-04.

| Cursor | Brush diameter | Renderer submissions/s | Submission interval p99 | Viewport GPU p99 |
| --- | --- | --- | --- | --- |
| Tool | 18 px | 119.3 | 15.57 ms | 0.237 ms |
| Tool and brush size | 18 px | 119.5 | 14.98 ms | 0.240 ms |
| Tool and brush size | 2048 px | 119.6 | 14.84 ms | 1.312 ms |
| Brush size baseline | 2048 px | 119.8 | 14.33 ms | 1.315 ms |

These are renderer submissions, not display presentation, so they do not qualify
the 120 fps target. Hover leaves the artwork revision unchanged. Raw records and
the report are in `artifacts/tool-cursors/viewport-final/` and
`artifacts/tool-cursors/viewport-report.json`. Low and mid reference tablets
remain unmeasured for these cursor modes.

## Layer reorder and retained photo translation

On 2026-10-04, the reference MovinkPad Pro 14 runs the 9504 × 6336 Sony photo
with one empty paint layer at Fit, default workspace and glass, density 280,
and thermal status 0 before and after each benchmark. The isolated benchmark
uses optimized release Rust and nondebuggable Kotlin, based on `7fcca04e5` with
the layer audit changes. App SHA256 is `dee1a234c5e2`, test `31c1ba8ea110`, and
production-source manifest `206808fab2dc`; full hashes and source-file digests
are in `artifacts/layer-audit-validation/performance/build.local.json`.

Each benchmark primes and resets its gesture before three five-second contacts.
Only moving windows count; percentiles use the nearest-rank calculation.

| Motion | Moving rate, per second | Moving gap p99, ms | Criterion |
| --- | --- | --- | --- |
| Layer reorder preview, native UI FrameMetrics | 119.41 / 117.20 / 118.85 | 8.38 / 16.67 / 16.67 | Pass: all runs exceed 114 fps and stay within 16.7 ms |
| Retained dry-photo Transform, renderer completions | 94.88 / 93.68 / 95.48 | 20.86 / 21.58 / 20.70 | Fail: all runs miss both bounds; canvas presentation unmeasured |

The transform keeps Navigator visible. Its completion counts include retained
Navigator work and do not establish fresh-input or displayed-canvas throughput.
Native UI stays static during these contacts; SurfaceFlinger records canvas
buffer traffic but lacks repeated canvas frame-timeline presentation records.
The reorder result qualifies the moving native layer preview, not photo editing
or physical input-to-display latency. These are current measurements without a
matched baseline, so they do not establish a regression or improvement.

Transform renderer-owner CPU median is 5.47–5.61 ms, callback wall time
8.44–8.47 ms, viewport encoding 1.83–1.85 ms and queue-present wall time
3.17–3.19 ms. Publication scheduling has a 0.012–0.014 ms median and at most
0.059 ms p99. Submit-to-observed-completion medians are 13.88–13.97 ms;
these include queueing and polling, not isolated GPU execution. Source/cache
counters remain unchanged. This evidence points to the renderer and driver
path rather than repeated native layer-row refresh, without establishing a
hardware limit.

Raw moving windows, FrameMetrics, renderer measurements, thermal boundaries,
SurfaceFlinger traces and the strict-window summary are in
`artifacts/layer-audit-validation/performance/`. Low and mid tiers and sustained
edge autoscroll were not measured here.

## Filter attachment feedback

Measured 2026-10-04 on MovinkPad Pro 14, thermal status 0 before and after,
with the 9504 × 6336 reference photo at Fit, default workspace and panel glass,
one clipped empty paint layer, and attached Gaussian Blur and Curves.
`AndroidTitleBarTest#layerSwipeFrameTiming` with `layerReorderBenchmark` and
`layerRelationshipBenchmark` drags Curves through the neighboring filter gaps.
One priming gesture precedes three five-second mouse gestures; every drop is
canceled.

| Run | 1 | 2 | 3 |
| --- | --- | --- | --- |
| Native UI frames/s | 93.24 | 93.93 | 102.71 |
| Moving-frame interval p99, ms | 33.33 | 33.33 | 25.00 |

These `FrameMetrics` observations do not meet the tier target. They measure the
attached-filter workload, separately from ordinary row reorder. No matched
baseline was measured. The benchmark APK uses release Rust and unminified Kotlin
from `7f69a9356` plus the clipping-filter changes. APK/source hashes, raw frames,
display and thermal records, and the summary are in
`artifacts/clipping-filter/build-provenance.json`,
`artifacts/clipping-filter/top/` and
`artifacts/clipping-filter/motion-summary.json`.

## Brightness to Opacity and Threshold

Measured on the reference tablet on 2026-10-05. Fresh merged first-pair
measurements use `merged-pair` benchmark APK
`5cc546ae3f9bb1a0372f660660757c970949becfca8f90440f3c85edeb75cf97`, built from
`09e2193f86f155977022fadd4248c768b7fd5acc`. Each completed case verifies its
installed APK; raw completion and contact timing are independently recomputed.
Recorded thermal boundaries are zero.

The optimized Android benchmark uses 9504 × 6336, G-Pen 2048 px, default
workspace with Navigator, Stats closed, prediction enabled and workspace ink
color. Motion cases use three warmed five-second strokes. Threshold uses Black,
Threshold Transparency and retained Alpha Threshold 37%; Brightness to Opacity
has no parameters. Enabled and disabled cases use matching authored graphs and
workloads.

**Incremental efficiency accepted; absolute tier targets are not met.** The
revised criterion accepts efficient incremental work over an already failing
disabled baseline. Disabled-baseline failures remain failures; paired throughput
does not isolate GPU filter cost or prove optimality.

| Filter | Blending | Zoom | 64 px canvas on / off | Large canvas on / off | 64 px fresh on / off | Large fresh on / off | Added large canvas time (ms) |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| Brightness to Opacity | Perceptual | Fit | 212.87 / 234.56 | 127.10 / 168.57 | 162.58 / 168.68 | 126.90 / 158.39 | +1.936 |
| Brightness to Opacity | Perceptual | 100% | 358.48 / 370.23 | 49.88 / 65.84 | 198.31 / 197.91 | 49.88 / 65.84 | +4.859 |
| Brightness to Opacity | Linear | Fit | 211.49 / 227.44 | 132.10 / 177.22 | 162.40 / 166.81 | 130.30 / 165.70 | +1.927 |
| Brightness to Opacity | Linear | 100% | 367.05 / 371.91 | 51.09 / 70.52 | 198.07 / 197.14 | 51.09 / 70.52 | +5.394 |
| Threshold | Perceptual | Fit | 217.97 / 235.14 | 128.29 / 167.81 | 163.93 / 171.05 | 127.32 / 156.66 | +1.836 |
| Threshold | Perceptual | 100% | 360.13 / 370.00 | 56.89 / 66.24 | 197.32 / 197.97 | 56.89 / 66.24 | +2.481 |
| Threshold | Linear | Fit | 206.20 / 227.84 | 132.00 / 176.43 | 163.16 / 166.93 | 131.21 / 164.25 | +1.908 |
| Threshold | Linear | 100% | 364.60 / 365.83 | 57.66 / 69.82 | 197.96 / 197.52 | 57.66 / 69.82 | +3.020 |

The target is 120 completed canvas updates/s with p99 gaps at most 16.667 ms.
Fresh input-consuming updates and their gaps are reported separately. Medians
above summarize three runs; the gap and age columns show ranges across the three
per-run percentiles. Added time is `1000/enabled rate − 1000/disabled rate`, a
throughput comparison rather than isolated GPU filter timing.

Response measurements for the large brush (ranges across per-run percentiles):

| Filter / blending / zoom / state | Canvas gap p99 (ms) | Fresh gap p99 (ms) | Input age p95 (ms) | Input→GPU p95 (ms) |
| --- | ---: | ---: | ---: | ---: |
| Threshold / perceptual / Fit / Enabled | 15.18–24.44 | 16.49–24.44 | 30.00–60.00 | 29.64–64.45 |
| Threshold / perceptual / Fit / Disabled | 18.71–22.03 | 19.25–22.34 | 40.64–65.00 | 44.69–66.05 |
| Brightness to Opacity / perceptual / Fit / Enabled | 17.73–25.10 | 17.76–25.10 | 30.00–65.00 | 33.37–66.52 |
| Brightness to Opacity / perceptual / Fit / Disabled | 16.10–17.08 | 17.77–18.16 | 45.00–50.00 | 48.79–56.73 |
| Threshold / perceptual / 100% / Enabled | 27.09–29.42 | 27.09–29.42 | 56.00–60.00 | 59.12–62.44 |
| Threshold / perceptual / 100% / Disabled | 19.45–21.81 | 19.45–21.81 | 50.00–50.00 | 49.00–49.72 |
| Brightness to Opacity / perceptual / 100% / Enabled | 27.94–30.23 | 27.94–30.23 | 65.00–65.00 | 66.49–68.88 |
| Brightness to Opacity / perceptual / 100% / Disabled | 19.54–20.37 | 19.54–20.37 | 49.06–50.00 | 48.65–52.75 |
| Threshold / linear / Fit / Enabled | 19.14–24.80 | 19.14–24.80 | 45.00–60.00 | 48.57–64.56 |
| Threshold / linear / 100% / Enabled | 27.41–27.89 | 27.41–27.89 | 55.00–60.00 | 58.43–60.74 |
| Threshold / linear / Fit / Disabled | 13.56–21.12 | 15.59–21.12 | 35.00–80.00 | 39.10–83.60 |
| Threshold / linear / 100% / Disabled | 18.09–19.68 | 18.09–19.68 | 45.00–50.00 | 45.58–49.99 |
| Brightness to Opacity / linear / Fit / Enabled | 17.55–28.64 | 19.42–28.64 | 40.00–75.00 | 44.70–76.30 |
| Brightness to Opacity / linear / 100% / Enabled | 27.76–28.81 | 27.76–28.81 | 65.00–65.00 | 65.52–66.49 |
| Brightness to Opacity / linear / Fit / Disabled | 15.79–19.88 | 16.03–19.88 | 40.00–75.00 | 41.16–80.92 |
| Brightness to Opacity / linear / 100% / Disabled | 17.89–19.56 | 17.89–19.56 | 45.00–45.00 | 45.86–46.51 |

Among 36 completed motion cases, 17 pass all three runs of the canvas
rate-and-gap gate, 8 pass the fresh rate-and-gap gate, and 9 meet the age limit.
These separate counts do not establish a combined pass.

[Full current rows, individual
gates](../../artifacts/illustration-filters/pair1/merged-pair-performance-doc-draft-top.txt).
Absolute disabled-baseline misses remain failures; the revised criterion
assesses efficient incremental cost separately.

 | Fit Perceptual large brush | State | Long-stroke settle range (ms) | | --- |
--- | ---: | | threshold | Enabled | 254.97–303.82 | | threshold | Disabled |
237.79–279.52 | | brightness_to_opacity | Enabled | 266.37–296.80 | |
brightness_to_opacity | Disabled | 202.59–352.37 |

Three long strokes give a range, not a settling percentile. Short local-stroke
settling remains separate.

| Three-photo translucent stack, varying pressure | Canvas on / off | Fresh on / off |
| --- | ---: | ---: |
| threshold | 158.87 / 207.72 | 135.98 / 164.09 |
| brightness_to_opacity | 160.40 / 207.02 | 136.46 / 163.78 |

Native pointwise output is evaluated into retained filter images before
reduction. Invalid required native pages are captured in contiguous row batches
of at most 16, skipping valid pages and gaps. Historical native-direct
diagnostic traces and reviews describe this implementation; their older APK
measurements do not qualify this merged build. Current paired rates preserve the
material native-resolution increments: approximately 9–10 ms per effective
canvas update on Mid, 2.5–3.0 ms for Threshold and 4.9–5.4 ms for Brightness to
Opacity on Top. The pointwise algorithm evaluates required changed native pixels
once before reduction. Current 73 GPU tests and 16 refinement tests cover no-op
reuse, bounded passes, native ordering, source leases and the older-path
counterfactual. Source and output bindings cache actual view tuples for the
current and previous frame; different resources require distinct groups. These
code and physical-work checks support scoped algorithmic efficiency acceptance
without proving every measured tail unavoidable or global optimality.

On the 9504 × 6336 canvas, matched Threshold enabled/disabled runs each
completed 20 five-second 2048 px GPen contacts with Undo between contacts, then
retained the final stroke for 120 seconds. Threshold uses Colors=1,
Transparency=1 and alpha threshold=37 in Perceptual blending. Thermal status was
0 at both boundaries of every run. Settling uses nearest-rank p95 across 20
contacts.

| Threshold | Five-second settle p95 (ms) | Max sampled GPU allocated / reserved (MiB) | Max sampled PSS / RSS (MiB) | Minimum sampled system MemAvailable (MiB) |
| --- | ---: | ---: | ---: | ---: |
| Enabled | 373.656 | 2541.29 / 2582.82 | 912.95 / 938.16 | 2365.65 |
| Disabled | 394.175 | 2505.49 / 2566.82 | 845.80 / 869.80 | 2473.76 |

The maxima and minimum span the whole captured sampler, including setup and Undo
after idle where recorded; maxima are separate samples, not simultaneous or
continuous peaks. Idle comparisons exclude Undo after idle. These long-stroke
settling results do not establish the short local-stroke settling target.

Enabled contact 5→20 endpoints: PSS +21.18 MiB, RSS +28.94 MiB, mappings -158;
tracked GPU allocated +0.03 MiB and reserved +0.00 MiB. During 120.040s idle:
PSS +5.48 MiB, RSS -20.41 MiB, mappings -3, tracked GPU allocated -0.07 MiB and
reserved +0.00 MiB. Disabled contact 5→20 endpoints: PSS +12.90 MiB, RSS +15.33
MiB, mappings +82; tracked GPU allocated +0.00 MiB and reserved +0.00 MiB.
During 120.031s idle: PSS -31.61 MiB, RSS -35.23 MiB, mappings -221, tracked GPU
allocated -0.02 MiB and reserved +0.00 MiB.

After idle, enabled minus disabled was PSS +42.01 MiB, RSS +42.21 MiB, tracked
GPU allocated +8.39 MiB and reserved +0.00 MiB. These are separate
processes/runs with matching requested setup; the differences do not isolate
allocator or driver history.

The private-photo large accepted-affine journey passed in 187.994s: 20 nonzero
drags, accepted Apply, 120.236s idle, resumed nonzero drag and Apply. All 30
snapshots and 21 input probes share one PID and ordered boot times; map counts
and live canvas input paths match. This exercises accepted resource high-water,
not an exceptionally large individual command buffer.

| Large affine stage samples | GPU allocated / reserved (MiB) | PSS / RSS (MiB) | Minimum system MemAvailable (MiB) |
| --- | ---: | ---: | ---: |
| Separate maxima over 30 stages | 2458.49 / 2525.46 | 1351.02 / 1387.17 | 2638.62 |

Accepted idle: PSS +284.98 MiB, RSS -161.89 MiB, mappings -539, tracked GPU
allocated +0.00 MiB and reserved +0.00 MiB. PSS and RSS moved in opposite
directions; the collected resident components cannot reconstruct sharing,
memtrack or the sampling-time difference. Idle end→resumed Apply: PSS -407.49
MiB, RSS +130.07 MiB, mappings +1338, tracked GPU allocated +0.00 MiB and
reserved +0.00 MiB. PSS and RSS moved in opposite directions; the collected
resident components cannot reconstruct sharing, memtrack or the sampling-time
difference.

[Large-affine resource
ledger](../../artifacts/illustration-filters/pair1/top-merged-pair-large-memory-summary.json).

PSS is process-level evidence, not isolated driver command-memory accounting.
Snapshot fields are read sequentially; nearest same-PID contact PSS samples are
strictly inside idle with explicit time offsets. Mapping counts do not count
command pools. These sampled runs establish neither a transient peak bound, a
per-command byte bound, nor a leak or its absence.

[Current resource ledger and raw
audit](../../artifacts/illustration-filters/pair1/merged-pair-current-memory-summary.json).

Resumed 100 ms contacts use three runs of 21 contacts per state. Cold is the
first contact; warm groups use actual pending-composition flags. Requested 5
ms/1,000 ms gaps alone do not determine actual state. The table reports actual
warm pending/settled next GPU completion, including completions after release;
GPU completion is not scanout.

| Filter | Requested gap (ms) | Actual warm group | Count on / off | Next GPU p95 on / off (ms) | Added p95 (ms) |
| --- | ---: | --- | ---: | ---: | ---: |
| threshold | 5 | warm_pending | 60 / 60 | 129.44 / 125.76 | +3.675 |
| threshold | 1000 | warm_settled | 60 / 60 | 132.09 / 111.01 | +21.077 |
| brightness_to_opacity | 5 | warm_pending | 60 / 60 | 168.82 / 116.06 | +52.759 |
| brightness_to_opacity | 1000 | warm_settled | 60 / 60 | 139.27 / 107.97 | +31.307 |

All completed matched resume rows fail absolute throughput and input-age
criteria. Contacts without an active-window fresh completion remain failures;
post-release completions count only as latency evidence. Final-input settling is
not per-contact settling p95. Positive paired contact penalties remain material
unresolved observations; historical traces do not establish their current cause
or inevitability. They remain explicit limitations of the scoped algorithmic
efficiency acceptance.

| Contacts without fresh completion in the active window | 5 ms enabled / disabled | 1,000 ms enabled / disabled |
| --- | ---: | ---: |
| threshold | 11/63 / 9/63 | 62/63 / 44/63 |
| brightness_to_opacity | 11/63 / 9/63 | 61/63 / 43/63 |

[Current raw resume
recomputation](../../artifacts/illustration-filters/pair1/merged-pair-matched-resume-summary.json).
Measurements remain attributed to benchmark APK
`5cc546ae3f9bb1a0372f660660757c970949becfca8f90440f3c85edeb75cf97`; final
fixture-correction APK confirmation is separate and does not relabel these
samples.

Separate final APK
`9de4984f656bbbd12a17e3023f7fc85e959d10d9885a3cc6d6150e3d25026c76` confirmation
uses three warmed Fit, Perceptual large-brush strokes per state. The
fixture-only correction has a separate runtime-equivalence audit; the full
matrix and resource evidence above remain attributed to the measured APK.

| Final APK Fit confirmation | Canvas on / off | Fresh on / off |
| --- | ---: | ---: |
| threshold | 139.24 / 171.53 | 137.24 / 162.01 |
| brightness_to_opacity | 123.61 / 172.31 | 123.21 / 164.69 |

[Final APK primary raw
audit](../../artifacts/illustration-filters/pair1/merged-pair-final-matched-evidence-audit-primary.json).
Native paint/mask/history/reopen and parameter/source-persistence checks pass
separately on both tiers. The Top supplemental trace and identical rerun fail
strict GPU coverage and remain preserved.

The focused final-APK Brightness to Opacity rapid-contact capture and identical
rerun fail strict elapsed-GPU coverage; the rerun also lacks mandatory phase
records for changed renderer submissions. Complete GPU-time attribution is
unavailable. A separate physical-counter/CPU audit covers every nonempty
submission from input begin through settled: 351 enabled and 449 disabled,
across 63 contacts per state. It excludes every GPU timing observation. The
traced warm pending contact p95 is 146.475 / 173.662 ms enabled / disabled,
while the mean is 70.113 / 64.267 ms; this does not reproduce the untraced
+52.759 ms p95 increment or establish a speedup.

| Focused rapid-contact physical evidence | Enabled | Disabled |
| --- | ---: | ---: |
| Root composition pixels | 109,427,712 | 131,486,912 |
| Command passes | 5,662 | 4,897 |
| Source misses across three runs | 0 / 48 / 52 | 0 / 47 / 44 |
| Upload drains / backing restores | 0 / 0 | 0 / 0 |
| Texture / buffer allocation spans | 0 / 0 | 0 / 0 |
| Binding creation spans | 30,418 | 6,921 |
| Mean owner CPU per callback (ms) | 15.217 | 11.076 |

Root pixels do not increase, while passes and binding creation increase with the
enabled native filter. Scene bindings retain current and previous frame view
tuples; this prevents repeated same-view creation inside retained frames. The
unlabeled allocation spans do not prove that every added binding is necessary.
Higher renderer service is consistent with queue amplification, but incomplete
GPU observations leave the cause of the untraced contact-tail increment
unresolved. This evidence excludes neither all redundancy nor a further
efficiency improvement.

[Separate complete physical-counter/CPU
audit](../../artifacts/illustration-filters/pair1/merged-pair-final-top-bto-resume-physical-diagnostic.json).
The strict timing failures remain failures; physical evidence establishes no
total-GPU or phase-time pass.


## Filters previews

On 2026-10-05/06, MovinkPad Pro 14 benchmark builds compare the original preview
scheduler at `f2a56cdc5` with the adaptive preview change based on `2dc8a08cc`.
Both use the same 9504 × 6336 Sony photo and private application ID. Opening
All filters until the Curves thumbnail contains photo pixels takes
**6.782 → 0.943–1.015 s** (one baseline opening, two final openings). This is
first ordinary-thumbnail readiness, not completion of every source-analysis row.

`AndroidRasterTest#largePhotoFilterPreviewDrawing` alternates three hidden-panel
controls with three visible-panel strokes of five seconds, after a priming
stroke. Across two final runs, visible-panel p95 input-queue time is
7.979–8.433 ms versus 7.794–8.323 ms hidden; CPU time is 9.710–10.194 ms versus
9.206–9.955 ms, and frame-start gaps are 10.064–10.762 ms versus
9.573–10.324 ms. All six visible strokes pass the +2 ms queue and CPU limits
against their run's slowest hidden control. One stroke in the first run misses
the +0.5 ms frame-gap limit by 0.110 ms; the other two pass. The unchanged repeat
passes all three comparisons at thermal status 0 before and after the run. The
baseline passes these interference checks too. Keep the first failure visible:
these host timings do not establish GPU-completed updates, fresh-input throughput
or the 120 fps target. Thermal state was not recorded for the first run.

The fixture's post-motion thumbnail wait falls from 7.067–7.844 s to 30–287 ms,
but starts after telemetry collection and must not be read as physical pen-up
latency. Raw measurements and build logs are in
`artifacts/fx-performance/{baseline,final,final-repeat}/`. Low-tier and mid-tier
performance remain unmeasured for this change.

## Zoom settings navigation comparison

The 2026-10-09 1.0.12 run uses the tier photo, an empty paint layer and the
Navigator, warm shaders, thermal status 0 and three five-second gestures.
SurfaceFlinger actual-present timestamps establish the displayed rates. The
`smooth_zoom` benchmark uses native stylus input and verifies changing zoom
percentages without changing the drawing revision.

Baseline `f02d74518` horizontal Zoom measured 34.32–38.44 fps, p99 50.00 ms. Both
builds miss this tier. The measured build includes upstream rendering changes
as well as Zoom settings, so this comparison does not isolate their cost or
establish no regression.
Current per-gesture evidence: `artifacts/navigation-controls/zoom-audit-20261009/post-rebase-results.json`.
## GTK pen pressure utility

Remeasured on 2026-10-10 UTC on the `996cc49f3` working tree with GTK panel
surfaces owned by their workspace containers, using the frozen tester and app
identified in [GTK Sketch Layers diagnostic](#gtk-sketch-layers-diagnostic).
The private Mutter display and maximized window are 1600 × 1000 at 120 Hz,
scale 1, on the same NVIDIA RTX PRO 6000 Blackwell Max-Q/Vulkan workstation.
The fixture uses a 256 × 256 F32 drawing at Fit and default Low transparency;
the utility itself remains opaque. These desktop results do not qualify the
reference tablet or its 61 MP canvas.

After the mouse, touch and injected-pen functional journey warms the controls,
each theme runs three five-second panel drags and three curve drags. The
utility follows a 200 × 100 logical-pixel ellipse; placement snaps to device
pixels. The curve handle follows a 4 × 20 logical-pixel ellipse inside the
graph, changing both authored input and output. The native target hit is
checked before every press. Only completed presentations during observed
moving native allocations or authored-control updates count. Idle intervals
are excluded; interval p99 uses nearest rank. Screen recording, other GPU
journeys and builds are stopped during measurement.

| Motion | Light presents/s / maximum p99 gap | Dark presents/s / maximum p99 gap |
| --- | --- | --- |
| Utility frame | 119.20–119.21 / 8.740 ms | 119.20–119.40 / 9.207 ms |
| Pressure curve | 120.00 / 8.470 ms | 120.00 / 8.463 ms |

All twelve current gestures meet the desktop 114 presents/s floor and strict
2000/120 = 16.6667 ms gap limit. Matched Light runs with the preserved
`996cc49f3` production sources give 112.62–119.41 presents/s and maximum p99
24.994 ms for the utility, and 119.80–120.00 with maximum p99 8.466 ms for the
curve. The preserved tester is the same one identified in the Color resize
comparison. These small samples do not establish reference-device performance.

Moving the utility updates shared bounds without rebuilding the curve plot.
GTK retains content and measurement, then presents pending placement through
the workspace frame callback. The native journey verifies zero utility
backdrop regions and separately checks opaque, faded and translucent synthetic
surfaces. Both themes pass functional mouse, touch and injected-pen input;
these paths do not establish physical pen behavior.

Current raw reports are in
`artifacts/panel-surface-ownership/pressure8-{light,dark}/pressure-motion.json`;
the matched Light baseline is in `baseline-pressure8/pressure-motion.json`.
`pressure-metrics.json` retains all eighteen measured gestures. Extra probes
at 4 ms input cadence record zero movement in both builds, so they provide
no timing result. The documented 8 ms cadence passes in both builds; no input
or fixture code is changed for those probes. Earlier measurements at
1100 × 800 are in `artifacts/pressure-calibration/immediate-removal-checks/`
and are not a matched comparison with this larger viewport.

Run `native_pressure_calibration --tablet` with `LAYER_PRESSURE_MOTION=1` and
`LAYER_NATIVE_EVENT_MS=8` to repeat the workload.

## Apple native measurements

Current integrated Apple sources are unqualified. Functional Palette resize and
allocation checks do not establish presentation rates. Current optimized motion
measurements remain required.

| Hardware / workload | Current rate and metric | Motion interval p99 | Qualification |
| --- | --- | --- | --- |
| M2 Pro Mac mini, 90 Hz: drawing profiles | Not measured | Not measured | Unqualified; current optimized comparison required |
| M4 iPad Pro, 120 Hz: drawing profiles | Not measured | Not measured | Unqualified; current optimized comparison required |
| Mac: Palette resize, light and dark | Not measured | Not measured | Unqualified; current optimized measurement required |
| Mac and iPad: layer scrolling and drawer transitions, light and dark | Not measured | Not measured | Unqualified; current motion measurements required |
| M2 Pro Mac: brush-size stamp preview, light | Longest unchanged or unsampled interval: 92.44 ms; prior 1,244.49 ms | Not measured | Four original six-second continuity checks pass; presentation rate unqualified |

Record source and binary identity, date, canvas/display extent, workload, thermal
coverage and collector limitations with each result. Use the
[Apple measurement procedure](../../apps/layer-apple/PERFORMANCE.md) and
[motion rules](measuring.md). Distinguish completed canvas updates, presented
frames and renderer submissions; a shared geometry poll is not a native frame.
Synthetic documents and renderer-only rates do not qualify the reference tier
or physical input latency.

The 2026-10-24 Apple bubble diagnostic uses a Release build on the non-reference
M2 Pro Mac at 90 Hz, the prescribed 9504 × 6336 photo at Fit, an empty selected
paint layer above it and default glass. The native editor is 1200 × 870 points
at 2× scale. Five owners comprise three eligible tool faces, folded Layers and
Color; this differs from the Android five-owner mix and is not a direct host
comparison. `tests/tool-bubble-motion.swift` primes a six-second native mouse
drag, then measures three six-second contacts through four neighbours. Every
owner must move more than eight points. Workspace Undo and settling happen
outside timing. Timestamped native owner poses define the first and last
observed movement; windows exclude pickup delay and 100 ms at either moving
edge. Unpresented drawables and uncovered boundaries remain explicit failures.

Compiled inputs are based on `be77c6cc8` plus the Apple port, digest
`a4517f57063e556f3c9b68dab67b27a3b4e427cbad983c1c66fd5982605d936f`.
Executable SHA-256 is
`8e5c280eba0a2f68ea2d2c26d9e569e3f8a212038913b9685e5029fabd0650cb`.

| Theme | Canvas presents/s, three moving windows | Presentation interval p99, ms |
| --- | --- | --- |
| Light | 89.905 / 90.081 / 89.866 | 11.111292 |
| Dark | 89.918 / 90.062 / 89.873 | 11.111292 |

All six windows have covered boundaries, unique positive presentation timestamps
and no missing callbacks or recorder drops. The maximum interval is 22.222417 ms,
including missed refresh intervals. The observed moving windows last 5.584–5.597 s;
all five owners move and their sampled pose-change gaps remain below 20.1 ms.
All 91 thermal samples per theme are nominal, covering startup through the end
of each 90-second trace.

Pooled CPU owner-service p99 is 2.958 ms light and 2.782 ms dark. This is wall time
inside the frame's owner-queue block, excluding its earlier queue wait and later
presentation callback. GPU queue-span p99 is 2.447 ms light and 2.592 ms dark;
it includes gaps between submissions and is not isolated GPU busy time. These
overlapping measures cannot be added. Per-window and pooled attribution remain
separate in `motion-capability-pose-final-summary.json`.

`CAMetalLayer` presented timestamps establish canvas presentation only; native
overlay presentation and reference-iPad input/timing remain unqualified.
Raw traces, motion windows, placement witnesses, captures and source hashes are
under `artifacts/apple/tool-bubbles/`.

On 2026-10-18, a Release offscreen comparison on the M2 Pro Mac mini checked
the partial-opacity material correction against the previous contact-feature
gate. Both variants use the `cc6d1ab48` shared source, the same monotonic replay
clock and seed settlement, a 9504 × 6336 photo, 1600 × 1000 offscreen target,
1000 px brushes, opacity 0.4, and eight samples per update. Four setup runs and
three measured runs per variant and brush retain all input, artwork and exact
Undo/Redo assertions. Measured motion lasts 6.087–7.012 seconds per run.

| Brush | Median completed updates/s, before → after | Median per-run p99 update duration, before → after |
| --- | --- | --- |
| G-Pen | 174.37 → 195.30 | 8.236 → 8.008 ms |
| Eraser | 169.26 → 184.12 | 9.920 → 9.566 ms |

These are queue-completed offscreen updates, without native input or display
presentation. Thermal state was not sampled in this comparison. The results
establish no native frame rate or tier qualification.
Source, binaries, raw CSVs and the retained earlier failed runs are under
`artifacts/apple/port-oct13-regenerated/source-audit/partial-opacity-specialized-kernel-correction/M2Pro-proxy-preparation/monotonic-phase-clock-preparation/`.

A separate optimized M2 Pro comparison on 2026-10-18 batches watercolor
transport in compute passes instead of opening a render pass per page. Both
controls use shared source `9a2dba37f` plus the `75644d71a` reservoir
specialization, a 9504 × 6336 photo, a 1600 × 1000 offscreen target, Wet
Watercolor at 320 px and opacity 0.66, and eight samples per update. Two
900-frame setup runs precede three matched pairs of 225 moving updates,
lasting 7.049–7.819 seconds. All original assertions pass, and the native
painted-root records match byte for byte.

| Metric | Median before → after |
| --- | --- |
| Completed updates/s | 28.778 → 31.912 |
| Per-run completion p99 | 53.074 → 45.211 ms |
| Per-run CPU wall p99 | 17.786 → 12.497 ms |

All three pairs improve these metrics. These are queue-completed offscreen
updates, without native presentation or physical input; thermal state was not
sampled. The result does not qualify a tier or native frame rate. Source,
binaries and raw results remain under
`artifacts/apple/port-oct13-regenerated/source-audit/watercolor-transport-batched-compute-source-preparation/`.

A native comparison uses the preserved ordinary Release controls and
`75644d71a` plus this transport change, with unchanged Swift code. Each run
uses the synthetic 2K Wet Watercolor profile at 320 px, ten seconds of warm-up,
thirty seconds of measurement and ten seconds of postlude. GPU timing is off;
thermal state stays nominal. Both hosts actually display the dark theme;
light-theme verification of these builds remains unavailable.

| Host | Presented frames/s before → after | Cadence overruns before → after | Admission → presentation p99 before → after |
| --- | --- | --- | --- |
| M2 Pro Mac mini, 90 Hz | 87.399 → 86.999 | 53 → 56 | 32.953 → 32.955 ms |
| M4 iPad Pro, 120 Hz | 97.173 → 116.125 | 682 → 93 | 34.209 → 28.454 ms |

Both hosts retain zero render errors, rejected inputs and missing or zero-time
presentation callbacks. Mac cadence does not improve. The measured
receipt-to-first-associated-presentation proxy p99 increases on both hosts:
39.544 → 41.739 ms on Mac and 31.079 → 34.280 ms on iPad. For associated
receipts, receipt-to-admission p99 also increases, while the same frames'
admission-to-presentation p99 is essentially unchanged. Unmatched receipts remain
in the raw results, with
different association rates on iPad. These associations do not identify the
first displayed paint sample or establish physical input latency. The short
2K diagnostic does not qualify the sustained, full-canvas or native-chrome
targets. Signed products, captures and per-receipt components remain under
`artifacts/apple/port-oct13-regenerated/source-audit/current763-ordinary-Rust-archive-preparation/transport-compute-Release-refresh/`.

The same signed `7e3a522e4` transport candidates also complete one ten-minute
GPU0 watercolor run per host on 2026-10-18, with the same 2K/320 px profile and
dark appearance. Mac measures 600.001 seconds and 52,141 presentations; M4
measures 600.008 seconds and 69,577 presentations. Each has about 135,000 real
input samples, nominal thermal state throughout, and zero measured render
errors, rejected inputs, missing/zero-time callbacks, duplicate presentations or
dropped records. One startup zero-time callback per host remains outside the
measured interval.

Peak physical footprint is 1.641 GB on Mac and 1.452 GB on M4; measured
first-to-last growth is 687 MB and 408 MB respectively. Continued authored
painting and history are included; no memory plateau or leak conclusion follows.
This closes the ten-minute stability/accounting observation for this profile,
with full-canvas, native chrome, light appearance and physical latency still
unqualified. Traces and captures remain in `sustained600/` under the same
artifact directory.

The Apple `photo` workload uses the same 9504 × 6336 reference photo,
with an empty paint layer above it, explicit 512 px Wet Watercolor and package
sRGB U8 color. One six-second priming contact is settled and undone before
three six-second pressure-1 contacts, separated by one-second lifts. These are
synthetic owner inputs, with no supplied prediction.

On `1a8cc3b75` plus the consumed-paint trace and workload changes, all four
Mac/M4 light/dark observations complete the 4,320 measured samples, priming
Undo and three contact boundaries without rejected input or readiness loss.
M4 has no missing or zero-time measured callbacks in either theme. Its light
contacts measure 106.67/109.17/111.17 presentations/s, and dark contacts
102.31/103.31/105.82/s. Interval p99 is about 16.667 ms in both themes; these
miss the 114 presentations/s qualification floor.

Mac records one zero-time measured callback in each theme, on the first frame
after the third contact resumes. Both have normal CPU owner service and no
resize or readiness loss; the trace does not identify the compositor cause.
The light first contact separately has a 177.779 ms presentation gap. Mac's
90 Hz display cannot qualify the top-tier display floor. These results leave
presentation qualification open; they are not four passing qualification runs.

The new counter distinguishes engine-consumed real Down/Move timestamps from
receipts; associating that counter with a presentation still does not prove
fresh pixels or physical input latency. M4 captures show the photo, painted
layer and matching thumbnails in both themes. Mac's dark capture shows the
photo and paint but placeholder layer thumbnails after the ten-second postlude;
its failed light case has no retained screenshot. Evidence remains under
`reference-photo-Release-refresh/runtime/` and `runtime-Dark/` beside the
transport artifacts.

A further light-theme pair uses the shader specialization from `10d515055`
and Apple sources at `3d842c718`, with the same package, brush, priming Undo
and three contacts. Both complete all samples with zero measured render,
input, readiness, missing-callback or zero-time-callback failures. Mac contacts
measure 87.34/89.31/89.32 presentations/s on its 90 Hz display; M4 measures
106.48/109.82/108.99/s with interval p99 about 16.667 ms. The first-contact
maximum displayed gaps remain 177.779 ms and 141.670 ms. Maximum owner service
is 115.400 ms on Mac and 79.313 ms on M4, versus 118.292 and 76.227 ms in the
retained light controls. No material first-contact or pacing gain is established.
One zero-free Mac case does not establish that the earlier idle/resume issue is
fixed. Engine progress and presentation observations still do not qualify GPU
completed-update throughput. Current traces and both reviewed light captures
remain under `current10d-Release-refresh/runtime-Light/` beside those controls.

A further comparison replaces zero-only paint, coverage and wetness clears with
GPU copies from immutable zero pages. The working and scalar sources add
1.25 MiB, included in resident storage. On `194b011e7` plus this change, the
headless four-tile first-contact regression falls from 23 to 7 render/compute
passes. Five matched cases preserve all 380 captured raw color, wetness,
coverage and active-side files exactly through prediction, movement, cancel,
commit and restore. Both normal Apple Release apps build and pass signing and
ABI checks.

One light-theme observation per host uses the same photo workload and retained
`10d515055` controls described above:

| Device | Control contact presentations/s | Zero-page copies, contact presentations/s | First-contact maximum displayed gap, control → candidate |
| --- | --- | --- | --- |
| M2 Pro Mac mini, 90 Hz | 87.34 / 89.31 / 89.32 | 87.45 / 89.28 / 89.27 | 177.779 → 177.779 ms |
| M4 iPad Pro, 120 Hz | 106.48 / 109.82 / 108.99 | 111.17 / 113.34 / 113.17 | 141.670 → 141.670 ms |

The iPad completes all input, priming Undo and callback-accounting guards with
zero missing or zero-time callbacks; interval p99 remains about 16.667 ms.
Its contact rates still miss the 114 presentations/s floor. The Mac completes
the workload but fails the original guard with one zero-time callback on the
third contact's resume, retaining the earlier failure class. Neither host's
first-contact displayed gap improves. Reduced command count therefore does not
establish that the initial response stall is fixed. These presentation
observations do not qualify GPU-completed brush updates or physical latency.
Current traces and reviewed captures are in
`batched-page-clear-Release-refresh/runtime-Light/` beside those controls.

A further transport correction checks whether a missing neighbor actually has
native color backing or intersects an original image before splitting the batch
for borrowed cache views. A restored-empty four-tile regression falls from 12 to
3 transport passes. Six matched cases preserve all 508 raw color, wetness,
coverage and active-side files exactly. Both normal Apple Release apps build
and pass signing and ABI checks.

One light-theme pair on `c43539591` plus this correction uses the same photo,
brush, priming Undo and contacts:

| Device | Contact presentations/s, control → candidate | Maximum prepare, control → candidate | First-contact maximum displayed gap, control → candidate |
| --- | --- | --- | --- |
| M2 Pro Mac mini, 90 Hz | 87.453 / 89.280 / 89.265 → 87.332 / 89.497 / 89.336 | 113.218 → 100.849 ms | 177.779 → 166.668 ms |
| M4 iPad Pro, 120 Hz | 111.167 / 113.336 / 113.168 → 116.828 / 113.994 / 113.157 | 75.776 → 78.533 ms | 141.670 → 141.670 ms |

Both complete all input, priming Undo and callback-accounting guards with zero
missing or zero-time callbacks and nominal thermal state. Mac interval p99 is
11.111 ms; iPad contact p99 values are 8.334/16.667/16.667 ms. The iPad's
second and third contacts still miss 114 presentations/s. The Mac prepare
reduction does not close its first-contact response gap, and one zero-free run
does not establish that the earlier resumed-contact skip is fixed. GPU-completed
updates and physical latency remain unqualified. Traces and reviewed light
captures are in `empty-backed-neighbor-Release-refresh/runtime-Light/` beside
the preceding controls. These builds precede the Paper texture-dialog changes.

On 2026-10-18, one Mac light-theme case at `f147c48b7` includes the Paper dialog
integration and retained display-sampler bindings. The normal Release build uses
the same photo, 512 px Wet Watercolor, priming Undo and three contacts:

| Device | Contact presentations/s | Maximum prepare | First-contact maximum displayed gap |
| --- | --- | --- | --- |
| M2 Pro Mac mini, 90 Hz | 87.133 / 89.475 / 89.335 | 106.590 ms | 177.779 ms |

The measured interval is 19.991034 s, with all 4320 real samples and 1725
presentations. All input, readiness, Undo and callback-accounting guards pass;
missing, zero-time and duplicate callbacks are zero, with nominal thermal state.
Contact interval p99 is 11.111 ms. Maximum owner service is 106.807 ms, versus
101.061 ms in the preceding case; these single cases do not establish a gain.
The initial response gap remains open. This 90 Hz Mac observation does not
qualify top-tier cadence, GPU-completed brush updates or physical latency.
The current iPad build passes signing and minimum-OS inspection, but has no
corresponding runtime measurement. The Mac trace and reviewed capture are in
`latest-Apple-integrated-Release-preparation/runtime-photo20-Light-Mac/`.

The subsequent native thumbnail-wake change on `43c8c67a9` passes both normal
Release builds and four mounted thumbnail replacement cases. Its Mac photo run
fails the unchanged callback guard with one measured zero-time presentation and
zero missing callbacks. Contact rates are 87.122/89.493/89.300 presentations/s;
maximum prepare is 107.342 ms. The captured photo-layer previews remain blank.
A separate idle mounted check delivers the correct photo preview after its
three-second observation, within the original 45-second bound. This does not
close preview latency or the earlier presentation failure. Evidence is in
`latest-Apple-integrated-Release-preparation/runtime-thumbnail-photo20-Light-Mac/`
and `layer-thumbnail-photo-private-storage-successor/` beside that directory.

The same thumbnail-wake build on the M4 iPad Pro completes the photo workload:
20.0036 measured seconds, all 4320 samples and three contacts, with zero rejected
inputs, render errors, missing callbacks or zero-time callbacks in the measured
interval. Five zero-time callbacks outside that interval remain in the full
trace. Contact rates are 116.666/112.675/110.175 presentations/s; maximum prepare
is 74.070 ms and the first-contact maximum displayed gap is 141.670 ms. The
second and third contacts still miss 114 presentations/s. Both paint-layer
previews are visible in the reviewed light capture. Direct app launch avoids
XCTest, so this does not verify the pending interactive iPad journeys. Evidence
is in `M4-current-thumbnail-Release-photo-Light-current-install/` beside
`latest-Apple-integrated-Release-preparation/`.

The bounded thumbnail polling change on `39f880308` retains the four-page
budget and shared input priority, but uses the shared completion-delay hint
instead of pausing 120 ms between every idle batch. Both normal Release builds,
three focused Host GPU cases and four mounted replacement cases pass. On the
M2 Pro, the 61 MP photo preview is still absent at the three-second observation.
It arrives before the final all-images check in both themes. Elapsed time from document
activation to the all-images check is 3.943 s light and 3.849 s dark, including
the fixture's fixed three-second drain; these are upper bounds, not exact
arrival times. Both photo previews preserve every normalized Display P3 RGBA
byte. The mounted checks do not qualify UIKit or drawing cadence. Captures,
timings and exact comparisons are in
`latest-Apple-integrated-Release-preparation/layer-thumbnail-idle-throttle-verification/`.

The normal Mac Release capture now shows both the painted-ink and photo
thumbnails. Its unchanged workload guard still fails with one measured
zero-time presentation, zero missing callbacks, all 4320 samples and no input
rejections, frame errors or trace drops. The measured interval is
19.999975 s; contact rates are 87.190/89.459/88.998 presentations/s, maximum
prepare is 110.205 ms and the first-contact maximum displayed gap is
177.779 ms. These single cases do not establish a cadence or response gain.
The trace and capture are in
`latest-Apple-integrated-Release-preparation/runtime-thumbnail-throttle-photo20-Light-Mac/`.

The M4 iPad Pro's corresponding direct Release case passes its workload guards:
20.006619 measured seconds, all 4320 samples, priming Undo and three contacts,
with zero measured missing or zero-time callbacks, rejected inputs, frame errors
or trace drops. Four zero-time callbacks outside measurement remain recorded.
Contact rates are 116.327/117.174/115.328 presentations/s; maximum prepare is
78.687 ms, maximum owner service 78.860 ms and first-contact maximum displayed
gap 150.004 ms. Contact-three boundary coverage and instrumentation eligibility
remain false; these synthetic presentation rates do not qualify completed brush
updates, physical latency or the tier. The reviewed capture has correct ink and
photo previews; the photo's retained 32 × 32 window crop matches the control
exactly. Evidence is in
`M4-current-thumbnail-throttle-Release-photo-Light-current-Mac/` beside
`latest-Apple-integrated-Release-preparation/`.

A 2026-10-18 optimized headless comparison on the M2 Pro measures the
Painterly change in `52b8cd4bc`. One frozen Release executable replaces the
linked filter module with either the preceding shader or the current shader.
Both use the 9504 × 6336 photo, a 1600 × 1000 output, Strength 100 and alternating
Radius 5/5.5. Three processes per variant run in before/after, after/before,
before/after order. Rows 0–3 warm each process; matching rows 4–23 supply the
20 measured samples. Extra rows from the first three processes remain recorded.

| Matched pair | Previous completion median, ms | Current completion median, ms | Change |
| --- | ---: | ---: | ---: |
| 1 | 896.076 | 912.774 | +1.86% |
| 2 | 896.413 | 906.521 | +1.13% |
| 3 | 895.724 | 915.462 | +2.20% |

Every parameter sample evaluates 60,217,344 filter pixels through 20 effect
passes and 21 image-window submissions; cached frames perform no effect work.
Source-miss and command-pass ranges vary slightly in the first two pairs and
match in the third. These serialized completion costs include source paging
and composition, so they do not isolate shader execution or measure native
control-preview latency. Retained thermal samples report no warning; one
post-run sample and temperatures are unavailable. These small samples establish
neither presented frame rate nor a reference-tier pass. Raw rows, source hashes,
cold results and limitations are in
`artifacts/apple/port-oct13-regenerated/source-audit/painterly-continuous-weighting-proposal/optimized-cost-control/upstream52b8-controls/run1/`.

A matched normal Release comparison on 2026-10-18 measures the cold watercolor
source-reduction change on the same 61 MP photo workload. The control contains
`956dff895`; the candidate changes only the shared renderer's material-scratch
selection. Aligned pages outside the existing wet-page neighborhood reduce
their ordinary color inputs directly. The new 2048² first-contact regression
fails at 105 command passes before the change and passes the unchanged 64-pass
bound for both first contact and the contact after Undo, with committed pixels
checked against the exact renderer and unchanged distant photo pixels.

On the M2 Pro Mac mini's 90 Hz display, first-contact CPU preparation falls from
96.681 to 7.412 ms. The largest positive presentation interval in that contact
falls from 166.668 to 22.222 ms. The first admitted contact frame presents
180.901 ms after Down in the control and 54.967 ms after Down in the candidate;
that association does not prove physical input-to-pixel latency. Both complete
all 4320 samples and three contacts without input rejection or rendering errors.
Both retain their original strict workload failure: the first drawable of the
third contact receives a zero presentation timestamp. No callback is discarded,
no guard is relaxed and neither run qualifies a frame-rate target. The actual
traces, reviewed original captures and closed-process proofs are in
`latest-Apple-integrated-Release-preparation/source-reduction-normal-pair-summary.json`.

The matching M4 iPad Pro normal Release pair passes both original workload
guards. First-contact CPU preparation falls from 75.809 to 4.592 ms and its
marker-window presentation gap from 141.670 to 25.000 ms. First-contact
presentation rate changes from 116.531 to 119.500/s. Later contact rates change
from 114.514/114.998 to 112.834/112.823/s; their boundary coverage remains
incomplete, so neither pair qualifies completed brush throughput or the tier.
Both retain all 4320 samples, zero missing or zero-time measured callbacks and
the unchanged reference photo. Original captures show the photo, painted ink
and populated layer thumbnails. Matched phase analysis and limitations are in
`M4-source-reduction-Release-photo-Light-matched-pair/matched-contact-phase-analysis.json`
beside `latest-Apple-integrated-Release-preparation/`.

Normal Release checks after the unit-opacity pixel-copy correction and transform
integration through `175b8917e` pass the original guards on both Apple devices:
4320 samples, three contacts, priming Undo, and zero measured missing or zero-time
presentation callbacks, input rejections or rendering errors.

| Device | Previous first-contact prepare, ms | Current, ms | Previous associated Down-to-present, ms | Current, ms |
| --- | ---: | ---: | ---: | ---: |
| M2 Pro Mac mini | 7.412 | 7.581 | 54.967 | 53.850 |
| M4 iPad Pro | 4.592 | 5.367 | — | — |

The M4's first-contact maximum presentation interval remains about 25 ms and its
presentation rate changes from 119.500 to 119.667/s. These single comparisons
preserve the watercolor preparation improvement; they do not establish a causal
repair for older Mac zero-time callbacks, completed brush-update cadence, physical
latency or a reference-tier target. Current evidence is in
`latest-Apple-integrated-Release-preparation/runtime-current175b-opacity-GPU0-photo20-Light-Mac/`
and `M4-current175b-Opacity-Release-photo-Light/` beside it.

The counter observation following `9fefe503e` measures the same normal Release
photo workload on 2026-10-18 with GPU queries disabled. The readonly renderer
counter distinguishes nonempty canvas submissions from viewport-only frames;
each original advancing frame is joined to its positive presentation callback.
Three source Down/Up intervals contribute 17.9875 active seconds, including
contact-start delays. Presentation supplies an upper bound on completion time,
so the rates below are conservative lower bounds.

| Device | Completed nonempty updates/s | Fresh-associated updates/s | Fresh completion-gap p99, ms | First fresh completion upper bounds by contact, ms |
| --- | ---: | ---: | ---: | --- |
| M2 Pro Mac mini, 90 Hz | 89.284 | 89.229 | 11.111 | 61.375 / 61.430 / 50.375 |
| M4 iPad Pro, 120 Hz | 117.582 | 117.470 | 16.667 | 41.084 / 45.319 / 45.371 |

Both captures retain all 4320 samples, priming Undo and three complete source
contacts, with no measured missing or zero-time callbacks, rejected input,
render errors or dropped records. Nine Mac and six iPad submissions complete
outside their contact windows and receive no completion credit. The iPad's third
contact retains incomplete presentation-edge coverage; it is not a display-rate
qualification. Generic canvas counts may include refinement; fresh-associated
counts require newly consumed real input but do not prove its pixels are shown.
Neither diagnostic qualifies exact GPU timing, physical Pencil latency, sustained
acceptance or the reference tier.

The original collectors failed while importing the analyzer's new helper.
Their failures remain recorded; the same captures were analyzed offline without
another app run. A setup renderer replacement before priming supplies a new
pre-contact counter baseline; a reset during the measured cohort remains invalid.
The normal Mac/device archives are `d17e02f1`/`2c267fb5`. Evidence is in
`latest-Apple-integrated-Release-preparation/runtime-CanvasCounter-GPU0-photo20-Light-Mac/Mac-photo-light-candidate/finite-offline-qualification.json`
and `M4-CanvasCounter-Release-photo-Light/CanvasCounter-photo20-Light-offline-recovery-summary.json`
beside `latest-Apple-integrated-Release-preparation/`.

Separate native drawing diagnostics on 2026-10-18 use the ordinary `ink` and
`wet-watercolor` workload profiles, with ten seconds of warm-up, about thirty
seconds of measurement and ten seconds of postlude. These private optimized
apps contain the integration patches on the `8a819ffa1` Rust base and final
`cc6d1ab48` Swift; they do not cover the later built-in Paper artwork changes.
The 2048 × 2048 synthetic documents use a 24 px G-Pen or 320 px Wet Watercolor.
Metal presentation callbacks are enabled and GPU timestamp queries disabled.

| Host and profile | Measured seconds | Presented drawables | Active interval p99 | Intervals over cadence tolerance |
| --- | --- | --- | --- | --- |
| M2 Pro Mac, 90 Hz, ink | 30.019 | 2593 | 22.222 ms | 34 |
| M2 Pro Mac, 90 Hz, wet-watercolor | 29.9973 | 2617 | 22.222 ms | 57 |
| M4 iPad Pro, 120 Hz, ink | 30.008 | 3503 | 12.500 ms | 46 |
| M4 iPad Pro, 120 Hz, wet-watercolor | 30.0084 | 3251 | 16.667 ms | 342 |

Thermal state remains nominal. Measured intervals contain no render errors,
rejected input batches, missing presentation callbacks or zero-time callbacks.
Startup accounting is separate; the Mac ink trace has two startup zero-time
presentations. The Mac watercolor trace retains the analyzer's short-duration
warning. These short synthetic runs do not qualify the reference canvas, brush
sizes, sustained drawing or physical input latency. Native chrome motion is
unmeasured and cadence overruns remain. No target is reported as met. Raw
traces, exact source and binary identities, initial blocked
attempts and analyzer reports are in
`artifacts/apple/port-oct13-regenerated/source-audit/native-Release-workload-presentation-diagnostics/`.

The stamp-preview check on 2026-10-17 used an optimized build, a 9504 × 6336
sRGB 8-bit photo and a 1200 × 870 logical viewport at 2× scale. Recognizing wrapped
brush settings as metadata removes their wait behind unrelated raster backing.
One priming and three measured six-second contacts retain the original 250 ms
continuity bound, exact artwork, authored resources and Undo/Redo checks. During
73 brush-unready samples, the diameter changed through 59 values. All supplied
samples arrived in order. This measures control-state continuity; presentation
FPS, motion p99 and thermal coverage remain unmeasured. Raw before/after results
are in `artifacts/apple/port-oct13-regenerated/source-audit/current-surface-and-optimized-motion-ready/stamp-delivery-diagnostic/full7-run1`
and the sibling `stamp-classifier-fixed/full7-run1`.

### Apple HDR color bubble diagnostic

The 2026-10-24 follow-up uses the same non-reference M2 Pro Mac mini (16 GiB,
macOS 26.6.2), 90 Hz display and 1200 × 870-point editor at 2×. A real
9504 × 6336 F16 drawing contains the imported reference photo and an empty
selected paint layer at Fit. The 216-point HDR color bubble moves through four
neighbors with default glass. Its paint pair and palette are populated: 17
palette swatches plus foreground, background, transparent and palette controls.
Recent colors are enabled but have zero beads. Actual viewing headroom is 1 and
`hdr_output` is false; this does not measure physical HDR output.

Four sequential Release runs cover drag and intensity in both themes. Each has
one prime and three six-second native mouse contacts. Timestamped owner or
published-marker movement defines each window with 100 ms edges; no FPS-based
trimming is used. Drag restores the exact workspace with Undo; intensity leaves
workspace and artwork unchanged. All workload assertions pass.

| Motion / theme | Metal canvas presents/s, three windows | Interval p99, ms | Pooled CPU owner p99, ms | Pooled GPU queue-span p99, ms |
| --- | --- | --- | --- | --- |
| Drag, light | 89.919 / 89.592 / 89.398 | ≤11.111331 | 0.526 | 0.448 |
| Drag, dark | 89.938 / 89.611 / 89.922 | 11.111292 | 0.493 | 0.552 |
| Intensity, light and dark | Unmeasured: no drawable presentations | Unavailable | Polls only; not interaction cost | No GPU spans |

All six drag windows are collector-eligible, with unique positive presentation
timestamps, covered boundaries, and no zero-time, missing or duplicate callbacks.
Their duration is 5.694–5.705 s; the maximum presentation interval is 22.222500 ms.
All five owners move; sampled pose-change gaps remain below 16.1 ms. CPU owner
service is wall time inside the frame block, excluding queue wait, action/UI work
and later callbacks. GPU queue spans include submission gaps, not isolated busy
time; these overlapping measures cannot be added.

Intensity changes the cached native UI without acquiring Metal canvas drawables,
so those six windows cannot establish presentation FPS. Light contacts visit
170 / 180 / 186 distinct published EV values; dark visits 174 / 178 / 185.
Observed changed-marker samples occur at 31.753 / 32.392 / 32.898 per second
light and 31.848 / 32.701 / 32.932 dark, with maximum observation gaps
52.471–59.783 ms. These are model observations, not fresh displayed-marker frames.
Their windows last 5.669–5.739 s. Low owner poll times do not measure interaction
cost or SwiftUI/main-thread work.

All four traces retain 90 seconds and cover their final moving window (ending
43.668–47.985 s after trace start). Each has 91 nominal thermal samples from
startup through trace end, with no recorder drops. Native overlay presentation,
physical input latency, physical HDR, Pencil and reference-iPad timing remain
unqualified; no tier pass follows from these canvas diagnostics.

Measured inputs are `c9c46f8ab` plus the Apple HDR port, compiled-input digest
`646b2eaa5b3fd348807be94c69235ee1c78d3163f842bc93cc4ec297c629e145`;
Release executable SHA-256 is
`6a3e2017d6dc4e6d9d53e160399c50faa517cacfdd468927f1125d761ed192ed`.
This identifies the measured build, not a later rebased HEAD. Raw traces,
`setup.json`, timestamps, screenshots, per-window reports, source/binary manifests
and `motion-summary.json` remain in `artifacts/apple/hdr-tool-bubbles/`.

## Windows Domain Warp comparison

On 2026-10-15, the Surface Laptop 5, i7-1255U and Intel Iris Xe driver
32.0.101.7085 compare the original Domain Warp with explicit fused multiply-add
for phase, noise feedback and displacement. Both use the `f6871aab0` working tree,
Release Rust 1.98.1 and Naga 30.0.1. The diagnostic source manifest is
`C5703F14F38DDE5BB97CF07BFEEF88F0B1540E08932080FA02F0D4531932B320`.
This laptop is not the reference tablet.

The matched workload uses F32 source images, three octaves, scale 96, distance 24
and phase 3.125. Each renderer warms twice, then evaluates three changed source
images with alternating original/candidate order. Work counters match for every
pair. Values are medians of three GPU durations in milliseconds.

| Backend | Extent | Original | Explicit FMA |
| --- | --- | --- | --- |
| D3D12 | 384 × 256 | 1.009 | 1.036 |
| D3D12 | 1600 × 1000 | 27.610 | 21.258 |
| Vulkan | 384 × 256 | 1.369 | 1.345 |
| Vulkan | 1600 × 1000 | 24.652 | 21.361 |

The large-image samples range from 21.786–28.674 to 15.310–26.513 ms on D3D12,
and from 17.917–26.090 to 18.625–25.541 ms on Vulkan. These short samples do not
establish a sustained speedup, displayed frame rate or input latency. They show
no consistent increase in the cost of the corrected arithmetic. Reference-tier
motion remains unmeasured.

The fixed 32 × 24 saved Domain image passes all 3072 channels on both backends
without changing the baseline or tolerances. Maximum error falls from
0.000232697 to 0.000104904 on D3D12 and 0.000236988 to 0.000102520 on Vulkan;
the limit is 0.0002. Procedural FBM sources and rendered pixels remain unchanged
against their own controls. Raw logs and all matched counters are in
`artifacts/windows/port-20261010/post188-fma-guard-and-resolution-preparation-bf342fa0d8fe40d6b393697bda9e36d2/actual-67ac6b6355224e608a3968556c7fee98/`.

### Windows angular filter accuracy observations

On 2026-10-15, Surface Laptop 5 / Intel Iris Xe driver 32.0.101.7085,
Rust 1.98.1 and Naga 30.0.1, the corrected shared axis and Cartesian Kaleidoscope fold pass all 16
fixed U8/F32 angular cases on D3D12 and Vulkan. Original arithmetic fails three
cases on D3D12 and two on Vulkan. Baselines and tolerances are unchanged; the
full saved-artwork contract remains required and its Painterly failure is open.

The diagnostic source was `B6E74B1A` at commit `1c246f3d`, targeting upstream
`37fabf99`. Each filter used two warm frames followed by three alternating
original/corrected changed-source pairs at 384 × 256 and 1600 × 1000, with
matching work counters and a positive effect-pass count. The 96 observations
per backend are short headless samples, not sustained motion, presentation,
input latency or reference-tier qualification. Large-image GPU medians (ms):

| Filter | D3D12 original | D3D12 corrected | Vulkan original | Vulkan corrected |
| --- | ---: | ---: | ---: | ---: |
| Kaleidoscope | 23.473 | 21.056 | 21.560 | 23.291 |
| Swirl | 23.194 | 20.024 | 20.313 | 22.094 |
| Motion blur | 29.704 | 23.665 | 25.736 | 25.585 |
| Halftone | 22.910 | 20.933 | 21.849 | 22.522 |
| Crosshatch | 8.985 | 8.601 | 8.531 | 8.705 |
| Emboss | 22.759 | 21.203 | 22.633 | 18.809 |
| Chromatic aberration | 22.684 | 22.087 | 20.699 | 19.236 |
| Gradient fill | 16.413 | 13.701 | 13.919 | 14.004 |

Vulkan Swirl increases consistently by 1.056–1.156× in the large-image paired
samples (median 20.313 → 22.094 ms). Individual paired outliers reach 3.272×
at the small extent and 1.531× at the large extent. These observations support
no general speedup or no-regression claim; affected reference-tier motion
qualification remains open.

Raw logs are in
`artifacts/windows/port-20261010/post37-angular-eight-red-control-b301fd0b59614cb5b9ab4c4849e5bd5e/actual-5298332983894c02a39fc96eae9afb80/`.
The complete 32-comparison table and original/corrected samples are retained
alongside it as `angular-eight-actual-cost-summary-v2.md` and
`angular-eight-actual-cost-summary.json` in the proposal directory.


### Windows hue-coordinate accuracy comparison

On 2026-10-16, the same Surface Laptop 5, i7-1255U and Intel Iris Xe driver
32.0.101.7085 ran Release Rust 1.98.1/Naga 30.0.1 source `4B86F4F0` at
`b0fc6c6c`, targeting upstream `86a0fd0e`. The bounded hue-angle polynomial
passes all nine physical-color tests, four Selective Color tests, exact filter
endpoint tests and four fixed Hue/Selective U8/F32 render rows on D3D12 and
Vulkan. The full saved-artwork contract remains required; Painterly is still open.

The matched programs differ only in the hue-angle call: original native
`atan2` versus the corrected polynomial. Both keep the same parameters,
spatial reference, exact endpoint behavior and rendering work. Non-neutral Hue
and Selective Color recipes use pointwise and zero-radius image evaluation,
two warm frames and three changed-source pairs in AB/BA/AB order at each size.
All 48 work-counter pairs match; all 96 CPU, complete and GPU records are finite
and positive. GPU medians and individual paired candidate/original ratios:

| Backend | Effect/path | Size | GPU original to candidate ms | Three paired ratios |
|---|---|---|---|---|
| Dx12 | hue_saturation/pointwise | 384x256 | 0.490 to 0.560 | 1.232/1.241/1.140 |
| Dx12 | hue_saturation/pointwise | 1600x1000 | 13.541 to 11.182 | 0.726/1.378/0.826 |
| Dx12 | hue_saturation/image | 384x256 | 0.973 to 0.929 | 1.028/0.955/1.044 |
| Dx12 | hue_saturation/image | 1600x1000 | 23.712 to 22.258 | 0.812/1.211/0.659 |
| Dx12 | selective_color/pointwise | 384x256 | 0.423 to 0.469 | 1.114/1.062/1.122 |
| Dx12 | selective_color/pointwise | 1600x1000 | 12.358 to 10.396 | 0.845/1.169/0.827 |
| Dx12 | selective_color/image | 384x256 | 0.800 to 0.929 | 1.207/0.959/1.018 |
| Dx12 | selective_color/image | 1600x1000 | 23.694 to 21.711 | 0.916/1.157/0.614 |
| Vulkan | hue_saturation/pointwise | 384x256 | 0.818 to 0.799 | 0.964/1.027/0.941 |
| Vulkan | hue_saturation/pointwise | 1600x1000 | 11.644 to 11.498 | 1.017/0.987/0.892 |
| Vulkan | hue_saturation/image | 384x256 | 1.268 to 1.211 | 1.103/0.911/0.812 |
| Vulkan | hue_saturation/image | 1600x1000 | 22.265 to 24.721 | 1.110/0.906/1.136 |
| Vulkan | selective_color/pointwise | 384x256 | 0.789 to 0.789 | 1.001/1.021/0.958 |
| Vulkan | selective_color/pointwise | 1600x1000 | 11.402 to 11.376 | 1.009/0.829/0.612 |
| Vulkan | selective_color/image | 384x256 | 1.167 to 1.212 | 1.024/0.971/1.040 |
| Vulkan | selective_color/image | 1600x1000 | 16.162 to 21.947 | 1.374/1.301/1.009 |

Large D3D12 pointwise medians improve, while small D3D12 pointwise samples and
Vulkan image evaluation regress. Large Vulkan Selective Color image ratios
are 1.374, 1.301 and 1.009; the 16.162 to 21.947 ms medians do not support a
no-regression claim. Keep the individual samples and outliers when comparing
future changes. These short headless frames measure rendering work, not
sustained motion, presentation, physical pen latency or reference-tier targets.

Raw logs, paired CPU/complete/GPU records and matching work counters are in
`artifacts/windows/port-20261010/hue-selective-both-paths-cost-unapplied-2280d4a64c38412aae6c7c5af6a32447/actual-28100bdd80154a5c9374c334e608330a/`,
in `results.json` and `paired-cost-summary.json`.

### Windows current-build input comparison, 2026-10-17

The Surface Laptop 5, i7-1255U and Intel Iris Xe D3D12 driver
32.0.101.7085 compare qualified source `0E13A14FD1A5` with
`44207B3D00F4`, based on `origin/main` revision `6307b53623cd`.
The display is 2256 x 1504 at 60 Hz, density 1.5, Balanced power.
These builds include upstream and local changes; this comparison does not
isolate one fix. The original plain 8192 x 6336 project, 18 px G-Pen,
Immediate presentation and maximum frame latency 1 are unchanged.
Pen canvas bounds are 1418 x 988; navigation uses 2256 x 1432.

| Contact | Previous input-to-frame-return p99 (ms) | Current p99 (ms) | Current maximum (ms) |
| --- | ---: | ---: | ---: |
| Pen 1 | 206.665 | 21.956 | 127.979 |
| Pen 2 | 26.694 | 26.359 | 134.032 |
| Pen 3 | 49.969 | 19.437 | 121.219 |
| Hand | 19.258 | 18.029 | 49.168 |
| Zoom | 18.262 | 15.005 | 52.621 |
| Rotate view | 18.589 | 19.566 | 49.647 |

Each unprimed pen contact requests ten seconds at nominal 240 Hz, after the
original three-second settle. Navigation retains one excluded priming
contact and three six-second contacts, resetting view between contacts.
The navigation rows pool input latency across their measured contacts;
these are not averages of contact frame rates. All 20,895 injected inputs
were consumed with zero unmatched records and no trace overflow.

Every pen input-to-frame-return sample above 16.667 ms occurs in the first
132.8081 ms of its contact. The first host-frame render spans are
71.6399, 74.2565 and 70.4790 ms. The underlying blocking call remains
unidentified. The first-contact pause is unresolved, and navigation does
not show a uniform latency improvement.

The original reporter, input pacing, 180-second outer and 90-second
shutdown limits are unchanged. UI tracing is absent and SkipPresentMon
is retained. These software timestamps exclude physical digitizer and
panel response; they are not scanout latency, displayed fps or a
reference-tier qualification. All 24 navigation endpoint PNGs were viewed;
no missing tiles or error banners were observed. The blank document limits
content validation, and stills do not establish temporal continuity.

Raw captures: `artifacts/windows/port-20261010/performance/` under
`dynamic-pen-navigation-e9123645f3d4418ba15d2c419e91e24e/`.
Comparison and full source/runtime/ownership closure:
`n6307-six-closure-93587e8786b641a99c0a8770cf24bb79/`
(`closure-and-release.json`, `B2967F434089`). Image review:
`navigation-24-image-review-6307/review.json` (`5328D28B1504`).
Current EXE/DLL prefixes are `371C35B30BF7`/`AC8122F54893`;
normal Release qualification is `55F6B17F6D8E`.

### Windows Pencil input, 2026-10-18

The Surface Laptop 5, i7-1255U and Intel Iris Xe D3D12 driver
32.0.101.7085 ran the normal Release sources now committed in `509e4a1a9`,
including shader specialization `10d515055`. The build began at `26a1ec0a3`
with the same three frozen regression files. Executable/DLL SHA-256 prefixes
are `4E0AC4825C23`/`6DF248B8DB3B`; DXC is `9A5100511E12`.

The existing plain 8192 × 6336 project, 4 px Pencil, Immediate presentation,
maximum frame latency 1 and private display are unchanged. One process runs a
one-second priming contact, then three nominal six-second contacts at 240 Hz.
Each contact is undone to the original drawing before the next. Actual measured
input rates are 215.8–216.3 Hz; all 4,564 inputs, including priming, are consumed
with zero unmatched records. Artwork, camera, controls and Undo checks pass.

| Measured contact | Submitted frames/s | Input-to-frame-return p99, ms | Pen-down to frame return, ms |
| --- | ---: | ---: | ---: |
| 1 | 205.98 | 52.48 | 83.28 |
| 2 | 206.12 | 57.96 | 83.66 |
| 3 | 207.49 | 49.02 | 82.07 |

The three contacts retain every initial moving sample. Pooled input-to-frame
return is 3.70 ms median, 9.85 ms p95, 57.09 ms p99 and 113.24 ms maximum.
Submitted-frame intervals have p99 7.93 ms and maximum 49.34 ms. The first
native render spans are 66.38–66.87 ms, followed by acquisition waits of
26.96–36.37 ms. The trace does not identify the blocking operation. These
contact-start pauses remain unresolved; shader specialization does not by itself
establish that native input stutter is fixed.

These are software timestamps and submitted frames, not presented fps, physical
Wacom latency or a reference-tier qualification. No PresentMon or per-frame UI
tracing is enabled. The known wrapper log-sentinel failure is preserved;
independent process/desktop closure precedes the unchanged reporter on the same
capture. Raw data and summary are in
`artifacts/windows/port-20261010/pencil-final-prepared-a3f7d2e9/`.

### Windows Pencil input after native activation, 2026-10-18

The Surface Laptop 5, i7-1255U and Intel Iris Xe D3D12 driver
32.0.101.7085 ran the normal production Release at `2ece7f68c`, including
native pen/touch activation `d025bd38f`. Executable/DLL SHA-256 prefixes are
`5D145DBDB319`/`896DA9DFFD20`; DXC is `9A5100511E12`. The official Rust and
C++ build passes, with 48 native objects rebuilt in a private intermediate
directory.

The plain 8192 × 6336 drawing, 4 px Pencil, Immediate presentation and maximum
frame latency 1 match the previous normal Release at `ba0f45ea2`
(`40AA47DCACA3`/`E57288CF9ED3`). One priming contact is undone before three
nominal six-second contacts requested at 240 Hz. Actual input is
213.21–217.43 Hz, compared with 209.54–209.63 Hz previously. All 4,564 inputs
are consumed, with zero unmatched records or statistics errors. Artwork,
camera, controls and all four Undo restorations pass. No compiler, debugger,
PresentMon or UI tracing runs during capture. The motion injector and reporter
are unchanged.

| Contact | Previous input-to-frame-return p99, ms | Current p99, ms | Previous pen-down to frame return, ms | Current pen-down, ms | Current submitted frames/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 25.54 | 5.90 | 34.05 | 29.88 | 217.25 |
| 2 | 36.61 | 6.31 | 72.87 | 38.76 | 217.51 |
| 3 | 24.70 | 6.18 | 34.20 | 23.01 | 213.31 |

Pooled input-to-frame return changes from 5.77 to 2.34 ms median,
10.62 to 3.07 ms p95, 30.32 to 6.22 ms p99 and 75.03 to 38.76 ms maximum.
Current submitted-frame intervals have p99 5.84 ms and maximum 7.79 ms.
Native frame work has p99 2.30 ms and maximum 20.93 ms; acquisition waits reach
3.64 ms. This comparison measures the integrated builds and does not isolate
one change. Contact starts still reach 38.76 ms after priming; first-use shader
compilation and physical pen latency are not qualified by this capture.

The original reporter, process and private-display closure pass, and runtime
hashes remain unchanged. These are software timestamps and renderer submissions;
they do not establish physical Wacom latency, displayed fps, long-running
stability or reference-tier targets. Current raw traces and summary are in
`artifacts/windows/port-20261010/pencil-2ece-final-7f14/`; the previous normal
capture is in `pencil-finite-final-prepared-1ed6ee73/` beside it.

### Windows Painterly control diagnostic, 2026-10-18

On the same Intel Iris Xe laptop, a Debug headless capture at `082083b64`
plus the Painterly candidate compares the previous shader with weighted central
variance and continuous quadrant blending. The executable SHA-256 begins
`DAFA74BD9C48`. Both variants use the same renderer, 1920 × 1080 F32 source,
one warm-up and 20 actual Strength changes between 80 and 100 per radius.

| Radius | Previous GPU median, ms | Candidate GPU median, ms | Previous completion p95, ms | Candidate completion p95, ms |
| --- | ---: | ---: | ---: | ---: |
| 5 | 27.93 | 41.27 | 37.78 | 54.07 |
| 16, maximum | 25.67 | 35.04 | 35.63 | 45.67 |

Candidate completion maxima are 77.19 and 67.20 ms. Alpha is unchanged;
independent GPU tests cover signed HDR, transparent samples, edges and near-tie
continuity. The appearance review accepts the continuous blend while preserving
broad color and edge structure. The two Painterly reference rows change with
that behavior; other saved-render rows and tolerances remain unchanged.

This diagnostic measures completion after a parameter change, including waiting
in the test runner. It is not a native presented-control measurement or a Release
performance qualification. It does not establish live painted-ink age, settling,
resumed drawing, or a reference-tier Slow-filter exception. Raw images, exact
fixed-fixture output and timing rows are retained in
`artifacts/windows/port-20261010/painterly-1080-control-504f8293287248668e1f602181da537e/`.

### Windows ARM64 brush completion, 2026-10-18

Surface Pro 11, Snapdragon X Plus X1P64100, 16 GB and Adreno X1-85 D3D12
driver 31.0.137.0 ran the native ARM64 Release example on Windows 11 build
26200, on AC power. Rust sources are `aaa7a8273`, unchanged through
`e69325d02`. The `brush_frames.exe` SHA-256 begins `460C39ECCF73` and the
bundled native DXC 1.9.2609.5 begins `EC234595C270`.

The synthetic 9504 × 6336 Paint/Paper canvas uses a 1600 × 1000 viewport,
1000 document-pixel brushes, pressure 1, upright tilt and two samples per
frame. Each preset has 64 warm-up frames followed by three moving contacts
of 6.48–7.17 seconds. Each frame waits for GPU completion in the offscreen
runner. Rates include every moving frame after warm-up, including contact
starts and tails; separate pen-up rows are excluded from moving statistics.
The completion p99 is the nearest-rank percentile within each contact.

| Brush | Median completed updates/s | Contact rate range, updates/s | Completion p99 range, ms | Pen-up completion range, ms |
| --- | ---: | ---: | ---: | ---: |
| G-Pen | 250.22 | 246.30–262.43 | 5.36–5.87 | 3.48–4.46 |
| Pencil | 304.77 | 302.74–308.86 | 4.78–5.01 | 2.73–2.81 |
| Eraser | 223.65 | 221.89–225.11 | 6.51–6.89 | 3.58–4.06 |
| Paintbrush | 156.36 | 154.64–156.53 | 9.77–10.14 | 4.65–5.82 |
| Airbrush | 240.25 | 235.36–245.21 | 6.02–6.27 | 3.36–4.25 |
| Marker | 158.11 | 153.46–158.23 | 9.25–9.81 | 4.73–6.02 |
| Shading Pencil | 385.24 | 385.14–390.90 | 3.90–3.96 | 2.01–2.77 |
| Bristle Paintbrush | 158.27 | 157.16–159.75 | 8.57–8.94 | 4.84–5.86 |

All eight native processes exit 0. Exact Undo/Redo restoration passes for all
24 contacts; 38,184 raw rows, including 24 pen-up rows, and eight captures are
retained. No compiler or other test workload runs during these contacts.
The calibration observes the companion native DXC module at 2.33 seconds;
compiler provenance for the sustained processes is inferred from unchanged
executable, DXC, PATH and Auto compiler configuration. Sustained contacts
have no module polling. The calibration wrapper loses its exit code; only
the sustained runs with retained process handles establish native exit 0.

These CPU-observed GPU completion rates do not establish native displayed FPS,
physical pen latency or reference-tier targets. The hardware, synthetic canvas
and upright brush pose differ from the reference workload. Raw CSV, process
closure, captures and summary are in
`artifacts/windows/arm64-validation/performance/sustained-30a9a7b61cea48e2a7ee70f974f5d54e/`;
the compiler observation is in `offscreen-2ef84b0ee012415baf1378f954d4725b/`
beside it.

### Windows ARM64 native pen input, 2026-10-18

The same Surface Pro 11 runs the ordinary native ARM64 Release app from
`098d05435`, with the Rust renderer sources above. EXE/DLL SHA-256 prefixes
are `1D238E16871C`/`821AB2FF00D1`. The synthetic 9504 × 6336 source image
has an empty active paint layer; project SHA-256 begins `C791D1738B8F`.
The maximized canvas viewport is 2880 × 1824 physical pixels at 200% scaling,
with a 16% fitted camera, Immediate presentation and maximum frame latency 1.
A composed preflight capture verifies the entire 400 × 240 pixel pen ellipse
lies inside the artwork and clear of panels.

Each brush runs in a separate process with one one-second priming contact
and three nominal six-second contacts. G-Pen uses 18 px and Pencil uses 4 px.
Actual contacts last 6.44–6.57 seconds at 219.18–223.56 injected samples/s.
All 4,564 inputs per process, including priming, are
consumed with zero unmatched records or overflow. Each contact changes the
native document title and enables Undo; Undo returns to the clean drawing,
unchanged camera and unchanged arranged canvas. The app exits 0.

| Brush and contact | Renderer submissions/s | Submitted interval p99, ms | Input-to-frame-return p99, ms | Pen-down to frame return, ms |
| --- | ---: | ---: | ---: | ---: |
| G-Pen 1 | 173.42 | 13.14 | 14.78 | 21.17 |
| G-Pen 2 | 158.87 | 14.01 | 13.58 | 18.32 |
| G-Pen 3 | 161.88 | 13.31 | 14.31 | 18.87 |
| Pencil 1 | 171.69 | 13.71 | 14.64 | 9.91 |
| Pencil 2 | 156.02 | 14.40 | 16.44 | 9.62 |
| Pencil 3 | 171.28 | 13.25 | 14.46 | 9.66 |

Pooled G-Pen input-to-frame return is 5.23 ms median, 8.71 ms p95, 14.32 ms
p99 and 40.67 ms maximum. Pencil p99 is 15.57 ms and maximum is 35.66 ms.
All measured Down/Move/Up samples, initial samples and
tails remain included; priming is retained separately. No PresentMon,
per-frame UI tracing, compiler or other owned workload runs during capture.
These are renderer submissions and software timestamps, excluding physical
digitizer and panel response. They do not establish displayed FPS or a
reference-tier target; the reporter leaves both qualifications unset.
Raw traces, capture, geometry image and summary are in
`artifacts/windows/arm64-validation/performance/native-maximized-23ff405f7da8454490005ab6f1aef246/`
under `g-pen-18/` and `pencil-4/`.

## iPadOS 17 compatibility diagnostics

The 2026-10-17 older-device GPU-counter candidate is based on `86a0fd0e4`;
its iPad executable SHA-256 is
`e786b68a7bf083f4cb05138ec30395a0334c59dabef6def1fa5839ebd9000445`.
These supplemental Apple runs use the native `ink` producer on a 2048 × 2048
document, one paint layer, G-Pen at 24 px, ten seconds of warm-up and a
ten-second postlude. Input is synthetic; these are actual drawable presentation
rates, not fresh painted-update rates or physical Pencil latency.

| Device and measured interval | Actual presentations/s | Interval p99, ms | Valid GPU spans | Thermal states | Result |
| --- | --- | --- | --- | --- | --- |
| A10X iPad Pro 12.9-inch, second generation; iPadOS 17.7.11; 30 s | 31.44 | 183.38 | 2,369 | 0 | Completed with postlude; no restart |
| Same device and binary; 120 s repeat | 48.27 | 175.42 | 6,567 | 0, 1, 2 | Completed with postlude; no restart |

Both older-device runs enable paired GPU counter markers. They have zero
rejected input batches, frame errors, missing presentation callbacks, invalid
GPU samples and GPU polling errors. Full readback slots skip observations
without blocking drawing. Earlier one-sided compute markers triggered a GPU
page fault and device restart on this hardware; paired markers avoid that
observed failure in these runs. The longer run becomes thermally constrained.
Neither run qualifies a tier target, the required workload matrix or ten-minute
acceptance. Evidence is in
`artifacts/ipados17/old-release/{paired-counter,paired-repeat}-report.json`.

The completed compatibility Release executable has SHA-256
`90025051cf26b1f39b2d8ce57e8065720619b3fa505f57746119a88d46fb52c7`.
The same older iPad completes two 30-second ink windows with its final UI fixes:

| Final A10X run | GPU timing off | GPU timing on |
| --- | --- | --- |
| Measured duration, s | 30.134 | 30.091 |
| Actual presentations/s | 37.90 | 24.92 |
| Presentation interval p99, ms | 208.38 | 183.38 |
| Owner service p99, ms | 12.62 | 12.18 |
| Peak footprint, decimal MB | 437.7 | 442.2 |

Both complete the postlude with zero frame errors, rejected inputs, missing
callbacks and dropped trace records. The measured windows include 1,898 and
2,217 zero-time presentation callbacks, respectively; these do not count as
actual presentations. The GPU-timed window has 2,339 valid spans, zero invalid
samples or polling errors, and 841 skipped observations across its whole trace.
It reaches fair thermal state; the timing-disabled run stays nominal. This
sequential pair does not isolate profiling overhead, and neither meets the
120 Hz target. Ordinary launches leave GPU profiling off. Evidence is in
`artifacts/ipados17/final-release/summary.json`.

The measured iPadOS 17.5-minimum compatibility Release executable has SHA-256
`25df098c40eaae567d31627148a7b3f4f9e44f42c8f1d50d2796e4c78d4959ab`.
On the M4 iPad Pro 13-inch with iPadOS 27.0, a baseline/candidate/candidate/baseline
comparison disables GPU timing and uses the same 30-second ink measurement,
2048 × 2048 document, 24 px brush, warm-up and postlude. The baseline is
`86a0fd0e4`, executable SHA-256
`cf0694ccf81e98fffbcbe3cab71d9780e006562f60828eeb4ddf75469f4f1f27`.

| M4, GPU timing disabled | Baseline range | Candidate range |
| --- | --- | --- |
| Actual presentations/s | 116.14–116.21 | 116.01–116.27 |
| Presentation interval p95, ms | 8.334 | 8.334 |
| Presentation interval p99, ms | 8.33–12.50 | 8.33–13.58 |
| Owner service p95, ms | 2.263–2.425 | 2.273–2.338 |
| Owner service p99, ms | 2.669–2.676 | 3.273–4.396 |
| Peak footprint, decimal MB | 539–542 | 538–539 |

Cadence, owner p95 and footprint overlap; owner p99 is higher in both candidate
windows. Nearly all owner-service frames above 8.33 ms occur within 150 ms of
pen-up, with long acquisition or work outside the recorded Rust stage spans.
The trace does not identify the cause. These runs do not establish unchanged
latency tails. All four complete without rejected input, frame errors or missing
presentation callbacks, with nominal thermal state.

A candidate run with GPU timing enabled presents at 116.00/s, with presentation
interval p99 8.338 ms, owner-service p95 2.761 ms and GPU queue-span p95
2.581 ms. Owner p95 is 18–21% above the candidate GPU-timing-disabled runs. The
paired counter instrumentation therefore has measurable overhead; its queue
spans must not be treated as uninstrumented GPU busy time. It has zero invalid
GPU samples, frame errors, rejected input and missing presentation callbacks.
Evidence is in `artifacts/ipados17/comparison/gpu0-final/`. These supplemental
measurements do not qualify the workload matrix, physical Pencil latency or
ten-minute acceptance.

A second comparison holds the measured source constant and changes only its minimum
iPadOS version: 18.0 → 17.5 → 18.0. The 18.0 executable SHA-256 is
`bc36589ca5e13c6c7f953b1b24770c2e175f5b07849fb518ea85238ecb623ae7`.
All three GPU-timing-disabled runs complete with nominal thermal state and no
input rejections, frame errors or missing presentations.

| Same-source deployment control | 18.0 first | 17.5 repeat | 18.0 second |
| --- | --- | --- | --- |
| Actual presentations/s | 116.116 | 116.133 | 116.200 |
| Owner service p95, ms | 2.443 | 2.482 | 2.273 |
| Owner service p99, ms | 3.068 | 2.980 | 3.305 |
| Presentation interval p99, ms | 12.500 | 8.334 | 13.833 |

The repeated 17.5 CPU p99 is below both same-source 18.0 controls. This does not
reproduce a latency-tail penalty from lowering the deployment target. It does
not isolate compatibility source changes from temporal runtime variation against
the frozen baseline. A Mac test-bundle compile overlapped the 17.5 run; the
reference iPad ran only the workload. Evidence and stage/pen-up attribution are
in `artifacts/ipados17/comparison/codegen-control/`.


## GTK panel-tab input diagnostic

The 2026-10-19 UTC release build captures the tab preview at pickup from its
retained source. It uses base `05fc2e765` plus the GTK capture and regression
changes, including the motion fixture's post-press Undo baseline. The default
untitled canvas runs on Ryzen Threadripper PRO 9995WX and RTX PRO 6000 Blackwell
Max-Q, NVIDIA 615.71.09 Vulkan, with private Mutter 50.5 at 1600 × 1000, 120 Hz and
scale 1. The executable SHA-256 is
`3afe682024fd09a83bdfdc56f2fc77f1cd6a2bda68e2845678d39c1a9829f853`.

The single corrected-fixture invocation observes 237 native input samples in the
first 2.2 s group/mouse motion, passing the required more than 200. It presents at
103.530/s, below the 114 Hz desktop floor, with a 9.410 ms presentation-gap p99.
GPU time is 0.127 ms median and 0.364 ms p95. The run stops at this first rate failure;
no retry runs. That invocation leaves tab, touch, column and toolbar motion unmeasured. The
failed rate gate, short duration and non-reference hardware prevent tier
qualification.

[Raw metrics](../../artifacts/panel-tab-press/gtk-preview/workspace-motion-corrected.json)
and [build metadata](../../artifacts/panel-tab-press/gtk-preview/workspace-motion-corrected-build.json)
identify the measured source. The [run log](../../artifacts/panel-tab-press/gtk-preview/workspace-motion-corrected.log)
records the unchanged input and rate gates. The earlier lifecycle-assertion run
is preserved separately in the same artifact directory.

## GTK surface handoff diagnostic

On 2026-10-24, the release build at `5579e7b17` plus the surface handoff fixes
ran `native_frame_pacing` with Pan in both themes. NVIDIA RTX PRO 6000 Blackwell
Max-Q, Vulkan 615.71.09 and GSK Vulkan drove private Mutter at 1600 × 1000,
120 Hz. The fixture document was 1333 × 1000 with a 1200 × 900 viewport.
Each theme supplied one six-second synthetic host gesture; the reported window
excludes the first and last quarter-second.

| Theme | Canvas worker frames | Presented frames in interior window | Presents/s | Gap p99, ms |
| --- | ---: | ---: | ---: | ---: |
| Light | 724 | 660 | 120.010 | 8.679 |
| Dark | 723 | 660 | 120.008 | 8.615 |

This measures canvas presentation after the ownership changes. It has no frozen
baseline, reference tablet, tier photo,
three repeated gestures or per-frame camera matrices, so it does not qualify a
tier or establish a before/after comparison. [Raw data and window rules](../../artifacts/unsafe-followup/verification/surface/desktop-moving-canvas-summary.json)
and [build provenance](../../artifacts/unsafe-followup/verification/surface/build-provenance.json)
identify this diagnostic.

## GTK tab-hover diagnostic

The 2026-10-19 UTC release build uses `f35817f4d` plus the tab-hover changes in
`753d5c012`. Its executable SHA-256 is
`6a6310d5d71ad00e39ab952e81e525ff72d499e4eb20eba47fed9c583f83466e`.
The blank startup drawing is 3508 × 2480 pixels. The desktop has a Ryzen
Threadripper PRO 9995WX and RTX PRO 6000 Blackwell Max-Q, NVIDIA 615.71.09 Vulkan.
Private Mutter 50.5 runs at 1600 × 1000, 120 Hz and scale 1. Other tests from this
task are idle, and its Windows VM is paused with no active fixture.

The scoped `native_workspace_tab_motion_input` invocation uses
`LAYER_NATIVE_EVENT_MS=4`, preserving the full motion mode's input pacing.
One gesture of 2.2 seconds for each input type passes the unchanged gate of more
than 200 native samples and the 114 Hz presentation floor:

| Input | Native samples | Presents/s | Presentation gap p99, ms | Pickup command to first present, ms |
| --- | ---: | ---: | ---: | ---: |
| Mouse | 268 | 120.001 | 9.007 | 9.102 |
| Touch | 550 | 120.011 | 8.662 | 12.310 |

Mouse/touch dispatch p95 is 0.213/0.139 ms and native placement p95 is
0.005688/0.007011 ms. Both gestures retain the UI models with zero refreshes.
No canvas or backdrop frames are produced during the sampled motion, so there
is no canvas GPU timing. Pickup timing includes native input-driver command
overhead and does not measure physical input latency.

This short desktop diagnostic does not qualify the tier: it lacks the reference
tablet, tier photo and three sustained gestures of 5–10 seconds. It is a different
scenario from the earlier group probe and does not establish a before/after
comparison.
[Raw metrics](../../artifacts/panel-tab-press/hover-contours/tab-motion-corrected.json),
[build metadata](../../artifacts/panel-tab-press/hover-contours/tab-motion-corrected-build.json)
and the [run log](../../artifacts/panel-tab-press/hover-contours/tab-motion-corrected.log)
identify the frozen executable, source hashes and successful gates.

The continuous tab-contour change based on `b25c628ec` was checked on
2026-10-24 with release Rust, system GTK 4.22.5/libadwaita 1.9.4, Mutter 50.5,
the same workstation GPU and driver, and a private 1600 × 1000 display at
120 Hz and scale 1. Medium glass and the blank fixture drawing remain enabled.
Three independent scoped tab-motion invocations record one 2.2-second contact
per input type, with no builds or other tests from this task running:

| Input | Native samples | Presents/s range | Worst presentation gap p99, ms | Pickup command to first present, ms |
| --- | ---: | ---: | ---: | ---: |
| Mouse | 267–268 | 119.999–120.011 | 8.635 | 8.043–10.913 |
| Touch | 550 | 110.785–120.023 | 9.409 | 15.261–25.248 |

All contacts pass the native-input gate, with zero model refreshes and no canvas
or backdrop frames during motion. The first touch trial fails the unchanged
114 Hz gate; the next two pass. The initial baseline invocation fails drag
pickup before measurement. Its retry gives 119.558/119.576 presents/s for
mouse/touch, with p99 gaps 9.299/8.583 ms. These short, uneven trials do not
establish a sustained before/after comparison or qualify reference hardware.
[All three trial summaries](../../artifacts/tab-gap-candidate/motion/summary.json),
[source and executable hashes](../../artifacts/tab-gap-candidate/source-binary.sha256)
and [the failed trial log](../../artifacts/tab-gap-candidate/motion/trial-1-test.log)
identify the measured build and retained failure.

## Panel-tab compositing

The Android, Apple, Web and Windows continuous-tab backgrounds based on
`9baca504f` remain unqualified on reference hardware. The GTK measurements above
cover its separate revision. Native pixel checks and drag journeys establish
visual and input behaviour only on the hosts where they run; they do not
establish sustained frame rate.

Desktop headless Chrome's matched baseline/candidate mouse and touch tab probes
use 1440 × 913 CSS pixels at scale 1, Off glass and one 2.2-second input window
per contact. Both preserve the DOM and model with zero full state or layout
refreshes. The candidate records 133 animation callbacks per
contact at 60.00 Hz, with dispatch p95 0.50/0.40 ms for mouse/touch. These are
callbacks rather than presentation timestamps. An earlier touch serialization
failure did not recur in this comparison; the broader workspace journey still
fails its zero-GPU-rendering assertion for Navigator. These runs do not qualify
performance. [Comparison and provenance](../../artifacts/panel-tab-hover/web-touch-matched-baseline.local.log)
identify the frozen sources and matching browser, package and settings.
Windows software-adapter checks do not measure hardware presentation, and Apple
native timing is unavailable without a Mac.

Android's matched Huion Kamvas Pad 12 (Mali-G57) baseline/candidate
`continuousDragFrameTiming` runs use the same debug APK configuration, optimized
`dev-perf` Rust, private application
ID, default Low glass and blank fixture. Both stop after the first five-second
mouse/attached gesture at the existing retained-model assertion: one full model
publication, expected zero. That assertion precedes timing output, so neither
run supplies frame rates or interval distributions. This is not a timing
comparison or reference-tier qualification. [Android results](../../artifacts/tab-seams-android/results.json)
and [build hashes](../../artifacts/tab-seams-android/hashes.json) identify both
executables and the failed retention gate.

## Web Navigator layout

The 2026-10-19 Wacom MovinkPad Pro 14 comparison uses Chrome 154.0.8037.126
and hardware WebGPU, at its unchanged display settings and thermal status 0
before and after every gesture. Native landscape content is 1646 × 908 CSS
pixels at DPR 1.75. Both optimized `dev-perf` builds use the 9504 × 6336 tier
photo, one empty front paint layer, Fit zoom, the starting Paint workspace and
default panel glass. A one-second priming gesture is reset before three
six-second gestures for each operation. CDP supplies mouse movement every
4 ms, with 1460–1473 input events per measured gesture.

| Operation | Frozen baseline presents/s | Layout fix presents/s | Baseline p99 gap, ms | Layout fix p99 gap, ms |
| --- | ---: | ---: | ---: | ---: |
| Navigator column resize | 30.34–30.89 | 30.00–30.64 | 50.00–58.34 | 50.00–58.34 |
| Navigator drag | 61.50–64.68 | 60.50–64.92 | 25.00–41.67 | 25.00–33.33 |

These are SurfaceFlinger actual-present timestamps from Chrome's child surface,
cleared before each contact and polled during input. The ranges overlap and
remain below the 114 presents/s floor, with p99 above 16.7 ms. Polling can include
the stationary contact edge and a final presentation tail; exact matching of
geometry changes to presentations is unavailable, so tier qualification remains
unset. Synthetic contacts do not establish physical pen latency. Browser
geometry callbacks are retained separately and do not count as presentations.
Earlier sequential-dispatch probes in the same artifact directory are diagnostics
because CDP acknowledgement paced their input.

The frozen baseline is `a46600c8d`, Wasm SHA-256 prefix `4dfb5f6e7fe9`; the
layout candidate has prefix `ea29c3c8fc48`. Source differences, exact settings,
input counts, thermal observations and raw presentation timestamps are retained
in `artifacts/navigator-layout-fix/movinkpad14/`, with the matched results in
`baseline-performance-queued/` and `fixed-performance-queued/`.

## Vector layers

Measured 2026-10-20 on the MovinkPad Pro 14 at `2567874a7`, built from `git archive`
(benchmark APK SHA-256 `5ca3906e…`, native library `b177594c…`), with
`android-vector-benchmark.py` and, for inking, `android-brush-benchmark.py`
([measuring](measuring.md#vector-layers-android)). The document is a
`crowded_vector_fixture` generated at that commit, 9504 × 6336, with 50,000
hatching paths on the edited Ink vector layer below a translucent Tones vector
layer, an empty Base paint layer and Paper hidden, at Fit (zoom 0.238) in the
default Sketch workspace with Stats and Navigator closed. The panel presented at
120 Hz.
Rows marked *round 3* were re-measured on 2026-10-20 on the round 2 and 3 tree
(`git archive e7cdd23412d7`, the same fixture generator) in alternating runs
against `2567874a7` where noted, and rows marked *round 4* on 2026-10-21 on the
round 2–4 tree (`git archive e2667d46`; the Outline width scrub on
`git archive a4027fb6`), each with a fixture generated from its tree; the
other rows are from `2567874a7`.
Each row has one 1.5 s priming gesture and three 5 s gestures (10 s for
inking), each undone; thermal status stayed 0. Presented rates join the canvas
SurfaceView's buffers to SurfaceFlinger presents; drag start is pen-down to the
present of the first frame submitted after the first move. Responses give the
first changed present and, where later, the present that completes the work.
Causes and proposed fixes for the misses are in [known gaps](known-gaps.md);
summaries and symbolized profiles are in `artifacts/vector-tablets-20261020/`.

| Row | Target | Measured | Result |
| --- | --- | --- | --- |
| G-Pen 24 px on the crowded layer, *round 4* | 120 | 239.4–244.8 completed updates/s, gap p99 12.8 ms; 235.8–240.9 at `2567874a7` | **Met** |
| Pen-down 5 ms after committing a 195 ms line | 17 ms | First GPU completion p50 8.2–10.3, p95 14.4–18.0, max 16.6–19.4 ms | **Not met** in one run of three (p95 18.0 ms) |
| Move drag, 1 path, *round 3* | 120, 100 ms start | 119.8–120.0 presented/s, p99 ≤8.5 ms; start 43–80 ms | **Met**; start 123–136 ms at `2567874a7` |
| Move drag, 100 / 5,000 paths, *round 3* | 120, 100 ms start | 120.0/s, p99 ≤8.5 ms; start 41–59 / 40–53 ms | **Met** |
| Scale with the Move frame, 100 / 5,000 paths | 120 | 120.0/s each, p99 8.3 ms; 5,000 settles 0.44–0.45 s after release | **Met** |
| Rotate with the Move frame, 100 / 5,000 paths | 120 | 120.0/s each, p99 8.3 ms | **Met** |
| Edit Points anchor drag, 1 / 100 paths selected | 120 | 118.8–119.0 / 118.6–119.0/s, p99 ≤8.5 ms; start 66–79 ms | **Met** |
| Eraser: touched part / whole paths / up to intersection (64 px) | 120 | 119.8–120.0 / 118.8–119.8 / 119.2–120.0/s, p99 ≤16.7 ms | **Met** |
| Properties Outline width scrub, 100 / 5,000 paths, *round 4* | 120 | 120.0 / 29.6–32.1 updates/s, p99 8.3 / 58.3–66.7 ms, in two sets of three; 5,000 at 18.5–21.4 on `e7cdd234` and 26.1–35.0 at `2567874a7` | 100 **met**; 5,000 **not met**, back to the `2567874a7` rate |
| Pan / pinch / rotate, no paths selected | 120 | 27.8–28.8 / 36.8–38.9 / 25.9–26.1/s, p99 50 / 83–92 / 50 ms | **Not met** |
| Same, 5,000 paths selected; pinch *round 3* | 120 | 27.7–28.1 / 21.1–21.4 / 25.6–26.1/s; pinch gaps 0.67 s (0.68–0.70 s at `2567874a7`) | **Not met** |
| Fill inside a closed path | 100 ms | First change 22–62 ms, complete 122–137 ms | **Met** |
| Fill outside the paths (new layer below) | 250 ms | Complete 2.10–2.12 s | **Not met** |
| Commit after a 3 s line | No separate limit | Committed path presented 135–139 ms after pen-up | See the pen-down row |
| Undo of that line | 250 ms | 146–166 ms | **Met** |
| Merge Down onto a vector layer | 100 ms response | 170–178 ms | **Not met** |
| Merge Down onto a paint layer, from the notice | 100 ms response | 1.93–2.45 s; nothing changes on screen until it completes | **Not met** |
| Rasterize Layer | 100 ms response | 1.42–2.36 s; nothing changes until it completes | **Not met** |
| Frame that shows the finished Rasterize | Motion only | 63–87 ms CPU and 87–136 ms GPU | Not a motion row |
| Open the document | 6 s | Parsed 757–806 ms, first frame 1.08–1.14 s, settled 1.78–1.85 s | **Met** |
| Paint-layer thumbnail after filling the whole Base layer | No limit | 6.40–6.44 s, polling every 120 ms like the Layers panel | Unqualified |
| Pan / G-Pen stroke while that thumbnail is pending | 120 | 38.8–40.7 / 119.8–120.0/s; 38.7–40.5 / 119.8/s without polling | No extra drops |

### V1 geometry correction measurements (2026-10-22)

Re-measured the affected rows on the MovinkPad Pro 14, at 9504 × 6336 with
50,000 paths, using the same authored crowded fixture for all builds. Baseline
source is `3ac660dc`. Edit Points and ink use `47c53335` plus the geometry
snapshot with manifest SHA-256 `bb5bd518…` (benchmark APK `cc27a4b8…`); erasing
uses the measured cap-aware eraser snapshot `dbe23b92…` (APK `b05e9fbc…`). The
earlier point and ink measurements retain their own source provenance. The
final projection pruning for Butt and Square caps (`erase.rs` SHA-256
`09703e62…`) is outside the measured Round branch. All builds use the same
optimized benchmark profile, private app ID, Fit view, Sketch workspace, closed
Stats and Navigator, and default 120 Hz panel setting. Each row has a priming
gesture and three 5 s gestures, each undone; thermal status stayed 0. The
edited hatches have authored pressure profiles and Round caps and joins. These
measurements supersede the corresponding older rows above.

| Row | Baseline presents/s | Corrected presents/s | Corrected worst run p99, ms | 120 fps target |
| --- | --- | --- | --- | --- |
| Edit Points, 1 path | 120.0–120.0 | 119.6–120.0 | 8.33 | **Met** |
| Edit Points, 100 paths | 120.0–120.0 | 120.0–120.0 | 8.33 | **Met** |
| Eraser, touched part | 118.6–120.0 | 119.8–120.0 | 8.40 | **Met** |
| Eraser, whole paths | 119.0–119.4 | 119.4–119.6 | 8.52 | **Met** |
| Eraser, up to intersection | 119.0–120.0 | 119.8–120.0 | 8.33 | **Met** |

Intersection erasing starts its first moving presentation in 53.4–67.9 ms,
compared with 67.6–81.8 ms in the baseline. All three current runs meet the
separate 100 ms response limit (no baseline runs exceeded it). The motion-rate
qualification above does not include start latency.

The ink runs below count completed canvas activity. Both builds report zero
input-attributed updates/s because vector samples do not advance the recorded
consumed-paint timestamp. These rates therefore do not establish fresh ink
throughput, and the current inking target remains unqualified.

| Row | Baseline canvas updates/s | Corrected canvas updates/s | Corrected worst run p99, ms | Result |
| --- | --- | --- | --- | --- |
| G-Pen 24 px, constant pressure | 251.3–253.4 | 249.2–251.3 | 12.37 | **Unqualified** |
| G-Pen 24 px, variable pressure | 252.9–259.3 | 245.3–251.5 | 12.43 | **Unqualified** |

Build manifests, source diffs, fixed fixtures, per-run reports and presentation
traces are retained under `artifacts/vector-v1-fixes-20261022/performance/`, in
`baseline/top/`, `candidate-batch2-fast/top/` and
`candidate-batch2-cap-aware/top/`. The middle tier was reserved by another
session and was not measured.

## GTK rotation snapping diagnostic

Measured 2026-10-24 on NVIDIA RTX PRO 6000 Blackwell Max-Q, Vulkan driver
615.71.09, optimized GTK with a private 1600 × 1000 display at 120 Hz.
The candidate is based on `4a6abf44b`, test executable SHA-256
`a439c109d2591bc0720ae6ec9669b886f0f780deec50579acf6db477b34ee5e1`.
A generated 6000 × 4000 ProPhoto source, 32 paint layers and five pointwise
adjustments produce 958 distinct moving presentations over 7.992 s:
119.882 presents/s, p99 gap 8.557 ms and maximum gap 16.694 ms.
Artwork and preview revisions remain unchanged, and presentation stops at idle.

This desktop diagnostic uses 1.6-second workload phases and a generated source;
it does not qualify sustained five-second gestures, the reference photo or
reference tablet hardware. Evidence is under `artifacts/rotation-snap/gtk/`.

## GTK navigation cache comparison

Measured 2026-10-25 on the local NVIDIA RTX PRO 6000 Blackwell Max-Q hardware
GPU, driver 615.71.09, GTK 4.22.5 and Mutter 50.5. The private Wayland display
is 1600 × 1000 at 120 Hz, scale 1; both compositor and canvas use the same GPU.
Baseline `e27a493fb` and fix checkpoint `d8ac988f0` use identical release fixtures.
The later rebase preserves these navigation paths; source/build hashes remain
in the evidence. Each row has three eight-second contacts after priming.

These rates count changed-camera canvas frames joined to `wp_presentation`,
divided by the entire contact duration. Rotation snapping is enabled, so
intentional stationary holds remain in that denominator. Raw gaps across those
holds are not moving-only gap qualification. The software pointer sends about
200 events/s; GTK delivery/coalescing also limits the observed rate. These are
interactive diagnostics on a virtual display, not reference-tier or physical
input qualification. CPU medians below use the frames joined to moving
presentations.

| Workload | Baseline changed presents/s | Fixed changed presents/s | Change | Worker wall p50, ms, baseline → fix |
| --- | --- | --- | --- | --- |
| default-startup-rotate | 64.11 | 65.73 | +2.52% | 0.441 → 0.385 |
| default-startup-zoom | 80.11 | 80.86 | +0.94% | 0.380 → 0.371 |
| photo-closed-pan | 61.73 | 61.73 | -0.01% | 0.624 → 0.584 |
| photo-closed-rotate | 65.60 | 66.36 | +1.15% | 0.603 → 0.556 |
| photo-closed-zoom | 63.49 | 63.86 | +0.58% | 0.705 → 0.561 |
| photo-open-pan | 59.86 | 59.86 | -0.01% | 0.589 → 0.559 |
| photo-open-rotate | 66.11 | 64.99 | -1.69% | 0.500 → 0.589 |
| photo-open-zoom | 62.86 | 62.86 | +0.01% | 0.569 → 0.533 |

All three-contact rate ranges overlap; no substantial interactive speedup is
established. The actual GTK startup document is 1333 × 1000 with its default
Classic layout and visible Navigator, Coldpress Naturalwhite and relief 100.
Photo rows use the exact 9504 × 6336 tier JPEG plus an empty paint layer at Fit,
with Navigator explicitly closed/open. Camera, extent, display and material
match within each pair. An initial default-zoom pair with a 14 px camera offset
is excluded; the accepted retry matches exactly.

Fourteen existing functional invocations pass across baseline/fix: navigation
controls cover both themes, and GPU recovery, background-tab surfaces and
pending-stroke Undo/Redo run in light and dark. Initial fixture compilation and
private-socket setup failures remain in the logs. Raw joined frame IDs, pointer
cadence, CPU/GPU distributions, variance, snap-hold reconstruction limits and
provenance are under `artifacts/nav-rebase-qualification-20261025/gtk/`.

## Web navigation comparison

Measured 2026-10-25 in Chrome 155 on the MovinkPad Pro 14, using hardware
WebGPU on Adreno 735. Baseline `b25c628ec` and fix `d7d827671` use release Wasm,
the same collector and matched camera, paper, layout and display geometry.
Each row has three eight-second contacts after priming. The fresh Sketch
workspace has a 2401 × 1801 default document and closed Navigator; fullscreen
is 2880 × 1800 at factory density 280 and font scale 1. Photo rows use the exact
9504 × 6336 tier JPEG plus an empty paint layer at Fit.

The table counts presentations from Chrome's content surface over the entire
Java input contact, including onset, tail and intentional rotation snap holds.
It reports the median of three contacts. Canvas submissions are observed
separately but are not joined to individual displayed frames, so these rates
do not establish fresh-canvas FPS or qualify the 120 fps target.

| Workload | Baseline content presents/s | Fixed content presents/s | Change |
| --- | --- | --- | --- |
| default-rotate | 118.20 | 118.47 | +0.23% |
| default-pinch | 117.91 | 118.21 | +0.26% |
| photo-closed-pan | 118.20 | 118.19 | -0.01% |
| photo-closed-rotate | 117.71 | 117.33 | -0.32% |
| photo-closed-pinch | 63.60 | 62.85 | -1.18% |
| photo-open-pan | 118.18 | 118.08 | -0.08% |
| photo-open-rotate | 117.83 | 117.72 | -0.10% |
| photo-open-pinch | 65.73 | 67.10 | +2.07% |

Seven paired run ranges overlap. Open-Navigator pinch ranges are 64.840–66.592
and 66.599–68.102 presents/s; this small separation in three contacts does not
establish a sustained improvement. No material Web speedup is demonstrated.
Photo zoom stalls on both builds: closed-Navigator `app.frame` maxima are
266–363 ms before and 426–445 ms after. One fixed contact has a 664 ms prefix
before its first presentation in the analyzed motion window. Full-contact
rates retain that stall; first-to-last presentation cadence would conceal it.
GPU completion callback timings include callback-delivery delay.

All 48 contacts retain matched source, lifecycle, geometry, input, canvas and
thermal checks and equal SurfaceFlinger-ring/Perfetto presentation counts.
Ten contacts have corroboration flags: six exceed the frozen 20 µs timestamp
agreement bound, and four contain vendor trace-parser warnings. They remain
in the table; no contacts are selected by passing the timing check. No trace
data-loss counters or ring rollover are reported. Earlier batches with stacked
observers are excluded diagnostics. Raw traces, full-contact and motion-window
rates, onset/tail coverage, flags and build hashes are under
`artifacts/nav-rebase-20261025/`.

The hardware Web pen/cursor, G-Pen pixels and Undo/Redo journey passes in light
and dark. The broader editor journey fails in both themes at its pure-white
paper expectation, reading `[245,242,233,255]` from the default Naturalwhite
paper. The same failure reproduces on the baseline; no runtime errors occur.
Its later save/open/export checks are not reached.

## GTK pen tilt utility

Measured on 2026-10-25 UTC with the pen tilt editor on the `04d8dc0c3` working
tree. The final release test executable has SHA-256
`25e44c1c0063806988b41a136629c750b2b79a4f184c433e302bd68041abe4b4`.
The private Mutter display is 1600 × 1000 at 120 Hz, scale 1, on NVIDIA RTX PRO
6000 Blackwell Max-Q/Vulkan, driver 615.71.09. The fixture is a 256 × 256 F32
drawing at Fit with default transparency. These are desktop diagnostics, not
reference-tablet or 61 MP painting qualification.

Both themes first exercise Preferences, mouse/touch/pen controls, live native
tilt, graph and frame dragging, drawing, Apply, Reset, Cancel, Close and Escape.
Each motion case then runs three five-second frame drags and three curve drags,
with input every 8 ms. Frame motion follows a 200 × 100 logical-pixel ellipse;
the authored interior control follows a 4 × 20 logical-pixel ellipse. Only native
presentations during observed motion count. The unchanged gates are at least
114 presents/s and interval p99 at most 2000/120 ms.

| Case | Light presents/s / maximum p99 gap | Dark presents/s / maximum p99 gap |
| --- | --- | --- |
| Captured pre-task pressure: Frame | 100.04–118.22 / 25.087 ms | — |
| Captured pre-task pressure: Curve | 100.07–101.86 / 25.039 ms | — |
| Shared pressure: Frame | 111.23–118.61 / 24.915 ms | 116.01–118.21 / 16.821 ms |
| Shared pressure: Curve | 116.61–118.81 / 16.763 ms | 117.61–118.81 / 16.659 ms |
| Tilt before table reuse: Frame | 118.21–118.81 / 16.669 ms | 114.82–119.80 / 16.782 ms |
| Tilt before table reuse: Curve | 117.80–119.01 / 16.683 ms | 108.84–119.21 / 25.129 ms |
| Final tilt: Frame | 108.24–119.40 / 24.983 ms | 99.53–116.21 / 25.022 ms |
| Final tilt: Curve | 100.43–117.62 / 25.033 ms | 97.07–100.51 / 25.085 ms |

Final tilt misses at least one gate in 10 of 12 gestures. The earlier tilt build
misses in 6 of 12. The captured pre-task pressure build also misses in 5 of 6
Light gestures; its exact source commit is not established. These runs retain
competing desktop GPU activity, and the final runs also overlap other sessions’
compilation. They do not establish an isolated regression or improvement, and
none qualifies the performance target. No thresholds were relaxed and no user
processes were stopped. Pressure's widget path is unchanged by tilt table reuse;
its current measurements precede that cache change.

The final shared implementation reuses compiled tables for direction/frame
changes and skips recompiling unchanged device profiles. A headless regression
checks response-table sharing and unchanged mapping across all four frames.
Functional tilt and pressure journeys pass in both themes. The related tonal
Curves keyboard journey fails its modifier-release/Undo assertion in both current
and captured pre-task executables; this editor change does not resolve that
existing issue.

JSON and screenshots are under `artifacts/tilt-editor-design/`, including
`before-cache`, `after-cache`, `pre-task-captured`, and `motion-after-summary.json`.
Reproduce the tilt case with `native_tilt_calibration --tablet`,
`LAYER_TILT_MOTION=1`, and `LAYER_NATIVE_EVENT_MS=8`. The
[tilt guide](../ui/pen-tilt.md) describes profile and coordinate-frame limits.

### Immediate tilt editing and compact calibration controls

Measured on 2026-10-25 UTC on the `ea6bc7ba8` working tree, with editing enabled
before pen identification, cursor-independent UI wakes, a joined direction icon
bar and standard dialog footers in both utilities. The release test executable
SHA-256 is `1a2e01477679eb50ad72bc002385e94d8586e50a8d6706a420c084b9343f2a19`.
The GPU, driver, 1600 × 1000/120 Hz private display, drawing and six-gesture
protocol match the preceding diagnostic. Tilt also exercises first-hover
identification with the brush cursor hidden and no stroke, retained pending
edits, combined icon toggles, tooltips, 20-pixel aligned insets, 34-pixel footer
buttons and the 8-pixel Cancel/Apply gap. Both themes pass all functional and
geometry assertions for tilt and pressure; screenshots were inspected.

| Case | Light presents/s / maximum p99 gap | Dark presents/s / maximum p99 gap |
| --- | --- | --- |
| Tilt: Frame | 114.43–118.41 / 16.690 ms | 119.01–119.02 / 10.766 ms |
| Tilt: Curve | 117.41–118.61 / 16.761 ms | 117.41–118.60 / 16.644 ms |
| Pressure: Frame | 116.82–118.41 / 16.713 ms | 116.82–118.61 / 16.748 ms |
| Pressure: Curve | 117.41–118.01 / 16.695 ms | 117.81–118.60 / 16.669 ms |

All 24 current gestures pass the unchanged 114 presents/s rate floor. Fifteen
pass both gates; nine exceed the strict 2000/120 ms gap limit, with a worst p99
of 16.761 ms. External desktop and application GPU activity remains present.
These diagnostics do not qualify the tier target or establish an isolated
performance comparison. No thresholds were adjusted or user processes stopped.
Raw reports, screenshots, load observations and the complete assertion summary
are in `artifacts/tilt-editor-startup-fix/`, including `motion/` and
`verification-summary.json`.

### Matched square calibration plots and centered direction controls

Measured on 2026-10-25 UTC on the `1d952d603` working tree, with the revised
384-pixel utilities and common font-measured axis gutters. Final release test
SHA-256 is `5646bf37e0186d4122edd7f74584af688731ad3e45dcf1e165e6ff1b1e734980`.
The same GPU/driver, private 1600 × 1000/120 Hz display, drawing and six-gesture
protocol are used. Both themes pass native input, startup, history and layout
assertions. The final render has matching 262 × 262 plots, title/Close centers,
16-pixel content margins, 34-pixel actions and a preview stacked above its icon
bar on the plot centerline. The initial undersized allocation warning is gone.
The tonal Curves calibration/history smoke also passes; its layout branch is
unchanged. Screenshot and per-widget geometry review cover both themes.

| Case | Light presents/s / maximum p99 gap | Dark presents/s / maximum p99 gap |
| --- | --- | --- |
| Tilt: Frame | 101.43–105.05 / 25.032 ms | 111.19–117.41 / 25.155 ms |
| Tilt: Curve | 99.84–105.83 / 25.135 ms | 105.88–116.01 / 33.146 ms |
| Pressure: Frame | 111.62–115.22 / 16.833 ms | 82.39–102.17 / 33.555 ms |
| Pressure: Curve | 103.00–105.65 / 25.002 ms | 76.90–88.45 / 33.330 ms |

External GPU renderer tests and Rust/Android builds were active throughout these
runs. Twenty of 24 gestures miss the 114 presents/s floor and all 24 exceed the
strict 2000/120 ms p99 gap limit; none qualifies both gates. These are contended
system diagnostics and do not establish an isolated layout regression or qualify
the tier target. All native movement, presentation-count and pixel-snapping
assertions pass, without GTK/GDK sizing warnings. No thresholds were changed,
measurements repeated for better results, or external processes stopped.
Reports, load observations, screenshots and geometry are in
`artifacts/tilt-layout-review/`, with final functional results under `final/` and
all pacing results under `motion-final/summary.json`.

### Calibration utilities above expanded columns

Measured on 2026-10-25 UTC on the `2fd981a34` working tree with shared
clear-canvas placement and the GTK utility above column drawers and connections.
Final release test SHA-256 is
`0439f08dc943b9c7939049fb42759138480279cd585a8e0a2ab08dfc6d4e1c0e`.
The private 1600 × 1000/120 Hz display, 256 × 256 F32 drawing, RTX PRO 6000
at `F1:00.0`, driver `615.71.09` and six-gesture protocol are unchanged.
All four native journeys pass, including mouse/touch/pen controls, default
placement, native clicks over expanded columns and exact utility-interior pixel
comparisons with the column shown and hidden. Light and dark renders retain
matching plots and use the app's compact 24-pixel direction segments.

| Case | Light presents/s / maximum p99 gap | Dark presents/s / maximum p99 gap |
| --- | --- | --- |
| Tilt: Frame | 116.02–117.41 / 16.710 ms | 118.81–119.01 / 16.516 ms |
| Tilt: Curve | 116.42–118.61 / 16.693 ms | 117.81–119.20 / 16.665 ms |
| Pressure: Frame | 117.61–118.61 / 16.732 ms | 115.02–118.21 / 16.723 ms |
| Pressure: Curve | 117.61–118.41 / 16.696 ms | 117.22–118.01 / 16.668 ms |

All 24 gestures meet the unchanged 114 presents/s floor; nine miss the strict
2000/120 ms p99 limit, leaving 15 gestures that meet both gates. Desktop GPU
utilization was 14% before motion, and external Rust builds remained active.
These desktop diagnostics leave reference-tablet qualification open and do not
establish a change in speed from the earlier runs under different loads.
The dark pressure run reports one GDK duplicate-presentation warning with an
8.516 ms timestamp difference. Other runs have no GTK/GDK warnings; existing
portal/a11y helper and secret-proxy warnings remain in the logs. No thresholds
were changed, runs repeated for better results, or external processes stopped.
Reports, screenshots, geometry, warnings and load observations are under
`artifacts/calibration-layering/`; shared placement and full UI results are under
`artifacts/pen-response-placement/`.

### First pen identification and dialog sensor input

Measured on 2026-10-25 UTC on the `e1460e2e5` working tree with preserved
first-contact geometry, corrected measurement recovery, sensor input over the
GTK utilities and 36-pixel controls. Final release test SHA-256 is
`f3bee058deff1ea364f63a587a92006cef7aa310959a8b49caa28e1b7bf3fb0c`.
The private 1600 × 1000/120 Hz display, 256 × 256 F32 drawing, RTX PRO 6000
at `F1:00.0`, driver `615.71.09` and six-gesture protocol remain the same.
The four native functional cases and existing canvas pen-button journey pass.
They verify untouched first Lasso contact, retained manual placement, recovery
from mismatched measurements, native sensor previews over the dialog and circle
without artwork/history changes, and matching 36-pixel actions in both themes.
The utilities retain matching 262-pixel square plots.

| Case | Light presents/s / maximum p99 gap | Dark presents/s / maximum p99 gap |
| --- | --- | --- |
| Tilt: Frame | 99.27–117.61 / 41.683 ms | 105.85–119.61 / 25.045 ms |
| Tilt: Curve | 117.81–119.21 / 16.662 ms | 117.81–118.61 / 16.674 ms |
| Pressure: Frame | 116.02–118.61 / 16.814 ms | 116.63–118.21 / 16.687 ms |
| Pressure: Curve | 108.05–117.81 / 25.131 ms | 109.84–118.41 / 24.994 ms |

Nineteen of 24 gestures meet the unchanged 114 presents/s floor; ten meet both
that floor and the strict 2000/120 ms p99 limit. GPU utilization was 15% before
motion; two other private native/compositor jobs and external Rust builds were
active. These desktop diagnostics leave reference-tablet qualification open and
do not establish an isolated speed change. The dark pressure run reports one
GDK duplicate-presentation warning with an 8.611 ms timestamp difference.
Other cases have no GTK/GDK warnings; existing portal/a11y/secret-proxy warnings
remain in the logs. Every motion case ran once after functional validation,
without changing thresholds or stopping external processes.

Fresh-window input coverage also exposed two test-proxy faults: tablet events
targeted the first window, and recycled GTK protocol IDs retained an unrelated
old interface. Both are fixed and covered by wire/target regressions. The
corrected functional journeys pass, while their earlier failures remain recorded.
Reports, exact hashes, screenshots, regression failures and all motion samples
are under `artifacts/calibration-first-pen/`. The shared release suite passes
1,850 tests with two ignored cases; the 17 focused tilt tests and six Python
proxy tests pass. Physical pen input and other native hosts were not exercised.

### Web pen response utility callback diagnostics

Measured on 2026-10-25 UTC from the working tree at `db176bee6`, using optimized
`dev-perf` Wasm, a private 120 Hz Mutter display and Chrome with hardware WebGPU
on an NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation Edition. The Wasm SHA-256
is `32d924ede8ebeea6c54652511214904bcb338a9a3d90af8455f706f99222edbf`;
`artifacts/pen-response-port/web/build.json` identifies the exact host sources.

Each row contains three 5.19–5.20-second CDP mouse gestures after a priming drag.
Only animation callbacks whose utility transform or curve controls change count.

| Motion | Light callbacks/s / maximum p99 gap | Dark callbacks/s / maximum p99 gap |
| --- | --- | --- |
| Tilt: Frame | 117.07–120.08 / 10.80 ms | 118.52–120.00 / 10.40 ms |
| Tilt: Curve | 117.05–119.60 / 10.20 ms | 116.27–120.04 / 11.10 ms |

These are desktop callback diagnostics, not presentation timestamps or
reference-tablet qualification. The functional tilt and pressure journeys pass
in both themes; tonal Curves also passes its mouse, touch and pen checks.
Raw samples, source hashes, logs and inspected captures are under
`artifacts/pen-response-port/web/`. Reproduce with `LAYER_TILT_MOTION=1` and
`tools/performance/workspace-motion.sh web --tilt-calibration`.

## Native source preparation qualification

| Change | Affected work | Reference-device qualification |
| --- | --- | --- |
| Shared bounded preparation workers | Cold source/mip preparation, raster restoration and simultaneous windows | Not measured; existing motion results do not qualify this pool |

The [architecture](../architecture.md) defines the worker and queue bounds.
Warm canvas motion keeps its existing preparation and GPU paths.

## Linux photo-helper import diagnostic

Measured 2026-10-25 on AMD Ryzen Threadripper PRO 9995WX, x86_64 Linux,
using the debug photo-reader integration executable SHA-256
`917623fe4812f57a2831904502fdf155b373713bae2ecb9b85fe95ba95f536fd`
and helper `91575493f2dc6172b93031e559075b91c2901d858bba66c925d8516a03f69f56`.
Two fresh processes decoded the generated solid-color progressive JPEG
8192 × 7324 (59,998,208 pixels), fixture SHA-256
`15fe621b13d9c05acb83c7c87f3c80907a67dee64728123df6d77fd1b8b42bfb`.
Both used 768 MiB codec and 64 MiB source limits and returned identical complete
row hashes and metadata.

| Current decode path | Time until decoded photo is returned | Parent peak RSS, KiB | Child peak RSS, KiB |
| --- | --- | --- | --- |
| Local reader | 5.153 s | 539,268 | — |
| Isolated helper and validated package adoption | 5.501 s | 27,804 | 539,244 |

These are single debug runs on a shared desktop, not reference-tier, release-speed
or frame-rate measurements. Separate RSS maxima are sampled after row hashing
and cannot be added into a simultaneous aggregate. The measured import difference
is 0.348 s; warm canvas motion does not enter this transport. Reproduce with the
`photo_reader` integration executable's `--measure-direct PATH` and
`--measure-isolated PATH` modes under private disk-backed `TMPDIR`.
Raw commands, source hashes and limits are retained under
`artifacts/input-safety-next/m4-linux-photo-reader-verification.txt`.
