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
Keep the codebase simple: prefer established SwiftUI, UIKit and AppKit patterns
and one shared menu/popup helper across editor surfaces. Do not compensate for
an unsuitable component with gesture overrides, contact handoff machinery or
presentation hacks. Productivity ergonomics are required: fast menus, readable
opaque theme colors, minimal animation and no distracting glass shine. Retain
native control, keyboard and accessibility conventions.

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

The user's 2026-09-13 clarification makes perceptual parity the visual acceptance
criterion: exact pixel identity is unnecessary when differences are imperceptible
at normal viewing size. Fix visible differences and straightforward mismatches;
do not add complexity to eliminate insignificant rasterization differences.
Keep the full comparisons as diagnostic evidence. Simplicity and removal of
dead, deprecated or unnecessary paths are explicit goals alongside functionality.

For the current validation scope, demonstrate sustained **90 Hz drawing on Mac
and 120 Hz on iPad**, using representative simple and complex brushes, prediction
where supported, pen-up and 4K multilayer documents, including ten-minute sessions.
The user explicitly deferred Mac 120 Hz testing on 2026-09-11 until suitable
hardware is available. That future check does not block current Mac milestones
and must not be marked as passed. Evaluate the current frame budgets of 11.11 ms
on Mac and 8.33 ms on iPad; retain the original 8.33 ms diagnostics for future
Mac 120 Hz work. Measure CPU/GPU frame work and separately report actual
presentation cadence and input-to-present latency: p50/p95/p99, maxima, missed
deadlines, memory growth, idle behavior and thermal effects. Record the actual
display capability and refresh rate; a report evaluated at 90 Hz cannot establish
120 Hz presentation acceptance. The iPad 120 Hz requirement is unchanged.
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

The user's 2026-09-12 scope revision removes Save Layout and Load Layout from
both Apple apps and the shared menus. These actions are excluded from future
milestones. Workspace persistence, switching, layout history and panel sizing
remain in scope. Earlier saved-layout implementation and validation below record
the previous design and do not reinstate the removed UI.

