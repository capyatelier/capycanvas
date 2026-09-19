# Color-management delivery milestones

[Implementation design](color-management-research.md) ·
[User journeys](../ui/color-management.md) ·
[Benchmark guidance](../development/gpu-raster-benchmarks.md)

**Proposed execution plan, 2026-09-13. None of the four milestones is complete.**

Deliver four substantial outcomes on main. Benchmarking, platform integration
and cleanup belong inside each milestone. There is no prescribed PR count or
separate milestone for each subsystem. Use a cohesive delivery change; extract
supporting merges only when they make implementation and review materially easier.
Every merge keeps the currently exposed application workflows working.

The implementation design retains the detailed contracts and acceptance corpus.
This plan sets the delivery order and explicit success criteria.

## Milestone map

| Milestone | Outcome on main | Depends on |
| --- | --- | --- |
| 1 — Raster foundation and efficient SDR8 | New raster save/undo/recovery, encoded sRGB8 editing and a measured shared rendering foundation. Old archive/replay and linear8 painting paths removed. | — |
| 2 — Complete SDR color and photo editing | Integer8/integer16, wide-gamut color, reversible photo controls, inspection and profiled interchange within measured memory/latency limits. | 1 |
| 3 — Print proofing | Reliable print simulation with proof and delivery profiles kept separate. | 2 |
| 4 — HDR editing and delivery | Half-float HDR editing, useful SDR viewing/rendition and tested HDR export. | 2; 3 for mapped HDR proofing |

Follow this delivery order. Milestones 1 and 2 are useful releases in their own
right; SDR does not wait for print or HDR. Cross-platform qualification is part
of each milestone's success criteria, not a separate final phase.

## Gates that apply to every milestone

Capture the hardware baseline at the start of milestone 1, before changing the
rendering path. Use the current drawing suite plus representative sparse 4K art,
dense 24/45/60 MP photos, multiple documents and concurrent save/export tasks.
These are qualification workloads, not a promise of 120 Hz at every size on every
device. New workflows join the matrix as they become available.

Record exact commits, device/driver/OS/browser, display refresh, power conditions,
workload, profile/depth and sample counts. Compare both with the merge's parent
and a fixed program baseline so repeated small regressions do not accumulate
unnoticed. Establish actual per-device memory and latency budgets before enabling
a new path; an unfilled budget or missing measurement does not pass.

| Gate | Required evidence |
| --- | --- |
| Correct data | Exact committed tile/mask/profile round trips and undo restoration. Edited results meet operation-specific reference tolerances defined before acceptance. Include alpha, physical pass boundaries and failure/cancellation. |
| Responsive interaction | CPU submission, GPU completion, p95/p99 input-to-present, missed presentation deadlines and sustained behavior on the qualified hardware. Measure cold/first-use and warm interaction separately. Frame cadence alone does not establish input latency. |
| Bounded memory | Peak/steady CPU and GPU allocations, source/composite/scratch/cache/history copies, staging and worker queues all fit explicit budgets. Account for process pressure on unified-memory devices. No silent precision reduction. |
| Efficient work | Dirty regions, pass counts and cache invalidations match actual dependencies. No per-dab conversion pass, warm-path shader compilation, blocking input-thread readback or unjustified full-document rebuild. |
| Complete integration | Relevant shared tests, host builds and real native/browser workflow checks pass. Numerical/GPU tests accompany visual review. Every exposed mode completes its supported Open/Edit/Save/Export route. |
| Cleanup | Replacement merges delete superseded codecs, APIs, shaders and unused dependencies. No old-project reader, migration, compatibility renderer or old filter-ABI adapter. |

For unchanged hot paths, initially hold and investigate a reproducible p95/p99
increase greater than `max(0.05 × baseline time, 0.2 ms)`. Calibrate noise before
using that comparison. This is a proposed regression trigger, not a per-merge
allowance; exceeding an absolute device/workload budget fails regardless of the
relative change. Correctness changes and the extra useful work of integer16/HDR
need explicit measured budgets, not a claim of identical cost to SDR8.

