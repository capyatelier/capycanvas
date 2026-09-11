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

## Native workspace checkpoint

The shell now displays shared-layout tool tiles, brush previews, size presets,
and basic layer controls. Numeric editing uses Rust's expressions, units,
logarithmic mapping and stepping. Color and opacity open native flyouts.
Unchanged panel structures retain their controls across value updates; camera
patches update only the camera readout. Full and camera snapshots use separate
coalesced slots so camera motion cannot replace an unpublished workspace update.

~~~powershell
./apps/layer-windows/scripts/exercise-workspace.ps1 -ProcessId <app-process-id>
~~~

This test uses native UI Automation to check brush values, control retention,
layer creation and undo. It does not verify OS pointer delivery. Build-time asset
staging reuses the web app's SVG icons and brush previews under the ignored output
directory. No generated assets or captures need to be committed.

The current web app has also been built and captured in local hardware-backed
Chrome using tools/visual/chrome-capture.mjs. Comparison identified and corrected
uniform spacing/corner construction, numeric units and track styling, and UTF-8
source decoding. Detailed layer layout, docking/customization, durable settings,
remaining panels and full visual parity are unfinished. Native frame accounting
and presentation/input acceptance remain open.

## Input transport checks

Canvas histories are bounded and ordered; UI commands never block the UI thread.
A full command channel reports an explicit error and cancels active input.
Wheel and canvas shortcuts use shared Rust navigation/keymap policy. Native
widgets keep their own text, slider and focus-navigation keys.

Run allocation/order checks from a Visual Studio developer PowerShell:

~~~powershell
./apps/layer-windows/scripts/test-input.ps1
~~~

With CAPY_SMOKE_TEST=1, Test backlog replays 32,768 records in bounded batches.
CAPY_TRACE_TRANSPORT=1 logs capacity waits locally to input-transport.log.
Invoke Test backlog and then Close with exercise-window.ps1 to exercise shutdown
during producer backpressure. Both transport and raw input tracing must be off
for timing runs. Replay is not evidence of physical input delivery.

WinUI's independent input source reports terminal capture loss and routed release;
the adapter cancels shared contact state on those paths. Physical mouse/pen/touch,
keyboard, wheel and mixed-DPI continuity still require end-to-end validation.
See Microsoft's [InputPointerSource event ordering](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.input.inputpointersource?view=windows-app-sdk-1.8)
for the OS routing contract.

## Presentation probe

Build Release, then start the opt-in steady-content probe at normal user privilege:

~~~powershell
./apps/layer-windows/scripts/start-presentation-probe.ps1
~~~

The probe waits for shader readiness and then keeps presenting the same canvas
through DXGI, without a timer or per-frame disk logging. It writes local surface
identity/configuration to presentation-probe.json. This is a baseline for the
window/compositor path; it cannot pass sustained painting or input latency gates.

Install the official standalone [PresentMon 2.5.1 release](https://github.com/GameTechDev/PresentMon/releases/tag/v2.5.1)
under ~/.local/tools/presentmon/2.5.1, or pass its path explicitly. Capture the
process ID returned by the launch script:

~~~powershell
./apps/layer-windows/scripts/capture-presentation.ps1 -ProcessId <probe-process-id>
~~~

Windows requires ETW tracing rights for capture. If access is denied, run only
the capture script from Administrator PowerShell and leave the app at normal
privilege. The script does not elevate itself or change account/group membership.
It targets that process, disables input tracking, verifies the current display
runs at least 120 Hz, and matches the DXGI canvas identity instead of assuming
WinUI's other swap chains are the canvas. Keep the probe window fixed during
capture; reconfiguration invalidates the run.

Raw CSV, logs, display metadata and binary hashes stay under ignored
artifacts/windows/presentation. Analyze actual display intervals and dropped
frames separately from submission rate. No display-cadence conclusion is available
without a valid capture and workload review; this probe measures no mouse/pen latency.

## Native header and Preferences checks

Edit/View/Workspace menus bind to shared command state. The same GPU canvas
continues behind the header; native caption buttons and measured drag regions
remain above it. Preferences uses native controls with the shared settings model,
including image tiles, numeric policy, search, validation and shortcut editing.

For a controlled review instance, set CAPY_TRACE_UI=1 and an absolute disposable CAPY_SETTINGS_DIRECTORY before launch, then run:

~~~powershell
./apps/layer-windows/scripts/exercise-header-settings.ps1 -ProcessId <app-process-id> -StateFile <app-output-directory>/ui-state.json
~~~

The fixture checks shared acknowledgments and native control state. Run it only
against a disposable review instance; it edits settings and toggles fullscreen.
It does not verify physical keyboard or pointer delivery. CAPY_TEST_PRIMARY=1
with CAPY_TEST_DISPLAY=1 places review windows on the primary display, allowing
a separate 120 Hz probe to remain visible.

The opt-in ui-state.json contains app state and may include private settings.
It stays ignored alongside captures and traces, and must be off for performance
runs. Settings persistence, OS theme changes, remaining workspace features and
the full acceptance gates are still open.

Analyze a captured directory locally with:

~~~powershell
./apps/layer-windows/scripts/analyze-presentation.ps1 -Directory <local-capture-directory>
./apps/layer-windows/scripts/test-presentation-analysis.ps1
~~~

The analyzer keeps submission rate, displayed-frame rate, missing display
records and present-to-display latency distinct. It does not declare a 120 Hz
or input-latency acceptance pass. Its tests use synthetic data. See the latest
validation findings in ../../docs/windows-implementation.md for measured results
and unresolved integration checks.

## Private preferences storage

Settings live in `%LOCALAPPDATA%\CapyAtelier\CapyCanvas\settings.json`.
Windows uses the shared settings schema and migration rules. Writes run on a
dedicated worker and replace the previous file atomically after flushing. An
unreadable file is preserved as `settings.recovery.*.json` when a later change
is saved. Save failures appear in the window and Preferences; drawing continues.
A final save failure does not yet offer a Retry/Keep Open shutdown dialog.

`CAPY_SETTINGS_DIRECTORY` overrides the storage directory and must be absolute.
Use an owned, disposable directory under ignored artifacts for native UI tests.
The header settings fixture refuses a profile without this override.

The restart/failure fixture creates and owns its isolated profiles and review
processes. Pass a built app executable; it validates close-time drafts, restart,
locked-file recovery and preservation of unreadable files:

~~~powershell
./apps/layer-windows/scripts/exercise-settings-storage.ps1 -Executable ./artifacts/windows/Review/CapyCanvas.exe
~~~

These local profiles and reports must never be committed. Native text drafts use
the synchronous [TextChanging event](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.xaml.controls.textbox.textchanging?view=windows-app-sdk-1.8)
to keep formatting updates distinct from edits before close.
