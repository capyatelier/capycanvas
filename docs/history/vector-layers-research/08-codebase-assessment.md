# Vector layers in Capy Canvas: architecture assessment

[Vector layers research](../vector-layers-research.md) · source report, 2026-09-25

Paths are relative to the repository root. Line numbers were taken from the worktree at `796f7f5b`.

## Summary

- **The document stores only raster pixels. It does not keep strokes.** `Stroke` is documented as "Never part of document persistence or undo history" (`crates/layer-core/src/lib.rs:1203-1222`). Layers hold immutable sparse 256² tile revisions (`Layer.raster`, `lib.rs:174-199`). Undo swaps those revisions (`Edit::SetRaster`, `lib.rs:1639-1682`). Some docs say otherwise: `README.md:149-151`, `docs/internals/documents.md` ("Paint layers contain strokes") and `docs/reference/gpu-brush-engine.md:262` describe a stroke-replay model that the code no longer uses. `docs/reference/project-format.md` is accurate.
- **The stroke-replay machinery exists and is tested for determinism, but it only covers the most recent stroke.** `DabGenerator::generate(&Stroke, space, &mut dabs)` (`crates/layer-engine/src/brush.rs:270-279`) replays a stroke from its stored samples and `BrushSnapshot`. Tests assert that the live dabs equal the replayed dabs, using real Wacom captures (`crates/layer-engine/src/canvas.rs:3093-3180`, `brush.rs:1180`). The engine uses this to re-render the last stroke for end taper and late corrections (`canvas.rs:1853-1880`), then throws the stroke away.
- **There is no curve or path geometry anywhere.** Nothing in `Cargo.lock` provides it: no kurbo, lyon, usvg, resvg, tiny-skia, vello or font stack. `quick-xml` and `xml-rs` are present only as build-time dependencies of `wayland-scanner` and `gl_generator`. The only geometry types are:
  - polygon selection contours;
  - analytic line, rectangle and ellipse "Figures";
  - affine transforms.
- **The best way in is "vector content as layer metadata, with the raster revision as its cache".** This mirrors how `ImportedImage` layers pair an immutable `source` with raster overrides. Keeping the raster means opening, exporting, compositing, undo and GPU recovery keep working unchanged, and re-rendering happens only when the user edits the vector content.

---

## 1. Layer model

**Layer kinds.** `LayerKind` has seven variants: `Paint`, `ImportedImage`, `AiSuggestion`, `Background`, `Group`, `Effect` and `Selection` (`lib.rs:162-171`). There are no text, vector or fill layers. The layer research doc explicitly deferred "vector masks, smart objects" and "vector creation" (`docs/history/layers-initial-design.md:322`, `docs/history/layers-research.md:136`).

**What a layer holds.** `Layer` (`lib.rs:174-199`) has these fields:
- `raster: RasterRevision` (`#[serde(skip)]`; stored separately as tiles);
- `source: Option<Arc<SourceImage>>` and `asset`;
- `properties: LayerProperties`;
- `mask: Option<LayerMask>`;
- `pending_operations: Vec<LayerOperation>` (transient, `serde(skip)`);
- `effect: Option<Arc<EffectInstance>>`;
- `selection`.

`LayerProperties` (`crates/layer-core/src/layers.rs:398-416`) holds:
- `parent` (groups);
- `offset`;
- `placement: Affine`, a non-destructive placement that is never resampled into the backing;
- `alpha_locked`, `locked`, `clipped`;
- `blend`: seven `LayerBlend` modes (`layers.rs:364-373`).

**Masks.** `LayerMask` (`layers.rs:418-438`) is a raster revision with an `initial: Option<Selection>`, link and inversion settings. It has its own `LayerId`, so strokes can target it.

**Adding a kind.** There are about 169 non-test references to `LayerKind::`. Most are in `crates/layer-ui/src/art_layers.rs` (41), `layers.rs` (22), `session.rs` and `selection_masks.rs`. Most are `==` or `matches!` checks rather than exhaustive matches, so a new variant compiles but falls silently into the "not paint" branches. Each reference needs an audit. The key gates are:
- `drawing_target()`, which allows painting only on `LayerKind::Paint` (`layers.rs:948-967`);
- `validate_layer()` (`layers.rs:1019`);
- `Edit::SetReferences` (`lib.rs` around line 1490).

