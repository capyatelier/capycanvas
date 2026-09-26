# Implementation audit: geometry and transform (GEO, XF)

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-25 · baseline `dac76c20`

Read-only implementation audit made by an agent against baseline `dac76c20`. It checks each item in the first draft of the build list against the code, the platform hosts and the design records. "Plan line N" refers to that superseded first draft; the current [research record](../photo-editing-research.md) incorporates the corrections. Line numbers can drift in later commits; verify before relying on one.

---

Scope: `docs/history/photo-editing-research.md` checked against baseline `dac76c20`. All paths are relative to `the repository root`.

**Four findings change the plan's direction:**
1. **Skew already works everywhere except the handles.** The model, shader and renderer take a full affine matrix. Only the transform handles and their pose lack a shear term.
2. **Paint layers already have a lossless placement matrix.** `LayerProperties.placement` applies to painted pixels as well as photo sources. However, a paint layer's editable area is tied to the canvas size, and that is the real blocker for a non-destructive crop or resize.
3. **Liquify Reconstruct is not implemented.** The renderer rejects it. This invalidates the Reconstruct part of XF-5, and T-4's claim that the engine already implements it.
4. **Image-wide pixel rewrites already have a template.** Convert Color Space and Change Bit Depth run on a background CPU worker, can be cancelled, and are adopted atomically. The plan assumes a GPU resample kernel that does not exist.

## Inventory rows (section 1)

**Geometry row — Wrong or imprecise claim**
- **Evidence:**
  - "Export can shrink to fit a box" is wrong. `ExportSize::Fit { bounds, enlarge }` is at `crates/layer-ui/src/export.rs:98-105` and 125-129.
  - The export dialogs already offer "Allow enlargement": `apps/layer-linux/src/files/export.rs:212-217`, `apps/layer-apple/Shared/Editor/ExportForm.swift:123` and `apps/layer-android/app/src/main/java/art/capycanvas/ExportDialog.kt:35`.
  - Enlargement uses Catmull-Rom cubic (`crates/layer-color/src/resize.rs:1-3` and 48-71).
  - The zoom range of 2% to 1600% is correct (`crates/layer-ui/src/camera.rs:43` and 79).
  - The view commands are already labelled "Rotate view 90° left" and "Flip view horizontally" (`crates/layer-ui/src/lib.rs:1132-1135`).
- **Correction:** say "Export can fit a box, optionally enlarging (Catmull-Rom); the document itself cannot be resized."

**Transform row — Partially wrong**
- **Evidence:**
  - Groups can already be translated. The Move tool allows every kind except Background (`crates/layer-ui/src/art_layers.rs:110`). `move_target_edit` changes the group offset (`crates/layer-core/src/layers.rs:1108-1128`), and child offsets add up (`layers.rs:5-27`).
  - Several layers already transform together for a freshly imported batch: `Placement.members`, `preview_layers` and `batch_bounds` at `crates/layer-ui/src/operation/placement.rs:7-41` and 85-92.
  - The "W/H" fields are percentages, not pixels (`crates/layer-ui/src/operation.rs:369-376` and 392-405). The fields are defined at `operation.rs:360-432`, not line 444.
  - Negative percentages already flip a layer (`operation.rs:459-464` validates the absolute value).
  - On Web, Ctrl+T is reserved by the browser (`crates/layer-ui/src/shortcuts.rs:121-126`).
- **Correction:** "Groups can be moved but not scaled or rotated. A multi-layer transform exists only for an import batch. A layer can be flipped by typing -100%."

**Summary item 4 — Wrong in part**
- "Liquify … two of its eight engine modes" should read "seven implemented modes". Reconstruct is refused by the GPU with `UnsupportedBrushFeature("liquify reconstruct snapshot")` at `crates/layer-render-wgpu/src/lib.rs:1636-1642`.
- In the shader, mode 7 falls into the same branch as Edge (`material_brush.wgsl:489-492`; `material_sources.rs:141`).

## Geometry (GEO)

### GEO-1 Crop tool — Overlooked existing infra, and the model conflicts with an invariant

