# Stacked collapsed columns

[Workspace and UI](README.md) · [Panel customization](panel-customization.md) · [Drag convention](drag-and-reorder.md)

Every host presents collapsed side columns as stacks; the workspace model, drop
validation, geometry and history are shared Rust.

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
to the expanded column.

There is no Expand button. Escape closes open columns. Auto-hide consumes the outside canvas contact and
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

The shared model does not serialize an open member; saved stacks start closed,
except that Paint opens its right stack on load. Photo and Paint's stacks are
described in [default workspaces](default-workspaces.md#arrangements).

Apple uses the ordinary SwiftUI dock groups, dividers and drawer connector shape
for full-column opening. Column grips expose the shared stack preferences, and
closed aggregate dividers have no native resize source.

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

The history comparison includes collapsed member boundaries. Moving the last
member into the preceding column can preserve the panel order while changing
the grouping; that remains a real move with one undo step.

## Checks

```sh
bash tools/performance/workspace-motion.sh gtk --native-test=native_stack_member_drop_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_column_group_append_input
bash tools/performance/workspace-motion.sh gtk --native-test=native_layout_drop_input
bash tools/performance/workspace-motion.sh web --column-stacks
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/column-stacks.swift
bash apps/layer-apple/scripts/test-project-files.sh apps/layer-apple/tests/column-stack-persistence.swift
./apps/layer-windows/scripts/exercise-column-stacks.ps1 -Executable artifacts/windows/Release/CapyCanvas.exe -Device mouse
```

Repeat the Windows run with `-Device pen` and `-Device touch`, serially on an
unlocked desktop; the pointer leaves the strip before the Zen assertion because
hovering visible chrome reveals it. `node apps/layer-web/test.mjs --headless --column-stacks`
runs against a development server where headless WebGPU works, and
`--column-drops` checks adjacent tile and divider boundaries. Android runs
`AndroidInteractionTest#stackedColumnsOpenAndResizeOrdinaryGroups` and
`#stackedColumnDropsKeepFooterAppendTargets`; Apple runs `testColumnStacks` and
`testColumnStacksDark`. Injected pen and touch do not establish physical-device
acceptance.
