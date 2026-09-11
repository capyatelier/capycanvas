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

Its GPU-accelerated brush and compositing engines are designed to improve
performance and battery life, particularly on mobile devices. The interface lets
artists adapt layouts and shortcuts to match the muscle memory they have developed
in other apps.

This project is fully free and open source, and keeps artists in control of their
own data. There are no accounts, subscriptions or tracking. Drawing and editing
happen locally on your device, and the code is licensed under MIT or Apache-2.0.

## Overall architecture

When designing Capy Canvas, we did not want to compromise on UI responsiveness.
Controls and pen input need to run at 120 fps on every supported platform, even
while the drawing engine handles large brushes and hundreds of layers.
We also need tools and documents to behave consistently across platforms, even
though each uses different UI and graphics APIs.

To achieve these goals, we designed the app around a shared core written in Rust.
The core contains the editor's business logic, including tools, UI behavior and
layout, brushes, layers and document editing. We cross-compile it for each
platform and wrap it in the native toolkit, which supplies the widgets and OS
integration. The web client runs the same core compiled to WebAssembly.

For the best user experience, we use each platform's own UI toolkit:
GTK4/libadwaita on Linux, WinUI 3 on Windows, Jetpack Compose on Android,
AppKit on macOS and UIKit on iPadOS.

The canvas also needs a common way to use each platform's GPU. We built its
pixel-processing stack on `wgpu`, a Rust graphics library that translates our
shaders and rendering commands to Vulkan on Linux and Android, Metal on macOS and
iPadOS, Direct3D 12 on Windows and WebGPU in the browser. This lets us implement
a brush, filter or layer operation once and use it in every client.

```text
Native UI toolkit or browser controls
    | pen samples and tool commands
    v
Shared Rust core
    +-- editor: documents, tools, UI behavior and layout
    +-- stroke processing: pressure and brush placement
    +-- renderer: GPU brushes, filters and composition via wgpu
    |
    v
Vulkan / Metal / Direct3D 12 / WebGPU
    |
    v
Native drawing surface or browser canvas
```

To keep a long draw from blocking controls or pen input, we keep GPU waits off
the native UI threads. The web client schedules drawing through browser animation
callbacks. We batch new brush marks for submission and update the regions affected
by an edit, reusing the rest of the image between frames.

