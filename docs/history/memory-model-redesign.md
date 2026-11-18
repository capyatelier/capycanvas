# Memory model: known issues and redesign requirements

[Design history](README.md) · [Memory architecture plan](../development/memory-architecture.md) · [Performance targets](../PERFORMANCE_TARGETS.md)

This record describes the renderer memory model at 9a2b63c96 (2026-11-17), the
issues that stopped its release, and why another redesign is needed. The
model began at 5e196eb79 (one residency ledger, display composed in one pass)
and was extended by the scheduler steps that followed. Check the code before
relying on any claim here.

## Known issues

### Photo import on low-memory tablets

- **Symptom.** On the TCL TAB 11 Gen 2, importing a 4248×2832 photo as a layer
  into a 32-layer 3840×2160 drawing and applying it blanks the drawing for
  about 1 minute to more than 4 minutes. The photo arrives in blocks, and menus
  and panels respond late. The Wacom MovinkPad Pro 14 handles the same flow.
- **Cause.** The display working set does not fit the low-tier budget, and
  refused work does not wait.
  - Every refused display admission used to search the whole heap and page
    store for room, once per unit; 684a3235d limits that to one search per
    refused class and size per frame.
  - `Inputs::accept_query` turns a residency refusal into `NotReady` and drops
    its wake. Held decode results then stay Runnable and frames repeat with no
    progress. A fix that blocks held results on the release (branch
    `import-spin`, not merged) moved the same pattern to the decode, display
    and Navigator terms.
- **Root fix.** Carry a residency refusal through every path as a typed wait
  with its release as the wake, and size the low-tier working set to what the
  view needs.

### A stroke that never lands

- **Symptom.** With low-tier memory facts and a warm shader cache, 5 of 10
  fixture runs on 6e0e11bf5 left a stroke unapplied while the host framed at
  about 478 frames per second.
- **Cause.** Decoded results held for the stroke's display filled the decode
  working set. The restore's own decode was refused, and
  `PendingTile::prepare` ignored the refusal. The restore drain took only the
  results its reads matched (6eaa5facc), so nothing freed room, and
  `restore_preparation` reported Runnable every frame.
- **Status.** 9a2b63c96 reverts the paced drains (6eaa5facc and 6e0e11bf5);
  0 of 20 runs stuck after it. `PendingTile::prepare` still ignores a refused
  admission.


### Pen latency over photo layers below 100%

- **Symptom.** On the Wacom MovinkPad Pro 14, a Pencil stroke at
  2048 px over a 9504×6336 photo object at Fit reaches the next GPU frame in
  70–84 ms (p50) and 135–154 ms (p95). The build before 2a24c512d measured
  59.7 and 107.8 ms. Strokes show correctly.
- **Cause.** 02a184451 made the cells under a stroke Authored and let their
  canonical object windows advance during the stroke. At Fit those windows
  sample level-0 photo tiles, which the open had evicted, so each new stroke
  position decodes 16–48 tiles in its first frame. The earlier design showed the
  photo's resident mip there instead. The defect 02a184451 set out to fix was
  that the mip was not reliably resident while shown: it was evicted and rebuilt
  during the stroke.
- **Why the obvious fixes fail.**
  - Building the window from the mip on every frame (63061b3fc, not merged) cut
    command passes to the old level but was slower on the device. The values were
    not cached, so every composed cell resampled the whole level-1 mip every
    frame.
  - Keeping level-0 tiles resident costs about 238 MiB for this photo, or 64 MiB
    near the last dab for about half the coverage.
  - The per-frame demand stamp cannot hold the mip: it lapses at the next epoch,
    and pressure between frames evicts what is on screen.
- **Root fix.**
  - The presented cell's retained page holds a persistent guard on the mip it
    shows, with pinned-byte accounting.
  - A missing mip is rebuilt nearest the dab first, at the cell's class.
  - The Authored promotions from 02a184451 and 6eaa5facc are reverted.
  - The mip becomes the final cached value of a level ≥ 1 cell.

### Windows: frames continue after a pen long press and Escape

- **Symptom.** The `tool-bubbles` light-pen fixture never goes idle after a pen
  long press on the colour picker bubble and Escape. The mouse variants and the
  dark-pen and HDR pen variants pass.
