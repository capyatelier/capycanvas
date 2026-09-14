# Stacked collapsed columns

This replaces the former Group panel presentation. GTK, Web, Android, Windows, macOS and iPadOS implement this presentation;
the workspace model, drop validation, geometry and history are shared Rust.

A stack contains one or more collapsed columns. Each member retains its dock
tree, tab groups, selected tabs, split ratios and expanded width. Each member
has an immediate drag handle. Dropping that handle on another member stacks
the columns; dropping beside a stack or at a side edge unstacks the member.
Tile bodies retain the application-wide hold-before-drag convention.

Only top-level side columns can collapse. Columns inside vertically stacked
groups remain expanded, including when their parent column opens. Side-by-side
top-level columns can collapse independently; a stack's members are the only
collapsed children of its root. Saved nested collapsed columns and nested stacks
reopen as ordinary groups, preserving their panels, selected tabs and split tree.

Dropping an individual panel, a whole toolbar, or a tab group into a
member's empty lower area or footer grip creates a new collapsed column after
that member. The gap between members is also an insertion target. The new
column contains only the dropped content; a whole group retains its tab order,
active tab and tab style, and a toolbar retains its tools. Stack preferences
apply to the new member. Icon bodies still insert tabs and group dividers still
insert groups within a member. A 12 px target centered on the bottom edge of
the last tile appends a new group inside that member, with a blue insertion
line at the tile boundary. It includes the trailing gap above the footer grip,
so compact members earlier in a stack have the same target as its last member.
It only appears when the last group is scrolled into view and excludes the
grip itself. The grip and spacing between members continue creating stack
members. All hosts use the shared target geometry.

“Open individual panels” and Auto-hide belong to the stack and both default to
disabled for a new stack. Explicit saved preferences are retained. Enabling
“Open individual panels” opens the selected tab group in a compact popover.
With it disabled, a tile selects its tab and opens that member's complete
ordinary column toward the canvas. One member can be
open per stack; choosing another member switches the open column. Clicking an
already selected tile in the open member closes it.

Stacked members have the standard 6 px workspace gap between them. No separator
is drawn above the first group; separators only appear between groups within
a member. Top padding retains the drop target for inserting a first group.

An open member uses the full available workspace height, aligned with the
stack’s top and bottom. It keeps the normal outer workspace inset, without
additional vertical padding. Horizontal spacing separates it from the stack.
Its normal tab groups and split dividers are rendered by the ordinary dock views. The sidebar highlights the active tile of every visible
tab group in the open column. Drawer-style connectors join those active tiles
to the expanded column. Detailed visual refinements follow this first version.

The double-caret Expand control is removed. A replacement mechanism is deferred.
Escape closes open columns. Auto-hide consumes the outside canvas contact and
respects popups, nested drawers and active gestures. Open/close is transient;
membership, preferences, widths and ordinary split ratios are saved. Completed
stacking and resizing gestures each produce one workspace undo step, with
cancellation restoring the previous state.

A closed stack containing multiple columns has a fixed sidebar width. Its
outer divider has no resize affordance; dragging it, nudging it with the
keyboard or resetting its width cannot expand the aggregate tree. Open a member
with a tile, then resize its canvas-facing edge. That changes only the open
member's remembered width and stops at its panels' minimum width, preserving
the stack, its other members and the existing undo/cancel behavior.

Old Group panel settings migrate to “Open individual panels” disabled. The
custom all-tabs renderer, per-panel height weights and dedicated resize actions
are retired.

The shared model does not serialize an open member. Photo has a permanently
expanded far-right column: Color / Diagnostics at the top, Properties / Filters
in the middle, and Layers at the bottom. Its collapsed strip immediately to the
left contains Tool Set, Tool / Brush size, then Navigator, with Auto-hide and
Open individual panels disabled. The strip starts closed on load, preview and
reset. Paint retains its original expanded left panels and initially open
right stack. Untouched older Photo defaults migrate to the new arrangement;
untouched Paint defaults using it return to Paint's original arrangement.
Customized workspaces retain their layouts until Restore Starting Layout is
chosen. Other saved stacks start closed.

Apple uses the ordinary SwiftUI dock groups, dividers and existing drawer
connector shape for full-column opening. Column grips expose the shared stack
preferences. Closed aggregate dividers have no native resize source. The former
Apple drawer fallback and shared platform opt-in gates are removed.

Run the Apple native input and actual-file persistence checks with:

```sh
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/column-stacks.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/column-stack-persistence.swift
```

The AppKit input fixture uses real mouse/tablet events on both Apple presets.
It checks immediate grips/tabs, held icons, member and trailing-group drops,
switching, fixed closed widths, resizing, focus cancellation and exact Undo/Redo.
The persistence fixture switches and restarts actual temporary workspace
libraries, retaining membership, preferences, width, working tools and history
while discarding transient opening. Native `testColumnStacks` and
`testColumnStacksDark` exercise the editor on each host. Physical Pencil and
complete visual/performance acceptance remain separate checks.