Start with a Linux reference GPU and a constrained mobile GPU. Qualify Metal,
D3D12 and actual browser paths as their integration changes. Each milestone must
cover its declared support on GTK, Web, Android, macOS, iPadOS and Windows; missing
hardware evidence remains outstanding. Run targeted checks for an individual
change and the complete affected matrix before declaring the milestone done.
There are no new hardware measurements behind this planning document.

## 1 — Raster foundation and efficient SDR8

**Outcome:** the existing drawing app uses lossless editable raster projects and
encoded sRGB8, with one efficient renderer and no legacy persistence path.

**Work:** establish baselines and allocation accounting; introduce explicit pixel,
color and alpha contracts across Rust/FFI/hosts; measure representative integer16
and FP16 working kernels before freezing the raster schema. Keep Float32 math,
bounded working tiles and the existing batching/incremental rendering advantages.

Implement raster revisions, affected-region undo, lossless indexed storage,
stable save snapshots and bounded asynchronous readback/compression. Preserve
layers, masks, sources and embedded live adjustments. Define active-stroke,
late-input-correction and wet-state boundaries. Distinguish GPU-completed edits,
host-backed recovery checkpoints and durably published saves.

Move existing brushes, effects, transforms, sampling and export to the selected
encoded8 path. Prepare dependencies before switching production. Activate the
new raster save/undo/recovery and encoded8 rendering together, updating every
caller and deleting their predecessors in that replacement. Live dab generation
and bounded in-progress stroke state remain; historical reconstruction does not.

**Scope and shortest route:** this is primarily a document/renderer/persistence
replacement with minimal visible UI. Reuse the existing layer model, brush
algorithms, compositor damage tracking, effect fusion and file-operation state
machine. Raster content ownership and history must change; the whole editor
does not need redesigning. Keep the exposed document mode at sRGB8 for this
milestone. New profile/depth controls and other color-management UI belong to
milestone 2.

Capture the current hardware baseline, then prove one complete internal path:
paint a tile → commit a raster edit → undo/redo → snapshot → save/reopen → paint
again. Use the existing 256×256 paint pages and add explicit encoding/alpha and
revision ownership. Exercise low-opacity blending and asynchronous snapshots in
that path before extending it across every existing tool. Representative
integer16/FP16 kernel checks inform the working-buffer choice; they do not expand
this milestone into the complete photographic or HDR workflows.

Use one tile-revision model for painting, undo and save snapshots, while keeping
their lifetimes and durability states explicit. Reuse immutable tile data and
capture only changed regions; compression and file I/O stay off the input owner.
Change texture load/store, sampling and quantization boundaries around shared
Float32 math. Retain existing cache layouts where they meet this milestone's
budgets; the wider source/composite residency work belongs to milestone 2.

After the complete path and existing tool families pass the gates, connect the
host transports and activate/delete the predecessor paths together. Visible UI
changes should be limited to existing file-operation busy/cancel/error handling,
correct dirty-state reporting and a clear unsupported-old-project error. Reuse
New/Open/Save/Save As/Export and undo/redo; no new color menus are required here.

**Success criteria:**

- New defaults to sRGB8. Existing paint/erase, wet/smudge, fill/gradient, mask,
  transform, blend/group/clipping and built-in/custom-effect workflows pass their
  reference checks, including fused versus physical-pass execution. Corrected
  encoded8 output is not judged against the old linear8 quantization defect.
- Draw → save → reopen → continue editing works with exact committed raster,
  mask and profile data. Live adjustments remain editable. Undo/redo restores
  stored states without historical stroke reconstruction.
- Save/autosave during drawing, cancelled/failed open/save/close and device-loss
  tests preserve the specified checkpoint and dirty state. Recovery never claims
  to restore GPU-only samples lost before capture. Repeated saves reuse unchanged
  content; snapshot/history queues stay bounded.
- Before/after hardware reports show existing qualified drawing workloads within
  the latency and memory gates, including pen-up, undo and background snapshot
  contention. No per-frame full-document readback or unexplained cache growth.
- Integer16/FP16 kernel comparisons establish a specific sampling, blending and
  working-buffer strategy before the schema is fixed. No FP16 bottleneck is
  assumed acceptable for future integer16 precision.
- Every supported host completes the replacement workflow. Old project IDs fail
  before replacing a live document; old archive codecs, replay-only state,
  linear8 paint targets and obsolete APIs/ABIs are removed.

