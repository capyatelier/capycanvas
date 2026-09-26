# Implementation audit: retouching (RET)

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-25 · baseline `dac76c20`

Read-only implementation audit made by an agent against baseline `dac76c20`. It checks each item in the first draft of the build list against the code, the platform hosts and the design records. "Plan line N" refers to that superseded first draft; the current [research record](../photo-editing-research.md) incorporates the corrections. Line numbers can drift in later commits; verify before relying on one.

---

HEAD is `dac76c20`, the same baseline the plan used. All paths are relative to ``. The biggest findings:
- **Liquify Reconstruct does not exist.** The renderer rejects it.
- **RET-1's snapshot rule is too narrow.** It also has to cover the Visible and Editing sources.
- **Clone does not fit the Smudge path.** It belongs on the dry-deposit path.
- **T-12 conflicts with RET-1's snapshot semantics.**
- **Multi-stop gradients already exist** in Gradient Map.
- **Pen barrel buttons are hard-wired to Pan** on every host checked.

---

## Inventory row "Retouch and paint"

**Verdict:** Wrong or imprecise claim (partly).

**Accurate:**
- "35 presets" is correct. `crates/layer-ui/src/tools.rs:178-298` lists them and the test at `tools.rs:748` asserts 35. The count includes Eraser and the two Liquify presets.
- Liquify presets are only Push and Twirl (`tools.rs:288-297`, with TwirlClockwise at `crates/layer-core/src/presets.rs:254-268`).
- The only Liquify control is Strength (`crates/layer-ui/src/tool_settings.rs:255-261`).
- Eyedropper is 1–101 px (`crates/layer-ui/src/eyedropper.rs:8`) and samples the composite or one layer (`ColorSampleSource`, `crates/layer-render/src/lib.rs:294-298`).
- `BrushExecution` is at `layer-core/src/lib.rs:425`.

**Wrong:** "the engine also has … Reconstruct".
- `crates/layer-render-wgpu/src/lib.rs:1637-1642` returns `UnsupportedBrushFeature("liquify reconstruct snapshot")`.
- The shader has no Reconstruct branch: modes ≥5.5 fall into the Edge displacement (`material_brush.wgsl:490-493`).
- The docs agree: `docs/reference/gpu-brush-engine.md:306-308` ("Reserved… not implemented: reconstruct snapshots") and `docs/history/advanced-brush-engine.md:15-18`.
- Summary item 4 has the same error ("two of its eight engine modes"). Only seven modes are implemented.

**Better evidence for "brushes cannot sample other layers":**
- Material passes bind only the target layer's 3×3 page neighbourhood (`material_brush.wgsl:22-30, 53-91`).
- The distant-page gather reads `raw_layer_neighborhood(batch.layer_id…)` (`material_sources.rs:399, 415`).
- Watercolor is explicitly same-layer (`layer-core/src/lib.rs:430-432`).

**Missing from the "present" column:**
- 8 brush blend modes run on the GPU (`layer-core/src/lib.rs:439-449`, `material_brush.wgsl:191-229`). There is no UI for them (no hits in layer-ui or apps); only Multiply Glaze uses one (`presets.rs:270-282`).
- Smudge has a blur (≤4 px, `material_brush.wgsl:116-137`), but it is not exposed in tool settings.
- Alpha lock.
- Stroke-uniform accumulation.
- Lasso Fill (`tools.rs:534`).
- Fill with a Visible, Editing or Reference source.
- Gradient on masks (`art_layers.rs:1914-1917`).

**Suggested text:**
- "Liquify … presets Push and Twirl; the engine also implements Twirl CCW, Pinch, Expand, Crystals and Edge; `LiquifyMode::Reconstruct` is declared but rejected by the renderer."
- Add "8 brush blend modes exist on the GPU with no UI" to the present column.

---

## RET-1 Sample source from reference layers (sanity check)

**Verdict:** Citations are accurate, but several precision gaps and one design gap.

Checked and correct:
- `Document.reference_layers` at `lib.rs:1295`.
- `Edit::SetReferences` at `lib.rs:1494-1505` (undoable).
- `RegionSource` at `art_layers.rs:37-43`.
- The UI mapping at `region_tools.rs:170-181`.
- `reference_snapshot` at `layers.rs:635-691`.
- The error message at `region_tools.rs:176-178`.

