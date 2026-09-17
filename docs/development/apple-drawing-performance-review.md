# Large-photo drawing: algorithm review — 2026-09-17

The current tile-based design is appropriate, but approximately 50–60 ms p99 is
not an established hardware limit. The shared follow-up below removes redundant
preparation and overlaps bounded submissions. Retain it; further optimization
should follow measured composition costs rather than a new rendering architecture.
No brush-size, preset-name, document-name or gesture special case is justified.

The subsequent physical retest of the current qualified iPad runtime reports
approximately **60 ms p99**. A later read-only capture of the open drawing shows
CPU median/p95/p99 **33.40/72.86/82.16 ms** and GPU
**31.35/63.52/70.35 ms**. Both are elevated; perceptual impact remains unconfirmed.
These are retained Diagnostics values, not a controlled capture. They do not demonstrate closure
of the large-photo tail or establish a hardware floor. A five-minute CPU/Metal
attachment completes without restarting the drawing, but contains no target GPU
execution or CPU encoder rows. No drawing-completion reply is received during
that window. That preliminary trace cannot attribute the reported cost.
Evidence is `artifacts/apple-photo-cost-v4/`; the controlled device replay below
supersedes the need for another uncoordinated Pencil recording.

## Physical iPad reproduction without another Pencil retry

A private iPad wrapper now runs the existing `photo_interaction` replay at
`56c19fc8`, whose runtime sources match the preceding `20cecad7` integration.
It reads the same saved 9504×6336 drawing, uses 570.7 px G-Pen, 180 on-canvas
drawing updates with eight synthetic samples each, manual 8 ms prediction and
the same fixed 768 MiB display admission as the local comparisons. It adds a
native autorelease pool around each frame, matching the host's task lifetime;
no production renderer or input path is changed.

Completed drawing updates have median/p95/p99 **30.966/42.919/55.964 ms**,
maximum **65.270 ms**. The three slowest are final pen-up and early updates 2
and 5. Composition's elapsed intervals account for 4.702 of 5.564 seconds
(84.5%); they include command finalization and waits for earlier GPU work.
This reproduces a similar tail on the actual iPad without UIKit input or the
Diagnostics panel. This unprofiled run alone does not identify the expensive
GPU passes.

The following 360 cached zoom updates have median/p99 **1.820/8.213 ms**.
Exact native Undo/Redo passes, and the original drawing file is unchanged.
Evidence is `artifacts/apple-photo-ipad-replay-v1/`. The eight-sample batching,
fixed admission and offscreen completion measurement remain explicit limits:
this is neither a replay of the user's current Pencil gesture nor native
presentation/perceptual acceptance. Use this reproducible device workload for
pass attribution before requesting another physical drawing retry.

A subsequent five-second Metal/CPU recording correlates 83,936 GPU intervals
to 112 replay updates, with no observed identity/order conflicts. All observed
encoders have GPU matches in the first 111 updates; the last update is partial.
Those 111 updates contain a median **529 observed encoders**, **15.357 ms** of
GPU active time and **41.684 ms** total completion. In the slow captured early
updates, observed GPU activity is 17–19 ms within 78–88 ms completion intervals.
This capture ends before pen-up. GPU stages overlap, so per-label durations are
unioned and must not be added together.

Command finalization appears in **1,537 of 2,260 ms** of sampled running render
CPU stacks (68.0%). These inclusive samples overlap their enclosing composition
stacks. Copies, scene draws, committed/predicted painting and display reduction
all contribute GPU work. This makes command count and CPU/driver materialization
a measured optimization target; it does not establish a memory-bandwidth floor
or promise that the frame can reach its observed GPU-active duration.

Profiling increases elapsed time, so retain the unprofiled distribution above
as the baseline. The first recorder's observer expired during finalization and
its incomplete trace is rejected. The successful retry keeps the same recorder
alive through processing and preserves exact final artwork and Undo/Redo.
The ordinary published review is restored with all eleven recoveries, saved
files and settings preserved. No new physical Pencil acceptance is claimed.

### Rejected source-upload copy removal

One shared candidate decodes native U8/U16 samples directly from an uploaded
storage buffer, removing the temporary integer textures and their upload copy.
Five existing decoder regressions pass, including exhaustive integer-code,
profile, alpha, ownership and upload-bound checks. The separate standalone
decode benchmark remains ignored. Numerical tolerances are unchanged; the
resident-byte assertion changes only by the removed texture allocation.

Sixteen Mac replays compare four workloads in both execution orders: ordinary
and smaller input batches, a small zigzag brush and a wider circular brush.
Final native pixels, exact Undo/Redo and primary work counts match. The large
brush's median completion does not improve meaningfully. A subsequent unprofiled
iPad replay confirms **30.966 → 30.959 ms** median, **55.964 → 53.678 ms** p99 and
**65.270 → 65.708 ms** maximum. This is one device run per version, separated by
profiling/build preparation, not a repeated controlled device pair. The small
tail movement cannot establish a reliable improvement; one cached-zoom source
miss also differs and remains in the comparison record.

