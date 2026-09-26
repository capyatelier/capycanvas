# Code inventory: adjustments, color, files, view, history and automation

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-25 · baseline `dac76c20`

Read-only code inventory made by an agent against baseline `dac76c20`. It lists what existed in code at that commit, with `file:line` references. The implementation audits (reports 05–10) corrected several of its claims; the [research record](../photo-editing-research.md) reflects the corrections.

---

**Architecture in brief.** Filters and adjustments are defined as data, not Rust code. `assets/filters/manifest.json` (format 1, ABI 3) lists **40 filters** in 7 categories (tone, color, detail, blur, artistic, distort, texture). Their WGSL lives in `assets/filters/{effects,filter_library,gaussian-prepare}.wgsl`. The loader is `crates/layer-core/src/effect_catalog.rs:14` (`bundled_effect_catalog`) and the schema is in `crates/layer-core/src/effects.rs`. Hosts can hot-load more filter packages (add/replace/merge); the example is `examples/filters/tent-blur`. The web manifest `apps/layer-web/filters/manifest.json` has the same IDs.

---

## 1. Adjustments and filters

### Model: non-destructive only
- Each filter is an **effect layer**: `LayerKind::Effect` (`crates/layer-core/src/lib.rs:168`), with one `effect: Option<Arc<EffectInstance>>` per layer (`lib.rs:196`).
- `EffectAction::Insert` (`crates/layer-ui/src/effects.rs:740`) inserts a new layer above the target's clipping stack.
  - An effect affects the composite below it inside its group.
  - Clipping it to a layer makes it act like a "smart filter" on that one layer. There is no per-layer filter stack; you stack effect layers instead.
- Effect layers support opacity, 7 blend modes, visibility (this is the only "bypass"), clipping and raster masks. Mask, opacity and blend apply to the final pass only.
- **There is no destructive path.** Nothing bakes, rasterizes or merges an effect into pixels. The `LayerAction` enum (`crates/layer-ui/src/art_layers.rs:117`) has no Merge Down, Merge Visible, Flatten or Stamp.
- Parameters are saved in `.capy` along with the resolved WGSL, and stay editable after reopening.

### Masking and selection
- **Masking: exists.** `LayerMask` (`crates/layer-core/src/layers.rs:418`) is a raster mask with linked/unlinked, inverted and enabled states.
- **Apply to selection: partial.** Insert does not mask to the active selection automatically. The user has to run `LayerAction::MaskSelection {hide}`, which turns into `AddMask{replace:true}` from the selection (`art_layers.rs:783`).
- **Live preview: exists.** The canvas updates live while you edit. A `Gesture` action groups a slider drag into one undo entry and can be cancelled (`effects.rs:177`).
- The picker shows GPU thumbnails for each filter, cached and scheduled asynchronously (`crates/layer-ui/src/filter_previews.rs`, `crates/layer-render-wgpu/src/filter_previews.rs`), with category plus search.
- **Missing:** no before/after split or compare view, no user presets for adjustment settings, and no copy/paste of settings. Controls are Set, Reset per key, Number, CurvePoint and GradientStop.

### Adjustments that exist (manifest line numbers)

| Adjustment | Line | Controls and notes |
|---|---|---|
| Curves | 19 | Master plus separate R/G/B curves (per-channel curves exist). Up to 32 points, Hermite segments. "Encoded RGB" or "Log HDR" domain with an HDR range in EV. No histogram behind the curve. |
| Levels | 71 | Input black/white, gamma, output black/white, clamp input/output. **Composite only: no per-channel levels, no auto.** |
| Brightness / Contrast | 135 | |
| Hue / Saturation | 162 | Hue, sat, lightness. **Master only: no per-hue-range editing, no colorize.** |
| Color Balance | 195 | Shadows, midtones and highlights × 3 color pairs, plus preserve luminosity. |
| Exposure | 279 | EV, offset, gamma. Float32 documents get ±126 EV (`effects.rs:367`). |
| Vibrance | 312 | Vibrance, saturation, protect skin tones. |
| Black & White | 344 | Six hue sliders plus tint and tint color. |
| Gradient Map | 406 | Stops (color and alpha), reverse, amount. |
| Posterize | 441 | |
| White Balance | 704 | Temperature, tint, preserve luminosity. **Sliders only: no neutral-point eyedropper.** |
| Split Tone | 737 | |
| Solarize | 1194 | Has a threshold parameter; it is not a Threshold adjustment. |

**Absent adjustments** (grep finds zero hits): Invert (as an adjustment), Threshold, Channel Mixer, Selective Color, Photo Filter, Shadows/Highlights, HDR Toning (as a layer), Color Lookup / LUT / .cube, Match Color, Replace Color, Desaturate, Equalize, any auto tone/levels/color, dehaze, clarity/texture.

