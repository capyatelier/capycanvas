# Image Open, Import and Drop: updated plan

[Workspace and UI](README.md) · [Color requirements](color-management.md) ·
[GTK implementation and measurements](../development/image-placement-gtk-progress.md)

**Published:** the approved GTK implementation is in `8de027b7`, integrated with
newer upstream renderer changes in `522db8dc` and pushed to `origin/main`.
See the [Web/Android handoff](../development/image-placement-web-android-handoff.md).
The reassessment below records the starting point and approval scope.

**Reassessed 2026-09-16; HEAD now equals the fetched `origin/main` at `bef744d3`.**
The latest sync fast-forwarded from `448c1ee4`, preserving 130 changed paths
byte-for-byte and reconciling five overlapping renderer files. Backups and merge
verification are in `/tmp/capy-plan-refresh-r0zjgxlf/`. The implementation is
committed and published as recorded above. Focused verification is complete,
and the user has approved the result.

## Completed GTK goal

**The GTK Open/Import/Drop workflow and large-photo responsiveness are approved.**
The 61 MP reproduction crosses a fixed preview-memory cap at about 1.19× fit:
renderer completion previously jumped from ~2 ms to ~108 ms per motion frame.
The fix uses existing GPU-memory admission and retains the finer Float32 preview.
Native clipped translation and ordinary painting pass at 1.1×, 1.2× and 2× fit,
plus a two-photo case. Single-photo GPU p95 stays below 4.5 ms for both actions.
The replacement package and its actual relocated JPEG launch pass.
See the [runnable build, results and limitations](../development/image-placement-gtk-review.md).
The user explicitly replied “Approved” to the review request for this build.
General smudge optimization remains deferred.

Following the user's direction to focus on the goal, finish the implemented GTK
photo workflow and test that it meets the original requirements:

1. Verify the merged renderer, actual multiple-selection Import chooser and
   representative multi-photo delivery, including ordering, Cancel and one-step Undo.
2. Build the current package and test Open/Import/Drop, fit/Apply, ordinary painting,
   save/reopen and Original Size with the real 24 MP and 61 MP photos. Check exact
   source retention, usable interaction, thumbnails and memory on the merged build.
3. Provide the runnable package, reviewed captures, reproduction steps and measured
   limitations for user approval.

Fix failures in these workflows. Do not expand codec variants, other-platform work
or general brush-engine optimization during closeout. The wider investigations
below remain recorded follow-ups; known limitations must be disclosed, and the
goal is not complete until the working result has been reviewed and approved.

## Recommendation

**Build on the existing photo pipeline and finish the GTK placement work.**
Opening a JPEG from disk already works upstream. Source retention, tiled storage,
photo import/paste, painting and color management are also implemented. This
branch adds persistent layer transforms, external file drops, batch placement
and more image decoders. Those foundations should be reused.

Keep the agreed behavior: Open creates a source-sized document; Import/Paste and
canvas Drop add layers with placement handles. Applying placement retains the
full-resolution layer and stores its transform. Canvas size controls the visible
output rectangle, not the stored source dimensions.

### Completion summary

| Order | Work from the current starting point | Completion evidence |
| --- | --- | --- |
| 1 | Complete: merge and native Import/multi-photo delivery. | Correct ordering, source retention, Cancel, Apply and one-step Undo pass. |
| 2 | Complete: current GTK package and real large-photo qualification. | Open/Import/Drop, fit/Apply, clipped translation and drawing, save/reopen, Original Size, responsiveness and memory checks pass. |
| 3 | Complete: user review and approval. | The user approved the Open/Import/Drop workflow and large-photo responsiveness of the delivered build. |

The main change to the plan is that photo Open, retained sources and basic import
are established foundations. The local placement/drop implementation also exists;
the focused fixes and acceptance tests are complete. Further format variants
and broad smudge optimization remain follow-ups. Existing large-smudge results
and measurement limits are recorded in the review document; they do not expand
this closeout into an unrelated brush redesign.

## Open, Import and Apply behavior

