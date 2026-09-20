# Wacom 2048 px G-Pen: the path to 30 FPS

Follow-up to [latency attribution](android-pen-latency-20260920.md) and
[the conditional bandwidth model](android-pen-bandwidth-20260920.md).
Implementation starts from freshly fetched `origin/main`, `90320a41`.

The subsequent [Wacom comparison of main commit 39e9cba3](android-pen-main-comparison-20260920.md)
measures the additional swept-contact and composition optimizations against this build.

## Conclusion and scope

30 visible canvas updates/s is achievable on this device and workload. The
final admitted-memory build measured **32.29/s** over the entire ten-second fast
stroke, including startup, with light probes. Its preceding fixed-limit prototype
measured 33.29/s. The matched control and final build are recorded below.
This is an average-throughput result, not a guarantee that every frame takes
33.3 ms: stroke-start stalls and slower undo/redo remain.

All app measurements use the attached Wacom `5ll21u1002931`, primary package
`art.capycanvas`, 9504×6336 photo, a separate initially empty paint layer, hidden
paper, opaque 2048 px G-Pen, 17.164% fit zoom and 16 ms prediction fallback.
`AndroidPenMotion` supplies 200 Hz Android stylus events around the same screen
ellipse, center (1410,948), radii (520,299). Fast motion is one loop/s; slow is
0.25 loop/s. No input samples are deliberately dropped by these changes.
Each workflow also measures hover, undo and redo. The slow replay's final
pen-up chord is unchanged; see the original measurement protocol.
The throughput claim applies to this repeated opaque stroke. Exactness tests
cover additional pressure/color cases, but 30 FPS across arbitrary brush
settings, translucent strokes or different motion/overlap is not qualified.

## Why the old implementation could not meet the budget

30 FPS gives 33.33 ms per update. The fresh matched baseline (`baseline-fit-fast`)
produced **0.70 canvas latches/s**, with average observed GPU intervals of
419.1 ms paint, 21.9 ms prediction and 456.7 ms composition/mips; whole-renderer
interval was 901.6 ms. Removing only the brush would still leave hundreds of
milliseconds. Removing only composition would also miss 30 FPS.

The baseline's eight callbacks starting during the stroke consumed 11.847 s
wall time but only 2.353 s scheduled CPU time, including their tails after the
stroke marker. Approximately **80% was off CPU**. Nested bounded waits consumed
9.717 s wall time, including 0.591 s running. Calling that all "CPU rendering"
would misdiagnose GPU/driver synchronization as arithmetic on the CPU.

There is also real CPU overhead. Command finalization consumed 880 ms scheduled
CPU in that window. The 432 action-filtered owner stack samples attribute 39.35%
inclusively to command finalization, 23.15% to command-buffer freeing and 7.18%
to the Vulkan allocation entry point. Inclusive percentages overlap. This is
substantial command/driver work, not CPU shading of the whole photo.

A long callback allows more motion to accumulate. The next batch then covers
more tiles and more ordered contacts, becoming more expensive again. The old
32.26 MP/update backlogged footprint is therefore not a fixed hardware workload
that every 30 FPS implementation must process each frame.

## Changes and causal evidence

1. **Group independent source decodes before adjacent display draws.** Previously
   each decode interrupted a run of draws to the same large retained attachment.
   The stable grouping preserves decode order and layer/draw order, stops at
   source-slot overwrite hazards and other job types, and bounds each group to
   64 distinct written views. This reduces pass and driver work. With the
   original 64 MiB cache and unchanged brush, slow motion improved from about
   11.3 to **23.9/s**; GPU composition fell from 49.6 to **13.5 ms**. Fast motion
   improved to 4.8/s but still entered the backlog cycle. These measurements
   establish a useful improvement without assuming the brush was the first
   bottleneck. They do not measure physical attachment spill bytes.
2. **Retain enough decoded source tiles to avoid repeated eviction/conversion.**
   A 64-tile cache holds only 4.19 MP of Float32 pixels. With grouping and
   128 tiles, slow motion reached **28.7/s**, composition 9.8 ms, while fast
   motion remained 5.6/s. The fast cache still missed about 140 times/update.
   The final upper tier is 256 tiles / 256 MiB. An exploratory 512-tile cache
   gave 27.8/s versus 27.5/s for a repeat at 256, so its extra 256 MiB was rejected.
