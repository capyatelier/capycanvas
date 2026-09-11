# Linux development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Linux client uses GTK4/libadwaita for controls and the shared wgpu renderer
through Vulkan. It currently runs on Wayland only.

## Prerequisites

Install a recent stable Rust toolchain, a C/C++ build toolchain, `pkg-config`,
GTK4 and libadwaita development packages, and Wayland development libraries.
Package names vary by distribution. The current development stack is GTK 4.22
and libadwaita 1.9; the enabled API features are declared in
[`apps/layer-linux/Cargo.toml`](../../apps/layer-linux/Cargo.toml).

Check the libraries visible to the build:

```bash
pkg-config --modversion gtk4 libadwaita-1 wayland-client
```

Running the canvas requires a Wayland session and a hardware Vulkan driver with
mailbox presentation and premultiplied-alpha surface support. The app has no X11,
GLES or CPU canvas fallback. GTK chooses its own renderer for controls; that is
separate from the canvas's Vulkan renderer.

## Build and run

```bash
cargo run --locked --release -p layer-linux
```

Use the release profile when judging responsiveness. The executable is normally
`target/release/layer-linux`; Cargo may use another location when
`CARGO_TARGET_DIR` is set.

## How the host works

[`main.rs`](../../apps/layer-linux/src/main.rs) creates the native application and
workspace. [`canvas.rs`](../../apps/layer-linux/src/canvas.rs) adapts the shared
session and prepares updates on an independent 120 Hz timer. A dedicated
[render worker](../../apps/layer-linux/src/render_thread.rs) owns canvas GPU work
and presents into an app-owned [Wayland subsurface](../../apps/layer-linux/src/wayland.rs)
beneath the GTK controls. The timer target alone does not establish display latency.

GTK owns native input and the parent window. The canvas and cursor are rendered
by the shared GPU viewport path; GTK does not download or reimport a canvas-sized
bitmap. Window movement, resizing and scaling must preserve the relationship
between the native surface, pen coordinates and document camera.

[`files.rs`](../../apps/layer-linux/src/files.rs) supplies native dialogs and local
project transport. New/Open preserve the current drawing in its own window while
the incoming document is validated. Saving uses a background write and atomic
replacement; the [project reference](../reference/project-format.md) explains
checkpoints and failure handling.

## Stage a native bundle

In addition to the build prerequisites, install Node.js, `strip` and
`desktop-file-validate`, then run:

```bash
node apps/layer-linux/package.mjs
dist/capycanvas-linux/bin/capycanvas
```

The staging directory includes the executable, desktop launcher, icon, runtime
filters and project notices. GTK/libadwaita remain system dependencies. The script
does not install the application into the desktop. Distribution requirements are
covered in the [publication guide](publication.md).

## Validate

The [testing guide](testing.md) lists GTK interaction and shared-engine checks.
The [UI implementation record](../history/ui-implementation.md) and
[Wayland feasibility record](../history/wayland-subsurface-feasibility.md) explain
past integration decisions and measurements.
