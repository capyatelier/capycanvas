# Windows development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Windows client is a WinUI 3 app in C++/WinRT around the shared Rust editor.
Its Rust bridge, `layer-windows`, uses `NativeHost` and the shared wgpu renderer
on D3D12, presenting through a `SwapChainPanel`. The
[package README](../../apps/layer-windows/README.md) describes the source layout
and threads, and the [porting guide](../WINDOWS_PORTING_GUIDE.md) covers bringing
upstream features to Windows.

## Prerequisites

On Windows 11, install:

- [Visual Studio Build Tools](https://aka.ms/vs/18/stable/vs_buildtools.exe): select
  **Desktop development with C++**, the MSVC x64/x86 build tools (v143 or v145)
  and **Windows 11 SDK (10.0.26100.0)**. For ARM64 builds also select
  **MSVC C++ ARM64 build tools** (`Microsoft.VisualStudio.Component.VC.Tools.ARM64`).
  The full Visual Studio IDE and a separate
  Windows App SDK installation are unnecessary.
- [Rust stable](https://www.rust-lang.org/tools/install) with an MSVC host
  toolchain. An existing x64 Rust host can cross-compile the native ARM64 app;
  add its target with `rustup target add aarch64-pc-windows-msvc`. Add
  `x86_64-pc-windows-msvc` when building x64 from an ARM64 Rust host.

Unless `nuget.exe` is already on `PATH`, download the official CLI to the location
the build script searches:

```powershell
New-Item -ItemType Directory -Force "$env:USERPROFILE/.local/tools/nuget" | Out-Null
Invoke-WebRequest https://dist.nuget.org/win-x86-commandline/latest/nuget.exe `
  -UseBasicParsing -OutFile "$env:USERPROFILE/.local/tools/nuget/nuget.exe"
```

In-place native paint writes require the adapter extension and read/write
storage support for both RGBA and coverage float textures. Windows uses the
shared renderer capability check when requesting device features.

Painting needs a hardware D3D12 GPU. On Linux, [Windows VMs](windows-vm.md) build
the client and run tests and UI fixtures on the software adapter by default.
The optional [hardware GPU pool](windows-vm.md#hardware-gpu-pool) assigns a real
card for renderer tests and hardware UI journeys. [Devices](devices.md#windows)
lists the physical Windows hardware.

## Build and run

Open a new PowerShell terminal after installing Rust so it sees Cargo on `PATH`.
Windows PowerShell 5.1, included with Windows, is sufficient. From the repository
root, build and run:

```powershell
.\apps\layer-windows\run.cmd
```

The launcher builds optimized Release and opens the app after a successful build.
It selects the PC's native architecture, including from an emulated shell.
Pass `-Architecture arm64` or `-Architecture x64` to choose explicitly; the same
option applies to `build.ps1` and `test-without-gpu.ps1`.
Architecture arguments are case-insensitive and package names use lowercase.
It sets the script execution policy for its child PowerShell process, leaving
your settings intact. Use Release for everyday development, drawing and performance
checks. Reserve `-Configuration Debug` for stepping through code, inspecting
variables or running debug assertions; unoptimized painting can lag.

Pass `-RustProfile dev-perf` to `run.cmd` or `build.ps1` for optimized incremental
Rust compilation while developing. This uses the shared `dev-perf` Cargo profile
and separate `Release-dev-perf` output and native intermediates by default.
Its first build prepares another cache; measure edited builds after that cache
is warm. `-RustProfile release` uses the normal release profile, and Debug defaults
to `dev`. Compare frame measurements within the same profile; the development
profile does not establish release performance.

To build without launching, allow local scripts in this terminal and call the
build script directly:

```powershell
Set-ExecutionPolicy -Scope Process RemoteSigned -Force
./apps/layer-windows/scripts/build.ps1
```

The build script restores the NuGet packages pinned in
[`packages.config`](../../apps/layer-windows/packages.config) into ignored
`artifacts/windows/packages`, builds the Rust DLL and then the WinUI app into
`artifacts/windows/Release` for x64 or `artifacts/windows/arm64/Release` for ARM64,
and prints the path of `CapyCanvas.exe`. Rust builds use an explicit MSVC target;
MSBuild links and copies that target's DLL from Cargo's resolved target directory,
including a configured `CARGO_TARGET_DIR`.

The self-contained runtime uses Windows App SDK 2.5.1. Keep its component packages
and the project imports at the versions required by the SDK package. This includes
Microsoft's ScrollView hide-indicators timer lifetime fix and later popup and
shutdown fixes. After updating the runtime, run `exercise-selection.ps1` and
`exercise-color-editor.ps1` in both themes, retaining their close assertions.
To compare with an older runtime, build it into a new `-OutputDirectory`; an
existing output keeps the newer runtime DLLs.
See the [WinUI release notes](https://github.com/microsoft/microsoft-ui-xaml/releases).

For a Debug build, pass `-Configuration Debug`; `Debug` replaces `Release` in
the architecture's output path.
`-OutputDirectory` builds elsewhere and keeps native objects and precompiled headers
in `<OutputDirectory>.obj`. Pass `-IntermediateDirectory` to reuse an owned native
build cache across serial builds with separate outputs. Keep its architecture and
configuration the same, and let MSBuild track changed inputs.
`-PackagesDirectory` reuses another restore
and `-SkipRestore` skips it. `-ControlFixture` and `-SoftwareAdapterTests` build
the [control reviews](#control-reviews) and the [VM fixture build](windows-vm.md#ui-fixtures-on-the-software-adapter).
`-ReleaseIdentity` gives the build the installed app's [folders](#where-files-live);
only packaging uses it.

Always rebuild with `build.ps1` after Rust or shared changes. MSBuild alone copies
whatever Rust DLL and `capy-photo-reader.exe` already exist, so you would test
stale code. Photo import fails without the helper beside `CapyCanvas.exe`;
`photo_reader::tests` decodes through it and checks that its AppContainer cannot
read a file in the user's temp folder. Close the test
apps you started before rebuilding. Close every window of the current app before
switching configurations; a new launch otherwise activates the running build.

The build copies `dxcompiler.dll` from the pinned `Microsoft.Direct3D.DXC` package
for the selected architecture beside the executable. The host compiles D3D12 shaders with it; it uses its
internal validator, so no `dxil.dll` is needed. Without it the host falls back to
the system FXC compiler, which is several times slower. Set
`WGPU_DX12_COMPILER=fxc` to compare the two. The Windows App SDK runtime is also
copied beside the executable, so development builds run unpackaged.

DXC is pinned to 1.9.2609.5, whose ARM64 DLL loads in a native ARM64 process.
The earlier 1.9.2607.13 package's `arm64` DLL contains ARM64EC/x64 code and
cannot load there. Build and packaging checks reject incompatible PE machines.
Windows App SDK ARM64X companion DLLs are identified by their dynamic relocation
metadata and retained beside their matching x64 DLLs for emulation on Arm.

### Changing the host

- **Integers in native JSON.** Emit unsigned identifiers and nanosecond
  timestamps as integer tokens through `NativeJson.h`; the WinRT JSON number type
  stores doubles.
- **Workspace queries.** `QueryWorkspace` answers only the queries in the packet
  allowlist in `native/src/workspace.rs`. Admit each new shared query there and
  cover its result and error packets in the native tests.
- **Line endings.** Fluent catalogs and shared cursor SVGs check out with LF
  through `.gitattributes`; their bytes must match across hosts. Dependency
  notices in `tools/build/licenses/*.txt` keep their exact bytes.

Reuse these native helpers instead of writing another:

| Need | Helper |
| --- | --- |
| Button pressed and hover feedback | `ButtonFeedback.h`; dropdowns use `choiceColors` from `ChoiceFeedback.h` |
| Dialog background and footer buttons | `dialogSurface` |
| Text that follows language changes | `copyName` |
| Preview images (scopes, filters, layers, paper, gradients) | `BitmapUpload` |
| Immediate flyouts; menus filled by an asynchronous query | `TrackPopup` and `OwnedPopup` in `NativeMenus.h`; check the ticket before showing |
| Shortcuts on ancestors of native editors | `ShortcutKeys` in `UiControls.h` |
| Custom pointer gestures with rollback | `PointerGesture::OnCompleted`, `PrimaryContact` and `OnLeaving`; native Button and Slider use `PointerCaptureLoss::CompleteLift` |
| Scrub buttons | `PointerGesture::Clicks` with `Suppress(pointerId)` and `SuppressClick()` |

### Where files live

[App storage](../internals/storage.md) describes each kind of file. The Rust
bridge resolves every folder once at startup, in
[`storage.rs`](../../apps/layer-windows/native/src/storage.rs): a process with
package identity uses its package's `ApplicationData` folders, and every other
build uses Known Folders named for its build.

| Files | ZIP or installer | Development build | Microsoft Store |
| --- | --- | --- | --- |
| Preferences, export presets | `%APPDATA%\CapyAtelier\CapyCanvas` | `%APPDATA%\CapyAtelier\CapyCanvas-Dev` | `LocalState` |
| Workspaces and palettes, color profiles, editing sessions | `%LOCALAPPDATA%\CapyAtelier\CapyCanvas` | `%LOCALAPPDATA%\CapyAtelier\CapyCanvas-Dev` | `LocalState` |
| Cache | `%LOCALAPPDATA%\CapyAtelier\CapyCanvas\Cache` | `%LOCALAPPDATA%\CapyAtelier\CapyCanvas-Dev\Cache` | `LocalCache` |
| Parked drawing tabs, copies of opened `.capy` files, clipboard images | `%TEMP%\CapyCanvas` | `%TEMP%\CapyCanvas-Dev` | `TempState` |

The Store folders are under `%LOCALAPPDATA%\Packages\<package family name>`.
Only [`package.ps1`](#portable-zip) builds with the `release-identity` feature, so
a development build never opens an installed app's files or windows. The MSIX
holds that same build; with package identity it writes only to its package
folders, so MSIX file-system virtualization never redirects its files and cannot
split the workspace database from its `-wal` and `-shm` files. Uninstalling the
Store app deletes its folders; deleting the ZIP's folder or uninstalling the
installer build leaves them.
Nothing is written beside the executable; opt-in trace files go to the working
directory.

`CAPY_STORAGE_DIR` replaces all of these with `config`, `data`, `state`, `cache`
and `temp` in one absolute folder ([test storage](../internals/storage.md#test-storage)).
Unit tests that write app storage fail without it.

One process owns each installation's storage, as on the other platforms. Every
later launch, with or without files, hands itself to that process through a
message-only window named for its storage folders, then exits. Files open in the
frontmost window; a launch without files opens a new window. A launch waits up to
a minute for an instance that is still starting or closing. Builds with different
storage never hand launches to each other; `exercise-file-activation.ps1` checks
both cases. It also drags an image and a saved drawing from another window onto
the drawing tabs, which opens each as a new drawing.

### Environment switches

Remove a switch with `Remove-Item Env:NAME`. An empty variable still counts as
set, and passing `$null` to `[Environment]::SetEnvironmentVariable` can leave one.

| Variable | Effect |
| --- | --- |
| `CAPY_STORAGE_DIR` | Absolute folder replacing every [app folder](#where-files-live). Test and review runs use a fresh one under `artifacts/windows`; smoke-test hooks require it. |
| `CAPY_TRACE_UI=1` | Writes `ui-state-<pid>-<window>.json`, `camera-state-<pid>-<window>.json`, `panel-metrics-<pid>-<window>.json`, `windows-<pid>.json` and `lifecycle.log` in the working directory, and adds gesture state to UI Automation. These files can contain private settings. |
| `CAPY_SMOKE_TEST=1` | Adds test buttons: replayed stroke, pan, pen and backlog, filter reload and GPU device removal. Replay bypasses OS input delivery. |
| `CAPY_TEST_HDR=1` | With `CAPY_SMOKE_TEST`, adds buttons that simulate HDR and SDR output. |
| `CAPY_TEST_DISPLAY=1` | Opens the window on a display running at 120 Hz or more, or on the primary display with `CAPY_TEST_PRIMARY=1`. |
| `CAPY_LATENCY_TRACE=1` | Bounded input and presentation timing capture, and prediction frame counts at exit. |
| `CAPY_TRACE_SHADER_JOBS=1` | Logs pipeline compile durations to stderr. |
| `CAPY_FILTERS_DIR`, `CAPY_FILTERS_MODE` | [Runtime filter package](#runtime-filter-packages) override. |
| `LAYER_GPU_INDEX` | Adapter index for headless renderer tests. The default adapter can be Vulkan; select the D3D12 one explicitly. |

The panel-metrics trace contains the shared catalog's layout metrics. Fixtures
match its process and window IDs to the owned drawing before checking native
control bounds. A background worker publishes it atomically; readers wait for
the matching file before using its metrics.

Turn tracing off for performance runs.

Latency traces include the input-drain time and the prepare, compose, submit and
Present boundaries in QPC nanoseconds. `pacing_ns` and `acquisition_ns` are elapsed
durations for the drawing fence and DXGI acquisition. `submitted_ns` is sampled
after submission returns and before Present; it is an upper bound on the CPU
submission boundary, not GPU completion or display time.

`FramePacing::IN_FLIGHT` controls both GPU drawing depth and the DXGI maximum
frame latency. Pacing tracks the drawing submission returned by the presenter;
an additional submission after Present would also include unrelated queued work.

### Runtime filter packages

The filter catalog is embedded. `CAPY_FILTERS_DIR` loads an external package at
startup and `CAPY_FILTERS_MODE` chooses `add`, `replace` or `merge` (the default).
From the repository root, for a built app:

```powershell
$env:CAPY_FILTERS_DIR = (Resolve-Path examples/filters/tent-blur).Path
$env:CAPY_FILTERS_MODE = 'add'
& ./artifacts/windows/Release/CapyCanvas.exe
```

[Runtime filters](../reference/runtime-filters.md#windows-file-transport)
describes the loader and the `capy_load_filter_directory` reload API.

## Package

### Portable ZIP

Build a Windows 11 ZIP for the PC's native architecture from a committed
checkout, then check it on an unlocked desktop:

```powershell
./apps/layer-windows/scripts/package.ps1
./apps/layer-windows/scripts/exercise-package.ps1 -Archive <path-to-zip>
```

Pass `-Architecture arm64` or `-Architecture x64` to `package.ps1` to choose
explicitly. ZIP, installer and MSIX names include `windows-arm64` or
`windows-x64`. The converters take their architecture from the portable manifest;
an ARM64 installer refuses to install on a different PC architecture.

The packager runs a Release build with the release identity into a fresh
directory, collects the
self-contained Windows App SDK, the app-local Visual C++ runtime and all notices,
and records every payload file's size and SHA-256 in `package-manifest.json`. The packager
selects the target's runtime and DXC binaries and dependency notices, and rejects
EXE/DLL files whose PE architecture does not match the manifest. MSIX conversion
preserves that architecture in `ProcessorArchitecture`.
An x64 SDK payload can also contain an ARM64X `_ec.dll` companion for x64
emulation on Arm. Validation requires its matching x64 DLL and ARM64X relocation
metadata; plain ARM64 binaries and unpaired companions remain errors.
CRT collection selects compatible binaries from the redistributable directory,
which can also contain ARM64EC companions that native ARM64 cannot load.
Portable packaging and both converters reject the development-only WARP runtime.
It assembles the archive twice and requires identical hashes. Output stays under
ignored `artifacts/windows/distribution`. `-SkipRestore` reuses restored packages;
uncommitted changes need `-AllowDirty`, which marks the package as a development
build. `-SignArguments` passes its values to `signtool sign` for `CapyCanvas.exe`,
`layer_windows.dll` and `capy-photo-reader.exe` before the payload is inventoried, for example
`-SignArguments /sha1,<thumbprint>,/fd,SHA256,/tr,<timestamp URL>,/td,SHA256`
from a Visual Studio developer shell. The exercise extracts to a path with spaces and launches with an unrelated
working directory and an isolated profile.

Deployment follows Microsoft's
[self-contained Windows App SDK guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/self-contained-deploy/deploy-self-contained-apps)
and [Visual C++ redistribution guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files).

### Installer

Convert a portable build into a per-user setup program for direct downloads,
then install it, start the installed app with a `.capy` drawing, and uninstall it
on a private desktop:

```powershell
./apps/layer-windows/scripts/package-installer.ps1 -PortableResultFile <portable-result.json> -TestIdentity
./apps/layer-windows/scripts/test-desktop.ps1 -Script ./apps/layer-windows/scripts/test-installer.ps1 -Parameters @{ResultFile='<installer-result.json>'}
```

[`installer.nsi`](../../apps/layer-windows/scripts/installer.nsi) installs into
`%LOCALAPPDATA%\Programs\Capy Canvas` without administrator rights, adds a Start
menu shortcut and an Apps entry, and opens `.capy` drawings with the app. Setup
replaces an earlier version in place and asks the painter to close a running app
first; uninstalling removes the program, its shortcut and file type, and keeps
preferences, workspaces and editing sessions. The packager downloads the pinned
NSIS 3.11 release into `artifacts/windows/tools` and checks its SHA-256. The
installer uses NSIS's zlib/libpng-licensed stub and zlib compression, not the
LZMA module, which is under the Common Public License. It builds the setup
program twice and requires identical bytes, and versions it like the MSIX.
`-TestIdentity` gives the program, uninstall entry and file type a separate name
so a test never replaces an installed app; `test-installer.ps1` refuses any
other installer. The test identity preserves the default app for `.capy` files.
The check verifies the installed files, Apps entry, shortcut and registered file
type command, starts the test app directly, checks that setup refuses to replace
a running app and reinstalls a closed one, then uninstalls it. This same-version
replacement does not verify updating from the preceding release.
Opening the drawing itself needs a hardware GPU;
`exercise-file-activation.ps1` covers it. After the repeat-build check,
`-SignArguments` builds the setup program once more with NSIS signing its
uninstaller, then signs the setup program, with the same `signtool` values as the
ZIP.

### MSIX

Convert a portable build into an unsigned MSIX, using the portable `result.json`,
from an STA PowerShell session (Windows PowerShell 5.1 or PowerShell 7):

```powershell
./apps/layer-windows/scripts/package-msix.ps1 -PortableResultFile <portable-result.json>
./apps/layer-windows/scripts/test-msix.ps1 -ResultFile <msix-result.json>
```

The app runs as a `packagedClassicApp` at `mediumIL` with `runFullTrust` and
keeps its files in the package's [app data](#where-files-live); the converter
refuses a portable build without the release identity. The manifest declares the
languages in the shared shipping inventory (`SHIPPED_LANGUAGES`), and
`test-msix.ps1` checks that they and the indexed names match the shared catalogs.
Package and launcher display names use a
MakePRI index generated from `common-app-name` in the shared catalogs, following
[Microsoft's manifest localization workflow](https://learn.microsoft.com/en-us/windows/uwp/app-resources/using-mrt-for-converted-desktop-apps-and-games).
Output
stays under ignored `artifacts/windows/msix`. The package version is the
workspace version with a fourth part of 0, and the Store requires a nonzero major
part; `-AllowDirty` works as for the ZIP.
MakeAppx writes wall-clock ZIP timestamps, so `normalize-msix.ps1` rewrites only
their date and time fields and refuses signed packages. Never normalize after
signing. The default identity is the one Partner Center assigned the Store app:
`CapyAtelier.CapyCanvas`, publisher `CN=25C3FE75-9C78-42B1-91A5-2D7CD498E9E7`.
The Store signs uploads; to sign elsewhere, pass `-Publisher` to match the
certificate's subject.
The package must be
[signed before distribution](https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview).

For a local install test, `-UnsignedTestIdentity` appends `.Test` to the identity
and uses Microsoft's unsigned-test publisher. Installing it
[needs Administrator PowerShell](https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package):
`Add-AppxPackage -Path <test.msix> -AllowUnsigned`. Remove the test package
afterwards.

## Test

| Where | Checks |
| --- | --- |
| Linux or Windows | `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib` and `cargo clippy --locked -p layer-windows --all-targets -- -D warnings` |
| Windows, no GPU needed | `./apps/layer-windows/scripts/test-without-gpu.ps1`: native-architecture Release build, the windowless script regressions in `apps/layer-windows/tests`, native input queue tests, and serial Rust unit and `d3d12_` document tests on the Microsoft software adapter. Pass `-Architecture x64` or `-Architecture arm64` to choose explicitly, and `-Configuration Debug` for unoptimized builds and tests. On Linux: `tools/windows-vm/windows-vm.py check`. |
| Windows desktop | The `exercise-*.ps1` [UI fixtures](#ui-fixtures) for the changed area. On Linux: `tools/windows-vm/windows-vm.py fixtures <name>`. |
| Hardware D3D12 | `cargo test --locked -p layer-windows --lib d3d12_ -- --ignored --test-threads=1`, which adds the HDR and color round-trip tests; [GPU reconstruction](#gpu-reconstruction); renderer tests with `LAYER_GPU_INDEX`; performance. |

`scripts/test-input.ps1` compiles the C++ input queue and composition-key tests
with `cl` and needs a Visual Studio developer shell. Windows PowerShell 5.1 reads
scripts without an encoding marker in the system code page, so scripts with
Unicode text use a UTF-8 BOM; `tests/script-encoding.ps1` checks this. Sort
arrays of hashtables with an explicit key, such as `Sort-Object { $_.difference }`,
so Windows PowerShell 5.1 and PowerShell 7 agree.

The `d3d12_` tests that write app storage need `CAPY_STORAGE_DIR` set to a fresh
absolute folder. In Debug builds the HDR and native color tests need
`RUST_MIN_STACK=8388608`. Before direct Cargo GPU tests, put the build folder
first on `PATH` so D3D12 loads the app's pinned DXC. For native ARM64, use the
ARM64 build folder and explicit target, even from an x64 Rust host:

```powershell
$env:PATH=(Resolve-Path artifacts/windows/arm64/Release).Path+';'+$env:PATH
$env:CAPY_STORAGE_DIR=Join-Path (Get-Location) ('artifacts/windows/test-storage/'+[Guid]::NewGuid().ToString('N'))
cargo test --locked --release --target aarch64-pc-windows-msvc -p layer-windows --lib d3d12_ -- --ignored --test-threads=1
```

The same `--target` applies to shared tests, Clippy and renderer examples.
D3D12-specific fixtures construct an instance with `Backends::DX12`; a generic
`PRIMARY` headless instance can select Vulkan.

Pass `-OutputDirectory <dir>` to `test-without-gpu.ps1` to keep its application,
native intermediates and input-test binaries in your own build directory.

Software-adapter runs use WARP. The `software-adapter-tests` feature gives GPU
readbacks one shared 180-second budget, because WARP's first Levels Auto dispatch
takes about two minutes; hardware tests and performance runs keep it disabled.
Native x64 tests use the system WARP, and the test script refuses an app-local
WARP DLL beside Cargo test executables. For ARM64 the test script stages the
pinned `Microsoft.Direct3D.WARP` 1.0.21 from
[`tests/packages.config`](../../apps/layer-windows/tests/packages.config) beside
the test executables and removes it on exit. Its
[license](https://www.nuget.org/packages/Microsoft.Direct3D.WARP/1.0.21) permits
internal development and testing only: keep it out of application builds,
packages and public crash dumps.

### UI fixtures

Run fixtures under a Windows account with an available Desktop folder.
`test-desktop.ps1` checks the shell profile before launching any windows.

Each `apps/layer-windows/scripts/exercise-<area>.ps1` drives the real app
through UI Automation and guarded OS mouse, pen and touch input. Fixtures that
take `-Executable` launch their own app with a disposable profile and close it;
those that take `-ProcessId` attach to an app you launched with
`CAPY_TRACE_UI=1` and a disposable profile (see
[diagnosis](#diagnose-a-running-app)). A fixture's `param` block lists its
journeys, themes and devices. Results, captures and profiles stay under ignored
`artifacts/windows`.

Run fixtures on a private desktop with `test-desktop.ps1`, which restores the
original desktop and closes its own process tree. Raise `-TimeoutSeconds` (600 by
default) for long journeys such as documents and Liquify:

```powershell
foreach ($theme in 'dark','light') {
    ./apps/layer-windows/scripts/test-desktop.ps1 `
      -Script ./apps/layer-windows/scripts/exercise-activation.ps1 `
      -Parameters @{ Executable = (Resolve-Path ./artifacts/windows/Release/CapyCanvas.exe).Path; Device = 'pen'; Theme = $theme }
}
```

The [VM runner](windows-vm.md#ui-fixtures-on-the-software-adapter) discovers
the same scripts: `fixtures <area>` runs every variant of a fixture,
`<area>:default` only its default, and `<area>:<choice>` one switch or
`ValidateSet` choice, such as `tool-bubbles:light-pen` or `layers:Relationships`.
Run changed UI in both themes, and input changes with each device.

`exercise-edge-reach.ps1 -SweepOnly` samples edge reach bands, group gaps and
empty closed-strip space in Photo, Photo with both columns closed, and Paint.
Run it with each `-Device` and both `-Theme` values.

Run `gh workflow run windows-readiness.yml -f source_ref=<commit-sha>` for x64
WARP fixtures on hosted Windows VMs; its `fixtures` input takes a JSON array
such as `["canvas-bar:default","canvas-bar:light"]`. For a document crash,
select a narrow `documents` journey and set `capture_crashes=true` to record CDB
stacks. Run
`gh workflow run release-checks.yml -f windows-only=true -f source_ref=<commit-sha>`
for the x64 and ARM64 release build, test and Clippy jobs. Neither workflow
qualifies hardware rendering or performance.

Focused tab-drop journeys are `tab-drag:release-dark`, `tab-drag:release-light`,
`hdr:tab-drop-dark` and `hdr:tab-drop-light`. They cover panel and drawing tabs
with mouse, pen and touch, including native control identity, arranged release
positions and order history. Run all `tab-pickup` variants after input changes.

Fixtures share the helpers in `CapyUia.ps1`, such as `Owned-DrawingWindow`,
`Control -Arranged`, `Workspace-Root`, `Tool-Tile`, `Use-PaintWorkspace`,
`Focus-Keyboard`, `New-DrawingPixels`, `Wait-StablePixels` and `Read-Snapshot`,
and the native input drivers `RowPointerDriver.cs` and `CanvasTouchDriver.cs`.
Follow these rules when writing one:

- **Unlocked desktop, one at a time.** Fixtures need an unlocked interactive
  desktop and foreground input. Run them sequentially.
- **Resolve the drawing window.** Use `Owned-DrawingWindow` at startup and after
  reopening, and keep its root and HWND for input, sizing, captures and closing.
  `Process.MainWindowHandle` can select an input-indicator overlay in the same
  process. Pass `-StateDirectory` when the app's working directory differs from
  its executable directory.
- **Wait for layout.** Before injecting input, wait for the published state and
  the arranged control bounds, then check that the hit belongs to the owned app.
  A model update alone does not mean the visible rows have moved. Reveal panel
  rows within the published `panel-body-scroll-N` bounds: a scroll provider's
  bounds can include its whole content extent. Convert DIP sizes with the owned
  window's DPI before resizing.
- **Use real keyboard focus.** Arrange keyboard targets, then use `Focus-Keyboard`,
  which uses native Tab traversal; UI Automation's SetFocus does not establish
  keyboard modality.
- **Measure timed input.** Use the native test drivers for double presses,
  holds and prediction strokes and record their actual timing; PowerShell sleeps
  can exceed a gesture's interval under load.
- **Wait for the operation you started.** Compare the document revision or the
  requested state before and after input. An already enabled Undo does not prove
  a new stroke committed, and `WaitForInputIdle` does not acknowledge an operation.
  After zooming, steady-motion checks also wait for the committed
  `windows_recovery.camera.zoom` and its applied UI revision; a later session
  checkpoint can publish another full snapshot without changing workspace content.
- **Prepare the selected tool.** Invoking an already selected grouped tool opens
  its drawer, and the first outside contact dismisses an open drawer.
- **Capture composed pixels.** GPU pixel assertions use the owned window's screen
  rectangle on the private desktop; `PrintWindow` can return an earlier
  SwapChainPanel image. Before `Wait-StablePixels` samples, the `presentation-state`
  acknowledgement must show the required present as displayed, because the
  desktop compositor can show frames more than a second late. Compare restoration
  with the original pixels, not a theme-specific threshold, and move the pointer
  and tooltips out of the capture first.
- **Use current identifiers.** Menu items use the shared command ID, other layer
  actions `layer-menu-<op>`. Resolve a toolbar tile once and keep its ID:
  `resolved_control` changes when its variant does.
- **One snapshot per assertion.** Read related fields from one state file and
  check its `process_id`, `window_id` and freshness. Use `Workspace-Root` for
  drag state; only the inner workspace publishes it.
- **Keep callback data distinct.** `Wait-Until` callbacks run in its scope;
  expected values must not share names with its parameters.
- **Own the test processes.** Close a fixture's app before rebuilding. The
  private-desktop and VM runners preserve evidence and close what they own.
- **Injection is not hardware.** UI Automation and injected input don't establish
  physical pen and touch behavior, cadence or latency. Compare an injected-input
  failure with physical input before changing native capture.

### Tool bubbles

`exercise-tool-bubbles.ps1` follows the GTK bubble journeys with native mouse,
pen and touch contacts. Run its six `<theme>-<device>` variants, such as
`tool-bubbles:light-pen`, through the VM or private-desktop runner. Its switches
select focused scopes; use `-HdrOnly` with `-HdrDepth F16` and `F32` after
changing HDR color bubbles, and `-BlendersOnly` after changing blender bubbles.
The `tool-bubbles:native-controls-dark` and `tool-bubbles:native-controls-light`
variants cover native card controls, scrolling, touch takeover and inertia.
Use `tool-bubbles:inertia-dark` and `tool-bubbles:inertia-light` to isolate live
scroll interruption and the next pan.

### GPU reconstruction

These remove the process's real D3D12 device. Run each alone and serially, on
hardware, separately from performance measurements:

```powershell
./apps/layer-windows/scripts/exercise-documents.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe -RecoverGpu
./apps/layer-windows/scripts/exercise-documents.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe -FailGpu
./apps/layer-windows/scripts/exercise-lifecycle.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe -RecoverGpu
./apps/layer-windows/scripts/exercise-multiwindow.ps1 -Executable ./artifacts/windows/Release/CapyCanvas.exe -RecoverGpu
cargo test --locked -p layer-windows --lib device::tests::validation_error_releases_pipeline_and_allows_device_replacement -- --ignored --exact --nocapture
cargo test --locked -p layer-windows --lib documents::recovery_tests -- --ignored --test-threads=1 --nocapture
cargo test --locked -p layer-windows --lib filter_packages::tests::recovery_tests -- --ignored --test-threads=1 --nocapture
```

`-RecoverGpu` removes the device during pen strokes and checks exported pixels,
history and thumbnails after reconstruction. `-FailGpu` also blocks
reconstruction and checks that completed edits remain saveable. The UI test hook
drains rendering and presentation before `ID3D12Device5::RemoveDevice`, because
WARP can crash when removal races queued presentation; the ignored Rust tests
above remove the device with work in flight. Surface replacement keeps active
contacts so the stroke can be reconstructed.

### Control reviews

`build.ps1 -ControlFixture Number|Color|Bitmap|Popup|Feedback|LayerDrop` replaces the app's
entry point with production controls for review. Number and Color reviews
compare with the Web controls through shared Rust models; generate the models at
the display's actual scale, and regenerate native and Web inputs together after
changing their contract:

```powershell
cargo run --locked -p layer-windows --example number-fixture -- artifacts/windows/numeric-matrix
./apps/layer-windows/scripts/build.ps1 -ControlFixture Number -OutputDirectory artifacts/windows/NumberReview
./apps/layer-windows/scripts/capture-number-controls.ps1 -Executable artifacts/windows/NumberReview/CapyCanvas.exe -FixtureFile artifacts/windows/numeric-matrix/fixture-windows-light.json

cargo run --locked -p layer-host --example compact_color_fixture -- artifacts/windows/compact-color-parity 1.5
./apps/layer-windows/scripts/build.ps1 -ControlFixture Color -OutputDirectory artifacts/windows/ColorFixture
./apps/layer-windows/scripts/capture-compact-color.ps1 -Executable artifacts/windows/ColorFixture/CapyCanvas.exe -FixtureFile artifacts/windows/compact-color-parity/dark-160.json
```

Pass the native reports to the `number-controls` and `color-panel` scenarios of
[the visual tools](../../tools/visual/README.md). The Feedback build runs
`exercise-button-feedback.ps1`, `exercise-sdk-feedback.ps1`,
`exercise-scroll-indicators.ps1` and `exercise-navigator-allocation.ps1` through
`test-desktop.ps1`. The Bitmap build reads its input from an absolute
`CAPY_BITMAP_FIXTURE` path and writes `bitmap-upload-fixture-result.json` beside
it. Rebuild normally before using the editor again.

The LayerDrop build mounts the production layer panel and holds its final drop
reply until the fixture releases it. Build it in a separate output folder and
pass its executable as `-DropFixture` to `exercise-layer-pickup.ps1` for the full
journey. `-PendingDropOnly` runs the overlap check directly with the LayerDrop
executable. Run both through `test-desktop.ps1`.

`-PresentationsOnly` in the layer pickup fixture, `-ToolbarRangeOnly` in
selection, `-ArcContextOnly` in HDR and `-PixelsOnly` in Navigator run those
portions of their normal journeys.
`-ColumnEarlyOnly` in workspace pickup tests early motion along a collapsed strip.
`-SwipeCleanupOnly` in switcher drag runs its native swipe Delete cleanup.

## Matched editor captures

Compare the editor with the Web app at the same document, workspace, theme,
effective accent, viewport and scale. Build the Web reference with
`bash apps/layer-web/build.sh`
(from Git Bash, after the [Web prerequisites](web.md#prerequisites)). Install
Node.js and the Python packages in
[`tools/visual/requirements.txt`](../../tools/visual/requirements.txt), and set
`CAPY_CHROME` to Chrome's executable. Chrome must use hardware WebGPU.
The reference capture allows 60 seconds for staged GPU startup; layout and
control acknowledgements retain their separate 25-second limits.

Launch an owned review with a disposable profile, select Sketch, Paint or Photo,
then capture both themes with fitted and zoomed paper:

```powershell
$exe = (Resolve-Path ./artifacts/windows/Release/CapyCanvas.exe).Path
$run = Join-Path (Get-Location) ('artifacts/windows/review/' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run -Force | Out-Null
$env:CAPY_STORAGE_DIR = Join-Path $run 'profile'
$env:CAPY_TRACE_UI = '1'
Remove-Item Env:CAPY_SMOKE_TEST,Env:CAPY_TEST_DISPLAY,Env:CAPY_TEST_PRIMARY -ErrorAction SilentlyContinue
$review = Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe) -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
$null = $review.Handle
./apps/layer-windows/scripts/capture-editor.ps1 -ProcessId $review.Id -OutputDirectory $run
$first = (Get-Content -LiteralPath (Join-Path $run 'fixtures.json') -Raw | ConvertFrom-Json).fixtures[0]
node tools/visual/chrome-capture.mjs $first.viewport[0] $first.viewport[1] $first.scale $run light windows-editor (Join-Path $run 'fixtures.json')
python tools/visual/compare.py (Join-Path $run 'web-light-initial.png') (Join-Path $run 'native-light-initial.png') --output (Join-Path $run 'diff-light-initial')
```

`capture-editor.ps1 -Width` (default 960 logical pixels) covers the compact and
expanded headers; repeat at 744 and 1200. It refuses smoke-test controls, measures
the actual display scale and saves the raw client image together with the
measured XAML surface. Windows keeps an OS frame above the XAML content; the
manifest records the offset and effective palette. The Web collector supplies
the native effective accent through the shared `system_theme_changed` action
and asserts that the resulting accent matches. Preserve earlier captures when
correcting unmatched inputs.

Compare every image pair and look at the screenshots as well as the geometry
reports. Use [visual acceptance](../../tools/visual/README.md#comparing-images): fix
visible differences and record intentional native styling. Preserve raw captures
and exact comparison failures; never rescale, crop, mask or loosen tolerances to
make a comparator pass. Perceptual acceptance does not mean exact pixel equality.

## Diagnose a running app

Launch an owned review as in [matched editor captures](#matched-editor-captures).
After a native file picker, pass the drawing HWND from `windows-<pid>.json` to
helpers that accept `-WindowHandle`. `Process.MainWindowHandle` can select an
input-service window in the same process. Retain the verified drawing HWND for
foreground input and closing.
Then:

- `./apps/layer-windows/scripts/inspect-window.ps1 -ProcessId $review.Id` saves a
  window capture and lists UI Automation names; `-ClientOnly` captures the client
  area. It resolves the drawing window for PID-only calls; pass `-WindowHandle`
  when the process has multiple drawing windows. `probe-displays.ps1` reports each display's active mode.
- Read stderr and the `CAPY_TRACE_UI` files. Per-window JSON files are replaced
  atomically; a leftover `.pending` file means a failed replacement even when the
  app advanced. The `Drawing workspace` element's UIA HelpText records the pointer
  device, capture state and last cancellation; its ItemStatus reports layout
  publication and the workspace revision.
- A timeout does not prove the app exited. Close only your own review with
  `./apps/layer-windows/scripts/exercise-window.ps1 -ProcessId $review.Id -Action Close`,
  which checks for a clean exit. Add `-DiscardUnsaved` only for a disposable review.

For a native crash, reproduce with a Debug build under Visual Studio, WinDbg or
CDB attached to the review's process, with the PDBs from that build and
Microsoft's public symbols:

```powershell
cdb -p $review.Id -logo (Join-Path $run 'debugger.log')
```

At the prompt, run `.symfix` with an absolute cache directory under
`artifacts/windows`, `.sympath+` with the executable directory, then `sxe av` and
`g`. At the fault, record the exception code and the `kv` stack; in a dump,
select the exception context with `.ecxr` first. See Microsoft's
[exception controls](https://learn.microsoft.com/en-us/windows-hardware/drivers/debuggercmds/sx--sxd--sxe--sxi--sxn--sxr--sx---set-exceptions-)
and [CDB symbol setup](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/setting-symbol-and-source-paths-in-cdb).

Keep profiles, databases, captures, traces, debugger logs and dumps under ignored
`artifacts/windows`: they can contain document contents and paths. Measure
performance without a debugger, tracing or competing builds and GPU work; the
[measuring guide](../performance/measuring.md) has the Windows workloads.
