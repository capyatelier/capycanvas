# Apple implementation goal and acceptance tracker

> Historical design or validation record. Statements about completion and remaining
> work describe the recorded checkpoint. Start with the [current technical guides](../README.md).

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
integrate them, validate both Apple targets, and commit/push completed major
milestones to the shared `main` branch. Group supporting fixes and validation
with their milestone rather than publishing each small task separately.
Keep signing material, account/team/device identifiers and private local data
out of GitHub. Completion requires demonstrated parity and performance on both
platforms with no required work remaining.

This scope supersedes the earlier iPad-only goal and the original design
review's treatment of macOS as a later port.

## Shared replay milestone — 2026-09-11

A direct renderer regression reproduced the intermittent Metal startup failure
without AppKit, UIKit or frame recording. Replaying seven filled 4096×4096 paint
layers together exhausted wgpu's limit of 4096 outstanding native command
buffers; rendering the same fills incrementally succeeded. The shared renderer
now splits recording after 512 render/compute passes and finishes/submits each
chunk in queue order. Staging uploads close before submission and their reuse
callbacks remain on the final chunk. Production adds no CPU wait for the GPU.

The full 4K replay now matches every exported incremental pixel with renderer
telemetry off and on. The renderer suite passed 117 checks, with 18 explicit
hardware benchmarks/stress checks ignored and the known strict filter-reference
failure retained. Its actual PNG and difference report are byte-identical to
unchanged `fa71061`: 3,274 pixels above one byte across 70 cases, maximum error
47. The new replay stress check was run explicitly and passed.

Changes from the other ports through `fa71061` are integrated. Apple tests now
look up diagnostic metrics by label and select the Brush size tab explicitly,
following the shared layout changes. All 32 Apple, 15 host and 233 UI checks
passed; the existing host hardware benchmark remains ignored. The shared renderer
also passed its WebAssembly compile check. Both Apple Release targets rebuilt;
the signed iPad build was installed into the separate benchmark app.

Both physical hosts completed twenty measured seconds of `wet-watercolor-4k`,
plus warm-up and postlude, using the published display scheduler. Each delivered
4,513 nonpredicted samples. The measured Mac interval recorded 1,645 actual
presentations and iPad recorded 2,201, with zero rejected input, missing callbacks
or zero-time presentations. The Mac's completed canvas was captured and visibly
contains the synthetic paint. Both benchmark apps were stopped afterwards, with
the original Mac editor preserved. Build/device logs, traces and images stay in
ignored local artifacts.
The full traces retain one Mac and four iPad zero-time presentations outside the
measured intervals, plus 14 and 19 omitted GPU samples respectively. Neither
trace contains a renderer error or recorder overflow.

The final integration also includes Windows milestone `ed27d2b`, which extracts
the existing shared thumbnail query into a reusable native helper. The Apple
image-import/thumbnail pixel check passes on both platform profiles after that
integration, and both Apple Release builds pass. The short device pilots above
precede this helper extraction.
The subsequent shared resize milestone `f6fe901` is also integrated: all 234 UI
checks, the Apple workspace drag/cancel/history pixel check on both profiles,
and both Apple Release rebuilds pass after the rebase.

This milestone establishes replay correctness and submission capacity on the
shared renderer. The separate Apple display-link experiment remains unpublished.
Full visual/feature parity, sustained performance, instrumentation calibration,
physical input latency and the remaining lifecycle matrix are still open.

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
| 2. Launch, live canvas under header, idle scheduling and basic input | Physical app launches. Simulator launch/geometry capture passes. User confirms basic Pencil pressure, pen-up and Undo/Redo; full lifecycle checks remain. | Launch/render, full-window geometry, mouse stroke and keyboard undo/redo checked. Shared frame admission and final-state flush implemented; lifecycle/idle measurements remain. |
| 3. Complete input contract and bounded transport | Coalescing, prediction and estimated corrections implemented with synthetic oracles. Basic physical Pencil/palm check passes; correction delivery, full sensors/navigation/interruption coverage remain. | Mouse/tablet/proximity, wheel, trackpad and keyboard adapters exist. Physical sensors, complete shortcuts, interruption coverage and bounded transport remain. |
| 4. Complete feature inventory and editor/settings implementation | Initial shared editor controls exist; full inventory, specialized controls and all workflows remain. | Same shared controls compile; full inventory and native desktop actions/services remain. |
| 5. Document/settings/workspace persistence and lifecycle | Atomic preferences and private artwork recovery implemented; Simulator settings/workspace and artwork restart checks pass. Manual Save/Open, recovery and checkpoint policy pass direct checks; provider delivery and full physical lifecycle matrix remain. | Same persistence and recovery; native restart and owner isolation pass. Manual Save/Open, recovery and unsaved close pass direct checks; full window/display/sleep/memory-pressure matrix remains. |
| 6. Progressive visual acceptance for every editor component/state | Matching initial simulator/Chrome capture and full pixel report exist; baseline fails parity. Device captures and complete fixture matrix remain. | Matching native Mac/Chrome initial captures exist; baseline fails parity. Complete fixture matrix remains. |
| 7. Hardware performance, sustained sessions and delivery | Shared recorder and five synthetic profiles implemented. One physical ten-minute 4K watercolor run completes; CPU p99 is 9.120 ms. Full matrix, frame-budget tails, physical latency and overhead calibration remain. | Same profiles; one native ten-minute 4K watercolor run completes. Current display configuration is 90 Hz. Full matrix, memory growth, physical latency, overhead calibration and 120 Hz evidence remain. |

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
[`apps/layer-apple/PERFORMANCE.md`](../../apps/layer-apple/PERFORMANCE.md). Opt-in
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


Filter investigation now separates a reproducible import-color issue from the
saved-reference discrepancy. Shared GPU import explicitly decodes sRGB bytes
before linear paint storage; all encoded channel values at six alpha levels
match the transfer-curve reference within one exported byte. The original
`3f6d2d5` filter implementation also fails against the saved PNG on Metal. With
the same explicit import decoding, the original and current implementations
match exactly across all 160 filter/scope cases. The PNG fixture and one-byte
threshold remain unchanged and failing; this is not a full pixel parity pass.
See [`docs/../reference/runtime-filters.md`](../reference/runtime-filters.md) for the evidence and new
per-filter failure artifacts. Further cross-backend numerical investigation
remains part of visual acceptance.


The color-conversion milestone passes both signed Apple builds and all 15
Apple/shared-host tests. Its broader GPU run passes 102 tests, with the known
saved-reference failure and 16 explicitly ignored tests retained. After
integrating the parallel transform-readiness and dock-topology work, both Apple
builds, all 15 host tests, five GPU-startup tests, 51 shared-layout tests and the
WebAssembly compile check pass. The merged iPad build installs and launches,
and the merged Mac build launches. The new import-color check covers 1,536
pixels (all 256 encoded values per channel at six alpha levels). GUI parity
captures, full native controls and sustained input/performance acceptance remain
open; these direct checks do not substitute for them.

The next Apple control milestone implements the shared Tool Settings schema and
command actions on both platforms, reachable from the Workspace menu. Tool Set
projects non-paint groups/subtools (including Operation, rulers, figures and
region sources). Painting retains the full catalog list, matching the current
web host, because restricting it to the active family would hide brushes before
Apple toolbar customization is implemented. Complete drawers, toolbar pickers
and the remaining specialized panels are still open.

Numeric controls now use shared Rust expressions, units, hard/soft ranges,
slider mappings and stepping, with thin tracks, value fields and step buttons.
Apple shares the optimistic edit/acknowledgment state and local error feedback;
small native text adapters handle platform focus, selection and key commands.
Rapid snapshots preserve unfinished drafts; rejected semantic edits restore the
accepted value. Changing the tool or layer/mask target discards old field drafts.
The iOS Simulator omits unavailable Metal presentation callbacks, rather than
reporting invented presentation events.

Both final signed builds pass; the physical iPad installs and launches and the
Mac launches. All nine Apple ABI tests and eight shared-host tests pass. The new
ABI coverage edits every visible setting across the complete brush catalog for
both Apple platform configurations, checks ruler toggles, and validates actual
Metal transform preview, rejected zero scale, Apply/Cancel and exact pixel
Undo/Redo. The standalone Swift checks cover draft preservation, queued edits,
rollback and f32 acknowledgment. Platform panel-availability coverage passes.

