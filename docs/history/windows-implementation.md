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
