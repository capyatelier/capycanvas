# Implementation audit: selection and clipboard (SEL)

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-25 · baseline `dac76c20`

Read-only implementation audit made by an agent against baseline `dac76c20`. It checks each item in the first draft of the build list against the code, the platform hosts and the design records. "Plan line N" refers to that superseded first draft; the current [research record](../photo-editing-research.md) incorporates the corrections. Line numbers can drift in later commits; verify before relying on one.

---

The biggest miss: the plan treats clipboard, Feather, Border, Transform Selection, Clear Selected and Layer via Copy as new systems. For most of them the engine primitives already exist and only a command is missing.

Two claims are wrong:
- **"Delete clears the whole layer"** (journey 14). No key is bound to Clear Layer by default.
- **T-15 "not only in logs".** Every host already shows `host_error` on screen, except Apple for errors raised during a pen gesture.

**File index.** Line citations below use these short names; the full path is under ``.

| Short name | Full path |
| --- | --- |
| lib.rs | crates/layer-ui/src/lib.rs |
| session.rs | crates/layer-ui/src/session.rs |
| art_layers.rs | crates/layer-ui/src/art_layers.rs |
| selection_masks.rs | crates/layer-ui/src/selection_masks.rs |
| selection_tools.rs | crates/layer-ui/src/selection_tools.rs |
| region_tools.rs | crates/layer-ui/src/region_tools.rs |
| tonal_selection.rs | crates/layer-ui/src/tonal_selection.rs |
| operation.rs | crates/layer-ui/src/operation.rs |
| shortcuts.rs | crates/layer-ui/src/shortcuts.rs |
| application_menu.rs | crates/layer-ui/src/application_menu.rs |
| layout.rs | crates/layer-ui/src/layout.rs |
| layout_presets.rs | crates/layer-ui/src/layout_presets.rs |
| customization.rs | crates/layer-ui/src/customization.rs |
| project_files.rs | crates/layer-ui/src/project_files.rs |
| render/lib.rs | crates/layer-render/src/lib.rs |
| region_requests.rs | crates/layer-render-wgpu/src/region_requests.rs |
| selection_refine.wgsl | crates/layer-render-wgpu/src/selection_refine.wgsl |
| scene.wgsl | crates/layer-render-wgpu/src/scene.wgsl |
| snapshot.rs | crates/layer-render-wgpu/src/snapshot.rs |
| snapshot/output.rs | crates/layer-render-wgpu/src/snapshot/output.rs |
| canvas.rs | crates/layer-engine/src/canvas.rs |
| selection.rs | crates/layer-core/src/selection.rs |
| layers.rs | crates/layer-core/src/layers.rs |
| core/lib.rs | crates/layer-core/src/lib.rs |
| flatten.rs | crates/layer-color/src/flatten.rs |
| INV | docs/ui/selection-command-inventory.md |
| SELTOOLS | docs/development/selection-tools.md |
| TONAL | docs/development/tonal-selection.md |
| SAVED | docs/ui/saved-selections-assessment.md |
| PAINTSEL | docs/ui/paintable-selection-proposal.md |
| IMPORT | docs/ui/image-open-import-proposal.md |
| LCM | docs/history/layers-context-menu-audit.md |

---

### Inventory row "Selection" (plan line 89)
- **Verdict:** Accurate, but it overlooks the existing implementation that the "missing" items can reuse.
- **Evidence:**
  - Feather at creation is capped at 100 px (`SelectionRefinement::MAX_FEATHER`, render/lib.rs:414).
  - Grow/Shrink run through `SelectionRefinement.resize` on `RegionSource::Selection` (selection_masks.rs:831-922). SELTOOLS:183-184 records "Feathering an existing result, border, smooth, and selection-only transforms remain separate work".
  - "Select Similar" is listed as Later in INV:343.
- **Fix:** Cite SELTOOLS:173-184 and INV:343. Add: "The engine already has the GPU feather and the circular grow/shrink; only commands are missing (see SEL-5)."

