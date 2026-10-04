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

The BUILD20 comparisons measure the frozen M3 candidate on `4a2cf6aa0`.
Earlier operation rows and 83/BUILD15 effect and transform probes apply
to their named binaries; they do not establish BUILD20 canvas performance.
No selected BUILD20 canvas measurements were collected on this tier.

Overall M3 performance qualification remains pending. The finalized six normal
release offscreen navigation runs meet the warmed 5% p95 and +1 ms p99 comparison
bounds in all fifteen paired observations, for frame CPU, CPU through submission
and completion time. Cold half/native/double navigation in the first pair exceeds
5% p95; later same-phase comparisons improve, but individual cold observations
are not all accepted. The later 100-repeat sixteen-layer affinity ABBA pair on
frozen `4a2cf6aa0` meets the common moving and pen-up CPU-submit/completed bounds
in both adjacent temporal comparisons; pen-up completion p99 changes by -0.246
and -0.396 ms. This clears that bounded diagnostic, not the unrestricted matrix.
These frozen binaries do not qualify current `192601dac` source, reference-tablet
performance or physical input-to-present response. Exact results are retained in
`artifacts/format/m3-uninstrumented-27/navigation-analysis.txt` and
`artifacts/format/m3-final-ordinary-fixture-20261004/measurements/affinity-analysis.txt`.

