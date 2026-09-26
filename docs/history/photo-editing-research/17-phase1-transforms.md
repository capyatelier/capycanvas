# Phase 1 audit: transform session, distort and warp

[Photo editing research](../photo-editing-research.md) · [Phase 1 plan](../../development/canvas-action-bar-transforms.md) · source report, 2026-09-26 · baseline `5eb45a47`

Read-only implementation audit made by an agent against baseline `5eb45a47` to prepare Phase 1. It covers the transform transaction, the preview and commit pipeline, selections after a non-affine commit, interpolation, placed photos, tests, and window blur. The [Phase 1 plan](../../development/canvas-action-bar-transforms.md) incorporates the findings and resolves the points where the audits differ. Line numbers can drift in later commits; verify before relying on one.

---

This audit changed no files. All paths are relative to `the repository root`.

## 1. UI transaction

**Current state**
- **Pose and handles.** `Pose{offset, scale:[f32;2], angle}` has no shear term (`crates/layer-ui/src/operation.rs:14-30`), and `affine()` is `Affine::around(pivot, scale, angle, offset)` (`crates/layer-core/src/affine.rs:58-76`).
  - `Handle{Move, Scale([f32;2]), Rotate}` is at `operation.rs:32-36`, and `Drag{handle, press, current, pose}` at `:38-43`.
  - `Transaction{placement, request: TransformPreview, revision, basis, bounds, pose, drag}` is at `:44-52`.
  - `Operation{current, serial, aspect, changed}` is at `:54-59`.
  - There are 8 box handles (`HANDLES`, `:93-102`).
- **Hit testing.** `hit()` checks, in order, the rotate knob, the nearest of the 8 handles within `reach`, then the inside of the box (`:593-608`).
  - `reach` is `ruler_reach()`, which is `HIT_DISTANCE * dpi / zoom` (`crates/layer-ui/src/rulers.rs:66-71`).
  - Touch reaches handles only while placing: `placement_touch_hit` (`operation.rs:510-515`) is consulted at `crates/layer-ui/src/session.rs:1138-1141`. Every other finger contact navigates.
- **Drag math** is in `drag_pose` (`operation.rs:609-694`):
  - Shift on Move locks the axis (`:615-621`). Shift on Rotate snaps to 15° (`:628-631`).
  - Scale keeps the opposite handle fixed, or the pivot with Alt (`:638-642`). Shift or `aspect` keeps proportions (`:655-668`), and scale is clamped to ±0.001…100 (`:669-675`).
  - Modifiers are tracked live only for Shift and Alt, by key name (`session.rs:945-958`). `Modifiers.command` exists (`crates/layer-ui/src/interaction.rs:7-12`) but is updated only from the host's modifier field. A modifier change re-runs `update_transform_drag` (`session.rs:968`).
- **Numeric fields.** `transform_controls` publishes X, Y (maps to `pose.offset`), Width and Height (`pose.scale`, with the aspect link) and Angle (`pose.angle`) (`operation.rs:360-432`).
  - Edits are routed through `SetToolSetting` ids starting with `transform_` (`session.rs:2892-2895`) to `set_transform_control` (`operation.rs:433-468`).
  - They are published whenever `operation.active()` (`session.rs:4589-4590`).
- **Overlay.** `append_transform_overlay` (`operation.rs:532-578`) draws a dashed box (marker 0), a solid stem to the rotate knob (marker 1) and 9 filled square handles (marker 2).
  - `CursorSegment` markers are: 0 dashed, 1 solid, 2 filled rectangle, 3 triangle, 4 cross, 5 sight, 6 prohibited (`crates/layer-render/src/lib.rs:35-48`). There is no circle marker.
  - The segment buffer grows as needed (`crates/layer-render-wgpu/src/present.rs:807-829`), so drawing a mesh is not capacity-bound.
  - It is called from `crates/layer-ui/src/art_layers.rs:2000`.
- **Preview.** `update_transform` sets `request.transform.affine = pose.affine(center(bounds))` (`operation.rs:335`).
  - For a placement it writes each member's `properties.placement` through `preview_edit(ReplaceLayer)` (`:336-350`).
  - Otherwise it calls `engine.set_transform_preview` (`:352-354`), which validates in `crates/layer-engine/src/canvas.rs:432-457`.
