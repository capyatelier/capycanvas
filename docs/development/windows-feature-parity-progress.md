# Windows feature-gap review against Web and Android

Fresh review: 2026-09-18–19. Started from a clean Windows worktree, pulled
main from `20cecad7` to `59aff3df`, then integrated the disjoint Apple update
`c1472101`, the concurrent Float32/OpenEXR change `537464b7`, and shared
snapshot/HDR fixes through `68903c46`, using fast-forward/autostash integration
without overwriting local work. Earlier acceptance is historical;
the checks below are scoped to this review.

Read `AGENTS.md`, the [drag convention](../ui/drag-and-reorder.md),
[drag inventory](../ui/drag-inventory.md), [Windows acceptance](windows-acceptance.md),
[title-bar acceptance](title-bar-windows-acceptance.md), and the
[color](color-management-m2-windows-handoff.md) and
[shared-workflow](shared-workflow-centralization-handoff.md) handoffs. Compared
current shared command availability and host-request routing, Windows native
forms/workers/presentation, Web JavaScript/Rust, and Android Kotlin/Rust.
Web/Android source inspection is not a fresh browser/tablet acceptance run.

## Findings and fixes

| Area | Independent current-code finding | Result |
| --- | --- | --- |
| Print proofing | Web and Android consume `ProofPreparation`/`ProofView`; Windows excluded the three proof commands, did not handle `SoftProofSetup`, and never called `ViewportPresenter::set_proof`. The previous report's “deferred” label was stale relative to those ports. | Implemented native Proof Setup, Proof Colors and Gamut Warning, shared option projection, profile import/management, asynchronous preparation and status. Canvas and Navigator share the proof presenter. |
| Proof history and portability | Recipes belong to document history; temporary viewing switches do not. Replacing an embedded ICC requires preserving its original bytes locally. GPU replacement and recipe history can invalidate view resources. | Windows uses the shared preparation identity, validation, preservation decision, LUT cache and commit. Native file workers atomically preserve the old ICC before adoption. Stale/cancelled jobs cannot commit. View preparation is rebuilt after history/document changes and GPU resources are recreated by the shared presenter. |
| Previously blocked color-input check | The earlier foreground-ownership rejection did not reproduce in the fresh run. This exposed two later fixture failures: smoke-test buttons covered the bottom color swatches, and the drawer test required retired partial Zen. | Removed the unrelated smoke overlay from this fixture and used the current ordinary toolbar Color drawer. Foreground ownership and target-process guards remain intact. The complete mouse/pen/touch/keyboard journey passed. |
| SDR creation, document/source color, import/placement and export | Windows already uses shared creation drafts, `ColorWorkflow`, `SourceWorkflow`, `ImageImportBatch`, retained placement and export recipes/plans. New upstream HDR adoption is explicitly rejected on Windows, Web and Android. | Retained the existing shared ownership; requalified the Windows paths below rather than copying Web/Android rules. |
| Filters and thumbnail scheduling | The native document fixture exposed thumbnail starvation: shared `NativeHost` used the general dirty flag, which stays set while unrelated shaders warm up. Request traces showed settled document pixels but no accepted thumbnails. Windows filter previews already consume the shared preview cache protocol. | Changed shared thumbnail eligibility to ready canvas/export pipelines with no active input, pending edits or shared background editing. Added a D3D12 regression for both rejected unsettled imports and accepted thumbnails during unrelated warmup. Kept the native fixture’s original 15-second assertion. |
| Runtime filter fixture | An initial preview timeout did not repeat in isolated tracing; valid atlases reached the native cache. The subsequent replacement check could compare a retained bitmap before the new one was composed. | The fixture now waits, within its existing 30-second bound, for actual cyan pixels from the changed WGSL as well as a changed capture hash. The complete journey passed after removing diagnostic tracing. No production filter scheduling change was made. |
| Native recovery fixture | The expanded image-import journey left Move and the source layer active before its pen replay. Its early revision checks could therefore exercise image movement; the later ink check correctly failed. | Explicitly select the original ink layer and Pen and wait for brush readiness before replay. Keep the pixel, history, recovery and close-time assertions. |
| Workspace, header, layers and input | Current Windows registrations classify visible tile/row/grip/tab targets and pointer devices, with shared drop validation/publication/history. This change adds no reorderable surface. | The app-wide drag convention remains required. This review does not extend previous synthetic drag results to physical devices or claim to rerun the full drag matrix. |

## Ownership and implementation

- `crates/layer-ui/src/proof_workflow.rs` owns preparation identity, stale-result
  rejection, embedded-profile preservation policy, recipe commit and view cache.
  `PrintProofSettings` owns intent/BPC/simulation semantics; its new reusable
  `from_recipe` projects saved recipes back into settings.