The focused native UI test passes on Mac and iPad Simulator: expression entry,
step buttons, shared value updates, invalid-value feedback and brush selection.
Mac Escape cancellation passes. On Simulator, both app-level and focused-field
XCTest Escape probes reached neither UIKit key commands/presses nor text
insertion; the final test verifies correction of a rejected expression instead.
Physical iPad Escape delivery remains explicitly unverified. Temporary keyboard
tracing was removed. No OS menu bar was coordinate-tested.

Fresh initial light-theme captures use Mac 1200×870 and iPad Simulator
1376×1032 logical pixels, both at 2×. Exact comparisons still fail: Mac has
421,544 / 4,176,000 differing pixels (10.0944%); iPad has 372,895 / 5,680,128
(6.5649%). The raw Simulator image required an explicit lossless 90-degree
counterclockwise orientation correction, recorded by the comparison tool;
original images and all full-image artifacts remain local. Five comparison-tool
tests pass, including preservation of a single-pixel error after rotation.
These initial-state results are not directly comparable to the earlier
layer-added scenario. Tool Settings is currently implemented in GTK/shared
models but absent from the web renderer, so its native captures are inspection
evidence rather than matching Chrome fixtures. Complete visual, physical-input,
persistence and sustained-performance acceptance remains open on both platforms.

After integrating the subsequent shared collapsed-column interactions and
explicit GPU import quantization, both signed Apple builds pass, as do all 223
Apple/shared-host/shared-UI tests (9 + 8 + 206). The Metal filter-library checks
pass 15 tests, including the transfer-curve oracle; the unchanged saved-reference
failure remains at maximum channel error 255, with three explicitly ignored
benchmarks. The control UI tests and visual captures above precede that merge;
they do not validate the new collapsed-column UI, which Apple still needs to
project. The final merged builds install/launch through the normal native paths.

The next shared Apple milestone adds the native Color panel on both platforms:
HSV square, HLS triangle, foreground/background/transparent paint, swap and
component expressions. Rust supplies normalized geometry, hue memory, colors,
numeric specifications, hit policy and bounded picking; Apple shares rendering
and controls, with small native contact adapters. RGBA channel edits apply to
the current owner state so queued edits cannot overwrite other channels.

Focused Mac and iPad Simulator tests pass for color-space switching, hue/field
picking, expression entry, paint slots, a latched hue drag that exits transparent
paint, empty-corner rejection and swap. Existing numeric-control workflows also
pass on both. These checks found and fixed the outlined icon's incomplete hit
area and an AppKit focus transition that discarded a click into an always-visible
field. Temporary native event tracing was removed. Both signed apps build; the
physical iPad app installs and launches. Physical Pencil input remains a separate
acceptance requirement; Simulator touch does not establish it.

Each native color capture is sampled against Rust's actual picker at matching
normalized coordinates, after ICC conversion. Both platforms pass 758 HSV and
587 HLS samples within two 8-bit channel levels. Mac HLS field maximum error is
one level; the remaining ring/field maxima are two. Initial perceptual-gradient
and mesh-gradient attempts failed the same check; explicit device-space linear
gradients now reproduce display-encoded square and triangle interpolation. The
checker excludes only the declared boundary neighborhood and marker radii, and
rejects missing/transparent interior samples. This is sampled color correctness,
not full-image parity: the web renderer still lacks a matching custom wheel.
All captures, geometry metadata, reports and failed attempts remain local.

All 226 shared/Apple tests pass (207 UI + 8 host + 11 Apple ABI), including actual
Metal brush color, transparent erasing with the current tip and exact pixel Undo.
The WebAssembly compile check, standalone numeric edit-state checks and nine
visual-tool tests pass. Full feature inventory, remaining specialized panels,
persistence, complete visual/input coverage and sustained hardware performance
remain open on both platforms.

After integrating the incoming live collapsed-toolbar drawer work, both signed
Apple builds and all 227 shared/Apple tests pass (208 UI + 8 host + 11 ABI).
WebAssembly compilation passes; the merged physical iPad build installs and
launches, and the merged Mac build launches. The focused UI and color captures
above precede this merge; they do not validate Apple drawer projection, which
remains unfinished. No full-image or hardware performance gate is closed by
these integration checks.

The persistence milestone adds shared Apple settings and per-scene workspace
storage using the existing Rust models. Reads and atomic private-file writes run
on a separate I/O queue. The owner reserves restoration ahead of input and surface
tasks. Scene IDs survive system scene restoration; a new scene receives its own
initial workspace file without changing the default used for future windows.
Settings propagate across owners after successful commits, with pending local
writes protected from stale notifications. Failed saves retain accepted edits,
report errors and support retry. Invalid saved data is reported and preserved.

The shared native snapshot now emits workspace persistence data only when the
committed topology changes. In-flight drags, measurements and scroll allocations
are excluded by Rust; camera and ordinary brush updates cause no workspace saves.
Mac termination and iPad background adapters flush accepted work across both
queues. Their complete physical interruption/expiration matrix remains open.

Both signed builds pass. The standalone filesystem tests pass atomic old/new
generation reads, private permissions, limits, failure preservation and scene
isolation. Real Swift-owner/C-ABI tests pass on both platform configurations for
restore ordering, rapid cross-owner settings edits, exact restored workspaces,
write acknowledgments and forced write failure followed by retry. Focused Mac and
iPad Simulator UI tests retain the dark theme and Color panel across termination
and relaunch without fixture actions. They use isolated private namespaces; other
UI fixtures disable storage. All 228 shared/Apple tests pass (208 UI + 9 host +
11 ABI), as does WebAssembly compilation. See the reproducible commands and
remaining storage gates in [Apple persistence](../../apps/layer-apple/PERSISTENCE.md).
The final signed iPad build installs and launches on the attached device, and
the final Mac build launches normally. These launches do not establish physical
background-task expiration or termination/interruption persistence acceptance.

This does not save artwork through the application. File actions, autosave/recovery,
complete lifecycle acceptance and measured storage overhead remain required,
together with the other feature, input, visual and hardware performance gates.
No complete persistence or overall parity claim is made.

## Shared project integration

The incoming shared `Project` codec and watercolor material-update replay fix
are integrated. Both Apple targets use that format; there is no Apple-specific
document schema. Source images now remain available after import through the
shared renderer contract, and snapshots share immutable bytes for used images
and brush masks. The codec validates current editable content, prunes unreachable
history/assets and preserves exact effect definitions. Reopening starts a fresh
undo history. See [project format](../reference/project-format.md).

All 300 model/engine/UI/host/Apple tests pass (40 core, 31 engine, 208 UI, 9 host,
12 Apple). The new Apple case covers both platform configurations and compares
every document byte after fresh Metal replay of an imported image, textured
painting, applied mask and transform. New edits and undo also preserve the
reopened pixels. The separate live/reopened GPU workload, including subsequent
wet painting and multipass filters, passes on Metal. WebAssembly compilation
and signed macOS/iPadOS builds pass.

These are shared-code and headless GPU checks. Native Save/Open integration,
atomic artwork writes, recovery, live-gesture snapshot policy, background
validation/compression scheduling and physical lifecycle/performance evidence
remain open. In particular, retaining imported source bytes increases resident
memory by four bytes per image pixel; archive snapshots share those bytes, but
large-image memory peaks and storage latency still need measured acceptance.


## Native file transport integration

Apple now uses the document requests and undo-aware saved checkpoints introduced
by the GTK port. Both targets implement manual New/Open/Save/Save As through the
same Rust policy and shared Swift coordinator. Apple replaces the current editor
after shared unsaved-change confirmation; native Mac close/quit uses the same
Save/Discard/Cancel flow. Private atomic writes, cancellation boundaries, background
validation/GPU preparation, stale-result rejection and previous-document input
invalidation protect the live drawing. iPad Save As stages the archive before
asking the native export picker for its destination. Multiple iPad scenes are
enabled; their complete lifecycle behavior remains unverified.

The consolidated shared regression run passes 311 tests: 41 core, 32 engine,
214 UI, 10 host and 14 Apple. Actual Metal reopen pixels remain exact on both
Apple platform configurations. New checks cover saved-state branches, replacement
confirmation, animation clock reset, delayed input, cancellation, malformed input
and preservation of edits made while a file task runs. The standalone Swift file
checks cover real owner/coordinator effects with deterministic dialog choices;
native picker interaction and provider delivery remain separate evidence.

This is progress on persistence, not completion of that gate. Artwork autosave,
recovery, canvas-size creation UI, Apple PNG export, provider conflicts and
physical lifecycle/storage performance remain open. The existing full-image
visual failures, complete feature inventory and sustained hardware targets are
unchanged acceptance requirements. See [Apple persistence](../../apps/layer-apple/PERSISTENCE.md).

