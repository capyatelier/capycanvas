# Apple implementation goal and acceptance tracker

## Goal

Implement and validate complete native Capy Canvas apps for **iPadOS and macOS
together, with both platforms required at every milestone**. Use native UIKit
and AppKit hosts under `apps/layer-apple`, sharing Apple editor components,
bridge ownership and Metal integration. Keep platform-independent document
semantics, application behavior, brush dynamics, UI state/actions/layout and
GPU rendering in the shared Rust crates used by the other ports. Maintain one
implementation of each shared behavior and visual component; platform adapters
handle input, windowing, lifecycle and system services.

Achieve visual and behavioral parity with every feature exposed by the shared
and existing-host UI: all menus, actions, commands, tools, brushes, layers,
masks, blending, selections, fills, figures, rulers, transforms, filters,
dialogs, panels, docking/customization, shortcuts and Zen. Establish a complete
inventory and verify every entry on both platforms. Missing functionality must
not be hidden or removed to claim parity. The main editor retains shared assets,
geometry, colors and the live full-window canvas behind the title/header,
controls and HUD. Settings may use each platform's native appearance and
navigation while retaining complete shared functionality.
On macOS, top-level application menus live in the **OS menu bar**, as requested;
the editor header retains its title, Zen and other controls over the live canvas.
Zen clears the measured native close/minimize/fullscreen controls. These are
intentional Mac presentation differences, with menu functionality still in scope.

Implement reliable platform input: Pencil coalescing, supported sensors,
estimated-property corrections, visual-only prediction, palm rejection and
touch navigation on iPad; tablet/pen pressure, tilt and supported sensors,
mouse, trackpad and keyboard on Mac. Validate stroke termination, cancellation,
focus changes, hover/proximity, undo/redo and capture-time camera transforms.
Provide tested document save/reopen/autosave/recovery and settings/workspace
persistence across each platform's lifecycle and window/surface changes.

Build, install, launch and test the native apps on the attached 13-inch M4 iPad
and the development Mac, signing where required. Compare each native editor
against local Chrome at matching content dimensions, scale, state and sRGB
handling. Retain full pixel differences and geometry checks within one logical
pixel, tighter where exact alignment is possible. Permit only narrowly
documented platform rasterization differences and system-control accommodations.

Demonstrate sustained 120 Hz drawing on capable physical hardware for both
platforms, using representative simple and complex brushes, prediction where
supported, pen-up and 4K multilayer documents, including ten-minute sessions.
Measure CPU/GPU frame work against 8.33 ms and separately report actual
presentation cadence and input-to-present latency: p50/p95/p99, maxima, missed
deadlines, memory growth, idle behavior and thermal effects. Record the actual
display capability and refresh rate; a display limited to 60 Hz cannot establish
120 Hz presentation acceptance. A hardware limitation leaves that evidence open.
Do not substitute build success, simulator tests, timings from the other
platform, averages or reduced brush fidelity for acceptance.

Provide reproducible build/install/test commands and evidence for functionality,
visuals, persistence, lifecycle and performance. Pull other ports' changes,
integrate them, validate both Apple targets, and commit/push at milestones.
Keep signing material, account/team/device identifiers and private local data
out of GitHub. Completion requires demonstrated parity and performance on both
platforms with no required work remaining.

This scope supersedes the earlier iPad-only goal and the original design
review's treatment of macOS as a later port.

## Shared code and directory rules

| Location | Responsibility |
| --- | --- |
| `crates/layer-core`, `layer-engine`, `layer-render*`, `layer-ui` | Cross-platform document, input interpretation, rendering and UI policy. |
| `crates/layer-host` | Native session facade shared with Android and available to other native hosts. |
| `apps/layer-apple/native` | One Apple C ABI and Metal integration for both targets. |
| `apps/layer-apple/Shared/Bridge` | Serial owner, transport, snapshots and shared session coordination. |
| `apps/layer-apple/Shared/Editor` | Main editor views, controls, styling and layout consumption. |
| `apps/layer-apple/Shared/Settings` | Shared settings components with native platform navigation. |
| `apps/layer-apple/iOS/{App,Canvas,Input,Platform,Tests}` | UIKit scenes, surfaces, Pencil/touch adapters, services and platform tests. |
| `apps/layer-apple/macOS/{App,Canvas,Input,Platform,Tests}` | AppKit windows, surfaces, tablet/mouse/trackpad adapters, services and platform tests. |
| `tools/visual` | Common scenarios, Chrome capture and image comparison tools. |

