# macOS and iPadOS development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Apple clients use AppKit on macOS and UIKit on iPadOS. They share Swift editor
components and a Rust bridge built around `NativeHost`. Metal renders the canvas
through a `CAMetalLayer` behind native controls.

## Prerequisites

Use an Apple Silicon Mac with Xcode, the required iOS simulator runtime, Python 3
and Rust. The current build script targets arm64 macOS, arm64 iPad devices and the
arm64 iOS simulator:

```bash
rustup target add aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim
```

The script defaults to `/Applications/Xcode.app/Contents/Developer`. Set
`DEVELOPER_DIR` if Xcode is elsewhere. A physical iPad build also needs a signing
team and a provisioned device.

## Build

```bash
bash apps/layer-apple/scripts/build.sh macos
bash apps/layer-apple/scripts/build.sh simulator
```

For a device build:

```bash
CAPY_APPLE_TEAM=YOUR_TEAM_ID bash apps/layer-apple/scripts/build.sh device
```

These commands generate resources and `apps/layer-apple/CapyCanvas.xcodeproj`,
then invoke Xcode. They build the app; they do not install it on a device. Open
the generated project in Xcode to select a run destination and launch the
`CapyCanvas-Mac` or `CapyCanvas-iPad` scheme.

Set `CAPY_CONFIGURATION=Release` for optimized Swift and Rust builds.
`CAPY_DESTINATION='id=DEVICE_UDID'` selects a device build destination, and
`CAPY_DERIVED_DATA` changes the output directory. macOS builds use ad-hoc signing
unless a team is supplied.

## How the hosts work

The shared Swift components present Rust tool, layer and workspace models. AppKit
provides desktop windows, menus and tablet/mouse input; UIKit provides iPad controls
and Pencil events. Platform display callbacks drive presentation, with GPU work
kept separate from native control updates.

New editor sessions use the shared full editor workspace and grouped tool
catalog, matching the web preset. Saved workspaces retain their existing layout
and toolbar contents. The shared Swift views project those Rust models on both
Apple targets; native adapters handle focus, input and platform services.

The iPad adapter forwards coalesced and predicted touches and later updates to
estimated Pencil samples. Those corrections use the shared stroke model rather
than creating an Apple-specific brush implementation. macOS and iPadOS also have
different file services and window lifecycles despite sharing the Metal bridge.

## Validation status

Build success does not establish complete input or UI parity. The
[Apple acceptance record](../history/apple-acceptance.md) tracks the two clients,
including physical Pencil checks, file integration and remaining performance work.
The [Apple host notes](../../apps/layer-apple/README.md) contain focused test and
capture commands.
