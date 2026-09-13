# Linux development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

Linux with Wayland is the primary development target and receives new editor
features first. The client uses GTK4/libadwaita for controls and the shared wgpu
renderer through Vulkan. Coding agents use this implementation as the basis for
the web UI, then adapt that reference to the other native toolkits. The
[platform workflow](../platforms/README.md#development-workflow) explains how the
ports are compared.

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
session and prepares updates on an independent timer aligned to Wayland
presentation timing, with a 120 Hz fallback. A dedicated
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

### Focused UI debugging

Run from the repository root in a Wayland session. Use fresh storage for each
test process so workspace and settings tests cannot change your normal setup:

```bash
gtk_test_dir=$(mktemp -d)
CAPY_WORKSPACE_DIR="$gtk_test_dir/workspaces" \
LAYER_SETTINGS_FILE="$gtk_test_dir/settings.json" \
GDK_BACKEND=wayland GSK_RENDERER=vulkan G_DEBUG=fatal-criticals RUST_BACKTRACE=1 \
  cargo test --locked --release -p layer-linux \
  workspace::tests::workspace_switcher_tests::native_active_workspace_delete \
  -- --ignored --exact --test-threads=1 --nocapture >"$gtk_test_dir/test.log" 2>&1
cat "$gtk_test_dir/test.log"
```

Find related cases in [tests.rs](../../apps/layer-linux/src/tests.rs) and its
test modules, including [workspace_switcher_tests.rs](../../apps/layer-linux/src/workspace_switcher_tests.rs).
Reuse `native_test_app`, widget lookup helpers and `pump` to exercise actual GTK
dialogs; wait for workspace readiness and operation completion before assertions.
Persistence cases should verify reopening as well as the visible rows.

For the compact Color panel, run
`LAYER_TEST_ARTIFACTS="$PWD/artifacts/color-panel/gtk" bash tools/performance/workspace-motion.sh gtk --color-panel`.
This exercises native mouse/touch picking, overlapping paint swatches, the
visible swap button and its context menu, both shape alternatives, readout
cycles and keyboard activation. It checks 144/160/200/280/360 px panels in both
themes, all three shapes and HSB (circle/square), HLS (triangle),
OKLCH/RGB readouts, retaining screenshots and geometry. The entire control occupies one
square and compresses in short docks; `native_default_workspace` covers that
shipped layout. Four 36px tiles (144px including the panel's 8px content insets)
is the design minimum. Values are read-only; tap the model label to cycle between
HSB/HLS, OKLCH and RGB independently of the circle/square/triangle field. Two bare shape
buttons follow the upper-right arc; the swap button sits beside the overlapping
paints. Right-click or hold either paint swatch (or use Shift+F10 while focused)
also opens the swap action. The readout has no tooltip or hover decoration.
Picker coordinates are retained per paint so hue changes, drags through black,
alpha edits, swaps and saved-state reloads do not lose powerless components.
The circle uses a smooth elliptical projection of the full Okhsv square. Its
ring rotates 24° counterclockwise to place RGB blue at the bottom, and its blue
hue preview uses a short smooth ramp across the reference gamut-boundary jump.
Actual Okhsv conversion and field sampling are unchanged by that preview ramp.
Selecting circle defaults to OKLCH, square to HSB and triangle to HLS. The label
cycles readout models independently until another shape is selected. All readouts
use fixed digit cells and arc positions so digit-count changes do not move the values.
Square remains HSV and triangle remains HLS.
Switching shapes preserves sRGB paint; each model retains powerless coordinates.
The conversions adapt [Ottosson’s Okhsv reference](https://bottosson.github.io/posts/colorpicker/)
with explicit neutral/black handling and a more accurate blue gamut boundary.
OKLCH shows lightness percent, chroma to three decimal places and hue degrees
([conversion reference](https://www.w3.org/TR/css-color-4/#oklch)). Neutral colors
retain the picker's hue. Saved Lab readout preferences migrate to OKLCH.

The Okhsv field stays on the CPU. Its raster reuses an interpolated saturation
curve and sRGB transfer table; picking keeps full-precision conversion. Hosts
sample the smooth field at logical-pixel resolution and keep the ring, circular
clip and markers at native resolution. Shared conversion/raster tests and the
Web 2× interpolation comparison bound the measured color error. For native
raster timings, run
`cargo run --locked --release -p layer-ui --example color_wheel_bench`.
This measures computation only, not GTK snapshotting or display latency.

For real pointer/hold/drag delivery, use
`bash tools/performance/workspace-motion.sh gtk --workspace-switcher`.
That [runner](../../tools/performance/workspace-motion.sh) provides an isolated
D-Bus session, private Wayland runtime, temporary storage and log paths; it needs
Mutter with headless support, GJS and PipeWire in addition to the prerequisites
above. Its compositor setup can also wrap a focused Cargo test like the one above.
Headless Mutter still needs a working Vulkan GPU. Keep native input injection on
that private display; the [input driver](../../apps/layer-linux/bench/native-input.js)
must not control an ordinary desktop session.

For workspace-switching flashes or jumps, run
`bash tools/performance/workspace-motion.sh gtk --workspace-transitions`.
The run directory's `input/transitions.json` records canvas bounds after each GTK
frame, editor sensitivity changes, and transient notices; `steady.png` and
`notice.png` compare the canvas with a recovery notice. The test also checks real
mouse, touch, and keyboard input during a pause. Routine workspace operations
pause input without disabling/restyling the editor. Notices overlay the canvas
so they cannot resize its viewport or GPU surface.

For Group panels, run `LAYER_RESIZE_MIN_HZ=115 bash tools/performance/workspace-motion.sh gtk --column-groups`.
This uses real mouse/touch and private SQLite storage. It measures painted child
allocations in both axes, including the adjacent dock divider, and checks retained
widgets, cancellation and maintenance without closing panels. The run directory
contains per-case JSON and both-theme captures; see [measurements](../history/workspace-motion.md).
