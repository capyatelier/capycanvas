# Apple performance observations

The iPad and Mac use the same optional recorder, serial render owner and Rust
GPU timer. Ordinary launches leave trace recording disabled. Visible Diagnostics
collects renderer timings independently of that optional recorder.
The native CAMetalLayer subclass observes the drawables acquired by wgpu and
uses Metal's `addPresentedHandler` and `presentedTime` to record actual display
presentation. A display-link tick or completed Rust call is not a presentation.
The iOS Simulator SDK does not expose drawable IDs or presentation callbacks;
simulator runs omit these events and cannot establish presentation acceptance.

Retained ten-minute 4K watercolor and ink sessions are recorded on each
physical platform below, with their measured source revisions.
Complete workload-matrix results and physical input-to-pixel evidence remain
required on both platforms. The [Mac recorder comparison](#mac-recorder-overhead--2026-09-17)
below qualifies one workload; remaining instrumentation scope includes iPad.
Following the user's
2026-09-11 clarification, current Mac validation targets **90 Hz (11.11 ms)**;
Mac 120 Hz presentation testing is deferred until suitable hardware is available
and does not block current Mac milestones. The iPad target remains **120 Hz
(8.33 ms)**. Keep failing workloads and unsupported measurements visible.

On 2026-09-15 the user accepted smooth drawing with rare measured misses.
The refresh targets remain, but presentation p99 exceeding one refresh interval
is no longer an automatic release blocker. Historical strict-cadence failures
below retain their original measurements. Final acceptance combines them with direct
physical drawing, responsiveness, visible-stutter and thermal checks. It does
not permit reduced brush fidelity or conceal rejected input and renderer errors.

The user subsequently reports smooth, accurate drawing with pressure variation
and working Undo/Redo on both physical hosts, using Pencil and an XP-Pen
14-inch Ultra tablet. iPad palm rejection and two-finger zoom/rotation pass, as
does unsaved background/return followed by another stroke on both hosts. These
are direct physical observations; they do not replace the retained sustained
measurements. Evidence is `artifacts/apple-physical-input-review-v1/`.

That review exposed empty GPU values in Diagnostics. Replacing the duplicate
empty-pass timer with the shared asynchronous timer restored numbers on both
hosts, but the user then reported iPad GPU spikes into hundreds of milliseconds
and visible lag even with Diagnostics hidden. That candidate is not accepted.
The updated artwork is preserved in `artifacts/apple-diagnostics-regression-v1/`.

A focused Release probe on the physical iPad uses a private copy of the affected
2048×1536 drawing and Rough G-Pen at 120.7 px. With timing disabled/enabled/disabled,
480 supplied-input frames per case have input-plus-frame CPU p99 of
11.25/6.02/5.15 ms and maxima below 14 ms; the enabled GPU p50/p95/p99 is
2.08/2.94/3.28 ms. Thermal state is nominal. This isolates the renderer and does
not reproduce the reported stalls; it does not establish full-editor or physical
Pencil acceptance. A separate bounded Mac GPU probe of delayed sensor updates
also does not reproduce the stall.

The timer is revised to encode its nonempty timestamp markers in the existing
drawing submission, removing two extra queue submissions per measured frame
and excluding CPU encoding before submission from GPU time. It retains the
shared three-slot nonblocking readback implementation, lazy allocation and
visibility gating. Optional whole-frame recording retains its explicit queue
span. Both timer hardware tests and all four native Navigator/Diagnostics checks
pass, including final results after drawing stops and a 200 ms artificial CPU
encoding delay excluded from the GPU span. Both Release builds and the shared
WebAssembly check pass without compiler warnings. The same physical iPad probe with encoded markers passes all four cases;
Rough G-Pen GPU p50/p95/p99 is 0.96/1.36/1.54 ms, while input-plus-frame
p99 is 6.92 ms and maximum is 13.36 ms. Thermal state remains nominal. Lower
GPU numbers reflect changed measurement boundaries, not a claimed doubling of
drawing speed. A 600-second review trace contains no new input and cannot
reproduce the reported failure. A further probe with the full editor and native
display link mounted records 2,330 drawing frames, no renderer errors or dropped
trace records, nominal thermal state, owner-frame CPU p99/max 9.30/10.14 ms,
and Rough G-Pen GPU p99 2.37 ms. It uses supplied contacts and an isolated document;
one UIKit appearance-transition warning belongs to the private fixture's root
controller replacement. No severe stall is reproduced. Both revised review apps
are restored with the original drawings; live Pencil verification remains open.

## Bounded display composition — 2026-09-24

At `f697427e` the physical M4 iPad aborted the `layered-4k` workload during
setup: `IOGPUMetalCommandBufferStorageAllocResourceAtIndex` failed while
`Scene::compose` encoded a render pass. Complete display pyramids had allowed
128 tiles per command buffer (`f2f9020b`), and eight translucent layers put
several jobs on each tile. Display composition now also submits after 256
encoded jobs. One- and two-layer compositions (at most two jobs per tile) keep
their 128-tile batches; an instrumented run showed 4–6 jobs per tile in the
layered fixture.

Release, 60 measured seconds, synthetic input (M4 iPad at 120 Hz; M2 Pro Mac at
90 Hz, `layered-4k` 20 seconds). Owner service and GPU queue span are p50/p99 ms:

| Run | Before | After |
| --- | --- | --- |
| iPad `ink` | 112.8 frames/s, 1.53/1.97, GPU 1.26/1.96 | 112.6 frames/s, 1.49/1.99, GPU 1.22/1.89 |
| iPad `layered-4k` | aborts during setup | 113.1 frames/s, 1.95/2.88, GPU 1.92/3.53 |
| Mac `ink` | 84.7 frames/s, 1.95/3.18, GPU 3.82/5.98 | 84.8 frames/s, 1.90/3.08, GPU 4.72/6.39 |
| Mac `layered-4k` | 84.5 frames/s, 2.52/4.26, GPU 4.49/6.92 | 85.0 frames/s, 2.42/4.15, GPU 4.72/7.22 |

Mac GPU queue spans vary between runs of the same binary: repeating the unchanged
baseline `ink` build after the change measured GPU p50 4.68 ms. Traces are in
ignored `artifacts/apple-port-0924/perf/`.

## Wide-brush source reuse — 2026-09-18

Alternating independent composition-tile traversal retains more decoded sources
within the existing 64-slot cache. A captured access model and both hardware
replays agree on median misses **131 → 100.5**. Reversed-order Mac pairs improve
median completion **39.378 → 37.418 ms** and **39.169 → 36.900 ms**; physical iPad
pairs improve **41.559 → 40.355 ms** and **41.803 → 39.890 ms**. Physical p99 changes
**65.786 → 63.611 ms** and **65.930 → 63.296 ms**, with nominal thermals and cached
zoom still about 1.80 ms. Native artwork and Undo/Redo are exact. No cache expansion,
precision change or new brush path ships.

Ordinary two-sample 2000 px Mac pairs improve medians 31.281 → 27.562 ms and
31.720 → 27.049 ms. Their p99 results vary in direction (42.587 → 49.099 ms,
then 43.996 → 36.849 ms); do not claim uniform tail improvement. The 96 px control
also preserves exact pixels/history, with median 4.030 → 3.851 ms.

This is the same bounded 180-update full-pressure workload described below,
not a Pencil-to-display latency measurement. The [algorithm review](../../docs/development/apple-drawing-performance-review.md#source-reuse-within-the-existing-budget--2026-09-18)
records the model, failed-before/passing-after regression and remaining limits.
Thirty-one focused checks, both Release builds and Web compilation pass.
The updated normal iPad app preserves all 14 recoveries and 148 original files;
live full-pressure acceptance remains pending. Evidence is
`artifacts/apple-source-reuse-v1/`, based on `ab6d9370` plus the correction.

## Full-pressure 2000 px G-Pen — 2026-09-18

The user reports continued visible iPad lag after the earlier footprint correction,
with roughly 100 ms Diagnostics p99 at full pressure. Shared capsule tile planning,
sparse prediction retirement and batched Float32 dry-brush updates reduce matched
physical iPad replay median/p99 from **65.595/96.308 to 41.976/65.862 ms**.
Cached zoom median remains **1.804 ms** and both runs stay thermally nominal.
All 776 retained native tiles match exactly; 39 omitted tiles are verified empty.
Both runs preserve exact Undo/Redo. No cache expansion or precision reduction ships.

This uses 180 updates of eight synthetic samples at 240 Hz on the 61 MP source,
manual 8 ms prediction, 360 subsequent zoom updates and fixed 768 MiB display
admission. Completion includes offscreen presentation and queue waits; it does
not measure live Pencil-to-display latency. P99 uses sorted index
`floor(0.99 * (n - 1))`. The user reports improved drawing with roughly 60 ms p99 at maximum pressure
on the updated normal iPad Release. Algorithmic headroom remains under assessment;
all 14 recoveries and 147 original files were preserved. Both Release
builds, Web compilation and 30 distinct focused checks pass. The [algorithm review](../../docs/development/apple-drawing-performance-review.md#current-full-pressure-drawing-correction--2026-09-18)
records attribution, exact output qualification and rejected probes. Evidence is
`artifacts/apple-large-brush-attribution-v1/`, based on `24205989` plus this correction.

## Large-photo fast strokes and repeated zoom — 2026-09-16

The user reproduced the two reported stalls on the physical iPad while an
attached Metal/CPU trace recorded the existing review app: full-screen circles
with 570.7 px G-Pen over a 9504×6336 retained photo, followed by repeated zoom
sweeps. Thermal state stayed nominal. Destination brush pages account for
20.918 of 24.072 seconds of active GPU work, about 87%. CPU time attributed to
composition also includes waiting for previously submitted brush work. The
trace identifies avoidable rendering work; it does not support treating the
one-second delay as an unavoidable memory-bandwidth limit.

The shared renderer now restricts each dry-contact tile to the first through
last relevant contact, preserving contact order and the existing swept bounds.
Pointwise contact edits reuse touched-tile composition instead of repainting the
rectangle around a circle, including its untouched center. Nonlocal effects and
transformed targets retain conservative damage propagation. Display reduction
uses one compute pass with sequential mip dispatches and unchanged Float32
area-weighted arithmetic; the previous render-pass implementation is removed.

Two cache problems independently explain repeated zoom stalls. A byte capacity
that could not factor into a legal atlas rectangle caused repeated shrink/grow
and discarded pixels. Reserving the largest feasible rectangle fixes this.
Memory admission also discarded the entire measured allowance whenever the
full-resolution pyramid did not fit. Partial admission now retains reduced
levels alongside visible detail within that same allowance. The host memory
fraction and working precision are unchanged.

The reproducible `photo_interaction` example runs a private copy of the actual
recovery document on Mac Metal at the iPad's 2752×2064 framebuffer size. These
are offscreen completion measurements, not physical presentation or pen latency:

| Paired replay | Before | After |
| --- | ---: | ---: |
| 64 coalesced samples/frame, median completion | 1,035 ms | 131 ms |
| Same replay, p95 completion | 1,216 ms | 187 ms |
| Same replay, composited pixels | 2,487,443,904 | 217,328,896 |
| Three zoom sweeps, same 768 MiB allowance, recomposited pixels | 580,753,408 | 24,248,320 |

The coalesced stress frame contains 267 ms of input at 240 Hz. Both sides have
8,138 dabs, identical committed native tile roots and exact Undo/Redo; its
131 ms result is not a normal-frame cadence claim. The second and third zoom
sweeps perform zero recomposition after the fix. Final ordinary two/eight-sample
replays also preserve exact history; the eight-sample result matches baseline
committed roots. The physical iPad follow-up confirms smooth repeated zoom;
fast-circle drawing improves but still reports about 100 ms frame generation.
This remaining drawing cost is not an established hardware lower bound.

Regression coverage includes atlas retention, partial admission, sparse contact
composition versus a full rebuild, and long contact batches across tile edges.
The renderer library reports 281 passes, 29 ignored and the previously recorded
Mac/Linux filter-reference mismatch (maximum byte error 56); see
[filter qualification](../../docs/development/apple-filter-qualification.md).
All four contact integration tests and the large-JPEG save/reopen/GPU-replacement
test pass. Both final Apple Release builds and the WebAssembly check pass
without compiler warnings.
The iPad candidate is installed in place with all eleven recoveries preserved
and every saved file byte-identical across installation. The ordinary review
app preserves the user's working drawing. The Mac artist app remains untouched.
Evidence, paired results and private traces are in
`artifacts/apple-photo-lag-v1/`; remaining drawing diagnosis is under
`artifacts/apple-photo-lag-v2/`. The milestone is published as `6da20d05`.
Large-photo drawing acceptance and the wider
Apple release goal remain open.

## Remaining large-photo drawing cost — 2026-09-17

The physical follow-up accepts smooth repeated zoom but reports about 100 ms
frame generation during fast drawing. A short rolling trace retains the tail
of drawing, not the whole requested stroke: ten viewport encoders and 1,392
scene passes. Its CPU stacks concentrate in command materialization/submission;
brush GPU stages are much shorter than in the original capture. This supports
reducing pass overhead, not a theoretical memory-bandwidth or latency bound.

The shared normal-layer compositor now accepts complete destination-reading
preview tiles directly. Overlay-only previews and watercolor retain their
existing composition. Dry contact paint jobs also clip to the union of actual
contact bounds within each tile, skipping untouched tiles inside the batch's
bounding rectangle. Both persistent and predicted painting share that helper.
No input samples, precision, brush detail or memory allowance are reduced.

Paired Mac Metal replays preserve exact committed tile roots and Undo/Redo.
At eight input samples per frame, the repeated baseline has 12.98 ms median /
34.06 ms p95 completion; both changes reach 10.58 / 26.97 ms. In the retained
64-sample stress case, median/p95 move from 131.43/186.71 to 60.65/84.92 ms.
Cold maxima remain variable (the new stress maximum is 734 ms, concentrated in
first-frame preparation); these measurements do not close startup stalls or
physical iPad performance. A shader early-exit experiment preserved pixels but
showed no benefit and was removed. Evidence is `artifacts/apple-photo-lag-v2/`.
The renderer suite reports 282 passes, 29 ignored and the same pre-existing
filter-reference mismatch (maximum byte error 56). Both Release builds and the
WebAssembly check pass without compiler warnings. All four contact integration
tests pass. The iPad candidate is installed in place with all eleven recoveries
and saved files preserved byte-for-byte across installation. Mac review apps
remain unchanged. The pre-update screenshot retains CPU median/p95/p99 of
40.93/119.61/144.47 ms and GPU interval 35.11/106.54/125.63 ms; GPU intervals
include scheduling gaps and are not equivalent to active execution time.

The user now confirms faster drawing, with roughly 50 ms p99. The post-test
capture reports CPU median/p95/p99 of 12.08/47.32/55.75 ms and GPU intervals of
10.89/44.93/54.30 ms. Gesture, artwork and camera differ, so these captures are
not a controlled device A/B. The user requests an algorithm review before
accepting this as a limit; no repeat test is pending. The
[first-principles review](../../docs/development/apple-drawing-performance-review.md)
finds remaining rectangular allocation/source preparation and frequent bounded
submission waits. Retain the tiled architecture and memory limits; pursue those
shared opportunities before considering a rewrite. Physical performance closure
remains open. Local CPU sampling and arithmetic are in `artifacts/apple-photo-lag-v3/`.

The shared follow-up replaces repeated contact scans with one tile plan used by
allocation, paint/state updates, shader ranges, history and sparse composition.
It also overlaps CPU encoding with GPU execution using two eight-tile halves of
the existing 16-tile ceiling. Byte-based source staging and command-buffer bounds
remain intact; the last half submits with the frame without a terminal wait.
After integrating main `1cb1ebb8`, paired local completion medians improve across
ordinary input, 96 px zigzags, 1024 px circles and the coalesced 571 px case.
The latter moves from 59.58/79.10 ms median/p95 to 40.38/59.82 ms. Every retained
canonical tile is identical, omitted tiles are proven fully transparent, and
Undo/Redo is exact. Ordinary-input p99 is slightly higher in this pair; physical
improvement is still unqualified. The [review](../../docs/development/apple-drawing-performance-review.md#shared-implementation-follow-up)
records the complete matrix and limits.

Final qualification reports 284 renderer passes, 30 ignored and only the unchanged
filter-reference mismatch (maximum byte error 56); four contact integration tests,
both Release builds and the Web check pass. The bounded-display fixture now checks
8/9/16/17/32-tile boundaries and complete output pixels. The iPad is updated in
place with all eleven recovery records and saved files preserved byte-for-byte.
Ordinary review is open, recording disabled; physical fast-circle/Diagnostics and
large-photo local Save As/reopen confirmation are pending. Mac artist apps remain
unchanged.

The later physical retest still reports roughly 60 ms p99; a retained capture
shows CPU/GPU p99 of 82.16/70.35 ms. Correcting the local replay's mostly
off-canvas path exposes a redundant clear pass during source-tile loading.
A small shared ordering correction improves heavier paired Mac completion
medians by 8–12%, with identical artwork/history and work counts; it does not
close the full tail or establish an iPad speedup. See the
[corrected workload review](../../docs/development/apple-drawing-performance-review.md#corrected-workload-and-remaining-avoidable-passes).
After 31 focused Metal passes, Web/iOS compilation and both warning-free Release
builds, the iPad review is updated with all eleven recoveries and existing files
preserved. The user again reports roughly 60 ms; visible-lag acceptance remains
unconfirmed. Evidence is
`artifacts/apple-clear-pass-device-v1/`; recording is disabled.

The next local pass review removes redundant inactive paint-surface clears and
their flag, with 3–7.5% lower medians across sixteen paired small/large-brush
replays and unchanged exact artwork/history. Eight-sample 571 px p99 still
approaches 55 ms on Mac. This correction is not installed on either device;
it accompanies the native photo/16-bit qualification milestone. The [pass attribution and limits](../../docs/development/apple-drawing-performance-review.md#pass-attribution-and-redundant-paint-initialization)
remain distinct from physical latency and the unresolved iPad result.

## Metal display admission — 2026-09-16

Apple now admits the existing shared complete display and placed-photo caches
using one quarter of the smaller of current process/system headroom and Metal's
recommended working set minus existing GPU allocations. Unknown or exhausted
headroom admits zero and retains the bounded fallback. The fraction is our
application policy; Metal's [recommended working set](https://developer.apple.com/documentation/metal/mtldevice/recommendedmaxworkingsetsize)
is a performance estimate, not free memory or a reservation. iPadOS process
headroom uses the existing `os_proc_available_memory` wrapper. No new shader,
cache implementation, fixed memory-limit increase or scheduling path is added.

The paired Mac Metal navigation fixture runs both policies in the same Release
executable, using synthetic ProPhoto U16 sources with five adjustments and a
native mask. Each policy completes 960 offscreen viewport frames per size, with
unchanged source/paint/mask roots and artwork revision:

| Case | Bounded fallback | Measured admission |
| --- | ---: | ---: |
| 24 MP native-scale median completion | 0.765 ms | 0.580 ms |
| 24 MP peak display storage | 506.93 MiB | 489.14 MiB |
| 60 MP native-scale median completion | 2.753 ms | 0.591 ms |
| 60 MP all-navigation p99 completion | 4.851 ms | 0.927 ms |
| 60 MP maximum completion | 28.087 ms | 1.506 ms |
| 60 MP peak display storage | 561.32 MiB | 1220.82 MiB |

The larger admitted cache trades measured memory headroom for avoiding repeated
composition/source decoding during navigation. All admitted camera frames reuse
completed pixels. These are offscreen completion measurements, not physical
input-to-display or iPad large-photo acceptance. The fixture's new
`--bounded-display` option makes this comparison reproducible; its Linux-only
`/proc` memory report is now guarded on other hosts.

Both Release builds pass without warnings. Two admission checks, 16 display,
12 placement and 13 watercolor checks pass; the placement hardware benchmark
remains ignored. All 68 ordinary Apple bridge tests pass, followed by the
separate 61 MP JPEG painting, exact history/save/reopen and GPU-recovery test
on Mac Metal for both Apple policies. The 512-frame watercolor replay retains
exact artwork and per-frame work. Its unpaired renderer-only GPU median rises
from 4.618 to 5.411 ms; no painting-speed improvement is claimed.

Short timer-off native watercolor runs remain near their preceding results:
37/3788 (0.977%) long active intervals on Mac and 566/4544 (12.456%) on iPad.
Both have nominal thermal state, no frame errors, rejected input or missing
measured presentation callbacks, and reviewed artwork/previews. Peak process
footprints are 2.17 GB/2.23 GB; these short runs do not qualify sustained memory
pressure. Heavy-watercolor acceptance remains open. The iPad is restored to
ordinary review with all ten recoveries and 59 pre-existing files preserved;
the Mac artist review is untouched. Evidence is
`artifacts/apple-display-admission-v1/`.

A subsequent local experiment skips transport copies and attachment loads for
fully overwritten watercolor pages. All 13 watercolor checks pass, and four
paired 512-frame Mac Metal replays retain exact pixels and per-frame work.
GPU medians are 4.614/4.597 ms for the original and 4.624/4.649 ms for the
candidate, with no consistent tail improvement. The experiment is fully
reverted; neither physical review app is changed. Evidence is
`artifacts/apple-watercolor-full-pages-v1/`. This does not close the iPad gap.

## Watercolor after the shared photo fixes — 2026-09-17

The published renderer at `3fbb937b` also improves the retained 512-frame 4K
watercolor replay. Two baseline/current pairs use the frozen qualified
`90adbb6d` binary and the current binary, with no concurrent build or GPU work.
GPU medians move from 4.606/4.633 to 3.653/3.656 ms; completion medians move from
7.626/7.697 to 6.577/6.640 ms. Final 4096×4096 RGBA bytes are identical in all
four runs, as are every frame's composited-pixel, source-miss, source-upload and
paint-page counts. Display submissions change from zero to 287 because the
new half-batch submission begins GPU work earlier. These remain synchronous
local replay results, not physical iPad acceptance or pen-to-screen latency.

An isolated copy of the qualified Mac Release then completes the existing
45-second, timer-off native workload at the 90 Hz target. It records 34/3791
(**0.897%**) long continuous-active intervals, CPU owner-service p99 **5.544 ms**,
nominal thermals and a 2.073 GB peak footprint. The measured phase has no frame
errors, rejected input or missing/zero-time presentation callbacks. Artwork and
Navigator are reviewed. The analyzer's duration warning reflects a 10.5 µs
rounding difference from 45 seconds; completion and the postlude are present.
This is a short current-source Mac pass within the accepted rare-miss standard,
not ten-minute or memory-pressure qualification. The copied app uses the existing
workload's private session storage and is closed; artist Mac windows and the
pending iPad large-photo review stay unchanged.

No production edit accompanies this check. Evidence is
`artifacts/apple-watercolor-post-photo-v1/`. The remaining iPad heavy-watercolor
cadence check must follow the pending large-photo drawing/save retest, using
this already installed runtime before considering another rendering change.

## Sustained Mac watercolor after shared integration — 2026-09-17

The qualified Release at `f3a93595`, including the export-draft fix,
completes **600.0008 measured seconds** of the existing `wet-watercolor-4k`
workload. This is a 4096² sRGB/U8 document with eight paint layers plus paper,
320 px Wet Watercolor, prediction and 240 synthetic samples/second. The ordinary
editor, previews and recovery writer run throughout. GPU timestamps are disabled;
CPU/presentation recording and once-per-second memory sampling remain enabled.

At the Mac's 90 Hz target, **468/50,341 (0.930%)** continuous-active presentation
intervals exceed the budget plus the existing 5% timing tolerance. Their p99 is
**11.1113 ms**, maximum **22.2225 ms**. CPU owner-service p99 is **5.804 ms**,
maximum **12.314 ms**. All 135,001 measured real samples are accepted, with no
measured missing/zero-time presentations, frame errors or dropped trace records.
Per-minute long-interval rates range from 0.697% to 1.191%; CPU p99 stays between
5.676 and 5.989 ms. No compiler or other test/profiler ran during measurement.

Thermals remain nominal. Measured footprint peaks at 2.467 GB and grows 546 MB
from first to last sample; the last four minute peaks are 2.457, 2.457, 2.454 and
2.448 GB. This observed late plateau is not memory-pressure qualification or a
proof of a permanent bound. The complete trace retains larger startup costs:
817 ms maximum owner service and 3.696 GB peak footprint, both before measurement.
They are not silently folded into the steady drawing result. After pen-up, the
last frame is at 37.6 ms and the display link becomes idle at 38 ms.

The final artwork, Navigator and layer preview are reviewed. The private recovery
checkpoint completes after pen-up; the production `Project::read` and validation
load all nine layers at revision 431 successfully. The runner closes only its
owned test app, leaving both artist apps and the pending iPad review unchanged.
Runtime source hashes and the copied executable match the qualified inputs.
Evidence is retained in `artifacts/apple-watercolor-sustained-v1/`.

This closes the current Mac sustained sRGB/U8 watercolor cadence/artwork check
under the accepted rare-miss standard. It does not establish physical Pencil
latency, recorder-off overhead, other color depths, memory pressure or current
iPad watercolor/large-photo performance. No renderer change accompanies this
qualification; the pending physical iPad result remains the next device step.

## Mac recorder overhead — 2026-09-17

Six 45-second Mac Release intervals on runtime `dcfcb5de` compare the existing
sRGB/U8 layered-4K watercolor workload in **off, CPU-only, full, full, CPU-only,
off** order. A private build adds the same bounded drawable-presentation observer
and phase CPU/memory samples to every mode, including recorder-off. Production
input, rendering, panels and recovery remain active. No compiler, GPU profiler or
UI test overlaps the measurements. Each interval delivers 10,129–10,131 actual
samples, with nominal thermals; all six recoveries pass the production reader.

| Recorder mode | Mean process CPU over 45 seconds | Presentation interval p99 | Long intervals at 90 Hz, with 5% tolerance | Drawable acquisition-to-presentation p99 |
| --- | --- | --- | --- | --- |
| Off | 36.457 CPU seconds | 11.111 ms | 0.712–0.798% | 31.261–31.344 ms |
| CPU/input/presentation only | 36.243 CPU seconds | 11.111 ms | 0.797–0.851% | 31.259–31.623 ms |
| Full, including GPU timer | 37.985 CPU seconds | 11.111 ms | 0.664–0.798% | 41.385–41.430 ms |

CPU-only recording has no resolved CPU penalty within the recorder-off repeat
variation. Full recording uses approximately **4.2% more process CPU**, and its
drawable-to-presentation tail grows by roughly one 90 Hz refresh despite similar
presentation cadence. This is why cadence and scheduling delay must remain
separate measurements. Use `CAPY_TRACE_GPU=0` for cadence acceptance and short
GPU-enabled captures for attribution; do not apply a universal timing correction.
Mean final footprint increases by approximately 3.0 MiB with CPU-only recording
and 6.6 MiB with full recording. These short observations do not establish
sustained memory behavior.

The independent observer exactly matches the native trace's presentation records
in all four recorded runs. No measured callback has zero presentation time.
The full traces retain one startup zero-time callback each and 25/29 skipped GPU
samples, with no invalid GPU timings, polling errors, final pending samples,
rejected input, frame errors or recorder overflow. First/last artwork captures
are reviewed; all owned calibration processes are closed and artist editors are
preserved. Evidence and private observer sources are in
`artifacts/apple-recorder-calibration-v1/`.

The common observer has its own small cost. Two runs per mode do not establish
a statistical correction, and drawable acquisition is not pen input. Trace export
occurs after drawing and is excluded from the CPU intervals. Other workloads,
iPad overhead, physical input-to-display latency and the pending large-brush
Pencil acceptance remain separate. This private calibration build is not a
replacement review or shipping-build qualification.

## Mac recorder-off recovery and idle — 2026-09-17

The same qualified Release, with runtime hashes matching `eafd1d51`, completes
45 measured seconds of `wet-watercolor-4k` with `CAPY_TRACE_SECONDS=0`.
The existing configuration disables the complete recorder, including its GPU
timer; no tracing path or benchmark-only runtime change is added. The native
workload reaches its completion and postlude without a failure marker, and no
trace file is created. Canvas, Navigator and layer preview show the expected
painted artwork. The production reader validates the private recovery at revision
84 with all nine layers, 4096² extent and sRGB/U8 backing.

During a subsequent 15.45-second idle observation, process CPU time increases
by 0.04 seconds (about 0.26% of one core). Artwork/recovery bytes and persisted
workspace content remain unchanged. The initial all-files digest assertion
incorrectly included SQLite's normal ownership-lease renewal. Inspection of the
same live process shows only the expected lease changes; a semantic comparison
of all tables preserves every other value. No runtime fix or repeated workload
is needed. The owned app is then closed and existing artist processes remain
unchanged. The iPad review is untouched.

This qualifies one current Mac recorder-off drawing/recovery/idle case. It does
not measure unrecorded cadence, GPU power, thermal behavior, calibrated recorder
overhead or physical input latency, and does not qualify iPad. Evidence is
`artifacts/apple-recorder-off-v1/`.

The subsequent recorder-off resume check uses the same Release on source
`94bd5d1f`, with a ten-second measured watercolor setup and its normal warm-up
and postlude. After 15.45 seconds of foreground idle, a native mouse stroke
produces new durable artwork; Undo and Redo restore the exact document metadata
and all color/watercolor-state tile digests. The app's native Hide command then
precedes 15.43 seconds of hidden idle. Returning and drawing another stroke
passes the same complete recovery/history comparisons. Canvas and Navigator
captures show both new strokes and the intact preceding artwork.

Foreground/hidden idle consume 0.04/0.19 process CPU seconds respectively;
artwork/settings and workspace content remain unchanged except normal ownership
lease renewal. No trace file is produced. Direct-to-process mouse injection
initially produces no stroke; standard window-server delivery succeeds. The
external hide API also returns false, while the native Hide menu works. These
fixture corrections retain the same app process and require no runtime change.
The owned test app is closed and existing editor processes remain unchanged.
Evidence is `artifacts/apple-idle-resume-v1/`, held for the next milestone.
This qualifies native mouse drawing/history after idle and hidden return in one
Mac sRGB/U8 4K case; physical tablet/Pencil, iPad, other profiles, GPU power,
cadence and recorder overhead remain outside this result.

## Sustained Mac ProPhoto 16-bit watercolor — 2026-09-17

The shared workload runner now accepts the ordinary document color options
instead of fixing every run to sRGB/U8. The shared document reader validates
the requested profile/depth; the trace records them, and qualification checks
the actual saved document and native tile descriptors. Input, brush behavior,
rendering and storage use the existing paths. Both current Release builds pass
without compiler warnings, including the shared clear-pass cleanup.

The isolated Mac Release completes a 45-second preflight and **600.0084 measured
seconds** of 4096² ProPhoto/U16 Wet Watercolor, eight paint layers plus paper,
320 px brush and manual prediction, with 240 synthetic samples/second. Ordinary
previews and recovery remain enabled. CPU/presentation recording is enabled;
GPU timestamps are disabled. No build, test or GPU profiler overlaps measurement.

At 90 Hz, **461/50,312 (0.916%)** continuous-active intervals exceed the budget
plus the existing 5% timing tolerance. Presentation p99 is **11.1113 ms**, maximum
**33.3337 ms**; CPU owner-service p99 is **5.761 ms**, maximum **12.839 ms**.
All 135,003 measured input samples are accepted, with no rejected batches,
missing/zero-time measured presentations, frame errors or dropped records.
Per-minute long-interval rates are 0.797–0.998%, with CPU p99 5.671–5.860 ms.

Thermals remain nominal. Measured footprint peaks at **2.442 GB** in the first
minute; the last four minute peaks are 2.410, 2.422, 2.396 and 2.398 GB. The
first-to-last increase is 422 MB, without continued late growth in this run.
This is not imposed memory-pressure qualification. The full trace retains a
3.824 GB startup peak and one zero-time presentation outside measurement.
The display link becomes idle about 40 ms after pen-up.

Final canvas, Navigator and layer-preview captures agree. Production archive
reading/validation succeeds at revision 431, with nine layers, ProPhoto/U16
document color and 16-bit native color tiles. Both owned test apps close, and
pre-existing artist processes retain their identities. The iPad is not installed
or restarted. Evidence is `artifacts/apple-prophoto-sustained-v1/`.

This closes the current Mac sustained ProPhoto/U16 watercolor case within the
accepted rare-miss standard. It does not establish iPad large-brush performance,
physical pen latency, imposed memory pressure or recorder-off overhead. Retain
the existing sRGB/U8 and physical user workflow evidence separately.

## Sustained iPad ProPhoto 16-bit watercolor — 2026-09-17

The physical iPad Release at `1f96d5a6` completes a 45-second preflight and
**600.0081 measured seconds** of the same 4096² ProPhoto/U16 workload: eight
paint layers plus paper, 320 px Wet Watercolor, manual prediction and 240
synthetic samples/second. Ordinary previews and recovery remain enabled in
isolated storage. CPU/presentation recording is enabled; GPU timestamps are off.
No on-device profiler or container transfer overlaps measurement. Integration
checks and Release builds run concurrently on the Mac, using its own GPU.

At 120 Hz, **2,610/65,205 (4.003%)** continuous-active intervals exceed the
budget plus the existing 5% tolerance. Presentation median/p95/p99 are
**8.3333/8.3335/16.6668 ms**, maximum **25.0000 ms**. CPU owner-service p99 is
**9.124 ms**, maximum **17.581 ms**. The short preflight has 3.494% long
intervals and 9.126 ms CPU owner p99. The long run accepts 135,003 measured
nonpredicted samples and records 65,581 actual presentations, without rejected
batches, missing/zero-time measured presentations, frame errors or dropped
records. These figures do not establish the accepted smooth/rare-miss standard;
watercolor cadence acceptance remains open.

Thermals remain nominal across all 600 measured memory samples. Measured
footprint peaks at **2.358 GB** in the first minute; the last four minute peaks
are **1.969, 1.938, 1.936 and 1.909 GB**. First-to-last footprint falls by
23.6 MB, with no continuing late growth. The full trace retains a **3.952 GB**
startup peak, 10.217 seconds before measurement, and two zero-time presentations
before measurement. Postlude activity becomes idle about 28 ms after pen-up.

Final canvas, Navigator and layer-preview captures agree. Both recovery archives
pass the production reader with nine ProPhoto/U16 layers and **1,930 native
16-bit color tiles**; the long-run document reaches revision 431. Both workload
processes close. A subsequent ordinary review process is inspected at Recovered
Drawings and backed up before the update; all normal recoveries, saved drawings
and settings files are preserved. Evidence is
`artifacts/apple-ipad-prophoto-sustained-v1/`.

This supplies sustained physical iPad evidence for the 16-bit profile, with
artwork, recovery, nominal thermals and bounded observed memory. It does not
close the separate 570 px large-photo tail, watercolor cadence acceptance,
imposed memory pressure, recorder overhead or physical Pencil latency.

## Current iPad sustained watercolor — 2026-09-17

The qualified physical iPad Release has runtime source hashes matching
`eafd1d51` (the retained build was made at `f3a93595` with the export-draft fix).
It completes the existing `wet-watercolor-4k` workload for 45 seconds and then
**600.0083 measured seconds**: 4096² sRGB/U8, eight paint layers plus paper,
320 px Wet Watercolor, prediction and 240 synthetic samples/second. Each launch
uses the ordinary editor and a fresh isolated cache directory. CPU/presentation
recording is enabled; GPU timestamps are disabled. No compiler or other test
runs during either measurement.

| Measurement | 45 seconds | 600 seconds |
| --- | --- | --- |
| Long continuous-active intervals at 120 Hz, including 5% tolerance | 213/4,865 (4.378%) | 4,121/63,405 (6.499%) |
| Active presentation p99 / maximum | 16.667 / 16.667 ms | 16.667 / 25.000 ms |
| CPU owner-service p99 / maximum | 9.158 / 9.996 ms | 9.190 / 17.454 ms |
| Accepted nonpredicted samples | 10,131 | 135,003 |
| Measured peak footprint | 2.113 GB | 2.309 GB |

Both measured windows have no rejected input, missing/zero-time presentations,
frame errors or dropped trace records. Thermals remain nominal. During the long
run, per-minute long-interval rates range from 5.144% to 7.369%, while CPU owner
p99 remains between 9.170 and 9.221 ms. Measured footprint grows 273 MB from its
first to last sample, but peaks during minute two and declines to a 2.097 GB
peak in minute ten. This is not a memory-pressure test or proof of a permanent
bound. The complete long trace retains startup/postlude costs separately:
203.431 ms maximum owner service, 3.654 GB peak footprint and two zero-time
presentation callbacks, all outside the measured window.

Final watercolor artwork, Navigator and layer previews are reviewed. Both
isolated recovery archives pass the production reader and validation with all
nine layers, at revisions 84 and 431. The workload processes are verified closed.
The ordinary review app is restored at Recovered Drawings with recording disabled;
all eleven artist recoveries, normal settings and pre-existing saved files are
byte-identical to their backups. The other editors are unchanged.
Evidence is retained in `artifacts/apple-ipad-current-qualification-v1/`.

This supplies current sustained iPad evidence, but does not meet a strict 120 Hz
p99 target or establish perceptual acceptance under the user's rare-miss standard.
The lower long-interval rate than the older 12.456% short run is not a controlled
A/B speedup. It does not qualify the separate 570 px G-Pen/large-photo gesture,
other profiles/depths, recorder-off overhead or physical Pencil latency. Keep
the shared renderer and measure remaining costs before choosing another change.

## Integrated renderer GPU follow-up — 2026-09-16

At `a5f57e2c`, the 512-frame 4K watercolor replay reproduces the preceding
tile-copy milestone's final RGBA bytes and all per-frame composition, upload
and page counts. Active GPU median is 4.618 ms versus 4.650 ms; this short,
synchronous Mac replay establishes neither iPad cadence nor a performance gain.

The integrated Release is then installed on the approved physical iPad in
place, with all ten recoveries and 58 existing files preserved. A two-second
Metal capture during the existing 45-second heavy-watercolor workload matches
229 native frames to their observed encoders. GPU active time is median
5.008 ms, p95 7.350 ms and p99 8.038 ms (maximum 9.203 ms). Tile copies,
watercolor transport, scene composition and the viewport remain the dominant
observed costs. There are no invalid active intervals; 73 GPU intervals lack
their CPU encoder in the capture window, so this is diagnostic attribution,
not complete GPU coverage or presentation acceptance. Runtime errors, rejected
input, missing measured presentation callbacks and non-nominal thermal states
are absent. Captures show completed artwork and populated previews.

A local experiment using hardware bilinear filtering within atlas pages is
discarded: 15 display regressions pass, but rotating between different atlas
layouts changes Float32 samples by 0.0017832 in the remaining case. The original
shader and assertions are retained; no candidate was installed. Ordinary iPad
review is restored with recording disabled and all saved artwork unchanged.
Evidence is `artifacts/apple-watercolor-gpu-v2/`. Heavy-watercolor acceptance
remains open; the instrumented run does not replace the timer-off results below.

## Watercolor tile-copy batching — 2026-09-16

A two-second Metal System Trace on the physical iPad identifies interleaved
tile-preservation copies and draws as a substantial part of the remaining work.
The trace matches all observed encoders in 220 native frames; their GPU active
interval union has median 4.918 ms and p99 7.650 ms. Blits occupy 322.780 ms of
the complete observed window, transport 247.033 ms, viewport 241.617 ms and
composition 199.582 ms. These label totals overlap and must not be added as
cost shares. The trace has 38 GPU intervals without CPU encoder metadata and
intervals outside matched frames; it diagnoses work, not performance acceptance.

Persistent and predicted brush batches now preserve independent tile generations
before drawing them. Watercolor transport does the same for its first stage.
Source neighborhoods are still prepared and consumed one draw at a time so the
bounded cache cannot evict a pending binding. Generation changes, scissor bounds,
shader math and memory limits are unchanged. A shared whole-surface copy helper
replaces the repeated copy descriptors.

The 512-frame replay remains byte-identical, including all per-frame work counts
and the 4096-square RGBA capture. Against render-pass batching alone, active-frame
median encoded GPU span improves from 5.068 to 4.650 ms, p99 from 9.351 to
7.279 ms, and preparation plus completion median from 8.276 to 7.751 ms.
A 23.781 ms preparation outlier remains; the synchronous replay is not a
presentation or physical input-latency measurement.

Both Release builds pass with empty build logs. Each physical host completes
45 measured seconds of the same full-editor `wet-watercolor-4k` workload with
optional GPU queue timing disabled:

| Host | Long active intervals | Previous copy ordering | Preparation p99 / max, ms |
| --- | ---: | ---: | ---: |
| Mac, 90 Hz | 39 / 3,786 (**1.030%**) | 2.082% | 5.666 / 7.963 |
| iPad, 120 Hz | 582 / 4,523 (**12.868%**) | 15.740% | 4.258 / 8.806 |

Both have nominal thermals, no rejected input, renderer errors, dropped records
or missing/zero measured presentation callbacks. Reviewed captures retain the
artwork, Navigator and layer previews. This is a short improvement comparison;
the remaining iPad heavy-watercolor gap and sustained qualification stay open.
Event recording remains enabled, so it does not qualify full instrumentation
overhead. Together with render-pass batching, the change removes 130 net lines
of production code without new shaders, caches or scheduling paths.

All ten iPad recoveries and 57 pre-existing files remain intact after installation
and testing; only the workload trace is added. Ordinary review is restored with
recording disabled. The Mac artist review is not restarted.

The complete renderer library run reports **249 passed, 10 failed, 28 ignored**.
All ten failures reproduce individually on published `cd4134cd`: four scalar
canonical-float checks, four cache/upload accounting checks, one snapshot
allocation-accounting check and the filter reference atlas. The filter image and
difference report are identical between baseline and candidate. These are open
baseline qualification issues; the full suite is not a pass, and no assertion
or reference image was weakened. Evidence is in
`artifacts/apple-watercolor-gpu-v1/ipad-profile/` and `batched-copies/`.
The subsequent [renderer qualification](../../docs/development/apple-filter-qualification.md#current-renderer-follow-up--2026-09-16)
corrects nine test assumptions and reports 259 passes, with the unchanged Linux
atlas as the sole failure. Production rendering and these performance results
are unchanged; the independent Metal images pass scoped perceptual review.

## Watercolor render-pass batching — 2026-09-16

Consecutive ordinary, effect and watercolor draws to the same target now share
the existing scene render pass. The separate watercolor pass encoder is removed;
uploads, copies and target changes still end a batch. Brush shaders, input,
memory limits and native scheduling are unchanged.

A fresh 512-frame replay against `cd4134cd` preserves all damage/upload/page
counts and every byte of the 4096-square RGBA capture. Across 360 active frames,
median encoded GPU span drops from 5.703 to 5.068 ms; preparation plus completion
drops from 9.055 to 8.276 ms. One candidate CPU outlier remains in the evidence.
All 45 nonignored layer-composition regressions and 13 watercolor regressions
pass; 12 separate opt-in hardware benchmarks remain excluded. Both Release
builds pass with empty build logs.

Each full-editor run measures 45 seconds of `wet-watercolor-4k`:

| Host | Optional GPU queue timer | Long active intervals |
| --- | --- | ---: |
| Mac, 90 Hz | Enabled | 66 / 3,741 (**1.764%**) |
| Mac, 90 Hz | Disabled | 78 / 3,747 (**2.082%**) |
| iPad, 120 Hz | Enabled | 698 / 4,392 (**15.893%**) |
| iPad, 120 Hz | Disabled | 688 / 4,371 (**15.740%**) |

The preceding runtime measured 3.688%/17.929% with queue timing enabled.
All four current runs have nominal thermals, no rejected input, frame errors,
dropped records or missing/zero measured callbacks. Captures retain artwork,
Navigator and layer previews. The short timer comparison does not explain the
remaining iPad gap. Event recording stays enabled in both modes, so these runs
do not establish full recorder overhead or physical pen-to-screen latency.
Sustained heavy-watercolor acceptance remains open, especially on iPad.

The iPad update preserves ten recoveries and all 54 existing files. A subsequent
preflight detects a new background app process; its saved state is backed up
before testing continues. All 55 files present before the second workload remain
identical afterward, with only that workload's trace added. Ordinary review is
restored with recording disabled; the Mac review app is not restarted. Exact
source/build identities and evidence are in
`artifacts/apple-watercolor-gpu-v1/merged/`. This joins tile-copy batching in the
grouped performance milestone.

## Watercolor composition passes — 2026-09-16

The shared compositor unnecessarily rendered aligned watercolor into a temporary
page, copied that page into the layer, then blended the layer into its backdrop.
It now writes aligned pages into the layer target directly. A full-opacity normal
watercolor layer with no intervening mask can use its existing source-over
pipeline on the backdrop, eliminating the second intermediate. Translated,
masked, clipped and partial-opacity composition retains the required operations.
No brush algorithm, cache limit, source samples or native scheduling changes.

The existing `layered-strokes` example now accepts optional `gpu` timing and an
RGBA capture path. A 512-frame replay has identical per-frame damage, uploads
and paint-page counts, with an exact match across the 4096-square RGBA capture.
For the 360 active frames after the first stroke, median encoded GPU span falls
from 7.217 to 5.540 ms and median preparation-plus-completion from 11.096 to
8.841 ms. These synchronous renderer observations exclude native UI/presentation;
encoded spans can include gaps between GPU commands and are not GPU busy time.
A short Metal profile identified watercolor transport and composition work among
444 small command buffers. Its retained 46.923 ms window has incomplete encoder
coverage; it is diagnostic evidence, not complete per-frame attribution.
Shader sampling changes and a general normal-blend experiment were discarded
because they showed no material improvement.

Both final Release apps complete 45 measured seconds of `wet-watercolor-4k`
with ordinary warm-up/postlude, the full workspace and optional queue timing:

| Host | Long active intervals | Presentation p99, ms | Preparation p99 / max, ms |
| --- | ---: | ---: | ---: |
| Mac, 90 Hz | 136 / 3,688 (**3.688%**) | 22.222 | 7.142 / 9.027 |
| iPad, 120 Hz | 774 / 4,317 (**17.929%**) | 16.667 | 4.517 / 5.616 |

The preceding runtime measured 12.803%/28.347%. Both final runs have nominal
thermals, no rejected input, renderer errors, dropped records or missing/zero
measured callbacks. Two Mac and one iPad zero callbacks occur outside measurement.
Reviewed captures retain artwork, Navigator and layer previews. This improves
heavy-watercolor cadence but does not close its remaining performance gap,
particularly on iPad; no ten-minute failing workload is repeated.

All 13 existing watercolor Metal regressions and both Release builds pass. The
checks include masks, prediction, pigment/wetness independence and extended native
color. Before iPad benchmarking, the user's new Pencil test is preserved as a
tenth recovery: all 53 pre-existing files match after the workload, with only its
new trace added. Ordinary review is restored with recording disabled. The first
iPad preflight stopped before launch because the user had reopened the review;
that drawing was backed up before the authorized workload. Evidence and exact
source/build identities remain in ignored `artifacts/apple-watercolor-gpu-v1/`.

## Native tile reuse and final display batches — 2026-09-16

The current SDR follow-up reproduces two further sources of drawing stalls.
Native raster tiles already carry a SHA-256 identity over their representation
and exact samples. The decoded GPU cache now uses that identity plus source and
destination color spaces, allowing identical tiles in separate allocations or
revisions to reuse pixels. Its 64-tile residency limit and the existing 16 MiB
upload ceiling stay unchanged. Cache entries retain no history backing.

The renderer-only `layered-strokes wet-watercolor` replay has identical damage
in all 256 frames. Content reuse reduces source misses from 5,207 to zero and
synchronous upload drains from 44 to zero. Preparation p95 falls from 32.598 to
4.799 ms. The example also accepts a frame count to reproduce repeated strokes
without native UI or a wall-clock input producer.

The 2,048-frame replay then exposes a separate terminal display wait. Composition
previously submitted and waited after its sixteenth tile, even when that was the
last tile. It now drains before a seventeenth tile and sends the final batch with
the ordinary frame. Intermediate batches remain bounded at 16 tiles. Across the
1,680 active frames after the first two strokes, preparation p99 falls from
18.626 to 5.644 ms and frames above 11.11 ms fall from 31 to zero, with identical
damage. These preparation/completion observations are not screen presentation.

Content reuse alone completes a physical iPad ten-minute `layered-4k` run:
386/66,967 long active intervals (**0.576%**), presentation p99 8.334 ms and
preparation p99/max 4.112/11.078 ms. All measured input is accepted; there are no
renderer errors, recorder drops or missing/zero measured presentation callbacks.
Thermals stay nominal. Footprint peaks at 1,742.02 MiB, grows 5.91 MiB in the final
two minutes, then falls 196.34 MiB in the postlude. The canvas sleeps 29.90 ms after
pen-up, with no frames in the final five seconds. Two zero-time callbacks precede
measurement; 84 GPU observations are skipped, with no invalid/pending final samples.
This measured build precedes the terminal-display-wait change.

The earlier short iPad baseline has 11.582%/12.004% long intervals with optional
GPU timing on/off. The corrected short result is 0.516%. Mac watercolor improves
from 36.088% to 14.547% with content reuse alone; GPU timing off gives 13.305%,
so that intermediate Mac result remains outside the accepted rare-miss standard.
The CPU-sampled run aborts its producer and is excluded. With the terminal display
change, the full 45-second watercolor runs have **12.803% long intervals on Mac
and 28.347% on iPad**. Preparation p99/max is now 8.818/12.572 ms on Mac and
5.738/6.840 ms on iPad, with no renderer errors or rejected input. These final
heavy-workload cadence results still fail the accepted rare-miss standard. The
remaining GPU/display cost needs separate diagnosis; no ten-minute failing
watercolor run is repeated and no scheduling workaround is added.
Final 45-second layered-ink checks preserve the accepted cadence: 35/3,788 long
intervals (**0.924%**) on Mac and 18/5,043 (**0.357%**) on iPad. Presentation p99
is 11.111/8.334 ms; preparation p99 is 4.648/4.018 ms. Both have nominal thermals,
complete measured callbacks, no rejected input and no renderer/recorder errors.
Their reviewed captures retain artwork, Navigator and layer previews.

Five source-cache tests verify reuse, exact decoding, color interpretation,
eviction, cancellation and backing lifetime. Twenty-four display/cold-paint tests
pass, including exact 16/17/32-tile batch boundaries, complete paper pixels,
navigation, filters, prediction and history. Both Release builds pass without
warnings. The shared renderer also passes its WebAssembly compilation check with
the existing target-specific LLVM compiler and archiver.

Evidence: `artifacts/apple-sdr-device-v1/`, with exact runtime manifests,
`replay-comparison.json`, `content64/` and `final-batch/`. Device updates preserve
all nine recovery drawings and every pre-existing saved file. Workloads use
private cache roots. Physical SDR workflows, other profiles/ProPhoto-U16,
recorder-off resource behavior, calibrated overhead and physical pen latency
remain separately scoped.

## Layered SDR drawing: upload cache correction — 2026-09-16

The corrected sRGB/U8 `layered-4k` Mac Release completes 600 measured seconds
with **511/50,309 long active intervals (1.016%)**, meeting the user's accepted
rare-miss standard for this synthetic workload. The `448c1ee4` baseline had
14,621/35,319 (41.397%). Eight paint layers, prediction and 240 Hz input remain
unchanged. Both captures show correct artwork, Navigator and layer previews.

| Ten-minute Mac run | Long intervals | Presentation p50/p95/p99 ms | Preparation p99/max ms |
| --- | ---: | --- | --- |
| Original SDR | 41.397% | 11.111 / 22.222 / 33.334 | 22.107 / 38.882 |
| Corrected SDR | 1.016% | 11.111 / 11.111 / 22.222 | 8.005 / 13.549 |

The original 16-slot decoded-source cache could not retain neighboring tiles
across eight layers, repeatedly uploading unchanged color. A valid CPU profile
localized most renderer time to source-upload submission and waits. The cache
now retains up to 64 Float32 tiles, independently of staging. Upload accounting
uses actual bytes and removes the redundant count: U8/U16 inputs can use more
of the existing **16 MiB worst-case staging ceiling**. Ordered uploads and
bounded display-composition submissions remain in place. Decoded textures add
at most 48 MiB, with a total decoded-cache ceiling of 66.25 MiB including existing
integer inputs and transfer tables. This is a shared renderer correction.

The `layer-host` example `layered-strokes` reproduces the cause without native
UI or screen presentation. Its fixed three-sample batches produce identical
per-frame damage across all 256 frames. Misses fall from 3,234 to 986 and
synchronous upload drains from 112 to zero. Active preparation p95 falls from
7.479 to 2.331 ms. Cache retention alone left seven drains and 4.491% long
intervals in a short native run; byte accounting reduces the final short run
to 0.906%. These intermediate results remain scoped separately from the completed
ten-minute qualification.

The final sustained run has no rejected input, renderer errors, dropped records,
or missing/zero measured presentation callbacks. Thermals remain nominal.
GPU queue-span p99/max is 16.192/29.218 ms; this includes CPU gaps, not solely
GPU execution. Measured footprint peaks at 2,475.72 MiB, falls 30.69 MiB in the
last two minutes and another 234 MiB during the postlude. The canvas becomes
idle 45.10 ms after pen-up, with no frames in the final five seconds. No memory
workaround is justified by this curve; other device/workload budgets remain open.

All 12 source-cache, precision, native paint, source-preservation, prediction,
transform and exact-history regressions pass. Both macOS and iPadOS Release
builds pass without warnings. The separate source microbenchmark remains ignored.
The measured Mac binary matches every retained source hash for `448c1ee4` plus
this milestone's runtime patch. Runs use an isolated identity and storage;
existing review/artist processes and drawings are preserved. The first short
profile aborted its producer and mostly sampled idle time; it is invalid evidence.

Evidence: `artifacts/performance/sdr-448c1ee4/`, including baseline and final
`byte-budget-native/mac-layered-4k-600-gpu-1/`, the valid intermediate
`mac-layered-4k-45-gpu-1-profile2/cpu-sample.txt`, renderer CSVs and `review.json`.
Current iPad/other-profile and ProPhoto/U16 performance, recorder-off behavior,
instrumentation overhead and physical input-to-display measurement remain open.
CPU/GPU timestamps and synthetic input association are not physical pen latency.

## Recurrent physical Pencil stall — 2026-09-15

The user confirms lag after the revised timer restart. The actual Pencil trace
under `artifacts/apple-diagnostics-recurrence-v1/` records 1,234 real batches,
1,185 predicted batches and zero correction batches. Owner-frame p99 is
481.19 ms; drawable-acquisition p99 is 445.78 ms. Continuous presentation
interval p99/max is 509.80/620.84 ms, with nominal thermal state and no renderer
errors. These are unacceptable visible stalls, independent of the relaxed
rare-refresh-miss criterion. Diagnostics reports 190,850 dabs across 301 frames
and GPU p99 144.18 ms. The earlier synthetic probes did not cover the failing
physical update path and must not be used as acceptance of this build.

The candidate simplifies Pencil update correlation to UIKit's unique monotonic
index, removing an extra timestamp condition that can discard delayed updates.
A stranded estimate can keep the growing active stroke in the replaceable
preview. Original sample time/phase and partial property updates remain intact.
A bounded Mac Metal reproduction uses the same 720-point, 120.7 px Rough G-Pen
stroke with correction deliveries present or absent. Corrected input produces
4,549 preview dabs and frame p99 3.06 ms; dropped corrections produce 162,943
preview dabs and frame p99 173.86 ms. This establishes the cost of unresolved
estimates, not the exact callback values of the failing physical session: the
live trace records accepted correction batches, not raw UIKit callbacks.
The focused UIKit callback fixture fails on the old implementation when the
update's timestamp differs; all four groups pass on physical iPad with the fix.
It verifies partial/final updates, preserved original observation time, retired
indices and clean completion. The corrected Release build has zero warnings and
is restored with the original review artwork. The live drawing and trace are
preserved. Physical Pencil acceptance remains open.

## Captured Pencil root cause and bounded preview — 2026-09-15

The user reports that disabling Stroke Prediction removes lag, while enabling
it lags with iPadOS Prediction either on or off. The earlier index-matching fix
did not resolve the physical problem. Two subsequent capture attempts failed
before export and are excluded. The replacement recorder was verified writing
a growing local file while the app stayed open, then captured the user's full
14.12-second Pencil stroke: 3,390 real samples, 4,611 predicted samples and no
correction callbacks. All real samples were awaiting sensor updates; the raw
UIKit expected-property mask was force/roll for 3,389 samples and roll for the
terminal sample. No raw update callback arrived, so timestamp matching cannot
explain this capture. The reason for absent OS updates is not established.

The 29,426-event trace has zero dropped records, nominal thermal state, no
renderer errors and no missing presentation callbacks. Owner-frame p99 is
798.52 ms, drawable-acquisition p99 is 796.44 ms and continuous presentation p99
is 833.97 ms; CPU prepare p99 is 2.00 ms. Actual-input engine replay proves that
`finalized_real_points` stays zero for the entire contact. Each prediction frame
therefore regenerates and rerenders the full growing stroke. This unbounded
preview is the application defect, regardless of whether the OS later resolves
its estimated sensor values.

The shared engine now limits unresolved samples' preview retention to the
existing 50 ms maximum feedback window. It keeps correction tokens and honors
late updates using the existing persistent-ink rebuild. The regression test
fails before the change and passes afterward, including both prediction-source
settings, late pressure correction, exact finished dabs and one-step history.
All 53 engine tests pass. The native Metal final-sensor oracle now delays the
partial correction to 80 ms; G-Pen, Pencil, watercolor and smudge pass on both
Apple policies, including backing/composited pixels and exact Undo/Redo.

| Actual-input replay | Before | After |
| --- | ---: | ---: |
| Regular 120 Hz: maximum unfinished real samples | 3,389 | 13 |
| Regular 120 Hz: total preview dabs | 5,335,713 | 48,752 |
| Regular 120 Hz: maximum preview dabs per frame | 6,010 | 194 |
| Captured frame schedule: total preview dabs | 164,400 | 5,833 |
| Captured frame schedule: Metal frame p50, ms | 14.62 | 0.91 |
| Captured frame schedule: Metal frame p95, ms | 1,006.37 | 9.65 |
| Captured frame schedule: Metal frame p99, ms | 1,392.12 | 30.54 |
| Captured frame schedule: Metal frame maximum, ms | 1,590.66 | 41.34 |

These replays use recorded numeric samples, tokens, timestamps and receipt times.
The Metal comparison runs on Mac, renders 202 frames using the captured admission
schedule and waits for GPU completion; its times include CPU and GPU work. The
retained 72% view and brush settings are reconstructed, not raw camera telemetry.
The original stalled schedule still batches large quantities of input. With
prediction disabled, control p99 is 27.15/25.67 ms before/after. These comparisons
isolate the runaway preview; they do not establish iPad presentation acceptance.

Private evidence is under `artifacts/apple-lag-stream-v3/`; the production Release
build and restored review app are tracked in `artifacts/apple-prediction-bounded-v1/`.
Both Apple Release builds pass without warnings. The production iPad review app
is restored in place at Recovered Drawings with its artwork preserved, temporary
numeric recorder removed and tracing disabled. The user confirms **smooth drawing
in both cases** with Stroke Prediction enabled and iPadOS Prediction off/on. This
closes the reported physical stall. No failed recording or synthetic-only pass
is counted as physical acceptance; broader sustained measurements retain their
original scope.

## Startup progress without presentation callbacks — 2026-09-16

The recorded iPad startup failure reaches canvas/filter readiness but not shader
readiness. Four acquired drawables have no presentation callback, and 15,566
later display ticks are denied by the custom presentation counter. Missing
notifications therefore prevent any further owner frames from advancing startup.
The capture does not establish why the OS notifications were absent.

A focused real AppKit/Metal regression suppresses only drawable presentation
notification registration, leaving acquisition, rendering and presentation intact.
The published implementation stalls after three callbacks are withheld. Removing
the custom counter and callback retry lets both Apple policies reach readiness,
render edited artwork and reproduce exact thumbnail pixels through Undo/Redo,
with 64 and 66 callbacks withheld. The existing driver checks also pass for one
queued frame, intervening input, idle, detach and replacement surfaces.

CAMetalLayer now owns drawable availability. The serial render owner still drains
its autorelease pool per task; the shared driver still permits only one queued
frame. Presentation callbacks are optional recording only. Resume still requests
a redraw. The gate, its waiter/retry plumbing and implementation-only tests are
removed; no timeout or synthetic presentation fallback is introduced.

An unchanged `4b1837a` iPad relaunch after the same in-place update completes
600.000 seconds of 4K watercolor. Its 1,463/65,316 long active intervals (2.240%)
have p99/max 16.667/25.000 ms, with nominal thermals, no rejected input, renderer
errors, missing/zero measured presentation callbacks or dropped records. The
final canvas, Navigator and painted layer thumbnail are correct. This proves the
startup failure is intermittent, not resolved by reinstalling. It is baseline
evidence, not hardware acceptance of the simplified driver.

Both fixed Release builds pass without compiler warnings. Each physical host
then completes 600 seconds of the same 4096², eight-paint-layer, 320 px Wet
Watercolor workload with prediction, 240 Hz supplied input and the full workspace.
Mac reports a 2400×1740 viewport at 90 Hz; iPad reports 2752×2064 at 120 Hz.
Startup and entry into measurement are verified. Both measured intervals have
zero rejected input, renderer errors, dropped records, missing presentation
callbacks and zero-time presentations. Final canvas, Navigator and painted layer
thumbnails are visually correct.

| Fixed Release result | Mac | iPad |
| --- | ---: | ---: |
| Long active intervals / total | 481 / 50,155 (0.959%) | 1,310 / 65,603 (1.997%) |
| Active presentation p50 / p95 / p99 / max, ms | 11.111 / 11.111 / 11.111 / 33.334 | 8.333 / 8.334 / 16.667 / 25.000 |
| Owner CPU p50 / p95 / p99 / max, ms | 2.854 / 4.190 / 10.613 / 14.869 | 2.008 / 4.154 / 9.384 / 19.747 |
| GPU queue span p50 / p95 / p99 / max, ms | 8.927 / 10.610 / 11.462 / 22.350 | 5.762 / 8.458 / 10.205 / 19.456 |
| Frame admission to presentation p99 / max, ms | 43.926 / 66.145 | 25.980 / 34.314 |
| Footprint growth / peak, MiB | 719.44 / 2,299.14 | 238.03 / 2,065.74 |
| Footprint change in final 120 seconds, MiB | −13.94 | −61.12 |
| Canvas asleep after pen-up, ms | 34.651 | 24.202 |

Each postlude has two owner frames and no frames in its final five seconds.
Footprint falls another 273.77/303.72 MiB during the Mac/iPad postlude, and all
thermal samples are nominal. There are 31/18 skipped GPU samples, with no invalid
samples, poll errors or pending samples at the last poll. One Mac and twelve iPad
zero-time callbacks occur before measurement; none are missing. The measured
results fit the accepted rare-miss criterion and do not justify another scheduler
or memory workaround. These separate runs do not establish a speed improvement.

The before/after regression, baseline trace, exact runtime-source manifests,
Release identities and hardware results are retained under
`artifacts/apple-startup-stall-v1/`. Both test processes are closed. The normal
iPad review is restored on the fix with all seven recovered drawings and every
backed-up artist file byte-identical; the original Mac review remains running
unchanged. The optional recorder was enabled for these synthetic workloads.
Recorder-off behavior, instrumentation overhead, physical input-to-display
latency and other unclosed workload/interaction cases remain separate.

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/presentation-progress.swift
```

## Current 4K watercolor with layer previews — 2026-09-16

The published `4b1837a` Mac Release completes 600.001 measured seconds of
`wet-watercolor-4k`: a 4096-square document, eight paint layers, the normal
320-pixel Wet Watercolor brush and 240 Hz synthetic input. The full default
workspace, recovery writer, recorder and GPU queue timing remain enabled.
The final capture shows the painted active-layer thumbnail and matching
Navigator. This provides a sustained result after the document-epoch thumbnail
fix, rather than extending the earlier short capture's scope.

The display reports 90 Hz. Active presentation intervals have p50/p95/p99 of
11.111 ms and a 33.334 ms maximum; 469 of 50,032 intervals (0.937%) exceed the
target tolerance. CPU owner-service p99/max is 5.589/23.401 ms, and GPU queue-span
p99/max is 11.407/27.286 ms. Frame-admission-to-presentation p99/max is
43.869/55.057 ms; this is not physical pen-to-pixel latency. There are no rejected
input batches, renderer errors, dropped records or missing presentation callbacks.
Two measured callbacks return zero presentation timestamps at about 114 seconds,
within one 33.334 ms visible presentation gap. A third zero timestamp precedes
measurement. These remain explicit measurement limits. The run skips 63 GPU
samples, with no invalid timestamps, failed polls or final pending samples.

Thermal samples remain nominal. Footprint grows 793.75 MiB, reaching 2,379.60 MiB;
624.08 MiB of growth occurs in the first 120 seconds and 56.36 MiB in the final
120 seconds. The ten-second postlude releases 304.52 MiB. Two canvas-owner frames
follow measurement, the display link sleeps after 40.9 ms, and no owner frame
occurs in the final five seconds. Recorder capacity is 74.52 MiB and measured
event payload is 39.67 MiB; neither number attributes the footprint increase.
No build, UI test or profiler overlaps the measured Mac workload. The original
Mac review process is preserved and brought forward afterward.

The rare longer intervals alone do not fail the user's acceptance criterion.
This synthetic run does not establish physical latency, recorder-off resources,
instrumentation overhead, long idle/resume behavior or all platform/profile
coverage. Source and binary hashes, raw trace, complete distributions, capture,
zero-timestamp details and memory/idle review remain private under
`artifacts/performance/final-4b1837a/`.

The same Mac executable also completes a 45.009-second `ink-predicted` check,
with correct painted and Paper thumbnails, no rejected input or renderer errors,
and no missing/zero measured presentation callbacks. There are 54 longer active
intervals out of 3,734 (1.446%); p99/max are 22.222/22.222 ms, GPU queue-span p99
is 6.746 ms and thermal samples remain nominal. This is a short current-build
simple-brush result, separate from the earlier ten-minute ink measurement.

The first corresponding physical iPad 600-second attempt is **invalid**. It
never reaches workload setup or delivers drawing input. Startup submits fourteen
viewports, records ten zero-time presentation callbacks and leaves four callbacks
missing; subsequent ticks are denied drawable capacity. No ready shader state or
actual presentation is observed before the workload's 120-second startup timeout.
The final capture shows its explicit workload error. A following short probe of
the unchanged Release reaches readiness in 0.688 seconds and shows actual ink
before any background/return intervention, so it does not reproduce or explain
the earlier stall. Backgrounding then interrupts the synthetic producer, as
expected; that probe is not performance acceptance. The startup failure remains
open. Do not change rendering or scheduling from an assumed cause, or repeat a
long iPad run without verifying that measurement actually starts.

A separate uninterrupted iPad `ink-predicted` run completes 45.001 measured
seconds on the same Release, with correct painted/Paper thumbnails and Navigator.
At the reported 120 Hz, 55 of 5,006 active intervals (1.099%) exceed tolerance;
p50/p95/p99/max are 8.333/8.334/12.499/25.000 ms. GPU queue-span p99/max are
9.230/14.618 ms. There are no rejected input batches, renderer errors, dropped
records or missing/zero measured presentation callbacks; thermal samples remain
nominal. This confirms a working subsequent startup and short drawing session,
not resolution of the failed startup or a sustained iPad pass.

## Current Mac ink and watercolor — 2026-09-16

The isolated Release built from `d96a0a9` completes 600 measured seconds each
of `ink-predicted` and `wet-watercolor`, with ten-second warm-up and postlude.
Both use the same executable, normal brush settings, 240 Hz synthetic input,
the full editor and fresh private storage. The display reports 90 Hz. The
recorder and optional GPU queue-span timing are enabled; no build, UI test or
profiler runs during either measurement.

| Mac profile | Long active intervals / total | Interval p99 / max, ms | CPU owner p99, ms | GPU queue-span p99, ms |
| --- | ---: | ---: | ---: | ---: |
| Predicted ink, 2048² | 431 / 50,313 (0.857%) | 11.111 / 22.222 | 3.290 | 7.140 |
| Wet Watercolor, 2048² | 473 / 50,381 (0.939%) | 11.111 / 33.334 | 5.743 | 11.216 |

Both intervals have zero rejected input, renderer errors, dropped records,
missing presentation callbacks and zero-time presentations. Each trace has one
zero-time presentation before measurement. Watercolor skips nine GPU samples
during measurement (twelve across the whole trace); no invalid GPU sample,
failed poll or final pending sample is recorded. Queue spans include submission
gaps and profiler work; they are not isolated GPU execution. Frame-admission to
presentation p99 is 39.917/32.834 ms, which is not physical pen-to-pixel latency.
These rare long intervals alone do not fail the user's perceptual criterion.

Measured footprint grows 232.55/758.89 MiB, with maxima of 545.55/1,385.47 MiB.
Growth in the final 120 seconds is 20.06/67.14 MiB; watercolor's first window
grows 484.39 MiB. The curves are not monotonically flat and do not prove
indefinite stability. The postlude releases 32.66/181.30 MiB. Both runs record
nominal thermal state; they provide no fresh iPad evidence.
Each run has two postlude canvas-owner frames, sleeps the display link after
50.1/41.0 ms and records no canvas-owner frames in the final five seconds.
The recorder reserves 74.52 MiB and appends 40.91/39.86 MiB of event payload
during measurement; payload bytes do not attribute physical footprint growth.

Both reviewed captures show the expected artwork and Navigator but blank layer
thumbnails. A subsequent focused test reproduces stranded preview readbacks when
the benchmark replaces its startup document: the cache retains a request owned
by the retired renderer. Normal file-dialog New/Open already reset the cache.
Moving that reset to coherent document publication, alongside renderer changes,
removes the file-dialog-specific call and covers every replacement path. The
regression fails before the change and passes for both Apple policies afterward;
it mounts AppKit/Metal, not UIKit. Both Release builds pass without warnings.
A short final Mac Release capture shows the painted/checkerboard active-layer
thumbnail and white Paper thumbnail. Evidence is `artifacts/apple-thumbnail-epoch-v1/`.

These ten-minute drawing measurements retain their pre-fix source and missing-
thumbnail qualification. The later `4b1837a` 4K watercolor result above provides
a sustained post-fix check; it does not relabel these two earlier profiles.
No scheduler or memory workaround follows these observations.
Recorder-off resources, storage, idle/resume, instrumentation overhead, physical
input latency and remaining platform/profile coverage are still open. Evidence
is `artifacts/performance/final-d96a0a9/`, including source/binary hashes, summaries,
captures and the supplemental memory/idle review.

## Sustained memory and idle review — 2026-09-16

Read-only reanalysis of the retained `fb81ebe` ten-minute `layered-4k` pair
confirms completed measurement/postlude, nominal thermal samples, no rejected
input or renderer errors, and no missing/zero measured presentation callbacks.
Long intervals are 909/49,837 (1.824%) on Mac and 727/66,919 (1.086%) on iPad.
Those counts alone do not fail the user's revised perceptual criterion and do
not justify reopening scheduling experiments. They are not fresh-build results.

The recorded footprint growth slows rather than remaining constant:

| Measured window | Mac growth, MiB | iPad growth, MiB |
| --- | ---: | ---: |
| 0–120 seconds | 80.44 | 63.23 |
| 120–240 seconds | 85.53 | 72.47 |
| 240–360 seconds | 77.64 | 95.13 |
| 360–480 seconds | 52.59 | 46.48 |
| 480–600 seconds | 22.58 | 3.48 |

Whole-interval growth remains 326.47/291.28 MiB. The recorder adds 30.59/34.27 MiB
of event payload during measurement, within its 74.52 MiB reservation; payload
bytes are not a physical allocation attribution. The shared history implementation
is unchanged from the recorded source and already caps retained history at 256
entries and 512 MiB of additional accounted data. These facts do not identify
every allocation or prove indefinite stability, but they do not establish an
unbounded leak or justify a new memory-retention workaround.

Across the ten-second postlude's memory samples, footprint drops 60.64 MiB on
Mac and 156.30 MiB on iPad. Each host records only two more canvas-owner frames;
the display link sleeps 49.2/40.8 ms after measurement ends, and neither records
canvas-owner frames during the final five seconds. This qualifies that recorded
idle transition, not a longer recorder-off idle/resume or storage-growth test.

Retain these scoped results for final acceptance. Short profiles containing the
reverted Layers observation experiment and the fixed-minimum-cadence ink
candidate remain separately labeled. Final source-qualified workload coverage,
recorder-off resource behavior, instrumentation overhead and physical
input-to-pixel measurement remain open. No new benchmark or product change was
made for this review. Evidence is `artifacts/apple-performance-closure-v1/`.

## Remaining short workload profiles — 2026-09-15

The three smaller profiles now complete on both physical hosts with the full
default editor, unchanged brushes and 240 Hz synthetic input. Each has 45 measured
seconds after warm-up and a completed postlude. GPU timing is disabled; no builds,
UI tests or profiler run during measurement. The two physical hosts run some
profiles concurrently; device installation briefly overlaps the first Mac run.

| Host / profile | Long intervals / total | Interval p99, ms | CPU owner p99, ms |
| --- | ---: | ---: | ---: |
| Mac ink | 70 / 3,728 | 22.222 | 3.303 |
| Mac predicted ink | 40 / 3,779 | 22.222 | 3.198 |
| Mac watercolor | 50 / 3,745 | 22.222 | 6.059 |
| iPad ink | 38 / 5,061 | 8.334 | 7.230 |
| iPad predicted ink | 3 / 5,031 | 8.334 | 2.305 |
| iPad watercolor | 89 / 4,962 | 16.667 | 9.133 |

All six have nominal thermal state and no renderer errors, rejected input,
overflow, missing callbacks or zero-time measured presentations. Mac's retained
cadence problem also occurs in single-layer ink; it is not confined to 4K
multilayer rendering. iPad's ink p99 fits its refresh budget, while watercolor's
does not. Every run retains some longer intervals. Short-run memory growth is
2.6–21.2 MiB for ink and 143–169 MiB for watercolor; these observations do not
establish sustained memory behavior or attribute its cause.

These results fill short-profile coverage, not sustained acceptance or physical
input latency. They include the unpublished Layers observation experiment below.
Review found absent layer thumbnails in both Mac ink captures, while watercolor
and a short published-build comparison show previews. The observation change is
reverted to the established revision refresh rather than adding another refresh
path. The six measured binaries, hashes, reports and captures remain in ignored
`artifacts/performance/remaining-workloads-v1/`; the restoration and visual check
are under `artifacts/apple-layer-thumbnail-followup-v1/`. Both restored Release
builds pass without warnings, and the short Mac comparison shows the painted
layer preview and white Paper thumbnail. No cadence improvement
is claimed from the restoration, and the six measurements are not relabeled as
results from the restored source.

## Editor composition and update scope — 2026-09-15

The existing 45-second eight-layer 4K workload is compared with the normal Mac
editor and with Zen activated through its normal header button during warm-up.
GPU timestamps, compilers and UI tests are inactive during measurement. Both
conditions retain the 2400 × 1740 drawable and 90 Hz display. These are short
synthetic comparisons, not physical-input or sustained acceptance.

| Mac condition | Long continuous intervals / total | Continuous interval p99, ms |
| --- | ---: | ---: |
| Published app, normal editor | 61 / 3,764 | 22.222 |
| Published app, Zen | 19 / 3,767 | 11.111 |
| Layer-hosting correction, normal editor | 48 / 3,745 | 22.222 |
| Layer-hosting correction, Zen | 21 / 3,793 | 11.111 |
| Narrower Layers observation, normal editor | 49 / 3,743 | 22.222 |

Mac now assigns its Metal layer before enabling `wantsLayer`, following
[AppKit's layer-hosting contract](https://developer.apple.com/documentation/appkit/nsview/wantslayer).
The first candidate also includes the small Tool Settings heading style change.
Neither is established as a cadence fix. All sampled unpainted paper pixels
agree across the four original/candidate captures; no paper-loss bug is proven.

The subsequent experiment changes the Layers panel's observation from the
global editor revision to layer state. Rename and document replacement retain
their existing invalidation. The final normal-editor run does not demonstrate
a cadence benefit, and the thumbnail follow-up above subsequently reverts this
change. Both experimental Release builds have zero compiler warnings. The existing
row suite passes both Apple policies, and the Mac layer setup passes all
twenty-two existing assembled canvas workflow groups.

All five intervals and postludes complete with nominal thermal state and no
renderer errors, rejected input, recorder overflow or missing/zero-time measured
presentations. The Zen results justify investigating native editor composition,
but do not identify a particular panel or establish an optimization. The current
normal editor still misses the 90 Hz cadence requirement. No new scheduler,
iPad workload or ten-minute rerun follows these negative implementation results.
Evidence, retained baseline/candidate apps, captures and Release metadata are
under ignored `artifacts/performance/editor-composition-v1/`.

One focused Animation Hitches trace of that current Mac Release app records
no OS-classified hitches. The app recorder still reports 35 long continuous
intervals over the full 45-second workload; profiling makes this a diagnostic
run, not clean cadence acceptance. Six of the sixteen long intervals within
the profile contain no intermediate screen presentation. Seven contain an
intermediate screen presentation associated with an app UI update. These are
clock-aligned associations, not proof of a particular cause. The largest recorded
app update is 2.166 ms, and the CPU samples do not establish an expensive panel
or a large main-thread stall. Thumbnail queries already suppress unchanged
snapshots in the Rust publication path. No publication/scheduling workaround
follows. Both owned processes exit, and the workload/postlude complete without
renderer errors, rejected input or missing measured presentation callbacks.
The retained trace, exports and reproducible correlation are under ignored
`artifacts/performance/editor-hitches-v1/`. Resume other concrete blockers until
new evidence supports a specific performance change.

A bounded recovery-observation experiment also fails to improve cadence.
Replacing its broad `ObservableObject` publication with property observation
records 63 long intervals out of 3,747, with a 22.222 ms p99. The retained
pre-change run has 49 out of 3,743 and the same p99; this single comparison does
not establish a regression. Both Release builds and the complete local recovery
suite pass, but the source change is rejected and reverted. The original Mac
executable is restored and iPad Release rebuilt without compiler warnings.
Evidence and rejected source remain under ignored
`artifacts/performance/recovery-observation-v1/`. No native recovery workflow
is repeated for this discarded change.

## GPU endpoint correlation — 2026-09-15

The optional recorder now retains the existing GPU marker endpoints and samples
paired Metal clocks. This is an observation change only; rendering, admission
and presentation scheduling are unchanged. Both Release builds pass without
compiler warnings, as do the actual Metal timer test, native timing bridge
check, Swift recorder fixture and seventeen analyzer tests. The analyzer keeps
sampling uncertainty, missing observations and the existing cadence criteria.

One 45-second `layered-4k` run with GPU timing enabled and one with it disabled
complete on each physical host, serially after builds. All four measured
intervals and postludes complete with nominal thermal state, no rejected input,
renderer errors, recorder overflow or missing/zero-time measured presentations.
The complete traces retain setup zero-time callbacks: one per run except two
in the iPad timing-disabled run. Both Mac artwork/Navigator captures are reviewed.

| Measurement | Mac timing on / off | iPad timing on / off |
| --- | ---: | ---: |
| Actual measured presentations | 3,798 / 3,752 | 5,029 / 5,041 |
| CPU owner median, ms | 2.377 / 2.207 | 1.830 / 1.575 |
| CPU owner p99, ms | 4.115 / 4.077 | 9.092 / 9.131 |
| Long continuous intervals / total, timing on | 54 / 3,769 | 31 / 5,000 |
| Long continuous intervals / total, timing off | 70 / 3,723 | 43 / 5,012 |
| Calibrated measured GPU frames, timing on | 3,798 / 3,798 | 5,028 / 5,029 |

All 54 Mac long intervals have a right-hand frame whose GPU end marker precedes
its presentation target, by at least 1.85 ms. On iPad, 26 of 31 do; the other five
end 0.080–0.918 ms after target and were admitted only 1.027–3.059 ms before it.
Their CPU owner work is 1.393–1.990 ms. All long-interval endpoints are calibrated;
the one skipped iPad GPU observation remains missing elsewhere in measurement.
Recorded clocks are monotonic, and no calibrated GPU start precedes its frame's
admission. Maximum measured sampling-window uncertainty is 0.011 ms on Mac and
0.026 ms on iPad. Unknown clock drift is not included in those bounds.

Late GPU completion does not explain most observed gaps in these instrumented
runs. A presentation target is still not a Metal commit deadline, so this does
not establish a compositor or scheduling root cause. The single on/off pair
per host does not calibrate total recording overhead or prove a cadence benefit;
both cadence gates still fail. No renderer/scheduler workaround or ten-minute
rerun follows. Resume feature/state acceptance rather than repeating these
measurements without a new, actionable hypothesis.

A read-only follow-up reuses these four traces to check delayed main-thread
completion as an explanation for owner-pending denials. Neither timing-disabled
run has a denial after recorded owner completion. With timing enabled, Mac has
two such denials, only one between the admissions of long-interval endpoints;
iPad has none. The counts reproduce the analyzer's original cadence failures.
This does not support adding another admission gate or completion workaround.
The script and per-event results are under ignored
`artifacts/performance/input-state-followup-v1/`; no new workload or runtime
change accompanies this analysis, and the root cause remains unproven.

Evidence, source hashes, comparison reports, reproduction scripts and that
investigation's Release metadata are under ignored
`artifacts/performance/gpu-clock-correlation-v1/`. All workload processes are
closed. The disposable iPad app is removed, its original stopped test runner is
restored, and both artist editor descriptors are unchanged.
The subsequent OS file-launch startup fix rebuilt both apps; its Release
metadata is under `artifacts/apple-os-file-launch-v1/after/`. That fix changes
Open admission only, so no drawing workload is repeated and no new performance
claim is made.
The [handoff](../../docs/development/apple-handoff.md) identifies the latest
on-disk Release builds; later feature/lifecycle builds do not add performance
evidence unless a corresponding workload is recorded here.

## Coverage preparation cleanup and physical validation — 2026-09-15

Review of the retained command-encoding profile identifies redundant coverage
work in the shared renderer. Single-batch destination prediction already reads
committed coverage directly, but still allocated and initialized a private
coverage pair. The existing prediction-mode decision now retires that unused
pair. Multiple-batch and watercolor prediction retain their private coverage.
New persistent coverage pages also no longer receive two preliminary clear
passes: their stroke-owner transition clears the active surface, and the normal
batch copy initializes the inactive surface before rendering. The duplicate
initialization flags and loops are removed; no new rendering path is added.

A focused Metal regression first reproduces two unused coverage pairs across a
two-page prediction, then passes after the cleanup: prediction storage falls
from 768 to 512 KiB. Full-image equality holds when returning from a multiple-batch
preview, cancelling, recreating the preview and committing its ink. This is a
resource reduction, not a timing or presentation-cadence measurement.

All thirteen final Metal checks pass, including 120 material full-image
comparisons with zero channel difference, stroke-coverage reset, watercolor
prediction/pen-up, contact invariants and project/mask save/reopen/Undo/Redo.
Evidence is under ignored `artifacts/performance/preview-coverage-retirement-v1/`.
The cleanup accompanies the brush-draft milestone. Both final Release builds
pass, and the rebuilt apps complete one existing 45-second eight-layer 4K G-Pen
workload per physical host, with prediction and 240 Hz synthetic input. The runs
are serial and follow all builds/UI automation, with GPU timestamps and CPU
sampling disabled. Both measured intervals and postludes finish with nominal
thermal state, no rejected input, renderer errors, overflow or missing/zero-time
measured presentation callbacks. The Mac artwork/live Navigator is reviewed.

| Measurement | Mac, 90 Hz | Physical iPad, 120 Hz |
| --- | ---: | ---: |
| Measured seconds | 45.009 | 45.000 |
| Actual presentations | 3,777 | 5,075 |
| CPU owner p99 / max, ms | 3.733 / 14.886 | 9.097 / 17.692 |
| CPU frames over host budget | 28 | 67 |
| Long continuous intervals / total | 49 / 3,748 | 41 / 5,046 |
| Continuous interval p99 / max, ms | 22.222 / 22.222 | 8.334 / 25.000 |
| Measured footprint growth / peak, MiB | 51.88 / 1,276.97 | 22.02 / 1,299.80 |

Both cadence gates still fail. These current short runs are not a controlled
before/after comparison and do not establish a timing or memory-growth benefit.
No ten-minute run follows. Sustained cadence, physical-input latency, isolated
GPU timing, recorder overhead and the complete performance matrix remain open.
Grouped native evidence and that milestone's Release metadata are under
`artifacts/apple-brush-state-milestone-v1/` (`physical/`, `release/`). Both owned
processes are stopped; the physical diagnostic is removed and its prior test
runner restored, with the artist's editor descriptors unchanged.
The subsequent workspace-manager cleanup rebuilt both Release apps; its metadata is under
`artifacts/apple-workspace-manager-cleanup-v1/release/`.
That milestone did not affect the renderer or add a physical drawing run.
The newer diagnostic builds and recordings are described above.

## Hardware tile hashing, short physical comparison — 2026-09-15

The retained valid CPU profile identifies software SHA-256 work inside
`RasterCapture.finish` workers calling `TileBlob::encode`. The installed sha2
0.10 dependency gates its runtime-detected AArch64 SHA instructions behind its
`asm` feature. Enabling that existing feature only on AArch64 avoids another
hash implementation, tile-format change or capture-scheduling change. Wasm and
Intel dependency resolution retain their previous features.

A local Release benchmark encodes real 256-square paint and mask tiles with
solid, diagonal-stroke and deterministic-noise contents. Two runs per variant
use baseline/accelerated/accelerated/baseline order, each with 16 warm-up and
512 measured encodes per case. The six cases improve 4.5–5.6 times; every case
retains identical digests and compressed bytes and passes decoding. This measures
CPU tile encoding with output checks, not whole-app throughput or drawing cadence.

The 48 default core/workspace checks, 86 native workspace checks, iOS core
compilation and both Release app builds pass; the builds have no compiler
warnings. A 45-second before/after pair on each physical host uses the existing
eight-layer 4K G-Pen workload, 240 Hz synthetic input, prediction and ten-second
warm-up/postlude. GPU timestamps, CPU sampling and UI automation are disabled.
The Mac baseline precedes compilation and the changed run follows both builds.
The physical iPad baseline overlaps compilation on the Mac.

| Measurement | Mac before | Mac after | iPad before | iPad after |
| --- | ---: | ---: | ---: | ---: |
| CPU owner p99 / max, ms | 3.922 / 15.762 | 3.878 / 15.338 | 8.892 / 17.654 | 8.776 / 10.481 |
| CPU frames over host budget | 28 | 28 | 57 | 56 |
| Long continuous intervals / total | 57 / 3,734 | 49 / 3,772 | 28 / 5,017 | 18 / 5,008 |
| Continuous interval p99 / max, ms | 22.222 / 22.222 | 22.222 / 22.222 | 8.334 / 25.001 | 8.334 / 25.000 |
| Peak measured footprint, MiB | 1,262.58 | 1,324.11 | 1,299.03 | 1,310.92 |
| Measured footprint growth, MiB | 5.95 | 9.81 | 10.86 | 8.70 |

All four intervals and postludes complete, with nominal thermal state and no
rejected input, renderer errors, recorder overflow, or missing/zero-time measured
presentations. Mac captures show the expected artwork and live Navigator.
The unchanged presentation p99 and these single short pairs do not establish
reliable cadence or memory improvement. Both cadence gates still fail; no
ten-minute rerun follows. Physical-input latency, isolated GPU execution,
recorder overhead and sustained memory behavior remain unqualified.

This optimization accompanies the Settings text milestone. Evidence,
exact source/build hashes, original benchmark binaries and native trace summaries
are ignored under `artifacts/performance/tile-sha-acceleration-v1/`. Rebuilding
the benchmark's baseline against the changed core manifest would also enable
acceleration; retain the original binaries/results for any future comparison.

## Current contact renderer, eight-layer 4K ink — 2026-09-14

A later 20-second CPU sample of the retained milestone Release app completes
during the same 45-second workload. The main render-owner branch is command
encoding/finish in wgpu; snapshot publication and history trimming contribute
much less sampled work. The profile also identifies an unnecessary scan of all
paint pages in destination-companion preparation. That helper now visits only
the persistent destination-reading batches' damaged pages, removing two temporary
collections without another rendering path. Fourteen existing Metal contact,
project/history, destination-brush and sparse-page checks pass, as do both Apple
Release builds. A frame-rate improvement is not established by these checks.

Two other sampling attempts abort the workload and are invalid: the initial
collector also exited before its sampler report was ready; the post-change
report is mostly idle samples after the abort. Do not compare those samples to
the valid baseline or repeat sampler retries as the routine development loop.
The valid baseline and failures remain under ignored
`artifacts/performance/contact-cpu-profile-v1/`, `contact-cpu-profile-v2/` and
`contact-page-preparation-v1/`. CPU sample counts are not GPU timings, calibrated
CPU milliseconds, or proof of the cause of missed display intervals.

The post-change Mac Release run without the sampler completes all 45 measured
seconds and its postlude: 10,129 accepted samples, 3,769 presentations, no renderer
errors, recorder overflow or missing/zero-time measured callbacks. CPU owner
p99/max is 3.931/19.064 ms, with 26 frames above 11.11 ms. There are 53 long
continuous intervals out of 3,740, so the 90 Hz gate still fails. Measured footprint
grows 24.83 MiB; thermal state stays nominal. The full capture shows the completed
ink and live Navigator. This single run is not a calibrated before/after comparison
or sustained acceptance. All owned apps/profilers are terminal; the iPad build is
not installed or launched in this batch. The clean result is retained under
`artifacts/performance/contact-page-preparation-v1/clean/`.

After integrating the shared swept-contact renderer, both current Release apps
complete a 45-second `layered-4k` diagnostic. Each uses the unchanged 4096-square,
eight-paint-layer G-Pen fixture, prediction, pressure variation and 240 Hz input,
with ten-second warm-up/postlude and GPU timestamp recording disabled. Mac uses
2400 × 1740 drawable pixels at 90 Hz; physical iPad uses 2752 × 2064 at 120 Hz.
The runs are serial, with no compiler, UI automation or GPU profiler running.

| Short measurement | Mac | Physical iPad |
| --- | ---: | ---: |
| CPU owner p99 / max, ms | 4.035 / 15.695 | 7.939 / 17.628 |
| CPU frames over host budget | 27 | 48 |
| Long continuous intervals / total | 62 / 3,728 | 35 / 5,035 |
| Continuous interval p99 / max, ms | 22.222 / 33.334 | 8.334 / 25.000 |
| Peak measured footprint, MiB | 1,278.32 | 1,330.80 |
| Measured footprint growth, MiB | 21.52 | 12.55 |

Both intervals finish with no rejected input, renderer errors, recorder overflow,
missing callbacks or zero-time measured presentations. Full readiness precedes
measurement. Mac retains one zero-time presentation outside measurement. The
reviewed Mac capture shows pressure-varying ink over the underpaint, the complete
editor and live Navigator. These are valid runs with failing target cadence.

All 27 slow Mac CPU frames follow a pen-up receipt. However, 61 of its 62 long
continuous presentation intervals join frames whose latest receipts are both
movement; shortening pen-up work alone does not explain those gaps. On iPad,
22 slow CPU frames follow pen-up and 26 follow movement. The latter include
drawable-acquisition waits exceeding 1 ms. Receipt associations are diagnostic,
not causal proof or evidence of which input pixels appeared. No presentation,
admission, fidelity or snapshot change is adopted from this observation.

The same iPad binary subsequently completes 600.008 measured seconds with
135,003 nonpredicted samples and 67,295 actual presentations. CPU owner
p50/p95/p99/max is 1.743/2.664/9.245/22.298 ms, with 948 frames over 8.33 ms.
Continuous presentation p99/max is 12.498/29.167 ms; 727 of 66,919 intervals
exceed the existing cadence threshold. Drawable acquisition p99/max is
5.004/21.209 ms. No measured callbacks are missing or zero-time; no input is
rejected, and there are no renderer errors or recorder overflows. Full readiness
precedes measurement and the postlude completes. Sustained cadence still fails.

Measured iPad footprint grows 291.28 MiB to a 1,574.08 MiB peak. Thermal state
remains nominal. History, renderer and recorder contributions are not isolated,
so this does not establish a leak or bounded long-term memory use. The owned
process is verified closed, its disposable app removed and the existing test
runner restored. Both review and artist editor descriptors remain unchanged;
neither editor is updated or restarted. No XCTest startup retry is attempted.

The same Mac binary completes 600.001 measured seconds with 135,001
nonpredicted samples and 50,213 actual presentations. CPU owner
p50/p95/p99/max is 2.338/3.407/4.073/21.860 ms, with 322 frames over 11.11 ms.
Continuous presentation p99/max is 22.222/33.334 ms; 909 of 49,837 intervals
exceed the cadence threshold. Drawable acquisition p99/max is 0.155/0.654 ms.
Measured footprint grows 326.47 MiB to a 1,574.75 MiB peak, with nominal thermal
state. All input is accepted, readiness precedes measurement, the postlude
completes, and there are no renderer errors, recorder overflows or missing/
zero-time measured presentations. The final full-window capture is reviewed
and the owned process is verified closed. Sustained Mac cadence also fails.

Both ten-minute runs preserve the original workload on the same source; all
measurement jobs are terminal. A subsequent mask-only contact-brush correction
does not change their ordinary color-painting path. The following main integration
adds explicit grouping to the shader's existing integer hash expression.
Isolated GPU execution, calibrated recorder overhead,
physical Pencil latency, memory attribution and the complete workload matrix
remain open. Evidence and the completed-run checkpoint are ignored under
`artifacts/performance/contact-layered4k-fb81ebe/`.

## Metal presentation notification diagnostic — 2026-09-13

No renderer change was adopted from this diagnostic. Apple's
[drawable presentation contract](https://developer.apple.com/documentation/metal/mtldrawable/present())
tracks writes after command buffers are scheduled. Its
[command-buffer presentation convenience method](https://developer.apple.com/documentation/metal/mtlcommandbuffer/present(_:))
notifies the drawable from a scheduled handler. A direct call immediately after
`commit()` can run too early. The separate
[CAMetalDisplayLink deadline](https://developer.apple.com/documentation/quartzcore/cametaldisplaylink/update/targettimestamp)
allows GPU work to continue after the required presentation notification.

An isolated native fixture compared direct presentation after
`waitUntilScheduled()` with a separate presentation command buffer, as used by
wgpu. The corrected Mac runs used a 90 Hz Metal display link, a 1920 × 1440
drawable, two seconds of warm-up, twelve measured seconds and two seconds of
continued rendering before invalidating the link. A calibrated compute pass and
delayed CPU submission placed GPU completion after the CPU deadline but before
the presentation target. Compute calibration is per process; these are contract
checks, not matched application performance measurements.

| Corrected Mac mode | Admitted / actual presentations | Zero-time presentations | Long intervals |
| --- | ---: | ---: | ---: |
| Direct, clear only | 1079 / 1079 | 0 | 0 |
| Separate command buffer, clear only | 1080 / 1078 | 2 | 2 |
| Direct, compute | 1080 / 1080 | 0 | 0 |
| Separate command buffer, compute | 1080 / 1080 | 0 | 0 |

Both compute modes submitted their presentation requests before the deadline
on every admitted frame. The direct scheduling wait and queued presentation
buffer's scheduled callback also completed before that deadline on every frame.
GPU completion crossed the CPU deadline while remaining before the display
target on 1080 direct frames and 1076 queued frames. Neither mode presented
before its GPU work finished, and both completed without GPU errors or missing
callbacks. This does not support the hypothesis that a separate presentation
buffer necessarily waits for rendering to finish before notifying Core Animation.
The clear-only losses remain recorded; the fixture does not establish sustained
application cadence or drawing latency.

The earlier fixture revisions are retained as failed diagnostics. A shared-event
wait delayed command scheduling; direct presentation without a scheduling wait
could show content before rendering and produced GPU timeouts on iPad during
warm-up. That entire comparison is invalid for adoption. A second Mac revision
used real compute work and correct scheduling, but its CPU timers woke too late
to test the intended deadline condition. The corrected revision increases the
submission margin and retains a running postlude.

A separate audit of all six retained application display-link traces finds
that all 49, 54 and 57 skipped iPad presentations had completed CPU owner service
before their deadlines, with at least 3.012 ms remaining across the three runs.
Those traces do not record the actual Metal scheduling time. Late CPU owner
completion alone does not explain the skips, and the old supplied-drawable
adapter remains rejected.

The corrected iPad executable subsequently completed all four cases under the
separate disposable component-test identity. This preserved the installed review
editor and artist app. Each case used a 2752 × 2064 drawable, a requested 120 Hz
rate, two seconds of warm-up, twelve measured seconds and a running postlude.

| Corrected iPad mode | Admitted / actual presentations | Zero / missing presentations | GPU completed after deadline, before target |
| --- | ---: | ---: | ---: |
| Direct, clear only | 1440 / 1440 | 0 / 0 | 0 |
| Separate command buffer, clear only | 1440 / 1440 | 0 / 0 | 0 |
| Direct, compute | 1440 / 1440 | 0 / 0 | 1425 |
| Separate command buffer, compute | 1440 / 1440 | 0 / 0 | 1440 |

Each case retains one zero-time presentation during warm-up, outside its
designated measurement interval. The full trace audit records those four skips;
they are not missing callbacks or zero-latency frames.

Every measured presentation request and scheduling observation arrived before
the CPU deadline. All qualifying GPU-deadline crossings presented successfully;
there were no GPU errors, incomplete render buffers or presentations before GPU
completion, including warm-up/postlude error checks. Measured intervals had
p99 8.334 ms and a maximum of 8.342 ms across the four cases. The compute modes'
GPU duration p99 values were 3.610 ms direct and 3.511 ms queued. Calibration is
per process, so that difference is not an application performance comparison.
These results extend the Mac contract finding to the physical iPad; they do not
explain the actual application's retained cadence failures or establish drawing
latency, instrumentation overhead or ten-minute acceptance.

The first runner failed while listing the export directory. Its same live
process was observed, the original export recovered directly, and that process
closed without repeating the case. The remaining runs retrieved exports directly.
All four owned processes were verified closed, the disposable diagnostic removed,
and the current owned test runner restored. The review editor was not updated or
restarted. Private results and the retained runner failure are under
`artifacts/performance/metal-notification-ipad-isolated-v1/`; earlier fixtures,
binaries and connection logs remain under `metal-notification-2447108*`.
The production renderer is unchanged. Both-host application performance acceptance
remains open; actual viewport command scheduling/completion is the next observation
needed beyond CPU owner service and GPU queue-span timings.

A subsequent viewport-observer experiment was rejected and fully reverted.
It attempted to attach a completion handler to the existing viewport command
buffer through wgpu's `CommandEncoder::as_hal_mut`. Both Release builds and
standalone Metal callback checks passed, but the real Mac editor rejected mixing
normal wgpu encoding with raw encoder access before any measured drawing began.
The first runner's successful export exit did not represent a successful workload;
the retained trace contains 448 frame errors and no viewport submissions. A
second diagnostic captured the wgpu mode error and verified its owned process
closed. No iPad deployment occurred. All nine affected source/test files were
restored byte-for-byte to their pre-experiment state, preserving the compact
Color changes. The rejected builds and patch are retained under
`artifacts/performance/viewport-commands-v1/` and must not be used for measurement.
Existing Instruments captures remain the available source of actual GPU
execution observations; they do not supply unprofiled scheduling acceptance.

## Compact color fields and hue guide — 2026-09-13

The native compact wheel retains separate field and guide images. Color picking
updates the field only when its hue changes; marker, readout and unrelated
editor updates reuse it. Shape/size changes regenerate the guide from the same
adaptive sRGB gradient stops used by Web. The first implementation evaluated the
perceptual hue curve for every guide pixel; replacing that with the shared stops
reduces the largest measured circle-guide p99 from 16.519 to 4.803 ms.

The CPU-only Release benchmark uses 100 warm-up and 1,000 retained generations
per case, across all three fields/guides at 184, 260 and 396 physical pixels.
No compiler, UI automation or GPU profiler runs during measurement. The largest
case corresponds to the wheel in a 226-point panel at 2x scale:

| Shape | Field p99, ms | Guide p99, ms |
| --- | ---: | ---: |
| Okhsv circle | 1.308 | 4.803 |
| HSV square | 0.519 | 3.099 |
| HLS triangle | 0.213 | 3.155 |

The guide is static during color drags. These Mac measurements exclude host
allocation, image upload, drawing and presentation; they do not establish iPad
cost, complete interaction latency or sustained editor cadence. The source
benchmark is `crates/layer-ui/examples/color_field_benchmark.rs`; raw runs and
the earlier implementation remain in `artifacts/apple-compact-color-v1/`.

## Reserved drawable slot experiment — 2026-09-13

A lower admission limit was **not adopted**. In the retained ten-minute iPad
4K watercolor trace, all 898 drawable acquisitions exceeding 1 ms began with
two pending presentation callbacks. The experimental owner reserved one of the
three drawable slots for onscreen contents, using the existing callback retry.
Its capacity/lifecycle tests passed, but physical cadence regressed.

Eight fresh Release measurements compare `232cd2b` with only that admission
change. Each uses 45 measured seconds, the normal ten-second warm-up/postlude,
unchanged synthetic input and brush fidelity, and disabled GPU timestamps.
Both builds are archived per host. The measured viewports remain 2400 × 1740
on Mac and 2752 × 2064 on iPad, at 2x scale and 90/120 Hz respectively. Mac
startup size changes are retained separately; neither measured viewport changes.
Compilers, UI automation and GPU profilers were idle during measurement.

| Host / workload | CPU p99 before / candidate, ms | CPU frames over host budget before / candidate | Long continuous intervals before / candidate |
| --- | ---: | ---: | ---: |
| Mac ink | 3.512 / 3.315 | 0 / 0 | 84/3676 / 216/3559 |
| Mac 4K watercolor | 5.611 / 6.643 | 0 / 0 | 54/3702 / 868/2901 |
| iPad ink | 4.421 / 3.356 | 25 / 0 | 27/5048 / 35/5025 |
| iPad 4K watercolor | 9.136 / 4.930 | 95 / 7 | 89/4892 / 734/4315 |

All intervals complete with the expected input workload, no renderer errors,
missing or zero-time measured presentations, or recorder overflow. Each exported
PID matches its launch and each owned workload app closes afterward. The regular
artist apps/data are preserved, and the validated iPad review build is restored.
The candidate improves iPad CPU tails while making sustained presentation worse;
the production scheduler and its tests are restored. Shorter owner service is
insufficient evidence of smoother drawing. No ten-minute candidate pass is claimed.
The ignored `artifacts/performance/drawable-reserve-232cd2b` directory contains
the archived binaries, source patches, raw traces and reproducible comparison.

## Retry after presentation capacity returns — 2026-09-13

The shared frame driver can now retry a capacity-denied tick once when Metal
reports a presentation. Registration checks capacity under the same lock as
ticket retirement, so a callback just before registration cannot lose the wake.
The callback runs outside that lock and schedules the retry on the main queue.
A newer display tick, detach/replacement, pending frame or expired original
target rejects it. A retry that encounters a full pool does not register another
retry. Resize, resume and detach discard capacity waiters with their old tickets.
The ordinary display-link preference and idle pause remain unchanged.

This addresses a measured scheduling race, without treating presentation as a
guarantee that Core Animation has recycled the drawable. In the previous Mac
ten-minute watercolor trace, capacity-denied ticks preceded 1,878 of 3,651 long
continuous presentation intervals. The median delay from such a tick to the next
presentation callback was 0.055 ms; the driver previously waited for another
display tick. The corresponding iPad trace had no capacity-denied ticks, so this
diagnosis does not explain its acquisition stalls. These are temporal associations.

Two fresh 45-second `wet-watercolor-4k` Release runs on source `008b646`, before
and after the retry change, used the same 2400 × 1740 Mac viewport, 90 Hz target,
240 Hz synthetic input, prediction and disabled GPU queue timestamps. Both
complete with no rejected input, frame errors, missing/zero-time presentations
during measurement, or CPU service over 11.11 ms.

| Mac measured result | Before | Retry |
| --- | ---: | ---: |
| Actual presentations | 3,568 | 3,737 |
| CPU owner p99 / max, ms | 5.875 / 8.436 | 5.584 / 7.695 |
| Long continuous intervals / total | 225 / 3,539 (6.36%) | 46 / 3,708 (1.24%) |
| Admission-to-presentation p99, ms | 32.940 | 32.829 |

The same candidate then completed 600.001 measured seconds: 49,416 actual
presentations, CPU p50/p95/p99/max 3.444/4.912/5.722/9.139 ms, and zero CPU
service samples over the Mac budget. Continuous presentation p50/p95/p99/max
was 11.111/11.111/22.222/44.445 ms. Its 1,329 long intervals out of 49,040
(2.71%) still fail sustained cadence acceptance. Of 30,676 capacity-denied ticks,
29,946 received an admitted retry; these callbacks are recorded separately from
display ticks. Admission-to-presentation p99/max was 33.616/44.076 ms.

The measured interval has no rejected input, renderer errors, missing callbacks,
zero-time presentations or recorder overflow. The full trace retains three
zero-time presentations outside measurement. Measured peak footprint was
1,761.19 MiB, with 140.80 MiB first-to-last growth and nominal thermal samples.
The recorder and workload both contribute memory; this does not isolate a leak.
Builds, UI automation and GPU profiling were idle throughout measurement.
Each exported process identity matched its launch, and owned apps closed after
export. The short pair supports this scheduling improvement; the ten-minute
candidate is not an identical-source ten-minute before/after comparison.

Both physical Release targets compile. Direct shared-driver and real Metal
gate/owner checks cover atomic registration, concurrent duplicate callbacks,
deadline expiry, newer ticks, cancellation and surface lifecycle on both Apple
presets. All 29 trace-analysis tests pass. This is not new physical iPad timing
evidence. The complete workload matrix, iPad stalls, residual Mac cadence gaps,
isolated GPU work, calibrated instrumentation overhead and physical input latency
remain open. Raw traces and local signing/device information remain ignored.

## Owner lifetime and presentation admission — 2026-09-12

Historical implementation: the callback-dependent gate and its ticket-only tests
are retired by the startup-progress fix above. The owner autorelease policy and
frame-driver lifecycle checks remain.

Both native targets now drain autoreleased objects after every asynchronous
render-owner task, including the last task before idle. The prior queue inherited
its worker's pool policy. A direct test using 64 real owner requests fails against
that prior implementation because temporary native objects survive their task;
it passes on both platform presets with `autoreleaseFrequency: .workItem`.
Apple recommends an autorelease pool around drawable rendering in its
[CAMetalLayer guidance](https://developer.apple.com/documentation/quartzcore/cametallayer).
The pool change fixes resource lifetime; the short hardware pairs below do not
show that it alone resolves presentation stalls.

The shared frame driver also defers rendering when every drawable in the layer's
configured pool is still awaiting a presentation callback. Input and editor
requests continue through the serial owner, and the display link remains awake
to retry. A completed CPU submission does not retire a drawable ticket. Callback
retirement is idempotent; unsuccessful submissions release their ticket. Attach,
resize, detach and platform resume invalidate old tickets, whose late callbacks
cannot release replacement tickets. Core Animation can still delay drawable
recycling after presentation, so this is a capacity check, not a guarantee that
the next acquisition will be immediate. Display-link preferences and idle policy
are unchanged. Simulator builds omit physical presentation callbacks and gating.

The direct gate/driver checks cover capacity, retry, cancellation, duplicate and
concurrent callbacks, wake, detach and surface replacement. A real owner/Metal
fixture injects pending tickets and exercises resize, resume invalidation, detach
and reattachment on both presets. It verifies those code paths, not the complete
physical OS interruption matrix or real discarded callbacks. Signed Release
builds for both physical targets pass, as do the 28 trace-analysis tests.

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/owner-autorelease.swift
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/CanvasFrameDriver.swift \
  apps/layer-apple/tests/frame-driver.swift -o /tmp/capy-frame-driver
/tmp/capy-frame-driver
python3 -m unittest discover -s tools/performance -p 'test_*.py'
```

### Short ink comparisons

Separate 30-second measured `ink` runs compare published `dab000d`, that source
with only the owner pool change, and then the pool plus presentation gate. The
gate runs precede the final resume hooks and denial-reason telemetry. Optional
GPU timestamps are off in all six runs. No compiler, GPU profiler or UI automation
ran during measurement. Each process identity was verified against its exported
trace and each owned benchmark app was closed afterward.

| Host / change | CPU p99 / max, ms | CPU frames over host budget | Long continuous presentation intervals |
| --- | ---: | ---: | ---: |
| Mac baseline | 12.275 / 15.770 | 54 | 91 |
| Mac pool only | 12.301 / 12.527 | 71 | 110 |
| Mac pool + gate | 3.355 / 6.377 | 0 | 81 |
| iPad baseline | 4.506 / 13.723 | 20 | 20 |
| iPad pool only | 4.576 / 13.661 | 20 | 22 |
| iPad pool + gate | 3.285 / 9.336 | 1 | 28 |

Long intervals exceed the host period plus 5% tolerance, excluding intentional
idle boundaries. Presentation records show that all 54 baseline Mac CPU stalls
start with three acquired drawables still awaiting presentation; all 20 baseline
iPad stalls start with two. The pool-only runs retain the same association.
The gate reduces the short Mac CPU tail, but these samples do not establish a
sustained cadence benefit, and the iPad interval count does not improve.
Frame-admission latency excludes time spent waiting for admission; neither that
metric nor the recorder's latest-input receipt association proves physical
input-to-pixel latency.

### Ten-minute 4K watercolor on the final runtime

Both physical Release targets completed `wet-watercolor-4k` on shared source
through `990b515` plus this milestone's runtime changes. The profile uses a
4096 × 4096 document, eight paint layers, diameter-320 watercolor, 240 Hz
synthetic samples and visual prediction. Ten seconds of warm-up precede the
600-second measured interval and a ten-second postlude. GPU queue timestamps
are disabled. Builds, Chrome, GPU profiling and UI automation remained idle
during measurement; lightweight artifact analysis and process monitoring ran.

| Measured result | Mac, 90 Hz | Physical iPad, 120 Hz |
| --- | ---: | ---: |
| Duration, seconds | 600.000 | 600.008 |
| Delivered nonpredicted samples | 135,001 | 135,003 |
| Actual presentations | 46,920 | 65,953 |
| CPU owner p50 / p95 / p99 / max, ms | 3.395 / 5.138 / 6.025 / 9.402 | 2.067 / 3.492 / 9.122 / 17.711 |
| CPU frames over host budget | 0 | 1,176 |
| Drawable acquisition p99 / max, ms | 0.046 / 1.047 | 5.850 / 16.295 |
| Continuous presentation p50 / p95 / p99 / max, ms | 11.111 / 22.222 / 22.222 / 55.556 | 8.333 / 8.334 / 16.667 / 37.499 |
| Long continuous intervals / all continuous intervals | 3,651 / 46,544 | 1,126 / 65,577 |
| Ticks deferred for drawable capacity / pending owner | 3,107 / 1 | 0 / 1,283 |
| Admission-to-presentation p50 / p95 / p99 / max, ms | 32.829 / 33.032 / 33.207 / 44.166 | 17.620 / 17.654 / 25.972 / 34.306 |
| Measured peak footprint, MiB | 1,791.11 | 1,627.17 |
| Measured first-to-last footprint growth, MiB | 172.81 | 19.78 |
| Thermal states | nominal | nominal |

Both runs complete without rejected input, renderer errors, recorder overflow,
missing callbacks or zero-time presentations **during measurement**. Each full
trace retains two zero-time presentations outside that interval. Full-run peak
footprints, including setup and postlude, are 2,993.38 MiB on Mac and 3,312.47 MiB
on iPad. The recorder reserves 74.52 MiB; footprint growth includes workload and
recording effects and is not an isolated leak measurement. Exported PIDs match
the launched apps, and both owned benchmark processes close after export.

The Mac CPU budget passes in this run, but 7.84% of continuous presentation
intervals exceed its tolerance. The iPad exceeds both its CPU budget and its
presentation target, with 1.72% long continuous intervals. Acquisition accounts
for at least half of owner time in 808 of its 1,176 over-budget CPU frames.
The capacity check alone cannot resolve those iPad stalls: none of its measured
ticks were deferred for a full pool. These are failing sustained performance
results, not final acceptance. Remaining work includes presentation scheduling,
the full workload matrix, complete GPU observations, calibrated recorder overhead,
physical input latency and lifecycle/visual/feature acceptance on both platforms.
Raw traces, installation data and signing information stay in ignored artifacts.

After measurement, Android pickup, default workspace pinning and shared collapsed
divider changes through `dc2e651` were integrated. The 377 Apple bridge, shared UI
and native workspace checks pass. Both integrated signed Release builds verify
and complete separate five-second ink smoke intervals: 406 actual Mac
presentations and 563 iPad presentations, with no rejected input, renderer errors,
overflow or missing/zero-time presentations during measurement. A Mac window-only
capture after export shows the painted canvas. Both owned apps close afterward.
These launch/drawing checks do not extend the ten-minute evidence to the newly
integrated shared changes or constitute a new full visual comparison.

## Incremental workspace publication

Both Apple editors now consume the shared `workspace_update` contract. A full
model publication establishes the revision used by retained controls. Ordinary
tab/floating motion publishes absolute geometry, tab previews and drop hints;
native view placement moves hit areas, clipping and live Navigator allocations
together. Down, tear-off, release, cancellation and other model changes retain
their normal full publication. Every input phase still reaches Rust; history
and durable persistence remain shared behavior.

The paired C-ABI fixture gives separate compatibility/incremental owners the
same actions on both Apple presets. Complete snapshots match exactly after
removing the new `workspace_update` field. Across 32 floating moves, actual
serialized payload totals are:

| Preset | Compatibility bytes | Incremental bytes |
| --- | ---: | ---: |
| iPad | 2,675,515–2,675,520 | 5,403–5,440 |
| Mac | 2,681,947–2,681,952 | 5,403–5,440 |

The ranges cover cancellation and commit cases. This is about a 99.8% wire-size
reduction for this fixture; it is not a CPU/GPU timing or frame-rate result.
Geometry matches the compatibility layout on each move, intermediate updates
carry no durable persistence, and completion plus workspace Undo/Redo match.

The invisible AppKit workflow performs 24 floating moves per Apple preset,
checking real native drag/resize hit rectangles, tab bounds/clips and the live
Navigator allocation/image/clip while retaining its SwiftUI identity and the
panel models. A separate SwiftUI observation probe renders ten movements
without rebuilding unrelated command, panel, menu, layout, camera, other-group
or tab-visibility readers. Rejected revisions and camera-bearing or camera-less
updates have direct coverage. These checks exercise shared Apple code; UIKit
touch workflows and physical presentation remain separate acceptance evidence.

Reproduction commands are in the [Apple README](README.md). Raw measurements and
captures remain in ignored artifacts. Sustained 90 Hz Mac / 120 Hz iPad cadence,
isolated GPU timing, physical input latency and the remaining workload matrix
are still open. No hardware performance improvement is claimed from this
transport fixture alone.

## Repeatable native drawing workloads

Set `CAPY_WORKLOAD` to run a synthetic fixture through the same serial input
owner, display link, Rust renderer, editor panels, history and recovery writer
as ordinary drawing. Use a separate benchmark bundle identifier for device
installs, and a separate DerivedData directory. The opt-in workload additionally
uses a new private persistence root under `Caches/CapyPerformanceSessions` for
every editor instance. It never reads or replaces the artist's normal settings,
workspace or recovery copies. Ordinary launches have no workload timer.

| Profile | Document | Paint layers, excluding paper | Brush / diameter | Synthetic prediction |
| --- | --- | --- | --- | --- |
| `ink` | 2048×2048 | 1 | G-Pen / 24 px | Off |
| `ink-predicted` | 2048×2048 | 1 | G-Pen / 24 px | On |
| `wet-watercolor` | 2048×2048 | 1 | Wet Watercolor / 320 px | On |
| `layered-4k` | 4096×4096 | 8 | G-Pen / 24 px | On |
| `wet-watercolor-4k` | 4096×4096 | 8 | Wet Watercolor / 320 px | On |

The prediction column controls supplied native prediction batches, not the
shared Stroke Prediction preference. Workload storage starts with default
preferences: engine feedback is enabled with a 16 ms manual amount. Mac has no
native prediction provider, so both ink profiles retain manual engine prediction;
`ink` is not a master-prediction-off control. Supplied samples contain no pending
sensor-correction tokens and do not reproduce the physical Pencil stall above.

The 4K cases retain seven translucent full-document underpaint layers and the
active paint layer. Setup creates the document through the shared project job
and uses ordinary UI actions for layers, fills, brush selection and size. The
editor stays in its full default workspace. No brush quality settings are
reduced. Ten seconds of drawing warm up the fixture before measurement.

The versioned trajectory is in `DrawingWorkloadPlan.swift`: deterministic curves
inside the document, pressure varying from 0.25 to 1, 240 samples/second, 1.5-second
strokes and 0.1-second lift gaps. A main-run-loop producer delivers coalesced
batches at 120 callbacks/second independently of render admission. Predicted
points use the same native prediction path and remain visual-only. A delayed
producer catches up; a backlog of one second aborts the run instead of silently
dropping samples or lowering the input rate. Interval maxima expose producer
lateness, which is synthetic scheduling delay, not a physical Pencil metric.

`CAPY_WORKLOAD_SECONDS` is the measured duration after warm-up (default 600;
allowed 1–1800). The run records separate setup, warm-up, measured, end and
postlude markers. A ten-second postlude observes pen-up, deferred GPU work,
recovery and idle transitions; its end does not prove that rendering drained.
Trace recording defaults to the requested measurement plus a 140-second allowance
for setup/warm-up/postlude, but normally finishes at the postlude. An explicit
`CAPY_TRACE_SECONDS` overrides that limit, and can therefore truncate a run.

`CAPY_WORKLOAD_SPACE` and `CAPY_WORKLOAD_DEPTH` select the ordinary shared
document-creation color options, defaulting to `Srgb` and `U8`. For example,
`ProPhoto` and `U16` exercise wide-gamut 16-bit storage with the same input and
renderer. Shared document validation rejects unsupported values. The trace's
`workload.document_color` records the requested format; verify the resulting
recovery archive before counting a run as qualification of that format.

Launch the built, isolated Mac app through Launch Services to foreground it;
an occluded window cannot supply presentation evidence:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
python3 apps/layer-apple/scripts/prepare.py
python3 apps/layer-apple/scripts/project.py
xcodebuild -quiet -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-Mac -configuration Release -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath apps/layer-apple/DerivedData/PerformanceMac \
  PRODUCT_BUNDLE_IDENTIFIER=art.capycanvas.apple.mac.performance \
  CODE_SIGN_IDENTITY=- CODE_SIGNING_ALLOWED=YES build
open -n --env CAPY_WORKLOAD=ink --env CAPY_WORKLOAD_SECONDS=600 \
  --env CAPY_TRACE_DIRECTORY="$PWD/artifacts/performance/mac-ink" \
  apps/layer-apple/DerivedData/PerformanceMac/Build/Products/Release/CapyCanvas-Mac.app \
  --args -ApplePersistenceIgnoreState YES
```

For an installed iPad benchmark bundle:

```sh
xcodebuild -quiet -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-iPad -configuration Release -destination 'generic/platform=iOS' \
  -derivedDataPath apps/layer-apple/DerivedData/PerformanceDevice \
  PRODUCT_BUNDLE_IDENTIFIER=art.capycanvas.apple.ipad.performance \
  DEVELOPMENT_TEAM="$CAPY_APPLE_TEAM" 'CODE_SIGN_IDENTITY=Apple Development' \
  -allowProvisioningUpdates build
xcrun devicectl device install app --device DEVICE_ID \
  apps/layer-apple/DerivedData/PerformanceDevice/Build/Products/Release-iphoneos/CapyCanvas-iPad.app
xcrun devicectl device process launch --device DEVICE_ID --terminate-existing \
  --environment-variables '{"CAPY_WORKLOAD":"ink","CAPY_WORKLOAD_SECONDS":"600"}' \
  art.capycanvas.apple.ipad.performance
```

Keep the benchmark window visible and leave its editor untouched. Only one
benchmark window should run on each device. Copy traces from the benchmark's
container using its bundle identifier. Trace metadata labels input as synthetic
and records the fixture specification; reports separate the measured interval
from startup and postlude. `measurement_completed` means a complete, non-aborted
interval was recorded. It does not imply performance acceptance, pixel inclusion,
physical input latency, or successful coverage of the other profiles. Review
rejected input, frame errors, missing presentations, readiness and all warnings.
The report retains the first and last presentation's distance from the measured
interval boundaries, denied display-link admissions, frames without viewport
submission and missing/zero-time completions. A completed input producer cannot
establish continuous rendering if its window becomes occluded.

## Full-cadence experiment and ten-minute ink: 2026-09-12

An unpublished candidate requested `minimum = maximum = preferred` at the
display's maximum rate on both hosts, updating the Mac preference after display
changes. Apple describes [`preferredFrameRateRange`](https://developer.apple.com/documentation/quartzcore/cadisplaylink/preferredframeraterange)
as a callback preference subject to system policy, not a presentation guarantee.
The candidate retained the ordinary idle pause and three Metal drawables.
It was **not adopted**: the completed measurements below still miss cadence
targets and do not establish a sustained improvement. The published scheduler
is unchanged; the new runtime change only labels local trace exports with PID.

Twenty-second `ink` pairs with GPU timestamps enabled preceded the sustained
tests. Mac CPU p99 was 12.361 ms before / 12.324 ms with the candidate; continuous
long intervals were 57 / 48. iPad CPU p99 was 3.953 / 3.316 ms, with 21 / 4 long
intervals. A subsequent 45-second candidate run with GPU timestamps disabled
still recorded Mac CPU p99 12.261 ms and 173 long intervals. These short samples
do not isolate an instrumentation correction or establish a cadence benefit.

Both physical Release apps then completed ten measured minutes of `ink` on
shared changes through `4a0a808` plus that candidate, followed by the normal
postlude. GPU queue-span recording was disabled; the CPU/input/presentation
recorder remained enabled. No compiler, UI automation or GPU profiler ran during
measurement. Lightweight local analysis and process monitoring continued.
The viewport was 2752×2064 on iPad and settled at 2400×1740 on Mac. Each app used
its isolated benchmark bundle and fresh private workload persistence root.

| Measured interval | Physical iPad | Mac |
| --- | ---: | ---: |
| Duration, seconds | 600.008 | 600.010 |
| Display maximum / target, Hz | 120 | 90 |
| Nonpredicted input samples | 135,003 | 135,003 |
| Actual presentations | 66,975 | 49,811 |
| CPU owner service p50 / p95 / p99 / max, ms | 0.935 / 1.158 / 4.794 / 13.671 | 1.328 / 1.823 / 7.602 / 17.511 |
| CPU service above host budget | 347 | 497 |
| Continuous presentation p50 / p95 / p99 / max, ms | 8.333 / 8.334 / 12.499 / 20.832 | 11.111 / 11.111 / 22.222 / 44.445 |
| Continuous intervals above target plus 5% tolerance / total | 750 / 66,599 | 1,084 / 49,435 |
| Frame admission to actual presentation p50 / p95 / p99 / max, ms | 17.622 / 17.640 / 24.390 / 34.270 | 21.990 / 33.181 / 43.999 / 55.344 |
| Peak physical footprint, MiB | 338.16 | 251.84 |
| First-to-last measured footprint growth, MiB | +31.91 | +13.34 |
| Observed thermal states | Nominal | Nominal |

Both measured intervals have zero rejected input, renderer errors, missing
presentation callbacks and zero-time presentations. The full traces have no
recorder overflow; they retain four iPad and one Mac zero-time presentations
outside measurement. First and last presentations are within 10 ms of each
measured boundary. Memory includes the recorder's reserved 74.52 MiB capacity;
growth cannot be attributed solely to document/history or called a leak from
these observations. Nominal thermal states do not prove constant clock rates.

Both CPU p99 values fall below their host budgets, but maxima and presentation
tails still fail the sustained targets. Admission-to-presentation association
is not physical input latency. GPU execution for the entire interval is not
measured by this recorder-off pair; the separate short correlated captures above
do not fill that coverage gap. The full five-profile matrix, calibrated recorder
overhead, physical input latency, memory/lifecycle checks and complete performance
acceptance remain open. Mac 120 Hz stays deferred by the user.

Drawable acquisition accounts for at least 75% of owner service in all 347
over-budget iPad frames and 489 of the 497 Mac frames. On iPad, 346 of those
347 frames occur within 200 ms of a stroke restart; only 20 Mac over-budget
frames do. This identifies different scheduling patterns to investigate without
claiming that the native clock preference or GPU execution alone caused them.

Both completed benchmark apps were closed. The ordinary drawing apps were
preserved. Subsequent integration and final-build checks are separate from these
candidate measurements; raw artifacts remain local under
`artifacts/performance/resumed-ebc507a`.

## Physical ten-minute baseline: 2026-09-11

The existing Mac ten-minute trace was re-analyzed for the current 90 Hz target;
this is a new report over the original capture, not a new device run. CPU p99 is
6.083 ms, with eight owner-service samples above 11.11 ms. Continuous presentation
intervals have p50/p95 11.111 ms, p99 22.222 ms and maximum 77.778 ms; 746 of
48,890 continuous intervals exceed the 90 Hz period plus the existing 5%
cadence tolerance. These remaining gaps are visible at the current target and
are not waived by deferring 120 Hz. The report is stored locally under
`artifacts/performance/refresh-target-review/mac90-sustained.json`.

Both Release apps completed version 1 of `wet-watercolor-4k`: 4096×4096, eight
paint layers plus paper, Wet Watercolor at 320 px, synthetic pressure and
prediction. Each ran ten measured minutes after ten seconds of drawing warm-up,
followed by the ten-second postlude. The shared renderer and UI include incoming
changes through `d8a130b`. No other build or GPU test ran during measurement.
The iPad viewport was 2752×2064 physical pixels; Mac was 2400×1740 after window
layout settled. Separate benchmark bundles and private persistence roots were used.

| Measured interval | Physical iPad | Native Mac |
| --- | ---: | ---: |
| Duration, seconds | 600.000 | 600.009 |
| Display maximum, Hz | 120 | 90 |
| Nonpredicted samples delivered | 135,001 | 135,003 |
| Actual presentations | 65,407 | 49,266 |
| CPU owner service p50 / p95 / p99 / max, ms | 2.180 / 3.643 / 9.120 / 17.779 | 3.312 / 4.927 / 6.083 / 13.038 |
| CPU service over 8.33 ms | 1,092 | 28 |
| GPU queue span p50 / p95 / p99 / max, ms | 5.500 / 8.279 / 9.423 / 19.037 | 8.228 / 10.367 / 11.632 / 23.255 |
| Missing GPU samples | 204 | 221 |
| Presentation interval p50 / p95 / p99 / max, ms | 8.333 / 8.334 / 33.332 / 125.003 | 11.111 / 11.111 / 66.667 / 111.112 |
| Positive display-link target lateness p50 / p95 / p99 / max, ms | 8.338 / 8.352 / 16.670 / 28.032 | 22.297 / 33.411 / 33.419 / 66.741 |
| Display-link ticks denied admission / total | 1,132 / 66,914 | 241 / 49,882 |
| Producer interval-maximum lateness p50 / p95 / p99 / max, ms | 26.783 / 36.283 / 39.233 / 44.115 | 47.481 / 55.118 / 56.533 / 57.713 |
| Peak physical footprint, MiB | 1,798.05 | 1,910.91 |
| First-to-last measured footprint growth, MiB | +0.17 | +148.83 |
| Observed thermal states | Nominal | Nominal |

Presentation distributions retain the deliberate pen-up gaps; their high
percentiles must not all be called missed drawing frames. Every measured
presentation exceeded its display-link target by more than 1 ms. Neither target
lateness nor producer scheduling delay is physical input-to-pixel latency. GPU
queue spans include CPU submission gaps and the uncalibrated profiler; skipped
readbacks may bias their tails. They do not isolate GPU execution time.

Both measured intervals have zero rejected input batches, missing presentation
callbacks and zero-time presentations. The full traces have zero renderer errors
and recorder overflow. Each interval includes 375 admitted frames without a
viewport submission. The first/last actual presentations lie within 8 ms of
both interval boundaries on both hosts. The Mac's completed canvas was captured
after export and visibly contains the synthetic paint; no per-frame pixel oracle
or physical input latency assertion is inferred from that capture. Memory
includes document/history, ordinary recovery work and recorder storage; the Mac
growth remains to be characterized. Nominal thermal samples do not establish
the absence of clock-frequency changes.

These results leave the 8.33 ms tail budget and complete performance acceptance
open. The other four profiles still need ten-minute runs on both platforms,
along with physical input, overhead calibration and Mac 120 Hz presentation
evidence on a suitable display configuration. Artifacts stay local under
`artifacts/performance/{ipad,mac}-workload-sustained`.

A preceding twenty-second ink experiment reduced the Metal drawable count from
three to two. On iPad it increased median owner service from 1.113 to 9.482 ms,
with median drawable acquisition at 8.305 ms and median presentation intervals
at 16.667 ms. The Mac's median target lateness improved, but CPU budget
exceedances increased. The experiment was reverted on both targets; these final
runs retain three drawables. The retained scheduling change publishes native
canvas-readiness accessibility updates once per attached surface instead of
every submitted frame. The synthetic producer also leaves lift gaps asleep.
The pilot changed multiple factors, so it does not isolate this change's benefit.

## Large document replay safety

A direct Metal regression reproduced the startup device loss seen while
investigating frame scheduling: replaying seven filled 4096×4096 paint layers
in one frame attempted to create 4097 outstanding native command buffers.
The reproduction uses no native window or frame recorder. Incremental layer
fills succeeded, which explains why setup timing could hide the problem.

The shared renderer now records at most 512 render/compute passes per chunk,
then finishes and submits chunks in order after closing staging uploads.
Upload completion stays attached to the last chunk. The complete replay matches
every pixel of the incremental 4K image, with renderer telemetry off and on.
See the [renderer regression command](../../crates/layer-render-wgpu/README.md).
This fixes submission capacity; it does not close the sustained frame-time or
presentation requirements above. The published Apple scheduler is unchanged.

## Display scheduling comparison — 2026-09-11

The shared CAMetalDisplayLink experiment was **not adopted**. It supplied each
callback's drawable to the serial render owner and separated the CPU commit
deadline from the presentation target. The shorter CPU frame times did not
establish better presentation: the physical iPad repeatedly reported skipped
drawables with both requested rendering windows, including with GPU timing
disabled. The published hosts retain CADisplayLink and ordinary wgpu acquisition.

Each row below is a separate twenty-second measured `wet-watercolor-4k` run,
with ten-second warm-up and postlude. CPU times include the whole owner service;
admission-to-display times use actual nonzero Metal presentation callbacks.

| Host / requested Metal latency | GPU timer | Actual presentations | Zero-time presentations | CPU p99, ms | Admission-to-display p99, ms |
| --- | --- | ---: | ---: | ---: | ---: |
| iPad / 1 | On | 2,156 | 49 | 4.832 | 24.957 |
| iPad / 2 | On | 2,137 | 54 | 4.640 | 24.970 |
| iPad / 1 | Off | 2,154 | 57 | 4.387 | 24.916 |
| Mac / 1 | On | 1,642 | 0 | 6.371 | 33.380 |
| Mac / 2 | On | 1,655 | 1 | 5.875 | 33.376 |
| Mac / 1 | Off | 1,655 | 1 | 6.046 | 33.377 |

All six intervals completed with no renderer errors or missing presentation
callbacks. The Mac reports 90 Hz and the iPad 120 Hz. Changing the requested
latency did not change the observed target-to-deadline separation: approximately
22.222 ms on Mac and 8.333 ms on iPad. These observations do not demonstrate that
the requested latency is the actual end-to-end latency. Timer-disabled rows have
no GPU-duration samples; these short pairs do not calibrate all recorder overhead.

The experiment also exposed an unsafe explicit CATransaction commit on the
render queue: on iPad it invoked UIKit layout off the main thread and crashed
during attachment. That trial was removed before the six completed runs above.
A failed run produced no new trace; an older container file was rejected as
evidence. Collection must check file freshness and configuration against the
specific launch, not assume that the latest existing file belongs to it.

Local raw evidence remains under `artifacts/performance/{mac,ipad}-metal-link-`
`{safe1,safe2,no-gpu}`. The experiment is saved locally, not shipped in either
Apple target. Further scheduling work needs a new explanation and presentation
evidence; lower CPU measurements alone are insufficient.

After restoring CADisplayLink and integrating shared changes through `a7c048c`,
the same Release binaries were run once with GPU timing enabled and once with
it disabled. All four twenty-second intervals completed with zero rejected
input batches, missing callbacks or zero-time presentations:

| Stable host | GPU timer | Actual presentations | CPU p99, ms | Admission-to-display p99, ms |
| --- | --- | ---: | ---: | ---: |
| iPad | On | 2,190 | 9.077 | 25.965 |
| iPad | Off | 2,188 | 9.120 | 25.966 |
| Mac | On | 1,639 | 6.232 | 32.897 |
| Mac | Off | 1,655 | 5.873 | 32.928 |

The whole traces have no renderer errors or recorder overflow; startup/postlude
zero-time callbacks and omitted GPU samples remain in the reports. The iPad CPU
tail exceeds 8.33 ms with either instrumentation setting. Enabled GPU queue spans
have p99 9.389 ms on iPad and 10.363 ms on Mac and retain the overhead caveat.
This short pair does not establish a precise overhead correction or sustained
performance acceptance. The stable controls include subsequent shared layout
changes, so their comparison against the earlier Metal-link runs is not a
strictly identical-source scheduler-only experiment.
Reports are in ignored `artifacts/performance/{mac,ipad}-scheduler-control`
and corresponding `-no-gpu` directories. The Mac's completed synthetic painting
was captured and inspected; test apps were closed after collection.

## Snapshot transport

CPU sampling of the isolated 4K watercolor workload identified snapshot
construction, serialization and Foundation decoding in the owner publication
path. These profiling runs are diagnostic: sampling can interrupt the process,
so their frame timings are not used as an unsampled performance baseline.

Apple's snapshot request now uses `NativeHost::take_snapshot_bytes`, writing
UTF-8 directly from the shared models. Other native callers retain the value
API. Both paths use one schema and the same change detection, camera patches
and workspace-persistence policy. The writer preserves the value transport's
exact widening of `f32` numbers to `f64`; it does not shorten color or geometry
values. Failed serialization leaves the pending update unacknowledged.

Forty independent snapshots captured before the refactor match the new decoded
wire payloads exactly. They cover both Apple presets, fractional brush/color
values, camera changes, all five settings pages, collapsed drawers, workspace
history, Zen, surface errors and resize. The fixture emits the actual bytes
without an extra parse/re-encode step. Direct regressions also cover native
Android and Windows projections, unchanged-state suppression, mixed value/byte
consumers and serialization failure for full/camera/workspace updates.

After the final integration through `6eb0418`, all 40 current value/byte payload
pairs still match exactly. The original reference differs only by that incoming
change's removal of the theme-toggle item from the shared View menu. Every other
field and numeric value is preserved; the theme-toggle command remains in the
catalog. The original reference and the explicit difference report stay local.

```sh
mkdir -p artifacts
cargo run --release -p layer-host --example snapshot-transport \
  > artifacts/snapshot-value.json
cargo run --release -p layer-host --example snapshot-transport -- --stream \
  > artifacts/snapshot-stream.json
cargo run --release -p layer-host --example snapshot-transport -- --benchmark \
  > artifacts/snapshot-benchmark.json
```

On the development Mac, four alternating rounds of 200 full snapshots per mode
give these CPU transport measurements. Both rows run on Mac hardware with the
indicated platform's models; the iPad row is not physical iPad timing. Each
sample includes construction, encoding and destruction. The test excludes
Swift decoding, rendering and presentation and cannot establish a frame budget.

| Snapshot preset | Value path median / p99, ms | Direct path median / p99, ms |
| --- | --- | --- |
| iPad | 0.509 / 0.599 | 0.168 / 0.199 |
| Mac | 0.512 / 0.568 | 0.176 / 0.197 |

Matching twenty-second physical `wet-watercolor-4k` runs, with GPU timing
disabled and no CPU sampler, completed before and after the change. Each
retains warm-up, prediction, pen-up gaps and the full editor. These short pairs
are exploratory and do not establish sustained acceptance or precise overhead
calibration. Both builds use shared changes through `16c5886`; later integration
through `6eb0418` is outside these recorded hardware intervals.

| Host / transport | CPU owner p99 / max, ms | CPU frames over target | Time outside recorded Rust stages p99, ms | Actual presentations |
| --- | --- | --- | --- | --- |
| iPad / value | 7.713 / 10.925 | 22 | 2.415 | 2,220 |
| iPad / direct | 9.057 / 10.643 | 32 | 1.534 | 2,189 |
| Mac / value | 6.399 / 8.512 | 0 | 2.936 | 1,638 |
| Mac / direct | 5.508 / 9.057 | 0 | 1.844 | 1,641 |

The residual column subtracts all five measured Rust stages from owner service;
it is not an isolated publication timer. On iPad, drawable acquisition p99 rises
from 0.025 to 4.322 ms, and the overall CPU tail worsens despite the transport
improvement. Its continuous presentation interval p99 remains 16.667 ms and its
maximum rises from 33.332 to 50.001 ms. Mac retains continuous intervals up to
66.667 ms. None of these late frames are waived. All four measured intervals
have zero rejected input batches, renderer errors, recorder overflow, missing
presentation callbacks or zero-time presentations. Disabled GPU spans remain
unmeasured, and no physical input-to-pixel latency is inferred.

The detailed traces, stage distributions, memory/thermal observations and
comparison reports remain in ignored `artifacts/performance/*snapshot*` and
`artifacts/apple-snapshot-*` paths. This change reduces transport work; it does
not close the 90 Hz Mac / 120 Hz iPad performance gates. Further investigation
must include drawable waiting and presentation scheduling as well as the
remaining ten-minute workload matrix.

## Retained live panel resizing

Apple request 7 opts into the shared host's layout-aware incremental transport.
Divider motion, floating-panel resizing and transient native measurements send
resolved geometry, workspace dimensions, camera and measurements together. Swift
stages them before notifying observers and preserves the current control indexes
and content revision. Ordinary floating translation retains its smaller position
packet. Release, cancellation, changed controls, collapsed columns and viewport
changes still use full snapshots. Existing requests 3 and 5 retain their schemas.

The native ABI check compares two independent sessions after identical actions
on both Apple presets. Sixteen divider moves transmit 135,108 bytes instead of
1,330,874–1,334,090 bytes; sixteen floating resize moves transmit 151,110 bytes
instead of 1,346,875–1,350,091 bytes. Geometry, camera and measurements match the
compatibility path exactly. Intermediate updates carry no persistence request;
release, cancellation, Undo and Redo preserve the full workspace state.

```sh
cargo test -p layer-apple layout_apple_abi -- --nocapture
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-motion.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/panel-measurements.swift
```

The invisible AppKit view check covers each Apple preset with 64 live resize
moves and 24 floating translations. Navigator identity, resize hit areas and tab
clips remain coherent through motion, cancellation, completion and history.
Projection checks reject partial, stale and mismatched-content geometry packets
before mutation. These establish payload and native view behavior, not UIKit
pixel parity or a sustained hardware resize frame rate. The complete drawing
performance and presentation gates remain open.

## Native UI lookup investigation

The shared Swift transport now reads Foundation-decoded dictionaries and arrays
in their original representation. Native Swift containers retain their Swift
lookup path. Previously, each decoded dictionary field access could bridge the
whole dictionary into Swift; an earlier iPad CPU profile attributes substantial
main-thread work to these lookups. This supporting change has not established
an overall drawing-performance improvement.

The recursive transport check covers 219,911 values, including all 40 captured
wire snapshots, plus native/Foundation containers, missing fields, array bounds,
Unicode, full-width unsigned integers, immutable replacements and round trips.
Both Release targets build, and the direct SwiftUI drawer/action check passes.
These checks establish transport compatibility, not complete UI workflow or
pixel parity.

A CPU-only command-field traversal of those 40 snapshots, repeated in four
alternating rounds of 100 traversals on the development Mac, averages 11.642 ms
with the old decoded-container lookup and 2.990 ms with the candidate. Fully
native Swift trees average 1.173 and 1.287 ms respectively; the representation
check has a small cost there. A discarded Foundation-only variant took 4.804 ms
for native trees, which is why the candidate preserves both lookup paths. These
are whole-traversal component timings, not single-frame or physical iPad times.

Forty-five-second physical `wet-watercolor-4k` runs before and after the lookup
change retain the full workspace, prediction, recovery, ten-second warm-up and
postlude. GPU timing is disabled. Source is `afa058a` plus the candidate for the
after runs. No CPU sampler or build runs during these measured intervals. The
pairs are exploratory; ordering, cache and thermal history are not calibrated.

| Host / lookup | CPU owner p99 / max, ms | CPU frames over target | Continuous interval p99 / max, ms | Long continuous intervals / total |
| --- | --- | --- | --- | --- |
| iPad / previous | 8.757 / 10.196 | 54 | 16.667 / 41.667 | 67 / 4,922 |
| iPad / candidate | 9.133 / 17.573 | 75 | 16.667 / 41.665 | 62 / 4,900 |
| Mac / previous | 5.395 / 12.076 | 1 | 22.222 / 66.667 | 47 / 3,696 |
| Mac / candidate | 5.499 / 7.348 | 0 | 22.222 / 55.556 | 47 / 3,709 |

All four measured intervals complete with zero rejected input batches, renderer
errors, recorder overflow, missing presentation callbacks and zero-time
presentations. Their full traces retain zero-time presentations outside the
measured interval: respectively 3, 1, 2 and 1 in table order. Continuous interval
counts use the existing target-period plus 5% tolerance. Mac remains evaluated
at 90 Hz and iPad at 120 Hz. The iPad CPU tail and both presentation gates remain
open; a faster lookup benchmark does not close them.

The ordinary before traces show many long presentation intervals with a long
gap between frame admissions and no intervening denied display-link tick.
The largest examples repeat shortly after stroke contact begins, with fast
drawable acquisition in the following frame. This motivates investigating main
run-loop/UI work as well as drawable waits; it does not by itself identify the
blocking function.

A headless hardware-backed state probe with both Apple presets confirms that
stroke down/up legitimately changes roughly 100 UI fields, mostly menu/command
enablement and toolbar/history controls; ordinary moves produce no new full
snapshot. Suppressing those boundary updates would lose behavior. A subsequent
25-second Mac Time Profiler run completes in a 27 MB bundle. In its segment after
15 seconds, the main thread has 718 ms of sampled weight; 437 ms includes
AttributeGraph updates, while JSON field access accounts for 48 ms inclusively.
These sampled categories overlap and are diagnostic, not frame-budget evidence.
This points the next investigation toward repeated UI graph work when applying
required updates. Both probes remain local under `artifacts/apple-ui-state-probe`
and `artifacts/apple-json-ui-profile-*`.

Separate 45-second Metal System Trace recordings generated about 35 GB of data.
Their analysis was stopped after prolonged CPU-heavy processing. They produced
no accepted isolated-GPU result and are excluded from the table. The completed
in-app 90-second traces recorded alongside them are diagnostic only. Partial
Instruments bundles, frame traces, captures, build logs and detailed comparison
reports remain in ignored `artifacts/apple-json-*`, `artifacts/apple-metal-*`
and `artifacts/performance/*json-lookup*` paths.

## Selective native UI observation

Both Apple editors now retain one canonical immutable JSON snapshot and expose
live, main-actor readers for fields and individual command, panel and menu
entries. Required changes in stroke-boundary enablement still reach the UI.
Unchanged readers no longer receive a whole-editor publication, and camera-only
patches preserve the command/panel/menu indexes. The in-app menu view also owns
its array reads, preventing menu enablement from invalidating the entire iPad
editor through the header. Document and lifecycle consumers explicitly take
immutable whole-state copies. Rust remains authoritative for values and actions.

All related projections are staged before any observation signals are sent.
Comparisons preserve key presence, nulls, Boolean/number distinctions, precise
integers, signed zero and array order. Only previously read fields need their
individual values compared; whole-object readers additionally detect changes
to unread fields. This avoids allocating observation nodes and recursively
comparing values for unused fields. Retained JSON values never become live
mutable state.

Standalone checks cover those semantics and all 54 local wire fixtures (40
deterministic transport snapshots plus 14 hardware-backed stroke snapshots).
An invisible SwiftUI hosting view verifies fresh rendered values, unchanged
unrelated bodies, camera patches and no-op snapshots, including a child that
retains a field reader without its parent rebuilding. Direct shared drawer,
window-presentation and document workflow checks also pass. These checks use
no system menu automation. They do not establish full visual or physical-input
acceptance.

An initial candidate compared every field. A completed 25-second Mac CPU profile
showed 577 ms of main-thread sampled weight in its segment after 15 seconds,
versus 718 ms in the preceding lookup-only profile. Inclusive AttributeGraph
update weight fell from 437 to 283 ms, while `EditorStore.receive` rose from 9
to 110 ms. These overlapping samples motivated the final restriction to read
fields; they do not measure the final version or establish a frame-rate gain.
The first candidate's clean 45-second pair likewise did not establish a cadence
improvement: Mac long continuous intervals increased from 47/3,709 to 55/3,721,
and iPad from 62/4,900 to 106/4,947. Raw profiles and intermediate comparisons
remain private in ignored `artifacts/apple-projection-*` and
`artifacts/performance/*ui-projection*` paths.

The final implementation incorporates the shared changes through `9019e23`.
All 307 relevant Rust checks pass (34 Apple bridge, 18 host, 255 UI; one existing
hardware-only host check remains ignored). The final 40 value/byte snapshot
pairs are identical, and their decoded values also match the preceding
checkpoint. Both Release targets build, and the signed iPad build installs and
launches. The command audit still covers all 62 commands on each host; inventory
coverage alone does not close their behavioral acceptance.

The final clean 45-second Mac `wet-watercolor-4k` run on the 90 Hz display
records 3,764 measured presentations. CPU owner service p50/p95/p99/max is
3.395/4.913/5.560/14.519 ms, with one frame above 11.11 ms. Continuous
presentation interval p50/p95/p99/max is 11.111/11.111/22.222/44.445 ms;
49 of 3,735 intervals exceed the target period plus 5% tolerance. The interval
has no rejected input, renderer errors, recorder overflow, missing callbacks
or zero-time presentations. There is one zero-time presentation outside it.
Measured footprint grows 15.25 MiB and thermal state stays nominal. This is a
short CPU/presentation observation with GPU timing disabled and does not
establish a cadence improvement, physical-input latency or sustained acceptance.

The final 45-second iPad run at 120 Hz records 4,995 measured presentations.
CPU owner service p50/p95/p99/max is 2.061/3.509/9.095/12.703 ms, with 87 frames
above 8.33 ms. Continuous interval p50/p95/p99/max is
8.333/8.334/16.667/33.334 ms; 83 of 4,966 intervals exceed the target plus 5%
tolerance. Its measured interval has no rejected input, renderer errors,
overflow, missing callbacks or zero-time presentations. The complete trace
contains six zero-time presentations outside that interval. Measured footprint
falls 7.41 MiB and thermal state stays nominal. GPU timing is disabled. Neither
the CPU tail nor presentation cadence meets the iPad acceptance target.

The preceding iPad launch produced no trace and subsequently disappeared from
the process list. Its short diagnostic CPU recording contained no samples;
the cause of termination was not established. That attempt contributes no
performance result. After confirming it had ended, the same installed binary
was launched with console logging. The successful run above logged preparation,
measurement completion and postlude before exporting a fresh trace. Console
logging was connected for that iPad run, with phase messages only; no profiler
or build ran during its measured interval. The final Mac run used no console
connection. This environmental difference and the short sample sizes limit
comparisons; neither host's results establish a frame-rate improvement.

The later Android presentation and independent workspace-storage changes through
`13d139c` are also integrated. They change no Apple source or existing dependency
lock entries; the new storage crate is not an Apple dependency yet. New shared
workspace-manager flows and tab-drag animation parity remain separate work.

The final delivery additionally integrates tab-drag and workspace-transition
changes through `f6c58a7`. Both Release builds, all 310 relevant Rust checks
(34 Apple, 18 host, 258 UI), the direct drawer workflow and the 54-fixture Swift
observation check pass. All 40 value/byte snapshot pairs still match the earlier
values exactly. Both final apps complete a five-second 4K drawing smoke interval
after warmup, followed by the normal postlude, without rejected input, renderer
errors or missing/zero-time presentations in that interval. This establishes
startup and drawing after integration; the 45-second timings above precede it.
The smoke intervals do not establish performance acceptance. All completed
benchmark processes are closed and the original Mac editor remains open.

## Capture locally

Build with `CAPY_CONFIGURATION=Release` for performance investigations. See
[README.md](README.md) for signing and build options. Debug captures are useful
for validating instrumentation but do not close performance gates.

Set `CAPY_TRACE_GPU=0` to retain CPU, input, memory and actual presentation
observations while disabling the GPU timestamp marker submissions and readback
polls. The default is enabled when tracing is requested; ordinary unrecorded
launches still create no frame timer. The trace header and report expose
`gpu_timing_requested`. Disabled GPU measurements remain null, with an explicit
warning; they must not be treated as zero GPU cost. This comparison isolates
the optional GPU timer, not the remaining recorder overhead. Use the same build,
workload, duration and display state for each pair.

```sh
CAPY_CONFIGURATION=Release bash apps/layer-apple/scripts/build.sh macos
open -n --env CAPY_TRACE_SECONDS=30 \
  --env CAPY_TRACE_DIRECTORY="$PWD/artifacts/performance/mac" \
  apps/layer-apple/DerivedData/Build/Products/Release/CapyCanvas-Mac.app
```

For iPad, build/install the Release app using the README commands and then:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
xcrun devicectl device process launch --device DEVICE_ID --terminate-existing \
  --environment-variables '{"CAPY_TRACE_SECONDS":"30"}' art.capycanvas.apple.ipad
```

Keep the app foregrounded through the interval and the two-second callback grace
period. Copy its local trace directory after export:

```sh
xcrun devicectl device copy from --device DEVICE_ID \
  --domain-type appDataContainer --domain-identifier art.capycanvas.apple.ipad \
  --source Documents/Performance --destination artifacts/performance/ipad
python3 tools/performance/apple_trace.py TRACE.jsonl --target-hz 90 \
  --output artifacts/performance/report.json
```

`CAPY_TRACE_SECONDS` must be finite, positive and no more than 3600. The default
output directory is `Documents/Performance` in the app container. The optional
`CAPY_TRACE_DIRECTORY` overrides it with a local writable directory. A finished
trace is a JSONL file; `.partial` means export did not finish. Recording starts
when the session owner is created, so startup is included. Shader/catalog/canvas
readiness and display-link activity transitions are recorded separately.

Artifacts are ignored by Git. Records contain timings, counts, memory, thermal
state and display dimensions; they contain no coordinates, artwork, document
names, account/team/device identifiers or input-device serials. Keep build,
signing, device-tool output and trace files local. Use placeholders in shared
commands and review staged source before pushing.

## Interpret the report

- CPU owner queue age measures the interval from display-link admission to
  serial-owner execution. Owner service includes the bridge and snapshot work.
  The five Rust stages are CPU preparation, drawable acquisition, viewport
  submission, present call and polling. They are not GPU execution durations.
- GPU timestamps bracket queue work across paint and viewport submissions,
  including CPU submission gaps. They measure a **GPU queue span**, not isolated
  GPU busy time. Three readback slots and a 256-result queue bound profiler work;
  full slots skip observations. The owner never waits for a readback. A bounded
  trailing poll drains the last result when the display link sleeps. Each marker
  pass performs a tiny storage write; empty passes produced zero counters on
  Metal. Counter resolution is deferred until the marker submission completes:
  resolving within that submission returned stale values on the tested Mac.
  Both marker work and extra submissions contribute profiler overhead. Visible
  renderer Diagnostics now reuses this timer around drawing submissions;
  the optional recorder brackets the wider frame. Their spans remain distinct.
- Actual `presentedTime` values determine presentation intervals and lateness
  against the display link's target. Zero presentation time means skipped or
  unpresented, not zero latency. Missing callbacks are reported separately.
  Continuous cadence groups frames by display-link activity cycle, excluding
  intervals across recorded idle pauses. All intervals are also reported.
  The selected refresh-rate exceedance count uses a 5% cadence tolerance; target lateness over
  1 ms is a separate descriptive count. Neither is an acceptance waiver.
  Use `--target-hz 90` for current Mac validation and `--target-hz 120` for iPad
  (the default remains 120 for existing callers). Reports record the evaluation
  target and frame budget, and count CPU and continuous-cadence exceedances
  against that budget. Measured workloads retain their own continuous-cadence
  subset. The original `owner_service_over_8_33ms` and
  `continuous_intervals_over_120hz_budget` fields remain explicitly labeled
  diagnostics; they are not the current Mac 90 Hz acceptance thresholds.
  Refresh targets change report interpretation only, not app or input behavior.
  `frame_admission_to_present_ms` measures actual display time minus frame
  admission, independently of the scheduler's advertised target. It is software
  scheduling delay, not physical input-to-pixel latency. Comparing target
  lateness alone across different schedulers can conceal a changed target.
- Input enqueue/owner times measure transport queueing. The first presentation
  associated with each successfully received, nonpredicted batch is a **receipt
  proxy**. The renderer may defer that input or consume only part of its queue.
  This association does not establish that those pixels were included, nor
  physical Pencil input-to-pixel latency. Prediction remains visual-only.
  Corrections have separate batch counts, owner queue distributions and receipt
  proxies. Their sample timestamps remain the original observation times;
  correction delivery is measured from the new enqueue receipt.
- The report includes p50/p95/p99/max, missing/invalid/overflow counts, display
  capabilities, memory footprint and thermal states. Empty measurements are
null, not zero. Readiness requires canvas, shaders and bundled filter catalog.
  Startup memory growth and profiler storage are included in footprint; they
  must not be described as steady-state document growth.

The recorder reserves a capped array of fixed-size events (reported in the file
header), uses a short lock for concurrent callback appends, freezes once, and
streams JSONL on a utility queue. Memory sampling runs once per second. Timing
records and GPU timestamp submissions have overhead; run paired instrumentation
on/off investigations before drawing performance conclusions. Overflow or GPU
skips can bias distributions and must remain visible. Export keeps late callback
records for two seconds; missing completions at that boundary stay unverified.

## JSONL schema 1

The first line is metadata. Each remaining line is `[kind, a, b, ..., j]` with
unsigned integer fields; unused fields are zero. Host times use nanoseconds in
the CACurrentMediaTime monotonic clock domain. Frame IDs are the admission
timestamp. GPU queue durations use the queue's timestamp period. Raw GPU
endpoints and paired Metal clock samples retain their separate clock domains.

| Kind | Fields in order, excluding trailing zeros |
| --- | --- |
| 0 tick | admission time, target time, admitted flag, denial reason (0 unspecified, 1 inactive, 2 owner pending; historical 3 drawable capacity) |
| 1 frame | ID, target, owner start, owner end, five CPU stage durations, latest nonpredicted receipt ID |
| 2 input | enqueue ID/time, owner start/end, oldest/newest sample time, count, kind (0 real, 1 predicted, 2 correction), original last phase, tool, accepted flag |
| 3 drawable | frame ID, acquire start/end, drawable ID, acquired flag |
| 4 presented | frame ID, actual presentation time, callback observation time, drawable ID |
| 5 memory | observation time, physical footprint bytes, resident bytes, thermal state, Mach status |
| 6 display | observation time, pixel width/height, scale multiplied by 1000, maximum refresh rate |
| 7 GPU | frame ID, GPU queue span, status (1 valid, 2 readback failure, 3 invalid timestamps), raw GPU start/end ticks |
| 8 GPU status | observation time, support (0 uninitialized, 1 supported, 2 unavailable), requested/skipped/invalid/pending counts, poll-error flag |
| 9 state | observation time, frame ID, flags (1 canvas ready, 2 catalog loaded, 4 another frame needed, 8 shaders ready), frame-error flag |
| 10 activity | observation time, display-link awake flag |
| 11 workload | observation time, phase, profile ID, phase-dependent counters |
| 13 presentation retry (historical) | attempt time, original display target, admitted flag, denial reason using kind 0 values |
| 14 GPU clock | recorder time before sampling, Metal CPU nanoseconds, Metal GPU ticks, recorder time after sampling |

Optional GPU recording samples paired clocks at most ten times per second.
The analyzer follows Apple's [GPU-to-CPU timestamp conversion](https://developer.apple.com/documentation/metal/converting-gpu-timestamps-into-cpu-time),
interpolating only between recorded samples. Recorder times surrounding each
call bound the translation from Metal CPU time to the recorder clock. Reported
uncertainty covers that sampling window, not unknown clock drift. Invalid or
nonmonotonic clock samples disable calibration; absent or unbracketed endpoints
remain missing. These observations are available for both the whole trace and
its measured workload interval. The GPU end marker follows the frame's queued
work and includes submission/polling gaps; it is not isolated GPU busy time.
The display target is not a recorded Metal commit deadline, so completion before
that target alone does not establish the cause of a missed presentation.

The analyzer also retains local scheduling experiment records: kind 12 contains
frame ID, CPU commit deadline, presentation target and drawable admission status
(0 ordinary acquisition, 1 supplied drawable accepted, 2 stale drawable rejected).
The optional sixth field of kind 6 records the requested Metal frame latency;
zero means unavailable. The published CADisplayLink hosts do not emit these
experimental fields. Owner completion includes polling and snapshot publication,
so lateness relative to the commit deadline is an upper bound, not a measured
Metal commit timestamp.

Workload phases: 0 configuration, 1 warm-up begins, 2 measurement begins,
3 measurement ends, 4 postlude ends, 5 failure, 6 producer sample. Phase 0's
remaining fields are width, height, paint-layer count, brush ID, diameter ×1000
and prediction flag. Phases 2/3/6 record cumulative nonpredicted sample and batch
counts, followed by the maximum producer lateness since its previous sample.
The metadata `workload` object includes the profile version, expected duration
and sample rate. These additions retain schema 1; older traces omit them.

## Fast checks

```sh
cargo test -p layer-render-wgpu --lib frame_timing::tests
python3 -m unittest discover -s tools/performance -v
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/FrameTrace.swift \
  apps/layer-apple/tests/frame-trace.swift -o /tmp/capy-frame-trace-tests
/tmp/capy-frame-trace-tests
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/DrawingWorkloadPlan.swift \
  apps/layer-apple/tests/drawing-workload-plan.swift -o /tmp/capy-workload-plan-tests
/tmp/capy-workload-plan-tests
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/CanvasFrameDriver.swift \
  apps/layer-apple/tests/frame-driver.swift -o /tmp/capy-frame-driver-tests
/tmp/capy-frame-driver-tests
```

The GPU test requires timestamp-capable hardware and checks frame identity,
strictly positive timestamps, bounded pending observations, completion and slot reuse. The Swift test checks
concurrent capacity, overflow, freeze and late presentation records. The Python
tests preserve active missed frames while excluding idle gaps, prevent missing
or invalid timings becoming zeros, deduplicate input receipt associations and
exclude shader startup from the ready subset. These tests use no UI automation.
Workload checks cover contact termination, lift gaps, pressure, coordinates and
invalid configuration. Report checks retain incomplete/failed measurements and
missing render observations even when the input producer completes.