## 2 — Complete SDR color and photo editing

**Outcome:** users can draw in 8-bit wide gamut or edit a 16-bit photograph, inspect
and revise its adjustments, and deliver a correctly profiled image efficiently.

**Work:** bound source, composite and filter residency before enabling the large
integer16 workflow. Use source-backed tiles, copy-on-write edits, bounded decoded
caches, appropriate mips/halos and streaming conversion/output. Global effects
need explicit scheduling and measured limits. Display-only reduced-precision
caches must never feed document edits, exact sampling or export.

Complete integer16 backing and the common Float32 pipeline, ICC transforms,
managed display integration, sRGB/P3/Adobe RGB/ProPhoto and source-preserving
PNG/JPEG/TIFF routes. Keep profile, depth and processing domain independent.
Finish the agreed SDR journeys: New/Open/Place, live photo corrections, histogram
and clipping inspection, point/average sampling, numeric color entry and swatches,
Assign/Convert, precision changes, export presets and Preferences → Color.

**Success criteria:**

- An ordinary JPEG opens as 8-bit; a supported 16-bit photo preserves its depth
  and profile. An 8-bit P3 painting and a 16-bit ProPhoto photograph complete
  create/open → edit → native save/reopen → profiled export without forced sRGB
  clipping or hidden FP16 narrowing. Saving creates a separate editable master,
  rather than overwriting an opened source photo with a project.
- Both depths meet declared editing tolerances through all supported brushes,
  effects, resampling, low-alpha accumulation and long adjustment chains.
  Identity integer16 import/export preserves samples exactly; depth changes do
  not change blend domains or reconstruct strokes.
- Exposure, white balance/tint or neutral correction, Levels/Curves, hue/saturation
  and color balance remain revisable with masks after reopening. Sliders evaluate
  from retained state. Histograms, clipping and point/average samples describe
  the declared data and exclude checkerboards/proof/display overlays.
- Canvas, picker, swatches and previews agree through managed viewing, including
  monitor changes and supported SDR display fallbacks. Assignment preserves
  declared RGB numbers; conversion previews the complete result. Undo/cancel
  restores exact preconversion state, and source repair preserves baked edits.
- PNG/JPEG/TIFF and the scoped RGB/gray/CMYK interchange matrix pass matching
  sample/profile/metadata checks and external-editor handoff. Place/Paste retain
  richer sources appropriately; export transforms a copy. Missing/conflicting
  tags, intentional reductions and unsupported inputs follow the agreed policy.
- Named 24/45/60 MP, sparse-art and multiple-document cases fit their declared
  peak/steady memory and latency budgets. Validate warm/cold tiles, zoom-out,
  transforms, blur/global effects, slider/histogram activity and concurrent
  save/export. Publish measured limits for unsupported device/workload pairs.
- Full integer16/FP16 comparisons distinguish equal-processing format costs from
  the costs of their different precision contracts. Fix budget breaches before
  enabling the mode; do not pass by lowering fidelity. All supported hosts pass
  the core SDR user journeys and relevant integration checks.

## 3 — Print proofing

**Outcome:** users can preview a printer/paper workflow and deliver the file the
lab requests, with simulation kept separate from artwork and delivery settings.

**Work:** add proof-profile management, intent/BPC, supported paper/black-ink
simulation, gamut warnings and proof controls. Keep proof and delivery recipes
separate and reuse the shared color transforms and profiled export path.