3. **Skip mathematically ineffective contact evaluation.** For normal uniform
   pigment with incoming stroke coverage already 1, each requested alpha is
   clamped to at most 1. Thus `max(old, requested) - old` is zero for every
   remaining contact, and the color and coverage cannot change. Return the
   original values before entering that loop. All other pixels retain the
   original ordered evaluator, precision and blend operations. This saves
   arithmetic/contact reads on previously covered pixels; it is not a claim
   that every brush pixel or the whole GPU workload is bandwidth bound.
4. **Group contiguous mip tiles by row.** One dispatch uses Z workgroups for
   adjacent tiles, retaining the same 2×2 arithmetic, immutable first-tile
   parameters and level dependencies. There is no storage worklist, intermediate
   image or precision reduction. This reduces dispatch/binding commands rather
   than eliminating required mip texels. The earlier rejected worklist prototype
   is a different algorithm. With the correct brush shortcut, grouping and
   256 tiles, fast motion reached **25.7 and 27.5/s** under full phase probes.
5. **Admit a larger bounded upload window on devices with headroom.** The 16 MiB
   window still forced 22 synchronous waits in the repeat above, mostly during
   the first two seconds: 964 ms wall / 199 ms scheduled CPU in total. Raising
   it to 64 MiB removed those measured waits and produced 28.5/s under full
   probes and **33.3/s with light probes**. CPU/GPU phase instrumentation itself
   adds submissions and timing passes; it must not be treated as free.

Steps 3 and 4 were combined in the accepted experiment; their independent
speedups have not been measured. The staged results also interact through
backlog size, so their speedups must not be multiplied as independent factors.

The source/upload limits now reuse the native renderer's existing measured
headroom admission snapshot: below 1 GiB display allowance, retain 64/16 MiB;
at least 1 GiB, admit 128/32 MiB; at least 2 GiB, admit 256/64 MiB. Source pixels
use at most one eighth of that allowance in the larger tiers. Unknown budgets,
Web's default budget and background snapshot workers retain the old ceilings.
The upper tier adds at most **192 MiB resident sources + 48 MiB in-flight uploads**
over the old limits. Allocation is lazy. This snapshot is not a reservation or
dynamic protection against later memory pressure. New trace counters publish
the actual admitted limits and peak upload charge.

All rendering changes are in shared Rust/WGSL; Android's FIFO presentation and
command-buffer reclamation safeguards are unchanged.

## Final before/after measurements

The baseline and final full-probe captures have no trace error diagnostics and
matching 17.164% zoom counters. The final light-probe capture also has no trace
errors. Each action is fully retained. Both full-probe runs report thermal
status 0 before and after; available system memory was approximately 4 GiB.

| Fast stroke measurement | Matched baseline | Final full probes | Final light probes |
| --- | ---: | ---: | ---: |
| Actual canvas latches/s | **0.70** | **29.99** | **32.29** |
| Latch-gap median / p95 | 1125 / 2233 ms | 25.2 / 50.1 ms | 25.1 / 42.5 ms |
| Largest latch gap | 2233 ms | 242 ms | 246 ms |
| Input owner-queue median / p99 | 735 / 2134 ms | 11.9 / 126 ms | Not instrumented |
| GPU paint interval, mean | 419.1 ms | 4.77 ms | Not instrumented |
| GPU prediction interval, mean | 21.91 ms | 6.35 ms | Not instrumented |
| GPU composition/mips interval, mean | 456.7 ms | 12.57 ms | Not instrumented |
| Whole GPU interval, mean | 901.6 ms | 23.75 ms | Not instrumented |
| Undo: injected key to first canvas latch | 753 ms | 359 ms | 374 ms |
| Redo: injected key to first canvas latch | 625 ms | 496 ms | 397 ms |

The final slower-motion full-probe run reaches **36.78/s**, with mean GPU
intervals of 1.40 ms paint, 6.01 ms prediction and 9.42 ms composition/mips
(16.88 ms total), versus the previous slow control's 11.3/s and 73.45 ms total.
It also has zero trace errors. Undo/redo first latches are 346/460 ms.

The GPU ratios include the benefit of preventing backlog, rather than representing
isolated kernel speedups on identical input batches. Final light-probe hover is
84.4/s before drawing and 80.0/s afterward, similar to the old 83–85/s hover path.
During drawing, input responsiveness improves because the shared owner no longer
waits for second-long frames. This is not a physical pen latency measurement.

