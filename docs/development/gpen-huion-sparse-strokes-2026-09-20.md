# Huion: sparse swept contacts and direct float composition

2026-09-20. Initial measurement source base: `a8c51612cb5b40286fd21780a4d7adae8822bf14`.

Three costs explained the G-Pen GPU tails: prediction evaluated contacts across
entire pages instead of their footprint, pressure-dependent stamp spacing
produced redundant contacts for an already swept brush, and non-blendable
Float32 composition rendered and copied intermediate tiles. The initial
prediction-bound fix reduced the light-pressure 1K GPU p99 from roughly 30 ms;
the matched measurements below isolate the subsequent generator and composition
improvements. All three fixes are part of the final implementation.

## Latest-main verification

After cleanup, the optimization was rebased onto `df3940d53a98680bb4ceadc492556a19119d6660`
and measured again on the same Huion against an APK built from that exact `main`.
This includes the newer Android tracing, source-cache and command-batching work.
Both comparison APKs use identical temporary timestamp collection and the same
native input fixture, without adding GPU phase timers or calibration passes. The initial
timestamp drain before each measured stroke is excluded; valid observations are
deduplicated by frame. CPU is the same six renderer phase fields described below.

| Workload | GPU median, main → merged | GPU p99, main → merged | CPU p99, main → merged | GPU p99 speedup |
| --- | ---: | ---: | ---: | ---: |
| 1024×1024, 18 px, normal | 6.17 → 0.98 | 11.57 → 2.38 | 6.54 → 3.31 | 4.86× |
| 2048×1536, 20.2 px, fast | 20.04 → 1.82 | 61.51 → 5.02 | 10.78 → 7.34 | 12.26× |
| 2048×1536, 512 px, normal | 18.54 → 5.43 | 34.95 → 12.33 | 16.47 → 8.43 | 2.83× |

GPU sample counts, main/merged: 1068/1332, 419/1053 and 200/352 respectively.
The 1K and fast tests each contain three five-second strokes; the large-brush
control contains two three-second strokes at fixed pressure 1. CPU medians also
fell in every case: 2.82 → 1.47, 4.40 → 2.23 and 8.00 → 3.58 ms.

The optimized 1K p99 remains 2.38 ms and fast-stroke median remains 1.82 ms,
matching the earlier measurements. This latest-main baseline does not contain
the prediction-bound fix, so its fast-stroke speedup includes that fix as well.
Device clocks were not pinned; these are matched synthetic drawing measurements,
not a guarantee for every hand-drawn frame.

The scripts, raw JSON, APKs and build logs are retained locally under
`artifacts/gpen-huion-2026-09-20/merge/`. The isolated comparison checkout was
removed after archiving; benchmark hooks are absent from the production sources.

Merge validation covered all 65 engine tests and 315 non-ignored GPU renderer
test cases with Float32 attachment blending disabled. Two existing allocation
and batching assertions assumed the old device/path selection; their expectations
were corrected and the tests passed on separate reruns, retaining the pixel
checks. The 31 opt-in benchmark/profile-dependent cases remained ignored as
declared. The arm64 Android production release build and Huion drawing-history
lifecycle test also passed.

## Original matched results

Native Android, Huion Kamvas Pad 12 / KP1202, Mali-G57 MC2, Vulkan. Baseline
already includes the previously deployed prediction-bound fix. GPU spans exclude
presentation and input delivery. These totals have no extra phase markers.
Numbers pool the valid observations from three five-second strokes, or two
three-second strokes for the 512 px control. Values are milliseconds.

| Workload | GPU median, before → after | GPU p99, before → after | p99 speedup | GPU samples, before / after |
| --- | ---: | ---: | ---: | ---: |
| 1024×1024, 18 px, normal | 5.02 → 0.88 | 8.01 → 2.38 | 3.37× | 1132 / 1307 |
| 2048×1536, 20.2 px, normal | 4.84 → 1.08 | 9.83 → 2.60 | 3.78× | 1000 / 1154 |
| 2048×1536, 20.2 px, fast | 10.16 → 1.82 | 27.22 → 4.43 | 6.14× | 670 / 1130 |
| 2048×1536, 512 px, normal | 17.83 → 6.11 | 33.25 → 15.98 | 2.08× | 215 / 349 |

