# Web development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The browser client uses DOM controls around Rust compiled to WebAssembly. WebGPU
runs the same canvas renderer as the native clients. The finished app is static
files and can be packaged as an installable, offline PWA
([Web packaging](web-packaging.md)).

Canvas presentation uses a persistent GPU target with the shared renderer's damage
tracking. After rendering, a GPU texture copy publishes that target to the browser's
transient canvas texture, so a pen update doesn't repaint the whole viewport and
nothing depends on browser swapchain retention. Reconfiguration invalidates
presentation history; a change in size or format replaces the retained target.
`GPUCanvasConfiguration.usage` includes render attachment and copy destination,
following the [WebGPU canvas contract](https://gpuweb.github.io/types/interfaces/GPUCanvasConfiguration.html).
No canvas pixels are read or copied through JavaScript.

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

Rust owns editor behavior; JavaScript owns browser events, controls and services.
Where to look in [`apps/layer-web`](../../apps/layer-web):

| File | Role |
| --- | --- |
| `start.js` | Loads `app.js` only when the browser has the platform features the editor UI needs; otherwise `gpu.js` shows the update help in the canvas area. |
| `app.js` | Schedules frames on animation callbacks, routes input and applies shared UI changes to the DOM. |
| `gpu.js`, `gpu-diagnostics.js` | The help shown when WebGPU cannot start, and the runtime canvas failure report. |
| `documents.js` | Connects project requests to browser file access and downloads. |
| `raster-worker.js` | Workers created by `raster-worker-client.js`, one per kind of job: file decoding, encoding and color work, cold object tiles, placement coordinates and panel outline geometry. |
| `panel-vectors.js`, `panel-vector-queue.js` | Publish each panel outline projection with its matching DOM bounds, and coalesce motion. |
| `workspace-store.js`, `workspace-worker.js` | Workspaces in IndexedDB. |
| `restart-store.js` | Restart snapshots in IndexedDB. |
| `src/` | The `layer-web` crate that the JavaScript calls. |

### Changing the host

- **Keep the page thread free.** The session runs on the browser event loop
  without shared memory. Decoding, encoding and color work run in raster workers
  through the shared `package::transfer` descriptor and bounded transferable
  payloads ([binary payloads](../internals/binary-payloads.md)); the page adopts
  verified results without decoding images or hashing decoded content. A file
  worker reads opened files in slices of at most 8 MiB, so opening, image
  placement and paper import never hold the whole encoded file on the page.
- **Prepare pipelines asynchronously.** Every browser creates WebGPU pipelines
  through the asynchronous APIs, and a recipe shares one preparation among its
  callers. Command encoding cannot start a blocking compilation; canvas edits
  wait in the shared ordered input queue while their dependencies prepare, and
  snapshot jobs prepare their kernels before encoding tiles
  ([shader readiness](../internals/shared-shader-readiness.md)).
- **Hooks run before the app exists.** Input and page-lifecycle hooks also run
  while the Wasm app is being created. They retain held contacts but call
  compiler timing methods only after the app is published.
- **Text follows language changes.** Publication updates retained catalog objects
  and semantic DOM bindings together, so numeric entries, selection, dialogs, the
  canvas and WebGPU owners keep their identities. Control identity excludes
  localized captions ([localization](../ui/localization.md)).
- **Tool bubbles.** Bubbles use the shared presentations and hit testing; follow
  the [host contract](../ui/tool-bubbles.md#host-contract). `.tool-bubble` asks
  for its own compositor layer so moving it reuses its rendered contents; keep
  the hint on the bubble container, because promoting every bead adds
  unnecessary layers.

### Browser storage

Restart snapshots use the shared session codec. `restart-store.js` publishes
resources and drawing heads in IndexedDB transactions and holds Web Locks while
restoring; [session recovery](../internals/session-recovery.md) describes the
contract. Browsers may clear site storage under storage pressure, and Safari
after seven days without a visit unless the app is installed. The first
checkpoint of a changed drawing that was never saved to a file therefore asks
the browser to keep the site's storage (`navigator.storage.persist()`), once per
page. Pending state, or such a drawing while the browser has not agreed to keep
storage, requests the browser's native exit warning. Visibility and page-exit
events request final writes, but browsers may stop async work during exit.

## Tests

Pure unit tests need no browser or GPU:

```bash
node --test apps/layer-web/{run,package,frame,bubbles,glass,pointer,color-touch,workspace-client,canvas-bar,notice,zoom-readout,export-controls,size-dialog,text-input,localization}.test.mjs
node --test apps/layer-web/{numeric,histogram,gpu-diagnostics,raster-worker-client,pipeline-wait,panel-vector-paths,panel-vector-queue,editor-panels,workspace-manager-copy,toolbar-components-copy,color-controls-copy,document-color-copy,document-recovery,workflow-stack,color-button-lifecycle}.test.mjs
```

The launcher fixture uses Bash; `LAYER_TEST_BASH` selects its executable
(default `/bin/bash`). On Windows, set it in Git Bash before those commands:

```bash
export LAYER_TEST_BASH='C:/Program Files/Git/bin/bash.exe'
```

Two more checks launch their own temporary Chrome profile; `CHROME` may name
its executable. The first covers the IndexedDB restart store with a local server;
the second covers module-worker preparation and CDP teardown without the app,
Wasm or WebGPU:

```bash
node apps/layer-web/restart-store.test.mjs
node --test tools/cdp.test.mjs
```

### Browser journeys

The other `*.test.mjs` files are Chrome journeys that
[`test.mjs`](../../apps/layer-web/test.mjs) imports; do not run them with
`node --test`. Run one journey per invocation against the development server:

```bash
node apps/layer-web/test.mjs --headless --editor
```

`test.mjs` launches Chrome with a temporary profile over
`--remote-debugging-pipe`, with no TCP debugging port or Playwright. Without
`--headless` Chrome opens on the current Wayland display. Journeys require a
hardware WebGPU adapter and fail on a fallback or software one. On Linux the
runner selects offscreen Vulkan and disables accelerated Canvas2D widget drawing
to avoid transparent widget surfaces in that configuration; other hosts keep
their native GPU backend.

| Variable | Effect |
| --- | --- |
| `CHROME` | Chrome executable, `google-chrome` by default. On macOS, for example, `'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'`. |
| `LAYER_WEB_URL` | The app, `http://127.0.0.1:4173` by default. |
| `LAYER_WEB_CANVAS_2D=accelerated` | Accelerated Canvas2D widget drawing on Linux. |
| `LAYER_TEST_THEME` | Initial theme, `light` or `dark`. |
| `LAYER_TEST_VERBOSE=1` | Prints browser diagnostics. |
| `LAYER_TEST_ARTIFACTS` | Where journeys that save captures write them. |

The `runJourney` table in `test.mjs` maps each flag to its check and error check
and runs the first matching row. A flag that matches no row runs the default
smoke journey instead, so check the spelling against that table. `test.mjs`
grants Chrome's persistent-storage permission, as Chrome does for installed apps,
so journeys reload without an exit warning. The packaged app has its own checks;
see [Web packaging](web-packaging.md#preview-and-test).

On Linux, the same journeys also run headed inside a private Mutter compositor,
which builds the Wasm module first and serves it on port 4179:

```bash
bash tools/performance/workspace-motion.sh web --title-bar
```

`--workspace-motion`, `--workspace-resize`, `--numeric-controls` and
`--color-panel` use real Mutter mouse and touch input; the other journeys use
Chrome DevTools input. `--native-input` gives the browser the full display
instead of the emulated 1440 × 1000 viewport, as in
`bash tools/performance/workspace-motion.sh web --wheel-navigation --native-input`.
See the [Linux guide](linux.md#tests) for the runner's requirements.

Desktop browser journeys, including those that record frame callbacks, don't
qualify a performance tier; use the reference hardware, canvas and
completed-update rules in [performance measuring](../performance/measuring.md).

On macOS, enable Safari's Develop > Allow Remote Automation once, start
`safaridriver -p 4444`, and run `node apps/layer-web/safari-test.mjs` against the
local build. `SAFARIDRIVER_URL` selects an already running driver and
`LAYER_WEB_URL` selects the app. The test creates and closes its own isolated
automation session and runs the contact-brush journey with trusted mouse input
in both themes, with brush IDs `1,2,3,5` unless `LAYER_BRUSH_PRESETS` overrides
them. It catches Safari compiler and upload failures that native Metal and
Chrome tests can miss; it doesn't exercise pen pressure.

### Journeys with extra settings

| Journey | Setup |
| --- | --- |
| `--hdr`, `--proof` | Owned fixtures in `apps/layer-web/pkg/` that `build.sh` does not create: `hdr-pq.png`, `proof-cmyk.icc` and `proof-p3.icc`. |
| `--shared-workflows`, `--live-language-delivery` | An owned embedded-profile sample at `/pkg/prophoto16.png`: ProPhoto RGBA16, 513 × 257 pixels, with 31 fully transparent black columns and opaque color data elsewhere. |
| `--live-language-proof` | `LAYER_TEST_NAMELESS_PROOF=1` and `LAYER_NAMELESS_PROFILE_FILE` select an owned nameless ICC fixture. |
| `--workspace-store` | Write the fixture from current Rust first: `CAPY_STORE_CONTRACT_FIXTURE="$PWD/artifacts/store-contract.json" cargo test --locked -p layer-workspace --features native browser_transactions_match_sqlite_contract`, then run the journey with the same `CAPY_STORE_CONTRACT_FIXTURE`. |
| `--settings-audit` | Compares with GTK allocations. First run `bash tools/performance/workspace-motion.sh gtk --native-test=native_settings_typography --native-storage` from the same worktree; it writes reference JSON and captures for every settings page under `artifacts/ui/settings-audit`, and the Web audit writes its captures beside them. Regenerate them for the current GTK code. |
| `--palettes` | Imports its own exports unless `LAYER_PALETTE_SAMPLES` names a directory with `gpl`, `aco`, `ase`, `swatches` and `kpl` subdirectories of files from other applications. |
| `--image-object-fixture` | Opens the packages in `apps/layer-web/fixtures/`, authored with the `layer-color` `object_fixture` example from generated sample patterns. `LAYER_OBJECT_FIXTURE_URLS` selects equivalent fixtures as a JSON URL array. |
| `--tool-bubbles`, `--hdr-bubbles` | `LAYER_BUBBLE_THEMES` and `LAYER_BUBBLE_DEVICES` take comma-separated focused cases. With `--tool-bubbles`, `LAYER_BUBBLE_INPUT_TRACE=1` records native event cancellation on failure. |
| `--workspace-constraints` | `LAYER_CONSTRAINT_AXES=1` selects the focused 1257 × 691 case with German labels at device pixel ratios 1, 1.25 and 1.5. |
| `--session-restart-performance` | Writes its records under `artifacts/seamless-restart/web` unless `LAYER_TEST_ARTIFACTS` is set. |

### Writing a journey

Follow [`workspace-manager.test.mjs`](../../apps/layer-web/workspace-manager.test.mjs)
and add the flag to the `runJourney` table in `test.mjs`, and in
`device.test.mjs` when it should also run on a tablet.

- **Use the runner's helpers.** Use `call`, `evaluate` and `settle`, and check
  the rendered controls as well as stored state.
- **Wait for the operation you started.** `settle` waits for animation frames;
  deferred edits need their expected state change before recording a result.
  Between file operations, wait for both `document_file.busy` and
  `documents.busy()` to clear so browser cleanup finishes.
- **Wait for exact pixels.** Before pixel assertions or screenshots, await
  `brush_ready()` for required canvas pipelines, then frame and submitted-queue
  completion. `wait_for_canvas()` waits only for submitted work; an idle queue
  can precede required pipeline readiness. Exact comparisons of presented pixels
  also await `!canvas_work_pending()`. At 50% zoom and below the canvas shows
  reduced composition until idle refinement presents the exact result, which can
  differ by one 8-bit step at a few pixels.
- **Create drawings with the fixture.** `createDrawing` from
  `new-drawing-fixture.mjs` creates flat paper unless the journey passes `relief`.
- **Map compositor input.** Fixtures that inject input through Mutter map content
  coordinates through `native-input-fixture.mjs`, including the browser's screen
  position and chrome.
- **Observe workers from the start.** Register each observer with
  `prepareWorkers` from `tools/cdp.mjs` before starting attachment or reloading
  the page; it installs observers at the worker's first script boundary and
  releases its startup pause once.
- **Allow only expected errors.** Every journey has an error check and awaits its
  result before reporting success. A journey that injects diagnostics checks its
  specific allowance.
- **Reuse the vector fixture.** Vector journeys use `vector-journey-fixture.mjs`
  for input, capture and readiness waits, which allow 60 seconds. Keep each
  journey's line sample count, key event type and pixel reducer.
- **Restore what you change.** Use a temporary workspace for layout changes and
  restore the saved workspace, touch emulation and viewport afterwards.
- **Genuine text input.** A CSS text override doesn't establish operating-system
  text scaling, and DevTools keys aren't IME composition. Test composition with
  private IBus engines and native compositor keys: Chromium on XWayland with
  `GTK_IM_MODULE=ibus` can connect to a private IBus session, while a headless
  Mutter Wayland-only session may deliver ordinary keys without an input-method
  context. Check trusted preedit events and engine traffic before relying on the
  result. Browser zoom changes `devicePixelRatio`; record it and the CSS viewport
  separately from the app's text-size variable.

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
- The `runJourney` table in `device.test.mjs` lists the tablet journeys. Without
  a journey flag the runner checks the editor and two-finger zoom and rotation,
  and records startup times in `tablet-startup.json`. With `CAPY_ANDROID_SERIAL`
  set, `--canvas-bar`, `--zoom-readout`, `--image-placement` and `--paper-rows`
  also send real OS taps through `adb`.
- For `--paper-texture-readiness`, use a fresh private test origin and let its
  initial recovery setup and autosave finish before the runner reloads it.
- `--binary-transfer` needs a generated fixture; follow the
  [binary payload checks](../internals/binary-payloads.md#reproducible-checks).
- Use `chrome://inspect/#devices` in desktop Chrome for interactive inspection.

For selected-brush preparation on the low tier, run
`tools/performance/web-pen.mjs --reload --navigator --startup-pending` through
the reserved tablet runner ([measuring](../performance/measuring.md)). Set
`LAYER_PEN_PHOTO` to a served 4248 × 2832 tier photo, `LAYER_PEN_PRESET` to the
preset ID, `LAYER_PEN_BRUSH_SIZE=1024`, and `LAYER_PEN_IDLE_MS=0` or `30000` for
immediate or idle preparation. `--inflight-warmup` waits for a newly admitted
asynchronous pipeline before starting motion, and `--unprimed` skips the primer
that is otherwise undone before motion. With `--reload`, set `LAYER_WASM_SHA256`
to the frozen package's hash to verify the loaded Wasm bytes. Frame calls measure
CPU submissions, not fresh GPU updates or presented frames.

## Troubleshooting

- **Blank or black canvas in headless Chrome.** Some NVIDIA drivers lose Chrome's
  headless Dawn instance (`A valid external Instance reference no longer
  exists.`), and headless Chrome that composites in software omits WebGPU pixels
  from screenshots; `test.mjs` prints a warning when it does. Journeys that
  compare presented canvas pixels, such as `--contact-brushes`, `--editor`,
  `--proof`, `--selection-tools` and `--tonal-selection`, then fail headless.
  Run them headed through `workspace-motion.sh web`, which has hardware
  presentation. With `--offscreen-raster`, journeys whose rows use
  `checkRasterErrors` tolerate the lost Dawn instance; such a run checks exported
  GPU pixels and frame creation, not presentation.
- **Startup never completes.** On failure the runner saves
  `artifacts/ui/web-failure.png` and prints page errors and GPU diagnostics.
  Check `#gpu-notice`, and in the page evaluate `layerApp.state()`,
  `JSON.parse(layerApp.app.workspace_view())` or `layerApp.startupTimes`.
- **Canvas failure report.** Runtime canvas failures show the browser's message
  and expandable, copyable details: the adapter, browser, canvas and transform
  state, tracked renderer storage, main Wasm heap size and the last 16 GPU events
  before the renderer retires. Rust panics keep their cause and source location
  even when the browser exception only says `unreachable`. Worker events name
  their owner and Wasm heap; device loss and out-of-memory retire that worker and
  fail its requests. Tracked storage is not total GPU or browser memory, and a
  terminated browser process cannot produce the report.
- **Comparing captures with GTK.** Chrome can embed a different transfer curve
  even with `--force-color-profile=srgb`; convert each PNG's embedded profile to
  sRGB before comparing pixels. The [visual tools](../../tools/visual/README.md)
  do this.