- **Routing.** `begin_transform` (`operation.rs:216-292`) sends a photo layer with no selection and no mask to `begin_layer_placement` (`:225-230`).
  - Otherwise it maps the selection into layer-local space (`:233-235`).
  - It computes `bounds` = `content_bounds` ∪ companion ∩ selection (`:236-271`). `content_bounds` is tile-granular and folds pending ops through `t.affine.bounds` (`:136-198`).
- **Placement** (`crates/layer-ui/src/operation/placement.rs`):
  - `Placement{original, members, rollback, insertion, selected}` is at `:7-13`.
  - Batches use identity pose and basis over `batch_bounds` (the union, `:35-41, 88-92`). Each member gets `orig.then(basis).then(A).then(basis⁻¹)` (`:21-33`).
  - The pose is decomposed from the placement (`:56-66`). A skewed placement is refused when recomposition differs (`:67-79`).
  - `aspect` is forced true for placements (`:123`).
  - `PlacementOriginalSize` resets the scale (`:184-209`).
  - Finish validates on a probe document and applies one edit (`:134-182`).
- **Commands**, with `TransformAspect` as the template:
  - enum `crates/layer-ui/src/lib.rs:606-610`; `ALL: [Self;126]` `:867`
  - `is_toggle` `:743`; `icon()` `:821-825`; `label()` `:1112-1116`
  - tooltip `crates/layer-ui/src/customization.rs:1037` (exhaustive match)
  - shortcut id `crates/layer-ui/src/shortcuts.rs:289-292`; defaults: Ctrl+T, Enter, Esc (`:361-363`)
  - disabled reason `crates/layer-ui/src/command_catalog.rs:692-697`
  - enabled `session.rs:1998-2002`; selected `:2116`; dispatch `:4017-4033`
  - `tool_actions` while active `:4517-4528`
  - Segmented choices come from `ToolActionGroup` (`crates/layer-ui/src/tool_settings.rs:17-50`), rendered as `ToolOption::Choice` (`crates/layer-ui/src/toolbar_components.rs:318-336`).
- **Icon test.** `lib.rs:1610` requires `icon()` to be `Some`, the name to be in the catalog list (`lib.rs:~429`), and `apps/layer-web/icons/layer-{icon}-symbolic.svg` to exist.
  - Icon files that already exist: `flip-horizontal`, `flip-vertical`, `rotate-left`, `rotate-right`, `reset`, `domain-warp`, `transform`.
- **Name clash.** `CommandId::FlipHorizontal/FlipVertical/RotateLeft/RotateRight` already exist as **view** commands (`lib.rs:636-639`; labels `:1142-1145`; camera-selected state `session.rs:2117-2118`). The new session commands need new IDs.
- **Existing tests:**
  - `operation.rs:723` `every_handle_keeps_its_anchor…` and `:782` `hit_testing_and_zero_crossing…`
  - `session.rs:7739` (masks/linked) and `:7827`. The latter asserts 5 tool settings and 9 marker-2 handles (`:7849, :7866`), so it will need updating.
  - `crates/layer-ui/src/session_source_tests.rs:2, 29, 49, 112, 169, 192, 255` (placement)
  - `crates/layer-ui/src/renderer_lifecycle.rs:524`
  - `crates/layer-ui/src/toolbar_component_tests.rs:122`
  - No test covers the skew refusal.

**Required changes**
- **(a) Shear.** Add `shear` to `Pose`, composed as `T(pivot+off)·R·Sh·S·T(−pivot)`.
  - Add `Pose::from_affine`, an exact decomposition: sx=|col1|, θ=atan2(b,a), sy=det/sx, k=(ac+bd)/(sx·sy).
  - Add `Handle::Skew(edge)`, driven by Ctrl or Distort-mode edge handles, and a "Skew" numeric field.
  - Replace the refusal at `placement.rs:56-79` with `from_affine`. Batches already compose any affine.
- **(b) Quad.** Add `Handle::Corner(i)` and `Handle::Edge(i)`, and a Perspective option (symmetric move of the opposite corner).
  - The overlay draws the quad. Hit testing checks corners, then edges, then the inside of the quad.
- **(c) Mesh.** Node and tangent handles sit ahead of Move in `hit()`. Mesh curves are tessellated into marker-0 segments; tangents are marker-1 stems plus a smaller marker-2 handle.
  - `Drag` must snapshot the whole geometry rather than `pose`.
  - Touch: generalize `placement_touch_hit` to every active transaction, per AGENTS.md: grab handles drag without a hold.