Native hosts never refer to layer kinds (a grep of `apps/` finds nothing), so the blast radius is shared Rust plus a new icon in the shared SVG bank.

**Composition.** Composition is per tile, in `crates/layer-render-wgpu/src/scene.rs:840-880`. Each layer kind is handled one of these ways:
- groups recurse;
- effects run fused WGSL chains;
- layers with a placement use `placed_tile`;
- everything else samples its 256² paint pages and its tiled source.

A vector layer that owns ordinary paint pages would need no compositor changes.

## 2. Brush and stroke pipeline

**Input.** `PenEvent` (`crates/layer-engine/src/input.rs:65-81`) carries:
- `device_id`, `sequence`, `timestamp_ns` and `view_revision`;
- surface position, pressure, `tilt_radians`, `twist_radians` and `distance`;
- phase, tool and flags (`PREDICTED`, `ESTIMATED`, `CORRECTION` and others).

`to_stroke_point` (`input.rs:130-162`) converts each event to a `StrokePoint { position, pressure, tilt[2], twist, elapsed_micros }` (`lib.rs:273-284`, 28 bytes, `repr(C)`). Along the way it:
- maps the point into document space;
- applies the user's pressure curve, so the stored pressure is already mapped;
- rotates tilt and twist with the view;
- drops hover `distance`.

`StrokeBuilder` (`input.rs:203+`) keeps real and predicted samples apart. `finish()` returns only the real ones. Airbrushes append synthetic stationary samples through `append_stationary`, which makes them replayable.

**Smoothing and spacing.** Stabilization is a brush property, not an input-layer step. `BrushStabilization` covers streamline, stabilization, motion filtering, pressure smoothing and a limit on how fast pressure may fall. It runs inside `DabGenerator::stabilize` (`brush.rs:362-432`). The stored samples are therefore raw, and replay re-stabilizes them deterministically.

Spacing depends on the brush type:
- Stamp brushes are placed by distance (`spacing` as a fraction of diameter).
- Swept "contact" brushes (pencil, charcoal, ink) use an online simplifier with a tolerance of 0.25 px or 1% of the radius (`docs/development/contact-brush-engine.md`).

Prediction (`crates/layer-engine/src/feedback.rs` and `feedback/motion_fit.rs`) only ever produces replaceable preview batches and never reaches the document.

**Dabs and the GPU.** The CPU resolves `Dab` records: center, radii, rotation, motion, color, flow, hardness and contact/material vectors (`crates/layer-render/src/lib.rs:55-78`). It groups them into `DabBatch`es, which carry a `DabStyle` (`lib.rs:142-191`) and a `brush_to_layer: Affine`. The batches travel in a borrowed `FramePacket` (`lib.rs:196-216`). `WgpuRasterizer` draws instanced contact quads into sparse pages. Brushes that read existing paint (smudge, wet, watercolor, liquify) use ordered chunks and extra material state (`docs/reference/gpu-brush-engine.md`).

**Can a stroke be re-rendered?** At the dab level, yes. `Stroke` (`lib.rs:1206-1222`) is self-contained. It stores:
- the `BrushSnapshot`, the full brush state (`lib.rs:800-837`);
- the points;
- `material_updates` boundaries;
- `alpha_locked`;
- the selection captured at contact start.

Variation is seeded as `mix_seed(brush.seed, stroke_id)` (`brush.rs:127-134, 981`) using integer hashes, which are exact on every platform. Textured tips and grain refer to content-addressed `AssetId`s that the project already embeds (`BrushTip::Mask`, `BrushGrain`, `lib.rs:414-420, 612-622`).

The limits on this determinism:
1. CPU dynamics use f32 `powf`, `exp`, `atan2`, `sin` and `cos`, which are not correctly rounded across the different libm implementations on each platform. Dabs match within one build and platform, and are only nearly identical across platforms.
2. GPU shading and blending are not bit-exact across vendors.
3. The engine's appearance changes deliberately between versions. `contact-brush-engine.md:25-27` says it "does not promise pixel identity with earlier contact rendering". `BrushSnapshot.schema_version` is 4 or 5, but there is no renderer or engine version.
4. Destination-aware execution classes depend on stroke order and on the existing pixels.

