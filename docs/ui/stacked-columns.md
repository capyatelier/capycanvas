# Stacked collapsed columns

This replaces the former Group panel presentation. GTK is the first host;
the workspace model, drop validation, geometry and history are shared Rust.

A stack contains one or more collapsed columns. Each member retains its dock
tree, tab groups, selected tabs, split ratios and expanded width. Each member
has an immediate drag handle. Dropping that handle on another member stacks
the columns; dropping beside a stack or at a side edge unstacks the member.
Tile bodies retain the application-wide hold-before-drag convention.

Drawers and Auto-hide belong to the stack. Drawers enabled retains the existing
tabbed drawer behavior. With Drawers disabled, a tile selects its tab and opens
that member's complete ordinary column toward the canvas. One member can be
open per stack; choosing another member switches the open column. Clicking an
already selected tile in the open member closes it.

An open member uses the full available workspace height, with workspace spacing
on all four sides. Its normal tab groups and split dividers are rendered by the
ordinary dock views. The sidebar highlights the active tile of every visible
tab group in the open column. Drawer-style connectors join those active tiles
to the expanded column. Detailed visual refinements follow this first version.

The double-caret Expand control is removed. A replacement mechanism is deferred.
Escape closes open columns. Auto-hide consumes the outside canvas contact and
respects popups, nested drawers and active gestures. Open/close is transient;
membership, preferences, widths and ordinary split ratios are saved. Completed
stacking and resizing gestures each produce one workspace undo step, with
cancellation restoring the previous state.

Old Group panel settings migrate to Drawers disabled. Its custom all-tabs
renderer, per-panel height weights and dedicated resize actions are retired.

Other hosts retain stack membership and preferences and use ordinary tabbed
drawers until full-column opening is ported. The shared model does not serialize
an open member, so reopening a saved workspace starts with its stacks closed.
