# Platform integration

[Technical documentation](../README.md) · [Architecture](../architecture.md)

A platform client provides widgets, input, a GPU surface and operating-system
services around the shared editor. Every client uses the same document, brush
and canvas-rendering implementation. Sharing that code does not eliminate native
integration work or guarantee that every frontend exposes the same features yet.

## Clients

| Client | UI and canvas integration | Setup |
| --- | --- | --- |
| Linux | GTK4/libadwaita; Vulkan on an app-owned Wayland subsurface, with a dedicated render worker. | [Linux](../development/linux.md) |
| Web | DOM controls; Rust/WebAssembly and WebGPU on the browser event loop. | [Web](../development/web.md) |
| Android | Kotlin/Compose; JNI and `NativeHost`; Vulkan in a `SurfaceView`. | [Android](../development/android.md) |
| macOS | AppKit and shared Swift components; `NativeHost` and Metal in a `CAMetalLayer`. | [Apple](../development/apple.md) |
| iPadOS | UIKit and shared Swift components; the same Rust Metal bridge, with Pencil-specific input handling. | [Apple](../development/apple.md) |
| Windows | C++/WinRT and WinUI 3; `NativeHost` and D3D12 in a `SwapChainPanel`. | [Windows](../development/windows.md) |

GTK and web use `UiSession` directly. Android, Apple and Windows share additional
session and renderer integration through `layer-host`, with JNI or C bindings in
their app directories. `layer-ffi` is a separate C interface used by the headless
harness and available for embedding; it is not the common bridge for all clients.

## Input and UI ownership

Adapters collect native samples promptly, normalize their units and timestamps,
preserve chronological history, and identify predictions or corrections. The
shared engine owns pressure response, stroke placement and prediction policy.
[Input and stroke feedback](../internals/input.md) explains that boundary.

Rust supplies typed commands, availability and semantic layouts. The host creates
widgets and handles focus, accessibility, text editing and native allocation.
Controls should update from the shared changed-state notifications rather than
reconstructing the entire workspace for every pointer sample.

## Surfaces and timing

Each platform owns surface creation, resize, scale, lifecycle and presentation.
The shared renderer draws the canvas using the device selected for that surface.
Native GPU waits must stay off the UI input path. On the web, work follows browser
animation callbacks and cannot assume blocking waits or a separate thread.

Linux currently requires Wayland, Vulkan mailbox presentation and premultiplied
alpha. Android must handle `SurfaceView` recreation and application suspension.
Apple uses its native display callbacks and Metal-layer lifecycle. Windows must
coordinate swap-chain attachment and reconfiguration with the WinUI thread.
Browser GPU availability and event delivery depend on the browser and device.

The shared renderer has no software-painting fallback. Use the [performance guide](../development/testing.md#performance) to check frame
pacing on the target hardware.

## Storage and feature coverage

The shared session defines preferences, workspace serialization, project requests
and save/close policy. Hosts perform storage, native pickers and other services.
Local files, Android document-provider URIs and browser APIs have different access
and replacement guarantees.

GTK, Android, Apple and Windows have native project workflows. The web client
connects the shared `.capy` workflow to browser file access and download handling.
UI coverage and hardware validation still vary independently of shared tool
implementation.

The port records describe their measured checkpoints:

- [Android feature parity](../history/android-feature-parity.md) includes device
  workflow checks; the [implementation record](../history/android-implementation.md)
  also retains earlier emulator results.
- [Apple acceptance](../history/apple-acceptance.md) tracks macOS and iPadOS
  separately, including Pencil and native file behavior.
- [Windows implementation](../history/windows-implementation.md) records current
  integration gaps and the distinction between replay and real OS input tests.
- [GTK/web implementation](../history/ui-implementation.md) records earlier
  workspace and surface checks.

Treat these as evidence for particular revisions and devices. Build success,
shared model coverage and visual parity are separate claims.
