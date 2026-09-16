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

The native SDR foundation checkpoint now passes both Release builds and local
Swift/Metal checks. Existing effect/gradient controls preserve tagged colors;
startup and prepared-document constructors use native integer backing with
Float32 processing. P3/U8 and ProPhoto/U16 exact save/recovery/history and GPU
replacement pass on Mac Metal for both Apple policies. See the
[scoped acceptance record](apple-handoff.md#native-sdr-foundation).
The next checkpoint adds complete New Drawing options/presets/defaults, tagged
paint entry, workspace palettes and document-space wheel previews. Shared,
Swift/Metal owner and full Mac UI checks pass; see the
[workflow record](apple-handoff.md#sdr-creation-and-paint-workflows) for current
device coverage. Retained photo Open/Place/Paste now share the native worker and
profile/depth policies, replacing the lossy sRGB8 import path; see the
[photo workflow record](apple-handoff.md#retained-photo-open-place-and-paste).
**Continue with profile/depth changes and the remaining host workflows in step 3**, then
display integration and physical acceptance. Profiled export and device
performance are not closed.

The physical M4 startup check exposed and fixed a vendored wgpu Metal Float32
capability mismatch; startup and short synthetic painting now pass. The simulator
still lacks required Float32 filtering. Physical XCTest's extra runner is blocked
by the device's free-profile app limit; preserve the installed artist apps.
Use fast shared/native-owner checks during implementation and group remaining
device interaction checks, instead of repeating full-app simulator failures.

## Implement in this order

1. Existing effect/gradient controls in `Shared/Editor/PropertyControls.swift`
   now use the shared tagged form. Preserve this contract: shared `RgbColor` is
   `{space, rgba}` with **encoded** RGB in the named space. Preserve the tag in
   edits; convert native swatches/previews to their declared display space.
2. `native/src/metal.rs` and `native/src/project.rs` now use the native SDR path
   for startup, prepared documents and GPU replacement. Carry that contract
   through the new export/snapshot workflows. Use Android's
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
