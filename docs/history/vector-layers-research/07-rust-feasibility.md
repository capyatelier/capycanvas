# Vector layers for Capy Canvas: technical feasibility (Rust ecosystem, 2026-09-25)

[Vector layers research](../vector-layers-research.md) · source report, 2026-09-25

## 0. Summary

- **Feasible, with one hard problem.** The geometry, rendering, SVG and PDF building blocks exist under permissive licenses and all use **kurbo 0.13**: vello, usvg, hayro, linesweeper and Graphite. The hard problem is **robust boolean operations on curves**. Only one crate does this natively (linesweeper) and it is in "early beta".
- **Recommended core:** kurbo 0.13 for geometry, rstar 0.13 for spatial indexing, i_overlay 9 for robust polygon ops (including its new variable-width stroke) and linesweeper 0.4 for curve booleans. Raw samples are the stored source of truth, and fitted cubics are derived for editing.
- **Rendering:** reuse the **existing GPU dab engine** ("stamp-along-path") for textured brush strokes. Solid or pen strokes can use a cheap **SDF tapered-capsule** pass or **vello_cpu** tiles. Fills and Bézier paths use **vello_cpu** first and **vello_hybrid** later. The existing sparse raster pages act as the render cache, invalidated by dirty rectangles.
- **Blocking integration fact:** Capy Canvas vendors **wgpu 30.0.1** (`Cargo.toml` patches `vendor/wgpu*`). vello 0.10 and vello_hybrid 0.2 require **wgpu ^29.0.3**, so they cannot share your `Device` without a port. vello_cpu has no wgpu dependency, which avoids the problem.
- **Fill bounded by strokes:** vector face-finding on stroke **centerlines** plus gap-closing segments. Algorithms can come from OpenToonz, which is **BSD-3**. Keep a CSP-style raster "refer layer + close gap" fill as the fallback.

## 1. Codebase facts that constrain the design

- **Dab engine.** `docs/brush-renderer.md` describes it. Contacts are placed by traveled distance. Each 80-byte `Dab` record is drawn as an instanced quad with an analytic ellipse or an R8 tip. Brush properties are frozen at pen-down, and a deterministic seed is mixed with the stroke ID. Only damaged pixels are rasterized.
  - This makes stamp-along-path re-rendering natural: keep the samples, the frozen brush, the seed and the phase, then replay.
  - **Destination-aware brushes** (smudge, wet mix, watercolor, liquify, reservoir) depend on order and on existing pixels. They cannot be re-rendered locally, so exclude them from vector layers.
- **Layer model.** `Layer` holds an `Arc`-shared `RasterRevision` plus an optional immutable `source` (`crates/layer-core/src/lib.rs`). A vector layer fits as `LayerKind::Vector` with `Arc<VectorContent>`, using `raster` as the render cache.
- **`deny.toml` license allowlist:** Apache-2.0, BSD-2/3, MIT, ISC, Unicode-3.0, Zlib.
  - **BSL-1.0 (clipper2-rust), MPL, LGPL, GPL, AGPL and EUPL are not allowed.**
- **Web bundle size today:** 19.5 MB raw (dist) and 23.6 MB raw / 6.8 MB gzip (`apps/layer-web/pkg`).
- **Android wide-brush GPU cost is already the bottleneck.** `docs/development/android-wide-brush-performance.md` reports GPU medians of about 49 ms per update on Adreno. Re-rendering whole layers of wide textured strokes on Android is not viable, so caching is mandatory.
- **Raw input capture already exists** (`docs/development/stroke-recording.md`). This supports storing real samples, never predicted ones.

## 2. Geometry crates

