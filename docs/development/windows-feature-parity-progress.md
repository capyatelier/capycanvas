# Windows feature-gap review against Web and Android

HDR implementation update: 2026-09-19. The earlier review below is historical;
the Windows HDR implementation and fresh evidence are recorded in the new HDR
section. The earlier blanket HDR rejection is no longer the current Windows contract.

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

## Windows HDR implementation and verification (2026-09-19)

Started on a clean Windows main at `20932ce4`, pulled `caf46ebf`, then safely
integrated concurrent `cb577569`, `34810f1e` and `6a997a3d` changes without
dropping either side of the shared color/proof/storage work. Independently read the current Windows, shared Rust and GTK workflows. The previous report was
stale: shared Float32/EXR and Web/Android HDR work had landed, while Windows still
rejected HDR adoption/export, exposed only U8/U16 controls and created an SDR
presenter without tone analysis. Read AGENTS.md, the drag convention, Float32 HDR
scope/validation, M4 design and HDR export proposal. This work adds no draggable
surface and preserves the application drag rules.

### Implemented behavior and ownership

- Native New Drawing and precision conversion offer Float16 and Float32. Shared
  creation, source import/placement, color candidate transactions and renderer
  capability validation own adoption. Unsupported GPU capabilities fail without
  silently narrowing document samples. Existing source originals keep their own
  precision. Native .capy storage, history and recovery retain committed samples.
- Shared numeric color entry receives document precision and HDR intensity;
  parsing, range checks, signed/extended RGB and color conversion remain in Rust.
  Native picker, palette and gradient previews use the shared saved-rendition
  mapper; effect and gradient numeric fields receive HDR document precision. Histograms display
  the shared stop-axis bounds, including the Float32 range.
- View > Proof SDR opens native SDR Appearance controls projected from the shared
  proof form. Save makes one shared history edit; Cancel leaves the master and
  history unchanged. Temporary Preview SDR and print/gamut proof are viewing
  states. Print proof receives the mapped SDR rendition and shares the canvas and
  Navigator presenter; exports never consume temporary viewing switches.
- The existing immutable snapshot/file worker now exports Float32 OpenEXR,
  BT.2020 PQ PNG, explicitly range-clipped PQ PNG and saved-rendition SDR
  PNG/JPEG/TIFF. Shared export drafts preserve EXR document primaries and normalize
  deliberate SDR delivery. Preview, codec/range validation, output sizing,
  cancellation and atomic sibling-file publication use existing Rust workflows.
  Native open/placement uses the shared tagged PQ PNG and bounded OpenEXR decoder;
  the Open picker explicitly offers EXR.