1. **The snapshot rule is too narrow.**
   - The plan snapshots only when "target is itself a reference". But the Visible source always includes the target, and the Editing source *is* the target. Both need the stroke-start snapshot too.
   - Lazy per-tile capture is only correct if each tile is captured before the stroke first writes to it (copy-on-write).
   - The renderer keeps no stroke-start pages today. The only pre-stroke state is the CPU `ActiveStroke.before` revision (`crates/layer-engine/src/canvas.rs:85`), and uploading during contact is forbidden (`gpu-brush-engine.md:273`).
   - **Correction:** "All three sources read a stroke-start copy-on-write snapshot of every page the stroke will read or write."

2. **The target layer is excluded, unlike Photoshop.**
   - Photoshop's "Current & Below" includes the retouch layer. With RET-1, a new empty layer is excluded unless it is marked, so a second clone pass cannot see earlier fixes.
   - **Correction:** have the one-tap action mark both the photo and the target (or define the source as reference ∪ target in stack order). This makes "reference and target together" the default path, not an edge case.

3. **Only some layer kinds can be references.**
   - Paint, ImportedImage and Group only (`lib.rs:1496-1500`). Background and Effect layers cannot be marked.
   - Background paper therefore never contributes (`region_sources.rs:373-381`).
   - Hidden reference layers contribute nothing (`layers.rs:684-688`).
   - State all three.

4. **Wrong reuse pointer.**
   - The actual bounded composite query is `artwork::Capture::region` (`crates/layer-render-wgpu/src/artwork.rs:88-219`). It is already used per page by `region_sources.rs:698-707` and by `color_sample.rs:125`.
   - `scene_images.rs` is internal filter windowing (`docs/internals/rendering.md:143-151`). `layer_masks.rs:207` only prepares mask pages.
   - Constraints of `Capture::region`:
     - windows are ≤256² (`artwork.rs:121-127`);
     - there is one reusable target, which must be consumed before the next call (`:111-112`), so the cache needs its own pages;
     - there is a 256 MiB image limit (`:131-136`);
     - it submits a chunk when the window changes (`:141-148`);
     - it replays predicted preview contacts into the crop (`:160-211`), so capture must use a packet without preview batches.

5. **"Preserving float values" is not true everywhere.**
   - The capture target uses `device.working_format()` (`lib.rs:5744-5750`). That defaults to SRGB8 (`pipeline_device.rs:44, 127`); float precision holds only on native Float32 devices.
   - The gather fields are `Rgba32Float` (`material_sources.rs:38-45`).

6. **Performance.**
   - Capturing a tile mid-stroke means composing the whole scene for that window, effects included. That runs against the 8.33 ms p99 budget (`gpu-brush-engine.md:280-281`).
   - Precedent for a bounded document cache: the tonal cache (`region_sources.rs:39-50`).
   - **Correction:** prefetch tiles around the source point when the source is set or at pen-down, and cap captures per frame.

7. **Coordinate spaces differ.**
   - Editing samples raw layer-local paint (`layer-render/src/lib.rs:351-352`). Reference and Visible are in document space.
   - Placed targets use `DabStyle.brush_to_layer` (`layer-render/src/lib.rs:145-148`). The spec must say which space the source offset lives in.

8. **UI reuse and label mismatch.**
   - The source list can reuse `ToolActionGroup::SelectionSource` (`tool_settings.rs:16-51`) and the toolbar list-choice rule (`docs/ui/toolbar-components.md:56-60, 100-101`).
   - Labels differ today: Fill says "Visible artwork / Editing layer / Reference layers" (`tools.rs:439-441`); Wand says "Sample visible artwork…" (`lib.rs:1097-1099`).
   - The region default is Visible (`art_layers.rs:38-43`).
   - The one-tap action can reuse `LayerAction::Reference` and `ReferenceSelection` (`art_layers.rs:171-172, 975-986, 1068-1073, 1648-1653`).

9. **Masks.** The engine forces `Dry` for mask strokes (`canvas.rs:1302-1318`). Say what the source means on a mask target, or refuse.

---

## RET-2 Clone Stamp

**Verdict:** Overlooked existing infrastructure, plus wrong or imprecise claims.

