# Implementation audit: view, files and delivery (VIEW, IO)

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-25 · baseline `dac76c20`

Read-only implementation audit made by an agent against baseline `dac76c20`. It checks each item in the first draft of the build list against the code, the platform hosts and the design records. "Plan line N" refers to that superseded first draft; the current [research record](../photo-editing-research.md) incorporates the corrections. Line numbers can drift in later commits; verify before relying on one.

---

Every item below is traced to file:line. Verdict key: **Acc** = accurate; **Overlooked** = existing infrastructure the plan missed; **Wrong** = wrong or imprecise claim; **Partial** = partly implemented already; **Designed** = already designed in a doc.

---

### A. §1 inventory rows (plan lines 86, 95, 96, plus Summary line 36)

**Documents and color (l.86): Acc, but incomplete.**
- Citations check out: `RgbSpace` rgb.rs:6, `SampleDepth` profile.rs:13, "Photo editing" preset document_creation.rs:93.
- Missing from the row:
  - Document Properties (shared `DocumentInfo`, layer-color/src/document_info.rs:12-103).
  - Source Color Profile… and Rasterize Source (`CommandId::RepairSourceProfile`/`RasterizeSource`, lib.rs:536-537).
  - The "HDR drawing" preset (color-management.md:320).
  - Retained originals keep their embedded ICC (source.rs:101-107).

**View (l.95): Overlooked infra.**
- Every host already shows a zoom/rotation readout "N% · D°":
  - GTK workspace.rs:1021-1039 and 2575-2581
  - Web index.html:35, app.js:849-852
  - Android Workspace.kt:423-427 (click runs `fit_canvas`)
  - Apple EditorView.swift:223
  - Windows WorkspaceView.cpp:767
- It is a shared layout element, `CanvasInfoLayout` (header.rs:449), the "canvas-status HUD" in shared-ui.md:93-100.
- Also present but not listed:
  - Continuous Ctrl+wheel zoom with a `zoom_speed` setting (session.rs:3529-3535), plus pinch zoom.
  - Proof panel and Preview SDR.
  - Histogram window.
  - Drawing guides with Show/Snap toggles in the View menu (lib.rs:234-245).
- **Correction:** list these. Keep "no Actual Pixels / typed zoom / split view / Info / guides-grid / document History".

**Files (l.96): Wrong.**
1. "EXIF, XMP and IPTC are dropped on export (only orientation and PPI are parsed)" is imprecise.
   - Export deliberately writes a freshly generated minimal EXIF: Orientation=1 plus X/YResolution and ResolutionUnit.
     - The policy is stated at metadata.rs:1-2; the writer is `exif_output`, metadata.rs:103-129.
     - It is used by JPEG (jpeg_io.rs:164-165), HDR JPEG (gainmap/jpeg.rs:148), AVIF (avif_io/mux.rs:288-289) and native gain maps (gainmap/native.rs:197).
     - PNG writes pHYs (png_io.rs:229-230); TIFF writes rational density (tiff_io.rs:216-217).
   - Parsing also covers more than EXIF:
     - JFIF density (jpeg_markers.rs:113-123).
     - ICC in all containers.
     - Gain-map XMP through quick-xml (gainmap/jpeg_container.rs:84).
   - IPTC (APP13) is not parsed: jpeg_markers.rs has no 0xED branch.
2. "Size is fit-in-box only" is wrong. `ExportSize` is `Original | Fit{bounds, enlarge}` (export.rs:99-106); GTK offers "Original size / Fit within" plus "Allow enlargement" (files/export.rs:195-217).
3. Omitted from the row:
   - CMYK and Gray ICC delivery (export.rs:243-247, 272-279), which §8 relies on.
   - "Use print profile" delivery.
   - Multi-file Import, which is already GIMP-style "Open as Layers" (see IO-6).
- **Correction for Summary l.36 and row l.96:** "Camera EXIF, XMP and IPTC from sources are not retained; export writes only a generated EXIF with Orientation 1 and print density (plus pHYs/TIFF density). Size is Original or Fit (optional enlargement)."

**Geometry (l.87).**
- "zoom 2%–1600%" is Acc: clamp 0.02–16.0 at camera.rs:43 and :79, session.rs:2028-2029.

---

### B. VIEW items

