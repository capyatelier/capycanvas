# Native Windows implementation

> Historical design or validation record. Statements about completion and remaining
> work describe the recorded checkpoint. Start with the [current technical guides](../README.md).

Status: native canvas, workspace controls, header and Preferences checkpoints
pass. Full feature/visual parity, complete physical-input validation and final
performance acceptance remain open. The web reference runs in local Windows
Chrome; further 120 Hz benchmarking is deferred until the rest of the app is done.

## Goal and boundaries

WinUI 3/C++/WinRT controls bind to the existing Rust UiSession. The Windows
adapter supplies a D3D12 SwapChainPanel surface to layer-render-wgpu. One canvas
owner handles session commands and presentation; controls never wait for a live
frame. The full-window viewport extends behind the custom titlebar and caption
buttons. Settings may use native Windows styling; workspace geometry and visual
roles follow the existing apps.

## Acceptance gates

- Review matched light/dark workspace screenshots and logical geometry against
  the actual web app, at fixed viewport, DPI, document, workspace, and camera.
- Exercise settings, tools/layers, filters, import/export, shortcuts, docking,
  resizing, floating panels, undo/redo, and persistence through shared semantics.
- Confirm physical pen pressure/history/tilt/eraser/hover/cancellation, mouse,
  keyboard and touch navigation without conflicting routes.
- Verify titlebar drag/caption controls/Snap, maximize/restore, mixed DPI,
  minimize, close, surface/device recovery and full-window canvas continuity.
- Define hardware/workload/duration/missed-refresh limits in the first measured
  presentation report. Measure actual sustained >=120 Hz on supported displays
  separately from the existing input-to-present p99 <8.33 ms target.
- Keep readback outside input/render timing. Offscreen GPU timings alone do not
  demonstrate presentation throughput or physical input-to-photon latency.
- Provide reproducible build/run/package and automated test commands with
  dependency notices; keep packages, captures, binaries, and reports ignored.

## Milestones

1. Native XAML window, full-window D3D12 canvas, input and timing foundation.
2. Shared-state-driven workspace and settings; screenshot/interaction parity.
3. Remaining flows, lifecycle, physical device and sustained performance checks.
4. Reproducible native package and final acceptance report.

## Design self-review (before implementation continues)

Reviewed against the current shared session/input contracts, Android host,
wgpu 30.0.1 DX12 implementation, WinUI SetSwapChain threading documentation,
and upstream main at 40d9096. This is a design review, not a performance pass.

### Confirmed choices

- Keep WinUI 3 / C++/WinRT + XAML and the existing Rust renderer. A single
  SwapChainPanel fills the entire extended client area. Header controls are
  overlays; no separate canvas-sized image, CPU pixel transfer, or header
  rendering surface is needed.
- Keep one mutable UiSession/render owner. Use native platform APIs only for
  widgets, OS input collection, window/surface lifetime, and presentation.
- Keep the Windows ABI app-local. Do not expand the existing headless canvas
  ABI or extract an Android/Windows host framework prematurely.
- Start on pinned Windows App SDK 1.8.260804001 and C++/WinRT 2.0.250303.1.
  Record all restored transitive package versions. Keep deployment local and
  unpackaged during development; self-contained packaging is a later gate.

### Required corrections to the initial draft

1. **Input order and capture revision.** Raw records carry the capture-time
   camera/view revision and sequence. Do not retag delayed history using the
   worker's latest state. Retrieve complete history immediately, reverse
   newest-first histories, distinguish the terminal down/up from intermediate
   moves, and normalize physical coordinates and QPC timestamps consistently.
   Validate a complete batch before changing session state. Never silently
   drop an unaccepted suffix or stroke termination on queue saturation.
2. **Independent input collection.** A canvas presentation wait must not stop
   OS input collection. Prefer the SwapChainPanel independent input source on
   its own dispatcher thread, separate from the render owner. UI controls use
   the UI dispatcher. All three communicate through bounded transports with
   explicit ordering/back-pressure and shutdown rules. A UI-thread input path
   may be used to bootstrap, but does not pass the latency/input gate.
3. **Surface lifecycle.** SetSwapChain belongs on the XAML UI thread. Normal
   acquire/render/present and GPU waits belong on the canvas worker. Explicitly
   model initial attachment, running, quiescing for resize/rebind, suspended,
   device-lost and closing. Initial create-on-UI does not prove later recovery
   is safe. Rebind through an asynchronous quiescence handshake; never join a
   worker on the UI thread while it can be waiting for that dispatcher.
   Release acquired textures before resize and detach before destroying XAML.
4. **Device loss.** Surface reconfiguration is not device recovery. Recreate
   renderer resources from authoritative shared document/assets, preserve
   preferences/workspace, and surface errors. A panic crossing the ABI must
   poison/stop that host instead of continuing with potentially partial state.
5. **Frame pacing.** Use the display's current cadence and DXGI latency signal;
   no fixed 120 Hz timer. Compare maximum queued latency 1 versus 2. Acquire
   before draining the latest input for a frame, but keep collection active
   during the wait. Sleep when there is no work; bound retries on occlusion,
   resize and timeout. Do not infer actual display timestamps from Present().
6. **UI publication.** Static catalog data is fetched once. Use changed regions
   and revisions to reconcile controls. Camera-only updates use a small patch,
   while viewport/host state must independently invalidate relevant layout.
   A single state revision is insufficient for all host presentation changes.
7. **Filters/assets.** The draft used an incorrect API and ignored a load error.
   Use the existing load_effect_package operation with the actual WGSL modules
   and report validation/install results. Package editable shared filter assets.
   Do not claim an empty module list successfully loads the runtime library.
8. **Full-window input and scaling.** Define caption drag, passthrough control,
   canvas and popup hit regions independently of canvas pixels. Update them
   after layout/DPI changes and Zen transitions. Match the physical swapchain
   extent to composition scale; no hidden titlebar offset in the camera.
9. **No premature parity claim.** The foundation may have minimal controls.
   It is not a substitute for all current shared features, including upstream
   rulers/affine transforms. Catalog-driven UI and explicit shared host requests
   must remain the route to feature parity.

### Validation plan

- Pure adapter tests: malformed batch atomic rejection; history ordering;
  phase/eraser/cancellation mapping; capture revision preservation; timestamp
  conversion; queue saturation preserving down/up/cancel ordering.
- Lifecycle integration: paint while resizing and opening menus; mixed DPI;
  close during startup/rebind; minimize/restore; forced surface/device loss;
  verify bounded shutdown without dispatcher/worker deadlocks.
- Visual checks: paired compositor/window captures (not canvas exports) and
  geometry assertions, both themes, equivalent saved state, matching viewport
  and DPI. A stroke/panned document reaching the top of the window proves
  continuity behind the header; plain matching background colors do not.
- Presentation checks: record input delivery and queue age, engine/encoding,
  GPU completion timestamps, acquisition/Present timings, and actual display
  events independently. Capture screenshots outside measured gestures.
- Native UI accessibility, focus, IME, shortcuts, touch targets and system
  caption behavior remain explicit gates even where geometry matches the web.

### Hardware gate

A 3840 x 2160 display running at 120 Hz is available for local validation.
The >=120 Hz application presentation gate remains unmeasured. Run
`apps/layer-windows/scripts/probe-displays.ps1` to confirm the selected display's
active mode before each benchmark. Adapter-wide queries may report a different
monitor's mode. Keep raw host diagnostics, desktop coordinates, captures, and
machine-specific reports under ignored `artifacts/`; publish only reviewed,
sanitized results needed to reproduce the performance claim.

Before performance acceptance, record a fixed matrix including 4K/32-layer
documents, G-Pen, large eraser, Natural Blender, Watercolor Wash, pen-up,
pan/zoom and controls animating during drawing. Use repeated warmed runs and
a sustained thermal run, retain all samples, and publish p95/p99/max plus
missed-refresh counts. Numerical missed-refresh/latency limits must be fixed
before measuring, not selected afterward to make a run pass.

### Coordination and milestone publishing

Windows owns branch ports/windows in a dedicated checkout. The original main
checkout is left clean. Fetch origin/main before each milestone; commit only
reviewable validated Windows work, merge incoming main changes on this branch,
resolve conflicts without dropping other ports, and rerun affected checks before
pushing. Keep milestones small and report what passed and what remains open.
Do not push incomplete scaffolding as a completed foundation or reset/force-push
shared history. Shared-crate changes are minimal and explicit for other ports.

Milestone 0 is the reviewed design and acceptance plan. Milestone 1 requires a
built/running native window, actual GPU canvas, working input, lifecycle checks
and honest presentation diagnostics. Later milestones cover parity and package.

### Windows bootstrap checkpoint

The native C++/WinRT desktop project builds and launches with WinUI's built-in
control metadata and styles. Resource initialization happens in OnLaunched,
after application construction. One D3D12 surface covers the extended client
area; an inverse DXGI composition transform prevents double scaling at high DPI.
The independent input dispatcher starts after the first presented image, while
resize and shutdown use asynchronous ownership handoffs.

Local app-only captures verify a controlled replay stroke, undo/redo, and the
same painted document panned behind titlebar controls. Resize, native command
invocation, and coordinated close also pass. Automated OS mouse movement did not
reach the intended window, so real OS input delivery is explicitly unverified.
This checkpoint does not pass milestone 1: bounded input transport, capture and
cancellation details, recovery, presentation telemetry, and 120 Hz measurements
remain outstanding. Workspace/settings parity and final packaging also remain.

Upstream now provides layer-host and staged GPU startup. Integrate these shared
facilities as the Windows shell expands, keeping the Windows surface ABI local.

The checkpoint was rebuilt from a fresh C++ intermediate directory after merging
upstream b39e025. All 10 adapter/shared-host tests pass, and the merged runtime
again produced the replayed stroke and passed resize/close checks. The upstream
renderer now deprecates the eager constructor still used by this bootstrap;
staged startup integration remains required. Rounded physical extents avoid
truncation, but XAML content and the Win32 client capture still differ by one
physical row on the test setup. Resolve native-frame capture accounting before
using full-image comparisons to claim parity.

### Shared-host and staged GPU checkpoint

Windows now uses NativeHost for actions, snapshots, queries, gesture routing,
paint cancellation and startup readiness. Its typed pointer path keeps OS
timestamps and camera revisions as u64 values and preserves pressure, tilt,
twist and flags. The shared host assigns engine sequence numbers after any
inserted cancellation; predicted boundaries cannot end deferred real contacts.

Device and staged renderer preparation run on the render worker. Initial
swap-chain attachment uses the same UI-thread handoff as subsequent resize.
Paper is presented before background brush compilation, and the shared startup
state controls painting readiness. CPU-side workspace models can be queried
before the GPU is attached; the full Windows workspace is still to be built.

All 13 relevant Rust tests pass and the native desktop project builds. Local
controlled captures show the stroke, undo and redo; the same document pans
behind the titlebar. Resize and coordinated close pass with no runtime stderr.
These checks do not establish real OS input delivery, startup latency, visual
parity, or 120 Hz presentation. Those acceptance gates remain open.

### Initial native workspace checkpoint

WinUI now consumes the shared layout and panel models for the initial tool,
brush, size and layer panels. It reuses existing icons and brush preview assets.
The first snapshot is applied before GPU preparation, and later value updates
retain the corresponding native widgets. Camera-only patches are coalesced
separately from full snapshots. Native color/opacity flyouts and stateless Rust
numeric requests provide working brush controls without UI-thread GPU access.

The reproducible UI Automation smoke test passes logarithmic slider mapping,
size presets, text-field retention, layer creation and undo. Controlled replay
still paints through the compositor with the workspace present; resize and
close pass. The 13 adapter/shared-host Rust tests pass.

A matching web build runs in an isolated local Chrome profile on the hardware
GPU with no page errors. Comparing its dark workspace with the native capture
caught spacing/corner, numeric presentation and source-encoding issues. This is
a development comparison, not a visual parity pass: header and layer structure,
settings, docking/customization, other panels and native-frame accounting remain
unfinished. Captures, raw reports, traces and build outputs remain ignored and
local; only this sanitized validation summary is published.

After merging upstream d4eafac, Windows delegates staged frame preparation to
NativeHost::prepare_canvas_frame, shared with the Apple and Android presenters.
The merged build passes 14 adapter/shared-host tests and the native workspace,
controlled drawing and shutdown smoke checks. OS mouse automation still cannot
target this window reliably; physical pointer delivery remains unverified.

### Bounded input and navigation checkpoint

The canvas transport now bounds both queued items (256) and retained payload
allocations (1 MiB), reserving 32 slots and 64 KiB for UI commands. Pointer
histories are sent in ordered batches of at most 64.
The independent input producer waits for capacity without blocking XAML; drain,
close, render failure and explicit command overflow wake waiting producers.
UI command submission never waits. Exhaustion reports a persistent error,
drains accepted work, cancels active input and stops the render owner.

Wheel input uses shared scroll policy with Windows wheel settings, Ctrl zoom
and Shift horizontal pan. Canvas keyboard input uses the shared shortcut policy;
native widgets retain editing/navigation, and releases preserve their original
key identity across modifier changes. DPI and camera revision are captured
together. Routed release joins capture loss/routed-away as a cancellation path.
These routes still require physical-device and mixed-DPI interaction validation.

The native build, 14 adapter/shared-host Rust tests and queue allocation/order
tests pass. UI Automation workspace controls, controlled drawing, resize and
close pass. A 32,768-sample replay reaches bounded backpressure, and coordinated
close completes within five seconds with no runtime stderr. This validates
transport shutdown, not physical input delivery or presentation performance.
Local captures preserve the existing full canvas extent. Full workspace,
settings, lifecycle/recovery and measured 120 Hz acceptance remain open.

### Presentation capture preparation

An optimized build now provides an opt-in steady-canvas DXGI probe. It records
the native canvas swap-chain identity after startup readiness and refreshes that
identity after reconfiguration. The capture script filters to the probe process,
checks its active display, disables input tracking, and rejects reconfiguration
or a missing canvas swap-chain match. Reports and hashes remain local.

The Release build launches successfully with FIFO and maximum frame latency 1.
The capture preflight identifies the 120 Hz display, but Windows denies ETW trace
creation in the current non-elevated session. The prepared capture script requires
an administrator run; the drawing app stays at normal privilege. No presentation
rate, sustained painting rate or input latency result has been established.

### Native header and Preferences checkpoint

The full-client SwapChainPanel now sits beneath a shared-model header: Edit,
View and Workspace menus, document information, Zen, fullscreen and Preferences.
System caption buttons remain native, with drag rectangles computed from the
actual header controls and caption insets. A controlled drawing/pan capture shows
the document and ink continuing through the titlebar area. Fullscreen and return
to a normal window pass the native UI fixture.

Preferences uses the shared pages, rows, choices, validation, search, numeric
policy and shortcut editor in a native ContentDialog. Image choices retain the
reference tile geometry; numeric fields distinguish sliders from spin controls.
Palette brushes update in place, preserving settings fields and pending drafts.
Disabled rows disable their nested controls, including keyboard interaction.
The window shutdown path dismisses the dialog before releasing the canvas.

Native UI Automation passes theme-menu round trips, color validation, retained
fields, exclusive icon selection, shared search, dependent controls, shortcut
editor cancellation and fullscreen. These assertions wait for an opt-in shared
snapshot as well as native controls. Existing workspace checks and controlled
drawing/panning pass; Zen hides chrome on controlled canvas contact and its exit
button restores it. The 200 shared UI/host/Windows Rust tests pass. The layer test
now waits for the expected layer count through transient UI tree reconstruction.

CAPY_TRACE_UI is an explicit local test switch; ui-state.json can contain user
settings and stays ignored. Presentation-probe launch clears that switch.
Only source, tests and this sanitized account belong in the milestone.

Settings persistence, complete workspace/panel functionality, physical input and
shortcut capture, OS theme changes, mixed-DPI/lifecycle recovery, exact visual
parity and packaging remain unfinished. This checkpoint does not establish
120 Hz presentation, sustained painting performance or input-to-present latency.

### Current Windows validation findings

A manual pen test in the open prototype produced a visible stroke. This confirms
basic pen drawing for that run; the exact tested binary, pressure/tilt/history,
eraser, capture cancellation and latency have not been established.

The first identity-matched, 20-second steady-canvas trace on the nominal 120 Hz
display recorded about 60.00 presents/s and 50.35 displayed frames/s. Of 1,195
records, 1,003 reported reaching display; 192 did not report a display time.
Display interval p99 was 33.47 ms. Present-to-display p99 was 31.40 ms, which
excludes input and earlier drawing work and is not an input-latency result.
This run overlapped renderer GPU tests and is not an uncontended baseline.

The subsequent uncontended 20-second trace recorded 2,396 canvas frames, all
reported displayed, at 119.95 displayed frames/s on the nominal 120 Hz display.
Display interval p99 was 8.59 ms, with a maximum of 16.70 ms. Present-to-display
p99 was 7.60 ms. This establishes a steady unchanged-content baseline, not
sustained painting performance or input-to-present latency. It does not by itself
complete the performance acceptance criteria.

Further 120 Hz benchmarking is deferred until the rest of the app is complete.
The continuous probe is stopped; development and bounded correctness checks
continue without the external display.

The initial upstream integration passed 273 core/engine/UI/host/Windows tests.
Its serial hardware D3D12 suite reported 101 passes, 16 ignored benchmarks and
three failures. Two renderer failures are now corrected: Windows debug builds
keep GPU shader optimization enabled while retaining API validation, and image
initialization explicitly quantizes linear paint bytes before UNORM storage.
The actual selection-outline test and an expanded independent oracle covering
all 65,536 color-byte/alpha pairs pass. The import change matches the subsequent
upstream correction.

The targeted filter suite reports 14 passes, three ignored benchmarks and one
remaining strict historical-PNG failure. That reference also fails on this
machine's Vulkan backend; upstream documents the same unresolved reference
contract on Vulkan and Metal. The saved image and tolerance are unchanged.
Complete rendering parity remains unaccepted.

The native Preferences failure was reproduced as a close/reopen race, including
without concurrent GPU work. The popup disappears before its ShowAsync operation
finishes; a new shared open request is now reconciled after that operation
completes. The native fixture passes theme, color validation, retained controls,
icon selection, search, dependent controls, three rapid dialog reopen cycles,
shortcut cancellation and fullscreen. Local tracing used to identify the race
has been removed from production source.