| User action | Result |
| --- | --- |
| **Open image** | Create a document at the image's oriented pixel dimensions. Fit the view using zoom. Preserve transparency; save an editable `.capy` master separately from the input photo. |
| **Import / Paste image** | Add a layer to the current document, centered on the canvas, with placement handles active. |
| **Drop image on canvas** | Use the same placement flow, centered at the drop point captured in document coordinates. |
| **Drop onto Layers** | Show above/below/into feedback and use shared group, lock and clipping rules. |
| **Apply placement** | Store position, scale and rotation. Retain full-resolution source and existing paint in layer-local coordinates. |
| **Original Size (100%)** | Restore native pixel scale, including after Apply and save/reopen, while preserving center and rotation. |
| **Import / Drop several images** | Prepare every file before insertion, preserve provider order, fit each independently and select the batch with shared handles. Apply creates one undo step. |
| **Cancel initial placement** | Remove the complete provisional batch and restore the previous selection without an artwork undo entry. |

Initial fit uses
`min(1, canvas_width / source_width, canvas_height / source_height)`.
Small images keep native size. Embedded print DPI remains metadata and does not
change pixel-based placement.

**Accepting the transform does not downsample the layer to the canvas.**
A 6000×4000 photo fitted into a 2000×1500 canvas keeps its 6000×4000 source after
Apply, with approximately 33.3% layer scale. Returning to 100% reveals the retained
detail. Canvas edges hide content without deleting it. Disposable display previews
may be smaller; flattened export samples onto its output pixel grid while the
editable document keeps its source.

This preserves decoded source samples, not the original compressed JPEG bytes.
Enlargement beyond native resolution still requires interpolation. Explicit pixel
edits and pixel-selection transforms retain their established raster-edit meaning.

## What latest main already supplies

This table describes the established upstream photo foundations; the new shared
renderer changes in **`bef744d3`** are described immediately below it.

| Capability | Upstream state | Consequence |
| --- | --- | --- |
| JPEG/PNG/TIFF Open | GTK, Web, Android and Apple use shared signature-based decoding and photo-document policy, with supported depth, profile, orientation, alpha and density. | Reuse existing Open and photo preparation. |
| Import / Paste | Shared commands and host entry points already insert retained photo sources. GTK and Apple prepare off-thread and reject stale results. | Reuse prepared sources for active placement and additional transports. |
| Full-resolution storage | Immutable tiled `SourceImage` has independent dimensions and U8/U16 samples plus color interpretation. `.capy` stores sources; duplication/history can share them. | No second original-image store is needed. |
| Photo painting | Copy-on-write paint tiles override edited source regions. Source profile repair and explicit rasterization already exist. | Keep source-aware painting after placement. Accepting a transform must not require rasterization. |
| Sources larger than canvas | Independent source dimensions, decode budgets and tiled rendering exist. | Finish the local edit geometry and qualify oversized layers; an infinite canvas is unnecessary. |
| Accepted scale/rotation | Upstream layer geometry has translation offsets; generic transform Apply commits raster edits. | Source retention alone does not supply revisable scale/rotation. Persistent affine placement is local work. |
| External photo drops | Existing upstream receivers serve internal workspace/layer payloads. | The external-photo receiver is local branch work. |
| Image formats | Shared upstream decoding handles JPEG, PNG and TIFF with explicit variant restrictions. | Broader GTK format support comes from the local readers, not this main sync. |
| Apple photo/color tools | Retained Open/Place/Paste, document profile/depth controls, source repair, ICC import, histogram/sampling, corrections and masks are implemented. | Reuse these workflows; separate source retention from active-placement integration. |
| Apple export/viewing/recovery | Profiled export, previews, presets, saved ICC library, managed P3 SDR canvas/controls and recovery are implemented and have scoped acceptance records. | Preserve the existing snapshot, color and recovery paths. |

Code checked against upstream: [decoder](../../crates/layer-color/src/photo.rs),
[photo policy](../../crates/layer-color/src/photo_project.rs),
[source model](../../crates/layer-core/src/color/source.rs),
[layer properties](../../crates/layer-core/src/layers.rs),
[import](../../crates/layer-ui/src/art_layers.rs), and
[transform operation](../../crates/layer-ui/src/operation.rs).
These links display the working tree, which also includes local changes.

### What the newest commits change

- `996ad2c2` adds 64 decoded source-cache slots and byte-based upload admission
  with the existing 16 MiB staging ceiling. This supersedes the overlapping local
  admission/cache change. Keep upstream capacity on all renderers and retain local
  source packing.
