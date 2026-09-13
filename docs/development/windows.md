# Windows development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Windows client uses C++/WinRT and WinUI 3 controls. Its Rust bridge uses
`NativeHost` and the shared wgpu D3D12 renderer, presenting through a
`SwapChainPanel`.

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

The Windows App SDK runtime is copied beside the executable. No UWP application
package or generated application XAML is needed for this development build.

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
The Layers image picker decodes oriented sRGB pixels on the document worker;
shared Core owns insertion, Undo and embedded project assets. Panel configuration
and toolbar management use shared layout and actions. New Toolbar can start empty
or copy a saved definition; the native manager saves, adds, renames and deletes
library entries while keeping installed copies independent.
GPU Navigator previews use compositor clips when overlapping native panels.
The full editor preset and titlebar-aware Zen layout are available. Incremental
workspace messages retain panel models while native translation transforms move
floating panels, resize grips and GPU overview allocations. Full content refresh,
motion and camera updates retain their separate ordering rules. Complete
workspace gesture acceptance remains in progress. Runtime filter JSON/WGSL loads
from editable packaged assets through background file transport and the shared
GPU validator; compatible live replacement preserves current parameter values.
New Window creates independent native windows with shared preferences and storage. Native task workspace management uses the
shared store, preview and ownership policy, with a configurable scrolling titlebar switcher.
Project transport handles background work, save checkpoints and replacement of
local files; export does not mark the editable project as saved.

## Validate

```powershell
cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib
./apps/layer-windows/scripts/exercise-persistence.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-manager.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-startup-close.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-manager-focus.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-multiwindow.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-runtime-filters.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-toolbar-library.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-toolbars.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
```

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
Remove-Item Env:CAPY_SMOKE_TEST,Env:CAPY_PRESENT_PROBE,Env:CAPY_TEST_DISPLAY,Env:CAPY_TEST_PRIMARY -ErrorAction SilentlyContinue
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
per assertion rather than dumping whole models or mixing revisions. A timeout does not prove
the app exited: inspect its state and close only the owned test app with
`./apps/layer-windows/scripts/exercise-window.ps1 -ProcessId $review.Id -Action Close`,
which checks successful exit. Close the diagnostic PowerShell session afterward.
In test scripts, remove flags with `Remove-Item Env:NAME`: passing `$null` to
`.NET SetEnvironmentVariable` can leave an empty, still-enabled flag on newer runtimes.

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
Run [presentation measurements](../../apps/layer-windows/README.md#presentation-probe)
separately, with diagnostic tracing off and no competing builds or GPU tests.
Probe the actual display configuration first. Elevate only the capture script
if Windows denies ETW access. UI Automation and replay do not establish physical
pen/touch behavior, painting cadence or input latency.
