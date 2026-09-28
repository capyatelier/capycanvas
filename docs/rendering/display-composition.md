# Display-resolution composition prototype

Native presentation can compose eligible paint stacks at the resolution the view
needs. Authoritative paint remains in exact document tiles. Presentation pixels
never become paint, history, export, sampling, or project backing.

The implementation is in `crates/layer-render-wgpu/src/scene/scale.rs`. The
[TCL evaluation](../development/display-composition-20260927.md) records the
measured gains, controls and remaining limits.

## Ownership and execution

The view's largest singular value chooses a power-of-two texel footprint no
larger than a surface pixel, capped at 16 document pixels. At the benchmark's
7.14% zoom this is an 8 × 8 footprint. Admission bounds the reduced layer images
and two composition targets plus the adjacent output mip against the existing
display component budget.

Each visible paint layer has a premultiplied float image at this level and a
set of valid document-page coordinates. Source identity, immutable native tile
captures, committed brush damage and the previous prediction footprint determine
what must be refreshed. A changed exact page is averaged directly into its layer
image. Empty pages write zero without reading a full page. Source decoding uses
the existing bounded cache: each batch consumes its sources before their slots
can be reused. Composition then runs over the union of damaged regions, in layer
order, with layer opacity applied once. Unchanged layers reuse their pixels. A
small adjacent output mip is updated over the same damage for trilinear viewport
sampling; it avoids the fallback sixteen samples per screen pixel and is included
in cache accounting.

The cache replaces the full composite, live display pyramid/detail atlas,
transform static copies and obsolete scene image intermediates for its supported
stack. It does not keep these allocations alive underneath the new display.
Shader recipes belong to the device and survive cache or level replacement.
The original scene executor is the exact-query and unsupported-stack fallback;
`live_display.rs` is explicitly an exact-composition presentation fallback.
There is no runtime benchmark switch or second legacy copy of the supported
paint compositor.

Eligible temporary dry-brush tails use compact prediction pages. They share the
existing dry material evaluator, reading averaged exact destination color and
stroke coverage. The normal uniform camera uses the existing half-surface-pixel
contact evaluation density, capped at 4 document pixels per prediction texel.
At Fit this gives 64 × 64 prediction pages instead of 256 × 256 pages. No private
full-size coverage fork is needed. Reduction weights partial edge texels by their
actual document area. Changing or cancelling a tail invalidates its old footprint;
committing it still executes the authoritative full-resolution brush.

Exact queries replay a compact tail through the existing exact tile executor
using retained preview contacts. Replay preserves the existing full-size preview
semantics, including its view-dependent block evaluation. This happens on demand,
once for that tail, replacing the compact scratch pages. The already-composed display stays valid;
the next live tail returns to compact scratch. Save/undo history contains only
committed native pixels. There is no background exact-paint queue or settling
backlog to hide from the benchmark.

## Supported contract

The prototype admits native, untransformed, top-level normal paint layers and
paper, without masks, clipping, effects or persistent watercolor state. Visible
unsupported artwork routes the whole stack through exact composition. Fine
views, insufficient cache admission and active transforms also use that path.
Advanced brushes retain their exact temporary executor; only simple analytic dry
contacts without grain, selection, alpha lock or edge effects use compact tails.

During unchanged navigation, the cache may retain one neighboring level within
the same total component budget. Coarser levels derive from the already-reduced
layer images, with area weighting at document edges. Returning to a retained
level reuses its composed output. Artwork changes discard the spare immediately,
including its references to native tile captures. A new finer level still needs
authoritative content when it was not retained. This bounds ownership and avoids
repeated preparation at a zoom boundary; it does not eliminate every first-view
preparation cost. The current cache scans document page validity and composes a
bounding rectangle rather than maintaining a second fine-grained scheduler. Region and scale are explicit inputs to the new executor,
while effects and transforms remain future extensions with their own dependency
and quality requirements.

## Why display composition is approximate

Reduction and source-over do not generally commute, even with normal blending.
For premultiplied foreground `F`, foreground alpha `a`, background `B` and area
average `R`, the difference between reduced-layer composition and reduction of
exact composition is:

```
[R(F) + (1 - R(a)) R(B)] - R(F + (1 - a) B)
    = R(a B) - R(a) R(B)
```

This covariance vanishes for constant alpha or constant background within the
footprint. Correlated high-frequency alpha and background can produce visible
errors; normal blending alone does not guarantee equality. Compact prediction
adds contact-sampling error. Exact document output avoids both approximations.
The photographic and synthetic tests quantify particular fixtures, not a global
quality guarantee for arbitrary artwork or HDR values.

At 8 × reduction in each axis there are 64 times fewer composition pixels, but
changed exact paint must still be read, brush work still runs at document
resolution, and CPU submission, capture, source decoding and presentation remain.
The end-to-end measurements, rather than the pixel ratio, determine the benefit.

The design follows the general separation of responsive reduced feedback from
exact painting described by [Krita's Instant Preview documentation](https://docs.krita.org/en/reference_manual/instant_preview.html).
The [Vulkan render-pass sample](https://docs.vulkan.org/samples/latest/samples/performance/render_passes/README.html)
explains why attachment reads/writes and render-pass organization matter on tile
GPUs. Neither source predicts this renderer's measured speedup.

## Validation

Run on a physical GPU:

```sh
cargo test -p layer-render-wgpu --lib --offline -- --test-threads=1
cargo test -p layer-render-wgpu --test project --offline -- --test-threads=1
```

The scale tests compare against exact composition and exact output, exercise odd
edges, changing prediction footprints, opacity, ordering, source removal,
resolution changes and repeated entry/exit through the filter fallback. They
assert that superseded presentation allocations are absent. Project tests cover
all brush presets, save/reopen and exact undo/redo; the source-backed test also
compares the same committed stroke drawn at 12.5% and 100%.

An optional photographic oracle consumes a local 2048 × 1536 RGBA8 sRGB crop:

```sh
LAYER_DISPLAY_PHOTO_RGBA=/absolute/path/crop.rgba \
  cargo test -p layer-render-wgpu --lib --offline photographic_preview \
  -- --ignored --nocapture --test-threads=1
```

This evaluates 50, 460, 1000 and 2000 px G-Pen contacts, comparing both compact
prediction and committed display against area-reduced exact composition, and
requiring exact output equality in each case. The photo is deliberately not
included in the repository.