- **(d) Commands.** Add `TransformReset`, `TransformFlipHorizontal`, `TransformFlipVertical`, `TransformRotateLeft` and `TransformRotateRight`, implemented as `geometry.post(D)`. D is a document-space affine about the hull centre, conjugated by `basis`.
  - Reset should return to the session-start geometry. For placements, `Pose::identity()` is not the original, because the pose is absolute (`placement.rs:59-66`). This is a decision.
- **(e) Modes.** Add `TransformFree`, `TransformDistort`, `TransformWarp` and `TransformPerspective`, plus Warp grid choices. Put them in a new `ToolActionGroup::TransformMode` (segmented).
  - `TransformAspect` becomes Uniform, keeping its ID. Note that `aspect` persists across sessions today.

## 2. Preview and commit pipeline

**Current state**
- **Types.** `TransformPreview{transaction, layer, selection, transform: ImageTransform}` is at `crates/layer-render/src/lib.rs:467-473`.
  - `companion()` conjugates `from.then(A).then(to)` for a linked paint/mask pair (`:478-503`).
  - The trait method is at `:581-586`. The wgpu version stores it (`crates/layer-render-wgpu/src/lib.rs:3391-3404`), and it is applied at submit (`lib.rs:4447-4458`).
- **Two slots.** `PaintTransforms([ImageTransformState;2])` holds the primary and the companion (`crates/layer-render-wgpu/src/paint_transform.rs:9-18`). This is the "two targets" limit.
- **The pass is a render pipeline**, not compute. It draws a fullscreen triangle with a scissor per job, `LoadOp::Load`, `blend: None` (`crates/layer-render-wgpu/src/pixel_transform.rs:168-202, 637-658`).
  - The fragment inverse-maps `src = M·world + t` (`crates/layer-render-wgpu/src/pixel_transform.wgsl:46-53`) and does manual 4-tap bilinear through `selected()` (`:27-44`).
  - The uniform is 48 bytes: 2×2 linear, translation/origin, flags (1 linear, 2 identity, 4 placement), background (`pixel_transform.rs:98-110, 557-572`).
  - Group 1 holds 16 `textureLoad` views plus `SourceInfo` and the selection buffer (`:111-143, 789-798`). This is the "16-source-view split": `TRANSFORM_SLOTS=16` (`:7`).
- **Jobs.** `region_jobs` (`crates/layer-render-wgpu/src/paint_transform/snapshot.rs:137-252`) inverse-maps each destination page's corners with ±1 px and float error. It halves the region until the source footprint fits in 16 pages, with a 65,536-job cap (`:208`).
- **Read/write rule.** "Never samples and writes the same texture subresource" (`docs/reference/gpu-brush-engine.md:236-242`). Pages still used as sources are copied into captures before they are overwritten (`paint_transform.rs:515-518, 607-642`).
- **Preview cache.** Capture is reused while the transaction, layer and selection are unchanged; only old ∪ new regions are re-rendered (`paint_transform.rs:742-777`).
- **Commit** (`canvas.rs:472-506`) builds one `Transform` op per target, with `coverage.initial = selection`, and calls `append_operations(ops, Some(display_selection))`. That is one `Edit::Batch`, so one undo step (`:533-597`).
  - The renderer keeps the preview result when `matching_commit` finds `kind == Transform(preview.transform)` and `coverage.initial == preview.selection`. This is exact `PartialEq` (`paint_transform.rs:680-710`).
  - Otherwise it restores and replays (`lib.rs:3805-3813, 4140-4151`). `Scene::apply_operation` marks transforms `unreachable!` (`crates/layer-render-wgpu/src/scene.rs:1309-1311`).
- **Cut and place** (`pixel_transform.wgsl:27-62`):
  - Colour and selection are sampled together.
  - Colour: `moved + base·(1−sel)·(1−moved.a)`.
  - Scalar planes: `max(moved, remainder)`.
  - The visibility mask reverts the cut area to the mask default.