### Inventory row "Clipboard" (plan line 90)
- **Verdict:** Imprecise.
- **Evidence:**
  - Mask copy/paste is an in-memory `clipboard_mask` inside `LayerInteraction` (art_layers.rs:284, 878-910). LCM:39 calls it a "Session clipboard, not OS image clipboard".
  - There is one `UiSession` per document: `DocumentSessions<UiSession<…>>` in apps/layer-android/native/src/document_tabs.rs:18, apps/layer-apple/native/src/document_tabs.rs:10 and apps/layer-web/src/lib.rs:35. It is reset when a project is adopted (project_files.rs:93). So masks cannot be pasted into another document.
  - Every host only reads images from the clipboard; none writes one:
    - GTK: apps/layer-linux/src/files/place.rs:26-33
    - Web: apps/layer-web/documents.js:60-75
    - Android: apps/layer-android/app/src/main/java/art/capycanvas/ImageImport.kt:126
    - Apple: apps/layer-apple/Shared/Bridge/PhotoClipboard.swift:45-66
    - Windows: apps/layer-windows/DocumentView.cpp:246-262
  - The only clipboard writes are text: apps/layer-web/gpu.js:40 and apps/layer-web/preferences.js:29.
  - Paste Image does open placement handles: `import_sources(interactive=true)` → `begin_layer_placement`, scaled to fit and centred on the canvas (art_layers.rs:664-697).
- **Fix:** Reword to: "Paste Image as Layer (placement handles, source kept at full resolution). Copy/Replace mask uses a per-document, in-memory clipboard. No host writes images to the system clipboard."

### Section 3 lessons (only the ones that touch SEL)
- **"No floating state" — Accurate.** It matches INV:348, where floating selections are Later. However:
  - The verb names differ from the approved inventory: INV:133 and INV:282 use "Copy Selection to New Layer / Cut Selection to New Layer".
  - INV:348 puts "Paste Into/Outside" under Later.
  - **Fix:** Cite INV and reconcile the names and the Paste Into priority.
- **"Explain silent failures" — Partially accurate.**
  - The quoted string exists (art_layers.rs:776).
  - Several paths return with no message at all:
    - Move tool on a locked layer or the Background: `return Ok(())` (art_layers.rs:1773).
    - Fill, Gradient or Figure with no drawing content (art_layers.rs:1774-1778).
    - Region click with no drawing content (region_tools.rs:147-149).
    - Erasing on an alpha-locked layer does nothing (scene.wgsl:189-190).
  - **Fix:** List these silent paths explicitly as the targets of T-15.
- **"Modifier semantics are sacred" — Accurate.**
  - Selection tools already latch Shift/Alt/Ctrl before the gesture; keys pressed during the gesture become constraints (SELTOOLS:32-37, INV:313-316). SEL-3's Alt-drag copy must follow this latch rule.
  - INV:170 requires every persistent mode to be reachable without a keyboard.
- **"Keep transforms re-editable" — Relevant to SEL-1 and T-11.**
  - Ctrl+T on a layer that has a `source` and no selection opens re-editable placement, not a pixel transform (operation.rs:225-230).
  - If Paste creates a source-backed layer, "Ctrl+T transforms it" is lossless for free.
  - **Fix:** State that the paste payload is a `SourceImage` (see SEL-1).

---