The shared [drag convention](../ui/drag-and-reorder.md) also supersedes earlier
gesture checkpoints: tile bodies require a hold for every device; list-row
bodies require a hold for touch/pen while mouse remains immediate; grips and
title/tab bars remain immediate. The [source inventory](../ui/drag-inventory.md#apple-macos-and-ipados)
records required Apple tile gating and whole-row pickup work. Earlier successful
workspace/layer tests do not establish these new device-specific requirements.
Numeric sliders and other direct-manipulation controls retain their existing
interaction without a reorder hold.
The explicit Customize Title Bar surface follows the shared editor exception:
whole editable items and component-bank chips drag immediately after movement
slop with every device; bank placement is drag-only.

Editor menus remain vertical, anchored to their source, and permit a held
finger/Pencil contact to transition directly into dragging. Horizontal edit
bars are not acceptable. The 2026-09-13 productivity review supersedes the
blanket Liquid Glass request and the earlier native row-menu experiment.
Native styling does not take precedence over readability, fast interaction or
maintainability. Settings and the macOS system menu bar retain platform patterns.

## Settings choices and shortcut dialog fit — 2026-09-15

Apple Settings now shows the shared Zen icon previews and retains icons in
ordinary choices. Shortcut dialogs fit 320-point windows, reuse the existing
wrapping layout, and use native fitted sizing and bordered actions. Four focused
image-choice cases pass; narrow/wide light/dark captures cover both Apple policies.
Final grouped Mac and UIKit workflows pass actual image-choice Reset, reopen,
shortcut search and nested dialog cancellation. Both final Release builds pass
without warnings, and representative captures are reviewed. The [handoff](../development/apple-handoff.md)
records evidence and the retained inconclusive numeric Reset investigation.
Full feature/visual, physical input, provider/lifecycle and sustained Mac 90 Hz /
iPad 120 Hz acceptance remain open. The overall goal is **incomplete**.

## Property selection and menu navigation — 2026-09-15

New curve points and gradient stops select themselves from Rust's published
positions, fixing immediate removal/editing and single-point history restoration.
The shared menu handler adds Home/End navigation. Property colors reuse the
existing RGBA conversion; iPad scene closure makes its existing strong capture
explicit for the current compiler.

All eighteen native property component cases and the native menu keyboard check
pass, including three constrained window sizes. Both Release builds pass. Four
grouped editor workflows pass without failures or skips: Mac filter artwork/history
and blend choices, simulator filter artwork/history and submenu shortcuts.
Immediate insertion selection, later dragging/removal/reset and sampled artwork
Undo/Redo are covered. Representative Mac/simulator captures are reviewed and the
isolated apps/runners are stopped. No physical workflow accompanies this batch.
Evidence and limits are in the [handoff](../development/apple-handoff.md) and
ignored `artifacts/apple-property-selection-v1/`.

The compact-menu UIKit shortcut, remaining native feature/visual and hardware
acceptance, provider/lifecycle cases and sustained Mac 90 Hz/iPad 120 Hz gates
remain open. The overall goal remains **incomplete**.

## Zen, editor dialogs and row cleanup — 2026-09-15

UIKit reports chrome contacts before button activation through a passive native
observer, fixing the toolbar Zen transition while retaining native canvas touch
routing. Mac keeps its working tap observer. The initial shared spatial-gesture
candidate failed Mac mask menus and UIKit panning; it is removed. The batch also
removes the unused native row-menu adapter and always-false drag flags, preserves
focused Settings defaults after Reset, fits the recovery picker to narrow windows,
and searches enabled menu accelerators across the complete menu model.

All eight final grouped workflows pass with no failures or skips: three on Mac
and five on the iPad simulator. They cover Zen/Tab, Mac mouse stroke/history/mask
actions, UIKit finger navigation, drawer dragging, and UIKit immediate-grip and
held-menu layer dragging. Both final Release builds pass. Three native row suites,
four Settings text cases and the native menu keyboard fixture pass in their
documented component scopes. Representative drawer/navigation captures are
reviewed. The drawer fixture now explicitly selects individual-panel mode.

The compact-menu UIKit shortcut regression remains unqualified: its last run
fails, and it is not counted among the eight passing workflows. No physical-device
run accompanies this refinement batch. Complete feature/visual, physical input,
provider/lifecycle and sustained Mac 90 Hz / iPad 120 Hz acceptance remain open.
The [handoff](../development/apple-handoff.md) records final source/build evidence
and retained failures. The overall goal remains **incomplete**.

## Native editor workflows and current hardware drawing — 2026-09-14

This major milestone groups the native control, document and artwork workflows
following the editor-fit milestone. Shared Apple controls retain the selected
Web header surfaces and workspace pill, readable tool labels and mask-link hit
targets. Keyboard clearance follows UIKit's per-window guide, canvas shortcuts
resume after numeric editing and Settings, and a shared redraw request restores
artwork after background return. Mac retains a working File menu and opens a
new editor after its last window closes. Startup waits for both state and catalog
before constructing controls, fixing the blank opacity readout.

Native Mac/simulator workflows cover all thirty painting/erasing presets,
figures/gradients, fill/selection sources, rulers, navigation, mask/group transforms,
history and controls. Mac adds real artwork checks, full-screen transitions and
native Save/Save As/restart/Open with exact decoded PNG equality. The independent
Metal filter comparison meets the user's perceptual criterion. These retained
results have distinct scopes; simulator controls do not establish Pencil behavior.

Final integration catches and fixes Pencil painting on masks in the shared
engine by removing paper material together with its texture, preserving contact
geometry. Workspace comparisons retain exact JSON wire values. Sensor-correction
comparison permits one encoding level after full-image review finds only five
changed G-Pen bytes per Apple preset; raster structure, watercolor state and each
result's Undo/Redo remain exact. Temporary diagnostics are removed. The integrated
Apple/engine/host/UI suite passes 523 tests, with one host benchmark ignored.
All seven Metal contact/project checks and both Release builds pass after
integrating main through `1fd753a`; command and property-evidence audits pass.

Both physical hosts complete ten-minute eight-layer 4K ink workloads without
renderer errors, rejected input or missing/zero-time measured presentations.
Cadence still fails: 909 long intervals on Mac and 727 on iPad. All owned workload
processes are closed, the disposable iPad app is removed, its test runner is
restored, and both existing editor descriptors are unchanged. Measurements remain
distinct from the final mask-only repair and subsequent shader-expression grouping.

The [current handoff](../development/apple-handoff.md) records native evidence,
retained failures and remaining work. [Performance observations](../../apps/layer-apple/PERFORMANCE.md)
retain full timing and memory figures. Physical Pencil/keyboard/provider/lifecycle,
remaining native feature and visual cases, calibrated latency/overhead, memory
attribution and sustained 90/120 Hz acceptance remain open. The overall goal is
incomplete. All eleven recovery stashes and private evidence remain local.

## Editor fit, footer grips and gray title-bar backgrounds — 2026-09-13

Apple now fits docked Color wheels to available height, retaining the shared
128-point minimum and scroll behavior. One grip drawing matches Web/Android
header and vertical-footer rotation, opacity and inset. The user chose Web's
rounded gray title-bar backgrounds; the redundant text/icon halo renderer and
parameters are removed. Mac keeps top-level menus in the OS menu bar.

Integration through `59732aa` includes restored Paint defaults and Photo's
reviewed right-column arrangement; final Windows-only updates do not alter the
validated Apple/shared/Web source. All eleven recovery stashes remain. The
integrated shared regression passes 528 tests, with one existing host benchmark
ignored. Both signed version-5 builds pass. Four Mac and seven physical iPad
workflows pass with no failures or skips: both-theme Paint captures, Navigator
and blend Undo, Paint/Photo columns and relaunch; iPad additionally checks Color,
title-bar customization and tool drawers. Mac's preceding version-4 title-bar
checks also pass. Simulator Color, column, numeric and full-editor checks pass;
its later header drag moved the OS window and a following drawer containment
check failed. Those two windowed simulator cases remain unresolved.

Eight current native/Web fit/zoomed full-editor comparisons retain all pixels.
Normal-size review accepts the wheel fit, footer orientation and gray header
backgrounds. Exact image equality is not claimed. The capture helper uses current
managed Paint and visible Fit canvas, removing an obsolete Mac popup workaround.

The physical review app/runner are updated, but the post-test local CoreDevice
service timed out on status queries and direct review launch. Review-session
restoration and post-test descriptor verification remain pending. Evidence and
all failures are ignored under `artifacts/apple-editor-parity-v1/` and
`artifacts/apple-color-fit-v1/`. Complete feature, visual-state, physical-input,
provider/lifecycle and sustained Mac 90 Hz/iPad 120 Hz acceptance remain open.

## Retained renderer and committed recovery — 2026-09-13

This checkpoint follows `7d5d59b` and integrates shared main through `9c89f7a`,
including Android header transparency and the revised Paint columns. All eleven
recovery stashes remain intact.

Apple now uses shared renderer suspension/replacement instead of direct GPU
assignment. Failure callbacks belong to one device generation, and the serial
owner also polls health while native drawing is idle. Rendering errors retain
the CPU document, committed pixels, embedded assets, history and working values;
unfinished input is cancelled. Restart Canvas reconstructs that session and
Save As remains available. Native thumbnail/filter-preview generations reset.
Retirement runs on a worker; ordinary surface replacement retains a healthy GPU.

The retired input-only recovery barrier is replaced by preparation of a committed
snapshot without a drawable. Active ink can retain the previous committed raster.
The store reports lifecycle success only after preferences/workspace writes and
the existing project worker's atomic recovery publication. Recovery no longer
waits merely because a manual file request is pending. No second project codec,
compatibility renderer or recovery writer is introduced.

The integrated Apple/host/UI suite passes 442 tests, with one existing host
benchmark ignored. Hardware renderer iteration 6 covers both presets with
explicit suspension, actual device loss and uncaptured validation. It verifies
cancelled active input, exact restored source/raster pixels, saving while stopped,
pending requests, working settings/camera, stale callbacks and releases, document
Undo/Redo and new painting. Real-owner persistence iteration 4 passes both presets,
including pen-up followed by durable recovery without another drawable, cancelled
publication, newest-revision coalescing, failure/retry and owner restart.

Both signed iteration-3 builds pass. Native iteration 3 passes four workflows
per host with no failures or skips: idle renderer loss/validation and visible
restart with exact artwork samples and history; completed-copy relaunch;
column stacks; Metal/layers and Mac mouse drawing. Iteration 2's Mac stack
failure read width before asynchronous redo publication; its iPad pixel wait
ran after the editor left the foreground. The test now waits for exact width,
samples the full iPad screen and verifies the foreground. Earlier failures
remain recorded. A scoped retry after the final iPad app-query timeout restores
the review namespace and verifies the artist app descriptor unchanged.

Corrected iteration-4 captures reveal blank thumbnails after recovery. The
iteration-5 preview-readiness test fails on both hosts after 30 seconds: the
new staged GPU still waits for the host catalog, preventing idle thumbnails.
Apple now completes that startup gate for each GPU with the retained catalog;
the library is neither reloaded nor tracked by an additional state flag.

Both signed iteration-6 builds pass, and all four native workflows pass again
on each host with no failures or skips. The new thumbnail check passes. Reviewed
final recovery captures show the matching blue thumbnail and restored transparent
checkerboards on both hosts. These captures establish recovery presentation,
not the complete editor comparison against Web.
The final iPad app query timed out after successful tests. Its scoped retry
restored the review namespace and verified the artist app descriptor unchanged.

The first push was rejected because shared main advanced to `9c89f7a`. The
renderer milestone was rebased without overwriting that Paint-default update.
The integrated Apple/host/UI/workspace suite passes 528 tests, with one existing
host benchmark ignored. Both real-file column-stack persistence cases pass the
new default, customized membership/width/preferences, switching and history.
Iteration 8 passes the four existing native workflows on each host. Its new Paint
fixture used a coordinator field in the native command; `id` corrects that setup.
Iteration 9 passes the iPad Paint workflow. Read-only copies of the isolated
Mac database retain the Paint bindings, while relaunch chooses a separate Sketch
binding. Iteration 10 establishes that the visible scene identifier is unchanged;
that is a restoration failure, not a reason to relax the same-scene assertion.
Editor-owner construction now waits for the scene identifier, with its identity
boundary inside a stable container. The test also covers the legitimate new-scene
case by opening its saved workspace through the ordinary switcher.

Both signed iteration-12 builds pass. All five native workflows pass on each
host, with no failures or skips. The Mac restores Paint automatically in the
same scene; its event log contains no switcher action after relaunch. The iPad
run creates a new scene and reopens the saved workspace through the switcher.
Reviewed captures show the new Paint columns. The Color field is clipped by its
short scroll viewport at the default split; shared fit/scroll behavior remains
part of full-editor visual acceptance. Full app-inventory queries timed out after
the successful iPad run; scoped per-bundle queries restore the review session
and verify the artist app descriptor unchanged.

Evidence stays ignored under `artifacts/apple-renderer-recovery-v1/`. These checks
do not establish physical background expiration, provider interruption, every
window/surface transition, the complete feature/visual/physical-input inventory
or sustained Mac 90 Hz/iPad 120 Hz performance. The overall goal remains open.

## Shared column stacks — 2026-09-13

This checkpoint follows `9996f51` and integrates shared main through `e46f271`.
The eleven recovery stashes are preserved. Both Apple hosts use the ordinary
dock views to open a full-height stack member, with shared active-icon
connectors, selected tabs, split dividers and remembered widths. Native grips
expose shared preferences and remain immediate drag sources. Held icon bodies,
tabs, group grips and toolbar grips use shared stack-member and trailing-group
targets. Closed aggregates have no resize affordance. Fresh Paint opens its
right stack; custom saved layouts start closed. The retired Apple drawer fallback
and shared platform opt-in checks are removed.

After integration, 487 Apple/core/host/UI checks pass with one existing host
benchmark ignored. The native ABI validates ordinary group publication and
history on both presets. A hardware-GPU native contact test verifies auto-hide
consumes the entire contact without ink, permits the next stroke and restores
exact pixels with document Undo. Incoming diagnostics visibility and renderer
replacement coverage now includes both Apple presets.

AppKit iteration 4 passes eight checks across both presets and mouse/tablet
input: held icons, immediate tabs/grips, panel/group/toolbar member insertion,
trailing-group targets, fixed closed widths, member opening/switching, resize
history, focus cancellation and late-release rejection. Two real workspace-file
fixtures pass switching, relaunch, persisted membership/preferences/width and
Undo/Redo while retaining working brush settings and discarding transient open
state. These AppKit contacts do not establish physical Pencil acceptance.

Signed build iteration 3 passes on both hosts. Native iteration 3 passes three
tests on each host, with no failures or skips: both themes, stacking, switching,
native resize and Undo/Redo, preferences, auto-hide and compact drawers; plus
Metal launch/layers and Mac mouse drawing/document Undo/Redo. A timed-out iPad
post-test app query succeeds on scoped retry; the review namespace is restored
and the artist app descriptor is unchanged. All four stack captures were reviewed
at normal size. They establish legible ordinary groups and selected connectors
in these isolated fixtures, not the complete managed-editor/native-Web visual
gate. Web Wasm checking passes with Homebrew LLVM after an initial compiler
selection failure with Apple's clang.
The initial failed fixture iterations are retained and excluded from pass counts.
Evidence stays ignored under `artifacts/apple-column-stacks-v1/`; the integration
snapshot is under `artifacts/apple-main-integration-e46f271/`.

This closes implementation of Apple full-column stacks; broader UIKit/physical
input, full-editor visual parity, lifecycle/provider recovery, the complete
feature inventory and sustained 90/120 Hz acceptance remain open. The overall
Apple goal is **incomplete**.

## Shared native title bar — 2026-09-13

Apple now consumes the shared typed title-bar projection and native drag protocol
also used by Android. Both hosts expose Customize Title Bar, three shared sizes,
all three placement zones, footer visibility, component-bank dragging, multi-select
tool insertion, hidden-item overflow, context/keyboard movement, Cancel and one
Done history entry. Rust owns the frozen geometry, validation, placement and
history; AppKit/UIKit own timing, slop and contact capture. The native owner
resolves and applies release actions together on its serial queue. Mac menus
remain in the OS menu bar; unavailable controls are projected out of portable
saved models and rejected as bank sources.

Fresh Sketch uses individual header tools without the old toolbar bands or
footer. The established untouched-default update preserves working values and
edited histories. Apple's fixed header, separate Zen fallback, global clock
visibility view, unused workspace-switcher sizing mode and duplicate Main Menu
assembly are removed. Controls keep the live canvas underneath, with contrasting
ink, live foreground/background paints and shared drawer anchors. Menu-label
measurement follows the actual native font and shared spacing. Removing Web's
obsolete narrow workspace-pill padding rule aligns its allocation with Apple.

Native checks found two presentation defects: proportional measurement of a
monospaced clock changed geometry during a minute tick and canceled a held drag;
an inactive outer context wrapper replaced nested popup requests. Measurement
now uses the displayed monospaced digits, and the shared popup preference helper
appends active requests while preserving children. iPad fullscreen observation
compares effective scene and display coordinates. A final Mac capture exposed
brush hover underneath transparent title-bar controls. Native hit testing now
clears idle canvas hover over UI, while preserving active-contact completion.
The corrected capture retains the textured brush cursor on canvas and removes
it over the Layers opener; no renderer or visual workaround was added.

Integration through `ba77f9b` passes 483 Apple/core/host/UI tests, with one existing
host benchmark ignored. The separate workspace default-update test passes on
all five supported header platforms. Pixel regressions wait for the shared
asynchronous raster restore before exact comparisons; active-input checks still
advance individual frames. Web Wasm checking, header-menu and workspace-switcher
interaction checks pass. Native AppKit input iteration 9 covers both presets at
all sizes, inert bank clicks/holds, immediate mouse/pen pickup, held item menus,
same-contact movement, detach/re-entry, minute changes, cancellation, keyboard
movement and narrow overflow. Six managed persistence cases pass Done, workspace
switching, unfinished-preview close/restart and persisted Undo/Redo.

Both signed iteration-65 builds pass. Native iteration 64 passes Metal launch,
customization and fullscreen status on both hosts. The iPad journey includes
Main Menu → File → Recovered Drawings and Window → Customize Title Bar.
Iteration 65 corrects stale test button names and passes fresh Sketch drawer
switching/toggling on both hosts. Mac iteration 66 reproduces the hover issue;
signed build 68 and its native drawer workflow establish the fix. Native Mac
iteration 69 passes drawing and Undo/Redo afterward. The iPad review namespace
is restored and the artist app descriptor is unchanged; Files has no remaining
authentication blocker.

The visual matrix includes 144 native and 144 Web captures. Native item allocation
matches shared geometry within 0.5 logical pixels. After menu and switcher spacing
fixes, the comparison has no unmatched items and a largest geometry difference
of 4.14 logical pixels, attributable to clock padding/measurement. Native capture
iteration 6 preserves all header PNG bytes and geometry after shared API/default
integration. Normal-size captures verify readable ink, selection backgrounds,
live paints and the open Sketch drawer on both hosts. Web still has solid tile
backgrounds while GTK/Apple use transparent controls; full-editor visual acceptance
remains open. These AppKit event streams do not establish physical Pencil coverage.

The final integration through `1c83a95` changes Android header rendering,
Android/Windows validation and documentation, leaving the tested Apple and shared
Rust source unaffected. All ten recovery stashes remain. Evidence, integration
snapshots and failed iterations stay ignored under `artifacts/apple-titlebar-v1/`
and the named Apple main-integration folders. The overall goal remains incomplete:
full feature/menu/panel inventory, Apple column stacks, retained renderer replacement
and durable lifecycle recovery, provider/interruption cases, physical Pencil and
keyboard input, full-editor visual review and sustained Mac 90 Hz/iPad 120 Hz
workloads with current encoded-sRGB8 raster baselines are still required.

## Native Files, full Zen and raster integration — 2026-09-13

A physical-iPad diagnostic proves that the SwiftUI picker sheet can dismiss
before `UIDocumentPickerDelegate` delivers its selected URL. Treating that
sheet event as cancellation consumed the pending callback. The generated project
was present in Files, but native Open ignored it and Save did not acknowledge
the location. Both document and workspace-package services now complete only
through the native selection/cancellation delegate and share one picker adapter.
No timer, retry or presentation override is added. Temporary diagnostic code
and its failing inspection test are removed; raw evidence stays ignored under
`artifacts/apple-native-files-v1/`.

The iteration-41 physical-iPad batch passes four workflows with no failures or
skips: reopen the saved three-layer project, create/export/cancel and close,
full Zen with Tab restoring controls, and the Zen icon context menu. The fresh
native Save/restart/Open/PNG roundtrip passes in iteration 43 without failures or
skips, retaining all three layers and the editable project title. Its iteration-42
folder-title assertion failed before creating a folder or writing a file;
iteration 43 corrects that read-only query.

Shared main removed partial Zen. Apple's unused edge-toolbar projection and
partial-mode guards are removed. Obsolete tests now cover full Zen; its remaining
Change icon action is preserved. The iteration-42 Mac batch passes all three
checks without failures or skips: full Zen/Tab, collapsed and nested drawers,
and workspace order/pin persistence across restart. Its first run had two stale
test assumptions (Mac canvas element type and selected default workspace), both
corrected without changing product defaults.

The integration through `33eead2` passed 67 Apple/native-host checks, both signed
iteration-49 builds and three native workflows on each host. The three generated
Files folders were removed in iteration 48; temporary diagnostic and cleanup
source was removed. The next pull, `c6a6587`, replaces stroke archives with
immutable raster projects and encoded-sRGB8 paint tiles. Apple now delegates
save/recovery validation to the shared raster snapshot policy while retaining
the document-idle requirement for opening. An active-contact ABI regression
checks that saves and recovery contain only committed tiles, leave live ink
dirty, permit completion, and preserve exact Undo/Redo on both Apple presets.

Regression oracles now compare exact decoded tiles and mask coverage alongside
metadata and composited pixels. Loaded raster publication identities are not
content equality. Workspace publication tests validate each session's revisions
before normalizing independently allocated identities for schema comparison.
Completed-input fixtures wait for deferred raster capture instead of assuming
one frame always commits pen-up. No old stroke-history or archive reader remains.

The raster integration passes 469 Apple/host/core/UI tests, with one existing
hardware benchmark ignored, and the Swift file-service fixture on both Apple
presets. Both signed iteration-50 builds pass. Native iteration 50 passes five
Mac and six iPad workflows without failures or skips: Metal launch/drawing as
supported, artwork recovery, New/export cancellation, workspace order/pin
persistence and full Zen/Tab, plus a fresh iPad Save/restart/Open/PNG roundtrip.
The restored project retains its three layers and artwork. The review namespace
is restored and the artist app descriptor is unchanged. An opt-in native cleanup
check is added to the iteration-51 runner; its production executable is unchanged.
Files authentication is no longer a blocker.

Local main now includes `39a772c`, a Web event-loop capture follow-up with
unchanged native capture policy. All working files and seven stashes survived
the integration. The final 113 Apple/host/core checks pass with one existing
benchmark ignored; the unchanged UI suite's 356 passes remain applicable.
Both signed iteration-52 builds pass, and each host passes its native Metal
launch/layer workflow without failures or skips, including Mac mouse drawing
and Undo/Redo. The generated Files folder is removed by iteration 51. The current
iPad app and runner are installed, its review namespace is restored and the
artist descriptor is unchanged.
Evidence remains ignored under `artifacts/apple-main-integration-c6a6587/`,
`artifacts/apple-main-integration-39a772c/` and `artifacts/apple-native-files-v1/`.
Group these changes and acceptance into one milestone.

The shared title-bar editor, remaining provider/lifecycle cases, physical-input
coverage, visual inventory and sustained Mac 90 Hz/iPad 120 Hz performance
remain required. The new encoded-sRGB8 raster format needs current visual and
performance baselines; older linear8 filter comparisons remain historical.

## Native file-dialog cancellation — 2026-09-13

Both native hosts pass the original New/Export cancellation workflow with zero
failures or skips: invalid dimensions are rejected, a 63×47 drawing is created,
the system export picker is cancelled, and the drawing remains intact. The iPad
then closes its editor through File, verifying that cancellation releases the
document operation. A separate authenticated diagnostic also verifies that
Export becomes enabled again after cancellation.

The test's generic Cancel query selected a non-button accessibility element
ahead of Files' visible close buttons. The corrected query selects the native
button. No production picker change, coordinate workaround or authentication
bypass is added. Both signed iteration-32 builds pass; their production
executables match iteration 29. Temporary diagnostic source is removed, the
current iPad runner is installed, the review namespace is restored and the
artist app descriptor is unchanged. The earlier disconnected run, protected
picker capture and original invalid-target failure remain recorded under
`artifacts/apple-export-cancel-v1/` and the preceding docking evidence folder.

Routine file regressions use the existing isolated dialog fixtures. Real Files
checks are batched when native acceptance or picker changes require them, to
honor the user's request to avoid repeated authentication interruptions. No
authentication prerequisite remains pending. This closes the focused native cancellation gap;
provider delivery/interruption, the full feature/visual/input inventory and
sustained drawing performance remain incomplete.

## Native docking and contact identity — 2026-09-13

Both hosts pass drawer-tab reorder, tear-off, redock and whole-group tear-off,
collapsed/nested drawer navigation, and panel configuration followed by a live
group drag. The connected iPad also passes held toolbar tiles with exact
Undo/Redo and attached-column width/split resizing with history. Resulting panel
captures retain readable controls and the accepted compact Color appearance.

The original Mac drawer test failed at its first reorder. Native event tracing
showed consecutive injected mouse-down events with the same event number zero;
the adapter reused the preceding collapsed icon's hold policy for the drawer
tab. AppKit now compares the mouse-down event object shared by its pan and press
recognizers. This preserves native hold/slop and shared Rust drag/history rules.
An unreachable event-type branch is removed; tablet subtype classification
remains. Temporary diagnostic code is removed and the unchanged full Mac
drawer workflow passes with zero failures or skips.

The AppKit workspace fixture covers mouse and pen with both increasing and
repeated zero event counters, immediate drawer-tab movement through intermediate
points, exact Undo/Redo, held tile menus/release, collapsed-icon tear-off and
focus cancellation on both Apple presets. It now mounts the shared popup host.
Event round-trip checks tolerate only sub-millionth-point floating-point
rounding; button, event, window and tablet-subtype checks remain. Separate full
editor and real-canvas diagnostic fixtures also pass. Injected AppKit tablet
events do not establish physical digitizer acceptance.

Native layer and workspace-list checks pass on both presets. They retain pen
hold/menu continuation, immediate mouse rows and pen grips, shared order/history,
covered-row rejection, keyboard menus, scrolling, focus loss and source-removal
checks. The workspace-list fixture now mounts the shared popup host, resolving
its earlier keyboard-menu failure without changing product menu behavior.
The final signed Mac batch passes all four workflows with zero failures or
skips: drawer docking, panel configuration/live dragging, collapsed/nested
drawers and workspace-switcher order/pin persistence across restart.

The panel-control test now scopes duplicate Color controls to their popup and
finds the current group through the visible Brush Size tab. It no longer relies
on an obsolete preset group ID. Both signed iteration-28 builds pass; the iPad
production executable is identical to the validated, installed iteration-23 app.
All original failures, later passes, diagnostic cleanup and restored review
state remain under `artifacts/apple-docking-workflows-v1/`. After the user
opened Files, the `files-unlocked-v28` follow-up reached the visible export
picker without an authentication prompt. XCTest reported no usable hit point
for its remote Cancel element; tapping did not dismiss the picker and the test
failed. The review namespace was restored and the artist descriptor is
unchanged. Cancellation remains unverified, with no unlock question pending.
The full feature, visual, physical-input, lifecycle and sustained-performance
gates remain open.

Before publication, main advanced through `688fd76`; the clean integration
preserves milestone `d520d49` and all recovery stashes. Shared GPU upload
mapping failures now propagate to the host, including Apple's initial blank
presentation. Both signed iteration-29 builds pass. Mac mouse drawing,
Undo/Redo and layers, and iPad Metal launch/layers each pass their native test
with no failures or skips. The current iPad review app and runner are installed,
the saved namespace is restored and the artist descriptor is unchanged.

The merged suites pass 42 Apple, 25 host, 344 UI and 118 GPU checks, with 19
existing hardware/benchmark skips. The strict filter-reference check still
fails at maximum channel error 47. Its input, output and reference images and
per-case error table exactly match the retained Metal baseline; no new pixel
regression is observed and no oracle or tolerance is changed. Evidence is under
`artifacts/apple-main-integration-688fd76/` and the docking folder's
`integrated-v29/`.

## Workspace recovery and editor workflows — 2026-09-13

Fast native restarts now reclaim abandoned workspace ownership once every prior
client has exited. A shared OS file lock protects live clients; the first new
opener clears abandoned claims transactionally without shortening leases or
adding a platform retry. A real killed-process regression covers saved contents,
concurrent owners and stale fencing. The lock sidecar is protected from backup
overwrite. Built-in Layout History now enables restoration while retaining
name/deletion protections and owner/current/busy checks.

Mac and the connected iPad pass settings/workspace restart, completed artwork
recovery and independent windows. Mac also passes New/Export cancellation;
the iPad export check awaits user input at the system Files prompt. Interrupted
background/expiration and provider-delivery acceptance remain open.

Both hosts pass Navigator/Diagnostics, numeric tool controls, system status,
Layout History, filters and shortcut editing. Native curve/gradient labels now
include point/stop counts; ineffective separate value attributes are removed.
Shortcut search initially lost a character while typing. Settings, shortcut and
filter search now reuse the existing local-draft text helper; the old
workspace-only helper is removed. The full shortcut workflow now captures an
existing accelerator, replaces its binding and executes the new action in the
editor. Toolbar search, creation, rename, duplication and deletion also pass.

After integrating main through `b56bca3`, both signed builds pass and each host
passes six focused workflows with no failures or skips: Color, filters,
shortcuts, toolbar editing, restart and history. The shared regression passes
476 tests, with one existing hardware-only host check ignored. Six live Color
captures pass the unchanged oracle, with guide error at most one channel level
and field error zero. A final pull through `65a9855` retains steady command styling
while drawing; the 410 Apple/host/UI regressions and both signed builds pass.
Mac mouse drawing/Undo/Redo/layers and iPad Metal launch/layers also pass their
focused native follow-ups. The subsequent integration through `19d6722` also
passes both signed builds, both native follow-ups, 411 Apple/host/UI checks and
the focused CPU input-retirement regression. This retains the accepted Color
appearance; full-editor visual and physical-input/performance acceptance remain open.

Installed-device XCTest now uses the documented destination-artifact setup;
normal launch/relaunch works and the obsolete Color attach branch is removed.
The latest review app and runner are installed on the iPad, its saved namespace
is restored, and artist app data remains intact. Private evidence and earlier
failures remain under `artifacts/apple-lifecycle-workflows-v1/` and
`artifacts/apple-feature-workflows-v1/`.

## Compact Color panel — 2026-09-13

Both Apple editors now project the shared compact Color layout: Okhsv circle,
HSV square, HLS triangle, overlapping paint swatches, transparency, Swap, shape
icons and curved shape/RGB readouts. Rust owns color conversion, picking,
geometry and text. Native field/guide caches replace the old HLS-only cache and
bridge; header and readout font metrics use one shared helper. Native contact
adapters retain the starting wheel region and cancel on shape/slot changes.

The connected iPad workflow exposed foreground hit testing extending into the
visible background swatch. Adding the standard circular content shape to each
styled swatch fixes the original center tap; the same unchanged workflow passes
on Mac and iPad. Both final signed builds include main's latest shared docking
changes. Each host passes its full Color workflow with no skips, including all
shapes, readouts, slots, Swap, empty corners and continuous dragging. All six
live color oracles pass, with guide error at most one channel level and field
error zero. The iPad review namespace is restored; artist data remains intact.

The 216-case component matrix on each host passes color checks at the unchanged
two-level tolerance. Native control frames agree exactly, with Chrome differing
by at most 0.00521 points. Eighteen Mac hover/press/cancel captures also pass their
interaction checks. Complete PNG differences remain available. Representative
normal-size review, including the largest mean-error UIKit case, finds no
material geometry or color mismatch; residual native rasterization is acceptable
under the user's perceptual-parity criterion. This does not close whole-editor
visual, physical Pencil, lifecycle or sustained-performance acceptance.

Fractional native text positioning and standard Mac icon compositing improve
alignment without an additional renderer. UIKit icon compositing and alternative
primitive drawing methods were rejected because they did not help. No further
compositing experiment was adopted. The integrated shared regression passes
407 tests (42 Apple, 25 host, 340 UI), with one existing hardware-only host check
ignored. All 22 Metal-analysis tests pass; presentation stalls and the complete
performance workload matrix remain open. Private captures, deployment metadata
and failures are retained under `artifacts/apple-color-milestone-v1/` and the
preceding Color artifact directories.

## Shared flat editor menus — 2026-09-13

The shared flat menus, attached column panels and icon integration form one
editor milestone. The main-menu trigger and dark text are fixed; the connected
iPad and an isolated Mac editor pass the focused workflows below. The overall
Apple goal remains incomplete; resume from the [handoff](../development/apple-handoff.md).

The user confirmed a real upward-drag failure in the native iPad row menu;
the local probe recorded UIKit reoffering/cancelling the original contact during
menu presentation. Moving the native menu alone would not remove its lift
animation or input ownership change. Retired the native context-menu/drag-session
adapter and its attempted supplementary pan handoff.

`EditorActionMenu` now renders the shared menu model with native SwiftUI buttons,
vertical sections, checkmarks, disabled states, submenu navigation and keyboard
support. `EditorMenuButton` and `editorPopover` serve main menus, layers,
workspace rows/context menus and editor choices. A standard anchor preference
and root overlay keep popups outside clipped panels while retaining the source
contact. There is no extra window, presentation controller, drag-session bridge,
preview lift or blur. The existing UIKit hold/pan recognizers and shared
`ReorderContact` handle pickup, scrolling, cancellation and one completed drop.
Menus open inward where space permits and use opaque shared palette colors,
a thin border and a small shadow. Removed the editor-window contrast override.
macOS retains its OS application menus and native secondary-click menu adapter.

The main-menu buttons now give their entire padded labels an explicit hit shape.
Tracing showed UIKit delivering the center tap while SwiftUI never activated the
button; its state was not disappearing and no popup dismissal occurred. Removing
`ViewThatFits` did not help, so the responsive layout is retained. The shared
popup also inherits the editor root's environment instead of copying the source
control's whole environment, which had overridden dark-menu text colors.
The embedded UIKit context source transfers only appearance and enabled state;
copying its entire environment had hidden the Zen button from the connected
iPad's accessibility tree.

Menus reuse the existing native keyboard capture component for shared arrow,
submenu, activation and shortcut handling. UIKit returns focus to the preceding
responder on dismissal. A Mac fixture delivers actual app-local keys for submenu
entry/back, disabled-row skipping, Return, Escape and a shifted shortcut.
The iPad simulator and connected-device workflow pass arrow navigation, a menu
Undo shortcut and subsequent editor Redo after dismissal. XCTest's Escape and
Return checks remain open: a UIKit simulator probe received a printable key but
no Escape press or key-command callback despite owning first responder; the
connected-device Escape check also failed, and simulator Return did not execute
the action. No hardware-keyboard Escape/Return pass is inferred from Mac.

Five focused workflows pass on the connected iPad: light/dark main menus and
Undo; layer/mask/footer anchors and actions; upward layer dragging with exact
Undo/Redo; submenus, shortcuts and restored keyboard routing; workspace menu
actions and held dragging in both directions. The Zen test initially failed
because its button was absent from accessibility. After the explicit-environment
fix, its separate device run passes ordinary taps, hold/tap suppression, vertical
menu presentation and the resulting Total Zen action. No test was skipped.
Light/dark physical main-menu captures were inspected for opaque readable colors.
The isolated Mac editor passes both blend choices and Undo/Redo. Its native
keyboard fixture and the final UIKit touch/pen/mouse callback suite pass.
These checks do not claim a new physical Pencil pass or drawing-performance pass.

Final review also updates the existing workflow tests to the shared menu's
identifiers and selected traits. Both physical hosts pass workspace pinning,
reordering and persistence; toolbar styles and Zen; and toolbar creation,
renaming, duplication and deletion. Mac's Select All menu action now forwards
to the focused AppKit text editor before canvas dispatch, fixing Command-A
replacement in the picker. Toolbar grip labels include the toolbar name because
Mac does not expose a generic grip's accessibility value. The shared test uses
native Select All before typing instead of caret-dependent backspace counts.
The final customization checks pass separately on each host after these fixes.

Both signed Debug hosts build. The current shared regression run passes 41 Apple
bridge, 25 host and 306 UI checks; one existing hardware-only host check is ignored.
The complete visual, feature, input/lifecycle and sustained-performance acceptance
remains open. Local result bundles, traces, device/signing details and captures
stay under ignored artifacts. Source and documentation are published together.

## Attached column panels — 2026-09-13

Both Apple presets expose the shared Group panel mode and render attached
stacked bodies, connected column backgrounds and the published chevron icons.
The shared engine owns nested geometry, width and panel weights, resize
transactions and auto-hide. Apple projects the real divider rectangles and
keeps the width grip above split grips where they intersect. Motion projection
now includes `column_settings`, so live widths/weights agree with their geometry.

Four native fixture cases pass across both presets and both column sides,
covering geometry, immediate pen-classified resize, live publication, cancellation,
Undo/Redo, close/reopen, auto-hide and ordinary drawer mode. The iPad simulator
width/split resize workflow passes (one test, no skips); 306 shared UI tests,
snapshot projection, drawer and workspace-motion regressions pass. Both signed
Debug targets build. Physical performance acceptance remains separate.

## Shared icon integration — 2026-09-13

Integrated the published 157-icon bank and its shared command, tool, category
and filter mappings. Both Apple hosts retain brush stroke previews while showing
each preset's medium icon; filter categories and choices now expose their shared
icons. New Layer and custom close/check/more/plus controls use the canonical
assets. Filename normalization preserves internal words such as `add-layer`.
Tool-setting action buttons now include their command icons and match the
browser's text wrapping beside a leading glyph. All 96 AppKit button bounds
match Chrome exactly across both themes, two widths and enabled/selected states.

Both signed Debug targets build. Six SVG conversion tests, 306 shared UI tests
and 41 Apple bridge tests pass. Compiled AppKit/SwiftUI and isolated Chrome grids
cover all 157 icons in 18 combinations of size, theme, tint and disabled opacity.
All 216 flat paint samples pass with at most 1/255 channel error. Full unmasked
pixel differences remain visible; the [icon audit](../ui/icon-audit.md#apple-integration--2026-09-13)
records their bounds and distinguishes AppKit components from UIKit and whole
editor acceptance. A separate UIKit simulator capture now covers the same
157 icons and 18 cases using the production view and compiled assets. Its
216 flat paint samples match exactly; full-image comparisons retain residual
rasterization differences. Both signed targets build with the action-button
changes. The isolated physical iPad editor includes the new icon bank; its
completed upward gesture session prompted the flat-menu replacement above.

## Presentation-capacity retry — 2026-09-13

The shared Apple driver now makes one deadline-bounded retry when a presentation
callback frees capacity just after a denied display tick. Atomic waiter
registration, cancellation and old-surface retirement retain the serial owner's
single-frame admission. Retries have separate trace records, so they do not
inflate display-link counts. Direct driver/gate tests and actual owner/Metal
resize/resume/detach checks pass on both presets; both physical Release builds
compile, and all 29 trace-analysis tests pass.

A matched 45-second Mac watercolor pair reduced long continuous presentation
intervals from 225/3,539 to 46/3,708, with no CPU-budget exceedances. The candidate
then completed ten measured minutes: CPU p99/max 5.722/9.139 ms, no rejected
input, frame errors, overflow or missing/zero-time presentations during
measurement. It still has 1,329/49,040 long continuous intervals (2.71%).
The [performance report](../../apps/layer-apple/PERFORMANCE.md#retry-after-presentation-capacity-returns--2026-09-13)
retains the complete observations and limits. Physical iPad timing, residual Mac
cadence, the workload matrix and the broader parity/performance goal remain open.
This milestone does not claim new iPad hardware performance evidence.

## Header font and background alignment — 2026-09-12

A fresh 96-case comparison of both Apple presets against rebuilt local Chrome
reproduced the header allocation error: the widest title differed by 1.220
logical points and the largest edge difference was 1.078. Native font inspection
showed that the system Medium instance uses weight 510, while the web requests
CSS 500. Apple's shared header now measures and draws workspace labels using
that public variation axis at 500, retaining the native font when unsupported.
The workspace pill also uses the shared opaque 80%-background/20%-black track
color. Font and text-width caches stay bounded.

The same reference inputs now yield a maximum position/size difference of 0.603
points and edge difference of 0.578, meeting the one-point component geometry
bound across all 96 cases. Full sRGB differing-pixel fractions improve from
13.05–25.62% to 3.15–11.65%; maximum channel error is 214. All corresponding
full differences remain retained with the original comparison thresholds.
The geometry result does not close full pixel acceptance, whole-editor
composition, UIKit/device rasterization or the performance gates.

Both signed Apple builds pass. The physical iPad remains on its prepared layer
input fixture while awaiting feedback; this header change has not interrupted
that check. The [header capture guide](../../tools/visual/README.md#complete-header-components)
now describes recorded CSS/canvas metrics and the resolved browser font. Chrome's
[system font matcher](https://chromium.googlesource.com/codesearch/chromium/src/%2B/refs/tags/142.0.7402.2/third_party/blink/renderer/platform/fonts/mac/font_matcher_mac.mm)
applies the requested CSS weight as a variable-font weight value. Raw font,
geometry and capture artifacts remain ignored.

## Layer scrolling and interruption validation — 2026-09-12

A native 24-layer fixture exposed a stationary-capture bug: deleting the dragged
layer left its contact active until another pointer event arrived. Both hosts
now validate layer contacts when the document revision or active name editor
changes, cancelling an invalid source immediately. The native fixture fails
before this change and passes afterwards on both Apple presets.

Actual AppKit pen events retain the menu contact through edge scrolling after
the source leaves view; the completed drop matches its measured indicator and
one Undo/Redo restores it. Mouse checks cover source deletion, entering rename,
late release, list removal/remount and new-document replacement. The latter
uses the real Metal document owner and verifies cancellation of both capture
and deferred menus even when the replacement document reuses the layer ID.
Native group drops cover both group-interval boundaries and shared rejection of
locked sources/destinations or descendant cycles without changing document
revision or adding a history entry.

The iPad simulator independently passes ordinary touch scrolling before a hold,
immediate grip pickup, edge scrolling past the initially visible rows, preservation
of every other layer's order and actual toolbar Undo/Redo. A Debug-only opt-in
probe exposes complete order to this isolated test; normal scroll accessibility
values are unchanged. Both final signed builds pass. The isolated physical iPad
contains the cancellation fix and a disposable long layer list; its physical
finger/Pencil menu-to-drag and edge-scrolling check is still awaiting feedback.

Reproduce with `tests/layer-row-lifecycle.swift` through the existing Swift fixture
script and `EditorLaunchTests/testLayerListScrolling` in the iPad scheme. The
command-coverage references now include these layer workflows; the audit passes
against the retained schema-4 inventory and remains an inventory check. Complete
native workflow, visual, physical-input and sustained-performance gates remain
open. Raw fixtures, logs, captures, devices and signing details remain ignored.

## Whole-layer-row pickup and workspace footer — 2026-09-12

Both Apple hosts now route layer-row bodies and their grips through the retained
native input adapter. Mouse bodies and every grip drag immediately after native
movement slop; touch/pen bodies preserve scrolling before a hold. iPad rows use
native vertical context menus with a preview of the source row. Mac keeps the
row-anchored vertical popover, whose same-contact pen continuation passes the
owned-window fixture. The separate footer action keeps its existing popover.
Rust still owns mask/content context selection, menu capabilities, hierarchy
changes and the one completed drop/Undo transaction. Menu queries are deferred
until presentation, and removed sources cannot supply a deferred menu.

Measured row, grip, name and mask bounds distinguish pickup, native name editing
and mask actions. Paper keeps its context menu without becoming draggable.
Drag previews do not publish duplicate row measurements. Window-root recognizers
check which scroll view actually received the contact, so a floating panel
cannot accidentally pick a layer underneath it. The Mac regression reproduced
that covered-row failure before the guard and passes after it.

The direct AppKit fixture covers immediate mouse bodies and pen grips, early
pen rejection, retained hold/menu/drag contact, shared drop/Undo/Redo, covered
rows and focus cancellation on both presets. Separate model checks cover Paper,
rename ownership, checked multiselection and removed deferred-menu sources.
The iPad simulator passes mask creation/selection/deletion, content and footer
menus, short checkbox actions, immediate touch grip dragging and actual toolbar
Undo/Redo. The native UIKit callback fixture checks contact admission for all
three devices alongside existing session/cancellation/edge-scrolling coverage.
These are targeted checks: physical layer-row finger/Pencil continuation, long
layer lists, group/locked/descendant drops and the wider interruption matrix
still need coverage. Workspace physical input evidence below is separate.

Manage Workspaces now gives Cancel and Switch equal-width 40pt buttons with an
8pt gap, a subdued secondary fill and a solid shared-blue primary action.
The same shared style serves both hosts and follows the editor's text palette;
disabled and pressed states remain distinct. Actual component captures were
reviewed in light and dark themes. This changes appearance without changing
workspace selection or switching behavior. Final signed builds pass on both
hosts, and the isolated physical iPad app installs and launches with its existing
workspace list and order retained. The direct workspace row regression also
passes after the common native contact-admission change.

Reproduce the owned-window layer fixture through
`bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/layer-row-input.swift`.
The focused iPad tests are `EditorLaunchTests/testLayerContextMenuAnchors` and
`EditorLaunchTests/testLayerGripAndChildActions`. Use the existing portable
`test-native-rows.py` runner for UIKit delegate checks; it does not synthesize a
physical Pencil. Results, captures, destinations and signing details remain in
ignored local artifacts. Full visual and hardware performance gates remain open.

## Native context menus and workspace rows — 2026-09-12

The Apple targets now share a menu projection of Rust sections, availability,
checks, hints and action payloads. UIKit and AppKit provide native presentation
for non-draggable context sources such as Zen. Queries are deferred until
activation and retired when their source or document changes. Controls that
open a picker/form on an ordinary tap retain their existing popovers.

On iPad, Manage Workspaces attaches native context-menu and drag/drop
interactions to its existing scroll view. The preview belongs to the held row
and has an opaque backing so the original label cannot show through it. UIKit
retains the finger/Pencil contact across the vertical menu and drag. Mouse and
explicit grips retain the existing immediate pickup path. Native session and
contact identities reject stale callbacks and duplicate drops; teardown retires
native callbacks before deferring SwiftUI publication. Native edge scrolling
reuses the grip path and schedules display callbacks only while an edge can
scroll. Workspace order remains an application preference outside layout Undo.

Both Apple hosts now use 56pt workspace row minima, matching Android's 56dp
minimum and the web manager's 55px content plus divider. Measured default rows
and grips are 56pt high on both AppKit-hosted presets; options targets are
34pt wide and fill the row height. Native Settings text fields now retain
explicit labels when populated, including the base-color hex fields.

Validation includes native AppKit menu actions and shared Undo/Redo, asynchronous
source loading and removal, and existing mouse/tablet row checks on both presets.
The UIKit row-menu action passes and its move persists after restart. A 27-row
Simulator workflow passes ordinary scrolling before a hold, menu dismissal,
immediate grip dragging, offscreen capture and persisted order. Direct UIKit
delegate checks exercise native-session edge scrolling, leaving/reentering the
viewport, the resulting insertion target, cancelled lifts, old/new sessions and
deferred teardown. The user confirmed the production finger/Pencil hold-to-drag
workflow and subsequently confirmed the 27-row edge-scrolling check with both
finger and Pencil: the same held contact scrolls at the list edge and drops at
the indicated position without changing the selected workspace. Signed builds
pass on both hosts. These checks do not establish complete visual parity or
sustained drawing performance.

Mac workspace row menus retain the working vertical custom surface. An owned
AppKit context-menu probe posted tablet-subtype drag events during native menu
tracking; the app-local event monitor did not receive them. Native row handoff
therefore remains unaccepted on Mac; a native replacement must preserve the
same held contact. This does not affect the native non-row source/action checks.
The subsequent whole-layer-row milestone above extends the shared input adapter
to layer bodies and grips; its physical coverage remains separate.

## Readable popup surfaces — 2026-09-12

Custom overlays, popovers, app-owned sheets and the workspace pill now use
opaque colors. Popup foregrounds and backgrounds come from the same shared
palette; settings keep a native opaque semantic surface. Popup presentation
backgrounds also cover native margins and arrows. Action hints are stronger,
while disabled actions retain their disabled state and existing appearance.
Default configured sRGB popup text contrast is 9.79:1 dark and 11.55:1 light;
the 75%-opacity hint ratios are 6.34:1 and 5.54:1. These calculations cover the
default shared colors, not arbitrary user themes or native materials.

Both Apple targets build, including the signed device target. The user confirmed
that the updated full iPad editor is readable, then reported backgrounds that did
not follow Dark mode. Native window appearance now follows the explicit editor
setting while observing the underlying system separately. Returning to System
therefore works even after an explicit override. Presented sheets and popovers
read the live shared palette/theme instead of retaining their opening values.

The focused Settings workflow passes on Mac and iPad Simulator: choose the theme
opposite the system, then return to System with the sheet still open. Reviewed
captures show matching sheet, editor and layer-popup colors in both themes.
A direct AppKit fixture also verifies explicit choices, returning to System and
application appearance changes without UI automation. Earlier iPad layer-popup
and native Zen-menu captures established the opaque/increased-contrast surfaces;
these native-menu captures included uncommitted source-adapter work and do not
establish production row-menu acceptance. Direct AppKit captures confirm opaque
shared popup content in both themes; they do not measure native NSMenu pixels.
The final milestone also passes both focused UI workflows and the signed device
build with unfinished native-menu sources excluded. The isolated physical iPad
app is installed and launched with the theme correction.
The existing 96-case header fixture passes on both Apple presets; all 1,044
control rectangles match the previous geometry within floating-point noise.
The production native row menu/drag adapter still requires interaction validation.
These checks do not close full accessibility, visual parity or performance gates.
Raw captures, local identifiers and signing information remain in ignored artifacts.

## Glass surfaces and layer origins — 2026-09-12

Both Apple hosts share one availability-aware glass modifier for custom
workspace context overlays, workspace/recovery notices, document progress and
the header's workspace pill switcher.
Existing native sheets, alerts and popovers retain their system presentation.
The custom surfaces preserve their previous dimensions, padding, corner shapes
and actions; Reduce Transparency selects an opaque semantic background.

Layer menus previously attached to the entire panel, so iPadOS placed their
arrow at the panel center. Each visible row now owns its presentation, including
mask gestures, while the footer action owns a separate button presentation.
Pending queries are invalidated when the source disappears or the document
changes. The focused iPad Simulator workflow passes layer selection, mask
creation/deletion and content/footer menu presentation. Its full-screen captures
were inspected: mask and content arrows meet their respective rows, and the
footer arrow meets the invoking button.

Direct AppKit checks pass for mouse/pen row pickup, held-menu release and
same-contact dragging, immediate grips, scrolling, keyboard menu actions and
cancellation on both Apple presets with the new glass surface. Shared changes
through `b0c984b` are integrated; all 41 Apple, 25 host, 295 UI and 64 native
workspace tests pass (425 total, one existing host test ignored). Both final
Apple builds pass. The signed isolated iPad app installs and launches in a fresh
test workspace; that launch adds no physical Pencil or timing evidence.

The final shared prediction-capability change through `907c632` is also
integrated. The 41 Apple and now 296 UI checks pass again, along with the new
engine oracle for platform-prediction preference handling. The Apple defaults
retain native prediction availability.

With the switcher glass included, both Apple builds pass again and the focused
iPad editor capture passes. All 96 existing header component cases pass across
the two Apple presets, three task workspaces, narrow/wide widths, themes,
clock visibility and paper/surround backgrounds. Comparing their 1,044 measured
rectangles to the preceding switcher fixtures finds no geometry change beyond
floating-point noise. Invisible AppKit captures establish layout, not compositor
glass pixels; the separate iPad screen capture shows the presented glass pill.
Direct captures of an isolated native Mac window also confirm the glass pill in
light and dark editor themes. Native materials now receive the resolved editor
color scheme, preventing dark system glass behind light-theme text. The outer
editor still observes the OS scheme for the shared automatic-theme setting.
Both builds pass after this correction; the owned Mac capture windows are closed.

Concurrent workspace changes through `46e48c8` are integrated before publication,
including protected included-workspace names and shared attached-column layout.
The rebased Mac and signed iPad builds pass, along with all 426 Apple/host/UI/
native-workspace tests (one existing host test ignored). The earlier menu and
glass captures cover the unchanged Swift presentation; they do not establish
new attached-column UI parity.

This completes the popup appearance and layer-anchor refinement. Converting
custom context-menu contents to native Apple menus while retaining the confirmed
held-contact drag behavior remains open, as do the broader feature, visual and
performance gates. Raw captures, device/signing metadata and experimental
native-menu handoff code remain in ignored local artifacts.

## Native workspace pickup — implementation in progress, 2026-09-12

Toolbar tiles and collapsed-column icons now share retained contact ownership
with the workspace manager's AppKit/UIKit adapters. Native timing and device
classification gate pickup; tabs/grips remain immediate. Menus stay open on a
held pen/touch release and close when that same contact starts dragging. Rust
still validates drops and owns the layout/history transaction. Separate measured
source instances preserve both a column icon and its open drawer tab.

The native AppKit fixture passes mouse/tablet tile gating, menus, reorder and
one-step Undo/Redo on both Apple presets. It also passes immediate pen grip
pickup with focus-loss rollback, plus collapsed-icon gating, retained menus,
same-contact tear-off and Undo. The full row regression passes mouse/pen and
keyboard actions, removal during capture, late release, remounting, native
scrolling and offscreen capture. A failing remount fixture had omitted the
cancelled contact's final mouse-up before starting another mouse-down; the
corrected sequence verifies that late release cannot commit a removed source.
AppKit's native recognizers handle these events without a local event monitor.

The UIKit simulator workflow passes touch tile rejection, hold/release, menu
dismissal, same-contact reorder and Undo/Redo. Immediate toolbar-grip pickup now
also passes, with explicit assertions that the app window stays fixed and Undo
restores the original grip position. iPadOS 26 and later reserve 36 logical
pixels below editor controls to avoid the system's bottom-corner resize gesture.
This is a narrow platform control accommodation: Metal and the shared canvas
viewport retain the complete window, while Rust applies the runtime clearance
to docks, floating movement/resizing, expanded panels, drawers and partial Zen.
The clearance survives workspace changes, previews and Undo/Redo, but is excluded
from portable layouts, history and persistence. UIKit reported zero safe-area
insets at the failing corner; edge-gesture deferral did not prevent OS resizing
and was removed. Fresh iPad windows request the Mac's 1200×900 default size;
restored and OS-sized windows remain resizable.

Collapsed columns draw shared separator bounds, use the editor's 30% text color
for separators, point compact chevrons toward the canvas, and highlight only an
open drawer's origin. All 290 shared UI tests pass, including serialized divider
geometry, scrolling, native clearance and its storage lifetime. The 41 Apple and
24 host bridge tests pass with native Metal access; one existing host hardware
benchmark stays ignored. Both Debug apps compile. The invisible drawer geometry
regression also passes. Local evidence is under `artifacts/apple-workspace-input*`,
`apple-workspace-native-input*` and `apple-workspace-bottom*`; raw captures,
results and machine identifiers remain ignored.

Shared changes through `5c68977` are integrated, including starting-layout
preview before restoration. Both Apple hosts now show that preview before the
shared confirmation; Cancel or dismissal restores the previous layout, while
confirmation is one Undo/Redo step preserving tool settings and document state.
The real Swift coordinator checks pass for both Apple presets, along with all
59 native workspace tests. Integrated macOS and physical-iPad-target builds pass;
the latter is a compile check, without a new device install or physical-input
claim. Only the owned simulator fixture and runner were removed after testing.

These checks do not establish physical Pencil row/column behavior, the complete
disabled/divider/drawer input matrix, whole-layer-row pickup, small-window/header
parity, full pixel parity or sustained hardware performance. Those gates remain
open; no physical Pencil result is inferred from simulator or injected events.

The user subsequently confirmed the isolated physical iPad workspace-row Pencil
check: early row motion does not reorder; holding opens the menu; continuing
that contact closes the menu and reorders; lifting retains the menu; and the
left grip drags immediately, without stray selections or lag. This confirms
those row gestures on the installed switcher build, not the later tile/column
changes. The requested next refinement is native Apple context-menu presentation
while preserving this same-contact behavior; that migration remains open.

## Configurable workspace switcher — 2026-09-12

Both Apple hosts now project the shared saved pins and complete workspace order.
The header temporarily prepends an unpinned current workspace, scrolls overflowing
choices and uses the shared borderless recessed track. Manage Workspaces adds
pin marks, narrow grips and Show in top bar / Move Up / Move Down actions.
Preference edits preserve the current document, workspace identity and canvas
preview, and refresh other windows without rebroadcasting passive refreshes.
The manager follows the revised search-free workspace list and permanent-delete
confirmation; deleting its active workspace selects an available included
workspace through shared Rust policy instead of creating another workspace.
The direct history view now uses the shared legacy-caption normalization and
revision ordering, matching the controller-based hosts without rewriting saved
history. Current-row captions and selected descriptions use the same vocabulary.

Native AppKit/UIKit recognizers retain the actual device and contact. Mouse row
bodies and all grips drag immediately; touch/pen bodies require a native hold.
Touch/pen holds retain the menu after release and close it when the same contact
starts dragging. SwiftUI button recognition can coexist with pickup; a recognized
hold/drag suppresses the ordinary button action. Window loss, source invalidation
and removing a captured list retire the interaction. Menu buttons support explicit
keyboard focus, arrow navigation and Return. This migrates workspace-manager
rows only; the other Apple sources in the drag inventory still need work.

Validation at this checkpoint:

- All 41 Apple Rust tests and 58 native workspace tests pass after integrating
  the shared updates through `af09dfd`. Actual Swift manager workflows pass on
  both presets, including deletion replacement without creating another workspace.
- Final Debug builds and signature verification pass for both Apple targets.
  The attached iPad retains the earlier switcher gesture build while its physical
  check awaits feedback; final history-caption integration has not been installed.
- The Swift switcher checks cover saved pins/order, the unpinned current choice,
  unchanged document/history during preview, Cancel, two-window notifications,
  invalid drops and restart. Measured-row checks preserve insertion before an
  unmounted successor and cancel when the source disappears.
- AppKit event delivery passes under both presets: ordinary click, mouse hold
  without a menu, secondary click, keyboard menu action, immediate row/grip
  dragging, shared drop, focus-loss cancellation and removal during an active
  drag. Tablet-subtype events also cover early pen motion, native holds, retained
  menus, same-contact dragging and immediate grips. A 21-row list verifies native
  scroll phases dismiss menus, edge scrolling retains an offscreen source and
  the persisted order matches the measured drop hint. The fixture uses actual
  window hide/restore for focus loss and measures the current native clip after
  resizing. These checks use one owned window and no system-menu automation.
- The iPad simulator passes pin actions, early-motion rejection, stationary touch-hold menus, same-contact
  held dragging, immediate grip dragging, retained row selection, Move Down and
  pin/order persistence after restart. Offset menu buttons need measured in-app
  pointer delivery because XCTest omits their hit point; resulting state is checked.
  Physical iPad runner installation succeeds, but automation initialization times
  out before test execution. The corrected signed app is installed and launches;
  its physical Pencil row check is pending.
- A focused iPad simulator long-list test passes with 27 workspaces created
  through the real coordinator. Early touch motion scrolls without reordering;
  scrolling dismisses a held menu; a grip contact scrolls the original rows out
  of view and commits its drop. A separate SQLite read confirms only the dragged
  workspace moved, retaining the other 26 rows' order and every pin. The helper
  uses a disposable bundle and namespace, rediscovers the data container after
  XCTest installation, and removes its own app and runner afterwards.
- All 96 header component pairs retain full sRGB differences. Exact differing
  pixels range from 3.414% to 12.149%, with maximum channel error 214. The maximum
  position/size error is 1.220 logical points and edge error 1.078, so the full
  geometry/pixel gate remains open. These AppKit component captures cover both
  presets, not physical UIKit composition or dynamic overflow states.

Reproduce focused checks with `tests/workspace-switcher.swift`,
`tests/workspace-switcher-input.swift` and `tests/workspace-manager.swift` through
the [Swift fixture script](../../apps/layer-apple/scripts/test-project-files.sh),
or select `EditorLaunchTests/testWorkspaceSwitcher` in either Xcode scheme.
For the seeded simulator workflow, run
`python3 apps/layer-apple/scripts/test-workspace-scrolling.py` (optionally select
an available iPad with `--simulator`). It requires one passing test with no skips
and checks the final database; raw destinations, logs and results stay ignored.
The [header capture guide](../../tools/visual/README.md#complete-header-components)
describes the matched Chrome workflow. Raw results and device details remain in
ignored local artifacts. Physical Pencil row gestures, full input/visual coverage
and the existing hardware drawing performance gaps remain open.

## Owner resource lifetime and drawable admission — 2026-09-12

Both Apple targets now drain temporary native objects after every serial owner
task and defer frame admission while all configured drawables await presentation.
Callbacks retire tickets independently of CPU submission. Resize, resume and
surface replacement invalidate old tickets without allowing late callbacks to
retire new work. Display-link preferences and idle policy remain unchanged.
A direct 64-request lifetime regression fails against the prior owner and passes
on both presets. Gate/driver checks cover retries and callback races; a real
Metal owner fixture verifies explicit resize/resume/detach/reattach paths using
injected pending tickets. Full physical interruption coverage remains open.

Both signed physical Release apps complete ten measured minutes of predicted
4K multilayer watercolor on shared source through `990b515` plus these runtime
changes, with optional GPU timestamps disabled. No rejected input, renderer
errors, overflow or missing/zero-time presentations occur during measurement.
Mac records 46,920 presentations with CPU p99/max 6.025/9.402 ms; iPad records
65,953 with 9.122/17.711 ms. The Mac CPU budget passes in this sample, but 3,651
of 46,544 continuous intervals exceed its 90 Hz tolerance. iPad has 1,126 long
intervals out of 65,577 and 1,176 CPU frames above 8.33 ms. Both sustained
presentation gates still fail. Measured footprint growth is 172.81 MiB on Mac
and 19.78 MiB on iPad, with nominal thermal states; recording/workload allocation
has not been isolated from memory growth. Both owned apps are closed.

The [performance report](../../apps/layer-apple/PERFORMANCE.md#owner-lifetime-and-presentation-admission--2026-09-12)
retains the short baseline/pool/gate comparisons, complete sustained metrics,
full-run anomalies and reproduction commands. The analyzer distinguishes
drawable-capacity deferrals from pending owner work and retains legacy unknown
reasons; all 28 analysis checks pass. The iPad Simulator build also passes its
separate Metal API path. New workspace switcher controls and Apple device-specific
drag pickup remain implementation work. Full UI/visual, physical input/lifecycle,
GPU, latency and sustained workload acceptance remain open on both platforms.
Save/Load Layout remain excluded, Mac 120 Hz remains deferred, and raw artifacts
and signing/device information remain private.

Android pickup, default workspace pinning and shared divider changes through
`dc2e651` are integrated. The 377 Apple/shared UI/native workspace tests pass.
Both integrated signed Release apps verify and complete separate five-second
ink smoke checks with no measured input rejection, renderer errors or missing
presentations. The Mac capture shows visible strokes. These checks preserve
the separate source attribution of the ten-minute measurements above.

## Native GPU correlation and resumed hardware measurements — 2026-09-12

Both physical hosts now have actual Instruments GPU captures correlated to
native drawing-frame records. The new `metal_frames.py` analyzer uses exported
Mach clock anchors, complete CPU encoder intervals and process/encoder/buffer
identities. It unions overlapping GPU execution and retains partial, missing,
ambiguous and invalid observations. Native exports include their PID; legacy
captures require verified pairing. Ten new correlation checks and the existing
17 trace checks pass, along with the Swift recorder's concurrency, completion
grace and atomic process-tagged export checks.

The short ink diagnostics match 122 iPad frames and nine Mac frames. Observed
GPU union p99 is 1.704 ms and 0.760 ms respectively, but the capture windows are
incomplete and retain unmatched work. These results do not establish complete
GPU frame coverage or physical input latency. See
[the correlation guide](../../apps/layer-apple/PERFORMANCE.md#correlating-native-drawing-frames)
for reproduction, coverage limits and actual retained durations.

An unpublished full-cadence preference also completed ten measured minutes of
ink on both physical hosts, with optional GPU timestamps disabled. Each receives
135,003 nonpredicted input samples without rejection or renderer errors. iPad
records 66,975 actual presentations and Mac 49,811. CPU p99 is 4.794 ms on iPad
and 7.602 ms on Mac, but continuous presentation p99 is 12.499 ms and 22.222 ms.
There are 750 iPad and 1,084 Mac continuous intervals above the host period plus
5% tolerance. Drawable acquisition dominates over-budget CPU samples; iPad
stalls cluster at stroke restart while Mac stalls also occur during strokes.
The candidate does not establish a sustained improvement and was not adopted.
The published scheduler remains unchanged. The
[complete results](../../apps/layer-apple/PERFORMANCE.md#full-cadence-experiment-and-ten-minute-ink-2026-09-12)
retain maxima, memory growth, instrumentation settings and missing evidence.

Shared workspace/switcher, Windows numeric-control and GTK/Web drag milestones
through `bee29ce` are integrated. The 396 Rust checks pass with real Metal access;
one existing host hardware test remains ignored by default. A preceding sandbox
run failed its GPU cases with `AdapterUnavailable` and is retained locally.
Both final signed Release apps build and verify. Each completes a five-second
ink smoke interval after warm-up on the integrated source with the original
scheduler: 559 actual iPad presentations and 411 Mac presentations, with no
rejected input, renderer errors or missing/zero-time presentations during
measurement. Both exported PIDs match the launched process, and the owned apps
are closed. These short checks do not replace the candidate's separate
ten-minute measurements or establish sustained acceptance for the final build.
The new switcher configuration and Apple device-specific drag requirements remain
implementation work, as do full feature, visual, lifecycle, physical-input and
performance acceptance. Save/Load Layout remain excluded and Mac 120 Hz deferred.
Raw artifacts, process/device IDs and signing information stay private.

## Shared compact layer opacity — 2026-09-12

Both Apple targets now use the common numeric editor for layer opacity, removing
its separate expression field, slider and pending-value handling. Compact mode
reserves width from shared formatted limits, switches between readout and native
entry, and retains the common expression/slider policy and acknowledgment state.
Document/layer identity retires old drafts, and delayed edits check their target.
Inline errors retain an outline and accessible/hover feedback even when a backend
rejection arrives after entry closes. Formatted limit labels are cached on mount,
without adding range-formatting calls to each slider update.

The 72 invisible AppKit/Chrome pairs cover both Apple presets, both themes, three
panel widths, 0/50/100 values and enabled/disabled state. All 216 complete idle
control bounds are within one point, maximum 0.40625. Complete comparisons retain
1,251,072 pixels: 49,352 differ exactly, per-case fractions 0.873–7.338%, weighted
mean channel error 0.986982 and maximum 191. The previous implementation differed
at 92,542 pixels with mean error 4.201418. Exact parity still fails; differences
are retained. Native entry geometry and other font sizes remain outside this
idle-component fixture. The standard numeric fixture's four complete images and
792 geometry measurements remain unchanged, and both sets of mounted AppKit
editing checks pass. Numeric acknowledgment/rollback checks also pass.

The focused iPad Simulator workflow validates expression entry/correction,
independent Properties readback, Undo/Redo, keyboard dismissal, layer switching
with an invalid draft and slider input. Initial failures exposed two test
assumptions: UIKit accessibility returns glyph bounds for the padded readout,
and the onscreen keyboard covers the lower layer rows. The test now leaves full
geometry to the measured layout fixture and uses the visible keyboard-dismiss
control before switching targets. The final-source workflow passes; earlier
failed results remain local evidence. The full initial editor comparison differs
at 302,151 of 5,680,128 pixels (5.319440%), with mean channel error 1.075338 and
maximum 255. Both final signed Release targets build and verify. The Mac test
compiles but was not executed. The owned simulator is deleted, existing artist
sessions are preserved, and the incoming PWA packaging-test update is integrated.
Mac UI automation and physical-device execution remain separate from these tests.
Reproduction is in the
[compact-opacity guide](../../tools/visual/README.md#compact-layer-opacity).

The remaining feature/visual/lifecycle matrix, physical rendering and sustained
Mac 90 Hz / iPad 120 Hz performance gates remain open. Save/Load Layout remains
excluded.

## Shared property and layer dropdowns — 2026-09-12

Both Apple targets now use one dropdown component for property choices, Curves
channels and layer blending. The shared layout matches measured browser select
padding, flex shrink, disclosure position/shape, text baseline/clipping and
disabled opacity. The compact layer popup now uses the same shared check icon,
option routing and content-sized popover. Existing Rust edit actions are retained.

Direct native/Chrome comparisons cover both Apple presets, two themes, three
widths, short/long selections and enabled/disabled states: 48 images and 144
controls. All control bounds are within one logical point, maximum 0.126.
Complete comparisons retain 6,325,760 pixels: 196,638 differ exactly, with
per-case fractions 1.706–5.014%, weighted mean channel error 0.439592 and maximum
191. The preceding implementation differed at 1,526,184 pixels with mean error
5.104690. Exact parity still fails; font rasterization and narrow-label
truncation differences are retained. These default-font AppKit component
captures do not establish every font size, popup or physical UIKit/Mac result.

The focused iPad Simulator workflow passes property/compact popup selection,
synchronized values and in-app Undo/Redo. Its full initial editor capture
compared with the unchanged Chrome reference differs at 302,519 of 5,680,128
pixels (5.325919%), with mean channel error 1.086183 and maximum 255. The Mac
UI test target compiles but was not executed. Both signed Release apps build
and verify. The owned simulator is deleted, and existing artist sessions are
preserved. Reproduction is in the
[dropdown comparison guide](../../tools/visual/README.md#property-and-layer-choices).

The remaining feature/visual/lifecycle matrix, physical rendering and sustained
Mac 90 Hz / iPad 120 Hz performance gates remain open. Save/Load Layout remains
excluded.

## Shared vector icon paints and settled captures — 2026-09-12

Both Apple targets now preserve fixed colors in the canonical SVGs while tinting
`currentColor` with the editor foreground. The shared generator emits ordered
vector paints and a bundled manifest; the view reads it once and composites mixed
icons before applying disabled opacity. The Color tab is green, and overlapping
foreground/background swatches retain their black/white fills and drawing order.
Collapsed-column expansion uses the browser's foreground-colored `»` label.
The 92 ordinary symbolic icons retain their single-image path. No new package or
hand-maintained native icon geometry is introduced.

Six generator checks cover opacity, paint order, fill-before-stroke, unsupported
mixed compositing and the complete 96-icon catalog. Direct native and Chrome
captures cover all icons at three sizes, two palettes and normal/accent/disabled
states: 18 grids and 1,728 glyphs per host. All 216 independent flat-paint samples
pass with maximum channel error one. Complete comparisons retain 15,925,248
pixels: 527,943 differ exactly, with per-grid fractions 1.467–11.404%, weighted
mean channel error 0.226276 and maximum 107. Edge rasterization differences remain;
these component checks do not establish full UIKit or editor pixel parity.

The first live iPad Simulator capture exposed a startup race: Navigator/GPU
readiness can precede thumbnail readback and enabled command publication. The
shared capture fixture now waits for visible thumbnail images using opt-in Debug
metadata, ignored by Release. The corrected capture passes and differs from the
preceding native image at only 316 pixels in the Color tab icon. Its full matching
Chrome comparison differs at 303,396 of 5,680,128 pixels (5.341359%), with mean
channel error 1.108850 and maximum 255. The early capture is retained as invalid
startup-state evidence; it is not substituted for the settled result.

Both signed Release targets build and verify. Reproduction is in the
[icon comparison guide](../../tools/visual/README.md#shared-icon-paints).
Physical iPad/live Mac rendering, the remaining feature/visual/lifecycle matrix
and sustained Mac 90 Hz / iPad 120 Hz performance gates remain open. Save/Load
Layout remains excluded.

## Shared color-field rendering — 2026-09-12

Both Apple targets now draw the HSV field and hue ring with Canvas gradients.
The HLS triangle uses display-encoded RGBA8 generated by shared Rust at physical
pixel centers. A single-entry Apple cache rebuilds on hue or pixel-size changes;
lightness/saturation, opacity and marker updates reuse the image. Picking,
conversion and triangle geometry remain shared across ports. This replaces
native gradients whose output differed between bitmap and live rendering.

All 48 AppKit panel cases pass the unchanged independent picker check: 27,160
interior samples, maximum channel error two. The 48 corresponding Chrome cases
also pass. Their fixture manifests are identical to the previous checkpoint,
allowing reuse of the unchanged browser captures and 1,488 measured rectangles
within one logical point. Complete comparisons retain all 22,233,600 pixels:
3,376,206 differ exactly, with per-case fractions 9.777–20.803%, weighted mean
channel error 1.617740 and maximum 237. Exact full-panel parity still fails.

The focused iPad Simulator Color workflow passes its actions and independent
pixel checks. HSV passes 853 samples with maximum error two; HLS passes 680,
with hue-ring maximum two and all 310 sampled field pixels matching exactly.
These results close the previous HLS sample failure without changing tolerance.
Raster tests compare every pixel against actual picking across multiple sizes
and hues, including boundary coverage and invalid requests. Apple FFI and direct
Swift checks cover buffer validation, cache reuse, replacement and image lifetime.

On the development Mac, the Release CPU-only field benchmark records 1,000
generations per size after 100 warmups. At 320/452/768 pixels, p50 is
0.150/0.296/0.837 ms, p95 is
0.165/0.308/0.868 ms, p99 is 0.184/0.318/0.911 ms and maxima are
0.202/0.388/1.083 ms. Retained pixel storage is 409,600/817,216/2,359,296 bytes
for the one cached image. These measurements exclude host allocation, upload and
presentation and do not establish drawing-frame performance on either device.

Signed Release builds pass for both targets. The incoming Windows header/status
milestone is integrated; its clock-setting change preserves Apple behavior.
The [Color panel guide](../../tools/visual/README.md#complete-color-panels) contains
reproduction commands. Live Mac and physical iPad rendering, the remaining
feature/visual matrix, lifecycle and sustained Mac 90 Hz / iPad 120 Hz performance
gates remain open. Save/Load Layout remains excluded.

## Shared Color panel layout and native actions — 2026-09-12

Both Apple apps now use the shared Color panel layout: three paint slots with
matching checkerboard/selected backgrounds, labeled Swap and full-width
HSV-square/HLS-triangle buttons, and three vertical numeric slider controls.
Rust still owns color conversion, values, hit policy, expressions and actions.

Invisible AppKit captures cover both Apple presets, both themes, both color
spaces, all selected paint slots and two widths. All 1,488 control/paint bounds
across 48 production native/Chrome pairs are within one logical point. Full exact
pixels still differ at 14.789–24.191% per image, with mean channel error 2.487
across 22,233,600 pixels and maximum 207. Gradients, text and edges remain
unmasked; AppKit component captures do not establish physical UIKit/Mac parity.
Reproduction is in the [Color panel guide](../../tools/visual/README.md#complete-color-panels).

The focused UIKit simulator workflow passes with the current workspace library:
scrolling within the panel, wheel contacts, color-space changes, expression
entry, foreground/background/transparent paint, a latched hue drag exiting
transparency, empty-corner rejection and Swap. Both preset ABI checks also pass,
including real GPU brush/eraser pixels and exact Undo. The Mac UI test target
compiles; it was not executed. Both final signed Release apps build and pass
signature verification. Existing native sessions are preserved and both owned
test simulators are removed.

Color captures now record accepted Rust hue/RGBA through opt-in debug-only
accessibility metadata, replacing hard-coded post-touch colors. That improves
the reference but does not resolve the observed HLS discrepancy: HSV passes
853 interior samples within two channel levels; HLS has one of 680 samples at
error 3 against the unchanged tolerance 2. The failed report and complete images
are retained. The wheel renderer was not changed in this milestone. Its HLS
sample failure, AppKit gradient differences and colored tab icon remain open,
along with the full feature/visual/lifecycle and sustained Mac 90 Hz / iPad
120 Hz gates. Save/Load Layout remains excluded.

## Current workspace UIKit workflows and full-editor references — 2026-09-12

Full-editor/control-layout/numeric fixtures on both Apple targets now create an
isolated production workspace library. They verify all three workspace segments
with Illustrator selected. The earlier persistence-disabled fixture omitted
those controls. The numeric workflow now mounts the Brush size tab before using
its independent readout, preserving the invalid-expression value check instead
of querying an inactive panel.

Both focused UIKit workflows pass in a disposable 13-inch iPad simulator on
iOS 26.5: numeric expressions/correction, tab-switch readback, brush-setting
retention, initial and canvas-under-header capture, layer blend and Undo. The
Mac and physical iPad test targets also compile; they were not executed. Existing
Mac/iPad sessions were preserved, and the disposable simulator was removed.
The drawing runtime remains at the preceding signed numeric-control milestone.

The current web reference initially raced workspace ownership and showed a
recovery message. That fixture is rejected and retained locally. Chrome now
waits for ownership before setup actions, stable layout/camera and visible
previews before capture, and rejects application error/status messages. Both
corrected full references pass those checks at 1376×1032 logical points and 2×.

Exact full-image comparison still fails: initial/header scenarios differ at
5.818%/6.367% of pixels, with mean channel errors 1.372/1.451 and maxima 255.
Color controls, icons/text and header/fullscreen differences remain unmasked.
Reproduction is in the [visual guide](../../tools/visual/README.md#full-editor-captures).
This checkpoint establishes current simulator workflows and comparison fixtures,
not physical UIKit/Mac visual or input acceptance. Full feature/lifecycle and
sustained Mac 90 Hz / iPad 120 Hz gates remain open. Save/Load Layout is excluded.

## Shared numeric-control geometry and unit display — 2026-09-12

The shared Apple number control now truncates labels without wrapping or hiding
the formatted value. Spin fields display Rust's unit suffix while idle and group
the value, minus and plus buttons in the shared order and input surface. Both
native text adapters use tabular digits. Slider progress uses the panel/text
color mixture with a straight internal edge, and step buttons use the shared
disabled opacity at limits and when unavailable.

Four complete native/Chrome component pairs cover 120 controls across both Apple
presets, themes, three widths, endpoints and disabled states. All 792 measured
rectangles are within one logical point. Full raw pixel comparisons still fail:
light/dark differing fractions fall from 14.762%/14.880% to 4.862%/5.052%, with
mean channel errors falling from 8.266/8.705 to 3.604/3.746 levels. Text baseline,
rasterization and disabled-text differences remain visible without masks.

Mounted AppKit field-delegate checks pass on both presets through actual editor
actions: expressions with units, invalid-draft preservation, stepping, Escape,
and an external value change during an unfinished draft. These checks exercise
the shared Apple control with its native Mac field and Rust owner; they do not
establish physical UIKit input or pointer delivery. Reproduction is in the
[numeric-control workflow](../../tools/visual/README.md#numeric-editor-controls).
Both signed Release targets build and pass signature verification. All five
shared numeric-policy checks and the pending-edit state checks pass. The active
iPad session is preserved; this milestone adds no new device installation or
hardware-performance result.
Full feature/visual, physical input/lifecycle and sustained Mac 90 Hz / iPad
120 Hz gates remain open. Save/Load Layout remains excluded.

## Targeted GPU execution analysis — 2026-09-12

A short Instruments workflow now exports and analyzes actual GPU execution
intervals for one explicitly selected process. The analyzer resolves XML value
references, filters other processes, unions overlapping Active stages, separates
submission latency and retains missing durations and unmatched encoder identities.
Five new analyzer checks and all twelve existing frame-trace checks pass.

The headless Apple transform/Undo regression passes for both presets under the
profiler. Its 25 MiB capture supplies 321 target-process GPU intervals, including
clear-page and blit work. All 196 retained CPU encoder records have GPU matches;
six extra GPU intervals lack CPU metadata in the retained window. Complete
capture coverage and application-frame timing are not inferred from this result.
Reproduction, measurements and limitations are in
[Apple performance](../../apps/layer-apple/PERFORMANCE.md#short-gpu-execution-captures).

An alternative runtime pass-counter prototype remains unpublished: local Metal
returned a zero end counter for a clear-only drawing pass, failing its strict
coverage check. The normal Apple renderer/recorder and signed builds remain at
the preceding milestone. This checkpoint adds shared analysis tooling, without
a new native feature or physical iPad result. Current Mac 90 Hz / iPad 120 Hz
performance, complete visual/input/lifecycle gates and the full goal remain
open. Raw traces, device details and the prototype stay private and local.
Save/Load Layout remains excluded.

## Shared tool-action button presentation — 2026-09-12

All six Tool Settings actions now use one shared Apple button component.
Checkable actions retain the shared selected state, ordinary actions remain
unselected, and disabled controls apply one opacity step to the whole button.
Labels use shared bold typography, full column width, centered 24-point line
boxes and greedy word wrapping. This replaces native checkboxes/switches and
bordered buttons while preserving the existing `store.invoke` command route.
The same component compiles into both Apple targets.

Production AppKit and browser component captures cover light/dark themes,
120/226-point columns and all four enabled/selected combinations. All 96
measured control bounds match exactly. Full-image mean absolute channel errors
fall from 13.581/7.679/13.599/7.571 to 0.947/0.365/0.994/0.409 respectively.
Zero-tolerance comparisons still fail, with 8.202–13.028% differing pixels and
maximum channel errors of 106–173; no pixels are excluded. Reproduction and
the complete table are in the [visual guide](../../tools/visual/README.md#tool-action-buttons).
These invisible AppKit captures establish component geometry and appearance
evidence, not UIKit rendering or native activation/accessibility acceptance.

The integrated shared revision `9d747b0` includes the Windows startup-close fix
and material-pipeline specialization. All 276 UI and 39 Apple ABI tests pass
after integration, including ruler settings and transform Apply/Cancel with
exact pixel Undo/Redo. The incoming specialized-versus-uniform material check
also passes on Metal across pages and prediction. Those checks exercise editor
effects; they do not simulate SwiftUI clicks. Both signed Release targets build
and pass signature verification. No new physical install, input or performance
result is inferred. Save/Load Layout remains excluded, and full parity and
hardware-performance gates remain open. Raw captures, logs and private build
metadata stay in ignored local artifacts.

## Complete shipped property schemas and opacity lock policy — 2026-09-12

The schema 4 inventory enumerates paint, paper, groups and all forty shipped
filters on both Apple presets: 43 property scenarios and 160 editable fields.
Each field changes through its shared action, restores all properties with
Undo/Redo and resets to its declared default. Lockable targets retain the
disabled schema and reject a sampled stale edit. Paper has no Lock action and
retains editable opacity. The audit checks all six property kinds and rejects
missing scenarios, routes, handlers and contradictory history/lock observations.

These checks reproduced a shared bug: the Properties opacity route bypassed
the validation used by the layer header, allowing changes to locked paint
layers and groups. Both routes now use one shared validator, including inherited
locks and finite 0–1 bounds. Rejected edits preserve the document revision and
opacity; paper opacity and Undo remain supported. The regression fails before
the change and passes afterward. All 274 shared UI tests pass.

The direct Swift check replays all 160 edit/Undo/Redo/Reset routes and compares
all 43 schemas through the actual Apple editor and serial owner on both presets.
It uses disposable storage without windows or GPU attachment. Native UIKit
widgets, full document-state/property visuals and physical interaction remain
separate requirements. Reproduction is in the [Apple README](../../apps/layer-apple/README.md).
Both signed Release builds pass. The two focused Apple ABI property checks also
pass, including the existing Metal number-edit pixel and exact Undo regression.
Ten evidence-corruption probes per preset reject incomplete or contradictory
property inventories. Raw inventories, logs and signing information stay local
and ignored.
Save/Load Layout remains excluded; full performance and parity gates remain open.

## Shared editor control colors — 2026-09-12

Shared icon buttons now use the editor accent rather than the Mac system accent.
Icon, toolbar and tool-choice buttons use one shared style and apply disabled
opacity to the whole control once, including its selected background. This
removes extra native glyph dimming and fully opaque selection on disabled tiles.
Layer drop markers and resize highlights also use the shared accent; the main
editor tint references the same color definition on both Apple platforms.

Real SwiftUI and browser widget factories are compared in light and dark themes,
with all four enabled/selected combinations and three native accent environments.
The old controls fail the focused color check by up to 44/45 RGB levels and
change with the system accent. Corrected complete native rows are identical
across accents; flat fill samples match Chrome exactly in dark mode and within
one RGB level in light mode. Both fixtures contain 24 controls at 2× scale.

Full raw comparisons remain failures and retain every pixel. Light/dark mean
absolute channel errors fall from 5.028/4.330 to 0.354/0.236 levels. Exact
differing-pixel fractions are still 35.517%/3.571%; the light result includes
one-level fill rounding, and glyph/edge rasterization differences remain.
This is AppKit component evidence for shared code, not physical UIKit capture
or native interaction acceptance. Full visual, feature, physical input/lifecycle
and sustained performance gates remain open for both platforms.

Reproduction and the focused check's narrow scope are documented in
[editor control colors](../../tools/visual/README.md#editor-control-colors).
Raw fixtures, captures, comparisons and build/signing logs remain local and
ignored. Both signed Release builds pass, and the updated iPad validation app
installs on the attached device. Save/Load Layout remains excluded.

## Expanded feature inventory and workspace menu actions — 2026-09-12

The schema 3 inventory starts from the settled default drawing and task workspace
with synthetic managed identities. This exposes production workspace service
routes that bare host creation omitted. It records all 62 commands and their
initial dispatch results, all three task workspaces and 133 panel/group/tile/
ribbon/Zen context menus per Apple preset. Following the rendered tool-choice
graph with a hardware renderer and disposable filled drawing resolves 80 tool
choices and 28 setting IDs per preset, including transform controls.

The Apple review now covers nine workspace service commands, 14 panel control
types and six preference kinds, with implementation/check references. The audit
passes for both presets. Negative probes reject missing brushes, unresolved
tools, unknown panel controls, missing/duplicate workspace reviews and rejected
initially enabled commands. Save/Load Layout stays excluded; the inventory
continues to expose the existing unavailable iPad Full Screen capability.

Direct Swift checks dispatch actual shared menu payloads through the Apple
editor and workspace coordinator. All nine routes pass on both presets,
including form cancellation, manager/history dismissal, request acknowledgement,
switch/return, history timestamp persistence and document preservation. Existing
confirmed-form and coordinator workflows also pass on both presets. These checks
use isolated temporary storage, without OS menu automation or visible windows;
they exercise shared Apple code on macOS, not native UIKit widgets.
Both signed Release targets build successfully.
The integrated Windows milestone `ba9849a` adds a shared filter-readiness query
unused by Apple; all 273 shared UI tests and the inventory audit pass afterward.

Reproduction is documented in the [Apple README](../../apps/layer-apple/README.md).
The new inventory is a review aid, not complete UI acceptance: remaining dynamic
document/filter states, native workflows and widgets, full visual parity,
physical input/lifecycle coverage and sustained hardware performance remain
open on both platforms. This milestone changes validation tooling and records;
it adds no new drawing-runtime or hardware-performance evidence. Raw generated
inventories and logs stay in ignored local artifacts.

## Shared Apple and web header alignment — 2026-09-12

Both Apple targets use shared six-point menu/title/clock padding, fractional
native font advances and natural workspace-label widths. The task switcher is
a 34-point capsule with capped, truncated labels and the shared accent color.
Clock and battery backgrounds stay transparent over the canvas. Compact iPad
headers hide the title and retain all eight menus through the overflow control
when necessary; Mac retains OS menus and its window-control reservation.

The integrated web workspace header had a general button-height rule overriding
its compact cells. It now follows the shared capsule dimensions and ellipsizes
the actual text. A narrow menu overflow uses the existing recursive menu/action
renderer, closes obsolete popups during resize and transfers keyboard focus to
the visible menus. It preserves all top-level menus and their actions. Save/Load
Layout remains excluded under the user's revised scope.

Ninety-six real SwiftUI component captures cover both Apple presets, the three
task workspaces, 744/1200-point widths, both themes, clock visibility and two
backgrounds. Matching live Chrome captures use the real workspace switcher and
status component, with explicit Mac OS-menu/window-space and Apple fullscreen
adaptations. Measured control-edge error is at most 0.991 points; the separate
position/size report retains a 1.221-point maximum title-width difference from
accumulated font advances. Full raw pixel comparisons still fail exact equality;
font rendering, truncation, blending and remaining geometry differences are
retained without masks, rescaling or tolerance waivers. These invisible AppKit
captures establish shared-component evidence, not UIKit or Metal pixel parity.

Both final signed Release targets build, and the iPad build installs and launches.
All 385 integrated Rust checks pass (one existing hardware-only check ignored).
The actual web header workflow passes all eight overflow menus, Zoom In, resize
closure, restored menus, focus and capsule geometry. The web manager workflow
passes creation, rename, filtering/preview/cancel, switching, tools, history,
undo, restart and reset actions. Its text helper selects the input directly
before browser text insertion to avoid a reproduced headless Mac Chrome OS-key
routing stall. The fast native clock/battery lifecycle check also passes.

The final builds complete isolated three-second ink smoke checks after ten
seconds of warm-up on the physical Mac and iPad, with no frame errors, rejected
input or missing presentation callbacks. CPU p99 is 3.00 ms on Mac and 5.15 ms
on iPad. Mac has zero CPU frames over 11.11 ms and two long active presentation
intervals; iPad has three CPU frames over 8.33 ms and four long intervals.
The Mac measurement is 0.10 ms shorter than requested. One Mac and two iPad
zero-time callbacks occur outside the measured intervals. GPU timing is disabled;
these are startup/drawing checks, not sustained performance acceptance. Both
owned validation apps are closed. The later `42fa3c3` integration changes only
Windows's New Window capability in shared code; physical and component checks
precede it, with integrated builds and the affected UI suite checked afterward.

Reproduction and limitations are in the
[header comparison workflow](../../tools/visual/README.md#complete-header-components).
Raw captures, logs and signing/device metadata stay in ignored local artifacts.
Full visual, input, lifecycle and sustained performance acceptance remains open
on both platforms; this milestone does not close those gates.

## Retained live panel resizing — 2026-09-12

Both Apple apps now consume the shared layout-aware incremental publication.
Divider and floating-panel resizing stage resolved layout, workspace dimensions,
camera and measurements together while retaining control models and content
revision. Ordinary translation keeps its smaller placement packet. Complete
models still publish at release, cancellation, content changes and unsupported
reflow boundaries, preserving workspace persistence and history. Legacy bridge
requests retain their existing schemas.

The ABI compares independent compatibility and incremental sessions on both
Apple presets, including 16 intermediate moves for each resize/cancel case.
Geometry, camera and measurements match exactly; wire traffic drops by roughly
89–90%. Direct SwiftUI checks retain the Navigator identity through 64 resize
moves and 24 translations per preset, with coherent hit regions and tab clips,
final-position completion, cancellation and single-step Undo/Redo. Intrinsic
panel measurements, natural sizing, width reflow and settled publication also
pass. Projection checks reject incomplete and stale packets without mutation.
The view checks use invisible AppKit hosts for both presets, so UIKit pixels and
physical resize cadence remain separate acceptance work.

Shared changes through `21a786f` are integrated. Both signed Release builds and
385 Rust checks pass, with one existing hardware-only host check ignored.
The direct view and hardware smoke checks precede that additive shared workspace
controller integration; this checkpoint retains Apple's existing coordinator.
Short synthetic drawing smoke checks complete on the physical Mac and iPad,
with no renderer errors, rejected input or missing presentation callbacks.
Their configured one-second measurements follow ten seconds of warm-up; the
Mac report retains a 0.10 ms duration shortfall and both retain unmeasured GPU
work and startup zero-time callbacks. These are startup/drawing checks, not
sustained performance acceptance. Validation apps are closed. Raw evidence stays
in ignored `artifacts/apple-retained-reflow-*`; complete visual, physical input,
lifecycle and performance gates remain open.

## Task workspaces and native panel measurements — 2026-09-12

The Apple apps follow the revised shared design through `761b26b`: no Save/Load
Layout menus, manager page or Apple apply/save operations. Existing stored records
are preserved for the shared migration. New Workspace copies the current layout
and tool settings into independent history. Painter, Illustrator and Photographer
use shared seeded identities and appear beside the document title. Their renamed
labels and selected state follow storage; switching to an owned workspace focuses
its existing window. Reset All Brushes uses the shared confirmation and reset,
flushes the resulting working values, and preserves layout history and artwork.

Mounted SwiftUI bodies and lightweight tab labels now report intrinsic sizes to
Rust. Floating panels follow natural content height, including visibility changes
and added layers, while explicit resizing retains its chosen height. Tab groups
fit measured labels. Cached transient measurements revalidate after restoration;
settled facts stop publishing. Drawer measurements remain separate, and label
measurement mounts no extra Navigator, filter or thumbnail content.

Direct native checks cover both platform presets: manager actions and prompts,
new-workspace copying, reset cancellation/confirmation, persisted reset values,
independent workspaces, renamed default labels and focusing an existing owner;
coordinator migration, saves, ownership and restart; panel measurement, width
reflow, fitted tabs, control visibility, Undo and settled publication. These use
invisible AppKit hosts. Native component captures cover the three task workspaces
at narrow/wide widths in both themes; they exclude Metal and do not establish
UIKit or Chrome pixel parity. Live resizing with retained content and full-editor
visual validation remain open.

The final integrated Rust suite passes 376 checks (38 Apple, 24 host, 273 UI and
41 workspace; one existing hardware check ignored). Both signed Release targets
build successfully. Coordinator migration/restart checks pass again after the
final seed-migration integration. Twenty-four final AppKit editor-component
captures cover both Apple presets, all three task workspaces, two widths and two
themes; sixteen manager captures cover workspace/history pages before the last
style-only update. These are component-layout evidence with the limitations above.

Physical Mac/iPad drawing smoke checks completed with a one-second measured ink
interval after ten seconds of warmup, with zero frame errors, rejected input or
missing presentation callbacks. During the measured interval, Mac had two CPU
frames over 11.11 ms and three long presentation intervals; iPad had no CPU frames
over 8.33 ms or long intervals. One Mac and three iPad zero-time callbacks occurred
outside the measured interval. These checks preceded the final pill-color,
Photographer tile-size and seed-migration adjustment. They are startup/drawing
integration evidence, not sustained performance acceptance. Both owned validation
apps were closed afterward. Full parity, input, lifecycle and performance gates
remain open on both platforms.

## Shared Apple workspace library and live previews — 2026-09-12

The workspace library is enabled by default on both Apple targets. Shared Swift
manager pages use Rust's current compact workspace/saved-layout and toolbar actions.
The integrated shared changes through `afbc696` include Load Layout on the current
workspace and temporary live-editor previews for selected workspaces, saved
layouts and history. Loading a saved layout preserves the workspace identity and
working brush/color state; history Cancel leaves durable
capture untouched and Restore creates one undoable layout change. The main
manager follows the revised shared design without storage administration or
backup/version controls. Storage transport and retained-history capabilities
remain covered by direct service checks.

The coordinator covers legacy migration without dual writes, per-scene
restoration beside a live owner, latest-edit switching, failed-switch unlock,
workspace/saved-layout/toolbar/history operations and native package delivery. A
SQLite write lock leaves drawing-owner edits and queries responsive; edits
accepted during a blocked save survive its acknowledgement. Competing-owner
recovery preserves in-memory changes. Close waits for an acknowledged lease
release, canceled close revalidates editing, discarded iPad scenes retire their
claim, and explicit teardown preserves the last saved data. Read-only ownership
blocks new contacts and shortcuts while allowing existing ink to finish.

All 367 integrated Rust checks pass (38 Apple, 24 host, 269 UI, 36 workspace;
one existing hardware host check ignored), plus 14 Windows workspace checks.
The Windows storage-worker integration preserves both ports' settings policy;
Apple coordinator checks pass on both presets after that integration.
The subsequent Android/Web drawer integration passes the shared suite and
the direct native Mac drawer geometry/action check.
The actual Swift manager workflows
pass on both Apple presets, including held-contact metadata edits, saved-layout
application, selected-row preview without adoption or persistence, preview
cancellation and history restoration, local toolbar prompts, window
ownership and package delivery. Approved defaults, saved-layout naming, filtering
away a preview, rapid selection and dismissal while selection is pending are
covered directly. Twenty-four AppKit component captures cover both presets,
two themes, narrow/wide manager widths and workspace/layout/history states.
They establish component layout evidence, not UIKit or full-editor pixel parity.
Both final integrated Release targets build with signing enabled.

The focused iPad Simulator history check passes after its Debug fixture waits
for bundled-filter and document readiness. Both signed physical apps completed
an isolated ink smoke check and postlude with no renderer errors, rejected input
or missing presentations. Their one-second measured intervals have no zero-time
presentations; each app reports one outside that interval. These are launch/ink
checks, not sustained performance acceptance. The owned validation apps are
closed and the artist's original apps remain running. The final Mac smoke
interval has six CPU frames over its 11.11 ms budget; sustained performance
remains unaccepted. Physical checks include the selected-row preview integration
and precede the final approved selection defaults, captions and Windows worker
and Android/Web drawer integrations. Direct workflows and the focused Simulator check cover the dialog
adjustments; final signed builds, the shared suite and direct coordinator checks
cover the integrated worker. Full physical
lifecycle/input coverage, full visual parity and sustained 90 Hz Mac / 120 Hz
iPad acceptance remain open.
See [Apple persistence](../../apps/layer-apple/PERSISTENCE.md#workspace-library)
for implementation boundaries and reproduction commands. Raw logs, captures and
private platform details stay in ignored artifacts.

## Incremental Apple workspace publication — 2026-09-11

Both Apple targets now consume the shared incremental `workspace_update`
publication. Full snapshots establish retained models; ordinary motion updates
only absolute group placement, tab previews and drop hints. Native hit areas,
tab clips, resize handles and live Navigator allocations move with the panel.
Tab visibility is observed separately from its position. Camera patches preserve
the retained model revision, and camera-less motion preserves the readout.
Stale or mismatched presentations are rejected before any related state changes.
Every input phase still reaches Rust, which owns completion, history and durable
persistence. The previous C snapshot request remains available for compatibility.

The paired ABI fixture gives separate legacy/incremental owners identical
actions on both presets. Full snapshots match exactly after removing the new
field. Across 32 floating moves, serialized output falls from roughly 2.68 MB to
5.4 KB; each new position matches the legacy layout. Intermediate motion carries
no durable persistence, and cancellation, a newer release point and workspace
Undo/Redo retain the same result. This wire-size reduction does not establish a
CPU/GPU or presentation-cadence improvement.

Direct observation checks cover matching/missing/older revisions, atomic
completion, camera-bearing and camera-less packets, retained fields and 40
existing exact wire fixtures. An invisible SwiftUI probe renders ten movements
without rebuilding unrelated controls/readers. The real shared workspace check
performs 24 floating moves per Apple preset, preserving Navigator identity and
checking native hit, tab, clip, resize and overview allocations plus history and
a camera action. All eight native tab workflows pass. All 16 final before/drag
tab captures match the preceding native milestone exactly, with no masks,
resampling or relaxed tolerances. The previously recorded Chrome differences
therefore remain; no full-editor or UIKit pixel-parity claim is added.

The integrated tree includes shared changes through `c99595a` and subsequent
Android/GTK-only changes through `5619a3a`. All 316 Rust
checks pass (36 Apple, 20 host, 260 UI; one existing hardware-only host check is
ignored). Both Release targets build, the focused iPad Simulator touch drawer
workflow passes, and direct document checks pass for both Apple presets. The
signed iPad validation app installs. Both final physical apps complete a brief
isolated ink smoke check and its postlude with no renderer errors or rejected
input. Their one-second measured intervals have no missing or zero-time
presentations; two Mac and four iPad zero-time reports occur outside those
intervals. These are startup/drawing checks, not sustained performance acceptance.
Owned validation editors are closed and the artist's original apps are preserved.

The [Apple README](../../apps/layer-apple/README.md) and
[performance guide](../../apps/layer-apple/PERFORMANCE.md#incremental-workspace-publication)
document the protocol and reproduction commands. Raw evidence stays in ignored
artifacts. Full feature and visual parity, automatic panel fitting, incoming
workspace-management/long-press flows, physical input/lifecycle coverage,
filter references and sustained 90 Hz Mac / 120 Hz iPad acceptance remain open.
Mac 120 Hz remains deferred.

## Shared Apple tab dragging — 2026-09-11

Both Apple editors use the shared frozen tab-drag policy. Native headers capture
complete natural tab widths and the visible strip before dragging; Rust returns
the source position, neighbor offsets, insertion target and final release slot.
Visual copies move independently of the original input rectangles. Neighbors
use the web editor's 120 ms easing and respect Reduce Motion; the source follows
without easing. Owner acknowledgements retire completed previews, and gesture
tokens prevent an earlier cancellation from clearing a newer drag. Continuous
motion retains the latest completed preview while a newer query is in flight.

Docked and floating headers now scroll their natural-width labels, matching the
drawer behavior. Icon-only tabs are 36 points wide. Selected tabs have the shared
upper corners and concave lower shoulders; drawer and Navigator headers use the
reference background. Visual copies retain their own rounded backgrounds and
remain below the docking indicator, whose color and shape now match the shared
reference. No platform-specific insertion algorithm is introduced.

The extended workflow reproduced a drawer observation bug: quick Undo/Redo and
reopening could preserve a drawer view with disabled gesture preferences. Its
interaction state is now read inside the view that observes that drawer. Direct
test stores also inject disabled persistence explicitly. Their previous
environment switch was Debug-only and did not isolate standalone Swift binaries
from earlier standalone-test preferences.

The direct SwiftUI workflow covers both Apple presets, both themes and docked
and drawer tabs: clipped natural geometry, unchanged hit rectangles during
animation, halfway reversal, release between move events, exact workspace
Undo/Redo, cancellation/restart and icon-only widths. It renders the actual
compiled vector assets in an invisible AppKit host. Matching Chrome fixtures use
the live web editor, real pointer events and committed workspace state. These
component captures supplement physical and full-editor acceptance; an AppKit
rendering of the iPad preset does not establish UIKit pixel parity. The focused
iPad Simulator drawer workflow passed real touch reordering, tear-off, docking
back and moving the whole group. It precedes the final cosmetic changes.

Final integrated validation passes 315 Rust checks (35 Apple, 20 host, 260 UI;
one existing hardware-only host check remains ignored), all eight direct SwiftUI
tab cases and all eight Chrome pointer cases. The existing direct drawer,
document and window-presentation checks also pass with explicit isolated storage.
Both final Release targets build. The signed iPad app installs and both final
apps complete a brief isolated ink drawing smoke check, including its postlude,
with no renderer errors or rejected input. The one-second measured intervals
have no missing or zero-time presentations. These checks establish startup and
drawing, not sustained performance acceptance. Owned smoke editors are closed;
the artist's existing Mac and iPad app processes are preserved.

All 16 final before/drag tab-strip comparisons have identical clip rectangles
and a maximum native/Chrome tab-rectangle difference of 0.6875 logical pixels.
Exact sRGB comparisons still fail: 6.8585–8.0605% of pixels differ across the
cases, including text, subpixel geometry and edge rasterization. No pixel masks,
resampling or relaxed acceptance threshold are used. Full raw differences,
overlays, heatmaps and per-case reports remain in ignored artifacts; these
component results do not close the full-editor visual gate.

Incoming shared changes through `c3840f5` are integrated. Apple uses its shared
`BeginTabDrag` action. The compatibility snapshot transport remains in use;
migration to incremental `workspace_update` publication remains a performance
task. The final integrated Release builds, direct interaction checks and isolated
ink smoke checks are repeated after this last merge. Automatic native panel
measurement/fitting remains a parity gap; the clipped-tab comparison deliberately
uses an explicitly resized column in both hosts. Incoming workspace-management
and long-press continuation workflows still require Apple implementation and
acceptance. Full UI/visual parity, physical input/lifecycle coverage, filter
references and sustained 90 Hz Mac / 120 Hz iPad performance remain open.
Mac 120 Hz is deferred. Raw logs, captures and private platform details stay in
ignored artifacts. Reproduction commands are in the
[tab comparison guide](../../tools/visual/README.md#workspace-tabs).

## Shared native snapshot observation — 2026-09-11

Both Apple editors now observe individual snapshot fields and indexed command,
panel and menu entries. Required stroke-boundary enablement changes still reach
the controls, while unrelated SwiftUI readers avoid a whole-editor publication.
The iPad's in-app menu array is read inside its own view, keeping menu changes
local to that header component. Camera patches update the canonical state;
document and lifecycle consumers explicitly take immutable copies. Related
projections update together before notifications are sent. Transport values and
editor actions remain owned by Rust. The shared JSON reader also avoids bridging
a decoded Foundation dictionary in full for every field access.

The integrated 40 transport fixtures match both the value/byte APIs and the
preceding checkpoint exactly. Recursive JSON checks cover 219,911 values;
observation checks preserve those fixtures and 14 hardware-backed stroke
snapshots. An invisible SwiftUI host verifies actual rendered changes, unchanged
unrelated bodies, retained readers and camera/no-op behavior. Direct shared
drawer, document and window-presentation workflows pass without system menu
automation. Both final Release targets build, and the signed app installs on the
physical iPad. All 310 final Rust checks pass (34 Apple, 18 host, 258 UI;
one existing hardware-only host check remains ignored). The command inventory
still covers 62 commands, 11 panels and five settings pages for each Apple host;
this does not imply complete workflow acceptance.

Shared changes through `9019e23` are included in those builds and checks. Later
Android presentation and independent workspace-storage additions through
`13d139c` are integrated too, with no change to Apple source or its existing
dependency lock entries. The incoming workspace manager and tab-drag animation
behavior still require Apple parity work.

The final synchronization also includes shared tab-drag and workspace-transition
changes through `f6c58a7`. Both Release targets rebuild successfully; the direct
drawer workflow, all 40 exact value/byte snapshot pairs and the 54-fixture Swift
observation check pass again. Both final apps complete a brief 4K drawing smoke
check with no rejected input, renderer errors or missing/zero-time presentations
in its measured interval. These final checks establish startup and drawing;
the longer hardware measurements in the performance guide precede this last merge.

Local profiles show reduced UI graph work in an intermediate candidate but also
exposed the cost of comparing unread fields, which the final implementation
avoids. Short physical measurements do not establish a frame-rate improvement
or close sustained performance acceptance. The
[performance guide](../../apps/layer-apple/PERFORMANCE.md#selective-native-ui-observation)
records the measured versions, cadence gaps and limitations. Current targets
remain **90 Hz on Mac and 120 Hz on iPad**; Mac 120 Hz is explicitly deferred.
Full UI/visual parity, physical input/lifecycle coverage, isolated GPU timing,
physical input latency, the complete sustained workload matrix and existing
filter-reference failures remain open. Raw traces, captures, profiles and device
details stay in ignored artifacts; completed benchmark editors are closed while
the artist's original Mac window is preserved.

## Shared native snapshot transport — 2026-09-11

Both Apple targets now serialize full UI snapshots directly from the shared
host models to UTF-8, avoiding the temporary JSON tree on the serial render
owner. The existing value API and new byte API share one schema and publication
policy. Camera-only updates remain small patches, workspace persistence is sent
only when needed, and failed serialization leaves updates pending. Direct
floating-point formatting preserves the old exact numeric values.

Forty snapshots captured before the refactor match the new complete decoded
payloads exactly. Direct regressions cover all four current native host presets,
unchanged-state suppression, camera updates, errors, document adoption and
serialization failure. A real shared SwiftUI drawer check also passes without
XCTest or a visible editor. The CPU transport benchmark runs on the development
Mac with both Apple presets: median construction/encoding/destruction falls
from about 0.51 ms to 0.17 ms. This excludes Swift decoding and rendering.

Short physical 4K watercolor pairs show reduced owner time outside the measured
Rust stages on both devices. Mac CPU p99 falls from 6.399 to 5.508 ms. The iPad
pair has a worse drawable-acquisition tail and overall CPU p99 rises from 7.713
to 9.057 ms. Presentation gaps remain on both platforms. Each measured interval
has zero rejected input batches, renderer errors, overflow, missing callbacks
or zero-time presentations. These are twenty-second observations with GPU
timing disabled, not sustained acceptance or physical-input latency evidence.
The [performance guide](../../apps/layer-apple/PERFORMANCE.md#snapshot-transport)
retains the full scope, limitations and reproduction commands.

Incoming Android header, Windows workspace and GTK/shared sidebar fixes through
`6eb0418` are integrated. Both final Apple Release builds, all 34 Apple bridge,
18 host and 250 shared UI checks pass (302 total; one existing hardware-only
host check remains ignored). All 40 final value/byte payload pairs match. The
original reference differs only by the incoming shared View menu's removal of
the theme-toggle item; every other field and value is preserved. The direct
SwiftUI check passes through `c0596bb`. The command audit retains all 62 commands,
including the theme toggle, and the integrated
signed app installs and is observed running on the physical iPad before its
isolated process is closed. This last check establishes startup only.
The recorded hardware pairs precede this integration. Capture
bundles, traces, account/device details and build logs stay in ignored local
artifacts. Completed synthetic benchmark editors are closed; the original Mac
editor is preserved. Full UI/visual parity, input/lifecycle coverage, existing
filter-reference failures and sustained 90 Hz Mac / 120 Hz iPad performance
remain open; Mac 120 Hz remains deferred.

## Apple drawer docking and shared layout geometry — 2026-09-11

Both Apple editors now use one tab/header/grip implementation for docked panels,
floating groups and collapsed-column drawers. Drawer tabs support reordering,
individual tear-off and docking into another open drawer; the grip and unused
header space move the complete group. The same panel/group context actions and
configured tab styles apply in every presentation. SwiftUI reports clipped tab
rectangles and actual drawer bounds to Rust. New drags restore cached measurements
after transient invalidation; closing projections retire their input targets
before the exit animation ends. Drop indicators draw above the open drawers.

Two shared geometry bugs were reproduced and fixed. Dictionary-order tab
measurements could select the wrong insertion slot; the resolver now selects
the first logical slot independently of measurement order. A floating tabbed
toolbar's uniform-grid height estimate could clip the last tool column when
dividers affected wrapping. Its height now comes from the same allocator as the
toolbar body, with regression coverage for all five tile styles. Both fixes
apply to every port. Collapsing a column without a measurable width also rejects
the action before mutating the layout.

Debug startup actions previously ran against the native owner's 1×1 placeholder,
allowing collapsed-column fixtures to record an invalid expanded width. The
serial owner now applies these actions once, after restoration and its first
surface size. Release builds have no fixture override.

Focused Mac and iPad Simulator workflows pass tab reorder, tear-off, docking back
into the drawer and floating the complete group. Their final app captures show
the complete toolbar after the sizing fix. They exercise in-app editor gestures
without automating system menus. These captures precede the final incoming
tab-visibility change and are workflow evidence, not paired pixel comparisons.
The faster invisible AppKit hosting check covers actual shared SwiftUI tab/header
sources, serialized reordering, restoring an invalidated drop target and closing
input retirement without XCTest or GPU rendering.

Incoming GTK, web and Android drawer changes, including preservation of panel
tab-visibility preferences through `e1b1fe0`, are integrated. Shared bridge checks
exercise both Apple platform projections, drawer bounds, cancel, undo/redo and
preservation of brush/document state. Validation details for this integrated
checkpoint include 34 Apple bridge, 15 host and 249 shared UI checks (298 passing;
one existing hardware-only host check remains ignored), the direct SwiftUI check
and both Apple Release builds. The command audit retains all 62 commands in 14
groups for both platforms. The signed app installs and is observed running on
the physical iPad, then the isolated test process is closed; this establishes
startup only.

The integrated web build and headless Chrome drawer workflow also pass for mouse
and touch: active/inactive tabs, group grips and unused header space, reordering,
insertion, merging, top/bottom splits, clipped tab hits, cancel and undo/redo.
The browser runner uses the installed macOS Chrome executable and closes its
temporary browser and server afterward.

Local evidence stays in ignored `artifacts/apple-drawer-*` logs, captures and
result bundles. The original Mac editor is preserved. This checkpoint adds no
new physical Pencil, pixel-parity, latency or sustained-performance assertion.
Full feature/lifecycle coverage, existing visual/filter-reference failures and
the 90 Hz Mac / 120 Hz iPad hardware targets remain open. Mac 120 Hz testing
remains explicitly deferred.

## Apple toolbar rendering and Zen accessibility — 2026-09-11

Both Apple editors use one SwiftUI toolbar button/content implementation for
ribbons, floating panels, drawers and Zen strips. All five shared styles consume
Rust icon sizes, label line counts and font weight. Labeled tiles place text
beside the icon, size tiles use the shared glyph, and vertical bars draw horizontal
separators. A custom button style removes the extra native disabled dimming
before the shared 0.36 opacity and retains pressed feedback. Zen containers now
preserve their individual buttons' accessibility identifiers.

Direct bridge checks pass all five styles on both hosts, ribbon/drawer geometry,
Zen style projection and workspace undo/redo without changing brush or artwork.
One focused native editor workflow passes on Mac and iPad Simulator: 108×54
medium-labeled bounds, Zoom's effect on the camera readout, selecting 54×54
Medium Tiles through the in-app toolbar menu, and Zen hide/restore. These checks
exercise editor behavior and do not automate the OS menu bar.

The new repeatable component capture uses actual shared SwiftUI buttons and
compiled vector assets, rendered directly without an editor window or XCTest.
Chrome uses its real toolbar factory, CSS and SVGs with the same Rust-generated
panel views. The matrix contains seven selected/disabled, command, preset,
color, opacity, size and wrapping-label controls in each of five styles at 2×.
The displayed fixture fields agree across Mac, iPad and web projections; browser
tile, icon and label-column geometry checks pass.

Exact sRGB comparisons still fail: light differs at 190,902/1,010,880 pixels
(18.8847%) and dark at 17,949/1,010,880 (1.7756%). Most light differences are a
one-byte red-channel difference in the selected background. Pixels differing by
more than one byte number 15,353 light and 15,902 dark; text and edge differences
remain. The reports retain every pixel with no masks or relaxed acceptance
threshold. Mac ImageRenderer output does not establish physical iPad
rasterization or full-editor parity. The Mac workflow screenshot includes a
Zen tooltip and is not used as a clean full-editor pixel reference.

Concurrent changes through `fd1ade7`, including shared medium-style support,
column header actions and GTK drawer dragging, are integrated. Both Apple Release
builds pass, as do 33 Apple bridge, 15 host and 246 shared UI checks (294 total;
one existing hardware-only host check remains ignored). The command audit still
classifies all 62 commands on both hosts. The Mac workflow precedes `930ea78`;
the iPad Simulator workflow and web build follow it. Both workflows precede the
final GTK/shared drawer integration; both Apple builds and shared checks follow
that integration. The component capture passes again with identical pixels.
A separate
1200×870 Chrome initial-editor capture also completes with WebGPU ready and no
runtime errors, validating the existing full-editor capture path without
claiming a new paired full-editor comparison.

The integrated signed app installs and is observed running on the physical iPad;
this verifies startup only. Test processes are closed and the original Mac editor
is preserved. Reproduction commands are in the
[visual tools guide](../../tools/visual/README.md#toolbar-components). Raw captures,
pixel reports, result bundles and device/signing logs remain in ignored local
`artifacts/ui/toolbar-parity` and `artifacts/apple-toolbar-*` paths.

This milestone adds no new hardware timing or physical Pencil evidence. Full
feature/lifecycle coverage, full-editor visual differences, filter-reference
failures, physical latency and sustained 90 Hz Mac / 120 Hz iPad performance
remain open. Mac 120 Hz testing remains explicitly deferred.

## Apple system status and current refresh target — 2026-09-11

Both Apple editors now expose and implement Appearance's **Show battery and
clock** preference. The shared catalog previously excluded this setting for
Apple hosts. One Swift component supplies the clock and browser/Android battery
geometry, with UIKit battery monitoring on iPad and IOKit power notifications on
Mac. Unknown readings omit the battery; the development desktop shows the clock
alone. The title truncates when necessary to leave space for the trailing controls.

The clock refreshes on minute boundaries and system time/locale changes.
Visible windows share one native subscription; the last hidden/background
header releases it. Power-service reads run away from the UI/render queues.
Neither minute updates nor battery callbacks enter the Rust owner or drawing
display link. AppKit observations and UIKit scene geometry report actual
full-screen state for the preference. The iPad full-screen toggle remains
unavailable; this observation does not implement that command.

Direct Swift checks pass visibility policy, battery normalization and unknown
readings, minute scheduling, shared window lifetime, clock changes, stopping
hidden work and rejecting retired subscriptions' replies. The shared settings
test now covers Apple selection, serialization, reset and invalid choices.
Focused Mac and iPad Simulator workflows pass Always/Never preference effects,
Zen hide/restore and title/clock placement. Each captures its complete editor;
these tests neither exercise system menu mechanics nor establish physical
battery/input or full-screen transition acceptance.

A local component comparison uses the actual browser status module/CSS and
SwiftUI battery view at 2× scale, with seven empty/low/normal/full/charging
readings in both themes. Tile and battery placement agree; exact sRGB comparisons
still fail at 3,087/57,600 light pixels (5.3594%) and 3,533/57,600 dark pixels
(6.1337%). Text and edge rasterization differences are retained without masks or
relaxed thresholds. These component fixtures do not supersede the older failed
full-editor comparisons or validate physical iPad rasterization.

Other ports' changes through `9f32a5f` are integrated. Both Apple Release builds,
32 Apple bridge checks, 15 host checks and 243 shared UI checks pass; the existing
hardware-only host check remains ignored. The command inventory still classifies
all 62 commands on both hosts. Native status checks and all twelve trace analyzer
checks pass. Raw result bundles, images, local component harnesses, logs and
signing/device information stay in ignored `artifacts/apple-status-*` and
`artifacts/ui/system-status` paths.
The signed build is installed in the separate physical iPad test app and was
observed running after launch. This establishes startup only; physical battery changes,
scene transitions and Pencil behavior were not revalidated in this checkpoint.
Test apps and extra Simulator windows are closed; the original Mac editor is preserved.

Mac reports now accept `--target-hz 90` and evaluate the 11.11 ms frame period;
iPad remains at 120 Hz/8.33 ms. Existing 120 Hz diagnostics remain explicitly
labelled. Reanalysis of the original Mac ten-minute trace reports eight CPU
owner-service overruns and 746 long continuous presentation intervals out of
48,890, with presentation p99 22.222 ms. This is new analysis of existing data,
not a fresh sustained run. Mac 120 Hz work is deferred by the user's hardware
decision; the remaining 90 Hz gaps, full workload matrix, physical latency,
feature/lifecycle coverage and visual parity remain open.

## Presentation measurement checkpoint — 2026-09-11

The CAMetalDisplayLink trial was not adopted. Across short physical 4K
watercolor runs it reduced iPad CPU time but produced 49–57 skipped drawable
presentations per measured twenty-second interval. Both requested rendering
windows and a run without GPU instrumentation retained the problem. An explicit
Core Animation transaction trial also caused UIKit layout on the render thread
and was removed before those completed comparisons. The published app retains
its existing CADisplayLink scheduling on both platforms.

The shared recorder now supports `CAPY_TRACE_GPU=0`, retaining CPU/input/memory
and presentation observations while disabling GPU timestamp submissions and
polls. Metadata and report warnings distinguish this mode; absent GPU durations
remain null. The analyzer separately measures frame admission to actual display
and can interpret the local experiment's CPU deadline and presentation target.
These are software timings, not physical Pencil latency.

Other ports' changes through `a7c048c` are integrated. Both Apple Release builds,
32 Apple bridge checks, 15 shared host checks, 236 shared UI checks, the direct
trace recorder check and ten analyzer checks pass. The existing host hardware
benchmark remains explicitly ignored. The restored scheduler was then exercised
with and without GPU timing on both physical hosts, using the same binaries.
All four measured twenty-second intervals completed with zero rejected input,
missing presentation callbacks or zero-time presentations. The iPad CPU p99
remains above budget at 9.077/9.120 ms with GPU timing on/off. See the complete
[scheduling comparison](../../apps/layer-apple/PERFORMANCE.md#display-scheduling-comparison--2026-09-11)
for counts, display delays, whole-trace caveats and source-comparison limits.

The completed Mac painting was directly captured and inspected. Test apps were
closed after collection, preserving the original editor. Raw traces, images,
device/signing logs and the rejected scheduling experiment stay local. No
system menu-bar automation was used. Full visual/feature parity, the remaining
lifecycle matrix, physical input latency, calibrated recorder overhead and the
sustained workload matrix remain open. The current Mac display configuration
still reports 90 Hz and cannot establish the 120 Hz presentation gate.

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
| 5. Document/settings/workspace persistence and lifecycle | Atomic preferences, private artwork recovery and SQLite workspace library implemented. Migration, scene ownership, switching and history pass direct checks; Simulator restart and history Restore pass. Provider delivery and full physical lifecycle matrix remain. | Same persistence, recovery and workspace library. Native restart, owner isolation, Save/Open, recovery, workspace switching and failed/canceled close pass direct checks; full window/display/sleep/memory-pressure matrix remains. |
| 6. Progressive visual acceptance for every editor component/state | Matching initial simulator/Chrome capture and full pixel report exist; baseline fails parity. Device captures and complete fixture matrix remain. | Matching native Mac/Chrome initial captures exist; baseline fails parity. Complete fixture matrix remains. |
| 7. Hardware performance, sustained sessions and delivery | Shared recorder and five synthetic profiles implemented. One physical ten-minute 4K watercolor run completes; CPU p99 is 9.120 ms. Full matrix, frame-budget tails, physical latency and overhead calibration remain. | Same profiles; one native ten-minute 4K watercolor run completes. Current validation target is 90 Hz. Full matrix, memory growth, physical latency and overhead calibration remain. The user deferred 120 Hz testing until suitable hardware is available. |

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
