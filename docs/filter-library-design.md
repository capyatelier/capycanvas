# WGSL filter library

Implementation checklist for the expanded Filters picker and thirty additional
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

The expanded picker and thirty-filter catalog below remain implementation work,
not completed features. The repeat ten-filter 4096×4096 Vulkan baseline on the
workstation measured completion median/p99: 2.05/2.56 ms fused, 3.46/5.83 ms
masked, 2.89/3.94 ms clipped. This establishes no large baseline regression;
it does not yet establish neighborhood-filter, preview or device performance.

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
