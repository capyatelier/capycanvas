# Windows development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Windows client is a WinUI 3 app in C++/WinRT around the shared Rust editor.
Its Rust bridge, `layer-windows`, uses `NativeHost` and the shared wgpu renderer
on D3D12, presenting through a `SwapChainPanel`. The
[package README](../../apps/layer-windows/README.md) describes the source layout
and threads, and the [porting guide](../WINDOWS_PORTING_GUIDE.md) covers bringing
upstream features to Windows.

Incoming UTF-8 buffers stay borrowed inside the Rust bridge call. JSON ingress
deserializes into owned values; text-only transports use a scoped callback.
The boundary tests run on Linux too, and compile-fail doctests reject escaping
string borrows.

Native JSON contacts emit unsigned identifiers and nanosecond timestamps as
integer tokens through `NativeJson.h`; the WinRT JSON number type stores doubles.
Rejected input JSON reports a recoverable error before touching the session, so
the render owner can process subsequent events. Invalid handles, poisoned owners
and processing failures retain their fatal result.

Native buttons use `ButtonFeedback.h` to draw feedback between the model-owned
base fill and the retained content. Native pressed state takes precedence over
hover, including contacts without hover. Checked choices retain their fill;
selected header tools and joined panel tabs keep their established exceptions.
Authored previews and paint swatches use the same feedback brush on their rim.
Workspace pills retain their enabled appearance while the shared owner
temporarily gates switching during drawing or adoption. Their selection tint
follows adoption; holding a different workspace keeps its neutral base.

Dropdowns use `choiceColors` from `ChoiceFeedback.h`. Their native ComboBox
template retains selection, keyboard and popup handling; each instance owns its
state brushes and updates their colors when its palette brushes change. Feedback
composes over the input fill. Disabled content has one opacity owner, including
explicitly colored child content. `dialogSurface` applies the same button feedback
to the actual ContentDialog footer buttons and follows changes to the default
action. Keep native focus, activation and dialog enablement on those controls.

Startup leaves the composition surface detached while the native root shows the
shared surround. The render owner submits a defined surround while the first
constant background prepares, then admits document and brush work after that
background is drawable. Optional chrome follows the first paper frame. A queue
completion latch publishes surface readiness once per GPU generation; the UI
attaches the cached handle without waiting for the GPU. Replacement renderers
get a new latch, so retired callbacks cannot reveal an uninitialized surface.

Binary WebP preview calls reject invalid slice lengths and overlapping dimension
outputs before borrowing input. They write dimensions only after the decoded
preview handle is stored. Input and output allocations remain valid for the call.

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

Navigator reports its fixed native controls, row gap and insets separately from
the continuous overview. Each group body has one finite scroll viewport between
the fixed tab strip and the trailing drag grip. Existing panel scrollers own that
viewport; other panel roots receive a group scroller. Shared `min_body_height`
sets the minimum content extent without enlarging the viewport.
Fitted roots take the ScrollPresenter's finite allocation, including its native
pixel rounding. Assigning a separately rounded content height can create false
overflow; existing panel scrollers still measure their genuine content extent.
Navigator measures and arranges its overview and fixed action row together from
that allocation. It does not resize the overview in a later dispatcher callback.
Its allocation key includes its position in the window; changed coordinates
cancel an active Navigator contact before publishing the new placement.
Unchanged layout notifications do not republish its geometry.

Drawer height providers take the proposed column width before placement. A new
drawer stays hidden until its measured placement is ready. Its source publishes
one accepted anchor, direction and bounds tuple; header buttons and their bars
apply the shared facing and source-corner rules in the same native update.
Changing the toolbar context retires the previous drawers.

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

The release checks run tests with uncaptured Rust output and full backtraces.
Their Windows artifacts retain separate native stdout/stderr logs and, on failure,
Application Error and Windows Error Reporting events. A native process abort
still fails the job even when Rust cannot print a test assertion.

Ordinary restarts reopen saved and untitled drawings automatically, retaining tab
order, the active drawing, camera, selection and bounded Undo/Redo. The first
window's session worker counts the other saved window sessions, and the app opens
one window for each so every window returns at launch. Closing the
window flushes the session asynchronously and destroys its GPU and storage owners
on a worker after detaching the surface; explicitly closing a drawing still
uses Save/Discard/Cancel and removes its durable membership before cleanup.
Abnormal restores mark drawings as recovered until their next save.

Workspace manager futures remain on their creating render thread. The movable
canvas host holds a typed handle to that thread's workspace service; access from
another thread is rejected, and service shutdown removes it on its owner before
background host cleanup. File transport futures carry only owned, Send state.

Session workers reuse immutable resources and coalesce changes over two seconds.
Unchanged drawings produce no periodic writes. Atomic publication preserves the
previous complete drawing checkpoint when storage fails, and live windows hold
exclusive leases. Startup admits the complete session before preparing renderers;
inactive drawings use one scratch renderer at a time. The active canvas appears
after this bounded preparation of all tabs. A drawing that cannot be read or
prepared, or whose restore was interrupted, stays on disk with its identity
reserved while the others reopen; the window then asks about each one with the
shared Later, Discard and Retry copy. Known failure details stay with each saved
drawing across restarts and clear only after its restore is durably acknowledged.
Retry prepares that drawing on the session worker and adds it as an inactive tab.
Cleanup errors for retired copies are reported separately, so readable drawings
can reopen and successfully published checkpoints remain acknowledged. Resource
writes and membership publication must still succeed. Storage failures for the whole session present
Retry or Keep Open; Keep Open starts a separate session while retaining those copies.
A restored destination is checked before saving so external changes cannot be
silently overwritten. Use Save As when the original drawing changed.
File workers use `color_storage::export_profile` for imported profiles in export
options, export presets and print-proof setup.

Packages with unsupported or damaged authored content open in a separate read-only
view. The current drawing remains open. The view presents the shared package
status, available output names and bounded preview, with Copy Original, Export
Preview and Close actions. Copy Original writes the retained package bytes on the
document worker. Export Preview appears when the package has a verified preview
and writes those exact PNG bytes on the document worker, refusing the package's
own path. Both replace the save picker's placeholder file atomically.
Unsupported portable packages remain in that view.

Preview requests queued for a closed workflow return an empty image packet.
Current workflow requests still report invalid preview indices and preparation
errors. Closing an export or rebuilding the GPU therefore retires its images
without leaving an error banner from an outdated request.

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
when the window prunes them. Visible canvas notices update their text, ordered
action labels, availability and reasons without replacing retained action buttons
or restarting the notice deadline. Native action buttons wrap within the notice.
The notice measures both a side-by-side layout and actions below the message,
then uses the shorter layout, keeping the side-by-side layout when heights match.
This keeps narrow notices readable and contained even where the Web notice's
long buttons overflow. Run `exercise-localization.ps1 -NoticeOnly` in each theme
for the focused image-notice journey, including a narrow canvas and returning to
the original width.

Tool lists keep enabled choices clickable when their shared projection includes a
reason. Only their content dims; the selected background stays visible. The same
reason supplies native tooltips and HelpText, and wins over delayed shortcut copy.
Header and choice controls read the shared reason; notice actions read the shared
accent flag and keep native disabled colors. Renew the vector-layer, vector-edge
and image-notice journeys in both themes when changing these presenters.

Top-level application menus retain their buttons and flyouts when the language
changes. Captions and natural widths refresh once per localization generation;
unchanged publications skip menu text reads and measurements. Clock widths include
the native control's padding in both customization and fullscreen. Choice controls
retain each option's TextBlock even while WinUI moves the selected content into
the collapsed presenter, so visible and accessible captions update together.
Compact choice fields use zero vertical padding so the native selected presenter
fits the shared control height; larger dialog choices retain native padding.
Review the full closed gradient caption in both themes, including Gradient Fill
and Gradient Map.
The compact workspace selector keeps its shared Workspaces caption and a
144-DIP width. A single drawing title uses the Web header's 180-DIP natural and
80-DIP compact widths; multiple drawing tabs keep their existing width policy.
Expanded workspace buttons cache the measured caption plus their native padding,
up to 130 DIP. Their named group contains both the choices and Options; scroll
automation uses the inner viewer.

The native bridge uses typed generation handles for hosts, launches, localization,
preview packets and text. Stale and wrong-kind handles are rejected before access;
active operations retain their state through release. C++ owners copy text and
preview bytes through checked ranges of at most 8 MiB. Packet copies are shared
across preview readers and acquired on the image worker. Caller-owned input and
output buffers must remain readable or writable and exclusive during each call.

Pointer batches validate structure and chronology before routing. Numerical
sample refusals use shared admission so valid neighbors still reach the canvas.
Damaged real releases cancel from the last valid pose; the native recovery cache
keeps valid real samples and retires matching releases.

The render owner routes snapshots using ranges in the exact copied UTF-8.
A window coalesces raw presentation slots before one background decode. At most
one decoded batch waits for UI ownership; the next decode starts when the UI
takes that batch. Full models retain their dependent layout, motion, camera,
search and glass order, including localization carried by an earlier batch.
Query replies wait until the UI callback for their snapshot has been queued;
queued, in-flight and deferred queries share the eight-query admission limit.
Decoding and workspace-reply parsing stay off the UI thread. JSON objects transfer
exclusively to the UI, and closing retires queued work without waiting on it.
A decode failure stops the channel instead of applying dependent patches.
Full model parsing on the render owner is limited to opt-in UI traces.
Presentation and overview pipelines are prepared on the shader workers;
an acquired frame remains owned until presentation is ready. Cursor and brush
readiness keep their original publication boundaries.

Viewport and panel resizing use shared layout packets while their content
revision remains unchanged. Native controls retain focus, capture and scroll;
new content, collapse and completed gestures retain full model publication.
Header item measurements at an unchanged bar height also retain content;
height changes publish the complete model and its new clearance.
The presentation mailbox applies the latest required layout before later motion.
Inactive panel measurement uses the live body's allocated width and reapplies
bindings on an accepted full publication; width changes update and measure the
retained body. A disabled native control contains the inactive measurement tree.
Color-picking strips accept focus only while shown. Dialog focus restoration and
Tab navigation use visible controls.
Filter preview heights follow their stretched content width,
keeping cached and visible tab measurements consistent after resizing. Collapsed columns
retain their path while its size and corner radii are unchanged. GPU readiness
updates independently of that geometry cache. Layout observers must not replace
identical geometry or rebind unchanged hidden controls on every notification.
Workspace observers collect measurements, panel bridges, backgrounds and Navigator
placements through one pending XAML rendering callback. Control placement stays
immediate; glass collection does not re-enter layout observation. Accepted layout
or document changes invalidate outstanding bridge replies before the deferred
measurement. GPU acknowledgements update native fallback visibility separately.
Caption and interactive regions use the non-client pointer source directly;
the published passthrough cache includes the regions already submitted during
resize. Leaving the overlapped presenter clears custom caption/input regions.
WinUI attaches a composition-surface handle during creation and recovery.
Buffer configuration and resize stay on the render owner, outside the UI thread.
Buffer capacity grows in 128-pixel blocks; source rectangles and presentation
viewports follow the window without reallocating buffers within that capacity.
Surface loss forces reconfiguration even when the viewport fits existing buffers.
Three buffers permit a bounded two-frame queue while input is drained after
acquisition. The source-to-buffer ratio and DPI determine the composition matrix.
Continuous frame deadlines start at acquisition rather than after rendering and
follow the window's monitor refresh rate.

`QueryWorkspace` uses the native workspace packet allowlist in
`native/src/workspace.rs`. Admit each shared query there and cover its serialized
result/error packet in the native tests, including name validation and saved-profile
matching. UI replies retain their requesting prompt or dialog identity.

Empty localization metadata preserves the current context; a pending envelope
stays queued until a full snapshot carries it. Properties, tool controls, filter
results and options use semantic identities for their structural keys. Workspace
groups rebuild a panel body only for structural changes; bounds, resize handles,
footer grips and the `scrolls` overflow flag retain its controls, focus and drafts. Tool settings
use `group_id` for section identity and localized `group` only for heading copy;
toolbar schemas retain action group IDs when stripping translated text. Current
captions update retained controls, accessibility names and tooltips. Numeric
refusals retain their shared reason, and color forms relabel their rows and
re-read a refused value in the new language without changing the draft. Profile,
proof and export captions preserve raw names, selections and prepared candidates.
`NewDrawingDialog.cpp` presents the shared [New drawing](../ui/new-drawing.md)
view: it relabels from the view's copy, keeps a numeric draft that is still being
typed, and embeds Edit Color's form and the Choose Texture grid in its drawers.
The paper-color editor stays attached for the New drawing dialog's lifetime.
Reopening resets its draft from shared state; closing the dialog releases it.
Use `-Journey paper-color` for repeated transparent/color apply, Cancel and
reopen checks in both themes, including retained native control identities.
Fixtures create drawings with `New-DrawingPixels` from `CapyUia.ps1`.
Run `exercise-documents.ps1 -Executable <path> -Journey new-drawing` through
`test-desktop.ps1` for the existing dark and light New drawing journeys: presets,
paper and color drawers, typed Texture visibility with Use Texture and Cancel, narrow
footer layout and sheet creation. The default `full` journey also exercises the
document import, export, save, reopen and close workflows. Use
`-Journey gpu-recovery` for the focused reconstruction journey, or `-RecoverGpu`
with the full document journey.
Preferences reserves the measured titlebar height when fitting its centered
dialog; its narrow-window check dismisses it with an actual pointer click.

Apply `dialogSurface` to native dialogs for the shared `settings` background and
clear top overlay. Retained dialogs refresh it through their apply or render
hooks; embedded Edit Color inherits its parent surface.

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

The opt-in workspace geometry trace includes toolbar components, slider controls
and panel widgets, including the toolbar-only Sketch workspace. Matched captures
require these native controls to settle before recording images.

Header menus limit their scrolling presenters to the space below the heading
inside both the window content and the monitor work area. Moving or resizing a
window must preserve menu-heading access during hover switching.
Their natural width includes the capsule's measured horizontal padding and
heading spacing, so the final heading retains its full allocation.

