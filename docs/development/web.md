# Web development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The browser client uses DOM controls around Rust compiled to WebAssembly. WebGPU
runs the same canvas renderer as the native clients. The finished app is static
files and can be packaged as an installable, offline PWA
([Web packaging](web-packaging.md)).

Canvas presentation uses a persistent GPU target with the shared renderer's damage
tracking. After rendering, a GPU texture copy publishes that target to the browser's
transient canvas texture. Reconfiguration invalidates presentation history; a change
in size or format replaces the retained target. This avoids repainting the whole
viewport for every pen update without depending on browser swapchain retention.
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

Where to look in [`apps/layer-web`](../../apps/layer-web): `start.js` loads
`app.js` only when the browser has the platform features the editor UI needs;
otherwise `gpu.js` shows the update help in the canvas area. `app.js` schedules
frames on animation callbacks, routes input and applies shared UI changes to the
DOM; `gpu.js` writes the help shown when WebGPU cannot start;
`documents.js` connects project requests to browser file access and downloads;
`workspace-worker.js` and `workspace-store.js` keep the workspace in IndexedDB.
Rust owns editor behavior; JavaScript owns browser events, controls and services.
Runtime canvas failures show the browser's message and expandable, copyable
failure details. The report captures the adapter, browser, canvas and transform
state, tracked renderer storage, main Wasm heap size and the last 16 GPU events
before retiring the renderer. Rust panic events retain the cause and source
location even when the browser exception only says `unreachable`.
Worker events identify their owner and Wasm heap;
device loss and out-of-memory errors retire that worker and fail its requests
immediately. Validation errors remain diagnostic events. A failure while
suspending the renderer is reported alongside the original error. Tracked storage
is not total GPU or browser memory, and a terminated browser process cannot
produce this in-page report.
Diagnostics and raster transport invoke owned function snapshots, allowing a
callback to reconfigure its successor synchronously. The `--callback-boundaries`
journey checks both paths in both themes, including saved package output and
cancellable preview requests whose cancellation callbacks remain valid until
the worker promise settles.
WebGPU pipelines use asynchronous preparation on every browser. A recipe shares
one preparation result among its callers; command encoding cannot start a
blocking compilation or replace a pending compilation. Snapshot jobs prepare
their required kernels before encoding tiles, and share immutable transform
recipes across jobs on the same device. Canvas edits wait in the shared ordered
input queue while their dependencies prepare.
Input and page-lifecycle hooks also run while the Wasm app is being created.
They retain held contacts but call compiler timing methods only after the app is
published. The frame suite checks the actual hooks before and after publication,
including input before GPU attachment and the retained resume timer.

The session runs on the browser event loop without shared memory. File decoding,
encoding and color work run in `raster-worker.js` through the shared
`package::transfer` descriptor and bounded transferable payloads. The descriptor
uses the final manifest adapters plus private runtime identity and verification
receipts; Web has no second artwork schema. Main-thread adoption consumes verified
owners without image decoding or decoded-content hashing. Cold object tiles use a
dedicated worker to decompress native samples or convert ICC samples into working
pixels. Requests carry one tile and bounded profile chunks; completion wakes the
canvas instead of polling while the worker is busy. Nearest placement uses the
same preparation worker for bounded F64 coordinate batches; the receiving owner
validates and uploads packed coordinates without performing geometry loops on
input. Package writes stream
to private OPFS output jobs and return a Blob for picker/download publication;
completion closes that job. Editor actions resume when the shared request finishes;
browser cleanup retains its ownership guards until checkpoint and output cleanup
finish. Unsupported files keep their original Blob and show shared
package status, available outputs and an optional preview with Copy Original and
Export Preview Image. Presentation uses the shared decoder's RGBA pixels;
preview export retains the verified original PNG and refuses the original file handle. The
manual-save worker retains its output job and storage lock until publication
finishes and the host closes the job.

The host reads a selected document's four-byte prefix and asks shared Rust to
admit its size against the package or photo input budget. It passes the original
Blob to a file worker, which reads slices of at most 8 MiB through FileReaderSync; opening, image placement and paper import never
materialize the entire encoded input on the page. Native packages use that
backing directly instead of spooling a second copy. The worker owns the Blob
registration until decoding and result transfer finish, and rejects stale or
cross-thread reads. Unsupported files retain the original Blob for Copy Original.
This relies on the browser's [immutable Blob contract](https://www.w3.org/TR/FileAPI/);
it does not verify the browser implementation or impose an aggregate memory quota.


