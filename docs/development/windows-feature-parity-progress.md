# Windows feature parity port

Baseline: main pulled to `eafd1d51` on 2026-09-17, then integrated through
`1f96d5a6` before publishing. Release build and the explicit D3D12 workflow check
were repeated after the incoming shared tile-rendering optimization. The port consumes
upstream C1–C6 from the [shared workflow handoff](shared-workflow-centralization-handoff.md)
and the shared retained-image placement transaction. This document records the
implemented scope and its validation; device acceptance is tracked separately.

## Implemented features and ownership

| Feature | Windows integration | Shared owner of business/rendering behavior |
| --- | --- | --- |
| SDR document precision | Native U8/U16 and existing Float32 rendering for startup, Open and recovery. | `layer-render-wgpu` native renderer, document color state and prepared projects. |
| Effect and gradient colors | Tagged effect fields, shared swatches, sampled gradient previews and working-space color-wheel rasters. | Shared color forms, color conversion and gradient sampling; WinUI displays their results. |
| New document | Working space, integer depth, background, defaults and named preset save/delete. | `layer-ui` New-document drafts, validation and preference actions. |
| Photo Open | Retained photo decoding, missing-profile interpretation and separate editable master on Save. | Shared import preparation and photo adoption policy; native picker/worker/file transport. |
| Import, Paste and external Drop | Multi-file picker, clipboard files/bitmap, canvas/layer drops, insertion hints and Apply/Cancel placement. | `ImageImportBatch`, shared target validation and placement transaction with one-step history. |
| Document color | Properties, Assign, Convert, depth changes, comparison and flattened converted copy. | `ColorWorkflow`, candidate identity, color/brush/view remapping and exact history in shared Rust. |
| Source operations | Profile repair, rasterization and comparison. | `SourceWorkflow`, baked-edit handling, preview readiness and shared commit/history. |
| Export | PNG/JPEG/TIFF, profile, resize, resolution, intent/BPC, matte, dither, quality, comparison and presets. | Shared export draft actions, recipes, output planning, resampler, CMM and codecs. |
| Precise color and palettes | Color-readout context menu opens numeric color and tagged palette editing. Primary click retains readout switching. | Shared color editor and palette commands own values, validation and mutations. |
| Color preferences and ICC library | Native preferences and profile import/list/remove controls with app-owned storage. | Shared preferences and profile-library policy own identities, validation, limits and deduplication. Windows supplies atomic files and locks. |
| Histogram and area eyedropper | Native histogram display and shared point/3×3/5×5 sampling choices. | Shared histogram and sampling computation; native worker scheduling and chart controls. |
| Artwork restart recovery | Periodic committed checkpoints, Restore/Later/Discard, recovery errors/retry and close coordination. | `RecoveryState` owns checkpoint/retirement decisions and durable replacement ordering. Native worker supplies atomic storage and per-window leases. |
| Floating panel sizing | Native content/scroll measurements for panels and retained drawers. | Shared release sizing and layout publication; existing input arbitration stays native. |
| Toolbar drawer switching | One-click switching through the shared toolbar behavior. | Existing shared drawer policy and workspace history. |
| Predicted pointer input | Windows App SDK `PointerPredictor`, capability enabled only when available, predicted points flagged separately from actual samples. | OS supplies prediction; shared engine owns provisional rendering and stroke semantics. |

Host adapters are in `apps/layer-windows/native/src/document_workflows.rs`,
`document_color.rs`, `document_source.rs`, `document_export.rs`,
`color_storage.rs` and `recovery.rs`. WinUI owns controls, pickers, clipboard,
OS input and presentation. These adapters schedule preparation, forward shared
workflow transitions and retire old GPU resources off the editor owner.
They do not add a parallel implementation of color conversion, placement,
export rendering, profile policy or recovery policy.

The port also adds a reusable complete export validator to `layer-ui`. The shared
proof-view shader now advances its tetrahedral interpolation coordinate without
a dynamically indexed vector assignment: the previous expression failed in the
D3D12 FXC compiler. The interpolation math remains shared across backends.

