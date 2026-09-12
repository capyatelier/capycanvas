# Capy Canvas for Windows

Development prototype using WinUI 3/C++/WinRT and the shared Rust/wgpu D3D12
renderer. The prototype builds and runs with a GPU canvas, an independent input
dispatcher, correct DPI composition, and asynchronous resize/shutdown. Controlled
pointer replay verifies drawing, undo/redo, and a painted document continuing
behind titlebar controls. Full OS input, workspace parity, recovery and
presentation acceptance remain open; this is not a release package.

## Milestone integration

Windows implementation work lives on `ports/windows`. At each major milestone, merge
the latest `origin/main` into that branch, resolve conflicts with the shared
behavior intact, run the affected checks and record any remaining acceptance
gaps. Keep related implementation, fixes and validation together. Commit and push the
reviewed major milestone to `ports/windows`, then integrate
and push it to `main` so other port agents can use it. If another port advances
`main` during validation, merge that update and check the affected code before
retrying the push. Never force-push over another port's work.

Stage source, tests and public documentation explicitly. Keep local settings,
profiles, captures, traces, machine logs, binaries and private paths out of
commits.

## Build

See the [Windows development guide](../../docs/development/windows.md) for prerequisites,
NuGet setup, build commands and output locations.

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

The full acceptance plan is in ../../docs/history/windows-implementation.md.

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
and native layer controls (see the Layers checkpoint below). Numeric editing uses Rust's expressions, units,
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
source decoding. Full editor docking/customization, runtime filter imports, remaining panels
and matched-state visual parity are unfinished. Native frame accounting
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
validation findings in ../../docs/history/windows-implementation.md for measured results
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

## Color panel checks

