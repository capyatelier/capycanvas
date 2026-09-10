# Native and web workspace implementation

Paths under `artifacts/` refer to ignored local outputs, not files shipped in
this repository. See [publication notes](publication.md#publication-checks).

## Contract

`layer-ui` owns semantic controls, command availability, validated settings updates,
workspace docking, camera transforms, and shared mouse/pen/touch interpretation.
GTK4/libadwaita and the browser DOM own widgets, accessibility, focus, native
event collection, drag visuals, and surface lifecycle. Both use the same Rust
library; the web compiles it to Wasm.

Dock bands are ordered from the outside of the workspace toward the canvas.
Each consumes a strip on its top, bottom, left, or right edge. Earlier bands
own corners. Two right bands produce two adjacent columns. Inside a band,
binary splits contain tab groups. Stable node and panel IDs survive moves.
Frontends may translate this topology to native containers or use the shared
logical-pixel allocation result; they do not reimplement docking rules.

The default layout has a transparent native header (Zen icon at top-left,
Edit, View, document title and native window controls), a compact lighter tool
ribbon, Brushes/Brush Size on the left (68%/32%), Layers on the right, and a
canvas-status HUD. The ribbon and HUD end between the side panels; the HUD stays
above bottom docks. Workspace gaps are 6px: between panels, from panels to the
window sides/bottom, between header menu buttons, and above/below the 36px header
controls. Zen is 36×36px, aligned with a left ribbon at x=6px. Docks start directly
after the 48px header (6 + 36 + 6), without another outer top margin.
Standalone ribbons are flush, with tab-bar-matching trailing grips; tabbed Tools
retain a 4px content inset. The GPU canvas covers the entire window beneath all controls.
The surround is `#333333` dark / `#B8B8B8` light. Header controls, title and status
text use that background, invisible against the default surround but readable
over zoomed artwork. There is no full-width footer or header background.
Only one document is supported; its title is in the header rather than a tab bar.
Panels use compact native typography, soft shadows, and borderless light/dark
theme surfaces: darker tab bars, lighter contents, selected tabs joined to the
content with concave bottom shoulders, and input fields filled with `#333333`
in dark mode / `#FAFAFA` in light mode (the native slider knob interior). System
appearance is the default and updates live; Settings offers Light/Dark overrides.
The browser uses the same geometry and color roles with DOM
controls, leaving operating-system window decoration to the browser.
Web adds a 36px fullscreen button immediately left of Settings, using two-arrow
enter/exit SVGs from the existing icon bank. The browser Fullscreen API owns
this window state; `fullscreenchange` updates the icon and label even when the
browser exits fullscreen outside the button. Fullscreen covers the document
root, keeping Settings dialogs and the full UI visible. Unsupported fullscreen
is disabled and rejected requests leave a usable button with a brief message.
Canvas resizing uses the existing resize path; there is no GPU reinitialization
or shared-session fullscreen state, and GTK is unchanged.
The web canvas and its SVG cursor paths explicitly disable selection, image
dragging, tap highlighting and Safari touch callouts. The cursor stays outside
pointer hit testing; copyable document names and text fields are unchanged.
Hover styling follows the most recent pointer event, not a device/browser
allowlist: touch suppresses sticky hover until a mouse or pen moves or presses.
Pressed feedback and intentional selected/toggle colors remain unchanged,
including Zen mode. Hybrid touch/pen/mouse devices can switch immediately.
Both hosts leave 8px between the visible grip dots and the trailing panel edge,
matching the first tab label's left inset. The drag targets remain 20×24px
(24×20px at the bottom of vertical ribbons).

Both hosts use the shared Rust `UI_TEXT_PT` constant (11 pt) for all UI text,
including panel content, tabs, headings, preferences, menus and the zoom/rotation
status HUD. It is not a user setting. Text-bearing controls and inline +/− icons
use font-relative units; sliders, 16px checkboxes, 40px brush previews and 36px
tool tiles with 16px icons stay fixed. Brush entries retain 2px gaps and
size samples retain their preview space inside 3px-padded grid cells. Future
panel context menus use the same typography role. One overlay scrollbar
wrapper keeps the browser's native scrolling without consuming preview width.
Layer rows are reconciled only when their identity/order/labels change; opacity
and visibility updates keep controls mounted, including during pointer capture.
Preferences uses shared Rust page/group/row definitions and immediate validated
updates. Done only dismisses the view; each accepted change requests persistence.
GTK presents an adaptive native sidebar dialog; web mirrors its layout with a
gear in the header. Appearance, Canvas, Pen & Input, Shortcuts and About are
implemented. See [settings design](settings-implementation-plan.md).
Resize handles and grip backgrounds remain transparent through hover/press;
only panel-move insertion markers are accented. Number buttons repeat while
held; Enter in a number field does not implicitly dismiss Settings. Menus are
exclusive, and an outside canvas contact dismisses a menu/popover without also
painting. All panels use the same `0 2px 8px` black/16% shadow.

Web canvas, images, grips, tabs, and menus are not text-selectable and have no
tap or keyboard-focus halo (pen-first presentation). Document titles, status
values, and editable fields remain copyable. This changes visual highlighting,
not control semantics, shortcuts, or editable-field keyboard handling.
Application icons have one original SVG bank in `apps/layer-web/icons`, served
directly by web and embedded as a GTK resource by the native build. Command icon
identity comes from Rust, including layer controls; grips, spins and checkboxes
use the same assets. Settings search uses GTK's installed `edit-find-symbolic`;
web uses the project's own search SVG. GTK keeps native check/spin widgets and device-scale vector
rendering. Number inputs use tabular numerals in both hosts, with no separator
border beside the step buttons. No GNOME artwork or fonts are vendored; web
uses locally installed Adwaita Sans with a system-font fallback. Font metrics on
other systems, native browser color/select popups, OS window icons, and the absence
of an OS close button in web content are intentional platform differences.
For outlined SVGs, put `transparent-fill foreground-stroke` on each path,
circle or rectangle, not only its parent group: GTK's symbolic recoloring
otherwise fills the child shapes. The native/web pixel check includes hollow
eye centers in both themes to catch this independently of layout measurements.

Canvas cursors are shared Rust presentation data: brush-size outline (default),
outline with cross, cross, dot, or none. GTK's canvas worker draws the vectors in
the shared GPU viewport pass;
web uses a non-interactive SVG below the floating controls. A black 1px outline
with alternating white 3px dashes stays legible on light and dark artwork without
sampling canvas pixels. Pointer samples and settings/camera changes invalidate
the overlay, coalesced by the host display frame; hover never wakes paint raster.
The engine resolves cursor contacts through the same dynamics as painting:
pressure, aspect, rotation, tilt/twist/direction, size/rotation variation, flips,
and multi-contact scatter. Preview evaluation does not advance paint RNG or
deposit ink. A hovering pen shows nominal pressure; contact follows live pressure.

Mask silhouettes are traced lazily once from source R8 brush assets at 10%
coverage, including holes and the shader's circular clip. The source texel
resolution sets contour precision. The renderer caches vector metadata and
retains the small source brush masks, not canvas pixels; no GPU readback occurs.
Analytic tips use exact ellipse arcs. Camera zoom/rotation and device scale are
applied in Rust before stroking, so line thickness and dash lengths stay constant.
This is the primary stamp footprint, not a preview of grain/dual-mask modulation,
pigment diffusion, or the final mark's opacity. Randomized tools show a provisional
next contact rather than predicting an entire future stroke. Pointer exit, touch,
pan, popups, and focus loss suppress the drawing cursor.

Web requests `cursor: none` for both mouse and pen over the canvas, switching to
`grab` for panning. Chrome 152.0.7977.64 on Wayland has a browser-side limitation:
[tablet proximity sets a default arrow](https://chromium.googlesource.com/chromium/src/+/refs/tags/152.0.7977.64/ui/ozone/platform/wayland/host/wayland_tablet_tool.cc),
whereas [cursor hiding and custom bitmaps update only `wl_pointer`](https://chromium.googlesource.com/chromium/src/+/refs/tags/152.0.7977.64/ui/ozone/platform/wayland/host/wayland_cursor.cc).
The tablet cursor is separate, so it can remain visible alongside our outline.
A transparent CSS cursor cannot fix that routing. Chromium needs to apply the
window cursor to the active tablet tool, including a null tablet cursor surface
for `cursor: none`. Packaged browser tests check mouse/pen hover, drawing, release
and exit at the DOM level; CDP input does not exercise native Wayland tablet
delivery and cannot establish that the physical stylus cursor is hidden.

The default edge-reveal Zen mode fades chrome over 180 ms when the pointer is away. Hidden chrome reveals only
within 80 logical pixels of the top edge or an edge with a visible dock panel.
Empty left/right/bottom edges do not reveal; the status HUD is not a bottom panel.
Visible chrome retains its original
80px control-relative hide margin, and remains visible in the edge reveal zone.
Menus/settings and keyboard navigation keep
controls available. Hidden controls are not invisible click targets. Fading
does not move the artwork, resize the viewport or wake the canvas frame loop.
The shared work-area rectangle affects initial/explicit Fit Canvas. Zen is the
only global visibility toggle; Workspace manages individual panels. GTK also
offers With button and independent button visibility through Preferences or its context menu; see
[Zen modes](shared-ui.md#window-chrome-and-zen-mode).

The brush selector displays real GPU-rendered stroke samples, including seeded
destination interactions for blending and liquify tools. Both clients reuse
the same bundled transparent dark/light PNGs. Regenerate from the repository root:

```bash
cargo run --release -p layer-bench --bin gpu-bench -- --brush-previews
```

Actions are typed values with stable command/panel/layer identities. Commands
have one availability source for buttons, menus, shortcuts, and accessibility.
Changed-region flags tell hosts which cached views to refresh. Pointer batches
take a separate compact numeric path and do not serialize the UI state.
The web adapter reinterprets signed 32-bit DOM `pointerId` values as unsigned
32-bit IDs before both UI routing and packed brush samples reach Rust's `u64`.
This preserves negative Safari IDs without collisions or float-to-unsigned
clamping; DOM capture/release and host-only comparisons keep the original ID.
One synchronous `UiSession` calls the existing engine directly and refreshes
derived state. UI snapshots never become a second editable document model.

## Milestones and acceptance

- [x] Shared Rust layout: edge priority, adjacent bands, nested splits, tabs,
      resize, moves, invalid-action rejection, and deterministic allocations.
- [x] Shared state/actions: brush selection, size presets and slider, color,
      brush/eraser, layers (select/create/delete/reorder/visibility/opacity),
      undo/redo, reset view, theme, and transactional settings presentation.
- [x] Shared interaction: mouse and pen pressure, cancellation, anchored
      pan/zoom/rotate, two-touch gesture tracking, and transform revisions.
- [x] GPU viewport presenter: camera transform and canvas background, no live
      pixel readback; native lifecycle and browser asynchronous initialization.
- [x] GTK4/libadwaita shell using the contract, native controls and stylus
      history, movable panels/tabs, resizable splits, light and dark themes.
- [x] Browser shell using Wasm state and WebGPU, DOM controls, coalesced pointer
      input, matching docking and touch behavior, light and dark themes.
- [x] Shared behavior tests, actual Wasm/browser tests, GTK integration checks,
      screenshots of both themes/platforms, and visible drawing verification.
- [x] Document launch commands and commit/push working milestones.

Performance validation separates brush rendering, GTK CPU work and compositor
presentation. The browser reconfigures lost/outdated surfaces. GTK's dedicated
GPU worker uses one Vulkan device/queue for brush raster, layer composition,
the viewport and brush cursor. Its mailbox swapchain presents an app-owned
desynchronized Wayland child beneath the transparent GTK parent. The child has
an empty input region; GTK still receives all native events. Input, camera and
presentation coordinates remain full-window and top-left. A premultiplied-alpha
viewport shader rounds the outer corners, with no rounded clipping in GTK's
canvas path and no crop or strips. Unsupported Wayland/alpha/mailbox capabilities
produce an explicit startup error, not an alternative renderer.

`canvas.rs` owns the main-thread session adapter and independent 120 Hz timer;
`render_thread.rs` owns frame handoff and GPU/WSI operations; `wayland.rs` owns the
child and a separate event queue on GTK's borrowed connection. Geometry updates
request a parent commit through GTK. Neither the worker nor Vulkan dispatches
GTK's event queue or commits GTK's surface. There is no per-frame GTK texture
import or GTK cursor repaint. GPU surface acquisition/waits stay on the worker.
The frame handoff is capped at two in-flight paint frames, including current
work. A full handoff leaves pen records queued; it never drops a paint packet.
Only small contact/layer-property/cursor records cross threads, not pixel images
or layer stroke-history lists. The timer and worker sleep when idle. Surface
timeout/occlusion retries retain the latest viewport without replaying paint;
lost/outdated surfaces are recreated/reconfigured on the worker. Device-loss
reconstruction remains unvalidated. Shutdown joins the worker before GTK
destroys the borrowed parent.
One Vulkan instance is retained for the process: destroying an instance per
window reproduced an NVIDIA Wayland WSI crash in GTK's next window. Window
devices, swapchains, textures and child surfaces are still released normally.

## Current verification (2026-09-07)

Current native measurements and reproduction are in
`artifacts/benchmarks/gtk-wayland.md`. They measure GPU timestamps,
full GTK interaction/frame callbacks, timer wake lateness and compositor
`wp_presentation` feedback. A separate virtual-pointer test enters through Mutter,
Wayland and GDK rather than bypassing native event dispatch. These tests establish
sustained throughput on an isolated 120 Hz compositor, not physical-device polling
rates, hardware scanout timing or a hard real-time guarantee. Instrumentation is
test-only. Explicit screenshots read back a viewport only on request and
temporarily place it in a GTK picture for WidgetPaintable; timing runs never do
this. Compositor-level captures separately validate the actual child/GTK stacking.

The shared-core audit is complete; the findings and host/core boundary are in
[workspace-core-audit.md](workspace-core-audit.md). Both hosts consume the same
control catalog, durable workspace restore, divider lifecycle/nudges, viewport
fitting, toolbar geometry, shortcuts, Zen visibility and canvas input routing.

- The Rust workspace passes 115 tests, including 49 shared-UI tests. Eight
  desktop-only tests are skipped by that command and run separately as relevant.
  Native/wasm Clippy and all seven launcher discovery tests pass.
- GTK's native geometry fixture, fresh-window workspace restore and full
  controls/docking/ink integration tests pass on Wayland.
- Native and browser preferences tests cover all five pages, adaptive navigation,
  recording/conflict replacement, validation, persistence and restored shortcuts.
- The focused hardware Wasm/WebGPU GTK/web parity suite and full browser
  interaction suite pass in isolated Wayland Chrome with the new shared viewport
  shader. No X11 access or native window-title dragging is automated.

Core coverage includes validated atomic restore, editing restored IDs, repeated
restore at several viewport sizes, catalog/command consistency, native/DOM
control limits, divider grab offsets and nudges, first-viewport fit without
refitting dock edits, keyboard modifier/repeat/editing guards, pointer ownership,
cancellation, back-to-back strokes before a frame, and occupied-edge Zen reveal.

Cursor coverage also checks mask rotation/flips/aspect under camera rotation
and HiDPI, live pressure, unchanged paint RNG/document data, settings and pan
suppression, native GPU cursor geometry, and actual browser hover listeners.
`artifacts/ui/cursors/` contains native review PNGs and warm cursor-preparation
timings; browser equivalents are `artifacts/ui/parity/web-cursor-*.png`.
The cursor test evaluates 1,000 warm geometry preparations per preset (512px
brushes). Those are CPU preparation timings, not total GPU frame time or
pen-to-photon latency. Current frame benchmarks include GPU cursor rendering.
The bundled source R8 assets retained for lazy contour tracing total about
516KiB of host memory; the native worker prepares immutable contours once for
main-thread access, with no added
canvas-sized texture or GPU-to-CPU transfer. GTK 2× reference captures verify
the checkbox and shared icons at device resolution.

The GTK fixtures use actual native widgets and control/stylus signals, plus
pressure-varying pen records. They verify layer edits/undo, settings, dock moves,
tab selection, stable resizing, camera transforms, visible GPU ink, and hide/remap.
The restore test creates a new window, restores serialized workspace state and
compares active tabs and actual allocated bounds. No physical tablet delivery is
claimed; native timestamp/history wrap behavior has a separate unit test.

The browser runs the actual release Wasm app with hardware WebGPU in a visible
Wayland Chrome window through CDP. It checks ink in the presented framebuffer,
pressure, Space-pan without pen records, controls/layers/undo, settings, drag/drop,
captured-pointer resizing, touch camera gestures, Zen reveal and modal pinning.
The full test now reflects the occupied-edge rule: the bottom HUD does not reveal
hidden UI. Synthetic hover/assertion pairs execute together to avoid interleaved
desktop motion; this is not a physical-device test.

Matching native/web review captures and widget measurements are in
`artifacts/ui/parity/`: dark/light, Settings, Zen, wrapped and tabbed tools.
At 1200×900 logical pixels with matching display scale, measured panel/control
geometry differs by at most one logical pixel (native integer allocation versus
CSS subpixels). Font metrics, 36px controls, 6px workspace gaps, 2px tool gaps,
grip insets, 24px native close circle, settings persistence, copyable titles,
nonselectable chrome, no focus halos, live opacity and empty-edge Zen all pass.
The browser restores a saved workspace into a separate real Wasm/WebGPU session.
The parity harness forces sRGB screenshots for comparable samples: dark/light
panel backgrounds and shadow profiles match GTK on this machine. Normal app
color management is unchanged; no GNOME fonts/icons were imported.

Fresh full-suite captures also include `artifacts/ui/gtk-dark.png`,
`gtk-light.png`, `web-dark.png`, `web-light.png`, and Zen/zoom/docking variants.
GTK and browser tests run sequentially to avoid occlusion and desktop input
interference. Earlier pointer/presentation failures are not presented as current
passes; the runs listed above completed successfully.

No brush algorithm or live canvas readback was added by the native presentation
refactor. The historical `artifacts/benchmarks/gtk-vulkan.md`
reports completed-move p99 at a 2400×1800 physical viewport: Natural Blender
1.341ms, Wet Round 0.877ms, Watercolor Wash 2.404ms. These are not new measurements,
input-to-photon latency or universal 120Hz guarantees. The report also records
dependency validation diagnostics in the superseded image-import path.

Automatic workspace disk storage/named-workspace selection is deferred; the
versioned serialization and restore contract is implemented. Physical tablet/
touch delivery, native WM title grabs and device-loss recovery remain explicit
manual/unvalidated boundaries.

## Run

Prerequisites are Rust, the `wasm32-unknown-unknown` target, Python 3, and the
matching wasm-bindgen CLI. Linux also needs GTK4 4.22/libadwaita 1.9 or newer,
development libraries, a Wayland session and a hardware Vulkan driver supporting
mailbox presentation with premultiplied alpha. There is no X11, GLES or CPU canvas
fallback. GTK chooses its own scene renderer for native controls; this is separate
from the canvas's Vulkan renderer. Hardware tests use `GSK_RENDERER=vulkan` for
consistent control rendering. GTK's image-export/import capabilities are not a
canvas requirement.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked

# Native app, from the repository root:
cargo run --release -p layer-linux

# Browser app, from the repository root, in a separate terminal:
./apps/layer-web/run.sh
```

The web launcher looks for `wasm-bindgen` on `PATH`, then in
`${CARGO_HOME:-$HOME/.cargo}/bin`; Cargo's bin directory need not be on `PATH`.
Set `LAYER_WASM_BINDGEN` to explicitly select a CLI executable or command name.
An invalid override reports an error instead of silently using another tool.

Open `http://127.0.0.1:4173` in a hardware-WebGPU-capable browser. On the tested
Linux Chrome setup, use a dedicated profile and these GPU flags if required:

```bash
capy_profile=$(mktemp -d /tmp/capy-chrome-XXXXXX)
google-chrome --user-data-dir="$capy_profile" --ozone-platform=wayland \
  --no-first-run --no-default-browser-check \
  --enable-gpu --enable-unsafe-webgpu --use-angle=vulkan \
  http://127.0.0.1:4173
```

Do not additionally force Chrome's Vulkan compositor feature flags: they can
freeze the browser on some Linux configurations. The flags above leave Chrome's
compositor selection alone while enabling hardware WebGPU for the app.

Left-drag paints; Space + left-drag and middle/right drag pan. Wheel pans,
Shift-wheel pans horizontally and Ctrl-wheel zooms. Releasing Space before the
mouse finishes the existing pan without starting a stroke.
Two fingers pan/zoom/rotate without painting. B/E select brush/eraser, F fits,
Z toggles Zen mode, Tab reveals controls for keyboard navigation,
and Ctrl+Z/Ctrl+Shift+Z undo/redo. These bindings are configurable. GTK Preferences
lives in the top-right primary menu, alongside New Window, Keyboard Shortcuts
and About Capy Canvas; web uses a direct gear button. View contains Fit canvas,
Zen mode, Dark Mode and Reset layout. Drag panel grips/tabs: header slots insert/reorder tabs, body
centers append tabs, narrow body edges split beside a panel, and workspace
edges create dock bands. A highlighted insertion line previews the result. Drag
dividers or focus them and use arrow keys. Settings use the shared auto-saving
model with native presentation.
Ribbons contain only 36-unit square tiles; color/opacity open popovers. They
default to one row on top/bottom, one column on a side, and wrap when resized
across that axis. Standalone grips sit at the right/bottom; tabbed ribbons have
no additional grip inside their content. The tab-bar grip remains visible and
moves every tab as one unit; dragging a tab label still moves only that tab.
Right-click or touch-hold panels, tab headers and tiles to customize them. A tap
on the selected tab toggles its live two-column configuration; another tab
switches that view without closing it. The combined group and drawer cast one
stronger shadow. Named toolbars, tile contents, control visibility and tab styles
belong to the shared workspace; see [panel customization](panel-customization.md).
Zen uses the same right-facing capybara silhouette SVG on both platforms, with
the normal tool-selection highlight when enabled. Decoration controls use the
standard button radius, except the circular close button, and menus have no carets.

Human check: draw/erase with a mouse, then a real pressure-sensitive pen; check
pressure, tilt and fast curves; pan/rotate/pinch with two fingers; move/resize
docks and dismiss settings without losing the drawing. Applied preferences persist
to the native configuration directory or browser localStorage; workspace layout
disk storage and document save/import/AI controls remain outside this UI milestone.

## Verify

```bash
cargo test --workspace -- --test-threads=1
cargo check -p layer-web --target wasm32-unknown-unknown
node --test apps/layer-web/run.test.mjs
GDK_BACKEND=wayland GSK_RENDERER=vulkan G_DEBUG=fatal-criticals cargo test --release -p layer-linux native_workspace_controls_docking_and_ink -- --ignored --test-threads=1
# Run separately: GTK initialization belongs to one thread per process.
cargo test --release -p layer-linux native_frame_pacing -- --ignored --test-threads=1 --nocapture
# With the web server running, after the GTK test exits:
node apps/layer-web/test.mjs
# Focused parity (regenerate GTK references first; no OS pointer injection):
GDK_BACKEND=wayland GSK_RENDERER=vulkan cargo test --release -p layer-linux native_web_parity_reference -- --ignored --test-threads=1
node apps/layer-web/test.mjs --parity
# Use a fresh isolated settings path for the native persistence test:
preferences_test_dir=$(mktemp -d /tmp/capy-preferences-XXXXXX)
GDK_BACKEND=wayland GSK_RENDERER=vulkan LAYER_SETTINGS_FILE="$preferences_test_dir/settings.json" \
  cargo test --release -p layer-linux native_preferences_and_shortcuts -- --ignored --test-threads=1
node apps/layer-web/test.mjs --preferences
# Headless real-GPU UI checks when Chrome's Wayland import path is unavailable:
node apps/layer-web/test.mjs --package --preferences --headless
```

The browser test supplies the Linux Vulkan flags documented by the
[WebGPU implementation-status page](https://github.com/gpuweb/gpuweb/wiki/Implementation-Status).
WebGPU canvas readback in a later task can observe a cleared backbuffer; the
test therefore inspects Chrome's presented screenshot instead. Pixel readback
is confined to tests, not application rendering.

## Platform sources

- [Vulkan Wayland WSI](https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html#_wayland_platform)
  defines Vulkan's ownership of swapchain attachment/commits on the supplied surface.
- [GDK Wayland surface access](https://docs.gtk.org/gdkwayland/method.WaylandSurface.get_wl_surface.html)
  exposes the borrowed native parent; GTK continues to own that surface.
- [Wayland presentation feedback](https://wayland.app/protocols/presentation-time)
  reports actual presentation or discarded updates, distinct from GPU completion.
- [GTK GestureStylus](https://docs.gtk.org/gtk4/class.GestureStylus.html) exposes
  tablet axes and backlog samples.
- [libadwaita integration](https://gtk-rs.org/gtk4-rs/stable/latest/book/libadwaita.html)
  provides native controls and runtime light/dark styling.
- [Pointer Events](https://www.w3.org/TR/pointerevents3/) defines pointer
  identity, capture, pressure, coalesced samples, and cancellation on the web.
