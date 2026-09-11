# Native Windows implementation

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
