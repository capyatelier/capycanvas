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
The session runs on the browser event loop without shared memory. File decoding,
encoding and color work run in `raster-worker.js` through the shared
`package::transfer` descriptor and bounded transferable payloads. The descriptor
uses the final manifest adapters plus private runtime identity and verification
receipts; Web has no second artwork schema. Main-thread adoption consumes verified
owners without image decoding or decoded-content hashing. Package writes stream
to private OPFS output jobs and return a Blob for picker/download publication;
completion closes that job. Recovery uses the same writer and separate shared
publication tokens. Unsupported files keep their original Blob and show shared
package status, available outputs and an optional preview with Copy Original and
Export Preview Image. Preview export refuses the original file handle. The
manual-save worker retains its output job and storage lock until publication
finishes and the host closes the job.

Language changes use the running session's browser language tags and shared
`LanguageTransition`. Preparation runs in short event-loop tasks and publication
waits for pointer contacts and composition keys to retire. The publication updates
retained catalog objects and semantic DOM bindings together; numeric entries,
selection, dialogs, the canvas and WebGPU owners keep their identities. Same-origin
tabs receive preference choices through the existing storage event and reconcile
again when visible. Browser `languagechange` events refresh a System choice.

Retained numeric, color and file errors carry shared reasons. Publication formats
those reasons and cached document metadata without parsing drafts, inspecting
profiles or restarting prepared previews. Tool, toolbar and workspace control
identity excludes localized captions. Proof panels read scalar copy during refresh;
full profiles cross the boundary only when their proof generation changes or the
panel opens.

Color, retained-source and output previews share `output::preview_value` for
their extent and sRGB pixel array.

Histogram and Waveform are workspace panels, including the Photo workspace's
scope tabs. `src/scopes.rs` publishes shared normalized plots and bounded RGBA
waveforms only when their data or display settings change. `histogram.js` retains
the native canvases; Properties updates their embedded plots without replacing
focused numeric fields. Calibration, Auto Levels and targeted Curves edits use
shared actions and the existing canvas pointer path.

## Tests

Pure unit tests need no browser or GPU:

```bash
node --test apps/layer-web/{run,package,frame,pointer,workspace-client,canvas-bar,notice,zoom-readout,export-controls,size-dialog,text-input,localization}.test.mjs
node --test apps/layer-web/{numeric,histogram,raster-worker-client,workspace-manager-copy,toolbar-components-copy,color-controls-copy,document-color-copy}.test.mjs
```

The localization tests cover retained semantic copy, optional fields, binding
replacement and composite labels that contain native inputs. The copy suites check
retained controls, drafts, focus, options and prepared results, plus semantic errors
without repeating parsing, storage or rendering work.
The text-input tests cover composition key ownership through native key release,
including a keydown delivered after composition ends and engines that consume the
release. Real IME checks must also distinguish candidate confirmation from the
next ordinary Enter or Escape, and replace selected text in names and numbers.

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

Journeys by area; the table in `test.mjs` lists them all. `journeys.mjs` runs the
first matching row and its error check, or leaves the default journey to the host.

| Area | Selectors |
| --- | --- |
| Editor smoke, drawing, pen | `--editor`, `--pen`, `--prediction`, `--raster`, `--color-mixing` |
| Canvas bar, notices, footer zoom | `--canvas-bar`, `--notices`, `--zoom-controls`, `--zoom-readout`, `--move-selection` |
| Retouching | `--clone`, `--heal` |
| Color | `--color-panel`, `--color-picker`, `--palettes`, `--scopes-smoke`, `--scopes`, `--tonal-controls` |
| Layers and filters | `--layers`, `--blend-menu`, `--pass-through`, `--blending`, `--adjustments`, `--curves`, `--pointwise-effects`, `--filter-drawer`, `--filter-previews`, `--spatial-filter-windows`, `--photo-edit`, `--merges`, `--retouch-layers` |
| Canvas size, crop and image commands | `--canvas-size`, `--crop`, `--image-commands` |
| Photo files, packages and export | `--portable-photo`, `--package-view`, `--export-metadata`, `--document-errors` |
| Title bar | `--title-bar`, `--title-bar-state`, `--title-bar-feedback`, `--title-bar-overflow`, `--menu-labels`, `--compact-workspaces`, `--header-controls` |
| Docking and drags | `--drag-pickup`, `--layout-drops`, `--column-stacks`, `--column-drops`, `--columns`, `--workspace-rendering`, `--drawer-drag`, `--drawer-style` |
| Workspaces | `--workspace-manager`, `--workspace-switcher`, `--workspace-options`, `--workspace-options-refresh`, `--workspace-focus`, `--workspace-windows`, `--workspace-store` |
| Settings and retained copy | `--preferences`, `--settings-audit`, `--language-switching`, `--live-language-color`, `--live-language-proof`, `--live-language-delivery`, `--live-language-surfaces`, `--live-language-toolbar`, `--live-language-effects` |

