# WGSL filter library

Design and milestone checklist for the expanded Filters picker and thirty additional
filters. This extends the ten adjustments documented in
[adjustments-implementation.md](adjustments-implementation.md).

## Execution contract

All algorithms are original WGSL. Rust declares parameters, preview presets,
categories, sampling requirements and ordered passes. Hosts render the shared
catalog and property schema; they do not implement filter algorithms or policy.

- Pointwise adjustments stay fused, including compatible masks and clipping.
- Image passes sample full-resolution GPU inputs across paint-tile boundaries.
  A pass can read the preceding pass and the original input. This covers separable
  blur, unsharp masking and glow without introducing a general node editor.
- Sampling dependencies explicitly distinguish bounded neighborhoods from
  whole-image remapping. Dirty regions must expand through every dependent pass;
  remapping conservatively invalidates the affected output image.
  Use effective sampling bounds from the current parameters where possible.
  Validate localized output against full recomposition and benchmark sparse,
  broad and full-frame edits, including CPU scheduling/encoding costs. Prefer a
  contiguous expanded region when splitting it into tiles would add more work;
  incremental scheduling must be a measured benefit, not an assumption.
- Cache composition at image/time-dependent boundaries. Animation changes a time
  value, not the document or paint strokes. Static upstream results remain cached.
  No canvas readback, paint replay, pipeline compilation or image allocation belongs
  in a warmed animation frame.
- Time-aware programs expose shared Animate and Time controls. Frozen time is
  deterministic and also used for picker previews. Hosts request display frames
  only while a visible effect needs animation.
- Mask/opacity/blend apply to the final pass, not every intermediate. Clipped
  filters operate on the existing clipping stack and isolated groups retain their
  existing semantics.

Full-resolution image boundaries require additional GPU storage. Track this in
Stats and benchmarks; do not describe neighborhood filters as free. Scratch is
reused, and deleted/inactive boundaries release their caches.

## Picker

Compact categorized rows with a filter name and artwork preview; a category
dropdown and an expanding search field. Category/search results are core-driven.
Preview source is a nonempty crop at the selected insertion point, at one document
pixel per preview pixel. Use the G-Pen sample silhouette as an alpha mask **after**
filtering, so its edge does not contaminate blur or neighborhood operations.
Defaults are used unless neutral, in which case the catalog supplies a meaningful
preview preset. Empty documents have an explicitly generated sample instead.

Cache the source crop and previews by source/content revision, insertion context,
program/preset and extent. Changing search/category, hovering or panning must not
regenerate pixels. Only requested previews are generated, outside the drawing
critical path, and transferred as a small atlas rather than a canvas readback.

## Additional catalog (30)

| Purpose | Filters |
| --- | --- |
| Practical (10) | Gaussian Blur, Unsharp Mask, High Pass, Motion Blur, Denoise, Edge Detect, White Balance, Split Tone, Vignette, Film Grain |
| Exploratory (10) | Bloom, Soft Focus, Halftone, Crosshatch, Emboss, Pixel Mosaic, Chromatic Aberration, Painterly, Solarize, Pencil |
| Experimental (10) | Kaleidoscope, Swirl, Ripple, Glass, Rainy Glass, VHS, CRT, Heat Haze, Iridescence, Domain Warp |

These names describe effects, not compatibility with another application's
implementation. Time controls belong only to programs that use time.

## Foundation milestone

Implemented ABI 2 with ordered image passes, declared footprints, time controls,
edit-time Gaussian coefficient tables and reusable GPU checkpoints. Adjacent
image boundaries alias their predecessor's result; a topmost checkpoint is copied
to the composite in one region operation. Tests cover cross-tile reads, final-pass
masking, frozen-time cache hits, upstream reuse, cache memory accounting and
equivalence to fused filters in masked/clipped/isolated groups. This comparison
also fixed a pre-existing double-fusion bug that overwrote nested opacity.