- **Affected area.** `affected_regions` returns `[source, affine.bounds(source ± pad)]` (`affine.rs:25-41`). Validation requires an invertible affine (`crates/layer-core/src/layers.rs:528-539`).
- **Planes resampled** (`paint_transform.rs:895-969`):
  - colour page `active()`: `Rgba8UnormSrgb` or `Rgba32Float`
  - material wetness and watercolor wetness (active ping-pong): scalar format `R8Unorm` or `R32Float` (`crates/layer-render-wgpu/src/pipeline_device.rs:86-92`)
  - the mask as a separate target, via the visibility variant
  - Stroke coverage and reservoirs are not transformed.
- **1 px support sites.**
  - `affine.rs:29`
  - `snapshot.rs:184-185`
  - `pixel_transform.wgsl:35`
  - `crates/layer-render-wgpu/src/material_sources.rs:155`
  - a fifth copy in `scene.wgsl:48-64` (±.5 at `:54`)
- **Serialization.** `ImageTransform` and `LayerOperationKind` derive serde, but every holder is `#[serde(skip)]`: `crates/layer-core/src/lib.rs:194-195` and `layers.rs:434-435`. `docs/reference/project-format.md:49-51` calls them "transient submission commands". They are applied and discarded, so changing `ImageTransform` needs **no `.capy` or recovery format step**.

**Changes for (a) projective**
- Add `Projective([f32;9])` and a `TransformMap` enum to `ImageTransform`.
- Grow the uniform (3×3 inverse, w-divide, reject w≤0). The 48-byte size is hard-coded at `pixel_transform.rs:106, 232-233, 484, 574`.
- `region_jobs`: corner-based footprints are exact only when w>0 on all corners. Subdivide or skip regions across the horizon.
- Generalize the IDENTITY checks (`pixel_transform.rs:569`; `paint_transform.rs:236`; `snapshot.rs:168`; `affine.rs:26`; `canvas.rs:480`), `affected_regions`, `LayerOperation` validation, the engine and wgpu preview validation, `companion()` (conjugation still works) and `content_bounds`.
- About 31 non-test sites in 13 files touch `transform.affine` or `ImageTransform {`.

**Changes for (b) mesh**
- **Forward rasterization fits better than per-pixel inverse mapping.** The pass is already a render pipeline, and brush pipelines are a vertex+fragment precedent (`lib.rs:6359-6411`).
  - Inverse mapping a Bézier mesh needs a per-pixel cell search plus Newton, and folds have several preimages.
  - Compute cannot portably write layer pages: `STORAGE_BINDING` only for `Rgba32Float/R32Float` (`lib.rs:5774-5780`).
- **Recommended variant: a UV-map prepass.**
  - Pass A rasterizes the tessellated mesh (destination position, source UV) into a scratch `Rg32Float` map for the job region. It is renderable and needs no blending; Float32 blending is optional (`pipeline_device.rs:78-85`).
  - Pass B is the existing fullscreen fragment, reading `source_position` from the map (with a sentinel for "uncovered") instead of `M·world`.
  - This keeps a single composite shader for the colour, scalar and visibility variants, and never samples and writes the same subresource.
  - Add a skirt of at least 1 px so edges stay soft as today. Folds are last-writer-wins, which is acceptable for Phase 1.
- `region_jobs` needs CPU triangle binning per destination page: footprint = union of the source bounding boxes of the overlapping triangles plus support.
- Add the new pipeline to on-demand shader preparation (`crates/layer-render-wgpu/src/startup.rs:1007` pattern).

## 3. Selection after commit

- **Today.** Selections are `Contours` (even/odd) or packed `Pixels`, plus affine metadata (`crates/layer-core/src/selection.rs:205-217`). `transformed()` only composes the affine (`:323-337`).
  - `display_selection` = `selection.transformed(A).transformed(basis)` (`canvas.rs:509-531`), and commit uses it (`:484`).
- **Contours.**
  - Under a homography, vertex-wise mapping is exact, because lines map to lines, provided w>0.
  - Under a mesh, map on the CPU: clip each ring to the mesh domain (convex clipping per ring preserves even/odd), densify to ~1 px spacing in source terms, then evaluate the forward patch. This is synchronous and keeps one undo step.
  - Inverted selections map their holes and keep `inverted`.
