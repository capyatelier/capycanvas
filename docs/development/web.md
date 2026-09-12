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

## Debug headless Chrome

Run from the repository root with the development server in another terminal,
Node.js 22 or newer, and Chrome/Chromium:

```bash
LAYER_TEST_VERBOSE=1 LAYER_TEST_ARTIFACTS=artifacts/web-debug \
  node apps/layer-web/test.mjs --headless --column-drops
```

Select one scenario per run; other useful selectors are `--drawer-style`,
`--drag-pickup`, `--workspace-manager`, `--workspace-switcher` and `--workspace-focus`. See the dispatch
in [`test.mjs`](../../apps/layer-web/test.mjs) for the full list. It launches Chrome
with a temporary profile, uses Chrome DevTools Protocol (CDP) over
`--remote-debugging-pipe`, and removes the profile afterward. It does not expose a
TCP debugging port or require Playwright. Rebuild with
`bash apps/layer-web/build.sh` after Rust changes; the test does not build. The
runner waits for Wasm/WebGPU and workspace readiness. Verbose mode prints browser
diagnostics; scenario-specific screenshots go to `LAYER_TEST_ARTIFACTS` when supported.

Follow [workspace-manager.test.mjs](../../apps/layer-web/workspace-manager.test.mjs)
and [workspace-switcher.test.mjs](../../apps/layer-web/workspace-switcher.test.mjs)
for DOM queries, pointer input, state checks and reload assertions. Wait for
operations to finish and check rendered controls as well as stored state.
Reuse the runner's `call`, `evaluate` and `settle` helpers for CDP input,
`Runtime.evaluate` and `Page.captureScreenshot`. Useful page expressions are
`layerApp.state()`, `JSON.parse(layerApp.app.workspace_view())` and
`layerApp.startupTimes`; check `#gpu-notice` if startup never completes.

On failure the runner attempts to save `artifacts/ui/web-failure.png` and prints
page errors and GPU diagnostics. Check those when a headless run has a blank
canvas or cannot start WebGPU; hardware WebGPU remains required. On Linux, compare
with headed Chrome inside an isolated Mutter compositor using
`bash tools/performance/workspace-motion.sh web --workspace-manager`; see the
[Linux guide](linux.md#focused-ui-debugging) for that runner's dependencies.

## Test and debug Web on Android

Use the device selection and USB debugging setup in the [Android guide](android.md).
Keep the development server running, save user work, and use a dedicated test
origin: this harness attaches to the device's existing Chrome profile and some
scenarios modify documents or browser storage. The example uses the default
development port; keep the URL and forwarding ports consistent if changing it.

```bash
adb -s "$CAPY_ANDROID_SERIAL" reverse tcp:4173 tcp:4173
adb -s "$CAPY_ANDROID_SERIAL" forward tcp:9228 localabstract:chrome_devtools_remote
adb -s "$CAPY_ANDROID_SERIAL" shell am start -a android.intent.action.VIEW -d http://127.0.0.1:4173/ -p com.android.chrome
curl --max-time 5 http://127.0.0.1:9228/json/list
LAYER_DEVICE_CDP=http://127.0.0.1:9228 \
  LAYER_WEB_URL=http://127.0.0.1:4173/ LAYER_TEST_ARTIFACTS=artifacts/web-android \
  node apps/layer-web/device.test.mjs --drawer-style
```

[`device.test.mjs`](../../apps/layer-web/device.test.mjs) finds the tab by its exact
URL, including the trailing slash, connects to its CDP WebSocket and reloads it.
It supports a subset of desktop scenarios; check its dispatch before choosing a
flag. Android Chrome remains visible on the device; automation uses CDP rather
than Chrome's desktop `--headless` mode. Use `chrome://inspect/#devices` in desktop
Chrome for interactive inspection, or reuse the device runner's CDP helpers and
the forwarded `/json/list` endpoint. Device GPU/startup results are reported by
the harness; desktop headless results do not establish Android performance.

Remove only the forwarding rules added for this session when finished:

```bash
adb -s "$CAPY_ANDROID_SERIAL" forward --remove tcp:9228
adb -s "$CAPY_ANDROID_SERIAL" reverse --remove tcp:4173
```
