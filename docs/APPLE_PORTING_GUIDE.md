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
- **Included workspace migration.** Enabling a gate can change the Apple Sketch,
  Paint or Photo defaults. Before changing shared code, serialize
  `WorkspacePreset::layout(platform)` for every preset and platform, then make
  sure each previously shipped Apple default remains an accepted baseline in
  `layer-workspace/src/manager_migration.rs` and that other platforms' defaults
  are unchanged.
- **Coverage audit.** Keep `apps/layer-apple/command-coverage.json` in sync and
  run `cargo run --locked -p layer-host --example inventory -- --gpu` followed by
  `apps/layer-apple/scripts/audit-commands.py`.
- **Tests.** `cargo test --locked -p layer-apple --target aarch64-apple-darwin
  --lib -- --test-threads=1` exercises both Apple policies through the C ABI and
  Metal. Add shared UI journeys under `apps/layer-apple/Shared/Tests` and register
  them in both `EditorLaunchTests`. Physical iPad XCUITests need **Settings →
  Developer → Enable UI Automation** and an unlocked device.
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