**Rendering: "the dab path that Smudge uses" is wrong.**
- Smudge is an ordered destination backtrace in chunks of at most 3 contacts, whose live chunk is deferred to the preview (`canvas.rs:43, 1556`; `material_brush.wgsl:602-616`; `gpu-brush-engine.md:161-175, 227-230`).
- A clone reading an immutable snapshot is order-independent. It fits the dry-deposit loop with per-pixel pigment, the way bristles already do (`material_brush.wgsl:675-678`).
- It should use `BrushAccumulation::Uniform` (`lib.rs:452-457`). Stroke coverage then gives Photoshop-like per-stroke opacity (`material_brush.wgsl:639-713`).
- Distant source pages: add a Clone case (offset plus transform) to `sample_bounds` and the gather (`material_sources.rs:83-164, 324-480`).
- **Binding budget:** the material pass already binds 16 sampled textures: 11 in group 2 (`material_brush.wgsl:22-33`) and 5 in group 3 (`brush_textures.wgsl:1-5`). That equals WebGPU's default per-stage limit, so the source must reuse slots, e.g. the gathered-field slot at `header.x == 2` (`material_brush.wgsl:606-607`).
- Also update `BrushPassPlan` (`lib.rs:520-537`).

**Blend mode claim is wrong for the default workflow.**
- Brush Darken and Lighten exist (`lib.rs:446-447`), but the backdrop is the *target layer*.
- On an empty retouch layer (destination alpha = 0) they behave as Normal (`material_brush.wgsl:222-228`).
- Layer-level Darken/Lighten do not exist either (`layers.rs:364-372`; that is LYR-1).
- **Correction:** "Darken/Lighten compare against the RET-1 source sampled at the destination", or make the option depend on LYR-1.

**Tool family: "in the Tools panel" is imprecise.**
- The Tools panel shows only the current set's subtools (`tools.rs:365-368`). Sets live in the Brushes and Sculpting panels.
- Adding a family requires:
  - `Tool` and `ToolGroup` variants (`tools.rs:8-18, 93-174`);
  - a `ToolFamily` variant with a shortcut id (`:56-89`);
  - updating `is_drawing`, otherwise new tools appear under Brushes (`:382-384`); `is_sculpt` is at `:386-388`;
  - a new memory slot in `WorkspaceToolMemory`, which has only `drawing`/`sculpt` slots and uses `deny_unknown_fields` (`:586-596`);
  - new `DefaultBrushPreset` IDs of 36 or higher, since IDs are stable preview-cache and asset keys (`:176-177`);
  - native drawer projections on each host (`docs/ui/default-workspaces.md:48-58`).
- **Correction:** "a Retouch set beside Blend and Liquify in Sculpting, or a new Retouching set panel".

**Pen barrel mapping is overlooked (it does not exist).**
- Barrel buttons are hard-wired to Pan:
  - GTK buttons 2/3 at `apps/layer-linux/src/input.rs:262-275, 377-385`;
  - Android `CanvasSurfaceView.kt:226` → `layer-host/src/lib.rs:543-547`;
  - Windows `apps/layer-windows/native/src/host.rs:75-80`.
- `PointerButton` is {Primary, Pan, Other} (`interaction.rs:24-28`).
- `SampleFlags::BARREL_BUTTON` (`layer-engine/src/input.rs:44`) has no consumer.
- **Correction:** "requires a new pen-button binding setting; remapping removes Pan".

**Touch: long-press is taken.** A touch hold already opens the colour picker (`interaction.rs:93`; `color_picker_session.rs:201-222`), so "Set source" cannot use a hold.

**Source disc: reuse the transform-handle pattern.**
- Hit test and immediate drag: `operation.rs:470-485`.
- Hit radius: `ruler_reach` (`rulers.rs:66-71`).
- Touch routing: `placement_touch_hit` (`operation.rs:508-515`; `session.rs:1112`).
- Overlay drawing: `CursorSegment` (`operation.rs:533-580`).
- No holds on handles: `docs/ui/drag-and-reorder.md:115-116`.
- The cursor can already outline the brush (`cursor.rs:136-178`), but a pixel "clipped overlay" is new renderer work.