Both integrated signed builds pass. The merged iPad app installs and launches
normally on the attached device, and the Mac app launches. The Swift staged-export
and destination-first save checks pass, as do the settings/workspace regression
checks and WebAssembly compilation. These are launch and direct-effect results;
no new full-editor visual or sustained hardware acceptance is claimed.


## Canvas creation and PNG export

Both Apple targets now expose a native New drawing size form from the shared
catalog, with a 2048×1536 default and 1…8192 pixels per dimension. Validation runs
again in Rust before candidate allocation. PNG export uses the existing document
composite and shared RGBA8/sRGB encoder, also used by GTK. It excludes viewport
inspection aids and preserves the editable document's location and save checkpoint.

The owner submits a GPU snapshot, then transfers a ticket to the file worker.
Shader preparation, GPU waits, row packing and PNG encoding occur off the input
owner. The Metal check proves captured pixels remain exact after subsequent
painting and destruction of the original renderer. It also verifies non-aligned
row widths, dimensions and sRGB metadata. This scheduling removes synchronous
export waits from input dispatch; it does not establish the hardware frame budget
or large-document memory/latency acceptance.

A startup race found by the Mac UI check is fixed: file capture waits asynchronously
for bundled filter preparation. Bundled loading updates the filter library without
migrating embedded document definitions or changing the saved checkpoint. The
regression check exercises that preservation on both Apple configurations.

The shared regression suites pass 319 tests (41 core, 32 engine, 4 render,
215 UI, 11 host and 16 Apple). Standalone Swift document checks pass both platform
configurations, including sized creation, cancellation, PNG decoding, durable
writes and checkpoint preservation. The iPad Simulator workflow passes native
size entry, export cancellation and closing the last clean drawing. iPad's
floating number pad consumes an initial outside tap; the test dismisses it before
activating Create. The equivalent native Mac creation/export cancellation check also passes, using
shortcuts and Escape without system-menu coordinate automation.

The latest Android drawers/Navigator and GTK shared menu policy are integrated.
Mac workspace commands now appear in the native Window menu. The old unsupported
column test is updated for Android's new support, with a positive Android drawer
check and continued coverage of the Apple behavior awaiting column projection.
Both signed builds and WebAssembly compilation pass. The iPad build installs and
launches on the attached physical device; the final Mac build launches normally.

Artwork autosave/recovery, complete file-provider delivery and conflict handling,
physical lifecycle coverage, full editor pixels, the remaining feature inventory
and sustained 120 Hz workloads remain open. These file-workflow results do not
close those acceptance gates.


## Shared application menus and shortcut editing

Apple now projects File, Edit, Layer, Select, Filter, View, Window and Help from
the same live Rust menu models used by GTK and Android. Both native ports share
the incoming Android menu snapshot/query contract. Menu actions carry typed
keyboard chords, including custom/contextual bindings, alongside display hints.
The native Mac app places Settings/About in their standard application menu and
uses the OS menu bar for top-level menus. iPad keeps its menus over the canvas;
the title moves beside wide menus, with a complete submenu fallback when space
is limited. A clean landscape Simulator capture shows all eight menus with no
title overlap. Full pixel acceptance and the complete narrow-window matrix
remain open.

Keyboard Shortcuts now has a searchable action list, alternate binding editor,
Add/Remove, conflict replacement, per-action reset and Reset All. Native capture
uses the shared Rust validation and conflict policy. Mac captures events before
menu equivalents; iPad uses a focused native responder. Capture emits complete
key pairs so closing its sheet cannot leave a canvas key held. Named/function
keys use the same platform translation in capture and ordinary canvas input.
Settings search navigates to shared search results; About and shortcut commands
open the corresponding native settings page. Help links use the shared URL
resolver and acknowledge native browser handoff.

Direct Metal checks pass on both Apple configurations for actions selected from
the actual menu models: clear, full pixel selection/fill, deselection, Gaussian
blur insertion, and exact pixel restoration through Undo. The host check verifies
all eight transported models, link availability and current command state. Camera
patches contain no menu trees; unchanged snapshots remain absent. This does not
establish the cost of full menu publication in the sustained hardware workloads.

The focused shortcut UI workflow passes on Mac and iPad Simulator: search for
Zen, capture Command-Z, show the Undo conflict, explicitly replace it, close the
editors and toggle Zen twice with the new binding. It uses no system-menu clicks.
The affected regression suites pass 244 tests (215 UI, 12 host and 17 Apple).
The inventory example additionally emits menu states for initial content, a pixel
selection, a locked target, shortcut editing and a conflicting captured chord
on each Apple platform.

This closes the missing top-level menu projection and basic shortcut editor gaps.
Complete action/customization workflows, filter/property visual acceptance, recovery,
physical input/lifecycle coverage, full visual parity and sustained performance
remain required. In particular, the current shared capability policy still omits
New Window on iPad; enabling multi-scene support alone does not verify that flow.


After adopting the incoming Android document/menu changes, both shortcut UI
workflows and all 244 affected regression tests pass again. Both signed builds
pass; the final app installs and launches on the attached iPad, and the Mac app
launches normally. WebAssembly compilation also passes. Private screenshots,
logs, device/signing details and test artifacts remain outside version control.

## Shared filter picker and properties

Both Apple targets now project shared filter categories, search, empty state,
insertion actions and the live Properties schema. Number, toggle, choice,
straight sRGB color/alpha, curve and gradient controls use shared editing/reset
actions. Curve plots come from Rust's sampled interpolation; point ordering,
endpoint protection, gradient insertion colors, validation and undo stay in Rust.
Numeric fields retain drafts across ordinary updates, with a stable schema key
to reset them when their target/schema changes. Locked properties disable native
controls and graph hit testing.

One preview cache per editor combines visible rows across panel projections.
Requests contain at most eight rows, bounded to 512 by 128 pixels each. Painting
and pending document edits defer new requests through shared policy. The C ABI
transfers an owned straight-RGBA atlas independently of the editor; Swift image
creation runs on a utility worker. Polling is nonblocking, stops when visible
rows are current, and does not run on zoom-only snapshots. Document epoch,
paint/active-layer/catalog revision and pixel size reject stale results. These
bounds do not establish hardware performance acceptance.

Filter search exposed iPad keyboard avoidance translating the fixed dock layout
above the screen. The scene now retains full-window geometry while the keyboard
covers its lower region. The focused iPad test verifies stable canvas position
and height plus an onscreen search field, then exercises GPU preview loading,
radius expression input, curve insertion/reset and gradient insertion/position/
reset. It passes with the final graph gesture handling. Lower controls, floating
keyboards and the full input/lifecycle matrix remain open.

After integrating the GTK workspace milestone, all 249 affected tests pass:
217 shared UI, 12 native host and 20 Apple bridge tests. Apple checks cover all
six property kinds, reset and undo/redo on both platform policies; actual Metal
radius changes with exact undo/redo pixels; and preview buffer ownership after
editor teardown without changing document pixels. Signed builds for both targets
and the WebAssembly build pass. The updated physical iPad installs and launches.

Mac filter search, preview loading and insertion reached Properties during UI
runs. Subsequent complete runs stopped before assertions while macOS displayed
the XCTest Touch ID/password prompt to enable UI automation. The direct control
utility also reports missing Accessibility access for its launching session.
Full Mac gesture-workflow evidence remains open; these setup failures are not
passing tests. Independent bridge/Metal results still cover Mac.

The Chrome `filter-properties` fixture matches the tested stack and viewport.
The first valid full iPad capture differs at 8.2396% of pixels at zero tolerance.
After removing the redundant gradient label and adding inline stop opacity, a
direct final simulator capture differs at 7.9577%. Its portrait raster is rotated
90 degrees counterclockwise without resampling, then every pixel is compared.
Header, control and styling differences remain. Raw images, full difference,
heatmap and overlay stay local. The Mac capture is obstructed by the
pending permission dialog and is rejected as a comparison fixture. Full visual
parity, all panel projections/customization, recovery, physical input and
sustained hardware performance remain required; this is a partial milestone.

## Shared Navigator and Diagnostics

Navigator is now exposed by Apple capability policy and rendered by one shared
SwiftUI panel. Its six camera controls, drag/recenter/cancel behavior, document
aspect fit and rotated/reflected work-area outline use Rust actions and geometry.
A stateless geometry ABI consumes the already-published camera patch, avoiding
asynchronous session queries or duplicated camera math on the UI thread.

