<p align="center">
  <a href="https://capycanvas.art/">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="docs/assets/capy-dark.svg">
      <source media="(prefers-color-scheme: light)" srcset="docs/assets/capy-light.svg">
      <img src="docs/assets/capy-light.svg" width="112" height="112" alt="Capy Canvas">
    </picture>
  </a>
</p>

<h1 align="center">Capy Canvas</h1>

<p align="center">
  <a href="https://capycanvas.art/">Website</a> ·
  <a href="https://capycanvas.art/download/">Downloads</a> ·
  <a href="https://capycanvas.art/docs/">Documentation</a> ·
  <a href="https://editor.capycanvas.art/">Web&nbsp;Demo</a>
</p>

Capy Canvas is a free art and image editor in development for digital painters,
photographers and comic artists. It is built for Linux first, where artists have
long had fewer choices in professional software, but also works on Android, iPad,
Windows, Mac, and the web.

Its GPU-accelerated brush and compositing engines are
designed to improve performance and battery life, particularly on mobile devices.
A fully customizable interface lets artists adapt layouts and shortcuts to match
the muscle memory they have developed in other apps.

This project is fully free and open source, and keeps artists in control of their
own data. There are no accounts, subscriptions or tracking. Drawing and editing
happen locally on your device, and the code is licensed under MIT or Apache-2.0.

## Overall architecture

When designing Capy Canvas, we did not want to compromise on UI responsiveness.
Controls and pen input need to run at 120 fps on every supported platform, even
while the drawing engine handles large brushes and hundreds of layers.

For the best user experience, we use each platform's own UI toolkit:
GTK4/libadwaita on Linux, WinUI 3 on Windows, Jetpack Compose on Android,
AppKit on macOS and UIKit on iPadOS.

GPU access also differs across platforms. Drawing uses Vulkan on Linux and
Android, Metal on macOS and iPadOS, Direct3D 12 on Windows and WebGPU in the
browser. The brush and compositing engines need to run efficiently through each
API while keeping the tools' behavior consistent.

As a result, the app separates platform integration from a shared Rust editor
and renderer. The platform client collects pen samples and sends tool commands
to the editor. The editor manages the document and UI state on the CPU, applies
edits and calculates brush placement. The renderer uses `wgpu`, a Rust
graphics library that translates shared shaders and rendering commands to each
platform's GPU API. Every client uses the same brush and compositing engines.

```text
Platform client: native widgets or browser controls
    | pen samples and tool commands
    v
Shared Rust editor
    +-- document, tools, undo/redo and UI state
    +-- brush dynamics and stroke placement
    | incremental drawing work
    v
Shared renderer (wgpu)
    +-- GPU brushes, filters and layer composition
    |
    v
Vulkan / Metal / Direct3D 12 / WebGPU
    |
    v
Native drawing surface or browser canvas
```

This separation also lets input handling and drawing follow different schedules.
Native clients keep GPU waits off the UI thread, so controls and pen input can
continue while the previous frame is rendering. The browser schedules drawing
through animation callbacks. Each update batches new brush marks and recomputes
changed regions, reusing the rest of the image.

