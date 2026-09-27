# Apple porting guide

How to bring the macOS (AppKit) and iPadOS (UIKit) clients up to date with
features that landed first on GTK, Web and Android. Both Apple targets share
the Swift editor in `apps/layer-apple/Shared` and the Rust bridge in
`apps/layer-apple/native`, so every port applies to both.

## Scope

1. **Audit** every commit on `origin/main` since the last Apple port. Group the
   commits into user-facing features and shared infrastructure, trace each
   feature through shared Rust and the reference hosts, and record what Apple
   is missing. Keep that inventory in ignored `artifacts/` (not checked in);
   it is a working document, not durable documentation.
2. **Port** every inventoried feature to both macOS and iPadOS until behavior
   matches GTK, Web and Android.
3. **Keep up with main.** Fetch `origin/main` regularly while porting. Audit and
   port newly landed commits as part of the same effort, and add them to the
   inventory, so the Apple clients reach parity with the current main rather
   than the revision the audit started from.
4. **Do not regress performance.** Measure affected brush, frame and UI paths
   against the previous Apple baseline (see
   [apps/layer-apple/PERFORMANCE.md](../apps/layer-apple/PERFORMANCE.md)).

## Parity targets

- **Workspace and canvas behavior** (toolbars, panels, docking, drawers, canvas
  overlays, selection and mask presentation, loupe and previews, corner shapes,
  gestures) must match the reference hosts in layout, sizing and behavior.
  Compare against the Web app when visual confirmation is needed, using
  [the visual tools](../tools/visual/README.md) with matching state, viewport,
  scale and color space.
- **Menus, dialogs and settings** may use native AppKit/UIKit/SwiftUI styling,
  provided their layout, sizing and behavior match the shared design.
- Input follows the [drag and reorder convention](ui/drag-and-reorder.md) and
  distinguishes mouse, touch and Pencil on each platform.

## How to port

- Follow the [commit guide](COMMIT_GUIDE.md): business rules, validation, state
  transitions and history stay in shared Rust; Swift presents shared state and
  forwards native input. Replace superseded Apple code instead of layering new
  paths beside it.
- Prefer the most recent reference port (usually Android and Web) to see which
  host glue a feature needs, and reuse the same shared APIs.
- Validate on both targets: Rust bridge tests, the Swift fixtures under
  `apps/layer-apple/tests`, targeted Xcode UI tests on macOS and a physical
  iPad, and hardware timing where performance may change. See
  [macOS and iPadOS development](development/apple.md).
- Keep raw logs, screenshots and the audit inventory in ignored `artifacts/`.

## Checklist

- **Shared gates.** New features are often enabled per platform in shared Rust
  (for example `CommandId::available_on`, `Platform::color_picker`,
  `ToolbarControl::components_available` and the platform lists in
  `layout_presets.rs`). Add `Platform::Mac | Platform::Ios` only together with
  the Swift presentation, and extend the shared tests that loop over platforms.
- **Included workspace defaults.** Enabling a gate can change the Apple Sketch,
  Paint or Photo defaults. Before changing shared code, serialize
  `WorkspacePreset::layout(platform)` for every preset and platform, then make
  sure other platforms' defaults are unchanged.
- **Superseded Apple paths.** When main consolidates a subsystem that Apple
  previously duplicated (for example shared demand shader preparation replacing
  the bundled filter package reload), delete the Apple copy in the same port.
- **Separate publications.** `layer-host` can publish small packets beside the
  full state and geometry updates (for example `{command_search, revision}`).
  Handle each in `EditorSnapshotState.receive` without promoting the content
  revision, so retained panels and thumbnails do not refresh.
- **Coverage audit.** Keep `apps/layer-apple/command-coverage.json` in sync and
  run `cargo run --locked -p layer-host --example inventory -- --gpu` followed by
  `apps/layer-apple/scripts/audit-commands.py`.
