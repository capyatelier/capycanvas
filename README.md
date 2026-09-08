# Capy Canvas

This repository prototypes the portable shared core for Capy Canvas. The Rust engine
owns document semantics and emits batched work to the platform renderer. Platform
clients own their input, window, surface, and presentation timing.

```text
platform input adapter                      platform frame callback
Wayland / Pointer / Pencil /                 Vulkan / D3D12 /
MotionEvent / Web Pointer Events             Metal / WebGPU
              │                                      ▲
              ▼                                      │ one packet / frame
       PenEvent ingress ──► Rust engine ──► CanvasRenderer
                              │
                              ├─ document + reversible edits
                              ├─ brush dynamics + stroke resampling
                              └─ incremental frame packing
```

A non-web input callback uses the bounded SPSC ingress. A single-threaded Wasm
host may use the same producer and consumer on one event loop; the shared API
does not require a separate input or render thread.

The shared crates are UI-toolkit independent. Platform controls bind to the
shared Rust UI session; they are not drawn by Rust. wgpu belongs only
to the canvas renderer and never defines the document, engine, or UI state API.
A hardware GPU is required for painting: there is no software rasterizer,
host-memory canvas, or canvas-composition fallback.
CPU code still owns input normalization, brush dynamics/contact generation,
document commands, UI business logic, persistence, and command encoding; none
of those responsibilities evaluates or modifies canvas pixels.

## Workspace

- `layer-core` — immutable committed strokes, layers, cheap reversible edits,
  revisioned AI requests.
- `layer-render` — the small renderer contract: resolved fixed-size contacts, batches,
  canonical layer snapshots, assets, and `CanvasRenderer`.
- `layer-engine` — platform-event ingress, pressure and brush dynamics, stroke
  resampling, document orchestration, and one render packet per display frame.
- `layer-render-wgpu` — the sole canvas-pixel engine: sparse layers, dry and
  destination-aware brush raster, paint/erase/blend/wet/smudge/liquify,
  prediction, composition, and explicit export readback.
- `layer-ffi` — validated C ABI for batched canvas input and document commands.
- `layer-bench` — repeated 4K ABI benchmarks for non-blocking submission and
  completed GPU work, plus explicit-export PNG brush galleries.
- `layer-ui` — semantic UI state, typed actions, docking, camera/touch,
  validation, and user-flow coordination for platform frontends.

GTK4/libadwaita, Wasm/WebGPU and Android/Compose clients use the same
`UiSession`. Target bindings stay in each app. The shared UI API does not
require threads and compiles for `wasm32-unknown-unknown`. See the
[implementation checklist](docs/ui-implementation.md) for current progress.

## Run the UI

```bash
cargo run --release -p layer-linux
./apps/layer-web/run.sh
bash apps/layer-android/run.sh
```

The web client is served at `http://127.0.0.1:4173` and needs hardware WebGPU.
The Android launcher builds and installs into the tablet emulator; see
[Android SDK setup, validation and performance limits](docs/android-implementation.md).
For a static, installable/offline PWA, run `node apps/layer-web/package.mjs`.
The ignored `dist/capycanvas/` bundle can be hosted at a root or subpath;
see [packaging prerequisites and tests](docs/web-packaging.md). Deployment is
separate and no packaged artifacts are tracked here.
GTK requires Wayland and Vulkan mailbox presentation. A dedicated worker renders
brushes, the full-window viewport and cursor into an app-owned Wayland subsurface
beneath native controls. GTK handles input and UI, not canvas image imports.
An independent 120 Hz timer prepares small frame packets; swapchain waits and
all canvas-pixel work stay off GTK's main thread. Rounded corners are applied in
the viewport shader, without cropping.
Build prerequisites, Linux Chrome flags, test commands, and the manual
tablet/touch checklist are in [UI setup](docs/ui-implementation.md#run).
For a native staging bundle, run `node apps/layer-linux/package.mjs` (also needs
`strip` and `desktop-file-validate`). Run `dist/capycanvas-linux/bin/capycanvas`.
This ignored directory includes the `art.capycanvas.CapyCanvas` desktop launcher,
app icon and license files; GTK/libadwaita remain system dependencies. It does
not install files into your desktop or bundle third-party UI libraries.
The compact native/DOM controls float over a full-window canvas. **Zen mode**
in the top bar (or Z) fades them when the pointer moves away, with no camera
or viewport changes. Brush selectors show real GPU-rendered samples.

Preferences has core-driven Appearance, Canvas, Pen & Input, Keyboard Shortcuts
and About pages. GTK opens its adaptive sidebar dialog from the top-right main
menu; web uses a gear. Edits and shortcut changes use Apply/Cancel, with applied
settings saved across launches. See the [settings design](docs/settings-implementation-plan.md).

## Verify

```bash
cargo test --workspace
cargo run --release -p layer-engine --example hot_path
cargo run --release -p layer-bench -- --scenario painter --repeats 3
```

See [architecture.md](docs/architecture.md),
[gpu-brush-engine.md](docs/gpu-brush-engine.md),
[advanced-brush-engine.md](docs/advanced-brush-engine.md),
[brush-renderer.md](docs/brush-renderer.md),
[rendering-program.md](docs/rendering-program.md),
[platform-adapters.md](docs/platform-adapters.md),
[shared-ui.md](docs/shared-ui.md), [canvas-ffi.md](docs/canvas-ffi.md),
[gpu-raster-benchmarks.md](docs/gpu-raster-benchmarks.md), and the running
[rendering optimization log](docs/optimization-log.md) for the design,
measurements, and implementation sequence.

The implemented painterly GPU state model is summarized in
[painterly-paint-state.md](docs/painterly-paint-state.md). The
[benchmark and gallery commands](docs/gpu-raster-benchmarks.md) generate painter,
destination-interaction, watercolor and conductance review sheets locally.
Generated screenshots, traces and benchmark reports live under ignored
`artifacts/`; they are not distributed in this source repository. Runtime brush
previews are intentionally included and can be regenerated by the GPU harness.

Predictive, tip-locked drawing behavior and its cross-platform input mapping are
specified in [instant-stroke-feedback.md](docs/instant-stroke-feedback.md).

## License

This project's original code and non-brand assets are dual-licensed under
[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option.
The **Capy Canvas / Capy Atelier names and capybara mark are excluded** and have
separate [branding terms](BRANDING.md). Modified public distributions must use
their own branding unless permission is granted; accurate attribution is allowed.
Dependencies and attributed third-party material retain their own licenses and
distribution requirements; see [third-party notices](THIRD_PARTY_NOTICES.md).
Do not vendor third-party code, artwork, or fonts without checking their license
and required notices; prefer original implementations and Apache-compatible assets.
See the [publication checklist](docs/publication.md) for provenance, dependency
checks and the separate requirements for shipping compiled applications.

Unless explicitly agreed otherwise, software and non-brand asset contributions
intentionally submitted for inclusion in this project are offered under both
licenses, without additional terms or conditions.