### SEL-1 Pixel clipboard
- **Verdict:** Overlooked existing infrastructure, plus claims that are wrong or imprecise.
- **Evidence:**
  - **Capture already exists.** `SnapshotGpu::capture` / `SnapshotRenderer` is a worker-owned, exact-precision composite (snapshot.rs:1-2, 126-137). `flattened_document` produces a document-depth, document-space `SourceImage` with `SourceKind::Rasterized` (flatten.rs:13-45). All five hosts already run it:
    - apps/layer-android/native/src/color_edit.rs:85
    - apps/layer-apple/native/src/project_color.rs:50
    - apps/layer-linux/src/files/color/flatten.rs:16
    - apps/layer-web/src/output.rs:436
    - apps/layer-windows/native/src/document_color.rs:105
  - **PNG output with ICC/depth conversion and HDR→SDR exists** in `SnapshotRenderer::write_png(target, OutputEncoding)` (snapshot/output.rs:153-164). No crop/region parameter exists.
  - **Insertion already exists.** `import_sources` inserts source-backed layers and converts to the document only at render time (art_layers.rs:607-609, 642-697). A non-interactive, handle-free insertion path also exists (`import_layer_source`, art_layers.rs:610-623; IMPORT:164-165).
  - **"Cut" is not the Transform path.** Transform cut = `LayerOperationKind::Transform` with coverage (canvas.rs:473-507). A plain clear maps better onto `paint_operation`, which already turns soft or inverted selection coverage into a layer-local mask (art_layers.rs:1959-1996). But `Fill` is source-over (scene.wgsl:172-195), so an erase kind is needed. The Figure erase mode is the precedent (scene.wgsl:189-190, figures.rs:105).
  - **Copy/Copy Merged mirror the existing source vocabulary.** `RegionSource::{Visible, Editing, Reference}` (art_layers.rs:37-43) is labelled "Sample visible artwork / editing layer / reference layers" (lib.rs:1097-1099). `Document::reference_snapshot()` (layers.rs:635) is ready-made for a reference variant.
  - **Copy source definition is underspecified.** The existing "raw content" rule is: content alpha before layer opacity, masks, effects and clipping (INV:254-259).
  - **Cross-document paste.** The rich clipboard cannot live in `UiSession`, because sessions are per document (see the Clipboard row).
  - **Ctrl+T is unavailable on Web** (shortcuts.rs:119-125).
  - **Ctrl+V is already bound** to PasteImage (shortcuts.rs:258).
  - **Priority conflict.** "Paste Into" is Later in INV:348; the plan makes it P1.
  - **Colour conversion.** "Converts color space (not bit depth)" contradicts the retained-source model: sources keep their own depth and profile.
- **Fix:**
  - Where: replace "a region capture in layer-render-wgpu" with "reuse `SnapshotRenderer` / `flattened_document` (add a crop), and `write_png` for the system clipboard PNG".
  - Represent copies as a `SourceImage` (Rasterized) and paste through `import_sources` / `place_layer_sources`.
  - Keep the rich clipboard at window level (next to `DocumentSessions`), not in `LayerInteraction`.
  - Define Copy as "raw content, per INV:254-259" or state explicitly that masks and opacity are included.
  - Implement Cut and Clear as a `paint_operation` with a new erase kind.
  - Resolve the Ctrl+V collision with PasteImage (merge the two commands).
  - Mark Paste Into as a deliberate override of INV:348.
  - Note that Ctrl+T is unavailable on Web.

### SEL-2 Layer via Copy / Layer via Cut
- **Verdict:** Already specified in an existing design doc, and largely buildable from existing primitives.
- **Evidence:**
  - Already specified: INV:133 and INV:282 ("Copy Selection to New Layer; Cut Selection to New Layer | Next; preserve document-space placement…single undoable operation").
  - `LayerAction::Duplicate` copies the raster, `source`, pending operations, mask, parent and clip flag through shared `Arc`s and inserts at the source's index (art_layers.rs:1127-1183). Layer via Copy = Duplicate + erase outside the selection, in one `Edit::Batch`. No pixel readback is needed and full-resolution photo sources are kept.
  - Reselect comes for free: `layer_edit` stores the previous selection whenever an edit clears it (art_layers.rs:714-721).
  - Clipping insertion rule already exists: `Document::clipping_stack_top` ("never between its clipping base and attached layers", layers.rs:730-743). Inserting at the source index could make the copy the new base for the source's clipped layers.
  - Duplicate has no `CommandId`; it is only in the layer menu (art_layers.rs:1563-1574). So the "no selection → Ctrl+J duplicates" fallback needs a new command.
  - There is an existing Layer › New submenu (art_layers.rs:1680).
  - The "canvas selection menu" is the Select menu model, reached through "Selection Actions…" (selection_masks.rs:226-229) on:
    - Android: ToolPanels.kt:134
    - Apple: ToolControls.swift:145
    - Web: editor-panels.js:116
    - Windows: ToolView.cpp:219
    - GTK: no such button found; GTK uses the Select application menu (application_menu.rs:93-100).
  - `LayerKind` also includes `ImportedImage` and `AiSuggestion` (core/lib.rs:162-171), which the plan does not handle.