Validate the additional GTK targets with
`bash tools/performance/workspace-motion.sh gtk --native-test=native_stack_member_drop_input`.
This covers mouse/touch panel tabs, floating group handles, toolbar handles,
held collapsed icons and drawer tabs, singleton and multi-member targets,
insertion previews, cancellation, opening and one-step undo/redo in both themes.
Run `bash tools/performance/workspace-motion.sh gtk --native-test=native_column_group_append_input`
for appending groups before the fixed grips of the first and middle members,
including the divider created by the drop.
Physical pen verification remains a manual check; the native pickup recognizers
and their device rules are unchanged.

Validate the Web port with
`bash tools/performance/workspace-motion.sh web --column-stacks` on its isolated
Wayland display, or `node apps/layer-web/test.mjs --headless --column-stacks`
against a development server on hosts with working headless WebGPU. Chrome delivers mouse, touch and pen contacts
through the production recognizers. It covers full-height opening and switching,
active connectors, fixed closed-stack width, stack preferences, panel/group/toolbar
and held-icon drops, the trailing append target, cancellation and undo/redo.
The existing `--column-drops` suite checks adjacent tile and divider boundaries.

On GTK, Web and Android, dropping a panel, toolbar, tab group or column in the menu/title bar
prepends it to the side column directly below the pointer. An expanded column
receives new groups; a collapsed stack receives a new first member. A column
inserted among expanded groups keeps its tree and opens its contents. No panel
group reserves the upper portion of its body for a split insertion. Instead,
the upper 20% of each group's body extends its tab-strip target: the horizontal
pointer position chooses a tab insertion slot, with the preview line in the
tab strip. This applies to first and lower groups, floating groups and drawers.

Dropping into a GTK, Web or Android panel group's body prepends the incoming tabs and selects
the incoming content. A translucent filled rectangle with an outline covers
the target's content area. Existing tabs stay in the group. Tab strips continue
to offer precise insertion positions with a line preview.

Run `bash tools/performance/workspace-motion.sh gtk --native-test=native_layout_drop_input`
for mouse/touch menu-bar and body drops on both sides and themes, including
previews, cancellation, selected content and one-step undo/redo. Captures are
written to `LAYER_TEST_ARTIFACTS` when supplied. Physical pen review remains
separate; these changes do not alter native pickup timing or device arbitration.

The history comparison includes collapsed member boundaries. Moving the last
member into the preceding column can preserve the panel order while changing
the grouping; that remains a real move with one undo step.

Android device coverage is in `AndroidInteractionTest#stackedColumnsOpenAndResizeOrdinaryGroups`
and `AndroidInteractionTest#stackedColumnDropsKeepFooterAppendTargets`. These use
an isolated workspace store and typed mouse/finger/stylus events through native
views. They cover full-height opening, resizing, Back, auto-hide, compact drawers,
active connectors, first/middle member append targets, stack targets, drawer
tabs, held icons, cancellation and one-step undo/redo. Physical pen accuracy and
hover remain separate hardware checks.

`AndroidTitleBarTest#photoDefaultColumnsAndPaintRestorationSurviveRestart` checks
Photo's primary panel geometry, secondary icon order, both themes, tab selection,
mouse/finger/stylus opening, preview/Cancel/Restore and restart on
the Wacom tablet. It also restores and checks Paint's original columns. The
2026-09-13 run passed in 30.77 seconds; captures and logs are retained under
ignored `artifacts/android/photo-default-2026-09-13/`. Shared UI, workspace and
host tests (480 passed, one hardware-GPU test ignored) and Android debug
builds/lint also pass. Old-baseline menu availability and latest-default
restoration are covered by shared controller tests. The corrected Paint and Photo
layouts were also restored through the native menus in the tablet app.

Validation on the Wacom MovinkPad 14 used a separate test application ID and
isolated workspace stores. The production app and its data were retained.
Native screenshots are written to the test package’s `files/validation` directory.

Validate the Windows port serially on an unlocked desktop with
`./apps/layer-windows/scripts/exercise-column-stacks.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe -Device mouse`,
then repeat with `-Device pen` and `-Device touch`. Each run owns an isolated
profile and uses guarded OS input through the production pickup recognizers.
The fixture checks shared and arranged native group/connector bounds, selected
tiles, immediate handles and held icons, member insertion/switching, resize
cancellation and retained tabs, one-step history, individual drawers, consumed
auto-hide contacts, both themes, Zen and restart persistence. The pointer moves
off the strip before the Zen assertion because hovering visible chrome reveals
it by design. OS-injected pen/touch coverage does not establish physical device
or 120 Hz painting acceptance.