| Crate (latest) | License | Curves | Booleans | Notes |
|---|---|---|---|---|
| **kurbo 0.13.1** (2026-05-13) | MIT/Apache | yes | no | `stroke`/`stroke_with` expand constant-width strokes, with joins, caps and dashes, much faster since 0.12. `offset::offset_cubic` replaced `CubicOffset` in 0.12. `fit_to_bezpath` and `fit_to_bezpath_opt` (the latter experimental, about 50× slower, near-optimal). `simplify::simplify_bezpath` / `SimplifyBezPath`. `ParamCurveNearest`, `ParamCurveArclen` (`arclen`/`inv_arclen`), `PathSeg::intersect_line`, `Shape::winding/contains/area`. **No curve–curve intersection, no booleans, no variable width.** The stroker states it uses parallel curves rather than a "rigorously correct parallel sweep (which requires evolutes)". https://docs.rs/kurbo, https://github.com/linebender/kurbo/blob/main/CHANGELOG.md |
| **lyon 1.0.x** (tessellation 1.0.22, 2026-09-06) | MIT/Apache | yes (f32, euclid) | no | Fill and stroke tessellation; `StrokeOptions::variable_line_width` gives per-vertex width. `lyon_algorithms` has `hit_test`, `measure`, `walk`, `raycast`, `hatching`, `aabb`. `lyon_geom` has `cubic_intersections(_t)` and `line_intersections` via Bézier clipping. Stable. https://docs.rs/lyon_algorithms, https://docs.rs/lyon_geom |
| **flo_curves 0.8.1** (2026-08-25) | **Apache-2.0 only** (OK) | yes | yes (`path_add/sub/intersect`, `GraphPath`) | Includes:<br>• `fit_curve` (Schneider-style)<br>• `offset_lms_sampling` with a per-t normal-offset closure, i.e. variable width<br>• `offset_scaling`<br>• `curve_intersects_curve_clip`<br>• `nearest_point_on_curve`<br>• `walk_curve_evenly`<br>• **`flood_fill_concave/convex`**, a ray-cast vector fill with `FillSettings::with_min_gap` (default 5.0)<br>• **`vectorize::DaubBrushDistanceField`**, which turns brush daubs into a distance field, then contours, then curves<br>Single maintainer, about 300k downloads, source read locally. https://docs.rs/flo_curves |
| bezier-rs 0.5.0 (2025-08) | MIT/Apache | yes | no | Graphite dropped it from its workspace in favor of kurbo. **Avoid.** |
| Graphite `path-bool` | MIT (port of PathBool.js) | yes | yes | **Replaced by linesweeper** in Graphite PR #2670, merged 2026-03-11. Unpublished. https://github.com/GraphiteEditor/Graphite/pull/2670 |
| **linesweeper 0.4.0** (2026-06-14) | MIT/Apache | **native cubic** | **yes** | "Robust" Bentley–Ottmann sweep over Bézier segments. `binary_op(_with_eps)` plus an n-ary `Topology` with tagged winding numbers. **Closed paths only** (returns a `NonClosedPath` error). Self-described "early beta". The GitHub repo was archived 2026-01-25 and development moved to Radicle. Graphite depends on it, and PR testers reported crashes on complex art. https://docs.rs/linesweeper |
| **i_overlay 9.0.0** (2026-09-19) | MIT/Apache | **no (polygons)** | yes | Integer core, deterministic across platforms, with f32/f64 adapters. Supports all fill rules, `FloatClip` (polyline clipped by polygon), `FloatSlice` (polygon sliced by polylines), outline/stroke with joins and caps, and batch point location. **9.0.0 added variable-width strokes** (`VariableStrokeOffset::variable_stroke`, round caps and joins, per-vertex width, integer math only). Used by geo. https://github.com/iShape-Rust/iOverlay |
| i_curve 0.2.0 (2026-09-19) | MIT | yes | yes | Curve booleans built on i_overlay; keeps curve segments. **182 downloads, weeks old.** Worth watching. https://github.com/iShape-Rust/iCurve |
| clipper2 0.6 / clipper2-rust 1.2 | MIT/Apache wrapper around C++ (BSL upstream) / **BSL-1.0** | no | yes | Polygon only. BSL-1.0 needs a `deny.toml` exception. Not needed given i_overlay. |
| geo 0.33.1, geo-booleanop 0.3.2 | MIT/Apache, MIT | no | yes | geo is geospatial and wraps i_overlay 4.5. geo-booleanop is stale (2020). Skip both. |
| euclid 0.22, glam 0.33 | MIT/Apache | – | – | Math only. **Standardize on kurbo f64 in document space**, and use f32 only in GPU records. |

**Curve booleans verdict.** linesweeper is the only curve-native, robustness-oriented option with a production user (Graphite). Wrap booleans behind a trait with two backends:
- **(a) linesweeper**, for exact Bézier paths.
- **(b) flatten to i_overlay (integer, robust), then refit with kurbo.** This is the default for pressure strokes, which are polylines anyway, and the fallback when (a) errors or times out.

Fuzz both backends: linesweeper ships an `arbitrary` feature. Treat i_curve as the future alternative for (a).

## 3. Rendering

### Options

