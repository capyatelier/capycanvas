# Low tier: 60 fps on 12 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: TCL TAB 11 Gen 2 (9465X) with a 1200 × 1920, 60 Hz panel. The canvas
is 4248 × 2832.

- Every row targets **60 fps** unless marked soft.
- Geometry measurements use the 12 MP photo at Fit zoom. Older UI-only rows
  name their own canvas sizes.

## Operations

The BUILD32 G-Pen pair measures the M3 candidate captured on `192601dac`.
BUILD28 and selected BUILD20 canvas diagnostics measure frozen `4a2cf6aa0`
binaries. Earlier operation rows and 83/BUILD15 probes apply to their named
binaries. Each row retains its presentation and scope limits.

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

Shared codec diagnostic, 2026-10-04, desktop release builds: the committed
`b3f6f8e51` writer versus the manifest-compression follow-up on the same authored
input. A 2,048-fill metadata fixture shrinks from 2,951,109 to 461,626 bytes;
a real GTK recovery file shrinks from 5,417 to 3,570 bytes. Independent ZIP
reads preserve every decoded manifest byte, portable resource identity and
encoded resource range. Pixels and channels are not recompressed.

Seven metadata-only repetitions reduce preparation allocations from 913,050 to
646,741 and phase net heap growth from 82,273,847 to 45,559,412 bytes. Median
prepare/write/open times are 91.04/0.75/92.94 → 77.20/40.33/103.72 ms: compression
trades worker time for smaller files. The small GTK file uses
0.063/0.011/0.185 → 0.052/0.074/0.221 ms. These sequential codec diagnostics
include allocator instrumentation where enabled, exclude fsync and do not
qualify frame timing, total process memory or tablet latency. Full samples and
exact source/binary identities are under
`artifacts/format/m3-metadata-compression-44/`.

Decoded-source ownership diagnostic, 2026-09-29: the 12 MP photo transform
sequence retained 204 decoded tiles throughout after sharing the renderer's
source cache (APK `e91330b3a54e549712e60a89e5727ce31a5c604f1a6421fb5c2edcb119cae867`).
Before this fix, pixel translation retained 1,616 tiles and 2,098 MiB of total
GPU allocations; afterwards it retained 204 tiles and 686 MiB. The full sequence
completed with at least 2.2 GiB system memory available. Allocator and PSS sampling
make this a memory diagnostic, not frame-rate qualification. Records are under
`artifacts/latency-investigation/{current-geometry-4,fixed-geometry-memory-2}`.

