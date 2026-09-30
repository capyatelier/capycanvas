# Responsiveness

[Performance targets](../PERFORMANCE_TARGETS.md)

These interactions produce one change rather than continuous motion, so they are
measured as latency. The limits are the same on every tier unless a cell says
otherwise.

| Interaction | Limit | Low | Mid | Top |
| --- | --- | --- | --- | --- |
| Pen down → first submitted canvas update | 2 frames: 33 / 22 / 17 ms | **Initial contact not met:** 31.9–43.2 ms to submit; resumed-contact medians 21.0–23.3 ms (release, 12 MP G-Pen 1024, 2026-09-29; comparison below) | | **Not met.** 29.4–40.7 ms across three 5 s G-Pen strokes after quick color changes on the 61 MP photo; 2048 px, release build (2026-09-27). Navigation → first pen submission 6.7–11.0 ms (2026-09-22) |
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

## Supplemental cleanup comparison

Measured on Huion KP1202 on 2026-09-29: benchmark release APKs, 4248 × 2832,
G-Pen 18 px, prediction enabled, default workspace with Navigator, OS-injected
240 Hz stylus samples, three warmed five-second strokes per build.
Baseline source is `ea7d5a23`; APK SHA-256 is
`cdba34b28d2ebef62a52e28f18055b8a2b0be2c09ebca0839f2d237cff95e99b`.
The unread-metadata cleanup APK SHA-256 is
`a59fa1c30bf260f8edec0bbf0354bfd342d55f9ba2eaab6a55e0ee717342072b`.

| Metric | Baseline | Cleanup |
| --- | ---: | ---: |
| Median fresh completed canvas updates/s | 231.4 | 230.7 |
| Median per-run p99 completion gap | 9.41 ms | 9.70 ms |
| Input to GPU completion p99 | 14.48 ms | 13.49 ms |

A repeated baseline reached 232.1 fresh updates/s; the repeated cleanup reached
231.1, with a 9.58 ms p99 completion gap. Owner CPU p99 varied from
3.58 to 4.34 ms across baseline batches; cleanup was 3.98–4.05 ms. The samples
show no change beyond the observed run spread. These completed updates do not
measure display cadence or physical pen latency, and this small brush on Huion
does not qualify any reference-tier brush target.

## Resuming during refinement

Measured on 2026-09-29 on the TCL: 12 MP Perceptual reference photo at Fit,
G-Pen 1024 px, 250 × 140 px path and default workspace with Stats closed
and Navigator visible.
The optimized release build queues fresh artwork behind at most one two-page
refinement batch. APK SHA-256
`649f12d58147542a5a8925d7727d11907ceeafb68a7e68b135b3ef9539ca1d7f`.
Required raster work retains submission backpressure.
Each of three runs draws 25 contacts, lasting 100 ms each. Gaps of 100 ms leave
72 of 75 contacts arriving during refinement; 1,100 ms gaps leave none.
The comparison pairs those 72 resumed contacts with the same contact indices
in the settled runs. Initial contacts are excluded. Values span the three run
medians; thermal status remains zero.

| Contact latency | During refinement | After refinement |
| --- | --- | --- |
| Input queue | 1.76–4.24 ms | 0.17–0.19 ms |
| First canvas submission | 21.04–23.29 ms | 17.58–19.43 ms |
| GPU completion | 62.05–63.47 ms | 46.37–48.09 ms |

Paired run medians add 3.38 / 5.71 / 2.58 ms to submission and
16.02 / 16.02 / 13.96 ms to GPU completion. Added submission p95 is
6.05 / 6.31 / 2.79 ms; added GPU-completion p95 is
16.21 / 20.40 / 14.28 ms. Submission passes the one-frame 16.67 ms refinement
budget in all three runs; GPU completion exceeds it in one run. The preceding
four-page build added 23.16 / 20.57 / 15.22 ms to GPU-completion p95, exceeding
that budget in two runs. These metrics do not measure physical pen-to-photon
latency.

Initial contacts take 31.94–39.08 ms to submit in the fast-gap runs and
33.80–43.22 ms in the settled runs; one fast-gap and all three settled initial contacts exceed
the separate 33 ms target. Raw records are in
`artifacts/latency-investigation/qualified-25-brush-{fast,slow}`;
`qualified-25-pauses-matched-resume.json` includes every run and initial contact.
The previous build's records are `settle-21-brush-{fast,slow}` and
`settle-21-pauses-matched-resume.json`.

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