The preview path uses the existing shared 15Hz producer and a 256px maximum
dimension. The serial owner keeps one poll scheduled and one image delivery in
flight; utility-worker decoding must acknowledge delivery before another image
is sent. Camera-only frames reuse the image. Polling sleeps once the current
composition is consumed, including a final stroke that arrives inside the
throttle interval. Owned straight-sRGB pixels carry a document epoch. Document
replay and replacement cannot publish a previous drawing as the new preview.

Diagnostics projects the seven shared statistics rows, descriptions, 120-sample
chart and frame-budget reference. Its 5Hz query task runs only while visible.
Review found that restoring an open Diagnostics panel before GPU attachment, or
replacing its document renderer, lost the visibility-dependent sampling flag.
Attachment and shared project adoption now reapply that policy.

All 253 affected regression tests pass (217 UI, 12 host, 24 Apple). The four new
Apple tests exercise both platform policies: exact artwork/history preservation
through Navigator gestures and all six commands, owned preview pixels after
editor teardown, idle/camera reuse, delivery of the throttled final stroke,
replacement while an old preview is pending, and Diagnostics sampling across
attachment/replacement/hiding. The first focused iPad UI run exposed a missing
capability flag; after fixing it, the complete preview/controls/drag/Diagnostics
tab workflow passes and its full-screen capture is retained locally. Both signed
targets build; the updated physical iPad installs and launches.

The parallel shared in-surface GPU overview foundation is integrated, and its
three non-benchmark tests pass on Metal. Apple currently uses the bounded
exported-preview path above. Connecting the new in-surface presenter requires
native panel transparency/stacking integration and hardware measurement; its
renderer-only evidence does not validate Apple's current preview transport or
the sustained 120Hz target.

Screen recording now works, but the separate XCTest authentication dialog still
obstructs the Mac capture. Mac UI automation was not repeated in that state.
The current web host does not expose Navigator, so this native Navigator capture
has no matching Chrome fixture and is not a pixel-parity pass. Complete main
editor visual acceptance, custom panel projections/drawers, recovery, physical
input/lifecycle coverage and sustained performance remain open on both platforms.

## Shared workspace customization and live dragging

Both Apple targets now share panel/group/toolbar/tile and Zen context menus,
tool selection/search, toolbar creation/rename/duplicate/management/delete
dialogs, standalone color/opacity editing and expanded panel configuration.
The Rust models supply labels, eligibility, validation, selection and actions.
Context queries happen on activation; ordinary paint/camera publication does
not query menus or expansion geometry. A single cancellable task resolves the
shared expansion animation from native content measurements.

Panel/group movement and floating/divider resizing send shared down/move/up/
cancel actions. The gesture belongs to the persistent workspace root so tearing
a tab into a floating group preserves input ownership. Native source views only
register rectangles. Tile dragging allows one pending drop query, coalesces
position changes, rejects stale replies and applies the final Rust action.
Expanded toolbar tile geometry comes from the same layout used for drop hints.
Divider tiles are exposed on both Apple platforms.

All 256 affected regression tests pass (217 UI, 12 host, 27 Apple), after
integrating the incoming GTK in-surface Navigator and shared renderer updates.
The three new Apple checks exercise both platform policies: toolbar naming,
duplication, deletion cancellation and workspace undo; live drag cancellation,
resize and history; and exact expanded tile/drop geometry. Actual Metal artwork
pixels remain unchanged through the workspace edits. Both final signed builds
pass, the Mac UI test target compiles, and the physical iPad installs and launches.

The focused iPad workflows pass toolbar creation/rename/duplicate/delete and
control visibility/panel tear-off. Toolbar testing also verifies canvas pinch
navigation through empty workspace regions. Accessibility grouping and control
lookup errors found during these checks are corrected. The drag assertion
accepts the shared policy that hides a lone floating built-in panel's tab and
retains its footer grip. No system-menu coordinate testing was used. The
separate XCTest authentication dialog remains pending on Mac; a direct screen
capture verifies recording permission but is obstructed and rejected for parity.
Mac interaction evidence for this milestone remains incomplete.

The new Chrome `panel-configuration` fixture runs on hardware WebGPU at the
iPad's 1376 by 1032 logical viewport and 2x scale. Its full-image comparison
exposed SwiftUI clipping the expanded group to one child's width; the container
now fills the complete Rust bounds and its right-side controls are visible.
The configuration/tear-off check passes again after that correction. Exact
different pixels fall from 21.1210% to 18.0784%; the final maximum channel error
is 232. The portrait native raster is rotated 90 degrees counterclockwise
without resampling, and every pixel remains in the comparison. Configuration
control heights/styles, header, layer controls and other differences remain:
this is a failing visual gate, not a parity pass. Raw images, reports, device
details and test bundles remain local and ignored.

Collapsed columns/drawers, remaining panel projections and complete context/
dialog/drag workflows still require acceptance on both platforms. Recovery,
physical Pencil/tablet and lifecycle coverage, the complete visual fixture
matrix and sustained hardware performance remain open.

## Collapsed columns, content drawers and partial Zen

Apple now projects the same collapsed columns, tabbed column drawers, child
tool drawers and partial-Zen edge toolbar sections as the shared core/Android
path. Both platforms expose collapse/expand, content-panel toolbar choices and
the Commands panel. Ordinary dock topology and source panel ownership are
preserved. Column scrolling reports a native offset; clipped toolbar tile
rectangles update child anchors. Drawer tabs reuse the existing shared controls,
including filters, layers, tool settings, color, Navigator and Diagnostics.
The host query returns shared natural toolbar height and connection geometry.

Each drawer coalesces layout, content measurement and anchor changes behind
one pending geometry query; stale replies cannot publish earlier placement.
Camera/painting snapshots do not start new geometry queries unless those inputs
changed. Native hit testing respects drawer stacking and clipping, including
blank drawer regions covering dock grips. The actual animated bounds feed the
shared chrome policy. Context popovers and native sheets supply the popup fact.
Chrome visibility refreshes on Zen mode changes as well as geometry changes;
the focused UI check exposed and verified the missing mode refresh on exit.

Canvas admission stays on the serial Rust owner. A real down uses logical
workspace coordinates and shared dismissal before the physical pointer batch.
If consumed, the whole contact, including subsequent movement and prediction,
is suppressed until its terminal event. Focus loss clears that admission state.
Invalid and stale-document samples remain rejected before they affect chrome.
Both platform policies pass actual Metal checks for no paint through dismissal,
subsequent normal painting and exact Undo, including a 2x coordinate case.

All 258 affected tests pass (217 UI, 12 host, 29 Apple). The second new Apple
test verifies column/tab projection, child anchor movement/clipping, transient
measurement state, collapse undo, panel choices and Zen topology preservation.
Its Zen fixture restores an outward-facing lone toolbar: the shared policy
intentionally excludes a toolbar nested in a content tab group. Both signed
builds and WebAssembly compile; the Mac test target compiles. The physical iPad
build installs and launches, and a separate disposable Mac editor launches.

The iPad column/tab/child drawer/dismissal/expand workflow passes, and the Zen
entry/exit check passes after the visibility fix. An earlier Xcode invocation
reported zero executed tests despite the new method being present in its built
binary; removing only the disposable test runner allowed the focused checks to
execute. That zero-test result is not counted as passing evidence. Mac GUI
checks remain limited by the pending XCTest authentication prompt and were not
repeated. No system-menu coordinate tests were used.

The new 1376 by 1032, 2x Chrome/native Zen comparison retains every pixel and
rotates only the native portrait raster. It differs at 1.3477% of pixels, with
maximum channel error 166; all differing pixels lie in the top 119 physical
rows. Chrome currently omits the partial-Zen toolbar sections, so this is an
explicit host feature difference and a failing full-image result. The native
sections remain present. Complete drawer/style/gesture fixtures, source-corner
connections, full main-editor visual acceptance, recovery, physical input and
lifecycle coverage and sustained performance remain required on both platforms.

The incoming shared GPU Fill/Auto Select edge refinements are integrated. All
258 UI/host/Apple tests pass again, along with six focused GPU checks covering
flood masks, independent pixel morphology, antialiasing through history replay,
invalid requests and startup compilation. Two hardware latency benchmarks remain
explicitly ignored; these correctness results establish no performance claim.
The integrated signed iPad and Mac builds and WebAssembly build pass. The iPad
build installs and launches, and the Mac build launches with a disposable workspace. The
drawer UI and blank-canvas visual fixtures above predate the renderer merge and
do not establish visual parity for the incoming Fill/Auto Select refinements.