**Evidence:**
- **Editable area is tied to the canvas.** A paint layer's editable extent equals the canvas size (`Layer::local_extent`, `crates/layer-core/src/layers.rs:52-56`). Tile coordinates are `u32` (`crates/layer-core/src/raster.rs:104-106`). `RasterData::validate_index` rejects tiles beyond that extent (`raster.rs:362-367`). About 63 consumers of the extent exist across the renderer, engine and project storage.
  - Shrinking width or height therefore invalidates existing paint tiles.
  - Content left of or above the origin cannot be stored in layer coordinates.
  - As written, "Delete cropped pixels: off" cannot work for paint layers.
- **Translation machinery already exists:**
  - Per-layer `properties.offset` (`layers.rs:398-415`), with group offsets adding up.
  - Mask `offset` and `placement` (`layers.rs:417-438` and 567-576).
  - `Selection::translated` and `transformed` (`crates/layer-core/src/selection.rs:314-335`).
  - `RulerGeometry::translated` (`crates/layer-core/src/rulers.rs:74-81`).
  - Composition already handles arbitrary integer offsets, touching at most four source tiles per output tile (`crates/layer-render-wgpu/src/scene.rs:861-863`).
- **The renderer can change document size**, but only up to the GPU texture limit (`crates/layer-render-wgpu/src/lib.rs:1683-1713`; the limit check is at 1692-1695).
- **Composition and capture stop at the canvas edge** (`scene.rs:1467` and 1712-1726). The crop preview cannot currently show hidden off-canvas content.
- **Photos already keep off-canvas pixels** ("Canvas edges hide content without deleting it", `docs/ui/image-open-import-proposal.md:55-58` and 96-99).
- **Apply/Cancel already exist:**
  - Enter and Escape are bound to `ApplyTransform` and `CancelTransform` (`shortcuts.rs:220-221`).
  - They are shown as completion buttons in the tool options on every host (`crates/layer-ui/src/session.rs:4478-4491`; `crates/layer-ui/src/toolbar_components.rs:256-262`).
  - Cancel is allowed without a renderer (`crates/layer-ui/src/renderer_lifecycle.rs:111-114`).
- **Ratio and size entry already exist** for the rectangle selection: Free/Ratio/Size constraints, Shift for square, Alt from centre (`crates/layer-ui/src/selection_tools.rs:61`, 80-96 and 179-196).
- **Overlay primitive:** `CursorSegment` supports only lines and markers (`crates/layer-render/src/lib.rs:35-48`). There is no shaded fill for dimming outside the crop. The overlay is drawn in `present.wgsl:250-264`.
- **Touch:** finger contacts reach handles only during a photo placement (`operation.rs:508-515`; `session.rs:1109-1112`).

**Suggested correction:**
- Replace "prefer a canvas-origin offset" with an explicit decision:
  - (a) store a per-layer extent so that `local_extent` stops following the canvas, which needs a `.capy` version step; or
  - (b) rewrite tiles for paint layers and use offset or placement metadata only for photos, selections, masks and rulers.
- Only root layers' offsets need shifting.
- Crop preview: add off-canvas composition or declare that hidden paint is not shown.
- Reuse `SelectionOptions` for ratio and size, the `ApplyTransform`/`CancelTransform` pattern, and `placement_touch_hit` routing.
- Add a new overlay marker for the dimmed shield.

### GEO-2 Straighten — Overlooked existing infra

**Evidence:**
- A numeric angle field already exists (`operation.rs:406-419`), and Shift snaps rotation to 15° (`operation.rs:628-631`).
- A Straight ruler already stores a document-space line (`rulers.rs:15-20`), which could serve as the "measure then straighten" line (the GIMP workflow).
- Masks and saved selections can rotate as metadata (`LayerMask.placement`, `Selection.affine`), so no resampling is needed for them.
- Paint layers could also rotate as metadata via `properties.placement`, but that runs into the extent limit described under GEO-1.
- Committed transforms for several layers already form one undo step: `CanvasEngine::append_operations` (`crates/layer-engine/src/canvas.rs:534-597`).
- That function refuses locked layers and non-Paint owners (`canvas.rs:556-561`).