| Workload | CPU median, before → after | CPU p99, before → after |
| --- | ---: | ---: |
| 1024×1024, 18 px, normal | 2.40 → 1.51 | 6.01 → 4.23 |
| 2048×1536, 20.2 px, normal | 2.88 → 1.95 | 6.58 → 4.98 |
| 2048×1536, 20.2 px, fast | 4.07 → 2.03 | 10.03 → 4.80 |
| 2048×1536, 512 px, normal | 7.85 → 3.66 | 18.71 → 10.50 |

CPU is preparation/encoding/submission wall time, reconstructed from the six
native renderer phase fields, excluding presentation. It fell rather than rising.
Fast light-pressure drawing (pressure 0.10, 2048×1536 / 20.2 px) finished at
1.68 ms median / 4.23 ms p99. The older light-pressure fast
stress did not finish reliably, so it has no claimed numeric speedup.

The matched fast baseline reached 52.50 ms; the final fast run's maximum was
12.73 ms. This is not a guarantee that every future frame
will stay below that value. The visible Diagnostics p99 uses a rolling 120-sample
window; the table uses complete runs and can differ from that display.

## Changes and algorithm cost

- Contact brushes keep modeled input poses with an online geometric/pose
  simplifier, instead of inserting dense pressure-spaced stamps between them.
  The error bound uses traveled path length and chord length, with O(1) state
  and work per input. It preserves bends and meaningful pressure/tilt/twist
  changes; steady large brushes retain coarse spacing. The final endpoint is
  flushed. Live drawing, prediction, and replay share this generator, with no
  compatibility switch or retained dense-contact implementation.
- The first normal paint layer can apply source-over against its constant paper
  backdrop. On non-blendable Float32 devices, the existing scene job then writes
  directly into the changed canvas tile with compute. This removes the scratch
  render, destination copy, separate blend, final tile copy, and large render
  attachment load/store for that case. Masks and opacity use the same shader
  function as ordinary composition. Other layer stacks retain the existing path.
- Aligned one-to-one scene reads use a single texel load. Fractional sampling
  retains interpolation. Constant masks no longer perform a texture read.
- Full-page dry evaluation initializes a new stroke's coverage from the existing
  shared zero source and writes its new coverage alongside color. It no longer
  clears that page in a separate render pass before first use.

No fine spatial bins, per-pixel contact lists, or extra contact-search branches
were added. Ordinary stamped brushes retain their spacing loop. Contact density
and procedural variation change intentionally; old/new brush pixels are not
promised identical. Other materials were not visually qualified for this change.

In the fast baseline, median contacts submitted per drawing update were 164;
the final path uses 6. Median estimated pixel/contact candidates fell from
2.07 million to roughly 69 thousand. These are conservative shader-loop estimates,
not hardware instruction counters. The faster path also submits more frames for
the same incoming trace, which reduces the amount of input batched per update.

## Memory bandwidth and remaining floor

