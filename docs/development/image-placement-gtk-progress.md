# GTK image placement implementation progress

Implementation of [the revised proposal](../ui/image-open-import-proposal.md),
starting from `c6e616f5`. Latest upstream sync: **`bef744d3`, 2026-09-16**.

## Current goal closeout

Publication: implementation commit `8de027b7` and upstream integration `522db8dc`
are on `origin/main`. The integration resolves the watercolor compositor overlap
with layer-local target coordinates, preserving upstream pass/copy batching.
Post-merge validation passes 19 placement-related GPU checks, 13 watercolor
checks, one selected wet-brush check and `cargo check --locked --offline -p layer-linux`.
The hardware benchmark stays explicitly ignored in that correctness run.
See the [Web/Android handoff](image-placement-web-android-handoff.md).

**Complete and user-approved.** The user explicitly replied “Approved” to the
review request covering the delivered GTK Open/Import/Drop workflow and
large-photo responsiveness. The approved package still matches the verified
production sources and binary hash recorded below. Wider follow-ups remain
separate; this approval does not publish or commit the working tree.

### Clipped 61 MP photo: focused follow-up

The user reported ~100 ms GPU spikes during both translation and drawing when
the photo is slightly larger than the canvas, also at 2× size. This was the
final closeout issue; the previous fitted-photo measurements missed it.

The renderer reproduction uses the actual 9504×6336 JPEG on a 2000×1500 canvas.
At ~1.19× fit, the required preview changes from quarter-size to half-size.
The half-size Float32 preview needs about 231 MiB, exceeding a separate fixed
128 MiB cap. The compositor evicted the preview and repeatedly fetched source
tiles for every changed frame, affecting transforms and ordinary painting.

The fix removes that extra cap and uses the existing admission based on measured
GPU headroom. Required previews still share one allowance, spare detail yields
to other visible photos, and allocations remain bounded by admission. Precision,
original source samples, editable paint and exact capture are unchanged. The
61 MP preview uses about 172 MiB more than the previous quarter-size preview.
Low-memory devices can still fall back when the admitted allowance is exhausted;
this change does not introduce tiled preview paging.

| Photo width relative to fit | Before median / p95 | After median / p95 |
| --- | ---: | ---: |
| 1.0× | 1.86 / 2.21 ms | 2.44 / 2.78 ms |
| 1.1× | 2.20 / 2.50 ms | 2.38 / 2.75 ms |
| 1.2× | 108.16 / 114.36 ms | 2.34 / 2.65 ms |
| 2.0× | 51.05 / 54.42 ms | 2.26 / 2.74 ms |

These are isolated release renderer submit-plus-GPU-completion timings, with
20 warm translated/rotated frames. They are not GTK presentation or physical
pointer latency. After the fix all cases perform only the initial 950 source
tile fetches; motion reuses the prepared preview. A new regression confirms
admitted previews above 128 MiB survive the scale boundary without new fetches,
and reduce their detail when the allowance shrinks. Existing paint, prediction,
Cancel, Undo/Redo and multi-photo memory-priority checks pass.

Four native GTK journeys now pass on the fixed build, with 3200×2000 / 2× /
120 Hz output and 8 ms native pointer delivery:

| Case | Translation GPU p95 / max | Paint GPU p95 / max | Pose → presentation p95 |
| --- | ---: | ---: | ---: |
| 1.1× fit | 3.62 / 7.15 ms | 4.37 / 7.41 ms | 8.22 ms |
| 1.2× fit | 4.27 / 7.66 ms | 4.34 / 7.64 ms | 8.19 ms |
| 2× fit | 3.55 / 7.06 ms | 4.44 / 6.90 ms | 8.22 ms |
| 1.2× with a 24 MP background | 5.99 / 12.37 ms | 6.95 / 12.66 ms | 16.47 ms |

Each case checks native translation actually changes the clipped layer's pose,
then Apply, ordinary painting, exact Undo/Redo, source retention, save/reopen,
Original Size, Import cancellation and source-sized Open. Captures confirm
canvas clipping and retained paint at Original Size. Compilers and other test
workloads were stopped during measurements; the user's existing application
session stayed open. The replacement package built successfully in 2m02s and
passes its actual relocated 61 MP JPEG launch and capture. The running user
session was preserved and needs a restart to load the fix.

Current package SHA-256:
`802f9a65cf7ed8b1ad091f111e285ea31036534ccbf17f64ef952940cc7d0183`.
Native release test SHA-256:
`85b1ec3124110a15f4d34f636f18926b5dc27ffff4473cf4936d9ae68f76232f`.
The current [approved build](image-placement-gtk-review.md) completes the goal.
Source hashes, build records and separate native /
package evidence are in the clipped-photo directory below.

Evidence: `artifacts/image-placement/gtk-closeout/clipped-photo/`.
The only production change since the preceding review package is
`scene/placement/mips.rs`; two test files add the reproductions and native cases.

Separate completed checks in `additional-large-photo/` retain their earlier
production build: two large photos (24 MP + 61 MP) pass the entire native journey,
with scale/paint queue-span p95 6.26/7.28 ms. A native-size 61 MP smudge comparison
records 21.32 ms presentation-gap p95 versus 175.67 ms while fitted. This confirms
the source-space footprint greatly affects smudge cost; it is not a historical
main-versus-branch regression comparison. Broader smudge optimization is deferred
as directed by the user.

### Preceding fitted-photo closeout

The user directed work back to completing and testing the GTK photo workflow.
The closeout is: verify the latest merge and native multiple-selection Import;
build the current package; qualify real 24/61 MP Open/Import/Drop, fit/Apply,
ordinary painting, save/reopen, Original Size and a representative multi-photo
workflow; then present the runnable result and its limitations for approval.
Further codec variants and broad brush optimization are deferred during closeout.

The latest fetch adds `996ad2c2` and `bef744d3`. The branch fast-forwarded from
`448c1ee4`; 130 changed paths were preserved byte-for-byte and five overlapping
renderer paths reconciled. The merge keeps upstream byte admission, 64 decoded
slots on all renderers, content-digest reuse and deferred final display submission,
alongside local source packing, placement/material code and telemetry. Backups,
the original patch, prepared merges and verification are in
`/tmp/capy-plan-refresh-r0zjgxlf/`. GTK compilation and the new layered-strokes
example check pass (3.00 s / 3.69 s). Targeted GPU integration and native chooser
qualification now pass. The current package and real-photo workflows are verified;
**user approval is pending**. See the [review build and walkthrough](image-placement-gtk-review.md).

- 19 targeted GPU checks pass (source/cache/display plus placement/exact export);
  two hardware performance cases are explicitly ignored. Native chooser batch
  delivery passes using Wayland keyboard input, including Cancel and one-step Undo.
- Current common-format and HEIC/AVIF Open/Import/Paste pass, as do external
  mouse/touch canvas batches, Layers destinations and failed/stale batch rollback.
- Isolated 24/61 MP journeys pass in 26.47/29.00 s, both including material strokes,
  exact source/history/save/reopen checks, Original Size and source-sized Open.
  First photo arrives in 1.17/1.65 s, full thumbnail in 1.53/2.11 s, scale queue-span
  p95 is 3.73/4.13 ms and ordinary paint p95 is 4.10/4.34 ms. Peak process memory
  is 1,885/2,537 MiB. These are scoped results, not continuous 120 Hz guarantees.
- Nine relocated package launches pass across both camera JPEGs and PNG, TIFF,
  BMP, GIF, WebP, HEIC and AVIF. Bundled codec paths match for HEIC/AVIF.
  The verifier initially assumed JPEG must also load optional codecs; that check
  was corrected without a product change. Compilation passed in 3m34s; the
  sandbox blocked the strip subprocess, and the authorized package retry passed.
- The 1024 px smudge stress case remains slow (61 MP max presentation gap 175.67 ms).
  Pan remains approximately 60 changed-camera presentations/s at 120 Hz. These
  limitations are disclosed for review; broader brush/codec work is deferred.

Current package SHA-256:
`b493d4bffabc8278b047381b8458ed4d8e1e2e70e329af162c2747a68e9cc3c3`.
Instrumented release test SHA-256:
`efe47a9f6ae549eab4daa9c24c2fa201648799f4b53469b3de681723ff370af4`.
Both use the recorded production sources on `bef744d3`; package launches and
test-executable timings retain their separate scope. Evidence and an 813-file
source manifest are in `artifacts/image-placement/gtk-closeout/`.
Earlier results below retain their original build identities.

### Earlier synchronization records

The preceding `448c1ee4` sync
fast-forwarded from `2ef4e156` without conflicts. All 132 pre-existing changed
paths were verified byte-identical immediately after the merge. The revised
proposal separates upstream features, local implementation and remaining work.
At that checkpoint the work was not ready for user approval; current status is above.