**Suggested correction:**
- Straighten can reuse a Straight ruler as its line.
- Apply through `append_operations` for paint pixels and through metadata for masks, selections, photo placements and rulers.
- Rulers need an affine map; build it from `handles()` and `from_drag`.
- Define what happens to locked layers.

### GEO-3 Canvas Size, Reveal All, Trim, Crop to Selection — Imprecise, with name conflicts

**Evidence:**
- **Name clashes:**
  - "Reveal all" is already the mask command (`art_layers.rs:1545`).
  - "Fit canvas" is already the view command (`lib.rs:1129`).
  - The selection inventory names the command "Crop Canvas to Selection…" and places it in a **Document** menu. It defines the behaviour: crop to the bounding box of nonzero coverage; holes and soft edges do not erase; saved masks are affected (`docs/ui/selection-command-inventory.md:134` and 285).
- **Adding an Image menu:**
  - The fixed menu order is File, Edit, Layer, Select, Filter, View, Window, Help (`docs/familiar-workspace.md:90`).
  - That set was checked to fit at 1200 px (`docs/development/workspace-header-progress.md:129`).
  - `ApplicationMenu::ALL` has 8 entries (`crates/layer-ui/src/application_menu.rs:45-54`) and is projected by GTK, Web `header.js`, Android `WorkspaceHeader.kt` and Apple `CatalogMenuItems.swift`.
  - Assign Profile, Convert Color Space and Change Bit Depth currently sit in Edit (`lib.rs:223-232`).
- **Bounds:**
  - `content_bounds` is tile-granular (256 px) and clipped to the canvas (`operation.rs:133-198`).
  - Exact coverage bounds already come back from GPU readback (`crates/layer-render-wgpu/src/selection_readback.rs:56-64`). They are reachable through `SelectionAction::LoadCoverage` for layer alpha or masks (`crates/layer-ui/src/selection_masks.rs:73-77` and 987-996).
  - Contour selections pad their bounds by 1 px (`selection.rs:283-313`).
  - A wand region can sample the visible image (`RegionSource::Visible`, `art_layers.rs:37-44`), which suits Trim by corner colour.
- **Paper:** the background is a procedural layer with no raster (`crates/layer-core/src/lib.rs:1334-1349`; `paper_color` at `layers.rs:410-411`). Extending the canvas extends the paper automatically.
- **Size limits:**
  - Projects: 32,768 px per axis, 16,384 tile instances, 1 GiB raster (`crates/layer-core/src/project.rs:88-95`).
  - New documents: 8,192 px (`crates/layer-ui/src/document_files.rs:133`).
  - The GPU texture limit also applies (`lib.rs:1692-1695`).

**Suggested correction:**
- Rename to "Crop Canvas to Selection…" and adopt the inventory's semantics.
- Rename "Reveal All" to something like "Expand Canvas to Layers".
- Either add a Document/Image menu and requalify header width, or put these under the existing section in Edit. Moving Assign/Convert/Change Bit Depth there too would be consistent.
- Trim: use exact readback for transparent borders and a Visible-source wand for corner colour.
- Reveal All: use exact placement bounds for photos and tile unions for paint (up to 255 px over).
- Validate every result against project, new-document and GPU limits so the saved file reopens.

### GEO-4 Image Size — Overlooked existing infra, with wrong details

**Evidence:**
- **Whole-document rewrite template:**
  - `layer_color::prepare_document_color` (`crates/layer-color/src/document.rs:1-2`, 47-60 and 268) is a worker-owned, cancellable job with a 512 MiB limit.
  - GTK drives it with a comparison preview (`apps/layer-linux/src/files/color.rs:12` and 61-69).
  - It produces `Edit::SetColor`, which requires history admission (`crates/layer-core/src/lib.rs:1698-1715`).
  - Undo and Redo are prepared through `Editor::prepare_color_transition` and `commit_color_transition` (`crates/layer-core/src/color_history.rs:34-100`), with engine glue in `crates/layer-engine/src/color_transition.rs`.
  - `RowResampler` already provides Area reduction and Catmull-Rom enlargement, with alpha clamped and colour rescaled (`resize.rs:27-71` and ~149-160). It already runs on Web too (`apps/layer-web/src/output.rs:363`).
  - No "GPU resample kernel shared with XF-2" exists.