- `layer-color` builds the proof LUT; `layer-render-wgpu` applies it only during
  presentation, including the Navigator. No Windows copy of proof conversion,
  gamut calculation or shader math was introduced.
- `apps/layer-windows/native/src/document_proof.rs` adapts the existing document
  worker to those shared operations. Validation occurs on the editor owner
  before profile preservation and again before commit. Atomic file writes and
  storage locks reuse `color_storage.rs`.
- `native/src/proof.rs` schedules background view preparation, cancels obsolete
  work and joins workers before service teardown. The owner retains CPU LUTs;
  the presenter owns device-specific resources.
- `ProofForm.h` and `DocumentView.cpp` provide native controls, pickers and
  profile-library navigation, preserving the setup draft across those dialogs.
  Options/availability come from shared Rust. First use can be cancelled without
  editing the document. Errors leave the setup available for correction.
- The previously implemented native precision, tagged colors/palettes, color
  preferences, ICC storage, histogram/sampling, source operations, multi-image
  placement, export presets, restart recovery, panel sizing, toolbar drawer
  switching and prediction adapters remain present. Their implementation
  baseline is recorded in the earlier history of this document.

## Fresh validation

The review build is an unpackaged Release executable in
`artifacts/windows/parity-final` on `68903c46` plus this change. Color, successful
pen recovery, restart recovery and runtime-filter journeys passed on the preceding
`537464b7` integrated build with these production fixes. Proof, failed-GPU saving
and explicit D3D12 checks were rerun after the final shared snapshot integration.
Logs and disposable fixture profiles are
under ignored `artifacts/windows/parity-*`, `proof-ui`, `compact-color`,
`document-ui` and `artwork-recovery`.

Host: Intel Iris Xe Graphics, driver `32.0.101.6737`; Windows reports
2256 × 1504 at 59 Hz. Hardware D3D12 checks reject CPU/fallback adapters.
This identifies functional evidence, not high-refresh acceptance.

| Check | Fresh result and limits |
| --- | --- |
| Release Windows build | Passed; native C++/WinUI and Rust linked. Environmental CS1668 warnings reference two missing Visual Studio library search directories. Log: `parity-final-build.log`. |
| Rust library suites | Passed: 28 shared-host, 481 shared-UI and 117 Windows tests (626 total); 13 explicitly ignored in this ordinary run. Includes proof cancellation before/after preparation, stale requests, invalid ICC retry, exact recipe history, worker supersession and shutdown/restart. Log: `parity-final-rust.log`. |
| Strict Clippy | Passed for Windows and shared host, all targets, no dependency linting, warnings denied. Log: `parity-final-clippy.log`. |
| Native C++ input/queue tests | Passed bounded admission, cancellation/refusal ownership, ordering, retained models, query scheduling and completion. Log: `parity-native-input.log`. |
| Native proof journey | Passed first-use cancel, intent/BPC, profile-picker cancel and library draft return, setup/toggles, recipe and artwork Undo/Redo, identical PNG bytes with viewing on/off, D3D12 device replacement, Unicode save/reopen with recipe retained and view switches reset. Log: `parity-final-proof-ui.log`. |
| Guarded native color input | Passed mouse/pen/touch field/ring input, cancellation, keyboard/retained controls, slots/swap, context menus, mouse-hold exclusion and ordinary Color drawer; no document edit. Guard code was not changed. Log: `parity-color.log`. |
| Native document/recovery journey | Passed creation expressions/errors, image picker cancellation/drafts, invalid input recovery, placement/thumbnails/history, save/export cancellation and Unicode, corrupt-open preservation, replace/save-before-open, Preferences draft/close cancellation, two GPU replacements with queued/active pen and exact pixels, durable close and untitled discard. Log: `parity-documents.log`. |
| Failed-GPU native journey | Passed Save/Save As after recovery exhaustion, committed-raster preservation, queued contact cancellation, Cancel/Discard, durable reopen and identical exported pixels. Native close passed its original five-second bound after waiting for dialog completion. The ordinary document checks above also passed in this final build. Log: `parity-failed-gpu.log`. |
| Crash/restart recovery | Passed checkpoint restore, origin retirement, later edits surviving clean close and explicit discard. Log: `parity-artwork-recovery.log`. |
| Runtime filters | Passed startup package, native controls, live WGSL/metadata, compatible parameter retention, invalid/missing-module preservation and retry, changed GPU preview with actual new-shader pixels. Log: `parity-runtime-filters.log`. |
| Explicit native D3D12 integration | Passed import/export/source color, proof recipe persistence and ICC preservation, proof/export separation, exact Undo/Redo and thumbnail eligibility during unrelated warmup (111.75 s). Log: `parity-d3d12.log`. |
| D3D12 proof CPU/GPU oracle | Passed on the explicitly selected Intel hardware D3D12 adapter: 1,280 CPU/GPU comparisons across four working spaces, U8/U16, two display primaries, 16 pixel vectors and five proof/warning states; artwork and export remain unchanged (84.28 s). Log: `parity-d3d12-proof.log`. |
| Presentation analysis | Synthetic fixtures passed; this checks analysis math, not measured presentation cadence. Log: `parity-presentation.log`. |

