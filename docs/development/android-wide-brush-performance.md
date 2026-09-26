# Wacom native Android wide-brush diagnosis and optimization — 2026-09-19

The reported 500+ ms CPU and GPU p99 is reproducible. On the connected Wacom
DTHA140, one 15-second stroke ended with the actual Diagnostics rows showing
**CPU 162.96 / 350.29 / 754.08 ms** and
**GPU 145.73 / 274.00 / 558.70 ms** (median / p95 / p99).
The next stroke showed CPU p99 **478.96 ms**, GPU p99 **410.58 ms**.
This is a rendering, driver-allocation and synchronization problem; the evidence
does not support attributing it solely to exhausted RAM or a saturated GPU.

The selected refactor preserves brush math and raises the matched workload
from **5.1 to 16.0 drawing updates/s** over twenty 15-second strokes, a
**3.15× throughput improvement**. GPU medians are approximately **49 ms**.
The final build trades some peak throughput for periodic driver-pool cleanup:
late process mapping counts are 27–31k, versus 53.5k after twenty strokes without
cleanup. Full-stroke callback p99 remains **195–523 ms**, including cold stalls;
low rolling UI percentiles alone would overstate the improvement. Brush math
and pixel precision are unchanged. Work stops here before brush-algorithm changes;
the measured-work GPU estimate is roughly 18–20 updates/s, not a proven absolute
limit. The faster 2 Hz stress case still loses the GPU device on both baseline
and final builds. Final qualification and its limitations are recorded at the end.

## Method and limits

- Base revision: `e1251cd484b1c079e0b402e951ab9579f180e8c9`.
- Physical tablet: Wacom DTHA140, Android 15, Adreno Vulkan, 2880 × 1800.
- Native Android app, release Rust renderer, debug Kotlin host; isolated package
  `art.capycanvas.penperf`. The regular app was stopped for the controlled runs.
- Exact source: `sony_a7r_v_29 (1).jpg`, 9504 × 6336, SHA-256
  `3aac9c9b8b34c38a5e0121f16ad1ec806e92a19e15ee5e1128f36d987e888054`.
- Opened as an sRGB/U8 photo, G-Pen preset 1, opaque ink, 2048 px diameter,
  pressure 1.0. Fit zoom approximately 18%; the original user view was 22%.
- OS-injected stylus MotionEvents, approximately 200 samples/second, continuous
  ellipses inside the image. These exercise the real input and Choreographer
  path, but do not measure physical nib-to-photon latency or hardware prediction.
- Reproduction: one ellipse/second, feedback enabled, 16 ms fallback prediction.
  Saved platform-prediction toggle is enabled; the injected device has no native
  prediction capability. Earlier speed controls used 8 ms fallback prediction.
- The original comparison used two 15-second strokes; subsequent tuning used
  four, and final sustained qualification used twenty. Diagnostics is a rolling
  last-120-update window, not a distribution reset at each stroke. First-stroke
  history includes setup; the second can include first-stroke samples. Reported
  p99 values below come from the actual UI rows, which round percentile indices.
  The older instrumentation `cpu_ms`/`gpu_ms` helper floors indices and can differ.

## Measurements

| Brush / ellipse rate / prediction | Stroke | CPU UI p99 | GPU UI p99 | Result |
| --- | --- | ---: | ---: | --- |
| 96 px / 1 per second / 16 ms | 1 / 2 | **13.95 / 13.37 ms** | **11.08 / 11.35 ms** | Passed, matched 15-second control |
| 96 px / 2 per second / 8 ms | 1 / 2 | 29.30 / 15.56 ms | 14.98 / 14.94 ms | Passed, 10 seconds each |
| 2048 px / 0.25 per second / 8 ms | 1 / 2 | 116.93 / 131.49 ms | 103.04 / 110.56 ms | Passed; CPU sampling overlapped second stroke |
| 2048 px / 0.5 per second / 8 ms | 1 / 2 | 382.83 / 141.19 ms | 270.28 / 114.12 ms | Passed |
| 2048 px / 1 per second / 16 ms | 1 / 2 | **754.08 / 478.96 ms** | **558.70 / 410.58 ms** | Passed, 75 / 78 drawing callbacks |
| 2048 px / 2 per second / 8 ms | 1 | Not meaningful | Unavailable after loss | Three callbacks: 372, 1311, 3250 ms, then GPU device lost |

Changing size/rate clearly changes cost. These are short tail measurements,
not a claim that every stroke remains above 500 ms. The 2 Hz stress failure is
separate from the successful 1 Hz reproduction; its device-loss cause has not
been isolated to a driver watchdog, shader, or allocation failure.

## Attribution

1. **Command construction and driver allocation consume substantial CPU.**
   A 15-second `simpleperf` sample of `capy-canvas` during the slower wide-brush
   case recorded 4333 samples, zero lost, approximately 8.68 CPU seconds.
   Composition accounted for 78.81% of sampled active CPU; command finalization
   accounted for 44.01%. `vkFreeCommandBuffers` accounted for 25.69% and
   `AllocateCommandBuffers` for 9.12%. These are inclusive stack percentages;
   parent, allocator and child percentages must not be summed together.
   Allocator, page clearing and `madvise` stacks establish allocation churn,
   rather than merely inferring it from a large resident-memory counter.
2. **Composition includes waits for earlier GPU work.**
   At baseline, [the composition loop](../../crates/layer-render-wgpu/src/scene.rs)
   submitted batches of eight output tiles, overlapped two batches, then waited
   before recycling bounded resources. Its wall time includes command
   materialization and earlier paint/preview execution, not just composition
   shaders. At 1 Hz, composition wall-time medians were 153 and 161 ms;
   preparation p99 reached 341 ms on the first stroke. The warm ordinary
   preparation median was about 2 ms, indicating intermittent costs as well.