**Options: reuse the existing tool-option types.**
- Aligned: checkable `ToolSettingAction` (`tool_settings.rs:9-13`).
- Source: `ToolOption::Choice` (`toolbar_components.rs:117-135`).
- Rotation/scale: session-owned numeric settings, like `region_tools.rs:79` and `operation.rs:423`. Brush `DEFINITIONS` only map `BrushSnapshot` f32 fields (`tool_settings.rs:73-79`).

**Replay determinism.**
- Offset, transform, aligned and source kind must go in `Stroke`, like `alpha_locked` and `selection` (`lib.rs:1206-1224`).
- Late corrections replay from `completed_before` (`corrections.rs:101-122, 139-160`), so the snapshot must outlive pending estimates.

**Lock and alpha lock:** accurate (`canvas.rs:1280-1286`; `material_brush.wgsl:749-758`). Note that alpha lock on an empty layer makes Clone a silent no-op.

**Masks:** Clone on a mask would silently paint white (`canvas.rs:1302-1318`). Refuse, or specify a scalar clone.

**Cross-document clone (P2):** parked documents are not GPU-resident (`document_sessions.rs:1-21`).

**Performance:** add Clone to the acceptance matrix (`advanced-brush-engine.md:241-265`) and to the benchmark scenarios (`docs/development/gpu-raster-benchmarks.md:163-178`).

---

## RET-3 Healing brush

**Verdict:** Accurate that nothing exists, but related infrastructure was overlooked.

**No solver exists.** There is no Poisson, Jacobi or multigrid code.

**Reusable Float32 pyramid kernels:**
- `local_tone.rs:1-2` is a shared compute engine with host-owned idle scheduling and cancellation. Its shader has downsample, expand, accumulate and reconstruct (`local_tone.wgsl:76, 105, 125, 137`).
- `backdrop_blur.rs:1` is a local-Laplacian blur used only for the glass UI.
- Do **not** use display mips: `display_mips.rs:1-2` says "no editing consumer reads it".

**Working memory:** the gather fields are fixed 256² (`material_sources.rs:30-46`), too small for 300 px support. Use bounded windows in the scene_images pattern (`rendering.md:145-151`).

**"Converge at stroke end" has existing hooks:**
- `DabBatch.stroke_end` (`layer-render/src/lib.rs:186-188`).
- The post-stroke edge pass (`lib.rs:675`; `layer-render-wgpu/src/lib.rs:506, 551`; `stroke_edge.wgsl`).
- Deterministic pen-up replay (`advanced-brush-engine.md:205-209`).
- The solve needs fixed iteration counts so correction replay stays deterministic.
- The pen-up budget precedent is under 8.33 ms (`advanced-brush-engine.md:254`), so a sub-second heal must be an async job with a preview.

**Correction:** on an empty layer, take the boundary tone from the RET-1 source sampled at the destination, not from the empty target.

---

## RET-4 Spot Healing

**Verdict:** Accurate. There is no patch-similarity search.

**Correction:** same boundary-sampling note as RET-3.

---

## RET-5 Content-aware fill and Remove

**Verdict:** Accurate, but existing job and overlay patterns were overlooked.

**Async job patterns to reuse:**
- `RegionRequest` generation and revision staleness (`region_tools.rs:165-205`).
- The `local_tone` job model.
- `SuggestionRequest::is_stale_for` (`lib.rs:2178-2199`).

**Do not output to `LayerKind::AiSuggestion` (`lib.rs:165`).**
- It is an asset-backed image kind (`lib.rs:247-251`; `project.rs:176-179`).
- `InferenceBackend` has no implementation anywhere.
- Brushes refuse non-Paint owners (`canvas.rs:1281-1284`), so retouching the result afterwards would fail silently.
- Output to a Paint layer instead.

**Sampling area:** reuse painted selection and Quick Mask (`painted_selections.rs`; `CommandId::SelectionBrush`/`MaskOverlay` at `lib.rs:659`).

---

## RET-6 Patch

**Verdict:** Accurate.

Reuse the selected-pixel transform `LayerOperationKind::Transform` with coverage (`layers.rs:453, 520-533`).

---

## RET-7 Dodge and burn

**Verdict:** Wrong or imprecise claim.

**P1 neutral layer:**
- Overlay and Soft Light layers exist (`layers.rs:364-372`) and so does Fill (`layers.rs:456-458`).
- Layer blends operate in **linear document RGB** (`rendering.md:51-53`; `scene.wgsl:73-86`).
- **Correction:** the neutral fill is 0.5 *linear* (about 73.5% sRGB), not Photoshop's 50% encoded gray (128), and the tonal response differs from Photoshop.

