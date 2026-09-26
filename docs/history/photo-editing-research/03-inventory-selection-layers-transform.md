# Code inventory: selection, layers, transform and retouching

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-25 · baseline `dac76c20`

Read-only code inventory made by an agent against baseline `dac76c20`. It lists what existed in code at that commit, with `file:line` references. The implementation audits (reports 05–10) corrected several of its claims; the [research record](../photo-editing-research.md) reflects the corrections.

---

I checked this against HEAD `dac76c20` on 2026-09-25. The code's own enums are the source of truth here. The docs are only used to tell implemented work apart from planned work.

**Where the main enums live:**
- `CommandId`: crates/layer-ui/src/lib.rs:521
- `Tool`: crates/layer-ui/src/tools.rs:8
- `LayerCanvasTool`: crates/layer-ui/src/art_layers.rs:9
- `LayerAction`: art_layers.rs:117
- `SelectionTool`: crates/layer-ui/src/selection_tools.rs:9
- `SelectionAction`: crates/layer-ui/src/selection_masks.rs:45
- `LayerKind`: crates/layer-core/src/lib.rs:162
- Document `Edit`: lib.rs:1642
- `LayerBlend`: crates/layer-core/src/layers.rs:364

The application menus are File, Edit, Layer, Select, Filter, View, Window and Help (crates/layer-ui/src/application_menu.rs:33). There is no Image or Canvas menu.

---

## 1. Selection

**Doc status**
- selection-tools.md and tonal-selection.md describe implemented work.
- paintable-selection-proposal.md says "Core implementation complete on GTK, Web, Android, macOS, iPadOS and Windows".
- selection-command-inventory.md (2026-09-23) splits items into Existing and Core (both implemented) and Next and Later (not implemented).
- saved-selections-assessment.md is the agreed design, and its Core scope is implemented.

**Data model**
- A `Selection` (crates/layer-core/src/selection.rs:214) is one of two shapes:
  - even-odd contours (holes and islands allowed), or
  - GPU-produced pixel coverage (`SelectionPixels`, 8-bit "byte coverage", with an older 0..4 format still readable).
- Each selection also carries an `affine` placement and an `inverted` flag.
- Soft (partial) coverage is kept throughout. There is one current selection per document.