- **Fix:**
  - Cite INV and adopt its names, or justify the rename.
  - Implement as Duplicate + clear-outside batch.
  - Specify insertion using `clipping_stack_top`.
  - Add a `DuplicateLayer` CommandId.
  - "Mask restricted to the selection" is unnecessary: clearing outside on the copy keeps the visible result.
  - Name the handling of `ImportedImage`.

### SEL-3 Selection-aware Move
- **Verdict:** Accurate claim ("Move ignores selection"), overlooked infrastructure, one missing mechanism.
- **Evidence:**
  - Move changes only the layer or mask offset (`move_target_edit`, layers.rs:1108-1128; called from art_layers.rs:1798-1816).
  - Selected-pixel cut+place with the selection moving along already exists: `begin_transform` with a selection (operation.rs:216-291) and `commit_transform` (canvas.rs:473-507), which moves the selection via `display_selection`. It also works on layer masks (INV:360).
  - Copy mode does not exist: `ImageTransform` is only affine + interpolation, always "cut + placement" (core affine.rs:13-23). Alt-drag copy needs a new flag.
  - Capy's Move never picks layers automatically; it always acts on the active layer (layers.rs:1109-1112).
  - Move silently ignores locked layers and the Background (art_layers.rs:1773).
  - Photo workspace's default tool is Move (layout_presets.rs:31, test at :1210).
- **Fix:**
  - Implement as "Move drag drives a translation-only transform transaction (operation.rs:216) committed by `commit_transform`".
  - Add a copy flag to `ImageTransform`/`Transform`.
  - Note that GIMP's auto-pick issue does not apply to Capy.
  - Require a visible Alt-copy toggle (INV:170).
  - Call out the behaviour change for Photo's default tool.

### SEL-4 Clear Selected / Clear Outside
- **Verdict:** Accurate that it is "Next" in INV; "the only clear command" is imprecise; overlooked infrastructure.
- **Evidence:**
  - `LayerAction::Clear` wipes the whole raster and also drops `asset` and `source` (art_layers.rs:1232-1240).
  - It ignores the selection: it is enabled whether or not a selection exists (session.rs:1993-1999) and the edit never reads the selection.
  - It ignores alpha lock: its enable flag is `controls.alpha_lock`, which only means "paint and unlocked" (art_layers.rs:1424, 104).
  - INV:359: "`LayerAction::Clear`…must never back Clear Selected Pixels or a selection Delete shortcut."
  - Other clear commands exist:
    - Hide all / Reveal all on masks (`ClearMask`, art_layers.rs:1545-1546)
    - Clear Selection Coverage (lib.rs:1083)
    - `SelectionAction::ClearLayer{full}` for stored masks (selection_masks.rs:1070-1077)
  - Clear Outside is just the same operation with `selection.inverted ^= true`; `paint_operation` already handles inverted selections (art_layers.rs:1974-1976).
  - Mask-editing restrictions already exist (session.rs:1939-1940, art_layers.rs:774-777).
- **Fix:**
  - "Today the only artwork clear, Clear layer, ignores the selection and alpha lock and discards a placed photo's source."
  - Cite INV:359.
  - Implement via `paint_operation` + an erase kind.
  - Define the active-layer-mask target, e.g. `ClearMask` restricted to the selection.
  - Define alpha-lock behaviour; the existing erase is a no-op under alpha lock.