Native modal ordering is explicit: an import progress sheet closes before the
shared placement begins. Focus cancellation still applies to real application
switches and minimization, while invisible tablet/IME helper windows do not
cancel placement.

## Validation

- Release Rust library and WinUI executable build passed; strict Windows Clippy
  (`--all-targets --no-deps -- -D warnings`) passed.
- `cargo test -p layer-windows -p layer-ui --lib --locked`: 453 shared UI tests
  and 113 Windows tests passed; 12 Windows hardware/integration tests remain
  ignored by the ordinary suite.
- Native C++ input, query-queue and presentation checks passed.
- Explicit D3D12 integration test
  `d3d12_native_color_import_export_source_and_history_round_trip` passed on the
  hardware adapter. It checks native renderer/recovery preparation, Assign,
  Convert, U16 depth and exact color-state Undo/Redo; PNG/JPEG/TIFF resize and
  resolution; retained multi-image placement; source repair/rasterization;
  histogram; and profile-library import/list/remove.
- Native document UI regression passed: New validation, import picker cancellation
  and decoder error recovery, placement Apply/Undo/Redo, thumbnail and embedded
  image reopen, Unicode paths, export cancellation/dimensions/checkpoint, Save As,
  corrupt Open preservation, save-before-open, Preferences draft preservation and
  Save/Discard/Cancel close paths. This run did not inject GPU removal.
- Artwork restart recovery UI regression passed: committed stroke checkpoint,
  forced process termination, restore, durable replacement before origin deletion,
  another restart, Later surviving clean close and explicit Discard. Recovery
  adoption waits for shared document-idle eligibility during startup filter
  validation. Storage tests also cover concurrent live leases and failed atomic
  replacement preserving the previous copy.
- Compact-color synthetic input regression stopped at its foreground-ownership
  guard (`Review does not own foreground input`). It is incomplete; the guard
  was not bypassed. This run does not qualify all mouse/pen/touch picker paths.
- Runtime filter UI regression passed: startup package, rendered previews, native
  picker/properties, live WGSL/metadata replacement, compatible value preservation,
  invalid WGSL and missing-module rejection, retry and changed GPU preview.
  Windows now signals completion of startup catalog loading to the native
  renderer; this also releases previews after fallback or GPU replacement.

Reproduction uses `apps/layer-windows/scripts/build.ps1`, `test-input.ps1`,
`exercise-documents.ps1`, `exercise-artwork-recovery.ps1` and
`exercise-runtime-filters.ps1`. The D3D12 test is
explicitly ignored by default and can be selected with `cargo test -p
layer-windows --lib d3d12_native_color_import_export_source_and_history_round_trip
-- --ignored --test-threads=1`; use an isolated `CAPY_SETTINGS_DIRECTORY`.
Local logs and fixture files live under ignored `artifacts/windows/port-*`.

## Remaining acceptance

The feature implementations above are present. This does not extend historical
acceptance to untested hardware or workloads. Still required:

- Physical pen/touch prediction quality, pressure/tilt, capture and cancellation;
  multi-DPI/display transitions, suspend/resume and 120 Hz painting.
- Large retained-photo and color workflows, including the 61 MP acceptance set,
  peak memory, latency and export throughput. The D3D12 integration fixture is
  small and does not establish these limits.
- Exhaustive native-control interaction and accessibility review of the new
  forms, picker/clipboard format matrix and mixed-profile image batches.
- Cross-platform pixel comparisons, package installation/update and full release
  qualification from [Windows acceptance](windows-acceptance.md) and the
  [color handoff](color-management-m2-windows-handoff.md).

Existing deferred scope stays deferred: HDR, print proofing, unsupported codecs
and the dirty-rendering redesign. The app-wide
[drag convention](../ui/drag-and-reorder.md) continues to apply to panels,
retained drawers and toolbars; this port does not redefine it.