| Item | Status | Evidence / notes |
|---|---|---|
| Rectangle, Ellipse | Exists | `SelectionTool::Rectangle/Ellipse`. Options Free / Fixed Ratio / Fixed Size (`SelectionConstraint` selection_tools.rs:61), and draw from center |
| Freehand lasso | Exists | `SelectionTool::Lasso` → `LayerCanvasTool::Select`. Shortcut M |
| Polygonal lasso | Exists | `SelectionTool::Polygon`. 45° constraint (option or Shift). Complete and Cancel commands |
| Magic wand ("Auto select") | Exists | `SelectionTool::Wand` → `LayerCanvasTool::Region{fill:false}`. Contiguous via `RegionRequest.contiguous` (crates/layer-render/src/lib.rs:423). Shortcut W |
| Select by Color (global color range) | Exists | `SelectionTool::Color`, non-contiguous. Same classifier as the wand |
| Wand/Color options | Exists | `RegionTools` (crates/layer-ui/src/region_tools.rs:7): tolerance, Close gaps (≤32 px), Expansion (±32 px), Edge smoothing. Samples Visible artwork, Editing layer or Reference layers (`RegionSource` art_layers.rs:39). Classification runs on the GPU with no CPU pixel download |
| Tonal/luminosity selection | Exists | `SelectionTool::Tonal`, `TonalOptions` (crates/layer-ui/src/tonal_selection.rs:18). Presets Shadows, Mid-shadows, Midtones, Mid-highlights, Highlights; Bright HDR (float documents only); Custom interval in stops. Also Softness and Feather. Click samples a 5×5 area; drag-rectangle samples the 90% percentile range. Reads the visible composite and is HDR-aware. The older multi-band editor commands are retired (`available_on` returns false) |
| Paint selection (paintable/quick selection brush) | Exists | `SelectionTool::Brush`, `SelectionBrushOptions` (crates/layer-ui/src/painted_selections.rs:11): Add/Subtract, size, hardness, opacity, pressure controls size. Stroke code is in crates/layer-engine/src/selection_stroke.rs. This is a painted brush, not an edge-aware "Quick Selection" |
| Subject, sky or object select; AI select; edge-aware quick select | Absent | Listed as "Later" in the inventory §9 |
| Modes New/Add/Subtract/Intersect | Exists | `SelectionMode` selection.rs:120. Combined on the GPU. Modifiers are chosen before the gesture: Shift = Add, Alt = Subtract, Shift+Alt = Intersect, Ctrl = New |
| Anti-alias | Exists | `SelectionOptions.antialias` (selection_tools.rs:69) |
| Feather while drawing | Exists | `SelectionOptions.feather`, 0–100 px Gaussian (`SelectionRefinement::MAX_FEATHER`, layer-render lib.rs:414) |
| Feather an existing selection (Modify → Feather) | Absent | Inventory marks it "Next" |
| Grow / Shrink | Exists | `SelectionAction::BeginResize{grow}`. Circular, 1–128 px, keeps soft values. Applies to the current selection, Quick Mask or a saved mask. Menu at selection_masks.rs:233 |
| Border / Smooth | Absent | "Next" |
| Invert, Select All, Deselect | Exists | `InvertSelection`, `SelectAll`, `Deselect`. Shortcuts Ctrl+Shift+I, Ctrl+A, Ctrl+D |
| Reselect | Exists | `CommandId::Reselect`, Ctrl+Shift+D (selection_masks.rs:747) |
| Quick Mask | Exists | `CommandId::QuickMask`, shortcut Q (selection_masks.rs:732). Shows a temporary Layers row with reserved ID 0. Brush, eraser, bucket fill and gradient can paint the mask. Two painting conventions (`SelectionPaintBehavior` selection.rs:7): "Paint selection" and "Grayscale mask". Overlay color and opacity are adjustable. Move, Transform, LassoFill, Figure (art_layers.rs:991) and Filters are blocked while it is active |
| Saved selections ("Selection Layers", like alpha channels) | Exists | `LayerKind::Selection` and `SelectionTarget::Saved` (selection.rs:44). New Selection Layer and Save as Selection Layer. Load has Replace/Add/Subtract/Intersect and Load Inverted. Replace from Current Selection. The stored mask can be inverted, cleared, filled, grown or shrunk. Rows can be renamed, duplicated, deleted, locked and grouped. Ctrl/Cmd-click a thumbnail to load (Shift adds, Alt subtracts). Saved in crates/layer-core/src/project_storage/selections.rs |
| Selection from layer alpha, or from a layer mask | Exists | `coverage_menu_items` (selection_masks.rs:574), e.g. "Select Layer Opacity", each with Add/Subtract/Intersect. Uses raw content alpha. Group or effect-result alpha is "Later" |
| Selection from R/G/B or luminance channels | Absent | "Later". Tonal range covers luminance |
| Transform Selection (outline only) | Absent | "Next". However, Scale/Rotate on selected pixels moves the selection along with them, as one undo step |
| Refine Edge / Select and Mask | Absent | "Later" |
| Select Similar, Grow-by-similar, selection ↔ path, stroke selection | Absent | "Later" |
| Show Selection Outline toggle | Exists | `CommandId::SelectionOutline` |

---

## 2. Layers

**Layer kinds** (lib.rs:162)
- `Paint`. Imported photos become Paint layers that keep their original image as `source`.
- `ImportedImage`, which is the older asset-based import.
- `AiSuggestion`, which exists in core and the renderer only and has no UI.
- `Background` (paper color).
- `Group`.
- `Effect`, a non-destructive adjustment/filter layer.
- `Selection` (saved mask).

These do not exist: text layers, vector or shape layers, and fill layers (solid, gradient or pattern). Vector support is research only (docs/history/vector-layers-research.md says "not implemented capabilities"). `EffectKind::Generator` (crates/layer-core/src/effects.rs:50) is supported by the renderer, but no generator ships, so fill layers count as absent or at best partial.

