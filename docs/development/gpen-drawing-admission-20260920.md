# G-Pen taper and Android drawing qualification

The accepted G-Pen uses a 34,133 µs pressure-fall limit with a separate 4 ms
smoothing response. Input positions, timestamps and stored pressure remain
unchanged; rising pressure is immediate. Preview and final ink use the same
endpoint flush, and varying-radius contact sweeps preserve the taper edge.
The limiter applies to pressure tools, not mouse, touch or eraser input.

The black-flash fix preserves valid retained viewport contents with `Load`,
including full camera redraws. The user confirmed the physical Wacom flashes
were gone. Initial targets still clear; shared presentation can still tear.

The drawing regression originated in `f458d13e`: a one-update completion gate
serialized CPU/GPU work. Allowing two updates alone made throughput worse:
502 renderer updates produced only 69 submissions, with 433 acquisition
timeouts after raster work had already been submitted. The HAL now skips the
ordinary acquire-semaphore reuse wait only for an already-acquired shared
image. Present-slot fences and queue ordering remain intact. Android bounds
outstanding updates at two using one surface-owned completion counter.

## Measurements

Wacom `5ll21u1002931`, primary app `art.capycanvas`: visible 9504×6336 photo,
hidden Paper/prior ink, fresh paint layer, opaque black 2048 px G-Pen,
17.164% zoom. The existing `AndroidPenMotion` harness injects pressure-1 stylus
samples at 200 Hz around an ellipse centered at (1410,948), radii (520,299),
one loop/s. Short workflows include hover, a ten-second stroke, undo and redo.

| Run | Canvas latches/s | Owner CPU / latch |
| --- | ---: | ---: |
| Historical `2f69114d`, light probe | 35.99 | 20.97 ms |
| Review `421049b4`, paired full probe | 12.50 | 40.76 ms |
| Corrected admission, paired full probe | 41.09 | 20.27 ms |
| Corrected admission, 180-second light probe | 46.73 | 17.01 ms |
| Final installed APK, light probe | 42.60 | 18.00 ms |

The paired gain is 3.29×. Every ten-second sustained window reaches 44.0–47.8/s,
above the 30/s target. These traces retain all actions with no parser errors.
Final hover reaches 116–117/s; undo/redo first latch takes 342/521 ms.
Composition CPU falls from 10.61 to 3.56 ms/update; command-buffer allocations
fall from 201.2 to 96.5/update. Observed GPU raster intervals fall from 48.14 to
18.64 ms, with composition still the largest phase. GPU intervals include
submission gaps and are not exclusive hardware occupancy. Latches establish
compositor consumption, not physical pen-to-photon latency.

Remaining limits: occasional ~200 ms frame gaps, ~0.3–0.52 s history response,
and the existing 1 GiB serialized-raster limit. Accumulated benchmark layers
triggered that recovery limit during one deployment attempt, blocking later
actions; those layers were removed and the full workflow passed again.

## Validation and retained evidence

The deployed runtime passed 206 core/engine/host tests, two retained/full
viewport GPU comparisons, seven release/contact GPU tests and the Wacom
front-buffer lifecycle test (rotation, recreation, GPU recovery, exact artwork
undo/redo). A slow-stroke video confirms progressive visible ink; full and
sustained screenshots pass all 39,600 opaque stroke-interior samples.
The final app also completed the existing 12-second zoom/rotation replay.

Keep the captured `wacom-rapid-lift.csv` fixture and engine/GPU regressions:
they check batching, prediction, cancellation, raw-pressure preservation,
preview/final agreement, smooth fall, swept edges and one-step history.
`AndroidPenMotion` and `android-pen-report.py` reproduce the action-tagged
performance checks.

Raw APKs, traces, profiles, screenshots and harnesses remain under
`artifacts/wacom-zoom-flash-20260920/` and `artifacts/wacom-gpen-taper-20260920/`.
Superseded reports, probes and measurement dumps are archived locally as
`merged-review-evidence.tar.gz` in the former directory and preserved at
qualification commit `5b3942af`. They are not part of the maintained source tree.

Qualified native source: `e2716ff6`; APK SHA-256:
`56726e2151d0423a4ab6d3f6f30db19760b0242c3f9f71057c68efc582492df0`.
The installed APK matched that hash; only `art.capycanvas` remained installed.
The unminified lifecycle-test build had byte-identical native code.