One proof recovery run during concurrent compilation exhausted the existing
hardware-adapter recovery budget and correctly offered saving. The isolated rerun
passed. The cause was not established; this does not qualify recovery under CPU
or driver stress. No software fallback or longer recovery deadline was introduced.

The failed-GPU fixture also exposed stale Zen accessibility selectors and a
native-readiness race: shared export completion precedes progress-dialog teardown.
Windows correctly ignores window close while a document dialog remains open,
so those attempts never reached shared close handling. The fixture now uses the
stable Zen ID, waits for the canvas to become enabled, and posts close to the
known application HWND after checking its process ownership. Its explicit
Preferences-draft close case remains allowed. The five-second shutdown bound
is unchanged.

The broader shared-host suite also exposed an outdated Windows drag-preview
assertion; it now checks the same pointer-following geometry already implemented
for the other native hosts. A workspace create-failure fixture now injects its fault specifically into the
create transaction, allowing its prerequisite outgoing flush to complete. No production workspace policy was changed.

An integrated rebuild initially exhausted disk space. Removing only the verified
workspace Rust incremental cache allowed the Release rebuild to complete; source
files and UI evidence were retained.

## Reproduction

Run native GUI fixtures serially in disposable profiles:

~~~powershell
./apps/layer-windows/scripts/build.ps1 -Configuration Release -SkipRestore -OutputDirectory artifacts/windows/parity-final
cargo test --locked -p layer-windows -p layer-ui -p layer-host --lib
cargo clippy --locked -p layer-windows -p layer-host --all-targets --no-deps -- -D warnings
./apps/layer-windows/scripts/test-input.ps1
./apps/layer-windows/scripts/test-presentation-analysis.ps1
./apps/layer-windows/scripts/exercise-proof.ps1 -Executable artifacts/windows/parity-final/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-compact-color.ps1 -Executable artifacts/windows/parity-final/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-documents.ps1 -Executable artifacts/windows/parity-final/CapyCanvas.exe -RecoverGpu
./apps/layer-windows/scripts/exercise-documents.ps1 -Executable artifacts/windows/parity-final/CapyCanvas.exe -FailGpu
./apps/layer-windows/scripts/exercise-artwork-recovery.ps1 -Executable artifacts/windows/parity-final/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-runtime-filters.ps1 -Executable artifacts/windows/parity-final/CapyCanvas.exe
~~~

`test-input.ps1` needs the Visual Studio developer environment. For the explicit
D3D12 integration test, set `CAPY_SETTINGS_DIRECTORY` to a new absolute directory
under ignored artifacts, then run:

~~~powershell
cargo test --locked -p layer-windows --lib d3d12_native_color_import_export_source_and_history_round_trip -- --ignored --test-threads=1
cargo test --locked -p layer-render-wgpu --lib d3d12_proof_view_matches_cpu_without_changing_artwork_or_export -- --ignored --test-threads=1 --nocapture
~~~

The second test reuses the shared proof CPU/GPU oracle with an explicitly
selected D3D12 hardware device rather than inferring the backend from the OS.

## Remaining gaps and acceptance

- Physical pen/touch prediction, pressure/tilt/eraser, capture/cancellation and
  the full drag matrix; synthetic input does not qualify a digitizer.
- Mixed-display/DPI, suspend/resume and actual sustained 120 Hz painting and
  input-to-present latency. The available display cannot close those gates.
- The 61 MP photo set, peak process/driver memory, navigation latency, export
  throughput and constrained-device behavior. Small functional fixtures do not
  establish those limits.
- Physical printer/paper matching and a representative licensed CMYK printer
  profile matrix. Builtin RGB and generated embedded ICC fixtures qualify the
  integration paths, not physical proof accuracy.
- Complete accessibility/native-control review, clipboard/codec/profile-batch
  coverage, and fresh whole-editor cross-platform pixel comparisons.
- Clean-machine portable deployment and signed MSIX install/update/uninstall.
  This review builds an unpackaged executable; it does not refresh or qualify
  distribution packages.

HDR authoring/display integration, unsupported codecs and the dirty-rendering
redesign remain separate work. HDR is not currently an extra supported feature
of Web or Android that Windows silently omits. Print proofing is now implemented
and is no longer in that deferred list. See [Windows acceptance](windows-acceptance.md)
for the wider release gates.
