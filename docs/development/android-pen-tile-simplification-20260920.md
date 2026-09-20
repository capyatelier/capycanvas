# Wacom tile overhead: simplify first, then reuse

Implemented results and the qualified scope are recorded in the
[shared material executor report](android-pen-tile-executor-20260920.md).
The approximately 39/s estimate below is the original conditional forecast;
the measured implementation did not reach it.

Source: `51e2b005`, whose renderer remains `39e9cba3`. This follows the
[CPU/tile analysis](android-pen-cpu-composition-tiles-20260920.md) and responds to
the user's priorities: retain 256 px tiles, reduce code/algorithm complexity,
and permit brush changes that preserve artistic intent.

**Recommendation:** unify the existing material-tile executors, retain bindings
with their resource owners, and make prediction consume the existing sparse
tile plan. Do not start with texture arrays, a second compositor, or another
G-Pen-specific fast path. The first quantitative forecast is approximately
**39 canvas updates/s versus 33.1/s with the same full probes**, subject to the
explicit model below. This is a forecast, not an implemented speedup.

## New measurement: the old binding timer was incomplete

The previous 3.13 ms binding figure covered `PipelineDevice::create_bind_group`.
`create_material_bind_group` takes a raw `wgpu::Device` and bypassed that timer.
Consequently 3.13 ms was not a ceiling on all binding-creation savings.

A temporary four-line diagnostic patch adds a span to that helper and counters
for committed, planned-preview and rectangular-preview tile counts. It changes
no rendering algorithms. An optimized/profileable APK was run in the primary
`art.capycanvas` app on Wacom `5ll21u1002931`, with the same 9504x6336 photo,
2048 px opaque G-Pen, 17.164% zoom, pressure-1 200 Hz fast stylus replay and
hover/stroke/undo/redo workflow. All actions are retained, trace errors are zero,
thermal status is 0 before/after, and upload drains do not increase.

| Census measurement | Result |
| --- | ---: |
| Visible drawing updates/s | 33.09 |
| Drawing callbacks | 332 |
| Scheduled CPU/callback | 24.525 ms |
| Mean observed GPU interval | 20.044 ms |
| Previously instrumented binding construction | 2.969 ms/callback |
| Newly timed material-input construction | 2.350 ms/callback |
| Combined observed binding construction | **5.319 ms/callback** |
| Combined counted binding constructions | **593.20/callback** |

The two binding scopes are disjoint; both are nested in callback CPU time.
This captures the dominant material/scene binding classes, not necessarily all
raw-device resource helpers. It is not a before/after optimization benchmark.
Its 33.09/s is close to the prior same-algorithm full-probe 33.29/s; no speedup
or regression is inferred from that difference.

| Binding role | Calls/callback | CPU ms/callback |
| --- | ---: | ---: |
| Committed material input | 81.08 | 1.081 |
| Committed output | 81.08 | 0.785 |
| Preview material input | 109.41 | 1.269 |
| Preview output | 109.41 | 0.846 |
| Composition and its preparation | 212.23 | 1.338 |

## 1. Replace duplicate execution loops with one

`encode_material_batch`, `encode_preview_material_batch`, and
`encode_preview_material_from_persistent` repeat resource lookup, binding
construction, scissor/attachment choice, dispatch setup and generation handling.
`BrushEncodingContext`, `BrushEncodingTarget`, and `BrushPassPlan` already encode
most of the distinctions. Extend those existing concepts rather than adding a
parallel scheduler.

Use one sequence:

1. Resolve the sparse tile and immutable input generation once.
2. Resolve output color/state channels and whether a full-page write replaces
   initialization/copies.
3. Encode the resolved tile work using the current compute or fragment emitter.
4. Publish the output generation, or expose it as disposable prediction.

Persistent versus predicted output becomes resource selection. It should not
duplicate the execution loop. Keep real semantic differences explicit:
nonlocal brushes need stable neighborhoods; watercolor keeps its snapshot and
transport stages; the qualified fragment fallback for non-normal blends stays.
Consolidation must not reinstate copies that the current direct prediction path
already removed. Resolve indices/handles once instead of repeated linear page
searches in each stage.

**Speedup credited to refactoring alone: zero.** Its purpose is to delete
duplication and make each following improvement apply in one place.

## 2. Retain bindings with live resources

`PageSurface` already owns a reusable texture binding. Extend this ownership
pattern rather than adding a global LRU which pins evicted GPU resources.

