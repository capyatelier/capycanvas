# GTK incremental proof investigation

2026-09-19. Source inspected: `cb5775699a0ddcf97d13fcd66e427c9fc3b4c739`.
Investigation only; no application implementation changed. The measurements
below exercise the current shared CPU algorithm, not a new renderer.

This is the pre-implementation baseline. The subsequent GTK implementation and
qualification are recorded in [GPU guide milestone](gtk-gpu-tone-guide-milestone.md).

**Assessment:** incremental integration is feasible and is the appropriate
direction for live drawing. The main missing integration is the local SDR
illumination analysis. Final SDR mapping and print-LUT evaluation already run
in the GPU presenter. The present GTK analysis path cannot update that
illumination at 120 Hz: it waits for idle, reconstructs the whole document in a
snapshot worker, reads every pixel back, and rebuilds the guide on the CPU.
Exact analysis also has dependencies beyond the touched tile. Achieving 4K120
requires GPU analysis with explicit dependency tracking and measurement; moving
the existing worker to a per-tile loop would not suffice.

**Why the appearance arrives late.**

The relevant sequence is in
[`LocalToneView`](../../apps/layer-linux/src/local_tone_view.rs):

1. Its key includes document identity, GPU owner, color, dimensions, background
   and composition snapshots of all layers. It excludes camera and SDR recipe
   controls. A changed raster revision therefore invalidates the analysis;
   adjusting the Proof dial reuses it.
2. On a key change, `tick` cancels any worker, clears `published`, and immediately
   sends `set_local_tone(None)` (lines 104–117). This removes the old guide.
3. The replacement waits at least 180 ms, with a 100 ms periodic timer, and
   requires `require_document_snapshot_idle()` (lines 138–155). That gate rejects
   active strokes and pending input through
   [`require_idle`](../../crates/layer-ui/src/session.rs). Removing the debounce
   alone would not produce live stroke analysis.
4. A fresh snapshot renderer is constructed on a worker using the canvas GPU
   device/queue. It waits for pending raster backing, recomposes the image in
   bounded regions, and maps those pixels to the CPU. Sharing the device does
   not reuse the live renderer's completed composition tiles. The full scan is
   repeated for each replacement worker; the snapshot-local guide cache starts
   empty. See
   [`SnapshotGpu::capture`](../../crates/layer-render-wgpu/src/snapshot.rs),
   [`local_tone_guide`](../../crates/layer-render-wgpu/src/snapshot/output.rs)
   and [`build_local_tone_guide`](../../crates/layer-color/src/output_rows.rs).
5. The finished CPU guide is packed, copied into a newly allocated GPU storage
   buffer, and triggers presentation. See
   [`set_local_tone_guide`](../../crates/layer-render-wgpu/src/present.rs).

During the gap,
[`local_tone_artwork`](../../crates/layer-render-wgpu/src/hdr_view.wgsl)
uses its point-color estimate when the guide is absent. Proof is still being
drawn, but without the spatial illumination treatment; completing the guide
changes its appearance. Once a guide exists, new stroke pixels can use it in
the presenter immediately, but it does not incorporate the active stroke's
new illumination. Committed artwork changes restart the process above.
Repeated edits can cancel work and postpone completion further. An already
running cancelled worker must also finish before another starts.

This explains the reported delayed settling from code, rather than locating a
fixed one- or two-second timer. The exact contributions on the user's document
have not been captured in a GTK trace. The only explicit 500 ms cadence is for
animated imagery; that path retains its previous guide, unlike artwork-key
invalidation.

**What the algorithm actually computes.**

[`LocalToneBuilder`](../../crates/layer-core/src/color/hdr/local.rs) performs:

- A full-resolution, coverage-weighted area reduction of **log luminance** to a
  document-space guide whose longest edge is at most 768. It uses the working
  color space's luminance weights, unassociates positive alpha, ignores hidden
  RGB, and floors luminance at −24 stops for the logarithm.
- Coverage-normalized Gaussian pyramids with a separable five-tap kernel,
  descending to 1×1. For each intensity anchor, it builds a remapped pyramid
  and accumulates interpolated Laplacian detail. Anchors span the guide's
  occupied minimum/maximum with spacing no greater than half a stop. Black
  plus bright covered pixels can therefore require many anchors.
- Reconstruction of illumination as original log luminance minus reconstructed
  detail. Current analysis streams one remapped pyramid at a time, keeping
  memory bounded rather than retaining every anchor's pyramid.

The presenter gathers four guide samples with coverage and luminance-edge
weights, then applies the current fixed-baseline SDR shoulder, macro/micro
contrast, brightness and gamut policy to the sampled artwork color.
The pointwise equations are in
[`hdr_mapping.wgsl`](../../crates/layer-render-wgpu/src/hdr_mapping.wgsl).
Native HDR viewing bypasses this SDR stage when display headroom permits.

