# Measuring performance

[Performance targets](../PERFORMANCE_TARGETS.md)

## Rules

**What counts as a frame.** Two rates are used, depending on who paces the
frames:

- Brush strokes use **completed canvas updates per second**, as reported by
  `completed_updates_per_s` in `tools/performance/android-brush-report.py`. This
  counts GPU-completed, nonempty canvas updates inside the input window. Android
  draws ink into the front buffer, so this rate can exceed the display's refresh
  rate.
  Record `input_updates_per_s` and `input_completion_gap_p99_ms` alongside it.
  These count the first completed update consuming each new down or move input;
  prediction and refinement can produce further canvas updates between inputs.
  Each completion records the engine's last consumed paint timestamp. An input
  waiting in the queue cannot turn a refinement or cursor update into new ink.
  A high canvas update rate alone does not establish fresh input throughput.
- Navigation, drags, sliders, animations and other display-paced motion use
  **presented frames per second**: SurfaceFlinger actual-present times,
  `FrameMetrics`, or the host's presentation timestamps.
  - Renderer submissions per second are recorded where no presentation data
    exists. They are marked "renderer", because a submission is not a displayed
    frame.

**When a target is met:**

1. The rate is at least the target.
   - Display-paced motion on a panel whose refresh equals the target only needs
     95% of it: 57, 85.5 and 114 fps. This matches the workspace-motion floor
     `LAYER_MOTION_MIN_HZ`.
2. The 99th-percentile interval between frames or updates is at most two frame
   budgets: 33.3, 22.2 and 16.7 ms.

**How to measure "sustained".**

- Use at least three gestures of 5–10 s each, in a release or benchmark build.
- Warm up first: pipelines compiled, and one priming gesture undone.
- Run on the reference device at thermal status 0, using its default display
  settings and the default panel glass.

**Only motion counts.** A frame where nothing moves may take longer. This includes
the frame after a drag is released and a frame waiting on a click. The exception
is a still frame that delays the start of the next motion. Start and release
latencies have their own limits under [Responsiveness](responsiveness.md).

**The target document.**

- It is the tier's photo as the bottom layer plus one empty paint layer at Fit
  zoom, with the default workspace.
- Rows that name a layer count use that many visible layers instead.
- The photos are the Android brush benchmark's 9504 × 6336 Sony JPEG and exact 3:2
  downscales of it.
- The camera is sold as 61 MP; its image is 60.2 MP.

**Brush sizes.**

- Sizes are document-pixel diameters at pressure 1. The UI allows 0.5–2048 px.
- "Guaranteed" means every diameter up to that size meets the tier rate, with the
  preset's default spacing and settings.
- Above the guaranteed size a brush may run slower. It must still draw correctly,
  keep every input sample, and never lose the GPU device.

**Soft targets.** Full-screen filters and adjustments are soft targets. A filter
may run below its target only when two things are both true:

- A calculation shows that its FLOPs or memory traffic at the tier's canvas size
  exceed what the reference hardware can deliver in one frame budget.
- No valid approximation exists.

Record that arithmetic next to the row.

Previewing at display resolution while a control moves, then committing at full
resolution, is a valid approximation. Band-limited or tiled evaluation of the
visible region is one too. A filter cannot claim the waiver while an
approximation like these would reach the target.

For scale, take a single full-resolution pass that reads and writes 8-byte
(RGBA16F) pixels. It moves 16 bytes per pixel:

| Tier | Canvas | Bytes per pass | Peak bandwidth | Passes/s at peak |
| --- | --- | --- | --- | --- |
| Low | 12 MP | 0.19 GB | 14.4 GB/s | 74 |
| Mid | 24 MP | 0.38 GB | 17.1 GB/s | 45 |
| Top | 60.2 MP | 0.96 GB | 67.2 GB/s | 70 |

- Real sustained bandwidth is lower than peak, and the renderer's RGBA32Float
  pages double the traffic.
- So per-frame full-resolution filtering cannot meet the mid or top tier target
  even for a pointwise adjustment. On the low tier, a pointwise adjustment fits
  only as a single pass.