| Operation | Target | Measured | Source |
| --- | --- | --- | --- |
| Tool cursor hover, with and without brush size | 60 | Unmeasured on the reference tablet | [Top-tier rendering measurements](top-tier.md#tool-cursors) do not qualify this tier |
| Pan during private session checkpoints | 60 | Unqualified: renderer 60.00–60.12 submissions/s, interval p99 18.70–18.92 ms; screen presentation and thermal status unmeasured | [Session checkpoints](#session-checkpoints) |
| G-Pen 1024 px during private session checkpoints | 60 | Unqualified: 77.6–79.0 fresh completed updates/s, completion gap p99 23.36–24.07 ms; thermal status unmeasured | [Session checkpoints](#session-checkpoints) |
| G-Pen 1024 px, repeated short contacts during checkpoints | 60 | Diagnostic only: 11.02–14.27 MiB process writes per 50 contacts; observed head age max 3.52–4.37 s. Paused motion has no qualified frame rate | [Session checkpoints](#session-checkpoints) |
| Pan: Hand tool, one or two fingers | 60 | Photo, two fingers, Navigator open: screen 59.31 presents/s, p99 ≤16.70 ms; renderer 59.71 fresh completed updates/s | [Solid Color fills](#solid-color-fills) |
| Pinch zoom | 60 | Photo, Navigator open: screen 59.43 presents/s, p99 ≤16.86 ms; renderer 59.83 fresh completed updates/s | [Solid Color fills](#solid-color-fills) |
| Pan: Hand tool, one or two fingers, M3 BUILD20 | 60 | **Unqualified on current M3.** 59.078–59.729 completed updates/s; completion gap p99 18.821–19.033 ms; presentation unmeasured | [BUILD20 selected canvas comparison](#build20-selected-canvas-comparison); older actual presents below |
| Pinch zoom, earlier retained-Navigator revision | 60 | Photo, Navigator open: screen 59.40 presents/s, p99 ≤16.83 ms; viewport 59.90 fresh completed updates/s | Retained-Navigator navigation below |
| Two-finger rotate | 60 | | |
| Footer zoom and rotation sliders | 60 | Unmeasured on the reference tablet | |
| Navigator drag | 60 | | |
| Brush-cursor hover | 60 | | |
| Placed-photo translation | 60 | Navigator closed: screen 59.3 presents/s, p99 16.9 ms; renderer 122.9 completed updates/s | Earlier direct-presentation comparison below, `photo-translate-drag` |
| Placed-photo corner resize | 60 | Navigator open: screen 59.4 presents/s, p99 ≤17.0 ms; renderer 104.2 fresh completed updates/s | Current drag comparison below |
| Pixel transform corner resize: Free | 60 | Navigator open: screen 59.5 presents/s, p99 ≤16.88 ms; renderer 149.7 fresh completed updates/s | Reduction-encoder cleanup verification below |
| Pixel transform: Uniform, Skew or Rotate | 60 | | |
| Pixel transform translation | 60 | Navigator closed: screen 59.5 presents/s, p99 16.7 ms; renderer 175.1 completed updates/s | Earlier direct-presentation comparison below, `photo-pixels-translate-drag` |
| Pixel transform: Distort | 60 | Navigator open: screen 59.4 presents/s, p99 ≤16.89 ms; renderer 133.4 fresh completed updates/s | Reduction-encoder cleanup verification below |
| Pixel transform: Perspective | 60 | | |
| Pixel transform: Warp | 60 | Navigator open: screen 59.5 presents/s, p99 ≤16.9 ms; renderer 66.7 fresh completed updates/s | Current drag comparison below |
| Crop corner drag | 60 | Navigator closed: screen 59.2 presents/s, p99 16.7 ms; renderer 130.1 completed updates/s | Earlier direct-presentation comparison below, `crop-handle-drag` |
| Pixel resize after placing the photo at 45% size | 60 | Navigator closed: screen 58.6 presents/s, p99 16.8 ms; renderer 224.6 completed updates/s | Earlier direct-presentation comparison below, `scaled-photo-pixels-handle-drag` |
| Selection translation, full canvas | 60 | Renderer 136–139 submissions/s; GPU interval p99 14.3–17.3 ms (6000 × 4000) | Canvas-bar `selection-handle-drag` and `selection-distort-drag`, 2026-09-27 |
| Move tool layer drag | 60 | | |
| Move selected pixels: whole image | 60 | Screen 59.1 presents/s, p99 16.8 ms; renderer 81.0 completed updates/s | Two-page-refinement qualification below, `move-all-drag` |
| Move selected pixels: partial selection | 60 | Screen 59.4 presents/s, p99 16.8 ms; renderer 134.2 completed updates/s | Two-page-refinement qualification below, `move-part-drag` |
| Move selected pixels: Leave Copy | 60 | Screen 59.3 presents/s, p99 16.7 ms; renderer 131.1 completed updates/s | Two-page-refinement qualification below, `move-part-leave-copy-drag` |
| Marquee, Lasso or Polygon drag | 60 | | |
| Selection Brush or Quick Mask, 1024 px | 60 | | |
| Grow, Shrink or Feather drag, full canvas | 60, soft | | |
| Pointwise adjustment slider: Exposure | 60, soft | **Not met.** Current M3 12.131–32.686 completed updates/s; completion gap p99 63.347–281.970 ms; presentation unmeasured | [BUILD20 selected canvas comparison](#build20-selected-canvas-comparison) |
| Pointwise chain: Levels, Vibrance, Exposure slider | 60, soft | **Not met.** Screen 57.3 presents/s, p99 33.3 ms; renderer 25.1 completed updates/s, with a 13.9–31.7 range | Pointwise graph comparison below |
| Threshold slider | 60, soft | **Not met.** Current M3 3.362–3.789 completed updates/s; completion gap p99 260.867–338.872 ms; presentation unmeasured | [BUILD20 selected canvas comparison](#build20-selected-canvas-comparison) |
| Other pointwise adjustment sliders | 60, soft | | |
| Gaussian Blur slider, small radius | 60, soft | **Not met.** Screen 50.0 presents/s, p99 33.4 ms; renderer 23.5 completed updates/s | Two-page-refinement qualification below, Navigator open |
| Gaussian Blur slider, large radius | 60, soft | **Not met.** Screen 42.7 presents/s, p99 50.0 ms; renderer 15.5 completed updates/s | Two-page-refinement qualification below, Navigator open |
| Saved spatial lengths beyond editor bounds, including Gaussian sigma >85 | 60, soft | Unmeasured | Full authored lengths need moving-frame qualification; earlier capped-value measurements do not apply |
| Other neighbourhood filter sliders: Unsharp Mask, Edge-Preserving Smooth | 60, soft | | |
| Denoise Strength | 60, soft | **Not met.** 0.199 completed updates/s; whole-screen proxy 27.86–30.72 presents/s, p99 99.96 ms | [Fixed filter controls](#fixed-filter-controls), 2026-10-04 |
| Domain Warp Distance, animation frozen | 60, soft | **Not met.** 0.199 completed updates/s; whole-screen proxy 54.14–54.32 presents/s, p99 66.64–83.30 ms | [Fixed filter controls](#fixed-filter-controls), 2026-10-04 |
| Saved-selection overlay opacity | 60 | **Unqualified.** 7.78–21.71 completed updates/s; whole-screen proxy 59.21–59.62 presents/s, p99 16.85–33.33 ms; independent canvas presentation unmeasured | [Fixed filter controls](#fixed-filter-controls), 2026-10-04 |
| Animated or warping filter: Domain Warp, Ripple | 60, soft | | |
| Fill layer or gradient-fill edit | 60, soft | **Not met** for Solid Color opacity: screen 47.44 presents/s, p99 ≤49.98 ms; renderer 32.51 fresh completed updates/s. Gradient unmeasured | [Solid Color fills](#solid-color-fills) |
| Navigation with Shadows/Highlights or Clarity | 60 | **Unqualified on current M3.** 52.398–59.826 completed updates/s; first contact below 57/s for both guides; presentation unmeasured | [BUILD20 selected canvas comparison](#build20-selected-canvas-comparison) |
| Navigation with proof or tone guide shown | 60 | | |
| Gradient drag | 60 | | |
| Figure or ruler drag | 60 | | |
| Layer opacity scrub | 60 | **Not met.** Maskless Solid Color over photo: screen 47.44 presents/s, p99 ≤49.98 ms; renderer 32.51 fresh completed updates/s | [Solid Color fills](#solid-color-fills); baseline also misses |
| Layer reorder drag | 60 | | |
| Attached filter drag inside a clipping run | 60 | **Not met.** 55.64–59.72 native UI fps; moving-frame gap p99 16.75–33.34 ms | [Filter attachment feedback](#filter-attachment-feedback), 2026-10-04 |
| Navigation with 8 visible paint layers | 60 | | |
| Drawing with 8 visible paint layers, G-Pen 1024 px | 60 | **Not met.** Navigator open, Fit: 50.12 fresh updates/s (48.65–51.65), completion gap p99 29.30–32.87 ms | Retained-Navigator painting below; seven photo layers and one drawing layer |
| Panel, tab, column or toolbar drag and docking | 60 | Current lifecycle binary unmeasured. Earlier Web checkpoint: group/tab placements 47.38/60.22 Hz; Navigator assertion failed | Web workspace diagnostic below; no tier qualification |
| Panel or column resize | 60 | Color resize unmeasured on reference hardware. Earlier Web checkpoint: width changes 11.93–13.49 Hz; nine retained-resource/geometry checks pass | Web workspace diagnostic below; [GTK Color resize](top-tier.md#gtk-color-panel-resize), 2026-10-04; no tier qualification |
| Drawer open and close | 60 | | |
| Grouped tool menus, drawer switching and tile drag | 60 | Not measured on reference hardware | [Tool variations](../ui/panel-customization.md#tool-variations); functional checks do not qualify this tier |
| Grouped Drawing drawer scrolling | 60 | **Met**, UI FrameMetrics 58.68–59.52 Hz, maximum p99 33.32 ms | [Grouped tool drawer scrolling](#grouped-tool-drawer-scrolling) below |
| Colour wheel or picker drag | 60 | **Unqualified.** Committed-wheel diagnostic: mouse 48.24–51.16 UI Hz, touch 52.80–54.39; maximum p99 49.98 ms (2048 × 1536) | [Live paint icon diagnostic](#live-paint-icon-diagnostic) below |
| Slider scrub: size, opacity, flow | 60 | | |
| Canvas action bar show, hide and move | 60 | **Not met.** UI frame p50/p95: 32.9/41.7 ms moving the bar, 11.6/21.0 ms show and hide (2048 × 1536) | Canvas-bar `ui-bar-move` and `ui-bar-show-hide`, 2026-09-27 |
| Tool Options or panel content change | 60 | **Not met.** UI frame p50/p95 25.4/35.3 ms | Canvas-bar `ui-panel-change`, 2026-09-27 |
| List scrolling: layers, brushes, filters | 60 | | |
| Workspace choices: horizontal scroll | 60 | Screen 59.42–59.82 presents/s, maximum p99 33.24 ms | Workspace switcher scrolling below; long-list fixture |
| Workspace visibility checklist: vertical scroll | 60 | Screen 60.02 presents/s, maximum p99 17.03 ms | Workspace switcher scrolling below; long-list fixture |
| Menu open and close | 60 | | |
| Interface language change | 60 | Current lifecycle binary unmeasured. Earlier German checkpoint: cold publication 169.2–195.5 ms; warm 144.4–194.4 ms; preparation-only maximum 4.095 ms | Matched Web language checkpoint below; no tier qualification |

## Fixed filter controls

Measured 2026-10-04 on the reference TCL tablet with the 4248 × 2832 Sony photo
at Fit, in a release Rust benchmark APK, with three warmed five-second scrubs
per control. Denoise uses a fixed two-pixel neighborhood; Domain Warp uses three
noise octaves and a frozen animation phase. Thermal status was 0 before and
after both builds.

The baseline is `7fcca04e5`, with only the benchmark harness added. The candidate
is the same commit plus the artwork-contract changes, source patch SHA-256
`fbe1e761a0b3b6f5912a58f142aa94ba0533c5b1e2033b3180335b212c8beda2`.
Baseline and candidate APK SHA-256 values are
`571fe6a9150efe14fc27c3dc877b0a868304896f944e0126d8b0335a51396398` and
`c375c0760ad0ded0d328adc3acfcee463ba821be03c4d064ad9d4c9a457811a6`.

| Control | Baseline screen presents/s | Candidate screen presents/s | Baseline / candidate screen interval p99 |
| --- | --- | --- | --- |
| Denoise Strength | 30.21 / 31.55 / 31.13 | 29.50 / 30.92 / 31.44 | 99.88–116.63 / 83.44–99.98 ms |
| Domain Warp Distance | 52.90 / 52.52 / 52.47 | 53.72 / 53.35 / 54.10 | 66.66–83.29 / 66.66–66.81 ms |

Both builds record only one completed renderer update per contact, or
0.198–0.199 updates/s; there are too few completions for a gap p99. The screen
measurements include native controls and do not establish independent canvas
presentation rates: the retained canvas SurfaceFlinger layer reports one event
per contact. Neither build meets the target. This comparison finds no measurable
regression from fixing the kernels; it does not establish a hardware-limit waiver.
Source manifests, APK identities, thermal records and individual samples are in
`artifacts/format-stabilization/android/`, including `baseline/report.json` and
`filter-final-report.json`.

The final production changes were measured again after integrating `bd6532ada`,
with source patch SHA-256
`fa400bde6693b2e3b72c774095c10b90e07dfb488ae38f8bc9c772b64ce00991`,
APK SHA-256
`92e7818f24dccfcfd7d66447b7e39fd4bd4cd177d12ee17b12c1bbc45357b867`
and test APK SHA-256
`3b0ba43ec2b1d1bac03872c5efa57c5ad900fa137f0a71537ba3c621f51b54c6`.
The same three warmed five-second scrubs used the dark theme; light-theme
scrubs passed separately. Thermal status stayed 0, CPU/GPU temperature rose
from 51.1 to 57.6 °C and skin temperature from 31 to 38 °C.

| Control | Completed updates/s | Completion gap p99 | Whole-screen presents/s | Screen interval p99 |
| --- | --- | --- | --- | --- |
| Denoise Strength | 0.199 / 0.199 / 0.199 | Too few completions | 27.86 / 30.32 / 30.72 | 99.96 / 99.96 / 99.96 ms |
| Domain Warp Distance | 0.199 / 0.199 / 0.199 | Too few completions | 54.32 / 54.16 / 54.14 | 66.64 / 83.30 / 66.65 ms |
| Saved-selection overlay opacity | 13.13 / 7.78 / 21.71 | 231.20 / 335.53 / 71.16 ms | 59.21 / 59.62 / 59.41 | 33.33 / 16.85 / 33.31 ms |

Overlay scrubs changed 71, 96 and 54 opacity values while preserving the authored
checkpoint and modified state. Completion counters are diagnostic; they do not
prove independently presented canvas frames or stale pixels. The whole-screen
and native-window traces cannot qualify canvas motion. Native UI presentation
was 59.36–59.44 Hz with p99 35.72–38.75 ms, exceeding the 33.33 ms limit.
The overlay remains unqualified, and neither filter meets its target. Mid and
top tiers remain unmeasured for this change. Raw traces, thermal records and
individual samples are under
`artifacts/format-stabilization/android/integrated-perf/`; its `report.json`
contains the exact values.

## Layer attachment qualification

The current attachment implementation is unmeasured on the TCL reference
tablet. No-effects painting, ordinary clipping and painting above an attached
Gaussian remain unqualified on this tier. The reference tablet was reserved
by another session during the matched mid-tier comparison.

The frozen pre-attachment source, APKs and baseline records remain under
`artifacts/layer-attachment-baseline-a918057/`. They do not qualify the current
implementation; its Gaussian slider also differs in geometry and sampled values.

## Solid Color fills

Measured on 2026-10-03 on the TCL reference tablet, thermal status 0 before
and after each scenario, default 60 Hz display settings and panel glass.
The benchmark uses release Rust, the 4248 × 2832 Sony downscale at Fit,
default Photo workspace and open Navigator. Each scenario has a priming gesture
undone before three native input gestures: six seconds for opacity and five
seconds for navigation and painting. Painting uses Perceptual blending,
G-Pen 1024 px, default prediction and a 480 × 280 px stylus ellipse.

| Motion | Fresh completed updates/s, median (range) | Completion gap p99, range | Screen presents/s, median (range) | Screen gap p99, range | Status |
| --- | ---: | ---: | ---: | ---: | --- |
| Maskless Solid Color opacity over photo | 32.51 (31.54–36.56) | 58.96–103.79 ms | 47.44 (46.16–49.97) | 33.41–49.98 ms | **Not met** |
| Two-finger pan | 59.71 (59.69–59.79) | 18.72–19.31 ms | 59.31 (59.29–59.40) | 16.67–16.70 ms | Met |
| Pinch | 59.83 (59.74–59.91) | 19.21–20.43 ms | 59.43 (59.35–59.51) | 16.68–16.86 ms | Met |
| G-Pen 1024 px | 95.77 (89.67–96.53) | 23.43–27.60 ms | 59.28 (58.46–59.39) | 16.72–17.03 ms | Meets measured stroke criteria |

Navigation and painting place one empty paint layer above the photo and the
new ordinary white fill named Paper. Opacity inserts a maskless Solid Color
above the photo and scrubs its layer control; observed values span 22.1–71.8%.
Screen rates come from distinct SurfaceFlinger display actual-present timestamps
inside the moving windows. Android shared-demand presentation does not emit a
canvas surface frame for every update, so these are screen rates rather than
independently matched canvas presents. Renderer rates count nonempty GPU
completions inside motion; painting additionally requires distinct consumed
real input. This qualifies the measured motions, not every brush diameter or
resumed-contact latency.

The matched opacity baseline at `722eeca2d` uses the same measurement fixture
and control locator. Its fresh rate is 28.38 updates/s (26.22–34.57), completion
gap p99 66.92–80.68 ms, and screen rate 45.31 presents/s (42.72–49.61), screen
gap p99 33.40–50.00 ms. Both revisions miss the opacity target. The higher
candidate median does not establish a general improvement; its worst completion
gap is longer. There is no soft-target waiver.

Candidate APK SHA-256:
`3f96c2549631b0f3c4f240a0ca332d0ff787a701c36118b77537f89225a61904`.
Baseline APK SHA-256:
`acaf4c0e93730a523908ae39a46d96698002a5e967a4a058e8487d5a59cab1936`.
The candidate is `722eeca2d` plus the Paper-to-Solid-Color refactor.
Raw traces, gestures, fixture data, thermal records and binary hashes are in
`artifacts/android-fill-measured/` and `artifacts/android-fill-baseline/`;
`artifacts/analyze-fill.py` and `artifacts/analyze-fill-baseline.py` reproduce
the tables. Mid and top tiers were not measured for this renderer revision.

## Grouped tool drawer scrolling

Measured on the TCL reference tablet on 2026-10-03, thermal status 0 before and
after, 60 Hz default display settings. The 4248 × 2832 Sony photo has one empty
paint layer at Fit (25.5508%), default Photo panels and glass. The grouped Drawing
drawer receives one priming gesture and three six-second native finger gestures.
Actual moving windows span 5.948–5.981 seconds.

| Run | Moving UI FrameMetrics, Hz | Interval p99, ms | Moving draws / matched distinct vsyncs | Maximum draw-to-vsync lag, ms |
| --- | ---: | ---: | ---: | ---: |
| 1 | 58.68 | 33.30 | 350 / 350 | 15.35 |
| 2 | 59.52 | 16.77 | 357 / 357 | 6.31 |
| 3 | 59.02 | 33.32 | 354 / 354 | 13.13 |

Only draws with changed scroll offsets count. Their monotonic timestamps match
the nearest preceding `FrameMetrics` vsync; duplicate callbacks count once.
A 200 ms callback drain retains late metrics without extending the motion window.
All draws match within one frame, with zero lost callbacks or unmatched draws.
These UI FrameMetrics meet the 57 Hz floor and 33.333 ms p99 limit. No
SurfaceFlinger trace was collected; drawer opening, sibling switching, menus,
tile dragging and other tiers remain unqualified.

The unminified benchmark variant uses release Rust from `613ff17a8` plus the
all-tool-groups working tree, identified by exact source hashes in the evidence.
App APK SHA-256:
`81f2ee806046a4f8d70e55ef2b429d877b0e7306173188d781236d51075bdff7`.
Raw gestures, thermal records, fixture, exact hashes and the reproducible report
are retained in `artifacts/all-tool-groups/android/benchmark/`.
The Sony downscale JPEG hash differs from the earlier tool-variations fixture;
this run establishes current scrolling performance, not a matched comparison.

## Live paint icon diagnostic

Measured on the TCL reference tablet on 2026-10-03, thermal status 0 and default
glass, with a blank 2048 × 1536 drawing. Three warmed five-second committed-wheel
gestures per input update the visible paint icon. Both APKs use the same release
Rust library, including the shared paint-pair view; the comparison changes the
Android icon and compact-control presentation.

| Input | Baseline UI Hz range / median | Updated UI Hz range / median | Maximum interval p99, baseline → updated |
| --- | --- | --- | --- |
| Mouse | 49.01–50.23 / 49.60 | 48.24–51.16 / 51.03 | 49.97 → 49.98 ms |
| Touch | 53.34–55.07 / 54.17 | 52.80–54.39 / 53.66 | 33.35 → 33.38 ms |

Both builds miss the 57 Hz floor in this diagnostic. These are distinct intended
UI vsyncs observed through FrameMetrics, with changing paint values checked during
draws. They do not establish actual presentation or qualify the 12 MP photo
workload. Reference-canvas qualification remains outstanding.

The APKs precede the unrelated About dedication integration onto `f38ebc4f1`.
Baseline APK SHA-256 is
`eb5fde3d8cfc839478575a131dc589e2e6df94a9e6d5609de75f1fcc23ced566`;
updated APK SHA-256 is
`9a797e40e91e000da15aa2c610b375c55fdf7fceff564ea43d14a6bd117078a9`.
Raw samples, thermal records, APKs and provenance are in
`artifacts/paint-pair-android/{baseline,final-pre-rebase}/` and the adjacent
`baseline-motion.txt` and `candidate-motion-pre-rebase.txt` logs.

## Workspace switcher scrolling

Measured on the TCL reference tablet on 2026-10-03, thermal status 0 before and
after, 60 Hz default display settings. The 4248 × 2832 Sony photo has one empty
paint layer at Fit (15.9703%), default Paint panels and glass. The fixture uses a
small, left-aligned switcher-only header and 30 custom long names plus the three
included workspaces. Each surface has one priming gesture and three five-second
native touch gestures.

Both measured surfaces meet the 57 presents/s floor and 33.3 ms p99 limit.
Every moving draw sample matches an actual app or popup frame, whose display
token joins the SurfaceFlinger actual-present timestamp. Canvas surfaces and
unchanged draws are excluded. These results qualify this long-list fixture;
other header configurations, opening latency and other tiers remain unmeasured.

The unminified benchmark variant uses release Rust from `f224ef05e` plus Android
patch SHA-256 `5a0abb5b8f59fbef43f7d2dc4fcd09be2dd4fdb9cb46cc053911f369a69fa417`.
App APK SHA-256:
`b58c13f842eb77fd21d72acbff4c4a0b19728d57c563f79c4c9d84d14798803e`.
Raw gestures, trace, analysis, source patch, APKs and provenance are retained in
`artifacts/workspace-switcher-validation/android-switcher-benchmark/`.

## Language preparation

Measured on the TCL TAB 11 Gen 2 on 2026-10-02 with Release Wasm from
`19a086a37`, Chrome 154 and a cross-origin-isolated timer. Three fresh modules
per language cover all five catalogs and verify all 2,904 static labels.
The comparison replaces whole-resource parsing with complete Fluent groups
bounded to 32 entries and 4096 bytes.

| Preparation interval | Whole resources, maximum | Bounded groups, maximum |
| --- | ---: | ---: |
| One step, English already initialized | 2.920 ms | 2.020 ms |
| One step, fresh module | 5.325 ms | 4.190 ms |
| Production Web 2 ms deadline loop, English initialized | Unmeasured | 3.140 ms |

Web startup initializes English before a live switch. The fresh-module maximum
is the first parser/JIT invocation; other fresh-module runs stay at or below
1.990 ms. Total isolated preparation cost does not materially improve: foreign
catalogs take approximately 26–35 ms before grouping and 30–34 ms afterwards.
The smaller indivisible parse work allows more frequent deadline checks.

These elapsed intervals include scheduling and JIT. The batch probe runs the
production Rust deadline loop, excluding publication, view generation and
JavaScript serialization. It has no canvas workload and establishes neither
UI frame rate nor input-to-paint latency. Raw results and source/catalog hashes
are under `artifacts/localization-live-switching/production-preparation-*`;
the comparison and limits are in `preparation-quantum.local.md` there.

## Fifteen-language diagnostic

Measured 2026-10-03 on the TCL reference tablet with release Wasm based on
`155c7e1c0`, Chrome 154 and English initialized. Each of the ten added languages
was prepared once, verifying all 2,972 static labels; German was then reused.

| Interval | Measured |
| --- | ---: |
| Ten cold preparations, summed calls | 18.240–27.400 ms |
| Maximum isolated production batch | 2.355 ms |
| Cached German preparation | 0.030 ms |
| Stock cached English publication / first associated GPU completion | 293.590 / 255.655 ms |
| Candidate cold German publication / first associated GPU completion | 190.340 / 354.650 ms |
| Final preparation call plus shared publication, stock / candidate | 7.220 / 12.805 ms |

Preparation totals exclude waits between tasks and module fetch/instantiation.
Each publication sample precedes one five-second CDP pen motion on the 12 MP
photo. Publication includes host apply; GPU completion is associated with input
at the JavaScript boundary, not physical ink or scanout. The final preparation
call also publishes shared state; its duration is not an isolated parsing batch.
Different language/cache states and single samples establish no improvement or
frame-target result.
Desktop native preparation was 3.010–4.329 ms, a supporting diagnostic rather
than tier hardware. [Raw records and exact binary/source hashes](../../artifacts/localization-expansion/preparation/current155/focused-summary.local.json)
and the [unchanged build/deployment guard](../../artifacts/localization-expansion/preparation/current155/final-focused-sourceguard.local.json)
identify this minimal candidate; earlier broad measurements are excluded.

## Matched Web language checkpoint

Measured on 2026-10-03 on the TCL reference tablet, Chrome 154, release Wasm,
with a 4248 × 2832 photo, one empty paint layer above it and Paper, G-Pen
1024 px, Fit 15.9703%, Navigator open and default glass. The viewport is
1920 × 996 physical pixels, 1129 × 586 CSS pixels. Three fresh app modules per
build each perform one cold and two cached German switches after restoring
English. Camera, workspace, brush, settings, color, layers and contained pen
trajectory match. Thermal status was zero before and after the measured sequence.

| Interval | Baseline | Measured `6318d9c7` checkpoint |
| --- | ---: | ---: |
| Preparation-only call maximum across three cold switches | 3.100 ms | 4.095 ms |
| Cold preparation, sum of calls | 28.34–30.12 ms | 30.91–31.64 ms |
| Cold final call including shared publication | 10.66–12.90 ms | 9.75–11.04 ms |
| Cold host publication | 189.18–202.37 ms | 159.45–184.72 ms |
| Cold total publication | 201.74–215.28 ms | 169.19–195.49 ms |
| Cached total publication | 182.88–230.68 ms | 144.35–194.36 ms |
| Cold first associated GPU completion after resumed pen input | 320.08–346.99 ms | 421.57–557.21 ms |
| Cached first associated GPU completion after resumed pen input | 191.77–380.96 ms | 216.88–366.50 ms |

Cached switches have no preparation-only calls. The final preparation call
publishes shared state and is excluded from parsing maxima. The measured cold
maxima are 2.710, 2.950 and 4.095 ms; these elapsed intervals do not establish
the CPU budget. Publication is faster in this sequence, while cold resumed GPU
association is slower. No general responsiveness improvement is established.
Five-second CDP pen motions after publication reach 16.35–17.66 completed
updates/s cold and 18.95–21.32/s cached. GPU callbacks pair with input at the
JavaScript boundary; they do not measure consumed ink, physical latency or
canvas scanout and do not qualify the tier target.

A separate instrumented checkpoint trace attributes 52.1 ms to style updates
and 26.4 ms to layout within a 213.5 ms cold host publication, with no GC or
compilation span in that window. A copy-only experiment removing repeated tab
and proof refresh did not improve cached timings consistently, so production
preparation scheduling remains unchanged. Trace timings are diagnostic and
are not pooled with the matched runs.

Baseline is `0f6b5b708`; measured checkpoint Wasm SHA-256 is
`6318d9c79e92af194d3f4cef331275dbbdaf237c23399f9c8b0f1c408e4d8933`.
Both binaries' embedded producers identify rustc 1.96.0 and wasm-bindgen
0.2.128. [Repeated raw records and summary](../../artifacts/localization-resolution/performance/final-matched-summary.local.json),
[matched fixture checks](../../artifacts/localization-resolution/performance/final-matched-state-check.local.json),
[deployment and source guard](../../artifacts/localization-resolution/performance/final-deployment.local.json)
and [separate attribution](../../artifacts/localization-resolution/performance/attribution-summary.local.json)
retain exact scope, hashes and mixed results. [Runtime byte equivalence](../../artifacts/localization-resolution/performance/final-stack-runtime-equivalence.local.json)
identifies the same measured production bytes after the shared-host catalog
allocation correction. Earlier single-language samples and intermediate
candidates remain separate checkpoints.

The subsequent lifecycle build, Wasm SHA-256
`2282c166274739cb0715960de5a56b88543d6b83e8d00d4cd58875c9bca1102e`,
is **unmeasured on reference hardware**. Its [deployment and source guard](../../artifacts/localization-resolution/performance/final-lifecycle-deployment.local.json)
verify the exact release runtime; only Wasm differs from the measured
checkpoint. These retained timings do not establish current-binary performance
or qualify any tier target.

The rebased release, Wasm SHA-256
`313264f2fed8eef1285f2d010a820ba6286c18566ccf46eb5c4ab68c4b5d941c`,
also remains unmeasured on reference hardware. Its [runtime and source checks](../../artifacts/localization-resolution/web/acceptance-tool-variations-final.local.json)
qualify the recorded functional journeys only.

## Web workspace diagnostic

The same measured checkpoint and TCL photo fixture above perform three five-second
CDP mouse resizes each for the left column, right column and Navigator. Width
changes sampled through animation callbacks reach 12.04–13.23, 13.03–13.49 and
11.93–12.36 updates/s respectively; baseline samples were approximately
12–14/s. All nine gestures retain DOM/content and native-resolution resources
and pass shared-geometry, undo/redo and cancellation checks.

The first floating-group and tab drags sample 47.38 and 60.22 placements/s.
The Navigator drag then fails the same assertion as baseline: one full
`layout()` call occurs where the existing fixture expects zero. Remaining
repetitions and its trailing glass check do not run. The failed raw probe is
retained. These UI geometry samples have no presented-frame oracle, so neither
motion row is qualified. Narrow transient-open collapsed-column motion remains
unmeasured. [Checkpoint records, partial failure and hashes](../../artifacts/localization-resolution/performance/final-motion-summary.local.json)
and [baseline scope](../../artifacts/localization-resolution/performance/baseline-motion-summary.local.json)
record the bounded comparison; they do not establish unchanged performance
for other workspaces or tiers.

## Current drag comparison

Measured 2026-10-01 on the TCL reference tablet, 4248 × 2832 Perceptual photo,
17.699% Fit zoom, Navigator open, Stats closed and default glass. Both builds
use release Rust, the same work area and three warmed five-second gestures per
row, with thermal status zero. The composed cases transform a translucent photo
over another photo, where direct root presentation is ineligible.

| Journey | Before, fresh completed updates/s | Retained Navigator, fresh completed updates/s | Speedup | Screen presents/s | Screen gap p99 across runs |
| --- | ---: | ---: | ---: | ---: | ---: |
| Pixel transform resize: Free | 73.28 | 150.69 | 2.06× | 59.44 | 16.70–16.84 ms |
| Pixel transform: Distort | 64.52 | 133.97 | 2.08× | 59.41 | 16.70–16.86 ms |
| Placed-photo corner resize | 91.28 | 104.20 | 1.14× | 59.37 | 16.70–16.96 ms |
| Pixel transform: Warp | 62.45 | 66.66 | 1.07× | 59.45 | 16.71–16.88 ms |
| Two-photo composed Free resize | 40.24 | 40.78 | 1.01× | 40.98 | 33.38–50.01 ms |
| Two-photo composed Distort | 33.49 | 34.22 | 1.02× | 34.62 | 33.61–50.00 ms |

Fresh counts match completed submissions to newly consumed host input, excluding
thumbnail-only refreshes; they are not a distinct-transform-pose oracle. Raw
completed rates are 151.09/s for Free and 133.97/s for Distort. Fresh completion
gap p99 spans 9.35–10.01 ms and 9.33–9.96 ms, respectively. Median render-owner
CPU time falls from 6.19 to 3.35 ms for Free and from 5.80 to 3.26 ms for Distort.

The four single-photo rows meet the screen-cadence criteria. SurfaceFlinger
actual-present records include native UI, and this tablet exposes no separate
SurfaceView timeline, so they do not independently establish canvas scanout.
The two-photo composed cases remain below the 60 Hz target. One composed-Free
candidate run has a 55.65 ms completion-gap p99 and 50.01 ms screen-gap p99;
the other two runs have approximately 30 ms and 33.4 ms respectively. The
original samples remain in the comparison.

Three additional full-sequence runs with only scheduler and ART tracing retain
normal throughput: composed Free reaches 40.79, 40.81 and 41.23 fresh updates/s,
with completion-gap p99 29.46–33.00 ms and screen-gap p99 33.36–33.38 ms.
Across all six final-build runs its median is 40.80/s, versus the baseline's
40.24/s. These traces contain no app scopes or GPU diagnostic queries. Their
largest completion gaps overlap running and runnable render-owner time, with
no overlapping app GC pause. This supports unchanged composed throughput but
does not explain the original 55.65 ms tail or establish absence of a latency
regression. Records and aligned scheduler intervals are in
`artifacts/validation/candidate-clock-gc`.

Baseline is `f820bb89c`, APK SHA-256
`645a11a3c86d8d194910f66f50636c6066ac5dd9b5ba867c674ec40fbca879a7`.
The retained-Navigator build includes the shared session-clock correction,
APK SHA-256
`32cf373145f6c356713098726015ebc8454fa46b85e27532ed628dcfa0a876b6`.
Records, raw input/completion samples and complete presentation traces are in
`artifacts/validation/{baseline-final,candidate-clock-final}` in the
`navigator-implementation` worktree; its `candidate-clock-source.patch` records
the measured source. Separate diagnostic captures include phase timing and
memory; they do not qualify the rates above. These transform results do not
establish a general brush improvement or qualify another tier.

### Cost and expected throughput

Separate captures of the same builds and motions isolate the removed work.
Times below are mean GPU intervals; Navigator refreshes are included in total
render time, and main mip generation is included in main composition.

| GPU interval | Free before | Free retained | Distort before | Distort retained |
| --- | ---: | ---: | ---: | ---: |
| Main composition | 8.198 ms | 0.002 ms | 9.672 ms | 0.002 ms |
| Main mip generation | 2.396 ms | None | 2.654 ms | None |
| Total render | 8.243 ms | 0.158 ms | 9.643 ms | 0.175 ms |
| Presentation | 5.039 ms | 6.206 ms | 5.418 ms | 7.052 ms |
| Navigator, per refresh | — | 0.679 ms | — | 0.806 ms |

Rendering and presentation execute sequentially on the same GPU queue. Using
`1000 / (mean render ms + mean presentation ms)` gives an approximate remaining
work ceiling of 157.1 updates/s for Free and 138.4/s for Distort. Qualification
reaches 95.9% and 96.8% of those estimates. The model predicts 2.09× and 2.08×
speedups, compared with the measured 2.06× and 2.08×. These are workload estimates,
not hardware peak claims: intervals include scheduling gaps, bounded timestamp
rings omit some observations, and presentation observations have no matching
render submission ID. The baseline estimates of 75.3/s and 66.4/s are also close
to its measured 73.3/s and 64.5/s.

Navigator refreshes at 18.92 Hz for Free and 18.75 Hz for Distort. CPU refresh
work averages 0.692 and 0.681 ms. After the first refresh, pending artwork age
has p99 59.5 and 58.0 ms, with maxima 62.9 and 59.7 ms; both settle with no
pending thumbnail. The shared session clock ignores zero-time preparation and
preserves the last timestamp across untimed flushes, avoiding device-uptime
rounding that previously reduced the nominal 20 Hz cadence.

The retained image is 266 × 177 pixels, with 753,312 logical pixel bytes and
32 bytes of geometry. Allocated GPU memory increases by 0.78 MiB for Free and
0.80 MiB for Distort; reserved GPU memory is unchanged. This removes repeated
main-image writes, but warm cached allocations remain, so it does not establish
an allocation saving. Cost and memory records are in
`artifacts/validation/{baseline-diagnostic,candidate-clock-diagnostic}`.

### Reduction-encoder cleanup verification

A matched check uses the retained-Navigator build above and the shared reduction
encoder with identical Free/Distort fixtures, three warmed five-second gestures,
Navigator open and thermal status zero. Before rebasing, Free changes from 149.74
to 150.35 fresh completions/s; Distort from 132.94 to 133.16/s. That cleanup APK is
`3c8ee7e43906e68dc06dc2bae43ce1ad8f748ba80991e098b5e3b0753e3c1be1`;
raw runs are in `artifacts/validation/cleanup/{before,after}` in the same worktree.

The final build includes upstream `cb1ad24fb` spatial composition and buffered
Android presentation changes. Three new runs give Free 149.72 fresh completions/s
(135.54–152.39) and Distort 133.36/s (132.97–133.69). Screen rates are 59.49 and
59.40/s, with p99 gaps at most 16.88 and 16.89 ms. Fresh-completion gap p99 ranges
are 9.75–19.82 and 9.97–10.46 ms. Both retain the screen-cadence target; differences
from the saved before-build medians establish no throughput improvement. All slow
Free samples remain in the records. Final APK SHA-256 is
`74d3cf3bd7b06adabc74d321ffb95e6de386827093ab7d4b4f0edf41aee3d7af`;
raw runs are in `artifacts/validation/cleanup/post-rebase/after`.

### Retained-Navigator navigation

The retained-Navigator APK (`32cf373145f6`, before cleanup and buffered-presentation
changes) completes three warmed five-second two-finger pans and pinches on the
12 MP photo with a preparation stroke, Navigator open, Stats closed and 15.9703%
Fit zoom. Pan reaches 59.83 fresh viewport completions/s and a median 59.43 screen
presents/s; pinch reaches 59.90 and 59.40/s. Screen interval p99 ranges from
16.74–16.83 ms for pan and 16.76–16.83 ms for pinch. Both meet the screen-cadence
criterion, subject to the SurfaceFlinger accounting limit above. Fresh completion
gap p99 is 19.09–19.62 ms for pan and 19.94–21.56 ms for pinch.

That build uses FIFO without a retained surface target. The viewport harness
captures presenter GPU timestamps in these runs;
fresh counts use completed host input calls. Records are in
`artifacts/validation/candidate-clock-navigation`, including all three gesture
windows per motion and `matched-summary.json`. These measurements establish
photo navigation at that revision, not a before/after speedup.

### Retained-Navigator painting

Matched three warmed five-second G-Pen strokes use the 12 MP photo, Perceptual
blending, 1024 px size, 200 Hz input, prediction, Navigator open, Stats closed
and default glass. Both builds use 15.9703% Fit zoom and the same 500 × 280 px
ellipse. Its full brush footprint remains inside the photo. The eight-layer
case has one drawing layer over seven photo layers, six at 35% opacity.

| Stroke | Before, fresh updates/s | Retained Navigator, fresh updates/s | Before completion gap p99 | Retained completion gap p99 |
| --- | ---: | ---: | ---: | ---: |
| One photo at Fit | 60.92 (60.49–61.37) | 61.14 (60.69–61.70) | 27.02–28.18 ms | 28.98–29.50 ms |
| Eight visible layers at Fit | 51.47 (49.46–51.68) | 50.12 (48.65–51.65) | 30.22–38.03 ms | 29.30–32.87 ms |

The single-photo stroke meets the 60/s target. The eight-layer case remains
below target; its median falls 2.6%, with overlapping run ranges and a lower
worst p99 completion gap. This does not establish a general brush improvement.
Earlier 100% zoom comparisons are separate workloads and do not qualify Fit.

Optimized baseline APK SHA-256 is
`cea0ded9608d7543f53da150275cf7593551eeeb1503c064a3d9ed71264e636a`;
the final optimized APK is
`0a3ca41f35806af34f642e46a8e10194746768abf3e5b6e724ceaa59eb333814`.
Both use release Rust. Records are in
`artifacts/validation/{baseline-brush,candidate-clock-brush}`, with native
consumed-input traces, completed submissions, geometry and final screenshots.

## Earlier direct-presentation comparison

Measured 2026-09-29 on the 12 MP Perceptual reference photo at Fit: release Rust,
default glass, Stats closed and three warmed five-second gestures per row.
Thermal status is zero before and after all runs. Both renderers use the same
600 ms warm-up through the complete motion range and the same test APK.
The harness explicitly sets Navigator visibility and asserts the overview count.
Opening Navigator also changes the work area and Fit camera, so each comparison
pairs the same panel configuration. Values are medians across runs.

| Journey | Before, Navigator closed | Direct presentation, Navigator closed | Before, Navigator open | Direct presentation, Navigator open |
| --- | ---: | ---: | ---: | ---: |
| Placed-photo translation | 96.45 | 122.86 | 83.30 | 98.48 |
| Placed-photo corner resize, bar hidden | 94.82 | 113.20 | 82.37 | 89.27 |
| Placed-photo corner resize, bar visible | 96.58 | 113.25 | 90.59 | 89.88 |
| Pixel transform translation | 89.41 | 175.10 | 79.47 | 80.30 |
| Pixel transform resize: Free | 87.82 | 169.59 | 78.80 | 72.47 |
| Pixel transform: Distort | 85.92 | 152.25 | 77.33 | 63.85 |
| Pixel transform: Warp | 46.53 | 70.03 | 38.64 | 61.54 |
| Crop corner drag | 129.23 | 130.05 | — | — |
| Pixel resize after 45% placement | 117.78 | 224.55 | — | — |

Values are GPU-completed nonempty updates/s. In these earlier builds, single-input
affine and perspective previews sample the retained source directly during
presentation when Navigator is closed, avoiding the intermediate composition. With Navigator open, both
views share the existing materialized composition and preserve preview quality.
Placement drags also defer exact refinement until Apply or Cancel.

All 27 closed-Navigator gestures and all 21 open-Navigator gestures pass the
60 Hz screen-cadence criterion. Closed-panel median screen rates span
58.64–59.46 presents/s, with interval p99 16.71–16.94 ms. The old Warp fails in
both configurations. Closed-panel pixel transforms and placement exceed the old
compositor, including crop after specializing presentation by source type.
In these builds with Navigator open, Distort is 17.4% slower, Free resize 8.0% slower
and visible-bar placement resize 0.8% slower. Meeting screen cadence does not
close these throughput gaps.

Screen rates use SurfaceFlinger actual-present timestamps. This device provides
no separate SurfaceView timeline, so screen records do not independently
establish canvas scanout. Renderer counts exclude empty updates and terminal
polling. Setup waits for command availability outside timing. No allocator or
process-memory sampler runs inside qualification; a separate safety guard
observes system available RAM. The first specialized-presentation Navigator
run lacked continuous RAM sampling because its monitor could not locate adb;
its frame records are complete, but it provides no continuous memory bound.

Old renderer: `7382bd260` with read-only benchmark observers, APK SHA-256
`d60230a701dd4edcd6e7e0346898893122669da72447784304b72f27d4e3c718`.
Closed-panel transforms and placement: `e41ee80fe` plus direct transform
presentation, Navigator materialization and placement refinement scheduling,
APK SHA-256
`ede35aa5b23bd8770d1207929e1b633a25b481f7987eb7201645125a11e013e3`.
Raw runs are in `artifacts/latency-investigation/{old-closed-fixed,deferred-9-closed-fixed}`
and `old-navigator`. Later crop and open-Navigator rows include fragment
materialization and source-specific main-view shader entries, APK SHA-256
`1dbac0ef1dcf9208709ba3e7df71f0a028e99d6d25e4c60356e7413a270f1208`.
Their records are `present-18-crop` and `present-18-navigator`, including its
`extra` directory. The open-Navigator screen medians span 59.35–59.49/s,
with p99 intervals 16.74–16.92 ms. Earlier default-panel records retained
Navigator and are not evidence for a closed-panel workload.

### Two-page-refinement qualification

Measured 2026-09-29 on the 12 MP Perceptual photo at Fit, Navigator open, Stats
closed, default glass and three warmed five-second gestures per row. Thermal
status remains zero. This is the build used for the current brush and retouching
records, APK SHA-256
`ab0d8facb88f67cfc8da05284c70a639f4e7b0815aaed35ed79029e7365f4333`:
unminified Java for the white-box harness, optimized release Rust. Screen
observations use SurfaceFlinger actual-present times; they include native controls
and do not independently establish canvas scanout. No allocator or PSS sampler
runs during qualification. Available memory remains above 1 GiB.

| Journey | Completed updates/s, median | Completion gap p99, median | Screen presents/s, median | Screen interval p99, median |
| --- | ---: | ---: | ---: | ---: |
| Move whole image | 81.02 | 16.37 ms | 59.12 | 16.82 ms |
| Move partial selection | 134.19 | 12.36 ms | 59.44 | 16.85 ms |
| Move partial selection, Leave Copy | 131.10 | 14.52 ms | 59.26 | 16.74 ms |
| Placed-photo translation | 95.68 | 17.51 ms | 59.48 | 16.94 ms |
| Placed-photo resize, bar hidden | 87.13 | 18.18 ms | 59.38 | 16.92 ms |
| Placed-photo resize, bar visible | 88.50 | 17.50 ms | 59.48 | 16.95 ms |
| Pixel translation | 80.64 | 16.10 ms | 59.34 | 16.91 ms |
| Pixel resize: Free | 72.79 | 17.16 ms | 59.42 | 16.82 ms |
| Pixel transform: Distort | 63.80 | 17.85 ms | 59.42 | 16.86 ms |
| Pixel transform: Warp | 62.44 | 18.64 ms | 59.45 | 16.73 ms |
| Gaussian Blur, small radius | 23.48 | 71.39 ms | 49.95 | 33.37 ms |
| Gaussian Blur, large radius | 15.51 | 124.01 ms | 42.67 | 49.98 ms |

The Move, placement and transform screen-cadence measurements pass. In this build,
Navigator-open Distort, Free and visible-bar placement are below the old matched control's
77.33 / 78.80 / 90.59 completed updates/s. Both Gaussian rows fail the soft motion
target; no hardware calculation grants a waiver. The older spatial-filter
comparison uses a different camera and panel configuration. These runs contain
Gaussian sliders; they do not requalify the Exposure rows.

Raw records are `artifacts/latency-investigation/two-25-geometry`;
the analysis is `two-25-geometry-summary.txt`.

### Warp contour qualification

Three subsequent matched runs with curvature-bounded selection contours raise
Warp from 47.88 to 65.06 completed updates/s. Screen cadence is 59.37 presents/s
with a 16.84 ms p99 interval; all three runs meet the screen target, with thermal
status zero. This removes per-source-pixel contour subdivision: the previous
12 MP rectangular selection generated 14,160 points even along straight edges.
The editable grid and the pixel resampler are unchanged.

Build: `255309151` plus the contour fix, APK SHA-256
`643f97510b32802ab72f2ee9c30a7c52e0b1a104c2b044a2aaeb39cd33e9c96d`.
Raw runs: `artifacts/latency-investigation/warp-contour-warm`.
The cursor attribution trace is `cursor-count-profile`; it measures about
14,200 line segments during Warp versus eight during Distort.

### Earlier selected-pixel Move measurements

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

## BUILD32 G-Pen comparison

Measured on 2026-10-04 UTC with clean `192601dac` baseline BUILD29 and M3
candidate BUILD32 on the low-tier reference tablet. The contained G-Pen 1024 px
workload uses 4248 × 2832, Fit 15.9703%, dark theme, Linear blending, Navigator
open, Stats closed, Paper hidden, prediction enabled, pressure 1 and 200 Hz
injected input. Three warmed ten-second contacts retain all captured samples.
Logical setup matches; the nominal full tip stays at least 17.44 px inside the
photo. Both invocations finish successfully, with thermal status zero before
and after and no pending composition or edits at contact start.

| Build / contact | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline29 / 1 | 65.306 | 26.870 | 52.038 | 13.998 |
| Baseline29 / 2 | 64.870 | 27.905 | 50.290 | 14.252 |
| Baseline29 / 3 | 65.753 | 27.743 | 50.473 | 14.120 |
| Candidate32 / 1 | 64.416 | 25.855 | 51.523 | 14.231 |
| Candidate32 / 2 | 64.687 | 25.810 | 49.326 | 14.048 |
| Candidate32 / 3 | 65.539 | 24.960 | 51.480 | 13.921 |

All three candidate contacts meet 60 fresh completed updates/s and the 33.3 ms
fresh-gap limit. Throughput changes by −1.36 / −0.28 / −0.33%, owner CPU p95 by
+1.66 / −1.43 / −1.41%, and callback wall p95 by +4.02 / +0.20 / −2.64%, within
the measured 5% bounds. Response p99 changes by −0.515 / −0.964 / **+1.007 ms**.
The third pair's exact +1.006776 ms exceeds the +1 ms common bound by
0.006776 ms. The low-tier stroke result passes; the complete common comparison
gate remains open. This forward-order batch alone does not establish repeatability, other
workloads or overall M3 qualification; the reverse check follows below.

The reverse candidate→baseline check uses the same APKs and matching logical
setup. All three additional candidate contacts meet the tier limits. The
response excess repeats: candidate-minus-baseline p99 is **+1.192 / −3.075 /
+1.778 ms**; contacts 1 and 3 exceed +1 ms. Throughput changes by
+1.13 / −1.17 / −1.34%, owner CPU p95 by −1.62 / −0.28 / +3.39%, and callback
wall p95 by −2.00 / −0.40 / +1.38%, within 5%. Both orders remain separate;
averaging them or reversing their order does not clear the forward failure.

| Reverse build / contact | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Candidate32 / 1 | 65.713 | 27.643 | 54.054 | 13.725 |
| Candidate32 / 2 | 64.836 | 25.580 | 48.107 | 14.094 |
| Candidate32 / 3 | 64.295 | 26.772 | 51.993 | 14.470 |
| Baseline29 / 1 | 64.978 | 26.269 | 52.863 | 13.951 |
| Baseline29 / 2 | 65.603 | 27.053 | 51.183 | 14.133 |
| Baseline29 / 3 | 65.169 | 26.977 | 50.215 | 13.995 |

Tracked renderer residency is 805.641 MiB, except candidate contact 3 at
805.688 MiB; all boundaries satisfy max(16 MiB, 5% of baseline). Both retain
204 source slots. Reverse candidate residency is 805.641 MiB throughout; reverse
baseline contact 3 is 805.688 MiB. All reverse boundaries also satisfy the bound,
and thermal status remains zero. Dynamic cache budgets differ, so this does not establish
identical admission. Staging inventories and process RSS/high-water snapshots
are retained; continuous renderer/import/driver peaks are unmeasured. Fresh
completion and response metrics end at GPU callback service; presentation,
physical pen latency and GPU execution timestamps are unmeasured.

Exact APK SHA-256, baseline then candidate:
`e7ddce0b37f254df4fcdee4c502bc2220c5a2f050e644060bc4a47a06067e429` /
`acccdc9d9464c2ca2db5fc3291dc252b7affe683ca9cadb7628ffef58863a51c`.
Provenance is in `artifacts/format/m3-baseline192-android-build-29/provenance.json`
and `m3-candidate192-android-build-32/provenance.json`; raw captures, exact setup,
staging and every per-contact comparison are in `m3-low-brush-35/analysis.local.md`
and `analysis.json`. These APKs do not qualify later source edits. Earlier 4a
BUILD28 and BUILD20 results remain scoped to their binaries.

## BUILD28 G-Pen comparison

The frozen M3 candidate BUILD28 and clean `4a2cf6aa0` baseline BUILD20 use the
same contained G-Pen 1024 px workload on 4248 × 2832. Logical setup matches the
earlier BUILD20 pair exactly: Fit 15.9703%, dark theme, Linear blending, Navigator
open, Stats closed, Paper hidden, prediction enabled, pressure 1, 200 Hz injected
stylus input and three warmed ten-second strokes. Screen semiaxes are 240 × 120 px;
the nominal full brush tip remains at least 17.44 px inside the photo. Both
instrumentation logs finish successfully; thermal status is zero before and after
each invocation. All strokes begin without pending composition or edits.

| Build / stroke | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline20 / 1 | 65.186 | 25.815 | 50.922 | 13.923 |
| Baseline20 / 2 | 65.311 | 27.031 | 50.911 | 14.288 |
| Baseline20 / 3 | 64.964 | 25.788 | 51.601 | 14.573 |
| Candidate28 / 1 | 64.926 | 27.596 | 50.544 | 13.957 |
| Candidate28 / 2 | 65.022 | 25.959 | 49.502 | 14.109 |
| Candidate28 / 3 | 65.173 | 26.324 | 51.113 | 14.122 |

Every stroke meets 60 fresh completed updates/s and the 33.3 ms fresh gap limit.
Candidate throughput changes by −0.40 / −0.44 / +0.32%, owner CPU p95 by
+0.25 / −1.25 / −3.10%, and callback wall p95 by +0.44 / −2.38 / −2.41%.
Response p99 changes by −0.378 / −1.409 / −0.489 ms. The measured 5% CPU and
throughput bounds and +1 ms response bound hold in all three matched pairs.
These observations preserve the passing low-tier stroke row; they do not erase
the earlier BUILD20 +1.371 ms response sample or establish other workloads.

Tracked renderer residency is 805.641 MiB in both builds, with 204 decoded
source slots. Boundaries remain within max(16 MiB, 5% of baseline); continuous
renderer/import/driver peaks and later edit/undo/output lifetimes are unmeasured.
Rates count nonempty completions consuming fresh input inside contact, excluding
refinement. Response ends at GPU completion callback service. Neither metric
establishes actual screen presentation or physical pen latency; GPU execution
timestamps are absent.

Exact APK SHA-256, baseline then candidate:
`2dd557379b1cac6ac51ce813f8a168a46cd9a460249846efe08a8e79b5801af2` /
`b3e8bfe49f8e97bee101eb223a3c01debc540be34507c95e4e343eba470c4a5d`.
Provenance is in `artifacts/format/m3-baseline4a-android-build-20/provenance.json`
and `m3-candidate-android-build-28/provenance.json`; raw strokes, thermal records,
exact setup checks and per-stroke analysis are in
`artifacts/format/m3-low-brush-28/`. BUILD20 canvas diagnostics below and earlier
reference measurements remain scoped to their named binaries. The interrupted
mid-tier trace setup supplies no timing comparison or qualification evidence.

## BUILD20 selected canvas comparison

The matched 4248 × 2832 photo uses Navigator, dark theme, SDR and default glass.
Cameras and logical setup match in all fifteen contact pairs. Configured display
budgets differ; equal logical setup does not imply equal live headroom at cache
admission. Each Exposure contact records 63–93 distinct observed values and each
Threshold contact 83–106; parameters change in both builds. Pan has no
camera-after trace, so the input gesture does not establish every rendered pose.

| Motion / contact | Completed updates/s, baseline → M3 | Completion gap p99, ms, baseline → M3 | Owner CPU p95, ms, baseline → M3 |
| --- | --- | --- | --- |
| Pan, no effect / 1 | 59.154 → 59.078 | 19.087 → 19.033 | 4.387 → 5.184 |
| Pan, no effect / 2 | 59.885 → 59.729 | 19.053 → 18.961 | 4.891 → 5.639 |
| Pan, no effect / 3 | 59.776 → 59.624 | 18.902 → 18.821 | 4.093 → 4.162 |
| Pan, Shadows/Highlights / 1 | 52.854 → 52.639 | 22.777 → 24.011 | 5.680 → 5.652 |
| Pan, Shadows/Highlights / 2 | 59.573 → 59.641 | 19.289 → 19.714 | 4.804 → 4.559 |
| Pan, Shadows/Highlights / 3 | 59.611 → 59.782 | 18.958 → 18.945 | 5.039 → 4.244 |
| Pan, Clarity / 1 | 52.826 → 52.398 | 22.873 → 23.643 | 6.015 → 6.138 |
| Pan, Clarity / 2 | 59.612 → 59.598 | 19.441 → 19.490 | 4.048 → 4.259 |
| Pan, Clarity / 3 | 59.805 → 59.826 | 18.959 → 19.286 | 5.479 → 6.218 |
| Exposure scrub / 1 | 33.465 → 32.686 | 55.151 → 63.347 | 9.770 → 10.161 |
| Exposure scrub / 2 | 6.776 → 12.131 | 246.959 → 281.970 | 14.440 → 12.019 |
| Exposure scrub / 3 | 15.319 → 16.133 | 155.874 → 92.442 | 11.170 → 11.110 |
| Threshold scrub / 1 | 3.759 → 3.789 | 273.627 → 260.867 | 30.780 → 26.621 |
| Threshold scrub / 2 | 3.787 → 3.777 | 274.260 → 279.263 | 26.186 → 28.737 |
| Threshold scrub / 3 | 3.362 → 3.362 | 361.733 → 338.872 | 22.930 → 23.249 |

Bare pan completes near 60/s, but the first Shadows/Highlights and Clarity
contacts complete about 52/s in both builds. Exposure and Threshold miss the
60/s target in both builds; their stalls remain in the table. No matched
completion-throughput loss exceeds 5%. Owner CPU p95 increases by 18.17% and
15.29% in the first two bare-pan contacts, and 5.21% and 13.49% in the last two
Clarity contacts. Threshold has one 9.74% increase. These adverse repetitions
prevent declaring the common CPU gate cleared; they are not averaged away.

Tracked residency after contact is 573.350 MiB for bare pan, 522.788–522.985 MiB
with Shadows/Highlights or Clarity, and 516.985 MiB for Threshold in both builds.
Exposure is 574.350 MiB except the first M3 contact at 574.185 MiB. These matching
boundaries do not establish a continuous peak or an import/source lifetime bound.

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

## BUILD20 G-Pen comparison

Measured on 2026-10-04 UTC with the clean `4a2cf6aa0` baseline and the M3
candidate based on the same revision. Both are benchmark APKs with release Rust.
The tier photo has one drawing layer above it, Paper hidden, Linear blending,
Navigator open, Stats closed and default glass. Pressure is 1 and prediction is
enabled. A priming stroke is undone before three warmed ten-second strokes with
200 Hz OS-injected stylus input. Both before/after thermal samples are zero.
The matched Fit zoom is 15.9703%, with screen semiaxes 240 × 120 px; the
nominal full brush tip remains at least 17.44 px inside the photo.

Rates count completed nonempty updates consuming new paint input inside the
contact, excluding refinement-only completions. Response is the latest consumed
input event to GPU completion; its p99 differs from the intercompletion gap.
Neither metric establishes physical pen latency or screen presentation.

| Build / stroke | Fresh updates/s | Fresh gap p99, ms | Input→GPU response p99, ms | Owner CPU p95, ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline 1 | 65.793 | 25.665 | 50.743 | 13.745 |
| Baseline 2 | 65.281 | 26.588 | 52.332 | 13.795 |
| Baseline 3 | 66.138 | 26.681 | 49.989 | 13.455 |
| Candidate 1 | 65.103 | 27.007 | 52.114 | 13.834 |
| Candidate 2 | 64.995 | 27.159 | 52.525 | 13.984 |
| Candidate 3 | 65.270 | 27.904 | 50.531 | 13.886 |

Both builds meet 60 fresh updates/s and the 33.3 ms gap limit in every stroke.
The candidate loses 0.44–1.31% throughput and grows owner CPU p95 by 0.65–3.21%,
within the 5% comparison bound. Fresh gap p99 separately grows by
1.343 / 0.571 / 1.223 ms. Response p99 grows by 1.371 / 0.193 / 0.542 ms;
the first stroke exceeds the common +1 ms response bound. These samples do not
establish that bound for the candidate, despite the passing stroke target.

Accounted renderer residency after the strokes is 805.641 MiB in the candidate
and 805.641–805.656 MiB in the baseline, with 204 decoded source slots in both.

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

Target: **60 completed updates/s** at the guaranteed size, on the 12 MP canvas.

Except for G-Pen, Pencil and Eraser, measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets.

- The older fixtures fit their 520 × 299 px ellipse to 339 × 299 px in the
  TCL's 754 px work area at 16.0% zoom. The current G-Pen row uses the 240 × 120 px semiaxes documented in
  the current comparison above.
- Simple brushes are measured at their guaranteed 1024 px.

| Brush (id) | Class | Size | Measured | Status |
| --- | --- | --- | --- | --- |
| G-Pen (1) | Simple | 1024 px | Solid Color revision: 95.77 fresh updates/s (89.67–96.53); fresh completion gap p99 ≤27.60 ms | Meets measured 1024 px stroke criteria; [Solid Color fills](#solid-color-fills) |
| G-Pen (1), M3 BUILD32 | Simple | 1024 px | Both orders: 64.295–65.713 fresh updates/s; fresh gap p99 24.960–27.643 ms, Linear | Meets this stroke; common response excess repeats: forward **+1.007 ms**, reverse **+1.192/+1.778 ms**; presentation unmeasured; [BUILD32](#build32-g-pen-comparison) |
| G-Pen (1), M3 BUILD28 | Simple | 1024 px | BUILD28: 64.926–65.173 fresh updates/s; fresh gap p99 25.959–27.596 ms, Linear | Meets this stroke and matched CPU/response bounds; presentation unmeasured; [BUILD28](#build28-g-pen-comparison) |
| Rough G-Pen (28) | Simple | 1024 px | 25.6 updates/s (25.5–25.7); gap p99 69.5 ms | **Not met** |
| Calligraphy Pen (29) | Simple | 1024 px | 91.0 updates/s (90.7–91.5); gap p99 37.1 ms | **Not met** |
| Antique Pen (30) | Simple | 1024 px | 37.5 updates/s (37.1–38.0); gap p99 79.9 ms | **Not met** |
| Realistic Pen (31) | Simple | 1024 px | 28.3 updates/s (28.2–28.3); gap p99 63.4 ms | **Not met** |
| Wet Ink (32) | Simple | 1024 px | 23.0 updates/s (22.9–23.1); gap p99 73.7 ms | **Not met** |
| Pencil (2) | Simple | 1024 px | 11.1 fresh updates/s (11.08–11.36); completion gap p99 107.46–113.63 ms | **Not met** |
| Pointy Pencil (25) | Simple | 1024 px | 15.2 updates/s (15.2–15.3); gap p99 96.7 ms | **Not met** |
| Shading Pencil (26) | Simple | 1024 px | 58.3 updates/s (56.2–59.8); gap p99 58.0 ms | **Not met** |
| Charcoal (27) | Simple | 1024 px | 14.2 updates/s (14.2–14.3); gap p99 114.0 ms | **Not met** |
| Chalk (6) | Simple | 1024 px | 10.4 updates/s (10.3–10.5); gap p99 131.3 ms | **Not met** |
| Eraser (3) | Simple | 1024 px | 29.9 fresh updates/s (29.71–29.95); completion gap p99 44.67–45.06 ms | **Not met** |
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

## Integrated compositor measurement

Measured on 2026-09-30 against `ba8835fec`: Perceptual 12 MP photo,
240 × 140 px trajectory, 16 ms prediction, three five-second strokes after
warm-up. Stats is closed, Navigator is visible and thermal status is zero.
Fresh updates count completed frames that consumed new real pen samples.
This historical candidate separates the front layer from the balanced lower composite and
skips color reads for fully covered in-place uniform Normal pixels. It keeps
the same formats, memory allowance and existing pipelines.

| Workload | Main fresh updates/s | Candidate fresh updates/s (range) | Candidate completion gap p99, range | Target |
| --- | ---: | ---: | ---: | --- |
| G-Pen 1024 px, one photo at Fit | 54.81 | 65.70 (65.59–66.10) | 26.12–28.57 ms | Meets 60/s for this stroke |
| G-Pen 1024 px, eight photos at 50% | 29.88 | 35.67 (35.65–35.83) | 39.05–39.78 ms | Not met |
| G-Pen 1024 px, eight photos at 100% | 19.50 | 27.05 (27.02–27.05) | 44.06–44.75 ms | Not met |
| Pencil 1024 px, one photo at Fit | 11.14 | 11.14 (11.08–11.36) | 107.46–113.63 ms | Not met |
| Eraser 1024 px, one photo at Fit | 30.06 | 29.91 (29.71–29.95) | 44.67–45.06 ms | Not met |

The stacked rows have nine visible layers: one opaque photo, seven translucent copies and one
paint layer. They do not qualify the separate eight-paint-layer target. Fit is
15.97%. Candidate allocator residency is 812 MiB at Fit, 1,069 MiB at 50% and
1,008 MiB at 100%, within 2 MiB of the matched controls. G-Pen owner CPU p50 is
10.37 / 14.64 / 13.15 ms; these are CPU times, not frame or GPU intervals.
Settling is 590–613 / 930–961 / 700–736 ms. The size-class guarantee remains
open because other brushes and trajectories are not qualified.

Candidate: `c452a0642` (production source matches `fc5d00fd5` after the
test-fixture rebase), optimized benchmark APK SHA-256
`865b0dd05b8253b3b22eac806136023a5befeac5b642e5fddb2f3cd24194d047`.
Raw records are `artifacts/optimization-roi/{current-main,final}-low-fit`,
`{current-main,final}-stack{50,100}` and `main-other-simple`. Pencil and Eraser
controls use `829f1e223`; their rates remain within 1% of the candidate.
A neighboring seven-photo check at 100% reaches 27.12/s versus 25.94/s on
`829f1e223`; raw records are `{main,candidate1}-stack7-100`.

The preceding two-page idle-refinement comparison (2026-09-29) passes the
one-frame added-submission p95 budget in all three runs; added GPU-completion
p95 still fails in one run. See
[responsiveness](responsiveness.md#resuming-during-refinement). Its raw records
are `artifacts/latency-investigation/two-25-stack-{50,100}-{constant,pauses}`.
This does not qualify interruption after the current shader and graph changes.
In that comparison, smaller idle batches improve interruption while increasing total settling time;
Fit settling is 590–613 ms here versus 478–544 ms on the current main control.

### Painting below the front layer

Measured on 2026-09-30 on `c82970142` with a benchmark-only paint-position
control. Both optimized release builds keep the covered-pixel shader guard,
transparent-paper removal, memory budget and two-page refinement. The control
changes only the root from `combine(front, over(back))` to `over(all)`.
These are three warmed five-second strokes per position: 12 MP Perceptual
photo, G-Pen 1024 px, 100% zoom, 240 × 140 px path, Stats closed, default
workspace and thermal status zero. Paint stays above the opaque bottom photo;
the other photos have 35% opacity.

| Visible layers | Paint position | Balanced root, fresh updates/s | Front-biased root, fresh updates/s | Front-biased completion gap p99, range |
| --- | --- | ---: | ---: | ---: |
| Eight: seven photos and paint | Top | 16.50 | 26.90 | 44.07–46.94 ms |
| Eight: seven photos and paint | Middle | 16.48 | 13.54 | 81.76–88.15 ms |
| Eight: seven photos and paint | Lowest above the opaque photo | 16.52 | 13.74 | 81.54–86.91 ms |
| Nine: eight photos and paint | Lowest above the opaque photo | 13.72 | 13.71 | 82.20–85.18 ms |

None meets the 60/s target. The isolated root bias costs 17–18% throughput
on the middle and lower eight-layer cases. It reduces the top dependency path
from three blends to one, while six other paths grow from three to four.
Uniform edits across the eight operands therefore average 3.5 rather than 3
blends. Nine operands already have the same root shape with either construction.
The earlier front-only rates do not establish a benefit for drawing on all
layers. Pixel-correctness and logarithmic-work tests do not establish that
tradeoff either. These results compare root shapes with identical other
optimizations, not lower-layer performance against the older main APK.

APK SHA-256: front-biased
`882a013a1209f542caf0442f056ab14143fac58722dfdc0de027ec0f8240d50e`;
balanced `9e3e455bc1446a4e414582791ed288e755c984736e197f6e39c19e962a3d3f88`.
Raw records and source provenance:
`artifacts/optimization-roi/all-layer-followup/tree-comparison.json` and
`{biased,balanced}-{7,8}-{top,middle,lower}` for the listed cases.

### Layer-neutral composition and refinement

The fixed front-operand preference is removed. The selected four-page build
retains balanced whole-run grouping, the covered-pixel guard and transparent-paper
exclusion. A regression checks bounded blend paths and exact pixels for top,
middle and lower edits across layer counts and blend spaces.

Measured on 2026-09-30: the same eight-visible-layer, 100% zoom conditions above,
with three warmed five-second strokes per position. Fresh updates/s are
16.56 at the top, 16.50 in the middle and 16.54 above the opaque bottom photo.
Completion-gap p99 ranges are 68.42–70.34, 69.30–70.66 and 67.75–71.65 ms,
respectively. All remain below the 60/s target. This verifies the selected
implementation across positions; the preceding matched root comparison
establishes the cost of the removed preference.

At Fit, one photo and G-Pen 1024 px now reach 63.69 fresh updates/s
(63.53–63.90), with completion-gap p99 27.07–28.31 ms. These three five-second
strokes use a 250 × 140 px path, Perceptual blending, 16 ms prediction, default
workspace, Stats closed and thermal status zero. This stroke meets the moving
target; it does not qualify the brush class or the eight-paint-layer target.

Four-page batches restore median settling to 496 ms versus 596 ms with two
pages, without a continuous-stroke rate change. Typical resumed GPU completion
increases from 56.6 to 59.0 ms. The two-page cap is removed; fresh-input queueing
behind one unfinished batch remains. See the controlled
[refinement tradeoff](responsiveness.md#refinement-batch-tradeoff).

Source base: `1557688aa` plus the layer-neutral root and four-page cap changes.
Optimized release benchmark APK SHA-256:
`05d747e5f3e6417e9ea3a96a0d1b5bfe38a3aca00744891bbdd01c9a37334a8b`.
Raw records, immutable APKs and source patches:
`artifacts/refinement-tradeoff/{four-overlap-low-constant,final-layer-{top,middle,lower}}`
and `provenance.json`.

### GPU attribution with Stats closed

A separate five-second ATrace diagnostic of the candidate's eight-photo 100%
case enables bounded GPU timestamps without opening Stats. Its 26.54 updates/s
is diagnostic, not a rate qualification. Observations match renderer frame IDs
inside the input window; independently aggregated medians do not add up, and
GPU intervals include scheduling gaps rather than measuring hardware occupancy.

| Phase | Median GPU interval |
| --- | ---: |
| Drawing, 134 observations | 26.77 ms |
| Paint | 3.47 ms |
| Prediction and uncommitted-contact preview | 7.71 ms |
| Main composition, excluding paired mip interval | 10.10 ms |
| Main mips | 6.50 ms |
| Separate overview composition | 0.005 ms |

The main mip rectangle contains 258.3 million base-level pixels across this
stroke; changed output regions contain 213.3 million, or 82.6% of that area.
There are typically 24 tiled output regions per update. Sparse mips could avoid
17.4% of this base-level area, but their dispatch and merge cost is unmeasured.
That area ratio neither establishes a GPU-time saving nor closes the 60/s gap.
Main composition, preview and viewport work remain material costs. None of these
measurements proves the target impossible or establishes a complete path to it.
This diagnostic predates the rebase and uses the candidate APK SHA-256
`f0b87ae5094b33d4383bfb3f9d0ab8d8699d1aa508ee7f6c1fe11edb6d261ad2`.
Raw records: `artifacts/optimization-roi/candidate1-stack100-diagnostic`.

## Input during Healing finalization

TCL navigation during large Healing/Spot Healing finalization has input-queue
p95 of 15.8–19.4 ms and a maximum of 45.0 ms. The largest settle callback is
58.7 ms. The one-second pinch probes present at 54.9–57.0/s; these do not qualify
the sustained navigation row. System available memory stays above 2,198 MiB.

Measured on 2026-09-29, three runs per brush on the tier photo, 512 px,
Perceptual, Fit and Stats closed. Dependent painting queues until the healed
raster publishes. The [responsiveness record](responsiveness.md#healing-finalization)
contains the build, workload, tool-action limits and raw records.

## Pinned localization comparison

Measured on 2026-10-01 PDT / 2026-10-02 UTC on the TCL reference tablet.
Baseline `271918681` is compared with source tree
`e56742a57787e6bf8dd6da3f4fecadcc718657a6`. Release, unminified APK SHA-256:
baseline `616b5b10d19eb4732d9631db122cbce49be7cf139fcdd3f61d24208e93ec36c8`,
candidate `33e88926fd66491d437f8397af08755c2ae5e4832ac8aaf4b0d06ab38e124e48`.
These observations do not qualify the later GPU-bounds successor.

The 4248 × 2832 Sony photo has one empty paint layer, Perceptual blending,
Fit zoom 15.97%, default Navigator and glass, Stats closed and 16 ms prediction.
Three warmed ten-second OS stylus strokes use G-Pen 1024 px and a contained
220 × 100 px trajectory. Camera, settings and visible layers match; painting
thermal status is zero before and after each run.

| Source sequence | Fresh completed updates/s | Maximum fresh gap p99 | Stroke criteria |
| --- | --- | --- | --- |
| Baseline | 66.89–67.32 | 26.56 ms | Met |
| Candidate | 66.27–67.26 | 26.57 ms | Met |
| Repeated baseline | 64.77–66.49 | 27.62 ms | Met |

Candidate painting ranges overlap the baseline drift; no regression is
demonstrated for this stroke. This does not qualify other brushes or scanout.

Three warmed ten-second slider drags use the same photo, empty paint layer and
Navigator, with Exposure on the photo or a Levels/Vibrance/Exposure chain.
The identical test fixture uses the photo workload on both builds.

| Numeric motion | Baseline UI Hz / maximum p99 | Candidate UI Hz / maximum p99 | Raw canvas completions/s, baseline → candidate |
| --- | --- | --- | --- |
| Exposure | 48.93–52.07 / 49.99 ms | 53.45–54.36 / 33.35 ms | 12.27–13.17 → 10.49–10.98 |
| Chain Exposure | 59.11–59.62 / 33.32 ms | 59.32–59.62 / 16.73 ms | 8.49–10.28 → 7.28–9.18 |

Direct Exposure misses the native UI floor on both builds. The chain meets
native UI cadence, but fresh photo preview remains **unqualified**. Completion
counts include Navigator, have no effect input/revision pairing and cannot
establish preview freshness. All three direct candidate runs have fewer
completions; sampled values and dynamic memory budgets also differ. Neither
causal attribution nor harmlessness is established. Numeric thermal status was
zero before each gesture; the final after-snapshot was not captured.

Raw records and complete APK/test-fixture provenance are retained under
`artifacts/reference-localization/`. The initial overflowing paint footprint
and hidden-Navigator numeric attempts are excluded and retained separately.

## Session checkpoints

TCL release-profile diagnostic, 2026-10-04, working changes based on
`a9180575e60670cdd61d73771674ece7f2096d9f`; APK SHA-256
`ea04f5478d3e28795a7bc1269d498e9dfbcb99d781377d490ddc4274b8cda13a`.
The reference 4248 × 2832 photo and a paint layer use Fit zoom and Navigator,
with the default workspace. Each workload warms up and runs three five-second
motions while the two-second session poll is enabled. The G-Pen uses 1024 px,
with a contained 100 × 80 surface-pixel ellipse.

Pan reaches 60.00–60.12 renderer submissions/s with p99 intervals of
18.70–18.92 ms. G-Pen reaches 77.6–79.0 fresh completed updates/s with completion
gap p99 23.36–24.07 ms. The process writes 262–418 KiB per pan gesture and
172–1,664 KiB per continuous stroke. Pan's session storage grows 14,604 bytes in
the first run, then stays nearly constant. Painting grows it by 0–275,422 bytes
per run. The newest checkpoint age is 2.05–2.07 seconds in steady navigation,
with a four-second initial maximum. Continuous contact defers capture until
release and reaches 5.03–6.51 seconds.

These runs collect process-wide kernel write counters and sample session files;
the counters include all process disk writes. Pan records renderer submissions,
not screen presentation. Thermal status was not recorded before these runs, so
neither workload qualifies its tier target. There is no matched baseline APK.
[Raw timing and checkpoint samples](../../artifacts/seamless-restart/android/viewport-benchmark/)
and summaries are under `artifacts/seamless-restart/android/`. Mid and top tiers
remain unmeasured for this change because their reference tablets were reserved
by other work.

The repeated-contact diagnostic uses the same photo, brush and layout with
50 contacts of 100 ms separated by 100 ms pauses in each of three intervals.
Actual intervals are 9.827–9.834 seconds. APK SHA-256 is
`5938506b8c553a5b451e2215254843301c2cb163167f5ba65e39857ed27a08b5`,
with working changes based on `0516be628`; thermal status before the run is 0.
Each interval writes 11.02–14.27 MiB process-wide and grows session storage by
2.08–2.36 MiB. Observed head age reaches 3.52–4.37 seconds. The sampler targets
100 ms; actual median gaps are 125–187 ms and maximum gaps are 149–222 ms.
Head modification time measures observed publication age rather than the fsync
completion instant. The paused workload and 136–144 GPU timing samples do not
qualify a moving-frame rate. [The short-contact summary](../../artifacts/seamless-restart/android/short-checkpoint-summary.json)
retains each interval and its raw source.

## Filter attachment feedback

Measured 2026-10-04 on TCL TAB 11 Gen 2, thermal status 0 before and after,
with the 4248 × 2832 reference photo at Fit, default workspace and panel glass,
one clipped empty paint layer, and attached Gaussian Blur and Curves.
`AndroidTitleBarTest#layerSwipeFrameTiming` with `layerReorderBenchmark` and
`layerRelationshipBenchmark` drags Curves through the neighboring filter gaps.
One priming gesture precedes three five-second mouse gestures; every drop is
canceled.

| Run | 1 | 2 | 3 |
| --- | --- | --- | --- |
| Native UI frames/s | 55.89 | 55.64 | 59.72 |
| Moving-frame interval p99, ms | 33.33 | 33.34 | 16.75 |

These `FrameMetrics` observations do not meet the tier target. They measure the
attached-filter workload, separately from ordinary row reorder. No matched
baseline was measured. The benchmark APK uses release Rust and unminified Kotlin
from `7f69a9356` plus the clipping-filter changes. APK/source hashes, raw frames,
display and thermal records, and the summary are in
`artifacts/clipping-filter/build-provenance.json`,
`artifacts/clipping-filter/low/` and
`artifacts/clipping-filter/motion-summary.json`.
