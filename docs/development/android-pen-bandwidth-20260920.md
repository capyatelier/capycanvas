# CPU overhead and conditional memory-bandwidth model

Follow-up to [the primary-app latency investigation](android-pen-latency-20260920.md).
Source inspected at `cef89441`. No renderer behavior changed for this analysis.

## CPU: elapsed time versus active execution

The nine measured fast-stroke callbacks total 10.712 seconds elapsed but only
2.246 seconds scheduled on a CPU. About 79% of elapsed time is off CPU. The
bounded-wait scopes account for 8.572 seconds elapsed, including 0.472 seconds
running. These nested values overlap; they must not be summed with callbacks.
The owner cannot service queued pen input while executing these waits.

The remaining CPU work is substantial: about 250 ms per fast callback on average,
and 35 ms per slow light-probe callback. Command finalization accounts for 45.43%
of fast-stroke owner samples inclusively. The stroke contains 19,023 Vulkan
allocation API scopes and 10,607 instrumented binding creations. This is a
command-generation/driver workload, not evidence of the CPU shading 60 MP.

Two code mechanisms explain costs:

* `Scene::encode_jobs_inner` merges adjacent draws to the same target, but source
  upload/decode jobs interrupt these runs. `encode_jobs` clears its input-binding
  caches after each batch. This creates repeated pass and descriptor work.
* `vendor/wgpu-hal/src/vulkan/command.rs::reset_all` intentionally frees completed
  Android command buffers. Existing comments document failures with retaining
  them, and pool storage is reclaimed only periodically. Android already uses
  allocation granularity one, avoiding fifteen unused allocations per batch.
  Removing the safeguard is not a demonstrated safe optimization. Reducing
  commands upstream benefits all platforms and reduces the safeguard's cost.

These are concrete performance problems and tradeoffs, not proof of a new
correctness defect or one specific driver bug causing all latency.

## What bandwidth is actually known

