# Windows feature-gap review against GTK, Web and Android

Current-code follow-up: 2026-09-19. See the follow-up section below for the
portable photo, GPU tone-guide and native Proof panel work. Earlier codec and
panel limitations are historical, not the current implementation contract.

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
- Gain-map JPEG/AVIF was unavailable at this earlier HDR review. The portable
  photo follow-up below supersedes that limitation. Inputs outside the shared
  codec matrix still fail explicitly. The bounded EXR subset remains the shared flat
  scanline FLOAT contract, not arbitrary deep/multipart/tiled/HALF EXR.

## Current-code follow-up (2026-09-19)

Started with a clean Windows main at `8eebe84f`, fast-forwarded to
`a23c627a`, then integrated `54ca69cc` and concurrent shared-renderer work
through `29dddabf`, the disjoint tab/storage updates through `e32ded49`,
and the Multiply preview-routing fix `5004f1be`. Read AGENTS.md, the drag convention,
this report and current GTK, Android, Web, Windows and shared Rust sources.
Reference-host findings below are source inspection, not fresh GTK/Android/Web
runtime acceptance.

| Area | Current-code finding and Windows change |
| --- | --- |
| Portable photos | GTK `files/export.rs`, Android `documents.rs`/`inspection.rs` and Web `output.rs` now use shared portable gain-map codecs. Windows still filtered those formats and returned a codec-bundle error. Windows now uses shared gain-map preview/encoding, quality/background/range policy and its existing cancellable atomic file worker. Shared color-aware choices now include explicit clipping variants; transparent JPEG requires an explicit matte. The Open picker exposes AVIF/HEIF/HEIC/HIF alongside EXR. Shared decoding already owns tagged source precision and interpretation. |
| GPU tone guides | GTK `local_tone_view.rs`, Android and Web `hdr.rs` retain shared GPU guides. Windows used the downloaded CPU guide. Windows now publishes `GpuToneGuide` directly to canvas/Navigator, retains only compatible previous illumination during edits, waits for idle, throttles animated analysis, rejects canceled/stale completions, and releases guides before replacing a removed device. Guide construction and compatibility remain shared Rust. |
| Proof workspace | Shared `Panel::Proof` excluded Windows and its native panel builder had no Proof content. Windows now offers a retained panel in the shared Paint/Photo presets and through the Window menu, with Off/SDR/Print, four shared SDR numeric controls, print setup and gamut warning. Numeric expressions, ranges, mode policy, cancellation and one-step history remain shared. Fixed shared percent resolution so typed and stepped fractional values survive normalization. Native fields retain widgets and discard drafts on document identity changes; shared print preparation and existing native profile pickers remain in use. |
| Dependency notices | Windows packaging looked for vendored crates in registry directories and omitted new codec notices. Collection now resolves actual vendor paths, preserves local attribution metadata, and reuses the checked-in original Zune notice with a checksum normalized for Windows checkout line endings. |
| Drawing tabs | GTK `documents.rs`, Android `native/src/document_tabs.rs`, Web `src/document_tabs.rs` and Windows `native/src/document_tabs.rs` use shared `DocumentSessions`/`DocumentTabs`, retained editors, backing-store budgets and per-drawing recovery. Windows [`DrawingTabs.h`](../../apps/layer-windows/DrawingTabs.h) shows the plain title and dimensions for one drawing, and live-sliding tabs or the compact selector for several. `exercise-hdr.ps1` covers the title, unsaved markers, shortcuts, the selector and reordering. |

No new reorder gesture is introduced. The Proof panel uses the existing native
workspace panel/tab/grab-handle integration, including retained drawer views.
Windows numeric fields provide the four SDR parameters; the GTK/Web/Android
combined graphical dial and their consolidated Proof menu presentation are not
implemented by this follow-up.

Persistence uses the upstream version-6 LZ4 project format. Earlier v4/v5
projects and recovery copies are deliberately rejected by shared storage; there
is no migration reader in current main. Tests use fresh isolated v6 profiles and
do not establish compatibility with old Windows saved files.

### Follow-up validation

The final integrated runtime baseline is `e32ded49` plus this patch. The later
`5004f1be` delta only routes non-normal dry material to the established fragment
path; its final build/routing check is recorded below. GPU and GUI runs were
serial, without competing compilation, on Intel Iris Xe / driver `32.0.101.6737`.
All logs and disposable profiles are under ignored `artifacts/windows`.

