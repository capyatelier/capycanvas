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

Capy Canvas is a free, GPU-accelerated art and image editor being developed for
digital painters, photographers and comic artists. It is Linux-first because Linux has been underserved by professional art tools,
with native clients for Windows, macOS, iPadOS and Android, and a browser version.
The project is still in development, and the clients do not yet have the same
feature coverage or level of testing.

The editor combines pressure-sensitive painting with layers, groups, masks,
selections, transforms and programmable filters. These provide the shared tools
for drawing, image adjustment and composition. Its panels and toolbars can be
rearranged and customized, while Zen mode hides controls to leave more room for
the drawing. A shared Rust implementation keeps drawing behavior consistent
across platforms; each native client uses its platform's own UI toolkit.

The software is available under MIT or Apache-2.0. Drawing requires no login,
subscription or central server, and the code can be forked and used in other
projects. Coding agents are making it more practical for artists to build their
own tools. This project aims to provide a foundation shaped by artists’ workflows:
a responsive GPU canvas, an adaptable interface and a portable drawing engine.
The [branding terms](#license-and-branding) are separate from the software licenses.

This README introduces the code. The [technical documentation](docs/README.md)
explains individual systems and development setup; the
[website documentation](https://capycanvas.art/docs/) is for people using the app.

## Overall architecture

The application has three main parts. The shared Rust editor owns the document,
interprets input and manages tools, commands and UI state. The GPU renderer draws
brush marks and combines layers into the visible image. A platform client supplies
widgets, collects input and presents that image in a native window or browser.

```text
Platform client: native widgets or browser controls
    │ tool commands and pen samples
    ▼
Shared Rust editor
    ├── document, layers and undo/redo
    ├── tools, brush dynamics and stroke placement
    └── workspace, preferences and command state
    │ incremental drawing work
    ▼
Shared wgpu renderer
    ├── brush rasterization into GPU textures
    └── layer composition and canvas presentation
    │
    ▼
GPU API: Vulkan / Metal / D3D12 / WebGPU
    │
    ▼
Native window or browser canvas
```

The CPU handles the ordered work: interpreting pen samples, placing brush marks
and applying document edits. The GPU evaluates and combines the pixels. The
renderer uses `wgpu`, a Rust graphics library that targets each platform's GPU
API, so the native clients and browser use the same brush and composition code.
Painting requires a hardware GPU; there is no CPU painting fallback.

The drawing path is designed for high refresh rates, including 120 Hz displays.
Input collection does not wait for the GPU, and each drawing update batches new
brush marks into a render packet. The renderer retains its textures between
frames and updates affected regions. Moving the view can reuse the rendered
canvas instead of repainting the document. Native hosts keep GPU waits off their
UI threads; the browser schedules work through its animation callbacks. Actual
latency depends on the device, driver, brush and document; the
[testing guide](docs/development/testing.md#performance) explains how it is measured.

See [architecture and frame flow](docs/architecture.md) for ownership boundaries
and the path from a pen event to a displayed stroke.

## Package layout

The `layer-` names are the internal Cargo package names. Shared Rust code lives
under `crates/`; platform applications live under `apps/`.

| Package | Responsibility |
| --- | --- |
| [`layer-core`](crates/layer-core/src/lib.rs) | Document data, layers, reversible edits, brush definitions and the editable project format. It has no dependency on a UI toolkit or graphics API. |
| [`layer-engine`](crates/layer-engine/src/lib.rs) | Input processing, brush dynamics, stroke placement and the work sent to the renderer. |
| [`layer-ui`](crates/layer-ui/src/lib.rs) | The editor session, tool behavior, typed commands, panel layout, preferences and file-operation state. |
| [`layer-render`](crates/layer-render/src/lib.rs) | The contract between the engine and renderer, including brush contacts, frame packets and explicit image requests. |
| [`layer-render-wgpu`](crates/layer-render-wgpu/src/lib.rs) | GPU brushes, layer composition, filters, previews and the canvas viewport. |
| [`layer-host`](crates/layer-host/src/lib.rs) | Common session and renderer integration used by the Android, Apple and Windows bridges. GTK and web integrate the shared session directly. |
| [`layer-ffi`](crates/layer-ffi/src/lib.rs) | A C interface for embedding and headless validation. The platform apps also have their own bindings where needed. |
| [`layer-bench`](crates/layer-bench/src/main.rs) | GPU benchmarks, regression workloads and brush-preview generation. |

[`assets/`](assets) contains brush resources and runtime filter definitions.
The shared interface icons and bundled brush previews currently live under
[`apps/layer-web/`](apps/layer-web); native build scripts reuse those assets.

## Configurable interface

Painters, photographers and comic artists need different interfaces. Comic work
calls for quick access to reference layers, selections and lasso fill. Painting
needs detailed brush controls and room to work with the canvas. Photo editing
makes heavier use of effect chains—ordered filters applied to an image—and
controls that follow the selected tool, layer or filter. An interface arranged for one of these workflows can make the
others unnecessarily awkward.

Artists also bring muscle memory from the software they already use. Relearning
where tools live, how panels behave and which shortcuts to press takes time. The
UI should be flexible enough to accommodate familiar designs from existing apps,
so changing editors does not require adopting one prescribed layout.

The same core already supplies layers, masks, brushes and image effects. The UI
needs to expose the relevant tools without making every user navigate all of
them. Configurable panels and toolbars, together with tool-specific settings,
provide that flexibility; dedicated arrangements for each audience can build on
this shared implementation.

`UiSession` is the shared editor session. A frontend sends it a typed `UiAction`,
such as selecting a tool or moving a panel, and receives the resulting state
changes. Rust decides what an action means and whether it is available; the
frontend renders that state with GTK, DOM, Compose, UIKit, AppKit or WinUI controls.

The workspace model describes docked and floating panels, tabs, toolbars and
collapsed columns. Users can choose which tools or controls a panel contains. The shared layout
can be serialized and restored; automatic persistence is implemented by individual
hosts; GTK still starts from its default layout. Layout changes have their own undo history, separate
from edits to the artwork. Zen mode changes control visibility without resizing
the canvas or moving the drawing. Native focus, accessibility and widget behavior
remain the client's responsibility.

The [workspace guide](docs/ui/README.md) explains how layout, customization and
changed-state notifications fit together. Individual clients implement different
portions of this shared model as their UI work progresses.

## Rendering engine

The renderer stores painted layer content in GPU texture pages, allocating pages
as regions are touched. Its compositor—the code that combines layers into the
visible image—tracks which regions have changed and retains reusable results.
Groups, masks, clipping and blend modes are evaluated in the same composition
system as ordinary paint layers.

A change can affect more than the rectangle that was painted. A blur, for example,
needs neighboring pixels, while a filter on a group depends on that group's
contents. The renderer tracks these dependencies and caches intermediate images
where needed. Global or animated effects can require broader updates; incremental
rendering does not mean every operation has a small cost.

Filters use runtime JSON definitions and WGSL, the shader language used by WebGPU
and `wgpu`. Most can be added or replaced without rebuilding the app. Canvas
painting and presentation stay on the GPU. Export, thumbnails and color sampling
have explicit readback paths for the cases that need CPU-accessible results.

Start with [rendering and composition](docs/internals/rendering.md), then use the
[runtime filter reference](docs/reference/runtime-filters.md) for the shader contract.

## Brush engine

A stroke records real pen samples and the brush settings used to draw it. The
engine evaluates pressure, tilt and other available sensors, then places brush
contacts along the stroke. A contact is called a *dab*: one resolved brush mark
with a position, shape, color and material parameters. Placement and randomized
variation are deterministic so a stored stroke can be replayed.

Simple ink and eraser brushes use GPU blending. Brushes that interact with
existing paint, such as smudge, wet paint and watercolor, use additional GPU
state and ordered processing. They share the same stroke model, but do not all
have the same rendering cost. Temporary prediction helps the visible stroke
follow the pen; predicted samples are replaced as real input arrives and are
never saved as artwork.

The [brush guide](docs/internals/brushes.md) follows a stroke through the engine
and points to the detailed raster and paint-state references.

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

The current focus is the user interface. Help with product design, testing real
illustration workflows and shader development would be particularly useful.
For design or testing feedback, describe the task you were trying to complete,
what happened and the platform and input device you used. Discuss substantial
changes in an issue before implementing them so they can fit the current work.

Much of the codebase was generated with coding agents. Once the UX is in good
shape, we plan a substantial cleanup and a performance-focused review of the
implementation. The brush and compositor engines have become more complex than
we want; simplifying or rewriting them is part of that work. Their current
internal structure should not be treated as a settled API.

## License and branding

Original code and non-brand assets are available under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option. Contributions to those parts of the
project are submitted under both licenses unless agreed otherwise.

The Capy Canvas and Capy Atelier names and capybara artwork have separate
[branding terms](BRANDING.md). Modified public distributions must use their own
branding unless permission is granted. Dependencies retain their own licenses;
see [third-party notices](THIRD_PARTY_NOTICES.md) and the
[publication guide](docs/development/publication.md).
