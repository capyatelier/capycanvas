# Web development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The browser client uses DOM controls around Rust compiled to WebAssembly. WebGPU
runs the same canvas renderer as the native clients. The finished app is static
files and can be packaged as an installable, offline PWA
([Web packaging](web-packaging.md)).

## Prerequisites

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
```

The CLI version must match `wasm-bindgen` in `Cargo.lock`; `build.sh` prints the
install command when it is missing. The browser must expose hardware WebGPU on
the device under test. Browser tests also need Node.js 22 or newer and Chrome or
Chromium.

## Build and run

```bash
./apps/layer-web/run.sh
```

Open `http://127.0.0.1:4173`. The launcher runs
[`build.sh`](../../apps/layer-web/build.sh), which compiles the Wasm module with
the `dev-perf` profile, generates the browser bindings in `apps/layer-web/pkg/`
and the icon sprite, then serves `apps/layer-web` with Python.
`LAYER_WEB_PORT` changes the port, `LAYER_WASM_BINDGEN` selects the CLI and
`CAPY_RUST_PROFILE` the profile. Rebuild with `bash apps/layer-web/build.sh`
after Rust changes; the browser tests do not build.

Where to look in [`apps/layer-web`](../../apps/layer-web): `app.js` schedules
frames on animation callbacks, routes input and applies shared UI changes to the
DOM; `gpu.js` writes the help shown when WebGPU cannot start;
`documents.js` connects project requests to browser file access and downloads;
`workspace-worker.js` and `workspace-store.js` keep the workspace in IndexedDB.
Rust owns editor behavior; JavaScript owns browser events, controls and services.
The session runs on the browser event loop, without a worker thread or shared
memory.

## Tests

Pure unit tests need no browser or GPU:

```bash
node --test apps/layer-web/{run,package,frame,pointer,workspace-client,canvas-bar,notice,zoom-readout,export-controls,size-dialog}.test.mjs
```

The other `*.test.mjs` files are Chrome journeys that
[`test.mjs`](../../apps/layer-web/test.mjs) imports; do not run them with
`node --test`. Run one journey per invocation against the development server:

```bash
node apps/layer-web/test.mjs --headless --editor
```

`test.mjs` launches Chrome with a temporary profile over
`--remote-debugging-pipe`, with no TCP debugging port or Playwright. `CHROME`
selects the executable (default `google-chrome`); on macOS, for example,
`CHROME='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'`.
`LAYER_WEB_URL` changes the target (default `http://127.0.0.1:4173`). On Linux it
selects offscreen Vulkan; other hosts keep their native GPU backend. Without
`--headless` Chrome opens on the current Wayland display. `LAYER_TEST_VERBOSE=1`
prints browser diagnostics, and journeys that save captures write them to
`LAYER_TEST_ARTIFACTS`.

On Linux, the same journeys also run headed inside a private Mutter compositor,
which builds the Wasm module first and serves it on port 4179:

```bash
bash tools/performance/workspace-motion.sh web --title-bar
```