Print adds an image-independent ICC viewing LUT after HDR-to-SDR preparation.
[`ProofLut::build`](../../crates/layer-color/src/icc/proof/view_lut.rs) tries
65³, then 129³ grids with quality checks; the shader uses four-corner
tetrahedral interpolation. GTK's
[`ProofView`](../../apps/layer-linux/src/proof_view.rs) keys this cache by space
and recipe, not artwork pixels. Drawing should not regenerate that LUT. HDR
print proof shares the same delayed local SDR guide, so improving the guide
also helps that case. Initial print-profile preparation is a separate cost.

**Measurements from this investigation.**

An isolated release harness called the current `layer-core` implementation,
separating `LocalToneBuilder::push` from `finish`. Input rows were prepared
before timing. Three sequential repetitions ran on an AMD Ryzen Threadripper
PRO 9995WX; no clock or affinity controls were applied. All cases are synthetic,
opaque, grayscale images. The gradient spans −6 to +6 stops; the black/HDR
fixture combines covered black with a −2 to +6 stop gradient. These timings
exclude GTK, snapshot construction/composition, readback, debounce and upload.

| Document / content | Anchors | CPU reduction, median | CPU pyramid/reconstruction, median | CPU total, median |
| --- | ---: | ---: | ---: | ---: |
| 2048×1024, HDR gradient | 25 | 29.6 ms | 51.8 ms | 81.3 ms |
| 3840×2160, uniform gray | 2 | 107.0 ms | 21.1 ms | 128.5 ms |
| 3840×2160, HDR gradient | 25 | 107.1 ms | 60.1 ms | 167.2 ms |
| 3840×2160, black + HDR | 61 | 107.7 ms | 118.3 ms | 226.7 ms |
| 8192×7324, black + HDR | 61 | 829.6 ms | 187.1 ms | 1017.4 ms |

Stage medians are independent. The three 60 MP totals ranged from 790.2 to
1017.8 ms; these are diagnostic observations, not controlled performance gates.
The CPU work alone readily consumes a substantial fraction of the observed
delay, before the GTK/snapshot costs. Even uniform 4K analysis exceeds the
8.33 ms frame interval by more than an order of magnitude.

A second probe changed only an 8×8 patch in a 256×128 gradient, keeping the
original minimum and maximum. All 32,768 illumination cells changed by more
than `1e-5` log2 units. Outside a 32-pixel halo around the patch, 27,584 cells
changed; the maximum difference there was 0.00193 stops. Introducing a new
extremum produced a maximum outside-halo difference of 0.02912 stops. This
measures numerical dependence, not perceptual visibility, but disproves an
exact fixed-small-halo tile implementation of the current algorithm.

The harness, lockfile, environment, raw runs and summaries are retained in
`artifacts/color-m4/incremental-proof-investigation/` (ignored local artifacts).
The standalone harness can be rerun with:

```sh
cargo run --offline --release \
  --manifest-path artifacts/color-m4/incremental-proof-investigation/Cargo.toml \
  --target-dir target
```

**Where incremental rendering can help.**

The current renderer already has more suitable inputs than a project snapshot:
[`WgpuRasterizer::submit`](../../crates/layer-render-wgpu/src/lib.rs) tracks sparse
composition tiles, previous/new preview damage and a composite revision.
[`Scene::compose`](../../crates/layer-render-wgpu/src/scene.rs) and its
[`window path`](../../crates/layer-render-wgpu/src/scene/windows.rs) account for
effects and their support. Completed Float32 tiles feed
[`LiveDisplay::write_tile`](../../crates/layer-render-wgpu/src/live_display.rs)
and [`display_mips::write_tile`](../../crates/layer-render-wgpu/src/display_mips.rs).
Large documents use retained reduced levels and a visible-detail atlas;
integration must not require a new full-resolution document texture.

| Stage | Incremental treatment | Qualification needed |
| --- | --- | --- |
| Composite to guide statistics | Reduce changed completed artwork tiles on GPU; replace their retained contributions | Final effect-expanded damage, active previews, alpha, fractional guide-cell overlap, offscreen edits |
| Guide range and Gaussian pyramid | Retain statistics and propagate dirty regions upward with kernel support | Removing old extrema must work, including erasing and Undo |
| Remapped pyramids and illumination reconstruction | GPU processing of the bounded guide; later optimize dirty regions at each level | Coarse dependencies can spread through the whole image; changed range can invalidate every anchor |
| SDR mapping and ICC lookup | Keep the existing presenter stage using the current GPU guide/LUT | Matching frame generations and CPU/GPU numerical agreement |

The practical design is a retained GPU analysis resource alongside composition:

