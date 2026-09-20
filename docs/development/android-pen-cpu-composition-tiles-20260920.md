# Wacom: CPU simplification, composition and tile size

Analysis of freshly fetched `origin/main` at `4cad682b`, using the accepted
`new-fast-full` capture from the [matched Wacom comparison](android-pen-main-comparison-20260920.md).
The user permits brush changes when the result remains artistically acceptable.
This note analyzes implementation options; none of these additional changes
has been implemented or measured. No new tablet timing run was needed to derive
the following results. Reproducible arithmetic and the extracted CPU scopes are
in `artifacts/wacom-cpu-tile-analysis-20260920/analyze.py` and `analysis.json`.

The [deeper simplification census](android-pen-tile-simplification-20260920.md)
corrects the binding-cost interpretation below: 3.13 ms covers only the
instrumented device wrapper. The raw material-input helper bypasses it; timing
both in a new matched diagnostic capture accounts for 5.32 ms/update. The newer
note prioritizes unifying existing paths before considering new storage designs.

## What the CPU is doing

The 335 drawing callbacks average 25.04 ms elapsed and **24.12 ms scheduled CPU**.
Only 0.92 ms/callback is off CPU. There are no bounded-wait scopes and no upload
drain increments during drawing. This differs from the original backlogged
build, where most callback time was waiting. Removing explicit waits is no
longer the main opportunity for this steady drawing workload.

| CPU region | Mean scheduled ms/callback |
| --- | ---: |
| Preparation | 1.77 |
| Committed painting command encoding | 2.55 |
| Prediction command encoding | 3.13 |
| Composition command encoding | 4.96 |
| Publication | 10.28 |

Nested across those regions, command finalization costs **6.86 ms**, wgpu queue
submission **4.25 ms**, and binding creation **3.13 ms**. These nested figures
must not be added to the table. In particular, queue submission includes
resource maintenance/reclamation; it is not a measurement of 4.25 ms spent
waiting for GPU execution. The corresponding Vulkan QueueSubmit API scopes
consume only 0.43 ms/callback. Binding creation averages 401 calls/callback;
Vulkan command-buffer allocation API scopes average 123/callback. Allocation
counts do not mean 123 application-level queue submissions.

The action-filtered CPU stacks corroborate command finalization, descriptor
creation, allocator work and completed-command-buffer freeing. The Android
backend intentionally frees completed command buffers because retaining them
previously caused mapping failures. Reduce command demand upstream rather than
assuming that deleting that safeguard is a qualified fix.

## Concrete simplifications

1. **Separate changing metadata from stable bindings.**
   `material_sources.rs::DryRecords` already reuses one metadata buffer, but
   `material_bind_group` still creates a 13-entry material binding per page,
   including a fixed metadata-buffer offset. Dry brushes use only the center
   color source; other neighborhood entries are placeholders. Move varying
   metadata to dynamic offsets or a tile-record table, retain bounded bindings
   with each live page/bank, and use a smaller dry-brush layout. Invalidate on
   buffer growth or resource replacement. `Scene::encode_jobs` also clears its
   source/mask binding caches after every batch; retain a bounded working set
   without pinning evicted textures indefinitely. The measured 3.13 ms direct
   binding cost bounds the saving from eliminating those instrumented calls alone; actual
   savings will be smaller, with possible additional reclamation benefits.
2. **Batch page work as data.** `dry_material.rs` already uses one compute pass
   per batch, but still changes bindings and dispatches separately for every
   page. Put pages in bounded texture-array/atlas banks, retain input/output
   bindings, and dispatch a list of independent dirty pages through the Z
   dimension. Preserve read/write generations and batch ordering. Keep 256 px
   logical damage/history tiles while processing many with one resource set.
   This attacks CPU finalization and driver commands without processing clean
   gaps between dirty tiles. Similar grouping is already effective for mip
   rows in `display_mips.rs`; its CPU encoding now costs only 0.019 ms/callback.
3. **Specialize common composition plans.** The general compositor builds
   clear/draw/combine jobs per tile. For an ordinary photo plus normal paint,
   fuse the remaining layer operations into a direct tile operation with
   resident photo, paint and any preview inputs. The current direct path
   already avoids a final scratch-to-display copy in eligible cases; do not
   count removing that copy again. The new constant-backdrop compute kernel
   does not generalize to arbitrary photo-plus-paint stacks automatically.
4. **Avoid whole operations on provably unchanged paint.** The current opaque
   uniform-pigment shortcut skips contact arithmetic after coverage reaches 1,
   but the compute entry points still read/write whole pages. Conservative
   per-stroke proofs of fully saturated tiles could skip dispatches and avoid
   marking unchanged committed pixels dirty. Preview retirement must still
   restore its changed area. This is especially relevant to repeated opaque
   passes; it is not a general solution for translucent or wet brushes.

Repeated linear searches for page coordinates are another simplification
candidate, but the measured command/driver costs justify prioritizing the
above changes over small input-math improvements.

## What limits composition