- The D3D12 SwapChainPanel negotiates RGBA16Float with extended-linear sRGB.
  Native DXGI output discovery matches the HWND's current monitor across adapters,
  refreshing active HDR state and output limits at a 500 ms service deadline.
  It avoids GetContainingOutput on the composition swap chain. Presenter encoding
  changes between Windows scRGB and linear SDR; unadvertised float surfaces use
  ordinary SDR. Missing/invalid output reports and SDR monitors use the saved SDR
  rendition. HDR viewing retains the shared 203 cd/m² artwork reference and 80
  cd/m² scRGB encoding. Neither display changes nor fallback changes the document.
  This follows [Microsoft's Advanced Color contract](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/high-dynamic-range).
- The Windows owner schedules shared snapshot/local-tone analysis on one bounded,
  cancellable worker. Shared ToneKey plus GPU generation reject stale results.
  Closing, document/color/history changes and removed devices cancel obsolete work;
  workers join before device/service destruction. CPU tone guides and proof LUTs
  are uploaded to replacement presenters. Presenter identity also tracks working
  RGB changes, correcting the former stale-color presentation path.

### Fresh HDR validation

Functional evidence is recorded under ignored `artifacts/windows/hdr-*`. Small
fixtures establish behavior and pixel/storage contracts, not latency or memory
budgets. Hardware tests explicitly select D3D12 and reject CPU/fallback adapters.
Synthetic display reports are available only to an isolated HDR smoke-test process.

| Check | Result and evidence |
| --- | --- |
| Native Release build | Passed Rust Release and MSVC/WinUI build; executable at artifacts/windows/hdr-final/CapyCanvas.exe. Log: hdr-build.log. |
| Rust unit suites | 886 passed: color 88, core 97, engine 63, host 28, UI 492, Windows 118. The default runs leave 22 opt-in tests ignored; the three HDR hardware tests below were run explicitly. Log: hdr-rust-final.log. |
| Strict native lint | Passed layer-windows and layer-host, all targets, --no-deps, -D warnings. Log: hdr-clippy.log. Whole-dependency strict lint remains affected by existing layer-core lint findings; no workspace-wide clean claim. |
| D3D12 HDR pixel oracle | Passed 1,344 CPU/GPU combinations: Float16/Float32, sRGB/ProPhoto, seven vectors, three encodings (linear SDR, Windows scRGB, PQ), four recipes, two headrooms, proof on/off. Offscreen Float32 readback verifies conversion and unchanged master samples. Log: hdr-oracle.log. |
| D3D12 native document workflows | Both tests passed on the integrated code, 155.31 s together. Float16/Float32 source edits, saved SDR recipe and undo/redo, exact layered save/reopen, EXR/PQ/SDR delivery, proof/export separation, cancellation, pending-analysis device removal/replacement, precision conversion, signed/extreme Float32, rejected lossy demotion, and protected strict-PQ destinations. Log: hdr-d3d12-final.log. |
| Native GUI journeys | Float16 and Float32 passed creation, invalid/valid HDR numeric color, above-white painting and exact undo/redo, SDR appearance cancel/save/history, PNG/PQ/EXR export, synthetic display switching, proof/export separation, device recovery, Unicode save/reopen, and opening the exported HDR photo (PQ for Float16; EXR for Float32). Logs: hdr-ui-f16.log and hdr-ui-f32.log. |
| SDR native regression | Existing exercise-proof.ps1 passed first-use cancellation, shared options, profile-picker cancellation, library drafts, proof/history/pixel separation, export invariance, D3D12 recovery and save/reopen. Log: hdr-sdr-proof.log. |

The hardware adapter was Intel Iris Xe, D3D12 driver 32.0.101.6737. The real DXGI
output reported SDR with 400 cd/m² maximum luminance; RGBA16Float composition was
available. Injected SDR/HDR reports test host transitions and the five-times
headroom path, not a physical HDR panel. One hardware rerun timed out at the
existing 60-second canvas-settle deadline while a Release build was running;
the isolated serial rerun passed without changing the deadline. The failing log
is retained as hdr-d3d12-build-contention.log; its cause is not proven and no
performance qualification is inferred. Removed only unused target/debug/incremental
cache when disk space became tight; source and validation evidence were retained.

Native validation found and fixed delayed TextChanged notifications re-enabling
an invalid HDR color draft. A fixture race reading a reopened SDR dialog before
its snapshot arrived was fixed with an explicit readiness wait.

Reproduction, with GUI journeys run serially in disposable profiles:

~~~powershell
./apps/layer-windows/scripts/build.ps1 -Configuration Release -SkipRestore -OutputDirectory artifacts/windows/hdr-final
cargo test --locked -p layer-core -p layer-color -p layer-engine -p layer-ui -p layer-host -p layer-windows --lib
cargo clippy --locked -p layer-windows -p layer-host --all-targets --no-deps -- -D warnings
./apps/layer-windows/scripts/exercise-hdr.ps1 -Executable artifacts/windows/hdr-final/CapyCanvas.exe -Depth F16
./apps/layer-windows/scripts/exercise-hdr.ps1 -Executable artifacts/windows/hdr-final/CapyCanvas.exe -Depth F32
./apps/layer-windows/scripts/exercise-proof.ps1 -Executable artifacts/windows/hdr-final/CapyCanvas.exe
~~~

Set CAPY_SETTINGS_DIRECTORY to a new absolute directory under ignored artifacts
for each native workflow run. Run hardware tests serially:

~~~powershell
cargo test --locked -p layer-windows --lib d3d12_windows_hdr_documents_delivery_history_cancellation_and_recovery -- --ignored --test-threads=1 --nocapture
cargo test --locked -p layer-windows --lib d3d12_windows_float32_signed_range_rejects_lossy_demotion_and_protects_exports -- --ignored --test-threads=1 --nocapture
cargo test --locked -p layer-render-wgpu --lib d3d12_hdr_float16_float32_display_switching_and_proof_match_cpu -- --ignored --test-threads=1 --nocapture
~~~

### HDR release limitations

- Physical HDR output, calibrated luminance/gamut, OS SDR brightness settings,
  real HDR/SDR monitor crossing, mixed DPI/adapters, hot-plug, suspend and driver
  reset remain untested. Synthetic capability changes and GPU pixel comparisons
  do not close these hardware acceptance gates.
- Sustained painting cadence, large-photo 24/45/60 MP latency, peak process/driver
  memory, concurrent save/export throughput and constrained-device behavior have
  not been measured for Windows HDR. No performance acceptance is claimed.
- Windows has no qualified gain-map JPEG/AVIF codec bundle; those exports remain
  explicitly unavailable. Unsupported gain-map/HLG/HEIF inputs follow the shared
  codec's rejection contract. The bounded EXR subset remains the shared flat
  scanline FLOAT contract, not arbitrary deep/multipart/tiled/HALF EXR.

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

Windows HDR authoring and D3D12 viewing are now implemented as described above.
Physical HDR and performance acceptance, unsupported codecs and the dirty-rendering
redesign remain separate work. Print proofing remains implemented. See [Windows acceptance](windows-acceptance.md)
for the wider release gates.
