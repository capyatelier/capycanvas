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

Development uses `dev-perf`: release optimization level 3, incremental compilation,
16 codegen units and line tables for source-level profiling. Set
`CAPY_RUST_PROFILE=release` to compare ordinary release builds. The static packager
explicitly selects `web-release` (ThinLTO); its profile takes precedence over the
environment override. See [Rust build times](rust-build-times.md).

## How the host works

[`app.js`](../../apps/layer-web/app.js) schedules updates through browser animation
callbacks and applies shared UI changes to the DOM. Rust owns editor behavior;
JavaScript owns browser events, controls and services. The shared session runs on
the browser event loop without requiring a worker, shared memory or a Rust server.

Pointer Events supply pressure and other available pen data. Coalesced and predicted
samples depend on browser support. GPU access, file pickers and installation also
vary by browser and OS; a desktop browser test cannot establish mobile behavior.

Active pen contacts use `pointerrawupdate` when delivered, with coalesced samples,
later predictions from `pointermove`, and a per-contact move fallback. The cursor
shares the GPU viewport pass. See the [Huion G Pen investigation](web-pen-huion-2026-09-20.md)
for CPU fixes, 18 px device measurements, the `--pen` regression suite and the
repeatable Web pen benchmark, including Web front-buffer API limits.

GPU startup is staged so controls can appear before the complete shader catalog
is ready. The app reports unavailable GPU access rather than switching painting
to a CPU renderer. The [startup record](../history/web-staged-startup.md) describes
the scheduling work and its measured limits. The later
[tablet startup investigation](../history/web-startup-tablet-2026-09-17.md)
separates workspace input readiness from canvas/brush readiness, records the
startup-library input-lock fix, and records asynchronous pipeline compilation,
storage-completion wakeups and the restored first-canvas staging boundary.

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

For the shared title bar and its inline customization editor, run the two
focused browser suites on Linux with hardware WebGPU:

```bash
LAYER_TEST_ARTIFACTS="$PWD/artifacts/title-bar/input" \
  bash tools/performance/workspace-motion.sh web --title-bar
LAYER_TEST_ARTIFACTS="$PWD/artifacts/title-bar/state" \
  bash tools/performance/workspace-motion.sh web --title-bar-state
```

The first covers real mouse/touch/pen bank and item dragging, cancellation,
keyboard/context menus, tool pickers, drawers and action feedback. The second
covers Save/Done/Cancel, durable workspace transitions, small windows, overflow,
themes, all sizes, true 2× backing scale, fullscreen/status, footer and full Zen.
Use `--title-bar-feedback` for both-theme mouse/touch/pen checks of selected-tool,
open-drawer and action-press colors and the minimal Sketch default. The Android
device runner also accepts this selector; use a dedicated test origin.
Use `--title-bar-overflow` to reproduce the menu-label region collapsing as the
window narrows, then drag its hidden items with mouse, touch and pen at every
bar size. It checks real hit targets above the component bank, stable item IDs,
collapsed drop destinations, cancellation and workspace undo/redo.
Use `--menu-labels` for the fixed-width growth regression: add bank items after
Menu Labels in the left region, verify that the labels compact inside the same
item, and drag its still-visible body/grip and following neighbors. This runs
mouse/touch/pen in both themes at all sizes without removing Capy.
They can also run through `node apps/layer-web/test.mjs` against a running
development server. See the [acceptance record](title-bar-web-acceptance.md) for
captures, shared tests, existing regressions and device limitations.

For the compact Color panel, run `node apps/layer-web/test.mjs --headless --color-panel`
for mouse/pen input, readout and shape buttons, swap, keyboard activation,
cancellation, and both-theme captures at 144/160/200/280/360 px panel widths.
All three shapes and their two readouts (shape units or RGB) fit one square;
the four-tile minimum and short docks retain every control. GTK and Web use the
same Rust allocations, coordinate memory and smooth elliptical Okhsv circle raster.
The disc has extra spacing inside the hue ring, from the shared geometry.
The circle's hue guide rotates 24° counterclockwise and uses Okhsl's smooth C/L
curve with a 5% margin. Normalizing linear RGB preserves hue and softens the ring
without the maximum-saturation boundary jump; actual Okhsv picking is unchanged.
Selecting a shape always restores its units: OKLCH for circle (lightness %,
chroma to three decimals, hue °), HSB for square, HLS for triangle. Tapping the
label toggles between those units and RGB. Shape units are the default;
saved RGB preferences remain RGB. All readouts
use fixed digit cells and stable arc positions. The focused test checks actual
HSB/HLS/OKLCH glyph transforms across 9/10/100 at three widths.
Square and triangle retain their original
color models; switching shapes preserves the selected sRGB paint. Regression cases
include dragging through different black positions and then changing hue.
The CPU raster reuses saturation/transfer curves and logical-pixel samples;
ring, clip and markers retain display resolution. The focused test compares
the interpolated field against a full 2× raster across sizes and hue cusps.
The host retains staging canvases and the static ring, fetching hue stops only
when rebuilding that ring. Measure both raster cost and complete DOM updates
on the target device: a raster below 8.33 ms does not establish 120 Hz presentation.
On Linux, use
`LAYER_TEST_ARTIFACTS="$PWD/artifacts/color-panel/web" bash tools/performance/workspace-motion.sh web --color-panel`
to verify mouse and touch through the private Mutter compositor, plus pen through
CDP. Some Chrome builds deliver CDP touch contacts without compatibility clicks;
the compositor run verifies actual touch activation of the corner buttons.

