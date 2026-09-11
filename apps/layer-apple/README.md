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
See [the Apple goal and acceptance tracker](../../docs/apple-acceptance.md)
for the shared-code boundaries, milestone matrix and remaining work.

## Build

Install Xcode with the iOS SDK/simulator runtime and these Rust targets:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin
bash apps/layer-apple/scripts/build.sh simulator
bash apps/layer-apple/scripts/build.sh macos
CAPY_APPLE_TEAM=YOUR_TEAM_ID bash apps/layer-apple/scripts/build.sh device
```

Run from the repository root. The scripts select `/Applications/Xcode.app`
unless `DEVELOPER_DIR` is supplied, generate shared resource bundles and the
deterministic Xcode project, then build. No global project generator is needed.
`CAPY_CONFIGURATION=Release` selects optimized Swift and Rust builds.
`CAPY_DESTINATION='id=DEVICE_UDID'` selects a particular physical device.
`CAPY_DERIVED_DATA` optionally separates concurrent build directories.
Mac builds use ad-hoc signing by default. Set `CAPY_APPLE_TEAM` for development
signing with an installed certificate; Mac UI test runners must also be signed.

After generating once, open `CapyCanvas.xcodeproj` to build/debug in Xcode.
Rerun `scripts/prepare.py` when shared assets change and `scripts/project.py`
when adding or removing Swift files. Edit those generators rather than generated
project entries. Canonical icons/previews are currently in `apps/layer-web`;
the Apple bundle stages them and the root filter library without separate art.

Tool Set consumes shared figure, region, ruler and Operation choices. Painting
keeps the complete catalog brush list, also used by the web host, so every brush
remains reachable alongside custom toolbar tools.
Enable **Workspace → Tool Settings panel** for the
active tool's numeric fields and actions. Numeric expressions, units, ranges,
slider mappings and stepping resolve through Rust. The shared Apple control
handles optimistic edits and local validation feedback; small AppKit/UIKit
adapters handle text selection, keyboard focus, Return, Escape and arrow keys.

Enable **Workspace → Color panel** for the shared HSV square / HLS triangle,
foreground/background/transparent paint slots, swap and component expressions.
Rust owns color conversion, hue memory, normalized geometry, hit regions and
drag clamping. Both native hosts share the gradient drawing and controls; their
small input views latch the starting region for each mouse, Pencil or touch
contact. Picking a color exits transparent paint using the previous paint slot.
Channel edits update the current Rust state, preserving other queued changes.

For reproducible Debug editor fixtures, `CAPY_INITIAL_ACTIONS` accepts a JSON
array of shared actions at launch. For example, this opens Tool Settings without
driving the Mac system menu bar:

```sh
open -n --env CAPY_INITIAL_ACTIONS='[{"type":"customize","action":{"type":"set_panel_visible","panel":"tool_settings","visible":true}}]' \
  apps/layer-apple/DerivedData/Build/Products/Debug/CapyCanvas-Mac.app
```

Release builds ignore this variable. The focused `testNumericToolControls` test
uses the same fixture on both platforms. Standalone edit-state checks need no
GUI automation:

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
readback and PNG encoding run on the file worker; automatic artwork recovery
remains pending.

Both targets now project the live shared application menus. Keyboard Shortcuts
supports search, alternate bindings, conflict replacement and resets; Settings
search uses the shared results. The focused
`EditorLaunchTests/testShortcutConflictAndEditorEffect` test exercises capture
and the resulting Zen action without automating the system menu bar. The shared
inventory command emits representative menu and shortcut states:

```sh
cargo run -p layer-host --example inventory > /tmp/capy-inventory.json
```

Launch a local Mac build with:

```sh
open apps/layer-apple/DerivedData/Build/Products/Debug/CapyCanvas-Mac.app
```

## Validation

See [PERFORMANCE.md](PERFORMANCE.md) for opt-in local CPU/GPU/presentation traces,
the report tool, instrumentation checks and the remaining hardware evidence.

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

The shared Swift scheduler has a fast standalone check, with an asynchronous
native-owner test double and no application launch:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun swiftc -parse-as-library \
  apps/layer-apple/Shared/Bridge/CanvasFrameDriver.swift \
  apps/layer-apple/tests/frame-driver.swift -o /tmp/capy-frame-driver-tests
/tmp/capy-frame-driver-tests
```

Use launch tests for native surface geometry and targeted event-delivery checks:

```sh
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
xcrun simctl list devices available
xcodebuild -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-iPad -destination 'platform=iOS Simulator,id=SIMULATOR_ID' \
  -derivedDataPath apps/layer-apple/DerivedData/Simulator \
  -resultBundlePath artifacts/ui/parity/ipad-launch.xcresult \
  CODE_SIGNING_ALLOWED=NO test
```

Choose a fresh result-bundle path for subsequent runs. The test waits for a
successful Metal viewport submission, checks Zen-button dimensions, and retains
a full-screen landscape screenshot with full-window canvas geometry checks.
It does not measure drawable presentation or prove full visual/functional parity.
Physical input and hardware performance acceptance are required on each platform.
The optional Mac test exercises the app's full-window canvas, window-control
clearance, mouse stroke termination and keyboard undo/redo. Both launch tests
also exercise the same small layer workflow: add a layer, check a different row
without changing the drawing target, add a mask, switch content/mask targets,
and delete the mask through its own context menu. Mac uses an in-app right click;
iPad uses a long press. No system-menu clicks are required.

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  xcodebuild -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-Mac -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath apps/layer-apple/DerivedData/Mac \
  -resultBundlePath artifacts/ui/parity/mac-launch.xcresult \
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
Navigator uses Rust geometry, camera actions and an event-driven preview owner:
one GPU readback at a time, a 256px maximum image dimension, at most 15 updates
per second, and one worker decode awaiting UI acknowledgement. Camera-only
changes reuse the image; the last document update remains scheduled through the
refresh throttle. Images belong to a document epoch and survive editor teardown
without borrowing GPU resources. Diagnostics queries the shared rows and bounded
chart at 5Hz only while visible. Timing sampling is restored on GPU attachment
and document replacement.

`EditorLaunchTests/testNavigatorAndDiagnostics` is the focused shared UI check
for either scheme. It uses a disposable drawing, in-app navigation buttons and
an overview drag, then switches to Diagnostics and back. The faster Metal
regressions run with `cargo test -p layer-apple navigator -- --test-threads=1` and
verify actual document pixels, history, preview ownership, final delivery and
document replacement on both Apple platform policies. Full visual and physical
performance acceptance remains in [the matrix](../../docs/apple-acceptance.md).

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
Collapsed columns/drawers, remaining panel projections and complete interaction
and visual acceptance remain open on both targets.

This is an editor-shell milestone, not a finished port. The simulator renders
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
