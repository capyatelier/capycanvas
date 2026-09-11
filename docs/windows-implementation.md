# Native Windows implementation

Status: implementation started. No Windows UI, physical-pen, parity, or 120 Hz
acceptance has passed yet. The web reference runs in local Windows Chrome.

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
