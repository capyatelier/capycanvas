# Wacom assessment of main commit 39e9cba3

This compares `39e9cba3537e604705755ab4ac79dc870bc96884` (sparse swept contacts,
prediction bounds and direct Float32 composition) against its parent
`df3940d53a98680bb4ceadc492556a19119d6660`, the previously qualified
[30 FPS Wacom build](android-pen-30fps-20260920.md).
No rendering implementation was modified for this assessment.

## Result

**Drawing performance improved on the Wacom.** Repeated light-probe fast strokes
rose from a median **31.99 to 35.54 visible canvas updates/s**, an **11.1%** gain.
The new build was faster than every old-build fast-light repetition. This is
a useful incremental improvement, not the several-fold Huion gain on other
workloads. Hover is similar; redo is within the observed variation. Startup
stalls remain and not every latency percentile improves.

| Fast stroke, light probes | Canvas updates/s | Undo first latch | Redo first latch |
| --- | ---: | ---: | ---: |
| Old, initial process | 31.19 | 462 ms | 475 ms |
| New, after install | 35.29 | 291 ms | 496 ms |
| New, repeat | 35.79 | 284 ms | 464 ms |
| Old, reinstalled control | 31.99 | 305 ms | 577 ms |
| Old, repeat after reinstall | 32.69 | 370 ms | 431 ms |

This old→new→old sequence checks restart/process-age effects. The original
old process had been used since the previous investigation; both versions were
also measured after reinstall/recovery and after undoing a previous workflow.
These are five short repetitions, not a formal confidence interval. Available
system memory varied approximately 3.3–4.5 GiB; all captures report thermal
status 0 before and after, and clocks were not pinned.

Light-probe hover spans 77.2–84.0/s on the old build and 80.8–84.4/s on the new
build, depending on before/after drawing. Undo is faster in the new light runs,
but full-probe slow undo is unchanged at about 379 ms. Redo has no demonstrated
consistent gain. Fast light-probe largest latch gaps are 206–242 ms old and
212–237 ms new; stroke startup is still visibly discontinuous. The slower
full-probe p99 gap increases from 74.6 to 83.0 ms despite better average FPS and
p95. These few tail samples do not establish a systematic tail regression.

All nine accepted captures have fully retained actions and zero trace error
diagnostics. The candidate is restored on the tablet at the end of the assessment.

## Matched CPU/GPU attribution

The same optimized/profileable `art.capycanvas` package, Wacom
`5ll21u1002931`, 9504×6336 photo, empty selected paint layer, hidden paper,
2048 px opaque G-Pen and 17.164% fit zoom were used. AndroidPenMotion injects
200 Hz pressure-1 stylus events around the existing ellipse at one loop/s
(fast) or 0.25 loop/s (slow), with 16 ms prediction fallback. The slow replay's
final pen-up chord remains part of the workload. Full workflows also include
hover, undo and redo with the same settling intervals.

These full-probe captures have matching source-cache limits (256 MiB resident,
64 MiB in flight), matching zoom counters, fully retained actions and no trace
error diagnostics. Both versions have zero upload-drain increments during
drawing. CPU figures are scheduled owner CPU per callback; GPU figures are mean
observed intervals, which include submission gaps. CPU/GPU times overlap.

| Measurement | Old fast | New fast | Old slow | New slow |
| --- | ---: | ---: | ---: | ---: |
| Actual canvas latches/s | 29.19 | **33.29** | 36.48 | **39.69** |
| CPU scheduled / callback | 26.79 ms | **24.12 ms** | 19.42 ms | **18.23 ms** |
| GPU paint | 4.83 ms | 4.42 ms | 1.42 ms | 1.18 ms |
| GPU prediction | 6.49 ms | 5.11 ms | 6.03 ms | 4.78 ms |
| GPU composition/mips | 12.68 ms | 10.67 ms | 9.45 ms | 8.05 ms |
| Whole GPU interval | 24.04 ms | **20.25 ms** | 16.95 ms | **14.05 ms** |
| Input owner-queue median / p99 | 12.5 / 115.5 ms | 10.2 / 93.5 ms | 9.0 / 56.8 ms | 7.7 / 53.4 ms |
| Visible latch-gap p95 | 50.1 ms | 49.8 ms | 49.9 ms | 42.1 ms |

Fast drawing improves **14.0%** and slow drawing **8.8%** with full probes.
Mean GPU intervals fall **15.8% / 17.1%**; scheduled CPU falls **10.0% / 6.2%**.
These are measured workload-level differences, not independent kernel speedups.

