# Photo editing: user journeys, gap audit and build list

[Technical documentation](../README.md) · [Design and validation history](README.md)

Research date: **2026-09-25**; canvas action bar revision **2026-09-26**. Source baseline: **`origin/main` at `5eb45a47`** (2026-09-26); citations re-checked at `b7a73e07`.

The research was carried out against `dac76c20`, a bristle-brush branch commit. All `file:line` references in this record were then mapped to `5eb45a47` and use full repository paths. Source reports 1–10 keep their original `dac76c20` line numbers; reports 11–14 were made against `5eb45a47`.

The 2026-09-26 revision adds an interaction audit. The first draft placed most new verbs in menus, submenus, modal dialogs, the docked Tool Options bar or modifier keys. The revision adds a shared **canvas action bar** ([section 5](#canvas-action-bar-bar)): a floating bar beside the selection, transform box or other object being edited, which offers its most common next steps and mode switches. It also makes Transform one session with switchable modes, and moves destructive Distort and Warp ahead of the non-affine placement model.

**Phase 1** (milestone M1) is the canvas action bar plus the improved transforms. Its [implementation plan](../development/canvas-action-bar-transforms.md) lists the steps, tests and remaining decisions.

This record compares Capy Canvas with the photo-editing tasks people most often
learn, ask about and complain about in Photoshop, Affinity Photo, GIMP, Krita,
Photopea, Pixelmator Pro and Procreate. It defines 30 user journeys, grades
each one against the current code, and lists the features and tool changes
needed to support them. Everything under "build list" is proposed work, not an
implemented capability.

The build list was audited item by item against the code, the platform hosts
and the existing design records. The audit found reusable infrastructure,
earlier design decisions and constraints the first draft had missed.
[Section 4](#4-implementation-audit) records those findings, and each build-list
item names what it reuses. Check the cited code before relying on a claim here;
later commits may have changed it.

## Summary

Capy is already strong in several areas that GIMP and Photopea handle poorly:
- **Color:** color-managed 8/16-bit and float HDR documents, soft proofing, HDR delivery.
- **Non-destructive editing:** effect layers that can be clipped, masked and blended. This already answers the most-viewed complaint about GIMP, "adjust only one layer".
- **Selection:** a tonal (luminosity) selection measured in stops, Quick Mask and saved Selection Layers.
- **Reference layers:** already the sampling source for the Wand and Fill tools.
- **Photo placement:** imported photos keep their full resolution under a lossless placement matrix.

Photo editing starts with pixel and geometry work, and that is where Capy falls short:

1. **Document geometry.** There is no crop, straighten, canvas size, image size, or rotate/flip of the image itself.
   - The obstacle is structural: a paint layer's editable area always equals the canvas (`Layer::local_extent`, `crates/layer-core/src/layers.rs:52`).
2. **Moving pixels between layers.** There is no pixel clipboard, Copy/Cut Selection to New Layer, or selection-aware Move.
   - Layer via Copy can be done by hand in three steps: Duplicate, Reveal selection, Apply mask.
   - No platform app ever writes an image to the system clipboard.
3. **Retouching.** There is no clone, heal, spot heal, patch or content-aware fill. Brushes read only the layer they paint on.
4. **Transforms.** Only scale, rotate and move have handles.
   - Skew is already supported by the model and the shader; only the handles are missing.
   - Distort, perspective and warp need a projective or mesh geometry that no part of the code has yet.
   - Liquify exposes 2 of its 7 working modes. Reconstruct is declared but rejected by the renderer.
5. **Layer operations.** There are 7 blend modes, all computed in linear light, so none of them matches Photoshop's behavior on 8/16-bit files. There is no merge, flatten, stamp visible or Blend If.
6. **Adjustment precision.** There are no pickers for black, white or gray points, and Hue/Saturation cannot target a single hue range.
   - Several missing adjustments already exist in other forms. The GPU histogram, local tone mapping and 3D LUT code built for color management can be reused.
   - Gaussian Blur tops out at σ 21, a 63 px kernel.
7. **Delivery.** Export deliberately writes only a freshly generated minimal EXIF, so camera, lens, copyright and date information is lost.
   - Lossless WebP export needs only exposing: an encoder is already vendored.
   - There is no batch processing.
8. **Common next steps are far from the canvas.** Apart from the image-placement bar, every action on a selection, transform or mask lives in a menu, a layer-row submenu, a modal dialog, the docked Tool Options bar or a modifier key.
   - A completed selection publishes no actions at all. The host "Selection Actions…" button appears only while a selection tool is active, and GTK has none.
   - Transform has no flip command and no mode switch. Ctrl+T is reserved by the browser on Web.
   - Removing the last polygon point needs Backspace. Mask editing has no command to enter or leave it.
   - All six hosts already ship a compact placement bar with Original Size, Cancel and Apply. It is fixed at the bottom centre and positioned separately by each host.
   - The [canvas action bar](#canvas-action-bar-bar) generalizes that bar and gives each journey a route from the canvas that works without a keyboard.

**The three features named in the request:**

| Feature | Status |
| --- | --- |
| Clone | Absent. [RET-2](#ret-2-clone-stamp-p0) samples the **reference layers** ([RET-1](#ret-1-retouch-source-reference-layers-p0)). It needs three shared additions: a copy of each tile taken just before the stroke first changes it, pen barrel-button bindings, and a Retouch tool family. |
| Mesh transform | Absent. It becomes the Warp mode of one transform session, switched from the action bar ([BAR-2](#bar-2-transform-session-and-modes-p0)). Paint layers, masks and selected pixels already commit transforms by resampling pixels, so destructive Warp ([XF-3](#xf-3-warp-and-mesh-transform-p1-requested)) needs a new mesh pass but not the non-affine placement model. That model (P-10) is needed only to keep warped photo placements lossless. Skew ([XF-1a](#xf-1a-skew-handles-p0)) is nearly free. |
| Create layer from selection | Absent as a command. It is already specified as "Copy/Cut Selection to New Layer" in the [selection inventory](../ui/selection-command-inventory.md). It can be built from Duplicate plus a selection-aware clear, with no pixel readback ([SEL-2](#sel-2-copy-and-cut-selection-to-new-layer-p0)). It is a primary action on the selection bar ([BAR-1](#bar-1-selection-p0)). |

**Scorecard:** of the 30 journeys, **3 are supported**, **16 work with friction** and **11 are blocked**. The [recommended sequence](#7-recommended-sequencing) starts with the open decisions, then the canvas action bar and a batch of quick wins. It opens 8 of the 11 blocked journeys by the end of milestone M5.

## Method and evidence

**Code inventory.** Two code audits read the enums that define the command surface:
- `CommandId` (`crates/layer-ui/src/lib.rs:526`)
- `Tool` (`crates/layer-ui/src/tools.rs:8`)
- `LayerAction` (`crates/layer-ui/src/art_layers.rs:117`)
- `SelectionTool` (`crates/layer-ui/src/selection_tools.rs:9`)
- `SelectionAction` (`crates/layer-ui/src/selection_masks.rs:45`)
- `LayerKind` (`crates/layer-core/src/lib.rs:162`)
- `Edit` (`crates/layer-core/src/lib.rs:1642`)
- `LayerBlend` (`crates/layer-core/src/layers.rs:364`)
- `Panel` (`crates/layer-ui/src/layout.rs:615`)
- the filter manifest (`assets/filters/manifest.json`)

**Implementation audit.** Six further audits covered geometry and transform, selection and clipboard, retouching, layers, adjustments, and view and delivery. They checked every build-list item against:
- the shared Rust crates;
- all five platform apps;
- the design records in `docs/ui`, `docs/development`, `docs/history` and `docs/reference`.

**Interaction audit (2026-09-26).** Four further audits asked where each flow's next step appears, not only whether the feature exists:
- every existing canvas state, tool session and on-canvas object in the shared crates, with its anchor geometry and its current action surfaces;
- how each host can layer a non-modal bar over the canvas;
- the design records that anticipate or constrain contextual canvas actions;
- how other editors place and fill their contextual canvas bars.

Every build-list item was then checked for the canvas contexts it creates or changes ([section 5](#canvas-action-bar-bar)).

**Industry evidence** comes from four kinds of source:
- vendor documentation;
- community forums and issue trackers (community.adobe.com, the Affinity forum, discuss.pixls.us, gimp-forum.net, krita-artists, GIMP GitLab, Photopea GitHub);
- Stack Exchange view counts: the top 500 `gimp` questions on graphicdesign.SE and superuser, and Photography.SE's `gimp`, `photoshop` and `retouching` tags;
- tutorial authors: PiXimperfect, PHLEARN, Photoshop Training Channel, Julieanne Kost, James Ritson/Affinity.

Reddit, photopea.com/learn and some Adobe help pages refused automated access, so none are cited. View counts measure search demand, not usage. The rankings below are product judgment.

**Source reports.** The full reports, with every source URL and `file:line` detail, are in [`photo-editing-research/`](photo-editing-research/). They are dated agent output. Reports 1–10 use baseline `dac76c20`, and those audits refer to the superseded first draft of this plan. Reports 11–17 use `5eb45a47`; 15–17 prepare [Phase 1](../development/canvas-action-bar-transforms.md).

| # | Report | File |
| --- | --- | --- |
| 1 | Photoshop and Affinity Photo journeys and power-user details | [01-photoshop-affinity-journeys.md](photo-editing-research/01-photoshop-affinity-journeys.md) |
| 2 | GIMP, Krita, Photopea, Pixelmator Pro and Procreate journeys, friction and tool matrices | [02-gimp-krita-photopea-journeys.md](photo-editing-research/02-gimp-krita-photopea-journeys.md) |
| 3 | Code inventory: selection, layers, transform, retouching | [03-inventory-selection-layers-transform.md](photo-editing-research/03-inventory-selection-layers-transform.md) |
| 4 | Code inventory: adjustments, color, files, view, history | [04-inventory-adjustments-color-files.md](photo-editing-research/04-inventory-adjustments-color-files.md) |
| 5 | Audit: geometry and transform | [05-audit-geometry-transform.md](photo-editing-research/05-audit-geometry-transform.md) |
| 6 | Audit: selection and clipboard | [06-audit-selection-clipboard.md](photo-editing-research/06-audit-selection-clipboard.md) |
| 7 | Audit: retouching | [07-audit-retouching.md](photo-editing-research/07-audit-retouching.md) |
| 8 | Audit: layers and compositing | [08-audit-layers.md](photo-editing-research/08-audit-layers.md) |
| 9 | Audit: adjustments and filters | [09-audit-adjustments-filters.md](photo-editing-research/09-audit-adjustments-filters.md) |
| 10 | Audit: view, files and delivery | [10-audit-view-files.md](photo-editing-research/10-audit-view-files.md) |
| 11 | Audit: existing canvas states and the canvas action bar | [11-audit-canvas-states.md](photo-editing-research/11-audit-canvas-states.md) |
| 12 | Audit: host support for a floating canvas bar | [12-audit-host-overlays.md](photo-editing-research/12-audit-host-overlays.md) |
| 13 | Audit: design records that shape the canvas action bar | [13-audit-design-records.md](photo-editing-research/13-audit-design-records.md) |
| 14 | Research: contextual canvas bars in other editors | [14-contextual-bars-in-other-editors.md](photo-editing-research/14-contextual-bars-in-other-editors.md) |
| 15 | Phase 1 audit: canvas action bar in shared Rust | [15-phase1-bar-shared-rust.md](photo-editing-research/15-phase1-bar-shared-rust.md) |
| 16 | Phase 1 audit: canvas action bar on each host | [16-phase1-bar-hosts.md](photo-editing-research/16-phase1-bar-hosts.md) |
| 17 | Phase 1 audit: transform session, distort and warp | [17-phase1-transforms.md](photo-editing-research/17-phase1-transforms.md) |

**Out of scope for this study:** RAW development, generative AI fill, CMYK/Lab/Grayscale modes, layered PSD, and text or annotation. See [section 9](#9-out-of-scope-and-deferred).

## 1. Current capability inventory

**Documents and color**
- **Present:**
  - Four working spaces: sRGB, Display P3, Adobe RGB and ProPhoto.
  - U8, U16, F16 or F32 depth.
  - Assign Profile, Convert Color Space, Change Bit Depth.
  - Source Color Profile and Rasterize Source for retained originals, which keep their embedded ICC.
  - Soft proof with gamut warning.
  - SDR rendition with local tone mapping.
  - "Photo editing" (ProPhoto 16-bit) and "HDR drawing" presets (`crates/layer-ui/src/document_creation.rs:93`).
  - A shared Document Properties description (`crates/layer-color/src/document_info.rs:12`).
- **Missing or partial:**
  - No arbitrary ICC working space.
  - No Gray, CMYK or Lab document mode (out of scope).

**Geometry**
- **Present:**
  - Zoom from 2% to 1600%; rotate and flip of the view only (`crates/layer-ui/src/camera.rs:43`).
  - Export at Original size or Fit, with optional enlargement using Catmull-Rom interpolation (`crates/layer-ui/src/export.rs:99`).
- **Missing:**
  - Crop, straighten, canvas size, image size, trim, rotate/flip of the image, perspective crop.
  - A paint layer's editable area is always the canvas, which blocks all of these (`crates/layer-core/src/layers.rs:52`).

**Layers**
- **Present:**
  - Kinds: Paint, Group, Effect, Selection and Background. `ImportedImage` and `AiSuggestion` are vestigial but still appear in validation checks.
  - Opacity, alpha lock, lock and clipping.
  - Raster masks: enable, link, invert, Show mask area tint, reveal/hide all, apply, copy/paste, from selection.
  - Duplicate, group (groups can be moved), solo, reference layers.
  - Paint layers and photos have a lossless affine `placement` (`crates/layer-core/src/layers.rs:401`).
  - Blend modes also apply to effect layers.
- **Missing or partial:**
  - 7 blend modes, in linear light. Add and Color clamp HDR values.
  - Groups are always isolated; this was a deliberate decision.
  - No merge, flatten or stamp. No Blend If.
  - No mask density or feather. No fill layers, although the renderer supports generators.
  - No layer styles. No align or distribute.

**Selection**
- **Present:**
  - Rectangle, ellipse, lasso, polygon, wand, select by color, painted selection.
  - Tonal selection in EV bands.
  - New/Add/Subtract/Intersect modes, anti-alias.
  - GPU feather at creation (up to 100 px), Grow/Shrink.
  - Invert, Reselect, Quick Mask.
  - Saved Selection Layers; selection from layer alpha or from a mask.
- **Missing, although the GPU operations largely exist:**
  - Feather, Border and Smooth of an existing selection; Transform Selection.
  - Refine Edge, edge-aware quick select, subject select, channel selection, Select Similar. The inventory marks these "Later".

**Clipboard**
- **Present:**
  - Paste Image as Layer, with placement handles; the source is kept.
  - Copy/Replace mask through an in-memory clipboard for each document.
- **Missing:**
  - Copy, Cut, Copy Merged, Paste in Place, Copy/Cut Selection to New Layer, Clear Selected, Clear Outside.
  - No app writes images to the system clipboard.

**Transform**
- **Present:**
  - Scale/Rotate (Ctrl+T; the browser reserves this shortcut on Web) on:
    - paint;
    - selected pixels, cut and placed within the layer as one undo step;
    - masks;
    - photo placement.
  - Numeric X/Y, W/H in percent, and angle. Typing a negative percentage flips.
  - Apply/Cancel on Enter and Escape, also shown as tool-option buttons on every host.
  - Import batches transform together. The Move tool moves layers and groups.
- **Missing:**
  - Skew handles; the model and shader already support skew.
  - Distort, perspective, warp, puppet.
  - Flip, rotate-by-step and reset commands during a transform. Flipping needs a negative number or a handle dragged past the opposite edge.
  - Interpolation is Nearest or Linear, and the UI never offers Nearest.
  - Finger touch reaches handles only while placing a photo. Outside placement, a finger only navigates (`crates/layer-ui/src/session.rs:1138`).
  - Groups and multiple layers cannot be scaled or rotated. The pivot is fixed. No arrow-key nudge.

**Contextual actions** ([source report 11](photo-editing-research/11-audit-canvas-states.md))
- **Present:**
  - A compact **image-placement bar** with Original Size, Cancel and Apply on all six hosts. It stays visible when Tool Options or panels are hidden and ignores Zen. Each host positions it separately at the bottom centre; it is not anchored to the photo.
  - Transform and polygon completion actions (Apply/Cancel, Complete/Cancel) lead the docked Tool Options form (`crates/layer-ui/src/toolbar_components.rs:249`).
  - A "Selection Actions…" button on Web, Android, Apple and Windows opens the shared selection menu (`crates/layer-ui/src/selection_masks.rs:226`). It appears only while a selection tool is active and the form is not compact.
- **Missing:**
  - Any action surface attached to a completed selection. The selection persists under every tool but publishes no actions; masking, Selection Layer and Pixel Selection actions live in layer-row submenus.
  - A GTK "Selection Actions…" button.
  - Commands to enter or leave artwork-mask editing, to remove the last polygon point, and to fix a missing reference layer. These are reachable only through a row menu, Backspace or the Layer Settings submenu.
  - Live preview for Grow and Shrink, which use modal host dialogs.

**Adjustments**
- **Present:**
  - One catalog of 40 filters, all of kind `adjustment`. The tonal ones: Curves (master + R/G/B, Log HDR domain), Levels (composite), Brightness/Contrast, Hue/Saturation (master), Color Balance, Exposure, Vibrance, Black & White (tint = colorize), Gradient Map, Posterize, White Balance, Split Tone, Solarize.
  - A non-modal Histogram window with RGB/R/G/B/luminance, log scale, HDR axis and clipping counts.
  - Live preview; one undo step per slider gesture; GPU preview thumbnails.
- **Missing:**
  - Black/white/gray-point pickers. A "Use selected color" button exists but is wired only to paper and mask colors.
  - Per-hue Hue/Saturation, per-channel and auto Levels.
  - Channel Mixer, Selective Color, LUT, Shadows/Highlights, Clarity, Dehaze, Match Color.
  - User presets and copying across documents.
  - New effect layers ignore the active selection.

**Filters**
- **Present:**
  - Gaussian Blur (σ ≤ 21, kernel ≤ 63 px), Motion Blur, Bloom, Soft Focus.
  - Unsharp Mask, High Pass, Denoise (bilateral, radius 1–3).
  - Vignette (in percent), Film Grain, Chromatic Aberration (a uniform shift).
  - Artistic and distort filters.
  - Runtime WGSL filter packages ([runtime filters](../reference/runtime-filters.md)).
- **Missing:**
  - Large-radius blur, lens or tilt-shift blur, Surface Blur, Median, Dust & Scratches.
  - Real noise reduction, Smart Sharpen.
  - Lens corrections, frequency separation.

**Retouch and paint**
- **Present:**
  - 34 brush presets, including Smudge and Natural Blender (both run the Smudge execution). The bristle paintbrush, preset 35, is on a separate branch.
  - 8 brush blend modes on the GPU, with no UI.
  - Liquify presets Push and Twirl, plus an engine that also implements Twirl CCW, Pinch, Expand, Crystals and Edge. Liquify already respects the selection, which serves as a freeze mask.
  - Fill and Lasso Fill with a Visible/Editing/Reference source.
  - A two-color Gradient tool.
  - Figure: line, rectangle and ellipse.
  - Eyedropper sampling 1–101 px from the composite or one layer.
- **Missing:**
  - Clone, heal, spot heal, patch, content-aware fill.
  - Dodge, burn, sponge; sharpen brush.
  - History brush, red-eye.
  - Liquify Reconstruct.
  - Brushes cannot read other layers.

**View**
- **Present:**
  - Navigator.
  - A zoom and rotation readout on every host (`CanvasInfoLayout`, `crates/layer-ui/src/header.rs:449`).
  - Continuous zoom by wheel and pinch; view rotation and flip.
  - Drawing guides (straight, parallel, radial): durable, undoable and editable with the Move tool.
- **Missing:**
  - Actual Pixels (100%) and typed zoom entry.
  - Split before/after view.
  - Info panel and color samplers.
  - Horizontal/vertical guides, grid, edge rulers.
  - A document History panel.

**Files**
- **Present:**
  - **Import:** JPEG (including gain maps and CMYK), PNG, TIFF, WebP, GIF, BMP, HEIC, AVIF, EXR. EXIF orientation is applied. Importing several files adds one layer each, as one undo step.
  - **Export:** PNG, TIFF, JPEG and HDR formats with ICC, CMYK/Gray delivery, depth, quality, matte and PPI. Destination presets remember the last recipe.
  - **Metadata policy:** export writes a newly generated EXIF (Orientation 1 plus density). Stale camera EXIF is never copied (`crates/layer-color/src/photo/metadata.rs:1`).
  - **File associations** on Linux, Android, Web and macOS.
- **Missing:**
  - Camera, copyright and date metadata are not kept; IPTC is not parsed.
  - No WebP export (a lossless encoder is vendored), SDR AVIF or HEIC export.
  - Export size offers no long edge, percent or megapixels.
  - No per-layer or selection export, no batch, no folder picker on any host.
  - No RAW, no PSD.
  - No file associations on Windows.

The Photo workspace deliberately shows no placeholders for missing tools
([default workspaces](../ui/default-workspaces.md)). Add each feature to it in the same change that ships the feature.

## 2. The 30 journeys

**Status meanings**
- **Supported:** a Photoshop or Affinity user can finish the task to the same quality.
- **Friction:** the task can be finished only through a workaround, at lower quality, or without a control that forums treat as standard.
- **Blocked:** a required feature is absent.

**Tier** comes from the research ranking. Tier 1 tasks appear in beginner search demand, in professional tutorials and in several applications' forums. Tier 2 tasks are common but narrower.

**Needs** lists IDs from the [build list](#5-build-list-new-features) and the [tweaks](#6-tweaks-to-existing-tools).

**Route from the canvas** (added 2026-09-26). The statuses grade whether the features exist, not how many steps a journey takes. A journey also needs a route from the object being edited: on a tablet without a keyboard, each Tier 1 journey must be finishable from the canvas without the application menus, which stay the complete home for every command. `BAR` IDs in Needs mark journeys whose route depends on the [canvas action bar](#canvas-action-bar-bar); its [target routes](#target-routes) list the steps.

### A. Framing and geometry

| # | Journey | Industry workflow | Capy today | Status | Tier | Needs |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | **Crop** to a composition or ratio; crop one layer; crop to content | Crop tool with ratio presets, thirds overlay, "Delete cropped pixels"; Trim; Crop to Selection. PH "crop a single layer" 422k views. | No crop. A single layer can be masked by an inverted rectangle selection. | Blocked | 1 | GEO-0, GEO-1, GEO-3, SEL-4, BAR-1, BAR-4 |
| 2 | **Straighten** a tilted horizon | Crop tool straighten line; GIMP Measure then Straighten. | A layer rotates by an exact angle, but corners cannot be cropped away. | Blocked | 1 | GEO-0, GEO-2, XF-2, BAR-4 |
| 3 | **Correct perspective** and converging verticals | Transform Perspective/Distort, Upright, Perspective Warp, Perspective Crop. PS "change perspective" 272k. | No projective transform. | Blocked | 1 | XF-1b, XF-2, GEO-1, BAR-2 |
| 4 | **Resize, resample or extend** the canvas | Image Size with resampling method and resolution; Canvas Size with anchor. PS "increase size without losing quality" 213k. | Export can fit a box or enlarge a copy; the document size is fixed. | Blocked | 1 | GEO-0, GEO-3, GEO-4 |

### B. Global tone and color

| # | Journey | Industry workflow | Capy today | Status | Tier | Needs |
| --- | --- | --- | --- | --- | --- | --- |
| 5 | **Global tone** with Levels and Curves | Histogram inside the editor, black/white/gray pickers, clipping preview, per-channel Levels, Auto. | Curves and Levels effect layers; a separate histogram window with clipping counts. No pickers, no per-channel Levels, no Auto. | Friction | 1 | ADJ-1, ADJ-3, ADJ-4, VIEW-3, BAR-5 |
| 6 | **Neutralize a color cast** | Gray-point picker, white-balance picker, Match Color Neutralize. | White Balance and Color Balance sliders only. | Friction | 1 | ADJ-1, BAR-5 |
| 7 | **Recover shadows and highlights**, local contrast, haze | Shadows/Highlights, Clarity/Texture, Dehaze. | Curves plus tonal masks. The local tone code exists only as the document-wide SDR rendition. | Friction | 1 | ADJ-6 |
| 8 | **Color-grade**: black and white, split tone, LUT, selective color | Black & White, Gradient Map, Split Toning, Color Lookup, Selective Color, Channel Mixer. | B&W with hue sliders and tint, Split Tone, Gradient Map, Color Balance, Curves. No LUT, Selective Color, Channel Mixer or presets. | Friction | 1 | ADJ-5, ADJ-10 |
| 9 | **Consistent look across many photos** | Presets, copy/paste settings, Match Color, LUTs, Actions + Batch. SU "batch convert" 176k. | No user presets, no cross-document copy, no batch. | Blocked | 2 | ADJ-10, ADJ-12, IO-4 |
| 10 | **Sharpen detail and reduce noise** | Smart Sharpen, High Pass on Overlay, noise reduction, judged at 100%. | Unsharp Mask and High Pass effect layers. No 100% command; Denoise radius is 1–3. | Friction | 1 | VIEW-1, ADJ-8 |

### C. Local adjustments, selections and masks

| # | Journey | Industry workflow | Capy today | Status | Tier | Needs |
| --- | --- | --- | --- | --- | --- | --- |
| 11 | **Adjust one layer or region** non-destructively | Clipped adjustment; an adjustment created with a selection takes it as its mask; feather or paint the mask. PS "adjustment layer affect only one layer" 406k + 394k. | Clipping works. Masking to a selection is a second step. No feather after creation and no mask density. | Friction | 1 | T-1, SEL-5, LYR-4, BAR-1 |
| 12 | **Luminosity-mask adjustments** | TK/Raya panels, Select Tonal Range, Blend If. | Tonal selection, then Mask Selection or a saved Selection Layer. | Supported | 2 | LYR-3 (live variant) |
| 13 | **Recolor an object** keeping texture | Hue/Saturation per hue range, Colorize, Color blend mode. SU "change one colour to another" 304k. | Hue/Saturation is master-only. Colorize is available only through B&W Tint. Color blend and brush-selection masks work. | Friction | 1 | ADJ-2, BAR-1 |
| 14 | **Cut out a subject**: transparent or replaced background | Select Subject, Select and Mask, output to mask; Color to Alpha; Delete then PNG. GD "make background transparent" 1.64M. | Wand, color, lasso and brush selections; Hide Selection mask; Eraser within the selection; PNG keeps alpha. No refine, subject select, Color to Alpha, or Delete binding. | Friction | 1 | SEL-4, SEL-6, SEL-7, SEL-8, SEL-9, BAR-1 |
| 15 | **Mask hair, fur and foliage** | Refine Hair, channel masking, Calculations. | No edge refinement or channel selection. | Blocked | 1 | SEL-6, SEL-8, LYR-3, BAR-1 |
| 16 | **Replace a sky** | Sky Replacement, or Select Sky then harmonize. | Place a sky, mask it with a tonal Highlights selection, then clipped adjustments. | Friction | 2 | LYR-3, SEL-6, ADJ-12 |
| 17 | **Composite a subject into a scene** | Clipped Curves, Match Color/Harmonize, Luminosity/Hue blends, Blend If, shadows. | Placement, masks, clipped effects, Color blend. | Friction | 1 | LYR-1, LYR-3, ADJ-12, XF-1a, BAR-2, BAR-3 |
| 18 | **Fade or blend two photos** with a soft gradient | Layer mask plus gradient. GD "make image partially transparent" 174k. | Layer masks plus the Gradient tool on the mask. | Supported | 1 | — |
| 19 | **Blur the background** | Lens or Field Blur through a mask. | Gaussian effect with an inverted mask; σ ≤ 21 (kernel ≤ 63 px) is too small for large photos. | Friction | 2 | ADJ-7, T-3, BAR-1 |

### D. Retouching

| # | Journey | Industry workflow | Capy today | Status | Tier | Needs |
| --- | --- | --- | --- | --- | --- | --- |
| 20 | **Remove dust, blemishes and scratches** (including restoration) | Spot Healing and Healing on an empty layer sampling below; Dust & Scratches. | Nothing suitable. | Blocked | 1 | RET-1, RET-3, RET-4, ADJ-8 |
| 21 | **Remove an object, person or wire** | Remove tool, Content-Aware Fill, Clone. A Lightroom request for Photoshop-like heal drew 237 replies. | Nothing suitable. | Blocked | 1 | RET-2, RET-4, RET-5, BAR-1, BAR-7 |
| 22 | **Clone structured detail** | Clone Stamp with Aligned, source scope, overlay, rotate/scale/flip source. | Nothing suitable. | Blocked | 1 | RET-1, RET-2, BAR-6 |
| 23 | **Frequency separation** | Blurred low layer; high layer via Apply Image in Linear Light; heal on the high layer. | Needs editable pixels in Linear Light in the encoded domain, baking, and heal. | Blocked | 1 | LYR-1, LYR-2, ADJ-9, RET-3 |
| 24 | **Dodge and burn** | Gray layer in Soft Light/Overlay, or paired Curves layers, painted with low flow. | Works with an Overlay or Soft Light layer, but "neutral" there is linear 0.5 (about sRGB 188), and the response differs from Photoshop. Curves layers with masks also work. | Friction | 1 | RET-7, LYR-1 |
| 25 | **Reshape with Liquify** | Freeze/Thaw, Pin Edges, Reconstruct, re-editable. | Push and Twirl presets; the selection acts as a freeze mask. No Reconstruct; edits are destructive. | Friction | 1 | XF-5, T-4 |

### E. Rearranging and multi-image work

| # | Journey | Industry workflow | Capy today | Status | Tier | Needs |
| --- | --- | --- | --- | --- | --- | --- |
| 26 | **Move, duplicate or extract part of an image** | Ctrl+J/Ctrl+Shift+J, Copy/Copy Merged/Paste in Place, Move with a selection. SU "move things with selection in GIMP" 294k; PS "copy layers between documents" 1.01M. | Three steps: Duplicate, Reveal selection, Apply mask. Scale/Rotate moves selected pixels within a layer. Nothing crosses documents. | Friction | 1 | SEL-1, SEL-2, SEL-3, BAR-1 |
| 27 | **Warp an element onto a surface** | Warp/mesh on a smart object, Puppet Warp. GIMP issue #8017 has been open since 2022. | No distort or warp. | Blocked | 2 | XF-1b, XF-3, BAR-2 |
| 28 | **Combine several frames** | Load into a stack, Auto-Align, luminosity blending, Median, Photomerge. | Multi-file import with tonal masks, for frames shot on a tripod. No Difference blend, alignment, stacks or merges. | Friction | 2 | LYR-1, LYR-6, LYR-7, LYR-9 |

### F. Output

| # | Journey | Industry workflow | Capy today | Status | Tier | Needs |
| --- | --- | --- | --- | --- | --- | --- |
| 29 | **Deliver for web and social** | Export with sRGB, long edge, metadata choices, WebP/AVIF. | Presets, sRGB with embedded ICC, Original/Fit size. Camera and copyright EXIF are not kept. No WebP. | Friction | 1 | IO-1, IO-2, IO-3 |
| 30 | **Prepare a print** | Soft proof, gamut warning, output sharpening, profiled TIFF. | Soft proof, gamut warning, CMYK/RGB ICC output, PPI. GTK shows print size. | Supported | 2 | VIEW-1, GEO-4 |

## 3. Cross-application lessons

- **No floating state.**
  - GIMP's most-viewed complaints (294k and 89k views) concern pixels that float until they are anchored. Krita users have the opposite surprise.
  - Give each verb its own name:
    - Paste (as a new layer);
    - Paste in Place;
    - Copy/Cut Selection to New Layer;
    - Paste Into (Later in the selection inventory).
  - Each verb is one undo step, and none leaves a pending state.
- **Put the next step next to the object** ([source report 14](photo-editing-research/14-contextual-bars-in-other-editors.md)).
  - Where the bar sits:
    - Anchored to the object: Clip Studio Paint's [Selection Launcher](https://help.clip-studio.com/en-us/manual_en/330_selection/Selection_Launcher.htm) sits under the selection, Krita 5.3's [Selection Action Bar](https://docs.krita.org/en/user_manual/selections.html) under a new selection, and Photoshop's [Contextual Task Bar](https://helpx.adobe.com/photoshop/using/contextual-task-bar.html) under the selection or transform.
    - Fixed at the bottom of the screen on tablets: Procreate, Photoshop on iPad, Pixelmator Pro on iPad and ibisPaint.
  - Actions that most products offer:
    - **On a selection:** Deselect, Invert, Fill, Clear, Copy to New Layer, Transform, Refine/Feather, Mask and Crop to Selection.
    - **During a transform:** Commit/Cancel, Flip and rotate by a step.
    - **Transform modes:** the touch-first apps (Procreate's [transform toolbar](https://help.procreate.com/procreate/handbook/transform/transform-interface-gestures), Infinite Painter) put the switch between Free, Uniform, Distort and Warp on the bar; desktop apps keep it in a panel.
  - The recurring complaints:
    - bars that cover the work;
    - bars that jump or flicker;
    - bars that forget their position;
    - bars that appear unrequested with no obvious way to hide them ([Photoshop pinning](https://community.adobe.com/t5/photoshop-ecosystem-discussions/contextual-task-bar-pinned-position/m-p/14053297), [Krita feedback](https://krita-artists.org/t/feedback-for-the-new-selection-action-bar-in-krita-5-3/141290)).
  - Commit and Cancel must survive hiding the bar: [Photoshop Elements](https://helpx.adobe.com/photoshop-elements/using/contextual-task-bar.html) keeps it during Crop, Place and Transform.
  - This is not floating state: the bar acts on committed state, and each of its actions is one undo step.
- **Explain silent failures.**
  - GIMP's top questions come from tools that do nothing because of hidden state.
  - Capy already reports many refusals: GTK shows a label, Web a 7-second status, Windows a status line and Android a dialog. Apple does not show errors raised during a gesture.
  - Several operations still return without any message ([T-15](#6-tweaks-to-existing-tools)).
- **Retouching on an empty layer needs a source.**
  - "Clone does nothing on a new layer" is Photoshop's top clone question.
  - In Capy the source is the existing reference layers, as for Wand and Fill ([RET-1](#ret-1-retouch-source-reference-layers-p0)).
  - Setting the source must not depend on a modifier key, because tablets have none. Krita "clone on a tablet" has 19k views.
- **Algorithm quality is the moat.**
  - Photopea's maintainer stalled publicly on content-aware fill and puppet warp.
  - GIMP Heal "looks like cloning" (issue #16124) and runs on a single CPU core.
- **Each adjustment needs its own mask.**
  - GIMP 3 shares one mask per layer across its filters, which its developers call "one of the big regrets".
  - Keep Capy's model of one effect per layer.
- **Modifier semantics are sacred.**
  - Photoshop's 2019 change to proportional scaling forced it to add a legacy preference.
  - Capy tracks Shift and Alt live during a drag, but not Ctrl/Cmd (`crates/layer-ui/src/session.rs:947`).
  - Make every mode a sticky choice on the action bar and in Tool Options, and use modifiers only as temporary overrides. New modes then need no new modifier meanings, and pen and touch users reach them.
- **Keep transforms re-editable.**
  - Capy's `placement` affine already covers both photos and paint pixels.
  - Extend it rather than add a second model.

## 4. Implementation audit

### 4.1 Existing infrastructure to reuse

| Need | Existing code to build on | Used by |
| --- | --- | --- |
| **Sampling other layers** | Reference layers: `Document.reference_layers` (`crates/layer-core/src/lib.rs:1295`), undoable `Edit::SetReferences`, `Document::reference_snapshot` (`crates/layer-core/src/layers.rs:635`), the lighthouse button, `RegionSource::{Visible,Editing,Reference}` (`crates/layer-ui/src/art_layers.rs:39`). The bounded composite query is `artwork::Capture::region` (`crates/layer-render-wgpu/src/artwork.rs:88`). | RET-1…5, ADJ-1, ADJ-3, ADJ-12, SEL-1, T-22 |
| **Whole-document rewrite** | Convert Color Space / Change Bit Depth: the worker `layer_color::prepare_document_color` (`crates/layer-color/src/document.rs`) plus the `prepare_color_transition` / `commit_color_transition` history (`crates/layer-core/src/color_history.rs:34`). The CPU `RowResampler` provides area reduction and Catmull-Rom (`crates/layer-color/src/resize.rs`). | GEO-4, GEO-5, LYR-2 Flatten |
| **Composite snapshot** | `SnapshotRenderer::flattened_document` and `layer_color::flattened_document` (`crates/layer-color/src/flatten.rs:13`), called on every host. PNG output via `write_png(OutputEncoding)` (`crates/layer-render-wgpu/src/snapshot/output.rs:153`). | SEL-1 Copy Merged, LYR-2 |
| **Undoable GPU operations** | `LayerOperationKind` + `Scene::apply_operation` (`crates/layer-render-wgpu/src/scene.rs:1274`), which already settles watercolor and wetness for Apply Mask. `CanvasEngine::append_operations` batches several layers into one undo step (`crates/layer-engine/src/canvas.rs:534`). | LYR-2, GEO-2, GEO-5, SEL-4 |
| **Selection coverage as a mask** | `paint_operation` turns soft or inverted selections into a layer-local mask (`crates/layer-ui/src/art_layers.rs:1959`). `LayerAction::AddMask` maps selection → mask (`crates/layer-ui/src/art_layers.rs:1271`). `LayerAction::Duplicate` shares raster, source and mask through `Arc` (`crates/layer-ui/src/art_layers.rs:1127`). | SEL-2, SEL-4, T-1 |
| **Selected-pixel move** | `begin_transform` with a selection (`crates/layer-ui/src/operation.rs:216`) and `commit_transform` (`crates/layer-engine/src/canvas.rs:473`) cut, place and move the selection. | SEL-3, RET-6 |
| **Selection refinement** | `SelectionRefinement { resize, feather }` on the GPU (`crates/layer-render/src/lib.rs:400`). Grow/Shrink dialogs through `SelectionResizeView` on all six hosts (`crates/layer-ui/src/selection_masks.rs:93`). `Selection::transformed` changes only metadata (`crates/layer-core/src/selection.rs:314`). The tonal draft/refine amend pattern (`crates/layer-ui/src/tonal_selection.rs:232`). | SEL-5, SEL-6, T-10 |
| **Full affine placement** | `LayerProperties.placement` covers photo and paint pixels (`crates/layer-core/src/layers.rs:401`). `pixel_transform.wgsl` uses the full 2×2 linear part. Skewed placements already pass snapshot tests. | XF-1a, XF-6, GEO-2 |
| **Blend math** | A single `blend()` switch (`crates/layer-render-wgpu/src/scene.wgsl:73`) serves layer, effect, fused and cached composites. The brush `blend_color` (`crates/layer-render-wgpu/src/material_brush.wgsl:191`) duplicates it inconsistently. | LYR-1 |
| **Tone and statistics on the GPU** | Tonal histogram bins with integer atomics (`crates/layer-render-wgpu/src/tonal.wgsl:45`). `TonalProbe`, which samples the canvas into a parameter (`crates/layer-render/src/lib.rs:372`). `TonalBand` threshold/falloff math (`crates/layer-core/src/tonal.rs:10`). | ADJ-1, ADJ-3, ADJ-4, LYR-3 |
| **Pyramids and local tone** | Local-Laplacian SDR tone guide (`crates/layer-core/src/color/hdr/local.rs`, `local_tone.rs`/`.wgsl`: downsample, expand, accumulate). The dual-filter blur in `backdrop_blur.rs`. | ADJ-6, ADJ-7, RET-3 |
| **3D LUT** | `ProofLut` (65³/129³) with tetrahedral sampling (`crates/layer-color/src/icc/proof/view_lut.rs`, `crates/layer-render-wgpu/src/proof_view.wgsl:20`). The content-addressed `profile_library.rs` pattern. | ADJ-5 Color Lookup |
| **Effect parameters from the canvas** | `EffectAction::UseCurrentColor` / `color_action`, drawn by all five hosts (`crates/layer-ui/src/effects.rs:178`). The color-picker session lifecycle (`color_picker_session.rs`). | ADJ-1 |
| **Multi-stop gradients** | `GradientStop` (2–32 stops), `EffectValue::Gradient`, and stop editors on the hosts (`crates/layer-core/src/effects.rs:233`). | LYR-5, T-14 |
| **Generators** | `EffectKind::Generator` is supported through renderer, tests and serde (`crates/layer-render-wgpu/src/scene.rs:850`). No generator is shipped. | LYR-5 |
| **Guides** | `Document.rulers`: durable, undoable, up to 1,024, stored in `.capy`, drawn with `CursorSegment`, editable with Move (`crates/layer-core/src/rulers.rs`). | VIEW-4, GEO-2 |
| **Compare previews** | GTK `Comparison` Before/After (`apps/layer-linux/src/files/preview.rs:140`). The proof and SDR transforms applied at presentation. | VIEW-2 |
| **Zoom readout** | `CanvasInfoLayout` "N% · D°" on every host. `NumericControl` (`crates/layer-ui/src/numeric.rs`). | VIEW-1 |
| **History list** | The workspace history presentation, preview and restore (`crates/layer-workspace/src/history_presentation.rs`). | VIEW-5 |
| **Photo originals** | Placed photos keep their original immutably (`crates/layer-core/src/lib.rs:184`). | RET-9 "revert to original" |
| **Export** | `ExportPresets::remember`; the last folder; CMYK/Gray ICC. A vendored lossless WebP encoder (image-webp). File associations. Multi-file import. | IO-2, IO-3, IO-6 |
| **A bar over the canvas** | The image-placement bar on every host: GTK `PlacementActions` (`apps/layer-linux/src/tool_panels.rs:16`), Web `image-placement-controls` (`apps/layer-web/image-import.js:6`), Android `Popup` (`apps/layer-android/app/src/main/java/art/capycanvas/ImageImport.kt:196`), Apple `PhotoPlacementControls` (`apps/layer-apple/Shared/Editor/EditorView.swift:171`), Windows `placementBar` (`apps/layer-windows/WorkspaceView.cpp:141`). | BAR-0, BAR-3 |
| **Bar contents** | `ToolOption` (`crates/layer-ui/src/toolbar_components.rs:117`), `ToolbarComponentView` (`:64`) and the fitter `tool_options_layout`, which always reserves More (`:367`). `ToolbarContext` (`:51`) and `UiAction::ToolbarEdit` reject stale edits. The host row builders for numeric fields, choices and actions. The shared selection menu model (`crates/layer-ui/src/selection_masks.rs:226`). | BAR-0…8 |
| **Bar placement** | Overlay projection through `Camera::document_to_surface` (`crates/layer-ui/src/camera.rs:100`), already used for the transform box (`append_transform_overlay`, `crates/layer-ui/src/operation.rs:532`). "Host measures, Rust places": `drawer_placement` (`crates/layer-ui/src/drawers.rs:780`) and `DrawerDismissal::Explicit` (`:72`). `ResolvedLayout` work area, HUD and groups (`crates/layer-ui/src/layout.rs:1366`). The camera-only snapshot patch (`crates/layer-host/src/snapshot.rs:102`). | BAR-0 |
| **Disabled reasons** | `command_disabled_reason` (`crates/layer-ui/src/command_catalog.rs:862`), built for command search. | P-4, BAR-0, T-15 |

### 4.2 Design decisions to respect or explicitly supersede

| Decision | Record | Consequence |
| --- | --- | --- |
| Groups composite isolated; pass-through is deferred, following Clip Studio Paint (CSP) | `docs/history/layers-initial-design.md:265` | LYR-1 must propose superseding it, with migration. |
| Merge starts with Normal merges, whole clipping-stack baking, and isolated-group flattening; the lock blocks merges | `docs/history/layers-initial-design.md:209, 269` | LYR-2 adopts these. |
| "Show mask area" is a tinted overlay, not a black-and-white mask view | `docs/history/layers-initial-design.md:224` | LYR-4 extends the overlay instead of adding a mask-only view. |
| Add Mask consumes the selection | `docs/history/layers-initial-design.md:185, 189` | T-1 consumes it by default. |
| Import/Paste place images centered with handles | `docs/ui/image-open-import-proposal.md:80` | SEL-1/T-11 change this only for in-app copies. |
| The tonal panel has no apply/actions/destination UI (tested on Apple) | `docs/development/tonal-selection.md:41` | T-9 is withdrawn. A tonal result gets only the ordinary selection bar ([BAR-1](#bar-1-selection-p0)), never Apply/Cancel or tonal controls. |
| An optional floating selection action bar is Next work: default Deselect, Invert, Quick Mask, Fill, More; a View toggle; an explicit drag handle; no second customization system; never move during a stroke | `docs/ui/selection-command-inventory.md:142-151, 287` | BAR-1 supersedes the default set and the "optional, Next" status. More still opens the shared Selection Actions menu. The View toggle becomes one generic toggle. |
| A compact placement bar keeps Original Size, Cancel and Apply visible when Tool Options is hidden | `docs/development/image-placement-gtk-progress.md:868`, `docs/development/image-placement-web-android-progress.md:18`, `docs/development/apple-handoff.md:452` | BAR-3 replaces the six host bars rather than adding a second implementation (`docs/COMMIT_GUIDE.md:35`). Completion actions stay visible when the bar is hidden. |
| Tool Options orders completion actions first and its More always opens the complete form | `docs/ui/toolbar-components.md:56, 94` | Tool Options keeps the complete form, including completion actions and mode choices. The bar is a shorter projection of the same `ToolOption` data. |
| Mask editing shows a compact label such as "Editing Skin mask" with a route back to content, which can reuse the contextual tool surface | `docs/history/layers-initial-design.md:118`, `docs/ui/paintable-selection-proposal.md:188`, `docs/ui/saved-selections-assessment.md:62` | BAR-5 is that surface. |
| Fill and selection controls stay contextual, stable and clear of stylus contact; no required operation is gesture-only | `docs/history/layers-initial-design.md:199` | BAR-0 holds its position during contacts and moves only between them. Every bar item is a command. |
| A canvas touch hold starts the color picker | `docs/ui/color-picker.md:46` | The bar is never summoned by a hold. |
| Zen never projects alternate toolbar strips; floating panels stay visible | `docs/ui/shared-ui.md:250`, `docs/ui/window-bar.md:121` | As part of the panel layer, the bar behaves like a floating panel and stays visible in Zen. It is transient operation UI, not a projected toolbar strip. |
| Glass surfaces are registered per host and blurred by the shared presenter; popovers, menus and tooltips stay opaque; on GTK the blur lags a moving surface by about a frame | `docs/ui/panel-transparency.md:5, 240` | **Decided 2026-09-26:** the bar is a glass surface in the panel layer, registered like panels and drawers. It is stationary whenever visible, so the lag does not apply. Menus opened from it stay opaque. |
| No disabled placeholders for undelivered features; disabled commands give a reason | `docs/ui/selection-command-inventory.md:45`, `docs/ui/default-workspaces.md:25`, `docs/familiar-workspace.md:976` | Each bar item appears in the change that ships its command. |
| "Command bar" names the command search popup | `docs/ui/command-search.md:3` | The new surface is the **canvas action bar**. |
| Paint selection and Quick Mask have no confirmation step | `docs/ui/paintable-selection-proposal.md:65` | BAR-5 offers Exit, never Apply. |
| Names "Copy/Cut Selection to New Layer" and "Crop Canvas to Selection…" (Document); Paste Into, refine, subject and Select Similar are Later; `LayerAction::Clear` must never back Clear Selected | `docs/ui/selection-command-inventory.md:133, 134, 343, 348, 359` | Adopt the names. Raising a Later item is an explicit priority change. |
| The eight application menus are fixed and checked to fit at 1200 px | `docs/familiar-workspace.md:90`, `docs/development/workspace-header-progress.md:129` | A new Image/Document menu must be requalified at that width. |
| Stale EXIF is never copied into delivery files | `crates/layer-color/src/photo/metadata.rs:1` | IO-1 copies selected fields and regenerates the rest. |
| Save never adopts a photo's location | `crates/layer-ui/src/import_policy.rs:1`, `docs/ui/color-management.md:152` | "Edit in" round trips go through Export. |
| Photo codecs are pure Rust, with a vendor audit | `docs/development/portable-photo-core.md` | IO-2 lossy WebP needs a new pure-Rust encoder. |
| Undo history is not saved in projects | `docs/reference/project-format.md:32` | Persisted snapshots (RET-9) would change that policy. |
| Effect ABI 3 is checked for strict equality, with no adapter | `crates/layer-core/src/effects.rs:6`, `docs/reference/runtime-filters.md:40` | Add UI-only metadata as optional fields. A new value kind or binding strands saved documents unless the reader accepts both versions. |
| Blend modes composite in linear document RGB; Add/Subtract bounds are deliberate | `docs/internals/rendering.md:51` | LYR-1 decides each mode's domain and its HDR behavior. |

### 4.3 Easily mistaken facts

**Missing features**
- **Delete and Backspace have no default binding.** Clear Layer is unbound, ignores the selection and alpha lock, and discards a placed photo's source (`crates/layer-ui/src/art_layers.rs:1232`).
- **Liquify Reconstruct does not exist.** The renderer rejects it (`crates/layer-render-wgpu/src/lib.rs:1636`). Its documented design blends toward the stroke-start snapshot, not the original.
- **Rasterize Source does not bake placement.** No command currently bakes placement into pixels.
- **Artwork-mask editing has no command.** It is entered and left only through layer rows and their menus.
- **Removing the last polygon point is keyboard-only** (Backspace or Delete, `crates/layer-ui/src/selection_tools.rs:492`).

**Features that already exist**
- **Skew needs no model or shader change;** only `Pose`/`Handle` lack a shear term (`crates/layer-ui/src/operation.rs:14`).
- **Colorize exists** as Black & White with Tint.
- **Several "missing" adjustments can already be built from existing filters:**
  - Invert is Curves `[[0,1],[1,0]]`.
  - Threshold is roughly Posterize 2 (per channel).
  - Photo Filter is roughly White Balance or Split Tone.
- **Freeze-mask Liquify works:** Liquify respects the selection and Quick Mask.
- **Moving groups works.** Import batches already transform together.
- **A bar over the canvas already ships.** Every host shows a non-modal image-placement bar with Original Size, Cancel and Apply.
- **Transforms of paint, masks and selected pixels already resample on commit** (`LayerOperationKind::Transform`, `crates/layer-engine/src/canvas.rs:473`). Only a photo layer with no selection takes the lossless placement path (`crates/layer-ui/src/operation.rs:225`). Destructive Distort and Warp therefore need new passes, not a new placement model.

**Existing features that are narrower than they look**
- **Gaussian Blur's "Radius" is σ.** The kernel is `min(ceil(3σ),63)` px (`assets/filters/gaussian-prepare.wgsl:9`). Five other filters share σ: Unsharp Mask, High Pass, Bloom, Soft Focus and Pencil.
- **Vignette is expressed in percent,** so Image Size does not need to rescale it.
- **Pinch and Expand may be inverted** compared with Photoshop's Pucker and Bloat. Check visually before exposing them.
- **Flips do not swap canvas dimensions;** only 90° and 270° rotations do.
- **`Selection::bounds` ignores `inverted`** and walks every contour point (`crates/layer-core/src/selection.rs:284`). An inverted selection reports the bounds of its hole.
- **A finger reaches canvas tools only on photo-placement handles.** Every other touch contact navigates (`crates/layer-ui/src/session.rs:1138`).

**Behavior and platform details**
- **Export writes a deliberately minimal generated EXIF.** It does not drop metadata by accident.
- **The web filter manifest is a build copy,** not tracked in git. Edit only `assets/filters`.
- **The "proof lens" is an illustration,** not a comparison view.
- **Solo is an undoable document edit.** Do not use it for view-only comparison.
- **Each document has its own `UiSession`,** so any clipboard that crosses documents must live at window level.
- **Pen barrel buttons are hard-wired to Pan** on GTK, Android and Windows. `SampleFlags::BARREL_BUTTON` has no consumer.
- **A touch hold already opens the color picker.**
- **Window blur cancels an active transform or placement** through `cancel_layer_gesture` (`crates/layer-ui/src/session.rs:1184`, `crates/layer-ui/src/art_layers.rs:1867`). A bar that takes focus or opens its own window would discard the user's transform.
- **Any open host popup consumes the next canvas contact.** The contact dismisses the popup instead of drawing (`crates/layer-ui/src/session.rs:933, 1222`).
- **Published `CommandState.enabled` is frozen during a canvas contact** (`crates/layer-ui/src/session.rs:4745`). Use `UiSession::command` for live availability.
- **Escape does not leave Selection Layer editing,** only Quick Mask (`crates/layer-ui/src/session.rs:1035`).
- **Ruler actions appear in Tool Options whenever any ruler exists,** even under Paint (`crates/layer-ui/src/session.rs:4530`).
- **The camera reaches host UI one to two frames late on every host** ([source report 12](photo-editing-research/12-audit-host-overlays.md)).
- **The material brush pass binds 16 sampled textures,** which is WebGPU's default per-stage limit (11 in `material_brush.wgsl`, 5 in `brush_textures.wgsl`).
- **Hue-based adjustments compute hue from encoded RGB in the document's primaries,** so range boundaries shift in ProPhoto.
- **The histogram is computed on the CPU** from a full-resolution readback. It keys on the committed revision, so it does not update while a slider is being dragged.

### 4.4 New shared prerequisites

These pieces of infrastructure do not exist yet and block several build-list items each.

| ID | Prerequisite | Blocks |
| --- | --- | --- |
| P-1 | **Editable extent separate from the canvas** ([GEO-0](#geo-0-editable-extent-separate-from-the-canvas-p0-prerequisite)) | GEO-1…5, XF-6 |
| P-2 | **Stroke-start copy-on-write pages:** a copy of each tile taken before the stroke first changes it, kept on the GPU because nothing may be uploaded during contact | RET-1…4, RET-9, XF-5 Reconstruct |
| P-3 | **Window-level rich clipboard**, plus image writers in each host. Android needs a FileProvider for image `ClipData`. | SEL-1, SEL-2 across documents, ADJ-10 |
| P-4 | **Command disabled-reason field and a shared transient notice**, following the precedent of `ShortcutCapture.notice`. `command_disabled_reason` already produces text for command search (`crates/layer-ui/src/command_catalog.rs:862`); publish it on `CommandState`. | T-15, BAR-0 and every new command |
| P-5 | **Enumerated choices in tool settings** (today `DEFINITIONS` are f32 only, `crates/layer-ui/src/tool_settings.rs:72`) and **pen-button bindings**. Pen buttons belong to stage D of the command framework (explicit opt-in pen-button handling); never synthesize pen tip events from a barrel press ([command framework handoff](command-framework-handoff-2026-09-25.md)). | RET-2, XF-5, T-19 |
| P-6 | **Paged or conditional Properties UI** (only Curves is special-cased today) | ADJ-2, ADJ-4, ADJ-5 Selective Color |
| P-7 | **Optional effect-parameter metadata**: picker, slider mapping, soft bounds, page. These go in optional fields, not new ABI kinds. | ADJ-1, ADJ-2, T-3 |
| P-8 | **Pixel-tight content bounds.** `content_bounds` is tile-granular (`crates/layer-ui/src/operation.rs:133`). A GPU alpha-bounds pass exists in `crates/layer-render-wgpu/src/thumbnails.rs:408`. | GEO-3 Trim, XF-4, LYR-6 |
| P-9 | **A decision on the domain each blend mode computes in**, linear or encoded ([LYR-1](#lyr-1-blend-modes-blend-domain-and-pass-through-p0)) | LYR-1, RET-7, ADJ-9 |
| P-10 | **Projective and mesh placement geometry** through every consumer that assumes an affine: brush mapping, `Selection.affine`, mask transform, region jobs, mip selection, preview companions. Needed only for lossless (re-editable) Distort and Warp of placed photos; destructive Distort and Warp of paint, masks and selected pixels commit through the existing resampling path. | Lossless XF-1b and XF-3, GEO-6 |
| P-12 | **The canvas action bar** ([BAR-0](#bar-0-the-component-p0-prerequisite)): shared contexts, contents, anchor and placement in Rust; one non-modal host view on each platform, replacing the image-placement bars | BAR-1…8, and the canvas route of every selection, transform, crop and mask item |
| P-13 | **One transform session with composable geometry** ([BAR-2](#bar-2-transform-session-and-modes-p0)): affine pose with shear, then an optional corner quad, then an optional mesh, committed as one step | XF-1a, XF-1b, XF-3, XF-4, GEO-6 |
| P-11 | **An asynchronous GPU job model for heavy retouching**, reusing `local_tone` scheduling and `RegionRequest` generation staleness | RET-3…5, ADJ-6 |

### 4.5 Shortcut conflicts

New defaults give way to chords a user has already assigned. Two identical defaults are resolved silently by `CommandId::ALL` order (`crates/layer-ui/src/shortcuts.rs:646`).

Bindings are moving to the shared command framework ([command search](../ui/command-search.md), [shortcut audit](command-input-shortcut-audit-2026-09-25.md)). In that framework:
- a chord conflicts only when contexts and trigger lifecycles overlap, and the stage C contextual resolver decides precedence;
- presets that match other editors' defaults ship as stage F.

The shortcut audit's cross-editor tables list Photoshop, Krita, Clip Studio Paint and GIMP defaults for the commands proposed here (§3.1 clipboard, §3.3 tools, §3.5 selection, transform and layers). Use them when choosing defaults.

**Chords that conflict with existing defaults**

| Chord | Proposed use | Existing binding | What to do |
| --- | --- | --- | --- |
| Ctrl+V | Paste | Paste Image | Merge the two commands. |
| Ctrl+Shift+E | Merge Visible | Export | Choose another chord for Merge Visible. |

**Chords that the browser reserves on Web**

`KeyChord::available` (`crates/layer-ui/src/shortcuts.rs:119`) rejects some chords on Web: Ctrl+T, Ctrl+W, Ctrl+N, Ctrl+R, Ctrl+L, Ctrl+Q, Ctrl+P, and their Shift variants. Two proposed commands are affected:
- **Transform Again** (Ctrl+Shift+T) needs a Web-safe chord.
- **Edge rulers** cannot use Ctrl+R on Web.

**Chords that are free**
- **Copy, Cut and Copy Merged:** Ctrl+C, Ctrl+X, Ctrl+Shift+C. Keyboard focus in a native text field must still win.
- **Paste in Place:** Ctrl+Shift+V. **Paste Into:** Ctrl+Alt+Shift+V.
- **Copy/Cut Selection to New Layer:** Ctrl+J, Ctrl+Shift+J.
- **Feather:** Shift+F6.
- **Merge Down:** Ctrl+E. **Stamp Visible:** Ctrl+Alt+Shift+E.
- **Actual Pixels:** Ctrl+1. Fit keeps Ctrl+0.
- **Clear Selected:** Delete/Backspace. Which target wins when a ruler is selected or a header or tab has focus is a stage C context-precedence rule, not a new global binding.

## 5. Build list: new features

**Priority levels**
- **P0:** blocks a Tier 1 journey or a named feature.
- **P1:** removes friction from a Tier 1 journey or blocks a Tier 2 journey.
- **P2:** polish.
- **P3:** deferred.

**Quick win** marks items that mostly expose infrastructure that already exists.

Every new command needs:
- a `CommandId` variant, added to `ALL` (`crates/layer-ui/src/lib.rs:867`) and to `TOOLS` for tools;
- a label, an icon (an SVG in `apps/layer-web/icons`, which a test checks), `available_on`, and a customization description;
- a menu entry and a default shortcut;
- an enabled state and dispatch in `session.rs`;
- `command_without_renderer` for cancel commands.

Commands also reach the shared command catalog (`crates/layer-ui/src/command_catalog.rs`):
- The catalog adapts supported `CommandId`s and live menu entries automatically. Give each new command a useful catalog description; menu paths are only the fallback.
- Give each new tool a behavior `ToolCategory`. The ten existing categories are Drawing, Erasing, Blending, Warping, Selection, FillGradient, ShapesRulers, MoveTransform, ColorSampling and Navigation.
  - Crop and Straighten fit MoveTransform.
  - Clone, Heal and Spot Heal probably need a new Retouching category, because the command framework allows new categories for new tool domains.
- Extend `crates/layer-ui/src/command_catalog_tests.rs`.

Every command that acts on a selection, transform, crop, mask or other canvas object also declares its canvas action bar route: the [bar context](#canvas-action-bar-bar) it joins, its priority there, and a short label for touch. A document-wide command states that it has no bar route.

Serialized variant and shortcut IDs are persisted. Never rename them.

### Canvas action bar (BAR)

The canvas action bar is the shared component that shows a flow's next steps beside the object being edited. It generalizes the image-placement bar that every host already ships, and the optional selection action bar specified in the [selection inventory](../ui/selection-command-inventory.md) (§2).

The first draft of this plan gave each feature a menu entry, a dialog or a modifier. With the bar, each selection, transform, crop and mask flow can be finished from the canvas without a keyboard. Evidence: [source reports 11–14](photo-editing-research/).

**Principles**
- **Accelerator only.** The bar is an accelerator, not a home. Menus, Tool Options and command search stay complete. Every bar item is a `CommandId`, or a `UiAction` backed by one, so bindings, the catalog, validation and one-step history stay shared.
- **One bar, current context.** There is one bar per window, for the current canvas context. It has no bar-only actions and no disabled placeholders; each item appears in the change that ships its command.
- **Sticky modes.** Modes are sticky choices on the bar and in Tool Options. Modifier keys are temporary overrides and are never the only route.
- **Stays out of the way.** The bar never covers the pen contact, never moves during a contact, and never takes focus from the canvas.
- **Part of the panel layer.** The bar is a glass panel surface, like floating panels and drawers. It follows the panel transparency setting and theme, and stays visible in Zen. It is not a popover.

#### BAR-0 The component (P0, prerequisite)

**Replaces** the image-placement bars on all six hosts (section 4.1). The first delivery ports that context without losing behavior, and the six host bars are then deleted (`docs/COMMIT_GUIDE.md:35`).

**Shared Rust**
- **What it publishes:** a `CanvasBarView` in the UI state, holding the context kind, the context token, the anchor, and the items as `ToolOption`s in priority order.
  - Completion actions (Cancel, Apply, Done, Exit) sit at the trailing end and never overflow.
- **Context token:** a separate `CanvasBarContext` that embeds `ToolbarContext`, with the kind, the transaction and the fields `ToolbarContext` lacks.
  - The missing fields: current selection present, placing, mask target, Quick Mask, Selection Layer target, selected ruler, polygon in progress, picker active.
  - Extending `ToolbarContext` itself would bump its generation on every selection edit, and GTK rebuilds every Tool Options editor on each bump ([source report 15](photo-editing-research/15-phase1-bar-shared-rust.md)).
  - A `CanvasBarEdit` action rejects stale bar edits, as `ToolbarEdit` does for Tool Options.
- **Availability:** comes from `UiSession::command`, not from the published `CommandState.enabled`. That value is frozen whenever the canvas is not idle, which includes the whole of polygon construction. A disabled item shows its P-4 reason on tap or hover.
- **Fitting:** hosts measure natural sizes. Rust reserves the completion items first, then fits a prefix with `tool_options_layout` and places More before the completion items. `tool_options_layout` alone would stop at the first field that does not fit.
  - More opens the context's complete menu: the shared Selection Actions menu for selections, otherwise the complete Tool Options drawer.
  - The bar never scrolls; a scrolling bar hides actions.
- **Placement:** Rust computes it from the host-measured size, in the style of `drawer_placement`. There are two placements:
  - **Near object:**
    - The bar goes below the object's on-screen bounds, centred on the object, and flips above when there is no room below.
    - It is clamped to the work area, clear of the HUD and of docked and floating panels.
    - It also stays clear of the handles. The bottom handles sit on the lower edge. The rotate handle sits 2.5 × 12 DIP beyond the box's top edge, which ends up below the box after a vertical flip (`crates/layer-ui/src/operation.rs:581`).
  - **Bottom edge:**
    - The bar is centred at the bottom of the work area, above the HUD, where the placement bar sits today.
    - Near object falls back to it when:
      - the anchor is off-screen;
      - the anchor covers most of the work area (Select All, inverted and tonal selections);
      - the anchor changes with every stroke (paint selection, polygon construction).
    - Modes with no object ([BAR-5](#bar-5-modes-without-an-object-p1)) always use it.
- **Anchors:**
  - **Selection:** bounds of nonzero coverage that honor `inverted`, not `Selection::bounds`.
  - **Transform:** the quad that `append_transform_overlay` already projects.
  - **Other objects:** the crop rectangle, guide handles, the clone-source disc, a sampler point.
  - Hosts query the placement when the bar's view, its anchor or the layout changes, and when the camera settles. The bar is hidden while the camera moves, so it does not ride in the camera-only patch.

**Lifecycle**
- **When it appears:** once a context becomes current and the gesture that created it has ended. That can be pen-up, an asynchronous Wand or Color Select result, or a command such as Select All, Load Selection or Reselect.
- **When it hides:**
  - during any canvas contact or handle drag;
  - during camera gestures, reappearing at its new place when the camera settles, because the camera reaches host UI one to two frames late;
  - during the color-picker session;
  - during workspace panel drags.
- **How it hides:** Rust reports `canvas_bar_hidden` in the input reply, beside `chrome_hidden`, so pen-down needs no snapshot. Hosts hide at once and show again after a shared debounce. The debounce also covers wheel and pinch zoom, which have no end event, and stops a run of short contacts from making the bar blink.
- **What it never does:** move during a contact, pan the canvas, resize the viewport, or appear in response to a touch hold (the color picker owns holds).

**Input and focus**
- **Contacts:**
  - A contact on the bar never paints, and a canvas contact never dismisses it.
  - The bar never sets the host `popup_open` fact, never takes window focus and never opens a window of its own. A popup would make the next canvas contact dismiss it instead of drawing, and losing window focus cancels transforms and placements (section 4.3).
  - Popovers opened from its items (Refine, More) follow the ordinary popover rules and set `popup_open` while open. They must not take window focus either: Android's focusable menus would blur the window and cancel a transform.
  - The input facts carry the bar's bounds, so Zen hover over the bar does not reveal the docks, and a tap on More toggles its drawer instead of closing it as an outside contact.
- **Keyboard:**
  - It appears without taking focus.
  - A focus command moves into it (Canva uses Ctrl+F1, FigJam F6). Inside, it is one Tab stop with arrow keys (WAI-ARIA toolbar pattern).
  - Enter and Escape keep their context meaning (Apply and Cancel) while focus is in the bar. This is a stage C precedence rule.
- **Labels and targets:**
  - Accessible names and tooltips come from `CommandState`.
  - Touch targets are 48 px/dp, and 44 pt on Apple.
  - Use short text labels beside icons on touch where they fit. Unlabelled small icons are a recurring Krita complaint.

**Hosts** ([source report 12](photo-editing-research/12-audit-host-overlays.md)). None uses a popup.
- **GTK:** a new `DockSurface` slot, which shares the layout allocator and is collected for glass by `DockSurface::snapshot`. It is exempt from Zen fading, as floating panels are. Not a `gtk::Popover`.
- **Web:** an absolutely positioned child of `#workspace` in the floating-panel band, measured by `glass.js`. Not a `popover` or `<dialog>`, which set `popup_open`.
- **Android:** an in-tree Compose element placed like floating groups, drawn with `Modifier.glass` and registered as a `chromeRegion`. Not a `Popup`, which adds a window and cancels contacts when focusable.
- **Apple:** a view inside the `WorkspacePanels` `ZStack`, drawn with `glassSurface` and `GlassRegistration`, outside `dismissTransients` and `editorPopover`. In the root `ZStack` it would cover the drawers and the contact menu.
- **Windows:** a `Border` in the workspace `Canvas` with the glass brush, included in `WorkspaceView::glass()`. That function returns no regions while chrome is hidden (`apps/layer-windows/WorkspaceView.cpp:771`), so floating panels lose their blur in Zen today; fix it first.
- **Z-order on every host:** docked groups < floating groups < the bar < drawers < header < menus and popovers.
- **All hosts:** a Tool Options constructor that needs no dock tile, drag hooks or panel, with a More button anchored to the bar instead of a tile.

**Appearance and settings**
- **Surface:** a glass panel surface in the panel layer, drawn with the `GlassPalette` panel color and registered for blur like floating panels and drawers ([panel transparency](../ui/panel-transparency.md)). At the Off level it is the opaque panel theme.
  - It never moves while visible, because it hides during contacts, handle drags and camera gestures. The GTK blur lag for moving surfaces therefore does not apply.
  - It republishes its glass region only when it appears, disappears or changes place.
  - Today any change to the region list repaints every glass bound (`crates/layer-render-wgpu/src/backdrop_blur.rs:510`). Repaint only the symmetric difference, so hiding the bar at pen-down does not repaint all panel glass in the stroke's first frame.
  - Menus and popovers opened from it (More, Refine) stay opaque, like all popovers.
- **Zen:** the bar stays visible in Zen, as floating panels do.
- **Toggle:** one View toggle, **Show Canvas Action Bar**. It replaces the inventory's "Show Selection Action Bar".
  - While it is off, completion contexts (transform, placement, crop, refine sessions) still show Cancel and Apply at the bottom edge. Photoshop Elements does the same, and the placement bar already guarantees it.
- **Overflow:** the bar's More includes Hide Bar and Placement ▸ Near Object / Bottom Edge.

**Not in the first delivery**
- **Moving the bar.** When it is added:
  - an explicit handle that drags immediately (AGENTS.md);
  - a position that lasts until the next new context, as in Clip Studio Paint;
  - Reset Position in the overflow;
  - a pin stored relative to the window, not the display.
- **Customizing contents.** Later, only through the workspace command inventory, never a second customization system. Reordering then follows the drag convention.

**Acceptance**
- **Shared tests:**
  - context derivation for every state in [source report 11](photo-editing-research/11-audit-canvas-states.md);
  - placement: below, flipped, clamped, fallback, handle clearance and rotated views;
  - hide rules;
  - stale edits.
- **Native scenarios on each host, with mouse, touch and pen:**
  - a tap on the bar never paints;
  - a canvas contact never dismisses it;
  - an active transform survives a tap on the bar;
  - it hides during strokes, handle drags and camera gestures, and returns once;
  - it appears after an asynchronous Wand result;
  - both themes, and narrow widths.
- **Pen latency:** measure the bar's effect on Windows Independent Flip and Android shared-buffer presentation before enabling it by default.

**Decisions** (all confirmed as recommended on 2026-09-26)
1. **Scope.** Resolved 2026-09-26: the bar is the canvas route for selection, transform, placement, crop and mask flows, superseding the inventory's optional Next bar.
2. **Default placement.** Recommended: Near Object on desktop and tablets, Bottom Edge on phones; both selectable. Tablet-first products use a fixed bottom bar, while desktop editors anchor theirs.
3. **Selection bar under painting tools.** Recommended: hidden while a painting tool is active, shown for selection tools and Move. Illustrator hides its bar when a tool has its own on-canvas widgets, and painting inside a selection is the common case.
4. **Zen and surface.** Decided 2026-09-26: the bar is a glass surface in the panel layer, and stays visible in Zen like floating panels.
5. **Where the toggle is stored.** Resolved for Phase 1: `DockLayout.canvas_bar`, beside `canvas_info`. It is on in every built-in workspace. Saved layouts default missing fields, so no migration is needed.
6. **Movable in the first delivery.** Recommended: no; add the handle as P1 after the anchored bar is qualified.
7. **Name.** Resolved for Phase 1: "canvas action bar", because "command bar" is taken by command search.

#### BAR-1 Selection (P0)

**Contexts**
- **Completed selection:**
  - Shown while a selection tool or Move is active.
  - Also shown after a command creates a selection (Select All, Load Selection, Select Layer Opacity, Reselect), until the next tool change.
  - Painting tools hide it (decision 3).
- **Selection under construction (polygon):** Complete · Remove Last Point · Cancel, at the bottom edge. Remove Last Point is a new command; today only Backspace does it.
- **Tonal Range:** the ordinary completed-selection bar at the bottom edge, never Apply/Cancel or tonal controls (`docs/development/tonal-selection.md:38`).
- **Paint selection:** at the bottom edge, because its coverage changes with every stroke. There is no confirmation step.
- **Wand and Color Select:** while the last result can still be refined, a Tolerance slider leads the bar and amends the same undo step (T-10).
- **Move with a selection:** a Leave Copy toggle, the touch equivalent of Alt-drag (SEL-3).

**Contents, in priority order**

| # | Item | Command | Milestone |
| --- | --- | --- | --- |
| 1 | Deselect | Existing | M1 |
| 2 | Invert | Existing | M1 |
| 3 | Copy to Layer ▾ (Copy / Cut Selection to New Layer) | [SEL-2](#sel-2-copy-and-cut-selection-to-new-layer-p0) | M2 |
| 4 | Transform (selected pixels) | Existing `ScaleRotate` | M1 |
| 5 | Refine ▾: Grow, Shrink; Feather, Border, Smooth, Transform Outline; Refine Edge | Existing; [SEL-5](#sel-5-feather-border-smooth-transform-selection-p1); [SEL-6](#sel-6-refine-edge-p1-the-inventory-has-it-as-later) | M1; M2; M7 |
| 6 | Mask (Reveal Selection on the active layer) | Existing `LayerAction::AddMask` | M1 |
| 7 | Adjust ▾ (new effect layer masked by the selection) | [T-1](#6-tweaks-to-existing-tools) | M2 |
| 8 | Fill ▾: Fill Selection; Content-Aware Fill | Existing; [RET-5](#ret-5-content-aware-fill-and-remove-p1) | M1; M8 |
| 9 | Clear ▾: Clear Selected, Clear Outside | [SEL-4](#sel-4-clear-selected-and-clear-outside-p0) | M2 |
| 10 | Copy ▾: Copy, Copy Merged, Cut | [SEL-1](#sel-1-pixel-clipboard-p0) | M3 |
| 11 | Crop to Selection | [GEO-3](#geo-3-canvas-size-expand-canvas-to-layers-trim-crop-canvas-to-selection-p0) | M3 |
| 12 | Quick Mask; Save as Selection Layer… | Existing | M1 |
| — | More: the shared Selection Actions menu, including Export Selection ([IO-5](#io-5-export-layers-and-selection-p2)) | Existing | M1 |

**Notes**
- **Excluded items:**
  - Select All is left out: it replaces the selection the bar is acting on.
  - Items that differ only by modifier (Add/Subtract/Intersect) stay in Tool Options.
- **GTK:** T-20 adds the GTK "Selection Actions…" button, which opens the same menu as More.
- **Anchor:** bounds of nonzero coverage from the selection readback, or a cached contour bound that honors `inverted`.

#### BAR-2 Transform session and modes (P0)

**One session (P-13)**
- **Geometry:** the transaction holds, in order:
  1. the pose: offset, scale, rotation, and the shear added by XF-1a;
  2. an optional corner quad, for Distort and Perspective;
  3. an optional mesh over that quad, for Warp.
- **Switching modes keeps the accumulated geometry:**
  - Free → Distort takes the four transformed corners exactly.
  - Distort → Warp seeds the grid from the quad's projective map.
  - Returning from Warp to Free or Distort keeps the mesh and shows the outer handles on its hull. Photoshop and Procreate keep it; this is a decision.
- **Apply, Cancel and Reset:** Apply commits once, as one undo step. Cancel discards every mode. Reset returns to identity without leaving the session.
- **Modifiers:** temporary overrides only.
  - Shift keeps proportions or constrains.
  - Alt scales from the centre.
  - Once Ctrl/Cmd is tracked live (XF-1a), Ctrl/Cmd-drag of a corner distorts and of an edge skews.
- **Commands:**
  - Add a command per mode, for the catalog, bindings and search.
  - Relabel "Scale / rotate" as "Transform", keeping the persisted `ScaleRotate` ID.
  - The existing `TransformAspect` toggle becomes the Uniform mode.

**Contents**

| Item | Command | Milestone |
| --- | --- | --- |
| Mode: Free · Uniform | Existing `TransformAspect` | M1 |
| Mode: Distort (corners distort, edges skew; a Perspective option moves opposite corners symmetrically) | [XF-1b](#xf-1b-distort-and-perspective-p0) | M1 |
| Mode: Warp | [XF-3](#xf-3-warp-and-mesh-transform-p1-requested) | M1 |
| Flip Horizontal · Flip Vertical | New, within the session | M1 |
| Rotate 90° left · right | New | M1 |
| Reset | New | M1 |
| Interpolation ▾ | [XF-2](#xf-2-resampling-quality-p0) | M1 |
| Warp only: Grid ▾ (3×3, 4×4, 5×5, custom cells) | XF-3 | M1 |
| Warp only: Split | XF-3 | M5 |
| Pivot ▾ · Snapping | [XF-4](#xf-4-transform-ergonomics-p1) | M5 |
| Cancel · Apply | Existing | M1 |

**Notes**
- **Skew:** Ctrl/Cmd-drag of an edge, a Skew numeric field, and Distort's edge handles, all in M1.
- **Modes appear as they ship.** Each mode joins the segmented choice in the step that implements it; there are no disabled placeholders.
- **Anchor:** the transform quad, below the bottom handles.
  - Hidden while a handle or mesh point is dragged.
  - At the bottom edge when the box fills the view.
- **Entry points:**
  - Transform on the selection bar;
  - the Move tool's choice;
  - Ctrl+T on desktop;
  - the Photo toolbar tile.

  On Web, where Ctrl+T is reserved, the bar and the tile are the keyboard-free routes.
- **Touch:** today a finger reaches only placement handles. XF-4 adds finger-touch handles for every transform; the bar itself is finger-reachable from the start.

#### BAR-3 Placement and paste (P0)

- **Contents:** everything in BAR-2, plus Original Size (existing) and a count of the images being placed.
- **First context ported:** the first BAR-0 delivery ports this context and deletes the six host bars. It keeps their guarantees:
  - Original Size, Cancel and Apply stay visible while Tool Options is hidden;
  - Escape and Android Back cancel;
  - Apple targets are 44 pt.
- **Skewed layers:** they are refused today (`crates/layer-ui/src/operation/placement.rs:67`); XF-1a lifts that.
- **Distort and Warp on a placed photo** is a decision:
  - (a) **Bake on Apply.** No bake operation exists: Rasterize Source leaves placement untouched. Large or HDR photos would often exceed the history budget at Apply ([source report 17](photo-editing-research/17-phase1-transforms.md)).
  - (b) **Disabled with a reason until P-10**, pointing to the route that already works: Select All, then Transform, which takes the tested destructive path for photo pixels. **Confirmed for Phase 1.**
  - (c) **Later:** switching a single placement to Distort re-bases it into a pixel transaction that keeps the source, with no bake.
  - Free, Uniform, Skew, Flip, Rotate 90° and Reset stay lossless in every option.
- **Exit:** while a placement is active, layer actions are refused (`crates/layer-ui/src/session.rs:2140`), so the bar is the visible way out.

#### BAR-4 Crop (P0, with GEO-1)

- **Contents:**
  - Ratio ▾ (Free, Original, 1:1, 4:5, 3:2, 16:9, Custom), Swap Orientation;
  - Overlay ▾ (Thirds, Golden, Grid, None);
  - Straighten (GEO-2), Rotate 90°, Perspective (GEO-6);
  - Fit Content (Trim; GEO-3 with P-8);
  - Delete Cropped Pixels;
  - Reset, Cancel, Apply.
- **Anchor:** the crop rectangle, which usually fills most of the view, so the bar is mostly at the bottom edge.

#### BAR-5 Modes without an object (P1)

These use the bottom edge: a label, the actions, then the exit.

| Mode | Bar | Needs |
| --- | --- | --- |
| Quick Mask | "Quick Mask" · Invert · Fill · Clear · Grow/Shrink… · Save as Selection Layer… · Exit | Existing commands (`quick_mask_menu`); no Apply |
| Selection Layer editing | "Editing *name*" · Load · Invert Stored Mask · Return to Artwork | Existing; Escape to exit (T-25) |
| Artwork mask editing | "Editing *layer* mask" · Show Mask ▾ (tint or grayscale) · Invert · Density/Feather · Disable · Apply Mask · Edit Content | New commands to enter and leave mask editing; [LYR-4](#lyr-4-mask-properties-p1), T-21 |
| Picker armed | "Click a neutral point" (or black or white point) · Sample Size ▾ · Cancel | [ADJ-1](#adj-1-pickers-and-targeted-curves-p0-for-curves-and-white-balance-pair-levels-with-adj-4) |
| Targeted Curves | "Drag on the image" · Done | ADJ-1 |
| Before/After | "Before / After" · Split ▾ · Exit | [VIEW-2](#view-2-before-and-after-p1) |

Tool Options keeps its fields. The bar adds the label and the exit that `docs/history/layers-initial-design.md:118` asked for.

#### BAR-6 Small canvas objects (P1–P2)

- **Drawing guides (existing):**
  - Selecting a guide with the Ruler or Move tool anchors the bar to it: Delete Guide · Snap · Straighten Image to Guide (GEO-2, Straight rulers) · Hide Guides.
  - Tool Options then stops showing ruler actions under unrelated tools.
- **Clone source disc (RET-2):**
  - Tapping the disc shows Aligned · Source ▾ · Flip H/V · Rotation/Scale · Reset Offset · Overlay.
  - The bar is hidden while painting. The disc itself drags immediately.
- **Color samplers (VIEW-3):** Delete · Sample Size ▾ · Readout ▾.
- **Lens-blur focus pins:** get a bar if ADJ-7 adds them.

#### BAR-7 Region jobs (P1, with P-11)

- **Region-scoped jobs:** Content-aware fill (RET-5), Patch (RET-6) and similar jobs show progress and Cancel in the region's bar, then return to the selection context.
- **Heal:** completes after pen-up and has no bar.
- **Document-wide jobs:** Flatten and Image Size keep their existing progress presentation.

#### BAR-8 Layer without a selection (P2; decide later)

- **Evidence:** Photoshop's bar offers layer actions with no selection (Select Subject, Remove Background, Mask). No non-Adobe product does.
- **Candidate contents with the Move tool:** Transform, Flip, Mask, Select Layer Opacity, Duplicate, and Align/Distribute for several layers (LYR-6).
- **Anchor:** needs pixel-tight bounds (P-8).
- **Timing:** defer until BAR-1 and BAR-2 are qualified. The main complaint about these bars is that they appear unrequested.

#### Target routes

Each route starts from the canvas and uses no application menu.

| Journey | Route |
| --- | --- |
| 26 Extract part of an image | Select → **Copy to Layer** |
| 14 Cut out a subject | Select → **Refine ▾** → **Mask**, or **Clear** |
| 11 Adjust one region | Select → **Adjust ▾** → adjustment |
| 19 Blur the background | Select → **Invert** → **Adjust ▾** → blur |
| 27 Warp onto a surface | Select → **Transform** → **Warp** → drag → **Apply** |
| 3 Correct perspective | **Transform** → **Distort** (Perspective) → **Apply** → Crop |
| 17 Composite | Paste → **Distort** or **Warp** → **Apply** |
| 1 Crop | Crop → **Ratio ▾** → drag → **Apply** |
| 2 Straighten | Crop → **Straighten** → draw along the horizon → **Apply** |
| 21 Remove an object | Select → **Fill ▾** → **Content-Aware** |
| 22 Clone structured detail | Clone → **Set Source** → paint; adjust on the disc bar |
| 5, 6 Levels, Curves, cast | Picker in Properties → **"Click a neutral point"** → tap |

### Geometry (GEO)

#### GEO-0 Editable extent separate from the canvas (P0, prerequisite)

**Unblocks** 1–4 and XF-6.

**Problem**
- `Layer::local_extent` returns the canvas size for paint layers (`crates/layer-core/src/layers.rs:52`).
- Tile coordinates are `u32`, and tiles beyond the extent are rejected (`crates/layer-core/src/raster.rs:104, 362`).
- About 63 consumers across the renderer, engine and storage use the extent.
- As a result, shrinking the canvas invalidates paint tiles, content above or left of the origin cannot exist, and a non-destructive crop is impossible.
- Composition and capture also stop at the canvas edge (`crates/layer-render-wgpu/src/scene.rs:1467, 1723`).

**Decision needed**, with a recommendation:

| | Option | Pros | Cons |
| --- | --- | --- | --- |
| **(a) Recommended** | Store a per-layer extent and origin, independent of the canvas, with a `.capy` version step | Crop with "Delete cropped pixels" off; Expand Canvas to Layers; 90° rotation of non-square canvases; painting beyond the canvas after a placement transform (XF-6) | — |
| (b) | Rewrite tiles for paint layers on every geometry change, and use offsets/placement only for photos, selections, masks and rulers | — | Destructive only |

**Also required**
- Composition beyond the canvas for the crop preview, or an explicit statement that hidden paint is not shown.
- Validation against:
  - the project limits (`crates/layer-core/src/project.rs:88`: 32,768 px per axis, 16,384 tiles, 1 GiB);
  - the 8,192 px limit for new documents (`crates/layer-ui/src/document_files.rs:133`);
  - the GPU texture limit (`crates/layer-render-wgpu/src/lib.rs:1694`).
- A policy for locked layers: `append_operations` refuses them (`crates/layer-engine/src/canvas.rs:556`).

#### GEO-1 Crop tool (P0)

**Unblocks** 1, 2, 3.

**Behavior**
- A ratio-constrained rectangle with thirds and golden overlays.
- Dragging beyond the canvas extends it.
- Delete cropped pixels is off by default.
- Enter applies and Escape cancels.

**Reuse**
- **Ratio and size constraints:** Free/Ratio/Size from the rectangle selection (`crates/layer-ui/src/selection_tools.rs:61`).
- **Apply/Cancel:** the `ApplyTransform`/`CancelTransform` completion actions, already shown as tool-option buttons on every host (`crates/layer-ui/src/session.rs:4516`, `crates/layer-ui/src/toolbar_components.rs:256`).
- **Bar:** the crop context of the canvas action bar ([BAR-4](#bar-4-crop-p0-with-geo-1)). It holds Ratio ▾, Overlay ▾, Straighten, Rotate 90°, Fit Content, Delete Cropped Pixels, Reset, Cancel and Apply.
- **Finger-touch handles:** the `placement_touch_hit` routing (`crates/layer-ui/src/operation.rs:508`).
- **Shifting the geometry:**
  - offsets for root layers only (child offsets are cumulative);
  - `LayerMask.placement`;
  - `Selection::translated`;
  - `RulerGeometry::translated`.
- **Composition:** already handles arbitrary integer offsets (`crates/layer-render-wgpu/src/scene.rs:861`).
- **Document resize:** the renderer's resize path (`crates/layer-render-wgpu/src/lib.rs:1685`).

**New**
- `CursorSegment` has only lines and markers. The dimmed area outside the crop needs a new overlay marker.

**Constraints**
- Effects that depend on document geometry shift or rescale after a crop: Vignette and `fx_extent()` users, the pattern phase of Grain and Halftone, and the document-sampled Kaleidoscope, Swirl and CRT.
- The command follows `require_document_idle` and `!document_file.busy` (`crates/layer-ui/src/session.rs:1991`).

#### GEO-2 Straighten (P0)

**Unblocks** 2.

**Behavior**
- Draw a straighten line inside Crop, or reuse an existing Straight ruler's line (`crates/layer-core/src/rulers.rs:15`). That is the Measure-then-Straighten flow GIMP users know.
- A numeric angle field is available; Shift snaps to 15° steps.
- **Entry points:**
  - Straighten on the crop bar arms the line.
  - A selected Straight guide offers Straighten Image to Guide on its bar ([BAR-6](#bar-6-small-canvas-objects-p1p2)).

**Apply**
- **Paint pixels:** one undo step through `append_operations`, with XF-2 resampling.
- **Everything else (masks, saved selections, photo placements, rulers):** metadata only. Rulers need an affine map built from `handles()`.

**Depends on** GEO-0 and GEO-1.

#### GEO-3 Canvas Size, Expand Canvas to Layers, Trim, Crop Canvas to Selection (P0)

**Unblocks** 1, 4, 14.

**Naming**
- Use the inventory's name and meaning for **Crop Canvas to Selection…**: crop to the bounding box of nonzero coverage, affecting stored masks (`docs/ui/selection-command-inventory.md:134, 285`).
- Do not reuse "Reveal all" (the mask command) or "Fit canvas" (the view command).

**Menus**
- Either add a Document/Image menu, requalifying the header width, or group these commands in Edit next to Assign Profile, Convert Color Space and Change Bit Depth, which live there today (`crates/layer-ui/src/lib.rs:225`).

**Reuse**
- **Trim:** exact coverage bounds from GPU readback (`crates/layer-render-wgpu/src/selection_readback.rs:56`), and a wand with the Visible source for corner-color trimming.
- **Paper:** the background is procedural and needs no change.

**Bar**
- Crop to Selection on the selection bar.
- Trim appears as Fit Content on the crop bar.
- Canvas Size and Expand Canvas to Layers are document-wide dialogs with no bar route.

**Depends on** GEO-0 and P-8.

#### GEO-4 Image Size (P0)

**Unblocks** 4, 30.

**Pipeline**
- Build on the SetColor-style worker and the prepare/commit history transition, using the `RowResampler`. It already runs on Web too.
- No GPU resample kernel exists. Add one only if measurements justify it.

**What gets scaled, and how**
- **Selections and masks:** metadata only (`Selection.affine`, `LayerMask.placement`).
- **Placed photos:** update `placement`.
- **Effect parameters:** scale only those declared `unit: "px"`, clamped to their manifest maximum (Gaussian σ ≤ 21). Vignette and other percent-based parameters stay as they are.

**New**
- `Edit::SetResolution`; there is none today, and Document Properties is read-only.
- Conversion between px, cm, in and % in `NumericControl`, which has only a unit label.
- Validation against the limits listed under GEO-0.

#### GEO-5 Rotate and flip the image (P1)

**Behavior**
- Flips keep the canvas size. 90° and 270° rotations swap it.
- All rotations and flips are exact pixel permutations, done through `Interpolation::Nearest`. Existing flip oracle tests cover this path (`crates/layer-render-wgpu/src/pixel_transform_tests.rs:282`).

**Commands**
- New `CommandId`s such as `FlipImageHorizontal`, with new icons. The view commands already own the names, shortcut IDs and icons.

**Order**
- Flips ship first, through `append_operations` plus metadata.
- Rotating a non-square canvas needs GEO-0.

**Bar**
- These are document-wide menu commands with no bar route. The crop bar's Rotate 90° rotates the crop frame together with the image, as Photoshop's crop does.
- The transform bar's Flip and Rotate 90° act on the transformed content only.

#### GEO-6 Perspective crop (P2)

- Uses XF-1b's projective geometry. Paint pixels take the destructive projective pass. Placed photos need P-10, or baking as decided for BAR-3.
- A Perspective choice on the crop bar.

### Transform (XF)

#### XF-1a Skew handles (P0)

**Unblocks** 17 and part of 27. **Quick win.**

**Scope**
- Add shear to `Pose` and `Handle` (`crates/layer-ui/src/operation.rs:14`).
- Add skew numeric fields.
- Remove the refusal in `crates/layer-ui/src/operation/placement.rs:67`.
- No format or shader change is needed.

**Input**
- Track Ctrl/Cmd live, like Shift and Alt (`crates/layer-ui/src/session.rs:947`), so Ctrl-drag of an edge can skew.
- The shear term is the first step of the one transform session ([BAR-2](#bar-2-transform-session-and-modes-p0), P-13).
- **Touch and pen route:** the Skew numeric field in Tool Options and Distort mode's edge handles, both in M1. The first draft put mode chips in the Operation Tool Set, which lives only in the docked Tool Options; that plan is superseded.
- The Free and Uniform modes and the Flip, Rotate 90° and Reset commands ship with this item on the action bar.

#### XF-1b Distort and perspective (P0)

**Unblocks** 3 and 27.

**Destructive first (M1)**
- Distort is a mode of the transform session (BAR-2). Its corner quad commits through the existing resampling path (`LayerOperationKind::Transform`, `crates/layer-engine/src/canvas.rs:473`), which today resamples paint, masks and selected pixels.
- **Inverse mapping:** `ImageTransform` and `pixel_transform.wgsl` need a projective inverse. Update the four sites that hard-code 1 px of sampling support together with XF-2.
- **Moved selection:** after a projective commit the selection cannot stay `Selection.affine` metadata. Rasterize its coverage through the same pass.
- **No format step:** committed pixels need no `.capy` change.

**Lossless for placed photos (M5)**
- Generalize `LayerProperties.placement`, not `ImageTransform`, and every consumer that assumes an affine (P-10), with a `.capy` version step.
- **Decide** whether painting on a layer with perspective placement is allowed, or whether the placement must be baked first.
- Until then, the BAR-3 decision applies: bake on Apply, or keep Distort disabled on placements with a reason.

#### XF-2 Resampling quality (P0)

**Unblocks** 2, 3, 4, and fixes a current bug.

**Already in place**
- Linear, premultiplied Float32 transforms.
- The CPU resizer clamps overshoot.

**Changes**
- Add Bicubic and Lanczos, and offer Nearest (this absorbs T-6).
- Four sites hard-code 1 px of sampling support. Each must widen:
  - `crates/layer-core/src/affine.rs:29`;
  - `region_jobs` (`crates/layer-render-wgpu/src/paint_transform/snapshot.rs:184`);
  - `crates/layer-render-wgpu/src/pixel_transform.wgsl:35`;
  - the Liquify bounds in `material_sources.rs`.
- Respect the 16-source-view split (`crates/layer-render-wgpu/src/pixel_transform.rs:7`).
- Clamp overshoot in the R8 wetness and visibility pipelines too.

**Bug fix**
- Placement ignores `Interpolation`.
- Its preview mipmaps are display-only, so export and flatten of a scaled-down photo alias (`crates/layer-render-wgpu/src/scene.rs:1463`).
- Add prefiltering to the exact capture path.

**Bar**
- Interpolation ▾ on the transform bar (BAR-2) and in Tool Options.

#### XF-3 Warp and mesh transform (P1, requested)

**Unblocks** 25 and 27.

**Behavior**
- Grid presets: 3×3, 4×4, 5×5 and custom.
- Split lines.
- Select several points at once.
- Tangent handles.
- Named presets (Arc, Bulge and so on) are P2.

**Session and bar**
- Warp is a mode of the one transform session ([BAR-2](#bar-2-transform-session-and-modes-p0)), switched on the bar like Procreate's Warp and Clip Studio Paint's Mesh Transformation. It is not a separate command with its own transaction.
- The grid is seeded from the session's current quad, so a Free or Distort transform made first is kept.
- In Warp mode, the bar adds Grid ▾ and Split.
- Mesh points and tangent handles are on-canvas handles: they drag immediately with every device and never use holds.
- Selecting several points needs a tap-to-toggle rule for touch and pen, because Shift is not available.

**Rendering**
- `PixelTransform` is an inverse-mapping pass over a full target with an affine inverse. A mesh needs a new pass that tessellates the patch and rasterizes it forward with XF-2 sampling.
- The interactive preview is limited to two targets (`crates/layer-render-wgpu/src/paint_transform.rs:9`).

**Model: destructive first (M1)**
- Paint layers, masks and selected pixels already commit transforms by resampling (`crates/layer-engine/src/canvas.rs:473`). Commit the mesh result through the same one-step path, as Procreate and Clip Studio Paint do on raster layers.
- This needs no P-10 work and no format step.
- **Moved selection:** as for XF-1b, rasterize its coverage through the mesh pass.

**Model: lossless placements (M5)**
- A mesh stored in `placement` breaks every affine consumer (P-10).
- Either add a separate warp stage applied after the layer's own pixels, or disable painting on warped layers until the warp is baked.
- A `.capy` version step.
- Until then, the BAR-3 decision applies to placed photos.

**Reuse**
- The transaction pattern, Apply/Cancel, and one undo step (`crates/layer-ui/src/operation.rs:293`, `crates/layer-engine/src/canvas.rs:473`).

**References**
- Krita Mesh/Warp ([transform tool](https://docs.krita.org/en/reference_manual/tools/transform.html)).
- GIMP users are sent to Krita for this ([pixls thread](https://discuss.pixls.us/t/looking-for-gimp-equivalent-of-photoshops-transform-warp/34923)).

#### XF-4 Transform ergonomics (P1)

**Unblocks** 17, 26, 27. This absorbs T-13.

**Already in place**
- Apply/Cancel buttons, in Tool Options and, during placement, in the host placement bar. BAR-2 and BAR-3 carry them onto the canvas action bar.

**New**
- **Finger-touch handles** for paint and mask transforms, not only photo placement. Today every other finger contact navigates (`crates/layer-ui/src/session.rs:1138`).
- **Transforming groups and several layers together.** Groups cannot take a `placement` (`crates/layer-core/src/layers.rs:1086`), and `target_transform` adds only parent offsets. Generalize `Placement.members` and `append_operations`, and extend the preview beyond two targets. The bar anchors to the union of the members' bounds and adds Align ▾ once LYR-6 exists.
- **Pivot and reference point**, after pixel-tight bounds exist (P-8). Pivot ▾ goes on the transform bar.
- **Snapping** that reuses `choose_ruler` and `RulerConstraint::project` (`crates/layer-core/src/rulers.rs:113, 161`). A Snapping toggle goes on the transform bar, as in Procreate.
- **Arrow-key nudge**, with key repeat (today only Undo/Redo repeat, `crates/layer-ui/src/shortcuts.rs:426`). Repeated nudges merge into one undo step, as a continuous action (`Begin → Update → End`) under the command framework's stage C.
- **A Web-safe chord for Transform Again.**

#### XF-5 Liquify upgrade (P1)

**Unblocks** 25.

**Quick win (T-4)**
- Expose Twirl CCW, Pinch and Expand after visual verification.
- Expose Crystals together with `deform.distortion`.
- Expose `deform.momentum`.

**New renderer work: Reconstruct**
- The currently rejected mode, restoring toward a pre-Liquify original.
- Shares P-2 with RET-9.

**Not needed**
- **Freeze/Thaw:** selection and Quick Mask already freeze. Add a separate freeze mask only if it must be independent of the selection.
- **Show Backdrop:** on-canvas Liquify already shows the other layers.

**Re-editable displacement (P2)**
- Needs a new raster plane or an image-valued effect parameter, plus a `.capy` version step.

**Bar**
- None while Liquify is a brush: its modes are brush presets in Tool Options, and the bar stays hidden under painting tools.
- If re-editable displacement becomes a session, that session gets a bar with Reconstruct All, Reset, Cancel and Apply, as Procreate's Liquify menu has.

#### XF-6 Non-destructive transform for paint layers (P2)

- Scale/Rotate on a paint layer with no selection edits `placement` instead of resampling pixels.
- Add an explicit **Apply Transform to Pixels** command.
- Needs GEO-0, so that the layer can still be painted beyond the canvas after being scaled down.
- The transform session and its bar are unchanged; only the commit target differs. Distort and Warp then follow the P-10 rules for placements.

#### XF-7 Perspective Warp and Puppet Warp (P3)

- Multi-plane perspective and pin-based deformation, after XF-1b and XF-3.
- Pins and planes are further modes of the transform session, with their own options on its bar.

### Selection and clipboard (SEL)

#### SEL-1 Pixel clipboard (P0)

**Unblocks** 14 and 26.

**Commands**
- Copy (Ctrl+C), Cut (Ctrl+X), Copy Merged (Ctrl+Shift+C).
- Paste, merged with the existing Paste Image on Ctrl+V.
- Paste in Place (Ctrl+Shift+V).
- Paste Into stays Later, per inventory line 348, unless it is deliberately raised.

**Decide the copy source first.** The inventory defines layer content as raw: alpha before opacity, masks, effects and clipping (`docs/ui/selection-command-inventory.md:254`). Copy variants mirror `RegionSource`:
- Copy = the Editing layer;
- Copy Merged = Visible;
- an optional Copy Reference = Reference.

**Capture**
- Reuse `flattened_document` with a new crop parameter.
- Represent each copy as a Rasterized `SourceImage`.
- Paste through `import_sources` or `place_layer_sources`. A pasted layer then carries a lossless placement, so Ctrl+T on it loses nothing.
- Use `import_layer_source` when no handles are wanted.
- Sources keep their own profile and depth, so pasting needs no conversion.

**Cut**
- `paint_operation` with a new erase kind. `Fill` composites source-over; the Figure erase mode is the precedent.

**System clipboard**
- `write_png(OutputEncoding)` with a crop.
- Image writers on all five hosts (P-3).
- The Web precedent for a private payload is `web <mime>` (`apps/layer-web/documents.js:63`).

**Restrictions**
- Disabled in Quick Mask and in mask editing.
- Keyboard focus in a text field wins over the shortcuts.

**Bar**
- Copy ▾ (Copy, Copy Merged, Cut) on the selection bar ([BAR-1](#bar-1-selection-p0)).
- Paste opens the placement context ([BAR-3](#bar-3-placement-and-paste-p0)). Paste in Place (T-11) opens no session and no bar.
- Without a selection there is no canvas object to anchor to. Paste stays in the Edit menu, command search and its shortcut. A touch route like Procreate's three-finger Copy & Paste menu belongs to the command framework's gesture stage, not to this bar.

#### SEL-2 Copy and Cut Selection to New Layer (P0)

**Unblocks** 21, 23, 26. **Quick win** within a single document.

**Spec**
- Already specified as Next in `docs/ui/selection-command-inventory.md:133, 282`.
- Adopt those names; list "Layer via Copy/Cut" in the shortcut descriptions.

**Implementation**
- `Duplicate` followed by Clear Outside on the copy (SEL-4), in one `Edit::Batch`.
- No readback is needed, and the full-resolution photo source is kept.
- Reselect works for free: `layer_edit` stores the cleared selection (`crates/layer-ui/src/art_layers.rs:714`).
- Insert with `Document::clipping_stack_top` (`crates/layer-core/src/layers.rs:730`), so the copy never becomes the base of the source's clipped layers.

**Commands and routes**
- Add a `DuplicateLayer` `CommandId` so Ctrl+J can duplicate the layer when nothing is selected.
- **Primary route:** Copy to Layer ▾, third on the selection bar ([BAR-1](#bar-1-selection-p0)). Also list the commands in the Layer › New submenu (`crates/layer-ui/src/art_layers.rs:1680`) and in the Selection Actions menu.

**Also**
- Add the GTK "Selection Actions…" button, which the inventory marks Core.
- Refuse Background, Group, Effect, Selection and the vestigial kinds.

#### SEL-3 Selection-aware Move (P0)

**Unblocks** 26.

**Behavior**
- With a selection, a Move drag runs a translation-only transform transaction (`crates/layer-ui/src/operation.rs:216`) committed by `commit_transform`.
- Alt-drag leaves a copy behind. This needs a copy flag on the transform operation, because `ImageTransform` always cuts.
- The Alt latch works as in the selection tools: it is read before the gesture (`docs/development/selection-tools.md:32`). Before-contact and during-drag modifiers are separate stage C contexts.
- **Visible toggle:** Leave Copy, on the selection bar while Move is active ([BAR-1](#bar-1-selection-p0)) and in Move's Tool Options.

**Notes**
- Capy's Move never picks a layer automatically, so GIMP's wrong-layer complaint does not apply here.
- Photo's default tool is Move, so call out the behavior change.
- Move currently ignores locked and Background layers without saying so (T-15).

#### SEL-4 Clear Selected and Clear Outside (P0)

**Unblocks** 1, 14. **Quick win.**

**Implementation**
- `paint_operation` with the erase kind from SEL-1.
- Clear Outside inverts the coverage.

**Bindings and naming**
- Bind Delete/Backspace, deciding precedence against `DeleteRuler` and focused header or tab items.
- Keep Clear Layer unbound and rename it "Clear Entire Layer". Line 359 of the inventory forbids reusing it for Clear Selected.

**Define**
- A mask target: `ClearMask` restricted to the selection.
- Behavior under alpha lock: today erasing is a no-op under it.

**Bar**
- Clear ▾ (Clear Selected, Clear Outside) on the selection bar. Delete then works without a keyboard too.

#### SEL-5 Feather, Border, Smooth, Transform Selection (P1)

**Unblocks** 11, 14. **Quick win.**

**Generalize** `ResizeDraft` into Grow, Shrink, Feather, Border and Smooth.
- **Canvas route:** a Refine ▾ popover from the selection bar, with one slider per operation and a live preview. The first draft extended the modal Grow/Shrink dialog that every host renders; that dialog becomes the menu route, fed by the same draft.
- Procreate's Feather slider on its selection toolbar is the precedent.
- **Feather:** a resize request with `resize: 0, feather: r`, capped at 100 px.
- **Border:** expand, then subtract the shrunken result from it.
- **Smooth:** needs `RegionRefinement` wired for `RegionSource::Selection` (`crates/layer-render-wgpu/src/region_requests.rs:87`).

**Transform Selection**
- The operation handles over `Selection::transformed`, as a separate transaction for coverage (inventory line 360).
- It appears as Transform Outline in Refine ▾, clearly separate from Transform, which moves selected pixels.
- It uses the transform bar (BAR-2) limited to Free and Uniform, because `Selection.affine` is metadata.

**Other**
- Live preview through the tonal draft/refine pattern.
- Targets include Quick Mask and Selection Layers (`docs/ui/saved-selections-assessment.md:81`).

#### SEL-6 Refine Edge (P1; the inventory has it as Later)

**Unblocks** 14, 15, 16.

**Reuse**
- **Controls:** feather, resize for Shift Edge, smoothing.
- **Outputs:** `Edit::SetSelection`, `MaskSelection`, `SelectionAction::NewLayer`.
- **View modes:** extend the selection-mask Grayscale and overlay settings.

**New**
- Matting within the uncertain band, using a guided filter or closed-form matting.
- Decontaminate Colors, written to a new layer.

**Where**
- The refinement types in `crates/layer-render/src/lib.rs:434` and the GPU shaders `region_refine.wgsl` and `selection_refine.wgsl`.

**Session and bar**
- Refine Edge is an on-canvas session opened from Refine ▾. Its bar holds View ▾, Edge Radius, Smooth, Feather, Shift Edge, Decontaminate, Output ▾ (Selection, Layer Mask, New Layer with Mask), Cancel and Apply.
- Tool Options holds the full form.
- It is not a separate workspace. Photoshop's Select and Mask takeover is a recurring complaint ([Adobe community](https://community.adobe.com/questions-712/hate-the-new-select-and-mask-tool-1117387)), and Photoshop on iPad already runs Refine Edge as a mode with Done/Cancel.

#### SEL-7 Edge-aware quick selection (P1; Later)

- Reuse the painted-selection brush pipeline.
- Segmentation (superpixels or graph cut) on a downsampled pyramid.
- Its bar is the paint-selection context of BAR-1, at the bottom edge. Add and Subtract stay in Tool Options.

#### SEL-8 Channel selections and Color to Alpha (P1; Later)

**Channel selections**
- A new `RegionSource` variant beside `Tonal` and `Coverage` (`crates/layer-render/src/lib.rs:350`).

**Color to Alpha**
- A pointwise effect with `alpha: filter` and a Color parameter. It needs only a manifest entry and WGSL.

**Select Similar**
- P2 here only; remove it from T-10.

#### SEL-9 Select Subject and Select Sky (P2, research)

**Constraints**
- Only on-device models (README "editing happens locally").
- Bundle size on Web and Android.
- Model license.
- WebGPU inference cost.

**Bar**
- Select Subject is the Adobe bar's lead action on a layer with no selection. It becomes a candidate for [BAR-8](#bar-8-layer-without-a-selection-p2-decide-later) only if this item ships.

### Retouching (RET)

#### RET-1 Retouch source: reference layers (P0)

**Unblocks** 20–23.

**Decision**
- Retouch tools sample from **reference layers**, the mechanism Wand and Fill use.
- Source choices are the same as for region tools: Reference layers (default for retouch), Editing layer, Visible.
- Reuse `ToolActionGroup::SelectionSource` (`crates/layer-ui/src/tool_settings.rs:16`), and unify the source labels, which differ today between Fill and Wand.

**The retouch flow**
1. Mark the photo, or a group, as a reference.
2. Add an empty layer.
3. Retouch on that layer.

**Rules**
- A marked layer contributes its subtree and its clipped effects. Unclipped adjustment layers above it do not contribute, which matches Photoshop's "ignore adjustment layers".
- Only Paint, ImportedImage and Group layers can be references. Paper never contributes, and hidden references contribute nothing.
- **Include the target layer in the Reference source,** in stack order, so later strokes can see earlier fixes, as Photoshop's "Current & Below" does. This differs from Wand and Fill; record it in the tool description.
- If no references are marked, show the existing message and add a one-tap action that marks the nearest visible photo or paint layer below the target. Reuse `ReferenceSelection`.
  - Present it as a P-4 notice carrying that action, not as a frame error (T-23).
  - The same notice serves Wand and Fill. Their only fix today is the lighthouse button or the layer row's Layer Settings submenu (`crates/layer-ui/src/art_layers.rs:1652`).
- Refuse retouch tools on masks: mask strokes silently become Dry (`crates/layer-engine/src/canvas.rs:1308`).

**Snapshot (P-2)**
- All three sources read a stroke-start copy-on-write snapshot of every page the stroke reads or writes.
- The renderer keeps no stroke-start pages today, and uploads are forbidden during contact (`docs/reference/gpu-brush-engine.md:273`).
- The snapshot must survive until pending estimated-sample corrections have been replayed (`crates/layer-engine/src/corrections.rs:101`).

**Capture**
- `artwork::Capture::region`:
  - windows up to 256²;
  - one reusable target, so the cache needs its own pages;
  - a 256 MiB limit;
  - it must use a packet without predicted-preview batches.
- Prefetch tiles around the source when the source is set and at pen-down. Cap captures per frame to stay within the 8.33 ms p99 budget. The tonal cache (`crates/layer-render-wgpu/src/region_sources.rs:39`) is the precedent.

**Precision and coordinates**
- The capture target uses `working_format()`, which is SRGB8 unless the device supports native Float32. Float is preserved only there.
- Store the source offset in document space. Map Editing sources through `DabStyle.brush_to_layer`.

#### RET-2 Clone Stamp (P0)

**Unblocks** 21, 22, 23.

**Rendering**
- A new `BrushExecution::Clone` on the **dry-deposit path** with per-pixel pigment, as bristles do (`crates/layer-render-wgpu/src/material_brush.wgsl:675`), and `BrushAccumulation::Uniform`. It is not on the Smudge path, whose ordered backtrace is unnecessary for an immutable source.
- Add a Clone case to `sample_bounds` and to the gather (`crates/layer-render-wgpu/src/material_sources.rs:83, 324`).
- The source texture must reuse an existing binding slot, because the pass is at WebGPU's 16-texture limit.
- Update `BrushPassPlan`.

**Tool family**
- New `Tool`, `ToolGroup` and `ToolFamily` variants, with `is_drawing`/`is_sculpt` updated.
- A retouch slot in `WorkspaceToolMemory`, with a migration, because of `deny_unknown_fields`.
- Preset IDs of 36 or higher. `origin/main` ends at 34 (`BrushedInk`), and the bristle-brush branch claims 35.
- A behavior `ToolCategory` for the command catalog, probably a new Retouching category.
- Drawer projections on each host.
- Place it as a Retouch set in the Sculpting panel, next to Blend and Liquify, or as a new set panel.

**Setting the source**
- Alt/Option-click on desktop. The shortcut audit maps Alt-hold to temporary sampling for painting tools, so Clone's Alt-click is a tool-specific exception in the stage C resolver.
- A **Set source** button that arms the next pen or mouse tap. A finger tap navigates, as for every canvas tool except placement handles. The button is in Tool Options and on the disc's bar.
- An on-canvas source disc that drags immediately with every device, using the transform-handle pattern: `crates/layer-ui/src/operation.rs:470`, `ruler_reach`, `placement_touch_hit`, `CursorSegment`.
  - Tapping the disc shows its canvas action bar ([BAR-6](#bar-6-small-canvas-objects-p1p2)) with the options below.
  - The bar hides while painting.
- A hold cannot be used, because a touch hold opens the color picker.
- Barrel buttons need P-5 (command-framework stage D), and remapping one removes its Pan action.

**Options**
- Aligned, a checkable action.
- Source, a `ToolOption::Choice`.
- Overlay opacity, clipped to the brush. The pixel overlay is new renderer work.
- Rotation, scale and flip as numeric settings owned by the session.
- Darken and Lighten must compare against the RET-1 source at the destination. The existing brush Darken/Lighten compare against the target layer, which is empty.

**Replay**
- Store the offset, transform, the Aligned flag and the source kind in `Stroke` (`crates/layer-core/src/lib.rs:1206`), not in `BrushSnapshot`.

**Messages**
- Alpha lock on an empty layer makes Clone a silent no-op; report it (T-15).

**Cross-document cloning (P2)**
- Parked documents are not resident on the GPU.

**Performance**
- Add Clone to the acceptance matrix (`docs/history/advanced-brush-engine.md:241`) and to the benchmarks.

**Evidence**
- [Clone Source panel](https://helpx.adobe.com/photoshop/desktop/repair-retouch/heal-clone/clone-source-panel.html)
- [Kost tips](https://jkost.com/blog/2021/12/10-tips-for-the-clone-stamp-and-healing-brush-tools-in-photoshop.html)
- [GIMP clone](https://docs.gimp.org/3.0/en/gimp-tool-clone.html)
- [Procreate clone disc](https://help.procreate.com/procreate/handbook/adjustments/adjustments-clone)
- [Affinity sources](https://affinity.help/photo2/English.lproj/pages/Retouching/retouching_cloningHealing.html)

#### RET-3 Healing brush (P0)

**Unblocks** 20, 21, 23.

**Algorithm**
- A Poisson or membrane blend.
- No solver exists yet. Reuse the Float32 pyramid kernels in `local_tone.wgsl` (downsample, expand, accumulate), not the display mipmaps.
- Take the boundary tone from the RET-1 source sampled at the destination, because the target is empty.

**Execution**
- An asynchronous job (P-11) with a coarse live preview.
- It completes after pen-up; the precedent for pen-up work is under 8.33 ms.
- Fixed iteration counts keep correction replay deterministic.
- Hooks: `DabBatch.stroke_end` and the stroke-edge pass.
- Bounded windows follow the `scene_images` pattern, because the 256² gather fields are too small.

**Acceptance**
- A GPU oracle test in which texture is preserved while tone matches its surroundings. Without it, heal "looks like cloning" (GIMP issue #16124).

#### RET-4 Spot Healing brush (P0)

**Unblocks** 20, 21.

- No source is set. Search for similar patches in a ring around the stroke, then apply the RET-3 blend with the same boundary rule.
- Types: Content-Aware, Proximity, Create Texture.

#### RET-5 Content-aware fill and Remove (P1)

**Unblocks** 2 (corner fill) and 21.

**Algorithm**
- Multiscale PatchMatch on the GPU (P-11).
- Reuse `RegionRequest` generations and `SuggestionRequest::is_stale_for` to discard stale results.

**Inputs and outputs**
- The sampling area uses the painted selection or Quick Mask.
- Output goes to a **Paint** layer. Do not output to `LayerKind::AiSuggestion`: it is asset-backed, has no inference backend, and brushes refuse to paint on it.

**Remove brush**
- Paint a stroke; it is filled on release.

**Bar**
- Fill ▾ → Content-Aware Fill on the selection bar ([BAR-1](#bar-1-selection-p0)).
- While the job runs, the region's bar shows progress and Cancel ([BAR-7](#bar-7-region-jobs-p1-with-p-11)).
- The Remove brush is a painting tool and has no bar.

#### RET-6 Patch tool and content-aware move (P2)

- Reuse `LayerOperationKind::Transform` with coverage, then apply the RET-3 blend.
- Start it from the selection bar. It then uses a transform-session bar with Adapt ▾, Cancel and Apply.

#### RET-7 Dodge and burn (P1 command, P2 tools)

**Unblocks** 24.

**New Neutral Layer (P1)**
- Creates a Soft Light or Overlay layer filled with the neutral value.
- In today's linear blending, neutral is 0.5 linear (about 73.5% sRGB), not Photoshop's 128, and the tonal response differs.
- Ship this after the P-9 decision, so that it can match Photoshop.

**Dodge/Burn/Sponge brushes (P2)**
- New `blend_color` codes that need destination reads, which puts them off the dry compute path.
- They change only the target's pixels, so they do nothing on an empty layer.

#### RET-8 Blur and sharpen brushes (P2)

- A Smudge preset with pull 0 and blur 1 is already a blur brush up to 4 px, but blur has no UI.
- Sharpening needs RET-1's source.
- The route that works today is an effect layer with a painted mask.

#### RET-9 History brush and "revert to original photo" (P2; revert is a quick win)

**Revert to original photo**
- Placed photos keep an immutable original (`crates/layer-core/src/lib.rs:184`), so painting back toward it needs no snapshot.

**History brush**
- Snapshots need their own retention and accounting (`crates/layer-core/src/history_budget.rs:16`), because history is capped at 256 entries and 512 MiB and is evicted.
- They need GPU residency through `Capture::source_tile` and `SOURCE_SLOTS`, which share P-2.

#### RET-10 Red-eye (P3)

- Reuse the Wand region inside a clicked area plus a Hue/Saturation effect, instead of new detection.
- The click leaves a selection, so the selection bar's Adjust ▾ is the route to that effect.

### Layers and compositing (LYR)

#### LYR-1 Blend modes, blend domain and pass-through (P0)

**Unblocks** 17, 23, 28.

**Modes**
- Add Darken, Lighten, Color/Linear Burn, Color Dodge, Hard/Vivid/Linear/Pin Light, Hard Mix, Difference, Exclusion, Subtract, Divide, Hue, Saturation and Luminosity to the single `blend()` switch. That one change covers layer, effect, fused and cached composites.
- Merge the brush `blend_color` implementation into one shared include. Today the numbering and the Overlay tie-break differ.

**Domain (P-9)**
- Modes compute in linear document RGB. Photoshop blends 8/16-bit files in encoded values, so contrast modes will not match it.
- **Decide** between a per-mode domain and a document option to blend in encoded RGB. The helpers `fx_encode` and `fx_decode` already exist.

**HDR**
- Add and Color clamp today. Screen is non-monotonic above 1.
- Color uses Rec.601 luma weights. Reuse `fx_luma` and `fx_hsl` for the Hue, Saturation, Color and Luminosity modes.
- This conflicts with the documented "deliberate bounds" (`docs/internals/rendering.md:52`); settle that record.

**UI**
- The mode index doubles as the enum discriminant.
- Add an explicit mapping from menu order to code, and grouped menus on every host (`layer_blends`, `crates/layer-ui/src/lib.rs:517`).
- Benchmark: only Normal has a fast path.

**Pass Through**
- This supersedes `docs/history/layers-initial-design.md:265`. New groups default to Pass Through; existing documents deserialize as Isolated.
- It is structural. Update `Scene::group` scratch surfaces, the parent-only input capture in `scene_images`, `input_indices`, the checkpoint search and the sibling rule in `reference_snapshot`.
- Specify how group opacity and masks apply.
- Relax the Ungroup restrictions.
- Update `rendering.md` and `documents.md`.
- Extend the `cached_clipping_matches_tiled_composition` oracle test.

#### LYR-2 Merge, flatten, stamp and apply effect (P0)

**Unblocks** 23, and simplifies most journeys.

**Semantics** follow `docs/history/layers-initial-design.md:269`:
- Normal merges;
- baking of a whole clipping stack (baking one clipped effect is not appearance-preserving when other clips sit between it and the base);
- flattening an isolated group when the result can be represented.

**Implementation**
- Reuse `LayerOperationKind` and `Scene::apply_operation`, which already settles wet paint.
- **Stamp Visible:** copy `Scene::group(None)` output into new pages, excluding the root background clear.
- **Flatten Image:** reuse the snapshot flatten as a worker job. It needs a progress and cancel owner, which is still missing (`docs/internals/rendering.md:183`).

**Constraints**
- Paper cannot receive content, so Merge Down onto Paper is disabled.
- Clipping bases must be Paint or ImportedImage.
- References are dropped on removal; transfer them as Ungroup does.
- Budgets: 512 MiB of history, 1 GiB per publication, and `RasterRevision::pending` reservation. A full Float32 bake can exceed them.
- Rename the plan's "rasterize" rule to "resample a placed source into document pixels", to avoid confusion with Rasterize Source.
- Find a Merge Visible chord that does not collide with Export.

**Bug to fix**
- Apply Mask on a group tells the user to "flatten the group first", but no such command exists (`crates/layer-ui/src/art_layers.rs:1312`).

#### LYR-3 Blend If / Blend Ranges (P1; audit-deferred)

**Unblocks** 12, 15, 16, 17.

**Reuse**
- The `TonalBand` lower/upper/falloff math (`crates/layer-core/src/tonal.rs:10`, `crates/layer-render-wgpu/src/tonal.wgsl:12`). It works in stops of linear luminance.
- Define the mapping from Photoshop's encoded 0–255 scale.

**Composite paths**
- Four paths need the change: `scene.wgsl` op 4, the fused and folded effect outputs, the cached `ImageComposition`, and `portable_blend.wgsl`.
- Layers with an "underlying" range must leave the hardware Normal fast path.

**Cache invalidation**
- Automatic, because scene metadata compares whole layers.

**Record**
- Supersedes the deferral in `docs/history/layers-context-menu-audit.md:35`.

#### LYR-4 Mask properties (P1)

**Unblocks** 11.

**Density**
- A pointwise change in three places: `mask_tile`, `scene.wgsl`, and `effect_properties`.

**Feather**
- A live feather needs a `scene_images` stage with a declared halo.
- A destructive feather is a quick win: load the mask as a selection with feather (`LoadCoverage` hard-codes 0 today), then replace the mask.

**Viewing the mask**
- Keep the tinted overlay (decision above), but make its color and opacity configurable. Add a Grayscale mode by reusing `SelectionPaintBehavior`. Today the tint is fixed purple at 0.42.

**Out of scope**
- Mask opacity is deliberately absent (`docs/history/layers-initial-design.md:247`).

**Bar**
- Mask editing is a bar mode ([BAR-5](#bar-5-modes-without-an-object-p1)): "Editing *layer* mask", Show Mask ▾ (tint or grayscale), Invert, Density/Feather, Disable, Apply Mask and Edit Content.
- It needs `CommandId`s to enter and leave mask editing; today only layer rows do it.

#### LYR-5 Fill layers (P1 for Solid and Gradient, a quick win; P2 for Pattern)

**Solid Color and Gradient**
- Manifest and WGSL generators only. The renderer, Color/Gradient parameters and stop editors already exist.

**Pattern**
- Needs an image parameter, which is an ABI change.

**Limitations to state**
- Painting passes through an unmasked effect layer.
- Layers cannot be clipped to fill layers.
- Effect layers cannot be references.

#### LYR-6 Align, distribute and auto-align (P2)

**Unblocks** 28.

- Needs pixel-tight bounds (P-8).
- `move_target_edit` moves a single layer only.
- Rulers can serve as alignment targets.
- **Auto-Align:** homography estimation that writes P-10 geometry.
- **Bar:** Align ▾ and Distribute ▾ on the multi-layer transform bar (XF-4), and on the layer bar if [BAR-8](#bar-8-layer-without-a-selection-p2-decide-later) ships.

#### LYR-7 Stack modes (P2)

**Unblocks** 28.

- Median, Mean, Minimum and Maximum over several layers.
- ABI 3 effects read only the composite below, so this needs either a dedicated renderer operation or a multi-input ABI extension.

#### LYR-8 Layer styles (P3)

- Stroke, Drop Shadow and Glow.
- Effects cannot draw outside their base, because clipping keeps the base's alpha. This needs a new attachment kind.

#### LYR-9 Panorama and focus merge (P3)

- Builds on LYR-6 and LYR-7.
- Blend masks stay editable until the merge is committed.

### Adjustments and filters (ADJ)

#### ADJ-1 Pickers and targeted Curves (P0 for Curves and White Balance; pair Levels with ADJ-4)

**Unblocks** 5, 6.

**Quick win**
- Set `color_action` for every effect Color parameter: Split Tone, B&W tint, Pencil, Halftone.
- This is a Rust-only change, because all five hosts already draw it.

**Pickers**
- Reuse the `color_picker_session` lifecycle: previous-tool restore, touch-hold loupe, Escape.
- Route to a new destination beside the mask and current-color destinations (`crates/layer-ui/src/session.rs:3881`).

**Source**
- "The composite below the effect" is not a `ColorSampleSource` today.
- Build it from `RegionSource::Layers` or from the filter-preview composition (`crates/layer-ui/src/effects.rs:56`).
- Choose the averaging domain deliberately for black and white points.

**Targeted Curves**
- `EffectAction::CurvePoint` inside a `Gesture`, so the adjustment is one undo step.

**Bar**
- An armed picker and targeted Curves are bar modes ([BAR-5](#bar-5-modes-without-an-object-p1)).
  - An armed picker shows a prompt such as "Click a neutral point", Sample Size ▾ and Cancel.
  - Targeted Curves shows "Drag on the image" and Done.
- The pickers themselves stay in Properties.
- Pen and mouse sample with a tap; a finger uses the existing touch-hold loupe.

**Model (P-7)**
- An optional picker field on `EffectParameter`.
- Not a new `EffectParameterKind`, which older builds cannot read.

**Related**
- The Levels gray point needs per-channel Levels (ADJ-4).
- The White Balance neutral picker can be solved in closed form, but widen its ±0.8 EV gain range.

#### ADJ-2 Hue/Saturation by range (P0)

**Unblocks** 13.

**Reuse**
- B&W's six-sector hue weights, Vibrance's skin-hue distance, and the extended HSL domain.

**Hue space**
- Compute hue in Oklab (`working_to_oklab`) so that ranges do not shift in ProPhoto.

**Controls**
- About 49 parameters, within the 64-parameter limit.
- Needs the paged Properties UI (P-6).
- Colorize already exists as B&W Tint; offer it here as a convenience.

**Compatibility**
- Documents embed their programs. Upgrade them explicitly through `rebind`.

#### ADJ-3 Histogram in editors, a Histogram panel, clipping preview (P1)

**Unblocks** 5. This absorbs T-5.

**Computation**
- Replace the full-resolution CPU readback with the GPU histogram bins from `tonal.wgsl` and portable reductions.
- Offer scopes from `RegionSource`: Visible, Editing, Reference, Selection.
- Update the histogram during gestures, not only on committed revisions.

**UI**
- Levels has no custom editor, and no Curves editor draws a histogram on any host. Both need host UI on all five platforms.
- Five native histogram windows already exist. Move them into a `Panel`; Stats, Navigator and Proof are the precedents.

**Clipping preview**
- The precedents are the gamut-warning overlay and the live tonal band mask.

#### ADJ-4 Per-channel Levels and Auto (P1)

**Unblocks** 5, 6.

**Per-channel Levels**
- Levels `white` is capped at 1. Widen it for float documents, as Curves and Exposure already are.

**Auto**
- Percentiles need GPU statistics; the design is in `non_destructive_filters_wgsl_shader_subsystem.md` §15.
- Lesson: the SDR Proof "Auto" was built and then removed (`docs/history/color-management-proof-dial.md:26`).

#### ADJ-5 Missing pointwise adjustments (P1)

**Unblocks** 8, 13.

**Invert, Threshold (luminance), Desaturate and Photo Filter**
- Manifest and WGSL only, or presets of existing filters once ADJ-10 exists.

**Selective Color**
- Reuse the B&W sectors and the Color Balance tonal weights.

**Channel Mixer**
- Also needed by ADJ-12.

**Color Lookup**
- Reuse `ProofLut` tetrahedral sampling and the `profile_library` pattern.
- Effect lookups are computed from parameters only, with at most 4,096 records, so a 33³ LUT cannot be uploaded that way.
- Effect shaders cannot add bindings.
- Choose between an inline table (as Curves does) and a new binding with an ABI change that keeps reading ABI 3.

**Tests**
- New filters need independent oracle tests. Also update the catalog count assertions of 40 and the old pixel-reference fixture.

#### ADJ-6 Shadows/Highlights, Clarity, Dehaze (P1)

**Unblocks** 7.

**Shadows/Highlights and Clarity**
- Build on the SDR rendition's local-Laplacian Tone × Detail guide (`crates/layer-core/src/color/hdr/local.rs`), exposed as an effect.
- It does not depend on ADJ-7.
- Its constraints: it is document-wide today, has a guide of at most 768 px, is rebuilt after idle, and is not yet qualified on Metal, D3D12 or Web.

**Dehaze**
- Needs image statistics.
- Effect lookups cannot read the image.
- Document-sampled filters need their whole input. 256 MiB is now the minimum filter reserve: documents that fit in the shared composition allowance keep full inputs, and windowed evaluation is the fallback (`crates/layer-render-wgpu/src/scene/windows.rs:8`). Windowed evaluation cannot serve a document-sampled filter.

#### ADJ-7 Large-radius, lens and tilt-shift blurs (P1)

**Unblocks** 19.

**Current caps**
- Manifest σ ≤ 21.
- Kernel truncation at 63 px.
- 33 lookup records.

**Moderate radii (quick win)**
- Fit within existing lookup limits (up to 4,096 records and 256 lanes) at linear cost per pixel.

**Large radii**
- Use pyramids: reuse `local_tone` downsample and expand, or the dual filter in `backdrop_blur`.
- Reduced-resolution intermediates were designed (§13 of the shader-subsystem record) but never built.
- Halos count against the shared composition allowance. When a document falls back to windows, large halos make each window larger (`crates/layer-render-wgpu/src/scene/windows.rs`).

**Lens blur**
- Depth from a mask is impossible today: an effect's mask is its coverage, and effects have no auxiliary inputs (§18).
- On-canvas pins do not exist.
- Check licenses for bokeh references.

**Merge with T-8**
- Replace Denoise with **Surface Blur** (radius plus threshold) here.

#### ADJ-8 Noise reduction, Median, Dust & Scratches, Smart Sharpen (P1/P2)

**Unblocks** 10, 20.

- **Noise reduction:** split luminance from chroma through Oklab, and use non-local means or wavelets.
- **Median:** no median filter exists anywhere yet.
- **Dust & Scratches:** a median with a threshold.
- **Smart Sharpen:** extend Unsharp Mask's threshold gate.

#### ADJ-9 Frequency Separation command (P1)

**Unblocks** 23.

**Reuse**
- High Pass computes `0.5 + (enc(orig) − enc(blur))·amount` with the Gaussian taps. At amount 50% it produces the high layer.

**Linear Light**
- The Linear Light used to recombine the layers must compute in the **encoded** domain (P-9).

**Accuracy**
- 8-bit loses one code value, so state a tolerance.

**Depends on** LYR-1 and LYR-2, so that the high layer becomes editable pixels.

#### ADJ-10 Presets and copying effects (P1)

**Unblocks** 8, 9.

**Reuse**
- Per-filter preview presets (`crates/layer-core/src/effect_catalog.rs:43`).
- `EffectInstance::rebind`.
- Filter-preview thumbnails for up to eight instances.

**Cross-document copy**
- Needs P-3.
- Encoded values differ between working spaces: pasting Curves from ProPhoto into sRGB changes the result.
- `rebind` resets values from ranges widened for float documents.
- Resolve catalog-ID conflicts for pasted embedded programs.

**Storage**
- Use a new file rather than extending `Settings`, which uses `deny_unknown_fields`.

#### ADJ-11 Lens corrections (P2; vignette removal is a quick win)

- **Vignette removal:** allow negative Vignette strength; the manifest minimum is 0.
- **Distortion:** CRT's one-coefficient barrel is a prototype.
- **Chromatic aberration removal:** needs a new radial model.

#### ADJ-12 Match Color (P2)

**Unblocks** 9, 16, 17.

- A mean/covariance transfer produces a 3×3 matrix plus an offset. It therefore needs Channel Mixer (ADJ-5); Curves or Color Balance cannot express it.
- The source comes from reference layers or a selection.

### View and measurement (VIEW)

#### VIEW-1 Actual Pixels and zoom entry (P0; quick win)

**Unblocks** 10, 30.

**Why it is needed**
- Zoom In/Out multiplies the current zoom by √2, and wheel and pinch zoom are continuous, so the view practically never lands exactly on 100%.

**Command**
- Add `CommandId::ActualPixels` on Ctrl+1. Its label must differ from the placement command "Original Size (100%)".
- Zoom 1.0 is already true 1:1 on HiDPI screens.

**Controls**
- Make the `CanvasInfoLayout` readout a shared interactive control: typed percent via `NumericControl`, presets, and Fit.
- Share the navigator buttons, which are currently coded separately on each host.
- **Zoom to Selection:** generalize `Camera::fit` using `Selection::bounds`. Decide whether it keeps the view rotation (Fit resets it).

#### VIEW-2 Before and after (P1)

**Proof and SDR comparison**
- A region split in the present shader.

**Effects-off comparison**
- A render override that changes only the view: no history, and not Solo.
- Bind it to a new momentary shortcut (precedent: `ShortcutAction::Pan`) plus a touch control.

**Labels**
- Reuse the Before/After labels from the GTK comparison previews.

**Bar**
- The touch control is a bar mode ([BAR-5](#bar-5-modes-without-an-object-p1)): "Before / After", Split ▾ and Exit.
- The split divider is an on-canvas handle that drags immediately.

#### VIEW-3 Info panel and color samplers (P1)

**Unblocks** 5, 6.

**Reuse**
- `ColorSampleRequest`: point, average and circle samples, returning straight linear document RGB before the view transform.
- `ColorReadout` in OKLCH, HSB, HLS and RGB. Use OKLCH, the app's vocabulary.
- Visibility-gated sampling, as `sync_renderer_telemetry` does.

**New**
- Batched multi-point requests (today only one request is in flight at a time).
- A pre-edit sample source for before/after values.

**Bar**
- Tapping a sampler shows its bar ([BAR-6](#bar-6-small-canvas-objects-p1p2)): Delete, Sample Size ▾ and Readout ▾.
- Samplers drag immediately.

#### VIEW-4 Guides, grid and overlays (P2)

**Guides**
- Add horizontal and vertical guides as a `RulerGeometry` variant. Storage, undo, overlay, hit testing and Move editing already exist.
- Generalize snapping from brush strokes to crop, transform and selection handles.

**Naming and shortcuts**
- Rename "Show rulers" to "Show drawing guides" before adding edge rulers.
- Ctrl+R is reserved by the browser on Web.

**Grid**
- No grid code exists yet.

**Bar**
- A selected guide shows its bar ([BAR-6](#bar-6-small-canvas-objects-p1p2)): Delete Guide, Snap, Straighten Image to Guide and Hide Guides.
- Tool Options then stops showing ruler actions under unrelated tools, which it does today whenever any ruler exists.

#### VIEW-5 History panel (P2)

- Reuse the workspace history presentation pattern.
- Add a label to each `HistoryEntry`; entries have none today.
- Show that history is trimmed to 256 entries or 512 MiB.
- "Jump to a step" is sequential undo or redo, subject to pending-frame gating.

### Files and delivery (IO)

#### IO-1 Metadata (P0)

**Unblocks** 29.

**Policy**
- Keep the current rule (no stale EXIF).
- Copy only chosen fields: camera, lens, exposure, date, copyright, contact.
- Regenerate dimensions and orientation.
- Offer All, Copyright & Contact, or None, with a separate Remove Location option.

**Reuse**
- The single shared EXIF parser.
- The JPEG APPn preflight (add EXIF, XMP and APP13/IPTC).
- `quick-xml`.
- `SourceImage`.
- `DocumentInfo::describe`, which GTK duplicates.

**Constraints**
- Store metadata as binary payloads, with a 64 MiB limit.
- Projects exclude source filenames, so XMP needs a policy for embedded paths.
- Merge user XMP into the HDR JPEG's own XMP packet.
- Decide the rule for documents with several sources.
- The `.capy` version step must keep reading v6, v7 and recovery files.

#### IO-2 WebP and SDR AVIF export (P1; lossless WebP is a quick win)

**Unblocks** 29.

**Lossless WebP**
- The vendored image-webp VP8L encoder, with ICC, EXIF and XMP chunks.
- Needs a vendor audit, encode-memory admission (`PhotoMemoryBudget.encode_bytes`) and cancellation.

**Lossy WebP**
- Needs a new pure-Rust encoder.

**SDR AVIF**
- The mux requires a gain map and CICP, and uses a fixed 12-bit BT.2020 base. Make the gain map optional and add ICC `colr`.
- Latency is high on large photos and tablets.

#### IO-3 Export sizing, output sharpening, quick re-export (P1)

**Unblocks** 29.

**Already in place**
- The last recipe for each destination and the last folder are remembered.
- Original and Fit sizes exist.

**New**
- Long edge, short edge, percent and megapixels.
- Sharpening as a bounded row-window stage in the CPU resampler, feeding both HDR renditions.
- A file-size estimate. None exists; previews are 220×160 and JPEG artifacts are not previewed.
- A shared `ExportDraftAction` for Size and Resolution; hosts build the recipe JSON by hand today.

**Compatibility**
- New `ExportSize` variants change the `CAPYPRESETS\x01` preset file and the hand-built host JSON.

**Limits**
- Web cannot overwrite a file through the download fallback.

#### IO-4 Batch processing (P2)

**Unblocks** 9.

**Reuse**
- `read_import` and `PhotoOpenPolicy`, `DecodeLimits`, snapshot `CaptureControl` cancellation with progress, and `ExportPresets`.

**Blockers**
- Failures must be handled per file. `ImageImportBatch` is all-or-nothing; the precedent is Web/Android batch Open.
- The "Ask" missing-profile policy needs a non-interactive rule.
- No host has a folder picker.
- ADJ-10 and LUTs must exist first.

#### IO-5 Export layers and selection (P2)

- **Export layers:** export a project copy with visibility changed, reusing Solo's subtree logic but not its history edit.
- **Export selection:** needs output cropping. Its canvas route is the selection bar's More menu.
- Both need a folder picker.

#### IO-6 RAW hand-off and New Document from Files (P2)

**RAW hand-off**
- Already specified as 16-bit TIFF (`docs/ui/color-management.md:392`).
- File associations exist on Linux, Android, Web and macOS; add them on Windows.
- Save never adopts a photo's location, so a round trip means Export over the handed-off TIFF.

**New Document from Files**
- Import already loads several files as layers in one undo step.
- Add "New Document from Files", using native scale rather than Fit, for stacking.
- Add multi-file Open on GTK.

## 6. Tweaks to existing tools

| ID | Change | Why | Where / notes |
| --- | --- | --- | --- |
| T-1 | A new effect layer takes the active selection as its mask and consumes the selection, as Add Mask does. | Journey 11. Industry default. Brushes then route to the mask automatically. | Insert handler `crates/layer-ui/src/effects.rs:744`; reuse `LayerAction::AddMask` (`crates/layer-ui/src/art_layers.rs:1271`). Quick win. Canvas route: Adjust ▾ on the selection bar (BAR-1). |
| T-2 | Bind Delete/Backspace to Clear Selected Pixels (SEL-4). Rename Clear Layer "Clear Entire Layer", leave it unbound, and state that it discards a placed source. | Every editor does this; Delete is unbound today. | `CommandId::ClearLayer`, `crates/layer-ui/src/shortcuts.rs:324`. |
| T-3 | Let effect Number parameters declare a slider mapping (`Power`) and soft bounds. Raise σ for the six Gaussian-based filters. | Fine control at small σ, reach at large σ. | Optional `EffectParameter` fields (P-7); `NumericMapping` in `crates/layer-ui/src/numeric.rs:14`. There is no mechanism yet for defaults relative to document size. |
| T-4 | Expose Twirl CCW, Pinch and Expand after a visual check; expose Crystals together with distortion and momentum. Reconstruct is not included. | The engine already implements these modes. | `crates/layer-ui/src/tools.rs:178` presets and the count test at `:747` (34 on `origin/main`); new IDs ≥ 36, because the bristle-brush branch claims 35; `crates/layer-core/src/presets.rs:650`. Quick win. |
| T-7 | Numeric entry for Curves points, showing EV in Log HDR mode. Arrow-key point nudging on every host, not only Windows. | Precision. | `EffectAction::CurvePoint` already accepts exact coordinates; host UI only. |
| T-8 | Rename the Denoise label (keep the id `denoise`), then replace it with Surface Blur under ADJ-7. Reserve "Noise Reduction" for ADJ-8. | The name overpromises. | Documents keep the embedded label. |
| T-10 | Live tolerance preview for Wand and Select by Color, re-run from a saved baseline and amended into one undo step. | Over-selection complaints. | The tonal draft/refine pattern (`crates/layer-ui/src/tonal_selection.rs:232`, `engine.refine_selection`). Tolerance is shared with Fill. The slider leads the selection bar while the last result can be refined (BAR-1). |
| T-11 | In-app copies paste in place without handles; external images keep them. | Removes a surprise step. | Changes `docs/ui/image-open-import-proposal.md:80`. Needs a clipboard ownership marker or the window-level clipboard (P-3). |
| T-12 | (P2) Add a source setting to Smudge and Natural Blender, which both use the Smudge execution. | Blend on an empty layer. | Needs a live "target over reference" source, the opposite of RET-1's snapshot. Specify it separately. |
| T-14 | Gradient tool: multi-stop stops using the existing `GradientStop` editors, a reflected type, and dithering. | Graduated masks, fill layers, 8-bit banding. | Update both the layer `Gradient` operation and `SelectionGradient`. Choose one interpolation domain: the tool mixes linear premultiplied values while Gradient Map mixes encoded straight values. No dithering exists anywhere. |
| T-15 | Show every refusal. | Hidden state leads GIMP's question lists. | See the notes below the table. Disabled bar items show the same reason on tap or hover. |
| T-16 | Add new photo tools and panels to the Photo default. | Keeps the Photo workspace a photo workspace. | Keep the prior layout as `legacy_*_layout` with a migration entry (`crates/layer-ui/src/layout_presets.rs:143`). Photo's Diagnostics slot is the natural home for Histogram and Info. The canvas action bar is on in Photo (BAR-0 decision 5). |
| T-17 | Move the print-size summary and the "ICC embedded" note into the shared `ExportDraft`. | Web and Android lack them; ICC is always embedded. | GTK `apps/layer-linux/src/files/export.rs:293`. |
| T-18 | "New Document from Files" (IO-6) and multi-file Open on GTK. | Stacking starts here. | Import already handles multiple files. |
| T-19 | Expose the 8 brush blend modes as a brush setting. | Glazing, dodge-like painting. | Needs enumerated tool settings (P-5). |
| T-20 | Add a GTK "Selection Actions…" button. | The inventory marks it Core, and the other four hosts have it. | The selection menu model (`crates/layer-ui/src/selection_masks.rs:226`). The selection bar's More opens the same menu on every host. |
| T-21 | Make the layer-mask overlay color and opacity configurable. | Visibility against different images. | Fixed tint in `crates/layer-render-wgpu/src/scene.wgsl:205`; reuse the selection-mask settings. |
| T-22 | Add a Reference source to the Eyedropper. | Consistency with RET-1 and ADJ-1. | `ColorSampleSource` has only Composite and Layer. |
| T-23 | Show "Mark a reference layer" as a P-4 notice with a one-tap Use *layer* as Reference action, instead of a frame error. | It must reach the user on every host, and today the fix is only in a row submenu. | `crates/layer-ui/src/region_tools.rs:176`; the action is the one RET-1 specifies. |
| T-24 | Keep an active transform, placement or crop when the window loses focus; cancel only the contact in progress. | Switching windows or opening a host dialog discards a transform today, and no bar interaction may do so. | `UiInput::Blur` calls `cancel_layer_gesture` (`crates/layer-ui/src/session.rs:1184`, `crates/layer-ui/src/art_layers.rs:1867`). Decision in M0. |
| T-25 | Escape leaves Selection Layer editing, as it leaves Quick Mask. | Escape is the only keyboard exit users try, and the bar's Return to Artwork has no key. | `crates/layer-ui/src/session.rs:1035`. |
| T-26 | Add `CommandId`s for Lasso Fill, entering and leaving artwork-mask editing, and Remove Last Point. | Each is used by a bar context or command search, and today exists only as a tool action, a row gesture or Backspace. | Lasso Fill is only a tool (`crates/layer-ui/src/command_catalog.rs:469`); mask editing is entered from the row menu (`crates/layer-ui/src/art_layers.rs:1582`); Remove Last Point is Backspace (`crates/layer-ui/src/selection_tools.rs:492`). |

**T-15 details**
- **Silent paths to fix:**
  - Move on a locked or Background layer;
  - Fill, Gradient or Figure with no drawing content;
  - a Wand or Fill click with no drawing content;
  - erasing, or Clone, under alpha lock;
  - brushes refusing to paint on non-Paint layers;
  - mask strokes forced to Dry.
- **Missing presentation:** Apple does not show errors raised during a gesture.
- **New infrastructure:**
  - a reason field on `CommandState`, as `docs/ui/selection-command-inventory.md:45` requires. `command_disabled_reason` already computes the text for command search (`crates/layer-ui/src/command_catalog.rs:862`);
  - a shared transient notice (P-4);
  - refusal reasons for pen input: `Result<(), PenEvent>` currently carries no text (`crates/layer-ui/src/session.rs:3410`).

**Withdrawn after the audit:**
- **T-5** is part of ADJ-3.
- **T-6** is part of XF-2.
- **T-9** conflicts with `docs/development/tonal-selection.md:41`. T-1 covers the need: make a tonal selection, then insert an effect.
- **T-13** is part of XF-4.

## 7. Recommended sequencing

Work lands in the usual order: shared Rust and GTK first, then Web and Android, then Apple and Windows through the porting guides.

| Milestone | Contents | Journeys |
| --- | --- | --- |
| **M0 Decisions** (documents only) | GEO-0 extent model; P-9 blend domain; Pass Through default; Copy source semantics; whether the retouch Reference source includes the target; Image/Document menu versus Edit; shortcut conflicts (Ctrl+V, Merge Visible, Web-safe Transform Again). **Canvas action bar:** the BAR-0 decisions still open; whether leaving Warp keeps the mesh (BAR-2); Distort and Warp on placed photos (BAR-3); keeping transforms across focus loss (T-24). The Phase 1 plan lists the decisions it needs and their recommendations. | — |
| **M1 Phase 1: canvas action bar and transforms** ([implementation plan](../development/canvas-action-bar-transforms.md)) | BAR-0 with BAR-3, replacing the six host placement bars; BAR-2 with P-13: Free, Uniform, Distort (XF-1b) and Warp (XF-3), Flip, Rotate 90°, Reset, Skew (XF-1a), destructive commits; XF-2 bicubic and minification supersampling; finger-touch handles for every transform (part of XF-4); BAR-1 with existing selection commands and the polygon context; T-20; T-24; Remove Last Point (part of T-26). | Opens 27 for paint layers and selected pixels; improves 17, 26 |
| **M2 Quick wins** | Each lands with its bar item: T-1, SEL-4 with T-2, SEL-2 within a document, SEL-5, T-4, LYR-5 Solid/Gradient, ADJ-1 `color_action` quick win, ADJ-11 vignette removal, VIEW-1, IO-2 lossless WebP, T-8 rename, T-15 with P-4, T-23, fix the Apply Mask group message, RET-9 "revert to original photo". Also BAR-5 for Quick Mask, Selection Layers and mask editing; BAR-6 for guides; T-25; the rest of T-26. | Improves 11, 13, 14, 26, 29; canvas routes for 11, 14 and 26 |
| **M3 Foundations** | GEO-0, GEO-1 to GEO-5 with BAR-4, the rest of XF-2 (Lanczos, the export aliasing fix), SEL-1 with P-3, SEL-3, LYR-2, IO-1, T-16. | Opens 1, 2, 3, 4; completes 26 |
| **M4 Retouching** | P-2 stroke-start pages, P-5 enumerated options and pen buttons, P-11 job model, RET-1 to RET-4 with the clone-source bar (BAR-6), BAR-7, LYR-1 (after P-9), RET-7, ADJ-9. | Opens 20, 22, 23; 21 for small objects; improves 17, 24, 28 |
| **M5 Lossless transforms** (can run alongside M4) | P-8, P-10 with lossless Distort and Warp on placed photos, the rest of XF-4 (groups and several layers, pivot, snapping, nudge, Transform Again), split lines, XF-5, XF-6. | Opens 27 for placed photos; improves 17, 25 |
| **M6 Tone and color** | P-6, P-7, ADJ-1 with ADJ-4 and the picker bar modes (BAR-5), ADJ-2, ADJ-3, ADJ-5, ADJ-6, ADJ-10, VIEW-2 with its bar mode, VIEW-3 with sampler bars, IO-3, T-3, T-7, T-14. | Improves 5–10, 13, 29 |
| **M7 Masking and compositing** | SEL-6 as an on-canvas session, SEL-7, SEL-8, LYR-3, LYR-4, ADJ-7, ADJ-8, T-10. | Opens 15; improves 11, 12, 14, 16, 17, 19 |
| **M8 Advanced** | RET-5 (Content-Aware on the selection bar), RET-6, RET-8, RET-9 history brush, LYR-6 to LYR-9, IO-4 to IO-6, ADJ-11 remainder, ADJ-12, SEL-9, VIEW-4, VIEW-5, T-12, T-19, and the BAR-8 decision. | Opens 9; completes 21, 28 |

M1 to M5 open 8 of the 11 blocked journeys. M1 opens 27 for paint layers and selected pixels; placed photos follow with P-10 in M5, and until then use Select All, then Transform. M3 opens 1, 2, 3 and 4, and M4 opens 20, 22 and 23. Of the other three blocked journeys:
- journey 9 waits for M8;
- journey 15 waits for M7;
- journey 21 needs content-aware fill (M8) for large objects.

## 8. Rules for implementation agents

- **Shared core first.**
  - Commands, validation, history and tool state belong in `layer-ui` and `layer-core`.
  - Interactive pixel work belongs in `layer-render-wgpu`.
  - Export and delivery pixel work stays in `layer-color` as a bounded CPU row stream.
  - Hosts own dialogs, jobs, file access, clipboard and timing.
- **Reuse before inventing.**
  - Check [section 4.1](#41-existing-infrastructure-to-reuse) and the selection inventory's Next and Later tables.
  - Adopt existing names and sources: `RegionSource`, reference layers, `placement`, `paint_operation`, `append_operations`.
- **GPU rules.**
  - A pass must never sample and write the same subresource.
  - Order contacts correctly: dry contacts are instanced, Smudge runs in chunks of 3, Liquify runs 1 contact per swap.
  - No upload or readback during contact. Pipelines are ready before pen-down. Meet the 8.33 ms p99 budget.
  - Predictions never mutate persistent state, and late corrections replay deterministically ([GPU brush engine](../reference/gpu-brush-engine.md)).
- **Domains and HDR.**
  - Layer blends and resampling run in linear light. Adjustments run in encoded document RGB with declared semantics.
  - Every new operation states its domain and is correct at all four sample depths.
  - Do not add silent clamps; resolve the existing Add and Color clamps as part of LYR-1.
  - Hue-based operations compute hue in Oklab.
- **Selections are soft.**
  - Honor partial coverage and the `inverted` flag.
  - Disable destructive artwork commands in Quick Mask and in mask editing unless a scalar-mask counterpart is specified.
- **One action is one undo step.**
  - Admit large edits through `RasterRevision::pending` and `reserve_pending_bytes` (`crates/layer-core/src/raster.rs:422`) or through eager source admission (`crates/layer-core/src/lib.rs:1698`).
  - Respect 512 MiB of history, 1 GiB per publication, and the project, new-document and GPU size limits.
- **Formats and compatibility.**
  - `.capy` changes need a version step that keeps reading earlier versions and recovery files ([project format](../reference/project-format.md)).
  - Effect ABI 3 uses strict equality; prefer optional metadata fields.
  - Export presets (`CAPYPRESETS\x01`), settings (`deny_unknown_fields`) and workspace layouts (legacy layouts plus migrations) are versioned surfaces too.
- **Codecs.**
  - Pure Rust only, with a vendor audit.
  - Memory admission through `PhotoMemoryBudget` and `DecodeLimits`.
  - Cancellation through `CaptureControl`.
- **Interaction.**
  - Draggable panel or list UI follows the [drag and reorder convention](../ui/drag-and-reorder.md).
  - On-canvas handles (crop, transform, mesh points, clone source disc, samplers, the compare divider) drag immediately and never use holds.
  - Every modifier-based action has a visible touch or pen equivalent. For canvas objects that equivalent is on the canvas action bar or in Tool Options, never only in a menu.
- **Canvas action bar.**
  - Every command that acts on a selection, transform, crop, mask or other canvas object declares its bar context and priority in the same change that ships it, or states that it has none ([section 5](#canvas-action-bar-bar)).
  - Menus and command search stay complete; the bar never holds an action without a `CommandId`.
  - Modes are sticky choices, and modifiers are temporary overrides.
  - The bar is a glass surface in the panel layer. It never takes window focus, never sets `popup_open`, never moves during a contact and never covers the handles of its object.
  - Completion contexts keep Cancel and Apply visible when the bar is hidden.
  - Test each new bar item with mouse, touch and pen on every host that ships it.
  - A touch hold is already taken by the color picker.
  - Check each chord with `KeyChord::available` for Web.
  - Route contextual keys, held modifiers and pen buttons through the command framework's resolver stages (C and D) rather than host-specific handlers ([command framework handoff](command-framework-handoff-2026-09-25.md)).
- **Evidence.**
  - Independent GPU oracle tests for each algorithm.
  - Clone, heal and new blend modes join the brush acceptance matrix and the benchmarks.
  - A native GTK scenario for each journey in `apps/layer-linux/src/tests.rs`.
  - Record completed journeys in the current guides, not in this dated record.

## 9. Out of scope and deferred

| Item | Decision and reason |
| --- | --- |
| RAW development | Out of scope ([float32/HDR scope](../development/float32-hdr-scope.md), [color management](../ui/color-management.md)). Use the IO-6 hand-off. |
| Generative fill, expand, upscale and harmonize | Conflicts with local-only processing and the no-accounts principle. Exemplar inpainting (RET-5) and optional on-device segmentation (SEL-9) cover the core demand. |
| CMYK, Lab and Grayscale modes | Soft proofing plus CMYK/Gray ICC delivery already cover print. |
| Layered PSD | Already on the color-management roadmap; large interoperability surface. |
| Text, arrows and annotation | See the [vector research](vector-drawing-research.md) and [vector layers](vector-layers-research.md). Figure already covers lines and boxes. |
| Actions and scripting beyond WGSL | Batch presets (IO-4) come first. Embedded compute or storage in WGSL needs a security review. |
| Face-aware Liquify and red-eye detection | Need landmark detection; P3. |
| A separate refine workspace like Photoshop's Select and Mask | Rejected: Refine Edge is an on-canvas session with its own bar (SEL-6). |
| Moving, pinning and customizing the canvas action bar | Deferred past the first delivery (BAR-0). Moving uses an immediate handle; customizing uses the workspace command inventory, never a second system. |

## Sources

Selected sources. The source reports contain further threads and view counts.

**Official documentation**
- [Photoshop Clone Source panel](https://helpx.adobe.com/photoshop/desktop/repair-retouch/heal-clone/clone-source-panel.html)
- [Remove tool](https://helpx.adobe.com/photoshop/using/remove-tool.html)
- [Content-aware crop](https://helpx.adobe.com/photoshop/desktop/crop-resize-transform/crop-straighten/apply-content-aware-fill-while-cropping-images.html)
- [Match Color](https://helpx.adobe.com/photoshop/desktop/adjust-color/selective-color-adjustments/match-color-between-two-images.html)
- [Hue/Saturation object color](https://helpx.adobe.com/photoshop/desktop/adjust-color/selective-color-adjustments/replace-object-colors-by-applying-a-hue-or-saturation-adjustment.html)
- [Affinity cloning and healing](https://affinity.help/photo2/English.lproj/pages/Retouching/retouching_cloningHealing.html)
- [GIMP 3.0 transform tools](https://docs.gimp.org/3.0/en/gimp-tools-transform.html)
- [GIMP clone tool](https://docs.gimp.org/3.0/en/gimp-tool-clone.html)
- [GIMP copy/paste specification](https://developer.gimp.org/core/specifications/copy-paste/)
- [GIMP 3.2 release notes](https://www.gimp.org/release-notes/gimp-3.2.html)
- [Krita transform tool](https://docs.krita.org/en/reference_manual/tools/transform.html)
- [Krita clone engine](https://docs.krita.org/en/reference_manual/brushes/brush_engines/clone_engine.html)
- [Krita Smart Patch](https://docs.krita.org/en/reference_manual/tools/smart_patch.html)
- [Procreate clone](https://help.procreate.com/procreate/handbook/adjustments/adjustments-clone)
- [Procreate interpolation](https://help.procreate.com/procreate/handbook/transform/transform-interpolate)
- [Pixelmator Pro repair and clone](https://support.apple.com/guide/pixelmator-pro/repair-remove-and-clone-objects-in-images-pixc5f9d789e/mac)

**Community demand**
- Stack Exchange:
  - [GD make background transparent](https://graphicdesign.stackexchange.com/questions/5446)
  - [SU move things with selection in GIMP](https://superuser.com/questions/279725)
  - [PH crop a single layer](https://photo.stackexchange.com/questions/30956)
- Adobe community:
  - [Clone Stamp not working on a new layer](https://community.adobe.com/t5/photoshop-ecosystem-discussions/clone-stamp-tool-not-working-in-a-new-layer-photoshop-2022-and-cropping-issue/td-p/12777858)
  - [Pro Retouchers need better Liquify](https://community.adobe.com/feature-requests-713/p-pro-retouchers-need-better-liquify-653496/index4.html)
  - [Lightroom request for Photoshop-like clone/heal](https://community.adobe.com/feature-requests-564/p-more-photoshop-like-clone-healing-content-aware-brushes-666552/index3.html)
  - [Colors look dull in export](https://community.adobe.com/t5/photoshop-ecosystem-discussions/colors-look-dull-in-export/td-p/11526128)
  - [Hate the new Select and Mask](https://community.adobe.com/questions-712/hate-the-new-select-and-mask-tool-1117387)
- GIMP GitLab and discuss.pixls.us:
  - [Heal Selection as a native feature (#4762)](https://gitlab.gnome.org/GNOME/gimp/-/work_items/4762)
  - [GIMP equivalent of Transform-Warp](https://discuss.pixls.us/t/looking-for-gimp-equivalent-of-photoshops-transform-warp/34923)
  - [Per-effect masks in GIMP 3 NDE](https://discuss.pixls.us/t/nde-layer-workflow-in-gimp-3-0-masks/48935)
  - [Heal tool very slow](https://discuss.pixls.us/t/heal-tool-in-gimp-very-slow/38466)
- Photopea issues: content-aware fill #4254, Clone Source panel #5391, puppet modes #8114, Liquify freeze #3827 ([tracker](https://github.com/photopea/photopea/issues)).
- Affinity forum:
  - [Current Layer & Below default for Inpainting/Patch](https://forum.affinity.serif.com/index.php?/topic/200962-current-layer-below-as-default-for-inpainting-and-patch-tools/)
  - [Puppet Warp request](https://forum.affinity.serif.com/index.php?%2Ftopic%2F185611-feature-request-proper-puppet-warp-tool-like-photoshop%2F=)

**Contextual canvas bars** ([source report 14](photo-editing-research/14-contextual-bars-in-other-editors.md) lists every source)
- [Clip Studio Paint Selection Launcher](https://help.clip-studio.com/en-us/manual_en/330_selection/Selection_Launcher.htm) and [transformation types](https://help.clip-studio.com/en-us/manual_en/360_transform/Types_of_transformations.htm)
- [Procreate transform interface](https://help.procreate.com/procreate/handbook/transform/transform-interface-gestures) and [selection interface](https://help.procreate.com/procreate/handbook/selections/selections-interface)
- [Photoshop Contextual Task Bar](https://helpx.adobe.com/photoshop/using/contextual-task-bar.html) and [Photoshop Elements Contextual Task Bar](https://helpx.adobe.com/photoshop-elements/using/contextual-task-bar.html)
- [Krita selections](https://docs.krita.org/en/user_manual/selections.html) and [Selection Action Bar feedback](https://krita-artists.org/t/feedback-for-the-new-selection-action-bar-in-krita-5-3/141290)
- [Affinity Photo context toolbar](https://affinity.help/photo2/en-US.lproj/pages/Tools/tools_meshWarp.html)
- [Apple edit menus](https://developer.apple.com/design/human-interface-guidelines/edit-menus), [Microsoft CommandBarFlyout](https://learn.microsoft.com/en-us/windows/apps/design/controls/command-bar-flyout), [WAI-ARIA toolbar pattern](https://www.w3.org/WAI/ARIA/apg/patterns/toolbar/)

**Tutorials and reviews**
- [Julieanne Kost, 10 clone/heal tips](https://jkost.com/blog/2021/12/10-tips-for-the-clone-stamp-and-healing-brush-tools-in-photoshop.html)
- [Kost, Free Transform preference](https://jkost.com/blog/2019/06/new-free-transform-preference-in-photoshop.html)
- [PHLEARN, remove anything](https://phlearn.com/tutorial/how-to-remove-anything-photo-photoshop/)
- [PHLEARN, frequency separation](https://phlearn.com/tutorial/amazing-power-frequency-separation-retouching-photoshop/)
- [PTC, advanced hair masking](https://photoshoptrainingchannel.com/advanced-hair-masking/)
- [PTC, remove tourists with stack mode](https://photoshoptrainingchannel.com/remove-tourists-stack-mode/)
- [Affinity, frequency separation explained](https://www.affinity.studio/blog/frequency-separation-explained)
- [Fstoppers, Affinity Photo 2.5 review](https://fstoppers.com/reviews/affinity-photo-25-imperfect-perfect-alternative-photoshop-it-depends-669573)
- [XDA, Photoshop features missing in Affinity](https://www.xda-developers.com/photoshop-features-not-available-in-affinity-photo/)