| Check | Result and scope |
| --- | --- |
| Integrated Release build | Rust and C++/WinUI passed on `e32ded49`. Log: `parity-current-build-upstream.log`. |
| Integrated Rust library suites | Passed 499 UI, 99 core, 28 host and 119 Windows tests (745); 15 explicitly ignored. Windows used one test thread. Logs: `parity-current-upstream-shared.log` and `parity-current-upstream-windows.log`. The shared photo/color suite also passed 116 tests, with 14 ignored, before the final storage/tab integration: `parity-current-photo-core-retry.log`. |
| Shared policy regressions | The integrated UI suite includes explicit HDR clipping choices and a new test for fractional percentage expressions/steps across all four Proof parameters. Separate focused runs: `parity-current-export-choices.log`, `parity-current-proof-numeric.log`. |
| Strict Clippy | Windows/shared host, all targets, no dependency linting, warnings denied: passed. Log: `parity-current-upstream-clippy.log`. |
| Native C++ input/queue checks | Passed admission, ordering, cancellation/refusal ownership, retained models, query scheduling and completion. Log: `parity-current-native-input.log`. |
| Hardware HDR workflow | Passed F16/F32 GPU tone-guide comparison with a CPU reference, guide reuse across rendition edits, eight gain-map deliveries/reimports, canceled-output preservation, Proof cancel/undo/redo, v6 persistence and pending-analysis/device replacement (163.49 s). Log: `parity-current-upstream-d3d12.log`. |
| Hardware signed-range workflow | Passed F32 range preservation, lossy-demotion refusal and destination protection on the earlier `29dddabf` integration (46.69 s). Log: `parity-current-d3d12-signed.log`. |
| Native F16 and F32 HDR journeys | Both passed on `e32ded49`: invalid input/Escape, retained numeric widget identity, fractional commit and undo/redo, viewing-mode history, all four gain-map outputs and JPEG/AVIF reopen, PNG/PQ/EXR, painting history, synthetic display changes, GPU replacement and Unicode save/reopen. Logs: `parity-current-upstream-gui-f16.log`, `parity-current-upstream-gui-f32.log`. |
| Native print-proof journey | Passed first-use/picker cancellation, profile-library draft return, shared options, viewing/gamut switches, recipe/artwork history, uncontaminated exports, device replacement and save/reopen. Log: `parity-current-upstream-proof-ui.log`. |
| Crash/restart recovery | Passed checkpoint restore, durable origin retirement, later edits surviving clean close and explicit discard. Log: `parity-current-upstream-restart.log`. |
| Dependency notices | Collected 215 Cargo packages plus pinned NuGet/native runtime notices using actual vendored sources. Log: `parity-current-notices-final.log`. |

The direct debug HDR integration test exceeded the default test-thread stack
while extending coverage to AVIF. Its passing runs used
`RUST_MIN_STACK=8388608`, matching the native document worker's existing 8 MiB
allocation. No production stack limit or recovery deadline was increased.
An early Proof-worker timeout during competing compilation did not recur in
isolation; its 15-second deadline is unchanged. This does not qualify recovery
under load. An interrupted linker left one malformed generated PDB; only that
exact file was removed before the passing core rebuild.

The GUI fixture respects the already-docked Proof tab in the shared preset,
waits for Undo completion, explicitly chooses a JPEG matte for transparent
artwork, and discards imported test photos before opening the next. It retains
foreground/process input guards. GUI validation caught and corrected the native
panel builder's missing dedicated Proof dispatch; shared numeric tests caught
whole-unit rounding in the percentage specifications.

Reproduce hardware checks with a fresh absolute `CAPY_SETTINGS_DIRECTORY` under
ignored artifacts and `RUST_MIN_STACK=8388608`. Run the ignored Windows HDR and
signed-range tests individually with `--test-threads=1`. Run
`exercise-hdr.ps1 -Depth F16`, `exercise-hdr.ps1 -Depth F32`,
`exercise-proof.ps1` and `exercise-artwork-recovery.ps1` serially against the
Release executable in `artifacts/windows/parity-current`.

The final `5004f1be` integration also passed the Release Windows rebuild and
the focused `non_normal_dry_material_uses_the_fragment_path` regression. Logs:
`parity-current-build-final-main.log` and `parity-current-final-routing.log`.
The full runtime evidence above is scoped to the preceding `e32ded49` build.

## Retained drawings and Proof consolidation (2026-09-20)

Continued from `a2bd2822` and fast-forwarded the independent codec cleanup
`8bd5509e` and Apple parity work through `599a2ee6` while preserving the Windows
changes. Overlapping shared APIs were consolidated on upstream `exchange_with`;
Apple camera-generation changes remain intact. Rechecked current GTK
`documents.rs`/`proof_dial.rs`, Android `native/src/document_tabs.rs`, Web
`src/document_tabs.rs`, and the shared Rust drawing/Proof code. Those reference
hosts were inspected in source; they were not run on this Windows machine.
This section supersedes the single-document and separate SDR-dialog descriptions
in the earlier dated evidence above.

Windows now uses shared `DocumentSessions`/`DocumentTabs` for retained drawing
membership, labels, admission, order history and inactive tile budgets. Native
WinUI provides the retained strip, compact selector, capture, system slop and
hold recognition. New/Open retain dirty drawings; each drawing keeps its view,
artwork history, save checkpoint and recovery lease. Selecting an inactive drawing
releases the outgoing renderer and builds its replacement on the file worker.
Inactive tiles spill through shared Rust storage; Windows supplies temporary,
last-handle-deleted backing files. Failed writes preserve resident data and can
be retried from Drawing options. Closing one drawing and closing the window use
the same Save/Discard/Cancel workflow, advancing through the retained drawings.
Recovery publishes a new tab and retains the existing editor.