- **`Pixels`** (wand, feathered, painted) cannot stay affine metadata. There are two options:
  - **(i) GPU.** Extend `selection_resample.wgsl` (the affine compute resample, `crates/layer-render-wgpu/src/selection_clip.rs:76, 346-395`) with the projective or mesh map, then read back through `selection_readback::capture_selection` (`selection_readback.rs:5-80`, as used by `region_requests.rs:282`). Apply becomes two-phase: request, then commit on the result, still one edit.
  - **(ii) CPU.** Resample the packed words over the mapped bounds when Apply runs. This blocks for tens of ms on large selections.
  - **Recommendation:** (i). In the preview, hide the Pixels outline in non-affine modes, or show the hull.

## 4. Interpolation (XF-2)

- `Interpolation{Nearest, Linear(default)}` (`affine.rs:6-10`). No UI chooses it (`operation.rs:241, 279`), and placement hard-codes the default (`crates/layer-render-wgpu/src/scene/placement.rs:41-44, 93-106`).
- Nearest is `floor`; Linear is 4-tap bilinear (`wgsl:40-43`).
- Mips are display-only, and exact capture bypasses them (`scene.rs:1463-1466`).
- **Phase 1 minimum**, applied to paint/mask/pixel transforms:
  - **Bicubic** (Catmull-Rom or Mitchell): 16 taps, support 2 px. Clamp overshoot to the min/max of the 4 nearest taps, alpha to [0,1] and scalars to [0,1]; `R32Float` does not clamp on store.
  - **Jacobian-adaptive supersampling** for minification, up to 4×4 bilinear taps. The Jacobian is analytic for projective, or comes from UV-map derivatives for a mesh. This keeps the foreshortened areas of Distort and Warp from aliasing.
  - Default Distort and Warp to Bicubic, and expose an Interpolation choice.
- **Cost.**
  - Widen all 5 support sites together.
  - `original()` loops over up to 16 views per tap (`wgsl:14-25`), so bicubic is about 16×16 iterations worst case. Add a fast path when the whole tap block lies inside one view.
  - Wider footprints mean more jobs under the 16-view split. Watch the p99 budget of 8.333 ms.

## 5. Placed photos

- **No bake operation exists.**
  - `rasterize_source` re-encodes the source and leaves placement untouched (`crates/layer-color/src/rasterize.rs:1-2, 9-69`; commit `crates/layer-ui/src/source_edit.rs:248, 270-273`).
  - The only placement resampler writes disposable scene tiles (`scene/placement.rs:1-2, 83-150`).
  - Flatten creates a new document (`crates/layer-color/src/flatten.rs:1`).
  - Merge and Stamp do not exist.
- **What a bake needs:**
  - an op that keeps the source alive until submission (`lib.rs:1734-1744`)
  - compensation for linked masks (`layers.rs:567-587`)
  - an answer for off-canvas clipping to `local_extent` (`layers.rs:52-56`)
- **Admission.**
  - History is limited to 512 MiB (`crates/layer-core/src/history_budget.rs:6-7`). `RasterRevision::pending()` reserves 256 MiB (`crates/layer-core/src/raster.rs:23, 422-433`).
  - A `ReplaceLayer` that drops the source is admitted eagerly and refused over budget (`crates/layer-core/src/lib.rs:1702-1708, 1956-1960`).
  - Undo keeps the old `Arc<SourceImage>` alive (`lib.rs:1438`).
  - Large or HDR photos would often be refused at Apply.
- **Recommendation: (b) keep Distort and Warp disabled on placements and batches, with a reason.** Free, Uniform, Skew, Flip, Rotate 90° and Reset stay lossless through `placement`.
  - A no-cost route already exists: with a selection, a photo layer takes the destructive path in layer-local space (`operation.rs:225-235`). That path streams original tiles and is tested (`crates/layer-render-wgpu/src/paint_transform_tests.rs:1203`).
  - Suggested reason text: "Select an area to distort this photo's pixels".
  - A later option (c), better than (a): switching a single placement to Distort would restore the original placement and start a pixel transaction with T = new·orig⁻¹ as the starting pose. It keeps the source and needs no bake. Its risks are source-resolution preview cost (the photo benchmarks only print p95/p99) and clipping to `local_extent`.

## 6. Tests and qualification

- **Unit:** see §1. Also:
  - `affine.rs:153, 187`
  - `selection.rs:531`
  - `canvas.rs:2755, 2803, 2890, 2978`
- **GPU oracle** (`crates/layer-render-wgpu/src/pixel_transform_tests.rs`), with CPU premultiplied oracles at tolerance ≤2:
  - `:131, :282, :465, :806`
  - `transform_latency` `:652` is ignored and asserts p99 <8.333 ms (`:799`).
