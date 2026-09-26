# Implementation audit: layers and compositing (LYR)

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-25 · baseline `dac76c20`

Read-only implementation audit made by an agent against baseline `dac76c20`. It checks each item in the first draft of the build list against the code, the platform hosts and the design records. "Plan line N" refers to that superseded first draft; the current [research record](../photo-editing-research.md) incorporates the corrections. Line numbers can drift in later commits; verify before relying on one.

---

Audit of the Layers inventory row, LYR-1 to LYR-9, T-1 and §7 in docs/history/photo-editing-research.md

All paths are relative to the repository root. I only read files; nothing was changed.

Short version: the plan misses a lot of existing code and several documented decisions.
- **Blend modes:** one WGSL `blend()` switch already drives layer, effect-layer and cached compositions. It works in linear light, and Add and Color already clamp HDR values.
- **Pass Through:** making it the default reverses a documented decision.
- **Merge semantics** were already decided in the design docs.
- **Flatten:** the flatten implementation is in the snapshot renderer, not at `document_workflow.rs:67`.
- **Generators** work end to end in the renderer; only the shipped programs are missing.
- **Blend If math** already exists in the tonal-selection code.
- **T-1:** the selection-to-mask code already exists in `AddMask`.

---

## Inventory, Layers row (line 88)

**Verdict: imprecise.**

**Evidence:**
- `LayerKind` also has `ImportedImage` and `AiSuggestion` (`crates/layer-core/src/lib.rs:162-171`). They are vestigial: `Layer::image` (`lib.rs:247`) has no callers. Many checks still include them, for example `layers.rs:727`, `layers.rs:1043` and `layers.rs:1088`.
- Mask features the row leaves out:
  - Show mask area (`LayerMask.show_area`, `layers.rs:436`), drawn as a fixed purple tint at 0.42 (`scene.wgsl:205`).
  - Enable/disable, Reveal all and Hide all (`art_layers.rs:1330`, `art_layers.rs:1347-1355`).
  - Mask from selection with reveal or hide (`art_layers.rs:1454-1473`).
  - Selection from layer alpha or mask (`selection_masks.rs:987-1022`).
- Also missing:
  - Rasterize Source and Repair Profile (`art_layers.rs:793-794`, `source_edit.rs:215`).
  - Clear layer.
  - Effect layers take blend modes too (`crates/layer-render-wgpu/src/effects.rs:496-507`).
- `layers.rs:398` is `LayerProperties`; the placement field is at `layers.rs:401-404`.

**Correction:** list all seven kinds, noting the two vestigial ones. Add "blend modes apply to effect layers too", "mask Show-area tint", and "Rasterize Source".

---

## LYR-1: Complete blend modes and pass-through groups

**Verdict: overlooked infrastructure, wrong claims, and it contradicts a design decision.**

**Where blending is evaluated.** It is one switch, `blend(s,d,mode)` in `crates/layer-render-wgpu/src/scene.wgsl:73-86`.
- Layer composition calls it through `combine()` (`scene.rs:735-798`, op 4 at `scene.wgsl:206-213`).
- `scene.wgsl` is included into every effect shader (`effects.rs:580`).
- It is therefore also used for:
  - effect-layer blends (`effects_color.wgsl:8-16`);
  - fused effect output (`effects.rs:663`);
  - the composite folded into the effect shader (`effects.rs:710`, `scene.rs:762-784`);
  - the cached clipping composition (`scene_images.rs:119-121`).
- Adding a case here therefore covers all of those paths at once.

**Brush blend code to share.**
- `BrushBlendMode` (`lib.rs:439-449`) already has Subtract, Darken, Lighten and Overlay in WGSL: `blend_color` at `material_brush.wgsl:191-204`, with codes in `layer-render-wgpu/src/lib.rs:5189-5199`.
- Its numbering differs from `LayerBlend` (brush 4 = Subtract, layer 4 = Overlay).
- Overlay's tie-break also differs: `>=` at `material_brush.wgsl:201` versus `>` at `scene.wgsl:78`.
- Suggestion: move both into one shared `blend_modes.wgsl` include.