- **Fixed part.** a294b4866 stops held input from framing every tick.
- **Open part.**
  - With a294b4866, the deferred queue is empty, no stroke is active, and the
    UI demands no frame before each frame.
  - After each frame, a renderer `frame_demand` term reports Work, so every
    frame requests the next one.
  - The term is not yet named. The leading suspects are the terms still on the
    old readiness rule: colour sample re-encode, region requests, effect
    validation and preparing filter previews.
- **Related gap.** The shared session never ends a stroke whose Up is lost:
  `has_active_stroke` stays true and every queued action waits on it.

### Windows frame cost and rate on older hardware

- **Symptom.** On a Lenovo ThinkPad P43s (Intel UHD 620), a 9 px Pencil at 72%
  measured CPU 1.8 ms and GPU 1.3 ms per frame on main, against 1.7 and 0.9 ms
  at 622a1fef6. The submitted rate fell from about 100 to 90 frames per second.
  At 72% the view level is 0, so the zoomed-out work below does not apply.
- **GPU cause.**
  - A stroke frame recomposes every whole 256 px display cell the dab touches,
    through the below and above halves of the working layer, at Rgba32Float
    (`compose_contact`).
  - It also issues more passes per frame than the old dirty-rectangle composite,
    and GPU time on desktop adapters follows the dispatch count.
- **Pacing cause.**
  - The Windows loop keeps one frame in flight and serializes CPU frame building
    with GPU execution.
  - Since 5e196eb79, the next frame starts on a completion wake rather than a
    blocking poll. The idle gap between frames rose from about 1.0 ms to 5.6 ms
    on the same laptop.

### Windows memory at rest

- **Symptom.** On the same P43s, Task Manager shows 583 MB on main against
  196 MB at 622a1fef6.
- **Why.** On an integrated GPU, Windows counts GPU textures as process memory.
- **What the model keeps.** It retains view-sized pages:
  - an Rgba16Float display window;
  - Rgba32Float exact level-0 pages;
  - the working layer's below and above halves;
  - warm background drawings, capped at 512 MiB on desktop.

  It keeps evictable caches up to the budget grant. The split between pinned and
  evictable bytes for this case is unmeasured.

### Unverified behaviour

- **Out-of-memory notice.** Whether "Not enough memory to paint here. Close
  another drawing to free memory." reaches the person when nothing can be freed.
  A squeezed-lane probe never saw the notice set.
- **Idle wakes on a settled canvas.** Memory sampling rebalances the budget and
  wakes the display owner. These are wakes, not a spin.
- **Old readiness terms.** Some terms still use the old readiness rule: colour
  sample re-encode, region requests, effect validation and preparing filter
  previews.
- **Frames after pen-up.** Three gap frames follow each pen-up, which breaks the
  one-tick input rule.

### Test health on main

These suites fail on main at the release:
- 12 transform tests, such as
  `keyed_transforms_present_rotated_views_and_navigators`;
- 10–12 `scene::objects` tests;
- 8 `tests::cold_paint` tests;
- `canvas_transition_tests::a_settled_canvas_demands_no_frame…`;
- `scoped_raw_constant_fill`, `region_cache_restore`,
  `cold_composite_region_classification` and
  `fill_preview_becomes_ready_after_deferred_object_composition`;
- `an_icc_photo_stack_settles_under_a_grant_smaller_than_its_windows`;
- `cold_camera_storm` seeds, intermittently: 5 of 23 at four threads.
- `paper::tests::a_bake_step_waits_until_the_previous_submission_completes`,
  which fails alone: its completion flag is set by the poll owner's callback,
  not by the test's own `device.poll`, since 261ec398d changed `device_poll`.

They have not been sorted into stale tests and real bugs. While they stay red
they hide regressions. A canvas freeze after crop or rotate on masked layers
(fixed in 6e0e11bf5) surfaced only after unrelated fixture repairs turned
`canvas_geometry` green.

### Not related to the memory model

- The Linux client drops canvas contacts made during a tab switch.
  `Workspace::interact` refuses canvas Down and Move while
  `documents.changing`.