**P2: "fits the existing dry dab path" is imprecise.**
- A non-Normal effect needs destination reads (`layer-render-wgpu/src/lib.rs:520-523`) and is excluded from the dry compute path (`dry_material.rs:298-300`).
- These tools would be new `blend_color` codes (`material_brush.wgsl:191-203`), which clamp Add and assume values ≤1 for Screen.
- They change only the target layer's pixels, so they do nothing on an empty layer.

---

## RET-8 Blur and sharpen brushes

**Verdict:** Partially implemented already.

- A Smudge preset with pull 0 and blur 1 is already a blur brush, but its radius is capped at 4 px (`material_brush.wgsl:116-137`; `material_sources.rs:152-154`) and blur has no UI.
- The non-destructive route already exists: a Gaussian (≤21 px) or Unsharp Mask effect with a painted mask.

---

## RET-9 History brush and snapshots

**Verdict:** Partially accurate.

**History is evictable.**
- History entries are inverse `Edit`s over raster revisions (`lib.rs:1786-1799`; `project-format.md:46-52`).
- The budget is 512 MiB and 256 entries (`history_budget.rs:6-7`).
- Snapshots therefore need their own retention and their own charge in `Accounting` (`history_budget.rs:16-36`).

**GPU residency:** revision tiles live on the CPU. Painting from them needs GPU residency without uploads during contact. Reuse `Capture::source_tile` and the 16-slot `SOURCE_SLOTS` source cache (`artwork.rs:96-106`; `lib.rs:109`).

**Overlooked:** placed photos keep a permanent immutable original (`lib.rs:184-188`; `source_access.rs:1-2`). "Revert to original photo" needs no snapshot at all.

**Shared primitive:** RET-9, RET-1's self-reference case and Liquify Reconstruct all need the same stroke-start or snapshot source.

---

## RET-10 Red-eye

**Verdict:** Accurate.

Clicking an eye could reuse the GPU connected-region Wand / Select by Color (`flood.rs:1-2`; `region_tools.rs`) plus a Hue/Saturation effect, instead of new detection.

---

## T-4 Expose Liquify modes as presets

**Verdict:** Wrong claim.

**Reconstruct:**
- It is not implemented (see the inventory row).
- Even as designed, it blends toward a *stroke-start* snapshot (`advanced-brush-engine.md:199-200`), not toward the pre-Liquify original. That does not deliver Photoshop's Reconstruct and belongs with XF-5/RET-9.

**The other modes:**
- Twirl CCW, Pinch and Expand are implemented and GPU-tested (`placement_tests.rs:298-351`).
- Crystals needs `deform.distortion > 0` (`material_brush.wgsl:486-489`; packed at `lib.rs:5125-5130`). The default is 0 (`lib.rs:739-749`) and there is no UI for it.

**Labels may be inverted.**
- Pinch samples at 0.65·d, which magnifies the centre; Expand samples at 1.35·d (`placement_tests.rs:336-337`; `material_brush.wgsl:483-485`).
- That looks inverted relative to Photoshop's Pucker/Bloat and Procreate's Pinch/Expand. Check visually before exposing.

**"Where" is incomplete:**
- `tools.rs` `PRESETS` (`:178-298`) and the count test (`:748`).
- New stable IDs of 36 or higher.
- The built-in list at `presets.rs:652-677`.
- A mode *choice* is not possible in `DEFINITIONS`: they are f32-only, and overrides are stored as `BTreeMap<String, f32>` (`tools.rs:595`).

**Scheduling:** "now, before XF-5" contradicts M3, which schedules T-4 together with XF-5.

---

## T-12 Source setting for Smudge and Natural Blender

**Verdict:** Wrong or imprecise claim.

**Wrong execution type:** Natural Blender is `Smudge`, not `Wet` (`presets.rs:495-506`).

**Design conflict with RET-1.**
- Smudge relies on reading the progressively updated destination (`painterly-paint-state.md:74-78`; `gpu-brush-engine.md:227-230`).
- With a static stroke-start reference snapshot, the smear would not carry past one chunk.
- T-12 needs a live source of "target pages over the reference snapshot". That is the opposite of RET-1's rule and must be specified separately.