**Wrong claims about the current modes.**
- **Add is not a true Linear Dodge in HDR.** It clamps: `min(s+d,1)` at `scene.wgsl:77`. `docs/internals/rendering.md:52-53` records that clamp as deliberate ("Explicit Add/Subtract bounds remain part of their artistic formulas"). That conflicts with §7's "never silently clamp".
- **Color clamps above 1.** `set_luminance` clamps (`scene.wgsl:66-71`).
- **Color uses the wrong luma weights for linear data.** It uses .3/.59/.11 (`scene.wgsl:65`). The filter library already has HDR-safe helpers using the working space's luma: `fx_luma` with `FX_LUMA` and `fx_preserve_luma` (`assets/filters/effects.wgsl:15-22`), plus `fx_hsl` and `fx_hsl_rgb` (`effects.wgsl:65-75`). Hue, Saturation and Luminosity should reuse these.
- **Screen misbehaves above 1.** `s+d-s*d` is non-monotonic there. So "define behavior above 1.0" also applies to modes that already ship.

**Blend domain (this affects Photoshop matching).**
- Blends operate in **linear document RGB** at every bit depth (`rendering.md:51-52`; `scene.wgsl:207` unassociates linear premultiplied values).
- Photoshop blends 8/16-bit documents in encoded values. Overlay, Soft Light, Hard, Vivid and Pin Light, Burn/Dodge, Difference and Exclusion will therefore not match.
- The 50% neutral point is linear 0.5, which is about sRGB 188, not 128. This affects RET-7's "50% gray" layer and ADJ-9's Linear Light offset.
- The encoding helpers already exist: `fx_encode`/`fx_decode` and `sdr_encode(c,FX_SPACE)` (`effects.wgsl:3-12`).
- Correction: make each mode declare its domain, the way §7 already requires for tone controls, or add a document option to blend in encoded RGB.

**UI coupling.**
- The mode index doubles as the enum discriminant:
  - `LayerAction::Blend` indexes `LayerBlend::ALL` (`art_layers.rs:1266-1269`);
  - the Properties panel uses `blend as u32` as the Choice index (`effects.rs:455-461`);
  - shaders receive `blend as u32` (`scene.rs:792`).
- Hosts get a flat, ungrouped label list from `layer_blends` (`layer-ui/src/lib.rs:512`), consumed by `apps/layer-linux/src/layers.rs:373`, `apps/layer-android/.../Layers.kt`, `apps/layer-apple/Shared/Editor/LayerPanel.swift` and `apps/layer-web/layers.js`.
- With about 27 modes, the plan needs an explicit menu-order-to-code mapping and grouped menus on every host.

**Fast path.** `draw_normal_layer` handles Normal only (`scene.rs:1157-1170`), so new modes take the scratch path. That is fine, but it should be benchmarked.

**Pass Through contradicts a design decision.**
- `docs/history/layers-initial-design.md:265` says: "Pass-through groups are deferred … The isolated default follows the documented CSP option." It is listed as follow-up #5 at `:318`.
- `layers-research.md:49` notes CSP puts restrictions on clipping inside through-folders.
- The plan proposes Pass Through as the default for new groups. It should cite and supersede that decision. Existing documents must deserialize as isolated: use a serde default of Isolated, even if new groups default to Pass Through.

**Code that assumes isolated groups.** Pass Through is structural, not a flag:
- `Scene::group` gives every non-root group a transparent scratch surface (`scene.rs:988-1008`).
- Adjustment input capture composes the parent group only: `self.group(r, packet, layer.properties.parent, tile)` (`scene_images.rs:808-811`).
- `input_indices` says "Dependencies follow the same isolated group / clipping-stack boundaries as composition" (`scene_images.rs:277-301`).
- The checkpoint search is limited to siblings (`scene.rs:1012-1021`).
- `reference_snapshot` adds only an adjustment's siblings (`layers.rs:659-671`).
- `filter-clipping-regression.md:16` states that backdrop caches follow sibling scope "including isolated groups".

**Knock-on benefits to state.**
- Pass Through would make Group (`layers.rs:787`) and Ungroup appearance-preserving. Ungroup's restrictions (`layers.rs:854-869`, `layers.rs:885-889`) and `layers-context-menu-audit.md:70-73` exist only because groups are isolated.
- Groups cannot be clipping bases (`layers.rs:727`; design doc `:255`).
- The plan also needs to specify how opacity and a mask on a pass-through group apply.

**Docs to update:** `rendering.md:102-104` and `documents.md:19-21` say adjustments act "within its group".

**Oracle test to extend:** `cached_clipping_matches_tiled_composition` (`filter-clipping-regression.md:92-97`).

---

## LYR-2: Merge, flatten, stamp and apply effect

