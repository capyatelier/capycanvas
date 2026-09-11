# Capy Canvas for Windows

Development prototype using WinUI 3/C++/WinRT and the shared Rust/wgpu D3D12
renderer. The prototype builds and runs with a GPU canvas, an independent input
dispatcher, correct DPI composition, and asynchronous resize/shutdown. Controlled
pointer replay verifies drawing, undo/redo, and a painted document continuing
behind titlebar controls. Real OS input, workspace parity, recovery, bounded input
transport, and presentation acceptance remain open; this is not a release package.

## Build

Install Rust stable for x86_64-pc-windows-msvc, Visual Studio C++ Build Tools
(v143 or v145), Windows SDK 10.0.26100.0, and the official NuGet CLI. The CLI
can be on PATH or under ~/.local/tools/nuget/nuget.exe.

From the repository root:

~~~powershell
./apps/layer-windows/scripts/build.ps1
./apps/layer-windows/scripts/build.ps1 -Configuration Release
~~~

The script restores the exact versions in packages.config into ignored
artifacts/windows/packages. Use -PackagesDirectory to reuse an existing restore.
Build output is artifacts/windows/Debug or artifacts/windows/Release.

The shell constructs WinUI controls in C++ and uses WinUI's built-in metadata
provider and native control templates. It does not require UWP application
packaging or generated application XAML. The Windows App SDK runtime is copied
beside the executable for unpackaged development.

## Diagnostics and privacy

~~~powershell
./apps/layer-windows/scripts/probe-displays.ps1
./apps/layer-windows/scripts/inspect-window.ps1 -ProcessId <app-process-id>
~~~

Window captures and runtime logs stay in ignored artifacts/windows. Review the
exact staged diff before publishing: no local paths, credentials, settings,
crash dumps, generated binaries, user documents, desktop captures, or raw
machine diagnostics belong in source control. Publish sanitized validation
summaries only.

The full acceptance plan is in ../../docs/windows-implementation.md.

## Controlled rendering smoke test

Set CAPY_TEST_DISPLAY=1 to select an active display running at 120 Hz or higher.
Set CAPY_SMOKE_TEST=1 to expose Test stroke and Test pan commands. These commands
replay records through the input dispatcher; they do not validate OS input delivery.
Use exercise-window.ps1 with -Action 'Test stroke', Undo, Redo, 'Test pan', Resize,
and Close, and inspect-window.ps1 to capture the app after each change. The ordinary
Stroke action uses OS SendInput and fails unless the pointer reaches this process.
Raw pointer tracing is separately opt-in through CAPY_TRACE_INPUT=1 and must be
disabled for timing runs. All generated diagnostics remain local.

Use inspect-window.ps1 -ClientOnly for Win32 client-area captures. Record the
XAML viewport separately: the observed client capture includes one extra physical
row compared with the SwapChainPanel extent. Native-frame accounting remains part
of the parity setup; do not rescale or silently crop reference captures to hide it.

The bridge uses the shared NativeHost and staged GPU preparation. Device/shader
work runs on the render worker; swap-chain attachment and reconfiguration run on
the UI thread while the worker is parked. The first paper frame does not consume
the engine's pending document replay. Painting readiness follows the shared
brush preparation state.

Run the relevant bridge tests with:
~~~powershell
cargo test --locked -p layer-host -p layer-windows --lib
~~~