## Estimated Pencil observations and shared stroke correction

UIKit now handles delayed Pencil property updates through the common Apple ABI
and shared Rust input engine. Numeric observations retain their contact, token,
timestamp, scale and original resolved transform/pressure policy. Partial/final
updates include force, location, tilt and roll; prediction remains separate.
Corrections bypass chrome/pointer/cursor routing and cannot start a contact.
The adapter keeps pending observations after pen-up, releases them on blur and
clears canceled contacts. Repeated terminal observations and stationary
airbrush samples derived from an estimate follow the original token.

Pending observations use the replaceable tail where possible. The first exact
Metal oracle exposed watercolor material boundaries changing when persistent
samples waited for corrections. Recording those boundaries from real input and
retaining their indices through finalization fixes the discrepancy. Corrected
G Pen, Pencil, watercolor and smudge strokes now match the final-value input
oracle exactly in both stored semantics and GPU pixels on both Apple policies.
Undo removes the original stroke; Redo restores its corrected pixels.

Committed corrections amend the original history entry without adding an undo
step or discarding redoable artwork. Captured project snapshots remain immutable,
and affected save-checkpoint identities change while states before the stroke
retain theirs. Direct tests also cover camera-history eviction, capture-time
pressure policy, correction after Undo, repeated final callbacks, duplicate
terminal samples, stationary airbrush input and cancellation/contact isolation.
All 336 affected Rust tests pass (42 core, 35 engine, 217 UI, 12 host, 30 Apple).
The standalone Swift observation checks and all five trace analyzer checks pass.
Both final signed Apple builds pass, the physical iPad installs and launches,
and the Mac launches a disposable editor.

This establishes synthetic input correctness, not physical Pencil/tablet or
performance acceptance. Retention has explicit bounds and expiry counts; callback
loss/overflow, orientation changes and the complete physical interruption matrix
remain open. Corrections racing later explicit point edits retain a matching
guard and require broader workflow acceptance. Late corrections to persistent
ink currently replay the scene: long strokes, 4K multilayer costs and sustained
120 Hz performance remain unverified. The trace reports correction queueing and
receipt/presentation proxies separately. See [Apple input details](../../apps/layer-apple/INPUT.md)
for the shared contract, exact bounds, limitations and reproducible checks.

The parallel port's independent filter-reference reconciliation is integrated,
and the WebAssembly build passes with the shared correction changes. Its sRGB
import oracle passes on Metal. The strict `runtime_filter_pixel_reference`
comparison against the new Vulkan-generated v3 fixture fails on this Mac with
maximum channel error 255. That test constructs renderer packets directly and
does not exercise the input-correction path. Full input/output/difference
artifacts remain local; no channel masks, fixture replacement or tolerance
relaxation was applied. Cross-backend filter parity remains an explicit failing
gate and requires further investigation beyond this input milestone.

## Filter color isolation across backends

The strict reference discrepancy is reproduced on both Metal and a local Vulkan
SwiftShader numerical backend. Metal's full sheet also exactly matches the
pre-migration implementation with identical corrected imports. New independent
scalar tests cover the complete opaque Curves/Exposure channel ramp and
Halftone's full ink/paper endpoints; both tests pass on both backends. They
isolate color/storage behavior without generating expected images from renderer
output. Partial alpha, spatial filtering and the full 160-case reference remain
open. See [the filter investigation](../reference/runtime-filters.md) for numerical evidence.

The software backend is admitted only by an explicit opt-in in renderer unit-test
binaries. Production iPad and Mac hosts retain their hardware requirement.
Software results establish no performance claim. Output-rounding experiments
were reverted: neither solved the strict full-sheet comparison. The checked-in
reference, one-byte tolerance and all compared channels remain unchanged.
Both signed Apple builds and the production renderer library check pass with
the test-only diagnostics present. This milestone changes no app rendering code.

## Physical Pencil smoke check and resumed Mac workspace checks

The user confirmed pressure response, persistent ink after pen-up, drawing with
a resting palm and expected Undo/Redo on the physical iPad. Its four-minute Debug
capture contains about nine seconds of pointer activity, five Pencil contacts,
730 real pointer batches and 417 prediction batches. There are no frame errors,
recorder drops, missing presentation callbacks or invalid/skipped GPU samples.
Eight drawable callbacks report zero presentation time. No correction batches
were observed; physical estimated-property coverage remains open.

After readiness, owner CPU service is p50 3.70 ms, p95 8.01 ms, p99 11.79 ms and
maximum 39.43 ms, with 17 of 672 submitted frames above 8.33 ms. Across the whole
capture, continuous active presentation intervals are p50/p95 8.33 ms, p99
16.67 ms and maximum 25.00 ms; 25 of 999 intervals exceed the analyzer's 120 Hz
cadence allowance. These Debug measurements include profiler overhead and do
not establish sustained performance. All receipt latency proxies, startup memory
growth and missing/zero observations remain in the local full report. The
required Release workload matrix, ten-minute sessions and physical latency
measurements remain open on both platforms.

After the user enabled XCTest, two focused Mac workflows execute and pass:
collapsed column/tab/child drawer dismissal and expansion, plus control
visibility and live panel tear-off. Initial failures came from the test harness:
application-level coordinates have no finite Mac window extent, and XCTest
cannot always derive a hit point for the visible SwiftUI scroll controls. The
shared check now uses the actual editor window and a bounded, measured in-app
click fallback, retaining assertions on the resulting editor state. No system
menu coordinates or menu mechanics are tested. These interaction checks do not
close the outstanding full-image visual or full-workflow acceptance gates.
The separate Mac partial-Zen entry/exit check also passes with window-based
containment; three focused Mac checks execute in total. The iPad test target
compiles with the shared harness changes without interrupting the physical app.

Direct Mac screen capture now succeeds without the permission dialog. A settled
Brush size configuration fixture is compared with local hardware-WebGPU Chrome
at 1200 by 870 logical pixels and 2x scale. The full sRGB image differs at
25.3555% of pixels, maximum channel error 232. No masks, rescaling or cropping
are applied by the comparator. The Mac OS window controls/menu arrangement is
an intentional adaptation retained in the report; configuration control styles
and heights, panel placement, toolbar color shape and other differences still
fail visual acceptance. The source captures and complete report remain ignored.

## Configuration controls and panel surface alignment

Both Apple hosts now use the compact configuration controls already exposed by
web and Android: wrapping size buttons, a brush-color swatch that opens the
existing shared color editor, and the shared checkmark asset in 16-point checkboxes.
The live size grid follows the same two/three/four-column breakpoints, cell
padding and label line height, including wide drawers. Native content measurement
continues to feed Rust's expansion placement. Panel shadow opacity/offset/blur
and the dynamic toolbar color glyph now follow the reference styling and icon
geometry. Brush values, preset choices, color state/actions and document behavior
remain owned by the shared core.

The configuration fixture's full-image exact differences improve from 25.3555%
to 7.3088% on Mac and from 18.0784% to 3.9751% on iPad Simulator. Final maximum
channel errors are 209 and 204 respectively. Comparisons retain the same light
theme, default document, camera, complete pixels and sRGB handling; viewports are
1200 by 870 and 1376 by 1032 at 2x scale. The iPad portrait raster is rotated only,
without resampling. The Chrome fixtures remain unchanged; no thresholds or masks
were introduced. These are improvements to failing visual gates, not acceptance.
Header/menu differences, layer controls, typography, remaining icon/shadow
rasterization and other editor states still need full parity work.

The focused shared workflow additionally selects a compact preset and checks
both the live panel and configuration values, opens the color editor from its
swatch, changes the paint slot and returns to configuration. It retains the
control-visibility and live tear-off assertions. The surface comparison uses
direct captures, without UI event automation or system-menu testing.
The final workflow executes and passes once on each platform; both signed
Apple builds pass. Broader editor states, physical lifecycle and sustained
performance remain required beyond these targeted checks.

The incoming Android editor-preset/live-Navigator changes are integrated. Their
shared layout edits are conditional on Android and preserve Apple/Web fixture
geometry. All 259 UI/host/Apple bridge regression checks pass (217/12/30), along
with both integrated signed Apple builds and WebAssembly. The complete visual
reports above remain separate from functional and performance acceptance.
The integrated physical iPad build installs and launches successfully.