`open-application-menu.ps1` uses the submenu IDs of the visible menu entry point.
The compact `application-menus` button exposes `application-menu-<id>` entries;
primary and recovery menus retain their ID and localized-caption lookups.
Action lookup waits for an enabled visible match, skipping disabled duplicates.
Selector properties are fetched together in a UIA cache request using the same
raw tree view as the descendant search. Transient
ElementNotAvailable errors while a popup appears retry within the existing
discovery deadline.
After opening the compact menu, discovery reads the window's immediate children
and searches only its WinUI popup hosts, avoiding unrelated background controls.
Inspection still permits disabled visible buttons; an existing submenu completes only the
normal expansion path. Discovery retains that native entry through invocation
so another tree read cannot discard it during a menu transition. Visibility,
enabled state and expansion checks remain.

`exercise-header.ps1` compares the Clock caption before and after moving it from
the component bank into the title bar, including its visible width.
Use `-Journey background` for only mouse, touch and pen background-hold menus.
The editable title background captures its contact immediately, so touch and pen
holds keep their context menu through release. Outside customization, native
window dragging keeps its normal input path.
In Paint, it also checks the full workspace group, caption padding and options
containment at 1200 DIP, then the compact selector at 960 DIP. The switcher journey
adopts Paint and removes its center drawing title through customization before
creating long-named workspaces. It requires the expanded selector, checks horizontal
scrolling and fixed Options, then tests compact presentation separately.

Use `exercise-localization.ps1 -HeaderOnly` for retained application-menu names,
widths, shared/native geometry and current menu commands across all shipped
languages. `-BlendOnly`
collects English/Japanese/English Properties choice captions before, during and
after opening the popup, preserving selection, control identity and document
state. Run both themes. WinUI can omit separate text providers for boxed button
content and collapsed choices; review the saved raw captures for caption fit and
visible text instead of treating accessible names alone as visual proof.

`-ConfigurationOnly` switches English, Japanese and English through another
window's Preferences while Brush size configuration stays open. It checks
retained hint, visibility and control captions, native identities and unchanged
layout, brush, artwork, history and GPU ownership. Run both themes.

`-ZenThemeOnly` changes the theme through another window while the first stays
in Zen. It checks hidden native controls, unchanged drawing and workspace state,
normal restoration and exact header pixels after reentering Zen in the same theme.
Run both themes; its recorded Windows animation setting does not qualify frame pacing.

`-ColorOnly` retains Color controls through English, Japanese and English from
another window's Preferences. It checks names, status, shape captions, the Edit
tooltip and native readout/swatch menu captions, retaining control identities
and exact seeded artwork pixels, camera, layout, paints, brush, history and GPU
ownership. Run both themes. Renderer-error text has source coverage only;
HDR intensity remains in the existing HDR journeys.
Composed captures restore the caller's DPI context. Pixel phases measure bounds,
capture and verify the full projection in one physical DPI context.
Run `./apps/layer-windows/tests/capture-dpi.ps1` for the windowless capture DPI
scope and clear-paper sample regressions. Paper samples keep their full pixel
rectangle inside the canvas and visible paper, outside the utility, including
fractional edges and the complete requested pointer path.
Run `./apps/layer-windows/tests/uia-owned-search.ps1` for current-drawing canvas
lookup and ordinary context-menu popup lookup.
`tests/uia-projection-snapshot.ps1` checks fresh cached control projections and
the idle fixture's per-review canvas peer with live bounds, camera and foreground
checks for every pixel sample.

Activating Preferences deliberately sends shared Blur, restoring the tool used
before temporary eyedropping. The sampler journey checks that restoration, then
reenters the picker and checks current captions and unchanged sample choices.
A windowless shared test checks pure language publication while picking stays
active; the native fixture does not claim retained sampler identity across Blur.
Run `exercise-color-picker.ps1 -Executable <path> -Theme dark` and repeat with
`-Theme light` to check native double presses, sample choices and picker previews
in isolated profiles. The held-finger check scans within the visible canvas bounds
and checks that the accepted color comes from under the finger while the loupe
is lifted above it at the current display scale.

The Layers panel presents shared attachment actions, owner visibility and group mode.
Workspace groups use their shared panel title as the accessible name.
Decorative clipping rails and effect links use realized thumbnail geometry and remain
clipped to the scrolling viewport. A content-thumbnail drop attaches to that layer's
output; a row drop uses shared insertion policy. Native drag feedback uses the shared
normalized destination and release revalidates the original hit. Thumbnail selection
loads use Ctrl, Shift and Alt from the original pointer press, including group masks.
Right swipes dispatch
the shared row action. `exercise-layers.ps1 -Relationships` checks these interactions
and their undo history in both themes, alongside the existing pickup fixture variants.
Row padding belongs to the moving content; revealed actions fill the row height
and reach its right edge. `exercise-layer-drawer.ps1` checks those bounds and
equal 72-DIP action widths in the compact drawer. Selection moves with the row;
Lock and Duplicate blend text into an opaque window background. Pixel checks
keep those action fills independent of selection and panel transparency.
Pickup fixtures reveal virtualized sources with native scrolling. Across an
intentional selection, compare the project checkpoint and dirty state;
document revisions also advance for navigation.
For empty viewport space, measure the allocation between the native Layers
header and footer: transparent scroll peers report occupied content bounds.
When a drawer clips the footer, use those occupied bounds for row hit checks.
Check vertical row motion for unchanged authored state. Test reverse-close with
Delete explicitly visible, reopening it if native cancellation closed the strip.
Maximize for the child-control and group-drop tail so the mask context bar leaves
the row targets exposed; keep the earlier compact, floating and drawer checks.
`exercise-layers.ps1 -Paper` checks the missing-Paper row's placement, native
activation, scrolling and undo history in both themes; `-PaperLight` runs the
light-theme scrolling, history and empty-stack cases. Both variants also walk
Choose Texture from the Paper row and from Properties: its width against Edit
Color, three fully visible rows, keyboard choice, Cancel, Use Texture with one
Undo, Rename and Delete
in the open dialog, a seeded library paper scrolled into view and a refused
Use Texture after the drawing changes. They then drag Properties' Scale slider
and require nonuniform relief to change more than a third of the samples in
the same page crop while it is held, exact baseline pixels after the fade
and one Undo. With Texture visibility at 0 they require Light and Shows through
paint to be disabled, then check the Light dial's size and alignment with the
slider rows, its dot, mouse, pen and touch drags with one Undo each, Escape,
arrow steps and double-click reset.
The Layers fixture includes popup windows owned by its review process when
locating Choose Texture and its Rename prompt.
The Paper fixtures maximize their owned window and wait for two full rows in
the Layers viewport before comparing row heights; UIA bounds are clipped to
that viewport. Reacquire the current Layers list with bounded polling after
selection changes, since WinUI can replace its automation peer.
Read the pending choice with one selected-button query scoped to the active
texture grid. Repeated full-window searches can exhaust the original polling
deadline after the native selection has already changed.
Reveal a texture tile through its native scroll pattern or keyboard focus before
requiring arranged bounds for its context-menu pointer click.
Compare `edit-color-dialog-frame` and `texture-dialog-frame`, since focusing a
selected library texture can scroll its title out of view. The outer dialog
peer includes the popup backdrop. Check the width and visible rows again on
that library reopen.
Run `exercise-layer-pickup.ps1 -Executable <path> -Device mouse -Theme dark`;
repeat with `-Device pen` and `-Device touch`, each with `-Theme light` too.
The UIA helper resolves the current `layer-list` for row searches; explicit
scopes, names, root controls and owned popup searches keep their original scope.
Row activation after dragging uses a fresh pointer tap. Pointer targeting uses
`Control -Arranged` to retain the native peer while checking its live bounds;
repeated tree searches must not consume the stability deadline. Keep the original
visible bounds, stability, history and deadline checks.
The pickup fixture rejects locked body and handle motion without changing layer
order or adding history: one Undo unlocks the source, and Redo locks it again.
It also checks that successive native Escape presses close a submenu and its
parent without changing layer state or history.
The default layers fixture closes with a focused opacity draft, reopens the same
private profile, and verifies the saved draft and its clean Undo checkpoint.
`exercise-layer-drawer.ps1 -Theme dark|light` checks the combined header drawer,
the six-row floor and seventh-row growth, compact Properties, edge-to-edge rows
with the eye, alpha lock, New layer and Layer properties slots six points in,
26-point footer slots two points apart,
Lock/Unlock, Duplicate, Delete and Undo. When Paper is absent, the Layers list
presents the shared Add paper action after its last row. Its native button keeps
row sizing, pointer cancellation and keyboard activation. Adding Paper opens its
properties; shared Rust owns creation, bottom placement and history. The VM
fixture is `layer-drawer`. The overflow check retains arranged controls and reads
their live bounds and placement within its existing stability deadline.

`exercise-drawer-layout.ps1` checks Sketch Color against the shared color layout's
height-for-width and wheel geometry, Layers drawer containment, header source and
bridge continuity, and drawer removal on a workspace-context change. It also checks
that a fitting Navigator has no scroll range while Brushes and Tool Settings keep
their genuine overflow scrolling, including a fractional-DIP window size. Run the
focused journey in both themes on a private desktop:

```powershell
foreach ($theme in 'light','dark') {
    ./apps/layer-windows/scripts/test-desktop.ps1 -Script apps/layer-windows/scripts/exercise-drawer-layout.ps1 -Parameters @{Executable='artifacts/windows/build/CapyCanvas.exe';Theme=$theme}
}
```

Each imported image has one ordinary Object layer row. Its shared layer ID owns
the thumbnail, selection, visibility, masks, locks, context menu and reordering.
`exercise-image-rows.ps1` checks previews, selection, visibility, reordering and
cancellation, then ordinary Duplicate, structured Copy/Cut/Paste, Rasterize Layer,
history and Unicode save/reopen. Run each `-Device mouse|pen|touch`
variant with `-Theme dark` and `-Theme light`.
The [shared renderer tests](../../crates/layer-render-wgpu/tests/object_layer_consumers.rs)
check conversion pixels with at most one 8-bit display level of error per channel;
Undo and Redo require exact pixels. Screen samples retain the arranged canvas
control and verify its live bounds and camera before each comparison. Repeated
UI Automation tree searches must not consume the pixel-stability deadline. The
image-row journey hides the optional bar during artwork comparisons so its
blurred background stays outside the samples. It restores the bar to activate
Rotate, then hides it again; Undo and Redo still require exact pixels.

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

Signing, installed update and uninstall, and installs on a clean machine have not
been verified.

## Test

Run `exercise-stroke-recording.ps1 -Executable <path> -Theme dark` and `-Theme light`
through `test-desktop.ps1`. Each existing recording journey captures the docked
Start, Stop and Save control states and checks their allocated height against the
owned drawing's shared panel metric and window DPI. It also checks CAPYPEN4/gzip,
cancel retaining the recording, delivery releasing it and a zero exit. Omitting
`-Theme` preserves the default appearance. These checks do not cover floating or
drawer placement, or measure background and border colors.

Service shutdown joins every independent worker even when recovery shutdown
fails. Successful render-owner `capy_finish_services` makes stored callback
contexts quiescent. Callers retain callback code and context until that success;
an owner error requires retrying shutdown on the render thread.
Window-free document and recovery tests cover failed-worker shutdown and repeat
cleanup.

Windows PowerShell 5.1 reads scripts without an encoding marker using the system
code page. Scripts containing Unicode captions or test input use a UTF-8 BOM.
`tests/script-encoding.ps1` checks encoding and parsing before the native build.
It also checks that capture manifests, models, layout and localized caption
evidence retain Unicode in UTF-8 JSON without a BOM.
Windowless checks execute the fixtures' PNG, SHA-256 and nested JSON expressions
on known bytes and recursive clipboard data using Windows PowerShell 5.1 APIs.
Sort arrays of hashtables with an explicit key expression, such as
`Sort-Object { $_.difference }`, so selection agrees in Windows PowerShell 5.1
and PowerShell 7.

`tests/renderer-health.ps1` checks the shared fixture assertion against native
renderer failures and foreign window ownership in a fresh same-version PowerShell
process, keeping its mock native API out of later fixtures. A failing child stops
the checks. The window exercise helper checks
the owned visible `canvas-status` before Close; the layout journey also checks it
before capturing Diagnostics and reporting success. A native renderer failure
fails the fixture even when the window answers and the process exits normally.
Recovery fixtures keep their explicit recovery and save-only status assertions;
the shared check does not reject those messages or provide a blanket exception.

Run `pwsh -NoProfile -File ./apps/layer-windows/tests/model-snapshot-reader.ps1`
to check the UI fixture model reader during concurrent atomic snapshot publication.
This windowless test needs no GPU and writes its results under `artifacts/windows`.
It validates 100 reads and zero writer failures during concurrent publication
without a throughput target.
Reports retain read durations and exception details; the reader closes each file
before parsing its JSON. Release checks run script regressions before compiling
the native client and retain their failure reports.
`tests/presented-frame-wait.ps1` checks fourteen presentation-barrier cases,
including stale acknowledgements, retired reservations, displayed frames,
process ownership and sampling only after the display acknowledgement.
Trace publication uses `FileRenameInfoEx` with replace-existing and POSIX semantics
so readers retain complete old snapshots while subsequent opens see the replacement.
The native input regression holds both old and pending handles open while publishing.

Fluent catalogs and shared cursor SVGs use LF checkouts through `.gitattributes`.
Their parser structure and byte hashes must stay identical across hosts.
Original dependency notices in `tools/build/licenses/*.txt` preserve their exact
bytes without checkout conversion.

