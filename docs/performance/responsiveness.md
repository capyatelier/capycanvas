# Responsiveness

[Performance targets](../PERFORMANCE_TARGETS.md)

These interactions produce one change rather than continuous motion, so they are
measured as latency. The limits are the same on every tier unless a cell says
otherwise.

| Interaction | Limit | Low | Mid | Top |
| --- | --- | --- | --- | --- |
| Pen down → first submitted canvas update | 2 frames: 33 / 22 / 17 ms | Input queueing 5–13 ms (debug build; `68820ad8`, 2026-09-26) | | **Not met.** 29.4–40.7 ms across three 5 s G-Pen strokes after quick color changes on the 61 MP photo; 2048 px, release build (2026-09-27). Navigation → first pen submission 6.7–11.0 ms (2026-09-22) |
| Tap or press → visible response: buttons, tools, menus | 100 ms | | | |
| Transform or placement press → first moving frame | 100 ms | | 22 ms for a selection Distort, 47 ms for a photo handle (`debff77d`, `fe605aa3`, 2026-09-27) | |
| Undo or redo of one 1024 px stroke | 250 ms | | | |
| Magic Wand or Color Select on the tier canvas | 250 ms | | | |
| Tonal selection, warm mask on the tier canvas | 350 ms | | Huion: 244–266 ms on 61 MP ([tonal performance](../internals/tonal-performance.md), 2026-09-24) | |
| Filter preview after a parameter change | 100 ms p95 | | | |
| Command search open or query → drawn | 50 ms p95 | | Huion: 20.5 ms (`9b830b42`, 2026-09-25) | |
| Warm launch → canvas ready for a stroke | 2 s | | Huion: workspace ready 1.72–1.80 s ([shader readiness](../internals/shared-shader-readiness.md), 2026-09-25) | |
| Cold launch with an empty shader cache → ready | 10 s | About 18 s in instrumented runs | Huion: 3.50 s to all shaders; workspace 2.34 s ([shader readiness](../internals/shared-shader-readiness.md), 2026-09-25) | |
| Open the tier photo → first frame | 3 / 4 / 6 s | | | |

## Resuming during refinement

Measured on 2026-09-29 on the TCL: 12 MP Perceptual reference photo at Fit,
G-Pen 1024 px, default workspace with Stats closed. The renderer is `c99e6dfd5`
(APK `dd261bdf2e97af8b11c7dc73f067e44e4ff2277127528c9a352c689e2028898a`).
Each of three runs draws the same 25 contacts, lasting 100 ms each. Gaps of
100 ms leave 72 of 75 contacts arriving during refinement; 1,100 ms gaps leave
none. Values below are the range of the three run medians.

| Contact latency | During refinement | After refinement |
| --- | --- | --- |
| Input queue | 1.12–3.07 ms | 0.20–0.21 ms |
| First canvas submission | 21.33–24.61 ms | 18.22–18.65 ms |
| GPU completion | 58.92–62.94 ms | 52.03–53.76 ms |

The paired run medians add 2.78 / 4.82 / 5.96 ms to submission and
6.89 / 10.01 / 9.19 ms to GPU completion during refinement. All are below one
60 Hz frame. This establishes ordinary G-Pen interruption for this workload;
it does not qualify other operations or physical pen-to-photon latency.
Pending-contact submission p95 is 29.48–32.89 ms, versus 24.35–26.59 ms after
settling. Raw records are in `artifacts/latency-investigation/pauses-before-*`.

### Healing finalization

A separate Mid-tier diagnostic with the shared decoded-source cache, 24 MP
Perceptual photo at Fit, 512 px brushes and Stats closed resumes drawing 100 ms
after a five-second stroke. The second contact waits 1,001 ms in the input queue
for Healing and 1,728 ms for Spot Healing; first GPU completion takes 1,354 and
2,194 ms respectively. **This fails the interruption goal.** The two-contact
checks are diagnostic, not three-run qualification. Records are under
`artifacts/latency-investigation/mid-heal-interrupt`.

With Stats and tracing enabled separately, Healing spends 438–468 ms in GPU
relaxation, 196–204 ms in gathering/seeding, 47 ms in the pyramid and 76–85 ms
in application. Spot Healing adds 479–599 ms in candidate search. Finishing
these operations within one frame requires an algorithm or scheduling change;
reducing display-refinement batch size cannot remove this stroke-finalization
work. The Android trace also shows blocking `QueuePresentKHR` calls behind this
work. GPU completion observations include callback service and are not scanout.