- There are **no eyedroppers for black, white or gray point** in Levels, Curves or White Balance. The only color hook is `UseCurrentColor`, which copies the brush color into a color parameter (`effects.rs:174`).
- **Histogram is partial.** It is a separate non-modal inspector window, not a panel, and it is not shown inside Levels or Curves.
  - `Histogram` is defined at `crates/layer-core/src/color/histogram.rs:35` and the GTK inspector at `apps/layer-linux/src/histogram.rs:20`. It opens via `CommandId::Histogram`.
  - It covers the full-resolution composite only: 256 bins, R/G/B plus luminance, log scale, auto refresh, below/above clipping counts, and HDR stops axis.
  - It does not offer a per-layer or selection-only histogram.

### Filters that exist
- **Blur:** Gaussian Blur (manifest 462; **max radius 21 px**), Motion Blur (601; max 64 px), Bloom (875), Soft Focus (924).
- **Detail:** Unsharp Mask (502; radius, amount, threshold), High Pass (554), Denoise (633), Edge Detect (665), Emboss (1064).
  - Denoise is a small bilateral filter with radius 1–3 (`filter_library.wgsl:68`). It is closer to a tiny surface blur than real noise reduction.
- **Tone:** Vignette (775; strength, radius, softness, center). It creates a vignette; it is not lens correction.
- **Texture:** Film Grain (821). This is the only "add noise"-type filter. Also VHS and CRT.
- **Distort:** Chromatic Aberration (1130). It is an artistic add-fringe effect (separation plus angle), **not CA removal**. Also Kaleidoscope, Swirl, Ripple, Glass, Rainy Glass, Heat Haze, Domain Warp (a noise displacement, not a displacement map).
- **Artistic:** Halftone, Crosshatch, Pixel Mosaic, Painterly (a Kuwahara-like quadrant filter), Pencil.
- **Color:** Iridescence.
- Nine filters are animated (they take a `time` value).

**Absent filters:** Surface Blur (at usable radius), Lens Blur/bokeh, Smart Sharpen, real Noise Reduction, Median, Dust & Scratches, Lens Correction/distortion/defringe, Dehaze, Clarity, Frequency-separation helpers, Displace using a map image, Perspective/Warp.

### Retouch-type tools
- **Exist:** brush-based Liquify with modes Push, Twirl CW/CCW, Pinch, Expand, Crystals, Edge, Reconstruct (`LiquifyMode`, `crates/layer-core/src/lib.rs:489`), and Blend/Smudge.
- **Absent:** Clone Stamp, Healing, Spot Heal, Content-aware, Dodge/Burn, Sponge, Red-eye (all zero hits).

---

## 2. Color

- **Color modes:** RGB only. There are 4 built-in spaces: sRGB, Display P3, Adobe RGB, ProPhoto (`RgbSpace`, `crates/layer-core/src/color/rgb.rs:6`).
  - A document is `DocumentColor{space, depth}` (`crates/layer-core/src/color.rs:25`).
  - There is **no Grayscale, CMYK or Lab document mode**, and no arbitrary ICC working space; a document has to use one of the four built-ins.
- **Bit depth: exists.** 8-bit int, 16-bit int, 16-bit float, 32-bit float (`SampleDepth`, `color/profile.rs:13`). Float is linear, with RGB 1 = 203 cd/m².
  - New Document offers all four (`crates/layer-ui/src/settings_color.rs:103`).
  - Change Bit Depth, Assign Profile and Convert Color Space are commands (`CommandId`, `crates/layer-ui/src/lib.rs:521`), with optional stochastic 8-bit dithering.
- **ICC: exists.** Uses moxcms (`crates/layer-color/src/icc/*`).
  - Imported images keep their original samples and embedded ICC as "Original" sources.
  - Gray and CMYK sources are accepted if they carry an embedded profile (`photo.rs:232`). Untagged images are assumed sRGB and flagged. Repair Source Profile and Rasterize Source are available.
  - Rendering intents: 4 (`profile.rs:80`).
  - Black-point compensation is a field, but the portable CMM rejects it for conversion (`profile.rs:92`). It is used in proofing.
- **Soft proofing: exists.** `ProofRecipe` (`color/proof.rs:7`) takes a printer ICC, intent, BPC, simulate paper and simulate black ink. Commands: SoftProofSetup, SoftProof, GamutWarning. There is a Proof panel. The proof target is kept separate from the export profile.
- **HDR: exists.**
  - Float documents, HDR display output (Vulkan/GL on GTK; scRGB/PQ on Windows; Android).
  - A document-level `SdrRendition` (exposure, contrast, headroom, highlight color, balance; `color/hdr/sdr.rs:19`) with a local tone-mapping guide (`color/hdr/local.rs:16`). Preview SDR command. This is the closest thing to "HDR toning", but it is document-wide and not a layer.