3. **Input backlog increases work in later frames.**
   At 1 Hz the median delivered samples per render was 33–35, peaking at 191.
   In the failed 2 Hz run it reached 260. The
   [material shader](../../crates/layer-render-wgpu/src/material_brush.wgsl)
   evaluates each tile's ordered contact range. A full-pressure 2048 px disc
   covers approximately 3.29 million pixels before motion and tile padding.
   Longer frames accumulate more contact/motion work. This is a plausible
   amplification mechanism consistent with the traces, not proof of the exact
   GPU bottleneck or of a need to discard input samples.
4. **Neither p99 is a pure hardware-utilization measurement.**
   New debug-only thread CPU timing shows active CPU execution was 51.8% and
   53.8% of callback wall time at 1 Hz. Per-app `dumpsys gpu` activity deltas
   showed about 9.15 and 9.26 active GPU seconds over roughly 16-second
   measurement intervals: approximately 57% active, with counter-boundary
   imprecision. This is not shader occupancy or memory-bandwidth utilization.
   GPU timestamps span scheduling gaps between submissions. CPU timings include
   synchronous waits. Their similar values do not mean two independent serial
   500 ms workloads, nor establish continuous CPU/GPU saturation.

## Memory

Memory management matters in two different ways:

- **Capacity pressure was real in the original device state.** Available RAM
  was initially about 330 MB. A first exploratory run with both regular and
  isolated renderers resident triggered Android's low-memory killer, including
  termination of the background regular app. That run is confounded and is not
  the basis of the controlled performance conclusion. Existing recoveries were
  backed up and their `.capy` hashes verified unchanged before stopping the
  regular renderer for the remaining tests. The hashes were verified unchanged
  again after that initial investigation, and the regular app was relaunched
  at its recovery prompt; shell-injected clicks did not activate Recover. The existing
  recoveries and local backup archives are retained.
- **Removing that pressure did not remove the slowdown.** The successful
  754/559 ms reproduction started with 5.69 GiB available and ended with
  3.58 GiB. The app incurred **zero major page faults** in both strokes.
  System-wide swap-out still increased by about 76 MiB and 25 MiB respectively,
  so these runs are not described as having absolutely no reclamation. The
  separate 2 Hz device-loss run started with about 5.7 GiB available, had no
  swap-outs and only three process major faults.
- **Allocation churn is measured.** The 1 Hz runs incurred approximately
  1.30–1.36 million minor faults per stroke, alongside the allocator-heavy CPU
  profile. Minor faults are not evidence of disk swapping. Tracked canvas
  storage reached approximately 2.4 GiB and excludes imported assets and driver
  overhead. These short tests do not prove a memory leak or a long-term plateau.

The Android-specific
[command-buffer workaround](../../vendor/wgpu-hal/src/vulkan/command.rs)
intentionally frees every completed buffer: previous buffer retention and
periodic reclamation reached approximately 64,000 process mappings and failed
despite available RAM. Commit `668ff0a0` already removed repeated *pool-storage*
release, while retaining completed-buffer reclamation. Simply removing that
reclamation is not a justified fix for the measured remaining CPU overhead.
See [the earlier device investigation](image-placement-web-android-progress.md).

## Recent commits and platform reach

These optimizations are present in the tested revision and in shared code:

| Commit | Change | Android / Web / GTK reach |
| --- | --- | --- |
| `1f96d5a6`, `e350a585`, `4c4e7fa5` (Sept 17) | Remove redundant clears, dry-brush reads and copies | Shared renderer |
| `0a7b7d7e` (Sept 17) | Overlap bounded display submissions | Shared; native waits differ from browser queue behavior |
| `dcfcb5de`, `ab6d9370` (Sept 17–18) | Tighter contact bounds, sparse prediction retirement, batched dry compute | Shared Float32 dry-material path |
| `6afe7589` (Sept 18) | Alternate composition traversal to reuse decoded sources | Shared |
| `3e614482` (Sept 18) | Simplify source access and qualify all contact presets | Shared |
| `668ff0a0` (Sept 17) | Keep Vulkan pool storage while reclaiming completed buffers | Android-specific driver mitigation plus shared drawing changes |

The remaining Wacom cost does **not** mean these Apple-led changes were omitted
from Android. Backend-specific driver behavior and variable real-time input
batches matter. Earlier fixed-sample iPad benchmarks and small-brush Android
qualification do not establish this large-brush Android performance.

The allocation refactor below reduces composition/copy/source-decoding commands
while preserving explicit resource bounds.
Separately investigate cold tile/preview allocation and GPU device loss. GPU
pass counters or a controlled replay are needed to distinguish shader arithmetic
from GPU memory bandwidth; current evidence does not identify that split.

## Reproduction and artifacts