For the Palettes panel, run `node apps/layer-web/test.mjs --headless --palettes`.
It covers Paint/Photo placement and group fitting, starters, usage history,
history expansion, adding and naming, the chooser, menus from secondary click,
touch/pen holds and Shift+F10, immediate mouse/touch/pen reordering with
cancellation and one-step undo/redo, imports through the file input and worker,
exports through the save picker and download fallback, reload persistence, the
Sketch drawer, a float, both themes and measured automatic tab names. Set
`LAYER_PALETTE_SAMPLES` to a directory with `gpl`, `aco`, `ase`, `swatches` and
`kpl` subdirectories to import files exported by other applications; otherwise
the test imports its own exports. `device.test.mjs --palettes` runs the same
scenario on a tablet; it edits the built-in workspaces of its dedicated origin.

Other focused `node apps/layer-web/test.mjs --headless` modes:

- `--adjustments`: filter catalog, GPU previews, properties and live telemetry.
- `--drawer-switch`: first-click toolbar drawer switching and outside dismissal.
- `--layers`: layer thumbnails, references, row selection, masks and docking.
- `--ui-speed`: incremental Wasm state and Settings updates match full state.
- `--settings-audit`: run `bash tools/performance/workspace-motion.sh gtk
  --native-test=native_settings_typography` first; it compares every Settings
  page with the GTK allocations and font sizes in `artifacts/ui/settings-audit`.

For clipped workspace drags and content-aware release, run
`bash tools/performance/workspace-motion.sh web --workspace-rendering` on Linux,
or `node apps/layer-web/test.mjs --headless --workspace-rendering` against the
running development server. The existing rendering check covers mouse/touch/pen
at 1×/2×, compact Color, short Layers, long Filters, squashed and usable sidebar
heights, footer anchors, established floats, cancellation and undo/redo. It also
checks retained native pixels and placement through model updates. Run
`--workspace-motion` separately for sustained compositor mouse/touch timing.
Its Color-overlap case verifies that moving a panel over the wheel reuses both
the ring and field rasters.

Run `bash tools/performance/workspace-motion.sh web --layout-drops` for menu-bar
prepend targets, panel-body prepend/highlight, and the enlarged tab targets in
first and lower groups. It covers panel/group/toolbar/column sources, mouse,
touch and pen, both sides/themes and 1×/2×, cancellation and one-step history.
Only top-level columns can collapse; new stacks open full columns by default.
Compact-drawer fixtures explicitly enable Open individual panels. Run
`--drawer-drag`, `--drawer-style` and `--column-stacks` for those retained views.

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

For workspace-dialog pixel comparisons, capture the same default workspaces at
1440 × 1000 and scale 1 on both hosts:

```bash
LAYER_MOTION_VIEWPORT=1440x1000 LAYER_TEST_ARTIFACTS="$PWD/artifacts/ui/workspace-modal/gtk" \
  bash tools/performance/workspace-motion.sh gtk --workspace-manager-visual
LAYER_TEST_ARTIFACTS="$PWD/artifacts/ui/workspace-modal/web" \
  bash tools/performance/workspace-motion.sh web --workspace-manager-visual
```

These fixtures use fresh storage and retain full-window PNGs plus widget/DOM
bounds for dark/light themes and real pointer hover states. The Web scenario also
checks GTK's dialog/row allocations, hover targets and keyboard focus. Convert
each PNG's embedded color profile to sRGB before comparing pixels; Chrome can
encode a different transfer curve despite forcing the sRGB color gamut. Compare
the dialog at its recorded bounds without scaling or shifting it; surrounding
editor controls are separate from this comparison.

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

Use `--staged-startup` on that dedicated origin to verify workspace input,
mouse/touch/pen Settings activation and painting while shader validation is held.

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

The [61 MP Filters memory investigation](../history/filter-preview-tablet-memory-2026-09-17.md)
records the shared source-probe texture reuse fix, tablet measurements, and
remaining preview/display memory-budget work.

The [shared filter preview scheduling record](../history/filter-preview-scheduling-2026-09-17.md)
records the Rust lifecycle, 61 MP latency and drawing responsiveness measurements,
platform handoff and remaining memory work.

For preview pixels and lifecycle on an attached Android browser, use the existing
CDP forwarding workflow and an isolated test origin, then run:

```bash
LAYER_DEVICE_CDP=http://127.0.0.1:9230 LAYER_WEB_URL=http://127.0.0.1:8136/ \
  node apps/layer-web/device.test.mjs --filter-previews
```

This covers cached reopening, source/category changes and GPU replacement; it
writes `web-preview-lifecycle.json` and a capture under `artifacts/filter-memory/`.
The check modifies its test document and must not target a user's working tab.