**The recording facility is for prediction research, not document replay.** `.capystrokes` files (`CAPYPEN2` framing, gzip plus bincode 2.0.1; `crates/layer-engine/src/recording.rs:14-18`, `recording/schema.rs`, `docs/development/stroke-recording.md`) record raw `PenEvent`s and the predictor's own log, capped at 10 minutes and 32 MiB. The artifacts in `artifacts/stroke-recording-web/*.capystrokes`, `artifacts/strokes/*` and `artifacts/pen-traces/*` are prediction datasets and analyses. They are still useful as physical-input fixtures for vector-stroke tests. `layer-bench` also generates synthetic Bézier strokes (`crates/layer-bench/src/main.rs:2287`) for benchmarks.

## 3. Document format and history

**Container.** A `.capy` file (`crates/layer-core/src/project_storage.rs:16-60`) starts with the magic `CAPYRASTER\x07\0`. It continues with a u64 metadata length, a SHA-256 digest, JSON metadata (`Manifest { document, tile_size, rasters, blobs, sources, tiled_sources, selections }`, marked `deny_unknown_fields`) and a payload of LZ4-per-tile blobs that are deduplicated by digest.

- `Document` and `Layer` metadata go into JSON through serde. That includes `BrushSnapshot`, `Selection` contours, `Figure` and effect WGSL.
- Big binary data is pulled out into indices. `SelectionIndex` (`project_storage/selections.rs:196`) is the model to copy for a future `VectorIndex`.
- The limits (`crates/layer-core/src/project.rs:78-97`) are 64 MiB of metadata, 16384 tiles and 4096 layers.
- The format is explicitly unstable. Version 7 readers also accept version 6, and anything older is rejected.

**History.** `Editor` (`lib.rs:1783+`) keeps `Edit` inverses. `Edit` has 15 variants (`lib.rs:1639-1682`): `SetRaster`, `ReplaceLayer`, `InsertLayer`, `Batch` and so on. There are no command replays. Raster history uses immutable tile revisions captured by GPU readback at contact boundaries, with unchanged tiles shared. Fills, figures and transforms are "transient submission commands; their recipes ... are not stored" (`project-format.md`). History is capped at 256 edits and 512 MiB.

History accounting (`HistoryEntry::new`, `lib.rs:1793-1850`) serializes each layer's metadata to JSON and multiplies by 4. Big shared content has to be charged by `Arc` identity, as selections already are (`lib.rs:1820`). Otherwise every edit to a vector layer would cost O(content) to add up.

**Where vector data fits.** Give `Layer` a new field such as `vector: Option<Arc<VectorContent>>`, or add a new `LayerKind::Vector` that carries it. The small parts (object list, styles, a table of deduplicated `BrushSnapshot`s, transforms) go in JSON. Point and control-point arrays go in LZ4 blobs through a `VectorIndex`. The raster revision stays as the persisted cache. Undo is `ReplaceLayer` with `Arc`-shared object lists, plus `SetRaster` to a pending revision. On the web, the worker protocol detaches binary data before transfer (`docs/development/binary-payloads.md`), so new payloads need the same treatment.

## 4. Tools, selection and geometry

**Tool framework.** All tools are shared Rust; hosts only render the resolved view models. There are two families:
- **Paint tools:** `Tool` (`crates/layer-ui/src/tools.rs:8-17`: Pen, Pencil, Brush, Eraser, Airbrush, Decoration, Blend, Liquify), each backed by `BrushSnapshot` presets (`crates/layer-core/src/presets.rs`, `contact_presets.rs`).
- **Canvas tools:** `LayerCanvasTool` (`crates/layer-ui/src/art_layers.rs:9-36`: Paint, Move, Transform, Select, Selection{kind}, SelectColor, LassoFill, Hand, pickers, Region, Gradient, Figure{shape, paint}, Ruler).

A gesture accumulates into `LayerInteraction.path: Vec<Point>` (`art_layers.rs:279-291`). On commit, it becomes a `LayerOperation` (`crates/layer-ui/src/figures.rs`). Tool options come from `tool_settings::controls` and `ToolSetView`. AGENTS.md requires parity on GTK, Web, Android, macOS, iPadOS and Windows. On-canvas direct manipulation is exempt from the hold-to-drag rules (`docs/internals/input.md`).

