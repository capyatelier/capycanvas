# Phase 2 handoff: macOS and iPadOS

2026-09-16. Implement and qualify complete SDR color/photo editing on **both**
Apple hosts. GTK is qualified within its recorded envelope; Web/Android have
passing device workflows, with the latest user retest pending. Apple is not yet
compatible with all shared phase 2 contracts. Start from the integration of
`a8b3cd76` and Apple upstream `9b7c4eb7`; preserve the latter's prediction,
presentation-progress, lifecycle and diagnostics fixes.

Read the [current scope and remaining work](../history/color-management-m2-port-handoff.md),
[SDR journeys](../ui/color-management.md), and
[tablet validation](../history/color-management-web-android-m2-validation.md).
Historical plans are proposals/evidence; verify against current code and Metal.

The native SDR foundation checkpoint now passes both Release builds and local
Swift/Metal checks. Existing effect/gradient controls preserve tagged colors;
startup and prepared-document constructors use native integer backing with
Float32 processing. P3/U8 and ProPhoto/U16 exact save/recovery/history and GPU
replacement pass on Mac Metal for both Apple policies. See the
[scoped acceptance record](apple-handoff.md#native-sdr-foundation).
The next checkpoint adds complete New Drawing options/presets/defaults, tagged
paint entry, workspace palettes and document-space wheel previews. Shared,
Swift/Metal owner and full Mac UI checks pass; see the
[workflow record](apple-handoff.md#sdr-creation-and-paint-workflows) for current
device coverage. Retained photo Open/Place/Paste now share the native worker and
profile/depth policies, replacing the lossy sRGB8 import path; see the
[photo workflow record](apple-handoff.md#retained-photo-open-place-and-paste).
The subsequent [batch milestone](apple-handoff.md#interactive-photo-batches--2026-09-17)
adopts shared Open policy and interactive multi-image Place/Paste with
Original Size, Apply/Cancel and one-step history. Native format filters use shared
decoder capabilities; clipboard loading is sequential. External canvas/layer
drops now share that transport and placement policy, with captured targets and
delayed-provider cancellation; see the [drop record](apple-handoff.md#external-photo-drops--2026-09-17).
Native Mac cross-application canvas/row drops pass; physical UIKit placement/provider acceptance remains open.
Color and retained-source transactions now also consume shared workflow state,
choice/identity validation, comparison readiness and renderer rollback; the
[transaction record](apple-handoff.md#shared-colorsource-transactions--2026-09-17)
records the native integration and qualification scope. Both final Release builds,
shared/native/Swift owner checks and Web compilation pass. The combined iPad
review includes the subsequent composition scheduling correction, with all eleven
recovery records and saved files preserved; physical drawing/Diagnostics and
large-photo local save/reopen are pending. Artwork recovery now also executes
the shared policy's storage tickets; see the
[recovery integration](apple-handoff.md#shared-recovery-policy--2026-09-17).
Physical lifecycle interruption/expiration remains open.
Assign Profile, Convert Color Space, Change Bit Depth and Document Properties
now use the same worker, with complete before/after previews, source-safe flattened
copies and exact shared history; see the
[document color record](apple-handoff.md#document-profile-and-bit-depth-workflows).
Source-profile repair and full-extent rasterization now reuse shared source edits,
the document worker and comparison form. Native ICC import is available in both
repair and missing-profile prompts; see the
[source workflow record](apple-handoff.md#retained-source-editing-and-icc-import).
Histogram and Point/3×3/5×5 sampling now reuse shared inspection and eyedropper
paths; see the [inspection record](apple-handoff.md#histogram-and-sample-area-workflows).
The six retained photo corrections and local masks pass exact worker save/reopen
and re-editing checks on both Apple policies, using existing shared controls; see
the [correction record](apple-handoff.md#retained-photo-corrections-and-masks).
Profiled export/presets and the ICC library now use the shared snapshot/CMM and
file worker, with exact source output and both-policy owner checks; see the
[export record](apple-handoff.md#profiled-export-and-icc-library) for native/device
coverage. Managed SDR canvas and controls now share explicit Display P3 tags and
shared transforms, including working-space adoption and native display observation;
see the [display record](apple-handoff.md#managed-sdr-canvas-and-controls).
Retained source/paint/correction/mask recovery now passes the complete persistence
barrier and fresh-owner restoration on Mac Metal with both Apple policies;
the [recovery record](apple-handoff.md#sdr-recovery-and-current-review-builds)
identifies the review apps and user-confirmed drawing, Undo/Redo, background/return
and local Save As/reopen workflow on both devices.
The 61 MP class synthetic-JPEG G-Pen, exact history/save/reopen and GPU-loss
regression also passes both policies on Mac Metal. Physical iPad execution and
large-photo performance remain unqualified beyond the subsequent warm native
URL-open check on `90adbb6d`: the JPEG's canvas and Navigator render, with all
existing drawings preserved. The user then reports fast 570 px G-Pen circles
lagging by up to one second and repeatable mid-zoom stalls. The attached trace
and paired replay identify redundant contact/composition work and discarded
zoom cache pixels. Shared fixes pass focused integrity checks and both Release
builds. The user confirms smooth repeated zoom. Subsequent shared preview and
paint-region changes improve drawing further, with roughly 50 ms p99 reported;
drawing performance remains open. The requested
[algorithm review](apple-drawing-performance-review.md) identifies remaining
preparation/submission opportunities and does not establish a hardware floor.
The shared tile-plan/bounded-overlap follow-up improves paired local replay with
exact artwork/history and passes final host builds. The iPad is updated with all
drawings preserved; physical drawing and large-photo local save/reopen are pending. See the
[performance record](../../apps/layer-apple/PERFORMANCE.md#large-photo-fast-strokes-and-repeated-zoom--2026-09-16)
and `artifacts/apple-photo-lag-v1/`. Large-photo local save/reopen remains
unconfirmed.
The current SDR layered-4K Mac performance regression is corrected in the
shared renderer: decoded-tile retention and byte-based staging accounting reduce
ten-minute long active intervals from 41.397% to **1.016%**, within the accepted
rare-miss standard. Pixel/history regressions and both Release builds pass; see
the [performance record](../../apps/layer-apple/PERFORMANCE.md#layered-sdr-drawing-upload-cache-correction--2026-09-16).
The next correction reuses identical native tiles by their existing content
identity, with unchanged memory limits. Physical iPad layered-4K then completes
ten minutes with **0.576%** long intervals. Moving the bounded-display wait before
the next batch removes terminal CPU stalls. Heavy 4K watercolor still has
12.803%/28.347% long intervals on Mac/iPad in short runs. Removing redundant
watercolor composition passes lowers these to **3.688%/17.929%**, with exact
replay captures, 13 Metal regressions and both Release builds passing. Remaining
heavy-watercolor performance is still the immediate blocker. Shared render-pass
batching further lowers short-run misses to **1.764%/15.893%**. Optional GPU queue
timing does not explain the iPad gap: disabling it gives **15.740%**. Physical GPU
attribution then identifies costly interleaved tile copies. Batching those copies
reduces matching timer-off runs to **1.030%/12.868%** on Mac/iPad with exact replay
pixels and less production code. Nine broader-suite test-assumption failures are
then corrected without changing production rendering; 259 tests pass and the
unchanged Linux filter atlas remains the sole failure. The current independent
Metal comparison covers all 160 complete images with maximum raw difference two
levels, and representative pairs pass perceptual review. See the
[filter qualification](apple-filter-qualification.md#current-renderer-follow-up--2026-09-16) and
[current performance record](../../apps/layer-apple/PERFORMANCE.md#watercolor-tile-copy-batching--2026-09-16).
The subsequent fetch integrates shared retained-photo placement at `522db8dc`.
Apple bridge checks pass 68 tests; the integrated renderer's new preview fixture
passes after explicitly supplying its test allowance. All 163 filter images
reproduce the preceding comparison. See the
[integration qualification](apple-filter-qualification.md#retained-photo-integration--2026-09-16).
The subsequent `a5f57e2c` replay reproduces the earlier watercolor pixels and
per-frame work. The integrated iPad Release now runs in place with all ten
recoveries preserved; a short GPU trace confirms the remaining transport,
copy and composition cost. Ordinary review is restored with recording disabled.
The Mac artist review is unchanged. See the
[GPU follow-up](../../apps/layer-apple/PERFORMANCE.md#integrated-renderer-gpu-follow-up--2026-09-16).
Earlier presentation results remain source-scoped; the instrumented iPad run
does not establish sustained performance of the integrated renderer.
The next milestone enables the shared display caches using measured Metal and
process/system headroom. Paired 60 MP ProPhoto U16 Mac navigation improves from
2.753 to 0.591 ms median at native scale, with intact source/paint/mask roots.
Both Release builds, focused renderer checks and all 69 Apple bridge cases
(including the separate 61 MP JPEG regression) pass on Mac Metal. The updated
iPad review retains all ten recoveries. Short heavy-watercolor cadence remains
near the prior result, so this is a large-photo display improvement, not closure
of that blocker. See the [admission record](../../apps/layer-apple/PERFORMANCE.md#metal-display-admission--2026-09-16).
Continue with that GPU/display diagnosis, remaining current SDR profiles, provider/
background workflows in step 4 and remaining physical SDR controls/display checks. Device
performance and provider delivery are not fully closed.

The physical M4 startup check exposed and fixed a vendored wgpu Metal Float32
capability mismatch; startup and short synthetic painting now pass. The simulator
still lacks required Float32 filtering. Physical XCTest's extra runner is blocked
by the device's free-profile app limit; preserve the installed artist apps.
Use fast shared/native-owner checks during implementation and group remaining
device interaction checks, instead of repeating full-app simulator failures.

## Implement in this order

1. Existing effect/gradient controls in `Shared/Editor/PropertyControls.swift`
   now use the shared tagged form. Preserve this contract: shared `RgbColor` is
   `{space, rgba}` with **encoded** RGB in the named space. Preserve the tag in
   edits; convert native swatches/previews to their declared display space.
2. `native/src/metal.rs` and `native/src/project.rs` now use the native SDR path
   for startup, prepared documents and GPU replacement. Carry that contract
   through the new export/snapshot workflows. Use Android's
   `native/src/android.rs` and `documents.rs` as reference. Backing is straight
   alpha U8/U16; processing is Float32. Query actual Metal capabilities; preserve
   the shared portable publication path where in-place editing is unavailable.
   Never narrow edits/export through FP16 display caches.
3. Port New/Open/Place/Paste, tagged numeric colors/palettes, profile/depth
   changes and exact history, source repair/rasterization, histogram/sampling,
   six retained photo corrections/masks, ICC library and profiled export presets.
   Reuse `layer-color`, `layer-ui`, `snapshot`, and Android's `color_edit.rs`,
   `source_edit.rs`, `inspection.rs`, `color_preferences.rs`; keep heavy work off
   the UI/render owner and publish prepared document + renderer atomically.
4. Integrate managed canvas **and** native controls, Mac monitor changes, iPad
   display policy, native pickers/providers and background/recovery lifecycle.
   Keep current AppKit/UIKit input ownership and document-adoption prediction.

## Prove completion

Build Release macOS and iPadOS with [the Apple guide](apple.md). Capture a fresh
baseline first; finish benchmarking/optimization after functional integration.
Run shared tests plus actual Swift/Metal and device workflows: P3 U8 painting,
ProPhoto U16 exact save/export, revisable corrections/masks, cancellation,
undo/redo, and device replacement. Repeat **61 MP JPEG → G-Pen → save/reopen →
GPU recovery**; original photo pixels must survive every touched tile.

Retain ordered staging uploads and bounded composition submissions from
`37deed7b`; large blank recovery frames also need command-memory bounds. Qualify
unified-memory pressure and camera-only navigation separately from regeneration.
Target smooth iPad 120 Hz; current Mac evidence is 90 Hz, with 120 Hz hardware
qualification still deferred. Document actual refresh, missed frames and latency;
do not label CPU submission as presentation. Preserve users' files/recovery,
identify the installed build, commit significant checkpoints, and publish a
short acceptance record with remaining hardware gaps. Print proofing/HDR is later.