### SEL-5 Feather / Border / Smooth / Transform Selection
- **Verdict:** Partially implemented already. The plan misses that Feather and Transform Selection are nearly free.
- **Evidence:**
  - **Feather:** `SelectionRefinement` has a `feather` field, and `is_valid` allows feather when resize is 0 (render/lib.rs:402-421). Feather = the Grow/Shrink `ApplyResize` request with `resize: 0, feather: r` (selection_masks.rs:900-921). Cap is 100 px (render/lib.rs:414).
  - **Dialog UI:** Grow/Shrink uses a generic `SelectionResizeView{title, radius, numeric}` (selection_masks.rs:93-106, 831-889). All six hosts already render it:
    - apps/layer-linux/src/selection_masks.rs
    - apps/layer-web/selection-masks.js
    - apps/layer-android/app/src/main/java/art/capycanvas/SelectionMasks.kt
    - apps/layer-apple/Shared/Editor/SelectionControls.swift
    - apps/layer-windows/SelectionDialog.cpp
  - **Border:** two chained existing passes — resize +r, then resize −r with `mode: Subtract, previous` (combine: `max(0, old−value)`, selection_refine.wgsl:153-158).
  - **Smooth:** `RegionRefinement{gap_closing, smoothing}` exists (render/lib.rs:436-458), but it is not applied when the source is `RegionSource::Selection` (region_requests.rs:87-92, 129-145). It needs plumbing, or a close/open made from resize passes.
  - **Transform Selection:** `Selection::transformed` / `translated` compose affine metadata with no resampling (selection.rs:314-335). Preview via `set_selection_display` (canvas.rs:509-532). The handles, pose and numeric X/Y/W/H/angle come from operation.rs (13-60, 360-470). INV:360 requires a separate coverage transaction.
  - **Live preview:** INV:94 says "future refinements should add live preview"; the tonal draft + `engine.refine_selection` amend pattern exists (tonal_selection.rs:232-330, canvas.rs:755-758).
  - Stored-mask targets are specified as Next: INV:189-190, INV:216; SAVED:81-86.
- **Fix:**
  - Rewrite as: "Generalize `ResizeDraft` into {Grow, Shrink, Feather, Border, Smooth}. Feather and Border reuse `SelectionRefinement`; Smooth needs `RegionRefinement` wired for Selection sources."
  - Transform Selection = operation.rs handles over `Selection::transformed`.
  - Cite SELTOOLS:183-184 and SAVED:81-86.
  - Include Quick Mask and Selection Layer targets.
  - State the 100 px feather cap.

### SEL-6 Refine Edge
- **Verdict:** Overlooked existing infrastructure; the priority conflicts with INV.
- **Evidence:**
  - INV:343 lists "edge snapping/refinement" as Later.
  - Global controls already exist:
    - Feather: `SelectionRefinement.feather`
    - Shift Edge: `resize` ±128 px (render/lib.rs:405-415)
    - Smooth: `RegionRefinement.smoothing` / gap closing, exposed for Wand (region_tools.rs:46-107)
  - Outputs already exist:
    - Selection: `Edit::SetSelection`
    - Layer mask: `LayerAction::MaskSelection` (art_layers.rs:783-791)
    - New Selection Layer: `SelectionAction::NewLayer{save_current}` (selection_masks.rs:923-956)
  - The plan's "Where" is wrong: the refinement code lives in render/lib.rs:436-458 and the GPU `region_refine.wgsl` / `selection_refine.wgsl`; region_tools.rs only holds the controls.
- **Fix:** Cite INV:343. List the reused controls and outputs. Correct the "Where".

### SEL-7 Edge-aware quick selection
- **Verdict:** Accurate as a gap.
- **Evidence:** It is Later in INV:343. The Paint selection UI and pipeline exist (painted_selections.rs; SELTOOLS:112-121).
- **Fix:** Cite INV:343 as an explicit priority change.

