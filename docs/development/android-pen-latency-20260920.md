# Wacom 2048 px G-Pen: current latency and limits — 2026-09-20

Later shared-renderer changes and a measured 30+ FPS result are documented in
[the follow-up optimization report](android-pen-30fps-20260920.md). This report
preserves the original baseline investigation.

## Result

The primary app still has severe wide-pen latency. A fast 2048 px stroke on the
9504×6336 photo produced **0.6–0.8 visible canvas updates/s**. The original
installed release failed with `GPU readback mapping failed: Buffer is not mapped`.
Freshly restarted instrumented builds completed the workflow, but still slowed
below one update/s. This investigation does **not** establish a freeze or mapping
failure fix, or ship a brush optimization.

There are two substantial GPU costs: paint and composition. CPU wall-time
attribution alone misleadingly assigns almost everything to composition because
that stage waits for earlier paint commands. Undo/redo are dominated by
composition, not brush replay or the already-improved cached restore operation.
Cursor input shares this blocked owner thread. Thumbnail transport is secondary
in these drawing runs, though it can cause separate substantial owner stalls.

Freshly fetched base: `a8c51612` on `origin/main`. Its only change relative to
installed `737ca48e` is Web Zen styling. The installed baseline APK hash was
verified as `26d5492f240fac88e0ddfe1da5bfe7b13f2f58ab8f946783cb1ea03323bebd73`.
All app tests used **art.capycanvas** on Wacom DTHA140 `5ll21u1002931`, Android 15,
Adreno 735, 2880×1800, 120 Hz. No secondary application was used.

## Workload and measurement

The recovered photo has 60,217,344 pixels, a separate selected paint layer
(`Layer 3`), and hidden paper. G-Pen is 2048 px, opaque black, pressure 1.
Prediction is enabled with a 16 ms fallback; Android prediction is unavailable
for the injected device. The photo and panels were checked with screenshots.
The recovered document is not the earlier isolated benchmark's direct-on-photo
painting workload, so its historical 16 updates/s is not a current-app result.

`AndroidPenMotion.java` injects real Android stylus MotionEvents at 200 Hz.
The screen ellipse is centered at (1410,948), with radii (520,299). Each workflow
contains 5 s hover, 2 s idle, 10 s drawing, 3 s idle, Ctrl+Z, 3 s idle,
Ctrl+Shift+Z, 3 s idle, and 5 s hover. Markers use CLOCK_BOOTTIME. Fit zoom is
16.587% in the original process and 17.164% after recovery; the latter captures
share coordinates, zoom and brush settings. Recovery and process lifetime differ,
so release versus restarted-build differences are **not an optimization A/B**.
The replay's pen-up returns to the starting coordinate. At 0.25 loop/s, the
ten-second stroke ends halfway around the ellipse, so this adds a final chord;
the saved screenshot shows it. Keep that endpoint behavior when reproducing
these measurements. Some resulting work completes after the drawing marker.

The optimized/R8 benchmark build of the same primary package enables simpleperf.
The original release does not permit CPU stack sampling. Perfetto records input
age/owner-queue delay, CPU phase scopes, scheduler running time, driver API calls,
renderer/cache counters, GPU observations, and actual SurfaceView latches together.
Simpleperf samples owner stacks at 199 Hz with 16 KiB DWARF call chains. Native
addresses were resolved against each build's matching unstripped library.

GPU observations are asynchronous, bounded to three slots per timer, and never
wait for the reporting consumer. Extra GPU phase timers add marker passes and
resolve submissions **only during an Android trace with Diagnostics enabled**.
They are useful for attribution but perturb the workload: hover falls from
approximately 83–85 to 70–72 latches/s. Use the light-probe runs for normal
throughput. Slow-stroke throughput was 11.7/s before extra GPU phase probes and
11.3/s afterward; this is a probe-overhead comparison, not an improvement.

Latches demonstrate canvas buffer consumption, not physical nib-to-photon timing
or proof that every pixel belongs to the latest input. Undo/redo first-latch times
are measured after a three-second settling interval. Static periods after that
latch are not freezes. Full CPU scopes whose **start** falls inside an action are
retained, including any tail after its end. GPU tables below contain observations
delivered during the action; late observations are excluded. These are short
captures, not statistically qualified long-run p99 distributions.

## Current performance