The full-probe final stroke has 302 callbacks, averaging **27.97 ms elapsed /
26.36 ms scheduled CPU**. Their mean scheduled CPU breakdown is:

| Owner region | Scheduled CPU / callback |
| --- | ---: |
| Preparation | 2.07 ms |
| Committed paint encoding | 2.53 ms |
| Prediction encoding | 2.71 ms |
| Composition encoding | 5.67 ms |
| Publication, including command finalization/submission | 11.01 ms |

Other callback work accounts for the remainder. Nested across these regions,
command finalization averages 8.26 ms/callback and binding creation 3.03 ms.
Do not add these nested rows to the table. The action-filtered CPU profile agrees:
32.31% inclusive command finalization, 17.29% binding creation and 10.24%
command-buffer freeing. CPU work now occupies a much larger fraction of callback
time because the long waits have been removed. It remains a performance target,
not evidence that the GPU is unused or that the application rasterizes on CPU.

Vulkan allocation API scopes fall from 2119 to 134 per callback; instrumented
binding creations from 1134 to 397. Absolute ten-second counts rise because the
final build produces far more updates. Between first/last canvas publications,
source misses fall from 444.5 to 18.4/update and composed output falls from
31.62 to **6.84 MP/update**. These deltas are not exact per-action totals.
The final trace confirms 256 MiB resident admission, 64 MiB upload admission,
63.25 MiB peak charged uploads, **zero upload-drain counter increments** and
no `capy.bounded_wait` scopes during the stroke.

Undo/redo still have substantial composition work. Final full-probe undo's
296 ms callback includes 37.7 ms restoration and 243 ms composition; redo's
458 ms callback includes 0.006 ms restoration and 452 ms composition. A redo
thumbnail query also takes up to about 80 ms across the response window.
During drawing, all thumbnail queries together take 75 ms and all separately
instrumented parsing 8.4 ms across ten seconds. Thumbnail transport is secondary
for this drawing result; it is still relevant to individual history stalls.

For the current full-probe work, perfect overlap with unchanged mean CPU/GPU
costs would suggest roughly `min(1000/26.36, 1000/23.75) = 37.9` updates/s before
other owner work and presentation. This is a **current-work scheduling bound**,
not a silicon ceiling: changing cadence changes the work. Reaching 60 FPS would
require reducing both those intervals below 16.67 ms; eliminating upload waits
alone cannot do it. More reuse of binding/command state and less composition
work are the next measured targets. A guaranteed 30 FPS frame deadline also
requires removing cold allocation/source preparation stalls; average 30+ FPS
does not establish that stronger claim.

## Memory bandwidth and the ceiling

The [bandwidth report](android-pen-bandwidth-20260920.md) derives a conditional
67.2 GB/s platform peak from Qualcomm's supported LPDDR5x rate and an assumed
64-bit interface. This tablet's actual bus width, memory clock and sustained GPU
DRAM bandwidth remain unmeasured. The available traces do not establish memory
saturation. A peak-bandwidth division cannot honestly give a proven device FPS
ceiling under those conditions.

The relevant arithmetic nevertheless explains why 30 FPS is plausible. At
17.164% zoom the fast ellipse is approximately 3030×1742 pixels in radii, with
15,265 pixels traveled per second. At 30 updates/s, motion advances about
509 pixels/update. A radius-1024 brush sweeping that distance covers approximately
`pi × 1024² + 2048 × 509 = 4.34 MP`, before tile padding and old/new prediction
damage. This is far below either the 60.22 MP full canvas or the old backlogged
32.26 MP/update. It is a geometric estimate, not a measured dirty-tile count.

For a fixed 6 MP illustrative footprint, one paint read/write (40 B/pixel),
fused two-input composition (48 B/pixel) and five mip reductions (26.641 B/pixel)
total **0.688 GB/update**. Adding a once-per-pixel prediction read of color and
coverage plus color output (36 B/pixel) gives **0.904 GB/update**. At 30 FPS those
models require 20.6 or 27.1 GB/s. The conditional 67.2 GB/s peak would correspond
to 97.7 or 74.3 updates/s respectively; hypothetical sustained 30 GB/s would
correspond to 43.6 or 33.2/s. These are sensitivity calculations, not achieved
bandwidth, an exact traffic audit, or an absolute ceiling. Real prediction area,
multiple batches, source decode, cache/compression, attachment behavior, history,
CPU scheduling and presentation alter both sides of the model.

