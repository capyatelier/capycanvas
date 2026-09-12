# Web development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The browser client uses DOM controls around Rust compiled to WebAssembly. WebGPU
runs the same canvas renderer used by the native clients. The finished app can be
served as static files and packaged as an installable, offline PWA.

## Prerequisites

Install Rust, Bash, Python 3 and the matching `wasm-bindgen` CLI:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
```

The CLI version must match `wasm-bindgen` in `Cargo.lock`; the command above matches
the current repository. `build.sh` also reports the expected install command.
The browser must expose hardware WebGPU on the device being tested.

## Build and run

```bash
./apps/layer-web/run.sh
```

Open `http://127.0.0.1:4173`. The launcher builds the Wasm module, generates browser
bindings, stages runtime filters and starts a local Python server. Set
`LAYER_WEB_PORT` to use a different port. `LAYER_WASM_BINDGEN` can select a specific
CLI executable.

## How the host works

[`app.js`](../../apps/layer-web/app.js) schedules updates through browser animation
callbacks and applies shared UI changes to the DOM. Rust owns editor behavior;
JavaScript owns browser events, controls and services. The shared session runs on
the browser event loop without requiring a worker, shared memory or a Rust server.

Pointer Events supply pressure and other available pen data. Coalesced and predicted
samples depend on browser support. GPU access, file pickers and installation also
vary by browser and OS; a desktop browser test cannot establish mobile behavior.

GPU startup is staged so controls can appear before the complete shader catalog
is ready. The app reports unavailable GPU access rather than switching painting
to a CPU renderer. The [startup record](../history/web-staged-startup.md) describes
the scheduling work and its measured limits.

The client also persists its workspace in browser storage and connects shared
project requests to browser file access and downloads through
[`documents.js`](../../apps/layer-web/documents.js). Browser storage and picker
capabilities need testing on each supported device.

## Package and test

[Web/PWA packaging](web-packaging.md) documents the additional Node.js and license
and icon tools, the `node apps/layer-web/package.mjs` build, service-worker behavior
and browser checks. The resulting `dist/capycanvas/` directory can be hosted at a
domain root or subpath. Development serving and offline-package testing are
separate workflows.

For the editor workflow against the running development server:

```bash
node apps/layer-web/test.mjs --headless --editor
```

Use `node apps/layer-web/test.mjs --headless --columns` for real pointer checks
of canvas-facing divider double-clicks: normal starting widths, recursive groups,
collapsed columns, undo/redo, and resizing after a reset.

Set `CHROME` to the browser executable and `LAYER_WEB_URL` if the server uses a
different address. For example, macOS can use
`CHROME='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'`.
Linux tests select offscreen Vulkan; other hosts retain their native GPU backend.
The editor check includes actual Navigator pointer hits and a rendered-pixel
check that zoomed paper appears through empty header space. These checks do not
establish complete editor parity or hardware performance.

### Focused browser debugging

Use a recent Node.js and Chrome/Chromium. With the development server above
running, run one focused suite per invocation from the repository root:

```bash
LAYER_TEST_VERBOSE=1 node apps/layer-web/test.mjs --headless --workspace-manager
node apps/layer-web/test.mjs --headless --workspace-switcher
node apps/layer-web/test.mjs --headless --workspace-focus
```

The [harness](../../apps/layer-web/test.mjs) launches a fresh temporary browser
profile, waits for Wasm/WebGPU and workspace readiness, and removes the profile
on exit. It uses Chrome DevTools Protocol directly; no Playwright installation
is needed. Rebuild with `bash apps/layer-web/build.sh` after Rust changes.

Follow [workspace-manager.test.mjs](../../apps/layer-web/workspace-manager.test.mjs)
and [workspace-switcher.test.mjs](../../apps/layer-web/workspace-switcher.test.mjs)
for DOM queries, real pointer injection, state checks and reload assertions.
`evaluate` reads `layerApp.state()` and `JSON.parse(layerApp.app.workspace_view())`;
`call` sends CDP input or captures screenshots. Wait for operations to finish,
and check rendered controls as well as stored state. The switcher suite accepts
`LAYER_TEST_ARTIFACTS` for its screenshot directory.

Failures save `artifacts/ui/web-failure.png` and print page errors and GPU
diagnostics; `LAYER_TEST_VERBOSE=1` also exposes browser stderr. A headless GPU
startup failure or blank canvas needs checking against those diagnostics before
attributing it to UI code. Headless mode still requires supported hardware
WebGPU. On Linux, compare with a headed run on an isolated Wayland compositor via
`bash tools/performance/workspace-motion.sh web --workspace-manager`; see the
[Linux guide](linux.md#focused-ui-debugging) for that runner's dependencies.