The subsequent shared explicit filter-storage correction and independent v4
reference are also integrated. On Metal, 111 renderer checks pass and the strict
filter reference fails; 17 hardware benchmarks remain separately ignored. Both
independent color checks pass, including six alpha levels. The reference now
differs above one byte in 3,274 sampled pixels across 70 of 160 cases, with a
maximum error of 47. Full backend parity remains open; see the detailed
[filter investigation](../reference/runtime-filters.md). Both signed Apple builds and the
WebAssembly build pass after this renderer change, and the integrated physical
iPad app installs and launches. The configuration screenshots above contain
blank artwork and establish no filter-output acceptance or performance claim.

## Live GPU Navigator on both Apple hosts

Navigator now uses the shared in-surface overview presenter already adopted by
Android. Its image and work-area outline use the current GPU composition and
camera in the existing Metal presentation pass. The Apple bitmap ABI, 15 Hz
polling, worker decode and image-observable cache are removed. Native views send
logical layout records only; the render owner resolves current document extent,
display scale, clipping and stacking order. Layout updates are validated atomically
with a 32-record/16 KiB transport limit, and identical records do not dirty an idle
canvas. Optional GPU resources stay behind the initial paper presentation.

The shared SwiftUI workspace reveals the Metal image after each relevant panel's
background/clip/shadow, preserving higher panels and native controls. Direct
captures verify docked and column-drawer previews on both platforms, plus Mac
floating panels above and below Navigator. Those are compositing checks, not full
visual acceptance. The current web host does not expose Navigator, so Chrome
cannot supply this panel's complete reference. The unchanged configuration
fixture still compares every pixel against Chrome: Mac differs at 7.3088% with
maximum channel error 209; iPad differs at 3.9751% with maximum 204. Viewports,
scale and sRGB handling remain as above, with only the required iPad raster
rotation. Neither comparison passes; no masks or tolerances were introduced.

All 30 Apple bridge checks pass. Navigator coverage includes live camera/document
geometry, replacement with another aspect ratio, display-scale changes, valid
clipping metadata, atomic rejection, ordering, idle behavior and unchanged
document pixels/history during navigation on both platform policies. Three shared
Metal overview checks pass, covering actual image pixels, transparency, clipped
sampling, live paint, camera changes and resource reuse.

The focused native workflow executes once and passes on each platform. Mac
captures actual Navigator pixels before painting, after pen-up, Undo and Redo;
Undo restores the initial pixels exactly and Redo restores the painted pixels
exactly. iPad Simulator checks that a single finger leaves ink unchanged, then
both hosts exercise zoom, rotation, reflection, overview dragging, Diagnostics
and switching back. Simulator finger events are not Pencil evidence. The test
uses native accessibility semantics for each platform; a stale Simulator runner
was replaced before the current assertions were counted. No OS menu coordinates
or menu mechanics are tested. Both signed Apple builds pass, and the physical
iPad build installs and launches successfully.

A release-mode Metal presentation benchmark uses 2048×1536 artwork, 40 warmup
and 120 measured iterations per case. With one 256-pixel-wide overview, CPU
submission median/p95/p99 are 0.015/0.021/0.031 ms and GPU queue spans are
0.067/0.076/0.153 ms. With a moving camera/outline they are
0.020/0.026/0.035 ms and 0.081/0.093/0.131 ms. The baseline and repeated baseline
GPU medians are 0.064 and 0.066 ms; two overviews reach GPU p99 0.338 ms. These
are short Mac renderer measurements, excluding native UI compositing, physical
presentation and input latency. They establish neither iPad performance nor
the sustained workload/ten-minute gates, which remain open on both platforms.

The milestone integrates the incoming Windows port, Android panel-drag repair,
shared watercolor halo correction and built-in toolbar recovery. After the final
shared UI change, all 265 Apple bridge/host/UI checks pass (30/15/220), with one
host hardware check separately ignored. Both signed Apple builds and WebAssembly
pass; the final physical iPad build installs and launches. The integrated Metal
suite, run before the subsequent UI-only toolbar change, passes 113 checks and
retains the known strict filter-reference failure, with 17 benchmarks ignored.
Its differences remain 3,274 pixels across 70 of 160 cases, maximum error 47.
This milestone does not close that filter gate or the broader visual and
performance requirements. Local captures, device logs and signing identifiers
remain outside tracked files.

## Private artwork recovery on both Apple hosts

Both apps now capture unsaved artwork through the shared project format and
offer completed copies after restart, with a shared Recovered Drawings action
and native picker. One immutable capture/write runs at a time; edits retain only
the newest desired revision. Archives are synced before atomic generation
manifests publish them. Failed writes retain the previous completed copy, corrupt
records remain available for repair, and a stale discard cannot remove a newer
generation. Runtime owner identities prevent a restored scene's initial blank
canvas from overwriting the previous process's artwork.

Recovery uses the existing document replacement flow. Save/Discard/Cancel
protects the current drawing, including a manual save before opening the selected
archive. Recovered content has no provider destination and stays unsaved through
Undo to its initial state; only a completed manual save restores ordinary clean
checkpoint behavior. Lifecycle flushing drains queued pen-up without requiring
a drawable, then waits for settings/workspace and artwork storage. Cleanup follows
authorized window/scene closure or final accepted Mac termination. Details and
commands are in [Apple persistence](../../apps/layer-apple/PERSISTENCE.md).

The shared regression run passes 31 Apple bridge, 15 host and 220 UI checks,
with one host hardware check ignored. After adding the lifecycle input drain,
the focused Rust recovery check passes on both Apple policies with real Metal:
queued pen-up commits without presentation, recovered GPU pixels match exactly,
and automatic capture cannot acknowledge a manual save. Direct Swift checks use
the actual coordinator and owner for both policies. They pass private permissions,
cancelled capture, edits during an in-flight write, latest-revision flushing,
stale discard, malformed-record isolation, failed-write retry, owner replacement,
source migration and unsaved replacement/close decisions. The final replacement
check covers both Mac saves and iPad staged exports and verifies that saving the
current drawing preserves the selected recovery archive without another picker.
The existing direct manual file and settings/workspace suites also pass.

The focused restart UI workflow passes once on each of Mac and iPad Simulator
after the recovery picker was made to wait for document readiness. It waits for
a completed copy, terminates/relaunches the app, opens that copy through the actual
in-app control, verifies the restored layer count and waits for a new durable
copy. Captures show the picker and restored editor. An earlier Mac runner was
blocked before test execution by renewed automation permission; subsequent
executed failures exposed the readiness issue and are not counted as passes.
No system menu coordinates or OS menu mechanics were tested.

Both final signed builds and the WebAssembly build pass. The final physical iPad
build installs and launches successfully. These checks establish completed-copy
recovery, not physical background-task expiration, interrupted publication,
multi-window/provider delivery or sustained storage cost. The full physical
lifecycle and performance matrix remains open on both platforms. This milestone
adds no Chrome parity or strict filter-reference pass; the previously reported
visual/filter differences remain unresolved.

Before publication, the milestone integrates concurrent shared transform upload
buffer and transient GPU page reuse changes. All 13 affected Metal transform
checks pass, with three benchmarks ignored. The recovery pixel/lifecycle check
passes again with that renderer on both Apple policies, both signed Apple
targets and WebAssembly build, and the integrated physical iPad app installs and
launches. The earlier full regression and UI restart results precede this
renderer integration; no additional OS UI automation was needed for the changed
GPU paths.

## Independent editor windows on both Apple hosts

The shared capability policy now exposes New Window on iPad, whose native
WindowGroup and application manifest already support multiple scenes. The command
appears in File, shortcut configuration and toolbar customization. Both Apple
hosts use the shared window request and native scene-opening action. An environment
that cannot open multiple windows reports an error instead of silently accepting
the request. Each scene exposes its independent restoration identity to native
accessibility, allowing checks to address a specific editor without system-menu
coordinates.

The direct Apple owner check passes for both policies: New Window is available
in the live menu, toolbar insertion accepts it, the host request belongs to the
originating session, and layer edits/Undo do not mutate another owner. All 220 UI
and 15 host regression checks pass, with one host hardware check ignored. Both
signed Apple targets and WebAssembly build pass.

After integrating the concurrent Windows document milestone, all 235 shared
checks and the focused two-owner Apple check pass again. The WebAssembly and
signed device builds pass. Tool Set command buttons now use the live shared
enabled state and tooltip, including disabling Scale / rotate on an empty or
locked target. The shared test application helper registers teardown so failed
workflows do not leave unnecessary editor instances open.