Conversely, repeatedly processing the entire canvas at 114.641 B/pixel would
require 6.904 GB/update, or 207 GB/s at 30 FPS. That particular unfused,
uncompressed DRAM model cannot reach 30 FPS at the conditional peak. It does
**not** prove the actual dirty-tile algorithm cannot reach 30 FPS. The measured
30+ FPS result directly disproves that conclusion for this replay.

Required working-pixel reads/writes, mip reductions and prediction remain.
The avoidable work removed here is repeated source conversion/upload, interrupted
passes, excess mip command encoding, ineffective saturated-pixel contact loops
and forced upload drains. The common dry-brush and single-batch prediction paths
already avoided preliminary pixel copies; these changes do not take credit for
removing those copies again.

## Correctness and rejected experiments

The reference brush test compiles the original full evaluator with the new
shortcut disabled and compares exact Float32 working pixels, coverage and
prediction for U8, U16 translucent and F16 extended-color cases across variable
pressure/cadence and two strokes. A 4096×3072, actual 2048 px opaque case also
compares exact native backing. It passes on host and Wacom GPUs. Retained mip
updates match the original scratch reduction exactly on the Wacom; 22 host
live-display tests cover edited, transformed and retained display behavior.
Six source tests pass; the existing benchmark test remains ignored. The optimized
Android benchmark build/lint and Web target compilation pass.
The two existing native G-Pen opaque-source regression tests also pass.

An earlier **inside-loop** break at coverage 1 passed small GPU tests but made
dark seams in the actual 2048 px primary-app stroke. It was rejected, the
oracle expanded to the real brush size, and the branch moved outside the loop
to test only incoming saturation. All `saturated-*` / `saturated256-*` timings
are rejected regardless of their speed. A separate plain-contact specialization
failed exact Float32 equivalence and was also reverted. Primary screenshots
sample 39,600 positions safely inside the opaque stroke as an additional seam
check; this is not a substitute for the GPU pixel-reference tests.

## Reproduction and evidence

Raw data live in `artifacts/wacom-30fps-20260920/`: APKs, matching unstripped
libraries, source patches for rejected variants, setup/result screenshots,
40-second Perfetto captures, simultaneous 199 Hz simpleperf stacks, action
markers, memory/thermal state, logs, reports and SHA-256 manifest. See its README
for labels and exclusions. The tracked machine-readable summary is
`measurements/android-pen-30fps-20260920.json`.

Build the same `art.capycanvas` benchmark variant with `-PcapyOptimize` and
`-PcapyAbi=arm64-v8a`. Restore the photo, select an empty paint layer, set 2048 px,
open Diagnostics and Layers, then press Ctrl+0 **after layout settles**. Verify
17.164% in the trace counter and the setup screenshot; a nominal "Fit" command
alone does not establish matched geometry. Run `capture-run.py LABEL 1` for full
phase probes or `capture-run.py LABEL 1 capture-light.pbtxt` for throughput;
undo the previous workflow stroke before repeating. Parse with
`tools/performance/android-pen-report.py TRACE MARKERS --processor PROCESSOR`.

SurfaceFlinger `latchBuffer SurfaceView…art.capycanvas` events establish actual
canvas consumption, not physical nib-to-photon timing. GPU observations arrive
asynchronously and their intervals include inter-submission idle time. CPU
scopes are nested and must not be summed indiscriminately. These are short
qualification runs, not a long-run thermal or p99 guarantee.

`baseline-repeat-fast` is excluded because setup had not reached the matching
zoom. `control-slow` has trace parse diagnostics and `grouped-slow` has the
wrong zoom; neither is used for the accepted A/B. `upload64-fast` retains 50
systrace parse failures alongside 25 nonstandard lower-case LMKD pressure prints,
consistent with the parser rejecting those records. These are real pressure
notifications, not a reason to erase trace diagnostics or infer a sole cause of
startup stalls. Its clean light-probe companion and all three final captures
provide independent error-free presentation evidence. No app termination occurred
in those accepted runs.

The original unmapped-buffer readback failure, historical 2 Hz / 8 ms prediction
device loss, unexplained navigation pause and original freeze remain unqualified.
Successful drawing workflows do not establish fixes for them.