Create directories when their implementation needs them. Inject platform views
and services into shared components; avoid duplicating editor screens or
spreading UIKit/AppKit branches throughout them. Preserve one serial Rust owner
per editor session with explicit surface lifetime and input ordering. Keep hot
input/render work off the UI thread and avoid publishing the entire workspace
for camera-only or stroke updates. Review shared scheduling, session and
persistence logic for reuse as both adapters develop. Independent windows must
not accidentally share a canvas owner, active contact or wake callback.

Use canonical assets and the shared catalog/layout. Stage bundle resources from
their source rather than maintaining platform-specific edited copies. Changes
to general behavior belong in shared Rust and must remain compatible with the
other ports. Platform files contain the smallest useful native adaptation.

## Milestone matrix

Each row requires its shared work **and both platform columns**. Partial iPad
evidence from earlier milestones remains useful but does not close the expanded
milestone. Build both Apple targets after shared changes; run the affected
functional, visual and performance checks on each as appropriate. A successful
build is only build evidence.

| Milestone and shared gate | iPadOS evidence / remaining work | macOS evidence / remaining work |
| --- | --- | --- |
| 1. Shared host, Apple bridge and native target builds | Device/simulator builds pass; device signed and installed. | Native AppKit target and Rust Metal library build pass. |
| 2. Launch, live canvas under header, idle scheduling and basic input | Physical app launches. Simulator launch/geometry capture passes. Physical Pencil, undo/redo and lifecycle checks remain. | Launch/render, full-window geometry, mouse stroke and keyboard undo/redo checked. Shared frame admission and final-state flush implemented; lifecycle/idle measurements remain. |
| 3. Complete input contract and bounded transport | Coalesced/predicted input foundation exists; corrections, sensors, palm/navigation, interruption and real Pencil evidence remain. | Mouse/tablet/proximity, wheel, trackpad and keyboard adapters exist. Physical sensors, complete shortcuts, interruption coverage and bounded transport remain. |
| 4. Complete feature inventory and editor/settings implementation | Initial shared editor controls exist; full inventory, specialized controls and all workflows remain. | Same shared controls compile; full inventory and native desktop actions/services remain. |
| 5. Document/settings/workspace persistence and lifecycle | Save/reopen/recovery, rotation, background/foreground, multitasking, surface replacement and memory-pressure checks remain. | Save/reopen/recovery, window ownership/close/reopen, focus, display/scale changes, sleep/wake and memory-pressure checks remain. |
| 6. Progressive visual acceptance for every editor component/state | Matching initial simulator/Chrome capture and full pixel report exist; baseline fails parity. Device captures and complete fixture matrix remain. | Matching native Mac/Chrome initial captures exist; baseline fails parity. Complete fixture matrix remains. |
| 7. Hardware performance, sustained sessions and delivery | Shared opt-in CPU/GPU/actual-presentation trace and local analyzer implemented; physical startup/idle instrumentation checked. Workload matrix, physical input latency, overhead calibration and ten-minute acceptance remain. | Same shared instrumentation and startup/idle check; display maximum is 90 Hz. Workload matrix, physical input latency, overhead calibration and ten-minute acceptance remain. |

Start input/performance instrumentation and persistence early, and run visual
comparisons as components land. The rows are acceptance gates, not a reason to
postpone one platform or defer all measurements until the end. Every feature
milestone includes implementations and relevant evidence for both targets.

## Evidence and remaining acceptance

The shared host extraction and navigation bridge have eight passing host tests and an Android ARM64
compile check. The incoming figure/ruler/affine-transform and staged GPU startup
changes have been integrated. Apple and Android now share staged frame preparation:
paper is submitted before consuming pending document replay; document and current
brush dependencies precede remaining shaders. Apple uses private disposable shader
caches and submits bundled filters after document readiness. Cold/warm startup
responsiveness still needs hardware measurement on both Apple platforms.

Six direct Apple bridge tests pass. Both iPad and Mac session configurations cover
isolation through brush/zoom/settings actions, and real GPU document pixels after
painting, pen-up, undo and redo, plus preservation of pending ink through staged
paper/document/brush readiness. Layer cases additionally cover rename, blend,
opacity, locks, references, checked selection preserving the drawing target,
mask targeting/linking/enablement, menu policy, hierarchy and collapse. Image
import rejects incomplete data without mutation, changes actual GPU pixels,
produces a thumbnail and restores exact pixels through undo/redo. A stateless
numeric test checks the shared expression, formatting and slider policy.
These exercise the same C ABI used by the editor
and the shared staged frame preparation. Standalone checks of the actual Swift
frame driver cover one queued frame, wakes during pending work, detached views
staying asleep, and old completions not revealing replacement surfaces.
They do not prove physical pen input or drawable presentation. Both native targets
build; Mac ad-hoc and development signing work, and the updated iPad app is signed
and installed. The native mouse-input test has passed; unsuccessful OS-menu click
automation was removed because it targeted the wrong menu and added no useful
editor coverage.
The staged-startup iPad simulator launch/geometry check also passes and retains
an unobstructed full-editor capture. Physical startup checks on both platforms
remain open; build and headless GPU results do not close that gate.