The final iPad Simulator workflow passes. It installs ordinary New Window/Close
toolbar actions, creates four layers in the first scene, opens a distinct scene
with two layers, adds and undoes a layer there, closes only that scene, then
reactivates the app and verifies the first scene's document and independent Undo
history. It leaves the disposable document clean before termination. Earlier
executed failures include an invalid toolbar test fixture and a failed return-to-
window Undo expectation. A temporary owner trace then passed; the final passing
run removes that trace and explicitly brings the surviving app forward before
its next edit. No repeated tap or weaker document assertion was substituted.

The final Mac workflow also passes, including independent Undo after the second
window closes. Earlier Mac runners failed before execution while enabling UI
automation, even after individual prompt approvals. After the user configured
Apple's persistent automation authorization, the test runs successfully and
leaves no test application open. No system menu coordinates were used.

The physical iPad runner still times out while enabling device automation, before
executing the workflow; the device is confirmed unlocked. This is neither a
physical-device pass nor an executed editor failure. Physical window verification,
full scene restoration, simultaneous physical input, cross-display/split-window
layout and memory/performance acceptance remain open. The Mac and Simulator
checks do not close those gates or visual parity.

The milestone also integrates the later web workspace/document and Windows PNG
export changes. With those changes, the 235 shared regression checks, independent
Apple owner check and real-Metal PNG export pixel check pass; both signed Apple
targets and WebAssembly build. The native window UI results precede this last
integration, whose Apple changes are shared export plumbing and added model
metadata. The integrated physical iPad app installs and launches successfully.
Web now uses the full editor preset and grouped paint tools. Matching
those defaults on Apple and refreshing the Chrome comparison fixtures remain
part of the next UI parity work; the older comparison results are not a pass
against this new web baseline.

## Shared full editor workspace and grouped tools on Apple

Fresh iPad and Mac owners now initialize the shared full editor preset used by
the web host. The default workspace includes the Tools and Commands bars,
Tool Set, Tool Settings, Brush size, Color, Navigator/Diagnostics,
Properties/Filters and Layers. Saved workspaces still restore their exact layout
and toolbar contents. The new preset does not overwrite an older customization.

Both native targets project the Rust tool groups and subtools, replacing the
Apple-only flat painting catalog. Group buttons wrap with flexible equal widths
and brush previews retain their aspect ratio. A direct ABI check visits every
catalog brush through the live toolbar, group and subtool actions on both Apple
policies, checks the selected preset, unchanged paint color and layer state,
then verifies exact restoration of an older customized workspace. The native
numeric workflow also verifies that changing groups remembers the selected
brush and its edited size.

Testing the full preset exposed an iPad numeric-entry stall: accepting an
expression synchronously resigned the text field during a SwiftUI update.
A process sample located the repeated responder-graph/AttributeGraph cycle in
that update path. UIKit focus release now runs after the update with a guard
against newer focus changes. AppKit uses the same guarded deferred pattern;
the stall was reproduced on iPad Simulator, not on Mac. The final expression,
invalid-input, stepping and brush-group workflow passes on both Mac and iPad
Simulator. Earlier iPad Return stalls are executed failures that led to this
fix. No system menu coordinates were used.

The regression run passes 32 Apple bridge, 15 host and 220 UI checks, with one
host hardware check ignored. The later assertion for legacy workspace restoration
passes separately. Direct Swift persistence checks pass, including actual native
owner restoration on both policies. The signed Mac and physical iPad builds,
iPad Simulator test build and WebAssembly build pass. The updated physical iPad
app installs and launches. Concurrent documentation reorganization was integrated;
its only executable-file changes update documentation links. These checks do not
establish physical input, lifecycle or performance acceptance.

The refreshed light-theme `initial` comparisons use a fresh local Chrome profile,
a hardware WebGPU adapter and matching logical dimensions at 2× scale. The full
images retain native window controls, the Mac system-menu adaptation, all control
differences and system corner pixels. EXIF orientation is honored; neither image
is resized, cropped or masked. Results with zero channel tolerance are:

| Native target | Logical viewport | Different pixels / total | Different fraction | Maximum channel error | Exact result |
| --- | --- | --- | --- | --- | --- |
| iPad Simulator | 1376 × 1032 | 462,046 / 5,680,128 | 8.1344% | 219 | Fail |
| macOS | 1200 × 870 | 523,196 / 4,176,000 | 12.5286% | 255 | Fail |

The blank central paper rectangle matches all four Chrome edges exactly on both
platforms after orientation normalization. This checks that rectangle only;
it does not establish one-point geometry agreement for every editor control.
Visible remaining differences include tool/text spacing, color controls and tab
icon, Properties dropdown layout and Navigator sizing. The web overview's fixed
height clips its controls in the short default panel, while the Apple overview
fits the available height and exposes navigation actions. Full-window canvas
compositing during camera changes and the other visual scenarios remain open.

The final focused Mac capture passes with both history commands disabled and
panel configuration closed. Preliminary Mac captures with a lingering tooltip,
a canvas contact or an open configuration panel are excluded from the table.
The fixture dismisses help by opening and closing the existing panel configuration
controls, then moves the pointer within the app before capturing. No system menu
is addressed. Test teardown leaves only the user's original editor process.
The numeric interaction results precede these capture-only fixture refinements;
the final shared test source also compiles for iPad Simulator. Other UI fixtures
were adjusted for the shared preset's groups and Commands bar but their full
workflows were not rerun in this milestone.

Screenshots, process samples, result bundles, build logs and signing/device data
remain in ignored local artifacts. The physical iPad automation runner's earlier
setup timeout remains unresolved; installation/launch is not a physical UI-test
pass. The previous strict filter-reference differences (3,274 pixels above one
byte across 70 of 160 cases, maximum error 47), complete feature inventory,
physical lifecycle/input matrix and sustained hardware performance gates remain
open on both targets. A concurrent Android-only native time/battery header change
was also pulled before publication; it does not change these tested Apple paths.

## Editor control geometry and live header compositing

The Apple tool rows now use the browser panel's padding, text-line sizing and
spacing; numeric readouts use tabular digits. Properties uses a shared Apple
button/popover control instead of the Mac menu style that discarded the custom
label's appearance. Choice rows reserve the longest option's intrinsic width,
capped at 60% of the row, and keep the label alongside it. The Layers blend
control now exposes its current value to accessibility. Both targets retain a
background plate behind Settings when paper extends under the header.

The browser's fixed Navigator height clipped its controls in the default short
panel. Its overview now fits the available height above six 32-point controls,
matching the Apple layout. The surrounding background has its own cutout around
the live GPU image, with no bitmap readback or separate preview renderer. Flip
buttons also project the shared selected state. A later opaque-header CSS rule
was removed so the canvas remains visible between the header's individual
control backgrounds.

The focused `testEditorControlLayout` workflow passes on Mac and iPad Simulator.
It checks all Navigator targets, uses four actual zoom-in actions, selects
Multiply in Properties, observes the same value in Layers and restores Normal
through the in-app Undo control. It captures the initial editor and paper zoomed
behind the header before changing document history. The Mac fixture moves the
pointer back onto a panel tab to avoid a brush-hover mark in the capture. It does
not address the system menu bar. The first Mac run reached Multiply but failed
because the Layers button did not expose its value; the final projection and
check use that accessibility value. No failed run is counted as a pass.

The browser editor workflow passes with actual Navigator pointer hits and a
rendered-pixel assertion that paper is visible through empty header space. An
older fixture reselected already-active tabs and opened configuration over the
controls; it now activates a tab only when needed. The remaining editor workflow
also passes: color/tool actions, Navigator drag, partial Zen, collapsed columns,
nested drawers, project Save/Open/New, PNG export, cancellation and persistence.
The browser runner keeps Linux's offscreen Vulkan options while allowing native
GPU backends on other hosts.

Both native captures match local Chrome at 2× scale. Full-image comparisons with
zero channel tolerance retain every pixel and intentionally still fail:

| Native target / scenario | Logical viewport | Different pixels / total | Different fraction | Maximum channel error |
| --- | --- | --- | --- | --- |
| iPad Simulator / initial | 1376 × 1032 | 295,639 / 5,680,128 | 5.2048% | 219 |
| iPad Simulator / canvas under header | 1376 × 1032 | 393,062 / 5,680,128 | 6.9199% | 219 |
| macOS / initial | 1200 × 870 | 389,668 / 4,176,000 | 9.3311% | 255 |
| macOS / canvas under header | 1200 × 870 | 522,018 / 4,176,000 | 12.5004% | 255 |

