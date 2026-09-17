# Large-photo drawing: algorithm review — 2026-09-17

The current tile-based design is appropriate, but approximately 50 ms p99 is
not an established hardware limit. The shared follow-up below removes redundant
preparation and overlaps bounded submissions. Retain it; further optimization
should follow measured composition costs rather than a new rendering architecture.
No brush-size, preset-name, document-name or gesture special case is justified.

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

## Algorithm assessment of the reviewed iPad build

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

## Direct next work and stopping rule

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