**Validation without a printer:** printer ownership or an installed printer
driver is not a prerequisite. Use representative lab/standard ICC profiles and
test images to validate the software transforms, view behavior and delivery.
Soft proofing is an on-screen simulation using an output profile, which can come
from a printing service. [Krita's proofing workflow](https://docs.krita.org/en/user_manual/soft_proofing.html).

Comparison with an actual print is a separate, optional validation exercise for
this milestone, using a lab print or external tester if available. It does not
block software completion. Record whether it has been performed; numerical or
on-screen agreement must not be described as a verified physical print match.
Accurate visual comparison also depends on the display and viewing conditions.

**Success criteria:**

- Both SDR depths and supported working spaces match the reference-CMM proof
  cases. Check intent/BPC, paper simulation and output-gamut warnings numerically
  and on an appropriate display setup.
- A proof-only lab profile can be selected without implicitly converting or
  embedding it in delivery. Export follows its explicit RGB or scoped CMYK recipe.
- Toggling proof, warnings or display settings leaves raster samples unchanged.
  Simulation overlays never enter exported artwork; proof settings persist with
  the specified document/view ownership.
- Drawing under proof and switching proof settings meet the existing interaction
  and memory gates. Transforms/LUTs are bounded and reused; profile changes do not
  recompile per-stroke shaders or create unnecessary full-resolution copies.
- The complete setup → compare → export journey works through the shared actions
  and supported host UI, including keyboard/touch access and save/reopen, on a
  machine with no installed printers. Importing a proof profile does not require
  printer discovery or driver integration.

## 4 — HDR editing and delivery

**Outcome:** users can edit HDR artwork, view it meaningfully on SDR displays and
export deliberate HDR and SDR versions.

**Work:** finalize reference-white/range semantics, source normalization and
mapping; enable linear half-float storage with shared Float32 processing and
selective higher-precision accumulation. Extend editing controls and inspection
for HDR, persist an intentional SDR rendition, integrate host display headroom
and complete a supported HDR/gain-map interchange route.

**Success criteria:**

- Supported above-one/negative values and alpha behavior survive enabled editing
  operations, save/reopen and undo within the HDR precision contract. Unsupported
  inputs/operations fail explicitly rather than silently clipping the artwork.
- Exposure, curves, histogram/sampling and color entry represent HDR ranges.
  Editing/viewing on an SDR display retains the HDR data. Display headroom and
  monitor moves change presentation, not the document's permanent interpretation.
- At least one tested HDR input/edit/export route and deliberate SDR delivery
  interoperate with another application. Where gain maps are used, edits produce
  a new map from the authored renditions with matching luminance metadata.
- SDR rendition settings survive reopening; mapped print proofing and SDR export
  agree with their specified transforms. Saved source data is not mistaken for
  a reusable gain map after arbitrary editing.
- Complete FP16 workloads meet the declared peak-memory, interaction and sustained
  thermal budgets on qualified native/browser devices. Verify display changes,
  tone mapping, long effect chains and concurrent saving/export, using actual
  application measurements rather than kernel-only predictions.
- Each exposed HDR combination has a complete user journey and published host/
  format/display limits. No unselected prototype, obsolete shader or compatibility
  path remains after integration.

RAW development, layered PSD interchange, native CMYK layers, OCIO/ACES and full
Float32 documents remain separately scoped follow-on work. No current milestone
is marked complete merely because its UI exists or its offscreen render succeeds.

## Phase 4 GTK review evidence — 2026-09-17

[GTK HDR validation](color-management-gtk-m4-validation.md) records the implemented
half-float/PQ/SDR journey, numerical/native/browser regression tests and measured
workstation budgets. GTK mapped-SDR operation is qualified within that envelope.
Physical HDR display/monitor moves, constrained/mobile devices and other hosts'
HDR integration remain outstanding; this is not a cross-platform phase-4 signoff.
User feature feedback precedes any push to origin/master.

## Phase 4 GTK follow-up qualification — 2026-09-19

[GTK follow-up report](../development/color-management-m4-gtk-qualification.md)
records the normal-package startup fix, unchanged-tolerance SDR hue correction,
sustained local-tone presentation measurements, full 60 MP gain-map delivery,
concurrent process/driver/staging memory, guide limits and animation assessment.
The feature branch and runnable build are for review before merging. Existing
JPEG numerical failures and missing physical-input/mixed-monitor evidence remain
explicit; neither this entry nor software texture tests close whole phase 4.
Float32 document storage remains outside scope.


## Phase 4 Web/Android integration evidence — 2026-09-19

[Web/Android report](../development/color-management-web-android-m4.md) records
shared HDR editing, native persistence/recovery, GTK picker/dial controls, print
proofing and EXR/PQ/SDR delivery in real desktop/tablet Chrome and native Android.
Web has an explicit 12 MP HDR admission limit after larger documents exceeded
memory budgets. Native Float16 measurements reach 60 MP. Physical HDR surfaces,
gain-map delivery, full presentation/thermal qualification and Proof workspace
projection remain explicit follow-up work; this entry is not global signoff.