The presentation analyzer reports actual display intervals separately from
submission intervals and present-to-display latency. Synthetic checks cover
swap-chain/process filtering, missing display records, percentiles and rejected
invalid data. Raw captures, pixel differences and device metadata stay local.

The subsequent merge of shared column/drawer interactions and Apple controls
passes 284 core/engine/UI/host/Windows tests. The full serial D3D12 correctness
suite now passes 103 tests, skips 16 explicit benchmarks, and retains only the
strict historical-image failure described above. The rebuilt WinUI app passes
the header/Preferences and workspace fixtures, and closes with empty stderr.
Shutdown with shortcut capture open also passes. These checkpoints do not imply
that Windows has implemented the newly merged shared drawer functionality.

### Windows preferences persistence checkpoint

Windows restores shared settings before GPU startup from
`%LOCALAPPDATA%\CapyAtelier\CapyCanvas\settings.json`. Missing files leave
defaults untouched and do not cause an initial write. The shared settings
migration and validation policy remains authoritative.

A dedicated storage worker writes a bounded temporary file, flushes it, and
atomically replaces the saved file. Its mailbox retains at most one in-flight
write, one latest pending value and one completion. Superseded shared save
requests are retired; the latest request completes after the write finishes.
The render and UI threads perform no disk writes. Shutdown commits active
Preferences drafts, drains accepted commands and joins storage before releasing
the callback context.

Unreadable or unsupported settings remain unchanged on load. A subsequent valid
change preserves the original bytes in a recovery file before saving defaults
with the new preference. Failed replacements leave the last saved file intact,
report a recoverable error in the window and Preferences, and allow later saves.
Shutdown does not yet offer a Retry/Keep Open flow when a final write fails.

Native draft tracking uses synchronous text-change notifications so programmatic
formatting can be distinguished from user edits. Numeric fields also retain
paste and accessibility edits without depending on key-down events.

All 12 Windows Rust tests pass, including bounded coalescing, shared request
completion, migration, corruption recovery and a real Windows file-sharing
failure. The rebuilt WinUI app passes isolated restart tests for active text and
numeric drafts, rejection of an invalid closing draft, no restore/save echo,
visible write errors and subsequent recovery, and exact preservation of an
unsupported saved file. Controlled stroke and undo still complete after a save
failure. Existing header/Preferences and workspace fixtures pass; review
processes close with empty runtime stderr.

The storage fixture owns disposable profiles under ignored artifacts and never
uses the normal app profile. The header settings fixture now requires an
explicit isolated profile. Raw settings, recovery files, snapshots, traces and
reports remain private and local. Document/workspace persistence, complete
workspace controls, physical input acceptance, visual parity and packaging
remain open; the 120 Hz benchmark remains deferred.

The subsequent shared project/Apple persistence/Android immersive-display merge
passes all 301 core/engine/UI/host/Windows Rust tests. The hardware D3D12 project
integration test also passes exact live-versus-reopened pixels, including masked
content and subsequent wet painting. The merged native build passes the full
isolated storage fixture plus existing Preferences/header and workspace checks,
with review windows closed and empty runtime stderr. This verifies the shared
project foundation on Windows; Windows Save/Open UI is still unimplemented.

### Native Color panel checkpoint

Windows now exposes the shared Color panel and uses the same retained native
projection in the toolbar color popup. The HSV square, HLS triangle, hue ring,
markers, foreground/background/transparent swatches, swap action and compact
component fields follow shared Rust presentation and action models. The native
pointer adapter uses Rust hit testing and keeps the initial wheel region during
capture; resize, unload, capture loss and color-context changes cancel capture.

Direct2D draws the small gradient image into a WinUI SurfaceImageSource. The
image is cached until hue, space, extent or scale changes; marker motion uses
retained XAML shapes. This adds no continuous presentation loop and does not
alter the canvas swap chain. Surface loss and device errors have a redraw path,
but forced device-loss and mixed-DPI acceptance remain unverified.

The first captured HSV field exposed incorrect mesh interpolation. It now uses
the reference's white-to-hue gradient followed by transparent-to-black. The hue
ring and HLS triangle remain GPU meshes. App-only, uncropped HSV/HLS captures,
including remembered hue with black paint, pass the existing two-level channel
tolerance against the independent shared picker oracle. The four captures check
1,404 interior pixels in total; boundaries and markers are excluded by the
existing checker. This is sampled color correctness, not full-editor parity.

Native UI Automation passes numeric expressions and field retention, exact RGBA
preservation on space changes, stale draft cancellation on slot changes,
swatches/swap, popup/dock synchronization and repeated-tile dismissal. Shared
tile activation now toggles an already-open color/opacity popup on hosts using
that popup model; explicit OpenControl remains idempotent. An unchanged numeric
field no longer emits a redundant edit when focus moves.

The 233 UI/host/Windows Rust tests and five comparator tests pass. The rebuilt
app also passes the existing Preferences, workspace and settings-storage
fixtures, controlled drawing and normal close with empty runtime stderr.
Physical wheel gestures/capture, full connected drawers, complete tool/panel
functionality and whole-workspace visual parity remain open. The 120 Hz
benchmark is still deferred. Captures, settings and raw reports stay local.

After merging the portable source-retention and Android workspace updates, all
304 core/engine/UI/host/Windows Rust tests pass. The shared GPU project fixture
now snapshots assets through CanvasRenderer::source_asset and still passes exact
D3D12 save/reopen and continued wet-paint comparisons. The merged WinUI build
passes all four Color captures and its color, Preferences/header and workspace
interaction checks, then closes with empty runtime stderr.

### Native Tool Set and Tool Settings checkpoint

Tool Set now projects shared tool groups and subtools, including stroke previews,
selection state and non-paint tools. A compact chooser uses the shared command
catalog to keep every drawing tool reachable until Windows toolbar customization
is complete. Tool Settings is available from Workspace and renders shared numeric
schemas, group labels and checkable/enabled command actions.

Value updates retain fields, buttons and the scroll container. Changing a tool,
preset or editing target replaces the field context; detached controls cannot
apply drafts to a new context. Normal focus loss can commit an edit before a
target-changing command executes. The new target's controls then display the
acknowledged shared state.

The long Tool Settings panel uses WinUI ScrollView. The previous ScrollViewer
retained its controls but jumped when a value changed in the scrolled panel;
ScrollView passes the same strict scroll-offset test. No blanket suppression of
native focus scrolling is installed. Numeric controls avoid redundant text writes
and hide the default slider tooltip, which reports normalized positions instead
of the shared numeric units.

Opening Tool Settings and Color together exposed an existing fractional-width
allocation defect: the wheel could be one physical pixel wider than its height.
The wheel now chooses a square device-pixel extent before arranging and drawing.
The combined-panel HSV/HLS and remembered-hue captures pass the original color
tolerance; the checker and reference pixels were not relaxed.

All 233 UI/host/Windows Rust tests pass. Native UI Automation passes all 18
drawing commands (including transform), tool/subtool/schema projection, shared
expressions, retained fields/buttons/scrolling, stale tool draft handling, target
context replacement, gradient and figure choices, ruler toggles and transform
cancellation. Color, Preferences/header and workspace regressions pass on the
same isolated instance, which closes with empty runtime stderr. The combined
layout was also inspected visually.

Physical pointer/keyboard acceptance, full workspace layout parity, connected
drawers/customization, document workflows, lifecycle/device recovery and release
packaging remain open. The 120 Hz benchmark stays deferred. Raw traces, captures,
profiles and diagnostic logs remain ignored and local.

The merged Android drawers/Navigator, Apple document workflows and GTK
application-menu updates pass all 316 core/engine/UI/host/Windows Rust tests.
Capability tests now cover Android's drawers and collapsed columns explicitly;
Windows retains its current popup and dock-handle behavior. Native fixtures read
the shared menu title, now Window, rather than hard-coding its previous label.

The merged WinUI build passes the combined Tool Settings/Color fixtures, existing
Preferences/header and workspace checks, and all five isolated settings-storage
launches. Both hardware D3D12 project tests pass: retained/packed upload sources
and exact save/reopen followed by wet painting. The full renderer suite was not
rerun for this UI checkpoint.

One combined review exceeded the fixture's five-second close wait and then
exited normally without stderr. An uncontended repeat passed, with about 4.9
seconds elapsed for the close fixture including automation overhead. Shutdown
cost remains a lifecycle investigation; this is not presentation or input-latency
acceptance. All owned review processes are closed, and the existing user drawing
window is preserved.

### Windows document transport checkpoint

A dedicated document worker now captures immutable source-project saves through
the shared checkpoint policy. Compression, bounded project reading, atomic file
replacement and isolated GPU preparation run outside the live canvas owner.
The worker uses the same device and queue as the active renderer; only a fully
prepared candidate can replace the document. Retired and rejected candidate
resources return to the worker for destruction.

Dialog responses carry shared request IDs. Open/New and unsaved approvals also
carry the displayed document generation and revision. Intervening changes reject
replacement or close, while an older completed save leaves newer edits dirty.
Cancellation and file failures retire the request without acknowledging a save.
The dedicated FFI entry leaves the existing action/input decode path unchanged.

Validation passes 20 Windows adapter tests and one explicitly selected hardware
D3D12 test. The GPU fixture saves an imported source image, creates a new drawing,
reopens with exact pixels, preserves the live document after corrupt input and
invalid dimensions, and rejects a candidate after a newer edit. It also drives
the live renderer during candidate preparation. The WinUI build succeeds; all
five isolated settings-storage launches pass, including drawing after a storage
failure and normal shutdown with both workers.

This checkpoint provides the transport for the forthcoming native dialogs.
Windows File-menu commands, New/Open/Save dialogs, unsaved window-close handling
and PNG export remain to be connected and tested. Full workspace/input/lifecycle
and release acceptance also remain open. No 120 Hz probe was resumed.

The subsequent integration through shared commit 007284c passes 331
core/engine/UI/host/Windows unit tests. The dedicated hardware D3D12 document
test also passes. Shared typed Windows routing and Apple's delayed estimate
updates now converge on the same pointer policy while preserving estimate
tokens. A new hardware test proves that a correction received after pen-up
changes the original committed point's pressure, tilt and twist without creating
another stroke or acquiring a contact. A unit test rejects typed input from a
retired document or after close authorization.

Windows keeps Tool Settings and Color available; the merged Apple/Android
Navigator and Commands capabilities remain enabled on their implemented hosts.
Legacy popup and dock-handle tests still cover Windows and Web; Apple and Android
use their newer shared drawers and columns.

The merged WinUI build passes tool, Color pixel, Preferences/header and workspace
fixtures, plus all five settings-storage launches. The combined review exceeded
the existing five-second close check, then exited normally with empty stderr.
The simpler settings launches closed within the check. This repeats the earlier
shutdown-cost issue and remains a lifecycle investigation.

The full serial D3D12 renderer run reports 110 passed, 2 failed and 17 ignored
performance tests. The independent Curves ramp case differs by two encoded red
values (102 versus 104); its later Exposure portion was not reached. The strict
v3 filter sheet still reports maximum error 255. Imported sRGB ramp, Halftone
endpoint and the other functional checks pass. Neither reference data nor
tolerances were changed by Windows integration; filter parity remains open.

The 120 Hz display has been reconnected. Presentation/input-latency benchmarking
still awaits the remaining application work, as requested. All review processes
from this checkpoint are closed; captures, test profiles and logs stay local.

### Main branch integration checkpoint

The Windows milestone now integrates upstream through 75c72f8, including the
explicit filter-output storage conversion and watercolor dry-pigment isolation.
The shared unit suites pass all 331 checks, the native WinUI build succeeds,
and the bounded C++ input queue and synthetic presentation-analysis fixtures
pass. Hardware D3D12 tests also pass for background document save/open/new,
stale adoption rejection, delayed pen estimate correction, renderer-retained
source assets and save/reopen followed by wet painting.

The complete serial renderer suite reports 113 passed, one failed and 17 ignored
performance tests. Curves/Exposure scalar checks now pass across six alpha
levels, including the previously failing opaque Curves case. The strict v4
filter sheet still differs, with maximum byte error 30; its one-byte tolerance
and all channels remain intact. See [runtime filter validation](../reference/runtime-filters.md).
This is an outstanding acceptance gate, not a fully passing renderer claim.

Milestones are pushed to both `ports/windows` and `main` so the other ports can
consume shared changes. Incoming changes are merged and checked before each
integration; no force push is used. The public diff contains source, synthetic
tests and documentation. Raw captures, settings, profiles and machine logs stay
local.

Native document dialogs and File menu wiring are the next implementation
milestone. Full workspace parity, physical input, shutdown cost, device recovery,
release packaging and the deferred 120 Hz benchmark remain open. This integration
does not add a new native UI or physical-device acceptance claim.

### Native Windows document workflow milestone

The native File menu now supports New, Open, Save, Save As and Close. New uses
the shared numeric expression and dimension policy; Open and Save use Windows
desktop file pickers. Shared request IDs and document generations guard every
replacement and unsaved approval. Picker cancellation, corrupt input and failed
operations preserve the current drawing and its save checkpoint.

Preferences and document dialogs share a single modal slot. Canvas input stays
disabled until the dialog operation finishes, including its closing animation.
Closing from either the File menu or the window caption follows the shared
Save/Discard/Cancel policy, and pending Preferences drafts are committed first.

The document fixture passes expression validation, Unicode file paths, existing
file saves, Save As, picker cancellation, corrupt-open preservation, save before
open, cancelled replacement, and saved/discarded/cancelled window close. Both
owned document launches exit with code zero within the existing five-second
bound. The native build, bounded C++ input test, 254 host/UI/Windows unit tests,
all five isolated settings-storage launches, and header/Preferences and workspace
fixtures pass before upstream integration.

The earlier notes describing delayed review exits as normal were incomplete.
Native debugging identified an integer-divide fault in Microsoft.UI.Xaml during
late destruction of retained Preferences controls. Shutdown now waits for dialog
coroutines to unwind and releases all retained views while the window's XAML
context is still alive. The fixtures retain the process handle and require a
zero exit status; empty stderr or disappearance of a process is insufficient.
Opt-in local lifecycle traces also reach the normal application return.

Related fixes and checks are kept together and published only at major
milestones, to both `ports/windows` and `main`. Local profiles, synthetic project
files, traces, dumps and binaries remain excluded from GitHub.

PNG export and additional native windows remain disabled until implemented.
Full workspace parity, physical pen validation, broader lifecycle/DPI/device
recovery and release packaging remain open. The previously measured strict v4
filter-sheet maximum byte error of 30 remains unresolved; its one-byte tolerance
is unchanged. No 120 Hz benchmark or input-latency acceptance is added here.

Integration through upstream 80be3c1 preserves Apple's recovery and live
Navigator work, shared missing-toolbar restoration, and reusable GPU transform
uploads/pages. The merged build passes all 332 core/engine/UI/host/Windows unit
tests, 13 targeted hardware D3D12 transform tests, the hardware background
document test, and the bounded native input test. Three transform benchmarks
remain ignored; the full renderer suite was not repeated for this integration.

The merged native build also passes the document fixture's two launches, all
five isolated settings-storage launches, and the combined header/Preferences
and workspace review. All eight owned review launches exit with code zero
within the unchanged shutdown bound. These checks do not add physical-input,
visual workspace parity or 120 Hz presentation acceptance.

### Windows PNG export milestone

The native File menu now enables Export PNG with the shared filename, action
label and PNG file filter. Picker cancellation leaves the drawing, its dirty
state and its project destination intact. Export completion never acknowledges
a project save.

The canvas owner defers capture until staged shaders and document edits are ready,
then submits an independently owned GPU snapshot after presentation. GPU waiting,
row packing, shared sRGB PNG encoding and atomic destination replacement run on
the existing document worker. A document changed before deferred capture is
rejected with a retry message; edits after capture cannot change the snapshot.
No viewport zoom, rotation, cursor or native chrome enters the exported image.

Validation includes three adapter cases for cancellation/invalid destinations,
mismatched responses and stale deferred capture. The hardware D3D12 fixture
decodes a 63 by 47 RGBA PNG byte-for-byte, including transparency and the sRGB
tag, while the viewport is smaller and rotated. It verifies later edits,
preserved project checkpoints, a locked destination preserving its previous
bytes, retry, temporary-file cleanup, and a detached ticket completing on a
worker after the live renderer is destroyed.

The native document fixture also drives Export PNG through the real Windows
picker, cancels and accepts Unicode paths, checks the full document dimensions,
and confirms that unsaved status and the project destination survive export.
The surrounding New/Open/Save/Save As and close decisions continue to pass.

This adds PNG export to the existing single-window document workflow. Additional windows, full
workspace parity, physical input, broader lifecycle/DPI/device recovery and
release packaging remain open. The strict v4 filter reference difference and
deferred presentation/input-latency gates are unchanged.