1. Observe final artwork output before display reduction and UI overlays.
   Add a GPU log-luminance/coverage reducer and retain tile contributions or
   recompute the complete affected guide-cell footprints. Shared guide cells
   can straddle tiles: simply overwriting each tile's cell values is incorrect.
   Repeated subtract/add updates also need a strategy for numerical drift.
2. Update that state from live composed pixels, including active stroke changes
   and removal of old prediction damage. Honor expanded filter damage, layer
   transforms and global invalidations. Expose actual output damage from the
   compositor, rather than reusing just the raw brush rectangle.
3. Run the current guide algorithm on GPU and retain its Float32 output there.
   A first prototype should measure a full bounded-guide GPU rebuild after
   incremental source reduction. This is simpler to validate and reveals
   whether more complex per-level invalidation is needed. Stream or batch
   anchors with bounded scratch; retaining every pyramid multiplies memory by
   the number of anchors.
4. Publish guide updates in queue order with the corresponding artwork
   generation. GTK owns surface lifetime, scheduling and recovery; shared Rust
   owns analysis, damage and color semantics. Canvas and Navigator use the
   same analysis generation. Snapshot export retains a complete captured-frame
   result and remains a numerical reference.

There are several constraints on that design:

- Existing RGB display mips cannot substitute for the guide input:
  `log(mean(Y))` differs from `mean(log(Y))`. They also use a different spatial
  grid. Reuse the tile-processing opportunity, not the already-averaged colors.
- The current scene output can include a mask-area tint before
  `LiveDisplay::write_tile`. The analysis hook must precede that tint. Also cover
  dense-composite/direct-write shortcuts. The existing
  [`artwork::Capture`](../../crates/layer-render-wgpu/src/artwork.rs) demonstrates
  exact overlay-free queries and active-preview handling, but repeatedly
  recapturing all tiles would retain unnecessary work.
- The pyramid reaches global scales, and anchor positions are derived from
  global extrema. A new minimum/maximum can shift all anchor positions.
  Fixed anchors or truncated pyramid depth could simplify updates but would
  change the algorithm and require an appearance decision and export parity.
- Caching tone-mapped document tiles before downsampling changes the current
  sampling order. The presenter currently samples linear artwork **then**
  applies the nonlinear proof. `mean(proof(pixel))` is generally different from
  `proof(mean(pixel))`. Keeping final proof in the presenter avoids this change.
- Ordinary HDR viewing currently schedules local analysis for any floating-point
  document, even when its presenter is bypassing SDR. Demand-based scheduling
  is a possible additional saving, accounting for SDR outputs, print, Navigator
  and display changes.

**4K120 assessment and next investigation gate.**

A 3840×2160 surface at 120 Hz requires approximately one billion output pixels
per second and an 8.33 ms frame interval for the complete drawing workload.
Tile-based analysis reduces source work, but does not eliminate final viewport
shading or the guide's coarse-scale dependencies.

There is encouraging existing GTK evidence for the final transform:
the [integrated qualification report](color-management-m4-gtk-qualification.md)
records a 120-second run on a 4K/120 Hz desktop, with 60 MP artwork and twenty
effects, worker GPU p99 of 0.621 ms and 0.146% missed slots. However, that test
changes the Proof dial and explicitly requires unchanged artwork and the same
guide throughout. It does **not** measure guide updates during drawing, nor
establish the cost of a full-screen 4K paint workload on other GPUs. Its
request-to-present p99 of 14.058 ms is a latency measurement, distinct from the
8.33 ms throughput interval.

Retaining the previous guide during ordinary artwork reanalysis would remove
the immediate fallback-to-local appearance switch. It is a useful first UX
mitigation, but remains an approximate/stale preview until replacement. Existing
synthetic [animation measurements](color-management-m4-gtk-qualification.md)
already show that stale-guide publication can make sizable tonal jumps. Guide
retention alone must not be described as exact incremental proof or 120 Hz
analysis.

Before claiming 4K120, instrument and compare Proof off / SDR / Print while
actually drawing into HDR documents. Extend the existing GTK pen-up/stroke
and Wayland presentation harnesses with artwork and guide generations and
timestamps for invalidation, worker wait, source reduction, pyramid analysis,
publication and first presentation using the new guide. Record input-to-present
and input-to-current-guide separately, plus GPU stage p95/p99, missed refresh
slots and memory. Include continuous and rapid short strokes, erase/Undo,
transparent boundaries, black/bright marks introducing or removing extrema,
large filters, pan/zoom and device replacement. Compare incremental guides to
fresh CPU analysis, especially across tile boundaries and partial edge cells.

The recommended order is: measure and mitigate GTK guide eviction; prototype
GPU reduction from completed artwork tiles plus GPU guide construction; then
add per-level incremental analysis if the measured total needs it. This
investigation establishes a concrete path and identifies the current blocker;
it does not establish a 4K120 guarantee for freshly recomputed proof.
