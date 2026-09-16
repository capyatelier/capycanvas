# GPU raster benchmarks

[Technical documentation](../README.md)

Paths under `artifacts/` refer to ignored local outputs, not files shipped in
this repository. See [publication notes](publication.md#publication-checks).

The [GTK raster foundation qualification](../history/color-management-gtk-m1-validation.md)
records the encoded sRGB8 replacement's baseline, repeated drawing comparison,
dense-image/concurrent-save workloads, capture memory and native presentation.

## What is measured

`layer-bench` submits 15 legacy and 10 painter-focused 4096×4096 workloads
through the public C ABI. Every scenario has at least 32 visible paint layers
and every frame contains eight simulated coalesced pen samples.

The current diagnostic factory uses GTK's native integer-backed SDR renderer
with Float32 working tiles. Select `--space srgb|p3|adobe-rgb|prophoto` and
`--depth 8|16`; defaults are sRGB8. Generated reports identify the mode. Reports
from before this replacement used the older sRGB8 working renderer and cannot
qualify the current native editing path. The [final GTK SDR qualification](../history/color-management-gtk-m2-performance.md)
records comparison arms and declared budgets.

It reports two time boundaries:

- **submit**: event validation, queueing, shared brush dynamics/contact
  generation, wgpu command encoding, uploads, and queue submission. Call-return
  time includes any internal capacity/dependency wait; it is not necessarily
  nonblocking. Thread CPU time, where recorded, distinguishes computation from
  waiting;
- **completed**: the same work plus a benchmark-only wait for that exact GPU
  submission, including changed-tile capture copies and any backing-capacity
  delay needed to consume the measured input. Compression finishes asynchronously.

Deferred input is drained before completion is recorded. CPU frame creation is
reported separately from capacity/GPU waits. The capture allocated/reserved peak
adds pending staging reservations, reusable spares and active CPU scratch; it is
separate from the older canvas-residency counter and excludes source/history and
driver allocations. A returned buffer can remain charged in its pending capture
reservation until that job finishes, so the capture counter is conservative.

Normal scenario reports also write a sibling `*.frames.csv` containing every
measured frame: scenario, color-space code (0 sRGB, 1 P3, 2 Adobe RGB, 3 ProPhoto),
integer depth, one-based repetition/stroke/frame indices, pen-up flag, CPU and
completed-work microseconds, and capture reserved bytes. Serialization runs after
all measurements, using the samples already collected by the timing loop. Use
these records to compare matching strokes and inspect tail distributions; the
Markdown summary alone can hide which workload produced an outlier.

Initialization, shader/pipeline creation, target allocation, brush selection,
layer creation, and PNG export stay outside the measurement. Each repetition
creates a fresh canvas, runs one real stroke and undo to prime the exact
pipeline and GPU clocks, then contributes every measured frame to the aggregate
distribution. The release painter record uses three repetitions per brush.

## Workloads

The `raster_workloads` example separately exercises tiled native photos. Build
with `cargo build -p layer-render-wgpu --example raster_workloads --release` and
run the resulting executable with `24mp`, `45mp`, `60mp`, or `multiple`.
`--space` and `--depth` use the modes above; this example defaults to ProPhoto U16.

Add `--photo --output-dir PATH` for masked Exposure, White Balance, Levels,
Hue/Saturation and Color Balance, slider previews, cyclic pan/zoom, simultaneous
native save and profiled ProPhoto U16 PNG export, full-resolution histograms,
Gaussian blur and exact native paint/mask undo restoration. `multiple` retains
all three adjusted documents and edits the 60 MP document while save/export run.
The synthetic source includes low-order U16 variation; it is a reproducible
resource workload, not a photographic accuracy corpus.

Frame and worker CSVs share one monotonic clock. Use their overlap fields to
select frames actually concurrent with save/export, and separate source misses
from warm frames. Whole-operation durations include worker setup, file sync and
GPU renderer destruction; separate setup and cleanup intervals identify those costs.
These are offscreen CPU/GPU completion measurements, not native presentation
latency. Optional allocator observations occur outside frame timings and at
capture allocation boundaries. Capture workers reuse the active canvas device
with private pixels and nonblocking completion waits. Its allocator report covers
all live owners: take the maximum reported peak for that device, then add peaks
for other canvas devices. Do not add the same device's reservation twice. This is
a conservative overlap estimate; record process RSS high-water and driver memory
separately. Three histogram repetitions are individual observations, not a
well-sampled p99 distribution.

`--photo-capture` omits the variable-length editing interval, slider/navigation
loops and blur. It supplies identical snapshots to capture comparison arms;
decoded PNG sample checksums and histogram bin/endpoint checksums must agree.
Archive reopen compares retained source/profile, editable effects, and exact
native paint and mask digests. Serialize hardware runs and retain exact
executables, source hashes, environment and raw output for each arm.

`--photo-navigation` isolates unchanged-image pan/zoom/rotation on the adjusted
24/45/60 MP photographs. Each size runs two 96-frame revolutions at fit, 50%,
100% and 200% zoom, then two revolutions with continuously varying zoom. The
1600×1000 managed Float16 presentation target matches the reference viewport
size. CSV rows include camera scale/angle, frame CPU time, total CPU time through
presentation submission, completed queue time, recomposited pixels, source misses
and display storage. It waits for the presenter's submission too; waiting only
for the engine would omit presentation work. This remains an offscreen workload,
so native GTK frame delivery must be measured separately. Every frame preserves
artwork revision, and the run verifies unchanged native paint/mask roots. The
first navigation frame and cache misses stay in the results. `multiple` uses the
existing `--photo` worker fixture instead.

The native counterpart is the ignored GTK test
`workspace::tests::native_navigation::native_large_photo_navigation`. Build the
release test executable first, then run it alone with `gtk-raster.sh` and
`LAYER_NAVIGATION_PHOTO=24mp`, `45mp`, `60mp` or `61mp` (9504×6336). It maximizes the editor on the
private 1600×1000@120 display and records the actual canvas viewport. It generates
960 camera requests on an absolute 120 Hz schedule through the shared gesture
API and GTK change/wake path, without waiting for each render. This measures
software camera-request-to-presentation, not physical input delivery. The native
source has five pointwise adjustments and 32 paint layers; unlike the offscreen
fixture it has no painted stroke or adjustment mask.

For the high-DPI large-photo case, set `LAYER_TEST_MONITOR=3840x2160@120` and
`LAYER_TEST_SCALE=2`. The harness applies and verifies the private Mutter monitor's
scale; `GDK_SCALE` alone is insufficient on Wayland. Set
`LAYER_NAVIGATION_MAXIMIZE=0` for a 1200×900 logical window (2400×1800 physical at
scale 2). `LAYER_NAVIGATION_PHYSICAL=1` adds a Gaussian blur evaluated at native
photo resolution before display reduction. `LAYER_NAVIGATION_COMPLETE=1`
additionally requires zero recomposited pixels and zero source-tile misses in
camera frames, qualifying the complete
display-pyramid path on devices with sufficient reported memory headroom. The
fixture fails if any gesture is rejected or the GPU worker stops. Reports include
actual monitor scale, viewport, per-frame composition counters and display bytes.
The fifth `camera_work` column counts source misses within the timed canvas frame;
the fourth retains cumulative misses, including separately scheduled thumbnail
work. Presentation timing includes any interference from those background jobs.

`tools/performance/photo-navigation-report.py REPORT.json` matches exact camera
matrices and frame IDs to Wayland presentation feedback. It reports unmatched
requests, discarded feedback, refresh-slot gaps, phase-specific p95/p99/max,
worker CPU/GPU time and GTK frame-handler time. Do not infer request latency from
frame cadence alone, or ignore a failed phase in the whole-run aggregate. Keep
builds and other performance workloads out of the measurement interval.
The report excludes late-arriving startup feedback from navigation cadence,
retains unmatched requests by phase, and separately reports first-request to
first-navigation presentation, request-to-enqueue, and enqueue-to-present delay.
Repeated presentations of the same input count once for input latency, while
remaining in cadence and work totals. Inspect distinct presented requests as
well as refresh cadence: repeating old poses can hide coalesced input. Optional
`LAYER_NAVIGATION_SETTLE_MS` separates a ready-window run from the default cold
first interaction; retain both, including first response and lost requests.
The [GTK acceptance summary](../history/color-management-gtk-m2-acceptance.md)
records the qualified envelope and outstanding platform work.
The first-response measurement includes initial coalesced requests; the ordinary
request-latency distribution can only contain requests matched to presentation.

For scheduling investigations, `LAYER_NAVIGATION_PHASE_NS` selects the first
request's offset from the current display-clock prediction. The clock continues
to update from feedback; this is an initial offset, not a phase lock. Release
**test executables only** accept `LAYER_PACING_LEAD_NS` to override the lead before
predicted presentation. Production retains its three-quarter-refresh drawing deadline; unchanged-camera
bursts use the separately qualified input/presentation phase policy.
These controls compare scheduling without changing source pixels or rendering.

| Scenario | Workload |
| --- | --- |
| G‑Pen inking | Two long pressure-varying analytic strokes |
| Pencil shading | Twelve textured hatching strokes |
| Large eraser | 360/620 px analytic erase over textured underpaint |
| Large paintbrush | Three 520–880 px rotating textured strokes |
| Soft airbrush | Low-flow analytic coverage |
| Anchored-grain chalk | Mask tip plus canvas-locked grain |
| Flat marker | Directional high-aspect coverage |
| Scatter spray | Seven particles/contact with spatial and count jitter |
| Dual texture | Two transformed tips plus anchored grain |
| Multiply glaze | Destination-read blend mode over underpaint |
| Smudge pickup | Pull, blur, and zero loaded paint over underpaint |
| Wet round Oklab | Pickup, dilution, deposition, jitter, and perceptual mix |
| Liquify push/twirl | Inverse-mapped deformation over underpaint |
| Layered composite | 34 paint layers with opacity, ink, texture, and wash |

The painter suite uses four large, pressure-varying strokes in one coherent
coral/indigo/gold/teal palette over localized color swatches:

| Scenario | State path exercised |
| --- | --- |
| Textured Flat Filbert | advanced dry build-up |
| Dry Scumble | stroke-uniform coverage only, thresholded grain |
| Pastel Block | advanced dry build-up with dense paper grain |
| Transparent Glaze | deposited wetness only |
| Opaque Gouache | spatial reservoir + wetness |
| Watercolor Wash | coverage + R8 wetness + capillary transport + live edge |
| Wet Watercolor | coverage + stronger wet transport + R8 wetness live edge |
| Loaded Oil Mixer | spatial reservoir + wetness |
| Palette Knife | spatial reservoir + wetness |
| Natural Blender | ordered smudge advection |

This gives every new state primitive an independent path: coverage-only,
wetness-only, spatial reservoir, same-layer watercolor advection, and smudge
advection are timed directly. The watercolor cases also time the bounded
conductance-gated transport pass. Watercolor's edge is part of ordinary
composition; the older optional post-stroke edge remains in the pen-up
distribution only for non-watercolor brushes that request it.

## Workstation result

The painter result is
`artifacts/benchmarks/painter-brushes-4k.md`.
The corrected destination-feedback and liquify result is
`artifacts/benchmarks/complex-brush-interactions-4k.md`.
The current layer-wide watercolor and three-stage capillary-relaxation result is
`artifacts/benchmarks/watercolor-relaxation-4k.md`.
The unchanged-path regression result is
`artifacts/benchmarks/gpu-4k-paint-state-regression.md`,
with the pre-state baseline retained at
`artifacts/benchmarks/gpu-4k.md`. Exact
p50/p95/p99, maxima, over-budget wall-clock samples, work counts, sparse page
counts, and resident bytes remain in those generated reports rather than being
duplicated here.

The three-repeat legacy report contains one 24.804 ms serialized wall-clock
Pencil pen-up sample among only 36 pen-ups. The larger
`artifacts/benchmarks/pencil-pen-up-investigation.md`
measures 144 pen-ups at 2.903 ms p99 and 6,480 total frames at 2.880 ms move
p99. Combined with an unchanged 2.36 ms median, this classifies the isolated
sample as host/GPU scheduling noise rather than a repeatable raster regression;
the raw failed small-sample gate remains visible in the original report.

The state-specific implementation is also bounded structurally. A brush that
opts out keeps the original dry pipeline. The material stage has prepared
color-only, coverage-only, scalar-state-only, and combined target layouts, so
it does not attach or write unused state. Coverage, wetness, and watercolor
wetness pages allocate lazily. This is the optimization claim the timings test;
it is not a claim that a finite benchmark proves a globally optimal shader.

The offscreen suite proves brush and composition execution headroom but does not
include surface acquisition, compositor scheduling, or scanout. The current GTK
milestone-2 user scope requires smooth 120 Hz unchanged-photo navigation with
input-to-present latency documented; the earlier strict p99 below 8.33 ms is a
historical target. See the [scope and measured limits](../history/color-management-gtk-m2-performance.md).

## Run

On a multi-GPU benchmark host, `LAYER_GPU_INDEX=N` selects the enumerated
adapter used by the offscreen harness. Production frontends instead pass their
platform-selected adapter and device to the renderer.

```bash
cargo run --release -p layer-bench -- \
  --scenario painter --repeats 3 \
  --output-dir artifacts/painter-brushes \
  --report artifacts/benchmarks/painter-brushes-4k.md

cargo run --release -p layer-bench -- \
  --scenario legacy --repeats 3 \
  --output-dir /tmp/layer-legacy-regression \
  --report artifacts/benchmarks/gpu-4k-paint-state-regression.md
```

The first command writes ten explicit-export PNGs and a labeled HTML gallery.
The generated contact sheet provides a compact montage. Use
`--scenario all` for all 25 workloads or a scenario name to isolate one brush.

Functional brush review is intentionally separate from the performance
composition. The first command below renders three isolated marks over an
untouched canvas; the second renders two crossings over separated one-contact
opaque color wells for pickup, mixing, and smudge inspection, plus push/twirl
deformation over a fine grid:

```bash
cargo run --release -p layer-bench -- \
  --brush-validation blank \
  --output-dir artifacts/brush-validation/blank

cargo run --release -p layer-bench -- \
  --brush-validation destination \
  --output-dir artifacts/brush-validation/destination

cargo run --release -p layer-bench -- \
  --brush-validation watercolor \
  --output-dir artifacts/brush-validation/watercolor-v4-capillary-relaxation

cargo run --release -p layer-bench -- \
  --brush-validation transport \
  --output-dir artifacts/brush-validation/watercolor-transport-v3-relaxation
```

The watercolor command writes twelve controlled settings and interaction cases.
The transport command writes a 4-field × 3-rate/distance matrix covering long
and short, broad and narrow conductance, 16–88 px effect radii, wet mixing, and
dry bleed. Each visible update uses three bounded coarse-to-fine GPU stages; it
does not jump pigment directly across the configured radius.
Generated review outputs stay under the requested ignored artifact directory.
The [paint-state reference](../reference/painterly-paint-state.md) describes the
behavior these cases exercise.

### GTK native pen-up and following strokes

The ignored `workspace::tests::native_penup::native_penup_and_following_strokes`
benchmark uses a validated 4096² ProPhoto U16 project with 32 paint layers and a
720 px palette knife. It warms and undoes one contact, then draws twelve 800 ms
contacts by default (`LAYER_PENUP_STROKES=4..100`). Run a captured release test
executable with `tools/performance/gtk-raster.sh` on its private 120 Hz compositor.
Build separately from measurement and compare variants at one staged executable
pathname. The fixture validates its project before opening, so autosave/recovery
exercise a legal allocator and document.

The JSON records pen-up event timestamps, corresponding native-publication frame
IDs, worker elapsed/thread CPU and GPU timestamps, actual child-surface
presentation feedback, and the first observation of each host-backed revision.
Host backing is observed at approximately 2 ms event-loop intervals; this is an
upper-bound observation of completion, not a precise compression service time.
A missing/discarded presentation must remain visible in the report, rather than
being silently excluded from the pen-up denominator. Subsequent strokes begin
as soon as the preceding stroke is admitted and can overlap its backing work.
Queued-frame-to-present timing for movement excludes input-to-queue delay;
pen-up event-to-present includes that delay. Keep these boundaries distinct.

### Web and Android tablet photo navigation

Use an isolated Android package, preserving normal application data:

```sh
ANDROID_HOME="$HOME/Android/Sdk" CARGO_NET_OFFLINE=true \
  apps/layer-android/gradlew -p apps/layer-android --offline \
  :app:assembleDebug :app:assembleDebugAndroidTest \
  -PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.colorm2 \
  -PcapyAppLabel='Capy Canvas Color M2'
adb install -r apps/layer-android/app/build/outputs/apk/debug/app-debug.apk
adb install -r apps/layer-android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb shell run-as art.capycanvas.colorm2 tee files/photo-benchmark.jpg \
  < "$HOME/Downloads/sony_a7r_v_29.jpg" > /dev/null
adb shell am instrument -w -e photoBenchmark true \
  -e class art.capycanvas.AndroidPhotoNavigationBenchmarkTest#fullSizePhotoNavigation \
  art.capycanvas.colorm2.test/androidx.test.runner.AndroidJUnitRunner
adb pull /sdcard/Android/data/art.capycanvas.colorm2/files/photo-navigation-0.json
adb pull /sdcard/Android/data/art.capycanvas.colorm2/files/photo-navigation-1.json
adb pull /sdcard/Android/data/art.capycanvas.colorm2/files/photo-navigation-2.json
adb pull /sdcard/Android/data/art.capycanvas.colorm2/files/photo-navigation-info.json
```

The fixture requires the 9504×6336 Sony JPEG. It sends two-finger records through
ordinary owner scheduling at 361 Choreographer ticks per run, three runs, with
fit→20×→fit zoom, rotation and pan. It preserves isolated workspace/recovery
settings and records the actual physical viewport. Report import/cold run and
warm runs separately. A test finishing successfully does not by itself pass the
120 Hz gate: inspect delivered-frame counts, callback cadence and each CPU field.
Do not count expected-presentation timestamps as measured presentation feedback.

For Chrome, forward its debug socket, open the photo in a dedicated test tab and
select that tab's explicit ID from the endpoint's `/json/list` response:

```sh
adb forward tcp:9228 localabstract:chrome_devtools_remote
node apps/layer-web/photo-navigation-bench.test.mjs TEST_TAB_ID REPORT.json
```

The script requests fullscreen, uses ordinary app frame scheduling and records
three 361-request runs, CPU frame time, RAF cadence, camera, viewport and renderer
storage. It never changes Chrome flags or launches/closes user tabs. This injects
camera commands, not hardware touch; it is not an input-to-photon test. Run tablet
native and Web workloads separately, with other test photos released. Account for
browser refresh throttling independently of GPU rendering time. The milestone
[tablet validation record](../history/color-management-web-android-m2-validation.md)
records current results, exact baselines and remaining limits.