**Verdict: already decided in design docs, partly implemented, and the "Today" citation is imprecise.**

**Semantics already decided:**
- `layers-initial-design.md:269`: "start with simple Normal merges, whole clipping-stack baking, and isolated-group flattening when representable."
- `:209`: the edit lock blocks merging.
- `:251`: applying a group mask requires explicit group flattening.
- `layers-context-menu-audit.md:42` and `:96-98`: "GPU bake/snapshot as an undoable document source… settle wet-paint drying and clipping-base semantics."

**The existing flatten is not at `document_workflow.rs:67`.** That line is only the `ColorPreparation::Flatten` enum variant. The work happens in:
- `SnapshotRenderer::flattened_document` (`crates/layer-render-wgpu/src/snapshot/flatten.rs:4`);
- `layer_color::flattened_document` (`crates/layer-color/src/flatten.rs:13-43`). It streams rows into a `SourceKind::Rasterized` source and drops Paper with `layers.truncate(1)` at `:35`. The paper color is baked in through the snapshot background (`snapshot.rs:332`).
- Host callers: `apps/layer-linux/src/files/color/flatten.rs:16`, `apps/layer-android/native/src/color_edit.rs:85` and `apps/layer-apple/native/src/project_color.rs:50`.
- This is reusable for Flatten Image and Merge Visible as an off-interactive worker step. Worker-prepared edits are admitted through `Editor::validate_edit` (`lib.rs:1921`), as `source_edit.rs:94` does.

**GPU bake path to reuse:**
- `LayerOperationKind` (`layers.rs:450-466`) is executed by `Scene::apply_operation` (`scene.rs:1274-1423`).
- For ApplyMask, that function already resolves watercolor and clears wetness, material and coverage state (`scene.rs:1304-1420`). This covers the audit's wet-paint requirement.
- The engine queues these operations with `RasterRevision::pending()` (`layer-engine/src/canvas.rs:806-830`).
- Stamp Visible can be built by copying `Scene::group(None, tile)` output into new paint pages (`Job::Copy`, `scene.rs:1390`).
- Stamp must exclude the root background clear (`scene.rs:997-1004`). Paper is never a composited sibling (`scene.rs:1039`).

**Constraints the plan omits:**
- **Paper cannot receive content.** Paper is protected (`layers.rs:1071-1081`, `lib.rs:1563`; design doc `:158-161`). Merge Down onto Paper must be disabled, and Flatten must decide whether Paper stays.
- **Clipping.**
  - Clipping bases can only be Paint or ImportedImage (`layers.rs:721-729`).
  - Deleting a base without its clips is refused (`layers.rs:760-770`).
  - "Bake a clipped effect into its base" is not appearance-preserving when other clips sit between the base and the effect. A clipped adjustment processes the whole clipping stack (`rendering.md:108-113`). The unit is the whole stack, per design doc `:269`.
- **References.** `RemoveLayer` drops reference designations (`lib.rs:1530-1531`). Ungroup transfers them (`layers.rs:871-917`), which is a precedent to follow.
- **"Rasterize" clashes with existing terminology.** Rasterize Source only commits a retained original to document color. It keeps placement (`crates/layer-color/src/rasterize.rs:1-2`, `source_edit.rs:215-257`), and Apply never resamples backing (`layers.rs:401-402`). Rename the plan's rule to "resample a placed source into document pixels".
- **Budgets:**
  - History `BYTE_BUDGET` is 512 MiB and `ENTRY_BUDGET` is 256 (`history_budget.rs:6-7`).
  - A native publication is at most 1 GiB (`project-format.md:111-113`).
  - GPU raster operations are admitted with `RasterRevision::pending` and `reserve_pending_bytes` (`raster.rs:422-433`; comment at `lib.rs:1976-1982`). Edits that add a source are admitted eagerly (`lib.rs:1698-1715`).
  - A full-extent bake of a large Float32 photo can exceed 512 MiB.
- **Existing bug:** ApplyMask on a group says "Apply a group mask by flattening the group first" (`art_layers.rs:1312`), but no such command exists.

---

## LYR-3: Blend If / Blend Ranges

**Verdict: overlooked infrastructure; "fused into the existing composite pass" is imprecise.**