Fast command finalization falls from 8.35 to 6.86 ms CPU/callback; composition
encoding from 5.66 to 4.96 ms. Binding creation remains approximately 3.1 ms,
and bindings/callback remain about 398 versus 401. Vulkan command-buffer
allocation API scopes fall from 134 to 123/callback. The active owner remains
an important limiting stage; the change does not eliminate driver overhead.

Composited output remains similar: 6.88 versus 6.94 MP/publication between
the first and last fast-stroke counter samples. Source misses fall from 18.49
to 16.20/publication. The cumulative dab metric includes both committed and
preview batches: its delta is 841 versus 903 over the respective fast captures.
It is not evidence of a dramatic total-contact reduction on this workload.
The large Huion gains on smaller canvases/light-pressure input should therefore
not be projected onto this Wacom photo test. Composition and prediction account
for most of the measured GPU interval reduction here.

## Interpretation and correctness scope

The commit intentionally changes contact density and procedural variation;
[its own report](gpen-huion-sparse-strokes-2026-09-20.md) does not promise old/new
brush-pixel equivalence. This assessment is a matched-input performance comparison,
not a claim that every brush setting produces the old pixels.

All seven fast-run screenshots have the same RGB(19,19,18) at 39,600 sampled
positions per screenshot safely inside the opaque stroke; no recurrence of the earlier dark
seams was found there. The first light-run opaque masks have 99.864% intersection
over union within the canvas crop. That supports similar gross geometry, not
exact equivalence: these real-time replays have slightly different timestamps,
and the new generator intentionally simplifies the path.

Physical DRAM traffic and memory-stall counters remain unavailable. Faster
composition does not establish that the device was bandwidth-saturated or that
all gains came from removing copies. The commit bundles several changes; this
comparison does not isolate each one. Its constant-backdrop compute path is
conditional on layer shape and Float32 blending capabilities, and the photo
stack differs from the Huion paper-plus-ink scene.

## Validation and test limitation

The optimized/profileable Android build and all 65 engine tests passed. After
the performance captures, the candidate's Android renderer test binary passed
both `native_gpen` cases on the Wacom: opaque source pixels do not acquire dark
batch seams, and untouched photo pixels in touched tiles are preserved. The
Float32 coverage-persistence/reset test also passed.

The new `constant_backdrop_sparse_updates_match_tiled_float_composition` test
**fails in its default configuration on this Wacom**, at
`layer_tests.rs:3118`: it unconditionally unwraps `r.scene`. The unmasked simple
scene uses Float32 attachment blending here; `scene_required` is false, so the
renderer legitimately leaves that optional state absent. Running the unchanged
binary with `CAPY_GPU_NO_FLOAT32_BLEND=1` passes the same test, including its
masked cases and the <1e-5 Float32 comparison. This confirms the test assumes the
nonblendable path used in the other agent's validation; it is not evidence of
a drawing failure. The default-test portability defect remains unfixed in this
measurement-only assessment. Logs preserve both the failure and passing rerun.

## Reproduction

Raw evidence lives in `artifacts/wacom-main-comparison-20260920/`. It contains
APKs/symbols, setup/result screenshots, capture and analysis scripts, raw Perfetto
and simpleperf data, BOOTTIME action markers, thermal/memory state, logs,
action-filtered CPU profiles and hashes. The baseline APK/symbols are also
preserved in `artifacts/wacom-30fps-20260920/final*`.
The [tracked measurement summary](measurements/android-pen-main-comparison-20260920.json)
contains all nine runs, presentation distributions, GPU intervals, CPU time,
counter boundaries and thermal/memory observations.

APK SHA-256 identities:

- Baseline: `ca51c74d20d31517969798ad15e42dd3394b4530aadbb9bea549f48aa94c107d`.
- Candidate: `9112bff5418797eda70818e9fd341746b0883cadb3f7496dc5ee05c0119cd489`.

The final installed package matches the candidate hash. `final-primary.png`
records the photo ready for drawing with an empty paint layer and 2048 px G-Pen.

Use `capture-run.py LABEL 1 capture-light.pbtxt` for throughput, or omit the
config argument for full CPU/GPU phase attribution; use `.25` for slower motion.
Undo the preceding workflow stroke before repeating. Restore the selected empty
paint layer and verify 2048 px and 17.164% after each APK replacement. The injected
sequence and report definitions are unchanged from the original investigation.

SurfaceFlinger SurfaceView latches measure actual canvas-buffer consumption,
not physical nib-to-photon latency or proof that every latch contains the newest
input. GPU observations are asynchronous. Short captures and unpinned device
clocks do not establish long-run p99 or thermal stability. The original readback
mapping failure, 2 Hz / 8 ms prediction device loss and navigation freeze remain
outside this qualification.