The Window menu's Color panel and the toolbar's Brush color popup project the shared
HSV/HLS model with native controls and GPU gradients. Pointer-region selection
and all color edits go through Rust. The image uses Windows'
[Direct2D gradient meshes](https://learn.microsoft.com/en-us/windows/win32/api/d2d1_3/ns-d2d1_3-d2d1_gradient_mesh_patch)
through WinUI image-surface interop; the main canvas retains its independent
D3D12 presentation path.

Build the shared pixel oracle and run against a fresh, isolated review instance
with CAPY_TRACE_UI=1. Pass a Python interpreter with Pillow installed:

~~~powershell
cargo build --locked -p layer-ui --example color_wheel_reference
./apps/layer-windows/scripts/exercise-color.ps1 -ProcessId <app-process-id> -StateFile <app-output-directory>/ui-state.json -Python <python-executable>
~~~

The fixture opens Color, checks native actions and popup synchronization, then
captures the entire app window. Rust classifies sampled wheel pixels and computes
expected colors. Four HSV/HLS cases include remembered hue with black paint.
Reports remain under ignored artifacts/windows/color. This does not validate
physical pointer capture, full-window visual parity or presentation performance.

## Tool controls checks

Tool Set projects shared groups and subtools. Its Drawing tool chooser exposes
all primary drawing commands. The Window menu's Tool panel uses the shared
numeric schema and command state in a retained native ScrollView.

Run the fixture in a fresh isolated review instance with CAPY_TRACE_UI=1 and
CAPY_SMOKE_TEST=1 (the latter supplies the controlled stroke for transform):

~~~powershell
./apps/layer-windows/scripts/exercise-tools.ps1 -ProcessId <app-process-id> -StateFile <app-output-directory>/ui-state.json
~~~

The fixture checks tool/schema projection, numeric edits, field/button/scroll
retention, draft contexts, gradient and figure subtools, ruler toggles and
transform cancellation. It leaves the Tool panel visible, allowing the Color
fixture to additionally test a narrow fractional-width allocation. These checks
do not establish physical input, full-editor parity or presentation acceptance.

## Native document workflows

The Windows host has a bounded document worker for source-project saving and
background New/Open preparation. The worker writes a flushed sibling temporary
file before atomic replacement, validates decoded project limits, prepares a
separate renderer on the same D3D12 device, and retires replaced GPU resources
off the canvas owner. Shared checkpoints and document generations protect newer
edits during save, open and close decisions.

The native File menu provides New, Open, Save, Save As, Export PNG and Close. New drawing
uses shared size limits and numeric expressions. Open and Save use the Windows
App SDK desktop pickers; disk work and GPU preparation remain on the document
worker. A modified drawing presents Save, Discard Changes and Cancel before
replacement or close. Cancelling a picker preserves the current drawing.

Preferences and document prompts share the window's dialog slot. The canvas is
disabled until a modal dialog fully finishes. Closing commits Preferences drafts,
waits for document authorization and outstanding dialog callbacks, and releases
retained XAML controls before closing their window context. The opt-in UI trace
also writes local lifecycle.log stage timings.

Export PNG captures the full document in sRGB with transparency, independently
of viewport zoom, rotation and native chrome. The canvas owner submits a GPU
snapshot after presentation; GPU waiting, row packing, PNG encoding and atomic
replacement run on the document worker. Export preserves the project destination
and unsaved state. Edits made before a deferred capture is ready require retry;
edits after capture cannot change the exported snapshot. File replacement retries
brief Windows access or sharing failures on the worker for up to one second,
with cancellation checks; persistent failure preserves the prior file.

The Layers footer imports images through the Windows picker. The document
worker uses Windows BitmapDecoder to produce straight RGBA8 in sRGB, respecting
EXIF orientation. Source and oriented dimensions are checked before decoding
pixels, with an 8192-pixel limit per dimension (or the device limit if smaller).
PNG, JPEG, BMP, GIF, TIFF and JPEG XR use Windows codecs; WebP and HEIF depend on
installed codec support. Animated and multi-frame sources import their first
frame. Core owns placement, selection, Undo and embedding the pixels in saved
projects, so later Open does not need the source file.

The canvas owner captures the target after pending field edits and before
opening the picker. Cancellation, failure and stale results preserve the
drawing. A change to the document or editing target during decoding requires
retrying import. Other file operations cancel a pending import and wait for its
worker slot. Pixel decoding, orientation, color conversion and source copying
run off the UI and canvas threads; Core performs the final GPU asset upload.

Additional native windows remain pending, with New Window disabled. Full
workspace, physical input, device recovery, presentation and release acceptance
remain open.

~~~powershell
./apps/layer-windows/scripts/exercise-documents.ps1 -Executable artifacts/windows/Debug/CapyCanvas.exe
~~~

This fixture uses isolated profiles and synthetic projects. It drives actual
WinUI controls and native pickers, verifies Unicode paths, image import and
thumbnail readiness, focused drafts, import Undo/Redo, corrupt-file recovery,
save checkpoints, PNG export and saved/untitled close decisions. It requires exit code
zero within the original five-second close limit. Standard picker HWND controls
are used where Windows exposes no UI Automation pattern. This is controlled
automation, not evidence of physical pen delivery.

To explicitly discard a synthetic dirty review during cleanup, pass
`-DiscardUnsaved` to `exercise-window.ps1 -Action Close`. The option requires a
matching trace from an isolated profile. Ordinary Close never selects Discard
automatically.

~~~powershell
cargo test --locked -p layer-windows --lib
$env:LAYER_GPU_INDEX='<hardware-D3D12-adapter-index>'
cargo test --locked -p layer-windows documents::gpu_tests::d3d12_background_save_open_new_and_stale_adoption -- --ignored --exact
~~~

The explicit GPU test asserts hardware D3D12, saves retained source pixels,
creates a new document, reopens with exact pixels, and rejects corrupt files,
invalid sizes and stale adoption. It drives the old renderer while the worker
prepares the candidate. This is functional coverage, not frame-cadence or input
latency acceptance. Local test files and reports are not committed.

## GPU Navigator and window lifecycle

The Window menu's Navigator panel reveals the live composition through a native
XAML cutout. Its document image and camera outline draw in the main canvas's
existing GPU presentation pass. There is no preview bitmap, CPU image readback,
second swap chain, or camera-triggered document repaint. The overview pipeline
is prepared during GPU startup; unchanged placements do not request another frame.

Native zoom, rotation and reflection buttons use shared command labels and state.
Pointer capture routes Navigator gestures to the shared camera. The image uses
shared aspect-fit geometry, follows document replacement and display density,
and retains its native controls during camera updates and resize. The preview
height and button spacing follow the Android panel; both themes use shared colors.
The full editor dock preset, columns and drawers remain pending on Windows.

A minimized window is restored when an unsaved decision or picker needs to be
shown. Native SW_RESTORE preserves a previously maximized window. Existing-path
background saves do not restore the owner unnecessarily.

~~~powershell
./apps/layer-windows/scripts/exercise-navigator.ps1 -Executable artifacts/windows/Debug/CapyCanvas.exe
./apps/layer-windows/scripts/exercise-lifecycle.ps1 -Executable artifacts/windows/Debug/CapyCanvas.exe
cargo test --locked -p layer-ui --lib
cargo test --locked -p layer-windows --lib
cargo clippy --locked -p layer-windows --all-targets --no-deps -- -D warnings
$env:LAYER_GPU_INDEX='<hardware-D3D12-adapter-index>'
cargo test --locked -p layer-render-wgpu overview -- --test-threads=1
~~~

The Navigator fixture owns an isolated review, checks all six camera commands,
retained controls, actual preview pixels after a controlled stroke and Undo,
document aspect changes, resize, hide/reopen, theme colors and zero exit.
The lifecycle fixture checks clean and dirty minimized close, visible decisions,
Cancel preservation, maximized-state preservation and explicit Discard.
Captures and profiles remain under ignored artifacts/windows.

These are functional checks. Physical Navigator pointer gestures, full workspace
visual parity, mixed-DPI movement, device recovery, painting cadence and
input-to-present latency still require acceptance. The overview performance test
remains ignored during these checks.

## Properties and filter previews

Properties renders shared number, choice, toggle, RGBA color, curve and gradient
controls with native WinUI widgets. Curve plots and effect constraints come from
Rust. Reset, point/stop selection and document changes invalidate old edit drafts.

Filters uses the shared category/search catalog and GPU previews, following
Android's row layout. Only visible rows request previews, in batches of at most
eight every 200 milliseconds. The drawing owner polls without waiting, and a
separate worker converts packed RGBA pixels for native WinUI bitmaps. Preview
requests and memory are bounded, and obsolete document/revision results are
discarded. This optional picker readback does not replace the GPU canvas or
Navigator presentation paths.

~~~powershell
./apps/layer-windows/scripts/exercise-effects.ps1 -Executable artifacts/windows/Debug/CapyCanvas.exe
~~~

The isolated fixture checks all six property kinds, reset draft guards, retained
curve controls, resize, category/search/insertion, actual preview pixels after a
controlled stroke and exact Undo, both themes, document replacement and zero exit.
It saves only app captures and synthetic state under ignored artifacts/windows.
Runtime filter package import, complete Layers/workspace controls and the full
editor preset remain pending. These checks do not establish physical pointer
gesture, full visual parity or frame-cadence/input-latency acceptance.

Debug builds optimize Naga, the WGSL compiler dependency, while retaining
debuggable application Rust. Process exit joins retired shader workers after
all canvas hosts are destroyed. The lifecycle fixture covers close before
brush readiness, during shader warmup and from clean/dirty minimized windows;
it retains the five-second zero-exit requirement. One image-import milestone
review exceeded that limit in the final shader-worker join; subsequent document
and lifecycle runs pass, but intermittent cold-compilation close timing remains
an open acceptance issue.

## Native Layers editing checkpoint

Layers uses a virtualized WinUI ItemsRepeater. Its rows follow the Android
spacing, indentation, shared icons, selection tint, and separate content/mask
editing markers. Blend and opacity, alpha/edit locks, clipping, references,
selection checkboxes, visibility, masks, renaming, grouping and the recursive
context menus dispatch shared layer actions. Native drag-and-drop supplies row
geometry to the shared Drop action, including the paper anchor and group
insertion boundaries. Physical drag/pen/touch acceptance remains open.

Thumbnail requests and menu queries use a bounded queue separate from pen
input. Menu requests replace older queued menus; pixel queries retain FIFO
ordering. The shared host returns owned 32px thumbnail pixels without serializing
them into JSON. Windows converts them off the UI thread and retains at most
128 thumbnails, with at most eight pending readbacks. Document epochs and
thumbnail revisions reject obsolete results. Layer actions and effect drafts
also carry an epoch checked by the canvas owner.

Run the isolated native fixture against a built executable:

~~~powershell
./apps/layer-windows/scripts/exercise-layers.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
~~~

The fixture uses a disposable settings directory and app-only captures under
ignored artifacts. It covers painting and exact thumbnail restoration after
Undo, retained rows, shared controls, masks, multi-selection, rename,
duplication/deletion, grouping, collapsed editing targets, row virtualization,
theme changes and document replacement. It does not measure presentation or
physical input latency.

Image-as-layer import is covered by the native document fixture above. Runtime
filter package import remains pending. The complete workspace projection and
physical docking acceptance remain in progress, so the full Windows editor
preset is still gated.

## Workspace projection in progress

The Windows header projects all eight shared application menus. Menu labels,
sections, checkmarks, enabled states, shortcuts and actions come from the shared
snapshot. Help links resolve the shared link identity on the canvas owner and
use the asynchronous Windows launcher; completion or failure acknowledges the
shared request. The UI retries a full optional query queue without busy polling.

Native panel bodies are separate from tab and dock decoration. Each placement
owns its controls while sharing the window's thumbnail and filter-preview
caches. All five toolbar tile styles follow the shared rectangles and icon/label metadata;
divider tiles use their compact separator geometry. Diagnostics uses the shared
rows and chart order and requests CPU-only telemetry every 200 ms while visible.

The workspace owns native gesture capture, allowing the source tab or toolbar
to be reparented during tear-off. Native tabs supply drop-hit geometry; shared
actions own movement, divider/floating resize, docking and workspace history.
Drop hints use one outstanding asynchronous query, retain the latest position,
and resolve tile drops at release. Escape, capture loss, deactivation and close
cancel active gestures. Native chrome facts survive subsequent canvas events.

~~~powershell
./apps/layer-windows/scripts/exercise-workspace-layout.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
~~~

This isolated fixture checks menu identity/order, selection commands, Help's
About page, live Diagnostics rows, retained metrics, resize, visibility and
workspace Undo. It does not activate the external browser. Physical docking
and tablet gestures still need acceptance.

Content drawers and collapsed columns now have native projections. The shared
owner supplies animated bounds, connecting shapes, column widths, wrapping tile
geometry and collapse/expand history. WinUI reports natural content heights and
clipped tile origins. Independent panel bodies retain native controls across
drawer updates; Navigator uses the existing GPU overview path through a clear
opening in the drawer background. Native scrolling handles column overflow.
Keyboard context requests use the same shared menus as pointer requests.

~~~powershell
./apps/layer-windows/scripts/exercise-drawers.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
~~~

The fixture uses isolated settings and local captures. It covers repeat toggles,
keyboard context menus, expand/undo/redo, fixed-width tab selection, Navigator
preview/zoom and close. Its keyboard driver sends context-menu keys only while
the owned review has foreground focus. Shared unit
coverage includes Windows drawer toggles, column history, reversible resize,
native height queries and closing from retained geometry after model removal.
Startup chrome facts are held locally until WinUI has a nonzero viewport.

Column drawers now report their presented bounds through MeasureColumnDrawers.
Their individual tabs and fixed group grip route shared workspace drags; tab
drop targets are clipped through every native scrolling ancestor. Double-click
on a canvas-facing column divider requests the shared default-width reset.
Core tests cover Windows tear-off, cancellation, docking, tab reordering,
stale geometry and undo/redo. Physical native dragging remains unaccepted.

New Toolbar, Manage Toolbars and toolbar prompts use native WinUI dialogs with
shared names, search, selection, validation and confirmation text. Tool choices
are virtualized and native rows survive selection-only updates. Text drafts
survive delayed owner snapshots, cancellation waits for Core acknowledgement,
and dialogs share the window's modal slot with Preferences and file operations.

~~~powershell
./apps/layer-windows/scripts/exercise-toolbars.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
~~~

This isolated fixture covers catalog virtualization, retained selection, rapid
name edits, validation, creation, rename, duplicate, insertion, manager selection,
delete/cancel confirmation, workspace undo/redo, picker cancellation and closing
with a picker open. Toolbar grips support keyboard context requests.

Panel configuration uses the shared expansion placement, 200 ms transition,
joined outline and configuration width (380 DIP, clamped by the viewport).
Preview controls remain attached through opening, closing and resizing.
Configuration controls share values, visibility choices and toolbar actions
with Core. Layers respects the selected editing target and document epoch.

GPU Navigator previews stay on the existing canvas surface. A native
CompositionGeometricClip removes the preview openings from lower XAML visuals
in paint order, preventing an underlying panel from covering a higher preview.
Clip geometry changes with layout; painting does not copy preview pixels or
create additional swap chains. Removing an overview restores the lower visual.

~~~powershell
./apps/layer-windows/scripts/exercise-expansion.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
~~~

This isolated fixture checks shared width, narrow-window layout, retained
configuration controls, live numeric values, visibility, editing-layer selection
and targeted opacity, Escape, toolbar insertion and theme changes through
Preferences. Its app-only pixel check verifies a GPU overview over an opaque
lower panel and restoration after closing configuration.

## Full editor and Zen checkpoint

Windows initializes the same complete editor preset as Android and Web:
Tools, Tool Set, Tool/Brush size, Color, Navigator/Diagnostics,
Properties/Filters, Layers and Commands. The temporary tool chooser is removed.
Native tab widths and natural content heights are reported after layout, with
coalescing and acknowledgement checks. Layers measurements use the virtualized
scroll extent; reporting does not realize every row.

Partial Zen projects Core's toolbar sections with stable tile identities.
Native titlebar left/right insets and caption height are transient Core
measurements. They reserve caption-button space in the same geometry used for
toolbar hit testing and drawer anchors, survive workspace undo/reset/switching,
and never enter saved layout history. Windows excludes Zen controls from native
window-drag regions and only republishes those regions when they change.
The GPU canvas still extends continuously underneath the custom titlebar.

~~~powershell
./apps/layer-windows/scripts/exercise-editor.ps1 -Executable <native-exe>
~~~

The isolated fixture checks Core rectangles, settled panel measurements, all
drawing tools, retained fields/scrolling, partial/total Zen, drawer activation,
resize retention, restored workspace geometry, both themes and clean exit.
It also checks native `WM_NCHITTEST` results for top Zen controls. Its reference
capture uses a measured 986 by 658 DIP viewport and Fit canvas, with actual
display scale and uncropped client-image dimensions recorded locally. This
supports comparison with `tools/visual/chrome-capture.mjs`.

The editor styling pass aligns desktop header spacing, the shared grip asset,
disabled icons, active-tab shoulders, compact layer opacity and property-choice
rows. Tool Set follows the GTK/Android full-width preview arrangement; the Web
reference differs there. Full visual parity remains unaccepted.

Attached tabs use shared frozen geometry and insertion thresholds. Native
Composition animations slide neighboring copies without moving original hit
rectangles. The tab strip and scrolling content survive panel-body replacement
during tear-off, preserving the active pointer capture. The bridge queues Down
and BeginTabDrag together. Shared workspace updates supply tab previews and drop hints.

~~~powershell
./apps/layer-windows/scripts/exercise-tab-drag.ps1 -Executable <native-exe>
~~~

This fixture injects OS touch into its owned review window and checks two grab
positions, fixed hit rectangles, release insertion, attached and detached
cancellation, held floating movement and workspace Undo. A separate contact
timer keeps delivering held frames while UI Automation or screenshot capture
blocks the observation thread. It does not establish physical digitizer or
latency acceptance.

## Incremental workspace motion

Windows uses NativeHost::take_update_bytes. Full snapshots establish retained
models; matching workspace_update messages move only native presentation.
Revision checks reject stale placements and mismatched content. The UI mailbox
retains separate full-model, motion and camera slots, so replacing a motion
packet cannot lose its camera update. Full refreshes supersede older placement.
All input phases and commands remain ordered in the existing input queue.

Floating frames and their resize grips use native translation transforms.
TransformToVisual supplies matching hit/clip and GPU Navigator coordinates;
motion publishes the overview allocations and native transparent holes directly,
without rerunning panel layout or requiring a layout callback. Attached-tab
previews and drop hints use the same stream; only toolbar-tile drag/drop retains
the separate geometry query. Fast tear-off transfers the source Button capture
using the original press path, even when the first move is already outside it.

The touch fixture additionally verifies unchanged full-model counts and model
revisions during steady dragging, native control identities, shared/native
absolute positions, resize-grip movement, a simultaneous camera update, and GPU
Navigator pixels over an opaque lower panel at two positions. Cancellation
restores the lower pixels and workspace. CAPY_TRACE_UI exposes local presentation
diagnostics for these assertions; ordinary runs do not publish that test data.
The serializer tests compare full and incremental wire values, release/cancel,
Undo/Redo and camera state. Separate native tests cover mailbox coalescing,
revision ordering and ordered input boundaries.

Main through 007c3e9 is integrated, including the approved shared workspace
manager handoff. Workspace storage is integrated as described below. Windows
still needs the full manager UI, multiwindow support, runtime filter package import,
packaging, physical gestures, DPI/device lifecycle, full visual parity and all
overlap/scroll/drag combinations. The intermittent shader-worker final join and
strict GPU filter-reference mismatch remain open. Final painting presentation
and input-latency acceptance remain deferred.

## Workspace persistence and recovery

The active workspace now autosaves its layout history and latest tool values in
`workspaces.sqlite3`, beside the private preferences file. One shared native
SQLite worker serves each application directory. Futures remain on the exclusive
canvas owner and wake it only when replies arrive; idle service polling does not
force GPU presentation. Full captures follow committed layout generations, and
pending saves retain later edits for the next save.

Startup adopts the saved workspace at the shared idle/brush-ready boundary.
Workspace adoption and layout restoration reconcile native measurements even
when the existing controls have unchanged sizes. Earlier Windows preferences
never stored workspace layouts, so there is no legacy Windows layout to migrate.

Lease expiry blocks editing and cancels active input until ownership is recovered.
Failed saves retain the in-memory workspace and expose Retry, Save as New Workspace,
and backup export. If the database cannot be opened, the original is preserved
and the recovery view offers retry and database backup. Backups cannot replace the
live database or its WAL/SHM sidecars. Normal close flushes the final edit and
releases ownership before teardown; a failure offers Retry, Keep open, or explicit
Close without saving. These recovery controls are separate from the full manager,
whose approved UI is defined in the
[host handoff](../../docs/ui/workspace-manager-host-handoff.md).

~~~powershell
./apps/layer-windows/scripts/exercise-persistence.ps1 -Executable <native-exe>
~~~

This isolated native fixture checks autosave, layout/tool restoration, panel
measurements after adoption, final-edit close, unreadable storage, editing guards,
Keep open, repaired-storage retry, explicit discard, preservation of the original
file and zero-exit shutdown. Captures, database files and logs remain ignored/local.
Shared and bridge tests cover pending saves, ownership takeover, failed-operation
identity, backup contents and worker teardown. This does not establish full manager,
physical-device or presentation/input-latency acceptance.

## Native multiple windows

File > New Window and Ctrl+Shift+N create another native window in the same
process. Each window owns its document, GPU canvas, input dispatcher, dialogs and
workspace claim. Closing the original window leaves the others usable; process
shutdown follows the last window. The canvas continues behind each custom titlebar.

Preferences are shared between windows using the same private profile. New windows
inherit current values, including pending saves. Independent edits merge by field
and shortcut key, and background file replacement is serialized across windows.
A stopped window disconnects its callbacks before its host is released.

~~~powershell
./apps/layer-windows/scripts/exercise-multiwindow.ps1 -Executable ./artifacts/windows/Debug/CapyCanvas.exe
~~~

The fixture owns an isolated profile and checks native menu/shortcut creation,
workspace-owner activation, simultaneous Preferences dialogs, shared values,
independent documents, drawing while another window is modal, cancelled close,
closing the original first, and zero process exit within five seconds. Drawing
uses controlled replay; this is not physical input or performance acceptance.

With CAPY_TRACE_UI enabled, windows-<process>.json records live window IDs/HWNDs
and ui-state-<process>-<window>.json identifies each window's model. These local
files can contain private state and stay ignored. The initial window also keeps
the legacy ui-state.json path for existing single-window fixtures.

## Native task workspace management

The WinUI workspace manager follows the current
[host handoff](../../docs/ui/workspace-manager-host-handoff.md) and
[task workspace definitions](../../docs/ui/default-workspaces.md). The titlebar
pill switches the stable Painter, Illustrator and Photographer identities through
the shared ownership/save path. Names and edits follow each workspace; included
workspaces can be renamed but not deleted. Custom workspaces leave all three
segments unselected. The shared upgrade preserves customized Photographer layouts.

Manage Workspaces and Layout History preview arrangements in the editor behind
the dialog. Selection, double-click and Enter do not commit the preview. Explicit
confirmation applies it; Cancel, Escape and dismissal restore the original.
New Workspace asks only for a name and copies the current arrangement and tool
settings. Restore Starting Layout preserves current tool settings. Reset All
Brushes confirms once and saves the shared brush reset without layout history.
There is no Save Layout or Load Layout UI.

Read-only loads are cancellable and dialog generations reject stale replies.
Accepted writes finish through the ordered canvas service. Normal close restores
temporary previews and drains accepted operations. Ownership renewal continues
while a dialog is open; the outgoing claim is released after live adoption.
Selecting a workspace owned by another process activates that window through a
transient HWND property without taking its claim.

~~~powershell
./apps/layer-windows/scripts/exercise-manager.ps1 -Executable <native-exe>
./apps/layer-windows/scripts/exercise-manager-focus.ps1 -Executable <native-exe>
~~~

The first fixture checks native lists, previews, name-only creation, history,
baseline restoration, included/custom workspace policies, brush reset, header
switching and restart. The second checks independent app instances and owner
window activation. Both enforce the existing five-second close gate; the known
intermittent final shader-worker join can still fail that gate. Profiles and
captures stay local. New Window and runtime filter transport are implemented.
Full visual/gesture parity, toolbar library round trips, lifecycle/device/DPI
validation, distribution and final physical-input/presentation acceptance remain open.

## Runtime filter packages

Builds copy the shared manifest and WGSL modules into `Assets/filters` beside the
executable. Each window reads these files on a background worker and stages them
through the shared GPU validator. A missing default directory retains the embedded
fallback; an explicitly selected missing or invalid package reports an error.
`CAPY_FILTERS_DIR` selects another directory and `CAPY_FILTERS_MODE` selects
`add`, `replace` or `merge` (default). Startup refreshes the filter library without
migrating programs embedded in an existing document.

For the already-built app, from the repository root:

~~~powershell
$env:CAPY_FILTERS_DIR=(Resolve-Path examples/filters/tent-blur).Path
$env:CAPY_FILTERS_MODE='add'
& ./artifacts/windows/Debug/CapyCanvas.exe
~~~

Native integrations can send `CanvasCommandKind::Filters` through the ordered
canvas command queue. Its render-owner entry is `capy_load_filter_directory`,
accepting JSON with optional `directory`, `mode` (default `merge`) and `library`
(default `false`). Omitting the directory reloads the environment override or
installed assets. Explicit replacement updates compatible live instances;
`library: true` keeps embedded document programs unchanged. Conflicting WGSL
names are rejected under the shared contract. `windows_filter_load` snapshots
report request ID, pending state, phase and error. The call returns zero when
accepted, one when busy/rejected, and minus one on bridge failure.

File acquisition is bounded and stays off the UI and render threads. Publication
waits for GPU validation and an idle document boundary. A delayed explicit read
cannot migrate a replacement document. Invalid metadata, missing modules and
invalid WGSL preserve the working catalog, document and values. This is the same
programmatic loading contract as the other hosts; there is no shader-editor UI.
The reload button exists only with the opt-in `CAPY_SMOKE_TEST` controls.

~~~powershell
./apps/layer-windows/scripts/exercise-runtime-filters.ps1 -Executable <native-exe>
cargo test --locked -p layer-windows --lib filter_packages::tests::d3d12_file_packages_replace_pixels_atomically_and_preserve_live_values -- --ignored --exact --nocapture
~~~

The native fixture loads the example, inserts it, edits Radius, reloads changed
WGSL/metadata without changing the executable, checks failures/retry and verifies
a changed GPU preview. The separate hardware D3D12 test compares full-image bytes
for replacement, atomic rejection and library refresh without live migration.
Profiles, copied test packages, captures and reports stay under ignored artifacts.
Neither check establishes physical input latency or presentation performance.
