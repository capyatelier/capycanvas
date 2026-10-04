# Windows development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Windows client is a WinUI 3 app in C++/WinRT around the shared Rust editor.
Its Rust bridge, `layer-windows`, uses `NativeHost` and the shared wgpu renderer
on D3D12, presenting through a `SwapChainPanel`. The
[package README](../../apps/layer-windows/README.md) describes the source layout
and threads, and the [porting guide](../WINDOWS_PORTING_GUIDE.md) covers bringing
upstream features to Windows.

## Prerequisites

Install Rust stable for `x86_64-pc-windows-msvc`, Visual Studio C++ Build Tools
with v143 or v145, Windows SDK 10.0.26100.0, PowerShell 7 and the official NuGet
CLI, on `PATH` or at `~/.local/tools/nuget/nuget.exe`.

Painting needs a hardware D3D12 GPU. On Linux, [Windows VMs](windows-vm.md) build
the client and run the GPU-free tests and the UI fixtures on the software
adapter; they do not replace a hardware GPU. [Devices](devices.md#windows) lists
the Windows hardware.

## Build and run

From PowerShell at the repository root:

```powershell
./apps/layer-windows/scripts/build.ps1
./apps/layer-windows/scripts/build.ps1 -Configuration Release
```

The script restores the NuGet packages pinned in
[`packages.config`](../../apps/layer-windows/packages.config) into ignored
`artifacts/windows/packages`, builds the Rust DLL and then the WinUI app into
`artifacts/windows/Debug` or `Release`, and prints the path of `CapyCanvas.exe`.
`-OutputDirectory` builds elsewhere, `-PackagesDirectory` reuses another restore
and `-SkipRestore` skips it. `-ControlFixture` and `-SoftwareAdapterTests` build
the [control reviews](#control-reviews) and the [VM fixture build](windows-vm.md#ui-fixtures-on-the-software-adapter).
`-ReleaseIdentity` gives the build the installed app's [folders](#where-files-live);
only packaging uses it.

Always rebuild with `build.ps1` after Rust or shared changes. MSBuild alone copies
whatever Rust DLL already exists, so you would test stale code. Close the test
apps you started before rebuilding.

The build copies `dxcompiler.dll` from the pinned `Microsoft.Direct3D.DXC` package
beside the executable. The host compiles D3D12 shaders with it; it uses its
internal validator, so no `dxil.dll` is needed. Without it the host falls back to
the system FXC compiler, which is several times slower. Set
`WGPU_DX12_COMPILER=fxc` to compare the two. The Windows App SDK runtime is also
copied beside the executable, so development builds run unpackaged.

Ordinary restarts reopen saved and untitled drawings automatically, retaining tab
order, the active drawing, camera, selection and bounded Undo/Redo. Closing the
window flushes the session asynchronously and destroys its GPU and storage owners
on a worker after detaching the surface; explicitly closing a drawing still
uses Save/Discard/Cancel and removes its durable membership before cleanup.
Abnormal restores mark drawings as recovered until their next save.

Session workers reuse immutable resources and coalesce changes over two seconds.
Unchanged drawings produce no periodic writes. Atomic publication preserves the
previous complete drawing checkpoint when storage fails, and live windows hold
exclusive leases. Startup admits the complete session before preparing renderers;
inactive drawings use one scratch renderer at a time. The active canvas appears
after this bounded preparation of all tabs. Unfinished restore attempts remain on disk and require an
explicit retry; Keep Open starts a separate session while retaining those copies.
A restored destination is checked before saving so external changes cannot be
silently overwritten. Use Save As when the original drawing changed.
File workers use `color_storage::export_profile` for imported profiles in export
options, export presets and print-proof setup.

Packages with unsupported or damaged authored content open in a separate read-only
view. The current drawing remains open. The view presents the shared package
status, available output names and bounded preview, with Copy Original, Export
Preview and Close actions. Copy Original writes the retained package bytes on the
document worker. Export Preview appears when the package has a verified preview
and writes those exact PNG bytes to a new destination on the document worker.
Unsupported portable packages remain in that view. Private session failures
retain their snapshots on disk and present Retry or Keep Open.

Language changes prepare shared copy on the profile worker, then update retained
controls in every window using that private profile. Publication waits for native
text composition keys, canvas contacts and shared gestures to retire. System
resamples the OS preferred languages on the profile worker; launch tags are the
fallback when the OS query is unavailable. Text drafts and selections,
document history, HWNDs and swap chains stay with their existing owners. The
`localization` snapshot envelope carries one generation of catalog, document
delivery and bootstrap copy; stateless native helpers borrow that same immutable
context. Controls that outlive a language change, such as the canvas bar, bind
their names with `copyName`; localized tab titles update retained panel headers.
A button made from localized copy relabels its text and accessible name
but keeps an icon or panel given as its content, because copy callbacks also run
when the window prunes them.

Empty localization metadata preserves the current context; a pending envelope
stays queued until a full snapshot carries it. Properties, tool controls, filter
results and options use semantic identities for their structural keys. Current
captions update retained controls, accessibility names and tooltips. Numeric
refusals retain their shared reason, and color forms relabel their rows and
re-read a refused value in the new language without changing the draft. Profile,
proof and export captions preserve raw names, selections and prepared candidates.
Preferences reserves the measured titlebar height when fitting its centered
dialog; its narrow-window check dismisses it with an actual pointer click.

`exercise-localization.ps1 -Executable <path> -Theme dark` checks every shipped
language, retained numeric and Unicode drafts and selections, Properties,
shortcuts, filters, color forms, profile/proof/export controls, workspace handles,
and inactive and future windows. It checks drawing, pixel restoration through
undo/redo and Unicode save/reopen/export. Before recording curve history or drawing
pixels, fixtures require Undo availability in the current shared snapshot.
This checks the published command state; queued native input can still retire
after that snapshot. Repeat with `-Theme light` and
`-LargeText`, which sets the private VM user's system text scale to 150 percent
and restores it afterwards. The VM runner selects all four variants with
`localization`; suffixes `:default`, `:light`, `:LargeText` and `:light-large`
select a single variant. Genuine TSF candidate handling needs an interactive Windows
desktop with an enabled IME; physical GPU presentation needs Windows hardware.
`localization-smoke` checks English and Japanese, including compact menus and
narrow Preferences, in both themes. Menu selectors use semantic IDs or the exact
current caption from the shared snapshot.

Activating Preferences deliberately sends shared Blur, restoring the tool used
before temporary eyedropping. The sampler journey checks that restoration, then
reenters the picker and checks current captions and unchanged sample choices.
A windowless shared test checks pure language publication while picking stays
active; the native fixture does not claim retained sampler identity across Blur.

The Layers panel presents shared attachment actions, owner visibility and group mode.
Decorative clipping rails and effect links use realized thumbnail geometry and remain
clipped to the scrolling viewport. A content-thumbnail drop attaches to that layer's
output; a row drop uses shared insertion policy. Native drag feedback uses the shared
normalized destination and release revalidates the original hit. Right swipes dispatch
the shared row action. `exercise-layers.ps1 -Relationships` checks these interactions
and their undo history in both themes, alongside the existing pickup fixture variants.
The default layers fixture closes with a focused opacity draft, reopens the same
private profile, and verifies the saved draft and its clean Undo checkpoint.

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
both cases.

### Environment switches

Remove a switch with `Remove-Item Env:NAME`. An empty variable still counts as
set, and passing `$null` to `[Environment]::SetEnvironmentVariable` can leave one.

| Variable | Effect |
| --- | --- |
| `CAPY_STORAGE_DIR` | Absolute folder replacing every [app folder](#where-files-live). Test and review runs use a fresh one under `artifacts/windows`; smoke-test hooks require it. |
| `CAPY_TRACE_UI=1` | Writes `ui-state-<pid>-<window>.json`, `camera-state-<pid>-<window>.json`, `windows-<pid>.json` and `lifecycle.log` in the working directory, and adds gesture state to UI Automation. These files can contain private settings. |
| `CAPY_SMOKE_TEST=1` | Adds test buttons: replayed stroke, pan, pen and backlog, filter reload and GPU device removal. Replay bypasses OS input delivery. |
| `CAPY_TEST_HDR=1` | With `CAPY_SMOKE_TEST`, adds buttons that simulate HDR and SDR output. |
| `CAPY_TEST_DISPLAY=1` | Opens the window on a display running at 120 Hz or more, or on the primary display with `CAPY_TEST_PRIMARY=1`. |
| `CAPY_LATENCY_TRACE=1` | Bounded input and presentation timing capture, and prediction frame counts at exit. |
| `CAPY_TRACE_SHADER_JOBS=1` | Logs pipeline compile durations to stderr. |
| `CAPY_FILTERS_DIR`, `CAPY_FILTERS_MODE` | [Runtime filter package](#runtime-filter-packages) override. |
| `LAYER_GPU_INDEX` | Adapter index for headless renderer tests. The default adapter can be Vulkan; select the D3D12 one explicitly. |

Turn tracing off for performance runs.

### Runtime filter packages

The filter catalog is embedded. `CAPY_FILTERS_DIR` loads an external package at
startup and `CAPY_FILTERS_MODE` chooses `add`, `replace` or `merge` (the default).
From the repository root, for a built app:

```powershell
$env:CAPY_FILTERS_DIR = (Resolve-Path examples/filters/tent-blur).Path
$env:CAPY_FILTERS_MODE = 'add'
& ./artifacts/windows/Debug/CapyCanvas.exe
```

[Runtime filters](../reference/runtime-filters.md#windows-file-transport)
describes the loader and the `capy_load_filter_directory` reload API.

## Package

### Portable ZIP

Build a Windows 11 x64 ZIP from a committed checkout, then check it on an
unlocked desktop:

```powershell
./apps/layer-windows/scripts/package.ps1
./apps/layer-windows/scripts/exercise-package.ps1 -Archive <path-to-zip>
```

The packager runs a Release build with the release identity into a fresh
directory, collects the
self-contained Windows App SDK, the app-local Visual C++ runtime and all notices,
and records every payload file's size and SHA-256 in `package-manifest.json`. It
assembles the archive twice and requires identical hashes. Output stays under
ignored `artifacts/windows/distribution`. `-SkipRestore` reuses restored packages;
uncommitted changes need `-AllowDirty`, which marks the package as a development
build. `-SignArguments` passes its values to `signtool sign` for `CapyCanvas.exe`
and `layer_windows.dll` before the payload is inventoried, for example
`-SignArguments /sha1,<thumbprint>,/fd,SHA256,/tr,<timestamp URL>,/td,SHA256`
from a Visual Studio developer shell. The exercise extracts to a path with spaces and launches with an unrelated
working directory and an isolated profile.

Deployment follows Microsoft's
[self-contained Windows App SDK guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/self-contained-deploy/deploy-self-contained-apps)
and [Visual C++ redistribution guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files).

### Installer

Convert a portable build into a per-user setup program for direct downloads,
then install it, check that a `.capy` drawing starts the installed app, and
uninstall it on an unlocked desktop:

```powershell
./apps/layer-windows/scripts/package-installer.ps1 -PortableResultFile <portable-result.json> -TestIdentity
./apps/layer-windows/scripts/test-installer.ps1 -ResultFile <installer-result.json>
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
other installer. It checks the installed files against the manifest, the Apps
entry, shortcut and `.capy` association, that setup refuses to replace a running
app and upgrades a closed one in place, and that uninstalling leaves nothing
behind. Opening the drawing itself needs a hardware GPU;
`exercise-file-activation.ps1` covers it. `-SignArguments` signs the finished
setup program, after the repeat-build check, with the same `signtool` values as
the ZIP.

### MSIX

Convert a portable build into an unsigned MSIX, using the portable `result.json`,
from an STA PowerShell session (Windows PowerShell 5.1 or PowerShell 7):

```powershell
./apps/layer-windows/scripts/package-msix.ps1 -PortableResultFile <portable-result.json>
./apps/layer-windows/scripts/test-msix.ps1 -ResultFile <msix-result.json>
```

The app runs as a `packagedClassicApp` at `mediumIL` with `runFullTrust` and
keeps its files in the package's [app data](#where-files-live); the converter
refuses a portable build without the release identity. Output
stays under ignored `artifacts/windows/msix`. The package version is the
workspace version with a fourth part of 0, and the Store requires a nonzero major
part; `-AllowDirty` works as for the ZIP.
MakeAppx writes wall-clock ZIP timestamps, so `normalize-msix.ps1` rewrites only
their date and time fields and refuses signed packages. Never normalize after
signing. The default identity is `CapyAtelier.CapyCanvas`, publisher
`CN=Capy Atelier`; pass `-Publisher` to match the signing certificate's subject.
The package must be
[signed before distribution](https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview).

For a local install test, `-UnsignedTestIdentity` appends `.Test` to the identity
and uses Microsoft's unsigned-test publisher. Installing it
[needs Administrator PowerShell](https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package):
`Add-AppxPackage -Path <test.msix> -AllowUnsigned`. Remove the test package
afterwards.

Signing, installed update and uninstall, and installs on a clean machine have not
been verified.

## Test

`exercise-artwork-recovery.ps1 -Executable <path> -Theme dark` checks automatic
crash and orderly restarts, tab membership, the active drawing, Undo/Redo,
cancelled and discarded drawing closes, missing or changed saved originals,
and idle write coalescing. Repeat with
`-Theme light`; the VM fixture runner selects both variants.

Drawer and expansion query lifecycle tests live in `layer-host`. Windows
workspace tests cover native snapshot insets and the JSON/CPU packet boundary.

| Where | Checks |
| --- | --- |
| Linux or Windows | `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib` and `cargo clippy --locked -p layer-windows --all-targets -- -D warnings` |
| Windows, no GPU needed | `./apps/layer-windows/scripts/test-without-gpu.ps1`: build, native input queue tests, Rust unit tests and the `d3d12_` document tests. On Linux: `tools/windows-vm/windows-vm.py check`. |
| Windows desktop | The `exercise-*.ps1` fixtures for the changed area. On Linux: `tools/windows-vm/windows-vm.py fixtures <name>`. |
| Hardware D3D12 | `cargo test --locked -p layer-windows --lib d3d12_ -- --ignored --test-threads=1`, which adds the HDR and color round-trip tests; [GPU reconstruction](#gpu-reconstruction); renderer tests with `LAYER_GPU_INDEX`; performance. |

`scripts/test-input.ps1` compiles the C++ input queue and composition-key tests
with `cl` and needs a Visual Studio developer shell. `TextInput.cpp` observes the
focused TextBox's public TSF edit notifications so overlapping syllables and a
composition ending before routed KeyDown keep the same native key owner. The
key tests include nested message timestamps, release before the confirming key,
unrelated releases and clock wraparound. Real
IME checks must distinguish candidate Enter/Escape from the next ordinary press,
and include selected names, numeric refusal, pointer confirmation and focus
changes. The `d3d12_` tests that write app storage need `CAPY_STORAGE_DIR` set
to a fresh absolute folder. In Debug builds the HDR test needs
`RUST_MIN_STACK=8388608`, the native document worker's stack size.

Grouped toolbar and header tools retain their shared slot identity while presenting
`resolved_control`, label, icon and selection from the current snapshot. Secondary
click, the keyboard context key, and touch or pen holds open the tile or header
context menu, which starts with the shared variations. Title-bar overflow rows are
buttons that keep the group marker and open the same menu after closing the list.
Tool picker identifiers include the slot name, or the size in tenths of a pixel,
so independent choices remain independently selectable. Fixtures find tools with
`Tool-Tile` in `CapyUia.ps1`, which matches a tile's command or its slot's
current variation.

`exercise-tool-variations.ps1 -Executable <path> -Theme dark` walks grouped Paint
and Photo selection tools, the Eraser category, and a drawing group added to the
header through customization, then narrows the window until that group overflows,
with injected mouse, pen and touch. Repeat with `-Theme light`.
The VM fixture names are `tool-variations` and `tool-variations:light`.

Properties, Tool Settings and Tool Options share one gradient editor
(`GradientView.cpp`); a worker thread rasterizes its dithered preview through the
shared `color_ui` gradient image request. `exercise-effects.ps1` drags Gradient Map
stops with mouse, touch and pen and checks Escape, arrow keys, Delete,
interpolation, Reverse and the current-color bucket, each as one Undo step.
`exercise-tools.ps1` and `exercise-toolbar-components.ps1` cover the Gradient
tool's editor in Tool Settings and the Tool Options popup.

Canvas cursor shapes come from shared Rust and the shared GPU presenter. Tool
uses the active tool's icon, aligned to its working point; Tool and brush size
adds the current brush outline for brush tools. Windows presents the shared
cursor choices directly in Pen & Input preferences.
`exercise-cursors.ps1 -Executable <path> -Theme dark` selects both choices,
checks persisted settings, and compares composed mouse and pen hover pixels for
six tools without changing the drawing. Repeat with `-Theme light`, or use
`tools/windows-vm/windows-vm.py fixtures cursors` for both themes on WARP.

`exercise-ime.ps1 -Executable <path> -Theme dark` exercises the installed
Microsoft Japanese IME on the interactive test desktop. Enable Japanese input
in the private test user first; the fixture requires actual Hiragana preedit
and a converted candidate, with 100 ms between key presses, before checking
Enter, Escape, selected layer names,
numeric refusal, pointer confirmation and focus changes. Repeat with `-Theme
light`, or select `fixtures ime` in the VM runner. Captures include the private
desktop so the IME candidate window remains visible in the evidence.

### UI fixtures

Each `apps/layer-windows/scripts/exercise-*.ps1` drives the real app through UI
Automation and guarded OS mouse, pen and touch input. Fixtures that take
`-Executable` launch their own app with a disposable profile and close it; those
that take `-ProcessId` attach to an app you launched with `CAPY_TRACE_UI=1` and a
disposable profile (see [diagnosis](#diagnose-a-running-app)). Device-specific
fixtures take `-Device mouse|pen|touch`. Results, captures and profiles stay under
ignored `artifacts/windows`.

For switcher visibility and its context menu during title-bar customization, run
`tools/windows-vm/windows-vm.py fixtures switcher header:options header:options-light header:options-pen header:options-touch`.
The switcher fixture covers both themes and injected mouse, pen and touch input;
the focused header journeys also check placement cancellation and keyboard input.

- **Unlocked desktop, one at a time.** Fixtures need an unlocked interactive
  desktop and foreground input. Run them sequentially.
- **Wait for layout.** Before injecting input, wait for both the published state
  and the arranged control bounds, then check that the hit belongs to the owned
  app. Use `Control -Arranged` from `CapyUia.ps1` when locating a pointer target.
  A model update alone does not mean the visible rows have moved. Expand a
  collapsed column before addressing its group grip, and wait for a menu to
  acquire keyboard focus before sending Escape. Cancellation comparisons use
  the saved workspace layout; measured bounds can change as controls settle.
- **Measure timed input.** Use the native test drivers for double presses and
  prediction strokes. PowerShell sleeps and method calls can exceed a gesture's
  interval under load; retain the actual injection timing with the result.
  `RowPointerDriver.PenHover` and `Up(true)` keep a stationary pen in range;
  call `PenLeave` to end hover. The driver refreshes hover through its input timer.
- **Preserve OS cancellation.** A released sample with `IsCanceled` is a cancel,
  including for the hold recognizer. Keep the canvas-touch fixture's rapid
  canceled pairs: they must preserve the drawing instead of triggering Undo.
- **Wait for the operation you started.** Compare the document revision or the
  requested state before and after input. An already enabled Undo command does
  not prove that a new stroke has committed.
- **Capture composed pixels.** GPU pixel assertions use the owned window's
  screen rectangle on the private desktop. `PrintWindow` can return an earlier
  SwapChainPanel image. Wait for the expected visible change before comparing
  pixels. For restoration, compare with the original pixels; a fixed dark-panel
  threshold cannot verify the same journey in the light theme. Move the owned
  pointer away from the capture area and dismiss tooltips before a baseline.
- **Use current menu identifiers.** `NativeMenuItems` uses the shared command ID
  for routed commands; other layer actions use `layer-menu-<op>`. Check the shared
  menu definition when an item cannot be found.
- **One snapshot per assertion.** Read related fields from one state file; check
  its `process_id`, `window_id` and freshness.
- **Failures keep the app.** A failed fixture leaves its app running for
  inspection. Close it before rebuilding. The VM runner preserves its evidence
  and closes the processes it owns before continuing.
- **Injection is not hardware.** UI Automation and injected input do not establish
  physical pen and touch behavior, painting cadence or latency. Compare an
  injected-input failure with physical input before changing native capture.

`compact-color:dark` and `compact-color:light` select the compact color fixture's
swatch journeys. They check composed overlap and border pixels, native mouse,
pen and touch contacts, mouse and pen hover, transparent paint memory, keyboard
focus and retained control identities. The default `compact-color` also runs its
wheel, readout, menu and drawer journeys. Use both themes when changing swatch
presentation.
`compact-color:input` isolates overlap routing and focus from border pixel checks.

`exercise-workspace-pickup.ps1 -DebuggerPath <cdb.exe>` attaches CDB before input
and saves an access-violation stack and dump in the run directory.

Panel tab strips use `ScrollView` with horizontal content and reserve touch and
pen input for workspace dragging. Wheel scrolling remains native. Run every
`tab-pickup` device variant and `tab-drag` after changing the strip or its preview
clipping; they cover direct tear-off, retained contact, cancellation and history.

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
reconstruction and checks that completed edits remain saveable.

The isolated UI test hook drains the rendering and presentation queues on every
participating render worker before calling `ID3D12Device5::RemoveDevice`. WARP can crash when
manual removal races queued presentation work. The hook still removes the real
device; active contacts and queued input survive successful reconstruction.
Replacing the surface must not cancel those contacts. The ignored Rust removal
tests above exercise in-flight GPU work on hardware without this drain.

### Control reviews

Separate review builds replace the app's entry point with one production control
fed by synthetic Rust models, for comparison with the Web controls. Generate the
models at the display's actual scale, then build and capture:

```powershell
cargo run --locked -p layer-windows --example number-fixture -- artifacts/windows/numeric-matrix
./apps/layer-windows/scripts/build.ps1 -ControlFixture Number -OutputDirectory artifacts/windows/NumberReview
./apps/layer-windows/scripts/capture-number-controls.ps1 -Executable artifacts/windows/NumberReview/CapyCanvas.exe -FixtureFile artifacts/windows/numeric-matrix/fixture-windows-light.json

cargo run --locked -p layer-ui --example compact_color_fixture -- artifacts/windows/compact-color-parity 1.5
./apps/layer-windows/scripts/build.ps1 -ControlFixture Color -OutputDirectory artifacts/windows/ColorFixture
./apps/layer-windows/scripts/capture-compact-color.ps1 -Executable artifacts/windows/ColorFixture/CapyCanvas.exe -FixtureFile artifacts/windows/compact-color-parity/dark-160.json
```

Pass the native reports to the `number-controls` and `color-panel` scenarios of
[the visual tools](../../tools/visual/README.md). Rebuild normally before using
the editor again.

## Matched editor captures

Compare the editor with the Web app at the same document, workspace, theme,
viewport and scale. Build the Web reference with `bash apps/layer-web/build.sh`
(from Git Bash, after the [Web prerequisites](web.md#prerequisites)). Install
Node.js and the Python packages in
[`tools/visual/requirements.txt`](../../tools/visual/requirements.txt), and set
`CAPY_CHROME` to Chrome's executable. Chrome must use hardware WebGPU.

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
manifest records the offset.

Compare every image pair and look at the screenshots as well as the geometry
reports: matching geometry does not mean matching pixels. Never rescale, crop or
mask captures, and never loosen comparison tolerances to make a comparison pass.

## Diagnose a running app

Launch an owned review as in [matched editor captures](#matched-editor-captures).
Then:

- `./apps/layer-windows/scripts/inspect-window.ps1 -ProcessId $review.Id` saves a
  window capture and lists UI Automation names; `-ClientOnly` captures the client
  area. `probe-displays.ps1` reports each display's active mode.
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