At the foundation milestone the thirty-filter catalog and web/Android picker
ports were still pending. The repeat ten-filter 4096×4096 Vulkan baseline on the
workstation measured completion median/p99: 2.05/2.56 ms fused, 3.46/5.83 ms
masked, 2.89/3.94 ms clipped. This establishes no large baseline regression;
it does not yet establish neighborhood-filter, preview or device performance.

## GTK picker milestone (2026-09-10)

The shared category/search model and GTK categorized list now display actual
GPU-rendered previews. They use the same shader bodies as the canvas. The initial
catalog-wide preview pipeline was replaced with per-requested-filter pipelines
at the forty-filter milestone below. Non-neutral preview presets do not change layer
insertion defaults. Hosts request only visible rows, at most eight at a time;
the core defers requests while input, a stroke or document edits are pending.

The source is the composition at the insertion point in its layer-group scope,
without filters above that point or the mask-inspection overlay. A topmost source
copies the existing GPU composite in one operation. The GPU locates an actual
painted pixel, preferring a crop whose corners also contain artwork; an empty
document uses an original procedural color sample. Only the winning coordinate
pair (eight bytes) returns before rendering. This is asynchronous and never
waits on the drawing thread. A bounded multi-pass effect renders its crop plus
sampling halo; whole-document dependencies remain correct. The original G-Pen
sample alpha masks the final result, after filtering. PNG decoding happens once.

Source/pixel caches ignore camera movement and category/search changes. Cache
hits submit no GPU work. Source invalidation uses renderer paint epochs and
composition metadata, not the UI revision alone. Small atlas readbacks share the
existing layer-thumbnail color conversion and asynchronous mapping code. GTK
shares the received atlas memory between its image widgets.

Release Vulkan measurements on the workstation, eight 400×80 previews from a
4096×4096 source, five warm-ups and sixty samples:

| Operation | Median | p95 | p99 |
| --- | ---: | ---: | ---: |
| Refresh source + eight previews | 0.605 ms | 0.998 ms | 3.032 ms |
| Eight previews from cached source | 0.362 ms | 0.432 ms | 0.663 ms |
| Cached image delivery; no GPU work | 0.0150 ms | 0.0153 ms | 0.0154 ms |

These include completion and small-image transfer, not presentation. Initial
pipeline setup was 16–68 ms in two runs and is not included in warmed numbers.
Preview storage measured 67,419,024 GPU bytes (64.30 MiB), dominated by one 4K
RGBA8 source snapshot; CPU cache storage is 128,000 bytes per 400×80 row. Stats
includes preview GPU storage. Raw measurements are generated/ignored at
`artifacts/benchmarks/filter-previews.csv`.

Validation covers nonempty selection, insertion below a clipped adjustment,
100% pixel scale, blank-document fallback, camera-independent cache hits, and
cropped two-pass output matching full-document output across a tile boundary.
The GPU suite passes 43 tests; three opt-in benchmarks are excluded. Shared
core/engine/UI tests and GTK integration pass. The wasm target compiles; this is
not yet a web/Android UI-port or device-performance claim. The PNG dependency
passes the existing MIT/Apache-compatible distribution notice audit.

## Forty-filter GTK milestone (2026-09-10)

All thirty additions in the catalog above are now implemented as declarative
Rust programs and original WGSL. The same registry defines IDs, categories,
labels, controls and preview presets. The renderer does not branch on filter IDs.
Time-aware filters include Film Grain, Ripple, Rainy Glass, VHS, CRT, Heat Haze,
Iridescence and Domain Warp. Other effects do not request animation frames.

The blur family shares normalized separable Gaussian tap tables and two passes.
Denoise uses a bounded, alpha-aware bilateral neighborhood. Painterly chooses
the lowest-variance of four regions using fixed nine-sample quadrature per region;
it is an efficient approximation, not a full anisotropic Kuwahara implementation.
Halftone and Crosshatch are artistic screen treatments, not CMYK print separation.
Rainy Glass is an original procedural refraction/drop effect, not fluid simulation
or a port of Heartfelt. Bloom thresholds bilinear samples during its first pass;
it is a compact approximation to threshold-then-convolve, not an exact reference
implementation of that ordering. These limits keep the implementation explicit.