`--tool-variations` checks Photo's 15 and Paint's 17 tool buttons, compact variation
menus and secondary menus, retained icons and sibling choices in active-tool
drawers, Paint's command categories and remembered media icons, Sketch's header
groups, separate manual and automatic selection menus and Tool Set rows,
and mouse/touch/pen hold-to-reorder with one layout undo/redo in both themes.

`--language-switching` visits all shipped languages in light and dark themes,
checks retained Preferences and dirty size-entry identity, focus and selection,
rapid choices, browser language resolution and another same-profile tab.
The `--live-language-*` journeys compare current catalog copy in both themes
while retaining native controls, draft text, focus, selected options and prepared
results. They count expression submissions and worker preparation to reject
locale-only reparsing or repeated work. Delivery needs an owned embedded-profile
PNG served as `/pkg/prophoto16.png`. For the proof journey,
`LAYER_TEST_NAMELESS_PROOF=1` and `LAYER_NAMELESS_PROFILE_FILE` select an owned
nameless ICC fixture and check that its localized fallback never changes stored
profile or recipe names. The proof journey also routes an actual premature-apply
refusal through the retained panel and checks current copy without another worker
preparation. German and French narrow checks use a CSS text override;
this does not establish operating-system text scaling or genuine IME composition.
Test genuine composition separately with private IBus engines and native
compositor keys. Chromium on XWayland with `GTK_IM_MODULE=ibus` can connect to
that private IBus session; a headless Mutter Wayland-only session may deliver
ordinary keys without an input-method context. Verify trusted preedit events
and engine traffic before qualifying deferred publication, Enter or Escape.
Native browser zoom changes `devicePixelRatio`; record it and the CSS viewport
separately from the app text-size variable.

`--document-errors` opens a corrupt owned fixture, then calls another actual
Wasm method to verify that rejection released the borrow and preserved drawings.
It also checks that raw JavaScript Error and DOMException messages and strings stay literal,
including after a language change; other objects use deterministic JSON.

`--pointwise-effects` checks Hue range pages and Colorize value retention,
Threshold and Photo Filter controls, Invert/Desaturate insertion, slider history,
visible source pixels, keyboard focus and saved source identity at narrow and
wide widths in both themes. `node --test apps/layer-web/color-button-lifecycle.test.mjs`
checks that a removed field cannot publish a deferred color-dialog result.
`--pointwise-effects-smoke` reuses the journey at 1100 pixels wide in both themes,
checking Colorize and Threshold compositor pixels, controls, focus and archive
reopening without slider motion.

`--package-view` exercises preserved packages with and without a verified preview
in light and dark themes. It checks output names, preview pixels, exact original
and PNG export bytes, original-file refusal, cancelled pickers, failed writes and
retry. Cancelled or stale preparation must preserve the incumbent drawing, dirty
state, tabs and undo history. The journey builds its packages from the current
shared writer; it needs no external archive fixture.

Setup that some journeys need:

- `--workspace-rendering` checks Navigator GPU pixels at 1x and 2x, including
  live pen strokes, brush-up convergence and artwork undo/redo in both themes.
  Its screenshots include the painted document and Navigator.
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
- Check the journey table in `device.test.mjs` before choosing a flag. With
  `CAPY_ANDROID_SERIAL` set, `--canvas-bar`, `--zoom-readout` and
  `--image-placement` also send real OS taps through `adb`.
- `--staged-startup` holds shader validation and checks that controls, paper and
  painting become usable in order; `--filter-previews` checks preview pixels and
  cache lifecycle.
- Use `chrome://inspect/#devices` in desktop Chrome for interactive inspection.
  Desktop headless results do not establish tablet performance.

`--binary-transfer` checks bounded worker buffers and exact archive bytes for a
9504 × 6336 saved selection shared with a layer mask, plus a 2 MiB ICC profile
shared by proof and retained original samples. Generate its fixture from a valid
RGB profile so opening also passes shared color validation; use the
[binary payload commands](../internals/binary-payloads.md#reproducible-checks).
The generator adds a private data tag and writes `binary-transfer-fixture.capy.icc`
beside the package. Serve both files, set
`LAYER_BINARY_FIXTURE_URL=/pkg/binary-transfer-fixture.capy`, and run the tablet
runner with `--binary-transfer`. It compares every ICC byte with the companion
file and checks shared resource identities without replacing the open drawing.

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