- Full-frame previews must therefore reduce work during motion. Display-resolution
  evaluation is one option; native-resolution dirty regions can be cheaper for
  local edits. Choose from the actual dependency footprint, not canvas size alone.

## Live filter performance gates

Every new or substantially changed filter needs a performance note before its
algorithm is accepted, and measured results before release. Record the expected
class, ordinary and demanding parameter sets, dependency/invalidation model,
per-stage cost estimate, memory bound and proposed fallback. Update the note when
the algorithm changes. It is implementation evidence, not an authored parameter,
saved shader version or quality selector. The
[illustration proposal](../development/illustration-filters-proposal.md#first-pass-filter-performance-classes)
assigns initial classes by expected use; no class is a measured result.

### Targets and conditional slower results

All classes aim for the normal 60/90/120 tier rate. Fast means artists should
expect immediate drawing through an ordinary setting; Medium covers meaningful
neighborhood or multi-stage work; Slow covers substantial stylization or broad
sampling. Do not relabel a cheap effect Slow to excuse its implementation.

The following are initial release limits for the illustration work, not hardware
predictions or permanent file-format promises. Tune them only with measured
journeys and a reviewed product decision, not to make an individual failure pass.

| Expected class | Normal goal, low / mid / top | Lowest conditional fresh filtered updates/s, low / mid / top | p95 age of latest filtered ink during drawing | p95 settle after an ordinary local stroke |
| --- | --- | --- | --- | --- |
| Fast | 60 / 90 / 120 | No slower allowance for ordinary settings | At most two normal tier frame periods | 250 ms |
| Medium | 60 / 90 / 120 | 30 / 45 / 60 | 100 ms | 500 ms |
| Slow | 60 / 90 / 120 | 15 / 22.5 / 30 | 150 ms | 1 s |

At the qualified rate, require p99 gaps between fresh filtered completions of at
most two periods of that rate, in addition to the age limit. A high average with
bursts of stale frames fails. Measure age continuously during active input: the
latest available input timestamp minus the newest input timestamp incorporated
in the displayed filtered result; also report input-to-GPU-completion latency.
Use presentation timestamps when available and label completion-only proxies.
Count only revisions containing new authored input, never cursor motion,
prediction-only redraws or repeatedly presenting the same filtered image.
Completion must cover the visible affected output for that input revision; one
fresh tile cannot stand in for stale required neighbors. Unchanged regions need
not be reevaluated. During startup with no filtered result for the contact yet,
measure elapsed time from pen down rather than age from an earlier stroke.

Use the normal pen-down submission, filter-control response, navigation and
interrupted-refinement limits from [Responsiveness](responsiveness.md) and
[engineering budgets](../PERFORMANCE_TARGETS.md#engineering-budgets). First
filtered ink must appear within the class's age limit; submitting unfiltered
paint is not proof of filtered feedback. Input samples must all reach the
committed paint. Derived
renders may coalesce obsolete revisions, but the visible result must keep
advancing and converge to the latest source. Never silently bypass the filter.

The settle limits above apply to the recorded ordinary local-stroke fixture,
not a full-canvas replacement. Record broad damage, initial application, cold
preparation, Match/Update and exact export separately with their own predicted
completion budgets and measured elapsed times. These jobs may take longer but
must remain cancellable and must not delay resumed motion. A live filter that
cannot bound ordinary settling cannot qualify by calling every stroke setup.

A Medium/Slow allowance requires **all** of the following for that filter,
parameter set, stack and tier:

- The optimized dependency footprint and calibrated hardware model explain the
  missed normal target. A busy GPU or the cost of a deliberately naïve algorithm
  does not establish a hardware limit.
- Dirty-region evaluation, reuse, a cheaper algorithm, and a faithful moving
  approximation have been evaluated. If a suitable alternative meets the normal
  target, use it. Do not reject a useful preview merely because it is not exact.
- The lower rate, age/settle bounds, visual quality, memory admission and resumed
  input limits all pass. Reduced-rate filtered results are permitted; reduced-rate
  input processing, navigation or UI are not.
- The tier table records **qualified slower filter**, its exact workload and
  evidence, separately from **tier target met**. An unmeasured or unexplained
  miss stays open; one tier's exception does not qualify another.

### Painting workload and incremental correctness

The primary workload paints into the filter's actual input: on its owner, inside
a filtered group, or on paint below an adjustment that consumes that paint.
Pair each run with the same graph with the filter disabled. A filtered static
photo behind an unaffected front paint layer does not qualify this path.
The existing Android runner's `blurred-base` workload is that latter case; extend
the harness and revision/completion attribution before claiming filter coverage.
Do not invent a command-line option for a workload that is not implemented.

Use the normal reference canvas, workspace, warmup and repeated gestures. Cover
a 64 px detail brush and the tier's guaranteed large G-Pen size, Fit and 100%
zoom, interior strokes, tile crossings, separated dirty islands and source edges.
Use visible non-default parameters, an erase/undo case and partial alpha; a
zero-strength or empty-input early exit is a control, not a qualification run.
Also measure expensive controls through the ordinary slider range, admitted
extremes, a representative stack, and source/map/seed/parameter changes. Use at
least 20 repeated pen-up/resume contacts for settling and interruption percentiles;
report long-stroke settling separately from short-contact settling. Do not infer
a p95 claim from three run medians.

Record brush/source completion, fresh **filtered** completion, presentation,
result age, p99 gaps, pen-up settling and interrupted settling separately.
Measure end-to-end latency as well as shader time. If the disabled baseline
already misses, report that shared gap and the incremental filter cost; do not
claim either an absolute pass or a filter-specific hardware exception from it.

### Acceptance when the disabled baseline misses

A filter can be accepted while shared baseline performance improvements are
deferred if its matched disabled control already misses the applicable targets.
Require current reference-device measurements of throughput, age, gaps,
settle/resume and memory, with an added cost small enough to be explained by the
simplest adequate algorithm and its actual dependency footprint. Check simpler
alternatives and eliminate unnecessary regeneration, passes, copies and retained
work. A favorable rate ratio or lightweight shader alone is insufficient.

Preserve native color/alpha, every authored input and the existing final-result
correctness and error contract. Report material incremental latency and memory
costs, including interrupted resume. Correctness failures or unnecessary filter
work still block acceptance; unresolved latency must remain explicit rather than
being attributed to the baseline. Record the verdict as **incremental efficiency
accepted; baseline target gap deferred**.
Keep absolute misses visible: this is neither a tier-target pass nor a
filter-specific hardware exception. Workloads whose disabled baseline meets the
targets retain the normal filter gate.

### Dependency correctness

Each stage must declare both directions of dependency: which input an output
region samples, and which output an input edit invalidates. These differ for
displacement, radial paths and directional shadows. Compose them across passes,
masks and stacks, including halo growth, changed cell aggregates and off-frame
content. A tile edge is never an image edge. A single maximum-radius field is
not a substitute for a correct dependency model.

Require incremental and full evaluation to agree within the same operation's
tolerance after painting, erasing, undo, parameter changes and resource changes.
Compare at identical quality, coordinates and seed; separately qualify preview
approximation error. Updating one small region on a larger canvas must not
silently trigger work proportional to the full canvas for a local algorithm.
Trace dirty input/output pixels, processed pixels per stage/mip, reused work,
dispatches, allocations and bytes. Include distant islands to catch wasteful
bounding rectangles. A global algorithm must declare and budget its global
dependency, use valid incremental summaries, or retain explicit Update results;
downsampling a whole-layer analysis does not make it local.

### Hardware cost and algorithm efficiency

Model the smallest adequate algorithm before optimizing its shader. For each
pass record processed pixels including repeated halos, texture formats and
reads/writes, logical sample count, FLOPs, integer/special operations, atomics,
intermediate storage, dispatches and cache lifetime. Distinguish logical texture
taps from external-memory bytes; filtering and cache hits make them unequal.
Use actual renderer formats, not an assumed RGBA8 buffer. Shared intermediates
must have complete invalidation keys and bounded residency.

For a pass with external traffic `B` and FP32 work `F`, a first optimistic bound
is `max(B / peak_bandwidth, F / peak_FP32_throughput)`. Use consistent units and
count a fused multiply-add as two FLOPs. For sequential stages, account for each
stage on the critical path. This bound cannot predict texture throughput,
integer-heavy connectivity, division/exp, divergent paths, register pressure,
allocation or submission overhead. The
[instruction Roofline research](https://amcr.lbl.gov/wp-content/uploads/2025/11/InstructionRooflineModel-PMBS19-.pdf)
explains why floating-point throughput alone misses integer-heavy GPU bottlenecks;
the [Arm counter reference](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/mali-bifrost-family-performance-counters)
distinguishes shader operations, texture traffic, cache misses and external bytes.
These guide profiling; their device-specific throughput figures do not qualify
another GPU or API.

Calibrate with representative kernels, formats, tap counts and dirty sizes on
each reference device, including short-dispatch overhead. Use the
[tier sheet](hardware.md) as an initial ceiling, not measured sustained capacity.
Compare observed stage timings/traffic and total filtered-stroke latency with the
calibrated prediction. Costs more than 1.5× that estimate require investigation
under the engineering-budget rule; an unexplained gap blocks qualification.
Correct a deficient model from independent counters or matched microbenchmarks,
not by fitting an arbitrary efficiency factor to the slow filter itself.
Use bounded asynchronous diagnostics where counters are unavailable; never add
a GPU wait on the input path to obtain them.

The 12/8/6 ms GPU budgets cover painting, all filters, composition and presentation
together. Each filter does not receive a separate full-frame budget. Measure
stacks, memory pressure and sustained thermal behavior. Whole-document traffic
is not the right lower bound for a small dirty edit; a fast isolated kernel is
not evidence for a fast integrated stroke.

Reject a candidate as wasteful, even if it barely meets FPS, when a tested
alternative offers equivalent required quality with materially less work or
memory and no compensating advantage. Specifically reject:

- Full-layer recapture, upload, readback, histogram/tensor rebuilding or allocation
  on every dab when only bounded regions or already saved results changed.
- Recomputing unchanged upstream stages after only ink color, opacity or final
  grain strength changes, or regenerating static noise/texture fills on painting.
- Large brute-force two-dimensional Gaussian kernels instead of the appropriate
  separable method, per-pixel re-summing of the same mosaic/Voronoi cell, or
  radius/length-squared work where a validated cheaper method preserves intent.
  Small direct kernels may win on dispatch cost; compare actual break-even points.
- Treating exact disk morphology or nonlinear smoothing as separable without
  proving the result, omitting halos to save time, or reducing preview detail
  until ink, tone, thin lines or transparency no longer match the intended look.
- Letting obsolete refinement queue without bound, monopolize the GPU, or publish
  over newer artwork. A native-resolution export job is not a per-stroke strategy.

Freeze a measured operating envelope and any justified exception before release.
Failure of correctness, locality, efficiency or responsiveness remains a failure
even if the average FPS passes. Improve or replace the algorithm; if the complete
gate still fails, apply the proposal's explicit release cut rule.

## How to measure

Record every result with its device, build (commit and profile), canvas, brush,
size and date. Keep raw data in ignored `artifacts/` directories, and put the
headline number with its source in the table.

**Brushes (Android).**

- Reserve the tier's tablet and run device commands through
  `tools/devices/devices.py run` ([devices](../development/devices.md)). Build
  the release benchmark as the
  [Android guide](../development/android.md#brush-workload-benchmark) describes.
- Push the tier photo:
  - Top tier: the 9504 × 6336 original, at `/data/local/tmp/capy-brush-photo.jpg`.
  - Mid and low tiers: its 6000 × 4000 or 4248 × 2832 downscale, to any
    `/data/local/tmp/*.jpg`.
- Then run:

```bash
python3 tools/performance/android-brush-benchmark.py OUT --serial "$CAPY_ANDROID_SERIAL" \
  --package art.capycanvas.brushbench --photo /data/local/tmp/capy-tier-24mp.jpg \
  --presets 1,5,3 --size 1536 --radius-x 310 --radius-y 150 --prefix mid
python3 tools/performance/android-brush-report.py OUT --package art.capycanvas.brushbench
```

The runner defaults to the dry presets. Pass `--presets` to include the wet,
smudge and Liquify presets. The photo opens as a Perceptual document;
`--blending linear` measures it in linear light. `--color-mode grayscale` or
`--color-mode two_tone` constrains the empty paint layer; `full_color` is the
default. The benchmark draws a 200 Hz
stylus ellipse at Fit zoom, three 10 s strokes, with the default 16 ms
prediction.
Qualification runs keep the default workspace. `--trace` enables bounded GPU
timestamps and phase attribution without opening Stats. `--stats` opens Stats
and enables timestamp sampling even without a trace. Record diagnostic runs
separately: timing queries add work, and opening the panel also changes the work
area and Fit camera. Older runs without a `stats_panel` field always opened
Stats.
Display-graph traces split source preparation, main and overview composition,
and their mip reductions. Retained Navigator refreshes have a separate GPU phase,
CPU span, pixel count, image age, pending flag and storage counter. Main
composition includes source updates performed
inside that view. Trace mode flushes queued composition records before the mip
timestamp so the mip interval excludes deferred blends.
Idle exact refinement uses the main composition phase for native capture and
the main mip phase for updating the resident hierarchy.
Ordinary stroke publication has a separate `Capy GPU native capture ns` total.
`Capy GPU native preflight ns` covers the complete working-source validation,
including the initial shared status reset, before any native output writes.
`Capy GPU native encoding ns` covers native color/scalar encoding and any
canonical scratch promotion after that preflight. The total includes the split
markers; CPU resource preparation and backing readback/compression are outside
these GPU intervals. All six possible marker passes and the complete native job
must fit the current encoder before timing starts. Instrumentation never forces
a rotation or submission. Capacity or timer-slot omissions are explicit counters,
not zero cost; compare only matching renderer IDs with complete split observations.
Background/private jobs retain their existing timing behavior. These timestamp
intervals include GPU scheduling gaps and do not identify hardware occupancy or
prove that transfer arithmetic, validation, or memory traffic dominates.
Match GPU observations
to renderer frame IDs inside the input window. Ready timing counters drain at
each measured frame; final observations can arrive after motion has ended. Each
timer retains at most 256 ready observations between drains. Missing observations
are skipped, never waited
for, and do not represent zero cost.
Trace counters report the main composite's changed output pixels, output regions
and enclosing mip rectangle. Compare these on the same frames before replacing
the rectangle with sparse mip work; pixel-area savings alone do not establish
lower GPU time when the replacement needs more dispatches.
Keep the whole stroke footprint inside the photo for sustained painting tests.
Choose `--radius-x` and `--radius-y` from the photo's displayed bounds and brush
radius; the default work-area ellipse can leave a small Fit-view canvas. Compare
the observed camera, radii, layer count and settings in each `*-info.json`, even
when both invocations use identical command-line arguments.
Use `--workload clipped` for ordinary paint clipping or `--workload blurred-base
--effect-radius 8` for painting above a photo with attached Gaussian Blur. Keep
these separate from the default no-effects comparison. Record the reported
owner/effect handles, actual sigma, initial layer state and source manifest with
each run. The tier photo is opaque; these workloads exercise dependent composition
and resident-owner reuse, while shared pixel fixtures cover soft expanding alpha.
The runner waits for pending composition and queued GPU frames after each stroke.
Reports include `settled_after_input_ms`; completed updates per second still count
only nonempty updates completed inside the input window. Keep settling and the
next input's latency separate from that throughput measurement. `--mode pauses`
alternates 100 ms of drawing with 100 ms gaps. Set `--contact-ms` and `--pause-ms`
to compare the same contacts with and without pending refinement; the duration
must contain a whole number of contact/gap pairs. Its throughput excludes the gaps
and work completed after each contact; report contact queue delay, presentation
queued after consuming the contact, and GPU completion separately. Input records mark contacts that reached the
renderer while composition was pending. The GPU completion metric is not scanout
latency.

`--mode settle` exercises navigation and tool changes during Healing finalization
while a following paint contact waits for its source pixels. Record input queue
delay, tool action latency, camera changes, screen presentation and total settling
time separately. Its one-second pinch probes interruption; it does not qualify
the separate sustained five-second navigation target.

**Memory (Android).** Run a separate diagnostic with `--memory` to save GPU
allocator snapshots and process PSS. Each run starts a fresh memory log.
Use `--memory-idle-ms 30000` to retain the final stroke for thirty seconds before
Undo, with snapshots throughout and separate before/after idle records.
Sampling allocations adds CPU work, so do not use that run
to qualify frame rates. Keep process PSS, allocator allocated/reserved bytes and
system `MemAvailable` together. Android drivers can hold GPU allocations outside
process PSS; PSS alone cannot establish the memory bound. These measurements
overlap and must not be added together. Monitor the entire operation through
completion, including warm-up, pen-up, undo and deferred captures.

**Navigation (Android).**

- `AndroidViewportBenchmarkTest` measures presented pan and pinch rates
  (`-e viewportBenchmark true -e motion pan|pinch`).
- The brush runner's `-e mode pinch` does the same on the photo.

See [Android development](../development/android.md#benchmarks).

**Transforms, placement, selections and the canvas bar (Android).**

- Run `AndroidCanvasBarBenchmarkTest` with `-e width` and `-e height` set to the
  tier canvas.
- Its UI scenario opens the requested photo, adds an empty paint layer and uses
  Fit zoom. Pass `-e photo` with the tier Sony photo for target measurements; the
  generated image default does not establish the target workload. It primes the
  transform and bar transitions before collecting UI samples.
- Its "Hz" is renderer submissions. `gpu_completed_hz` counts completed canvas
  updates, including retained Navigator refreshes, so it does not establish fresh
  input throughput. Its JSON also records `display_hz` and the UI `FrameMetrics`.

**Web on a tablet.** In your own tablet Chrome tab ([devices](../development/devices.md)),
`tools/performance/web-pen.mjs` draws timed strokes over DevTools
(`LAYER_DEVICE_CDP`, `LAYER_WEB_URL`) and reports submissions per second, frame
CPU time and event-to-submission latency. `--os-input` replays a 200 Hz stylus
through Android's input dispatcher instead, using a helper built from
`tools/performance/AndroidPenMotion.java`; never pool the two kinds of run.
`tools/performance/web-refresh.mjs` reloads the tab and records startup
milestones. Both scripts list their options in their headers.

**Workspace motion (desktop and Web).** Use
`tools/performance/workspace-motion.sh` with `LAYER_MOTION_MIN_HZ` at 95% of the
tier rate.

**Brushes (Windows).** Build `cargo build --locked --release -p layer-render-wgpu
--example brush_frames`, list adapters with `brush_frames.exe --adapters`, then
run `brush_frames.exe OUT.csv dx12 240 3 [preset-ids]`. It draws a 1000 px brush
on a 9504 × 6336 canvas and reports completed generations, not displayed frames.

**Pen latency (Windows).** Against a Release build, run
`tools/performance/windows-pen-latency.ps1 -Executable <exe> -Project <.capy>
-OutputDirectory <dir>`, then `node tools/performance/windows-pen-report.mjs <dir>`.
It paces a pen circle at up to 240 Hz and records actual injection timestamps;
a delayed sample never triggers a catch-up burst. The report matches inputs to
DXGI frame statistics, so it reports software input-to-display time, not
input-to-photon. Per-window traces use `latency-<pid>-<window>` names. PresentMon
needs administrator rights.

**Desktop GPUs.**

- `layer-bench` and the renderer examples give completed-work timings, described
  in [GPU raster benchmarks](../development/gpu-raster-benchmarks.md).
- The Apple hosts record presentation, described in
  [Apple performance](../../apps/layer-apple/PERFORMANCE.md).
- Offscreen numbers exclude input delivery and presentation. They do not replace
  device measurements.
