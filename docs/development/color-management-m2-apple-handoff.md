# Phase 2 handoff: macOS and iPadOS

2026-09-16. Implement and qualify complete SDR color/photo editing on **both**
Apple hosts. GTK is qualified within its recorded envelope; Web/Android have
passing device workflows, with the latest user retest pending. Apple is not yet
compatible with all shared phase 2 contracts. Start from the integration of
`a8b3cd76` and Apple upstream `9b7c4eb7`; preserve the latter's prediction,
presentation-progress, lifecycle and diagnostics fixes.

Read the [current scope and remaining work](../history/color-management-m2-port-handoff.md),
[SDR journeys](../ui/color-management.md), and
[tablet validation](../history/color-management-web-android-m2-validation.md).
Historical plans are proposals/evidence; verify against current code and Metal.

## Implement in this order

1. Fix existing controls first: `Shared/Editor/PropertyControls.swift` still reads
   effect colors/gradient stops as arrays. Shared `RgbColor` is
   `{space, rgba}` with **encoded** RGB in the named space. Preserve the tag in
   edits; convert native swatches/previews to their declared display space.
2. Replace the attachment-based constructors in `native/src/metal.rs` and
   `native/src/project.rs` with the native SDR path, including initial startup,
   prepared documents, export/snapshots and GPU replacement. Use Android's
   `native/src/android.rs` and `documents.rs` as reference. Backing is straight
   alpha U8/U16; processing is Float32. Query actual Metal capabilities; preserve
   the shared portable publication path where in-place editing is unavailable.
   Never narrow edits/export through FP16 display caches.
3. Port New/Open/Place/Paste, tagged numeric colors/palettes, profile/depth
   changes and exact history, source repair/rasterization, histogram/sampling,
   six retained photo corrections/masks, ICC library and profiled export presets.
   Reuse `layer-color`, `layer-ui`, `snapshot`, and Android's `color_edit.rs`,
   `source_edit.rs`, `inspection.rs`, `color_preferences.rs`; keep heavy work off
   the UI/render owner and publish prepared document + renderer atomically.
4. Integrate managed canvas **and** native controls, Mac monitor changes, iPad
   display policy, native pickers/providers and background/recovery lifecycle.
   Keep current AppKit/UIKit input ownership and document-adoption prediction.

## Prove completion

Build Release macOS and iPadOS with [the Apple guide](apple.md). Capture a fresh
baseline first; finish benchmarking/optimization after functional integration.
Run shared tests plus actual Swift/Metal and device workflows: P3 U8 painting,
ProPhoto U16 exact save/export, revisable corrections/masks, cancellation,
undo/redo, and device replacement. Repeat **61 MP JPEG → G-Pen → save/reopen →
GPU recovery**; original photo pixels must survive every touched tile.

Retain ordered staging uploads and bounded composition submissions from
`37deed7b`; large blank recovery frames also need command-memory bounds. Qualify
unified-memory pressure and camera-only navigation separately from regeneration.
Target smooth iPad 120 Hz; current Mac evidence is 90 Hz, with 120 Hz hardware
qualification still deferred. Document actual refresh, missed frames and latency;
do not label CPU submission as presentation. Preserve users' files/recovery,
identify the installed build, commit significant checkpoints, and publish a
short acceptance record with remaining hardware gaps. Print proofing/HDR is later.