The inventory example (`cargo run -p layer-host --example inventory`) emits the
current catalog, command list, initial workspace and settings views. Extend it
through dynamic states and existing host controls: it is a starting point, not
a completeness proof. Track every entry's shared implementation, native service
dependencies and separate iPad/Mac verification. Incoming shared features from
other ports are in scope.
The latest shared Operation/transform controller is integrated and expands the
command catalog to 48 entries. Its specialized Apple controls remain unfinished.
The layer pixel reports below were captured before this final shared integration;
they do not establish acceptance of the new Operation workflows.

The inventory now includes six layer states on each Apple platform: paint/paper,
multiple checked rows, a mask with clipping/references and copied-mask state,
locked layers with disabled/unlinked masks, groups with children and collapsed
groups. Each records the shared row/header state and every available content/mask
context menu. Selection-dependent, imported-image and additional document states
remain to be enumerated.

The shared layer panel consumes these models for rows, blend/opacity, locks,
clipping/references, groups, mask/content targeting, rename, drag/drop, image
import and recursive context actions. GPU thumbnails have a separate observable
cache, visible-row requests, eight pending readbacks at most, stale-response
filtering and no polling once current. ImageIO decoding runs off the UI/render
queues and preserves orientation, sRGB, straight alpha and original resolution;
standalone synthetic-image checks pass. The serial owner performs document import.
Complete layer/menu/drag/long-press workflow acceptance and preview-cache stress
measurements remain open. Native blend/context popovers and other editor controls
still need visual refinement; the implementation is not a visual parity pass.
The focused layer workflow passes on the Mac and iPad simulator, including
creating a layer, checking another without changing the drawing target,
switching content/mask targets and deleting a mask through its context menu.
Explicit thumbnail hit shapes fix adjacent checkbox taps selecting content on
iPad. Context gestures attach directly to the relevant control, avoiding row
coordinate inference. These checks cover a small workflow, not every menu action.
The physical iPad app launches normally; its XCTest runner timed out while
enabling automation before running any assertions. Device input and performance
evidence remain required; simulator results do not replace them.

The initial iPad simulator launch test verifies a successful Metal viewport
submission, a full-window canvas, settled landscape bounds and 36-point Zen
control, and retains a full-screen capture. It does not establish presentation
timing or Pencil behavior. Use a full-screen XCTest capture for the simulator;
the app-scoped capture was observed to crop landscape content incorrectly.

The initial valid light-theme comparison at 1376×1032 logical points and scale 2
has 436,646 differing pixels out of 5,680,128 (7.6873%). This is a **failing
baseline**, with no tolerance accepted. Native main-page menus/sliders and the
incomplete layer panel still need work. The initial dark Mac comparison at
1200×900 logical points and scale 2 has 382,685 differing pixels out of 4,320,000
(8.8584%). This is also a **failing baseline**, including the intentional native
menu/window-control adaptation. No visual result has been accepted.
The layer-panel iteration at 1200×870 and scale 2 has 346,758 differing pixels
out of 4,176,000 (8.3036%), also failing. The reference now waits for staged
startup and visible GPU thumbnails, and both captures are unobstructed. The
remaining differences include text, control geometry/styles and the intentional
Mac header adaptation. This is a different window size from the earlier baseline,
so the percentages do not establish an improvement rate.
The iPad simulator's settled `layer-added` fixture at 1376×1032 and scale 2 has
383,613 differing pixels out of 5,680,128 (6.7536%), also failing. It matches one
new empty selected layer over the original ink/paper, with GPU previews present
in both captures. Early launch captures with unfinished previews are retained
locally but excluded from this fixture's parity evidence.
The comparison tool has four passing checks covering single-pixel errors,
orientation, dimension mismatch and transparent captures.