- The Windows render loop stops on any frame error.
- `required_sources` in liquify is unaudited.
- The watercolor stencil size is defined in three places.

## Why the design needs to be redone

1. **Readiness and wakes are rules per term, not a property of one graph.**
   - Each producer reports its own readiness and wake. Each new filter or gate
     needs a manual audit of every waiter on what it filters.
   - The release work fixed several spins and deadlocks of this one class:
     - completion edges with no delivered result (b41cd4881);
     - host wakes that framed while anything was pending (17530c4e0);
     - held input that framed every tick (a294b4866);
     - one scheduler serving two scenes (67984e38f);
     - a restore whose decodes a new drain filter never took (6e0e11bf5).
2. **Residency cannot say "held while shown".**
   - Per-frame demand stamps protect only within one epoch.
   - What the screen shows can be evicted between frames, and the fixes for that
     raised the class or exactness of the work, which moved the cost into the
     stroke frame.
3. **Exactness is stricter than the product rule.**
   - The product allows approximate display below 100%. Exact results are needed
     only at 100% and above and for exact queries such as export and sampling.
   - Several paths still demand level-0 exact samples while the pen is down.
   - `exact_at_view_level` turns off the stroke contact for every document with
     an image object or vector layer, so photo documents never take the fast
     contact path.
4. **Work is proportional to cells, not damage.** A small brush recomposes whole
   256 px cells through the halves at full precision on every frame.
5. **Precision and retention are fixed, not derived.**
   - Exact pages and halves are Rgba32Float wherever they are read.
   - Caches fill to their grant, and warm tabs use a fixed cap rather than
     memory pressure.
6. **The frame loop does not overlap CPU and GPU.** With one frame in flight, the
   frame interval is the sum of CPU time, GPU time and the wake delay, not their
   maximum.
7. **The cost model counts the wrong unit.**
   - Windowless probes count command passes, which treat a copy, a resample and a
     tile contribution as equal and ignore uploads.
   - On 2026-11-17 a change that matched the old pass count was 9–27% slower on
     the Pro 14.

## Requirements for the next design

- **One dependency graph from the presented frame to its inputs.**
  - Residency is the set of inputs the presented frame and in-flight work read,
    held by guards for as long as they are shown.
  - Eviction can never take a presented input.
  - Every wait the graph derives has a producer and a wake by construction, and
    a test enforces it.
- **The product exactness rule, encoded once.**
  - Below 100%, the display reduces then composes, using resident mips.
  - Exact level-0 work runs only at 100% and above and for exact queries.
  - Photo and vector documents use the same stroke contact path as paint.
- **Damage-proportional stroke frames.** The fold for a dab touches only the
  damaged rectangle and the fewest passes.
- **Precision from a range proof.** Rgba16Float for display data and kept pages
  of 8-bit documents; Rgba32Float only where the range or the blend requires it.
- **Retention sized by pressure.** Caches, halves and warm drawings shrink with
  available memory and are released when idle. The Diagnostics memory bars show
  pinned and evictable bytes separately.
- **Pipelined frames.** The CPU builds frame N+1 while the GPU runs frame N, on
  every host.
- **A calibrated cost unit.**
  - Probes record pixels written, texels read, bytes uploaded, decode taps and
    dispatches.
  - These are weighted once against GPU timestamps on each tier device, and
    device measurements decide.
- **Green suites as the gate.** Every failing test on main is fixed or removed
  before redesign work starts, so that a regression shows up as a new failure.

## Parked work to reuse or retire

| Work | Where | State |
| --- | --- | --- |
| Zoomed-out display (reduce-then-compose with kept per-layer pages) | branches `issue15-zport-t3`, `issue15-zport` | Edits below 100% 1.3–15× faster; fails 15 of 23 storm seeds against 5 on main (display over its grant at gigapixel L4, fewer display pages kept through storms) |
| Group admission (scheduler step 4) | branch `issue15-step4c` | Parked before proof review |
| Scheduler step 7 and the remaining step 2 pieces | not started | — |
| Mip-built object windows (option B) | branch `fix-object-window-mips` | Slower on the Pro 14; values were not cached |