There are **both CPU and GPU costs**: 4.96 ms CPU preparation and a 10.67 ms
GPU composition/mip interval. Finalization costs sit partly in publication.
The GPU interval includes ordering, dependencies and possible submission gaps;
it does not isolate shader arithmetic, memory stalls or device idle time.
The near-equality of callback wall and scheduled CPU excludes a large CPU
blocking-wait explanation for this capture, but not GPU-side barriers/stalls.

The update composes about **6.94 MP**, not all 60.22 MP. As an illustrative
fused model, reading two RGBA32Float inputs and writing one costs 48 logical
bytes/pixel. Five separate 2x2 mip reductions add 26.640625 bytes/base pixel.
Together that is **0.518 GB/update**, equivalent to 48.6 logical GB/s over the
observed interval. This is not measured DRAM bandwidth: actual layer draws,
prediction, source conversion, compression, cache hits and attachment behavior
change the traffic. It neither proves bandwidth saturation nor proves spare
bandwidth. The physical device bandwidth and memory-stall counters remain
unmeasured.

The code still uses large retained render attachments with per-tile scissors;
the Vulkan render area is the attachment extent. Repeated LOAD/STORE passes
can amplify traffic, although the existing decode grouping reduced that risk.
[Khronos's render-pass guidance](https://docs.vulkan.org/samples/latest/samples/performance/render_passes/README.html)
explains the attachment bandwidth cost. It does not establish that this driver
loads the entire canvas on every pass.

Separating these GPU causes requires a controlled composition probe: identical
dirty-tile lists and layers, timestamps separating source decode, layer draws
and mips, paired with format/traffic and dispatch-count variants. GPU hardware
memory-stall counters would provide stronger evidence if available. A bulk copy
benchmark alone cannot establish the application's bottleneck.

At the current 17.164% zoom, the visible detail selection uses mip level 2.
Stopping after level 2 instead of level 5 saves only **6.16% of mip traffic**
(25 versus 26.64 bytes/base pixel), or 2.20% of the two-input composition-plus-
mips model. The expensive first reductions still happen. Deferring coarse
levels may help scheduling, but cannot be credited with a large traffic win.

## Larger tiles

Larger tiles reduce per-tile CPU operations but increase boundary overdraw,
allocation/upload granularity and history work. Canvas dimensions alone do not
determine the best size: dirty-footprint size and shape do.

For a single 2048 px circular contact, averaged uniformly over tile-grid
alignment, the expected processed area is `pi*r^2 + 4*r*T + T^2`, with `r=1024`
and tile side `T`, if every touched tile is processed in full:

| Tile side | Expected touched tiles | Processed pixels | One RGBA32F tile |
| --- | ---: | ---: | ---: |
| 256 | 67.27 | 4.41 MP | 1 MiB |
| 512 | 21.57 | 5.65 MP | 4 MiB |
| 1024 | 8.14 | 8.54 MP | 16 MiB |

Thus 512 px yields **68% fewer tiles but 28% more pixel work** in this model.
1024 px nearly doubles pixel work relative to 256 px. These are geometric
results, not benchmark predictions: the actual workload sweeps contacts and
composes the union with old/new prediction damage. Small brushes can suffer
much more from larger full-page writes.

At a fixed 256 MiB decoded-source budget, capacity changes from 256 to 64 to
16 tiles; resident pixel capacity is unchanged. Keeping 256 cache entries
while enlarging tiles would instead quadruple or multiply memory by sixteen.
Current native raster/history tiles and several shader layouts assume 256 px,
so changing PAGE_SIZE alone is not a valid controlled implementation.

Prefer **larger batches of 256 px logical tiles** first. A 512 px physical-page
experiment is still justified afterward, with the same byte budgets and both
large and small brushes, but its net performance is not established yet.

## Artistically acceptable alternatives and performance budget

Permission to change brush behavior enables a simpler coverage-mask or swept
geometry path for ordinary G-Pen, and a cheaper temporary display preview while
full-resolution document work proceeds incrementally. At this zoom, level-2
output has 1/16 the pixel count of full-resolution output. That is an area
ratio, not a 16x frame-rate prediction: source sampling, brush updates and
history still cost work, and downsampling alpha-composited layers independently
is not generally equivalent. Avoid moving all deferred work into a pen-up stall.
Validate pressure taper, joins, translucent overlap, color/alpha, cancellation,
undo/redo and zoom changes visually.

Using Float16 for derived display intermediates would halve their color bytes,
but not necessarily their execution time. It needs gradient/HDR/alpha checks;
it does not require reducing native document/history precision. This is a
separate traffic experiment from command batching, so their effects can be
attributed.

With the current instrumented work, perfect overlap of unchanged mean stages
suggests at most `min(1000/24.12, 1000/20.25) = 41.5` updates/s before other
costs. Eliminating GPU work alone therefore cannot produce 60 FPS. Reaching a
16.67 ms budget requires at least **7.46 ms less CPU work (31%)** and **3.58 ms
less GPU interval (18%)**, plus allowance for presentation and tails. These are
budget requirements, not forecasts. Descriptor reuse alone is insufficient;
batching and simpler composition are the more substantial shared path.
