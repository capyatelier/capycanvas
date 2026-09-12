# Capy Canvas for Apple platforms

Native UIKit/iPadOS and AppKit/macOS targets share Swift editor components and
the Rust Metal bridge. The bridge uses `crates/layer-host`, also used by Android;
the document, engine, UI policy, catalog and rendering remain in shared Rust.
The full-window Metal layer stays behind editor controls and the header.
Mac top-level menus use the OS menu bar and target the focused editor window.
Zen clears the native window controls. The iPad keeps its in-app menus, sharing
the same menu item implementation and Rust catalog/actions.
Both platforms are required at every milestone, with separate functional,
visual and hardware performance evidence. Shared changes must build on both.
See [the Apple goal and acceptance tracker](../../docs/history/apple-acceptance.md)
for the shared-code boundaries, milestone matrix and remaining work.

[Performance workflows and measurements](PERFORMANCE.md) include five opt-in
synthetic drawing profiles shared by both targets and a ten-minute physical 4K
watercolor baseline on each. Current validation targets 90 Hz on Mac and 120 Hz
on iPad; the user deferred Mac 120 Hz testing until suitable hardware is available.
CPU spikes, missing GPU observations and the remaining workload matrix leave
performance acceptance open. Benchmark sessions use
isolated storage; ordinary launches do not start synthetic input or recording.

