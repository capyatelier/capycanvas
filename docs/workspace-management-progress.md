# Workspace management implementation

This checklist tracks the full requested GTK → web → Android rollout. It is
not a reduced MVP. The interaction contract lives in `panel-customization.md`
and will be updated with the implementation. No renderer changes are required.

| # | Required behavior | Core | GTK | Web | Android |
| --- | --- | --- | --- | --- | --- |
| 1 | Workspace menu: built-in-panel visibility, separate toolbar visibility section | Verified | Verified | Verified | Pending |
| 2 | Workspace menu: New Toolbar | Verified | Verified | Verified | Pending |
| 3 | Group menu: Add built-in panel submenu, checked membership, move existing panel | Verified | Verified | Verified | Pending |
| 4 | Group menu: Add Toolbar submenu, move existing toolbar | Verified | Verified | Verified | Pending |
| 5 | Configure entries use the actual panel/toolbar name | Verified | Verified | Verified | Pending |
| 6 | Standalone toolbar: Rename includes the current name (revised copy) | Verified | Verified | Verified | Pending |
| 7 | Duplicate toolbar: editable unique suggested name; names unique with built-in panels | Verified | Verified | Verified | Pending |
| 8 | Hide named panel/toolbar; retain configuration | Verified | Verified | Verified | Pending |
| 9 | Delete Toolbar confirmation, distinct from hiding; workspace undo guidance | Verified | Verified | Verified | Pending |
| 10 | Independent workspace undo/redo for every durable workspace edit; shortcuts | Verified | Verified | Verified | Pending |
| 11 | Small 1×1, large 2×2, labeled 3×2 tiles; correct wrapping/resize/drop slots | Verified | Verified | Verified | Pending |
| 12 | Toolbar configuration column contains management/display options; tab context stays concise | Verified | Verified | Verified | Pending |
| 13 | First tool supplies toolbar tab icon | Verified | Verified | Verified | Pending |
| 14 | Narrow vertical ribbons retain a central tab-merge drop zone | Verified | Verified | Verified | Pending |
| 15 | Adding tabs grows group to fit tab names/icons; later manual shrink allowed | Verified | Verified | Verified | Pending |
| 16 | Floating groups/toolbars: live tear-off/move, eight external resize edges, snap zones, Zen visibility, natural sizing and animated reset | Verified | Verified | Verified | Pending |

## Verification gates

- Core tests: transactional edits and validation, independent/coalesced undo,
  name conflicts, hidden/deleted/float restore, all tile geometries and slots,
  narrow ribbon merging, tab growth and explicit shrink, floating bounds/Zen.
- GTK: real widgets/actions/drag bindings, screenshots of menus, configurations,
  tile modes and floating/Zen states; visually inspect; commit and push.
- Web: same Rust state, DOM interactions and touch, screenshots/geometry, static
  package tests; visually inspect; commit and push.
- Android: same Rust state, Compose interactions including touch, emulator
  screenshots, instrumentation tests/lint, ARM64 and x86_64 APK; visually
  inspect; commit and push.
- Final audit: every row has direct test or inspected runtime evidence on all
  three platforms; generated output stays ignored; docs describe only the
  implemented design.

## Working decisions

- Keep one panel registry. A hidden panel has no dock/floating placement;
  deletion removes a toolbar from the registry. Built-in panels cannot be deleted.
- Keep floating tab groups alongside dock bands, sharing group IDs, content,
  tab selection, tile allocation and move operations. Floating groups cannot
  contain docking splits. Geometry and hit targets are resolved in Rust.
- Workspace history stores lightweight workspace values, not document pixels.
  A continuous resize/move gesture is one undo step. Cancel restores its start.
  Restoring a saved workspace starts a new history; transient menus/drawers and
  search drafts are not history entries.
- Hosts may supply measured text/content extents, but Rust decides tab growth,
  automatic floating height, placement, constraints and all action semantics.
- Current interaction contract: tear-off beyond 80px from the source; 80px snap
  reach with 40px individual-panel targets, leaving room to dock beside a whole
  sidebar. Top/bottom screen targets use 40px for outside the sidebars and the
  next 40px for inside. No center rectangle. Empty title space and the complete
  trailing grip strip move groups; singleton tabs move whole groups.
- All eight floating resize hit regions sit outside the border. Drag-area
  double-click first restores a custom size. At default size it toggles Hide tab
  for lone panels, or cycles compact/vertical/horizontal layouts for toolbars.
  Tile-size changes refit the active toolbar preset. Multi-tab groups only reset.
- One Rust 80px constant governs tear-off, snap reach, Zen edge reveal and the
  keep-visible margin. Float dragging does not reveal hidden docks until an
  occupied screen edge is reached; revelation lasts through that drag. Every
  drop returns to ordinary cursor proximity, with no post-drop pin. While hidden,
  docks and screen edges cannot capture a drop; only other floating tab groups
  remain targets. Floating groups never accept side-by-side split drops.
