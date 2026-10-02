# Platform integration

[Technical documentation](../README.md) · [Architecture](../architecture.md)

A platform client provides widgets, input, a GPU surface and operating-system
services around the shared editor. Every client uses the same document, brush
and canvas-rendering implementation. Sharing that code does not eliminate native
integration work or guarantee that every frontend exposes the same features yet.

## Development workflow

New editor features are developed first on Linux with Wayland. Coding agents
adapt the GTK interface to DOM controls in the web client, using the shared Rust
core compiled to WebAssembly. Further agents use the web implementation as a
reference when adapting the interface to Android, Apple and Windows toolkits.
Native clients compile the shared core for their own targets; they do not run the
WebAssembly build or embed the browser interface.

The web client provides a reference on each target platform through a browser
with hardware WebGPU. Compare it with GTK on Linux, then compare each native
client with that reference. Use the same document, workspace state, theme and
viewport dimensions for screenshots,
and repeat the same commands and interactions to check behavior. This supports
detailed UI parity checks at each porting step. Pen input, window lifecycle and
other OS integration still need tests on the actual host.

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
their app directories.

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
alpha. Android must handle `SurfaceView` recreation and application suspension;
see [Android presentation](#android-presentation).
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

## Android presentation

Android draws pen strokes into one retained Vulkan shared-demand image
(`VK_KHR_shared_presentable_image`), redrawing only the damaged regions. The
display can read while the app writes, so ink can tear; the trade is lower pen
latency. Camera changes switch to FIFO presentation and stay buffered until the
next paint contact, which switches back before any brush GPU work is submitted.
Buffered frames retain their acquired image until the existing completion
callback is polled, then present it on the next canvas tick. The canvas owner can
receive input and actions while the GPU works. Surface retirement discards any
held image before releasing the swapchain.
Each switch resets damage history, so its first frame is a full redraw. There is
no buffered fallback: a driver without shared presentation fails to initialize.
The swapchain is pre-rotated to the display's native orientation, and the
changes to wgpu's HAL are listed in [vendor/README.md](../../vendor/README.md).

The `capy-canvas` thread owns the Rust session, the wgpu device and the
swapchain; Compose stays on the main thread. Actions and models cross JNI as
JSON, and `Native.modelUpdate` sends only changed paths
([model_update.rs](../../crates/layer-host/src/model_update.rs)). The system bars
stay hidden, and the workspace reserves only display cutouts, so bar animations
never resize the Vulkan buffer. An opaque cover hides the `SurfaceView` until
the first frame of each surface generation completes. Surface loss keeps the
session and document.