**VIEW-1 Actual Pixels and zoom entry: Wrong claim + Overlooked infra.**
- **Wrong:** "users can reach 100% only by stepping in √2 increments". Zoom In/Out multiply the *current* zoom by √2 (session.rs:4244-4268). Starting from a fit value (camera.rs:39-46, 0.9 margin), stepping practically never lands on 100%. Wheel and pinch zoom are continuous with no 100% snap (session.rs:3529-3535).
- **Overlooked:**
  - The existing readout is the natural zoom-entry target, but it is inconsistent: GTK is a plain `gtk::Label` whose tooltip mentions only Ctrl+0; Web is a plain `<output>`; Android's click fits the canvas.
  - Typed entry should reuse `NumericControl` (numeric.rs:26, `percent()` at :79; docs/ui/numeric-controls.md).
  - Zoom to selection = generalize `Camera::fit` to a Rect using `Selection::bounds` (selection.rs:284). Note `fit` also resets rotation (camera.rs:45); decide whether 100% keeps rotation.
  - Navigator zoom buttons are hard-coded per host: GTK navigator.rs:350-357, Web editor-panels.js:327, Windows NavigatorView.cpp:52. None has Fit, 100% or a field.
  - Menu is shared `VIEW_MENU` (lib.rs:234-245); shortcuts are Ctrl+0/=/- (shortcuts.rs:229-231).
  - `Camera.zoom` is document px per physical surface px, so 1.0 is true 1:1 on HiDPI (compare `ruler_reach`, rulers.rs:66-71).
- **Naming conflict:** "Original Size (100%)" already means placement layer scale (lib.rs:1106).
- **Correction:**
  - "Add `CommandId::ActualPixels` (label distinct from 'Original Size (100%)') to `VIEW_MENU` and the Navigator row on every host."
  - "Make the canvas-info readout a shared interactive control (typed % via NumericControl, presets, Fit)."
  - "Check Ctrl+1 against `KeyChord::available(Web)` (shortcuts.rs:119-125)."

**VIEW-2 Before and after: Overlooked infra.**
- The "proof lens" is **not** a compare view. It is a static 512² glass illustration for the Proof dial (color-management-proof-lens.md:18-30).
- Existing compare infrastructure:
  - GTK `Comparison` side-by-side thumbnails. "Before/After" is used by Assign/Convert/Bit depth, Rasterize and Source profile (preview.rs:140-142; files/color.rs:139, rasterize.rs:16, source.rs:47). Export uses "Master/Output" (preview.rs:143-145).
  - Export already shows master beside output (color-management.md:171-172, 346-349).
  - Proof and HDR/SDR transforms are applied at presentation (present.rs:536-537 includes hdr_view.wgsl and proof_view.wgsl). So a split for Proof/SDR is a present-stage change, while "without effects" needs a second composite.
  - A momentary held shortcut already exists: `ShortcutAction::Pan` "momentary input mode" (shortcuts.rs:157-163).
- **Do not reuse Solo as-is.** It writes `Edit::SetLayerVisibility` through `layer_edit`/`apply_edit`, so it is undoable and dirties the document (art_layers.rs:1075-1123, 714-721). There is no effect bypass separate from visibility.
- There is no multi-view of one document (NewWindow opens a separate window, session.rs:4290-4293).
- **Correction:**
  - "Split/hold compare for Proof/SDR = present-shader region split."
  - "Effects-off compare = view-only render override (not Solo/history), bound via a new momentary ShortcutAction plus a touch control."
  - "Reuse Before/After labels."

**VIEW-3 Info panel and color samplers: Overlooked infra.**
- Sampling:
  - `ColorSampleRequest`/`ColorSample` (layer-render/src/lib.rs:294-345) samples Composite or Layer with Point/Average/Circle areas, returning straight linear document RGB *before* view transforms. That matches the rules "inspection independent of monitor/proof" and "readouts never depend on calibration" (color-management.md:113, 146).
  - Latest-point coalescing with a live `preview` already exists in `Eyedropper` (eyedropper.rs:1-2, 57-135).
  - Limit: one request in flight (color_sample.rs:1-3, 46-49). Four samplers plus the cursor need a batched request.
- Readout formatting exists: `ColorReadout` OKLCH/HSB/HLS/RGB (color.rs:49-63, 830-860). The app vocabulary is OKLCH, not "Lab/LCh". CIELAB code exists privately in layer-color/src/icc/proof/pcs.rs:101-113 and color/palette_file.rs:120.
- Visibility-gated sampling pattern: `sync_renderer_telemetry` for Stats/Diagnostics (session.rs:4633-4660).
- Placement options: a new Panel, or the status HUD.
- "Before/after while an adjustment is edited" has no sampling source today.
- **Correction:** cite these; use OKLCH (optionally CIELAB via the pcs.rs code); specify batched multi-point sampling and a pre-edit sample source.