A final native regression caught a transient access denial replacing a newly
picked project's empty placeholder. The live drawing was preserved and the
destination allowed exclusive access when inspected afterward; the process
holding it during failure was not identified. Windows App SDK placeholder
creation is also reported [upstream](https://github.com/microsoft/WindowsAppSDK/issues/5976).
The file worker now retries Windows access/sharing/lock errors for at most one
second between attempts, checking cancellation each time. It keeps the flushed
temporary file and never falls back to truncating the destination. An actual
Windows file-lock test verifies release/retry, cancellation and cleanup, while
the GPU test retains its persistent-lock failure assertion.

FFI safety documentation now records host ownership, the resize handoff, service
callback lifetime and returned-string ownership. Strict Windows adapter Clippy
passes without suppressed warnings.

Final validation passes all 336 core/engine/UI/host/Windows unit checks, both
hardware D3D12 document tests, strict Windows Clippy and the native WinUI build.
Two consecutive native document/export fixture runs pass all nine checks each;
their four owned launches exit with code zero within the existing shutdown
bound. The full renderer suite and presentation probes were not repeated.

### Live Windows Navigator and minimized-window decisions

Navigator now projects native WinUI camera controls and pointer capture while
sampling the live composition in the canvas's existing GPU presentation pass.
Shared geometry drives both the XAML image cutout and the GPU image/outline,
including document aspect changes, camera rotation/reflection and display scale.
Placements are bounded and validated atomically; unchanged layout stays idle.
Overview resources are prepared before painting becomes available and reused
across updates. No preview bitmap or CPU canvas readback is added.

The native panel retains controls during camera updates, document replacement
and resize. Preview height and control spacing follow Android's panel, with
shared palette colors in both themes. A light-theme surround mismatch found by
visual review is covered by an app-only pixel assertion. Windows still uses its
existing dock preset until Properties, Filters, columns and drawers are complete.

Minimized close decisions now restore the owner before showing UI. Actual native
testing found that OverlappedPresenter.Restore lost the previous maximized state;
SW_RESTORE preserves it. The fix is limited to operations requiring a decision
or picker, leaving existing-path background saves undisturbed. Clean minimized
close already worked on the tested hardware; no speculative render-loop rewrite
was made.

This milestone integrates main through 5f0cddd, preserving the full web editor,
Apple independent windows, Android header status and reorganized documentation.
The combined tree passes 337 core/engine/host/UI/Windows unit checks, the bounded
native queue test and strict Windows adapter Clippy with --no-deps. The three
shared overview checks pass on hardware D3D12, including live paint, alpha,
clipping and GPU resource reuse. The hardware PNG export regression also passes
after upstream's shared readback changes.

Native fixtures pass all six Navigator commands, preserved document state,
retained controls, visible stroke pixels with exact Undo restoration, adoption
of a different-aspect document, resize, hide/reopen and both themes. Minimized
lifecycle checks cover visible unsaved decisions, Cancel, maximized-state
preservation and Discard. The document/export picker regression also passes.
All five owned launches exit with code zero within the unchanged shutdown bound.

The updated web application builds and runs in local Windows Chrome with hardware
WebGPU. Fresh 1200 by 900 references in both themes complete without runtime
errors. These are runnable comparison references, not a full workspace visual
parity pass. The capture helper validates the canonical owned temporary profile
before recursive cleanup. Profiles, images and diagnostics remain local.

The full editor workspace, physical input (including Navigator gestures),
mixed-DPI/device recovery, additional native windows, packaging and release
acceptance remain open. The strict v4 filter-sheet difference and deferred
120 Hz/input-to-present gates are unchanged; no presentation benchmark ran here.

Before publication, the Apple editor-preset milestone 9a752ea was also merged.
Its shared layout change enables the full preset for Apple platforms and leaves
the Windows preset unchanged. The affected 245 UI/Windows unit checks, rebuilt
native app and complete Navigator fixture pass after that merge; the additional
owned review exits with code zero.

The final publication also integrates the documentation update 77661f0 and
GTK/watercolor milestone a70abfb. Its new selected-watercolor mixing test and
existing unselected-wet-paint preservation regression both pass on hardware
D3D12. The rebuilt native app again passes the complete Navigator fixture and
exits cleanly. These targeted checks cover the incoming shader change without
repeating unrelated rendering benchmarks.

### Native Windows Properties and GPU filter picker

The native Properties panel now projects all six shared property kinds: numbers,
choices, toggles, RGBA colors, curves and gradients. Rust remains responsible for
numeric expressions, sampled curve plots, effect values, point/stop constraints,
insertion, Undo and locked-layer enablement. Native point and stop editors retain
their graph controls across value changes and resize. Draft callbacks are bound
to the document epoch, layer, schema and selected point/stop generation, so a
reset or replacement cannot redirect an old edit into a different value.

Filters now follows Android's category/search and preview-row layout. Visible
rows request at most eight shared GPU previews, at most every 200 milliseconds.
A separate single-slot mailbox runs after painting and yields to queued input;
it neither fills the input queue nor waits for a GPU readback. An owned binary
atlas crosses to a worker for straight-RGBA to premultiplied-BGRA conversion.
WinUI bitmap creation happens on its dispatcher. The cache retains at most 64
rows (16 MiB at maximum tile size), rejects obsolete revisions and abandons
pending requests when document state changes. Request/revision identities cross
the bridge as strings, including the document epoch.

The Rust adapter's 27 unit tests pass (two explicit GPU tests remain ignored in
that command), and strict adapter Clippy with --no-deps passes. The native build
and isolated effects fixture pass all six property kinds, point/stop reset draft
guards, disabled endpoint coordinates, retained curve controls during edits and
resize, category/search/insertion, GPU preview pixel changes after a controlled
stroke with exact Undo restoration, both themes, document replacement, clean new
document state and exit code zero. Screenshot/profile/report output stays local.
The app-capture helper now also accepts absolute output paths.

This is a Properties/Filters milestone, not full workspace parity. Runtime filter
package import, full Layers operations, application menus, columns/drawers and
the full Windows editor preset remain pending. Physical curve/gradient pointer
gestures, broader DPI/device recovery, release packaging and deferred 120 Hz and
input-to-present acceptance remain open. The strict v4 filter reference
difference is unchanged.

The publication integrates main through d8a130b, including Apple editor/header
work, GTK/Web fullscreen and status controls, shared clock settings and the GPU
Navigator outline contrast correction. Windows now joins retired shader workers
after all hosts are destroyed and before process teardown. Instrumented testing
found a six-second join during unoptimized WGSL compilation; the development
profile now optimizes Naga while leaving application Rust debuggable. Startup,
shader-warmup, clean minimized and dirty close fixtures pass the unchanged
five-second zero-exit requirement with that profile. The worker is still joined;
no forced termination or timeout bypass was added.

The final tree passes 341 core/engine/host/UI/Windows unit checks, strict Windows
adapter Clippy, the native build, three hardware D3D12 filter-preview regressions
and four overview regressions. Preview checks cover insertion pixels, cached
results independent of the view, the empty-document sample and cropped
multipass output matching the full canvas. Their timing benchmarks stay ignored.
The shared canceled-worker join test also passes.

The merged native effects fixture passes again, including rapid search edits
through a theme change. The field now retains a typed draft until its shared
model acknowledgement and uses synchronous text-change suppression during
programmatic updates. Navigator passes all prior controls/pixel/resize/theme
checks after the incoming outline change. Final lifecycle checks include a
close requested before brush readiness, close during speculative shader warmup,
minimized clean close, Cancel preserving the drawing and previous maximized
state, and dirty Discard. Captures and temporary pipeline diagnostics remain
local; the temporary instrumentation was removed before publication.

## 2026-09-11: native Layers editing and binary thumbnails

Replaced the provisional two-button layer list with retained, virtualized WinUI
rows following the Android panel's spacing, indentation, icons and editing
markers. Separate selection and content/mask editing targets, visibility, blend,
opacity, alpha/edit locks, clipping, references, masks, rename, grouping,
collapse/expand, duplication, deletion and recursive shared context menus are
projected natively. Drag-and-drop forwards row position to shared Drop policy,
with exact group thresholds, a paper anchor, edge scrolling and cancellation
that survives recycling the source row. Physical drag acceptance is still open.

The optional query transport now admits bounded thumbnail and filter work
together, prioritizes menus, and replaces obsolete queued menu requests. It
does not consume reserved input command capacity. Thumbnail requests and
readbacks are bounded at eight, CPU conversion runs off the UI thread, and the
cache retains at most 128 images. The shared host's existing thumbnail query was
factored into an owned-image method; its JSON response remains compatible with
other ports. Windows keeps pixel bytes out of JSON.

The footer resolves its menu target on the canvas owner after earlier selection
actions. Widget actions and effect drafts carry a document epoch checked on that
owner, so an obsolete queued edit cannot target a replacement document. Native
workspace popups participate in chrome state. Numeric and rename fields commit
in synchronous LosingFocus handlers; close moves focus before asking shared
policy whether the document has unsaved edits.

The adapter's 31 unit tests pass, with two explicit GPU tests ignored, and strict
adapter Clippy with --no-deps passes. Queue tests cover ordering, reserved input
capacity, retained allocation bounds, refusal ownership, latest-menu replacement
and callback disposal. The native Layers fixture passes real app thumbnail
changes after replayed paint with exact Undo restoration, retained rows, header
locks and values, independent selection, mask target/link/enable state, clipping,
references, rename/duplicate/delete/Undo, grouping without redirecting the
editing target, edits inside a collapsed group, and ungrouping. It creates 68
layers while checking fewer than 40 native rows are realized, scrolls back to
older thumbnails, switches theme, replaces the document, commits a focused
numeric draft before close, cancels that close and then discards with exit zero.
The scroll presenter is named after its template becomes available. All
screenshots, profiles and diagnostics remain local and ignored.

This is a Layers editing milestone. Image-as-layer import, runtime filter
package import, full editor columns/drawers/expansion/docking, remaining command
coverage and native packaging are not complete. The full Windows editor preset
remains gated. These checks do not establish physical drag/pen/touch behavior,
full matched-state visual parity, 120 Hz painting or input-to-present latency.

The publication integrates main through ec71032, including the shared collapsed
column gesture correction, Tool tab naming, diagnostic row order, header battery
artwork and bounded Ripple phase. The final tree passes 279 UI/host/Windows unit
checks (three explicit GPU tests ignored), the native build and the complete
Layers fixture. The preceding merge also passes strict Windows adapter Clippy,
the hardware D3D12 spatial-filter linear-sampling oracle, all native effects
checks and startup, shader-warmup, minimized clean and dirty close lifecycle
checks. The last incoming change affects shared column resize policy; all shared
UI tests and the native Layers fixture pass again after that integration.

## 2026-09-11: native image-as-layer import

The Layers footer now opens the Windows App SDK image picker. Its request is
created on the canvas owner after preceding native draft commits, capturing the
document epoch, revision and editing layer without passing those identities
through floating-point JSON. Only the request ID and picker/worker status are
published to the UI; source paths stay inside the document job.

The existing bounded document worker uses Windows BitmapDecoder to request
straight RGBA8 pixels with EXIF orientation and conversion to sRGB. Source and
oriented dimensions are checked before pixel decoding, against both 8192 pixels
and the current device limit. A worker apartment owns all decoder objects.
Cancellation and per-stage deadlines are checked while native async work runs.
Packed pixels cross to Core as an immutable ProjectAsset; pixel copying and
disposal of rejected images stay off the canvas owner. Core retains placement,
selection, GPU asset upload, Undo and project embedding.

Cancel and decode failure preserve the drawing. Changed documents or editing
targets reject late completions. Duplicate and obsolete picker responses cannot
replace or cancel another request. New/Open/Save/Export/Close supersede pending
imports; their native dialogs wait for decoding to release the single worker
slot. Import progress and recoverable errors use the existing status area.

All 39 adapter unit checks pass, with two explicit GPU tests ignored by the
ordinary command. Real Windows codec fixtures cover straight alpha, grayscale,
16-bit PNG, EXIF-rotated TIFF, oversized/corrupt/missing files, private-path-free
errors and cancellation between native async stages. Service checks cover
lossless request IDs, cancellation, invalid destinations, changed revisions,
epochs and editing targets, close, and subsequent document requests.

The explicit hardware D3D12 document fixture now imports a generated PNG through
the worker, verifies exact Undo/Redo pixels and rejection after intervening edits,
and saves/reopens the drawing after deleting the source image. The native
document fixture drives the real picker, a focused opacity draft, Cancel,
corrupt-image recovery, selected image rows, ready thumbnails, Undo/Redo and
embedded project reopening. Its existing New/Open/Save/Save As/PNG/Preferences
and saved/untitled close checks also pass. Strict adapter Clippy and the native
build pass. Temporary compiler diagnostics were removed before publication.

One earlier untitled-close sample exceeded the five-second exit gate. Local
lifecycle records place 5.7 seconds in the final retired shader-worker join,
after the document worker and window had finished. An instrumented repeat and
the final normal build's document and four lifecycle scenarios pass, but do not
explain that intermittent sample. Cold-compilation close timing remains an open
acceptance issue; the timeout was not increased and shutdown still joins work.

Runtime filter package import, full editor columns/drawers/expansion/docking,
remaining native panels/commands, physical input, full visual parity and release
packaging remain open. The full Windows editor preset stays gated. Sustained
120 Hz painting and physical input-to-present latency remain deferred until the
application is otherwise complete. All generated images, projects, profiles and
diagnostic output remain ignored and local.

The publication integrates main through 43f9dcf, including reversible column
resize gestures and bounded GPU submission chunks for large document replay.
The merged tree passes 290 UI/host/Windows unit checks, strict Windows adapter
Clippy, both explicit hardware D3D12 document tests, and the release-mode 4K
seven-layer replay regression with exact pixel equality. The native build and
document, effects, Layers and all four lifecycle fixtures pass after the merge.
The intermittent shader-close sample above remains an acceptance gap.

## 2026-09-11: workspace projection, work in progress

Panel bodies now have independent native widget ownership and share the
existing per-window preview caches. The header reads the eight shared menus.
Diagnostics reads the shared StatsView through a bounded read-only query route,
with weak UI callbacks, one outstanding request per view and no GPU wait or
pixel transport. Its first request must be allowed before an empty panel has a
natural height. Native colors use the shared settings_secondary palette role.

A stable workspace capture owner routes panel, group, tile and resize gestures.
It keeps capture when shared layout reparents source controls, requests measured
tab drop hints, coalesces hint queries, and cancels on Escape, capture loss,
deactivation or close. The owner retains native chrome geometry between
high-rate canvas events. Floating resize strips do not participate in the body
structure key, avoiding rebuilds merely from moving a floating group.

Help link commands are enabled only after adding asynchronous Windows launcher
handling and completion acknowledgement. Link URLs remain defined in Rust.
The optional query route accepts explicit geometry/menu/statistics/link
operations and rejects package installation, actions and oversized payloads.
Stale query errors return to the native view without stopping the canvas.

The adapter's 42 unit tests, 236 shared UI tests and 15 host tests pass; three
explicit GPU tests remain ignored by the ordinary unit command. Strict Windows
adapter Clippy, bounded queue checks and the native build pass. The workspace
fixture checks the eight menus, selection and About, actual Diagnostics
submission values, retained rows, resize, visibility and workspace Undo. The
Layers, effects, documents and all four lifecycle fixtures also pass after
adding the gesture owner. Those fixtures use native UI Automation and controlled
canvas replay, so they do not accept physical docking or tablet input.

A subsequent workspace repeat after enabling Help links completed its functional
checks but exceeded the five-second process-exit gate. The native window closed
about 90 ms after authorization; the final retired shader-worker join took
5.26 seconds. The process exited later. This reproduces the existing intermittent
shutdown issue; no timeout was increased and that run has no accepted zero-exit
result.

This is unfinished workspace work, not full editor acceptance. Windows still
uses its gated legacy preset. Configuration/toolbar management, partial Zen,
native measurement and physical gesture validation remain, alongside the
earlier packaging, lifecycle, visual and performance acceptance gaps.
Presentation benchmarking remains deferred.

Content drawer and collapsed-column projections now follow shared geometry.
Opening and closing retain native body ownership while the owner computes the
200 ms placement animation. Native measurements include one initial zero height
per drawer column, followed by the actual body heights. Reporting an empty
height array made the shared query return no placement; that startup case is
now covered by the adapter query test. Early chrome refreshes also wait for
nonzero native layout before sending a viewport.

Tool drawers report their presented bounds and connection to shared chrome.
Column drawers report clipped toolbar tile origins, retain body controls across
tab switches and use native scrolling. The drawer backdrop leaves a GPU
Navigator opening. Column icons, expansion and keyboard context menus route
shared actions. Size presets follow Android's two/three/four-column breakpoints.
All tile sizes use the new shared icon and label metadata.

The native drawer fixture completed Color and Brush Size repeat toggles,
keyboard collapse, expansion and workspace Undo/Redo. A review closed its
window about 92 ms after authorization but spent 7.69 seconds in the final
shader-worker join; the five-second process-exit gate remains failed for that
run. The later process exit was observed without an accepted exit status.
A broader fixture adds tab-width/selection and Navigator checks; its first
attempt requested a context menu while the previous native flyout was closing.
The fixture now allows that native close animation to settle.

Main through fd1ade7 was fast-forwarded into the Windows worktree, preserving
uncommitted Windows work. The overlap in column-history tests was resolved by
keeping the new upstream width-reset coverage and the Windows collapse/history
test. The merged tree passes 304 unit tests (246 UI, 15 host, 43 Windows) and
strict Windows adapter Clippy. This remains one unpublished workspace milestone.
The merged native build and complete drawer fixture now pass, including
keyboard menus, Color and Size repeat toggles, expansion history, retained
column width through tab selection, repeated active tabs, Navigator preview and
zoom, closing cleanup and a zero exit within five seconds. The tab projection
uses the typed SelectPanelTab action; the shared collapsed-column policy keeps
repeat selection from opening configuration. A local capture shows the actual
controlled stroke in the GPU Navigator preview, and the native Border applies
the shared rounded/connected corners to its child content. This passing repeat
does not resolve the earlier intermittent shader shutdown delay. The 33 changed
source files have no high-signal secret matches; captures, profiles and logs
remain ignored and local.

At that point, column drawer dragging/MeasureColumnDrawers, canvas-facing
divider reset, origin corner clipping and overlapping GPU preview acceptance
were still pending, in addition to the unfinished full workspace work above.