The native Proof panel now includes the shared graphical SDR dial, native pointer
capture/cancellation, keyboard steps and resets, and the existing numeric controls.
Shared Rust owns geometry, hit mapping, recipe validation and one-step history.
Numeric actions update one field against the live recipe so queued edits do not
restore stale fields. Windows uses the consolidated Proof command and native print
setup. The obsolete Windows SDR modal, width/height-only creation action and
single-document host ownership path have been removed. Native request identities
remain monotonic across drawing activation so retained dialog views cannot swallow
a new request with an old ID.

The product is unreleased: older project/recovery formats are deliberately rejected;
migration and backward compatibility are not requirements. No compatibility reader,
old-format data structure or migration shim was added.

Validation runs use isolated profiles under ignored `artifacts/windows`. GPU and
GUI checks run serially, without competing compilation. The integrated Rust
library suites passed 106 core, 503 UI, 28 host and 119 Windows tests (756 total;
16 explicitly ignored). Strict Windows/host Clippy also passed. Logs:
`parity-tabs-integrated-cpu.log`, `parity-tabs-integrated-clippy.log`.

Native validation exposed and corrected retained-button capture, selector keyboard
selection, touch/pan arbitration in the Proof dial, an acquired DXGI image crossing
a tab change, and a retained removed-device handle that blocked GPU recovery.
Visual inspection also found that a redundant background-only startup frame could
clear an already prepared drawing. The HDR fixture now checks visible canvas ink
after reopening, in addition to independently comparing exported pixels.

| Runtime check | Result and scope |
| --- | --- |
| Integrated Release build | Passed Rust and WinUI on `599a2ee6` plus this change. Logs: `parity-tabs-build11.log`, `parity-tabs-build12.log` (native popup accessibility state). |
| F16/F32 native HDR journeys | Passed retained tabs/compact selector, synthetic mouse/touch/pen cancellation and order history, independent artwork history, graphical/numeric Proof controls, delivery formats, Unicode persistence, actual D3D12 removal/replacement, and visible reopened master/gain-map artwork. Logs: `parity-tabs-integrated-gui-f16.log`, `parity-tabs-integrated-gui-f32-4.log`. |
| Native print-proof journey | Passed first-use/picker/library cancellation, recipe and artwork history, uncontaminated exports, device replacement and save/reopen. Log: `parity-tabs-integrated-proof-ui.log`. |
| Crash/restart recovery | Passed restoring into a retained tab, durable origin retirement, Later across clean close/restart, and explicit discard. Log: `parity-tabs-integrated-recovery-ui.log`. |
| Exhausted GPU recovery | Passed native Save/Save As and canceled pickers, committed raster preservation, queued-contact cancellation, dirty-close Cancel/Discard, Unicode persistence, corrupt Open preservation, retained New/Open cancellation and durable reopen with identical exported pixels. Log: `parity-tabs-integrated-documents-failed-gpu.log`. |

The full GUI journeys above ran on `599a2ee6` plus this change. The later disjoint
renderer commit `dd7b3ec9` was fast-forwarded without disturbing the Windows
work. Its final Release rebuild and strict Clippy passed (`parity-tabs-build13.log`,
`parity-tabs-final-clippy.log`). Final-integration checks:

| Check | Result and scope |
| --- | --- |
| Windows library suite | 119 passed, 15 explicitly ignored; one test thread. Log: `parity-tabs-final-windows-cpu.log`. |
| Cached/native raster restore | Three hardware regressions passed: cached/cold ordering, scalar masks, pixel preservation, reuse and late-failure atomicity. The performance benchmark remains ignored. Log: `parity-tabs-final-restore-gpu.log`. |
| Retained drawing hardware workflow | Passed inactive backing spill failure/retry, exact pixels, independent artwork and order history, separate durable recovery copies, Save/Cancel/Close, accepted-Open cancellation, and selecting/saving an inactive CPU editor without a GPU. Log: `parity-tabs-final-tabs-gpu.log`. |
| HDR hardware workflow | Passed F16/F32 delivery, tone-guide reconstruction, cancellation, history, persistence and device replacement on Intel Iris Xe / D3D12 / driver `32.0.101.6737` (166.80 s). Log: `parity-tabs-final-hdr-gpu.log`. |
| Native C++ input/queue tests | Passed retained models, motion/camera ordering, completion boundaries, admission/refusal ownership and bounded query scheduling. Log: `parity-tabs-final-native-input.log`. |

The final native document journey also passed two D3D12 replacements with queued
and active pen strokes, exact exported pixels, recovered thumbnails, Undo/Redo,
retained New/Open cancellation, corrupt-file preservation, Unicode Save/Save As,
Preferences draft preservation and clean/dirty close flows. Log:
`parity-tabs-final-documents-recovery-ui.log`.

Run GPU tests individually with `--test-threads=1`, a fresh absolute
`CAPY_SETTINGS_DIRECTORY`, and `RUST_MIN_STACK=8388608` (the existing native
worker stack size). Run GUI fixtures serially against the Release executable.
The HDR fixture waits for native popup Closed state and restores list-row focus
after context-menu actions before sending Escape. These fixture synchronizations
do not increase production recovery or close deadlines.

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
