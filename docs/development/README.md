# Developer guide

[Technical documentation](../README.md)

Start by building one client. Shared Rust changes can usually be developed with
that client and focused crate tests; you do not need every platform SDK installed.
The [architecture guide](../architecture.md) explains the code boundaries before
you choose where to make a change.

## Build a client

Run the commands in these guides from the repository root unless stated otherwise.
Use a recent stable Rust toolchain. The workspace uses Rust 2024, and dependencies
may require a newer compiler than the edition's minimum. Keep `Cargo.lock` intact
when reproducing a build.

| Platform | Setup and build instructions |
| --- | --- |
| Linux | [GTK4/libadwaita, Wayland and Vulkan](linux.md). This is the primary UI development client. |
| Web | [WebAssembly, DOM and WebGPU](web.md), with a separate [static/PWA packaging reference](web-packaging.md). |
| Android | [Kotlin/Compose, Android SDK/NDK and Rust JNI](android.md). |
| macOS and iPadOS | [Xcode, AppKit/UIKit, Swift and the Rust Metal bridge](apple.md). |
| Windows | [WinUI 3, C++/WinRT and the Rust D3D12 bridge](windows.md). |

Painting requires a hardware GPU. Building code or running pure model tests does
not establish that a machine can run the canvas. Native clients also need their
platform's windowing environment and SDK; a workspace-wide build is not a
substitute for the platform build scripts.

## Find the right place to change

| Change | Start here |
| --- | --- |
| Layer semantics, edit history or saved drawing data | [Documents and edits](../internals/documents.md), then `layer-core`. |
| Stroke placement, pressure or brush dynamics | [Brushes](../internals/brushes.md), then `layer-engine`. |
| Pixel operations, blend behavior or GPU performance | [Rendering](../internals/rendering.md), then `layer-render-wgpu`. |
| Tools, commands, docking or customization | [Workspace and UI](../ui/README.md), then `layer-ui` and the affected frontend. |
| Preferences or shortcut rules | [Settings](../ui/settings.md), then the shared definitions. |
| Native widgets, input collection, surfaces or file pickers | [Platform integration](../platforms/README.md), then the relevant app under `apps/`. |
| A runtime filter | The [JSON/WGSL contract](../reference/runtime-filters.md) and [Tent Blur example](../../examples/filters/tent-blur). |

`layer-ffi` exposes a C API for embedding and the headless harness. It is not the
universal entry point for every application; native bridges live with their hosts.
The shared UI can also be called directly from Rust or WebAssembly bindings.

## Validate a change

The [testing guide](testing.md) separates model tests, native interaction checks
and GPU measurements. Use checks relevant to the behavior you changed, then test
on the affected hosts. A shared test cannot establish native widget parity or
physical pen behavior.

Generated builds, screenshots and traces belong in ignored output directories,
not in source commits. The [publication guide](publication.md) covers licensing
and distribution checks. Contribution priorities are in the
[root README](../../README.md#contributing).