**VIEW-4 Grid, guides and overlays: Wrong framing ("rulers stay separate") + Overlooked infra.**
- Guide infrastructure already exists:
  - `Document.rulers`, "Global, non-raster guides; durable, share document undo" (lib.rs:1296-1297).
  - `Edit::SetRulers` (lib.rs:1490-1492), up to 1,024 guides, persisted in .capy (project-format.md:55).
  - Drawn via the existing GPU `CursorSegment` overlay (rulers.rs:255-300; familiar-workspace.md:937-940).
  - Edited by the Move tool (familiar-workspace.md:1256; rulers.rs:99-108).
  - Show/Snap commands exist.
- Limit: snapping applies to brush strokes only (`set_ruler_snapping`, canvas.rs:673; rulers.rs:72-76).
- **Name collision:** "Show rulers"/"Snap to rulers" already mean drawing guides (lib.rs:1112-1113). Ctrl+R is browser-owned on Web (shortcuts.rs:124).
- No grid or pixel-grid code exists (present.wgsl has none).
- **Correction:** "Add H/V guides as a `RulerGeometry` variant (or sibling list) reusing storage, undo, overlay, hit-testing and Move editing. Generalize snapping from `choose_ruler` (rulers.rs:161) to operation/crop/selection handles. Rename 'Show rulers' to 'drawing guides' before adding edge rulers."

**VIEW-5 History panel: Partial pattern exists + constraints missing.**
- Reusable pattern: the workspace history list/preview/restore (history_presentation.rs:5-55, `ManagerHistoryView` :64-73; GTK workspace_history_dialog.rs:1 "temporary preview, committed only by Restore"), including diff-based labels (`layout_change_description`).
- Blockers:
  - Document `HistoryEntry` has no label (lib.rs:1795-1799).
  - History is trimmed to 256 entries / 512 MiB (history_budget.rs:6-7; project-format.md:127).
  - Undo is gated on pending frames (canvas.rs:308-314).
  - History is never saved in the project (project-format.md:32-33, 56). Snapshots in .capy (RET-9, §7) would reverse that policy.
- **Correction:** "Capture an action label per HistoryEntry. Show trimmed history. 'Jump' = sequential undo/redo under engine gating. Snapshot persistence is a policy change needing a format version."

---

### C. IO items

**IO-1 Metadata: Wrong "Today" line + Overlooked infra.**
- Existing machinery to build on:
  - One EXIF parser shared by all containers (`metadata::exif`, called from jpeg_markers.rs:125, png_io.rs:33, webp_io.rs:42, heif_io.rs:96, avif_io.rs:545, tiff_io.rs:43).
  - The JPEG APPn preflight already retains bounded ICC chunks (jpeg_markers.rs:94-112, `MAX_ICC_BYTES`); add EXIF/XMP/APP13 capture there.
  - quick-xml is already a dependency (Cargo.toml; jpeg_container.rs:4).
  - `SourceImage` already carries `resolution` and ICC (source.rs:101-107); `photo_project` copies resolution to the Document (photo_project.rs:36).
  - Document Properties: extend shared `DocumentInfo::describe` (document_info.rs:46-101), used by Web/Android/Apple/Windows. GTK duplicates it in files/properties.rs.
- "Update orientation tags" is already an invariant: orientation is baked into pixels and output writes Orientation 1 (metadata.rs:103-129).
- **Constraints to cite:**
  - Store blobs as binary payloads like ICC and proof profiles (lib.rs:1283-1286; project-format.md:65-75), not JSON. There is a 64 MiB metadata limit (project-format.md:99).
  - The project intentionally excludes source filenames (project-format.md:56). XMP often contains paths, so define a policy.
  - HDR JPEG already emits its own XMP APP1 (jpeg_container.rs:445-510). User XMP must be merged into it, never a competing packet (hdr-export-proposal.md:100-102).
  - Multi-source documents need a rule for whose metadata is exported.
  - A .capy version step must keep reading v6/v7 and recovery snapshots (project-format.md:83-88).
- **Correction:** replace "Today" with the generated-EXIF policy above and add these constraints.