The candidate is rejected as a performance change. After its removal, all 404
qualified runtime source hashes match the published baseline. No new product
path or test is retained. Evidence and the exact rejected patch remain private under
`artifacts/apple-photo-buffer-decode-v1/`. The ordinary iPad Release is restored
with all eleven recoveries, saved files and settings intact.

A subsequent fast-forward to `a1150ece` brings shared proof-rendering and
Web/Android proofing changes. The measurements above remain scoped to
`56c19fc8`; the installed ordinary review is unchanged. They are not a performance
qualification of the newer main revision. All 454 shared UI tests, the focused
Metal proof/CPU-reference regression and the Apple bridge compile check pass on
the newer source. Neither Apple Release nor device performance is requalified by
those checks.

The algorithm review therefore supports the existing sparse architecture, not a
claim of optimal frame time. A typical replay update composites 4.26 million
pixels, rather than the full 60.22 million. Contact evaluation, ordered blending,
prediction and mip work remain necessary. The physical trace now identifies
CPU/driver command preparation as substantial additional work. Any next
optimization should reduce commands across those shared stages and demonstrate
repeatable gains on multiple workloads; another isolated upload tweak or a
larger queue is not justified. Preserve pixel/history semantics and existing
memory ceilings. The GPU-active interval is neither a frame-time lower bound nor
a promised target. Large-brush perceptual acceptance remains open; a high p99
alone does not override the user's smooth-drawing/rare-miss criterion.

### Rejected source-decode scheduling extension

At `a1150ece`, a second small candidate moves pending source decoding before
preceding independent clears/draws so normal layers can share their existing
render pass. It stops at a reader of the reused cache texture and introduces
no storage or shader path. Sixteen reversed-order Mac replays preserve native
artwork, Undo/Redo and primary work counts, but show no repeatable speed gain:
ordinary-brush medians are **25.764 → 26.256 ms** and **25.724 → 25.265 ms**;
wide-brush medians are **47.298 → 47.207 ms** and **46.805 → 47.130 ms**.
Small-brush medians regress in both pairs. The first ordinary baseline's
113.781 ms p99 remains in the report, rather than being filtered away.

The candidate and its unexecuted additional regression are removed; no device
installation is warranted by these results. Evidence is
`artifacts/apple-source-order-v1/`. The native-root comparisons alone do not
qualify composite output. Retain the existing simpler ordering. These two
negative experiments narrow the next performance action: another scheduling
micro-change is not supported, and a broader batching design needs evidence
commensurate with its complexity. Continue the remaining feature/parity gates
while retaining the unresolved large-brush acceptance; do not keep changing
the renderer merely to obtain a lower diagnostic percentile.

## Corrected workload and remaining avoidable passes

The original `photo_interaction` circles used the viewport dimensions, although
the document occupied only 1900.8×1267.2 pixels of its 2752×2064 surface at 20%.
For the 570.7 px brush, an ideal circular footprint touched the document during
only about 26% of the path. Earlier paired comparisons remain comparisons of
identical work, but are poor evidence for sustained on-canvas circles. The example
now fits both paths within the visible document, accounting for brush diameter.
No production input behavior changes.

The corrected replay reads the user's latest saved 9504×6336 sRGB/U8 drawing
through the production reader. All measured drawing frames produce dabs.
Before further optimization, completion p99 ranges from 46.73 to 74.30 ms with
2, 8 or 16 samples per frame. Composition dominates its recorded wall phases.
A CPU stack sample records 5880 of 6449 frame-submission observations inside
composition, including 2230 in command finalization and 2412 in its explicit
wait. These sampled wall observations overlap nested calls; they are not GPU
execution measurements. A separate local Instruments recording fails to finalize
after its target exits and is closed; its incomplete output supplies no GPU claim.

One specific avoidable cost is source decoding between a scratch clear and its
first draw, which prevents their existing render-pass merge. The shared scene
planner now places that decode immediately before the adjacent clear. Decoding
writes the separate source cache, so it has no dependency on that scratch clear;
other jobs retain their order. This reuses the existing merge without another
rendering path, larger cache, changed precision or brush-specific policy.

Two reversed-order comparisons give these completed-frame medians:

| Samples per frame | Before (ms) | After (ms) | Median reduction |
| --- | --- | --- | --- |
| 2 | 15.91–16.17 | 15.53–15.70 | 1.3–4.0% |
| 8 | 29.00–29.34 | 26.57–26.93 | 8.2–8.4% |
| 16 | 43.75–44.60 | 39.43–39.51 | 9.7–11.6% |

