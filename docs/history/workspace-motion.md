# GTK and Web workspace motion

The GTK and Web migrations consume `UiSession::workspace_update()` from
`c3840f5`. A matching `model_revision` retains all panel models and content.
Every delivered input phase still enters the shared Rust gesture state machine;
only pending absolute presentation is replaced before the next display frame.
Drop targets, tab insertion policy, cancellation and history remain shared.

GTK applies retained child allocations on its frame clock. Native picking,
clipping, resize handles and GPU Navigator placement follow those allocations.
Tab slides retain GSK render nodes and frozen insertion slots. Web applies
native-pixel-aligned DOM translations in `requestAnimationFrame`, retaining the
controls and tab insertion slots. Both refresh models when their revision changes
and rebase any active placement before continuing. Completion and cancellation
clear pending motion immediately.

Web Navigator uses a native-resolution WebGPU canvas inside its DOM panel. It
samples the existing Rust renderer's document image using the shared overview
shader. Document/camera changes repaint it; translating the panel needs neither
GPU presentation nor document composition. Browser stacking, scrolling and
overflow clipping carry the retained canvas with the controls. This avoids a
full-window GPU redraw on each workspace placement. No control screenshots,
preview readback, or scaled control textures are introduced.

The Web collapsed-column footer grips also use the correct horizontal rotation,
and collapsed-panel icons are centered horizontally.

## Reproducing validation

Run from the repository root with the local GTK, Mutter, GJS, PipeWire, Chrome,
Wasm target and matching wasm-bindgen dependencies installed:

```sh
LAYER_MOTION_MIN_HZ=115 tools/performance/workspace-motion.sh gtk
LAYER_MOTION_MIN_HZ=115 tools/performance/workspace-motion.sh web
tools/performance/workspace-motion.sh web --workspace-rendering
tools/performance/workspace-motion.sh web --workspace-acceptance
tools/performance/workspace-motion.sh gtk --workspace-tabs
tools/performance/workspace-motion.sh gtk --workspace-hold
cargo test --locked --release -p layer-render-wgpu overview -- --test-threads=1
cargo test --locked -p layer-ui -p layer-host -p layer-workspace
```

The runner creates a private D-Bus session and a 1600×1000, 120 Hz Mutter
Wayland monitor. Its virtual mouse/touch events travel through Mutter and the
toolkit's actual native input/capture machinery. It never drives the user's
desktop. `LAYER_MOTION_REFRESH` changes the monitor rate; `LAYER_WEB_PORT` changes
the local server port. The minimum rate is optional for slower test machines.
Raw measurements and screenshots are written to `artifacts/workspace-motion/`.

GTK motion/resize benchmarks and the drawer pixel matrix use the in-memory
workspace fixture. This keeps asynchronous workspace adoption and periodic
storage maintenance from replacing the fixture or interrupting input. Native
drawer/column interaction tests and workspace-manager integration tests retain
their isolated storage; the timing benchmarks do not measure storage latency.

Each benchmark supplies 550 motion samples at requested 4 ms intervals for floating groups,
attached tabs, torn-off tabs and a visible Navigator, using both mouse and touch.
The steady-state probe starts after gesture activation and startup work finishes.
It checks retained content, unchanged model revisions, zero full model refreshes,
native geometry matching the shared update, drop, and single-step undo/redo.
Web also asserts zero legacy drop/tab queries and zero GPU frames during motion.
GTK checks native picking and frozen tab hit slots. Platform input coalescing is
expected; the frontend dispatches every event it actually receives.

