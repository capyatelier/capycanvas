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
| Warm launch → canvas ready for a stroke | 2 s | **Not met.** Android median 7.50 s to selected-brush readiness ([idle preparation](#idle-brush-preparation), 2026-10-08) | Huion: workspace ready 1.72–1.80 s ([shader readiness](../internals/shared-shader-readiness.md), 2026-09-25) | |
| Cold launch with an empty shader cache → ready | 10 s | **Not met.** Android median 35.29 s to selected-brush readiness ([idle preparation](#idle-brush-preparation), 2026-10-08) | Huion: 3.50 s to all shaders; workspace 2.34 s ([shader readiness](../internals/shared-shader-readiness.md), 2026-09-25) | |
| Open the tier photo → first frame | 3 / 4 / 6 s | | | |

## Idle brush preparation

Measured on the low-tier TCL TAB 11 Gen 2 on 2026-10-07–08 with optimized Android
and `web-release` builds. Native startup uses a fresh private workspace;
brush-selection probes use the 4248 × 2832 reference photo, Perceptual blending, Fit and the
default workspace with Navigator and Stats closed. Selection-to-ready includes
host dispatch and readiness observation, not physical pen-to-photon latency.

Native cold runs clear only the dedicated benchmark application's data and
shader cache. Each following warm run retains that cache and starts a new
process. Chrome runs use a dedicated test origin and foreground tab with hardware
WebGPU; its driver cache cannot be cleared independently without affecting
other sessions. Browser reload measurements therefore do not establish
shader-cold startup.

Initial selected-brush readiness and complete idle preparation are separate
milestones. The latter includes all built-in brush families in the new build;
painting starts at the former. The native initial-cache save remains early,
so a short first session still saves required startup shaders. Later warmed
variants are not added to that saved cache by this change.

| Android startup stage, median of three launches | Cold before | Cold after | Cached before | Cached after |
| --- | ---: | ---: | ---: | ---: |
| Canvas | 6.705 s | 6.700 s | 6.483 s | 6.502 s |
| Document | 20.374 s | 20.375 s | 7.403 s | 7.389 s |
| Selected brush ready | 35.352 s | 35.293 s | 7.516 s | 7.501 s |
| Complete background preparation | 63.464 s | 173.483 s | 7.519 s | 152.702 s |

Initial readiness shows no regression in these samples. The absolute 10-second
cold and 2-second cached-launch targets remain unmet. Full preparation takes
longer because its scope is larger, and its later variants compile again on
relaunch. Native startup records are under
`artifacts/shader-warmup/{baseline,candidate5}/android/startup`.

The Chrome startup comparison uses three reloads per build, bypassing the HTTP
cache and verifying the loaded Wasm hash. Each preceding shader queue finishes
before the next reload. Canvas, document and brush timings use their first
readiness marks; complete preparation refers to the final adopted renderer.

| Web reload stage, median of three | Before | After |
| --- | ---: | ---: |
| UI | 2.430 s | 2.343 s |
| Canvas | 10.263 s | 10.445 s |
| Document | 28.481 s | 28.751 s |
| Selected brush ready | 44.568 s | 44.590 s |
| Complete background preparation | 117.845 s | 191.286 s |

Selected-brush readiness differs by 23 ms in these medians. Browser driver-cache
state remains uncontrolled, and these reloads do not qualify warm-cache or
shader-cold launch targets. Raw captures are in
`artifacts/shader-warmup/baseline/web-startup-matched` and
`artifacts/shader-warmup/candidate5/web-startup`. Earlier normal-reload baseline
captures use a different HTTP-cache policy and are excluded from this comparison.

Android selection-to-ready medians below use three fresh GPU lifetimes per
condition. Each sequence selects G-Pen, Dual Texture, Wet Round, Smudge, Liquify
and Healing, then repeats that order. The baseline immediate sequence contains
only the first pass. These are sequential tool probes, so later tools can reuse
dependencies requested by earlier tools.

| Android tool | Before, immediate first selection | Idle preparation, immediate first selection | Idle preparation, first selection after 30 s |
| --- | ---: | ---: | ---: |
| G-Pen | 30.6 ms | 21.5 ms | 21.1 ms |
| Dual Texture | 7394.0 ms | 6408.6 ms | 4911.2 ms |
| Wet Round | 8626.6 ms | 4398.9 ms | 162.9 ms |
| Smudge | 4733.0 ms | 4759.0 ms | 168.0 ms |
| Liquify | 2986.1 ms | 3049.4 ms | 182.3 ms |
| Healing | 14347.4 ms | 14305.9 ms | 14436.2 ms |

Repeat-selection medians are 102.1–172.1 ms in the immediate batches and
119.7–162.2 ms after idle. A single baseline 30-second idle control still needs
3987.5 ms for Wet Round, 4696.8 ms for Smudge, 3019.8 ms for Liquify and
14434.8 ms for Healing; its repeated Healing selection takes 1507.0 ms.
Common tools benefit from the early idle work, but 30 seconds does not complete
the entire catalogue or remove Healing's first-use delay. Even warmed
selection-to-ready medians can exceed 100 ms; these probes do not qualify the
separate visible-response target.

One additional native lifetime waits for the full catalogue before probing.
First selections take 53.4 ms for G-Pen, 188.9 ms for Dual Texture, 228.2 ms
for Wet Round, 179.4 ms for Smudge, 99.4 ms for Liquify and 233.5 ms for Healing;
repeats take 135.5–213.7 ms. These single observations confirm the eventual
benefit for the later recipes; they are not three-run medians. Records are in
`artifacts/shader-warmup/candidate5/android/full-tools`.

Chrome uses the same sequential probes in three GPU lifetimes per condition,
starting with G-Pen selected. Every retained lifetime verifies the loaded Wasm
hash and bypasses the HTTP cache. Values are selection-to-ready medians.

| Web tool | Before, immediate | After, immediate | Before, after 30 s idle | After, after 30 s idle |
| --- | ---: | ---: | ---: | ---: |
| G-Pen | 131.5 ms | 159.0 ms | 236.0 ms | 150.5 ms |
| Dual Texture | 4099.4 ms | 3984.8 ms | 8328.5 ms | 2788.0 ms |
| Wet Round | 4484.4 ms | 4761.3 ms | 5011.0 ms | 247.4 ms |
| Smudge | 13373.7 ms | 5390.4 ms | 10180.9 ms | 277.4 ms |
| Liquify | 10353.3 ms | 4106.3 ms | 8050.5 ms | 2175.3 ms |
| Healing | 17416.8 ms | 10327.5 ms | 13412.4 ms | 9776.4 ms |

Immediate selection still requests unprepared shaders: Wet Round is 277 ms
slower in these medians, while Smudge, Liquify and Healing improve. After idle,
Wet Round and Smudge need no multi-second wait; Liquify and Healing still do.
Repeat-selection medians are 195.6–243.7 ms immediately and 197.3–282.7 ms
after idle. Baseline Healing repeats take 1066.7 and 368.0 ms respectively.
The browser driver cache remains outside this experiment's control. Raw probe
records are in `artifacts/shader-warmup/{baseline,candidate5}/web-tools-{0,30000}`;
discarded trials with a different initial preset remain separate diagnostics.

In a single fully warmed Chrome lifetime, first selections take 42.3 ms for
G-Pen, 161.0 ms for Dual Texture, 187.6 ms for Wet Round, 208.9 ms for Smudge,
193.2 ms for Liquify and 162.4 ms for Healing; repeats take 137.6–238.8 ms.
The photo's complete preparation takes 136.835 seconds. These probes reuse
the fully warmed motion test's document and GPU context, with the loaded Wasm
hash verified. Records are in
`artifacts/shader-warmup/candidate5/web-full-tools`.

Motion, compile-overlap and memory results are in
[the low-tier table](low-tier.md#idle-brush-preparation). Physical Apple/Windows
startup and reference-tier measurements above the low tier remain unverified.

## Export Again

Supplemental measurements on 2026-10-04 use small unchanged PNG fixtures and
the Export Again build based on `00b2d6e73`. Every repeat produces identical
bytes. Completion includes native input acknowledgement, test settling and file
publication; it is not input-to-visible-response latency or a reference-tier
canvas measurement. Ordinary export starts at destination acceptance, while
Export Again starts at command activation, so these are not encoder speed
comparisons. The action reuses the existing capture and export workers.

| Host and workload | Ordinary export | Export Again |
| --- | ---: | ---: |
| GTK release, NVIDIA RTX PRO 6000 Blackwell Max-Q, 64×48 → 32×24 U16 PNG, both themes at 640 | 294–395 ms | 357–362 ms |
| Chrome 154 / hardware Vulkan on the same GPU, 160×120 PNG, both themes at 640/1100 | 98–128 ms | 125–315 ms |
| Huion KP1202, Android debug, 128×128 PNG, both themes at 533 dp | 567–609 ms | 433–488 ms |

Build SHA-256 prefixes are GTK test executable `f87c415678794486` with local
GTK library `48d36af98003c10a`, Web Wasm `d80f8321457a4245` with file transport
`5c0aba2e7a8d86e5`, and Android APK `c234ae856a267f34`.
Browser destination handles and Android picker results are
controlled fixtures; real encoding, browser output workers and Android provider
writes run. These checks do not qualify large-photo export, external provider
latency or physical pen latency.

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

### Resumed contacts before the batch tradeoff

Measured on 2026-09-30 after the covered-pixel and composition changes at
`c82970142`, with Perceptual blending, default workspace, Stats closed and
thermal status zero. TCL uses the 12 MP photo, G-Pen 1024 px at Fit and a
250 × 140 px path. Three runs each draw 25 contacts of 100 ms. Gaps of 100 ms
put all 72 resumed contacts into pending refinement; 2,000 ms control gaps put
none there. Initial contacts are excluded and the same contact indices are
paired. The two-layer graph is identical in the front-biased and balanced
builds; this run uses the balanced control APK documented in
[the low-tier table](low-tier.md#painting-below-the-front-layer).

| Contact latency | During refinement, run medians | After refinement, run medians |
| --- | --- | --- |
| Input queue | 1.41–2.29 ms | 0.19–0.20 ms |
| First canvas submission | 17.64–21.11 ms | 13.07–15.96 ms |
| GPU completion | 56.45–60.16 ms | 43.43–45.10 ms |

Differences of run medians add 1.68–7.27 ms to submission and 11.36–16.73 ms
to GPU completion. Taking the p95 of the individual paired contact differences
adds 9.30 / 10.86 / 11.22 ms to submission and 20.02 / 21.08 / 27.59 ms to
GPU completion. Submission remains within the one-frame refinement allowance;
the GPU-completion increase exceeds it in all three runs. This percentile
of paired differences is distinct from subtracting the two latency
distributions' p95 values. These measurements are not physical pen-to-photon
latency and do not identify how much comes from exact composition versus
required raster publication.

The current top-tier build also ran three ten-contact sequences: 61 MP photo,
G-Pen 2048 px at Fit, 520 × 299 px path, with the same 100 ms versus 2,000 ms
gaps. Twenty-five matched resumed contacts have median first-submission
latencies of 30.87–44.66 ms during refinement versus 44.06–57.00 ms after
settling; GPU completion is 50.47–68.55 ms versus 101.53–114.50 ms. Both
retain SharedDemandRefresh, and the medians do not establish an added
refinement penalty on this device. The long-gap control has a separate
start-latency gap; its cause is unclassified. The top APK is the optimized
`c452a0642` build identified in [the top-tier table](top-tier.md#current-g-pen-comparison).

A preceding top-tier 25-contact sequence crashed in the second run after
Adreno reported `kgsl_sharedmem_alloc()` failure for 16 KB, with SIGSEGV in
`vulkan.adreno.so` during command-buffer initialization. A repeat of two
25-contact runs with memory sampling passed. The allocation failure's cause
and connection to refinement are unproven; it remains a robustness gap.

Raw records: `artifacts/optimization-roi/all-layer-followup/current-{low,top}-{fast,slow}-{25,10}`
for the corresponding tier, `current-pauses-paired.json`, failed
`current-top-fast`, and diagnostic `current-top-fast-25-memory`.

### Refinement batch tradeoff

Measured on 2026-09-30 with three controlled optimized release APKs, based on
`1557688aa`. All use balanced whole-run composition, the covered-pixel guard,
transparent-paper exclusion, independent batch-completion tokens and the actual
page-strip width. Only the page cap and the fresh-input submission gate differ.
The gated control waits for optional refinement to finish before submitting
fresh artwork; both queueing builds allow it behind at most one unfinished
batch. Required raster backpressure remains in every build.

TCL uses the 4248 × 2832 reference photo, G-Pen 1024 px and a 250 × 140 px path.
MovinkPad 11 uses 6000 × 4000, G-Pen 2048 px and a 479.7 × 299 px path.
Both use Perceptual blending, Fit, 16 ms prediction, default workspace with
Navigator, Stats closed and thermal status zero. Continuous measurements are
three warmed five-second strokes. Contact measurements are three sequences of
25 contacts, each 100 ms with 100 ms gaps. Each run excludes the initial contact;
the table reports the median of the three run medians for the 72 resumed contacts.
GPU completion is a proxy, not physical pen-to-photon latency.

| Tier / build | Fresh updates/s | Settling after continuous stroke, median | Resumed first submission, median | Resumed GPU completion, median | Resumed GPU completion, median run p95 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Low / two pages, queue fresh input | 63.70 | 596 ms | 17.6 ms | 56.6 ms | 76.2 ms |
| Low / four pages, queue fresh input | 63.69 | 496 ms | 19.9 ms | 59.0 ms | 79.3 ms |
| Low / four pages, gate fresh input | 64.00 | 527 ms | 19.7 ms | 57.5 ms | 73.5 ms |
| Mid / two pages, queue fresh input | 47.96 | 903 ms | 26.4 ms | 89.1 ms | 104.2 ms |
| Mid / four pages, queue fresh input | 48.04 | 687 ms | 27.8 ms | 92.4 ms | 103.5 ms |
| Mid / four pages, gate fresh input | 48.01 | 671 ms | 48.5 ms | 112.6 ms | 162.8 ms |

The two-page cap has no continuous-stroke gain. It saves only 2.4 ms on low-tier
and 3.4 ms on mid-tier typical resumed GPU completion while lengthening median
settling by 100 and 216 ms. Four pages are restored. Queueing remains because
it saves about 21 ms of mid-tier submission delay, 20 ms of median GPU completion
and 59 ms of GPU-completion p95 against the four-page gated control, without a
continuous-rate loss. Low-tier queueing differences are within a few milliseconds;
the mid-tier benefit supports the shared policy. This adds no second refinement
batch, cache or layer-dependent policy.

The preceding roughly 20 ms result compares pending refinement with an already
settled renderer. It is not a measured regression introduced by queueing.
Optional exact composition still occupies the GPU queue after pen-up. Its cost
recurs when short strokes resume before settling. These absolute contact latencies
do not qualify the separate added-latency
target, which requires paired pending-versus-settled runs of the selected build.

A separate immediate-after-lift one-second pinch probe records similar display
cadence across the variants: about 54–56 fps on TCL and 48–50 fps on MovinkPad 11.
The selected mid build retains navigation input-queue p95 of 94–109 ms. These
short presentation-traced diagnostics do not qualify sustained moving-canvas
navigation or resolve that queue gap. Top tier was reserved by another session
and is not remeasured. Physical stylus latency and the earlier rare Adreno
allocation failure remain unclassified.

APK SHA-256:

- Two pages, queueing:
  `44c681044dcfac79787feadbed8687ed817e81016d25acabdb1b22a1d2521a15`.
- Four pages, queueing, selected:
  `05d747e5f3e6417e9ea3a96a0d1b5bfe38a3aca00744891bbdd01c9a37334a8b`.
- Four pages, gated:
  `5d030a9a23f372060bd8bbbfa58f75b58e998ff76276662278ab94904b632e5e`.

Raw results, source patches and analysis:
`artifacts/refinement-tradeoff/{two-overlap,four-overlap,four-gated}-{low,mid}-{constant,pauses,settle}`,
`provenance.json`, `results.json` and `analyze.py`. Rate runs have tracing disabled;
only the short navigation probes use presentation tracing.

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