`--workspace-motion`, `--workspace-resize` and `--color-panel` then use real
Mutter mouse and touch input; the other journeys use Chrome DevTools input. See
the [Linux guide](linux.md#tests) for the runner's requirements.

Journeys by area; the dispatch in `test.mjs` lists them all:

| Area | Selectors |
| --- | --- |
| Editor smoke, drawing, pen | `--editor`, `--pen`, `--prediction`, `--raster`, `--color-mixing` |
| Canvas bar, notices, footer zoom | `--canvas-bar`, `--notices`, `--zoom-readout`, `--move-selection` |
| Retouching | `--clone` |
| Color | `--color-panel`, `--color-picker`, `--palettes` |
| Layers and filters | `--layers`, `--blend-menu`, `--adjustments`, `--filter-drawer`, `--photo-edit`, `--merges` |
| Canvas size, crop and image commands | `--canvas-size`, `--crop`, `--image-commands` |
| Photo files and export | `--portable-photo`, `--export-metadata` |
| Title bar | `--title-bar`, `--title-bar-state`, `--title-bar-feedback`, `--title-bar-overflow`, `--menu-labels`, `--compact-workspaces`, `--header-controls` |
| Docking and drags | `--drag-pickup`, `--layout-drops`, `--column-stacks`, `--column-drops`, `--columns`, `--workspace-rendering`, `--drawer-drag`, `--drawer-style` |
| Workspaces | `--workspace-manager`, `--workspace-switcher`, `--workspace-focus`, `--workspace-windows`, `--workspace-store` |
| Settings | `--preferences`, `--settings-audit` |

Setup that some journeys need:

- `--workspace-store` checks IndexedDB against the native SQLite contract. Write
  the fixture from current Rust first:
  `CAPY_STORE_CONTRACT_FIXTURE="$PWD/artifacts/store-contract.json" cargo test --locked -p layer-workspace --features native browser_transactions_match_sqlite_contract`,
  then run the journey with the same `CAPY_STORE_CONTRACT_FIXTURE`.
- `--settings-audit` compares with GTK allocations. Run
  `bash tools/performance/workspace-motion.sh gtk --native-test=native_settings_typography`
  first; both write to `artifacts/ui/settings-audit`.
- `--palettes` imports its own exports unless `LAYER_PALETTE_SAMPLES` names a
  directory with `gpl`, `aco`, `ase`, `swatches` and `kpl` subdirectories of
  files from other applications.

The packaged app has its own browser checks; see
[Web packaging](web-packaging.md#preview-and-test).

## Tests on an Android tablet

[`device.test.mjs`](../../apps/layer-web/device.test.mjs) runs a subset of the
journeys in Chrome on a tablet over DevTools. Reserve the tablet and forward the
development server and Chrome's DevTools socket as [devices](devices.md)
describes, open the test URL in your own tab, then run:

```bash
LAYER_DEVICE_CDP=http://127.0.0.1:$CAPY_CDP_PORT \
LAYER_WEB_URL=http://127.0.0.1:$CAPY_WEB_PORT/ \
LAYER_TEST_ARTIFACTS=artifacts/web-android \
  node apps/layer-web/device.test.mjs --canvas-bar
```

- The runner attaches to the tab whose URL equals `LAYER_WEB_URL` exactly,
  including the trailing slash, and reloads it. Set both variables; the defaults
  are not the development ports.
- The tab runs in the device's real Chrome profile, and many journeys change
  documents, workspaces or storage. Use your own origin and port, never an
  artist's tab.
- Check the dispatch at the end of `device.test.mjs` before choosing a flag. With
  `CAPY_ANDROID_SERIAL` set, `--canvas-bar`, `--zoom-readout` and
  `--image-placement` also send real OS taps through `adb`.
- `--staged-startup` holds shader validation and checks that controls, paper and
  painting become usable in order; `--filter-previews` checks preview pixels and
  cache lifecycle.
- Use `chrome://inspect/#devices` in desktop Chrome for interactive inspection.
  Desktop headless results do not establish tablet performance.

## Troubleshooting

- **Blank or black canvas in headless Chrome.** Some NVIDIA drivers lose Chrome's
  headless Dawn instance (`A valid external Instance reference no longer
  exists.`), and headless screenshots can omit WebGPU pixels. Run the journey
  headed through `workspace-motion.sh web`, which has hardware presentation.
- **Startup never completes.** On failure the runner saves
  `artifacts/ui/web-failure.png` and prints page errors and GPU diagnostics.
  Check `#gpu-notice`, and in the page evaluate `layerApp.state()`,
  `JSON.parse(layerApp.app.workspace_view())` or `layerApp.startupTimes`.
- **Writing a journey.** Follow
  [workspace-manager.test.mjs](../../apps/layer-web/workspace-manager.test.mjs):
  use the runner's `call`, `evaluate` and `settle` helpers, wait for operations
  to finish, and check the rendered controls as well as stored state.
- **Comparing captures with GTK.** Chrome can embed a different transfer curve
  even with `--force-color-profile=srgb`; convert each PNG's embedded profile to
  sRGB before comparing pixels. The [visual tools](../../tools/visual/README.md)
  do this.