- **Eyedropper: exists.**
  - Sample source: Composite or current layer (`ColorSampleSource`, `crates/layer-render/src/lib.rs:294`).
  - The UI offers sizes 1, 5, 15, 51, 101 px (`COLOR_SAMPLE_WIDTHS`, `crates/layer-ui/src/eyedropper.rs:8`). The renderer also supports 3×3 and 5×5 squares (`lib.rs:300`).
  - Circular samples are averaged in Oklab.
- **Color entry:** Document RGB, Linear RGB, sRGB hex, HSV, HLS, OKLCH (`crates/layer-ui/src/color/editor.rs:7`). Palettes import Adobe and other app formats (`color/palette_*.rs`).
- **Info panel / pixel readout: absent.** There is no cursor coordinate or RGB-under-cursor panel. The Stats panel shows GPU/CPU telemetry, not pixel info.

---

## 3. File I/O

- **Open/import: exists.** All decoders are pure Rust with no C codecs. Format is detected from the file signature. The list is `PHOTO_FORMATS` (`crates/layer-color/src/photo.rs:54`); dependencies are in `crates/layer-color/Cargo.toml:14-29`.

| Format | Limits |
|---|---|
| JPEG | Includes MPF primary image and gain-map HDR; CMYK/YCCK with ICC. |
| PNG | 8/16-bit, plus PQ HDR PNG. |
| TIFF | 8/16-bit unsigned only; **single page** (multi-page rejected, `tiff_io.rs:21`); no float TIFF. |
| WebP | |
| GIF | First frame only. |
| BMP / DIB | |
| HEIF / HEIC | `rust_h265`; SDR only. |
| AVIF | rav1d; includes gain maps. |
| OpenEXR | Flat RGB(A); no deep or multipart. |

- **Absent imports:** RAW/DNG with **no camera RAW processing**, PSD, SVG, JPEG XL, PDF, KRA/ORA. These are explicitly out of scope per `docs/development/float32-hdr-scope.md`.
- **EXIF orientation: exists.** Normalized losslessly on import (`photo/orientation.rs:7`; called from `jpeg_io.rs:59`, `raster_io.rs:126`).
- **Metadata: mostly dropped.** Only orientation and print density are parsed (`photo/metadata.rs:1-8`). **EXIF, XMP and IPTC are not carried into exports.** Export writes the ICC profile and PPI.
- **Open vs Import/Place: exists.**
  - Open creates a document at the oriented image size.
  - Import, Paste Image and Drop add a layer with placement handles. Several files can be imported at once (`ImageImportBatch`, `crates/layer-ui/src/import_policy.rs:127`).
  - A placed layer **keeps its full-resolution source through scale and rotate**, similar to a smart object, and has "Original Size (100%)".
- **Export: exists.** `ExportFormat` is at `crates/layer-ui/src/export.rs:9`.
  - SDR documents: PNG, TIFF, JPEG.
  - HDR documents: EXR, HDR PNG, JPEG gain map, AVIF gain map, each with an optional "mapped" variant.
  - CMYK output ICC is limited to TIFF and JPEG; gray ICC output is supported.
  - **No WebP, GIF, HEIC or SDR-AVIF export.**
- **Export options:**
  - Format, profile (built-in or source/imported ICC), depth, JPEG quality, intent, 8-bit dither, background matte (Preserve/White/Black), and PPI (Master/Ppi/Omit) (`ExportRecipe`, `export.rs:147`).
  - Resize is Fit-in-box only, with optional enlarge (`ExportSize`, `export.rs:99`). Downscaling averages pixels by area; enlarging uses Catmull-Rom (`crates/layer-color/src/resize.rs:1-3`).
  - Destination presets: Web/Share, Wide-color, Further editing, Custom, plus named user presets (`crates/layer-ui/src/export_presets.rs:25`). GTK shows an export preview (`apps/layer-linux/src/files/preview.rs`).
  - **Absent:** export selection, export layers, slices, batch export, save-for-web comparison.
- **Project format:** `.capy` v6 (LZ4, 256² sparse tiles). It stores layers, masks, source originals with ICC, live effects, selections, rulers and the proof recipe (`docs/reference/project-format.md`). **Undo history is not saved.** Crash recovery exists (`crates/layer-ui/src/recovery.rs`).

---

## 4. View, navigation and measurement

