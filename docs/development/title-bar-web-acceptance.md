# Web title-bar port acceptance

Reference: [GTK handoff](title-bar-web-handoff.md),
[drag convention](../ui/drag-and-reorder.md).

## Milestone 1: shared projection and input

Web now projects the shared header model and platform defaults, with retained
workspace/status controls, plain tool icons, overflow and footer measurements.
The compact component bank uses whole-item immediate pickup after browser slop.
Rust owns geometry, drag previews, validation, keyboard moves and final edits.
The existing tool picker and drawer renderer are reused. Browser IndexedDB and
lease ownership remain separate from native file locking.

Validation on 2026-09-13:

- Web release Wasm build passed.
- 353 shared UI tests and 86 workspace tests with `native` enabled passed,
  including `browser_leases_still_expire_and_fence_stale_writers`.
- 19 Web launcher/packaging tests passed; the header module is fingerprinted.
- `tools/performance/workspace-motion.sh web --title-bar` passed in real headed
  Chrome under isolated Mutter, using CDP mouse, touch and pen streams. It checks
  inert bank clicks, immediate bank/body pickup, outside bank cancellation,
  adding a component, original grab offset through detachment, re-entry,
  singleton removal/return and editor Cancel.
- Captures and machine-readable results: `artifacts/title-bar/web-headed/`.
  Generated artifacts are intentionally ignored by Git.

Headless Chrome completed those pointer assertions but reported a WebGPU
external-instance error and displayed a black canvas. Headed Chrome rendered
the canvas and completed without browser errors. Use the isolated compositor
for the acceptance run on this machine. CDP streams establish browser pointer
arbitration; physical stylus/tablet hardware and mobile browsers remain untested.

## Milestone 2: complete title-bar journeys

The expanded `--title-bar` and `--title-bar-state` suites passed with hardware
WebGPU in headed Chrome. Their checks include:

- Mouse/touch/pen body and bank pickup, inert clicks, frozen-slot neighbor
  sliding, detach/remove/re-entry, no model publication during motion, and
  Escape, capture loss, blur, source invalidation, resize and pointer cancel.
- Left/Right across regions, Delete/Backspace, Shift+F10 and shared contexts.
  Touch/pen holds show a menu, mouse holds do not, release retains a held menu,
  and dragging with the same contact dismisses it.
- Drop-only Tools, the existing picker, text-input ownership, nested Cancel,
  confirmation and outside tool removal. All 12 available content/brush/size
  families opened through real header buttons. Brush/blend/erase/lasso/transform
  drawers, Color swap, square lower corners, title-space dismissal and grey
  action feedback were checked with mouse/touch/pen where applicable.
- Actual project Save through the browser download flow; Done/Cancel; one-step
  workspace Undo/Redo; completed autosave and reload; reload and workspace
  manager switching during customization; actual second-window close followed
  by a read of its persisted IndexedDB layout/history.
- Dark/light × Small/Medium/Large × physical 1×/2×. PNG dimensions assert the
  backing scale. Editor wrapping and overflow at 744, 480 and 360 px; keyboard
  access to overflow and a generous empty-center drop destination.
- Actual fullscreen, retained status positions, charging/low battery, footer
  ownership and full Zen with edge reveal in both themes.

Browser testing found and fixed a release/resize race, stale focus/popups when
items entered overflow, and a minimum-width error in the small editor. It also
found a shared Rust persistence bug: the workspace picker could capture a title
preview before clearing customization state. Its baseline now uses the committed
header/footer; a new GTK/Web regression covers this. All 354 shared UI tests pass.

Evidence: `artifacts/title-bar/title-bar/` (input, drawers, action feedback and
results) and `artifacts/title-bar/state/` (size/theme/scale matrix, small windows,
fullscreen, Zen, downloaded test project and results). Input is CDP-generated
inside the real browser; physical stylus/tablet and mobile-browser coverage is
still a device limitation, not an inferred pass.

## Milestone 3: existing Web regressions

The following existing scenarios passed in headed Chrome with hardware WebGPU
and no browser errors: `--header-controls`, `--fullscreen`, `--zen`,
`--workspace-manager`, `--workspace-switcher`, `--drag-pickup`, `--editor`,
`--tool-picker`, `--workspace-windows` and `--workspace-store`.

These retain coverage of all eight menus and overflow focus, API/browser-owned
fullscreen and maximize, absent/denied battery APIs, ordinary toolbar holds,
canvas pixels behind empty header space, drawing controls, actual save/open/new
and PNG export, workspace rows and cross-tab refresh, independent autosaves,
stale-owner protection, interrupted transactions and recovery. The menu/status
and switcher assertions now reflect the shared title-bar projection: fullscreen
owns status visibility, and a compact Window menu replaces a pill that cannot fit.

Final shared checks: 354 UI tests, 86 workspace tests with `native`, and 19 Web
launcher/packaging tests passed. The release Wasm build passed. Before running
`--workspace-store`, regenerate its SQLite/browser contract fixture from current
Rust so the fixture uses the current saved-layout and Color-state schema:

```bash
CAPY_STORE_CONTRACT_FIXTURE=/tmp/capy-workspace-store-contract.json \
  cargo test --locked -p layer-workspace --features native \
  browser_transactions_match_sqlite_contract
```

Run each browser scenario with the isolated compositor command documented in
[Web development](web.md). Regression captures/results are under
`artifacts/title-bar/<scenario>/`; `artifacts/title-bar/regression-results.json`
records the completed suite output. The pointer/device limitations above also
apply to this final validation.

## Tablet review corrections

The user's tablet review identified three omissions in the first port:

- Web Sketch now uses the same minimal shared arrangement as GTK, without the
  two legacy docked toolbars. The shared upgrade recognizes untouched shipped
  layouts on GTK/Web, preserves working values and stable IDs, and leaves edited
  histories, custom baselines and independent copies alone.
- Selected tools retain GTK's 22% blue through hover and press. Open drawers
  use a neutral 10% highlight; action presses use neutral 16% feedback. Earlier
  tests checked `aria-pressed` without establishing the actual selected colour.
  The new `--title-bar-feedback` scenario checks the computed colours and captures
  real contact states in both themes.
- The workspace pill retains its rounded 34 px track and 26 px choices, centered
  vertically for Small/Medium/Large instead of stretching with the icon tiles.

Real-device checks also exposed Chrome's double-tap page zoom on repeated header
taps and its lack of held `:active` feedback for touch. Header controls now use
`touch-action: manipulation`; the editor keeps its immediate-drag `none` rule.
The host tracks the button contact only for visual feedback, clearing it on
release, cancellation, capture loss, blur, resize or source invalidation. Native
clicks, existing context holds and shared Rust tool actions retain ownership.

Validation on 2026-09-13: release Wasm build, 354 UI tests, 86 workspace tests,
headed Chrome `--title-bar`, `--title-bar-state`, `--title-bar-feedback`,
`--header-controls` and `--zen` passed. `--title-bar-feedback` also passed on the
USB-connected Wacom MovinkPad 14 (DTHA140), Chrome 152 in desktop-site mode,
Qualcomm Adreno 7xx WebGPU, at its actual fractional display scale. It exercises
mouse/touch/pen contacts, repeated taps, all sizes, both themes and release
cleanup on the tablet. These contacts are CDP-generated; the user still owns
manual stylus/touch review.

Evidence: `artifacts/title-bar/review/`, including `tablet/` for device captures.
The tablet review uses a separate local origin, leaving the earlier workspace
and document intact.
