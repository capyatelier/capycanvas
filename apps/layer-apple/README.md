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

Launch a local Mac build with:

```sh
open apps/layer-apple/DerivedData/Build/Products/Debug/CapyCanvas-Mac.app
```

## Validation

Check editor behavior directly without driving system menus:

```sh
cargo test -p layer-host --lib
cargo test -p layer-apple --lib
```

The Apple tests dispatch through the real C ABI for both platform configurations.
They check brush/zoom/settings actions, session isolation, committed ink after
pen-up, and exact GPU document pixels through undo/redo. The GPU tests require
hardware Metal access; they do not establish physical input or presentation timing.

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
clearance, mouse stroke termination and keyboard undo/redo:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  xcodebuild -project apps/layer-apple/CapyCanvas.xcodeproj \
  -scheme CapyCanvas-Mac -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath apps/layer-apple/DerivedData/Mac \
  -resultBundlePath artifacts/ui/parity/mac-launch.xcresult \
  DEVELOPMENT_TEAM=YOUR_TEAM_ID CODE_SIGN_IDENTITY='Apple Development' test
```

Use a fresh result-bundle path. The iPad and Mac targets share frame admission,
including wake preservation while a frame is queued, and flush final UI state
before going idle. Each window owns its own session. Reattaching a Metal layer
does not reload bundled filters over a session's edited filter library.
Use direct window captures and editor action/state/output checks for routine
parity work. Coordinate testing within the app is appropriate for custom canvas
and control behavior when useful. Reserve UI automation for targeted app
regressions; macOS menu mechanics are trusted platform behavior and are not
tested with coordinate clicks.

Use [the shared visual tools](../../tools/visual/README.md) for matching local
Chrome captures and complete image differences for either native target.

## Implementation status

This is an editor-shell milestone, not a finished port. The simulator renders
the live canvas; the iPad target builds, signs, installs and launches; the AppKit
target builds and launches, with mouse drawing and keyboard undo/redo checked.
Both targets share frame admission and per-window session ownership. Initial
shared menus, tool buttons, brush/size presets, basic layer
selection/visibility/opacity and ordinary native settings rows are wired.

Still required: complete panel/drawer/menu/dialog behavior and customization,
filters/properties and other specialized controls, complete settings/shortcut
UI, document and preference/workspace persistence, Pencil estimated-property
corrections, complete hover/sensor/shortcut routing, platform lifecycle coverage,
full pixel-difference validation, and measured iPad and Mac hardware performance.
Mac tablet/proximity, mouse, wheel, trackpad and keyboard adapters are present;
physical tablet sensors and the full shortcut/lifecycle contract still require
validation. Both platforms must complete the expanded acceptance matrix.