**The math already exists.**
- `TonalBand{lower, upper, falloff}` and `coverage()` (`crates/layer-core/src/tonal.rs:10-16`, `:56-77`) and the GPU `tonal_ramp` (`tonal.wgsl:12-17`) are exactly split black and white thresholds with smoothstep feathering, in stops.
- The luminance is linear and relative to reference white (`tonal.rs:1-2`).
- Photoshop's 0-255 Blend If is encoded gray. The plan must define the SDR mapping.

**There are several composite paths, not one:**
- `scene.wgsl` op 4 (`combine`);
- the Normal fast path, drawn with hardware `PREMULTIPLIED_ALPHA_BLENDING` (`scene.rs:1157`, `scene.rs:2154`). "Underlying layer" ranges need a backdrop read, so these layers must leave this path;
- `portable_blend.wgsl`, which supports only source-over, erase and max;
- the effect fused and folded output (`effects.rs:663`, `effects.rs:710`);
- the cached `ImageComposition` (`scene_images.rs:50-133`).

**Cache invalidation is automatic.** Scene metadata compares whole layers (`scene/metadata.rs:15-23`), so a new `LayerProperties` field invalidates caches with no extra code.

**Already listed as deferred:** `layers-context-menu-audit.md:35` defers Fill opacity, Blend If, knockout and more blend modes.

---

## LYR-4: Mask properties

**Verdict: partly implemented or overlooked, and it conflicts with a design decision.**

**What exists:**
- Enable, link, invert, reveal all / hide all, apply, delete, copy/paste and mask from selection (`art_layers.rs:1510-1552`).
- Selection from mask (`selection_masks.rs:987-1022`).
- GPU Gaussian feather for selections (`SelectionRefinement.feather`, `layer-render/src/lib.rs:408-418`; `selection_refine.wgsl:107-123`). `LoadCoverage` hard-codes `feather: 0.` (`selection_masks.rs:1005`).
- So a destructive feather is only a small change: load the mask as a selection with feather, then Replace mask from selection.

**Density** is a pointwise change in three places:
- `mask_tile` defaults (`scene.rs:806-810`);
- `scene.wgsl:200` and `scene.wgsl:216-225`;
- `effect_properties` (`effects.rs:496-507`).

**A live Feather needs neighbourhood sampling.** Masks are sampled per aligned 256² tile (`scene.rs:799-839`), so live feather has to go through `scene_images` image stages with a declared halo (`rendering.md:143-150`).

**"View the mask alone" conflicts with the design.** `layers-initial-design.md:224-228` explicitly chose a tinted overlay, "not a mask-only black/white replacement view". Selection masks already have a "Grayscale mask" mode plus overlay color and opacity (`selection_properties.rs:28-56`, `SelectionPaintBehavior`, `selection.rs:7-18`). Reuse that model; layer masks currently have a fixed purple tint at 0.42.

**Paste a luminosity mask:** Tonal selection (`RegionSource::Tonal`) followed by Mask Selection already does this in two steps. Copy/Paste mask is a session clipboard only (`art_layers.rs:878-910`; audit `:39`).

**Masks have no opacity by design:** `layers-initial-design.md:247` ("no independent layer opacity control in this MVP").

---

## LYR-5: Fill layers

**Verdict: partly implemented; "no shipped generators" is accurate but understates what exists.**

**Evidence:**
- All 40 manifest programs are `adjustment`. That part is true.
- But Generators are fully supported:
  - the renderer (`scene.rs:850-853`, `scene_images.rs:804-808`, `effects.rs:688-690` "Generators cannot be fused");
  - tests (`filter_previews.rs:886`, `filter_previews.rs:1239`);
  - serde (`effects.rs:48-53`).
- Parameter kinds `Color` and `Gradient` exist (core `effects.rs:207-239`), with up to 32 stops and a stop editor (`layer-ui/src/effects.rs:205-212`, `:654`; `runtime-filters.md:36-60`).
- So Solid Color and Gradient fill layers need only manifest and WGSL entries.
- Pattern needs an image parameter kind, which means an ABI 3 bump.
- Designed in `non_destructive_filters_wgsl_shader_subsystem.md:152-176`; follow-up #7 in `layers-initial-design.md:320`.

**Gaps to state:**
- On an unmasked Effect layer, painting passes through to the layer below (`layers.rs:958-966`).
- Clipping to a fill layer is impossible (`layers.rs:727`).
- Effect layers cannot be references (`lib.rs:1497-1502`).

---

## LYR-6: Align, distribute and auto-align

**Verdict: accurate (nothing exists), but it misses constraints.**