All twelve runs preserve exact native Undo/Redo and produce identical final
paint roots, dab counts, composition area, source misses and display submissions
within each pair. The eight-sample completion p99 is still 51.45–53.77 ms after
the change versus 54.36–54.39 ms before. This is a modest general improvement,
not closure of the full tail. The replay uses fixed 768 MiB display admission,
synthetic 240 Hz input and manual 8 ms prediction on Mac; it does not reproduce
physical iPadOS prediction, memory admission or Pencil-to-screen latency.
These local comparisons leave the physical review session untouched.
The final source passes 31 focused Metal regressions covering display pixels,
partial edges/mips, masks/prediction cancellation, source uploads, exact history
and device replacement; Web and iOS-target renderer compilation also pass.
Both subsequent Release builds pass without warnings. The iPad review is then
updated in place after stable complete backups: all eleven recovery records,
saved files and settings remain byte-identical through installation. Ordinary
review is open at Recovered Drawings, with recording disabled. The user is asked
for the same 570 px circles, perceptual lag and CPU/GPU p99. The user again reports
about 60 ms; they do not identify the metric or confirm whether visible lag remains.
Deployment evidence is `artifacts/apple-clear-pass-device-v1/`. Mac artist apps
remain live and unchanged; its candidate is built in a separate directory.
Keep the architecture. Neither the old nor corrected replay proves
that 60 ms is an unavoidable hardware limit.

## Pass attribution and redundant paint initialization

Following the repeated 60 ms report, a disposable Mac replay adds timestamps to
individual render/compute passes. Its 120 eight-sample frames have a median of
313 instrumented passes: 81 brush, 132 scene and 65 display-reduction passes,
plus other initialization/history work. The median union of measured intervals
is 11.54 ms within a 20.62 ms first-to-last-pass span. Intervals overlap, and
uninstrumented copies may execute between them. These numbers are not additive
GPU utilization, physical iPad attribution or an uninstrumented speed estimate.
The temporary probes are removed from product code after recording.

This supports reviewing pass overhead as well as brush arithmetic; it does not
justify declaring a bandwidth floor or rewriting tile storage. The ordinary
eight-sample replay composites a median 4.26 million pixels per frame, compared
with the document's 60.22 million pixels. It has already avoided full-image
recomposition. Sparse tile planning, ordered blending, prediction and bounded
caches remain appropriate. More invasive batching or shader changes need evidence
that their benefit outweighs added resource/dependency complexity.

One redundant operation is directly established: new inactive paint-color
surfaces are separately cleared, although their first use always overwrites them
with a full-page copy or the full-page post-stroke edge pass. The shared renderer
now removes that clear and its state flag, including the obsolete transform
assignment. Primary color and wetness initialization remain intact. This reduces
code and render passes without adding a brush, document or platform branch.

Two reversed-order comparisons, 180 drawing frames each, preserve exact native
artwork/Undo/Redo, dab counts, composition area, source misses, submission counts
and retained display bytes in all sixteen runs:

| Workload | Median completion before → after (ms) | Median reduction |
| --- | --- | --- |
| 2 samples/frame, 571 px circles | 15.84–16.38 → 15.31–15.49 | 3.3–5.4% |
| 8 samples/frame, 571 px circles | 26.64–27.97 → 25.27–25.88 | 5.1–7.5% |
| 8 samples/frame, 96 px zigzags | 7.87 → 7.42–7.44 | 5.5–5.7% |
| 8 samples/frame, 1024 px circles | 49.32–49.45 → 47.12–47.31 | 4.3–4.5% |

The 571 px eight-sample p99 remains 53.85–54.77 ms versus 58.20–60.25 ms before.
The second wide-brush p99 regresses from 91.42 to 92.61 ms, so this is not an
every-percentile improvement. Fixed 768 MiB display admission and manual 8 ms
prediction still limit transfer to the physical iPad. The local evidence is
`artifacts/apple-photo-pass-v5/`. Twenty-six focused Metal regressions pass,
covering cold paint, surface retirement/reuse, native blends and wet media,
source brushes, prediction/masks, sparse G-Pen/Pencil, transforms and exact history.
Web and iOS-target compilation pass. Neither artist app is restarted or updated.
The simplification joins the native photo/16-bit qualification milestone; it does
not close the reported delay or require another device installation for this small
gain alone.
Visible drawing lag warrants a coordinated physical trace; a high p99 alone does
not override the user's accepted smooth-drawing/rare-miss criterion.