- **Metadata instead of resampling:** saved and current selections scale through `Selection.affine` (`selection.rs:214-218` and 321-335). Masks scale through `LayerMask.placement`.
- **Vignette needs no scaling.** Its radius and centre are percentages of `fx_extent()` (`assets/filters/filter_library.wgsl:101-105`; the manifest vignette entry has no px parameters). Only manifest parameters with `unit: "px"` need scaling. That covers:
  - sigma, radius or size in Gaussian, Unsharp Mask, High Pass, Denoise, Edge Detect, Emboss, Bloom, Soft Focus, Pencil, Film Grain, Halftone and Pixel Mosaic;
  - distance, wavelength, scale or spacing in Motion Blur, Crosshatch, Chromatic Aberration, Painterly, Ripple, Glass, Rainy Glass, VHS, CRT, Heat Haze, Iridescence and Domain Warp.
  - Manifest maximums apply, for example Gaussian sigma at most 21.
- **Resolution:** `Document.resolution` exists, but no `Edit` changes it (`lib.rs:1642-1685`). Document Properties is read-only (`crates/layer-color/src/document_info.rs:12-40`; `customization.rs:985`).
- **Units:** `NumericControl` has only a unit label, with no px/cm/in conversion (`crates/layer-ui/src/numeric.rs:43` and 75).

**Suggested correction:**
- Build on the SetColor-style prepare/commit transition with the `RowResampler` worker, or justify a new GPU path.
- Scale selections and masks by metadata.
- Scale only `unit:"px"` parameters, clamped to their manifest maximums. Remove vignette from the list.
- Add a `SetResolution` edit, a unit-conversion control, and validation against the limits listed under GEO-3.

### GEO-5 Rotate and flip the image — Imprecise

**Evidence:**
- Flipping does not swap width and height; only 90° and 270° rotations do.
- The view labels already say "view" (`lib.rs:1132-1135`).
- The names `flip_horizontal` and `rotate_left`, the shortcut IDs `command.FlipHorizontal` (derived from the variant name at `shortcuts.rs:196-199`), and the flip/rotate icons are taken by the view commands.
- Exact 90° turns and flips are possible with `Interpolation::Nearest` through the existing transform, which has oracle tests including flips (`crates/layer-render-wgpu/src/pixel_transform_tests.rs:282-345`).
- `append_operations` batches all layers into one undo step (`canvas.rs:534-597`).
- A 90° turn of a non-square canvas hits the editable-extent limit from GEO-1.

**Suggested correction:**
- "Rotate 90° and 270° swap the canvas dimensions; flips do not."
- Require exact, non-resampling permutations.
- Create new CommandId names (for example `FlipImageHorizontal`) and new icons.
- Flips (no size change) can ship first using `append_operations` plus metadata. Rotations need the GEO-1 extent decision.

### GEO-6 Perspective crop — Accurate

- There is no projective transform or homography anywhere (a search of `crates` found none).
- The constraints listed under XF-1 apply.

## Transform (XF)

### XF-1 Skew, distort and perspective — Wrong or imprecise: skew needs no model change

**Evidence:**
- **Skew is supported everywhere but the handles:**
  - `Affine` is a full 2×3 matrix (`crates/layer-core/src/affine.rs:44-46`).
  - The shader uses a full 2×2 linear part (`crates/layer-render-wgpu/src/pixel_transform.wgsl:3` and 51-53).
  - Preview-level selection accounts for shear (`crates/layer-render-wgpu/src/scene/placement/mips.rs:55-62`).
  - Skewed placement already passes snapshot and brush tests, with mean channel error under 2.5/255 (`docs/development/image-placement-gtk-progress.md:922` and 1016-1018).
  - Only `Pose { offset, scale, angle }` and `Handle` lack shear (`operation.rs:14-36`).
