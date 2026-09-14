# Stacked collapsed columns

This replaces the former Group panel presentation. GTK, Web, Android and Windows implement this presentation;
the workspace model, drop validation, geometry and history are shared Rust.

A stack contains one or more collapsed columns. Each member retains its dock
tree, tab groups, selected tabs, split ratios and expanded width. Each member
has an immediate drag handle. Dropping that handle on another member stacks
the columns; dropping beside a stack or at a side edge unstacks the member.
Tile bodies retain the application-wide hold-before-drag convention.

On GTK, Web, Android and Windows, dropping an individual panel, a whole toolbar, or a tab group into a
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
members. All four hosts use the shared target geometry.

“Open individual panels” and Auto-hide belong to the stack. Enabling
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

Other hosts retain stack membership and preferences and use ordinary tabbed
drawers until full-column opening is ported. The shared model does not serialize
an open member. The unchanged Paint default on GTK, Web, Android and Windows opens its right stack when loaded,
previewed or reset, with Auto-hide and Open individual panels disabled. Other
saved arrangements start with their stacks closed.

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