- `content_bounds` is conservative at 256-px tile granularity (`crates/layer-ui/src/operation.rs:133-175`). Exact alignment needs a GPU alpha-bounds query, for example reusing the thumbnail alpha-bounds pipeline (`thumbnails.rs:408`).
- `move_target_edit` moves a single layer (`layers.rs:1108-1128`); this overlaps T-13.
- `placement` is rejected on groups and effects (`layers.rs:1087-1088`).
- Guides and snapping already exist (`Document.rulers`, `rulers.rs:26-91`) and could serve as align targets.

---

## LYR-7: Stack modes

**Verdict: imprecise.** A "stack effect over a group" needs multi-layer input. ABI 3 effects read only the composite below plus parameters (`runtime-filters.md`; `effects.rs:65-66`). Either extend the ABI or add a dedicated renderer operation.

---

## LYR-8: Layer styles

**Verdict: wrong mechanism.** "Effects that read layer alpha" cannot draw outside the base: clipping preserves the base's alpha (core `effects.rs:59-61`; `rendering.md:108-110`). Drop Shadow and Outer Glow need a new attachment kind.

---

## LYR-9: Panorama and focus merge

**Verdict: accurate.** It inherits the LYR-6 and LYR-7 constraints.

---

## T-1: New effect layer takes the active selection as its mask

**Verdict: the claim is correct, but the citation is imprecise and existing code is overlooked.**

**The claim holds.**
- `effects.rs:181` is only the `EffectAction::Insert` enum variant. The handler is `effects.rs:740-776`.
- It builds `Layer::paint(...)` with `kind = Effect` and no mask.
- It never reads or clears `doc.selection`, and the drawer "replacing" path keeps the old layer.

**Existing code to reuse.**
- The selection-to-mask mapping, including soft coverage, inversion and linked geometry, already exists in `LayerAction::AddMask` (`art_layers.rs:1271-1301`). `MaskSelection` (`art_layers.rs:783-791`) is just a rewrite onto it.
- `AddMask` always consumes the selection (`Edit::SetSelection(None)`, `art_layers.rs:1297`). That was a documented decision (`layers-initial-design.md:185`, `:189`), so a "keep selection" setting must cite it.

**What already follows from existing code.**
- Once the effect layer has a mask, brushes route to it automatically (`layers.rs:953-957`).
- Insertion is refused in mask mode (`effects.rs:592`).
- T-9's "Add adjustment with this mask" becomes a special case of T-1.

---

## §7 Rules for implementation agents

- **Linear light:** add that "layer blend modes currently compose in linear document RGB (`rendering.md:51`); each new mode declares its domain."
- **Clamping:** the "never silently clamp" rule conflicts with Add and Color today (`scene.wgsl:70`, `scene.wgsl:77`) and with the documented Add/Subtract bounds (`rendering.md:52-53`).
- **History admission:** cite `raster.rs:422-433` and `lib.rs:1698-1715`, not only `history_budget.rs`.
- **Incremental composition:** any new group or blend semantics must update `input_indices` and `capture_window` (`scene_images.rs:262-301`) and `reference_snapshot` (`layers.rs:635-692`).

---

## Other gaps not in the plan

1. The ApplyMask error text points to a group-flatten command that does not exist (`art_layers.rs:1312`).
2. Add and Color already clip HDR in float documents (`scene.wgsl:65-77`).
3. Brush and layer blend math are duplicated and inconsistent (`material_brush.wgsl:191-204` versus `scene.wgsl:73-86`).
4. Groups and fill or effect layers cannot be clipping bases (`layers.rs:727`; `art_layers.rs:1258-1262`).
5. The layer-mask overlay color and opacity are fixed (`scene.wgsl:205`), while selection masks can change theirs.
6. The ImportedImage and AiSuggestion kinds are dead but still spread through validation, which any merge rules must handle.
7. Fill opacity and knockout are unplanned; the audit deferred them (`layers-context-menu-audit.md:35`).
8. The mode-index and discriminant coupling plus the flat host lists (`layer-ui/src/lib.rs:512`) will not scale to about 27 grouped modes.
9. The flattened copy uses a CPU row stream (`snapshot/flatten.rs:22-33`). In-document Flatten reusing it needs a progress and cancel owner, which `rendering.md:183-185` notes is still missing.
10. The Gradient tool supports only two colors (`layers.rs:459-465`), while effect gradients support 32 stops. T-14 and LYR-5 should share one gradient model.