- **The plan names the wrong type.** Photo placement is `LayerProperties.placement: Affine` (`layers.rs:398-404`), not `ImageTransform`. Placement previews go through `Edit::ReplaceLayer` (`operation.rs:336-350`), not `TransformPreview`.
- **Everything assumes an affine matrix**, so perspective would need changes in all of these:
  - brush mapping `DabStyle.brush_to_layer` (`crates/layer-render/src/lib.rs:148`; `crates/layer-engine/src/canvas.rs:1329-1334`);
  - `Selection.affine`;
  - `LayerMask::transform_in_parent` (`layers.rs:567-576`);
  - `region_jobs` (`crates/layer-render-wgpu/src/paint_transform/snapshot.rs:138-195`);
  - preview level selection (`mips.rs:55-62`);
  - `TransformPreview::companion` (`crates/layer-render/src/lib.rs:476-505`).
- **Modifiers:** only Shift and Alt are tracked live on key press (`crates/layer-ui/src/session.rs:939-950`). Ctrl/Cmd is not, which breaks the documented rule "Modifier changes apply without another pointer movement" (`docs/familiar-workspace.md:1253-1254`). Pointer input carries no modifiers.
- **Mode chips:** the existing Tool Set groups "Move" and "Scale / rotate" (`operation.rs:104-131`) are the natural place for them.

**Suggested correction:**
- Split into XF-1a Skew and XF-1b Distort/Perspective.
  - Skew: add shear to `Pose`, add skew fields, remove the refusal at `placement.rs:67-79`. No file-format or shader change.
  - Distort and perspective: generalize `LayerProperties.placement` and every consumer above. Decide whether painting on a perspective-placed layer is allowed.
- Track Ctrl and Meta the same way Shift and Alt are tracked at `session.rs:939-950`.
- Put the mode chips in the Operation Tool Set.

### XF-2 Resampling quality — Partially implemented already

**Evidence:**
- **Already in place:**
  - The transform works on linear, premultiplied Float32 (`crates/layer-render-wgpu/src/pixel_transform.rs:1-2` and 270); so does the CPU resizer (`resize.rs:1`).
  - The CPU resizer already clamps overshoot (`resize.rs:~149-160`).
  - Photo placement already has 2×2 area preview levels (`display_mips.wgsl:7-30`; `mips.rs:55-119`).
- **Gaps:**
  - Those preview levels are display-only. Export and flatten bypass them (`mips.rs:1-2`; `scene.rs:1463-1466`), so downscaled photos are sampled bilinearly at full resolution and alias.
  - Placement always uses the default (Linear) filter and ignores `Interpolation` (`crates/layer-render-wgpu/src/scene/placement.rs:93-96` and 103-106).
- **Places where the 1 px sampling support is hard-coded:**
  - `affine.rs:29`;
  - `region_jobs` ±1 (`paint_transform/snapshot.rs:184-185`);
  - the shader edge test ±0.5 (`pixel_transform.wgsl:35`);
  - the Liquify sampling bounds (`material_sources.rs:~158`).
- **Other constraints:** at most 16 source views per region (`pixel_transform.rs:7`). The same resampling code serves the R8 wetness and visibility pipelines (`pixel_transform.rs:79-83`), so overshoot must be clamped to [0,1] there too.
- Interpolation is not stored in the project; transforms are transient (`docs/reference/project-format.md:49-52`).

**Suggested correction:** list all four support sites and the 16-view split. Add prefiltering to exact capture of placed photos, not only to transform Apply. Include the scalar and visibility pipelines. Adding Interpolation variants needs no file-format step unless placement starts storing an interpolation choice.

### XF-3 Warp and mesh transform — Accurate that nothing exists; missing constraints