### Immediate input after pen-up

The delayed probe above misses work starting at pen-up. The current benchmark
injects its pinch as soon as the pen-up injection returns, with no intentional
sleep or synchronous owner query first. Callback maxima include callbacks that
start before the navigation window and overlap its beginning. The pending-work
flag sampled during navigation includes camera work; it alone does not prove
that Healing is pending. The shared and native journeys separately assert that
navigation and queued painting occur before healed raster publication.

A matched MovinkPad 11 Clone diagnostic found a 94–98 ms presentation callback:
changing from shared presentation to FIFO synchronously drained outstanding GPU
work. The Android host now uses its existing completion counter to defer the
mode switch, returning to input processing until the preceding frame completes.
Three immediate probes then had 20–29 ms callback maxima and navigation queue
p95 of 16.1–18.5 ms, versus 38.5–46.6 ms before. The two-second controls remained
at 16.0–16.8 ms queue p95. Total measured owner CPU time across each one-second
pinch was 180–203 ms, versus 193–204 ms before. These are input-service
measurements, not physical pen-to-photon latency.

Thumbnail preparation also bypassed its budget for ordinary paint layers and
painted overrides of photos. Both passes now share the four-page preparation
budget; each native-host poll advances one request. Completed original-photo
contributions remain shared. Maximum thumbnail-query time in the matched
mid-tier Clone traces fell from 89.5 ms to 5.5 ms. The low-tier immediate probes
peaked at 12.5 ms; a settled control reached 21.4 ms. Pixel resolution and
integration are unchanged. This bounds preparation work, not an absolute
wall-clock guarantee on every callback.

Immediate Healing exposed a separate eager allocation of every destination
page at pen-up. Output companions now allocate within the existing eight-page
Apply batches. In three mid-tier runs, navigation queue p95 fell from
45.9–53.4 ms to 15.6–17.8 ms and the maximum fell from 107.5 to 27.3 ms.
Low-tier Healing queue p95 was 18.6–21.0 ms, maximum 33.4 ms. Mid-tier initial
callback maxima were 34.8–59.0 ms, including solver-buffer allocation; low-tier
maxima were 23.1–25.3 ms. Tool round trips reached 120 ms on TCL, so the separate
100 ms tools target remains unmet. Dependent paint still waits for publication.

Spot Healing candidate gathering allocated 64 MiB of companion textures in one
batch. Reducing that existing batch from 32 pages to eight preserves its pixel
work and limits each allocation batch to 16 MiB. Three mid-tier immediate probes
then had navigation queue p95 of 15.9–18.3 ms, versus 25.5–29.9 ms with the
destination-allocation fix alone. Maximum queue delay was 35.3 ms and maximum
overlapping callback was 52.5 ms; initial solver-buffer allocation remains.
Low-tier queue p95 was 14.0–20.3 ms, maximum 43.6 ms, with a 29.2 ms maximum
callback. Tool round trips still reached 117.1 ms on TCL. Gathering has the same
number of pixels and candidate evaluations, with more submission boundaries;
this is an interruption improvement, not a reduction in solver work.

These 2026-09-29 runs use the same 512 px, Perceptual, Fit workloads as above.
Clone records are in `artifacts/latency-investigation/{thumbnail-13-mid-0,switch-14-mid-0,switch-14-mid-2000,switch-14-low-0,switch-14-low-2000}`;
Healing records are in `heal-15-{low,mid}-healing`. The destination-allocation
build's optimized APK SHA-256 is
`3cf1dcd937b7ef4e1362b7dac2bc14c7c6e106f60682af38bfec0b7a55c18f80`.
The first mid-tier pinch's screen timestamps are absent from the retained trace;
those runs provide input/callback data, not presentation qualification. Remaining
one-second pinch records do not replace three sustained navigation gestures.
The eight-page gathering runs are in `heal-17-{low,mid}-healing`, optimized APK
SHA-256 `66cbb5126ab2ec80be2d0714ebac06b45615ea715fee6d2e21233128f3c1a1a6`.