A fresh fetch during the plan refresh confirms HEAD still equals `origin/main`
at `448c1ee4`; there is nothing further to merge. All 133 current changed paths
were verified unchanged before updating the proposal and this report. The
manifest, tracked patch and verification are in `/tmp/capy-plan-refresh-kz0i39vv/`.
The latest native stress result keeps large-smudge responsiveness as the immediate
priority. Source packing reduces its observed worst gap to approximately 185 ms;
tighter nonlinear bounds bring twirl gaps to 9.33 ms median / 14.83 ms p95. Both
now pass the full native 61 MP journey, including exact history/source checks.
Details are under [material preparation optimizations](#material-preparation-optimizations).
Ordered metadata-buffer reuse also passes GPU/native checks, with similar latency;
final approval remains open.

The latest two commits add Apple form-label/layout and feature-inventory
qualification, interrupted recovery publication checks and cleanup of abandoned
private recovery files. They change no GTK photo, placement, codec or renderer
production code. Post-sync shared placement/replacement tests pass (8 tests,
0.16 s). Backup: `/tmp/capy-main-refresh-hcgm84hz/`. The main fetch alone does
not establish native performance or Apple runtime evidence.

The preceding `438f5cd3` → `2ef4e156` sync added Apple managed P3 viewing and scoped recovery/
large-photo integrity checks, following export, presets and the saved ICC library.
Shared color/preview APIs preserve their existing sRGB entry points; GTK photo
loading and placement geometry are unchanged. Of 128 existing local paths, 126
remain byte-identical. The two overlapping files (`snapshot.rs` and the UI
`lib.rs`) match independent three-way merges exactly. Pre-merge files and patches
are retained in `/tmp/capy-main-refresh-_omcvarq/`.
Post-sync `cargo test --locked --offline -p layer-ui --lib placement` passes all
eight selected checks (0.16 s). The filter also includes replacement tests.
`cargo check --locked --offline -p layer-linux` also passes after the merge
(8.02 s), covering the changed shared snapshot and UI APIs in the GTK build.
The local HEIF/AVIF implementation changes photo preparation and packaging;
its qualification is recorded separately below.
Earlier native/performance results retain their original build hashes. Passing
codec references, native GTK delivery, relocated package launches and a 61 MP
workflow at 2×/120 Hz use pre-sync builds and retain their recorded scope.
The AVIF backend change and its new evidence are recorded under
[AVIF sequences and geometry](#avif-sequences-and-geometry).

## Full-source Layers thumbnails

The confirmed oversized-photo crop is fixed in the local renderer. Original
overview integration and paint corrections now use the complete local backing
extent, including pixels beyond the canvas. A separate 32px display pass applies
the layer's rotation, reflection and aspect changes. Thumbnail framing ignores
translation and uniform zoom: Original Size can reveal more detail on the canvas
without changing which source content the thumbnail represents. Only the
disposable overview is resampled; source and paint remain unchanged.

Original overviews still build in batches of at most four source tiles on GTK,
retain weak source identities, and reuse original contributions after editing.
Geometry changes reuse the original overview. The new display uniform adds only
16 bytes to the accounted GPU thumbnail storage. Shared UI preview revisions now
include geometry and immutable source replacement, so Apply/Undo and source
profile repair refresh the host's cached image. Source revision tracking uses
weak references rather than hashing photos or retaining them in UI caches.

- The added GPU regression fails before the fix: its green source quadrant is
  shown as red because only the upper-left canvas-sized crop was integrated.
- **Six GPU thumbnail checks pass, one performance case ignored** (16.09 s).
  Coverage includes full-source quadrants, off-canvas paint and undo, independent
  placements sharing a source, quarter-turns, 45-degree rotation, reflection,
  nonuniform scale, weak ownership and no source reread during geometry changes.
  Existing Float64 area-reference, bounded-batch and failure/retry checks pass.
- **All 434 shared UI checks pass** (18.34 s), including geometry/Undo thumbnail
  invalidation and source-profile repair on all five shared host policies.
- The unchanged ABI 2 package successfully repeats the tiny 24×48 Open capture,
  including its thumbnail. The earlier blank capture alone did not establish a
  persistent pixel defect. The new native fixture waits for the current preview.
- The first two native readiness attempts exposed a fixture assumption: the
  second window owns Sketch while Paint is leased by the first, so Layers was
  hidden and no preview was requested. The timeout capture confirms this. The
  fixture now opens the actual Layers drawer and checks its retained view.
  That corrected GTK release test build passes (2m16s, on the `2ef4e156`
  baseline). The native rerun now passes all three AVIF inputs (13.12 s),
  including actual Open/Import/Paste, current visible drawer textures and the
  tiny 24×48 geometry fixture. All three captures were reviewed.

Evidence: `artifacts/image-placement/thumbnail-fix/`; successful old-package
repeat: `artifacts/image-placement/avif-sequences/package-small-repeat/`.
The original failure and intermediate native diagnostics are retained alongside
the passing GPU/shared results, rather than being overwritten.

### Native 61 MP workflow after the thumbnail correction

The complete 61 MP JPEG workflow passes (24.10 s) on the thumbnail-corrected
test executable, SHA-256
`cde04e5fcf19797f90403f19e9b7b388b28bd3340847c3913881b0235cb4a474`.
This build predates the material-brush reach fix below. It was built on
`2ef4e156`; the subsequent `448c1ee4` merge changes no GTK production code.
No compiler, photo encoder or other GPU qualification ran alongside measurement.
Conditions remain isolated Mutter 3200×2000, 2× scale, 120 Hz, 8 ms delivered
pointer events, NVIDIA RTX PRO 6000 Blackwell Max-Q / Vulkan 610.57.04, and the
recorded 9504×6336 camera JPEG `/tmp/capy-real-photos/61mp-DSC02494.JPG`.

| Measurement | Result |
| --- | ---: |
| Drop → first presented photo | 1,958.31 ms |
| Drop → current visible Layers thumbnail | 2,742.09 ms |
| Thumbnail wait after native drop-source shutdown | 427.22 ms |
| Loading GTK owner-loop gap p95 / max | 17.67 / 30.87 ms |
| Scale GPU queue span p50 / p95 / max | 3.42 / 4.99 / 7.49 ms |
| Paint GPU queue span p50 / p95 / max | 4.01 / 6.15 / 9.45 ms |
| Delivered pose → presentation p50 / p95 / max | 7.68 / 8.23 / 29.43 ms |
| Presented changed poses / delivered poses | 108 / 120 |
| Save | 329.79 ms |
| Photo Open preparation / renderer readiness | 1,050.14 / 1,432.56 ms |
| Photo Open visible-thumbnail wait | 786.27 ms |
| Whole-workflow peak process RSS | 1,963,688 KiB (1,917.66 MiB) |

Exact artwork/source checks pass through Apply, paint/Undo/Redo, save/reopen and
Original Size. Reviewed active-placement and photo-Open captures now show the
complete photo thumbnail. The Original Size capture was taken before its updated
thumbnail arrived; the fixture now waits for that revision before capturing.
The subsequent material-brush build passes that check below. Pan in this earlier
run produces 57 changed-camera
presentations with median/p95 gaps of 16.67/25.05 ms. These queue-span and delivered
pose timings retain the scope described later; this is not isolated GPU memory
or a guarantee of continuous 120 Hz interaction.

Evidence: `artifacts/image-placement/thumbnail-fix/native-61mp-jpeg-2x120/` and
`build-v3.json`; native small/sequence captures: `native-captures-v3/`.

## Material-brush reach under persistent placement

Two targeted GPU regressions confirm that the fixed 3×3 source neighborhood was
insufficient for strongly reduced photos. Both use the same two-color source at
native size and at 1/32 scale. A valid interior backtrace should pull red into
green: the reduced liquify result was transparent, while smudge silently kept
green. Both pre-fix failures are retained; this is now a confirmed correctness
defect rather than an untested concern.

The local fix gathers disjoint original/paint tiles into reusable 256×256 Float32
sample fields before applying the material operation. Each gather pass binds at
most nine source pages, and nearby samples keep the existing path. This preserves
full-resolution sampling without a full-photo scratch texture or CPU readback.
The two fields and their completed-sample descriptor use 2 MiB + 160 bytes;
transient pass descriptors and the existing source cache are separate. Source
bounds conservatively include backtrace displacement, nonlinear liquify modes
and blur. The shader computes each exact sample coordinate, and disjoint page
contributions accumulate in Float32 without requiring hardware Float32 blending.
Startup promotes the gather pipelines with their brush dependencies.

**The complete renderer library suite passes: 270 tests, 29 explicitly ignored.**
It covers the new sampling path, existing material/source/cold-page behavior,
filters, masks, native precision, save/history and recovery. The run includes
four liquify and eight smudge placement/blur settings, single/private prediction,
cancel/commit/undo/redo and unchanged original source digests. Seven supported
liquify modes preserve an opaque interior at 1/32 scale. The first all-mode fixture
also tried Reconstruct, which the renderer already explicitly rejects for every
layer; the corrected fixture keeps that existing capability boundary.
The subsequent independent source-page identity test also passes (4.36 s), with
each source tile assigned a distinct color and output checked against its own
coordinate oracle. This test includes the later counter additions; the complete
suite preceded those instrumentation additions.

The GTK release test build passes (2m30s), including per-frame gather counters
and optional native large-smudge/twirl workloads. Evidence is under
`artifacts/image-placement/material-reach/`; the renderer run was a correctness
check, with the GTK CPU build overlapping part of it, not a performance run.

### Current ordinary 61 MP GTK workflow

The complete native JPEG journey passes in **29.62 s** on `448c1ee4` plus the
local thumbnail and material fixes. Executable SHA-256:
`f10bf34b9e6c79a38ff901af4bccef16d6c8d9bf936c907b05e768cf79411bff`.
Conditions match the earlier isolated 3200×2000 / 2× / 120 Hz NVIDIA run, with
8 ms native pointer events and the same 9504×6336 camera JPEG. No compiler,
encoder or other GPU test ran during measurement.

| Measurement | Result |
| --- | ---: |
| Drop → first presented photo | 1,948.29 ms |
| Drop → current visible Layers thumbnail | 2,817.47 ms |
| Loading owner-loop gap p50 / p95 / max | 5.28 / 16.10 / 32.75 ms |
| Scale GPU queue span p50 / p95 / max | 3.12 / 5.19 / 7.77 ms |
| Paint GPU queue span p50 / p95 / max | 3.63 / 5.59 / 9.20 ms |
| Delivered pose → presentation p50 / p95 / max | 7.76 / 8.31 / 22.01 ms |
| Presented changed poses / delivered poses | 108 / 120 |
| Save | 329.91 ms |
| Original Size current-thumbnail wait | 496.71 ms |
| Photo Open preparation / renderer readiness | 1,076.28 / 1,385.91 ms |
| Photo Open current-thumbnail wait | 832.81 ms |
| Whole-workflow peak process RSS | 1,790,268 KiB (1,748.31 MiB) |

Exact source/artwork checks pass through Apply, paint/Undo/Redo, save/reopen,
Original Size, menu Import cancellation and source-sized photo Open. All three
captures were reviewed: active placement, Original Size and photo Open show the
complete current thumbnail. Pan records 58 distinct camera presentations with
16.68 / 24.99 / 25.02 ms p50/p95/max gaps; the roughly 60 Hz camera cadence
remains. Queue span includes submission waiting and delivered-pose latency is
not physical-input latency. This is not isolated GPU allocation measurement.

Evidence: `artifacts/image-placement/material-reach/native-61mp-jpeg-2x120/`;
binary/source hashes: `gtk-build.json` in its parent directory.

### Large-material stress: performance work remains

The first native stress attempt used a 512 px smudge brush and failed the
assertion requiring distant-source gathering: it stayed on the nearby path.
That is insufficient coverage, rather than evidence for the new path's speed.
The test-only revision increases smudge to 1024 px and writes each material's
measurements before asserting coverage. Its release build passes in 2m14s;
executable SHA-256:
`050cc37992b8661d21906087459212ce3610e5e85dc705b63d1c69dbbeb53f4b`.
The production sampling code is unchanged from the passing ordinary journey.

The isolated rerun exercises **1,680 gather jobs / 3,360 passes** during smudge.
The two persistent sample fields and descriptor account for 2,097,312 bytes;
this excludes transient descriptors, the source cache and driver allocations.
The measured stroke interval is 1,467.76 ms, including a 300 ms feedback drain,
with only six presentations:

| 1024 px smudge measurement | p50 | p95 / max |
| --- | ---: | ---: |
| CPU frame total | 245.46 ms | 372.66 ms |
| GPU queue span | 245.35 ms | 373.40 ms |
| Presentation gap | 189.96 ms | 373.51 ms |

Smudge changes the raster while preserving the original source and placement;
Undo/Redo/Undo restores the expected exact raster/root. The captured result was
reviewed. **This large-brush workload is not sufficiently responsive.** The
similar CPU/queue spans justify profiling preparation and submission costs;
they do not isolate which operation dominates or measure pure GPU execution.
Inspect repeated per-tile preparation/bindings, conservative page bounds and
prediction work, retaining exact source sampling and bounded storage.

The following 240 px liquify-twirl attempt records one presentation and zero
gather jobs, then fails its coverage assertion. It does not qualify native
liquify correctness or speed. Verify selected-tool activation and actual stroke
delivery before interpreting it. The full optional stress journey therefore
fails (18.86 s); its post-liquify navigation/save stages did not run. The native
harness subsequently exits 134 under its fatal-criticals setting after reporting
the Rust assertion; that exit is not a separate production crash finding.

Evidence: `artifacts/image-placement/material-reach/native-61mp-materials-v2-2x120/`
contains the launch log, smudge capture and both per-material JSON records;
`gtk-build-v2.json` records the binary/source hashes. The first attempt remains
in `native-61mp-materials-2x120/`. Final packaging and user approval remain open.

### Material preparation optimizations

Further diagnostics distinguish elapsed frame time from actual thread CPU time
and record renderer preparation, persistent painting, capture encoding, prediction,
composition and submission phases. They are enabled by existing telemetry; native
test-only tracing also records selected tools, canvas hit targets and pen routes.
The initial profiler records **350.06 ms thread CPU within a 382.05 ms frame**.
Prediction preparation repeatedly uploads hundreds of source tiles. All native
Down/Move/Up events reach the paint queue for both material tools.

The earlier twirl coverage assertions ran before the slow stroke drained. The
native fixture now waits for pending host/engine input, the active stroke,
document edits, render work and frame scheduling to finish, then drains 300 ms
of presentation feedback. It records the extra stroke-completion wait separately.
Earlier measured windows remain historical evidence; they do not establish that
twirl input failed to reach the tool or give complete-stroke totals.

#### Fixed-width source packing

The source uploader previously expanded RGB/Gray channels through runtime-length
copies inside each pixel. Matching channel count/depth outside the pixel loop
allows fixed-size row kernels for U8/U16 RGB, Gray and GrayAlpha. RGBA direct copy,
ICC/CMYK conversion, source samples and GPU transfer remain unchanged. There is
no additional image allocation.

An isolated optimized CPU comparison processes 65.536 MP per case and asserts
byte equality before timing. Original/new milliseconds:

| Samples | Original | Fixed-width |
| --- | ---: | ---: |
| RGB8 | 371.46 | 21.82 |
| Gray8 | 369.64 | 10.89 |
| GrayAlpha8 | 370.97 | 35.41 |
| RGB16 | 320.36 | 22.97 |
| Gray16 | 305.51 | 32.69 |
| GrayAlpha16 | 301.58 | 65.19 |

This measures expansion kernels, not full photo loading. The existing exhaustive
GPU source check passes (9.68 s): all integer codes, supported RGB spaces,
U8/U16 Gray/GrayAlpha/RGB/RGBA, built-in/generated ICC interpretation, exact alpha
and extended linear values against independent decoder/CMM references.

The packing-only GTK executable, SHA-256
`1a6c2e769aaa9672ca0d1c4a1e2241fbde6607c2bf83eff385ae763ad82e6894`,
passes the complete optional native journey in 31.45 s. Both material strokes
change raster content and preserve source/placement and exact Undo/Redo/Undo.
Observed-window worst CPU spans are 179.24 ms for smudge and 594.09 ms for twirl.
This fixture still collected statistics before all slow work drained; subsequent
artwork/history checks pumped the remaining work and passed. Do not treat those
windows as complete-stroke comparisons.

Evidence: `artifacts/image-placement/source-packing/` contains the standalone
comparison source/results, exhaustive GPU log, GTK build/source hashes and
`native-61mp-2x120/`. Earlier diagnostics are under `material-reach/` in the
`native-61mp-profile-2x120/` and `native-61mp-phases-2x120/` directories.

#### Tighter nonlinear source bounds

Twirl's former full-circle expansion fetched many pages its actual angle could
not reach. The new bound includes rectangle corners throughout the contact's
partial rotation, including cardinal extrema; pinch/expand include their actual
strength-dependent scale interval. Bilinear margins and the nearby fast path
remain. The shader's sample-coordinate calculation is unchanged.

- The CPU bound check covers interior rectangle samples at 65 intermediate
  strengths, multiple centers/rectangles and both rotation directions (0.01 s).
- **Nine placement GPU checks pass** (47.32 s), including source-page colors for
  push, both twirls, pinch and expand, affine brush footprints, seven liquify
  modes, smudge/blur/prediction/history, masks and off-canvas edits.
- The first nonlinear color oracle mistakenly treated a pending raster revision
  as empty artwork. It failed identically with old and new bounds. A fresh
  renderer passed; inspection confirms pending revisions retain preceding paint
  when resetting. The corrected test explicitly restores empty paint before each
  mode, then starts a pending stroke. Color assertions and tolerances are unchanged.
  Diagnostic failures and the final passing log are retained.

The GTK release build passes (2m41s), executable SHA-256
`d06e84ab5881b5204d7209651e2df540ab840c0c3e63d075a8c86d56df6d6db2`.
The isolated native journey with completed-stroke measurement passes in **28.99 s**.
Same 9504×6336 JPEG, NVIDIA/Vulkan, 3200×2000, 2×/120 Hz and 8 ms delivered events;
no compiler, encoder or other GPU qualification overlaps measurement.

| Complete native stroke | 1024 px smudge | 240 px twirl |
| --- | ---: | ---: |
| Gather jobs / passes | 2,940 / 5,880 | 2,104 / 4,208 |
| CPU frame p50 / p95 / max | 87.43 / 184.59 / 184.59 ms | 9.25 / 14.60 / 25.04 ms |
| GPU queue span p50 / p95 / max | 87.99 / 185.08 / 185.08 ms | 9.59 / 14.51 / 27.76 ms |
| Presentation gap p50 / p95 / max | 87.98 / 185.24 / 185.24 ms | 9.33 / 14.83 / 26.92 ms |
| Gap sample count | 12 | 98 |
| Additional stroke-completion wait | 42.11 ms | <0.01 ms |

The broader journey preserves source samples, affine placement and exact
paint/material history through save/reopen and Original Size. Drop → first photo
is 1,608.08 ms; current Layers thumbnail 2,130.86 ms; Original Size thumbnail wait
95.63 ms; save 329.47 ms. Ordinary scale/paint queue-span p95 is 4.95/4.42 ms;
delivered-pose latency p95 is 8.69 ms. Pan still produces 58 changed-camera
presentations at roughly 60 Hz. Whole-workflow peak process RSS is 2,369,944 KiB
(2,314.40 MiB), including both large material strokes. It is not an identical
workload to the earlier ordinary 1,748.31 MiB journey, nor isolated GPU allocation.
Twirl improves substantially; large smudge remains below the responsiveness target.

Evidence: `artifacts/image-placement/material-bounds/`, including GTK binary/source
hashes, CPU/GPU diagnostics and `native-61mp-2x120/` raw results/captures. Queue spans
include CPU submission waiting; these are delivered native events, not physical
pen latency. Full renderer/codec/package results retain their earlier build scope.

#### Ordered material metadata reuse

The next local change reuses one 160-byte page-coordinate uniform, updating it
through ordered staging copies immediately before each consuming pass. This
removes per-pass GPU buffer creation. It does not use `Queue::write_buffer`, which
would overwrite all coordinates before queued passes execute. The completed-field
descriptor remains separate. Persistent gather storage becomes **2 MiB + 320
bytes**; staging/source-cache/driver storage remains separately accounted.
**Nine placement GPU checks pass** (39.12 s), including the independent nonlinear
source-page oracle and prediction/history. The GTK release test build passes
(2m44s), executable SHA-256
`d415acebc8d492696de2c0e84d14b224bff96716e46b6e8e0fd67de42b2643e0`.
The matching isolated native workflow passes in **29.06 s** under the same
61 MP / 2× / 120 Hz conditions, with completed-stroke measurement:

| Complete native stroke | 1024 px smudge | 240 px twirl |
| --- | ---: | ---: |
| Gather jobs / passes | 2,520 / 5,040 | 2,076 / 4,152 |
| CPU frame p50 / p95 / max | 86.54 / 182.43 / 182.43 ms | 8.97 / 16.78 / 25.71 ms |
| GPU queue span p50 / p95 / max | 86.92 / 183.09 / 183.09 ms | 9.83 / 17.08 / 27.13 ms |
| Presentation gap p50 / p95 / max | 86.95 / 183.46 / 183.46 ms | 8.60 / 15.23 / 31.76 ms |
| Gap sample count | 13 | 96 |

Metadata storage is the expected 2,097,472 bytes. This removes one temporary
GPU buffer per gather pass, but **does not establish a material frame-time
improvement** over the preceding bounds build. Different frame scheduling also
changes preview job counts; these are complete native journeys, not equal-count
kernel benchmarks. Keep the bounded allocation improvement without claiming that
it resolves smudge responsiveness.

Drop → first photo is 1,626.29 ms; visible thumbnail 2,116.49 ms; Original Size
thumbnail wait 94.77 ms; save 329.73 ms. Scale/ordinary-paint queue-span p95 is
4.77/4.45 ms. Delivered-pose latency p95 is 13.27 ms (110 presented changed poses),
so continuous 120 Hz response is still not established. Pan records 58 changed
camera presentations. Whole-workflow peak process RSS is 2,423,320 KiB
(2,366.52 MiB), including the two material strokes.

Active placement, Original Size, photo Open, smudge and twirl captures were all
reviewed. Complete thumbnails, visible transform controls and the expected edits
are present. Source/artwork/history/save/reopen assertions pass. Evidence:
`artifacts/image-placement/material-uniform/`, with build/source hashes, GPU log
and `native-61mp-2x120/`. The final package still predates these changes.

The proposal now reflects this starting point and keeps the remaining editing,
failure, chooser/provider/device, codec-variant, aggregate memory and final-package
gates. All 39 relative links/anchors in the two documents resolve; renderer-file
formatting and `git diff --check` pass. HEAD remains `448c1ee4`, matching the
fetched main. No commits or pushes were made.

### Source admission and direct material previews

Material subphase telemetry separates bounds, source preparation, gather bindings,
gather encoding and the final/nearby material binding. It runs only with existing
renderer telemetry. The instrumentation-only build (`a3caa122d51f2546f2dadeebfd37c3ae14044a9e33bcddda74469a3cca168292`)
passes the isolated native journey in 28.94 s. Smudge's worst CPU frame is
186.80 ms. In one 69.61 ms persistent-paint phase, source preparation consumes
60.99 ms; gather bindings consume 4.00 ms and gather-pass encoding 0.63 ms.
Early preview frames additionally spend about 20–22 ms in nearby material
bindings, with 448–470 source-cache misses per frame. This supports optimizing
source preparation and redundant copies before more metadata work.
Evidence: `artifacts/image-placement/material-profile/`.

#### Upload admission and completed-field bindings

Source upload admission now measures in-flight bytes instead of counting every
tile as a 1 MiB Float32 upload. Native U8/U16 uploads use 256/512 KiB. The existing
**16 MiB worst-case staging ceiling** is preserved, with room reserved for the
largest next upload, including mixed ICC/scalar work. The decoded GPU source
cache still has 16 slots. This reduces unnecessary submit-and-wait cycles without
changing source samples or increasing that ceiling.

After a smudge/liquify sample field is complete, the final material binding needs
only the current destination page for mixing and alpha lock. Its eight neighboring
pages were redundantly prepared. They now bind empty views; the completed field
already supplies all backtraced samples. The ordinary nearby sampling path remains.

- **Ten placement GPU checks pass** (56.85 s), including new partially transparent
  alpha-lock cases through affine placements, blur, single/private prediction,
  cancel, commit and exact undo/redo.
- **Three source GPU checks pass; one hardware benchmark is ignored** (27.31 s).
  Coverage includes all integer codes/ICC conversions, native raster precision,
  weak cache ownership, discarded commands, mixed upload sizes, byte admission
  and release of charges after cancellation.
- GTK release test build: 2m37s; executable SHA-256
  `6f63f3b18d2a06a3d7345b186626ec322dfec982102cccdf50a83794d00b75ad`.
  The isolated full native journey passes in **29.05 s**. Smudge CPU p50/max is
  67.50/177.11 ms, with presentation gaps p50/max 85.18/177.06 ms. Twirl gaps are
  8.35/12.92/33.44 ms p50/p95/max. This is a modest improvement; smudge remains slow.
  Measured peak source staging is 15,990,784 bytes (15.25 MiB), below the unchanged
  ceiling. Whole-workflow process RSS reaches 2,854,624 KiB, a separate measure
  including both strokes and the rest of the journey.

Evidence: `artifacts/image-placement/source-admission/`, including build/source
hashes, GPU logs and `native-61mp-2x120/`. The input/display/isolation conditions
match the earlier 61 MP / 2× / 120 Hz runs. These are native complete-stroke
measurements; scheduler-dependent preview counts differ between runs.

#### Direct full-page single-batch previews

Single-batch smudge/liquify prediction can read committed artwork directly.
The former scene path first copied/decoded full source pages into a private fork
and then ping-ponged that copy. The direct path now writes complete local preview
pages, including unchanged pixels outside each contact, for scene composition,
placement previews and exact queries. Multi-batch prediction retains its private
fork, and other scene material paths keep their existing behavior. No reduced
display image becomes brush input or stored artwork.

**Ten placement GPU checks pass** (61.08 s), now also checking unchanged pixels
outside the contact within its tile. **Five existing prediction checks pass**
(50.18 s): cold native/source neighborhoods, private-preview transitions,
watercolor exact queries and 120 full-image material shader-reference comparisons
with maximum channel error zero. These are correctness checks, with part of the
GTK CPU build overlapping them, not performance measurements.

The GTK release test build passes (2m35s), executable SHA-256
`5d6def5423811191915f4a5b7c27e12a087911e76c1752853db8c137cf33fa3f`.
The matching isolated native journey passes in **29.00 s**, but the standalone
change regresses median smudge latency. CPU p50 rises from 67.50 to 100.26 ms;
presentation-gap p50 rises from 85.18 to 120.53 ms. The maximum gap is 169.68 ms.
The first three material preview frames spend 110–115 ms in nearby source
bindings. The 16-slot decoded cache repeatedly reloads neighboring source rows
when direct prediction reads the original photo. Correctness alone does not
justify this as a performance improvement. Evidence:
`artifacts/image-placement/material-direct-preview/`.

#### Bounded native source-cache working set

The next qualification increases the decoded source-cache capacity to **64 tiles
on native Float32 renderers**, retaining 16 on legacy renderers. Each decoded
tile is 1 MiB, so this adds at most **48 MiB** of GPU cache; it does not retain a
whole photo. This is separate from the unchanged 16 MiB source staging ceiling
and the material sample fields. The tradeoff is explicit and needs native timing
and memory evidence.

**Ten placement GPU checks pass** (59.74 s). The extended cache-ownership check
also passes (11.17 s), covering both capacities, repeated eviction, weak source/
history ownership and discarded command invalidation. GTK release test build:
2m40s; executable SHA-256
`8384ff55b2dea7d365e337a4f65082a305853e8d98c00a8d44e7dd8cca878590`.
The isolated native 61 MP journey passes in **29.00 s**, with complete material
stroke draining and exact source/placement/Undo/Redo checks. Smudge presentation
gaps are **74.52 / 172.37 / 172.37 ms p50/p95/max** (14 CPU frames, 13 gaps).
Twirl gaps are **8.33 / 8.75 / 32.83 ms** (119 CPU frames, 116 gaps). Gather work
is 2,940 jobs / 5,880 passes for smudge and 2,502 / 5,004 for twirl. Smudge still
has visible stalls; this is an improvement, not completed brush qualification.

Drop reaches its first presented photo in 1,647.37 ms and visible full-photo
thumbnail in 2,095.26 ms. Scale/ordinary-paint queue-span p95 is 3.88/4.51 ms;
delivered transform pose-to-presentation p95 is 8.31 ms. Save takes 330.79 ms;
Original Size's updated thumbnail arrives in 107.11 ms. Whole-workflow peak
process RSS is 2,663,248 KiB (2,600.83 MiB). Source staging peaks at 15.25 MiB,
below its unchanged 16 MiB ceiling. This includes both material strokes and is
not an isolated GPU-memory measurement. The reviewed placement, Original Size
and photo-Open captures show complete thumbnails.

Evidence: `artifacts/image-placement/native-source-cache/`. These measurements
use `448c1ee4` plus local work and **predate the `bef744d3` merge**. Upstream now
supplies the 64-slot cache for legacy as well as native renderers, byte admission,
native content reuse and the final-display-wait correction.

## AVIF sequences and geometry

AVIF uses pinned libavif **1.4.2** plus dav1d **1.5.3**, through bridge ABI **2**.
HEIC keeps libheif/libde265. This replaces AVIF's libheif route rather than
adding a second source store. Actual tracks supply sequence frame zero even
when an independent primary poster exists. The layer name discloses first-frame
import. Native source planes are gathered into tiles with clean aperture,
counterclockwise rotation and mirroring applied once; no second full transformed
RGBA allocation is needed. ICC bytes, supported NCLX interpretation, precision,
alpha and density continue through shared source policy.

libavif's parser reads through a cancellable borrowed input. The bridge clears
borrowed callbacks on success and error and checks cancellation around native
decode/conversion. This does not yet prove prompt cancellation inside a large
dav1d frame. libavif skips track-header matrices and display scaling, so the
bridge rejects those cases explicitly until their mapping is qualified. HEIF
sequences remain open. Older AVIFs lacking `pixi` remain readable while AV1
precision, strict clean-aperture and alpha-geometry validation remain enabled.
The pixel admission ceiling is also clamped to libavif's supported maximum;
larger configured budgets must not cause tiny files to fail parsing.

Current checks:

- **13 focused codec checks pass** (0.54 s). The new cases cover actual first
  frames and a deliberately different poster, transparent/profiled animation,
  separate color/alpha grids, all 16 crop/quarter-turn/mirror combinations at
  10/12-bit, asymmetric DPI, explicit unsupported track transforms, cancellation
  during native parsing and after parse, and successful retry. Earlier still,
  ICC/NCLX, hidden-RGB, HDR and admission checks continue to pass.
- Independent AOM/libavif 1.3.0 decoding matches sequence/grid samples exactly.
  The crop/mirror cases compare against known source samples using a separate
  forward mapping. `tools/validation/avif_reference.py` pins external samples,
  creates the differing-poster fixture and records generated hashes. The initial
  non-essential-transform sample is an upstream invalid-input fixture and is
  tested as rejection, not as a valid crop reference.
- **72 ordinary color checks pass, 16 explicitly ignored** (22.59 s). Ten ignored
  HEIF/AVIF checks are exercised in the focused run above; the other six retain
  their earlier independent-reference/fixture scope. Counts overlap with focused
  checks and must not be added together.
- The native bundle build and packaging verifier pass with the added libavif
  source archive/license. The GTK release test build passes (3m40s), SHA-256
  `237a39a3fa098c95dae3a581bb3ee500dbf4b24a4b0911a42a75b7316f866869`.
- **Native GTK sequence/crop Open/Import/Paste passes** (7.20 s, three inputs),
  including first-frame naming, exact retained source, Apply/save/reopen/Undo,
  Paste/Cancel and source-sized Open with a separate master destination. This
  uses the actual GTK chooser/clipboard and Vulkan renderer on isolated Wayland.
  Log: `artifacts/image-placement/gtk-avif-sequence.log`.
- **Native HEIC/P3/ICC Open/Import/Paste also passes again** with ABI 2
  (7.34 s, three inputs), preserving the earlier still-image behavior.
  Log: `artifacts/image-placement/gtk-heif-avif-abi2.log`.

Codec/reference logs, both manifests and build/input hashes are retained in
`artifacts/image-placement/avif-sequences/`. The matching ABI 2 package has now
passed the relocated launches below. Native memory pressure, remaining encoded
delivery and HEIF/AVIF variant coverage still need qualification before final
approval; real large-frame cancellation measurements follow below.

### Matching ABI 2 package

The full GTK package builds successfully (2m56s) and opens five files after
relocation outside the checkout: HEIC, ICC-tagged AVIF, the different-poster AVIF
sequence, a 12-bit crop/rotation/mirror fixture and the 61 MP photo AVIF. Each
process loads exactly the five bundled bridge/libheif/libde265/libavif/dav1d
libraries, with no codec-path or library-path environment override. All five
exit successfully after capturing their real GTK canvas. This qualifies ABI 2
package discovery separately from earlier ABI 1 results.

Packaged executable SHA-256:
`ab3bf3f7d9f0ce07823635211d49d5c06fc8f5c1247be05028872945d0681147`.
The package was built on the `438f5cd3` baseline before the latest sync; it is
not a rebuilt `2ef4e156` binary. The report includes each source hash, actual
loaded library paths and sampled process high-water memory. Four seconds are
deliberately allowed for UI capture, so total process time is not decode latency.
Logs, report and captures: `artifacts/image-placement/avif-sequences/package/`;
build log: `artifacts/image-placement/avif-sequences/package-build.log`.

The reviewed large-photo Open capture has a complete Layers thumbnail by capture
time. The first small 24×48 geometry capture showed a blank thumbnail despite its
correct canvas and Navigator. A later repeat with the same package shows the
thumbnail; the original capture alone does not establish a persistent pixel
defect. Current readiness-based qualification is tracked under
[Full-source Layers thumbnails](#full-source-layers-thumbnails).
The sequence's long layer name is truncated in the capture; its
first-frame disclosure and exact samples are established by the native/reference
tests, not by reading that truncated label.

### 61 MP AVIF through the native GTK workflow

The actual photo journey passes with a **9504×6336 AVIF** on the ABI 2 test
executable above (**23.99 s**). It covers external file Drop, active placement,
Apply, paint/Undo/Redo, pan, Save/reopen with exact artwork/source comparison,
Original Size, menu Import/Cancel and source-sized Open. Reviewed captures show
the photo and active placement controls. Encoding and compilation were finished
before this run.

This input was encoded with FFmpeg/libaom from the existing 61 MP camera JPEG;
it is a real photo encoded as AVIF, not a camera-native AVIF capture. It is
8-bit 4:2:0, 991,145 encoded bytes, SHA-256
`63cce7e695dfac4bc1483766d886fe12224bca9218936a656d87748845c41cff`.
The output container has unspecified primaries/transfer (NCLX 2/2), so shared
untagged policy assumes sRGB. This workload does not establish large tagged/HDR
or high-depth decode performance. Encoding command, original-photo hash and
stream details are in `artifacts/image-placement/avif-sequences/build-and-photo.json`.

Conditions: private Mutter **3200×2000, scale 2, 120 Hz**, 8 ms delivered pointer
events, NVIDIA RTX PRO 6000 Blackwell Max-Q, Vulkan driver 610.57.04.

| Measurement | AVIF result |
| --- | ---: |
| Drop → first presented photo | 2,505.44 ms |
| Loading GTK owner-loop gap p95 / max | 6.94 / 30.31 ms |
| Scale GPU queue span p50 / p95 / max | 3.04 / 3.70 / 7.94 ms |
| Paint GPU queue span p50 / p95 / max | 3.89 / 4.80 / 6.99 ms |
| GTK delivery → presented pose p50 / p95 / max | 7.79 / 8.32 / 16.48 ms |
| Presented changed poses / input poses | 106 / 120 |
| Native Save | 309.25 ms |
| Photo Open preparation / renderer ready after adoption | 1,913.25 / 1,401.25 ms |
| Reopened master renderer ready | 1,302.92 ms |
| Whole-workflow peak process RSS | 1,859,368 KiB (1,816 MiB) |
| Retained compressed source | 54,774,168 bytes |

The native `gpu_ms` instrumentation writes a start timestamp in a separate
submission before CPU composition and an end timestamp in the drawing encoder.
These are **GPU queue spans, including possible idle gaps waiting for CPU
submissions**, not isolated GPU execution time. This definition also applies to
earlier native tables in this report. Presentation latency is measured separately
and includes only delivered poses that were presented, not physical input.
The existing roughly 60 Hz pan cadence remains: 57 distinct camera presentations,
median/p95 camera gaps 16.67/25.03 ms. Whole-process RSS includes retained sources,
archives, renderer resources and multiple windows; it is not a codec-only or
isolated GPU memory measurement.

Raw records, screenshots and master: `artifacts/image-placement/native-61mp-avif-2x120/`.

**Defect observed in this recorded build:** the placed photo's Layers thumbnail shows a
source crop instead of the fitted artwork. `source_thumbnails.rs` still integrates
in `document_extent` without placement geometry. Fix source-local/placed thumbnail
composition and qualify its background cost. The canvas/artwork/source checks
above pass, but they do not cover this thumbnail defect.
Existing non-photo thumbnails in `thumbnails.rs` frame alpha/content bounds
independently of layer position; the correction should retain that convention.
The small packaged fixture's missing thumbnail is a separate observation.
The subsequent fix, successful small-image repeat and current native fixture
qualification are recorded under [full-source thumbnails](#full-source-layers-thumbnails).

### Isolated AVIF load and cancellation

The release `photo_sources` runner now supports `cancel INPUT DELAY_MS` to
measure cancellation acknowledgement during a real load. It stops its timer
when a load finishes early and reports cancellation only when the shared reader
returns the cancellation error. This is a file/codec/source-packing workload;
it does not exercise GTK or identify the exact internal codec phase.

On the 61 MP AVIF above, with no compiler or GPU test active, source preparation
takes **1,941.64 ms**, peaks at **424,016 KiB (414 MiB)** process RSS, and leaves
64,348 KiB resident with the retained source. The source is RGBA/U8, 52.24 MiB
compressed, untagged/sRGB-assumed and has no density metadata.

| Requested after start | Cancellation acknowledgement | Peak process RSS |
| --- | ---: | ---: |
| 25.23 ms | 72.11 ms later | 126,100 KiB |
| 250.19 ms | 405.58 ms later | 356,496 KiB |
| 1,000.21 ms | 28.18 ms later | 377,932 KiB |

All cancelled loads return no source, with final RSS between 8,736 and 15,172 KiB.
These observations establish useful cancellation on this real 8-bit input;
they do not prove an allocation bound or latency ceiling for every AV1/HEVC
bitstream. Native codec calls can continue until their next cancellation check.
High-depth photos, HEIC and aggregate pressure remain to be measured.

Exact commands, runner/bundle hashes and raw output:
`artifacts/image-placement/avif-sequences/large-avif-decode-cancel.json`.

## Implemented so far

- GTK application file launches now support cold and warm local-file delivery,
  including CLI arguments forwarded by another process. One ordered Open reader
  uses the same source/profile preparation as the menu. A temporary progress
  parent holds the incoming file before any canvas exists; successful preparation
  creates its source-sized document directly, without an extra blank drawing.
  Cancel and parent close retire the pending list after reader acknowledgement.
  The desktop entry passes `%F`, declares decoded formats and stages a `.capy`
  MIME definition. No user file associations are changed by this work.
- `LayerProperties::placement` stores a finite invertible affine; shared target
  transforms compose parent offsets and linked/unlinked mask placement.
- Source-local editable bounds can exceed the canvas. Core archive validation
  and color-edit validation use those bounds. Projects needing placement or
  off-canvas raster backing write version 5; ordinary projects keep version 4.
  Both versions are readable. Older readers reject version 5.
- The scene compositor samples retained source and existing local paint using
  the bounded affine sampler. Placement itself creates no raster backing.
- GTK Import/Paste enter a provisional shared placement transaction, fit/center
  oversized sources, and expose Apply, Cancel and Original Size. Apply commits
  insertion and geometry together. Cancel creates no artwork history. Save,
  export, recovery and close require finishing the pending operation.
- Whole-photo Scale/Rotate uses persistent geometry when no pixel selection is
  active. Existing pixel transforms keep their raster-operation path.
- GTK canvas receives GDK file lists, captures document identity and coordinates,
  and feeds the existing file request. Project files use Open. A copy indicator
  labels image placement. Layers rows now receive file lists too, with shared
  above/below/into destination validation for groups, locks and clipping bases.
  Virtual row identity is resolved on each event; unbinding clears the indicator.
  Existing internal row/tile pickup conventions are unchanged.
- GTK Import's chooser now accepts multiple files. One worker prepares the batch
  against an aggregate source-memory allowance before publishing any layer.
  Cancellation or a failed file discards the whole preparation; mixed project/
  image drops report an unsupported-batch error. Sources fit independently,
  preserve provider order and share active transform handles. Apply inserts the
  batch in one undo step; Cancel restores the prior layer selection. Batch
  Original Size restores each member's native scale, center and rotation.
  Shared tests and native mouse/touch canvas and mouse row delivery pass at 1×/2×.
- A compact canvas placement bar keeps Original Size, Cancel and Apply visible
  even when Tool Settings is hidden in a customized workspace. Native clicks on
  Apply and Cancel are exercised by the drop test; the current capture is
  `artifacts/image-placement/native-photo-batch.png`.
- Shared photo-format metadata supplies GTK picker extensions and clipboard
  preference. Actual codecs now include JPEG/PNG/TIFF plus BMP/DIB, GIF and WebP.
  GTK Open/Import/Paste use detailed preparation and label animation imports
  `(first frame)`, keeping the disclosure within the layer-name length limit.
- BMP retains embedded ICC, calibrated V4/V5 color, density, palette and alpha.
  A virtual pixel header works around image 0.25.9's V4/V5 mask-offset bug after
  retaining the original color metadata. GIF composes the first frame at its
  logical-screen offset, respecting transparency/background and embedded ICC.
  WebP preserves ICC and normalizes EXIF orientation/density once, and handles
  animation backgrounds explicitly. Missing declared profiles are errors.
- New full-frame codecs preflight encoded length, dimensions and workspace
  admission, then pack decoded rows into the existing tiled source. WebP's
  vendored entropy-table patch bounds an allocation class that the upstream
  decoder limit omitted; provenance and reproduction are in
  [vendor notes](../../vendor/README.md#webp-entropy-table-admission).
- Preview preparation now reserves each visible photo’s required reduced image
  before spending unused display memory on finer detail. This addresses the
  first scale-up cache rebuild observed with a 24 MP JPEG; the complete native
  reruns now pass without source reloads during scale-handle motion. A retained finer preview yields spare memory to another photo.
- Live native rendering caches reduced local photo pixels within a capped
  allowance from admitted display memory. Exact artwork capture bypasses that
  cache. Cache entries hold weak source identities and do not own source/history.
  Paint, prediction, cancellation and raster-history changes update damaged
  source tiles incrementally. Admitted finer cache levels survive coarser views
  and temporary 100% placement, avoiding repeated rebuilds across LOD boundaries.
- Renderer target uniforms, page allocation, selections and restoration now use
  each target's local bounds. Targets with the same extent share uniform grids.
  The canvas still controls composition and presentation dimensions.
- Brush batches carry an affine from generated contacts to local pixels. GPU
  vertex expansion and material sampling preserve the brush's document-space
  footprint, including texture coordinates; the engine inverse-maps selections.
- Pixel-transform overlays and linked companion transforms now compose full
  geometry. Whole-photo bounds include existing raster edits beyond the source.
- Masks have independent affine geometry. Link/unlink preserves appearance,
  selection-created masks inverse-map through owner placement, and mask
  application composes coverage into the owner's local pixel grid.
- Placement validates rollback and commit before mutation, retains the pending
  operation when Apply fails, and blocks competing layer-property/effect actions.
- A rejected initial preview now clears the operation before changing the tool
  or selection. Default menu Import inserts above the complete clipped stack
  containing the active layer; explicit row destinations still reject a boundary
  that would change an existing clipping base. Focused shared tests pass.
- Core validation rejects a second placement on pixel-transform/Apply Mask
  operations, whose geometry already lives in the transform or coverage. The
  renderer would otherwise ignore that representation. Its regression test passes.
- Figure/gradient operations carry their coordinate mapping into source-local
  rendering. Editing-layer sampling and connected-region requests use full
  inverse geometry and local extents.
- Bounded snapshot preparation inverse-maps output regions into source-local
  backing. The unchanged-source shortcut now requires identity placement.
  Dedicated snapshot tests now pass for translated, reflected and skewed placement,
  off-canvas color backing and linked mask crops. PNG export also preserves
  placement when source and canvas dimensions match.

## Validation recorded so far

- Native `native_application_file_launch` **passes, 6.82 s**, on isolated
  Wayland/session bus with the real GTK/Vulkan renderer. Separate processes use
  production GApplication argument forwarding. Checks cover an initially empty
  primary, activation while loading, JPEG/project ordering and names with spaces,
  source dimensions/sample retention, saved U16 photo policy, separate photo
  master and native project destinations, warm Open preserving an edited drawing,
  its unsaved-close prompt, named read failure followed by the next valid file,
  progress Cancel, parent close, and missing-profile Cancel/Display P3 adoption.
  Exactly the requested documents remain; inputs stay byte-identical. Capture
  `artifacts/image-placement/file-launch/opened-photo.png` was reviewed.
  Log: `artifacts/image-placement/gtk-file-launch.log`.
  This qualifies application/CLI delivery, not every file-manager or remote provider.
- The existing native menu Open cancellation regression also passes after the
  preparation refactor (**4.01 s**), preserving the current document and releasing
  requests for retry. Log: `artifacts/image-placement/gtk-open-cancel-after-launch.log`.
- Desktop entry validation and generation of a temporary `.capy` MIME database
  pass. These checks do not install the application or change file associations.
- Post-merge `e7431720` shared-library run: **69 color, 80 core, 63 engine and
  434 UI passed** (646 total), five color cases ignored. Command:
  `cargo test --locked --offline -p layer-color -p layer-core -p layer-engine -p layer-ui --lib --quiet`.
  Log: `artifacts/image-placement/e743-shared.log`.
- Post-merge focused renderer checks: **six passed**, covering visible/raw point
  samples, sparse-page area samples, cold native restoration, spatial filters,
  combined U16 source/integer paint and declared color coordinates. This verifies
  the shared aligned-readback fix alongside source-local bounds. Log:
  `artifacts/image-placement/e743-renderer-sampling.log`.
- Post-merge release GTK build passes; see the current build and hash below.
  Documentation links and `git diff --check` pass. No native Apple tests ran here.
- The full renderer and four 60/120 Hz native timing runs predate `e7431720`;
  that sync leaves GTK placement/drop/input unchanged and adds the shared
  area-sample readback fix covered above. File-launch tests and a further 61 MP
  workflow then pass on `e7431720` plus local changes. The later `c8e77f66` sync
  changes Apple tests/docs only; it does not invalidate those GTK runtime results.
- Full renderer run on `d614f7b4` after the geometry changes: **252 passed, 11 failed, 29
  ignored**. Every failure reports the same cached clipping uniform buffer
  mismatch (96 allocated bytes, 128 required bytes). The allocation is fixed;
  **the full corrected rerun passed: 265 tests, zero failures, 29 ignored**
  (409.17 s), including the new exact-placement snapshot regressions. Current
  log: `artifacts/image-placement/renderer-current.log`. The earlier failing run
  is retained as diagnostic evidence. Log:
  `artifacts/image-placement/renderer-before-clipping-buffer-fix.log`.
- Native two-process `native_photo_file_drops` passes on isolated Wayland at
  **1× and 2× display scale**. A separate GTK process offers `text/uri-list`;
  Mutter delivers mouse and touch drags into the editor. Both canvas batch cases
  preserve profile/source samples, provider ordering and rotated-camera drop
  coordinates through Apply, archive round-trip and one undo. Mouse row cases
  verify above/inside/below-group indicators, insertion, group offsets and Cancel.
  The current runs also verify a malformed second file, whole-batch cancellation,
  a changed destination, explanatory forbidden feedback for mixed project/image
  batches, and single-project Open routing. Asynchronous URI delivery refreshes
  the copy indicator without requiring another pointer motion. Actual native
  clicks on the canvas bar Apply/Cancel finish the operations.
  Logs: `artifacts/image-placement/gtk-external-file-drops.log` and
  `artifacts/image-placement/gtk-external-file-drops-scale2.log` (18.28/18.64 s).
  This verifies compositor-delivered mouse/touch, not a physical pen or every
  file-manager/provider implementation.
- Focused UI checks after placement-start cleanup and clipping-stack insertion:
  **6 passed**, including the new rejected-start case. It verifies that failure
  keeps the prior tool/selection and leaves Save/recovery available.
- Fresh checks after merging `d614f7b4` and the local batch/row implementation:
  `cargo test --locked --offline -p layer-color -p layer-core -p layer-engine -p layer-ui --lib --quiet`
  passed **69 color, 80 core, 63 engine and 433 UI tests**; five color reference/
  fixture cases were ignored. The two new UI tests cover ordered atomic batch
  preparation, shared transforms, Original Size, selection rollback, one-step
  history, archive round-trip and group/lock/clipping destinations with offsets.
  `cargo check --locked --offline -p layer-linux` also passed. These are shared
  correctness and compilation checks, not native drag or performance acceptance.
- `layer-color --lib`: **69 passed, 4 external/reference cases ignored** before
  adding the ignored native-fixture writer. The current new-codec group passes
  **8 tests**, plus that ignored writer. Coverage includes ICC/calibration,
  alpha/hidden RGB, palette and row direction, EXIF/density, animation placement,
  truncation, dimensions/admission, source limits and failed/cancelled I/O.
- GTK release `native_common_raster_open_import_and_paste`: **passed, 7.15 s on the current build**,
  on isolated Wayland with the native Vulkan renderer. Each new format exercised
  native file selection, Open's prepared source-sized project, Import Apply/save/
  undo and clipboard Paste/Cancel. Source samples and profiles remained exact;
  GIF's first-frame label was visible in the resulting layer state. This is
  native file/clipboard integration evidence, not physical drag delivery or
  large-photo performance. Log: `artifacts/image-placement/gtk-common-formats-current.log`.
  Executable: `target/release/deps/layer_linux-26cdf45de1e69be4`, including
  the current batch, destination, placement-bar and clipboard registry changes.
- `layer-core --lib`: **80 passed**.
- Earlier `layer-ui --lib`: **431 passed**, including fit, Cancel, one-step Apply/undo,
  save/reopen and Original Size while retaining exact source samples.
- Hardware placement pixel test passed for attachment, Float32 and native modes:
  reductions, rotation, flips, negative placement, source tile boundaries and
  zero created paint backing. Native exact capture bypasses the display cache.
- Source-local editing test passed for attachment and native renderers: paint
  beyond canvas bounds, selection clipping, undo/redo and cold restoration.
- Native brush comparison passed for G Pen, Marker and Wet Round with nonuniform
  scale, rotation, reflection and skew (mean channel error below 2.5/255).
- Native mask test passed: scaled oversized photo, unlink/relink without a jump,
  subsequent following behavior, and Apply Mask preserving visible pixels.
- Native display-cache test passed: only affected tiles update, preview/cancel
  and undo/redo preserve the expected pixels, and LOD changes retain the cache.
- Native gradient/figure comparison passed under layer placement. The placement
  test group most recently passed **6 tests**, with the performance case ignored.
- Fresh tests after merging `9e3d2567`:
  `cargo test --locked --offline -p layer-core -p layer-engine -p layer-ui --lib --quiet`
  passed **80 core, 63 engine and 431 UI tests**.
- The earlier full renderer run recorded **260 passed, 2 failed, 29 ignored**.
  Both failures were test setup affected by the brush uniform change: the
  expected uniform size and a reference shader's include list. Both were fixed
  and their targeted reruns passed, including exact material reference matches.
  The current full rerun after the operation/uniform and snapshot changes
  passes (265 tests, zero failures, 29 ignored); see the latest result above. Earlier full-run log:
  `/tmp/image-placement-renderer-tests.log` (temporary local evidence).
- GTK release `native_profiled_place_paste_and_source_history`: **passed** on
  isolated Wayland, including Import/Paste, Apply, source precision, clear/undo,
  save/reopen and cancelled/unsupported clipboard input. Log:
  `artifacts/image-placement/gtk-import-paste.log`.
- An earlier `cargo check --locked --offline -p layer-linux` passed with the
  renderer/geometry changes. It predates the `438f5cd3` planning sync; this sync's
  checks are recorded under [the upstream change](#effect-of-the-latest-upstream-change).

The earlier profiled Import/Paste test and synthetic performance figures below
predate the latest cache/editing changes. The common-codec and native drop tests
have now been rerun with the current implementation. Their small deterministic
fixtures do not establish current-build large-photo performance qualification.

The native file test uses `tools/performance/gtk-raster.sh`, whose in-process
chooser setting is `GDK_DEBUG=no-portals`. An earlier runner attempt omitted that
setting and failed to locate its chooser. A subsequent test edit put a second
Apply at the wrong checkpoint; that was corrected before the passing run.

### Preliminary rendering measurements

Release build, NVIDIA RTX PRO 6000 Blackwell Max-Q, Vulkan driver 610.57.04.
2000×1500 destination; U16 source containing gradients, detail and noise, matching
the existing large-photo workload style. These are synthetic image sources at
realistic photo dimensions, **not physical GTK input-to-presentation measurements
or disk decoder timings**. Each motion sample includes GPU completion.

| Workload | Cold render | Motion median | Motion p95 | Motion max | Reported resident bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Initial exact Float32 path, 6000×4000 | 352.94 ms | 53.47 ms | 57.23 ms | 57.86 ms | 149,975,024 |
| Native renderer with reduced display cache, 6000×4000 | 449.22 ms | 2.21 ms | 2.59 ms | 2.83 ms | 234,117,696 |
| Native renderer with reduced display cache, 9504×6336 | 818.34 ms | 1.79 ms | 1.99 ms | 2.14 ms | 198,924,928 |

The initial and updated runs use different renderer modes, so their comparison
is diagnostic, not a controlled final acceptance comparison. The updated runs
use the production native renderer. Both updated runs decode source tiles only
during preparation (384 and 950 misses respectively, no subsequent source loads).
Fixture generation took 362 and 906 ms respectively, outside cold render timing.

Reproduce with the ignored release test `large_photo_placement_workload` and
`LAYER_PLACEMENT_SIZE=24mp` or `61mp`. `LAYER_PLACEMENT_PHOTO` can supply an actual
file through the portable decoder. GPU execution needs physical device access.

### Actual camera JPEGs: current renderer measurement

2026-09-16, upstream `d614f7b4` plus the local implementation. Current release
executable: `target/release/deps/layer_render_wgpu-f5a1119c28e66c96`.
Same NVIDIA/Vulkan hardware and 2000×1500 destination as above. Each case reads
the original JPEG from disk through the shared decoder, then measures a cold
native render and 20 placement-motion samples including GPU completion.

| Actual source | Decode/preparation | Cold render | Motion median / p95 / max | Renderer resident bytes | Peak process RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| Sony A7 III, 24 MP | 627.85 ms | 255.63 ms | 1.75 / 2.32 / 2.38 ms | 161,921,152 | 442,088 KiB |
| Sony A7R IV, 9504×6336 | 736.14 ms | 630.21 ms | 1.85 / 2.13 / 2.23 ms | 198,662,784 | 595,016 KiB |

Both tests pass: placement creates no paint backing and source-tile digests
remain unchanged. The 24 MP JPEG is stored at 6000×4000; EXIF normalization gives
the intended portrait source at 4000×6000. Source-cache miss totals are 384 and
950, matching the respective source tile counts. RSS is the whole test process,
including decoding and driver overhead; renderer residency is a separate metric.

Logs: `artifacts/image-placement/real-photo-24mp-gpu.log` and
`artifacts/image-placement/real-photo-61mp-gpu.log`. These results qualify the
renderer workload with real photos; they still do not measure GTK input-to-
presentation latency, native handle interaction, large-photo painting or Save.

Original fixtures are stored locally under `/tmp/capy-real-photos/`:

- `24mp-cameralabs-A7III.jpg`, 7,143,424 bytes, from
  [Cameralabs' original A7 III sample](https://www.flickr.com/photos/cameralabs/27430365417/sizes/o/).
  SHA-256: `866925ce6ec2fc386865bd0783eee927e48f156da3d7a11e941f6f8ecb317adc`.
- `61mp-DSC02494.JPG`, 33,947,648 bytes, from
  [LensTip's A7R IV sample photographs](https://www.lenstip.com/2323-news-Sony_FE_35_mm_f_1.8_i_A7R_IV_-_sample_shots.html).
  SHA-256: `351c6696fee3272f45a417c4274abd60c61db140cd9bdec417cab90d156e2f94`.

Their adjacent JSON manifests record the direct download URL and dimensions.
Sony's current gallery previews were checked first and are only 1280×853;
those preview downloads were not used for these measurements.

### Native real-photo workflow: first complete runs

Both `native_large_photo_placement_workflow` cases passed before the latest
preview-headroom change: **24 MP in 14.51 s; 61 MP in 16.49 s**. Actual mouse
input traverses Mutter/Wayland for Drop, scale-handle motion, Apply, painting and
pan navigation. Each case verifies one-step import undo/redo, stroke undo/redo,
source/placement retention, native chooser Save, archive restore and equal exact
artwork capture after reopening. Original Size keeps original source and paint.

The private monitor is 1600×1000 at 120 Hz; native motion is sent every 8 ms.
These are compositor-injected events, not measurements from physical hardware.
First presented placement was 947 ms / 1880 ms for 24 MP / 61 MP. The GTK 5 ms
heartbeat’s longest loading gaps were 15.6 / 16.4 ms. Frame records and screenshots
are under `artifacts/image-placement/native-24mp/` and `native-61mp/`.

This run exposed a **~200 ms first scale-up stall** on the 24 MP photo when its
initial 25% pose crossed a preview-resolution boundary. The preview preparation
fix uses spare admitted memory for finer detail during loading; its subsequent
native measurements below show no scale-up rebuild. The 61 MP paint sample also
included a preceding history restore still finishing on the render worker. The
updated driver waits for that preceding work before timing the stroke. Keep
these diagnostic results separate from final interaction acceptance.

The workflow test has since gained native Original Size button activation,
menu Import/Cancel and source-sized photo Open with a real GTK rendering window.
Those additional cases now pass in the current runs below. A first timing rerun
reached photo placement but rejected a discarded first frame; its detector now
follows retained source work into the first actually presented successor frame.

### Current native workflow after preview preparation fix

**24 MP: passed in 20.48 s. 61 MP: passed in 22.64 s.** This includes all of the
native workflow above plus actual Original Size button activation, menu
Import/Cancel, and source-sized photo Open in a new GTK rendering window.
The executable SHA-256 is
`c620532b190680e60d02529dd03548bf0b1eaa75e73f53f87c1cef00016f06d7`.
No other GPU workload or compiler ran during these measurements.

| Step | 24 MP | 61 MP |
| --- | ---: | ---: |
| Drop to first presented photo | 1,229.9 ms | 1,890.3 ms |
| Loading owner-loop gap, p95 / max | 8.9 / 16.1 ms | 8.3 / 20.7 ms |
| Scale GPU p50 / p95 / max | 2.95 / 3.46 / 7.01 ms | 3.09 / 4.66 / 7.85 ms |
| Paint GPU p50 / p95 / max | 3.52 / 5.53 / 9.18 ms | 3.74 / 5.44 / 9.98 ms |
| Pan GPU p50 / p95 / max | 0.066 / 0.113 / 0.200 ms | 0.067 / 0.110 / 0.270 ms |
| Native Save worker | 309.9 ms | 329.2 ms |
| Saved document: new renderer ready | 1,251.7 ms | 1,251.3 ms |
| Menu Import preparation/ready | 890.5 ms | 1,094.4 ms |
| Photo Open preparation | 879.7 ms | 1,041.3 ms |
| Photo Open: new renderer ready | 1,261.2 ms | 1,354.4 ms |

Scale and pan motion cause **zero additional source tile misses**. Painting
loads 33/34 additional source tiles, not the whole 384/950-tile photograph.
The original ~200 ms scale-up rebuild is absent. Observed presentation gaps
remain separate from GPU work: scale p95/max is 8.53/21.45 ms and 8.48/25.21 ms;
paint is 8.44/10.76 ms and 8.58/9.98 ms. These include cursor presentations and
are not proof that every pose or camera request reached a new 120 Hz frame.
The subsequent instrumentation below correlates submitted photo poses with
actual presentation and reports GTK owner delivery latency and coalesced poses.

RSS during scale/paint/pan is approximately 738/845/849 MiB for 24 MP and
849/933/938 MiB for 61 MP. Process peak across the full multi-window/test-archive
journey is 1,246/1,980 MiB. These process readings include driver and retained
fixture/archive objects; they are not standalone GPU allocation measurements.
GPU memory-pressure and wider combined-layer admission still need qualification.

Current evidence and screenshots are in
`artifacts/image-placement/native-24mp-headroom/` and `native-61mp-headroom/`,
including `workflow.json`, `native.log`, `active-placement.png`,
`original-size.png`, `opened-photo.png`, and the saved native master.

The two exact snapshot regressions also pass: off-canvas color/mask restoration
through repeated cropped reads (translation, skew and reflection), and PNG
export honoring placement even when canvas/source extents match. Their oracle
uses encoded Display P3 paint converted to linear RGB, matching the documented
native paint representation. Log: `artifacts/image-placement/exact-placement-snapshots.log`.

### Native pose-to-presentation and camera cadence

All four complete real-photo workflows pass: 24/61 MP at 60/120 Hz, 1600×1000,
1× scale, native mouse input every 8 ms. These runs use the default workspace
and the same RTX PRO 6000/Vulkan hardware described above. No other GPU workload
or compiler ran concurrently. They precede the `e7431720` sync; that merge changes
Apple host workflows and the shared area-sample readback, not GTK placement/input.
The test executable SHA-256 is
`3996b86888bd1ef9199ffd5c1f1f0d1f6a887dc04807cad343652b97e694f423`.

| Photo / refresh | Scale GPU p50 / p95 / max (ms) | Presented poses / 120 changed input poses | GTK delivery → presentation p50 / p95 / max (ms) |
| --- | --- | ---: | --- |
| 24 MP / 120 Hz | 3.06 / 3.36 / 7.04 | 112 | 7.77 / 8.36 / 18.65 |
| 24 MP / 60 Hz | 2.99 / 6.92 / 9.52 | 56 | 16.05 / 16.49 / 32.47 |
| 61 MP / 120 Hz | 3.10 / 5.34 / 8.89 | 109 | 7.69 / 8.29 / 18.72 |
| 61 MP / 60 Hz | 2.98 / 6.59 / 11.61 | 57 | 16.04 / 16.57 / 18.82 |

The test records the layer pose after GTK owner delivery, matches it to submitted
frames, and uses compositor presentation feedback. Only changed poses actually
presented contribute to latency; unmatched/coalesced inputs are counted separately.
GTK backlog delivery is not a physical device timestamp. These are software
delivery measurements, not pen-to-photon latency or proof of every frame meeting
its deadline. Paint GPU p95 is 5.39/5.25 ms for 24/61 MP at 120 Hz, and 5.72/6.27 ms
at 60 Hz. Source tile counters remain unchanged during scale and pan motion.

Middle-button pan exposes an existing cadence limit: at 120 Hz there are only
58 distinct camera presentations per sweep, with median gaps of 16.67 ms and
p95 gaps near 25 ms. At 60 Hz there are 57 camera presentations, median 16.67 ms
and p95 near 33.3 ms. The GPU pan p95 is below 0.14 ms. Cursor-only presentations
do not count as camera updates. This establishes the observed limit, not its
root cause or a 120 Hz camera-motion pass.

A temporary stylus-controller experiment conflicted with primary transform
capture; forwarding motion history through the existing pan drag passed the
workflow but did not improve cadence. Both production experiments were reverted.
The final `input.rs` diff contains test-only timing instrumentation; the pan
controllers match main. Broader navigation scheduling work needs a separate,
specific diagnosis rather than changing capture without a demonstrated benefit.

Raw records, native logs, screenshots and saved masters are under
`artifacts/image-placement/native-24mp-timing120/`, `native-24mp-timing60/`,
`native-61mp-timing120/` and `native-61mp-timing60/`. Each `workflow.json` includes
raw pose/frame/camera records and the measured summaries. Physical pen, 2×
large-photo motion, combined-layer memory pressure and sustained workloads remain
unqualified.

### Current 61 MP workflow after application file-launch integration

The complete 61 MP native workflow passes again on `e7431720` plus the current
placement/file-launch changes (**22.86 s**), with no compiler or other GPU test
running. It exercises actual Drop, scale handles, Apply, painting, pan, native
Save, archive reopening, Original Size, menu Import/Cancel and photo Open.
Source retention and exact saved/reopened artwork checks pass throughout.
Hardware/input settings match the 120 Hz, 1600×1000, 1×, 8 ms motion runs above.

| Measurement | Current 61 MP run |
| --- | ---: |
| Drop → first presented photo | 1,884.3 ms |
| Loading owner-loop gap p95 / max | 8.22 / 14.53 ms |
| Scale GPU p50 / p95 / max | 3.01 / 3.34 / 7.00 ms |
| Presented poses / 120 changed input poses | 110 |
| GTK delivery → presented pose p50 / p95 / max | 7.67 / 8.80 / 17.44 ms |
| Paint GPU p50 / p95 / max | 3.64 / 5.37 / 9.65 ms |
| Pan GPU p95 | 0.122 ms |
| Native Save | 330.5 ms |
| Photo Open preparation / new renderer ready | 1,035.9 / 1,381.8 ms |

Scaling and pan cause no additional source-cache misses. The existing pan limit
remains: 57 distinct camera presentations, median/p95 gaps of 16.67/25.00 ms.
This is not a blanket 120 Hz cadence pass or a physical device latency measurement.
Peak process RSS is 1,683 MiB across the test's retained photos, archives and
multiple windows; it is not an isolated GPU allocation measurement.

Raw timing, native log, captures and saved master:
`artifacts/image-placement/native-61mp-after-launch/`. The executable hash is
recorded with the current build below. The earlier 24 MP and 60 Hz results stay
scoped to their recorded builds; the latest production changes concern file
launch/preparation, not the placement sampler or input scheduler.

## Work still required by the authorized plan

1. Fix the measured large-smudge stalls and qualify native liquify delivery.
   The distant-source correction passes the full renderer suite and the current
   ordinary 61 MP native workflow; 1024 px smudge still reaches 374 ms presentation
   gaps. Preserve exact full-resolution sampling while profiling and reducing
   per-frame preparation/encoding work. Finish the editing audit and native
   pointer qualification. Geometry is now
   integrated into brushes, sampling, regions, figures/gradients and pixel
   transforms, with partial coverage. Raw sampling/region boundaries, bounded
   snapshots, remaining native inverse-scale material coverage, and watercolor
   transport/edge distances need further qualification.
2. Verify existing overrides, masks (including link toggles and application),
   clipping/groups, effects, pixel selections and explicit source operations.
   Harden provisional placement failure/recovery and competing-edit behavior.
3. Qualify native transform handles and actual file-manager drops, including
   mouse/touch/pen delivery, camera/DPI mapping, stale targets and cancellation.
   Exercise the implemented Layers-panel destinations and ordered batches through
   further native delivery and the multiple-selection chooser. Mouse/touch canvas
   and mouse row delivery, cancellation/stale/batch failure and visible Apply/Cancel
   now pass at 1×/2×; physical pen evidence and native chooser batches remain.
   Startup/file-launch routing now passes its native workflow above;
   default Import into clipping stacks and provisional-start cleanup are fixed.
   GTK has no separate document-free editor drop surface today.
4. Finish the existing local HEIF/AVIF decoder and packaging implementation:
   implement sequence policy and broaden high-depth HEIC coverage; qualify
   cancellation inside native work, large-photo memory/latency and encoded Drop.
   Independent color/alpha/orientation/gain-map references, native Open/Import/
   Paste and actual relocated-package photo launches now pass.
   See [current codec status](#heifavif-implementation-and-open-qualification).
   BMP/GIF/WebP now have
   working readers and native file/clipboard coverage. Independent lossless,
   VP8 and compressed-alpha WebP references now match libwebp exactly; broader
   real-file qualification remains.
5. Finish combined-layer memory admission, explicit GPU allocation measurements
   and sustained workloads. Exact placement snapshots/export, incremental raster
   invalidation, preview-level retention and native 24/61 MP painting now pass;
   extend coverage where concerns remain rather than repeating those checks.
6. Extend passing large-photo GTK Open/Import/drop/placement/save/reopen workflows
   to physical pen and remaining pressure/device cases. The 61 MP single-photo
   workflow now passes at 2×/120 Hz on the codec-enabled GTK build, with measured
   presentation latency and the existing pan cadence limit recorded below.
   Publish the current working build and scoped results, then obtain the user's
   approval after the remaining required work. Do not mark the implementation
   complete before that.

No commits, pushes or release publication have been requested or performed.

## Effect of the latest upstream change

Fetched `origin/main` and fast-forwarded `2ef4e156` → `448c1ee4` without
conflicts or a stash. The 16 incoming paths do not overlap any of the 132
pre-existing changed paths; all 132 were verified byte-identical immediately
afterward. HEAD equals the fetched main. Backup:
`/tmp/capy-main-refresh-hcgm84hz/`. The unrelated tablet handoff is preserved.

`6af9d193` adds the shared Apple `FormPicker`, visible UIKit labels and layout/
inventory evidence. `448c1ee4` adds process-interruption checks around actual
recovery file publication/discard and reclaims recognized abandoned private
temporary files after publication. The upstream record reports eight local
process-kill cases and complete hardware tool-routing coverage. These close
specific Apple gaps; physical iPad expiration/provider delivery, power loss and
full editor lifecycle remain separate. Apple runtime checks were not rerun here.

The shared host inventory example updates its color/gradient fixtures to typed
`RgbColor`/`GradientStop`. All other source changes are Apple-specific. No GTK
photo loading, placement, codec or renderer production code changes in this
sync. Earlier GTK timings and package checks retain their existing build scope.

The rewritten proposal reuses completed upstream features and existing local
placement, orders the remaining correctness/delivery/codec/acceptance work, and
keeps Apply as full-resolution source plus a persistent layer transform. Official
Adobe, Affinity and GIMP workflow references were rechecked. Post-sync
`cargo test --locked --offline -p layer-ui --lib placement` passes all eight
selected tests (0.16 s); the filter also selects replacement tests.
`cargo check --locked --offline -p layer-host --example inventory` also passes
(10.76 s), covering the changed inventory example and its shared dependencies.
All 73 relative links/anchors across the proposal, this report, Linux/vendor
notes and third-party notices resolve. Both revised documents have no trailing
whitespace, and `git diff --check` passes. Compared with the pre-merge backup,
only the two intended planning/progress documents changed locally in this turn.

### Previous sync: Apple managed viewing and recovery

Fetched `origin/main` again and fast-forwarded `438f5cd3` → `2ef4e156` with
autostash. HEAD then equaled fetched main. All 128 pre-existing local paths are
preserved: 126 byte-identical, and two matching independent three-way merges
exactly (`layer-render-wgpu/src/snapshot.rs`, `layer-ui/src/lib.rs`). There are
no conflicts. Backup: `/tmp/capy-main-refresh-_omcvarq/`.

`a2054398` adds managed Apple P3 SDR canvas/control viewing, display observations,
working-space adoption refresh and explicit Metal surface color tagging.
`2ef4e156` records retained SDR recovery and a 61 MP synthetic-JPEG integrity
regression on Mac Metal under both Apple policies. Shared snapshot/UI APIs add
an explicit destination space while preserving existing sRGB entry points.
GTK loading and placement geometry are unchanged. The proposal removes these
upstream foundations from remaining work while retaining physical-device,
provider and sustained-performance limits. Apple tests were not rerun here.

Post-sync checks pass: eight selected shared placement/replacement tests and
`cargo check --locked --offline -p layer-linux` (8.02 s). Documentation link/
anchor checks across the proposal, progress report, Linux and vendor notes and
third-party notices pass; `git diff --check` is clean. The ABI 2 package captures
recorded above were run after the sync using the already-built `438f5cd3` binary;
they qualify its codec bundle, not a new performance build on `2ef4e156`.

### Previous sync: Apple export and ICC library

Fetched `origin/main` and fast-forwarded `c8e77f66` → `438f5cd3` with autostash.
At that point HEAD equaled fetched main. Of 127 pre-existing modified/untracked paths,
126 stayed byte-identical. The one overlapping file, `customization.rs`, retains
its local Original Size action and adds exactly the upstream Export-tooltip
correction. Autostash reapplied cleanly; no conflict remains. Backup:
`/tmp/capy-main-refresh-asoeka1d/`. The unrelated tablet handoff is untouched.

`438f5cd3` implements Apple profiled PNG/TIFF/JPEG export through the shared
immutable snapshot worker, output previews, destination/named presets and a
saved ICC library. The [upstream acceptance record](apple-handoff.md#profiled-export-and-icc-library)
reports both-policy source/output checks and native Mac TIFF save/reopen,
ICC/preset reuse and cancellation. These results do not qualify physical iPad
delivery, sustained performance or this branch's active placement affine.
The proposal now removes export/presets/ICC-library construction from the Apple
backlog and preserves those APIs for future placed-photo host qualification.

The shared tooltip is the only non-Apple source change in this merge. No GTK
loading, placement, codec or renderer code changes upstream. This refresh does
not rebuild a performance artifact or rerun native Apple tests; earlier results
keep their original hashes and scope.

Post-sync `cargo test --locked --offline -p layer-ui --lib placement` passes
eight selected tests (0.16 s after build). Three directly cover photo placement:
fit/Apply/Cancel/Original Size with save/reopen and source equality, atomic batch
placement/history, and rejected-start cleanup. The substring also selects
replacement tests. This confirms shared behavior after the merge, without
claiming another native delivery or performance qualification.
All 53 relative links/anchors across this report and the proposal resolve, and
`git diff --check` passes.

### Previous sync: Apple correction and mask qualification

The previous fast-forward, `e7431720` → `c8e77f66`, adds Apple retained-photo
correction and local-mask qualification. It exercises all six correction types,
exact source/history save/reopen and re-editing on both Apple policies, plus a
native Mac control/file-panel workflow. Only Apple tests and documentation change.
The [acceptance record](apple-handoff.md#retained-photo-corrections-and-masks)
does not establish large-photo performance or the local placement affine; those
remain separately scoped. Corrections/masks are removed from the proposal's list
of missing Apple features.

Git reapplied the local changes without conflicts. The tracked patch and all
112 modified/untracked file digests matched their pre-merge copies before the
documentation update. Backup: `/tmp/capy-plan-main-sync-x13f3t20/`. The unrelated
tablet handoff is preserved. This planning sync did not rerun native Apple/GPU
tests or rebuild the unchanged GTK runtime; it checks the merge, documentation
links and whitespace.

### Previous sync: Apple color, source editing and sampling

The preceding fast-forward added `0290c196` (Apple document profile/bit depth),
`79a9638b` (source repair/rasterization and ICC import) and `e7431720`
(histogram/area sampling). The latter fixes an omitted aligned row pitch in the
shared raw color-sample readback. The merged file retains the local source-sized
sampling bounds alongside that fix. Apple capability exposure and shared tests
also expand to Mac/iOS; GTK file/placement behavior is unchanged by these commits.

Apple already has retained JPEG/PNG/TIFF Open/Place/Paste from `d614f7b4` and
still adopts Place/Paste with the direct-import API. Active placement and the
expanded local codec list need explicit host work. Document color, source-edit
and inspection interfaces must no longer be planned as missing foundations.
See the [revised platform boundary](../ui/image-open-import-proposal.md#platform-boundary-after-the-latest-merge)
for scoped upstream evidence and remaining Apple acceptance. No Apple runtime
or physical-device test was performed on this Linux host during this sync.

Git reapplied local changes without conflicts. All 37 checked untracked files
retained their pre-merge digests, and the unrelated tablet handoff is untouched.
The pre-merge tracked patch and untracked archive are backed up under
`/tmp/capy-before-main-sync-gpvlkr4n/`.

## Common-codec references and reproduction

BMP color/profile fields follow Microsoft's
[BITMAPV5HEADER definition](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapv5header).
WebP chunk flags, frame bounds and animation background handling follow Google's
[container specification](https://developers.google.com/speed/webp/docs/riff_container).
Pixel decoding uses the pinned Rust libraries; retained-source interpretation,
admission and naming remain in `layer-color`/GTK.

```sh
LAYER_RASTER_FIXTURES=/tmp/capy-raster-fixtures cargo test --locked --offline -p layer-color --lib write_native_raster_fixtures -- --ignored --nocapture
cargo test --locked --offline -p layer-linux --release --no-run
LAYER_RASTER_FIXTURES=/tmp/capy-raster-fixtures bash tools/performance/gtk-raster.sh target/release/deps/layer_linux-26cdf45de1e69be4 native_common_raster_open_import_and_paste artifacts/image-placement/gtk-common-formats
```

Use the executable path emitted by the current build; Cargo can change its hash.

### Independent WebP sample qualification

Three files from Google's [lossless/alpha gallery](https://developers.google.com/speed/webp/gallery2)
now pass the shared decoder against independently produced **libwebp 1.6.0**
RGBA output: the lossless yellow rose and two VP8 lossy images with compressed
ALPH chunks. All RGB and alpha bytes match exactly, including hidden RGB and
2,061/6,450 partial-alpha pixels. This extends the earlier generated fixture
coverage; it is codec correctness evidence, not an additional native GTK run.
Google credits these samples to Jon Sullivan and Fizyplankton as public domain.

`tools/validation/webp_reference.py` downloads only these hash-pinned samples,
uses the installed native libwebp to generate independent reference pixels,
and records source URLs, authors, hashes, chunk compression and decoder version.
The fixture files stay outside the repository. Reproduce with:

```sh
python3 tools/validation/webp_reference.py --output /tmp/capy-webp-reference
LAYER_WEBP_REFERENCES=/tmp/capy-webp-reference cargo test --locked --offline -p layer-color --lib external_webp_reference_samples -- --ignored --nocapture
```

The external reference test passes (one test, three files). The recorded maximum
RGB error is zero for every file; alpha comparisons are exact. Evidence:
`artifacts/image-placement/webp-reference.log` and `webp-reference-manifest.json`.

## Initial HEIF/AVIF dependency findings (2026-09-16)

The installed `libheif` is **1.21.2**. A runtime call to
`heif_have_decoder_for_format` reports an AV1 decoder and **no HEVC decoder**;
`ldd` confirms dav1d/aom linkage. HEIC cannot be advertised on this installation
merely because `libheif.so.1` is present. At the time of this initial probe, the
GTK package staged the app and assets without bundling photo codecs.

The resulting integration decision was to pin and package actual decoding
backends with maintained libheif. The researched release was
[1.23.4](https://github.com/strukturag/libheif/releases/tag/v1.23.4), which includes
item/reference-limit and decode-cycle fixes. Earlier recent releases also fixed
[metadata decompression limits](https://github.com/strukturag/libheif/security/advisories/GHSA-24wx-9w62-c96w).
This matters to the existing bounded, cancellable photo-worker contract. Codec
packaging, ICC/NCLX and high-depth preservation were still implementation work
at that point; the probe alone did not add support or change filters. The
subsequent local implementation is recorded next.

## HEIF/AVIF implementation and open qualification

Historical native-codec qualification: production readers and GTK packaging now
use the [shared Rust core](portable-photo-core.md). The native tools below are
optional interoperability references, and the old package verifier has been removed.

The working tree includes a Linux reader behind `layer-color`'s `heif` feature,
enabled by GTK. A narrow C bridge loads a packaged native bundle; capability
checks require compatible libheif and actual HEVC and AV1 backends. GTK filters
and encoded-clipboard preference use that availability. A cancellation token
reaches native decode and row packing. Supported ICC/NCLX source interpretation
is retained, high-depth samples use U16 storage, container orientation is applied
once and collection primary images are labelled. Sequences and PQ/HLG HDR remain
explicitly unsupported.

`tools/validation/photo-codecs/photo-codecs.py` verifies pinned archives and builds libheif 1.23.4,
libde265 1.1.3 and dav1d 1.5.3. `apps/layer-linux/photo-codecs.mjs` validates the
recipe, bridge, patch, library and source hashes, then stages replaceable
libraries and corresponding source/license material. The local libheif patch
preserves source NCLX metadata through RGB conversion. Package builds now use
Cargo's lockfile explicitly.

### Codec references and error handling

**Eight focused tests pass**, covering basic HEIC/AVIF, HEIC alpha, exact ICC,
density, supported SDR color, HDR rejection, unavailable codecs, admission and
truncation. Independent libavif 1.3.0/AOM references additionally prove:

- Exact 10/12-bit P3 sample retention, including partial alpha and hidden RGB.
- Exact opaque rotation with duplicate EXIF orientation ignored and asymmetric
  300×150 DPI swapped to 150×300 once.
- P3 preserved from the AV1 bitstream with the container color tag absent;
  bitstream-only PQ HDR is still rejected.
- A valid rotated-alpha sample matches every RGB and alpha code, including
  123,952 partial-alpha pixels.
- The SDR base of the gain-map sample matches every RGB and alpha code. The gain
  map is not applied as an implicit HDR conversion.

The original failing fixture expected P3 after deleting its only color tag;
that expectation is corrected, with separate explicitly bitstream-tagged
fixtures added. The malformed-file test now truncates inside media data: cutting
a collection in half may leave its primary fully intact and is not necessarily
a decode failure. Encoded/decoded/source memory admission and dimension rejection
pass for both HEIC and AVIF, followed by successful jobs in the same process.
A fresh-process test verifies that a missing bundle hides formats and returns an
explicit codec-unavailable error. Cancellation before parsing passes; cancellation
inside native parsing/decoding and total backend memory remain to be qualified.

The broader color suite passes **72 tests with HEIF enabled** (11 ignored), and
**69 without the feature** (6 ignored). These counts overlap with the focused
run; they are not additive. Evidence: `artifacts/image-placement/heif-avif-reference.log`,
`heif-avif-color-regression.log`, `heif-avif-default-feature-regression.log` and
`avif-reference-manifest.json` in that directory.

Reproduce references with the system libavif 1.3.0/AOM library and C compiler:

```sh
python3 tools/validation/avif_reference.py --fetch --output /tmp/capy-avif-reference
CAPY_PHOTO_CODEC_DIR=target/photo-codec-reference/prefix/lib LAYER_HEIF_REFERENCES=target/photo-codec-reference/build/libheif-1.23.4 LAYER_AVIF_REFERENCES=/tmp/capy-avif-reference cargo test --locked --offline -p layer-color --features native-codec-reference --lib heif_ -- --include-ignored --nocapture
```

Use absolute paths for the three environment variables if running outside the
repository root. The helper verifies pinned downloads and records generator and
pixel-reference hashes; fixture images remain outside application assets.

### Native GTK and complete-package delivery

`native_heif_avif_open_import_and_paste` passes **one test / three files in
11.54 s**: HEIC, 12-bit P3 AVIF and ICC-tagged AVIF. Actual chooser and encoded
clipboard delivery preserve the source through Import, Apply, archive reopen,
Undo and Paste/Cancel. Open creates a source-sized document without assigning the
original photo as its master Save location. Collection naming is checked too.
Evidence: `artifacts/image-placement/gtk-heif-avif.log`.

Native test executable: `target/release/deps/layer_linux-42c87a8c65fca76a`, SHA-256
`c7659791af86f5d1180bd9d6b6714cccc6e3e6f4bb601de6a1bd068ce5afae97`.
It contains the codec integration. Subsequent source changes in this milestone
only extend reference tests and move the existing app capture diagnostic to also
handle file-launched documents; they do not alter placement/rendering behavior.

The complete package builds at `dist/capycanvas-linux`, with packaged executable
SHA-256 `6bfa787867eceee5af1a47e782ed5d8a0b70087cb586c35758deaf0e4939eb9d`.
It was copied to `/tmp/capy-gtk-package-review-s9fahz8i/capycanvas-linux` and run
from a separate working directory with codec and loader overrides unset. All
three photos opened and produced inspected, correct captures. Process maps show
all four photo libraries loading from that relocated bundle. Each process exits
successfully. This verifies actual packaged application file launches, separately
from the native test executable. It is not a native packaged Drop/Paste run.

Evidence and captures: `artifacts/image-placement/heif-avif-package/`, plus
`heif-avif-bundle-manifest.json`, `heif-avif-package-build.log` and
`heif-avif-gtk-test-build.log`. The first package attempt compiled successfully
but the workspace sandbox prevented `strip`; the approved local rerun completed.
The initial log is retained. `tools/validation/gtk_package_photo.py` reproduces
relocation checks without touching the user's session/settings. Its four-second
capture delay is intentionally excluded from decode-performance claims.

Still open: HEIF/AVIF sequence support, high-depth HEIC breadth, cancellation
during native work, actual large HEIF/AVIF memory/latency, and encoded-format Drop
qualification. Existing small-sample admission checks are not proof of a bound
on every native backend allocation. See the [remaining codec work](../ui/image-open-import-proposal.md#finish-the-existing-heifavif-implementation).

## 61 MP workflow at 2× scale with codecs enabled

The complete native JPEG workflow passes in **23.03 s** on the codec-enabled
native test executable above: external Drop, active handles, Apply, painting,
Undo/Redo, pan, Save/reopen, Original Size, menu Import/Cancel and photo Open.
The immutable 9504×6336 source, stored placement and painted artwork remain exact
through the checked history and archive operations. The active-placement capture
was inspected, including visible Original Size, Cancel and Apply controls.

Private Mutter display: **3200×2000 at 120 Hz, scale 2**, injected mouse at an
8 ms interval. GPU: NVIDIA RTX PRO 6000 Blackwell Max-Q, Vulkan driver 610.57.04.
No compilation or other GPU workload ran during the measurement.

| Measurement | Result |
| --- | --- |
| Drop to first presented photo | 1,920.46 ms |
| GTK loading heartbeat p95 / max gap | 16.19 / 30.18 ms |
| Scale GPU time p50 / p95 / max | 3.12 / 4.55 / 11.07 ms |
| Paint GPU time p50 / p95 / max | 3.75 / 5.76 / 9.98 ms |
| Delivered GTK pose to presentation p50 / p95 / max | 7.77 / 13.14 / 16.12 ms |
| Changed scale poses presented | 105 of 120 delivered changes |
| Pan GPU time p95 | 0.20 ms |
| Distinct camera update gap median / p95 | 16.67 / 25.02 ms |
| Native master Save | 329.63 ms |
| Photo Open preparation / subsequent renderer readiness | 929.45 / 1,357.79 ms |
| Whole-test process peak RSS | 1,743.41 MiB |

GPU p95 fits the 8.3 ms display interval, but maxima and pose-to-presentation p95
exceed one interval. This does not establish uninterrupted 120 Hz motion or
physical-input latency. The inherited pan path still produces roughly 60 distinct
camera updates/s (58 distinct poses in this trace). Process RSS includes several
windows, archives and retained test sources; it is not isolated GPU allocation.
Combined large layers, sustained pressure and physical pen qualification remain.

Raw report, build hash, log, captures and saved master are under
`artifacts/image-placement/native-61mp-codecs-2x120/`. This closes the unmeasured
single-photo 2× case on the stated device; it does not qualify large HEIF/AVIF
files or other hardware.

## Build evidence on d614f7b4

`cargo build --locked --offline --release -p layer-linux` passes. Runnable app:
`target/release/layer-linux`, SHA-256
`9a8127f99173867db2dcbc651a284139c0b9a79378740912fa81ac70dc1745af`.
This build contains the placement/drop/common-codec implementation and
preview/clipping fixes on `d614f7b4`; test-only instrumentation is excluded.
It is a local working build, not final acceptance of the remaining plan items.

Full passing renderer executable:
`target/release/deps/layer_render_wgpu-f5a1119c28e66c96`, SHA-256
`a877a54b31e905012681f2d8a5c1049746b151f341ae051536dfd595aa741003`.
Native input-to-presentation test executable used for the four timing runs:
`target/release/deps/layer_linux-26cdf45de1e69be4`, SHA-256
`3996b86888bd1ef9199ffd5c1f1f0d1f6a887dc04807cad343652b97e694f423`.
Cargo output paths can be reused by later builds; the hashes identify the evidence
above, not a promise that those paths still contain the same executable.

## Build on e7431720, before application file launches

`cargo build --locked --offline --release -p layer-linux` passes on `e7431720`
plus the local placement changes. Runnable app: `target/release/layer-linux`,
SHA-256 `67f4aa195f8e24c4f153e58368203af6ff0eebf8c284a9dc70ec0436f7a7462a`.
Log: `artifacts/image-placement/e743-gtk-build.log`. Test-only instrumentation is
excluded. This is a local working build; remaining plan gates still apply.

The post-merge focused sampling tests use
`target/release/deps/layer_render_wgpu-f5a1119c28e66c96`, SHA-256
`c7b3516ec620100f8bd21f07565e63614c9f4b09fa327a8494b981cf8178de86`.
Build log: `artifacts/image-placement/e743-renderer-build.log`. The six focused
passes qualify the merged sampling change; the full 265-test renderer run and
native timing results remain scoped to their earlier recorded builds.

## Release build with application file launches, before HEIF/AVIF

The release GTK app builds without warnings on `e7431720` plus the current local
changes. Runnable app: `target/release/layer-linux`, SHA-256
`b0ddb6e7460f5f5d981296e9757a9e6b7da80627f41ca5e8ca103c342185cb64`.
Build log: `artifacts/image-placement/file-launch-release-build.log`.

The native application-launch, menu-cancellation and current 61 MP workflows use
`target/release/deps/layer_linux-26cdf45de1e69be4`, SHA-256
`76c41edbd6fa496d5c0022c94ef75a1ad3beb055b8e8536acc114951e22216b3`.
Build log: `artifacts/image-placement/file-launch-test-build.log`.

The first new-test invocation preceded build completion and matched zero tests;
it is not acceptance. One parallel compiler faulted in optimization and deadlocked
in its stack-reporting allocator. Its captured stacks are retained under
`artifacts/image-placement/file-launch-compiler-*.log`; serial release builds
then passed (`cargo ... -j 1`, with libgcc preloaded for compiler diagnostics).
No repository compiler flags or application runtime settings changed for that
recovery. Two initial test failures were corrected in the helper: a query after
GApplication unregistered the secondary process, and late cancellation of a tiny
already-decoded fixture. Native cancellation now runs before worker publication,
matching the existing Open regression. Original failed logs are retained beside
the passing native evidence. Remaining plan gates and user approval are still open.
