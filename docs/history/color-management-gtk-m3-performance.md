# GTK proofing performance qualification

2026-09-16. This supplements the [implementation/acceptance record](color-management-gtk-m3-validation.md).
Hardware, fresh parent baseline and executable identities are recorded there.
All measurements below use actual GTK, the Vulkan renderer and an isolated
3840×2160 120 Hz Wayland output at 200% scale. Documents are retained ProPhoto U16
photographs with 32 paint layers and five full-resolution pointwise adjustments.
Each navigation run requests 960 poses on an absolute 120 Hz schedule over eight
seconds: fit, half, native, double and continuous zoom with pan/rotation.

The accepted target remains smooth sustained 120 Hz, with latency reported,
not input-to-photon ≤8.33 ms. Input latency here is software request to compositor
presentation feedback. Hardware input-to-photon and calibrated output are unmeasured.
Dirty-image regeneration/resolution-aware previews remain deferred. A photo
already has a complete display composition before the timed sequence; every run
asserts zero camera-only recomposition/source decoding and unchanged artwork.

## Fresh baseline and final ordinary workloads

Baseline source: `7d2511e5d44e841975f82dec6a42c57b52506e31`.
Final production source: `53daf782ed36e1edb4c6a476af6dcf5eaf9f869a`.
The fixed M2 program baseline, including previously accepted startup/latency
limits, remains in [the M2 record](color-management-gtk-m2-performance.md#final-gtk-navigation-qualification--2026-09-15).
The final proof workload uses the recorded Krita CMYK profile, relative intent,
BPC and black-ink simulation (65³ cache). Preparation and a cached off/on toggle
finish before the navigation timer starts. Normal/baseline first-use figures
therefore include different initial UI settling; do not claim the proof's shorter
first response is an optimization.

All times in milliseconds. Poses count distinct requested poses, not repeated
frames; missed slots are measured from feedback, rounded by the output period.

| Workload | Poses / 960 | Missed slots | First response | Request→present p95 / p99 | Worker CPU p95 / p99 | Worker GPU p95 / p99 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 24 MP baseline | 958 | 0 | 26.660 | 9.788 / 9.951 | 0.313 / 0.347 | 0.248 / 0.288 |
| 24 MP normal | 957 | 0 | 34.329 | 8.861 / 9.099 | 0.324 / 0.375 | 0.250 / 0.320 |
| 24 MP proof | 960 | 0 | 8.327 | 8.326 / 8.415 | 0.341 / 0.366 | 0.277 / 0.320 |
| 45 MP baseline | 927 | 0 | 24.800 | 7.970 / 15.762 | 0.341 / 0.394 | 0.204 / 0.278 |
| 45 MP normal | 957 | 0 | 33.627 | 8.404 / 8.639 | 0.338 / 0.392 | 0.214 / 0.284 |
| 45 MP proof | 960 | 0 | 10.097 | 9.920 / 10.080 | 0.321 / 0.386 | 0.233 / 0.281 |
| 60 MP baseline | 957 | 0 | 34.729 | 9.538 / 9.637 | 0.325 / 0.406 | 0.242 / 0.331 |
| 60 MP normal | 957 | 0 | 34.628 | 9.331 / 9.409 | 0.321 / 0.375 | 0.224 / 0.287 |
| 60 MP proof | 960 | 0 | 10.342 | 9.924 / 10.206 | 0.354 / 0.392 | 0.251 / 0.341 |
| 61 MP baseline | 957 | 0 | 32.998 | 7.838 / 7.906 | 0.329 / 0.362 | 0.219 / 0.295 |
| 61 MP normal | 956 | 0 | 35.625 | 10.284 / 10.374 | 0.346 / 0.386 | 0.239 / 0.302 |
| 61 MP proof | 960 | 0 | 9.624 | 9.503 / 9.624 | 0.316 / 0.398 | 0.228 / 0.316 |

All final ordinary size runs have zero missed refresh slots. Proof presents all
3,840 requested poses; the normal runs omit only initial fit poses. The fresh
45 MP baseline also has 33 unmatched poses despite no empty refresh slots;
repeated presentation is not evidence that every gesture was displayed.

## Latency investigation and repeats

The initial 61 MP normal p99 rises from 7.906 to 10.374 ms, so it was not dismissed
as being below the frame budget. Three serial baseline/normal/proof repetitions
use `LAYER_NAVIGATION_PHASE_NS=2000000`. This is a nominal offset against the
app's predicted clock, not an independently controlled physical input phase.

| Repeat | Baseline p95 / p99 | Final normal p95 / p99 | Final proof p95 / p99 | Baseline / normal / proof missed slots |
| --- | ---: | ---: | ---: | ---: |
| 1 | 9.641 / 9.750 | 15.101 / 15.374 | 10.855 / 11.021 | 0 / 0 / 0 |
| 2 | 7.033 / 7.283 | 9.912 / 10.048 | 10.534 / 10.618 | 0 / 1 / 0 |
| 3 | 10.824 / 10.929 | 10.552 / 10.697 | 10.904 / 11.031 | 0 / 5 / 0 |

Proof presents 2,880/2,880 poses in these repeats, with no missed slots. Normal
repeat 1 has a 15.374 ms p99; repeat 3 has five missed slots. These outliers remain
part of the record. Request→enqueue p99 varies from 2.865–3.947 ms in baseline,
3.097–7.951 ms in normal, and 3.159–7.350 ms with proof. The largest delay occurs
before renderer execution; the worker CPU/GPU distributions do not explain it.
Only one normal repeat exceeds the 0.2 ms worker-CPU regression trigger, and that
increase does not reproduce in the other two. Ordinary size-run CPU/GPU p95/p99
increases are below 0.2 ms. No timing-policy change was made on this evidence.

The measurements establish smooth proof navigation on the qualified workload,
not a causal explanation or elimination of the pre-existing phase/startup latency
variation. A tighter latency guarantee and zero misses under arbitrary system
load are not claimed. The M2 fixed baseline documented 6.295–11.519 ms p99 and
14.304–36.201 ms first-use response; retain those historical limits rather than
replacing them with a single favorable run.

## Largest cache, cold preparation and memory

The native 129³ squared-grid stress case uses ProPhoto with CMYK saturation,
BPC off and ink simulation off. It passes with 959/960 poses, the one omission
in initial fit, **zero missed slots**, first response 9.837 ms, request→present
p95/p99 9.701/9.830 ms, CPU 0.388/0.561 ms and GPU 0.320/0.409 ms.

Ordinary native cold readiness is 345–360 ms; the refined shadow case is
3,699 ms, including rejected smaller candidates, validation and GPU upload.
Warm off/on comparison reuses the identical CPU cache and GPU storage; observed
completion is 21–22 ms with **20 ms polling**, not a submillisecond timing claim
or a physical presentation-latency measurement. The independent transform-only
420-case qualification spans 87–4,166 ms cold.

Steady samples occupy **5.24 or 40.95 MiB on each CPU and GPU**. The derivation
budget is 128 MiB CPU (active/pending/scratch) and 96 MiB GPU (atomic replacement),
excluding document-owned profiles and driver overhead. There is no full-resolution
proof copy. Tag aliases and original-stage memory are admitted before expensive
preparation; one worker per window prevents an unbounded queue. Diagnostics include
the resident proof buffer. GPU recovery reuses the CPU cache.

Process measurements include the entire application and driver. Native proof
readiness RSS for 24/45/60/61 MP is 744/999/1,180/1,193 MiB respectively; the large
cache is 1,292 MiB. `/usr/bin/time -v` whole-run maxima are 728/985/1,170/1,178 MiB
and 1,266 MiB for the shadow run. `/proc` RSS and wait4 high-water are different
sampled accounting observations on this host; retain both raw records, and do not
attribute their differences or allocator reuse entirely to proofing. Normal-run
maxima are 736/973/1,155/1,170 MiB. These are bounded qualified workloads, not
measurements of constrained/unified-memory hardware.

## Drawing and concurrent delivery

Twelve 800 ms size-720 palette-knife contacts on a 4096² ProPhoto U16 document
with 32 paint layers overlap exact raster backing. All commits, history and
host-backed publications pass. No per-stroke proof preparation occurs.

| Arm | Worker CPU p95 / p99 ms | Worker GPU p95 / p99 ms | Max pen-up→commit ms | Max pen-up→host-backed ms | Whole-run RSS high-water MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| Baseline | 1.030 / 1.210 | 1.194 / 1.409 | 0.182 | 16.440 | 445 |
| Final normal | 1.093 / 1.284 | 1.259 / 1.510 | 0.180 | 16.438 | 449 |
| Final proof | 1.040 / 1.287 | 1.215 / 1.535 | 0.179 | 16.434 | 494 |

These increases stay below the declared max(5%, 0.2 ms) trigger. First presentation
at/after each committed frame has maximum latency 9.792/10.067/8.168 ms for the
three arms. Some terminal frames are superseded by a subsequent frame, so this
is not an assertion that every terminal frame was presented independently.
Backing observation is sampled at 2 ms; it is not exact disk persistence latency.

The concurrent case overlaps the same 61 MP navigation with atomic native save,
reopen/profile verification, and a full-resolution RGB U16 TIFF export on the
production snapshot/export path. It produces a 482 MB TIFF and 290/291 MB native
master. Save completes in 157/172 ms; save+reopen+export in 6.504/6.611 seconds
(normal/proof). Both present 956/960 poses; missed slots are **2/4** over eight
seconds. Request→present p95/p99 is 8.460/8.696 ms normal and 9.987/10.268 ms
proof. Worker CPU p99 is 2.812/3.129 ms; GPU p99 0.423/0.471 ms. Process high-water
is 1,605/1,613 MiB. Brief export contention remains visible; this is not a zero-miss
concurrent-delivery claim. Export pixels do not contain proof/warning overlays.

## Reproduction and artifacts

Raw reports, compositor logs, console output, time records and derived summaries
are under `artifacts/color-m3/{baseline,final-performance}/`. The native runner
isolates settings/workspace/recovery and leaves the user's app intact. Build first;
do not compile or run a second GPU workload during timing.

```sh
# See acceptance record for preserved build identities.
LAYER_TEST_MONITOR=3840x2160@120 LAYER_TEST_SCALE=2 \
 LAYER_NAVIGATION_PHOTO=61mp LAYER_NAVIGATION_COMPLETE=1 \
 LAYER_NAVIGATION_MAXIMIZE=1 GSK_RENDERER=vulkan \
 LAYER_BENCH_PROOF=/usr/share/color/icc/krita/cmyk.icm \
 /usr/bin/time -v -o artifacts/color-m3/final-performance/proof-61mp-time.txt \
 bash tools/performance/gtk-raster.sh artifacts/color-m3/final-performance/gtk-tests \
 native_large_photo_navigation artifacts/color-m3/final-performance/proof-61mp
python3 tools/performance/photo-navigation-report.py artifacts/color-m3/final-performance/proof-61mp.json
```

Repeat with 24mp/45mp/60mp; unset `LAYER_BENCH_PROOF` for normal. Use the preserved
baseline executable for parent measurements. Add `LAYER_NAVIGATION_PHASE_NS=2000000`
for phase repeats or `LAYER_NAVIGATION_CONCURRENT=1` for save/export overlap.
Use filter `native_penup_and_following_strokes` for drawing. For the largest cache,
add `LAYER_BENCH_PROOF_SHADOW=1` and use `gtk-shadow-tests` (the only difference is
that optional benchmark policy). Its SHA-256 is
`ea4169e5d816b0987ce666e01c8d88b35062229d75f2f4891ebd945b2b89d4e0`.
The serial workload lists are retained as `run.py` and `run-extra.py` in the
artifact directory. This evidence is GTK-only; other-platform and physical-print
qualification remain outside this handoff.