- **Zoom: exists.** Range 2%–1600% (`crates/layer-ui/src/camera.rs:79`), with Zoom In/Out in √2 steps and Fit (`session.rs:4237-4260`). **There is no Actual Pixels (100%) command** and no zoom-percentage entry.
- **Pan (Hand tool) and rotate view: exist.** Free rotation by gesture plus Rotate Left/Right in 90° steps. Flip view horizontal/vertical changes the view only (`camera.rs:152`).
- **Navigator: exists** (`crates/layer-ui/src/navigator.rs:48`, `Panel::Navigator`).
- **Rulers and guides: different from Photoshop.** The "rulers" are drawing guides — Straight, Parallel and Radial — that brush strokes snap to (`crates/layer-core/src/rulers.rs:7`; commands Ruler, ShowRulers, SnapRulers).
  - **Absent:** edge rulers with units, horizontal/vertical guides, grid or pixel grid, snap-to-guides for transforms, and a measure tool.
- **Split before/after:** absent. Soft-proof and Preview SDR toggles are the only compare-style views.
- **Multiple views of one document:** absent. Document tabs and New Window open separate drawings.
- **Panels** (`Panel` enum, `crates/layer-ui/src/layout.rs:615`): Toolbar, Commands, Brushes, BrushSets, FilterTypes, SculptSets, Tools, ToolSettings, Color, Palettes, Sizes, Layers, Adjustments, Properties, Stats, Navigator, Proof. **There is no Info, Histogram, History, Channels or Actions panel.**

---

## 5. History

- **Undo/redo: exists.** Edits are inverted in place (`crates/layer-core/src/lib.rs:2019-2040`). The budget is 512 MiB and 256 entries (`history_budget.rs:6-7`). Tiles are shared between revisions, and color-mode changes and effect gestures each take one step.
- There is also a separate workspace-layout undo (`UndoWorkspace` / `RedoWorkspace`) with a history dialog. It covers UI layout only (`crates/layer-workspace/src/history_presentation.rs`).
- **Absent:** a document History panel, snapshots, history brush and persistent history.

---

## 6. Automation

- **Actions/macros, batch processing, scripting: absent** (no hits).
- **Closest things that exist:**
  - Runtime WGSL filter packages, which work as shader "plugins": no rebuild needed, loaded transactionally, with a sandbox of declared bindings only (`docs/reference/runtime-filters.md`).
  - Web console calls `layerApp.loadFilters()` and `layerApp.state()`.
  - Windows C FFI `capy_load_filter_directory`.
  - Pen-input recording for diagnostics and prediction datasets (`crates/layer-engine/src/recording.rs`), not user macros.
- There is no shader-editor UI and no node graph.

---

## Notable gaps and extras you didn't ask about

**Gaps:**
- **Crop, canvas size, image size/resample and whole-image rotate are all missing.** Document Properties shows the canvas size read-only (`apps/layer-linux/src/files/properties.rs:69`). The `Edit` enum (`lib.rs:1642`) has no resize or crop variant.
- **No copy or cut of pixels.** There is Paste Image only, plus Copy/Paste Mask.
- **Transform is affine only**, with Nearest or Linear interpolation (`crates/layer-core/src/affine.rs:6-16`). No perspective, distort or warp, and no bicubic for transforms.
- **Only 7 layer blend modes:** Normal, Multiply, Screen, Add, Overlay, Soft Light, Color (`layers.rs:364`). Missing Darken, Lighten, Difference, Luminosity, Hue, Saturation and others.
- No text tool and no vector layers (research docs only).
- The default photographer workspace deliberately leaves out clone, heal and crop (`docs/ui/default-workspaces.md:25`).

**Extras:**
- **Selection tools:** Rectangle, Ellipse, Lasso, Polygon, Wand, Color, Brush and Tonal (`crates/layer-ui/src/selection_tools.rs:9`).
  - Tonal is a luminosity-band selection measured in EV stops: 16 bands, HDR-aware (`crates/layer-core/src/tonal.rs`). It acts like luminosity masks.
  - Quick Mask and saved Selection Layers exist. Region refinement offers gap closing, expansion and edge smoothing (`region_tools.rs:31`). There is no feather or Select Subject.
- Clipping masks, groups and reference layers exist. Shape tool: line, rectangle, ellipse. Gradient and fill tools exist.
- GPU-only rendering (wgpu) with no CPU fallback. Cross-backend pixel parity is not established: the strict v4 reference still fails on Metal and D3D12 per `runtime-filters.md`.
- All six hosts (GTK, Web, Android, Windows, macOS, iOS) route through the shared `layer-ui` commands, but qualification varies by platform. The project-format doc says GTK is the reference host.