Apple snapshot publication serializes the shared host models directly to UTF-8,
avoiding the intermediate JSON tree on the render owner. Incremental workspace
updates retain the panel models while native placement follows floating motion;
divider and floating-panel resizing update layout dimensions and camera together
while retaining control models and view identities. Tab previews and drop hints
use the same shared publication. Release, cancellation and content changes still
publish complete models, preserving history and persistence. The compatibility
value/byte APIs keep their existing schema and exact numeric values.
The [snapshot transport checks](PERFORMANCE.md#snapshot-transport) include
reproducible payload fixtures and a CPU benchmark.

## Build

See the [Apple development guide](../../docs/development/apple.md) for prerequisites,
macOS/iPadOS build commands, signing and running in Xcode. Rerun
`scripts/prepare.py` when shared assets change and `scripts/project.py` when adding
or removing Swift files. Edit the generators rather than generated project entries.

## Shared editor controls

Fresh editors use the shared full editor preset: the Tools and Commands bars,
Tool Set, Tool, Brush size, Color, Navigator/Diagnostics,
Properties/Filters and Layers. Restoring a saved workspace preserves its layout
and toolbar contents, including workspaces from earlier Apple builds.

Both apps include Manage Workspaces, Layout History and toolbar management backed
by the shared SQLite library. Selecting a workspace previews its arrangement in
the editor; Switch to Workspace restores its latest layout and tool settings.
Cancel and filtering away a selection restore the previous arrangement. New
Workspace asks for a name and copies the current layout and tool settings into
independent history. Save Layout, Load Layout and their separate manager page
have been removed from the product scope. Shared Rust owns
availability, forms, history and storage policy; the Apple coordinator keeps
database work off the drawing owner. See [Apple persistence](PERSISTENCE.md#workspace-library)
for migration, window ownership and direct workflow checks.

The shared Painter, Illustrator and Photographer workspaces appear in the header
between the document title and clock. Their stable identities retain edited
names, arrangements and tool settings. Switching saves the outgoing workspace;
an existing owner is focused instead of replaced. Fresh storage opens Illustrator,
while upgrades resume their previous workspace. Reset All Brushes uses the shared
confirmation and clears every brush override in the current workspace, preserving
its color, selected tool, layout, document and other workspaces.

Both hosts support all five shared toolbar styles: small, medium, large, medium
labeled and large labeled. Ribbons, floating panels, content drawers and partial
Zen use the Rust icon sizes, label line counts and weight. Labeled tiles place
text beside the icon; size controls retain the shared size glyph. Vertical bars
use horizontal separators. Zen strips retain individually accessible buttons,
and disabled toolbar controls apply one dimming step while remaining inactive.
`testToolbarStylesAndActions` checks native style selection, button bounds,
the Zoom action and Zen visibility. Direct bridge checks cover all style
projections and workspace history on both hosts.

The [toolbar component capture](../../tools/visual/README.md#toolbar-components)
compares all five styles with Chrome using actual SwiftUI controls and shared
vector assets, without launching an editor or automating window/menu controls.
Component evidence supplements the full-editor visual and physical input gates.
Icon, toolbar and tool-choice selections now share the editor accent and one
disabled-opacity step over the whole button, independent of the Mac system
accent. The [control-color matrix](../../tools/visual/README.md#editor-control-colors)
compares enabled/selected combinations in both themes and retains full raw
pixel differences; UIKit rendering and full-editor acceptance remain separate.

Docked, floating and drawer tab strips use natural label widths, horizontal
scrolling and the shared 36-point icon-only size. Their moving visual copies use
Rust's frozen halfway points and clip bounds; native input measurements stay in
their original slots. Neighbors animate for 120 ms, respecting Reduce Motion.
Release and cancellation retire the preview after the owner acknowledges the
layout transaction. A reopened drawer observes its own interaction state, so
coalesced close/reopen and workspace history cannot leave its gestures disabled.
The [tab comparison workflow](../../tools/visual/README.md#workspace-tabs) captures
real shared SwiftUI headers and the corresponding live Chrome editor, without
system-menu automation. Complete visual parity remains open; this focused
workflow does not establish full editor acceptance.

Native panel controls report intrinsic body heights and tab widths to the shared
Rust `measure_panels` action. Rust fits floating panels and tab groups, caps their
height and honors manual sizing. Measurements use the mounted controls before
scroll clipping; lightweight copies measure inactive tab labels without mounting
extra Navigator, thumbnail or filter content. Inactive bodies retain their last
measurement until mounted again. Drawer bodies keep their separate width and
measurement path. Changes are coalesced and quantized to 1/64 point to avoid
float conversion feedback; transient restoration republishes cached facts without
adding workspace history or storage writes.

The direct check uses actual SwiftUI geometry in an invisible AppKit host for
both Apple presets. It checks natural floating sizes, width reflow, tab fitting,
control visibility, workspace Undo, growing layer content and settled measurement
publication. UIKit pixels and sustained resizing performance require their own
validation:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/panel-measurements.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-motion.swift
```

Tool Set projects the shared groups and subtools for painting, figures, regions,
rulers and Operation. Every catalog brush remains reachable through its family;
Rust remembers the selected subtool and edited settings when changing groups.
The Tool panel shows the active tool's numeric fields and actions. Numeric
expressions, units, ranges, slider mappings and stepping resolve through Rust.
The shared Apple control
handles optimistic edits and local validation feedback; small AppKit/UIKit
adapters handle text selection, keyboard focus, Return, Escape and arrow keys.
Native focus changes are deferred until after SwiftUI updates to avoid entering
the hosting responder graph recursively when accepting an expression.

Numeric labels truncate within compact panels, leaving values readable. Spin
fields keep the shared unit suffix when idle, with the value and both step buttons
in one input surface. Sliders use the shared panel/text fill color and straight
progress edge; endpoint buttons receive the shared disabled opacity. Both native
text adapters use tabular digits. The
[numeric-control comparison](../../tools/visual/README.md#numeric-editor-controls)
includes both Apple presets/themes, width and endpoint cases, plus mounted-field
checks of actual editor actions. Full UIKit and editor pixel parity remain open.

The Color panel provides the shared HSV square / HLS triangle,
foreground/background/transparent paint slots, swap and component expressions.
Rust owns color conversion, hue memory, normalized geometry, hit regions and
drag clamping. Both native hosts share the gradient drawing and controls; their
small input views latch the starting region for each mouse, Pencil or touch
contact. Picking a color exits transparent paint using the previous paint slot.
Channel edits update the current Rust state, preserving other queued changes.

For reproducible Debug editor fixtures, `CAPY_INITIAL_ACTIONS` accepts a JSON
array of shared actions at launch. For example, this opens the Tool panel without
driving the Mac system menu bar:

```sh
open -n --env CAPY_INITIAL_ACTIONS='[{"type":"customize","action":{"type":"set_panel_visible","panel":"tool_settings","visible":true}}]' \
  apps/layer-apple/DerivedData/Build/Products/Debug/CapyCanvas-Mac.app
```

Release builds ignore this variable. The focused `testNumericToolControls` test
uses a fresh light-theme editor on both platforms. It checks expression acceptance,
invalid input, stepping and remembered brush settings across group changes.
`testCompleteEditorCapture` captures only the settled default workspace and its
logical dimensions for the Chrome `initial` comparison. Standalone edit-state
checks need no GUI automation:

```sh
xcrun swiftc apps/layer-apple/Shared/Editor/NumericEditState.swift \
  apps/layer-apple/tests/numeric-edit.swift -o /tmp/capy-numeric-edit
/tmp/capy-numeric-edit
cargo test -p layer-apple apple_tool_panels
cargo test -p layer-apple apple_transform_settings
cargo test -p layer-apple apple_color_
```

The focused `testColorControls` test checks native wheel contacts, color-space
and paint-slot controls and expression entry on both targets. It retains full
captures plus measured wheel geometry for the shared
[color sampling check](../../tools/visual/README.md). The headless ABI tests
verify resulting brush/eraser pixels and exact Undo without driving menus.

The Color panel shares the web layout on both Apple targets: three paint slots
with a checkerboard/selected background, labeled Swap and color-space buttons,
and three full numeric slider controls. Its fast
[complete-panel fixture](../../tools/visual/README.md#complete-color-panels)
compares 48 native/Chrome cases without visible native windows. The focused UI
workflow records the actual accepted Rust color through opt-in debug metadata,
so its color oracle does not assume ideal touch coordinates.

Properties choice controls share an Apple button/popover projection, sizing the
closed control to the longest option while keeping room for its row label.
The layer blend control exposes its current value to accessibility.
`testEditorControlLayout` checks all six Navigator hit targets, changes a blend
mode through Properties, verifies the Layers value and undoes the change. It
also attaches matching `initial` and `canvas-under-header` captures. The latter
uses four shared zoom-in steps so the paper is visible through empty header
space; title, Zen and Settings retain their own background plates.

## Install and run

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
xcrun devicectl list devices
xcrun devicectl device install app --device DEVICE_ID \
  apps/layer-apple/DerivedData/Build/Products/Debug-iphoneos/CapyCanvas-iPad.app
xcrun devicectl device process launch --device DEVICE_ID \
  --console art.capycanvas.apple.ipad
```

Physical devices need Developer Mode, pairing, a development certificate/private
key and a profile covering the selected device. A first Personal Team installation
may also require trusting the developer account in the device's Settings.
Team IDs, keys and provisioning profiles are not stored in this repository.

Settings and committed workspace layouts now persist in private Application
Support files. Settings propagate across live owners; each restored scene keeps
its own workspace. See [PERSISTENCE.md](PERSISTENCE.md) for ordering, atomic writes,
failure/retry behavior and fast tests. Native New/Open/Save/Save As use the shared
project format. Both targets support custom canvas dimensions and PNG export. GPU export
readback and PNG encoding run on the file worker. Unsaved artwork also receives
private recovery copies. Use **File → Recovered Drawings…** to open one; copies
are offered after restart and retain unsaved status until you explicitly save.
See [PERSISTENCE.md](PERSISTENCE.md) for atomic generations, lifecycle handling,
reproducible checks and remaining physical-device/performance acceptance.

Both targets now project the live shared application menus. Keyboard Shortcuts
supports search, alternate bindings, conflict replacement and resets; Settings
search uses the shared results. The focused
`EditorLaunchTests/testShortcutConflictAndEditorEffect` test exercises capture
and the resulting Zen action without automating the system menu bar. The shared
inventory command emits the settled default document/workspace, every command
and panel's availability, all five settings pages, and representative
menu/layer/shortcut states for **both** Apple policies. Schema 4 stores these
under `platforms.ios` and `platforms.mac`; unavailable commands remain visible.
It also follows the actual tool-choice graph, including all shipped brushes,
tool modes, settings and actions, and emits all three task workspace layouts,
their registered panels and panel/group/tile/ribbon/Zen context menus. Managed
workspace IDs are synthetic; the inventory never opens user storage. The
`--gpu` mode seeds a disposable drawing through shared fill actions so transform
controls can be enumerated with a real hardware renderer.

The [command review](command-coverage.json) classifies all 62 commands, nine
workspace service commands, 14 panel control types, six preference kinds and
six property kinds,
with Apple handler/check references. The audit detects catalog and availability
drift, unvisited or unresolved tool choices, and missing control/service reviews:

```sh
cargo run -p layer-host --example inventory -- --gpu > /tmp/capy-inventory.json
python3 apps/layer-apple/scripts/audit-commands.py /tmp/capy-inventory.json
python3 apps/layer-apple/scripts/test-property-audit.py /tmp/capy-inventory.json
CAPY_PROPERTY_INVENTORY=/tmp/capy-inventory.json \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/property-actions.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-menu-actions.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-manager.swift
```

The current graph contains 80 tool choices and 28 setting IDs per Apple preset;
the workspace scenarios include 133 context menus per preset. Without `--gpu`,
renderer-dependent tool failures remain explicit and fail the expanded audit.
Schema 4 also records paint, paper, groups and every shipped filter: currently
43 property scenarios and 160 editable fields per preset. Each field changes
through its real action, restores all properties with Undo/Redo, and resets to
the shared default. Lockable targets retain their disabled schemas and reject
a sampled stale field edit. Paper's protected stack position has no Lock action;
its opacity remains editable. Ten corruption probes per preset check that the
audit rejects missing filters, controls, handlers and contradictory results.
Older schema 2 artifacts receive command-only checks; schemas 2/3 warn that
property scenarios are not checked.

The direct Swift property check replays those actions through `EditorStore` and
the serial `NativeOwner`, comparing the actual published schemas and values.
Both presets run without visible windows or a renderer, in disposable storage.
This covers shared Apple routing, decoding, history and lock behavior; it does
not establish UIKit widget, GPU filter-pixel or physical interaction acceptance.

The direct Swift menu check dispatches actual shared menu payloads through the
Apple editor and workspace service on both presets. It checks all nine routes,
form cancellation, manager/history dismissal, host-request acknowledgement,
workspace switch/return and document preservation using isolated storage and
no OS menu automation. The companion manager check exercises confirmed forms
and coordinator workflows. These macOS-hosted checks do not exercise UIKit
widgets. Passing the audit establishes catalog coverage; complete native
workflows, dynamic controls, visual states and hardware performance still
require their own evidence. Save/Load Layout remains excluded.

Tool Settings renders checkable and ordinary actions with the same shared
button component on both Apple targets. Labels use the shared bold text size,
24-point line boxes and greedy wrapping; selected and disabled states use the
editor accent and one opacity step. The command route remains `store.invoke`.
The [tool-action capture workflow](../../tools/visual/README.md#tool-action-buttons)
compares all six actions against the real browser factory at two widths in both
themes, without visible editor windows. Exact geometry passes; full pixel,
native activation and UIKit appearance acceptance remain open.

Mac customized controls can now invoke the shared Full Screen command. AppKit
notifications update its selected state and icon after the window actually
changes mode. View keeps AppKit's native Full Screen menu item, and native
Control-Command shortcuts pass through the canvas responder. Repeated requests,
native transitions, failures and detachment are checked without system-menu
automation or visible test windows:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/window-presentation.swift
```

The native adapter uses [AppKit window full screen](https://developer.apple.com/documentation/appkit/nswindow/togglefullscreen(_:)).
The installed UIKit SDK's [iOS geometry preferences](https://developer.apple.com/documentation/uikit/uiwindowscene/geometrypreferences/ios)
expose orientation changes but no equivalent iPad window toggle. That shared
command capability remains explicitly unavailable on iPad and open in the review.
Full-screen editor layout/rendering remains an open acceptance item.

Both Apple headers implement **Show battery and clock** from Appearance settings:
Always, In fullscreen mode, or Never. The shared Swift component uses the editor
palette and the browser/Android battery geometry. Desktops without an internal
battery show only the clock; unavailable readings never become a fabricated
percentage. Mac full-screen notifications and iPad scene geometry observations
control the fullscreen-only policy. Observing an iPad scene does not add the
still-unavailable full-screen toggle.

Menu labels, the title and clock use the shared six-point side padding. Compact
iPad headers hide the title and preserve all menus in an overflow control when
the workspace pill and status controls need the space. Mac keeps its OS menus,
document title and window-control reservation. Workspace labels use natural
widths with truncation, a 34-point capsule and the shared app accent. Clock and
battery backgrounds stay transparent over the canvas. See the
[complete header comparison](../../tools/visual/README.md#complete-header-components)
for fast native/Chrome captures and explicit platform adaptations.

Visible headers share one native battery subscription and one minute-aligned
clock timer. The last hidden/background header stops monitoring; minute and
power updates stay outside the Rust owner and drawing display link. UIKit uses
[battery monitoring](https://developer.apple.com/documentation/uikit/uidevice/isbatterymonitoringenabled),
restoring its previous state after use. AppKit uses IOKit power notifications,
with power-service reads on a utility queue. Fast lifecycle checks need no GUI:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/SystemStatus.swift \
  apps/layer-apple/macOS/Platform/BatterySource.swift \
  apps/layer-apple/tests/system-status.swift -o /tmp/capy-system-status-tests
/tmp/capy-system-status-tests
```

`testSystemStatusSetting` exercises preference and Zen effects in the editor.
Exact component rasterization and the complete full-screen/window-layout matrix
remain part of visual acceptance; implementation is not a pixel-parity pass.

Launch a local Mac build with:

```sh
open apps/layer-apple/DerivedData/Build/Products/Debug/CapyCanvas-Mac.app
```

## Validation

See [PERFORMANCE.md](PERFORMANCE.md) for opt-in local CPU/GPU/presentation traces,
the report tool, instrumentation checks and the remaining hardware evidence.
See [INPUT.md](INPUT.md) for Pencil corrections, shared stroke/history handling,
fast input checks and the physical-device evidence still required.

Check editor behavior directly without driving system menus:

```sh
cargo test -p layer-host --lib
cargo test -p layer-apple --lib
```

The Apple tests dispatch through the real C ABI for both platform configurations.
They check brush/zoom/settings actions, session isolation, committed ink after
pen-up, and exact GPU document pixels through undo/redo. The GPU tests require
hardware Metal access; they do not establish physical input or presentation timing.
The staged-startup check also verifies pending ink survives the initial paper
frame and stays undoable while document/brush shaders become ready.
Layer checks cover checked selection versus the drawing target, mask targeting,
hierarchy, rename, blend/opacity/locks/references, shared menu enablement, image
import and GPU thumbnails. Imported pixels round-trip exactly through undo/redo.
Numeric expressions and slider mapping use a stateless shared-policy entry point.

Check the native image decoder without launching an app:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/JSON.swift \
  apps/layer-apple/Shared/Bridge/LayerImageImport.swift \
  apps/layer-apple/tests/image-import.swift -o /tmp/capy-image-import-tests
/tmp/capy-image-import-tests
```

Synthetic images check EXIF orientation, sRGB channels, straight alpha, row order
and rejection of dimensions beyond the shared import limit. File decoding runs
off the UI and render-owner queues; document import runs on the serial owner.

The shared JSON transport uses direct Foundation container lookup to avoid
bridging a complete dictionary for each field read by the editor. Check native
and decoded containers, scalar fidelity, bounds, immutable edits and round trips
without launching an app; optional JSON files extend the recursive check to wire
fixtures:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -O -parse-as-library \
  apps/layer-apple/Shared/Bridge/JSON.swift \
  apps/layer-apple/tests/json-lookup.swift -o /tmp/capy-json-lookup-tests
/tmp/capy-json-lookup-tests
```

Both editors observe snapshot fields and individual command, panel and menu
entries through the shared `EditorSnapshotState`. SwiftUI reevaluates readers
when their values change; stroke-boundary enablement updates no longer publish
the entire editor as changed. Values and actions still come from Rust.
`store.state` and `store.snapshot` are live main-actor field readers. A subscript
returns immutable `JSON`; `.json` takes an immutable whole-object snapshot and
observes all its fields. Use that explicit copy for document/lifecycle handoff.
Related readers are updated before observation signals are published, so even
synchronous observers see a coherent revision. Camera patches update the same
canonical state without rebuilding the command, panel or menu indexes.

Interactive publication uses C request 5 (`NativeHost::take_update_bytes`). A
full snapshot establishes `workspace_update.model_revision`; later motion must
match that revision and cannot precede the last accepted presentation revision.
`WorkspaceMotion` stages group positions, tab previews and drop hints atomically
with the models. `WorkspacePlacement` moves each native panel and resize handle,
including its hit areas, tab clipping and live Navigator allocation. Controls
continue reading the retained layout and panel models. Their `state.revision`
changes with a full model publication; `workspaceMotion.revision` tracks later
motion. Optional camera patches update canonical camera state independently.
Document and lifecycle consumers still receive full immutable model snapshots.
Request 3 remains available for compatibility checks. Do not alternate the two
publication APIs on one interactive owner because they acknowledge separately.

The direct floating-workspace check exercises both Apple presets in an invisible
AppKit host, retaining Navigator identity through tear-off and group movement,
checking actual hit/clip/resize allocations, completion, history and a camera
action. It does not measure UIKit pixels or hardware presentation cadence:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-motion.swift
cargo test -p layer-apple incremental_apple_abi -- --nocapture
```

Check observation fidelity and actual SwiftUI rendered-value propagation without
XCTest or a visible editor. The latter uses an invisible AppKit hosting view;
both checks exercise the code shared with iPad:

```sh
for check in snapshot-projection snapshot-views; do
  DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -O -parse-as-library \
    apps/layer-apple/Shared/Bridge/JSON.swift \
    apps/layer-apple/Shared/Bridge/SnapshotProjection.swift \
    apps/layer-apple/Shared/Bridge/WorkspaceMotion.swift \
    apps/layer-apple/Shared/Bridge/EditorSnapshotState.swift \
    "apps/layer-apple/tests/$check.swift" -o "/tmp/capy-$check-tests" || exit
  "/tmp/capy-$check-tests" || exit
done
```

The field check also accepts optional JSON wire-fixture files. These checks
establish data fidelity and selective invalidation; hardware results and the
remaining presentation gaps are recorded in [PERFORMANCE.md](PERFORMANCE.md).

The shared Swift scheduler has a fast standalone check, with an asynchronous
native-owner test double and no application launch:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/CanvasFrameDriver.swift \
  apps/layer-apple/tests/frame-driver.swift -o /tmp/capy-frame-driver-tests
/tmp/capy-frame-driver-tests
```

Use a focused launch test for a settled default editor capture:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
xcrun simctl list devices available
xcodebuild -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-iPad -destination 'platform=iOS Simulator,id=SIMULATOR_ID' \
  -derivedDataPath apps/layer-apple/DerivedData/Simulator \
  -resultBundlePath artifacts/ui/parity/ipad-launch.xcresult \
  -only-testing:CapyCanvas-iPadTests/EditorLaunchTests/testCompleteEditorCapture \
  CODE_SIGNING_ALLOWED=NO test
```

Choose a fresh result-bundle path for subsequent runs. The capture checks that
the default controls exist and waits for the Metal canvas and live Navigator.
It attaches the full landscape screen on iPad or the editor window on Mac.
Full-editor, control-layout and numeric workflows use a private UUID namespace
with the real workspace library, retaining all three task-workspace segments.
The numeric workflow selects the Brush size tab before inspecting its independent
readout; the current default layout groups that panel with Tool Settings.
Both focused UIKit workflows pass on the 13-inch iPad simulator. Matching full
Chrome captures still fail exact pixel parity; see the
[full-editor comparison](../../tools/visual/README.md#full-editor-captures).
It does not measure drawable presentation or prove full visual/functional parity.
Physical input and hardware performance acceptance are required on each platform.
Replace the final test method with `testNumericToolControls` for the focused
numeric input and grouped brush workflow.

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  xcodebuild -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-Mac -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath apps/layer-apple/DerivedData/Mac \
  -resultBundlePath artifacts/ui/parity/mac-launch.xcresult \
  -only-testing:CapyCanvas-MacTests/EditorLaunchTests/testCompleteEditorCapture \
  DEVELOPMENT_TEAM=YOUR_TEAM_ID CODE_SIGN_IDENTITY='Apple Development' test
```

The shared `testFilterSearchPreviewAndProperties` workflow checks filter search,
GPU preview loading, radius expressions, curve insertion/reset and gradient
insertion/position/reset. On iPad it also checks canvas geometry while the search
keyboard is open. A faster Mac-only alternative avoids XCTest startup and never
addresses the system menu bar:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  xcrun swift apps/layer-apple/tests/effect-controls-mac.swift \
  apps/layer-apple/DerivedData/Mac/Build/Products/Debug/CapyCanvas-Mac.app
```

This utility requires existing Accessibility permission for its launching
terminal/agent. It opens a separate editor with persistence disabled, uses
control identifiers and graph-relative pointer events, and leaves its final
fixture open for direct capture. Missing permission returns failure without
launching or modifying an editor. The Chrome `filter-properties` scenario in
[`tools/visual`](../../tools/visual/README.md) reproduces its final document.

Use a fresh result-bundle path. The iPad and Mac targets share frame admission,
including wake preservation while a frame is queued, and flush final UI state
before going idle. Each window owns its own session. Reattaching a Metal layer
does not reload bundled filters over a session's edited filter library.
Both Apple targets now use the same staged GPU frame preparation as Android:
paper first without consuming document replay, then document and active-brush
dependencies, followed by remaining shaders. The editor uses its shared theme
background until the first Metal viewport submission. Bundled filters merge
after document readiness, and shader caches remain in the app's local caches
directory. Submission is distinct from GPU completion and on-screen presentation;
cold/warm startup latency and hardware frame cadence still require measurement.
Use direct window captures and editor action/state/output checks for routine
parity work. Coordinate testing within the app is appropriate for custom canvas
and control behavior when useful. Reserve UI automation for targeted app
regressions; macOS menu mechanics are trusted platform behavior and are not
tested with coordinate clicks.

Use [the shared visual tools](../../tools/visual/README.md) for matching local
Chrome captures and complete image differences for either native target.

Navigator and Diagnostics share their SwiftUI projections across Apple targets.
Navigator uses Rust geometry and camera actions, with the shared GPU overview
drawn in the existing Metal canvas presentation pass. The same live composition
supplies the main canvas and preview, including its camera outline. Native layout
submits logical bounds, visible clips and stacking order; Rust resolves current
document dimensions and display scale. No preview bitmap, readback, worker decode
or refresh timer remains. Native panels reveal the image at their own stacking
position while retaining their controls, shadows and drawer clipping. Optional
overview resources are prepared after the first paper presentation and reused.
Diagnostics queries the shared rows and bounded chart at 5Hz only while visible.
Timing sampling is restored on GPU attachment and document replacement.

`EditorLaunchTests/testNavigatorAndDiagnostics` is the focused shared UI check
for either scheme. It uses a disposable drawing, in-app navigation buttons and
an overview drag, then switches to Diagnostics and back. Mac also compares actual
preview pixels after drawing, Undo and Redo. Simulator touch verifies the shared
single-finger non-painting policy; it cannot synthesize physical Pencil evidence.
The faster checks run with `cargo test -p layer-apple navigator -- --test-threads=1`
and verify document pixels/history, live geometry, atomic bounded layout records,
idle behavior, document replacement and display scaling on both Apple policies.
`cargo test -p layer-render-wgpu overview -- --test-threads=1` checks actual GPU
overview pixels, clipping, transparency, camera changes and resource reuse.
Full visual and physical performance acceptance remains in
[the matrix](../../docs/history/apple-acceptance.md).

## Implementation status

Workspace customization shares its presentation and gesture ownership across
iPad and Mac. Panel/group/toolbar/tile and Zen context menus query the live Rust
models on activation. Toolbar creation, search, selection, naming, duplication,
management and delete confirmation use shared validation/actions. Expanded
configuration keeps its live preview beside editable controls and visibility
toggles. Dragging a panel/group, resizing floating panels and dividers, and
dropping tiles use Rust placement, eligibility and workspace history. The root
owns a drag across tab tear-off; source views register only measured rectangles.
Expansion queries run only during layout/configuration changes and animation;
ordinary painting/camera updates do not start menu or expansion queries.

The focused `testToolbarCustomization` and
`testPanelConfigurationAndLiveDrag` checks use in-app controls. The iPad toolbar
check also verifies canvas pinch navigation through empty workspace regions.
Run the faster shared Metal/action regressions with
`cargo test -p layer-apple workspace -- --test-threads=1`.
The Chrome `panel-configuration` fixture enables direct full-image comparison.
Collapsed columns, tabbed content drawers, child tool drawers and partial-Zen
edge toolbars now share native projections on both targets. Content-panel tiles
and the Commands panel are available. The core supplies column geometry, drawer
composition, natural toolbar height, anchors, connections and dismissal policy.
Native scrolling reports clipped tile bounds so a child drawer follows its
origin and disappears when the tile scrolls out of view. Drawers retain their
body only through closing and coalesce geometry requests behind one pending
query per projection. Canvas contact admission runs on the Rust owner: an outside
contact that dismisses a drawer cannot leak a later move or prediction into paint.
Mode changes refresh chrome visibility even when native measurements are equal.

Drawer headers share their tab buttons, context menus and group grip with docked
and floating panels. Native preferences report the visible drawer and clipped
tab bounds to Rust, enabling tab reordering, individual tear-off, whole-group
dragging and drops into open column drawers. Closing projections stop accepting
input while their exit animation finishes. Unused header space drags the group,
and the drop indicator stays above the drawers. New drags restore cached bounds
after workspace changes clear Rust's transient measurements. Shared docking accepts unordered tab
measurements, and collapsing a column with no measurable width fails without
changing the workspace. Floating tabbed toolbars use the same natural-height
allocator as their body, including divider extents and all five tile styles.

`testDrawerDragAndDock` exercises these native gestures on each Apple target.
The faster `tests/workspace-drawers.swift` check uses an invisible AppKit hosting
window to exercise actual shared SwiftUI geometry, tab/header sources, serial drag
dispatch, reordered presentation, restored drop targets and closing input retirement:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-drawers.swift
```

Debug-only `CAPY_INITIAL_ACTIONS` fixtures run once after restoration and the first
native surface size, so layout actions use the editor's viewport instead of the
owner's 1×1 placeholder. Release builds have no fixture override.

`testCollapsedColumnsDrawersAndZen` exercises column tabs, a child drawer,
outside dismissal and restoring the column; `testPartialZenToolbar` checks the
default standalone toolbar projection and exiting Zen. These shared checks use
in-app controls. The `partial-zen` Chrome fixture uses the same shared workspace
policy; its older comparison predates the browser's edge toolbar implementation.
Full interaction, visual, physical input and performance acceptance remain open
on both targets.

Full port acceptance remains open. The simulator renders
the live canvas; the iPad target builds, signs, installs and launches; the AppKit
target builds and launches, with mouse drawing and keyboard undo/redo checked.
Both targets share frame admission and per-window session ownership. Initial
shared menus, tool buttons, brush/size presets and ordinary native settings rows
are wired. The shared layer panel includes GPU thumbnails, checked selection,
content/mask targets, blend/opacity/locks/clipping/references, groups, rename,
reordering, image import and the Rust context-menu model. Thumbnail work is
limited to visible rows and eight pending readbacks, with no idle polling once
previews are current. Context gestures, dragging and all menu workflows still
need complete acceptance on both platforms.

Shared Filters/Properties, menus/shortcut editing, color/tool settings,
Navigator, Diagnostics and workspace customization are implemented, with focused workflow evidence in
the acceptance document. Still required: complete panel/drawer/menu/dialog
behavior and customization, the remaining specialized controls and complete
workflow coverage, document recovery and settings/workspace lifecycle coverage, Pencil estimated-property
corrections, complete hover/sensor/shortcut routing, platform lifecycle coverage,
full pixel-difference validation, and measured iPad and Mac hardware performance.
Mac tablet/proximity, mouse, wheel, trackpad and keyboard adapters are present;
physical tablet sensors and the full shortcut/lifecycle contract still require
validation. Both platforms must complete the expanded acceptance matrix.
