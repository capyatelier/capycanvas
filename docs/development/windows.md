# Windows development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

See the [current acceptance status](windows-acceptance.md) for validated workflows,
package scope, known differences and remaining release gates.

The Windows client uses C++/WinRT and WinUI 3 controls. Its Rust bridge uses
`NativeHost` and the shared wgpu D3D12 renderer, presenting through a
`SwapChainPanel`.

The [Vulkan/native presentation investigation](../history/windows-vulkan-presentation-20260922.md)
records measured backend differences, shared-buffer feasibility probes and the
remaining latency experiment. Implementation and any backend switch are deferred.

## Prerequisites

Install Rust stable for `x86_64-pc-windows-msvc`, Visual Studio C++ Build Tools
with v143 or v145, Windows SDK 10.0.26100.0, and the official NuGet CLI. NuGet can
be on `PATH` or at `~/.local/tools/nuget/nuget.exe`.

Use a Windows development machine with a hardware D3D12-capable GPU. The current
scripts produce unpackaged development builds, not a Microsoft Store submission.

## Build and run

From PowerShell at the repository root:

```powershell
./apps/layer-windows/scripts/build.ps1
./apps/layer-windows/scripts/build.ps1 -Configuration Release
```

The script restores the versions pinned in
[`packages.config`](../../apps/layer-windows/packages.config) and builds the Rust
and C++ parts. Packages go into ignored `artifacts/windows/packages`; use
`-PackagesDirectory` to reuse an existing restore. Output is in
`artifacts/windows/Debug` or `artifacts/windows/Release`. Launch the generated
`CapyCanvas.exe` from the chosen output directory.

The build copies `dxcompiler.dll` from the pinned `Microsoft.Direct3D.DXC` package
beside the executable. D3D12 shaders compile with that app-local DXC, which uses
its internal validator and needs no `dxil.dll`. Without it the host falls back to
the system FXC compiler, which takes about five times longer for the startup
shader set. Set `WGPU_DX12_COMPILER=fxc` to compare the two compilers.

The Windows App SDK runtime is copied beside the executable. No UWP application
package or generated application XAML is needed for this development build.

## Current Web references on Windows