| Capture | Drawing rate | Canvas updates/s | Drawing callback median / maximum | Undo first latch | Redo first latch |
| --- | ---: | ---: | ---: | ---: | ---: |
| Installed release, existing process | 1 loop/s | **0.6** | 1873 / 2860 ms | Failed process | Failed process |
| Light probes, restarted primary (`traced1`) | 1 loop/s | **0.8** | 1225 / 1885 ms | **821 ms** | **592 ms** |
| Light probes, slower motion (`traced-slow`) | 0.25 loop/s | **11.7** | 88.7 / 291 ms | **1545 ms** | **566 ms** |
| Additional GPU phase probes (`phases2`) | 1 loop/s | **0.8** | 1301 / 2415 ms | **1175 ms** | **572 ms** |
| Additional GPU phase probes (`phases-slow`) | 0.25 loop/s | **11.3** | 96.0 / 285 ms | **1219 ms** | **808 ms** |

Light-probe hover reaches 83–85 latches/s. Callback medians are approximately
6 ms. Input owner-queue delay during fast drawing is **586 ms median, 1541 ms
p99, 1623 ms maximum**, versus roughly 5 ms median while hovering. At the slower
rate its p99 is 202 ms. Fast-stroke callback times grow successively rather than
settling: in `phases2`, 305 → 625 → 1100 → 1207 → 1301 → 1486 → 1703 → 2018 →
2415 ms. The final queued work continues after pen-up. A slow frame admits more
motion before the next render, expanding both contact evaluation and dirty-area
composition. These traces support that feedback mechanism; they do not prove that
dropping real pen samples would preserve brush behavior.

## Where time goes

For the nine fast-stroke callbacks starting during `traced1`:

| CPU owner region | Summed wall time | Summed scheduled CPU time |
| --- | ---: | ---: |
| Whole drawing callback | 10,712 ms | 2246 ms |
| Preparation | 278 ms | 277 ms |
| Committed paint encoding | 78 ms | 78 ms |
| Prediction encoding | 15 ms | 15 ms |
| Composition, including nested work and waits | **10,281 ms** | **1817 ms** |

Nested within the same stroke window are 8572 ms in bounded waits (472 ms
actually running) and 912 ms finalizing command encoders (911 ms running).
These overlap the composition row; **do not add them to it**. The corresponding
416 owner CPU samples attribute 45.43% inclusively to command finalization,
16.11% to command-buffer freeing, and 10.34% to the Vulkan allocation entry
point. These stack percentages also overlap.

There are **19,023 AllocateCommandBuffers API calls**, 85 instrumented command
finalizations, 69 bounded waits, and 10,607 instrumented bind-group creations
in that stroke window. API-call counts are not counts of allocated command
buffers or heap objects. Resource probes cover the shared device wrapper, not
every raw device allocation. GPU source-cache misses advance from 1990 to 5057
between the first and eighth stroke publications; paint pages grow from 82 to
555. This exposes cache/command churn alongside expanding dirty work.

The additional GPU probes bracket actual command-stream phases:

| GPU interval, averages of completed observations during drawing | Fast, 6 samples | Slow, 111 samples |
| --- | ---: | ---: |
| Committed paint | **421.4 ms** | **10.1 ms** |
| Prediction | **19.9 ms** | **13.6 ms** |
| Composition and mips | **464.5 ms** | **49.6 ms** |
| Whole renderer interval | **909.4 ms** | **73.5 ms** |

The fast observed interval is approximately 46% paint, 2% prediction and 51%
composition. The slow interval is approximately 14%, 19% and 68%. GPU intervals
include gaps between submissions and are **not shader occupancy or pure GPU busy
time**. The mip-encoding CPU probe is tiny (about 6 ms total over 118 slow-stroke
encodes), but that does not establish cheap GPU mip execution. Source decode,
composition draws and mip GPU costs are not individually separated here.

The critical distinction is now measured: a 1.2-second *CPU composition scope*
does not imply a 1.2-second composition shader. It includes waiting for the
earlier brush execution as well as composition commands and driver maintenance.

Undo/redo confirms an independent composition problem. In `traced1`, undo's
775 ms callback contains 74 ms restoration and 673 ms composition; redo's
560 ms callback contains 4.4 ms restoration and 550 ms composition. In
`phases-slow`, undo spends 57 ms restoring and 1110 ms composing; redo spends
5.9 ms restoring and 759 ms composing. GPU paint and prediction are approximately
0.016 ms each in those history actions. Cached-restore batching cannot remove
the hundreds of milliseconds that follow it.