| Item | Status | Evidence / notes |
|---|---|---|
| Blend modes | Partial | `LayerBlend` has 7 modes: Normal, Multiply, Screen, Add, Overlay, Soft Light, Color. Missing: Darken, Lighten, Color Dodge, Color Burn, Linear Burn, Hard Light, Vivid/Linear/Pin Light, Hard Mix, Difference, Exclusion, Subtract, Divide, Hue, Saturation, Luminosity, Dissolve. Groups are always isolated, so there is no Pass Through. Brush blend modes are a separate enum (`BrushBlendMode` lib.rs:439: Normal, Multiply, Screen, Add, Subtract, Darken, Lighten, Overlay); only the Multiply Glaze preset uses one, and there is no user setting for it |
| Opacity | Exists | `Edit::SetLayerOpacity`. Works on groups and effect layers. No separate "Fill" opacity |
| Lock alpha, lock editing | Exists | `LayerProperties.alpha_locked` (Paint layers only) and `locked`, which children inherit (layers.rs:398). No separate position lock |
| Clipping masks | Exists | `properties.clipped`, `clipping_base` and `clipping_stack_top` (layers.rs:721/732). There is a "New clipping layer" command |
| Raster layer masks | Exists | `LayerMask` layers.rs:418. Actions: Add, Enable, Link/Unlink, Invert, Show mask area, Copy/Paste, Reveal all/Hide all, Reveal/Hide selection, Apply, Delete. Masks can be painted with brush, gradient and fill, and can be transformed. Apply works on Paint layers only; applying a group mask is refused with a message to flatten the group first (art_layers.rs:1312) |
| Vector masks, mask density/feather properties | Absent | |
| Layer effects/styles (shadow, stroke, glow, bevel) | Absent | No code found |
| Adjustment/filter layers | Exists | Inserted with `EffectAction::Insert` (crates/layer-ui/src/effects.rs:740). They can be clipped, masked and blended. There are 41 filters (list below). Users can add their own WGSL filters (example in examples/filters/tent-blur). There is no destructive "apply filter to pixels" |
| Merge Down / Merge Visible / Flatten | Absent | No layer command. The only flatten is the "flattened copy" produced by Convert Color Space (`ColorPreparation::Flatten`, crates/layer-ui/src/document_workflow.rs:67), plus export |
| Duplicate | Exists | `LayerAction::Duplicate/DuplicateSelected` (art_layers.rs:1125) |
| Group/Ungroup, reorder/reparent, drag and drop, rename, delete, visibility, solo, "reference" layers, clear layer | Exists | `LayerAction` enum. "Clear layer" wipes the whole layer and ignores the selection |
| Layer via Copy/Cut, new layer from selection, floating selection | Absent | Inventory "Next" |
| Copy / Cut / Copy Merged | Absent | "Next" |
| Paste | Partial | "Paste Image as Layer" (`CommandId::PasteImage`, Ctrl+V) creates a new layer from the clipboard image, centered, with placement handles. No Paste in Place or Paste Into |
| Smart objects / linked layers | Partial analog | Imported photos keep an immutable full-resolution `source` plus a persistent non-destructive `placement` affine (layers.rs:398), with "Original Size (100%)", "Rasterize Source…" and "Repair Source Profile…". No embedded or linked smart objects and no smart filters (clipped effect layers come closest) |
| Align/Distribute layers | Absent | |

**The 41 filters**, from assets/filters/manifest.json:
- Tone: Curves, Levels, Brightness/Contrast, Exposure, Vignette.
- Color: Hue/Saturation, Color Balance, Vibrance, Black & White, Gradient Map, White Balance, Split Tone, Solarize, Iridescence.
- Detail: Unsharp Mask, High Pass, Denoise, Edge Detect, Emboss.
- Blur: Gaussian, Motion, Bloom, Soft Focus.
- Artistic: Posterize, Halftone, Crosshatch, Pixel Mosaic, Painterly, Pencil.
- Distort: Chromatic Aberration, Kaleidoscope, Swirl, Ripple, Glass, Rainy Glass, Heat Haze, Domain Warp.
- Texture: Film Grain, VHS, CRT.

---

## 3. Transform