**Evidence:**
- No mesh, warp or triangle-rasterization code exists.
- `PixelTransform` is a full-screen inverse-mapping pass with an affine inverse (`pixel_transform.wgsl:10-13` and 45-63).
- Storing a mesh in placement breaks the same affine-only consumers listed under XF-1.
- The transaction, Apply/Cancel and one-undo-step pattern is reusable (`operation.rs:293-330`; `canvas.rs:473-513`).
- The interactive preview is limited to two targets (`crates/layer-render-wgpu/src/paint_transform.rs:9`).

**Suggested correction:** state that this needs a new forward-rasterizing pass, and that a mesh stored in placement needs either a separate warp stage applied after the layer's own pixels, or painting disabled on warped layers.

### XF-4 Transform ergonomics — Partially implemented already, and imprecise

**Evidence:**
- **"Visible Apply/Cancel on touch" already exists** as tool-option completion actions (`session.rs:4478-4491`; `toolbar_components.rs:256-262`).
- **Finger handles are missing:** only photo placement accepts finger touch on handles (`operation.rs:508-515`).
- **Groups and several layers:** group translation exists via the Move tool, and import batches move together (see the Transform row).
  - Groups cannot take a placement (`layers.rs:1086-1088`).
  - `target_transform` adds up only parent offsets, not parent placements (`layers.rs:32-39`).
  - The Move tool moves only the active layer.
  - `can_transform` checks only the active layer. It never looks at the other selected layers; it simply ignores them (`operation.rs:201-215`).
- **Pivot:** always the box centre (`operation.rs:27-29` and 335).
- **Bounds** are conservative, not pixel-tight (`operation.rs:133-135`; `docs/familiar-workspace.md:1304-1305`). This undermines reference points and edge snapping.
- **Snapping:** rulers snap brush strokes only (`crates/layer-ui/src/rulers.rs:72-75`). `choose_ruler` and `RulerConstraint::project` (`rulers.rs:113-121` and 161) could be reused.
- **Arrow keys** are valid shortcut keys (`shortcuts.rs:76-79`) but are used only for divider nudges (`session.rs:1062-1077`). Key repeat is enabled only for Undo/Redo (`shortcuts.rs:284`). History caps at 256 edits (`project-format.md:124`).
- **"Transform Again" (Ctrl+Shift+T)** is reserved on Web (`shortcuts.rs:121-126`).
- **Move also edits rulers** (`familiar-workspace.md:1256`).

**Suggested correction:**
- Remove Apply/Cancel from the new work and add finger-touch handles for every transform.
- Multi-layer and group transforms: generalize `Placement.members` and `append_operations`, add parent-placement composition, and extend the preview beyond two targets.
- Nudge: register with key repeat enabled and merge repeated presses into one undo step.
- Choose a Web-safe binding for Transform Again.
- Add pixel-tight bounds before reference points and snapping.

### XF-5 Liquify upgrade — Wrong or imprecise

**Evidence:**
- Reconstruct is rejected by the renderer (`crates/layer-render-wgpu/src/lib.rs:1636-1642`), so it is new work that needs an original snapshot.
- **Freeze already works:** Liquify respects the soft selection (`material_brush.wgsl:159-164`), which is tested for LiquifyPush and LiquifyTwirl (`crates/layer-render-wgpu/src/layer_tests.rs:1579-1640`). Quick Mask and the Selection Brush already act as a freeze mask.
- `BrushDeform` has pressure, momentum and distortion (`crates/layer-core/src/lib.rs:730-747`), but only strength is exposed (`crates/layer-ui/src/tool_settings.rs:255-261`). Crystals depends on distortion (`material_sources.rs:137-140`).
- Liquify already works on placed photos for every mode except Reconstruct (`crates/layer-render-wgpu/src/placement_tests.rs:212-230`).
- "Show Backdrop" is a Photoshop dialog feature; on-canvas Liquify already shows the other layers.
- Re-editable liquify would need a new raster plane (current planes at `raster.rs:78-83`) or an image-valued effect parameter (none exist: `crates/layer-core/src/effects.rs:207-222`), plus a file-format step.

**Suggested correction:** mark Reconstruct as new renderer work (this also fixes T-4). Replace "Freeze/Thaw painting" with "a dedicated freeze mask only if it must be separate from the selection". Expose momentum and distortion. Drop Show Backdrop.