Current parameter values determine bounded sampling support. Dependency indices
respect isolated groups and clipping bases and update only on structural edits.
Write-only input tiles draw directly into image caches in a shared pass, removing
redundant tile copies. All forty filters and preview crops pass pixel tests, and
GTK insertion, controls, categories and search pass integration tests.

Preview pipelines now compile lazily per requested filter. The initial single
catalog-wide switch would compile every kernel up front; with forty effects its
measured cold startup reached 413 ms. Compiling only the eight requested rows
reduced that to 128 ms in the first comparison, with warmed eight-row generation
still around 0.34 ms. Cold startup remains a one-time cost, not a claim of instant
compilation. Row image and pipeline caches survive category/search changes.

See [forty-filter validation](filter-library-validation.md) for all measured
latencies, incremental correctness, memory, artwork and remaining platform gates.
The web categorized preview picker now passes the forty-filter browser integration
test, including search, generated properties, visible preview pixels and animation
markers. Android's picker port and expanded platform performance benchmarks remain
to be done; compiling its shared Rust catalog is not a completed port.

## Validation gates

- Shader validation and generated controls for every registered program.
- Known-pixel, alpha/mask/clip/group and tile-boundary tests; incremental output
  equals full recomposition. Frozen animation is stable; animated output changes.
- Inspect contact sheets on detailed color artwork and transparent edges.
- Bench single filters and a five-expensive-filter stack: CPU submit, GPU and
  completion median/p95/p99, cold compilation separately, tracked GPU storage.
  Target warmed frames below 8.33 ms on the workstation; report device/emulator
  limitations rather than equating shader timing with end-to-end 120 Hz.
- Bench preview cold generation and cache hits; prove unchanged inputs cause no
  GPU work. Keep previews out of interactive stroke measurements.
- GTK first, then web and Android with the same shaders/catalog/schema.

## Research and licensing

[GIMP's filter index](https://docs.gimp.org/2.10/en/filters.html) informs categories
and practical effects. [Darkly](https://github.com/darkly-art/darkly) is visual
inspiration only: its [license](https://github.com/darkly-art/darkly/blob/dev/LICENSE)
is AGPL-3.0-or-later. Do not import its source or assets.
[ghostty-shaders](https://github.com/0xhckr/ghostty-shaders) offers effect ideas but
does not expose a repository-wide license in its root listing; individual shader
provenance cannot be assumed permissive. No shader code is imported from it.
Implement common mathematical operations independently under this repository's
MIT OR Apache-2.0 license, with no translation of third-party shader code.
The [WGSL specification](https://gpuweb.github.io/gpuweb/wgsl/) is the API reference.

Algorithm references include GIMP's [Unsharp Mask](https://docs.gimp.org/2.10/en/gimp-filter-unsharp-mask.html),
[Gaussian Blur](https://docs.gimp.org/2.10/en/gimp-filter-gaussian-blur.html), and
[Newsprint](https://docs.gimp.org/2.10/en/gimp-filter-newsprint.html) documentation
for conventional controls and behavior. No implementation or assets are copied.

Additional leads supplied by the user: Dave Hoskins (Bokeh Venice), p4vv37
(Generalized Kuwahara), Martijn Steinrucken / BigWIngs (Heartfelt), FMS_Cat
(20151110_VHS), aeva (watercolor propagation), Inigo Quilez (Domain warping), and
nimitz / stormoid (Watery). Attribution by an intermediate project is not a
redistribution grant; inspect the original license before adapting any code.
Garrett Gunnell's [Post-Processing library](https://github.com/GarrettGunnell/Post-Processing)
does have an [MIT license](https://github.com/GarrettGunnell/Post-Processing/blob/main/LICENSE.md)
(copyright 2022 Garrett Gunnell). Its documentation also explicitly discusses
combining modular effects into fewer passes. No code or assets from these
additional references have been imported.