Thumbnail queries total only 0.28 ms during the overloaded fast stroke and
65 ms over the ten-second slow stroke. One redo query takes 147 ms, including
render/readback/transport and parsing; that entire duration must not be labelled
JSON parsing. Thumbnail queries run separately from frame callbacks on the same
owner. Parsing has its own nested trace scope. During the slow run, each measured
thumbnail query is below 15 ms. Transport is an independent cleanup opportunity,
not the main explanation for multi-second drawing callbacks in these captures.

Presentation is also distinct: in the successful light-probe fast stroke,
acquisition is at most 0.046 ms and present at most 0.518 ms. Canvas latches
advance with the scarce submitted frames. The dominant captured delay occurs
before presentation, unlike the earlier Mailbox replacement-only freeze.

## Ceiling and targets

The follow-up [CPU and bandwidth analysis](android-pen-bandwidth-20260920.md)
separates these current timings from a conditional bytes-per-update model.

There is no experimentally established absolute brush ceiling yet. The useful
bounds and counterfactuals are:

* **Display ceiling: 120 visible updates/s**, one refresh every 8.33 ms. The
  current hover path reaches about 83–85/s with light probes. Its ~6 ms median
  callback suggests useful CPU headroom, but does not prove a sustained 120 Hz
  implementation or physical input latency.
* **Current slow-stroke submission pattern: about 13.6 updates/s** from the
  73.5 ms average renderer GPU interval, before additional presentation costs.
  This is a scheduling/workload reference, not a hard hardware limit: shortening
  inter-submission CPU gaps can shorten that interval. Measured owner CPU is
  about 37 ms/update in the phase run, suggesting a separate idealized owner
  throughput limit near 27/s if that CPU work were unchanged and fully overlapped.
* **Removing composition entirely would leave about 23.8 ms of paint/prediction
  GPU intervals**, or roughly 42/s before presentation, remaining CPU work and
  coordination. This is an optimistic counterfactual, not an achievable claim.
  Removing only paint/prediction still leaves ~49.6 ms of composition, about 20/s.
  A brush-only rewrite therefore cannot plausibly deliver 60 Hz for this measured
  slow workload while leaving composition unchanged.
* **60 Hz requires a 16.67 ms total update budget; 120 Hz requires 8.33 ms.**
  Relative to the current slow GPU interval, those require about 4.4× / 8.8×
  less elapsed work and scheduling delay. Faster update cadence should also
  shrink the backlog, so this is not a linear shader-speedup requirement.

The earlier 18–20 updates/s estimate applied to another measured workload and
submission pattern. It remains evidence that the same brush math has done better,
not a promise for this separate-layer workflow. A sensible first milestone is
stable interactive progress without the expanding backlog, then a matched
measurement against that historical throughput. Claiming a guaranteed 60/120 Hz
wide-brush target now would exceed the evidence.

## Next shared work, ordered by evidence

1. Reduce source/decode/composition command churn while retaining the current
   cache and upload bounds. In `Scene::encode_jobs_inner`, source decodes are
   interleaved with output draws, interrupting runs that could share a render
   pass. A bounded grouping of independent source preparation and output work
   deserves a controlled replay. Reduced pass/attachment overhead is a hypothesis,
   not a proven fix; source-slot reuse must remain ordered.
2. Keep cursor/input service from waiting for a whole expanding drawing batch.
   Prefer reduced bounded shared work before adding host schedulers or an
   unbounded queue. Any incremental progression must preserve every real sample,
   ordered paint, prediction retirement, memory bounds and one-step undo.
3. Profile paint execution on a fixed contact stream after controlling backlog.
   The fast GPU capture now establishes substantial paint cost, but does not
   distinguish arithmetic, memory traffic and ordered-contact traversal. Preserve
   the existing pixel oracle before changing the evaluator.
4. Remove avoidable thumbnail serialization and redundant empty hover renderer
   work. Neither should be credited with solving the measured composition stalls.

## Failures and qualification

The original `737ca48e` process failed during the stroke, before the scripted
undo. Screenshot, logs and trace are preserved. It also logged Adreno GPU
snapshot warnings. The generic error name is used by upload staging as well as
readback; the exact source was not recovered from the release stack. New staging
error context helps distinguish those sites on recurrence. This is **not** a
demonstrated upload fix or proof of a GPU watchdog fault. Its trace contains 84
`systrace_parse_failure` diagnostics and is not accepted as a clean progress pass.