### SEL-8 Channel selections / Color to Alpha
- **Verdict:** Already in the backlog; overlooked infrastructure; internally inconsistent with T-10.
- **Evidence:**
  - INV:347 lists "selection from luminance/color channels" as Later.
  - `RegionSource::Tonal` already computes per-pixel luminance on the GPU with soft shoulders (render/lib.rs:358-371; TONAL:87-92). `RegionSource::Coverage` is the analogous raw-channel source (render/lib.rs:352-353). A channel source slots in beside them.
  - SEL-8 says "Select Similar… P2", but T-10 (milestone M5) adds Select Similar.
- **Fix:** Cite INV:347. Implement as a new `RegionSource` variant. Reconcile the Select Similar priority.

### SEL-9 Select Subject / Sky
- **Verdict:** Accurate; the "principle" is uncited.
- **Evidence:** INV:343 lists subject detection as Later. The local-only principle is stated in README.md:31-33 ("Drawing and editing happen locally on your device").
- **Fix:** Cite README.md:31-33 and INV:343.

### T-2 Delete/Backspace clears the selection; rename "Clear layer"
- **Verdict:** Wrong or imprecise claim.
- **Evidence:**
  - Plain Delete and Backspace have no default binding (shortcuts.rs:209-267). Shift+Backspace is Fill selection (shortcuts.rs:237).
  - Clear layer has no default key. It is reached from:
    - the Edit menu (lib.rs:228)
    - the layer menu (art_layers.rs:1663)
    - the default Commands toolbar button (layout.rs:1471-1483); Photo removes that button (layout_presets.rs:318-331).
  - Journey 14's "Delete clears the whole layer" is also false.
  - "Clears the selection" is ambiguous: it would read as Deselect.
  - Naming collisions to avoid:
    - "Clear layer" (lib.rs:1119), described as "Erase all artwork on the editing layer" (customization.rs:1050)
    - "Clear Selection Coverage" (lib.rs:1083)
    - `SelectionAction::ClearLayer` (selection_masks.rs:84)
  - Delete is used in focused host contexts:
    - header item removal: apps/layer-web/header.js:392, HeaderInput.kt:93, EditorHeader.swift:133
    - drawing tab close: apps/layer-web/drawing-tabs.js:113
  - `DeleteRuler` (session.rs:1975-1980) has no key but is a natural Delete target.
- **Fix:**
  - "Bind Delete/Backspace to a new Clear Selected Pixels command (INV:279). Keep Clear Layer unbound and unchanged (INV:359). Rename it, e.g. 'Clear Entire Layer'."
  - Define Delete precedence against a selected ruler and focused header/tab items.
  - Fix journey 14's wording.

### T-9 Tonal options bar actions
- **Verdict:** Contradicts an approved design; partially redundant.
- **Evidence:**
  - TONAL:41-43: "There is no Apply/Cancel workflow, source selector, range manager, destination label, or selection-actions menu in this panel." Enforced by a test in apps/layer-apple/Shared/Tests/TonalSelectionChecks.swift:19.
  - Tonal can already write straight into a Selection Layer (TONAL:61-66).
  - `SaveSelectionLayer` is an existing command (lib.rs:574; Select menu at application_menu.rs:95).
  - The Tool Options row is `tonal_extra` (tonal_selection.rs:404-425).
  - "Add adjustment with this mask" depends on T-1.
- **Fix:** Cite TONAL:41-43 and justify reversing it. Alternatively, make it a workspace default toolbar entry for the existing command instead of changing the tonal panel. Mark the dependency on T-1.

### T-10 Tolerance preview and Select Similar
- **Verdict:** Accurate that no live preview exists; overlooked reusable infrastructure; internal inconsistency.
- **Evidence:**
  - Editing tolerance calls `cancel()` and never re-runs (region_tools.rs:88-107).
  - Tolerance is shared with the Fill tool (region_tools.rs:7-9).
  - Tonal's "result applies immediately; later numeric refinements amend that operation" (TONAL:21-23, 43-47) is implemented by tonal_selection.rs:232-330 and `engine.refine_selection` (canvas.rs:755-758). Reuse it: re-run the same seed from the saved baseline and amend one undo step.
  - Select by Color is already a non-contiguous "similar" selection from a click (art_layers.rs:47; SELTOOLS:39-43).
  - Select Similar is Later (INV:343) and P2 in SEL-8.
