# Capy Canvas for Apple platforms

Native UIKit/iPadOS and AppKit/macOS targets share Swift editor components and
the Rust Metal bridge. The bridge uses `crates/layer-host`, also used by Android;
the document, engine, UI policy, catalog and rendering remain in shared Rust.
The full-window Metal layer stays behind editor controls and the header.

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

## Launch validation

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
a landscape screenshot. It does not measure drawable presentation or prove full
visual/functional parity. Physical Pencil and performance acceptance are separate.

## Implementation status

This is an editor-shell milestone, not a finished port. The simulator renders
the live canvas; the iPad target builds, signs and installs; the AppKit target
builds. Initial shared menus, tool buttons, brush/size presets, basic layer
selection/visibility/opacity and ordinary native settings rows are wired.

Still required: complete panel/drawer/menu/dialog behavior and customization,
filters/properties and other specialized controls, complete settings/shortcut
UI, document and preference/workspace persistence, Pencil estimated-property
corrections, complete hover/sensor/shortcut routing, platform lifecycle coverage,
full pixel-difference validation, and measured M4 iPad performance acceptance.
The Mac target currently provides presentation and shared UI; its native input
and platform services remain future work.

Keep the complete acceptance scope in [the iPad acceptance tracker](../../docs/ipados-acceptance.md).