The device reports `SM8635` / `DTHA140`. [Wacom specifies Snapdragon 8s Gen 3 and
12 GB RAM](https://www.wacom.com/en-us/products/wacom-movinkpad-pro-14).
[Qualcomm specifies support for LPDDR5x up to 4200 MHz](https://docs.qualcomm.com/bundle/publicresource/87-73942-1_REV_C_Snapdragon_8s_Gen_3_Mobile_Platform_Product_Brief.pdf).
At 8400 MT/s over an **assumed 64-bit aggregate memory interface**, peak bandwidth
would be `8.4e9 × 64 / 8 = 67.2 GB/s` (decimal). The public brief does not verify
this tablet's installed bus width, memory speed or sustained GPU bandwidth.
Treat 67.2 GB/s here as a conditional platform peak, not a device measurement.

The current production-device checks deny access to the GPU/devfreq sysfs
directories. Perfetto's registered sources expose GPU memory allocation, but no
GPU bandwidth counter source. A listed LLCC PMU is not a DRAM-throughput reading.
The saved traces contain no measured DRAM bytes/s, memory-stall fraction or
shader occupancy. They cannot establish that the GPU is memory-bandwidth bound.
Shader contact loops and inter-submission CPU gaps remain competing explanations.

## Bytes and conditional frame rates

Native working pixels and retained composition levels use RGBA32Float: 16 bytes
per pixel. The 9504×6336 image is 60,217,344 pixels, or 0.963 GB per full-size
working image. A complete read plus write is 1.927 GB, taking at least 28.7 ms at
the conditional 67.2 GB/s peak when all those bytes cross DRAM uncompressed.
This is **not** the cost of every drawing update: the renderer uses dirty tiles.

Between the first and last stroke publications in the light-probe traces:

* Slow: `680,853,504 / 117 = 5,819,261` composited pixels/update.
* Fast: `225,837,056 / 7 = 32,262,437` composited pixels/update.

These counters describe composed output area, not actual DRAM traffic, brush
pixel invocations, unique stroke pixels, or a fixed amount of work at all rates.

For scale, consider an **illustrative optimized pass budget** per affected pixel:

| Assumed operation, once per pixel | Logical bytes |
| --- | ---: |
| Paint: read/write RGBA32F color and R32F coverage | 40 |
| Fused composition: read photo and paint, write composite | 48 |
| Five mip reductions: four reads and one write per output texel | 26.641 |
| Total | **114.641** |

The mip calculation is `20 × (1 + 1/4 + 1/16 + 1/64 + 1/256)` bytes per
base-level pixel. It follows the current 2×2 reduction kernel and the five
levels needed to reduce this canvas to at most 512 pixels on a side.

Assume paint and composition cover the same output area, source pixels are
already resident, no repeated paint microbatches, and no extra prediction,
upload, history, attachment spill, presentation or CPU cost. Then:

| Fixed footprint | Logical traffic/update | At conditional 67.2 GB/s | At hypothetical 30 GB/s |
| --- | ---: | ---: | ---: |
| Slow measured composition area, 5.82 MP | 0.667 GB | 101 updates/s | 45 updates/s |
| Backlogged fast composition area, 32.26 MP | 3.699 GB | 18 updates/s | 8.1 updates/s |
| Entire canvas, 60.22 MP | 6.904 GB | 9.7 updates/s | 4.3 updates/s |

30 GB/s is a sensitivity example, **not a measured or asserted sustained rate**.
These are arithmetic results for the stated pass model, not bounds on the actual
implementation: caches/compression/fusion may reduce external traffic, while
extra passes, prediction, source conversion and repeated contacts may increase
it. The fast footprint also shrinks if a faster implementation prevents backlog.

Under the 0.667 GB/update example, 60 Hz needs about 40 GB/s just for these
operations; 120 Hz needs about 80 GB/s. Thus this model does not rule out 60 Hz,
but it does not prove it achievable. A 120 Hz implementation would need less
traffic per update or a smaller affected footprint under the assumed peak.
The previously reported 13.6/s was a current-timing reference, not a silicon limit.

## Repeated work and copy audit

1. The decoded GPU source cache is 64 tiles × 256² pixels = 4.19 MP / 64 MiB.
   Source-cache misses average 39.1 per slow publication interval and 438.1 per
   fast interval. Misses perform staging/upload and source conversion again;
   unchanged photo regions can be revisited after eviction. Counts include all
   source kinds, so they are not an exact count of redundant photo conversions.
   Investigate bounded reuse and traversal before increasing memory ceilings.
2. Direct display composition uses a large retained target with per-tile scissors.
   Source decodes interrupt adjacent output draws, repeatedly opening passes with
   LOAD/STORE. The Vulkan backend sets the render area to the full attachment
   extent. A scissor limits fragments; it does not itself restrict attachment
   load/store semantics. This is a credible traffic-amplification risk, **not a
   measured full-canvas copy on every pass**. Driver tile elimination, direct
   rendering and compression may substantially change actual traffic. The
   [Vulkan attachment guidance](https://docs.vulkan.org/samples/latest/samples/performance/render_passes/README.html)
   explains the load/store bandwidth issue.
3. The dry brush dispatch writes whole 256² pages, preserving unchanged pixels
   through its shader, and traverses the page's ordered contacts. Border pixels
   and repeated contact evaluation can be wasted work relative to the visible
   change. This includes arithmetic as well as memory traffic.
4. Prediction causes independent pixel evaluation and additional composition of
   old/new preview damage. The common single-batch path already reads committed
   color/coverage directly and avoids committed-to-preview and preview ping-pong
   copies. Dry committed paint also already avoids a separate preliminary copy.
   Do not credit those removed copies as a new optimization opportunity.
5. Mips are maintained for edited tiles even at zoom levels that do not sample
   every retained level. They support navigation and other consumers; deferring
   unused levels is a possible scheduling change, not permission to discard
   correctness or drop precision. A mip chain occupies ~33% extra storage but
   reads plus writes cost ~26.6 bytes per changed base pixel in this kernel.

The next discriminating measurements are per-phase bytes/passes (including
load/store and conversion), an on-device sustained throughput control with the
same formats and access pattern, and a matched primary-app replay with bounded
decode/draw grouping. GPU counters, if available through an enabled profiling
path, should distinguish memory stalls from contact arithmetic and idle gaps.
No saturation, redundant-copy speedup, or 60 Hz result is claimed here.

Raw device capability checks: `artifacts/wacom-latency-20260920/bandwidth-*.txt`.