Build and install the isolated release package described in
[the brush workload benchmark](android.md#brush-workload-benchmark), then run
the wide G-Pen workload:

```sh
python3 tools/performance/android-brush-benchmark.py artifacts/wacom-wide-pen \
  --serial 5ll21u1002931 --presets 1 --size 2048 --duration 15000 --repeats 2
```

Do not run over an artist's active document. The benchmark opens and paints
its own photo. Instrumentation does not add a frame pump during strokes.
The host change is debug telemetry for actual render-thread CPU time. The
measurements above were taken before the renderer refactor described below.

Local, ignored evidence is under `artifacts/wacom-wide-pen/`: `wide-fast.json`
contains the successful reproduction, `wide-single.json` the isolated failure,
the other JSON files the speed/size controls, and `wide-slow.perf.data` plus
`wide-slow-symbolized.txt` the CPU profile. Raw timelines and resource snapshots
are retained. `*.summary.json` separates whole-stroke host timings from the
rolling diagnostics; use `diagnostics_rows` for exact UI percentiles.


## Allocation and copy refactor

The shared renderer now:

- Updates an admitted complete display pyramid directly, with permanent mip
  bindings, immutable tile-coordinate records and one compute pass per bounded
  tile batch. Previously each tile reduced through scratch images and copied
  each result back into the retained pyramid. Weighted area reduction,
  including odd document edges, is unchanged.
- Draws eligible source-over compositions into the retained full-resolution
  texture. This removes the scratch-to-display copy and allows adjacent draws
  to share a render pass. Effects, intermediate dependencies and portable
  blending keep their existing composition path.
- Serializes dry-material tile metadata directly into pooled mapped storage and
  uploads to a GPU buffer that grows geometrically. This avoids an intermediate
  CPU byte vector, its copy and a GPU allocation per tile. Ordered uploads are
  essential: later batches reuse offsets only after earlier draws consume them.
- Uses the existing staging belt for native and ICC source-tile uploads. Cache
  misses no longer create and discard a separate upload buffer. New source bytes
  still need copying into mapped staging storage; decoded cache hits avoid that
  upload entirely. The existing in-flight source-byte charge and submission
  limits remain in force.
- Uses stack arrays for fixed-size material binding descriptors, reuses decoded
  texture views, and retains composition-table capacity across tile batches.
  Brush job lists also retain capacity. These collections release resource
  handles after encoding so they do not pin evicted paint or source textures.
- Batches up to 128 independent direct-composition tiles per submission, with
  at most two batches in flight before a completion wait. Intermediate/effect
  composition falls back to the original eight-tile batch bound. The separate
  16 MiB in-flight source-upload charge and 512-pass encoder bound are unchanged.
- On Android Vulkan, reuses a bounded cache of framebuffers at completed-encoder
  boundaries. The cache holds at most 128 entries per encoder after reset and
  expires unused entries on the next completed cycle. Permanent view identities
  prevent matching retired attachments. The vendor patch and lifetime rationale
  are recorded in `vendor/README.md`. Every completed command buffer is still
  freed on every reset. Once per 256 nonempty completed resets, retained driver
  pool storage and cached framebuffers are also released.

Immutable mip metadata adds about 1.16 MiB for this photo and is included in
complete cache admission and tracked storage. Pixel texture formats, editing
precision, color conversions and the complete-display allowance are unchanged. Upload
storage is reused after GPU completion. The Android completed-command-buffer
reclamation safeguard is unchanged; a reset-before-free experiment did not
improve the benchmark and was discarded.

The renderer changes apply to Android, Apple, GTK and Web; the framebuffer
cache and periodic driver-pool cleanup apply only to Android Vulkan. The complete-pyramid optimization applies
when the device admits the complete cache. This investigation measures Android performance; it does not claim a
measured speedup on the other hosts.

Intermediate measurements, same 2048 px / 1 Hz / 16 ms workload:

| Build | CPU median, strokes 1 / 2 | GPU median, strokes 1 / 2 | Active owner CPU median, strokes 1 / 2 |
| --- | ---: | ---: | ---: |
| Baseline | 162.96 / 168.29 ms | 145.73 / 149.49 ms | 85.7 / 92.0 ms |
| Batched in-place mips | 112.66 / 118.88 ms | 97.70 / 101.69 ms | 53.38 / 55.30 ms |
| Plus metadata reuse | 114.51 / 119.93 ms | 99.36 / 103.82 ms | 51.75 / 56.33 ms |
| Plus direct composition | 121.21 / 122.46 ms | 101.04 / 100.69 ms | 55.05 / 56.63 ms |

The metadata change eliminates repeated allocations by construction; these
short runs do not show a separate timing improvement from that step. Direct
composition reduced minor faults to 341–353 thousand per stroke, versus
1.30–1.36 million in the baseline, but its median was slightly higher than the
mip-only runs. Across four strokes its CPU UI p99 was 776.94 / 554.85 / 496.87 /
560.40 ms and GPU p99 was 468.43 / 375.23 / 345.05 / 283.62 ms. It did not eliminate
tail stalls. The first two rolling windows are comparable to the two-stroke
baseline; subsequent windows show sustained behavior without being independent
statistical trials.

Current-turn artifacts are in `artifacts/wacom-allocation-fix/`. The intermediate
runs are `batched-mips.json`, `reused-records.json` and `direct-final.json`; the
last name predates the additional source-upload pooling change. Baseline and
candidate APKs and native test executables are retained alongside their logs.


### Correctness and build checks

- Android ARM64 debug host with release Rust: builds successfully. Shared Web
  renderer: `cargo check --target wasm32-unknown-unknown` passes.
- New exact comparison uses odd document edges, transparency, multiple partial
  batches, opacity edits and every retained mip. The previous scratch path is
  the independent oracle. It passes bit for bit on both the Wacom Vulkan GPU
  and the software renderer used only for numerical tests.
- Final native G-Pen/photo test passes on Wacom for U8/U16 and both native
  publication modes: untouched source pixels and opaque photo alpha survive.
- Source-cache suite with pooled uploads passes on the Wacom: 5 passed, 1
  benchmark intentionally ignored. This includes U8/U16 code preservation,
  built-in and ICC profiles, native cache reuse, eviction and discarded
  submissions. The same suite passes on the numerical software backend.
- Display suite on Wacom, after direct composition and metadata pooling:
  19 passed, 3 failed. Passing checks include sparse G-Pen prediction retirement,
  native undo/redo and device replacement, rejected views, abandoned writes,
  and repeated compositions beyond source-cache capacity.
- All three hardware failures reproduce identically on pristine baseline
  `e1251cd4`: filtered/masked display comparison at pixel 53120/channel 0
  (`0.9999794` versus `1`), and retained detail at pixel 30333/channel 0
  (`0.51995945` versus `0.51996446`), and rotated detail at pixel 3165/channel 0
  (`0.3803952` versus `0.37800542`). Assertions were not loosened.
- The material specialization comparison fails identically on baseline and
  candidate Wacom builds: legacy renderer, operation 0 / variant 0 / prediction
  phase 1, maximum channel error 93. The numerical software run passes all 240
  full-image comparisons across legacy and native renderers. This remains an
  existing hardware correctness issue; the allocation refactor does not claim
  to fix it.
- Two numerical-software display failures also reproduce identically on the
  baseline: retained detail at pixel 16319/channel 0, and rotated detail at pixel
  2205/channel 0. These are kept visible rather than reported as a clean suite.

These are targeted renderer and native Android checks, not full application or
Apple/GTK/browser runtime qualification. GPU times were collected only from the
physical tablet, without overlapping correctness-test processes.


The intermediate pooled-upload APK is `artifacts/wacom-allocation-fix/final.apk`,
SHA-256 `7a9ddb8943633c74284d15d119cccf2fd052d9cdb39b44bc51e4a55b9a283e3b`.
It predates framebuffer reuse and submission/dispatch tuning. The regular app was not replaced or cleared. Its two `.capy` recovery files were
backed up again and verified byte-for-byte unchanged during this refactor.


### Submission and driver overhead

A subsequent 1 Hz CPU profile recorded 3281 samples, zero lost, and about
6.58 active render-thread CPU seconds in 15.03 seconds. Framebuffer creation
and destruction accounted for 14.02% and 7.53% of samples, respectively;
command-buffer freeing/allocation for 7.47% / 3.90%, and material source binding
creation for 9.51%. Percentages include child calls. This profile identified
remaining work; its different ellipse rate prevents treating its percentages
as a direct before/after comparison with the original 0.25 Hz profile.

The framebuffer cache reduced four-stroke minor faults to about 254–257k per
stroke, versus roughly 338–353k at the preceding stage. It did not independently
produce a clear throughput improvement. Larger direct-composition batches did:

| Direct batch size | Drawing callbacks in 4 × 15 seconds | Average callbacks/s |
| --- | ---: | ---: |
| 8, framebuffer cache | 413 | 6.88 |
| 16 | 462 | 7.70 |
| 32 | 513 | 8.55 |
| 64 | 702 | 11.70 |
| 128 | 1020 | 17.00 |

The 16/32/64 stages also progressively introduced reusable brush job lists;
the 128 stage serialized metadata directly into staging. These small structural
changes are not assigned isolated timing gains. Source-upload limits and brush
math stayed unchanged. Runs are sequential device experiments, not randomized
thermal/frequency-controlled trials.

At 128 tiles, CPU/GPU UI medians were approximately 20 / 49 ms. Full-stroke
callback p99 remained 198–290 ms, despite UI CPU p99 of 31–33 ms. More than 120
updates now fit in each stroke, so the rolling UI window excludes early cold
stalls. The low UI p99 must not be mistaken for elimination of cold allocation
or preparation stalls. Raw full-stroke timelines are retained alongside the
rolling diagnostics.

The earlier ten-stroke pooled-upload run (`final-sustained.json`) completed
1049 drawing callbacks in 150 seconds without device loss. Its last five
mapping counts were 33–34k. This is historical qualification of that stage,
not a substitute for sustained qualification of the later batching changes.

A rejected experiment replaced individual tile dispatches with one dispatch per
mip level using the third workgroup dimension and a reusable coordinate buffer.
It passed an exact pixel comparison, but measured 985 callbacks in 60 seconds
(16.42/s), versus 1020 (17.00/s) for the preceding version. That experiment was
reverted: fewer GPU commands did not produce a measured throughput gain. Its
artifacts are `dispatch.json` and `device-dispatch-exact.log`; it is not in the
final implementation.


### Interpreting the performance limit

A GPU-active-time-per-update estimate is conditional on that update's work;
it is not an absolute limit of the brush algorithm. The earlier roughly
100 ms estimate came from larger input backlogs and many small submissions.
Reducing those stalls also reduced contacts accumulated per update. The same
brush math subsequently reached 17 updates/s with roughly 49 ms GPU intervals.
It would therefore be incorrect to call the earlier estimate a universal
10 updates/s ceiling.

The remaining material bind-group creation and driver command allocation are
not eliminated. Cross-frame binding caches would have to follow tile and
coverage lifetimes without retaining evicted textures. Minor fault counts are
an allocation-pressure indicator, not counts of heap allocation calls. Cold
page preparation, prediction growth, publication/history work and occasional
long callbacks also remain visible. No claim of an entirely allocation-free
drawing path or a mathematically proven optimal implementation is made.

The 128-tile tuning run's per-UID GPU-active counters gave about 54–56 ms per
drawing update, including surrounding presentation work. Its resource windows
were roughly 16 seconds around each 15-second stroke, with 85–90% GPU activity.
For that fixed work, the approximate GPU ceiling is 18–19 updates/s; the
renderer-only 49 ms timestamp suggests about 20/s before presentation. These
are measured-work bounds with coarse counter boundaries, not a hardware FLOP
model or proof that the brush shader itself cannot be optimized further.


### Pre-reclamation candidate: ten-stroke qualification

`optimized-sustained.json` completed all ten 15-second strokes without device
loss: **2582 drawing callbacks / 150 seconds = 17.21/s**, versus the original
153 / 30 = 5.10/s (**3.38× throughput**). This predates final periodic driver-pool cleanup. It includes fallback
submission bounds and excludes the rejected dispatch change.

| Stroke | CPU UI median / p95 / p99, ms | GPU UI median / p95 / p99, ms | Full-stroke callback p99, ms | Process mappings |
| --- | --- | --- | ---: | ---: |
| 1 | 18.63 / 24.73 / 27.43 | 49.68 / 59.08 / 60.64 | 316.55 | 28,697 |
| 2 | 20.56 / 27.65 / 34.84 | 49.90 / 58.30 / 66.98 | 251.24 | 35,288 |
| 3 | 20.01 / 26.61 / 28.53 | 49.08 / 58.08 / 59.05 | 184.80 | 37,439 |
| 4 | 19.03 / 26.24 / 31.49 | 49.01 / 56.87 / 58.81 | 268.63 | 38,840 |
| 5 | 19.71 / 27.01 / 32.79 | 49.07 / 58.44 / 59.36 | 199.87 | 38,951 |
| 6 | 19.33 / 27.97 / 31.97 | 49.42 / 58.67 / 60.66 | 263.04 | 40,568 |
| 7 | 19.58 / 29.03 / 33.27 | 49.58 / 58.62 / 65.06 | 211.48 | 42,801 |
| 8 | 20.20 / 28.06 / 33.96 | 49.35 / 58.32 / 59.11 | 190.34 | 42,878 |
| 9 | 19.55 / 27.83 / 29.79 | 49.27 / 58.48 / 59.66 | 290.56 | 44,193 |
| 10 | 20.87 / 27.12 / 34.05 | 49.13 / 58.80 / 59.96 | 302.17 | 44,763 |

Active render-thread CPU medians were 28.1–30.6 ms, versus 85.7–92.0 ms in the
original reproduction. Minor faults fell from roughly 17.4k per drawing callback
to 1.26k averaged over this run, about 93% fewer per update. Per-stroke faults
were 285–360k versus 1.30–1.36m. These are fault measurements, not exact allocation
counts. Six process major faults occurred across the ten strokes; system-wide
swap-out totaled about 78 MiB. Available RAM ended around 1.68 GiB. Canvas pixel
storage remained approximately 2.4 GiB; process PSS grew from 892 to 1752 MiB,
which includes retained history and driver/host storage outside that counter.

This candidate passed the expanded 136-tile exact mip comparison,
complete and partial submission boundaries, native U8/U16 G-Pen/photo pixel
preservation, and native undo/redo plus device replacement on the Wacom.
`device-optimized-correctness.log` records all four checks. Retired presenter
views also passed with the same framebuffer-cache implementation. Final shared
Web compilation passed; the final material changes passed all 240 numerical
reference comparisons. The previously reproduced baseline hardware failures
listed above remain unresolved and were not hidden by weaker assertions.

Historical pre-reclamation APK: `artifacts/wacom-allocation-fix/optimized.apk`, SHA-256
`d240a44d4e9540d1df392da1e0b7fc74dbf31e0a1420efce5e716ff2daae4ad7`.
Native test executable: `optimized-render-tests`, SHA-256
`fc240d8287e3850c1d85616e37149cc6f72d43c26955fb92987b319ffaea81cf`.


The extended pre-reclamation run (`optimized-long.json`) completed **5047
callbacks in 300 drawing seconds = 16.82/s**, all twenty strokes, without device
loss. GPU UI medians stayed 48.65–49.66 ms; full-stroke callback p99 ranged
163–331 ms. Mapping snapshots were captured during this stability run, so the
ten-stroke run above remains that candidate’s primary throughput comparison.

Mappings grew from 24,098 after stroke 1 to 46,778 after stroke 10 and 53,507 after
stroke 20. Most sampled mappings were GPU-driver mappings (29,105 of 41,390 in
one mid-run snapshot). This is below the previous failure near 64k but **does
not establish a plateau**. It motivated a separate experiment periodically
releasing completed pool storage while retaining per-reset command-buffer
reclamation; qualified results are recorded below.


The 96 px control on the pre-reclamation build completed both strokes:
CPU UI p99 **6.26 / 5.69 ms**, GPU p99 **9.72 / 9.18 ms**. This compares with
baseline **13.95 / 13.37 ms CPU** and **11.08 / 11.35 ms GPU**. The later
reclamation policy is checked separately for small-brush tail regressions.


### Driver-pool reclamation tuning

The first reclamation experiment released pool storage and cached framebuffers
once per 32 nonempty completed resets, continuing to free completed command
buffers on every reset. It completed twenty strokes without device loss and
reduced end-of-stroke mappings to **11,953–20,281**, with about **2.94 GiB** still
available after the last stroke, versus 1.27 GiB without periodic reclamation.
However, it averaged **14.96 drawing updates/s** and incurred late full-stroke
callback p99 values up to **1254 ms**. GPU UI medians remained 49–51 ms. The
late stalls included preparation, prediction and composition waits; this
experiment does not isolate every stall to the cleanup call itself.

The 32-reset policy also raised the 96 px CPU UI p99 to **13.28 / 10.14 ms**
(GPU **9.42 / 9.85 ms**), versus **6.26 / 5.69 ms** CPU for the preceding build.
It was therefore not accepted as the final frequency. `periodic-reclaim.json`,
`periodic-small.json` and the thermal snapshot preserve these results. The
thermal-service snapshot after that run reported status 0; it is not a
continuous record proving that frequency/thermal variation never occurred.

The final implementation spaces reclamation by 256 nonempty completed resets,
reducing mapping growth with less interruption. It retains
every-reset command-buffer freeing; it does not revive the previously failing
policy of keeping completed command buffers between resets.


### Final qualification: 256-reset pool cleanup

`reclaim256.json` completed all twenty 15-second strokes without device loss:
**4812 drawing callbacks / 300 seconds = 16.04/s**, versus **5.10/s** baseline
(**3.15× throughput**). The first fifteen strokes averaged 16.51/s and the last
five 14.64/s. These are sequential runs without fixed GPU/CPU frequencies;
the tail degradation is preserved rather than hidden by a short warm sample.

| Stroke | CPU UI median / p95 / p99, ms | GPU UI median / p95 / p99, ms | Full-stroke callback p99, ms | Process mappings |
| --- | --- | --- | ---: | ---: |
| 1 | 20.31 / 25.94 / 33.29 | 49.82 / 59.20 / 64.82 | 208.73 | 21,698 |
| 2 | 21.02 / 29.50 / 37.25 | 49.05 / 57.77 / 58.58 | 232.46 | 21,940 |
| 3 | 18.94 / 25.49 / 33.38 | 49.39 / 57.76 / 59.67 | 209.79 | 19,919 |
| 4 | 18.70 / 26.14 / 34.54 | 49.51 / 58.92 / 60.61 | 216.44 | 23,757 |
| 5 | 19.07 / 25.01 / 34.84 | 49.54 / 59.78 / 71.36 | 259.08 | 17,578 |
| 6 | 19.21 / 27.12 / 30.19 | 49.07 / 59.24 / 66.07 | 230.73 | 22,705 |
| 7 | 19.53 / 25.23 / 29.06 | 49.18 / 58.67 / 59.33 | 195.13 | 25,849 |
| 8 | 19.95 / 26.54 / 29.48 | 49.25 / 58.64 / 59.71 | 227.01 | 26,696 |
| 9 | 19.28 / 27.59 / 35.16 | 49.25 / 59.28 / 67.84 | 218.95 | 26,204 |
| 10 | 20.36 / 28.11 / 32.82 | 49.33 / 58.68 / 60.57 | 222.97 | 22,414 |
| 11 | 18.15 / 26.85 / 29.70 | 49.39 / 58.70 / 59.40 | 217.29 | 22,344 |
| 12 | 20.16 / 29.77 / 54.70 | 49.33 / 58.91 / 59.69 | 262.27 | 23,644 |
| 13 | 19.42 / 25.73 / 36.91 | 49.09 / 58.84 / 66.15 | 218.83 | 27,296 |
| 14 | 19.25 / 27.31 / 30.96 | 49.42 / 58.37 / 60.75 | 276.13 | 29,583 |
| 15 | 21.86 / 29.28 / 35.46 | 49.24 / 58.05 / 58.90 | 246.49 | 28,546 |
| 16 | 22.39 / 30.35 / 48.51 | 49.08 / 58.27 / 59.77 | 457.69 | 30,296 |
| 17 | 23.18 / 33.85 / 52.48 | 49.69 / 59.54 / 72.33 | 523.39 | 27,321 |
| 18 | 23.48 / 31.38 / 48.00 | 49.42 / 58.18 / 67.12 | 367.28 | 31,079 |
| 19 | 22.57 / 33.75 / 48.87 | 49.08 / 58.41 / 66.75 | 356.88 | 28,387 |
| 20 | 22.56 / 29.87 / 43.78 | 49.24 / 59.00 / 65.37 | 309.91 | 27,476 |

Active render-thread CPU medians are **28.3–32.8 ms**, compared with **85.7–92.0
ms** originally. Minor faults average **1283 per drawing callback**, down from
17,378 (about **93% fewer**). Six process major faults occurred across twenty
strokes. System-wide swap-out totaled approximately 140 MiB; this is not all
attributable to this app. Available RAM went from 5.22 to **2.41 GiB**. Process
PSS ended at **1229 MiB**, versus 830 MiB after the first stroke, peaking at
1485 MiB. Tracked canvas storage remains about 2.4 GiB.

End-of-stroke process mappings ranged from 17,578 to 31,079 and decreased on
multiple cleanup cycles; the last five were **30,296 / 27,321 / 31,079 / 28,387 /
27,476**. This materially improves on the pre-reclamation run's growth to
53,507. Twenty strokes establish sustained behavior for this run, not proof
of an indefinite global memory plateau. Undo/history storage still accumulates
as strokes are committed.

The final small-brush control (`reclaim256-small.json`, 96 px, two 15-second
strokes) has CPU UI p99 **8.75 / 10.07 ms**, GPU UI p99 **9.74 / 9.43 ms**, and
full-stroke callback p99 **15.93 / 12.55 ms**. CPU tails are higher than the
6.26 / 5.69 ms pre-reclamation candidate, but still below the original
13.95 / 13.37 ms control. This is the measured storage/latency tradeoff.

The final Android native test executable passed the **384-cycle retired-view
and cache-shape test** and the **136-tile bit-exact mip comparison**. Logs:
`device-reclaim256-lifetime.log` and `device-reclaim256-exact.log`. The preceding
four pixel/history/submission checks, all 240 numerical material comparisons,
and final native/Web compile checks are described above. The existing baseline
hardware failures remain unresolved; no assertion was relaxed.

Per-UID GPU counters average **56.84 ms of activity per drawing callback**,
ranging from 54.0 to 65.0 ms across strokes, with roughly 80–89% activity during
the surrounding resource windows. Together with the approximately 49 ms
renderer timestamps, this puts the current **measured-work** ceiling around
**18–20 updates/s** before other overhead. The achieved 16.0/s is roughly
80–90% of that conditional bound. GPU execution/scheduling now dominates warm
throughput; CPU preparation, resource growth and synchronization still cause
cold stalls. These counters cannot distinguish arithmetic from bandwidth or
shader occupancy, and backlog changes mean this is not a proof of the current
algorithm's theoretical maximum. No brush sampling or evaluation changes were
made. Further brush-algorithm work is deliberately deferred.

Final installed isolated APK: `artifacts/wacom-allocation-fix/optimized-final.apk`,
SHA-256 `4d128a3a7dedf8292b2b948e9395ac272798697d4641d00a46007804d11156de`.
Final native test executable: `reclaim256-render-tests`, SHA-256
`89d079364caffb391f72081f6d5c35ad3d9d4b313de85a20f3b53f670de0feb5`.
The isolated package is `art.capycanvas.penperf` (Capy Pen Performance).
The regular application APK was not replaced.


### Remaining fast-stroke failure and device handoff

The final 2048 px / 2 Hz / 8 ms prediction retest still loses the GPU device
after three drawing callbacks (**359 / 1144 / 2527 ms**). Baseline failed in the
same scenario after **372 / 1311 / 3250 ms**. This is **not fixed** by the
allocation/copy refactor. `optimized-final-stress.json` and
`optimized-final-stress.log` preserve the failed test; no usable GPU timing
percentiles survived device loss.

The post-failure process had only 5520 mappings, with 5.19 GiB available before
the stroke and 7.82 GiB after renderer teardown. Those snapshots do not establish
peak memory during the failure, but they do not reproduce the sustained
mapping-limit mechanism. Preparation/prediction stalls accumulated a large
input backlog before the long composition wait. A watchdog, long shader work,
resource failure, or another driver error has not been isolated. Therefore the
successful twenty-stroke 1 Hz result is workload-specific, not a claim that all
wide-brush motion is now safe or that the remaining shader has a proven maximum.

After testing, the isolated app was stopped and the original app relaunched
at its recovery prompt (recorded in `original-app-final.png`). No recovery was
discarded or overwritten. Both original `.capy` recovery files were verified byte-for-byte unchanged
against the pre-test backup (`original-recoveries-final.tar`). The regular APK
and artwork were not replaced. The optimized isolated APK remains installed
for review; no commit or deployment to the regular app was performed.


## Merge-readiness review

The optimization was reviewed against freshly fetched `origin/main`
`a23c627acb769e22b5b6cfc888689e6ad8715b39`, nine commits ahead of the measured
baseline. The tracked patch applies cleanly to a separate archive of that
revision (`/tmp/capy-pen-merge-review`); this is an integration check, not a
merge or change to the active branch. During qualification, `origin/main`
advanced once more to `54ca69cc5488202f21f881914688a4557f2b148d`; its
packaging/test-fixture-only delta also applies cleanly to the integration copy
and does not change renderer code. GTK checks pass again on that revision
(`review-main-latest-gtk-check.log`). New documentation and the vendor patch
must be included with the source changes. The unrelated local
`capycanvas-tablet-handoff.md` is outside this change.

The review found a portability error in the new test, not in renderer routing:
the batch-count assertion assumed Float32 hardware blending. With
`CAPY_GPU_NO_FLOAT32_BLEND=1` it expected zero intermediate submissions for
128 tiles while the correct eight-tile fallback produced fifteen. The test now
checks the actual capability contract, restores the original bounded-cache
8/9/16/17/32-tile cases, and covers complete-cache batching separately.
The rejected single-dispatch experiment and old one-off source-upload helper
are absent. The scratch reduction, partial cache and portable blending paths
remain necessary fallbacks, not dead code. Stale “half” batching comments and
vendor reset formatting were cleaned up. The subsequent correctness fix below
simplifies normal source-over without changing brush geometry, tuning or the
mathematical blending rule.

| Change | Android | Apple / GTK | Web |
| --- | --- | --- | --- |
| Pooled uploads, material records, reused job/binding-table capacity | Shared | Shared | Shared |
| In-place retained mip updates | When complete cache admitted | Same admission requirement | Same admission requirement |
| Direct composition and larger batches | Requires complete cache and hardware Float32 blending | Same capability checks | Same capability checks; portable blend retains eight-tile batches |
| Framebuffer reuse and periodic Vulkan pool cleanup | Android Vulkan only | Existing backend policy | Existing backend policy |
| CPU attribution telemetry | Debug-only Android host and opt-in test | No added host telemetry | No added host telemetry |

The browser live renderer already calls `raster_worker::install`, which sets
its capacity-based complete-display allowance. Native Apple/GTK/Android query
their existing memory-admission policy. No platform fork or duplicated brush
implementation was introduced, and no new precision or memory-admission policy
was added. Performance gains on other physical platforms are not yet measured.

Integration checks and numerical tests are recorded in
`artifacts/wacom-allocation-fix/review-main-*.log`. These supplement the earlier
Wacom qualification; software GPU results establish correctness, not performance.


## Black contact-border investigation and correction

The artist reported black borders in **Capy Pen Performance**, the isolated
optimized build. `review-black-borders-current.png` records the visible repeated
crescents. Its recovery was backed up as `user-border-recovery.tar` before any
investigation; native test executables run in separate processes and do not
change either application's document.

A new end-to-end native G-Pen regression paints a flat opaque source with
2048 px contacts, varying pressure and 16 ms prediction. It checks every valid
saved pixel: normal source-over cannot make a channel darker than both the
ink and background, or remove the opaque background's alpha. This catches
corruption missed by the older untouched-corner/alpha-only photo assertion.

| Controlled Wacom run | Dark pixels below both inputs |
| --- | ---: |
| Pre-allocation-refactor HEAD, prediction enabled | 119,428 |
| Allocation-refactored build, prediction enabled | 119,428 |
| Prediction disabled | 0 |
| Prediction policy enabled, GPU preview batches suppressed | 119,428 |
| Fragment renderer instead of dry compute | 119,428 |
| Scratch native publication instead of in-place publication | 119,428 |
| Original uniform-dispatch shader reference | 0 |
| Simplified normal source-over, final compute and fragment variants | 0 |

The two builds had the same first failing pixel, `(2303, 332)`, with RGBA
`[170, 184, 63, 255]`. This defect predates the allocation refactor. Prediction
changes the committed batch structure; the disposable preview itself does not
need to render for corruption to occur. Changing native publication mode or
fragment/compute execution did not help. Software rendering passed the original
fixture, while the physical Adreno shader path failed.

The shared material shader now handles normal blending directly:
`destination.rgb * (1 - source_alpha) + source.rgb * source_alpha`, with the
existing alpha-locked equivalent. This is algebraically the same source-over
rule; it avoids unnecessary backdrop unassociation and the expanded general
blend expression. The original expression is mathematically valid, so these
controls point to a shader/backend compilation issue on this device, rather
than proving a fault in allocation ownership. The exact compiler defect is not
established. Other blend modes retain their original implementation.

The final regression covers U8 and U16 documents, opaque and translucent ink,
and compute and fragment rendering, checking saved native pixels and requiring
that the stroke actually painted. All four combinations pass on the Wacom and
on the numerical software renderer. The existing photo preservation test also
passes on Wacom in both native publication modes. The full native workflow test
also passes on Wacom across all four working color spaces and U8/U16/F16/F32:
undo/redo, save/reopen, continued drawing and device replacement preserve the
canonical samples (`review-border-final-history.log`, 557.45 seconds). The current-main integration
copy passes the new regression with hardware Float32 blending enabled and with
`CAPY_GPU_NO_FLOAT32_BLEND=1` forcing the portable fallback. All temporary diagnostic environment switches
and alternative shader experiments were removed from source.

Evidence:

- `review-border-large-baseline-wacom.log` and `review-border-large-wacom.log`:
  identical pre-fix failures.
- `review-border-no-prediction.log`, `review-border-no-gpu-preview.log`,
  `review-border-fragment-wacom.log`, `review-border-scratch-commit.log`, and
  `review-border-uniform-wacom.log`: isolation controls.
- `review-border-final-wacom.log`: both native G-Pen tests pass.
- `review-border-final-history.log`: all 16 native workflow combinations pass.
- `review-border-final-software.log`, `review-main-border-fixed.log`: final
  regression passes numerically on the working branch and integrated main.
  `review-main-border-fixed-portable.log` also passes without hardware Float32
  blending.
- `review-border-final-material-software.log`: all 240 specialized/reference
  material image comparisons pass numerically.

A separate package, `art.capycanvas.penverify` (**Capy Pen Validation**), was
installed for the fixed-build native UI check, preserving the artist's existing
Capy Pen Performance session. `border-fixed-photo.png` shows the first completed
2048 px stroke on the 61 MP photo without the repeated black borders. The
brush-outline cursor in that screenshot is UI, not painted pixels.

Two 15-second, 1 Hz, 16 ms prediction runs completed without a renderer failure:

| Fixed-build run | Updates/s | GPU median | Active owner CPU median | Full-stroke callback p99 | Process mappings |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 15.73 | 49.24 ms | 29.06 ms | 255.19 ms | 20,427 |
| 2 | 17.73 | 49.14 ms | 30.06 ms | 200.64 ms | 28,933 |

These short runs confirm retention of the earlier optimization's performance;
they do not replace the longer 20-stroke qualification or establish a new
speedup claim. GPU summaries are the host's rolling 120 samples; callback
percentiles use all paint callbacks during each stroke. Full evidence is in
`border-fixed-performance.json`, `border-fixed-performance-analysis.json`, and
`review-border-final-performance.log`.

Fixed validation APK: `border-fixed-validation.apk`, SHA-256
`1df948fe36ac11160f4efd57cb0656221d4e45a08e3c3c932a481556a86aa8dd`.
Native regression executable: `border-final-render-tests`, SHA-256
`9e274132d835436755811d69038914690b01b1e804b1352b50524e5936f2f4fe`.

This fix prevents new corrupted paint; it cannot infer or reconstruct source
colors already overwritten by a bad stroke. The saved artist recovery is
preserved for manual recovery/undo decisions. The final performance-app backup
(`user-border-recovery-final.tar`) contains the same 164,621,415-byte project
as the initial backup, with SHA-256
`23538313f94f3340a9533baceb749e96af7ab139393802cda6fe25c95eac70cb`.
The regular app's current recovery files were also backed up independently as
`review-regular-recoveries.tar`; neither existing package was replaced.

### Remaining qualification limits

The Wacom's pre-existing material specialization/reference comparison still
fails in the legacy non-native Multiply preview case (operation 0, variant 0,
phase 1, maximum channel difference 93; final candidate log:
`review-border-final-material-wacom.log`). The same failure occurs before the
allocation refactor. It is not covered up by a relaxed assertion or by the
normal-blend correction. The earlier 2 Hz stress/device-loss limit also remains
unresolved; the new 1 Hz runs do not qualify that workload. These prevent an
unqualified claim that every Android brush/rendering scenario is regression-free.

The scoped allocation changes and normal G-Pen fix have no newly observed
failures in the checks above. Shared optimizations and the shader correction
benefit all hosts using this renderer, while physical Apple/GTK/Web performance
and complete behavior qualification remain unmeasured. No merge, commit or
push was performed, and brush-algorithm redesign remains paused.

## Follow-up: 2048 px, 2 Hz execution attribution

On Wacom `5ll21u1002931` (DTHA140), the isolated
`art.capycanvas.penverify` package reproduced device loss with the same 61 MP
photo, a 2048 px G-Pen, 2 Hz motion, and 8 ms prediction. This package was
used so the installed artist application was not replaced. The host timing
records CPU phase boundaries and whole-submission GPU timestamps; it does not
yet expose per-pass GPU timestamps.

The failed run had four rendering callbacks. Their total callback times were
131.8, 545.0, 1333.2, and 2864.2 ms. On the fourth callback the CPU owner used
only 44.8 ms while composition/present blocked for 2847.7 ms. The preceding
third callback spent 499.3 ms in preparation and 818.6 ms in composition.
This is a growing GPU queue/backlog, not CPU saturation or a long CPU-only
brush loop. Android exposes no usable Adreno per-pass counter through `dumpsys
gpu`; whole-frame timestamps and the host phase boundaries are the available
GPU attribution on this device.

Setting the prediction horizon to zero reproduced the same fourth-callback
device loss (124.4, 447.5, 997.6, and 1932.7 ms), including a final 1914.5 ms
composition/present block. Therefore the look-ahead distance is not the cause.
A 1024 px, otherwise identical 2 Hz control completed: CPU p50/p95/p99
14.91/23.03/26.80 ms and GPU p50/p95/p99 33.49/40.17/41.22 ms over 120
timestamped updates. Doubling diameter quadruples the local pixel field and
crosses a device-specific GPU work/backlog limit.

After the Multiply routing change, the rebuilt isolated package also completed
the 2048 px, 1 Hz, 16 ms control: CPU p50/p95/p99 17.88/28.33/30.10 ms and GPU
p50/p95/p99 48.58/58.30/58.74 ms over 120 timestamped updates. This checks the
unchanged normal G-Pen compute route; it is not a hardware qualification of the
Multiply fallback.

`simpleperf` hardware counters during the failed 2 Hz attempt reported 1.35
GHz and IPC 1.03 over its ten-second app-wide interval; this interval includes
startup and is not a paint-only comparison. It is nevertheless inconsistent
with a CPU-core saturation explanation. A successful 1 Hz control recorded
substantial Vulkan descriptor activity in the app-wide profile, but cannot
separate driver work from paint shader execution. These are diagnostic results,
not a claimed performance improvement.

The dry-material path dispatches each affected 256x256 page independently.
Within each pixel invocation it evaluates its ordered dab range, so the
unavoidable work is proportional to covered pixels times contacts per pixel;
pages parallelize that work but do not remove it. A work-budget/backpressure
policy would cap the amount submitted before the GPU queue grows without bound.
Changing that bound is scheduling, not a brush-algorithm redesign; making the
brush approximation cheaper would alter brush behavior and is out of scope.