**Selection.** `SelectionShape` is either `Contours(Arc<[Arc<[Point]>]>)` (even/odd polylines) or `Pixels` (`crates/layer-core/src/selection.rs:205-218`). It has an affine and an inversion flag, and is built with `Selection::polygon` (`selection.rs:258`). Rectangle, Ellipse, Lasso and Polygon selections produce polygon contours. Contours are rasterized on the GPU by a compute pass that counts even/odd crossings: `selection_clip.rs:200-240` builds the edge list, and `selection_clip_init.wgsl` evaluates it at 4 coverage samples per pixel. This is the only general path-fill rasterizer in the codebase. It supports even/odd only (no nonzero rule), and 4 samples are too few for vector-quality anti-aliasing. "Selection to Path" and "Path to Selection" are listed as backlog items that need "a compatible vector editing model" (`docs/ui/selection-command-inventory.md:344`).

**Shapes.** `Figure { shape: Line|Rectangle|Ellipse, paint: Outline|Fill|Both, start, end, width, colors }` (`crates/layer-core/src/figures.rs:7-35`) renders as an analytic signed-distance function in `scene.wgsl` (`figure_color`, line 114; dispatched from `scene.rs:1341-1370`). It is then baked into pixels, and the recipe is not kept.

**Transform.** `Affine` and `ImageTransform { affine, interpolation }` (`crates/layer-core/src/affine.rs:13-46`) cover affine transforms only; there is no mesh or perspective warp. Raster transforms resample on the GPU (`paint_transform.rs`), and image layers keep a non-destructive `placement`.

**Text.** There is no text tool, no text layer and no font or shaping crate.

**Geometry code.** Only these exist:
- the polygon contour type;
- the analytic figure SDFs;
- contour tracing for tip outlines (`crates/layer-render/src/outline.rs`);
- Hermite curves for effect curves.

There are no Bézier types, no curve flattening, no stroke outlining, no boolean operations and no curve fitting.

## 5. Import and export

**Import.** `ImportSource::identify(prefix, intent)` (`crates/layer-ui/src/import_policy.rs:14-40`) is a two-way choice between a `.capy` project and a photo. Photos decode through `crates/layer-color/src/photo/*`: JPEG, PNG, TIFF, BMP, GIF, WebP, HEIF, AVIF and EXR. The result is a `SourceImage` that is placed as an `ImportedImage` layer. PSD, ORA, SVG and PDF are unsupported: "RAW, PSD, SVG, PDF, EXR/HDR ... require development, structured-document, rasterization or extended-range workflows" (`docs/ui/image-open-import-proposal.md:295`).

**Export.** Only flattened images are exported. `ExportFormat` (`crates/layer-ui/src/export.rs:9-20`) covers PNG, TIFF, JPEG, EXR and the HDR variants of PNG, JPEG and AVIF. Export goes through a GPU snapshot or readback.

**SVG today.** SVG appears only for UI icons (157–161 icons in `apps/layer-web/icons`, compiled natively per host) and for the web cursor overlay.

## 6. Rendering

- **Storage.** Sparse 256² pages per layer and mask (`PAGE_SIZE`, `crates/layer-render-wgpu/src/lib.rs:105`), allocated on first touch, with Float32 working attachments on the native path. The composite is the size of the document.
- **Invalidation.** Damage `Rect`s travel on each `DabBatch`. Scene caching tracks dependencies between clipping, masks and effects (`scene.rs`, `scene_images.rs`).
- **Display.** The viewport samples the composite. Zoomed-out views use retained mips and a detail atlas (`live_display.rs`, `display_mips.rs`). Zoomed-in views magnify level 0, so on-screen resolution is capped at the document's pixel resolution.
- **Generated content already has a pipeline.** Figures, fills and gradients are queued as `pending_operations`, and `CanvasEngine::apply_edit` turns them into `DabBatchKind::LayerOperation(i)` batches (`canvas.rs:768-830`). Setting the raster to `RasterRevision::pending()` makes the renderer rasterize the layer and read the changed tiles back into an immutable revision.