A fresh Web reference also needs a Wasm-capable Clang for the shared raster
compression dependency. A portable [WASI SDK](https://github.com/WebAssembly/wasi-sdk/releases)
provides Clang and llvm-ar without changing the installed Windows toolchain.
With its extracted directory assigned to `$wasiSdk`, build the current Web source:

~~~powershell
$env:CC_wasm32_unknown_unknown = Join-Path $wasiSdk 'bin/clang.exe'
$env:AR_wasm32_unknown_unknown = Join-Path $wasiSdk 'bin/llvm-ar.exe'
cargo build --locked --release -p layer-web --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir apps/layer-web/pkg target/wasm32-unknown-unknown/release/layer_web.wasm
~~~

Use the wasm-bindgen version pinned in the Web manifest. These target-specific
compiler variables apply to the Web reference build. Follow the [matched editor capture commands](../../apps/layer-windows/README.md#matched-editor-captures)
for isolated profiles and native/Web evidence. The reviewed reference build used
WASI SDK 34; an older generated Wasm bundle is not evidence for current source.

## Portable package

Build an unsigned Windows 11 x64 ZIP from a committed checkout:

~~~powershell
./apps/layer-windows/scripts/package.ps1
~~~

This runs the Release Rust/WinUI build into a fresh output directory, collects
the self-contained Windows App SDK and app-local Visual C++ runtime, and adds
project, dependency and runtime notices. The pinned Cargo and NuGet dependencies
are recorded with the compiler/SDK versions and source commit. Every payload file
has a size and SHA-256 entry in package-manifest.json. Packages and build logs
stay under ignored artifacts/windows/distribution.

Use -SkipRestore when the pinned NuGet packages are already available. Uncommitted
changes require -AllowDirty, which marks both the filename and manifest as a
development package. A clean build fails if source changes during packaging.
The packager assembles the same payload twice with sorted paths and fixed ZIP
timestamps, requires identical hashes, and writes a .sha256 sidecar. This verifies
deterministic archive assembly; it does not establish identical compilation
across machines or toolchain installations.

Extract the complete archive and launch CapyCanvas.exe. To check an archive on an
unlocked Windows desktop:

~~~powershell
./apps/layer-windows/scripts/exercise-package.ps1 -Archive <path-to-zip>
~~~

The fixture verifies the complete file inventory, extracts to a fresh path
containing spaces, and launches with an unrelated working directory and isolated
preferences. It checks packaged filter loading, app-local runtime origins,
drawing/Undo/Redo, pan, resize and clean exit. A failed live app is retained for
inspection. Captures and diagnostics stay outside the payload.

Deployment follows Microsoft's
[self-contained Windows App SDK guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/self-contained-deploy/deploy-self-contained-apps)
and [Visual C++ redistribution guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files).
Clean-machine installation, distribution signing, full visual and physical-input
acceptance and sustained painting performance remain separate acceptance work.

## MSIX package

Convert an existing portable build into an unsigned MSIX, using its result.json:

~~~powershell
./apps/layer-windows/scripts/package-msix.ps1 -PortableResultFile <portable-result.json> -Version 1.0.0.0
./apps/layer-windows/scripts/test-msix.ps1 -ResultFile <msix-result.json>
~~~

Run from an STA PowerShell session on Windows. Both Windows PowerShell 5.1 and
PowerShell 7 are supported. The packager verifies the portable inventory before
copying it, retains the app-local runtimes and notices, and generates package
logos from the shared symbolic brand asset. The application runs as a
`packagedClassicApp` at `mediumIL`, with the `runFullTrust` capability.
Output stays under ignored artifacts/windows/msix.

The manifest records the application source commit separately from the packager
source commit, along with hashes of the packaging scripts and brand asset.
Uncommitted sources require -AllowDirty and mark the result and filename as a
development package. The four-part package version requires a nonzero major
component and components no greater than 65535.

MakeAppx performs its normal semantic validation. It writes wall-clock ZIP
timestamps even when source timestamps are fixed, so normalize-msix.ps1 checks
the ZIP32/ZIP64 headers and replaces only their date/time fields before signing.
Compressed blocks, file contents and block maps are preserved. It refuses signed
packages and validates all headers before changing any bytes. Two assemblies
must produce the same SHA-256. This establishes repeatable archive assembly for
identical inputs, rather than bit-identical compiler output across machines.

The test verifies the complete inventory, URI-encoded license paths, extracted
hashes using MakeAppx, activation metadata, repeat logo generation and
normalization idempotence. Independent ZIP32 fixtures check content preservation
and refusal to modify a signed archive. Invalid payload hashes, duplicate and
escaping paths, undeclared files, versions and identity lengths are rejected.
These checks do not establish installed application behavior.

For local Windows 11 installation testing, create a separate test identity:

~~~powershell
./apps/layer-windows/scripts/package-msix.ps1 -PortableResultFile <portable-result.json> -UnsignedTestIdentity
~~~

This adds .Test to the identity and Microsoft's unsigned-test publisher OID.
[Microsoft requires administrator privilege for unsigned packages containing executable activations](https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package).
From Administrator PowerShell, install the exact reviewed test artifact with
`Add-AppxPackage -Path <test.msix> -AllowUnsigned`. Check for an existing test
installation before replacing it, and remove the test package after acceptance.
The test identity is for local testing, not distribution.

The standard output uses identity CapyAtelier.CapyCanvas and publisher
CN=Capy Atelier. Pass -Publisher to match the distribution certificate's exact
subject. It must be
[signed before distribution](https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview);
never normalize the archive after signing. Installed identity, launch, update,
uninstall, clean-machine behavior and distribution signing remain acceptance
gates. The first ordinary-user install attempt was rejected by Windows because
the unsigned package contains an executable.

## How the host works

The native shell collects pointer history and dispatches shared commands. A render
worker owns GPU and engine work, keeping UI controls independent of GPU waits.
Attaching or reconfiguring the swap chain requires coordination with the UI thread;
the worker pauses for that surface operation, not for ordinary widget updates.

Shared snapshots drive tool, color, layer, Navigator and preference controls. Windows persists
preferences in `settings.json` and workspaces in `workspaces.sqlite3` under
`%LOCALAPPDATA%\CapyAtelier\CapyCanvas`. Workspace layout history and latest tool
settings use the shared manager and asynchronous SQLite worker. The canvas owner
polls completed operations; database I/O stays off the UI and render threads.
Autosave and close preserve edits accepted while an earlier save is pending.
The native File menu and pickers provide New, Open, Save, Save As and PNG Export.
Layers uses virtualized native rows and bounded asynchronous GPU thumbnails.
Layer menus and editing controls share their policy with the other ports.
The Layers image picker and image drops decode on the document worker and place
the color-managed sources through the shared placement workflow. Panel configuration
and toolbar management use shared layout and actions. New Toolbar can start empty
or copy a saved definition; the native manager saves, adds, renames and deletes
library entries while keeping installed copies independent. Toolbars host the
shared brush size/opacity sliders (stamp previews and bookmarks) and Tool Options,
and dock to compact edge regions; Rust owns their fitting, numeric math and stamps.
GPU Navigator previews use compositor clips when overlapping native panels.
The full editor preset and titlebar-aware Zen layout are available. Incremental
workspace messages retain panel models while native translation transforms move
floating panels, resize grips and GPU overview allocations. Full content refresh,
motion and camera updates retain their separate ordering rules. The
[acceptance status](windows-acceptance.md) records completed gesture journeys and
remaining physical-device checks. Runtime filter JSON/WGSL loads
from editable packaged assets through background file transport and the shared
GPU validator; compatible live replacement preserves current parameter values.
New Window creates independent native windows with shared preferences and storage. Native task workspace management uses the
shared store, preview and ownership policy, with a configurable scrolling titlebar switcher.
Project transport handles background work, save checkpoints and replacement of
local files; export does not mark the editable project as saved.
Panel measurements follow Web: inactive tabs of fitted groups are measured from
offscreen copies that cannot dispatch, every non-toolbar panel except Color
reports scroll metrics so the shared fitter can shrink it to four rows, and a
fitted Color panel reports its natural height at the column width.
Keys reach Rust from the XAML thread while pointer samples come from the input
thread, so a contact start carries its own WinUI key modifiers: when they differ
from the last key event, the host queues the modifier change ahead of the
contact. Selection modes latched by Shift/Alt therefore match the pressed keys
even when a key event is late. The window subclass drops the Alt-only keyboard
menu (`SC_KEYMENU` without a character): Windows otherwise enters menu mode when
Alt is released and stalls presentation until the next click. Alt+Space and
Alt+F4 are unchanged.

## Validate

The [independent filter comparison](windows-filter-qualification.md) records
D3D12/Vulkan migration checks and distinguishes them from the still-failing
Linux PNG reference comparison.

```powershell
cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib
./apps/layer-windows/scripts/exercise-persistence.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-manager.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-startup-close.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-manager-focus.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-multiwindow.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
pwsh -NoProfile -Sta -File ./apps/layer-windows/scripts/exercise-multiwindow.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe -FailPreferences
pwsh -NoProfile -Sta -File ./apps/layer-windows/scripts/exercise-documents.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe -RecoverGpu
./apps/layer-windows/scripts/exercise-runtime-filters.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-toolbar-library.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-toolbars.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
pwsh -NoProfile -Sta -File ./apps/layer-windows/scripts/exercise-toolbar-components.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe
pwsh -NoProfile -Sta -File ./apps/layer-windows/scripts/exercise-color-picker.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe
pwsh -NoProfile -Sta -File ./apps/layer-windows/scripts/exercise-selection.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe
pwsh -NoProfile -Sta -File ./apps/layer-windows/scripts/exercise-palettes.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe
pwsh -NoProfile -Sta -File ./apps/layer-windows/scripts/exercise-transparency.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe
```

The document journey checks native import/save/export pickers, Unicode paths,
corrupt-file recovery, Preferences drafts and save/cancel/close behavior. With
`-RecoverGpu`, it also verifies two GPU reconstructions, queued and active
controlled pen strokes, identical exported images, thumbnails and Undo/Redo.
Preferences opens through Edit, independently of the configured titlebar buttons.

The persistence fixture owns disposable profiles through the absolute
`CAPY_SETTINGS_DIRECTORY` override. It verifies autosave, restart, final-edit
saving and storage recovery with native UI Automation. Its profiles, captures,
and reports remain under ignored `artifacts/windows`.

The [Windows host notes](../../apps/layer-windows/README.md) contain interaction
and diagnostic commands. The [implementation record](../history/windows-implementation.md)
tracks workspace, lifecycle and physical-input acceptance gaps. Replayed input and
UI Automation checks do not replace real tablet and display validation.

## Debugging and visual checks

Rebuild with the script above after Rust or shared UI changes: MSBuild alone
copies the existing Rust DLL and can leave you testing stale code. Close owned
test apps before rebuilding. Run strict Clippy and a fixture for the changed area:

```powershell
cargo clippy --locked -p layer-windows --all-targets -- -D warnings
./apps/layer-windows/scripts/exercise-layers.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
```

Read the chosen `exercise-*.ps1` script's parameters: `-Executable` fixtures
launch their own app; `-ProcessId` fixtures use an already-running app. Run native
UI fixtures sequentially in an unlocked desktop session. Use UI Automation
control IDs and state assertions. Before injecting pointer input, wait for both
published state and arranged control bounds, then verify the hit belongs to the
owned app. A model update alone does not mean the visible rows have moved.

For manual diagnosis or matched captures, use a fresh PowerShell session at the
repository root, a disposable profile and the executable's working directory:

```powershell
$exe = (Resolve-Path ./artifacts/windows/Debug/CapyCanvas.exe).Path
$run = Join-Path (Get-Location) ('artifacts/windows/review/' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run -Force | Out-Null
$env:CAPY_SETTINGS_DIRECTORY = Join-Path $run 'profile'
$env:CAPY_TRACE_UI = '1'
Remove-Item Env:CAPY_SMOKE_TEST,Env:CAPY_TEST_DISPLAY,Env:CAPY_TEST_PRIMARY -ErrorAction SilentlyContinue
$review = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe) -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
$null = $review.Handle
./apps/layer-windows/scripts/capture-editor.ps1 -ProcessId $review.Id -OutputDirectory $run
```

The capture fixture waits for workspace ownership, brush/filter readiness,
visible thumbnails and stable layout/camera state, then records both themes and
paper extending beneath the titlebar. It measures the current display scale;
do not assume a particular resolution or DPI.

On failure, run `./apps/layer-windows/scripts/inspect-window.ps1 -ProcessId $review.Id`
and inspect stderr before relaunching. Opt-in `ui-state-<pid>-<window>.json`,
`camera-state-<pid>-<window>.json` and `lifecycle.log` are in the app's working
directory. Check snapshot `process_id`, `window_id` and freshness. The per-window
JSON files use atomic replacement; compatibility files `ui-state.json` and
`camera-state.json` can be read mid-write. Read relevant fields from one snapshot
per assertion rather than dumping whole models or mixing revisions. Compare a
suspect snapshot's workspace revision with the native workspace's UIA ItemStatus;
a leftover `.pending` file can indicate failed diagnostic replacement even when
the app advanced correctly. A timeout does not prove
the app exited: inspect its state and close only the owned test app with
`./apps/layer-windows/scripts/exercise-window.ps1 -ProcessId $review.Id -Action Close`,
which checks successful exit. Add `-DiscardUnsaved` only for an owned disposable
review whose synthetic edits can be discarded. Close the diagnostic PowerShell session afterward.
In test scripts, remove flags with `Remove-Item Env:NAME`: passing `$null` to
`.NET SetEnvironmentVariable` can leave an empty, still-enabled flag on newer runtimes.

For a native crash, reproduce with a Debug build under Visual Studio's native
debugger, WinDbg or CDB, attached to the owned review PID. Load the PDBs from that
exact build and Microsoft's public symbols. With CDB on `PATH`, the same review
session can be attached from PowerShell:

```powershell
cdb -p $review.Id -logo (Join-Path $run 'debugger.log')
```

At the debugger prompt, use `.symfix` with an absolute cache directory under
`artifacts/windows`, then `.sympath+` with the executable directory (quote paths
containing spaces). Run `sxe av` and `g` to stop on access violations. At the
fault, record the exception code and `kv` stack before closing or restarting;
when inspecting a crash dump, select its exception context with `.ecxr` first.
See Microsoft's [exception controls](https://learn.microsoft.com/en-us/windows-hardware/drivers/debuggercmds/sx--sxd--sxe--sxi--sxn--sxr--sx---set-exceptions-)
and [symbol setup](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/setting-symbol-and-source-paths-in-cdb).
For workspace gestures, inspect the `Drawing workspace` element's UIA HelpText
with `CAPY_TRACE_UI=1`: it records the actual pointer device, capture state and
last cancellation. Its ItemStatus reports layout publication separately. Compare
an injected-input failure with physical input before changing native capture;
they can differ even when the test reports the expected device type.

Keep debugger logs and dumps local: they can contain document contents and paths.
Debugger runs are for diagnosis; measure presentation without an attached debugger.

For a browser reference, follow the [web prerequisites](web.md#prerequisites)
and run `bash ./apps/layer-web/build.sh` (for example, using Git Bash). Install
Node.js and the Python dependencies in [the visual tools](../../tools/visual/requirements.txt),
and set `CAPY_CHROME` to the installed Chrome executable. The capture tool starts
its own local server and temporary browser profile. From the same checkout, run
the real app in hardware-backed Chrome using the native manifest:

```powershell
$manifest = Join-Path $run 'fixtures.json'
$first = (Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json).fixtures[0]
node tools/visual/chrome-capture.mjs $first.viewport[0] $first.viewport[1] $first.scale $run light windows-editor $manifest
python tools/visual/compare.py (Join-Path $run 'web-light-initial.png') (Join-Path $run 'native-light-initial.png') --output (Join-Path $run 'diff-light-initial')
```

One Chrome invocation processes all manifest scenes. Compare every image pair and
inspect the screenshots as well as geometry reports; passing geometry does not
establish raster identity. Preserve full client captures and measured XAML
boundaries, and do not rescale images, mask discrepancies or loosen reference
tolerances to make a comparison pass. See [matched editor captures](../../apps/layer-windows/README.md#matched-editor-captures)
for details.

Keep profiles, databases, images, traces and machine diagnostics under ignored
`artifacts/windows`; publish only reviewed source and sanitized summaries.
Run [pen latency measurements](windows-pen-latency-20260920.md#reproduction)
separately, with diagnostic tracing off and no competing builds or GPU tests.
UI Automation and replay do not establish physical pen/touch behavior, painting
cadence or input latency.

## GPU reconstruction checks

Run the document and lifecycle fixtures with actual D3D12 device removal enabled:

```powershell
./apps/layer-windows/scripts/exercise-documents.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe -RecoverGpu
./apps/layer-windows/scripts/exercise-documents.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe -FailGpu
./apps/layer-windows/scripts/exercise-lifecycle.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe -RecoverGpu
./apps/layer-windows/scripts/exercise-multiwindow.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe -RecoverGpu
cargo test --locked -p layer-windows --lib device::tests::validation_error_releases_pipeline_and_allows_device_replacement -- --ignored --exact --nocapture
cargo test --locked -p layer-windows --lib documents::recovery_tests -- --ignored --test-threads=1 --nocapture
cargo test --locked -p layer-windows --lib filter_packages::tests::recovery_tests -- --ignored --test-threads=1 --nocapture
```

The native fixtures create isolated profiles. The document check removes the
process-owned device twice, then compares exported PNG bytes and verifies
history, state, thumbnails and subsequent saving. It queues a complete pen stroke
while reconstruction runs, then repeats with a stroke already visibly painting
and its final samples queued during reconstruction. The smoke samples vary
pressure, tilt and twist; per-process lifecycle traces verify admission occurred
inside the reconstruction interval. A missed interval fails the fixture.
The lifecycle check overlaps
removal with startup, minimized windows and close decisions. The Rust regression
checks failed-pipeline cleanup and replacement of a removed hardware device.
The multiwindow fixture removes the shared device from each of two open windows,
checking reconstruction in the idle sibling and independent document Undo/Redo.
The `-FailGpu` variant removes the device and prevents reconstruction in its
isolated test profile until the normal retry deadline expires. It checks Save,
Save As, canceled pickers, Cancel/Discard close decisions, retained preferences,
and durable reopen with identical exported pixels. It also checks failure in Zen
mode: File remains accessible without changing the saved Zen preference. A
complete pen stroke and an unfinished tail are admitted during failed recovery;
both unrendered contacts are canceled, and the saved/reopened drawing must match
the completed raster edits from before removal.

When reconstruction fails, painting stops and completed host-backed raster edits
remain saveable. Queued and unfinished contacts are canceled; without a renderer,
raw pointer samples cannot produce raster pixels. Document/settings services
stay alive until an approved close, and
GPU-dependent commands are disabled. An accepted save can finish; a PNG export
that has not captured its image is canceled. Reopen the saved drawing in a new
window to resume painting.

The document recovery test module selects hardware D3D12 explicitly and removes
only its own process devices. Run it alone and serially. It exercises real worker
completions before adoption, including a decoded image whose original file is
already deleted, New/Open candidates with embedded image data, an accepted save,
a captured PNG ticket and a viewport upload. It verifies retained pixels/history,
failed-operation retry, protected export destinations and error return without
unwinding. The ordinary CPU suite separately covers canceling a deferred import.

The filter recovery module uses the same hardware selection/removal helpers and
must also run alone and serially. It removes the device after file transport has
started and after validation has been submitted but before publication. Original
package files are then deleted. Reconstructed pixels, parameter values and
one-step Undo/Redo must match uninterrupted replacement. Exhausted recovery must
cancel the candidate without changing the current catalog or document. These are
controlled worker/publication boundaries, not proof of removal during a particular
shader-compiler instruction or native pointer event.

The loader retains acquired source bytes while the device is removed or the
renderer is absent. If painting becomes suspended, pending reads settle as a
visible failure and late file results are discarded; new loads are rejected.
Ordinary tests cover suspension both during acquisition and while waiting for
a renderer, including completion after the failure has already been published.

Run these separately from performance measurements. They do not establish
physical driver-reset or suspend behavior, every native picker/input overlap,
or recovery during every filter operation.