- Make the dry metadata record offset dynamic. Its current fixed buffer offset
  makes tile-list position part of every newly constructed input binding.
  Changing record contents should not require a new binding.
- Retain input variants for the two color/coverage generations and the initial
  zero-coverage case; retain output variants with the output surfaces.
- Keep global dabs/styles/metadata buffers separate from changing contents.
  Resource replacement or buffer growth invalidates the dependent binding set.
- For the ordinary unmasked scene, retain each source's default-mask binding
  with its page or decoded slot. Reuse the existing paired-input machinery for
  masks/effects, with bounded ownership. Do not simply remove cache clearing
  and leave unbounded references in `Scene::source_bindings`.

The accounting below is for the measured normal, uniform, unmasked workload.
It is not a claim that arbitrary changing masks/colors have the same reuse.

### Resource-count derivation

The capture reaches 555 paint pages, 132 preview surfaces and an admitted
256-slot decoded-source cache. With stable shared buffers, the required binding
variants for this workload are bounded by:

| Resource class | Conservative number of initial constructions |
| --- | ---: |
| Material inputs: zero coverage + two generations, plus empty input | 3x555+1 = 1666 |
| Committed outputs: two generations | 2x555 = 1110 |
| Preview outputs: one per surface | 132 |
| Scene inputs: paint generations + preview + decoded slots + empty | 1499 |
| Total | **4407** |

The current capture constructs approximately 196,944 bindings in these observed
classes. Reusing variants converts repeated per-update creation into roughly
2.2% as many initial creations under the stated assumptions. This is a resource
model, not a measured cache-hit ratio. Buffer growth, eviction of live surfaces,
changing style resources and more complex masks invalidate the bound and must
be counted in the prototype.

Applying each class's observed mean construction cost to these initial counts
costs **0.131 ms/callback**, amortized over the capture. Therefore:

```text
CPU after reuse = 24.525 - 5.319 + 0.131 + new lookup/retirement cost
                = 19.336 ms + new lookup/retirement cost
```

This deliberately gives no credit for fewer descriptor frees, less allocator
pressure, fewer repeated page searches or faster command finalization. It also
assumes the measured mean construction cost transfers to initial constructions;
cold-driver behavior may differ. New lookup/retirement work must stay below
**0.707 ms/update** to keep CPU below the existing 20.044 ms GPU interval.
That is a concrete implementation acceptance budget, not an assumed lookup cost.

## 3. Use the existing sparse plan for preview

Committed paint consumes `BrushTile` records. Direct prediction instead walks
`page_coordinates(damage)` and looks up whether each rectangle tile has a
contact; even tiles without contacts get full color writes. Meanwhile composition
already tracks sparse old/new preview damage. This is duplicate planning and
unnecessary work, rather than a need for larger tiles.

The new census counts **36,325 rectangular preview tiles versus 29,702 planned
tiles** during drawing: 109.41 versus 89.46/update. Use the same sparse list for
preview allocation, execution and selection. Compose the union of committed
damage, retired preview tiles and new preview tiles. Readers must test current
preview membership/generation; an old pooled page must not override committed
pixels merely because its coordinate lies in a bounding rectangle.

This removes **18.23% of preview tile executions**, 19.95/update. The color-only
logical read/write saving is **41.84 MB/update** (256² x 32 bytes x 19.95).
It may also remove coverage reads, depending on shader dead-code elimination.
These are logical bytes, not external DRAM transactions. Remaining contact
evaluation is unchanged, so 18.23% fewer tiles does not imply an 18.23% faster
prediction interval.

Without binding reuse this would also avoid approximately 40 binding creations
per update. With reuse, those are already avoided: do not count that CPU saving
twice. Do not assign a second independent percentage speedup to this stage.

## 4. Remove the accidental coupling to source-slot count

Both dry paths flush compute jobs every `SOURCE_SLOTS` (16) pages. Source-slot
reuse is a real hazard for borrowed original pixels. Independently owned paint
pages have a different lifetime. In this workload the active paint layer is
separate and initially empty; its committed sources are owned pages and its
absent preview sources are transparent.

The recorded tile counts imply **4263** compute passes at the current 16-page
limit, versus **650** nonempty brush batches. With stable inputs, a common
executor can keep one pass open per brush batch while retaining separate
per-tile dispatches. No texture-array allocator is required. If preparation
writes/evicts a borrowed source slot, flush before that hazard; do not remove
the correctness boundary for source-backed/nonlocal operations.