**Plug-in options for a vector layer:**
- **(A) Bake to document resolution (recommended).** Add a `LayerOperation`-style rebuild that clears the damaged region and replays the intersecting vector objects or strokes, in order and scissored to that region. Blend, clip, mask and filter behavior is then inherited unchanged.
- **(B) Resolution-independent display.** Crisp vector display at any zoom would need a composition pass in view space, because every blend and filter stage runs on document pixels. That is a deep change. A cheaper alternative is to draw editing overlays (anchors, handles, the path preview) at surface resolution through the existing overlay contracts. These are the `canvas_cursor` vector paths (`docs/ui/shared-ui.md:465-467`), `set_selection_outline` and `set_transform_preview` (`crates/layer-render/src/lib.rs` around line 575).
- **Export.** Re-rasterizing at a larger scale for export, or after a future Image Size feature, is a later extension of (A). `brush_to_layer` currently keeps the nominal footprint, so this would need a scale term.

## 7. Constraints

- **GPU.** A hardware GPU is required; there is no CPU paint fallback ("No brush pixel is rasterized on the CPU", `docs/reference/gpu-brush-engine.md`). Device limits are `wgpu::Limits::downlevel_defaults()` (`crates/layer-render-wgpu/src/lib.rs:1051`), and compute shaders are already used on all targets, WebGPU included.
- **Web.** WASM and WebGPU run single-threaded on the event loop: no blocking waits, and work proceeds in `requestAnimationFrame` slices. Distribution is built with a `web-release` profile using thin LTO.
- **Android.** Performance is GPU-bound. Wide brushes measured about 49 ms GPU median per update on Adreno (`docs/development/android-wide-brush-performance.md`). Replaying thousands of strokes has to be chunked and limited to the damaged region.
- **Licenses.** `deny.toml` allows Apache-2.0, BSD-2/3, MIT, ISC, Unicode-3.0 and Zlib, plus two pinned exceptions. It also rejects unknown registries and git sources. The workflow is `cargo deny --locked check licenses sources`, with `THIRD_PARTY_NOTICES.md` kept up to date (`docs/development/publication.md:44-62`).
- **Vendored code.** `vendor/` holds patched forks of wgpu, image-webp, rav1d and rust_h265, applied through `[patch.crates-io]` (`Cargo.toml:22-31`, `vendor/README.md`). `docs/development/vendored-code-audit.md` prefers registry crates over forks and is against adding a large dependency to replace a few lines.
- **Dependency footprint.** `layer-core` is deliberately small (serde, serde_json, lz4_flex, sha2 and half, some pinned with `=`).
- **Commit guide.** `docs/COMMIT_GUIDE.md` says to keep logic in shared Rust, replace obsolete paths instead of adding parallel ones, and measure brush and frame paths.

---

## What vector layers can reuse

1. `Stroke` plus `DabGenerator::generate`: deterministic, tested replay of brush strokes from raw samples, used as the "vector brush stroke" primitive.
2. The pending-operation raster pipeline (`LayerOperationKind`, `DabBatchKind::LayerOperation`, pending revisions, tile capture), which gives cached rasters, undo, saving and GPU recovery for free.
3. The `SelectionIndex`-style binary extraction and deduplicated, digest-checked LZ4 blobs in `.capy` files.
4. The GPU even/odd crossing rasterizer, as a starting point for path fills.
5. Canvas overlay contracts for drawing handles and paths at screen resolution.
6. Arc-shared immutable snapshots and identity-based history accounting.
7. Content-addressed brush assets that are already embedded in projects.
8. The capture fixtures and the `layer-bench` stroke workloads.

## What is missing

- A geometry library: Béziers, flattening, stroke expansion with joins, caps and dashes, hit-testing, bounding boxes, curve fitting and booleans. Candidates are `kurbo` (small, MIT/Apache) and `lyon` for tessellation. Both need a `cargo deny` check.
- A path renderer with good anti-aliasing and nonzero winding.
- A vector object model and a layer kind for it.
- An engine or renderer version tag for strokes.
- A spatial index and incremental re-render that is limited to damaged regions.
- Path tools: pen/Bézier creation, direct selection, anchor and handle editing, and a vector eraser.
- An SVG parser (`usvg`, which is large and needs fontdb and rustybuzz for text) and an SVG writer (hand-written, so no dependency).
- Host UI on all six clients.

