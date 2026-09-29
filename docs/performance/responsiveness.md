# Responsiveness

[Performance targets](../PERFORMANCE_TARGETS.md)

These interactions produce one change rather than continuous motion, so they are
measured as latency. The limits are the same on every tier unless a cell says
otherwise.

| Interaction | Limit | Low | Mid | Top |
| --- | --- | --- | --- | --- |
| Pen down → first submitted canvas update | 2 frames: 33 / 22 / 17 ms | **Initial contact not met:** 35.0–52.8 ms to submit; resumed-contact medians 18.3–22.7 ms (release, 12 MP G-Pen 1024, 2026-09-29; comparison below) | | **Not met.** 29.4–40.7 ms across three 5 s G-Pen strokes after quick color changes on the 61 MP photo; 2048 px, release build (2026-09-27). Navigation → first pen submission 6.7–11.0 ms (2026-09-22) |
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
G-Pen 1024 px, 250 × 140 px path and default workspace with Stats closed.
The build shares decoded sources and includes the drag-cache fixes
(APK `89ad253df04ed082393f9b134766427b49f133da3794f34c990b855394879926`).
Each of three runs draws 25 contacts, lasting 100 ms each. Gaps of 100 ms leave
72 of 75 contacts arriving during refinement; 1,100 ms gaps leave none.
The comparison pairs those 72 resumed contacts with the same contact indices
in the settled runs. The initial contact in each run is excluded from this
interruption comparison. Values are ranges of the three run medians.

| Contact latency | During refinement | After refinement |
| --- | --- | --- |
| Input queue | 1.11–1.50 ms | 0.20–0.21 ms |
| First canvas submission | 21.67–22.69 ms | 18.34–19.26 ms |
| GPU completion | 59.82–61.48 ms | 53.49–54.59 ms |

Paired run medians add 3.33 / 3.10 / 3.44 ms to submission and
7.77 / 7.44 / 5.22 ms to GPU completion. Submission p95 is 27.90–31.42 ms,
versus 24.44–28.25 ms after settling. The added p95 delays are 3.17–3.69 ms
for submission and 10.70–15.48 ms for GPU completion. All these differences
are below one 60 Hz frame. This establishes ordinary G-Pen interruption for
this workload; it does not qualify other operations or physical pen-to-photon
latency. Thermal status remains zero.

The excluded initial contacts take 35.01–52.82 ms to submit in these runs and
miss the separate 33 ms first-submission target. That initial cost also occurs
without pending refinement. Raw records are in
`artifacts/latency-investigation/stable-brush-{fast,slow}` and the matched
summary is `stable-pauses-matched-resume.json`.

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