`exercise-artwork-recovery.ps1 -Executable <path> -Theme dark` checks automatic
crash and orderly restarts, tab membership, the active drawing, Undo/Redo,
cancelled and discarded drawing closes, missing or changed saved originals,
and idle write coalescing. It retains the owned drawing window at each restart
and captures the composed canvas after canvas and brush readiness. Review the
reopened artwork as well as tab and history state. Repeat with `-Theme light`;
the VM fixture runner selects both variants. Window-free `recovery::tests` also
check saved failure details, stale restore completions, identity remapping,
publication failures and cleanup warnings after committed checkpoints.
`tests/recovery-fixture.ps1 -Output <fresh-directory>` checks failed-source byte
preservation and relative notice paths without a window, including PowerShell 5.
`tests/ink-color.ps1` checks document-fixture color comparisons with encoded RGB
and optional preserved linear channels; black need not store `linear_rgb`.
`tests/document-invoke.ps1` checks a paper drawer action whose UIA peer retires
after committing: observe the shared acknowledgement without repeating the action.
Document fixtures activate the owner before focusing a native text field, wait
for focus before Enter, and edit controlled ink through the visible Color panel.
Focused GPU and writeback checks maximize the owned workspace first. File menu
actions use the focused native menu item and Enter.
Use `exercise-documents.ps1 -Journey gpu-recovery` for just the imported-image
checkpoint, two GPU replacements, queued/active pen input, exact exported pixels
and retained history. Native writeback initializes its pointer driver before
parking or injecting input, and disposes it after each checked operation.
Shared filesystem and SQLite calls resolve existing paths before native publication,
including when private storage or backup directories exceed the legacy Windows path
length. Window-free core and native workspace regressions check long-path recovery
details, atomic cancellation, database WAL data and exact backup contents without
requiring the system-wide long-path setting.

Drawer and expansion query lifecycle tests live in `layer-host`. Windows
workspace tests cover native snapshot insets and the JSON/CPU packet boundary.
Native glass geometry uses `NativeHost::set_glass`: a changed layout requests a
redraw while retaining the UI model, including when the region count changes.
Windows publishes counts and GPU panel acknowledgements as compact metadata;
camera and search updates preserve those acknowledgements. Deferred glass
collection coalesces requests queued before the next traversal. Identical or
invalid layouts preserve the publication cache. The window-free
`glass_topology_retains_layout_publications_and_document_state` regression checks
this contract; run `exercise-canvas-bar.ps1` with `-Theme dark` and `-Theme light`
to check the selection bar's native glass registration and menu actions.
The same journey checks Adjust attaching Curves to the editing layer with the
selection as its mask, then verifies that one Undo restores the layer, selection
and composed pixels.
Loading a mask produces integer pixel bounds within one document pixel of the
original contour; Adjust Undo must still restore the exact original contour.
It also checks Keep proportions across Scale and Distort,
and switches between the wide mode row and narrow mode menu.
The full journey also attaches selection-masked Invert, merges visible layers,
checks exact interior and exterior pixels through Undo and Redo, and forces a
fresh baked-paint redraw by hiding and showing it. Run both themes after changing
native private-bake preparation; layer counts alone do not qualify capture pixels.
The fixture prepares two-dimensional ink and checks that its transform-body
target clears the shared handle and pivot hit regions before injecting a finger
drag. Thin ink can leave no unambiguous body target at Fit.
Use `-Theme tail-dark` or `tail-light` for the remaining bar controls, masked bake
and transform journeys after the separate Invert and glass checks have passed.

Use `exercise-photo-tools.ps1 -Theme transform-dark` and `transform-light` for
the remaining transform and layer operations: Warp Grid and Escape, Merge Down,
Stamp Visible, Flatten confirmation, and the Distort footprint and saved-pixel
checks. These scopes start with fresh ink and reuse the full journey's assertions.
Use `-Theme distort-bilinear-dark` or `distort-bicubic-dark` for one filter's
rollback, Apply, paint, history and reopen checks. Each has a matching light
variant and retains the full exact-pixel and checkpoint assertions.