**Correct "Where":**
- `material_brush.wgsl:602-616, 722-734`.
- `material_sources.rs:97-100, 152-154, 399, 415`.

---

## T-14 Gradient tool

**Verdict:** Overlooked existing infrastructure.

**Multi-stop gradients already exist:**
- `GradientStop` and `EffectValue::Gradient` (`effects.rs:233-239`), validated at 2–32 stops (`:556-566`), evaluated by `gradient_value` (`:681-695`).
- Stop editors exist on GTK (`apps/layer-linux/src/gradient_preview.rs`), Android (`Effects.kt:218`) and Apple (`PropertyControls.swift:260`), through `EffectAction::GradientStop` (`layer-ui/src/effects.rs:205, 608-661`).

**The tool today:**
- `LayerOperationKind::Gradient{colors: [[f32;4];2], radial}` (`layers.rs:459-465`).
- Foreground→background or foreground→clear, times brush opacity (`art_layers.rs:1926-1942`).
- Mask gradients use a separate `SelectionGradient` path (`art_layers.rs:1914-1917`; `layer-render/src/selection.rs:33`). Both paths need updating.

**Interpolation is inconsistent:**
- The tool mixes premultiplied *linear* RGB (`scene.wgsl:176-182`).
- Gradient Map mixes straight *encoded* RGB (`effects.rs:681-695`).
- There is no dithering code anywhere.

**Scope:**
- A reflected type is a small change to `t` (`scene.wgsl:177-179`).
- Gradient recipes are not stored (`project-format.md:49-51`), so there is no `.capy` version step.
- T-14 has no milestone.

---

## M2 row

**Verdict:** Imprecise.

1. Add **T-16**. The plan requires the no-placeholders update in the same change, and `docs/ui/default-workspaces.md:24-26` says the same.
2. Add **T-15**. RET-1's "Mark a reference layer" message depends on it being shown to the user.
3. Journey 20 lists ADJ-8, which is in M5. Say "opens 20 except Dust & Scratches".
4. List the prerequisite infrastructure explicitly:
   - stroke-start copy-on-write snapshot pages;
   - a reference-composite tile cache;
   - Clone fields in `Stroke`;
   - a pen-button binding setting;
   - a retouch tool-memory slot.
5. T-14 is not scheduled in any milestone.

---

## Constraints the plan should cite

- **Read/write rule:** never sample and write the same subresource in a pass (`gpu-brush-engine.md:236-247`).
- **Ordering:** dry contacts are instanced; Smudge uses chunks of 3; Liquify uses 1 contact per swap (`gpu-brush-engine.md:249-258`).
- **Contact-time rules:** no upload or readback during contact, pipelines ready before pen-down, 8.33 ms p99 (`gpu-brush-engine.md:270-281`).
- **Prediction:** predictions never mutate persistent state (`brush-renderer.md:48-52`), and captures replay predicted contacts (`artwork.rs:160-211`).
- **Corrections:** late sensor corrections restore and replay (`corrections.rs:101-160`).
- **Selection:** coverage is captured per stroke (`brush-renderer.md:82-84`; `lib.rs:1221-1223`).
- **Linear blend domain:** `rendering.md:51-53`.

## Other gaps found while auditing

- No UI exists for the 8 brush blend modes.
- Tool settings cannot express an enum mode choice.
- The Eyedropper cannot sample reference layers (`ColorSampleSource` has only Composite and Layer), which is inconsistent with RET-1's shared source.
- Strokes on masks silently switch to Dry for every execution type.
- Brushes silently refuse non-Paint owners.
- `BrushSnapshot` only accepts schema versions 4 and 5 (`lib.rs:886-888`). Store clone parameters in `Stroke`, not in the snapshot.

## Key files

- `crates/layer-render-wgpu/src/material_brush.wgsl`
- `crates/layer-render-wgpu/src/material_sources.rs`
- `crates/layer-render-wgpu/src/artwork.rs`
- `crates/layer-render-wgpu/src/lib.rs`
- `crates/layer-ui/src/tools.rs`
- `crates/layer-ui/src/tool_settings.rs`
- `crates/layer-engine/src/canvas.rs`
- `crates/layer-core/src/effects.rs`
- `docs/reference/gpu-brush-engine.md`