- `bef744d3` reuses equal native raster tiles by their existing content digest and
  color spaces, and removes the wait after the final display-composition batch.
  These shared renderer changes require GTK integration/performance verification.
  Upstream Apple layered-ink improvements and remaining watercolor limitations are
  scoped in the [performance record](../../apps/layer-apple/PERFORMANCE.md#native-tile-reuse-and-final-display-batches--2026-09-16).

The preceding Apple qualification commits remain relevant foundations:

- `6af9d193` adds a shared Apple form picker with visible labels on iPad, layout
  captures and expanded feature-inventory coverage.
- `448c1ee4` fixes cleanup of abandoned private recovery files after durable
  publication/discard. Eight local process-interruption cases pass in the
  upstream record. The hardware inventory follow-up covers the complete tool
  selection set, including Point/3×3/5×5 sampling.

Those preceding commits changed no GTK photo loading, placement, codec or renderer
code. Their completed Apple tasks do not qualify GTK placement. Details:
[form/inventory record](../development/apple-handoff.md#sdr-form-fit-and-feature-inventory),
[interrupted recovery record](../development/apple-handoff.md#recovery-publication-under-process-interruption).

### Platform boundary after the latest merge

GTK is the current implementation and approval target. Web/Android already have
retained-source workflows; adopting and qualifying the local placement/drop
changes remains separate host work.

Apple's shared photo worker replaces its old sRGB8 adapter and 8192-pixel import
cap. Shared memory/dimension limits and renderer limits still apply. Apple
Place/Paste currently calls `import_layer_source`, inserting directly, rather
than the local `place_layer_sources` active-placement transaction. Picker and
clipboard exposure remains JPEG/PNG/TIFF. Reuse the shared preparation/capability
contract when extending it.

Upstream Apple records cover native Mac controls, retained photo edits/export,
managed SDR recovery, synthetic 61 MP integrity, narrow form layouts and local
process interruption. Physical iPad/provider workflows, cross-display appearance
and sustained camera-photo performance remain separate acceptance work. Apple
runtime checks were not rerun on this Linux machine. See the
[Apple handoff](../development/apple-handoff.md) and
[Windows handoff](../development/color-management-m2-windows-handoff.md).

## What the local GTK implementation already adds

These paths exist in the working tree with partial qualification. They are not
features shipped in main or a claim of final acceptance.

| Area | Implemented locally | Evidence / remaining work |
| --- | --- | --- |
| Persistent placement/storage | Layer affine, local source/paint extents, composed mask geometry and version 5 archives when required. Ordinary documents retain version 4. | Fit/Apply/Cancel/Original Size, batch history and exact save/reopen pass. Older readers reject version 5; finish editing/recovery audits. |
| Import/Paste/Drop | Fit/center placement, shared batch handles, visible Apply/Cancel even with Tool Settings hidden, canvas and Layers `GdkFileList` COPY receivers. | Native mouse/touch canvas batches and mouse row destinations pass at 1×/2×, including cancellation, stale targets and errors. Native multiple-selection chooser batches also pass. Physical pen coverage remains a follow-up. |
| Application file Open | Cold/warm file launches, ordered documents, cancellation/profile prompts and source-sized renderer creation. | Native launch checks pass; a cold photo launch creates no extra blank drawing. |
| Rendering/editing | Source and paint share placement geometry; brushes, regions, figures/gradients, selections, masks and sampling have integration. Distant-source smudge/liquify sampling, source packing and tighter nonlinear bounds are implemented locally. | Earlier renderer suite: 270 pass, 29 ignored. Nine placement GPU checks pass after the bounds change, including nonlinear source-page colors. Native smudge/twirl history and source checks pass; large smudge is slow. Wet pickup, wider editing and failure paths remain. |
| Display/thumbnail caches | Bounded reduced-photo cache; incremental edit/history invalidation. Thumbnails integrate full local backing and display its orientation. | Six thumbnail GPU checks, 434 shared UI checks and native AVIF drawer delivery pass. Current 61 MP JPEG Open/Drop/Original Size captures all show the complete thumbnail. |
| Extra formats | BMP/DIB, GIF, WebP and Linux HEIF/HEIC/AVIF readers, shared GTK filters/MIME preference, first-frame naming and a pinned codec bundle. | Native/reference and relocated ABI 2 package checks pass for recorded inputs. The rebuilt package also passes its 61 MP JPEG launch. Further variants and aggregate decoder memory remain follow-ups. |

Implementation: [placement transaction](../../crates/layer-ui/src/operation/placement.rs),
[GTK drop receiver](../../apps/layer-linux/src/files/drop.rs),
[Open](../../apps/layer-linux/src/files/open.rs),
[Import/Paste](../../apps/layer-linux/src/files/place.rs),
[target geometry](../../crates/layer-render-wgpu/src/target_geometry.rs),
[compositor/cache](../../crates/layer-render-wgpu/src/scene/placement.rs), and
[GPU placement tests](../../crates/layer-render-wgpu/src/placement_tests.rs).

## Recorded follow-up scope

The closeout sequence above is the current delivery plan. The broader audits and
extensions below remain tracked, but do not all precede core workflow review.
Confirmed pixel loss, broken history or failure of the core workflows still
requires a fix before presenting them as complete.

### 1. Finish placement correctness and brush responsiveness

1. **Preserve the corrected thumbnail behavior.** The confirmed oversized-photo crop
   is fixed locally: overview integration uses full source/local backing, and the
   small display pass applies orientation. Translation/uniform scale do not change
   thumbnail framing. Geometry and source-profile replacement invalidate host
   previews. GPU/shared checks, native AVIF drawer delivery and the current 61 MP
   workflow pass. The full thumbnail appears by 2.12 s after Drop; the Original
   Size revision also arrives and its capture is verified. Preserve this behavior
   in the final packaged build.
2. **Fix large-brush performance and finish material qualification.**
   [`material_brush.wgsl`](../../crates/layer-render-wgpu/src/material_brush.wgsl)
   previously sampled only a 3×3 tile neighborhood. Two new regressions confirm
   missing samples at 1/32 scale: liquify produced transparency and smudge kept
   the wrong color. The local fix gathers full-resolution samples in bounded
   passes, retaining the nearby fast path. The complete renderer suite passes,
   including rotated/mirrored/blur/prediction/history checks and seven supported
   liquify modes; the independent source-page identity check also passes.
   Profiling established CPU preparation/encoding as the main large-smudge cost.
   Fixed-width source-row packing preserves every source code and reduces that
   work. Tighter twirl/pinch/expand bounds preserve the shader's exact coordinate
   calculation while fetching fewer pages. The updated complete native journey
   passes: 1024 px smudge uses 2,520 gather jobs / 5,040 passes; 240 px twirl uses
   2,076 / 4,152. Smudge still reaches 183 ms; twirl presentation gaps are
   8.60 / 15.23 / 31.76 ms p50/p95/max. Continue reducing preparation, binding and
   prediction costs, retaining exact full-resolution artwork. Ordered metadata
   buffer reuse also passes; it removes per-pass allocations without materially
   improving measured latency. Then cover watercolor transport and
   wet pickup, which use separate paths.
   Earlier zero-gather twirl results sampled before slow strokes drained; actual
   native events reached the paint queue. The fixture now waits for completed
   stroke/input/render work before reporting statistics.
3. **Finish the editing audit.** Cover existing overrides, masks/link toggles and
   application, groups/clipping, effects, selections, sampling/regions, source
   repair/rasterization and renderer replacement. Preserve document-space brush
   size and source-local paint/mask alignment. Exact snapshots/export must never
   use reduced display pixels as artwork.
4. **Harden transaction failures.** Exercise failed Apply/Cancel, member deletion,
   locks, competing edits, document replacement and renderer loss. Save/recovery/
   close must not serialize half-committed placement. Preserve the passing
   rollback after a failed initial preview.

**Gate:** oversized fit → Apply → paint/mask → save/reopen → Original Size retains
source samples and full local edits, with correct history and exact output.
General unbounded painting, negative tile coordinates and canvas resizing are
separate work.

### 2. Qualify GTK file delivery and finish destination edge cases

- Complete actual multiple-selection chooser coverage and remaining native
  file-manager/provider/device delivery. Keep local hard-drive files as the
  current transport: providers without local paths are explicitly rejected;
  remote streams would require bounded cancellable staging.
- Preserve camera/DPI/rotation mapping captured at drop time. Delayed decoding
  must not retarget a request after camera or document changes.
- Preserve above/below/into Layers validation, group subtrees and clipping bases.
  Default Import inserts above the selected clipped stack; explicit invalid
  boundaries are rejected.
- Retain atomic batch preparation: provider order, one layer per image, one
  Apply undo step, and no insertion on failure/cancellation. Identify the failed
  file; reject mixed project/image batches until a mixed-document flow exists.
- Preserve cold/warm application Open, ordered source-sized documents, profile
  prompts and cancellation acknowledgement. GTK currently has no separate
  document-free editor drop surface. A temporarily unavailable renderer in an
  existing document must not be treated as an empty application.

Native hosts own external data acquisition and pointer capture. Shared Rust owns
destination validation, placement and history. External drags arrive recognized
and add no destination hold. Follow the [drag convention](drag-and-reorder.md)
for existing rows/tiles/handles. Mouse, touch and pen need separate evidence;
callback injection alone does not establish physical-device delivery.

**Gate:** Open, Import, Paste and both Drop destinations use the same prepared
source contract, with correct ordering, history and stale-result safety.

### 3. Deliver common-format parity through the shared decoder

Use one real capability list for Open/Import/Drop filters and errors; Paste uses
the same encoded readers when those representations are offered. Detect file
signatures rather than trusting extensions. Preserve supported profiles, depth,
transparency and orientation once, including source interpretation prompts.

| Format family | Current state and remaining scope |
| --- | --- |
| JPEG | Upstream SDR/profile/orientation/density handling exists. Preserve it across new codec and packaging changes. |
| PNG | Upstream U8/U16, palette, alpha and color handling exists. APNG is currently rejected; deliberate first-frame support remains an extension. |
| TIFF | Supported unsigned 8/16-bit SDR layouts exist. Qualify additional common layouts/pages explicitly. |
| BMP/DIB, GIF, WebP | Local readers and GTK Open/Import/Paste pass. Animated GIF/WebP use the composited first frame with a layer-name notice. Independent WebP references pass; broaden real-file and Drop coverage. |
| HEIF/HEIC, AVIF | Local readers preserve ICC/supported NCLX, high depth and container geometry. AVIF sequence/grid/crop references and native cases pass. HEIF sequences and AVIF track display matrices/scaling are still rejected; high-depth HEIC, cancellation/memory and remaining delivery need qualification. PQ/HLG HDR remains unsupported. |
| RAW, PSD, SVG, PDF, EXR/HDR | Require development, structured-document, rasterization or extended-range workflows beyond ordinary SDR photo placement. |

“Common image formats” should mean consistent supported variants across entry
points. Do not promise every valid TIFF/HEIF/PNG variant before its decoder policy
and tests exist, or silently reduce rich images to untagged RGBA8.

#### Finish the existing HEIF/AVIF implementation

Reuse the [native reader](../../crates/layer-color/src/photo/heif_io.rs),
[C bridge](../../crates/layer-color/src/photo/heif_bridge.c),
[pinned codec recipe](../../tools/build/photo-codecs.py) and
[package verifier](../../apps/layer-linux/photo-codecs.mjs).
ABI 2 uses libheif/libde265 for HEIC and libavif/dav1d for AVIF. Package the
replaceable libraries, licenses and corresponding sources.

1. Preserve independent exact high-depth, alpha, ICC/NCLX, grid and all 16
   crop/quarter-turn/mirror references. Broaden real high-depth HEIC inputs.
2. Finish HEIF sequence policy and AVIF track display geometry, including guards
   for unsupported geometry. Retain actual first-frame selection when an AVIF's
   primary poster differs, selected-frame metadata and first-frame disclosure.
3. Qualify malformed/unavailable-backend cases, native decode cancellation,
   large high-depth photos and aggregate batches. Decoder context limits alone
   do not establish a bound on all backend allocations.
4. Extend native encoded Drop and rebuild the final package. Preserve the five
   successful relocated ABI 2 launches. Their package predates the latest main
   sync and thumbnail fix; they do not qualify a current review binary.

### 4. Qualify realistic GTK workflows and obtain approval

Reuse the existing actual 24 MP and 61 MP photos and native journey: Open;
oversized Import/Drop into 2000×1500; transform handles; Apply; paint; navigation;
save/reopen; Original Size. Single-photo workflows already pass on recorded
builds at 60/120 Hz, including 61 MP at 2× display scale with codecs enabled.

| Recorded 61 MP workload, 2×/120 Hz | Latest JPEG test build | Earlier AVIF build, derived from that photo |
| --- | ---: | ---: |
| Drop → first presented photo | 1.65 s | 2.51 s |
| Scale GPU queue-span p95 | 3.88 ms | 3.70 ms |
| Paint GPU queue-span p95 | 4.51 ms | 4.80 ms |
| Delivered pose → presentation p95 | 8.31 ms | 8.32 ms |

The JPEG run uses `448c1ee4` plus local thumbnail/material, source-packing,
tighter-bounds, metadata-buffer, direct-preview and native-cache fixes. Both
measurements predate the `bef744d3` merge; the AVIF run also predates those local
fixes. These scoped NVIDIA runs do not establish
continuous 120 Hz motion. GPU queue span can include waiting for CPU
submission; it is not isolated GPU execution time. Pose latency covers delivered
and presented GTK events, not physical input. Pan currently produces roughly
60 distinct camera updates/s on the 120 Hz test display. Isolated 61 MP AVIF
preparation measured 1.94 s / 414 MiB peak process RSS; three cancellation runs
acknowledged in 28–406 ms. None establishes aggregate or high-depth HEIC bounds.

The updated pre-merge JPEG journey passes in 29.00 s, including source/artwork checks through
Apply, paint and both material strokes/Undo/Redo, save/reopen, Original Size,
Import cancellation and photo Open. Visible thumbnail: 2.10 s after Drop;
whole-workflow peak process RSS: 2,600.83 MiB. This includes two large material
strokes, so its memory peak is not comparable to the earlier ordinary journey's
1,748.31 MiB as an identical workload. Passing the journey does not establish
acceptable large-smudge latency. Current placement, Original Size and photo-Open
captures were reviewed. Build hashes
and raw evidence are in the progress report.

Finish combined large-layer workloads, memory pressure/cache retirement,
sustained editing and physical pen coverage. Measure decode/admission, first
placement and thumbnail, motion, paint, navigation and CPU/GPU allocation peaks
separately. Record hardware, binary/input hashes, display scale/refresh,
p50/p95/max, dropped presentations and actual camera changes. Target display
frame budgets of 16.7 ms at 60 Hz and 8.3 ms at 120 Hz; report cold loading
separately and keep controls responsive. Check exact source/artwork retention
alongside speed.

Prepare a current packaged build, reviewed captures, reproduction steps and
measured limitations, then obtain user approval. Other hosts retain separate
acceptance; the implementation goal remains open until the GTK work and review
are complete. Detailed logs, hashes and measurement scope remain in the
[progress report](../development/image-placement-gtk-progress.md).

## Professional-tool patterns supporting the recommendation

- Photoshop Place adds a Smart Object, fits oversized artwork and presents a
  transform with Commit/Cancel. OS file drops can use this flow.
  [Adobe: Place files](https://helpx.adobe.com/photoshop/using/placing-files.html).
- Affinity exposes Original Size at native dimensions and accepts Finder/Explorer
  drops as new layers. Its placement flow also allows clicking or dragging to
  set the initial size. Capy's fit-with-handles policy is our design choice.
  [Affinity: Placing content](https://affinity.help/photo2/en-US.lproj/pages/Media/placeImages.html).
- GIMP's Open as Layers adds content to the current image, separately from Open.
  [GIMP: Open as Layers](https://docs.gimp.org/3.0/en/gimp-file-open-as-layer.html).

Official sources rechecked 2026-09-16. They support the interaction recommendation;
the storage/editing design follows Capy's retained-source architecture.

## Evidence and limits of this reassessment

- Fetched and fast-forwarded main to `bef744d3`; 130 pre-existing changed paths
  remain byte-identical and five overlapping renderer files were reconciled.
  Manifest, backups and verification: `/tmp/capy-plan-refresh-r0zjgxlf/`.
  Earlier synchronization records remain in the progress report.
- Read the two new upstream commits, Apple acceptance records, upstream decoder/
  import/geometry paths and relevant local placement/GTK code separately.
- Post-sync `cargo test --locked --offline -p layer-ui --lib placement` passes
  all eight selected tests (0.16 s). The substring also selects replacement
  tests; this is shared behavior evidence, not native performance qualification.
- `cargo check --locked --offline -p layer-host --example inventory` passes
  (10.76 s), including the upstream inventory changes and shared dependencies.
- Previous GPU/native/codec/package results retain their original build scope.
  Source packing passes the exhaustive integer-code GPU check; tighter nonlinear
  bounds pass nine placement GPU checks and the full native material journey.
  Large-smudge latency remains a confirmed problem; native twirl delivery is now
  qualified for the recorded workload. Apple runtime tests were not run here.
  The packaged application still predates the thumbnail/material fixes.
- Earlier document-link, whitespace and renderer-format checks passed on their
  recorded versions. Current merge and closeout verification is recorded in the
  progress report. No commits or pushes were made.