- **Fix:** Point to the tonal draft/refine mechanism. Clarify the difference from Select by Color. Pick one priority for Select Similar.

### T-11 Paste Image follows SEL-1
- **Verdict:** Imprecise, and it modifies an approved design.
- **Evidence:**
  - IMPORT:55-56 and IMPORT:80: agreed behaviour is that Import/Paste add layers "centered on the canvas, with placement handles active".
  - Paste is host-driven: `DocumentRequest::Paste` (session.rs:3947-3949) reaches five host readers (listed under the Clipboard row). Because no host writes images, nothing can tell an "in-app copy" from an external image. A Capy ownership marker, or a window-level rich clipboard checked before the host request, is required.
  - Web already reads custom `web <mime>` clipboard formats (apps/layer-web/documents.js:63), a precedent for a private payload.
  - T-11 says "paste in place", but SEL-1 says "copied position if visible, else centred", and SEL-1 has a separate Paste in Place.
- **Fix:** Cite IMPORT:80 as the baseline being changed. Specify where the ownership check lives. Align the position rule with SEL-1.

### T-15 Show refusal reasons
- **Verdict:** Wrong or imprecise ("not only in logs"); overlooked existing infrastructure.
- **Evidence:**
  - Errors raised during a gesture go to `state.host_error` (lib.rs:1274; session.rs:3479-3483). Current display per host:

    | Host | How host_error is shown | Evidence |
    | --- | --- | --- |
    | GTK | Persistent error label | apps/layer-linux/src/workspace.rs:1071-1073, 2064-2073 |
    | Web | Status text, cleared after 7 s | apps/layer-web/app.js:95-101, 857 |
    | Android | Modal AlertDialog "Could not complete action" | apps/layer-android/app/src/main/java/art/capycanvas/Documents.kt:354-358 |
    | Windows | Status text | apps/layer-windows/CanvasWindow.cpp:1278-1287 |
    | Apple | Not shown: no non-test Swift reads `host_error`; the "Canvas error" panel shows only dispatch and snapshot errors | apps/layer-apple/Shared/Editor/EditorView.swift:60-72 |

  - The region-tool failure (the lighthouse hint) travels as a `frame()` error, not through `host_error` (region_tools.rs:175-182, 244-247; session.rs:3830).
  - At-cursor infrastructure exists: the blocked-cursor glyph (session.rs:603-611, 643-647).
  - One published reason exists: `MaskEditingView.reason` / `mask_brush_reason` (selection_masks.rs:108-117, 623-637).
  - `CommandState` has no field for why a command is disabled (lib.rs:1156-1167), although INV:45-46 requires "Disable … with a useful reason".
- **Fix:**
  - Reframe as: (a) silent no-op paths (see Section 3); (b) inconsistent presentation (modal on Android, missing on Apple); (c) no reason field on commands.
  - Where: `host_error`, the blocked cursor, and a new reason field on `CommandState`.

---

### Journey text corrections
- **J14** "Delete clears the whole layer rather than the selection" is wrong (see T-2). Workarounds exist: the Eraser is clipped by the selection; Figure with Transparent paint erases (figures.rs:105); or Mask: hide selection + Apply mask.
- **J26** "Nothing else in this workflow exists" is imprecise. Duplicate → "Mask: reveal selection" → "Apply mask to layer" (art_layers.rs:1542-1549, 1563-1574) is a 3-step Layer via Copy. Only cross-document copy is truly blocked.
- **J11** "No feather after the fact" is accurate, but the GPU operation exists (see SEL-5).