### XF-6 Non-destructive transform for paint layers — Wrong or imprecise

**Evidence:**
- **"Rasterize Source (existing) bakes it" is wrong.** Rasterize Source converts the retained original to document pixels and keeps placement (`docs/reference/project-format.md:31-35`; `customization.rs:996`). No command bakes placement into pixels.
- **Paint layers already have lossless placement:** `properties.placement` covers "local source AND raster pixels" (`layers.rs:401-404`), and Paint layers are allowed to carry it (`layers.rs:1087-1088`).
- **The limit:** a paint layer's editable extent equals the canvas (`layers.rs:52-56`), so painting outside it after a downscale is clipped.

**Suggested correction:** redefine XF-6 as "Scale/Rotate a paint layer without a selection edits its placement". Solve the editable-extent limit. Add an explicit "Apply Transform to Pixels" command.

### XF-7 Perspective Warp and Puppet Warp — Accurate

- Nothing related exists in the code.

## Tweaks

### T-6 — Imprecise, and duplicates XF-2

**Evidence:**
- There is no "document type" concept. Presets are not stored with the document (`crates/layer-ui/src/document_creation.rs:73-105`).
- Transform options are not saved: `Operation.aspect` lives only for the session, and placement forces it on (`operation.rs:53-59`; `placement.rs:123`).
- Placement ignores `Interpolation` (`scene/placement.rs:93-96`).

**Suggested correction:** merge into XF-2. Store the choice in settings or tool state. Say that a per-layer choice for placed photos needs a new stored field and a file-format step.

### T-13 — Duplicates XF-4; imprecise

**Evidence:**
- The line reference `operation.rs:201` is correct.
- `can_transform` refuses non-Paint layers, but it does not refuse multi-layer selections; it ignores them.
- Groups can already be moved.

**Suggested correction:** fold into XF-4 with the evidence listed there.

## Additional gaps not in the plan

1. Finger touch cannot grab Scale/Rotate handles for paint or mask transforms; only photo placement can (`operation.rs:511`; `session.rs:1109-1112`).
2. Exporting or flattening a downscaled photo aliases because exact capture skips the preview levels (`scene.rs:1463-1466`).
3. Transform box bounds are tile-granular, not pixel-tight (`operation.rs:133-198`).
4. Ctrl/Cmd changes are not tracked live during a drag (`session.rs:939-950`).
5. Composition and capture are limited to the canvas (`scene.rs:1467` and 1712-1726).
6. Committed multi-layer operations refuse locked layers (`canvas.rs:556-561`). Document-wide geometry commands need a locked-layer policy.
7. Size results must be validated against the project limits (`project.rs:88-95`), the 8,192 px new-document limit (`document_files.rs:133`) and the GPU limit (`lib.rs:1692-1695`). Otherwise a saved result may not reopen.
8. Effects that depend on document geometry shift or rescale after a crop or resize:
   - Vignette and any `fx_extent()` user filters rescale.
   - Grain, Halftone and other position-based effects shift pattern phase.
   - Kaleidoscope, Swirl and CRT use document-wide sampling.
9. New document-level commands must follow the idle and busy rules (`require_document_idle` plus `!document_file.busy`, `session.rs:1961-1963`). `require_document_idle` already fails while a transform or placement is active (`document_files.rs:527-534`).
10. Registration checklist for each new command:
    - the `CommandId` variant, `ALL: [Self; 125]` (`lib.rs:859`), and `TOOLS` if it is a tool (`lib.rs:988`);
    - label, icon and `available_on`;
    - a customization description (`customization.rs:~984-1040`);
    - a menu entry and a default shortcut (`shortcuts.rs:209-262`);
    - enabled state and dispatch (`session.rs:~1940-2031` and `3930-4260`);
    - `command_without_renderer` for cancel commands;
    - an SVG in `apps/layer-web/icons`, which the icon test checks (`tools.rs:720-744`).
    - Serde and shortcut IDs are persisted, so never rename existing variants.