The installed wgpu-core creates a pass command buffer plus a transitions
prepass for a compute pass (`command/compute.rs` and
`InnerCommandEncoder::close_and_swap`). Thus the modeled removal of 10.88
passes/update eliminates about **21.77 native command buffers/update**, before
other effects. This addresses only part of the roughly 123 allocation API calls
in the prior capture. Allocation/free/recording costs are not uniformly
proportional to buffer count, so do not multiply all 6.97 ms finalization by
that fraction and call the result measured savings.

Sparse planning alone reduces the 16-page pass count to 3856. Its pass reduction
is included in the 4263-to-650 figure; these savings must not be summed again.

## FPS estimate and its limits

CPU and GPU overlap. A transparent calibrated model is:

```text
interval = max(CPU, GPU interval) + R
R = 1000/33.092 - max(24.525, 20.044) = 5.694 ms
```

`R` collects scheduling/presentation/other effects in this capture. It is not
a proven irreducible constant. Keeping it and GPU time unchanged gives:

| Binding-cost removal scenario | Modeled FPS |
| --- | ---: |
| None (observed census) | 33.09 |
| Half of observed binding cost | 36.29 |
| Three quarters | 38.12 |
| Resource-count model, if new bookkeeping stays below 0.707 ms | **38.85** |

The first two improvement rows are sensitivity cases, not guessed hit rates or
confidence intervals. The resource model supports a conditional first target
of **about 39/s, +17%**, with approximately 5.2 ms CPU cost removed before new
bookkeeping. The sparse-preview and pass simplifications may improve this, but
their remaining GPU/driver savings are not yet measured. Under unchanged GPU
intervals, even perfect scheduling would cap this model at **49.9/s**. A 2x or
60 FPS forecast is not justified by binding reuse alone. These probe-derived
values must not be mixed directly with the 35.54/s light-probe median.

## Brush-model simplification permitted by artistic intent

If CPU cleanup exposes brush-state traffic as the next limit, replace the
uniform dry-ink representation rather than adding a second G-Pen renderer.
For a constant ink color C, background premultiplied pixel B and maximum
stroke coverage A, the current uniform source-over recurrence reduces to:

```text
A = max(contact coverage accumulated during this stroke)
pixel = (1-A)*B + A*(C.rgb, 1)
```

This follows from the current incremental alpha `(Anew-Aold)/(1-Aold)`.
It preserves uniform overlapping ink mathematically for constant color and
normal blending, allowing rounding/coverage-shape differences to be judged
artistically. Erasing uses `pixel=(1-A)*B`. Color changes, alpha locking and
non-normal blends need their own defined semantics; the formula must not be
applied blindly to them. Simply changing Uniform to Flow makes overlaps build
opacity and is not an equivalent simplification of ink intent.

Store one scalar coverage field for ordinary uniform ink while drawing, and
have committed/predicted rendering use the same coverage evaluator and existing
masked-color composition concept. Replace redundant persistent/predicted RGBA
updates for this brush family; do not retain old/new algorithm toggles. R32F
coverage read/write is 8 logical bytes/pixel versus 40 for color plus coverage,
an **80% brush-state traffic reduction** for that operation. Composition must
now read the coverage and stroke-start base, and completed color must be
materialized for consumers/history. Those costs prevent an 80% FPS claim.

Only pursue this representation if it removes more update/generation machinery
than it adds. Use the existing active-stroke lifecycle; do not introduce an
unbounded deferred-stroke graph. A pen-up flattening stall, slower next stroke,
or delayed undo invalidates a drawing-only win. No numeric FPS gain is booked
for this larger change before measuring its complete lifecycle.

## Acceptance and evidence

Implement/review the common executor and ownership first; require removal of
the duplicate loops, unchanged tile/history sizes, and no extra preset-specific
renderer branches. Then measure binding counts and CPU, sparse-preview writes,
pass counts, GPU intervals and actual canvas latches. Qualify both large and
small brushes, pressure/taper/corners, translucent overlap, masks, cancellation,
undo/redo, pen-up/next-stroke latency and zoom transitions. Wet and nonlocal
materials must retain their required dependencies. Visual approval concerns
the intended brush appearance/feel, not old-pixel identity.

Raw APK, matching symbols, four-line patch, complete trace/profile, screenshots,
logs and analysis scripts are retained in
`artifacts/wacom-tile-simplification-20260920/`. The
[tracked calculation](measurements/android-pen-tile-simplification-20260920.json)
records the arithmetic and assumptions. Diagnostic source changes were removed
after building, and the verified production APK was restored on the Wacom.
No renderer optimization from this plan has been committed.