**IO-2 WebP and SDR AVIF export: Wrong/imprecise + Overlooked.**
- A pure-Rust **lossless-only (VP8L)** WebP encoder with ICC/EXIF/XMP chunk support is already in the dependency graph: vendored image-webp encoder.rs:620-676, already used in tests (raster_tests.rs:255-272).
  - It needs the whole RGBA buffer, has no cancellation, and the vendor memory patch covers decoder files only (image-webp-memory.patch; vendored-code-audit.md:183-206).
  - Lossy WebP has no dependency.
- AVIF:
  - "AVIF export exists for HDR only": the mux `assemble` *requires* a gain map (mux.rs:225-235), writes CICP nclx only (mux.rs:131-134), and uses a fixed 12-bit BT.2020/sRGB-transfer base (encode.rs:1-27; portable-photo-core.md:618).
  - SDR AVIF needs an optional gain map and ICC `colr/prof` (or restricting to CICP spaces).
  - Latency: ~10 s for 2 MP on desktop, 11.5 s for 1 MP on a tablet (portable-photo-core.md:91-96).
- **Correction:** "Lossless WebP is available now via image-webp (vendor audit plus encode admission via `PhotoMemoryBudget.encode_bytes`, memory.rs:9-24). Lossy WebP needs a new pure-Rust encoder. SDR AVIF reuses the rav1e/mux path with the noted changes. Other hosts qualify separately (color-management.md:317-318)."

**IO-3 Export sizing, sharpening, re-export: Partial.**
- Already present:
  - `ExportPresets::remember` stores the last successful recipe per destination (export_presets.rs:107-118; GTK export.rs:1136-1143).
  - The last export folder is remembered (chooser.rs:6-11, 51).
  - Missing: the last destination index and target file.
- Constraints:
  - Export refuses to overwrite the editable drawing (export.rs:1070).
  - Web download fallback cannot rewrite a file (documents.js:86-104).
  - Resampling is CPU row-streaming, area-reduction (resize.rs:1-3; `RowResampler`, lib.rs:9). Sharpening must be a bounded row-window stage.
  - HDR outputs build gain maps at final size, so sharpening must feed both renditions (hdr-export-proposal.md:101, 120).
  - No size estimate exists: previews are 220×160 (preview.rs:47-61) and "JPEG compression artifacts are not previewed" (export.rs:871).
  - New `ExportSize` variants change the `CAPYPRESETS\x01` file (export_presets.rs:243-249) and hand-built host JSON (export-controls.js:57, ExportDialog.kt:44).
  - `ExportDraftAction` has no Size/Resolution action (export.rs:329-337).
- **Correction:** cite the above; add a Size draft action to shared Rust.

**IO-4 Batch processing: Overlooked infra + missing dependencies.**
- Reuse:
  - `read_import` / `PhotoOpenPolicy` (import_policy.rs:93-121).
  - `DecodeLimits`.
  - Snapshot capture with `CaptureControl` cancellation and row progress (snapshot.rs:27-31, 127-136; GTK progress export.rs:1088-1097).
  - `ExportPresets`.
- Semantics: `ImageImportBatch` is all-or-nothing (import_policy.rs:127-257), the wrong semantics for batch. The per-file-failure precedent is Web/Android batch Open (document-tabs-progress.md:180-201).
- Blockers:
  - The missing-profile policy `Ask` prompts (import_policy.rs:49-51); batch needs a non-interactive rule.
  - Effect presets (ADJ-10) and LUTs (ADJ-5) don't exist.
  - No host has a folder picker (no `showDirectoryPicker`, `OPEN_DOCUMENT_TREE` or `select_folder` anywhere).

**IO-5 Export layers and selection: Partial infra.**
- Export renders a `Project` snapshot (snapshot.rs:127-136), so per-layer export = project copy with visibility altered. Reuse Solo's subtree, clipping-base and parent logic (art_layers.rs:1082-1117), but not its history edit.
- Selection-bounds export needs output cropping, which is absent (`set_output_extent` only resizes, output.rs:144-150); this overlaps GEO-1/3.
- Needs a folder picker (see IO-4).

**IO-6 RAW hand-off and Open as Layers: Designed + Overlooked.**
- The hand-off is already specified (color-management.md:392-395: 16-bit TIFF).
- "Register as external editor" is largely done:
  - Linux .desktop registers image/tiff etc. with `Exec=capycanvas %F` (art.capycanvas.CapyCanvas.desktop:6, 12), with serial launch handling (files/launch.rs).
  - Android VIEW/SEND/SEND_MULTIPLE (AndroidManifest.xml:14-30).
  - Web `file_handlers` including TIFF (package.mjs:238).
  - macOS registers photos as Viewer/Alternate (macOS Info.plist:19-33).
  - Windows has none.
