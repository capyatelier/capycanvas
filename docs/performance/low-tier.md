# Low tier: 60 fps on 12 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: TCL TAB 11 Gen 2 (9465X) with a 1200 × 1920, 60 Hz panel. The canvas
is 4248 × 2832.

- Every row targets **60 fps** unless marked soft.
- Geometry measurements use the 12 MP photo at Fit zoom. Older UI-only rows
  name their own canvas sizes.

## Operations

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
| Pan: Hand tool, one or two fingers | 60 | Photo, two fingers, Navigator open: screen 59.43 presents/s, p99 ≤16.83 ms; viewport 59.83 fresh completed updates/s | Retained-Navigator navigation below |
| Pinch zoom | 60 | Photo, Navigator open: screen 59.40 presents/s, p99 ≤16.83 ms; viewport 59.90 fresh completed updates/s | Retained-Navigator navigation below |
| Two-finger rotate | 60 | | |
| Navigator drag | 60 | | |
| Brush-cursor hover | 60 | | |
| Placed-photo translation | 60 | Navigator closed: screen 59.3 presents/s, p99 16.9 ms; renderer 122.9 completed updates/s | Earlier direct-presentation comparison below, `photo-translate-drag` |
| Placed-photo corner resize | 60 | Navigator open: screen 59.4 presents/s, p99 ≤17.0 ms; renderer 104.2 fresh completed updates/s | Current drag comparison below |
| Pixel transform corner resize: Free | 60 | Navigator open: screen 59.4 presents/s, p99 ≤16.8 ms; renderer 150.7 fresh completed updates/s | Current drag comparison below |
| Pixel transform: Uniform, Skew or Rotate | 60 | | |
| Pixel transform translation | 60 | Navigator closed: screen 59.5 presents/s, p99 16.7 ms; renderer 175.1 completed updates/s | Earlier direct-presentation comparison below, `photo-pixels-translate-drag` |
| Pixel transform: Distort | 60 | Navigator open: screen 59.4 presents/s, p99 ≤16.9 ms; renderer 134.0 fresh completed updates/s | Current drag comparison below |
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
| Pointwise adjustment slider: Exposure | 60, soft | **Not met.** Screen 52.7 presents/s, p99 33.4 ms; renderer 30.8 completed updates/s | Pointwise graph comparison below |
| Pointwise chain: Levels, Vibrance, Exposure slider | 60, soft | **Not met.** Screen 57.3 presents/s, p99 33.3 ms; renderer 25.1 completed updates/s, with a 13.9–31.7 range | Pointwise graph comparison below |
| Other pointwise adjustment sliders | 60, soft | | |
| Gaussian Blur slider, small radius | 60, soft | **Not met.** Screen 50.0 presents/s, p99 33.4 ms; renderer 23.5 completed updates/s | Two-page-refinement qualification below, Navigator open |
| Gaussian Blur slider, large radius | 60, soft | **Not met.** Screen 42.7 presents/s, p99 50.0 ms; renderer 15.5 completed updates/s | Two-page-refinement qualification below, Navigator open |
| Other neighbourhood filter sliders: Unsharp Mask, Edge-Preserving Smooth | 60, soft | | |
| Animated or warping filter: Domain Warp, Ripple | 60, soft | | |
| Fill layer or gradient-fill edit | 60, soft | | |
| Navigation with proof or tone guide shown | 60 | | |
| Gradient drag | 60 | | |
| Figure or ruler drag | 60 | | |
| Layer opacity scrub | 60 | | |
| Layer reorder drag | 60 | | |
| Navigation with 8 visible paint layers | 60 | | |
| Drawing with 8 visible paint layers, G-Pen 1024 px | 60 | **Not met.** Navigator open, Fit: 50.12 fresh updates/s (48.65–51.65), completion gap p99 29.30–32.87 ms | Retained-Navigator painting below; seven photo layers and one drawing layer |
| Panel, tab, column or toolbar drag and docking | 60 | | |
| Panel or column resize | 60 | | |
| Drawer open and close | 60 | | |
| Colour wheel or picker drag | 60 | | |
| Slider scrub: size, opacity, flow | 60 | | |
| Canvas action bar show, hide and move | 60 | **Not met.** UI frame p50/p95: 32.9/41.7 ms moving the bar, 11.6/21.0 ms show and hide (2048 × 1536) | Canvas-bar `ui-bar-move` and `ui-bar-show-hide`, 2026-09-27 |
| Tool Options or panel content change | 60 | **Not met.** UI frame p50/p95 25.4/35.3 ms | Canvas-bar `ui-panel-change`, 2026-09-27 |
| List scrolling: layers, brushes, filters | 60 | | |
| Menu open and close | 60 | | |

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

### Retained-Navigator navigation

The same final APK completes three warmed five-second two-finger pans and pinches on the
12 MP photo with a preparation stroke, Navigator open, Stats closed and 15.9703%
Fit zoom. Pan reaches 59.83 fresh viewport completions/s and a median 59.43 screen
presents/s; pinch reaches 59.90 and 59.40/s. Screen interval p99 ranges from
16.74–16.83 ms for pan and 16.76–16.83 ms for pinch. Both meet the screen-cadence
criterion, subject to the SurfaceFlinger accounting limit above. Fresh completion
gap p99 is 19.09–19.62 ms for pan and 19.94–21.56 ms for pinch.

The host's existing navigation policy uses FIFO without a retained surface
target. The viewport harness captures presenter GPU timestamps in these runs;
fresh counts use completed host input calls. Records are in
`artifacts/validation/candidate-clock-navigation`, including all three gesture
windows per motion and `matched-summary.json`. These measurements establish
current photo navigation, not a before/after speedup.

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

## Brushes

Target: **60 completed updates/s** at the guaranteed size, on the 12 MP canvas.

Except for G-Pen, Pencil and Eraser, measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets.

- The older fixtures fit their 520 × 299 px ellipse to 339 × 299 px in the
  TCL's 754 px work area at 16.0% zoom. The current G-Pen row uses the 500 × 280 px path documented in the
  retained-Navigator painting comparison above.
- Simple brushes are measured at their guaranteed 1024 px.

| Brush (id) | Class | Size | Measured | Status |
| --- | --- | --- | --- | --- |
| G-Pen (1) | Simple | 1024 px | 61.14 fresh updates/s (60.69–61.70); completion gap p99 28.98–29.50 ms | **Met for this stroke** |
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