The four edges of both the main paper rectangle and Navigator paper rectangle
match Chrome exactly in each initial capture. Three fixed points four logical
pixels below the top edge change from the shared gray surround to white paper
after zooming on each native target. These are scoped geometry/compositing
checks, not proof that every control meets the one-point alignment requirement.
Remaining differences include text rasterization, tabs/grips, color controls,
control details and the intentional Mac window/menu adaptation. Other themes,
documents, UI states and physical-device visual coverage remain open.

Before publication, the milestone integrates concurrent Windows Navigator,
watercolor selection-boundary, native shader-worker shutdown and web/GTK
fullscreen/system-status changes. All 32 Apple bridge, 15 host and 221 UI tests
pass, with one host hardware check ignored. The focused selected-watercolor
transport and shader-shutdown tests pass on the local native backend. Signed Mac
and iPad builds and WebAssembly build pass; the integrated iPad app installs and
launches. The browser editor workflow passes again after integration.

The native UI workflows/captures precede that shared integration; no Apple view
or tested property-action implementation changed in it. All four Chrome
references were recaptured afterward. Three match their earlier references
exactly; the Mac initial reference differs in 653 pixels with a maximum channel
error of 5. The table uses that later reference and applies no masking or extra
tolerance. This checkpoint does not establish Apple coverage for the new
fullscreen/system-status surfaces or close the complete feature inventory.

## Apple command coverage and native full-screen requests

The command inventory now uses the same initial full editor workspace and
document-replacement policy as `capy_apple_create`, at a declared 1200×900 logical
viewport and scale 2. Both Apple platforms have initial snapshots, all five
settings pages, every command and panel's availability, and the existing dynamic
menu/layer/shortcut scenarios. Schema 2 places the platform-specific fixtures
under `platforms.ios` and `platforms.mac`. The old iPad-only initial/settings
fixture used the generic `NativeHost` workspace and did not represent a fresh
Apple editor.

`apps/layer-apple/command-coverage.json` classifies all 62 current commands in 14
groups, with references and separate iPad/Mac remaining-work notes. The audit
requires every catalog entry exactly once in both platform inventories and in
the review, verifies referenced files exist, and detects availability changes.
It passes with 62 commands, 11 panels and five settings pages per platform. An
injected new command, omitted iPad entry, duplicate classification and changed
Mac capability are each rejected. These are inventory checks; partial/open
group notes are not promoted to successful workflow or complete UI acceptance.

The newly shared Full Screen command now routes to the native Mac window.
`WindowPresentation` serializes requests per editor; `DocumentWindowDelegate`
adapts AppKit transitions while forwarding SwiftUI's original delegate callbacks.
The requested mode does not optimistically change selection or the enter/exit
icon. Actual notifications, including changes through native controls, update
the shared state. Repeated enter requests cannot toggle the window back out;
requests during a native transition wait for its result. Failed transitions and
detachment complete the shared request with a visible error. Consecutive native
observations are retained even when the serial owner has not published the first
snapshot yet. Other Mac/iPad editor sessions retain their independent state.

Mac View uses its existing native Full Screen item; the shared command remains
available to toolbar customization and shortcut editing. Control-Command chords
pass through the canvas's key-equivalent handler to AppKit. The adapter follows
[AppKit's full-screen request](https://developer.apple.com/documentation/appkit/nswindow/togglefullscreen(_:))
and [completion notification](https://developer.apple.com/documentation/appkit/nswindowdelegate/windowdidenterfullscreen(_:)).
The installed UIKit SDK's iOS scene geometry preferences expose orientation
changes, with no corresponding native iPad full-screen request. Mac Catalyst's
separate geometry preferences do not apply to this UIKit target. The inventory
retains `fullscreen` as explicitly unavailable on iPad; this capability remains
open instead of being omitted from the parity review.

The direct Swift window check passes request/acknowledgement ordering, repeated
requests, external changes, rapid observations, failed enter/exit, detachment,
delegate forwarding and session isolation against actual serial Rust owners.
Its invisible AppKit test window simulates OS notifications; it verifies our
adapter and editor state, not system menu mechanics or full-screen rendering.
The existing shared full-screen test additionally exercises the Mac availability
and exit request, while retaining the iPad exclusion and browser F11 behavior.
Both signed Apple builds and all 32 Apple bridge, 15 host and 221 UI tests pass
(268 total; the existing host hardware-only test remains ignored). Incoming
README changes through `5c94d3b` are integrated.
The signed iPad build is installed and launches on the attached device. No new
physical input or GUI workflow assertion is inferred from that launch.

Local evidence is retained under `artifacts/apple-window-*`: command fixtures and
audit, direct Swift check, both build logs and shared regression output. These
private artifacts are excluded from Git. This checkpoint adds no new visual,
physical Pencil, latency or sustained-performance evidence. Full-screen editor
geometry/rendering, optional system-status parity, full feature workflow coverage,
existing pixel-difference failures and the hardware targets remain open.

The previous strict filter-reference failure, physical input/lifecycle matrix,
physical iPad automation setup and sustained hardware performance gates remain
open. Capture bundles, logs, device/signing data and pixel reports stay in ignored
local artifacts. Test editors and the temporary web server are closed; the
original user editor is preserved.

## Repeatable Apple drawing and ten-minute hardware baseline

Both targets now share five opt-in synthetic drawing workloads, with versioned
240 Hz trajectories, pressure, explicit pen-up gaps and prediction where
specified. Production input, the serial renderer, editor panels, history and
recovery stay active. Each run uses isolated persistence and a separate benchmark
bundle; artist settings and recovery copies are preserved. Wall-clock input
production is independent of frame admission, and excessive producer backlog
marks the run failed instead of reducing the input load.

The trace records setup, warm-up, measurement, postlude and failure markers.
Reports isolate the measured interval and retain rejected input, denied frame
admissions, missing GPU observations and presentation callbacks, interval-edge
gaps, memory and thermal state. A complete producer interval is explicitly
separate from frame-budget or pixel/latency acceptance. Native canvas readiness
now updates accessibility once per attached surface instead of on every frame.

One ten-minute Release `wet-watercolor-4k` session completed on each physical
platform with eight paint layers plus paper, a 320 px brush and synthetic
prediction. Both have zero recorded renderer errors, rejected input batches and
recorder overflow. The iPad observed 65,407 measured presentations at a median
8.333 ms interval; CPU owner service p99 was 9.120 ms, with 1,092 frames over
8.33 ms. Mac observed 49,266 presentations at a median 11.111 ms interval on its
current 90 Hz configuration; CPU p99 was 6.083 ms, with 28 frames over 8.33 ms.
Both retain late presentations and missing GPU readbacks in the results.
Measured footprint growth was +0.17 MiB on iPad and +148.83 MiB on Mac, including
history, ordinary recovery work and recorder storage. Thermal samples remained
nominal. Mac memory growth and timing tails require further investigation.

The full distributions, limitations, reproduction commands and rejected
two-drawable experiment are recorded in
[`apps/layer-apple/PERFORMANCE.md`](../../apps/layer-apple/PERFORMANCE.md).
The ten-minute sessions include shared changes through `d8a130b`. Subsequent
Windows effect controls, the shared Navigator-height adjustment and Ripple
phase correction through `af33bcd` are integrated before publication. These
later layout/filter changes are not represented by those earlier hardware
timings or by the older visual comparison table.

After integration, both normal signed Apple builds pass, as do all 32 Apple
bridge, 15 host and 223 UI tests (270 total; one existing hardware-only host test
remains ignored). The incoming spatial-filter linear GPU oracle also passes on
the local native backend; it supplements the still-open strict PNG-reference
gate. Direct Swift checks cover the workload's contact/pressure contract,
frame admission/surface replacement and bounded trace recording. All eight
Python report checks pass, including failed/truncated producers and missing
render observations. The integrated normal iPad app is installed and launches;
this does not add a new physical-input assertion. These checks require no system
menu automation.

Remaining acceptance includes the other four ten-minute profiles on both
platforms, calibrated profiler overhead, isolated GPU work, physical input-to-
pixel latency, complete input/lifecycle workflows, Mac 120 Hz presentation
evidence and the existing visual/filter-reference failures. Basic Pencil checks
remain the previously user-confirmed evidence. This checkpoint does not close
the complete performance or parity goal. The benchmark apps are closed and the
original Mac editor is preserved. Raw traces, captures, build logs and
signing/device information stay in ignored local artifacts.
