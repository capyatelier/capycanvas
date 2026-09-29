# Responsiveness

[Performance targets](../PERFORMANCE_TARGETS.md)

These interactions produce one change rather than continuous motion, so they are
measured as latency. The limits are the same on every tier unless a cell says
otherwise.

| Interaction | Limit | Low | Mid | Top |
| --- | --- | --- | --- | --- |
| Pen down → first submitted canvas update | 2 frames: 33 / 22 / 17 ms | **Initial contact not met:** 37.9–51.3 ms to submit; resumed-contact medians 20.1–23.2 ms (release, 12 MP G-Pen 1024, 2026-09-29; comparison below) | | **Not met.** 29.4–40.7 ms across three 5 s G-Pen strokes after quick color changes on the 61 MP photo; 2048 px, release build (2026-09-27). Navigation → first pen submission 6.7–11.0 ms (2026-09-22) |
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
The optimized release benchmark includes bounded Healing and native raster
publication (APK
`e145c6cf350d3e8f1e4fbffd0c8d47aa72c42e0de489c73a4174ef45fb30a18a`).
Each of three runs draws 25 contacts, lasting 100 ms each. Gaps of 100 ms leave
72 of 75 contacts arriving during refinement; 1,100 ms gaps leave none.
The comparison pairs those 72 resumed contacts with the same contact indices
in the settled runs. Initial contacts are excluded. Values span the three run
medians; thermal status remains zero.

| Contact latency | During refinement | After refinement |
| --- | --- | --- |
| Input queue | 0.96–2.23 ms | 0.19–0.20 ms |
| First canvas submission | 20.10–23.15 ms | 17.77–18.94 ms |
| GPU completion | 57.64–63.74 ms | 52.01–54.14 ms |

Paired run medians add 2.09 / 5.38 / 1.68 ms to submission and
5.63 / 10.85 / 5.55 ms to GPU completion. Submission p95 is 26.68–32.89 ms,
versus 23.19–26.19 ms after settling. Added submission p95 stays below one
60 Hz frame. Added GPU-completion p95 is 8.74 / 20.56 / 8.67 ms: **the second
run exceeds the one-frame refinement budget**. These observations establish
continued input service, not physical pen-to-photon latency or a universal
one-frame bound.

Initial contacts take 37.92–51.30 ms to submit during these fast-gap runs and
41.01–46.32 ms in the settled runs, missing the separate 33 ms target.
Raw records are in `artifacts/latency-investigation/heal-brush-{fast,slow}`;
`heal-pauses-matched-resume.json` includes the paired results and initial contacts.
The preceding drag-cache build's results remain in
`stable-pauses-matched-resume.json`.

### Healing finalization

Healing advances through bounded GPU batches, including native raster validation
and conversion. Navigation presents the existing composition. Tools remain
selectable; dependent paint contacts wait in order with their original settings
and camera coordinates. The completed heal publishes one raster revision and one
undo step. This does not promise a new paint mark before its source is ready.

Measured on 2026-09-29: TCL 12 MP and MovinkPad 11 24 MP reference photos,
Perceptual, Fit, default workspace, Stats closed, thermal status zero. Each brush
has three five-second 512 px strokes on a 400 × 240 px ellipse. After pen-up,
the runner waits 100 ms, switches tools, queues a short G-Pen contact and injects
one second of pinch navigation. Every run starts that probe with finalization
pending; every sampled camera revision advances. Values below span the three
runs. Callback maxima include all work from pen-up through final publication.

| Device / brush | Navigation queue p95 | Maximum navigation queue | Maximum settle callback | Pen-up through completion, including probe |
| --- | --- | --- | --- | --- |
| TCL / Healing | 18.5–19.4 ms | 45.0 ms | 58.7 ms | 3.43–3.54 s |
| TCL / Spot Healing | 15.8–18.5 ms | 38.0 ms | 44.2 ms | 4.25–4.37 s |
| MovinkPad 11 / Healing | 17.3–18.4 ms | 23.7 ms | 36.4 ms | 3.35–3.47 s |
| MovinkPad 11 / Spot Healing | 15.2–18.4 ms | 28.8 ms | 51.7 ms | 5.86–6.01 s |

Tool-action round trips, including the owner barrier and main-thread publication,
range from 8.1–118.6 ms on TCL and 9.0–98.6 ms on MovinkPad 11. They are not
physical tap-to-photon measurements. TCL still has a cold tool-layout outlier
above the separate 100 ms response target. Brush previews now decode off the UI
thread and retain a bounded cache across panel changes. Android production
optimization is enabled in these runs; the unminified test build has larger UI
layout delays.

Actual screen presents during the pinch are 54.9–57.0/s on TCL and 56.5–58.6/s
on MovinkPad 11. Their median intervals are 16.7 ms; individual maximum gaps
reach 83.3 and 50.3 ms respectively. These short interruption probes establish
continued presentation, not the sustained navigation target. The Mid panel stays
at 60 Hz, below its 90 Hz tier target. Minimum system available memory across
warm-up, strokes, finalization and undo is 2,198 MiB on TCL and 1,358 MiB on Mid.

The release benchmark APK SHA-256 is
`e145c6cf350d3e8f1e4fbffd0c8d47aa72c42e0de489c73a4174ef45fb30a18a`.
Raw records are in `artifacts/latency-investigation/heal-staged-5-{low,mid}`;
`heal-staged-5-summary.json` includes every run and outlier. GTK native pen/touch,
Web Healing, and Android mouse/touch/pen journeys also check output and undo.
Apple/Windows journeys and physical stylus-to-scanout latency are unverified.

The preceding monolithic Mid diagnostic queued the next contact for 1,001 ms
with Healing and 1,728 ms with Spot Healing. First GPU completion took 1,354 and
2,194 ms. Separate phase traces measured 438–468 ms in relaxation,
196–204 ms in gathering/seeding, 47 ms in the pyramid and 76–85 ms in application;
Spot Healing added 479–599 ms of candidate search. These are real pixel-work
costs, so interruptible batches retain the solver's resolution and fixed sweeps.
Scheduling increases elapsed settling time while allowing navigation between
batches; it does not reduce the solver's operation count. The baseline diagnostic
is in `artifacts/latency-investigation/mid-heal-interrupt`.
