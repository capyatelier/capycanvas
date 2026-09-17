# Large-photo drawing: algorithm review — 2026-09-17

The current tile-based design is appropriate, but approximately 50 ms p99 is
not an established hardware limit. Retain the shared fixes and optimize the
remaining general preparation/submission overhead before considering a new
rendering architecture. No brush-size, preset-name, document-name or gesture
special case is justified.

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