## Risks

1. **Appearance drift.** Brush-engine changes re-render old strokes differently. Mitigation: treat the persisted raster as the truth, re-render only on explicit edits, and tag each stroke with the engine version.
2. **Cross-platform and cross-GPU nondeterminism.** f32 libm differences and GPU math mean re-renders differ slightly across devices. Tests need tolerances, and the cache must not be invalidated on open.
3. **Destination-aware brushes are order- and pixel-dependent.** Restrict vector layers to Dry and contact execution classes, and disable smudge, wet, watercolor and liquify on them.
4. **Replay cost.** Cost is O(total dabs), and ink accumulation is per stroke. This needs `Stroke.bounds` indexing, scissored regional replay and chunking across frames on web and Android.
5. **Size limits.** The JSON metadata limit (64 MiB), history accounting that re-serializes whole layers, and the 131072-point live input budget all need binary payloads and identity-based accounting.
6. **Stroke identity.** Randomness is keyed by `StrokeId`, so duplicate, paste and merge must keep each stroke's resolved seed.
7. **Editing time-based strokes.** Moving the control points of a stroke whose dynamics depend on speed or time (airbrush stationary samples) is ill-defined. Editing should be limited to transform, recolor, resize and delete, or points should be resampled with interpolated time.
8. **Display resolution.** Display stays at document resolution, and paper and grain textures live in document space. "Crisp at any zoom" is not achievable without composition in view space.
9. **Silent new-kind fallthrough.** A new `LayerKind` falls through about 169 equality checks, so each needs review.
10. **Stale docs.** README and documents.md describe persisted strokes. Fix them first so the design is not built on that assumption.

## Suggested phased plan

**Phase 0: Groundwork (shared Rust, no UI).**
- Correct the docs.
- Add `kurbo` (or an equivalent) to `layer-core` after `cargo deny` and a WASM size check.
- Define `VectorContent`, an `Arc`-shared object list whose objects are either `BrushStroke(Stroke + resolved seed + engine_version)` or `Path(BezPath, fill rule/paint, stroke style, transform)`.
- Add a `VectorIndex` to the project format (bump the magic to v8) and charge history by identity.

**Phase 1: Vector brush-stroke layer.**
- Add `LayerKind::Vector` and let `drawing_target` accept it for Dry and contact brushes.
- At pen-up (`canvas.rs:1427-1490`), append the completed `Stroke` to the layer's content in the same edit as the pending raster, keeping live rendering unchanged.
- Add a rebuild operation that clears the damaged region and replays the strokes intersecting it, in order and chunked.
- Add stroke-level operations: select, delete, recolor, change brush or size, transform (map points and scale diameter), and a stroke eraser that deletes whole strokes.
- Add tests asserting that rebuild dabs equal the original live dabs (extending `canvas.rs:3093-3180`).

**Phase 2: Bézier paths.**
- Add a GPU path fill: flatten with kurbo at document-pixel tolerance, then use a crossing and coverage compute pass with nonzero winding and more samples (or MSAA-tessellated triangles).
- Strokes use kurbo stroke expansion into fills.
- Add Pen and Direct Selection canvas tools, with overlays through the cursor and outline contracts.
- Add conversions: Selection ↔ Path, and "Stroke path with brush". The latter feeds sampled path points into `DabGenerator` to produce a Phase 1 stroke, which unifies the two models.
- Consider extending the `Figure` tools to emit editable paths.

**Phase 3: SVG.**
- **Export:** write paths directly. Brush strokes become variable-width outline approximations or embedded per-layer PNGs.
- **Import:** add an `ImportSource::Vector` variant that sniffs `<svg` or `<?xml`, parses with `usvg` on the file worker under ProjectLimits-style caps, and maps solid-paint paths to path objects. Rasterize unsupported features (filters, masks, patterns) or refuse them with a notice. Text needs fonts, which the web has no system source for.

**Phase 4: Extensions.**
- Re-rasterize at a larger scale for export or document resize.
- Edit stroke curves by fitting them with pressure profiles.
- Split strokes with the vector eraser.
- Evaluate crisp zoomed display only for layers at the top of the stack with Normal blend and no effects above them.
