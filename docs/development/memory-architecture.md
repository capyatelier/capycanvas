# Memory architecture plan

[Developer guide](README.md) · [Rendering and composition](../internals/rendering.md) · [Region and scale composition](../rendering/display-composition.md) · [Performance targets](../PERFORMANCE_TARGETS.md)

**Status: proposed.** Milestone 1 fixes the unbounded GPU growth behind issue #24
and the slowest tab switches, and should land first. Later milestones change
where authored pixels live.

## Goal

A drawing of any size and layer count opens, paints, pans and zooms at its tier's
frame rate on a device whose memory it fits on disk. RAM and VRAM hold what the
current view and the current edit need. The document and its history live on
local disk, and every other copy is a cache that can be dropped.

These rules define the end state:

- **Resident memory follows the screen.** GPU memory and the RAM the renderer
  needs are a bounded number of window-sized images plus the area being edited,
  not a function of layer count, photo size or history depth.
- **No frame waits.** Input, presentation and composition never wait on disk,
  decoding, upload or a budget decision. Missing detail shows the finest level
  that is resident, down to a root overview that always is.
- **Edits start promptly.** A presentable frame is not an editable layer. The
  tiles a stroke, transform or filter needs are prepared ahead of every
  speculative job, and the first dab after a layer, tool or view change has its
  own latency gate.
- **One copy of record.** Each authored tile has exactly one durable copy, in the
  session store or in the saved package. Every other copy can be dropped at any
  time without loss.
- **One budget per process.** A budget manager owns every cache's share of RAM
  and VRAM for every window and document in the process, from the operating
  system's and driver's live figures and the painter's memory limit. Caches don't
  size themselves from constants or from their own share of headroom.
- **Use RAM eagerly, never greedily.** Within the budget, keep every copy that
  will save work later; never keep a copy that saves nothing. The budget is a
  ceiling, not a target.
- **Small display errors are acceptable.** Display data uses half floats; only
  data that half floats would get badly wrong, and the tiles being edited, use
  RGBA32F.
- **Tab switches are immediate.** A recently used drawing appears on the next
  frame and takes a stroke at once; a switch never waits for disk writes,
  captures or a renderer rebuild.
- **One implementation.** Discrete and unified memory differ in parameters
  chosen at startup, not in separate code paths.

## What fails today