- **Paint transform** (`paint_transform_tests.rs`): replay comparisons at `:54, 407, 618, 658, 767, 852, 1203`. The ignored latency tests are `:979` and `:985` (asserting `:1198`).
- **Placement:** `crates/layer-render-wgpu/src/layer_tests/placement_tests.rs:443, 912, 1124`.
- GPU tests have no skip path. Run `cargo test --locked -p layer-render-wgpu <name> -- --test-threads=1`; benchmarks add `--release --ignored --nocapture` (`docs/development/testing.md:21-31, 66-69`).
- **GTK:**
  - `apps/layer-linux/src/tests.rs:3151` `native_operation_tool` (pen move/resize, angle, `tool-action-TransformAspect`)
  - `:11005` frame pacing, including a Transform workload
  - `apps/layer-linux/src/photo_workflow_tests.rs:175`
  - `apps/layer-linux/src/place_source_tests.rs:130, 228`
  - `apps/layer-linux/src/photo_drop_tests.rs:31, 242, 319`
  - Run with `GDK_BACKEND=wayland GSK_RENDERER=vulkan … cargo test --locked --release -p layer-linux <name> -- --ignored --test-threads=1` (`testing.md:81-88`).
- **Shared:** `cargo test --locked -p layer-core -p layer-engine -p layer-ui` (`testing.md:15`).

## 7. Window blur

- **Blur currently cancels the transaction.** `UiInput::Blur` calls `cancel_layer_gesture` (`session.rs:1184-1192`), which calls `cancel_transform` (`art_layers.rs:1867`). That cancels both pixel transforms and placements.
- **A second route also cancels it.** After a Blur that reports `cancel_paint`, GTK (`apps/layer-linux/src/input.rs:717-730`) and Web (`apps/layer-web/app.js:1560-1565`) send `PenPhase::Cancel`, and `transform_pen` then cancels the whole transaction (`operation.rs:501-503`).
- **Changes needed:**
  - Add `cancel_layer_contact()`, used by Blur, which for transforms only rolls back an active `drag` (restore its geometry snapshot, clear `drag`, update the preview).
  - Make `PenPhase::Cancel` do the same.
  - Keep full cancel for Escape (`session.rs:1030`), tool switches, `layer_pen` errors (`session.rs:3512-3516`) and `reconcile_transform` (`:4807`).
  - **Required dependency:** `suspend_renderer` relies on Blur to cancel the transform (`crates/layer-ui/src/renderer_lifecycle.rs:158`; the test at `:524-562` asserts this). GPU captures die with the renderer, so `suspend_renderer` must call `cancel_transform` explicitly. Apple and Android send Blur on detach (`apps/layer-apple/native/src/lib.rs:469`; `apps/layer-android/native/src/android.rs:781`).
- **Host reliance:**
  - No transform test asserts that Blur cancels.
  - Windows already works around the cancel to "preserve the idle placement" across IME and tablet activation (`apps/layer-windows/CanvasWindow.cpp:197-205`).
  - GTK blurs only when the window is inactive and focus is outside it (`input.rs:710`).

## Type sketch