Keep the sparse planner and the finalization ordering correction published in
`0a7b7d7e`, alongside the small clear-pass corrections. None establishes a
physical hardware floor. Do not start a storage/shader rewrite or increase cache
limits to chase a theoretical number. If visible stalls remain, attribute their
GPU passes on the physical build before choosing another change. The current
Mac ten-minute watercolor result also supports retaining the shared design,
within its separately documented
[workload scope](../../apps/layer-apple/PERFORMANCE.md#sustained-mac-watercolor-after-shared-integration--2026-09-17).

## Current-code decision

Rechecking main at `1f96d5a6` confirms that the sparse planner, finalization
overlap and both clear-pass simplifications are already present. The earlier
algorithm table below describes the build that motivated those changes, not
additional unimplemented work.

| Current stage | Remaining cost | Decision |
| --- | --- | --- |
| Shared touched-tile plan | O(K log T) planning for K contact/tile intersections and T touched tiles; some resident-page lookups still scan N pages | Keep the single plan. O(TN) lookup work is a candidate only if profiling makes it material. |
| Ordered contact evaluation | Each pixel tests its tile's first-to-last contact span, including gaps when a path leaves and returns | Correct, but not asymptotically optimal for every path. Measure shader cost before adding per-tile contact lists. |
| Paint preservation | Full color/coverage tiles are copied before clipped updates | Possible bandwidth savings, but in-place edits must preserve destination reads, prediction and nonlocal brush dependencies. No rewrite is justified yet. |
| Composition and submission | Bounded batches now overlap command finalization with GPU execution; source uploads still have their own limits | Retain the measured improvement. Attribute remaining GPU passes and synchronization before changing batching or storage. |
| Display and prediction | Completed drawing/mips are reused; unfinished prediction is rebuilt | Retain. Do not trade image precision or input fidelity for a favorable benchmark. |

For the dry shader, tile planning is only one term: contact evaluation can scale
with the sum of each tile's shaded area multiplied by its retained contact-span
length. A small distinct painted area therefore does not imply little arithmetic.
Conversely, dirty-rectangle traversal can visit untouched tile coordinates, but
the sparse membership check prevents painting/compositing those tiles. Neither
observation establishes the dominant device cost.

At 120 Hz, 50 ms spans six refresh periods. It is a tail statistic, not evidence
of sustained 20 fps or a measurement of input-to-display latency. A frame can
also wait for work encoded in an earlier phase, so the large composition CPU
interval does not prove that composition shaders dominate. The bandwidth
estimates below isolate illustrative transfer costs; they do not predict a
complete frame or prove how much faster this workload can become.

Recommendation: keep the current architecture and qualified shared changes.
Do not declare the remaining 50 ms unavoidable. The physical replay attribution
above now supplies the bounded GPU/CPU investigation. Use it to focus any further
work on shared command batching, retaining only a broadly useful change with
repeatable replay gains, unchanged pixels/history and bounded memory.
If drawing meets the accepted smoothness standard, prioritize the remaining
release gates. There is no evidence here that another architecture or larger
cache is necessary to finish the Apple goal.

## Diagnostics overhead and interpretation of the repeated 60 ms report

The displayed percentiles cover the last 120 drawing updates, rather than a
time window or every screen refresh. With a full window, p99 is the second
slowest update. Values remain while idle. A roughly 60 ms p99 does not establish
60 ms on every frame, and these observations do not measure Pencil-to-screen
latency. This is an interpretation of the metric, not dismissal of a slow update.

A bounded comparison at `1f96d5a6` uses one private replay executable with
renderer timestamps off/on, then on/off. Each run draws 180 frames with eight
samples per frame, the same saved large photo, 570.7 px brush, on-canvas circles,
manual prediction and fixed 768 MiB display admission. Completion medians are
26.15/25.86 ms in the first off/on pair and 25.81/25.75 ms in the second on/off
pair. Whole-run p99 is respectively 88.19/54.43 and 53.89/54.38 ms. The larger
first off-run spikes are retained; the result does not show a repeatable timing
penalty or justify removing GPU Diagnostics. All four runs preserve identical
paint roots, work counts and exact Undo/Redo. The source drawing is unchanged,
all replay processes are closed and the temporary example edit is restored.

Reexamining both earlier qualified eight-sample candidate runs locates their
five slowest updates at frames 0, 1, 2, 5 and final pen-up (179). Those updates
take approximately 52–62 ms, principally inside composition's wall interval,
which includes submission/waiting for earlier GPU work. Startup and pen-up
therefore deserve separate attribution from steady motion; they remain part
of the user experience and the reported full-run statistics. This is local Mac
evidence, not proof that the physical iPad's spikes have the same cause. The
comparison also excludes native panel/recorder overhead and physical input.
Evidence is `artifacts/apple-photo-telemetry-v1/`.

The ordinary iPad review subsequently receives the complete published milestone. All
eleven recoveries, saved files and settings are verified byte-identical through
installation, and the restored Recovered Drawings screen is reviewed. Recording
is disabled. A preceding private UIKit preview fixture terminated before any
capture; it supplies no form acceptance and has been replaced by the ordinary
Release. Restoration evidence is `artifacts/apple-ipad-final-qualification-v1/`.
No new physical performance pass is claimed. Keep the remaining investigation
focused on the actual slow Pencil frames; another general renderer rewrite or
uncoordinated device trace is not supported by these results.

The later populated-form hardware review succeeds, and the integrated ordinary
Release at `20cecad7` is restored with all eleven recoveries and artist files
preserved. Its separate sustained ProPhoto/U16 watercolor run does not reproduce
the 570 px G-Pen workload or close this remaining tail. See the
[current qualification record](apple-handoff.md#large-photo-composition-review--2026-09-17)
for the source-scoped checks and remaining limits.

## Evidence and its limits

The user confirms faster physical iPad drawing after the preview-composition
and paint-region changes. The retained post-test capture reports CPU
median/p95/p99 of **12.08/47.32/55.75 ms**, and GPU intervals of
**10.89/44.93/54.30 ms**. This is consistent with the reported roughly 50 ms p99.
It is not a controlled device A/B measurement: gesture, document contents and
camera differ from the earlier capture (now 20%, 0 degrees; previously 22%,
11 degrees). The earlier repeated-zoom regression remains physically closed.

CPU diagnostics include prepare, encode, submit and synchronous waits inside
those operations. GPU intervals include scheduling gaps. They overlap; adding
them does not yield frame latency. Neither measures Pencil-to-screen latency.

The short iPad trace from before these latest changes retains only the drawing
tail, with ten viewport encoders. It exposed many small scene/blit passes.
A fresh local Mac sample of the installed candidate's offscreen replay confirms
heavy time in command materialization and explicit submission waits. In the
main replay call tree, 1,947 of 3,389 sampled renderer observations occur in
display composition's chunk submission (1,328 in its wait); another 745 occur
in source-paint initialization, largely in submission/waiting. These are sampled
wall observations, including blocked threads, not independent CPU/GPU timing
totals or physical iPad attribution. Private evidence is under
`artifacts/apple-photo-lag-v2/` and `artifacts/apple-photo-lag-v3/`.

## First-principles cost

The relevant work is the newly affected area plus the short prediction tail,
layer composition and display updates. Total document size determines storage
and cold-data costs, but should not force a full-image repaint per sample.

The 9504×6336 fixture contains 60,217,344 pixels. RGBA32Float color occupies
16 bytes/pixel: one full-image read and write is 1.927 GB. The M4 iPad's
advertised 120 GB/s memory bandwidth gives an ideal **16.06 ms** transfer time
for that operation alone. This is why avoiding full-image work matters.
[Apple's device specifications](https://support.apple.com/en-us/119891)
provide the hardware figure; sustained bandwidth is not established here.

For comparison, a 256×256 color tile is 1 MiB and its R32Float coverage is
0.25 MiB. Copying both, then reading and writing both once in a dry-paint pass,
accounts for about 5 MiB of logical traffic per complete tile, or 80 bytes per
pixel. An illustrative one-million-pixel update therefore accounts for 80 MB,
whose ideal bandwidth-only time is **0.67 ms**. This is not a frame-time
prediction: it excludes brush arithmetic, repeated contact evaluation,
prediction, other layers, mips, attachments, driver overhead and synchronization.
Caches and tile memory also mean logical traffic is not measured DRAM traffic.
The model cannot prove a sub-millisecond frame, but it cannot justify 50 ms as
an unavoidable consequence of the image's size either.

For a round brush of diameter d moving distance L without crossing itself, the
swept area is approximately dL + pi*d*d/4. A 570 px brush moving 1000 document
pixels therefore covers about 825,000 pixels, before tile rounding. Revisiting
pixels reduces the distinct area but can increase contact evaluations. Fast
motion, sample batching and zoom determine this work; the 61 MP document size
alone does not. The useful cost model includes both transferred bytes and
per-pixel contact evaluations, plus CPU encoding, synchronization and cold
resource costs. Neither peak bandwidth nor one diagnostic percentile supplies
all of these terms.

## Original algorithm assessment before the shared follow-up

| Stage | Current property | Assessment |
| --- | --- | --- |
| Input/prediction | Committed prefix is retained; prediction rebuilds the unfinished tail | Appropriate. Preserve samples, native/manual policy and committed history. No evidence here supports reopening the earlier prediction algorithm fix. |
| Contact evaluation | Each touched tile evaluates an ordered first-to-last contact span; the shader computes exact coverage | Correct general restriction. Interior gaps in the span and repeated per-tile bounds scans remain, but shader micro-optimization is lower priority than measured submission costs. |
| Allocation/source loading | Persistent paint/state allocation still iterates the batch's enclosing rectangle; new native paint pages load their original contents | Incomplete sparsity. Execution skips untouched regions, but some preparation still pays for them. Carry one conservative touched-tile plan through allocation, source initialization, copies and drawing. |
| Dry paint copies | Full color/coverage pages are preserved before a clipped update | Correct across nonlocal materials, but dry pointwise edits may admit fewer copies. Defer broader in-place/compute changes until remaining costs are measured; preserve nonlocal brush dependencies. |
| Layer composition | Complete destination-reading predictions now use the existing normal-layer path | Keep. It removes intermediates without assuming a preset, size or gesture. Overlay-only previews, masks and watercolor retain their required semantics. |
| Submission | Display composition submits and synchronously waits after every 16 tiles; source uploads have a separate byte ceiling; native command materialization is also chunked | Measured optimization opportunity. Review whether waits are required by live resource charges instead of tile count. Keep explicit memory and command-buffer bounds; do not simply delete waits or enlarge caches. |
| Display/cache | Bounded retained mip levels and visible detail; repeated zoom reuses completed pixels | Appropriate and physically confirmed for the reported zoom case. Keep working precision and admission limits. |

Source: [paint allocation and execution](../../crates/layer-render-wgpu/src/lib.rs),
[contact bounds/ranges](../../crates/layer-render-wgpu/src/material_sources.rs),
[composition and waits](../../crates/layer-render-wgpu/src/scene.rs),
[upload accounting](../../crates/layer-render-wgpu/src/scene/sources.rs),
[command-buffer bounds](../../crates/layer-render-wgpu/src/submission.rs).

Apple likewise recommends minimizing command-buffer submissions while keeping
the GPU occupied; overly frequent submissions can cause synchronization stalls.
That supports reviewing this scheduling boundary, not an unbounded submission
queue. [Metal command-buffer guidance](https://developer.apple.com/library/archive/documentation/3DDrawing/Conceptual/MTLBestPracticesGuide/CommandBuffers.html).

## Original optimization plan and stopping rule

1. Unify conservative touched-tile planning for pointwise contacts and use it
   before allocation/source loading, preserving the current conservative
   behavior for smudge, liquify, watercolor and spatial effects. This removes
   work and duplicated bounds decisions rather than introducing another path.
2. Measure submission waits separately from encoding and active GPU execution.
   Change scheduling only with explicit staging, texture and command-buffer
   bounds and queue-order correctness. Test bounded overlap or waits driven by
   actual resource pressure before changing the storage architecture.
3. Validate representative sizes, straight/curved/zigzag input, contact presets,
   prediction on/off, masks/transforms, cold/warm artwork and history. Reuse local
   replays; reserve physical retests for meaningful combined changes. Do not
   require the full Cartesian product or tune to one gesture.

Keep an optimization only when it gives a repeatable benefit on representative
workloads, preserves pixels/history and bounded memory, and stays understandable.
Revert neutral micro-optimizations, as with the tested saturation early exit.
Do not promise a particular p99 from advertised bandwidth. The accepted
perceptually smooth/rare-miss standard still governs hardware acceptance;
remaining visible stalls or measured avoidable work justify further focused
optimization. A wholesale atlas/compute rewrite is not justified by the current
evidence.

## Shared implementation follow-up

The follow-up integrates main at `1cb1ebb8` and replaces repeated contact-bound
scans with one [brush tile plan](../../crates/layer-render-wgpu/src/brush_tiles.rs).
Persistent color/state allocation, companion retention, coverage reset, painting,
contact ranges, history capture and sparse composition use that plan. Nonlocal
materials keep their conservative region and contact order. Complete preview
base pages retain their initialization contract; no extra prediction mode is added.

Display composition now divides the existing 16-tile ceiling into two halves:
the CPU prepares eight tiles while the previous eight execute. It waits before
submitting another half. The final half goes with the frame, so it needs no
terminal CPU wait. Source upload byte accounting and native command-buffer
limits are unchanged. The implementation adds no persistent scheduling state,
brush-size threshold or platform-specific renderer path.

Paired offscreen Mac Metal replay against the integrated pre-follow-up renderer:

| Input / brush / path | Median completion before → after, ms | p95 before → after, ms |
| --- | --- | --- |
| 2 samples/frame, 571 px, circles | 4.21 → 4.01 | 15.85 → 14.78 |
| 8 samples/frame, 96 px, zigzags | 11.76 → 10.61 | 22.74 → 18.81 |
| 8 samples/frame, 1024 px, circles | 26.49 → 24.28 | 43.04 → 38.19 |
| 64 samples/frame, 571 px, circles | 59.58 → 40.38 | 79.10 → 59.82 |

Every retained canonical tile is byte-identical. The candidate omits 25–213
formerly allocated color tiles per workload; every omitted digest equals the
canonical fully transparent native tile. Exact Undo/Redo passes in every run.
The largest coalesced case's first-frame preparation falls sharply; its maximum
completion drops from 526 to 109 ms in this pair. Earlier pairs show the same
heavy-workload improvement. Ordinary-input p99 changes from 18.32 to 19.19 ms,
so this is not a claim that every percentile improves. Repeated-zoom medians
remain approximately 1.6 ms.

These are local completion measurements, including offscreen presentation and
queue drain, not physical iPad or pen-to-screen latency. The 64-sample stress
batch represents approximately 267 ms of input and is not a normal frame-cadence
claim. Final qualification has 284 renderer passes, 30 ignored and only the
unchanged filter-reference mismatch (maximum byte error 56), plus four passing
contact integration tests. Both Apple Release builds and Web compilation pass.
The iPad is updated in place, with all eleven recovery records and saved files
preserved byte-for-byte across installation. Its ordinary review app is open at
Recovered Drawings, recording disabled. Physical drawing/Diagnostics and local
large-photo Save As/reopen checks are pending; Mac artist apps are unchanged. Evidence
is retained in `artifacts/apple-photo-lag-v3/`, including both frozen binaries,
source hashes, failed intermediate assertions and final pixel comparisons.

## Algorithmic limits after the follow-up

The dry-contact planner visits each contact's intersected tiles once and inserts
them into an ordered map. With K contact/tile intersections and T distinct tiles,
its map work is O(K log T), producing T tile preparation entries. It no longer
allocates the entire enclosing rectangle or rescans every contact independently
for each destination tile. Exact coverage still belongs to the shader; the
conservative bounds do not discard painted pixels.

That complexity describes tile planning, not the entire renderer. The current
paint encoder still locates resident pages with linear searches, so those
lookups can cost O(TN) for N resident pages. Composition still visits the dirty
rectangle's tile coordinates and tests membership in the sparse set before
encoding a tile. Neither traversal proves a dominant cost: the composition
interval also contains GPU waits. Replacing containers or iterating the sparse
set directly should follow attribution, with clipping and row-order requirements
preserved.

This is not a proof of optimality. Each tile retains a first-to-last contact
range, so a path that leaves and revisits a tile can cause its shader to test
intervening contacts. Full-page preservation also remains before clipped dry
updates. Per-tile contact lists or in-place painting could remove some of that
work, but introduce indexing or resource-dependency costs and must preserve
ordered blending, prediction and nonlocal materials. Current evidence does not
establish either as the next dominant device cost. Keep them as measured
follow-up candidates, not reasons to rewrite the renderer now.

The improvements extend beyond the large-photo gesture. Two paired local 4K
watercolor replays against the retained `90adbb6d` baseline produce byte-identical
final pixels while GPU median falls from about 4.62 to 3.65 ms. A short native
Mac run on `3fbb937b` records 0.897% long active intervals at the 90 Hz target.
These checks support the generality of the shared changes; they do not establish
iPad timing, sustained memory-pressure behavior or physical pen latency. See the
[watercolor qualification](../../apps/layer-apple/PERFORMANCE.md#watercolor-after-the-shared-photo-fixes--2026-09-17).
The next performance decision should use the pending physical iPad result on
this installed build before adding another optimization.

Reanalysis of the retained final replay CSVs puts **82–96% of total CPU frame
time inside composition**, across the four workloads above. For example, the
ordinary replay's slowest frame completes in 26.61 ms and spends 23.00 ms in
composition; the coalesced replay's slowest completes in 108.67 ms and spends
102.04 ms there. These phase intervals include waits for earlier GPU work and
source preparation, so they do not establish that composition's shader is the
bottleneck or that brush execution is free. They do locate the next measurement:
separate command preparation, source-cache misses and queue waits within that
phase before changing contact indexing or texture storage. This analysis reuses
existing measurements, without another simulator/device run. CPU preparation
accounts for only 1–6% in these runs; even eliminating that phase entirely would
remove only that share of the recorded CPU time. The small brush-encoding phase
does not similarly bound GPU brush cost, because its execution can be waited
for later in composition.

Rechecking those CSVs also limits what can be inferred about source-cache misses.
Every frame with new brush dabs in the ordinary, wide and coalesced replays has
at least one source miss. Only 17 of 292 such frames in the small-zigzag replay
have none. Most no-miss frames contain no new brush dabs, so their faster times
are not a controlled warm-cache comparison. The median completion times when
restricted to frames with new dabs are 10.42, 11.64, 24.70 and 40.38 ms,
respectively; the earlier table includes all stroke-phase frames. Miss counts
alone cannot distinguish first-use decoding from repeated eviction or establish
which dominates elapsed time. The coalesced replay has only 48 stroke frames,
so its extreme percentiles are especially sensitive to individual frames.

The next bounded investigation should attribute composition's source preparation,
command materialization and queue waits on the same replay, separating first use
from repeated passes. Then change only the dominant avoidable cost. The current
64 decoded-tile slots and 16 MiB upload ceiling bound different resource
lifetimes; enlarging either without attribution can spend memory without
improving the critical path. Preserve queue ordering, exact pixels/history and
memory ceilings when evaluating less frequent waits or reuse of completed work.
Linear page lookup, bounding-rectangle traversal and contact-span gaps remain
secondary candidates until measured. This keeps the investigation general to
all shared-renderer platforms and avoids another drawing architecture.

The practical recommendation is to keep the current shared design and its
bounded overlap, not claim optimality. A 50 ms p99 represents roughly six
120 Hz refresh periods, but is neither the average frame time nor measured
pen-to-screen latency. If that tail still produces visible stalls, further
focused optimization is warranted. If drawing meets the accepted smoothness
standard, prioritize the remaining parity gates over speculative shader/cache
changes. Retain a further change only after representative local A/B replays
show repeatable improvement with unchanged pixels/history and bounded memory,
then qualify the combined result once on iPad.

Subsequent full Apple integration testing exposed a correctness omission in the
sparse plan: the direct Pencil brush encoder still traversed the enclosing
rectangle, including untouched pages that allocation no longer created. It now
uses the existing tile plan, just as the material encoder does. The native
pixel/save/reopen regression fails before this correction and passes afterward;
the disjoint-contact pixel/allocation test now covers both Pencil and G-Pen.
This correction is not a new timing claim, and the pending iPad review still
uses the preceding installed build. Evidence is in
`artifacts/apple-shared-preferences-v1/`.

Main advanced again to `668ff0a0` during publication. Its shared renderer reuses
the complete retained placed-photo image during prediction and maps sparse
contact damage into document coordinates for transformed photos and masks.
This removes another unnecessary composition fallback using existing paths.
It is relevant to placed/transformed artwork; no timing benefit is assumed for
an identity-placed large drawing. The replay numbers above precede
this integration, and its Android measurements do not establish iPad timing.
The physical iPad review remains unchanged. The combined source passes five
focused Metal pixel/history tests, both Release builds without warnings and
Web compilation; it has no new device performance qualification.


## Composition attribution and finalization overlap

A further bounded review uses the same four local Mac Metal replays at source
base `4e833915`, with frozen binaries and temporary nested timing probes. The
probes separate source preparation, other scene-job encoding, native command
finalization, queue submission and explicit completion waits. Their intervals
are disjoint, exclude earlier paint/source initialization and later presentation,
and sum to no more than the enclosing composition interval in every frame.
The remaining interval includes scene planning and display-cache encoding.
All instrumentation is removed from the production change; its exact patches,
binary/source hashes and CSVs remain in `artifacts/apple-composition-review-v1/`.

| Share of composition elapsed time | Ordinary circles | Small zigzags | Wide circles | Coalesced circles |
| --- | ---: | ---: | ---: | ---: |
| Source preparation | 7.4% | 11.2% | 8.9% | 13.4% |
| Other scene-job encoding | 1.0% | 0.9% | 0.8% | 0.8% |
| Command finalization | 28.3% | 28.3% | 25.1% | 24.3% |
| Queue submission | 3.0% | 3.5% | 3.2% | 3.0% |
| Explicit completion waits | 59.6% | 55.4% | 61.5% | 58.0% |

These are elapsed CPU intervals, not GPU shader attribution. In particular,
waiting can include brush work encoded before composition, and command
finalization can include driver synchronization. The results do not establish
that source conversion, brush arithmetic or composition shaders are free, or
that all wait time can be removed.

They do expose one avoidable serialization: the previous implementation waited
for the first eight-tile half before finalizing and submitting the second.
The second half now finishes and submits first, then the CPU waits for the first
before preparing a third. This overlaps costly native finalization with the
previous GPU batch without adding persistent state or another rendering path.
Commands still execute in the same queue order. At most two halves (16 tiles)
are live; the final half still accompanies the ordinary frame. The source-upload
byte ceiling, decoded cache, staging callbacks and native command chunk ceiling
are unchanged. Browser waits were already asynchronous/no-op here; this is a
shared native scheduling improvement, not a claim of faster Web execution.

Two paired runs, with reversed run order on the repeat, give these stroke-phase
completion medians. Ranges show the two observations, not confidence intervals.

| Workload | Before, ms | After, ms | Median improvement |
| --- | ---: | ---: | ---: |
| 2 samples/frame, 571 px circles | 4.10–4.14 | 4.09–4.10 | Approximately unchanged |
| 8 samples/frame, 96 px zigzags | 10.64–10.71 | 9.67–9.75 | About 9% |
| 8 samples/frame, 1024 px circles | 23.58–23.73 | 19.36–19.96 | About 15–18% |
| 64 samples/frame, 571 px circles | 39.23–39.67 | 30.47–31.53 | About 20–23% |

Ordinary-input p95 also improves in both pairs, from 15.27/14.80 ms to
13.45/13.00 ms. All four workloads retain byte-identical canonical tile roots
and exact Undo/Redo. Dab counts, composited pixels, source misses, display batch
counts and retained display bytes match across all runs. Equal display bytes do
not establish equal process RSS or native driver allocation peaks. The 64-sample
case remains a stress batch representing approximately 267 ms of input.

A recheck of those same CSVs restricts the comparison to frames with new dabs,
excluding stroke-phase frames that submit no new brush work. This changes the
ordinary median substantially; the earlier table is not the typical cost of a
painting frame. No benchmark is rerun for this calculation.

| Workload | Frames with new dabs per run | Median before, ms | Median after, ms |
| --- | ---: | ---: | ---: |
| Ordinary circles | 182 | 10.32–10.72 | 9.47–9.61 |
| Small zigzags | 292 | 11.67–11.99 | 10.54–10.60 |
| Wide circles | 168 | 24.10–24.39 | 19.78–20.14 |
| Coalesced circles | 48 | 39.23–39.67 | 30.47–31.53 |

Both populations support retaining the correction. Neither transfers Mac
completion times to iPad or measures individual GPU pass cost. In particular,
the first pair's slowest small-zigzag completion is 75.86 ms while composition
accounts for 15.84 ms; aggregate CPU phase shares do not explain every tail.

Keep this small ordering correction. The measurements justify it without a
brush-specific threshold, larger cache, lower precision or architecture rewrite.
They also answer the optimality question: the previous 50 ms result was not a
proven floor, and a general avoidable cost remained. The next useful physical
check is one combined iPad qualification of the resulting build; these offscreen
Mac measurements do not predict its p99 or Pencil-to-screen latency. Further
structural optimization should wait for remaining visible stalls and measured
GPU pass costs. Do not continue changing containers, contact spans or cache sizes
solely because theoretical peak bandwidth suggests a much shorter frame.

The final uninstrumented change passes 31 focused Metal tests covering bounded
and complete display pixels, partial edges, mips, transformed/masked artwork,
preview cancellation, exact source upload/capture, Undo/Redo, rejected cache
writes and device replacement. Web and iOS-target renderer compilation pass.
No iPad app was installed or restarted during the attribution review. The change
is grouped with the following shared color/source transaction milestone, following
the user's check-in policy; its handoff records subsequent device deployment.
