# Phase 2 handoff: Windows

2026-09-17 implementation update: see the [Windows feature parity port](windows-feature-parity-progress.md) for the current shared-core integration and validation. The historical acceptance below is not automatically extended to the new workflows.

2026-09-16. Bring WinUI/D3D12 to complete SDR color/photo editing. Shared phase 2
and GTK/Web/Android implementations are available through `a8b3cd76`; Windows
still has incompatible existing color controls and uses the old attachment
renderer. This is an integration starting point, not Windows release acceptance.

Read the [current scope and remaining work](../history/color-management-m2-port-handoff.md),
[SDR journeys](../ui/color-management.md), and
[tablet validation](../history/color-management-web-android-m2-validation.md).
Use [Windows acceptance](windows-acceptance.md) for unrelated native gaps;
historical success on older shared code does not qualify this color port.

The [shared workflow centralization handoff](shared-workflow-centralization-handoff.md)
is the prerequisite Web/Android cleanup for portable workflow rules. Consume its
shared services as they land rather than copying the existing app-local color,
source, export, profile-library or recovery state machines into Windows. The
references below remain useful integration examples, not instructions to duplicate
their business or rendering decisions.

## Implement in this order

1. Fix `apps/layer-windows/EffectView.cpp` and `GradientView.cpp`: they still read
   color arrays. Shared `RgbColor` is `{space, rgba}`; RGB is encoded in its named
   space. Preserve tags in component/gradient edits and convert WinUI previews
   correctly. Merely extracting `rgba` loses wide-gamut meaning.
2. Switch `native/src/host.rs` and `native/src/documents.rs`, including startup,
   prepared documents, snapshots/export and recovery, to the native SDR renderer.
   Reference Android's `native/src/android.rs` and `documents.rs`. Canonical paint
   is straight-alpha U8/U16 with Float32 processing. Request/query D3D12's actual
   filtering/blending/storage capabilities and use the shared portable publication
   mode when in-place editing is unavailable. Unsupported modes must fail before
   replacing the current drawing; no silent FP16/sRGB reduction or legacy adapter.
3. Port the complete SDR journeys: profile/depth creation/import, retained
   Place/Paste, tagged colors/palettes, Assign/Convert/depth/history, source
   repair/rasterization, histogram/sampling, six editable corrections and masks,
   ICC library, PNG/JPEG/TIFF export and presets. Reuse shared Rust and Android's
   `color_edit.rs`, `source_edit.rs`, `inspection.rs`, `color_preferences.rs`.
   Prepare expensive work asynchronously; publish document/renderer/history
   together. Opening a photo must save a separate editable master.
4. Qualify managed canvas and WinUI colors together, monitor/DPI transitions,
   native file pickers, failure/cancellation and removed-device recovery.

## Prove completion

Capture a fresh baseline; finish optimization after functional integration.
Build Release with [the Windows guide](windows.md), run the shared/bridge tests
and actual WinUI/D3D12 workflows. Check exact P3 U8/ProPhoto U16 native history,
identity PNG/TIFF samples/ICC, profiled JPEG copies, retained effects/masks and
GPU replacement. Include **61 MP JPEG → G-Pen → undo/redo → save/reopen → recovery**
and fullscreen high-DPI pan/zoom/rotation.

Keep ordered uploads and bounded command batches (`37deed7b`); no source upload
does not mean a large recovery frame has bounded driver memory. Admit useful
display mips from measured memory headroom, retain full-resolution edit/export
semantics, and document constrained fallbacks. Smooth 120 Hz navigation requires
a real suitable display; the earlier 60 Hz Windows results cannot establish it.
Report cold/warm CPU/GPU, missed frames, latency and memory separately.

Preserve normal installation data and explicitly identify the tested package.
Commit significant milestones and integrate without force-pushing other ports'
work. Record actual hardware/build/workflow evidence and remaining gaps; print
proofing/HDR and resolution-aware dirty rendering are later work.