Visual fixtures must match logical size, pixel scale, application/document state,
insets and sRGB handling. Compare complete images without resizing or hiding
differences; retain overlays/heatmaps and regional geometry evidence. Cover
light/dark, iPad landscape/portrait, resized Mac windows and display scales,
menus/popovers/dialogs, disabled/selected controls, docking/floating panels,
Zen and painted/zoomed artwork continuing behind the header. Explicitly account
for native system controls while preserving the app's title/header comparison.
Settings receives full behavioral and native snapshot/accessibility checks.
Prefer direct editor action/state and canvas-output checks plus direct window
captures for routine iteration. UI automation is reserved for specific app
input/lifecycle risks. macOS menu mechanics are trusted; tests target the editor
effects after an action is dispatched, including undo/redo and the correct session.
Coordinate testing of the app's own canvas and controls is allowed where it
provides useful coverage; coordinate testing of the system menu bar is excluded.

For hardware performance, include simple/complex brushes, erasing, blending,
watercolor, liquify, large brushes, dense 4K multilayer documents, prediction
where available and pen-up. Report queue age, acquisition, CPU preparation, GPU
completion and actual presentation separately. An app's submitted-frame count
or CPU frame costs alone do not prove 120 Hz presentation or input latency.
Keep per-platform results separate and retain failing workloads in the report.
The current Mac display reports a maximum of 90 Hz. It can provide CPU/GPU and
90 Hz presentation measurements, but cannot establish the 120 Hz presentation gate.

Generated captures and test results belong in ignored `artifacts/ui/parity`;
local timing traces and reports belong in ignored `artifacts/performance`. Signing material and device/account identifiers remain
local. Repository commits contain source, reproducible commands with placeholders
and sanitized findings. Review staged content before each push.


Shared Apple performance instrumentation is documented in
[`apps/layer-apple/PERFORMANCE.md`](../apps/layer-apple/PERFORMANCE.md). Opt-in
captures distinguish CPU queue/service/stages, GPU queue spans, actual Metal
presentation callbacks, display-link idle transitions, input receipt associations,
memory and thermal state. The recorder and GPU readbacks are bounded. Missing,
skipped, invalid and overflow observations remain explicit; input receipt is
not proof that the frame includes those pixels. The GPU test exposed stale/zero
Metal counters: marker passes now contain a storage write, and counter resolution
runs asynchronously after marker completion. Hardware checks require positive
timestamps and verify slot saturation/reuse. The older renderer telemetry rejects
zero counters but still needs migration from empty marker passes; these Apple
reports use the separate queue-span timer.

The native scheduler and concurrent recorder checks pass without app automation.
Four report tests distinguish active missed frames from idle gaps, preserve
missing/invalid observations, deduplicate receipt associations and require shader
readiness. Seven Apple C ABI tests and the hardware GPU timer test pass. Both
signed builds compile and direct physical app launches work. Startup/idle probes
validate instrumentation only; they do not close any drawing workload, physical
Pencil latency, 120 Hz sustained-session, visual or functional parity gate.


The corrected 30-second Debug startup/idle probes recorded 1,545 valid GPU spans
on Mac and 510 on the physical iPad. Every acquired drawable received a
presentation callback: Mac reported 1,544 actual presentations and one zero-time
(skipped/unpresented) callback; iPad reported 507 actual presentations and three
zero-time callbacks. Neither trace overflowed, skipped GPU observations, retained
pending GPU readbacks or reported invalid GPU counters. Shader/canvas/catalog
readiness was observed at about 17.59 seconds on Mac and 4.41 seconds on iPad,
followed by render idle. These values include startup and profiler overhead;
they are not representative drawing benchmarks. The probes precede integration
of the incoming linked-mask/sparse-transform work. Early all-zero GPU traces are
retained locally as rejected instrumentation evidence and excluded from timing
distributions by the analyzer.


The incoming linked-mask transform and sparse-capture work is now integrated.
Both signed Apple builds pass after that merge; the merged physical iPad app
installs and launches normally, all 15 Apple/shared-host tests pass, and the
shared GPU crate checks for WebAssembly. The broader Metal GPU run passed 100
tests, ignored 16 explicitly marked tests and reported two failures. One was a
startup-cache test hardcoded to Vulkan; it now chooses the available backend,
checks exact staged/eager pixels on Metal's unsupported-cache fallback and
retains persistence assertions when driver caches are supported. Its isolated
Metal run passes. The other, `runtime_filter_pixel_reference`, still fails with
maximum channel error 255. The same failure reproduces in an isolated checkout
of the pre-milestone `d263697` baseline, so it predates these instrumentation and
merge changes. It remains an open rendering/visual acceptance issue; the fixture
and tolerance were not changed. Full filter parity is not accepted.
