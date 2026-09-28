# Photo editing M2–M4: quick wins, foundations and retouching

[Developer guide](README.md) · [Photo editing research](../history/photo-editing-research.md) · [Phase 1 plan](canvas-action-bar-transforms.md) · [Canvas action bar](../ui/canvas-action-bar.md) · [Drag convention](../ui/drag-and-reorder.md)

Status: **in progress** (2026-09-27), written against `origin/main` at `6fcc6fba`. Done: M2, M3, M4.1, M4.2, M4.4 and Color mixing from M4.5. See [Remaining work](#remaining-work).

This plan turns milestones M2, M3 and M4 of the [research record's sequencing](../history/photo-editing-research.md#7-recommended-sequencing) into ordered, testable steps. The product specification is sections 5 and 6 of the research record. This document records:
- where the code has moved since the research baseline (`5eb45a47`);
- the decisions the research left open;
- the steps, their tests and each milestone's exit test.

Hosts in scope are GTK, Web and Android. Apple and Windows follow later through the [Apple](../APPLE_PORTING_GUIDE.md) and [Windows](../WINDOWS_PORTING_GUIDE.md) porting guides; [Apple and Windows](#apple-and-windows) lists what each step leaves for them.

Follow [AGENTS.md](../../AGENTS.md) and the [commit guide](../COMMIT_GUIDE.md) throughout:
- Keep rules, validation and history in shared Rust.
- Keep native timing, capture, dialogs, files and the clipboard in the hosts.
- Replace obsolete paths rather than adding a second implementation.
- Every command that acts on a selection, transform, crop, mask or other canvas object declares its bar context and priority in the same change, or states that it has none.

## Scope

**M2 Quick wins.** Each item lands together with its bar item.
- **Refusals:** P-4 (a disabled reason on `CommandState`, plus a shared notice), T-15, T-23, and the Apply Mask message.
- **Selection actions:**
  - T-1: an effect layer takes the selection as its mask;
  - SEL-4 with T-2: Clear Selected, Clear Outside, Delete, and Clear Entire Layer;
  - SEL-2: Copy and Cut Selection to New Layer;
  - SEL-5: Feather, Border, Smooth and Transform Outline;
  - the bar menu items they need.
- **Mode bars:** BAR-5 (Quick Mask, Selection Layer editing and artwork-mask editing), BAR-6 for guides, T-25 and T-26.
- **Small items:**
  - T-4 (Liquify modes), LYR-5 (Solid and Gradient fill layers);
  - the ADJ-1 `color_action` quick win, ADJ-11 (vignette removal);
  - T-8 (rename Denoise), RET-9 (Revert to Original Photo);
  - VIEW-1 (Actual Pixels and zoom entry), IO-2 (lossless WebP).

**M3 Foundations**
- **Geometry:** GEO-0 (the editable extent), GEO-1 Crop with the crop bar (BAR-4), GEO-2 Straighten, GEO-3, GEO-4 Image Size and GEO-5 Rotate and Flip Image.
- **Also required:** pixel-tight bounds (P-8) on the CPU, which Trim and Reveal All need, and the rest of XF-2 (Lanczos and the export aliasing fix).
- **Clipboard:** SEL-1 with P-3, including Paste Into.
- **Other:** SEL-3 (selection-aware Move), LYR-2 (merges), IO-1 (metadata) and T-16 (the Photo workspace).

**M4 Retouching**
- **Foundations:** P-5 (the Retouching tool category and source setting), P-2 (stroke-start pages and the reference cache) and a minimal P-11.
- **Tools:** RET-1 to RET-4 (Clone Stamp, Healing and Spot Healing), with the clone-source disc and its bar (BAR-6).
- **Compositing:** LYR-1 (blend modes), P-9 (the Perceptual blend space, with encoded composition and brush mixing), Pass Through groups, RET-7 (New Dodge & Burn Layer) and ADJ-9 (Frequency Separation).

**Moved out of M2–M4**
- **BAR-7 moves to M8.** No M4 job needs it: healing finishes in the pen-up frame, and content-aware fill is M8.
- **Retouch extras:** the clone pixel overlay, source rotation and scale, Spot Healing's Content-Aware and Create Texture types, Darken and Lighten clone modes, and the Visible source when the target is not the top layer.
- **Refine sliders on the bar:** the bar has no numeric items, so M2 extends the existing dialog instead.
- **Clipboard extras:** Copy Reference, lazily encoded clipboard PNGs, and the Web `paste`-event read path.
- **Metadata extras:** writing IPTC-IIM, and editing copyright or contact details in Document Properties.
- **Pass Through performance:** its checkpoint optimization.
- **Resampling on encoded values.** It is deliberately left out; see [decision 4](#decisions).
- **Per-layer linear blending for non-Normal layers** (glows with Add or Screen in Perceptual documents), as Affinity and GIMP offer. It would be cheap, but waits until artists ask for it.
- **Gradient interpolation (T-14, M6):** Oklab by default, with Linear light and Classic as choices.

## Changes since the research

The research cites `5eb45a47`. About 250 commits have landed since. These corrections change the plan:

| Research says | Current code |
| --- | --- |
| `LayerKind::ImportedImage` | Removed (`f8cfc77e`). A placed photo is a Paint layer with a `source`; its edits are raster tiles over the source. |
| T-16 keeps the old layout as `legacy_*_layout` with a migration | The upgrade chain was removed (`8842fdfd`). Saved workspaces keep their layout, and Restore Starting Layout adopts the new preset. T-16 only edits the preset. |
| `.capy` keeps reading v6 and v7 | The format is `CAPYRASTER\x08` (`project_storage.rs:15`); v6 and earlier are already unsupported. |
| A stage C contextual resolver decides chord precedence | Not built. `BindingScope` and `specificity` exist (`shortcuts.rs:265`), but every command is Application scope, and dispatch stops at a disabled match. |
| Phase 1 designed a `Refine` bar menu | Never shipped. Grow and Shrink are reachable only through More, and bar items must be commands or choices (`canvas_bar.rs:52`). |
| Pen barrel buttons are hard-wired to Pan | All three hosts send `UiInput::PenButton`, which is resolved per `ToolCategory` (`gesture_input.rs:73`). Buttons are unbound by default. |
| The material pass is at WebGPU's 16-texture limit | It binds 14 today. The bristle branch raises that to 16, so Clone still reuses a slot (`reservoir_texture`). |
| `SuggestionRequest::is_stale_for` | Deleted (`c9447410`). The staleness pattern left is the region `Target { generation, revision }` check. |
| No GPU resampling kernel | Phase 1 added bicubic sampling and adaptive supersampling to `pixel_transform.wgsl`, up to 4×4 taps. |
| `blend()` in `scene.wgsl`; `portable_blend.wgsl` blends | `blend()` is in the shared `blend_modes.wgsl`. `portable_blend.wgsl` only replaces fixed-function source-over. There is no CPU compositor: export renders through the same `Scene`. |
| `cached_clipping_matches_tiled_composition` | Deleted (`9c01e575`). Use `all_effects_incremental_masks_groups_and_clipping_match_full_recomposition` and `image_windows_match_full_composition_with_halos_masks_and_clipping`. |
| New preset IDs start at 36 because the bristle branch claims 35 | Confirmed. `origin/main` ends at 34 (`BrushedInk`). |
| Pinch and Expand may be inverted | Confirmed by the shader math: Pinch magnifies and Expand shrinks. |
| ADJ-1 `color_action` is a Rust-only change | All three hosts drop the parameter's label when `color_action` is set, so each host needs a small fix. |
| Everything is in `append_operations` | It is private (`canvas.rs:557`), and a layer cannot be inserted and given an operation in one step. SEL-2, RET-7 and the merges need that. |
| `Capture::region` feeds reference sampling | It can wait on the GPU (`submit_chunk` → `device.poll(Wait)`), so it must not run during a contact. |

**Two bugs found by the audit.** M2.1 fixes both.
- **A missing reference layer stops the canvas.** When Wand or Fill uses the Reference source with no reference layer marked, the frame fails (`region_tools.rs:181`, `session.rs:3804`). GTK then offers "Restart canvas", Web stops the GPU, and Android suspends the renderer.
- **Errors raised during a gesture are shown inconsistently.** They set `host_error` without raising `regions::HOST`. GTK and Web show them late or not at all. Android shows a modal dialog. Nothing ever clears them.

## Decisions

The user asked for these to be settled by research into other editors, without new complexity and without making the painter default worse. The painter default is the Sketch workspace (`LayoutPreset::Painter`) with the capy keymap. Evidence is in the research record's source reports and in the sources below each row.

| # | Decision | Choice | Reason |
| --- | --- | --- | --- |
| 1 | GEO-0, the editable extent | **(a-lite).** Each layer stores an extent that never shrinks with the canvas. The root layer `offset` is its origin. Growing the canvas left or up re-keys tiles in whole-tile steps. Tile coordinates stay unsigned. | A crop only changes metadata, so hidden pixels survive, and Reveal All and 90° turns of non-square canvases work. The renderer already handles layer extents that differ from the canvas, as placed photos show. It costs about the same as option (b), which is destructive and would be thrown away. Full signed coordinates (a) would touch the latency-critical brush path for no M3 benefit. |
| 2 | Crop default | Non-destructive. **Delete Cropped Pixels** is off by default and is a toggle on the crop bar. | GIMP 2.10.20 and later, Affinity and Pixelmator Pro keep cropped pixels by default. Photoshop defaults the option on, but offers it. |
| 3 | Document geometry on locked layers | It applies to them. | Photoshop does the same, and one document must stay one size. Locks protect content from editing, not from canvas changes. |
| 4 | P-9, the blend space | Each operation uses the space that gives the better result. A document setting, **Blending: Perceptual / Linear light**, sits in the New Document dialog and in Edit ▸ Blending. In Perceptual documents these run on encoded values: every blend mode, Normal opacity, masks, groups, how brush dabs lay over paint, brush blend modes, healing's tone blend, and retouching filters (blur, sharpen, High Pass). Resampling, Liquify and light-based filters stay linear. Paint colour mixing is a per-brush choice: Oklab by default, Linear light or Classic. The composite holds encoded values, and layer pixels stay linear. New 8/16-bit documents default to Perceptual, in every workspace. Existing and float documents stay Linear. Decided by the user, 2026-09-27: match existing tools with minimal performance impact. | Photoshop, Clip Studio Paint (CSP), Krita and Affinity composite and paint 8/16-bit documents on encoded values. Opacity, soft brush edges, contrast modes, neutral 50% grey, dodge and burn, Frequency Separation and retouching blurs then all match them. Hardware source-over is arithmetic that is correct in any space, so Normal layers keep hardware blending once their draw shader outputs encoded values. Brushes already read the pixels under each dab, except edged dry brushes. Filters read a window that is captured once per pixel. The added cost is a per-pixel conversion, measured as an exit gate. Encoded resampling is a known defect, and matching it costs per-tap conversions on drags; see [Blending in the Perceptual space](#blending-in-the-perceptual-space-m45). |
| 5 | Pass Through as the default for new groups | **No.** New groups stay Isolated. Pass Through becomes a blend mode that groups can use. A preference, **Use Pass Through for new groups**, is off by default (decided by the user, 2026-09-27). | CSP folders default to Normal (isolated), and CSP offers the same preference ([CSP](https://help.clip-studio.com/en-us/manual_en/180_layers/Layer_folders.htm)). GIMP and Krita also default to isolated. Photoshop and Affinity default to Pass Through, and their users can turn the preference on once. `layers-initial-design.md` is accurate. |
| 6 | Copy semantics | **Copy** takes the active layer's raw content (before opacity, masks, effects and clipping) times the selection's coverage; with no selection, the whole layer within the canvas. **Copy Merged** takes the visible composite. **Paste** always makes a new layer. A copy from Capy pastes at its original position when that is in view, otherwise centred, with no handles (T-11). An external image opens the placement bar. **Paste in Place** always keeps the position. **Paste Into** is included. | Copy follows the inventory's definition, Photoshop and GIMP. CSP pastes in place. Photoshop centres by default, and its Paste in Place stays on Ctrl+Shift+V. Paste Into is a paste in place plus Add Mask in one step (size S), and it completes the compositing journey. |
| 7 | Retouch source | Reference layers **below** the target, plus the target, in stack order, like Photoshop's "Current & Below". With nothing marked, the source is the target alone. An empty target with no references shows the T-23 notice. **Editing layer** is the other choice; **Visible** is deferred. | Later strokes see earlier fixes, and healing on an empty layer works. Only the target changes during a stroke, so stroke-start copies are needed for the target's pages only. Adjustment layers are ignored implicitly, as in Photoshop's "Ignore Adjustment Layers". The tool description states the difference from Wand and Fill. |
| 8 | Where the image commands go | An **Edit ▸ Image ▸** submenu: Crop to Selection, Canvas Size…, Image Size…, Rotate 90° Left/Right, Rotate 180°, Flip Horizontal/Vertical, Reveal All, Trim…. There is no ninth menu. | CSP keeps Change Image Resolution, Change Canvas Size and Crop in Edit. "Image" is the word Photoshop, GIMP, Krita and Pixelmator users look for. The Select menu already builds submenus on all three hosts, so no header requalification is needed. Crop's main routes are the tool, the crop bar, search and C. |
| 9 | Shortcuts | See [Shortcuts](#shortcuts). | The capy keymap gains a chord only when it is free, changes no painter binding and passes `KeyChord::available`. Photo-specific chords go in the `photoshop`, `affinity` and `gimp` presets. |
| 10 | Raising "Later" items | None, except Paste Into (decision 6). | Refine Edge, Select Subject and Select Similar stay in M7 and M8. |
| 11 | T-16, the Photo workspace | Edit the Photo preset only. Crop joins in M3, and Clone, Heal and Spot Heal in M4. | Current policy since `8842fdfd`: Restore Starting Layout adopts a new preset. |
| 12 | Bar dropdowns (Copy to Layer ▾, Clear ▾, Refine ▾, Adjust ▾, Copy ▾) | A bar item is a primary command plus an optional menu. Updated hosts open the menu; a host that does not know the field yet runs the primary command. | One item type serves every dropdown. Apple and Windows keep working until they are ported. |
| 13 | SEL-5 controls | Generalize the modal Grow/Shrink dialog into a non-modal Refine panel. Refine ▾ on the bar opens it. While the value moves, previews are computed at reduced resolution over the selection's bounds in short GPU chunks, and change no document state. When the value rests, or on Apply, the exact result runs and becomes one undo step. Grow and Shrink go up to 128 px, and Feather up to 100 px. | The bar cannot hold numeric fields. Reduced-resolution previews while a control moves are a valid approximation under the performance targets. |
| 14 | Clear Selected rules | Under alpha lock, disabled with a reason. On a placed photo, it clears the raster over the source, and Revert to Original brings it back. In Quick Mask, Selection Layer editing and mask editing, disabled with a reason (the Quick Mask bar has its own Clear). **Clear Outside Selection** has no default chord. | Refusals are visible (T-15). Unlike CSP and Affinity, Delete never removes a whole photo layer. CSP's own documentation gives two different chords for Delete Outside. |
| 15 | Delete and Backspace precedence | In order: text fields and headers; the polygon's last point; a selected guide (Ruler or Move tool); Clear Selected. Bindings get scopes, and dispatch runs the first **enabled** match in order of specificity. | This is a minimal stage C that keeps all precedence in shared Rust. |
| 16 | Healing algorithm | Poisson-style seamless cloning: a pull-push membrane plus fixed Jacobi sweeps, computed in the pen-up frame. The live preview is the plain clone. Spot Healing ships Proximity only: it tries 16 candidate offsets, scores them on the GPU, then applies the same blend. | Deterministic, with no readback. One undo step comes free because the raster is already pending at pen-up. Photoshop's healing brush also previews as a clone. |
| 17 | Liquify Pinch and Expand | Swap their signs so Pinch shrinks toward the centre (Photoshop's Pucker) and Expand bulges (Bloat). Twirl presets are labelled by the direction an oracle test measures. | Neither mode is reachable today, so nothing changes for saved brushes. |
| 18 | Denoise label | "Edge-Preserving Smooth", keeping the `denoise` id. | It is a 1–3 px bilateral smoothing filter. "Noise Reduction" is reserved for ADJ-8. |
| 19 | Dodge and burn | **New Dodge & Burn Layer** in Layer › New: a Soft Light layer filled with the neutral grey of the document's blend space. There is no New Layer dialog. | It gives the Photoshop result in one step, with no new dialog. |
| 20 | Frequency Separation | One step, as in Affinity: Low (Gaussian) and High (Linear Light) layers in an isolated group, built with LYR-2's bake. In Linear-space documents it is disabled, with a reason that points to Edit ▸ Blending. | The recombination is exact only when splitting and blending use the same domain. |
| 21 | Merges | Follow `layers-initial-design.md`. Merge Down needs a Normal upper layer. A clipping base bakes its whole clipping stack. Merge Down on an effect layer applies the effect to the layer below. Flatten and Stamp Visible use the same in-frame bake operation, measured before any worker job is added. | These rules were approved earlier. One operation serves all merges, and measurement comes before new job machinery. |
| 22 | Move with a selection (SEL-3) | Dragging moves the selected pixels. Alt, or the **Leave Copy** toggle, keeps the original. Without a selection, Move keeps its lossless layer offset. | Photoshop, and CSP's Move Layer tool with its "Keep original image" option ([CSP](https://help.clip-studio.com/en-us/manual_en/180_layers/Basic_operations.htm)). The painter default gains the CSP behaviour. |
| 23 | Export metadata default | **All metadata except location.** The choices are All, Copyright & Contact, and None, plus **Remove location**, which is on by default. Dimensions and orientation are always regenerated. | This keeps the camera, lens, date and copyright information photographers expect (the IO-1 goal) without leaking GPS. |
| 24 | `.capy` versions | One version step per pushed change that alters the format. Readers accept v8 and every later version, including recovery files. | Each milestone stays readable by every later build. |

## Shortcuts

Check each chord against the other editors' defaults ([settings](../ui/settings.md)) before binding it. Bump a preset's `revision` whenever its rows change.

| Command | capy | photoshop | affinity | gimp |
| --- | --- | --- | --- | --- |
| Clear Selected | Delete, Backspace | Delete, Backspace | Delete, Backspace | Delete |
| Copy / Cut Selection to New Layer | Ctrl+J / Ctrl+Shift+J | same, replacing `layer.duplicate` | same | none; its Ctrl+Shift+J is Fit Canvas |
| Actual Pixels | Ctrl+1, Ctrl+Alt+0 (CSP) | Ctrl+1 | Ctrl+1 | 1 |
| Feather Selection | none | Shift+F6 | none | none |
| Copy / Cut / Copy Merged | Ctrl+C / Ctrl+X / Ctrl+Shift+C | same | same | same |
| Paste (merged with Paste Image, keeping its ID) | Ctrl+V | Ctrl+V | Ctrl+V | Ctrl+V |
| Paste in Place | Ctrl+Shift+V | Ctrl+Shift+V | Ctrl+Shift+V | Ctrl+Alt+V |
| Paste Into | none | Ctrl+Alt+Shift+V | none | none |
| Crop tool | C | C | C | Shift+C |
| Canvas Size / Image Size | none | Ctrl+Alt+C / Ctrl+Alt+I | Ctrl+Alt+C / Ctrl+Alt+I | none |
| Merge Down | Ctrl+E (as in CSP) | Ctrl+E | Ctrl+E | none; its Ctrl+E is re-export |
| Merge Visible | none; Ctrl+Shift+E is Export | Ctrl+Shift+E | Ctrl+Shift+E | Ctrl+M |
| Stamp Visible | none | Ctrl+Alt+Shift+E | Ctrl+Alt+Shift+E | none |
| Clone Stamp | S | S | S | C |
| Healing, Spot Healing | none; J is Blend | J | J | H |

**Notes**
- The photoshop and affinity presets already move Export to Ctrl+Alt+Shift+W, so Ctrl+Shift+E is free there.
- Ctrl+1 switches browser tabs. Check that `preventDefault` stops it in Chrome and Firefox.
- Update the stale preset notes: "Capy has no merge-down command", "no 100% zoom", and GIMP's Delete and Ctrl+E notes.

## Design notes

### Bar menu items (M2.2)

**Shared Rust**
- `CanvasBarItem` gains `menu: Option<CanvasBarMenu>`, with `CanvasBarMenu { CopyToLayer, Clear, Refine, Adjust, Copy }`.
- The item's `option` stays the primary command.
- Menus are served through `canvas_bar_choice_menu(context, id)` as a `ContextMenu` whose items are wrapped in `CanvasBarEdit`.
- Stale-edit validation accepts any action in the current item's menu.

**Hosts**
- GTK: a `MenuButton`, filled by `populate_canvas_bar_choice`.
- Web: export `canvas_bar_choice_menu` from wasm. Web builds choice menus locally today.
- Android: `opensWindowlessMenu` with the existing `choiceMenu` query.

### Refusals and the shared notice (M2.1)

**The disabled reason**
- `CommandState` gains `disabled_reason: Option<Cow<'static, str>>`.
- `refresh_commands` runs every frame over about 160 commands, so it fills the reason only for disabled commands while the canvas is idle, and freezes it during a contact, as it already freezes `enabled`.
- A `disabled_reason_unchecked` variant skips the second `command_flags` call.
- The field always serializes, as `null` when enabled. Omitting it would change the object's keys and make Android's model diff replace whole command objects.
- Measure `refresh_commands` before and after.

**The notice**
- `UiState.notice: Option<Notice { id, text, action: Option<NoticeAction { label }> }>`, published under `regions::HOST`.
- `UiAction::Notice { id, accept }` rejects stale ids. The core keeps the action to run.
- Hosts own the timeout (about 4 s) and dismiss the notice on the next canvas contact.
- Refusals during gestures move from `host_error` to the notice. `host_error` stays for file and renderer errors. Android stops showing modal dialogs for refusals.

**Silent paths**
- A pure `CanvasEngine::stroke_refusal(tool)`, used by both `process_event` and `pen_inner` at pen-down, covers brushes on locked, missing or non-Paint targets, erasing under alpha lock, and mask strokes forced to Dry (once per mask-editing session).
- A shared `drawing_refusal(doc)` covers Move, Lasso Fill, Gradient, Figure and Fill.

**T-23:** `CommandId::UseReferenceBelow` marks the nearest visible Paint layer below the target as a reference, in one undo step. It is the notice's action, labelled "Use *layer* as Reference". The region-tool failure no longer fails the frame.

### Clear, and inserting with an operation (M2.2)

**`LayerOperationKind::Erase { alpha_locked }`**
- It draws like ApplyMask, with complemented coverage, the same watercolor settle and the same photo source-page handling.
- Its bounds are clipped to the bounds of a non-inverted selection, so clearing a small area of a 24 MP photo does not rewrite every tile.
- Clear Outside passes the selection with `inverted` toggled.

**`CanvasEngine::insert_with_operations(prefix, ops, selection_after)`**
- It makes insert and operation one undo step.
- The renderer creates an absent target from its restore source.
- SEL-2 uses it as Duplicate, then Erase outside on the copy, plus Erase inside on the source for Cut.
- RET-7 and LYR-2 reuse it.
- Reselect is recorded explicitly.
- The copy goes above `clipping_stack_top(source)`, unclipped.
- With no selection, Ctrl+J duplicates the layer, as in Photoshop, so no separate `DuplicateLayer` command is needed.
- Clearing outside a small selection on a photo copy writes override tiles across the layer. Admit them through `reserve_pending_bytes`.

### Mode and guide bars (M2.4)

**New kinds:** `CanvasBarKind::{QuickMask, SelectionLayer, LayerMask, Guide}`.
- The three modes sit at the bottom edge with a label and an exit, and they show under painting tools.
- Unlike transform completion, they hide when the bar is turned off.
- Guide anchors to the selected guide's handles, with the Ruler or Move tool.

**New commands**
- `LoadSelectionLayer`, `InvertSelectionLayer`, `InvertLayerMask`, `LayerMaskEnabled`, `ApplyLayerMask`, `EditLayerMask` and `EditLayerContent`, all for T-26.
- `LassoFill` becomes a tool command (T-26).

**Guide selection**
- A Move click that misses a guide deselects it.
- Guide actions leave Tool Options under unrelated tools.

**T-25:** Escape leaves any `selection_masks.target()`, not only Quick Mask.

### Editable extent and the geometry plan (M3.1)

**The extent**
- `LayerProperties.extent: Option<[u32; 2]>`, and `local_extent = max(canvas, stored, source extent)`. All 62 consumers inherit it.
- Fix the two inline copies that bypass it: `scene.rs:871` and `source_thumbnails.rs:188`.
- Operations with no selection, such as Fill, Gradient and Figure, are bounded to the canvas window so they do not fill hidden area.
- Brush dabs past the canvas edge may write hidden pixels, as they already do on photo layers. This keeps the brush path unchanged.
- Growing the canvas left or up rebases a paint layer by whole tiles. Its tiles are re-keyed with shared `Arc`s, its offset moves by 256·K, and its mask offset and `initial` move with it.
- A layer with a source never rebases, so a new strip beside a photo stays unpaintable, as a moved photo's does today.

**One plan for every geometry command**
- A new `layer-core/src/canvas_geometry.rs` turns `CanvasGeometry { rect, linear, interpolation, delete_outside }` into one `Edit::Batch`, containing:
  - `SetCanvasSize` (new);
  - `ReplaceLayer` for each layer, covering offsets, rebases, photo `placement`, and pending `Transform` operations for paint pixels and masks;
  - the selection, saved selections and guides;
  - effect parameters in px, scaled for Image Size.
- The limits are checked before commit:
  - the project limits: 32,768 px, 16,384 tiles including hidden ones, 1 GiB;
  - the GPU texture limit, through a new `CanvasRenderer::max_document_dimension()`.
- The camera re-centres by the crop origin.
- The renderer's resize reset must restore every layer on undo and redo. That path has never run, so a GPU test is written first.

### Crop (M3.2)

**Tool**
- `CommandId::Crop`, `LayerCanvasTool::Crop`, MoveTransform category.
- A new `layer-ui/src/crop.rs` session. It reuses the rectangle's Free/Ratio/Size constraints, the transform handles, `transform_touch_hit` and T-24 blur survival.
- Apply and Cancel are the existing completion commands, with the label "Apply crop".

**Crop bar**
- `CanvasBarKind::Crop` at the bottom edge.
- Items: Ratio ▾ (Free, Original, 1:1, 4:5, 2:3, 5:7, 16:9), Swap Orientation, Overlay ▾ (Thirds, Grid, Diagonal, Golden Ratio; O cycles them), Straighten, Delete Cropped Pixels, Reset · Cancel, Apply.
- Dragging past the canvas grows it with transparency.

**Dimmed overlay**
- A new `CropOverlay { to_crop, dim }` beside the selection overlay, drawn in `present.wgsl`.
- It is shared renderer code, so hosts draw nothing.
- Hidden pixels past the canvas are not shown in the preview.

**Straighten**
- Draw a line (Shift snaps to 15°) or type an angle.
- Paint pixels resample through a `Transform` operation into a grown extent.
- Photos, masks, selections and guides change as metadata. `RulerGeometry::transformed` is new.

### Image Size, flips and turns, resampling (M3.3)

**Image Size** uses the GPU route: `Transform` operations through the Phase 1 kernels, lossless photo placements, `Selection.affine` and `Edit::SetResolution`. Three changes come first:
- Commit and capture passes get a higher tap cap, about 16, so reductions beyond 4× do not alias. This also fixes XF-2's export aliasing for scaled-down placements.
- Pages left empty by a whole-layer reduction are pruned, so they don't count against the tile limit.
- The 1 GiB publication ceiling is measured on 16-bit documents.

**Rotate and Flip Image**
- `Transform` with Nearest sampling: exact permutations.
- Quarter turns resample through a square scratch extent, then prune.

**Lanczos-3:** a shader flag with 6×6 taps and clamped overshoot, offered in the Interpolation choice and the Image Size dialog.

**Pixel-tight bounds (P-8)** on the CPU: decode only the boundary tiles and scan alpha. This is pure Rust and runs on every host. It serves Trim, Reveal All and Crop's Fit Content.

**Dialogs:** `CanvasSizeView` and `ImageSizeView` in the `SelectionResizeView` pattern. Each host adds a 3×3 anchor picker.

### Clipboard (M3.5)

**Capture**
- `UiSession::capture_clipboard` derives the export-style project:
  - for Copy, one layer with its raw properties;
  - for Copy Merged, the visible composite.
- The copy is cropped in `SnapshotRenderer`, and coverage is applied on the CPU to the final rows.
- A single pass writes a `SourceImage` at the document's depth and profile, plus an sRGB 8-bit PNG.
- An untouched photo copied with Select All reuses its original `Arc`.
- A full 24 MP copy takes seconds on Android, so it uses the import progress and Cancel.

**The clip**
- `PixelClip { nonce, source, origin, color, png }` lives at window level: `DocumentSessions`, and the whole application on GTK.
- Across documents it is tagged Rasterized only when the colour settings match, otherwise Original with an explicit profile.

**Cut** is a capture, then Clear Selected, guarded by the document revision.

**System clipboard writers**
- GTK: a `ContentProvider` union of `image/png` and a private nonce type.
- Web: `navigator.clipboard.write` with a `ClipboardItem` created synchronously inside the key or click task, plus a custom `web` format where the browser supports one.
- Android: `FileProvider` and `ClipData.newUri`, with the nonce in `ClipDescription.extras`; only the latest file is kept.

### Merges (M3.6)

**`LayerOperationKind::Bake { members }`**
- It composites the members, isolated, into a new Paint layer through `Scene::group` with export-quality sampling.
- It runs through `insert_with_operations`, which removes the members and moves their references to the result.
- The removed members' GPU pages stay until the operation has run.

**Where it is used:** Merge Down, Merge Visible, Flatten Image, Stamp Visible and Merge Group all use it.

**Placed photos** become document pixels. The command descriptions say so.

**Budget:** a full-canvas 24 MP Float32 bake is 384 MiB, so bakes reserve pending bytes and are refused with a reason above the limits.

### Metadata (M3.7)

**Reading**
- `PhotoMetadata { exif, xmp, iptc }` (at most 64 MiB) is read on open: the Exif and GPS IFDs, plus XMP from JPEG, PNG, TIFF, AVIF and WebP.
- It is stored on the `Document`. Imports and pastes never change it.

**Export**
- `ExportRecipe.metadata { keep, remove_location }`, following decision 23.
- A fresh EXIF block with regenerated orientation, dimensions and resolution, plus a filtered XMP packet.
- It is merged into the HDR JPEG's own XMP.

**Hosts:** each export dialog gains a row.

### Blend modes (M4.4)

**Types**
- `LayerBlend` gets explicit discriminants. Codes 0–6 keep their meaning, and 17 new modes follow from 7.
- The persisted form is the variant name, so documents are unaffected.
- `LayerBlend::MENU` groups the modes Photoshop's way.
- Rust builds a grouped blend menu for all three hosts. `layer_blends` stays flat for Apple and Windows.

**Shaders**
- One `blend_composite` and one `blend_clip` in `blend_modes.wgsl` replace the four copies of the composite formula.
- One `blend_code()` helper replaces the five discriminant casts. The code carries the mode, the blend space and the float-document flag as bits.
- Non-Normal brush modes call the shared `blend`. The brush Normal path keeps its direct form because of an Adreno driver bug.

**HDR:** no result is clamped in float documents. Modes that are defined only on [0, 1] clamp their operands and are hidden from the menu in float documents, as Photoshop hides them in 32-bit.

**Changes to existing documents** (recorded in M4.4's decision record):
- Color and the other component modes use Rec. 601 weights on encoded values in Perceptual documents (Photoshop parity), and the document's luma weights in Linear ones.
- The Add clamp is lifted in float documents.

### Blending in the Perceptual space (M4.5)

**The setting**
- `Document.blend_space: BlendSpace { Linear, Perceptual }` with `#[serde(default)]`, so older files read as Linear.
- It changes through the undoable `Edit::SetBlendSpace`, and it is allowed to be Perceptual only at 8 and 16 bits. Converting a document to float sets Linear in the same `SetColor` batch.
- Changing it later changes only how layers combine. Pixels already painted keep their values.
- Photos opened as documents start in Perceptual. Old `.capy` files keep the value they were saved with (Linear).

**Where users see it.** It sits beside the document's colour space and bit depth, which are chosen and changed in the same places. Capy has no page settings; the paper is a layer.
- **New Document dialog:** a **Blending** choice under the colour space and depth, with two options:
  - **Perceptual**, described as "like Photoshop and Clip Studio Paint";
  - **Linear light**, described as "physically based".

  It is disabled at 32-bit float, with the reason "Float documents blend in linear light". It is saved with New Document presets (`NewDocumentOptions` gains a `#[serde(default)]` field).
- **Changing it later:** an **Edit ▸ Blending ▸** submenu, next to Assign Profile, Convert Color Space and Change Bit Depth. It holds two radio items, `BlendPerceptual` and `BlendLinear`.
  - There is no dialog, because the change is instant and one undo step.
  - At float depth both items are disabled, with the reason above.
  - Command search finds them.
- **Document Properties** stays read-only and gains a "Blending" row, through `DocumentInfo::describe`.
- **Host work:** the New Document field on GTK, Web and Android. The menu and the Properties row are shared Rust.
- **How other editors expose it.** When the option exists, it is buried and tied to the document's colour format:
  - Photoshop: one application-wide setting, *Edit › Color Settings › Advanced › Blend RGB Colors Using Gamma* (off, so it blends encoded values); 32-bit documents are linear.
  - Affinity Photo: a per-layer *Blend Gamma* in Blend Options (default 2.2); 32-bit documents are linear.
  - Krita: implied by the profile chosen in New Document or Convert Image Color Space.
  - GIMP: an *Image › Encoding* choice.
  - CSP and Procreate: no option, always encoded.

  A named document choice next to colour space and depth is clearer than any of these, and it keeps the default out of the way.

**What is encoded.** Layer pixels stay linear premultiplied. Only the composite and everything drawn into it hold encoded values, premultiplied: `enc(c / a) · a`, where `enc` is the document's own transfer curve (`sdr_encode` in `working_color`). That curve is sRGB for sRGB and Display P3, 563/256 for Adobe RGB, and 1.8 with a linear toe for ProPhoto.

**Composition**
- Layer draws (`scene.wgsl`, `fragment_main`) convert the sampled layer to encoded values on output. Normal layers keep hardware `PREMULTIPLIED_ALPHA_BLENDING`, which is correct on encoded values.
- Blend modes (op 4), clipping and masks read and write encoded values directly, with no conversion.
- Group scratch surfaces, checkpoints, cached compositions (`scene_images`) and the folded constant backdrop (`scene_constant.wgsl`) hold encoded values. Their cache keys include the blend space.
- Fused paths (ops 12–15, the placed-photo draw and the watercolor direct draw) convert inline.

**Effects**
- Each filter's input window is captured once per pixel. The filter's taps then read that window, so the space is chosen once per pixel, never per tap.
- **Built-in filters declare their space** in an optional manifest field, following the P-7 pattern. In ABI 3 terms, the input stays premultiplied, with its channels in the declared space.
  - Retouching filters follow the document's Blending, so they run on encoded values in Perceptual documents, as in Photoshop: Gaussian Blur, Unsharp Mask, High Pass, Sharpen, Soft Focus and the like.
  - Filters that model light stay linear: Vignette in stops, Bloom and the later optical blurs.
  - Filters with no declaration, including user filters embedded in documents, keep receiving linear input exactly as today.
- The effect's output is converted once when it is composited. Fused pointwise adjustments convert inline.

**Brushes**
- **How a dab lays over paint follows the document's Blending.** This is the `source_over` in `material_brush.wgsl`, which already reads the destination: soft edges, opacity and flow build-up.
  - It covers the dry deposit and coverage passes, the Wet deposit and Watercolor's final deposit.
  - The layer page is decoded before the composite and encoded after it.
- **Brush blend modes follow the document's Blending too.** `blend_color` calls the shared layer formulas, so an Overlay brush matches an Overlay layer.
- **Paint colour mixing is a per-brush choice, independent of the document.** This is `mix_color`, used by Smudge and blender pickups and the Wet reservoir exchange.
  - `ColorMixSpace` gains `Classic` (encoded values, as CSP mixes) beside `LinearRgb` and `Oklab`.
  - It is exposed as a **Colour mixing** choice in brush Tool Options, using the `ToolActionGroup` pattern.
  - Mixing brushes keep Oklab as their default.
- **Unchanged:**
  - Erasing is the same in both spaces, because premultiplied erase scales colour and alpha together.
  - Watercolor's pigment transport stays linear, because it is a physical model.
  - Liquify resamples existing pixels, so it stays linear, like transforms.
- **Pipelines:** `BrushPassPlan` skips the hardware-blended direct pipelines (brushes with wet or burnt edges) in Perceptual documents, as it already does on devices without float blending.
- `DabStyle` carries the space as a uniform flag. There are no new specialized pipeline variants, so every brush pipeline stays ready before pen-down.
- **Retuning:** Watercolor, Wet and Smudge presets are checked against CSP references, and retuned where their look drifts.

**Healing** computes its tone correction in the document's Blending, because Photoshop heals on encoded values.

**Readers of the composite.** Each decodes where it needs linear values:
- presentation (`present.wgsl`, before colour management, proof and the SDR and HDR rendition);
- export and snapshot rows;
- drag previews (`display_layers.wgsl`);
- the eyedropper;
- Wand and Fill on the composite, and tonal selection, which measures stops of linear luminance;
- the histogram and the local tone guide;
- thumbnails and filter previews.

M4.5 begins by listing every shader and Rust reader of the composite in the [rendering guide](../internals/rendering.md), so later readers state their space.

**Cost**
- The cost is one transfer-curve evaluation per pixel per layer draw, and one per pixel for each reader that decodes. Both are arithmetic, not memory traffic.
- Most of it lands on full recompositions, which happen on still frames. Motion frames composite few layers.
- If measurement shows it, replace `pow` with a fitted approximation that is accurate to Float32 precision.

**What stays linear, and why.** Each operation uses the space that gives the better result:

| Operation | Space |
| --- | --- |
| Layer compositing, opacity, masks, blend modes | Document Blending |
| How a dab lays over paint; brush blend modes; healing's tone blend | Document Blending |
| Retouching filters (blur, sharpen, High Pass) | Document Blending, declared by the filter |
| Paint colour mixing | Per brush: Oklab (default), Linear light or Classic |
| Transform resampling, Image Size, Liquify | Linear |
| Light-based filters (Vignette in stops, Bloom, optical blurs) | Linear |
| Watercolor pigment transport | Linear |
| Curves and Levels | Encoded (unchanged) |
| Hue ranges, gradient interpolation (T-14), eyedropper averaging | Oklab |

**Resampling** stays linear on purpose:
- Encoded resampling is the known "gamma error": shrinking dims thin bright detail.
- Matching it would mean converting 16–64 reads per output pixel during transform drags, the 120 Hz path.
- An encoded copy of each layer would cost 384 MiB more per 24 MP float layer.
- Storing pixels encoded, as Photoshop does, would rewrite the colour pipeline for this one difference.

**User choices:**
- Blending, per document.
- Colour mixing, per brush.
- Later, gradient interpolation, per gradient.

Each is a uniform flag read by a pass that already runs, so choosing Linear light costs what Capy costs today. **Not offered:**
- Per-layer linear blending for Normal layers; it would take those layers off hardware blending.
- A choice of resampling space.
- User-chosen filter spaces.

### Pass Through groups (M4.6)

**Core**
- `LayerBlend::PassThrough` is valid only on groups.
- New groups are Isolated unless the **Use Pass Through for new groups** setting is on. The setting is a `#[serde(default)]` field of `Settings`, shown with the other layer preferences on every host.
- Opacity and a mask crossfade between the backdrop and the result: `lerp(backdrop, result, opacity × mask)`.
- A clipped group composites isolated.
- Ungroup is relaxed: it is allowed at opacity 1 with no mask and no clipping.

**Renderer**
- `Scene::group_into` inlines a pass-through group's children onto the running backdrop, and propagates `stop_before`.
- `input_indices`, `clip_input`, `source_scope` and `reference_snapshot` widen to the nearest isolated ancestor.
- The first cut keeps the checkpoint search on direct children only; this is correct, and only wastes some outer work.

### Retouching (M4.1–M4.3)

**Tools**
- New tools: `Tool::{Clone, Heal, SpotHeal}`, `ToolFamily::Retouch` and `ToolCategory::Retouching`.
- Presets 36 and later; the M2 Liquify presets take 36–39, so retouch presets follow them.
- They join the Sculpting panel through `is_sculpt`, so no host panel code is needed.

**Choices**
- Source ▾ reuses the three `Selection*` source commands, keeping their persisted IDs. Their labels are unified as Visible artwork, Editing layer and Reference layers.
- Aligned, Flip and Reset Offset are commands.

**Setting the source**
- **Set Source** is a momentary command. While it is held or armed, the next pen or mouse contact sets the source instead of painting.
- Alt gets a Retouching-scoped hold, and a barrel button can be bound through the existing pen-button settings.
- A finger never sets the source (it navigates), but it can drag the disc.

**The source disc**
- Drawn as cursor segments.
- It drags immediately with every device, through `transform_touch_hit`-style routing.
- A tap shows `CanvasBarKind::CloneSource`: Aligned · Source ▾ · Flip H · Flip V · Reset Offset · Set Source.

**Stroke-start pages (P-2)**, renderer-private `RetouchSources`:
- **Reference cache.** Composites of the reference layers below the target, in `Rgba32Float` pages keyed by the reference frame's identity.
  - Captured by a dedicated `Capture` that renders into cache pages.
  - LRU of about 96 MiB.
  - Pages are prefetched in a ring around the source and hover points while idle.
- **During a contact**, a page is captured only when that cannot wait:
  - no filter windows;
  - no pending decodes;
  - room left in the upload ring.

  Otherwise the page counts as a miss, and the stroke replays exactly at pen-up.
- **Target copies.** Before a stroke first writes a target page, the page is copied GPU to GPU into a pooled page. This is neither an upload nor a readback.
- **Lifetime.** Copies are dropped at the next stroke, or when the 2 s correction window ends; replays re-make them.

**Clone rendering**
- `BrushExecution::Clone` on the fragment path. Each page needs its own gather before its deposit, so it cannot use the batched dry compute path.
- The gather reads `RetouchSources` into the `reservoir_texture` slot. The deposit is the dry loop with the gathered colour.
- `Stroke.retouch` holds the offset, the Aligned flag, the flip and the source kind for replay; it is not persisted.

**Healing (decision 16)**
- A new RGBA `heal.wgsl`, encoded at the `stroke_end` batch. The pen-up capture then includes it, which gives one undo step.
- The membrane is computed at full resolution up to about 4 MP of stroke damage, and at half or quarter resolution beyond that.

**Spot Healing**
- The live stroke deposits a translucent tint.
- At pen-up, an argmin over 16 candidate offsets writes the chosen offset to a buffer the heal gather reads.

## Steps

Each step is one commit or a short series. Each keeps every suite green and ships its tests in the same change. Order within a step: shared Rust, then GTK, then Web and Android. A group of steps (for example M2.1) is committed and pushed to `origin/main` once its exit checks pass on all three hosts.

Sizes: S ≤ ½ day, M ≈ 1 day, L 2–3 days, XL > 3 days.

### M2 Quick wins

| Step | Change | Tests | Size |
| --- | --- | --- | --- |
| M2.1 | P-4 reason field and shared notice; T-15 silent paths; T-23 `UseReferenceBelow` and the frame-failure fix; gesture errors as notices; Apply Mask message for groups and effects | Every command's `disabled_reason` equals `command_disabled_reason`; the reason is frozen during a contact; a notice for each silent path; stale notice ids rejected; `model_update` diff of the notice; Wand with no reference → `frame` Ok plus a notice whose action marks the right layer in one step. GTK: Move on a locked layer. Web: notice renderer. Android: the notice and its action | L |
| M2.2 | Bar menu items; `Erase`; Clear Selected, Clear Outside, Clear Entire Layer; the Delete resolver; `insert_with_operations`; SEL-2 Copy/Cut to New Layer; T-1 (effects and, later, fill layers take the selection as their mask) | Soft, inverted and photo selections; alpha-lock reason; one undo step each; photo `Arc` shared; clip stack placement; Delete precedence (polygon → guide → clear → header); the Web focused-tab Delete fix; Erase bounds oracle; bar menu contents and stale edits. GTK, Web and Android: Select → Copy to Layer, Select → Clear ▾, Select → Adjust ▾ → Curves, with mouse, touch and pen | XL |
| M2.3 | SEL-5 Refine dialog (Grow, Shrink, Feather, Border, Smooth) with live preview; their commands; Transform Outline; Refine ▾ | Draft, amend and cancel for each kind; oracles for Border and Smooth; Transform Outline changes only metadata; host dialog label tests; one undo step per Refine | XL |
| M2.4 | BAR-5 mode bars, BAR-6 guide bar, T-25, T-26 commands | Context, label, items and exit for each mode; hidden when the bar is off; guide anchor and Tool Options; Escape leaves Selection Layer editing. GTK `canvas_bar_tests.rs`; Web; Android journey | L |
| M2.5 | T-4 presets 36–39 with sign fixes; LYR-5 generators (`solid_color`, `gradient_fill`) in Layer › New with a reveal-all mask; ADJ-1 `color_action` and host label fix; ADJ-11; T-8; RET-9 `RevertToOriginal` | Preset count; a GPU oracle for Pinch, Expand and Twirl direction; fill oracles at four depths; every Color control has an action; negative Vignette brightens and stays ≤ alpha in SDR; Revert keeps the source and undoes in one step | L |
| M2.6 | VIEW-1 `ActualPixels` (integral translation at 90° turns) and the zoom readout menu and field; IO-2 lossless WebP (`ExportFormat::Webp`, 8-bit, ≤16,384 px, memory admission; the `layer-host` catch-all fixed) | Zoom 1 with integral translation; `SetZoom` clamps; readout tests updated on every host; WebP round trip (pixels, ICC, alpha); size and budget refusals; host format and MIME lists | L |

**M2 exit test**
- **Journeys**, keyboard-free on GTK, Web and Android with mouse, touch and pen:
  - adjust one region (Select → Adjust ▾ → Curves);
  - extract part of an image (Select → Copy to Layer);
  - cut out (Select → Refine ▾ Feather → Mask, or Clear ▾);
  - leave Quick Mask, Selection Layer editing and mask editing from their bars.
- **Refusals:** every T-15 path shows a notice on all three hosts. A Wand click with no reference offers "Use *layer* as Reference" and never restarts the canvas.
- **Performance:**
  - `native_frame_pacing` with `photo24` shows no regression;
  - brush latency is unchanged;
  - the `refresh_commands` cost is recorded;
  - the Android canvas bar journey passes on the MovinkPad 14.

### M3 Foundations

| Step | Change | Tests | Size |
| --- | --- | --- | --- |
| M3.1 | GEO-0 (a-lite), `canvas_geometry.rs`, `SetCanvasSize`, limits helper, `.capy` v9; Canvas Size dialog with anchor; Crop Canvas to Selection; Edit ▸ Image submenu | Rebase with masks and `initial`; offsets change only on root layers; an exact undo; a v8 file reads; hidden pixels survive shrinking and reappear on growing; GPU capture after a crop equals the source sub-rectangle; the resize reset restores every layer on undo and redo; painting in a new strip. Host dialog tests | XL |
| M3.2 | GEO-1 Crop tool and BAR-4 bar; `CropOverlay`; GEO-2 Straighten (line, angle, from a Straight guide on its bar); T-16 Crop in the Photo preset | Ratio, handles and touch; one undo step; straighten against a CPU rotation oracle; locked layers follow. GTK `crop_tests.rs` with mouse, touch, pen and the bar; Web crop journey; Android crop test; 120 fps crop-handle drags on `photo24` | XL |
| M3.3 | P-8 on the CPU; Trim, Reveal All, Fit Content; GEO-5 flips and turns; XF-2 Lanczos and the higher tap cap; GEO-4 Image Size with page pruning and `SetResolution` | Exact flip and turn permutations; Lanczos against a CPU oracle; a zone plate exported at 1/8 scale against `RowResampler`'s area reduction; a 24→6 MP Image Size leaves no stray tiles; timing of Image Size on a 24 MP photo on GTK and the MovinkPad 14 | XL |
| M3.4 | SEL-3: Move with a selection moves pixels; `keep_source` on `ImageTransform`; Alt and Leave Copy; locked and Background layers refuse with a notice | Session tests; `keep_source` oracle; GTK journey-26 scenario; Web and Android toggle tests | L |
| M3.5 | SEL-1 and P-3: Copy, Cut, Copy Merged, Paste (relabelled Paste Image), Paste in Place, Paste Into; Copy ▾ on the selection bar; window-level clip; host writers | Capture derivation, colour tagging, paste in place, Cut guarded by revision; text focus wins over Ctrl+C/V. GTK copy→paste round trips, including between documents and to another app; Web clipboard test; Android copy→paste and an external app reading the URI; copy latency recorded for 24 MP | XL |
| M3.6 | LYR-2: `Bake`; Merge Down (and Apply Effect), Merge Visible, Flatten, Stamp Visible, Merge Group | The rules; one undo step; references transferred; composite oracles; budget refusals; Flatten time on a 24 MP photo with 10 layers on GTK and the MovinkPad 14 | XL |
| M3.7 | IO-1 metadata, read on open and written on export; `.capy` step; the export row on every host | Field selection; GPS removal; regenerated orientation; XMP filtering; a recipe preset round trip; host export tests | L |

**M3 exit test**
- **Journeys** on all three hosts:
  - crop to a ratio and apply, then Reveal All brings the pixels back;
  - straighten along the horizon;
  - Canvas Size with an anchor;
  - Image Size;
  - rotate and flip the image;
  - Select → Copy → Paste, within a document, between documents and to another app;
  - Move a selection, with and without Leave Copy;
  - Merge Down, Flatten and Stamp Visible;
  - export JPEG and WebP keeping camera, lens and copyright, without GPS.
- **Performance:**
  - crop handle drags and selection moves hold 120 fps on `photo24` on GTK;
  - crops and canvas changes apply with no pixel work;
  - Image Size, Flatten and copy times are recorded in the commit messages.

### M4 Retouching

| Step | Change | Tests | Size |
| --- | --- | --- | --- |
| M4.1 | P-5: Retouching category; Set Source (momentary, Alt, barrel); source choices; P-2 reference cache and stroke-start target copies; minimal P-11 prefetch and miss replay | Stroke-start oracle (cloning over the stroke's own dabs reads stroke-start pixels); cache keying and eviction; no uploads or readbacks during contact (counters); miss → exact pen-up replay; replay determinism | XL |
| M4.2 | RET-2 Clone Stamp: tools, presets, stroke fields, the Clone pass, the disc and its bar; T-16 retouch tools in the Photo preset | An integer offset clones exactly; a second pass sees the first; flip, selection clip, alpha lock; Aligned versus non-aligned. GTK `--tablet`: Alt-click, a barrel binding, the disc dragged by pen, mouse and touch, the disc bar; Web and Android journeys; 8.33 ms p99 at 100, 300 and 700 px on a 24 MP reference | XL |
| M4.3 | RET-3 Healing and RET-4 Spot Healing (Proximity) | Texture kept and tone matched (mean within ε of the destination, high-pass correlation > 0.9); a spot on a uniform texture; one undo step; replay determinism; pen-up heal time at 0.25, 1 and 4 MP on GTK and the MovinkPad 14 | L |
| M4.4 | LYR-1 modes, the grouped menu and the brush merge; decision records superseded | A per-mode oracle at U8, U16, F16 and F32 (±1 code, 1e-5 in float) across layer, clip, effect, folded and `ImageComposition` paths; live composite equals snapshot rows; a `layered_blend_composite` bench at 120 fps on 24 MP | L |
| M4.5 | P-9 Perceptual blending: encoded composite and caches; effect windows in each filter's declared space; dab composites and brush blend modes in the document space; the per-brush Colour mixing choice with `Classic`; composite readers; the New Document field, Edit ▸ Blending and the Properties row; the new-document default; the `.capy` step; `rendering.md` and `layers-initial-design.md` updated | **Linear documents render byte-identically to before** across every oracle suite. Perceptual matches an independent CPU encoded compositor: black at 50% over white gives 128 ± 1 at 8 bits; every mode, mask, group and clip; a soft brush edge's profile; an Overlay brush equals an Overlay layer. Gaussian Blur on a hard black and white edge gives Photoshop's midpoint (128 ± 1) in Perceptual and 188 in Linear; undeclared filters receive exactly today's input. Colour mixing gives the same result in both document spaces. Export equals the live composite. Changing the space is one undo step, and a previous-version file reads as Linear. New Document presets round-trip the choice, and older settings read without it. **Performance gate:** GTK `native_frame_pacing` with `photo24` in both spaces, brush latency within 8.33 ms p99 in Perceptual, and the Android bar and brush benchmarks on the MovinkPad 14, each within noise of Linear | XL |
| M4.6 | Pass Through groups; the new-group preference | The preference picks the new group's mode, and older settings files read with it off; nested groups, opacity, mask, an adjustment inside, a clip stack inside, against full recomposition; Ungroup preserves appearance; drag preview fallback | L |
| M4.7 | RET-7 New Dodge & Burn Layer; ADJ-9 Frequency Separation | Neutral codes (128 and 32768); one undo step; Frequency Separation reconstructs within 2 codes at 8-bit and 2/65535 at 16-bit; disabled with a reason in Linear documents | M |

**M4 exit test**
- **Journeys:**
  - clone and heal on an empty layer over a reference photo, with pen and mouse, and by dragging the disc with a finger;
  - spot-heal a blemish;
  - dodge and burn on a neutral layer;
  - Frequency Separation;
  - a Pass Through group of adjustments;
  - in a new 8-bit document, a 50% black layer, a soft black brush edge and an Overlay layer match Photoshop's values within one code, while an existing document looks exactly as before.
- **Brush budget:** clone and heal contacts, and ordinary brushes in Perceptual documents, stay within 8.33 ms p99 on GTK, with no upload or readback during contact and pipelines ready before pen-down.
- **Tablet:** on the MovinkPad 14, clone, heal and ordinary strokes, drags, pan and zoom hold 120 fps.
- **Composition:** 24 MP composition with non-Normal layers and Pass Through holds 120 fps on GTK while panning and zooming, in both blend spaces.

## Order if time runs short

- **Milestones:** land in order: M2, then M3, then M4. Stop only at a step-group boundary, with that group committed, pushed and recorded in [Remaining work](#remaining-work).
- **M3:** geometry (M3.1–M3.3) opens journeys 1–4, so it goes first. SEL-3 and SEL-1 finish journey 26. LYR-2 must land before M4.7. IO-1 is independent.
- **M4:** the named feature, Clone, goes first (M4.1–M4.3), then compositing (M4.4–M4.7).
- **Units that can be cut whole:**
  - Transform Outline;
  - Trim, Reveal All and Fit Content, together with P-8;
  - GEO-4;
  - Paste Into;
  - IO-1;
  - Pass Through;
  - Spot Healing.

## Execution

- **Branch:** `git fetch`, then branch from `origin/main`. Install the hooks (`sh tools/git/install-hooks.sh`) and never bypass them.
- **Tracks:**
  - One implementer takes shared Rust and GTK.
  - Web and Android agents then work in parallel, each in its own worktree.
  - No more than three agents run at once.
- **Verify before pushing:** check each agent's commit yourself (build, suites, and a tree that matches what was measured).
- **Tablets:** reserve and run through `tools/devices/devices.py` ([devices](devices.md)). Performance targets count only on each tier's reference tablet ([performance targets](../PERFORMANCE_TARGETS.md)); the Huion Kamvas Pad 12 serves pen-input journeys.
- **Shared renderer changes:** after any of them (M2.2 `Erase`, M3.1, M3.3, M4.1–M4.6), run the Android journey `AndroidInteractionTest#canvasActionBarJourneysAcrossDevices`, not only GTK.
- **Cargo:** run `cargo test -p layer-core -p layer-engine -p layer-render-wgpu -p layer-ui -p layer-host`, then Clippy. [Known failures on main](testing.md#known-failures-on-main) are not this work's to chase.
- **Performance gaps that already exist on main:** GTK `native_frame_pacing` with `photo24` presents the Transform scenario slower than GPen, Pan and Hand.

## Apple and Windows

Record each step's host needs in `apps/layer-apple/README.md`, the Windows README and the bar guide's host sections.
- **M2:**
  - bar menu items;
  - the notice, and the command reason key;
  - mode and guide bar labels;
  - the Refine dialog label;
  - the zoom readout control;
  - WebP in the export format list and file types;
  - the labelled Color row with `color_action`.
  - Apple also does not show errors raised during a gesture; the notice fixes that.
- **M3:**
  - the Canvas Size and Image Size dialogs with the anchor picker;
  - the Edit ▸ Image submenu;
  - the crop tool icon (the overlay is shared renderer code);
  - clipboard image writers;
  - the export metadata row.
- **M4:**
  - retouch tool icons;
  - the grouped blend menu;
  - the New Document Blending field (the menu and Properties row are shared);
  - the Colour mixing brush choice;
  - the Use Pass Through for new groups setting.
  - The disc and its bar are shared.
- **Label-based tests:** Apple has them (for example the preset count in `CanvasToolChecks.swift`), and they need updating.

## Risks

- **The notice and the reason field:** they add to every publication. Keeping reasons frozen during contacts and always serializing the key keeps diffs small; measure both.
- **Delete precedence** without a full stage C resolver: the scoped-binding change touches every dispatch. Test the precedence order exhaustively.
- **Resize reset (GEO-0):** the renderer's resize reset has never run on undo and redo. M3.1 starts with that GPU test.
- **Hidden tiles** count against the 16,384-tile limit. When a crop is refused for that reason, the message points to Delete Cropped Pixels.
- **Clipboard platforms:**
  - Web clipboard writes need user activation;
  - custom formats are Chromium-only;
  - Android needs a `FileProvider`.
- **Bake memory:** a 24 MP Float32 bake can evict older history within the 512 MiB budget.
- **Reference capture during contact:** the capture can wait on the GPU. Prefetch, the no-wait rule and pen-up replay mitigate it; measure the miss rate.
- **Float32 pages are 1 MiB each.** The reference cache, stroke-start copies and heal pyramids compete for memory on tablets.
- **Pen-up heal time on mobile:** if it exceeds a frame, split levels across frames, holding back the raster capture.
- **Perceptual blending:**
  - **Breadth.** A reader of the composite that misses its conversion shows wrong colours in one place. M4.5 lists them first, and the byte-identical Linear test guards existing documents.
  - **Brush feel changes in new documents,** in every workspace: soft edges and low-opacity build-up look as they do in CSP and Photoshop. Watercolor, Wet and Smudge were tuned in linear and may need retuning.
  - **Conversion cost on tablets:** measured as an exit gate, with a fitted curve as the fallback.
- **Remaining difference from Photoshop** in Perceptual documents: transform resampling works on linear values, on purpose.
- **Filter declarations:** a built-in filter that declares the wrong space gives a subtly wrong result. Each declared filter gets an oracle test against the Photoshop reference.
- **Pass Through caches:** these caches have regressed before. A stale widened backdrop is the likely failure.
- **A larger shader include:** `blend_modes.wgsl` grows in every effect pipeline, which raises register pressure on Adreno and Mali. Measure with the filter microbench.
- **Scope:** by the audits' estimates, M2 is about the size of Phase 1, M3 about half as large again, and M4 larger still now that Perceptual blending covers Normal layers and brushes. [Order if time runs short](#order-if-time-runs-short) defines the stopping points.

## Remaining work

**Done**
- **M2.1** on GTK, Web and Android:
  - disabled reasons on `CommandState` and the shared notice;
  - T-15 refusals, and T-23 with `UseReferenceBelow`; a Wand or Fill click with no reference no longer fails the frame;
  - Android shows refusals and dispatch errors as notices instead of modal dialogs;

- **M2.2** on GTK, Web and Android:
  - bar menu items (Copy to Layer ▾, Refine ▾, Adjust ▾, Clear ▾);
  - Clear Selected and Clear Outside Selection through a bounded `Erase` operation; Clear Entire Layer;
  - Delete and Backspace resolved by scoped bindings, and a focused drawing tab keeps Delete on Web and Android;
  - Copy and Cut Selection to New Layer in one step through `CanvasEngine::insert_with_operations`;
  - new effects masked by the selection (T-1).
- **M2.6** on GTK, Web and Android:
  - Actual Pixels (Ctrl+1 and Ctrl+Alt+0) at whole-pixel translations;
  - a zoom readout that opens a zoom field and the shared zoom menu;
  - lossless WebP export ("WebP · lossless", 8-bit RGB, refused above 16,384 px before rendering).
- **M2.5** on GTK, Web and Android:
  - Liquify Twirl Clockwise, Pinch, Expand and Crystals presets (36–39), with Pinch and Expand now matching Photoshop's Pucker and Bloat. The existing preset 13 measured counterclockwise, so it is now labelled "Liquify Twirl Counterclockwise".
  - Solid Color and Gradient Fill layers in Layer › New, masked by the selection.
  - "Use current colour" buckets on every effect colour, in labelled rows.
  - Vignette down to −100; Denoise renamed "Edge-Preserving Smooth"; Revert to Original Photo.
- **M2.4** on GTK, Web and Android:
  - bottom-edge bars for Quick Mask, Selection Layer editing and layer-mask editing, each with a label and an accented exit;
  - a guide bar (Delete, Snap, Guides) beside a guide selected with the Ruler or Move tool;
  - Escape leaves Selection Layer and mask editing;
  - commands to load and invert Selection Layers, invert, enable and apply layer masks, and move between a layer's mask and its content;
  - Lasso Fill as a tool command.
  - Bar precedence is transform, polygon, guide, mode, then selection.
- **M3.1** on GTK, Web and Android:
  - the editable extent: layers keep pixels outside the canvas; `.capy` v9 reads v8;
  - one canvas geometry plan (`canvas_geometry.rs`, `Edit::SetCanvasSize`), with limits checked before commit;
  - Canvas Size… with a 3×3 anchor, and Crop Canvas to Selection (Crop on the selection bar);
  - the Edit ▸ Image submenu.
- **M2.3** on GTK, Web and Android:
  - a non-modal Refine panel for Grow, Shrink, Feather, Border and Smooth, with a live preview;
  - Transform Outline (Free and Uniform only);
  - Refine ▾ with Feather as its primary command.
- **M3.2** on GTK, Web and Android:
  - the Crop tool with Ratio and Overlay choices, Swap, Straighten (a drawn line or an angle), Delete Cropped Pixels, Reset, and Fit on the crop bar;
  - a dimmed crop overlay drawn by the shared renderer;
  - Straighten Image to Guide on the guide bar;
  - Crop in the Photo workspace.
- **M3.3** on GTK, Web and Android:
  - Image Size… with Lanczos, Constrain proportions and resolution;
  - Rotate 90° Left/Right, Rotate 180°, and Flip Horizontal/Vertical for the image;
  - Trim, Reveal All, and Fit Content on the crop bar, from CPU content bounds;
  - resampling drops the pages it empties;
  - exports of scaled-down photos no longer alias.
- **M3.4** on GTK, Web and Android: with a selection, Move drags the selected pixels in one undo step; Leave Copy (or Alt) keeps the original.
- **M3.5** on GTK, Web and Android:
  - Copy, Cut, Copy Merged, Paste, Paste in Place and Paste Into, with Copy ▾ on the selection bar;
  - a window-level clip that keeps a copy from Capy at full fidelity;
  - PNG on the system clipboard on each host.
- **M3.6** on GTK, Web and Android:
  - Merge Down (which applies an effect to the layer below, or merges a clipping stack), Merge Visible, Flatten Image, Stamp Visible and Merge Group, through one isolated `Bake` operation;
  - Flatten confirms through the shared notice when it would discard hidden layers.
  - Merge Down needs a Normal, visible layer below. Merge Visible and Flatten accept any blend mode, with the paper kept separate, so a non-Normal layer can look different where the paper shows through.
- **M3.7** on GTK, Web and Android: photos opened as documents keep their Exif, XMP and IPTC in `.capy` (version 10); exports keep camera, lens, exposure, dates, copyright and contact, with location removed by default; a Metadata row in each export dialog.
- **M4.1–M4.2** on GTK, Web and Android: Clone Stamp (preset 40) with a source disc that drags with every device and a bar (Aligned, Source ▾, Flip H/V, Reset Offset, Set Source); Set Source with Alt or a bound pen side button; sources read from stroke-start pages and a cached reference composite, with no upload or wait during contact and a pen-up replay after a miss.
- **M4.5, Color mixing** on GTK, Web and Android: a per-brush Color mixing choice (Oklab, Linear light, Classic) in the Tool Options of brushes that mix paint; Classic mixes encoded values. Smudge and Natural Blender now mix in the Oklab their presets declare.
- **M4.4** on GTK, Web and Android: 17 more blend modes (24 in all) from one shared set of formulas on every composite path and for brushes; a grouped blend menu from shared Rust; modes defined only on 0–1 hidden in float documents.

**Follow-ups**
- **Erase right after a stroke:** an Erase on a raster that is still pending, or that holds watercolor or wet state, damages the whole layer so the layer settles. Clearing right after a stroke therefore rewrites every page. It is a still-frame cost.
- **Copy to New Layer** drops the source's mask and clipping, and the copy is unlocked. Copying from a locked layer is allowed; cutting is not.
- **Liquify render rate:** a 240 Hz stylus Liquify stroke renders more often than its input rate on Android. Check whether contacts are rendered more than once.
- **Stale Web filter copy:** `apps/layer-web/filters` was committed in `3618b3c2`, although nothing reads it (filters load from `assets/filters`). It is out of date; delete it or regenerate it.
- **Headless Web `--selection-tools`:** it now stops at its first workspace submit on `origin/main` too.
- **GTK tablet proxy:** GTK `--tablet` runs lose their Wayland connection whenever Quick Mask or Selection Layer rows change (also on `origin/main`), so `native_canvas_bar_modes` runs with mouse and touch only.
- **Android bar captions** clip their last glyph (for example "Apply", "Disable" and "Edit Content").
- **Accessible names:** relabelled bar buttons are announced by their command's label ("Enable Layer Mask" for a button reading "Disable").
- **Headless Web `--layers`** stops at "Delete mask" because `add_mask` is refused right after a Lasso Fill stroke; this also happens on `origin/main`.
- **Canvas size changes rebuild every layer:** the renderer's resize reset re-uploads every layer after a canvas size change, even when only the canvas window moves. Keep layer textures when their extents are unchanged; scheduled before M3.2 ships.
- **Undo after a canvas change** is briefly disabled and says "Nothing to undo".
- **Hidden pixels** can still be written by brush dabs past the canvas edge and by a fill through an inverted selection.
- **Reselect** keeps the selection's position on undo of a canvas change.
- **Eyedropper:** choosing another tool while the Eyedropper is active returns to the previous tool (also on `origin/main`).
- **Web frame rate on tablets:** the Web host redraws the whole WebGPU canvas every animation frame, which limits motion on the Huion even without other work. Skip unchanged presents.
- **Canvas size changes:** the first frame after one recomposes the whole display. Recompose visible tiles first, and show the shifted old display meanwhile. Keeping layer pages across the change (local branch `canvas-resize-textures`) did not shorten it.
- **Flaky test:** `live_display::tests::moving_transforms_drawn_into_the_display_match_recomposition_and_release_exactly` fails intermittently under heavy parallel GPU load, also on `origin/main`.
- **Straighten and Delete Cropped Pixels** leave vacated transparent pages until M3.3's pruning. Delete Cropped Pixels trims masks by tile, so a band under 256 px of mask coverage can remain.
- **Web on a tablet:** twice, interior tiles drew white after a crop or straighten Apply in the Huion's Chrome; it did not recur in six later runs.
- **Android Tool Options numbers** that are not sliders need two taps: one shows the field, one focuses it.
- **Image commands:** undo of a turn or resize does not re-centre the view, and the resampled-tile prediction can overcount by one row or column.
- **Android Tool Set** lists Crop twice while the Crop tool is active.
- **Moving a Select All selection:** releasing the drag on a 24 MP photo replays the commit in one still frame, long enough on tablets to delay a drag started right after. Spread the replay across frames.
- **Leave Copy** is not remembered across sessions.
- **Copying on tablets:** a composed 24 MP copy on the Huion spends most of its time reading the composite back from the GPU in bands.
- **Disabled shortcuts:** a shortcut pressed while its command is disabled gives no notice (for example Ctrl+C while a selection is still being prepared).
- **GTK pen clipboard journey:** under `--tablet`, the proxy loses the Wayland connection at the first clipboard write, so pen is covered on Web and Android.
- **Merging on tablets:** a 24 MP merge takes one to two seconds on the Huion's canvas thread, and the Android UI shows a 150–250 ms frame afterwards, probably the layer list. Trace it.
- **Metadata:** writing IPTC-IIM, and Extended XMP for packets larger than one JPEG segment.
- **Soft Light** uses the W3C formula; Photoshop's differs. Decide in M4.5 whether Perceptual documents use Photoshop's.
- **Properties panel** still offers a flat blend choice in code order, including modes hidden from the menu in float documents.
- **Clone Stamp on tablets** is GPU-bound: its 0.08 spacing lays about 38 dabs per update against the G-Pen's 6, and each page runs its own gather. Wider spacing and one gather per update are the likely fixes.
- **Clone source bar on tablets** sits at the bottom of the work area instead of beside the disc.
- **Web tests on tablets:** after a run leaves an unsaved document, the next `device.test.mjs` load waits on "Recover drawing?".
- **Preference actions** sent while Settings is closed are refused with no visible error.
- **Brush previews** 10 (Smudge) and 24 (Natural Blender) still show their linear mixing; regenerating them on this machine changes every preview slightly.
- **GTK Tool Options** put every grouped checkbox option in one radio group, which would misbehave if two groups ever showed at once.
- **Test timing:** the Android notices test raced a pending Move pointer-up; it now waits for the canvas to be idle before invoking Hand.
- **Android:** right after a stylus Wand selection is published, a layer edit can briefly be refused with "Finish the canvas interaction first". The notice test waits for `add_layer` to be enabled.
- **Apple and Windows:**
  - present `UiState.notice` and answer `UiAction::Notice`;
  - read `CommandState.disabled_reason`;
  - retire `canvas_bar_reason` once Apple reads the field;
  - open bar menu items (`CanvasBarItem.menu` and `icon`, through `canvas_bar_choice_menu`);
  - add icons for the new commands to Apple's coverage list;
  - draw a `checkable: false` action unpressed even when its command is selected (Android did not);
  - the Canvas Size dialog (`layer_tools.canvas_size`) with its anchor picker;
  - the labelled Color row with a "use current colour" bucket; Apple's `CanvasToolChecks.swift` must expect the new Liquify labels (Push, Twirl Counterclockwise, Twirl Clockwise, Pinch, Expand, Crystals);
  - the zoom readout control (the `zoom_menu` query and `UiCatalog.zoom`), and WebP in the export lists and file types. The WebP edits to Apple's `ExportForm.swift` and `ProjectFiles.swift` and to Windows' `ExportForm.h` are untested.

**Remaining:** M4.3 and M4.5–M4.7. M2 and M3 are complete. Record milestone completion in the research record's section 7.