- The user-approved GTK behavior is now the reference for both ports. The
  current contract supersedes the original goal wherever later feedback differs:
  no central float rectangle; 80/40px snapping; top targets below the app header;
  only standalone toolbars in horizontal dock bands; style-aware ribbon refits;
  recursive column reclaim/minima; singleton floating toolbar reset; named menu
  entries; and independent Hide tab with a rotated bottom grip/configure action.
- Hide tab is separate from name/icon style. A single panel hides its tab on
  tear-off, restores the original flag on docking alone during that drag, and
  clears it when merging. Multi-tab groups show tabs. See the full contract for
  already-floating moves, undo and explicit visibility changes.

## Current evidence

- Shared core: 111 tests cover the workspace model, transactional history,
  eight-edge geometry, measured sizing, tear-off, snapping and Zen rules. The
  interaction tests run against GTK, web and Android platform configurations.
- GTK `native_workspace_management`: passed with dark/light PNGs covering menus,
  duplicate/rename/delete dialogs, tile modes, floating groups and configuration.
- GTK `native_floating_gestures`: passed for all eight resize targets, permitted
  panel edge drops (no RefCell panic), title reset interpolation, whole-sidebar
  snap lines and Zen drag/drop behavior. `native_toolbar_sizing` covers all four
  edges, three tile sizes, grip reset, style refit and lone-toolbar group collapse.
- GTK `native_hidden_tabs`: passed for the independent name/icon/hide state,
  bottom grip dragging, its Configure action and the drawer, restoring an icon
  tab and dark/light screenshots. `native_column_removal` confirms single-row
  reclaim versus multiple-row fill on both edges, with workspace undo.
- GTK drawing/control, customization, and stacked-divider regressions pass.
  Native submenu screenshots cover both Add built-in panel and Add Toolbar.
- A genuine pointer-event regression through isolated Mutter reproduced the
  toolbar snap-back that signal-only tests missed. Workspace drag input now
  belongs to the stable surface controller, so unparenting the pressed tab
  cannot cancel the sequence. Group widgets and dividers also reconcile
  independently. Both the standalone ribbon and multi-tab tear-off pass real
  pointer delivery. Temporary logging is removed; regressions remain in
  `bench/native-input.js --workspace-drag` (`LAYER_NATIVE_DRAG_TAB=1` for a tab).
- `bench/native-input.js --workspace-clicks` reproduces the missed first
  double-click after a real resize. Resetting source click recognizers at drag
  start fixes the stale denied sequence (the stable controller consumes its
  release). Real pointer resize → first-double-click reset → panel tab toggles
  and all three toolbar layouts now pass, without temporary logging.
- GTK `native_zen_floating_targets` verifies no hidden bottom/top/sidebar snap,
  while a nearby float still accepts a tab merge even beside an inactive edge.
  `native_toolbar_sizing` captures all three layouts in all tile styles/themes
  and checks that changing tile size preserves the active preset.
- Review PNGs: `artifacts/ui/workspace-management/gtk/` (ignored). Inspected
  top-edge line below the app header, size-reset animation, dark/light labeled
  toolbars, configuration drawers, hidden-tab menus/grips, column collapse and
  the three default floating grids after another tab is removed.
- Web `--package --workspace`: passed real mouse/touch tear-off and continued
  movement after DOM reconciliation, all eight external resize handles, first
  double-click reset, panel tab toggling, all three toolbar layouts in all tile
  styles/themes, preset-preserving style changes, toolbar-only group collapse,
  narrow-ribbon merges, stacked dividers, top snap below the header, menus,
  prompts, validation, hide/show, workspace undo/redo and its keyboard shortcut.
  Zen tests cover hidden-edge rejection, floating-only tab merging, edge reveal
  latching through a drag, and ordinary proximity after release. External resize
  strips suppress browser text selection so native selection cannot steal a drag.
- Web `--package --customization`: passed picker/cancel/restore, real context
  clicks and touch long-press, native tile DND and touch reorder, clipped/wrapped
  toolbars, live controls, two-column/tab animations and Zen drawer dismissal.
- Refreshed `native_web_parity_reference` and web `--package --parity`: passed
  measured control geometry within 1px, 11pt text, shared icons, input/selection
  behavior, default spacing and dark/light panel/shadow pixels within 3/255.
  Inspected web review PNGs: `artifacts/ui/workspace-management/web/` (ignored),
  including menus, prompts, configuration, all floating tile styles, hidden tabs,
  Zen floating merges and the narrow-ribbon/top-edge indicators.
- Final web gates passed: preferences, injected GPU startup/failure cases,
  static PWA install/offline/update/scope tests, and the complete GPU drawing/input
  smoke run with the workspace/customization suites. Legacy panel HTML-DND tests
  and routing were replaced by the shared pointer suite; native HTML-DND remains
  only for tiles. Added direct side-width preservation and manual-shrink/overflow
  tab-append checks. Narrow GPU help keeps 11pt text and fits at 900×760 using
  tighter responsive spacing. Launcher/package tests, 111 core tests, Wasm
  strict clippy and formatting pass. Generated builds and review PNGs stay ignored.
- Initial GTK milestones `a7e0f9a` and `8614af3` are pushed. Android presentation
  is still pending; shared-core coverage does not count as frontend validation.
  Continue the Android port after the web platform commit/push gate.
