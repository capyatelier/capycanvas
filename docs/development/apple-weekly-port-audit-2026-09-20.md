# macOS and iPadOS weekly port audit — 2026-09-20

## Scope and checkout state

Audited the seven-day change inventory starting **2026-09-12 23:53:27 America/Los_Angeles**, through fetched `origin/main` **`5004f1bead31f9b3359b34b5713919fdedcd73ff`** (2026-09-19 23:37:10 PDT). This includes 564 commits with merges, **478 non-merge commits**, and **232 non-merge commits touching GTK, Android, or Web**. Shared Rust changes and Apple follow-up commits were included when determining whether a feature needed a host port.

The primary checkout was at `c1472101`, with substantial uncommitted Apple HDR work. Fetch succeeded over HTTPS; the fast-forward could not proceed because it would overwrite local changes. `origin/main` was updated, and its exact revision was checked out separately at `/private/tmp/capycanvas-week-audit-20260920`. The primary checkout remains 77 commits behind that revision. Existing production files were left unchanged.

This report distinguishes **committed main** from the **existing local Apple port**. Counting the local implementation as absent would substantially overstate the remaining work. Source links marked “main” are pinned to the reviewed revision; relative Apple source links refer to the local working tree inspected during this audit.

Method: enumerate the full commit/file inventory; group changes into user-facing features and shared infrastructure; trace reference-host entry points, shared ownership, and Apple adapters; inspect retained native acceptance evidence; run the latest Web implementation where visual behavior needed confirmation. This is a feature-port audit with targeted runtime checks, not new physical-device certification or a line-by-line review of every vendored codec.

- [Complete non-merge commit inventory](../../artifacts/weekly-port-audit-2026-09-20/commits.md)
- [Machine-readable commits and changed files](../../artifacts/weekly-port-audit-2026-09-20/commits.json)

## Results at a glance

All remaining gaps below affect **both macOS and iPadOS**, because the relevant SwiftUI views and Rust bridge are shared.

| Feature | Committed Apple main | Existing local Apple work | Remaining priority |
| --- | --- | --- | --- |
| Multiple drawings in one window | Missing | Missing | High |
| F16 authoring, EDR, live Proof, intensity, PQ PNG | Missing newer HDR workflow | Substantially implemented, with retained Mac/iPad acceptance evidence | Integrate before reassessing |
| Float32 authoring and OpenEXR delivery | Missing host workflow | Still incomplete | High |
| JPEG/AVIF HDR gain-map delivery and decoded comparisons | Explicitly disabled | Partial writer wiring; incomplete UI, previews, and file types | High |
| Retained GPU local-tone guides and interaction scheduling | No HDR worker | Older worker contract | Medium |
| HDR Edit Color context and reference layout | Older SDR form | Partial HDR form, behind reference behavior | Medium |
| Actual HDR/SDR display status and details | Missing | Missing | Medium |
| Translucent docking body target | Opaque fill | Same | Low |
| Protected Paper guidance introduced on GTK | Missing | Same | Low |

## 1. Drawing tabs and per-drawing ownership are absent

GTK added tabs in `6a997a3d`; `f5400869` introduced shared `DocumentSessions` and safe parking; Web followed in `a3f4c196`, Android in `db92d0da`. Later commits cover ordered external opens, compact-selector contacts, and close lifetime (`e8df2d95`, `4be6cf2c`, `273fe577`, `e32ded49`).

Apple still creates one replacement-style `NativeHost` per editor (`native/src/lib.rs`, `set_document_replacement(true)`). [EditorHeader.swift](../../apps/layer-apple/Shared/Editor/EditorHeader.swift) renders only `state["tabs"][0]` as a title at lines 277–280. [ProjectFiles.swift](../../apps/layer-apple/Shared/Editor/ProjectFiles.swift) enables multi-selection only for photo placement, not ordinary Open (lines 483–487). Existing photo drops target the canvas/layers; there is no title-area open-as-drawing route.

The missing port includes:

- Independent documents, histories, camera/tool state, save/close decisions, and recovery within a window.
- Adaptive tab strip and compact selector, reorder history, cycle/menu commands, and native focus/accessibility behavior.
- Shared inactive-session ownership, exact GPU parking, and disk-backed spill.
- Ordered multi-file Open and title-area drops that open drawings instead of placing layers.

Existing native multi-window support is implemented. It does not provide these new same-window workflows. Reuse the [main shared document sessions](https://github.com/capyatelier/capycanvas/blob/5004f1bead31f9b3359b34b5713919fdedcd73ff/crates/layer-ui/src/document_sessions.rs) and native host lifecycle; do not recreate document ownership in Swift.

The drag convention remains binding: visible title/tab bars drag after movement slop without a hold; compact list-row bodies hold for touch/pen; explicit grips drag immediately. The new tab implementation needs those distinct targets.

## 2. The local HDR port needs integration, not reimplementation

On committed main, [Apple project loading](https://github.com/capyatelier/capycanvas/blob/5004f1bead31f9b3359b34b5713919fdedcd73ff/apps/layer-apple/native/src/project.rs#L512) still calls `require_sdr_host`, and [export](https://github.com/capyatelier/capycanvas/blob/5004f1bead31f9b3359b34b5713919fdedcd73ff/apps/layer-apple/native/src/project_export.rs#L73) rejects all HDR formats and EXR. Its creation and display adapters likewise predate the new HDR workflow.

The local changes already add F16 creation/conversion, extended-linear EDR presentation, docked Off/SDR/Print Proof, the shared glass dial, intensity editing, HDR inspection axes, a local-tone worker, and PQ PNG delivery. The [local validation record](../history/color-management-apple-m4-validation.md) includes release builds and actual Mac/physical-iPad HDR and SDR journeys. Those existing results were inspected, not rerun.

There are concrete integration conflicts with newer shared APIs:

- [project_preferences.rs](../../apps/layer-apple/native/src/project_preferences.rs), lines 32 and 42, calls `gainmap_available()`, removed by `5ef1152f`.
- [project_export.rs](../../apps/layer-apple/native/src/project_export.rs), lines 76–83, has no `ExportFormat::Exr` match arm.
- The local color-form request uses string `change_intensity`; current shared code separates numeric `change_intensity` from `change_intensity_text` and adds document-depth/rendition context.

Consequently, the local patch cannot be treated as a qualified port of current main until the shared changes and native callers are reconciled. No merged-overlay build was attempted during this audit.

## 3. Float32 and OpenEXR remain incomplete

Reference changes: `537464b7`, `68903c46`, `bc5d3cec`, `5750c234`.

Even with the local F16 work:

- [NewDrawingForm.swift](../../apps/layer-apple/Shared/Editor/NewDrawingForm.swift), lines 77–78, and [DocumentColorForm.swift](../../apps/layer-apple/Shared/Editor/DocumentColorForm.swift), line 114, expose U8/U16/F16 only.
- [HistogramPresentation.swift](../../apps/layer-apple/Shared/Editor/HistogramPresentation.swift), line 36, falls through to “8-bit” for F32.
- [ExportForm.swift](../../apps/layer-apple/Shared/Editor/ExportForm.swift), line 61, exposes the output-range selector only for F16; its format/depth descriptions lack EXR/F32 handling.
- The native writer has no EXR arm, and the native save-type mapping defaults unknown formats to PNG.
- Edit Color does not pass document depth, so it cannot use the latest depth-specific range validation/readouts correctly.

The shared renderer and EXR reader already exist. Apple derives photo import types from shared `photo::formats()` through [ProjectFileIO.swift](../../apps/layer-apple/Shared/Bridge/ProjectFileIO.swift); this is **not evidence of a missing EXR decoder**. What remains is complete host authoring/conversion, truthful inspection, export selection/type/writer wiring, and end-to-end F32 import/save/reopen/export qualification.

## 4. Gain-map export is not an end-to-end Apple workflow

Reference changes: `ad8d2f57`, `e09951ae`, `a7b888a6`, `1379462f`, `5ef1152f`, plus later AVIF quality/performance fixes. GTK, Web, and Android now use the shared Rust codecs.

The local Apple writer already calls `write_gainmap`, but simply enabling the formats would expose several defects:

- Obsolete availability filtering must be removed/reconciled.
- `ExportForm` describes every HDR choice as 16-bit BT.2020 PQ with preserved transparency. It lacks the gain-map-specific range choices, explanatory labels, and quality controls; quality is editable only for ordinary `Jpeg`.
- `ProjectFiles.swift:265` chooses `.jpeg` only for `Jpeg`, `.tiff` only for `Tiff`, and `.png` for everything else. HDR JPEG, AVIF, and EXR would therefore receive the wrong native save type/extension routing.
- `project_export.rs:52` uses `preview_hdr_output` for all HDR formats. That is not the reference encode/decode comparison for the actual gain-map delivery.
- There is no comparison selector between decoded HDR reconstruction and the encoded SDR base.

Port the format-specific options, preflight, save types, preset behavior, and actual decoded comparisons together. See [main Web output.rs](https://github.com/capyatelier/capycanvas/blob/5004f1bead31f9b3359b34b5713919fdedcd73ff/apps/layer-web/src/output.rs#L376) and [export-controls.js](https://github.com/capyatelier/capycanvas/blob/5004f1bead31f9b3359b34b5713919fdedcd73ff/apps/layer-web/export-controls.js#L76). Ordinary SDR JPEG compression preview limitations are shared and are not counted as an Apple-specific gap.

## 5. Local-tone guide retention and scheduling lag behind

Reference changes: `a00b6118` and `6a6f275c`.

The local [local_tone.rs](../../apps/layer-apple/native/src/local_tone.rs) discards its completed guide whenever its content key changes (lines 82–83). It checks snapshot idleness before starting work, but result publication checks cancellation/key validity without rechecking current interaction idleness. It carries a CPU `LocalToneGuide`, which the Metal adapter uploads.

The [main GTK worker](https://github.com/capyatelier/capycanvas/blob/5004f1bead31f9b3359b34b5713919fdedcd73ff/apps/layer-linux/src/local_tone_view.rs) retains compatible guides with `ToneKey::can_preview`, cancels analysis during active interaction, verifies idleness before publication, and publishes `GpuToneGuide` directly. Web/Android also adopted shared GPU guides.

Apple therefore still needs compatible-guide retention, interaction cancellation/publication checks, and direct GPU-guide handoff. Otherwise SDR mapping can lose its completed guide between edits, and analysis/publication can overlap a new contact. These are code-derived risks; this audit did not measure new physical drawing latency.

Nuance: current shared `snapshot.local_tone_guide()` already wraps GPU generation plus bounded download. Merging it would inherit GPU analysis. The remaining direct-GPU gap is the download/upload round trip and host lifetime policy, not an assertion that the merged code still analyzes every pixel on the CPU.

## 6. HDR Edit Color behavior and visual layout need the newer port

Reference change: `34810f1e`, with subsequent HDR-control refinements.

[ColorEditor.swift](../../apps/layer-apple/Shared/Editor/ColorEditor.swift), line 38, always starts in `document_rgb` and omits `document_depth` and `rendition`. Its `ManagedColorButton` entry point supplies neither intensity nor viewing context. This leaves property/gradient color editing without the automatic HDR EV/Base/Adjusted behavior and latest definition/range feedback used by the [main Web form](https://github.com/capyatelier/capycanvas/blob/5004f1bead31f9b3359b34b5713919fdedcd73ff/apps/layer-web/color-controls.js#L63) and Android. The local paint-color entry point does have explicit EV editing; raw above-white values are not universally rejected.

Visual inspection confirmed these differences:

- Current Web defaults HDR editing to Linear RGB, places contiguous Base/Adjusted patches before the model, and groups the numeric rows.
- Local Apple defaults to encoded Document RGB, puts the model first, and uses separate stacked rounded text fields.
- Apple Proof uses a native segmented/pill selector; macOS also shows the extra “Proof” label and accent selection. Reference Proof uses the neutral flat Off/SDR/Print row.

The **Proof glass texture and dial are implemented** in the local port. Final native XCTest attachments show the texture; an older placeholder screenshot must not be used to report it missing. The selector/form differences are visual parity work, separate from missing form context.

## 7. HDR display status and Display Details are missing

GTK/Web/Android distinguish actual HDR output, SDR mapping, preparation, and failure, with a details entry point explaining the active display behavior. See [main proof.js](https://github.com/capyatelier/capycanvas/blob/5004f1bead31f9b3359b34b5713919fdedcd73ff/apps/layer-web/proof.js#L60) and Android `Workspace.kt` / `Hdr.kt`.

Apple's [ProofIndicator](../../apps/layer-apple/Shared/Editor/ProofForm.swift) only presents print-proof status and explicitly disables hit testing. Local Settings shows the document's HDR working space and headroom, but that does not tell the user whether the current canvas is actually showing HDR, mapped SDR, a pending guide, or a failed preview. Local-tone errors remain internal.

Add a status projection from the native presentation/guide state and an accessible details action. A float document label alone is insufficient to describe its current display result.

## 8. Docking target feedback is only partly ported

Reference changes: GTK `2d2071a4`, Web `a865b49c`, Android `1532bee5`.

[WorkspacePlacement.swift](../../apps/layer-apple/Shared/Editor/WorkspacePlacement.swift), lines 24–31, always fills the whole hint rectangle with opaque accent color. The other hosts distinguish insertion markers from tab-group body targets; body targets use roughly 25% opacity and a 2-pixel outline.

Apple already inherits shared target validation and placement. The remaining issue is native feedback: dropping onto a group body paints an opaque slab over its content instead of the translucent outlined destination. Classify the hint target when rendering it, preserving the thin insertion marker treatment.

## 9. GTK's protected Paper guidance is absent

GTK `3336fc79` adds “Paper is protected; select a paint layer to draw” guidance to the relevant name/target/thumbnail/lock affordances and adds “Protected” metadata ([main layers.rs](https://github.com/capyatelier/capycanvas/blob/5004f1bead31f9b3359b34b5713919fdedcd73ff/apps/layer-linux/src/layers.rs#L1354)).

[LayerPanel.swift](../../apps/layer-apple/Shared/Editor/LayerPanel.swift) still has generic name help, “Drawing target” accessibility value, and “Edit layer content” thumbnail labeling without that Paper distinction. This is a small GTK-to-Apple guidance/accessibility gap, not missing document protection. It is not claimed to be implemented on every other host.

## Changes already ported or inherited

These were included in the week's review and should not become duplicate Apple port tasks:

| Change group | Apple evidence / disposition |
| --- | --- |
| Customizable title bar, drag-only bank, compact/overflow behavior, icons and transparent header | Native implementation in `9996f51d`, `1952f311`, `c6e616f5`, with subsequent platform fixes. |
| Column stacks, footer targets, clipped/frozen drag previews, content-aware floating drops | Native stacks in `7d5d59b2` and follow-ups; body feedback is the exception above. |
| Workspace switcher/manager, pinning/history, shipped-default restoration | Shared layout/state and existing native views are wired; not a newly absent host feature. |
| Tile/row/handle input rules | `ReorderContact.swift` distinguishes tiles, rows, handles/customization, and actual pen/touch/mouse; mouse holds do not open menus. Extended device acceptance remains a validation concern, not blanket evidence of a missing port. |
| Compact color picker, tool/brush previews, toolbar styles, Filters/Properties/Diagnostics | Apple compact controls and later parity fixes exist (`6c36d0a9`, `9b7c4eb7`, and follow-ups). |
| Immutable raster storage, save/recovery, renderer replacement | Apple integrations and lifecycle fixes exist; shared storage/render changes are inherited on rebuild. |
| U8/U16 creation, tagged paint/palettes, color/profile conversion and exact history | Apple ports `9e3d2567`, `0290c196`, and shared transaction follow-ups. |
| Profile-preserving Open/Place/Paste, source repair/rasterization, canvas/layer drops | `d614f7b4`, `79a9638b`, `1b1f7ea5`, `4e833915`. Title-area drawing opens are separately missing. |
| Histogram/area sampling, managed P3 viewing, profiled SDR export, presets and ICC library | `e7431720`, `a2054398`, `438f5cd3`; F32 labels/new delivery variants remain above. |
| Print-proof recipes, profile preservation, viewing and comparisons | `e578f476` and shared proof policy; newer docked HDR controls are in the local port. |
| Rust JPEG/ICC/AVIF/HEIC codecs, LZ4 storage, brush/material rendering fixes | Shared code paths, including Apple's `read_import`/format enumeration. No separate missing Apple decoder was found; actual platform file-provider/output journeys still need qualification for new formats. |

Web PWA/OPFS/startup compilation, GTK Wayland/package integration, and Android-specific PQ presentation are host implementation details. Apple needs equivalent user-visible outcomes, not literal copies of those mechanisms. Earlier accepted Apple drawing/recovery work is not reopened solely because another host received an optimization.

## Runtime and visual evidence

New checks in this audit:

- Latest Web release build: **passed**. [Build log](../../artifacts/weekly-port-audit-2026-09-20/web-build.log).
- Latest committed Apple Rust bridge: **`cargo check --offline --locked -p layer-apple --lib` passed**. [Check log](../../artifacts/weekly-port-audit-2026-09-20/apple-main-check.log). This does not compile the unmerged local patch or Swift targets.
- Live Chrome/WebGPU: Apple Metal adapter, `isFallbackAdapter: false`, 1376×1032 at 2×, no captured JavaScript errors. [Runtime metadata](../../artifacts/weekly-port-audit-2026-09-20/web-hdr/chrome-capture.json).
- Created an F16 drawing alongside the existing drawing; inspected creation depth choices, two independent tab IDs, HDR Edit Color, Proof, and export choices. [Captured UI data](../../artifacts/weekly-port-audit-2026-09-20/web-hdr/audit-ui.json). The export options inventory includes hidden selects; it is not a claim that every option was simultaneously visible or that each codec export was exercised.
- Ran the repository's Proof illustration comparison in the live browser: 24 samples, 24 distinct colors, maximum channel error 2 against the shared reference. This validates the illustration, not HDR luminance or whole-window pixel parity.

Visual comparisons:

| Surface | Current Web | Existing local native evidence |
| --- | --- | --- |
| Proof | [Web capture](../../artifacts/weekly-port-audit-2026-09-20/web-hdr/proof-sdr.png) | [Final Mac XCTest capture](../../artifacts/weekly-port-audit-2026-09-20/native-mac/95B07A54-6F25-482C-BAC2-0376AE978D19.png), [iPad capture](../../artifacts/apple-color-management/proof-panel-ipad-final.png) |
| Edit Color | [Web capture](../../artifacts/weekly-port-audit-2026-09-20/web-hdr/edit-color.png) | [Final Mac XCTest capture](../../artifacts/weekly-port-audit-2026-09-20/native-mac/7E0FEAB3-028F-4669-99E2-D44013E6A35F.png), [iPad capture](../../artifacts/apple-color-management/edit-color-ipad-final.png) |
| Creation / export | [New drawing](../../artifacts/weekly-port-audit-2026-09-20/web-hdr/new-drawing-depths.png), [export](../../artifacts/weekly-port-audit-2026-09-20/web-hdr/export-sdr.png) | Compared against native controls and bridge routing described above. |

Native captures are retained evidence from the existing local port, not freshly run native sessions. Different document values, window sizes, and themes preclude a meaningful whole-window pixel score. No new GTK/Android device run, Swift/Xcode build, physical Pencil/pen matrix, gain-map file round trip, or F32 native journey was performed here. Temporary browser/server processes exited after capture. The isolated source checkout and audit artifacts remain available for reproduction.

Recommended port order: reconcile the existing HDR work with current shared APIs; complete Float32/EXR and gain-map delivery; adopt retained GPU guides and display status; port drawing-session ownership and tabs as a separate substantial feature; finish the color-form and smaller visual/accessibility gaps.