The rendering test checks seven collapsed-column icons at both 1× and 2×:
horizontal center error is below 0.01 CSS pixels and footer grips rotate 90°.
Dragging retained controls changed 72/168,000 sampled channels at 1× (maximum
difference 1/255) and 219/672,000 at 2× (maximum 6/255). More than 99.95% were
identical. The test rejects changes exceeding 0.1% of channels or 8/255 in any
channel. Navigator image pixels were identical before/during movement at both
scales; the native canvases were 338×164 and 676×328 pixels. Camera changes
updated the outline. Screenshots are decoded with CPU-readable canvases, as in
the existing Web harness, to avoid unreliable GPU 2D readback on this driver.
The test also exercises a concurrent theme refresh and cancellation before a
queued placement frame runs.

Web acceptance passed the existing editor, mouse/touch drawer, drag-cursor and
long-press suites together: clipped insertion targets, merge and split drops,
resize, capture loss, blur, cancellation, undo/redo, Navigator controls, nested
drawers and project persistence. GTK native tab-slide tests passed at 1× and 2×,
the native long-press suite passed, and the merged main menu test passed through
the same input driver. Shared host/UI tests passed (20 host and 260 UI tests),
as did all four applicable GPU overview regressions and 19 Web package/launcher
tests. The GPU tests compare standalone overview pixels with existing viewport
overview pixels and check transparent margins. Shared overview regressions also
cover live paint and camera changes.

## Measured results

Final runs were sequential, with no other acceptance suite running alongside
them. All 16 cases passed the optional 115 Hz floor on the 120 Hz monitor.

| Input | Scenario | GTK presentation Hz | Web placement Hz |
| --- | --- | ---: | ---: |
| Mouse | group | 120.01 | 119.94 |
| Mouse | tab | 120.00 | 120.21 |
| Mouse | tear-off | 118.21 | 119.93 |
| Mouse | navigator | 120.00 | 120.08 |
| Touch | group | 120.00 | 120.17 |
| Touch | tab | 120.00 | 120.17 |
| Touch | tear-off | 119.99 | 120.03 |
| Touch | navigator | 120.02 | 120.21 |

Every case performed **zero full model refreshes** during steady dragging.
Web also performed **zero GPU frames**, including both Navigator cases.
Worst-case per-scenario p95 input dispatch was 0.060 ms on GTK and 0.20 ms on
Web; p95 placement application was 0.040 ms and 0.10 ms respectively. These
are well below the 8.33 ms frame interval. GTK received 264–268 mouse events
and all 550 touch events per case; Chrome received 267–270 native events per
case after toolkit coalescing.

## Measurement scope

GTK drawer parity validation on 2026-09-12 retained 116.42–120.02 Hz dragging
and 118.65–119.11 Hz resizing across native mouse and touch. All 14 cases passed
the 115 Hz floor with zero full model refreshes; maximum per-case p95 dispatch
was 0.052 ms for dragging and 0.030 ms for resizing. These follow-up runs used
the in-memory fixture described above. A preliminary storage-enabled tear-off
run reached 110.61 Hz, and another encountered fixture/input interference;
the isolated results do not establish the same rate during storage activity.

Measured on 2026-09-11 using an NVIDIA RTX PRO 6000 Blackwell Max-Q Workstation
Edition, NVIDIA 610.57.04, Mutter 50.4, GTK 4.22.4, Chrome 152.0.7977.64,
release Rust/Wasm builds and hardware Vulkan/WebGPU. GTK reports completed GDK
presentation timestamps; Web reports placement callbacks on the browser display
clock, with a separate count of frames whose native styles changed. Pixel
rounding can legitimately leave adjacent tab frames identical.
CPU timings cover input dispatch and placement callbacks, not total native
paint/compositor/GPU time. Web timer resolution is approximately 0.1 ms.

The virtual monitor validates sustained display-paced delivery and UI cost. It
does not measure physical scanout, physical touch hardware, or end-to-end input
latency. Web callback frequency is not proof that every frame reached a physical
display. These results do not promise 120 Hz on all GPUs, browsers or displays.
The samples exclude initial tear-off/model rebuilding and final drop work;
those boundaries deliberately refresh models. Concurrent content changes may
also legitimately refresh models and render new pixels.