The Windows workspace drawers and toolbar management milestone now includes
native creation/insertion, rename, duplicate, management and delete-confirmation
dialogs. Lists are virtualized and retain rows through selection updates.
A synchronous TextChanging handler captures drafts before a delayed snapshot
can overwrite newer typing; native cancellation waits for Core acknowledgement
before releasing the modal slot. The event choice follows
[WinUI's documented event ordering](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.xaml.controls.textbox.textchanging?view=windows-app-sdk-1.8).
Toolbar grips accept keyboard context requests. Dock tabs, drawer headers and
collapsed icons update their labels after rename or tab-presentation changes.

Column drawers publish presented bounds through MeasureColumnDrawers, route
individual tab and whole-group drags, and clip drop-hit rectangles through all
native scrolling ancestors. The fixed group grip remains outside scrolling
tabs. Canvas-facing column dividers request shared default-width reset on
double-click. Windows joins the shared tests for drawer tear-off, cancellation,
docking, tab reordering, stale measurements and one-entry reset history.
Native physical dragging is still unaccepted.

Main through e1b1fe0 merged cleanly. The merged tree passes 304 unit tests
(246 UI, 15 host, 43 Windows; three explicit GPU tests ignored), strict Windows
Clippy, native queue tests and the C++ build. Native toolbar and drawer fixtures
pass on the final build, including rapid names, rename/duplicate/insert history,
delete/cancel/manager handoffs, closing with a picker open, keyboard drawer
menus, tab width, GPU Navigator preview and zero exit. The workspace/Diagnostics,
Layers and effects fixtures also passed in this milestone. The effects fixture
now retains the last complete isolated, process-matched trace during a partial
trace read; new-value waits still keep their existing timeout.

Document regression reached the second launch's final discarded close after
its document, import, export, Preferences-draft and picker checks. That process
exceeded the five-second exit gate: its window closed about 72 ms after
authorization, and the final shader join took about 4.83 seconds before the
remaining process teardown. The lifecycle fixture passed immediate startup
close but failed warmup close, where the final shader join took about 5.53
seconds. Both processes later exited; those runs have no accepted zero-exit
result. These reproduce the existing shutdown limitation. No timeout was
increased, and the document/lifecycle suites are not marked passed for this
milestone.

This is a workspace implementation milestone, not complete Windows acceptance.
Configuration expansion, full editor defaults, partial Zen, origin corner
clipping, overlapping GPU previews, runtime filter packages, packaging,
physical gestures, DPI/device lifecycle, Chrome parity and final presentation
and input-latency gates remain. Captures, profiles, traces and binaries remain
ignored/local; only reviewed source, fixture and documentation changes are
included. Presentation benchmarking remains deferred.

Before publication, main advanced to 16c5886 with Apple drawer work and shared
toolbar-height, tab-drop ordering and collapsed-layout fixes. That update
merged cleanly. The resulting 307 unit tests (249 UI, 15 host, 43 Windows),
strict Windows Clippy and native rebuild pass. Toolbar and drawer fixtures pass
again on that merged build. The toolbar fixture also closes a modified drawing
while its picker is open, verifies the unsaved-decision handoff and exits with
zero status. The earlier document/lifecycle shutdown failures remain open.

The next workspace milestone implements native panel configuration expansion.
The shared CPU query supplies both columns, animation retargeting, tile wrapping
and joined corners; retained preview widgets move without being recreated on
each geometry update. The configuration uses shared titles, hints, control
visibility and toolbar actions, 12 DIP outer padding and 12/6 DIP section/control
spacing. Numeric controls, brush presets, color, tool settings, properties,
filters, statistics, Navigator and layer controls reuse native implementations.
Layer selection/opacity carry the editing target and document epoch.

A visual review caught two defects before publication: aggregate padding
initialized only its left edge, and the shared button tint was mistakenly
opaque. Explicit four-edge padding and the shared 13/255 tint now match the
Android configuration. A second capture exposed lower XAML panels appearing
through the GPU Navigator opening. Native compositor geometry now subtracts
higher preview rectangles from lower workspace visuals, preserving native paint
order (including equal-z child order) and restoring clips when previews close.
The path uses Direct2D geometry through
[CompositionPath](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.composition.compositionpath.-ctor?view=windows-app-sdk-1.8);
it adds no CPU image readback or separate presentation surface.

Main through afa058a merged without disturbing the Windows changes. Shared
snapshot streaming, collapsed-column fixes and the move of theme selection
into Preferences are included. The merged tree passes 312 unit tests
(250 UI, 18 host, 44 Windows; three explicit GPU tests ignored), strict Windows
Clippy and the native build. Bridge coverage checks retained expansion geometry
across resize/close and shared toolbar wrapping through the actual metadata
packet interface. The final expansion, drawers, toolbar management and Layers
fixtures pass, including zero process exit. Expansion checks shared width and
narrow resizing, retained controls, live brush values, visibility, layer
selection/targeted opacity, Escape, toolbar insertion and both themes. Its pixel
check verifies a GPU preview over a lower opaque panel and exact restoration
after closing. The merged Layers fixture now scopes its Preferences Close
button to that dialog, avoiding the identically named window caption button.

Before publication, main advanced to 2fbcc23 with GTK/Web and Android workspace
cursor updates. Those changes merged cleanly and do not modify the validated
Windows or shared Rust sources.

This remains partial Windows acceptance. Full editor defaults and native panel
measurement, partial Zen, full Chrome parity, runtime filter package import,
packaging, physical gestures, device/DPI lifecycle and final presentation/input
latency gates remain. The earlier intermittent final shader-worker shutdown
delay is still open. Captures, profiles, logs and binaries stay ignored/local.

## Full editor, measurements and native titlebar-aware Zen

The Windows host now initializes the full platform workspace before optional
restoration, following the Android host's startup sequence. Enabling the preset
alone was insufficient because NativeHost starts with generic defaults.
Tools and Commands expose the complete shared command set; Tool Set no longer
contains the temporary chooser. Native tab widths and scroll-content extents
feed Core's floating-panel and tab sizing. Reports are coalesced after arrange,
retain inactive measurements and avoid realizing the full Layers list.

Partial Zen retains native toolbar controls while Core supplies section
splitting, placement, tile sizes, clipping and drawer anchors. Screenshot
review caught an overlap with Windows caption buttons. A transient shared
titlebar measurement now reserves the actual native insets and caption height.
Window-drag regions exclude the resulting Zen controls; unchanged regions
are cached so painting updates do not repeatedly publish non-client geometry.
The titlebar facts are validated, absent from durable history, and retained
through workspace undo, reset, adoption and drag cancellation. This continues
to use the existing GPU canvas and presentation surface.

The integration includes main through f6c58a7. The shared history work required
explicitly stripping titlebar facts from durable revisions while retaining
them in the current window. The new workspace storage/manager infrastructure
is available on main; this milestone does not wire the Windows host to that
store. GTK/Web attached-tab drag changes and Android drag pacing are included.

The final shared tree passes 323 unit tests (259 UI, 18 host, 46 Windows;
three explicit GPU tests ignored) and strict Windows Clippy. Core/bridge
coverage checks startup versus saved layout authority, titlebar validation,
serialization and history, and matching Zen tile/anchor/hit geometry outside
caption controls. Native editor coverage includes settled measurements,
all tools and their changing schemas, numeric expressions, retained controls
and scrolling, editing-target guards, partial/total Zen, color drawers,
resize, restored layout, both themes and the five-second process-exit gate.
Top Zen tiles return client hit results through `WM_NCHITTEST`.
The Layers regression also passes thumbnails/undo, virtualization/recycling,
masks, independent selection, group operations, theme and document replacement,
and focused-draft handling before close.

The local Chrome reference build runs with hardware WebGPU and no page errors.
The native reference fixture measures and adjusts to the same 986 by 658 DIP
viewport, records display scale/client capture size, and invokes Fit canvas.
At 1.5 scale the native XAML viewport is 1479 by 987 pixels; its uncropped client
capture has one additional physical row, recorded rather than silently removed.
Visual review confirms the shared outer geometry and identifies remaining
differences in Tool Set button arrangement, header spacing, grips, disabled
icons and property/layer controls. This is not full pixel parity acceptance.

Physical pen history/pressure/tilt/eraser, all workspace gestures and clips,
device/DPI lifecycle, runtime filter packages, packaging, workspace storage
integration and final presentation/input-latency gates remain. The intermittent
shader-worker final-join delay is still open. No new presentation benchmark
was run, and no private captures, profiles, logs or binaries are published.

## Native editor styling and attached tab dragging

The desktop header now uses shared menu spacing and centers the document in the
remaining space before the Windows caption controls. Drag regions are computed
after native arrange, including when theme changes replace the header controls.
Caching natural title width avoids changing text measurement from hit-region
queries. Native checks cover menu/settings client hits and unused draggable
space in both themes, alongside the existing Zen caption exclusions.

Panel grips use the shared SVG, orientation and inset. Active tabs have joined
six-DIP shoulders; drawer tabs respect shared icon/name visibility. Disabled
command and numeric-step icons use the shared dimming. Layer opacity keeps its
slider beside the numeric entry, with editing-target and lock guards. Property
choices use the GTK/Web horizontal label/control row. Tool Set retains the
GTK/Android full-width brush preview; that arrangement currently differs from
the Web reference. Local captures verify both themes and record the native
viewport's one-physical-pixel client origin offset.

Attached tab previews use Core's frozen slots and insertion thresholds. Native
Composition animations move neighboring copies without moving their original
hit rectangles. Windows transfers the contact from the source Button after
drag slop and claims the source scrolling content for that gesture. Retaining
the tab ScrollViewer and its content across panel-body replacement prevents
capture loss during tear-off. Down and the shared BeginTabDrag action are
dispatched together; the shared workspace_drag_preview query supplies both
tab motion and drop hints.

The OS-touch fixture checks two grab positions, fixed hit rectangles, release
insertion, attached/detached cancellation, continued floating movement after
an 800 ms hold, workspace Undo and clean exit. A separate contact timer maintains
injected hold frames while UI Automation or capture blocks the observation
thread. This remains synthetic input, not physical digitizer or latency
acceptance. Temporary native input diagnostics were removed.

Main through 08a0c15 is integrated. The final shared Windows tree passes 328
unit tests (260 UI, 20 host, 48 Windows; three explicit GPU tests ignored),
strict Windows Clippy and the native build. Editor, tab-drag, Layers, drawers,
effects and configuration-expansion fixtures pass, including their process-exit
gates. The latter fixtures now use the full editor preset: they distinguish
permanent panels from drawers, select Properties only when needed, and constrain
the window to establish the required GPU-over-opaque-panel pixel comparison.
The final upstream Apple/GTK-only update does not change the validated shared
Rust or Windows sources.

This is an editor styling/interaction milestone. Windows adoption of shared
incremental workspace publication, workspace store/manager integration, complete
gesture/scroll/overlap parity, multiwindow support, runtime filter packages,
DPI/device lifecycle, packaging and physical input remain. The intermittent
shader-worker shutdown delay and strict GPU filter-reference mismatch are still
open. Final painting presentation and input-latency acceptance remain deferred.
Private captures, profiles, traces, logs and binaries remain ignored/local.

## Retained native workspace motion

The Windows bridge now serializes NativeHost::take_update_bytes directly and
appends escaped Windows metadata without constructing another tree of UI models.
A full snapshot establishes the model revision; later workspace updates apply
absolute geometry against that retained revision. A three-slot mailbox
keeps full models, motion and camera updates separately. A later motion without
a camera cannot erase an earlier camera change. Full completion/cancellation
snapshots discard older presentation. Input actions still use the ordered queue.

Floating panel frames and resize grips retain native translation transforms.
The transform moves rendered controls and their hit/clip coordinates without
rerunning content layout. TransformToVisual gives GPU overview allocations in
the same coordinates; the native transparent holes and compositor occlusion
are updated with those placements. This uses the existing canvas swap chain.
Attached-tab previews and drop hints consume the shared update stream instead
of issuing a query for each motion. Toolbar-tile drop queries remain separate.

The OS-touch fixture caught a fast tear-off capture failure: the first move's
OriginalSource could already be outside the source tab. Windows still held the
Button capture, but the adapter looked for it along the new source's ancestors.
The adapter now retains weak references along the original press path and
transfers that contact after drag slop. Preview hiding retains the local grab
until gesture completion, so a queued pre-Down snapshot cannot discard it.

The complete native motion fixture passes attached reorder, two grab positions,
fixed hit rectangles, release/cancel, held floating movement, retained tab and
numeric controls, native resize-grip movement and workspace Undo. A fast
Navigator tear-off additionally checks matching shared/native/GPU positions,
unchanged models through a mixed camera update, retained Navigator controls,
GPU pixels over an opaque lower panel at two positions, cancellation and lower
pixel restoration. All review input is OS-injected touch. Local screenshots
were visually inspected; these checks do not establish physical pen behavior,
full visual parity or input/presentation latency.

Main through 3e14bab is integrated. The shared Windows tree passes 334 unit tests
(264 UI, 20 host, 50 Windows; three explicit GPU tests ignored), strict Windows
Clippy and the native build. Native queue/mailbox tests verify stale revision
rejection, matching retained models, camera preservation and completion ordering.
The Windows serialization fixture compares 32 floating moves with the
compatibility wire format, including mixed camera, final release/cancellation
and Undo/Redo, and requires incremental traffic below one tenth of full models.

The editor, Layers, drawers, effects and configuration-expansion native suites
also pass, including their five-second zero-exit gates. Four shared overview
regressions pass on the Windows D3D12 adapter; the explicitly ignored presentation
benchmark was not run. The final Android/Web-only integration leaves the
validated shared Rust and Windows sources unchanged.

Workspace store/manager integration, full gesture/scroll/overlap parity,
multiwindow, runtime filter packages, DPI/device recovery, packaging and physical
input remain. The intermittent shader-worker shutdown delay and strict GPU
filter-reference mismatch remain open. Final painting presentation and input
latency benchmarks are deferred. Private captures, profiles, traces, logs and
binaries remain ignored/local.

## Windows workspace persistence and recovery

Windows now uses the shared WorkspaceManager and native SQLite worker for its
active workspace. Layout history and latest tool settings survive restart;
preferences remain in their separate private settings file. No previous Windows
layout store existed to migrate. Startup prepares and adopts a saved capture at
the shared idle/active-brush boundary, without waiting for optional shader work.
The full Workspaces / Saved Layouts / History UI remains the next milestone,
using the approved host handoff integrated through main 007c3e9.

The canvas owner retains non-Send manager futures and polls them only after a
wake. Database work runs on StoreWorker, with no per-frame disk I/O. A separate
service observation accumulator survives UI snapshot publication. Committed
layout generations trigger full captures; ordinary tool edits update working
state. Immutable saves preserve newer accepted edits, and idle service polling
can publish status without forcing a GPU frame. Ownership renewal uses the shared
lease policy. Expiry cancels live input and rejects queued editor mutations;
preferences acknowledgements and native measurements can still complete.

WinUI recovery controls offer Retry, Save as New Workspace and export of the
current in-memory workspace. Unavailable storage preserves the original database
and offers retry/database backup. A failed close retains the window until Retry,
Keep open or explicit Close without saving. Normal close captures the final edit,
flushes and releases ownership before teardown. Retained wakers are disarmed
before their native callback context expires; the last SQLite client drains
accepted requests and joins the worker. Native backup flush now uses a writable
Windows handle, and exports reject the live database and WAL/SHM destinations.

The integrated editor fixture exposed a measurement acknowledgement regression:
adopting a saved layout discarded transient measurements, while identical retained
native sizes suppressed a resend. Full model application now schedules measurement
reconciliation even without another LayoutUpdated event. Controls remain retained.
A separate document-worker regression found a lost shutdown notification between
the stopping-predicate check and condition-variable wait. Changing that predicate
under the mailbox mutex fixes the hang; the existing import-completion teardown
fixture exercises 100 iterations. This is separate from the still-open occasional
shader-worker final-join delay.

Main through 007c3e9 is integrated. The combined tree passes 383 unit tests
(264 UI, 24 host, 62 Windows, 33 workspace; three explicit GPU tests ignored),
strict Windows Clippy, native queue/mailbox tests and the Rust/C++ build. The
isolated persistence fixture passes autosave, restored layout/tool values,
measurements after adoption, final-edit close, unreadable storage, editing guards,
Keep open, retry after repair, explicit discard, original-file preservation and
zero-exit shutdown. Its recovery dialog capture was visually inspected. The editor
and OS-injected tab-drag suites pass, including retained controls, titlebar hit
regions, both themes, matching GPU Navigator motion and five-second exit gates.

This milestone does not establish full manager or multiwindow behavior, complete
visual/gesture parity, physical input, DPI/device lifecycle, distribution packaging,
or final painting/presentation latency. The strict GPU filter-reference mismatch
and occasional shader-worker shutdown delay remain open. Final 120 Hz painting
and physical input-to-present benchmarks remain deferred. Private profiles,
databases, captures, traces, logs and binaries remain ignored/local.

## Native task workspace manager milestone — 2026-09-12

Integrated main through e21850a, including the approved workspaces-only product,
three stable task workspaces, renamed default identities and the preserved-layout
Photographer upgrade. Windows now projects the shared manager into retained WinUI
lists, name-only prompts and layout history. The header pill follows current names
and actual adoption, uses the shared reference's muted selection tint, and bounds
long labels before they overlap menus or caption controls. Legacy saved-layout
commands have no native dialog route.

Selection, Enter and double-click preview the live editor without changing its
durable capture or tool values. Explicit confirmation adopts the selection.
Cancel, Escape, stale reads, filtering away a selection and normal window close
restore the original arrangement. New Workspace copies current working values and
starts independent history. Restore Starting Layout and history restoration keep
current tool values. Reset All Brushes confirms once, resets the shared brush
overrides and flushes through normal retry semantics without a layout event.
Included workspaces permit rename and reject deletion. Existing quick-access
toolbar controls remain in their approved sibling submenu.

The canvas owner polls cancellable reads independently from accepted writes.
Epochs fence delayed dialog/row actions. Ownership renewal stays active through
previews; outgoing ownership is released after live adoption. Selecting another
process's owned workspace activates its HWND without claiming or switching the
source workspace. Same-process New Window is still pending.

Close review found and fixed two manager races: a confirmed create queued behind
autosave could be cancelled by close, and a completed rename could reopen its list
while close waited for that dialog. Accepted queued/submitted operations now drain,
and a failed manager write during close can keep the window open. Bridge tests
exercise both races with held SQLite acknowledgements. Two upstream catalog tests
now drop their SQLite owners before removing their temporary directories on Windows.

The combined tree passes 411 unit tests (273 UI, 24 host, 73 Windows, 41 workspace;
three explicit GPU tests ignored), strict Windows-crate Clippy, native queue tests
and Rust/C++ builds. Native manager tests cover the included identities, retained
rows, preview cancellation, explicit switch/history restore, baseline restoration,
name-only creation, rename/delete, brush reset, header switching and restart after
closing during a preview. A two-process fixture verifies independent ownership
and native window activation. The toolbar, editor, persistence/recovery and
OS-injected tab-drag regressions pass their scoped checks, including retained
controls, titlebar hit regions, overview movement and successful five-second exit
samples. Native manager, task-workspace and reset-dialog captures were inspected;
full matched-viewport Chrome parity remains unaccepted.

The earlier intermittent final shader-worker join is not fixed: manager and
persistence runs during this milestone included joins around 5.5–6.1 seconds,
exceeding the unchanged five-second close gate. Later passing runs establish only
their own samples. Full visual/gesture/scroll/overlap parity, same-process window
creation, runtime filter packages, physical input, mixed-DPI/device/suspend
lifecycle, distribution packaging and strict GPU filter-reference agreement
remain open. Final 120 Hz painting and physical input-to-present benchmarks remain
deferred until the rest of the app is ready. Profiles, databases, captures, logs,
traces and binaries remain ignored/local.

Publication integration through a398e46 adds the shared host controller/browser
store, Web/Android workspace hosts and Apple retained-resize work. The final merged tree passes 419 unit tests
(273 UI, 24 host, 73 Windows, 49 workspace; three explicit GPU tests ignored),
strict Windows-crate Clippy and the Rust/C++ build. These additive shared/browser
and Apple changes leave the exercised Windows UI paths unchanged.

## Native New Window milestone — 2026-09-12

File > New Window and the shared Ctrl+Shift+N shortcut now create native windows
within one process. App retains each CanvasWindow by WindowId until its input
dispatcher, renderer, storage and XAML views have completed teardown. Every
window keeps its own independently presented canvas beneath the custom titlebar,
document, dialogs and workspace ownership. Window creation acknowledges its source
request once, including failure. The first window can close without terminating
the others, and the last window still follows normal process/shader teardown.

The pinned WinUI runtime passes simultaneous ContentDialogs in separate XamlRoots.
A same-process ownership test found that ContentDialog focus restoration could
undo Switch to Window activation. Owner activation now waits until the source
dialog has fully unwound. Both same-process and two-process activation fixtures
pass, including retaining the source workspace.

Preferences now share a profile-scoped in-memory authority across render owners.
New windows inherit accepted values before disk completion. Field/key differences
against each owner's last adopted state preserve unrelated concurrent edits,
including shortcut override removal. The shared Settings schema still validates
the merged result. Writes remain on bounded workers and serialize replacement
across windows; stale jobs cannot overwrite the latest state. Host callbacks
disconnect under a fence that waits for any in-flight callback.

Per-process window manifests and per-window snapshots make tests identify the
exact HWND and model instead of relying on MainWindowHandle. The initial window
retains legacy snapshot paths for existing single-window fixtures. Reused native
window IDs invalidate their earlier diagnostic model before registration.
All diagnostics, profiles, documents, captures and binaries remain opt-in/ignored/local.

The combined tree passes 424 unit tests (273 UI, 24 host, 78 Windows, 49 workspace;
three explicit GPU tests ignored), strict Windows-crate Clippy, native queue tests
and Rust/C++ builds. New bridge tests cover concurrent unrelated settings changes,
inheritance before persistence, stale writes, profile isolation, removed shortcuts
and callback disconnection. The three-window native fixture passes menu/OS shortcut
creation, simultaneous dialogs, shared preferences, independent drawing while
another window is modal, cancel-close, closing the original first, and five-second
zero process exit. The two-process owner-activation fixture and all five isolated
preferences restart/failure/recovery launches also pass.

Full visual/gesture/scroll/overlap parity, runtime filter-package import, toolbar
library round-trip verification, physical pen/touch input, mixed-DPI/device/suspend
lifecycle and distribution packaging remain open. Earlier strict GPU filter
reference disagreement and intermittent final shader-worker shutdown delays
remain unresolved. These passing close samples do not erase the earlier failures.
Final sustained 120 Hz painting and input-to-present benchmarks remain deferred
until the rest of the app is ready. This milestone does not complete the goal.

## Runtime filter transport milestone — 2026-09-12

Windows stages the shared JSON/WGSL files beside the executable and acquires
packages on a bounded background worker. Startup and the render-owner loading
API use the shared parser, GPU validation and atomic publication. The host adds
transport progress/errors and an idle-boundary readiness check. Startup refreshes
the library without migrating embedded document programs. Explicit loading
preserves compatible live values and rejects a delayed read after document
replacement. Missing default resources use the embedded fallback; invalid
explicit resources remain visible errors. There is no new product import dialog
or shader editor; only the opt-in smoke controls expose a test reload button.

Seven new CPU tests cover render/preparation module reads, edited files without
rebuild, path and size limits, invalid/missing data, overlap, retry, pending GPU
attachment and document replacement. The explicit hardware D3D12 test passes:
changed WGSL changes full-image pixels, invalid replacement preserves pixels and
values, conflicting library declarations reject atomically, and compatible
library refresh preserves the current embedded document program. This does not
relax the shared namespace validator or resolve the older strict PNG gate.

The combined tree passes 431 unit tests (273 UI, 24 host, 85 Windows, 49 workspace;
four explicit GPU tests ignored in that default run), strict Windows-crate Clippy,
native queue tests and Rust/C++ builds. The new hardware test passes separately.
The native runtime fixture passes startup loading, picker insertion, Radius edits,
live metadata/WGSL replacement, invalid WGSL and missing module preservation,
retry, changed preview pixels and unchanged executable hash. Effects and
multiwindow regressions pass. All three owned processes close successfully with
empty stderr; the native replacement capture was visually inspected. These passing
close samples do not resolve the intermittent final shader-worker join failures.

Integration includes the Apple/web header alignment from dfa9580. Full matched
Chrome visual/gesture/scroll/overlap parity, toolbar library round trips, physical
pen/touch input, mixed-DPI/device/suspend lifecycle, distribution packaging, strict
GPU filter-reference agreement and shutdown timing remain open. Final sustained
120 Hz painting and physical input-to-present benchmarks remain deferred until
the rest of the app is ready. Profiles, packages used by tests, captures and
reports remain ignored/local. This milestone does not complete the goal.

## Saved toolbar library milestone — 2026-09-12

Windows now connects New/Manage Toolbars to the shared SQLite library, following
GTK's empty-or-saved New Toolbar form and This Workspace/Saved Toolbars views.
Current toolbars expose shared visibility, save, replace and existing local
rename/duplicate/delete actions. Saved entries can be added, renamed and deleted.
No saved-layout, metadata, recovery-bin or update-version screens were added.

Shared installation owns control order, independent panel/tile identities,
display options, target placement and one-step layout history. Library rename
or deletion preserves installed copies. The native service transports definitions
asynchronously, checks ownership at adoption, observes the accepted layout and
flushes before completing the operation. Persistence retry cannot install twice.
Close retains accepted work queued behind a source read. Page changes cancel
obsolete reads and reject delayed row/menu actions; forms wait for their choices
before submission.

Five new bridge tests cover save/restart/reuse, library rename/delete versus
independent instances, empty creation, placement-preserving replacement, layout
undo/redo, visibility persistence, stale page/selection callbacks, persistence
retry and close during an accepted read. The four crates pass 436 unit tests
(273 UI, 24 host, 90 Windows, 49 workspace; four explicit GPU tests ignored), strict Windows-crate Clippy and
Rust/C++ builds. The four-launch native library fixture passes save/restart,
copy/restart, library rename, insertion, cancel/delete, persistent removal and
survival of independent copies. The toolbar-editing native regression also passes,
including its final five-second exit. Workspace-manager actions and restart pass
their functional checks, but its last process exceeds the five-second close gate.
This Workspace was visually inspected; the updated shared inventory example also
compiles on Windows.

Earlier native runs during this milestone exceeded the unchanged five-second
close gate. Later library launches all passed it; these samples do not fix or
supersede the earlier failures. The fixture can continue after a slow but confirmed
successful exit to gather remaining functional evidence, and still fails overall
if any close exceeded five seconds. No input or presentation benchmark was run.

Integration includes Apple inventory and selection/disabled styling through
0b2e1cb. Full matched Chrome visual/gesture/scroll/overlap parity, physical pen/touch,
mixed-DPI/device/suspend lifecycle, distribution packaging, strict GPU reference
agreement and shutdown timing remain open. Final sustained 120 Hz painting and
physical input-to-present acceptance remain deferred until the rest of the app is
ready. Profiles, databases, captures and logs stay ignored/local. The goal remains
active.

## GPU startup and close milestone — 2026-09-12

Tracing isolated the intermittent final join to the combined destination-brush
pipeline, which took about 6.5 seconds to compile in the native app. The shared
renderer now specializes that WGSL by material operation. Attachment variants
share the shader module; persistent, private-preview and direct-preview passes
use the same operation as startup dependency selection. GPU optimization stays
enabled, the compiler still cancels queued jobs on teardown, and final process
exit still joins the in-flight driver call.

One hardware regression compares all six operations, alpha lock, erase, sparse
page boundaries, both prediction paths and subsequent commits against uniform
dispatch. All 120 full-image comparisons match exactly on Vulkan and D3D12.
The Windows regression explicitly requests the D3D12 backend. Four isolated
native probes after specialization closed in 0.35–1.25 seconds, with a maximum
material pipeline compile of 723 ms. Final delayed-close samples exit in
0.36–0.41 seconds. These are development-build close measurements, not painting
cadence or physical input latency.

Native lifecycle review also exposed early close requests rejected during
read-only catalog validation, and initialization waiting to adopt a workspace
after the document had authorized close. Shared snapshot/close policy now permits
library-only background validation while retaining the interaction and document
migration guards. New/Open still waits for validation. Unsaved decisions and
save completion keep their normal checkpoint semantics; native replies retain
epoch/revision fencing. Windows releases an unadopted startup workspace claim
without saving its provisional layout or waiting for GPU readiness.

Two shared tests cover library versus document migration, active interaction,
replacement rejection, cancelling unsaved decisions, immutable save capture and
close only after save acknowledgement. A Windows service test verifies immediate
claim reuse and unchanged stored layout after close before startup adoption.
The new startup/close fixture uses isolated profiles, exact process handles,
several close delays, optional compiler timing logs and the unchanged five-second
zero-exit gate. Earlier failures remain recorded; delayed but successful exit
does not pass the gate.

Integration includes shared opacity locks and Apple property coverage through
841cc25. The combined host/UI/workspace/Windows tree passes 440 unit tests
(24 host, 276 UI, 49 workspace, 91 Windows; four explicit GPU tests ignored),
Windows Clippy, native builds, the shared inventory example, and WebAssembly
UI/renderer checks. The broader renderer run has 118 passes, one existing strict
filter-reference failure (maximum channel error 30), and 18 intentionally ignored
benchmarks. Seven startup ordering/teardown regressions pass. Native lifecycle
and runtime-filter fixtures pass after integration, including early close,
minimized unsaved decisions, cancellation, discard, live WGSL replacement and
preservation after invalid imports.

A transient titlebar-measurement error observed during minimize/restore remains
to investigate. Full matched Chrome visual/gesture/scroll/overlap parity, physical
pen/touch, mixed-DPI/device/suspend recovery, distribution packaging, strict
GPU reference agreement and final 120 Hz painting/input acceptance remain open.
Profiles, documents, databases, captures, logs and binaries stay ignored/local.
The goal remains active.

## Matched Windows editor captures and caption restoration

The minimized-window trace reproduced a negative native RightInset even after
IsIconic cleared. The host now keeps the last valid caption projections together
and retries after the transition, while continuing to resize the GPU surface.
Activation remeasures restored geometry. The lifecycle fixture checks valid
caption measurements and native error status after transient startup messages.
The existing cancel/discard and five-second close requirements remain.

The new native capture fixture exercises actual theme and camera controls and
records the complete drawing surface, full client, model and UI Automation bounds.
A separate opt-in camera trace includes camera-only publications so a full model
snapshot cannot supply stale camera evidence. Captures verify the visible readout
and settled layout. The tested client contains a one-physical-pixel OS frame above
the XAML content; the complete XAML surface is selected from the same retained raw
frame, with its offset recorded explicitly.

A shared Chrome capture mode consumes the manifest. Hardware WebGPU initializes
without browser exceptions, and native/Chrome camera transforms match for both
themes and initial/zoomed-under-header states at 960 x 660 logical and 1.5 scale.
Chrome reserves native caption-button space without importing native panel
measurements or substituting native camera transforms. Full-image comparisons
retain all differences, including system caption glyphs.

Tool Set now follows the shared compact group and subtool layout: stacked group
icons/labels, four-DIP group gaps, equal flex rows, eight-DIP group/list separation,
and horizontal 82 x 32 preview boxes with preserved aspect ratio. Position, size
and edge differences are at most half a physical pixel in all four captures.
Native tools regression checks pass. Full-editor raster parity remains open:
responsive header behavior, shadows, numeric controls and layer-row details are
among the observed differences. This checkpoint does not establish physical input,
DPI/device recovery, package delivery, strict GPU agreement or final 120 Hz cadence.

Integration includes Apple's tool-action styling and focused Metal execution
analysis through 219cda5. Both new Chrome capture scenarios remain available.
Native builds, the tool regression fixture and the strengthened lifecycle fixture
pass; the merged Chrome runner passes the four hardware editor captures and Tool
Set geometry checks. Capture artifacts and isolated profiles stay ignored/local.

## Native numeric controls and editing — 2026-09-12

Windows panel numbers now follow the shared 24-DIP value/track rows, label
insets, six-DIP gaps, rounded spin fields, formatted value widths and disabled
step appearance. A retained TextBox owns native editing and accessibility; a
plain unfocused readout avoids the hidden caret gutter affecting the shared
layout. Unchanged text reuses its measured width. The implementation lives in
NumberControl.cpp so changing the common control no longer recompiles every
panel. Compact layer opacity and value-only color entries retain their shared
numeric policy and target guards.

Preferences use 34-DIP value fields, 32-DIP ranges, accent fill and native thumbs.
Descriptions remain with the title above the track. The Pen & Input page was
visually inspected in light and dark themes. Enter commits, Escape cancels, and
spin Up/Down route through preview key handling before TextBox consumes them.
A slider replaces an invalid text draft and restores formatted units. Numeric
math, bounds, stepping, parsing and formatting continue to use shared Rust.

A separate opt-in review entry point instantiates production Windows controls
from a Rust-generated synthetic sheet, without a document or user storage.
Thirty controls in each theme cover three widths, endpoints, intermediate values,
long labels and disabled ancestors. Native value/step/opacity assertions pass,
as do the browser's required geometry checks: no missing or extra controls and
maximum error 0.020203 logical pixels at 1.5 scale. Full unmasked zero-tolerance
comparisons retain raster differences in 4.34% of light and 4.31% of dark pixels.
These results establish control geometry, not identical native/browser pixels.

Native tools, settings/status, color, effects and layers fixtures pass. Coverage
includes actual Enter/Up/Down/Escape delivery to the owned native window,
expressions and invalid drafts, retained controls, model/context replacement,
preview painting and exact undo. UI Automation scroll setup now settles the first
field's queued bring-into-view before requesting the final offset; subsequent
visible-slider edits retain both the scroll object and position. Preferences
lookup retains the actual observed dialog/control instead of a second lookup.
Color startup waits for workspace adoption and checks that a departed slot's
draft cannot affect the newly selected slot.

Production and fixture native builds, release Wasm, strict Windows Clippy and
442 unit tests pass (24 host, 278 UI, 91 Windows, 49 workspace; four explicit GPU
tests excluded). The latest four production editor captures at 960 x 660 logical
and 1.5 scale retain header error at most 1.665 physical pixels, Tool Set error
at most 0.501, and matching camera viewport/zoom/translation/work area. Full-image
raster differences remain, including shadows, layer rows and color/control
rendering details. Main integration through ebc507a includes other-port work and
the shared drag rules; the merged Chrome runner passes both numeric themes and
all four editor captures.

The broader renderer's existing strict filter-reference failure remains open
(maximum channel error 30 versus tolerance 1); this milestone does not replace
that reference or relax the gate. Whole-editor visual/gesture/scroll/overlap
parity, the drag pickup convention, physical pressure/tilt/eraser/touch input,
mixed-DPI/suspend/device recovery, distribution packaging and final sustained
120 Hz painting plus physical input-to-present acceptance remain open. No
presentation benchmark ran. Profiles, documents, captures, logs and binaries
remain ignored/local. The goal remains active.

## Layers and composition shadows (2026-09-12)

The native Layers panel now follows the shared 24-DIP header control height,
conditional thumbnail/mask gaps, row label and metadata line heights, rename
spacing, thumbnail corner radius, grip geometry and disabled icon opacity.
Panel, expanded-configuration and drawer shadows use retained Composition
SpriteVisuals with separate XAML alpha masks. Their padded roots preserve blur
outside the panel while the existing Navigator occlusion path excludes GPU
image regions. Shadows follow panel transforms and disposal. The header adopts
the shared recessed, borderless switcher style.

The complete editor capture now measures 18 visible Layers elements in each
of four 960 x 660-DIP scenes at 1.5 scale: light/dark and fit/under-header camera
positions. Maximum Layers geometry error is 1.000031 physical pixels; header
error remains at most 1.664063 and Tool Set error at most 0.5. Camera checks pass.
A sampled 25-pixel standard-panel shadow edge matches Chrome exactly in dark
mode and differs by at most one color level in light mode after blur calibration.
This sample does not establish whole-image equality or mixed-DPI acceptance.
Complete zero-tolerance initial-scene comparisons still differ in 8.43% of light
pixels and 7.58% of dark pixels; these raster differences remain available for
review and are not an accepted whole-editor parity result.

Named row/container/image peers improve accessibility and expose preview
readiness. Captures wait for each visible raster thumbnail's asynchronous
readback before taking a frame. Opt-in diagnostics record RenderSize after
layout settles: UI Automation may include invisible focus decoration or omit
container padding, while TextBlock ActualWidth can report content width.
Raw automation bounds, ActualWidth/Height bounds, parked virtualized elements,
the full client and complete XAML image remain available locally. Browser
geometry uses its own production layout. No image differences are masked.

Native layer checks cover painting and exact undo, masks, row retention and
recycling, document replacement, and focused-draft commit on close. Drawer and
expansion checks pass, including Navigator GPU occlusion/restoration; their
captures were inspected. Following integration through main 8b37ee2, production
WinUI and release Wasm builds, strict Windows Clippy, all four Chrome captures,
the manager/restart fixture and 450 unit tests pass (24 host, 279 UI, 91 Windows,
56 workspace; four explicitly ignored hardware tests). Shared Rust cleanup
uses fixed-size record chunks and boxes the large workspace reply/outcome
payloads while retaining the serialized protocol. Final integration through
98294ff also includes the other ports' collapsed-column drop improvements and
Web focus-refresh fix; 452 unit tests, strict Windows Clippy and the Wasm check
pass after that integration.

Remaining work includes configurable-switcher UI and cross-window refresh,
the shared drag pickup rules (including no mouse hold menus), further editor
raster details, the existing strict GPU filter-reference failure, physical
input, mixed DPI/lifecycle/recovery, distribution and final 120 Hz painting and
input-latency acceptance. No presentation benchmark ran during this milestone.
Review artifacts stay ignored and local; the implementation goal remains active.

Publication also integrates concurrent main 76b4dd5, which adjusts shared divider
targets and toolbar group creation. All 284 shared UI tests, strict Windows
Clippy and the Wasm check pass after that merge.

## Configurable workspace switcher and native row gestures (2026-09-12)

The titlebar switcher now follows shared saved order and visibility, including
custom workspaces. New workspaces start pinned; an unpinned current workspace
appears first temporarily. The pill scrolls within its available header width.
Manage Workspaces exposes left grips, pins, a separate active-workspace check,
and Show in top bar / Move Up / Move Down menu actions. Reconciliation retains
row and header controls while preference updates preserve selection and preview.

Preference writes have their own asynchronous lifetime. Cancel restores the
workspace preview without undoing accepted preferences, and closing the app
waits for accepted writes. A pending focus refresh can be superseded by an
enabled menu action, but a submitted write cannot. Same-process windows refresh
without activation; independent processes refresh on focus. Shared Rust owns
validation, persistence and ordering. Windows also uses the shared history
formatter for layout captions and ordering.

Mouse row bodies and every device's grips drag after system movement slop.
Touch and pen bodies preserve native scrolling until a native hold wins. Held
menus retain the original contact for dragging, remain open after a held release,
and close on cancellation. Stable surface capture, insertion hints, edge
scrolling and source invalidation preserve the preview and saved order.
Releases after focus loss or minimization are rejected immediately.

The input fixture checks the actual source and device, arranged row bounds and
atomic per-window snapshots. Cancellation checks inspect native terminal
decisions before asynchronous persistence can hide a submitted move. Touch
injection and synthetic pen device removal produce PointerCanceled; mouse uses
Escape. The pen driver uses the same removal path for focus-loss cleanup.
These checks exercise OS routing, not physical digitizers or input latency.
The multiwindow fixture waits for a Preferences dialog before querying its
Close button after a theme update. Test launchers remove absent environment
flags explicitly so an empty value cannot accidentally enable probe controls.

Integration through main 907c632 includes shared startup reuse, column settings,
and the updated Illustrator preset. Startup validation compares the adopted
layout with that full shipped preset. The 481 unit tests pass (25 host, 296 UI,
96 Windows and 64 workspace; four explicitly ignored hardware tests), together
with strict Windows Clippy, the production Rust/WinUI build and release Wasm.
The Windows development guide now includes a portable debugging and visual-test
starter without machine-specific paths or private data.

Native manager/restart, switcher, owner-focus and multiwindow checks pass,
including preference updates in inactive windows. Full row fixtures pass for
mouse, touch and pen, with native cancellation and minimize evidence. The latest
integration rechecks the manager, switcher, multiwindow workflow and pen gestures.
All four native/Chrome editor scenes pass the unchanged geometry checks: maximum
header error 1.664063 physical pixels, Layers 1.000031 and Tool Set 0.5. Camera
and titlebar-underlay checks pass. Unmasked zero-tolerance raster comparisons
still differ in 7.54%/8.38% of dark/light initial pixels and 10.05%/9.42% of
under-header pixels; these are not accepted whole-editor raster parity results.

Publication also integrates concurrent main 49889a7. Included workspace names
are now protected by shared policy; Windows omits their Rename/Delete actions
and the unused separator. Custom workspace renaming still updates its titlebar
entry, and the active-workspace check precedes the pin. All 481 unit tests,
strict Windows Clippy, the native build, the manager/restart workflow and pen
row gestures pass after this integration. The manager handoff now reflects the
same protected-name policy.

Other workspace drag families, attached column group panels and per-column
preferences, the shared starting-layout preview, remaining editor raster
differences and the strict GPU filter-reference failure remain open. Physical
pressure/tilt/eraser/touch validation, mixed DPI/lifecycle/device recovery,
distribution and sustained 120 Hz painting plus physical input-to-present
acceptance are still required. No presentation benchmark ran during this
milestone. Profiles, captures, diagnostics and binaries remain ignored/local.

## Held workspace tiles and collapsed icons (2026-09-12)

Toolbar tiles, dividers, disabled commands and collapsed-column icons now use
native hold recognition for mouse, touch and pen. Before admission, native
scrolling remains available. Stable workspace capture preserves the original
contact through menus and icon tear-off. Pen/touch menus remain after held
release and close when dragging starts; mouse holds only arm pickup. Grips and
title/tab strips remain immediate. Shared Rust still owns drop validation,
layout publication and one-step history, including incremental workspace motion.

A native crash investigation found WinUI's deferred default context callback
reaching an icon removed during tear-off. Registered sources now disable the
competing XAML hold throughout their visual subtree. Owned submenu activation
no longer cancels the workspace menu. Cancellation of a menu without a contact
also preserves an immediately following accessibility invocation. Dividers are
focusable buttons; disabled command buttons retain their disabled state inside
customization hit targets. Zen assigns control IDs to the actual buttons and
preserves pen/touch context menus without enabling toolbar movement there.

The native pickup fixture passes for all three devices: source identity, short
clicks, early-motion rejection, held menus, same-contact drag, cancellation,
floating and drawer toolbars, divider/disabled tiles, collapsed-icon tear-off,
nested drawer origins, Zen behavior, keyboard menus, minimize and exact Undo/Redo.
The tab-motion regression also passes with retained controls, GPU Navigator
motion and shared incremental publication. Drawer and full-editor regressions
pass, including both Zen modes, theme changes, resizing and titlebar hit regions.
The updated header/settings regression passes prediction search/toggling,
shortcut-editor cancellation, immediate dialog reopen and fullscreen/status
policies. Its close helper targets the ContentDialog footer explicitly, avoiding
the shortcut row also named Close.
These are OS-delivered synthetic input and UI Automation results, not physical
stylus, painting cadence or latency acceptance.

Integration through main 8ea4548 passes 526 unit tests (41 engine, 25 host,
300 UI, 96 Windows and 64 workspace; four explicit hardware ignores), strict
Windows Clippy, the Rust/WinUI build and the release web build. All four matched
native/Chrome editor scenes pass the unchanged geometry gates: maximum header
error 1.664063 physical pixels, Layers 1.000031 and Tool Set 0.5. Camera and
paper-under-titlebar checks pass. Full-image zero-tolerance comparisons still
differ in 7.51%/8.38% of dark/light initial pixels and 10.03%/9.42% of the
under-header pixels. Complete visual parity and the GPU filter-reference failure
remain open; reference images and tolerances were not changed.

The portable development guide includes native crash triage, and the pickup
fixture can attach an optional CDB debugger. Diagnostic profiles, screenshots,
logs, dumps, symbol caches and binaries remain local and ignored. Layer-row
pickup, attached column group panels and per-column preferences, starting-layout
preview, remaining raster differences, physical pressure/tilt/eraser/touch,
DPI/device recovery, distribution and sustained 120 Hz painting/input latency
still require work. No presentation benchmark ran during this milestone.

## Whole layer-row pickup checkpoint

Windows now recognizes layer pickup across row whitespace and child controls,
using actual native mouse, pen and touch identities. Mouse bodies and explicit
grips move after system slop; pen/touch bodies wait for a native hold and leave
early motion to scrolling. The same held contact can open a menu and then drag;
released holds retain the menu. Child clicks are suppressed after pickup, while
native rename fields keep selection and text-menu ownership.

The retained layer surface owns capture through edge scrolling and virtualization.
ScrollPresenter required two distinct fixes: grips must claim before its Down
handler redirects input, and held contacts require temporarily disabling its
scroll axes as well as ignoring new input. All paths restore scrolling. The
native scrollbar now reserves its template column instead of covering row grips.
Actual cancellation, capture loss, source removal and loss of focus retire the
contact and its menu.

A shared read-only drop query uses the same reparent plan as the final edit,
preserving locks, clipping, cycles and layer/mask offsets. No-op placements do
not create history entries. The query is document-epoch fenced; the native host
also rejects stale source, target and revision responses.

Shared validation passes 424 tests across UI, host and Windows, with four
explicit hardware ignores; strict Windows Clippy and the Rust/WinUI build pass.
The existing full Layers editing regression also passes. The pickup fixture
covers each device's body/whitespace/child/grip arbitration, held menus,
group drops, native scrolling, edge capture, source removal, rename ownership,
minimization and one-step Undo/Redo, including floating and drawer rows.
Floating setup uses mouse input: a separate pen tab tear-off exposed capture loss
and remains open. These results do not establish physical pen/touch acceptance.

A diagnostic race left an old per-window JSON file after a failed atomic rename,
even though native Redo and its toolbar state had advanced. Opt-in trace writes
now retry transient replacement failures briefly and report persistent errors to
the debugger. The fixture records native publication evidence alongside failures;
trace timing is excluded from presentation measurements.

Visual parity, attached column panels/preferences, starting-layout preview,
physical input and recovery, distribution, sustained 120 Hz painting and physical
input latency remain open. The unrelated GPU filter-reference investigation is
excluded from this milestone. No performance benchmark ran here.

## Audited shared icons on Windows

The shared SVG bank and Rust catalog mappings from the published icon audit
are integrated with Windows. All 157 SVGs are staged in light and dark variants,
preserving explicit colors and opacity while resolving `currentColor`. Native
tool groups, subtools, commands and filter layers consume the shared identities.
Filter category headings, the selected category and preview captions now show
their specific icons. Native checkable tool actions retain checkbox semantics
beside command glyphs. New Layer uses the document-plus asset; column expansion
uses the shared inward-facing double chevrons.

The integrated shared suite passes 428 tests with four explicit hardware ignores,
along with strict Windows Clippy and the Rust/WinUI build. Native tool editing,
filter category/search/insertion, preview/Undo, both-theme filter replacement,
drawer/column history and Navigator drawer checks pass. The staged SVGs match
their source in both themes. Full raster parity and physical-device/performance
acceptance remain open.

## Attached column groups and starting-layout preview

Windows collapsed columns now expose Drawers, Group panel, Auto-hide and Apply
to all columns through the shared column menu. Attached groups occupy shared
vertical slots beside the icon strip, with native width/split resize cursors and
immediate captured pickup. Shared Rust owns allocation, cancellation, preferences
and one-step history. Resizing retains native bodies and scrolling, including
Layers, toolbar tiles and GPU Navigator content. Widths and split weights survive
restart; open groups remain transient. Chrome hiding in Zen removes the attached
presentation and restores it when chrome returns.

Opening Preferences or another customization dialog previously cleared attached
bodies while leaving their layout allocation. Shared reconciliation now preserves
that presentation on both GTK and Windows. The same column contract tests run
for both platforms, including preferences, resizing, dismissal, cancellation,
all-column settings and history restoration.

Restore Starting Layout now previews its saved baseline in the editor behind the
native confirmation. Cancel and window close restore the current arrangement;
confirmation preserves tool settings and creates one workspace history step.
The native service test checks that preview never reaches storage, failed writes
retain the current arrangement, Retry applies once, and Undo/Redo remain exact.

The column fixture measures actual native slot bounds against shared geometry
using the Win32 client origin and current DPI. It covers both sides, every pointer
device, retained resize/cancellation/history, presentation switching, auto-hide
without painting, all-column preferences, Navigator, Layers, Zen, both themes and
restart. Toolbar and row pickup fixtures select Drawers or Group panel explicitly.
Their row-body target avoids the attached resize strip, and cancellation accounts
for immediate attached-panel removal rather than a drawer closing animation.

The affected unit checks pass 502 tests (25 host, 315 UI, 98 Windows and
64 workspace), with four explicit hardware ignores. Strict Windows Clippy and
both Debug and Release Rust/WinUI builds pass. Column fixtures pass for mouse,
pen and touch; toolbar and layer-row pickup pass all six device/presentation
combinations. The existing drawer regression and Release manager/editor fixtures
also pass, including preview/Cancel, one-step reset history, restart, both Zen
modes, themes, retained controls, resizing and titlebar hit regions. Native slot
bounds agree with shared allocations within one physical pixel. These checks do
not establish full-image parity, physical digitizer features or painting cadence.

Capture diagnostics and the focused tab fixture remain available. The user
confirmed physical Layers tab tear-off follows pen contact through release;
injected pen/touch tab capture loss is still unresolved. No diagnostic capture,
private profile, machine report, generated asset or binary belongs in this commit.

Full-image workspace parity, strict renderer reference agreement, the complete
physical input matrix, mixed-DPI/device recovery, distribution and sustained
120 Hz painting plus physical input latency remain open. No performance
benchmark ran during this milestone.

## Portable Windows distribution

The Windows packager now builds Release Rust/WinUI into a fresh staging directory
and produces an unsigned Windows 11 x64 ZIP with self-contained WinUI and app-local
Visual C++ runtimes. It includes project/branding terms, notices for the resolved
Cargo and pinned NuGet dependencies, and Rust/native runtime notices. Missing
unreviewed notices fail packaging. Supplemental texts retain their exact upstream
source commits.

The package records its source commit, development status, toolchain versions and
every file's size/hash. Uncommitted changes require an explicit development option.
A clean source change during packaging aborts the build. Sorted paths, fixed ZIP
timestamps and normalized entry attributes make archive assembly repeatable for
identical payloads; two assemblies must have the same SHA-256. This is narrower
than bit-identical recompilation across toolchain installations.

The extracted-package fixture validates the complete inventory before launch.
It uses a path with spaces, an unrelated working directory and a disposable
profile, then checks complete filter loading, runtime origins, drawing, Undo/Redo,
pan, resize and zero-exit shutdown. Rust, XAML, Windows App Runtime and the C++
runtime load from the package. The observed shader compiler loads from the Windows
system directory. Captures, loaded-module paths and profiles remain local.

The Release build, notice collection, repeated archive comparison and extracted
native fixture pass. The dirty-source gate and rejection of changed or undeclared
archive files also pass; invalid packages never launch. This checkpoint does not
establish clean-machine deployment,
MSIX/signing, full-image parity, physical pen/touch features, mixed-DPI/device
recovery or sustained 120 Hz painting/input latency. No performance benchmark ran.

### Compact Windows Color picker milestone

Windows now projects the compact shared ColorPanelView introduced on main:
Okhsv circle, HSV square, HLS triangle, paint-pair/transparent swatches,
shape switches, swap and curved shape/RGB readouts. The native view fits a
solo docked panel to its available height. Shared Rust supplies logical layout,
projection-aware hit tests, display hue guides and field pixels. WinUI retains
the buttons and capture owner; unhandled wheel contacts are classified against
shared geometry before capture, independently of the image's hit-test surface.
Native swatch menus support secondary click, pen/touch holds and the keyboard.

The four shared SVG color icons are staged as native path geometry so rotation
does not resample a bitmap. The hue mesh uses a continuous annulus clip.
Painted wheel-image edges follow the browser canvas's logical-pixel snapping,
while shared edit geometry remains unchanged. Chromium's
[canvas painter](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/paint/html_canvas_painter.cc)
and the actual composed reference establish this distinction from DOM bounds.

Validation: 523 shared/native unit tests and strict Windows Clippy pass.
Documented Debug and Release builds pass. Release checks pass the full editor,
titlebar hit regions, native measurements, both Zen modes, retained resize,
workspace restoration, themes and zero exit. The dedicated compact picker
fixture passes all three projections with synthetic mouse, pen and touch,
field/hue contacts beneath the curved readout, cancellation, keyboard activation,
unchanged paint on shape/readout toggles, retained buttons, slots/swap, native
context menus, the mouse-hold negative case and retained Color drawer input.
Picker edits leave the document unchanged and every accepted fixture exits zero.

Matched production native/browser fixtures cover 128, 160, 226 and 360 logical
pixels, dark/light themes and both readouts (48 pickers on eight complete
surfaces) at the available 150% display scale. Other native scales are not
accepted. Reported bounds agree except the intentionally snapped wheel image
at size160, whose paint width differs from the fractional DOM box by at most
0.672 logical pixels.

**Exact raster parity is not accepted.** The unchanged whole-image,
zero-tolerance comparison fails all eight surfaces: 22.84–26.23% of pixels
differ, with mean absolute channel error 0.465–1.399 on the 0–255 scale and
maximum channel error 137–146. Curved text and edge rasterization remain
visible in the difference images. References were not masked, rescaled or
replaced and tolerances were not changed. Capture commands and evidence
boundaries are in the Windows README; machine captures and reports stay
ignored. Physical digitizer and final painting/performance acceptance remain
separate, and the 120 Hz display is currently unavailable.

### Compact picker text and state refinement

Paired instrumentation measured the actual visible native/browser glyph
baselines within 0.000026 logical pixels, with matching Segoe UI advances.
The large curved-digit differences came from rasterization rather than layout.
Native digits now use DirectWrite font outlines as retained WinUI geometry,
transformed before rasterization. The readout remains a named native button.
The temporary text instrumentation was removed after preserving local evidence.

Keyboard focus follows main's styles. The readout uses a centered rounded
stroke; shape, swatch and swap buttons do not add a system outline absent from
the reference. Four focus and three hover states were captured in dark and
light at size 160. Hover and focus affect the corresponding reference regions.
Native captures use OS mouse input because cursor repositioning alone did not
reliably cause pointer-over styling.

Documented Debug/Release builds and the final production input fixture pass,
including all projections/devices, keyboard activation, menus, cancellation,
retained drawer input, unchanged documents and zero exit. A drawer test race
was measured on the same failed instance: its visible wheel continued moving
during the opening animation. The fixture now waits for stable complete bounds
and native capture release before switching devices. No Rust behavior changed
since the preceding 523-test and strict-Clippy checkpoint.

All eight complete default captures improve: mean absolute channel error is
0.433–1.326 on the 0–255 scale; maximum channel error is 82–101, down from
137–146. Exact differing pixels remain 22.71–26.21%. The fourteen size-160
focus/hover comparisons have mean error 0.838–0.986 and maximum 88–89.
**Every zero-tolerance comparison still fails.** Text coverage, gradient
quantization and edge rasterization remain open. The comparator, reference
rendering and tolerances are unchanged. These results establish neither
additional native display scales nor physical pen or 120 Hz painting acceptance.

### Compact picker native text rasterization

The picker now rasterizes Segoe UI through DirectWrite at each final glyph
transform. It matches the reference's three-channel grayscale reduction, sRGB
text correction, font-cache precision and font-table hinting. Horizontal
labels use whole-pixel baselines; rotated digits use quarter-pixel positions.
The backing size is integral, including the fractional extent at size 226.
Glyph images and correction tables are retained; cache keys include effective
raster scale. The native readout button still owns input and its full accessible
description. The unrotated swap control reuses the native SVG loader, preserving
the shared rounded stroke geometry without the extra XAML subpath workaround.
The three rotated shape controls retain native vector geometry.

Independent raw-canvas probes narrowed curved digits and the bold label to a
maximum alpha difference of one at the investigated sizes. Those probes explain
the implementation; they are not the acceptance images. The complete production
matrix at actual display scale 1.5 has these zero-tolerance results:

| Theme / size | Exact differing pixels | Mean absolute RGB error (0–255) | Maximum |
| --- | ---: | ---: | ---: |
| Dark 128 | 21.790% | 0.9771 | 72 |
| Dark 160 | 25.345% | 0.7487 | 66 |
| Dark 226 | 25.622% | 0.5860 | 68 |
| Dark 360 | 26.019% | 0.4041 | 85 |
| Light 128 | 22.021% | 0.9370 | 90 |
| Light 160 | 25.533% | 0.7396 | 64 |
| Light 226 | 25.655% | 0.5789 | 72 |
| Light 360 | 26.078% | 0.4113 | 98 |

All eight default surfaces improve in average error over the preceding
milestone. Four focus and three hover states in both themes at size 160 have
mean error 0.7353–0.7578 and maximum error 64–78. Fractional label measurement
also preserves the existing focus rectangle. **All twenty-two complete-image
comparisons still fail exact equality.** No reference styles, pixels, image
boundaries, comparator or tolerance were changed.

Geometry reports retain the distinction between the snapped native paint image
and the logical browser canvas: at size 160 the reported wheel widths differ
by 0.671875 DIP. Other default sizes have matching reported bounds. Exposing the
input Canvas directly was investigated and reverted after its zero ActualWidth
prevented fixture readiness; both owned diagnostic instances were inspected
before closing normally. This does not establish exact input-bound geometry.
All temporary browser and readiness instrumentation was removed.

The documented Debug/Release builds and production picker input fixture pass:
all three shapes with synthetic mouse/pen/touch in field and ring, cancellation,
keyboard activation, paint slots and swap, native menus, mouse-hold negative
case, retained drawer input, unchanged document and zero exit. The complete
Release editor check also passes titlebar hit regions, tools, Zen modes, retained
resize, workspace restore, theme changes and zero exit. Rust is unchanged
from the preceding 523-test and strict-Clippy checkpoint. Custom ClearType tuning,
other physical display scales, physical digitizer behavior and 120 Hz painting
remain separate acceptance gates.

### Compact picker label spacing and hue stroke

The native labels now retain the font's pair kerning and quarter-pixel glyph
origins. Font metrics use Chromium's hundredth-pixel effective font size while
layout keeps the original logical size. Independent browser probes reproduce
all four complete labels from individually positioned glyphs at three sizes;
native DirectWrite metrics confirm the LC and RG pair adjustments.

The transparency checker now samples the shared repeating conic gradient's
quadrant boundaries, including the tile center. This removes the reversed gray
cells at the actual 1.5 display scale. The hue mesh is retained in an image brush
and strokes a single ellipse, matching the reference drawing operation and
reducing the previous annulus clip's rim differences. The wheel uses its whole
allocated bitmap extent for the normalized drawing transform. No new runtime
dependency or per-drag cache allocation was introduced.

The complete default surfaces at scale 1.5 now compare as follows:

| Theme / size | Exact differing pixels | Mean absolute RGB error (0–255) | Maximum |
| --- | ---: | ---: | ---: |
| dark 128 | 21.461% | 0.8252 | 64 |
| dark 160 | 25.064% | 0.6483 | 63 |
| dark 226 | 25.285% | 0.4927 | 68 |
| dark 360 | 25.911% | 0.3386 | 68 |
| light 128 | 21.715% | 0.7839 | 71 |
| light 160 | 25.266% | 0.6383 | 58 |
| light 226 | 25.338% | 0.4835 | 68 |
| light 360 | 25.976% | 0.3460 | 82 |

Average error improves at every default size and theme. The fourteen keyboard
focus and hover surfaces at size 160 have mean error 0.6340–0.6574 and maximum
error 58–78. **All twenty-two complete-image comparisons still fail exact pixel
equality.** The eight production browser references were recaptured and remain
byte-identical. Reported bounds retain the preceding size-160 wheel discrepancy.
No reference styles, comparator tolerances or capture boundaries were changed.

Normal Debug and Release builds, the production Color fixture, the full Release
picker input exercise and the full Release editor exercise pass. Picker checks
cover all shapes with synthetic mouse/pen/touch in the field and ring,
cancellation, keyboard activation, paint slots and swap, native menus, the
mouse-hold negative case, retained drawer input and an unchanged document.
Editor checks cover titlebar hits, tools, Zen modes, retained resize, workspace
restoration, themes and zero exit. Rust is unchanged from the 523-test and
strict-Clippy checkpoint. Physical pen, other display scales and 120 Hz painting
remain open acceptance work.

### Compact picker retained circle field

The circular color field now fills a native ellipse using a retained bitmap
brush. The brush follows the field cache's hue, size, shape and device lifetime.
This removes the temporary ellipse clipping layer from each redraw and improves
field-edge coverage. The shared field pixels and input geometry are unchanged.

At actual display scale 1.5, all default surfaces improve in average error:

| Theme / size | Exact differing pixels | Mean absolute RGB error (0–255) | Maximum |
| --- | ---: | ---: | ---: |
| dark 128 | 21.470% | 0.8240 | 64 |
| dark 160 | 25.020% | 0.6433 | 63 |
| dark 226 | 25.273% | 0.4873 | 68 |
| dark 360 | 25.900% | 0.3369 | 68 |
| light 128 | 21.715% | 0.7818 | 71 |
| light 160 | 25.238% | 0.6269 | 58 |
| light 226 | 25.321% | 0.4748 | 68 |
| light 360 | 25.965% | 0.3412 | 78 |

Four focus and three hover states in both themes have mean error 0.6226–0.6525
and maximum error 58–78. **All twenty-two complete-image comparisons still fail
exact equality.** References remain byte-identical to the preceding recapture;
capture boundaries and comparator tolerances are unchanged.

Independent plain white circle probes also reproduce the browser edge error,
isolating it from color conversion. The installed browser's
[pinned Skia revision](https://github.com/google/skia/tree/4f574af2444846ceca4d277a8095c5d4229d175f)
uses curve subdivision and scan-position rounding. Contour reconstruction remains
diagnostic research; no additional rasterizer or dependency was introduced.

Normal Debug/Release and Color fixture builds pass. Full Release picker checks
pass all three shapes with synthetic mouse/pen/touch, cancellation, keyboard,
slots/swap, native menus, retained drawer input and unchanged document. Full
Release editor checks also pass, including titlebar hits, tools, Zen, resize,
workspace restoration, themes and zero exit. Rust remains unchanged from the
523-test and strict-Clippy checkpoint. Physical input, other physical scales,
strict visual parity and 120 Hz painting remain open.

### Reproducible MSIX archive assembly

The MSIX packager consumes a verified portable payload, preserves app-local
runtimes and license notices, and derives its package logos from the shared
symbolic mark using the existing GTK icon proportions and colors. App source
and packaging source are recorded separately, with script and asset hashes.
Dirty inputs produce an explicit development artifact.

Both the normal unsigned package and the separate Windows 11 unsigned test
identity pass MakeAppx semantic validation and repeated assembly with identical
SHA-256 hashes. The normalizer handles the SDK's ZIP64 headers, changing only
timestamps before signing. It preserves compressed blocks and block maps and
refuses signed packages before mutation.

The 1,085-entry archives pass inventory hashing, MakeAppx extraction, activation
metadata and repeat-logo checks. ZIP32 fixtures verify payload preservation,
normalization idempotence and signed-archive refusal without mutation. Negative
inputs cover altered hashes, duplicate/traversal paths, undeclared files and
invalid versions/identity lengths. The archive suite passes under both Windows
PowerShell 5.1 and PowerShell 7.

An ordinary-user installation attempt failed with Windows error 0x80073D2B:
unsigned executable activations require administrator installation. No test
package remained registered. Installed identity, launch, update, uninstall,
clean-machine behavior and publisher signing remain unverified. The existing
portable ZIP and its runtime acceptance are unchanged. Strict picker pixels,
physical pen, GPU recovery and 120 Hz painting also remain open.

### Color-wheel visual acceptance

The wheel acceptance requirement now permits imperceptible differences and
prioritizes implementation simplicity. Normal-size review of the existing native
and production-browser captures at 128, 160, 226 and 360 logical pixels, in both
themes at display scale 1.5, finds no material difference in the three wheel
projections, hue guides or marker positions. The current Direct2D wheel is
accepted for those conditions. The exact-difference measurements above remain
valid diagnostics; exact pixel equality is no longer required for the wheel.

The app retains its existing native renderer and dependency set. Experimental
Skia rendering and canvas-export probes remain local research and are not part
of the app. This review changes no production code, capture bounds, reference
styles or comparator tolerances. The preceding interaction and build validation
still applies to the unchanged implementation. Text/edge antialiasing differences
remain visible under close comparison; additional physical display scales,
digitizer behavior, device recovery and 120 Hz painting still need acceptance.

### Windows GPU reconstruction and failed-resource cleanup

The Windows host detects both D3D12 removal and the deferred wgpu device-loss
callback. It parks the render owner, detaches the old swap chain on the XAML
thread, retires the renderer, and prepares a replacement with bounded retries.
The existing session retains document history, queued input, active strokes,
source image assets, camera, brush settings and workspace. GPU-only readbacks
are canceled, pending filter validation restarts from owned source bytes, and
native preview caches reset when the renderer generation changes. Document
candidates prepared against the old device cannot replace the live document.

The uncaptured-error callback records the first error and returns normally.
With the pinned wgpu version, a panic during failed pipeline creation can leave
its resource handle allocated, retaining the removed D3D12 device and preventing
a second reconstruction. Recording the error lets the returned handle drop;
normal host boundaries still report validation errors. Callback state never
owns the GPU. Arbitrary ABI panics continue to poison the host.

Validation at this checkpoint:

- 569 ordinary Rust tests pass across engine, host, UI, Windows and workspace;
  five hardware tests are explicitly excluded from that ordinary run.
- The new opt-in D3D12 regression passes: a deliberately invalid pipeline reports
  its error without a panic, releases its registry handle, and permits a new
  hardware device after actual removal even while callback state is retained.
- Strict all-target Clippy for engine, UI and Windows passes. Three existing
  test-style findings were corrected without changing expected float bits.
- Normal Debug and Release builds pass. Both native document fixtures pass two
  actual device removals, byte-identical PNG exports, preserved document and
  workspace state, Undo/Redo, subsequent Save/Save As and clean shutdown. Release
  also verifies imported-layer thumbnails become ready after each replacement.
- Debug lifecycle checks pass removal overlapping startup/brush preparation,
  close, minimize, Cancel and Discard. The strict C++ work-buffer suite passes.

The removal hook is available only in an isolated smoke-test host and removes
this process's D3D12 device; it does not reset the physical display adapter.
This milestone does not cover permanent recovery failure: exhausted retries
still end render services, so retaining Save/Save As and the dirty-close decision
in that state remains required work. Pending export/open/import overlaps,
multiple-window removal, physical driver reset, sleep/resume, digitizer behavior
and 120 Hz painting remain separate acceptance gates. No package or presentation
benchmark was regenerated for this checkpoint.

### Integrated GPU recovery and two native windows

The reconstruction milestone is integrated with upstream main through ce41feb,
including Apple compact color controls and the shared GTK/Web/Android floating
preview and content-size work. The merged engine/host/UI/Windows/workspace suite
passes 574 ordinary tests. Strict all-target Clippy passes after a small iterator
cleanup in the incoming hue-guide and HSV raster loops; their calculations and
accepted output are unchanged. The full 343-test UI suite passes after cleanup.
Normal merged Debug and Release builds pass.

The merged Release document fixture passes repeated real removal, unchanged PNG
exports, thumbnail readiness, state/history preservation and subsequent saving.
The multiwindow fixture also passes removal initiated once from each of two open
windows. Both the initiating window and idle sibling reconstruct, retain their
separate document/camera/brush/workspace state, and support Undo/Redo. Subsequent
independent closes and creating another native window pass with zero process
exit and no stderr output.

The complete merged Release editor and compact-color interaction fixtures pass,
including titlebar hits, retained resize, themes, Zen modes, all three picker
shapes, synthetic mouse/pen/touch, cancellation, keyboard, menus and drawer input.
These checks preserve the accepted native wheel implementation. Permanent GPU
failure and CPU saving, concurrent document-operation recovery, physical input,
suspend/driver-reset behavior, installed distribution and 120 Hz acceptance
remain open. Local profiles, screenshots and binaries are excluded from commits.

### Saving after GPU reconstruction cannot finish

Exhausted reconstruction now preserves the native document and settings services
until the user saves or approves closing. The render owner stops input admission,
retires already admitted samples without GPU submission, keeps completed stroke
history and cancels an unfinished stroke. It then releases rendering resources,
cancels pending GPU readbacks and filter candidates, and continues CPU document
operations. An accepted save keeps its worker; an uncaptured PNG export is
canceled. Source assets and in-memory document history remain owned by the session.

The unavailable-painting screen uses the current theme and exposes File, Save,
Save As and Preferences. It restores the header even when failure occurs in Zen
mode, without rewriting the saved Zen preference. Shared command/action policy
rejects operations that need rendering. The ordinary and retirement input paths
reuse the same native contact, dialog and chrome arbitration.

Validation:

- 578 ordinary Rust tests pass across engine, host, UI, Windows and workspace;
  five opt-in hardware tests are excluded from that suite. New CPU-only tests
  exercise more than one engine batch and an overflowing native input queue,
  completed versus unfinished strokes, durable archive contents, an accepted
  save, a waiting PNG export, and transform/filter-candidate cancellation.
- Strict all-target engine/host/UI/Windows Clippy and C++ input tests pass.
  Debug and Release builds pass.
- Debug and Release native document fixtures pass exhausted recovery, Save and
  Save As, canceled pickers, dirty-close Cancel/Discard, preferences, and clean
  shutdown. Release also covers failure in Zen. Saved projects reopen and export
  PNG bytes identical to the pre-failure drawing. The failure screen was inspected.
- Ordinary repeated-removal and two-window recovery fixtures still pass in
  Release. The complete Release editor regression passes after the final header
  and failure-screen changes.

This fixture removes the process-owned D3D12 device and prevents reconstruction
only in an isolated test host. Arbitrary ABI panics keep their existing fatal
handling. Concurrent document-operation recovery, physical input, mixed DPI,
suspend/driver-reset behavior, installed MSIX/clean-machine delivery and 120 Hz
painting/input latency remain open. No package or performance measurement was
regenerated for this milestone.

The milestone is integrated with upstream main through 65a9855, including steady
command-icon presentation during canvas strokes. The merged 578-test suite,
strict all-target Clippy and Release build pass. The merged native exhausted-
recovery fixture passes Save/Save As, Zen, Cancel/Discard, durable reopen and
identical PNG output. The compact picker regression also passes all three shapes,
synthetic mouse/pen/touch, cancellation, keyboard, menus, slots/swap and retained
drawer input. This integration preserves the accepted native wheel renderer.

### Document operations overlapping GPU removal

A decoded image could finish while reconstruction temporarily removed the
renderer. The document service then discarded that import, and successful GPU
recovery cleared its transient error. The completion now waits in the existing
bounded mailbox until a usable renderer returns. Canceled or stale imports and
decoder errors still drain immediately, including after permanent GPU failure.

Actual removal during New/Open also exposed mapped-buffer panics inside document
preparation. The zero-filled unrestricted-coverage buffer now uses wgpu's normal
zero initialization, without mapping. Uploads retain the same staging belt and
copy commands but use its allocation API to return mapping errors. Rendering and
viewport presentation propagate those errors through the existing host paths.
The Windows, Web, GTK, Apple and Android presenter callers consume the result;
no alternative renderer, queue or dependency was introduced.

Four opt-in hardware D3D12 tests pass without worker panics:

- A completed import survives removal before adoption, then temporary renderer
  absence, even after its original source file is deleted. Restored pixels and
  one-step Undo/Redo match the pre-removal reference.
- New/Open candidates are checked both after worker completion and immediately
  after submission. An obsolete candidate cannot replace the live document;
  retry succeeds with embedded image data. A save accepted before removal still
  completes durably and preserves source assets.
- A captured PNG ticket either produces the exact captured image or reports an
  error while preserving the existing destination. Export succeeds after GPU
  replacement and does not acknowledge a document save.
- A viewport upload on a removed device returns a mapping error without
  unwinding. Retiring those resources permits reconstruction and identical pixels.

The 579-test ordinary engine/host/UI/Windows/workspace suite passes, with nine
opt-in hardware tests excluded. Strict all-target engine/host/UI/Windows Clippy,
renderer-library Clippy and the Web Wasm compile check pass. Nine further rendering
tests pass with hardware D3D12 selected, covering Navigator presentation,
selection outlines and pixel transforms. Their two opt-in latency benchmarks
were not run.

These are process-owned device-removal and functional rendering checks. Physical
digitizer input, mixed-display and sleep/driver-reset behavior, all native
picker/input/filter overlaps, installed MSIX/clean-machine delivery, broad visual
acceptance and 120 Hz painting/input latency remain separate. The other native
presenter callers were mechanically updated and reviewed; their platform builds
are not established by this Windows validation. No package was regenerated.

Normal Debug and Release builds pass. The Debug native document fixture passes
repeated reconstruction; the Release exhausted-recovery fixture passes Save,
Save As, Cancel/Discard, restored Zen preferences, durable reopen and identical
PNG output. The complete Release editor fixture also passes. A fixture race on
reopen was corrected: it now waits for asynchronous workspace restoration before
checking Zen, and for a visible header before invoking File. The same failed
window successfully exited Zen and opened File during diagnosis; no production
UI change was needed for that race.

Integration with upstream main through 2142149 retains the native workspace
lease-reclamation and included-layout history fixes. The combined suite passes
581 ordinary tests, strict Clippy, the Web Wasm compile check and a normal
Release build. The merged Release exhausted-recovery document fixture, workspace
manager/starting-layout/history/restart fixture and two-window actual-removal
fixture all pass with clean shutdown.

### Filter loading across GPU reconstruction

Native filter loading now checks actual D3D12 removal before submitting an
acquired package. It keeps the existing bounded source mailbox until the
replacement renderer is available. If GPU recovery is exhausted, pending reads
report a failure immediately, completed file results are discarded and new loads
are rejected. The read can finish without delaying access to saving. Shared
validation cancellation preserves the current catalog, embedded programs and
parameter values. No production queue, pause protocol or dependency was added.

Three new opt-in hardware D3D12 checks pass: removal after file transport starts,
removal after validation submission but before publication, and cancellation after
exhausted recovery. Deleting the original package files proves that reconstruction
uses retained bytes. Successful replacement matches uninterrupted pixels and
one-step Undo/Redo while preserving a live parameter value; failed recovery leaves
the source document unchanged. Device selection/removal helpers are shared with
the document recovery tests. An ordinary regression covers pending reads and
acquired packages during suspension, rejected new requests and late completion.

The Windows ordinary suite passes 103 tests with 11 opt-in hardware tests excluded;
strict all-target Windows Clippy and normal Debug/Release native builds pass.

The four document recovery hardware tests pass with the shared helpers. The
Release native runtime-filter fixture passes picker/previews, live WGSL and
metadata replacement, retained parameter values, invalid WGSL and missing-module
rejection, retry and clean shutdown. The Release exhausted-recovery document
fixture also passes Save/Save As, canceled pickers, close decisions, restored Zen
preferences, durable reopen and identical exported pixels.

Actual native queued pointer overlap, physical pen and mixed-display acceptance,
sleep/driver-reset behavior, installed MSIX/clean-machine checks, remaining visual
review and 120 Hz painting/input latency remain open. No package or performance
measurement was regenerated for this milestone.

### Native pen queue during GPU reconstruction

The native render loop could drain a complete stroke after GPU replacement but
before the replacement brushes became ready. Shared startup policy then deferred
the press and discarded the whole contact. An isolated native UI reproduction
confirmed a ready, responsive canvas with a missing queued stroke.

The loop now leaves work in its existing bounded queue until the existing GPU
recovery counter resets at brush readiness. There is no additional queue, renderer
state or timing protocol. Initial startup keeps its existing input policy.

The document fixture now replays typed pen samples with varying pressure, tilt
and twist. Per-process trace timestamps prove the samples were admitted during
reconstruction; missing that interval fails the fixture. It checks a whole queued
stroke and an already-visible active stroke with its completion queued during
removal. Full exported PNGs, thumbnails and one-step Undo/Redo match the reference.
The active-stroke fixture checks canvas pixels away from the cursor and chrome;
command enabled styling deliberately stays stable during painting.

The original missing-stroke reproduction fails before the queue fix and passes
with exact exported pixels afterwards. Normal Debug/Release builds and C++ queue,
publication and preview-mailbox checks pass. Debug and Release native document
recovery runs pass both queued and active pen cases, with the active stroke
visually inspected before removal.

Release exhausted recovery also passes with a complete pen stroke followed by an
unfinished tail admitted while reconstruction fails. CPU retirement preserves the
complete stroke, cancels the tail, keeps File accessible in Zen, and permits
Save/Save As. Reopening the saved project reproduces the baseline exported pixels.
The Release multiwindow fixture still passes two shared device removals,
independent documents, Undo/Redo and clean shutdown.

These checks use the actual native input queue and process-owned D3D12 removal,
with controlled samples. They do not establish physical digitizer delivery,
sleep/driver-reset behavior, mixed-display acceptance or 120 Hz painting latency.
Broader interaction/visual acceptance and installed MSIX remain open; no package
or performance measurement was regenerated for this milestone.

The final Release document run also waits for Undo to reach the canvas before
checking the active pen segment. The complete Release editor regression passes
native titlebar hits, geometry, tools, both Zen modes, retained resize, themes,
workspace restoration and zero exit. Its tool-projection fixture now waits for
exact labels/selection/settings/actions on freshly acquired native controls;
the shared snapshot can arrive before XAML replaces the old schema. The original
assertions passed on the same failed window after publication settled, so only
the fixture needed adjustment.

### Shared title-bar and native ownership integration

Upstream integration brings the shared workspace-owned title-bar model, native
per-item kernel ownership locks and full Zen. The Windows editor fixture now
checks full Zen in both themes, hidden chrome, retained canvas/device on resize,
and restored workspace geometry. Its obsolete partial-Zen toolbars and Total Zen
preference checks were removed to match the shared behavior.

The integrated tree passes 653 ordinary core/engine/host/UI/Windows/workspace
tests, a normal Release build and the Web Wasm check. Native Release queued/active
pen recovery, exhausted-recovery Save/Save As with tail cancellation, full editor
and two-window recovery checks pass with clean shutdown. Strict Clippy currently
reports three incoming shared UI issues: the header measurement clamp, the drag
constructor argument count, and the large workspace-restore action variant.
These were not suppressed or represented as a passing check.

The subsequent committed-header picker baseline change also passes a fresh
normal Release build, the ordinary suites and Wasm check. The native workspace
manager verifies preview cancellation, history, starting-layout Undo/Redo,
creation, switching, rename/delete, brush reset, preview-close restart and clean
exit. Its header selectors now resolve stable workspace IDs through the shared
model, following the new Sketch/Paint/Photo keys without changing production
controls. Actual switching was confirmed in the same window as the obsolete
selector failure before rerunning the complete fixture successfully.

Windows still uses its existing native header projection, as allowed by the
shared port handoff. Projecting the new customizable title bar is remaining
Windows design work. Physical input/display, packaging and performance acceptance
remain open as described above.


### Portable and MSIX refresh after native pen recovery

The portable Windows 11 x64 package was rebuilt from clean published source
`33eead2674bf7b7474fc318417dc72de90c836fc`. It contains 1,079 files and its two
archive assemblies have identical SHA-256
`155220f475ffa3d45a912fd9090208dbd36bf3bfbb78dad158162fa3603f5615`.
The extracted package passes its complete inventory, launch from a path with
spaces and unrelated working directory, app-local runtime origins, packaged
filters, drawing/Undo/Redo, pan/resize and clean process exit on this host.

An unsigned MSIX was then assembled from that exact portable payload, using the
same committed packager source. Repeated assembly matches SHA-256
`321f7da0b3e759d7ce75ce3adc92d78d3978dba5078ef90c1da1151b0ba44b3f`.
Its 1,085-file archive passes inventory and MakeAppx extraction, activation
metadata, repeat logo generation, normalization idempotence, ZIP32 preservation,
signed-archive refusal without mutation and invalid-input rejection.

This verifies reproducible archive assembly for the recorded inputs and portable
runtime behavior on the development host. Installed MSIX launch/update/uninstall,
distribution signing and clean-machine acceptance remain open. No elevation or
installation was attempted. Artifacts and local reports remain outside Git.


### Immutable raster recovery and shared UI cleanup

The Windows port integrates the immutable raster/project work through upstream
`c1ec69e`. Successful renderer replacement now retains the existing active
stroke builder and queued samples. Completed strokes restore from immutable
raster roots; recovery does not replay historical strokes or add another queue.
The engine regression covers G Pen, Natural Blender and Watercolor, queued pen-up,
Undo/Redo and a second replacement that restores history without replaying dabs.

If reconstruction is exhausted, queued and unfinished contacts are canceled.
Completed host-backed raster edits remain saveable through the existing document
service. The obsolete path that tried to process raw pen samples without a GPU
has been removed. Commands admitted before failure are processed after renderer
suspension, so Save/Close remain available and GPU-dependent commands are disabled.

The integrated tree passes 664 ordinary core/engine/host/UI/Windows/workspace
library tests, a normal Release build and the C++ input/publication/queue tests.
Strict Clippy passes with warnings denied for Windows, UI and workspace, including
all targets and their library dependencies. Four document and three filter
recovery tests pass with actual process-owned D3D12 device removal, run serially.
Release document fixtures pass queued-stroke and active-stroke reconstruction,
exact PNG comparisons, history and thumbnails. Exhausted recovery passes Save,
Save As, cancel/discard and exact persisted-raster reopen. Two native windows
recover from two shared device removals while retaining independent documents
and Undo/Redo, then exit cleanly.

The pre-removal active-ink fixture now checks that the probe area is clear after
Undo and visibly contains ink after pen-down. Same-process inspection found one
channel value of difference in one pixel between live and committed sRGB8 output;
the final exported PNG was identical. Final image comparisons remain exact.
Project save checks compare complete persisted bytes rather than process-local
raster publication identities. Snapshot fixtures start with shared document
identities while preserving their complete model comparisons.

The shared title-bar cleanup retains NaN-safe native measurement bounds, replaces
eight positional drag arguments with one pickup-data struct, and boxes large
workspace payloads in shared enums. Native geometry is still captured at pickup;
drag behavior and serialized host/workspace formats are unchanged. Existing
storage/migration/history tests and an explicit restore-action JSON check pass.
Incoming simple Clippy issues were fixed without lint suppressions.

The native host build of the Web Rust crate also passes. The Wasm-target check
stopped at the new zstd native dependency because this Windows toolchain lacks
Clang; it is not a passing Web build. GTK, Apple and Android runtime validation
is outside this Windows check. The native color wheel remains the accepted
Direct2D implementation; imperceptible differences do not justify extra machinery.

This milestone does not refresh packages or establish physical pen delivery,
real driver reset, sleep/resume, mixed-display behavior or 120 Hz input latency.
The new workspace-owned title-bar editor and whole-editor visual acceptance
remain Windows work. No filter reference was regenerated or tolerance relaxed.


### Shared column-stack integration after raster recovery

The subsequent integration through upstream `45a1786` retains the validated
Windows recovery path and brings shared column stacks, default-layout migration,
Web event-loop raster capture and Apple document completion updates. Conflict
resolution preserves the new stack defaults and adapts incoming Rust callers to
the boxed workspace API. Three incoming test-loop lints were simplified while
retaining their assertions.

The combined tree passes 670 ordinary library tests (45 core, 48 engine, 25 host,
364 UI, 103 Windows and 85 workspace), strict all-target Clippy for Windows/UI/
workspace, a normal Release build, C++ input/queue/publication checks and the
native host build of the Web Rust crate. The final Release editor, workspace
manager and two-window device-removal fixtures pass with clean shutdown. These
cover titlebar hit regions, geometry, native tools and numeric editing, both
themes, full Zen, retained resizing, workspace history/preview/starting-layout
Undo/Redo, creation/switching/rename/delete, restart and shared-device recovery.

The editor fixture reacquires its shared snapshot as well as native controls
while waiting for tool projection. A transform transition can supersede an older
snapshot during raster completion. The original assertion passed against the
current model in the same failed window; the complete rerun then passed with all
label, selection, settings and action comparisons retained.

Windows now uses ordinary tabbed drawers while retaining shared stack membership
and preferences. GTK's replacement full-column opening remains a Windows port
item, alongside the workspace-owned customizable title bar. This integration
adds no claim of complete visual parity, physical input or 120 Hz acceptance,
and does not refresh the previously validated portable/MSIX payloads.


A later non-force atomic push encountered concurrent upstream title-bar overflow
work through `0c472fb`. That integration preserves the cleaned-up drag constructor
and adapts its two new test call sites. All 365 shared UI tests and strict Clippy
pass; the normal Windows Release rebuild also passes. Windows does not yet invoke
the shared HeaderDrag path, so the native editor/manager/multiwindow acceptance
above continues to cover the unchanged native interaction paths. The additional
header regression brings the ordinary test coverage across these suites to 671.


The final integration through `6aee96d` adds the Web column-stack projection and
shared stack geometry/history metadata. Host/UI/Windows/workspace suites pass
again (25/366/103/85 tests); unchanged core/engine suites retain their 45/48 passes,
for 672 ordinary tests across the validated suites. Strict Clippy and a normal
Release rebuild pass. The native manager rerun passes preview, starting-layout
history, Undo/Redo, creation, switching, rename/delete, brush reset, restart and
clean exit. Windows still uses tabbed drawers pending the full-column port.

### Native Windows column stacks and raster-worker integration

Windows now uses the shared full-column stack projection. Open members reuse
ordinary native panel groups; retained icon buttons, group dividers and connector
paths follow shared geometry. Every visible group's active icon is selected.
Closed multi-member stacks have no resize affordance; an open member retains
its own width and native tab controls while resizing. Shared Rust continues
to own drop validation, layout publication and one-step history.

The guarded production-editor fixture passes with OS-delivered mouse, pen and
touch. It covers immediate grips/tabs, held icon bodies and device-specific
menus, stack/member insertion, cancellation, Undo/Redo, member switching, retained
resizing, fixed closed widths, individual drawers, consumed auto-hide contacts,
both themes, full Zen and restart. Membership, widths and preferences persist;
temporary open columns do not. Paint opens its default right column on adoption
and reset. Sketch keeps its docked tools pending native header projection.

The integration includes upstream main through 23bc780, retaining Android/Web
stack and raster-worker changes. Platform gates and shared destination tests
include both Android and Windows. The combined ordinary suites pass 674 tests
(45 core, 49 engine, 25 host, 367 UI, 103 Windows and 85 workspace). Strict
all-target Windows/UI/workspace Clippy, the normal Release build and C++
input/queue/publication checks pass. Seven actual D3D12 document/filter removal
tests pass serially. Two incoming conditional lints and one unused native local
were simplified without suppressions.

The hardware readback helper waits for pending document edits to finish before
reading pixels, following the new deferred raster-frame contract. Its exact
import/recovery/Undo/Redo pixel comparisons remain unchanged. The Zen fixture
moves the pointer off the strip before asserting hidden chrome; hovering the
strip intentionally reveals it. The native color wheel remains unchanged.

This milestone does not establish whole-editor visual identity, physical
pressure/tilt/eraser behavior, mixed-display or real driver-reset acceptance,
installed MSIX acceptance, or sustained physical 120 Hz painting/input latency.

The final Release editor and workspace manager also pass titlebar hit regions,
shared/native geometry, tools, both themes, full Zen, retained canvas resizing,
preview/history/starting-layout transactions and restart. The complete native
RecoverGpu and FailGpu document journeys pass queued/active pen recovery,
exact exported pixels, thumbnail/history restoration, Save/Save As after
exhausted recovery, committed raster preservation, queued contact cancellation,
Cancel/Discard and durable reopen. Every owned process closes normally.

### Portable and MSIX refresh after native stacks

Both packages now contain clean production source 110a434, including immutable
raster recovery and native full-column stacks. The portable ZIP contains 1,096
files and is 57,012,058 bytes; its SHA-256 is
370d7af4116814c5bbd51fb5f1fb6179bd6deb48daeec33b6fba77429887b58a.
The unsigned 1.0.0.0 MSIX contains 1,102 archive files and is 57,339,803 bytes;
its SHA-256 is 6cba431c98f924698f7cb9068816da24f79b8a8874564d3a03681455c860ef10.

Repeated assembly produces identical archives. The extracted ZIP passes manifest
inventory, launch from a path with spaces and an unrelated working directory,
app-local runtime origins, filters, drawing/Undo/Redo, pan/resize and clean exit.
The package fixture now isolates PATH to Windows directories and restores the
calling shell afterward. A build shell otherwise supplies optional SDK shader
compilers and invalidates the clean-runtime check. Both the ordinary-shell run
and a run with SDK DXC deliberately present in the caller PATH pass.

MSIX archive inventory, MakeAppx extraction, activation manifest, repeated logos,
normalization, ZIP32, signed-package refusal and invalid-input checks pass. No
signing, installation, update/uninstall, clean-machine test or UAC retry occurred.
The payload is unchanged by this subsequent fixture/documentation-only update.

The subsequent integration through upstream ba77f9b brings Android titlebar
projection and the shared native HeaderRequest transport. Windows retains its
existing header and Sketch tools pending its own projection. Host/UI/Windows/
workspace suites pass again (26/367/103/85); unchanged core/engine coverage brings
the ordinary total to 675. Strict Clippy, normal Release, and complete native
editor/manager/restart checks pass. The packaged production milestone remains
110a434; the later shared-header integration is not included in those archives.

### Upstream port through c65e145 (2026-09-25)

This round integrates upstream through c65e145 and closes most of the gaps
found by the September Windows port audit.

Shader compilation. The Windows host fell back to the system FXC compiler
because no DXC shipped. Cold startup needed about 60 seconds to finish the
shader set, with single pipelines up to 9.5 seconds. A transform or brush
change inside that window waited behind the compiler, and the shared host
dropped the deferred contact, so an early Scale/rotate drag did not move the
selection. The build now copies `dxcompiler.dll` from the pinned
`Microsoft.Direct3D.DXC` package (internal validator, no `dxil.dll`), and the
host selects it explicitly with FXC as the fallback. On the development GPU
the startup shader set finishes in about 13 seconds, with no pipeline above
0.7 seconds. The extracted portable package loads the packaged compiler.

Workspace and chrome:
- Standalone Zen Capy, mouse pressure, right-drag panning and a pan cursor.
- A floating image-placement bar.
- Collapsed-column drawer sources stay joined, and glass corners match.
- Preferences: shortcut groups, defaults and modified markers; search empty
  state and focus; page icons. Space and Enter are recorded as shortcuts.
- Layer rename limits and state-aware accessible names.
- The Filter Types visibility setting is honored.
- Ctrl+Alt shortcuts work after toolbar clicks.
- Header focus follows folded menus.
- Tooltips no longer open on touch holds.
- Tool Set group rows match upstream's 112x36 layout.
- Proof is measured as fixed-height content.

Files:
- Drawings open from drops on the canvas or tab strip, from launch
  arguments, from the MSIX .capy association and from multi-select Open.
  A native queue opens them one at a time.
- Export remembers per-destination recipes and suggests the drawing name.
- Paste keeps PNG alpha and names the layer "Pasted image".
- Restore works while the current drawing is modified.
- Diagnostics saves stroke recordings, compressing them off the UI and
  render threads.

Frames that run without new input, such as filter loading or animations,
now wait for the next display refresh. Input-driven frames still present
immediately.

Fixture updates for current shared layouts bring canvas editing, documents,
switcher, Navigator, expansion, drawers, tab drag, HDR and the Photo header
back to passing. New fixtures cover tooltips and stroke recording. The touch
tab-pickup case still fails identically on the 980d4aa6 baseline: the
workspace loses capture while the torn-off tab is reparented.