- **Round-trip conflict:** Save never adopts a photo's location (import_policy.rs:1-2, 41-46; color-management.md:152-153). "Edit In" round-trip must be Export over the handed-off TIFF.
- "Open as Layers": Import already takes multiple files, one layer each, with batch handles and one undo (image-open-import-proposal.md:85, 381; place.rs:106 `open_multiple`). What remains is only "new document from files". Stacking needs native scale ("Original Size (100%)", proposal.md:84) rather than fit.

---

### D. Tweaks

- **T-15: Wrong premise + Partial.**
  - Dispatch errors already surface on GTK (red notice, workspace.rs:2064-2081), Web (7 s status, app.js:95-101, 855) and Android (modal AlertDialog, Documents.kt:354-357).
  - A shared blocked-cursor glyph exists (session.rs:607-647, cursor.rs:183-185).
  - Reason strings exist (`mask_brush_reason`, selection_masks.rs:623, projected at :1100).
  - Real gaps:
    - Pen refusals return `Result<(), PenEvent>` with no text (session.rs:3378).
    - Presentation differs per host.
  - **Correction:** "Add a shared transient-notice model (precedent: core-authored `ShortcutCapture.notice`, shortcuts.rs:190-193) and pen-refusal reasons at the cursor."
- **T-16: Constraint missing.**
  - Changing the Photo default requires keeping the exact prior layout as a `legacy_*_layout` and a migration entry (layout_presets.rs:143-262; manager_migration.rs:72-84; default-workspaces.md:176-178).
  - New Panels need string IDs (layout.rs:635-690) and per-platform availability.
  - Photo currently includes Diagnostics, i.e. renderer telemetry plus stroke recording (layout_presets.rs:39-43; default-workspaces.md:36). That is the natural slot for Histogram/Info.
- **T-17: Partial.**
  - GTK already shows "Output: W×H px / cm · ppi" (export.rs:293-308) under "Resolution metadata: From master/Custom/Omit" (:218-230).
  - Web and Android lack it and use different labels (export-controls.js:24; ExportDialog.kt:139).
  - ICC is always embedded; there is no option (export.rs:147-156). Export notes are GTK-authored strings (export.rs:700-704).
  - **Correction:** move the print-size and ICC notes into the shared `ExportDraft`.
- **T-18: Overlooked.**
  - Import already does multi-file layers (see IO-6).
  - Multi-file Open already exists on Web (documents.js:78) and Android; GTK Open is single-file (chooser.rs:64-73, files.rs:325).

---

### E. §7 rules and §8 out-of-scope

**§7 should add:**
1. Pure-Rust codec policy (portable-photo-core.md:3-5, 11-24) and the vendored-code audit for any codec change (vendored-code-audit.md).
2. Encode/decode memory admission via `PhotoMemoryBudget` and `DecodeLimits` (memory.rs:9-34; photo.rs:141-160), plus cancellation via `CaptureControl` and `read_*_with_cancel`.
3. "Hosts own dialogs and jobs" (export.rs:1) and file-access ownership (import_policy.rs:1-2), including Web download fallback.
4. Format versioning beyond .capy:
   - `CAPYPRESETS\x01` export presets.
   - Workspace schemas and legacy layouts for new Panels.
   - History is excluded from projects today.
5. The export pixel path lives in layer-color (CPU, row-bounded), not layer-render-wgpu.

**§8:**
- RAW: cite color-management.md:392-395 in addition to float32-hdr-scope.md:73.
- PSD is already on the roadmap (color-management.md:178-179).
- The CMYK/gray delivery claim is Acc (export.rs:243-247, 272-279).

---

### F. Gaps not in the plan

- The zoom readout string is formatted separately on five hosts; its interaction differs (only Android is clickable).
- Navigator button lists are host-coded rather than shared.
- `ExportDraftAction` lacks Size/Resolution/Quality, so hosts hand-build recipe JSON.
- Export notes and the print-size summary are GTK-only strings.
- No folder picker exists on any host.
- Windows has no file associations.
- GTK Document Properties duplicates the shared `DocumentInfo`.
- The color sampler allows only one request in flight.
- Solo is an undoable document edit.
- The name "Show rulers" collides with edge rulers.
- AVIF encode latency makes SDR AVIF slow on large photos and tablets.