```rust
// layer-core/src/affine.rs (+ warp.rs)
pub struct Projective(pub [f32; 9]);            // forward, row-major
impl Projective { fn from_affine(Affine)->Self; fn rect_to_quad(Rect,[Point;4])->Option<Self>;
  fn map(self,Point)->Option<Point>; fn inverse(self)->Option<Self>; fn then(self,Self)->Self; }
pub struct MeshMap { pub frame: Affine /*unit square→source*/, pub cells: [u16;2],
  pub net: Arc<[Point]> /*(3c+1)×(3r+1) tensor Bézier, destination*/ }
impl MeshMap { fn map(&self,Point)->Option<Point>; fn tessellate(&self,tol:f32)->Tessellation;
  fn bounds(&self)->Rect /*control hull*/; fn post(&self,Affine)->Self; }
pub enum TransformMap { Affine(Affine), Projective(Projective), Mesh(Arc<MeshMap>) }
pub struct ImageTransform { pub map: TransformMap, pub interpolation: Interpolation } // loses Copy
impl ImageTransform { fn is_identity; fn validate; fn affected_regions; fn conjugate(to:Affine) }

// layer-ui/src/operation.rs
struct Pose { offset: Point, scale: [f32;2], angle: f32, shear: f32 }
enum TransformMode { Free, Uniform, Distort { perspective: bool }, Warp { cells: [u16;2] } }
enum Geometry {
  Pose(Pose),
  Quad([Point;4]),                                   // image of bounds TL,TR,BR,BL
  Mesh { mesh: Arc<MeshMap>, outer: Projective },    // outer = Free/Distort edits after Warp
}
impl Geometry {
  fn to_quad(&self,b:Rect)->Self;          // Pose→exact corners; Mesh keeps mesh, edits outer
  fn to_mesh(&self,b:Rect,cells)->Self;    // from H: exact nodes, approx tangents; bakes outer
  fn post(&self,d:Affine)->Self;           // flip/rot90, exact for all kinds
  fn map(&self,b:Rect)->TransformMap; fn hull(&self,b:Rect)->[Point;4];
}
enum Handle { Move, Scale([f32;2]), Rotate, Skew(usize), Corner(usize), Edge(usize),
  Node(u32), Tangent{ node: u32, side: u8 } }
struct Drag { handle: Handle, press: Point, current: Point, start: Geometry }
struct Transaction { placement, request, revision, basis, bounds, mode: TransformMode,
  start: Geometry, geometry: Geometry, drag: Option<Drag> }
```

## Risks

- **Many jobs under strong perspective.** Deep perspective increases the source footprint and the job count, and can hit the 65,536 cap (`snapshot.rs:208`). Require a convex quad and prefilter minification.
- **No blending.** Float32 blending is not portable, so the mesh composites in the shader and folds are last-writer-wins.
- **Performance.** Wider kernels multiplied by the 16-view loop threaten 8.333 ms.
- **Pixels selections need async Apply.**
- **Losing Copy** on `ImageTransform` ripples through about 31 sites.
- **Decisions:**
  - whether grid "3×3" means cells or points
  - Reset semantics for placements
  - whether flip and rotate act on document axes or the box's own axes
  - whether Warp→Free keeps the mesh (research doc: keep it, with outer handles on the hull)

## Commit-sized steps (shared Rust + GTK first)

1. **Shear.** `Pose.shear`, `from_affine`, Skew handle and field; lift `placement.rs:67`. Extend the `operation.rs` tests and `session.rs:7827` counts; add a skewed-placement session test.
2. **Session commands.** Reset, flips and rotate 90° via `post` (reusing existing icon files), wired through `lib.rs`, customization, shortcuts, catalog and session. Test identities (flip twice, rotate ×4), placement losslessness and one undo step. Update the GTK `native_operation_tool` scenario.
3. **Blur keeps the transaction.** Contact-only cancel; `PenPhase::Cancel` rolls back the drag; `suspend_renderer` cancels explicitly. Add unit tests plus a GTK check.
4. **Mode model.** `ToolActionGroup::TransformMode` with Free and Uniform (`TransformAspect`); Distort and Warp listed but disabled with a reason.
5. **`TransformMap` refactor.** Affine variant only; no behaviour change, all suites green.
6. **Projective in layer-core.** Map, bounds, validity, conjugation and contour mapping; CPU tests.
7. **Projective in the renderer.** wgpu uniform, shader, `region_jobs`; oracle tests (projective, affine-as-projective equivalence, seamless regions, latency).
8. **Distort mode in the UI.** Quad handles, Perspective, conversions, overlay, contour selection mapping; disabled for placements. GTK scenario.
9. **Selection pixels via the GPU.** `Pixels` selections through the resample compute and readback, with a two-phase Apply.
10. **XF-2 interpolation.** Bicubic, clamps, the 5 support sites, adaptive supersampling, Interpolation choice. Oracle tests and latency.
11. **Mesh in layer-core.** `MeshMap`, tessellation and seeding; CPU tests: an affine seed is exact; a quad seed is within tolerance of H.
12. **Mesh render pass.** UV-map prepass plus triangle binning; oracle tests against the CPU tessellation, and latency.
13. **Warp mode in the UI.** Grid presets, node and tangent handles, outer handles on the hull, finger handles. GTK scenario.
14. **Other hosts.** Web, Android, Apple and Windows projections, then the canvas action bar work (BAR-0/BAR-2).