| Operation | Target | Measured | Source |
| --- | --- | --- | --- |
| Pan: Hand tool, one or two fingers | 90 | Renderer 60.1 completed canvas updates/s; completion gap p99 18.8–19.3 ms, 24 MP photo | Spatial composition comparison below; 90 Hz not met |
| Pinch zoom | 90 | Every 60 Hz vsync, 4096 px document; GPU p50 7.8 ms Linear, 8.5 ms Perceptual | [Blend space](../internals/rendering.md#blend-space), 2026-09-28 |
| Two-finger rotate | 90 | | |
| Footer zoom and rotation sliders | 90 | Unmeasured on the reference tablet | |
| Navigator drag | 90 | | |
| Brush-cursor hover | 90 | | |
| Placed-photo drag (24 MP photo) | 90 | screen 59.0/s; renderer 99.4 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Pixel transform handle drag: Free, Uniform, Skew or Rotate | 90 | Free: screen 59.2/s; renderer 182.1 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Pixel transform: Distort or Perspective | 90 | Distort: screen 59.2/s; renderer 168.2 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Pixel transform: Warp | 90 | screen 59.0/s; renderer 72.5 completed updates/s, Navigator closed | Transform presentation below; 90 Hz not met |
| Selection transform, full canvas | 90 | Renderer 217 submissions/s (handle and Distort); worst frame after release 16.4–27.1 ms | `6fcc6fba`, 2026-09-27 |
| Move tool layer drag | 90 | | |
| Marquee, Lasso or Polygon drag | 90 | | |
| Selection Brush or Quick Mask, 1536 px | 90 | | |
| Grow, Shrink or Feather drag, full canvas | 90, soft | | |
| Pointwise adjustment slider: Levels, Curves, Exposure, Hue/Saturation, Color Balance, White Balance, Black & White | 90, soft | | |
| Neighbourhood filter slider: Gaussian Blur, Unsharp Mask, Edge-Preserving Smooth | 90, soft | Gaussian at 50%: 7.2/s small, 4.6/s large fresh completed canvas updates; UI 58.7–59.3/s and 40.3–41.8/s. Fit: 38.9/s and 26.1/s fresh canvas updates | Spatial composition comparison below; target not met; other filters unmeasured |
| Animated or warping filter: Domain Warp, Ripple | 90, soft | | |
| Fill layer or gradient-fill edit | 90, soft | Solid Color revision unmeasured on this reference device | [Low-tier measurements](low-tier.md#solid-color-fills) do not qualify this tier |
| Navigation with proof or tone guide shown | 90 | | |
| Gradient drag | 90 | | |
| Figure or ruler drag | 90 | | |
| Layer opacity scrub | 90 | Solid Color revision unmeasured on this reference device | [Low-tier measurements](low-tier.md#solid-color-fills) do not qualify this tier |
| Layer reorder drag | 90 | | |
| Layer swipe right: alpha lock (24 MP photo) | 90 | **Not met.** Android 59.0–59.2 fps, interval p99 16.8 ms; Web 53.1–54.6 fps, interval p99 33.5–50.2 ms | `1d251ece`, 2026-09-27; details below |
| Navigation with 16 visible paint layers | 90 | | |
| Drawing between 16 photo layers, G-Pen 1024 px (17 visible layers) | 90 | **Not met.** BUILD20: 74.304–75.047 fresh updates/s; fresh gap p99 18.236–19.527 ms, Linear | [BUILD20 middle-layer comparison](#drawing-in-the-middle-of-sixteen-photo-layers); common response bound not cleared |
| Panel, tab, column or toolbar drag and docking | 90 | **Not met.** Floating panel-group drag frame p50/p95 13.4/15.5 ms | `cbfad9e5`, 2026-09-26 |
| Panel or column resize | 90 | | |
| Drawer open and close | 90 | | |
| Grouped tool menus, drawer switching and tile drag | 90 | Not measured on reference hardware | [Tool variations](../ui/panel-customization.md#tool-variations); desktop functional checks do not qualify this tier |
| Colour wheel or picker drag | 90 | Huion: frame CPU p50 4.6–5.1 ms, p95 under 9.6 ms; XP-Pen swatch comparison below is diagnostic only | [Colour picker](../ui/color-picker.md); [swatch comparison](#selected-swatch-comparison), 2026-10-03 |
| Slider scrub: size, opacity, flow | 90 | | |
| Canvas action bar show, hide and move | 90 | **Not met.** UI frame p50: 22.8 ms show and hide, 34.8 ms moving the bar | `cbfad9e5`, 2026-09-26 |
| Tool Options or panel content change | 90 | **Not met.** UI frame p50 21.4 ms | `cbfad9e5`, 2026-09-26 |
| List scrolling: layers, brushes, filters | 90 | | |
| Menu open and close | 90 | | |
| G-Pen 1536 px stroke with a pending language change | 90 | **Not met.** 56.80 fresh updates/s (52.40–56.99), completion-gap p99 32.08–34.97 ms | Language-change diagnostic below; synthetic owner replay, no scanout qualification |

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

## Selected swatch comparison

Measured 2026-10-03 on the XP-Pen Magic Note Pad (MNP1095), at its default
90 Hz and thermal status 0. The benchmark APKs share the same Rust library;
the candidate adds Android stacking from the shared front-swatch field. Both
builds start from `62b01a884`. APK and native-library hashes and raw records are
in `artifacts/android-color-overlap/provenance.json` and `motion-summary.json`.

Three warmed five-second gestures per device use the existing color-panel
fixture and its small default document:

| Input | Before, UI frames/s | After, UI frames/s |
| --- | --- | --- |
| Mouse | 88.62–89.01 | 88.97–89.05 |
| Touch | 88.83–89.08 | 88.79–89.05 |

Both builds have FrameMetrics vsync-interval p95 of 11.15–11.19 ms and publish
no retained workspace snapshots or panel contents during the valid runs. The
first cold baseline run failed that retention assertion with two snapshots;
its record is retained separately. This comparison uses a non-reference device,
a small document and p95 rather than p99, so it does not qualify the 24 MP target.

## Language-change diagnostic

Measured on the Wacom MovinkPad 11 on 2026-10-02 with `19a086a37`, release Rust
in the unminified Android benchmark APK. The 6000 × 4000 Perceptual photo has an
empty drawing layer above it and visible Paper below. Navigator is open, panel
glass uses its default and Fit zoom is 15.99%. G-Pen uses 1536 px and pressure 1.
Three warmed five-second strokes per scenario follow an ellipse with screen
semiaxes 310 × 150 px; the whole brush footprint stays at least 46.9 px inside
the canvas. The priming stroke is undone. Thermal status is zero before and
after the runs.

| Same-build workload | Median fresh GPU updates/s (range) | Completion-gap p99 across runs |
| --- | ---: | ---: |
| Steady language | 55.40 (47.00–57.40) | 30.98–51.27 ms |
| Language requested halfway through the stroke | 56.80 (52.40–56.99) | 32.08–34.97 ms |

Preparation takes 8.81–11.23 ms. Publication waits for pen-up; pen-up to observed
published model takes 222.51–303.51 ms. The harness resumes input within
0.014–0.025 ms of observing publication. Consumed input to the first resumed
GPU submission takes 14.06–19.30 ms, and to completion 57.00–65.15 ms.

The 90/s and 22.2 ms criteria are not met. Overlapping run ranges and the slower
first steady run establish no improvement or regression. This compares two
scenarios in the candidate, not a previous revision. Synthetic owner replay
does not qualify OS input, display scanout or a real pen. Visible Paper also
differs from the official brush fixture, which hides it. UI publication frame
cadence remains unmeasured.

APK SHA-256 is
`3f7fe53170f5a8282b2766bd5cc828c6a9c971148956b76561809abf777a7553`.
Raw records are under `artifacts/localization-live-switching/android/viewport-tier/`;
`viewport-tier-language-summary.json` in its parent directory records the
samples, geometry, layers and publication/resume intervals.

## Spatial composition comparison

Measured on 2026-10-02 UTC on the Wacom MovinkPad 11, Mali-G57 MC2, with
release Rust in the benchmark APK. The baseline is `f820bb89c`; the optimized
build is based on `864629ed8`. Its APK SHA-256 is
`bbce2e5cb432c2155cfec07eb85c21ea190696dfd7813b6f9a48897e9e7ffd5f`.
The 6000 × 4000 reference photo has an empty paint layer above it; Navigator
and the filter properties are open, Stats is closed, and panel glass uses its
default. Gaussian slider gestures have one priming drag and three warmed
five-second moving drags per range and camera. Thermal status is zero before
and after every run. The measured sigma ranges are 2.6–4.9 document pixels
(small) and 14.2–16.6 (large). The physical surface is 2200 × 1440, with a
1150 × 1272 work area. The 50% camera is translated by (-341, -207); Fit is 17.25%.

| Gaussian workload | Before fresh canvas updates/s, median (range) | After fresh canvas updates/s, median (range) | GPU composition p50 before → after |
| --- | ---: | ---: | ---: |
| 50%, small radius | 0.40 (0.40–0.40) | 7.17 (7.17–7.18) | 1796.89 → 116.20 ms; 15.46× |
| 50%, large radius | 0.20 (0.20–0.20) | 4.56 (4.56–4.59) | 4209.94 → 195.17 ms; 21.57× |
| Fit, small radius | 37.34 (29.31–38.27) | 38.89 (38.50–43.64) | Not traced |
| Fit, large radius | 25.08 (24.52–25.31) | 26.12 (25.32–26.12) | Not traced |

Canvas rates count changed raster frames queued and GPU-completed within the
motion window. They are not presentation rates. UI rates come from
`FrameMetrics`, which also include controls moving while the canvas waits.
The two 50% ranges show 58.7–59.3/s and 40.3–41.8/s UI frames after optimization;
the baseline shows 57.1–57.7/s and 42.2–42.8/s. The display holds 60 Hz, and
neither range meets the 90 fps target. These results do not establish a soft
target waiver. A separate traced drag supplies the GPU medians; observations
are matched to raster frame IDs queued during motion, including timing counters
published after release. Baseline GPU samples are sparse (two small, one large)
because its full-image fallback takes seconds per update.

At 50%, the output window is 2560 × 1792 texels. The input window includes the
sum of the chain's declared sampling radii; global effects retain whole-image
dependencies. A successful standalone calibration measured both Gaussian
passes at sigma 3, 8 and 21 on a 2048² RGBA32Float image, with 512² scissor
passes, four warmups and 30 retained samples per direction and radius.
Interpolating those per-pixel costs at half the native radius, adding three
copy-equivalent composition operations and 10–20 ms for overview/mip work,
predicts 103–113 ms for the small range and 206–216 ms for the large range.
Measured composition is within about 12% of the corresponding predicted
speedups. The coefficients were measured independently of the slider journey.
This is an approximate cost model, not a bound on GPU scheduling contention.

The retained-photo controls use three warmed five-second native-input gestures:

| Control | Before | After |
| --- | ---: | ---: |
| Pan, completed canvas updates/s, median | 59.94 | 60.05 |
| Pan, completion gap p99, range | 17.46–17.58 ms | 18.79–19.28 ms |
| G-Pen 1536 px, fresh/input updates/s, median | 55.28 | 54.78 |
| G-Pen 1536 px, input completion gap p99, range | 29.84–32.07 ms | 30.98–34.40 ms |

The G-Pen path is a contained 310 × 150 px ellipse at 15.99% Fit, with 200 Hz
stylus input and 16 ms prediction. These controls show no material throughput
regression; the brush remains below its 90 updates/s guarantee. Pan records GPU
completion rather than scanout, so it does not qualify the display-paced target.
The final Fit medians are slightly higher. No separate ROI speedup is expected
when the whole photo is visible.

Raw runs, traces, calibration, cost model and validation logs are retained in
`artifacts/bounded-spatial-effects-results/`. The shared pixel oracle verifies
chained filters, masks, clipping, offscreen damage, both blend spaces and exact
refinement. The actual tablet viewport admits the 24 MP single-filter window
under the 608 MiB composition-cache limit; total renderer storage includes
additional source and paint allocations and is not bounded by that cache limit.

## BUILD20 G-Pen comparison

Measured on 2026-10-04 UTC with the clean `4a2cf6aa0` baseline and the M3
candidate based on the same revision. Both are benchmark APKs with release Rust.
The tier photo has one drawing layer above it, Paper hidden, Linear blending,
Navigator open, Stats closed and default glass. Pressure is 1 and prediction is
enabled. A priming stroke is undone before three warmed ten-second strokes with
200 Hz OS-injected stylus input. Both before/after thermal samples are zero.
The matched Fit zoom is 15.9900%, with screen semiaxes 310 × 150 px; the
nominal full brush tip remains at least 46.90 px inside the photo.

Rates count completed nonempty updates consuming new paint input inside the
contact, excluding refinement-only completions. Response is the latest consumed
input event to GPU completion; its p99 differs from the intercompletion gap.
Neither metric establishes physical pen latency or screen presentation.

| Build / stroke | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline 1 | 59.067 | 28.304 | 58.165 | 10.016 |
| Baseline 2 | 59.393 | 27.050 | 54.822 | 9.825 |
| Baseline 3 | 59.128 | 26.423 | 56.324 | 9.803 |
| Candidate 1 | 58.890 | 27.519 | 56.363 | 10.096 |
| Candidate 2 | 59.327 | 25.264 | 57.640 | 10.058 |
| Candidate 3 | 59.790 | 25.503 | 57.149 | 10.003 |

Both builds miss 90 fresh updates/s and the 22.2 ms gap limit at 1536 px.
Candidate rate changes range from −0.30% to +1.12%, with owner CPU p95 growth
of 0.80–2.37%. Response p99 changes by −1.802 / +2.818 / +0.826 ms; the second
stroke exceeds the +1 ms bound, while the first improves. The samples do not
resolve a common response pass or a repeatable regression.

Accounted renderer residency is 1466.523 MiB candidate versus 1467.523 MiB
baseline. Both retain 384 decoded source slots.

### Drawing in the middle of sixteen photo layers

This additional matched 1024 px workload has sixteen photos plus the drawing
layer: seventeen visible layers, with eight photos above and eight below the
paint layer. Photo copies have 35% opacity over the opaque base photo. Other
settings match the guarantee run. Its nominal full-tip margin is 87.83 px.

| Build / stroke | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline 1 | 75.333 | 18.901 | 41.797 | 10.456 |
| Baseline 2 | 74.592 | 19.177 | 42.259 | 10.558 |
| Baseline 3 | 75.477 | 18.473 | 40.982 | 10.311 |
| Candidate 1 | 74.304 | 19.244 | 44.138 | 10.501 |
| Candidate 2 | 74.861 | 19.527 | 44.399 | 10.420 |
| Candidate 3 | 75.047 | 18.236 | 40.778 | 10.189 |

Both builds miss 90 updates/s. Candidate throughput changes by
−1.37 / +0.36 / −0.57%, and owner CPU p95 remains within 5%. Response p99
changes by +2.341 / +2.140 / −0.204 ms: two strokes exceed +1 ms, so the common
response bound is not cleared. The adverse repetitions remain part of the
comparison. Accounted renderer residency is 1709.094 MiB in both builds.

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

## Brushes

Target: **90 completed updates/s** at the guaranteed size, on the 24 MP canvas.
Simple brushes are guaranteed through **1536 px**; complex and very complex
brushes retain their 1024 px and 512 px guarantees.

### JNI cleanup comparison

Measured on 2026-09-30 on the MovinkPad 11: 6000 × 4000, G-Pen 1024 px at Fit,
default workspace with Navigator, prediction enabled and OS-injected 240 Hz input.
Each batch contains three warmed five-second strokes; batches alternate builds.
Baseline source is `c993253ca`, APK SHA-256
`a59fa1c30bf260f8edec0bbf0354bfd342d55f9ba2eaab6a55e0ee717342072b`.
The JNI cleanup builds on `829f1e223`, APK SHA-256
`3a05ab028cc7bafb409313ad23741c403b643c277dfc235a4f2fb30194599e24`.

| Metric | Baseline | JNI cleanup | Repeated baseline | Repeated cleanup |
| --- | ---: | ---: | ---: | ---: |
| Median fresh completed canvas updates/s | 166.1 | 164.4 | 166.2 | 165.7 |
| Median per-run p99 fresh completion gap | 14.92 ms | 14.98 ms | 14.74 ms | 14.71 ms |
| Input to GPU completion p99 | 28.01 ms | 27.97 ms | 27.82 ms | 27.48 ms |
| Owner CPU p99 | 6.20 ms | 6.01 ms | 6.08 ms | 5.95 ms |

Individual rates overlap: baseline 163.2–169.6 and cleanup 164.1–170.0 updates/s.
This empty-canvas 1024 px workload meets the 90 updates/s and 22.2 ms gap criteria;
the 1536 px guarantee and display-paced motion need their own measurements.

### Shared geometry comparison on Huion

Measured on 2026-09-30 on Huion KP1202: 4248 × 2832, 512 px watercolor brushes,
Fit, 100 × 70 px ellipse, 16 ms prediction, Stats closed and 200 Hz stylus input.
Each alternating batch contains three warmed ten-second strokes. Baseline
source is `5b514bbd`; benchmark APK SHA-256
`f14abb270b0e3a654de9a22cacb6b8f568def358ecab7b30809cdded21fe2944`.
The shared geometry/comment cleanup APK SHA-256 is
`84a27bef3875899f1eea4ad38e894489e83ba7d20d74d48b39d342fe249e077a`.
Both APKs have identical Java code and startup profiles.

| Metric | Before | After | Repeated before | Repeated after |
| --- | ---: | ---: | ---: | ---: |
| Watercolor Wash, median fresh updates/s | 3.80 | 3.69 | 3.60 | 3.80 |
| Watercolor Wash, median completion gap p99 | 461.3 ms | 471.2 ms | 470.3 ms | 433.5 ms |
| Wet Watercolor, median fresh updates/s | 4.19 | 3.99 | 4.09 | 3.99 |
| Wet Watercolor, median completion gap p99 | 407.0 ms | 482.2 ms | 451.9 ms | 459.3 ms |

Run ranges overlap; the samples do not establish a throughput change. This
12 MP comparison on Huion does not qualify the 24 MP reference-tier targets.
Reports: `artifacts/simplification-cleanup/performance/m28/`.

### Retained-photo brush measurements

Except for G-Pen, measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets. The ellipse is 520 × 299 px at 16.0% zoom.

The older 2048 px simple-brush rows are above the 1536 px guarantee and do not
classify performance at the guaranteed size. Only G-Pen is remeasured at 1536 px;
the rest of the simple class remains unqualified there.

| Brush (id) | Class | Size | Measured | Status |
| --- | --- | --- | --- | --- |
| G-Pen (1) | Simple | 1536 px | BUILD20: 58.890–59.790 fresh updates/s; fresh gap p99 25.264–27.520 ms, Linear | **Not met**; [BUILD20](#build20-g-pen-comparison) |
| Rough G-Pen (28) | Simple | 2048 px | 12.2 updates/s (12.0–12.2); gap p99 152.2 ms | 1536 px unmeasured |
| Calligraphy Pen (29) | Simple | 2048 px | 35.2 updates/s (35.1–35.3); gap p99 73.2 ms | 1536 px unmeasured |
| Antique Pen (30) | Simple | 2048 px | 16.2 updates/s (16.0–16.3); gap p99 171.2 ms | 1536 px unmeasured |
| Realistic Pen (31) | Simple | 2048 px | 13.4 updates/s (13.4–13.5); gap p99 131.6 ms | 1536 px unmeasured |
| Wet Ink (32) | Simple | 2048 px | 11.0 updates/s (10.9–11.1); gap p99 168.6 ms | 1536 px unmeasured |
| Pencil (2) | Simple | 2048 px | 5.3 updates/s (5.3–5.4); gap p99 225.4 ms | 1536 px unmeasured |
| Pointy Pencil (25) | Simple | 2048 px | 5.5 updates/s (5.4–5.5); gap p99 222.4 ms | 1536 px unmeasured |
| Shading Pencil (26) | Simple | 2048 px | 14.6 updates/s (14.5–14.9); gap p99 89.2 ms | 1536 px unmeasured |
| Charcoal (27) | Simple | 2048 px | 4.4 updates/s (4.3–4.4); gap p99 295.9 ms | 1536 px unmeasured |
| Chalk (6) | Simple | 2048 px | 3.8 updates/s (3.7–3.8); gap p99 324.4 ms | 1536 px unmeasured |
| Eraser (3) | Simple | 2048 px | 7.1 updates/s (7.0–7.2); gap p99 174.5 ms | 1536 px unmeasured |
| Airbrush (5) | Simple | 2048 px | 9.7 updates/s (9.7–9.9); gap p99 135.9 ms | 1536 px unmeasured |
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

## G-Pen at the 1536 px guarantee

Measured on 2026-10-01 UTC on the Wacom MovinkPad 11 at `2e7dd29f4`:
6000 × 4000 reference photo beneath one empty paint layer, Perceptual blending,
Fit, 310 × 150 px ellipse, 200 Hz injected stylus samples and 16 ms prediction.
The optimized release benchmark uses the default workspace with Navigator and
Stats closed. Each size has a priming stroke undone followed by three warmed
five-second strokes. Tracing and memory sampling are disabled; all before/after
thermal-status readings are zero. Both sizes use the same APK and setup.
The observed zoom is 15.99%. The path leaves at least 5.96 surface pixels
between the full 2048 px brush footprint and the photo edge; the 1536 px
footprint leaves at least 46.90 pixels.

| G-Pen diameter | Fresh completed updates/s, median (range) | Completion-gap p99, range | Settling after pen-up, median | Qualification |
| --- | ---: | ---: | ---: | --- |
| 1536 px, guaranteed size | 55.64 (54.86–56.06) | 31.87–33.45 ms | 540 ms | Below 90/s; gap exceeds 22.2 ms |
| 2048 px, above guarantee | 33.36 (33.11–33.47) | 45.61–56.32 ms | 708 ms | Above-guarantee comparison |

Reducing diameter raises this stroke's fresh throughput by 66.8%. The 1536 px
row still needs 61.7% more throughput to reach 90/s, along with shorter tail
gaps. This size comparison is not a renderer-code speedup or a hardware
impossibility proof. It does not qualify other trajectories, the rest of the
simple-brush class or 90 Hz screen presentation. The previously measured
2048 px refinement and contact latencies below retain their original conditions.

The wider 479.7 × 299 px path clips the brush at the photo edges. The same APK
reaches 71.59 fresh updates/s at 1536 px and 48.03/s at 2048 px on that path,
with completion-gap p99 24.94–28.21 and 35.65–37.79 ms. Those are diagnostic
comparisons and do not qualify the contained-footprint workload required by
[the measuring guide](measuring.md#rules). The older wide-path 2048 px records
below also retain that distinction.

Optimized benchmark APK SHA-256:
`eb91ff69cafd3560603e242fda1e46af9f372d97b4004442ed95927cc1ffd2cc`.
Raw records, immutable APK and build provenance:
`artifacts/mid-tier-1536/contained-gpen-{1536,2048}`, `contained-results.json`
and `provenance.json`. The clipped diagnostics remain in `gpen-{1536,2048}`.

## G-Pen above the 1536 px guarantee

Measured on 2026-09-30 against `ba8835fec`: the tier photo beneath one paint
layer, Perceptual blending, 2048 px G-Pen, Fit, 16 ms prediction, default
workspace with Stats closed, warm-up and three five-second strokes. Thermal
status is zero. This historical front-stack and covered-pixel candidate raises fresh input
throughput from 37.72 to 49.02 updates/s. Completion-gap p99 is
32.55–34.35 ms. The 90/s rate target and 22.2 ms gap target remain open.
This does not qualify the class or its other brushes.
Settling increases from 671–679 ms to 787–811 ms; smaller idle-refinement
batches trade completion time for admission of fresh input.

Candidate: `c452a0642` (production source matches `fc5d00fd5` after the
test-fixture rebase), optimized benchmark APK SHA-256
`865b0dd05b8253b3b22eac806136023a5befeac5b642e5fddb2f3cd24194d047`.
Raw records: `artifacts/optimization-roi/{current-main,final}-mid-fit`.

### Layer-neutral composition and refinement

Measured on 2026-09-30 on the MovinkPad 11: 24 MP Perceptual photo, G-Pen
2048 px at Fit, 479.7 × 299 px path, 16 ms prediction, default workspace,
Stats closed and thermal status zero. Three warmed five-second strokes in the
selected balanced-root, four-page build reach 48.04 fresh updates/s
(47.90–48.08), with completion-gap p99 36.78–38.06 ms. The 90/s and 22.2 ms
targets remain open.

The controlled two-page build reaches 47.96/s. Restoring four-page batches
shortens median settling from 903 to 687 ms, while median resumed-contact
GPU completion changes from 89.1 to 92.4 ms. Allowing fresh-input submission
behind one unfinished batch reduces median submission from 48.5 to 27.8 ms
and GPU completion from 112.6 to 92.4 ms against the four-page gated control.
Continuous-stroke rates are unchanged. The cap is restored to four; queueing
remains. See [responsiveness](responsiveness.md#refinement-batch-tradeoff) for
the complete controls and limits.

Source base: `1557688aa` plus the layer-neutral root and four-page cap changes.
Optimized release benchmark APK SHA-256:
`05d747e5f3e6417e9ea3a96a0d1b5bfe38a3aca00744891bbdd01c9a37334a8b`.
Raw records: `artifacts/refinement-tradeoff/four-overlap-mid-constant`;
the source patches and other controls are in the same artifact directory.

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
directly into presentation when Navigator is closed. In these measured builds, a
visible Navigator shares a materialized composition with the main canvas to
preserve area-filtered preview quality. Opening its panel also changes the Fit camera and work area;
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

## Pinned localization comparison

Measured on 2026-10-01 PDT / 2026-10-02 UTC on the Wacom reference tablet,
comparing `271918681` with source tree `e56742a5`. The release APK hashes and
attribution limits are in the [low-tier comparison](low-tier.md#pinned-localization-comparison).
These observations do not qualify the later GPU-bounds successor.

The 6000 × 4000 Sony photo has one empty paint layer, Perceptual blending,
Fit zoom 15.99%, default Navigator and glass, Stats closed and 16 ms prediction.
Three warmed ten-second OS stylus strokes use G-Pen 1536 px and a contained
310 × 150 px trajectory. Settings, camera and visible layers match; painting
thermal status is zero before and after each run.

| Source sequence | Fresh completed updates/s | Maximum fresh gap p99 | 90 fps / 22.2 ms criteria |
| --- | --- | --- | --- |
| Baseline | 55.54–55.90 | 30.61 ms | Not met |
| Candidate | 55.81–56.45 | 29.55 ms | Not met |
| Repeated baseline | 55.49–56.17 | 28.86 ms | Not met |

The ranges overlap; no regression is demonstrated for this stroke. Both
versions miss the target. This does not qualify other brushes or scanout.

| Numeric motion, three warmed ten-second drags | Baseline UI Hz / maximum p99 | Candidate UI Hz / maximum p99 | Raw canvas completions/s, baseline → candidate |
| --- | --- | --- | --- |
| Exposure | 59.20–59.50 / 16.77 ms | 59.20–59.40 / 16.77 ms | 32.05–32.95 → 31.08–32.42 |
| Chain Exposure | 59.00–59.30 / 33.50 ms | 59.20–59.30 / 16.79 ms | 24.46–24.77 → 24.05–24.67 |

The display holds 60 Hz and neither numeric row meets 90 fps. UI frames and
raw completions do not establish fresh photo previews: completion records
include Navigator and lack effect input/revision pairing. The low-tier numeric
decrease and its attribution limits remain explicit. Numeric thermal status
was zero before every gesture; the final after-snapshot was not captured.
