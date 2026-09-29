# Developer guide

[Technical documentation](../README.md)

Start by building one client. Shared Rust changes can usually be developed with
that client and focused crate tests; you do not need every platform SDK. Read the
[architecture guide](../architecture.md) before choosing where to make a change,
and follow the rules in [`AGENTS.md`](../../AGENTS.md) and the
[commit guide](../COMMIT_GUIDE.md).

## Working here

| Guide | Covers |
| --- | --- |
| [Environment](environment.md) | Worktrees, build directories, build profiles, test state, ports and environment variables on a shared machine. |
| [Devices](devices.md) | The test tablets and how to reserve them, the Windows VMs and PC, Apple hardware. |
| [Testing](testing.md) | Which checks to run for each kind of change, and known failures on `main`. |
| [Writing](writing.md) | Where docs belong, their style, UI text and handoffs. |
| [Publication](publication.md) | Licensing, dependencies and distribution checks. |
| [GPU benchmark workloads](gpu-raster-benchmarks.md) | Offscreen GPU workloads for comparing revisions. |
| [Stroke recording](stroke-recording.md) | Recording tablet input and replaying it for predictor comparisons. |

## Build a client

Run commands from the repository root unless a guide says otherwise. Use a recent
stable Rust toolchain; the workspace uses Rust 2024. Pass `--locked` and keep
`Cargo.lock` intact.

| Platform | Guide |
| --- | --- |
| Linux | [GTK4/libadwaita, Wayland and Vulkan](linux.md). The primary UI development client. |
| Web | [WebAssembly, DOM and WebGPU](web.md), with [static/PWA packaging](web-packaging.md). |
| Android | [Kotlin/Compose, Android SDK/NDK and Rust JNI](android.md). |
| macOS and iPadOS | [Xcode, AppKit/UIKit, Swift and the Rust Metal bridge](apple.md). |
| Windows | [WinUI 3, C++/WinRT and the Rust D3D12 bridge](windows.md), or [a VM from Linux](windows-vm.md). |

Painting requires a hardware GPU. Building code or running model tests does not
show that a machine can run the canvas, and a workspace-wide build is not a
substitute for the platform build scripts.

## Find the right place to change

| Change | Start here |
| --- | --- |
| Layer semantics, edit history or saved drawing data | [Documents and edits](../internals/documents.md), then `layer-core`. |
| Stroke placement, pressure or brush dynamics | [Brushes](../internals/brushes.md), then `layer-engine`. |
| Pixel operations, blend behaviour or GPU performance | [Rendering](../internals/rendering.md), then `layer-render-wgpu`. |
| Tools, commands, docking or customization | [Workspace and UI](../ui/README.md), then `layer-ui` and the affected client. |
| Preferences or shortcut rules | [Settings](../ui/settings.md), then the shared definitions. |
| Native widgets, input collection, surfaces or file pickers | [Platform integration](../platforms/README.md), then the client under `apps/`. |
| A runtime filter | The [JSON/WGSL contract](../reference/runtime-filters.md) and the [Tent Blur example](../../examples/filters/tent-blur). |
| Porting a feature to Apple or Windows | The [Apple](../APPLE_PORTING_GUIDE.md) and [Windows](../WINDOWS_PORTING_GUIDE.md) porting guides. |

New UI is built on GTK first, then Web, then the native clients; the
[platform guide](../platforms/README.md#development-workflow) explains the order
and how parity is checked.

## Work in progress

- [Photo editing roadmap](photo-editing-roadmap.md) lists what is left of the
  [photo editing build list](../history/photo-editing-research.md) after M4, and
  [photo editing performance](photo-editing-performance.md) the performance and
  memory gates M2–M4 did not meet.
- [Canvas action bar and transforms](canvas-action-bar-transforms.md) is Phase 1
  of the same list.
