# Workspace management implementation

This checklist tracks the full requested GTK → web → Android rollout. It is
not a reduced MVP. The interaction contract lives in `panel-customization.md`
and will be updated with the implementation. No renderer changes are required.

| # | Required behavior | Core | GTK | Web | Android |
| --- | --- | --- | --- | --- | --- |
| 1 | Workspace menu: built-in-panel visibility, separate toolbar visibility section | Verified | Verified | Pending | Pending |
| 2 | Workspace menu: New Toolbar | Verified | Verified | Pending | Pending |
| 3 | Group menu: Add built-in panel submenu, checked membership, move existing panel | Verified | Verified | Pending | Pending |
| 4 | Group menu: Add Toolbar submenu, move existing toolbar | Verified | Verified | Pending | Pending |
| 5 | Configure entries use the actual panel/toolbar name | Verified | Verified | Pending | Pending |
| 6 | Standalone toolbar: Rename includes the current name (revised copy) | Verified | Verified | Pending | Pending |
| 7 | Duplicate toolbar: editable unique suggested name; names unique with built-in panels | Verified | Verified | Pending | Pending |
| 8 | Hide named panel/toolbar; retain configuration | Verified | Verified | Pending | Pending |
| 9 | Delete Toolbar confirmation, distinct from hiding; workspace undo guidance | Verified | Verified | Pending | Pending |
| 10 | Independent workspace undo/redo for every durable workspace edit; shortcuts | Verified | Verified | Pending | Pending |
| 11 | Small 1×1, large 2×2, labeled 3×2 tiles; correct wrapping/resize/drop slots | Verified | Verified | Pending | Pending |
| 12 | Toolbar configuration column contains management/display options; tab context stays concise | Verified | Verified | Pending | Pending |
| 13 | First tool supplies toolbar tab icon | Verified | Verified | Pending | Pending |
| 14 | Narrow vertical ribbons retain a central tab-merge drop zone | Verified | Verified | Pending | Pending |
| 15 | Adding tabs grows group to fit tab names/icons; later manual shrink allowed | Verified | Verified | Pending | Pending |
| 16 | Floating groups/toolbars: live tear-off/move, eight external resize edges, snap zones, Zen visibility, natural sizing and animated reset | Verified | Verified | Pending | Pending |

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
- All eight floating resize hit regions sit outside the border. Double-clicking
  empty floating title space resets intrinsic size with a brief animation.
- One Rust 80px constant governs tear-off, snap reach, Zen edge reveal and the
  keep-visible margin. Float dragging does not reveal hidden docks until an
  occupied screen edge is reached; revelation lasts through that drag. Every
  drop returns to ordinary cursor proximity, with no post-drop pin.
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

- Shared core: 108 tests cover the workspace model, transactional history,
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
- Review PNGs: `artifacts/ui/workspace-management/gtk/` (ignored). Inspected
  top-edge line below the app header, size-reset animation, dark/light labeled
  toolbars, configuration drawers, hidden-tab menus/grips, column collapse and
  the three default floating grids after another tab is removed.
- Web/Android presentation has not yet been ported to these new models. Do not
  treat shared-core coverage as frontend validation. GTK milestone is ready;
  web is next, followed by Android, with commit/push at each platform gate.