- **Tests.** `cargo test --locked -p layer-apple --target aarch64-apple-darwin
  --lib -- --test-threads=1` exercises both Apple policies through the C ABI and
  Metal. Add shared UI journeys under `apps/layer-apple/Shared/Tests` and register
  them in both `EditorLaunchTests`. Physical iPad XCUITests need **Settings →
  Developer → Enable UI Automation** and an unlocked device.
- **Shared defects.** Apple's defaults (2048 × 1536 drawings, Metal, Retina
  surfaces) can expose shared bugs that the reference hosts' journeys miss.
  Reproduce them headlessly in a `layer-host` or `layer-render-wgpu` test and fix
  them in shared Rust with that regression test, not with a Swift workaround.
- **Stale journeys.** Shared layout and tool changes (fitted columns, renamed
  tools, merged tool sets) break journeys that assume window positions or
  labels. Place canvas strokes and samples relative to `editorPaper(in:)`,
  select items by label rather than index, reveal scrolled controls before
  activating them, and wait for published state instead of reading it right
  after a click. When a journey fails, first check whether the reference hosts
  changed the rule; fix Apple only when it disagrees with them.
- **iPad automation limits.** Finger drags on the iPad canvas navigate unless
  the session claims the finger, as on a transform handle, inside a transform
  box or on a Warp node; only Pencil draws, and XCTest cannot synthesize Pencil. Create selections and
  artwork through menus in iPad journeys. XCTest's Escape never reaches an iPad
  app, so assert Escape on macOS and use visible controls on iPad. iOS reports a
  SwiftUI accessibility container's frame as the union of its children, so do not
  compare iPad widths against values measured on macOS. The software keyboard
  covers the lower part of the window; keep taps above it.
- **Queries that decide visibility.** SwiftUI never runs `.task` or `.onAppear`
  on an empty `Group`. Attach a query that decides whether content appears (such
  as `canvas_bar_layout`) to a container that is always present.
- **Window size in Mac journeys.** macOS saves window frames per build, even
  with `-ApplePersistenceIgnoreState`, so a zoomed window carries into the next
  test, and a new build opens at its default size. Do not depend on the width a
  journey starts at. For a wider work area, enter full screen and leave it in a
  teardown block; a journey that resizes the window restores its size the same
  way, since every later journey inherits the saved `editor-AppWindow-1` frame.
- **Settings sub-pages.** macOS sheets have no title bar, so navigation titles
  and `.navigation` toolbar items inside Settings never appear. Put a sub-page's
  title and Back button in its content, as the shortcut page does.
- **Editor menus.** An open `EditorMenuButton` menu is modal to accessibility,
  so its button cannot be queried while it is open. Close it by pressing the
  button's recorded frame, and reveal rows in long menus before activating them.
- **UIKit fields in SwiftUI.** iPadOS consumes Escape before SwiftUI key
  handlers; use the UIKit text field with priority key commands. Give it an
  explicit height and assign fonts and placeholders only when they change, or
  size invalidation can loop.
- **Exact history.** Displayed Undo/Redo pixels must match exactly; the shared
  renderer redraws rounded pages at pen-up. Compare against a stroke only after
  it stops changing, and do not add tolerances for display differences.
- **One UI run at a time.** Swift fixtures that open windows steal focus from a
  running XCUITest batch; run them before or after, not alongside.
- **Performance.** Record `CAPY_WORKLOAD` runs (`ink`, `layered-4k`) on both
  devices before and after, and summarize them with
  `tools/performance/apple_trace.py`.

## Commits

Commit and push to `origin/main` at each significant milestone (a complete
feature area, building and validated on both targets), not for every small
change. Rebase on the latest `origin/main` before pushing and port anything new
that the rebase brings in. Install the repository's commit and push guards with
`sh tools/git/install-hooks.sh` and never add `Co-Authored-By` trailers naming an
AI assistant or agent (see [Commit and push checks](COMMIT_GUIDE.md)). If
`origin/main` has been rewritten, compare the rewritten commits' trees with your
local ones and move uncommitted work onto the new tip instead of merging the old
history back in.