| Item | Status | Evidence / notes |
|---|---|---|
| Move tool | Exists | `LayerCanvasTool::Move` and `Document::move_target_edit` (layers.rs:1108). Translates the whole active layer (and a linked mask), or the mask alone. It cannot move only the selected pixels; the Scale/Rotate move handle does that |
| Free transform: scale and rotate | Exists | Shortcut Ctrl+T. `Handle::{Move, Scale, Rotate}` (crates/layer-ui/src/operation.rs:32) with 8 scale handles. Shift keeps proportions or snaps rotation to 15°; Alt scales about the center; "Keep proportions" toggle. Numeric X/Y/angle/width/height (operation.rs:444). Scale range 0.1–10000%; negative scale flips. Works on paint content, on selected pixels (cut and placed within the same layer, selection travels with it, one undo), on layer masks, and on the placement of photo sources (non-destructive). `can_transform` (operation.rs:201) rules out groups, effect layers and transforming several layers at once (except a batch of imports being placed) |
| Skew / Distort / Perspective | Absent | crates/layer-ui/src/operation/placement.rs:67 refuses skewed geometry ("scale/rotate handles cannot edit it yet"). The data model's affine can hold shear, but nothing in the UI creates it. No projective transform |
| Warp, mesh, puppet warp | Absent | |
| Liquify | Partial, brush only | `Tool::Liquify` with presets "Liquify Push" and "Liquify Twirl", plus a Strength setting (crates/layer-ui/src/tool_settings.rs:258). The engine's `LiquifyMode` (lib.rs:489) also has Twirl CCW, Pinch, Expand, Crystals, Edge and Reconstruct, but I found no UI that selects them. No liquify dialog, mesh or freeze mask |
| Canvas Size, Image Size/resample, Crop, Trim, Rotate/Flip canvas, Straighten, Perspective Crop, Content-Aware Scale | Absent | Document size is fixed when the document is created; nothing edits width or height. The View-menu Rotate Left/Right and Flip H/V only change the camera (crates/layer-ui/src/session.rs:4246). "Crop Canvas to Selection" is "Next" |
| Interpolation modes | Partial | `Interpolation` has `Nearest` and `Linear` (default) (crates/layer-core/src/affine.rs:6). The UI never lets you choose Nearest. No bicubic or Lanczos for transforms. The only bicubic is in the CPU resizer used by export's "Fit" size: area reduction plus Catmull–Rom enlargement (crates/layer-color/src/resize.rs; `ExportSize` crates/layer-ui/src/export.rs:99) |
| Image placement | Exists | Open creates a document at the image's size. Import, Paste and drop onto the canvas add a layer with placement handles. Dropping onto Layers shows above/below/into. Initial fit is `min(1, canvas/source)`. Apply stores the affine without downsampling. Includes "Original Size (100%)" and batch import as one undo step. GTK is user-approved (docs/development/image-placement-gtk-progress.md); Web/Android status is in image-placement-web-android-progress.md and the handoff doc |

---

## 4. Painting and retouching tools

**What exists**
- **Brush tools:** `Tool` = Pen, Pencil, Brush, Eraser, Airbrush, Decoration, Blend, Liquify. There are 35 `DefaultBrushPreset`s (crates/layer-core/src/presets.rs:20), covering ink, pencil, pastel, oil, watercolor, bristle and similar.
- **Eraser:** a single preset. Transparent paint color also erases with Figure and Gradient.
- **Smudge and blending:** under Blend there are "Smudge" and "Natural Blender" (`BrushExecution::Smudge/Wet`, lib.rs:425).
- **Fill bucket:** `LayerCanvasTool::Region{fill:true}`, shortcut F, with the same tolerance, gap-closing, expansion, smoothing and sample-source options as the wand. Also "Lasso fill" and "Fill selection" (Shift+Backspace).
- **Gradient tool:** shortcut G. Linear or radial, each as color-to-color or color-to-clear. Two colors only, no multi-stop editor.
- **Figure tool:** line, rectangle or ellipse, drawn as outline, fill or both (crates/layer-core/src/figures.rs:8).
- **Eyedropper:** samples visible color or layer color, sample width 1–101 px.

**What is absent**
- Clone stamp
- Healing brush, spot healing, patch
- Content-aware fill or move
- Dodge, burn, sponge
- Blur and sharpen brushes
- History brush
- Red-eye
- Color replacement

A search for all of these found nothing. The closest workaround is an effect layer (Gaussian Blur, Unsharp Mask, Denoise) with a painted mask.

---

## Other things worth knowing
- **No History panel.** Only undo/redo. The `Panel` enum is at crates/layer-ui/src/layout.rs:615.
- **No text tool, no layered PSD/ORA import or export, no RAW support.**
- **File formats:**
  - Import: JPEG, PNG, TIFF, AVIF, HEIF/HEIC, WebP, GIF, BMP, EXR, HDR PNG, gain maps (crates/layer-color/src/photo/).
  - Export: EXR, PNG, HDR PNG, JPEG, HDR JPEG, AVIF (HDR), TIFF (`ExportFormat` export.rs:9).
- **Strong color management:** assign/convert profile, bit depth U8/U16/Float32 HDR, soft proof, gamut warning, SDR rendition, histogram, and local tone mapping in the renderer.
- **Rulers and guides:** straight, parallel and radial, with snapping (crates/layer-core/src/rulers.rs:7).
- **Planned roadmap is written down.** The "Next" items are in docs/ui/selection-command-inventory.md §1–7 and the "Later" items in §9. Next covers Feather, Border, Smooth, Transform Selection, Copy/Cut/Copy Merged, Copy/Cut to New Layer, Clear Selected/Outside, and Crop to Selection.
- **Groups have no Pass Through**, so adjustment layers inside a group only affect that group.
