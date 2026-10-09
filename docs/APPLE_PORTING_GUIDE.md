# Apple porting guide

[Technical documentation](README.md)

How to bring the macOS (AppKit) and iPadOS (UIKit) clients up to date with
features that land first on GTK, Web and Android. Both targets share the Swift
editor in `apps/layer-apple/Shared` and the Rust bridge in
`apps/layer-apple/native`, so every port applies to both. Build and test
commands are in [macOS and iPadOS development](development/apple.md).

## Standing rules

- **Both platforms at every milestone.** Build, test and review macOS and iPadOS
  together. A result on one target never stands in for the other.
- **Shared behavior stays in shared Rust.** Business rules, validation, state
  transitions, layout and history live in the shared crates. Swift presents
  shared state; the AppKit and UIKit adapters handle input, windowing, lifecycle
  and system services. `native/` holds only Apple integration, never a second
  engine or a home for general settings or layout rules.
- **Fix shared defects in shared Rust with a headless test.** Apple's defaults
  (2048 × 1536 drawings, Metal, Retina surfaces) can expose shared bugs that the
  reference hosts' journeys miss. Reproduce them in a `layer-host` or
  `layer-render-wgpu` test and fix them there, not with a Swift workaround.
- **Keep native code simple.** Use established SwiftUI, UIKit and AppKit
  patterns. Editor menus use `EditorActionMenu` and `editorPopover`, with one
  `EditorPopoverHost` at the editor or sheet root outside clipped panels. Do not
  add gesture overrides, UIKit menu or drag-session handoffs, per-menu
  presenters, test-only product behavior or speculative input and rendering
  workarounds. Replace superseded Apple code in the same change.
- **Parity is perceptual.** Fix every perceptible difference from Web and
  Android. Imperceptible rasterization differences do not justify extra code.
  Never hide or remove a feature to claim parity.
- **Ergonomics.** Fast menus, readable opaque theme colors, minimal animation
  and no glass shine. Keep native control, keyboard and accessibility
  conventions.

## Parity targets

- **Workspace and canvas behavior** (toolbars, panels, docking, drawers, canvas
  overlays, selection and mask presentation, loupe and previews, corner shapes,
  gestures) must match the reference hosts in layout, sizing and behavior.
  Compare against the Web app using [the visual tools](../tools/visual/README.md)
  with matching state, viewport, scale and color space.
- **Menus, dialogs and settings** may use native AppKit, UIKit or SwiftUI
  styling, provided their layout, sizing and behavior match the shared design.
- Input follows the [drag and reorder convention](ui/drag-and-reorder.md) and
  distinguishes mouse, touch and Pencil on each platform.
- **Intentional platform differences.** macOS top-level menus live in the OS
  menu bar; the editor header keeps its title, Zen and other controls over the
  live canvas, and Zen clears the native window controls. The iPad keeps in-app
  menus built from the same catalog. iPad full screen uses the native iPadOS
  window control, so the shared Fullscreen command stays unavailable there.

## Scope of a port