The device identifies as MT8391. MediaTek documents it as Genio 720 with Mali-G57
MC2, supporting LPDDR5 up to 6400 MT/s or LPDDR4X up to 4266 MT/s
([official SoC documentation](https://genio.mediatek.com/doc/android/hw/mt8391-soc.html)).
The tablet's actual DRAM configuration and operating clock were not readable
without privileged access. For an assumed 32-bit bus, the nominal data-rate ceilings
would be 25.6 or 17.064 GB/s respectively (`MT/s × 4 bytes`). These are conditional
upper bounds, not a verified Huion memory specification.

A GPU timestamp probe uses two RGBA32Float textures and two buffers, initialized
with spatially varying data. Ten measured trials follow two warmups. Sixteen
alternating copies amortize submission overhead; a single-copy control exposes
small-job overhead. Both reads and writes count toward logical bytes transferred.

| Probe | Effective logical bandwidth |
| --- | ---: |
| 2048² compute copy, 16 copies | 17.37 GB/s |
| 2048² texture copy, 16 copies | 17.55 GB/s |
| 2048² buffer copy, 16 copies | 16.22 GB/s |
| 256² compute copy, 16 copies | 8.60 GB/s |
| 256² compute copy, one copy | 6.41 GB/s / 0.327 ms |

The accepted probe results are in `bandwidth-valid-bandwidth.json`. Shader
validation and variable timestamps were verified before accepting the probe.

For the final simple paper-plus-ink scene, a useful logical-traffic model is:

```text
bytes ≈ 65536 × (40 × committed dry pages + 32 × predicted color pages)
        + 32 × composited pixels
```

A committed page reads/writes 16-byte color plus 4-byte coverage (40 bytes/pixel).
A predicted page reads/writes color (32 bytes/pixel), with additional coverage
reads inside its contact bounds. Composition reads color and writes the final
color (32 bytes/pixel). Source cache, metadata, history capture, coverage reads,
initialization and presentation are not fully counted. These are logical bytes,
not measured DRAM transactions; caches and driver behavior affect traffic.

The final fast attribution run, excluding pen-up and missing frame joins,
has median modeled traffic **13.63 MB/frame** and median GPU time **1.88 ms**.
The ideal bulk-copy floor is **0.79 ms**; using measured tile-copy throughput gives
**1.59 ms**. Median per-frame effective throughput is **7.00 GB/s** overall and
**8.28 GB/s** for composition: about **81%** and **96%** of the tile-copy calibration.
Phase markers add overhead, so use the uninstrumented table for user-facing timing.

This approaches the measured small-tile transfer rate, but does **not** reach the
physical DRAM ceiling. The remaining difference includes brush evaluation,
dispatch/barriers, coverage, and scheduling. Pen-up also captures persistent raster
history: the earlier compute attribution observed 8.7–11.0 ms capture spans on
two stroke endings. Further major reductions would require changing full-page
paint/prediction storage or history capture; simply reducing more contact tests
will not remove that traffic.

## Method and validation

Synthetic 240 Hz pointer ingress goes through the native app's normal owner and
vsync scheduling. The fast trace runs the same curve at four times the normal
speed. Pressure varies from 0.35 to 0.95, except the fixed-pressure controls.
The canvas fits at 51.86% zoom for 2K and 103.71% for 1K. Engine prediction has
its normal 16 ms horizon; platform prediction is disabled for synthetic input.
Each test uses a fresh isolated document and a 1.5-second warm stroke. Device
clocks are not pinned. These are reproducible traces, not a recording of the
user's exact hand movement or an input-to-photon measurement.

Validation passed:

- All 65 layer-engine tests, including corner/pressure retention, sparse fast
  contacts, large-brush spacing, endpoint behavior, and replay tests.
- GPU comparison of sparse versus full tiled composition, including fractional
  placement, masked/translucent paint, odd edge extents and distant pixels.
- Preview/commit/cancel pixel checks and native Float32 coverage persistence
  within a stroke and reset across strokes.
- Native startup/filter compilation with Float32 attachment blending disabled.
- Huion drawing history: exact undo/redo, saved raster history, spill/reload, and
  Activity recreation. The separate fault-injection recovery test requires a
  debug session and was not runnable in the release benchmark variant.

Temporary JNI probes, per-frame counters, and calibration shaders are absent
from the production build. `instrumentation.patch`, benchmark APKs, raw JSON,
analysis scripts, screenshots, build logs and a production source patch are
retained under `artifacts/gpen-huion-2026-09-20/sparse/` in the original checkout.

## Deployment

Replaced `art.capycanvas` in place with the merged arm64 production release
(R8/resource shrinking, release Rust, existing signing certificate). Application
data is preserved. Removed `art.capycanvas.gpenaudit` and its instrumentation app.
Launch succeeded, the installed APK hash matches, and only `art.capycanvas`
remains installed.

The merged build also passed `AndroidRasterTest#drawingTabsKeepHistorySpillAndLifecycle`
on the Huion after the matched benchmark runs.

APK SHA-256: `d5b5945657d7cb9be1794bbda037d7ae14a2ded7bda8d714617cd3ada7e7cdd0`.