- **vello 0.10.0** (2026-08-14), the compute renderer.
  - The README now says it "remains an experimental implementation for compute-capable GPUs".
  - `render_to_texture` needs `Rgba8Unorm` + `STORAGE_BINDING`.
  - Requires WebGPU on the web.
  - Mobile compute has known problems: unpredictable memory use, and shared-memory read bugs on Android/Vulkan that were fixed in 0.9.
  - **Not recommended as a dependency.** https://github.com/linebender/vello
- **vello_hybrid 0.2.0** (2026-08-07).
  - Processes paths on the CPU (sparse strips) and rasterizes and composites on the GPU without compute shaders. It has native WebGL2 and wgpu backends.
  - Linebender (Q1 2026): "roughly beta quality… should be usable". The README says it is "intended to become the primary renderer for production GPU use cases". The `vello_gpu` crate name was reserved 2026-08-26, which suggests an upcoming rename.
  - Panics on mask layers, complex filter graphs and some non-isolated blend modes.
  - Depends on wgpu 29, which conflicts with your wgpu 30.
  - https://docs.rs/vello_hybrid, https://linebender.org/blog/tmil-25/
- **vello_cpu 0.2.0**, the recommended first step.
  - The README calls it the "most mature" of the three.
  - SIMD on x86, NEON and WASM SIMD; optional multithreading.
  - Benchmarks: "second place in many… often beating… Skia and Cairo", with Blend2D faster (https://linebender.org/blog/tmil-19/).
  - No GPU coupling. Render dirty tiles by translating the scene into a tile-sized `Pixmap`, then upload into your sparse pages. This is best for fills, Bézier paths and export.
  - The cost is CPU time plus upload bandwidth, which is acceptable for dirty tiles but not for full-screen animation.
- **tiny-skia 0.12** (BSD-3): slower than vello_cpu. Skip it.
- **lyon plus your own WGSL:** stable and has no version coupling. Good for **overlays** such as the pen-tool preview, selection outlines and handles. Anti-aliasing needs MSAA or fringe geometry.
- **SDF tapered capsules** for solid pressure strokes.
  - Draw one instanced quad per centerline segment, evaluating an exact "uneven capsule" SDF (https://iquilezles.org/articles/distfunctions2d/, `sdUnevenCapsule`).
  - Write with **MAX blending** into the per-stroke R8 coverage your engine already has ("stroke-ID-keyed R8 coverage"), then composite once with the stroke's opacity. This avoids double-darkening at overlaps.
  - Resolution-independent, far fewer primitives than dabs, and naturally handles pressure. It also serves as the "pen/G-pen" fast path.

### Stamp-along-path (textured brushes)

**Replay procedure.** Replay the stored samples through the existing contact placement (distance-based, same seed, same frozen brush), then draw with the existing wgpu pipeline, scissored to the dirty rect.
- For split pieces, store `phase` (arc-length offset into the dab sequence) and the parent `seed`. Otherwise jitter and texture would shift after an erase.

**Cost model.**
- Dabs ≈ length ÷ spacing. Take 5k strokes × 300 dabs = 1.5M dabs; at 80 B each that is 120 MB. So **do not cache dab lists for all strokes**; regenerate them, with at most a small LRU cache for recently edited strokes.
- GPU cost is fill-rate bound: dab area × overlap. At 20 px dabs, 1.5M dabs is about 600M fragments.
  - Desktop: a few ms to tens of ms.
  - Mid-range Android: more than 100 ms, and far worse for wide brushes (see §1).
- **Conclusion:** full-layer replay only on load, when not cached, and only for visible tiles. Every edit must be incremental.

**Caching strategy.**
1. **Layer pages are the cache; vector data is the source of truth.** Optionally persist the cache in the project container so files open fast.
2. **Dirty-rect invalidation.** `dirty = old_bounds ∪ new_bounds`, dilated by the maximum radius plus AA. Clear those pages, query the R-tree for intersecting shapes, and re-stamp them in z-order with a scissor. Cost scales with local overlap, not layer size.
3. **Interactive drags and transforms.** Freeze "below" and "above" composites and render only the moving selection each frame. Re-bake on release.
4. **Zoom quality.** Render at document resolution, like raster layers, so textured output matches raster painting. SDF/vello solids can optionally re-render visible tiles at display resolution when zoomed past 100% (Rnote regenerates per-viewport images the same way).
5. **Tile scheduling.** Stamp work goes into the existing GPU queue. vello_cpu tiles go to worker threads on native. On wasm, run them on the main thread unless threads are enabled, so keep tiles small.

## 4. Variable-width strokes

**Representation:** width *w(s)* as a function of arc length, either per-sample or as knots.

**Outline algorithms**, needed for export, booleans, "outline stroke" and fill boundaries (not for on-screen rendering):
- **Naive normal offset** (perfect-freehand; the Rust port `perfect-freehand` 0.1.1 is MIT, tiny and barely used). Folds at high curvature and at cusps. Fine for previews only.
- **Tiller–Hanson** (offset the control polygon). Raph Levien measured O(n²) error scaling, "surprisingly poor". https://raphlinus.github.io/curves/2022/09/09/parallel-beziers.html
- **Levien curve-fit parallel curves.** O(n⁶) scaling with interval-arithmetic cusp detection. Implemented in kurbo for **constant** width. The same fitting framework "extends to variable-width strokes" but no crate ships that.
  - `santhoshtr/kurbo-curve-fit-stroke` (MIT, 2026) does variable width through linear perturbation plus refit. https://thottingal.in/blog/2026/02/20/var-interpolatable-smooth-curves/
- **Exact envelope of circles.** Offset point = `p + r(−r′T ± √(1−r′²)N)`, valid while |r′|<1. More accurate than a normal offset where width changes quickly.
- **Union of tapered capsules** (the convex hull of consecutive pressure circles), computed with robust polygon union. This is the most robust choice, and **i_overlay 9's `variable_stroke`** implements it directly: round caps and joins, integer-deterministic. Refit the result with kurbo `simplify_bezpath` or a Schneider fit for compact Bézier output.
- **flo_curves `DaubBrushDistanceField`**: distance field, then contours, then curves. Works for arbitrary tip shapes (non-round nibs).
- **Google `ink`** (Apache-2.0 C++, Jetpack Ink core) is a reference for mesh extrusion of modeled strokes. https://github.com/google/ink

**Recommendation:**
- Screen: dabs or SDF.
- Export and geometry: i_overlay variable_stroke, then fit.
- Constant-width Bézier paths: `kurbo::stroke`.

## 5. Curve fitting from pen samples

**Store raw samples as truth.** Mirror Graphite's MIT/Apache brush `Stroke`: `position: Vec<DVec2>`, and `pressure/tilt/twist/time` channels that are either `Uniform(T)` or `Samples(Vec<T>)`, plus `seed`. Quantize for storage: f32 positions relative to the stroke origin, u16 pressure, delta plus gzip. Optionally simplify at commit with a pressure-aware RDP pass.

**Smoothing.** Keep the engine's existing stabilizer. Google's Ink Stroke Modeler (Apache-2.0 C++: wobble filter, spring-mass position model, Kalman prediction) is the reference design; `ink-stroke-modeler-rs` (MIT/Apache) is a partial port used by Rnote.

**Fitting for control-point editing** (derived data):
- **Schneider** (Graphics Gems 1990). Paper.js `PathFitter` (MIT) is a clean reference. Add corner detection first.
- **kurbo `fit_to_bezpath`** (area and moment matching, quartic solve). Levien shows it avoids Schneider's local minima. It needs a `ParamCurveFit` source, so implement one over a smoothed polyline such as centripetal Catmull-Rom. https://raphlinus.github.io/curves/2021/03/11/bezier-fitting.html
- `simplify_bezpath` for paths that are already Bézier.
- Hyperbeziers, a new curve family (Levien, 2026-08), are future work aimed explicitly at "fitting touch or pen data". https://linebender.org/blog/hyperbezier/

**Width profile.** Use piecewise-linear knots over normalized per-segment parameters, CSP-style, where each control point carries a width. After a node edit, **the Bézier and its width knots become the truth**. Regenerate samples by arc-length resampling (`inv_arclen`) and keep the seed and phase.

## 6. Eraser, hit testing and editing

**Index (rstar 0.13).**
- Layer-level R-tree of `GeomWithData<Rectangle, ShapeId>`. Per-stroke chunk AABBs cover about 32 samples each and are built lazily.
- Keep a `ShapeId → envelope` map so removal is O(log n). Rnote's `KeyTree::remove_with_key` does a linear scan, which is the thing to avoid.

**Erase touched area.**
- For each candidate, compute the arc-length intervals where the distance from the centerline to the eraser's swept capsule chain is below `r_eraser` (optionally plus half the stroke width).
- Split, interpolating samples at the cuts. Pieces inherit `seed`, `phase` and brush.
- Choose a cut-end policy (a blunt cut or a re-taper), then invalidate dirty rects.
- Complexity: S–M.

**Whole line.** Hit-test and delete. Size S.

**Up to intersection.**
- Compute the touched stroke's intersections with every other stroke on the layer, including self-intersections. Use the R-tree prefilter, then segment–segment tests on polylines, or `lyon_geom::cubic_intersections` / flo_curves Bézier clipping for Béziers.
- Sort by arc length and delete the interval containing the hit parameter.
- Optionally treat near-misses under the gap tolerance as intersections, matching CSP behavior.
- Complexity: M.

**Adjust width after drawing.**
- A per-stroke `width_scale`: S.
- A local thicken/thin brush that edits width samples within a radius: M.
- "Normalize to fixed width": S.

**Selection, transform and node editing.** Graphite's `vector-types` (MIT/Apache) has a point/segment/region domain model with half-edge "face orbits", `offset_bezpath.rs`, `intersection.rs` and `merge_by_distance.rs`, plus its pen and path tools. These are direct references, reusable with attribution.

**Simplify:** kurbo, size S. **Offset path:** per-segment `offset_cubic`, joins, then a union cleanup. Size M.

## 7. Fill regions bounded by strokes (with gap closing)

- **A. Raster, CSP-style.** Flood fill on the rendered coverage of the reference layers. Gap closing treats pixels within the gap radius of lines as barriers, then dilates the fill back.
  - Cheap (M) and matches CSP, where fills are commonly done on a raster layer referencing the vector layer.
  - Downsides: resolution-bound, and edges can leave AA halos.
  - Krita's colorize mask (LazyBrush) and close-gap fill are GPL, so study only.
- **B. Vector planar arrangement** (Illustrator Live Paint / OpenToonz).
  1. Flatten the **centerlines**. The fill boundary then sits under the line, so there is no halo, which is the main quality advantage.
  2. Add **autoclose segments**: endpoint to nearest stroke, and endpoint to endpoint, within a maximum distance and angle.
  3. Compute faces. Options, in order of preference:
     - i_overlay `FloatSlice`: slice the canvas rectangle by all polylines, then batch point location.
     - A half-edge DCEL, as in Graphite's face orbits.
     - flo_curves `flood_fill_concave`, which has `min_gap` built in.
  4. Store the result as a static `Fill` shape placed under the strokes, with optional links to the boundary shape IDs.
  - Gap-closing and region code to port with attribution comes from **OpenToonz (BSD-3)**: `toonz/sources/common/tvectorimage/tl2lautocloser.*`, `tcomputeregions.cpp`, `tstrokeoutline.cpp` (https://github.com/opentoonz/opentoonz).
  - A static fill is size L. A "live" fill that recomputes when strokes change is **XL**.
- **C. Holes of the union of stroke outlines.** Union the i_overlay variable-stroke outlines plus bridging capsules; the hole containing the click is the region. Robust, but the edges end up at the outline, not the centerline.

**Recommendation:** B, using i_overlay slicing with OpenToonz-style autoclose. Keep A for cross-layer and raster references.

## 8. SVG, PDF and text

**SVG import: usvg 0.48.1** (MIT/Apache, kurbo 0.13).
- Normalizes shapes to paths, applies CSS, resolves `use`, converts units, and turns arcs and relative commands into absolute cubics and quads.
- Keeps groups, transforms, gradients, patterns, clip paths, masks, filters and text (text can be flattened to paths).
- Drops scripts, animation, `a`/`view`/`cursor`, and ignores unsupported features.
- Map paths and paints to `PathShape`. Rasterize filters and masks through resvg, or drop them.
- Use `default-features = false` on the web. Measured: 385 KB without text versus 1.51 MB with text.
- https://docs.rs/usvg

**SVG export.** Write it directly (`BezPath::to_svg` plus xmlwriter, or the `svg` 0.18 crate).
- SVG has **no variable-width stroke**, so pressure strokes export as filled outlines (i_overlay then fit).
- Textured stamp strokes export as embedded PNG images per layer or group, with an optional outline.

**PDF export: krilla 0.8.2** (MIT/Apache, used by Typst).
- Paths, fills and strokes, gradients, masks, blend modes, PDF/A and PDF/UA.
- 90+ snapshot tests and 210+ visual regression tests across six PDF viewers, plus 1500+ SVG integration tests.
- Measured about 1.15 MB of wasm with default features off. `pdf-writer` 0.15 is the lower-level alternative.
- https://github.com/LaurenzV/krilla

**PDF import: hayro 0.7** (MIT/Apache).
- `hayro-interpret`'s `Device::draw_path` hands you a **kurbo `BezPath`**, a `Paint` and a draw mode, which maps directly onto shapes. `hayro-svg` converts PDF to SVG and can be chained through usvg.
- Self-described as "still in a very development stage", but handles 1,400+ test PDFs.
- Measured hayro-svg at 3.0 MB raw / 1.38 MB gzip with the default embedded fonts and cmaps, so **lazy-load it on the web**.
- Alternatives: `pdfium-render` (MIT/Apache, but ships a native PDFium binary, which is heavy); `mupdf` (**AGPL — no**); lopdf and pdf-rs (MIT, low-level only).

**Text on path.** Lay out with parley 0.11, get outlines from skrifa 0.47, and place glyphs with `inv_arclen`. Size L.
- skrifa versions are fragmented: usvg uses 0.44, hayro and krilla use 0.42. Expect duplicate code until they align.

## 9. Image tracing

- **vtracer**: MIT OR Apache per crates.io 0.6.5; GitHub shows MIT with 1.0.0-alpha on the repo.
  - Handles color, with pixel, polygon and spline output. Fast and wasm-capable.
  - The crate pulls `image` 0.23 and `clap` 2, so depend on **visioncortex 0.9.3** directly.
  - Outline tracing only.
- **potrace (GPL-2) — flagged.** Inkscape's bucket fill uses it.
- **autotrace (GPL/LGPL) — flagged.** It is the one with centerline tracing.
- **Centerline tracing of lineart into vector strokes with width** (skeletonize, distance transform for width, fit) has no permissive crate. Size **XL**.

## 10. Apps to learn from

- **Graphite**. The README now says **"dual-licensed under… MIT… or Apache 2.0"**; GitHub metadata still shows Apache-2.0, and both are compatible.
  - Stack: kurbo 0.13, vello 0.10, linesweeper 0.4, usvg/resvg 0.47, parley, polycool, wgpu 29.
  - Reusable pieces: vector graph model, pen and path tools, brush `Stroke` channels plus `BrushCache`, offset and intersection code.
  - Its node-graph runtime is too heavy to adopt; take components only.
- **Rnote (GPL-3.0-or-later): study only.**
  - Uses kurbo 0.11, ink-stroke-modeler-rs, rstar, parry2d, piet/cairo.
  - Pattern worth copying in spirit: a per-stroke image cache regenerated per viewport on worker threads, and an eraser that splits strokes.
- **OpenToonz (BSD-3):** the best permissive reference for vector-lineart topology (thick-quadratic `TStroke`, regions, autoclose).
- **lib2geom (LGPL-2.1 / MPL-1.1)** and **Inkscape PowerStroke (GPL): study only.**
- **Runebender** (Apache-2.0, ported to Xilem in 2026) and `linebender/spline`: pen-tool and spline UX references.
- **Krita (GPL): study only.**

## 11. Recommended stack and license table

| Role | Crate | Version | License | Status |
|---|---|---|---|---|
| Geometry core | kurbo | 0.13.1 | MIT/Apache | OK |
| Root finding (kurbo dependency) | polycool | 0.4.0 | MIT/Apache | OK |
| Spatial index | rstar | 0.13.0 | MIT/Apache | OK |
| Robust polygon ops, variable stroke, slicing | i_overlay (+ i_float, i_shape: MIT) | 9.0.0 | MIT/Apache (MIT) | OK |
| Curve booleans | linesweeper | 0.4.0 | MIT/Apache | OK, beta |
| Curve booleans (watch) | i_curve | 0.2.0 | MIT | OK, very new |
| Extra curve algorithms (optional) | flo_curves | 0.8.1 | Apache-2.0 | OK |
| Curve–curve intersection (optional) | lyon_geom | 1.0.19 | MIT/Apache | OK |
| Overlay tessellation (optional) | lyon_tessellation | 1.0.22 | MIT/Apache | OK |
| CPU vector raster | vello_cpu (+ vello_common, peniko) | 0.2.0 / 0.6.1 | MIT/Apache | OK |
| GPU vector raster (later) | vello_hybrid | 0.2.0 | MIT/Apache | wgpu 29 conflict |
| SVG in | usvg (+ resvg for filters) | 0.48.1 | MIT/Apache | OK |
| PDF out | krilla / pdf-writer | 0.8.2 / 0.15.0 | MIT/Apache | OK |
| PDF in | hayro-interpret / hayro-svg | 0.7.x | MIT/Apache | OK, early |
| Text | parley, skrifa | 0.11.1 / 0.47.0 | MIT/Apache | OK |
| Tracing | visioncortex (vtracer) | 0.9.3 / 0.6.5 | MIT/Apache | OK |
| Smoothing reference | ink-stroke-modeler (C++), ink-stroke-modeler-rs | – / 0.1.0 | Apache / MIT-Apache | OK |
| **Flagged** | potrace, autotrace, Rnote, Krita, Inkscape (GPL); lib2geom (LGPL/MPL); mupdf (AGPL); clipper2-rust (BSL-1.0, not allowlisted); contour_tracing (EUPL) | | | **Do not link or copy** |

## 12. Proposed vector layer data model

```rust
enum LayerKind { …, Vector }
struct Layer { …, raster: RasterRevision /* render cache */, vector: Option<Arc<VectorContent>> }

struct VectorContent { shapes: Vec<Arc<Shape>> /* z-order */, index: RTree<GeomWithData<Rectangle<[f64;2]>, ShapeId>> /* rebuilt, not saved */ }

struct Shape { id: ShapeId, transform: kurbo::Affine, geometry: Geometry, paint: Paint, bounds: Rect /* incl. max radius */ }

enum Geometry {
    Stroke(Centerline),                   // freehand brush vectors
    Path { path: BezPath, closed: bool, width_knots: Option<WidthProfile> }, // pen tool, imported SVG/PDF
    Region { boundary: BezPath, fill_rule: FillRule, links: Vec<ShapeId> }, // fills from face finding
}
struct Centerline {
    samples: SampleChannels,              // pos (f32 rel. origin), pressure/tilt/twist/time: Uniform|Samples
    fitted: Option<(BezPath, WidthProfile)>, // derived; becomes truth after node edits
    phase: f32, seed: u64,                // stable stamping after splits
}
enum Paint {
    Stamp { brush: BrushSnapshotId /* frozen, hashed, shared */, color: Rgba, width_scale: f32 }, // stamp engine
    Solid { stroke: Option<SolidStroke /* color, width profile, cap/join/dash */>, fill: Option<FillPaint> }, // SDF / vello_cpu
}
```

- **Render dispatch:**
  - `Stamp` goes to layer-engine contact placement and then layer-render-wgpu.
  - `Solid` strokes go to SDF capsules; constant-width `Path` strokes go through `kurbo::stroke` into vello_cpu.
  - `Region` and fills go to vello_cpu.
- **History:** operations of the form Add, Remove or Replace(`Arc<Shape>`). This is cheap, and matches the existing `Arc` revision style. Store computed geometry (boolean and fill results) in history rather than recomputing it, so cross-platform floating-point differences cannot diverge replays. i_overlay's integer mode is deterministic.
- **Split of responsibilities** (per `AGENTS.md`):
  - Hosts: input capture and timing only.
  - Rust: hit testing, erase, boolean ops, fills, fitting, serialization, undo and caching.

## 13. Complexity estimates

| Feature | Size |
|---|---|
| Vector layer model, serialization, undo, R-tree, dirty-rect cache | M |
| Brush-stroke capture into vectors with stamp-along-path replay (reusing the engine) | L |
| Solid strokes via SDF capsules; fills and paths via vello_cpu tiles | M |
| Select, move and transform strokes (below/above cache) | M |
| Vector eraser: whole line S, touched area M, up to intersection M | S–M |
| Adjust width: global S, local brush M | S–M |
| Fit plus control-point editing of brush strokes | L |
| Pen tool and node editing (Bézier) | L |
| Boolean ops: integration M; hardening and fuzzing on real art | L |
| Offset M; simplify S; outline stroke: constant S, variable M | S–M |
| Fill: raster close-gap M; vector static faces with autoclose L; live-updating fill XL | M–XL |
| SVG import M, export S–M; PDF export M; PDF import L | S–L |
| Text on path | L |
| Color tracing (visioncortex) M; centerline lineart tracing XL | M–XL |
| vello_hybrid GPU path on wgpu 30 (fork or wait) | M–L |

## 14. Risks

1. **Boolean robustness.** linesweeper is beta, its GitHub repo is archived and development is on Radicle, and Graphite PR #2670 has crash reports.
   - Mitigate with the dual backend (polygon fallback), time and size limits, fuzzing, and never running booleans on the input path.
2. **wgpu version skew.** vello and vello_hybrid use wgpu 29 while you vendor a patched wgpu 30.
   - Start with vello_cpu. Port vello_hybrid's WGSL and pipelines to your device later, or wait for an upstream bump.
3. **Android GPU cost** of re-stamping wide textured strokes (~49 ms GPU per update is already measured).
   - Keep invalidation strictly incremental, use below/above caches during drags, ban destination-aware brushes on vector layers, and use SDF for pen strokes.
4. **Memory and file size of raw samples.**
   - Quantize and compress, and optionally simplify at commit.
5. **wasm size.** Measured below; about +0.55 MB gzip for the core set, and hayro plus krilla add about 1.8 MB gzip.
   - Lazy-load a separate import/export wasm module and disable usvg text.
6. **i_overlay variable stroke is 6 days old.**
   - Benchmark and golden-test it. The custom capsule union through the same boolean engine is the fallback.
7. **Fill UX.** Vector gap closing is heuristic (distance and angle thresholds), so expect tuning. Live fills are XL.
8. **Determinism.** f64 transcendental functions can differ between platforms.
   - Persist derived geometry, and use i_overlay's integer mode where replay must be bit-exact.

## 15. Measured wasm32 sizes

Standalone `cdylib` probes built outside the repository: `opt-level="z"`, LTO, 1 codegen unit, `panic=abort`, stripped, no wasm-opt. Each probe calls the listed APIs so they are not dead-code eliminated.

| Probe | Raw | gzip |
|---|---|---|
| kurbo: stroke, simplify, offset_cubic, fit, nearest, arclen | 84 KB | 36 KB |
| i_overlay: union + variable_stroke | 182 KB | 61 KB |
| linesweeper binary_op (+kurbo) | 191 KB | 74 KB |
| vello_cpu fill+stroke, no text/png | 1.37 MB | 306 KB |
| same with `+simd128` | 351 KB | 126 KB |
| usvg, no default features | 385 KB | 158 KB |
| usvg + text | 1.51 MB | 531 KB |
| rstar | 32 KB | 14 KB |
| kurbo + i_overlay + linesweeper + vello_cpu + usvg + rstar (no simd128) | 2.02 MB | 554 KB |
| krilla, no default features | 1.15 MB | 416 KB |
| hayro-svg, defaults | 3.04 MB | 1.38 MB |
| *Current Capy Canvas web wasm* | *19.5–23.6 MB* | *6.8 MB* |

The vello_cpu measurement suggests enabling `simd128` for the web build; I found no `simd128` in the repo configs.

## Sources

- https://docs.rs/kurbo · https://github.com/linebender/kurbo/blob/main/CHANGELOG.md
- https://github.com/linebender/vello · https://docs.rs/vello_hybrid · https://docs.rs/vello_cpu · https://linebender.org/blog/tmil-25/ · https://linebender.org/blog/tmil-19/ · https://linebender.org/blog/hyperbezier/
- https://docs.rs/linesweeper · https://github.com/jneem/linesweeper · https://github.com/GraphiteEditor/Graphite/pull/2670 · https://github.com/GraphiteEditor/Graphite/discussions/3528
- https://github.com/iShape-Rust/iOverlay · https://github.com/iShape-Rust/iCurve
- https://docs.rs/flo_curves · https://docs.rs/lyon_algorithms · https://docs.rs/lyon_geom · https://docs.rs/lyon_tessellation
- https://github.com/GraphiteEditor/Graphite (Cargo.toml, README, node-graph/libraries/vector-types, brush-types) · https://github.com/r-flash/PathBool.js
- https://github.com/flxzt/rnote · https://github.com/opentoonz/opentoonz · https://gitlab.com/inkscape/lib2geom
- https://raphlinus.github.io/curves/2022/09/09/parallel-beziers.html · https://raphlinus.github.io/curves/2021/03/11/bezier-fitting.html · https://raphlinus.github.io/curves/2023/04/18/bezpath-simplify.html · https://arxiv.org/abs/2405.00127 (GPU-friendly stroke expansion)
- https://thottingal.in/blog/2026/02/20/var-interpolatable-smooth-curves/ · https://iquilezles.org/articles/distfunctions2d/
- https://github.com/google/ink-stroke-modeler · https://github.com/google/ink · https://github.com/flxzt/ink-stroke-modeler-rs · https://github.com/sibaiper/perfect_freehand
- https://docs.rs/usvg · https://github.com/LaurenzV/krilla · https://github.com/LaurenzV/hayro · https://docs.rs/hayro-interpret
- https://github.com/visioncortex/vtracer · crates.io API (versions, licenses, dates) for every crate listed