| Where | Checks |
| --- | --- |
| Linux or Windows | `cargo test --locked -p layer-host -p layer-ui -p layer-workspace -p layer-windows --lib` and `cargo clippy --locked -p layer-windows --all-targets -- -D warnings` |
| Windows, no GPU needed | `./apps/layer-windows/scripts/test-without-gpu.ps1`: native-architecture Release build, native input queue tests, architecture and mixed-payload regressions, serial Rust unit and `d3d12_` document tests on the Microsoft software adapter. Pass `-Architecture x64` or `-Architecture arm64` to choose explicitly, and `-Configuration Debug` for unoptimized builds and tests. On Linux: `tools/windows-vm/windows-vm.py check`. |
| Windows desktop | The `exercise-*.ps1` fixtures for the changed area. On Linux: `tools/windows-vm/windows-vm.py fixtures <name>`. |
| Hardware D3D12 | `cargo test --locked -p layer-windows --lib d3d12_ -- --ignored --test-threads=1`, which adds the HDR and color round-trip tests; [GPU reconstruction](#gpu-reconstruction); renderer tests with `LAYER_GPU_INDEX`; performance. |

`exercise-header.ps1 -Journey zen` checks retained Capy button and glyph bounds
across workspaces and sizes in both themes. It also checks the docked toolbar's
Show in Zen mode toggle, retained native controls, edge touch swipe peek and
outside-touch dismissal. It changes themes while in Zen, checks that Escape
closes command search while keeping Zen, then that the next Escape leaves Zen.
Use `-Journey zen-workspace` for only the toolbar and edge touch checks in both
themes, including switches through Sketch, Photo and Paint with arranged native
Histogram controls in Photo.

D3D12-specific fixtures construct an instance with `Backends::DX12`; a generic
`PRIMARY` headless instance can select Vulkan. The software-adapter gate permits
CPU adapters but does not force every ordinary test onto WARP.

The `software-adapter-tests` feature gives GPU readbacks and their raster
consumers one shared 180-second budget, because WARP's first Levels Auto
statistics dispatch takes about two minutes; normal hardware builds retain 30 seconds.
The large-canvas fixture keeps its 60-second overall deadline and exact pose and
pixel checks. Hardware qualification uses normal builds.

Native x64 tests use Windows' system WARP. The test script checks its architecture
and reports its path, version and SHA-256 without copying or removing the DLL.
It refuses an app-local WARP DLL beside Cargo test executables. NuGet x64 WARP
1.0.20 and 1.0.21 can stall the hosted float32 readback tests even in isolated
processes; the system runtime completes the same cases without changing their
deadlines or assertions.

For ARM64, the test script restores `Microsoft.Direct3D.WARP` 1.0.21 from the separate
[`tests/packages.config`](../../apps/layer-windows/tests/packages.config) into
`artifacts/windows/test-packages`. It stages only the ARM64
`d3d10warp.dll` beside Cargo test executables, using Cargo's resolved target
directory, and removes its own copy on exit. System ARM64 WARP 10.0.26100.8972
can fail fast in its pixel JIT during clipboard readbacks; the pinned runtime
passes that case. Microsoft's [WARP package license](https://www.nuget.org/packages/Microsoft.Direct3D.WARP/1.0.21)
permits internal Windows development and testing only. Keep its DLL and symbols
out of application builds, distribution packages and public crash dumps.

Failed rendering waits report pending bounds, capture and edit jobs alongside the
shared notice, so completed work can be distinguished from a stalled operation.

`scripts/test-input.ps1` compiles the C++ input queue and composition-key tests
with `cl` and needs a Visual Studio developer shell. Native numeric contacts in
the header and workspace restore their checkpoints when the window loses focus
or becomes blocked by a dialog. The cancellation registry retires every current
owner before controls are disabled; ordinary pointer handoffs stay scoped to
their pointer. `TextInput.cpp` observes the
focused TextBox's public TSF edit notifications so overlapping syllables and a
composition ending before routed KeyDown keep the same native key owner. The
key tests include nested message timestamps, release before the confirming key,
unrelated releases and clock wraparound. Real
IME checks must distinguish candidate Enter/Escape from the next ordinary press,
and include selected names, numeric refusal, pointer confirmation and focus
changes. Workspace manager uses the composition guard for its inline name field,
parent list, row F2 action, dialog Escape and primary confirmation.
The `d3d12_` tests that write app storage need `CAPY_STORAGE_DIR` set
to a fresh absolute folder. In Debug builds the HDR and native color tests need
`RUST_MIN_STACK=8388608`, the native document worker's stack size.
The test script adds its build folder to the child process DLL search path so
D3D12 uses the same pinned DXC as the app. Before direct Cargo GPU tests, prepend
that folder to `$env:PATH`, for example
`$env:PATH=(Resolve-Path artifacts/windows/Release).Path+';'+$env:PATH`.

For native ARM64 checks, use the ARM64 build folder and explicit Cargo target,
even when the installed Rust host is x64:

```powershell
$env:PATH=(Resolve-Path artifacts/windows/arm64/Release).Path+';'+$env:PATH
$env:CAPY_STORAGE_DIR=Join-Path (Get-Location) ('artifacts/windows/test-storage/'+[Guid]::NewGuid().ToString('N'))
cargo test --locked --release --target aarch64-pc-windows-msvc -p layer-windows --lib d3d12_ -- --ignored --test-threads=1
```

Use a fresh storage folder for each run. Hardware tests and performance runs keep
the software-adapter feature and overrides disabled. The same `--target` applies
to shared tests, Clippy and renderer examples. Run native UI fixtures through
`scripts/test-desktop.ps1` with the ARM64 executable and private test profiles.

The renderer's ignored
`dry_material::tests::fine_aggregate_pencil_color_pipeline_prepares_on_hardware`
checks the fine-tip, aggregate Pencil compute pipeline, elliptical and rounded
endpoint and turn completion, and Multiply color and coverage pipelines through
the native DXC compiler. Each case retains its own error scopes and reports its
contact flags, operation, entry point and blend code. Place the app's pinned
`dxcompiler.dll` beside the renderer test
executable before running it. Set `$env:WGPU_ADAPTER_NAME` to a distinctive part
of your hardware adapter's name. The test requires hardware and rejects software
adapters; a successful WGSL validation alone does not cover pipeline creation.

The zoom menu grows to fit its shared captions and shortcut hints. Its readout
exposes localized open/closed accessibility status after the native popup events.
`exercise-navigation-controls.ps1` checks the Zoom Click, Drag and Direction
text segments, Center clicked point, separate Zoom toolbar placement, canvas
wheel/touch preferences and settings persistence. Run
`fixtures navigation-controls navigation-controls:light` through the VM runner
for both themes. The native Canvas switches for Quick pinch to fit and Reverse
pinch to restore view start enabled; this fixture changes and independently
resets them, then checks their saved values after closing.

`exercise-zoom-readout.ps1` checks each caption's full text and visible character
rectangles, containment and shortcut separation, and waits for popup acknowledgment.
Review the raw captures for complete glyphs; positive character bounds can still
include partial glyphs. It checks wheel navigation
while the middle or right mouse button remains held, including horizontal, Shift
and Ctrl wheel input, zoom locks and continuing the same pan after scrolling.
Modifier keys remain held until the app acknowledges the input. Before painting,
the fixture focuses the canvas and closes the tool drawer with Escape. It waits
for visible ink before scrolling and after each continuation of the same contact,
including while Shift or Ctrl remains held, then checks the unchanged camera and
one-step stroke history. Run both themes and review the captures.

Use `exercise-zoom-readout.ps1 -Executable <path> -Navigation -Device mouse`
for navigation tools, held keys, view commands, tool double presses and drawing
shortcuts. Repeat with `-Device pen` and `-Device touch`, each with `-Theme dark`
and `-Theme light`. The mouse cases capture the active Windows cursor using its
handle and hotspot over the screen pixels; review those images separately from
the shared cursor state. The mouse journey also checks hidden brush cursors
after held navigation and command-search dismissal without moving the pointer.
Synthetic input does not qualify physical pen, touch or
trackpad behavior. Run the ordinary zoom journey separately.

`exercise-clipboard.ps1 -Executable <path> -Theme dark` checks clipboard formats,
Copy/Cut history and layer structure, shown-position and cursor placement,
Paste as New Image from pixels, image objects and external file batches, startup
Paste and ordinary Paste into a saved drawing. Selected-region checks preserve
separate layers through Copy, Cut, Paste and Undo. Focused-mask checks keep
internal and external paste in the mask without changing paint content or adding
layers. New-image checks verify the save prompt for authored layers, save the
result and return to an unchanged source
tab. A second native window pastes the retained layer, copies it and closes;
the original window then pastes that copy as a new image and compares its
authored properties and raster payloads with the original saved package.
Cursor placement waits for the shared cursor to appear in the composed
canvas before sending the shortcut, then checks the exact saved image affine.
Ctrl+Insert, Shift+Delete and Shift+Insert exercise artwork Copy, Cut and Paste
and native text editing. Review the open Edit-menu capture for the longer
shortcut hints. Repeat with `-Theme light`.

`exercise-color-editor.ps1 -Executable <path> -Theme dark` checks color editing,
native clipboard ownership and the Paper fill. It records the immediate fill,
then waits for the same layer thumbnail to be ready before capturing it again.
Numeric input cases check canceled touch scrubs and outside mouse, pen and touch
releases followed by native activation, plus stationary updates with modifier
keys during a held scrub.
Review the thumbnail color in both themes; readiness alone does not prove its
visible content. Check the split footer preview against its actual shared frame
and the footer buttons; fractional display scaling rounds border pixels.
Repeat with `-Theme light`.

`exercise-idle-brush.ps1 -Executable <path> -Theme dark` checks painting as soon
as the selected brush is ready, later Wet Round, Smudge, Liquify and G-Pen
contacts with pixel and Undo/Redo checks, and closing after first readiness.
It records whether shader compilation is still pending at close; inspect that
observation before claiming coverage of closing during compilation. Run it on
an activated, owned private desktop with `CAPY_PRIVATE_DESKTOP` set
to that desktop's name; it verifies both the thread and active input desktop
before launching the app or sending input. Repeat with `-Theme light`. Its
180-second wait checks completion of requested shader preparation, not preparation
of every brush. Later selections exercise first-use preparation and exact history;
these checks do not replace startup deadlines or performance measurements.
Use `-PresetIds <ids>` for selected preset journeys with the same pixel and
history assertions. That selection omits the separate Liquify Adjust/Reset tail;
the results list the presets actually checked. Allocate the outer desktop-runner
timeout for the selected cases without changing their functional deadlines.

The render worker's existing service poll advances optional paper lighting while
the canvas is idle. A completed lighting transition requests one presentation;
pending compilation does not keep frames running. The headless hardware test
`layer-host::tests::idle_paper_lighting_wakes_one_presentation_without_input`
checks the initial plain paper, the idle lighting transition, unchanged authored
data and the return to a quiet canvas.

`exercise-canvas-editing.ps1 -Executable <path> -Theme dark` checks raster selection
and transform previews with mouse and pen. Selecting Pen accepts the nonzero
rotation preview and waits for the brush to be selected and ready; scaling and
translation retain explicit Apply. Exact exported PNG comparisons check the
changed result and one-step Undo, Redo and restoration. The fixture uses an
isolated English profile and the owned drawing window. Repeat with `-Theme light`
and review the captures, including the transform controls after changing theme.

Use `-Journey smoke` for mouse selection, move preview/cancel and clean Undo;
`-Journey preview` for mouse and pen move/scale/rotation cancellation without
PNG export, or `-Journey controls` for anchors, typed position, snapping, Transform Again
and workspace controls. Each reuses the full journey's assertions and cleanup.
Add `-Theme light` or use the matching `smoke-light`, `preview-light` or
`controls-light` journey. The default remains `full`.

Open header menus follow shared command state while document operations finish.
Their native items retain focus and identity; unchanged menu models skip updates.

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
and Photo selection tools, the single-preset Eraser settings, and a drawing group
added to the header through customization, then narrows the window until that
group overflows, with injected mouse, pen and touch. Repeat with `-Theme light`.
It checks Eraser's absent group marker and settings-only drawer, Marker under Pen,
Spray under Airbrush, all 65 customization presets and native search for Marker,
Spray and Dual Texture. The vector-layer fixture retains the three vector
Eraser modes.
The VM fixture names are `tool-variations` and `tool-variations:light`.
`tool-variations:sketch` and `tool-variations:sketch-light` isolate all eight
default Sketch header drawers, native pen pixels, retained anchors and exact
Undo/Redo pixels in each theme.

Properties sliders use the shared [compact numeric layout](../ui/numeric-controls.md),
with the track beneath the caption and room for the value and its editor.
Long captions trim and relabel without replacing the control.
Compact inline fields grow to fit their formatted value and keep that width
when a later value is shorter. Review the fractional Layers readout in the
fine-scrub captures.
After editing a toolbar number, pressing another native control keeps focus
and pointer capture on that control.
`exercise-effects.ps1 -Executable <path> -PropertyLayout -Theme dark` checks
opacity and filter sliders, typed edits, mouse/pen/touch value drags that snap
like the slider, a mouse drag that continues ten times slower while Shift is
held, release, Escape while held, label reset and one Undo per drag in both
themes.
It also checks narrow Russian captions and a return to wider English controls
with retained values and identity.
Use `-VisibilityOnly` for active curve preview, hiding Properties through its
visible Window menu item, shared hide acknowledgement and exact curve history.
The ordinary full effects journey also runs the value drags. A secondary mouse
drag on the readout must preserve the shared
value, editor and focus. Review the composed captures.

Properties, Tool Settings and Tool Options share one gradient editor
(`GradientView.cpp`); a worker thread rasterizes its dithered preview through the
shared `color_ui` gradient image request. `exercise-effects.ps1` drags Gradient Map
stops with mouse, touch and pen and checks Escape, arrow keys, Delete,
interpolation, Reverse and the current-color bucket, each as one Undo step.
`exercise-tools.ps1` and `exercise-toolbar-components.ps1` cover the Gradient
tool's editor in Tool Settings and the Tool Options popup.
Scrolling panel presenters publish `panel-scroll-<panelId>` when their native
template is created or replaced. Tool fixtures choose scroll direction from the
control's native, unclipped bounds. UI Automation must still prove full visibility,
height and stable bounds.

Pen pressure and tilt reuse the curve renderer and `ResponseCalibration`
modeless utility frame. Shared Rust owns the response drafts, axes, device
binding and Apply/Cancel/Reset. Native controls use 16px gutters, square plots,
36px actions and independent tilt segments, and a 72px direction preview. The
opaque utility sits in the top overlay above the native header, expanded panels
and drawers. Its visible bounds join the native caption passthrough regions so
header overlap cannot start OS window dragging. Initial exposure waits for
the shared owner to accept the native measured extent. Graph drawing clips to its native input bounds, including
edge handles. Keyboard capture forwards every press in a combined key message's
repeat count; Rust owns the step and gesture history.
`exercise-pressure.ps1 -Executable <path> -Theme dark`
checks opening from Preferences, curve contacts, drag-out removal, touch and pen
cancellation of curve and panel drags, live pressure,
canvas painting and calibration transactions. Repeat with `-Theme light`;
the VM fixture names are `pressure` and `pressure:light`. Physical pen and frame
performance require Windows hardware.

`PenInput.h` registers native pen metadata through the existing `pen_input`
owner action before numeric canvas samples. `PenDeviceInterop` supplies the
physical PenId when available; a product-name key, or a shared Windows pen key,
is the fallback when Windows provides no unique identity. Runtime pointer IDs
and native HANDLE values are not saved as physical identities. The canvas
registration cache retains only the latest descriptor, re-registers returned
runtime IDs, and resets on selected drawing and document epoch changes; the
native input tests exercised by `test-without-gpu.ps1` cover both transitions.
Utility hover and contact send calibration-only samples through the same owner,
including when a child control handles the routed event. The host forwards
native X/Y plane angles in radians and an identity coordinate frame; automatic
rotation correction is not inferred from a mapped monitor or digitizer display
orientation. Physical pen identity and rotated display behavior need hardware
verification.

`exercise-tilt.ps1 -Executable <path> -Theme dark` checks cold first Lasso contact,
preserved utility placement, live circle, header and blank body sensing without canvas or
document-history changes, all eight independent axis combinations, mouse/touch/pen
curve edits, cancellation, Apply/Reset and persisted profiles after restart.
Pen-contact cases maximize the owned drawing and use Fit after arranging the
workspace so a full paper sample remains visible beside the utility. The
separate short-window case checks scrolling
and the footer at reduced height.
Repeat with `-Theme light`, or run
`tools/windows-vm/windows-vm.py fixtures tilt pressure` for both themes on WARP.
Synthetic native input verifies routing and editor behavior, not physical sensors
or frame performance.

The same tilt journey checks cold curve, axis and Reset drafts; a first pen
contact directly on the graph; and first pen sensing while a separate native
touch contact retains the curve. Cancellation restores the prior edit intent,
editor epoch, placement and exact artwork sample. It also checks live
pressure/tilt replacement, matching geometry, native clicks and opaque pixels
above an expanded column and connector, title dragging over the native header,
and body scrolling with an accessible footer in a short window. It reveals
body controls through their native scroll provider before input
and checks dragged placement against the shared viewport limits in both logical
and native coordinates. It never scrolls while a contact holds the curve.
Use the freshly verified scroll ancestor for `Control -Arranged -Within`
when re-querying a body target. Keep `-NoPopups`, runtime identity, process
ownership, finite viewport containment and stable-bounds acknowledgements.
The guarded drivers support concurrent touch and pen; concurrent mouse and pen is
not exercised. Repeat the complete journey in both themes.

Histogram, Waveform and the input statistics in Levels and Curves
(`ScopesView.cpp`) draw the shared plots. The snapshot's `windows_scopes` revision
changes when a view's counts, channel, Log counts or palette change; the views
then send one `scopes` workspace query, which returns the plots and a
premultiplied BGRA Waveform scaled to the graph on the render thread. Properties
presents the shared action row (sampling menu, Auto, targeted adjustment), the
Color Lookup selector and Import LUT, whose `.cube` file is read and parsed on the
document worker. `exercise-scopes.ps1 -Executable <path> -Theme dark` walks the
Photo scopes, Levels, Curves and Color Lookup with mouse, pen and touch; the VM
fixture names are `scopes` and `scopes:light`. It waits for Photo's first
Histogram counts before using the canvas, because the render worker answers menus
and commands only between GPU frames and those counts are the first GPU job
Photo starts; on the software adapter that job sometimes never completes, and the
fixture then fails before Fit. Auto waits up to three minutes on the software
adapter and reports the shared notice when Auto fails. Scope, filter, layer, document,
paper and gradient previews use
`BitmapUpload`: one active upload, one latest pending request and two reusable
software bitmaps per context. Workers copy pixels and convert straight RGBA to
premultiplied BGRA. The UI submits `SoftwareBitmapSource.SetBitmapAsync` and
resumes after completion without blocking the dispatcher. Document,
setting and extent changes retire the source and its buffers. Retired buffers
are not reused while native readers may still hold them. A completed frame from
the same context may appear while a newer frame is pending. Layer thumbnails
retire the source for each new revision so retained WinUI fills display the new
pixels. `exercise-color-editor.ps1` checks the Paper fill in the composed
thumbnail outside its type badge; Ready alone does not check pixel content.
Log counts has its own row above the precision and clipping controls,
as on GTK and Web. The `layout-dark` and `layout-light` theme
variants check both scope panels in every registered language at 1100 and 1500
pixels wide.

Painting cursor shapes come from shared Rust and the shared GPU presenter. Tool
uses the active tool's icon, aligned to its working point; Tool and brush size
adds the current brush outline for brush tools. Windows presents the shared
cursor choices directly in Pen & Input preferences.
Idle pointer movement over native chrome retires the canvas cursor through the
independent input dispatcher, including when the native control handles movement.
`exercise-cursors.ps1 -Executable <path> -Theme dark` selects both choices,
checks persisted settings, and compares composed mouse and pen hover pixels for
six tools without changing the drawing. Repeat with `-Theme light`, or use
`tools/windows-vm/windows-vm.py fixtures cursors` for both themes on WARP.

Navigation follows the shared [drawing and navigation](../ui/shared-ui.md#drawing-and-navigation)
commands and input rules. Windows consumes `navigation_cursor` for native pan,
zoom and rotation cursors; zoom uses embedded cursor resources at multiple pixel
sizes. Cursor changes update independent canvas input and the XAML island
source when the pointer actually hits the canvas, including while it is still.
The island update excludes other windows, native controls, menus and dialogs.
Panel and header tool buttons forward double presses with the current
shared `resolved_control`. Drawing-cycle keys and Ctrl+Shift+A for Drawings use
customizable shared commands and respect native text input. The document service
completes `AdjacentDrawing` before activating the next tab,
so the originating drawing can be parked without a pending request.
Window activation refreshes stored workspace choices while a drawing is parked;
workspace operations use the shared controller's validation. Drawing-cycle
fixtures also require the native canvas status to clear after activation.

`exercise-ime.ps1 -Executable <path> -Theme dark` exercises the installed
Microsoft Japanese IME on the interactive test desktop. Enable Japanese input
in the private test user first; the fixture requires actual Hiragana preedit
and a converted candidate, with 100 ms between key presses, before checking
Enter, Escape, selected layer names,
numeric refusal, pointer confirmation and focus changes. Repeat with `-Theme
light`, or select `fixtures ime` in the VM runner. Captures include the private
desktop so the IME candidate window remains visible in the evidence.

### UI fixtures

Run `gh workflow run windows-readiness.yml -f source_ref=<commit-sha>` for native
x64 WARP fixtures on hosted Windows VMs.
One build supplies the runtime to up to eight parallel fixture jobs. The
`fixtures` input accepts a JSON array of registered cases, for example
`["canvas-bar:default","canvas-bar:light"]`; the default covers the
priority vector, brush, touch, bubble and Zen journeys.
The VM workflow selects a supported temporary display up to 2560 by 1600,
with at least 1920 by 1080. Its `runtime_run_id`
input can reuse a completed run's runtime when `source_ref` matches that build.
The manual workflow checks functional behavior; it does not qualify hardware
rendering or performance targets. Idle-brush journeys retain their default
readiness deadlines; other VM fixtures use the runner's normal wait scale.
For a document crash, select a narrow `documents` journey and set
`capture_crashes=true`. CDB attaches to each owned private-profile window and
records first-chance and unhandled access-violation stacks. Uploaded evidence
excludes memory dumps and downloaded symbols. Do not debug performance runs.
Run `gh workflow run release-checks.yml -f windows-only=true -f source_ref=<commit-sha>`
to run the existing x64 and ARM64 release build, test and Clippy jobs directly.

The menu automation helper matches current UIA properties after walking the
owned window; WinUI can omit visible controls from property-filtered queries.
Each polling attempt shares one tree snapshot across the visible and compact
menu routes, so an expensive traversal cannot consume the wait on fallback IDs.
Expired UIA elements are skipped; invalidated tree reads retry within the same
deadline.
The input suite checks delayed publication, disappearing submenu routes and
current-property discovery through this complete helper.
Workspace setup uses Undo Layout Change (Ctrl+Alt+Z in the default keymap).

Each `apps/layer-windows/scripts/exercise-*.ps1` drives the real app through UI
Automation and guarded OS mouse, pen and touch input. Fixtures that take
`-Executable` launch their own app with a disposable profile and close it; those
that take `-ProcessId` attach to an app you launched with `CAPY_TRACE_UI=1` and a
disposable profile (see [diagnosis](#diagnose-a-running-app)). Device-specific
fixtures take `-Device mouse|pen|touch`. Results, captures and profiles stay under
ignored `artifacts/windows`.

The `exercise-touch-gestures.ps1 -ThreeFinger` theme pair also runs the complete
docked Layers body; `-Simple` runs it on the simple Layers card. Keep their
effective seed and merge pixel/Undo/Redo checks without a duplicate default-body
pair. Use `-Journey held-history` for held Undo/Redo and cancellation recovery,
or `-Journey layers` for layer holds, opacity, swipes and inclusive merge history
without repeating canvas pinch checks. Run each needed scope in both themes.
CancelAll checks the next real Redo and exact recovery; it does not prove
absence after a cancellation acknowledgement. Two-finger row swipes leave
selection, opacity, Alpha Lock, history and camera unchanged at the pair boundary;
the next effective one-finger Alpha Lock and its Undo check exact recovery.
The full docked route also edits the two pinch preferences through native
Preferences and checks ordinary zoom in each disabled direction, enabled fitting
and unchanged authored history. Keep this in the existing `-ThreeFinger` theme
pair rather than adding another default-body pair. `-SlidersOnly` checks Dither
and Scatter's primary controls and effective strokes in each theme after its
independent global-slider persistence proof. It checks their logarithmic whole-pixel
Cell Size, retained overrides and reset, plus Scatter's Spread/Cell Size/Grit ring
order. Each saves the painted drawing, keeps it open for an app/session restart,
and checks restored Cell Size12, source, pixels and history.
`exercise-workspace-layout.ps1` checks Paint/Photo group allocation,
retained tab bounds and saved-tab reload. These functional scopes do not establish
OS Blur/drawing retirement, physical input or performance measurements.

Routine fixture captures use `inspect-window.ps1 -CaptureOnly` for the image,
window title and responsiveness. They retain separate model and layout evidence. Failure captures keep the full
control-name inventory. Run the inspector without that switch to request it directly.

`exercise-navigator.ps1 -Executable <path> -Theme dark` checks Navigator commands,
ten native mouse/pen/touch overview contacts, secondary-mouse rejection, live
preview pixels, exact Undo, retained controls, resizing and an internal theme
change. Contacts preserve document revision and modified state. Repeat with
`-Theme light` to exercise the contacts in both starting themes. It retains the
owned drawing window and trace directory for capture, input, resize and close.
The window exercise helper retries transient UI Automation discovery within its
bounded wait, keeping the same process, HWND and drawing-canvas ownership checks.

The delayed-popup regression uses real native cancellation events and fixture-owned
reply gates. Build it into a separate output, then run in both themes:

```powershell
./apps/layer-windows/scripts/build.ps1 -ControlFixture Popup `
  -OutputDirectory artifacts/windows/popup-review
./apps/layer-windows/scripts/test-desktop.ps1 `
  -Script ./apps/layer-windows/scripts/exercise-popup.ps1 `
  -Parameters @{
    Executable = (Resolve-Path ./artifacts/windows/popup-review/CapyCanvas.exe).Path
    Theme = 'dark'
  }
```

Repeat with `Theme = 'light'`. The fixture checks delayed replies after native
Escape, a handled press on an untagged control, window deactivation, source
unload, request replacement and window close. It also checks cancellation
registration before showing, cancellation between ShowAt and Opened, and the
visible flyout's lifetime. A background window cannot admit a source owned by
another window. A request begun inside a native routed press survives that press
and cancels on the next untagged native press. Normal
menu selection and held-release behavior remain in the native device matrix.
The same fixture checks that replacing hidden chrome retains its collapsed,
transparent, noninteractive state, then restores it when Zen ends. Decorative
replacements retain their own input exclusion.
The workspace phase starts a real TSF composition in an owned TextBox and
requires the production composition observer to acknowledge it. The fixture
retains a real `ITfCompositionSink` until composition ends and
releases the composition before deactivating its TSF client. It checks
inline Enter, Escape and F2, dialog Escape and primary confirmation, then the
next ordinary action. All 11 popup cases and five workspace cases must pass
within the existing 30-second process deadline. Candidate lists from actual
language services still need their own IME journeys.

Run native input fixtures on a private desktop with `test-desktop.ps1`; it
restores the original desktop and closes its own process tree on completion or
timeout. Its Windows PowerShell child uses the default module search path, so
the fixture can also be launched from PowerShell 7. For example, from the
repository root with local scripts allowed:

```powershell
./apps/layer-windows/scripts/test-desktop.ps1 `
  -Script ./apps/layer-windows/scripts/exercise-activation.ps1 `
  -Parameters @{
    Executable = (Resolve-Path ./artifacts/windows/Release/CapyCanvas.exe).Path
    Device = 'pen'
    Theme = 'dark'
  }
```

Run `exercise-activation.ps1` and `exercise-tab-pickup.ps1` with each device
(`mouse`, `pen`, `touch`) in both themes (`dark`, `light`). Activation checks use
native contacts, including small movement, retained pen hover, cancellation and
release outside a button. UI Automation sets up and observes state; invoking an
automation pattern does not prove that a pen or finger can activate a control.
Palette cases also check held-menu dragging, canceled reorders and Escape
dismissal after the native menu is visible, preserving color and swatch order.
They release at a new slot after acknowledging an earlier preview, then require
the final order and one-step Undo.
Run `exercise-toolbar-components.ps1` in both themes for compact numeric faces:
release pen and touch scrubs, then activate with mouse, keyboard and accessibility.
The journey also hides and restores inline tracks with Show Sliders and verifies
native pointer input after restoring them.
Toolbar choice faces use the shared `control_fill` RGBA overlay, refreshed with
the palette. Horizontal dropdowns fit their widest localized item, icon and arrow.
Use `-Journey numeric` and `-Journey numeric-light` for these track checks in both
themes without the gradient, Zoom and compact-face journeys.
Tool Options retain the complete numeric track and input host when changing slider
visibility, so compact controls keep their native pointer surface together.
The full `exercise-hdr.ps1` journey also switches paint slots during an acknowledged
arc contact and checks that its remaining input cannot edit the new slot.
Run `exercise-tab-drag.ps1` in both themes after changing
tab capture or preview behavior.
Run `exercise-switcher-drag.ps1` with each device in both themes for workspace
rows, including ordinary scrolling with `-Overflow`.
The middle-button cases first prove each exact point with an ordinary native tap
and a fresh row release, then restore the preview before middle-button motion.
Native window ownership, visibility and bounds checks remain required.
Pen and touch reveal Delete by swiping and check that an outside press still
activates its target. The mouse journey holds pin, Apply and row contacts across
both Rename acknowledgement orders. Its private SQLite gate rolls back within
four seconds; native target and ancestor identities, claims, preview and the next
ordinary click must survive. The gate opens only the fixture's own profile.
Inline-name saves keep the dialog's geometry stable while controls remain
interactive; loading and blocking saves retain their status row.
Run `exercise-hdr.ps1 -DrawingTabsOnly` in both themes for drawing tabs and their
compact list; this mode uses ordinary documents and does not test HDR. These
fixtures check native row activation and outside releases as well as held and
grip dragging. Small outside releases cross the middle tab's edge within the
native client area and must preserve the active drawing and tab order.
Use `exercise-layer-pickup.ps1 -SwipeOnly` for the swipe strip, canceled contacts,
grip handoff and exact Delete/Undo and reorder history. With no device or theme
argument it covers mouse, pen and touch in both themes. Use
`exercise-workspace-pickup.ps1 -ZenOnly` for hidden-control exclusion, retained
layout and restoration in both themes.
For short pointer contacts, capture bounds before Down and inspect UI Automation
after Up; a slow lookup during contact can start a hold. Await a fresh native
gesture generation and recognized cancellation before checking that a displaced
release inside an action button did nothing.

`exercise-documents.ps1 -Journey color-workflows` isolates automatic comparisons,
Properties Appearance and conversion into a separate unsaved drawing. Repeat
with `-Journey color-workflows-light`.
Run `exercise-documents.ps1 -Journey native-writeback` and then
`-Journey native-writeback-light` through the private desktop harness for native
Grayscale and Two-tone painting. Each mode and device uses one continuous mouse
or pen stroke across three tile columns, exact canvas pixels for one Undo/Redo,
native Save and fresh Open, retained mode and identical encoded tile ranges after
saving the reopened drawing. The fixture records its script and binary hashes;
pixel comparisons and strokes prepare the owned window bounds and camera once,
then verify the full projection afterward. Pixel polls and stroke points avoid
UI Automation and model discovery during sampling and pointer contact. Together,
these functional checks do not establish pointer cadence or performance targets.
`exercise-hdr.ps1 -ControlsOnly` isolates
HDR arc, intensity, proof and screen-status controls in both themes; choose
`-Depth F16` or `-Depth F32` explicitly. These focused selections reuse the full
journeys' assertions and disposable profiles. Software-adapter checks establish
functional behavior; HDR output and presentation timing require hardware.
The HDR geometry helper matches the reviewed executable and Rust DLL: native
Windows PowerShell and .NET Framework on ARM64, a compiled x64 helper on x64.
Both check the loaded CLR architecture before calling the current Rust geometry export.
HDR and bubble geometry readers use the bridge's owned text handles and checked
byte copies, with at most 8 MiB in each copy.
Use `manager:default` for the full dark manager journey and
`manager:workflow-light` for the light name, search and toolbar-description checks.
The full `manager:both` selection can use `--timeout-minutes 60` on the software
adapter. `localization:profiles-dark` and `localization:profiles-light` cover
profile-library search in English and Japanese without the document import/export
matrix.

Secondary-button motion must leave palette colors and row order unchanged; locked rows and
disabled grips must release ownership and allow fresh native selection.
Keep the fast Down/Move-inside/Move-outside/Up burst separate from an admitted
drag. The burst does not assume drag admission: release must restore Ready,
preserve selection and order, clear menus and contact ownership, and permit a
fresh native tap. The admitted-outside case first acknowledges Dragging inside
the strip, then checks detachment and unchanged order after release.
Run `exercise-shortcut-page.ps1` and `exercise-command-search.ps1` with
`-Theme dark` and `-Theme light` for native Escape routing, including canceling
shortcut recording while keeping its editor open.

Use `TrackPopup` in `NativeMenus.h` for immediate native flyouts. For asynchronous
menu queries, keep an `OwnedPopup` per view, begin its ticket before submitting
the query, and check the ticket before populating and showing the flyout. It
owns cancellation from the pending reply through native closure, including a
later press outside registered drag sources. Both helpers track popup counts in
their `WorkspaceData` registry; window cancellation must dismiss both header and
workspace registries. Tickets gate replies; menu command callbacks retain their
semantic document/target checks and do not require an active ticket.
Cancellation must cover opening menus: a flyout can become visible before
`Opened` arrives. Release admission after accepted `Closing`; waiting for the
later `Closed` notification can reject the next button's menu. Retain lifecycle
guards until `Closed`, and preserve admission when closing is canceled.
Use `ShortcutKeys` in `UiControls.h` for ancestor
shortcuts: non-Escape keys preview, while native editors and submenus handle
Escape before gesture and window cancellation. Shortcut capture retains raw keys.
Ordinary custom editors use `PointerGesture::PrimaryContact` before committing
drafts or capturing input. Native buttons keep their own Click handling. An
unclaimed reorder candidate must not take their capture before a hold or drag
is admitted. Use `PointerGesture::OnLeaving` to retire an uncaptured candidate
when it leaves its observed surface, preserving native child capture and Click.
Immediate custom drag surfaces capture on press; native Buttons keep their capture until
the drag is admitted.
Feed recognizers only from non-generated motion events; exits and scene changes
can replay older samples.
For scrubs with modifier keys, ignore repeated coordinates and read
`KeyModifiers` from the pointer event.
The canvas has an independent input source; an ancestor XAML observer alone
cannot guarantee delivery of an outside release. Gaps in the bubble overlay also
deliver canvas contacts through XAML. Both paths copy native samples and capture
their physical coordinates with the matching view revision before queueing;
the input thread assigns sequence numbers and tracks contacts. Wheel and held
picker positions likewise retain the scale from collection across a resize.
`CanvasContactGate` discards a contact that a modal dialog or chrome dismissal
claims until its release; hover never claims a pointer, and each later press
starts a fresh contact.
Escape and Control shortcuts
pressed during a canvas contact are counted from raw keyboard input on a
key-reader thread; the input thread holds later canvas samples until the UI
thread has queued each such key, for at most one second, so the key keeps its
place among the samples. `vector-edges` checks this by suspending the UI thread
mid-drag.

For switcher visibility and its context menu during title-bar customization, run
`tools/windows-vm/windows-vm.py fixtures switcher header:options header:options-light header:options-pen header:options-touch`.
The switcher fixture covers both themes and injected mouse, pen and touch input;
the focused header journeys also check placement cancellation and keyboard input.

- **Unlocked desktop, one at a time.** Fixtures need an unlocked interactive
  desktop and foreground input. Run them sequentially.
- **Resolve the drawing window.** Use `Owned-DrawingWindow` from `CapyUia.ps1` at
  startup and after reopening. It creates UI Automation roots from native window
  handles owned by the process and requires the drawing-canvas peer.
  An empty enumeration waits for window creation; only a reported native error
  aborts discovery. Clear Win32's last-error value before enumerating.
  Keep its root and HWND for input, sizing, captures and window close;
  `Process.MainWindowHandle` can select an input-indicator overlay in the same
  process. Startup-close checks resolve this window without
  waiting for brush readiness. Set `CapyTraceDirectory` when traces live outside the
  executable directory, and pass that directory as `-StateDirectory` to
  `exercise-window.ps1` for sizing and closing.
  Nested Preferences checks receive the same HWND through `-WindowHandle`.
  Pass `-StateDirectory` for every action when the app's working directory differs
  from its executable directory, so every action records the same owner.
  Keep the launched process's `Handle` before waiting for exit so its exit code
  remains available after Windows removes the process.
- **Wait for layout.** Before injecting input, wait for both the published state
  and the arranged control bounds, then check that the hit belongs to the owned
  app. Use `Control -Arranged` from `CapyUia.ps1` when locating a pointer target.
  A model update alone does not mean the visible rows have moved. Expand a
  collapsed column before addressing its group grip, and wait for a menu to
  acquire keyboard focus before sending Escape. Cancellation comparisons use
  the saved workspace layout; measured bounds can change as controls settle.
  Arrange keyboard targets before setting focus, then confirm their native focus
  before sending keys. `Focus-Keyboard` uses native Tab traversal when testing
  keyboard activation: UI Automation's SetFocus requests programmatic focus, and
  HasKeyboardFocus alone does not establish keyboard modality for Space.
  `-Backward` reverses the traversal when the next target is the canvas, whose
  Tab keys are handled as application input.
  Fixtures that need docked Paint controls call `Use-PaintWorkspace` before
  locating them; a fresh profile starts in Sketch.
  `Search-State` reads the owned window's partial command-search publication;
  `Model` can retain an older query. Live picker samples likewise come from
  the workspace presentation's `color_preview`, independently of full models.
  An accessibility container's bounds can describe its children without its
  padding. Compare shared width thresholds with the native panel allocation;
  check each button's bounds separately for padding, size and spacing.
  The Layers fixture requests keyboard focus explicitly before Space activates
  Add paper or native toggles.
  Revealing an offscreen row can move focus to another child.
  UI Automation hit queries can expose only the owned native host pane. Retain
  stable target bounds and native window ownership, then require acknowledged
  native activation on that same control.
  Convert DIP sizes with the owned window's DPI before resizing. A compact model
  projection does not prove that its native selector is visible.
- **Keep logs observational.** Log acknowledged state and owned control properties
  already used by assertions. A global focused-element lookup can lose its UI
  Automation peer during input; avoid querying it only for a log message.
  Record `CapyRowPointer.ContactDurationMilliseconds` for short contacts; a
  requested sleep does not measure the elapsed injection span.
- **Measure timed input.** Use the native test drivers for double presses and
  prediction strokes. PowerShell sleeps and method calls can exceed a gesture's
  interval under load; retain the actual injection timing with the result.
  `[CapyRowPointer]::PenHover($x,$y)` and `[CapyRowPointer]::Up($true)` keep a
  stationary pen in range; call `[CapyRowPointer]::PenLeave()` to end hover.
  The driver refreshes hover through its input timer.
- **Preserve OS cancellation.** A released sample with `IsCanceled` is a cancel,
  including for the hold recognizer. Use `PointerGesture::OnCompleted` for custom
  controls with rollback: it observes handled release, cancel and capture-loss
  events. Its default cancels capture loss; native Button and Slider gestures use
  `PointerCaptureLoss::CompleteLift` to complete a normal lift while canceling
  loss during contact. Match the owning `PointerId` before handling a completion,
  ignore unrelated or repeated completions, and clear ownership before releasing
  capture.
  Scrub buttons use `PointerGesture::Clicks` to scope suppression to their native
  contact, including cancellation and releases that produce no Click. Retain its
  returned state, mark an admitted scrub with `Suppress(pointerId)`, and gate the
  native Click with `SuppressClick()`. The helper retires leftover suppression
  at native release completion, including deferred right taps; cancellation
  permits the next keyboard or accessibility activation. It observes capture owned
  by the native Button and starts no scrub when that capture failed. The Button alone captures
  its pointer and raises Click. Unloading cancels the owning contact.
  The window-free input test checks the terminal-event mapping and suppression
  state. Use canceled touch input for rollback and subsequent pointer/accessibility
  checks; use normal releases to qualify keyboard activation. Injected canceled-UP
  can leave an ordinary WinUI Button unable to activate with Enter. Destroying a
  synthetic pen device also does not establish a delivered cancellation event.
  Physical pen cancellation needs a device check.
  Wheel and HDR contacts forward their shared `color_wheel_context` or
  `color_edit_context` through `ColorEdit`. Rust validates it when queued input
  runs, so a delayed slot or mask change cannot redirect an old contact or its
  rollback. Wheel context preserves dragging from Transparent into remembered paint.
  Keep the canvas-touch fixture's rapid canceled pairs: they must preserve the
  drawing instead of triggering Undo.
- **Wait for the operation you started.** Compare the document revision or the
  requested state before and after input. An already enabled Undo command does
  not prove that a new stroke has committed.
  Use the relevant control's terminal state to acknowledge a released contact;
  `WaitForInputIdle` does not acknowledge that operation.
  Shared worker notifications can arrive before the document operation finishes;
  wait for its completion state.
- **Prepare the selected tool.** Invoking an already selected grouped tool opens
  its drawer. Skip that invocation when restoring the current variant, then
  check the selection mode and closed drawer before a canvas gesture. The first
  outside contact dismisses an open drawer.
- **Capture composed pixels.** GPU pixel assertions use the owned window's
  screen rectangle on the private desktop. `PrintWindow` can return an earlier
  SwapChainPanel image. Wait for the expected visible change before comparing
  pixels. Traced snapshots identify their render-owner observation with
  `presentation_epoch`. The `presentation-state` acknowledgement must cover that
  observation and its resolved `required` present must be displayed before
  `Wait-StablePixels` samples. Completed no-op frames retire unused reservations,
  while DXGI supplies the actual displayed count. This barrier is needed
  because the desktop compositor can show canvas frames more than a second late.
  On the GPU-pool VMs DXGI can also withhold the count for an idle window's
  final present until it presents again. A traced window then repeats that
  unchanged frame once after a second without progress; the repeat never
  becomes `required`. Untraced windows never repeat frames.
  For restoration, compare with the original pixels; a fixed dark-panel
  threshold cannot verify the same journey in the light theme. Keep the sample
  grid, camera and canvas bounds from one checked projection. Move the owned
  pointer away from the capture area and dismiss tooltips before a baseline.
  Capture pointer failures with composed pixels before synthetic pointer cleanup.
- **Use current menu identifiers.** `NativeMenuItems` uses the shared command ID
  for routed commands; other layer actions use `layer-menu-<op>`. Check the shared
  menu definition when an item cannot be found.
  For popup dismissal, the palette fixture counts menu items whose parent is a
  `Menu` container. Permanent `MenuBar` headings remain after a flyout closes.
  `exercise-palettes.ps1 -Theme dark` and `-Theme light` preserve swatch order
  and seeded Redo after held touch/pen cancellation and mouse Escape. They also
  require the exact color definition and selected swatch IDs to remain, then
  check ordinary short taps and accessible swatch Invoke still select colors.
  Outside releases use an owned canvas work-area point beyond the swatch grid.
  Saved swatches and chooser results use native scrollbars for mouse input;
  pen and touch retain the same viewport and scroll position with bars hidden.
  The fixture imports its own overflow palette, compares exact scroll-provider
  and six-tile geometry across device changes, drags both native scrollbar
  thumbs, and checks last-tile color hits, reorder slots and one-step Undo/Redo.
- **Retain tool-slot identity.** Resolve a toolbar tile once and keep its ID
  when returning after a variant change; `resolved_control` remembers the chosen
  variant, so the original command may no longer identify that slot.
- **Select the telemetry owner.** Use `Workspace-Root` from `CapyUia.ps1` for
  drag state and arranged workspace bounds. The outer window and inner workspace
  share an accessible name; only the inner workspace publishes this state.
  The helper follows fresh native child containers and accepts the existing
  workspace schema, with an owned-root search when that route is unavailable.
  It keeps the original geometry, readiness and stability checks.
- **Keep callback data distinct.** `Wait-Until` callbacks run in its PowerShell
  scope. Expected values must not share names with the helper's parameters.
- **One snapshot per assertion.** Read related fields from one state file; check
  its `process_id`, `window_id` and freshness.
  Retain successful native gesture readiness snapshots for immediate source,
  device, grip, capture and release assertions; do not reread between them.
- **Own the test processes.** A fixture run directly can leave its app running
  for inspection after failure; close it before rebuilding. The private-desktop
  and VM runners preserve evidence and close the processes they own.
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
Swatch pen and touch holds use the native gesture recognizer and suppress the
release click. Short touch contacts still select a paint slot.

`exercise-compact-color.ps1 -Journey slot-menu-dark` and `slot-menu-light` walk
the shared paint-slot menu through mouse, keyboard, pen and touch. They check
editing another slot without changing selection on Cancel, targeted Apply,
Palettes and Swap, with exact seeded artwork pixels and history.
`color-editor` (dark) and `color-editor:light` walk Edit Color: format rows, paste
and copy, typing and refusal, value scrubbing and arrow steps, Current, the
swatch sheet and its search, remembered formats, canvas picking through the
strip, Use Color and the Paper's fill thumbnail. Resolve the active
`edit-color-dialog-frame` after opening or returning to Edit Color, and scope
its arranged geometry checks to that frame; canvas and popup choices retain
their original scope.
`enclose-fill` (dark) and `enclose-fill:light` draw an Enclose and Fill loop with
the mouse over reference ink: two closed holes fill a new layer, a crossed hole
and the exterior stay empty, and one Undo, Redo and Escape behave as on GTK.
`vector-layers` (dark) and `vector-layers:light` walk vector layers with the pen:
the Layers footer's New vector layer choosing G-Pen, the Pencil refusal's Use
G-Pen and New Paint Layer, an Ellipse outline filled inside and a paint layer
filled below it, Erase up to intersection from the Tools list, and Merge Down's
notice after the Gradient refusal's New Paint Layer Below. Each step checks
composed canvas pixels; the run fails if a Layers footer or notice button is
clipped or unlabelled.
The default journey also checks Sketch's path choices, locked header reasons,
clickable dimmed tool rows and immediate brush/Sculpt explanations. It compares
the same selected row's composed text contrast before and after its reason clears,
checks suggested-action fills against the shared palette and follows Back to Layer
after an outside fill. The German narrow notice must stack actions below text.

`exercise-vector-layers.ps1 -ToolsOnly` draws Figure and Path shapes, edits a
path anchor, restyles its width through Properties and scales a Move frame.
Both themes require exact composed artwork samples after one Undo and Redo.
`exercise-vector-edges.ps1` also checks saved tool choices across a language
restart, row interactions and ordered keyboard cancellation during path drags.

`exercise-idle-brush.ps1` walks all three Blender sets, Mixing Brush, Pencil,
Charcoal and Pastel Block through native categories, preset choices and settings.
Each paints the seeded drawing and restores exact composed samples through Undo
and Redo before the next preset.
It retains the selected-brush startup and Liquify Adjust/Reset checks.
The composed signature includes the original 768 grid samples and an exact ARGB
SHA256 of every whole pixel inside the projected document. Every preset starts
from the same seed signature. Accepted before, after, Undo, Redo and final Undo
states save document crops from the sampled bitmap with camera and document
metadata. Run `tests/uia-projection-snapshot.ps1` in PowerShell 5 and 7 for the
cached native projection, live canvas guards and dense pixel regression.

Use `exercise-tool-bubbles.ps1 -BlendersOnly` with `-Variant dark-mouse` and
`light-mouse` for independent Blender panel, ring and saved-state checks. Wait
for the native bubble layout to settle after preset reselection before reset.
The VM cases `tool-bubbles:blender-controls-dark` and `blender-controls-light`
stop after those controls; `blenders-dark/light` retain painting and persistence,
and `sliders-dark/light` also run the global slider preference and fan checks.
Blender session checks validate the saved document center before resizing the
restored window. Maximizing can change toolbar width and the visible work area;
check the shared resize behavior and project the same document sample into its
new native position before comparing exact pixels and history.
Use native Actual Pixels before the paint baselines so fractional display
rounding across a window resize does not change the compared samples.
Choose it from View; a workspace can hide the canvas view readout.
Use `slider-preference-dark/light` to check the global preference, workspace
history and SQLite restart without repeating the independent fan and Blender
checks. `exercise-idle-brush.ps1 -WetRoundOnly` retains the single Wet Round
stroke, pixel, history and close checks with the normal functional deadlines.
Use `effects:visibility-dark` and `visibility-light` for the focused Properties
visibility journey on the VM.
Use `header:catalog-only` and `header:catalog-only-light` for categories, Pen
order, shortcut tooltips and the header catalog. The legacy `header:Catalog`
switch still runs the full header journey.
Use `exercise-edge-reach.ps1 -ColumnResetOnly -Device mouse|pen|touch` for the
actual six-unit column dividers; `-Theme dark|light` selects one theme. A native
whole-column move creates a nested left owner beside the toolbar. The scope
checks a no-op single tap, outward resizing, exact preferred-width restoration
on double press, the untouched adjacent owner, and one-step workspace Undo/Redo.
Use `-CanvasResetOnly -Device touch` for finger double taps on the canvas side
of the outside column edge. It checks exact preferred-width reset at 20 and
35 units, exclusion beyond the 36-unit reach, a canceled first contact followed
by one fresh tap, and exact workspace Undo/Redo without assuming a default width.
The 20-unit case requires the reset while the second contact is still held.
The ordinary edge-reach journey checks contacts inside the wider free reach;
it does not replace the divider check. It also checks that Escape during pending
and claimed edge contacts restores the workspace and clears the contact.
Its touch case docks Layers beside a group
gap, fills the native list and hides its action footer through configuration.
An actual layer row must keep a sideways swipe and yield a vertical contact to
row resizing; one workspace Undo restores the exact layout.
`-ClosedStripOnly -Device pen -Theme dark` isolates the default Paint open
column's close-and-outward-swipe sequence without repeating the other checks.
`-LayerRowOnly -Device touch` isolates the real layer-row arbitration case.
`-CancelOnly -Device pen|touch` checks native pointer cancellation during
pending and claimed edge contacts, including exact rollback and released capture.
`-NumericOnly -Device mouse|pen|touch -Theme dark|light` checks that the numeric
caption, readout, slider and gaps inside the component remain reserved for input
even within a panel resize edge's reach. The whole NumberControl root has a
transparent hit surface and `edge_contact=input`, matching GTK's component
ownership. Ordinary brush-size buttons keep their normal edge handoff.
`-EdgeEscapeOnly -Device touch -Theme dark|light` checks that an actual selected
Tool Set subtool within the column reach keeps its immediate bubble contact.
Escape during pending and claimed edge contacts must preserve the brush, tool
and layout after release, and a subsequent contact must still claim the edge.
Use `exercise-expansion.ps1 -ResizeOnly -Device mouse|pen|touch -Theme dark|light`
for floating left/right/bottom/corner reach, native mouse hover and drag cursors,
toolbar-band lanes, column and stack collapse/reversal, adjacent toolbar widths
and exact workspace Undo, while the artwork revision stays unchanged.
Each move must acknowledge the matching native pointer position within two
seconds before geometry assertions; an existing claimed flag is insufficient.
An inactive native tab's top six units must yield a
vertical gap resize without selecting the tab; tapping selects it on release.
The fixture grows Stats through its native bottom
handle before probing the body, waits for native frame/tab geometry, and hovers
the published six-unit handles rather than the wider interior reach.
`-ResizeTailOnly` runs the toolbar-band and column/stack cases without repeating
the floating-panel and tab-strip checks. Mouse reach probes cancel with Escape
before releasing the button, so probing a toolbar command cannot activate it.
`-ResizeStackOnly` creates two members through a native column-header drop and checks
transient member opening and close/reversal without repeating the other cases.
It compares the exact durable layout and Undo availability after physical release.
Touch and pen first scroll floating Stats vertically through native input and
check its actual ScrollView offset without changing the frame width. Its native
scroll pattern restores the offset before the horizontal edge checks.
Use `exercise-header.ps1 -FullscreenOnly -Theme dark|light` for F11 entry and
exit from normal and maximized windows. It checks restored native bounds and
the fullscreen edge-gesture property, and retains window, monitor and DWM state.
The presenter-change trace must report successful DWM transition suppression
on entry and exit, followed by successful re-enabling in windowed state.
`DWMWA_TRANSITIONS_FORCEDISABLED` is a setter-only attribute. Timed private-desktop frames
record each transition for visual review. `-FullscreenBaseline` records an older
executable's policy and frames without requiring the new property or trace.
These checks do not qualify physical Shell edge swipes.
With `CAPY_TRACE_UI`, the `settings-title` peer publishes actual Preferences row,
swatch-strip and custom-field allocations in one native coordinate space.
`exercise-header-settings.ps1` uses these measurements for closed, open and
closed-again spacing checks; passive layout containers need no automation peers.
After choosing a preset, wait for its shared value/index and native selection
before opening Custom; an already closed field does not acknowledge that choice.

`exercise-canvas-touch.ps1 -ScrubOnly -Theme dark` checks three-finger clearing
at horizontal, diagonal and vertical angles, cancellation and locked-layer refusal.
Each clear preserves the camera and has one exact Undo and Redo. Fresh two-finger
navigation and pen painting must still work. Repeat with `-Theme light`.

Touch history compares authored checkpoints and layer order; thumbnail revisions
are cache invalidation tokens and may change when Undo or Redo restores a layer.
Bubble control journeys use the enabled settings in the shared control profile.

Run `exercise-workspace-pickup.ps1 -Executable <exe> -Device <mouse|pen|touch>`
in both `-Theme dark` and `-Theme light` on the private test display. Run
`exercise-workspace-layout.ps1 -Executable <exe>` in both themes too. These
fixtures check workspace input and layout correctness, not motion timing.

For floating-panel checks, center the actual drag preview in the shared work area.
The canvas element extends behind the editor controls, and a preview near a dock
can snap there while the pointer is over the canvas. After release, require an
idle native gesture, an ended shared drag and the panel in the committed floating
layout before testing its controls.

`exercise-workspace-pickup.ps1` and `exercise-documents.ps1` accept
`-DebuggerPath <cdb.exe>` to attach CDB before input and save access-violation
stacks and a local dump in each process's run directory.

Panel tab strips use `ScrollView` with horizontal content and reserve touch and
pen input for workspace dragging. Wheel scrolling remains native. Run every
`tab-pickup` device variant and `tab-drag` after changing the strip or its preview
clipping; they cover drifting panel/drawer tab taps, direct tear-off, retained
contact, cancellation and history. `exercise-hdr.ps1` also checks drifting pen
taps on drawing tabs before reordering and Undo/Redo.

`exercise-transparency.ps1` compares panel-tab lower corners at rest, on hover
and during a held mouse press in both themes and every transparency level. It
checks both neighboring directions, visible inactive hover, selection before
release and retained native tab buttons. Baseline shade samples move keyboard
focus to Settings so native focus borders are not mistaken for the tab fill.
`-Journey hover-dark` and
`-Journey hover-light` run the four-tab lower-hover contour matrix, covering
all active/inactive pairs, straight ends, reverse feet and active-foot protection.
They also check absolute colors along the 18-DIP upper curves and 6-DIP concave
feet at Off and High, including antialiased edge pixels over contrasting solid
canvases at the native display scale.
Opaque captures validate the color range, solid geometry anchors and smooth
curves, then supply native edge coverage for predicting the translucent colors.

`tab-pickup` checks the regular mouse
cursor, selection before mouse release, and short mouse, pen and touch taps.
`tab-drag` presses an inactive tab before pickup and compares its selected
shoulder pixels with the drag preview. Preview geometry is captured at pickup,
after press selection, and late selection publication updates its active fill.
Use `tab-drag:preview-dark` and `tab-drag:preview-light` for the focused selection,
preview pixels and cancellation checks.

`exercise-illustration-filters.ps1 -Executable <path> -Theme dark` checks
Brightness to Opacity, Threshold, Dither Spread, Scatter and Crystallize through
the native Filters and Properties panels. It compares composed pixels, changes
color and transparency choices, checks one-step history and saves and reopens the
hidden alpha threshold and Artistic parameters. Artistic controls and loaded
previews follow the shared schema; Crystallize exposes only Cell size and Stretch. Repeat
with `-Theme light`; the VM runner names are `illustration-filters` and
`illustration-filters:light`.
After reopening, select Layers before invoking a layer row; Filters can remain
the active tab in their shared panel group.

`exercise-image-objects.ps1 -Executable <path> -Theme dark` opens the fixed shared
image packages with built-in and ICC profiles, including Nearest sampling with
F64 placement. It checks displayed pixels and visibility history, then saves and
reopens each drawing while preserving shared image identities and object poses.
Repeat with `-Theme light`. Use `exercise-image-rows.ps1` for native
image-authoring and conversion journeys.

### Tool bubbles

`exercise-tool-bubbles.ps1` follows the GTK bubble journeys with native mouse,
pen and touch contacts in light and dark themes. Run the six `tool-bubbles`
variants through the VM fixture runner or the private-desktop runner on hardware.
It selects Sketch through the native workspace switch, authors its header through
the native picker, fits the drawing after maximizing
the window, and uses shared presentation geometry for contact points. The fit
must place the paper inside the work area at a usable scale before pixel checks.
Checks include
fast tear-off, canvas gaps, cancellation, persistent options, wheel drag-out,
two-finger movement and pinch, Layers pages, dab previews and exact artwork
Undo/Redo. The retained presenter shares the ordinary wheel, swatches, brush
previews, thumbnail cache, numeric controls and Layers inspector.
A second finger cancels an active native control edit before moving the card;
contacts starting on layer rows retain their native layer gestures.
HDR color bubbles use the shared plate contour and glass mask while retaining
the wheel's size and center. The native intensity arc presents the shared ramp,
marker and dynamic range, including temporary eyedropper previews. Its accessible
slider supports keyboard adjustment and Home/End range endpoints. Double taps
reset to zero, and holds or a second touch restore the
starting color before taking ownership. Transparent paint disables intensity
editing, and mask colors use the circular SDR layout.
The ramp and marker use the same SDR rendition as the Windows Color panel.
Run `exercise-tool-bubbles.ps1 -HdrOnly -HdrDepth F16` and `-HdrDepth F32`, each
with dark and light variants, for intensity input, cancellation, color-context
changes and composed plate, ramp, bead and palette checks. The fixture also checks
that repeated input keeps an expanded range and that triangle fields stay inside
their shared shape in both the docked Color panel and the bubble.
Use `-HdrContactsOnly` with an explicit dark or light variant for the existing
mouse, pen and touch intensity and wheel commit/cancel checks, hold rollback,
double-tap reset and outside clamp cancellation. This scope stops before the
keyboard focus and composed-pixel checks and preserves exact color and history
assertions.
Use `-HdrRetirementOnly` with either depth and theme for the focused slot, mask
and drawing-switch checks. Drawing activation must clear the native status;
both the SDR destination and returning HDR drawing must paint and undo to their
exact previous pixels and published checkpoint.
`exercise-tool-bubbles.ps1 -ColorUndoOnly` runs dark and light: a mouse drag on
the Color panel wheel is one step that its right-click menu undoes, outside the
ring the panel keeps its own menu, injected two- and three-finger taps on the
wheel, the intensity arc and the color ring undo and redo, a second finger
retires the first finger's pick, and a tap leaves the ring in place. Hovering the
ring opens no tooltip, and a finger pick on it keeps its color after lift as one
step that a two-finger tap undoes.
The fixture checks selected-face hover and pressed pixels against the theme's
text-color overlays and requires the ordinary face to keep its size. Moving past
the drag threshold and returning must retain hover shading; pressing starts a
new pressed state. Hover follows the current rendered placement while a held
pointer stays still. Escape must restore the lifted face's original bounds before
pointer release. Hidden chooser tags appear during a choice, not passive hover,
and cancelling the check restores the original chooser and artwork state. Run
`-VisualOnly` in both themes for minimum, intermediate and maximum wheel sizes
at all four corners, crowded choosers, and every transparency setting. The
captures and geometry records include native wheel bounds and palette buttons:
the 184 by 30 DIP pill has 32, 116 and 32 DIP buttons separated by 2 DIP gaps.
GTK remains the visual authority for shadows, labels, previews, disabled opacity
and transitions; Web is a secondary reference.
Run `-BeadTransitionsOnly` in both themes for held orbit movement past and back
inside drag slop, and native bead retirement and revival within 220 ms. It records
intermediate native bounds and captures, checks restored geometry and unchanged
artwork, and does not compare the per-frame alpha curve with a reference capture.
The history capsule exposes separate native Undo and Redo buttons for keyboard
and accessibility input, with labels and availability from Rust. Run
`tool-bubbles:HistoryOnly` and `tool-bubbles:HistoryLightOnly` to check their
actions against exact artwork pixels in both themes.
Use `NumericOnly`, `CurveOnly`, `NativeControlsOnly`, `ScrollEpochOnly`,
`HistoryOnly`, `HistoryLightOnly`, `MenuOnly` and `RapidOnly` to isolate the
corresponding journeys on a private desktop with a disposable profile.
`EraserOnly` and `EraserLightOnly` use Sketch's default Eraser source for native
tear-out, slider options and size controls, half-pressure pen erasure and exact
Undo/Redo pixels, and saved workspace restoration in each theme. The injected
pen reports 512/1024 pressure; its erase stroke must reduce the seeded sample's
RGB difference from blank by at least half. Raw blank, painted and erased
samples accompany the result.

Hardware qualification uses a normal optimized build and actual mouse, fingers
and a pressure pen. Repeat tear-out and return, chooser and halo changes, wheel
cancel, numeric rollback before two-finger takeover, active scrolling and inertia,
and row recycling and reorder in cards, drawers and docked panels. Check native
menus followed immediately by card controls, drawing replacement, focus loss,
minimize/restore, resize, Zen, reduced motion and return from stroke fade.
Move between displays with different scale factors during a contact and verify
that coordinates retain their original view revision. Synthetic input cannot
qualify physical digitizer behavior or eraser-end flags that the OS omits.

Compare composed captures with GTK at matched size, scale and theme, including
rounded glass, arcs, disabled opacity, labels, shadows and native card content.
Check HDR output and wheel colors on a compatible display. Measure moving
bubbles, collisions, chooser/halo expansion, wheel pinch, scrolling, folding,
reopening and stroke fade using actual presentation records and separate CPU/GPU
attribution under the [measurement rules](../performance/measuring.md). Frame
callbacks and submission counts do not qualify frame-rate targets. Retain source
and binary identities, captures and failures locally; record current measurements
in the tier tables.

Halo tracks, fills and thumbs retain their native elements during motion. Bead
content changes independently of placement. Dab requests stop after completion
and resume when their shared model changes. Wheel field rasterization and bitmap
upload use a worker-owned Direct2D context; device and request identities guard
delivery to the retained view. Changing drawings retires pending dab requests
even when the bubble keeps its ID and color. Parent placement and child geometry
update together when a bubble changes size; asynchronous motion replies do not
leave native hit targets at the previous extent.
Lists and folded faces register their thumbnail targets with the same cache by
owner. Requests and late replies use the combined targets, so one viewport cannot
discard another's pending pixels. Unloading an owner retires only its targets.

Numeric controls retain the native Slider for rendering, keyboard input and
accessibility. Painting sliders capture pointer input on their common numeric
surface and use the existing Rust value mapping. Explicit cancellation releases
that capture and restores the edit during a two-finger handoff; releasing a
stock touch Slider's capture alone can leave its pressed state active.
Completion, cancellation and owner retirement invalidate deferred slider
changes. Layers opacity uses the shared effect gesture transaction for rollback
and a single Undo step.

Panel scrolling shares `PanelScroll.h`: native pan and inertia events update a
ScrollView, with explicit cancellation before two-finger bubble takeover.
Pointer release and cancellation are observed at the XAML root so another
control's capture cannot retain a stale panel contact. Inertia stops at a
scroll boundary; a finger still touching the panel can reverse direction there.
Its built-in manipulation ignores touch and pen, which belong to the custom
pan handler, so a compositor gesture cannot take those contacts after handoff.
The ScrollView keeps keyboard, wheel, scrollbar and accessibility handling,
and provides the ItemsRepeater's virtualized viewport. Its background and the
inner presenter's background stay null so empty space belongs to the containing
card; the content keeps its own pan surface. A second touch in that empty space
must cancel the first control's edit and move the bubble, with a fresh control
gesture and exact Undo still working afterward. Fixture `Scroll-Pattern`
resolves its native inner presenter; the outer named container retains row
gesture evidence and geometry. Numeric, curve, row and
scroll controls offer a second contact to the bubble before starting an edit.
A row pan suppresses the row's click until release, including when pointer
motion arrives before the ScrollView's offset notification.
Row gestures and revealed swipe actions join the workspace's existing transient
registry. Escape dismisses them before the shared action can close their drawer
or card; destroying a list removes its registration.
Leaving the Layers surface preserves a pending row drag while a native child
button still owns its capture. Capture loss continues to cancel the gesture.
A handoff cancels every native owner of both contacts, including both row and
scroll handling, and requires successful capture before starting the bubble drag.
The current native manipulation completes before root capture transfers.
Unloading or changing drawings retires the scroll contact.
Native controls checks exercise wheel, Page Down and scroll automation alongside
touch scrolling, inertia interruption, held-contact reversal at both scroll
boundaries, recycling, reorder and exact Undo.
`tool-bubbles:ScrollEpochOnly` switches drawings during an active native pan,
returns to the original drawing and checks fresh scrolling and unchanged history.
The extra touch surface sits behind the bubbles while a first touch is active,
so the card keeps its native hit targets. Covering them redirects touch motion
before XAML scrolling starts, even with pointer capture.
Normal lift capture loss completes the bubble contact so a menu cannot leave
that surface active over the next interaction. Capture loss from the bubble input
root cancels its drag even if the native capture list has not yet removed the
pointer. Descendant capture transfers do not cancel a root-owned drag.

Run `tool-bubbles:RapidOnly` separately for reopening during the 150 ms card
fold. It observes native card retirement before injecting the reopen contact,
bounds that contact against the last observed open card, and checks the reopened
footer and Properties controls. The folded model alone does not establish native
event ordering; input outside the interval does not qualify the transition.

Hollow native paths can route a canvas contact through XAML instead of the
independent input source. `CanvasWindow` admits unhandled events targeted at its
`SwapChainPanel`, copies native samples on the event's owning thread, and sends
the values to the existing input worker. Both sources share sample conversion,
ordering, modifiers and contact tracking. Missing or empty intermediate history
uses the current point, including during resize and pointer rerouting. Each
source runs its hold recognizer on its own thread; the worker validates the
active contact before a hold acts.
Native caption passthrough regions cover only intersections with caption areas.

The VM establishes software-adapter behavior. Moving-frame performance and
physical pen parity still require reference hardware.

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

Resolve queued UIA InvokePatterns before triggering GPU loss. Invoke those same
controls after the recorded preparation boundary; queued pen events must still
fall strictly inside the recorded reconstruction interval.

The isolated UI test hook drains the rendering and presentation queues on every
participating render worker before calling `ID3D12Device5::RemoveDevice`. WARP can crash when
manual removal races queued presentation work. The hook still removes the real
device; active contacts and queued input survive successful reconstruction.
Replacing the surface must not cancel those contacts. The ignored Rust removal
tests above exercise in-flight GPU work on hardware without this drain.

Window-free document fixtures call `DocumentService::renderer_unavailable` before
retiring the active renderer. This releases the device retained for tab activation
and follows the production reconstruction lifecycle. An opened native package
that fails execution admission remains in the read-only package view; removal
fixtures check its exact retained bytes and close that view before retrying.

Paper image decoding, library reads and writes run on workers. Paper and
palette jobs use the workspace task's guarded wakeup. Shutdown disconnects
those wakeups before releasing the native window callback, including while
file work is still running. The picker uses four columns with 84-DIP tiles
and 72-DIP previews. Tile captions use regular text and wrap at whole words.

For an existing owned window on the private test display,
`exercise-tool-features.ps1` takes `ProcessId`, `WindowHandle`, `StateFile`,
`OutputDirectory`, `Journey` and `Theme`. Run `smoothing`, `size-keys`, `preview`
and `size-controls` in both themes. The size controls journey selects Sketch's
Pencil and Eraser, clicks and drags with mouse, pen and touch, and checks the
shared size, retained slider, popup dismissal, unchanged drawing and exact
canvas pixels. The preview journey checks changed and restored stamp pixels.
The size-key journey holds each bracket key through repeated native key-downs and
records the accepted calls; it does not qualify physical keyboard auto-repeat.
The canvas and Curves forward every press in Windows combined key messages;
shared Rust retains shortcut steps and repeat policy.

### Control reviews

Separate review builds replace the app's entry point with production controls
or upload helpers. Number and Color use shared Rust models for comparison
with the Web controls. Fixtures that create workspace data adopt the shared
localization bootstrap before constructing controls.
Generate those models at the display's actual scale,
then build and capture. Number models include shared captions, both formatted
range endpoints and panel metrics. Use a fresh model directory after changing
that contract. Every numeric report requires its visible formatted readout,
including the disabled 12 px spin row. Panel sliders retain their track without
step buttons; spin rows retain both buttons and their bound and opacity checks.
Keep the value host alive while constructing spin and value-only rows. The
formatted readout supplies the visible ink; the idle native editor stays
transparent in both enabled and disabled rows.
Reviews compare the actual caption text bounds and require visible value pixels
in enabled and disabled spin rows:

```powershell
cargo run --locked -p layer-windows --example number-fixture -- artifacts/windows/numeric-matrix
./apps/layer-windows/scripts/build.ps1 -ControlFixture Number -OutputDirectory artifacts/windows/NumberReview
./apps/layer-windows/scripts/capture-number-controls.ps1 -Executable artifacts/windows/NumberReview/CapyCanvas.exe -FixtureFile artifacts/windows/numeric-matrix/fixture-windows-light.json

cargo run --locked -p layer-host --example compact_color_fixture -- artifacts/windows/compact-color-parity 1.5
./apps/layer-windows/scripts/build.ps1 -ControlFixture Color -OutputDirectory artifacts/windows/ColorFixture
./apps/layer-windows/scripts/capture-compact-color.ps1 -Executable artifacts/windows/ColorFixture/CapyCanvas.exe -FixtureFile artifacts/windows/compact-color-parity/dark-160.json
```

Color models come from an unattached shared session with U8 SDR document state.
Regenerate both native and Web inputs together after changing its preview or
query contract; the Web capture requires shared Layout/HueStops responses and
completed field requests for every shape, including Square. Its fixed SDR test
requests are checked through the production Web raster worker against the CPU
RGBA files; they do not execute the Web App's metadata-selection method.

Color and Number captures select a unique review surface from the owned process's
top-level windows, checking both UI Automation and Win32 process ownership.
A process can own an input overlay as well as its review window; its reported
main window is not a control-surface identity. Keep the 45-second discovery bounds.
Read fixture and native report JSON with `Read-Snapshot` so UTF-8 captions survive
both supported PowerShell versions.

Build `Feedback` to check button feedback in the main root, a ContentDialog and
a Flyout. Popup terminal events do not reach the main root; both local and root
observers retire contacts through the same completion rule. The focused journey
checks mouse, pen and touch release/cancel feedback and exact click counts in
both themes:

```powershell
./apps/layer-windows/scripts/build.ps1 -ControlFixture Feedback -OutputDirectory artifacts/windows/FeedbackReview
./apps/layer-windows/scripts/test-desktop.ps1 -Script apps/layer-windows/scripts/exercise-button-feedback.ps1 -Parameters @{Executable='artifacts/windows/FeedbackReview/CapyCanvas.exe'}
```

The same build runs `exercise-sdk-feedback.ps1` for actual ComboBox templates
and ContentDialog footer buttons. It checks native pointer and keyboard
completion, disabled appearance, retained resources and default-action changes:

```powershell
foreach ($theme in 'light','dark') {
    ./apps/layer-windows/scripts/test-desktop.ps1 -Script apps/layer-windows/scripts/exercise-sdk-feedback.ps1 -Parameters @{Executable='artifacts/windows/FeedbackReview/CapyCanvas.exe';Theme=$theme}
}
```

`exercise-navigator-allocation.ps1` uses that build for the actual Navigator's
first native allocation. It checks natural height under an infinite parent,
the minimum allocation and fractional dimensions directly from Loaded and
LayoutUpdated events, before a later dispatcher callback could correct them:

```powershell
./apps/layer-windows/scripts/test-desktop.ps1 -Script apps/layer-windows/scripts/exercise-navigator-allocation.ps1 -Parameters @{Executable='artifacts/windows/FeedbackReview/CapyCanvas.exe'}
```

For an old-code negative control, `-ExpectFailure` requires the first allocated
220-unit overview mismatch.

Build `Bitmap` to check the common native uploader with owned pixel buffers and
the same private context setup as the other control reviews:

```powershell
./apps/layer-windows/scripts/build.ps1 -Configuration Release -ControlFixture Bitmap -OutputDirectory artifacts/windows/BitmapReview
$run = Join-Path (Get-Location) ('artifacts/windows/bitmap-review/' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run | Out-Null
$env:CAPY_BITMAP_FIXTURE = Join-Path (Resolve-Path $run).Path 'input.json'
'{"schema":1}' | Set-Content -LiteralPath $env:CAPY_BITMAP_FIXTURE -Encoding utf8
```

Launch `artifacts/windows/BitmapReview/CapyCanvas.exe` visibly on the fixture
runner's private test desktop with that environment. The input path must be
absolute; the review creates its isolated context beside it. Require exit code
zero and `bitmap-upload-fixture-result.json` with `passed: 14`. Keep the
90-second outer, 10-second callback and five-second close bounds.

The review checks real asynchronous uploads and rendered pixels, RGBA/BGRA
encoding, two-buffer reuse, failed-copy retirement, latest-request coalescing,
callback reentrancy and exceptions, context cancellation during upload,
closed-owner rejection, independent thumbnail owners, and new-document requests
after cancelling an old document's active upload. A retained ImageBrush also checks exact white-to-blue
Paper pixels at 32 × 32. It checks loaded, connected state before waiting for a
visible rendering event. Its 3 × 2 source-pixel oracle uses Image dimensions
divided by `XamlRoot.RasterizationScale` and a natural-size `RenderTargetBitmap`
capture, preserving exact physical pixels at the actual display scale.

The Zoom readout journey checks that opening and closing the readout preserves
the exact camera, artwork and undo history. It also checks the saved Snap
rotation checkbox, rotation slider and native Rotate View drags with snapping enabled and disabled,
including the 3° hold and smooth transition back to free rotation by 10°.
For the focused preference and gesture journey, pass `-RotationSnapping` with
one of `dark-mouse`, `light-mouse`, `dark-pen` or `dark-touch`. It uses an owned
private desktop and retains the default full readout journey. The VM runner accepts
`zoom-readout:dark-mouse`, `zoom-readout:light-mouse`, `zoom-readout:dark-pen` and
`zoom-readout:dark-touch`.

Pass the native reports to the `number-controls` and `color-panel` scenarios of
[the visual tools](../../tools/visual/README.md). Rebuild normally before using
the editor again.

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

The Web collector waits for submitted GPU work after layout settles before
capturing the canvas.

Before each capture, the fixture searches every fresh `layer-list` peer for
visible rows and requires each row to exist in shared state with its required
previews `Ready`. It retains the camera and readiness checks and allows 45 seconds
for three consecutive unchanged geometry observations.

Tool Set geometry compares the visible part of each control after viewport and
ancestor clipping, retaining full DOM bounds in `unclipped_bounds`. Controls
wholly outside the visible panel are excluded from its geometry inventory.
Layers uses the current semantic blend button. Each host still measures its
own controls; these records do not alter the raw images or comparison limits.

Workspace shadow masks keep their host at XAML opacity 1. Hide the host through
its compositor visual so XAML rendering can refresh the `GetAlphaMask` surface
after shape changes. Keep the source brush mapping and shadow geometry unchanged.

Panel content measurements use the current width with unconstrained height;
allocated or clipped body heights must not become natural sizing inputs.
Inactive tabs measure their controls too, using copies without preview queries
or shared rendering caches. Fixed controls and one complete scroll row reserve
space before a fitted sibling receives the remaining height.
Automatic tabs publish cached full-caption widths even when their labels are
currently hidden; the tab strip reserves the shared 20-pixel member grip.
Histogram and Waveform put Log counts on its own row between the graph and the
status and clipping controls. Their graph stretches within the allocated body while
the selectors, axis, Log counts and footer retain their height; the preferred graph height
remains the natural sizing input. Histogram axis labels stay with their graph,
4 pixels below the plot.
The plot uses its panel surface without a separate input-field backplate.
Log counts, Tool Settings, Tool Options, Properties and Proof checkboxes share
`compactCheck`: at least the 26-pixel panel control height, with the 4-pixel
control gap between the native box and its caption, as on GTK and Web.
Scope status and scale labels, including Curves axes, use the secondary text
role at 55% opacity.
Tool Set rows use the defined text line height and preserve fractional layout coordinates
so native pixel rounding does not accumulate into larger rows.
Workspace group frames preserve the shared fractional allocation through their
native subtree so display scaling cannot change Tool Set wrapping.
Add Filter and the Layers blend control use the shared control fill.

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

Scrollable native panels publish changed geometry through their existing layout
callback, including compositor-only scrolling that does not raise LayoutUpdated.
Tool fixtures focus the requested control before revealing its complete row.
Native focus can bring the control into view; the full-row geometry checks remain.
For workspace panel controls, reveal rows within the published native
`panel-body-scroll-N` bounds, converted to screen coordinates. A scroll provider's
UIA bounds can include its full content extent. Keep its ScrollPattern for input;
use the allocated body viewport for visibility and scroll direction.
If an offscreen control has empty bounds, `Reveal` starts one bounded search from
the provider's top, then retains the same viewport containment checks.
Groups also have a horizontal tab-strip scroller. Panel allocation fixtures select
the unique owned body provider by the published body viewport before checking its
scroll extent; counting every provider below the whole group includes the tabs.
Top captures reuse each row's validated expected height when revealing it again.
Docked Tool fixtures dismiss any tool drawer with canvas focus and Escape after
selection, then wait for shared closure and native drawer removal before finding
tool controls.
Before a panel action, fixtures reveal its collapsed column through the native
column icon and wait for the shared active panel.

Fixture ownership records use the observed process ID, native HWND and executable.
A traced WindowID is optional and comes only from a validated window registry;
it is not the HWND. Registration rechecks native ownership and any available
registry, including a registry published after the first observation. Performance
fixtures keep UI tracing disabled.

Run the owner-census helper regression without opening an application:

```powershell
pwsh -NoProfile -File apps/layer-windows/tests/fixture-owner-census.ps1 `
  -Helper apps/layer-windows/scripts/CapyUia.ps1 `
  -Output artifacts/windows/fixture-owner-census-fresh
```