### Shortcut conflicts
New defaults automatically give way to chords the user has already assigned (shortcuts.rs:496-501). Two identical defaults are resolved silently by `CommandId::ALL` order, with no warning (shortcuts.rs:527-541).

| Chord | Plan use | Current binding | Status |
| --- | --- | --- | --- |
| Ctrl+C | Copy | Canvas: none. Native text uses `TextEditAction::Copy` (shortcuts.rs:24-42) | Free; text focus must win (INV:311) |
| Ctrl+X | Cut | Same as Ctrl+C | Free |
| Ctrl+Shift+C | Copy Merged | none | Free |
| Ctrl+V | Paste | **PasteImage** (shortcuts.rs:258) | **Conflict**: merge the commands |
| Ctrl+Shift+V | Paste in Place | none | Free |
| Ctrl+Alt+Shift+V | Paste Into | none | Free |
| Ctrl+J | Layer via Copy | none (plain J = `tools.blend`, :215) | Free; needs a new CommandId |
| Ctrl+Shift+J | Layer via Cut | none | Free |
| Shift+F6 | Feather | none (F1-F24 are valid keys, :81-84) | Free |
| Ctrl+E | Merge Down | none (plain E = Eraser, :216) | Free |
| Ctrl+Shift+E | Merge Visible | **ExportDocument** (:261) | **Conflict** |
| Ctrl+Alt+Shift+E | Stamp Visible | none | Free |
| Ctrl+1 | 100% zoom | none | Free |
| Ctrl+0 | Fit (kept) | FitCanvas (:229) | Consistent |
| Ctrl+Shift+T | Transform Again | none | Free on desktop; **unavailable on Web**: `KeyChord::available` rejects Ctrl+(Shift+)T (:119-125) |
| Ctrl+T | "transforms it" (SEL-1) | ScaleRotate (:219) | Also unavailable on Web |
| Delete / Backspace | Clear Selected | none (Shift+Backspace = FillSelection, :237) | Free; define precedence (see T-2) |

### Additional gaps not in the plan
1. **Clipboard scope is per document.** Mask clipboard and any future pixel clipboard live in the per-document `UiSession` (see the Clipboard row). Cross-document paste needs window-level storage.
2. **Android cannot write an image clipboard yet.** It has no FileProvider/ContentProvider (grep of the Android `src/main` sources for FileProvider, ContentProvider and `<provider`: nothing found). `ClipData` images need a content URI.
3. **Copy/Cut/Clear with a layer mask active.** Clear Layer is disabled (session.rs:1996). `ClearMask` exists as the mask-side counterpart (art_layers.rs:1545-1546). The plan does not define this target.
4. **Alpha lock.** Clear Layer ignores it (art_layers.rs:1232-1240), while the erase shader is a no-op under it (scene.wgsl:189-190). Cut/Clear need a rule and a message.
5. **Registration conventions to cite:**
   - `CommandId` enum and fixed-size `ALL: [Self; 125]` (lib.rs:521-651, 859)
   - `available_on` (lib.rs:653-721)
   - labels (lib.rs:1021+) and descriptions (customization.rs:~998-1051)
   - enablement (session.rs:1930-2030) and dispatch (session.rs:4189-4202)
   - `EDIT_MENU` (lib.rs:223-233), Select menu (application_menu.rs:93-100), layer menu New submenu (art_layers.rs:1680)
   - distinct-SVG-icon test for toolbar commands (lib.rs:~1595-1625)
   - no-placeholder rule (INV:45-46)
   - Photo-workspace additions go through layout_presets.rs.
6. **Quick Mask restriction.** PAINTSEL:244-245 disables destructive artwork commands during Quick Mask; this applies to every new SEL command.
7. **Outdated doc.** LCM:41 still says selections are polygon contours only; `SelectionShape::Pixels` exists (selection.rs:204-209). Update it or don't cite it.
8. **GTK "Selection Actions…" button.** grep of apps/layer-linux/src found none, while the other four hosts have it. INV:164 marks it Core. This affects SEL-2's "canvas selection menu" placement.
