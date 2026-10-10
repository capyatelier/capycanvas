# Top tier: 120 fps on 61 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: Wacom MovinkPad Pro 14 (DTHA140) with a 1800 × 2880, 120 Hz OLED. The
canvas is 9504 × 6336. Every row targets **120 fps** unless marked soft.

## Operations

Required-only shader startup and first-use tool preparation are unmeasured on
this reference device. Earlier idle-catalog measurements describe their named
builds, not the current startup policy. See [startup preparation](responsiveness.md#demand-driven-startup)
for the Apple diagnostic and its limits.

The M3 cutover is committed at `b3f6f8e51`. The latest G-Pen measurements use
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
| Pan: Hand tool, one or two fingers | 120 | **Misses:** 33.73–34.56 presented fps, p99 41.67 ms, 61 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized 1.0.12 APK `ef20cbb3`, navigation `cbcee39a`, base `ee4805dfe`; `artifacts/navigation-controls/zoom-audit-20261009/post-rebase/top/` |
| Pinch zoom | 120 | **Misses:** 33.51–36.69 presented fps, p99 41.67–50.00 ms, 61 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized 1.0.12 APK `ef20cbb3`, navigation `cbcee39a`, base `ee4805dfe`; `artifacts/navigation-controls/zoom-audit-20261009/post-rebase/top/` |
| Mouse-wheel pan and Ctrl-wheel zoom, including held navigation buttons | 120 | Unmeasured on the reference tablet | [Wheel input contract](../ui/shared-ui.md); desktop correctness checks do not qualify this tier |
| Two-finger rotate | 120 | **Misses:** 33.33–45.48 presented fps, p99 33.33–41.67 ms, 61 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized 1.0.12 APK `ef20cbb3`, navigation `cbcee39a`, base `ee4805dfe`; `artifacts/navigation-controls/zoom-audit-20261009/post-rebase/top/` |
| Smooth Zoom tool, left/right | 120 | **Misses:** 35.75–42.09 presented fps, p99 50.00–58.34 ms, 61 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized 1.0.12 APK `ef20cbb3`, navigation `cbcee39a`, base `ee4805dfe`; `artifacts/navigation-controls/zoom-audit-20261009/post-rebase/top/` |
| Smooth Zoom tool, up/down | 120 | **Misses:** 34.56–43.73 presented fps, p99 50.00–58.34 ms, 61 MP photo, Navigator open, thermal 0 | Measured 2026-10-09, optimized 1.0.12 APK `ef20cbb3`, navigation `cbcee39a`, base `ee4805dfe`; `artifacts/navigation-controls/zoom-audit-20261009/post-rebase/top/` |
| Painting with Filters previews pending (61 MP) | 120 | One frame-gap outlier; repeat passes. Completed-update rate unqualified. Curves thumbnail opening 6.782 → 0.943–1.015 s | [Filters previews](#filters-previews), 2026-10-06 |
| Footer zoom and rotation sliders | 120 | Unmeasured on the reference tablet | |
| Navigator drag | 120 | | |
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
| Marquee, Lasso or Polygon drag | 120 | Met on a small document: in-stroke interval p50/p99 4.2/6.9 ms with the canvas bar shown, p99 8.8 ms with it off (2048 × 1536) | `ba9483a8`, 2026-09-27 |
| Enclose and Fill: loop drag and navigation during completion | 120 | Unmeasured on the reference tablet; 61 MP completion exceeds the current 128 MiB component-buffer binding limit | Shared lasso overlay; GPU region discovery starts on release. Desktop checks do not qualify this tier |
| Selection Brush or Quick Mask, 2048 px | 120 | | |
| Grow, Shrink or Feather drag, full canvas | 120, soft | **Not met.** Feather: 14.9 updates/s on 6000 × 4000; 72.5 updates/s on 2048 × 1536 | Canvas-bar `refine-feather-drag`, 2026-09-27 |
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
| Animated or warping filter: Domain Warp, Ripple | 120, soft | | |
| Fill layer or gradient-fill edit | 120, soft | Solid Color revision unmeasured on this reference device | [Low-tier measurements](low-tier.md#solid-color-fills) do not qualify this tier |
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
| Layer opacity scrub | 120 | Solid Color revision unmeasured on this reference device | [Low-tier measurements](low-tier.md#solid-color-fills) do not qualify this tier |
| Layer reorder drag | 120 | **Met.** Native UI 117.20–119.41 fps; moving-frame gap p99 8.38–16.67 ms | [Layer reorder and retained photo translation](#layer-reorder-and-retained-photo-translation), 2026-10-04 |
| Layer thumbnail selection animation | 120 | **Not met.** Repeated content/mask selection 77.08–79.86 window frames/s; worst moving-frame gap p99 33.33 ms | [Layer thumbnail selection](#layer-thumbnail-selection), 2026-10-05 |
| Attached filter drag inside a clipping run | 120 | **Not met.** 93.24–102.71 native UI fps; moving-frame gap p99 25.00–33.33 ms | [Filter attachment feedback](#filter-attachment-feedback), 2026-10-04 |
| Navigation with 32 visible paint layers | 120 | | |
| Drawing with 32 visible paint layers, G-Pen 1024 px | 120 | | |
| Panel, tab, column or toolbar drag and docking | 120 | **Not met.** Toolbar or component drag 103–119 fps | `dc27e04d`, 2026-09-23 |
| Panel or column resize | 120 | **Unqualified.** GTK desktop Color column 12.88–44.61, floating panel 39.63–100.58 presents/s; reference tablet unmeasured | [GTK Color resize](#gtk-color-panel-resize), 2026-10-04 |
| Drawer open and close | 120 | | |
| Grouped tool menus, drawer switching and tile drag | 120 | Not measured on reference hardware | [Tool variations](../ui/panel-customization.md#tool-variations); desktop functional checks do not qualify this tier |
| Colour wheel or picker drag | 120 | **Unqualified.** Current GTK 2× workstation picker diagnostic: docked SDR 75.00, HDR 62.96 canvas presents/s; reference tablet unmeasured | [GTK Color diagnostic](#gtk-color-panel-resize), 2026-10-04; earlier [swatch diagnostic](#gtk-selected-swatch-diagnostic) |
| Slider and value scrub: size, opacity, flow | 120 | **Unqualified.** GTK workstation diagnostic only; reference tablet unmeasured | [GTK panel slider diagnostic](#gtk-panel-slider-diagnostic), 2026-10-09 |
| Canvas action bar show, hide and move | 120 | **Not met.** UI frame p50/p95: 63.2/90.0 ms moving the bar, 23.0/34.6 ms show and hide (2048 × 1536) | Canvas-bar `ui-bar-move` and `ui-bar-show-hide`, 2026-09-27 |
| Tool Options or panel content change | 120 | **Not met.** UI frame p50/p95 25.1/30.6 ms | Canvas-bar `ui-panel-change`, 2026-09-27 |
| List scrolling: layers, brushes, filters | 120 | | |
| Menu open and close | 120 | Application menu heading switching unmeasured on reference hardware. Selection-bar diagnostic: menu open adds no canvas frames; UI frame p50/p95 11.5/26.6 ms | Canvas-bar `selection-bar-menu-open`, 2026-09-27; [menu interaction](../ui/window-bar.md) needs presented-frame qualification |

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

## GTK panel slider diagnostic

Measured 2026-10-09 with compact Properties fields and fine value dragging
based on `1e94b39df`, in a release build on NVIDIA RTX PRO 6000 Blackwell
Max-Q/Vulkan 615.71.09. The private Mutter display is 1600 × 1000 at a requested
120 Hz, with a 2048 × 1536 empty drawing. `native_panel_slider_motion` uses
native mouse input at requested 4 ms intervals, warms each field, and records
three six-second scrubs each of brush size, opacity and flow in both themes,
for both slider and value handles. Each scrub traverses sixteen legs: 60% of the
track, or 240 logical pixels for size values and 120 for opacity/flow values.
This requests enough distinct values to exercise 120 Hz presentation.
Only GTK presentations whose numeric value changed are counted.

The preceding centered-field track diagnostic ranged from 114.32 to 118.62
moving presentations/s, with a maximum interval p99 of 17.004 ms.

| Theme / handle / field | Moving presentations/s | Interval p99 |
| --- | --- | --- |
| Light / Slider / Size | 118.03–118.20 | 16.667–16.669 ms |
| Light / Slider / Opacity | 118.36–118.69 | 16.606–16.644 ms |
| Light / Slider / Flow | 116.71–118.36 | 16.667–16.752 ms |
| Light / Value / Size | 118.85–119.52 | 8.678–8.717 ms |
| Light / Value / Opacity | 119.51–120.00 | 8.492–8.826 ms |
| Light / Value / Flow | 119.84–120.01 | 8.484–8.506 ms |
| Dark / Slider / Size | 117.87–118.36 | 16.553–16.704 ms |
| Dark / Slider / Opacity | 118.36–118.52 | 8.615–16.671 ms |
| Dark / Slider / Flow | 117.70–118.52 | 16.553–16.653 ms |
| Dark / Value / Size | 118.85–119.18 | 8.647–10.520 ms |
| Dark / Value / Opacity | 119.44–120.00 | 8.522–8.597 ms |
| Dark / Value / Flow | 119.84–119.84 | 8.534–8.621 ms |

The earlier four-leg diagnostic is limited by pointer-position changes and
does not measure rendering capacity. These are workstation diagnostics with a
small drawing. They do not qualify any reference-tablet target.
Raw records are in `artifacts/ui/panel-sliders/`.

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

Measured 2026-10-04 in a release build based on `48efaf593`, on AMD Ryzen
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

Target: **120 completed updates/s** at the guaranteed size, on the 61 MP canvas.

Except for G-Pen and Watercolor Wash, measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets. The ellipse is 520 × 299 px at 16.5% zoom.

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
| G-Pen (1) | Simple | 2048 px | 99.49 (99.39–102.36) fresh updates/s; completion-gap p99 25.51–29.35 ms, Full color | **Not met**; [layer color modes](#layer-color-modes) |
| Rough G-Pen (28) | Simple | 2048 px | 61.8 updates/s (60.6–62.7); gap p99 33.6 ms | **Not met** |
| Calligraphy Pen (29) | Simple | 2048 px | 189.3 updates/s (188.6–190.5); gap p99 10.6 ms | Met |
| Antique Pen (30) | Simple | 2048 px | 82.0 updates/s (81.7–82.2); gap p99 26.4 ms | **Not met** |
| Realistic Pen (31) | Simple | 2048 px | 68.5 updates/s (68.0–68.8); gap p99 31.3 ms | **Not met** |
| Wet Ink (32) | Simple | 2048 px | 54.6 updates/s (54.4–54.8); gap p99 36.1 ms | **Not met** |
| Pencil (2) | Simple | 2048 px | 47.2 updates/s (47.0–47.2); gap p99 32.4 ms | **Not met** |
| Pointy Pencil (25) | Simple | 2048 px | 47.0 updates/s (46.9–47.6); gap p99 32.0 ms | **Not met** |
| Shading Pencil (26) | Simple | 2048 px | 89.0 updates/s (88.7–89.0); gap p99 19.4 ms | **Not met** |
| Charcoal (27) | Simple | 2048 px | 52.0 updates/s (51.9–52.2); gap p99 31.7 ms | **Not met** |
| Chalk (6) | Simple | 2048 px | 38.5 updates/s (38.2–38.8); gap p99 45.0 ms | **Not met** |
| Eraser (3) | Simple | 2048 px | 49.7 updates/s (49.6–49.9); gap p99 46.0 ms | **Not met** |
| Airbrush (5) | Simple | 2048 px | 59.7 updates/s (59.5–60.6); gap p99 30.9 ms | **Not met** |
| Marker (7) | Complex | 1024 px | 103.5 updates/s (103.1–103.7); gap p99 20.7 ms | **Not met** |
| Blotty Ink (33) | Complex | 1024 px | 117.8 updates/s (117.1–118.4); gap p99 17.0 ms | **Not met** |
| Realistic Brushed Ink (34) | Complex | 1024 px | 112.5 updates/s (111.8–112.7); gap p99 18.3 ms | **Not met** |
| Pastel Block (17) | Complex | 1024 px | 82.8 updates/s (82.1–82.8); gap p99 22.2 ms | **Not met** |
| Paintbrush (4) | Complex | 1024 px | 90.58 (90.07–90.78) fresh updates/s; completion-gap p99 23.52–24.55 ms, Full color | **Not met**; [layer color modes](#layer-color-modes) |
| Textured Flat (15) | Complex | 1024 px | 109.2 updates/s (108.8–110.0); gap p99 19.0 ms | **Not met** |
| Dry Scumble (16) | Complex | 1024 px | 87.7 updates/s (87.4–88.0); gap p99 22.7 ms | **Not met** |
| Transparent Glaze (18) | Complex | 1024 px | 99.9 updates/s (99.7–100.0); gap p99 21.6 ms | **Not met** |
| Multiply Glaze (14) | Complex | 1024 px | 76.1 updates/s (76.1–76.2); gap p99 28.6 ms | **Not met** |
| Dual Texture (9) | Complex | 1024 px | 80.0 updates/s (79.5–80.6); gap p99 21.3 ms | **Not met** |
| Spray (8) | Complex | 1024 px | 33.8 updates/s (32.7–34.3); gap p99 113.2 ms | **Not met** |
| Opaque Gouache (19) | Very complex | 512 px | **Crashed**: native allocator out of memory (Scudo map failure) | **Not met** |
| Watercolor Wash (20) | Very complex | 512 px | 3.99 (3.79–3.99) fresh updates/s; completion-gap p99 575.35–609.82 ms, Full color | **Not met**; [layer color modes](#layer-color-modes) |
| Wet Watercolor (21) | Very complex | 512 px | **Crashed**: SIGSEGV inside the Adreno Vulkan driver (fault address 0x1c) | **Not met** |
| Loaded Oil (22) | Very complex | 512 px | **Crashed**: SIGSEGV inside the Adreno Vulkan driver (fault address 0x1c) | **Not met** |
| Palette Knife (23) | Very complex | 512 px | **Crashed**: SIGSEGV inside the Adreno Vulkan driver (fault address 0x1c) | **Not met** |
| Wet Round (11) | Very complex | 512 px | 16.0 updates/s (15.7–17.0); gap p99 139.9 ms | **Not met** |
| Natural Blender (24) | Very complex | 512 px | **Crashed**: SIGSEGV inside the Adreno Vulkan driver (fault address 0x1c) | **Not met** |
| Smudge (10) | Very complex | 512 px | 20.1 updates/s (19.1–20.4); gap p99 100.1 ms | **Not met** |
| Liquify Push (12) | Very complex | 512 px | 28.5 updates/s (27.6–29.7); gap p99 101.0 ms | **Not met** |
| Liquify Twirl Clockwise (36) | Very complex | 512 px | 73.5 updates/s (71.7–74.1); gap p99 36.5 ms | **Not met** |
| Liquify Twirl Counterclockwise (13) | Very complex | 512 px | 74.4 updates/s (74.2–74.6); gap p99 34.4 ms | **Not met** |
| Liquify Pinch (37) | Very complex | 512 px | 67.3 updates/s (66.1–67.4); gap p99 39.4 ms | **Not met** |
| Liquify Expand (38) | Very complex | 512 px | 68.4 updates/s (66.7–69.2); gap p99 38.3 ms | **Not met** |
| Liquify Crystals (39) | Very complex | 512 px | 41.6 updates/s (39.3–42.2); gap p99 70.0 ms | **Not met** |

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
