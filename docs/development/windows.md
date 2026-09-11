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
settings under `%LOCALAPPDATA%\CapyAtelier\CapyCanvas\settings.json`. The native File menu and pickers provide New, Open, Save, Save As and PNG Export.
Layers uses virtualized native rows and bounded asynchronous GPU thumbnails.
Layer menus and editing controls share their policy with the other ports.
The Layers image picker decodes oriented sRGB pixels on the document worker;
shared Core owns insertion, Undo and embedded project assets. The full editor
docking layout and runtime filter package import remain in progress.
Project transport handles background work, save checkpoints and replacement of
local files; export does not mark the editable project as saved.

## Validate

```powershell
cargo test --locked -p layer-host -p layer-windows --lib
```

The [Windows host notes](../../apps/layer-windows/README.md) contain interaction
and diagnostic commands. The [implementation record](../history/windows-implementation.md)
tracks workspace, lifecycle and physical-input acceptance gaps. Replayed input and
UI Automation checks do not replace real tablet and display validation.