| Area | Today | Effect |
| --- | --- | --- |
| Scene scratch | `Scene::reserve_format` appends 1 MiB RGBA32F pages with no limit. Display-graph evaluation reserves before fallible steps (`objects` in `crates/layer-render-wgpu/src/scene/scale.rs`, `combine` in `crates/layer-render-wgpu/src/scene/scale/graph.rs`), and deferred recovery truncates jobs without releasing them. | Pages leak while object work defers: several GB on Linux and Windows, growing with no input. |
| Source and mip budget | Image mips and decoded source slots share one pool of up to 1 GiB; mips are admitted first. | Mips can leave about 20 source slots, so canonical work keeps deferring and the 100 % view never finishes. |
| Presentation while deferred | `presentation_views` returns nothing when object work defers and no display backup exists. | The view keeps an old frame. |
| Budgets | Each cache takes a fixed size (decoded tiles 512 MiB, display 608 MiB, object results 64 MiB × 2, history 512 MiB) or a quarter of the headroom reported when it was admitted. | Caches don't see each other; together they can exceed the device, or starve one another. Nothing reacts to memory pressure. |
| Imported photos | A `SourceImage` keeps every LZ4 tile in RAM. LZ4 doesn't compress RGB photo tiles. A fixed 512 MiB per-document asset limit refuses further imports. | Two RAM copies of each photo (tiles plus decoded cache), and a refused 15th 12 MP photo on a 64 GB machine. |
| Opening a file | `ResourceReader` materialises every tile into RAM and transcodes deflate tiles to LZ4. | RAM grows with document size on open. |
| Spill | `raster_storage::Bytes` supports `Disk` and `External` backings, but only parked tabs use them. `Disk` reads seek synchronously on the decode worker, with no read cache. | The open document can't use the disk. |
| Recovery | `SessionStore` writes every tile to disk; live tiles never point at those files. | The bytes are on disk already, but RAM keeps the only copy the editor uses. |
| GPU precision | Paint pages, source slots, mips, scratch and display levels are RGBA32F. | 16 bytes per pixel where the document stores 3–8. |
| Image mips | Built from every level-0 tile of every visible image, each session, regardless of the view. | A 61 MP photo keeps all 942 source tiles resident on an RTX PRO 6000 (Linux, 2026-10-31). |
| Complete hierarchy | An optional full-document RGBA32F pyramid including level 0, kept even when image objects make it unusable. | A blank 61 MP document holds 1.22 GB of GPU memory on an RTX PRO 6000 (Linux, 2026-10-31). |
| Scheduling | One first-in, first-out decode pool (up to three workers, eighteen jobs) for every window; no viewport priority, no motion prediction, partial cancellation. | Work for views the painter left competes with the current one. |
| Parked tabs | Inactive documents spill to disk once together they pass a fixed 64 MiB, whatever RAM is free. | Switching back to a tab reads from disk even on a machine with RAM to spare. |
| Tab switches | Every host drops the outgoing renderer and builds a fresh one for the incoming drawing; GTK also creates a new GPU device. Hosts wait for captures, recovery writes or spills first (see [Tab switches](#tab-switches)). | Each switch recompiles or re-prepares and recomposes the incoming drawing. |
| Windows | wgpu's D3D12 backend creates a 64 KiB sampler-index buffer for every bind group with samplers (`vendor/wgpu-hal/src/dx12/device.rs`). | Adds about 6 % to every scratch page and counts as committed RAM. |

What we keep: immutable, content-identified tiles shared by history, captures
and parked tabs; the display hierarchy (window, adjacent level, overview); the
expression graph with cached branches; per-frame tap and page limits; idle-only
exact refinement; the `Disk`/`External` backings; OPFS storage on the web.

## Terms

| Term | Meaning |
| --- | --- |
| Tile | A 256 × 256 block of one plane of one source at one level. |
| Level | A power-of-two reduction; level ℓ has 1/2^ℓ of the pixels per axis. |
| Window | The page-aligned region of a level the current view needs, plus a margin. |
| Stream | A sequence of tiles that composition reads for new screen area: one layer, or one cached composite. |
| Pool | A physical memory the budget manager allocates from: VRAM and RAM on discrete GPUs, one shared pool on unified devices. |
| Pivot | The edit target with cached composites of what lies below and above it. |

## Data classes

| Class | Examples | Copy of record | May be dropped |
| --- | --- | --- | --- |
| Authored | Photo tiles, committed paint, coverage, wetness, history versions | Session store or saved package | Every in-memory copy |
| Derived | Image mips, reduced source levels, composite pages, Navigator image | None; rebuilt from authored data | Always; some are cached on disk |
| Working | Edited tiles in RGBA32F, brush material state, stroke coverage | The stroke until it commits | No, while the stroke or transform is open |
| Transient | Decode buffers, upload ring, readback, scratch pages | None | After the work that holds them completes |

## Residency classes

Every derived and authored copy in RAM or VRAM belongs to one class. Eviction
takes from the lowest class first and, within it, by the lowest
`recent use × refetch cost ÷ bytes`.

| Class | What | Where | Leaves when |
| --- | --- | --- | --- |
| Pinned | Tiles the current frame reads; working tiles; tiles leased by encoded commands; the root overview; dirty authored data: captures not yet published and tiles not yet durable | VRAM or RAM | Their commands complete, the stroke commits, or the data is durable |
| Edit-ready | The dependencies of pending contacts and operations; then a bounded neighbourhood of the edit target around the pointer, sized by what the current tool reads (brush radius, smudge and wet-paint support, filter halo) | VRAM, compressed copies in RAM | The target, tool or pointer moves on |
| Visible | Window pages at the presented level and the adjacent level; admitted pivot images | VRAM, with compressed copies in RAM | The view moves away |
| Ahead | Pages in the direction of motion; the next level during a pinch; tiles a queued operation will read | RAM, then VRAM as they near the view | The prediction changes |
| Warm | Recently visible pages, per-layer reduced windows and overviews, recent history versions, parked tabs and other windows' documents | RAM, compressed | RAM is over budget; authored copies go once their store copy is written |
| Cold | Everything else | Disk | Never; the store is the copy of record |

Only the root overview is guaranteed resident: one whole-document image of at
most about 1 MP, which guarantees a presentable frame. Per-layer overviews and
pivot images are admitted from the same allowance as the graph's branches, so
the resident bound is a fixed number of admitted images plus the edit-ready set,
not one image per layer.

**Edit readiness.** Edit-ready is bounded and ordered:

1. The dependencies of a pending contact or operation come first: the
   neighbourhood restoration already derives for brushes, materials, Liquify
   and retouching (`crates/layer-render-wgpu/src/raster/staging.rs`). Nothing
   optional joins that set, so no prewarm can delay a real contact.
2. Then a bounded neighbourhood around the pointer, sized by the tool's reach and
   capped in pages and bytes, prewarms when a layer, tool or view changes.
3. A wider prewarm of the target under the view is optional idle work, below any
   missing visible detail. At Fit that would be the whole document, so it is never
   required.

Readiness can be partial: a stroke starts as soon as its own neighbourhood is
prepared. Input stays ordered: as today, the engine holds a contact until its
restoration is prepared (`crates/layer-engine/src/canvas.rs`), so a cold edit
is late, never wrong. The gates measure that first-dab latency after a Navigator jump, a
layer change and a lazy open.

## Memory pools and budgets

### Sources

| Host | Budget queries | Pressure signals | Hard limit |
| --- | --- | --- | --- |
| Windows | `GlobalMemoryStatusEx` available physical memory and commit; DXGI `QueryVideoMemoryInfo` local and non-local budgets | `CreateMemoryResourceNotification` (low memory); `RegisterVideoMemoryBudgetChangeNotificationEvent` | Commit limit; job objects |
| Linux (GTK) | `MemAvailable`; the process's cgroup v2 `memory.max`, `memory.high`, `memory.current`; `VK_EXT_memory_budget` | `GMemoryMonitor` low-memory warnings; PSI triggers on the cgroup's `memory.pressure` | cgroup limit; OOM killer |
| Android | `ActivityManager.MemoryInfo` (`availMem`, `threshold`); Vulkan budget where present | `onTrimMemory`, which since API 34 reports only `UI_HIDDEN` and background levels | Low-memory killer |
| iPadOS | `os_proc_available_memory`; Metal `recommendedMaxWorkingSetSize`, `currentAllocatedSize` | `UIApplication` memory warnings; dispatch memory-pressure source | Jetsam limit |
| macOS | `host_statistics64`; Metal working set | Dispatch memory-pressure source | Swap and compressor |
| Web | `navigator.deviceMemory` (coarse, at most 8 GiB); none for the GPU | GPU `out-of-memory` error scopes; device lost | wasm32's 4 GiB; the tab |

Today only the queries exist, sampled once per admission
(`crates/layer-color/src/photo/memory.rs`,
`crates/layer-render-wgpu/src/display_memory.rs`). No signal is subscribed on
any host.

### Computing the budget

The budget manager keeps one ledger of every allocation it has granted, by pool
and class, so it never needs a cache to guess its share from headroom.

**RAM (discrete) or the whole pool (unified):**

```text
B_ram(t) = min( memory limit,
                process limit − pinned,
                max(floor, ours(t) + available(t) − reserve) )

memory limit = the painter's setting, or share × physical when it is Automatic
```

- `available(t)` is the OS figure above; `ours(t)` is what the ledger holds, so the
  budget measures headroom as if our caches were free.
- `reserve` keeps room for the system and other apps: max(10 % of physical,
  1 GiB) on desktops, max(15 %, 0.75 GiB) on tablets.
- `share` is the Automatic limit: 0.6 of physical memory on desktops, 0.45 on
  tablets. A painter's setting replaces it, higher or lower; the reserve still
  applies, so a high setting never makes the system kill Capy.
- `floor` is the smallest working budget, about 384 MiB: pinned and visible
  classes for one window.
- On unified devices GPU allocations count against this pool, and Metal's
  recommended working set caps the GPU part.

**VRAM (discrete):**

```text
B_vram(t) = 0.8 × driver budget(t) − fixed allocations
```

- The driver budget is DXGI's local budget or the device-local heap budget from
  `VK_EXT_memory_budget`. It already accounts for other processes. Without one
  (RADV disables the extension), use 0.5 × the local heap and rely on allocation
  failure handling.
- The 20 % margin matters on Windows, where going over the budget does not fail:
  WDDM moves allocations into shared system memory and GPU reads then cross PCIe.

**Web:** start from `0.25 × deviceMemory`, capped by wasm headroom, and a GPU
budget of 1 GiB on desktop user agents and 512 MiB otherwise. Halve the GPU
budget after an `out-of-memory` error scope; rebuild at half after a device loss.

**Change over time:** shrink at once when a pressure signal fires or available
memory falls below the reserve: trim the RAM caches to half and recompute.
Grow by at most 10 % every five seconds while available memory stays above the
reserve plus 10 %. Re-read driver budgets on Windows' change event, otherwise
about once a second off the frame path.

### Controls

Pool budgets, cache shares, spill, prefetch distance and undo depth are
automatic. Painters get two settings in shared Preferences
(`crates/layer-ui/src/settings.rs`), with their text in every Fluent catalog:

- **Memory limit:** Automatic (the default) or a size, from 1 GB up to the
  device's physical memory. It sets `memory limit` above. A painter with a slow
  drive raises it so more stays in RAM; one running other heavy applications
  lowers it. It applies to the RAM pool on discrete GPUs and to the whole pool on
  unified devices; VRAM always follows the driver's budget.
- **Scratch disk** (desktop only): where the session store lives. The default is
  the app's cache directory. Painters with a small system drive and a large
  second drive need to choose; Capy can't know which drive they want filled.

Low memory normally shows only as slower sharpening. Messages appear only when
new work can't be admitted (see below) or when the store can't grow: then Capy
keeps tiles in RAM and asks the painter to free space or choose another scratch
disk.

### When required memory runs out

Pinned data, including dirty authored tiles, can't be evicted. The ledger keeps
it as a bounded debt: pending captures, unpublished outputs and write-behind
each have a ceiling inside the budget, charged until they complete.

- **Debt always drains.** Under pressure only speculative work is deferred.
  Accepted captures, durability writes and renderer retirement keep bounded
  forward progress, within the resumed-input delay bound, so the debt shrinks
  and the next operation can be admitted.
- **New operations are refused, not failed.** When a new stroke, import or
  filter would exceed what is left after evicting every evictable byte, Capy
  keeps the drawing and its navigation working and refuses the operation with a
  message saying memory is full, instead of failing an allocation.
- **A stroke in progress always finishes.** Pen-down reserves the stroke's
  commit and cancel capacity and a continuation allowance for the pages it may
  reach next. When a growing stroke uses that allowance up, new dabs wait in
  order for captures to drain, as backpressure: presentation continues and no
  sample is dropped. If the shortage lasts (RAM exhausted and the disk full),
  the stroke commits what has been painted, nothing is lost, and the next stroke
  is refused.
- **Limit changes are gradual.** Lowering the memory limit or a pressure signal
  shrinks caches through bounded work after presentation, never inside a frame
  or a stroke.
- **Web without storage.** When OPFS is unavailable or over quota, the open
  drawing runs RAM-only under the same rules.

### Eager, not greedy

The budget is a ceiling. Within it, a copy stays in RAM or VRAM while it is
likely to save work, and leaves only when the budget needs the room:

- **Keep:** compressed authored tiles of every open document, including parked
  tabs and other windows, so nothing is read back from disk while RAM is free;
  recently visible pages and adjacent levels; recent history versions; derived
  data that costs more to rebuild than to hold (mips, composite pages, pivot
  branches). Write-behind still puts authored tiles in the session store for
  recovery, but the RAM copy stays.
- **Don't keep:** a decoded CPU copy of a tile that is already on the GPU, or
  any decoded copy once its upload completes; prefetch beyond the view's
  predicted motion; whole-document decoding or mip chains for layers no view
  needs; copies of tiles nothing references.

A machine with spare RAM therefore keeps a whole 28-photo document in memory
and never touches the disk; a tight one keeps the view and the edit and reads the
rest from the store. The fixed 64 MiB parked-tab budget goes away: parked tabs
are Warm, behind the active document.

### Windows and processes

One budget manager serves the whole process: every window, document, tab and
renderer draws from the same ledger and the same pools. The focused window comes
first; other visible windows keep their presented images and drop to Warm for
everything else; parked tabs follow.

| Host | Windows today | Change |
| --- | --- | --- |
| Linux | One `GtkApplication`; a second launch hands its files to the running instance | None |
| Windows | New Window opens in the same process; a second launch hands its files to the running instance (`apps/layer-windows/App.cpp`) | None |
| Android, iPadOS, macOS | One process for all windows and scenes | None |
| Web | Each tab is its own page, wasm instance and GPU device | None: each tab keeps its own budget, and the browser manages memory across tabs |

### Admission

- Importing or pasting checks free disk space for the tiles and RAM for the
  decoder's transient peak; it never compares against a per-document constant.
  The 512 MiB asset limit becomes a disk check.
- Decoders stream rows into tiles where the codec allows (JPEG, PNG, TIFF, WebP),
  so their peak is a band, not the whole image. Today the JPEG path decodes the
  whole image first.

## Session tile store

The session store is the open document's copy of record until it is saved.

- **Layout:** append-only pack files of 8 MiB chunks (`SPILL_CHUNK_BYTES`), indexed
  in memory by the tile's existing identity: about 40 bytes per tile, 0.2 MB for
  the 28-photo document.
- **Writing:** imports and commits append compressed tiles on a file worker. Once
  a write is flushed, the tile's `Bytes` records its `Disk` location and keeps its
  RAM copy while the budget allows, so dropping it later costs nothing. Recovery
  reads the store and its generation metadata instead of writing a second copy.
- **Reading:** an IO queue with the residency classes as priorities, cancellation
  by view generation, and a compressed read cache in RAM. Reads never run on the
  input, render or presentation thread. `Bytes::ready` reports a pending `Disk`
  read as not ready, like `External`.
- **Opening a saved file:** keep deflate tiles in the package and read them on
  demand, through an immutable lease on the package file that the document,
  its history and every job hold. Transcode a tile into the store's LZ4 when it
  is first read or at idle. Saving copies unchanged tiles straight from the
  leased original; an atomic save over the same path keeps the old file alive
  for its leases. Before a recovery generation refers to a tile, that tile is in
  the store, so recovery never depends on the original file. Package and store
  reads are both asynchronous; the transport's synchronous `poll` stays on
  workers.
- **Codec:** LZ4 for paint, which compresses well; a fast predictive codec for photo
  tiles, which LZ4 leaves at raw size. Both must decode at over 1 GB/s per tablet
  core, so decoding stays cheaper than an SSD read.
- **Durability order:** a tile stays Pinned until its pack write is flushed. A
  recovery generation's metadata and head are published only after every tile it
  names is durable, and the previous good generation stays until the new one is
  complete. A commit still only in RAM is covered by the next generation; that
  window is the recovery interval, as today.
- **Reclaiming space:** a record is dead only when no document, history step,
  capture, save, export, reader or current or previous durable generation refers
  to it. Compaction writes new packs, publishes the remapped index atomically and
  keeps the old packs until their readers finish.
- **Storage speed:** Capy assumes an SSD or tablet flash. On a slower drive the
  painter raises the memory limit, and the eager policy keeps more in RAM.
- **Web:** the existing OPFS store, extended from parked tabs to the open
  document, with tile-sized reads instead of whole-chunk loads.

## Derived cache

- **Image mips** are built per window from the tiles the view needs, at the
  document's depth, instead of from every level-0 tile. A complete chain is built
  at idle, nearest the view first, and written to an on-disk derived cache keyed
  by image identity, level and working colour space. A new session reads them
  instead of rebuilding.
- **Composite pages** replace the complete hierarchy. The display hierarchy keeps
  its window, adjacent level and overview; pages outside the window go into a
  sparse page cache (VRAM, then compressed in RAM), so panning back reads one
  stream. The all-or-nothing full-document RGBA32F pyramid is removed.
- The derived cache never enters `.capy` and is capped locally (2 GiB by default,
  oldest first).

## GPU formats

Small display errors are acceptable; display data uses half floats. The
exception is data half floats would get badly wrong, not slightly: effects run
in extended range even in 8- and 16-bit documents (`FX_EXTENDED` in
`crates/layer-render-wgpu/src/effects.rs`), Exposure spans ±126 EV, and float
paint can exceed half-float range. Raising Exposure by 20 stops and lowering it
again returns white through RGBA32F, but 0.06 through a half-float step between.
Marking only an effect and its consumers isn't enough: Exposure +24 EV above a
composite of near-black Multiply layers needs its input's tiny values, which a
half-float input would already have flushed to zero. So precision is propagated
both ways. An extended-range effect, a float source, everything downstream of
them, and the whole input ancestry of an extended-range effect keep RGBA32F.
Changing such an effect's gain invalidates its half-float caches.

| Data | Format |
| --- | --- |
| Level-0 view tiles of 8-bit documents | `Rgba8UnormSrgb` or `Rgba8Unorm`, as stored; the shader converts on read |
| Level-0 view tiles of 16-bit documents | `Rgba16Unorm` where supported, else `Rgba16Float` |
| Reduced levels, mips, composites, pivot and operator images, scratch | `Rgba16Float`, premultiplied, in the blend space |
| Extended effects, float sources, their downstream nodes and the input ancestry of extended effects | `Rgba32Float` |
| Working tiles of the edit target; brush material; exact evaluation | `Rgba32Float`, as today |
| Presentation copies | The existing guarded half-float conversion (`crates/layer-render-wgpu/src/scene/scale/half.wgsl`) |

Reduced levels average premultiplied values, so they can't be stored straight.
Exact queries, export and idle refinement read authored tiles, so display
precision never reaches a file. Editing restores working RGBA32F tiles from
authored tiles too, never from display data; that conversion is charged to the
work arbiter like any upload. Shared recipes and caches are keyed by working
colour space, depth, blend space and HDR encoding as well as by device.

## Composition

### Today

`stack::compose` builds one expression for exact and display composition.
Unclipped Normal runs become balanced `over` trees; masks, opacity, other blend
modes, clipping and effects are expression boundaries. The display graph caches
branch images within the display allowance, so painting one Normal layer among
32 at reduced zoom needs about log2(32) blends
(`cached_branches_recompose_logarithmic_work_and_preserve_untouched_regions` in
`crates/layer-render-wgpu/src/scene/scale/tests.rs`).
Non-Normal boundaries chain, so a target under k boundaries recomposes at least
k nodes.
Native evaluation and exact idle refinement get no branch budget and walk every
layer for each repaired page. Each source also keeps its own reduced window. The
vector split keeps cached images above and below a moving selection, and rejects
backdrop-dependent blends.

### End state: pivot on the edit target

When a layer becomes the edit target (a stroke, transform or filter on it, or
the selection of a painting tool), the existing graph reassociates the
target's enclosing run around it into **below**, **target** and **above**, and
gives those branches cache priority. This is the same graph and the same branch
cache, not a second cache: ownership of valid branch pages transfers, nothing is
duplicated, and pages that aren't rebuilt yet render through the current
expression.

- **Below** is the backdrop the target blends onto: the composite of everything
  beneath it in its isolation context. Any blend mode is allowed; it is only the
  result.
- **Above** is split into segments. A segment of layers whose blend is *proved*
  affine in its backdrop collapses into a cached operator; the rest stay live.

Writing the premultiplied composite as `co = a·cd + b·αd + c` and
`αo = (1 − αs)·αd + αs`, a layer is affine in its backdrop when its blend
function is affine in the backdrop colour for each source pixel, over the
backdrop values that can actually reach it:

| Affine for any backdrop | Affine only with a domain proof | Not affine: live |
| --- | --- | --- |
| Normal, Multiply | Hard Light and Exclusion: they clamp the backdrop to [0, 1], and an extended-range effect below can exceed it | Overlay, Soft Light (piecewise in the backdrop) |
| Screen in 8- and 16-bit documents (unclamped `s + d − s·d`) | Divide in float documents: affine, but coefficients grow as the source nears zero | Darken, Lighten, Pin Light, Difference |
| Add in float documents | | Color Burn, Color Dodge, Vivid Light; Divide in 8- and 16-bit documents |
| Any of these with a layer mask and opacity | | Linear Burn, Linear Light, Subtract, Hard Mix, Add in 8- and 16-bit documents, Screen in float documents (clamped) |
| | | Hue, Saturation, Color, Luminosity (non-separable); adjustments; spatial filters |

The algebra holds in either blend space, because the perceptual composite stores
encoded premultiplied values and the blend formulas act on those. A domain proof
comes from the expression: if nothing between the document's sources and the
segment can leave [0, 1], the clamped modes join the affine column.

Composed affine maps stay affine, so each proved segment collapses into one
operator:

- **Normal only:** one premultiplied image (`b = 0`, `a = 1 − αs`), which is the
  existing `over` associativity.
- **Mixed, over a proved opaque backdrop:** a multiply image and an add image,
  `co = A ⊙ cd + C`. Opacity is proved per segment: an opaque Paper or Fill at
  the bottom of the same isolation context, with no alpha-changing step between.
  Paper keeps its place in the expression whenever a backdrop-dependent layer
  sits above it (`crates/layer-core/src/paper.rs`). Inside an isolated group,
  or after a filter that changes alpha, the backdrop isn't opaque.
- **Mixed, otherwise:** three images, adding the `b·αd` term.

Operator images use half floats like other display data, and RGBA32F when the
segment is extended.

**What stays live, and what it costs.** Live layers above the target are
evaluated every frame over the damaged region. That includes their passes,
not only their reads: a masked Pass Through fade (difference, mask, sum), a
clipped member composing onto its base before the base joins the outer stack,
an attached effect after its owner's mask, and a spatial filter with its halo and
its own intermediate targets. The order owner mask → attached effects → clipping
→ base blend is preserved exactly. A transform whose dependencies can't be
expressed as a static below and above (attached spatial effects on the target,
clipping members) uses the current expression instead, as the vector split
does today.

**Per-frame cost while painting**, per damaged texel, for a proved segment chain
in one fused pass:

```text
reads  = 1 (below) + target + operator images (1–3 per segment) + live layers
writes = 1 root page, plus the intermediate targets of live multi-pass steps
```

A 32-layer Normal stack goes from about ten reads and five intermediate writes
with balanced branches to three reads and one write. That model covers eligible
segments only; masked groups, spatial filters and clipping add their own passes
and must be measured, not extrapolated.

**Memory.** Pivot images are admitted from the graph's branch allowance like any
other branch: window-sized, at most 131 MB each in RGBA16F at 2560 × 1600 just
before a level switch. An isolated group holding the target adds a below and an
above per nesting level, and each operator segment adds its images; whatever
doesn't fit falls back to the current expression. Per-source reduced windows of
other layers move to Warm only once no fallback reads them.

**Building a pivot.** Reuse valid branch pages; build the rest in page batches
nearest the pointer first, inside the frame's work budget. A newer target
cancels an unfinished build. Fallback inputs stay resident until their
replacement pages are usable, so a fast run of layer switches never starts a
fresh decode for the fallback. O(log N) construction applies to eligible Normal
subtrees with valid branches; cold, masked or mixed stacks build from leaves.

**Rollout.** Normal-only reassociation and cache priority come first. Mixed
operators are admitted only where measurements on Mali and Adreno show that
build cost, reuse and bytes moved beat the current graph, not read counts alone.

**Exactness.** Pivot images and operators are display data. Exact queries,
export and idle refinement evaluate the full stack natively, as today.

### Navigation

Panning and zooming don't edit, so they read the root window, the adjacent
level, the overview and the composite page cache. New pages compose from all
contributing layers at the display level, which is O(N) per new texel: the same
as today, but budgeted per frame. Panning back reads cached pages, one stream.
On spilled documents the composite page cache is what keeps panning to one disk
stream.

## Frame paths

Each frame does at most what its budget allows; the rest waits for the next
frame or for idle.

**One work arbiter.** Today canonical object work (2²⁰ interactive / 2²³ idle
taps), mip preparation (2 or 32 steps), exact refinement (16 or 4 pages),
uploads and readbacks each keep their own allowance, and camera motion is not
"input" to the canonical pause. In the end state one arbiter charges every kind
of preparation, composition, upload, readback and retirement against a single
per-frame budget. It keeps a reserve for fresh input: after a pen-down or a
camera change, no further speculative work is submitted until that input's frame
is encoded. Work already submitted is bounded, so it delays resumed input by at
most the one extra frame the targets allow; a timestamp can't recall it. Costs
start from conservative per-page and per-tap estimates and adapt from GPU
timestamps where devices report them. The arbiter keeps today's guards:
refinement only while the camera, artwork and Navigator are unchanged, readback
yielding to presentation, and at most one extra frame of delay when input
resumes ([performance targets](../PERFORMANCE_TARGETS.md)). Priority order:
Pinned, Edit-ready, Visible, Ahead, then idle work.

| Motion | Per frame | Fallback when work isn't ready |
| --- | --- | --- |
| Pan | Copy window overlap; compose new pages from resident tiles within budget; schedule reads for the ring ahead | Overview or adjacent level, resampled |
| Pinch or level crossing | Present the old level resampled; compose new-level pages nearest the view centre | Old level, then overview |
| Navigator jump | Same as a level crossing, at the new position | Overview |
| Paint | Brush dabs into working tiles; recompose damaged root pages from the pivot | Balanced-branch graph until the pivot is built |
| Transform drag | Resample the target's pyramid over below and above composites | Coarser level of the target |
| Layer change | Present the root; prewarm the Edit-ready set, then build the new pivot | Root as it is |
| Tab switch | Present the incoming drawing's retained root; resume its scene | Its overview, then refinement as after a Navigator jump |
| Idle | Exact refinement, mip and pivot completion, derived-cache writes, write-behind | — |

**Rules for every frame:**

- No allocation larger than a page, no eviction of Pinned or Visible data, and no
  call that can block on IO or a lock held by a worker.
- Uploads per frame stay under the arbiter's transfer budget: about 1 ms of
  link time, 8 MB over a laptop's PCIe link. On unified devices the same budget
  counts decode, copy and conversion traffic in DRAM, which competes with the
  GPU's own bandwidth. Larger uploads are split across frames.
- Trimming and texture destruction happen after presentation, batched behind
  submission completion.
- Bind groups belong to long-lived pages, so pool reuse creates none, and the
  page count is bounded and trimmed. On D3D12 each retained bind group with
  samplers still owns a 64 KiB sampler-index buffer; removing those needs a
  sampler-free binding layout or a backend change, measured separately.

## Tab switches

A tab switch should feel like flipping a page: the other drawing appears on the
next frame and takes a stroke right away.

### Today

Every host rebuilds the incoming drawing's GPU state on every switch, and most
wait for slow work first:

| Host | Before the switch | At the switch |
| --- | --- | --- |
| Linux | Blocks input; waits until the outgoing drawing is idle, for every pending tile capture including undo and redo, and for the recovery writer (`apps/layer-linux/src/documents.rs`) | Stops and joins the outgoing render thread on the UI thread, creates a new render worker with a new GPU device, so every shader recipe, the decoded-tile cache, scene, mips and display levels start empty (`apps/layer-linux/src/canvas.rs`, `apps/layer-linux/src/render_thread.rs`); waits for parked-tab spill writes before input resumes |
| Windows | Waits until the outgoing drawing is idle; an unfinished spill refuses the switch | Keeps the device, drops the outgoing renderer and builds a fresh one on a worker (`crates/layer-host/src/window.rs`) |
| Android | Blocks input; waits for the recovery capture | As Windows, then waits for spill writes before input resumes |
| Apple | Waits for the recovery flush | Spills parked drawings, then drops and rebuilds the renderer |
| Web | Blocks input; waits for spill writes, then for every tile of the incoming drawing to load from OPFS (`apps/layer-web/app.js`) | Drops the renderer and builds a fresh one on the same device |

The incoming drawing then restores its rasters, builds mips from every level-0
tile and composes its display before its first artwork frame, and brush
readiness gates its first dab.

Measured on Linux (RTX PRO 6000, 2560 × 1600, 2026-11-01) between two drawings
of the same kind, from the switch request to the first presented artwork:

| Drawings | First switch | Later switches |
| --- | --- | --- |
| 4248 × 2832, ten painted layers | 10.5 s | 1.15–1.31 s |
| 4216 × 3016, eight photos and a painted layer | 10.6 s | 1.74–1.84 s |
| 9504 × 6336, one photo and a painted layer | 8.6 s | 1.37–2.23 s |

The first switch waits 6.6–8.6 s for the outgoing drawing's first recovery
checkpoint and about 0.3 s for spill writes. Every later switch spends 1.1–1.7 s
rebuilding the incoming drawing on a new device and renderer. Joining the old
render thread freezes the UI for 95–150 ms, up to 330 ms. In the eight-photo
drawing, object work never settles after a switch: the source cache churns
about 750 tiles a second, the same starvation as in milestone 1.

### End state

- **One device per process.** Every window and drawing shares one GPU device and
  one set of compiled recipes; the decoded-tile cache and other device-level
  caches live under the shared budget. The adapter is chosen once, compatible
  with every surface the process presents to; a window on a display that adapter
  can't present to gets its own device, with its own VRAM budget and recipes,
  under the same RAM ledger.
- **Parked drawings keep their scenes.** Switching away parks the renderer state
  with the drawing instead of dropping it: the presented root window, overview,
  branch images, mips and source windows stay resident as Warm, ordered by last
  use behind the active drawing. Under pressure their pages are evicted like any
  other Warm data; nothing is destroyed just because a tab changed. Parking
  cancels the drawing's speculative decode, mip and refinement work, so it can't
  delay the incoming drawing's first dab; its captures and recovery writes
  continue.
- **The switch waits for nothing slow.** It needs only the outgoing drawing's
  input to end, as today an open stroke commits or cancels, and its accepted
  captures to be owned: by the backing barrier while renderers are dropped
  (milestone 1), and by capture leases that survive with the parked renderer
  once renderers are kept (milestone 2). Recovery writes and spills keep running
  in the background, but the switch still captures an immutable snapshot of the
  departing drawing at once, so recovery observes every edit made before the
  switch; scheduled recovery also checks parked drawings with unsaved changes.
  Once the session store exists a checkpoint writes only metadata, because tiles
  are already on disk. No host awaits a disk write, a recovery flush or a tile
  load before showing the incoming drawing, and none joins a render thread or
  destroys GPU resources on the UI thread; retirement runs on a worker,
  independently of attaching the incoming renderer.
- **Background work yields to the active drawing.** Capture transfers,
  retirement, recovery writes and spills from every parked or retiring renderer
  share one device-wide priority with the active drawing's frames: they submit
  only while presentation and fresh input are served, with bounded transfer sizes
  and bounded CPU and IO concurrency. Spill and write-behind snapshot their
  payloads under a short lock and do their IO without holding it, so readiness
  and accounting never wait on a disk write.
- **Late completions stay with their drawing.** Every background job is fenced by
  its drawing and generation: a capture finishing after the painter switched
  back, undid, redid or closed the tab updates only the drawing it belongs to,
  and closing a tab or window hands its remaining jobs to a retirement owner
  that finishes or fails them under the existing failure rules.
- **The incoming drawing presents at once.** A warm drawing presents its retained
  root on the next frame with no composition. A drawing whose pages were evicted
  presents its overview, a ≤ 1 MP image per drawing kept in RAM like a tab
  thumbnail, and refines like a Navigator jump, Edit-ready first.
- **Input follows readiness, not the switch.** Input unblocks as soon as the
  incoming renderer is attached; a dab waits only for its own neighbourhood, as
  everywhere else. Shaders compiled for any drawing serve every drawing; a brush
  never used in this process still compiles once.

A parked scene costs at most a few window-sized images plus its caches; the
budget evicts the oldest parked scenes first, so ten open tabs on a tight device
behave like ten cold tabs, never like a failed switch.

## Discrete and unified memory

The architecture is shared. A `MemoryTopology` chosen at startup from the
existing probes supplies the parameters.

| | Discrete GPU | Unified memory |
| --- | --- | --- |
| Pools | VRAM and RAM, budgeted separately | One pool for CPU and GPU |
| Scarce resource | VRAM | The pool |
| Occupancy target | 80 % of the driver budget | The RAM formula, GPU included |
| RAM cache | Generous: compressed tiles re-upload in about 30 µs each | Small; never a decoded CPU copy of a tile already on the GPU |
| Upload | Decode into the mapped upload ring, copy over PCIe | Decode into the upload ring; the copy stays in DRAM |
| Spill trigger | RAM pressure | Any pressure |
| Over budget | Windows quietly demotes to shared memory; Linux drivers fail the allocation | The OS ends the app |

Classification: Vulkan `INTEGRATED_GPU` with a device-local, host-visible memory
type (as `crates/layer-render-wgpu/src/display_memory.rs` checks today), Metal
`hasUnifiedMemory`, and DXGI `UMA` from `D3D12_FEATURE_DATA_ARCHITECTURE`. The
web host is treated as unified, the safer assumption.

## Failure handling

- **GPU allocation fails:** drop Visible pages outside the current window and
  Warm data, lower the VRAM budget by a quarter, and continue with coarser levels.
- **Device lost:** every active and parked scene on that device is invalid;
  pending captures resolve under their existing failure rules. The focused
  drawing rebuilds first at half the budget; parked drawings rebuild only when
  shown, never all at once.
- **Disk full:** keep new tiles in RAM, stop write-behind, and show the scratch
  disk message. Never drop authored data.
- **Slow or busy disk:** the frame keeps presenting coarser levels while reads
  catch up; the painter can raise the memory limit.

## Milestones

Four milestones. Each ships alone, and so does each numbered step inside it,
with windowless tests and frame-rate, latency and memory rows against a matched
control on the tier devices.

1. **Stop the worst lag.** Fixes issue #24 and the slowest tab switches.
   1. Scratch slots become leases released on every return path, with a pool
      cap and idle trimming; source slots are reserved before mips; the coarser
      level stays on screen while object work defers; bind groups are owned by
      pages. Test: repeated deferred frames over many image layers keep the pool
      bounded and the view presentable.
   2. Background work yields to the active drawing: one device-wide submission
      priority for captures and transfers, aggregate admission for pending
      captures and retiring renderers, and spill IO outside the `Bytes` lock.
      Test: a stalled writer never blocks readiness or accounting; captures from
      another drawing never delay a pen-down past one frame.
   3. Tab switches stop waiting for recovery writes, spills and the outgoing
      render thread: the switch captures the departing snapshot, writes it in the
      background, and retires the renderer on a worker. The backing barrier
      stays. GTK keeps one GPU device and its recipes for the process. Tests: a
      switch with a recovery write and a spill pending presents the incoming
      drawing without waiting; an edit made just before a switch is restored
      after a crash once its background recovery write is durable.
2. **One budget and warm tabs.**
   1. The process-wide budget manager: ledger with pinned debt, pool budgets,
      host queries and pressure signals, the Memory limit setting, the
      admission refusal rule, and eager-not-greedy rules for today's caches. It
      replaces the fixed cache constants, the 512 MiB asset limit (a budget check
      until milestone 3) and the 64 MiB parked-tab budget. Tests: simulated
      budgets, limits and signals give the expected shares and trims across
      several windows; lowering the limit mid-stroke loses nothing and drops no
      frame; with the allowance full and the painter navigating, the debt
      drains and the next stroke is admitted; a growing stroke under pressure
      and disk full commits everything painted.
   2. One per-frame work arbiter with a fresh-input reserve, absorbing today's
      separate tap, step and page allowances. Test: queued speculative work
      never delays a pen-down past one frame.
   3. Warm tabs: parked drawings keep their renderer state under the budget,
      with capture leases replacing the backing barrier, parked speculative work
      cancelled, and late completions fenced by drawing and generation. Tests: a
      warm switch composes no pages and presents on the next frame; switching
      back mid-capture, undo, redo and closing either tab keep every edit.
   4. Half-float display data, with extended effects, float sources, their
      downstream nodes and the input ancestry of extended effects in RGBA32F.
      Test: display stays within a small tolerance of exact output; Exposure
      chains and float paint beyond half range stay exact.
3. **Drawings on disk, residency by view.**
   1. The session store on recovery's files, with durability order, generation
      roots and leases, write-behind, the IO queue and read cache, asynchronous
      `Disk` readiness, and the asset limit as a disk check. Tests: crash
      injection at every publication and compaction boundary; disk full while
      painting keeps every edit.
   2. Lazy package reads with package leases. Tests: overwrite the original,
      Save As, undo to an unread tile, and restart recovery after removing the
      original.
   3. Residency by view: classes including Edit-ready and Ahead, view-ordered
      reads with cancellation and motion prediction, windowed mips, overviews,
      the derived cache, and the composite page cache replacing the complete
      hierarchy. Tests: a drawing larger than the RAM budget opens, pans, paints,
      undoes, switches tabs and saves identically to an in-memory run; resident
      bytes for 1, 28 and 100 layers stay within the same bound.
   4. Streaming decoders, the predictive photo codec and deduplicated imports,
      each gated on its own measurements.
4. **Pivot composition.** Normal-only reassociation and branch-cache priority
   around the edit target inside the existing graph, with cancellation and
   fallback retention. Then, only where Mali and Adreno measurements show fewer
   bytes moved, proved affine operators and nested groups. Test: every blend mode
   stays within the display tolerance of the exact path, including clamped modes
   over extended backdrops; reads per damaged texel don't grow with layer count
   for eligible stacks.

## Performance gates

Add rows to each tier's table: pan, pinch and Navigator jump on the tier canvas
with 1, 8 and 28 layers, cold and warm; painting on the bottom, middle and top
layer of a 28-layer stack, at 100 % and reduced zoom; rapid layer switches while
painting; and a document twice the RAM budget, spilled. Each row must hold the
tier rate on frames where something moves, with the same refinement completion
limits as today.

Latency rows sit beside them: first dab after a layer change, a tool change, a
Navigator jump and a lazy open, cold and warm; and pan → stroke and pinch →
stroke, where resumed input may wait at most one extra frame.

Tab switch rows on every host: between two tier-canvas drawings with 10 and 28
layers, warm and after eviction, with a capture and a recovery write still
pending, and a stroke started right after the switch. A warm switch presents the
incoming drawing on the next frame and accepts a dab within two; a cold one
presents its overview on the next frame and meets the Navigator-jump latency
gates.

One combined stress row per tier: many open tabs, background captures and
writes, the memory limit under pressure, and immediate painting and navigation
in the active drawing. Motion holds the tier rate and resumed input waits at most
one extra frame.

## Open questions

- Whether affine operators earn their extra images on the low tier, or Normal-only
  collapse is enough there.
- How much of the decoded-tile cache survives once level-0 tiles live on the GPU
  at document depth.
- Whether the composite page cache should spill to disk, or RAM is enough.
