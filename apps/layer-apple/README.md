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
See [the Apple goal](../../docs/history/apple-acceptance.md#goal) for scope and
shared-code boundaries, and the [current release checklist](../../docs/development/apple-release-checklist.md)
for remaining work. Historical milestone lists do not supersede later passes.

Popup text uses opaque shared-theme surfaces, including the workspace pill.
`EditorPopupSurface` pairs the shared text and panel colors for custom overlays;
`EditorPopupPresentation` fills app-owned popovers and sheets, including their
margins and arrows. Settings retain an opaque native semantic background.
Keep existing sizes and attach contextual presentations to the invoking control
or row. Layer menus use the pressed row; the footer uses its own button.
Editor menus share an opaque vertical action list and an anchor overlay at the
editor or sheet root. The overlay inherits the root's current palette; copying
the invoking control's whole environment can override menu text colors.
Mac system menus retain AppKit appearance and accessibility behavior.

[Performance workflows and measurements](PERFORMANCE.md) include five opt-in
synthetic drawing profiles shared by both targets and a ten-minute physical 4K
watercolor baseline on each. Current validation targets 90 Hz on Mac and 120 Hz
on iPad; the user deferred Mac 120 Hz testing until suitable hardware is available.
The user accepts smooth drawing with rare measured misses. The captured Pencil
prediction stall is fixed and physically confirmed; Diagnostics GPU values work
on both hosts. Complete sustained workload/resource and physical latency evidence
remains open. Benchmark sessions use isolated storage; ordinary launches do not
start synthetic input or recording.

The shared render owner drains temporary native resources after each task. The
frame driver permits one queued render operation and lets CAMetalLayer manage
drawable availability. Presentation callbacks collect optional diagnostics;
rendering continues when those notifications are missing. Resume requests a
fresh frame, and surface generations protect replacement views from old replies.
See the [startup-progress regression](PERFORMANCE.md#startup-progress-without-presentation-callbacks--2026-09-16)
and current checklist before repeating historical performance experiments.

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

For isolated device installs and UI tests, set `CAPY_APPLE_BUNDLE_ID` on the
`xcodebuild` command. The app uses that identity and its test target uses the
`.tests` suffix, so the regular editor can remain installed. Use a private
`CAPY_PERSISTENCE_NAMESPACE` for Debug UI fixtures as described below.

The shared SVG generator preserves fixed colors and ordered `currentColor` paints
as vector assets, with a bundled paint manifest read once by the shared icon view.
It retains the single-image path for ordinary symbolic icons. See the
[icon comparison guide](../../tools/visual/README.md#shared-icon-paints) for direct
native/Chrome captures and compositing checks.

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
for startup, window ownership and direct workflow checks.

The header follows saved workspace pins and list order. Manage Workspaces offers
Show in top bar, Move Up/Down and narrow row grips; new workspaces are pinned by
the shared manager. An unpinned current workspace appears temporarily at the
front. These preferences survive restart and refresh across windows without
changing the canvas preview, document or layout history. The compact workspace
list has no search field; toolbar management retains its search controls.
Deleting the active workspace selects an available included workspace through
shared policy. The confirmation describes deletion as permanent.

The included Sketch, Paint and Photo workspaces retain stable
identities, edited names, arrangements and tool settings. Switching saves the outgoing workspace;
an existing owner is focused instead of replaced. Fresh storage uses the shared
default workspace; returning editors resume their previous workspace. Reset All Brushes uses the shared
confirmation and clears every brush override in the current workspace, preserving
its color, selected tool, layout, document and other workspaces.

Both hosts support all five shared toolbar styles: small, medium, large, medium
labeled and large labeled. Ribbons, floating panels and content drawers
use the Rust icon sizes, label line counts and weight. Labeled tiles place
text beside the icon; size controls retain the shared size glyph. Vertical bars
use horizontal separators. Zen hides editor controls until Tab restores them.
UIKit observes chrome contacts before button activation so the Zen button's
release cannot immediately reveal chrome again. Its passive observer excludes
the native canvas, which owns contact dismissal and drawing. Mac retains its
tap observer. Disabled toolbar controls apply one dimming step while inactive.
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

Native panel controls report intrinsic body heights, fixed controls, scrolling
row heights and tab widths to the shared Rust `measure_panels` action. Dragged
panels preserve their visible size past workspace edges, then Rust applies the
shared floating release budget and useful scrolling minimum in one history step.
Compact Color content stays whole. Rust also fits tab groups and honors manual
sizing. Measurements use the mounted controls before
scroll clipping; lightweight copies measure inactive tab labels without mounting
extra Navigator, thumbnail or filter content. Inactive bodies retain their last
measurement until mounted again. Drawer bodies keep their separate width and
measurement path. Changes are coalesced and quantized to 1/64 point to avoid
float conversion feedback; transient restoration republishes cached facts without
adding workspace history or storage writes.

The direct check uses actual SwiftUI geometry in an invisible AppKit host for
both Apple presets. It checks natural floating sizes, width reflow, tab fitting,
control visibility, workspace Undo, growing layer content, settled measurement
publication, frozen drag previews and fitted scrolling releases with Undo/Redo.
UIKit pixels and sustained resizing performance require their own
validation:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/panel-measurements.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-motion.swift
```

[Stacked collapsed columns](../../docs/ui/stacked-columns.md) use shared Rust
membership, drop targets, preferences, geometry and history. The ordinary dock
views render an open member's panels and split dividers; existing drawer shapes
connect its selected sidebar icons. Grip menus choose full-column opening,
individual tabbed drawers and Auto-hide. Closed multi-member stacks have no
resize affordance; an open member retains its own resizable width. Fresh Paint
opens the right stack. Saved custom arrangements start closed while retaining
their stack settings and ordinary panel layout.

`tests/column-stacks.swift` checks AppKit mouse/tablet input on both presets,
including held icons, immediate tabs/grips, target distinctions, cancellation and
one-step history. `tests/column-stack-persistence.swift` uses real temporary
workspace libraries for switching, relaunch and persisted Undo/Redo. Run these
with `scripts/test-project-files.sh`. Both native UI targets provide
`testColumnStacks` and `testColumnStacksDark`; physical Pencil validation remains
part of the broader input gate.

Tool Set projects the shared groups and subtools for painting, figures, regions,
rulers and Operation. Every catalog brush remains reachable through its family;
Rust remembers the selected subtool and edited settings when changing groups.
Brush previews fill the row above a right-aligned single-line label, matching Web.
Open tool buttons use the adjoining drawer's panel color; selection keeps its
shared accent highlight.
The Tool panel shows the active tool's numeric fields and actions. Numeric
expressions, units, ranges, slider mappings and stepping resolve through Rust.
The shared Apple control
handles optimistic edits and local validation feedback; small AppKit/UIKit
adapters handle text selection, keyboard focus, Return, Escape and arrow keys.
Native focus changes are deferred until after SwiftUI updates to avoid entering
the hosting responder graph recursively when accepting an expression.

Settings' Done action submits the focused text or numeric field through SwiftUI
before closing, preserving valid drafts even when native focus-loss callbacks
arrive afterward. Result and sidebar navigation release search focus, and the native sidebar
width keeps page labels readable. Shared
text fields ignore unchanged native callbacks so ending editing cannot resubmit
the old query after navigation clears it. Run `tests/settings-text-input.swift` with
`scripts/test-project-files.sh` for both theme-color fields on the shared Apple
presets, including Reset to Default while a text draft is focused and subsequent
Done/reopen. Updated theme-color values replace the focused draft so Done cannot
restore a discarded value. The grouped `testNumericSettingsDone` editor workflow checks expression
entry, Done, reopen, search-result/sidebar navigation and iPad keyboard dismissal
through the actual native Settings window.
Toolbar Color/Opacity controls use their existing drawers; the explicit
configuration popup contains only Color. The obsolete modal opacity path is
removed from the shared action and Apple dialog.

For focused canvas checks without simulator startup:

```bash
cargo test -p layer-apple apple_region_ -- --nocapture
CAPY_TEST_ASSETS_APP=apps/layer-apple/DerivedData/Mac/Build/Products/Debug/CapyCanvas-Mac.app \
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/canvas-native-input.swift
```

The region checks exercise the Apple pointer/action bridge with Metal. The
canvas fixture mounts the assembled editor with its visible panels and overlays.
It sends local AppKit mouse, key and focus events through verified canvas hit
targets for lasso/fill and ruler workflows, checking exact exported pixels and
saved ruler geometry through cancellation and Undo/Redo. New mouse and supplied
standalone tablet contacts also clear stale modifier flags from other controls.
New Drawing uses the
native Discard button and waits for its alert to close. The application's
suspend/resume entry points and actual renderer restart also cancel unfinished
contacts without a mouse-up, ignore stale movement and allow the next lasso,
preserving exact artwork/history. Navigation checks supply AppKit wheel, pinch
and rotation values to the real canvas callbacks, covering scroll units,
modifiers, anchors and contact exclusion/recovery after a cancellation frame.
The test does not sleep the machine or emulate a physical trackpad. Point
`CAPY_TEST_ASSETS_APP` at a built Mac app to supply vector/filter resources.
Both shared Apple configurations run on Mac; these checks do not establish
external OS event posting, OS menu navigation or UIKit/Pencil/tablet delivery.
Group full-application UI runs at milestone boundaries.

The focused iPad `EditorLaunchTests/testTwoFingerCanvasNavigation` workflow
checks UIKit-delivered pinch-in, rotation, a fresh pinch-out, fixed window bounds,
unchanged artwork history and Fit restoration. It hides panels through ordinary
workspace customization: XCTest starts pinch-out near the full canvas element's
corners, which otherwise lie beneath docked controls. This simulator check does
not establish physical finger/Pencil, cancellation or performance acceptance.

`tests/canvas-modifiers.swift` is a standalone UIKit scene application built
with the production Shared/iOS sources, Rust bridge and bundled filters. Its
eighty groups cover mouse/Pencil modifier flags, stale control flags, interruption,
touch identity reuse and palm rejection; saved ruler geometry; all figure and
gradient variants; constrained/free painting with all three ruler types; and
transform edge/corner scaling, movement, rotation, Shift/Alt constraints and
Apply/Cancel.
Cancellation and Undo/Redo compare every decoded PNG pixel. The same suite passes
on simulator and the physical iPad GPU without XCTest, using temporary storage
and supplied event values. `CAPY_INPUT_RUN` identifies the launch in the fixture's
`Documents/canvas-input-result.json`, allowing verification when device console
output is missing. Physical sensors/key delivery, visible editor hit
targets and OS interruption delivery remain separate acceptance checks.

Numeric labels truncate within compact panels, leaving values readable. Spin
fields keep the shared unit suffix when idle, with the value and both step buttons
in one input surface. Sliders use the shared panel/text fill color and straight
progress edge; endpoint buttons receive the shared disabled opacity. Both native
text adapters use tabular digits. The
[numeric-control comparison](../../tools/visual/README.md#numeric-editor-controls)
includes both Apple presets/themes, width and endpoint cases, plus mounted-field
checks of actual editor actions. Full UIKit and editor pixel parity remain open.

Property slider drags use the existing shared effect gesture transaction for
numeric values, layer/Paper opacity and gradient stop controls. Moves
preview without adding history; release commits one Undo step and cancellation
restores the original value while preserving Redo. Ordinary text, step and tap
edits retain their discrete numeric validation path.
New curve points and gradient stops become selected from Rust's published list,
so removal and color/position edits work immediately after insertion. Undoing a
single-point removal selects the restored point. Effect colors and gradient stops
use the shared tagged color form, preserving the original space and exact values
through unchanged and alpha-only edits. Use Color publishes one history edit;
Cancel discards the draft. Shared transforms supply Display P3 previews and gradient
ramps interpolated in the document's encoded RGB space. The canvas, Navigator,
color controls and preview images share that tagged SDR viewing contract; the
OS converts to the current display profile. SwiftUI artwork canvases composite
in extended linear space, preserving P3 chroma and linear alpha. Settings →
Color → Display Details reports the host's observed screen and conversion policy.
`bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/managed-display.swift`
checks native swatches and actual Metal output through document-space changes,
resizes and layer replacement, while preserving saved artwork.

`bash apps/layer-apple/scripts/test-color-input.sh` checks real AppKit text entry
and default-button delivery for all four RGB spaces without a renderer or
simulator. It covers unchanged precision, alpha edits, extended RGB values and
invalid drafts. Modal dismissal and UIKit delivery remain separate UI checks.

The Color panel and brush-color popup expose **Edit Color…** and **Palettes…**.
Paint entry uses the same tagged form, captures its foreground/background target,
and rejects publication into a replacement document. Palette creation, naming,
swatch storage and removal use the shared workspace library; previews never
replace the retained color definition. The former inline RGB sliders are removed.

New Drawing exposes shared presets, dimensions, background and independent
working-space/bit-depth choices. Optional preset/default changes use shared
validation before creating the document on the file worker. The focused
`tests/color-workflows.swift` owner check exercises validation/retry/cancellation,
all four tagged palette spaces, document adoption and fresh-owner persistence
with temporary storage and Metal. Run it through `test-project-files.sh`.
`EditorLaunchTests/testNativeSDRCreationAndPalettes` is the separate native-control
workflow; owner checks alone do not qualify AppKit/UIKit control delivery.
Full-app SDR tests require a Metal adapter with Float32 filtering and blending.
The current iPad simulator lacks Float32 filtering and cannot run these workflows;
use supported physical hardware for rendering-dependent iPad tests. Isolated
UIKit component tests that do not construct this renderer remain useful.

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/property-slider-input.swift
cargo test -p layer-apple property_edits_and_gestures_preserve_exact_metal_history_on_both_platforms -- --test-threads=1
```

The native component check uses local AppKit contacts through the shared property
views for both Apple presets, including insertion/selection/removal, history,
view-removal cancellation and locked layers. The Metal check compares every
artwork pixel through preview, commit, Undo/Redo and cancellation. UIKit delivery
is checked separately by the grouped `testInlineLayerOpacity` editor workflow.

The compact Color panel follows the shared layout down to 128 logical points:
an Okhsv circle, HSV square or HLS triangle; overlapping foreground/background
swatches; transparent paint; Swap; two alternate shape buttons; and a curved
OKLCH/HSB/HLS readout that toggles to RGB. Rust owns the layout, conversion,
readout text, hue memory, hit regions and drag clamping. Both hosts use shared
RGBA8 fields and hue guides converted from the document space, retaining separate
field/guide images across marker changes and invalidating them when the gamut
changes. Native clips, markers and controls remain at display resolution.
Wheel painting follows Web's rounded destination edges, with the field/guide
raster sized to those physical bounds to avoid extra interpolation. Swatch
selection borders sit behind their paint interiors. Each styled swatch has a
circular hit region so the foreground's empty corners do not intercept taps on
the overlapping background swatch. Native button styles match
the shared swatch, shape and Swap hover/press feedback while retaining ordinary
activation and cancellation. The readout uses the shared accent and label outline
when its native focus binding is active.
Curved text uses the shared native font metrics and fractional CoreText glyph
positions, retaining normal font smoothing instead of rounding each rotated glyph.
The two Mac shape icons composite before rotation to keep their outlines smooth.
This standard SwiftUI drawing step is limited to macOS, where complete-panel
captures show an improvement; UIKit uses its validated existing rendering.
The input views latch the starting region for each mouse, Pencil or touch contact.
Picking exits transparent paint through the previous paint slot; changing shape
or paint slot cancels the old contact.

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
`testCompleteEditorCapture` captures the settled Paint workspace with its
default right stack open for the Chrome `paint-expanded` comparison. Its metadata
records logical dimensions and any native window-control clearance. Standalone edit-state
checks need no GUI automation:

```sh
xcrun swiftc apps/layer-apple/Shared/Editor/NumericEditState.swift \
  apps/layer-apple/tests/numeric-edit.swift -o /tmp/capy-numeric-edit
/tmp/capy-numeric-edit
cargo test -p layer-apple apple_tool_panels
cargo test -p layer-apple apple_transform_settings
cargo test -p layer-apple apple_color_
```

The focused `testColorControls` test checks native wheel contacts, shapes,
readout switching, paint slots, Swap and continuous dragging. It retains full
captures plus measured wheel geometry for the shared
[color sampling check](../../tools/visual/README.md). The headless ABI tests
verify resulting brush/eraser pixels and exact Undo without driving menus.

The Color panel shares the Web layout, vector icons and readout spacing on both
Apple targets. Docked wheels shrink to the available viewport height, retaining
the shared 128-point minimum and scrolling below that size. The Paint and Photo default
checks include visibility of the wheel and every corner control. Panel headers,
vertical toolbar footers and collapsed-column footers use one grip drawing with
the same orientation, opacity and inset as Web/Android. The Color
[complete-panel fixture](../../tools/visual/README.md#complete-color-panels)
compares 216 cases per host: both presets/themes, three shapes, two readouts,
three paint slots and three widths. The same source has AppKit and UIKit capture
entry points. The focused UI
workflow records the actual accepted Rust color through opt-in debug metadata,
so its color oracle does not assume ideal touch coordinates.

Properties choice controls share an Apple button/popover projection, sizing the
closed control to the longest option while keeping room for its row label.
The layer blend control exposes its current value to accessibility.
`testEditorControlLayout` checks all six Navigator hit targets, changes a blend
mode through Properties, verifies the Layers value and undoes the change. It
also attaches matching `paint-expanded` and `paint-canvas-under-header` captures. The latter
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
project format. New Drawing supports shared presets, sRGB/Display P3/Adobe RGB/
ProPhoto working spaces, independent 8/16-bit backing and white/transparent
backgrounds. Export supports profiled PNG/TIFF at 8/16-bit and JPEG at 8-bit,
output-size/background choices, comparisons and shared destination/named presets.
Float32 snapshots, color conversion and encoding run on the file worker. Imported
ICC profiles can be reused through the saved library in Export, source-profile
choices and Settings → Color. Unsaved artwork also receives
private recovery copies. Use **File → Recovered Drawings…** to open one; copies
are offered after restart and retain unsaved status until you explicitly save.
GPU failure preserves the editor's CPU session and offers **Restart Canvas** or
**Save As…**. Restart reconstructs the document through the shared renderer API,
retaining history and working settings. Recovery preparation handles queued
pen-up without a drawable; lifecycle success waits for durable publication.
See [PERSISTENCE.md](PERSISTENCE.md) for atomic generations, lifecycle handling,
reproducible checks and remaining physical-device/performance acceptance.

`EditorLaunchTests/testArtworkRecoveryAfterRestart` checks painted artwork,
native background/return to the same scene, fill Undo/Redo, then process restart
and reopening the private recovery copy. It compares sampled pixels and layer
structure. Mac uses Hide/activate; UIKit uses Home/activate. This workflow does
not establish physical pen interruption or memory-pressure termination.

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

The [command review](command-coverage.json) classifies all 63 commands, nine
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
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/editor-appearance.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/native-context-menu.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/native-context-source.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/editor-menu-keyboard.swift
```

The current graph contains 90 tool choices and 28 setting IDs per Apple preset;
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

The Filters category control reuses the shared editor choice and its opaque menu.
Opening a choice focuses its selected enabled row; Escape closes filter search.
The focused native check visits every category, types a search, verifies Escape
and unchanged document state, and captures both themes on both Apple presets:

```sh
CAPY_TEST_ASSETS_APP=apps/layer-apple/DerivedData/Mac/Build/Products/Debug/CapyCanvas-Mac.app \
CAPY_FILTER_CAPTURES=/tmp/capy-filter-controls \
  bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/filter-controls.swift
```

Point `CAPY_TEST_ASSETS_APP` at a built Mac app to supply the shared vector assets.
This check uses local mouse/keyboard events in temporary AppKit windows; it does
not launch the simulator or validate UIKit event delivery or GPU filter previews.

The direct Swift menu check dispatches actual shared menu payloads through the
Apple editor and workspace service on both presets. It checks all nine routes,
form cancellation, manager/history dismissal, host-request acknowledgement,
workspace switch/return and document preservation using isolated storage and
no OS menu automation. The companion manager check exercises confirmed forms
and coordinator workflows. These macOS-hosted checks do not exercise UIKit
widgets. Passing the audit establishes catalog coverage; complete native
workflows, dynamic controls, visual states and hardware performance still
require their own evidence. Save/Load Layout remains excluded.

`editor-appearance.swift` checks explicit Light/Dark, returning to System and
native application appearance changes with an isolated AppKit editor window.
`testPopupThemeFollowsExplicitAndSystem` runs the Settings workflow on either
Apple target and attaches captures for reviewing sheet and popup colors.

The native context-menu checks invoke actual AppKit menu items and verify shared
actions/Undo, availability, checks and shortcuts. The source check opens one
temporary window, waits for native menu dismissal between actions, and verifies
that removing a source retires its pending query; keep that fixture in the
foreground. `testNativeZenContextAction` checks the UIKit source and resulting
Zen layout. `testNativeWorkspaceContextAction` checks the UIKit row menu and its
persisted move action. None of these checks uses Mac system-menu coordinates.
For quick UIKit contact/scroll checks with one booted iPad Simulator, run
`python3 apps/layer-apple/scripts/test-native-rows.py` (use `--simulator` to choose
among several). It builds a disposable callback fixture, requires its completion
marker even if `simctl` exits successfully, and removes its own app afterward.
The fixture covers real scroll-view edge movement in both directions and shared
contact policies; it does not synthesize physical finger/Pencil gestures.
`EditorLaunchTests/testLayerMenuDragUpward` and `EditorMenuChecks` exercise the
UIKit editor on simulator or device destinations with disposable persistence.
The connected iPad passes upward layer dragging with exact Undo/Redo, layer
menu anchors/actions, main menus in both themes, submenus/shortcuts and workspace
menu actions followed by held dragging in both directions. Mac's focused
`testBlendChoices` verifies both blend controls and Undo/Redo in an isolated app.
Both hosts also pass `testWorkspaceSwitcher`, `testToolbarStylesAndActions` and
`testToolbarCustomization`. Toolbar grips include their names in accessibility
labels; Mac's Select All command respects the focused native text editor.
The direct menu keyboard check uses native events in its own Mac window for
arrows, Home/End, Return, Escape, disabled rows and shifted shortcuts. Submenu
pages retain their parent row, so returning from a later submenu restores keyboard navigation
to that row instead of the first enabled item. Accelerators search the complete
menu tree, so an enabled action remains reachable from another page or before
its submenu opens; disabled actions stay inactive. The focused
`EditorMenuChecks/testCompactMenuShortcutAcrossPages` checks Undo/Redo through
the real iPad compact menu; its last result fails and acceptance remains open
in the [handoff](../../docs/development/apple-handoff.md). Menus shrink to the available
window bounds while retaining their native scroller. The fixture verifies
700×500, 360×500 and 700×760 windows, including scrolling to and activating the
last row; set `CAPY_MENU_CAPTURES` to save captures of its owned windows.
The grouped `testBlendChoices` and `testToolbarStylesAndActions` workflows check
actual choice and toolbar menu bounds, selection, actions and history on both
hosts. Menus reuse
`ShortcutKeyCapture`; UIKit restores the preceding responder when a menu closes.
The iPad workflow verifies arrows and command shortcuts. XCTest Escape produced
no UIKit press or key-command callback in a traced first-responder probe; Return
also did not execute the menu action. Those device key checks remain open and
are not inferred from Mac results. Use `EditorActionMenu` and
`editorPopover` for editor menus; install `EditorPopoverHost` at an editor/sheet
root outside clipped panels. Keep input on the existing native hold/pan path;
do not add UIKit menu/drag-session handoffs or per-menu presenters.

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

Both Apple hosts project the shared [workspace title bar](../../docs/ui/window-bar.md).
Window → Customize Title Bar… opens the inline editor. Whole items and bank
chips use native slop with immediate mouse/touch/pen pickup; shared Rust owns
placement, overflow, removal and the single Done history entry. Hidden overflow
items remain editable. The existing multi-select tool picker handles Add Tools.
Cancel restores the arrangement and footer, and closing an unfinished edit does
not save its preview. Mac application menus remain in the OS menu bar.

Clock and Battery are workspace components, replacing the former Apple global
visibility preference. They occupy space only in fullscreen and have editable
placeholders while windowed or when battery data is unavailable. iPad observes
[effective scene geometry](https://developer.apple.com/documentation/uikit/uiwindowscene/effectivegeometry)
and compares its coordinate space with its display; Mac observes native window
fullscreen notifications. Neither observation requests an iPad fullscreen change.

Small, Medium and Large use shared tile/icon dimensions and six-point gaps.
The selector retains its pill background and compacts to a menu when necessary.
Title-bar controls, menu labels and status text use Web's rounded theme-gray
backgrounds over artwork; gaps retain the live canvas. Color shows the live
foreground/background paints. The retired text/icon halo renderer is removed. See the
[header comparison](../../tools/visual/README.md#complete-header-components)
for native/Web captures and recorded host differences.

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

`testTitleBarToolDrawers` exercises fresh Sketch Color/Brush/Layers switching and
toggling. Mac captures verify that native hit testing retains the brush cursor
on canvas and clears it beneath title-bar controls.
`testTitleBarSystemStatus` exercises removal/Cancel and actual fullscreen status.
`testTitleBarCustomization` exercises native bank dragging, multi-selection across
searches, Done and nested picker cancellation. Mac also checks picker Escape;
physical iPad Escape remains an input acceptance item. Focused native-owner checks:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/header-native-input.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/header-persistence.swift
```

The first uses owned AppKit windows for both presets and mouse/pen event streams,
including a minute update during a held drag with stable monospaced clock geometry;
the second uses temporary workspace libraries for switching, reopening and
persisted history. Neither establishes physical Pencil coverage or full visual
acceptance. Inspect current captures at normal viewing size.

Launch a local Mac build with:

```sh
open apps/layer-apple/DerivedData/Build/Products/Debug/CapyCanvas-Mac.app
```

## Validation

See [PERFORMANCE.md](PERFORMANCE.md) for opt-in local CPU/GPU/presentation traces,
the report tool, instrumentation checks and the remaining hardware evidence.
The [native frame correlation guide](PERFORMANCE.md#correlating-native-drawing-frames)
joins exported Instruments GPU work to recorded drawing frames on both physical
Apple hosts, retaining unmatched work and incomplete capture windows.
See [INPUT.md](INPUT.md) for Pencil corrections, shared stroke/history handling,
fast input checks and the physical-device evidence still required.

For UI tests using already installed iPad apps, set `UseDestinationArtifacts`
in the `.xctestrun` target with `TestHostBundleIdentifier`,
`UITargetAppBundleIdentifier` and `TestBundleDestinationRelativePath`.
Omit `TestHostPath`, `TestBundlePath`, `UITargetAppPath` and
`DependentProductPaths`: retained local dependencies can make XCTest attempt
to install an unavailable bundle during `app.launch()`. Tests launch their own
isolated namespaces; no separate prelaunch/attach path is needed.

Use `bash apps/layer-apple/scripts/test-project-files.sh` for routine file
regressions. Its isolated dialog fixtures exercise both Apple configurations,
including New, Save/Open, PNG export, cancellation and preserved file contents.
Run `testNewDrawingAndExportCancellation` when native picker acceptance or a
picker presentation change needs device validation. If Files requires device
authentication, batch necessary picker checks within the authenticated session
to avoid repeated interruptions.

For profiled output and preferences, run
`cargo test -p layer-apple profiled_apple_export` and
`bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/export-owner.swift`.
They cover exact retained U16 PNG/TIFF samples, embedded profiles, previews,
JPEG/resize, cancellation/retry, owner destruction, atomic ICC/preset storage and
unchanged masters. The Mac `testNativeProfiledExport` workflow covers the actual
options, ICC import/library, named preset and native destination panels. Physical
iPad Files/provider delivery and sustained large-photo acceptance remain separate.

Check editor behavior directly without driving system menus:

```sh
cargo test -p layer-host --lib
cargo test -p layer-apple --lib
```

`python3 apps/layer-apple/scripts/test-workspace-scrolling.py` runs the focused
iPad simulator long-list workflow with coordinator-created data, a disposable
app identity and a final persisted-order check. Use `--simulator` to select an
available iPad; results stay under ignored `artifacts/` and owned apps are removed.

The Apple tests dispatch through the real C ABI for both platform configurations.
They check brush/zoom/settings actions, session isolation, committed ink after
pen-up, and exact GPU document pixels through undo/redo. The GPU tests require
hardware Metal access; they do not establish physical input or presentation timing.
`cargo test -p layer-apple lasso_pointer_contacts -- --test-threads=1` checks pen
contacts through that ABI: empty/cancelled paths preserve selection and Redo;
enclosed selection/fill paths change actual document pixels and restore exactly
through Undo/Redo. The native `testLassoControls` check covers control routes and,
on Mac, ordinary clicks followed by Redo and layer editing. Freehand OS input
and physical Pencil acceptance remain separate.
The staged-startup check also verifies pending ink survives the initial paper
frame and stays undoable while document/brush shaders become ready.
Layer checks cover checked selection versus the drawing target, mask targeting,
hierarchy, rename, blend/opacity/locks/references, shared menu enablement, image
import and GPU thumbnails. Imported pixels round-trip exactly through undo/redo.
Numeric expressions and slider mapping use a stateless shared-policy entry point.

Open, Place and Paste use shared import policy and the retained photo decoder
in `layer-color`. Native filters follow its compiled format capabilities.
Decoding keeps original profiles, integer depth and samples; Open selects the
source working space and a separate native Save destination. Place/Paste preserve
the receiving document's space and prepare a complete image batch before entering
shared placement. Apply commits one undoable edit; Cancel removes every member.
Original Size and Apply/Cancel remain available with workspace panels hidden.
The missing-profile preference can pause preparation for an explicit
interpretation, and invalid choices remain editable. Reads use the same
coordinated, security-scoped file access as native projects, off the UI/render
owner. Clipboard items load and decode sequentially without OS bitmap conversion;
a failed member stops subsequent reads and leaves the document unchanged.
External canvas/layer drops remain an integration task.

Run the focused Metal and Swift owner workflows without simulator automation:

```sh
cargo test -p layer-apple tests::photo -- --test-threads=1
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/image-import-owner.swift
```

The large-photo regression runs separately with a disposable, tagged sRGB
9504×6336 JPEG:

```sh
CAPY_APPLE_PHOTO_JPEG=/path/to/photo.jpg cargo test -p layer-apple --lib \
  large_jpeg_gpen_preserves_photo_through_save_and_gpu_recovery -- \
  --ignored --nocapture --test-threads=1
```

It checks original source samples, opaque G-Pen paint tiles, exact Undo/Redo and
save/reopen, then destroys and replaces the Metal device. Both policies execute
on the local Mac; this is data-integrity coverage, not physical iPad acceptance
or sustained presentation/latency measurement. The input JPEG is read-only.

The native checks cover P3/U8 and ProPhoto/U16 source retention through painting,
history and native save/reopen, interpretation retry, promotion policy, cancellation
and stale document/target/device rejection. The Swift fixture uses temporary files
and both Apple policies to exercise coordinated reads, picker cancellation,
encoded Paste, errors, history and source-safe Save. Shared codec tests own EXIF,
channel, alpha and admission-limit checks. These owner tests do not establish
native picker/clipboard-provider delivery or cloud access. The Mac UI workflow
`testNativeImageImport` covers the actual picker, cancellation, the imported layer
name, sampled artwork and Undo/Redo.

Assign Profile, Convert Color Space, Change Bit Depth and Document Properties
also use the native document worker. Shared code owns color semantics and exact
Undo/Redo; the worker prepares complete before/after compositions and a replacement
renderer before atomic publication. Original photo samples remain retained. A
flattened converted copy has a separate destination and leaves the editable
drawing unchanged. AppKit uses its Save panel; iPad uses a folder choice and
filename, rejecting an existing file rather than overwriting it without consent.

```sh
cargo test -p layer-apple tests::document_color -- --test-threads=1
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/document-color-owner.swift
```

These checks exercise both Apple policies: painted documents, exact pixels and
history, retained source samples, save/reopen, cancellation, stale adoption,
copy protection and failure/retry. `testNativeDocumentColor` separately checks
native forms, complete previews, Apply/Cancel, history and Properties. Physical
iPad folder/provider delivery remains a device acceptance case.

Repair Source Profile and Rasterize Source share this coordinator and comparison
form. Shared source rules preserve original samples when changing interpretation,
add a corrected original when a layer already has pixel edits, and rasterize the
full image extent without replacing existing paint/masks. The native owner only
validates and publishes shared edits; conversion, ICC inspection and GPU previews
remain on the file worker. Both repair and missing-profile forms can import an
ICC file using coordinated access, bounded input and shared CMM validation.
Imported bytes remain exact. Saved profile-library management is still pending.

```sh
cargo test -p layer-apple tests::source -- --test-threads=1
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/source-edit-owner.swift
```

The focused Metal checks cover U16 original tiles, painted edits, masks,
off-canvas extent, exact history/save/reopen, continued painting and cancelled or
stale publication. Swift owner checks cover both policies and ICC read/retry;
`testNativeSourceEditing` separately exercises Mac controls and the ICC picker.
Physical UIKit interaction and provider delivery remain separate acceptance cases.

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

A mounted AppKit/Metal regression withholds presentation notifications while
checking startup, rendered artwork and exact thumbnail Undo/Redo on both Apple
policies. This verifies progress independently of display-timing callbacks:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/presentation-progress.swift
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
keyboard is open. The Chrome `filter-properties` scenario in
[`tools/visual`](../../tools/visual/README.md) provides the corresponding property
fixture.

Use `testFilterArtworkAndHistory` for brightness expressions, Red-channel curve
insertion/selection/dragging/removal/reset, Gradient Map reversal/color editing
and stop insertion/selection/dragging/removal/reset, filter deletion and one-step
Undo/Redo. Curve-point selection also preserves pending Redo; dragging retains
the original grab offset.
Both hosts create the drawing through native Select/Fill commands, compare exact
8-by-8 displayed canvas samples and retain
full editor screenshots.
These checks exercise mouse/touch controls; they do not establish Pencil input
or all-pixel filter parity. Both workflows use the native test runner above;
the obsolete direct-event Mac probe is removed.

Use `testMaskActionsAndHistory` for selection-based mask creation/replacement,
copy/paste, enable/invert, reveal/hide all, mask-area inspection, apply/delete and
one-step Undo/Redo. It creates bounded artwork through native Select/Fill and
numeric transform controls, checks four exact 8-by-8 canvas samples across the
mask boundary, and retains full editor captures. Copy must preserve pending
Redo, and paste must also work on a second layer. This supplements
`testMaskTransforms`; physical Pencil interaction remains separate.

Use `testLayerContentActionsAndHistory` for independent duplicates, clearing,
alpha-locked fills, editing-lock capabilities, clipping and new clipping layers.
It also duplicates and deletes a selected clipping stack, checking layer counts,
exact sampled artwork and one-step Undo/Redo. The original bounded paint layer
must survive recoloring and deleting its copies. Both hosts use native menus,
selection controls and numeric transforms, with full editor captures retained.

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

## Histogram and color sampling

View → Histogram opens a nonmodal inspector on both Apple hosts. RGB/luminance,
linear/log chart scaling, clipping/endpoints, manual Refresh and debounced Auto
update use the shared full-resolution committed composite. Visible paper counts;
transparent pixels and display/selection overlays do not. Animated effects report
the captured time. Closing, replacing the drawing or editing it cancels obsolete
worker results. Capture retains immutable project/GPU state and never a session
pointer; GPU inspection runs on the existing document worker.

Eyedropper exposes Point, 3×3 and 5×5 sampling for Visible color and Layer color.
Shared sampling averages premultiplied document-linear color and coverage before
unassociating. Paint retains the document tag and brush opacity stays independent.
An explicit aligned single-row copy pitch fixes omitted Metal readback rows;
there is no separate Apple averaging or color-conversion path.

```sh
cargo test -p layer-apple tests::inspection -- --test-threads=1
cargo test -p layer-render-wgpu sampling -- --test-threads=1
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/inspection-owner.swift
```

The native `testNativeHistogram` workflow checks editor changes while inspection
is open, manual/automatic refresh, channels/log scale, close/reopen and sample
controls. Physical Pencil sampling and sustained inspection performance remain
part of final device acceptance.

## Retained photo corrections

Exposure, White Balance, Levels, Curves, Hue / Saturation and Color Balance use
the shared Filters catalog and Properties controls. Corrections and masks stay
separate from the retained photo; reset, bypass and history use shared actions.

```sh
cargo test -p layer-apple tests::correction -- --test-threads=1
```

This Metal check covers both Apple policies with P3/U8 and ProPhoto/U16 drawings:
local selection masks, exact save/reopen/history and later re-editing preserve
original U16 photo samples, including off-canvas pixels. The native Mac
`testNativePhotoCorrections` workflow exercises actual controls, mask actions,
local file panels and re-editing after reopen. Physical SDR device qualification
remains part of final acceptance.

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
Collapsed columns, tabbed content drawers and child tool drawers share native
projections on both targets. Content-panel tiles
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

For native AppKit mouse/tablet contacts, run these fixtures one at a time; each
owns a temporary foreground window and isolated storage:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-native-input.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/layer-row-input.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/workspace-switcher-input.swift
```

The workspace check covers held tiles, immediate drawer tabs, continuous moves,
menus, cancellation and exact Undo/Redo on both Apple presets. It includes
repeated injected event numbers: pan and press share the mouse-down event object,
while separate contacts must reclassify their visible source. These fixtures do
not establish physical Pencil or tablet-sensor acceptance.

Debug-only `CAPY_INITIAL_ACTIONS` fixtures run once after restoration and the first
native surface size, so layout actions use the editor's viewport instead of the
owner's 1×1 placeholder. Release builds have no fixture override.

`testCollapsedColumnsDrawersAndZen` exercises column tabs, a child drawer,
outside dismissal and restoring the column; `testZenHidesChromeAndTabRestoresIt`
checks full Zen and its keyboard exit. The removed partial-Zen toolbar projection
and Total Zen preference have no native UI or test paths.
Full interaction, visual, physical input and performance acceptance remain open
on both targets.

The opt-in iPad `testNativeFilesProjectRoundTrip` uses a fresh UUID supplied as
`CAPY_FILE_TEST_TOKEN` in the test runner environment. It creates a matching
folder in On My iPad, saves generated artwork, reopens it after restarting the
editor, and exports a PNG. Painting runs through enabled native menus after Metal
is ready; the reopened drawing must retain three layers and matching sampled
pixels. The current simulator run also verifies the delivered PNG separately:
all 3,145,728 pixels retain the opaque blue artwork at 2048×1536. This establishes
local Files delivery, not physical or cloud-provider acceptance. Evidence is in
`artifacts/apple-files-artwork-v1/`. Retain the token with ignored local evidence. After
reviewing the result, run `testNativeFilesProjectRoundTripCleanup` with the same
token to remove that folder through Files. Cleanup verifies the expected two
items before deletion. Routine tests skip both native provider checks unless
explicitly selected and configured.

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

For a fresh environment, start with the concise
[Mac/iPad testing and debugging handoff](../../docs/development/apple.md#testing-and-debugging-on-local-hardware).