Private document candidates drive their initial frame to the shared parking
boundary before restoring session history or adopting the drawing.

Clipboard output carries the full-depth source transfer and a separate PNG for
other applications. The receiver separates the declared PNG trailer before
validating and adopting the source transfer.

Restart snapshots use the shared session codec, preserving every open drawing,
tab order, selection, camera, undo/redo and manual-save checkpoint. Browser events
schedule shared checkpoint intervals; unchanged stamps skip preparation and writes.
The retained checkpoint worker reuses immutable payloads, and prepares session
metadata and validates disk bytes away from the input thread. Live session transfer
uses shared history records and bounded resource chunks without replaying history
on the browser thread. Before packing a checkpoint, the browser polls compressed
backing for every live, undo and redo tile without blocking its event loop; published
tile descriptors alone do not mean disk bytes are ready.
Export profile payloads use the same immutable resource inventory and worker delta
cache; stamps and transfer metadata retain references instead of inline ICC bytes.

`restart-store.js` publishes immutable resources and current/previous drawing heads
in strict IndexedDB transactions. Exact checkpoint metadata is protected by SHA-256
and checked before decoding. A separate authoritative window manifest records
membership before a drawing's first publication. Closing a drawing removes its
membership and stored resources together. Automatic restore holds Web Locks,
admits the candidate before adopting it, then transfers the same drawing key between
window manifests in one transaction. A failed transaction preserves the old owner;
a failed restore retains its copy and offers Retry, Later or Discard. Corrupt current
data may use the previous complete drawing, with a recovered title until saving.
The next checkpoint retains that verified drawing as its previous generation.
Lost acknowledgements retry with newer drawing generations or identical manifest
receipts, including an already committed ownership transfer.

Recovery dialogs include the actual restore failure. Retry closes the dialog and
rechecks canvas readiness before another attempt, so GPU loss cannot trap the
Restart canvas action behind repeated recovery dialogs. GPU notices retain the
original failure even if renderer suspension also fails. Restart repeats renderer
cleanup before creating a replacement; a cleanup failure remains visible and
does not attempt a second attachment. Initial device failures offer the same
restart action; successful attachment clears the old notice.

Ordinary restart opens drawings without a dialog. Browsers may clear site storage
under storage pressure, and Safari after seven days without a visit unless the app
is installed. The first checkpoint of a changed drawing that was never saved to a
file therefore asks the browser to keep the site's storage
(`navigator.storage.persist()`), once per page. An up-to-date checkpoint permits
quiet browser exit. Pending state, or such a drawing while the browser has not
agreed to keep storage, requests the browser's native exit warning.
Saved originals are read through retained handles without requesting permission.
A missing, changed or inaccessible original protects the restored drawing until
the painter saves or explicitly discards it. A verified original stays clean.
Visibility and page-exit events request final writes, but browsers may stop async
work during exit. Original and Export Again handles are retained when supported. Save rechecks permission
and the shared destination fingerprint; unavailable or externally changed files
require choosing a destination rather than overwriting unrelated changes.

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
waveforms only when their data or display settings change. Scope captions and
choice labels invalidate their intrinsic measurement copy when they change;
plot publications and control values retain that copy. Scope graphs use the
available panel space while their controls keep their measured height. Their
raster follows the actual graph bounds; intrinsic copies keep the natural plot
height. Log counts has its own row above the status, with a label target at the panel control
height. Secondary status, axis and layer metadata use one color derived from the published text
at 55% opacity.
Toolbar and canvas bar fitting round widget widths up to whole pixels, so text never truncates and the tiles after it stay on the pixel grid.
Toolbar number captions and value widths and Add Filter also round up, as GTK labels do, through
CSS `calc-size()`; browsers without it keep fractional widths. `histogram.js` retains
the native canvases; Properties updates their embedded plots without replacing
focused numeric fields. Calibration, Auto Levels and targeted Curves edits use
shared actions and the existing canvas pointer path.

## Tests