1. **Audit** every commit on `origin/main` since the last recorded port (see
   [Branches and commits](#branches-and-commits)). Group the commits into
   user-facing features and shared infrastructure, trace each feature through
   shared Rust and the reference hosts, and record what Apple is missing. Keep
   that inventory in ignored `artifacts/`; it is a working document.
2. **Port** every inventoried feature to both macOS and iPadOS until behavior
   matches GTK, Web and Android.
3. **Keep up with main.** Fetch `origin/main` regularly while porting, and audit
   and port newly landed commits as part of the same effort, so the Apple
   clients reach parity with the current main rather than the audit's start.
4. **Do not regress performance.** Hold the
   [performance targets](PERFORMANCE_TARGETS.md) and measure affected brush,
   frame and UI paths as described in
   [Apple performance](../apps/layer-apple/PERFORMANCE.md).

## How to port

- Follow the [commit guide](COMMIT_GUIDE.md). Prefer the most recent reference
  port (usually Android and Web) to see which host glue a feature needs, and
  reuse the same shared APIs.
- Validate on both targets: Rust bridge tests, the Swift fixtures under
  `apps/layer-apple/tests`, targeted XCTest journeys on macOS and a physical
  iPad, and hardware timing where performance may change.
- Keep raw logs, screenshots and the audit inventory in ignored `artifacts/`.

## Checklist

- **Shared gates.** Features are often enabled per platform in shared Rust, for
  example `CommandId::available_on`, `HeaderItem::available_on`, the capability
  methods on `Platform` in `crates/layer-ui/src/settings.rs` and the platform
  checks in `layout_presets.rs`. Enable `Platform::Mac` or `Platform::Ios` only
  together with the Swift presentation, and extend the shared tests that loop
  over `Platform::ALL`.
- **Included workspace defaults.** Enabling a gate can change the Apple Sketch,
  Paint or Photo defaults. Before changing shared code, serialize
  `WorkspacePreset::layout(platform)` for every preset and platform, then make
  sure other platforms' defaults are unchanged.
- **Separate publications.** `layer-host` can publish small packets beside the
  full state and geometry updates (for example `{command_search, revision}`).
  Handle each in `EditorSnapshotState.receive` without promoting the content
  revision, so retained panels and thumbnails do not refresh.
- **Navigation.** Present the published navigation cursor and shared tool
  double-activation action. Native timing and gesture phases stay in the host;
  view recall, command availability and customized shortcuts stay in Rust.
  Complete drawing-cycle requests before switching so they cannot replay when
  a parked drawing is restored. Group overlapping native recognizers around
  the first accepted event, including a beginning blocked by a pen contact.
- **Queries that decide visibility.** SwiftUI never runs `.task` or `.onAppear`
  on an empty `Group`. Attach a query that decides whether content appears (such
  as `canvas_bar_layout`) to a container that is always present.
- **Layer relationships.** Present shared connection endpoints over measured
  row and content-thumbnail frames, clipped to the native scroll viewport.
  Read current `relationship` and `text` palette roles for rails and FX links.
  Forward the supplied attachment and right-swipe actions; preserve local
  visibility when displaying inherited hiding. Preview and release both query
  `layer_drop` with epoch, raw target, fraction and hit surface. Guard replies
  against cancellation and document changes, and commit the raw release hit
  through shared `Drop` rather than reconstructing policy from the preview.
- **Object layers.** Each imported image is an ordinary layer row. Use the
  shared layer actions, context menu and thumbnail target for selection,
  visibility, masks and reordering. There are no child image rows or separate
  object actions. The `object` flag identifies preserved source content.
- **Settings sub-pages.** macOS sheets have no title bar, so navigation titles
  and `.navigation` toolbar items inside Settings never appear. Put a sub-page's
  title and Back button in its content, as the shortcut page does.
- **UIKit fields in SwiftUI.** iPadOS consumes Escape before SwiftUI key
  handlers; use the UIKit text field with priority key commands. Give it an
  explicit height and assign fonts and placeholders only when they change, or
  size invalidation can loop.
- **Coverage audit.** Keep the expected unavailable commands and native handler
  references in `apps/layer-apple/command-coverage.json` current. Run the audit in the
  [Apple guide](development/apple.md#command-coverage-audit).

## Writing XCTest journeys

Shared journeys are `check…` functions under `apps/layer-apple/Shared/Tests`,
called from a `test…` method in `Shared/Tests/EditorLaunchTests.swift`, which
both UI test targets compile. Platform-only journeys extend `EditorLaunchTests`
in `iOS/Tests` or `macOS/Tests`.

- **Test editor effects, not system menus.** Never coordinate-test the Mac
  system menu bar; macOS menu mechanics are trusted platform behavior. Use
  `CAPY_INITIAL_ACTIONS`, in-app controls or the editor's own menus, and check
  the resulting state, pixels and history.
- **XCTest cannot synthesize Pencil.** Finger drags on the iPad canvas navigate
  unless the session claims the finger (a transform handle, inside a transform
  box, a Warp node); only Pencil draws. Create selections and artwork through
  menus in iPad journeys. Simulator and injected input never replace a physical
  Pencil check.
- **Escape never reaches an iPad app from XCTest.** Assert Escape on macOS and
  use visible controls on iPad.
- **The software keyboard covers the lower part of the window.** Keep taps above
  it, scroll fields into view before typing, and replace text without assuming a
  triple tap selects it. Tap a picker's native button, not its static label.
- **Frames differ between platforms.** iOS reports a SwiftUI accessibility
  container's frame as the union of its children, so do not compare iPad widths
  against values measured on macOS.
- **Do not depend on window positions or labels that shared changes move.** Place
  canvas strokes and samples relative to `editorPaper(in:)`, select items by
  label rather than index, reveal scrolled controls before activating them, and
  wait for published state instead of reading it right after a click. When a
  journey fails, first check whether the reference hosts changed the rule; fix
  Apple only when it disagrees with them.
- **Mac window size carries between journeys.** macOS saves window frames per
  build, even with `-ApplePersistenceIgnoreState`, so a zoomed or resized window
  carries into the next test, and a new build opens at its default size. For a
  wider work area, enter full screen and leave it in a teardown block; a journey
  that resizes the window restores its size the same way.
- **Editor menus are modal to accessibility.** An open `EditorMenuButton` menu
  hides its button from queries. Close it by pressing the button's recorded
  frame, and reveal rows in long menus before activating them.
- **Exact history.** Displayed Undo/Redo pixels must match exactly; the shared
  renderer redraws rounded pages at pen-up. Compare against a stroke only after
  it stops changing, and do not add tolerances for display differences.
- **One UI run at a time.** Swift fixtures that open windows steal focus from a
  running XCTest batch; run them before or after, not alongside. Run only one
  benchmark or UI run per device.
- **Evidence before retries.** After an inconclusive UI failure, get new
  evidence and a specific hypothesis before another run or an input workaround.
  Do not build new automation infrastructure for each control; prefer the Swift
  fixtures and Rust tests for anything they can check.
- **Installed-app runs.** To test apps already installed on the iPad, use
  `UseDestinationArtifacts` in the `.xctestrun`, as described in the
  [Apple guide](development/apple.md#ui-tests).

## Acceptance

- **Name what a result proves.** Shared correctness checks, mounted Swift
  fixtures, full-app journeys and physical input each prove different things.
  Reuse passing evidence unless changed code or a reproduced failure invalidates
  it.
- **Performance.** Apple silicon iPads and Macs exceed the top tier, so its
  [targets](PERFORMANCE_TARGETS.md) apply. The Mac's display presents at 90 Hz;
  evaluate Mac presentation at 90 Hz until a 120 Hz display is available.
  Earlier rejected scheduling experiments are listed in
  [Apple performance](../apps/layer-apple/PERFORMANCE.md#rejected-scheduling-changes).
- **Hardware that is not available.** There is no physical iPad hardware
  keyboard, second Mac display, 120 Hz Mac display or working iCloud Drive
  account. Leave those checks unverified; do not request the hardware, grant OS
  permissions for them or block a milestone on them.

## Branches and commits

Work lands on `origin/main` as described in
[Sharing `main`](COMMIT_GUIDE.md#sharing-main); there are no long-lived port
branches. Commit at each significant milestone (a complete feature area,
building and validated on both targets), rebase onto the latest `origin/main`
before pushing, and port anything new that the rebase brings in.

- **Record the port watermark.** The last commit of a port states the
  `origin/main` revision it covers, as a body line `Ported origin/main through
  <sha>`. The next audit starts from the most recent such line
  (`git log --grep='Ported origin/main through'`).
- If `origin/main` has been rewritten, compare the rewritten commits' trees with
  your local ones and move uncommitted work onto the new tip instead of merging
  the old history back in.
- Keep drawings, private traces, signing material, team IDs and device or account
  identifiers out of commits.
