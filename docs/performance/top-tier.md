# Top tier: 120 fps on 61 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: Wacom MovinkPad Pro 14 (DTHA140) with a 1800 × 2880, 120 Hz OLED. The
canvas is 9504 × 6336. Every row targets **120 fps** unless marked soft.

## Operations

| Operation | Target | Measured | Source |
| --- | --- | --- | --- |
| Pan: Hand tool, one or two fingers | 120 | Met on a small document: 118.8 fps, interval p50/p99 8.3/12.0 ms (1024 px document; not yet at 61 MP) | [Android development](../development/android.md#benchmarks), 2026-09-27 |
| Pinch zoom | 120 | **Met.** 119.3 fps on the 61 MP photo; 117.7 fps, p99 15.6 ms on a 1024 px document | 2026-09-22; `2c3cb244`, 2026-09-27 |
| Two-finger rotate | 120 | | |
| Navigator drag | 120 | | |
| Brush-cursor hover | 120 | | |
| Placed-photo drag (24 MP photo) | 120 | | |
| Pixel transform handle drag: Free, Uniform, Skew or Rotate | 120 | | |
| Pixel transform: Distort or Perspective | 120 | | |
| Pixel transform: Warp | 120 | | |
| Selection transform, full canvas | 120 | | |
| Move tool layer drag | 120 | | |
| Marquee, Lasso or Polygon drag | 120 | Met on a small document: in-stroke interval p50/p99 4.2/6.9 ms with the canvas bar shown, p99 8.8 ms with it off (2048 × 1536) | `ba9483a8`, 2026-09-27 |
| Selection Brush or Quick Mask, 2048 px | 120 | | |
| Grow, Shrink or Feather drag, full canvas | 120, soft | **Not met.** Feather: 14.9 updates/s on 6000 × 4000; 72.5 updates/s on 2048 × 1536 | Canvas-bar `refine-feather-drag`, 2026-09-27 |
| Pointwise adjustment slider: Levels, Curves, Exposure, Hue/Saturation, Color Balance, White Balance, Black & White | 120, soft | | |
| Neighbourhood filter slider: Gaussian Blur, Unsharp Mask, Edge-Preserving Smooth | 120, soft | | |
| Animated or warping filter: Domain Warp, Ripple | 120, soft | | |
| Fill layer or gradient-fill edit | 120, soft | | |
| Navigation with proof or tone guide shown | 120 | | |
| Gradient drag | 120 | | |
| Figure or ruler drag | 120 | | |
| Layer opacity scrub | 120 | | |
| Layer reorder drag | 120 | | |
| Navigation with 32 visible paint layers | 120 | | |
| Drawing with 32 visible paint layers, G-Pen 1024 px | 120 | | |
| Panel, tab, column or toolbar drag and docking | 120 | **Not met.** Toolbar or component drag 103–119 fps | `dc27e04d`, 2026-09-23 |
| Panel or column resize | 120 | | |
| Drawer open and close | 120 | | |
| Colour wheel or picker drag | 120 | Picker callback p95 3.8–4.5 ms (callback time, not presented rate) | `3e521c63`, 2026-09-24 |
| Slider scrub: size, opacity, flow | 120 | | |
| Canvas action bar show, hide and move | 120 | **Not met.** UI frame p50/p95: 63.2/90.0 ms moving the bar, 23.0/34.6 ms show and hide (2048 × 1536) | Canvas-bar `ui-bar-move` and `ui-bar-show-hide`, 2026-09-27 |
| Tool Options or panel content change | 120 | **Not met.** UI frame p50/p95 25.1/30.6 ms | Canvas-bar `ui-panel-change`, 2026-09-27 |
| List scrolling: layers, brushes, filters | 120 | | |
| Menu open and close | 120 | Menu open adds no canvas frames; UI frame p50/p95 11.5/26.6 ms | Canvas-bar `selection-bar-menu-open`, 2026-09-27 |

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

## Brushes

Target: **120 completed updates/s** at the guaranteed size, on the 61 MP canvas.

Except for G-Pen, measured on 2026-09-27 at `be5a7c38` with the [brush benchmark](measuring.md#how-to-measure). Each result is three 10 s strokes of a 200 Hz stylus ellipse at Fit zoom, at pressure 1 with 16 ms prediction, painting into an empty layer above the photo. The measured value is the median of the three strokes' completed updates per second, followed by the range across strokes. The gap is the interval between update starts. A brush meets its target when the median reaches it and the gap p99 is at most two frame budgets. The ellipse is 520 × 299 px at 16.5% zoom.

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
| G-Pen (1) | Simple | 2048 px | 150.2 fresh updates/s (138.61–154.45); completion gap p99 20.02–36.63 ms | **Not met** |
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
| Paintbrush (4) | Complex | 1024 px | 109.1 updates/s (109.0–110.6); gap p99 18.9 ms | **Not met** |
| Textured Flat (15) | Complex | 1024 px | 109.2 updates/s (108.8–110.0); gap p99 19.0 ms | **Not met** |
| Dry Scumble (16) | Complex | 1024 px | 87.7 updates/s (87.4–88.0); gap p99 22.7 ms | **Not met** |
| Transparent Glaze (18) | Complex | 1024 px | 99.9 updates/s (99.7–100.0); gap p99 21.6 ms | **Not met** |
| Multiply Glaze (14) | Complex | 1024 px | 76.1 updates/s (76.1–76.2); gap p99 28.6 ms | **Not met** |
| Dual Texture (9) | Complex | 1024 px | 80.0 updates/s (79.5–80.6); gap p99 21.3 ms | **Not met** |
| Spray (8) | Complex | 1024 px | 33.8 updates/s (32.7–34.3); gap p99 113.2 ms | **Not met** |
| Opaque Gouache (19) | Very complex | 512 px | **Crashed**: native allocator out of memory (Scudo map failure) | **Not met** |
| Watercolor Wash (20) | Very complex | 512 px | **Crashed**: SIGSEGV inside the Adreno Vulkan driver (fault address 0x1c) | **Not met** |
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

## Current G-Pen comparison

Measured on 2026-09-30 against `ba8835fec`: the tier photo beneath one paint
layer, Perceptual blending, 2048 px G-Pen, Fit, 16 ms prediction, default
workspace with Stats closed, warm-up and three five-second strokes. Thermal
status is zero. The front-stack and covered-pixel candidate raises fresh input
throughput from 102.45 to 150.24 updates/s. Completion-gap p99 is
20.02–36.63 ms. The rate exceeds 120/s, but the 16.7 ms gap target remains open.
This does not qualify the class or its other brushes.
Settling increases from 715–793 ms to 1,074–1,438 ms; smaller idle-refinement
batches trade completion time for admission of fresh input.

Candidate: `c452a0642` (production source matches `fc5d00fd5` after the
test-fixture rebase), optimized benchmark APK SHA-256
`865b0dd05b8253b3b22eac806136023a5befeac5b642e5fddb2f3cd24194d047`.
Raw records: `artifacts/optimization-roi/{current-main,final}-top-fit`.

Current short-contact tests show a separate start-latency gap after two seconds
of idle, despite longer settling not increasing the matched run medians.
A 25-contact rapid sequence also encountered a GPU-driver allocation crash;
a memory-sampled repeat passed. Neither the cause of the idle start delay nor
the intermittent allocation failure is established. See
[the resumed-contact audit](responsiveness.md#resumed-contacts-before-the-batch-tradeoff) before
treating settling time as harmless to subsequent drawing.

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

## Latest refinement validation status

The subsequent 2026-09-30 shared renderer removes the fixed front-layer
preference and restores four-page idle-refinement batches while retaining
fresh-input queueing. It is measured on low and mid tiers in
[the controlled tradeoff](responsiveness.md#refinement-batch-tradeoff).
MovinkPad 14 was reserved by another session, so that version has no top-tier
remeasurement. The G-Pen and retouching rows above retain their frozen builds;
they do not qualify the latest version.

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