The four accepted instrumented captures above retain the full ~40 s trace and
report no trace errors. `phases1` is excluded: UI setup left an obstructed target,
the drawing phase produced zero dabs, and only the final hover reached the canvas.
It is retained in the artifacts and JSON rather than counted as a fast stroke.
Thermal-service snapshots were status 0; frequencies were not locked. These are
short sequential experiments, not proof of indefinitely stable driver mappings.

Brush algorithms, precision, prediction settings, history behavior, source-cache
limits, FIFO presentation and Vulkan reclamation policy are unchanged. Added
instrumentation is not a performance optimization. The old 2 Hz / 8 ms
device-loss workload and the unexplained prior navigation pause remain unqualified.
Other platform GUIs were not tested.

Validation passed: Android optimized benchmark build, Android release build and
release lint, Web renderer compilation, two existing GPU timestamp lifecycle
tests, and the two existing native G-Pen photo-pixel/batch-edge preservation
tests on the host hardware GPU. The initial sandboxed G-Pen test attempt could
not acquire a hardware adapter; the rerun with GPU access passed. These pixel
tests qualify unchanged brush behavior on that adapter, not long-run Android
driver stability. The primary app remains on the optimized profileable phase
build used for the accepted captures.

## Reproduction and evidence

Build the optimized profileable primary APK with:

```sh
cd apps/layer-android
./gradlew :app:assembleBenchmark -PcapyAbi=arm64-v8a -PcapyOptimize
```

Install it in place, recover/open the photo, select the separate paint layer,
set G-Pen to 2048 px, show Diagnostics and Layers, and use Fit Canvas. Verify
the unobstructed canvas and coordinates before injecting. From the repository
root, compile and push the helper (adjust SDK versions if needed):

```sh
mkdir -p /tmp/capy-pen/classes /tmp/capy-pen/dex
javac -cp "$ANDROID_HOME/platforms/android-37.0/android.jar" \
  -d /tmp/capy-pen/classes tools/performance/AndroidPenMotion.java
"$ANDROID_HOME/build-tools/37.0.0/d8" --min-api 29 \
  --lib "$ANDROID_HOME/platforms/android-37.0/android.jar" \
  --output /tmp/capy-pen/dex /tmp/capy-pen/classes/AndroidPenMotion.class
adb -s 5ll21u1002931 push /tmp/capy-pen/dex/classes.dex \
  /data/local/tmp/capy-pen-motion.dex
```

Start Perfetto with `tools/performance/android-pen.pbtxt` and simpleperf together,
then run the injector after two seconds:

```sh
# Terminal 1:
adb -s 5ll21u1002931 shell perfetto --txt -c - \
  -o /data/misc/perfetto-traces/capy-latency.perfetto-trace \
  < tools/performance/android-pen.pbtxt
# Terminal 2, started together with terminal 1:
adb -s 5ll21u1002931 shell simpleperf record --app art.capycanvas \
  -e cpu-clock -f 199 --call-graph dwarf,16384 --duration 38 \
  -o /data/local/tmp/capy-latency.data
# In another terminal while the 40-second trace and profile run:
adb -s 5ll21u1002931 shell \
  'CLASSPATH=/data/local/tmp/capy-pen-motion.dex app_process / AndroidPenMotion 1410 948 520 299 workflow 1' \
  > markers.txt
# Replace the final 1 with .25 for the slow control.
# After both captures have finished successfully:
adb -s 5ll21u1002931 pull /data/misc/perfetto-traces/capy-latency.perfetto-trace \
  capture.perfetto-trace
adb -s 5ll21u1002931 pull /data/local/tmp/capy-latency.data capture.perf.data
python3 tools/performance/android-pen-report.py capture.perfetto-trace markers.txt \
  --processor /path/to/trace_processor > report.json
```

Undo the preceding test stroke before the next run; allow it to finish. Capture
screenshots and inspect application errors as well as counters. Wait for both
profilers to finish successfully before pulling outputs. The reporter includes
trace errors and action windows; it does not silently convert a failed process
or an obstructed test into a responsiveness pass.

Compact tracked measurements: [action reports](measurements/android-pen-latency-20260920.json).
Local raw evidence, matching APKs/libraries, CPU reports, full traces and screenshots:
`artifacts/wacom-latency-20260920/`. The earlier FIFO mitigation and its limitations
remain documented in [the presentation investigation](android-presentation-progress.md).