The drawing path targets high refresh rates, including 120 Hz displays, and
requires a hardware GPU. The [performance guide](docs/development/testing.md#performance)
explains how to measure frame time and input-to-display latency on a given device.

See [architecture and frame flow](docs/architecture.md) for the responsibilities
of each component and the path from a pen event to a displayed stroke.

## Package layout

The `layer-` names are the internal Cargo package names. Shared Rust code lives
under `crates/`; platform applications live under `apps/`.

| Package | Responsibility |
| --- | --- |
| [`layer-core`](crates/layer-core/README.md) | Document data, layers, reversible edits, brush definitions and the editable project format. It has no dependency on a UI toolkit or graphics API. |
| [`layer-engine`](crates/layer-engine/README.md) | Input processing, brush dynamics, stroke placement and the work sent to the renderer. |
| [`layer-ui`](crates/layer-ui/README.md) | The editor session, tool behavior, typed commands, panel layout, preferences and file-operation state. |
| [`layer-render`](crates/layer-render/README.md) | The contract between the engine and renderer, including brush contacts, frame packets and explicit image requests. |
| [`layer-render-wgpu`](crates/layer-render-wgpu/README.md) | GPU brushes, layer composition, filters, previews and the canvas viewport. |
| [`layer-host`](crates/layer-host/README.md) | Common session and renderer integration used by the Android, Apple and Windows bridges. GTK and web integrate the shared session directly. |
| [`layer-ffi`](crates/layer-ffi/README.md) | A C interface for offscreen drawing, tests and benchmarks. Interactive clients use their own platform bindings. |
| [`layer-bench`](crates/layer-bench/README.md) | GPU benchmarks, regression workloads and brush-preview generation. |

[`assets/`](assets) contains brush resources and runtime filter definitions.
The shared interface icons and bundled brush previews currently live under
[`apps/layer-web/`](apps/layer-web); native build scripts reuse those assets.

## Configurable interface

The UI separates editor behavior from the widgets used to present it. Shared Rust
code defines commands, controls and layout, so the same tools can be arranged into
different workspaces without changing their behavior.

Panels can be docked, floated, grouped in tabs or collapsed. Toolbars can contain
chosen tools and commands, with control visibility and sizing represented in the
workspace model. Tool Settings follows the active tool, and Properties exposes the
selected filter's parameters.

`UiSession` coordinates this shared state. A frontend sends it a typed `UiAction`,
such as selecting a tool or moving a panel. Rust validates the action and reports
which parts of the UI changed. The frontend updates the affected controls,
keeping widgets in place while someone is dragging a slider or editing a value.
Buttons, menus and shortcuts use the same command definitions and availability
rules.

Workspace changes have their own undo history, separate from edits to the artwork.
Zen mode hides controls without resizing the canvas or moving the drawing. These
boundaries let users adjust the interface without disturbing work on the canvas.

The [workspace guide](docs/ui/README.md) covers layout, customization and the
shared UI contract in more detail.

## Rendering engine

A large illustration can contain hundreds of layers, many with only a few marks
or a small part of the image. Allocating a full canvas for each layer wastes
graphics memory; recomputing the whole stack after every pen movement wastes
processing time. The renderer needs to fit these sparse layers into a limited
memory budget and update only the parts affected by an edit.

Capy Canvas stores painted layers in 256 × 256 GPU texture pages, allocated as
regions are touched. Shaders draw into those pages and combine them into the
visible image. The compositor tracks changed regions and the results that depend
on them, reusing unchanged content between frames.

Masks and effect layers introduce branching dependencies. In this example, a
color adjustment is clipped to a paint layer beneath an ink layer. Its mask and
opacity mix the filtered color with the original paint, preserving the paint's
coverage. Background and ink enter separately:

```text
Paint tile ----+----> Color filter ------+
               |                         |
               +---------------------+   |
                                     v   v
Effect mask + opacity -----------> Mix with original
                                          |
                                          v
Background tile -----------------> Paint over background
                                          |
                                          v
Ink tile ------------------------> Ink over result
                                          |
                                          v
                                      Output tile
```

The diagram shows logical dependencies. Compatible color adjustments that
operate on each pixel independently can be fused into one shader, including
their masks and opacity, without writing an intermediate texture for each effect.
Results that need separate passes remain in GPU resources. Blurs need neighboring
pixels, and some filters require full-image intermediates, so their update and
memory costs can be larger than a tile.

Many laptops and mobile devices now share physical RAM between CPU and GPU.
Older designs that keep separate CPU and GPU images and repeatedly copy between
them do not automatically benefit from this unified memory. Capy Canvas keeps
drawing and composition in GPU textures; shared RAM still has bandwidth costs
and requires synchronization. Export and small UI image requests use explicit
readback paths when CPU access is needed.

The [rendering guide](docs/internals/rendering.md) covers caching, memory use and
shader execution. The [runtime filter reference](docs/reference/runtime-filters.md)
explains how filters are defined in JSON and WGSL shader code.

## Brush engine

Simple brushes draw a stroke by stamping small marks, or dabs, along its path.
That can run well on a CPU. The workload becomes much heavier when a brush picks up
and mixes existing color, smears paint or deforms it with liquify. Watercolor flow and
oil mixing add state that must change as the stroke advances. Each contact can
then require repeated reads and updates across a large area, making computation
and memory access a bottleneck.

Capy Canvas keeps pressure response, stroke placement and other sequential input
work on the CPU. It sends batches of resolved contacts to the GPU, which evaluates
brush coverage and pixel interactions in parallel. Ordered passes preserve the
sequence of operations when a brush needs the result of an earlier contact.

Layer pixels and paint state stay in GPU textures throughout drawing and
composition. Wet brushes can track the paint carried by the tip; the current
watercolor model tracks localized water and pigment transport. Sparse texture
pages and damage bounds limit updates to affected areas. Simple ink and eraser
brushes use a direct blending path without allocating or updating unused wet-paint
state. This avoids both unnecessary processing and reading the canvas back to the
CPU between brush updates.

The goal is substantially better performance for these complex brushes than a
CPU-only pixel engine, especially on modern tablets. Reducing memory traffic,
CPU overhead and unnecessary GPU work also targets battery efficiency and sustained
performance within a mobile device's thermal limits. Comparative speed and energy
gains still need measurement on the target hardware.

The [brush guide](docs/internals/brushes.md) explains stroke generation, GPU paint
state and the performance considerations behind this design.

## Settings and documents

Preferences are defined in Rust, including defaults, validation, available
choices and keyboard shortcut rules. Platform clients display those definitions
and save accepted changes using their own storage APIs. Workspace layout has a
separate serialization model from preferences and from the drawing.

Editable `.capy` projects retain layers, strokes, source assets and the operations
needed to reconstruct the drawing. Saving a project is different from exporting
a flattened PNG. Shared code tracks unsaved changes and file requests; the client
provides file pickers and storage. These flows are still being integrated and
validated across the ports.

See [settings and persistence](docs/ui/settings.md) and
[documents and edits](docs/internals/documents.md).

## Platform support

Each client connects the shared editor to a native window or browser surface.
Platform code translates input, manages surface lifetime and handles services
such as file access; it does not implement a separate painting engine.

| Client | UI and graphics | Integration considerations |
| --- | --- | --- |
| [Linux](docs/development/linux.md) | GTK4/libadwaita and Vulkan on Wayland. | A dedicated GPU worker presents into a Wayland subsurface beneath GTK controls. The current host requires mailbox presentation and has no X11 canvas path. |
| [Web](docs/development/web.md) | DOM controls, Rust compiled to WebAssembly and WebGPU. | Runs on the browser event loop and can be packaged as an offline PWA. GPU access, pen data and file APIs depend on the browser. |
| [Android](docs/development/android.md) | Kotlin/Jetpack Compose and Vulkan through a `SurfaceView`. | JNI connects to Rust. Android owns pen history, frame callbacks, lifecycle and document-provider access. |
| [macOS / iPadOS](docs/development/apple.md) | AppKit / UIKit, shared Swift components and Metal. | A `CAMetalLayer` presents the canvas. The iPad adapter also handles Apple Pencil predictions and later corrections to estimated samples. |
| [Windows](docs/development/windows.md) | C++/WinRT, WinUI 3 and D3D12 through a `SwapChainPanel`. | Input and rendering run independently of UI controls. Native file workflows are implemented; workspace parity and physical-input acceptance still have gaps. |

The [platform guide](docs/platforms/README.md) describes the adapter contract and
links to the ports' validation records. A working client is not yet a release or
a claim of feature parity.

## Build and run on Linux

Linux with Wayland is the primary development target and receives new editor
features first. Coding agents port the Linux interface to the web client, where
the shared Rust core compiles to WebAssembly. Further agents use that web
implementation as the reference for each platform's native UI toolkit:

```text
Linux / Wayland (GTK4)
    |  New features and UI development
    |
    |  Coding agents port the interface
    v
Web (DOM + Rust/WebAssembly)
    |
    |  Coding agents adapt the UI to native toolkits
    +----> Android (Jetpack Compose)
    +----> macOS (AppKit)
    +----> iPadOS (UIKit)
    +----> Windows (WinUI 3)

Shared Rust editor and renderer compile for every target.
```

The web build runs on each target platform in a browser with hardware WebGPU.
Agents can compare it with GTK on Linux, then compare native clients against
that same reference using screenshots and interaction tests. Native clients
compile the Rust core for
their platform; the porting work is in the interface and OS integration. The
[platform guide](docs/platforms/README.md) covers those boundaries and links to
parity checks.

Use a recent stable Rust toolchain, a C/C++ build toolchain, `pkg-config`, GTK4
and libadwaita development packages. The current development environment uses
GTK 4.22 and libadwaita 1.9. Running the native app requires a Wayland session and
a hardware Vulkan driver with mailbox presentation and premultiplied-alpha
surface support.

```bash
git clone https://github.com/capyatelier/capycanvas.git
cd capycanvas
cargo run --locked --release -p layer-linux
```

The [Linux setup guide](docs/development/linux.md) covers dependencies and
packaging. The [developer guide](docs/development/README.md) is the entry point
for all platforms, testing and performance measurements.

## Contributing

We need help with product design, testing painting, photo and comic workflows,
and shader development. The current focus is getting the user interface right.
Next comes a substantial cleanup of the agent-generated code, performance
optimization, and simplifying or rewriting the brush and compositor engines.

## License and branding

Original code and non-brand assets are available under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option. Contributions to those parts of the
project are submitted under both licenses unless agreed otherwise.

The Capy Canvas and Capy Atelier names and capybara artwork have separate
[branding terms](BRANDING.md). Modified public distributions must use their own
branding unless permission is granted. Dependencies retain their own licenses;
see [third-party notices](THIRD_PARTY_NOTICES.md) and the
[publication guide](docs/development/publication.md).