Tool bubbles use the shared Rust presentations and hit testing. The host retains
the wheel, swatches, layer thumbnails and the Layers inspector used by drawers.
The folded Layers canvas retains pixels across themes for the same document and
layer; changing either replaces the canvas before requesting its preview.
Bubble motion updates glass regions without measuring stationary toolbars again.
Each bubble requests its own compositor layer so moving it reuses its rendered
contents. Keep the hint on the bubble container; promoting every bead adds
unnecessary layers.
Run `bash tools/performance/workspace-motion.sh web --tool-bubbles` for the
light/dark mouse, touch and pen journeys; `device.test.mjs --tool-bubbles` runs
the same contacts in tablet Chrome. See the [host contract](../ui/tool-bubbles.md#host-contract)
for presentation and input requirements.

`workspace-motion.sh web --hdr-bubbles` checks the HDR bubble's equal-margin
outline in both themes at all three sizes, with the paint pair, recent colors
and palette enabled. Mouse, touch and pen contacts edit intensity, reset it and
restore it on cancellation; Quick Mask removes the HDR arc. The journey also
checks pickup from an open drawer and records moving-frame callback diagnostics.
Those browser callback measurements do not qualify presentation rates.

Pure unit tests need no browser or GPU:

```bash
node --test apps/layer-web/{run,package,frame,bubbles,glass,pointer,workspace-client,canvas-bar,notice,zoom-readout,export-controls,size-dialog,text-input,localization}.test.mjs
node --test apps/layer-web/{numeric,histogram,gpu-diagnostics,raster-worker-client,pipeline-wait,workspace-manager-copy,toolbar-components-copy,color-controls-copy,document-color-copy}.test.mjs
```

The pointer harness runs the app's contact and release registrations in order,
retaining every listener for each event type. It checks independent contacts,
deferred header release and pointer IDs reused before a release callback runs.

Viewport checks keep the selected input device active across resizes; touch
emulation changes only when selecting a device case or restoring test state.
Wall-clamp contacts use the current shared work area and reject return-home
targets before dragging.
Bubble source-journey failures capture the viewport, hit target and native input
trace before restoring the workspace; enable `LAYER_BUBBLE_INPUT_TRACE=1` to
retain that trace.

The launcher fixture uses Bash; `LAYER_TEST_BASH` selects its executable
(default `/bin/bash`). On Windows, set it in Git Bash before those commands:

```bash
export LAYER_TEST_BASH='C:/Program Files/Git/bin/bash.exe'
```

`workspace-motion.sh web --numeric-controls` checks panel slider and editor
geometry, complete readouts and unit alignment, large pixel scrubbing, typed
fractions and cancellation in both themes. Mouse and touch use the private
Wayland display; pen and keyboard use trusted browser input. It verifies the
actual glyph fonts against an independent inherited UI-font sample, retaining the
Adwaita Sans preference and system fallback. It records three sustained value
scrubs per theme as browser callback diagnostics.

`test.mjs --gpu-failure-lifecycle` injects an out-of-memory event on the actual
canvas device and checks expanded details, copying and restarting in both themes.
A late event from the retired device must leave the replacement canvas usable.
This checks error handling; it does not reproduce memory exhaustion.

`test.mjs --wheel-navigation` checks wheel navigation with held middle/right
mouse buttons, Ctrl/Shift precedence, cursor anchoring, release, view lock,
active-paint exclusion and browser zoom ownership in both themes. The pointer
unit suite also covers ancestor-targeted wheel events and pixel/line/page units.
Run the journey on the private display with
`bash tools/performance/workspace-motion.sh web --wheel-navigation --native-input`.

The storage regression launches its own temporary Chrome profile and local server:

```bash
node apps/layer-web/restart-store.test.mjs
```

It covers aborted resource publication, immutable-ID conflicts, metadata corruption, current/previous
retention, interrupted restore, stale writes, atomic ownership transfer and closed
membership. `--drawing-tabs-recovery` walks automatic restart in both themes with
clean and dirty drawings, tab order, cameras, undo/redo, save checkpoints and close.
It checks Selection Layer overlay pixels immediately after color, opacity and
native eye edits and after restart, while the portable artwork remains unchanged.
It also checks the storage request and exit warning with Chrome's persistent-storage
permission denied and then granted. `test.mjs` grants that permission, as Chrome
does for installed apps, so other journeys reload without an exit warning.
`--session-restart-performance` records three six-second pen gestures alongside
background checkpoints, logical resource/metadata bytes, snapshot age
at publication and browser callback/input costs under `artifacts/seamless-restart/web`.
It excludes still callbacks from motion statistics and checks unchanged state
does not write again. These desktop diagnostics do not qualify a tablet tier;
use the reference hardware, canvas and completed-update rules in
[performance measuring](../performance/measuring.md).

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
selects offscreen Vulkan and disables accelerated Canvas2D widget drawing to
avoid transparent widget surfaces in that configuration. WebGPU painting and
composition still require a hardware adapter. `LAYER_WEB_CANVAS_2D=accelerated`
selects the accelerated widget path; other hosts keep their native GPU backend. Without
`--headless` Chrome opens on the current Wayland display. `LAYER_TEST_VERBOSE=1`
prints browser diagnostics, `LAYER_TEST_THEME=light` or `dark` selects the initial
theme, and journeys that save captures write them to `LAYER_TEST_ARTIFACTS`.
`--hdr`, `--proof` and `--shared-workflows` read owned fixtures from
`apps/layer-web/pkg/` that `build.sh` does not create: `hdr-pq.png`,
`proof-cmyk.icc`, `proof-p3.icc` and `prophoto16.png`.

Worker tracing uses `prepareWorkers` in `tools/cdp.mjs`. Register each observer
before starting attachment or reloading the page. The helper installs observers
at the worker's first script boundary and releases its startup pause once.

Run `node --test tools/cdp.test.mjs` with Chrome available (`CHROME` may name
its executable). It checks module-worker preparation and CDP teardown using a
private profile and loopback fixture, without the app, Wasm or WebGPU requests.

On macOS, enable Safari's Develop > Allow Remote Automation once, start
`safaridriver -p 4444`, and run `node apps/layer-web/safari-test.mjs` against the
local build. `SAFARIDRIVER_URL` selects an already running driver and
`LAYER_WEB_URL` selects the app. The test creates and closes its own isolated
automation session. It reuses the contact-brush journey with trusted mouse input,
checking startup, visible GPU strokes, Undo/Redo and uncaptured GPU errors in
both themes. The default brush IDs are `1,2,3,5`; `LAYER_BRUSH_PRESETS` overrides
them. This catches Safari compiler and upload failures that native Metal and
Chrome tests can miss. It does not verify pen pressure or qualify performance.

On Linux, the same journeys also run headed inside a private Mutter compositor,
which builds the Wasm module first and serves it on port 4179:

```bash
bash tools/performance/workspace-motion.sh web --title-bar
```

`--workspace-motion`, `--workspace-resize`, `--numeric-controls` and `--color-panel` use real
Mutter mouse and touch input; the other journeys use Chrome DevTools input. See
the [Linux guide](linux.md#tests) for the runner's requirements.
Native browser fixtures map content coordinates through `native-input-fixture.mjs`
before compositor injection, including the browser's screen position and chrome.

Journeys by area; the table in `test.mjs` lists them all. `journeys.mjs` runs the
first matching row and its error check, or leaves the default journey to the host.
Every selected journey requires an error check and awaits its result before
reporting success. Fixtures that inject diagnostics check their specific allowance.
`--stale-storage` awaits drawing recovery and durable checkpoints before
reloading its corrupted workspace and preference fixtures.

| Area | Selectors |
| --- | --- |
| Editor smoke, drawing, pen | `--editor`, `--pen`, `--prediction`, `--raster`, `--color-mixing`, `--touch-scrub` |
| Enclose and Fill | `--enclose-fill` |
| Vector layers | `--vector-tools`, `--vector-milestone`, `--vector-edges`, `--vector-files` |
| Canvas bar, notices, footer zoom | `--canvas-bar`, `--notices`, `--zoom-controls`, `--zoom-readout`, `--zoom-tool-settings`, `--move-selection` |
| Retouching | `--clone`, `--heal` |
| Color | `--color-panel`, `--color-wheel-resize`, `--color-picker`, `--palettes`, `--scopes-smoke`, `--scopes`, `--tonal-controls` |
| Layers and filters | `--paper-texture-readiness`, `--tip-resize-readiness`, `--paper-rows`, `--paper-library`, `--layers`, `--layer-relationships`, `--blend-menu`, `--pass-through`, `--blending`, `--adjustments`, `--curves`, `--gradients`, `--pointwise-effects`, `--filter-drawer`, `--filter-previews`, `--spatial-filter-windows`, `--photo-edit`, `--merges`, `--retouch-layers` |
| Canvas size, crop and image commands | `--canvas-size`, `--crop`, `--image-commands` |
| New drawing | `--new-drawing` walks the dialog: sizes, presets, paper, Texture visibility, paper color, the Import… tile and Create. Other journeys create drawings with `createDrawing` from `new-drawing-fixture.mjs`, on flat paper unless they pass `relief`. |
| Photo files, packages and export | `--portable-photo`, `--package-view`, `--image-object-fixture`, `--export-metadata`, `--document-errors` |
| Title bar | `--title-bar`, `--title-bar-state`, `--title-bar-feedback`, `--title-bar-overflow`, `--menu-labels`, `--compact-workspaces`, `--header-controls` |
| Tool bubbles | `--tool-bubbles` |
| Bubble menus, thumbnails and Zen keep/peek | `--bubble-release`, `--bubble-release-cold` |
| Retained configuration and translated dialog layout | `--bubble-release-localization` |
| Docking and drags | `--drag-pickup`, `--layout-drops`, `--column-stacks`, `--column-drops`, `--columns`, `--workspace-rendering`, `--workspace-constraints`, `--drawer-drag`, `--drawer-style` |
| Workspaces | `--workspace-manager`, `--workspace-manager-visual`, `--toolbar-manager`, `--workspace-grips`, `--workspace-held-name`, `--workspace-switcher`, `--workspace-options`, `--workspace-options-refresh`, `--workspace-focus`, `--workspace-windows`, `--workspace-store` |
| Settings and retained copy | `--preferences`, `--settings-audit`, `--pressure-calibration`, `--tilt-calibration`, `--language-switching`, `--live-language-color`, `--live-language-proof`, `--live-language-delivery`, `--live-language-surfaces`, `--live-language-toolbar`, `--live-language-effects` |

`--touch-scrub` runs in both browser runners. It clears the active painted layer
with horizontal, vertical and diagonal three-finger scrubs in both themes,
preserves the camera and layer underneath, and checks exact presented pixels
through Undo/Redo. Run the desktop journey with
`bash tools/performance/workspace-motion.sh web --touch-scrub`.

`--workspace-constraints` also runs in `device.test.mjs`. It starts each Sketch,
Paint and Photo theme journey from its starting layout, resizes through seven
CSS viewports from 640 × 360 to 1440 × 1000, and uses Navigator, Histogram,
floating controls and the last palette swatch and Add button. It checks that
automatic fitting leaves the saved layout alone and an intentional change
survives reload. `constraints.json` records the tablet's initial viewport and
DPR alongside each effective CSS viewport; emulation does not establish physical
orientation or browser zoom coverage. The journey uses a temporary workspace and
leaves the original workspace and built-in histories intact.
`LAYER_CONSTRAINT_AXES=1` selects a focused 1257 × 691 journey with German labels,
DPR 1, 1.25 and 1.5, inactive tab returns and keyboard drawer opening and closing,
in both themes. This does not qualify browser zoom or OS text scaling; the
registered catalogs contain no RTL locale.

`--header-controls` checks menu focus through compact and overflow layouts.
Expansion restores the first visible, enabled control in the same zone, skipping
passive items.
Battery indicators retain their authored width inside padded status tiles;
title-bar scaling changes their rendered size without flex shrinkage.

`--tool-variations` checks Photo's 16 and Paint's 18 tool buttons, compact variation
menus and secondary menus, retained icons and sibling choices in active-tool
drawers, Paint's command categories and remembered media icons, Sketch's header
groups, separate manual and automatic selection menus and Tool Set rows,
and mouse/touch/pen hold-to-reorder with one layout undo/redo in both themes.

`--tool-bubbles` runs in both browser runners with mouse, touch and pen in both
themes. It checks title-bar and brush-row tear-out, native list scrolling,
selection/halo/chooser taps, arc adjustment/reset/cancellation, presented dabs
and bounded readouts, persistent option menus, wheel picks and drag-out,
two-finger movement and pinching, Layers cards and Properties, Undo/Redo,
Zen, return home, layout history and viewport walls. Each case starts with an
empty bubble desk and restores the saved workspace afterward. Touch emulation
is configured for each input type and the original touch capability is restored
afterward.
`LAYER_BUBBLE_THEMES` and `LAYER_BUBBLE_DEVICES` accept comma-separated focused
cases; `LAYER_BUBBLE_INPUT_TRACE=1` records native event cancellation on failure.

Drawer sizing uses each column's final allocated width and its children's natural
height. Width changes schedule another measurement even with reduced motion;
the prior scroll viewport height must not prevent a drawer from shrinking.
`--drawer-switch` checks complete Palette footers on every toolbar edge with
ordinary and reduced motion. `--drawer-style` checks that toolbar and column-group
dividers draw the shared `divider_metrics` line, centred in its slot.

The Layers list runs edge to edge in the panel, drawers and the Layers bubble,
with 6-pixel side padding on rows, the header, the footer and the properties
disclosure, so their first icons line up with the eye. `--layer-properties-drawer`
and `--tool-bubbles` check those slots.

`--enclose-fill` checks real pointer contacts in both themes against transparent
reference ink. Two fully enclosed holes fill a separate paint layer; a hole
crossed by the lasso stays empty. Shared export previews check actual GPU pixels,
and saved raster identities check untouched references, one-step Undo/Redo and
Escape cancellation.

`--selection-tools` checks mode-row alignment inside the active drawer and
Photo command availability through the slots' published variant menus.

`--smoothing` checks the ungrouped Smoothing control and its 30-at-75% slider
mapping in both themes, then draws pen contacts at strengths 0, 6, 50 and 100 with stroke
preview on and off. It requires visible committed pixels and exact Undo/Redo.
The same journey runs through `device.test.mjs` on a reserved tablet.

`--language-switching` visits all shipped languages in light and dark themes,
checks retained Preferences and dirty size-entry identity, focus and selection,
rapid choices, browser language resolution and another same-profile tab.
Concurrent tab edits await each tab's model and published language matching the
current saved preference before capturing the final choice.
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

`--panel-controls` captures Tool Settings, Properties, Histogram, Waveform, Proof and Palettes
in light and dark themes at 242- and 320-pixel panel widths. It checks 8-pixel
content edges, shared dropdown columns, 4-pixel peer gaps on both axes, controls
and radii at the shared panel control metrics, 36-pixel slider rows, editor focus
and squircle radii, and GTK's 16-pixel checkbox box, corner and caption gap. Panel captions start
at the row top and end above the slider hit area. Properties checks the 4-pixel
gap after its heading and the 6-pixel gap before the parameter group.

Panel dropdowns are native selects drawn like GTK's: the value in bold and the
shared chevron on the input fill. Property, Tool Settings and picker rows keep
their caption beside the control at every panel width, wrapping it to two lines.
Radio segments, the Tool panel's selection menu, the Layers blend button and Add
Filter use GTK's button fill (`--control-fill`). Horizontal Tool Options dropdowns
take their natural width and refit when the chosen item changes.

Panel and toolbar checkboxes (Log counts, Tool Settings checks,
Center on click, Proof options) stay native inputs for focus, keyboard and
accessibility but draw like GTK's: a 16-pixel box with a 4-pixel corner, a 2-pixel
`--divider-line` outline unchecked, the accent fill and check mark checked, and
4 pixels before the caption in the 26-pixel panel control row. Icon-only Tool
Options actions centre their icon in the tile in horizontal and vertical toolbars.

`--pointwise-effects` checks Hue range pages and Colorize value retention,
Threshold and Photo Filter controls, Invert/Desaturate insertion, slider history,
visible source pixels, keyboard focus and saved source identity at narrow and
wide widths in both themes. `node --test apps/layer-web/color-button-lifecycle.test.mjs`
checks that a removed field cannot publish a deferred color-dialog result.
`--pointwise-effects-smoke` reuses the journey at 1100 pixels wide in both themes,
checking Colorize and Threshold compositor pixels, controls, focus and archive
reopening without slider motion.

Before pixel assertions or screenshots, await `brush_ready()` for required canvas
pipelines, then frame and submitted-queue completion. `wait_for_canvas()` waits
only for submitted work; an idle queue can precede required pipeline readiness.
Exact comparisons of presented pixels also await `!canvas_work_pending()`. At 50%
zoom and below the canvas shows reduced composition until idle refinement
presents the exact result, which can differ by one 8-bit step at a few pixels.

`--layer-relationships` exercises contextual attachment controls, clipping rails,
effect links, hidden owners, group modes and normalized saved-selection drops.
It checks narrow and wide panels in both themes; `--layers` includes this journey.
Use separate `--layer-hold` and `--layer-swipes` invocations for the related input
paths. Layer grips retain their layout while shared effective locks disable
pickup; parent locks use the same published row state. Drop-surface availability
remains independent of source pickup. The holding journey checks locked body
and grip contacts, unlock restoration and exact Undo/Redo in both themes with
mouse, pen and touch.
`--layer-properties-drawer` checks the combined Layers/Properties body in both
themes, including native layer creation, sizing, overflow, scroll restoration,
swipe actions, filter summaries and a custom toolbar opener.

`--gradients` exercises the shared stop editor in Properties, Tool Settings and
the toolbar popup, including mouse, pen and touch contacts, numeric edits,
cancellation, undo and selected-stop retention. It checks two-dimensional
integer preview dithering, unchanged floating previews, and gradient definitions
through archive reopening and renderer recreation in both themes and widths.
The same journey checks real pen cancellation for figures, gradients and lasso:
`pointercancel` restores the unchanged drawing, while ordinary release commits
one undoable edit. Native cancellation stays distinct from the browser's
post-release capture-loss event.

`--image-object-fixture` opens the fixed builtin, ICC and Nearest-only shared-image
packages in `apps/layer-web/fixtures/`. Their sources are generated sample
patterns: a 512 × 512 RGBA8 gradient and the ProPhoto RGBA16 pattern used by the
Web color journeys, with its embedded profile. They contain no external photos.
The packages were authored with the `layer-color` `object_fixture` example and
fixed F64 placements. The journey checks cold worker preparation before input,
object-only GPU pixels, 64× source minification, F64 placement beyond F32
precision, immutable image identity, visibility history, package reopening and
renderer replacement in both themes. It also exercises comparison, output and
clipboard captures, and restores
pending redo through a real IndexedDB restart.
`LAYER_OBJECT_FIXTURE_URLS` can select equivalent fixtures as a JSON URL array.

`--package-view` exercises preserved packages with and without a verified preview
in light and dark themes. It checks output names, preview pixels, exact original
and PNG export bytes, original-file refusal, cancelled pickers, failed writes and
retry. Cancelled or stale preparation must preserve the incumbent drawing, dirty
state, tabs and undo history. The journey builds its packages from the current
shared writer; it needs no external archive fixture.

Setup that some journeys need:

- `--shared-workflows` needs its owned embedded-profile sample at
  `/pkg/prophoto16.png`: ProPhoto RGBA16, 513 × 257 pixels, with 31 fully
  transparent black columns and opaque color data elsewhere. Its round-trip
  assertions preserve alpha and channel layout as well as color/depth.
- `--tab-styles` checks light/dark docked and drawer tabs for visible hover,
  selection before release, retained tab elements and the regular pointer. Its
  four-tab contour matrix compares both lower corner regions for every active
  and inactive pairing, including reverse feet, straight ends and scrolled clips.
  It also checks the circular fallback when squircle corners are unavailable.
- `--tab-surfaces` checks absolute colors along selected-tab faces and feet at
  scales 1 and 2 in both themes. It covers opaque and translucent fills with equal
  and unequal role opacity, round and squircle contours, docked and drawer tabs,
  insertion after detached construction, fractional scroll offsets and dragged
  previews over a contrasting backdrop.
- `--workspace-rendering` checks Navigator GPU pixels at 1x and 2x, including
  live pen strokes, brush-up convergence and artwork undo/redo in both themes.
  It compares active tabs with their panel bodies at every transparency level.
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
- `--paper-texture-readiness` holds a genuine new Paper texture pipeline while
  a complete Pencil contact arrives, then checks intact pixels and exact Undo/Redo
  in both themes. Use a fresh private test origin and let its initial recovery
  setup and autosave finish before the runner reloads it. Every pen point must
  reach the canvas without a recovery dialog covering it.
- `--paper-rows` runs the same Add paper layout, input, history and fill-mask
  checks as the desktop journey, in both themes, and walks Choose Texture from
  the Paper row and Properties: its size against Edit Color, Cancel, Use Texture
  with one Undo, a locked Paper and Paper's disabled mask controls.
- `--paper-library` imports an image with Import… in Choose Texture, then
  renames it in the tile menu's Rename prompt, relists it, deletes it, adds the
  drawing's own paper back with Add to Library, and checks that library edits
  leave the drawing alone.
  The raster worker keeps the library in IndexedDB and decodes, converts and
  encodes papers there.
- Check the journey table in `device.test.mjs` before choosing a flag. With
  `CAPY_ANDROID_SERIAL` set, `--canvas-bar`, `--zoom-readout`,
  `--image-placement` and `--paper-rows` also send real OS taps through `adb`.
- `--staged-startup` holds shader validation and checks that controls, paper and
  painting become usable in order, including Space-drag right after Preferences
  closes and returns focus to its clicked button; `--filter-previews` checks
  that built-in rows show their bundled images without preview requests. `--paper-rows` checks that Space presses
  a keyboard-focused button.
- `--pipeline-readiness` holds compilation during Fill followed by Deselect,
  checks pixels and undo history, then applies a cold transform and commits
  another by switching tools. It instruments both page and worker devices before
  startup and requires zero blocking pipeline creation calls.
  Run `node --test apps/layer-web/pipeline-wait.test.mjs` to check that releasing
  held compilation also releases pipeline promises that finish later.
- Use `chrome://inspect/#devices` in desktop Chrome for interactive inspection.
  Desktop headless results do not establish tablet performance.

`--binary-transfer` checks bounded worker buffers and exact archive bytes for a
9504 × 6336 saved selection and a full-size layer mask, plus a 2 MiB ICC profile
shared by proof and retained original samples. Generate its fixture from a valid
RGB profile so opening also passes shared color validation; use the
[binary payload commands](../internals/binary-payloads.md#reproducible-checks).
The generator adds a private data tag and writes `binary-transfer-fixture.capy.icc`
beside the package. Serve both files, set
`LAYER_BINARY_FIXTURE_URL=/pkg/binary-transfer-fixture.capy`, and run the tablet
runner with `--binary-transfer`. It compares every ICC byte with the companion
file and checks shared resource identities without replacing the open drawing.

For selected-brush preparation on the low tier, run `web-pen.mjs --reload
--navigator --startup-pending` through the reserved tablet runner.
Set `LAYER_PEN_PHOTO` to a served 4248 × 2832 tier photo,
`LAYER_PEN_PRESET` to the preset ID, `LAYER_PEN_BRUSH_SIZE=1024`, and
`LAYER_PEN_IDLE_MS=0` or `30000` for immediate or idle preparation.
The report retains selected-brush readiness separately from startup completion,
and the normal event-to-submission latency and frame interval distributions.
Add `--inflight-warmup` to wait for a newly admitted asynchronous pipeline before
starting motion; the report records its label and start/end times beside input.
The normal primer is undone before motion; `--unprimed` skips it.
With `--reload`, set `LAYER_WASM_SHA256` to the frozen package's hash to verify
the loaded Wasm bytes before motion. Reload bypasses the HTTP cache.
Frame calls measure CPU submissions, not fresh GPU updates or presented frames.

## Troubleshooting

- **Blank or black canvas in headless Chrome.** Some NVIDIA drivers lose Chrome's
  headless Dawn instance (`A valid external Instance reference no longer
  exists.`), and headless Chrome that composites in software omits WebGPU pixels
  from screenshots; `test.mjs` prints a warning when it does. Journeys that
  compare presented canvas pixels, such as `--contact-brushes`, `--editor`,
  `--proof`, `--selection-tools` and `--tonal-selection`, then fail headless.
  Run them headed through `workspace-motion.sh web`, which has hardware
  presentation.
- **Original Size placement check fails.** The headed `--image-placement`
  journey also fails on the untouched `5c76e202f` product: after Original Size
  and Apply, the saved scale is `0.625` instead of `1`. Keep its unit-scale
  assertion when comparing changes. The baseline requires the worker Bounds
  spy to inspect the final snapshot task, rather than array index `1`.
- **Startup never completes.** On failure the runner saves
  `artifacts/ui/web-failure.png` and prints page errors and GPU diagnostics.
  Check `#gpu-notice`, and in the page evaluate `layerApp.state()`,
  `JSON.parse(layerApp.app.workspace_view())` or `layerApp.startupTimes`.
- **Vector journeys.** Reuse `vector-journey-fixture.mjs` for input, capture and
  readiness waits. Keep each journey's line sample count, key event type and pixel
  reducer; Tools averages a 5 × 5 patch, while Milestone takes its darkest pixel
  and Edges uses a rounded 9 × 9 patch. Readiness waits allow 60 seconds.
- **Writing a journey.** Follow
  [workspace-manager.test.mjs](../../apps/layer-web/workspace-manager.test.mjs):
  use the runner's `call`, `evaluate` and `settle` helpers, wait for operations
  to finish, and check the rendered controls as well as stored state. `settle`
  waits for animation frames; deferred edits need their expected state change
  before recording a result. Between file operations, wait for both
  `document_file.busy` and `documents.busy()` to clear so browser cleanup finishes.
- **Comparing captures with GTK.** Chrome can embed a different transfer curve
  even with `--force-color-profile=srgb`; convert each PNG's embedded profile to
  sRGB before comparing pixels. The [visual tools](../../tools/visual/README.md)
  do this.