Painting requires a hardware GPU. The [performance guide](docs/development/testing.md#performance)
explains how we measure frame time and input-to-display latency. The
[architecture guide](docs/architecture.md) follows a pen event through the shared
core to the displayed stroke.

## Package layout

We keep the shared Rust code under `crates/` and the platform clients under
`apps/`. The Rust workspace contains the following packages; each package's
README explains its main types and where to start reading the code. The `layer-`
prefix is the internal Cargo naming convention.

| Package | Responsibility |
| --- | --- |
| [`layer-core`](crates/layer-core/README.md) | Defines documents, layers and brushes, applies undoable edits, and handles the `.capy` project format. |
| [`layer-engine`](crates/layer-engine/README.md) | Turns pen samples into brush marks, including pressure response, stroke stabilization and spacing. |
| [`layer-ui`](crates/layer-ui/README.md) | Implements editor tools, commands, panel layout, preferences and file-operation state shared by the clients. |
| [`layer-render`](crates/layer-render/README.md) | Defines the drawing work and image requests passed between the engine and renderer. |
| [`layer-render-wgpu`](crates/layer-render-wgpu/README.md) | Draws brushes, combines layers, runs filters and presents the canvas using `wgpu`. |
| [`layer-host`](crates/layer-host/README.md) | Connects the shared editor and renderer for Android, Apple and Windows. GTK and web connect them directly. |
| [`layer-ffi`](crates/layer-ffi/README.md) | Exposes offscreen drawing through a C interface for tests, benchmarks and embedding. |
| [`layer-bench`](crates/layer-bench/README.md) | Runs GPU benchmarks and renderer regressions, and generates brush previews. |

Brush resources and runtime filter definitions live in [`assets/`](assets).
We share the interface icons and bundled brush previews in
[`apps/layer-web/`](apps/layer-web) with the native clients; their build scripts
reuse those files.

## Configurable interface

We built the workspace from configurable panels and toolbars so artists can put
the controls they use where they expect to find them. Panels can be docked,
floated, grouped in tabs or collapsed. Artists can choose toolbar contents and
adjust control visibility and sizing. Tool Settings follows the active tool,
and Properties shows the selected filter's parameters.

We keep this behavior in Rust. When a client receives a button press or a layout
change, it sends a typed `UiAction` to `UiSession`, which owns the editor session.
The session applies the action and reports which parts of the UI changed. Clients
can then update the affected controls in place, preserving a slider drag or text
edit. Buttons, menus and shortcuts use the same command definitions, so they
agree on what a command does and when it is available.

Workspace changes have their own undo history so moving a toolbar does not become
another step in the painting history. Zen mode hides controls without resizing
the canvas, keeping the drawing in place when the interface disappears.

The [workspace guide](docs/ui/README.md) explains how the layout model and shared
actions connect to native widgets.

## Settings and documents

We define preferences in Rust so every platform uses the same defaults, validation
and shortcut rules. Each client builds its settings controls from those definitions
and saves the user's choices through its own storage APIs. Workspace layout has a
separate storage model from preferences and artwork, so rearranging panels does
not mark the drawing as modified.

An editable project needs to preserve more than the pixels on screen. The `.capy`
format stores layers, strokes, source assets and the operations needed to
reconstruct the drawing. Exporting a PNG instead produces a flattened image for
use in other apps. The Rust core tracks unsaved changes and file requests, while
each client supplies file pickers and reads or writes the bytes.

The [settings guide](docs/ui/settings.md) explains preference definitions and
storage. The [document guide](docs/internals/documents.md) covers editable projects,
undo and save handling.

## Rendering engine

We need to handle illustrations with hundreds of layers, even though many layers
contain only a few marks. Giving each layer a full-canvas texture would quickly
use up graphics memory, and recomputing the entire stack after every pen movement
would repeat work on unchanged pixels.

We store painted content in 256 × 256 tiles held in GPU textures, allocating them
as regions are touched. Shaders update the affected tiles, and the compositor
combines them with the other layers to produce the visible image. We track which
regions changed and which cached results depend on them, so we can reuse the
remaining results.

Those dependencies branch when an adjustment uses a mask: the compositor needs
the original paint, the filtered result and the mask. In this example, a color
adjustment is clipped to a paint layer, so it must change that paint without
changing the background or the ink above it. The effect's mask and opacity control
how much filtered color replaces the original, while the paint's coverage stays
the same:

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

If the mask changes, we recompute the adjustment and the composition above it,
reusing the paint, background and ink tiles. We can also combine several of these
operations in one shader. For compatible effects that process each pixel
independently, we fuse the adjustments and their masks
so one effect can use the previous result without writing another texture first.
Blurs need neighboring pixels and can require larger intermediate images. We keep
those images on the GPU and cache them for later updates; some filters still need
full-image storage.

We keep the composed image in GPU textures through presentation, avoiding a
second rendered canvas on the CPU. Many laptops and mobile devices now use
unified memory, where the CPU and GPU share physical RAM, but keeping two copies
still consumes memory and copying between them still uses bandwidth. We read
results back when a feature such as export, thumbnails or color sampling needs
CPU access. Even with shared RAM, that access requires CPU/GPU synchronization.

The [rendering guide](docs/internals/rendering.md) explains how we track changes
and reuse intermediate results. The [runtime filter reference](docs/reference/runtime-filters.md)
describes how to add filters using JSON definitions and WGSL shader code.

## Brush engine

A brush that stamps small marks, or dabs, can run well on a CPU. Once it needs to
pick up color, smear paint or deform it with liquify, it must repeatedly sample
and update the existing image. Large brush tips multiply that work. More elaborate
watercolor and oil models also need to track and move paint as the stroke advances.

We put these pixel operations on the GPU while keeping pressure response and
stroke placement on the CPU. The CPU sends batches of dabs to the GPU, which can
evaluate many pixels in parallel. When a brush needs the result of an earlier dab,
we preserve that ordering across GPU passes.

We keep layer pixels and the paint carried by wet brushes in GPU textures, so a
stroke does not have to read the canvas back to the CPU between updates. The
current watercolor model also tracks localized water and pigment transport.
Sparse tile storage and bounds on the changed regions limit the work per update.
Ordinary ink and erasers use a direct blending path, avoiding the extra wet-paint
state that those brushes do not need.

The goal is to make complex brushes substantially faster than a CPU pixel engine
while reducing CPU overhead and memory traffic. This matters particularly on
modern tablets, where battery use and heat limit sustained performance. We still
need comparative measurements on target hardware to measure the effect on speed
and energy use.

The [brush guide](docs/internals/brushes.md) follows a stroke from pen samples to
GPU paint updates and explains the state used by different brush types.

## Platform support

Pen input, window lifecycle and file access differ across operating systems,
so each client adapts those services to the shared core:

| Client | UI and graphics | Platform integration |
| --- | --- | --- |
| [Linux](docs/development/linux.md) | GTK4/libadwaita and Vulkan on Wayland. | Runs the GPU worker separately from GTK and presents the canvas in a Wayland subsurface beneath the controls. |
| [Web](docs/development/web.md) | DOM controls and WebGPU. | Runs the shared core as WebAssembly, schedules drawing through browser callbacks, and supports offline installation as a PWA. |
| [Android](docs/development/android.md) | Kotlin/Jetpack Compose and Vulkan. | Calls Rust through JNI, handles pen-event history and `SurfaceView` recreation, and uses Android document providers for files. |
| [macOS / iPadOS](docs/development/apple.md) | AppKit / UIKit and Metal. | Presents the canvas through a `CAMetalLayer`. The iPad client also forwards Pencil predictions and later corrections to estimated samples. |
| [Windows](docs/development/windows.md) | C++/WinRT, WinUI 3 and Direct3D 12. | Hosts the canvas in a `SwapChainPanel` and keeps input and rendering independent of control updates. |

We are still bringing the ports into UI parity. The [platform guide](docs/platforms/README.md)
links to their implementation notes and device-test records.

## Development workflow

After implementing a feature in GTK, we use coding agents to adapt the interface
to the web client, where the shared core compiles to WebAssembly. Other agents use
that browser implementation as the reference when adapting the interface to each
platform's native toolkit:

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

A browser with hardware WebGPU lets us run the web reference beside each native
app. We first compare the web UI with GTK, then compare each native port with the
web UI, using screenshots and the same interaction tests at each step. The
[platform guide](docs/platforms/README.md#development-workflow) explains these checks.

## Build and run on Linux

Linux with Wayland is our primary development target and receives new editor
features first.

To build the Linux app, install a recent stable Rust toolchain, a C/C++ build
toolchain, `pkg-config`, and GTK4, libadwaita and Wayland development packages.
We develop with GTK 4.22 and libadwaita 1.9. Running the canvas requires a Wayland
session and a hardware Vulkan driver with mailbox presentation and
premultiplied-alpha surface support.

```bash
git clone https://github.com/capyatelier/capycanvas.git
cd capycanvas
cargo run --locked --release -p layer-linux
```

The [Linux setup guide](docs/development/linux.md) covers dependencies and
packaging. Start with the [developer guide](docs/development/README.md) for the
other platforms, testing and performance measurements.

## Contributing

We need help with product design, testing painting, photo and comic workflows,
and shader development. Our current focus is getting the user interface right.
Once the UX is in good shape, we plan to clean up the agent-generated code,
optimize performance, and simplify or rewrite the brush and compositor engines.

## License and branding

We license the original code and non-brand assets under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE); you can choose either license. Contributions to those
parts of the project are submitted under both licenses unless agreed otherwise.

The Capy Canvas and Capy Atelier names and capybara artwork have separate
[branding terms](BRANDING.md). If you publish a modified version, use your own
branding unless you have permission. Dependencies keep their own licenses; see
[third-party notices](THIRD_PARTY_NOTICES.md) and the
[publication guide](docs/development/publication.md).
